//! Shared `context-engine` store contract suite (TASK-0170, G9).
//!
//! The canonical durable selection seam is
//! [`context_engine::port::ContextStorePort`] (ADR-0042): atomic
//! (plan, items, trace) persistence with reopen-after-restart
//! durability. Every store implementing the seam must satisfy
//! [`run_context_store_contract`].
//!
//! What the contract asserts (only behavior the seam promises):
//!
//! ```text
//! roundtrip → save then load returns byte-identical plan, items, trace
//! explain   → envelope + traced decision per item; unknown → None
//! missing   → unknown plan/item/trace loads are None, never invented
//! idempot.  → byte-identical re-save succeeds; conflicting re-save refuses
//! reopen    → after close + reopen, every read still holds
//! secret    → SECRET fixtures persist rejected with their refusal reason
//! cohere    → incoherent triples refuse and persist nothing
//! xplan     → an item id never spans two plans
//! envsrc    → divergent envelope sources round-trip byte-identical
//! super     → supersession targets must exist (triple or durable)
//! ```
//!
//! Deliberately NOT in the contract (not promised by the seam):
//!
//! ```text
//! admission → selection decisions are pure domain behavior, pinned in
//!             `context_engine::selection` tests, never re-derived here
//! ranking   → priority order is admission behavior, not storage behavior
//! latency   → no timing promise
//! ```

use context_engine::mint::{mint_item_id, mint_plan_id};
use context_engine::model::*;
use context_engine::port::ContextStorePort;
use context_engine::selection::{admit, Candidate, SelectionInputs};

/// Test-only fixtures shared by store implementers.
pub mod fixtures {
    use super::*;

    const HASH: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    pub fn item(seq: u128, priority: u16, required: bool, sensitivity: Sensitivity) -> ContextItem {
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
            .unwrap()
            .with_transform("normalize".to_owned())
            .with_reopen(ReopenRef::Handle("repo:src/lib.rs@rev1".to_owned())),
        }
    }

    pub fn inputs(plan_seq: u128, candidates: Vec<(ContextItem, u64)>) -> SelectionInputs {
        SelectionInputs {
            plan_id: mint_plan_id(1000, plan_seq),
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
}

use fixtures::{inputs, item};

fn admitted() -> (ContextPlan, Vec<ContextItem>, RetrievalTrace) {
    triple(1, 1)
}

/// One admitted triple with caller-chosen id space (each negative
/// case mints fresh ids so only the targeted check can fire).
fn triple(plan_seq: u128, item_base: u128) -> (ContextPlan, Vec<ContextItem>, RetrievalTrace) {
    let req = item(item_base, 1, true, Sensitivity::Public);
    let hi = item(item_base + 1, 90, false, Sensitivity::Public);
    let lo = item(item_base + 2, 10, false, Sensitivity::Public);
    let secret = item(item_base + 3, 255, false, Sensitivity::Secret);
    let in_ = inputs(
        plan_seq,
        vec![
            (req.clone(), 40),
            (hi.clone(), 40),
            (lo.clone(), 40),
            (secret.clone(), 10),
        ],
    );
    let (plan, trace) = admit(&in_).unwrap();
    (plan, vec![req, hi, lo, secret], trace)
}

/// Runs the full store contract against one implementation.
///
/// `reopen` must return a fresh handle to the same durable state
/// (close + reopen semantics); every read is asserted again after it.
pub fn run_context_store_contract<S: ContextStorePort>(store: &mut S, reopen: impl Fn() -> S) {
    let (plan, items, trace) = admitted();
    store.save_selection(&plan, &items, &trace).unwrap();

    assert_roundtrip(store, &plan, &items, &trace);
    assert_missing(store);
    assert_idempotent_conflict(store, &plan, &items, &trace);
    assert_save_coherence(store);
    assert_cross_plan_refusal(store);
    assert_envelope_source_roundtrip(store);
    assert_supersession_targets(store);

    let fresh = reopen();
    assert_roundtrip(&fresh, &plan, &items, &trace);
    assert_missing(&fresh);
}

fn assert_roundtrip<S: ContextStorePort>(
    store: &S,
    plan: &ContextPlan,
    items: &[ContextItem],
    trace: &RetrievalTrace,
) {
    assert_eq!(store.load_plan(&plan.id).unwrap().as_ref(), Some(plan));
    for item in items {
        assert_eq!(
            store.load_item(&item.id).unwrap().as_ref(),
            Some(item),
            "item {} must round-trip",
            item.id
        );
    }
    assert_eq!(store.load_trace(&plan.id).unwrap().as_ref(), Some(trace));

    let selected = &plan.selected[0];
    let (envelope, decision) = store.explain(&plan.id, selected).unwrap().unwrap();
    let expected_item = items.iter().find(|i| &i.id == selected).unwrap();
    assert_eq!(&envelope, &expected_item.provenance);
    assert_eq!(decision.outcome, DecisionOutcome::Selected);

    let rejected = &plan.rejected[0];
    let (_, decision) = store.explain(&plan.id, &rejected.0).unwrap().unwrap();
    assert_eq!(decision.outcome, DecisionOutcome::Rejected);
    assert_eq!(decision.reason, Some(ReasonCode::SensitivityRefusal));
}

fn assert_missing<S: ContextStorePort>(store: &S) {
    let unknown_plan = mint_plan_id(2000, 999);
    let unknown_item = mint_item_id(2000, 999);
    assert_eq!(store.load_plan(&unknown_plan).unwrap(), None);
    assert_eq!(store.load_item(&unknown_item).unwrap(), None);
    assert_eq!(store.load_trace(&unknown_plan).unwrap(), None);
    assert_eq!(store.explain(&unknown_plan, &unknown_item).unwrap(), None);
}

fn assert_idempotent_conflict<S: ContextStorePort>(
    store: &mut S,
    plan: &ContextPlan,
    items: &[ContextItem],
    trace: &RetrievalTrace,
) {
    store.save_selection(plan, items, trace).unwrap();

    let mut conflicted = plan.clone();
    conflicted.revision_seq += 1;
    let err = store.save_selection(&conflicted, items, trace).unwrap_err();
    assert!(
        matches!(err, ContextError::InvalidInput(_)),
        "conflicting re-save must refuse, got {err}"
    );
}

fn assert_save_coherence<S: ContextStorePort>(store: &mut S) {
    // Trace mirrors the plan: id, fingerprint, cache boundaries.
    let (plan, items, mut trace) = triple(10, 100);
    trace.plan_id = mint_plan_id(1000, 11);
    refuses_without_persist(store, &plan, &items, &trace, "trace plan id");

    let (plan, items, mut trace) = triple(12, 200);
    trace.policy_fingerprint = "policy:other".to_owned();
    refuses_without_persist(store, &plan, &items, &trace, "trace fingerprint");

    let (plan, items, mut trace) = triple(13, 300);
    trace.cache_boundaries.clear();
    refuses_without_persist(store, &plan, &items, &trace, "trace boundaries");

    let (plan, items, mut trace) = triple(22, 1000);
    trace.budget.soft_tokens += 1;
    refuses_without_persist(store, &plan, &items, &trace, "trace budget");

    let (plan, items, mut trace) = triple(23, 1100);
    trace.renderer = "renderer:other".to_owned();
    refuses_without_persist(store, &plan, &items, &trace, "trace renderer");

    // Sections cover exactly the saved items. items[2] is deferred
    // (never a boundary member), so dropping it isolates the section
    // check instead of the boundary check.
    let (plan, items, trace) = triple(14, 400);
    let mut dropped = items.clone();
    dropped.remove(2);
    refuses_without_persist(store, &plan, &dropped, &trace, "section item gap");

    // An extra item outside every section refuses up front.
    let (plan, mut items, mut trace) = triple(24, 1200);
    let extra = item(1204, 5, false, Sensitivity::Public);
    trace.decisions.push(TraceDecision {
        item_id: extra.id.clone(),
        outcome: DecisionOutcome::Selected,
        reason: None,
        rule: "test:unsectioned".to_owned(),
    });
    items.push(extra);
    refuses_without_persist(store, &plan, &items, &trace, "unsectioned item");

    // One id in two sections refuses up front.
    let (mut plan, items, trace) = triple(25, 1300);
    let dup = plan.required[0].clone();
    plan.selected.push(dup);
    refuses_without_persist(store, &plan, &items, &trace, "section overlap");

    // Scalar-invalid items refuse with input taxonomy, not backend.
    let (plan, mut items, trace) = triple(30, 1700);
    items[0].trust.reason = "x".repeat(5000);
    refuses_without_persist(store, &plan, &items, &trace, "scalar overlong");

    // Boundaries cover exactly the rendered set.
    let (mut plan, items, mut trace) = triple(31, 1800);
    let dropped = plan.cache_boundaries[0].items.pop().unwrap();
    let pos = trace.cache_boundaries[0]
        .items
        .iter()
        .position(|id| *id == dropped)
        .unwrap();
    trace.cache_boundaries[0].items.remove(pos);
    refuses_without_persist(store, &plan, &items, &trace, "boundary gap");

    let (mut plan, items, mut trace) = triple(32, 1900);
    let stray = plan.deferred[0].0.clone();
    plan.cache_boundaries[0].items.push(stray.clone());
    trace.cache_boundaries[0].items.push(stray);
    refuses_without_persist(store, &plan, &items, &trace, "boundary stray");

    // Boundary members are saved items (both sides mutated so the
    // plan/trace equality check passes and membership fires).
    let (mut plan, items, mut trace) = triple(15, 500);
    let foreign = mint_item_id(1000, 599);
    plan.cache_boundaries[0].items.push(foreign.clone());
    trace.cache_boundaries[0].items.push(foreign);
    refuses_without_persist(store, &plan, &items, &trace, "boundary item gap");

    // Every item gets exactly one traced decision.
    let (plan, items, mut trace) = triple(16, 600);
    trace.decisions.pop();
    refuses_without_persist(store, &plan, &items, &trace, "decision gap");

    let (plan, items, mut trace) = triple(17, 700);
    let dup = trace.decisions[0].clone();
    trace.decisions.push(dup);
    refuses_without_persist(store, &plan, &items, &trace, "duplicate decision");
}

fn refuses_without_persist<S: ContextStorePort>(
    store: &mut S,
    plan: &ContextPlan,
    items: &[ContextItem],
    trace: &RetrievalTrace,
    case: &str,
) {
    let err = store
        .save_selection(plan, items, trace)
        .expect_err(&format!("{case} must refuse"));
    assert!(
        matches!(err, ContextError::InvalidInput(_)),
        "{case} must refuse with InvalidInput, got {err}"
    );
    assert_eq!(
        store.load_plan(&plan.id).unwrap(),
        None,
        "{case} must persist nothing"
    );
    assert_eq!(
        store.load_item(&items[0].id).unwrap(),
        None,
        "{case} must leave no orphan items"
    );
}

fn assert_cross_plan_refusal<S: ContextStorePort>(store: &mut S) {
    let (first, items, first_trace) = triple(20, 800);
    store.save_selection(&first, &items, &first_trace).unwrap();

    // Same item ids under a fresh plan id: re-selection mints fresh
    // ids, so the store refuses instead of aliasing durable rows.
    let (mut replayed, _, _) = triple(21, 900);
    replayed.required.clone_from(&first.required);
    replayed.selected.clone_from(&first.selected);
    replayed.deferred.clone_from(&first.deferred);
    replayed.rejected.clone_from(&first.rejected);
    replayed
        .cache_boundaries
        .clone_from(&first.cache_boundaries);
    let mut replayed_trace = first_trace.clone();
    replayed_trace.plan_id.clone_from(&replayed.id);
    replayed_trace
        .policy_fingerprint
        .clone_from(&replayed.policy_fingerprint);
    let err = store
        .save_selection(&replayed, &items, &replayed_trace)
        .unwrap_err();
    assert!(
        matches!(err, ContextError::InvalidInput(_)),
        "cross-plan item reuse must refuse, got {err}"
    );
    assert_eq!(store.load_plan(&replayed.id).unwrap(), None);
}

fn assert_envelope_source_roundtrip<S: ContextStorePort>(store: &mut S) {
    // The envelope source is independent of the item source; a
    // divergent pair must persist and load back byte-identical.
    let req = item(1400, 1, true, Sensitivity::Public);
    let mut hi = item(1401, 90, false, Sensitivity::Public);
    hi.provenance.source = SourceRef::decision("req-9").unwrap();
    assert_ne!(hi.source_ref, hi.provenance.source);
    let in_ = inputs(26, vec![(req.clone(), 40), (hi.clone(), 40)]);
    let (plan, trace) = admit(&in_).unwrap();
    let items = vec![req, hi];
    store.save_selection(&plan, &items, &trace).unwrap();
    assert_eq!(
        store.load_item(&items[1].id).unwrap().as_ref(),
        Some(&items[1]),
        "divergent envelope source must round-trip"
    );
}

fn assert_supersession_targets<S: ContextStorePort>(store: &mut S) {
    // Same-triple supersession saves: the target ships in the triple.
    let old = item(1500, 50, false, Sensitivity::Public);
    let mut new = item(1501, 50, false, Sensitivity::Public);
    new.provenance.supersession = Some(Supersession {
        supersedes: old.id.clone(),
        reason: "fresher".to_owned(),
    });
    let in_ = inputs(27, vec![(old.clone(), 10), (new.clone(), 10)]);
    let (plan, trace) = admit(&in_).unwrap();
    let items = vec![old, new];
    store.save_selection(&plan, &items, &trace).unwrap();
    assert_eq!(
        store.load_item(&items[1].id).unwrap().as_ref(),
        Some(&items[1])
    );

    // Cross-plan supersession saves: the target persists already.
    let mut next = item(1502, 50, false, Sensitivity::Public);
    next.provenance.supersession = Some(Supersession {
        supersedes: items[1].id.clone(),
        reason: "fresher still".to_owned(),
    });
    let solo = item(1503, 1, true, Sensitivity::Public);
    let in_ = inputs(28, vec![(solo.clone(), 10), (next.clone(), 10)]);
    let (plan, trace) = admit(&in_).unwrap();
    let items = vec![solo, next];
    store.save_selection(&plan, &items, &trace).unwrap();

    // Nowhere targets refuse and persist nothing.
    let (plan, mut items, trace) = triple(29, 1600);
    items[0].provenance.supersession = Some(Supersession {
        supersedes: mint_item_id(1000, 1699),
        reason: "ghost".to_owned(),
    });
    refuses_without_persist(store, &plan, &items, &trace, "unknown supersedes");
}
