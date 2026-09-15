//! Trusted application services: the only path that mints authoritative
//! Task lifecycle and Execution/Attempt runtime facts.
//!
//! A provider, agent, model, or frontend can supply evidence and request
//! work, but only this layer — after domain validation — commits a
//! durable mutation together with its Event Journal record in one
//! SQLite transaction (via `storage-sqlite`). `TaskService` owns the
//! Task mutation capability; `RuntimeService` owns the runtime mutation
//! capability (ADR-0022). Neither service decides scheduling, and neither
//! grants Task completion outside the trusted evaluation path.

mod approval_service;
mod clock;
mod error;
mod managed_identity;
mod orchestration_service;
mod planning_service;
mod runtime_service;
mod security_service;
mod task_service;
mod workspace_service;

pub use approval_service::ApprovalService;
pub use clock::{format_rfc3339, Clock, FixedClock, SystemClock};
pub use error::{ServiceError, ServiceResult};
pub use managed_identity::{
    branch_for_execution, decode_readable_slug, is_ascii_casefold_stable, is_canonical_v2_slug,
    is_windows_reserved_basename, managed_slug, MANAGED_DIGEST_PREFIX, MANAGED_READABLE_PREFIX,
    WINDOWS_RESERVED_BASENAMES,
};
pub use orchestration_service::{
    ExecutionEntry, OrchestrationIds, OrchestrationService, PreAttemptResumePlan,
};
pub use planning_service::{
    AcceptedPlanRevision, PlannedOrchestrationError, PlannedOrchestrationOutcome, PlanningCycle,
    PlanningPolicyContext, PlanningProposalOutcome, PlanningService, PlanningServiceError,
};
pub use runtime_service::{AppliedExecution, PlannedExecution, RuntimeService};
pub use security_service::{
    G6Authorization, G6SecurityError, G6SecurityService, PrivilegedOperation, ProcessSecuritySpec,
};
pub use task_service::{AppliedTransition, PlannedTransition, TaskService};
pub use workspace_service::{
    CleanedWorkspace, CleanupOutcome, EnsureOutcome, EnsuredWorkspace, ManagedWorkspaceRoot,
    WorkspaceRequest, WorkspaceService,
};

// Orchestration contract re-exports: these types are the public vocabulary
// of `OrchestrationService::schedule` (injected executor/evaluator ports,
// bounded outcome). Scheduling mechanism itself stays in `scheduler`;
pub use evaluation::{EvaluationPort, EvaluationRequest, EvaluationVerdict};
pub use scheduler::{
    ExecutionPort, ExecutionProblem, OrchestrationOutcome, WorkEvidence, WorkItem,
};
