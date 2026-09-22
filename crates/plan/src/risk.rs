//! Risk and proof-method vocabulary (V3/V5/V9).
//!
//! `RiskClass` ordering (`Low < Medium < High < Critical`) is v1 vocabulary
//! owned by this crate; tuning what each class permits belongs to planning
//! policy (INV-112 mechanism/policy split), never to the planner itself.
//! `VerificationMethod` is the closed ADR-0027 vocabulary: a covering child
//! criterion always carries one of these, so "no proof method" is
//! unrepresentable and V3 only checks preservation, never presence.

use std::str::FromStr;

use task_domain::RequirementKind;

use crate::error::{checked_id, PlanDomainError};
use crate::ids::PlanNodeId;

/// Ordered risk class. Higher variants demand stronger gates (V5/V9).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RiskClass {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn parse(value: &str) -> Result<Self, PlanDomainError> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            other => Err(PlanDomainError::input(format!(
                "unknown risk class {other:?}"
            ))),
        }
    }
}

/// Closed proof-method vocabulary from ADR-0027 V3.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VerificationMethod {
    AutomatedTest,
    IndependentReview,
    SecurityCheck,
    HumanApproval,
}

impl VerificationMethod {
    /// Canonical text form used in canonical hashing.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutomatedTest => "automated-test",
            Self::IndependentReview => "independent-review",
            Self::SecurityCheck => "security-check",
            Self::HumanApproval => "human-approval",
        }
    }
}

impl FromStr for VerificationMethod {
    type Err = PlanDomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "automated-test" => Ok(Self::AutomatedTest),
            "independent-review" => Ok(Self::IndependentReview),
            "security-check" => Ok(Self::SecurityCheck),
            "human-approval" => Ok(Self::HumanApproval),
            other => Err(PlanDomainError::input(format!(
                "unknown verification method {other:?}"
            ))),
        }
    }
}

/// ADR-0030 §3: the planning-side proof method a proposal must declare for a
/// canonical Task requirement kind (materialization direction). One-directional
/// vocabulary reconciliation — never a strength ranking.
#[must_use]
pub const fn requirement_kind_for_method(method: VerificationMethod) -> RequirementKind {
    match method {
        VerificationMethod::AutomatedTest => RequirementKind::TestSuite,
        VerificationMethod::IndependentReview => RequirementKind::IndependentReview,
        VerificationMethod::SecurityCheck => RequirementKind::SecurityCheck,
        VerificationMethod::HumanApproval => RequirementKind::HumanApproval,
    }
}

/// Inverse reading (derivation direction, V3/V9): the planning-side proof
/// method a canonical Task's explicit evidence requirement demands. Every
/// capture kind other than `test-suite` is still an `automated-test`
/// obligation on the planning side; the kinds stay distinct in the Task
/// contract.
#[must_use]
pub const fn method_for_requirement_kind(kind: RequirementKind) -> VerificationMethod {
    match kind {
        RequirementKind::TestSuite
        | RequirementKind::ProcessExit
        | RequirementKind::ArtifactHash
        | RequirementKind::FsState
        | RequirementKind::GitState => VerificationMethod::AutomatedTest,
        RequirementKind::IndependentReview => VerificationMethod::IndependentReview,
        RequirementKind::SecurityCheck => VerificationMethod::SecurityCheck,
        RequirementKind::HumanApproval => VerificationMethod::HumanApproval,
    }
}

/// Opaque reference to a required human/policy gate (V5 annotation).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GateRef(String);

impl GateRef {
    /// Creates the gate reference, refusing blank values.
    pub fn new(value: impl Into<String>) -> Result<Self, PlanDomainError> {
        Ok(Self(checked_id("required_gate", value.into())?))
    }

    /// Borrows the gate reference text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One acceptance-criterion declaration on a plan node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CriterionDecl {
    /// Criterion identity.
    pub id: crate::ids::CriterionId,
    /// How this criterion is proven (always present by construction).
    pub method: VerificationMethod,
}

impl CriterionDecl {
    /// Declares a criterion with its proof method.
    #[must_use]
    pub fn new(id: crate::ids::CriterionId, method: VerificationMethod) -> Self {
        Self { id, method }
    }
}

/// Per-node risk-class transition claimed by a proposal (possibly empty).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskDelta {
    /// Node whose risk class moves.
    pub node: PlanNodeId,
    /// Class before the change.
    pub from: RiskClass,
    /// Class after the change.
    pub to: RiskClass,
}

impl RiskDelta {
    /// Returns true when the transition raises the risk class (V9 trigger).
    #[must_use]
    pub fn is_escalation(&self) -> bool {
        self.to > self.from
    }
}
