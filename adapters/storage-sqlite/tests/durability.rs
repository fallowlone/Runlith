//! G2 durability and contract tests for the SQLite adapter.
//!
//! Real files only (no `:memory:` reliance): every test owns a temporary
//! database file, and the core proof drops the store and reopens the same
//! file. Raw SQL appears solely in corruption fixtures; production paths
//! stay typed.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use plan_domain::{
    AffectedParentCriterion, CriterionId, PlanningInputSnapshotId, RiskClass, ScopeBound,
    VerificationMethod,
};
use storage_sqlite::{SqliteStore, StorageError, SUPPORTED_SCHEMA_VERSION};
use task_domain::{
    EvidenceRequirement, Project, ProjectId, RequirementKind, Task, TaskId, TaskState,
};

/// Previous-schema migration fixtures replay the checked-in migration files
/// exactly as the adapter did at that version (the `security_audit.rs`
/// `create_schema_v15` precedent), so a legacy database is real DDL rather
/// than a hand-written approximation.
const MIGRATIONS_THROUGH_16: [&str; 16] = [
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
];
const MIGRATION_0017: &str = include_str!("../migrations/0017_evidence_requirements.sql");

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

#[cfg(unix)]
fn secure_raw_fixture(path: &std::path::Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .expect("secure raw sqlite fixture");
}

#[cfg(not(unix))]
fn secure_raw_fixture(_path: &std::path::Path) {}

// -- evidence requirements (TASK-0123 / ADR-0030) ------------------------

/// Migration 0017 refuses ambiguous duplicate legacy evidence, rolls back to
/// v16 without deleting rows, and opens after explicit remediation.
#[test]
fn schema_16_duplicate_acceptance_rows_fail_closed_then_remediate() {
    let db = TestDb::new("requirement-legacy-backfill");
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        for migration in MIGRATIONS_THROUGH_16 {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO projects (id, name) VALUES ('P-legacy', 'legacy project');
                 INSERT INTO tasks (id, project_id, objective, state)
                     VALUES ('T-legacy', 'P-legacy', 'legacy objective', 'DRAFT');
                 INSERT INTO tasks (id, project_id, objective, state)
                     VALUES ('T-empty', 'P-legacy', 'no acceptance', 'DRAFT');
                 INSERT INTO task_strings (task_id, slot, position, value) VALUES
                     ('T-legacy', 'acceptance', 0, 'criterion-a'),
                     ('T-legacy', 'acceptance', 1, 'criterion-b'),
                     ('T-legacy', 'acceptance', 2, 'criterion-b'),
                     ('T-legacy', 'allowed_path', 0, 'repo:src');
                 PRAGMA user_version = 16;",
            )
            .unwrap();
    }
    secure_raw_fixture(&db.path);
    let error = match SqliteStore::open(&db.path) {
        Ok(_) => panic!("ambiguous evidence must refuse migration"),
        Err(error) => error,
    };
    assert!(matches!(error, StorageError::Migration(_)));
    let connection = rusqlite::Connection::open(&db.path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        16
    );
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM task_strings WHERE task_id = 'T-legacy' AND slot = 'acceptance'", [], |row| row.get::<_, u32>(0)).unwrap(), 3);
    drop(connection);
    let restart_error = match SqliteStore::open(&db.path) {
        Ok(_) => panic!("unremediated restart must refuse"),
        Err(error) => error,
    };
    assert!(matches!(restart_error, StorageError::Migration(_)));
    let connection = rusqlite::Connection::open(&db.path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        16
    );
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM task_strings WHERE task_id = 'T-legacy' AND slot = 'acceptance'", [], |row| row.get::<_, u32>(0)).unwrap(), 3);
    connection.execute("DELETE FROM task_strings WHERE task_id = 'T-legacy' AND slot = 'acceptance' AND position = 2", []).unwrap();
    drop(connection);
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);

    let legacy = store
        .get_task(&TaskId::new("T-legacy").unwrap())
        .unwrap()
        .expect("legacy task survives the backfill");
    assert_eq!(legacy.acceptance(), ["criterion-a", "criterion-b"]);
    let requirements = legacy.evidence_requirements();
    assert_eq!(requirements.len(), 2);
    assert_eq!(requirements[0].criterion_id, "criterion-a");
    assert_eq!(requirements[1].criterion_id, "criterion-b");
    for requirement in requirements {
        assert_eq!(
            requirement.requirement_kind,
            task_domain::RequirementKind::TestSuite
        );
        // Pins the SQL literal against the Rust-owned default: drift between
        // migration 0017 and `default_subject_binding` fails here.
        assert_eq!(
            requirement.subject_binding,
            task_domain::RequirementKind::TestSuite.default_subject_binding()
        );
        assert!(requirement.required);
        assert!(requirement.policy_fingerprint.is_none());
    }

    let empty = store
        .get_task(&TaskId::new("T-empty").unwrap())
        .unwrap()
        .expect("task without acceptance");
    assert!(empty.acceptance().is_empty());
    assert!(empty.evidence_requirements().is_empty());
}

#[test]
fn schema_17_planning_criteria_backfill_all_requirement_kinds_and_restart() {
    let db = TestDb::new("planning-requirement-backfill");
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        for migration in MIGRATIONS_THROUGH_16 {
            connection.execute_batch(migration).unwrap();
        }
        connection.execute_batch(MIGRATION_0017).unwrap();
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        connection
            .execute(
                "INSERT INTO planning_input_snapshots
                 (snapshot_id, plan_branch_id, task_ref, expected_plan_head, baseline_risk,
                  policy_fingerprint, input_fingerprint, created_at_ms)
                 VALUES ('snapshot-legacy', 'branch-legacy', 'task-legacy', 0, 'low',
                         'policy:legacy', ?1, 1)",
                rusqlite::params![fingerprint],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_input_scope_roots (snapshot_id, position, scope_root)
                 VALUES ('snapshot-legacy', 0, 'repo:src')",
                [],
            )
            .unwrap();
        for (position, (criterion, method)) in [
            ("criterion-a", "automated-test"),
            ("criterion-b", "independent-review"),
            ("criterion-c", "security-check"),
            ("criterion-d", "human-approval"),
        ]
        .into_iter()
        .enumerate()
        {
            connection
                .execute(
                    "INSERT INTO planning_input_criteria
                     (snapshot_id, position, criterion_id, verification_method)
                     VALUES ('snapshot-legacy', ?1, ?2, ?3)",
                    rusqlite::params![position as i64, criterion, method],
                )
                .unwrap();
        }
        connection.pragma_update(None, "user_version", 17).unwrap();
    }
    secure_raw_fixture(&db.path);

    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let expected = [
        (
            "criterion-a",
            VerificationMethod::AutomatedTest,
            RequirementKind::TestSuite,
        ),
        (
            "criterion-b",
            VerificationMethod::IndependentReview,
            RequirementKind::IndependentReview,
        ),
        (
            "criterion-c",
            VerificationMethod::SecurityCheck,
            RequirementKind::SecurityCheck,
        ),
        (
            "criterion-d",
            VerificationMethod::HumanApproval,
            RequirementKind::HumanApproval,
        ),
    ]
    .into_iter()
    .map(|(criterion, method, kind)| AffectedParentCriterion {
        id: CriterionId::new(criterion).unwrap(),
        method,
        requirement: Some(
            EvidenceRequirement::new(
                criterion,
                kind,
                kind.default_subject_binding(),
                Vec::new(),
                None,
                true,
            )
            .unwrap(),
        ),
    })
    .collect::<Vec<_>>();
    assert!(store
        .planning_input_policy_matches(
            &PlanningInputSnapshotId::new("snapshot-legacy").unwrap(),
            "task-legacy",
            &ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
            RiskClass::Low,
            &expected,
            &[],
            None,
            "policy:legacy",
        )
        .unwrap());
    drop(store);

    let connection = rusqlite::Connection::open(&db.path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT verification_method, requirement_kind, subject_binding,
                    requirement_required, requirement_policy_fingerprint
             FROM planning_input_criteria
             WHERE snapshot_id = 'snapshot-legacy'
             ORDER BY position",
        )
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "automated-test".to_owned(),
                "test-suite".to_owned(),
                "content+toolchain+config".to_owned(),
                1,
                None,
            ),
            (
                "independent-review".to_owned(),
                "independent-review".to_owned(),
                "content+toolchain+config".to_owned(),
                1,
                None,
            ),
            (
                "security-check".to_owned(),
                "security-check".to_owned(),
                "content+toolchain+config".to_owned(),
                1,
                None,
            ),
            (
                "human-approval".to_owned(),
                "human-approval".to_owned(),
                "content+toolchain+config".to_owned(),
                1,
                None,
            ),
        ]
    );
    drop(statement);
    drop(connection);

    let reopened = db.open();
    assert_eq!(reopened.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
}

#[test]
fn schema_17_unknown_planning_method_fails_migration_closed() {
    let db = TestDb::new("planning-requirement-unknown");
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        for migration in MIGRATIONS_THROUGH_16 {
            connection.execute_batch(migration).unwrap();
        }
        connection.execute_batch(MIGRATION_0017).unwrap();
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        connection
            .execute(
                "INSERT INTO planning_input_snapshots
                 (snapshot_id, plan_branch_id, task_ref, expected_plan_head, baseline_risk,
                  policy_fingerprint, input_fingerprint, created_at_ms)
                 VALUES ('snapshot-corrupt', 'branch-corrupt', 'task-corrupt', 0, 'low',
                         'policy:legacy', ?1, 1)",
                rusqlite::params![fingerprint],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        connection
            .execute(
                "INSERT INTO planning_input_criteria
                 (snapshot_id, position, criterion_id, verification_method)
                 VALUES ('snapshot-corrupt', 0, 'criterion-corrupt', 'unknown-method')",
                [],
            )
            .unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints = OFF;")
            .unwrap();
        connection.pragma_update(None, "user_version", 17).unwrap();
    }
    secure_raw_fixture(&db.path);

    assert!(matches!(
        SqliteStore::open(&db.path),
        Err(StorageError::Migration(_))
    ));
    let connection = rusqlite::Connection::open(&db.path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        17
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT verification_method FROM planning_input_criteria
                 WHERE snapshot_id = 'snapshot-corrupt'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "unknown-method"
    );
}

#[test]
fn migration_backfills_explicit_default_requirements_for_legacy_criteria() {
    let db = TestDb::new("requirement-backfill");
    {
        let mut store = db.open();
        let project = stored_project(&mut store, "P-1");
        let mut task = draft_task("P-1", "T-legacy");
        task.define_planning_metadata(
            vec!["repo:src".to_owned()],
            vec!["criterion-a".to_owned(), "criterion-b".to_owned()],
            "low",
        )
        .unwrap();
        store.create_task(&task).unwrap();
        assert_eq!(project.id().as_str(), "P-1");
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    let task = store
        .get_task(&TaskId::new("T-legacy").unwrap())
        .unwrap()
        .expect("legacy task");
    let requirements = task.evidence_requirements();
    assert_eq!(requirements.len(), 2);
    for requirement in requirements {
        assert_eq!(
            requirement.requirement_kind,
            task_domain::RequirementKind::TestSuite,
            "legacy criteria backfill to the explicit TEST_SUITE default"
        );
        assert_eq!(
            requirement.subject_binding,
            task_domain::RequirementKind::TestSuite.default_subject_binding()
        );
        assert!(requirement.required);
        assert!(
            requirement.policy_fingerprint.is_none(),
            "legacy rows are not policy-derived (CV-7 stale input)"
        );
    }
}

#[test]
fn evidence_requirements_round_trip_with_params_and_restart() {
    let db = TestDb::new("requirement-round-trip");
    {
        let mut store = db.open();
        stored_project(&mut store, "P-1");
        let mut task = draft_task("P-1", "T-1");
        task.define_planning_metadata(
            vec!["repo:src".to_owned()],
            vec!["reviewed".to_owned(), "tested".to_owned()],
            "medium",
        )
        .unwrap();
        task.define_evidence_requirements(vec![
            task_domain::EvidenceRequirement::new(
                "reviewed",
                task_domain::RequirementKind::IndependentReview,
                task_domain::RequirementKind::IndependentReview.default_subject_binding(),
                Vec::new(),
                Some("policy:v1".to_owned()),
                true,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "tested",
                task_domain::RequirementKind::TestSuite,
                task_domain::RequirementKind::TestSuite.default_subject_binding(),
                vec![
                    task_domain::RequirementParam::new("selector", "cargo test -p task-domain")
                        .unwrap(),
                ],
                Some("policy:v1".to_owned()),
                true,
            )
            .unwrap(),
        ])
        .unwrap();
        store.create_task(&task).unwrap();
    }
    // Reopen: the store, not the test, is the authority on what persisted.
    let store = db.open();
    let task = store
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .expect("task survives restart");
    let requirements = task.evidence_requirements();
    let reviewed = requirements
        .iter()
        .find(|requirement| requirement.criterion_id == "reviewed")
        .expect("reviewed criterion requirement");
    assert_eq!(
        reviewed.requirement_kind,
        task_domain::RequirementKind::IndependentReview
    );
    let tested = requirements
        .iter()
        .find(|requirement| requirement.criterion_id == "tested")
        .expect("tested criterion requirement");
    assert_eq!(tested.params.len(), 1);
    assert_eq!(tested.params[0].key(), "selector");
    assert_eq!(tested.params[0].value(), "cargo test -p task-domain");
    assert_eq!(tested.policy_fingerprint.as_deref(), Some("policy:v1"));
}

#[test]
fn corrupt_requirement_rows_fail_closed_on_read() {
    let db = TestDb::new("requirement-corrupt");
    {
        let mut store = db.open();
        stored_project(&mut store, "P-1");
        let mut task = draft_task("P-1", "T-1");
        task.define_planning_metadata(
            vec!["repo:src".to_owned()],
            vec!["criterion".to_owned()],
            "low",
        )
        .unwrap();
        store.create_task(&task).unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        // The closed CHECK vocabulary is the write-side fail-closed guard and
        // refuses this UPDATE as-is. Corruption fixtures bypass it explicitly
        // (the documented 0001 mechanism) to prove the read side fails closed
        // too: stored text outside the registry cannot reconstruct.
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints = ON;
                 UPDATE task_evidence_requirements SET requirement_kind = 'made-up-kind'
                 WHERE task_id = 'T-1';
                 PRAGMA ignore_check_constraints = OFF;",
            )
            .unwrap();
    }
    secure_raw_fixture(&db.path);
    let store = db.open();
    // An unknown stored kind is corruption, never silently repaired.
    assert!(store.get_task(&TaskId::new("T-1").unwrap()).is_err());
}

#[test]
fn unmapped_criterion_at_read_fails_closed() {
    let db = TestDb::new("requirement-orphan");
    {
        let mut store = db.open();
        stored_project(&mut store, "P-1");
        let mut task = draft_task("P-1", "T-1");
        task.define_planning_metadata(
            vec!["repo:src".to_owned()],
            vec!["criterion".to_owned()],
            "low",
        )
        .unwrap();
        store.create_task(&task).unwrap();
    }
    {
        let connection = rusqlite::Connection::open(&db.path).unwrap();
        connection
            .execute(
                "INSERT INTO task_evidence_requirements
                 (task_id, position, criterion_id, requirement_kind, subject_binding,
                  required, policy_fingerprint)
                 VALUES ('T-1', 1, 'phantom', 'test-suite', 'content', 1, NULL)",
                [],
            )
            .unwrap();
    }
    secure_raw_fixture(&db.path);
    let store = db.open();
    // A requirement for a criterion that is not an acceptance obligation
    // breaks the coverage invariant; restore refuses the Task.
    assert!(store.get_task(&TaskId::new("T-1").unwrap()).is_err());
}

// -- migration ----------------------------------------------------------

#[test]
fn fresh_database_migrates_to_current() {
    let db = TestDb::new("fresh");
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 19);
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
    secure_raw_fixture(&db.path);
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
             CREATE TABLE IF NOT EXISTS task_strings (
                 task_id TEXT NOT NULL REFERENCES tasks (id),
                 slot TEXT NOT NULL, position INTEGER NOT NULL, value TEXT NOT NULL,
                 PRIMARY KEY (task_id, slot, position));
             INSERT INTO projects (id, name) VALUES ('', 'blank id');
             INSERT INTO tasks (id, project_id, objective, state)
                 VALUES ('T-1', '', 'objective', 'DRAFT');",
            )
            .unwrap();
    }
    secure_raw_fixture(&db.path);
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
    secure_raw_fixture(&db.path);
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
