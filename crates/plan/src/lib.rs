//! Pure plan-domain model for G7 adaptive planning (TASK-0076).
//!
//! Transcribes ADR-0027 §§1–5 into executable types. Persistence-free:
//! no SQLite, no Event Journal, no scheduler, no provider calls.
//!
//! Planner output cannot mutate Task state by construction: every type in
//! this crate is owned data (ids, refs, scopes, hashes). The crate depends
//! only on `sha2`; it cannot name Task, storage, scheduler, workspace,
//! policy-engine, or provider types, so no value produced here can carry
//! mutation authority over durable state.

pub mod control;
pub mod error;
pub mod guardrails;
pub mod hash;
pub mod ids;
pub mod model;
pub mod risk;
pub mod scope;
pub mod validation;

pub use control::PlanningInputSnapshot;
pub use error::PlanDomainError;
pub use guardrails::{
    call_within_envelope, cumulative_spend_within_envelope, l3_no_progress_tripped,
    merged_graph_has_leaf, GuardrailTrip, MAX_CHILDREN_PER_DECOMPOSITION, MAX_DECOMPOSITION_DEPTH,
    MAX_NODES_PER_PLAN_BRANCH, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS, MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_OUTPUT_TOKENS,
    MAX_PLANNER_WALL_CLOCK_SECS, MAX_REVISIONS_PER_CYCLE_PER_BRANCH, NO_PROGRESS_REJECTIONS_TRIP,
};
pub use hash::{
    compute_proposal_hash, decomposition_fingerprint, planning_evidence_key, verify_proposal_hash,
};
pub use ids::{
    CriterionId, EvidenceRef, PlanBranchId, PlanNodeId, PlannerAdapterId, PlanningCostReceiptRef,
    PlanningInputSnapshotId, PlanningRunId,
};
pub use model::{
    AtomicityAssessment, AtomicityBlocker, AtomicityRequest, AtomicityVerdict, BudgetDelta,
    DecompositionRequest, EdgeAddition, EdgeRemoval, GateUpdate, NodeAddition, NodeUpdate,
    PlanChangeSet, PlanGraph, PlanNode, PlanRevision, PlannerCallSpend, PlannerFault,
    PlannerIdentity, PlannerPort, PlanningBudget, PlanningCostReceipt, ReplanRequest,
    RevisionProposedBy, ValidatedBy,
};
pub use risk::{CriterionDecl, GateRef, RiskClass, RiskDelta, VerificationMethod};
pub use scope::{PlanScope, ScopeBound};
pub use validation::{
    validate_proposal, AffectedParentCriterion, LimitDetail, RiskEscalationGrant,
    ValidationContext, ValidationOutcome, ValidationRefusal,
};
