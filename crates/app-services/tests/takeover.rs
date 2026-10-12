//! TASK-0172 service tests: TAKEOVER apply, append, render, reconcile.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{FixedClock, HumanDecisionService, TaskService};
use plan_domain::{
    planning_evidence_key, EvidenceRef, HumanContinuation, HumanDecisionReason,
    HumanDecisionStatus, PlanBranchId, PlanningInputSnapshot, RiskClass, ScopeBound,
};
use storage_sqlite::{NewHumanDecisionRequest, PlanningRunState, SqliteStore, StorageError};
use takeover::TakeoverStatus;
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const NOW: u64 = 1_800_000_000_000;
const EXPIRY: u64 = NOW + 3_600_000;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-takeover-service-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir.join("takeover.db"),
        }
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.path).unwrap()
    }

    fn decisions(&self, now: u64) -> HumanDecisionService<FixedClock> {
        HumanDecisionService::attach(self.store(), FixedClock(now))
    }

    fn tasks(&self, now: u64) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(now)).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

fn evidence(value: &str) -> EvidenceRef {
    EvidenceRef::new(value).unwrap()
}

struct Fixture {
    task_id: TaskId,
    branch: PlanBranchId,
    hdr_id: plan_domain::HumanDecisionRequestId,
}

fn seed_running_task(store: &mut SqliteStore, id: &str, objective: &str) -> TaskId {
    let project = ProjectId::new("proj-takeover-svc").unwrap();
    if store.get_project(&project).unwrap().is_none() {
        store
            .create_project(&Project::new(project.clone(), "takeover svc".to_owned()).unwrap())
            .unwrap();
    }
    let mut task = Task::draft(TaskId::new(id).unwrap(), project, objective.to_owned()).unwrap();
    task.mark_ready().unwrap();
    task.claim().unwrap();
    task.start().unwrap();
    store.create_task(&task).unwrap();
    task.id().clone()
}

fn seed_resolved_takeover(
    store: &mut SqliteStore,
    task_id: &TaskId,
    leave_run_active: bool,
) -> Fixture {
    let branch = PlanBranchId::new("branch-takeover-svc").unwrap();
    let gate_evidence = vec![evidence("takeover-gate-svc")];
    let snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        task_id.as_str().to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        gate_evidence.clone(),
        vec![],
        None,
        "policy:takeover-svc".to_owned(),
    )
    .unwrap();
    let run = store
        .start_human_gate_planning_run(&snapshot, &planning_evidence_key(&gate_evidence), NOW)
        .unwrap();
    let hdr = store
        .publish_planning_input_and_create_human_decision(
            &run,
            &snapshot,
            NewHumanDecisionRequest {
                task_id: task_id.clone(),
                plan_branch_id: branch.clone(),
                reason: HumanDecisionReason::PlannerBlocked,
                risk: RiskClass::Low,
                affected_scope: ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
                options: vec![HumanContinuation::Takeover],
                recommendation: None,
                evidence: gate_evidence,
                proposal_hash: None,
                approved_proposal: None,
                plan_head: 0,
                planning_input_snapshot_id: snapshot.id.clone(),
                planning_input_fingerprint: snapshot.input_fingerprint.clone(),
                policy_fingerprint: "policy:takeover-svc".to_owned(),
                expires_at_ms: Some(EXPIRY + 3_600_000),
            },
            NOW + 10,
        )
        .unwrap();
    if !leave_run_active {
        store
            .finish_planning_run(
                &run.id,
                run.generation,
                PlanningRunState::Succeeded,
                NOW + 20,
            )
            .unwrap();
    }
    store
        .resolve_human_decision(
            &hdr.id,
            HumanContinuation::Takeover,
            "human:operator",
            NOW + 30,
        )
        .unwrap();
    Fixture {
        task_id: task_id.clone(),
        branch,
        hdr_id: hdr.id,
    }
}

#[test]
fn apply_takeover_admits_and_parks() {
    let db = TestDb::new("apply-happy");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let fixture = seed_resolved_takeover(&mut store, &task_id, false);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    let record = decisions.apply_takeover(&fixture.hdr_id, EXPIRY).unwrap();
    assert_eq!(record.status, TakeoverStatus::Active);
    assert_eq!(record.task_id, fixture.task_id);
    let tasks = db.tasks(NOW + 40);
    let task = tasks.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Blocked);
    let hdr = decisions.get(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(hdr.status, HumanDecisionStatus::Consumed);
}

#[test]
fn apply_takeover_refuses_bad_expiry() {
    let db = TestDb::new("apply-expiry");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let fixture = seed_resolved_takeover(&mut store, &task_id, false);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    for bad in [NOW + 40, NOW + 40 + takeover::TAKEOVER_MAX_TTL_MS + 1] {
        let refusal = decisions
            .apply_takeover(&fixture.hdr_id, bad)
            .expect_err("a bad expiry must refuse");
        assert!(
            matches!(
                refusal,
                app_services::ServiceError::Storage(StorageError::ConstraintViolation(_))
            ),
            "ConstraintViolation, got {refusal:?}"
        );
    }
    let hdr = decisions.get(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(hdr.status, HumanDecisionStatus::Resolved);
    let tasks = db.tasks(NOW + 40);
    let task = tasks.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn apply_takeover_surfaces_active_run_refusal() {
    let db = TestDb::new("apply-active-run");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let fixture = seed_resolved_takeover(&mut store, &task_id, true);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    let refusal = decisions
        .apply_takeover(&fixture.hdr_id, EXPIRY)
        .expect_err("an ACTIVE run must refuse");
    assert!(
        matches!(
            refusal,
            app_services::ServiceError::Storage(StorageError::HumanDecisionState {
                ref state,
                ..
            }) if state == "TAKEOVER_ACTIVE_RUN"
        ),
        "typed TAKEOVER_ACTIVE_RUN, got {refusal:?}"
    );
}

#[test]
fn apply_local_refuses_takeover() {
    let db = TestDb::new("apply-local");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let fixture = seed_resolved_takeover(&mut store, &task_id, false);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    let refusal = decisions
        .apply_local(&fixture.hdr_id)
        .expect_err("apply_local must refuse TAKEOVER");
    assert!(
        matches!(
            refusal,
            app_services::ServiceError::Storage(StorageError::HumanDecisionState {
                ref state,
                ..
            }) if state == "RESOLVED_TAKEOVER_REQUIRES_TAKEOVER_BOUNDARY"
        ),
        "takeover-boundary refusal, got {refusal:?}"
    );
}

fn authored(text: &str) -> takeover::AuthoredField {
    takeover::AuthoredField::new(
        continuity::model::NoteBody::Inline(text.to_owned()),
        context_engine::model::Sensitivity::Project,
    )
    .unwrap()
}

#[test]
fn append_takeover_sets_fields() {
    let db = TestDb::new("append-svc");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let fixture = seed_resolved_takeover(&mut store, &task_id, false);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    let record = decisions.apply_takeover(&fixture.hdr_id, EXPIRY).unwrap();
    let updated = decisions
        .append_takeover(
            &record.id,
            app_services::TakeoverAppends {
                actions: Some(authored("restart the workers")),
                uncertainty: None,
                question: Some(authored("which queue depth?")),
                resulting_revision: None,
            },
        )
        .unwrap();
    assert_eq!(updated.actions, Some(authored("restart the workers")));
    assert_eq!(updated.question, Some(authored("which queue depth?")));
}

#[test]
fn reconcile_abandons_expired_takeover() {
    let db = TestDb::new("reconcile-svc");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let fixture = seed_resolved_takeover(&mut store, &task_id, false);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    let record = decisions.apply_takeover(&fixture.hdr_id, EXPIRY).unwrap();
    assert!(decisions
        .reconcile_takeover_expiry(Some(&fixture.task_id))
        .unwrap()
        .is_empty());
    drop(decisions);
    let mut late = db.decisions(EXPIRY + 1);
    let abandoned = late
        .reconcile_takeover_expiry(Some(&fixture.task_id))
        .unwrap();
    assert_eq!(abandoned.len(), 1);
    assert_eq!(abandoned[0].id, record.id);
    assert_eq!(abandoned[0].status, TakeoverStatus::Expired);
    let tasks = db.tasks(EXPIRY + 1);
    let task = tasks.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn render_handoff_packet_assembles_refs() {
    let db = TestDb::new("render-svc");
    let mut store = db.store();
    let task_id = seed_running_task(&mut store, "task-takeover", "ship the widget");
    let open_dep = seed_running_task(&mut store, "task-open-dep", "open dependency");
    let mut closed = Task::draft(
        TaskId::new("task-closed-dep").unwrap(),
        ProjectId::new("proj-takeover-svc").unwrap(),
        "closed dependency".to_owned(),
    )
    .unwrap();
    let mut snapshot = closed.snapshot();
    snapshot.state = TaskState::Completed;
    closed = Task::restore(snapshot).unwrap();
    store.create_task(&closed).unwrap();
    let mut tasks = TaskService::open(&db.path, FixedClock(NOW)).unwrap();
    tasks.add_dependency(&task_id, &open_dep).unwrap();
    tasks
        .add_dependency(&task_id, &TaskId::new("task-closed-dep").unwrap())
        .unwrap();
    drop(tasks);
    let fixture = seed_resolved_takeover(&mut store, &task_id, false);
    drop(store);

    let mut decisions = db.decisions(NOW + 40);
    let record = decisions.apply_takeover(&fixture.hdr_id, EXPIRY).unwrap();
    decisions
        .append_takeover(
            &record.id,
            app_services::TakeoverAppends {
                actions: Some(authored("restart the workers")),
                uncertainty: None,
                question: None,
                resulting_revision: None,
            },
        )
        .unwrap();
    let tasks = db.tasks(NOW + 50);
    let packet = decisions.render_handoff_packet(&tasks, &record.id).unwrap();
    assert_eq!(packet.goal, "ship the widget");
    assert_eq!(packet.state.task_id, task_id.as_str());
    assert_eq!(packet.state.plan_branch_id, fixture.branch.as_str());
    assert_eq!(packet.state.plan_head, 0);
    assert_eq!(packet.state.lease_id, None);
    let hdr = decisions.get(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(
        packet.state.task_event_seq,
        hdr.precondition.task_event_seq + 1
    );
    assert_eq!(packet.blockers.len(), 2);
    assert!(packet.blockers.contains(&open_dep.as_str().to_owned()));
    assert!(packet.blockers.contains(&"PLANNER_BLOCKED".to_owned()));
    assert_eq!(packet.actions, Some(authored("restart the workers")));
    assert_eq!(packet.uncertainty, None);
}
