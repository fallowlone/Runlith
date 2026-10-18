use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use evaluation::{
    candidate_sha256, subject_fingerprint, CandidateCheckpointPhase, CandidateDeletion,
    CandidateFreshnessCheckpoint, CandidateIntegrityAssessment, CandidateIntegrityOutcome,
    EffectId, EffectKind, EffectPayload, EffectProvenance, EffectSubjectRef, Sha256Digest,
};
use security_audit::{SecurityAuditAuthor, SecurityAuditEvent};
use storage_sqlite::{
    CandidateSelectionOutcome, EventType, NewJournalEvent, NewSecurityAuditRecord,
    SecurityAuditOutcome, SecurityAuditSubject, SecurityAuditSubjectKind, SqliteStore,
    StorageError, SUPPORTED_SCHEMA_VERSION,
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
fn candidate_result_and_typed_assessment_survive_restart_as_one_commit() {
    let db = TestDb::new("candidate-assessment-restart");
    let (execution, attempt, effect_id) = {
        let mut store = db.open();
        let (execution, attempt) = seed_runtime(&mut store);
        store
            .record_candidate_selection(
                "T-EFFECT",
                execution.as_str(),
                attempt.as_str(),
                "candidate-operation-1",
                &"1".repeat(40),
                &"3".repeat(40),
                &format!("sha256:{}", "b".repeat(64)),
                1,
                &format!("sha256:{}", "c".repeat(64)),
                "criterion-1",
                "candidate.file-contains.v1",
                "src/main.rs",
                "accepted",
                10,
            )
            .unwrap();
        let subject = EffectSubjectRef::new(format!("attempt:{}", attempt.as_str())).unwrap();
        let candidate_manifest = format!("sha256:{}", "d".repeat(64));
        let mut assessment = CandidateIntegrityAssessment {
            task_id: "T-EFFECT".to_owned(),
            execution_id: execution.as_str().to_owned(),
            attempt_id: attempt.as_str().to_owned(),
            subject_ref: subject.as_str().to_owned(),
            requirement_revision: format!("sha256:{}", "b".repeat(64)),
            policy_revision: 1,
            policy_fingerprint: format!("sha256:{}", "c".repeat(64)),
            criterion_id: "criterion-1".to_owned(),
            selector: "candidate.file-contains.v1".to_owned(),
            selector_path: "src/main.rs".to_owned(),
            selector_literal: "accepted".to_owned(),
            invocation_id: "candidate-invocation-1".to_owned(),
            evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
            runtime_ref: "agentd/1".to_owned(),
            enforcement_ref: "runner-operation-1".to_owned(),
            invocation_descriptor_version: 1,
            invocation_descriptor_sha256: String::new(),
            evaluator_executable: "/trusted/agentd".to_owned(),
            toolchain_ref: "git-executable=/usr/bin/git;git version 2.54.0".to_owned(),
            runner_backend: "native".to_owned(),
            runner_plan_sha256: format!("sha256:{}", "6".repeat(64)),
            runner_enforced_axes: 3,
            runner_env_isolated: true,
            base_commit_oid: "1".repeat(40),
            base_tree_oid: "2".repeat(40),
            base_manifest_sha256: format!("sha256:{}", "a".repeat(64)),
            candidate_commit_oid: "3".repeat(40),
            candidate_tree_oid: "4".repeat(40),
            candidate_manifest_sha256: format!("sha256:{}", "d".repeat(64)),
            deletions: vec![CandidateDeletion {
                path: b"src/removed.rs".to_vec(),
                base_mode: 0o100644,
                base_blob_sha256: format!("sha256:{}", "5".repeat(64)),
            }],
            checkpoints: [
                CandidateCheckpointPhase::Selection,
                CandidateCheckpointPhase::BeforeEvaluation,
                CandidateCheckpointPhase::AfterEvaluation,
                CandidateCheckpointPhase::ResultAdmission,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, phase)| CandidateFreshnessCheckpoint {
                phase,
                observed_at_ms: 11 + index as u64,
                git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
                live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
                matched: true,
            })
            .collect(),
            outcome: CandidateIntegrityOutcome::Intact,
            started_at_ms: 10,
            completed_at_ms: 20,
        };
        assessment.refresh_invocation_descriptor_digest();
        let git_path_bound_digest = assessment.invocation_descriptor_sha256.clone();
        assessment.toolchain_ref = "git-executable=/other/git;git version 2.54.0".to_owned();
        assert_ne!(
            git_path_bound_digest,
            assessment.canonical_invocation_descriptor_digest(),
            "resolved Git executable path participates in the canonical invocation binding"
        );
        assessment.toolchain_ref = "git-executable=/usr/bin/git;git version 2.54.0".to_owned();
        assessment.refresh_invocation_descriptor_digest();
        let config = candidate_sha256(
            format!(
                "{}\0{}\0{}\0{}\0{}\0{}",
                assessment.requirement_revision,
                assessment.policy_fingerprint,
                assessment.selector,
                assessment.selector_path,
                assessment.selector_literal,
                assessment.invocation_descriptor_sha256,
            )
            .as_bytes(),
        );
        let fingerprint = subject_fingerprint(
            EffectKind::TestSuiteResult,
            subject.as_str(),
            &[
                ("content", &candidate_manifest),
                ("toolchain", &assessment.runtime_ref),
                ("config", &config),
            ],
        )
        .unwrap();
        store
            .append_security_audit(&NewSecurityAuditRecord {
                event: SecurityAuditEvent::IsolationEnforcement,
                author: SecurityAuditAuthor::Runner,
                outcome: Some(SecurityAuditOutcome::Enforced),
                occurred_at_ms: 20,
                correlation_id: assessment.enforcement_ref.clone(),
                causation_id: Some(assessment.invocation_id.clone()),
                operation_id: Some(assessment.enforcement_ref.clone()),
                task_id: Some(assessment.task_id.clone()),
                execution_id: Some(assessment.execution_id.clone()),
                attempt_id: Some(assessment.attempt_id.clone()),
                subject: Some(SecurityAuditSubject {
                    kind: SecurityAuditSubjectKind::RunnerBackend,
                    id: assessment.runner_backend.clone(),
                }),
                policy_fingerprint: Some(assessment.policy_fingerprint.clone()),
                rule_id: None,
                reason_code: None,
                approval_request_id: None,
                capabilities: vec![],
            })
            .unwrap();
        let effect = store
            .append_candidate_result(
                subject,
                fingerprint,
                EffectPayload::TestSuiteResult {
                    passed: 1,
                    failed: 0,
                    skipped: 0,
                    result_sha256: Sha256Digest::new(format!("sha256:{}", "9".repeat(64))).unwrap(),
                },
                "2026-09-23T12:00:00Z",
                EffectProvenance::new(execution.clone(), attempt.clone(), "agentd/1")
                    .unwrap()
                    .with_lineage(
                        Some("runlith.agentd.candidate-evaluator.v1".to_owned()),
                        Some("candidate-invocation-1".to_owned()),
                    )
                    .unwrap(),
                &assessment,
            )
            .unwrap();
        assert!(store
            .get_candidate_evidence(execution.as_str(), attempt.as_str())
            .is_err());
        let pending_completion = store
            .get_candidate_evidence_for_completion(execution.as_str(), attempt.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            pending_completion.integrity.deletions[0].path,
            b"src/removed.rs"
        );
        assert_eq!(
            pending_completion.integrity.invocation_descriptor_version,
            1
        );
        store
            .append_candidate_checkpoint(
                execution.as_str(),
                attempt.as_str(),
                &CandidateFreshnessCheckpoint {
                    phase: CandidateCheckpointPhase::Completion,
                    observed_at_ms: 21,
                    git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
                    live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
                    matched: true,
                },
            )
            .unwrap();
        store
            .update_candidate_operation_status(
                execution.as_str(),
                attempt.as_str(),
                "PENDING",
                "RESULT_ADMITTED",
                None,
                21,
            )
            .unwrap();
        store
            .update_candidate_operation_status(
                execution.as_str(),
                attempt.as_str(),
                "RESULT_ADMITTED",
                "COMPLETE",
                None,
                21,
            )
            .unwrap();
        (execution, attempt, effect.effect_id().clone())
    };
    let mut store = db.open();
    assert_eq!(
        store
            .get_observed_effect(&effect_id)
            .unwrap()
            .unwrap()
            .effect_kind(),
        EffectKind::TestSuiteResult
    );
    let evidence = store
        .get_candidate_evidence(execution.as_str(), attempt.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(evidence.effect.effect_kind(), EffectKind::TestSuiteResult);
    assert_eq!(
        evidence.integrity.candidate_manifest_sha256,
        format!("sha256:{}", "d".repeat(64))
    );
    assert_eq!(evidence.integrity.deletions[0].path, b"src/removed.rs");
    assert_eq!(evidence.integrity.invocation_descriptor_version, 1);
    assert_eq!(
        evidence.integrity.invocation_descriptor_sha256,
        evidence.integrity.canonical_invocation_descriptor_digest()
    );
    assert_eq!(evidence.integrity.checkpoints.len(), 5);
    assert_eq!(
        store
            .candidate_operation_status(execution.as_str(), attempt.as_str())
            .unwrap()
            .as_deref(),
        Some("COMPLETE")
    );
    store
        .append_candidate_checkpoint(
            execution.as_str(),
            attempt.as_str(),
            &CandidateFreshnessCheckpoint {
                phase: CandidateCheckpointPhase::Completion,
                observed_at_ms: 23,
                git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
                live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
                matched: true,
            },
        )
        .expect("same-subject Completion recovery is idempotent");
    assert!(store
        .append_candidate_checkpoint(
            execution.as_str(),
            attempt.as_str(),
            &CandidateFreshnessCheckpoint {
                phase: CandidateCheckpointPhase::Completion,
                observed_at_ms: 24,
                git_metadata_fingerprint: format!("sha256:{}", "0".repeat(64)),
                live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
                matched: true,
            },
        )
        .is_err());
    store
        .append_candidate_checkpoint(
            execution.as_str(),
            attempt.as_str(),
            &CandidateFreshnessCheckpoint {
                phase: CandidateCheckpointPhase::RestartReuse,
                observed_at_ms: 22,
                git_metadata_fingerprint: format!("sha256:{}", "7".repeat(64)),
                live_worktree_manifest_sha256: format!("sha256:{}", "8".repeat(64)),
                matched: true,
            },
        )
        .unwrap();
    assert_eq!(
        store
            .get_candidate_evidence(execution.as_str(), attempt.as_str())
            .unwrap()
            .unwrap()
            .integrity
            .checkpoints
            .len(),
        6
    );
}

#[test]
fn candidate_mismatch_and_blocked_operation_statuses_survive_restart() {
    for (status, detail) in [("MISMATCH", "freshness-mismatch"), ("BLOCKED", "g6-denied")] {
        let db = TestDb::new(&format!("candidate-operation-{status}"));
        let mut store = db.open();
        let (execution, attempt) = seed_runtime(&mut store);
        store
            .record_candidate_selection(
                "T-EFFECT",
                execution.as_str(),
                attempt.as_str(),
                "candidate-operation-failure",
                &"1".repeat(40),
                &"3".repeat(40),
                &format!("sha256:{}", "b".repeat(64)),
                1,
                &format!("sha256:{}", "c".repeat(64)),
                "criterion-1",
                "candidate.file-contains.v1",
                "src/main.rs",
                "accepted",
                10,
            )
            .unwrap();
        store
            .record_candidate_operation_failure(
                execution.as_str(),
                attempt.as_str(),
                status,
                detail,
                11,
            )
            .unwrap();
        drop(store);
        let store = db.open();
        assert_eq!(
            store
                .candidate_operation_status(execution.as_str(), attempt.as_str())
                .unwrap()
                .as_deref(),
            Some(status)
        );
        let history = store
            .candidate_operation_history(execution.as_str(), attempt.as_str())
            .unwrap()
            .unwrap();
        assert!(history.starts_with("PENDING\n"));
        assert!(history.contains(detail));
    }
}

/// TASK-0144 R2 (owner review 2026-09-25, MAJOR): `record_candidate_operation_failure`
/// must never regress a durable `RESULT_ADMITTED` or `COMPLETE` row — the
/// prior unconditional `UPDATE` could downgrade either into a failure
/// status. Exercises the storage-layer compare-and-swap guard and its
/// DB-level trigger backstop (`candidate_input_operations_status_no_regression`,
/// migration 0022) directly, complementing the real-evaluator coverage in
/// `bins/agentd/src/execution.rs`.
#[test]
fn candidate_operation_failure_never_regresses_result_admitted_or_complete() {
    for terminal in ["RESULT_ADMITTED", "COMPLETE"] {
        let db = TestDb::new(&format!("candidate-no-regression-{terminal}"));
        let mut store = db.open();
        let (execution, attempt) = seed_runtime(&mut store);
        store
            .record_candidate_selection(
                "T-EFFECT",
                execution.as_str(),
                attempt.as_str(),
                &format!("candidate-no-regression-{terminal}"),
                &"1".repeat(40),
                &"3".repeat(40),
                &format!("sha256:{}", "b".repeat(64)),
                1,
                &format!("sha256:{}", "c".repeat(64)),
                "criterion-1",
                "candidate.file-contains.v1",
                "src/main.rs",
                "accepted",
                10,
            )
            .unwrap();
        store
            .update_candidate_operation_status(
                execution.as_str(),
                attempt.as_str(),
                "PENDING",
                "RESULT_ADMITTED",
                None,
                11,
            )
            .unwrap();
        if terminal == "COMPLETE" {
            store
                .update_candidate_operation_status(
                    execution.as_str(),
                    attempt.as_str(),
                    "RESULT_ADMITTED",
                    "COMPLETE",
                    None,
                    12,
                )
                .unwrap();
        }

        // A failure classified after reaching the terminal state must not
        // change `status`, but must still succeed (journaling the failure)
        // rather than erroring.
        store
            .record_candidate_operation_failure(
                execution.as_str(),
                attempt.as_str(),
                "MISMATCH",
                "late-failure-must-not-regress",
                13,
            )
            .unwrap();
        assert_eq!(
            store
                .candidate_operation_status(execution.as_str(), attempt.as_str())
                .unwrap()
                .as_deref(),
            Some(terminal),
            "a failure after {terminal} must not regress the durable status"
        );
        let history = store
            .candidate_operation_history(execution.as_str(), attempt.as_str())
            .unwrap()
            .unwrap();
        assert!(
            history.contains("late-failure-must-not-regress"),
            "the failure must still be journaled: {history:?}"
        );

        // The DB-level trigger is a backstop against any writer that
        // bypasses the guarded store methods: a raw UPDATE regressing
        // status away from a terminal state must itself fail. A separate
        // raw connection is used because `SqliteStore::connection` is
        // crate-private.
        drop(store);
        let raw_connection = rusqlite::Connection::open(&db.path).unwrap();
        let raw = raw_connection.execute(
            "UPDATE candidate_input_operations SET status = 'BLOCKED' \
             WHERE execution_id = ?1 AND attempt_id = ?2",
            rusqlite::params![execution.as_str(), attempt.as_str()],
        );
        assert!(
            raw.is_err(),
            "a raw UPDATE regressing {terminal} must be rejected by the DB trigger"
        );
        drop(raw_connection);
        let store = db.open();
        assert_eq!(
            store
                .candidate_operation_status(execution.as_str(), attempt.as_str())
                .unwrap()
                .as_deref(),
            Some(terminal),
            "a rejected raw UPDATE must leave the durable status unchanged"
        );
    }
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
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 35);
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
fn candidate_failure_after_admission_is_sticky_and_blocks_reuse() {
    for terminal in ["RESULT_ADMITTED", "COMPLETE"] {
        let db = TestDb::new(&format!("candidate-sticky-invalidation-{terminal}"));
        let mut store = db.open();
        let (execution, attempt) = seed_runtime(&mut store);
        store
            .record_candidate_selection(
                "T-EFFECT",
                execution.as_str(),
                attempt.as_str(),
                &format!("candidate-sticky-invalidation-{terminal}"),
                &"1".repeat(40),
                &"3".repeat(40),
                &format!("sha256:{}", "b".repeat(64)),
                1,
                &format!("sha256:{}", "c".repeat(64)),
                "criterion-1",
                "candidate.file-contains.v1",
                "src/main.rs",
                "accepted",
                10,
            )
            .unwrap();
        store
            .update_candidate_operation_status(
                execution.as_str(),
                attempt.as_str(),
                "PENDING",
                "RESULT_ADMITTED",
                None,
                11,
            )
            .unwrap();
        if terminal == "COMPLETE" {
            store
                .update_candidate_operation_status(
                    execution.as_str(),
                    attempt.as_str(),
                    "RESULT_ADMITTED",
                    "COMPLETE",
                    None,
                    12,
                )
                .unwrap();
        }

        // Invalidate after reaching the terminal state
        store
            .invalidate_candidate_operation(
                execution.as_str(),
                attempt.as_str(),
                "TAMPERED",
                "post-admission-tamper",
                13,
            )
            .unwrap();

        // Status must still be the terminal state (no regression)
        assert_eq!(
            store
                .candidate_operation_status(execution.as_str(), attempt.as_str())
                .unwrap()
                .as_deref(),
            Some(terminal),
            "status must not regress after invalidation"
        );

        // Invalidation must be set and sticky
        let invalidation = store
            .candidate_operation_invalidation(execution.as_str(), attempt.as_str())
            .unwrap();
        assert!(invalidation.is_some(), "invalidation must be recorded");
        let (_, code) = invalidation.unwrap();
        assert!(
            code.starts_with("TAMPERED:"),
            "invalidation code must start with TAMPERED: got {code}"
        );

        // All evidence getters must return ConstraintViolation when invalidated
        let evidence_err = store
            .get_candidate_evidence(execution.as_str(), attempt.as_str())
            .unwrap_err();
        assert!(
            matches!(evidence_err, StorageError::ConstraintViolation(_)),
            "get_candidate_evidence must fail with ConstraintViolation when invalidated"
        );

        let evidence_for_completion_err = store
            .get_candidate_evidence_for_completion(execution.as_str(), attempt.as_str())
            .unwrap_err();
        assert!(
            matches!(evidence_for_completion_err, StorageError::ConstraintViolation(_)),
            "get_candidate_evidence_for_completion must fail with ConstraintViolation when invalidated"
        );

        let assessment_err = store
            .get_candidate_integrity_assessment(execution.as_str(), attempt.as_str())
            .unwrap_err();
        assert!(
            matches!(assessment_err, StorageError::ConstraintViolation(_)),
            "get_candidate_integrity_assessment must fail with ConstraintViolation when invalidated"
        );

        // append_candidate_checkpoint must refuse when invalidated
        let checkpoint_err = store
            .append_candidate_checkpoint(
                execution.as_str(),
                attempt.as_str(),
                &CandidateFreshnessCheckpoint {
                    phase: CandidateCheckpointPhase::Completion,
                    observed_at_ms: 14,
                    git_metadata_fingerprint: "fp".to_owned(),
                    live_worktree_manifest_sha256: format!("sha256:{}", "d".repeat(64)),
                    matched: true,
                },
            )
            .unwrap_err();
        assert!(
            matches!(checkpoint_err, StorageError::ConstraintViolation(_)),
            "append_candidate_checkpoint must fail with ConstraintViolation when invalidated"
        );

        // update_candidate_operation_status must refuse when invalidated
        if terminal == "RESULT_ADMITTED" {
            let status_err = store
                .update_candidate_operation_status(
                    execution.as_str(),
                    attempt.as_str(),
                    "RESULT_ADMITTED",
                    "COMPLETE",
                    None,
                    15,
                )
                .unwrap_err();
            assert!(
                matches!(status_err, StorageError::ConstraintViolation(_)),
                "update_candidate_operation_status must fail with ConstraintViolation when invalidated"
            );
        }

        // A second failure record must not change the invalidated_at_ms/code (idempotent)
        store
            .record_candidate_operation_failure(
                execution.as_str(),
                attempt.as_str(),
                "MISMATCH",
                "second-failure",
                16,
            )
            .unwrap();
        let invalidation2 = store
            .candidate_operation_invalidation(execution.as_str(), attempt.as_str())
            .unwrap();
        assert_eq!(
            invalidation2,
            Some((13, "TAMPERED:post-admission-tamper".to_owned())),
            "invalidation must be unchanged by a second failure record"
        );

        // A raw SQL UPDATE attempting to clear invalidation must fail (trigger)
        drop(store);
        let raw_connection = rusqlite::Connection::open(&db.path).unwrap();
        let raw = raw_connection.execute(
            "UPDATE candidate_input_operations SET invalidated_at_ms = NULL, invalidation_code = NULL \
             WHERE execution_id = ?1 AND attempt_id = ?2",
            rusqlite::params![execution.as_str(), attempt.as_str()],
        );
        assert!(
            raw.is_err(),
            "a raw UPDATE attempting to clear invalidation must be rejected by the trigger"
        );
    }
}

#[test]
fn candidate_journal_only_failure_after_admission_keeps_evidence_valid() {
    // Verify that record_candidate_operation_failure (journal-only, no invalidation)
    // does not block evidence access or prevent completion transitions on admitted rows.
    let db = TestDb::new("candidate-journal-only-failure");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-journal-only",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            10,
        )
        .unwrap();
    store
        .update_candidate_operation_status(
            execution.as_str(),
            attempt.as_str(),
            "PENDING",
            "RESULT_ADMITTED",
            None,
            11,
        )
        .unwrap();

    // Record a journal-only failure (non-integrity error, precondition failure, etc.)
    store
        .record_candidate_operation_failure(
            execution.as_str(),
            attempt.as_str(),
            "BLOCKED",
            "transient-error",
            12,
        )
        .unwrap();

    // Status must still be RESULT_ADMITTED (no regression)
    assert_eq!(
        store
            .candidate_operation_status(execution.as_str(), attempt.as_str())
            .unwrap()
            .as_deref(),
        Some("RESULT_ADMITTED"),
        "journal-only failure must not regress status"
    );

    // Invalidation must be None (not recorded)
    let invalidation = store
        .candidate_operation_invalidation(execution.as_str(), attempt.as_str())
        .unwrap();
    assert_eq!(
        invalidation, None,
        "journal-only failure must not set invalidation"
    );

    // History must contain the failure entry
    let history = store
        .candidate_operation_history(execution.as_str(), attempt.as_str())
        .unwrap();
    assert!(
        history.is_some_and(|h| h.contains("BLOCKED:transient-error")),
        "failure must be journaled in status_history"
    );

    // Evidence getters must still work (not blocked)
    // Note: load_candidate_evidence requires Completion checkpoint, so only test the
    // lower-level assertion: invalidation check should pass.
    assert!(
        store
            .candidate_operation_invalidation(execution.as_str(), attempt.as_str())
            .unwrap()
            .is_none(),
        "evidence must still be accessible after journal-only failure"
    );

    // COMPLETE transition must still be possible
    store
        .update_candidate_operation_status(
            execution.as_str(),
            attempt.as_str(),
            "RESULT_ADMITTED",
            "COMPLETE",
            None,
            13,
        )
        .unwrap();
    assert_eq!(
        store
            .candidate_operation_status(execution.as_str(), attempt.as_str())
            .unwrap()
            .as_deref(),
        Some("COMPLETE"),
        "COMPLETE transition must succeed after journal-only failure"
    );
}

#[test]
fn candidate_invalidate_with_invalid_status_fails() {
    // Verify that invalidate_candidate_operation rejects BLOCKED status
    let db = TestDb::new("candidate-invalidate-blocked-status");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-invalid-status",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            10,
        )
        .unwrap();

    // Attempting to invalidate with BLOCKED status must fail
    let result = store.invalidate_candidate_operation(
        execution.as_str(),
        attempt.as_str(),
        "BLOCKED",
        "invalid-status",
        11,
    );
    assert!(
        matches!(result, Err(StorageError::ConstraintViolation(_))),
        "invalidate_candidate_operation with BLOCKED status must fail"
    );
}

#[test]
fn cv_2_arbitrary_effect_id_cannot_be_selected_by_caller() {
    let db = TestDb::new("id-owned-by-store");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    let effect = append_sample(&mut store, &execution, &attempt, "artifact:id");
    assert_eq!(effect.effect_id(), &EffectId::new("effect-1").unwrap());
}

#[test]
fn candidate_selection_distinguishes_subject_mismatch_from_incomplete() {
    let db = TestDb::new("candidate-selection-outcomes");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);

    // First record: new row inserted
    let result = store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-op-1",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            10,
        )
        .unwrap();
    assert_eq!(result, CandidateSelectionOutcome::Recorded);

    // Same subject while PENDING: returns Incomplete
    let result = store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-op-1",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            11,
        )
        .unwrap();
    assert_eq!(result, CandidateSelectionOutcome::Incomplete);

    // Different selected_commit_oid: returns SubjectMismatch
    let result = store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-op-1",
            &"1".repeat(40),
            &"4".repeat(40), // Different
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            12,
        )
        .unwrap();
    assert_eq!(result, CandidateSelectionOutcome::SubjectMismatch);

    // Advance to RESULT_ADMITTED
    store
        .update_candidate_operation_status(
            execution.as_str(),
            attempt.as_str(),
            "PENDING",
            "RESULT_ADMITTED",
            Some("admitted"),
            13,
        )
        .unwrap();

    // Same subject after admission: returns ReuseAdmitted
    let result = store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-op-1",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            14,
        )
        .unwrap();
    assert_eq!(result, CandidateSelectionOutcome::ReuseAdmitted);
}

#[test]
fn invalidate_without_operation_row_returns_false() {
    let db = TestDb::new("invalidate-without-row");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);

    // Try to invalidate a non-existent operation row
    let result = store.invalidate_candidate_operation(
        execution.as_str(),
        attempt.as_str(),
        "TAMPERED",
        "test-code",
        now_millis(),
    );

    // Should return Ok(false), not an error
    assert!(
        !result.unwrap(),
        "invalidate should return false for non-existent row"
    );
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// F15: the assessment must pair with the selection recorded for the same
// attempt, not merely be internally consistent.
fn pairing_assessment(
    execution: &ExecutionId,
    attempt: &AttemptId,
    mutate: impl FnOnce(&mut CandidateIntegrityAssessment),
) -> CandidateIntegrityAssessment {
    let mut assessment = CandidateIntegrityAssessment {
        task_id: "T-EFFECT".to_owned(),
        execution_id: execution.as_str().to_owned(),
        attempt_id: attempt.as_str().to_owned(),
        subject_ref: format!("attempt:{}", attempt.as_str()),
        requirement_revision: format!("sha256:{}", "b".repeat(64)),
        policy_revision: 1,
        policy_fingerprint: format!("sha256:{}", "c".repeat(64)),
        criterion_id: "criterion-1".to_owned(),
        selector: "candidate.file-contains.v1".to_owned(),
        selector_path: "src/main.rs".to_owned(),
        selector_literal: "accepted".to_owned(),
        invocation_id: "candidate-invocation-1".to_owned(),
        evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
        runtime_ref: "agentd/1".to_owned(),
        enforcement_ref: "runner-operation-1".to_owned(),
        invocation_descriptor_version: 1,
        invocation_descriptor_sha256: String::new(),
        evaluator_executable: "/trusted/agentd".to_owned(),
        toolchain_ref: "git-executable=/usr/bin/git;git version 2.54.0".to_owned(),
        runner_backend: "native".to_owned(),
        runner_plan_sha256: format!("sha256:{}", "6".repeat(64)),
        runner_enforced_axes: 3,
        runner_env_isolated: true,
        base_commit_oid: "1".repeat(40),
        base_tree_oid: "2".repeat(40),
        base_manifest_sha256: format!("sha256:{}", "a".repeat(64)),
        candidate_commit_oid: "3".repeat(40),
        candidate_tree_oid: "4".repeat(40),
        candidate_manifest_sha256: format!("sha256:{}", "d".repeat(64)),
        deletions: Vec::new(),
        checkpoints: [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, phase)| CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms: 11 + index as u64,
            git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
            live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
            matched: true,
        })
        .collect(),
        outcome: CandidateIntegrityOutcome::Intact,
        started_at_ms: 10,
        completed_at_ms: 20,
    };
    mutate(&mut assessment);
    assessment.refresh_invocation_descriptor_digest();
    assessment
}

fn append_paired(
    store: &mut SqliteStore,
    execution: &ExecutionId,
    attempt: &AttemptId,
    assessment: &CandidateIntegrityAssessment,
) -> Result<evaluation::ObservedEffect, StorageError> {
    let subject = EffectSubjectRef::new(assessment.subject_ref.clone()).unwrap();
    let config = candidate_sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            assessment.requirement_revision,
            assessment.policy_fingerprint,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_descriptor_sha256,
        )
        .as_bytes(),
    );
    let fingerprint = subject_fingerprint(
        EffectKind::TestSuiteResult,
        subject.as_str(),
        &[
            ("content", &assessment.candidate_manifest_sha256),
            ("toolchain", &assessment.runtime_ref),
            ("config", &config),
        ],
    )
    .unwrap();
    store.append_candidate_result(
        subject,
        fingerprint,
        EffectPayload::TestSuiteResult {
            passed: 1,
            failed: 0,
            skipped: 0,
            result_sha256: Sha256Digest::new(format!("sha256:{}", "9".repeat(64))).unwrap(),
        },
        "2026-09-23T12:00:00Z",
        EffectProvenance::new(execution.clone(), attempt.clone(), "agentd/1")
            .unwrap()
            .with_lineage(
                Some("runlith.agentd.candidate-evaluator.v1".to_owned()),
                Some("candidate-invocation-1".to_owned()),
            )
            .unwrap(),
        assessment,
    )
}

#[test]
fn candidate_result_must_pair_with_the_recorded_selection() {
    type Mutation = fn(&mut CandidateIntegrityAssessment);
    let mismatches: [(&str, Mutation); 6] = [
        ("policy-fingerprint", |a| {
            a.policy_fingerprint = format!("sha256:{}", "7".repeat(64));
        }),
        ("requirement-revision", |a| {
            a.requirement_revision = format!("sha256:{}", "7".repeat(64));
        }),
        ("policy-revision", |a| a.policy_revision = 2),
        ("candidate-commit", |a| {
            a.candidate_commit_oid = "8".repeat(40)
        }),
        ("selector-literal", |a| {
            a.selector_literal = "other".to_owned()
        }),
        ("criterion", |a| a.criterion_id = "criterion-2".to_owned()),
    ];
    for (name, mutate) in mismatches {
        let db = TestDb::new(&format!("pairing-{name}"));
        let mut store = db.open();
        let (execution, attempt) = seed_runtime(&mut store);
        store
            .record_candidate_selection(
                "T-EFFECT",
                execution.as_str(),
                attempt.as_str(),
                "candidate-operation-1",
                &"1".repeat(40),
                &"3".repeat(40),
                &format!("sha256:{}", "b".repeat(64)),
                1,
                &format!("sha256:{}", "c".repeat(64)),
                "criterion-1",
                "candidate.file-contains.v1",
                "src/main.rs",
                "accepted",
                10,
            )
            .unwrap();
        let assessment = pairing_assessment(&execution, &attempt, mutate);
        let result = append_paired(&mut store, &execution, &attempt, &assessment);
        assert!(
            matches!(result, Err(StorageError::ConstraintViolation(_))),
            "{name}: expected ConstraintViolation, got {result:?}"
        );
    }
}

#[test]
fn candidate_result_without_a_recorded_selection_is_refused() {
    let db = TestDb::new("pairing-no-selection");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    let assessment = pairing_assessment(&execution, &attempt, |_| {});
    let result = append_paired(&mut store, &execution, &attempt, &assessment);
    assert!(
        matches!(result, Err(StorageError::ConstraintViolation(_))),
        "got {result:?}"
    );
}

// F16: edits that keep the row type-valid (same kind, well-formed payload,
// valid timestamp, plausible receipt) must still fail the record fingerprint.
#[test]
fn type_valid_edits_to_an_observed_effect_fail_the_record_fingerprint() {
    let edits = [
        (
            "payload",
            "payload_ref",
            "'bytes=999;sha256=sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
        ),
        ("captured_at", "captured_at", "'2026-09-24T12:00:00Z'"),
        ("receipt", "receipt_ref", "'forged-receipt'"),
        ("runtime", "runtime_ref", "'other-runtime'"),
    ];
    for (name, column, value) in edits {
        let db = TestDb::new(&format!("fingerprint-{name}"));
        let effect_id = {
            let mut store = db.open();
            let (execution, attempt) = seed_runtime(&mut store);
            append_sample(&mut store, &execution, &attempt, "artifact:tamper")
                .effect_id()
                .clone()
        };
        rusqlite::Connection::open(&db.path)
            .unwrap()
            .execute(
                &format!("UPDATE observed_effects SET {column} = {value} WHERE effect_id = ?1"),
                [effect_id.as_str()],
            )
            .unwrap();
        let store = db.open();
        assert!(
            matches!(
                store.get_observed_effect(&effect_id),
                Err(StorageError::Corrupt(_))
            ),
            "edit {name} was accepted"
        );
    }
}
// TASK-0151 storage hardening: compact candidate fixture reusing the
// known-good values from candidate_result_and_typed_assessment test.
struct CandidateSeed {
    execution: ExecutionId,
    attempt: AttemptId,
    effect_id: EffectId,
}

fn seed_candidate(store: &mut SqliteStore) -> CandidateSeed {
    let (execution, attempt) = seed_runtime(store);
    store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-operation-1",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            10,
        )
        .unwrap();
    let subject = EffectSubjectRef::new(format!("attempt:{}", attempt.as_str())).unwrap();
    let candidate_manifest = format!("sha256:{}", "d".repeat(64));
    let mut assessment = CandidateIntegrityAssessment {
        task_id: "T-EFFECT".to_owned(),
        execution_id: execution.as_str().to_owned(),
        attempt_id: attempt.as_str().to_owned(),
        subject_ref: subject.as_str().to_owned(),
        requirement_revision: format!("sha256:{}", "b".repeat(64)),
        policy_revision: 1,
        policy_fingerprint: format!("sha256:{}", "c".repeat(64)),
        criterion_id: "criterion-1".to_owned(),
        selector: "candidate.file-contains.v1".to_owned(),
        selector_path: "src/main.rs".to_owned(),
        selector_literal: "accepted".to_owned(),
        invocation_id: "candidate-invocation-1".to_owned(),
        evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
        runtime_ref: "agentd/1".to_owned(),
        enforcement_ref: "runner-operation-1".to_owned(),
        invocation_descriptor_version: 1,
        invocation_descriptor_sha256: String::new(),
        evaluator_executable: "/trusted/agentd".to_owned(),
        toolchain_ref: "git-executable=/usr/bin/git;git version 2.54.0".to_owned(),
        runner_backend: "native".to_owned(),
        runner_plan_sha256: format!("sha256:{}", "6".repeat(64)),
        runner_enforced_axes: 3,
        runner_env_isolated: true,
        base_commit_oid: "1".repeat(40),
        base_tree_oid: "2".repeat(40),
        base_manifest_sha256: format!("sha256:{}", "a".repeat(64)),
        candidate_commit_oid: "3".repeat(40),
        candidate_tree_oid: "4".repeat(40),
        candidate_manifest_sha256: candidate_manifest.clone(),
        deletions: vec![],
        checkpoints: [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, phase)| CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms: 11 + index as u64,
            git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
            live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
            matched: true,
        })
        .collect(),
        outcome: CandidateIntegrityOutcome::Intact,
        started_at_ms: 10,
        completed_at_ms: 20,
    };
    assessment.refresh_invocation_descriptor_digest();
    let config = candidate_sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            assessment.requirement_revision,
            assessment.policy_fingerprint,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_descriptor_sha256,
        )
        .as_bytes(),
    );
    let fingerprint = subject_fingerprint(
        EffectKind::TestSuiteResult,
        subject.as_str(),
        &[
            ("content", &candidate_manifest),
            ("toolchain", &assessment.runtime_ref),
            ("config", &config),
        ],
    )
    .unwrap();
    store
        .append_security_audit(&NewSecurityAuditRecord {
            event: SecurityAuditEvent::IsolationEnforcement,
            author: SecurityAuditAuthor::Runner,
            outcome: Some(SecurityAuditOutcome::Enforced),
            occurred_at_ms: 20,
            correlation_id: assessment.enforcement_ref.clone(),
            causation_id: Some(assessment.invocation_id.clone()),
            operation_id: Some(assessment.enforcement_ref.clone()),
            task_id: Some(assessment.task_id.clone()),
            execution_id: Some(assessment.execution_id.clone()),
            attempt_id: Some(assessment.attempt_id.clone()),
            subject: Some(SecurityAuditSubject {
                kind: SecurityAuditSubjectKind::RunnerBackend,
                id: assessment.runner_backend.clone(),
            }),
            policy_fingerprint: Some(assessment.policy_fingerprint.clone()),
            rule_id: None,
            reason_code: None,
            approval_request_id: None,
            capabilities: vec![],
        })
        .unwrap();
    let effect = store
        .append_candidate_result(
            subject,
            fingerprint,
            EffectPayload::TestSuiteResult {
                passed: 1,
                failed: 0,
                skipped: 0,
                result_sha256: Sha256Digest::new(format!("sha256:{}", "9".repeat(64))).unwrap(),
            },
            "2026-09-23T12:00:00Z",
            EffectProvenance::new(execution.clone(), attempt.clone(), "agentd/1")
                .unwrap()
                .with_lineage(
                    Some("runlith.agentd.candidate-evaluator.v1".to_owned()),
                    Some("candidate-invocation-1".to_owned()),
                )
                .unwrap(),
            &assessment,
        )
        .unwrap();
    CandidateSeed {
        execution,
        attempt,
        effect_id: effect.effect_id().clone(),
    }
}

/// Raw-SQL probe handle: trigger/migration guarantees have no public
/// API by design, so these tests open the database file directly.
fn raw(path: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(path).unwrap()
}

// Finding 1: candidate operation rows must be delete-proof.
#[test]
fn candidate_operation_rows_cannot_be_deleted() {
    let db = TestDb::new("candidate-no-delete");
    let mut store = db.open();
    let seed = seed_candidate(&mut store);
    drop(store);
    let conn = raw(&db.path);
    let deleted = conn.execute(
        "DELETE FROM candidate_input_operations WHERE execution_id = ?1",
        [seed.execution.as_str()],
    );
    assert!(
        deleted.is_err(),
        "a BEFORE DELETE guard must refuse operation-row deletion"
    );
    let remaining: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM candidate_input_operations WHERE execution_id = ?1",
            [seed.execution.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 1, "the operation row must survive");
}

// Finding 2: appending a result onto an invalidated operation refuses.
#[test]
fn append_candidate_result_refuses_invalidated_operation() {
    let db = TestDb::new("candidate-append-invalidated");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-operation-1",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            10,
        )
        .unwrap();
    // Drive to a terminal state, then invalidate (mirrors the sticky test).
    for (from, to) in [
        ("PENDING", "RESULT_ADMITTED"),
        ("RESULT_ADMITTED", "COMPLETE"),
    ] {
        store
            .update_candidate_operation_status(
                execution.as_str(),
                attempt.as_str(),
                from,
                to,
                None,
                11,
            )
            .unwrap();
    }
    store
        .invalidate_candidate_operation(
            execution.as_str(),
            attempt.as_str(),
            "TAMPERED",
            "post-admission-tamper",
            13,
        )
        .unwrap();
    drop(store);
    // A late result for the invalidated operation must be refused even
    // though lineage and fingerprints check out.
    let mut store = db.open();
    let subject = EffectSubjectRef::new(format!("attempt:{}", attempt.as_str())).unwrap();
    let candidate_manifest = format!("sha256:{}", "d".repeat(64));
    let mut assessment = CandidateIntegrityAssessment {
        task_id: "T-EFFECT".to_owned(),
        execution_id: execution.as_str().to_owned(),
        attempt_id: attempt.as_str().to_owned(),
        subject_ref: subject.as_str().to_owned(),
        requirement_revision: format!("sha256:{}", "b".repeat(64)),
        policy_revision: 1,
        policy_fingerprint: format!("sha256:{}", "c".repeat(64)),
        criterion_id: "criterion-1".to_owned(),
        selector: "candidate.file-contains.v1".to_owned(),
        selector_path: "src/main.rs".to_owned(),
        selector_literal: "accepted".to_owned(),
        invocation_id: "candidate-invocation-1".to_owned(),
        evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
        runtime_ref: "agentd/1".to_owned(),
        enforcement_ref: "runner-operation-1".to_owned(),
        invocation_descriptor_version: 1,
        invocation_descriptor_sha256: String::new(),
        evaluator_executable: "/trusted/agentd".to_owned(),
        toolchain_ref: "git-executable=/usr/bin/git;git version 2.54.0".to_owned(),
        runner_backend: "native".to_owned(),
        runner_plan_sha256: format!("sha256:{}", "6".repeat(64)),
        runner_enforced_axes: 3,
        runner_env_isolated: true,
        base_commit_oid: "1".repeat(40),
        base_tree_oid: "2".repeat(40),
        base_manifest_sha256: format!("sha256:{}", "a".repeat(64)),
        candidate_commit_oid: "3".repeat(40),
        candidate_tree_oid: "4".repeat(40),
        candidate_manifest_sha256: candidate_manifest.clone(),
        deletions: vec![],
        checkpoints: [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, phase)| CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms: 11 + index as u64,
            git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
            live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
            matched: true,
        })
        .collect(),
        outcome: CandidateIntegrityOutcome::Intact,
        started_at_ms: 10,
        completed_at_ms: 20,
    };
    assessment.refresh_invocation_descriptor_digest();
    let config = candidate_sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            assessment.requirement_revision,
            assessment.policy_fingerprint,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_descriptor_sha256,
        )
        .as_bytes(),
    );
    let fingerprint = subject_fingerprint(
        EffectKind::TestSuiteResult,
        subject.as_str(),
        &[
            ("content", &candidate_manifest),
            ("toolchain", &assessment.runtime_ref),
            ("config", &config),
        ],
    )
    .unwrap();
    let refused = store.append_candidate_result(
        subject,
        fingerprint,
        EffectPayload::TestSuiteResult {
            passed: 1,
            failed: 0,
            skipped: 0,
            result_sha256: Sha256Digest::new(format!("sha256:{}", "9".repeat(64))).unwrap(),
        },
        "2026-09-23T12:00:00Z",
        EffectProvenance::new(execution.clone(), attempt.clone(), "agentd/1")
            .unwrap()
            .with_lineage(
                Some("runlith.agentd.candidate-evaluator.v1".to_owned()),
                Some("candidate-invocation-1".to_owned()),
            )
            .unwrap(),
        &assessment,
    );
    assert!(
        refused.is_err(),
        "appending onto an invalidated operation must refuse"
    );
}

// Findings 3+4: invalidated results are unreadable as evidence, and the
// read-plus-invalidation check is one atomic statement.
#[test]
fn get_observed_effect_quarantines_invalidated_results() {
    let db = TestDb::new("candidate-read-invalidated");
    let mut store = db.open();
    let seed = seed_candidate(&mut store);
    for (from, to) in [
        ("PENDING", "RESULT_ADMITTED"),
        ("RESULT_ADMITTED", "COMPLETE"),
    ] {
        store
            .update_candidate_operation_status(
                seed.execution.as_str(),
                seed.attempt.as_str(),
                from,
                to,
                None,
                21,
            )
            .unwrap();
    }
    assert!(store
        .get_observed_effect(&seed.effect_id)
        .unwrap()
        .is_some());
    store
        .invalidate_candidate_operation(
            seed.execution.as_str(),
            seed.attempt.as_str(),
            "TAMPERED",
            "post-admission-tamper",
            22,
        )
        .unwrap();
    assert!(
        store
            .get_observed_effect(&seed.effect_id)
            .unwrap()
            .is_none(),
        "an invalidated result must not resolve as evidence"
    );
}

// 0033 backfill must fingerprint quarantined effects too: it enumerates
// every row, so it must read past the evidence quarantine instead of
// failing the whole migration as Corrupt.
#[test]
fn upgrade_backfills_fingerprint_for_quarantined_effect() {
    let db = TestDb::new("quarantine-backfill-0033");
    let effect_id = {
        let mut store = db.open();
        let seed = seed_candidate(&mut store);
        for (from, to) in [
            ("PENDING", "RESULT_ADMITTED"),
            ("RESULT_ADMITTED", "COMPLETE"),
        ] {
            store
                .update_candidate_operation_status(
                    seed.execution.as_str(),
                    seed.attempt.as_str(),
                    from,
                    to,
                    None,
                    21,
                )
                .unwrap();
        }
        store
            .invalidate_candidate_operation(
                seed.execution.as_str(),
                seed.attempt.as_str(),
                "TAMPERED",
                "post-admission-tamper",
                22,
            )
            .unwrap();
        assert!(store
            .get_observed_effect(&seed.effect_id)
            .unwrap()
            .is_none());
        seed.effect_id
    };
    // Rewind to the pre-0033 shape: fingerprint column gone.
    raw(&db.path)
        .execute_batch(
            "ALTER TABLE observed_effects DROP COLUMN record_fingerprint;
             DROP TABLE IF EXISTS entity_links;
             DROP TABLE IF EXISTS entities;
             DROP TABLE IF EXISTS memory_links;
             DROP TABLE IF EXISTS memory_anchors;
             DROP TABLE IF EXISTS memory_source_refs;
             DROP TABLE IF EXISTS routine_runs;
             DROP TABLE IF EXISTS routines;
             DROP TABLE IF EXISTS memories;
             PRAGMA user_version = 32;",
        )
        .unwrap();
    // Upgrade must succeed (not fail with Corrupt 'observed effect vanished').
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 35);
    // Fingerprint backfilled although the row stays quarantined.
    let fingerprint: Option<String> = raw(&db.path)
        .query_row(
            "SELECT record_fingerprint FROM observed_effects WHERE effect_id = ?1",
            [effect_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(fingerprint.is_some());
    assert!(store.get_observed_effect(&effect_id).unwrap().is_none());
}

// Findings 3+4, consumer side: the per-attempt effect list (the mint
// basis) skips quarantined rows instead of erroring.
#[test]
fn attempt_effect_list_skips_invalidated_results() {
    let db = TestDb::new("candidate-list-invalidated");
    let mut store = db.open();
    let seed = seed_candidate(&mut store);
    for (from, to) in [
        ("PENDING", "RESULT_ADMITTED"),
        ("RESULT_ADMITTED", "COMPLETE"),
    ] {
        store
            .update_candidate_operation_status(
                seed.execution.as_str(),
                seed.attempt.as_str(),
                from,
                to,
                None,
                21,
            )
            .unwrap();
    }
    assert_eq!(
        store
            .list_observed_effects_for_attempt(&seed.execution, &seed.attempt)
            .unwrap()
            .len(),
        1
    );
    store
        .invalidate_candidate_operation(
            seed.execution.as_str(),
            seed.attempt.as_str(),
            "TAMPERED",
            "post-admission-tamper",
            22,
        )
        .unwrap();
    assert!(
        store
            .list_observed_effects_for_attempt(&seed.execution, &seed.attempt)
            .unwrap()
            .is_empty(),
        "quarantined rows must vanish from the evidence list"
    );
}

// Finding 5: repeated failures keep appending; history keeps its tail.
#[test]
fn status_history_keeps_tail_under_repeated_failures() {
    let db = TestDb::new("candidate-status-tail");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    store
        .record_candidate_selection(
            "T-EFFECT",
            execution.as_str(),
            attempt.as_str(),
            "candidate-operation-1",
            &"1".repeat(40),
            &"3".repeat(40),
            &format!("sha256:{}", "b".repeat(64)),
            1,
            &format!("sha256:{}", "c".repeat(64)),
            "criterion-1",
            "candidate.file-contains.v1",
            "src/main.rs",
            "accepted",
            10,
        )
        .unwrap();
    for index in 0..300u64 {
        store
            .record_candidate_operation_failure(
                execution.as_str(),
                attempt.as_str(),
                "MISMATCH",
                &format!("freshness-mismatch-{index:04}-{}", "x".repeat(48)),
                100 + index,
            )
            .unwrap_or_else(|_| panic!("failure {index} must still append"));
    }
    drop(store);
    let conn = raw(&db.path);
    let history: String = conn
        .query_row(
            "SELECT status_history FROM candidate_input_operations WHERE execution_id = ?1",
            [execution.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        history.len() <= 8192,
        "history stays within its bound, got {}",
        history.len()
    );
    assert!(
        history.contains("freshness-mismatch-0299"),
        "the tail keeps the latest failure"
    );
}

// Finding 6: an assessment cannot land without its selection row.
#[test]
fn candidate_result_requires_selection_row() {
    let db = TestDb::new("candidate-no-selection");
    let mut store = db.open();
    let (execution, attempt) = seed_runtime(&mut store);
    // No record_candidate_selection: lineage exists, selection does not.
    let subject = EffectSubjectRef::new(format!("attempt:{}", attempt.as_str())).unwrap();
    let candidate_manifest = format!("sha256:{}", "d".repeat(64));
    let mut assessment = CandidateIntegrityAssessment {
        task_id: "T-EFFECT".to_owned(),
        execution_id: execution.as_str().to_owned(),
        attempt_id: attempt.as_str().to_owned(),
        subject_ref: subject.as_str().to_owned(),
        requirement_revision: format!("sha256:{}", "b".repeat(64)),
        policy_revision: 1,
        policy_fingerprint: format!("sha256:{}", "c".repeat(64)),
        criterion_id: "criterion-1".to_owned(),
        selector: "candidate.file-contains.v1".to_owned(),
        selector_path: "src/main.rs".to_owned(),
        selector_literal: "accepted".to_owned(),
        invocation_id: "candidate-invocation-1".to_owned(),
        evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
        runtime_ref: "agentd/1".to_owned(),
        enforcement_ref: "runner-operation-1".to_owned(),
        invocation_descriptor_version: 1,
        invocation_descriptor_sha256: String::new(),
        evaluator_executable: "/trusted/agentd".to_owned(),
        toolchain_ref: "git-executable=/usr/bin/git;git version 2.54.0".to_owned(),
        runner_backend: "native".to_owned(),
        runner_plan_sha256: format!("sha256:{}", "6".repeat(64)),
        runner_enforced_axes: 3,
        runner_env_isolated: true,
        base_commit_oid: "1".repeat(40),
        base_tree_oid: "2".repeat(40),
        base_manifest_sha256: format!("sha256:{}", "a".repeat(64)),
        candidate_commit_oid: "3".repeat(40),
        candidate_tree_oid: "4".repeat(40),
        candidate_manifest_sha256: candidate_manifest.clone(),
        deletions: vec![],
        checkpoints: [
            CandidateCheckpointPhase::Selection,
            CandidateCheckpointPhase::BeforeEvaluation,
            CandidateCheckpointPhase::AfterEvaluation,
            CandidateCheckpointPhase::ResultAdmission,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, phase)| CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms: 11 + index as u64,
            git_metadata_fingerprint: format!("sha256:{}", "e".repeat(64)),
            live_worktree_manifest_sha256: format!("sha256:{}", "f".repeat(64)),
            matched: true,
        })
        .collect(),
        outcome: CandidateIntegrityOutcome::Intact,
        started_at_ms: 10,
        completed_at_ms: 20,
    };
    assessment.refresh_invocation_descriptor_digest();
    let config = candidate_sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            assessment.requirement_revision,
            assessment.policy_fingerprint,
            assessment.selector,
            assessment.selector_path,
            assessment.selector_literal,
            assessment.invocation_descriptor_sha256,
        )
        .as_bytes(),
    );
    let fingerprint = subject_fingerprint(
        EffectKind::TestSuiteResult,
        subject.as_str(),
        &[
            ("content", &candidate_manifest),
            ("toolchain", &assessment.runtime_ref),
            ("config", &config),
        ],
    )
    .unwrap();
    let stored = store.append_candidate_result(
        subject,
        fingerprint,
        EffectPayload::TestSuiteResult {
            passed: 1,
            failed: 0,
            skipped: 0,
            result_sha256: Sha256Digest::new(format!("sha256:{}", "9".repeat(64))).unwrap(),
        },
        "2026-09-23T12:00:00Z",
        EffectProvenance::new(execution.clone(), attempt.clone(), "agentd/1")
            .unwrap()
            .with_lineage(
                Some("runlith.agentd.candidate-evaluator.v1".to_owned()),
                Some("candidate-invocation-1".to_owned()),
            )
            .unwrap(),
        &assessment,
    );
    assert!(
        stored.is_err(),
        "an assessment without its selection row must refuse"
    );
}

// Finding 14: databases shaped at a pre-fix 0022 head heal on open.
#[test]
fn migration_repairs_pretrigger_operations_shape() {
    let db = TestDb::new("candidate-0022-repair");
    {
        let _store = db.open();
    }
    {
        let conn = raw(&db.path);
        conn.execute_batch(
            "DROP TABLE candidate_input_operations;
             DROP TABLE agent_claims;
             DROP TABLE claim_verifications;
             DROP TABLE policy_exemptions;
             DROP TABLE approval_discharges;
             DROP TABLE completion_admissions;
             DROP TABLE review_records;
             DROP TABLE review_path_settings;
             DROP TABLE reviewer_profiles;
             DROP TABLE owner_presence_proofs;
             DROP TABLE binding_proof_consumption;
             DROP TABLE side_effect_escalations;
             DROP TABLE side_effect_transitions;
             DROP TABLE side_effect_intents;
             DROP TABLE context_trace_revs;
             DROP TABLE context_trace_decisions;
             DROP TABLE context_traces;
             DROP TABLE context_cache_boundary_items;
             DROP TABLE context_cache_boundaries;
             DROP TABLE context_plan_hashes;
             DROP TABLE context_item_transforms;
             DROP TABLE context_items;
             DROP TABLE context_plans;
             DROP TABLE continuity_receipt_refs;
             DROP TABLE continuity_budget_events;
             DROP TABLE continuity_migration_links;
             DROP TABLE continuity_session_links;
             DROP TABLE continuity_history_refs;
             DROP TABLE continuity_activations;
             DROP TABLE continuity_checkpoint_notes;
             DROP TABLE continuity_checkpoint_evidence;
             DROP TABLE continuity_checkpoint_sources;
             DROP TABLE continuity_notes;
             DROP TABLE continuity_checkpoints;
             DROP TABLE continuity_epochs;
             DROP TABLE continuity_meta;
             DROP TABLE IF EXISTS entity_links;
             DROP TABLE IF EXISTS entities;
             DROP TABLE IF EXISTS memory_links;
             DROP TABLE IF EXISTS memory_anchors;
             DROP TABLE IF EXISTS memory_source_refs;
             DROP TABLE IF EXISTS routine_runs;
             DROP TABLE IF EXISTS routines;
             DROP TABLE IF EXISTS memories;
             DROP TABLE takeover_admission_anchors;
             DROP TABLE takeover_admissions;
             DROP TRIGGER IF EXISTS candidate_input_assessments_require_selection;
             DROP TRIGGER IF EXISTS event_journal_no_update;
             DROP TRIGGER IF EXISTS event_journal_no_delete;
             DROP TRIGGER IF EXISTS event_plan_causes_no_update;
             DROP TRIGGER IF EXISTS event_plan_causes_no_delete;
             ALTER TABLE event_journal DROP COLUMN previous_hash;
             ALTER TABLE event_journal DROP COLUMN record_hash;
             ALTER TABLE observed_effects DROP COLUMN record_fingerprint;
             PRAGMA user_version = 22;",
        )
        .unwrap();
    }
    let store = db.open();
    assert_eq!(store.schema_version().unwrap(), 35);
    drop(store);
    let conn = raw(&db.path);
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN
             ('candidate_input_operations', 'agent_claims', 'claim_verifications')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 3, "repair recreates the missing tables");
    let triggers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name IN
             ('candidate_input_operations_status_no_regression',
              'candidate_input_operations_invalidation_sticky',
              'candidate_input_operations_no_delete',
              'candidate_input_assessments_require_selection')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(triggers, 4, "repair recreates all operations triggers");
}

// Finding 14, second half: a database with the pre-column assessments
// shape cannot be repaired without fabricating evidence, so it fails
// closed with a recreate directive instead of migrating silently.
#[test]
fn migration_refuses_precolumn_assessments_shape() {
    let db = TestDb::new("candidate-0022-oldshape");
    {
        let _store = db.open();
    }
    {
        let conn = raw(&db.path);
        conn.execute_batch(
            "DROP TABLE agent_claims;
             DROP TABLE claim_verifications;
             DROP TABLE policy_exemptions;
             DROP TABLE approval_discharges;
             DROP TABLE completion_admissions;
             DROP TABLE candidate_input_assessments;
             CREATE TABLE candidate_input_assessments (
                 assessment_id TEXT PRIMARY KEY,
                 effect_id TEXT NOT NULL UNIQUE,
                 task_id TEXT NOT NULL,
                 execution_id TEXT NOT NULL,
                 attempt_id TEXT NOT NULL,
                 subject_ref TEXT NOT NULL,
                 requirement_revision TEXT NOT NULL,
                 policy_revision INTEGER NOT NULL,
                 policy_fingerprint TEXT NOT NULL,
                 criterion_id TEXT NOT NULL,
                 selector TEXT NOT NULL,
                 selector_path TEXT NOT NULL,
                 selector_literal TEXT NOT NULL,
                 invocation_id TEXT NOT NULL,
                 evaluator_ref TEXT NOT NULL,
                 runtime_ref TEXT NOT NULL,
                 enforcement_ref TEXT NOT NULL,
                 base_commit_oid TEXT NOT NULL,
                 base_tree_oid TEXT NOT NULL,
                 base_manifest_sha256 TEXT NOT NULL,
                 candidate_commit_oid TEXT NOT NULL,
                 candidate_tree_oid TEXT NOT NULL,
                 candidate_manifest_sha256 TEXT NOT NULL,
                 checkpoints TEXT NOT NULL,
                 integrity_outcome TEXT NOT NULL,
                 started_at_ms INTEGER NOT NULL,
                 completed_at_ms INTEGER NOT NULL
             );
             PRAGMA user_version = 22;",
        )
        .unwrap();
    }
    let reopened = SqliteStore::open(&db.path);
    match reopened {
        Err(StorageError::Migration(message)) => assert!(
            message.contains("recreate"),
            "the refusal must direct a recreate, got {message:?}"
        ),
        Ok(_) => panic!("pre-column shape must fail closed, but the open succeeded"),
        Err(other) => panic!("pre-column shape must fail with Migration, got {other}"),
    }
}
