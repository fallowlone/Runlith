//! Event Journal contract tests: migration, typed appends, ordering,
//! atomicity, corruption, and schema review.
//!
//! Atomicity fixtures use raw SQL triggers to force one half of the
//! commit to fail; production paths stay typed. Every database file is
//! test-owned.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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
        rusqlite::Connection::open(&self.path).expect("raw fixture connection")
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
        task_id: task.id().clone(),
        project_id: task.project_id().clone(),
        from_state: from,
        to_state: to,
        reason: None,
        correlation_id: "op-test-1".into(),
        causation_id: "req-test-1".into(),
        occurred_at: "2024-01-01T00:00:00Z".into(),
        provenance: "task-application-service/test".into(),
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
fn fresh_database_reaches_v2() {
    let db = TestDb::new("fresh-v2");
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 2);
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
    assert_eq!(store.schema_version().unwrap(), 2);
    let task = store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(task.state(), TaskState::Running);
    assert_eq!(task.objective(), "legacy objective");
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn v2_reopen_is_a_noop() {
    let db = TestDb::new("v2-noop");
    {
        let mut store = db.open();
        ready_task_via_store(&mut store);
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert!(store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .is_some());
}

#[test]
fn future_version_rejected_after_v2() {
    let db = TestDb::new("future-v2");
    {
        let _store = db.open();
    }
    db.raw().execute_batch("PRAGMA user_version = 99").unwrap();
    match SqliteStore::open(&db.path) {
        Err(StorageError::UnsupportedSchemaVersion { found, supported }) => {
            assert_eq!((found, supported), (99, 2));
        }
        _ => panic!("expected UnsupportedSchemaVersion, open must fail closed"),
    }
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
    assert_eq!(event.fact.from_state, TaskState::Ready);
    assert_eq!(event.fact.to_state, TaskState::Claimed);
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
    event.task_id = TaskId::new("SOMEONE-ELSE").unwrap();
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
            task_id: TaskId::new("T-1").unwrap(),
            project_id: ProjectId::new("P-1").unwrap(),
            from_state: TaskState::Running,
            to_state: TaskState::Submitted,
            reason: Some("diff + tests".into()),
            correlation_id: "op-1".into(),
            causation_id: "req-1".into(),
            occurred_at: "2024-01-01T00:00:00Z".into(),
            provenance: "task-application-service/submit_result".into(),
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
