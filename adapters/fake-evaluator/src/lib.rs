//! Deterministic independent evaluation fixture (TASK-0037, G3).
//!
//! [`FakeEvaluator`] simulates the independent verification authority with
//! a fixed verdict (`always_verified` / `always_rejected`). No AI, no
//! shell, no filesystem execution, no network.
//!
//! This is not FakeAgent: FakeAgent simulates the producer/executor, this
//! crate simulates the verification authority. One type never implements
//! both roles — this crate depends on the `evaluation` contract only and
//! must never depend on `fake-agent`, `storage-sqlite`, `TaskService`,
//! `RuntimeService`, or `rusqlite` (`cargo tree -p fake-evaluator` proves
//! the separation structurally).

use std::sync::atomic::{AtomicUsize, Ordering};

use evaluation::{EvaluationPort, EvaluationRequest, EvaluationResult, EvaluationVerdict};

/// Deterministic verdict mode for one evaluator instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluatorMode {
    /// Every evaluation returns [`EvaluationVerdict::Verified`].
    AlwaysVerified,
    /// Every evaluation returns [`EvaluationVerdict::Rejected`].
    AlwaysRejected,
}

/// Deterministic independent evaluation fixture.
///
/// Stateless verdict logic plus an atomic invocation counter, so tests can
/// prove the evaluator ran exactly when expected (and never on blocked,
/// failed, malformed, or partial paths). Shares no mutable global state:
/// two evaluators in one test process never interfere.
#[derive(Debug)]
pub struct FakeEvaluator {
    mode: EvaluatorMode,
    calls: AtomicUsize,
}

impl FakeEvaluator {
    /// Builds an isolated fixture for one deterministic verdict.
    #[must_use]
    pub fn new(mode: EvaluatorMode) -> Self {
        Self {
            mode,
            calls: AtomicUsize::new(0),
        }
    }

    /// How many times this instance was asked for a verdict.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl EvaluationPort for FakeEvaluator {
    fn evaluate(&self, _request: &EvaluationRequest) -> EvaluationResult<EvaluationVerdict> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(match self.mode {
            EvaluatorMode::AlwaysVerified => EvaluationVerdict::Verified,
            EvaluatorMode::AlwaysRejected => EvaluationVerdict::Rejected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evaluation::{AttemptId, ExecutionId, TaskId};

    fn request() -> EvaluationRequest {
        EvaluationRequest {
            task_id: TaskId::new("T-1").unwrap(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            summary: "did the work".to_owned(),
            evidence_ref: "fake-evidence:success:E-1:A-1".to_owned(),
        }
    }

    #[test]
    fn modes_return_their_fixed_verdicts() {
        let verified = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        let rejected = FakeEvaluator::new(EvaluatorMode::AlwaysRejected);
        assert_eq!(
            verified.evaluate(&request()).unwrap(),
            EvaluationVerdict::Verified
        );
        assert_eq!(
            rejected.evaluate(&request()).unwrap(),
            EvaluationVerdict::Rejected
        );
    }

    #[test]
    fn invocations_are_counted_per_instance() {
        let first = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        let second = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        assert_eq!(first.call_count(), 0);
        first.evaluate(&request()).unwrap();
        first.evaluate(&request()).unwrap();
        assert_eq!(first.call_count(), 2);
        assert_eq!(second.call_count(), 0, "no shared mutable global state");
    }
}
