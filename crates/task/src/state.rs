//! Canonical Task lifecycle: the single authoritative transition table.
//!
//! State names and the accepted-path edges below come from
//! `docs/architecture/task-engine.md` (§5–§7). Everything that document
//! marks unresolved — all incoming `FAILED` / `CANCELLED` edges, the
//! `SUBMITTED → VERIFYING` bypass, review-rejection paths — is absent here
//! and therefore rejected by [`can_transition`], never guessed.

use std::fmt;

/// Canonical lifecycle state. Names are fixed by the accepted contract —
/// never renamed, added, removed, or reordered here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TaskState {
    Draft,
    Ready,
    Claimed,
    Running,
    Blocked,
    Submitted,
    AwaitingReview,
    Verifying,
    RetryReady,
    Completed,
    Failed,
    Cancelled,
}

impl TaskState {
    /// Every canonical state, in contract order.
    #[must_use]
    pub const fn all() -> [Self; 12] {
        [
            Self::Draft,
            Self::Ready,
            Self::Claimed,
            Self::Running,
            Self::Blocked,
            Self::Submitted,
            Self::AwaitingReview,
            Self::Verifying,
            Self::RetryReady,
            Self::Completed,
            Self::Failed,
            Self::Cancelled,
        ]
    }

    /// Canonical wire/representation name, exactly as accepted
    /// (mirrored by `contracts/json-schema/task.schema.json`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Ready => "READY",
            Self::Claimed => "CLAIMED",
            Self::Running => "RUNNING",
            Self::Blocked => "BLOCKED",
            Self::Submitted => "SUBMITTED",
            Self::AwaitingReview => "AWAITING_REVIEW",
            Self::Verifying => "VERIFYING",
            Self::RetryReady => "RETRY_READY",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
        }
    }

    /// Whether the state is terminal per the accepted contract.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single authoritative transition table: every `(from, to)` pair the
/// accepted architecture establishes (`task-engine.md` §7, "Accepted" rows
/// only). Lifecycle rules live here and nowhere else; callers route through
/// [`can_transition`] or the validated [`Task`](crate::Task) operations.
pub const ACCEPTED_TRANSITIONS: [(TaskState, TaskState); 11] = [
    (TaskState::Draft, TaskState::Ready),
    (TaskState::Ready, TaskState::Claimed),
    (TaskState::Claimed, TaskState::Running),
    (TaskState::Running, TaskState::Blocked),
    (TaskState::Blocked, TaskState::Running),
    (TaskState::Running, TaskState::Submitted),
    (TaskState::Submitted, TaskState::AwaitingReview),
    (TaskState::AwaitingReview, TaskState::Verifying),
    (TaskState::Verifying, TaskState::Completed),
    (TaskState::Verifying, TaskState::RetryReady),
    (TaskState::RetryReady, TaskState::Running),
];

/// Returns whether `from → to` is an accepted lifecycle transition.
///
/// Only [`ACCEPTED_TRANSITIONS`] returns `true`. In particular, no state
/// transitions directly to `COMPLETED` except `VERIFYING`, and nothing
/// leads into `FAILED` / `CANCELLED` yet (all incoming edges unresolved).
#[must_use]
pub fn can_transition(from: TaskState, to: TaskState) -> bool {
    ACCEPTED_TRANSITIONS.contains(&(from, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn census_is_exactly_twelve_canonical_states() {
        let all = TaskState::all();
        assert_eq!(all.len(), 12);
        let names: Vec<_> = all.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            names,
            [
                "DRAFT",
                "READY",
                "CLAIMED",
                "RUNNING",
                "BLOCKED",
                "SUBMITTED",
                "AWAITING_REVIEW",
                "VERIFYING",
                "RETRY_READY",
                "COMPLETED",
                "FAILED",
                "CANCELLED",
            ]
        );
    }

    #[test]
    fn accepted_table_has_eleven_edges() {
        assert_eq!(ACCEPTED_TRANSITIONS.len(), 11);
        for (from, to) in ACCEPTED_TRANSITIONS {
            assert!(can_transition(from, to), "{from} → {to}");
        }
    }

    #[test]
    fn exhaustive_matrix_matches_the_table() {
        for from in TaskState::all() {
            for to in TaskState::all() {
                let expected = ACCEPTED_TRANSITIONS.contains(&(from, to));
                assert_eq!(
                    can_transition(from, to),
                    expected,
                    "matrix mismatch at {from} → {to}"
                );
            }
        }
    }

    #[test]
    fn only_verifying_reaches_completed() {
        for from in TaskState::all() {
            assert_eq!(
                can_transition(from, TaskState::Completed),
                from == TaskState::Verifying,
                "COMPLETED must be reachable only from VERIFYING (from {from})"
            );
        }
    }

    #[test]
    fn failed_and_cancelled_have_no_incoming_edges() {
        for from in TaskState::all() {
            assert!(
                !can_transition(from, TaskState::Failed),
                "no accepted edge into FAILED (from {from})"
            );
            assert!(
                !can_transition(from, TaskState::Cancelled),
                "no accepted edge into CANCELLED (from {from})"
            );
        }
    }

    #[test]
    fn terminal_states_have_no_outgoing_edges() {
        for terminal in [
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ] {
            for to in TaskState::all() {
                assert!(
                    !can_transition(terminal, to),
                    "terminal {terminal} must have no outgoing edge (to {to})"
                );
            }
        }
    }

    #[test]
    fn terminal_classification_matches_contract() {
        for state in TaskState::all() {
            let terminal = matches!(
                state,
                TaskState::Completed | TaskState::Failed | TaskState::Cancelled
            );
            assert_eq!(state.is_terminal(), terminal, "{state}");
        }
    }

    #[test]
    fn schema_names_stay_aligned_with_contract() {
        // Test-time fixture check only: the Rust domain never parses the
        // schema at runtime and no codegen is involved. The schema file is
        // pulled in at compile time so a rename on either side fails here.
        const SCHEMA: &str = include_str!("../../../contracts/json-schema/task.schema.json");
        for state in TaskState::all() {
            let token = format!("\"{}\"", state.as_str());
            assert!(
                SCHEMA.contains(&token),
                "schema must enumerate canonical state {}",
                state.as_str()
            );
        }
    }
}
