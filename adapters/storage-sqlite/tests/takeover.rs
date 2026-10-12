//! TASK-0172 store tests: TAKEOVER admission, appends, return, abandonment.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use plan_domain::{
    planning_evidence_key, EvidenceRef, HumanContinuation, HumanDecisionReason,
    HumanDecisionStatus, PlanBranchId, PlanningInputSnapshot, RiskClass, ScopeBound,
};
use storage_sqlite::{
    AbandonTakeover, AdmitTakeover, AppendTakeover, CompleteTakeoverReturn, EventType,
    NewHumanDecisionRequest, NewJournalEvent, PlanningRunState, SqliteStore, StorageError,
    TakeoverCompletion,
};
use takeover::{AuthoredField, TakeoverRecordId, TakeoverStatus};
use task_domain::{Project, Task, TaskId, TaskState};
use workspace::{
    BranchRef, RepositoryRef, Revision, TrustedCommonGit, WorkspaceId, WorkspaceLease,
    WorkspaceStore,
};

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
            "runlith-takeover-storage-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir.join("takeover.db"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).unwrap()
    }

    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.path).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        } else {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn evidence(value: &str) -> EvidenceRef {
    EvidenceRef::new(value).unwrap()
}

struct Admissible {
    task_id: TaskId,
    branch: PlanBranchId,
    hdr_id: plan_domain::HumanDecisionRequestId,
    project: task_domain::ProjectId,
}

/// Seeds the takeover task in `state`. Runtime and lease fixtures for
/// the scope must be seeded after this but before [`seed_resolved_hdr`]:
/// every execution, attempt and lease fact is task journal evidence,
/// and the HDR precondition binds the exact sequence at creation.
fn seed_task(store: &mut SqliteStore, state: TaskState) -> Admissible {
    let project = task_domain::ProjectId::new("proj-takeover").unwrap();
    store
        .create_project(&Project::new(project.clone(), "takeover".to_owned()).unwrap())
        .unwrap();
    let mut task = Task::draft(
        TaskId::new("task-takeover").unwrap(),
        project.clone(),
        "takeover scope".to_owned(),
    )
    .unwrap();
    task.mark_ready().unwrap();
    task.claim().unwrap();
    task.start().unwrap();
    if state == TaskState::Blocked {
        task.block("seeded block").unwrap();
    }
    assert_eq!(task.state(), state);
    store.create_task(&task).unwrap();
    Admissible {
        task_id: TaskId::new("task-takeover").unwrap(),
        branch: PlanBranchId::new("branch-takeover").unwrap(),
        hdr_id: plan_domain::HumanDecisionRequestId::new("hdr-seed").unwrap(),
        project,
    }
}

fn seed_resolved_hdr(
    store: &mut SqliteStore,
    partial: Admissible,
    leave_run_active: bool,
    options: Vec<HumanContinuation>,
) -> Admissible {
    seed_hdr_with_selection(store, partial, leave_run_active, options, true)
}

fn seed_pending_hdr(
    store: &mut SqliteStore,
    partial: Admissible,
    leave_run_active: bool,
    options: Vec<HumanContinuation>,
) -> Admissible {
    seed_hdr_with_selection(store, partial, leave_run_active, options, false)
}

fn seed_hdr_with_selection(
    store: &mut SqliteStore,
    partial: Admissible,
    leave_run_active: bool,
    options: Vec<HumanContinuation>,
    resolve: bool,
) -> Admissible {
    let branch = partial.branch.clone();
    let gate_evidence = vec![evidence("takeover-gate")];
    let snapshot = PlanningInputSnapshot::new(
        branch.clone(),
        "task-takeover".to_owned(),
        0,
        ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        RiskClass::Low,
        vec![],
        gate_evidence.clone(),
        vec![],
        None,
        "policy:takeover".to_owned(),
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
                task_id: TaskId::new("task-takeover").unwrap(),
                plan_branch_id: branch.clone(),
                reason: HumanDecisionReason::PlannerBlocked,
                risk: RiskClass::Low,
                affected_scope: ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
                options: options.clone(),
                recommendation: None,
                evidence: gate_evidence,
                proposal_hash: None,
                approved_proposal: None,
                plan_head: 0,
                planning_input_snapshot_id: snapshot.id.clone(),
                planning_input_fingerprint: snapshot.input_fingerprint.clone(),
                policy_fingerprint: "policy:takeover".to_owned(),
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
    if resolve {
        let resolved = store
            .resolve_human_decision(&hdr.id, options[0], "human:operator", NOW + 30)
            .unwrap();
        assert_eq!(resolved.status, HumanDecisionStatus::Resolved);
    }
    Admissible {
        task_id: partial.task_id,
        branch: partial.branch,
        hdr_id: hdr.id,
        project: partial.project,
    }
}

fn seed_resolved_takeover(
    store: &mut SqliteStore,
    state: TaskState,
    leave_run_active: bool,
    options: Vec<HumanContinuation>,
) -> Admissible {
    let partial = seed_task(store, state);
    seed_resolved_hdr(store, partial, leave_run_active, options)
}

fn park_event(fixture: &Admissible, record_hint: &str) -> NewJournalEvent {
    NewJournalEvent {
        event_type: EventType::Blocked,
        task_id: Some(fixture.task_id.clone()),
        project_id: Some(fixture.project.clone()),
        from_state: Some(TaskState::Running),
        to_state: Some(TaskState::Blocked),
        reason: Some(format!(
            "takeover {record_hint} (human decision {})",
            fixture.hdr_id.as_str()
        )),
        correlation_id: "op-takeover-admit-test".to_owned(),
        causation_id: format!("human-decision:{}", fixture.hdr_id.as_str()),
        occurred_at: "2026-10-08T00:00:00Z".to_owned(),
        provenance: "human-decision-service/test".to_owned(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

fn admit_params(fixture: &Admissible) -> AdmitTakeover {
    AdmitTakeover {
        hdr_id: fixture.hdr_id.clone(),
        expires_at_ms: EXPIRY,
        park_event: park_event(fixture, "takeover-1"),
        now_ms: NOW + 40,
    }
}

#[test]
fn admit_takeover_parks_task_and_records_active() {
    let db = TestDb::new("admit-happy");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Running,
        false,
        vec![HumanContinuation::Takeover],
    );

    let record = store.admit_takeover(admit_params(&fixture)).unwrap();

    assert_eq!(record.status, TakeoverStatus::Active);
    assert!(record.id.as_str().starts_with("takeover-"));
    assert_eq!(record.hdr_id, fixture.hdr_id);
    assert_eq!(record.task_id, fixture.task_id);
    assert_eq!(record.expires_at_ms, EXPIRY);
    assert_eq!(record.basis_plan_revision, 0);
    assert_eq!(record.actions, None);
    let hdr = store.get_human_decision(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(hdr.status, HumanDecisionStatus::Consumed);
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Blocked);
    let active = store
        .active_takeover_for_scope(&fixture.task_id, &fixture.branch)
        .unwrap()
        .unwrap();
    assert_eq!(active.id, record.id);
    assert!(store.takeover_anchors(&record.id).unwrap().is_empty());
}

#[test]
fn admit_refuses_active_run_with_zero_state_change() {
    let db = TestDb::new("admit-active-run");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Running,
        true,
        vec![HumanContinuation::Takeover],
    );

    let refusal = store
        .admit_takeover(admit_params(&fixture))
        .expect_err("an ACTIVE run must refuse admission");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. }
                if state == "TAKEOVER_ACTIVE_RUN"
        ),
        "typed TAKEOVER_ACTIVE_RUN refusal, got {refusal:?}"
    );
    let hdr = store.get_human_decision(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(hdr.status, HumanDecisionStatus::Resolved);
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
    assert!(store
        .active_takeover_for_scope(&fixture.task_id, &fixture.branch)
        .unwrap()
        .is_none());
}

fn runtime_event(
    event_type: EventType,
    fixture: &Admissible,
    execution_id: Option<ExecutionId>,
    attempt_id: Option<AttemptId>,
    attempt_outcome: Option<storage_sqlite::AttemptOutcome>,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type,
        task_id: Some(fixture.task_id.clone()),
        project_id: Some(fixture.project.clone()),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "op-takeover-runtime-test".to_owned(),
        causation_id: "req-takeover-runtime-test".to_owned(),
        occurred_at: "2026-10-08T00:00:00Z".to_owned(),
        provenance: "runtime-application-service/test".to_owned(),
        execution_id,
        attempt_id,
        attempt_outcome,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

fn seed_execution(store: &mut SqliteStore, fixture: &Admissible, id: &str) -> Execution {
    let execution = Execution::begin(ExecutionId::new(id).unwrap(), fixture.task_id.clone());
    let event = runtime_event(
        EventType::ExecutionCreated,
        fixture,
        Some(execution.id().clone()),
        None,
        None,
    );
    store
        .create_execution_with_event(&execution, &event)
        .unwrap();
    execution
}

fn seed_open_attempt(
    store: &mut SqliteStore,
    fixture: &Admissible,
    execution: &mut Execution,
    id: &str,
) {
    let attempt = execution
        .start_attempt(
            AttemptId::new(id).unwrap(),
            ProviderRef::new("claude").unwrap(),
        )
        .unwrap()
        .clone();
    let event = runtime_event(
        EventType::AttemptCreated,
        fixture,
        Some(execution.id().clone()),
        Some(attempt.id().clone()),
        None,
    );
    store.create_attempt_with_event(&attempt, &event).unwrap();
}

fn seed_lease(
    store: &mut SqliteStore,
    db: &TestDb,
    execution: &Execution,
    ws: &str,
    base_revision: &str,
) -> String {
    let lease = WorkspaceLease::allocate(
        WorkspaceId::new(ws).unwrap(),
        RepositoryRef::new("repo-local:test").unwrap(),
        execution.id().clone(),
        Revision::new(base_revision).unwrap(),
        BranchRef::new(format!("refs/heads/agent/{}", execution.id().as_str())).unwrap(),
        format!("/tmp/runlith-takeover-test/{ws}"),
    )
    .unwrap();
    let anchor_dir = db
        .path
        .parent()
        .expect("test db dir")
        .join(format!("anchor-{ws}"));
    std::fs::create_dir_all(&anchor_dir).unwrap();
    let canonical = std::fs::canonicalize(&anchor_dir).unwrap();
    let anchor = TrustedCommonGit::restore(canonical.to_str().unwrap()).unwrap();
    store
        .insert_allocated_lease_with_anchor(&lease, &anchor, NOW)
        .unwrap();
    canonical.to_str().unwrap().to_owned()
}

#[test]
fn admit_refuses_open_attempt_with_zero_state_change() {
    let db = TestDb::new("admit-open-attempt");
    let mut store = db.open();
    let partial = seed_task(&mut store, TaskState::Running);
    let mut execution = seed_execution(&mut store, &partial, "exec-open");
    seed_open_attempt(&mut store, &partial, &mut execution, "att-open");
    let fixture = seed_resolved_hdr(
        &mut store,
        partial,
        false,
        vec![HumanContinuation::Takeover],
    );

    let refusal = store
        .admit_takeover(admit_params(&fixture))
        .expect_err("an open attempt must refuse admission");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. }
                if state == "TAKEOVER_OPEN_ATTEMPT"
        ),
        "typed TAKEOVER_OPEN_ATTEMPT refusal, got {refusal:?}"
    );
    let hdr = store.get_human_decision(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(hdr.status, HumanDecisionStatus::Resolved);
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn admit_allows_concluded_attempts() {
    let db = TestDb::new("admit-concluded");
    let mut store = db.open();
    let partial = seed_task(&mut store, TaskState::Running);
    let mut execution = seed_execution(&mut store, &partial, "exec-done");
    seed_open_attempt(&mut store, &partial, &mut execution, "att-done");
    let event = runtime_event(
        EventType::AttemptConcluded,
        &partial,
        Some(execution.id().clone()),
        Some(AttemptId::new("att-done").unwrap()),
        Some(storage_sqlite::AttemptOutcome::from(
            agent_runtime::AttemptOutcome::Succeeded,
        )),
    );
    store
        .conclude_attempt_with_event(
            &AttemptId::new("att-done").unwrap(),
            agent_runtime::AttemptOutcome::Succeeded,
            "done",
            &event,
        )
        .unwrap();
    let fixture = seed_resolved_hdr(
        &mut store,
        partial,
        false,
        vec![HumanContinuation::Takeover],
    );

    let record = store.admit_takeover(admit_params(&fixture)).unwrap();
    assert_eq!(record.status, TakeoverStatus::Active);
}

#[test]
fn admit_refuses_when_task_not_running() {
    let db = TestDb::new("admit-not-running");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Blocked,
        false,
        vec![HumanContinuation::Takeover],
    );

    let refusal = store
        .admit_takeover(admit_params(&fixture))
        .expect_err("a non-Running task must refuse admission");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. }
                if state == "TAKEOVER_TASK_NOT_RUNNING"
        ),
        "typed TAKEOVER_TASK_NOT_RUNNING refusal, got {refusal:?}"
    );
}

#[test]
fn admit_refuses_consumed_hdr() {
    let db = TestDb::new("admit-consumed");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Running,
        false,
        vec![HumanContinuation::Takeover],
    );
    store.admit_takeover(admit_params(&fixture)).unwrap();

    let refusal = store
        .admit_takeover(admit_params(&fixture))
        .expect_err("a Consumed HDR must refuse re-admission");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. } if state == "CONSUMED"
        ),
        "stored-state refusal carrying CONSUMED, got {refusal:?}"
    );
}

#[test]
fn admit_captures_lease_anchors() {
    let db = TestDb::new("admit-anchors");
    let mut store = db.open();
    let partial = seed_task(&mut store, TaskState::Running);
    let execution = seed_execution(&mut store, &partial, "exec-anchored");
    let anchor_path = seed_lease(&mut store, &db, &execution, "ws-1", &"a".repeat(40));
    let fixture = seed_resolved_hdr(
        &mut store,
        partial,
        false,
        vec![HumanContinuation::Takeover],
    );

    let record = store.admit_takeover(admit_params(&fixture)).unwrap();
    let anchors = store.takeover_anchors(&record.id).unwrap();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].execution_id, "exec-anchored");
    assert_eq!(anchors[0].lease_id, "ws-1");
    assert_eq!(anchors[0].base_revision, "a".repeat(40));
    assert_eq!(anchors[0].anchor, Some(anchor_path));
}

#[test]
fn admit_refuses_hdr_without_takeover_option() {
    let db = TestDb::new("admit-no-option");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Running,
        false,
        vec![HumanContinuation::Replan],
    );

    let refusal = store
        .admit_takeover(admit_params(&fixture))
        .expect_err("a non-TAKEOVER selection must refuse admission");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. } if state == "RESOLVED"
        ),
        "stored-state refusal carrying RESOLVED, got {refusal:?}"
    );
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn admit_refuses_stale_hdr() {
    let db = TestDb::new("admit-stale");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Running,
        false,
        vec![HumanContinuation::Takeover],
    );
    let current = store.get_task(&fixture.task_id).unwrap().unwrap();
    let mut next = current.clone();
    next.block("drift before admission").unwrap();
    let mut drift = park_event(&fixture, "takeover-drift");
    drift.reason = Some("drift before admission".to_owned());
    store
        .apply_lifecycle_transition(&fixture.task_id, TaskState::Running, &next, &drift)
        .unwrap();

    let refusal = store
        .admit_takeover(admit_params(&fixture))
        .expect_err("a stale HDR must refuse admission");
    assert!(
        matches!(refusal, StorageError::HumanDecisionStale { .. }),
        "HumanDecisionStale refusal, got {refusal:?}"
    );
}

#[test]
fn concurrent_takeover_deciders_have_exactly_one_winner() {
    let db = TestDb::new("decide-race");
    let mut store = db.open();
    let partial = seed_task(&mut store, TaskState::Running);
    let fixture = seed_pending_hdr(
        &mut store,
        partial,
        false,
        vec![HumanContinuation::Takeover],
    );
    drop(store);

    let mut first = db.open();
    let mut second = db.open();
    let winner = first
        .resolve_human_decision(
            &fixture.hdr_id,
            HumanContinuation::Takeover,
            "human:first",
            NOW + 30,
        )
        .unwrap();
    assert_eq!(winner.status, HumanDecisionStatus::Resolved);
    let loser = second.resolve_human_decision(
        &fixture.hdr_id,
        HumanContinuation::Takeover,
        "human:second",
        NOW + 31,
    );
    assert!(
        matches!(
            loser,
            Err(StorageError::HumanDecisionState { ref state, .. }) if state == "RESOLVED"
        ),
        "single-winner typed refusal, got {loser:?}"
    );
    let store = db.open();
    let hdr = store.get_human_decision(&fixture.hdr_id).unwrap().unwrap();
    assert_eq!(hdr.decided_by.as_deref(), Some("human:first"));
}

#[test]
fn takeover_record_roundtrips_through_get() {
    let db = TestDb::new("record-get");
    let mut store = db.open();
    let fixture = seed_resolved_takeover(
        &mut store,
        TaskState::Running,
        false,
        vec![HumanContinuation::Takeover],
    );
    let record = store.admit_takeover(admit_params(&fixture)).unwrap();

    let loaded = store.get_takeover(&record.id).unwrap().unwrap();
    assert_eq!(loaded, record);
    let missing_id = TakeoverRecordId::new("takeover-999").unwrap();
    assert!(store.get_takeover(&missing_id).unwrap().is_none());
}

fn authored(text: &str) -> AuthoredField {
    AuthoredField::new(
        continuity::model::NoteBody::Inline(text.to_owned()),
        context_engine::model::Sensitivity::Project,
    )
    .unwrap()
}

fn unpark_event(
    fixture: &Admissible,
    record_id: &takeover::TakeoverRecordId,
    causation: &str,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: EventType::Started,
        task_id: Some(fixture.task_id.clone()),
        project_id: Some(fixture.project.clone()),
        from_state: Some(TaskState::Blocked),
        to_state: Some(TaskState::Running),
        reason: None,
        correlation_id: record_id.as_str().to_owned(),
        causation_id: causation.to_owned(),
        occurred_at: "2026-10-08T01:00:00Z".to_owned(),
        provenance: "human-decision-service/test".to_owned(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

fn admitted(store: &mut SqliteStore) -> (Admissible, takeover::TakeoverAdmission) {
    let fixture = seed_resolved_takeover(
        store,
        TaskState::Running,
        false,
        vec![HumanContinuation::Takeover],
    );
    let record = store.admit_takeover(admit_params(&fixture)).unwrap();
    (fixture, record)
}

#[test]
fn append_sets_authored_fields() {
    let db = TestDb::new("append-sets");
    let mut store = db.open();
    let (_fixture, record) = admitted(&mut store);

    let updated = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: Some(authored("restart the workers")),
            uncertainty: None,
            question: Some(authored("which queue depth?")),
            resulting_revision: None,
            now_ms: NOW + 100,
        })
        .unwrap();
    assert_eq!(updated.actions, Some(authored("restart the workers")));
    assert_eq!(updated.question, Some(authored("which queue depth?")));
    assert_eq!(updated.uncertainty, None);
    assert_eq!(updated.status, TakeoverStatus::Active);
    let loaded = store.get_takeover(&record.id).unwrap().unwrap();
    assert_eq!(loaded, updated);
}

#[test]
fn append_accumulates_across_calls() {
    let db = TestDb::new("append-accumulates");
    let mut store = db.open();
    let (_fixture, record) = admitted(&mut store);
    store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: Some(authored("a")),
            uncertainty: None,
            question: None,
            resulting_revision: None,
            now_ms: NOW + 100,
        })
        .unwrap();
    let updated = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: None,
            uncertainty: Some(authored("u")),
            question: None,
            resulting_revision: None,
            now_ms: NOW + 200,
        })
        .unwrap();
    assert_eq!(updated.actions, Some(authored("a")));
    assert_eq!(updated.uncertainty, Some(authored("u")));
}

#[test]
fn append_refuses_oversize_field() {
    let db = TestDb::new("append-oversize");
    let mut store = db.open();
    let (_fixture, record) = admitted(&mut store);
    let big = AuthoredField {
        body: continuity::model::NoteBody::Inline("x".repeat(4097)),
        sensitivity: context_engine::model::Sensitivity::Project,
    };
    let refusal = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: Some(big),
            uncertainty: None,
            question: None,
            resulting_revision: None,
            now_ms: NOW + 100,
        })
        .expect_err("an oversize authored field must refuse");
    assert!(
        matches!(refusal, StorageError::ConstraintViolation(_)),
        "ConstraintViolation, got {refusal:?}"
    );
}

#[test]
fn append_refuses_smuggled_secret_inline() {
    let db = TestDb::new("append-secret-inline");
    let mut store = db.open();
    let (_fixture, record) = admitted(&mut store);
    let smuggled = takeover::AuthoredField {
        body: continuity::model::NoteBody::Inline("token: hunter2".to_owned()),
        sensitivity: context_engine::model::Sensitivity::Secret,
    };
    let refusal = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: Some(smuggled),
            uncertainty: None,
            question: None,
            resulting_revision: None,
            now_ms: NOW + 100,
        })
        .expect_err("a smuggled SECRET inline value must refuse");
    assert!(
        matches!(refusal, StorageError::ConstraintViolation(_)),
        "ConstraintViolation, got {refusal:?}"
    );
    let loaded = store.get_takeover(&record.id).unwrap().unwrap();
    assert_eq!(loaded.actions, None);
}

#[test]
fn append_refuses_unknown_resulting_revision() {
    let db = TestDb::new("append-bad-revision");
    let mut store = db.open();
    let (_fixture, record) = admitted(&mut store);
    let refusal = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: None,
            uncertainty: None,
            question: None,
            resulting_revision: Some(99),
            now_ms: NOW + 100,
        })
        .expect_err("an unaccepted resulting revision must refuse");
    assert!(
        matches!(refusal, StorageError::ConstraintViolation(_)),
        "ConstraintViolation, got {refusal:?}"
    );
}

#[test]
fn append_refuses_non_active_record() {
    let db = TestDb::new("append-resolved");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    let event = unpark_event(&fixture, &record.id, "trigger:return-test");
    store
        .complete_takeover_return(CompleteTakeoverReturn {
            record_id: record.id.clone(),
            unpark_event: event,
            now_ms: NOW + 100,
        })
        .unwrap();
    let refusal = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: Some(authored("late")),
            uncertainty: None,
            question: None,
            resulting_revision: None,
            now_ms: NOW + 200,
        })
        .expect_err("append on a COMPLETED record must refuse");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. } if state == "COMPLETED"
        ),
        "stored-state COMPLETED refusal, got {refusal:?}"
    );
}

#[test]
fn append_reconciles_expired_record_first() {
    let db = TestDb::new("append-expired");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    let refusal = store
        .append_takeover(AppendTakeover {
            record_id: record.id.clone(),
            actions: Some(authored("late")),
            uncertainty: None,
            question: None,
            resulting_revision: None,
            now_ms: EXPIRY + 1,
        })
        .expect_err("append on an expired record must abandon and refuse");
    assert!(
        matches!(
            refusal,
            StorageError::HumanDecisionState { ref state, .. } if state == "EXPIRED"
        ),
        "stored-state EXPIRED refusal, got {refusal:?}"
    );
    let loaded = store.get_takeover(&record.id).unwrap().unwrap();
    assert_eq!(loaded.status, TakeoverStatus::Expired);
    assert_eq!(loaded.actions, None);
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn complete_return_unparks_and_completes() {
    let db = TestDb::new("return-happy");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    let event = unpark_event(&fixture, &record.id, "trigger:return-test");

    let completion = store
        .complete_takeover_return(CompleteTakeoverReturn {
            record_id: record.id.clone(),
            unpark_event: event,
            now_ms: NOW + 100,
        })
        .unwrap();
    let completed = match completion {
        TakeoverCompletion::Completed(record) => record,
        TakeoverCompletion::ExpiredAbandoned(record) => {
            panic!("a live record must complete, got abandoned {record:?}")
        }
    };
    assert_eq!(completed.status, TakeoverStatus::Completed);
    assert_eq!(completed.resolved_at_ms, Some(NOW + 100));
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
    assert!(store
        .active_takeover_for_scope(&fixture.task_id, &fixture.branch)
        .unwrap()
        .is_none());
}

#[test]
fn complete_return_refuses_moved_anchor() {
    let db = TestDb::new("return-anchor");
    let mut store = db.open();
    let partial = seed_task(&mut store, TaskState::Running);
    let execution = seed_execution(&mut store, &partial, "exec-anchored");
    seed_lease(&mut store, &db, &execution, "ws-1", &"a".repeat(40));
    let fixture = seed_resolved_hdr(
        &mut store,
        partial,
        false,
        vec![HumanContinuation::Takeover],
    );
    let record = store.admit_takeover(admit_params(&fixture)).unwrap();
    db.raw()
        .execute(
            "UPDATE workspace_leases SET base_revision = ?1 WHERE workspace_id = 'ws-1'",
            rusqlite::params!["b".repeat(40)],
        )
        .unwrap();
    drop(store);
    let mut store = db.open();
    let event = unpark_event(&fixture, &record.id, "trigger:return-test");

    let refusal = store
        .complete_takeover_return(CompleteTakeoverReturn {
            record_id: record.id.clone(),
            unpark_event: event,
            now_ms: NOW + 100,
        })
        .expect_err("a moved anchor must refuse return");
    assert!(
        matches!(refusal, StorageError::TakeoverAnchorMismatch { .. }),
        "TakeoverAnchorMismatch, got {refusal:?}"
    );
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Blocked);
    let loaded = store.get_takeover(&record.id).unwrap().unwrap();
    assert_eq!(loaded.status, TakeoverStatus::Active);
}

#[test]
fn complete_return_abandons_when_expired() {
    let db = TestDb::new("return-expired");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    let event = unpark_event(&fixture, &record.id, "trigger:return-test");

    let completion = store
        .complete_takeover_return(CompleteTakeoverReturn {
            record_id: record.id.clone(),
            unpark_event: event,
            now_ms: EXPIRY + 1,
        })
        .unwrap();
    match completion {
        TakeoverCompletion::ExpiredAbandoned(abandoned) => {
            assert_eq!(abandoned.status, TakeoverStatus::Expired);
        }
        TakeoverCompletion::Completed(record) => {
            panic!("an expired record must abandon, got completed {record:?}")
        }
    }
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn abandon_unparks_and_expires() {
    let db = TestDb::new("abandon-happy");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    let event = unpark_event(&fixture, &record.id, "takeover-expired:test");

    let abandoned = store
        .abandon_takeover(AbandonTakeover {
            record_id: record.id.clone(),
            unpark_event: event,
            now_ms: EXPIRY + 1,
        })
        .unwrap();
    assert_eq!(abandoned.status, TakeoverStatus::Expired);
    assert_eq!(abandoned.resolved_at_ms, Some(EXPIRY + 1));
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
}

#[test]
fn abandon_refuses_live_record() {
    let db = TestDb::new("abandon-live");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    let event = unpark_event(&fixture, &record.id, "takeover-expired:test");

    let refusal = store
        .abandon_takeover(AbandonTakeover {
            record_id: record.id.clone(),
            unpark_event: event,
            now_ms: NOW + 100,
        })
        .expect_err("abandoning a live record must refuse");
    assert!(
        matches!(refusal, StorageError::ConstraintViolation(_)),
        "ConstraintViolation, got {refusal:?}"
    );
    let task = store.get_task(&fixture.task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Blocked);
}

#[test]
fn expired_active_takeovers_lists_only_expired() {
    let db = TestDb::new("expired-scan");
    let mut store = db.open();
    let (fixture, record) = admitted(&mut store);
    assert!(store
        .expired_active_takeovers(Some(&fixture.task_id), NOW + 100)
        .unwrap()
        .is_empty());
    let expired = store
        .expired_active_takeovers(Some(&fixture.task_id), EXPIRY + 1)
        .unwrap();
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].id, record.id);
    let other = TaskId::new("task-other").unwrap();
    assert!(store
        .expired_active_takeovers(Some(&other), EXPIRY + 1)
        .unwrap()
        .is_empty());
    let all = store.expired_active_takeovers(None, EXPIRY + 1).unwrap();
    assert_eq!(all.len(), 1);
}

#[test]
fn live_anchors_track_workspace_drift() {
    let db = TestDb::new("live-anchors");
    let mut store = db.open();
    let partial = seed_task(&mut store, TaskState::Running);
    let execution = seed_execution(&mut store, &partial, "exec-anchored");
    seed_lease(&mut store, &db, &execution, "ws-1", &"a".repeat(40));
    let fixture = seed_resolved_hdr(
        &mut store,
        partial,
        false,
        vec![HumanContinuation::Takeover],
    );
    let record = store.admit_takeover(admit_params(&fixture)).unwrap();
    let captured = store.takeover_anchors(&record.id).unwrap();
    assert_eq!(
        store.live_takeover_anchors(&fixture.task_id).unwrap(),
        captured
    );
    db.raw()
        .execute(
            "UPDATE workspace_leases SET base_revision = ?1 WHERE workspace_id = 'ws-1'",
            rusqlite::params!["b".repeat(40)],
        )
        .unwrap();
    drop(store);
    let store = db.open();
    assert_ne!(
        store.live_takeover_anchors(&fixture.task_id).unwrap(),
        captured
    );
}

#[test]
fn resulting_revision_fk_rejects_unaccepted_revisions() {
    let db = TestDb::new("revision-fk");
    let mut store = db.open();
    let (_fixture, record) = admitted(&mut store);
    drop(store);
    let raw = db.raw();
    raw.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    let failure = raw
        .execute(
            "UPDATE takeover_admissions SET resulting_revision = 99 WHERE record_id = ?1",
            rusqlite::params![record.id.as_str()],
        )
        .expect_err("the FK must reject an unaccepted revision");
    let message = failure.to_string();
    assert!(
        message.contains("FOREIGN KEY"),
        "FK violation, got {message}"
    );
}
