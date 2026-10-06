//! TASK-0078 regression coverage for the trusted proposal -> guardrails ->
//! validation -> applicability -> persistence boundary.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use app_services::{
    FixedClock, HumanDecisionCreate, HumanDecisionService, OrchestrationIds, OrchestrationService,
    PlanApplicabilityRefusal, PlannedExecutionEntryOutcome, PlannedOrchestrationOutcome,
    PlanningAssessmentOutcome, PlanningPolicyContext, PlanningProposalOutcome, PlanningService,
    PlanningServiceError, RuntimeService, ServiceError, TaskService,
};
use evaluation::{
    subject_fingerprint, ClaimVerdict, ClaimedOutcome, ClaimedSubject, EffectKind, EffectPayload,
    EffectProvenance, EffectSubjectRef, EvidenceRefs, Sha256Digest,
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
    SecurityAuditEvent, SqliteStore, StorageError,
};
use task_domain::{DomainError, Project, ProjectId, Task, TaskId, TaskState};

const FIXED_MILLIS: u64 = 1_704_067_200_000;
const MIGRATIONS_THROUGH_16: [&str; 16] = [
    include_str!("../../../adapters/storage-sqlite/migrations/0001_task_core.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0002_event_journal.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0003_event_scope.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0004_runtime_core.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0005_workspace_leases.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0006_attempt_reservations.sql"),
    include_str!(
        "../../../adapters/storage-sqlite/migrations/0007_managed_identity_reservations.sql"
    ),
    include_str!("../../../adapters/storage-sqlite/migrations/0008_approvals.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0009_secret_refs.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0010_security_audit.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0011_workspace_trusted_anchor.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0012_plan_persistence.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0013_plan_cost_receipts.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0014_plan_guardrails.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0015_human_decisions.sql"),
    include_str!("../../../adapters/storage-sqlite/migrations/0016_g7_closure_remediation.sql"),
];

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
    project_id: ProjectId,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let db = Self::new_unseeded(name);
        db.seed_tasks(&[], true);
        db
    }

    fn new_unseeded(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-planning-service-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
            project_id: ProjectId::new(format!("P-PLANNING-{id}")).expect("project id"),
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
        seed_into(&mut tasks, &self.project_id, names, ready);
    }
}

fn seed_into(
    tasks: &mut TaskService<FixedClock>,
    project: &ProjectId,
    names: &[&str],
    ready: bool,
) {
    if tasks
        .store()
        .get_project(project)
        .expect("read project")
        .is_none()
    {
        tasks
            .create_project(&Project::new(project.clone(), "planning fixture".to_owned()).unwrap())
            .expect("create project");
    }
    let parent_id = TaskId::new("TASK-0078").expect("parent task id");
    if tasks
        .store()
        .get_task(&parent_id)
        .expect("read parent")
        .is_none()
    {
        let parent = Task::draft(
            parent_id.clone(),
            project.clone(),
            "planning root".to_owned(),
        )
        .expect("draft parent");
        tasks.create_task(&parent).expect("create parent");
        if ready {
            tasks.mark_ready(parent.id()).expect("mark parent ready");
        }
    }
    for name in names {
        let id = TaskId::new(*name).expect("task id");
        if tasks.store().get_task(&id).expect("read task").is_some() {
            continue;
        }
        let mut task =
            Task::draft(id, project.clone(), "planned work".to_owned()).expect("draft task");
        if task.id() != &parent_id {
            task.set_parent(parent_id.clone()).expect("set task parent");
        }
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
    ForeignNode,
    /// The planner names the owning parent Task as a new child.
    ParentAsChild,
    /// The new child `node-{base+1}` depends on the previous head's child.
    BehindPrevious,
    /// The child declares a proof method the canonical Task cannot carry.
    NonAutomatedCriterion,
    /// The child repeats one criterion id; validation must refuse it before
    /// the materialization boundary can turn it into an untyped context error.
    DuplicateCriterion,
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
            ProposalCase::NonAutomatedCriterion => {
                let criterion = proposal.node_additions[0].criteria[0].id.clone();
                proposal.node_additions[0].criteria = vec![CriterionDecl::new(
                    criterion,
                    VerificationMethod::HumanApproval,
                )];
            }
            ProposalCase::DuplicateCriterion => {
                let criterion = proposal.node_additions[0].criteria[0].clone();
                proposal.node_additions[0].criteria.push(criterion);
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
            ProposalCase::ForeignNode => {
                proposal.node_additions[0].id =
                    PlanNodeId::new("foreign-task").expect("foreign node id");
            }
            ProposalCase::ParentAsChild => {
                proposal.node_additions[0].id =
                    PlanNodeId::new("TASK-0078").expect("parent node id");
            }
            ProposalCase::BehindPrevious => {
                proposal.edge_additions = vec![EdgeAddition {
                    from: PlanNodeId::new(format!("node-{}", input.base_plan_revision))
                        .expect("previous node id"),
                    to: child_id.clone(),
                }];
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
        baseline_risk: input.baseline_risk,
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
                requirement: None,
            }]
        } else {
            vec![]
        },
        parent_task_ref: "TASK-0078".to_owned(),
        canonical_task_authority: false,
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
        baseline_risk: RiskClass::Low,
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

fn seed_unrelated_task(db: &TestDb, project: ProjectId, id: &str) {
    let mut tasks = TaskService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("tasks");
    if tasks
        .store()
        .get_project(&project)
        .expect("read project")
        .is_none()
    {
        tasks
            .create_project(&Project::new(project.clone(), "unrelated fixture".to_owned()).unwrap())
            .expect("create project");
    }
    let task = Task::draft(
        TaskId::new(id).expect("task id"),
        project,
        "unrelated task".to_owned(),
    )
    .expect("draft unrelated task");
    tasks.create_task(&task).expect("create unrelated task");
    tasks
        .mark_ready(task.id())
        .expect("mark unrelated task ready");
}

/// Advances the branch head through the product planning path. Each accepted
/// revision materializes its planner child `node-N` as a READY descendant of
/// `TASK-0078`; nothing is pre-seeded under a planner-proposed id.
fn seed_head(db: &TestDb, target: u64) {
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
    let request = human_decision_create(db, task_id, options);
    let mut service = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    service.create(request).expect("create human decision")
}

fn human_decision_create(
    db: &TestDb,
    task_id: &str,
    options: Vec<HumanContinuation>,
) -> HumanDecisionCreate {
    let accepted = db
        .service()
        .current_accepted_plan(&branch())
        .expect("current accepted plan")
        .expect("accepted plan required");
    HumanDecisionCreate {
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
    }
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
fn planner_cannot_capture_foreign_or_unrelated_canonical_tasks_before_publication() {
    let db = TestDb::new("trusted-node-identity");
    let foreign_project = ProjectId::new("P-FOREIGN-IDENTITY").unwrap();
    seed_unrelated_task(&db, foreign_project, "foreign-task");

    let mut service = db.service();
    let foreign = TestPlanner::new(ProposalCase::ForeignNode);
    let outcome = service
        .propose_decomposition(
            &foreign,
            &branch(),
            request("foreign-task-capture"),
            &policy(true),
        )
        .expect("foreign identity is a typed refusal");
    assert!(matches!(
        outcome,
        PlanningProposalOutcome::NotApplicable(PlanApplicabilityRefusal::UntrustedNodeIdentity { ref node })
            if node == "foreign-task"
    ));
    assert_no_plan_persisted(&db);

    // A new service handle must derive the same bounded tree; a durable
    // foreign Task cannot become executable after a restart.
    let restarted = db.service();
    assert!(matches!(
        restarted
            .current_accepted_plan_for_task(&TaskId::new("foreign-task").unwrap())
            .unwrap(),
        app_services::AcceptedTaskPlanResolution::Missing
    ));
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

/// A planner whose assessment call faults in a way that opens a human gate.
struct BlockedAssessmentPlanner;

impl PlannerPort for BlockedAssessmentPlanner {
    fn assess_atomicity(&self, _: AtomicityRequest) -> Result<AtomicityAssessment, PlannerFault> {
        Err(PlannerFault::DepthExhausted)
    }

    fn propose_decomposition(
        &self,
        _: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        Err(PlannerFault::DepthExhausted)
    }

    fn propose_replan(&self, _: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        Err(PlannerFault::DepthExhausted)
    }
}

/// TASK-0116: a blocked assessment opens a human gate. The gate publishes the
/// branch's current planning input, because a durable HumanDecision can only
/// be created, and later resumed, while its bound snapshot is current. Holding
/// assessment's snapshot back from current input must not turn this designed
/// WaitingForHuman outcome into a storage error.
#[test]
fn assessment_planner_fault_opens_a_human_gate_rather_than_failing() {
    let db = TestDb::new("assessment-post-call-gate");
    db.seed_tasks(&["TASK-0078"], true);
    let mut service = db.service();

    let outcome = service
        .assess_atomicity(
            &BlockedAssessmentPlanner,
            &branch(),
            assessment_request("blocked-assessment"),
            &policy(false),
        )
        .expect("a blocked assessment must open a human gate, not fail closed on the input fence");
    match outcome {
        PlanningAssessmentOutcome::WaitingForHuman(decision) => {
            assert_eq!(decision.reason, HumanDecisionReason::PlannerBlocked);
            // The carve-out, pinned: the gate published its bound snapshot as
            // current input, which is what makes the decision resumable.
            assert!(
                service
                    .store()
                    .current_planning_input_matches(
                        &branch(),
                        &decision.precondition.planning_input_snapshot_id,
                        &decision.precondition.planning_input_fingerprint,
                    )
                    .unwrap(),
                "a human gate must leave its bound snapshot as current input, or \
                 the decision it just created could never be resumed"
            );
        }
        other => panic!("expected a human gate, got {other:?}"),
    }
}

/// TASK-0116: assessment answers a question and proposes nothing, so it must
/// not republish the branch's current planning input. Before this, an
/// assessment carrying a different evidence set made the accepted revision
/// fail the publication fence until something reconciled it.
#[test]
fn atomicity_assessment_does_not_de_admit_the_accepted_revision() {
    let db = TestDb::new("assessment-keeps-current-input");
    db.seed_tasks(&["TASK-0078"], true);
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);

    let accepted = match service
        .propose_decomposition(
            &planner,
            &branch(),
            request("proposal-evidence"),
            &policy(false),
        )
        .expect("proposal")
    {
        PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("expected an accepted revision, got {other:?}"),
    };
    assert!(
        service
            .store()
            .current_planning_input_matches(
                &branch(),
                accepted.planning_input_snapshot_id(),
                accepted.planning_input_fingerprint(),
            )
            .unwrap(),
        "the accepted revision starts as the branch's current planning input"
    );

    let outcome = service
        .assess_atomicity(
            &planner,
            &branch(),
            assessment_request("unrelated-question"),
            &policy(false),
        )
        .expect("assessment");
    assert!(matches!(outcome, PlanningAssessmentOutcome::Assessed(_)));

    assert!(
        service
            .store()
            .current_planning_input_matches(
                &branch(),
                accepted.planning_input_snapshot_id(),
                accepted.planning_input_fingerprint(),
            )
            .unwrap(),
        "an assessment with different evidence must not replace the accepted \
         revision's current planning input"
    );
}

/// TASK-0119 non-regression guard for the pre-call gate's claim-mode change.
///
/// The gate now claims its generation without publishing and publishes only
/// with its decision. This test pins the end state that change had to
/// preserve: the gate's bound snapshot is current, which is the only thing
/// that makes the decision resumable.
///
/// It asserts end state only, so it does NOT detect an un-fused gate — it
/// passes unchanged against the pre-TASK-0119 sequence, which reached the same
/// end state through two transactions. Atomicity is covered at the storage
/// boundary by `a_failed_gate_decision_leaves_the_accepted_revision_admissible`.
/// The fused writer is the only way to publish outside a run claim, but a gate
/// that claimed through `ControlRunMode::Proposal` would publish at claim time
/// and reopen the window, so the claim mode is load-bearing — even though this
/// test cannot observe it. Flipping the gate back to a `Proposal` claim leaves
/// every assertion here passing, because that claim publishes the same
/// snapshot the fused writer then upserts idempotently. What this test does
/// catch is a partial revert: a `HumanGate` claim with a separate
/// `create_human_decision_request` publishes nothing and errors out.
#[test]
fn the_pre_call_gate_publishes_the_snapshot_its_decision_is_bound_to() {
    let db = TestDb::new("pre-call-gate-publication");
    db.seed_tasks(&["TASK-0078"], true);
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);

    let accepted = match service
        .propose_decomposition(
            &planner,
            &branch(),
            request("proposal-evidence"),
            &policy(false),
        )
        .expect("proposal")
    {
        PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("expected an accepted revision, got {other:?}"),
    };

    let mut exhausted = request("pre-call-gate");
    exhausted.depth_remaining = 0;
    let decision = match service
        .propose_decomposition(&planner, &branch(), exhausted, &policy(false))
        .expect("depth exhaustion is a bounded outcome")
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected a pre-call human gate, got {other:?}"),
    };
    assert_eq!(decision.reason, HumanDecisionReason::PlannerBlocked);
    assert_eq!(
        planner.call_count(),
        1,
        "the pre-call gate must not spend a planner call"
    );

    assert!(
        service
            .store()
            .current_planning_input_matches(
                &branch(),
                &decision.precondition.planning_input_snapshot_id,
                &decision.precondition.planning_input_fingerprint,
            )
            .unwrap(),
        "the gate must leave its own bound snapshot current, or the decision it \
         just recorded could never be resumed"
    );
    assert!(
        !service
            .store()
            .current_planning_input_matches(
                &branch(),
                accepted.planning_input_snapshot_id(),
                accepted.planning_input_fingerprint(),
            )
            .unwrap(),
        "the gate deliberately takes current input from the accepted revision; \
         that is the designed cost of a resumable decision, and it is paid only \
         when the decision is actually recorded"
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
    use std::ffi::OsString;
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

    /// Explicit provider host environment for the fixture executable, so the
    /// tests do not depend on the runner's ambient login session: containers
    /// commonly have no USER, which the provider requires (TASK-0155).
    fn host_environment(db: &TestDb) -> Vec<(OsString, OsString)> {
        vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), db.dir.clone().into_os_string()),
            ("USER".into(), "runlith-fixture".into()),
        ]
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
        let planner = ClaudePlanner::bind_trusted_host_with_host_environment(
            config,
            workdir,
            db.dir.join("env"),
            host_environment(db),
        )
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
        &db.project_id,
        &["unplanned-task"],
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
            None,
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

/// Planner children are always Task-backed at commit (materialized READY in
/// the revision transaction), so a planner predecessor can no longer lack a
/// canonical Task; the literal `PredecessorWithoutTask` rule is covered by
/// the `plan_graph` unit tests. The reachable service-level form is a
/// predecessor naming a Task the branch cannot own: refused before commit,
/// with neither the plan nor its successor child materialized.
#[test]
fn plan_predecessor_without_owned_canonical_task_is_refused_before_commit() {
    let db = TestDb::new("predecessor-unowned");
    seed_unrelated_task(&db, ProjectId::new("P-FOREIGN-STEP").unwrap(), "step-a");
    let mut orchestration = db.orchestration();
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
            None,
        )
        .expect("applicability refusal is a typed outcome");
    assert!(matches!(
        outcome,
        PlannedOrchestrationOutcome::PlanningStopped(PlanningProposalOutcome::NotApplicable(
            PlanApplicabilityRefusal::UntrustedNodeIdentity { ref node }
        )) if node == "step-a"
    ));
    assert_eq!(executor.calls(), 0);
    assert_no_plan_persisted(&db);
    assert!(db
        .store()
        .get_task(&TaskId::new("step-b").unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn structurally_valid_plan_with_new_child_has_runnable_work_before_commit() {
    let db = TestDb::new("new-runnable-child");
    db.seed_tasks(&["TASK-0078"], true);
    let outcome = db
        .service()
        .propose_decomposition(
            &FakePlanner::for_scenario(FakePlannerScenario::Valid),
            &branch(),
            request("new-runnable-child"),
            &policy(true),
        )
        .expect("new child is accepted only with runnable work");
    let accepted = match outcome {
        PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("new child must produce an accepted runnable plan, got {other:?}"),
    };
    assert_eq!(accepted.revision().revision_seq, 1);
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
    let child = db
        .store()
        .get_task(&TaskId::new("planned-child-0").unwrap())
        .unwrap()
        .expect("accepted plan materializes its child");
    assert_eq!(child.state(), TaskState::Ready);
    assert_eq!(child.parent().map(TaskId::as_str), Some("TASK-0078"));
    assert_eq!(
        task_facts(&db, "planned-child-0")
            .iter()
            .map(|(event_type, from, to, _, _)| (*event_type, *from, *to))
            .collect::<Vec<_>>(),
        vec![
            (
                EventType::TaskCreated,
                Some(TaskState::Draft),
                Some(TaskState::Draft)
            ),
            (
                EventType::MarkedReady,
                Some(TaskState::Draft),
                Some(TaskState::Ready)
            ),
        ]
    );
}

#[test]
fn structurally_valid_plan_without_runnable_task_trips_l4_before_commit() {
    let db = TestDb::new("draft-only-child");
    db.seed_tasks(&["TASK-0078"], true);
    db.seed_tasks(&["planned-child-0"], false);
    let child = TaskId::new("planned-child-0").unwrap();
    let snapshot_before = db.store().get_task(&child).unwrap().unwrap().snapshot();
    let facts_before = task_facts(&db, "planned-child-0");

    let outcome = db
        .service()
        .propose_decomposition(
            &FakePlanner::for_scenario(FakePlannerScenario::Valid),
            &branch(),
            request("draft-only-child"),
            &policy(true),
        )
        .expect("non-runnable existing identity is a typed refusal");
    assert!(
        matches!(
            outcome,
            PlanningProposalOutcome::NotApplicable(
                PlanApplicabilityRefusal::UntrustedNodeIdentity { ref node }
            ) if node == "planned-child-0"
        ),
        "a DRAFT-only canonical Task must never produce Accepted, got {outcome:?}"
    );
    assert_no_plan_persisted(&db);
    let store = db.store();
    assert_eq!(
        store.get_task(&child).unwrap().unwrap().snapshot(),
        snapshot_before
    );
    assert_eq!(task_facts(&db, "planned-child-0"), facts_before);
}

#[test]
fn planning_path_errors_never_follow_a_plan_mutation() {
    // Pre-commit provider failure: Err, head unchanged.
    let db = TestDb::new("err-no-mutation");
    let mut orchestration = db.orchestration();
    seed_into(orchestration.tasks_mut(), &db.project_id, &["other"], true);
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
            None,
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
            None,
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
            None,
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
            None,
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
    seed_into(
        orchestration.tasks_mut(),
        &db.project_id,
        &["unplanned"],
        true,
    );
    let planning = db.service();
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

    let none = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch(),
            &TaskId::new("unplanned").unwrap(),
            ids("NO-PLAN"),
            &executor,
            &evaluator,
            None,
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
            None,
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
fn production_entry_with_multiple_legitimate_current_plans_is_ambiguous_and_non_mutating() {
    let db = TestDb::new("production-entry-ambiguous-plan");
    let task_id = TaskId::new("node-1").unwrap();
    let branch_a = PlanBranchId::new("ambiguous-branch-a").unwrap();
    let branch_b = PlanBranchId::new("ambiguous-branch-b").unwrap();
    let planner = TestPlanner::new(ProposalCase::Valid);

    // Both branches accept the identical proposal: branch B's `node-1`
    // materialization is an exact retry of branch A's child, the only way a
    // planner child is legitimately represented by two current plans.
    for branch in [&branch_a, &branch_b] {
        let accepted = db
            .service()
            .propose_decomposition(&planner, branch, request("ambiguous-plan"), &policy(false))
            .expect("legitimate branch proposal");
        assert!(matches!(accepted, PlanningProposalOutcome::Accepted(_)));
    }

    let execution_id = ExecutionId::new("E-AMBIGUOUS-PLAN").unwrap();
    let attempt_id = AttemptId::new("A-AMBIGUOUS-PLAN").unwrap();
    let mut orchestration = db.orchestration();
    let outcome = orchestration
        .enter_from_current_plan(
            &db.service(),
            &task_id,
            OrchestrationIds {
                execution_id: execution_id.clone(),
                attempt_id: attempt_id.clone(),
            },
        )
        .expect("ambiguity is a typed safe outcome");
    match outcome {
        PlannedExecutionEntryOutcome::AmbiguousAcceptedPlan { mut branches } => {
            branches.sort_by(|left, right| left.as_str().cmp(right.as_str()));
            assert_eq!(branches, vec![branch_a, branch_b]);
        }
        other => panic!("expected ambiguous accepted plan, got {other:?}"),
    }

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
// TASK-0087: trusted child task materialization
// ---------------------------------------------------------------------------

type TaskFact = (
    EventType,
    Option<TaskState>,
    Option<TaskState>,
    String,
    String,
);

/// Journal facts of one Task as (type, from, to, provenance, causation).
fn task_facts(db: &TestDb, id: &str) -> Vec<TaskFact> {
    db.store()
        .list_events_for_task(&TaskId::new(id).unwrap())
        .unwrap()
        .into_iter()
        .map(|event| {
            let fact = event.fact;
            (
                fact.event_type,
                fact.from_state,
                fact.to_state,
                fact.provenance,
                fact.causation_id,
            )
        })
        .collect()
}

fn task_created_count(db: &TestDb) -> usize {
    db.store()
        .list_events_ordered()
        .unwrap()
        .iter()
        .filter(|event| event.fact.event_type == EventType::TaskCreated)
        .count()
}

fn accepted_hash(outcome: &PlanningProposalOutcome) -> String {
    match outcome {
        PlanningProposalOutcome::Accepted(accepted) => accepted.revision().changeset_hash.clone(),
        other => panic!("expected accepted proposal, got {other:?}"),
    }
}

#[test]
fn accepted_decomposition_materializes_planner_child_ready_under_the_parent() {
    let db = TestDb::new("child-materialized-ready");
    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::Valid),
            &branch(),
            request("child-ready"),
            &policy(false),
        )
        .unwrap();
    let hash = accepted_hash(&outcome);

    let child = db
        .store()
        .get_task(&TaskId::new("node-1").unwrap())
        .unwrap()
        .expect("accepted revision materializes its child");
    assert_ne!(child.id().as_str(), "TASK-0078");
    assert_eq!(child.parent().map(TaskId::as_str), Some("TASK-0078"));
    assert_eq!(child.state(), TaskState::Ready);
    let provenance = storage_sqlite::CHILD_TASK_MATERIALIZATION_PROVENANCE.to_owned();
    assert_eq!(
        task_facts(&db, "node-1"),
        vec![
            (
                EventType::TaskCreated,
                Some(TaskState::Draft),
                Some(TaskState::Draft),
                provenance.clone(),
                hash.clone(),
            ),
            (
                EventType::MarkedReady,
                Some(TaskState::Draft),
                Some(TaskState::Ready),
                provenance,
                hash,
            ),
        ]
    );
}

#[test]
fn duplicate_criterion_id_is_a_typed_validation_refusal_before_materialization() {
    let db = TestDb::new("duplicate-criterion-validation");
    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::DuplicateCriterion),
            &branch(),
            request("duplicate-criterion"),
            &policy(false),
        )
        .expect("duplicate criterion is a proposal refusal, not a service error");
    assert!(matches!(
        outcome,
        PlanningProposalOutcome::Refused(ValidationRefusal::Malformed { .. })
    ));
    assert_no_plan_persisted(&db);
}

/// ADR-0030 §6 replaces the ADR-0027 TASK-0096 refusal with real carriage:
/// a criterion whose proof method the v1 Task contract could not carry
/// materializes as an explicit evidence requirement on the child Task. The
/// strengthened obligation arrives as itself — never downgraded to
/// automated-test and never silently dropped.
#[test]
fn materialization_carries_a_strengthened_proof_obligation_as_an_explicit_requirement() {
    let db = TestDb::new("child-method-carried");
    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::NonAutomatedCriterion),
            &branch(),
            request("child-method"),
            &policy(false),
        )
        .expect("a human-approval criterion materializes as carried obligation");
    assert!(matches!(outcome, PlanningProposalOutcome::Accepted(_)));
    let child = db
        .store()
        .get_task(&TaskId::new("node-1").unwrap())
        .unwrap()
        .expect("materialized child exists");
    let facts = child.snapshot();
    let requirement = facts
        .evidence_requirements
        .iter()
        .find(|requirement| requirement.criterion_id == "criterion-child-1")
        .expect("the child criterion carries its requirement");
    assert_eq!(
        requirement.requirement_kind,
        task_domain::RequirementKind::HumanApproval,
        "the human-approval obligation arrives as itself"
    );
    assert_eq!(
        requirement.policy_fingerprint.as_deref(),
        Some("policy:test"),
        "materialization stamps the planning policy fingerprint"
    );
}

#[test]
fn children_behind_non_runnable_work_trip_l4_without_materializing() {
    let db = TestDb::new("child-l4-blocked");
    seed_head(&db, 1);
    // node-1 is CLAIMED: neither runnable nor COMPLETED. The only new work
    // the planner offers waits on it, so the candidate has no runnable node.
    TaskService::open(&db.path, FixedClock(FIXED_MILLIS))
        .unwrap()
        .claim(&TaskId::new("node-1").unwrap())
        .unwrap();

    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::BehindPrevious),
            &branch(),
            request("child-l4-blocked"),
            &policy(false),
        )
        .expect("L4 escalation is typed");
    match outcome {
        PlanningProposalOutcome::WaitingForHuman(decision) => {
            assert_eq!(decision.reason, HumanDecisionReason::PlannerBlocked);
        }
        other => panic!("L4 must never accept a plan without runnable work, got {other:?}"),
    }
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).unwrap(), Some(1));
    assert!(store
        .get_task(&TaskId::new("node-2").unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn child_behind_running_predecessor_trips_l4_without_materializing() {
    let db = TestDb::new("child-l4-running");
    seed_head(&db, 1);
    let node = TaskId::new("node-1").unwrap();
    let mut tasks = TaskService::open(&db.path, FixedClock(FIXED_MILLIS)).unwrap();
    tasks.claim(&node).unwrap();
    tasks.start(&node).unwrap();

    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::BehindPrevious),
            &branch(),
            request("child-l4-running"),
            &policy(false),
        )
        .expect("L4 escalation is typed");
    assert!(matches!(
        outcome,
        PlanningProposalOutcome::WaitingForHuman(decision)
            if decision.reason == HumanDecisionReason::PlannerBlocked
    ));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert!(db
        .store()
        .get_task(&TaskId::new("node-2").unwrap())
        .unwrap()
        .is_none());
}

/// TASK-0125 fixture: admits completion through the trusted gate with
/// durable, subject-bound evidence — one captured observed effect for the
/// concluded attempt, one claim plus a current VERIFIED verdict per
/// criterion, then the admissible transition under the declaring policy
/// fingerprint. Stands in for the capture/evaluation pipeline the product
/// still has to wire (TASK-0125 follow-up).
fn complete_with_captured_evidence(
    tasks: &mut TaskService<FixedClock>,
    task_id: &TaskId,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    policy_fingerprint: &str,
) {
    let task = tasks
        .store()
        .get_task(task_id)
        .expect("task")
        .expect("task");
    let digest = format!("sha256:{}", "a".repeat(64));
    let subject_ref =
        EffectSubjectRef::new(format!("attempt:{}", attempt_id.as_str())).expect("ref");
    let fingerprint = subject_fingerprint(
        EffectKind::ArtifactHash,
        subject_ref.as_str(),
        &[("content", digest.as_str())],
    )
    .expect("fingerprint");
    let effect = tasks
        .store_mut()
        .append_observed_effect(
            subject_ref,
            fingerprint.clone(),
            EffectPayload::ArtifactHash {
                bytes: 3,
                sha256: Sha256Digest::new(digest).expect("digest"),
            },
            "2024-01-01T00:00:00Z",
            EffectProvenance::new(
                execution_id.clone(),
                attempt_id.clone(),
                "runlith.local-observer.v1",
            )
            .expect("provenance"),
        )
        .expect("captured effect");
    for requirement in task.evidence_requirements().to_vec() {
        let claim = tasks
            .store_mut()
            .append_agent_claim(
                task_id,
                &requirement.criterion_id,
                ClaimedOutcome::Satisfied,
                ClaimedSubject {
                    subject_ref: format!("subject:{}", requirement.criterion_id),
                    subject_fingerprint: fingerprint.clone(),
                },
                EvidenceRefs::default(),
                "agent:fixture-builder",
                "2024-01-01T00:00:00Z",
            )
            .expect("claim");
        tasks
            .store_mut()
            .append_claim_verification(
                &claim.claim_id,
                requirement.requirement_kind,
                fingerprint.clone(),
                evaluation::requirement_revision(&requirement),
                requirement.policy_fingerprint.clone(),
                ClaimVerdict::Verified,
                EvidenceRefs::new(vec![], vec![effect.effect_id().as_str().to_owned()], vec![])
                    .expect("refs"),
                None,
                "2024-01-01T00:00:00Z",
            )
            .expect("verdict");
    }
    tasks
        .verification_succeeded_under_policy(
            task_id,
            "verified with durable captured evidence",
            Some(policy_fingerprint),
        )
        .expect("evidence-backed completion is admitted");
}

/// TASK-0125: a completion attempt refused by the admission gate — the
/// task stays VERIFYING with typed per-criterion outcomes.
fn assert_completion_admission_refusal(error: ServiceError) {
    match error {
        ServiceError::Domain(DomainError::InvalidTransition(reason)) => {
            assert!(
                reason.contains("completion admission refused"),
                "expected a completion-admission refusal, got {reason:?}"
            );
            assert!(reason.contains("UNVERIFIED"), "got {reason:?}");
        }
        other => panic!("expected a completion-admission refusal, got {other:?}"),
    }
}

#[test]
fn child_behind_completed_predecessor_is_runnable_and_can_be_accepted() {
    let db = TestDb::new("child-l4-completed");
    seed_head(&db, 1);
    let node = TaskId::new("node-1").unwrap();
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    // TASK-0125: the independent-evaluation verdict alone is no longer
    // completion authority. An evaluator/agent claim without durable,
    // subject-bound evidence is refused and the Task stays VERIFYING.
    let refused = db
        .orchestration()
        .schedule(
            &node,
            ids("complete-predecessor"),
            &executor,
            &evaluator,
            None,
        )
        .expect_err("a claim without durable captured evidence must not complete");
    assert_completion_admission_refusal(refused);
    assert_eq!(
        db.store().get_task(&node).unwrap().unwrap().state(),
        TaskState::Verifying
    );
    // The trusted boundary admits completion once the plan-materialized
    // criterion holds a current VERIFIED claim verification bound to a
    // captured observed effect (the fixture stands in for capture).
    let mut tasks = TaskService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("tasks");
    complete_with_captured_evidence(
        &mut tasks,
        &node,
        &ExecutionId::new("E-complete-predecessor").unwrap(),
        &AttemptId::new("A-complete-predecessor").unwrap(),
        "policy:test",
    );
    drop(tasks);
    assert_eq!(
        db.store().get_task(&node).unwrap().unwrap().state(),
        TaskState::Completed
    );

    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::BehindPrevious),
            &branch(),
            request("child-l4-completed"),
            &policy(false),
        )
        .expect("completed predecessor permits runnable successor");
    assert!(matches!(outcome, PlanningProposalOutcome::Accepted(_)));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(2));
    assert_eq!(
        db.store()
            .get_task(&TaskId::new("node-2").unwrap())
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Ready
    );
}

#[test]
fn planner_cannot_materialize_the_parent_task_as_a_child() {
    let db = TestDb::new("parent-as-child");
    let parent = TaskId::new("TASK-0078").unwrap();
    let before = db.store().get_task(&parent).unwrap().expect("parent");
    let created_before = task_created_count(&db);

    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::ParentAsChild),
            &branch(),
            request("parent-as-child"),
            &policy(false),
        )
        .expect("parent reuse is a typed refusal");
    assert!(
        matches!(
            outcome,
            PlanningProposalOutcome::NotApplicable(PlanApplicabilityRefusal::UntrustedNodeIdentity { ref node })
                if node == "TASK-0078"
        ),
        "{outcome:?}"
    );
    assert_no_plan_persisted(&db);
    let store = db.store();
    assert_eq!(
        store.get_task(&parent).unwrap().unwrap().snapshot(),
        before.snapshot()
    );
    assert_eq!(task_created_count(&db), created_before);
    assert_eq!(
        store.list_tasks_for_project(&db.project_id).unwrap().len(),
        1,
        "no child task was created"
    );
}

#[test]
fn exact_child_materialization_retry_reuses_the_existing_task() {
    let db = TestDb::new_unseeded("child-exact-retry");
    let planner = TestPlanner::new(ProposalCase::Valid);
    let child = TaskId::new("node-1").unwrap();
    let mut legacy_request = request("child-retry");
    legacy_request.parent_task_ref = "TASK-0078".to_owned();
    legacy_request.base_plan_revision = 0;
    legacy_request.acceptance_criteria_refs.clear();
    legacy_request.scope_bound = ScopeBound::new(vec!["repo:src".to_owned()]).unwrap();
    legacy_request.baseline_risk = RiskClass::Low;
    let hash = planner.proposal(&legacy_request).proposal_hash;
    let trigger_key = planning_evidence_key(&[evidence("child-retry")]);
    let input_fingerprint = format!("sha256:{}", "a".repeat(64));
    let decomposition_fingerprint = format!("sha256:{}", "b".repeat(64));

    // Build an actual schema-v16 database from the checked-in historical
    // migrations. The existing child, its exact lifecycle pair, the accepted
    // plan state and its planning-input binding all predate migrations 0017
    // and 0018; no current-schema rows are authored and then reverse-engineered.
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        for migration in MIGRATIONS_THROUGH_16 {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute(
                "INSERT INTO projects (id, name) VALUES (?1, 'planning fixture')",
                rusqlite::params![db.project_id.as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tasks (id, project_id, objective, state)
                 VALUES ('TASK-0078', ?1, 'planning root', 'READY')",
                rusqlite::params![db.project_id.as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tasks (id, project_id, objective, parent_id, state, risk)
                 VALUES ('node-1', ?1, 'planned child task node-1', 'TASK-0078', 'READY', 'low')",
                rusqlite::params![db.project_id.as_str()],
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO task_strings (task_id, slot, position, value) VALUES
                     ('node-1', 'allowed_path', 0, 'repo:src'),
                     ('node-1', 'acceptance', 0, 'criterion-child-1');",
            )
            .unwrap();
        for (event_type, to_state, reason) in [
            ("task.created", "DRAFT", Some("plan child materialization")),
            ("task.marked_ready", "READY", None),
        ] {
            connection
                .execute(
                    "INSERT INTO event_journal
                     (event_type, event_version, task_id, project_id, from_state, to_state,
                      reason, correlation_id, causation_id, occurred_at, provenance)
                     VALUES (?1, 1, 'node-1', ?2, 'DRAFT', ?3, ?4,
                             'plan-child:node-1', ?5, '2024-01-01T00:00:00Z', ?6)",
                    rusqlite::params![
                        event_type,
                        db.project_id.as_str(),
                        to_state,
                        reason,
                        hash,
                        storage_sqlite::CHILD_TASK_MATERIALIZATION_PROVENANCE,
                    ],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO plan_cost_receipts
                 (receipt_ref, input_tokens, output_tokens, wall_time_ms, price_class)
                 VALUES ('legacy-receipt', 1, 1, 1, 'test')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_branches (plan_branch_id, head_revision)
                 VALUES (?1, 1)",
                rusqlite::params![branch().as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_revisions
                 (plan_branch_id, revision_seq, base_revision, changeset_hash, reason,
                  planner_adapter_id, planner_model_ref, planning_cost_receipt_ref,
                  validated_service_id, policy_fingerprint, created_at_ms)
                 VALUES (?1, 1, 0, ?2, 'legacy accepted proposal', 'fixture-planner',
                         'fixture-model', 'legacy-receipt', 'planning-service', 'policy:test', ?3)",
                rusqlite::params![branch().as_str(), hash, FIXED_MILLIS as i64],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_revision_causes
                 (plan_branch_id, revision_seq, position, evidence_ref)
                 VALUES (?1, 1, 0, 'child-retry')",
                rusqlite::params![branch().as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_nodes
                 (plan_branch_id, revision_seq, position, node_id, risk_class)
                 VALUES (?1, 1, 0, 'node-1', 'low')",
                rusqlite::params![branch().as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_node_scopes
                 (plan_branch_id, revision_seq, node_id, position, scope_root)
                 VALUES (?1, 1, 'node-1', 0, 'repo:src')",
                rusqlite::params![branch().as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_node_criteria
                 (plan_branch_id, revision_seq, node_id, position, criterion_id, verification_method)
                 VALUES (?1, 1, 'node-1', 0, 'criterion-child-1', 'automated-test')",
                rusqlite::params![branch().as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_input_snapshots
                 (snapshot_id, plan_branch_id, task_ref, expected_plan_head, baseline_risk,
                  policy_fingerprint, input_fingerprint, created_at_ms)
                 VALUES ('legacy-snapshot', ?1, 'TASK-0078', 0, 'low', 'policy:test', ?2, ?3)",
                rusqlite::params![branch().as_str(), input_fingerprint, FIXED_MILLIS as i64],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_input_scope_roots (snapshot_id, position, scope_root)
                 VALUES ('legacy-snapshot', 0, 'repo:src')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_input_evidence (snapshot_id, position, evidence_ref)
                 VALUES ('legacy-snapshot', 0, 'child-retry')",
                [],
            )
            .unwrap();
        for (position, task_ref) in ["TASK-0078", "node-1"].into_iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO planning_input_candidate_tasks (snapshot_id, position, task_ref)
                     VALUES ('legacy-snapshot', ?1, ?2)",
                    rusqlite::params![position as i64, task_ref],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO planning_input_heads (plan_branch_id, snapshot_id, input_fingerprint)
                 VALUES (?1, 'legacy-snapshot', ?2)",
                rusqlite::params![branch().as_str(), input_fingerprint],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_runs
                 (planning_run_id, plan_branch_id, generation, snapshot_id, input_fingerprint,
                  expected_plan_head, trigger_evidence_key, state, created_at_ms, finished_at_ms)
                 VALUES ('legacy-run', ?1, 1, 'legacy-snapshot', ?2, 0, ?3, 'SUCCEEDED', ?4, ?4)",
                rusqlite::params![
                    branch().as_str(),
                    input_fingerprint,
                    trigger_key,
                    FIXED_MILLIS as i64,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_run_heads (plan_branch_id, planning_run_id, generation)
                 VALUES (?1, 'legacy-run', 1)",
                rusqlite::params![branch().as_str()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO plan_revision_lineage
                 (plan_branch_id, revision_seq, parent_task_ref, decomposition_fingerprint,
                  trigger_evidence_key, planning_input_snapshot_id, planning_input_fingerprint,
                  planning_run_id, planning_run_generation)
                 VALUES (?1, 1, 'TASK-0078', ?2, ?3, 'legacy-snapshot', ?4, 'legacy-run', 1)",
                rusqlite::params![
                    branch().as_str(),
                    decomposition_fingerprint,
                    trigger_key,
                    input_fingerprint,
                ],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 16).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&db.path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let migrated = SqliteStore::open(&db.path).expect("migrate legacy child through open");
    assert_eq!(
        migrated
            .get_task(&child)
            .unwrap()
            .unwrap()
            .evidence_requirements()[0]
            .policy_fingerprint,
        None
    );
    let facts = task_facts(&db, "node-1");
    drop(migrated);

    // Same child id, provenance, causation (proposal hash) and snapshot.
    let retry_branch = PlanBranchId::new("child-retry-branch").unwrap();
    let retry = db
        .service()
        .propose_decomposition(
            &planner,
            &retry_branch,
            request("child-retry"),
            &policy(false),
        )
        .unwrap();
    assert_eq!(accepted_hash(&retry), hash);

    let store = db.store();
    assert_eq!(store.get_plan_head(&retry_branch).unwrap(), Some(1));
    assert_eq!(
        task_facts(&db, "node-1"),
        facts,
        "reused, never re-journaled"
    );
    assert_eq!(
        store.list_tasks_for_project(&db.project_id).unwrap().len(),
        2,
        "parent plus exactly one child row"
    );
    assert_eq!(
        store
            .get_task(&child)
            .unwrap()
            .unwrap()
            .evidence_requirements()[0]
            .policy_fingerprint
            .as_deref(),
        Some("policy:test")
    );
}

#[test]
fn foreign_provenance_child_collision_is_refused_without_partial_state() {
    let db = TestDb::new("child-foreign-provenance");
    // node-1 already exists under the same parent but was authored through
    // TaskService: a different creator than plan child materialization.
    db.seed_tasks(&["node-1"], true);
    let child = TaskId::new("node-1").unwrap();
    let snapshot = db.store().get_task(&child).unwrap().unwrap().snapshot();
    let facts = task_facts(&db, "node-1");
    let created_before = task_created_count(&db);

    let outcome = db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::Valid),
            &branch(),
            request("child-foreign"),
            &policy(false),
        )
        .expect("foreign collision is a typed refusal");
    assert!(
        matches!(
            outcome,
            PlanningProposalOutcome::NotApplicable(PlanApplicabilityRefusal::UntrustedNodeIdentity { ref node })
                if node == "node-1"
        ),
        "{outcome:?}"
    );
    assert_no_plan_persisted(&db);
    assert_eq!(
        db.store().get_task(&child).unwrap().unwrap().snapshot(),
        snapshot
    );
    assert_eq!(task_facts(&db, "node-1"), facts);
    assert_eq!(task_created_count(&db), created_before);
}

#[test]
fn child_snapshot_mismatch_fails_closed_in_storage_with_full_rollback() {
    let db = TestDb::new("child-snapshot-mismatch");
    let planner = TestPlanner::new(ProposalCase::Valid);
    let child = TaskId::new("node-1").unwrap();
    accepted_hash(
        &db.service()
            .propose_decomposition(&planner, &branch(), request("child-drift"), &policy(false))
            .unwrap(),
    );
    // Provenance and causation still match the identical proposal, but the
    // stored snapshot moved on: only storage can see the mismatch.
    TaskService::open(&db.path, FixedClock(FIXED_MILLIS))
        .unwrap()
        .claim(&child)
        .unwrap();
    let snapshot = db.store().get_task(&child).unwrap().unwrap().snapshot();
    let facts = task_facts(&db, "node-1");

    let drift_branch = PlanBranchId::new("child-drift-branch").unwrap();
    let error = db
        .service()
        .propose_decomposition(
            &planner,
            &drift_branch,
            request("child-drift"),
            &policy(false),
        )
        .expect_err("storage refuses a drifted child snapshot");
    assert!(
        matches!(
            error,
            PlanningServiceError::Storage(StorageError::ConstraintViolation(_))
        ),
        "{error:?}"
    );
    let store = db.store();
    assert_eq!(store.get_plan_head(&drift_branch).unwrap(), None);
    assert!(store.get_plan_lineage(&drift_branch).unwrap().is_empty());
    assert_eq!(
        store.get_task(&child).unwrap().unwrap().snapshot(),
        snapshot
    );
    assert_eq!(task_facts(&db, "node-1"), facts);
}

// ---------------------------------------------------------------------------
// TASK-0079: bounded durable Human Decision Queue
// ---------------------------------------------------------------------------

#[test]
fn risk_decision_survives_restart_and_resume_is_consumed_with_plan_commit() {
    let db = TestDb::new("human-risk-resume");
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

    let approved = db
        .store()
        .get_approved_human_decision_proposal(&decision.id)
        .unwrap()
        .expect("risk decision stores the exact approved proposal");
    let mut replay_with_different_receipt = approved.changeset.clone();
    replay_with_different_receipt.planning_cost_receipt =
        PlanningCostReceipt::new(100, 50, 777, "test".to_owned()).unwrap();
    assert_ne!(
        compute_proposal_hash(&replay_with_different_receipt),
        decision.proposal_hash.as_deref().expect("proposal hash"),
        "measured receipt variation changes the proposal hash; RESUME must use the durable approved proposal"
    );

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
    drop(human);

    // RESUME rehydrates the exact approved immutable proposal and original
    // measured receipt. There is no second PlannerPort call; the trusted
    // service creates only the grant-bearing control snapshot/run and then
    // revalidates the stored proposal immediately before atomic publication.
    let accepted = db
        .service()
        .resume_approved_risk_proposal(&decision.id, &policy(false))
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
    assert_eq!(
        planner.call_count(),
        1,
        "V9 RESUME must publish the approved changeset without invoking PlannerPort again"
    );

    let replay = db
        .service()
        .resume_approved_risk_proposal(&decision.id, &policy(false))
        .expect_err("consumed V9 RESUME cannot be applied twice");
    assert!(matches!(
        replay,
        PlanningServiceError::Storage(StorageError::HumanDecisionStale { .. })
            | PlanningServiceError::Storage(StorageError::HumanDecisionState { .. })
    ));
    assert_eq!(
        planner.call_count(),
        1,
        "duplicate RESUME must not gain a PlannerPort call"
    );

    let audit = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", decision.id.as_str()))
        .unwrap();
    let events: Vec<_> = audit.iter().map(|record| record.fact.event).collect();
    assert_eq!(
        events,
        vec![
            SecurityAuditEvent::HumanDecisionRequested,
            SecurityAuditEvent::HumanDecisionDecided,
            SecurityAuditEvent::HumanDecisionApplied,
        ],
        "request, operator decision and atomic application remain distinct audit facts"
    );
}

#[test]
fn corrupted_persisted_approved_proposal_fails_closed_before_publication_or_consumption() {
    let db = TestDb::new("human-risk-corrupt-approved-proposal");
    let decision = match db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::RiskEscalation),
            &branch(),
            request("risk-corrupt-approved"),
            &policy(false),
        )
        .unwrap()
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected risk decision, got {other:?}"),
    };
    HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .unwrap();

    {
        let raw = rusqlite::Connection::open(&db.path).unwrap();
        let mut blob: Vec<u8> = raw
            .query_row(
                "SELECT approved_changeset FROM human_decision_requests WHERE request_id = ?1",
                rusqlite::params![decision.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let needle = b"deterministic planning-service proposal";
        let offset = blob
            .windows(needle.len())
            .position(|window| window == needle)
            .expect("encoded reason bytes");
        blob[offset] = b'e';
        raw.execute(
            "UPDATE human_decision_requests SET approved_changeset = ?1 WHERE request_id = ?2",
            rusqlite::params![blob, decision.id.as_str()],
        )
        .unwrap();
    }

    let error = db
        .service()
        .resume_approved_risk_proposal(&decision.id, &policy(false))
        .expect_err("hash-mismatched approved proposal must fail closed");
    assert!(matches!(
        error,
        PlanningServiceError::Storage(StorageError::Corrupt(ref detail))
            if detail.contains("proposal_hash")
    ));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), None);
    assert_eq!(
        HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
            .get(&decision.id)
            .unwrap()
            .unwrap()
            .status,
        HumanDecisionStatus::Resolved,
        "corruption must be refused before once-only authority is consumed"
    );
}

#[test]
fn approved_proposal_trailing_bytes_fail_closed_before_publication_or_consumption() {
    let db = TestDb::new("human-risk-approved-trailing-bytes");
    let decision = match db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::RiskEscalation),
            &branch(),
            request("risk-approved-trailing"),
            &policy(false),
        )
        .unwrap()
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected risk decision, got {other:?}"),
    };
    HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .unwrap();
    {
        let raw = rusqlite::Connection::open(&db.path).unwrap();
        let mut blob: Vec<u8> = raw
            .query_row(
                "SELECT approved_changeset FROM human_decision_requests WHERE request_id = ?1",
                rusqlite::params![decision.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        blob.extend_from_slice(b"trailing-data");
        raw.execute(
            "UPDATE human_decision_requests SET approved_changeset = ?1 WHERE request_id = ?2",
            rusqlite::params![blob, decision.id.as_str()],
        )
        .unwrap();
    }

    let error = db
        .service()
        .resume_approved_risk_proposal(&decision.id, &policy(false))
        .expect_err("trailing approved proposal bytes must fail closed");
    assert!(matches!(
        error,
        PlanningServiceError::Storage(StorageError::Corrupt(ref detail))
            if detail.contains("trailing bytes")
    ));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), None);
    assert_eq!(
        HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
            .get(&decision.id)
            .unwrap()
            .unwrap()
            .status,
        HumanDecisionStatus::Resolved
    );
}

#[test]
fn concurrent_v9_resume_publishes_and_consumes_exactly_once() {
    let db = TestDb::new("human-risk-concurrent-resume");
    let decision = match db
        .service()
        .propose_decomposition(
            &TestPlanner::new(ProposalCase::RiskEscalation),
            &branch(),
            request("risk-concurrent-resume"),
            &policy(false),
        )
        .unwrap()
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected risk decision, got {other:?}"),
    };
    HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .unwrap();

    let participants = 4;
    let barrier = Arc::new(Barrier::new(participants));
    let mut handles = Vec::new();
    for _ in 0..participants {
        let path = db.path.clone();
        let id = decision.id.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let mut service =
                PlanningService::open(path, FixedClock(FIXED_MILLIS)).expect("concurrent service");
            barrier.wait();
            service.resume_approved_risk_proposal(&id, &policy(false))
        }));
    }
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().expect("concurrent RESUME thread"))
        .collect::<Vec<_>>();
    let accepted = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Ok(PlanningProposalOutcome::Accepted(_))))
        .count();
    assert_eq!(
        accepted, 1,
        "V9 authority must publish exactly once: {outcomes:?}"
    );
    for outcome in outcomes
        .iter()
        .filter(|outcome| !matches!(outcome, Ok(PlanningProposalOutcome::Accepted(_))))
    {
        assert!(
            matches!(
                outcome,
                Err(PlanningServiceError::Storage(
                    StorageError::HumanDecisionState { .. }
                )) | Err(PlanningServiceError::Storage(
                    StorageError::HumanDecisionStale { .. }
                )) | Err(PlanningServiceError::Storage(
                    StorageError::PlanningRunStale { .. }
                )) | Err(PlanningServiceError::Storage(
                    StorageError::PlanningInputStale { .. }
                ))
            ),
            "losing V9 consumer must receive a typed stale/consumed refusal: {outcome:?}"
        );
    }

    let restarted = PlanningService::open(&db.path, FixedClock(FIXED_MILLIS))
        .expect("restart after concurrent RESUME");
    assert_eq!(restarted.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert_eq!(
        restarted.store().get_plan_lineage(&branch()).unwrap().len(),
        1
    );
    assert_eq!(
        HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
            .get(&decision.id)
            .unwrap()
            .unwrap()
            .status,
        HumanDecisionStatus::Consumed
    );
}

#[test]
fn expired_risk_decision_is_superseded_once_and_replaced_after_restart() {
    let db = TestDb::new("human-risk-expiry-replacement");
    let planner = TestPlanner::new(ProposalCase::RiskEscalation);

    let first = match db
        .service()
        .propose_decomposition(&planner, &branch(), request("risk-expiry"), &policy(false))
        .unwrap()
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected first risk decision, got {other:?}"),
    };
    assert_eq!(planner.call_count(), 1);

    let after_expiry = FIXED_MILLIS + (31 * 60 * 1000);
    let mut restarted =
        PlanningService::open(&db.path, FixedClock(after_expiry)).expect("restart after TTL");
    let second = match restarted
        .propose_decomposition(&planner, &branch(), request("risk-expiry"), &policy(false))
        .expect("expired decision is superseded through trusted planning")
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected replacement risk decision, got {other:?}"),
    };
    assert_ne!(first.id, second.id);
    assert_eq!(planner.call_count(), 2);

    let first_persisted = HumanDecisionService::attach(db.store(), FixedClock(after_expiry))
        .get(&first.id)
        .unwrap()
        .expect("superseded decision remains durable");
    assert_eq!(first_persisted.status, HumanDecisionStatus::Superseded);
    let first_audit = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", first.id.as_str()))
        .unwrap();
    assert_eq!(
        first_audit
            .iter()
            .map(|record| record.fact.event)
            .collect::<Vec<_>>(),
        vec![
            SecurityAuditEvent::HumanDecisionRequested,
            SecurityAuditEvent::HumanDecisionSuperseded,
        ]
    );

    // A second restart sees the replacement as the one current blocker. It
    // must not churn requests or call the planner again while that request is
    // still current.
    let mut restarted_again =
        PlanningService::open(&db.path, FixedClock(after_expiry)).expect("second restart");
    let repeated = restarted_again
        .propose_decomposition(&planner, &branch(), request("risk-expiry"), &policy(false))
        .unwrap();
    assert!(matches!(
        repeated,
        PlanningProposalOutcome::WaitingForHuman(ref current) if current.id == second.id
    ));
    assert_eq!(planner.call_count(), 2);
}

#[test]
fn replacement_supersession_is_audited_for_pending_and_resolved_chain_and_survives_restart() {
    let db = TestDb::new("human-replacement-audit-chain");
    seed_head(&db, 1);
    let options = vec![HumanContinuation::Resume, HumanContinuation::Replan];

    let first = create_human_decision(&db, "TASK-0078", options.clone());
    let second = create_human_decision(&db, "TASK-0078", options.clone());
    assert_eq!(
        HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
            .get(&first.id)
            .unwrap()
            .unwrap()
            .status,
        HumanDecisionStatus::Superseded
    );
    let first_events = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", first.id.as_str()))
        .unwrap()
        .into_iter()
        .map(|record| record.fact.event)
        .collect::<Vec<_>>();
    assert_eq!(
        first_events,
        vec![
            SecurityAuditEvent::HumanDecisionRequested,
            SecurityAuditEvent::HumanDecisionSuperseded,
        ],
        "PENDING predecessor replacement must have authoritative supersession evidence"
    );

    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    human
        .decide(&second.id, HumanContinuation::Replan, "operator:test")
        .unwrap();
    drop(human);
    let third = create_human_decision(&db, "TASK-0078", options);
    let second_events = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", second.id.as_str()))
        .unwrap()
        .into_iter()
        .map(|record| record.fact.event)
        .collect::<Vec<_>>();
    assert_eq!(
        second_events,
        vec![
            SecurityAuditEvent::HumanDecisionRequested,
            SecurityAuditEvent::HumanDecisionDecided,
            SecurityAuditEvent::HumanDecisionSuperseded,
        ],
        "RESOLVED predecessor replacement must have authoritative supersession evidence"
    );

    let mut restarted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    assert_eq!(
        restarted.get(&first.id).unwrap().unwrap().status,
        HumanDecisionStatus::Superseded
    );
    assert_eq!(
        restarted.get(&second.id).unwrap().unwrap().status,
        HumanDecisionStatus::Superseded
    );
    assert_eq!(
        restarted.get(&third.id).unwrap().unwrap().status,
        HumanDecisionStatus::Pending
    );
    let unusable = restarted
        .decide(&first.id, HumanContinuation::Resume, "operator:stale")
        .expect_err("superseded decision must remain unusable");
    assert!(matches!(
        unusable,
        app_services::ServiceError::Storage(StorageError::HumanDecisionState { .. })
    ));
    drop(restarted);

    // Later replacement scans must not manufacture a second supersession
    // fact for rows that are already SUPERSEDED.
    let _fourth = create_human_decision(
        &db,
        "TASK-0078",
        vec![HumanContinuation::Resume, HumanContinuation::Replan],
    );
    let first_superseded_count = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", first.id.as_str()))
        .unwrap()
        .into_iter()
        .filter(|record| record.fact.event == SecurityAuditEvent::HumanDecisionSuperseded)
        .count();
    assert_eq!(first_superseded_count, 1);
}

#[test]
fn replacement_audit_failure_rolls_back_supersession_and_new_request() {
    let db = TestDb::new("human-replacement-audit-rollback");
    seed_head(&db, 1);
    let first = create_human_decision(
        &db,
        "TASK-0078",
        vec![HumanContinuation::Resume, HumanContinuation::Replan],
    );

    {
        let raw = rusqlite::Connection::open(&db.path).unwrap();
        raw.execute_batch(
            "CREATE TRIGGER test_fail_human_supersession_audit
             BEFORE INSERT ON security_audit
             WHEN NEW.event_type = 'human_decision.superseded'
             BEGIN
               SELECT RAISE(ABORT, 'forced human decision audit failure');
             END;",
        )
        .unwrap();
    }
    let mut service = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    let error = service
        .create(human_decision_create(
            &db,
            "TASK-0078",
            vec![HumanContinuation::Resume, HumanContinuation::Replan],
        ))
        .expect_err("audit failure must abort replacement transaction");
    assert!(matches!(error, app_services::ServiceError::Storage(_)));
    drop(service);
    {
        let raw = rusqlite::Connection::open(&db.path).unwrap();
        raw.execute_batch("DROP TRIGGER test_fail_human_supersession_audit")
            .unwrap();
    }

    let restarted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    assert_eq!(
        restarted.get(&first.id).unwrap().unwrap().status,
        HumanDecisionStatus::Pending,
        "predecessor mutation must roll back with audit failure"
    );
    assert_eq!(restarted.list_pending().unwrap(), vec![first.clone()]);
    let events = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", first.id.as_str()))
        .unwrap()
        .into_iter()
        .map(|record| record.fact.event)
        .collect::<Vec<_>>();
    assert_eq!(events, vec![SecurityAuditEvent::HumanDecisionRequested]);
}

#[test]
fn resolved_risk_resume_supersedes_when_snapshot_changes_before_application() {
    let db = TestDb::new("human-risk-resume-stale-snapshot");
    let planner = TestPlanner::new(ProposalCase::RiskEscalation);
    let decision = match db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("risk-resume-stale"),
            &policy(false),
        )
        .unwrap()
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected risk decision, got {other:?}"),
    };
    HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .unwrap();

    // Simulate a distinct trusted planning operation moving only the current
    // PlanningInputSnapshot while the Plan head remains at zero.
    let replacement_evidence = vec![evidence("risk-resume-new-snapshot")];
    let replacement = PlanningInputSnapshot::new_with_candidates(
        branch(),
        "TASK-0078".to_owned(),
        vec!["TASK-0078".to_owned()],
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        replacement_evidence.clone(),
        vec![],
        None,
        "policy:test".to_owned(),
    )
    .unwrap();
    db.store()
        .start_planning_run(
            &replacement,
            &planning_evidence_key(&replacement_evidence),
            FIXED_MILLIS + 1,
        )
        .unwrap();

    let error = db
        .service()
        .resume_approved_risk_proposal(&decision.id, &policy(false))
        .expect_err("stale approved proposal must fail before publication");
    assert!(matches!(
        error,
        PlanningServiceError::Storage(StorageError::HumanDecisionStale { .. })
    ));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), None);
    assert_eq!(planner.call_count(), 1);

    let persisted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .get(&decision.id)
        .unwrap()
        .unwrap();
    assert_eq!(persisted.status, HumanDecisionStatus::Superseded);
    let events = db
        .store()
        .list_security_audit_by_correlation(&format!("human-decision:{}", decision.id.as_str()))
        .unwrap()
        .into_iter()
        .map(|record| record.fact.event)
        .collect::<Vec<_>>();
    assert_eq!(
        events,
        vec![
            SecurityAuditEvent::HumanDecisionRequested,
            SecurityAuditEvent::HumanDecisionDecided,
            SecurityAuditEvent::HumanDecisionSuperseded,
        ]
    );
}

#[test]
fn risk_decision_replan_uses_original_policy_input_and_normal_g7_boundary() {
    let db = TestDb::new("human-risk-replan");
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

#[test]
fn failed_replan_is_superseded_without_erasing_decision_evidence() {
    let db = TestDb::new("human-replan-failure-recovery");
    seed_head(&db, 1);
    db.seed_tasks(&["TASK-0078"], true);
    let decision = create_human_decision(&db, "TASK-0078", vec![HumanContinuation::Replan]);
    let mut human = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS));
    human
        .decide(&decision.id, HumanContinuation::Replan, "operator:test")
        .unwrap();

    let mut planning = db.service();
    let refusal = human
        .apply_replan(
            &mut planning,
            &TestPlanner::new(ProposalCase::InvalidScope),
            &decision.id,
            &policy(false),
        )
        .expect("a refused continuation is an honest planning result");
    assert!(matches!(
        refusal,
        PlanningProposalOutcome::Refused(_) | PlanningProposalOutcome::NotApplicable(_)
    ));

    let persisted = human
        .get(&decision.id)
        .unwrap()
        .expect("superseded decision remains durable evidence");
    assert_eq!(persisted.status, HumanDecisionStatus::Superseded);
    assert_eq!(persisted.selected, Some(HumanContinuation::Replan));
    assert_eq!(persisted.decided_by.as_deref(), Some("operator:test"));
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
}
