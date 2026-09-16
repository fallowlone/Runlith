//! Durable planning-input and planning-run control state (ADR-0028).

use plan_domain::{
    AffectedParentCriterion, PlanBranchId, PlanningInputSnapshot, PlanningInputSnapshotId,
    PlanningRunId, RiskClass, RiskEscalationGrant, ScopeBound,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use task_domain::TaskId;

use crate::error::{StorageError, StorageResult};
use crate::plan_guardrail_store::require_evidence_key;
use crate::plan_store::PlanRevisionLineage;
use crate::task_store::SqliteStore;

type StoredPlanningInputRow = (
    String,
    String,
    i64,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

type CurrentPlanningRunRow = (
    String,
    i64,
    String,
    String,
    i64,
    String,
    String,
    i64,
    Option<i64>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanningRunState {
    Active,
    Succeeded,
    Refused,
    Superseded,
    Failed,
}

impl PlanningRunState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Succeeded => "SUCCEEDED",
            Self::Refused => "REFUSED",
            Self::Superseded => "SUPERSEDED",
            Self::Failed => "FAILED",
        }
    }

    fn parse(value: &str) -> StorageResult<Self> {
        match value {
            "ACTIVE" => Ok(Self::Active),
            "SUCCEEDED" => Ok(Self::Succeeded),
            "REFUSED" => Ok(Self::Refused),
            "SUPERSEDED" => Ok(Self::Superseded),
            "FAILED" => Ok(Self::Failed),
            _ => Err(StorageError::Corrupt(format!(
                "unknown planning run state {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningRunRecord {
    pub id: PlanningRunId,
    pub plan_branch_id: PlanBranchId,
    pub generation: u64,
    pub snapshot_id: PlanningInputSnapshotId,
    pub input_fingerprint: String,
    pub expected_plan_head: u64,
    pub trigger_evidence_key: String,
    pub state: PlanningRunState,
    pub created_at_ms: u64,
    pub finished_at_ms: Option<u64>,
}

impl SqliteStore {
    /// Checks the trusted policy-derived portion of one immutable planning
    /// input snapshot. Triggering evidence is intentionally excluded because
    /// REPLAN has new control-plane evidence; all policy authority inputs must
    /// still match the snapshot the human decision was based on exactly.
    #[allow(clippy::too_many_arguments)]
    pub fn planning_input_policy_matches(
        &self,
        snapshot_id: &PlanningInputSnapshotId,
        task_ref: &str,
        scope_bound: &ScopeBound,
        baseline_risk: RiskClass,
        affected_parent_criteria: &[AffectedParentCriterion],
        lineage_fingerprints: &[String],
        risk_grant: Option<&RiskEscalationGrant>,
        policy_fingerprint: &str,
    ) -> StorageResult<bool> {
        type PolicyRow = (
            String,
            String,
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
        );
        let row: Option<PolicyRow> = self
            .connection()
            .query_row(
                "SELECT task_ref, baseline_risk, policy_fingerprint,
                        risk_grant_proposal_hash, risk_grant_expires_at_ms,
                        risk_grant_consumed
                 FROM planning_input_snapshots WHERE snapshot_id = ?1",
                params![snapshot_id.as_str()],
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
            .optional()?;
        let Some(row) = row else {
            return Ok(false);
        };

        let expected_grant = risk_grant
            .map(|grant| {
                Ok::<_, StorageError>((
                    Some(grant.proposal_hash.as_str()),
                    Some(sqlite_u64(grant.expires_at_ms, "risk grant expiry")?),
                    Some(if grant.consumed { 1_i64 } else { 0_i64 }),
                ))
            })
            .transpose()?
            .unwrap_or((None, None, None));
        if row.0 != task_ref
            || row.1 != risk_name(baseline_risk)
            || row.2 != policy_fingerprint
            || row.3.as_deref() != expected_grant.0
            || row.4 != expected_grant.1
            || row.5 != expected_grant.2
        {
            return Ok(false);
        }

        let mut scope_statement = self.connection().prepare(
            "SELECT scope_root FROM planning_input_scope_roots
             WHERE snapshot_id = ?1 ORDER BY position",
        )?;
        let stored_scope = scope_statement
            .query_map(params![snapshot_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if stored_scope != scope_bound.roots() {
            return Ok(false);
        }

        let mut criteria_statement = self.connection().prepare(
            "SELECT criterion_id, verification_method FROM planning_input_criteria
             WHERE snapshot_id = ?1 ORDER BY position",
        )?;
        let stored_criteria = criteria_statement
            .query_map(params![snapshot_id.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut expected_criteria = affected_parent_criteria
            .iter()
            .map(|criterion| {
                (
                    criterion.id.as_str().to_owned(),
                    criterion.method.as_str().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        expected_criteria.sort();
        expected_criteria.dedup();
        if stored_criteria != expected_criteria {
            return Ok(false);
        }

        let mut lineage_statement = self.connection().prepare(
            "SELECT fingerprint FROM planning_input_lineage
             WHERE snapshot_id = ?1 ORDER BY position",
        )?;
        let stored_lineage = lineage_statement
            .query_map(params![snapshot_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut expected_lineage = lineage_fingerprints.to_vec();
        expected_lineage.sort();
        expected_lineage.dedup();
        Ok(stored_lineage == expected_lineage)
    }

    /// Publishes the immutable input snapshot as current and claims a new
    /// durable planning generation for the branch. Any previous ACTIVE run
    /// is atomically superseded.
    pub fn start_planning_run(
        &mut self,
        snapshot: &PlanningInputSnapshot,
        trigger_evidence_key: &str,
        created_at_ms: u64,
    ) -> StorageResult<PlanningRunRecord> {
        require_evidence_key(trigger_evidence_key)?;
        require_evidence_key(&snapshot.input_fingerprint)?;
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        persist_snapshot(&tx, snapshot, created_at_ms)?;

        tx.execute(
            "INSERT INTO planning_input_heads (plan_branch_id, snapshot_id, input_fingerprint)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(plan_branch_id) DO UPDATE SET
               snapshot_id = excluded.snapshot_id,
               input_fingerprint = excluded.input_fingerprint",
            params![
                snapshot.plan_branch_id.as_str(),
                snapshot.id.as_str(),
                snapshot.input_fingerprint,
            ],
        )?;

        let current: Option<(String, i64)> = tx
            .query_row(
                "SELECT planning_run_id, generation FROM planning_run_heads
                 WHERE plan_branch_id = ?1",
                params![snapshot.plan_branch_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let generation = match current.as_ref() {
            Some((old_run, raw_generation)) => {
                let current_generation = positive_u64(*raw_generation, "planning run generation")?;
                tx.execute(
                    "UPDATE planning_runs SET state = 'SUPERSEDED', finished_at_ms = ?1
                     WHERE planning_run_id = ?2 AND state = 'ACTIVE'",
                    params![sqlite_u64(created_at_ms, "finished_at_ms")?, old_run],
                )?;
                current_generation.checked_add(1).ok_or_else(|| {
                    StorageError::ConstraintViolation(
                        "planning run generation would overflow".to_owned(),
                    )
                })?
            }
            None => 1,
        };
        let id = PlanningRunId::new(format!(
            "planning-run:{}:{generation}",
            snapshot.plan_branch_id.as_str()
        ))
        .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        tx.execute(
            "INSERT INTO planning_runs
             (planning_run_id, plan_branch_id, generation, snapshot_id, input_fingerprint,
              expected_plan_head, trigger_evidence_key, state, created_at_ms, finished_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ACTIVE', ?8, NULL)",
            params![
                id.as_str(),
                snapshot.plan_branch_id.as_str(),
                sqlite_u64(generation, "planning run generation")?,
                snapshot.id.as_str(),
                snapshot.input_fingerprint,
                sqlite_u64(snapshot.expected_plan_head, "expected_plan_head")?,
                trigger_evidence_key,
                sqlite_u64(created_at_ms, "created_at_ms")?,
            ],
        )?;
        tx.execute(
            "INSERT INTO planning_run_heads (plan_branch_id, planning_run_id, generation)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(plan_branch_id) DO UPDATE SET
               planning_run_id = excluded.planning_run_id,
               generation = excluded.generation",
            params![
                snapshot.plan_branch_id.as_str(),
                id.as_str(),
                sqlite_u64(generation, "planning run generation")?,
            ],
        )?;
        tx.commit()?;
        Ok(PlanningRunRecord {
            id,
            plan_branch_id: snapshot.plan_branch_id.clone(),
            generation,
            snapshot_id: snapshot.id.clone(),
            input_fingerprint: snapshot.input_fingerprint.clone(),
            expected_plan_head: snapshot.expected_plan_head,
            trigger_evidence_key: trigger_evidence_key.to_owned(),
            state: PlanningRunState::Active,
            created_at_ms,
            finished_at_ms: None,
        })
    }

    pub fn finish_planning_run(
        &mut self,
        run_id: &PlanningRunId,
        generation: u64,
        state: PlanningRunState,
        finished_at_ms: u64,
    ) -> StorageResult<()> {
        if matches!(
            state,
            PlanningRunState::Active | PlanningRunState::Superseded
        ) {
            return Err(StorageError::ConstraintViolation(
                "finish_planning_run requires a terminal non-superseded state".to_owned(),
            ));
        }
        let tx = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let branch: String = tx
            .query_row(
                "SELECT plan_branch_id FROM planning_runs WHERE planning_run_id = ?1",
                params![run_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::NotFound(format!("planning run {}", run_id.as_str())))?;
        require_current_run(&tx, &branch, run_id.as_str(), generation)?;
        let updated = tx.execute(
            "UPDATE planning_runs SET state = ?1, finished_at_ms = ?2
             WHERE planning_run_id = ?3 AND generation = ?4 AND state = 'ACTIVE'",
            params![
                state.as_str(),
                sqlite_u64(finished_at_ms, "finished_at_ms")?,
                run_id.as_str(),
                sqlite_u64(generation, "planning run generation")?,
            ],
        )?;
        if updated != 1 {
            return Err(StorageError::PlanningRunStale {
                branch,
                expected_generation: generation,
                found_generation: generation,
            });
        }
        tx.commit()?;
        Ok(())
    }

    pub fn current_planning_run(
        &self,
        branch: &PlanBranchId,
    ) -> StorageResult<Option<PlanningRunRecord>> {
        load_current_run(self.connection(), branch)
    }

    pub fn current_planning_input_matches(
        &self,
        branch: &PlanBranchId,
        snapshot_id: &PlanningInputSnapshotId,
        fingerprint: &str,
    ) -> StorageResult<bool> {
        require_evidence_key(fingerprint)?;
        let current: Option<(String, String)> = self
            .connection()
            .query_row(
                "SELECT snapshot_id, input_fingerprint FROM planning_input_heads
                 WHERE plan_branch_id = ?1",
                params![branch.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(current.is_some_and(|(id, hash)| id == snapshot_id.as_str() && hash == fingerprint))
    }

    /// Finds every accepted current Plan whose head graph contains `task_id`.
    /// The application layer requires exactly one for production execution.
    pub fn find_current_plan_heads_for_task(
        &self,
        task_id: &TaskId,
    ) -> StorageResult<Vec<(PlanBranchId, u64)>> {
        let mut statement = self.connection().prepare(
            "SELECT DISTINCT b.plan_branch_id, b.head_revision
             FROM plan_branches b
             JOIN plan_nodes n
               ON n.plan_branch_id = b.plan_branch_id
              AND n.revision_seq = b.head_revision
             WHERE b.head_revision > 0 AND n.node_id = ?1
             ORDER BY b.plan_branch_id",
        )?;
        let rows = statement.query_map(params![task_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (branch, head) = row?;
            out.push((
                PlanBranchId::new(branch)
                    .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
                positive_u64(head, "plan head")?,
            ));
        }
        Ok(out)
    }

    pub fn revision_has_current_planning_input(
        &self,
        branch: &PlanBranchId,
        revision_seq: u64,
    ) -> StorageResult<bool> {
        let binding: Option<(String, String)> = self
            .connection()
            .query_row(
                "SELECT planning_input_snapshot_id, planning_input_fingerprint
                 FROM plan_revision_lineage
                 WHERE plan_branch_id = ?1 AND revision_seq = ?2",
                params![branch.as_str(), sqlite_u64(revision_seq, "revision_seq")?],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((snapshot, fingerprint)) = binding else {
            return Ok(false);
        };
        let snapshot_id = PlanningInputSnapshotId::new(snapshot)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?;
        self.current_planning_input_matches(branch, &snapshot_id, &fingerprint)
    }
}

pub(crate) fn enforce_publication_fence(
    connection: &Connection,
    lineage: &PlanRevisionLineage,
    branch: &PlanBranchId,
    expected_plan_head: u64,
) -> StorageResult<()> {
    let current_input: Option<(String, String)> = connection
        .query_row(
            "SELECT snapshot_id, input_fingerprint FROM planning_input_heads
             WHERE plan_branch_id = ?1",
            params![branch.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if !current_input.is_some_and(|(id, hash)| {
        id == lineage.planning_input_snapshot_id.as_str()
            && hash == lineage.planning_input_fingerprint
    }) {
        return Err(StorageError::PlanningInputStale {
            branch: branch.as_str().to_owned(),
            expected_fingerprint: lineage.planning_input_fingerprint.clone(),
        });
    }

    require_current_run(
        connection,
        branch.as_str(),
        lineage.planning_run_id.as_str(),
        lineage.planning_run_generation,
    )?;
    let run: (String, String, i64, String) = connection.query_row(
        "SELECT snapshot_id, input_fingerprint, expected_plan_head, state
         FROM planning_runs WHERE planning_run_id = ?1",
        params![lineage.planning_run_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let run_head = nonnegative_u64(run.2, "planning run expected_plan_head")?;
    if run.0 != lineage.planning_input_snapshot_id.as_str()
        || run.1 != lineage.planning_input_fingerprint
        || run_head != expected_plan_head
        || run.3 != "ACTIVE"
    {
        return Err(StorageError::PlanningRunStale {
            branch: branch.as_str().to_owned(),
            expected_generation: lineage.planning_run_generation,
            found_generation: lineage.planning_run_generation,
        });
    }
    Ok(())
}

pub(crate) fn mark_publication_succeeded(
    connection: &Connection,
    lineage: &PlanRevisionLineage,
    finished_at_ms: u64,
) -> StorageResult<()> {
    let updated = connection.execute(
        "UPDATE planning_runs SET state = 'SUCCEEDED', finished_at_ms = ?1
         WHERE planning_run_id = ?2 AND generation = ?3 AND state = 'ACTIVE'",
        params![
            sqlite_u64(finished_at_ms, "finished_at_ms")?,
            lineage.planning_run_id.as_str(),
            sqlite_u64(lineage.planning_run_generation, "planning run generation")?,
        ],
    )?;
    if updated != 1 {
        return Err(StorageError::PlanningRunStale {
            branch: String::new(),
            expected_generation: lineage.planning_run_generation,
            found_generation: lineage.planning_run_generation,
        });
    }
    Ok(())
}

fn persist_snapshot(
    tx: &Transaction<'_>,
    snapshot: &PlanningInputSnapshot,
    created_at_ms: u64,
) -> StorageResult<()> {
    let (grant_hash, grant_expiry, grant_consumed) = snapshot
        .risk_grant
        .as_ref()
        .map(|grant| {
            (
                Some(grant.proposal_hash.as_str()),
                Some(sqlite_u64(grant.expires_at_ms, "risk grant expiry")),
                Some(if grant.consumed { 1_i64 } else { 0_i64 }),
            )
        })
        .unwrap_or((None, None, None));
    tx.execute(
        "INSERT INTO planning_input_snapshots
         (snapshot_id, plan_branch_id, task_ref, expected_plan_head, baseline_risk,
          policy_fingerprint, input_fingerprint, risk_grant_proposal_hash,
          risk_grant_expires_at_ms, risk_grant_consumed, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(snapshot_id) DO NOTHING",
        params![
            snapshot.id.as_str(),
            snapshot.plan_branch_id.as_str(),
            snapshot.task_ref,
            sqlite_u64(snapshot.expected_plan_head, "expected_plan_head")?,
            risk_name(snapshot.baseline_risk),
            snapshot.policy_fingerprint,
            snapshot.input_fingerprint,
            grant_hash,
            grant_expiry.transpose()?,
            grant_consumed,
            sqlite_u64(created_at_ms, "created_at_ms")?,
        ],
    )?;

    let stored: StoredPlanningInputRow = tx.query_row(
        "SELECT plan_branch_id, task_ref, expected_plan_head, baseline_risk,
                    policy_fingerprint, input_fingerprint, risk_grant_proposal_hash,
                    risk_grant_expires_at_ms, risk_grant_consumed
             FROM planning_input_snapshots WHERE snapshot_id = ?1",
        params![snapshot.id.as_str()],
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
            ))
        },
    )?;
    let expected_grant_expiry = snapshot
        .risk_grant
        .as_ref()
        .map(|grant| sqlite_u64(grant.expires_at_ms, "risk grant expiry"))
        .transpose()?;
    let expected_grant_consumed =
        snapshot
            .risk_grant
            .as_ref()
            .map(|grant| if grant.consumed { 1_i64 } else { 0_i64 });
    if stored.0 != snapshot.plan_branch_id.as_str()
        || stored.1 != snapshot.task_ref
        || nonnegative_u64(stored.2, "expected_plan_head")? != snapshot.expected_plan_head
        || stored.3 != risk_name(snapshot.baseline_risk)
        || stored.4 != snapshot.policy_fingerprint
        || stored.5 != snapshot.input_fingerprint
        || stored.6.as_deref()
            != snapshot
                .risk_grant
                .as_ref()
                .map(|grant| grant.proposal_hash.as_str())
        || stored.7 != expected_grant_expiry
        || stored.8 != expected_grant_consumed
    {
        return Err(StorageError::Corrupt(format!(
            "planning input snapshot {} identity collision",
            snapshot.id.as_str()
        )));
    }

    persist_ordered_values(
        tx,
        "planning_input_scope_roots",
        "scope_root",
        snapshot.id.as_str(),
        snapshot.scope_bound.roots(),
    )?;
    persist_ordered_pairs(
        tx,
        snapshot.id.as_str(),
        &snapshot
            .affected_parent_criteria
            .iter()
            .map(|criterion| (criterion.id.as_str(), criterion.method.as_str()))
            .collect::<Vec<_>>(),
    )?;
    persist_ordered_values(
        tx,
        "planning_input_evidence",
        "evidence_ref",
        snapshot.id.as_str(),
        snapshot
            .triggering_evidence
            .iter()
            .map(|value| value.as_str().to_owned())
            .collect(),
    )?;
    persist_ordered_values(
        tx,
        "planning_input_lineage",
        "fingerprint",
        snapshot.id.as_str(),
        snapshot.lineage_fingerprints.clone(),
    )?;
    persist_ordered_values(
        tx,
        "planning_input_candidate_tasks",
        "task_ref",
        snapshot.id.as_str(),
        snapshot.trusted_candidate_task_refs.clone(),
    )?;
    Ok(())
}

fn persist_ordered_values(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    snapshot_id: &str,
    values: Vec<String>,
) -> StorageResult<()> {
    // Table/column names are compile-time constants from the callers above.
    let insert = format!(
        "INSERT INTO {table} (snapshot_id, position, {column}) VALUES (?1, ?2, ?3)
         ON CONFLICT(snapshot_id, position) DO NOTHING"
    );
    for (position, value) in values.iter().enumerate() {
        tx.execute(&insert, params![snapshot_id, position as i64, value])?;
    }
    let select = format!("SELECT {column} FROM {table} WHERE snapshot_id = ?1 ORDER BY position");
    let mut statement = tx.prepare(&select)?;
    let stored = statement
        .query_map(params![snapshot_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if stored != values {
        return Err(StorageError::Corrupt(format!(
            "planning input snapshot {snapshot_id} has conflicting {column} values"
        )));
    }
    Ok(())
}

fn persist_ordered_pairs(
    tx: &Transaction<'_>,
    snapshot_id: &str,
    values: &[(&str, &str)],
) -> StorageResult<()> {
    for (position, (id, method)) in values.iter().enumerate() {
        tx.execute(
            "INSERT INTO planning_input_criteria
             (snapshot_id, position, criterion_id, verification_method)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(snapshot_id, position) DO NOTHING",
            params![snapshot_id, position as i64, id, method],
        )?;
    }
    let mut statement = tx.prepare(
        "SELECT criterion_id, verification_method FROM planning_input_criteria
         WHERE snapshot_id = ?1 ORDER BY position",
    )?;
    let stored = statement
        .query_map(params![snapshot_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected = values
        .iter()
        .map(|(id, method)| ((*id).to_owned(), (*method).to_owned()))
        .collect::<Vec<_>>();
    if stored != expected {
        return Err(StorageError::Corrupt(format!(
            "planning input snapshot {snapshot_id} has conflicting criteria"
        )));
    }
    Ok(())
}

fn load_current_run(
    connection: &Connection,
    branch: &PlanBranchId,
) -> StorageResult<Option<PlanningRunRecord>> {
    let row: Option<CurrentPlanningRunRow> = connection
        .query_row(
            "SELECT r.planning_run_id, r.generation, r.snapshot_id, r.input_fingerprint,
                    r.expected_plan_head, r.trigger_evidence_key, r.state,
                    r.created_at_ms, r.finished_at_ms
             FROM planning_run_heads h
             JOIN planning_runs r ON r.planning_run_id = h.planning_run_id
             WHERE h.plan_branch_id = ?1",
            params![branch.as_str()],
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
                ))
            },
        )
        .optional()?;
    let Some((
        run_id,
        generation,
        snapshot_id,
        fingerprint,
        expected_head,
        key,
        state,
        created,
        finished,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(PlanningRunRecord {
        id: PlanningRunId::new(run_id)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
        plan_branch_id: branch.clone(),
        generation: positive_u64(generation, "planning run generation")?,
        snapshot_id: PlanningInputSnapshotId::new(snapshot_id)
            .map_err(|error| StorageError::DomainReconstruction(error.to_string()))?,
        input_fingerprint: fingerprint,
        expected_plan_head: nonnegative_u64(expected_head, "planning run expected head")?,
        trigger_evidence_key: key,
        state: PlanningRunState::parse(&state)?,
        created_at_ms: nonnegative_u64(created, "planning run created_at")?,
        finished_at_ms: finished
            .map(|value| nonnegative_u64(value, "planning run finished_at"))
            .transpose()?,
    }))
}

fn require_current_run(
    connection: &Connection,
    branch: &str,
    run_id: &str,
    generation: u64,
) -> StorageResult<()> {
    let current: Option<(String, i64)> = connection
        .query_row(
            "SELECT planning_run_id, generation FROM planning_run_heads
             WHERE plan_branch_id = ?1",
            params![branch],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let found_generation = current
        .as_ref()
        .and_then(|(_, raw)| u64::try_from(*raw).ok())
        .unwrap_or(0);
    if !current.is_some_and(|(id, raw)| id == run_id && u64::try_from(raw).ok() == Some(generation))
    {
        return Err(StorageError::PlanningRunStale {
            branch: branch.to_owned(),
            expected_generation: generation,
            found_generation,
        });
    }
    Ok(())
}

fn risk_name(risk: RiskClass) -> &'static str {
    match risk {
        RiskClass::Low => "low",
        RiskClass::Medium => "medium",
        RiskClass::High => "high",
        RiskClass::Critical => "critical",
    }
}

fn sqlite_u64(value: u64, field: &str) -> StorageResult<i64> {
    i64::try_from(value).map_err(|_| {
        StorageError::ConstraintViolation(format!("{field} exceeds SQLite INTEGER range"))
    })
}

fn positive_u64(value: i64, field: &str) -> StorageResult<u64> {
    let value = u64::try_from(value)
        .map_err(|_| StorageError::Corrupt(format!("{field} stores a negative value")))?;
    if value == 0 {
        return Err(StorageError::Corrupt(format!("{field} stores zero")));
    }
    Ok(value)
}

fn nonnegative_u64(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value)
        .map_err(|_| StorageError::Corrupt(format!("{field} stores a negative value")))
}
