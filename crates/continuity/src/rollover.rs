//! Rollover gating: budget monitor, activation planner (TASK-0171).
//!
//! The Budget Monitor is a Continuity-owned observer over ADR-0042 plan
//! budgets plus provider usage signals (ADR-0043 §4): it may trigger
//! checkpoint and rollover, it never selects content.

use context_engine::model::Budget;

use crate::model::ActivationRecord;

/// Budget Monitor verdict: proceed, start rollover checkpointing, or
/// block new cycles until rollover completes (fail-closed past hard).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetVerdict {
    Ok,
    SoftExceeded,
    HardExceeded,
}

/// Observer over plan budgets plus untrusted provider usage hints.
pub struct BudgetMonitor;

impl BudgetMonitor {
    /// Evaluates consumed tokens against the plan budget. The provider
    /// hint is untrusted: the effective usage is the max, so a low
    /// hint can never hide consumption. An inverted budget (soft >
    /// hard) blocks fail-closed — an unbounded window must never
    /// pretend it was managed.
    #[must_use]
    pub fn evaluate(budget: &Budget, used: u64, hint: Option<u64>) -> BudgetVerdict {
        if budget.soft_tokens > budget.hard_tokens {
            return BudgetVerdict::HardExceeded;
        }
        let effective = used.max(hint.unwrap_or(0));
        if effective >= budget.hard_tokens {
            BudgetVerdict::HardExceeded
        } else if effective >= budget.soft_tokens {
            BudgetVerdict::SoftExceeded
        } else {
            BudgetVerdict::Ok
        }
    }
}

/// Crash-window recovery decision for one activation record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationAction {
    AlreadyComplete,
    CompletePending,
}

/// Plans recovery from an activation record (ADR-0043 §9): a completed
/// activation is a no-op (CR-3 forbids a second successor), anything
/// else completes idempotently.
#[must_use]
pub fn plan_activation(record: &ActivationRecord) -> ActivationAction {
    if record.completed_at.is_some() {
        ActivationAction::AlreadyComplete
    } else {
        ActivationAction::CompletePending
    }
}

/// Retention cutoff instant: rows older than this are purge-eligible.
/// Saturates at zero for durations past the chain's birth.
#[must_use]
pub fn retention_cutoff(now_millis: u64, older_than_ms: u64) -> u64 {
    now_millis.saturating_sub(older_than_ms)
}

#[cfg(test)]
mod tests {
    use context_engine::model::{Budget, CounterMethod};

    use crate::mint::mint_checkpoint_id;
    use crate::model::ActivationRecord;
    use crate::rollover::{retention_cutoff, ActivationAction, BudgetMonitor, BudgetVerdict};

    fn budget(soft: u64, hard: u64) -> Budget {
        Budget {
            soft_tokens: soft,
            hard_tokens: hard,
            counter: CounterMethod::CharHeuristic,
        }
    }

    #[test]
    fn monitor_fires_soft_then_hard() {
        let plan = budget(100, 200);
        assert_eq!(BudgetMonitor::evaluate(&plan, 50, None), BudgetVerdict::Ok);
        assert_eq!(
            BudgetMonitor::evaluate(&plan, 100, None),
            BudgetVerdict::SoftExceeded
        );
        assert_eq!(
            BudgetMonitor::evaluate(&plan, 200, None),
            BudgetVerdict::HardExceeded
        );
    }

    #[test]
    fn monitor_distrusts_low_hint() {
        let plan = budget(100, 200);
        assert_eq!(
            BudgetMonitor::evaluate(&plan, 150, Some(10)),
            BudgetVerdict::SoftExceeded
        );
        assert_eq!(
            BudgetMonitor::evaluate(&plan, 50, Some(250)),
            BudgetVerdict::HardExceeded
        );
    }

    #[test]
    fn monitor_blocks_on_inverted_budget() {
        let inverted = budget(300, 200);
        assert_eq!(
            BudgetMonitor::evaluate(&inverted, 0, None),
            BudgetVerdict::HardExceeded
        );
    }

    #[test]
    fn activation_planner_noops_when_complete() {
        let done = ActivationRecord {
            checkpoint_id: mint_checkpoint_id(1000, 1),
            successor: None,
            started_at: 1000,
            completed_at: Some(1001),
        };
        assert_eq!(
            crate::rollover::plan_activation(&done),
            ActivationAction::AlreadyComplete
        );
        let pending = ActivationRecord {
            completed_at: None,
            ..done
        };
        assert_eq!(
            crate::rollover::plan_activation(&pending),
            ActivationAction::CompletePending
        );
    }

    #[test]
    fn retention_cutoff_saturates_at_zero() {
        assert_eq!(retention_cutoff(1000, 100), 900);
        assert_eq!(retention_cutoff(50, 100), 0);
    }
}
