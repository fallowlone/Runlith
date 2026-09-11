//! Durable WorkspaceLease ownership tests: lease CRUD and guarded
//! transitions through the [`workspace::WorkspaceStore`] port, journal
//! scope enforcement (write path, SQL CHECK backstop, read-path decode),
//! corruption matrix, v4→v6 migration preservation, and the no-dirtiness
//! invariant. Every database file is test-owned.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{Execution, ExecutionId};
use storage_sqlite::{EventType, NewJournalEvent, SqliteStore, StorageError, EVENT_SCHEMA_VERSION};
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{
    BranchRef, LeaseLifecycle, RepositoryRef, Revision, WorkspaceEventKind, WorkspaceFailure,
    WorkspaceId, WorkspaceJournalEvidence, WorkspaceLease, WorkspaceStore,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-workspace-test-{}-{suite}-{id}",
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

fn seed_task(store: &mut SqliteStore, project_id: &str, id: &str) -> Task {
    store.create_project(&project(project_id)).unwrap();
    let task = draft_task(project_id, id);
    store.create_task(&task).unwrap();
    task
}

fn seed_execution(store: &mut SqliteStore, task: &Task, exec_id: &str) -> Execution {
    let execution = Execution::begin(ExecutionId::new(exec_id).unwrap(), task.id().clone());
    let event = NewJournalEvent {
        event_type: EventType::ExecutionCreated,
        task_id: Some(task.id().clone()),
        project_id: task.project_id().clone(),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "op-exec-1".into(),
        causation_id: "req-exec-1".into(),
        occurred_at: "2024-01-01T00:00:00Z".into(),
        provenance: "runtime-application-service/test".into(),
        execution_id: Some(execution.id().clone()),
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
    };
    store
        .create_execution_with_event(&execution, &event)
        .unwrap();
    execution
}

fn lease(ws: &str, exec: &str) -> WorkspaceLease {
    WorkspaceLease::allocate(
        WorkspaceId::new(ws).unwrap(),
        RepositoryRef::new("repo-local:test").unwrap(),
        ExecutionId::new(exec).unwrap(),
        Revision::new("a".repeat(40)).unwrap(),
        BranchRef::new(format!("refs/heads/agent/{exec}")).unwrap(),
        format!("/tmp/runlith-ws-test/{ws}"),
    )
    .unwrap()
}

fn evidence(
    kind: WorkspaceEventKind,
    task: &Task,
    exec: &Execution,
    ws: &str,
) -> WorkspaceJournalEvidence {
    WorkspaceJournalEvidence {
        kind,
        task_id: task.id().clone(),
        project_id: task.project_id().clone(),
        execution_id: exec.id().clone(),
        workspace_id: WorkspaceId::new(ws).unwrap(),
        correlation_id: "op-ws-1".into(),
        causation_id: "req-ws-1".into(),
        occurred_at: "2024-01-01T00:00:00Z".into(),
        provenance: "workspace-application-service/test".into(),
    }
}

fn created_events(store: &SqliteStore, ws: &str) -> usize {
    store
        .list_events_ordered()
        .unwrap()
        .iter()
        .filter(|event| {
            event.fact.event_type == EventType::WorkspaceCreated
                && event.fact.workspace_id.as_ref().map(|id| id.as_str()) == Some(ws)
        })
        .count()
}

// -- lease CRUD and guards --------------------------------------------------

#[test]
fn lease_crud_round_trips_through_the_port() {
    let db = TestDb::new("crud");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = seed_execution(&mut store, &task, "E-1");

    assert!(store
        .get_workspace(&WorkspaceId::new("ws-1").unwrap())
        .unwrap()
        .is_none());
    assert!(store
        .get_workspace_for_execution(execution.id())
        .unwrap()
        .is_none());

    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();

    let loaded = store
        .get_workspace(&WorkspaceId::new("ws-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(loaded, lease("ws-1", "E-1"));
    assert_eq!(loaded.lifecycle(), LeaseLifecycle::Allocated);
    assert_eq!(loaded.base_revision().as_str(), &"a".repeat(40));
    assert_eq!(loaded.branch().as_str(), "refs/heads/agent/E-1");
    assert_eq!(loaded.worktree_ref(), "/tmp/runlith-ws-test/ws-1");

    let by_owner = store
        .get_workspace_for_execution(execution.id())
        .unwrap()
        .unwrap();
    assert_eq!(by_owner.id().as_str(), "ws-1");
}

#[test]
fn insert_guards_reject_duplicates_and_missing_owners() {
    let db = TestDb::new("insert-guards");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    seed_execution(&mut store, &task, "E-1");
    seed_execution(&mut store, &task, "E-2");

    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();

    // Duplicate lease id.
    let err = store.insert_allocated_lease(&lease("ws-1", "E-2"));
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    // Second lease for the same execution: one Execution owns exactly one lease.
    let err = store.insert_allocated_lease(&lease("ws-2", "E-1"));
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    // Owning execution absent.
    let err = store.insert_allocated_lease(&lease("ws-9", "E-9"));
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    // Only Allocated intents are inserted.
    let mut ready = lease("ws-3", "E-2");
    ready.mark_ready().unwrap();
    let err = store.insert_allocated_lease(&ready);
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    // Exactly one lease survived all rejections.
    assert!(store
        .get_workspace(&WorkspaceId::new("ws-2").unwrap())
        .unwrap()
        .is_none());
}

// -- Ready transition and exactly-once --------------------------------------

#[test]
fn ready_transition_commits_state_and_created_event_atomically() {
    let db = TestDb::new("ready-atomic");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = seed_execution(&mut store, &task, "E-1");
    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();

    store
        .mark_lease_ready(
            &WorkspaceId::new("ws-1").unwrap(),
            &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-1"),
        )
        .unwrap();

    let loaded = store
        .get_workspace(&WorkspaceId::new("ws-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.lifecycle(), LeaseLifecycle::Ready);
    assert!(store
        .has_workspace_event(
            &WorkspaceId::new("ws-1").unwrap(),
            WorkspaceEventKind::Created
        )
        .unwrap());

    let history = store.list_events_ordered().unwrap();
    let created = history
        .iter()
        .find(|event| event.fact.event_type == EventType::WorkspaceCreated)
        .unwrap();
    assert_eq!(created.event_version, EVENT_SCHEMA_VERSION);
    assert_eq!(created.fact.task_id.as_ref().unwrap().as_str(), "T-1");
    assert_eq!(created.fact.project_id.as_str(), "P-1");
    assert_eq!(created.fact.execution_id.as_ref().unwrap().as_str(), "E-1");
    assert_eq!(created.fact.workspace_id.as_ref().unwrap().as_str(), "ws-1");
    assert!(created.fact.attempt_id.is_none());
    assert!(created.fact.attempt_outcome.is_none());
    assert!(created.fact.from_state.is_none());
    assert!(created.fact.to_state.is_none());
    assert_eq!(
        created.fact.from_workspace_state,
        Some(LeaseLifecycle::Allocated)
    );
    assert_eq!(created.fact.to_workspace_state, Some(LeaseLifecycle::Ready));
    // Persisted columns carry the lease vocabulary, not Task states.
    let raw_from: String = db
        .raw()
        .query_row(
            "SELECT from_state FROM event_journal WHERE event_type = 'workspace.created'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let raw_to: String = db
        .raw()
        .query_row(
            "SELECT to_state FROM event_journal WHERE event_type = 'workspace.created'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((raw_from.as_str(), raw_to.as_str()), ("ALLOCATED", "READY"));
}

#[test]
fn ready_transition_is_guarded_and_never_duplicates_created() {
    let db = TestDb::new("ready-guarded");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = seed_execution(&mut store, &task, "E-1");
    let id = WorkspaceId::new("ws-1").unwrap();
    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();

    // Missing lease: no state, no event.
    let err = store.mark_lease_ready(
        &WorkspaceId::new("ws-missing").unwrap(),
        &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-missing"),
    );
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    store
        .mark_lease_ready(
            &id,
            &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-1"),
        )
        .unwrap();
    assert_eq!(created_events(&store, "ws-1"), 1);

    // Second attempt: guarded failure, still exactly one created fact.
    let err = store.mark_lease_ready(
        &id,
        &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-1"),
    );
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );
    assert_eq!(created_events(&store, "ws-1"), 1);

    // Wrong-kind evidence never drives the transition.
    let err = store.mark_lease_ready(
        &id,
        &evidence(WorkspaceEventKind::Removed, &task, &execution, "ws-1"),
    );
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    // Evidence for a different lease never drives this transition.
    let err = store.mark_lease_ready(
        &id,
        &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-other"),
    );
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );
}

// -- CleanupPending / Removed (persistence level; no service path) -----------

#[test]
fn cleanup_pending_is_guarded_and_emits_no_event() {
    let db = TestDb::new("pending");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = seed_execution(&mut store, &task, "E-1");
    let id = WorkspaceId::new("ws-1").unwrap();
    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();

    // Pending requires Ready.
    let err = store.mark_lease_cleanup_pending(&id);
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );

    let before = store.list_events_ordered().unwrap().len();
    store
        .mark_lease_ready(
            &id,
            &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-1"),
        )
        .unwrap();
    store.mark_lease_cleanup_pending(&id).unwrap();
    let loaded = store.get_workspace(&id).unwrap().unwrap();
    assert_eq!(loaded.lifecycle(), LeaseLifecycle::CleanupPending);
    // No new taxonomy: exactly the created fact, nothing else.
    assert_eq!(store.list_events_ordered().unwrap().len(), before + 1);

    // Second request: guarded failure.
    let err = store.mark_lease_cleanup_pending(&id);
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );
}

#[test]
fn removed_codec_round_trips_with_exact_scope() {
    let db = TestDb::new("removed");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = seed_execution(&mut store, &task, "E-1");
    let id = WorkspaceId::new("ws-1").unwrap();
    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();
    store
        .mark_lease_ready(
            &id,
            &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-1"),
        )
        .unwrap();
    store.mark_lease_cleanup_pending(&id).unwrap();

    // Removed requires CleanupPending.
    let mut direct = db.open();
    let err = direct.mark_lease_removed(
        &id,
        &evidence(WorkspaceEventKind::Removed, &task, &execution, "ws-1"),
    );
    // Same connection already pending: this succeeds; guard is proven below
    // on a fresh Allocated lease instead.
    err.unwrap();

    let loaded = store.get_workspace(&id).unwrap().unwrap();
    assert_eq!(loaded.lifecycle(), LeaseLifecycle::Removed);
    assert!(loaded.lifecycle().is_terminal());

    let history = store.list_events_ordered().unwrap();
    let removed = history
        .iter()
        .find(|event| event.fact.event_type == EventType::WorkspaceRemoved)
        .unwrap();
    assert_eq!(removed.fact.task_id.as_ref().unwrap().as_str(), "T-1");
    assert_eq!(removed.fact.execution_id.as_ref().unwrap().as_str(), "E-1");
    assert_eq!(removed.fact.workspace_id.as_ref().unwrap().as_str(), "ws-1");
    assert!(removed.fact.attempt_id.is_none());
    assert!(removed.fact.attempt_outcome.is_none());
    assert_eq!(
        removed.fact.from_workspace_state,
        Some(LeaseLifecycle::CleanupPending)
    );
    assert_eq!(
        removed.fact.to_workspace_state,
        Some(LeaseLifecycle::Removed)
    );

    // Guard: removed from Allocated fails with no event.
    let task2 = seed_task(&mut store, "P-2", "T-2");
    let exec2 = seed_execution(&mut store, &task2, "E-2");
    store.insert_allocated_lease(&lease("ws-2", "E-2")).unwrap();
    let before = store.list_events_ordered().unwrap().len();
    let err = store.mark_lease_removed(
        &WorkspaceId::new("ws-2").unwrap(),
        &evidence(WorkspaceEventKind::Removed, &task2, &exec2, "ws-2"),
    );
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "got {err:?}"
    );
    assert_eq!(store.list_events_ordered().unwrap().len(), before);
}

// -- SQL CHECK backstop ------------------------------------------------------

#[test]
fn journal_check_rejects_mis_scoped_workspace_rows() {
    let db = TestDb::new("ws-check");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    seed_execution(&mut store, &task, "E-1");
    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();
    // The execution seed above journaled one execution.created fact; scope
    // rejections must leak nothing beyond it.
    let before = store.list_events_ordered().unwrap().len();
    let raw = db.raw();

    // Missing workspace_id.
    let err = raw.execute(
        "INSERT INTO event_journal
         (event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id)
         VALUES ('workspace.created', 1, 'T-1', 'P-1', 'ALLOCATED', 'READY',
                 NULL, 'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                 'E-1', NULL, NULL, NULL)",
        [],
    );
    assert!(
        err.is_err(),
        "workspace.created without workspace_id must fail"
    );

    // Attempt scope on a workspace fact.
    let err = raw.execute(
        "INSERT INTO event_journal
         (event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id)
         VALUES ('workspace.created', 1, 'T-1', 'P-1', 'ALLOCATED', 'READY',
                 NULL, 'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                 'E-1', 'A-1', NULL, 'ws-1')",
        [],
    );
    assert!(err.is_err(), "workspace.created with attempt_id must fail");

    // Task lifecycle vocabulary in the shared state columns.
    let err = raw.execute(
        "INSERT INTO event_journal
         (event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id)
         VALUES ('workspace.created', 1, 'T-1', 'P-1', 'DRAFT', 'READY',
                 NULL, 'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                 'E-1', NULL, NULL, 'ws-1')",
        [],
    );
    assert!(err.is_err(), "workspace.created with Task states must fail");

    // Workspace scope on a G2 fact.
    let err = raw.execute(
        "INSERT INTO event_journal
         (event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id)
         VALUES ('task.started', 1, 'T-1', 'P-1', 'CLAIMED', 'RUNNING',
                 NULL, 'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                 NULL, NULL, NULL, 'ws-1')",
        [],
    );
    assert!(err.is_err(), "G2 fact with workspace_id must fail");

    // Workspace scope on a G3 fact.
    let err = raw.execute(
        "INSERT INTO event_journal
         (event_type, event_version, task_id, project_id, from_state, to_state,
          reason, correlation_id, causation_id, occurred_at, provenance,
          execution_id, attempt_id, attempt_outcome, workspace_id)
         VALUES ('execution.created', 1, 'T-1', 'P-1', NULL, NULL,
                 NULL, 'op-1', 'req-1', '2024-01-01T00:00:00Z', 'test',
                 'E-1', NULL, NULL, 'ws-1')",
        [],
    );
    assert!(err.is_err(), "G3 fact with workspace_id must fail");

    // No partial rows leaked through any rejection.
    assert_eq!(store.list_events_ordered().unwrap().len(), before);
}

// -- read-path corruption ----------------------------------------------------

#[test]
fn corrupt_lease_rows_fail_explicitly() {
    let db = TestDb::new("lease-corrupt");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    seed_execution(&mut store, &task, "E-1");
    seed_execution(&mut store, &task, "E-2");
    seed_execution(&mut store, &task, "E-3");
    seed_execution(&mut store, &task, "E-4");
    seed_execution(&mut store, &task, "E-5");
    let raw = db.raw();

    // Unknown lifecycle marker (CHECK bypassed to plant it).
    raw.execute_batch(
        "PRAGMA ignore_check_constraints = ON;
         INSERT INTO workspace_leases
            (workspace_id, execution_id, repository_ref, base_revision,
             branch_ref, worktree_ref, lifecycle)
         VALUES ('ws-bad-life', 'E-2', 'repo-local:x', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                 'refs/heads/agent/e-2', '/tmp/ws-bad-life', 'RUNNING');
         PRAGMA ignore_check_constraints = OFF;",
    )
    .unwrap();
    drop(store);
    let store = db.open();
    let err = store.get_workspace(&WorkspaceId::new("ws-bad-life").unwrap());
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "unknown lifecycle must fail, got {err:?}"
    );

    // Invalid base revision.
    db.raw()
        .execute(
            "INSERT INTO workspace_leases
                (workspace_id, execution_id, repository_ref, base_revision,
                 branch_ref, worktree_ref, lifecycle)
             VALUES ('ws-bad-rev', 'E-3', 'repo-local:x', 'main',
                     'refs/heads/agent/e-3', '/tmp/ws-bad-rev', 'ALLOCATED')",
            [],
        )
        .unwrap();
    let err = store.get_workspace(&WorkspaceId::new("ws-bad-rev").unwrap());
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "non-OID revision must fail, got {err:?}"
    );

    // Invalid branch ref.
    db.raw()
        .execute(
            "INSERT INTO workspace_leases
                (workspace_id, execution_id, repository_ref, base_revision,
                 branch_ref, worktree_ref, lifecycle)
             VALUES ('ws-bad-branch', 'E-4', 'repo-local:x',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     'main', '/tmp/ws-bad-branch', 'ALLOCATED')",
            [],
        )
        .unwrap();
    let err = store.get_workspace(&WorkspaceId::new("ws-bad-branch").unwrap());
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "non-ref branch must fail, got {err:?}"
    );

    // Blank worktree ref.
    db.raw()
        .execute(
            "INSERT INTO workspace_leases
                (workspace_id, execution_id, repository_ref, base_revision,
                 branch_ref, worktree_ref, lifecycle)
             VALUES ('ws-bad-ref', 'E-5', 'repo-local:x',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     'refs/heads/agent/e-5', '   ', 'ALLOCATED')",
            [],
        )
        .unwrap();
    let err = store.get_workspace(&WorkspaceId::new("ws-bad-ref").unwrap());
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "blank worktree ref must fail, got {err:?}"
    );

    // Owning execution missing (planted on an FK-relaxed connection;
    // reads through the store still enforce FK and fail explicitly).
    let planted = db.raw();
    planted.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
    planted
        .execute(
            "INSERT INTO workspace_leases
                (workspace_id, execution_id, repository_ref, base_revision,
                 branch_ref, worktree_ref, lifecycle)
             VALUES ('ws-no-owner', 'E-ghost', 'repo-local:x',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     'refs/heads/agent/e-ghost', '/tmp/ws-no-owner', 'ALLOCATED')",
            [],
        )
        .unwrap();
    let err = store.get_workspace(&WorkspaceId::new("ws-no-owner").unwrap());
    assert!(
        matches!(err, Err(WorkspaceFailure::StorageFailure { .. })),
        "dangling owner must fail, got {err:?}"
    );
}

#[test]
fn corrupt_workspace_journal_rows_fail_on_read() {
    let db = TestDb::new("journal-corrupt");
    let mut store = db.open();
    let task = seed_task(&mut store, "P-1", "T-1");
    let execution = seed_execution(&mut store, &task, "E-1");
    store.insert_allocated_lease(&lease("ws-1", "E-1")).unwrap();
    store
        .mark_lease_ready(
            &WorkspaceId::new("ws-1").unwrap(),
            &evidence(WorkspaceEventKind::Created, &task, &execution, "ws-1"),
        )
        .unwrap();

    // Unsupported event version with otherwise valid scope (CHECK
    // bypassed to plant it; the SQL CHECK would stop this on write).
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE event_journal SET event_version = 2
             WHERE event_type = 'workspace.created';
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let err = store.list_events_ordered();
    assert!(
        matches!(err, Err(StorageError::Corrupt(_))),
        "version 2 workspace fact must fail, got {err:?}"
    );
    db.raw()
        .execute(
            "UPDATE event_journal SET event_version = 1
             WHERE event_type = 'workspace.created'",
            [],
        )
        .unwrap();

    // Task-state vocabulary smuggled into a workspace row (CHECK bypassed).
    db.raw()
        .execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             UPDATE event_journal SET from_state = 'DRAFT'
             WHERE event_type = 'workspace.created';
             PRAGMA ignore_check_constraints = OFF;",
        )
        .unwrap();
    let err = store.list_events_ordered();
    assert!(
        matches!(err, Err(StorageError::Corrupt(_))),
        "Task vocabulary on workspace row must fail, got {err:?}"
    );
}

// -- migration preservation --------------------------------------------------

#[test]
fn v4_database_with_g2_g3_data_migrates_preserving_everything() {
    const M0001: &str = include_str!("../migrations/0001_task_core.sql");
    const M0002: &str = include_str!("../migrations/0002_event_journal.sql");
    const M0003: &str = include_str!("../migrations/0003_event_scope.sql");
    const M0004: &str = include_str!("../migrations/0004_runtime_core.sql");
    let db = TestDb::new("v4-migrate");
    {
        let connection = db.raw();
        connection.execute_batch(M0001).unwrap();
        connection.execute_batch(M0002).unwrap();
        connection.execute_batch(M0003).unwrap();
        connection.execute_batch(M0004).unwrap();
        connection
            .execute_batch(
                "INSERT INTO projects (id, name) VALUES ('P-1', 'project P-1');
                 INSERT INTO tasks (id, project_id, objective, state)
                    VALUES ('T-1', 'P-1', 'objective T-1', 'RUNNING');
                 INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
                 INSERT INTO execution_attempts
                    (attempt_id, execution_id, position, provider_ref, outcome, detail)
                 VALUES ('A-1', 'E-1', 0, 'fake:scenario', 'succeeded', 'done');
                 INSERT INTO event_journal
                    (event_type, event_version, task_id, project_id, from_state,
                     to_state, reason, correlation_id, causation_id, occurred_at,
                     provenance, execution_id, attempt_id, attempt_outcome)
                 VALUES
                    ('task.created', 1, 'T-1', 'P-1', 'DRAFT', 'DRAFT', NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'task-application-service/create_task',
                     NULL, NULL, NULL),
                    ('task.started', 1, 'T-1', 'P-1', 'CLAIMED', 'RUNNING', NULL,
                     'op-2', 'req-2', '2024-01-01T00:00:01Z', 'task-application-service/start',
                     NULL, NULL, NULL),
                    ('execution.created', 1, 'T-1', 'P-1', NULL, NULL, NULL,
                     'op-3', 'req-3', '2024-01-01T00:00:02Z', 'runtime-application-service/create_execution',
                     'E-1', NULL, NULL),
                    ('attempt.created', 1, 'T-1', 'P-1', NULL, NULL, NULL,
                     'op-4', 'req-4', '2024-01-01T00:00:03Z', 'runtime-application-service/create_attempt',
                     'E-1', 'A-1', NULL),
                    ('attempt.concluded', 1, 'T-1', 'P-1', NULL, NULL, NULL,
                     'op-5', 'req-5', '2024-01-01T00:00:04Z', 'runtime-application-service/conclude_attempt',
                     'E-1', 'A-1', 'succeeded');
                 PRAGMA user_version = 4;",
            )
            .unwrap();
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 6);

    // Every historical fact survives verbatim: seq, ids, types, versions,
    // scopes, outcomes, states, correlation/causation, timestamps,
    // provenance — plus workspace_id NULL throughout.
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 5);
    let kinds: Vec<&str> = history
        .iter()
        .map(|event| event.fact.event_type.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "task.created",
            "task.started",
            "execution.created",
            "attempt.created",
            "attempt.concluded"
        ]
    );
    for (index, event) in history.iter().enumerate() {
        let seq = (index + 1) as i64;
        assert_eq!(event.seq, seq);
        assert_eq!(
            event.id,
            storage_sqlite::EventId::parse(&format!("evt-{seq}")).unwrap()
        );
        assert_eq!(event.event_version, 1);
        assert!(event.fact.workspace_id.is_none());
        assert!(event.fact.from_workspace_state.is_none());
        assert!(event.fact.to_workspace_state.is_none());
    }
    assert_eq!(
        history[1].fact.from_state,
        Some(task_domain::TaskState::Claimed)
    );
    assert_eq!(
        history[1].fact.to_state,
        Some(task_domain::TaskState::Running)
    );
    assert_eq!(history[1].fact.correlation_id, "op-2");
    assert_eq!(history[1].fact.causation_id, "req-2");
    assert_eq!(history[1].fact.occurred_at, "2024-01-01T00:00:01Z");
    assert_eq!(
        history[4].fact.attempt_outcome,
        Some(storage_sqlite::AttemptOutcome::Succeeded)
    );
    assert_eq!(
        history[4].fact.provenance,
        "runtime-application-service/conclude_attempt"
    );

    // Relational state intact; the new lease table starts empty.
    assert!(store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .is_some());
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_some());
    assert!(store
        .get_workspace(&WorkspaceId::new("ws-1").unwrap())
        .unwrap()
        .is_none());
}

/// A real v5 database (built from the canonical 0001..0005 files) with an
/// old-format managed branch, an open attempt, and an `Allocated` lease
/// migrates to v6 with every durable fact preserved bit-for-bit and an
/// empty reservation table (TASK-0051 §58 upgrade compat: persisted
/// branch/path facts are never recomputed, so the old `agent/E-1` branch
/// stays observable, managed-scoped, and reconcilable under the new
/// application version).
#[test]
fn v5_database_with_lease_migrates_to_v6_preserving_everything() {
    const M0001: &str = include_str!("../migrations/0001_task_core.sql");
    const M0002: &str = include_str!("../migrations/0002_event_journal.sql");
    const M0003: &str = include_str!("../migrations/0003_event_scope.sql");
    const M0004: &str = include_str!("../migrations/0004_runtime_core.sql");
    const M0005: &str = include_str!("../migrations/0005_workspace_leases.sql");
    let db = TestDb::new("v5-migrate");
    {
        let connection = db.raw();
        connection.execute_batch(M0001).unwrap();
        connection.execute_batch(M0002).unwrap();
        connection.execute_batch(M0003).unwrap();
        connection.execute_batch(M0004).unwrap();
        connection.execute_batch(M0005).unwrap();
        connection
            .execute_batch(
                "INSERT INTO projects (id, name) VALUES ('P-1', 'project P-1');
                 INSERT INTO tasks (id, project_id, objective, state)
                    VALUES ('T-1', 'P-1', 'objective T-1', 'RUNNING');
                 INSERT INTO executions (id, task_id) VALUES ('E-1', 'T-1');
                 INSERT INTO execution_attempts
                    (attempt_id, execution_id, position, provider_ref, outcome, detail)
                 VALUES ('A-1', 'E-1', 0, 'fake:scenario', NULL, NULL);
                 INSERT INTO workspace_leases
                    (workspace_id, execution_id, repository_ref, base_revision,
                     branch_ref, worktree_ref, lifecycle)
                 VALUES ('ws-1', 'E-1', 'repo-local:v5',
                         'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                         'refs/heads/agent/E-1', '/tmp/runlith-ws-v5/ws-1', 'ALLOCATED');
                 INSERT INTO event_journal
                    (event_type, event_version, task_id, project_id, from_state,
                     to_state, reason, correlation_id, causation_id, occurred_at,
                     provenance, execution_id, attempt_id, attempt_outcome, workspace_id)
                 VALUES
                    ('task.created', 1, 'T-1', 'P-1', 'DRAFT', 'DRAFT', NULL,
                     'op-1', 'req-1', '2024-01-01T00:00:00Z', 'task-application-service/create_task',
                     NULL, NULL, NULL, NULL),
                    ('task.claimed', 1, 'T-1', 'P-1', 'READY', 'CLAIMED', NULL,
                     'op-2', 'req-2', '2024-01-01T00:00:01Z', 'task-application-service/claim',
                     NULL, NULL, NULL, NULL),
                    ('task.started', 1, 'T-1', 'P-1', 'CLAIMED', 'RUNNING', NULL,
                     'op-3', 'req-3', '2024-01-01T00:00:02Z', 'task-application-service/start',
                     NULL, NULL, NULL, NULL),
                    ('execution.created', 1, 'T-1', 'P-1', NULL, NULL, NULL,
                     'op-4', 'req-4', '2024-01-01T00:00:03Z', 'runtime-application-service/create_execution',
                     'E-1', NULL, NULL, NULL),
                    ('attempt.created', 1, 'T-1', 'P-1', NULL, NULL, NULL,
                     'op-5', 'req-5', '2024-01-01T00:00:04Z', 'runtime-application-service/create_attempt',
                     'E-1', 'A-1', NULL, NULL);
                 PRAGMA user_version = 5;",
            )
            .unwrap();
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 6);

    // Execution with its still-open attempt survived.
    let execution = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .expect("E-1 survives");
    assert_eq!(execution.attempts().len(), 1);
    assert!(execution.attempts()[0].is_open());

    // The old-format lease survived verbatim and stays managed-scoped:
    // old branches are observed/cleaned through their persisted facts,
    // never re-derived.
    let lease = store
        .get_workspace(&WorkspaceId::new("ws-1").unwrap())
        .unwrap()
        .expect("ws-1 survives");
    assert_eq!(lease.branch().as_str(), "refs/heads/agent/E-1");
    assert!(lease.branch().is_managed());
    assert_eq!(lease.worktree_ref(), "/tmp/runlith-ws-v5/ws-1");
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);

    // Journal history intact (5 facts, no workspace scope mixed in).
    let history = store.list_events_ordered().unwrap();
    assert_eq!(history.len(), 5);
    for event in &history {
        assert!(event.fact.workspace_id.is_none());
    }

    // The new reservation table exists and starts empty — migration
    // invents no reservations for historical attempts.
    let raw = db.raw();
    let reservations: i64 = raw
        .query_row("SELECT COUNT(*) FROM attempt_reservations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(reservations, 0);
}

// -- structural invariants ---------------------------------------------------

#[test]
fn lease_table_has_no_dirtiness_columns() {
    let db = TestDb::new("no-dirt");
    let _store = db.open();
    let raw = db.raw();
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('workspace_leases') ORDER BY cid")
        .unwrap();
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "workspace_id",
            "execution_id",
            "repository_ref",
            "base_revision",
            "branch_ref",
            "worktree_ref",
            "lifecycle",
        ]
    );
    for column in &columns {
        for token in ["dirty", "status", "count", "modified", "untracked"] {
            assert!(
                !column.contains(token),
                "lease column {column:?} must not persist dirtiness"
            );
        }
    }
}

#[test]
fn persisted_workspace_envelope_matches_event_schema() {
    // Same compile-time fixture style as the G2/G3 schema tests: the schema
    // file is authority for field names; production code never parses it.
    const SCHEMA: &str = include_str!("../../../contracts/json-schema/event.schema.json");
    for required in [
        "\"workspace_id\"",
        "\"workspace.created\"",
        "\"workspace.removed\"",
        "\"ALLOCATED\"",
        "\"READY\"",
        "\"CLEANUP_PENDING\"",
        "\"REMOVED\"",
    ] {
        assert!(SCHEMA.contains(required), "schema must define {required}");
    }
}

#[test]
fn workspace_event_taxonomy_stays_closed() {
    assert_eq!(EventType::WorkspaceCreated.as_str(), "workspace.created");
    assert_eq!(EventType::WorkspaceRemoved.as_str(), "workspace.removed");
    assert!(EventType::WorkspaceCreated.is_workspace_scoped());
    assert!(EventType::WorkspaceRemoved.is_workspace_scoped());
    assert!(!EventType::ExecutionCreated.is_workspace_scoped());
    assert!(!EventType::TaskCreated.is_workspace_scoped());
    assert_eq!(
        EventType::parse("workspace.created").unwrap(),
        EventType::WorkspaceCreated
    );
    assert_eq!(
        EventType::parse("workspace.removed").unwrap(),
        EventType::WorkspaceRemoved
    );
    // Invented workspace vocabulary fails explicitly — no generic bucket.
    for bogus in [
        "workspace.allocated",
        "workspace.ready",
        "workspace.cleanup_pending",
        "workspace.reconciled",
        "workspace.failed",
        "workspace.*",
        "Workspace.Created",
    ] {
        assert!(
            EventType::parse(bogus).is_err(),
            "{bogus:?} must not decode as an event type"
        );
    }
}
