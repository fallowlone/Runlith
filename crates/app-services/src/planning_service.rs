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
    call_within_envelope, l3_no_progress_tripped, planning_evidence_key, validate_proposal,
    AffectedParentCriterion, AtomicityAssessment, AtomicityBlocker, AtomicityRequest,
    DecompositionRequest, EvidenceRef, GuardrailTrip, PlanBranchId, PlanChangeSet, PlanDomainError,
    PlanGraph, PlanRevision, PlannerCallSpend, PlannerFault, PlannerPort, PlanningBudget,
    PlanningCostReceipt, PlanningCostReceiptRef, PlanningInputSnapshot, PlanningInputSnapshotId,
    ReplanRequest, RevisionProposedBy, RiskClass, RiskEscalationGrant, ScopeBound, ValidatedBy,
    ValidationContext, ValidationOutcome, ValidationRefusal, MAX_DECOMPOSITION_DEPTH,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
    MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_OUTPUT_TOKENS, MAX_PLANNER_WALL_CLOCK_SECS,
    MAX_REVISIONS_PER_CYCLE_PER_BRANCH,
};
use storage_sqlite::{
    PlanCommitJournalEvidence, PlanRevisionLineage, PlanRevisionLineageRecord,
    PlannerCallSettlement, PlanningRunRecord, PlanningRunState, SqliteStore, StorageError,
};
use task_domain::TaskId;

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
    pub lineage_fingerprints: Vec<String>,
    pub risk_grant: Option<RiskEscalationGrant>,
    pub policy_fingerprint: String,
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
        let key = trigger_key(&request.evidence_refs)?;
        let lineage = self.store.get_plan_lineage(branch)?;
        let state = self.guardrail_snapshot(branch, &key, &lineage)?;
        if let Some(trip) = pre_call_guardrail(&state, false) {
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
        let (snapshot, run) =
            self.start_control_run(branch, &request.evidence_refs, policy, head, &lineage, &key)?;

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
                self.finish_run(&run, PlanningRunState::Refused)?;
                Ok(PlanningAssessmentOutcome::GuardrailTripped(trip))
            }
            Metered::Faulted(fault) => match planner_fault_outcome(fault)? {
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
            },
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
        let base = match self.prepare_proposal(
            branch,
            &request.evidence_refs,
            request.depth_remaining,
            policy,
        )? {
            Ok(base) => base,
            Err(trip) => return Ok(PlanningProposalOutcome::GuardrailTripped(trip)),
        };

        request.parent_task_ref = policy.parent_task_ref.clone();
        request.base_plan_revision = base.head;
        request.acceptance_criteria_refs = policy
            .affected_parent_criteria
            .iter()
            .map(|criterion| criterion.id.clone())
            .collect();
        request.scope_bound = policy.scope_bound.clone();
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
        mut request: ReplanRequest,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        let base = match self.prepare_proposal(
            branch,
            &request.triggering_evidence_refs,
            request.depth_remaining,
            policy,
        )? {
            Ok(base) => base,
            Err(trip) => return Ok(PlanningProposalOutcome::GuardrailTripped(trip)),
        };

        request.branch_root_ref = policy.parent_task_ref.clone();
        request.base_plan_revision = base.head;
        request.scope_bound = policy.scope_bound.clone();
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

    fn start_control_run(
        &mut self,
        branch: &PlanBranchId,
        triggering_evidence: &[EvidenceRef],
        policy: &PlanningPolicyContext,
        head: u64,
        durable_lineage: &[PlanRevisionLineageRecord],
        trigger_key: &str,
    ) -> Result<(PlanningInputSnapshot, PlanningRunRecord), PlanningServiceError> {
        let mut lineage_fingerprints = policy.lineage_fingerprints.clone();
        lineage_fingerprints.extend(
            durable_lineage
                .iter()
                .map(|record| record.decomposition_fingerprint.clone()),
        );
        lineage_fingerprints.sort();
        lineage_fingerprints.dedup();
        let snapshot = PlanningInputSnapshot::new(
            branch.clone(),
            policy.parent_task_ref.clone(),
            head,
            policy.scope_bound.clone(),
            policy.baseline_risk,
            policy.affected_parent_criteria.clone(),
            triggering_evidence.to_vec(),
            lineage_fingerprints,
            policy.risk_grant.clone(),
            policy.policy_fingerprint.clone(),
        )?;
        let run = self
            .store
            .start_planning_run(&snapshot, trigger_key, self.clock.now_millis())?;
        Ok((snapshot, run))
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
                self.finish_run(&base.run, PlanningRunState::Refused)?;
                Ok(PlanningProposalOutcome::GuardrailTripped(trip))
            }
            Metered::Faulted(fault) => {
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

        match validate_proposal(&base_graph, &proposal, &validation) {
            ValidationOutcome::Refused(refusal) => {
                if let Some(limit) = refusal.limit_detail() {
                    return Ok(PlanningProposalOutcome::GuardrailTripped(
                        GuardrailTrip::new(limit.which, limit.observed),
                    ));
                }
                return self.record_rejection(
                    branch,
                    &trigger_key,
                    PlanningProposalOutcome::Refused(refusal),
                );
            }
            ValidationOutcome::Accepted { .. } => {}
        }

        // Structural validity is not product applicability. Resolve the
        // detached candidate against canonical Task state before durable
        // acceptance so no pre-detectable scheduling failure can follow a
        // committed revision. These Task reads are not in the revision's
        // transaction (separate bounded context); a Task changing in between
        // can only make an accepted plan wait, because plan-gated scheduling
        // re-derives predecessor and Task readiness before admission.
        let candidate = apply_validated_changeset(&base_graph, &proposal)?;
        if let Some(task_id) = target {
            if let Some(refusal) = target_applicability(&self.store, &candidate, task_id)? {
                return self.record_rejection(
                    branch,
                    &trigger_key,
                    PlanningProposalOutcome::NotApplicable(refusal),
                );
            }
        }
        if !has_runnable_node(&self.store, &candidate)? {
            return Ok(PlanningProposalOutcome::GuardrailTripped(
                GuardrailTrip::new("runnable-leaf", 0),
            ));
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

        match self
            .store
            .commit_plan_revision(&proposal, &revision, &candidate, &lineage, &evidence)
        {
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
    Ok(())
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
