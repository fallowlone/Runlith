//! Live provider-process registry (TASK-0046, G5).
//!
//! Answers "is this execution owner still live?" from real process
//! ownership: the executor registers its execution id at spawn and
//! unregisters at every terminal outcome. The registry is in-memory and
//! refcounted (one entry per live child); it is never persisted, never
//! journalled, and never authoritative beyond process liveness.
//!
//! Wiring (composition root only): share one tracker between the
//! [`crate::ClaudeExecutor`] and the Git backend
//! (`GitCli::with_liveness`), so workspace cleanup refuses teardown of a
//! live execution's workspace through the provider-neutral
//! [`workspace::OwnerLiveness`] seam. No provider types cross that seam —
//! just the durable execution id.

use std::collections::HashMap;
use std::sync::Mutex;

use agent_runtime::ExecutionId;
use workspace::OwnerLiveness;

/// In-memory registry of executions with at least one live provider
/// child. Clone-shares the same table; thread-safe.
#[derive(Debug, Clone, Default)]
pub struct LiveProcessTracker {
    live: std::sync::Arc<Mutex<HashMap<String, usize>>>,
}

impl LiveProcessTracker {
    /// Empty registry.
    #[must_use]
    pub fn fresh() -> Self {
        Self::default()
    }

    /// Records one live child for `owner`. Balances exactly one
    /// [`Self::mark_finished`] call on the terminal path.
    pub fn mark_live(&self, owner: &ExecutionId) {
        if let Ok(mut live) = self.live.lock() {
            *live.entry(owner.as_str().to_owned()).or_insert(0) += 1;
        }
    }

    /// Retires one live child for `owner`. Saturates at zero: an
    /// unbalanced call (lock poisoning excluded) can never underflow or
    /// panic — liveness is advisory, cleanup safety never rests on exact
    /// counts alone.
    pub fn mark_finished(&self, owner: &ExecutionId) {
        if let Ok(mut live) = self.live.lock() {
            match live.get_mut(owner.as_str()) {
                Some(count) if *count > 1 => *count -= 1,
                _ => {
                    live.remove(owner.as_str());
                }
            }
        }
    }

    /// Whether `owner` currently has at least one registered live child.
    #[must_use]
    pub fn is_live_execution(&self, owner: &ExecutionId) -> bool {
        self.live
            .lock()
            .is_ok_and(|live| live.get(owner.as_str()).is_some_and(|count| *count > 0))
    }
}

impl OwnerLiveness for LiveProcessTracker {
    fn is_live(&self, owner: &ExecutionId) -> bool {
        self.is_live_execution(owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execution(id: &str) -> ExecutionId {
        ExecutionId::new(id).unwrap()
    }

    #[test]
    fn liveness_follows_registration_lifecycle() {
        let tracker = LiveProcessTracker::fresh();
        let owner = execution("E-1");
        assert!(!tracker.is_live_execution(&owner));
        tracker.mark_live(&owner);
        assert!(tracker.is_live_execution(&owner));
        tracker.mark_finished(&owner);
        assert!(!tracker.is_live_execution(&owner));
    }

    #[test]
    fn refcounting_survives_nested_attempts_and_unbalanced_calls() {
        let tracker = LiveProcessTracker::fresh();
        let owner = execution("E-1");
        tracker.mark_live(&owner);
        tracker.mark_live(&owner);
        tracker.mark_finished(&owner);
        assert!(tracker.is_live_execution(&owner));
        tracker.mark_finished(&owner);
        assert!(!tracker.is_live_execution(&owner));
        // Unbalanced finish never panics, never goes negative.
        tracker.mark_finished(&owner);
        assert!(!tracker.is_live_execution(&owner));
    }

    #[test]
    fn owners_are_isolated() {
        let tracker = LiveProcessTracker::fresh();
        tracker.mark_live(&execution("E-1"));
        assert!(!tracker.is_live_execution(&execution("E-2")));
    }

    #[test]
    fn clones_share_one_table() {
        let tracker = LiveProcessTracker::fresh();
        let shared = tracker.clone();
        shared.mark_live(&execution("E-1"));
        assert!(tracker.is_live_execution(&execution("E-1")));
    }
}
