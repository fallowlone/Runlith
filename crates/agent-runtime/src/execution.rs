//! One durable execution of a Task under a concrete assignment.
//!
//! An Execution is a structural container: its identity, its single Task
//! owner, and the ordered, append-only history of its attempts. It owns
//! no Task lifecycle state (no `task_state` shadow copy) and grants no
//! completion: runtime outcomes are evidence, never verdicts.
//!
//! Retry discipline: a retry starts a NEW attempt. Attempts are
//! sequential — at most one open attempt exists — and concluded history
//! is immutable.

use task_domain::TaskId;

use crate::attempt::{Attempt, AttemptOutcome};
use crate::error::{RuntimeError, RuntimeResult};
use crate::id::{AttemptId, ExecutionId, ProviderRef};
use crate::snapshot::ExecutionSnapshot;

/// One durable execution of exactly one Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    id: ExecutionId,
    task_id: TaskId,
    attempts: Vec<Attempt>,
}

impl Execution {
    /// Begins an execution for `task_id` with no attempts yet. The Task
    /// itself is not cloned or shadowed here: only its identity travels
    /// (the Project follows indirectly through Task ownership).
    pub fn begin(id: ExecutionId, task_id: TaskId) -> Self {
        Self {
            id,
            task_id,
            attempts: Vec::new(),
        }
    }

    /// Execution identity, independent from the Task identity.
    #[must_use]
    pub fn id(&self) -> &ExecutionId {
        &self.id
    }

    /// The single Task this execution performs. Fixed at creation.
    #[must_use]
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    /// Ordered attempt history, oldest first. Append-only: entries are
    /// never mutated, removed, or reordered after the fact.
    #[must_use]
    pub fn attempts(&self) -> &[Attempt] {
        &self.attempts
    }

    /// The currently running attempt, if any.
    #[must_use]
    pub fn open_attempt(&self) -> Option<&Attempt> {
        self.attempts.iter().find(|attempt| attempt.is_open())
    }

    /// Looks up one attempt by identity.
    #[must_use]
    pub fn get_attempt(&self, id: &AttemptId) -> Option<&Attempt> {
        self.attempts.iter().find(|attempt| attempt.id() == id)
    }

    /// Starts a new attempt for `provider`. Fails when another attempt
    /// is still open (retries are sequential) or the identity is already
    /// used in this execution. Previous attempts are untouched.
    pub fn start_attempt(
        &mut self,
        id: AttemptId,
        provider: ProviderRef,
    ) -> RuntimeResult<&Attempt> {
        if self.attempts.iter().any(|attempt| attempt.id() == &id) {
            return Err(RuntimeError::DuplicateAttempt(format!(
                "execution {} already contains attempt {id}",
                self.id
            )));
        }
        if let Some(open) = self.open_attempt() {
            return Err(RuntimeError::AttemptAlreadyOpen(format!(
                "execution {} still runs attempt {}",
                self.id,
                open.id()
            )));
        }
        let execution_id = self.id.clone();
        self.attempts
            .push(Attempt::begin(id, execution_id, provider));
        Ok(self.attempts.last().expect("attempt just pushed"))
    }

    /// Records the single conclusion of one open attempt. The outcome and
    /// its evidence note commit together; concluded history is immutable
    /// afterwards. This never touches Task lifecycle state.
    pub fn conclude_attempt(
        &mut self,
        id: &AttemptId,
        outcome: AttemptOutcome,
        detail: impl Into<String>,
    ) -> RuntimeResult<AttemptOutcome> {
        let attempt = self
            .attempts
            .iter_mut()
            .find(|attempt| attempt.id() == id)
            .ok_or_else(|| {
                RuntimeError::AttemptNotFound(format!("execution {} has no attempt {id}", self.id))
            })?;
        attempt.conclude(outcome, detail)
    }

    /// Exports this execution with its ordered attempt history (oldest
    /// first) as a plain-data snapshot for the outbound storage port.
    /// `Vec` order is the aggregate order; the adapter persists an
    /// explicit position from it.
    #[must_use]
    pub fn snapshot(&self) -> ExecutionSnapshot {
        ExecutionSnapshot {
            id: self.id.clone(),
            task_id: self.task_id.clone(),
            attempts: self.attempts.iter().map(Attempt::snapshot).collect(),
        }
    }

    /// Rebuilds an execution from a storage-port snapshot, re-validating
    /// every field through the same constructors and discipline the live
    /// aggregate uses: each attempt must belong to this execution, carry
    /// a unique identity, and at most one attempt may be open. Stored
    /// content that violates discipline fails instead of being repaired.
    pub fn restore(snapshot: ExecutionSnapshot) -> RuntimeResult<Self> {
        let mut execution = Self::begin(snapshot.id.clone(), snapshot.task_id);
        for attempt in snapshot.attempts {
            if attempt.execution_id != snapshot.id {
                return Err(RuntimeError::input(format!(
                    "attempt {} does not belong to execution {}",
                    attempt.id, snapshot.id
                )));
            }
            let restored = Attempt::restore(attempt)?;
            if execution
                .attempts
                .iter()
                .any(|known| known.id() == restored.id())
            {
                return Err(RuntimeError::DuplicateAttempt(format!(
                    "execution {} restores attempt {} twice",
                    execution.id,
                    restored.id()
                )));
            }
            if restored.is_open() && execution.open_attempt().is_some() {
                return Err(RuntimeError::AttemptAlreadyOpen(format!(
                    "execution {} restores two open attempts",
                    execution.id
                )));
            }
            execution.attempts.push(restored);
        }
        Ok(execution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use task_domain::Task;

    fn task_id(id: &str) -> TaskId {
        TaskId::new(id).unwrap()
    }

    fn execution() -> Execution {
        Execution::begin(ExecutionId::new("E-1").unwrap(), task_id("T-1"))
    }

    fn provider(name: &str) -> ProviderRef {
        ProviderRef::new(name).unwrap()
    }

    #[test]
    fn execution_belongs_to_exactly_one_task() {
        let execution = execution();
        assert_eq!(execution.id().as_str(), "E-1");
        assert_eq!(execution.task_id().as_str(), "T-1");
        assert!(execution.attempts().is_empty());
        assert_eq!(execution.open_attempt(), None);
    }

    #[test]
    fn attempts_are_sequential_and_uniquely_identified() {
        let mut execution = execution();
        execution
            .start_attempt(AttemptId::new("A-1").unwrap(), provider("executor-a"))
            .unwrap();
        assert_eq!(execution.open_attempt().unwrap().id().as_str(), "A-1");
        // A second attempt while one is open is rejected; history untouched.
        assert!(matches!(
            execution.start_attempt(AttemptId::new("A-2").unwrap(), provider("executor-a")),
            Err(RuntimeError::AttemptAlreadyOpen(_))
        ));
        assert_eq!(execution.attempts().len(), 1);
        // Conclude, then the retry is a NEW attempt.
        execution
            .conclude_attempt(
                &AttemptId::new("A-1").unwrap(),
                AttemptOutcome::Failed,
                "process disappeared",
            )
            .unwrap();
        execution
            .start_attempt(AttemptId::new("A-2").unwrap(), provider("executor-b"))
            .unwrap();
        assert_eq!(execution.attempts().len(), 2);
        // Duplicate identity is rejected even after conclusion.
        assert!(matches!(
            execution.start_attempt(AttemptId::new("A-1").unwrap(), provider("executor-b")),
            Err(RuntimeError::DuplicateAttempt(_))
        ));
    }

    #[test]
    fn retry_history_preserves_failed_attempts() {
        let mut execution = execution();
        let a1 = AttemptId::new("A-1").unwrap();
        let a2 = AttemptId::new("A-2").unwrap();
        execution
            .start_attempt(a1.clone(), provider("executor-a"))
            .unwrap();
        execution
            .conclude_attempt(&a1, AttemptOutcome::Failed, "quota exhausted")
            .unwrap();
        execution
            .start_attempt(a2.clone(), provider("executor-b"))
            .unwrap();
        execution
            .conclude_attempt(&a2, AttemptOutcome::Succeeded, "evidence produced")
            .unwrap();
        // Attempt 1 remains failed: history is never rewritten as success.
        assert_eq!(
            execution.get_attempt(&a1).unwrap().outcome(),
            Some(AttemptOutcome::Failed)
        );
        assert_eq!(
            execution.get_attempt(&a1).unwrap().detail(),
            Some("quota exhausted")
        );
        assert_eq!(
            execution.get_attempt(&a2).unwrap().outcome(),
            Some(AttemptOutcome::Succeeded)
        );
        // Attempts cannot migrate: both belong to E-1.
        for attempt in execution.attempts() {
            assert_eq!(attempt.execution_id().as_str(), "E-1");
        }
    }

    #[test]
    fn conclude_unknown_or_concluded_attempt_changes_nothing() {
        let mut execution = execution();
        let ghost = AttemptId::new("GHOST").unwrap();
        assert!(matches!(
            execution.conclude_attempt(&ghost, AttemptOutcome::Failed, "x"),
            Err(RuntimeError::AttemptNotFound(_))
        ));
        assert!(execution.attempts().is_empty());
        let a1 = AttemptId::new("A-1").unwrap();
        execution
            .start_attempt(a1.clone(), provider("executor-a"))
            .unwrap();
        execution
            .conclude_attempt(&a1, AttemptOutcome::TimedOut, "no progress")
            .unwrap();
        assert!(matches!(
            execution.conclude_attempt(&a1, AttemptOutcome::Succeeded, "late"),
            Err(RuntimeError::AttemptAlreadyConcluded(_))
        ));
        assert_eq!(
            execution.get_attempt(&a1).unwrap().outcome(),
            Some(AttemptOutcome::TimedOut)
        );
    }

    #[test]
    fn runtime_success_cannot_complete_the_task() {
        // The runtime flow runs to Succeeded while the durable Task —
        // owned exclusively by task-domain/TaskService — never moves.
        let mut task = Task::draft(
            task_id("T-1"),
            task_domain::ProjectId::new("P-1").unwrap(),
            "objective",
        )
        .unwrap();
        let mut execution = Execution::begin(ExecutionId::new("E-1").unwrap(), task.id().clone());
        execution
            .start_attempt(AttemptId::new("A-1").unwrap(), provider("executor-a"))
            .unwrap();
        execution
            .conclude_attempt(
                &AttemptId::new("A-1").unwrap(),
                AttemptOutcome::Succeeded,
                "evidence produced",
            )
            .unwrap();
        // No runtime-domain API accepts a Task, so none could have moved
        // it: the only path to COMPLETED stays the trusted evaluation
        // transition, which this crate cannot invoke.
        assert_eq!(task.state(), task_domain::TaskState::Draft);
        assert!(task.verification_succeeded("runtime said so").is_err());
        assert_eq!(task.state(), task_domain::TaskState::Draft);
    }

    #[test]
    fn every_fake_agent_scenario_remains_representable() {
        // Scenario names are adapter-layer simulation identities; this
        // test only proves the runtime model can RECORD each of them as
        // an opaque outcome detail — without granting them ontology.
        let scenarios = [
            ("success", AttemptOutcome::Succeeded),
            ("crash", AttemptOutcome::Failed),
            ("hang", AttemptOutcome::TimedOut),
            ("timeout", AttemptOutcome::TimedOut),
            ("malformed_result", AttemptOutcome::Failed),
            ("forbidden_path_write", AttemptOutcome::Failed),
            ("forbidden_command", AttemptOutcome::Failed),
            ("quota_reached", AttemptOutcome::Failed),
            ("network_failure", AttemptOutcome::Failed),
            ("partial_result", AttemptOutcome::Failed),
            ("review_rejection", AttemptOutcome::Failed),
        ];
        assert_eq!(scenarios.len(), 11);
        let mut execution = execution();
        for (index, (scenario, outcome)) in scenarios.iter().enumerate() {
            let id = AttemptId::new(format!("A-{index}")).unwrap();
            execution
                .start_attempt(id.clone(), provider("scenario-driver"))
                .unwrap();
            execution
                .conclude_attempt(&id, *outcome, *scenario)
                .unwrap();
            let recorded = execution.get_attempt(&id).unwrap();
            assert_eq!(recorded.outcome(), Some(*outcome));
            assert_eq!(recorded.detail(), Some(*scenario));
        }
        assert_eq!(execution.attempts().len(), 11);
    }
}
