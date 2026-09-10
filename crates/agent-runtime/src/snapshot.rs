//! Plain-data images of the runtime aggregates for the outbound storage
//! port, following the `task-domain` snapshot pattern.
//!
//! The snapshots carry no behavior and no storage concepts: no SQL, no
//! row ids, no positions. Attempt order travels as `Vec` order (oldest
//! first); the adapter persists an explicit position column from that
//! order and validates contiguity on load. Rebuilding goes exclusively
//! through [`Execution::restore`] / [`Attempt::restore`], so database
//! content can never bypass domain validation.

use task_domain::TaskId;

use crate::attempt::AttemptOutcome;
use crate::id::{AttemptId, ExecutionId, ProviderRef};

/// Plain-data image of one [`crate::Attempt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptSnapshot {
    /// Attempt identity.
    pub id: AttemptId,
    /// Owning execution; must match the restoring execution.
    pub execution_id: ExecutionId,
    /// Opaque executor reference recorded at attempt start.
    pub provider: ProviderRef,
    /// Recorded conclusion; `None` while open.
    pub outcome: Option<AttemptOutcome>,
    /// Opaque evidence note; `Some` (non-blank) exactly when concluded.
    pub detail: Option<String>,
}

/// Plain-data image of one [`crate::Execution`] with its ordered
/// append-only attempt history (oldest first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSnapshot {
    /// Execution identity.
    pub id: ExecutionId,
    /// The single Task this execution performs.
    pub task_id: TaskId,
    /// Ordered attempt history, oldest first.
    pub attempts: Vec<AttemptSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Execution;

    fn restored() -> Execution {
        let mut execution = Execution::begin(
            ExecutionId::new("E-1").unwrap(),
            TaskId::new("T-1").unwrap(),
        );
        execution
            .start_attempt(
                AttemptId::new("A-1").unwrap(),
                ProviderRef::new("executor-a").unwrap(),
            )
            .unwrap();
        execution
            .conclude_attempt(
                &AttemptId::new("A-1").unwrap(),
                AttemptOutcome::Failed,
                "quota exhausted",
            )
            .unwrap();
        execution
            .start_attempt(
                AttemptId::new("A-2").unwrap(),
                ProviderRef::new("executor-b").unwrap(),
            )
            .unwrap();
        Execution::restore(execution.snapshot()).unwrap()
    }

    #[test]
    fn snapshot_round_trip_preserves_history_and_order() {
        let execution = restored();
        assert_eq!(execution.id().as_str(), "E-1");
        assert_eq!(execution.task_id().as_str(), "T-1");
        let attempts = execution.attempts();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].id().as_str(), "A-1");
        assert_eq!(attempts[0].outcome(), Some(AttemptOutcome::Failed));
        assert_eq!(attempts[0].detail(), Some("quota exhausted"));
        assert_eq!(attempts[1].id().as_str(), "A-2");
        assert!(attempts[1].is_open());
    }

    #[test]
    fn restore_rejects_migrating_and_duplicate_attempts() {
        let mut execution = Execution::begin(
            ExecutionId::new("E-1").unwrap(),
            TaskId::new("T-1").unwrap(),
        );
        execution
            .start_attempt(
                AttemptId::new("A-1").unwrap(),
                ProviderRef::new("executor-a").unwrap(),
            )
            .unwrap();
        let mut snapshot = execution.snapshot();
        // An attempt owned by another execution cannot move here.
        snapshot.attempts[0].execution_id = ExecutionId::new("E-2").unwrap();
        assert!(Execution::restore(snapshot.clone()).is_err());
        // Nor can one identity appear twice.
        snapshot.attempts[0].execution_id = ExecutionId::new("E-1").unwrap();
        snapshot.attempts.push(snapshot.attempts[0].clone());
        assert!(Execution::restore(snapshot).is_err());
    }

    #[test]
    fn restore_rejects_split_outcome_detail() {
        let snapshot = AttemptSnapshot {
            id: AttemptId::new("A-1").unwrap(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            provider: ProviderRef::new("executor-a").unwrap(),
            outcome: Some(AttemptOutcome::Failed),
            detail: None,
        };
        assert!(crate::Attempt::restore(snapshot).is_err());
    }
}
