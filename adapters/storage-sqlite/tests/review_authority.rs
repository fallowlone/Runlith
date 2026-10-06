//! Review authority durability tests (TASK-0145 phase b2): presence
//! proofs bind exact operations, setting changes consume proofs,
//! review records round-trip with verified fingerprints, and history
//! is append-only with sticky invalidation and linear supersession.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use evaluation::{ReviewPathName, ReviewRecord, ReviewRecordFields, ReviewVerdictName};
use policy::{ReviewPath, ReviewPathSetting, ReviewerProfile};
use storage_sqlite::{EventType, NewJournalEvent, SqliteStore};
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const STAMP: &str = "2026-09-23T12:00:00Z";

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-review-authority-{name}-{}-{id}",
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

    fn raw(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.path).unwrap()
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

fn sha(byte: u8) -> String {
    format!("sha256:{}", (byte as char).to_string().repeat(64))
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
        correlation_id: "review-authority-test".to_owned(),
        causation_id: "review-authority-test".to_owned(),
        occurred_at: STAMP.to_owned(),
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

fn seed_task(store: &mut SqliteStore) -> Task {
    let project = Project::new(ProjectId::new("P-REVIEW").unwrap(), "reviews").unwrap();
    store.create_project(&project).unwrap();
    let task = Task::draft(
        TaskId::new("T-REVIEW").unwrap(),
        project.id().clone(),
        "review me",
    )
    .unwrap();
    store.create_task(&task).unwrap();
    store.get_task(task.id()).unwrap().unwrap()
}

fn seed_attempt(store: &mut SqliteStore, task: &Task) -> (ExecutionId, AttemptId) {
    let execution_id = ExecutionId::new("E-REVIEW").unwrap();
    let mut execution = Execution::begin(execution_id.clone(), task.id().clone());
    store
        .create_execution_with_event(
            &execution,
            &event(
                EventType::ExecutionCreated,
                task,
                Some(execution_id.clone()),
                None,
            ),
        )
        .unwrap();
    let attempt_id = AttemptId::new("A-REVIEW").unwrap();
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
                task,
                Some(execution_id.clone()),
                Some(attempt_id.clone()),
            ),
        )
        .unwrap();
    (execution_id, attempt_id)
}

fn seed_setting_and_profile(store: &mut SqliteStore, task: &Task) {
    let profile = ReviewerProfile::new("owner-primary", 1, "key:platform-1").unwrap();
    store.record_reviewer_profile(&profile).unwrap();
    store
        .record_presence_proof(
            "nonce-setting-1",
            "review-setting-change:rev=1:path=review-path/v1:b1",
            &sha(b'e'),
            1_700_000_000_000,
        )
        .unwrap();
    store
        .record_review_path_setting(
            task.id().as_str(),
            &ReviewPathSetting::new(1, ReviewPath::B1).unwrap(),
            "nonce-setting-1",
            1_700_000_000_000,
        )
        .unwrap();
}

fn sample_record(record_id: &str, supersedes: Option<String>) -> ReviewRecord {
    ReviewRecord::new(ReviewRecordFields {
        record_id: record_id.to_owned(),
        task_id: "T-REVIEW".to_owned(),
        requirement_revision: "rev-1".to_owned(),
        base_manifest: sha(b'a'),
        candidate_manifest: sha(b'b'),
        changed_inputs: vec!["tests/unit.rs".to_owned()],
        affected_checks: vec!["cargo-test-unit".to_owned()],
        policy_revision: 7,
        policy_fingerprint: sha(b'c'),
        profile_id: "owner-primary".to_owned(),
        profile_revision: 1,
        setting_revision: 1,
        setting_path: ReviewPathName::B1,
        reviewer_nonce: format!("nonce-{record_id}"),
        reviewer_input_digest: sha(b'd'),
        reviewer_verdict: ReviewVerdictName::Approved,
        implementer_execution_id: "E-REVIEW".to_owned(),
        implementer_attempt_id: "A-REVIEW".to_owned(),
        implementer_admission_ref: "runner-admission-1".to_owned(),
        rationale: "covers the changed check".to_owned(),
        evidence_refs: vec!["effect-1".to_owned()],
        captured_at_ms: 1_700_000_000_000,
        supersedes,
    })
    .unwrap()
}

#[test]
fn setting_change_consumes_exact_proof() {
    let db = TestDb::new("setting-ok");
    let mut store = db.open();
    let task = seed_task(&mut store);
    store
        .record_presence_proof(
            "nonce-setting-1",
            "review-setting-change:rev=1:path=review-path/v1:b1",
            &sha(b'e'),
            1_700_000_000_000,
        )
        .unwrap();
    let setting = ReviewPathSetting::new(1, ReviewPath::B1).unwrap();
    store
        .record_review_path_setting(
            task.id().as_str(),
            &setting,
            "nonce-setting-1",
            1_700_000_000_000,
        )
        .unwrap();
    let restored = store
        .get_review_path_setting(task.id().as_str())
        .unwrap()
        .expect("setting persists");
    assert_eq!(restored.fingerprint(), setting.fingerprint());
}

#[test]
fn setting_change_with_wrong_operation_proof_refuses() {
    let db = TestDb::new("setting-op");
    let mut store = db.open();
    let task = seed_task(&mut store);
    store
        .record_presence_proof(
            "nonce-setting-2",
            "review-setting-change:rev=2:path=review-path/v1:b1",
            &sha(b'e'),
            1_700_000_000_000,
        )
        .unwrap();
    let setting = ReviewPathSetting::new(1, ReviewPath::B1).unwrap();
    assert!(store
        .record_review_path_setting(
            task.id().as_str(),
            &setting,
            "nonce-setting-2",
            1_700_000_000_000,
        )
        .is_err());
    assert!(store
        .get_review_path_setting(task.id().as_str())
        .unwrap()
        .is_none());
}

#[test]
fn nonce_replay_refuses_across_tables() {
    let db = TestDb::new("nonce-replay");
    let mut store = db.open();
    let task = seed_task(&mut store);
    store
        .record_presence_proof(
            "nonce-once",
            "review-setting-change:rev=1:path=review-path/v1:b1",
            &sha(b'e'),
            1_700_000_000_000,
        )
        .unwrap();
    let first = ReviewPathSetting::new(1, ReviewPath::B1).unwrap();
    store
        .record_review_path_setting(task.id().as_str(), &first, "nonce-once", 1_700_000_000_000)
        .unwrap();
    let second = ReviewPathSetting::new(2, ReviewPath::B2).unwrap();
    assert!(store
        .record_review_path_setting(task.id().as_str(), &second, "nonce-once", 1_700_000_100_000,)
        .is_err());
    let current = store
        .get_review_path_setting(task.id().as_str())
        .unwrap()
        .expect("first setting stands");
    assert_eq!(current.revision(), 1);
}

#[test]
fn review_record_round_trip_restores_with_verified_fingerprint() {
    let db = TestDb::new("record-ok");
    let mut store = db.open();
    let task = seed_task(&mut store);
    let (_execution, _attempt) = seed_attempt(&mut store, &task);
    seed_setting_and_profile(&mut store, &task);
    let record = sample_record("review-1", None);
    store
        .record_presence_proof(
            record.reviewer_nonce(),
            "b1-review:record=review-1",
            record.reviewer_input_digest(),
            1_700_000_000_000,
        )
        .unwrap();
    store.record_review(&record).unwrap();
    let restored = store
        .get_review_record("review-1")
        .unwrap()
        .expect("record persists");
    assert_eq!(restored.fingerprint(), record.fingerprint());
    assert_eq!(restored.changed_inputs(), record.changed_inputs());
    let listed = store
        .list_review_records_for_task(task.id().as_str())
        .unwrap();
    assert_eq!(listed.len(), 1);
}

#[test]
fn review_state_survives_reopen() {
    let db = TestDb::new("record-reopen");
    let fingerprint = {
        let mut store = db.open();
        let task = seed_task(&mut store);
        let (_execution, _attempt) = seed_attempt(&mut store, &task);
        seed_setting_and_profile(&mut store, &task);
        let record = sample_record("review-r", None);
        store
            .record_presence_proof(
                record.reviewer_nonce(),
                "b1-review:record=review-r",
                record.reviewer_input_digest(),
                1_700_000_000_000,
            )
            .unwrap();
        store.record_review(&record).unwrap();
        store
            .invalidate_review_record("review-r", "superseded-policy", 1_700_000_100_000)
            .unwrap();
        record.fingerprint().to_owned()
    };
    let store = db.open();
    let restored = store
        .get_review_record("review-r")
        .unwrap()
        .expect("record survives reopen");
    assert_eq!(restored.fingerprint(), fingerprint);
    let state = store
        .review_record_state("review-r")
        .unwrap()
        .expect("state survives reopen");
    assert_eq!(
        state.invalidation_code.as_deref(),
        Some("superseded-policy")
    );
    let head = store.live_review_record_head("T-REVIEW").unwrap();
    assert!(head.is_none(), "invalidated record is not the live head");
}

#[test]
fn review_with_mismatched_proof_operation_refuses() {
    let db = TestDb::new("record-op");
    let mut store = db.open();
    let task = seed_task(&mut store);
    let (_execution, _attempt) = seed_attempt(&mut store, &task);
    seed_setting_and_profile(&mut store, &task);
    let record = sample_record("review-2", None);
    store
        .record_presence_proof(
            record.reviewer_nonce(),
            "b1-review:record=review-OTHER",
            record.reviewer_input_digest(),
            1_700_000_000_000,
        )
        .unwrap();
    assert!(store.record_review(&record).is_err());
    assert!(store.get_review_record("review-2").unwrap().is_none());
}

#[test]
fn supersession_chains_and_double_supersede_refuses() {
    let db = TestDb::new("supersede");
    let mut store = db.open();
    let task = seed_task(&mut store);
    let (_execution, _attempt) = seed_attempt(&mut store, &task);
    seed_setting_and_profile(&mut store, &task);
    for id in ["review-a", "review-b", "review-c"] {
        let record = sample_record(
            id,
            if id == "review-a" {
                None
            } else {
                Some("review-a".to_owned())
            },
        );
        store
            .record_presence_proof(
                record.reviewer_nonce(),
                &format!("b1-review:record={id}"),
                record.reviewer_input_digest(),
                1_700_000_000_000,
            )
            .unwrap();
        if id == "review-c" {
            assert!(store.record_review(&record).is_err());
        } else {
            store.record_review(&record).unwrap();
        }
    }
    let state = store
        .review_record_state("review-a")
        .unwrap()
        .expect("parent state");
    assert_eq!(state.superseded_by.as_deref(), Some("review-b"));
}

#[test]
fn invalidation_is_sticky_and_deletes_refuse() {
    let db = TestDb::new("invalidate");
    let mut store = db.open();
    let task = seed_task(&mut store);
    let (_execution, _attempt) = seed_attempt(&mut store, &task);
    seed_setting_and_profile(&mut store, &task);
    let record = sample_record("review-x", None);
    store
        .record_presence_proof(
            record.reviewer_nonce(),
            "b1-review:record=review-x",
            record.reviewer_input_digest(),
            1_700_000_000_000,
        )
        .unwrap();
    store.record_review(&record).unwrap();
    store
        .invalidate_review_record("review-x", "superseded-policy", 1_700_000_100_000)
        .unwrap();
    let state = store
        .review_record_state("review-x")
        .unwrap()
        .expect("state");
    assert_eq!(
        state.invalidation_code.as_deref(),
        Some("superseded-policy")
    );
    assert!(store
        .invalidate_review_record("review-x", "other", 1_700_000_200_000)
        .is_err());
    let raw = db.raw();
    assert!(raw
        .execute(
            "DELETE FROM review_records WHERE record_id = 'review-x'",
            []
        )
        .is_err());
    assert!(raw
        .execute(
            "UPDATE review_records SET reviewer_verdict = 'rejected' WHERE record_id = 'review-x'",
            []
        )
        .is_err());
}
