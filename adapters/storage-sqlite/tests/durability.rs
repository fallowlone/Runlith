//! G2 durability and contract tests for the SQLite adapter.
//!
//! Real files only (no `:memory:` reliance): every test owns a temporary
//! database file, and the core proof drops the store and reopens the same
//! file. Raw SQL appears solely in corruption fixtures; production paths
//! stay typed.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use storage_sqlite::{SqliteStore, StorageError, SUPPORTED_SCHEMA_VERSION};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Test-owned database file, removed on drop. Never a user database.
struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-storage-test-{}-{suite}-{id}",
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

fn stored_project(store: &mut SqliteStore, id: &str) -> Project {
    let project = project(id);
    store.create_project(&project).unwrap();
    project
}

// -- migration ----------------------------------------------------------

#[test]
fn fresh_database_migrates_to_current() {
    let db = TestDb::new("fresh");
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 11);
}

#[test]
fn migration_is_idempotent_and_preserves_data() {
    let db = TestDb::new("idempotent");
    let project = {
        let mut store = db.open();
        let project = stored_project(&mut store, "P-1");
        store.create_task(&draft_task("P-1", "T-1")).unwrap();
        project
    };
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(store.get_project(project.id()).unwrap(), Some(project));
    assert!(store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .is_some());
}

#[test]
fn future_schema_version_fails_closed() {
    let db = TestDb::new("future");
    {
        let _store = db.open();
    }
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 99")
            .unwrap();
    }
    let opened = SqliteStore::open(&db.path);
    assert!(
        matches!(
            opened,
            Err(StorageError::UnsupportedSchemaVersion { found: 99, .. })
        ),
        "expected UnsupportedSchemaVersion(99), open must fail closed"
    );
}

#[test]
fn foreign_keys_are_actually_enabled() {
    let db = TestDb::new("fk");
    let store = db.open();
    assert!(store.foreign_keys_enabled().unwrap());
}

// -- project round-trip ---------------------------------------------------

#[test]
fn project_create_get_list_round_trip() {
    let db = TestDb::new("projects");
    let mut store = db.open();
    assert!(store.list_projects().unwrap().is_empty());
    let a = stored_project(&mut store, "P-A");
    let b = stored_project(&mut store, "P-B");
    assert_eq!(store.get_project(a.id()).unwrap(), Some(a.clone()));
    assert_eq!(store.get_project(b.id()).unwrap(), Some(b.clone()));
    assert_eq!(
        store
            .get_project(&ProjectId::new("MISSING").unwrap())
            .unwrap(),
        None
    );
    assert_eq!(store.list_projects().unwrap(), [a, b]);
}

#[test]
fn duplicate_project_id_is_rejected() {
    let db = TestDb::new("dup-project");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    match store.create_project(&project("P-1")) {
        Err(StorageError::Duplicate(_)) => {}
        other => panic!("expected Duplicate, got {other:?}"),
    }
}

// -- task round-trip ------------------------------------------------------

#[test]
fn task_create_get_list_round_trip() {
    let db = TestDb::new("tasks");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    let mut task = draft_task("P-1", "T-1");
    task.mark_ready().unwrap();
    task.add_acceptance("compiles").unwrap();
    store.create_task(&task).unwrap();
    let reloaded = store.get_task(task.id()).unwrap().expect("stored task");
    assert_eq!(reloaded, task);
    assert_eq!(
        store.get_task(&TaskId::new("MISSING").unwrap()).unwrap(),
        None
    );
    assert_eq!(
        store.list_tasks_for_project(task.project_id()).unwrap(),
        [task]
    );
}

#[test]
fn duplicate_task_id_is_rejected() {
    let db = TestDb::new("dup-task");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    store.create_task(&draft_task("P-1", "T-1")).unwrap();
    match store.create_task(&draft_task("P-1", "T-1")) {
        Err(StorageError::Duplicate(_)) => {}
        other => panic!("expected Duplicate, got {other:?}"),
    }
}

#[test]
fn task_with_nonexistent_project_is_rejected() {
    let db = TestDb::new("no-project");
    let mut store = db.open();
    match store.create_task(&draft_task("GHOST", "T-1")) {
        Err(StorageError::ConstraintViolation(_)) => {}
        other => panic!("expected ConstraintViolation, got {other:?}"),
    }
    assert!(store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .is_none());
}

#[test]
fn save_task_persists_domain_operation_outcome() {
    let db = TestDb::new("save");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    let mut task = draft_task("P-1", "T-1");
    store.create_task(&task).unwrap();
    task.mark_ready().unwrap();
    task.claim().unwrap();
    task.start().unwrap();
    store.save_task(&task).unwrap();
    let reloaded = store.get_task(task.id()).unwrap().expect("stored task");
    assert_eq!(reloaded.state(), TaskState::Running);
    assert_eq!(reloaded, task);
}

// -- all 12 states --------------------------------------------------------

/// Builds one task per canonical state using only legal domain operations,
/// except `FAILED` / `CANCELLED`, whose incoming edges are intentionally
/// undecided: those two go through the validated restore path (the smallest
/// legitimate adapter boundary), never through new transition authority.
fn task_in_state(project: &str, id: &str, state: TaskState) -> Task {
    let mut task = draft_task(project, id);
    match state {
        TaskState::Draft => {}
        TaskState::Ready => {
            task.mark_ready().unwrap();
        }
        TaskState::Claimed => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
        }
        TaskState::Running => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
        }
        TaskState::Blocked => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
            task.block("impediment").unwrap();
        }
        TaskState::Submitted => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
            task.submit_result("attempt").unwrap();
        }
        TaskState::AwaitingReview => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
            task.submit_result("attempt").unwrap();
            task.route_to_review().unwrap();
        }
        TaskState::Verifying => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
            task.submit_result("attempt").unwrap();
            task.route_to_review().unwrap();
            task.begin_verification().unwrap();
        }
        TaskState::RetryReady => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
            task.submit_result("attempt").unwrap();
            task.route_to_review().unwrap();
            task.begin_verification().unwrap();
            task.verification_failed("REJECTED").unwrap();
        }
        TaskState::Completed => {
            task.mark_ready().unwrap();
            task.claim().unwrap();
            task.start().unwrap();
            task.submit_result("attempt").unwrap();
            task.route_to_review().unwrap();
            task.begin_verification().unwrap();
            task.verification_succeeded("VERIFIED").unwrap();
        }
        TaskState::Failed | TaskState::Cancelled => {
            let mut snapshot = task.snapshot();
            snapshot.state = state;
            task = Task::restore(snapshot).unwrap();
        }
    }
    assert_eq!(task.state(), state);
    task
}

#[test]
fn all_twelve_states_round_trip_exactly() {
    let db = TestDb::new("states");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    let mut expected = Vec::new();
    for (index, state) in TaskState::all().into_iter().enumerate() {
        let id = format!("T-{index:02}");
        let task = task_in_state("P-1", &id, state);
        store.create_task(&task).unwrap();
        expected.push(task);
    }
    for task in &expected {
        let reloaded = store.get_task(task.id()).unwrap().expect("stored task");
        assert_eq!(reloaded.state(), task.state());
        assert_eq!(reloaded, *task);
    }
}

// -- dependencies ---------------------------------------------------------

#[test]
fn dependency_survives_reopen() {
    let db = TestDb::new("dep-reopen");
    {
        let mut store = db.open();
        stored_project(&mut store, "P-1");
        store.create_task(&draft_task("P-1", "A")).unwrap();
        store.create_task(&draft_task("P-1", "B")).unwrap();
        store
            .add_dependency(&TaskId::new("A").unwrap(), &TaskId::new("B").unwrap())
            .unwrap();
    }
    let store = db.open();
    assert_eq!(
        store.list_dependencies(&TaskId::new("A").unwrap()).unwrap(),
        [TaskId::new("B").unwrap()]
    );
    let reloaded = store.get_task(&TaskId::new("A").unwrap()).unwrap().unwrap();
    assert_eq!(reloaded.depends_on(), [TaskId::new("B").unwrap()]);
}

#[test]
fn dependency_with_nonexistent_task_is_rejected() {
    let db = TestDb::new("dep-missing");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    store.create_task(&draft_task("P-1", "A")).unwrap();
    match store.add_dependency(&TaskId::new("A").unwrap(), &TaskId::new("GHOST").unwrap()) {
        Err(StorageError::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
    assert!(store
        .list_dependencies(&TaskId::new("A").unwrap())
        .unwrap()
        .is_empty());
}

#[test]
fn duplicate_dependency_is_rejected() {
    let db = TestDb::new("dep-dupe");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    store.create_task(&draft_task("P-1", "A")).unwrap();
    store.create_task(&draft_task("P-1", "B")).unwrap();
    store
        .add_dependency(&TaskId::new("A").unwrap(), &TaskId::new("B").unwrap())
        .unwrap();
    match store.add_dependency(&TaskId::new("A").unwrap(), &TaskId::new("B").unwrap()) {
        Err(StorageError::InvalidGraph(_)) => {}
        other => panic!("expected InvalidGraph, got {other:?}"),
    }
    assert_eq!(
        store
            .list_dependencies(&TaskId::new("A").unwrap())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn self_dependency_is_rejected_before_persistence() {
    let db = TestDb::new("dep-self");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    store.create_task(&draft_task("P-1", "A")).unwrap();
    match store.add_dependency(&TaskId::new("A").unwrap(), &TaskId::new("A").unwrap()) {
        Err(StorageError::InvalidGraph(_)) => {}
        other => panic!("expected InvalidGraph, got {other:?}"),
    }
    assert!(store
        .list_dependencies(&TaskId::new("A").unwrap())
        .unwrap()
        .is_empty());
}

#[test]
fn cross_project_dependency_is_rejected_before_persistence() {
    let db = TestDb::new("dep-cross");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    stored_project(&mut store, "P-2");
    store.create_task(&draft_task("P-1", "A")).unwrap();
    store.create_task(&draft_task("P-2", "F")).unwrap();
    match store.add_dependency(&TaskId::new("A").unwrap(), &TaskId::new("F").unwrap()) {
        Err(StorageError::InvalidGraph(_)) => {}
        other => panic!("expected InvalidGraph, got {other:?}"),
    }
    assert!(store
        .list_dependencies(&TaskId::new("A").unwrap())
        .unwrap()
        .is_empty());
}

#[test]
fn cycle_is_rejected_before_persistence() {
    let db = TestDb::new("dep-cycle");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    for id in ["A", "B", "C"] {
        store.create_task(&draft_task("P-1", id)).unwrap();
    }
    let edge = |from: &str, to: &str| (TaskId::new(from).unwrap(), TaskId::new(to).unwrap());
    let (a, b) = edge("A", "B");
    let (b2, c) = edge("B", "C");
    let (c2, a2) = edge("C", "A");
    store.add_dependency(&a, &b).unwrap();
    store.add_dependency(&b2, &c).unwrap();
    match store.add_dependency(&c2, &a2) {
        Err(StorageError::InvalidGraph(_)) => {}
        other => panic!("expected InvalidGraph, got {other:?}"),
    }
    // The refused edge left no trace.
    assert!(store.list_dependencies(&c2).unwrap().is_empty());
    let graph = store
        .load_project_graph(&ProjectId::new("P-1").unwrap())
        .unwrap();
    assert_eq!(graph.len(), 3);
}

#[test]
fn remove_dependency_is_idempotent() {
    let db = TestDb::new("dep-remove");
    let mut store = db.open();
    stored_project(&mut store, "P-1");
    store.create_task(&draft_task("P-1", "A")).unwrap();
    store.create_task(&draft_task("P-1", "B")).unwrap();
    let (a, b) = (TaskId::new("A").unwrap(), TaskId::new("B").unwrap());
    store.add_dependency(&a, &b).unwrap();
    store.remove_dependency(&a, &b).unwrap();
    assert!(store.list_dependencies(&a).unwrap().is_empty());
    store.remove_dependency(&a, &b).unwrap();
}

// -- corruption fixtures (raw SQL only) ------------------------------------

#[test]
fn unknown_persisted_state_is_corruption() {
    let db = TestDb::new("corrupt-state");
    {
        let mut store = db.open();
        stored_project(&mut store, "P-1");
        store.create_task(&draft_task("P-1", "T-1")).unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
             UPDATE tasks SET state = 'BOGUS' WHERE id = 'T-1';
             PRAGMA ignore_check_constraints = OFF;",
            )
            .unwrap();
    }
    let store = db.open();
    match store.get_task(&TaskId::new("T-1").unwrap()) {
        Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected Corrupt, got {other:?}"),
    }
}

#[test]
fn invalid_persisted_identifiers_fail_reconstruction() {
    let db = TestDb::new("corrupt-ids");
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
             PRAGMA user_version = 1;
             CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS tasks (
                 id TEXT PRIMARY KEY, project_id TEXT NOT NULL, objective TEXT NOT NULL,
                 parent_id TEXT NULL, state TEXT NOT NULL, kind TEXT NULL, risk TEXT NULL);
             INSERT INTO projects (id, name) VALUES ('', 'blank id');
             INSERT INTO tasks (id, project_id, objective, state)
                 VALUES ('T-1', '', 'objective', 'DRAFT');",
            )
            .unwrap();
    }
    let store = db.open();
    // Blank stored project id cannot rebuild a validated ProjectId.
    assert!(store.list_projects().is_err());
}

#[test]
fn broken_reference_without_fk_fails_explicitly() {
    let db = TestDb::new("corrupt-ref");
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
             PRAGMA user_version = 1;
             CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS tasks (
                 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects (id),
                 objective TEXT NOT NULL, parent_id TEXT NULL,
                 state TEXT NOT NULL, kind TEXT NULL, risk TEXT NULL);
             CREATE TABLE IF NOT EXISTS task_dependencies (
                 task_id TEXT NOT NULL REFERENCES tasks (id),
                 depends_on_id TEXT NOT NULL REFERENCES tasks (id),
                 PRIMARY KEY (task_id, depends_on_id));
             CREATE TABLE IF NOT EXISTS task_strings (
                 task_id TEXT NOT NULL REFERENCES tasks (id),
                 slot TEXT NOT NULL, position INTEGER NOT NULL, value TEXT NOT NULL,
                 PRIMARY KEY (task_id, slot, position));
             INSERT INTO tasks (id, project_id, objective, state)
                 VALUES ('T-1', 'GHOST', 'objective', 'DRAFT');",
            )
            .unwrap();
    }
    let store = db.open();
    // The row references a project that does not exist: the adapter must
    // fail explicitly rather than hand back a task in a phantom project.
    // (project_id '' / unknown ids fail id validation first.)
    match store.get_task(&TaskId::new("T-1").unwrap()) {
        Err(StorageError::DomainReconstruction(_)) | Err(StorageError::Corrupt(_)) => {}
        other => panic!("expected reconstruction failure, got {other:?}"),
    }
}

// -- durability core ---------------------------------------------------------

#[test]
fn restart_preserves_complete_project_graph() {
    let db = TestDb::new("durability");
    let expected_tasks = {
        let mut store = db.open();
        stored_project(&mut store, "P-1");
        let mut design = draft_task("P-1", "DESIGN");
        design.mark_ready().unwrap();
        design.add_acceptance("api sketched").unwrap();
        let mut build = draft_task("P-1", "BUILD");
        build.add_acceptance("compiles").unwrap();
        build.add_acceptance("tests pass").unwrap();
        store.create_task(&design).unwrap();
        store.create_task(&build).unwrap();
        store
            .add_dependency(
                &TaskId::new("BUILD").unwrap(),
                &TaskId::new("DESIGN").unwrap(),
            )
            .unwrap();
        // State change after creation persists through save.
        design.claim().unwrap();
        store.save_task(&design).unwrap();
        vec![
            store
                .get_task(&TaskId::new("DESIGN").unwrap())
                .unwrap()
                .unwrap(),
            store
                .get_task(&TaskId::new("BUILD").unwrap())
                .unwrap()
                .unwrap(),
        ]
    };
    // The store (and its connection) is dropped at block end; the same
    // file is reopened below and the full graph must match exactly.
    let store = db.open();
    let projects = store.list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id().as_str(), "P-1");
    let graph = store
        .load_project_graph(&ProjectId::new("P-1").unwrap())
        .unwrap();
    assert_eq!(graph.len(), 2);
    let design = graph
        .iter()
        .find(|task| task.id().as_str() == "DESIGN")
        .unwrap();
    let build = graph
        .iter()
        .find(|task| task.id().as_str() == "BUILD")
        .unwrap();
    assert_eq!(design.state(), TaskState::Claimed);
    assert_eq!(design.acceptance(), ["api sketched"]);
    assert!(design.depends_on().is_empty());
    assert_eq!(build.depends_on(), [TaskId::new("DESIGN").unwrap()]);
    assert_eq!(build.acceptance(), ["compiles", "tests pass"]);
    // Exact whole-graph equality against the pre-restart image.
    let mut expected = expected_tasks;
    expected.sort_by(|a, b| a.id().cmp(b.id()));
    assert_eq!(graph, expected);
}
