//! TASK-0062 approval control plane over real local IPC.
//!
//! Proves v3 observation/decision plus durable restart-independent
//! authorization. The wire never registers or consumes authority.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use app_services::{ApprovalService, FixedClock, ServiceError};
use ipc_contracts::proto::{
    policy_core_request, policy_core_response, ApprovalDecision, DecideApprovalRequest,
    GetApprovalRequest, PolicyCoreRequest,
};
use ipc_transport::platform::unix::{UnixEndpoint, SOCKET_FILE_NAME};
use policy::{
    ApprovalBinding, ApprovalRequirement, Capability, EffectiveCapabilities, FileScope,
    PolicyDecision, PolicyRequest, RequestOrigin, RequestedCapabilities,
};
use storage_sqlite::StorageError;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Sandbox {
    root: PathBuf,
    endpoint: UnixEndpoint,
    db: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            std::env::temp_dir().join(format!("runlith-approval-ipc-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        Self {
            endpoint: UnixEndpoint {
                socket_path: root.join("run").join(SOCKET_FILE_NAME),
                runtime_dir: root.join("run"),
            },
            db: root.join("db").join("state.sqlite3"),
            root,
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn inert_spawn() -> std::io::Result<std::process::Child> {
    Command::new("true").spawn()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn decision(expires_at_ms: u64) -> PolicyDecision {
    let capability = Capability::FilesystemWrite(FileScope::new("ws-1", "src/api").unwrap());
    let request = PolicyRequest::new(
        "op-approval-ipc",
        "agent-runtime",
        RequestOrigin::new("TASK-1", Some("E-1".into()), Some("A-1".into())).unwrap(),
        RequestedCapabilities::new(vec![capability.clone()]),
    )
    .unwrap();
    let requirement = ApprovalRequirement::new(
        "approval-ipc-1",
        ApprovalBinding::new(request.clone(), "policy-v1", expires_at_ms).unwrap(),
        EffectiveCapabilities::new(vec![capability]),
    )
    .unwrap();
    PolicyDecision::require_approval(request, requirement, "approval.write", "human_required")
        .unwrap()
}

async fn wait_ready(endpoint: &UnixEndpoint) {
    for _ in 0..100 {
        if agentctl::connect_and_handshake_v3(endpoint).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not expose protocol v3");
}

#[tokio::test(flavor = "current_thread")]
async fn human_decision_over_v3_survives_daemon_and_consumes_once() {
    let sandbox = Sandbox::new();
    let endpoint = sandbox.endpoint.clone();
    let db = sandbox.db.clone();
    let daemon = tokio::spawn(async move {
        agentd::run_on_endpoint(&endpoint, agentd::DaemonConfig::with_execution(db)).await
    });
    wait_ready(&sandbox.endpoint).await;

    let base = now_ms();
    let decision = decision(base + 60_000);
    {
        let mut service = ApprovalService::open(&sandbox.db, FixedClock(base)).unwrap();
        service.register(&decision).unwrap();
    }

    let get = agentctl::policy_request(
        &sandbox.endpoint,
        inert_spawn,
        PolicyCoreRequest {
            payload: Some(policy_core_request::Payload::GetApproval(
                GetApprovalRequest {
                    request_id: "approval-ipc-1".into(),
                },
            )),
        },
    )
    .await
    .unwrap();
    match get.payload {
        Some(policy_core_response::Payload::GetApproval(response)) => {
            let approval = response.approval.unwrap();
            assert_eq!(approval.state, "pending");
            assert_eq!(approval.requested[0].scope, "src/api");
        }
        other => panic!("expected pending approval, got {other:?}"),
    }

    let decided = agentctl::policy_request(
        &sandbox.endpoint,
        inert_spawn,
        PolicyCoreRequest {
            payload: Some(policy_core_request::Payload::DecideApproval(
                DecideApprovalRequest {
                    request_id: "approval-ipc-1".into(),
                    decision: ApprovalDecision::Approve as i32,
                },
            )),
        },
    )
    .await
    .unwrap();
    match decided.payload {
        Some(policy_core_response::Payload::DecideApproval(response)) => {
            let approval = response.approval.unwrap();
            assert_eq!(approval.state, "approved");
            assert_eq!(
                approval.approver.as_deref(),
                Some("authenticated-local-control-plane")
            );
        }
        other => panic!("expected approved response, got {other:?}"),
    }

    agentctl::shutdown(&sandbox.endpoint).await.unwrap();
    daemon.await.unwrap().unwrap();

    let mut service = ApprovalService::open(&sandbox.db, FixedClock(base + 1_000)).unwrap();
    let effective = service.authorize(&decision).unwrap();
    assert_eq!(effective.as_slice().len(), 1);
    assert!(matches!(
        service.authorize(&decision),
        Err(ServiceError::Storage(StorageError::ConstraintViolation(_)))
    ));
}
