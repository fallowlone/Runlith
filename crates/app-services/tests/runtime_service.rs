//! Trusted `RuntimeService` contract tests: atomic runtime mutations,
//! authoritative event scope, conclude-once discipline, retry history,
//! restart durability, and Task-independence.
//!
//! Deterministic clock throughout (`FixedClock`). Task/Project rows are
//! created through `TaskService` (the only Task writer); runtime facts go
//! through `RuntimeService`. Every database file is test-owned.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, AttemptOutcome, ExecutionId, ProviderRef};
use app_services::{FixedClock, RuntimeService, ServiceError, TaskService};
use storage_sqlite::{EventType, JournalEvent};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 2024-01-01T00:00:00Z in millis: every `occurred_at` below must render
/// exactly this stamp.
const FIXED_MILLIS: u64 = 1_704_067_200_000;
const FIXED_STAMP: &str = "2024-01-01T00:00:00Z";

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-runtime-service-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
    }

    fn tasks(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open task service")
    }

    fn runtime(&self) -> RuntimeService<FixedClock> {
        RuntimeService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open runtime service")
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

/// One project + one DRAFT task, committed through the trusted Task
/// service (the only Task writer). Returns the task identity.
fn seed_task(db: &TestDb, project: &str, id: &str) -> TaskId {
    let mut service = db.tasks();
    service
        .create_project(
            &Project::new(
                ProjectId::new(project).unwrap(),
                format!("project {project}"),
            )
            .unwrap(),
        )
        .unwrap();
    let task = Task::draft(
        TaskId::new(id).unwrap(),
        ProjectId::new(project).unwrap(),
        format!("objective {id}"),
    )
    .unwrap();
    service.create_task(&task).unwrap();
    task.id().clone()
}

fn journal_len(service: &RuntimeService<FixedClock>) -> usize {
    service.list_events_ordered().unwrap().len()
}

fn execution(id: &str) -> ExecutionId {
    ExecutionId::new(id).unwrap()
}

fn attempt(id: &str) -> AttemptId {
    AttemptId::new(id).unwrap()
}

fn provider(name: &str) -> ProviderRef {
    ProviderRef::new(name).unwrap()
}

// -- trusted mutations ----------------------------------------------------------

#[test]
fn full_runtime_flow_commits_canonical_scoped_history() {
    let db = TestDb::new("flow");
    let task_id = seed_task(&db, "P-1", "T-1");
    let mut service = db.runtime();
    assert_eq!(journal_len(&service), 2);

    let created = service
        .create_execution(execution("E-1"), task_id.clone())
        .unwrap();
    assert_eq!(created.event.fact.event_type, EventType::ExecutionCreated);
    assert_eq!(created.event.event_version, 1);
    assert_eq!(created.event.fact.task_id, Some(task_id.clone()));
    assert_eq!(
        created.event.fact.project_id.as_ref().unwrap().as_str(),
        "P-1"
    );
    assert_eq!(
        created.event.fact.execution_id.as_ref().unwrap().as_str(),
        "E-1"
    );
    assert_eq!(created.event.fact.attempt_id, None);
    assert_eq!(created.event.fact.attempt_outcome, None);
    assert!(created.event.fact.from_state.is_none());
    assert!(created.event.fact.to_state.is_none());
    assert_eq!(created.event.fact.occurred_at, FIXED_STAMP);
    assert!(
        created
            .event
            .fact
            .provenance
            .starts_with("runtime-application-service/"),
        "provenance must name the trusted runtime service"
    );

    service
        .create_attempt(&execution("E-1"), attempt("A-1"), provider("executor-a"))
        .unwrap();
    service
        .conclude_attempt(
            &execution("E-1"),
            &attempt("A-1"),
            AttemptOutcome::Failed,
            "process disappeared",
        )
        .unwrap();
    service
        .create_attempt(&execution("E-1"), attempt("A-2"), provider("executor-b"))
        .unwrap();
    let concluded = service
        .conclude_attempt(
            &execution("E-1"),
            &attempt("A-2"),
            AttemptOutcome::Succeeded,
            "evidence produced",
        )
        .unwrap();
    assert_eq!(
        concluded.event.fact.attempt_outcome,
        Some(storage_sqlite::AttemptOutcome::Succeeded)
    );

    // Exact committed order across Task creation and runtime facts.
    let history = service.list_events_ordered().unwrap();
    let types: Vec<EventType> = history.iter().map(|event| event.fact.event_type).collect();
    assert_eq!(
        types,
        [
            EventType::ProjectCreated,
            EventType::TaskCreated,
            EventType::ExecutionCreated,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
        ]
    );
    let ids: Vec<String> = history.iter().map(|event| event.id.to_string()).collect();
    assert_eq!(
        ids,
        ["evt-1", "evt-2", "evt-3", "evt-4", "evt-5", "evt-6", "evt-7"]
    );
    // Every runtime fact carries the derived project scope.
    for event in &history[2..] {
        assert_eq!(event.fact.project_id.as_ref().unwrap().as_str(), "P-1");
        assert_eq!(event.fact.task_id, Some(task_id.clone()));
        assert_eq!(event.fact.execution_id.as_ref().unwrap().as_str(), "E-1");
    }
    assert_eq!(
        history[4].fact.attempt_outcome,
        Some(storage_sqlite::AttemptOutcome::Failed)
    );

    // Retry is a new attempt: A-1 stays failed after A-2 succeeds.
    let execution = service.get_execution(&execution("E-1")).unwrap().unwrap();
    assert_eq!(execution.attempts().len(), 2);
    assert_eq!(
        execution.attempts()[0].outcome(),
        Some(AttemptOutcome::Failed)
    );
    assert_eq!(
        execution.attempts()[1].outcome(),
        Some(AttemptOutcome::Succeeded)
    );

    // The Task never moved: runtime success completes nothing, and no
    // lifecycle event beyond task.created exists.
    let tasks = db.tasks();
    let task = tasks.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Draft);
    let lifecycle: Vec<&JournalEvent> = history
        .iter()
        .filter(|event| {
            !matches!(
                event.fact.event_type,
                EventType::ProjectCreated
                    | EventType::TaskCreated
                    | EventType::ExecutionCreated
                    | EventType::AttemptCreated
                    | EventType::AttemptConcluded
            )
        })
        .collect();
    assert!(lifecycle.is_empty());
    let _ = tasks;
}

#[test]
fn execution_for_a_missing_task_fails_with_no_event() {
    let db = TestDb::new("missing-task");
    seed_task(&db, "P-1", "T-1");
    let mut service = db.runtime();
    let before = journal_len(&service);
    match service.create_execution(execution("E-1"), TaskId::new("GHOST").unwrap()) {
        Err(ServiceError::Storage(_)) => {}
        other => panic!("expected Storage error, got {other:?}"),
    }
    assert_eq!(journal_len(&service), before);
}

#[test]
fn attempt_for_a_missing_execution_fails_with_no_event() {
    let db = TestDb::new("missing-exec");
    seed_task(&db, "P-1", "T-1");
    let mut service = db.runtime();
    let before = journal_len(&service);
    match service.create_attempt(&execution("GHOST"), attempt("A-1"), provider("executor-a")) {
        Err(ServiceError::Storage(_)) => {}
        other => panic!("expected Storage error, got {other:?}"),
    }
    assert_eq!(journal_len(&service), before);
}

#[test]
fn second_open_attempt_is_rejected_with_no_event() {
    let db = TestDb::new("sequential");
    let task_id = seed_task(&db, "P-1", "T-1");
    let mut service = db.runtime();
    service.create_execution(execution("E-1"), task_id).unwrap();
    service
        .create_attempt(&execution("E-1"), attempt("A-1"), provider("executor-a"))
        .unwrap();
    let before = journal_len(&service);
    match service.create_attempt(&execution("E-1"), attempt("A-2"), provider("executor-b")) {
        Err(ServiceError::Storage(error)) => {
            assert!(
                error.to_string().contains("already open"),
                "rejection must name the discipline, got {error}"
            );
        }
        other => panic!("expected Storage error, got {other:?}"),
    }
    assert_eq!(journal_len(&service), before);
    let execution = service.get_execution(&execution("E-1")).unwrap().unwrap();
    assert_eq!(execution.attempts().len(), 1);
}

#[test]
fn second_conclusion_fails_with_history_unchanged() {
    let db = TestDb::new("conclude-once");
    let task_id = seed_task(&db, "P-1", "T-1");
    let mut service = db.runtime();
    service.create_execution(execution("E-1"), task_id).unwrap();
    service
        .create_attempt(&execution("E-1"), attempt("A-1"), provider("executor-a"))
        .unwrap();
    service
        .conclude_attempt(
            &execution("E-1"),
            &attempt("A-1"),
            AttemptOutcome::Failed,
            "first",
        )
        .unwrap();
    let before = journal_len(&service);
    match service.conclude_attempt(
        &execution("E-1"),
        &attempt("A-1"),
        AttemptOutcome::Succeeded,
        "late claim",
    ) {
        Err(ServiceError::Storage(error)) => {
            assert!(
                error.to_string().contains("already concluded"),
                "rewrite must fail explicitly, got {error}"
            );
        }
        other => panic!("expected Storage error, got {other:?}"),
    }
    assert_eq!(journal_len(&service), before);
    let attempt = service.get_attempt(&attempt("A-1")).unwrap().unwrap();
    assert_eq!(attempt.outcome(), Some(AttemptOutcome::Failed));
    assert_eq!(attempt.detail(), Some("first"));
}

#[test]
fn concluding_a_missing_attempt_fails_with_no_event() {
    let db = TestDb::new("conclude-missing");
    let task_id = seed_task(&db, "P-1", "T-1");
    let mut service = db.runtime();
    service.create_execution(execution("E-1"), task_id).unwrap();
    let before = journal_len(&service);
    match service.conclude_attempt(
        &execution("E-1"),
        &attempt("GHOST"),
        AttemptOutcome::Failed,
        "x",
    ) {
        Err(ServiceError::Storage(_)) => {}
        other => panic!("expected Storage error, got {other:?}"),
    }
    assert_eq!(journal_len(&service), before);
}

// -- restart durability -----------------------------------------------------------

#[test]
fn runtime_state_survives_service_reopen() {
    let db = TestDb::new("restart");
    let task_id = seed_task(&db, "P-1", "T-1");
    {
        let mut service = db.runtime();
        service
            .create_execution(execution("E-1"), task_id.clone())
            .unwrap();
        service
            .create_attempt(&execution("E-1"), attempt("A-1"), provider("executor-a"))
            .unwrap();
        service
            .conclude_attempt(
                &execution("E-1"),
                &attempt("A-1"),
                AttemptOutcome::Failed,
                "process disappeared",
            )
            .unwrap();
        service
            .create_attempt(&execution("E-1"), attempt("A-2"), provider("executor-b"))
            .unwrap();
        service
            .conclude_attempt(
                &execution("E-1"),
                &attempt("A-2"),
                AttemptOutcome::Succeeded,
                "evidence produced",
            )
            .unwrap();
    }
    // Both services (and their connections) are dropped; the same file
    // reopens below with history intact and the Task still DRAFT.
    let service = db.runtime();
    let execution = service.get_execution(&execution("E-1")).unwrap().unwrap();
    assert_eq!(execution.task_id(), &task_id);
    assert_eq!(execution.attempts().len(), 2);
    assert_eq!(execution.attempts()[0].id().as_str(), "A-1");
    assert_eq!(
        execution.attempts()[0].outcome(),
        Some(AttemptOutcome::Failed)
    );
    assert_eq!(execution.attempts()[1].id().as_str(), "A-2");
    assert_eq!(
        execution.attempts()[1].outcome(),
        Some(AttemptOutcome::Succeeded)
    );
    assert_eq!(execution.attempts()[1].provider().as_str(), "executor-b");
    assert_eq!(journal_len(&service), 7);
    let tasks = db.tasks();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Draft
    );
}

#[test]
fn committed_state_survives_abrupt_drop_with_open_attempt() {
    let db = TestDb::new("crash");
    let task_id = seed_task(&db, "P-1", "T-1");
    {
        let mut service = db.runtime();
        service.create_execution(execution("E-1"), task_id).unwrap();
        service
            .create_attempt(&execution("E-1"), attempt("A-1"), provider("executor-a"))
            .unwrap();
        service
            .conclude_attempt(
                &execution("E-1"),
                &attempt("A-1"),
                AttemptOutcome::Failed,
                "crash",
            )
            .unwrap();
        service
            .create_attempt(&execution("E-1"), attempt("A-2"), provider("executor-b"))
            .unwrap();
        // Dropped with A-2 still open: no graceful finalizer involved.
    }
    let service = db.runtime();
    let execution = service.get_execution(&execution("E-1")).unwrap().unwrap();
    assert_eq!(execution.attempts().len(), 2);
    assert_eq!(
        execution.attempts()[0].outcome(),
        Some(AttemptOutcome::Failed)
    );
    assert!(execution.attempts()[1].is_open());
}

#[test]
fn reads_emit_no_runtime_events() {
    let db = TestDb::new("read-quiet");
    let task_id = seed_task(&db, "P-1", "T-1");
    {
        let mut service = db.runtime();
        service.create_execution(execution("E-1"), task_id).unwrap();
    }
    let service = db.runtime();
    assert!(service.get_execution(&execution("E-1")).unwrap().is_some());
    assert!(service.get_attempt(&attempt("MISSING")).unwrap().is_none());
    assert_eq!(journal_len(&service), 3);
    drop(service);
    let service = db.runtime();
    assert_eq!(journal_len(&service), 3);
}
