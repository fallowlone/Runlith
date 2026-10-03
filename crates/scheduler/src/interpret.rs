//! Executor-behavior interpretation: the orchestration layer maps
//! provider/runtime behavior onto canonical [`AttemptOutcome`] values.
//!
//! This mapping is deliberately narrow: valid complete evidence becomes
//! `Succeeded`; a bounded timeout becomes `TimedOut`; cancellation becomes
//! `Cancelled`; every other failure — including malformed output and
//! explicitly partial results — becomes `Failed` with its evidence note
//! preserved in the attempt detail. No new `AttemptOutcome` variant is
//! created here, and adapter scenario names never appear in this mapping.

use agent_runtime::AttemptOutcome;

use crate::execution_port::{ExecutionProblem, WorkEvidence, WorkItem};

/// Orchestration interpretation of one executor behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptInterpretation {
    /// Structurally valid complete evidence: the attempt concludes
    /// `Succeeded` and the evidence may be submitted for independent
    /// verification. Success here is evidence — never Task completion.
    Submittable {
        /// Attempt conclusion detail (bounded evidence note).
        detail: String,
        /// Submission summary handed to `submit_result`.
        summary: String,
        /// Evidence reference handed to the independent evaluator.
        evidence_ref: String,
    },
    /// The attempt concludes terminally without submission. No automatic
    /// retry is performed: TASK-0037 records one outcome and returns.
    NotSubmittable {
        /// Canonical durable outcome.
        outcome: AttemptOutcome,
        /// Attempt conclusion detail (bounded evidence note).
        detail: String,
    },
}

/// Maps one executor behavior onto its canonical attempt interpretation.
///
/// Matching is on provider-neutral behavior only (`WorkEvidence` flags,
/// [`ExecutionProblem`] variants) — never on adapter scenario names.
#[must_use]
pub fn interpret(
    behavior: Result<WorkEvidence, ExecutionProblem>,
    work: &WorkItem,
) -> AttemptInterpretation {
    match behavior {
        Ok(evidence) if !evidence.partial => AttemptInterpretation::Submittable {
            detail: format!(
                "executor evidence for attempt {} ({})",
                work.attempt_id, evidence.evidence_ref,
            ),
            summary: evidence.summary,
            evidence_ref: evidence.evidence_ref,
        },
        Ok(evidence) => AttemptInterpretation::NotSubmittable {
            outcome: AttemptOutcome::Failed,
            detail: format!(
                "partial executor evidence for attempt {} is not submittable as success ({})",
                work.attempt_id, evidence.evidence_ref,
            ),
        },
        Err(ExecutionProblem::TimedOut { detail }) => AttemptInterpretation::NotSubmittable {
            outcome: AttemptOutcome::TimedOut,
            detail: format!("attempt {} timed out: {detail}", work.attempt_id),
        },
        Err(ExecutionProblem::Cancelled) => AttemptInterpretation::NotSubmittable {
            outcome: AttemptOutcome::Cancelled,
            detail: format!("attempt {} was cancelled", work.attempt_id),
        },
        // F5: terminal like Failed — no new AttemptOutcome, no automatic
        // retry — but the `transient` marker survives in the evidence note
        // so quota/overload stays distinguishable from hard failure.
        Err(ExecutionProblem::Transient { detail }) => AttemptInterpretation::NotSubmittable {
            outcome: AttemptOutcome::Failed,
            detail: format!("attempt {} transient failure: {detail}", work.attempt_id),
        },
        Err(ExecutionProblem::Failed { detail }) => AttemptInterpretation::NotSubmittable {
            outcome: AttemptOutcome::Failed,
            detail: format!("attempt {} failed: {detail}", work.attempt_id),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{AttemptId, ExecutionId};
    use task_domain::TaskId;

    fn work() -> WorkItem {
        WorkItem {
            task_id: TaskId::new("T-1").unwrap(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    }

    fn evidence(partial: bool) -> WorkEvidence {
        WorkEvidence {
            summary: "did the work".to_owned(),
            evidence_ref: "evidence:E-1:A-1".to_owned(),
            partial,
        }
    }

    #[test]
    fn valid_complete_evidence_is_submittable_but_not_completion() {
        let interpretation = interpret(Ok(evidence(false)), &work());
        let AttemptInterpretation::Submittable {
            summary,
            evidence_ref,
            ..
        } = interpretation
        else {
            panic!("complete evidence must be submittable");
        };
        assert_eq!(summary, "did the work");
        assert_eq!(evidence_ref, "evidence:E-1:A-1");
        // Interpretation carries no Task authority: it is evidence plus an
        // outcome, never a lifecycle state.
    }

    #[test]
    fn partial_evidence_is_failed_not_silent_success() {
        let interpretation = interpret(Ok(evidence(true)), &work());
        assert!(
            matches!(
                interpretation,
                AttemptInterpretation::NotSubmittable {
                    outcome: AttemptOutcome::Failed,
                    ..
                }
            ),
            "partial_result != successful submitted result"
        );
    }

    #[test]
    fn timeout_and_cancellation_keep_their_canonical_outcomes() {
        let timed_out = interpret(
            Err(ExecutionProblem::TimedOut {
                detail: "budget exhausted".to_owned(),
            }),
            &work(),
        );
        assert!(matches!(
            timed_out,
            AttemptInterpretation::NotSubmittable {
                outcome: AttemptOutcome::TimedOut,
                ..
            }
        ));
        let cancelled = interpret(Err(ExecutionProblem::Cancelled), &work());
        assert!(matches!(
            cancelled,
            AttemptInterpretation::NotSubmittable {
                outcome: AttemptOutcome::Cancelled,
                ..
            }
        ));
    }

    #[test]
    fn transient_is_failed_with_transient_preserved_in_detail() {
        // F5 RED: the transient problem must stay distinguishable from
        // hard failure in the evidence note (same terminal outcome, no
        // silent retry), so operators can tell quota/overload from bugs.
        let interpretation = interpret(
            Err(ExecutionProblem::Transient {
                detail: "upstream overloaded".to_owned(),
            }),
            &work(),
        );
        let AttemptInterpretation::NotSubmittable {
            outcome,
            detail: kept,
        } = interpretation
        else {
            panic!("transient failure must not be submittable");
        };
        assert_eq!(outcome, AttemptOutcome::Failed);
        assert!(kept.contains("transient"), "got {kept:?}");
        assert!(kept.contains("upstream overloaded"));
    }

    #[test]
    fn every_other_failure_is_failed_with_detail_preserved() {
        for detail in [
            "provider terminated",
            "malformed provider output",
            "capability denied",
            "quota exhausted",
            "network failure",
        ] {
            let interpretation = interpret(
                Err(ExecutionProblem::Failed {
                    detail: detail.to_owned(),
                }),
                &work(),
            );
            let AttemptInterpretation::NotSubmittable {
                outcome,
                detail: kept,
            } = interpretation
            else {
                panic!("failure must not be submittable");
            };
            assert_eq!(outcome, AttemptOutcome::Failed);
            assert!(kept.contains(detail), "evidence note must survive");
        }
    }

    #[test]
    fn no_interpretation_path_mentions_a_task_lifecycle_state() {
        // Static guard: interpretation maps to AttemptOutcome only. Task
        // completion vocabulary (COMPLETED / VERIFYING / RETRY_READY) must
        // never appear as an interpretation result.
        for behavior in [
            Ok(evidence(false)),
            Ok(evidence(true)),
            Err(ExecutionProblem::TimedOut {
                detail: "x".to_owned(),
            }),
            Err(ExecutionProblem::Cancelled),
            Err(ExecutionProblem::Failed {
                detail: "x".to_owned(),
            }),
        ] {
            match interpret(behavior, &work()) {
                AttemptInterpretation::Submittable { .. } => {}
                AttemptInterpretation::NotSubmittable { outcome, .. } => {
                    assert!(matches!(
                        outcome,
                        AttemptOutcome::Succeeded
                            | AttemptOutcome::Failed
                            | AttemptOutcome::TimedOut
                            | AttemptOutcome::Cancelled
                    ));
                }
            }
        }
    }
}
