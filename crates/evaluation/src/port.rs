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
}
