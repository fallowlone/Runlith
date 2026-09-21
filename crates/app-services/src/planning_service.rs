//! Trusted G7 planning application service (TASK-0078).
//!
//! `PlannerPort` output is proposal data only. This service owns the accepted
//! control flow of every planner call:
//!
//! ```text
//! durable guardrails (depth, L3, revisions per evidence cycle, calls, tokens)
//! → durable call reservation → PlannerPort call
//! → durable settlement (measured receipt | conservative charge)
//! → per-call / cumulative envelope
//! → V1-V9 against the durable head and durable V7 lineage
//! → detached candidate graph → canonical Task applicability + L4
//! → atomic revision + lineage + journal commit → AcceptedPlanRevision
//! ```
//!
//! Guardrail state is keyed by (plan branch, trusted triggering evidence) in
//! SQLite. Restarts and new service handles continue the same budget, L3
//! history and lineage; there is no caller-owned reset. Only an
//! [`AcceptedPlanRevision`] read from committed durable state may gate
//! execution through the existing scheduler path.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use plan_domain::{
    call_within_envelope, l3_no_progress_tripped, planning_evidence_key,
    requirement_kind_for_method, validate_proposal, AffectedParentCriterion, AtomicityAssessment,
    AtomicityBlocker, AtomicityRequest, DecompositionRequest, EvidenceRef, GuardrailTrip,
    HumanContinuation, HumanDecisionReason, HumanDecisionRequest, HumanDecisionRequestId,
    HumanDecisionStatus, PlanBranchId, PlanChangeSet, PlanDomainError, PlanGraph, PlanRevision,
    PlannerCallSpend, PlannerFault, PlannerPort, PlanningBudget, PlanningCostReceipt,
    PlanningCostReceiptRef, PlanningInputSnapshot, PlanningInputSnapshotId, ReplanRequest,
    RevisionProposedBy, RiskClass, RiskEscalationGrant, ScopeBound, TaskPlanningAuthority,
    ValidatedBy, ValidationContext, ValidationOutcome, ValidationRefusal, MAX_DECOMPOSITION_DEPTH,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
    MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_OUTPUT_TOKENS, MAX_PLANNER_WALL_CLOCK_SECS,
    MAX_REVISIONS_PER_CYCLE_PER_BRANCH,
};
use storage_sqlite::{
    ApprovedPlanProposal, ChildTaskMaterialization, EventType, NewHumanDecisionRequest,
    NewJournalEvent, PlanCommitJournalEvidence, PlanRevisionLineage, PlanRevisionLineageRecord,
    PlannerCallSettlement, PlanningRunRecord, PlanningRunState, SqliteStore, StorageError,
    CHILD_TASK_MATERIALIZATION_PROVENANCE,
};
use task_domain::{EvidenceRequirement, Task, TaskId, TaskState};

use crate::clock::{format_rfc3339, Clock, SystemClock};
use crate::plan_graph::{
    apply_validated_changeset, has_runnable_node, merged_depth, plan_predecessors, remaining_depth,
    target_applicability,
};

const VALIDATOR_SERVICE_ID: &str = "plan-application-service";
const PLAN_PROVENANCE: &str = "app-services/planning-service";
/// Input tokens reserved durably before every planner call and charged when a
/// started call reports no usage: one full ADR-0027 §6 per-call allowance.
const CONSERVATIVE_CALL_CHARGE: u32 = MAX_PLANNER_INPUT_TOKENS;
const HUMAN_DECISION_TTL_MS: u64 = 30 * 60 * 1000;

/// Trusted non-planner inputs used by canonical V1-V9 validation.
///
/// These values come from product policy/reconciliation state. They are never
/// accepted from provider output. `lineage_fingerprints` may add fingerprints
/// from outside this branch (for example an inherited parent lineage); the
/// branch's own lineage is always read from durable state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningPolicyContext {
    pub scope_bound: ScopeBound,
    pub baseline_risk: RiskClass,
    pub affected_parent_criteria: Vec<AffectedParentCriterion>,
    pub parent_task_ref: String,
    /// True only for contexts produced by
    /// [`PlanningService::canonical_policy_context`], which every production
    /// planning path uses. Plan publication then re-derives the Task-owned
    /// scope/acceptance/risk/candidate authority inside the publishing
    /// transaction and refuses stale snapshots. Hand-built trusted fixture
    /// contexts are not Task-derived and leave this false.
    ///
    /// Scope of the flag since TASK-0091: it governs the **staleness** fence
    /// only. Parent admission — the parent Task exists, its state authorizes
    /// decomposition, and every child carries the parent's project — is an
    /// invariant of publication enforced unconditionally, so clearing this
    /// cannot reach it.
    pub canonical_task_authority: bool,
    pub lineage_fingerprints: Vec<String>,
    pub risk_grant: Option<RiskEscalationGrant>,
    /// Durable HumanDecisionRequest that produced `risk_grant`. V9 plan
    /// publication consumes it atomically; a raw grant without this binding
    /// can validate pure domain rules but cannot publish production state.
    pub risk_decision_id: Option<HumanDecisionRequestId>,
    pub policy_fingerprint: String,
}

/// Which planning operation is claiming a generation.
///
/// Every mode advances the run generation, so every planner call is fenced and
/// its spend is durably reserved (ADR-0028 §3, §4). They differ only in whether
/// the claim republishes the branch's current planning input.
#[derive(Clone, Copy, Debug)]
enum ControlRunMode {
    /// Intends to publish a revision, so its snapshot becomes current input.
    Proposal,
    /// V9 approved replay, fenced against its `HumanDecision` plan head.
    ApprovedReplay { approved_plan_head: u64 },
    /// Atomicity assessment; proposes nothing and leaves current input alone.
    Assessment,
    /// A human gate. It leaves current input alone at claim time and publishes
    /// its snapshot in the same transaction that records the gate's
    /// `HumanDecision` (TASK-0119).
    HumanGate,
}

/// Durable guardrail view for one plan branch + triggering-evidence event.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanningGuardrailState {
    /// Planner calls made for this event (including refused-before-spawn).
    pub planner_calls: u32,
    /// Input tokens charged: measured usage or conservative charges.
    pub charged_input_tokens: u64,
    /// Consecutive refused proposals since the last accepted revision (L3).
    pub consecutive_rejections: u32,
    /// Revisions accepted on the branch from this evidence event (V6).
    pub revisions_accepted: u32,
}

/// An accepted revision capability, read from or minted by TASK-0077's atomic
/// revision+graph+journal commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedPlanRevision {
    revision: PlanRevision,
    graph: PlanGraph,
    planning_cost_receipt: PlanningCostReceipt,
    planning_input_snapshot_id: PlanningInputSnapshotId,
    planning_input_fingerprint: String,
}

impl AcceptedPlanRevision {
    #[must_use]
    pub fn revision(&self) -> &PlanRevision {
        &self.revision
    }

    #[must_use]
    pub fn graph(&self) -> &PlanGraph {
        &self.graph
    }

    #[must_use]
    pub fn planning_cost_receipt(&self) -> &PlanningCostReceipt {
        &self.planning_cost_receipt
    }

    #[must_use]
    pub fn planning_input_snapshot_id(&self) -> &PlanningInputSnapshotId {
        &self.planning_input_snapshot_id
    }

    #[must_use]
    pub fn planning_input_fingerprint(&self) -> &str {
        &self.planning_input_fingerprint
    }

    /// Returns the Plan predecessors that must complete before `task_id` can
    /// become RUNNABLE. Plan edges are ordered `from -> to`, where `from`
    /// must precede `to`; canonical Task readiness/policy still run after
    /// this derived Plan gate.
    pub fn predecessor_task_ids(
        &self,
        task_id: &TaskId,
    ) -> Result<Option<Vec<TaskId>>, PlanningServiceError> {
        let id = task_id.as_str();
        if !self.graph.nodes.iter().any(|node| node.id.as_str() == id) {
            return Ok(None);
        }

        let mut predecessors = BTreeSet::new();
        for from in plan_predecessors(&self.graph, id) {
            let predecessor = TaskId::new(from.as_str().to_owned()).map_err(|error| {
                PlanningServiceError::InvalidContext(format!(
                    "plan predecessor {} is not a valid Task id: {error}",
                    from.as_str()
                ))
            })?;
            predecessors.insert(predecessor);
        }
        Ok(Some(predecessors.into_iter().collect()))
    }
}

/// A structurally valid candidate plan that cannot be applied to canonical
/// Task state. Detected before durable acceptance; nothing is committed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanApplicabilityRefusal {
    /// A planner referenced a canonical Task outside the identity set that
    /// Runlith bound to this planning snapshot.
    UntrustedNodeIdentity { node: String },
    /// The selected Task is not represented by the candidate graph.
    TaskNotInPlan { task_id: TaskId },
    /// The selected Task is represented but has no canonical Task.
    TaskMissing { task_id: TaskId },
    /// A Plan predecessor of the selected Task has no canonical Task.
    PredecessorWithoutTask { node: String },
}

/// One bounded proposal application result. Refusal and guardrail exhaustion
/// are data, not half-committed failures. Only `Accepted` implies a Plan
/// mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanningProposalOutcome {
    Accepted(Box<AcceptedPlanRevision>),
    WaitingForHuman(Box<HumanDecisionRequest>),
    Refused(ValidationRefusal),
    NotApplicable(PlanApplicabilityRefusal),
    GuardrailTripped(GuardrailTrip),
    Blocked(AtomicityBlocker),
}

/// One bounded atomicity assessment result. Assessment never mutates Plan
/// state; its planner call is budgeted and recorded like every other call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanningAssessmentOutcome {
    Assessed(AtomicityAssessment),
    WaitingForHuman(Box<HumanDecisionRequest>),
    GuardrailTripped(GuardrailTrip),
    Blocked(AtomicityBlocker),
}

/// Resolution of a Task against the current accepted Plan heads. Production
/// execution requires exactly one accepted Plan; callers never select the
/// authoritative branch themselves (ADR-0028 §6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptedTaskPlanResolution {
    Missing,
    Ambiguous {
        branches: Vec<PlanBranchId>,
    },
    Accepted {
        branch: PlanBranchId,
        plan: Box<AcceptedPlanRevision>,
    },
}

/// Planning application-service failure. No variant implies a Plan mutation:
/// the branch head never advances on `Err`. Planner calls that were made
/// still leave their durable accounting evidence (guardrail counters and,
/// when usage was measured, a cost receipt).
#[derive(Debug)]
pub enum PlanningServiceError {
    Planner(PlannerFault),
    Storage(StorageError),
    Domain(PlanDomainError),
    InvalidContext(String),
}

impl fmt::Display for PlanningServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Planner(error) => write!(f, "planner failed: {error:?}"),
            Self::Storage(error) => write!(f, "plan persistence failed: {error}"),
            Self::Domain(error) => write!(f, "plan domain rejected trusted construction: {error}"),
            Self::InvalidContext(detail) => write!(f, "invalid planning context: {detail}"),
        }
    }
}

impl std::error::Error for PlanningServiceError {}

impl From<StorageError> for PlanningServiceError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<PlanDomainError> for PlanningServiceError {
    fn from(value: PlanDomainError) -> Self {
        Self::Domain(value)
    }
}

/// Durable proposal preconditions read before a planner call.
struct ProposalBase {
    head: u64,
    graph: PlanGraph,
    lineage_fingerprints: Vec<String>,
    state: PlanningGuardrailState,
    trigger_key: String,
    remaining_depth: u32,
    snapshot: PlanningInputSnapshot,
    run: PlanningRunRecord,
}

/// Outcome of one budgeted, durably accounted planner call.
enum Metered<T> {
    Returned {
        value: T,
        receipt_ref: PlanningCostReceiptRef,
    },
    Faulted(PlannerFault),
    Tripped(GuardrailTrip),
}

/// Trusted owner of Plan proposal validation, guardrails and persistence.
pub struct PlanningService<C: Clock = SystemClock> {
    store: SqliteStore,
    clock: C,
}

impl<C: Clock> PlanningService<C> {
    /// Re-derives the trusted planning authority for `task_id` from canonical
    /// product state. IPC/provider input is deliberately absent from this
    /// API: Task owns scope/acceptance/risk while Policy owns the fingerprint.
    pub fn canonical_policy_context(
        &self,
        task_id: &TaskId,
        policy_fingerprint: impl Into<String>,
    ) -> Result<PlanningPolicyContext, PlanningServiceError> {
        let task = self.store.get_task(task_id)?.ok_or_else(|| {
            PlanningServiceError::InvalidContext(format!(
                "canonical planning Task {} does not exist",
                task_id.as_str()
            ))
        })?;
        let facts = task.snapshot();
        let authority = TaskPlanningAuthority::derive(
            &facts.allowed_paths,
            &facts.forbidden_paths,
            &facts.acceptance,
            &facts.evidence_requirements,
            facts.risk.as_deref(),
        )
        .map_err(|PlanDomainError::InvalidInput(detail)| {
            PlanningServiceError::InvalidContext(format!(
                "canonical planning Task {} {detail}",
                task_id.as_str()
            ))
        })?;
        let policy = PlanningPolicyContext {
            scope_bound: authority.scope_bound,
            baseline_risk: authority.baseline_risk,
            affected_parent_criteria: authority.affected_parent_criteria,
            parent_task_ref: task_id.as_str().to_owned(),
            canonical_task_authority: true,
            lineage_fingerprints: Vec::new(),
            risk_grant: None,
            risk_decision_id: None,
            policy_fingerprint: policy_fingerprint.into(),
        };
        validate_policy_context(&policy)?;
        Ok(policy)
    }

    pub fn open(path: impl AsRef<Path>, clock: C) -> Result<Self, PlanningServiceError> {
        Ok(Self {
            store: SqliteStore::open(path)?,
            clock,
        })
    }

    #[must_use]
    pub fn attach(store: SqliteStore, clock: C) -> Self {
        Self { store, clock }
    }

    #[must_use]
    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    pub(crate) fn now_millis(&self) -> u64 {
        self.clock.now_millis()
    }

    /// Durable guardrail state for `branch` and the triggering evidence set.
    /// Evidence order and repetition do not change the event identity.
    pub fn guardrail_state(
        &self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
    ) -> Result<PlanningGuardrailState, PlanningServiceError> {
        let key = trigger_key(triggering_evidence)?;
        let lineage = self.store.get_plan_lineage(branch)?;
        self.guardrail_snapshot(branch, &key, &lineage)
    }

    /// Measured receipt recorded for the `call_seq`-th (1-based) planner call
    /// of one branch + evidence event, if that call reported usage.
    pub fn planner_call_receipt(
        &self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        call_seq: u32,
    ) -> Result<Option<PlanningCostReceipt>, PlanningServiceError> {
        let key = trigger_key(triggering_evidence)?;
        let receipt_ref = call_receipt_ref(branch, &key, call_seq)?;
        Ok(self.store.get_plan_cost_receipt(&receipt_ref)?)
    }

    /// The canonical accepted head of `branch`, reconstructed from committed
    /// durable state. `None` when the branch has no accepted revision.
    pub fn current_accepted_plan(
        &self,
        branch: &PlanBranchId,
    ) -> Result<Option<AcceptedPlanRevision>, PlanningServiceError> {
        let Some(head) = self.store.get_plan_head(branch)?.filter(|head| *head > 0) else {
            return Ok(None);
        };
        let corrupt = |what: &str| {
            PlanningServiceError::Storage(StorageError::Corrupt(format!(
                "plan branch {} head {head} has no {what}",
                branch.as_str()
            )))
        };
        let revision = self
            .store
            .get_plan_revision(branch, head)?
            .ok_or_else(|| corrupt("revision"))?;
        let graph = self
            .store
            .get_plan_graph(branch, head)?
            .ok_or_else(|| corrupt("graph snapshot"))?;
        let planning_cost_receipt = self
            .store
            .get_plan_cost_receipt(&revision.proposed_by.planning_cost_receipt_ref)?
            .ok_or_else(|| corrupt("resolvable planning cost receipt"))?;
        let lineage = self.store.get_plan_lineage(branch)?;
        let binding = lineage
            .last()
            .ok_or_else(|| corrupt("planning input/run lineage"))?;
        if binding.revision_seq != head {
            return Err(corrupt("head planning input/run lineage"));
        }
        if !self
            .store
            .revision_has_current_planning_input(branch, head)?
        {
            return Err(PlanningServiceError::Storage(
                StorageError::PlanningInputStale {
                    branch: branch.as_str().to_owned(),
                    expected_fingerprint: binding.planning_input_fingerprint.clone(),
                },
            ));
        }
        Ok(Some(AcceptedPlanRevision {
            revision,
            graph,
            planning_cost_receipt,
            planning_input_snapshot_id: binding.planning_input_snapshot_id.clone(),
            planning_input_fingerprint: binding.planning_input_fingerprint.clone(),
        }))
    }

    /// Resolves a Task to product-owned current Plan state. Zero or multiple
    /// current heads are explicit outcomes; the caller never supplies a
    /// branch name as execution authority.
    pub fn current_accepted_plan_for_task(
        &self,
        task_id: &TaskId,
    ) -> Result<AcceptedTaskPlanResolution, PlanningServiceError> {
        let matches = self.store.find_current_plan_heads_for_task(task_id)?;
        match matches.as_slice() {
            [] => Ok(AcceptedTaskPlanResolution::Missing),
            [(branch, _)] => {
                let plan = self.current_accepted_plan(branch)?.ok_or_else(|| {
                    PlanningServiceError::Storage(StorageError::Corrupt(format!(
                        "task {} resolved to plan branch {} without an accepted head",
                        task_id.as_str(),
                        branch.as_str()
                    )))
                })?;
                Ok(AcceptedTaskPlanResolution::Accepted {
                    branch: branch.clone(),
                    plan: Box::new(plan),
                })
            }
            many => Ok(AcceptedTaskPlanResolution::Ambiguous {
                branches: many.iter().map(|(branch, _)| branch.clone()).collect(),
            }),
        }
    }

    /// Budgeted, recorded atomicity assessment. Trusted policy values replace
    /// caller-supplied task/scope/criteria fields.
    pub fn assess_atomicity(
        &mut self,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        mut request: AtomicityRequest,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningAssessmentOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        if let Some(decision) = self.planning_human_gate(branch, policy, None)? {
            return Ok(PlanningAssessmentOutcome::WaitingForHuman(Box::new(
                decision,
            )));
        }
        let key = trigger_key(&request.evidence_refs)?;
        let lineage = self.store.get_plan_lineage(branch)?;
        let state = self.guardrail_snapshot(branch, &key, &lineage)?;
        if let Some(trip) = pre_call_guardrail(&state, false) {
            if let Some(decision) = self.pre_call_human_intervention(
                branch,
                &request.evidence_refs,
                policy,
                &lineage,
                trip.clone(),
            )? {
                return Ok(PlanningAssessmentOutcome::WaitingForHuman(Box::new(
                    decision,
                )));
            }
            return Ok(PlanningAssessmentOutcome::GuardrailTripped(trip));
        }
        request.task_ref = policy.parent_task_ref.clone();
        request.scope_bound = policy.scope_bound.clone();
        request.acceptance_criteria_refs = policy
            .affected_parent_criteria
            .iter()
            .map(|criterion| criterion.id.clone())
            .collect();

        let (head, _) = self.load_base(branch)?;
        // Assessment proposes nothing, so it reserves a run for its planner
        // spend without republishing current input (ADR-0027, TASK-0116).
        let (snapshot, run) = self.start_control_run_with_mode(
            branch,
            &request.evidence_refs,
            policy,
            head,
            &lineage,
            &key,
            ControlRunMode::Assessment,
        )?;

        let metered = self.metered_call(
            branch,
            &key,
            &run,
            || planner.assess_atomicity(request),
            assessment_receipt,
        )?;
        match metered {
            Metered::Returned { value, .. } => {
                self.finish_run(&run, PlanningRunState::Succeeded)?;
                debug_assert_eq!(snapshot.id, run.snapshot_id);
                Ok(PlanningAssessmentOutcome::Assessed(value))
            }
            Metered::Tripped(trip) => {
                if let Some(reason) = human_reason_for_guardrail(&trip) {
                    let opened =
                        self.open_assessment_human_gate(branch, policy, &run, &snapshot, reason);
                    let finished = self.finish_run(&run, PlanningRunState::Refused);
                    let decision = match (opened, finished) {
                        (Ok(decision), Ok(())) => decision,
                        (Err(error), _) | (Ok(_), Err(error)) => return Err(error),
                    };
                    return Ok(PlanningAssessmentOutcome::WaitingForHuman(Box::new(
                        decision,
                    )));
                }
                self.finish_run(&run, PlanningRunState::Refused)?;
                Ok(PlanningAssessmentOutcome::GuardrailTripped(trip))
            }
            Metered::Faulted(fault) => {
                let human_reason = match &fault {
                    PlannerFault::DepthExhausted | PlannerFault::Unplannable { .. } => {
                        Some(HumanDecisionReason::PlannerBlocked)
                    }
                    PlannerFault::BudgetExhausted { .. } => {
                        Some(HumanDecisionReason::PlannerBudget)
                    }
                    PlannerFault::ProviderFailure { .. } => None,
                };
                if let Some(reason) = human_reason {
                    let opened =
                        self.open_assessment_human_gate(branch, policy, &run, &snapshot, reason);
                    let finished = self.finish_run(&run, PlanningRunState::Failed);
                    let decision = match (opened, finished) {
                        (Ok(decision), Ok(())) => decision,
                        (Err(error), _) | (Ok(_), Err(error)) => return Err(error),
                    };
                    return Ok(PlanningAssessmentOutcome::WaitingForHuman(Box::new(
                        decision,
                    )));
                }
                match planner_fault_outcome(fault)? {
                    PlanningProposalOutcome::Blocked(blocker) => {
                        self.finish_run(&run, PlanningRunState::Failed)?;
                        Ok(PlanningAssessmentOutcome::Blocked(blocker))
                    }
                    PlanningProposalOutcome::GuardrailTripped(trip) => {
                        self.finish_run(&run, PlanningRunState::Refused)?;
                        Ok(PlanningAssessmentOutcome::GuardrailTripped(trip))
                    }
                    other => Err(PlanningServiceError::InvalidContext(format!(
                        "unexpected assessment fault outcome {other:?}"
                    ))),
                }
            }
        }
    }

    pub fn propose_decomposition(
        &mut self,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        request: DecompositionRequest,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        self.decompose(planner, branch, request, policy, None)
    }

    /// Decomposition whose acceptance additionally requires the candidate
    /// plan to be applicable to `task_id` (plan-gated scheduling).
    pub(crate) fn decompose_for_task(
        &mut self,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        request: DecompositionRequest,
        policy: &PlanningPolicyContext,
        task_id: &TaskId,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        self.decompose(planner, branch, request, policy, Some(task_id))
    }

    fn decompose(
        &mut self,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        mut request: DecompositionRequest,
        policy: &PlanningPolicyContext,
        target: Option<&TaskId>,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        if let Some(decision) = self.planning_human_gate(branch, policy, None)? {
            return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
        }
        let base = match self.prepare_proposal(
            branch,
            &request.evidence_refs,
            request.depth_remaining,
            policy,
        )? {
            Ok(base) => base,
            Err(trip) => {
                let lineage = self.store.get_plan_lineage(branch)?;
                if let Some(decision) = self.pre_call_human_intervention(
                    branch,
                    &request.evidence_refs,
                    policy,
                    &lineage,
                    trip.clone(),
                )? {
                    return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
                }
                return Ok(PlanningProposalOutcome::GuardrailTripped(trip));
            }
        };

        request.parent_task_ref = policy.parent_task_ref.clone();
        request.base_plan_revision = base.head;
        request.acceptance_criteria_refs = policy
            .affected_parent_criteria
            .iter()
            .map(|criterion| criterion.id.clone())
            .collect();
        request.scope_bound = policy.scope_bound.clone();
        request.baseline_risk = policy.baseline_risk;
        request.depth_remaining = request.depth_remaining.min(base.remaining_depth);
        request.budget_remaining = remaining_budget(&base.state);

        let metered = self.metered_call(
            branch,
            &base.trigger_key,
            &base.run,
            || planner.propose_decomposition(request),
            proposal_receipt,
        )?;
        self.conclude_proposal(branch, base, metered, policy, target)
    }

    pub fn propose_replan(
        &mut self,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        request: ReplanRequest,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        self.propose_replan_with_human_decision(planner, branch, request, policy, None)
    }

    pub(crate) fn propose_replan_with_human_decision(
        &mut self,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        mut request: ReplanRequest,
        policy: &PlanningPolicyContext,
        human_decision_id: Option<&HumanDecisionRequestId>,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        if let Some(decision) = self.planning_human_gate(branch, policy, human_decision_id)? {
            return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
        }
        let base = match self.prepare_proposal(
            branch,
            &request.triggering_evidence_refs,
            request.depth_remaining,
            policy,
        )? {
            Ok(base) => base,
            Err(trip) => {
                let lineage = self.store.get_plan_lineage(branch)?;
                if let Some(decision) = self.pre_call_human_intervention(
                    branch,
                    &request.triggering_evidence_refs,
                    policy,
                    &lineage,
                    trip.clone(),
                )? {
                    return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
                }
                return Ok(PlanningProposalOutcome::GuardrailTripped(trip));
            }
        };

        request.branch_root_ref = policy.parent_task_ref.clone();
        request.base_plan_revision = base.head;
        request.scope_bound = policy.scope_bound.clone();
        request.baseline_risk = policy.baseline_risk;
        request.depth_remaining = request.depth_remaining.min(base.remaining_depth);
        request.budget_remaining = remaining_budget(&base.state);

        let metered = self.metered_call(
            branch,
            &base.trigger_key,
            &base.run,
            || planner.propose_replan(request),
            proposal_receipt,
        )?;
        self.conclude_proposal(branch, base, metered, policy, None)
    }

    /// Publishes the exact immutable proposal previously bound to a resolved
    /// V9 RESUME decision. No PlannerPort is accepted by this API, so RESUME
    /// cannot accidentally replay a provider call or depend on incidental
    /// measured receipt metadata being reproduced.
    pub fn resume_approved_risk_proposal(
        &mut self,
        id: &HumanDecisionRequestId,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        let record = match self
            .store
            .validate_human_decision_current(id, self.clock.now_millis())
        {
            Ok(record) => record,
            Err(StorageError::HumanDecisionStale { .. }) => {
                self.store.supersede_unusable_human_decision(id)?;
                return Err(PlanningServiceError::Storage(
                    StorageError::HumanDecisionStale {
                        request_id: id.as_str().to_owned(),
                        reason: "V9 RESUME decision is stale".to_owned(),
                    },
                ));
            }
            Err(error) => return Err(error.into()),
        };
        if record.reason != HumanDecisionReason::RiskEscalation
            || record.status != HumanDecisionStatus::Resolved
            || record.selected != Some(HumanContinuation::Resume)
        {
            return Err(PlanningServiceError::Storage(
                StorageError::HumanDecisionState {
                    request_id: id.as_str().to_owned(),
                    state: record.status.as_str().to_owned(),
                },
            ));
        }
        if policy.parent_task_ref != record.task_id.as_str()
            || policy.risk_grant.is_some()
            || policy.risk_decision_id.is_some()
            || !self.store.planning_input_policy_matches(
                &record.precondition.planning_input_snapshot_id,
                &policy.parent_task_ref,
                &policy.scope_bound,
                policy.baseline_risk,
                &policy.affected_parent_criteria,
                &policy.lineage_fingerprints,
                None,
                &policy.policy_fingerprint,
            )?
        {
            self.store.supersede_unusable_human_decision(id)?;
            return Err(PlanningServiceError::Storage(
                StorageError::HumanDecisionStale {
                    request_id: id.as_str().to_owned(),
                    reason: "current planning policy no longer matches approved proposal"
                        .to_owned(),
                },
            ));
        }
        let approved = self
            .store
            .get_approved_human_decision_proposal(id)?
            .ok_or_else(|| {
                PlanningServiceError::Storage(StorageError::Corrupt(format!(
                    "risk decision {} has no durable approved proposal",
                    id.as_str()
                )))
            })?;
        if record.proposal_hash.as_deref() != Some(approved.changeset.proposal_hash.as_str()) {
            return Err(PlanningServiceError::Storage(StorageError::Corrupt(
                format!(
                    "risk decision {} approved proposal binding changed",
                    id.as_str()
                ),
            )));
        }
        let expires_at_ms = record.expires_at_ms.ok_or_else(|| {
            PlanningServiceError::Storage(StorageError::Corrupt(format!(
                "risk decision {} has no expiry",
                id.as_str()
            )))
        })?;
        let mut resumed_policy = policy.clone();
        resumed_policy.risk_grant = Some(RiskEscalationGrant {
            proposal_hash: approved.changeset.proposal_hash.clone(),
            expires_at_ms,
            consumed: false,
        });
        resumed_policy.risk_decision_id = Some(id.clone());

        let base = match self.prepare_approved_proposal(
            &record.plan_branch_id,
            &approved.changeset.source_evidence_refs,
            &resumed_policy,
            record.precondition.plan_head,
        ) {
            Ok(base) => base,
            Err(PlanningServiceError::Storage(StorageError::PlanStale { .. })) => {
                return match self.store.supersede_unusable_human_decision(id) {
                    Ok(_) => Err(PlanningServiceError::Storage(
                        StorageError::HumanDecisionStale {
                            request_id: id.as_str().to_owned(),
                            reason: "V9 RESUME decision is stale".to_owned(),
                        },
                    )),
                    Err(StorageError::HumanDecisionState { request_id, state }) => {
                        Err(PlanningServiceError::Storage(
                            StorageError::HumanDecisionState { request_id, state },
                        ))
                    }
                    Err(error) => Err(error.into()),
                };
            }
            Err(error) => return Err(error),
        };
        let run = base.run.clone();
        let outcome = self.validate_persist_proposal(
            &record.plan_branch_id,
            base,
            approved.changeset,
            approved.receipt_ref,
            &resumed_policy,
            None,
        );
        match &outcome {
            Ok(PlanningProposalOutcome::Accepted(_)) => {}
            Ok(_) => {
                self.finish_run(&run, PlanningRunState::Refused)?;
                self.store.supersede_unusable_human_decision(id)?;
            }
            Err(PlanningServiceError::Storage(StorageError::PlanningRunStale { .. }))
            | Err(PlanningServiceError::Storage(StorageError::PlanningInputStale { .. }))
            | Err(PlanningServiceError::Storage(StorageError::HumanDecisionStale { .. })) => {
                self.store.supersede_unusable_human_decision(id)?;
            }
            Err(_) => {
                self.finish_run(&run, PlanningRunState::Failed)?;
                self.store.supersede_unusable_human_decision(id)?;
            }
        }
        outcome
    }

    /// Rehydrates all authoritative publication inputs for a stored proposal
    /// without applying planner-call quotas or reserving new provider spend.
    fn prepare_approved_proposal(
        &mut self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        policy: &PlanningPolicyContext,
        approved_plan_head: u64,
    ) -> Result<ProposalBase, PlanningServiceError> {
        let trigger_key = trigger_key(triggering_evidence)?;
        let (head, graph) = self.load_base(branch)?;
        let lineage = self.store.get_plan_lineage(branch)?;
        let remaining_depth = remaining_depth(&graph);
        let state = self.guardrail_snapshot(branch, &trigger_key, &lineage)?;
        let (snapshot, run) = self.start_control_run_with_mode(
            branch,
            triggering_evidence,
            policy,
            head,
            &lineage,
            &trigger_key,
            ControlRunMode::ApprovedReplay { approved_plan_head },
        )?;
        Ok(ProposalBase {
            head,
            graph,
            lineage_fingerprints: lineage
                .into_iter()
                .map(|record| record.decomposition_fingerprint)
                .collect(),
            state,
            trigger_key,
            remaining_depth,
            snapshot,
            run,
        })
    }

    fn load_base(&self, branch: &PlanBranchId) -> Result<(u64, PlanGraph), PlanningServiceError> {
        let Some(head) = self.store.get_plan_head(branch)? else {
            return Ok((0, PlanGraph::new(vec![], vec![])?));
        };
        let graph = self.store.get_plan_graph(branch, head)?.ok_or_else(|| {
            PlanningServiceError::Storage(StorageError::Corrupt(format!(
                "plan branch {} has head {head} with no graph snapshot",
                branch.as_str()
            )))
        })?;
        Ok((head, graph))
    }

    fn guardrail_snapshot(
        &self,
        branch: &PlanBranchId,
        key: &str,
        lineage: &[PlanRevisionLineageRecord],
    ) -> Result<PlanningGuardrailState, PlanningServiceError> {
        let event = self
            .store
            .get_plan_guardrail_event(branch, key)?
            .unwrap_or_default();
        let revisions_accepted = lineage
            .iter()
            .filter(|record| record.trigger_evidence_key == key)
            .count();
        Ok(PlanningGuardrailState {
            planner_calls: event.planner_calls,
            charged_input_tokens: event.charged_input_tokens,
            consecutive_rejections: event.consecutive_rejections,
            revisions_accepted: u32::try_from(revisions_accepted).unwrap_or(u32::MAX),
        })
    }

    /// Reads every durable precondition and applies the pre-call guardrails
    /// (ADR-0027 §5: no LLM spend into a certain refusal).
    fn prepare_proposal(
        &mut self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        requested_depth: u32,
        policy: &PlanningPolicyContext,
    ) -> Result<Result<ProposalBase, GuardrailTrip>, PlanningServiceError> {
        let trigger_key = trigger_key(triggering_evidence)?;
        let (head, graph) = self.load_base(branch)?;
        let lineage = self.store.get_plan_lineage(branch)?;
        let remaining_depth = remaining_depth(&graph);
        if remaining_depth == 0 || requested_depth == 0 {
            return Ok(Err(GuardrailTrip::new(
                "depth",
                u64::from(MAX_DECOMPOSITION_DEPTH),
            )));
        }
        let state = self.guardrail_snapshot(branch, &trigger_key, &lineage)?;
        if let Some(trip) = pre_call_guardrail(&state, true) {
            return Ok(Err(trip));
        }
        let (snapshot, run) = self.start_control_run(
            branch,
            triggering_evidence,
            policy,
            head,
            &lineage,
            &trigger_key,
        )?;
        Ok(Ok(ProposalBase {
            head,
            graph,
            lineage_fingerprints: lineage
                .into_iter()
                .map(|record| record.decomposition_fingerprint)
                .collect(),
            state,
            trigger_key,
            remaining_depth,
            snapshot,
            run,
        }))
    }

    /// Opens a human gate from an assessment.
    ///
    /// Assessment holds its snapshot back from the branch's current planning
    /// input, but a gate is not an assessment result: the durable
    /// `HumanDecision` can only be created, and later resumed, while its bound
    /// snapshot is current. Publishing here is the same carve-out the pre-call
    /// gate already relies on, applied to the post-call gate (TASK-0116).
    fn open_assessment_human_gate(
        &mut self,
        branch: &PlanBranchId,
        policy: &PlanningPolicyContext,
        run: &PlanningRunRecord,
        snapshot: &PlanningInputSnapshot,
        reason: HumanDecisionReason,
    ) -> Result<HumanDecisionRequest, PlanningServiceError> {
        self.open_human_gate(
            branch,
            policy,
            run,
            snapshot,
            reason,
            &snapshot.triggering_evidence,
        )
    }

    /// Claims a planning generation for an operation that intends to publish a
    /// revision, so its snapshot becomes the branch's current planning input.
    fn start_control_run(
        &mut self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        policy: &PlanningPolicyContext,
        head: u64,
        durable_lineage: &[PlanRevisionLineageRecord],
        trigger_key: &str,
    ) -> Result<(PlanningInputSnapshot, PlanningRunRecord), PlanningServiceError> {
        self.start_control_run_with_mode(
            branch,
            triggering_evidence,
            policy,
            head,
            durable_lineage,
            trigger_key,
            ControlRunMode::Proposal,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_control_run_with_mode(
        &mut self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        policy: &PlanningPolicyContext,
        head: u64,
        durable_lineage: &[PlanRevisionLineageRecord],
        trigger_key: &str,
        mode: ControlRunMode,
    ) -> Result<(PlanningInputSnapshot, PlanningRunRecord), PlanningServiceError> {
        let mut lineage_fingerprints = policy.lineage_fingerprints.clone();
        lineage_fingerprints.extend(
            durable_lineage
                .iter()
                .map(|record| record.decomposition_fingerprint.clone()),
        );
        lineage_fingerprints.sort();
        lineage_fingerprints.dedup();
        let snapshot = PlanningInputSnapshot::new_with_candidates(
            branch.clone(),
            policy.parent_task_ref.clone(),
            trusted_candidate_task_refs(&self.store, policy)?,
            head,
            policy.scope_bound.clone(),
            policy.baseline_risk,
            policy.affected_parent_criteria.clone(),
            triggering_evidence.to_vec(),
            lineage_fingerprints,
            policy.risk_grant.clone(),
            policy.policy_fingerprint.clone(),
        )?;
        let run = match mode {
            ControlRunMode::ApprovedReplay { approved_plan_head } => {
                self.store.start_or_join_identical_planning_run(
                    &snapshot,
                    trigger_key,
                    approved_plan_head,
                    self.clock.now_millis(),
                )?
            }
            ControlRunMode::Proposal => {
                self.store
                    .start_planning_run(&snapshot, trigger_key, self.clock.now_millis())?
            }
            ControlRunMode::Assessment => self.store.start_assessment_planning_run(
                &snapshot,
                trigger_key,
                self.clock.now_millis(),
            )?,
            ControlRunMode::HumanGate => self.store.start_human_gate_planning_run(
                &snapshot,
                trigger_key,
                self.clock.now_millis(),
            )?,
        };
        Ok((snapshot, run))
    }

    /// A durable HumanDecisionRequest is a planning gate, not advisory data.
    /// Pending/resolved requests stop fresh planner calls for their owner
    /// scope. The only bypasses are the exact continuations being applied by
    /// the trusted application layer: a bound V9 RESUME grant or the exact
    /// REPLAN request currently being consumed.
    fn planning_human_gate(
        &mut self,
        branch: &PlanBranchId,
        policy: &PlanningPolicyContext,
        replan_decision_id: Option<&HumanDecisionRequestId>,
    ) -> Result<Option<HumanDecisionRequest>, PlanningServiceError> {
        let task_id = policy_task_id(policy)?;
        let Some(record) = self
            .store
            .latest_human_decision_for_scope(&task_id, branch)?
        else {
            return Ok(None);
        };
        let current = match self
            .store
            .validate_human_decision_current(&record.id, self.clock.now_millis())
        {
            Ok(current) => current,
            Err(StorageError::HumanDecisionStale { .. }) => {
                // A stale request is never made current again.  Superseding
                // it here is the trusted return path which lets this normal
                // planning pass create a bounded replacement if needed.
                self.store.supersede_unusable_human_decision(&record.id)?;
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        match current.status {
            HumanDecisionStatus::Pending => Ok(Some(current)),
            HumanDecisionStatus::Resolved
                if current.reason == HumanDecisionReason::RiskEscalation
                    && current.selected == Some(HumanContinuation::Resume)
                    && policy.risk_decision_id.as_ref() == Some(&current.id) =>
            {
                Ok(None)
            }
            HumanDecisionStatus::Resolved
                if current.selected == Some(HumanContinuation::Replan)
                    && replan_decision_id == Some(&current.id) =>
            {
                Ok(None)
            }
            HumanDecisionStatus::Resolved => Ok(Some(current)),
            HumanDecisionStatus::Consumed if current.selected == Some(HumanContinuation::Abort) => {
                Err(PlanningServiceError::Storage(
                    StorageError::HumanDecisionState {
                        request_id: current.id.as_str().to_owned(),
                        state: "ABORTED_BY_HUMAN".to_owned(),
                    },
                ))
            }
            HumanDecisionStatus::Consumed | HumanDecisionStatus::Superseded => Ok(None),
        }
    }

    /// Converts only guardrails whose canonical ADR semantics require human
    /// intervention. Ordinary validation limits remain typed guardrail
    /// outcomes for TASK-0080's failure matrix.
    fn pre_call_human_intervention(
        &mut self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        policy: &PlanningPolicyContext,
        durable_lineage: &[PlanRevisionLineageRecord],
        trip: GuardrailTrip,
    ) -> Result<Option<HumanDecisionRequest>, PlanningServiceError> {
        let Some(reason) = human_reason_for_guardrail(&trip) else {
            return Ok(None);
        };
        if let Some(existing) = self.planning_human_gate(branch, policy, None)? {
            return Ok(Some(existing));
        }
        let (head, _) = self.load_base(branch)?;
        let trigger_key = trigger_key(triggering_evidence)?;
        let (snapshot, run) = self.start_control_run_with_mode(
            branch,
            triggering_evidence,
            policy,
            head,
            durable_lineage,
            &trigger_key,
            ControlRunMode::HumanGate,
        )?;
        let decision =
            self.open_human_gate(branch, policy, &run, &snapshot, reason, triggering_evidence);
        let finished = self.finish_run(&run, PlanningRunState::Refused);
        match (decision, finished) {
            (Ok(decision), Ok(())) => Ok(Some(decision)),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }

    /// Opens a planning human gate: publishes the gate's snapshot as the
    /// branch's current planning input and records its `HumanDecision` in one
    /// transaction.
    ///
    /// A gate's decision can only be created, and later resumed, while its
    /// bound snapshot is current, so the gate has to publish. Doing that in a
    /// transaction of its own left a crash window where the branch's current
    /// input had moved with no gate recorded, which de-admits the previously
    /// accepted revision and does not heal on restart (TASK-0119).
    fn open_human_gate(
        &mut self,
        branch: &PlanBranchId,
        policy: &PlanningPolicyContext,
        run: &PlanningRunRecord,
        snapshot: &PlanningInputSnapshot,
        reason: HumanDecisionReason,
        evidence: &[EvidenceRef],
    ) -> Result<HumanDecisionRequest, PlanningServiceError> {
        let request =
            self.planning_human_decision_request(branch, policy, snapshot, reason, evidence)?;
        Ok(self
            .store
            .publish_planning_input_and_create_human_decision(
                run,
                snapshot,
                request,
                self.clock.now_millis(),
            )?)
    }

    /// Records a `HumanDecision` without touching the branch's current
    /// planning input.
    ///
    /// The proposal path uses this: its run claim already published the
    /// snapshot when it claimed the generation, so there is nothing further to
    /// publish here. A human gate must use [`Self::open_human_gate`] instead.
    fn create_planning_human_decision(
        &mut self,
        branch: &PlanBranchId,
        policy: &PlanningPolicyContext,
        snapshot: &PlanningInputSnapshot,
        reason: HumanDecisionReason,
        evidence: &[EvidenceRef],
    ) -> Result<HumanDecisionRequest, PlanningServiceError> {
        let request =
            self.planning_human_decision_request(branch, policy, snapshot, reason, evidence)?;
        Ok(self
            .store
            .create_human_decision_request(request, self.clock.now_millis())?)
    }

    fn planning_human_decision_request(
        &mut self,
        branch: &PlanBranchId,
        policy: &PlanningPolicyContext,
        snapshot: &PlanningInputSnapshot,
        reason: HumanDecisionReason,
        evidence: &[EvidenceRef],
    ) -> Result<NewHumanDecisionRequest, PlanningServiceError> {
        let task_id = policy_task_id(policy)?;
        Ok(NewHumanDecisionRequest {
            task_id,
            plan_branch_id: branch.clone(),
            reason,
            risk: policy.baseline_risk,
            affected_scope: policy.scope_bound.clone(),
            options: vec![HumanContinuation::Replan, HumanContinuation::Abort],
            recommendation: None,
            evidence: evidence.to_vec(),
            proposal_hash: None,
            approved_proposal: None,
            plan_head: snapshot.expected_plan_head,
            planning_input_snapshot_id: snapshot.id.clone(),
            planning_input_fingerprint: snapshot.input_fingerprint.clone(),
            policy_fingerprint: policy.policy_fingerprint.clone(),
            expires_at_ms: Some(
                self.clock
                    .now_millis()
                    .saturating_add(HUMAN_DECISION_TTL_MS),
            ),
        })
    }

    /// Reserves the call durably, invokes the planner, then settles the call
    /// with its measured receipt or a conservative charge in one transaction.
    /// No provider-started outcome can skip settlement.
    fn metered_call<T>(
        &mut self,
        branch: &PlanBranchId,
        key: &str,
        run: &PlanningRunRecord,
        invoke: impl FnOnce() -> Result<T, PlannerFault>,
        receipt_of: fn(&T) -> &PlanningCostReceipt,
    ) -> Result<Metered<T>, PlanningServiceError> {
        self.ensure_current_run(run)?;
        let Some(reserved) = self.store.reserve_plan_planner_call(
            branch,
            key,
            MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            CONSERVATIVE_CALL_CHARGE,
        )?
        else {
            return Ok(Metered::Tripped(GuardrailTrip::new(
                "planner-calls",
                u64::from(MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH).saturating_add(1),
            )));
        };

        let result = invoke();

        // A newer planning generation may have started while the provider
        // call was in flight. Spend still settles below, but the old worker
        // is no longer allowed to publish or report an authoritative result.

        let spend = match &result {
            Ok(value) => PlannerCallSpend::Measured(receipt_of(value).clone()),
            Err(fault) => fault.spend(),
        };
        let receipt_ref = call_receipt_ref(branch, key, reserved.planner_calls)?;
        let (charged_input_tokens, measured) = match &spend {
            PlannerCallSpend::Measured(receipt) => (
                u64::from(receipt.input_tokens),
                Some((&receipt_ref, receipt)),
            ),
            PlannerCallSpend::Unmeasured => (u64::from(CONSERVATIVE_CALL_CHARGE), None),
            PlannerCallSpend::NotStarted => (0, None),
        };
        let settled = self.store.settle_plan_planner_call(
            branch,
            key,
            &PlannerCallSettlement {
                reserved_input_tokens: CONSERVATIVE_CALL_CHARGE,
                charged_input_tokens,
                measured,
            },
        )?;
        self.ensure_current_run(run)?;

        if let PlannerCallSpend::Measured(receipt) = &spend {
            if let Some(trip) = per_call_envelope_trip(receipt) {
                return Ok(Metered::Tripped(trip));
            }
        }
        if settled.charged_input_tokens > u64::from(MAX_PLANNER_CUMULATIVE_INPUT_TOKENS) {
            return Ok(Metered::Tripped(GuardrailTrip::new(
                "planner-input-tokens",
                settled.charged_input_tokens,
            )));
        }
        Ok(match result {
            Ok(value) => Metered::Returned { value, receipt_ref },
            Err(fault) => Metered::Faulted(fault),
        })
    }

    fn ensure_current_run(&self, run: &PlanningRunRecord) -> Result<(), PlanningServiceError> {
        let current = self.store.current_planning_run(&run.plan_branch_id)?;
        if current.as_ref().is_some_and(|current| {
            current.id == run.id
                && current.generation == run.generation
                && current.snapshot_id == run.snapshot_id
                && current.input_fingerprint == run.input_fingerprint
                && current.state == PlanningRunState::Active
        }) {
            return Ok(());
        }
        Err(PlanningServiceError::Storage(
            StorageError::PlanningRunStale {
                branch: run.plan_branch_id.as_str().to_owned(),
                expected_generation: run.generation,
                found_generation: current.map_or(0, |value| value.generation),
            },
        ))
    }

    fn finish_run(
        &mut self,
        run: &PlanningRunRecord,
        state: PlanningRunState,
    ) -> Result<(), PlanningServiceError> {
        self.store
            .finish_planning_run(&run.id, run.generation, state, self.clock.now_millis())?;
        Ok(())
    }

    fn conclude_proposal(
        &mut self,
        branch: &PlanBranchId,
        base: ProposalBase,
        metered: Metered<PlanChangeSet>,
        policy: &PlanningPolicyContext,
        target: Option<&TaskId>,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        match metered {
            Metered::Tripped(trip) => {
                if let Some(reason) = human_reason_for_guardrail(&trip) {
                    let decision = self.create_planning_human_decision(
                        branch,
                        policy,
                        &base.snapshot,
                        reason,
                        &base.snapshot.triggering_evidence,
                    )?;
                    self.finish_run(&base.run, PlanningRunState::Refused)?;
                    return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
                }
                self.finish_run(&base.run, PlanningRunState::Refused)?;
                Ok(PlanningProposalOutcome::GuardrailTripped(trip))
            }
            Metered::Faulted(fault) => {
                let human_reason = match &fault {
                    PlannerFault::DepthExhausted | PlannerFault::Unplannable { .. } => {
                        Some(HumanDecisionReason::PlannerBlocked)
                    }
                    PlannerFault::BudgetExhausted { .. } => {
                        Some(HumanDecisionReason::PlannerBudget)
                    }
                    PlannerFault::ProviderFailure { .. } => None,
                };
                if let Some(reason) = human_reason {
                    let decision = self.create_planning_human_decision(
                        branch,
                        policy,
                        &base.snapshot,
                        reason,
                        &base.snapshot.triggering_evidence,
                    )?;
                    self.finish_run(&base.run, PlanningRunState::Failed)?;
                    return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
                }
                let outcome = planner_fault_outcome(fault);
                self.finish_run(&base.run, PlanningRunState::Failed)?;
                outcome
            }
            Metered::Returned { value, receipt_ref } => {
                let run = base.run.clone();
                let outcome = self.validate_persist_proposal(
                    branch,
                    base,
                    value,
                    receipt_ref,
                    policy,
                    target,
                );
                match &outcome {
                    Ok(PlanningProposalOutcome::Accepted(_)) => {}
                    Ok(_) => self.finish_run(&run, PlanningRunState::Refused)?,
                    Err(PlanningServiceError::Storage(StorageError::PlanningRunStale {
                        ..
                    }))
                    | Err(PlanningServiceError::Storage(StorageError::PlanningInputStale {
                        ..
                    })) => {}
                    Err(_) => {
                        let _ = self.finish_run(&run, PlanningRunState::Failed);
                    }
                }
                outcome
            }
        }
    }

    fn validate_persist_proposal(
        &mut self,
        branch: &PlanBranchId,
        base: ProposalBase,
        proposal: PlanChangeSet,
        receipt_ref: PlanningCostReceiptRef,
        policy: &PlanningPolicyContext,
        target: Option<&TaskId>,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        let ProposalBase {
            head: trusted_base_head,
            graph: base_graph,
            lineage_fingerprints,
            state,
            trigger_key,
            snapshot,
            run,
            ..
        } = base;

        self.ensure_current_run(&run)?;
        if !self.store.current_planning_input_matches(
            branch,
            &snapshot.id,
            &snapshot.input_fingerprint,
        )? {
            return Err(PlanningServiceError::Storage(
                StorageError::PlanningInputStale {
                    branch: branch.as_str().to_owned(),
                    expected_fingerprint: snapshot.input_fingerprint.clone(),
                },
            ));
        }

        // The provider call is deliberately outside storage transactions.
        // Re-check the exact head that owns `base_graph` before interpreting
        // provider output. A fresh durable head paired with this older graph
        // would turn V8 into an unsafe implicit rebase.
        if let Some(outcome) = self.stale_head_refusal(branch, &trigger_key, trusted_base_head)? {
            return Ok(outcome);
        }

        let depth_after_merge = merged_depth(&base_graph, &proposal);
        let mut lineage = policy.lineage_fingerprints.clone();
        lineage.extend(lineage_fingerprints);
        lineage.sort();
        lineage.dedup();
        let affected_parent_criteria =
            affected_criteria_for_proposal(&base_graph, &proposal, policy)?;
        let validation = ValidationContext {
            // V8 validates against the exact snapshot supplied to the
            // planner. Durable-head checks detect a concurrent writer
            // without pairing a fresh head with this stale graph.
            branch_head: trusted_base_head,
            scope_bound: policy.scope_bound.clone(),
            baseline_risk: policy.baseline_risk,
            affected_parent_criteria,
            depth_after_merge,
            revisions_accepted_this_cycle: state.revisions_accepted,
            parent_task_ref: policy.parent_task_ref.clone(),
            lineage_fingerprints: lineage,
            now_ms: self.clock.now_millis(),
            risk_grant: policy.risk_grant.clone(),
        };

        let risk_escalated = match validate_proposal(&base_graph, &proposal, &validation) {
            ValidationOutcome::Refused(refusal) => {
                if let Some(limit) = refusal.limit_detail() {
                    return self.record_rejection_with_human_intervention(
                        branch,
                        &trigger_key,
                        PlanningProposalOutcome::GuardrailTripped(GuardrailTrip::new(
                            limit.which,
                            limit.observed,
                        )),
                        policy,
                        &snapshot,
                        &proposal.source_evidence_refs,
                    );
                }
                if matches!(refusal, ValidationRefusal::RiskGateOpen { .. }) {
                    // A caller that already supplied a grant reached V9 with
                    // an invalid/expired/mismatched grant. Fail closed as the
                    // validation refusal; only the grant-absent path can mint
                    // a fresh HumanDecisionRequest.
                    if policy.risk_grant.is_some() {
                        return self.record_rejection_with_human_intervention(
                            branch,
                            &trigger_key,
                            PlanningProposalOutcome::Refused(refusal),
                            policy,
                            &snapshot,
                            &proposal.source_evidence_refs,
                        );
                    }
                    let task_id = target
                        .cloned()
                        .or_else(|| TaskId::new(policy.parent_task_ref.clone()).ok())
                        .ok_or_else(|| {
                            PlanningServiceError::InvalidContext(
                                "human decision requires a valid owning Task id".to_owned(),
                            )
                        })?;
                    if let Some(existing) = self.planning_human_gate(branch, policy, None)? {
                        return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(existing)));
                    }
                    let decision = self.store.create_human_decision_request(
                        NewHumanDecisionRequest {
                            task_id,
                            plan_branch_id: branch.clone(),
                            reason: HumanDecisionReason::RiskEscalation,
                            risk: proposal
                                .risk_delta
                                .iter()
                                .map(|delta| delta.to)
                                .max()
                                .unwrap_or(policy.baseline_risk),
                            affected_scope: ScopeBound::new(proposal.scope_delta.roots())?,
                            options: vec![
                                HumanContinuation::Resume,
                                HumanContinuation::Replan,
                                HumanContinuation::Abort,
                            ],
                            recommendation: None,
                            evidence: proposal.source_evidence_refs.clone(),
                            proposal_hash: Some(proposal.proposal_hash.clone()),
                            approved_proposal: Some(ApprovedPlanProposal {
                                changeset: proposal.clone(),
                                receipt_ref: receipt_ref.clone(),
                            }),
                            plan_head: trusted_base_head,
                            planning_input_snapshot_id: snapshot.id.clone(),
                            planning_input_fingerprint: snapshot.input_fingerprint.clone(),
                            policy_fingerprint: policy.policy_fingerprint.clone(),
                            expires_at_ms: Some(
                                self.clock
                                    .now_millis()
                                    .saturating_add(HUMAN_DECISION_TTL_MS),
                            ),
                        },
                        self.clock.now_millis(),
                    )?;
                    return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
                }
                return self.record_rejection_with_human_intervention(
                    branch,
                    &trigger_key,
                    PlanningProposalOutcome::Refused(refusal),
                    policy,
                    &snapshot,
                    &proposal.source_evidence_refs,
                );
            }
            ValidationOutcome::Accepted { risk_escalated } => risk_escalated,
        };

        let risk_decision = if risk_escalated {
            Some(policy.risk_decision_id.as_ref().ok_or_else(|| {
                PlanningServiceError::InvalidContext(
                    "risk escalation grant requires a durable HumanDecisionRequest binding"
                        .to_owned(),
                )
            })?)
        } else {
            None
        };

        // Structural validity is not product applicability. Resolve the
        // detached candidate against canonical Task state before durable
        // acceptance so no pre-detectable scheduling failure can follow a
        // committed revision. These Task reads are not in the revision's
        // transaction (separate bounded context); a Task changing in between
        // can only make an accepted plan wait, because plan-gated scheduling
        // re-derives predecessor and Task readiness before admission.
        let candidate = apply_validated_changeset(&base_graph, &proposal)?;
        // Trusted child identity (TASK-0087, Option B): planner output is
        // proposal data only. A `node_addition` id that names no existing
        // Task is a new child the accept path materializes READY atomically
        // below; a `node_addition` id that names the parent, or any existing
        // Task other than this proposal's own prior materialization (exact
        // retry), is refused here and again fail-closed by storage on the
        // commit path. Base-graph nodes keep the trusted-candidate rule.
        let pending_children = match child_identity_outcome(
            &self.store,
            &candidate,
            &snapshot,
            &proposal,
            &policy.parent_task_ref,
        )? {
            ChildIdentityOutcome::Pending(ids) => ids,
            ChildIdentityOutcome::Refused(refusal) => {
                return self.record_rejection_with_human_intervention(
                    branch,
                    &trigger_key,
                    PlanningProposalOutcome::NotApplicable(refusal),
                    policy,
                    &snapshot,
                    &proposal.source_evidence_refs,
                );
            }
        };
        if let Some(task_id) = target {
            if let Some(refusal) =
                target_applicability(&self.store, &candidate, task_id, &pending_children)?
            {
                return self.record_rejection_with_human_intervention(
                    branch,
                    &trigger_key,
                    PlanningProposalOutcome::NotApplicable(refusal),
                    policy,
                    &snapshot,
                    &proposal.source_evidence_refs,
                );
            }
        }

        if !has_runnable_node(&self.store, &candidate, &pending_children)? {
            let decision = self.create_planning_human_decision(
                branch,
                policy,
                &snapshot,
                HumanDecisionReason::PlannerBlocked,
                &proposal.source_evidence_refs,
            )?;
            return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
        }

        // Catch a writer that won after the first head check. Storage repeats
        // the same CAS inside BEGIN IMMEDIATE, closing the final race.
        if let Some(outcome) = self.stale_head_refusal(branch, &trigger_key, trusted_base_head)? {
            return Ok(outcome);
        }

        let revision_seq = trusted_base_head.saturating_add(1);
        let revision = PlanRevision::new(
            revision_seq,
            branch.clone(),
            trusted_base_head,
            proposal.proposal_hash.clone(),
            proposal.reason.clone(),
            proposal.source_evidence_refs.clone(),
            RevisionProposedBy::new(proposal.proposed_by.clone(), receipt_ref),
            ValidatedBy::new(
                VALIDATOR_SERVICE_ID.to_owned(),
                policy.policy_fingerprint.clone(),
            )?,
            self.clock.now_millis(),
        )?;
        let evidence = PlanCommitJournalEvidence {
            correlation_id: format!("plan:{}:{revision_seq}", branch.as_str()),
            causation_id: proposal
                .source_evidence_refs
                .first()
                .map(|value| value.as_str().to_owned())
                .unwrap_or_else(|| format!("plan:{}:base:{}", branch.as_str(), trusted_base_head)),
            occurred_at: format_rfc3339(self.clock.now_millis()),
            provenance: PLAN_PROVENANCE.to_owned(),
        };
        let lineage = PlanRevisionLineage {
            parent_task_ref: policy.parent_task_ref.clone(),
            trigger_evidence_key: trigger_key.clone(),
            planning_input_snapshot_id: snapshot.id.clone(),
            planning_input_fingerprint: snapshot.input_fingerprint.clone(),
            planning_run_id: run.id.clone(),
            planning_run_generation: run.generation,
        };

        let child_tasks = self.materialize_child_tasks(&proposal, policy, &pending_children)?;
        match self
            .store
            .commit_plan_revision_with_human_decision_and_children(
                storage_sqlite::CommitPlanRevisionRequest {
                    changeset: &proposal,
                    revision: &revision,
                    resulting_graph: &candidate,
                    lineage: &lineage,
                    evidence: &evidence,
                    risk_decision,
                    canonical_task_authority: policy.canonical_task_authority,
                    child_tasks: &child_tasks,
                },
            ) {
            Ok(_) => {}
            Err(StorageError::PlanStale { found, .. }) => {
                return self.record_rejection(
                    branch,
                    &trigger_key,
                    PlanningProposalOutcome::Refused(ValidationRefusal::StaleBase {
                        expected: found,
                        observed: trusted_base_head,
                    }),
                );
            }
            Err(error) => return Err(error.into()),
        }
        Ok(PlanningProposalOutcome::Accepted(Box::new(
            AcceptedPlanRevision {
                revision,
                graph: candidate,
                planning_cost_receipt: proposal.planning_cost_receipt,
                planning_input_snapshot_id: snapshot.id,
                planning_input_fingerprint: snapshot.input_fingerprint,
            },
        )))
    }

    /// Materializes planner-proposed nodes only after the proposal has passed
    /// trusted validation. Planner output remains proposal data; this method
    /// is the application-owned boundary that mints canonical Task identity.
    ///
    /// Every addition reaching here was proved brand-new, or an exact retry
    /// of this proposal's own materialization, by [`child_identity_outcome`]:
    /// no existence skip exists on this path, so reconciliation stays
    /// executable and storage remains the authority that re-fences
    /// availability inside the commit transaction (a conflicting or
    /// concurrent claim of the same id fails the whole commit closed).
    fn materialize_child_tasks(
        &mut self,
        proposal: &PlanChangeSet,
        policy: &PlanningPolicyContext,
        pending_children: &BTreeSet<String>,
    ) -> Result<Vec<ChildTaskMaterialization>, PlanningServiceError> {
        let mut children = Vec::new();
        for addition in &proposal.node_additions {
            if !pending_children.contains(addition.id.as_str()) {
                continue;
            }
            // ADR-0030 §6: the obligation travels. The plan-side proof method
            // maps onto the child's explicit evidence requirements through the
            // §3 mapping, so a strengthened obligation (independent review,
            // security check, human approval) materializes as itself instead
            // of being refused. This replaces the ADR-0027 TASK-0096
            // materialization refusal; a method the plan vocabulary cannot
            // express remains unrepresentable and fails closed at the parser,
            // never here.
            let task_id = TaskId::new(addition.id.as_str().to_owned())
                .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            let project_id = self
                .store
                .get_task(
                    &TaskId::new(policy.parent_task_ref.clone())
                        .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?,
                )?
                .ok_or_else(|| {
                    PlanningServiceError::InvalidContext(
                        "parent task disappeared during child materialization".to_owned(),
                    )
                })?
                .project_id()
                .clone();
            let mut child = Task::draft(
                task_id,
                project_id,
                format!("planned child task {}", addition.id.as_str()),
            )
            .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            child
                .set_parent(
                    TaskId::new(policy.parent_task_ref.clone())
                        .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?,
                )
                .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            child
                .define_planning_metadata(
                    addition.scope.roots(),
                    addition
                        .criteria
                        .iter()
                        .map(|criterion| criterion.id.as_str().to_owned())
                        .collect(),
                    // Domain-safe serialization: the canonical lowercase
                    // risk name (`low`/`medium`/...) that
                    // `TaskPlanningAuthority::derive` parses back. `Debug`
                    // (`Low`/`Medium`/...) would persist an unparseable
                    // value and break derived planning authority.
                    addition.risk_class.as_str(),
                )
                .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            // ADR-0030 §6 carriage: the child's criteria arrive as explicit
            // evidence requirements, derived under the planning policy
            // fingerprint that validated this proposal. Parameters stay empty
            // (the verification boundary resolves kind-specific selectors).
            let requirements = addition
                .criteria
                .iter()
                .map(|criterion| {
                    let kind = requirement_kind_for_method(criterion.method);
                    EvidenceRequirement::new(
                        criterion.id.as_str(),
                        kind,
                        kind.default_subject_binding(),
                        Vec::new(),
                        Some(policy.policy_fingerprint.clone()),
                        true,
                    )
                })
                .collect::<Result<Vec<_>, task_domain::DomainError>>()
                .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            child
                .define_evidence_requirements(requirements)
                .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            let created_event = child_lifecycle_event(
                &child,
                EventType::TaskCreated,
                TaskState::Draft,
                Some("plan child materialization".to_owned()),
                proposal,
                self.clock.now_millis(),
            );
            // A validated child is executable work, never a DRAFT
            // placeholder: it is marked READY in the same atomic commit
            // (ADR-0027 L4 counts it only through that guarantee).
            child
                .mark_ready()
                .map_err(|error| PlanningServiceError::InvalidContext(error.to_string()))?;
            let ready_event = child_lifecycle_event(
                &child,
                EventType::MarkedReady,
                TaskState::Draft,
                None,
                proposal,
                self.clock.now_millis(),
            );
            children.push(ChildTaskMaterialization {
                task: child,
                created_event,
                ready_event,
            });
        }
        Ok(children)
    }

    fn stale_head_refusal(
        &mut self,
        branch: &PlanBranchId,
        trigger_key: &str,
        trusted_base_head: u64,
    ) -> Result<Option<PlanningProposalOutcome>, PlanningServiceError> {
        let durable_head = self.store.get_plan_head(branch)?.unwrap_or(0);
        if durable_head == trusted_base_head {
            return Ok(None);
        }
        self.record_rejection(
            branch,
            trigger_key,
            PlanningProposalOutcome::Refused(ValidationRefusal::StaleBase {
                expected: durable_head,
                observed: trusted_base_head,
            }),
        )
        .map(Some)
    }

    /// Counts one refused proposal against the durable L3 state of its
    /// triggering evidence event.
    fn record_rejection(
        &mut self,
        branch: &PlanBranchId,
        trigger_key: &str,
        refusal: PlanningProposalOutcome,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        let event = self
            .store
            .record_plan_proposal_rejection(branch, trigger_key)?;
        if l3_no_progress_tripped(event.consecutive_rejections) {
            return Ok(PlanningProposalOutcome::GuardrailTripped(
                GuardrailTrip::new("no-progress", u64::from(event.consecutive_rejections)),
            ));
        }
        Ok(refusal)
    }

    fn record_rejection_with_human_intervention(
        &mut self,
        branch: &PlanBranchId,
        trigger_key: &str,
        refusal: PlanningProposalOutcome,
        policy: &PlanningPolicyContext,
        snapshot: &PlanningInputSnapshot,
        evidence: &[EvidenceRef],
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        let outcome = self.record_rejection(branch, trigger_key, refusal)?;
        if matches!(
            &outcome,
            PlanningProposalOutcome::GuardrailTripped(trip) if trip.bound == "no-progress"
        ) {
            let decision = self.create_planning_human_decision(
                branch,
                policy,
                snapshot,
                HumanDecisionReason::NoProgress,
                evidence,
            )?;
            return Ok(PlanningProposalOutcome::WaitingForHuman(Box::new(decision)));
        }
        Ok(outcome)
    }
}

fn proposal_receipt(proposal: &PlanChangeSet) -> &PlanningCostReceipt {
    &proposal.planning_cost_receipt
}

fn assessment_receipt(assessment: &AtomicityAssessment) -> &PlanningCostReceipt {
    &assessment.planning_cost_receipt
}

/// Trusted triggering-evidence identity. Planning without evidence has no
/// event to bound, so it is refused before any read or call.
fn trigger_key(evidence: &[EvidenceRef]) -> Result<String, PlanningServiceError> {
    if evidence.is_empty() {
        return Err(PlanningServiceError::InvalidContext(
            "planning requires at least one triggering evidence ref".to_owned(),
        ));
    }
    Ok(planning_evidence_key(evidence))
}

/// Product-owned identity of one planner call: branch, evidence event and
/// the durable 1-based call sequence. Independent of proposal content, so
/// malformed or refused calls still get exactly one receipt identity.
fn call_receipt_ref(
    branch: &PlanBranchId,
    trigger_key: &str,
    call_seq: u32,
) -> Result<PlanningCostReceiptRef, PlanningServiceError> {
    Ok(PlanningCostReceiptRef::new(format!(
        "planning-call:{}:{trigger_key}:{call_seq}",
        branch.as_str()
    ))?)
}

fn pre_call_guardrail(
    state: &PlanningGuardrailState,
    creates_revisions: bool,
) -> Option<GuardrailTrip> {
    if l3_no_progress_tripped(state.consecutive_rejections) {
        return Some(GuardrailTrip::new(
            "no-progress",
            u64::from(state.consecutive_rejections),
        ));
    }
    if creates_revisions && state.revisions_accepted >= MAX_REVISIONS_PER_CYCLE_PER_BRANCH {
        return Some(GuardrailTrip::new(
            "revisions-per-cycle",
            u64::from(state.revisions_accepted.saturating_add(1)),
        ));
    }
    if state.planner_calls >= MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        return Some(GuardrailTrip::new(
            "planner-calls",
            u64::from(state.planner_calls.saturating_add(1)),
        ));
    }
    if state.charged_input_tokens >= u64::from(MAX_PLANNER_CUMULATIVE_INPUT_TOKENS) {
        return Some(GuardrailTrip::new(
            "planner-input-tokens",
            state.charged_input_tokens,
        ));
    }
    None
}

fn remaining_budget(state: &PlanningGuardrailState) -> PlanningBudget {
    let tokens =
        u64::from(MAX_PLANNER_CUMULATIVE_INPUT_TOKENS).saturating_sub(state.charged_input_tokens);
    PlanningBudget {
        calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH.saturating_sub(state.planner_calls),
        input_tokens_remaining: u32::try_from(tokens).unwrap_or(0),
    }
}

fn per_call_envelope_trip(receipt: &PlanningCostReceipt) -> Option<GuardrailTrip> {
    if call_within_envelope(receipt) {
        return None;
    }
    if receipt.wall_time_ms > MAX_PLANNER_WALL_CLOCK_SECS * 1_000 {
        return Some(GuardrailTrip::new(
            "planner-wall-clock",
            receipt.wall_time_ms,
        ));
    }
    if receipt.input_tokens > MAX_PLANNER_INPUT_TOKENS {
        return Some(GuardrailTrip::new(
            "planner-input-tokens-per-call",
            u64::from(receipt.input_tokens),
        ));
    }
    if receipt.output_tokens > MAX_PLANNER_OUTPUT_TOKENS {
        return Some(GuardrailTrip::new(
            "planner-output-tokens-per-call",
            u64::from(receipt.output_tokens),
        ));
    }
    Some(GuardrailTrip::new("planner-call-envelope", 1))
}

fn affected_criteria_for_proposal(
    base_graph: &PlanGraph,
    proposal: &PlanChangeSet,
    policy: &PlanningPolicyContext,
) -> Result<Vec<AffectedParentCriterion>, PlanningServiceError> {
    let removed: BTreeSet<&str> = proposal
        .node_removals
        .iter()
        .map(|node| node.as_str())
        .collect();
    let mut affected = BTreeMap::<String, AffectedParentCriterion>::new();

    for criterion in &policy.affected_parent_criteria {
        affected.insert(criterion.id.as_str().to_owned(), criterion.clone());
    }
    for node in &base_graph.nodes {
        if !removed.contains(node.id.as_str()) {
            continue;
        }
        for criterion in &node.criteria {
            let candidate = AffectedParentCriterion {
                id: criterion.id.clone(),
                method: criterion.method,
            };
            if let Some(existing) = affected.get(candidate.id.as_str()) {
                if existing.method != candidate.method {
                    return Err(PlanningServiceError::InvalidContext(format!(
                        "criterion {} has conflicting verification methods in policy and durable plan",
                        candidate.id.as_str()
                    )));
                }
            } else {
                affected.insert(candidate.id.as_str().to_owned(), candidate);
            }
        }
    }

    Ok(affected.into_values().collect())
}

fn validate_policy_context(policy: &PlanningPolicyContext) -> Result<(), PlanningServiceError> {
    if policy.parent_task_ref.trim().is_empty() {
        return Err(PlanningServiceError::InvalidContext(
            "parent_task_ref must be non-blank".to_owned(),
        ));
    }
    if policy.policy_fingerprint.trim().is_empty() {
        return Err(PlanningServiceError::InvalidContext(
            "policy_fingerprint must be non-blank".to_owned(),
        ));
    }
    if policy.risk_grant.is_some() != policy.risk_decision_id.is_some() {
        return Err(PlanningServiceError::InvalidContext(
            "risk escalation grant and durable HumanDecisionRequest binding must be supplied together"
                .to_owned(),
        ));
    }
    Ok(())
}

fn policy_task_id(policy: &PlanningPolicyContext) -> Result<TaskId, PlanningServiceError> {
    TaskId::new(policy.parent_task_ref.clone()).map_err(|error| {
        PlanningServiceError::InvalidContext(format!(
            "planning policy parent_task_ref is not a valid Task id: {error}"
        ))
    })
}

/// Resolves the only canonical Task identities a planner may name for one
/// branch: its owning parent Task and descendants linked through the existing
/// Task parent relation. The result is sorted by the domain snapshot builder
/// before it is fingerprinted and persisted with the planning run.
fn trusted_candidate_task_refs(
    store: &SqliteStore,
    policy: &PlanningPolicyContext,
) -> Result<Vec<String>, PlanningServiceError> {
    let parent_id = policy_task_id(policy)?;
    let parent = store.get_task(&parent_id)?.ok_or_else(|| {
        PlanningServiceError::InvalidContext(format!(
            "planning policy parent task {} does not exist",
            parent_id.as_str()
        ))
    })?;
    let tasks = store.list_tasks_for_project(parent.project_id())?;
    Ok(plan_domain::trusted_candidate_task_refs(
        parent_id.as_str(),
        tasks
            .iter()
            .map(|task| (task.id().as_str(), task.parent().map(TaskId::as_str))),
    ))
}

/// One planner-child lifecycle fact minted by the planning application
/// service. Correlation and causation bind it to the materialized node and
/// the accepted proposal: the key storage uses to recognize exact retries.
fn child_lifecycle_event(
    child: &Task,
    event_type: EventType,
    from_state: TaskState,
    reason: Option<String>,
    proposal: &PlanChangeSet,
    now_millis: u64,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type,
        task_id: Some(child.id().clone()),
        project_id: Some(child.project_id().clone()),
        from_state: Some(from_state),
        to_state: Some(child.state()),
        reason,
        correlation_id: format!("plan-child:{}", child.id().as_str()),
        causation_id: proposal.proposal_hash.clone(),
        occurred_at: format_rfc3339(now_millis),
        provenance: CHILD_TASK_MATERIALIZATION_PROVENANCE.to_owned(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

/// Outcome of resolving candidate Plan node identities against canonical
/// Task state (TASK-0087 trusted child materialization, Option B).
enum ChildIdentityOutcome {
    /// Every candidate node is trusted. Carries the `node_addition` ids that
    /// name brand-new Tasks the accept path must materialize atomically.
    Pending(BTreeSet<String>),
    /// A candidate node names an identity the planner cannot own.
    Refused(PlanApplicabilityRefusal),
}

/// Resolves the only canonical Task identities a planner may name for one
/// branch: the trusted snapshot set (owning parent plus existing descendants)
/// for already-materialized nodes, plus `node_addition` ids that the accept
/// path materializes. Planner output never owns an existing Task: an addition
/// naming the parent, or colliding with any stored Task that is not an exact
/// retry of this proposal's own materialization (same parent, child
/// provenance, proposal-hash causation; storage re-checks the full snapshot),
/// is refused, as is any non-addition node outside the trusted set.
fn child_identity_outcome(
    store: &SqliteStore,
    candidate: &PlanGraph,
    snapshot: &PlanningInputSnapshot,
    proposal: &PlanChangeSet,
    parent_task_ref: &str,
) -> Result<ChildIdentityOutcome, PlanningServiceError> {
    let additions: BTreeSet<&str> = proposal
        .node_additions
        .iter()
        .map(|addition| addition.id.as_str())
        .collect();
    let mut pending = BTreeSet::new();
    for node in &candidate.nodes {
        let id = node.id.as_str();
        if additions.contains(id) {
            let task_id = TaskId::new(id.to_owned()).map_err(|error| {
                PlanningServiceError::InvalidContext(format!(
                    "plan addition {id} is not a valid Task id: {error}"
                ))
            })?;
            // A proposed child addition creates a new materialized identity.
            // Existing tasks cannot be rebound by planner output. Storage
            // still accepts exact retries through its own reconciliation
            // boundary, but the planner application layer must not silently
            // bind an arbitrary existing id into a new proposal.
            if id == parent_task_ref {
                return Ok(ChildIdentityOutcome::Refused(
                    PlanApplicabilityRefusal::UntrustedNodeIdentity {
                        node: id.to_owned(),
                    },
                ));
            }
            if let Some(existing) = store.get_task(&task_id)? {
                let exact_retry = existing
                    .parent()
                    .is_some_and(|parent| parent.as_str() == parent_task_ref)
                    && store.list_events_for_task(&task_id)?.iter().any(|event| {
                        event.fact.event_type == storage_sqlite::EventType::TaskCreated
                            && event.fact.provenance
                                == storage_sqlite::CHILD_TASK_MATERIALIZATION_PROVENANCE
                            && event.fact.causation_id == proposal.proposal_hash
                    });
                if !exact_retry {
                    return Ok(ChildIdentityOutcome::Refused(
                        PlanApplicabilityRefusal::UntrustedNodeIdentity {
                            node: id.to_owned(),
                        },
                    ));
                }
            }
            pending.insert(id.to_owned());
        } else if !snapshot
            .trusted_candidate_task_refs
            .iter()
            .any(|candidate| candidate == id)
        {
            return Ok(ChildIdentityOutcome::Refused(
                PlanApplicabilityRefusal::UntrustedNodeIdentity {
                    node: id.to_owned(),
                },
            ));
        }
    }
    Ok(ChildIdentityOutcome::Pending(pending))
}

fn human_reason_for_guardrail(trip: &GuardrailTrip) -> Option<HumanDecisionReason> {
    match trip.bound {
        "no-progress" => Some(HumanDecisionReason::NoProgress),
        "planner-calls" | "planner-input-tokens" | "planner-budget" => {
            Some(HumanDecisionReason::PlannerBudget)
        }
        "depth" | "runnable-leaf" => Some(HumanDecisionReason::PlannerBlocked),
        _ => None,
    }
}

fn planner_fault_outcome(
    fault: PlannerFault,
) -> Result<PlanningProposalOutcome, PlanningServiceError> {
    match fault {
        PlannerFault::DepthExhausted => Ok(PlanningProposalOutcome::GuardrailTripped(
            GuardrailTrip::new("depth", u64::from(MAX_DECOMPOSITION_DEPTH)),
        )),
        PlannerFault::BudgetExhausted { .. } => Ok(PlanningProposalOutcome::GuardrailTripped(
            GuardrailTrip::new("planner-budget", 1),
        )),
        PlannerFault::Unplannable { blocker, .. } => Ok(PlanningProposalOutcome::Blocked(blocker)),
        failure @ PlannerFault::ProviderFailure { .. } => {
            Err(PlanningServiceError::Planner(failure))
        }
    }
}
