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
        EventType::TaskCreated,
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
    let mut previous_to = Some(TaskState::Draft);
    let mut previous_seq = 0;
    for (index, event) in history.iter().enumerate() {
        // Exactly one event per mutation, in commit order, chained states.
        // Seqs are the store-global commit order (creation events for
        // other scopes interleave globally), so order — not 1-based
        // density — is asserted here.
        assert!(event.seq > previous_seq, "journal order must advance");
        previous_seq = event.seq;
        assert_eq!(event.id.to_string(), format!("evt-{}", event.seq));
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
    assert_eq!(previous_to, Some(TaskState::Completed));
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
            EventType::TaskCreated,
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
            .all(|event| event.fact.to_state != Some(TaskState::Completed)),
        "no event may claim completion outside verification"
    );
}

// -- invalid transitions ----------------------------------------------------------

#[test]
fn invalid_transition_changes_nothing_and_appends_nothing() {
    let db = TestDb::new("invalid");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    let seeded_len = journal_len(&service);
    match service.verification_succeeded(&task_id, "self-report") {
        Err(ServiceError::Domain(_)) => {}
        other => panic!("DRAFT → verification must fail domain validation, got {other:?}"),
    }
    assert_eq!(
        service.store().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Draft
    );
    assert_eq!(journal_len(&service), seeded_len);
}

#[test]
fn mutation_of_missing_task_appends_nothing() {
    let db = TestDb::new("missing");
    let mut service = db.service();
    let seeded_len = journal_len(&service);
    match service.mark_ready(&TaskId::new("GHOST").unwrap()) {
        Err(ServiceError::Storage(_)) => {}
        other => panic!("expected storage NotFound, got {other:?}"),
    }
    assert_eq!(journal_len(&service), seeded_len);
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
    assert_eq!(history.len(), 6);
    assert!(
        history.windows(2).all(|pair| pair[0].seq < pair[1].seq),
        "journal order must survive restart"
    );
    // The reopened service keeps committing with deterministic order.
    let mut service = service;
    service.start(&TaskId::new("T-1").unwrap()).unwrap();
    let history = service.store().list_events_ordered().unwrap();
    assert_eq!(history.len(), 7);
    assert_eq!(history[6].id.to_string(), "evt-7");
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

// -- G2 creation + dependency mutations -------------------------------------------

#[test]
fn project_creation_commits_project_created_event() {
    let db = TestDb::new("projcreate");
    let mut service = db.service();
    let event = service
        .create_project(&Project::new(ProjectId::new("P-1").unwrap(), "Runlith").unwrap())
        .unwrap();
    assert_eq!(event.fact.event_type, EventType::ProjectCreated);
    assert_eq!(event.fact.task_id, None);
    assert_eq!(event.fact.from_state, None);
    assert_eq!(event.fact.to_state, None);
    assert_eq!(event.fact.project_id.as_ref().unwrap().as_str(), "P-1");
    assert_eq!(event.fact.occurred_at, FIXED_STAMP);
    assert!(
        event
            .fact
            .provenance
            .starts_with("task-application-service/"),
        "provenance must name the trusted service"
    );
    assert!(service
        .get_project(&ProjectId::new("P-1").unwrap())
        .unwrap()
        .is_some());
    // Exactly one state row plus exactly one event: no double append.
    assert_eq!(service.list_projects().unwrap().len(), 1);
    assert_eq!(journal_len(&service), 1);
}

#[test]
fn duplicate_project_creation_writes_nothing() {
    let db = TestDb::new("projdup");
    let mut service = db.service();
    service
        .create_project(&Project::new(ProjectId::new("P-1").unwrap(), "One").unwrap())
        .unwrap();
    match service.create_project(&Project::new(ProjectId::new("P-1").unwrap(), "Two").unwrap()) {
        Err(ServiceError::Storage(_)) => {}
        other => panic!("expected storage Duplicate, got {other:?}"),
    }
    assert_eq!(journal_len(&service), 1);
    assert_eq!(
        service
            .get_project(&ProjectId::new("P-1").unwrap())
            .unwrap()
            .unwrap()
            .name(),
        "One"
    );
}

#[test]
fn task_creation_commits_task_created_event() {
    let db = TestDb::new("taskcreate");
    let mut service = db.service();
    service
        .create_project(&Project::new(ProjectId::new("P-1").unwrap(), "P").unwrap())
        .unwrap();
    let task = Task::draft(
        TaskId::new("T-1").unwrap(),
        ProjectId::new("P-1").unwrap(),
        "objective T-1",
    )
    .unwrap();
    let event = service.create_task(&task).unwrap();
    assert_eq!(event.fact.event_type, EventType::TaskCreated);
    assert_eq!(event.fact.task_id.as_ref().unwrap().as_str(), "T-1");
    assert_eq!(event.fact.from_state, Some(TaskState::Draft));
    assert_eq!(event.fact.to_state, Some(TaskState::Draft));
    let stored = service
        .get_task(&TaskId::new("T-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(stored.state(), TaskState::Draft);
    assert_eq!(stored.objective(), "objective T-1");
    assert_eq!(journal_len(&service), 2);
}

#[test]
fn dependency_add_commits_typed_event_and_invalid_edges_write_nothing() {
    let db = TestDb::new("depadd");
    let mut service = db.service();
    let a = seed_task(&mut service, "P-1", "A");
    let b = seed_task(&mut service, "P-1", "B");
    let before = journal_len(&service);
    let event = service.add_dependency(&a, &b).unwrap();
    assert_eq!(event.fact.event_type, EventType::DependencyAdded);
    assert_eq!(event.fact.task_id.as_ref().unwrap(), &a);
    // Edges never mutate lifecycle state: from and to are the unchanged
    // current state.
    assert_eq!(event.fact.from_state, Some(TaskState::Draft));
    assert_eq!(event.fact.to_state, Some(TaskState::Draft));
    assert_eq!(
        service.list_dependencies(&a).unwrap().as_slice(),
        std::slice::from_ref(&b)
    );
    assert_eq!(journal_len(&service), before + 1);
    // Self-edge, duplicate, and unknown target all fail with nothing
    // written — no edge, no event.
    assert!(service.add_dependency(&a, &a).is_err());
    assert!(service.add_dependency(&a, &b).is_err());
    assert!(service
        .add_dependency(&a, &TaskId::new("GHOST").unwrap())
        .is_err());
    assert_eq!(
        service.list_dependencies(&a).unwrap().as_slice(),
        std::slice::from_ref(&b)
    );
    assert_eq!(journal_len(&service), before + 1);
}

#[test]
fn dependency_cycle_is_rejected_without_mutation() {
    let db = TestDb::new("depcycle");
    let mut service = db.service();
    let a = seed_task(&mut service, "P-1", "A");
    let b = seed_task(&mut service, "P-1", "B");
    service.add_dependency(&a, &b).unwrap();
    let before = journal_len(&service);
    match service.add_dependency(&b, &a) {
        Err(ServiceError::Domain(_)) => {}
        other => panic!("expected domain DependencyCycle, got {other:?}"),
    }
    assert!(service.list_dependencies(&b).unwrap().is_empty());
    assert_eq!(journal_len(&service), before);
}

#[test]
fn cross_project_dependency_is_rejected_without_mutation() {
    let db = TestDb::new("depxproj");
    let mut service = db.service();
    let a = seed_task(&mut service, "P-1", "A");
    let f = seed_task(&mut service, "P-2", "F");
    let before = journal_len(&service);
    assert!(service.add_dependency(&a, &f).is_err());
    assert!(service.list_dependencies(&a).unwrap().is_empty());
    assert_eq!(journal_len(&service), before);
}

#[test]
fn read_only_operations_emit_no_events() {
    let db = TestDb::new("readonly");
    let mut service = db.service();
    let task_id = seed_task(&mut service, "P-1", "T-1");
    let project_id = ProjectId::new("P-1").unwrap();
    let before = journal_len(&service);
    assert!(service.get_project(&project_id).unwrap().is_some());
    assert_eq!(service.list_projects().unwrap().len(), 1);
    assert!(service.get_task(&task_id).unwrap().is_some());
    assert_eq!(
        service.list_tasks_for_project(&project_id).unwrap().len(),
        1
    );
    assert!(service.list_dependencies(&task_id).unwrap().is_empty());
    assert_eq!(service.list_events_for_task(&task_id).unwrap().len(), 1);
    assert!(!service
        .list_events_for_project(&project_id)
        .unwrap()
        .is_empty());
    assert_eq!(journal_len(&service), before);
}
