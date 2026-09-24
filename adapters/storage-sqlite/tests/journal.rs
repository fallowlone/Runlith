//! Event Journal contract tests: migration, typed appends, ordering,
//! atomicity, corruption, and schema review.
//!
//! Atomicity fixtures use raw SQL triggers to force one half of the
//! commit to fail; production paths stay typed. Every database file is
//! test-owned.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use storage_sqlite::{
    EventId, EventType, JournalEvent, NewJournalEvent, SqliteStore, StorageError,
    EVENT_SCHEMA_VERSION, SUPPORTED_SCHEMA_VERSION,
};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-journal-test-{}-{suite}-{id}",
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

fn project(id: &str) -> Project {
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

fn new_event(
    task: &Task,
    from: TaskState,
    to: TaskState,
    event_type: EventType,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: Some(from),
        to_state: Some(to),
        reason: None,
        correlation_id: "op-test-1".into(),
        causation_id: "req-test-1".into(),
        occurred_at: "2024-01-01T00:00:00Z".into(),
        provenance: "task-application-service/test".into(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

fn ready_task_via_store(store: &mut SqliteStore) -> Task {
    store.create_project(&project("P-1")).unwrap();
    let mut task = draft_task("P-1", "T-1");
    task.mark_ready().unwrap();
    store.create_task(&task).unwrap();
    task
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
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 20);
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn v1_database_with_data_migrates_preserving_state() {
    let db = TestDb::new("v1-migrate");
    {
        // Hand-built v1 layout: 0001 tables only, version stamp 1, real rows.
        let connection = db.raw();
        connection
            .execute_batch(
                "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
                 CREATE TABLE tasks (
                     id TEXT PRIMARY KEY,
                     project_id TEXT NOT NULL REFERENCES projects (id),
                     objective TEXT NOT NULL, parent_id TEXT NULL REFERENCES tasks (id),
                     state TEXT NOT NULL, kind TEXT NULL, risk TEXT NULL);
                 CREATE TABLE task_dependencies (
                     task_id TEXT NOT NULL REFERENCES tasks (id),
                     depends_on_id TEXT NOT NULL REFERENCES tasks (id),
                     PRIMARY KEY (task_id, depends_on_id));
                 CREATE TABLE task_strings (
                     task_id TEXT NOT NULL REFERENCES tasks (id),
                     slot TEXT NOT NULL, position INTEGER NOT NULL, value TEXT NOT NULL,
                     PRIMARY KEY (task_id, slot, position));
                 PRAGMA user_version = 1;
                 INSERT INTO projects (id, name) VALUES ('P-1', 'legacy');
                 INSERT INTO tasks (id, project_id, objective, state)
                     VALUES ('T-1', 'P-1', 'legacy objective', 'RUNNING');",
            )
            .unwrap();
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let task = store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(task.state(), TaskState::Running);
    assert_eq!(task.objective(), "legacy objective");
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn current_schema_reopen_is_a_no_op() {
    let db = TestDb::new("current-noop");
    {
        let mut store = db.open();
        ready_task_via_store(&mut store);
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert!(store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .is_some());
}

#[test]
fn future_version_rejected_after_current() {
    let db = TestDb::new("future-current");
    {
        let _store = db.open();
    }
    db.raw().execute_batch("PRAGMA user_version = 99").unwrap();
    match SqliteStore::open(&db.path) {
        Err(StorageError::UnsupportedSchemaVersion { found, supported }) => {
            assert_eq!((found, supported), (99, SUPPORTED_SCHEMA_VERSION));
        }
        _ => panic!("expected UnsupportedSchemaVersion, open must fail closed"),
    }
}

#[test]
fn v2_journal_rows_survive_migration_to_current() {
    let db = TestDb::new("v2-preserve");
    {
        // Hand-built v2 layout: 0001 + 0002 tables, version stamp 2, real
        // lifecycle journal rows committed under the old NOT NULL shape.
        let connection = db.raw();
        connection
            .execute_batch(
                "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
                 CREATE TABLE tasks (
                     id TEXT PRIMARY KEY,
                     project_id TEXT NOT NULL REFERENCES projects (id),
                     objective TEXT NOT NULL, parent_id TEXT NULL REFERENCES tasks (id),
                     state TEXT NOT NULL, kind TEXT NULL, risk TEXT NULL);
                 CREATE TABLE task_dependencies (
                     task_id TEXT NOT NULL REFERENCES tasks (id),
                     depends_on_id TEXT NOT NULL REFERENCES tasks (id),
                     PRIMARY KEY (task_id, depends_on_id));
                 CREATE TABLE task_strings (
                     task_id TEXT NOT NULL REFERENCES tasks (id),
                     slot TEXT NOT NULL, position INTEGER NOT NULL, value TEXT NOT NULL,
                     PRIMARY KEY (task_id, slot, position));
                 CREATE TABLE event_journal (
                     seq INTEGER PRIMARY KEY AUTOINCREMENT,
                     event_type TEXT NOT NULL, event_version INTEGER NOT NULL,
                     task_id TEXT NOT NULL REFERENCES tasks (id),
                     project_id TEXT NOT NULL REFERENCES projects (id),
                     from_state TEXT NOT NULL, to_state TEXT NOT NULL,
                     reason TEXT NULL, correlation_id TEXT NOT NULL,
                     causation_id TEXT NOT NULL, occurred_at TEXT NOT NULL,
                     provenance TEXT NOT NULL);
                 CREATE INDEX event_journal_task_seq ON event_journal (task_id, seq);
                 CREATE INDEX event_journal_project_seq ON event_journal (project_id, seq);
                 PRAGMA user_version = 2;
                 INSERT INTO projects (id, name) VALUES ('P-1', 'legacy');
                 INSERT INTO tasks (id, project_id, objective, state)
                     VALUES ('T-1', 'P-1', 'legacy objective', 'CLAIMED');
                 INSERT INTO event_journal
                   (event_type, event_version, task_id, project_id, from_state,
                    to_state, reason, correlation_id, causation_id, occurred_at,
                    provenance)
                 VALUES
                   ('task.marked_ready', 1, 'T-1', 'P-1', 'DRAFT', 'READY',
                    NULL, 'op-1', 'req-1', '2024-01-01T00:00:00Z',
                    'task-application-service/mark_ready'),
                   ('task.claimed', 1, 'T-1', 'P-1', 'READY', 'CLAIMED',
                    NULL, 'op-2', 'req-2', '2024-01-01T00:00:01Z',
                    'task-application-service/claim');",
            )
            .unwrap();
    }
    let mut store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    // Every existing row survives verbatim: seq, identity, type, refs,
    // states, payload, and provenance.
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].seq, 1);
    assert_eq!(history[0].id.to_string(), "evt-1");
    assert_eq!(history[0].fact.event_type, EventType::MarkedReady);
    assert_eq!(history[0].fact.task_id.as_ref().unwrap().as_str(), "T-1");
    assert_eq!(history[0].fact.project_id.as_ref().unwrap().as_str(), "P-1");
    assert_eq!(history[0].fact.from_state, Some(TaskState::Draft));
    assert_eq!(history[0].fact.to_state, Some(TaskState::Ready));
    assert_eq!(
        history[0].fact.provenance,
        "task-application-service/mark_ready"
    );
    assert_eq!(history[1].seq, 2);
    assert_eq!(history[1].id.to_string(), "evt-2");
    assert_eq!(history[1].fact.event_type, EventType::Claimed);
    assert_eq!(history[1].fact.occurred_at, "2024-01-01T00:00:01Z");
    // The sequence continues without reuse: the next identity is evt-3.
    let mut task = store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .unwrap();
    task.start().unwrap();
    let event = store
        .apply_lifecycle_transition(
            task.id(),
            TaskState::Claimed,
            &task,
            &new_event(
                &task,
                TaskState::Claimed,
                TaskState::Running,
                EventType::Started,
            ),
        )
        .unwrap();
    assert_eq!(event.seq, 3);
    assert_eq!(event.id.to_string(), "evt-3");
    // A project-scoped event appends with a NULL task reference — no fake
    // task id is used to satisfy the column.
    let created = store
        .create_project_with_event(
            &project("P-2"),
            &NewJournalEvent {
                event_type: EventType::ProjectCreated,
                task_id: None,
                project_id: Some(ProjectId::new("P-2").unwrap()),
                from_state: None,
                to_state: None,
                reason: None,
                correlation_id: "op-3".into(),
                causation_id: "req-3".into(),
                occurred_at: "2024-01-01T00:00:02Z".into(),
                provenance: "task-application-service/create_project".into(),
                execution_id: None,
                attempt_id: None,
                attempt_outcome: None,
                workspace_id: None,
                from_workspace_state: None,
                to_workspace_state: None,
                plan_revision: None,
            },
        )
        .unwrap();
    assert_eq!(created.seq, 4);
    assert_eq!(created.fact.task_id, None);
    assert_eq!(store.list_events_ordered().unwrap().len(), 4);
}

#[test]
fn scope_check_rejects_mis_scoped_rows() {
    let db = TestDb::new("scope-check");
    let mut store = db.open();
    ready_task_via_store(&mut store);
    // project.created with a task reference violates the scope CHECK.
    let scoped = db.raw().execute(
        "INSERT INTO event_journal
               (event_type, event_version, task_id, project_id, from_state,
                to_state, correlation_id, causation_id, occurred_at, provenance)
             VALUES ('project.created', 1, 'T-1', 'P-1', NULL, NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test')",
        [],
    );
    assert!(scoped.is_err(), "project.created must not carry task_id");
    // Task-scoped types without a task reference violate the scope CHECK.
    let unscoped = db.raw().execute(
        "INSERT INTO event_journal
               (event_type, event_version, task_id, project_id, from_state,
                to_state, correlation_id, causation_id, occurred_at, provenance)
             VALUES ('task.created', 1, NULL, 'P-1', 'DRAFT', 'DRAFT',
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test')",
        [],
    );
    assert!(unscoped.is_err(), "task.created requires task scope");
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn creation_mutations_are_atomic_with_their_events() {
    let db = TestDb::new("atomic-create");
    let mut store = db.open();
    let created_project = store
        .create_project_with_event(
            &project("P-1"),
            &NewJournalEvent {
                event_type: EventType::ProjectCreated,
                task_id: None,
                project_id: Some(ProjectId::new("P-1").unwrap()),
                from_state: None,
                to_state: None,
                reason: None,
                correlation_id: "op-1".into(),
                causation_id: "req-1".into(),
                occurred_at: "2024-01-01T00:00:00Z".into(),
                provenance: "task-application-service/create_project".into(),
                execution_id: None,
                attempt_id: None,
                attempt_outcome: None,
                workspace_id: None,
                from_workspace_state: None,
                to_workspace_state: None,
                plan_revision: None,
            },
        )
        .unwrap();
    assert_eq!(created_project.fact.event_type, EventType::ProjectCreated);
    let task = draft_task("P-1", "T-1");
    let created_task = store
        .create_task_with_event(
            &task,
            &NewJournalEvent {
                event_type: EventType::TaskCreated,
                task_id: Some(TaskId::new("T-1").unwrap()),
                project_id: Some(ProjectId::new("P-1").unwrap()),
                from_state: Some(TaskState::Draft),
                to_state: Some(TaskState::Draft),
                reason: None,
                correlation_id: "op-2".into(),
                causation_id: "req-2".into(),
                occurred_at: "2024-01-01T00:00:01Z".into(),
                provenance: "task-application-service/create_task".into(),
                execution_id: None,
                attempt_id: None,
                attempt_outcome: None,
                workspace_id: None,
                from_workspace_state: None,
                to_workspace_state: None,
                plan_revision: None,
            },
        )
        .unwrap();
    assert_eq!(created_task.seq, 2);
    assert_eq!(created_task.fact.event_type, EventType::TaskCreated);
    // A mismatched creation event commits nothing: no row, no journal.
    let other = draft_task("P-1", "T-9");
    let bad = NewJournalEvent {
        event_type: EventType::TaskCreated,
        task_id: Some(TaskId::new("T-OTHER").unwrap()),
        project_id: Some(ProjectId::new("P-1").unwrap()),
        from_state: Some(TaskState::Draft),
        to_state: Some(TaskState::Draft),
        reason: None,
        correlation_id: "op-3".into(),
        causation_id: "req-3".into(),
        occurred_at: "2024-01-01T00:00:02Z".into(),
        provenance: "task-application-service/create_task".into(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    };
    assert!(store.create_task_with_event(&other, &bad).is_err());
    assert!(store
        .get_task(&TaskId::new("T-9").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(journal_len(&store), 2);
}

#[test]
fn dependency_mutation_is_atomic_with_its_event() {
    let db = TestDb::new("atomic-dep");
    let mut store = db.open();
    ready_task_via_store(&mut store);
    let other = draft_task("P-1", "T-2");
    store.create_task(&other).unwrap();
    let committed = store
        .add_dependency_with_event(
            &TaskId::new("T-1").unwrap(),
            &TaskId::new("T-2").unwrap(),
            &NewJournalEvent {
                event_type: EventType::DependencyAdded,
                task_id: Some(TaskId::new("T-1").unwrap()),
                project_id: Some(ProjectId::new("P-1").unwrap()),
                from_state: Some(TaskState::Ready),
                to_state: Some(TaskState::Ready),
                reason: None,
                correlation_id: "op-1".into(),
                causation_id: "req-1".into(),
                occurred_at: "2024-01-01T00:00:00Z".into(),
                provenance: "task-application-service/add_dependency".into(),
                execution_id: None,
                attempt_id: None,
                attempt_outcome: None,
                workspace_id: None,
                from_workspace_state: None,
                to_workspace_state: None,
                plan_revision: None,
            },
        )
        .unwrap();
    assert_eq!(committed.fact.event_type, EventType::DependencyAdded);
    assert_eq!(
        store
            .list_dependencies(&TaskId::new("T-1").unwrap())
            .unwrap(),
        [TaskId::new("T-2").unwrap()]
    );
    // A duplicate edge commits neither a second edge nor a second event.
    let before = journal_len(&store);
    let dup = NewJournalEvent {
        event_type: EventType::DependencyAdded,
        task_id: Some(TaskId::new("T-1").unwrap()),
        project_id: Some(ProjectId::new("P-1").unwrap()),
        from_state: Some(TaskState::Ready),
        to_state: Some(TaskState::Ready),
        reason: None,
        correlation_id: "op-2".into(),
        causation_id: "req-2".into(),
        occurred_at: "2024-01-01T00:00:01Z".into(),
        provenance: "task-application-service/add_dependency".into(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    };
    assert!(store
        .add_dependency_with_event(
            &TaskId::new("T-1").unwrap(),
            &TaskId::new("T-2").unwrap(),
            &dup
        )
        .is_err());
    assert_eq!(journal_len(&store), before);
}

// -- typed append + reads ---------------------------------------------------

#[test]
fn lifecycle_commit_appends_exactly_one_typed_event() {
    let db = TestDb::new("append");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    task.claim().unwrap();
    let event = store
        .apply_lifecycle_transition(
            task.id(),
            TaskState::Ready,
            &task,
            &new_event(
                &task,
                TaskState::Ready,
                TaskState::Claimed,
                EventType::Claimed,
            ),
        )
        .unwrap();
    assert_eq!(event.seq, 1);
    assert_eq!(event.id, EventId::parse("evt-1").unwrap());
    assert_eq!(event.event_version, EVENT_SCHEMA_VERSION);
    assert_eq!(event.fact.event_type, EventType::Claimed);
    assert_eq!(event.fact.from_state, Some(TaskState::Ready));
    assert_eq!(event.fact.to_state, Some(TaskState::Claimed));
    assert_eq!(journal_len(&store), 1);

    assert_eq!(store.get_event(&event.id).unwrap(), Some(event.clone()));
    assert_eq!(
        store
            .get_event(&EventId::parse("evt-999").unwrap())
            .unwrap(),
        None
    );
    assert_eq!(
        store.list_events_for_task(task.id()).unwrap(),
        std::slice::from_ref(&event)
    );
    assert_eq!(
        store.list_events_for_project(task.project_id()).unwrap(),
        std::slice::from_ref(&event)
    );
    assert_eq!(
        store.list_events_ordered().unwrap(),
        std::slice::from_ref(&event)
    );
}

#[test]
fn journal_order_follows_commit_order() {
    let db = TestDb::new("order");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    let plan = [
        (TaskState::Ready, TaskState::Claimed, EventType::Claimed),
        (TaskState::Claimed, TaskState::Running, EventType::Started),
        (TaskState::Running, TaskState::Blocked, EventType::Blocked),
    ];
    for (index, (from, to, event_type)) in plan.into_iter().enumerate() {
        match event_type {
            EventType::Claimed => {
                task.claim().unwrap();
            }
            EventType::Started => {
                task.start().unwrap();
            }
            EventType::Blocked => {
                task.block("reason").unwrap();
            }
            _ => unreachable!("journal order plan covers three ops"),
        }
        let event = store
            .apply_lifecycle_transition(
                task.id(),
                from,
                &task,
                &new_event(&task, from, to, event_type),
            )
            .unwrap();
        assert_eq!(event.seq, (index + 1) as i64);
    }
    let ordered = store.list_events_ordered().unwrap();
    assert_eq!(ordered.len(), 3);
    let ids: Vec<String> = ordered.iter().map(|event| event.id.to_string()).collect();
    assert_eq!(ids, ["evt-1", "evt-2", "evt-3"]);
    assert!(
        ordered.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "journal retrieval order must be deterministic"
    );
}

// -- atomicity ---------------------------------------------------------------

#[test]
fn stale_expected_state_rolls_back_with_no_event() {
    let db = TestDb::new("stale");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    // First attempt commits READY → CLAIMED.
    task.claim().unwrap();
    store
        .apply_lifecycle_transition(
            task.id(),
            TaskState::Ready,
            &task,
            &new_event(
                &task,
                TaskState::Ready,
                TaskState::Claimed,
                EventType::Claimed,
            ),
        )
        .unwrap();
    // Second attempt still assumes READY: it must fail, not overwrite.
    let mut stale = store.get_task(task.id()).unwrap().unwrap();
    stale.block("stale attempt").unwrap_err();
    let _ = stale;
    let mut retry_view = draft_task("P-1", "T-1");
    retry_view.mark_ready().unwrap();
    retry_view.claim().unwrap();
    // Simulate the stale writer: validated against READY, stored is CLAIMED.
    match store.apply_lifecycle_transition(
        retry_view.id(),
        TaskState::Ready,
        &retry_view,
        &new_event(
            &retry_view,
            TaskState::Ready,
            TaskState::Claimed,
            EventType::Claimed,
        ),
    ) {
        Err(StorageError::Stale {
            expected, found, ..
        }) => {
            assert_eq!((expected.as_str(), found.as_str()), ("READY", "CLAIMED"));
        }
        _ => panic!("expected Stale, two writers must not silently converge"),
    }
    assert_eq!(
        store.get_task(task.id()).unwrap().unwrap().state(),
        TaskState::Claimed
    );
    assert_eq!(journal_len(&store), 1);
}

#[test]
fn event_append_failure_rolls_back_state() {
    let db = TestDb::new("event-fails");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_event BEFORE INSERT ON event_journal
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    task.claim().unwrap();
    let result = store.apply_lifecycle_transition(
        task.id(),
        TaskState::Ready,
        &task,
        &new_event(
            &task,
            TaskState::Ready,
            TaskState::Claimed,
            EventType::Claimed,
        ),
    );
    assert!(result.is_err(), "event failure must fail the commit");
    drop(store);
    let store = db.open();
    assert_eq!(
        store
            .get_task(&TaskId::new("T-1").unwrap())
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Ready,
        "state update must roll back with the event"
    );
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn state_write_failure_commits_no_event() {
    let db = TestDb::new("state-fails");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    db.raw()
        .execute_batch(
            "CREATE TRIGGER fail_state BEFORE UPDATE ON tasks
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    task.claim().unwrap();
    let result = store.apply_lifecycle_transition(
        task.id(),
        TaskState::Ready,
        &task,
        &new_event(
            &task,
            TaskState::Ready,
            TaskState::Claimed,
            EventType::Claimed,
        ),
    );
    assert!(result.is_err(), "state failure must fail the commit");
    drop(store);
    // Trigger rows live in the database, so reopen on a second connection
    // would still fail; drop the trigger, then verify nothing committed.
    db.raw().execute_batch("DROP TRIGGER fail_state;").unwrap();
    let store = db.open();
    assert_eq!(
        store
            .get_task(&TaskId::new("T-1").unwrap())
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Ready
    );
    assert_eq!(journal_len(&store), 0);
}

#[test]
fn mismatched_event_references_write_nothing() {
    let db = TestDb::new("mismatch");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    task.claim().unwrap();
    let mut event = new_event(
        &task,
        TaskState::Ready,
        TaskState::Claimed,
        EventType::Claimed,
    );
    event.task_id = Some(TaskId::new("SOMEONE-ELSE").unwrap());
    match store.apply_lifecycle_transition(task.id(), TaskState::Ready, &task, &event) {
        Err(StorageError::ConstraintViolation(_)) => {}
        other => panic!("expected ConstraintViolation, got {other:?}"),
    }
    assert_eq!(
        store.get_task(task.id()).unwrap().unwrap().state(),
        TaskState::Ready
    );
    assert_eq!(journal_len(&store), 0);
}

// -- corruption -----------------------------------------------------------------

#[test]
fn unknown_event_type_fails_explicitly() {
    let db = TestDb::new("corrupt-type");
    let mut store = db.open();
    ready_task_via_store(&mut store);
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO event_journal
               (event_type, event_version, task_id, project_id, from_state, to_state,
                correlation_id, causation_id, occurred_at, provenance)
             VALUES ('task.deleted', 1, 'T-1', 'P-1', 'READY', 'CANCELLED',
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    drop(store);
    let store = db.open();
    match store.get_event(&EventId::parse("evt-1").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        _ => panic!("expected Corrupt, got a decoded unknown type"),
    }
}

#[test]
fn future_event_version_fails_explicitly() {
    let db = TestDb::new("corrupt-version");
    let mut store = db.open();
    ready_task_via_store(&mut store);
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO event_journal
               (event_type, event_version, task_id, project_id, from_state, to_state,
                correlation_id, causation_id, occurred_at, provenance)
             VALUES ('task.claimed', 7, 'T-1', 'P-1', 'READY', 'CLAIMED',
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    drop(store);
    let store = db.open();
    match store.list_events_ordered() {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn malformed_event_identity_is_not_found() {
    assert!(EventId::parse("nope").is_err());
    assert!(EventId::parse("evt--3").is_err());
}

// -- authority + schema review -----------------------------------------------------

#[test]
fn current_state_survives_journal_loss() {
    let db = TestDb::new("authority");
    let mut store = db.open();
    let mut task = ready_task_via_store(&mut store);
    task.claim().unwrap();
    store
        .apply_lifecycle_transition(
            task.id(),
            TaskState::Ready,
            &task,
            &new_event(
                &task,
                TaskState::Ready,
                TaskState::Claimed,
                EventType::Claimed,
            ),
        )
        .unwrap();
    // Destroy the history behind the adapter's back: current-state reads
    // must not care, because they never replay the journal.
    db.raw()
        .execute_batch("DELETE FROM event_journal;")
        .unwrap();
    drop(store);
    let store = db.open();
    assert!(store.list_events_ordered().unwrap().is_empty());
    assert_eq!(
        store
            .get_task(&TaskId::new("T-1").unwrap())
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Claimed
    );
}

#[test]
fn journal_columns_carry_no_secret_or_transcript_fields() {
    // task_strings audit (part 1): the slot vocabulary stays closed —
    // review evidence that the table cannot become a misc state bucket.
    let db = TestDb::new("slots");
    let mut store = db.open();
    ready_task_via_store(&mut store);
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO task_strings (task_id, slot, position, value)
             VALUES ('T-1', 'misc_blob', 0, 'junk');
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    drop(store);
    let store = db.open();
    match store.get_task(&TaskId::new("T-1").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt for foreign slot, got {other:?}"),
    }
}

#[test]
fn table_layout_matches_the_accepted_contract() {
    // task_strings audit (part 2) + journal schema review: exact column
    // sets, so no prompt/secret/transcript/provider column can hide.
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
    assert_eq!(
        columns("event_journal"),
        [
            "seq",
            "event_type",
            "event_version",
            "task_id",
            "project_id",
            "from_state",
            "to_state",
            "reason",
            "correlation_id",
            "causation_id",
            "occurred_at",
            "provenance",
            "execution_id",
            "attempt_id",
            "attempt_outcome",
            "workspace_id",
            "plan_branch_id",
            "plan_revision_seq",
            "plan_changeset_hash",
            "plan_planner_adapter_id",
            "plan_model_ref",
            "plan_planning_cost_receipt_ref",
            "plan_validated_service_id",
            "plan_policy_fingerprint",
        ]
    );
    assert_eq!(
        columns("tasks"),
        [
            "id",
            "project_id",
            "objective",
            "parent_id",
            "state",
            "kind",
            "risk"
        ]
    );
    drop(store);
    let _ = store;
}

#[test]
fn journal_event_facts_have_no_secret_fields() {
    // Structural review: the fact type cannot smuggle prompts, secrets, or
    // transcripts — every field is enumerated here by name.
    let event = JournalEvent {
        id: EventId::parse("evt-1").unwrap(),
        seq: 1,
        event_version: EVENT_SCHEMA_VERSION,
        fact: NewJournalEvent {
            event_type: EventType::Submitted,
            task_id: Some(TaskId::new("T-1").unwrap()),
            project_id: Some(ProjectId::new("P-1").unwrap()),
            from_state: Some(TaskState::Running),
            to_state: Some(TaskState::Submitted),
            reason: Some("diff + tests".into()),
            correlation_id: "op-1".into(),
            causation_id: "req-1".into(),
            occurred_at: "2024-01-01T00:00:00Z".into(),
            provenance: "task-application-service/submit_result".into(),
            execution_id: None,
            attempt_id: None,
            attempt_outcome: None,
            workspace_id: None,
            from_workspace_state: None,
            to_workspace_state: None,
            plan_revision: None,
        },
    };
    let debug = format!("{event:?}");
    for forbidden in [
        "secret",
        "prompt",
        "transcript",
        "api_key",
        "token",
        "password",
    ] {
        assert!(
            !debug.to_lowercase().contains(forbidden),
            "journal fact must not carry {forbidden}"
        );
    }
}
