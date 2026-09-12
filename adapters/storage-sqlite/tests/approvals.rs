//! TASK-0062 durable approval lifecycle tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use storage_sqlite::{
    ApprovalCapabilityRecord, ApprovalState, NewApprovalRecord, SqliteStore, StorageError,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb(PathBuf);

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-approval-store-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.join("state.sqlite3"))
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.0).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        if let Some(parent) = self.0.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

fn capability(path: &str) -> ApprovalCapabilityRecord {
    ApprovalCapabilityRecord {
        kind: "filesystem.write".into(),
        workspace: Some("ws-1".into()),
        scope: path.into(),
    }
}

fn pending(id: &str, binding: &str, expiry: u64) -> NewApprovalRecord {
    NewApprovalRecord {
        request_id: id.into(),
        binding_fingerprint: binding.into(),
        operation_id: "op-1".into(),
        requester: "agent-runtime".into(),
        task_id: "TASK-1".into(),
        execution_id: Some("E-1".into()),
        attempt_id: Some("A-1".into()),
        policy_fingerprint: "sha256:policy".into(),
        expires_at_ms: expiry,
        requested: vec![capability("src/api")],
        effective: vec![capability("src/api")],
    }
}

#[test]
fn approval_survives_reopen_and_consumes_once() {
    let db = TestDb::new("durable");
    {
        let mut store = db.open();
        let created = store
            .create_approval(&pending("approval-1", "binding-1", 100))
            .unwrap();
        assert_eq!(created.state, ApprovalState::Pending);
        assert_eq!(created.requested, vec![capability("src/api")]);
    }

    let mut store = db.open();
    let approved = store
        .decide_approval("approval-1", ApprovalState::Approved, "local-user", 50)
        .unwrap();
    assert_eq!(approved.state, ApprovalState::Approved);
    assert_eq!(approved.approver.as_deref(), Some("local-user"));

    drop(store);
    let mut store = db.open();
    let consumed = store
        .consume_approval("approval-1", "binding-1", 60)
        .unwrap();
    assert_eq!(consumed.state, ApprovalState::Consumed);
    assert_eq!(consumed.consumed_at_ms, Some(60));
    assert!(matches!(
        store.consume_approval("approval-1", "binding-1", 61),
        Err(StorageError::ConstraintViolation(_))
    ));
}

#[test]
fn mismatch_expiry_and_second_decision_fail_closed() {
    let db = TestDb::new("refusal");
    let mut store = db.open();
    store
        .create_approval(&pending("approval-1", "binding-1", 100))
        .unwrap();
    store
        .decide_approval("approval-1", ApprovalState::Approved, "local-user", 50)
        .unwrap();
    assert!(matches!(
        store.decide_approval("approval-1", ApprovalState::Denied, "other", 51),
        Err(StorageError::ConstraintViolation(_))
    ));
    assert!(matches!(
        store.consume_approval("approval-1", "binding-other", 60),
        Err(StorageError::ConstraintViolation(_))
    ));
    assert!(matches!(
        store.consume_approval("approval-1", "binding-1", 101),
        Err(StorageError::ConstraintViolation(_))
    ));
}

#[test]
fn denied_or_expired_pending_approval_never_authorizes() {
    let db = TestDb::new("deny-expire");
    let mut store = db.open();
    store
        .create_approval(&pending("denied", "binding-denied", 100))
        .unwrap();
    store
        .decide_approval("denied", ApprovalState::Denied, "local-user", 50)
        .unwrap();
    assert!(matches!(
        store.consume_approval("denied", "binding-denied", 60),
        Err(StorageError::ConstraintViolation(_))
    ));

    store
        .create_approval(&pending("expired", "binding-expired", 100))
        .unwrap();
    assert!(matches!(
        store.decide_approval("expired", ApprovalState::Approved, "local-user", 101),
        Err(StorageError::ConstraintViolation(_))
    ));
}

#[test]
fn same_id_with_different_binding_is_refused() {
    let db = TestDb::new("binding-reuse");
    let mut store = db.open();
    let first = pending("approval-1", "binding-1", 100);
    store.create_approval(&first).unwrap();
    assert_eq!(
        store.create_approval(&first).unwrap().state,
        ApprovalState::Pending
    );
    assert!(matches!(
        store.create_approval(&pending("approval-1", "binding-2", 100)),
        Err(StorageError::Duplicate(_))
    ));
}
