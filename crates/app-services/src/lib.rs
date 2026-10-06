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
mod binding_service;
mod candidate_verdict;
mod clock;
mod error;
mod human_decision_service;
mod managed_identity;
mod observed_effect_service;
mod orchestration_service;
mod plan_graph;
mod planned_orchestration;
mod planning_service;
mod preflight_service;
mod presence;
mod requirement_binding;
mod review_admission;
mod review_scope;
mod review_service;
mod runtime_service;
mod security_service;
mod side_effect_recovery;
mod task_service;
mod verification_execution;
mod verification_levels;
mod workspace_service;

pub use approval_service::ApprovalService;
pub use binding_service::{
    BindingService, BindingServiceError, BindingServiceResult, OperatorBindingInput,
};
pub use candidate_verdict::mint_candidate_verdicts;
pub use clock::{format_rfc3339, Clock, FixedClock, SystemClock};
pub use error::{ServiceError, ServiceResult};
pub use human_decision_service::{HumanDecisionCreate, HumanDecisionService};
pub use managed_identity::{
    branch_for_execution, decode_readable_slug, is_ascii_casefold_stable, is_canonical_v2_slug,
    is_windows_reserved_basename, managed_slug, MANAGED_DIGEST_PREFIX, MANAGED_READABLE_PREFIX,
    WINDOWS_RESERVED_BASENAMES,
};
pub use observed_effect_service::{ObservedEffectService, ObservedEffectServiceError};
pub use orchestration_service::{
    ExecutionEntry, OrchestrationIds, OrchestrationService, PreAttemptResumePlan,
};
pub use planned_orchestration::{
    PlannedExecutionEntryOutcome, PlannedOrchestrationError, PlannedOrchestrationOutcome,
};
pub use planning_service::{
    AcceptedPlanRevision, AcceptedTaskPlanResolution, PlanApplicabilityRefusal,
    PlanningAssessmentOutcome, PlanningGuardrailState, PlanningPolicyContext,
    PlanningProposalOutcome, PlanningService, PlanningServiceError,
};
pub use preflight_service::{
    check_prerequisites, PreflightFinding, PreflightReport, PrerequisiteStatus,
};
pub use presence::{
    DenyPresence, FreshPresenceProof, PresenceAuthority, PresenceError, TestPresenceAuthority,
    PRESENCE_FRESHNESS_MS,
};
pub use requirement_binding::{
    bind_candidate_input_params, binding_input_digest, binding_operation, BindingProvenance,
    BoundEvidenceRequirement, ProposedCandidateBinding, RequirementBindingError, BINDER_OPERATOR,
};
pub use review_admission::{
    decide_review_admission, policy_review_path, review_path_name, ReviewAdmissionDecision,
    ReviewAdmissionInput,
};
pub use review_scope::review_scope;
pub use review_service::{
    B1ReviewInput, ReviewAdmissionQuery, ReviewService, ReviewServiceError, ReviewServiceResult,
};
pub use runtime_service::{AppliedExecution, PlannedExecution, RuntimeService};
pub use security_service::{
    G6Authorization, G6SecurityError, G6SecurityService, PrivilegedOperation, ProcessSecuritySpec,
};
pub use task_service::{AppliedTransition, PlannedTransition, TaskService};
pub use verification_execution::{
    default_resolve_verification_program, validate_plan_commands, ExecutableResolver, PlanRefusal,
    VerificationExecutionError, VerificationExecutionRequest, VerificationRunner, VerifiedCommand,
    DEFAULT_STREAM_CAP_BYTES, DEFAULT_VERIFICATION_TIMEOUT_SECS, VERIFICATION_RUNNER_REF,
};
pub use verification_levels::{
    derive_commands, detect_ecosystem, plan_verification, required_kinds_for_level,
    select_verification_level, Ecosystem, RiskTier, VerificationCommand, VerificationLevel,
    VerificationPlan,
};
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
