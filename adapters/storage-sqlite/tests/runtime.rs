//! G3 runtime persistence tests: migration 0004, Execution/Attempt
//! durability, atomic state+event commits, corruption matrix, and ADR-0022
//! scope enforcement.
//!
//! Real files only: every test owns a temporary database file. Raw SQL
//! appears solely in migration fixtures, failure-injection triggers, and
//! corruption fixtures; production paths stay typed. Atomicity fixtures
//! reuse the proven G2 trigger style from `journal.rs`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use agent_runtime::{
    Attempt, AttemptId, AttemptOutcome as DomainOutcome, AttemptSnapshot, Execution, ExecutionId,
    ProviderRef,
};
use storage_sqlite::{
    AttemptOutcome, EventId, EventType, JournalEvent, NewJournalEvent, SqliteStore, StorageError,
    EVENT_SCHEMA_VERSION, SUPPORTED_SCHEMA_VERSION,
};
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-runtime-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("open test db")
    }

    fn raw(&self) -> rusqlite::Connection {
        let connection = rusqlite::Connection::open(&self.path).expect("raw fixture connection");
        #[cfg(unix)]
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
            .expect("secure raw sqlite fixture");
        connection
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

fn make_project(id: &str) -> Project {
    Project::new(ProjectId::new(id).unwrap(), format!("project {id}")).unwrap()
}

fn draft_task(project: &str, id: &str) -> Task {
    Task::draft(
        TaskId::new(id).unwrap(),
        ProjectId::new(project).unwrap(),
        format!("objective {id}"),
    )
    .unwrap()
}

fn seed_task(store: &mut SqliteStore, project: &str, id: &str) -> Task {
    if store
        .get_project(&ProjectId::new(project).unwrap())
        .unwrap()
        .is_none()
    {
        store.create_project(&make_project(project)).unwrap();
    }
    let task = draft_task(project, id);
    store.create_task(&task).unwrap();
    task
}

fn execution(task: &Task, id: &str) -> Execution {
    Execution::begin(ExecutionId::new(id).unwrap(), task.id().clone())
}

fn open_attempt(execution: &mut Execution, id: &str, provider: &str) -> Attempt {
    execution
        .start_attempt(
            AttemptId::new(id).unwrap(),
            ProviderRef::new(provider).unwrap(),
        )
        .unwrap()
        .clone()
}

fn base_event(
    event_type: EventType,
    task: &Task,
    execution_id: Option<ExecutionId>,
    attempt_id: Option<AttemptId>,
    attempt_outcome: Option<AttemptOutcome>,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "op-test-1".into(),
        causation_id: "req-test-1".into(),
        occurred_at: "2024-01-01T00:00:00Z".into(),
        provenance: "runtime-application-service/test".into(),
        execution_id,
        attempt_id,
        attempt_outcome,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

fn journal_len(store: &SqliteStore) -> usize {
    store.list_events_ordered().unwrap().len()
}

// -- migration ------------------------------------------------------------

#[test]
fn fresh_database_reaches_current_schema() {
    let db = TestDb::new("fresh-current");
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 30);
    assert!(store.list_events_ordered().unwrap().is_empty());
    assert!(store
        .get_execution(&ExecutionId::new("MISSING").unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn current_schema_reopen_is_a_no_op() {
    let db = TestDb::new("reopen");
    {
        let mut store = db.open();
        let task = seed_task(&mut store, "P-1", "T-1");
        let execution = execution(&task, "E-1");
        store
            .create_execution_with_event(
                &execution,
                &base_event(
                    EventType::ExecutionCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    None,
                    None,
                ),
            )
            .unwrap();
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_some());
    assert_eq!(journal_len(&store), 1);
}

/// A real v3 database (built from the canonical 0001..0003 files, not a
/// hand copy) with G2 state and journal rows migrates to the current schema with every
/// historical fact preserved bit-for-bit and NULL runtime/workspace scope.
#[test]
fn v3_database_with_g2_data_migrates_preserving_everything() {
    const M0001: &str = include_str!("../migrations/0001_task_core.sql");
    const M0002: &str = include_str!("../migrations/0002_event_journal.sql");
    const M0003: &str = include_str!("../migrations/0003_event_scope.sql");
    let db = TestDb::new("v3-migrate");
    {
        let connection = db.raw();
        connection.execute_batch(M0001).unwrap();
        connection.execute_batch(M0002).unwrap();
        connection.execute_batch(M0003).unwrap();
        connection
            .execute_batch(
                "INSERT INTO projects (id, name) VALUES ('P-1', 'project P-1');
                 INSERT INTO tasks (id, project_id, objective, state)
                    VALUES ('T-1', 'P-1', 'objective T-1', 'READY');
                 INSERT INTO event_journal
                    (event_type, event_version, task_id, project_id, from_state,
                     to_state, reason, correlation_id, causation_id, occurred_at,
                     provenance)
                 VALUES
                    ('task.created', 1, 'T-1', 'P-1', 'DRAFT', 'DRAFT', NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'task-application-service/create_task'),
                    ('task.marked_ready', 1, 'T-1', 'P-1', 'DRAFT', 'READY', NULL,
                     'op-2', 'req-2', '2024-01-01T00:00:01Z', 'task-application-service/mark_ready');
                 PRAGMA user_version = 3;",
            )
            .unwrap();
    }
    let mut store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    // History decodes unchanged through the current codec.
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, EventId::parse("evt-1").unwrap());
    assert_eq!(history[0].fact.event_type, EventType::TaskCreated);
    assert_eq!(history[0].event_version, EVENT_SCHEMA_VERSION);
    assert_eq!(
        history[0].fact.from_state,
        Some(task_domain::TaskState::Draft)
    );
    assert_eq!(
        history[0].fact.to_state,
        Some(task_domain::TaskState::Draft)
    );
    assert_eq!(history[1].id, EventId::parse("evt-2").unwrap());
    assert_eq!(history[1].fact.event_type, EventType::MarkedReady);
    assert_eq!(history[1].fact.correlation_id, "op-2");
    assert_eq!(history[1].fact.causation_id, "req-2");
    assert_eq!(history[1].fact.occurred_at, "2024-01-01T00:00:01Z");
    assert_eq!(
        history[1].fact.provenance,
        "task-application-service/mark_ready"
    );
    for event in &history {
        assert_eq!(event.fact.execution_id, None);
        assert_eq!(event.fact.attempt_id, None);
        assert_eq!(event.fact.attempt_outcome, None);
    }
    // Task state survived untouched.
    let task = store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(task.state(), task_domain::TaskState::Ready);
    // The sequence continues without reuse: the next fact is evt-3.
    let task_event = NewJournalEvent {
        event_type: EventType::Claimed,
        task_id: Some(TaskId::new("T-1").unwrap()),
        project_id: Some(ProjectId::new("P-1").unwrap()),
        from_state: Some(task_domain::TaskState::Ready),
        to_state: Some(task_domain::TaskState::Claimed),
        reason: None,
        correlation_id: "op-3".into(),
        causation_id: "req-3".into(),
        occurred_at: "2024-01-01T00:00:02Z".into(),
        provenance: "task-application-service/test".into(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    };
    let mut live = task.clone();
    live.claim().unwrap();
    let committed = store
        .apply_lifecycle_transition(task.id(), task.state(), &live, &task_event)
        .unwrap();
    assert_eq!(committed.id, EventId::parse("evt-3").unwrap());
    // And runtime facts now commit on the migrated database.
    let execution = execution(&live, "E-9");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &live,
                Some(ExecutionId::new("E-9").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    assert_eq!(journal_len(&store), 4);
}

// -- table layout review ----------------------------------------------------

#[test]
fn runtime_tables_carry_no_session_or_secret_columns() {
    let db = TestDb::new("layout");
    let store = db.open();
    let columns = |table: &str| -> Vec<String> {
        let raw = db.raw();
        let mut statement = raw
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(columns("executions"), ["id", "task_id"]);
    assert_eq!(
        columns("execution_attempts"),
        [
            "attempt_id",
            "execution_id",
            "position",
            "provider_ref",
            "outcome",
            "detail"
        ]
    );
    let journal = columns("event_journal");
    for required in ["execution_id", "attempt_id", "attempt_outcome"] {
        assert!(
            journal.contains(&required.to_owned()),
            "journal lacks {required}"
        );
    }
    for table in ["executions", "execution_attempts", "event_journal"] {
        let names = columns(table).join(",");
        for forbidden in [
            "session",
            "transcript",
            "secret",
            "token",
            "credential",
            "password",
            "payload_json",
            "metadata_json",
            "scheduler",
        ] {
            assert!(
                !names.contains(forbidden),
                "table {table} must not carry {forbidden}"
            );
        }
    }
    drop(store);
}

// -- foreign keys -------------------------------------------------------------

#[test]
fn execution_requires_an_existing_task() {
    let db = TestDb::new("fk-task");
    let mut store = db.open();
    store.create_project(&make_project("P-1")).unwrap();
    // No task row: the relational constraint (not just service logic)
    // rejects the write.
    let ghost = Task::draft(
        TaskId::new("GHOST").unwrap(),
        ProjectId::new("P-1").unwrap(),
        "objective",
    )
    .unwrap();
    let execution = execution(&ghost, "E-1");
    match store.create_execution_with_event(
        &execution,
        &base_event(
            EventType::ExecutionCreated,
            &ghost,
            Some(ExecutionId::new("E-1").unwrap()),
            None,
            None,
        ),
    ) {
        Err(StorageError::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn attempt_requires_an_existing_execution() {
    let db = TestDb::new("fk-exec");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "GHOST");
    let attempt = open_attempt(&mut execution, "A-1", "executor-a");
    match store.create_attempt_with_event(
        &attempt,
        &base_event(
            EventType::AttemptCreated,
            &task,
            Some(ExecutionId::new("GHOST").unwrap()),
            Some(AttemptId::new("A-1").unwrap()),
            None,
        ),
    ) {
        Err(StorageError::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn duplicate_execution_id_is_rejected() {
    let db = TestDb::new("dup-exec");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = execution(&task, "E-1");
    let event = || {
        base_event(
            EventType::ExecutionCreated,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            None,
            None,
        )
    };
    store
        .create_execution_with_event(&execution, &event())
        .unwrap();
    match store.create_execution_with_event(&execution, &event()) {
        Err(StorageError::Duplicate(_)) => {}
        other => panic!("expected Duplicate, got {other:?}"),
    }
    assert_eq!(journal_len(&store), 1);
}

// -- durability ---------------------------------------------------------------

fn conclude(
    store: &mut SqliteStore,
    task: &Task,
    execution_id: &str,
    attempt_id: &str,
    outcome: DomainOutcome,
    detail: &str,
) -> JournalEvent {
    let journal_outcome = AttemptOutcome::from(outcome);
    store
        .conclude_attempt_with_event(
            &AttemptId::new(attempt_id).unwrap(),
            outcome,
            detail,
            &base_event(
                EventType::AttemptConcluded,
                task,
                Some(ExecutionId::new(execution_id).unwrap()),
                Some(AttemptId::new(attempt_id).unwrap()),
                Some(journal_outcome),
            ),
        )
        .unwrap()
}

#[test]
fn runtime_history_survives_close_and_reopen() {
    let db = TestDb::new("durability");
    {
        let mut store = db.open();
        let task = seed_task(&mut store, "P-1", "T-1");
        let mut execution = execution(&task, "E-1");
        store
            .create_execution_with_event(
                &execution,
                &base_event(
                    EventType::ExecutionCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    None,
                    None,
                ),
            )
            .unwrap();
        let first = open_attempt(&mut execution, "A-1", "executor-a");
        store
            .create_attempt_with_event(
                &first,
                &base_event(
                    EventType::AttemptCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    Some(AttemptId::new("A-1").unwrap()),
                    None,
                ),
            )
            .unwrap();
        conclude(
            &mut store,
            &task,
            "E-1",
            "A-1",
            DomainOutcome::Failed,
            "process disappeared",
        );
        execution
            .conclude_attempt(
                &AttemptId::new("A-1").unwrap(),
                DomainOutcome::Failed,
                "process disappeared",
            )
            .unwrap();
        let second = open_attempt(&mut execution, "A-2", "executor-b");
        store
            .create_attempt_with_event(
                &second,
                &base_event(
                    EventType::AttemptCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    Some(AttemptId::new("A-2").unwrap()),
                    None,
                ),
            )
            .unwrap();
        conclude(
            &mut store,
            &task,
            "E-1",
            "A-2",
            DomainOutcome::Succeeded,
            "evidence produced",
        );
        execution
            .conclude_attempt(
                &AttemptId::new("A-2").unwrap(),
                DomainOutcome::Succeeded,
                "evidence produced",
            )
            .unwrap();
    }
    let store = db.open();
    let reloaded = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .expect("execution survives reopen");
    assert_eq!(reloaded.task_id().as_str(), "T-1");
    let attempts = reloaded.attempts();
    assert_eq!(attempts.len(), 2);
    // Order, outcomes, providers, and notes all survive.
    assert_eq!(attempts[0].id().as_str(), "A-1");
    assert_eq!(attempts[0].outcome(), Some(DomainOutcome::Failed));
    assert_eq!(attempts[0].detail(), Some("process disappeared"));
    assert_eq!(attempts[0].provider().as_str(), "executor-a");
    assert_eq!(attempts[1].id().as_str(), "A-2");
    assert_eq!(attempts[1].outcome(), Some(DomainOutcome::Succeeded));
    assert_eq!(attempts[1].detail(), Some("evidence produced"));
    // The domain object round-trips exactly: no replay, no rewrite.
    assert_eq!(reloaded.snapshot().attempts.len(), 2);
    // Runtime events survive in commit order with canonical scope.
    let history = store.list_events_ordered().unwrap();
    let types: Vec<EventType> = history.iter().map(|event| event.fact.event_type).collect();
    assert_eq!(
        types,
        [
            EventType::ExecutionCreated,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
        ]
    );
    assert_eq!(
        history[2].fact.attempt_outcome,
        Some(AttemptOutcome::Failed)
    );
    assert_eq!(
        history[4].fact.attempt_outcome,
        Some(AttemptOutcome::Succeeded)
    );
    for event in &history {
        assert_eq!(event.event_version, EVENT_SCHEMA_VERSION);
        assert!(event.fact.from_state.is_none());
        assert!(event.fact.to_state.is_none());
    }
    // The Task is untouched: runtime success completes nothing.
    let task = store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(task.state(), task_domain::TaskState::Draft);
}

#[test]
fn committed_state_survives_crash_style_reopen() {
    let db = TestDb::new("crash");
    {
        let mut store = db.open();
        let task = seed_task(&mut store, "P-1", "T-1");
        let mut execution = execution(&task, "E-1");
        store
            .create_execution_with_event(
                &execution,
                &base_event(
                    EventType::ExecutionCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    None,
                    None,
                ),
            )
            .unwrap();
        let first = open_attempt(&mut execution, "A-1", "executor-a");
        store
            .create_attempt_with_event(
                &first,
                &base_event(
                    EventType::AttemptCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    Some(AttemptId::new("A-1").unwrap()),
                    None,
                ),
            )
            .unwrap();
        conclude(
            &mut store,
            &task,
            "E-1",
            "A-1",
            DomainOutcome::Failed,
            "crash",
        );
        execution
            .conclude_attempt(
                &AttemptId::new("A-1").unwrap(),
                DomainOutcome::Failed,
                "crash",
            )
            .unwrap();
        let second = open_attempt(&mut execution, "A-2", "executor-b");
        store
            .create_attempt_with_event(
                &second,
                &base_event(
                    EventType::AttemptCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    Some(AttemptId::new("A-2").unwrap()),
                    None,
                ),
            )
            .unwrap();
        // No graceful finalizer runs here: the store is simply dropped
        // with A2 still open.
    }
    let store = db.open();
    let reloaded = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.attempts().len(), 2);
    assert_eq!(
        reloaded.attempts()[0].outcome(),
        Some(DomainOutcome::Failed)
    );
    assert!(reloaded.attempts()[1].is_open());
    assert_eq!(reloaded.open_attempt().unwrap().id().as_str(), "A-2");
}

#[test]
fn current_state_survives_journal_loss() {
    let db = TestDb::new("authority");
    {
        let mut store = db.open();
        let task = seed_task(&mut store, "P-1", "T-1");
        let mut execution = execution(&task, "E-1");
        store
            .create_execution_with_event(
                &execution,
                &base_event(
                    EventType::ExecutionCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    None,
                    None,
                ),
            )
            .unwrap();
        let first = open_attempt(&mut execution, "A-1", "executor-a");
        store
            .create_attempt_with_event(
                &first,
                &base_event(
                    EventType::AttemptCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    Some(AttemptId::new("A-1").unwrap()),
                    None,
                ),
            )
            .unwrap();
        conclude(
            &mut store,
            &task,
            "E-1",
            "A-1",
            DomainOutcome::TimedOut,
            "no progress",
        );
        db.raw()
            .execute_batch("DELETE FROM event_journal;")
            .unwrap();
    }
    let store = db.open();
    assert!(store.list_events_ordered().unwrap().is_empty());
    // Relational state answers without replaying anything.
    let reloaded = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        reloaded.attempts()[0].outcome(),
        Some(DomainOutcome::TimedOut)
    );
}

#[test]
fn reads_emit_no_runtime_events() {
    let db = TestDb::new("read-quiet");
    {
        let mut store = db.open();
        let task = seed_task(&mut store, "P-1", "T-1");
        let execution = execution(&task, "E-1");
        store
            .create_execution_with_event(
                &execution,
                &base_event(
                    EventType::ExecutionCreated,
                    &task,
                    Some(ExecutionId::new("E-1").unwrap()),
                    None,
                    None,
                ),
            )
            .unwrap();
    }
    let store = db.open();
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_some());
    assert!(store
        .get_attempt(&AttemptId::new("MISSING").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(journal_len(&store), 1);
    drop(store);
    let store = db.open();
    assert_eq!(journal_len(&store), 1);
}

// -- atomicity (failure injection) ----------------------------------------------

#[test]
fn execution_event_failure_rolls_back_the_execution() {
    let db = TestDb::new("atomic-exec-event");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_event BEFORE INSERT ON event_journal
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let execution = execution(&task, "E-1");
    let result = store.create_execution_with_event(
        &execution,
        &base_event(
            EventType::ExecutionCreated,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            None,
            None,
        ),
    );
    assert!(result.is_err(), "event failure must fail the commit");
    drop(store);
    db.raw().execute_batch("DROP TRIGGER fail_event;").unwrap();
    let store = db.open();
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn execution_state_failure_commits_no_event() {
    let db = TestDb::new("atomic-exec-state");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_exec BEFORE INSERT ON executions
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let execution = execution(&task, "E-1");
    let result = store.create_execution_with_event(
        &execution,
        &base_event(
            EventType::ExecutionCreated,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            None,
            None,
        ),
    );
    assert!(result.is_err(), "state failure must fail the commit");
    drop(store);
    db.raw().execute_batch("DROP TRIGGER fail_exec;").unwrap();
    let store = db.open();
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn attempt_event_failure_rolls_back_the_attempt() {
    let db = TestDb::new("atomic-attempt-event");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_event BEFORE INSERT ON event_journal
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let attempt = open_attempt(&mut execution, "A-1", "executor-a");
    let result = store.create_attempt_with_event(
        &attempt,
        &base_event(
            EventType::AttemptCreated,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            Some(AttemptId::new("A-1").unwrap()),
            None,
        ),
    );
    assert!(result.is_err(), "event failure must fail the commit");
    drop(store);
    db.raw().execute_batch("DROP TRIGGER fail_event;").unwrap();
    let store = db.open();
    let reloaded = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert!(reloaded.attempts().is_empty());
    assert_eq!(journal_len(&store), 1);
}

#[test]
fn attempt_state_failure_commits_no_event() {
    let db = TestDb::new("atomic-attempt-state");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_attempt BEFORE INSERT ON execution_attempts
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let attempt = open_attempt(&mut execution, "A-1", "executor-a");
    let result = store.create_attempt_with_event(
        &attempt,
        &base_event(
            EventType::AttemptCreated,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            Some(AttemptId::new("A-1").unwrap()),
            None,
        ),
    );
    assert!(result.is_err(), "state failure must fail the commit");
    drop(store);
    db.raw()
        .execute_batch("DROP TRIGGER fail_attempt;")
        .unwrap();
    let store = db.open();
    let reloaded = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert!(reloaded.attempts().is_empty());
    assert_eq!(journal_len(&store), 1);
}

#[test]
fn conclusion_event_failure_preserves_the_open_attempt() {
    let db = TestDb::new("atomic-conclude-event");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    let attempt = open_attempt(&mut execution, "A-1", "executor-a");
    store
        .create_attempt_with_event(
            &attempt,
            &base_event(
                EventType::AttemptCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                Some(AttemptId::new("A-1").unwrap()),
                None,
            ),
        )
        .unwrap();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_event BEFORE INSERT ON event_journal
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let result = store.conclude_attempt_with_event(
        &AttemptId::new("A-1").unwrap(),
        DomainOutcome::Failed,
        "process disappeared",
        &base_event(
            EventType::AttemptConcluded,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            Some(AttemptId::new("A-1").unwrap()),
            Some(AttemptOutcome::Failed),
        ),
    );
    assert!(
        result.is_err(),
        "event failure must roll back the conclusion"
    );
    drop(store);
    db.raw().execute_batch("DROP TRIGGER fail_event;").unwrap();
    let store = db.open();
    let attempt = store
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert!(
        attempt.is_open(),
        "conclusion must roll back with the event"
    );
    assert_eq!(journal_len(&store), 2);
}

#[test]
fn conclusion_state_failure_appends_no_event() {
    let db = TestDb::new("atomic-conclude-state");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    let attempt = open_attempt(&mut execution, "A-1", "executor-a");
    store
        .create_attempt_with_event(
            &attempt,
            &base_event(
                EventType::AttemptCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                Some(AttemptId::new("A-1").unwrap()),
                None,
            ),
        )
        .unwrap();
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_conclude BEFORE UPDATE ON execution_attempts
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let result = store.conclude_attempt_with_event(
        &AttemptId::new("A-1").unwrap(),
        DomainOutcome::Failed,
        "process disappeared",
        &base_event(
            EventType::AttemptConcluded,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            Some(AttemptId::new("A-1").unwrap()),
            Some(AttemptOutcome::Failed),
        ),
    );
    assert!(result.is_err(), "state failure must fail the commit");
    drop(store);
    db.raw()
        .execute_batch("DROP TRIGGER fail_conclude;")
        .unwrap();
    let store = db.open();
    let attempt = store
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert!(attempt.is_open());
    assert_eq!(journal_len(&store), 2);
}

#[test]
fn second_conclusion_fails_with_history_unchanged() {
    let db = TestDb::new("conclude-once");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    let attempt = open_attempt(&mut execution, "A-1", "executor-a");
    store
        .create_attempt_with_event(
            &attempt,
            &base_event(
                EventType::AttemptCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                Some(AttemptId::new("A-1").unwrap()),
                None,
            ),
        )
        .unwrap();
    conclude(
        &mut store,
        &task,
        "E-1",
        "A-1",
        DomainOutcome::Failed,
        "first",
    );
    let before = journal_len(&store);
    // A rewrite into success must fail explicitly and append nothing.
    let rewrite = store.conclude_attempt_with_event(
        &AttemptId::new("A-1").unwrap(),
        DomainOutcome::Succeeded,
        "late claim",
        &base_event(
            EventType::AttemptConcluded,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            Some(AttemptId::new("A-1").unwrap()),
            Some(AttemptOutcome::Succeeded),
        ),
    );
    match rewrite {
        Err(StorageError::ConstraintViolation(_)) => {}
        other => panic!("expected ConstraintViolation, got {other:?}"),
    }
    assert_eq!(journal_len(&store), before);
    let attempt = store
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(attempt.outcome(), Some(DomainOutcome::Failed));
    assert_eq!(attempt.detail(), Some("first"));
}

#[test]
fn sequential_discipline_rejects_a_second_open_attempt() {
    let db = TestDb::new("sequential");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let mut execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    let first = open_attempt(&mut execution, "A-1", "executor-a");
    store
        .create_attempt_with_event(
            &first,
            &base_event(
                EventType::AttemptCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                Some(AttemptId::new("A-1").unwrap()),
                None,
            ),
        )
        .unwrap();
    // A-1 is still open: bypassing the domain and writing A-2 directly
    // must still fail closed at the commit boundary.
    let rogue = Attempt::restore(AttemptSnapshot {
        id: AttemptId::new("A-2").unwrap(),
        execution_id: ExecutionId::new("E-1").unwrap(),
        provider: ProviderRef::new("executor-b").unwrap(),
        outcome: None,
        detail: None,
    })
    .unwrap();
    match store.create_attempt_with_event(
        &rogue,
        &base_event(
            EventType::AttemptCreated,
            &task,
            Some(ExecutionId::new("E-1").unwrap()),
            Some(AttemptId::new("A-2").unwrap()),
            None,
        ),
    ) {
        Err(StorageError::ConstraintViolation(_)) => {}
        other => panic!("expected ConstraintViolation, got {other:?}"),
    }
    assert_eq!(journal_len(&store), 2);
}

// -- corruption matrix ----------------------------------------------------------

#[test]
fn blank_execution_id_fails_journal_reconstruction() {
    // A blank stored id is unnameable through the typed API (id
    // validation rejects it at construction), so the reachable proof
    // goes through journal decode: a runtime fact referencing a blank
    // execution must fail explicitly, never decode.
    let db = TestDb::new("corrupt-exec-id");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             PRAGMA foreign_keys = OFF;
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance,
                 execution_id, attempt_id, attempt_outcome)
             VALUES ('execution.created', 1, 'T-1', 'P-1', NULL, NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                     '', NULL, NULL);
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::DomainReconstruction(_)) => {}
        other => panic!("expected DomainReconstruction, got {other:?}"),
    }
}

#[test]
fn attempt_with_blank_identity_or_provider_fails() {
    let db = TestDb::new("corrupt-attempt-id");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
             INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('', 'E-1', 0, 'executor-a', NULL, NULL);",
        )
        .unwrap();
    let store = db.open();
    match store.get_attempt(&AttemptId::new("A-1").unwrap()) {
        Ok(None) => {}
        other => panic!("expected None for unknown id, got {other:?}"),
    }
    match store.get_execution(&ExecutionId::new("E-1").unwrap()) {
        Err(StorageError::DomainReconstruction(_)) => {}
        other => panic!("expected DomainReconstruction, got {other:?}"),
    }
    // Blank provider reference fails the same way.
    db.raw()
        .execute_batch("UPDATE execution_attempts SET attempt_id = 'A-1', provider_ref = '';")
        .unwrap();
    drop(store);
    let store = db.open();
    match store.get_execution(&ExecutionId::new("E-1").unwrap()) {
        Err(StorageError::DomainReconstruction(_)) => {}
        other => panic!("expected DomainReconstruction, got {other:?}"),
    }
}

#[test]
fn unknown_attempt_outcome_is_corruption() {
    let db = TestDb::new("corrupt-outcome");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
             INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('A-1', 'E-1', 0, 'executor-a', 'crash', 'boom');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    match store.get_execution(&ExecutionId::new("E-1").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for unknown outcome, got {other:?}"),
    }
}

#[test]
fn attempt_without_execution_fails_explicitly() {
    let db = TestDb::new("corrupt-orphan");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('A-9', 'GHOST', 0, 'executor-a', NULL, NULL);",
        )
        .unwrap();
    let store = db.open();
    match store.get_attempt(&AttemptId::new("A-9").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for orphan attempt, got {other:?}"),
    }
}

#[test]
fn execution_without_task_fails_explicitly() {
    let db = TestDb::new("corrupt-orphan-exec");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO executions (id, task_id) VALUES ('E-9', 'GHOST');",
        )
        .unwrap();
    let store = db.open();
    match store.get_execution(&ExecutionId::new("E-9").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for orphan execution, got {other:?}"),
    }
}

#[test]
fn non_contiguous_attempt_order_is_corruption() {
    let db = TestDb::new("corrupt-order");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
             INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('A-1', 'E-1', 0, 'executor-a', 'failed', 'x'),
                    ('A-2', 'E-1', 2, 'executor-b', NULL, NULL);",
        )
        .unwrap();
    let store = db.open();
    match store.get_execution(&ExecutionId::new("E-1").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for gapped order, got {other:?}"),
    }
}

#[test]
fn duplicate_attempt_positions_are_rejected_by_constraint() {
    let db = TestDb::new("dup-position");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = execution(&task, "E-1");
    store
        .create_execution_with_event(
            &execution,
            &base_event(
                EventType::ExecutionCreated,
                &task,
                Some(ExecutionId::new("E-1").unwrap()),
                None,
                None,
            ),
        )
        .unwrap();
    let inserted = db.raw().execute(
        "INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('A-1', 'E-1', 0, 'executor-a', NULL, NULL)",
        [],
    );
    assert!(inserted.is_ok());
    // The same position twice violates UNIQUE(execution_id, position):
    // order can never become ambiguous.
    let duplicate = db.raw().execute(
        "INSERT INTO execution_attempts
            (attempt_id, execution_id, position, provider_ref, outcome, detail)
         VALUES ('A-2', 'E-1', 0, 'executor-b', NULL, NULL)",
        [],
    );
    assert!(duplicate.is_err(), "duplicate position must fail closed");
}

#[test]
fn unsupported_runtime_event_version_fails_explicitly() {
    let db = TestDb::new("corrupt-evt-version");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
             INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('A-1', 'E-1', 0, 'executor-a', 'failed', 'x');
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance,
                 execution_id, attempt_id, attempt_outcome)
             VALUES ('attempt.concluded', 7, 'T-1', 'P-1', NULL, NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                     'E-1', 'A-1', 'failed');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for v7 runtime event, got {other:?}"),
    }
}

#[test]
fn unknown_runtime_event_type_fails_explicitly() {
    let db = TestDb::new("corrupt-evt-type");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance)
             VALUES ('attempt.exploded', 1, 'T-1', 'P-1', NULL, NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for unknown type, got {other:?}"),
    }
}

#[test]
fn invalid_runtime_scopes_fail_on_read() {
    let db = TestDb::new("corrupt-scope");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    // attempt.concluded without an outcome.
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             PRAGMA foreign_keys = OFF;
             INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
             INSERT INTO execution_attempts
                (attempt_id, execution_id, position, provider_ref, outcome, detail)
             VALUES ('A-1', 'E-1', 0, 'executor-a', NULL, NULL);
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance,
                 execution_id, attempt_id, attempt_outcome)
             VALUES ('attempt.concluded', 1, 'T-1', 'P-1', NULL, NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                     'E-1', 'A-1', NULL);
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for outcome-less conclusion, got {other:?}"),
    }
    // attempt.created carrying an outcome.
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance,
                 execution_id, attempt_id, attempt_outcome)
             VALUES ('attempt.created', 1, 'T-1', 'P-1', NULL, NULL,
                     'op-2', 'req-2', '2024-01-01T00:00:01Z', 'test',
                     'E-1', 'A-1', 'failed');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    drop(store);
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for outcome-ful creation, got {other:?}"),
    }
    // Runtime fact carrying Task lifecycle states.
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance,
                 execution_id, attempt_id, attempt_outcome)
             VALUES ('execution.created', 1, 'T-1', 'P-1', 'DRAFT', 'READY',
                     'op-3', 'req-3', '2024-01-01T00:00:02Z', 'test',
                     'E-1', NULL, NULL);
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    drop(store);
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for Task states on runtime fact, got {other:?}"),
    }
}

#[test]
fn g2_event_with_runtime_scope_fails_on_read() {
    let db = TestDb::new("corrupt-g2-scope");
    {
        let mut store = db.open();
        seed_task(&mut store, "P-1", "T-1");
    }
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             PRAGMA foreign_keys = OFF;
             INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
             INSERT INTO event_journal
                (event_type, event_version, task_id, project_id, from_state,
                 to_state, correlation_id, causation_id, occurred_at, provenance,
                 execution_id, attempt_id, attempt_outcome)
             VALUES ('task.claimed', 1, 'T-1', 'P-1', 'READY', 'CLAIMED',
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                     'E-1', NULL, NULL);
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for G2 fact with runtime scope, got {other:?}"),
    }
}

#[test]
fn mismatched_runtime_envelope_writes_nothing() {
    let db = TestDb::new("runtime-mismatch");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = execution(&task, "E-1");
    // An execution.created event naming a different execution must not
    // commit the state row.
    let mut event = base_event(
        EventType::ExecutionCreated,
        &task,
        Some(ExecutionId::new("E-1").unwrap()),
        None,
        None,
    );
    event.execution_id = Some(ExecutionId::new("SOMEONE-ELSE").unwrap());
    match store.create_execution_with_event(&execution, &event) {
        Err(StorageError::ConstraintViolation(_)) => {}
        other => panic!("expected ConstraintViolation, got {other:?}"),
    }
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(journal_len(&store), 0);
}

// -- schema contract compatibility -----------------------------------------------

#[test]
fn persisted_runtime_envelope_matches_event_schema() {
    // Same compile-time fixture style as the G2 schema test in
    // app-services: the schema file is authority for field names;
    // production code never parses it at runtime, no codegen.
    const SCHEMA: &str = include_str!("../../../contracts/json-schema/event.schema.json");
    for required in [
        "\"execution_id\"",
        "\"attempt_id\"",
        "\"attempt_outcome\"",
        "\"execution.created\"",
        "\"attempt.created\"",
        "\"attempt.concluded\"",
        "\"succeeded\"",
        "\"failed\"",
        "\"timed_out\"",
        "\"cancelled\"",
    ] {
        assert!(SCHEMA.contains(required), "schema must define {required}");
    }
}
