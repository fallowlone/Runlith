//! Trusted context-selection composition (TASK-0170, ADR-0042).
//!
//! The service counts candidate text through its [`TokenCounter`],
//! admits through pure [`admit`](context_engine::selection::admit),
//! and persists through its [`ContextStorePort`]. Admission and
//! persistence are one service call so a plan is never admitted
//! without its durable evidence — and never persisted without
//! admission.

use std::sync::atomic::{AtomicU64, Ordering};

use context_engine::mint::mint_plan_id;
use context_engine::model::{
    Budget, ContextError, ContextItem, ContextItemId, ContextPlan, ContextPlanId,
    ProvenanceEnvelope, RetrievalTrace, TraceDecision, TrustLevel,
};
use context_engine::port::ContextStorePort;
use context_engine::selection::{admit, Candidate, SelectionInputs, TokenCounter};

/// One admission request: items with the text the counter measures.
#[derive(Debug)]
pub struct SelectionRequest {
    pub revision_seq: u64,
    pub candidates: Vec<(ContextItem, String)>,
    pub soft_tokens: u64,
    pub hard_tokens: u64,
    pub policy_fingerprint: String,
    pub renderer: String,
}

/// Trusted selection service: count, admit, persist, explain.
pub struct ContextService<S, C> {
    store: S,
    counter: C,
    now_millis: u64,
    trust_floor: TrustLevel,
    plan_seq: AtomicU64,
}

impl<S: ContextStorePort, C: TokenCounter> ContextService<S, C> {
    /// Composes the service over its store and counter.
    /// `now_millis` stamps minted plan ids and evaluates freshness;
    /// callers pass wall-clock time, tests pass fixed values for
    /// exact assertions. `trust_floor` rejects below-floor items
    /// even when required.
    pub fn new(store: S, counter: C, now_millis: u64, trust_floor: TrustLevel) -> Self {
        Self {
            store,
            counter,
            now_millis,
            trust_floor,
            plan_seq: AtomicU64::new(0),
        }
    }

    /// Counts, admits and durably persists one selection.
    /// Returns the minted plan id; failures persist nothing.
    pub fn admit_and_persist(
        &mut self,
        request: SelectionRequest,
    ) -> Result<ContextPlanId, ContextError> {
        let seq = self.plan_seq.fetch_add(1, Ordering::SeqCst);
        let plan_id = mint_plan_id(self.now_millis, u128::from(seq));
        let candidates = request
            .candidates
            .into_iter()
            .map(|(item, text)| Candidate {
                item,
                estimated_tokens: self.counter.count(&text),
            })
            .collect();
        let inputs = SelectionInputs {
            plan_id: plan_id.clone(),
            revision_seq: request.revision_seq,
            candidates,
            budget: Budget {
                soft_tokens: request.soft_tokens,
                hard_tokens: request.hard_tokens,
                counter: self.counter.method(),
            },
            policy_fingerprint: request.policy_fingerprint,
            renderer: request.renderer,
            now_millis: self.now_millis,
            trust_floor: self.trust_floor,
        };
        let (plan, trace) = admit(&inputs)?;
        let items: Vec<ContextItem> = inputs.candidates.into_iter().map(|c| c.item).collect();
        self.store.save_selection(&plan, &items, &trace)?;
        Ok(plan_id)
    }

    /// Loads a persisted plan; `None` when unknown.
    pub fn load_plan(&self, id: &ContextPlanId) -> Result<Option<ContextPlan>, ContextError> {
        self.store.load_plan(id)
    }

    /// Loads a persisted trace; `None` when the plan is unknown.
    pub fn load_trace(
        &self,
        plan_id: &ContextPlanId,
    ) -> Result<Option<RetrievalTrace>, ContextError> {
        self.store.load_trace(plan_id)
    }

    /// Explains one item from durable state; `None` when unknown.
    pub fn explain(
        &self,
        plan_id: &ContextPlanId,
        item_id: &ContextItemId,
    ) -> Result<Option<(ProvenanceEnvelope, TraceDecision)>, ContextError> {
        self.store.explain(plan_id, item_id)
    }
}
