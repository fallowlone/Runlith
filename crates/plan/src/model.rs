//! Proposal-only planning seam types (ADR-0027 §§1–2, §4).
//!
//! Everything here is owned data: references, scopes, deltas, hashes.
//! No field carries a `&mut`, a storage handle, a scheduler capability, or
//! any Task type — planner output is untrusted data until the trusted
//! application service validates it (V1–V9) and persists exactly one
//! [`PlanRevision`] per accepted structural mutation.
//!
//! `PlanRevision` carries provenance, not full node blobs: structural deltas
//! live once in the validated [`PlanChangeSet`] referenced by
//! `changeset_hash` (single canonical owner per AGENT.md §20; full blobs
//! live in the plan store per ADR-0027 §4, never in the revision or event).

use std::collections::BTreeMap;

use crate::error::{checked_id, PlanDomainError};
use crate::ids::{
    CriterionId, EvidenceRef, PlanBranchId, PlanNodeId, PlannerAdapterId, PlanningCostReceiptRef,
};
use crate::resolved_evidence::ResolvedEvidence;
use crate::risk::{CriterionDecl, GateRef, RiskClass, RiskDelta};
use crate::scope::{PlanScope, ScopeBound};

// ---------------------------------------------------------------------------
// Base-graph snapshot (read-only validation input)
// ---------------------------------------------------------------------------

/// One node of the base-revision graph snapshot used for validation.
///
/// This is a detached read-only snapshot: holding one grants no authority
/// over durable Task state and offers no mutation path to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanNode {
    /// Node identity.
    pub id: PlanNodeId,
    /// Authorized scope of the node's work.
    pub scope: PlanScope,
    /// Acceptance criteria declared on the node.
    pub criteria: Vec<CriterionDecl>,
    /// Current risk class.
    pub risk_class: RiskClass,
    /// Mandatory gate when risk exceeds the branch baseline (V5).
    pub required_gate: Option<GateRef>,
}

impl PlanNode {
    /// Builds a node; duplicate criterion ids are refused.
    pub fn new(
        id: PlanNodeId,
        scope: PlanScope,
        criteria: Vec<CriterionDecl>,
        risk_class: RiskClass,
        required_gate: Option<GateRef>,
    ) -> Result<Self, PlanDomainError> {
        let mut seen = std::collections::BTreeSet::new();
        for criterion in &criteria {
            if !seen.insert(criterion.id.as_str()) {
                return Err(PlanDomainError::input(format!(
                    "node {} declares criterion {} twice",
                    id.as_str(),
                    criterion.id.as_str()
                )));
            }
        }
        Ok(Self {
            id,
            scope,
            criteria,
            risk_class,
            required_gate,
        })
    }
}

/// Detached base-revision graph snapshot: nodes plus dependency edges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanGraph {
    /// Snapshot nodes.
    pub nodes: Vec<PlanNode>,
    /// Dependency edges (`from` must precede `to`).
    pub edges: Vec<(PlanNodeId, PlanNodeId)>,
}

impl PlanGraph {
    /// Builds the snapshot; duplicate node ids are refused.
    pub fn new(
        nodes: Vec<PlanNode>,
        edges: Vec<(PlanNodeId, PlanNodeId)>,
    ) -> Result<Self, PlanDomainError> {
        let mut seen = std::collections::BTreeSet::new();
        for node in &nodes {
            if !seen.insert(node.id.as_str()) {
                return Err(PlanDomainError::input(format!(
                    "duplicate node {}",
                    node.id.as_str()
                )));
            }
        }
        Ok(Self { nodes, edges })
    }
}

// ---------------------------------------------------------------------------
// Changeset deltas
// ---------------------------------------------------------------------------

/// A new node proposed by the planner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeAddition {
    /// Fresh node id (must not collide with the base revision).
    pub id: PlanNodeId,
    /// Scope of the new node's work (must satisfy V4).
    pub scope: PlanScope,
    /// Acceptance criteria the new node proves (V3 coverage source).
    pub criteria: Vec<CriterionDecl>,
    /// Proposed risk class (V5 gate annotation above baseline).
    pub risk_class: RiskClass,
    /// Mandatory gate when risk exceeds the branch baseline.
    pub required_gate: Option<GateRef>,
}

/// Field deltas for one existing node.
///
/// Only planner-owned fields are updatable: scope, risk class, and gate
/// annotation. There is deliberately no status/lifecycle/policy field to
/// update — Task mutation is inexpressible in this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeUpdate {
    /// Base-revision node to update.
    pub target: PlanNodeId,
    /// Replacement scope (still subject to V4).
    pub scope: Option<PlanScope>,
    /// Replacement risk class (V5/V9 apply to the post-update value).
    pub risk_class: Option<RiskClass>,
    /// Gate annotation change.
    pub required_gate: Option<GateUpdate>,
}

/// Gate-annotation delta.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateUpdate {
    /// Attach (or replace) the required gate.
    Set(GateRef),
    /// Remove the required gate.
    Clear,
}

/// A proposed dependency edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeAddition {
    /// Predecessor.
    pub from: PlanNodeId,
    /// Successor.
    pub to: PlanNodeId,
}

/// A retracted dependency edge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EdgeRemoval {
    /// Predecessor.
    pub from: PlanNodeId,
    /// Successor.
    pub to: PlanNodeId,
}

/// Planning + expected execution cost movement claimed by the proposal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetDelta {
    /// Movement in planning cost (tokens/time, signed).
    pub planning_cost_movement: i64,
    /// Movement in expected execution cost (signed).
    pub expected_execution_cost_movement: i64,
}

impl BudgetDelta {
    /// Records the claimed cost movement.
    #[must_use]
    pub const fn new(planning_cost_movement: i64, expected_execution_cost_movement: i64) -> Self {
        Self {
            planning_cost_movement,
            expected_execution_cost_movement,
        }
    }
}

// ---------------------------------------------------------------------------
// Planner identity, cost receipt, changeset
// ---------------------------------------------------------------------------

/// Planner identity: provenance only, never authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannerIdentity {
    /// Adapter instance that produced the proposal.
    pub adapter_id: PlannerAdapterId,
    /// Model ref behind the adapter (informational).
    pub model_ref: String,
}

impl PlannerIdentity {
    /// Records planner provenance.
    pub fn new(adapter_id: PlannerAdapterId, model_ref: String) -> Result<Self, PlanDomainError> {
        Ok(Self {
            adapter_id,
            model_ref: checked_id("model_ref", model_ref)?,
        })
    }
}

/// Per-call cost receipt (ADR-0027 §6): tokens in/out, wall time, price
/// class. Cost is derived reporting evidence, not an enforcement limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningCostReceipt {
    /// Input tokens consumed by the planner call.
    pub input_tokens: u32,
    /// Output tokens produced by the planner call.
    pub output_tokens: u32,
    /// Wall-clock milliseconds of the planner call.
    pub wall_time_ms: u64,
    /// Provider price class (informational).
    pub price_class: String,
}

impl PlanningCostReceipt {
    /// Records one planner call's cost evidence.
    pub fn new(
        input_tokens: u32,
        output_tokens: u32,
        wall_time_ms: u64,
        price_class: String,
    ) -> Result<Self, PlanDomainError> {
        Ok(Self {
            input_tokens,
            output_tokens,
            wall_time_ms,
            price_class: checked_id("price_class", price_class)?,
        })
    }
}

/// Planner provenance attached to an accepted revision: planner identity plus
/// a reference to the planning-cost receipt recorded for the proposal call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionProposedBy {
    /// Planner identity that produced the accepted proposal.
    pub planner: PlannerIdentity,
    /// Reference to the separately recorded planner cost receipt.
    pub planning_cost_receipt_ref: PlanningCostReceiptRef,
}

impl RevisionProposedBy {
    /// Binds planner identity to the receipt reference required by ADR-0027 §4.
    #[must_use]
    pub fn new(
        planner: PlannerIdentity,
        planning_cost_receipt_ref: PlanningCostReceiptRef,
    ) -> Self {
        Self {
            planner,
            planning_cost_receipt_ref,
        }
    }
}

/// The immutable proposal object (ADR-0027 §2): every field is required; a
/// missing or blank core field is malformed and rejected before validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanChangeSet {
    /// Branch head the proposal was built against (V8 optimistic concurrency).
    pub base_plan_revision: u64,
    /// Non-empty reason why this change exists.
    pub reason: String,
    /// Typed refs: execution/attempt/test/human-decision ids.
    pub source_evidence_refs: Vec<EvidenceRef>,
    /// New nodes with scope + acceptance-criteria refs.
    pub node_additions: Vec<NodeAddition>,
    /// `(target id, field deltas)` for existing nodes.
    pub node_updates: Vec<NodeUpdate>,
    /// Target ids to remove.
    pub node_removals: Vec<PlanNodeId>,
    /// Edges to add / remove.
    pub edge_additions: Vec<EdgeAddition>,
    /// Edges to add / remove.
    pub edge_removals: Vec<EdgeRemoval>,
    /// What executable work this proposal unlocks.
    pub expected_outcome: String,
    /// Per-node risk-class transitions (possibly empty).
    pub risk_delta: Vec<RiskDelta>,
    /// Planning + expected execution cost movement.
    pub budget_delta: BudgetDelta,
    /// Must satisfy `scope_delta ⊆ scope_bound` (V4).
    pub scope_delta: PlanScope,
    /// `parent_criterion_id -> [child_criterion_ids]` (V3).
    pub coverage_map: BTreeMap<CriterionId, Vec<CriterionId>>,
    /// SHA-256 over canonical changeset bytes, computed by the proposer;
    /// any material mutation changes the hash.
    pub proposal_hash: String,
    /// Planner identity (adapter id + model ref): provenance, not authority.
    pub proposed_by: PlannerIdentity,
    /// Tokens in/out, wall time, price class (§6 envelope evidence).
    pub planning_cost_receipt: PlanningCostReceipt,
}

impl PlanChangeSet {
    /// Rejects malformed proposals before V-rule validation runs: blank
    /// reason/outcome, blank hash, or duplicate addition ids.
    pub fn check_wellformed(&self) -> Result<(), PlanDomainError> {
        if self.reason.trim().is_empty() {
            return Err(PlanDomainError::input("reason must be non-empty"));
        }
        if self.expected_outcome.trim().is_empty() {
            return Err(PlanDomainError::input("expected_outcome must be non-empty"));
        }
        if self.proposal_hash.trim().is_empty() {
            return Err(PlanDomainError::input("proposal_hash must be present"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for addition in &self.node_additions {
            if !seen.insert(addition.id.as_str()) {
                return Err(PlanDomainError::input(format!(
                    "duplicate node addition {}",
                    addition.id.as_str()
                )));
            }
            // ADR-0030 §2/§6 (TASK-0123): one node declares each criterion at
            // most once. V3 compares methods through a last-wins map, so a
            // duplicate would pass validation and then fail materialization as
            // an untyped service error — refused here as malformed input
            // instead, so it is a typed refusal with a durable L3 count.
            let mut criteria = std::collections::BTreeSet::new();
            for criterion in &addition.criteria {
                if !criteria.insert(criterion.id.as_str()) {
                    return Err(PlanDomainError::input(format!(
                        "node addition {} declares criterion {} twice",
                        addition.id.as_str(),
                        criterion.id.as_str()
                    )));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PlanRevision provenance (ADR-0027 §4)
// ---------------------------------------------------------------------------

/// Who validated an accepted revision: reconciler/service identity plus the
/// policy fingerprint the validation ran under.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedBy {
    /// Reconciler/service identity.
    pub service_id: String,
    /// Policy fingerprint the validation ran under.
    pub policy_fingerprint: String,
}

impl ValidatedBy {
    /// Records validation provenance.
    pub fn new(service_id: String, policy_fingerprint: String) -> Result<Self, PlanDomainError> {
        Ok(Self {
            service_id: checked_id("service_id", service_id)?,
            policy_fingerprint: checked_id("policy_fingerprint", policy_fingerprint)?,
        })
    }
}

/// Exactly one versioned revision per accepted structural mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanRevision {
    /// Monotonic sequence per plan branch (genesis revision is 1).
    pub revision_seq: u64,
    /// Branch this revision belongs to.
    pub plan_branch_id: PlanBranchId,
    /// Parent revision this supersedes (genesis base is 0).
    pub base_revision: u64,
    /// SHA-256 over canonical `PlanChangeSet` bytes.
    pub changeset_hash: String,
    /// Required, non-empty reason.
    pub reason: String,
    /// Typed evidence refs (execution/attempt/test/human-decision).
    pub caused_by: Vec<EvidenceRef>,
    /// Planner identity + planning-cost-receipt ref.
    pub proposed_by: RevisionProposedBy,
    /// Reconciler/service identity + policy fingerprint.
    pub validated_by: ValidatedBy,
    /// Creation timestamp supplied by the trusted caller (Unix epoch ms).
    /// The pure domain never reads the system clock itself.
    pub created_at: u64,
}

impl PlanRevision {
    /// Builds a revision, refusing empty sequence numbers, blank reasons,
    /// and blank hashes.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        revision_seq: u64,
        plan_branch_id: PlanBranchId,
        base_revision: u64,
        changeset_hash: String,
        reason: String,
        caused_by: Vec<EvidenceRef>,
        proposed_by: RevisionProposedBy,
        validated_by: ValidatedBy,
        created_at: u64,
    ) -> Result<Self, PlanDomainError> {
        if revision_seq == 0 {
            return Err(PlanDomainError::input("revision_seq must be >= 1"));
        }
        if reason.trim().is_empty() {
            return Err(PlanDomainError::input("reason must be non-empty"));
        }
        if changeset_hash.trim().is_empty() {
            return Err(PlanDomainError::input("changeset_hash must be present"));
        }
        Ok(Self {
            revision_seq,
            plan_branch_id,
            base_revision,
            changeset_hash,
            reason,
            caused_by,
            proposed_by,
            validated_by,
            created_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Atomicity seam types (ADR-0027 §1)
// ---------------------------------------------------------------------------

/// Pure assessment verdict. `Blocked` carries a typed blocker; assessment
/// performs zero durable writes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomicityVerdict {
    /// Execute directly; no decomposition needed.
    Atomic,
    /// Worth decomposing through `propose_decomposition`.
    Decomposable,
    /// Cannot proceed; typed reason for the orchestrator to escalate.
    Blocked {
        /// Machine-readable blocker for escalation routing.
        blocker: AtomicityBlocker,
    },
}

/// Typed blocker carried by a `Blocked` assessment or an `Unplannable` fault.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomicityBlocker {
    /// Stable blocker code.
    pub code: String,
    /// Human-readable detail.
    pub detail: String,
}

impl AtomicityBlocker {
    /// Creates a typed blocker.
    pub fn new(
        code: impl Into<String>,
        detail: impl Into<String>,
    ) -> Result<Self, PlanDomainError> {
        Ok(Self {
            code: checked_id("blocker_code", code.into())?,
            detail: checked_id("blocker_detail", detail.into())?,
        })
    }
}

/// Pure atomicity assessment: verdict + rationale + evidence refs, plus the
/// per-call cost receipt ADR-0027 §6 requires for every `PlannerPort` call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomicityAssessment {
    /// Assessment verdict.
    pub verdict: AtomicityVerdict,
    /// Non-empty rationale.
    pub rationale: String,
    /// Evidence refs behind the assessment.
    pub evidence_refs: Vec<EvidenceRef>,
    /// Tokens in/out, wall time, price class of the assessment call.
    pub planning_cost_receipt: PlanningCostReceipt,
}

impl AtomicityAssessment {
    /// Builds an assessment; blank rationales are refused.
    pub fn new(
        verdict: AtomicityVerdict,
        rationale: String,
        evidence_refs: Vec<EvidenceRef>,
        planning_cost_receipt: PlanningCostReceipt,
    ) -> Result<Self, PlanDomainError> {
        if rationale.trim().is_empty() {
            return Err(PlanDomainError::input("rationale must be non-empty"));
        }
        Ok(Self {
            verdict,
            rationale,
            evidence_refs,
            planning_cost_receipt,
        })
    }
}

/// Everything `assess_atomicity` may read: refs and bounds only — no graph
/// handles, no policy handles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomicityRequest {
    /// Task under assessment (opaque ref).
    pub task_ref: String,
    /// Acceptance criteria under assessment.
    pub acceptance_criteria_refs: Vec<CriterionId>,
    /// Authorized scope bound.
    pub scope_bound: ScopeBound,
    /// Evidence refs the planner may read.
    pub evidence_refs: Vec<EvidenceRef>,
    /// ADR-0029 option B: content the trusted service resolved for a
    /// subset of `evidence_refs`. Refs without an item stay identity-only.
    pub resolved_evidence: Vec<ResolvedEvidence>,
    /// Execution-policy ref (opaque; policy content never enters the port).
    pub execution_policy_ref: String,
}

/// Input to `propose_decomposition`: the planner receives its remaining
/// depth/budget headroom so it can refuse before a wasted call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecompositionRequest {
    /// Parent task to decompose (opaque ref).
    pub parent_task_ref: String,
    /// Branch head the proposal must build against.
    pub base_plan_revision: u64,
    /// Parent acceptance criteria the decomposition must preserve (V3).
    pub acceptance_criteria_refs: Vec<CriterionId>,
    /// Authorized scope bound (V4).
    pub scope_bound: ScopeBound,
    /// Evidence refs the planner may read.
    pub evidence_refs: Vec<EvidenceRef>,
    /// ADR-0029 option B: content the trusted service resolved for a
    /// subset of `evidence_refs`. Refs without an item stay identity-only.
    pub resolved_evidence: Vec<ResolvedEvidence>,
    /// Remaining decomposition depth headroom.
    pub depth_remaining: u32,
    /// Baseline risk the proposal is judged against (V5). A node above this
    /// baseline must carry a `required_gate`, so the planner needs the
    /// threshold for the same reason it needs depth and budget headroom:
    /// otherwise it proposes into a certain rejection.
    pub baseline_risk: RiskClass,
    /// Remaining planner-call budget headroom.
    pub budget_remaining: PlanningBudget,
}

/// Input to `propose_replan`: one affected branch only — regenerating the
/// whole project plan is forbidden input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplanRequest {
    /// Affected branch root (opaque ref).
    pub branch_root_ref: String,
    /// Branch head the proposal must build against.
    pub base_plan_revision: u64,
    /// Triggering evidence refs.
    pub triggering_evidence_refs: Vec<EvidenceRef>,
    /// ADR-0029 option B: content the trusted service resolved for a
    /// subset of `triggering_evidence_refs`. Refs without an item stay
    /// identity-only.
    pub resolved_evidence: Vec<ResolvedEvidence>,
    /// Authorized scope bound (V4).
    pub scope_bound: ScopeBound,
    /// Remaining decomposition depth headroom.
    pub depth_remaining: u32,
    /// Baseline risk the proposal is judged against (V5), as in
    /// [`DecompositionRequest::baseline_risk`].
    pub baseline_risk: RiskClass,
    /// Remaining planner-call budget headroom.
    pub budget_remaining: PlanningBudget,
}

/// Remaining planner-call budget headroom (ADR-0027 §6 envelope).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanningBudget {
    /// Planner calls still allowed for this evidence event + branch.
    pub calls_remaining: u32,
    /// Cumulative input tokens still allowed.
    pub input_tokens_remaining: u32,
}

/// Provider spend evidence attached to a planner fault (ADR-0027 §6: no
/// silent spend). It is authored by adapter control flow, never parsed from
/// model output, and lets the trusted service charge every provider-started
/// call even when no proposal/assessment can be returned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlannerCallSpend {
    /// Refused before any provider call started: zero spend.
    NotStarted,
    /// A provider call started and reported exact usage.
    Measured(PlanningCostReceipt),
    /// A provider call may have started, but exact usage is unrecoverable.
    /// The trusted service charges conservatively; no receipt is fabricated.
    Unmeasured,
}

/// Planner failure taxonomy (ADR-0027 §1, exact): a planner that cannot
/// produce a valid smaller decomposition reports a fault; the orchestrator
/// then BLOCKs or escalates. Never retried silently. Variants that can occur
/// after a provider call started carry [`PlannerCallSpend`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlannerFault {
    /// No valid decomposition exists; carries the typed blocker.
    Unplannable {
        /// Why planning cannot proceed.
        blocker: AtomicityBlocker,
        /// Provider spend incurred before the planner concluded.
        spend: PlannerCallSpend,
    },
    /// Per-call or per-evidence-event call/token envelope exhausted.
    BudgetExhausted {
        /// `NotStarted` for pre-call refusal; measured spend when the
        /// provider-reported usage itself breached the envelope.
        spend: PlannerCallSpend,
    },
    /// No depth headroom remains (always refused before any provider call).
    DepthExhausted,
    /// Provider-layer failure with a typed cause.
    ProviderFailure {
        /// Typed provider failure cause.
        typed_cause: String,
        /// Provider spend incurred before the failure was observed.
        spend: PlannerCallSpend,
    },
}

impl PlannerFault {
    /// Provider spend evidence carried by this fault.
    #[must_use]
    pub fn spend(&self) -> PlannerCallSpend {
        match self {
            Self::Unplannable { spend, .. }
            | Self::BudgetExhausted { spend }
            | Self::ProviderFailure { spend, .. } => spend.clone(),
            Self::DepthExhausted => PlannerCallSpend::NotStarted,
        }
    }
}

/// The proposal-only planning seam (ADR-0027 §1).
///
/// Implementations propose only; validated application services mutate the
/// graph. All inputs/outputs are owned data, so an implementation cannot
/// smuggle mutation authority through this trait.
pub trait PlannerPort {
    /// Pure atomicity assessment: zero durable writes.
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault>;

    /// Proposes a decomposition; the returned changeset carries no authority.
    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault>;

    /// Proposes a branch-scoped replan; whole-plan regeneration is
    /// forbidden input.
    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault>;
}
