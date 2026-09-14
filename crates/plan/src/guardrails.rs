//! Recursion/liveness guardrails (ADR-0027 §5) and the planner call budget
//! envelope (ADR-0027 §6).
//!
//! Bounds are v1 defaults owned by planning policy (mechanism/policy split,
//! INV-112): tunable without domain rewrites, never tunable by the planner
//! itself. Numeric values MUST match ADR-0027 exactly; re-tuning needs an
//! ADR amendment, not a silent edit.

use std::collections::{BTreeMap, BTreeSet};

/// Maximum decomposition depth: branch root → deepest leaf, in edges.
pub const MAX_DECOMPOSITION_DEPTH: u32 = 4;
/// Maximum new children per decomposition (one revision stays reviewable).
pub const MAX_CHILDREN_PER_DECOMPOSITION: usize = 8;
/// Maximum total nodes in the affected branch after merge.
pub const MAX_NODES_PER_PLAN_BRANCH: usize = 64;
/// Maximum accepted revisions per reconcile cycle per branch.
pub const MAX_REVISIONS_PER_CYCLE_PER_BRANCH: u32 = 1;
/// Consecutive same-branch/same-evidence rejections that trip L3.
pub const NO_PROGRESS_REJECTIONS_TRIP: u32 = 2;

/// Per-`PlannerPort`-call wall-clock bound, in seconds.
pub const MAX_PLANNER_WALL_CLOCK_SECS: u64 = 120;
/// Per-call input token bound.
pub const MAX_PLANNER_INPUT_TOKENS: u32 = 32_000;
/// Per-call output token bound.
pub const MAX_PLANNER_OUTPUT_TOKENS: u32 = 8_000;
/// Per-triggering-evidence-event per-branch planner-call bound.
pub const MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH: u32 = 8;
/// Per-triggering-evidence-event per-branch cumulative input-token bound.
pub const MAX_PLANNER_CUMULATIVE_INPUT_TOKENS: u32 = 256_000;

/// A tripped guardrail: typed, never silent (ADR-0027 §5).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardrailTrip {
    /// Which bound tripped (e.g. `"depth"`, `"fan-out"`, `"branch-nodes"`,
    /// `"revisions-per-cycle"`, `"planner-calls"`, `"planner-input-tokens"`).
    pub bound: &'static str,
    /// Observed value that exceeded the bound.
    pub observed: u64,
}

impl GuardrailTrip {
    /// Records a trip.
    #[must_use]
    pub const fn new(bound: &'static str, observed: u64) -> Self {
        Self { bound, observed }
    }
}

/// L3 no-progress trip (ADR-0027 §5): 2 consecutive rejections on the same
/// branch with the same triggering evidence BLOCKs the branch and escalates
/// to a `HumanDecisionRequest` (REPLAN/ABORT).
#[must_use]
pub const fn l3_no_progress_tripped(consecutive_rejections: u32) -> bool {
    consecutive_rejections >= NO_PROGRESS_REJECTIONS_TRIP
}

/// Checks one planner call against the §6 per-call envelope (wall clock,
/// input tokens, output tokens). Exhaustion → BLOCKED + HDR, no silent spend.
#[must_use]
pub fn call_within_envelope(receipt: &crate::model::PlanningCostReceipt) -> bool {
    receipt.wall_time_ms <= MAX_PLANNER_WALL_CLOCK_SECS * 1_000
        && receipt.input_tokens <= MAX_PLANNER_INPUT_TOKENS
        && receipt.output_tokens <= MAX_PLANNER_OUTPUT_TOKENS
}

/// Checks cumulative per-evidence-event per-branch spend against the §6
/// envelope. Exhaustion → BLOCKED + HDR, no silent spend.
#[must_use]
pub const fn cumulative_spend_within_envelope(
    calls_made: u32,
    cumulative_input_tokens: u32,
) -> bool {
    calls_made <= MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH
        && cumulative_input_tokens <= MAX_PLANNER_CUMULATIVE_INPUT_TOKENS
}

/// L4 structural pre-check (ADR-0027 §5): the merged branch must retain at
/// least one leaf node. Necessary but NOT sufficient for L4 — derived
/// RUNNABLE readiness (ADR-0017 §21.1.3) is scheduler-owned and checked
/// against durable state by TASK-0077/0078; this only rejects revisions
/// that cannot possibly leave executable work.
#[must_use]
pub fn merged_graph_has_leaf(
    node_ids: &BTreeSet<&str>,
    edges: &BTreeMap<&str, BTreeSet<&str>>,
) -> bool {
    node_ids.iter().any(|node| {
        edges
            .get(*node)
            .is_none_or(std::collections::BTreeSet::is_empty)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PlanningCostReceipt;

    fn receipt(wall_ms: u64, input: u32, output: u32) -> PlanningCostReceipt {
        PlanningCostReceipt::new(input, output, wall_ms, "standard".to_owned())
            .expect("valid receipt")
    }

    /// ADR-0027 §5 L3: "2 consecutive rejections on the same branch with the
    /// same triggering evidence BLOCK the branch and escalate into a
    /// `HumanDecisionRequest` (REPLAN/ABORT)."
    #[test]
    fn l3_trips_at_exactly_two_consecutive_rejections() {
        assert_eq!(NO_PROGRESS_REJECTIONS_TRIP, 2);
        assert!(!l3_no_progress_tripped(0));
        assert!(!l3_no_progress_tripped(1));
        assert!(l3_no_progress_tripped(2));
        assert!(l3_no_progress_tripped(3));
    }

    /// ADR-0027 §6 envelope: per-call wall clock, input tokens and output
    /// tokens; at-bound spend is allowed, any overage is not.
    #[test]
    fn call_envelope_accepts_at_bounds_and_rejects_overage_on_each_axis() {
        let at_bounds = receipt(
            MAX_PLANNER_WALL_CLOCK_SECS * 1_000,
            MAX_PLANNER_INPUT_TOKENS,
            MAX_PLANNER_OUTPUT_TOKENS,
        );
        assert!(call_within_envelope(&at_bounds));
        assert!(!call_within_envelope(&receipt(
            MAX_PLANNER_WALL_CLOCK_SECS * 1_000 + 1,
            0,
            0
        )));
        assert!(!call_within_envelope(&receipt(
            0,
            MAX_PLANNER_INPUT_TOKENS + 1,
            0
        )));
        assert!(!call_within_envelope(&receipt(
            0,
            0,
            MAX_PLANNER_OUTPUT_TOKENS + 1
        )));
    }

    /// ADR-0027 §6 envelope: cumulative per-evidence-event per-branch spend;
    /// at-bound spend is allowed, any overage is not.
    #[test]
    fn cumulative_envelope_accepts_at_bounds_and_rejects_overage_on_each_axis() {
        assert!(cumulative_spend_within_envelope(
            MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            MAX_PLANNER_CUMULATIVE_INPUT_TOKENS
        ));
        assert!(!cumulative_spend_within_envelope(
            MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH + 1,
            0
        ));
        assert!(!cumulative_spend_within_envelope(
            0,
            MAX_PLANNER_CUMULATIVE_INPUT_TOKENS + 1
        ));
    }

    /// ADR-0027 §5 L4 structural pre-check: the merged branch must retain at
    /// least one executable leaf.
    #[test]
    fn merged_branch_without_leaves_is_rejected_and_leaf_passes() {
        let (a, b) = ("a", "b");
        // Every node keeps an outgoing edge (a -> b, b -> a): no leaf remains.
        let mut edges = BTreeMap::new();
        edges.insert(a, BTreeSet::from([b]));
        edges.insert(b, BTreeSet::from([a]));
        assert!(!merged_graph_has_leaf(&BTreeSet::from([a, b]), &edges));
        assert!(merged_graph_has_leaf(
            &BTreeSet::from([a]),
            &BTreeMap::new()
        ));
        assert!(!merged_graph_has_leaf(&BTreeSet::new(), &BTreeMap::new()));
    }
}
