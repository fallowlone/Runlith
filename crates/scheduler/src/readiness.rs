//! Derived scheduling readiness: disposable orchestration view over
//! authoritative Task/dependency state.
//!
//! Canonical vocabulary follows `docs/architecture/task-engine.md` §8
//! (derived examples, never lifecycle): `RUNNABLE`,
//! `WAITING_ON_DEPENDENCY`, and terminal/non-runnable states. Readiness is
//! recomputed from current state on every scheduling decision and is never
//! persisted into the Task row — persisting it would create a second
//! scheduler status competing with the authoritative lifecycle (INV-047).

use task_domain::{TaskId, TaskState};

/// Disposable scheduling view of one Task. This is orchestration evidence,
/// never durable Task state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleReadiness {
    /// `Task == READY` and every required dependency is COMPLETED.
    /// The only state in which the orchestrator may claim the Task.
    Runnable,
    /// `Task == READY` but at least one required dependency is not
    /// COMPLETED. The Task row is untouched: no claim, no Execution, no
    /// Attempt, no executor invocation, no runtime event.
    WaitingOnDependency {
        /// Dependencies blocking execution (not COMPLETED).
        blocking: Vec<TaskId>,
    },
    /// The Task lifecycle state is not executable right now (including
    /// terminal states). Never mutated by scheduling.
    NotRunnable {
        /// Authoritative lifecycle state observed.
        state: TaskState,
    },
}

/// Derives scheduling readiness from authoritative state.
///
/// `state` is the current Task lifecycle state; `blocking` lists required
/// dependencies that are not COMPLETED (empty when none block). The runnable
/// rule is exactly: `state == READY && blocking.is_empty()`.
#[must_use]
pub fn derive_readiness(state: TaskState, blocking: Vec<TaskId>) -> ScheduleReadiness {
    if state != TaskState::Ready {
        return ScheduleReadiness::NotRunnable { state };
    }
    if blocking.is_empty() {
        ScheduleReadiness::Runnable
    } else {
        ScheduleReadiness::WaitingOnDependency { blocking }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use task_domain::{ProjectId, Task};

    fn task_id(id: &str) -> TaskId {
        TaskId::new(id).unwrap()
    }

    #[test]
    fn ready_without_blockers_is_runnable() {
        assert_eq!(
            derive_readiness(TaskState::Ready, Vec::new()),
            ScheduleReadiness::Runnable
        );
    }

    #[test]
    fn ready_with_unfinished_dependency_waits_without_mutation() {
        let readiness = derive_readiness(TaskState::Ready, vec![task_id("T-B")]);
        assert_eq!(
            readiness,
            ScheduleReadiness::WaitingOnDependency {
                blocking: vec![task_id("T-B")]
            }
        );
    }

    #[test]
    fn every_non_ready_lifecycle_state_is_not_runnable() {
        // Census over the canonical twelve: only READY may execute.
        for state in TaskState::all() {
            let readiness = derive_readiness(state, Vec::new());
            if state == TaskState::Ready {
                assert_eq!(readiness, ScheduleReadiness::Runnable, "{state}");
            } else {
                assert_eq!(
                    readiness,
                    ScheduleReadiness::NotRunnable { state },
                    "{state}"
                );
            }
            // Even with no blockers, a non-READY task never runs; even with
            // blockers, a non-READY task reports its own state, not the
            // dependency view.
            let blocked = derive_readiness(state, vec![task_id("T-B")]);
            if state == TaskState::Ready {
                assert!(matches!(
                    blocked,
                    ScheduleReadiness::WaitingOnDependency { .. }
                ));
            } else {
                assert_eq!(blocked, ScheduleReadiness::NotRunnable { state });
            }
        }
    }

    #[test]
    fn readiness_derivation_never_mutates_the_task() {
        let mut task =
            Task::draft(task_id("T-A"), ProjectId::new("P-1").unwrap(), "objective").unwrap();
        task.mark_ready().unwrap();
        let before = task.clone();
        let _ = derive_readiness(task.state(), vec![task_id("T-B")]);
        let _ = derive_readiness(task.state(), Vec::new());
        assert_eq!(task, before, "readiness is a pure derived view");
        assert_eq!(task.state(), TaskState::Ready);
    }
}
