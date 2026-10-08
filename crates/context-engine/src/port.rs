//! Durable selection seam: plans, items and traces behind a port.
//!
//! The port promises round-trip persistence with reopen-after-restart
//! durability. Admission stays pure ([`crate::selection`]); the store
//! never invents decisions, sections or reasons.

use crate::model::{
    ContextError, ContextItem, ContextItemId, ContextPlan, ContextPlanId, ProvenanceEnvelope,
    RetrievalTrace, TraceDecision,
};

/// Durable store for admitted selections.
pub trait ContextStorePort {
    /// Persists one admitted (plan, items, trace) triple atomically.
    ///
    /// Idempotent for byte-identical retries; a conflicting payload
    /// under an existing plan id refuses. Takes `&mut self` like every
    /// other SQLite write path in the adapter. The triple must cohere
    /// (items validate; every saved item sits in exactly one plan
    /// section; boundaries cover exactly the rendered items; decisions
    /// reference exactly the saved items; the trace mirrors the plan
    /// id, budget, renderer, fingerprint, and cache boundaries;
    /// supersession targets exist) or the call refuses without
    /// persisting.
    fn save_selection(
        &mut self,
        plan: &ContextPlan,
        items: &[ContextItem],
        trace: &RetrievalTrace,
    ) -> Result<(), ContextError>;

    /// Loads a plan by id; `None` when unknown (never invented).
    fn load_plan(&self, id: &ContextPlanId) -> Result<Option<ContextPlan>, ContextError>;

    /// Loads an item by id; `None` when unknown.
    fn load_item(&self, id: &ContextItemId) -> Result<Option<ContextItem>, ContextError>;

    /// Loads a plan's trace; `None` when the plan is unknown.
    fn load_trace(&self, plan_id: &ContextPlanId) -> Result<Option<RetrievalTrace>, ContextError>;

    /// Explains one item: its envelope plus its traced decision.
    /// Answers "why did this item enter context?" from durable state.
    fn explain(
        &self,
        plan_id: &ContextPlanId,
        item_id: &ContextItemId,
    ) -> Result<Option<(ProvenanceEnvelope, TraceDecision)>, ContextError>;
}
