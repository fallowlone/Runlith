//! Minimal provider-neutral verification seam (TASK-0037, G3).
//!
//! This crate defines the evaluation boundary only: the request shape, the
//! two G3 verdicts, and the port trait. It implements no judge, no score
//! system, no test runner, no scanner — those belong to the G8 Evaluation
//! Engine. For G3 the only implementation is the deterministic
//! `fake-evaluator` adapter.
//!
//! Authority (ADR-0018): the evaluator produces a trusted verdict plus
//! evidence; only the trusted Task application path commits the
//! authoritative lifecycle transition on that verdict. The evaluator never
//! touches Task persistence, runtime persistence, or the journal.

mod error;
mod port;
mod request;
mod verdict;

pub use error::{EvaluationError, EvaluationResult};
pub use port::EvaluationPort;
pub use request::EvaluationRequest;
pub use verdict::EvaluationVerdict;

// Identity re-exports so port implementors depend on this contract crate
// only: request fields stay typed without extra direct dependencies.
pub use agent_runtime::{AttemptId, ExecutionId};
pub use task_domain::TaskId;
