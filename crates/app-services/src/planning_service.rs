//! Trusted G7 planning application service (TASK-0078).
//!
//! `PlannerPort` output is proposal data only. This service owns the accepted
//! control flow: bounded call -> canonical V1-V9 validation -> detached graph
//! construction -> L4 leaf guard -> atomic TASK-0077 persistence. Only an
//! [`AcceptedPlanRevision`] minted after the durable commit may gate execution
//! through the existing `OrchestrationService` scheduler path.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::path::Path;

use plan_domain::{
    call_within_envelope, compute_proposal_hash, decomposition_fingerprint, l3_no_progress_tripped,
    merged_graph_has_leaf, validate_proposal, AffectedParentCriterion, AtomicityBlocker,
    DecompositionRequest, EvidenceRef, GateUpdate, GuardrailTrip, PlanBranchId, PlanChangeSet,
    PlanDomainError, PlanGraph, PlanNode, PlanRevision, PlannerFault, PlannerPort, PlanningBudget,
    PlanningCostReceipt, PlanningCostReceiptRef, ReplanRequest, RevisionProposedBy, RiskClass,
    RiskEscalationGrant, ScopeBound, ValidatedBy, ValidationContext, ValidationOutcome,
    ValidationRefusal, MAX_DECOMPOSITION_DEPTH, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS, MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_OUTPUT_TOKENS,
    MAX_PLANNER_WALL_CLOCK_SECS, MAX_REVISIONS_PER_CYCLE_PER_BRANCH,
};
use scheduler::{ExecutionPort, OrchestrationOutcome};
use storage_sqlite::{PlanCommitJournalEvidence, SqliteStore, StorageError};
use task_domain::{TaskId, TaskState};

use crate::clock::{format_rfc3339, Clock, SystemClock};
use crate::error::ServiceError;
use crate::orchestration_service::{OrchestrationIds, OrchestrationService};
use crate::EvaluationPort;

const VALIDATOR_SERVICE_ID: &str = "plan-application-service";
const PLAN_PROVENANCE: &str = "app-services/planning-service";

/// Trusted non-planner inputs used by canonical V1-V9 validation.
///
/// These values come from product policy/reconciliation state. They are never
/// accepted from provider output.
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

/// Per-branch, per-triggering-evidence planning budget state.
///
/// `begin_new_evidence_event` resets the event envelope and one-revision cycle
/// while retaining accepted lineage fingerprints for duplicate-decomposition
/// detection during the running reconciler lifetime.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlanningCycle {
    calls_made: u32,
    cumulative_input_tokens: u32,
    revisions_accepted: u32,
    consecutive_rejections: u32,
    last_rejection_evidence: Option<Vec<EvidenceRef>>,
    lineage_fingerprints: Vec<String>,
}

impl PlanningCycle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn calls_made(&self) -> u32 {
        self.calls_made
    }

    #[must_use]
    pub const fn cumulative_input_tokens(&self) -> u32 {
        self.cumulative_input_tokens
    }

    #[must_use]
    pub const fn revisions_accepted(&self) -> u32 {
        self.revisions_accepted
    }

    /// Starts a new budget/reconcile cycle after fresh execution evidence.
    pub fn begin_new_evidence_event(&mut self) {
        self.calls_made = 0;
        self.cumulative_input_tokens = 0;
        self.revisions_accepted = 0;
        self.consecutive_rejections = 0;
        self.last_rejection_evidence = None;
    }

    fn remaining_budget(&self) -> PlanningBudget {
        PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH.saturating_sub(self.calls_made),
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS
                .saturating_sub(self.cumulative_input_tokens),
        }
    }

    fn pre_call_guardrail(&self) -> Option<GuardrailTrip> {
        if l3_no_progress_tripped(self.consecutive_rejections) {
            return Some(GuardrailTrip::new(
                "no-progress",
                u64::from(self.consecutive_rejections),
            ));
        }
        if self.revisions_accepted >= MAX_REVISIONS_PER_CYCLE_PER_BRANCH {
            return Some(GuardrailTrip::new(
                "revisions-per-cycle",
                u64::from(self.revisions_accepted.saturating_add(1)),
            ));
        }
        if self.calls_made >= MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
            return Some(GuardrailTrip::new(
                "planner-calls",
                u64::from(self.calls_made.saturating_add(1)),
            ));
        }
        if self.cumulative_input_tokens >= MAX_PLANNER_CUMULATIVE_INPUT_TOKENS {
            return Some(GuardrailTrip::new(
                "planner-input-tokens",
                u64::from(self.cumulative_input_tokens),
            ));
        }
        None
    }

    fn start_call(&mut self) {
        self.calls_made = self.calls_made.saturating_add(1);
    }

    /// A provider failure may happen after spend but before a typed receipt
    /// can cross `PlannerPort` (for example malformed provider output). The
    /// stable fault taxonomy carries no usage evidence, so charge one full
    /// per-call input allowance conservatively. This is budget reservation,
    /// not fabricated measurement; exact receipts are recorded only when the
    /// provider supplied them.
    fn charge_unreceipted_provider_failure(&mut self) {
        let remaining =
            MAX_PLANNER_CUMULATIVE_INPUT_TOKENS.saturating_sub(self.cumulative_input_tokens);
        self.cumulative_input_tokens = self
            .cumulative_input_tokens
            .saturating_add(remaining.min(MAX_PLANNER_INPUT_TOKENS));
    }

    fn observe_receipt(&mut self, receipt: &PlanningCostReceipt) -> Option<GuardrailTrip> {
        self.cumulative_input_tokens = self
            .cumulative_input_tokens
            .saturating_add(receipt.input_tokens);
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
        if self.calls_made > MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
            return Some(GuardrailTrip::new(
                "planner-calls",
                u64::from(self.calls_made),
            ));
        }
        if self.cumulative_input_tokens > MAX_PLANNER_CUMULATIVE_INPUT_TOKENS {
            return Some(GuardrailTrip::new(
                "planner-input-tokens",
                u64::from(self.cumulative_input_tokens),
            ));
        }
        if !call_within_envelope(receipt) {
            return Some(GuardrailTrip::new("planner-call-envelope", 1));
        }
        None
    }

    fn record_rejection(&mut self, evidence: &[EvidenceRef]) -> bool {
        if self.last_rejection_evidence.as_deref() == Some(evidence) {
            self.consecutive_rejections = self.consecutive_rejections.saturating_add(1);
        } else {
            self.consecutive_rejections = 1;
            self.last_rejection_evidence = Some(evidence.to_vec());
        }
        l3_no_progress_tripped(self.consecutive_rejections)
    }

    fn record_acceptance(&mut self, fingerprint: String) {
        self.revisions_accepted = self.revisions_accepted.saturating_add(1);
        self.consecutive_rejections = 0;
        self.last_rejection_evidence = None;
        if !self.lineage_fingerprints.contains(&fingerprint) {
            self.lineage_fingerprints.push(fingerprint);
        }
    }
}

/// An accepted revision capability minted only after TASK-0077's atomic
/// revision+graph+journal commit succeeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptedPlanRevision {
    revision: PlanRevision,
    graph: PlanGraph,
    planning_cost_receipt: PlanningCostReceipt,
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
        for (from, to) in &self.graph.edges {
            if to.as_str() != id {
                continue;
            }
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

/// One bounded proposal application result. Refusal and guardrail exhaustion
/// are data, not half-committed failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanningProposalOutcome {
    Accepted(Box<AcceptedPlanRevision>),
    Refused(ValidationRefusal),
    GuardrailTripped(GuardrailTrip),
    Blocked(AtomicityBlocker),
}

/// Planning application-service failure. No variant implies a successful
/// durable mutation unless an `AcceptedPlanRevision` was returned.
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

/// Trusted owner of Plan proposal validation and persistence.
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

    pub fn propose_decomposition(
        &mut self,
        planner: &impl PlannerPort,
        cycle: &mut PlanningCycle,
        branch: &PlanBranchId,
        mut request: DecompositionRequest,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        let (head, base_graph) = self.load_base(branch)?;
        let remaining_depth = remaining_depth(&base_graph);
        if remaining_depth == 0 || request.depth_remaining == 0 {
            return Ok(PlanningProposalOutcome::GuardrailTripped(
                GuardrailTrip::new("depth", u64::from(MAX_DECOMPOSITION_DEPTH)),
            ));
        }
        if let Some(trip) = cycle.pre_call_guardrail() {
            return Ok(PlanningProposalOutcome::GuardrailTripped(trip));
        }

        request.parent_task_ref = policy.parent_task_ref.clone();
        request.base_plan_revision = head;
        request.acceptance_criteria_refs = policy
            .affected_parent_criteria
            .iter()
            .map(|criterion| criterion.id.clone())
            .collect();
        request.scope_bound = policy.scope_bound.clone();
        request.depth_remaining = request.depth_remaining.min(remaining_depth);
        request.budget_remaining = cycle.remaining_budget();

        cycle.start_call();
        let proposal = match planner.propose_decomposition(request) {
            Ok(proposal) => proposal,
            Err(fault) => {
                if matches!(fault, PlannerFault::ProviderFailure { .. }) {
                    cycle.charge_unreceipted_provider_failure();
                }
                return planner_fault_outcome(fault);
            }
        };
        self.validate_persist_proposal(cycle, branch, head, base_graph, proposal, policy)
    }

    pub fn propose_replan(
        &mut self,
        planner: &impl PlannerPort,
        cycle: &mut PlanningCycle,
        branch: &PlanBranchId,
        mut request: ReplanRequest,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        validate_policy_context(policy)?;
        let (head, base_graph) = self.load_base(branch)?;
        let remaining_depth = remaining_depth(&base_graph);
        if remaining_depth == 0 || request.depth_remaining == 0 {
            return Ok(PlanningProposalOutcome::GuardrailTripped(
                GuardrailTrip::new("depth", u64::from(MAX_DECOMPOSITION_DEPTH)),
            ));
        }
        if let Some(trip) = cycle.pre_call_guardrail() {
            return Ok(PlanningProposalOutcome::GuardrailTripped(trip));
        }

        request.branch_root_ref = policy.parent_task_ref.clone();
        request.base_plan_revision = head;
        request.scope_bound = policy.scope_bound.clone();
        request.depth_remaining = request.depth_remaining.min(remaining_depth);
        request.budget_remaining = cycle.remaining_budget();

        cycle.start_call();
        let proposal = match planner.propose_replan(request) {
            Ok(proposal) => proposal,
            Err(fault) => {
                if matches!(fault, PlannerFault::ProviderFailure { .. }) {
                    cycle.charge_unreceipted_provider_failure();
                }
                return planner_fault_outcome(fault);
            }
        };
        self.validate_persist_proposal(cycle, branch, head, base_graph, proposal, policy)
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

    fn validate_persist_proposal(
        &mut self,
        cycle: &mut PlanningCycle,
        branch: &PlanBranchId,
        trusted_base_head: u64,
        base_graph: PlanGraph,
        proposal: PlanChangeSet,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError> {
        // Cost evidence is durable independently of proposal acceptance. The
        // content-derived ref is trusted computation over the parsed proposal;
        // a malformed/tampered proposal can be refused later without erasing
        // the spend already incurred to obtain it.
        let receipt_ref = PlanningCostReceiptRef::new(format!(
            "planning-receipt:{}",
            compute_proposal_hash(&proposal)
        ))?;
        self.store
            .record_plan_cost_receipt(&receipt_ref, &proposal.planning_cost_receipt)?;
        if let Some(trip) = cycle.observe_receipt(&proposal.planning_cost_receipt) {
            return Ok(PlanningProposalOutcome::GuardrailTripped(trip));
        }

        // The provider call is deliberately outside storage transactions.
        // Re-check the exact head that owns `base_graph` before interpreting
        // provider output. A fresh durable head paired with this older graph
        // would turn V8 into an unsafe implicit rebase.
        let durable_head = self.store.get_plan_head(branch)?.unwrap_or(0);
        if durable_head != trusted_base_head {
            return proposal_refusal(
                cycle,
                &proposal,
                ValidationRefusal::StaleBase {
                    expected: durable_head,
                    observed: trusted_base_head,
                },
            );
        }

        let depth_after_merge = merged_depth(&base_graph, &proposal);
        let mut lineage = policy.lineage_fingerprints.clone();
        lineage.extend(cycle.lineage_fingerprints.iter().cloned());
        lineage.sort();
        lineage.dedup();
        let affected_parent_criteria =
            affected_criteria_for_proposal(&base_graph, &proposal, policy)?;
        let validation = ValidationContext {
            // V8 validates against the exact snapshot supplied to the
            // planner. A later durable-head check below detects a concurrent
            // writer without pairing a fresh head with this stale graph.
            branch_head: trusted_base_head,
            scope_bound: policy.scope_bound.clone(),
            baseline_risk: policy.baseline_risk,
            affected_parent_criteria,
            depth_after_merge,
            revisions_accepted_this_cycle: cycle.revisions_accepted,
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
                if cycle.record_rejection(&proposal.source_evidence_refs) {
                    return Ok(PlanningProposalOutcome::GuardrailTripped(
                        GuardrailTrip::new("no-progress", 2),
                    ));
                }
                return Ok(PlanningProposalOutcome::Refused(refusal));
            }
            ValidationOutcome::Accepted { .. } => {}
        }

        // Catch a writer that won after the first head check but before graph
        // materialization. Storage repeats the same CAS inside BEGIN
        // IMMEDIATE, closing the final race before commit.
        let durable_head = self.store.get_plan_head(branch)?.unwrap_or(0);
        if durable_head != trusted_base_head {
            return proposal_refusal(
                cycle,
                &proposal,
                ValidationRefusal::StaleBase {
                    expected: durable_head,
                    observed: trusted_base_head,
                },
            );
        }

        let resulting_graph = apply_validated_changeset(&base_graph, &proposal)?;
        if !graph_has_leaf(&resulting_graph) {
            return Ok(PlanningProposalOutcome::GuardrailTripped(
                GuardrailTrip::new("runnable-leaf", 0),
            ));
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

        match self
            .store
            .commit_plan_revision(&proposal, &revision, &resulting_graph, &evidence)
        {
            Ok(_) => {}
            Err(StorageError::PlanStale { found, .. }) => {
                return proposal_refusal(
                    cycle,
                    &proposal,
                    ValidationRefusal::StaleBase {
                        expected: found,
                        observed: trusted_base_head,
                    },
                );
            }
            Err(error) => return Err(error.into()),
        }
        let fingerprint = decomposition_fingerprint(&policy.parent_task_ref, &proposal);
        cycle.record_acceptance(fingerprint);
        Ok(PlanningProposalOutcome::Accepted(Box::new(
            AcceptedPlanRevision {
                revision,
                graph: resulting_graph,
                planning_cost_receipt: proposal.planning_cost_receipt,
            },
        )))
    }
}

fn proposal_refusal(
    cycle: &mut PlanningCycle,
    proposal: &PlanChangeSet,
    refusal: ValidationRefusal,
) -> Result<PlanningProposalOutcome, PlanningServiceError> {
    if cycle.record_rejection(&proposal.source_evidence_refs) {
        return Ok(PlanningProposalOutcome::GuardrailTripped(
            GuardrailTrip::new("no-progress", 2),
        ));
    }
    Ok(PlanningProposalOutcome::Refused(refusal))
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
        PlannerFault::BudgetExhausted => Ok(PlanningProposalOutcome::GuardrailTripped(
            GuardrailTrip::new("planner-budget", 1),
        )),
        PlannerFault::Unplannable { blocker } => Ok(PlanningProposalOutcome::Blocked(blocker)),
        failure @ PlannerFault::ProviderFailure { .. } => {
            Err(PlanningServiceError::Planner(failure))
        }
    }
}

fn remaining_depth(graph: &PlanGraph) -> u32 {
    MAX_DECOMPOSITION_DEPTH.saturating_sub(graph_depth(graph))
}

fn graph_depth(graph: &PlanGraph) -> u32 {
    graph_depth_from_parts(
        graph.nodes.iter().map(|node| node.id.as_str()),
        graph
            .edges
            .iter()
            .map(|(from, to)| (from.as_str(), to.as_str())),
    )
}

fn merged_depth(graph: &PlanGraph, proposal: &PlanChangeSet) -> u32 {
    let removed_nodes: BTreeSet<&str> = proposal
        .node_removals
        .iter()
        .map(|node| node.as_str())
        .collect();
    let removed_edges: BTreeSet<(&str, &str)> = proposal
        .edge_removals
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    let mut nodes: BTreeSet<String> = graph
        .nodes
        .iter()
        .map(|node| node.id.as_str().to_owned())
        .chain(
            proposal
                .node_additions
                .iter()
                .map(|node| node.id.as_str().to_owned()),
        )
        .collect();
    for edge in &proposal.edge_additions {
        nodes.insert(edge.from.as_str().to_owned());
        nodes.insert(edge.to.as_str().to_owned());
    }
    nodes.retain(|node| !removed_nodes.contains(node.as_str()));
    let mut edges: Vec<(String, String)> = graph
        .edges
        .iter()
        .filter(|(from, to)| !removed_edges.contains(&(from.as_str(), to.as_str())))
        .map(|(from, to)| (from.as_str().to_owned(), to.as_str().to_owned()))
        .collect();
    edges.extend(
        proposal
            .edge_additions
            .iter()
            .map(|edge| (edge.from.as_str().to_owned(), edge.to.as_str().to_owned())),
    );
    graph_depth_from_parts(
        nodes.iter().map(String::as_str),
        edges.iter().map(|(from, to)| (from.as_str(), to.as_str())),
    )
}

fn graph_depth_from_parts<'a>(
    nodes: impl Iterator<Item = &'a str>,
    edges: impl Iterator<Item = (&'a str, &'a str)>,
) -> u32 {
    let nodes: BTreeSet<&str> = nodes.collect();
    if nodes.is_empty() {
        return 0;
    }
    let mut indegree: BTreeMap<&str, usize> = nodes.iter().map(|node| (*node, 0)).collect();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in edges {
        if nodes.contains(from) && nodes.contains(to) {
            outgoing.entry(from).or_default().push(to);
            *indegree.entry(to).or_default() += 1;
        }
    }
    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect();
    let mut depth: BTreeMap<&str, u32> = nodes.iter().map(|node| (*node, 0)).collect();
    let mut visited = 0_usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        let current = *depth.get(node).unwrap_or(&0);
        if let Some(next) = outgoing.get(node) {
            for target in next {
                let candidate = current.saturating_add(1);
                let entry = depth.entry(*target).or_insert(0);
                *entry = (*entry).max(candidate);
                if let Some(degree) = indegree.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(target);
                    }
                }
            }
        }
    }
    if visited != nodes.len() {
        return u32::MAX;
    }
    depth.values().copied().max().unwrap_or(0)
}

fn apply_validated_changeset(
    graph: &PlanGraph,
    proposal: &PlanChangeSet,
) -> Result<PlanGraph, PlanDomainError> {
    let removed: BTreeSet<&str> = proposal
        .node_removals
        .iter()
        .map(|node| node.as_str())
        .collect();
    let mut nodes: BTreeMap<String, PlanNode> = graph
        .nodes
        .iter()
        .filter(|node| !removed.contains(node.id.as_str()))
        .map(|node| (node.id.as_str().to_owned(), node.clone()))
        .collect();
    for update in &proposal.node_updates {
        let node = nodes.get_mut(update.target.as_str()).ok_or_else(|| {
            PlanDomainError::InvalidInput(format!(
                "validated changeset update target {} is not live after removals",
                update.target.as_str()
            ))
        })?;
        if let Some(scope) = &update.scope {
            node.scope = scope.clone();
        }
        if let Some(risk) = update.risk_class {
            node.risk_class = risk;
        }
        match &update.required_gate {
            Some(GateUpdate::Set(gate)) => node.required_gate = Some(gate.clone()),
            Some(GateUpdate::Clear) => node.required_gate = None,
            None => {}
        }
    }
    for addition in &proposal.node_additions {
        let node = PlanNode::new(
            addition.id.clone(),
            addition.scope.clone(),
            addition.criteria.clone(),
            addition.risk_class,
            addition.required_gate.clone(),
        )?;
        nodes.insert(addition.id.as_str().to_owned(), node);
    }

    let removed_edges: BTreeSet<(&str, &str)> = proposal
        .edge_removals
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    let mut edges = graph
        .edges
        .iter()
        .filter(|(from, to)| !removed_edges.contains(&(from.as_str(), to.as_str())))
        .cloned()
        .collect::<Vec<_>>();
    for addition in &proposal.edge_additions {
        if !edges
            .iter()
            .any(|(from, to)| from == &addition.from && to == &addition.to)
        {
            edges.push((addition.from.clone(), addition.to.clone()));
        }
    }
    edges.sort_by(|(left_from, left_to), (right_from, right_to)| {
        (left_from.as_str(), left_to.as_str()).cmp(&(right_from.as_str(), right_to.as_str()))
    });
    PlanGraph::new(nodes.into_values().collect(), edges)
}

fn graph_has_leaf(graph: &PlanGraph) -> bool {
    let node_ids: BTreeSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (from, to) in &graph.edges {
        edges.entry(from.as_str()).or_default().insert(to.as_str());
    }
    merged_graph_has_leaf(&node_ids, &edges)
}

/// Result of one bounded production planning+execution cycle.
#[derive(Debug)]
pub enum PlannedOrchestrationOutcome {
    /// Initial proposal was refused/blocked before scheduler execution.
    PlanningStopped(PlanningProposalOutcome),
    /// The selected plan node exists but one or more Plan predecessors have
    /// not reached canonical Task COMPLETED state yet. No Task claim,
    /// Execution, Attempt, or provider invocation has happened.
    WaitingOnPlanDependency {
        plan: Box<AcceptedPlanRevision>,
        blocking: Vec<TaskId>,
    },
    /// Accepted revision gated execution. A failed/rejected execution may
    /// have produced exactly one bounded replan attempt from fresh evidence.
    Executed {
        plan: Box<AcceptedPlanRevision>,
        execution: OrchestrationOutcome,
        replan: Option<Result<PlanningProposalOutcome, PlanningServiceError>>,
    },
}

/// Error before an `Executed` result can be returned.
#[derive(Debug)]
pub enum PlannedOrchestrationError {
    Planning(PlanningServiceError),
    Orchestration(ServiceError),
}

impl fmt::Display for PlannedOrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Planning(error) => write!(f, "planning cycle failed: {error}"),
            Self::Orchestration(error) => write!(f, "execution cycle failed: {error}"),
        }
    }
}

impl std::error::Error for PlannedOrchestrationError {}

impl<C: Clock> OrchestrationService<C> {
    /// Product-owned bounded `propose -> validate -> persist -> execute ->
    /// replan` path. Execution is delegated to the existing canonical
    /// scheduler service and therefore retains Task/Policy/Evaluation
    /// authority boundaries.
    #[allow(clippy::too_many_arguments)]
    pub fn schedule_with_planning(
        &mut self,
        planning: &mut PlanningService<C>,
        planner: &impl PlannerPort,
        cycle: &mut PlanningCycle,
        branch: &PlanBranchId,
        decomposition: DecompositionRequest,
        policy: PlanningPolicyContext,
        task_id: &TaskId,
        ids: OrchestrationIds,
        executor: &impl ExecutionPort,
        evaluator: &impl EvaluationPort,
    ) -> Result<PlannedOrchestrationOutcome, PlannedOrchestrationError> {
        let planned = planning
            .propose_decomposition(planner, cycle, branch, decomposition, &policy)
            .map_err(PlannedOrchestrationError::Planning)?;
        let accepted = match planned {
            PlanningProposalOutcome::Accepted(accepted) => accepted,
            stopped => return Ok(PlannedOrchestrationOutcome::PlanningStopped(stopped)),
        };
        let Some(predecessors) = accepted
            .predecessor_task_ids(task_id)
            .map_err(PlannedOrchestrationError::Planning)?
        else {
            return Err(PlannedOrchestrationError::Planning(
                PlanningServiceError::InvalidContext(format!(
                    "task {} is not represented by accepted plan {}:{}",
                    task_id.as_str(),
                    accepted.revision.plan_branch_id.as_str(),
                    accepted.revision.revision_seq
                )),
            ));
        };

        let mut blocking = Vec::new();
        for predecessor in predecessors {
            let task = self
                .tasks()
                .get_task(&predecessor)
                .map_err(PlannedOrchestrationError::Orchestration)?
                .ok_or_else(|| {
                    PlannedOrchestrationError::Planning(PlanningServiceError::InvalidContext(
                        format!(
                            "plan predecessor {} has no canonical Task",
                            predecessor.as_str()
                        ),
                    ))
                })?;
            if task.state() != TaskState::Completed {
                blocking.push(predecessor);
            }
        }
        if !blocking.is_empty() {
            return Ok(PlannedOrchestrationOutcome::WaitingOnPlanDependency {
                plan: accepted,
                blocking,
            });
        }

        let execution = self
            .schedule(task_id, ids, executor, evaluator)
            .map_err(PlannedOrchestrationError::Orchestration)?;
        let trigger = replan_evidence(&execution);
        let replan = trigger.map(|evidence| {
            cycle.begin_new_evidence_event();
            let mut replan_policy = policy.clone();
            // Replanning after execution evidence changes the affected branch
            // without claiming to replace parent acceptance criteria. Existing
            // criteria stay on the durable base nodes; V3 applies only when a
            // proposal explicitly decomposes/replaces affected criteria.
            replan_policy.affected_parent_criteria.clear();
            planning.propose_replan(
                planner,
                cycle,
                branch,
                ReplanRequest {
                    branch_root_ref: policy.parent_task_ref.clone(),
                    base_plan_revision: accepted.revision.revision_seq,
                    triggering_evidence_refs: vec![evidence],
                    scope_bound: policy.scope_bound.clone(),
                    depth_remaining: MAX_DECOMPOSITION_DEPTH,
                    budget_remaining: PlanningBudget {
                        calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
                        input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
                    },
                },
                &replan_policy,
            )
        });
        Ok(PlannedOrchestrationOutcome::Executed {
            plan: accepted,
            execution,
            replan,
        })
    }
}

fn replan_evidence(outcome: &OrchestrationOutcome) -> Option<EvidenceRef> {
    let attempt = match outcome {
        OrchestrationOutcome::ExecutionFailed { attempt_id, .. }
        | OrchestrationOutcome::ExecutionSucceededButRejected { attempt_id, .. } => attempt_id,
        OrchestrationOutcome::NotRunnable { .. }
        | OrchestrationOutcome::WaitingOnDependency { .. }
        | OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        | OrchestrationOutcome::Cancelled { .. } => return None,
    };
    EvidenceRef::new(format!("attempt:{}", attempt.as_str())).ok()
}
