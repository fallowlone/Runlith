//! Evaluation failure: the evaluator could not reach a verdict.
//! Distinct from `Rejected` — a rejection is a verdict, this is the
//! absence of one. The Task stays VERIFYING (durable) so the verdict can
//! be sought again without losing evidence.

use std::fmt;

/// The evaluator could not produce a verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationError {
    detail: String,
}

impl EvaluationError {
    /// Builds an evaluator failure from an adapter-minted diagnostic.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    /// Adapter-minted diagnostic note.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "independent evaluation failed: {}", self.detail)
    }
}

impl std::error::Error for EvaluationError {}

/// Evaluation result alias.
pub type EvaluationResult<T> = Result<T, EvaluationError>;
