//! G6 approval control-plane dispatcher. The wire may observe and resolve
//! approvals; only `ApprovalService` can register or consume authority.

use app_services::{ApprovalService, Clock, ServiceError};
use ipc_contracts::proto::{
    policy_core_request, policy_core_response, ApprovalCapabilityDto, ApprovalDecision,
    ApprovalDto, BusinessError, BusinessErrorCode, DecideApprovalResponse, GetApprovalResponse,
    PolicyCoreRequest, PolicyCoreResponse,
};
use policy::HumanDecision;
use storage_sqlite::ApprovalRecord;

use crate::dispatch::map_error;

const LOCAL_APPROVER_PROVENANCE: &str = "authenticated-local-control-plane";

pub fn dispatch<C: Clock>(
    service: &mut ApprovalService<C>,
    request: PolicyCoreRequest,
) -> PolicyCoreResponse {
    match request.payload {
        Some(policy_core_request::Payload::GetApproval(request)) => {
            if !valid_id(&request.request_id) {
                return invalid_argument("approval request id is invalid");
            }
            match service.get(&request.request_id) {
                Ok(Some(record)) => success(policy_core_response::Payload::GetApproval(
                    GetApprovalResponse {
                        approval: Some(dto(record)),
                    },
                )),
                Ok(None) => not_found(&request.request_id),
                Err(error) => error_response(error),
            }
        }
        Some(policy_core_request::Payload::DecideApproval(request)) => {
            if !valid_id(&request.request_id) {
                return invalid_argument("approval request id is invalid");
            }
            let decision = match ApprovalDecision::try_from(request.decision) {
                Ok(ApprovalDecision::Approve) => HumanDecision::Approve,
                Ok(ApprovalDecision::Deny) => HumanDecision::Deny,
                _ => return invalid_argument("approval decision must be approve or deny"),
            };
            match service.decide(&request.request_id, decision, LOCAL_APPROVER_PROVENANCE) {
                Ok(record) => success(policy_core_response::Payload::DecideApproval(
                    DecideApprovalResponse {
                        approval: Some(dto(record)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        None => invalid_argument("empty policy request"),
    }
}

pub fn unavailable() -> PolicyCoreResponse {
    PolicyCoreResponse {
        payload: Some(policy_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::Internal as i32,
            message: "approval service unavailable".to_owned(),
        })),
    }
}

fn dto(record: ApprovalRecord) -> ApprovalDto {
    ApprovalDto {
        request_id: record.request_id,
        operation_id: record.operation_id,
        requester: record.requester,
        task_id: record.task_id,
        execution_id: record.execution_id,
        attempt_id: record.attempt_id,
        policy_fingerprint: record.policy_fingerprint,
        expires_at_ms: record.expires_at_ms,
        state: record.state.as_str().to_owned(),
        approver: record.approver,
        decided_at_ms: record.decided_at_ms,
        consumed_at_ms: record.consumed_at_ms,
        requested: record.requested.into_iter().map(capability_dto).collect(),
        effective: record.effective.into_iter().map(capability_dto).collect(),
    }
}

fn capability_dto(record: storage_sqlite::ApprovalCapabilityRecord) -> ApprovalCapabilityDto {
    ApprovalCapabilityDto {
        kind: record.kind,
        workspace: record.workspace,
        scope: record.scope,
    }
}

fn success(payload: policy_core_response::Payload) -> PolicyCoreResponse {
    PolicyCoreResponse {
        payload: Some(payload),
    }
}

fn error_response(error: ServiceError) -> PolicyCoreResponse {
    let (code, message) = map_error(&error);
    PolicyCoreResponse {
        payload: Some(policy_core_response::Payload::Error(BusinessError {
            code: code as i32,
            message,
        })),
    }
}

fn invalid_argument(message: &str) -> PolicyCoreResponse {
    PolicyCoreResponse {
        payload: Some(policy_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::InvalidArgument as i32,
            message: message.to_owned(),
        })),
    }
}

fn not_found(request_id: &str) -> PolicyCoreResponse {
    PolicyCoreResponse {
        payload: Some(policy_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::NotFound as i32,
            message: format!("approval {request_id} does not exist"),
        })),
    }
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= ipc_contracts::MAX_WIRE_ID_LEN
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use app_services::FixedClock;
    use ipc_contracts::proto::{
        policy_core_request, policy_core_response, ApprovalDecision, DecideApprovalRequest,
        GetApprovalRequest,
    };
    use policy::{
        ApprovalBinding, ApprovalRequirement, Capability, EffectiveCapabilities, FileScope,
        PolicyDecision, PolicyRequest, RequestOrigin, RequestedCapabilities,
    };

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDb(PathBuf);

    impl TestDb {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "runlith-agentd-approval-{}-{id}",
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

    fn decision() -> PolicyDecision {
        let capability = Capability::FilesystemWrite(FileScope::new("ws-1", "src/api").unwrap());
        let request = PolicyRequest::new(
            "op-1",
            "agent-runtime",
            RequestOrigin::new("TASK-1", Some("E-1".into()), Some("A-1".into())).unwrap(),
            RequestedCapabilities::new(vec![capability.clone()]),
        )
        .unwrap();
        let approval = ApprovalRequirement::new(
            "approval-1",
            ApprovalBinding::new(request.clone(), "policy-v1", 100).unwrap(),
            EffectiveCapabilities::new(vec![capability]),
        )
        .unwrap();
        PolicyDecision::require_approval(request, approval, "approval.write", "human_required")
            .unwrap()
    }

    #[test]
    fn wire_can_observe_and_decide_but_not_mint_approval() {
        let db = TestDb::new();
        let mut service = ApprovalService::open(&db.0, FixedClock(10)).unwrap();
        service.register(&decision()).unwrap();
        let get = dispatch(
            &mut service,
            PolicyCoreRequest {
                payload: Some(policy_core_request::Payload::GetApproval(
                    GetApprovalRequest {
                        request_id: "approval-1".to_owned(),
                    },
                )),
            },
        );
        match get.payload {
            Some(policy_core_response::Payload::GetApproval(response)) => {
                let approval = response.approval.expect("approval dto");
                assert_eq!(approval.state, "pending");
                assert_eq!(approval.requested[0].scope, "src/api");
            }
            other => panic!("expected get approval, got {other:?}"),
        }

        let decided = dispatch(
            &mut service,
            PolicyCoreRequest {
                payload: Some(policy_core_request::Payload::DecideApproval(
                    DecideApprovalRequest {
                        request_id: "approval-1".to_owned(),
                        decision: ApprovalDecision::Approve as i32,
                    },
                )),
            },
        );
        match decided.payload {
            Some(policy_core_response::Payload::DecideApproval(response)) => {
                let approval = response.approval.expect("approval dto");
                assert_eq!(approval.state, "approved");
                assert_eq!(
                    approval.approver.as_deref(),
                    Some(LOCAL_APPROVER_PROVENANCE)
                );
            }
            other => panic!("expected decide approval, got {other:?}"),
        }
    }
}
