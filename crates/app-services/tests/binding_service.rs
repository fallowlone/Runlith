//! Operator binding control-surface path (TASK-0160 phase c): the
//! service mints the presence proof for the exact binding operation,
//! binds DRAFT criteria only (Option 2a timing — never a later
//! update), and persists the task image with the proof consumption
//! atomically. Denied or stale presence refuses before anything is
//! stored.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{
    BindingService, BindingServiceError, DenyPresence, FixedClock, OperatorBindingInput,
    PresenceError, TestPresenceAuthority,
};
use policy::{CandidateSelectorCatalog, CandidateSelectorCatalogRevision};
use storage_sqlite::SqliteStore;
use task_domain::{EvidenceRequirement, Project, ProjectId, RequirementKind, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const NOW: u64 = 1_700_000_000_000;
const POLICY_FINGERPRINT: &str =
    "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

fn open_store(name: &str) -> (SqliteStore, PathBuf) {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "runlith-binding-service-{name}-{}-{id}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("db.sqlite");
    (SqliteStore::open(&path).unwrap(), path)
}

fn catalog() -> CandidateSelectorCatalogRevision {
    CandidateSelectorCatalogRevision::new(1, CandidateSelectorCatalog::v1()).unwrap()
}

fn seed_task(store: &mut SqliteStore) -> Task {
    let project = Project::new(ProjectId::new("P-OP").unwrap(), "op").unwrap();
    store.create_project(&project).unwrap();
    let mut task = Task::draft(
        TaskId::new("T-OP").unwrap(),
        project.id().clone(),
        "bind me",
    )
    .unwrap();
    task.add_acceptance("criterion-1").unwrap();
    task.define_evidence_requirements(vec![EvidenceRequirement::new(
        "criterion-1",
        RequirementKind::TestSuite,
        RequirementKind::TestSuite.default_subject_binding(),
        Vec::new(),
        Some("policy:test-seed".to_owned()),
        true,
    )
    .unwrap()])
        .unwrap();
    store.create_task(&task).unwrap();
    store.get_task(task.id()).unwrap().unwrap()
}

fn input() -> OperatorBindingInput {
    OperatorBindingInput {
        task_id: "T-OP".to_owned(),
        criterion_id: "criterion-1".to_owned(),
        selector: "candidate.file-contains.v1".to_owned(),
        path: "src/lib.rs".to_owned(),
        literal: "needle".to_owned(),
        policy_fingerprint: POLICY_FINGERPRINT.to_owned(),
    }
}

#[test]
fn operator_binding_binds_draft_criterion() {
    let (mut store, path) = open_store("happy");
    seed_task(&mut store);
    let mut service = BindingService::attach(store, FixedClock(NOW), catalog());
    let presence = TestPresenceAuthority::new(NOW);
    let bound = service.bind_for_task(&presence, input()).unwrap();
    assert_eq!(bound.requirement.params.len(), 3);
    assert_eq!(bound.provenance.binder(), "operator");
    // Durable state through a second handle: bound params plus the
    // recorded consumption.
    let reader = SqliteStore::open(&path).unwrap();
    let restored = reader
        .get_task(&TaskId::new("T-OP").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(restored.evidence_requirements()[0].params.len(), 3);
    let consumption = reader
        .get_binding_consumption("T-OP", "criterion-1")
        .unwrap()
        .expect("consumption persists");
    assert!(consumption.operation.starts_with("candidate-binding:"));
}

#[test]
fn binding_without_presence_refuses() {
    let (mut store, path) = open_store("deny");
    seed_task(&mut store);
    let mut service = BindingService::attach(store, FixedClock(NOW), catalog());
    let error = service.bind_for_task(&DenyPresence, input()).unwrap_err();
    assert!(matches!(
        error,
        BindingServiceError::Presence(PresenceError::ControlHostUnavailable)
    ));
    let reader = SqliteStore::open(&path).unwrap();
    let restored = reader
        .get_task(&TaskId::new("T-OP").unwrap())
        .unwrap()
        .unwrap();
    assert!(restored.evidence_requirements()[0].params.is_empty());
    assert!(reader
        .get_binding_consumption("T-OP", "criterion-1")
        .unwrap()
        .is_none());
}

#[test]
fn binding_stale_presence_refuses() {
    let (mut store, _path) = open_store("stale");
    seed_task(&mut store);
    let mut service = BindingService::attach(store, FixedClock(NOW + 600_000), catalog());
    let presence = TestPresenceAuthority::new(NOW);
    let error = service.bind_for_task(&presence, input()).unwrap_err();
    assert!(matches!(error, BindingServiceError::StalePresence { .. }));
}

#[test]
fn binding_non_draft_task_refuses() {
    let (mut store, _path) = open_store("timing");
    let mut task = seed_task(&mut store);
    task.mark_ready().unwrap();
    store.save_task(&task).unwrap();
    let mut service = BindingService::attach(store, FixedClock(NOW), catalog());
    let presence = TestPresenceAuthority::new(NOW);
    let error = service.bind_for_task(&presence, input()).unwrap_err();
    assert!(matches!(error, BindingServiceError::TaskNotDraft { .. }));
}

#[test]
fn binding_same_criterion_twice_refuses() {
    let (mut store, _path) = open_store("double");
    seed_task(&mut store);
    let mut service = BindingService::attach(store, FixedClock(NOW), catalog());
    let presence = TestPresenceAuthority::new(NOW);
    service.bind_for_task(&presence, input()).unwrap();
    let error = service.bind_for_task(&presence, input()).unwrap_err();
    assert!(matches!(
        error,
        BindingServiceError::Binding(
            app_services::RequirementBindingError::RequirementAlreadyBound
        )
    ));
}

#[test]
fn binding_unknown_task_or_criterion_refuses() {
    let (mut store, _path) = open_store("missing");
    seed_task(&mut store);
    let mut service = BindingService::attach(store, FixedClock(NOW), catalog());
    let presence = TestPresenceAuthority::new(NOW);
    let mut bad_task = input();
    bad_task.task_id = "T-NOPE".to_owned();
    assert!(matches!(
        service.bind_for_task(&presence, bad_task).unwrap_err(),
        BindingServiceError::TaskNotFound(_)
    ));
    let mut bad_criterion = input();
    bad_criterion.criterion_id = "criterion-NOPE".to_owned();
    assert!(matches!(
        service.bind_for_task(&presence, bad_criterion).unwrap_err(),
        BindingServiceError::CriterionNotFound(_)
    ));
}
