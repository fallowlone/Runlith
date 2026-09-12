use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use app_services::{
    ApprovalService, FixedClock, G6Authorization, G6SecurityError, G6SecurityService,
    PrivilegedOperation, ProcessSecuritySpec, ServiceError,
};
use policy::{
    Capability, CompiledPolicy, EffectiveCapabilities, FileScope, HumanDecision, PolicyLayer,
    PolicyOutcome, PolicyRequest, PolicyRule, RequestOrigin, RequestedCapabilities, ScopeId,
};
use runner::IsolationRequirements;
use secrets::{
    SecretBackend, SecretBackendError, SecretBinding, SecretBindingId, SecretDelivery,
    SecretDeliveryError, SecretDeliveryTarget, SecretRef, SecretsBroker,
};
use security_audit::{SecurityAuditEvent, SecurityAuditOutcome, SecurityAuditSubjectKind};
use storage_sqlite::{SqliteStore, StorageError};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDb(PathBuf);

impl TestDb {
    fn new(label: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        Self(std::env::temp_dir().join(format!(
            "runlith-g6-integration-{label}-{}-{id}.sqlite3",
            std::process::id()
        )))
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn request(operation: &str, capabilities: Vec<Capability>) -> PolicyRequest {
    PolicyRequest::new(
        operation,
        "integration-test",
        RequestOrigin::new("TASK-G6", Some("E-G6".into()), Some("A-G6".into())).unwrap(),
        RequestedCapabilities::new(capabilities),
    )
    .unwrap()
}

fn policy(effect: PolicyOutcome, capabilities: &[Capability]) -> CompiledPolicy {
    CompiledPolicy::compile(
        capabilities
            .iter()
            .enumerate()
            .map(|(index, capability)| {
                PolicyRule::new(
                    format!("g6.rule.{index}"),
                    "g6_keeper",
                    PolicyLayer::UserManaged,
                    effect,
                    capability.clone(),
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}

fn launch_caps(workspace: &str) -> Vec<Capability> {
    vec![
        Capability::FilesystemRead(FileScope::new(workspace, ".").unwrap()),
        Capability::FilesystemWrite(FileScope::new(workspace, ".").unwrap()),
        Capability::ProcessExecute(ScopeId::new("claude").unwrap()),
    ]
}

#[test]
fn policy_runner_and_audit_form_one_trusted_launch_boundary() {
    let db = TestDb::new("launch");
    let capabilities = launch_caps("ws-1");
    let request = request("op-launch", capabilities.clone());
    let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();
    let authorization = security
        .authorize(
            &request,
            &EffectiveCapabilities::new(capabilities.clone()),
            &policy(PolicyOutcome::Allow, &capabilities),
            100,
        )
        .unwrap();
    let effective = authorization.effective().unwrap().clone();
    let operation = PrivilegedOperation::from_policy_request(&request);
    let invoked = Arc::new(Mutex::new(false));
    let observed = invoked.clone();

    security
        .enforce_process_operation(
            &operation,
            &runner_native::NativeRunner,
            &effective,
            &ProcessSecuritySpec::new(
                "ws-1",
                ScopeId::new("claude").unwrap(),
                IsolationRequirements::trusted_host(),
                "claude-code",
            ),
            move || {
                *observed.lock().unwrap() = true;
                Ok::<_, ()>(())
            },
        )
        .unwrap()
        .unwrap();
    assert!(*invoked.lock().unwrap());

    let audit = security.list_audit_ordered().unwrap();
    let events: Vec<_> = audit.iter().map(|record| record.fact.event).collect();
    assert!(events.contains(&SecurityAuditEvent::PolicyRequest));
    assert!(events.contains(&SecurityAuditEvent::PolicyDecision));
    assert!(events.contains(&SecurityAuditEvent::EffectiveAuthority));
    assert!(events.contains(&SecurityAuditEvent::IsolationEnforcement));
    assert!(events.contains(&SecurityAuditEvent::ProviderInvocation));
    assert!(audit
        .iter()
        .all(|record| record.fact.correlation_id == "op-launch"));
}

#[test]
fn unsupported_isolation_fails_closed_before_privileged_operation() {
    let db = TestDb::new("isolation");
    let capabilities = launch_caps("ws-1");
    let request = request("op-isolation", capabilities.clone());
    let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();
    let authorization = security
        .authorize(
            &request,
            &EffectiveCapabilities::new(capabilities.clone()),
            &policy(PolicyOutcome::Allow, &capabilities),
            100,
        )
        .unwrap();
    let invoked = Arc::new(Mutex::new(false));
    let observed = invoked.clone();
    let error = security
        .enforce_process_operation(
            &PrivilegedOperation::from_policy_request(&request),
            &runner_native::NativeRunner,
            authorization.effective().unwrap(),
            &ProcessSecuritySpec::new(
                "ws-1",
                ScopeId::new("claude").unwrap(),
                IsolationRequirements::new(true, false, false),
                "claude-code",
            ),
            move || {
                *observed.lock().unwrap() = true;
                Ok::<_, ()>(())
            },
        )
        .unwrap_err();
    assert!(matches!(error, G6SecurityError::Runner(_)));
    assert!(!*invoked.lock().unwrap());
    let audit = security.list_audit_ordered().unwrap();
    assert!(audit.iter().any(|record| {
        record.fact.event == SecurityAuditEvent::IsolationViolation
            && record.fact.outcome == Some(SecurityAuditOutcome::Violated)
    }));
}

#[test]
fn child_and_cross_workspace_authority_expansion_fail_closed() {
    let db = TestDb::new("authority-expansion");
    let parent = EffectiveCapabilities::new(vec![
        Capability::ProcessExecute(ScopeId::new("claude").unwrap()),
        Capability::FilesystemWrite(FileScope::new("ws-a", ".").unwrap()),
    ]);
    let requested = vec![
        Capability::ProcessExecute(ScopeId::new("sh").unwrap()),
        Capability::FilesystemWrite(FileScope::new("ws-b", ".").unwrap()),
    ];
    let request = request("op-expand", requested.clone());
    let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();
    let error = security
        .authorize(
            &request,
            &parent,
            &policy(PolicyOutcome::Allow, &requested),
            100,
        )
        .unwrap_err();
    assert!(matches!(error, G6SecurityError::Policy(_)));
    let audit = security.list_audit_ordered().unwrap();
    assert!(audit.iter().any(|record| {
        record.fact.event == SecurityAuditEvent::DeniedOperation
            && record.fact.outcome == Some(SecurityAuditOutcome::Denied)
    }));
}

#[test]
fn repository_allow_cannot_create_privileged_authority() {
    let db = TestDb::new("repository-non-authority");
    let capability = Capability::ProcessExecute(ScopeId::new("claude").unwrap());
    let request = request("op-repository-allow", vec![capability.clone()]);
    let parent = EffectiveCapabilities::new(vec![capability.clone()]);
    let policy = CompiledPolicy::compile(vec![PolicyRule::new(
        "repo.self-grant",
        "repository_content",
        PolicyLayer::Repository,
        PolicyOutcome::Allow,
        capability,
    )
    .unwrap()])
    .unwrap();
    let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();

    let authorization = security.authorize(&request, &parent, &policy, 100).unwrap();
    assert!(matches!(authorization, G6Authorization::Denied { .. }));
    assert!(authorization.effective().is_none());
}

#[test]
fn runner_revalidates_exact_process_target_adjacent_to_operation() {
    let db = TestDb::new("runner-target-recheck");
    let capabilities = launch_caps("ws-1");
    let request = request("op-target-recheck", capabilities.clone());
    let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();
    let authorization = security
        .authorize(
            &request,
            &EffectiveCapabilities::new(capabilities.clone()),
            &policy(PolicyOutcome::Allow, &capabilities),
            100,
        )
        .unwrap();
    let invoked = Arc::new(Mutex::new(false));
    let observed = invoked.clone();

    let error = security
        .enforce_process_operation(
            &PrivilegedOperation::from_policy_request(&request),
            &runner_native::NativeRunner,
            authorization.effective().unwrap(),
            &ProcessSecuritySpec::new(
                "ws-1",
                ScopeId::new("sh").unwrap(),
                IsolationRequirements::trusted_host(),
                "claude-code",
            ),
            move || {
                *observed.lock().unwrap() = true;
                Ok::<_, ()>(())
            },
        )
        .unwrap_err();
    assert!(matches!(error, G6SecurityError::Runner(_)));
    assert!(!*invoked.lock().unwrap());
}

#[test]
fn approval_survives_restart_consumes_once_and_replay_is_refused() {
    let db = TestDb::new("approval");
    let capability = Capability::git("repo-1").unwrap();
    let request = request("op-approval", vec![capability.clone()]);
    let parent = EffectiveCapabilities::new(vec![capability.clone()]);
    let policy = policy(
        PolicyOutcome::RequireApproval,
        std::slice::from_ref(&capability),
    );

    let request_id = {
        let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();
        match security.authorize(&request, &parent, &policy, 100).unwrap() {
            G6Authorization::ApprovalRequired { request_id, .. } => request_id,
            other => panic!("expected approval requirement, got {other:?}"),
        }
    };
    ApprovalService::open(&db.0, FixedClock(20))
        .unwrap()
        .decide(&request_id, HumanDecision::Approve, "local-user")
        .unwrap();

    let mut security = G6SecurityService::open(&db.0, FixedClock(30)).unwrap();
    let authorized = security.authorize(&request, &parent, &policy, 100).unwrap();
    assert!(authorized.effective().unwrap().permits(&capability));
    drop(security);

    let mut security = G6SecurityService::open(&db.0, FixedClock(40)).unwrap();
    let replay = security
        .authorize(&request, &parent, &policy, 100)
        .unwrap_err();
    assert!(matches!(
        replay,
        G6SecurityError::Service(ServiceError::Storage(StorageError::ConstraintViolation(_)))
    ));
    let audit = security.list_audit_ordered().unwrap();
    assert!(audit.iter().any(|record| {
        record.fact.event == SecurityAuditEvent::ApprovalRequested
            && record.fact.approval_request_id.as_deref() == Some(request_id.as_str())
    }));
    assert!(audit.iter().any(|record| {
        record.fact.event == SecurityAuditEvent::ApprovalDecided
            && record.fact.outcome == Some(SecurityAuditOutcome::Approved)
    }));
    assert!(audit.iter().any(|record| {
        record.fact.event == SecurityAuditEvent::ApprovalDecided
            && record.fact.outcome == Some(SecurityAuditOutcome::Consumed)
    }));
}

#[derive(Clone, Default)]
struct MemoryBackend {
    values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    loads: Arc<Mutex<u64>>,
}

impl SecretBackend for MemoryBackend {
    fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError> {
        self.values
            .lock()
            .unwrap()
            .insert(secret.as_str().to_owned(), value.to_vec());
        Ok(())
    }

    fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
        *self.loads.lock().unwrap() += 1;
        self.values
            .lock()
            .unwrap()
            .get(secret.as_str())
            .cloned()
            .ok_or(SecretBackendError::NotFound)
    }

    fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError> {
        self.values.lock().unwrap().remove(secret.as_str());
        Ok(())
    }
}

#[derive(Default)]
struct CaptureDelivery(Vec<Vec<u8>>);

impl SecretDelivery for CaptureDelivery {
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
fn secret_release_requires_exact_authority_and_audits_only_opaque_reference() {
    let db = TestDb::new("secret");
    let secret = SecretRef::new("github-token-ref").unwrap();
    let target = SecretDeliveryTarget::tool("github-api").unwrap();
    let binding = SecretBinding::new(
        SecretBindingId::new("binding-github").unwrap(),
        secret.clone(),
        target.clone(),
    );
    let backend = MemoryBackend::default();
    let material: Vec<u8> = (0_u8..64)
        .map(|value| value.wrapping_mul(17).wrapping_add(23))
        .collect();
    backend.store(&secret, &material).unwrap();
    let store = SqliteStore::open(&db.0).unwrap();
    let mut broker = SecretsBroker::new(store, backend.clone());
    broker.register_ref(&secret).unwrap();
    broker.bind(&binding).unwrap();
    let operation = PrivilegedOperation {
        operation_id: "op-secret".into(),
        task_id: "TASK-G6".into(),
        execution_id: Some("E-G6".into()),
        attempt_id: Some("A-G6".into()),
    };
    let mut security = G6SecurityService::open(&db.0, FixedClock(10)).unwrap();
    let mut delivery = CaptureDelivery::default();

    let denied = security.release_secret(
        &operation,
        &broker,
        binding.id(),
        &target,
        &EffectiveCapabilities::none(),
        &mut delivery,
    );
    assert!(denied.is_err());
    assert!(delivery.0.is_empty());
    assert_eq!(
        *backend.loads.lock().unwrap(),
        0,
        "unauthorized release must not load material"
    );

    let authority = EffectiveCapabilities::new(vec![Capability::secret(secret.as_str()).unwrap()]);
    security
        .release_secret(
            &operation,
            &broker,
            binding.id(),
            &target,
            &authority,
            &mut delivery,
        )
        .unwrap();
    assert_eq!(delivery.0.as_slice(), std::slice::from_ref(&material));
    assert_eq!(*backend.loads.lock().unwrap(), 1);

    let audit = security.list_audit_ordered().unwrap();
    let secret_records: Vec<_> = audit
        .iter()
        .filter(|record| record.fact.event == SecurityAuditEvent::SecretAccess)
        .collect();
    assert!(secret_records.len() >= 4);
    assert!(secret_records.iter().all(|record| {
        matches!(
            record.fact.subject.as_ref(),
            Some(subject)
                if subject.kind == SecurityAuditSubjectKind::SecretRef
                    && subject.id == secret.as_str()
        )
    }));
    drop(security);
    drop(broker);
    let bytes = fs::read(&db.0).unwrap();
    assert!(
        !bytes
            .windows(material.len())
            .any(|window| window == material),
        "secret material must not enter SQLite or Security Audit"
    );
}
