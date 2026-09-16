//! TASK-0078 regression coverage for the trusted proposal -> guardrails ->
//! validation -> applicability -> persistence boundary.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use app_services::{
    FixedClock, HumanDecisionCreate, HumanDecisionService, OrchestrationIds, OrchestrationService,
    PlanApplicabilityRefusal, PlannedExecutionEntryOutcome, PlannedOrchestrationOutcome,
    PlanningAssessmentOutcome, PlanningPolicyContext, PlanningProposalOutcome, PlanningService,
    PlanningServiceError, RuntimeService, TaskService,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use fake_planner::{FakePlanner, FakePlannerScenario};
use plan_domain::{
    compute_proposal_hash, planning_evidence_key, AffectedParentCriterion, AtomicityAssessment,
    AtomicityRequest, AtomicityVerdict, BudgetDelta, CriterionDecl, CriterionId,
    DecompositionRequest, EdgeAddition, EvidenceRef, GateRef, HumanContinuation,
    HumanDecisionReason, HumanDecisionRequest, HumanDecisionRequestId, HumanDecisionStatus,
    NodeAddition, PlanBranchId, PlanChangeSet, PlanNodeId, PlanScope, PlannerAdapterId,
    PlannerCallSpend, PlannerFault, PlannerIdentity, PlannerPort, PlanningBudget,
    PlanningCostReceipt, PlanningInputSnapshot, ReplanRequest, RiskClass, RiskDelta, ScopeBound,
    ValidationRefusal, VerificationMethod, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS, MAX_PLANNER_INPUT_TOKENS,
};
use scheduler::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};
use storage_sqlite::{
    EventType, ExecutionEntryAdmission, PlanHeadPrecondition, PlanningInputPrecondition,
    SqliteStore, StorageError,
};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

const FIXED_MILLIS: u64 = 1_704_067_200_000;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-planning-service-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
        }
    }

    /// A fresh service handle over the same durable database: the restart
    /// boundary for every durable-guardrail regression below.
    fn service(&self) -> PlanningService<FixedClock> {
        PlanningService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open planning service")
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("open store")
    }

    fn orchestration(&self) -> OrchestrationService<FixedClock> {
        OrchestrationService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("orchestration")
    }

    /// Creates canonical Tasks named like plan nodes. `ready` Tasks are
    /// RUNNABLE; others stay DRAFT.
    fn seed_tasks(&self, names: &[&str], ready: bool) {
        let mut tasks = TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("tasks");
        seed_into(&mut tasks, names, ready);
    }
}

fn seed_into(tasks: &mut TaskService<FixedClock>, names: &[&str], ready: bool) {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let project = ProjectId::new(format!("P-PLANNING-{id}")).expect("project id");
    tasks
        .create_project(&Project::new(project.clone(), "planning fixture".to_owned()).unwrap())
        .expect("create project");
    for name in names {
        let task = Task::draft(
            TaskId::new(*name).expect("task id"),
            project.clone(),
            "planned work".to_owned(),
        )
        .expect("draft task");
        tasks.create_task(&task).expect("create task");
        if ready {
            tasks.mark_ready(task.id()).expect("mark ready");
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Clone, Copy, Debug)]
enum ProposalCase {
    Valid,
    RiskEscalation,
    Cyclic,
    InvalidHash,
    MissingCriteria,
    InvalidScope,
    PolicyWeakening,
    OversizedTokenReceipt,
    UpdateRemovedNode,
    DropRemovedCriteria,
    StaleBase(u64),
}

struct TestPlanner {
    case: ProposalCase,
    calls: AtomicUsize,
}

impl TestPlanner {
    fn new(case: ProposalCase) -> Self {
        Self {
            case,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn proposal(&self, input: &DecompositionRequest) -> PlanChangeSet {
        let scope_root = input
            .scope_bound
            .roots()
            .into_iter()
            .next()
            .expect("test scope");
        let child_id = PlanNodeId::new(format!("node-{}", input.base_plan_revision + 1))
            .expect("derived node id");
        let child_criterion =
            CriterionId::new(format!("criterion-child-{}", input.base_plan_revision + 1))
                .expect("derived criterion");
        let criteria = vec![CriterionDecl::new(
            child_criterion.clone(),
            VerificationMethod::AutomatedTest,
        )];
        let coverage_map = input
            .acceptance_criteria_refs
            .first()
            .cloned()
            .map(|parent| BTreeMap::from([(parent, vec![child_criterion.clone()])]))
            .unwrap_or_default();

        let mut proposal = PlanChangeSet {
            base_plan_revision: match self.case {
                ProposalCase::StaleBase(base) => base,
                _ => input.base_plan_revision,
            },
            reason: "deterministic planning-service proposal".to_owned(),
            source_evidence_refs: input.evidence_refs.clone(),
            node_additions: vec![NodeAddition {
                id: child_id.clone(),
                scope: PlanScope::new(vec![scope_root.clone()]).expect("test node scope"),
                criteria,
                risk_class: RiskClass::Low,
                required_gate: None,
            }],
            node_updates: vec![],
            node_removals: vec![],
            edge_additions: vec![],
            edge_removals: vec![],
            expected_outcome: "one executable leaf is available".to_owned(),
            risk_delta: vec![],
            budget_delta: BudgetDelta::new(0, 0),
            scope_delta: PlanScope::new(vec![scope_root]).expect("test scope delta"),
            coverage_map,
            proposal_hash: String::new(),
            proposed_by: identity(),
            planning_cost_receipt: small_receipt(),
        };

        match self.case {
            ProposalCase::RiskEscalation => {
                proposal.node_additions[0].risk_class = RiskClass::High;
                proposal.node_additions[0].required_gate =
                    Some(GateRef::new("human-review").expect("gate"));
                proposal.risk_delta = vec![RiskDelta {
                    node: child_id.clone(),
                    from: RiskClass::Low,
                    to: RiskClass::High,
                }];
            }
            ProposalCase::Cyclic => {
                let second =
                    PlanNodeId::new(format!("node-cycle-{}", input.base_plan_revision + 1))
                        .expect("cycle node id");
                proposal.node_additions.push(NodeAddition {
                    id: second.clone(),
                    scope: proposal.node_additions[0].scope.clone(),
                    criteria: vec![],
                    risk_class: RiskClass::Low,
                    required_gate: None,
                });
                proposal.edge_additions = vec![
                    EdgeAddition {
                        from: child_id.clone(),
                        to: second.clone(),
                    },
                    EdgeAddition {
                        from: second,
                        to: child_id,
                    },
                ];
            }
            ProposalCase::MissingCriteria => {
                proposal.node_additions[0].criteria.clear();
            }
            ProposalCase::InvalidScope => {
                let escaped =
                    PlanScope::new(vec!["repo:outside".to_owned()]).expect("escaped scope");
                proposal.node_additions[0].scope = escaped.clone();
                proposal.scope_delta = escaped;
            }
            ProposalCase::PolicyWeakening => {
                proposal.node_additions[0].risk_class = RiskClass::High;
            }
            ProposalCase::OversizedTokenReceipt => {
                proposal.planning_cost_receipt =
                    PlanningCostReceipt::new(MAX_PLANNER_INPUT_TOKENS + 1, 1, 1, "test".to_owned())
                        .expect("oversized receipt remains representable evidence");
            }
            ProposalCase::UpdateRemovedNode => {
                let target = PlanNodeId::new(format!("node-{}", input.base_plan_revision))
                    .expect("derived existing node id");
                proposal.node_updates.push(plan_domain::NodeUpdate {
                    target: target.clone(),
                    scope: None,
                    risk_class: None,
                    required_gate: None,
                });
                proposal.node_removals.push(target);
            }
            ProposalCase::DropRemovedCriteria => {
                let target = PlanNodeId::new(format!("node-{}", input.base_plan_revision))
                    .expect("derived existing node id");
                proposal.node_removals.push(target);
                proposal.coverage_map.clear();
            }
            ProposalCase::Valid | ProposalCase::InvalidHash | ProposalCase::StaleBase(_) => {}
        }

        proposal.proposal_hash = compute_proposal_hash(&proposal);
        if matches!(self.case, ProposalCase::InvalidHash) {
            proposal.proposal_hash = "0".repeat(64);
        }
        proposal
    }
}

fn identity() -> PlannerIdentity {
    PlannerIdentity::new(
        PlannerAdapterId::new("planning-service-test").expect("adapter id"),
        "deterministic-test-model".to_owned(),
    )
    .expect("planner identity")
}

fn small_receipt() -> PlanningCostReceipt {
    PlanningCostReceipt::new(100, 50, 5, "test".to_owned()).expect("receipt")
}

fn replan_as_decomposition(input: ReplanRequest) -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: input.branch_root_ref,
        base_plan_revision: input.base_plan_revision,
        acceptance_criteria_refs: vec![],
        scope_bound: input.scope_bound,
        evidence_refs: input.triggering_evidence_refs,
        depth_remaining: input.depth_remaining,
        budget_remaining: input.budget_remaining,
    }
}

impl PlannerPort for TestPlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        AtomicityAssessment::new(
            AtomicityVerdict::Decomposable,
            "deterministic test assessment".to_owned(),
            input.evidence_refs,
            small_receipt(),
        )
        .map_err(|error| PlannerFault::ProviderFailure {
            typed_cause: error.to_string(),
            spend: PlannerCallSpend::Measured(small_receipt()),
        })
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.proposal(&input))
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.propose_decomposition(replan_as_decomposition(input))
    }
}

struct ConcurrentAdvancePlanner {
    path: PathBuf,
}

impl PlannerPort for ConcurrentAdvancePlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        TestPlanner::new(ProposalCase::Valid).assess_atomicity(input)
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let mut writer =
            PlanningService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("concurrent writer");
        let committed = writer
            .propose_decomposition(
                &TestPlanner::new(ProposalCase::Valid),
                &branch(),
                request("concurrent-writer"),
                &policy(false),
            )
            .expect("concurrent revision");
        assert!(matches!(committed, PlanningProposalOutcome::Accepted(_)));

        let mut forged = input;
        forged.base_plan_revision = forged.base_plan_revision.saturating_add(1);
        Ok(TestPlanner::new(ProposalCase::Valid).proposal(&forged))
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.propose_decomposition(replan_as_decomposition(input))
    }
}

/// Planner that always fails with one fixed fault and records the budget
/// headroom the trusted service handed it.
struct FaultPlanner {
    fault: PlannerFault,
    calls: AtomicUsize,
    budgets: Mutex<Vec<PlanningBudget>>,
}

impl FaultPlanner {
    fn new(fault: PlannerFault) -> Self {
        Self {
            fault,
            calls: AtomicUsize::new(0),
            budgets: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn budgets(&self) -> Vec<PlanningBudget> {
        self.budgets.lock().expect("budgets").clone()
    }
}

impl PlannerPort for FaultPlanner {
    fn assess_atomicity(&self, _: AtomicityRequest) -> Result<AtomicityAssessment, PlannerFault> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(self.fault.clone())
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.budgets
            .lock()
            .expect("budgets")
            .push(input.budget_remaining);
        Err(self.fault.clone())
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.propose_decomposition(replan_as_decomposition(input))
    }
}

/// Re-proposes the same decomposition content (parent, child scope,
/// criterion, evidence) on every call; only the node id follows the head.
struct ReplayPlanner;

impl PlannerPort for ReplayPlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        TestPlanner::new(ProposalCase::Valid).assess_atomicity(input)
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let mut forged = input;
        forged.acceptance_criteria_refs.clear();
        let mut proposal = TestPlanner::new(ProposalCase::Valid).proposal(&forged);
        proposal.node_additions[0].id =
            PlanNodeId::new(format!("replay-node-{}", forged.base_plan_revision + 1)).unwrap();
        proposal.node_additions[0].criteria = vec![CriterionDecl::new(
            CriterionId::new("replay-criterion").unwrap(),
            VerificationMethod::AutomatedTest,
        )];
        proposal.source_evidence_refs = vec![evidence("replayed-evidence")];
        proposal.proposal_hash = compute_proposal_hash(&proposal);
        Ok(proposal)
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.propose_decomposition(replan_as_decomposition(input))
    }
}

struct CountingExecutor {
    calls: AtomicUsize,
}

impl CountingExecutor {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ExecutionPort for CountingExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("planning-service-fixture").expect("provider ref")
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(WorkEvidence {
            summary: format!("fixture executed {}", work.task_id.as_str()),
            evidence_ref: format!("fixture:{}", work.attempt_id.as_str()),
            partial: false,
        })
    }
}

fn ids(suffix: &str) -> OrchestrationIds {
    OrchestrationIds {
        execution_id: ExecutionId::new(format!("E-{suffix}")).expect("execution id"),
        attempt_id: AttemptId::new(format!("A-{suffix}")).expect("attempt id"),
    }
}

fn branch() -> PlanBranchId {
    PlanBranchId::new("task-0078-branch").expect("branch id")
}

fn evidence(value: &str) -> EvidenceRef {
    EvidenceRef::new(value).expect("evidence")
}

fn policy(with_criterion: bool) -> PlanningPolicyContext {
    PlanningPolicyContext {
        scope_bound: ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope bound"),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: if with_criterion {
            vec![AffectedParentCriterion {
                id: CriterionId::new("criterion-root").expect("criterion id"),
                method: VerificationMethod::AutomatedTest,
            }]
        } else {
            vec![]
        },
        parent_task_ref: "TASK-0078".to_owned(),
        lineage_fingerprints: vec![],
        risk_grant: None,
        risk_decision_id: None,
        policy_fingerprint: "policy:test".to_owned(),
    }
}

fn request_with(evidence_refs: Vec<EvidenceRef>) -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: "untrusted-caller-value".to_owned(),
        base_plan_revision: u64::MAX,
        acceptance_criteria_refs: vec![],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).expect("caller scope"),
        evidence_refs,
        depth_remaining: 4,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    }
}

fn request(evidence_ref: &str) -> DecompositionRequest {
    request_with(vec![evidence(evidence_ref)])
}

fn assessment_request(evidence_ref: &str) -> AtomicityRequest {
    AtomicityRequest {
        task_ref: "untrusted-caller-value".to_owned(),
        acceptance_criteria_refs: vec![],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).expect("caller scope"),
        evidence_refs: vec![evidence(evidence_ref)],
        execution_policy_ref: "policy:test".to_owned(),
    }
}

/// No revision, lineage, or plan journal fact exists for the test branch.
fn assert_no_plan_persisted(db: &TestDb) {
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).expect("read head"), None);
    assert!(store.get_plan_lineage(&branch()).unwrap().is_empty());
    assert!(!store
        .list_events_ordered()
        .unwrap()
        .iter()
        .any(|event| event.fact.event_type == EventType::PlanRevisionCommitted));
}

fn seed_head(db: &TestDb, target: u64) {
    let names: Vec<String> = (1..=target + 1).map(|n| format!("node-{n}")).collect();
    db.seed_tasks(&names.iter().map(String::as_str).collect::<Vec<_>>(), true);
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::Valid);
    for revision in 1..=target {
        let outcome = service
            .propose_decomposition(
                &planner,
                &branch(),
                request(&format!("seed-evidence-{revision}")),
                &policy(false),
            )
            .expect("seed planning call");
        assert!(
            matches!(outcome, PlanningProposalOutcome::Accepted(_)),
            "seed revision {revision}: {outcome:?}"
        );
    }
}

fn create_human_decision(
    db: &TestDb,
    task_id: &str,
    options: Vec<HumanContinuation>,
) -> HumanDecisionRequest {
    let accepted = db
        .service()
        .current_accepted_plan(&branch())
        .expect("current accepted plan")
        .expect("accepted plan required");
    let mut service = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    service
        .create(HumanDecisionCreate {
            task_id: TaskId::new(task_id).expect("task id"),
            plan_branch_id: branch(),
            reason: HumanDecisionReason::PlannerBlocked,
            risk: RiskClass::Low,
            affected_scope: ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope"),
            options,
            recommendation: None,
            evidence: vec![evidence("human-decision-fixture")],
            proposal_hash: None,
            plan_head: accepted.revision().revision_seq,
            planning_input_snapshot_id: accepted.planning_input_snapshot_id().clone(),
            planning_input_fingerprint: accepted.planning_input_fingerprint().to_owned(),
            policy_fingerprint: "policy:test".to_owned(),
            expires_at_ms: None,
        })
        .expect("create human decision")
}

fn replace_current_planning_input(
    db: &TestDb,
    scope_roots: Vec<&str>,
    risk: RiskClass,
    policy_fingerprint: &str,
    evidence_ref: &str,
) {
    let accepted = db
        .service()
        .current_accepted_plan(&branch())
        .expect("accepted plan query")
        .expect("accepted plan");
    let trigger = vec![evidence(evidence_ref)];
    let snapshot = PlanningInputSnapshot::new(
        branch(),
        "TASK-0078".to_owned(),
        accepted.revision().revision_seq,
        ScopeBound::new(scope_roots.into_iter().map(str::to_owned).collect()).expect("scope"),
        risk,
        vec![],
        trigger.clone(),
        vec![],
        None,
        policy_fingerprint.to_owned(),
    )
    .expect("replacement snapshot");
    db.store()
        .start_planning_run(
            &snapshot,
            &planning_evidence_key(&trigger),
            FIXED_MILLIS + 1,
        )
        .expect("replace planning input");
}

fn measured_fault(input_tokens: u32) -> PlannerFault {
    PlannerFault::ProviderFailure {
        typed_cause: "fixture failure after measured spend".to_owned(),
        spend: PlannerCallSpend::Measured(
            PlanningCostReceipt::new(input_tokens, 10, 5, "test".to_owned()).unwrap(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Proposal validation and persistence (round 1 invariants preserved)
// ---------------------------------------------------------------------------

#[test]
fn valid_proposal_validates_and_persists_one_revision() {
    let db = TestDb::new("valid");
    db.seed_tasks(&["planned-child-0"], true);
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);

    let outcome = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("evidence-valid"),
            &policy(true),
        )
        .expect("valid proposal");
    let accepted = match outcome {
        PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("expected accepted proposal, got {other:?}"),
    };

    assert_eq!(accepted.revision().revision_seq, 1);
    assert_eq!(accepted.revision().base_revision, 0);
    let state = service
        .guardrail_state(&branch(), &[evidence("evidence-valid")])
        .unwrap();
    assert_eq!(
        (
            state.planner_calls,
            state.charged_input_tokens,
            state.revisions_accepted
        ),
        (1, 100, 1)
    );
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).expect("head"), Some(1));
    assert_eq!(
        store.get_plan_graph(&branch(), 1).expect("graph"),
        Some(accepted.graph().clone())
    );
    let receipt = store
        .get_plan_cost_receipt(&accepted.revision().proposed_by.planning_cost_receipt_ref)
        .expect("receipt read");
    assert_eq!(receipt, Some(accepted.planning_cost_receipt().clone()));
    assert_eq!(
        service
            .planner_call_receipt(&branch(), &[evidence("evidence-valid")], 1)
            .unwrap(),
        receipt,
        "the accepted revision references its product-owned call receipt"
    );
    assert_eq!(
        service.current_accepted_plan(&branch()).unwrap().as_ref(),
        Some(&*accepted),
        "the durable head reconstructs exactly the accepted capability"
    );
}

#[test]
fn stale_base_uses_canonical_stored_head_and_never_persists() {
    let db = TestDb::new("stale-base");
    seed_head(&db, 5);
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::StaleBase(4));

    let outcome = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("stale-evidence"),
            &policy(false),
        )
        .expect("stale proposal is a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::StaleBase { expected, observed }) => {
            assert_eq!(expected, 5);
            assert_eq!(observed, 4);
        }
        other => panic!("expected STALE_BASE, got {other:?}"),
    }
    assert_eq!(service.store().get_plan_head(&branch()).unwrap(), Some(5));
    assert!(service
        .store()
        .get_plan_revision(&branch(), 6)
        .unwrap()
        .is_none());
}

#[test]
fn concurrent_head_advance_refuses_stale_graph_even_if_proposal_claims_new_head() {
    let db = TestDb::new("concurrent-head");
    seed_head(&db, 1);
    let mut service = db.service();
    let planner = ConcurrentAdvancePlanner {
        path: db.path.clone(),
    };

    let error = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("outer-proposal"),
            &policy(false),
        )
        .expect_err("newer planning generation fences the stale outer worker");
    assert!(matches!(
        error,
        PlanningServiceError::Storage(StorageError::PlanningRunStale {
            expected_generation: 2,
            found_generation: 3,
            ..
        })
    ));
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).unwrap(), Some(2));
    assert!(store.get_plan_revision(&branch(), 3).unwrap().is_none());
    let graph = store
        .get_plan_graph(&branch(), 2)
        .unwrap()
        .expect("concurrent writer graph remains canonical");
    assert!(graph.nodes.iter().any(|node| node.id.as_str() == "node-2"));
    assert!(!graph.nodes.iter().any(|node| node.id.as_str() == "node-3"));
}

#[test]
fn malformed_and_invalid_proposals_refuse_before_persistence() {
    let cases = [
        (ProposalCase::Cyclic, "CYCLIC_PROPOSAL"),
        (ProposalCase::InvalidHash, "MALFORMED_PROPOSAL"),
        (ProposalCase::MissingCriteria, "CRITERION_DROPPED"),
        (ProposalCase::InvalidScope, "SCOPE_ESCAPE"),
    ];

    for (case, expected_code) in cases {
        let db = TestDb::new(expected_code);
        db.seed_tasks(&["node-1"], true);
        let mut service = db.service();
        let planner = TestPlanner::new(case);
        let outcome = service
            .propose_decomposition(&planner, &branch(), request(expected_code), &policy(true))
            .expect("invalid proposal must be a typed refusal");
        match outcome {
            PlanningProposalOutcome::Refused(refusal) => assert_eq!(refusal.code(), expected_code),
            other => panic!("expected {expected_code}, got {other:?}"),
        }
        assert_no_plan_persisted(&db);
    }
}

#[test]
fn update_and_removal_of_same_node_is_typed_refusal_never_panic_or_persistence() {
    let db = TestDb::new("update-removed-node");
    seed_head(&db, 1);
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::UpdateRemovedNode);

    let outcome = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("update-removed-node"),
            &policy(false),
        )
        .expect("update/removal overlap must be a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::DanglingRef { detail }) => {
            assert!(detail.contains("removed by the same changeset"));
        }
        other => panic!("expected DANGLING_REF, got {other:?}"),
    }
    assert_eq!(service.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert!(service
        .store()
        .get_plan_revision(&branch(), 2)
        .unwrap()
        .is_none());
}

#[test]
fn removing_criterion_bearing_node_requires_derived_v3_coverage() {
    let db = TestDb::new("removed-criterion");
    seed_head(&db, 1);
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::DropRemovedCriteria);

    let outcome = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("drop-existing-criterion"),
            &policy(false),
        )
        .expect("criterion loss must be a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::CriterionDropped { detail }) => {
            assert!(detail.contains("criterion-child-1"));
        }
        other => panic!("expected CRITERION_DROPPED, got {other:?}"),
    }
    assert_eq!(service.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert!(service
        .store()
        .get_plan_revision(&branch(), 2)
        .unwrap()
        .is_none());
}

#[test]
fn token_budget_exhaustion_refuses_and_never_persists() {
    let db = TestDb::new("token-budget");
    db.seed_tasks(&["node-1"], true);
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::OversizedTokenReceipt);

    let outcome = service
        .propose_decomposition(&planner, &branch(), request("token-budget"), &policy(false))
        .expect("token guardrail");
    match outcome {
        PlanningProposalOutcome::GuardrailTripped(trip) => {
            assert_eq!(trip.bound, "planner-input-tokens-per-call");
        }
        other => panic!("expected token budget guardrail, got {other:?}"),
    }
    assert_eq!(planner.call_count(), 1);
    assert_eq!(
        service
            .planner_call_receipt(&branch(), &[evidence("token-budget")], 1)
            .unwrap()
            .map(|receipt| receipt.input_tokens),
        Some(MAX_PLANNER_INPUT_TOKENS + 1),
        "the over-envelope call's receipt is durable evidence, not discarded"
    );
    assert_no_plan_persisted(&db);
}

#[test]
fn persistence_failure_leaves_no_partial_plan_state() {
    let db = TestDb::new("persistence-failure");
    db.seed_tasks(&["node-1"], true);
    let impossible_sqlite_millis = i64::MAX as u64 + 1;
    let mut service =
        PlanningService::open(&db.path, FixedClock(impossible_sqlite_millis)).expect("open");
    let planner = TestPlanner::new(ProposalCase::Valid);

    let error = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("persistence-failure"),
            &policy(false),
        )
        .expect_err("commit must fail after validation");
    assert!(matches!(error, PlanningServiceError::Storage(_)));
    drop(service);
    assert_eq!(
        db.service()
            .guardrail_state(&branch(), &[evidence("persistence-failure")])
            .unwrap()
            .revisions_accepted,
        0
    );
    assert_no_plan_persisted(&db);
}

#[test]
fn policy_weakening_is_rejected_before_persistence() {
    let db = TestDb::new("policy-rejection");
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::PolicyWeakening);

    let outcome = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("policy-rejection"),
            &policy(false),
        )
        .expect("policy weakening must be a typed refusal");
    assert!(matches!(
        outcome,
        PlanningProposalOutcome::Refused(ValidationRefusal::PolicyWeakening { .. })
    ));
    assert_no_plan_persisted(&db);
}

// ---------------------------------------------------------------------------
// R2-01: every provider-started planner call leaves accounting evidence
// ---------------------------------------------------------------------------

#[test]
fn provider_failure_consumes_conservative_budget_without_fabricating_usage_evidence() {
    let db = TestDb::new("provider-failure-budget");
    db.seed_tasks(&["TASK-0078"], true);
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::ProviderFailure);
    let trigger = [evidence("provider-failure")];

    for expected_calls in 1..=MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        let error = service
            .propose_decomposition(
                &planner,
                &branch(),
                request("provider-failure"),
                &policy(false),
            )
            .expect_err("provider failure remains explicit");
        assert!(matches!(error, PlanningServiceError::Planner(_)));
        let state = service.guardrail_state(&branch(), &trigger).unwrap();
        assert_eq!(state.planner_calls, expected_calls);
        assert_eq!(
            state.charged_input_tokens,
            u64::from(expected_calls) * u64::from(MAX_PLANNER_INPUT_TOKENS)
        );
        assert_eq!(
            service
                .planner_call_receipt(&branch(), &trigger, expected_calls)
                .unwrap(),
            None,
            "unmeasured spend is charged, never recorded as a fabricated receipt"
        );
    }

    let stopped = service
        .propose_decomposition(
            &planner,
            &branch(),
            request("provider-failure"),
            &policy(false),
        )
        .expect("ninth attempt is stopped before provider call");
    assert!(matches!(
        stopped,
        PlanningProposalOutcome::WaitingForHuman(ref decision)
            if decision.reason == HumanDecisionReason::PlannerBudget
                && decision.options == [HumanContinuation::Replan, HumanContinuation::Abort]
    ));
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );
    assert_eq!(
        service
            .guardrail_state(&branch(), &trigger)
            .unwrap()
            .charged_input_tokens,
        u64::from(MAX_PLANNER_CUMULATIVE_INPUT_TOKENS)
    );
    assert_no_plan_persisted(&db);
}

#[test]
fn pre_provider_refusal_counts_the_call_but_charges_no_tokens() {
    let db = TestDb::new("not-started");
    let mut service = db.service();
    let planner = FaultPlanner::new(PlannerFault::ProviderFailure {
        typed_cause: "launch refused before spawn".to_owned(),
        spend: PlannerCallSpend::NotStarted,
    });
    let trigger = [evidence("not-started")];

    assert!(service
        .propose_decomposition(&planner, &branch(), request("not-started"), &policy(false))
        .is_err());
    let state = service.guardrail_state(&branch(), &trigger).unwrap();
    assert_eq!((state.planner_calls, state.charged_input_tokens), (1, 0));
    assert_eq!(
        service
            .planner_call_receipt(&branch(), &trigger, 1)
            .unwrap(),
        None
    );
}

#[test]
fn atomicity_assessment_shares_the_bounded_recorded_call_budget() {
    let db = TestDb::new("assessment-budget");
    db.seed_tasks(&["TASK-0078"], true);
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);
    let trigger = [evidence("assessment")];

    for call in 1..=MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        let outcome = service
            .assess_atomicity(
                &planner,
                &branch(),
                assessment_request("assessment"),
                &policy(false),
            )
            .expect("assessment");
        assert!(matches!(outcome, PlanningAssessmentOutcome::Assessed(_)));
        assert_eq!(
            service
                .planner_call_receipt(&branch(), &trigger, call)
                .unwrap(),
            Some(PlanningCostReceipt::new(100, 50, 5, "test".to_owned()).unwrap())
        );
    }
    let stopped = service
        .assess_atomicity(
            &planner,
            &branch(),
            assessment_request("assessment"),
            &policy(false),
        )
        .expect("bounded assessment");
    let decision_id = match stopped {
        PlanningAssessmentOutcome::WaitingForHuman(decision) => {
            assert_eq!(decision.reason, HumanDecisionReason::PlannerBudget);
            decision.id
        }
        other => panic!("expected planner-budget human decision, got {other:?}"),
    };
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );
    let stopped_proposal = service
        .propose_decomposition(&planner, &branch(), request("assessment"), &policy(false))
        .expect("proposal shares the event budget");
    assert!(matches!(
        stopped_proposal,
        PlanningProposalOutcome::WaitingForHuman(decision) if decision.id == decision_id
    ));
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );
}

#[cfg(unix)]
mod claude_fixture {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    use provider_claude::{ClaudeConfig, ClaudePlanner};

    pub(super) struct Fixture {
        pub(super) planner: ClaudePlanner,
        spawns: PathBuf,
    }

    impl Fixture {
        pub(super) fn spawn_count(&self) -> usize {
            std::fs::read_to_string(&self.spawns)
                .map(|text| text.len())
                .unwrap_or(0)
        }
    }

    /// Real `ClaudePlanner` over a deterministic provider executable that
    /// records each spawn and prints `document` as its final result.
    pub(super) fn fixture(db: &TestDb, name: &str, document: &str, exit_code: i32) -> Fixture {
        let spawns = db.dir.join(format!("{name}.spawns"));
        let script = db.dir.join(format!("{name}.sh"));
        let mut file = std::fs::File::create(&script).expect("fixture script");
        write!(
            file,
            "#!/bin/sh\nprintf x >> '{}'\nprintf '%s' '{}'\nexit {}\n",
            spawns.display(),
            document,
            exit_code
        )
        .expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        let workdir = db.dir.join(format!("{name}-work"));
        std::fs::create_dir_all(&workdir).expect("workdir");
        let config = ClaudeConfig::default()
            .with_executable(&script)
            .expect("fixture executable")
            .with_timeout(Duration::from_secs(10))
            .expect("timeout");
        let planner = ClaudePlanner::bind_trusted_host(config, workdir, db.dir.join("env"))
            .expect("claude planner");
        Fixture { planner, spawns }
    }

    pub(super) fn result_document(result: &str, usage: &str) -> String {
        format!(
            r#"{{"type":"result","subtype":"success","is_error":false,"result":"{result}","session_id":"planning-fixture","usage":{usage}}}"#
        )
    }
}

#[cfg(unix)]
#[test]
fn post_call_over_envelope_usage_is_charged_and_durably_receipted() {
    let db = TestDb::new("claude-over-envelope");
    let fixture = claude_fixture::fixture(
        &db,
        "over-envelope",
        &claude_fixture::result_document(
            "x",
            r#"{"input_tokens":5,"cache_creation_input_tokens":40000,"cache_read_input_tokens":0,"output_tokens":10}"#,
        ),
        0,
    );
    let mut service = db.service();
    let trigger = [evidence("over-envelope")];

    let outcome = service
        .propose_decomposition(
            &fixture.planner,
            &branch(),
            request("over-envelope"),
            &policy(false),
        )
        .expect("post-call excess is a typed guardrail outcome");
    match outcome {
        PlanningProposalOutcome::GuardrailTripped(trip) => {
            assert_eq!(trip.bound, "planner-input-tokens-per-call");
            assert_eq!(trip.observed, 40_005);
        }
        other => panic!("expected per-call guardrail, got {other:?}"),
    }
    assert_eq!(fixture.spawn_count(), 1);
    drop(service);

    let service = db.service();
    let state = service.guardrail_state(&branch(), &trigger).unwrap();
    assert_eq!(
        (state.planner_calls, state.charged_input_tokens),
        (1, 40_005)
    );
    let receipt = service
        .planner_call_receipt(&branch(), &trigger, 1)
        .unwrap()
        .expect("measured receipt survives the BudgetExhausted fault");
    assert_eq!((receipt.input_tokens, receipt.output_tokens), (40_005, 10));
    assert_no_plan_persisted(&db);

    let mut service = service;
    let assessment = service
        .assess_atomicity(
            &fixture.planner,
            &branch(),
            assessment_request("over-envelope-assessment"),
            &policy(false),
        )
        .expect("assessment excess is typed");
    assert!(matches!(
        assessment,
        PlanningAssessmentOutcome::GuardrailTripped(trip)
            if trip.bound == "planner-input-tokens-per-call"
    ));
    assert_eq!(
        service
            .guardrail_state(&branch(), &[evidence("over-envelope-assessment")])
            .unwrap()
            .charged_input_tokens,
        40_005
    );
}

#[cfg(unix)]
#[test]
fn repeated_post_call_budget_excess_cannot_bypass_the_cumulative_limit() {
    let db = TestDb::new("claude-cumulative");
    db.seed_tasks(&["TASK-0078"], true);
    let fixture = claude_fixture::fixture(
        &db,
        "cumulative",
        &claude_fixture::result_document(
            "x",
            r#"{"input_tokens":5,"cache_creation_input_tokens":40000,"output_tokens":10}"#,
        ),
        0,
    );
    let trigger = [evidence("cumulative")];

    let mut last = None;
    for _ in 0..MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        // A new handle per call: no in-memory state may carry the budget.
        let mut service = db.service();
        last = Some(
            service
                .propose_decomposition(
                    &fixture.planner,
                    &branch(),
                    request("cumulative"),
                    &policy(false),
                )
                .expect("typed outcome"),
        );
    }

    let service = db.service();
    let state = service.guardrail_state(&branch(), &trigger).unwrap();
    assert_eq!(
        fixture.spawn_count(),
        7,
        "the 8th call is refused pre-spawn"
    );
    assert_eq!(state.planner_calls, 7);
    assert_eq!(state.charged_input_tokens, 7 * 40_005);
    assert!(state.charged_input_tokens > u64::from(MAX_PLANNER_CUMULATIVE_INPUT_TOKENS));
    match last.expect("calls ran") {
        PlanningProposalOutcome::WaitingForHuman(decision) => {
            assert_eq!(decision.reason, HumanDecisionReason::PlannerBudget);
        }
        other => panic!("expected cumulative-budget human decision, got {other:?}"),
    }
    for call in 1..=7 {
        assert!(service
            .planner_call_receipt(&branch(), &trigger, call)
            .unwrap()
            .is_some());
    }
    assert_no_plan_persisted(&db);
}

#[cfg(unix)]
#[test]
fn malformed_planner_output_after_measured_spend_keeps_receipt_and_charge() {
    let db = TestDb::new("claude-malformed");
    let fixture = claude_fixture::fixture(
        &db,
        "malformed",
        &claude_fixture::result_document(
            "not planner json",
            r#"{"input_tokens":20000,"output_tokens":7000}"#,
        ),
        0,
    );
    let mut service = db.service();
    let trigger = [evidence("malformed")];

    let error = service
        .propose_decomposition(
            &fixture.planner,
            &branch(),
            request("malformed"),
            &policy(false),
        )
        .expect_err("malformed provider output stays a typed failure");
    assert!(matches!(
        error,
        PlanningServiceError::Planner(PlannerFault::ProviderFailure { .. })
    ));
    let state = service.guardrail_state(&branch(), &trigger).unwrap();
    assert_eq!(
        (state.planner_calls, state.charged_input_tokens),
        (1, 20_000)
    );
    let receipt = service
        .planner_call_receipt(&branch(), &trigger, 1)
        .unwrap()
        .expect("measured receipt is not discarded");
    assert_eq!(
        (receipt.input_tokens, receipt.output_tokens),
        (20_000, 7_000)
    );
    assert_no_plan_persisted(&db);
}

#[cfg(unix)]
#[test]
fn provider_failure_without_usage_is_charged_conservatively_without_receipt() {
    let db = TestDb::new("claude-unmeasured");
    let fixture = claude_fixture::fixture(&db, "unmeasured", "", 3);
    let mut service = db.service();
    let trigger = [evidence("unmeasured")];

    assert!(service
        .propose_decomposition(
            &fixture.planner,
            &branch(),
            request("unmeasured"),
            &policy(false),
        )
        .is_err());
    assert_eq!(fixture.spawn_count(), 1);
    let state = service.guardrail_state(&branch(), &trigger).unwrap();
    assert_eq!(
        (state.planner_calls, state.charged_input_tokens),
        (1, u64::from(MAX_PLANNER_INPUT_TOKENS))
    );
    assert_eq!(
        service
            .planner_call_receipt(&branch(), &trigger, 1)
            .unwrap(),
        None,
        "no exact usage exists, so no measured receipt may be fabricated"
    );
    assert_no_plan_persisted(&db);
}

// ---------------------------------------------------------------------------
// R2-03: guardrails are product-owned and survive restart
// ---------------------------------------------------------------------------

#[test]
fn planner_call_budget_is_hard_and_prevents_ninth_call() {
    let db = TestDb::new("call-budget");
    db.seed_tasks(&["TASK-0078"], true);
    let planner = FaultPlanner::new(measured_fault(100));
    let trigger = [evidence("same-evidence")];

    for _ in 0..4 {
        let mut service = db.service();
        assert!(service
            .propose_decomposition(
                &planner,
                &branch(),
                request("same-evidence"),
                &policy(false)
            )
            .is_err());
    }
    // Restart: a new service handle continues from the durable count.
    assert_eq!(
        db.service()
            .guardrail_state(&branch(), &trigger)
            .unwrap()
            .planner_calls,
        4
    );
    let mut service = db.service();
    for _ in 4..MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        assert!(service
            .propose_decomposition(
                &planner,
                &branch(),
                request("same-evidence"),
                &policy(false)
            )
            .is_err());
    }
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );

    let mut restarted = db.service();
    let blocked = restarted
        .propose_decomposition(
            &planner,
            &branch(),
            request("same-evidence"),
            &policy(false),
        )
        .expect("budget trip");
    match blocked {
        PlanningProposalOutcome::WaitingForHuman(decision) => {
            assert_eq!(decision.reason, HumanDecisionReason::PlannerBudget);
            assert_eq!(
                decision.options,
                vec![HumanContinuation::Replan, HumanContinuation::Abort]
            );
        }
        other => panic!("expected planner-budget human decision, got {other:?}"),
    }
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );
    assert_eq!(
        restarted
            .guardrail_state(&branch(), &trigger)
            .unwrap()
            .charged_input_tokens,
        u64::from(MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH) * 100
    );
    assert_no_plan_persisted(&db);
}

#[test]
fn cumulative_token_budget_survives_restart_for_same_evidence() {
    let db = TestDb::new("token-restart");
    let planner = FaultPlanner::new(measured_fault(30_000));
    for _ in 0..3 {
        let mut service = db.service();
        assert!(service
            .propose_decomposition(&planner, &branch(), request("token-event"), &policy(false))
            .is_err());
    }

    let mut restarted = db.service();
    assert!(restarted
        .propose_decomposition(&planner, &branch(), request("token-event"), &policy(false))
        .is_err());
    let budgets = planner.budgets();
    assert_eq!(budgets.len(), 4);
    assert_eq!(
        budgets[3].input_tokens_remaining,
        MAX_PLANNER_CUMULATIVE_INPUT_TOKENS - 90_000,
        "the restarted handle hands the planner the reduced durable headroom"
    );
    assert_eq!(budgets[3].calls_remaining, 5);
    assert_eq!(
        restarted
            .guardrail_state(&branch(), &[evidence("token-event")])
            .unwrap()
            .charged_input_tokens,
        120_000
    );
}

#[test]
fn l3_no_progress_survives_restart_for_same_evidence() {
    let db = TestDb::new("l3-restart");
    db.seed_tasks(&["TASK-0078"], true);
    let planner = TestPlanner::new(ProposalCase::InvalidHash);

    let first = db
        .service()
        .propose_decomposition(&planner, &branch(), request("l3-evidence"), &policy(false))
        .expect("first rejection");
    assert!(matches!(
        first,
        PlanningProposalOutcome::Refused(ValidationRefusal::Malformed { .. })
    ));

    let second = db
        .service()
        .propose_decomposition(&planner, &branch(), request("l3-evidence"), &policy(false))
        .expect("second rejection");
    let decision_id = match second {
        PlanningProposalOutcome::WaitingForHuman(decision) => {
            assert_eq!(decision.reason, HumanDecisionReason::NoProgress);
            assert_eq!(
                decision.options,
                vec![HumanContinuation::Replan, HumanContinuation::Abort]
            );
            decision.id
        }
        other => panic!("second same-evidence rejection must escalate to human, got {other:?}"),
    };

    let third = db
        .service()
        .propose_decomposition(&planner, &branch(), request("l3-evidence"), &policy(false))
        .expect("tripped branch");
    assert!(matches!(
        third,
        PlanningProposalOutcome::WaitingForHuman(decision) if decision.id == decision_id
    ));
    assert_eq!(
        planner.call_count(),
        2,
        "a tripped event never reaches the planner"
    );
    assert_no_plan_persisted(&db);
}

#[test]
fn v7_duplicate_decomposition_is_refused_from_durable_lineage_after_restart() {
    let db = TestDb::new("v7-restart");
    db.seed_tasks(&["replay-node-1", "replay-node-2"], true);
    let accepted = db
        .service()
        .propose_decomposition(
            &ReplayPlanner,
            &branch(),
            request("replayed-evidence"),
            &policy(false),
        )
        .expect("first decomposition");
    assert!(matches!(accepted, PlanningProposalOutcome::Accepted(_)));
    let lineage = db.store().get_plan_lineage(&branch()).unwrap();
    assert_eq!(lineage.len(), 1);

    // Restart with a fresh trigger: the planner replays decomposition X.
    let mut restarted = db.service();
    let outcome = restarted
        .propose_decomposition(
            &ReplayPlanner,
            &branch(),
            request("fresh-trigger"),
            &policy(false),
        )
        .expect("duplicate decomposition is a typed refusal");
    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::DuplicateDecomposition {
            fingerprint,
        }) => assert_eq!(fingerprint, lineage[0].decomposition_fingerprint),
        other => panic!("expected DUPLICATE_DECOMPOSITION from durable lineage, got {other:?}"),
    }
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
}

#[test]
fn same_evidence_in_any_order_can_never_reset_guardrails() {
    let db = TestDb::new("fake-reset");
    db.seed_tasks(&["node-1"], true);
    let accepted = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::Valid),
            &branch(),
            request_with(vec![evidence("attempt-a"), evidence("attempt-b")]),
            &policy(false),
        )
        .expect("accepted");
    assert!(matches!(accepted, PlanningProposalOutcome::Accepted(_)));

    // A new handle, reordered and repeated evidence: the same event.
    let planner = TestPlanner::new(ProposalCase::Valid);
    let mut restarted = db.service();
    let state = restarted
        .guardrail_state(
            &branch(),
            &[
                evidence("attempt-b"),
                evidence("attempt-a"),
                evidence("attempt-a"),
            ],
        )
        .unwrap();
    assert_eq!((state.planner_calls, state.revisions_accepted), (1, 1));
    let outcome = restarted
        .propose_decomposition(
            &planner,
            &branch(),
            request_with(vec![evidence("attempt-b"), evidence("attempt-a")]),
            &policy(false),
        )
        .expect("same event is bounded");
    assert!(matches!(
        outcome,
        PlanningProposalOutcome::GuardrailTripped(trip) if trip.bound == "revisions-per-cycle"
    ));
    assert_eq!(planner.call_count(), 0);

    let empty = restarted
        .propose_decomposition(&planner, &branch(), request_with(vec![]), &policy(false))
        .expect_err("planning without triggering evidence has no event to bound");
    assert!(matches!(empty, PlanningServiceError::InvalidContext(_)));
    assert_eq!(planner.call_count(), 0);
}

// ---------------------------------------------------------------------------
// R2-04: applicability and L4 are checked before durable acceptance
// ---------------------------------------------------------------------------

#[test]
fn task_not_in_candidate_plan_is_refused_before_commit() {
    let db = TestDb::new("task-not-in-plan");
    let mut orchestration = db.orchestration();
    seed_into(
        orchestration.tasks_mut(),
        &["planned-child-0", "unplanned-task"],
        true,
    );
    let mut planning = db.service();
    let executor = CountingExecutor::new();

    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &FakePlanner::for_scenario(FakePlannerScenario::Valid),
            &branch(),
            request("not-in-plan"),
            policy(true),
            &TaskId::new("unplanned-task").unwrap(),
            ids("NOT-IN-PLAN"),
            &executor,
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .expect("applicability refusal is a typed outcome");
    assert!(matches!(
        outcome,
        PlannedOrchestrationOutcome::PlanningStopped(PlanningProposalOutcome::NotApplicable(
            PlanApplicabilityRefusal::TaskNotInPlan { .. }
        ))
    ));
    assert_eq!(executor.calls(), 0);
    assert_no_plan_persisted(&db);
}

#[test]
fn plan_predecessor_without_canonical_task_is_refused_before_commit() {
    let db = TestDb::new("predecessor-missing");
    let mut orchestration = db.orchestration();
    seed_into(orchestration.tasks_mut(), &["step-b"], true);
    let mut planning = db.service();
    let executor = CountingExecutor::new();

    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &FakePlanner::for_scenario(FakePlannerScenario::DependencyChain),
            &branch(),
            request("predecessor-missing"),
            policy(true),
            &TaskId::new("step-b").unwrap(),
            ids("PREDECESSOR"),
            &executor,
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .expect("applicability refusal is a typed outcome");
    match outcome {
        PlannedOrchestrationOutcome::PlanningStopped(PlanningProposalOutcome::NotApplicable(
            PlanApplicabilityRefusal::PredecessorWithoutTask { node },
        )) => assert_eq!(node, "step-a"),
        other => panic!("expected missing predecessor refusal, got {other:?}"),
    }
    assert_eq!(executor.calls(), 0);
    assert_no_plan_persisted(&db);
}

#[test]
fn structurally_valid_plan_without_runnable_task_trips_l4_before_commit() {
    for (label, seeded, ready) in [
        ("no-canonical-task", &[][..], true),
        ("task-not-ready", &["planned-child-0"][..], false),
    ] {
        let db = TestDb::new(label);
        db.seed_tasks(&["TASK-0078"], true);
        db.seed_tasks(seeded, ready);
        let outcome = db
            .service()
            .propose_decomposition(
                &FakePlanner::for_scenario(FakePlannerScenario::Valid),
                &branch(),
                request(label),
                &policy(true),
            )
            .expect("L4 escalation is typed");
        assert!(
            matches!(
                outcome,
                PlanningProposalOutcome::WaitingForHuman(ref decision)
                    if decision.reason == HumanDecisionReason::PlannerBlocked
                        && decision.options == [HumanContinuation::Replan, HumanContinuation::Abort]
            ),
            "{label}: a sink node without runnable canonical work must escalate L4, got {outcome:?}"
        );
        assert_no_plan_persisted(&db);
    }
}

#[test]
fn planning_path_errors_never_follow_a_plan_mutation() {
    // Pre-commit provider failure: Err, head unchanged.
    let db = TestDb::new("err-no-mutation");
    let mut orchestration = db.orchestration();
    seed_into(
        orchestration.tasks_mut(),
        &["planned-child-0", "other"],
        true,
    );
    let mut planning = db.service();
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let target = TaskId::new("planned-child-0").unwrap();

    let error = orchestration
        .schedule_with_planning(
            &mut planning,
            &FakePlanner::for_scenario(FakePlannerScenario::ProviderFailure),
            &branch(),
            request("provider-error"),
            policy(true),
            &target,
            ids("PROVIDER-ERROR"),
            &executor,
            &evaluator,
        )
        .expect_err("provider failure is an error");
    assert!(matches!(
        error,
        app_services::PlannedOrchestrationError::Planning(PlanningServiceError::Planner(_))
    ));
    assert_no_plan_persisted(&db);

    let error = orchestration
        .schedule_with_planning(
            &mut planning,
            &FakePlanner::for_scenario(FakePlannerScenario::Valid),
            &branch(),
            request_with(vec![]),
            policy(true),
            &target,
            ids("NO-EVIDENCE"),
            &executor,
            &evaluator,
        )
        .expect_err("invalid context is an error");
    assert!(matches!(
        error,
        app_services::PlannedOrchestrationError::Planning(PlanningServiceError::InvalidContext(_))
    ));
    assert_no_plan_persisted(&db);

    // Post-commit scheduling failure (duplicate execution identity): the
    // revision is accepted, so the result is Ok carrying the plan.
    let other = orchestration
        .schedule(
            &TaskId::new("other").unwrap(),
            ids("DUPLICATE"),
            &executor,
            &evaluator,
        )
        .expect("unrelated execution");
    assert!(matches!(
        other,
        app_services::OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &FakePlanner::for_scenario(FakePlannerScenario::Valid),
            &branch(),
            request("post-commit-failure"),
            policy(true),
            &target,
            ids("DUPLICATE"),
            &executor,
            &evaluator,
        )
        .expect("post-commit failure is an outcome, never Err");
    match outcome {
        PlannedOrchestrationOutcome::SchedulingFailed { plan, .. } => {
            assert_eq!(plan.revision().revision_seq, 1);
        }
        other => panic!("expected SchedulingFailed with accepted plan, got {other:?}"),
    }
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
}

#[test]
fn current_plan_scheduling_reports_missing_plan_and_unrepresented_task_without_planner() {
    let db = TestDb::new("current-plan-negative");
    let mut orchestration = db.orchestration();
    seed_into(orchestration.tasks_mut(), &["node-1", "unplanned"], true);
    let planning = db.service();
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

    let none = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch(),
            &TaskId::new("node-1").unwrap(),
            ids("NO-PLAN"),
            &executor,
            &evaluator,
        )
        .expect("no plan is an outcome");
    assert!(matches!(none, PlannedOrchestrationOutcome::NoAcceptedPlan));

    let mut writer = db.service();
    assert!(matches!(
        writer
            .propose_decomposition(
                &TestPlanner::new(ProposalCase::Valid),
                &branch(),
                request("current-plan"),
                &policy(false),
            )
            .unwrap(),
        PlanningProposalOutcome::Accepted(_)
    ));
    let unrepresented = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch(),
            &TaskId::new("unplanned").unwrap(),
            ids("UNREPRESENTED"),
            &executor,
            &evaluator,
        )
        .expect("unrepresented task is an outcome");
    assert!(matches!(
        unrepresented,
        PlannedOrchestrationOutcome::TaskNotInAcceptedPlan { .. }
    ));
    assert_eq!(executor.calls(), 0);
}

#[test]
fn production_entry_without_accepted_plan_leaves_task_execution_and_attempt_unmodified() {
    let db = TestDb::new("production-entry-no-plan");
    db.seed_tasks(&["node-1"], true);
    let task_id = TaskId::new("node-1").unwrap();
    let execution_id = ExecutionId::new("E-NO-ACCEPTED-PLAN").unwrap();
    let attempt_id = AttemptId::new("A-NO-ACCEPTED-PLAN").unwrap();
    let mut orchestration = db.orchestration();
    let planning = db.service();

    let outcome = orchestration
        .enter_from_current_plan(
            &planning,
            &task_id,
            OrchestrationIds {
                execution_id: execution_id.clone(),
                attempt_id: attempt_id.clone(),
            },
        )
        .expect("missing plan is a typed outcome");
    assert!(matches!(
        outcome,
        PlannedExecutionEntryOutcome::NoAcceptedPlan
    ));

    let store = db.store();
    assert_eq!(
        store.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Ready
    );
    assert!(store.get_execution(&execution_id).unwrap().is_none());
    assert!(store.get_attempt(&attempt_id).unwrap().is_none());
    assert!(store
        .get_attempt_reservation(&attempt_id)
        .unwrap()
        .is_none());
}

#[test]
fn production_entry_for_task_outside_current_plan_leaves_state_unmodified() {
    let db = TestDb::new("production-entry-unrepresented");
    seed_head(&db, 1);
    db.seed_tasks(&["unplanned"], true);
    let task_id = TaskId::new("unplanned").unwrap();
    let execution_id = ExecutionId::new("E-UNREPRESENTED").unwrap();
    let attempt_id = AttemptId::new("A-UNREPRESENTED").unwrap();
    let mut orchestration = db.orchestration();
    let planning = db.service();

    let outcome = orchestration
        .enter_from_current_plan(
            &planning,
            &task_id,
            OrchestrationIds {
                execution_id: execution_id.clone(),
                attempt_id: attempt_id.clone(),
            },
        )
        .expect("unrepresented task is a typed outcome");
    assert!(matches!(
        outcome,
        PlannedExecutionEntryOutcome::NoAcceptedPlan
    ));

    let store = db.store();
    assert_eq!(
        store.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Ready
    );
    assert!(store.get_execution(&execution_id).unwrap().is_none());
    assert!(store.get_attempt(&attempt_id).unwrap().is_none());
    assert!(store
        .get_attempt_reservation(&attempt_id)
        .unwrap()
        .is_none());
}

#[test]
fn production_entry_uses_current_accepted_plan_without_creating_a_revision() {
    let db = TestDb::new("production-entry-current-plan");
    seed_head(&db, 1);
    let task_id = TaskId::new("node-1").unwrap();
    let execution_id = ExecutionId::new("E-CURRENT-PLAN").unwrap();
    let attempt_id = AttemptId::new("A-CURRENT-PLAN").unwrap();
    let mut orchestration = db.orchestration();
    let planning = db.service();

    let outcome = orchestration
        .enter_from_current_plan(
            &planning,
            &task_id,
            OrchestrationIds {
                execution_id: execution_id.clone(),
                attempt_id: attempt_id.clone(),
            },
        )
        .expect("accepted current plan admits execution");
    assert!(matches!(
        outcome,
        PlannedExecutionEntryOutcome::Entered { .. }
    ));

    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).unwrap(), Some(1));
    assert!(store.get_plan_revision(&branch(), 2).unwrap().is_none());
    assert_eq!(
        store.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );
    assert!(store.get_execution(&execution_id).unwrap().is_some());
    assert!(store
        .get_attempt_reservation(&attempt_id)
        .unwrap()
        .is_some());
}

#[test]
fn stale_plan_head_at_admission_leaves_task_execution_and_attempt_unmodified() {
    let db = TestDb::new("stale-plan-head-admission");
    seed_head(&db, 1);
    let task_id = TaskId::new("node-1").unwrap();
    let execution_id = ExecutionId::new("E-STALE-PLAN-HEAD").unwrap();
    let attempt_id = AttemptId::new("A-STALE-PLAN-HEAD").unwrap();

    let accepted = db
        .service()
        .current_accepted_plan(&branch())
        .expect("read accepted plan")
        .expect("revision one is accepted");
    assert_eq!(accepted.revision().revision_seq, 1);

    // Build the otherwise valid entry intent while revision 1 is canonical,
    // then advance the branch before SQLite begins admission. This is the
    // former app-check-to-transaction TOCTOU window, made deterministic.
    let mut tasks = TaskService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("open tasks");
    let mut runtime =
        RuntimeService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("open runtime");
    let claimed = tasks.plan_claim(&task_id).expect("plan claim");
    let started = tasks.plan_start(&claimed.task).expect("plan start");
    let planned = runtime
        .plan_execution(execution_id.clone(), task_id.clone())
        .expect("plan execution");

    let mut writer = db.service();
    assert!(matches!(
        writer
            .propose_decomposition(
                &TestPlanner::new(ProposalCase::Valid),
                &branch(),
                request("advance-before-admission"),
                &policy(false),
            )
            .expect("advance head"),
        PlanningProposalOutcome::Accepted(_)
    ));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(2));

    let managed_branch = app_services::branch_for_execution(&execution_id).expect("branch");
    let admission = ExecutionEntryAdmission {
        plan_head: Some(PlanHeadPrecondition {
            plan_branch_id: branch(),
            revision_seq: accepted.revision().revision_seq,
            planning_input: None,
        }),
        task_id: &task_id,
        claimed_task: &claimed.task,
        claimed_event: &claimed.event,
        started_task: &started.task,
        started_event: &started.event,
        execution: &planned.execution,
        execution_event: &planned.event,
        attempt_id: &attempt_id,
        reserved_at: planned.reserved_at.clone(),
        managed_branch: managed_branch.as_str(),
    };
    let error = match db.store().admit_execution_entry(&admission) {
        Ok(_) => panic!("moved head must refuse admission"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        StorageError::PlanStale {
            ref branch,
            expected: 1,
            found: 2,
        } if branch == "task-0078-branch"
    ));

    let store = db.store();
    assert_eq!(
        store.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Ready
    );
    assert!(store.get_execution(&execution_id).unwrap().is_none());
    assert!(store.get_attempt(&attempt_id).unwrap().is_none());
    assert!(store
        .get_attempt_reservation(&attempt_id)
        .unwrap()
        .is_none());
    assert!(store
        .list_events_for_task(&task_id)
        .unwrap()
        .iter()
        .all(|event| {
            !matches!(
                event.fact.event_type,
                EventType::Claimed | EventType::Started | EventType::ExecutionCreated
            )
        }));
}

#[test]
fn stale_planning_input_at_admission_with_unchanged_plan_head_rolls_back_everything() {
    let db = TestDb::new("stale-planning-input-admission");
    seed_head(&db, 1);
    let task_id = TaskId::new("node-1").unwrap();
    let execution_id = ExecutionId::new("E-STALE-PLANNING-INPUT").unwrap();
    let attempt_id = AttemptId::new("A-STALE-PLANNING-INPUT").unwrap();
    let accepted = db
        .service()
        .current_accepted_plan(&branch())
        .expect("read accepted plan")
        .expect("revision one is accepted");

    let mut tasks = TaskService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("open tasks");
    let mut runtime =
        RuntimeService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("open runtime");
    let claimed = tasks.plan_claim(&task_id).expect("plan claim");
    let started = tasks.plan_start(&claimed.task).expect("plan start");
    let planned = runtime
        .plan_execution(execution_id.clone(), task_id.clone())
        .expect("plan execution");

    // Supersede only the product-owned planning input. The accepted Plan
    // head deliberately stays at revision 1 so this exercises the second
    // execution-admission fence rather than Plan CAS.
    let trigger = vec![evidence("superseding-input")];
    let snapshot = PlanningInputSnapshot::new(
        branch(),
        "TASK-0078".to_owned(),
        1,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        trigger.clone(),
        vec![],
        None,
        "policy:superseding-input".to_owned(),
    )
    .unwrap();
    let mut store = db.store();
    store
        .start_planning_run(
            &snapshot,
            &planning_evidence_key(&trigger),
            FIXED_MILLIS + 1,
        )
        .expect("supersede planning input");
    assert_eq!(store.get_plan_head(&branch()).unwrap(), Some(1));

    let managed_branch = app_services::branch_for_execution(&execution_id).expect("branch");
    let admission = ExecutionEntryAdmission {
        plan_head: Some(PlanHeadPrecondition {
            plan_branch_id: branch(),
            revision_seq: accepted.revision().revision_seq,
            planning_input: Some(PlanningInputPrecondition {
                snapshot_id: accepted.planning_input_snapshot_id().clone(),
                input_fingerprint: accepted.planning_input_fingerprint().to_owned(),
            }),
        }),
        task_id: &task_id,
        claimed_task: &claimed.task,
        claimed_event: &claimed.event,
        started_task: &started.task,
        started_event: &started.event,
        execution: &planned.execution,
        execution_event: &planned.event,
        attempt_id: &attempt_id,
        reserved_at: planned.reserved_at.clone(),
        managed_branch: managed_branch.as_str(),
    };
    let error = match store.admit_execution_entry(&admission) {
        Ok(_) => panic!("stale planning input must refuse admission"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        StorageError::PlanningInputStale { ref branch, .. }
            if branch == "task-0078-branch"
    ));

    assert_eq!(
        store.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Ready
    );
    assert!(store.get_execution(&execution_id).unwrap().is_none());
    assert!(store.get_attempt(&attempt_id).unwrap().is_none());
    assert!(store
        .get_attempt_reservation(&attempt_id)
        .unwrap()
        .is_none());
    assert!(store
        .list_events_for_task(&task_id)
        .unwrap()
        .iter()
        .all(|event| {
            !matches!(
                event.fact.event_type,
                EventType::Claimed | EventType::Started | EventType::ExecutionCreated
            )
        }));
}

// ---------------------------------------------------------------------------
// TASK-0079: bounded durable Human Decision Queue
// ---------------------------------------------------------------------------

#[test]
fn risk_decision_survives_restart_and_resume_is_consumed_with_plan_commit() {
    let db = TestDb::new("human-risk-resume");
    db.seed_tasks(&["TASK-0078", "node-1"], true);
    let planner = TestPlanner::new(ProposalCase::RiskEscalation);

    let waiting = db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("risk-needs-human"),
            &policy(false),
        )
        .expect("risk escalation produces a bounded decision request");
    let decision = match waiting {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected human decision request, got {other:?}"),
    };
    assert_eq!(decision.reason, HumanDecisionReason::RiskEscalation);
    assert_eq!(decision.status, HumanDecisionStatus::Pending);
    assert_eq!(decision.risk, RiskClass::High);
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), None);

    let calls_before_retry = planner.call_count();
    let repeated = db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("risk-needs-human"),
            &policy(false),
        )
        .expect("pending human decision remains the planning gate");
    assert!(matches!(
        repeated,
        PlanningProposalOutcome::WaitingForHuman(ref existing) if existing.id == decision.id
    ));
    assert_eq!(
        planner.call_count(),
        calls_before_retry,
        "a pending V9 decision cannot be bypassed by asking the planner again"
    );

    // A new service handle over the same SQLite file is the daemon-restart
    // boundary. The request and its exact preconditions survive it.
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    assert_eq!(human.get(&decision.id).unwrap(), Some(decision.clone()));
    assert_eq!(human.list_pending().unwrap(), vec![decision.clone()]);
    human
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect("resolve after restart");
    let grant = human.risk_grant(&decision.id).expect("bound V9 grant");
    drop(human);

    // RESUME re-enters the ordinary G7 planning path. TASK-0078 correctly
    // gives the grant-bearing retry a new PlanningInputSnapshot fingerprint;
    // commit accepts only that exact derived transition and consumes the
    // source decision in the same SQLite transaction as revision 1.
    let mut resumed_policy = policy(false);
    resumed_policy.risk_grant = Some(grant);
    resumed_policy.risk_decision_id = Some(decision.id.clone());
    let accepted = db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("risk-needs-human"),
            &resumed_policy,
        )
        .expect("exact resumed proposal commits");
    assert!(matches!(accepted, PlanningProposalOutcome::Accepted(_)));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));

    let persisted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .get(&decision.id)
        .unwrap()
        .expect("decision after commit");
    assert_eq!(persisted.status, HumanDecisionStatus::Consumed);
    assert_eq!(persisted.selected, Some(HumanContinuation::Resume));
    assert!(persisted.consumed_at_ms.is_some());
    assert_eq!(planner.call_count(), 2);
}

#[test]
fn risk_decision_replan_uses_original_policy_input_and_normal_g7_boundary() {
    let db = TestDb::new("human-risk-replan");
    db.seed_tasks(&["TASK-0078", "node-1"], true);
    let waiting = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::RiskEscalation),
            &branch(),
            request("risk-replan"),
            &policy(false),
        )
        .unwrap();
    let decision = match waiting {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected risk decision, got {other:?}"),
    };
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    human
        .decide(&decision.id, HumanContinuation::Replan, "operator:test")
        .unwrap();

    let replan_planner = TestPlanner::new(ProposalCase::Valid);
    let mut planning = db.service();
    let outcome = human
        .apply_replan(&mut planning, &replan_planner, &decision.id, &policy(false))
        .expect("risk REPLAN uses the original canonical policy baseline");
    assert!(matches!(outcome, PlanningProposalOutcome::Accepted(_)));
    assert_eq!(replan_planner.call_count(), 1);
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert_eq!(
        human.get(&decision.id).unwrap().unwrap().status,
        HumanDecisionStatus::Consumed
    );
}

#[test]
fn raw_risk_grant_without_durable_decision_binding_cannot_publish() {
    let db = TestDb::new("human-risk-no-binding");
    db.seed_tasks(&["TASK-0078", "node-1"], true);
    let planner = TestPlanner::new(ProposalCase::RiskEscalation);
    let waiting = db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("risk-no-binding"),
            &policy(false),
        )
        .unwrap();
    let decision = match waiting {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected human decision, got {other:?}"),
    };
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    human
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .unwrap();
    let grant = human.risk_grant(&decision.id).unwrap();
    drop(human);

    let mut bearer_only = policy(false);
    bearer_only.risk_grant = Some(grant);
    let error = db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("risk-no-binding"),
            &bearer_only,
        )
        .expect_err("raw grant is not publication authority");
    assert!(matches!(error, PlanningServiceError::InvalidContext(_)));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), None);
}

#[test]
fn resume_is_restart_durable_once_only_and_reopens_only_owner_scope() {
    let db = TestDb::new("human-resume-restart");
    seed_head(&db, 1);
    let task_id = TaskId::new("node-1").unwrap();
    let decision = create_human_decision(
        &db,
        task_id.as_str(),
        vec![HumanContinuation::Resume, HumanContinuation::Abort],
    );

    // Restart between request and response.
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    assert_eq!(human.list_pending().unwrap().len(), 1);
    human
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .unwrap();
    drop(human);

    // Restart after response and before continuation. The resolved row is
    // durable and is consumed exactly once.
    let mut restarted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    assert_eq!(
        restarted.get(&decision.id).unwrap().unwrap().status,
        HumanDecisionStatus::Resolved
    );
    let consumed = restarted.apply_local(&decision.id).expect("apply resume");
    assert_eq!(consumed.status, HumanDecisionStatus::Consumed);
    let replay = restarted
        .apply_local(&decision.id)
        .expect_err("consumed decision cannot replay");
    assert!(matches!(
        replay,
        app_services::ServiceError::Storage(StorageError::HumanDecisionState { .. })
    ));
    drop(restarted);

    let mut orchestration = db.orchestration();
    let outcome = orchestration
        .enter_from_current_plan(&db.service(), &task_id, ids("HDQ-RESUME"))
        .expect("resume removes bounded gate");
    assert!(matches!(
        outcome,
        PlannedExecutionEntryOutcome::Entered { .. }
    ));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
}

#[test]
fn pending_and_abort_block_only_owner_while_independent_task_enters() {
    let db = TestDb::new("human-bounded-blocking");
    seed_head(&db, 2);
    let task_a = TaskId::new("node-1").unwrap();
    let task_b = TaskId::new("node-2").unwrap();
    let decision = create_human_decision(
        &db,
        task_a.as_str(),
        vec![
            HumanContinuation::Resume,
            HumanContinuation::Replan,
            HumanContinuation::Abort,
        ],
    );
    let planning = db.service();
    let mut orchestration = db.orchestration();

    let waiting = orchestration
        .enter_from_current_plan(&planning, &task_a, ids("HDQ-A-WAIT"))
        .expect("owner is gated");
    assert!(matches!(
        waiting,
        PlannedExecutionEntryOutcome::WaitingForHuman { .. }
    ));
    assert_eq!(
        db.store().get_task(&task_a).unwrap().unwrap().state(),
        TaskState::Ready
    );

    let independent = orchestration
        .enter_from_current_plan(&planning, &task_b, ids("HDQ-B-INDEPENDENT"))
        .expect("independent task remains runnable");
    assert!(matches!(
        independent,
        PlannedExecutionEntryOutcome::Entered { .. }
    ));
    assert_eq!(
        db.store().get_task(&task_b).unwrap().unwrap().state(),
        TaskState::Running
    );

    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    human
        .decide(&decision.id, HumanContinuation::Abort, "operator:test")
        .unwrap();
    human
        .apply_local(&decision.id)
        .expect("consume bounded abort");
    drop(human);

    let aborted = orchestration
        .enter_from_current_plan(&planning, &task_a, ids("HDQ-A-ABORT"))
        .expect("abort is a bounded outcome");
    assert!(matches!(
        aborted,
        PlannedExecutionEntryOutcome::AbortedByHuman { .. }
    ));
    assert_eq!(
        db.store().get_task(&task_a).unwrap().unwrap().state(),
        TaskState::Ready
    );
    assert_eq!(
        db.store().get_task(&task_b).unwrap().unwrap().state(),
        TaskState::Running,
        "ABORT on A must not cancel or mutate independent B"
    );
}

#[test]
fn human_decision_stale_fences_task_plan_snapshot_scope_and_risk_changes() {
    // Task state/evidence changed.
    let db = TestDb::new("human-stale-task");
    seed_head(&db, 1);
    let decision = create_human_decision(&db, "node-1", vec![HumanContinuation::Resume]);
    TaskService::open(&db.path, FixedClock(FIXED_MILLIS))
        .unwrap()
        .claim(&TaskId::new("node-1").unwrap())
        .unwrap();
    let error = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect_err("Task mutation makes decision stale");
    assert!(matches!(
        error,
        app_services::ServiceError::Storage(StorageError::HumanDecisionStale { ref reason, .. })
            if reason.contains("Task state")
    ));

    // Plan head moved.
    let db = TestDb::new("human-stale-head");
    seed_head(&db, 1);
    let decision = create_human_decision(&db, "node-1", vec![HumanContinuation::Resume]);
    assert!(matches!(
        db.service()
            .propose_decomposition(
                &TestPlanner::new(ProposalCase::Valid),
                &branch(),
                request("move-head-after-decision"),
                &policy(false),
            )
            .unwrap(),
        PlanningProposalOutcome::Accepted(_)
    ));
    let error = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect_err("moved Plan head makes decision stale");
    assert!(matches!(
        error,
        app_services::ServiceError::Storage(StorageError::HumanDecisionStale { ref reason, .. })
            if reason == "Plan head moved"
    ));

    // Same Plan head, new snapshot/fingerprint only.
    let db = TestDb::new("human-stale-snapshot");
    seed_head(&db, 1);
    let decision = create_human_decision(&db, "node-1", vec![HumanContinuation::Resume]);
    replace_current_planning_input(
        &db,
        vec!["repo:src"],
        RiskClass::Low,
        "policy:test",
        "snapshot-only-change",
    );
    let error = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect_err("changed snapshot makes decision stale");
    assert!(matches!(
        error,
        app_services::ServiceError::Storage(StorageError::HumanDecisionStale { ref reason, .. })
            if reason == "PlanningInputSnapshot changed"
    ));

    // Scope widened under the same Plan head.
    let db = TestDb::new("human-stale-scope");
    seed_head(&db, 1);
    let decision = create_human_decision(&db, "node-1", vec![HumanContinuation::Resume]);
    replace_current_planning_input(
        &db,
        vec!["repo:src", "repo:other"],
        RiskClass::Low,
        "policy:test",
        "scope-widened",
    );
    let error = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect_err("widened scope makes decision stale");
    assert!(matches!(
        error,
        app_services::ServiceError::Storage(StorageError::HumanDecisionStale { ref reason, .. })
            if reason == "affected scope widened"
    ));

    // Risk increased under the same Plan head.
    let db = TestDb::new("human-stale-risk");
    seed_head(&db, 1);
    let decision = create_human_decision(&db, "node-1", vec![HumanContinuation::Resume]);
    replace_current_planning_input(
        &db,
        vec!["repo:src"],
        RiskClass::High,
        "policy:test",
        "risk-increased",
    );
    let error = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect_err("increased risk makes decision stale");
    assert!(matches!(
        error,
        app_services::ServiceError::Storage(StorageError::HumanDecisionStale { ref reason, .. })
            if reason == "risk increased"
    ));
}

#[test]
fn invalid_unknown_superseded_and_duplicate_human_decisions_fail_closed() {
    let db = TestDb::new("human-lifecycle-refusals");
    seed_head(&db, 1);
    let first = create_human_decision(&db, "node-1", vec![HumanContinuation::Resume]);
    let second = create_human_decision(&db, "node-1", vec![HumanContinuation::Abort]);
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));

    let superseded = human
        .decide(&first.id, HumanContinuation::Resume, "operator:test")
        .expect_err("older request was superseded");
    assert!(matches!(
        superseded,
        app_services::ServiceError::Storage(StorageError::HumanDecisionState { ref state, .. })
            if state == "SUPERSEDED"
    ));

    let invalid = human
        .decide(&second.id, HumanContinuation::Resume, "operator:test")
        .expect_err("option outside bounded set is refused");
    assert!(matches!(
        invalid,
        app_services::ServiceError::Storage(StorageError::ConstraintViolation(_))
    ));

    let unknown = HumanDecisionRequestId::new("hdr-unknown").unwrap();
    let missing = human
        .decide(&unknown, HumanContinuation::Abort, "operator:test")
        .expect_err("unknown id is typed not-found");
    assert!(matches!(
        missing,
        app_services::ServiceError::Storage(StorageError::NotFound(_))
    ));

    human
        .decide(&second.id, HumanContinuation::Abort, "operator:test")
        .expect("allowed option resolves");
    human.apply_local(&second.id).expect("first consume");
    let duplicate = human
        .apply_local(&second.id)
        .expect_err("duplicate consume is refused");
    assert!(matches!(
        duplicate,
        app_services::ServiceError::Storage(StorageError::HumanDecisionState { ref state, .. })
            if state == "CONSUMED"
    ));
}

#[test]
fn replan_decision_uses_normal_g7_planning_boundary_and_is_once_only() {
    let db = TestDb::new("human-replan");
    seed_head(&db, 1);
    db.seed_tasks(&["TASK-0078"], true);
    let decision = create_human_decision(&db, "TASK-0078", vec![HumanContinuation::Replan]);
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    human
        .decide(&decision.id, HumanContinuation::Replan, "operator:test")
        .unwrap();
    drop(human);

    let planner = TestPlanner::new(ProposalCase::Valid);
    let mut planning = db.service();
    let mut restarted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    let outcome = restarted
        .apply_replan(&mut planning, &planner, &decision.id, &policy(false))
        .expect("REPLAN crosses ordinary PlanningService boundary");
    assert!(matches!(outcome, PlanningProposalOutcome::Accepted(_)));
    assert_eq!(planner.call_count(), 1);
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(2));
    assert_eq!(
        restarted.get(&decision.id).unwrap().unwrap().status,
        HumanDecisionStatus::Consumed
    );

    let replay = restarted
        .apply_replan(&mut planning, &planner, &decision.id, &policy(false))
        .expect_err("REPLAN cannot be applied twice");
    assert!(matches!(
        replay,
        PlanningServiceError::Storage(StorageError::HumanDecisionStale { .. })
            | PlanningServiceError::Storage(StorageError::HumanDecisionState { .. })
    ));
    assert_eq!(planner.call_count(), 1);
}
