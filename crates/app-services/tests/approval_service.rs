//! TASK-0062 trusted approval lifecycle: exact binding, restart durability,
//! expiry, denial and replay refusal.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use app_services::{ApprovalService, FixedClock, ServiceError};
use policy::{
    ApprovalBinding, ApprovalRequirement, Capability, EffectiveCapabilities, FileScope,
    HumanDecision, PolicyContractError, PolicyDecision, PolicyRequest, RequestOrigin,
    RequestedCapabilities,
};
use secrets::{
    SecretBackend, SecretBackendError, SecretBinding, SecretBindingId, SecretBrokerError,
    SecretDelivery, SecretDeliveryError, SecretDeliveryTarget, SecretMetadataStore, SecretRef,
    SecretsBroker,
};
use storage_sqlite::SqliteStore;
use storage_sqlite::{ApprovalState, StorageError};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb(PathBuf);

impl TestDb {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-approval-service-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.join("state.sqlite3"))
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

fn file_write(path: &str) -> Capability {
    Capability::FilesystemWrite(FileScope::new("ws-1", path).unwrap())
}

fn approval_decision(
    request_id: &str,
    path: &str,
    policy_fingerprint: &str,
    expires_at_ms: u64,
) -> PolicyDecision {
    let request = PolicyRequest::new(
        "op-1",
        "agent-runtime",
        RequestOrigin::new("TASK-1", Some("E-1".into()), Some("A-1".into())).unwrap(),
        RequestedCapabilities::new(vec![file_write(path)]),
    )
    .unwrap();
    let requirement = ApprovalRequirement::new(
        request_id,
        ApprovalBinding::new(request.clone(), policy_fingerprint, expires_at_ms).unwrap(),
        EffectiveCapabilities::new(vec![file_write(path)]),
    )
    .unwrap();
    PolicyDecision::require_approval(request, requirement, "approval.write", "human_required")
        .unwrap()
}

fn secret_approval_decision(request_id: &str, expires_at_ms: u64) -> PolicyDecision {
    let capability = Capability::secret("github-token").unwrap();
    let request = PolicyRequest::new(
        "op-secret-1",
        "tool-dispatch",
        RequestOrigin::new("TASK-1", Some("E-1".into()), Some("A-1".into())).unwrap(),
        RequestedCapabilities::new(vec![capability.clone()]),
    )
    .unwrap();
    let requirement = ApprovalRequirement::new(
        request_id,
        ApprovalBinding::new(request.clone(), "policy-v1", expires_at_ms).unwrap(),
        EffectiveCapabilities::new(vec![capability]),
    )
    .unwrap();
    PolicyDecision::require_approval(request, requirement, "approval.secret", "human_required")
        .unwrap()
}

fn test_secret_material() -> Vec<u8> {
    (0_u8..40)
        .map(|value| value.wrapping_mul(31).wrapping_add(5))
        .collect()
}

#[derive(Clone, Default)]
struct MemorySecretBackend(Arc<Mutex<HashMap<String, Vec<u8>>>>);

impl SecretBackend for MemorySecretBackend {
    fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError> {
        self.0
            .lock()
            .unwrap()
            .insert(secret.as_str().to_owned(), value.to_vec());
        Ok(())
    }

    fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
        self.0
            .lock()
            .unwrap()
            .get(secret.as_str())
            .cloned()
            .ok_or(SecretBackendError::NotFound)
    }

    fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError> {
        self.0.lock().unwrap().remove(secret.as_str());
        Ok(())
    }
}

#[derive(Default)]
struct CaptureSecretDelivery(Vec<Vec<u8>>);

impl SecretDelivery for CaptureSecretDelivery {
    fn deliver(
        &mut self,
        _target: &SecretDeliveryTarget,
        value: &[u8],
    ) -> Result<(), SecretDeliveryError> {
        self.0.push(value.to_vec());
        Ok(())
    }
}

#[test]
fn approved_exact_binding_survives_restart_and_is_single_use() {
    let db = TestDb::new("restart");
    let decision = approval_decision("approval-1", "src/api", "policy-v1", 100);
    {
        let mut service = ApprovalService::open(&db.0, FixedClock(10)).unwrap();
        let pending = service.register(&decision).unwrap();
        assert_eq!(pending.state, ApprovalState::Pending);
        assert_eq!(pending.requested[0].scope, "src/api");
    }
    {
        let mut service = ApprovalService::open(&db.0, FixedClock(50)).unwrap();
        assert_eq!(
            service.get("approval-1").unwrap().unwrap().state,
            ApprovalState::Pending
        );
        service
            .decide("approval-1", HumanDecision::Approve, "local-user")
            .unwrap();
    }
    let mut service = ApprovalService::open(&db.0, FixedClock(60)).unwrap();
    let effective = service.authorize(&decision).unwrap();
    assert!(effective.permits(&file_write("src/api/routes")));
    assert!(matches!(
        service.authorize(&decision),
        Err(ServiceError::Storage(StorageError::ConstraintViolation(_)))
    ));
}

#[test]
fn widened_or_rebound_replay_is_refused() {
    let db = TestDb::new("mismatch");
    let exact = approval_decision("approval-1", "src/api", "policy-v1", 100);
    let mut service = ApprovalService::open(&db.0, FixedClock(50)).unwrap();
    service.register(&exact).unwrap();
    service
        .decide("approval-1", HumanDecision::Approve, "local-user")
        .unwrap();

    for changed in [
        approval_decision("approval-1", "src", "policy-v1", 100),
        approval_decision("approval-1", "src/api", "policy-v2", 100),
    ] {
        assert!(matches!(
            service.authorize(&changed),
            Err(ServiceError::Policy(
                PolicyContractError::ApprovalScopeMismatch
            ))
        ));
    }
}

#[test]
fn denial_expiry_and_hard_deny_remain_final() {
    let db = TestDb::new("deny-expire");
    let denied = approval_decision("approval-deny", "src/api", "policy-v1", 100);
    {
        let mut service = ApprovalService::open(&db.0, FixedClock(50)).unwrap();
        service.register(&denied).unwrap();
        service
            .decide("approval-deny", HumanDecision::Deny, "local-user")
            .unwrap();
        assert!(matches!(
            service.authorize(&denied),
            Err(ServiceError::Policy(PolicyContractError::ApprovalDenied))
        ));
    }

    let expired = approval_decision("approval-expired", "src/api", "policy-v1", 100);
    {
        let mut service = ApprovalService::open(&db.0, FixedClock(50)).unwrap();
        service.register(&expired).unwrap();
        service
            .decide("approval-expired", HumanDecision::Approve, "local-user")
            .unwrap();
    }
    let mut service = ApprovalService::open(&db.0, FixedClock(101)).unwrap();
    assert!(matches!(
        service.authorize(&expired),
        Err(ServiceError::Policy(PolicyContractError::ApprovalExpired))
    ));

    let request = PolicyRequest::new(
        "op-hard-deny",
        "agent-runtime",
        RequestOrigin::new("TASK-1", None, None).unwrap(),
        RequestedCapabilities::new(vec![file_write("src")]),
    )
    .unwrap();
    let hard_deny = PolicyDecision::deny(request, "hard.no-write", "hard_deny").unwrap();
    assert!(matches!(
        service.register(&hard_deny),
        Err(ServiceError::Policy(PolicyContractError::HardDenied))
    ));
    assert!(matches!(
        service.authorize(&hard_deny),
        Err(ServiceError::Policy(PolicyContractError::HardDenied))
    ));
}

#[test]
fn approved_secret_row_is_not_bearer_authority_for_broker_release() {
    let db = TestDb::new("secret-boundary");
    let decision = secret_approval_decision("approval-secret", 100);
    let mut approval = ApprovalService::open(&db.0, FixedClock(50)).unwrap();
    approval.register(&decision).unwrap();
    approval
        .decide("approval-secret", HumanDecision::Approve, "local-user")
        .unwrap();

    let secret = SecretRef::new("github-token").unwrap();
    let binding = SecretBinding::new(
        SecretBindingId::new("binding-secret").unwrap(),
        secret.clone(),
        SecretDeliveryTarget::tool("github-api").unwrap(),
    );
    let backend = MemorySecretBackend::default();
    let material = test_secret_material();
    backend.store(&secret, &material).unwrap();
    let mut store = SqliteStore::open(&db.0).unwrap();
    store.ensure_ref(&secret).unwrap();
    store.put_binding(&binding).unwrap();
    let broker = SecretsBroker::new(store, backend.clone());
    let mut delivery = CaptureSecretDelivery::default();

    assert_eq!(
        broker.release(
            binding.id(),
            binding.target(),
            &EffectiveCapabilities::none(),
            &mut delivery,
        ),
        Err(SecretBrokerError::Unauthorized)
    );
    assert!(delivery.0.is_empty());
    drop(broker);

    let effective = approval.authorize(&decision).unwrap();
    let store = SqliteStore::open(&db.0).unwrap();
    let broker = SecretsBroker::new(store, backend);
    broker
        .release(binding.id(), binding.target(), &effective, &mut delivery)
        .unwrap();
    assert_eq!(delivery.0, [material]);
}
