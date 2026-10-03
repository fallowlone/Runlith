//! The independent evaluation port: replaceable verification authority.
//!
//! The component that generated the work must never implement this port for
//! its own output. For G3 the implementation is `fake-evaluator`; G8 adds
//! real verifiers behind this same seam without touching completion
//! authority.

use crate::{EvaluationRequest, EvaluationResult, EvaluationVerdict};

/// Independent verification authority over submitted evidence.
pub trait EvaluationPort {
    /// Judges `request` and returns a trusted verdict. Pure judgment: no
    /// persistence, no Task mutation, no journal writes. An `Err` means no
    /// verdict was reached (the Task stays VERIFYING); only `Rejected`
    /// routes to retry.
    fn evaluate(&self, request: &EvaluationRequest) -> EvaluationResult<EvaluationVerdict>;

    /// Revalidates any evaluator-specific freshness conditions immediately
    /// before the orchestration service applies a `Verified` verdict.
    /// Fail-closed default: evaluators without a deferred completion
    /// condition must override with an explicit `Ok(())`. A missing
    /// override refuses completion instead of silently passing it.
    fn revalidate_before_completion(&self, _request: &EvaluationRequest) -> EvaluationResult<()> {
        Err(crate::EvaluationError::new(
            "revalidation not implemented for this evaluator; refusing completion",
        ))
    }
}
