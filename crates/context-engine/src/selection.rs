//! Pure deterministic admission (ADR-0042 sections 5-7, rules CX-1..CX-10).
//!
//! [`admit`] turns caller-supplied candidates into a [`ContextPlan`]
//! plus its [`RetrievalTrace`]. Same inputs always yield byte-identical
//! outputs ([`verify_replay`] proves it); callers persist both.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::*;

/// One selection candidate: the item plus its counted cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub item: ContextItem,
    pub estimated_tokens: u64,
}

/// Complete admission inputs (the replay unit, CX-5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionInputs {
    pub plan_id: ContextPlanId,
    pub revision_seq: u64,
    pub candidates: Vec<Candidate>,
    pub budget: Budget,
    pub policy_fingerprint: String,
    pub renderer: String,
    /// Evaluation instant for the freshness re-check (CX-5): part of
    /// the replay unit, so replays compare against the same clock.
    pub now_millis: u64,
    /// Minimum trust for model context; below-floor items reject even
    /// when required (safety gates override requirement).
    pub trust_floor: TrustLevel,
}

/// Token counting behind a port (ADR-0042 section 6).
pub trait TokenCounter {
    fn count(&self, text: &str) -> u64;
    fn method(&self) -> CounterMethod;
}

/// The `chars/4` estimator, explicitly labeled — never a silent basis.
#[derive(Debug, Clone, Copy, Default)]
pub struct CharHeuristicCounter;

impl TokenCounter for CharHeuristicCounter {
    fn count(&self, text: &str) -> u64 {
        let chars = text.chars().count() as u64;
        chars.div_ceil(4)
    }

    fn method(&self) -> CounterMethod {
        CounterMethod::CharHeuristic
    }
}

/// Admits candidates into a plan plus trace, enforcing CX rules.
///
/// Order of checks: renderer shape, fingerprint shape, budget shape,
/// per-item structure (duplicate ids, full item validation incl.
/// bindings), SECRET refusal, trust floor, required fit, freshness
/// re-check, priority fill, supersession. Every candidate gets a
/// traced decision; output sections are id-sorted for determinism.
pub fn admit(inputs: &SelectionInputs) -> Result<(ContextPlan, RetrievalTrace), ContextError> {
    let fingerprint_chars = inputs.policy_fingerprint.chars().count();
    if fingerprint_chars == 0 || fingerprint_chars > MAX_TEXT_CHARS {
        return Err(ContextError::MalformedPlan(format!(
            "policy fingerprint must be 1..={MAX_TEXT_CHARS} chars"
        )));
    }
    let renderer = inputs.renderer.strip_prefix("renderer:").ok_or_else(|| {
        ContextError::MalformedPlan("renderer id must be renderer:<name>".to_owned())
    })?;
    if renderer.trim().is_empty() {
        return Err(ContextError::MalformedPlan(
            "renderer id must be renderer:<name>".to_owned(),
        ));
    }
    if inputs.renderer.chars().count() > MAX_RENDERER_CHARS {
        return Err(ContextError::MalformedPlan(format!(
            "renderer id exceeds {MAX_RENDERER_CHARS} chars"
        )));
    }
    if inputs.budget.soft_tokens > inputs.budget.hard_tokens {
        return Err(ContextError::InvalidInput(
            "soft budget exceeds hard budget".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    for candidate in &inputs.candidates {
        let id = candidate.item.id.as_str();
        if !seen.insert(id.to_owned()) {
            return Err(ContextError::MalformedPlan(format!("duplicate item {id}")));
        }
        candidate.item.validate()?;
    }

    let superseded: BTreeSet<String> = inputs
        .candidates
        .iter()
        .filter_map(|c| {
            c.item
                .provenance
                .supersession
                .as_ref()
                .map(|s| s.supersedes.as_str().to_owned())
        })
        .filter(|id| seen.contains(id))
        .collect();

    let mut required: Vec<&Candidate> = Vec::new();
    let mut optional: Vec<&Candidate> = Vec::new();
    let mut decisions: BTreeMap<String, TraceDecision> = BTreeMap::new();
    for candidate in &inputs.candidates {
        let id = candidate.item.id.clone();
        if candidate.item.sensitivity == Sensitivity::Secret {
            decisions.insert(
                id.as_str().to_owned(),
                TraceDecision {
                    item_id: id,
                    outcome: DecisionOutcome::Rejected,
                    reason: Some(ReasonCode::SensitivityRefusal),
                    rule: "secret-refusal".to_owned(),
                },
            );
            continue;
        }
        if candidate.item.trust.level < inputs.trust_floor {
            decisions.insert(
                id.as_str().to_owned(),
                TraceDecision {
                    item_id: id,
                    outcome: DecisionOutcome::Rejected,
                    reason: Some(ReasonCode::TrustBelowFloor),
                    rule: "trust-floor".to_owned(),
                },
            );
            continue;
        }
        if candidate.item.required {
            required.push(candidate);
        } else {
            optional.push(candidate);
        }
    }

    let required_cost: u64 = required.iter().map(|c| c.estimated_tokens).sum();
    if required_cost > inputs.budget.hard_tokens {
        return Err(ContextError::BudgetExceeded {
            needed: required_cost,
            hard: inputs.budget.hard_tokens,
        });
    }

    let mut required_ids: Vec<ContextItemId> = required.iter().map(|c| c.item.id.clone()).collect();
    required_ids.sort();
    let stale_ids: BTreeSet<String> = inputs
        .candidates
        .iter()
        .filter(|c| {
            c.item
                .freshness
                .stale_after
                .is_some_and(|limit| inputs.now_millis > limit)
        })
        .map(|c| c.item.id.as_str().to_owned())
        .collect();
    for id in &required_ids {
        // Freshness re-check is a traced step (CX-5): required items
        // stay selected but carry the stale marker in their rule.
        let rule = if stale_ids.contains(id.as_str()) {
            "required-stale"
        } else if superseded.contains(id.as_str()) {
            "required-over-superseded"
        } else {
            "required"
        };
        decisions.insert(
            id.as_str().to_owned(),
            TraceDecision {
                item_id: id.clone(),
                outcome: DecisionOutcome::Selected,
                reason: None,
                rule: rule.to_owned(),
            },
        );
    }

    optional.sort_by(|a, b| {
        b.item
            .priority
            .cmp(&a.item.priority)
            .then_with(|| a.item.id.cmp(&b.item.id))
    });
    let mut running = required_cost;
    let mut selected_ids = Vec::new();
    let mut deferred = Vec::new();
    for candidate in optional {
        let id = candidate.item.id.clone();
        if stale_ids.contains(id.as_str()) {
            deferred.push((id.clone(), ReasonCode::StaleSource));
            decisions.insert(
                id.as_str().to_owned(),
                TraceDecision {
                    item_id: id,
                    outcome: DecisionOutcome::Deferred,
                    reason: Some(ReasonCode::StaleSource),
                    rule: "stale-source".to_owned(),
                },
            );
            continue;
        }
        if superseded.contains(id.as_str()) {
            deferred.push((id.clone(), ReasonCode::Superseded));
            decisions.insert(
                id.as_str().to_owned(),
                TraceDecision {
                    item_id: id,
                    outcome: DecisionOutcome::Deferred,
                    reason: Some(ReasonCode::Superseded),
                    rule: "superseded".to_owned(),
                },
            );
            continue;
        }
        if running + candidate.estimated_tokens <= inputs.budget.soft_tokens {
            running += candidate.estimated_tokens;
            selected_ids.push(id.clone());
            decisions.insert(
                id.as_str().to_owned(),
                TraceDecision {
                    item_id: id,
                    outcome: DecisionOutcome::Selected,
                    reason: None,
                    rule: "priority-fill".to_owned(),
                },
            );
        } else {
            deferred.push((id.clone(), ReasonCode::BudgetExceeded));
            decisions.insert(
                id.as_str().to_owned(),
                TraceDecision {
                    item_id: id,
                    outcome: DecisionOutcome::Deferred,
                    reason: Some(ReasonCode::BudgetExceeded),
                    rule: "soft-cap".to_owned(),
                },
            );
        }
    }
    selected_ids.sort();
    deferred.sort();

    let mut rejected: Vec<(ContextItemId, ReasonCode)> = decisions
        .values()
        .filter(|d| d.outcome == DecisionOutcome::Rejected)
        .map(|d| {
            (
                d.item_id.clone(),
                d.reason.unwrap_or(ReasonCode::DeferredByPolicy),
            )
        })
        .collect();
    rejected.sort();

    let mut source_hashes: Vec<ContentHash> = inputs
        .candidates
        .iter()
        .map(|c| c.item.content_hash.clone())
        .collect();
    source_hashes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    source_hashes.dedup_by(|a, b| a.as_str() == b.as_str());

    let mut source_revs: Vec<String> = inputs
        .candidates
        .iter()
        .map(|c| c.item.source_ref.revision_binding())
        .collect();
    source_revs.sort();
    source_revs.dedup();

    let mut ordered_selected = required_ids.clone();
    ordered_selected.extend(selected_ids.iter().cloned());
    let cache_boundaries = vec![CacheBoundary {
        // Recording shape is frozen; boundary optimization
        // (StablePrefix splitting) is future work.
        class: CacheClass::SessionEphemeral,
        items: ordered_selected,
    }];
    let plan = ContextPlan {
        id: inputs.plan_id.clone(),
        revision_seq: inputs.revision_seq,
        required: required_ids,
        selected: selected_ids,
        deferred,
        rejected,
        budget: inputs.budget.clone(),
        cache_boundaries: cache_boundaries.clone(),
        renderer: inputs.renderer.clone(),
        source_hashes,
        policy_fingerprint: inputs.policy_fingerprint.clone(),
    };
    let trace = RetrievalTrace {
        plan_id: inputs.plan_id.clone(),
        decisions: decisions.into_values().collect(),
        budget: inputs.budget.clone(),
        source_revs,
        renderer: inputs.renderer.clone(),
        policy_fingerprint: inputs.policy_fingerprint.clone(),
        cache_boundaries,
    };
    Ok((plan, trace))
}

/// Replays admission and byte-compares against recorded outputs (CX-5).
pub fn verify_replay(
    inputs: &SelectionInputs,
    plan: &ContextPlan,
    trace: &RetrievalTrace,
) -> Result<(), ContextError> {
    let (replayed_plan, replayed_trace) = admit(inputs)?;
    if &replayed_plan != plan {
        return Err(ContextError::ReplayMismatch(
            "replayed plan differs from recorded plan".to_owned(),
        ));
    }
    if &replayed_trace != trace {
        return Err(ContextError::ReplayMismatch(
            "replayed trace differs from recorded trace".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::mint::mint_item_id;
    use crate::model::MAX_TEXT_CHARS;
    use crate::selection::*;

    const HASH: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn item(seq: u128, priority: u16, required: bool, sensitivity: Sensitivity) -> ContextItem {
        ContextItem {
            id: mint_item_id(1000, seq),
            kind: ContextKind::FileExcerpt,
            source_ref: SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap(),
            content_hash: ContentHash::new(HASH).unwrap(),
            priority,
            freshness: Freshness {
                observed_at: 1000,
                source_rev: "rev1".to_owned(),
                stale_after: None,
            },
            trust: Trust {
                level: TrustLevel::Unverified,
                reason: "fixture".to_owned(),
            },
            sensitivity,
            required,
            cache_class: CacheClass::SessionEphemeral,
            provenance: ProvenanceEnvelope::new(
                SourceRef::repo_path("rev1", "src/lib.rs", None).unwrap(),
                "rev1",
                1000,
                "fixture-selector",
                "scope:test",
                800,
            )
            .unwrap(),
        }
    }

    fn inputs(candidates: Vec<(ContextItem, u64)>) -> SelectionInputs {
        SelectionInputs {
            plan_id: crate::mint::mint_plan_id(1000, 1),
            revision_seq: 1,
            candidates: candidates
                .into_iter()
                .map(|(item, estimated_tokens)| Candidate {
                    item,
                    estimated_tokens,
                })
                .collect(),
            budget: Budget {
                soft_tokens: 100,
                hard_tokens: 200,
                counter: CounterMethod::CharHeuristic,
            },
            policy_fingerprint: "policy:fp".to_owned(),
            renderer: "renderer:plain".to_owned(),
            now_millis: 1000,
            trust_floor: TrustLevel::Unverified,
        }
    }

    #[test]
    fn admit_selects_required_and_fills_by_priority() {
        let req = item(1, 1, true, Sensitivity::Public);
        let hi = item(2, 90, false, Sensitivity::Public);
        let lo = item(3, 10, false, Sensitivity::Public);
        let (plan, trace) = admit(&inputs(vec![
            (req.clone(), 40),
            (hi.clone(), 40),
            (lo.clone(), 40),
        ]))
        .unwrap();
        assert_eq!(plan.required, vec![req.id.clone()]);
        assert_eq!(plan.selected, vec![hi.id.clone()]);
        assert_eq!(
            plan.deferred,
            vec![(lo.id.clone(), ReasonCode::BudgetExceeded)]
        );
        assert!(plan.rejected.is_empty());
        assert_eq!(trace.decisions.len(), 3);
    }

    #[test]
    fn admit_rejects_secret_and_never_selects_it() {
        let secret = item(1, 255, true, Sensitivity::Secret);
        let (plan, trace) = admit(&inputs(vec![(secret.clone(), 10)])).unwrap();
        assert!(plan.required.is_empty());
        assert!(plan.selected.is_empty());
        assert_eq!(
            plan.rejected,
            vec![(secret.id.clone(), ReasonCode::SensitivityRefusal)]
        );
        let decision = trace
            .decisions
            .iter()
            .find(|d| d.item_id == secret.id)
            .unwrap();
        assert_eq!(decision.outcome, DecisionOutcome::Rejected);
    }

    #[test]
    fn admit_refuses_whole_plan_when_required_exceeds_hard() {
        let big = item(1, 1, true, Sensitivity::Public);
        let err = admit(&inputs(vec![(big, 999)])).unwrap_err();
        assert!(matches!(err, ContextError::BudgetExceeded { .. }));
    }

    #[test]
    fn admit_refuses_unbound_source_ref() {
        let mut bad = item(1, 1, true, Sensitivity::Public);
        bad.source_ref = SourceRef::Task {
            task_id: String::new(),
            event_seq: 42,
        };
        let err = admit(&inputs(vec![(bad, 10)])).unwrap_err();
        assert!(matches!(err, ContextError::UnboundSourceRef(_)));

        let mut bad_env = item(2, 1, true, Sensitivity::Public);
        bad_env.provenance.source = SourceRef::Task {
            task_id: String::new(),
            event_seq: 7,
        };
        let err = admit(&inputs(vec![(bad_env, 10)])).unwrap_err();
        assert!(matches!(err, ContextError::UnboundSourceRef(_)));
    }

    #[test]
    fn admit_refuses_blank_and_overlong_fingerprint() {
        let solo = item(5, 1, false, Sensitivity::Public);
        let mut blank = inputs(vec![(solo.clone(), 10)]);
        blank.policy_fingerprint = String::new();
        assert!(matches!(admit(&blank), Err(ContextError::MalformedPlan(_))));

        let mut overlong = inputs(vec![(solo, 10)]);
        overlong.policy_fingerprint = "p".repeat(MAX_TEXT_CHARS + 1);
        assert!(matches!(
            admit(&overlong),
            Err(ContextError::MalformedPlan(_))
        ));
    }

    #[test]
    fn admit_measures_text_bounds_before_trimming() {
        // Raw 4098 chars, trimmed 4094: the SQL CHECK counts stored
        // chars, so the domain must refuse (not fail at the backend).
        let mut padded = item(6, 1, false, Sensitivity::Public);
        padded.trust.reason = format!("  {}  ", "x".repeat(MAX_TEXT_CHARS - 2));
        let err = admit(&inputs(vec![(padded, 10)])).unwrap_err();
        assert!(matches!(err, ContextError::InvalidInput(_)));

        let mut snug = item(7, 1, false, Sensitivity::Public);
        snug.trust.reason = " padded ".to_owned();
        let (plan, _) = admit(&inputs(vec![(snug.clone(), 10)])).unwrap();
        assert_eq!(plan.selected, vec![snug.id.clone()]);
    }

    #[test]
    fn admit_refuses_bad_renderer_and_duplicate_ids() {
        let first = item(1, 1, false, Sensitivity::Public);
        let mut dup = item(2, 1, false, Sensitivity::Public);
        dup.id = first.id.clone();
        let err = admit(&inputs(vec![(first, 10), (dup, 10)])).unwrap_err();
        assert!(matches!(err, ContextError::MalformedPlan(_)));

        let solo = item(5, 1, false, Sensitivity::Public);
        let mut bad_renderer = inputs(vec![(solo, 10)]);
        bad_renderer.renderer = "plain".to_owned();
        let err = admit(&bad_renderer).unwrap_err();
        assert!(matches!(err, ContextError::MalformedPlan(_)));
    }

    #[test]
    fn admit_defers_superseded_non_required() {
        let old = item(1, 50, false, Sensitivity::Public);
        let mut new = item(2, 50, false, Sensitivity::Public);
        new.provenance = new.provenance.clone().with_supersession(Supersession {
            supersedes: old.id.clone(),
            reason: "fresher".to_owned(),
        });
        let (plan, _) = admit(&inputs(vec![(old.clone(), 10), (new.clone(), 10)])).unwrap();
        assert_eq!(
            plan.deferred,
            vec![(old.id.clone(), ReasonCode::Superseded)]
        );
        assert_eq!(plan.selected, vec![new.id.clone()]);
    }

    #[test]
    fn admit_keeps_required_winner_over_supersession() {
        let old = item(1, 50, true, Sensitivity::Public);
        let mut new = item(2, 50, false, Sensitivity::Public);
        new.provenance.supersession = Some(Supersession {
            supersedes: old.id.clone(),
            reason: "fresher".to_owned(),
        });
        let (plan, trace) = admit(&inputs(vec![(old.clone(), 10), (new.clone(), 10)])).unwrap();
        assert_eq!(plan.required, vec![old.id.clone()]);
        assert!(plan.deferred.is_empty());
        let decision = trace
            .decisions
            .iter()
            .find(|d| d.item_id == old.id)
            .unwrap();
        assert_eq!(decision.outcome, DecisionOutcome::Selected);
        assert_eq!(decision.rule, "required-over-superseded");
    }

    #[test]
    fn admit_is_deterministic_and_replay_verifies() {
        let a = item(1, 50, false, Sensitivity::Public);
        let b = item(2, 50, false, Sensitivity::Public);
        let in_a = inputs(vec![(a.clone(), 10), (b.clone(), 10)]);
        let in_b = inputs(vec![(b.clone(), 10), (a.clone(), 10)]);
        let (plan_a, trace_a) = admit(&in_a).unwrap();
        let (plan_b, trace_b) = admit(&in_b).unwrap();
        assert_eq!(plan_a, plan_b);
        assert_eq!(trace_a, trace_b);
        verify_replay(&in_a, &plan_a, &trace_a).unwrap();

        let mut tampered = trace_a.clone();
        tampered.decisions.swap(0, 1);
        assert!(matches!(
            verify_replay(&in_a, &plan_a, &tampered),
            Err(ContextError::ReplayMismatch(_))
        ));
    }

    #[test]
    fn stale_optional_defers_and_required_carries_stale_rule() {
        let mut old = item(1, 90, false, Sensitivity::Public);
        old.freshness.stale_after = Some(999);
        let mut req_old = item(2, 1, true, Sensitivity::Public);
        req_old.freshness.stale_after = Some(999);
        let (plan, trace) = admit(&inputs(vec![(old.clone(), 10), (req_old.clone(), 10)])).unwrap();
        assert_eq!(
            plan.deferred,
            vec![(old.id.clone(), ReasonCode::StaleSource)]
        );
        assert_eq!(plan.required, vec![req_old.id.clone()]);
        let rule = trace
            .decisions
            .iter()
            .find(|d| d.item_id == req_old.id)
            .unwrap()
            .rule
            .clone();
        assert_eq!(rule, "required-stale");
    }

    #[test]
    fn trust_floor_rejects_even_required() {
        let mut shady = item(1, 90, true, Sensitivity::Public);
        shady.trust.level = TrustLevel::Untrusted;
        let (plan, _) = admit(&inputs(vec![(shady.clone(), 10)])).unwrap();
        assert!(plan.required.is_empty());
        assert_eq!(
            plan.rejected,
            vec![(shady.id.clone(), ReasonCode::TrustBelowFloor)]
        );
    }

    #[test]
    fn replay_detects_fingerprint_and_clock_drift() {
        let solo = item(1, 50, false, Sensitivity::Public);
        let base = inputs(vec![(solo, 10)]);
        let (plan, trace) = admit(&base).unwrap();
        verify_replay(&base, &plan, &trace).unwrap();

        let mut other_fp = base.clone();
        other_fp.policy_fingerprint = "policy:other".to_owned();
        assert!(matches!(
            verify_replay(&other_fp, &plan, &trace),
            Err(ContextError::ReplayMismatch(_))
        ));

        // A stale boundary proves the clock matters to replay:
        let mut expiring = item(7, 50, false, Sensitivity::Public);
        expiring.freshness.stale_after = Some(1500);
        let expiring_inputs = inputs(vec![(expiring, 10)]);
        let (plan_e, trace_e) = admit(&expiring_inputs).unwrap();
        assert!(plan_e.selected.len() == 1);
        let mut late = expiring_inputs.clone();
        late.now_millis = 2000;
        assert!(matches!(
            verify_replay(&late, &plan_e, &trace_e),
            Err(ContextError::ReplayMismatch(_))
        ));
    }

    #[test]
    fn renderer_over_cap_and_blank_trust_reason_refuse() {
        let solo = item(1, 1, false, Sensitivity::Public);
        let mut overlong = inputs(vec![(solo, 10)]);
        overlong.renderer = format!("renderer:{}", "x".repeat(1025));
        assert!(matches!(
            admit(&overlong).unwrap_err(),
            ContextError::MalformedPlan(_)
        ));

        let mut blank_trust = item(2, 1, false, Sensitivity::Public);
        blank_trust.trust.reason.clear();
        assert!(matches!(
            admit(&inputs(vec![(blank_trust, 10)])).unwrap_err(),
            ContextError::InvalidInput(_)
        ));
    }

    #[test]
    fn char_heuristic_counts_ceil_chars_over_four() {
        let counter = CharHeuristicCounter;
        assert_eq!(counter.method(), CounterMethod::CharHeuristic);
        assert_eq!(counter.count(""), 0);
        assert_eq!(counter.count("abcd"), 1);
        assert_eq!(counter.count("abcde"), 2);
    }
}
