//! Independent evaluation request: what was submitted, for which durable
//! attempt. Identity plus bounded evidence references — never transcripts,
//! prompts, secrets, or unbounded provider output.

use agent_runtime::{AttemptId, ExecutionId};
use task_domain::TaskId;

/// What the independent evaluator is asked to judge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationRequest {
    /// Durable Task under verification.
    pub task_id: TaskId,
    /// Durable Execution that produced the evidence.
    pub execution_id: ExecutionId,
    /// Durable Attempt (concluded `Succeeded`) carrying the evidence.
    pub attempt_id: AttemptId,
    /// Bounded submission summary from `submit_result`.
    pub summary: String,
    /// Deterministic evidence reference minted by the executor.
    pub evidence_ref: String,
}
