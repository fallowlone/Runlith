//! Review authority service wiring (TASK-0145 phase c2): presence
//! gates every setting change and B1 review, nonces chain through the
//! store, supersession auto-chains to the live head, and admission
//! decides from durable state.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use app_services::{
    B1ReviewInput, DenyPresence, FixedClock, PresenceError, ReviewAdmissionDecision,
    ReviewAdmissionQuery, ReviewService, ReviewServiceError, TestPresenceAuthority,
};
use storage_sqlite::{EventType, NewJournalEvent, SqliteStore};
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const NOW: u64 = 1_700_000_000_000;
const MAX_AGE: u64 = 86_400_000;

fn sha(byte: u8) -> String {
    format!("sha256:{}", (byte as char).to_string().repeat(64))
}

fn open_store(name: &str) -> (SqliteStore, PathBuf) {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "runlith-review-service-{name}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("db.sqlite");
    (SqliteStore::open(&path).unwrap(), path)
}

fn seed_task(store: &mut SqliteStore) -> Task {
    let project = Project::new(ProjectId::new("P-SVC").unwrap(), "svc").unwrap();
    store.create_project(&project).unwrap();
    let task = Task::draft(
        TaskId::new("T-SVC").unwrap(),
        project.id().clone(),
        "review me",
    )
    .unwrap();
    store.create_task(&task).unwrap();
    let execution_id = ExecutionId::new("E-SVC").unwrap();
    let mut execution = Execution::begin(execution_id.clone(), task.id().clone());
    let stamp = "2026-09-23T12:00:00Z".to_owned();
    let event = |kind: EventType, attempt: Option<AttemptId>| NewJournalEvent {
        event_type: kind,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "review-service-test".to_owned(),
        causation_id: "review-service-test".to_owned(),
        occurred_at: stamp.clone(),
        provenance: "app-services/test".to_owned(),
        execution_id: Some(execution_id.clone()),
        attempt_id: attempt,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    };
    store
        .create_execution_with_event(&execution, &event(EventType::ExecutionCreated, None))
        .unwrap();
    let attempt_id = AttemptId::new("A-SVC").unwrap();
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
            &event(EventType::AttemptCreated, Some(attempt_id)),
        )
        .unwrap();
    store.get_task(task.id()).unwrap().unwrap()
}

fn scoped_policy() -> policy::TestMutationPolicyRevision {
    let policy = policy::TestMutationPolicy::scoped_reviewed(
        vec!["tests/unit.rs".to_owned()],
        vec!["cargo-test-unit".to_owned()],
    )
    .unwrap();
    policy::TestMutationPolicyRevision::new(7, policy).unwrap()
}

fn review_input(record_id: &str, supersedes: Option<String>) -> B1ReviewInput {
    B1ReviewInput {
        record_id: record_id.to_owned(),
        task_id: "T-SVC".to_owned(),
        requirement_revision: "rev-1".to_owned(),
        base_manifest: sha(b'a'),
        candidate_manifest: sha(b'b'),
        changed_inputs: vec!["tests/unit.rs".to_owned()],
        affected_checks: vec!["cargo-test-unit".to_owned()],
        policy_revision: 7,
        policy_fingerprint: scoped_policy().fingerprint().to_owned(),
        profile_id: "owner-primary".to_owned(),
        profile_revision: 1,
        reviewer_input_digest: sha(b'd'),
        reviewer_verdict: evaluation::ReviewVerdictName::Approved,
        implementer_execution_id: "E-SVC".to_owned(),
        implementer_attempt_id: "A-SVC".to_owned(),
        implementer_admission_ref: "runner-admission-1".to_owned(),
        rationale: "covers the changed check".to_owned(),
        evidence_refs: vec!["effect-1".to_owned()],
        supersedes,
    }
}

fn query() -> ReviewAdmissionQuery {
    ReviewAdmissionQuery {
        task_id: "T-SVC".to_owned(),
        policy: scoped_policy(),
        profile: policy::ReviewerProfile::new("owner-primary", 1, "key:platform-1").unwrap(),
        path: "tests/unit.rs".to_owned(),
        check: "cargo-test-unit".to_owned(),
        requirement_revision: "rev-1".to_owned(),
        base_manifest: sha(b'a'),
        candidate_manifest: sha(b'b'),
        changed_inputs: vec!["tests/unit.rs".to_owned()],
        affected_checks: vec!["cargo-test-unit".to_owned()],
        max_age_ms: MAX_AGE,
    }
}

#[test]
fn setting_change_mints_proof_and_appends_revision() {
    let (mut store, _path) = open_store("setting-ok");
    seed_task(&mut store);
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    let presence = TestPresenceAuthority::new(NOW);
    let first = service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B1)
        .unwrap();
    assert_eq!(first.revision(), 1);
    let second = service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B2)
        .unwrap();
    assert_eq!(second.revision(), 2);
    assert_eq!(*second.path(), policy::ReviewPath::B2);
}

#[test]
fn setting_change_without_presence_refuses() {
    let (store, _path) = open_store("setting-deny");
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    let error = service
        .change_review_path(&DenyPresence, "T-SVC", policy::ReviewPath::B1)
        .unwrap_err();
    assert!(matches!(
        error,
        ReviewServiceError::Presence(PresenceError::ControlHostUnavailable)
    ));
}

#[test]
fn stale_presence_proof_refuses() {
    let (store, _path) = open_store("setting-stale");
    let mut service = ReviewService::attach(store, FixedClock(NOW + 600_000));
    let presence = TestPresenceAuthority::new(NOW);
    let error = service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B1)
        .unwrap_err();
    assert!(matches!(error, ReviewServiceError::StalePresence { .. }));
}

#[test]
fn b1_review_chains_to_live_head() {
    let (mut store, _path) = open_store("review-chain");
    seed_task(&mut store);
    store
        .record_reviewer_profile(
            &policy::ReviewerProfile::new("owner-primary", 1, "key:platform-1").unwrap(),
        )
        .unwrap();
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    let presence = TestPresenceAuthority::new(NOW);
    service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B1)
        .unwrap();
    let first = service
        .record_b1_review(&presence, review_input("review-1", None))
        .unwrap();
    assert_eq!(first.supersedes(), None);
    let second = service
        .record_b1_review(&presence, review_input("review-2", None))
        .unwrap();
    assert_eq!(second.supersedes(), Some("review-1"));
}

#[test]
fn b1_review_with_dead_parent_refuses() {
    let (mut store, _path) = open_store("review-dead");
    seed_task(&mut store);
    store
        .record_reviewer_profile(
            &policy::ReviewerProfile::new("owner-primary", 1, "key:platform-1").unwrap(),
        )
        .unwrap();
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    let presence = TestPresenceAuthority::new(NOW);
    service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B1)
        .unwrap();
    service
        .record_b1_review(&presence, review_input("review-1", None))
        .unwrap();
    service
        .invalidate_review("review-1", "superseded-policy")
        .unwrap();
    let error = service
        .record_b1_review(
            &presence,
            review_input("review-2", Some("review-1".to_owned())),
        )
        .unwrap_err();
    assert!(matches!(error, ReviewServiceError::ParentNotLive(_)));
}

#[test]
fn b1_review_without_setting_refuses() {
    let (mut store, _path) = open_store("review-nosetting");
    seed_task(&mut store);
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    let presence = TestPresenceAuthority::new(NOW);
    let error = service
        .record_b1_review(&presence, review_input("review-1", None))
        .unwrap_err();
    assert!(matches!(error, ReviewServiceError::NoSettingForTask(_)));
}

#[test]
fn b1_review_under_b2_setting_refuses() {
    let (mut store, _path) = open_store("review-b2");
    seed_task(&mut store);
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    let presence = TestPresenceAuthority::new(NOW);
    service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B2)
        .unwrap();
    let error = service
        .record_b1_review(&presence, review_input("review-1", None))
        .unwrap_err();
    assert!(matches!(error, ReviewServiceError::SettingPathNotB1 { .. }));
}

#[test]
fn admission_flows_through_service() {
    let (mut store, _path) = open_store("admission-flow");
    seed_task(&mut store);
    store
        .record_reviewer_profile(
            &policy::ReviewerProfile::new("owner-primary", 1, "key:platform-1").unwrap(),
        )
        .unwrap();
    let mut service = ReviewService::attach(store, FixedClock(NOW));
    assert!(matches!(
        service.admission_for_task(query()).unwrap(),
        ReviewAdmissionDecision::Deny { .. }
    ));
    let presence = TestPresenceAuthority::new(NOW);
    service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B1)
        .unwrap();
    assert_eq!(
        service.admission_for_task(query()).unwrap(),
        ReviewAdmissionDecision::BlockedOnReview
    );
    service
        .record_b1_review(&presence, review_input("review-1", None))
        .unwrap();
    assert_eq!(
        service.admission_for_task(query()).unwrap(),
        ReviewAdmissionDecision::Admit
    );
    service
        .change_review_path(&presence, "T-SVC", policy::ReviewPath::B1)
        .unwrap();
    assert!(matches!(
        service.admission_for_task(query()).unwrap(),
        ReviewAdmissionDecision::Deny { .. }
    ));
}
