//! Durable G9 context selections (TASK-0170, ADR-0042).
//!
//! Write discipline: only admitted (plan, items, trace) triples are
//! persisted, atomically, in one SQLite transaction. Saves are
//! idempotent for byte-identical retries; a conflicting payload under
//! an existing plan id refuses. Reads never invent: unknown ids load
//! as `None`, and stored values that fail domain reconstruction
//! surface as backend failures, never repaired silently.
//!
//! Integer note: SQLite stores `INTEGER` as signed 64-bit; domain
//! counters are `u64` and always small in practice (millis, sequence
//! numbers, token counts). `CHECK (... >= 0)` guards every counter
//! column, and casts run through `u64_to_i64` / `i64_to_u64` so a
//! far-future overflow fails loudly instead of wrapping.

use context_engine::model::{
    CacheBoundary, CacheClass, ContentHash, ContextError, ContextItem, ContextItemId, ContextKind,
    ContextPlan, ContextPlanId, CounterMethod, DecisionOutcome, Freshness, ProvenanceEnvelope,
    ReasonCode, ReopenRef, RetrievalTrace, Sensitivity, SourceRef, Supersession, TraceDecision,
    Trust, TrustLevel,
};
use context_engine::port::ContextStorePort;
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use crate::task_store::SqliteStore;

fn u64_to_i64(field: &str, value: u64) -> Result<i64, ContextError> {
    i64::try_from(value)
        .map_err(|_| ContextError::InvalidInput(format!("{field} exceeds i64 range")))
}

fn i64_to_u64(field: &str, value: i64) -> Result<u64, ContextError> {
    u64::try_from(value).map_err(|_| ContextError::BackendFailure {
        detail: format!("stored {field} is negative"),
    })
}

fn sqlite(error: rusqlite::Error) -> ContextError {
    ContextError::BackendFailure {
        detail: error.to_string(),
    }
}

fn reconstruct<T>(parsed: Result<T, ContextError>, what: &str) -> Result<T, ContextError> {
    parsed.map_err(|error| ContextError::BackendFailure {
        detail: format!("stored {what} failed domain reconstruction: {error}"),
    })
}

/// Encoded source payload: (kind, p1, p2, n1, span_start, span_end).
type EncodedSource = (String, String, String, i64, Option<i64>, Option<i64>);

fn encode_source(source: &SourceRef) -> Result<EncodedSource, ContextError> {
    let encoded = match source {
        SourceRef::Task { task_id, event_seq } => (
            "TASK".to_owned(),
            task_id.clone(),
            String::new(),
            u64_to_i64("event seq", *event_seq)?,
            None,
            None,
        ),
        SourceRef::RepoPath { rev, path, span } => {
            let (start, end) = match span {
                Some((s, e)) => (
                    Some(u64_to_i64("span start", *s)?),
                    Some(u64_to_i64("span end", *e)?),
                ),
                None => (None, None),
            };
            (
                "REPO_PATH".to_owned(),
                rev.clone(),
                path.clone(),
                0,
                start,
                end,
            )
        }
        SourceRef::Plan {
            branch_id,
            revision_seq,
        } => (
            "PLAN".to_owned(),
            branch_id.clone(),
            String::new(),
            u64_to_i64("revision seq", *revision_seq)?,
            None,
            None,
        ),
        SourceRef::Attempt {
            execution_id,
            attempt_seq,
        } => (
            "ATTEMPT".to_owned(),
            execution_id.clone(),
            String::new(),
            u64_to_i64("attempt seq", *attempt_seq)?,
            None,
            None,
        ),
        SourceRef::Decision { request_id } => (
            "DECISION".to_owned(),
            request_id.clone(),
            String::new(),
            0,
            None,
            None,
        ),
        SourceRef::Event { seq } => (
            "EVENT".to_owned(),
            String::new(),
            String::new(),
            u64_to_i64("event seq", *seq)?,
            None,
            None,
        ),
        SourceRef::Memory { opaque } => (
            "MEMORY".to_owned(),
            opaque.clone(),
            String::new(),
            0,
            None,
            None,
        ),
        SourceRef::Observation { handle } => (
            "OBSERVATION".to_owned(),
            handle.clone(),
            String::new(),
            0,
            None,
            None,
        ),
    };
    Ok(encoded)
}

fn decode_source(
    kind: &str,
    p1: &str,
    p2: &str,
    n1: i64,
    span_start: Option<i64>,
    span_end: Option<i64>,
) -> Result<SourceRef, ContextError> {
    let seq = i64_to_u64("source counter", n1)?;
    let span = match (span_start, span_end) {
        (Some(s), Some(e)) => Some((i64_to_u64("span start", s)?, i64_to_u64("span end", e)?)),
        (None, None) => None,
        _ => {
            return Err(ContextError::BackendFailure {
                detail: "stored span is half-present".to_owned(),
            });
        }
    };
    let parsed = match kind {
        "TASK" => SourceRef::task(p1, seq),
        "REPO_PATH" => SourceRef::repo_path(p1, p2, span),
        "PLAN" => SourceRef::plan(p1, seq),
        "ATTEMPT" => SourceRef::attempt(p1, seq),
        "DECISION" => SourceRef::decision(p1),
        "EVENT" => Ok(SourceRef::event(seq)),
        "MEMORY" => SourceRef::memory(p1),
        "OBSERVATION" => SourceRef::observation(p1),
        other => {
            return Err(ContextError::BackendFailure {
                detail: format!("stored source kind is unknown: {other}"),
            });
        }
    };
    reconstruct(parsed, "source ref")
}

impl SqliteStore {
    fn insert_selection(
        &mut self,
        plan: &ContextPlan,
        items: &[ContextItem],
        trace: &RetrievalTrace,
    ) -> Result<(), ContextError> {
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite)?;
        transaction
            .execute(
                "INSERT INTO context_plans
                 (plan_id, revision_seq, budget_soft, budget_hard, counter_method,
                  renderer, policy_fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    plan.id.as_str(),
                    u64_to_i64("revision seq", plan.revision_seq)?,
                    u64_to_i64("soft budget", plan.budget.soft_tokens)?,
                    u64_to_i64("hard budget", plan.budget.hard_tokens)?,
                    plan.budget.counter.as_str(),
                    plan.renderer,
                    plan.policy_fingerprint,
                ],
            )
            .map_err(sqlite)?;
        for (position, hash) in plan.source_hashes.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO context_plan_hashes (plan_id, position, content_hash)
                     VALUES (?1, ?2, ?3)",
                    params![plan.id.as_str(), position as i64, hash.as_str()],
                )
                .map_err(sqlite)?;
        }
        for (position, boundary) in plan.cache_boundaries.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO context_cache_boundaries (plan_id, position, cache_class)
                     VALUES (?1, ?2, ?3)",
                    params![plan.id.as_str(), position as i64, boundary.class.as_str()],
                )
                .map_err(sqlite)?;
        }
        let mut sections = std::collections::HashMap::new();
        for id in &plan.required {
            sections.insert(id.as_str(), ("REQUIRED", None));
        }
        for id in &plan.selected {
            sections.insert(id.as_str(), ("SELECTED", None));
        }
        for (id, reason) in &plan.deferred {
            sections.insert(id.as_str(), ("DEFERRED", Some(reason.as_str())));
        }
        for (id, reason) in &plan.rejected {
            sections.insert(id.as_str(), ("REJECTED", Some(reason.as_str())));
        }
        for item in items {
            insert_item(&transaction, plan.id.as_str(), item, &sections)?;
        }
        // Boundary membership rows reference items: they must land
        // after the items themselves (FK order).
        for (position, boundary) in plan.cache_boundaries.iter().enumerate() {
            for (item_position, item_id) in boundary.items.iter().enumerate() {
                transaction
                    .execute(
                        "INSERT INTO context_cache_boundary_items
                         (plan_id, boundary_position, item_position, item_id)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            plan.id.as_str(),
                            position as i64,
                            item_position as i64,
                            item_id.as_str()
                        ],
                    )
                    .map_err(sqlite)?;
            }
        }
        transaction
            .execute(
                "INSERT INTO context_traces (plan_id) VALUES (?1)",
                params![plan.id.as_str()],
            )
            .map_err(sqlite)?;
        for decision in &trace.decisions {
            transaction
                .execute(
                    "INSERT INTO context_trace_decisions
                     (plan_id, item_id, outcome, reason_code, rule)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        plan.id.as_str(),
                        decision.item_id.as_str(),
                        decision.outcome.as_str(),
                        decision.reason.map(|reason| reason.as_str()),
                        decision.rule,
                    ],
                )
                .map_err(sqlite)?;
        }
        for (position, rev) in trace.source_revs.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO context_trace_revs (plan_id, position, source_rev)
                     VALUES (?1, ?2, ?3)",
                    params![plan.id.as_str(), position as i64, rev],
                )
                .map_err(sqlite)?;
        }
        transaction.commit().map_err(sqlite)?;
        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
fn insert_item(
    transaction: &rusqlite::Transaction<'_>,
    plan_id: &str,
    item: &ContextItem,
    sections: &std::collections::HashMap<&str, (&str, Option<&str>)>,
) -> Result<(), ContextError> {
    let (section, reason) = sections.get(item.id.as_str()).copied().ok_or_else(|| {
        ContextError::InvalidInput(format!("item {} has no plan section", item.id.as_str()))
    })?;
    let (source_kind, p1, p2, n1, span_start, span_end) = encode_source(&item.source_ref)?;
    let (env_kind, env_p1, env_p2, env_n1, env_span_start, env_span_end) =
        encode_source(&item.provenance.source)?;
    let (reopen_handle, reopen_reason) = match &item.provenance.reopen {
        ReopenRef::Handle(handle) => (Some(handle.as_str()), None),
        ReopenRef::NotReopenable { reason } => (None, Some(reason.as_str())),
    };
    let (supersedes, supersession_reason) = match &item.provenance.supersession {
        Some(link) => (Some(link.supersedes.as_str()), Some(link.reason.as_str())),
        None => (None, None),
    };
    transaction
        .execute(
            "INSERT INTO context_items
             (item_id, plan_id, section, reason_code, kind,
              source_kind, source_p1, source_p2, source_n1,
              source_span_start, source_span_end, content_hash, priority,
              required, cache_class, freshness_observed_at, freshness_rev,
              freshness_stale_after, trust_level, trust_reason, sensitivity,
              env_source_rev, env_retrieved_at, env_selector,
              env_reopen_handle, env_reopen_unavailable_reason,
              env_permission_scope, env_confidence,
              env_supersedes, env_supersession_reason,
              env_source_kind, env_source_p1, env_source_p2, env_source_n1,
              env_source_span_start, env_source_span_end)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                     ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23,
                     ?24, ?25, ?26, ?27, ?28, ?29, ?30,
                     ?31, ?32, ?33, ?34, ?35, ?36)",
            params![
                item.id.as_str(),
                plan_id,
                section,
                reason,
                item.kind.as_str(),
                source_kind,
                p1,
                p2,
                n1,
                span_start,
                span_end,
                item.content_hash.as_str(),
                i64::from(item.priority),
                i64::from(item.required),
                item.cache_class.as_str(),
                u64_to_i64("observed at", item.freshness.observed_at)?,
                item.freshness.source_rev,
                item.freshness
                    .stale_after
                    .map(|v| u64_to_i64("stale after", v))
                    .transpose()?,
                item.trust.level.as_str(),
                item.trust.reason,
                item.sensitivity.as_str(),
                item.provenance.source_rev,
                u64_to_i64("retrieved at", item.provenance.retrieved_at)?,
                item.provenance.selector,
                reopen_handle,
                reopen_reason,
                item.provenance.permission_scope,
                i64::from(item.provenance.confidence),
                supersedes,
                supersession_reason,
                env_kind,
                env_p1,
                env_p2,
                env_n1,
                env_span_start,
                env_span_end,
            ],
        )
        .map_err(sqlite)?;
    for (position, step) in item.provenance.transform_chain.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO context_item_transforms (item_id, position, transform)
                 VALUES (?1, ?2, ?3)",
                params![item.id.as_str(), position as i64, step],
            )
            .map_err(sqlite)?;
    }
    Ok(())
}

/// One decoded `context_items` row in `decode_item` column order.
type ItemRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    String,
    i64,
    i64,
    String,
    i64,
    String,
    Option<i64>,
    String,
    String,
    String,
    String,
    i64,
    String,
    Option<String>,
    Option<String>,
    String,
    i64,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    i64,
    Option<i64>,
    Option<i64>,
);

#[allow(clippy::too_many_lines)]
fn decode_item(
    connection: &rusqlite::Connection,
    item_id: &str,
) -> Result<Option<ContextItem>, ContextError> {
    let row: Option<ItemRow> = connection
        .query_row(
            "SELECT item_id, kind, source_kind, source_p1, source_p2,
                    source_n1, source_span_start, source_span_end,
                    content_hash, priority, required, cache_class,
                    freshness_observed_at, freshness_rev, freshness_stale_after,
                    trust_level, trust_reason, sensitivity,
                    env_source_rev, env_retrieved_at, env_selector,
                    env_reopen_handle, env_reopen_unavailable_reason,
                    env_permission_scope, env_confidence,
                    env_supersedes, env_supersession_reason,
                    env_source_kind, env_source_p1, env_source_p2, env_source_n1,
                    env_source_span_start, env_source_span_end
             FROM context_items WHERE item_id = ?1",
            params![item_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                    row.get(16)?,
                    row.get(17)?,
                    row.get(18)?,
                    row.get(19)?,
                    row.get(20)?,
                    row.get(21)?,
                    row.get(22)?,
                    row.get(23)?,
                    row.get(24)?,
                    row.get(25)?,
                    row.get(26)?,
                    row.get(27)?,
                    row.get(28)?,
                    row.get(29)?,
                    row.get(30)?,
                    row.get(31)?,
                    row.get(32)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let (
        id,
        kind,
        source_kind,
        p1,
        p2,
        n1,
        span_start,
        span_end,
        hash,
        priority,
        required,
        cache_class,
        observed_at,
        freshness_rev,
        stale_after,
        trust_level,
        trust_reason,
        sensitivity,
        env_rev,
        retrieved_at,
        selector,
        reopen_handle,
        reopen_reason,
        scope,
        confidence,
        supersedes,
        supersession_reason,
        env_kind,
        env_p1,
        env_p2,
        env_n1,
        env_span_start,
        env_span_end,
    ) = row;
    let mut statement = connection
        .prepare(
            "SELECT transform FROM context_item_transforms WHERE item_id = ?1 ORDER BY position",
        )
        .map_err(sqlite)?;
    let transforms: Vec<String> = statement
        .query_map(params![item_id], |row| row.get(0))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    let reopen = match (reopen_handle, reopen_reason) {
        (Some(handle), None) => ReopenRef::Handle(handle),
        (None, Some(reason)) => ReopenRef::NotReopenable { reason },
        _ => {
            return Err(ContextError::BackendFailure {
                detail: format!("stored reopen ref for {item_id} is malformed"),
            });
        }
    };
    let supersession = match (supersedes, supersession_reason) {
        (Some(target), Some(reason)) => Some(Supersession {
            supersedes: reconstruct(ContextItemId::new(target), "supersedes id")?,
            reason,
        }),
        (None, None) => None,
        _ => {
            return Err(ContextError::BackendFailure {
                detail: format!("stored supersession for {item_id} is half-present"),
            });
        }
    };
    Ok(Some(ContextItem {
        id: reconstruct(ContextItemId::new(id), "item id")?,
        kind: reconstruct(ContextKind::parse(&kind), "kind")?,
        source_ref: decode_source(&source_kind, &p1, &p2, n1, span_start, span_end)?,
        content_hash: reconstruct(ContentHash::new(hash), "content hash")?,
        priority: u16::try_from(priority).map_err(|_| ContextError::BackendFailure {
            detail: "stored priority exceeds u16".to_owned(),
        })?,
        freshness: Freshness {
            observed_at: i64_to_u64("observed at", observed_at)?,
            source_rev: freshness_rev,
            stale_after: stale_after
                .map(|v| i64_to_u64("stale after", v))
                .transpose()?,
        },
        trust: Trust {
            level: reconstruct(TrustLevel::parse(&trust_level), "trust level")?,
            reason: trust_reason,
        },
        sensitivity: reconstruct(Sensitivity::parse(&sensitivity), "sensitivity")?,
        required: required != 0,
        cache_class: reconstruct(CacheClass::parse(&cache_class), "cache class")?,
        provenance: ProvenanceEnvelope {
            source: decode_source(
                &env_kind,
                &env_p1,
                &env_p2,
                env_n1,
                env_span_start,
                env_span_end,
            )?,
            source_rev: env_rev,
            retrieved_at: i64_to_u64("retrieved at", retrieved_at)?,
            selector,
            transform_chain: transforms,
            reopen,
            permission_scope: scope,
            confidence: u16::try_from(confidence).map_err(|_| ContextError::BackendFailure {
                detail: "stored confidence exceeds u16".to_owned(),
            })?,
            supersession,
        },
    }))
}

fn decode_plan(
    connection: &rusqlite::Connection,
    plan_id: &str,
) -> Result<Option<ContextPlan>, ContextError> {
    let row: Option<(i64, i64, i64, String, String, String)> = connection
        .query_row(
            "SELECT revision_seq, budget_soft, budget_hard, counter_method,
                    renderer, policy_fingerprint
             FROM context_plans WHERE plan_id = ?1",
            params![plan_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(sqlite)?;
    let Some((revision_seq, soft, hard, counter, renderer, fingerprint)) = row else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare(
            "SELECT item_id, section, reason_code FROM context_items
             WHERE plan_id = ?1 ORDER BY item_id",
        )
        .map_err(sqlite)?;
    let mut required = Vec::new();
    let mut selected = Vec::new();
    let mut deferred = Vec::new();
    let mut rejected = Vec::new();
    let sections: Vec<(String, String, Option<String>)> = statement
        .query_map(params![plan_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    for (id, section, reason) in sections {
        let id = reconstruct(ContextItemId::new(id), "section item id")?;
        match section.as_str() {
            "REQUIRED" => required.push(id),
            "SELECTED" => selected.push(id),
            "DEFERRED" => deferred.push((
                id,
                reconstruct(
                    ReasonCode::parse(&reason.unwrap_or_default()),
                    "deferred reason",
                )?,
            )),
            "REJECTED" => rejected.push((
                id,
                reconstruct(
                    ReasonCode::parse(&reason.unwrap_or_default()),
                    "rejected reason",
                )?,
            )),
            other => {
                return Err(ContextError::BackendFailure {
                    detail: format!("stored section is unknown: {other}"),
                });
            }
        }
    }
    let mut statement = connection
        .prepare(
            "SELECT content_hash FROM context_plan_hashes
             WHERE plan_id = ?1 ORDER BY position",
        )
        .map_err(sqlite)?;
    let hashes: Vec<String> = statement
        .query_map(params![plan_id], |row| row.get(0))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    let mut source_hashes = Vec::with_capacity(hashes.len());
    for hash in hashes {
        source_hashes.push(reconstruct(ContentHash::new(hash), "plan hash")?);
    }
    let mut statement = connection
        .prepare(
            "SELECT position, cache_class FROM context_cache_boundaries
             WHERE plan_id = ?1 ORDER BY position",
        )
        .map_err(sqlite)?;
    let boundaries: Vec<(i64, String)> = statement
        .query_map(params![plan_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    let mut cache_boundaries = Vec::with_capacity(boundaries.len());
    for (position, class) in boundaries {
        let mut statement = connection
            .prepare(
                "SELECT item_id FROM context_cache_boundary_items
                 WHERE plan_id = ?1 AND boundary_position = ?2 ORDER BY item_position",
            )
            .map_err(sqlite)?;
        let ids: Vec<String> = statement
            .query_map(params![plan_id, position], |row| row.get(0))
            .map_err(sqlite)?
            .collect::<Result<_, _>>()
            .map_err(sqlite)?;
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            items.push(reconstruct(ContextItemId::new(id), "boundary item id")?);
        }
        cache_boundaries.push(CacheBoundary {
            class: reconstruct(CacheClass::parse(&class), "boundary class")?,
            items,
        });
    }
    Ok(Some(ContextPlan {
        id: reconstruct(ContextPlanId::new(plan_id.to_owned()), "plan id")?,
        revision_seq: i64_to_u64("revision seq", revision_seq)?,
        required,
        selected,
        deferred,
        rejected,
        budget: context_engine::model::Budget {
            soft_tokens: i64_to_u64("soft budget", soft)?,
            hard_tokens: i64_to_u64("hard budget", hard)?,
            counter: reconstruct(CounterMethod::parse(&counter), "counter")?,
        },
        cache_boundaries,
        renderer,
        source_hashes,
        policy_fingerprint: fingerprint,
    }))
}

fn decode_trace(
    connection: &rusqlite::Connection,
    plan_id: &str,
    plan: &ContextPlan,
) -> Result<Option<RetrievalTrace>, ContextError> {
    let present: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM context_traces WHERE plan_id = ?1)",
            params![plan_id],
            |row| row.get(0),
        )
        .map_err(sqlite)?;
    if !present {
        return Ok(None);
    }
    let mut statement = connection
        .prepare(
            "SELECT item_id, outcome, reason_code, rule FROM context_trace_decisions
             WHERE plan_id = ?1 ORDER BY item_id",
        )
        .map_err(sqlite)?;
    let rows: Vec<(String, String, Option<String>, String)> = statement
        .query_map(params![plan_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    let mut decisions = Vec::with_capacity(rows.len());
    for (item_id, outcome, reason, rule) in rows {
        decisions.push(TraceDecision {
            item_id: reconstruct(ContextItemId::new(item_id), "decision item id")?,
            outcome: reconstruct(DecisionOutcome::parse(&outcome), "outcome")?,
            reason: reason
                .map(|code| reconstruct(ReasonCode::parse(&code), "decision reason"))
                .transpose()?,
            rule,
        });
    }
    let mut statement = connection
        .prepare(
            "SELECT source_rev FROM context_trace_revs
             WHERE plan_id = ?1 ORDER BY position",
        )
        .map_err(sqlite)?;
    let source_revs: Vec<String> = statement
        .query_map(params![plan_id], |row| row.get(0))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    Ok(Some(RetrievalTrace {
        plan_id: plan.id.clone(),
        decisions,
        budget: plan.budget.clone(),
        source_revs,
        renderer: plan.renderer.clone(),
        policy_fingerprint: plan.policy_fingerprint.clone(),
        // Single source of truth: the trace carries the plan's recorded
        // cache decisions (ADR-0042 section 7) without a divergent copy.
        cache_boundaries: plan.cache_boundaries.clone(),
    }))
}

fn load_items_for_plan(
    connection: &rusqlite::Connection,
    plan_id: &str,
) -> Result<Vec<ContextItem>, ContextError> {
    let mut statement = connection
        .prepare("SELECT item_id FROM context_items WHERE plan_id = ?1 ORDER BY item_id")
        .map_err(sqlite)?;
    let ids: Vec<String> = statement
        .query_map(params![plan_id], |row| row.get(0))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(item) = decode_item(connection, &id)? {
            items.push(item);
        }
    }
    Ok(items)
}

impl ContextStorePort for SqliteStore {
    fn save_selection(
        &mut self,
        plan: &ContextPlan,
        items: &[ContextItem],
        trace: &RetrievalTrace,
    ) -> Result<(), ContextError> {
        for item in items {
            item.validate()?;
        }
        if trace.plan_id != plan.id {
            return Err(ContextError::InvalidInput(
                "trace plan id differs from plan id".to_owned(),
            ));
        }
        if trace.policy_fingerprint != plan.policy_fingerprint {
            return Err(ContextError::InvalidInput(
                "trace policy fingerprint differs from plan policy fingerprint".to_owned(),
            ));
        }
        if trace.budget != plan.budget {
            return Err(ContextError::InvalidInput(
                "trace budget differs from plan budget".to_owned(),
            ));
        }
        if trace.renderer != plan.renderer {
            return Err(ContextError::InvalidInput(
                "trace renderer differs from plan renderer".to_owned(),
            ));
        }
        if trace.cache_boundaries != plan.cache_boundaries {
            return Err(ContextError::InvalidInput(
                "trace cache boundaries differ from plan cache boundaries".to_owned(),
            ));
        }
        let item_ids: std::collections::BTreeSet<&str> =
            items.iter().map(|item| item.id.as_str()).collect();
        // Boundaries segment exactly the rendered set: every
        // required/selected item sits in one, nothing else does.
        let mut bounded = std::collections::BTreeSet::new();
        for boundary in &plan.cache_boundaries {
            for id in &boundary.items {
                if !item_ids.contains(id.as_str()) {
                    return Err(ContextError::InvalidInput(format!(
                        "cache boundary references missing item {}",
                        id.as_str()
                    )));
                }
                bounded.insert(id.as_str());
            }
        }
        for id in plan.required.iter().chain(plan.selected.iter()) {
            if !bounded.contains(id.as_str()) {
                return Err(ContextError::InvalidInput(format!(
                    "rendered item {} sits in no cache boundary",
                    id.as_str()
                )));
            }
        }
        for id in plan
            .deferred
            .iter()
            .map(|(id, _)| id)
            .chain(plan.rejected.iter().map(|(id, _)| id))
        {
            if bounded.contains(id.as_str()) {
                return Err(ContextError::InvalidInput(format!(
                    "unrendered item {} sits in a cache boundary",
                    id.as_str()
                )));
            }
        }
        let mut sectioned = std::collections::BTreeSet::new();
        for id in plan
            .required
            .iter()
            .chain(plan.selected.iter())
            .chain(plan.deferred.iter().map(|(id, _)| id))
            .chain(plan.rejected.iter().map(|(id, _)| id))
        {
            if !item_ids.contains(id.as_str()) {
                return Err(ContextError::InvalidInput(format!(
                    "section references missing item {}",
                    id.as_str()
                )));
            }
            if !sectioned.insert(id.as_str()) {
                return Err(ContextError::InvalidInput(format!(
                    "item {} appears in two plan sections",
                    id.as_str()
                )));
            }
        }
        for id in &item_ids {
            if !sectioned.contains(id) {
                return Err(ContextError::InvalidInput(format!(
                    "item {id} has no plan section"
                )));
            }
        }
        let mut decided = std::collections::BTreeSet::new();
        for decision in &trace.decisions {
            if !item_ids.contains(decision.item_id.as_str()) {
                return Err(ContextError::InvalidInput(format!(
                    "decision references missing item {}",
                    decision.item_id.as_str()
                )));
            }
            if !decided.insert(decision.item_id.as_str()) {
                return Err(ContextError::InvalidInput(format!(
                    "duplicate decision for item {}",
                    decision.item_id.as_str()
                )));
            }
        }
        for id in &item_ids {
            if !decided.contains(id) {
                return Err(ContextError::InvalidInput(format!(
                    "item {id} has no traced decision"
                )));
            }
        }
        for item in items {
            let foreign: bool = self
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM context_items
                     WHERE item_id = ?1 AND plan_id <> ?2)",
                    params![item.id.as_str(), plan.id.as_str()],
                    |row| row.get(0),
                )
                .map_err(sqlite)?;
            if foreign {
                // Re-selection mints fresh ids (provenance differs by
                // construction); an id never spans two plans.
                return Err(ContextError::InvalidInput(format!(
                    "item {} is already selected under another plan",
                    item.id.as_str()
                )));
            }
            if let Some(link) = &item.provenance.supersession {
                // Supersession normally targets an earlier plan, so the
                // target must exist durable-side or inside this triple —
                // never nowhere.
                let target = link.supersedes.as_str();
                let known_in_triple = item_ids.contains(target);
                let known_durable: bool = self
                    .connection()
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM context_items WHERE item_id = ?1)",
                        params![target],
                        |row| row.get(0),
                    )
                    .map_err(sqlite)?;
                if !known_in_triple && !known_durable {
                    return Err(ContextError::InvalidInput(format!(
                        "item {} supersedes unknown item {target}",
                        item.id.as_str()
                    )));
                }
            }
        }
        if let Some(existing) = decode_plan(self.connection(), plan.id.as_str())? {
            let mut stored_items = load_items_for_plan(self.connection(), plan.id.as_str())?;
            stored_items.sort_by(|a, b| a.id.cmp(&b.id));
            let mut given_items = items.to_vec();
            given_items.sort_by(|a, b| a.id.cmp(&b.id));
            let stored_trace = decode_trace(self.connection(), plan.id.as_str(), &existing)?;
            if existing == *plan
                && stored_items == given_items
                && stored_trace.as_ref() == Some(trace)
            {
                return Ok(());
            }
            return Err(ContextError::InvalidInput(format!(
                "plan {} exists with different bytes",
                plan.id.as_str()
            )));
        }
        self.insert_selection(plan, items, trace)
    }

    fn load_plan(&self, id: &ContextPlanId) -> Result<Option<ContextPlan>, ContextError> {
        decode_plan(self.connection(), id.as_str())
    }

    fn load_item(&self, id: &ContextItemId) -> Result<Option<ContextItem>, ContextError> {
        decode_item(self.connection(), id.as_str())
    }

    fn load_trace(&self, plan_id: &ContextPlanId) -> Result<Option<RetrievalTrace>, ContextError> {
        let Some(plan) = decode_plan(self.connection(), plan_id.as_str())? else {
            return Ok(None);
        };
        decode_trace(self.connection(), plan_id.as_str(), &plan)
    }

    fn explain(
        &self,
        plan_id: &ContextPlanId,
        item_id: &ContextItemId,
    ) -> Result<Option<(ProvenanceEnvelope, TraceDecision)>, ContextError> {
        let belongs: bool = self
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM context_items WHERE plan_id = ?1 AND item_id = ?2)",
                params![plan_id.as_str(), item_id.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite)?;
        if !belongs {
            return Ok(None);
        }
        let Some(item) = decode_item(self.connection(), item_id.as_str())? else {
            return Ok(None);
        };
        let row: Option<(String, Option<String>, String)> = self
            .connection()
            .query_row(
                "SELECT outcome, reason_code, rule FROM context_trace_decisions
                 WHERE plan_id = ?1 AND item_id = ?2",
                params![plan_id.as_str(), item_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(sqlite)?;
        let Some((outcome, reason, rule)) = row else {
            return Ok(None);
        };
        Ok(Some((
            item.provenance,
            TraceDecision {
                item_id: item.id,
                outcome: reconstruct(DecisionOutcome::parse(&outcome), "outcome")?,
                reason: reason
                    .map(|code| reconstruct(ReasonCode::parse(&code), "decision reason"))
                    .transpose()?,
                rule,
            },
        )))
    }
}
