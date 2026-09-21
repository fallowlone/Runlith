//! TASK-0084 canonical Task authority publication fence.
//!
//! `PlannerPort` runs outside every SQLite transaction. Canonical Task facts
//! captured by the `PlanningInputSnapshot` (scope, acceptance, baseline risk
//! and the trusted candidate Task set) must still be current inside the
//! transaction that publishes the resulting `PlanRevision`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use app_services::{
    FixedClock, HumanDecisionService, PlanningPolicyContext, PlanningProposalOutcome,
    PlanningService, PlanningServiceError, TaskService,
};
use fake_planner::FakePlanner;
use plan_domain::{
    AtomicityAssessment, AtomicityRequest, DecompositionRequest, EvidenceRef, HumanContinuation,
    HumanDecisionStatus, PlanBranchId, PlanChangeSet, PlanNodeId, PlannerFault, PlannerPort,
    PlanningBudget, ReplanRequest, RiskClass, RiskDelta, ScopeBound,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};
use storage_sqlite::{EventType, SqliteStore, StorageError};
use task_domain::{EvidenceRequirement, Project, ProjectId, Task, TaskId};

const FIXED_MILLIS: u64 = 1_704_067_200_000;
const ROOT: &str = "T-AUTHORITY";
const CHILD: &str = "T-AUTHORITY-CHILD";
/// The planner-proposed child: a new identity, never the parent or `CHILD`.
const LEAF: &str = "T-AUTHORITY-LEAF";
const POLICY: &str = "policy:task-authority";

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
    project: ProjectId,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-task-authority-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test directory");
        let db = Self {
            path: dir.join("runlith.db"),
            dir,
            project: ProjectId::new("P-AUTHORITY").expect("project id"),
        };
        let mut tasks = db.tasks();
        tasks
            .create_project(&Project::new(db.project.clone(), "authority").unwrap())
            .expect("create project");
        tasks
            .create_planning_task(
                task(ROOT),
                db.project.clone(),
                "planning root",
                vec!["repo:src".to_owned()],
                vec!["criterion-root".to_owned()],
                "low",
            )
            .expect("author canonical planning task");
        tasks.mark_ready(&task(ROOT)).expect("root ready");
        let mut child = Task::draft(task(CHILD), db.project.clone(), "existing child").unwrap();
        child.set_parent(task(ROOT)).unwrap();
        tasks.create_task(&child).expect("create child");
        db
    }

    fn tasks(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("task service")
    }

    fn service(&self) -> PlanningService<FixedClock> {
        PlanningService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("planning service")
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("store")
    }

    fn canonical_policy(&self) -> PlanningPolicyContext {
        self.service()
            .canonical_policy_context(&task(ROOT), POLICY)
            .expect("canonical planning policy")
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn task(id: &str) -> TaskId {
    TaskId::new(id).expect("task id")
}

fn branch() -> PlanBranchId {
    PlanBranchId::new("task-authority-branch").expect("branch")
}

fn request(evidence: &str) -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: "caller-ignored".to_owned(),
        base_plan_revision: 0,
        acceptance_criteria_refs: vec![],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).unwrap(),
        evidence_refs: vec![EvidenceRef::new(evidence).unwrap()],
        depth_remaining: 4,
        baseline_risk: RiskClass::Low,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    }
}

fn replan_request(evidence: &str) -> ReplanRequest {
    ReplanRequest {
        branch_root_ref: "caller-ignored".to_owned(),
        base_plan_revision: 0,
        triggering_evidence_refs: vec![EvidenceRef::new(evidence).unwrap()],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).unwrap(),
        depth_remaining: 4,
        baseline_risk: RiskClass::Low,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    }
}

/// Out-of-band canonical Task writes performed while the provider call is in
/// flight. They model a concurrent writer; they are never planning authority.
#[derive(Clone, Copy, Debug)]
enum Drift {
    Scope,
    Acceptance,
    Risk,
    ForbiddenPath,
    DescendantAdded,
    DescendantReparented,
}

fn apply_drift(db_path: &PathBuf, drift: Drift) {
    let mut store = SqliteStore::open(db_path).expect("concurrent writer");
    let target = match drift {
        Drift::DescendantReparented => CHILD,
        _ => ROOT,
    };
    if let Drift::DescendantAdded = drift {
        let mut added = Task::draft(
            task("T-AUTHORITY-LATE"),
            ProjectId::new("P-AUTHORITY").unwrap(),
            "late child",
        )
        .unwrap();
        added.set_parent(task(ROOT)).unwrap();
        store
            .create_task(&added)
            .expect("concurrent child creation");
        return;
    }
    let mut snapshot = store
        .get_task(&task(target))
        .expect("read canonical task")
        .expect("canonical task exists")
        .snapshot();
    match drift {
        Drift::Scope => snapshot.allowed_paths = vec!["repo:elsewhere".to_owned()],
        Drift::Acceptance => {
            snapshot.acceptance.push("criterion-added".to_owned());
            // ADR-0030 §2: acceptance and evidence requirements are one
            // invariant; drift on both stays a well-formed (moved) Task so
            // the fence — not the domain — is what is being tested.
            snapshot
                .evidence_requirements
                .push(EvidenceRequirement::authored_default("criterion-added").unwrap());
        }
        Drift::Risk => snapshot.risk = Some("high".to_owned()),
        Drift::ForbiddenPath => snapshot.forbidden_paths = vec!["repo:src/secret".to_owned()],
        Drift::DescendantReparented => snapshot.parent = None,
        Drift::DescendantAdded => unreachable!("handled above"),
    }
    store
        .save_task(&Task::restore(snapshot).expect("drifted task remains valid"))
        .expect("persist concurrent drift");
}

struct DriftingPlanner {
    db_path: PathBuf,
    drift: Option<Drift>,
    inner: FakePlanner,
}

impl DriftingPlanner {
    fn new(db: &TestDb, drift: Option<Drift>) -> Self {
        Self {
            db_path: db.path.clone(),
            drift,
            inner: FakePlanner::for_single_leaf(PlanNodeId::new(LEAF).unwrap()),
        }
    }

    fn drift(&self) {
        if let Some(drift) = self.drift {
            apply_drift(&self.db_path, drift);
        }
    }
}

impl PlannerPort for DriftingPlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        self.inner.assess_atomicity(input)
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        self.drift();
        self.inner.propose_decomposition(input)
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.drift();
        // Same single-leaf shape as decomposition so the proposal covers the
        // canonical parent criterion and reaches the publication boundary.
        self.inner.propose_decomposition(DecompositionRequest {
            parent_task_ref: input.branch_root_ref,
            base_plan_revision: input.base_plan_revision,
            acceptance_criteria_refs: vec![plan_domain::CriterionId::new("criterion-root").unwrap()],
            scope_bound: input.scope_bound,
            evidence_refs: input.triggering_evidence_refs,
            depth_remaining: input.depth_remaining,
            baseline_risk: input.baseline_risk,
            budget_remaining: input.budget_remaining,
        })
    }
}

fn assert_stale_without_publication(
    db: &TestDb,
    result: Result<PlanningProposalOutcome, PlanningServiceError>,
) {
    assert!(
        matches!(
            result,
            Err(PlanningServiceError::Storage(
                StorageError::PlanningInputStale { .. }
            ))
        ),
        "canonical Task drift must fail closed as stale planning input, got {result:?}"
    );
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).unwrap(), None);
    assert!(store.get_plan_lineage(&branch()).unwrap().is_empty());
    assert!(!store
        .list_events_ordered()
        .unwrap()
        .iter()
        .any(|event| event.fact.event_type == EventType::PlanRevisionCommitted));
}

#[test]
fn unchanged_canonical_authority_publishes_through_the_fence() {
    let db = TestDb::new("unchanged");
    let planner = DriftingPlanner::new(&db, None);
    let outcome = db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("unchanged"),
            &db.canonical_policy(),
        )
        .expect("fresh canonical authority publishes");
    assert!(
        matches!(outcome, PlanningProposalOutcome::Accepted(_)),
        "{outcome:?}"
    );
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), Some(1));
}

#[test]
fn canonical_task_drift_during_planner_call_fences_publication() {
    for drift in [
        Drift::Scope,
        Drift::Acceptance,
        Drift::Risk,
        Drift::ForbiddenPath,
        Drift::DescendantAdded,
        Drift::DescendantReparented,
    ] {
        let db = TestDb::new(&format!("{drift:?}").to_lowercase());
        let policy = db.canonical_policy();
        let planner = DriftingPlanner::new(&db, Some(drift));
        let result = db.service().propose_decomposition(
            &planner,
            &branch(),
            request(&format!("drift-{drift:?}")),
            &policy,
        );
        assert_eq!(
            planner.inner.call_count(),
            1,
            "{drift:?} drift happens during the call"
        );
        assert_stale_without_publication(&db, result);
    }
}

#[test]
fn canonical_task_drift_during_replan_call_fences_publication() {
    for drift in [
        Drift::Scope,
        Drift::Acceptance,
        Drift::Risk,
        Drift::DescendantAdded,
    ] {
        let db = TestDb::new(&format!("replan-{drift:?}").to_lowercase());
        let policy = db.canonical_policy();
        let planner = DriftingPlanner::new(&db, Some(drift));
        let result = db.service().propose_replan(
            &planner,
            &branch(),
            replan_request(&format!("replan-drift-{drift:?}")),
            &policy,
        );
        assert_stale_without_publication(&db, result);
    }
}

#[test]
fn drift_between_policy_derivation_and_planning_start_fences_publication() {
    let db = TestDb::new("pre-call-drift");
    let policy = db.canonical_policy();
    apply_drift(&db.path, Drift::Scope);
    let planner = DriftingPlanner::new(&db, None);
    let result =
        db.service()
            .propose_decomposition(&planner, &branch(), request("pre-call-drift"), &policy);
    assert_stale_without_publication(&db, result);
}

#[test]
fn replanning_after_fenced_drift_re_derives_fresh_canonical_authority() {
    let db = TestDb::new("rederive");
    let stale_policy = db.canonical_policy();
    let planner = DriftingPlanner::new(&db, Some(Drift::Scope));
    let stale = db.service().propose_decomposition(
        &planner,
        &branch(),
        request("rederive-first"),
        &stale_policy,
    );
    assert_stale_without_publication(&db, stale);

    let fresh_policy = db.canonical_policy();
    assert_eq!(fresh_policy.scope_bound.roots(), vec!["repo:elsewhere"]);
    let outcome = db
        .service()
        .propose_replan(
            &DriftingPlanner::new(&db, None),
            &branch(),
            replan_request("rederive-second"),
            &fresh_policy,
        )
        .expect("fresh canonical authority publishes");
    assert!(
        matches!(outcome, PlanningProposalOutcome::Accepted(_)),
        "{outcome:?}"
    );
    let accepted = db
        .service()
        .current_accepted_plan(&branch())
        .unwrap()
        .expect("accepted plan");
    assert!(db
        .store()
        .planning_input_policy_matches(
            accepted.planning_input_snapshot_id(),
            ROOT,
            &fresh_policy.scope_bound,
            fresh_policy.baseline_risk,
            &fresh_policy.affected_parent_criteria,
            &[],
            None,
            POLICY,
        )
        .unwrap());
}

/// Planner whose only proposal escalates the leaf risk and therefore needs a
/// durable V9 RESUME decision before publication.
struct RiskPlanner(FakePlanner);

impl PlannerPort for RiskPlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        self.0.assess_atomicity(input)
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let mut proposal = self.0.propose_decomposition(input)?;
        let node = proposal.node_additions[0].id.clone();
        proposal.node_additions[0].risk_class = RiskClass::High;
        proposal.node_additions[0].required_gate =
            Some(plan_domain::GateRef::new("human-review").unwrap());
        proposal.risk_delta = vec![RiskDelta {
            node,
            from: RiskClass::Low,
            to: RiskClass::High,
        }];
        proposal.proposal_hash = plan_domain::compute_proposal_hash(&proposal);
        Ok(proposal)
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.0.propose_replan(input)
    }
}

#[test]
fn v9_resume_with_task_drift_after_policy_derivation_fails_closed_without_consumption() {
    let db = TestDb::new("v9-resume-drift");
    let planner = RiskPlanner(FakePlanner::for_single_leaf(PlanNodeId::new(LEAF).unwrap()));
    let decision = match db
        .service()
        .propose_decomposition(
            &planner,
            &branch(),
            request("v9-drift"),
            &db.canonical_policy(),
        )
        .expect("risk escalation waits for a human")
    {
        PlanningProposalOutcome::WaitingForHuman(decision) => *decision,
        other => panic!("expected V9 decision, got {other:?}"),
    };
    HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .decide(&decision.id, HumanContinuation::Resume, "operator:test")
        .expect("operator resumes");

    // The daemon derived current canonical authority, then a concurrent
    // writer changed the Task before the RESUME publication transaction.
    let derived_before_drift = db.canonical_policy();
    apply_drift(&db.path, Drift::Acceptance);
    let result = db
        .service()
        .resume_approved_risk_proposal(&decision.id, &derived_before_drift);
    assert!(
        matches!(
            result,
            Err(PlanningServiceError::Storage(
                StorageError::PlanningInputStale { .. }
            ))
        ),
        "{result:?}"
    );
    assert_eq!(db.store().get_plan_head(&branch()).unwrap(), None);
    assert_eq!(
        planner.0.call_count(),
        1,
        "RESUME never replays PlannerPort"
    );
    let persisted = HumanDecisionService::attach(db.store(), FixedClock(FIXED_MILLIS))
        .get(&decision.id)
        .unwrap()
        .expect("decision evidence remains");
    assert_ne!(persisted.status, HumanDecisionStatus::Consumed);
    assert!(persisted.consumed_at_ms.is_none());
}
