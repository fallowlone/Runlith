//! Validated opaque reference ids.
//!
//! Every cross-boundary reference in a proposal (branches, nodes, criteria,
//! evidence, planner adapters) is an opaque string id: the planner can name
//! things but can never smuggle a handle, pointer, or mutation capability
//! through one. This is part of the compile-time no-mutation property.

use crate::error::{checked_id, PlanDomainError};

macro_rules! opaque_id {
    ($name:ident, $field:literal) => {
        /// Opaque validated reference id.
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Creates the id, refusing blank or control-character values.
            pub fn new(value: impl Into<String>) -> Result<Self, PlanDomainError> {
                Ok(Self(checked_id($field, value.into())?))
            }

            /// Borrows the id text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

opaque_id!(PlanBranchId, "plan_branch_id");
opaque_id!(PlanNodeId, "plan_node_id");
opaque_id!(CriterionId, "criterion_id");
opaque_id!(EvidenceRef, "evidence_ref");
opaque_id!(PlannerAdapterId, "planner_adapter_id");
opaque_id!(PlanningCostReceiptRef, "planning_cost_receipt_ref");
opaque_id!(PlanningInputSnapshotId, "planning_input_snapshot_id");
opaque_id!(PlanningRunId, "planning_run_id");
