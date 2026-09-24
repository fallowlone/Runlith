//! Bounded orchestration outcome: what one scheduling cycle decided.
//!
//! This is orchestration evidence for the caller, not Task lifecycle
//! authority. It deliberately stays small — no permanent scheduler state
//! taxonomy is created here. Durable facts (Task state, Execution/Attempt
//! rows, journal events) remain readable from their canonical owners.

use agent_runtime::{AttemptId, AttemptOutcome, ExecutionId};
use task_domain::{TaskId, TaskState};

/// What one bounded `schedule` cycle concluded. At most one Execution and
/// one Attempt are ever created per cycle; there is no retry loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestrationOutcome {
    /// The Task lifecycle state is not executable. Nothing was claimed,
    /// created, or invoked.
    NotRunnable {
        /// Task identity.
        task_id: TaskId,
        /// Authoritative lifecycle state observed.
        state: TaskState,
    },
    /// The Task is READY but a dependency is not COMPLETED. Nothing was
    /// claimed, created, or invoked.
    WaitingOnDependency {
        /// Task identity.
        task_id: TaskId,
        /// Dependencies blocking execution (not COMPLETED).
        blocking: Vec<TaskId>,
    },
    /// Full G3 path: executor evidence submitted, routed, verified, and
    /// the trusted verification-success transition committed COMPLETED.
    ExecutionSucceededAndVerified {
        /// Task identity.
        task_id: TaskId,
        /// Durable execution created by this cycle.
        execution_id: ExecutionId,
        /// Durable attempt concluded `Succeeded` by this cycle.
        attempt_id: AttemptId,
    },
    /// Executor evidence was valid and the Attempt stays `Succeeded`, but
    /// the independent evaluator rejected it: the trusted rejection
    /// transition committed the canonical retry state. Attempt success
    /// plus evaluation rejection is valid and expected.
    ExecutionSucceededButRejected {
        /// Task identity.
        task_id: TaskId,
        /// Durable execution created by this cycle.
        execution_id: ExecutionId,
        /// Durable attempt (still `Succeeded`) created by this cycle.
        attempt_id: AttemptId,
    },
    /// The executor behavior concluded the Attempt terminally without
    /// submission (failure, timeout, partial, …). Exactly one Attempt was
    /// recorded; no retry was attempted. The caller can see durable state
    /// exists from the returned identities.
    ExecutionFailed {
        /// Task identity.
        task_id: TaskId,
        /// Durable execution created by this cycle.
        execution_id: ExecutionId,
        /// Durable attempt concluded by this cycle.
        attempt_id: AttemptId,
        /// Canonical terminal outcome recorded.
        outcome: AttemptOutcome,
    },
    /// The execution was cancelled through the canonical path. Exactly one
    /// Attempt was recorded as `Cancelled`; no retry was attempted.
    Cancelled {
        /// Task identity.
        task_id: TaskId,
        /// Durable execution created by this cycle.
        execution_id: ExecutionId,
        /// Durable attempt concluded `Cancelled` by this cycle.
        attempt_id: AttemptId,
    },
    /// TASK-0135 (G8): declared prerequisites failed preflight after
    /// admission but before any Attempt was created. The trusted journaled
    /// `Block` transition committed with the bounded blocker report as its
    /// reason, so the report persists durably on the task record. No
    /// provider was invoked; recovery is the existing
    /// admitted-but-not-attempted resume path once the prerequisite is
    /// satisfied. `Indeterminate` findings never produce this outcome -
    /// only positively `Failed` checks block.
    BlockedByPrerequisite {
        /// Task identity.
        task_id: TaskId,
        /// Durable execution created by this cycle (zero attempts).
        execution_id: ExecutionId,
        /// Bounded human-readable blocker report (journaled as the Block
        /// reason).
        reason: String,
    },
}
