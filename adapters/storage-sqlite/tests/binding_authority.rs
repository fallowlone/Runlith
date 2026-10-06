//! Binding authority durability (TASK-0160 phase b): binding proof
//! nonces are consumed exactly once per (task, criterion), foreign or
//! cross-protocol proofs refuse, and the task save + consumption land
//! atomically (the loser of a race keeps the first values).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use storage_sqlite::{BindingProofConsumption, SqliteStore};
use task_domain::{EvidenceRequirement, Project, ProjectId, RequirementKind, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const NOW: i64 = 1_700_000_000_000;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-binding-authority-{name}-{}-{id}",
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

fn sha(byte: u8) -> String {
    format!("sha256:{}", (byte as char).to_string().repeat(64))
}

fn seed_task(store: &mut SqliteStore) -> Task {
    let project = Project::new(ProjectId::new("P-BIND").unwrap(), "bind").unwrap();
    store.create_project(&project).unwrap();
    let task = Task::draft(
        TaskId::new("T-BIND").unwrap(),
        project.id().clone(),
        "bind me",
    )
    .unwrap();
    let mut task = task;
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

fn bound_task(task: &Task, literal: &str) -> Task {
    let mut task = task.clone();
    let params = ["literal", "path", "selector"]
        .into_iter()
        .zip([literal, "src/lib.rs", "candidate.file-contains.v1"])
        .map(|(key, value)| task_domain::RequirementParam::new(key, value.to_owned()).unwrap())
        .collect();
    task.define_evidence_requirements(vec![EvidenceRequirement::new(
        "criterion-1",
        RequirementKind::TestSuite,
        RequirementKind::TestSuite.default_subject_binding(),
        params,
        Some(sha(b'p')),
        true,
    )
    .unwrap()])
        .unwrap();
    task
}

fn consumption(nonce: &str, operation: &str) -> BindingProofConsumption {
    BindingProofConsumption {
        nonce: nonce.to_owned(),
        operation: operation.to_owned(),
        task_id: "T-BIND".to_owned(),
        criterion_id: "criterion-1".to_owned(),
        bound_at_ms: NOW,
    }
}

fn prove(store: &mut SqliteStore, nonce: &str, operation: &str) {
    store
        .record_presence_proof(nonce, operation, &sha(b'd'), NOW)
        .unwrap();
}

fn binding_op() -> String {
    format!("candidate-binding:{}", "a".repeat(64))
}

#[test]
fn binding_consumption_round_trip() {
    let db = TestDb::new("round-trip");
    let mut store = db.open();
    let task = seed_task(&mut store);
    let bound = bound_task(&task, "needle");
    prove(&mut store, "nonce-1", &binding_op());
    store
        .save_task_with_binding_proof(&bound, &consumption("nonce-1", &binding_op()))
        .unwrap();
    let restored = store.get_task(task.id()).unwrap().unwrap();
    assert_eq!(restored.evidence_requirements().len(), 1);
    assert_eq!(restored.evidence_requirements()[0].params.len(), 3);
    let recorded = store
        .get_binding_consumption("T-BIND", "criterion-1")
        .unwrap()
        .expect("consumption persists");
    assert_eq!(recorded.nonce, "nonce-1");
}

#[test]
fn binding_nonce_replay_refuses() {
    let db = TestDb::new("replay");
    let mut store = db.open();
    let task = seed_task(&mut store);
    prove(&mut store, "nonce-1", &binding_op());
    store
        .save_task_with_binding_proof(
            &bound_task(&task, "needle"),
            &consumption("nonce-1", &binding_op()),
        )
        .unwrap();
    let error = store
        .save_task_with_binding_proof(
            &bound_task(&task, "needle"),
            &consumption("nonce-1", &binding_op()),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        storage_sqlite::StorageError::ConstraintViolation(_)
    ));
}

#[test]
fn binding_same_criterion_twice_keeps_first_values() {
    let db = TestDb::new("double-bind");
    let mut store = db.open();
    let task = seed_task(&mut store);
    prove(&mut store, "nonce-1", &binding_op());
    prove(&mut store, "nonce-2", &binding_op());
    store
        .save_task_with_binding_proof(
            &bound_task(&task, "first"),
            &consumption("nonce-1", &binding_op()),
        )
        .unwrap();
    let error = store
        .save_task_with_binding_proof(
            &bound_task(&task, "second"),
            &consumption("nonce-2", &binding_op()),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        storage_sqlite::StorageError::ConstraintViolation(_)
    ));
    let restored = store.get_task(task.id()).unwrap().unwrap();
    let literal = restored.evidence_requirements()[0]
        .params
        .iter()
        .find(|param| param.key() == "literal")
        .unwrap();
    assert_eq!(literal.value(), "first");
}

#[test]
fn binding_with_mismatched_proof_operation_refuses_atomically() {
    let db = TestDb::new("op-mismatch");
    let mut store = db.open();
    let task = seed_task(&mut store);
    store
        .record_presence_proof("nonce-1", &binding_op(), &sha(b'd'), NOW)
        .unwrap();
    let bad = consumption("nonce-1", &format!("candidate-binding:{}", "b".repeat(64)));
    let error = store
        .save_task_with_binding_proof(&bound_task(&task, "needle"), &bad)
        .unwrap_err();
    assert!(matches!(
        error,
        storage_sqlite::StorageError::ConstraintViolation(_)
    ));
    let restored = store.get_task(task.id()).unwrap().unwrap();
    assert!(restored.evidence_requirements()[0].params.is_empty());
}

#[test]
fn binding_with_review_proof_refuses() {
    let db = TestDb::new("cross-protocol");
    let mut store = db.open();
    let task = seed_task(&mut store);
    // A B1 review proof is valid in its own protocol but must never
    // authorize a binding (cross-protocol replay).
    let review_op = "b1-review:record=review-1".to_owned();
    store
        .record_presence_proof("nonce-1", &review_op, &sha(b'd'), NOW)
        .unwrap();
    let bad = consumption("nonce-1", &review_op);
    let error = store
        .save_task_with_binding_proof(&bound_task(&task, "needle"), &bad)
        .unwrap_err();
    assert!(matches!(
        error,
        storage_sqlite::StorageError::ConstraintViolation(_)
    ));
    let restored = store.get_task(task.id()).unwrap().unwrap();
    assert!(restored.evidence_requirements()[0].params.is_empty());
}
