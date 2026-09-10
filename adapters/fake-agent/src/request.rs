//! Provider-neutral execution request shape.
//!
//! The same minimal identities future real adapters receive: which durable
//! Task is being executed, under which Execution, for which Attempt. No
//! provider-specific fields, no session transcript, no credentials, no
//! prompt text. The [`crate::FakeScenario`] selector accompanies — never
//! enters — this request.

use agent_runtime::{AttemptId, ExecutionId};
use task_domain::TaskId;

/// Minimal canonical request handed to an execution implementation.
///
/// Identity-only on purpose: orchestration owns scheduling and scope;
/// the executor reports behavior for exactly this Attempt and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequest {
    /// Durable Task under execution (identity only, never mutated here).
    pub task_id: TaskId,
    /// Durable Execution this try belongs to.
    pub execution_id: ExecutionId,
    /// Durable Attempt this execution reports for.
    pub attempt_id: AttemptId,
}

impl ExecutionRequest {
    /// Builds a request from already-validated durable identities.
    #[must_use]
    pub fn new(task_id: TaskId, execution_id: ExecutionId, attempt_id: AttemptId) -> Self {
        Self {
            task_id,
            execution_id,
            attempt_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_only_durable_identities() {
        let request = ExecutionRequest::new(
            TaskId::new("T-1").unwrap(),
            ExecutionId::new("E-1").unwrap(),
            AttemptId::new("A-1").unwrap(),
        );
        assert_eq!(request.task_id.as_str(), "T-1");
        assert_eq!(request.execution_id.as_str(), "E-1");
        assert_eq!(request.attempt_id.as_str(), "A-1");
        assert_eq!(request.clone(), request);
    }
}
