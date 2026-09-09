//! Trusted-service contract tests: atomic lifecycle mutations, audit
//! trail, completion authority, restart durability, schema compatibility.
//!
//! Deterministic clock throughout (`FixedClock`), so timestamps,
//! correlation, and causation ids assert exactly. Every database file is
//! test-owned.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{FixedClock, ServiceError, TaskService};
use storage_sqlite::EventType;
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 2024-01-01T00:00:00Z in millis: every `occurred_at` below must render
/// exactly this stamp.
const FIXED_MILLIS: u64 = 1_704_067_200_000;
const FIXED_STAMP: &str = "2024-01-01T00:00:00Z";

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-service-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
    }

    fn service(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open service")
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

fn seed_task(service: &mut TaskService<FixedClock>, project: &str, id: &str) -> TaskId {
    if service
        .store()
        .get_project(&ProjectId::new(project).unwrap())
        .unwrap()
        .is_none()
    {
        service
            .create_project(
                &Project::new(
                    ProjectId::new(project).unwrap(),
                    format!("project {project}"),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let task = Task::draft(
        TaskId::new(id).unwrap(),
        ProjectId::new(project).unwrap(),
        format!("objective {id}"),
    )
    .unwrap();
    service.create_task(&task).unwrap();
    task.id().clone()
}

fn journal_len(service: &TaskService<FixedClock>) -> usize {
    service.store().list_events_ordered().unwrap().len()
}

// -- audit trail ---------------------------------------------------------------

#[test]
fn full_lifecycle_commits_matching_ordered_history() {
    let db = TestDb::new("lifecycle");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    let expected_types = [
        EventType::MarkedReady,
        EventType::Claimed,
        EventType::Started,
        EventType::Submitted,
        EventType::RoutedToReview,
        EventType::VerificationBegan,
        EventType::VerificationSucceeded,
    ];
    service.mark_ready(&task_id).unwrap();
    service.claim(&task_id).unwrap();
    service.start(&task_id).unwrap();
    let submitted = service.submit_result(&task_id, "diff + tests").unwrap();
    assert_eq!(submitted.task.state(), TaskState::Submitted);
    assert_eq!(submitted.event.fact.event_type, EventType::Submitted);
    assert_eq!(submitted.event.fact.reason.as_deref(), Some("diff + tests"));
    service.route_to_review(&task_id).unwrap();
    service.begin_verification(&task_id).unwrap();
    let completed = service
        .verification_succeeded(&task_id, "VERIFIED: refs")
        .unwrap();
    assert_eq!(completed.task.state(), TaskState::Completed);

    let history = service.store().list_events_for_task(&task_id).unwrap();
    assert_eq!(history.len(), expected_types.len());
    let mut previous_to = TaskState::Draft;
    for (index, event) in history.iter().enumerate() {
        // Exactly one event per mutation, in commit order, chained states.
        assert_eq!(event.seq, (index + 1) as i64);
        assert_eq!(event.id.to_string(), format!("evt-{}", index + 1));
        assert_eq!(event.fact.event_type, expected_types[index]);
        assert_eq!(event.fact.from_state, previous_to);
        assert_eq!(event.event_version, 1);
        assert_eq!(event.fact.occurred_at, FIXED_STAMP);
        assert!(!event.fact.correlation_id.is_empty());
        assert!(!event.fact.causation_id.is_empty());
        assert_ne!(event.fact.correlation_id, event.fact.causation_id);
        assert!(
            event
                .fact
                .provenance
                .starts_with("task-application-service/"),
            "provenance must name the trusted service, got {}",
            event.fact.provenance
        );
        previous_to = event.fact.to_state;
    }
    assert_eq!(previous_to, TaskState::Completed);
    // Current state comes from the relational row, matching the trail tip.
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Completed
    );
}

#[test]
fn block_resume_and_retry_paths_commit_events() {
    let db = TestDb::new("paths");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    service.mark_ready(&task_id).unwrap();
    service.claim(&task_id).unwrap();
    service.start(&task_id).unwrap();
    let blocked = service.block(&task_id, "waiting on input").unwrap();
    assert_eq!(blocked.task.state(), TaskState::Blocked);
    assert_eq!(
        blocked.event.fact.reason.as_deref(),
        Some("waiting on input")
    );
    service.start(&task_id).unwrap();
    service.submit_result(&task_id, "attempt").unwrap();
    service.route_to_review(&task_id).unwrap();
    service.begin_verification(&task_id).unwrap();
    let retry = service
        .verification_failed(&task_id, "REJECTED: gap")
        .unwrap();
    assert_eq!(retry.task.state(), TaskState::RetryReady);
    service.start(&task_id).unwrap();
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );
    let types: Vec<EventType> = service
        .store()
        .list_events_for_task(&task_id)
        .unwrap()
        .iter()
        .map(|event| event.fact.event_type)
        .collect();
    assert_eq!(
        types,
        [
            EventType::MarkedReady,
            EventType::Claimed,
            EventType::Started,
            EventType::Blocked,
            EventType::Started,
            EventType::Submitted,
            EventType::RoutedToReview,
            EventType::VerificationBegan,
            EventType::VerificationFailed,
            EventType::Started,
        ]
    );
}

#[test]
fn submit_result_does_not_complete() {
    let db = TestDb::new("submit");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    service.mark_ready(&task_id).unwrap();
    service.claim(&task_id).unwrap();
    service.start(&task_id).unwrap();
    let applied = service.submit_result(&task_id, "done?").unwrap();
    assert_eq!(applied.task.state(), TaskState::Submitted);
    let history = service.store().list_events_for_task(&task_id).unwrap();
    assert!(
        history
            .iter()
            .all(|event| event.fact.to_state != TaskState::Completed),
        "no event may claim completion outside verification"
    );
}

// -- invalid transitions ----------------------------------------------------------

#[test]
fn invalid_transition_changes_nothing_and_appends_nothing() {
    let db = TestDb::new("invalid");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    match service.verification_succeeded(&task_id, "self-report") {
        Err(ServiceError::Domain(_)) => {}
        other => panic!("DRAFT → verification must fail domain validation, got {other:?}"),
    }
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Draft
    );
    assert_eq!(journal_len(&service), 0);
}

#[test]
fn mutation_of_missing_task_appends_nothing() {
    let db = TestDb::new("missing");
    let mut service = db.service();
    match service.mark_ready(&TaskId::new("GHOST").unwrap()) {
        Err(ServiceError::Storage(_)) => {}
        other => panic!("expected storage NotFound, got {other:?}"),
    }
    assert_eq!(journal_len(&service), 0);
}

// -- restart durability ---------------------------------------------------------------

#[test]
fn restart_preserves_state_history_and_order() {
    let db = TestDb::new("restart");
    {
        let mut service = db.service();
        seed_task(&mut service, "P-1", "T-1");
        seed_task(&mut service, "P-1", "T-2");
        service.mark_ready(&TaskId::new("T-1").unwrap()).unwrap();
        service.claim(&TaskId::new("T-1").unwrap()).unwrap();
        service.mark_ready(&TaskId::new("T-2").unwrap()).unwrap();
    }
    let service = db.service();
    assert_eq!(
        service
            .store()
            .get_task(&TaskId::new("T-1").unwrap())
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Claimed
    );
    let history = service.store().list_events_ordered().unwrap();
    assert_eq!(history.len(), 3);
    assert!(
        history.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "journal order must survive restart"
    );
    // The reopened service keeps committing with deterministic order.
    let mut service = service;
    service.start(&TaskId::new("T-1").unwrap()).unwrap();
    let history = service.store().list_events_ordered().unwrap();
    assert_eq!(history.len(), 4);
    assert_eq!(history[3].id.to_string(), "evt-4");
}

// -- schema compatibility ---------------------------------------------------------------

#[test]
fn persisted_envelope_matches_event_schema() {
    // Compile-time fixture check: the schema file is authority for field
    // names; production code never parses it at runtime, no codegen.
    const SCHEMA: &str = include_str!("../../../contracts/json-schema/event.schema.json");
    for required in [
        "\"event_type\"",
        "\"schema_version\"",
        "\"occurred_at\"",
        "\"correlation_id\"",
        "\"causation_id\"",
    ] {
        assert!(SCHEMA.contains(required), "schema must require {required}");
    }
    let db = TestDb::new("schema");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    service.mark_ready(&task_id).unwrap();
    let event = &service.store().list_events_ordered().unwrap()[0];
    assert!(!event.fact.correlation_id.is_empty());
    assert!(!event.fact.causation_id.is_empty());
    assert_eq!(event.fact.occurred_at, FIXED_STAMP);
    assert_eq!(event.event_version, 1);
}
