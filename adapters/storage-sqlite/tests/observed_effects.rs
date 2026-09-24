use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use evaluation::{
    subject_fingerprint, EffectId, EffectKind, EffectPayload, EffectProvenance, EffectSubjectRef,
    Sha256Digest,
};
use storage_sqlite::{
    EventType, NewJournalEvent, SqliteStore, StorageError, SUPPORTED_SCHEMA_VERSION,
};
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-observed-effect-{name}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            path: dir.join("db.sqlite"),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).unwrap()
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

fn event(
    kind: EventType,
    task: &Task,
    execution_id: Option<ExecutionId>,
    attempt_id: Option<AttemptId>,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: kind,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "observed-effect-test".to_owned(),
        causation_id: "observed-effect-test".to_owned(),
        occurred_at: "2026-09-23T12:00:00Z".to_owned(),
        provenance: "storage-sqlite/test".to_owned(),
        execution_id,
        attempt_id,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

fn seed_runtime(store: &mut SqliteStore) -> (ExecutionId, AttemptId) {
    let project = Project::new(ProjectId::new("P-EFFECT").unwrap(), "effects").unwrap();
    store.create_project(&project).unwrap();
    let task = Task::draft(
        TaskId::new("T-EFFECT").unwrap(),
        project.id().clone(),
        "capture observed effect",
    )
    .unwrap();
    store.create_task(&task).unwrap();
    let execution_id = ExecutionId::new("E-EFFECT").unwrap();
    let mut execution = Execution::begin(execution_id.clone(), task.id().clone());
    store
        .create_execution_with_event(
            &execution,
            &event(
                EventType::ExecutionCreated,
                &task,
                Some(execution_id.clone()),
                None,
            ),
        )
        .unwrap();
    let attempt_id = AttemptId::new("A-EFFECT").unwrap();
    execution
        .start_attempt(
            attempt_id.clone(),
            ProviderRef::new("test-provider").unwrap(),
        )
        .unwrap();
    let attempt = execution.get_attempt(&attempt_id).unwrap().clone();
    store
        .create_attempt_with_event(
            &attempt,
            &event(
                EventType::AttemptCreated,
                &task,
                Some(execution_id.clone()),
                Some(attempt_id.clone()),
            ),
        )
        .unwrap();
    (execution_id, attempt_id)
}

fn append_sample(
    store: &mut SqliteStore,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    subject: &str,
) -> evaluation::ObservedEffect {
    let subject_ref = EffectSubjectRef::new(subject).unwrap();
    let fingerprint = subject_fingerprint(
        EffectKind::ArtifactHash,
        subject_ref.as_str(),
        &[(
            "content",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )],
    )
    .unwrap();
    store
        .append_observed_effect(
            subject_ref,
            fingerprint,
            EffectPayload::ArtifactHash {
                bytes: 3,
                sha256: Sha256Digest::new(
                    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
                .unwrap(),
            },
            "2026-09-23T12:00:00Z",
            EffectProvenance::new(
                execution_id.clone(),
                attempt_id.clone(),
                "runlith.local-observer.v1",
            )
            .unwrap(),
        )
        .unwrap()
}

#[test]
fn observed_effect_survives_close_and_reopen() {
    let db = TestDb::new("restart");
    let effect = {
        let mut store = db.open();
        let (execution, attempt) = seed_runtime(&mut store);
        append_sample(&mut store, &execution, &attempt, "artifact:one")
    };
    let store = db.open();
    assert_eq!(
        store.get_observed_effect(effect.effect_id()).unwrap(),
        Some(effect)
    );
}

#[test]
fn concurrent_writers_allocate_distinct_effect_ids_atomically() {
    let db = TestDb::new("atomic-id");
    let (execution, attempt) = {
        let mut store = db.open();
        seed_runtime(&mut store)
    };
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for index in 0..2 {
        let path = db.path.clone();
        let execution = execution.clone();
        let attempt = attempt.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let mut store = SqliteStore::open(path).unwrap();
            barrier.wait();
            append_sample(
                &mut store,
                &execution,
                &attempt,
                &format!("artifact:{index}"),
            )
            .effect_id()
            .as_str()
            .to_owned()
        }));
    }
    barrier.wait();
    let mut ids = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, ["effect-1", "effect-2"]);
}

#[test]
fn corrupted_observed_effect_rows_fail_closed() {
    let corruptions = [
        ("kind", "effect_kind", "'UNKNOWN'"),
        ("authority", "captured_by", "'provider-authored'"),
        ("timestamp", "captured_at", "'2026-02-31T12:00:00Z'"),
        (
            "fingerprint",
            "subject_fingerprint",
            "'sha256:xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx'",
        ),
        ("payload", "payload_ref", "'exit=0'"),
        ("runtime", "runtime_ref", "''"),
        ("provenance", "execution_id", "'E-OTHER'"),
    ];
    for (name, column, value) in corruptions {
        let db = TestDb::new(name);
        let effect_id = {
            let mut store = db.open();
            let (execution, attempt) = seed_runtime(&mut store);
            append_sample(&mut store, &execution, &attempt, "artifact:corrupt")
                .effect_id()
                .clone()
        };
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = OFF; PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        connection
            .execute(
                &format!("UPDATE observed_effects SET {column} = {value} WHERE effect_id = ?1"),
                [effect_id.as_str()],
            )
            .unwrap();
        drop(connection);
        let store = db.open();
        assert!(
            matches!(
                store.get_observed_effect(&effect_id),
                Err(StorageError::Corrupt(_))
            ),
            "corruption case {name} was accepted"
        );
    }
}

#[test]
fn cv_2_schema_18_migrates_to_19_with_closed_capture_authority() {
    const THROUGH_18: [&str; 18] = [
        include_str!("../migrations/0001_task_core.sql"),
        include_str!("../migrations/0002_event_journal.sql"),
        include_str!("../migrations/0003_event_scope.sql"),
        include_str!("../migrations/0004_runtime_core.sql"),
        include_str!("../migrations/0005_workspace_leases.sql"),
        include_str!("../migrations/0006_attempt_reservations.sql"),
        include_str!("../migrations/0007_managed_identity_reservations.sql"),
        include_str!("../migrations/0008_approvals.sql"),
        include_str!("../migrations/0009_secret_refs.sql"),
        include_str!("../migrations/0010_security_audit.sql"),
        include_str!("../migrations/0011_workspace_trusted_anchor.sql"),
        include_str!("../migrations/0012_plan_persistence.sql"),
        include_str!("../migrations/0013_plan_cost_receipts.sql"),
        include_str!("../migrations/0014_plan_guardrails.sql"),
        include_str!("../migrations/0015_human_decisions.sql"),
        include_str!("../migrations/0016_g7_closure_remediation.sql"),
        include_str!("../migrations/0017_evidence_requirements.sql"),
        include_str!("../migrations/0018_planning_input_evidence_requirements.sql"),
    ];
    let db = TestDb::new("migration-18-19");
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&db.path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        connection
            .execute_batch("PRAGMA foreign_keys = ON")
            .unwrap();
        for migration in THROUGH_18 {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch("PRAGMA user_version = 18")
            .unwrap();
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 20);
    let connection = rusqlite::Connection::open(&db.path).unwrap();
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='observed_effects'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(sql.contains("captured_by = 'runlith.local-harness.v1'"));
}

#[test]
fn cv_2_arbitrary_effect_id_cannot_be_selected_by_caller() {
    let db = TestDb::new("id-owned-by-store");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    let effect = append_sample(&mut store, &execution, &attempt, "artifact:id");
    assert_eq!(effect.effect_id(), &EffectId::new("effect-1").unwrap());
}
