//! Provider-neutral bounded human-decision control-plane model (TASK-0079).
//!
//! A human decision is durable input to trusted orchestration. It carries no
//! Task, Plan, Policy, approval, provider, or execution mutation capability.

use task_domain::{TaskId, TaskState};

use crate::{
    EvidenceRef, HumanDecisionRequestId, PlanBranchId, PlanDomainError, PlanningInputSnapshotId,
    RiskClass, ScopeBound,
};

pub const HUMAN_DECISION_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HumanContinuation {
    Resume,
    Replan,
    Abort,
}

impl HumanContinuation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resume => "RESUME",
            Self::Replan => "REPLAN",
            Self::Abort => "ABORT",
        }
    }

    pub fn parse(value: &str) -> Result<Self, PlanDomainError> {
        match value {
            "RESUME" => Ok(Self::Resume),
            "REPLAN" => Ok(Self::Replan),
            "ABORT" => Ok(Self::Abort),
            _ => Err(PlanDomainError::InvalidInput(format!(
                "unknown human continuation {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanDecisionReason {
    RiskEscalation,
    NoProgress,
    PlannerBudget,
    PlannerBlocked,
}

impl HumanDecisionReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RiskEscalation => "RISK_ESCALATION",
            Self::NoProgress => "NO_PROGRESS",
            Self::PlannerBudget => "PLANNER_BUDGET",
            Self::PlannerBlocked => "PLANNER_BLOCKED",
        }
    }

    pub fn parse(value: &str) -> Result<Self, PlanDomainError> {
        match value {
            "RISK_ESCALATION" => Ok(Self::RiskEscalation),
            "NO_PROGRESS" => Ok(Self::NoProgress),
            "PLANNER_BUDGET" => Ok(Self::PlannerBudget),
            "PLANNER_BLOCKED" => Ok(Self::PlannerBlocked),
            _ => Err(PlanDomainError::InvalidInput(format!(
                "unknown human decision reason {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HumanDecisionStatus {
    Pending,
    Resolved,
    Consumed,
    Superseded,
}

impl HumanDecisionStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Resolved => "RESOLVED",
            Self::Consumed => "CONSUMED",
            Self::Superseded => "SUPERSEDED",
        }
    }

    pub fn parse(value: &str) -> Result<Self, PlanDomainError> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "RESOLVED" => Ok(Self::Resolved),
            "CONSUMED" => Ok(Self::Consumed),
            "SUPERSEDED" => Ok(Self::Superseded),
            _ => Err(PlanDomainError::InvalidInput(format!(
                "unknown human decision status {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanDecisionPrecondition {
    pub task_state: TaskState,
    pub task_event_seq: u64,
    pub plan_head: u64,
    pub planning_input_snapshot_id: PlanningInputSnapshotId,
    pub planning_input_fingerprint: String,
    pub policy_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanDecisionRequest {
    pub id: HumanDecisionRequestId,
    pub version: u32,
    pub task_id: TaskId,
    pub plan_branch_id: PlanBranchId,
    pub reason: HumanDecisionReason,
    pub risk: RiskClass,
    pub affected_scope: ScopeBound,
    pub options: Vec<HumanContinuation>,
    pub recommendation: Option<HumanContinuation>,
    pub evidence: Vec<EvidenceRef>,
    pub proposal_hash: Option<String>,
    pub precondition: HumanDecisionPrecondition,
    pub created_at_ms: u64,
    pub expires_at_ms: Option<u64>,
    pub status: HumanDecisionStatus,
    pub selected: Option<HumanContinuation>,
    pub decided_by: Option<String>,
    pub decided_at_ms: Option<u64>,
    pub consumed_at_ms: Option<u64>,
}

impl HumanDecisionRequest {
    #[must_use]
    pub fn allows(&self, continuation: HumanContinuation) -> bool {
        self.options.contains(&continuation)
    }

    #[must_use]
    pub fn blocks_scope(&self) -> bool {
        matches!(
            self.status,
            HumanDecisionStatus::Pending | HumanDecisionStatus::Resolved
        ) && !matches!(self.selected, Some(HumanContinuation::Abort))
    }
}
