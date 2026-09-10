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

mod clock;
mod error;
mod orchestration_service;
mod runtime_service;
mod task_service;

pub use clock::{format_rfc3339, Clock, FixedClock, SystemClock};
pub use error::{ServiceError, ServiceResult};
pub use orchestration_service::{OrchestrationIds, OrchestrationService};
pub use runtime_service::{AppliedExecution, RuntimeService};
pub use task_service::{AppliedTransition, TaskService};

// Orchestration contract re-exports: these types are the public vocabulary
// of `OrchestrationService::schedule` (injected executor/evaluator ports,
// bounded outcome). Scheduling mechanism itself stays in `scheduler`;
pub use evaluation::{EvaluationPort, EvaluationRequest, EvaluationVerdict};
pub use scheduler::{
    ExecutionPort, ExecutionProblem, OrchestrationOutcome, WorkEvidence, WorkItem,
};
