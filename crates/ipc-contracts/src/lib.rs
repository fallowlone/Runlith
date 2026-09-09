//! Versioned IPC contract boundary shared by `agentd` and `agentctl`.
//!
//! Contract boundary only: protocol version primitives plus the generated
//! Protobuf control-plane DTOs live here. No orchestration, storage,
//! provider, UI, transport, framing, or negotiation logic.

/// Generated Protobuf control-plane contract types.
///
/// Derived build output from the canonical wire contract
/// (`proto/agentd_control.proto`), compiled into Cargo `OUT_DIR` by
/// `build.rs`. Data contracts only: no version negotiation, handshake
/// validation, framing, or transport behavior lives here.
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/ai_engineering_os.ipc.rs"));
}

/// G1 bootstrap protocol version: handshake, ping, shutdown only.
/// Frozen history — never removed, never repurposed (ADR-0021).
pub const IPC_PROTOCOL_VERSION_V0: u32 = 0;

/// G2 business protocol version: adds the bounded `task_core` envelope
/// (`TaskCoreRequest` / `TaskCoreResponse`) to the G1 surface (ADR-0021).
pub const IPC_PROTOCOL_VERSION_V1: u32 = 1;

/// Current IPC protocol version: the highest version this build speaks.
pub const IPC_PROTOCOL_VERSION: u32 = IPC_PROTOCOL_VERSION_V1;

/// Alias making "current" explicit at call sites.
pub const IPC_PROTOCOL_VERSION_CURRENT: u32 = IPC_PROTOCOL_VERSION_V1;

/// Every protocol version this build negotiates, oldest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: [u32; 2] =
    [IPC_PROTOCOL_VERSION_V0, IPC_PROTOCOL_VERSION_V1];

/// Maximum length of a client-supplied human/request `reason` (block
/// reason, submission summary, verdict note) in bytes. The trusted
/// service validates non-blank where the operation requires a reason;
/// TASK-0028 enforces this bound server-side before persistence.
pub const MAX_REASON_LEN: usize = 4096;

/// Maximum length of a client-supplied business identifier on the wire.
/// Mirrors the domain identifier discipline (trimmed, non-blank,
/// bounded); the trusted service re-validates every id.
pub const MAX_WIRE_ID_LEN: usize = 256;

/// Returns the current IPC protocol version.
#[must_use]
pub fn protocol_version() -> u32 {
    IPC_PROTOCOL_VERSION
}

/// Reports whether a protocol version is negotiated by this build.
///
/// Membership in [`SUPPORTED_PROTOCOL_VERSIONS`]: both 0 (G1 bootstrap)
/// and 1 (G2 business) stay accepted so old G1 clients keep working.
#[must_use]
pub fn is_compatible(peer_version: u32) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&peer_version)
}

/// Reports whether a single version is in the negotiated set.
#[must_use]
pub fn is_supported_version(version: u32) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}

/// Reports whether a negotiated session version carries business
/// authority: only version 1 enables `TaskCoreRequest`. A version-0
/// session stays bootstrap-only (handshake, ping, shutdown).
#[must_use]
pub fn supports_business_ipc(negotiated_version: u32) -> bool {
    negotiated_version == IPC_PROTOCOL_VERSION_V1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_matches_declared_constant() {
        assert_eq!(protocol_version(), IPC_PROTOCOL_VERSION);
    }

    #[test]
    fn same_version_is_compatible() {
        assert!(is_compatible(IPC_PROTOCOL_VERSION));
    }

    #[test]
    fn different_version_is_incompatible() {
        assert!(!is_compatible(IPC_PROTOCOL_VERSION.wrapping_add(1)));
        assert!(!is_compatible(u32::MAX));
    }

    #[test]
    fn protocol_v0_remains_supported_and_v1_is_current() {
        assert_eq!(IPC_PROTOCOL_VERSION_V0, 0);
        assert_eq!(IPC_PROTOCOL_VERSION_V1, 1);
        assert_eq!(IPC_PROTOCOL_VERSION, 1);
        assert_eq!(IPC_PROTOCOL_VERSION_CURRENT, 1);
        assert_eq!(SUPPORTED_PROTOCOL_VERSIONS, [0, 1]);
        assert!(is_supported_version(0));
        assert!(is_supported_version(1));
        assert!(!is_supported_version(2));
        assert!(is_compatible(0));
        assert!(is_compatible(1));
    }

    #[test]
    fn only_v1_carries_business_authority() {
        assert!(!supports_business_ipc(IPC_PROTOCOL_VERSION_V0));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V1));
        assert!(!supports_business_ipc(99));
    }

    #[test]
    fn reason_and_id_bounds_are_sane() {
        assert_eq!(MAX_REASON_LEN, 4096);
        assert_eq!(MAX_WIRE_ID_LEN, 256);
    }
}

#[cfg(test)]
mod contract_tests {
    use prost::Message;

    use crate::proto::{
        control_request, control_response, task_core_request, task_core_response, BusinessError,
        BusinessErrorCode, ControlRequest, ControlResponse, CreateProjectRequest,
        CreateTaskRequest, HandshakeRequest, LifecycleRequest, PingRequest, ShutdownRequest,
        TaskCoreRequest, TaskCoreResponse, TaskDto, TaskLifecycleOp,
    };

    fn handshake_fixture() -> HandshakeRequest {
        HandshakeRequest {
            supported_protocol_versions: vec![crate::IPC_PROTOCOL_VERSION_V0],
            client_kind: "agentctl".to_owned(),
            client_build_version: "0.1.0".to_owned(),
            requested_capabilities: vec![],
        }
    }

    #[test]
    fn generated_control_types_exist() {
        let handshake = handshake_fixture();
        let ping = PingRequest {};
        let shutdown = ShutdownRequest {};
        let envelope = ControlRequest {
            payload: Some(control_request::Payload::Handshake(handshake)),
        };
        assert!(matches!(
            envelope.payload,
            Some(control_request::Payload::Handshake(_))
        ));
        let _ = (ping, shutdown);
    }

    #[test]
    fn handshake_request_protobuf_round_trip() {
        let original = handshake_fixture();
        let bytes = original.encode_to_vec();
        let decoded = HandshakeRequest::decode(bytes.as_slice()).expect("must decode");
        assert_eq!(decoded.supported_protocol_versions, vec![0]);
        assert_eq!(decoded.client_kind, "agentctl");
        assert_eq!(decoded.client_build_version, "0.1.0");
        assert!(decoded.requested_capabilities.is_empty());
    }

    #[test]
    fn control_request_handshake_variant_round_trip() {
        let original = ControlRequest {
            payload: Some(control_request::Payload::Handshake(handshake_fixture())),
        };
        let bytes = original.encode_to_vec();
        let decoded = ControlRequest::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(control_request::Payload::Handshake(handshake)) => {
                assert_eq!(handshake.client_kind, "agentctl");
                assert_eq!(handshake.supported_protocol_versions, vec![0]);
            }
            other => panic!("expected handshake variant, got {other:?}"),
        }
    }

    #[test]
    fn g1_envelope_field_numbers_are_frozen() {
        // Oneof tag byte = (field_number << 3) | wire_type(2 = LEN).
        // G1 history: handshake = 1 (0x0A), ping = 2 (0x12),
        // shutdown = 3 (0x1A). These bytes must never change.
        let handshake = ControlRequest {
            payload: Some(control_request::Payload::Handshake(handshake_fixture())),
        };
        assert_eq!(handshake.encode_to_vec()[0], 0x0A);
        let ping = ControlRequest {
            payload: Some(control_request::Payload::Ping(PingRequest {})),
        };
        assert_eq!(ping.encode_to_vec()[0], 0x12);
        let shutdown = ControlRequest {
            payload: Some(control_request::Payload::Shutdown(ShutdownRequest {})),
        };
        assert_eq!(shutdown.encode_to_vec()[0], 0x1A);
        let handshake_reply = ControlResponse {
            payload: Some(control_response::Payload::Ping(
                crate::proto::PingResponse {},
            )),
        };
        assert_eq!(handshake_reply.encode_to_vec()[0], 0x12);
    }

    #[test]
    fn task_core_envelope_uses_field_four_and_round_trips() {
        let business = ControlRequest {
            payload: Some(control_request::Payload::TaskCore(TaskCoreRequest {
                payload: Some(task_core_request::Payload::CreateTask(CreateTaskRequest {
                    id: "T-1".to_owned(),
                    project_id: "P-1".to_owned(),
                    objective: "ship G2".to_owned(),
                })),
            })),
        };
        let bytes = business.encode_to_vec();
        // Field 4, wire type LEN: (4 << 3) | 2 = 0x22.
        assert_eq!(bytes[0], 0x22);
        let decoded = ControlRequest::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(control_request::Payload::TaskCore(request)) => match request.payload {
                Some(task_core_request::Payload::CreateTask(create)) => {
                    assert_eq!(create.id, "T-1");
                    assert_eq!(create.project_id, "P-1");
                    assert_eq!(create.objective, "ship G2");
                }
                other => panic!("expected create_task, got {other:?}"),
            },
            other => panic!("expected task_core envelope, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_request_round_trip_preserves_semantic_op() {
        // No generic set-state exists: the wire carries only the semantic
        // op enum, and submit_result stays distinct from verification.
        let submit = LifecycleRequest {
            task_id: "T-1".to_owned(),
            op: TaskLifecycleOp::SubmitResult as i32,
            reason: "diff + tests".to_owned(),
        };
        let bytes = submit.encode_to_vec();
        let decoded = LifecycleRequest::decode(bytes.as_slice()).expect("must decode");
        assert_eq!(decoded.op, TaskLifecycleOp::SubmitResult as i32);
        assert_ne!(
            decoded.op,
            TaskLifecycleOp::VerificationSucceeded as i32,
            "submit_result must never alias verification_succeeded"
        );
        let verify = LifecycleRequest {
            task_id: "T-1".to_owned(),
            op: TaskLifecycleOp::VerificationSucceeded as i32,
            reason: "VERIFIED: refs".to_owned(),
        };
        let bytes = verify.encode_to_vec();
        let decoded = LifecycleRequest::decode(bytes.as_slice()).expect("must decode");
        assert_eq!(decoded.op, TaskLifecycleOp::VerificationSucceeded as i32);
    }

    #[test]
    fn lifecycle_op_enum_covers_exactly_the_trusted_service_ops() {
        // 1:1 with TaskService: mark_ready, claim, start (incl. resume and
        // retry resume), block, submit_result, route_to_review,
        // begin_verification, verification_failed, verification_succeeded.
        // No set-state, no complete-task, no cancel/fail (unresolved).
        let ops = [
            TaskLifecycleOp::MarkReady,
            TaskLifecycleOp::Claim,
            TaskLifecycleOp::Start,
            TaskLifecycleOp::Block,
            TaskLifecycleOp::SubmitResult,
            TaskLifecycleOp::RouteToReview,
            TaskLifecycleOp::BeginVerification,
            TaskLifecycleOp::VerificationFailed,
            TaskLifecycleOp::VerificationSucceeded,
        ];
        assert_eq!(ops.len(), 9);
        for op in ops {
            let request = TaskCoreRequest {
                payload: Some(task_core_request::Payload::Lifecycle(LifecycleRequest {
                    task_id: "T-1".to_owned(),
                    op: op as i32,
                    reason: "note".to_owned(),
                })),
            };
            let bytes = request.encode_to_vec();
            let decoded = TaskCoreRequest::decode(bytes.as_slice()).expect("must decode");
            match decoded.payload {
                Some(task_core_request::Payload::Lifecycle(lifecycle)) => {
                    assert_eq!(lifecycle.op, op as i32);
                }
                other => panic!("expected lifecycle, got {other:?}"),
            }
        }
    }

    #[test]
    fn task_dto_round_trip_carries_only_g2_fields() {
        let dto = TaskDto {
            id: "T-1".to_owned(),
            project_id: "P-1".to_owned(),
            objective: "ship G2".to_owned(),
            state: "DRAFT".to_owned(),
        };
        let response = TaskCoreResponse {
            payload: Some(task_core_response::Payload::GetTask(
                crate::proto::GetTaskResponse { task: Some(dto) },
            )),
        };
        let bytes = response.encode_to_vec();
        let decoded = TaskCoreResponse::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(task_core_response::Payload::GetTask(get)) => {
                let task = get.task.expect("task dto");
                assert_eq!(task.id, "T-1");
                assert_eq!(task.project_id, "P-1");
                assert_eq!(task.objective, "ship G2");
                assert_eq!(task.state, "DRAFT");
            }
            other => panic!("expected get_task, got {other:?}"),
        }
    }

    #[test]
    fn business_error_round_trip_is_typed() {
        let error = BusinessError {
            code: BusinessErrorCode::InvalidTransition as i32,
            message: "RUNNING cannot accept VerificationSucceeded".to_owned(),
        };
        let response = TaskCoreResponse {
            payload: Some(task_core_response::Payload::Error(error)),
        };
        let bytes = response.encode_to_vec();
        let decoded = TaskCoreResponse::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::InvalidTransition as i32);
                assert!(!error.message.is_empty());
            }
            other => panic!("expected error, got {other:?}"),
        }
        // Every required business code exists on the wire.
        for code in [
            BusinessErrorCode::InvalidArgument,
            BusinessErrorCode::NotFound,
            BusinessErrorCode::AlreadyExists,
            BusinessErrorCode::InvalidTransition,
            BusinessErrorCode::Conflict,
            BusinessErrorCode::StaleState,
            BusinessErrorCode::CorruptState,
            BusinessErrorCode::UnsupportedProtocol,
            BusinessErrorCode::Internal,
        ] {
            assert_ne!(code as i32, 0, "{code:?} must not be the unspecified value");
        }
    }

    #[test]
    fn create_project_request_round_trip() {
        let request = TaskCoreRequest {
            payload: Some(task_core_request::Payload::CreateProject(
                CreateProjectRequest {
                    id: "P-1".to_owned(),
                    name: "Runlith".to_owned(),
                },
            )),
        };
        let bytes = request.encode_to_vec();
        let decoded = TaskCoreRequest::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(task_core_request::Payload::CreateProject(create)) => {
                assert_eq!(create.id, "P-1");
                assert_eq!(create.name, "Runlith");
            }
            other => panic!("expected create_project, got {other:?}"),
        }
    }
}
