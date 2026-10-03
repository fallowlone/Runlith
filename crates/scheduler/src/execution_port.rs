//! Provider-neutral execution seam for the G3 orchestrator.
//!
//! The orchestrator receives exactly one injected executor per scheduling
//! decision. There is no provider comparison, no quota-aware routing, no
//! model scoring here — those belong to later gates. For G3 tests the
//! injected executor is a thin test-side wrapper around `FakeAgent`; future
//! real adapters implement this same seam without touching orchestration.
//!
//! Adapter-specific failure vocabularies (including every `FakeAgentError`
//! variant) are normalized to [`ExecutionProblem`] at the adapter boundary,
//! never inside orchestration. Scenario names never cross this seam.

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use task_domain::TaskId;

/// One bounded unit of executor work: durable identities only. No prompt
/// text, no transcript, no credentials, no provider-specific fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkItem {
    /// Durable Task under execution (identity only, never mutated here).
    pub task_id: TaskId,
    /// Durable Execution this try belongs to.
    pub execution_id: ExecutionId,
    /// Durable Attempt this execution reports for.
    pub attempt_id: AttemptId,
}

/// Successful executor behavior: bounded operational evidence, never a
/// verdict. `partial == true` means structurally valid but explicitly
/// incomplete — orchestration must never silently promote it to success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkEvidence {
    /// Bounded operational summary (never reasoning, never secrets).
    pub summary: String,
    /// Deterministic evidence reference minted by the executor.
    pub evidence_ref: String,
    /// Whether the result is explicitly incomplete.
    pub partial: bool,
}

/// Provider-neutral execution problem, normalized at the adapter boundary.
/// Adapter error variants (quota, network, crash, capability denial,
/// malformed output, …) arrive as `Failed` with their detail; only the
/// orchestration-relevant timeout/cancelled/transient split keeps its own
/// shape. `Transient` is terminal like `Failed` — the scheduler performs
/// no automatic retry (TASK-0037); the split exists so overload and
/// quota exhaustion stay distinguishable from hard failure in the
/// evidence note, and retry stays an explicit operator/orchestration
/// decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionProblem {
    /// The try made no progress within its bound.
    TimedOut {
        /// Adapter-minted diagnostic note (never a scenario name).
        detail: String,
    },
    /// The try was cancelled before concluding on its own.
    Cancelled,
    /// Rate-limit/quota/overload/5xx signal. Terminal: interprets as
    /// attempt `Failed` with `transient` preserved in the detail.
    Transient {
        /// Adapter-minted diagnostic note (never a scenario name).
        detail: String,
    },
    /// Any other executor/runtime failure. The detail is opaque evidence,
    /// never durable vocabulary.
    Failed {
        /// Adapter-minted diagnostic note (never a scenario name).
        detail: String,
    },
}

/// The single injected executor for one orchestration cycle.
pub trait ExecutionPort {
    /// Opaque executor reference recorded on the durable Attempt. Identity
    /// only — orchestration never branches on it, and it is never a
    /// credential.
    fn provider_ref(&self) -> ProviderRef;

    /// Runs one bounded execution for `work`. Exactly one terminal behavior
    /// per call — never an internal retry. No persistence, no Task access,
    /// no journal writes.
    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem>;
}
