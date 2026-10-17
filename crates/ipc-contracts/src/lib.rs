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

/// G5 execution protocol version: adds the bounded `execution_core`
/// envelope (`ExecutionCoreRequest` / `ExecutionCoreResponse`) to the
/// G2 surface (TASK-0047). A v2 session carries both business and
/// execution authority.
pub const IPC_PROTOCOL_VERSION_V2: u32 = 2;

/// G6 approval protocol version: adds the bounded `policy_core` envelope.
pub const IPC_PROTOCOL_VERSION_V3: u32 = 3;

/// G7 Human Decision Queue protocol version: adds the bounded
/// `human_decision_core` envelope while preserving all older authority.
pub const IPC_PROTOCOL_VERSION_V4: u32 = 4;

/// G7 planning operations: trusted planning trigger and continuation apply.
pub const IPC_PROTOCOL_VERSION_V5: u32 = 5;

/// G7 Task authoring (TASK-0084): creates a Task together with its canonical
/// planning metadata through the bounded `task_authoring_core` envelope.
pub const IPC_PROTOCOL_VERSION_V6: u32 = 6;

/// ADR-0039 operator credential vault: adds the bounded `vault_core`
/// envelope (unlock, lock, store, status, rotate, password and recovery).
/// G9 repository finder (TASK-0147): bounded search pages and span
/// reopens through the `finder_core` envelope.
pub const IPC_PROTOCOL_VERSION_V7: u32 = 7;

/// G9 TAKEOVER rollout (TASK-0177): preserves every older surface and
/// enables the TAKEOVER human continuation plus its apply/append/render/
/// reconcile operations in the `human_decision_core` envelope
/// (ADR-0027 TASK-0169 amendment, wire assignment v8).
pub const IPC_PROTOCOL_VERSION_V8: u32 = 8;

/// G10 structured memory (TASK-0184): adds the bounded `memory_core`
/// envelope (capture, get, validate, supersede, retire, list eligible).
pub const IPC_PROTOCOL_VERSION_V9: u32 = 9;

/// Current IPC protocol version: the highest version this build speaks.
pub const IPC_PROTOCOL_VERSION: u32 = IPC_PROTOCOL_VERSION_V9;

/// Alias making "current" explicit at call sites.
pub const IPC_PROTOCOL_VERSION_CURRENT: u32 = IPC_PROTOCOL_VERSION_V9;

/// Every protocol version this build negotiates, oldest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: [u32; 10] = [
    IPC_PROTOCOL_VERSION_V0,
    IPC_PROTOCOL_VERSION_V1,
    IPC_PROTOCOL_VERSION_V2,
    IPC_PROTOCOL_VERSION_V3,
    IPC_PROTOCOL_VERSION_V4,
    IPC_PROTOCOL_VERSION_V5,
    IPC_PROTOCOL_VERSION_V6,
    IPC_PROTOCOL_VERSION_V7,
    IPC_PROTOCOL_VERSION_V8,
    IPC_PROTOCOL_VERSION_V9,
];

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
/// authority. Version 1 introduced `TaskCoreRequest`; every later version
/// preserves that surface. Version 0 stays bootstrap-only.
#[must_use]
pub fn supports_business_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V1
            | IPC_PROTOCOL_VERSION_V2
            | IPC_PROTOCOL_VERSION_V3
            | IPC_PROTOCOL_VERSION_V4
            | IPC_PROTOCOL_VERSION_V5
            | IPC_PROTOCOL_VERSION_V6
            | IPC_PROTOCOL_VERSION_V7
            | IPC_PROTOCOL_VERSION_V8
            | IPC_PROTOCOL_VERSION_V9
    )
}

/// Reports whether a negotiated session version carries execution (G5
/// `execution_core`) authority. Version 2 introduced it and every later
/// version preserves it. A v0/v1 session rejects execution requests.
#[must_use]
pub fn supports_execution_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V2
            | IPC_PROTOCOL_VERSION_V3
            | IPC_PROTOCOL_VERSION_V4
            | IPC_PROTOCOL_VERSION_V5
            | IPC_PROTOCOL_VERSION_V6
            | IPC_PROTOCOL_VERSION_V7
            | IPC_PROTOCOL_VERSION_V8
            | IPC_PROTOCOL_VERSION_V9
    )
}

/// Reports whether a negotiated session carries G6 approval control-plane
/// authority. Version 3 introduced it and every later version preserves it.
#[must_use]
pub fn supports_policy_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V3
            | IPC_PROTOCOL_VERSION_V4
            | IPC_PROTOCOL_VERSION_V5
            | IPC_PROTOCOL_VERSION_V6
            | IPC_PROTOCOL_VERSION_V7
            | IPC_PROTOCOL_VERSION_V8
            | IPC_PROTOCOL_VERSION_V9
    )
}

/// Reports whether a negotiated session carries G7 Human Decision Queue
/// observation/resolution authority. Version 4 introduced it and every later
/// version preserves it.
#[must_use]
pub fn supports_human_decision_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V4
            | IPC_PROTOCOL_VERSION_V5
            | IPC_PROTOCOL_VERSION_V6
            | IPC_PROTOCOL_VERSION_V7
            | IPC_PROTOCOL_VERSION_V8
            | IPC_PROTOCOL_VERSION_V9
    )
}

/// Planning operations are new authority and are never enabled for v0-v4.
/// Every later version preserves them.
#[must_use]
pub fn supports_planning_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V5
            | IPC_PROTOCOL_VERSION_V6
            | IPC_PROTOCOL_VERSION_V7
            | IPC_PROTOCOL_VERSION_V8
            | IPC_PROTOCOL_VERSION_V9
    )
}

/// Task planning-metadata authoring is new authority and is never enabled for
/// v0-v5, whose `CreateTaskRequest` keeps its original objective-only shape.
/// Version 7 preserves it.
#[must_use]
pub fn supports_task_authoring_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V6
            | IPC_PROTOCOL_VERSION_V7
            | IPC_PROTOCOL_VERSION_V8
            | IPC_PROTOCOL_VERSION_V9
    )
}

/// Repository finder pages and span reopens are new authority and are
/// never enabled below v7.
#[must_use]
pub fn supports_finder_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V7 | IPC_PROTOCOL_VERSION_V8 | IPC_PROTOCOL_VERSION_V9
    )
}

/// Reports whether a negotiated session version accepts the TAKEOVER
/// human continuation (TASK-0169 amendment): version 8 or higher only.
/// TASK-0177 assigns version 8: only a v8 session accepts TAKEOVER or
/// its apply/append/render/reconcile operations. Every older negotiable
/// session refuses TAKEOVER by construction.
#[must_use]
pub fn supports_takeover_continuation(negotiated_version: u32) -> bool {
    negotiated_version >= IPC_PROTOCOL_VERSION_V8
}

/// Structured memory is new authority and is never enabled below v9.
#[must_use]
pub fn supports_memory_ipc(negotiated_version: u32) -> bool {
    negotiated_version >= IPC_PROTOCOL_VERSION_V9
}

/// The operator credential vault is new authority and is never enabled for
/// v0-v6 (ADR-0039).
#[must_use]
pub fn supports_vault_ipc(negotiated_version: u32) -> bool {
    matches!(
        negotiated_version,
        IPC_PROTOCOL_VERSION_V7 | IPC_PROTOCOL_VERSION_V8 | IPC_PROTOCOL_VERSION_V9
    )
}

/// The one vault entry an operator may store (ADR-0039): the Claude Code
/// setup-token. The daemon refuses every other name.
pub const VAULT_PROVIDER_CREDENTIAL_ENTRY: &str = "provider.claude.oauth_token";

/// The setup-token exposes no expiry; it is treated as valid for one year
/// from the time it was stored.
pub const PROVIDER_CREDENTIAL_LIFETIME_DAYS: u64 = 365;

/// Operators are warned this many days before the lifetime ends.
pub const PROVIDER_CREDENTIAL_WARN_DAYS: u64 = 30;

/// Generated `Debug` is skipped for these messages (see `build.rs`); this one
/// names the type and nothing else, so a logged request or response cannot
/// carry a password, recovery phrase or credential value.
macro_rules! redacted_debug {
    ($($name:ident),+ $(,)?) => {
        $(impl std::fmt::Debug for proto::$name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        })+
    };
}

redacted_debug!(
    VaultCreateRequest,
    VaultUnlockRequest,
    VaultStoreRequest,
    VaultChangePasswordRequest,
    VaultRecoverRequest,
    VaultRotateRequest,
    VaultPhraseResponse,
);

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
    fn historical_versions_remain_supported_and_v9_is_current() {
        assert_eq!(IPC_PROTOCOL_VERSION_V0, 0);
        assert_eq!(IPC_PROTOCOL_VERSION_V1, 1);
        assert_eq!(IPC_PROTOCOL_VERSION_V2, 2);
        assert_eq!(IPC_PROTOCOL_VERSION_V3, 3);
        assert_eq!(IPC_PROTOCOL_VERSION_V5, 5);
        assert_eq!(IPC_PROTOCOL_VERSION_V6, 6);
        assert_eq!(IPC_PROTOCOL_VERSION_V7, 7);
        assert_eq!(IPC_PROTOCOL_VERSION_V8, 8);
        assert_eq!(IPC_PROTOCOL_VERSION_V9, 9);
        assert_eq!(IPC_PROTOCOL_VERSION, 9);
        assert_eq!(IPC_PROTOCOL_VERSION_CURRENT, 9);
        assert_eq!(SUPPORTED_PROTOCOL_VERSIONS, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        for version in 0..=9 {
            assert!(is_supported_version(version), "v{version} negotiates");
            assert!(is_compatible(version), "v{version} negotiates");
        }
        assert!(!is_supported_version(10));
        assert!(!is_compatible(10));
        assert!(!supports_memory_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_memory_ipc(IPC_PROTOCOL_VERSION_V9));
    }

    #[test]
    fn protocol_authority_is_cumulative_and_v4_adds_human_decisions() {
        assert!(!supports_business_ipc(IPC_PROTOCOL_VERSION_V0));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V1));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V2));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V3));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V4));
        assert!(!supports_business_ipc(99));
        assert!(!supports_execution_ipc(IPC_PROTOCOL_VERSION_V0));
        assert!(!supports_execution_ipc(IPC_PROTOCOL_VERSION_V1));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V2));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V3));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V4));
        assert!(!supports_execution_ipc(99));
        assert!(!supports_policy_ipc(IPC_PROTOCOL_VERSION_V2));
        assert!(supports_policy_ipc(IPC_PROTOCOL_VERSION_V3));
        assert!(supports_policy_ipc(IPC_PROTOCOL_VERSION_V4));
        assert!(!supports_policy_ipc(99));
        assert!(!supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V3));
        assert!(supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V4));
        assert!(supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V5));
        assert!(!supports_planning_ipc(IPC_PROTOCOL_VERSION_V4));
        assert!(supports_planning_ipc(IPC_PROTOCOL_VERSION_V5));
        assert!(!supports_human_decision_ipc(99));
    }

    #[test]
    fn v6_adds_task_authoring_without_granting_it_to_historical_sessions() {
        for historical in IPC_PROTOCOL_VERSION_V0..=IPC_PROTOCOL_VERSION_V5 {
            assert!(
                !supports_task_authoring_ipc(historical),
                "v{historical} never gains Task authoring authority"
            );
        }
        assert!(supports_task_authoring_ipc(IPC_PROTOCOL_VERSION_V6));
        assert!(!supports_task_authoring_ipc(99));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V6));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V6));
        assert!(supports_policy_ipc(IPC_PROTOCOL_VERSION_V6));
        assert!(supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V6));
        assert!(supports_planning_ipc(IPC_PROTOCOL_VERSION_V6));
    }

    #[test]
    fn v7_adds_the_vault_without_granting_it_to_historical_sessions() {
        for historical in IPC_PROTOCOL_VERSION_V0..=IPC_PROTOCOL_VERSION_V6 {
            assert!(
                !supports_vault_ipc(historical),
                "v{historical} never gains vault authority"
            );
        }
        assert!(supports_vault_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(!supports_vault_ipc(99));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_policy_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_planning_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_task_authoring_ipc(IPC_PROTOCOL_VERSION_V7));
    }

    /// Every secret-bearing vault message, reached through the envelopes a
    /// session or log line would format, renders without its secret bytes.
    #[test]
    fn vault_secrets_never_render_in_debug() {
        use crate::proto::{
            control_request, control_response, vault_core_request, vault_core_response,
            ControlRequest, ControlResponse, VaultChangePasswordRequest, VaultCoreRequest,
            VaultCoreResponse, VaultCreateRequest, VaultPhraseResponse, VaultRecoverRequest,
            VaultRotateRequest, VaultStoreRequest, VaultUnlockRequest,
        };
        use vault_core_request::Payload;

        const MARKER: &str = "SECRET-MARKER-7f3a";
        let secret = || MARKER.as_bytes().to_vec();
        let requests = [
            Payload::Create(VaultCreateRequest {
                password: secret(),
                idle_minutes: 0,
            }),
            Payload::Unlock(VaultUnlockRequest {
                password: secret(),
                idle_minutes: 0,
            }),
            Payload::Store(VaultStoreRequest {
                password: secret(),
                name: "provider.claude.oauth_token".to_owned(),
                value: secret(),
            }),
            Payload::ChangePassword(VaultChangePasswordRequest {
                current_password: secret(),
                new_password: secret(),
            }),
            Payload::Recover(VaultRecoverRequest {
                recovery_phrase: secret(),
                new_password: secret(),
                idle_minutes: 0,
            }),
            Payload::Rotate(VaultRotateRequest { password: secret() }),
        ];
        for payload in requests {
            let envelope = ControlRequest {
                payload: Some(control_request::Payload::VaultCore(VaultCoreRequest {
                    payload: Some(payload),
                })),
            };
            let rendered = format!("{envelope:?}");
            assert!(!rendered.contains(MARKER), "leaked: {rendered}");
            assert!(rendered.contains("Vault"), "type still named: {rendered}");
        }
        let response = ControlResponse {
            payload: Some(control_response::Payload::VaultCore(VaultCoreResponse {
                payload: Some(vault_core_response::Payload::Phrase(VaultPhraseResponse {
                    recovery_phrase: secret(),
                    status: None,
                })),
            })),
        };
        assert!(!format!("{response:?}").contains(MARKER));
    }

    #[test]
    fn v7_adds_finder_without_granting_it_to_historical_sessions() {
        for historical in IPC_PROTOCOL_VERSION_V0..=IPC_PROTOCOL_VERSION_V6 {
            assert!(
                !supports_finder_ipc(historical),
                "v{historical} never gains finder authority"
            );
        }
        assert!(supports_finder_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(!supports_finder_ipc(99));
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_policy_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_planning_ipc(IPC_PROTOCOL_VERSION_V7));
        assert!(supports_task_authoring_ipc(IPC_PROTOCOL_VERSION_V7));
    }

    #[test]
    fn takeover_continuation_requires_v8_or_higher() {
        for historical in IPC_PROTOCOL_VERSION_V0..=IPC_PROTOCOL_VERSION_V7 {
            assert!(
                !supports_takeover_continuation(historical),
                "v{historical} never accepts the TAKEOVER continuation"
            );
        }
        assert!(supports_takeover_continuation(8));
        assert!(supports_takeover_continuation(9));
    }

    #[test]
    fn v8_preserves_every_older_surface_and_adds_takeover() {
        assert!(supports_business_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_execution_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_policy_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_human_decision_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_planning_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_task_authoring_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_finder_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(supports_vault_ipc(IPC_PROTOCOL_VERSION_V8));
        assert!(!supports_business_ipc(99));
        assert!(!supports_finder_ipc(99));
        assert!(!supports_vault_ipc(99));
    }

    #[test]
    fn v0_through_v7_still_refuse_takeover_after_v8_assignment() {
        for version in 0..=7u32 {
            assert!(is_supported_version(version));
            assert!(!supports_takeover_continuation(version));
        }
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
    fn execution_core_envelope_uses_field_five_and_round_trips() {
        use crate::proto::{
            execution_core_request, CancelExecutionRequest, ExecutionCoreRequest,
            GetExecutionRequest, ResumeExecutionRequest, RunExecutionRequest,
        };
        let business = ControlRequest {
            payload: Some(control_request::Payload::ExecutionCore(
                ExecutionCoreRequest {
                    payload: Some(execution_core_request::Payload::Run(RunExecutionRequest {
                        task_id: "T-1".to_owned(),
                        execution_id: "E-1".to_owned(),
                        attempt_id: "A-1".to_owned(),
                        repository_ref: "repo-local:demo".to_owned(),
                        repository_path: "/srv/demo".to_owned(),
                        base_revision: "HEAD".to_owned(),
                    })),
                },
            )),
        };
        let bytes = business.encode_to_vec();
        // Field 5, wire type LEN: (5 << 3) | 2 = 0x2A.
        assert_eq!(bytes[0], 0x2A);
        let decoded = ControlRequest::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(control_request::Payload::ExecutionCore(request)) => match request.payload {
                Some(execution_core_request::Payload::Run(run)) => {
                    assert_eq!(run.task_id, "T-1");
                    assert_eq!(run.execution_id, "E-1");
                    assert_eq!(run.attempt_id, "A-1");
                    assert_eq!(run.repository_path, "/srv/demo");
                    assert_eq!(run.base_revision, "HEAD");
                }
                other => panic!("expected run, got {other:?}"),
            },
            other => panic!("expected execution_core envelope, got {other:?}"),
        }
        // Get, cancel, and resume variants decode on the same envelope.
        for request in [
            ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                    execution_id: "E-1".to_owned(),
                })),
            },
            ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Cancel(
                    CancelExecutionRequest {
                        execution_id: "E-1".to_owned(),
                    },
                )),
            },
            ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Resume(
                    ResumeExecutionRequest {
                        execution_id: "E-1".to_owned(),
                        repository_path: "/srv/demo".to_owned(),
                        repository_ref: String::new(),
                        base_revision: String::new(),
                    },
                )),
            },
        ] {
            let bytes = request.encode_to_vec();
            let decoded = ExecutionCoreRequest::decode(bytes.as_slice()).expect("must decode");
            assert!(decoded.payload.is_some());
        }
    }

    #[test]
    fn execution_resume_response_round_trip() {
        use crate::proto::{
            execution_core_response, ExecutionCoreResponse, ResumeExecutionResponse,
        };
        // The additive TASK-0054 variant keeps the error field clear: the
        // success range grows (resume = 5) while error stays 4.
        let resumed = ExecutionCoreResponse {
            payload: Some(execution_core_response::Payload::Resume(
                ResumeExecutionResponse {
                    task_id: "T-1".to_owned(),
                    execution_id: "E-1".to_owned(),
                    attempt_id: "A-1".to_owned(),
                    task_state: "RUNNING".to_owned(),
                },
            )),
        };
        let bytes = resumed.encode_to_vec();
        let decoded = ExecutionCoreResponse::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(execution_core_response::Payload::Resume(resumed)) => {
                assert_eq!(resumed.execution_id, "E-1");
                assert_eq!(resumed.attempt_id, "A-1");
                assert_eq!(resumed.task_state, "RUNNING");
            }
            other => panic!("expected resume, got {other:?}"),
        }
    }

    #[test]
    fn execution_status_and_error_round_trip() {
        use crate::proto::{
            execution_core_response, AttemptView, BusinessError, BusinessErrorCode,
            ExecutionCoreResponse, GetExecutionResponse,
        };
        let status = ExecutionCoreResponse {
            payload: Some(execution_core_response::Payload::Get(
                GetExecutionResponse {
                    task_id: "T-1".to_owned(),
                    execution_id: "E-1".to_owned(),
                    task_state: "RUNNING".to_owned(),
                    attempts: vec![AttemptView {
                        attempt_id: "A-1".to_owned(),
                        outcome: "OPEN".to_owned(),
                        provider_ref: "claude-code".to_owned(),
                        detail: String::new(),
                    }],
                    provider_ref: "claude-code".to_owned(),
                    workspace_path: "/tmp/ws".to_owned(),
                    execution_live: true,
                },
            )),
        };
        let bytes = status.encode_to_vec();
        let decoded = ExecutionCoreResponse::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(execution_core_response::Payload::Get(get)) => {
                assert_eq!(get.attempts.len(), 1);
                assert_eq!(get.attempts[0].outcome, "OPEN");
                assert!(get.execution_live);
            }
            other => panic!("expected get, got {other:?}"),
        }
        // The execution surface reuses the business error taxonomy plus
        // the G5 provider-unavailable code.
        let error = ExecutionCoreResponse {
            payload: Some(execution_core_response::Payload::Error(BusinessError {
                code: BusinessErrorCode::ProviderUnavailable as i32,
                message: "provider unavailable".to_owned(),
            })),
        };
        let bytes = error.encode_to_vec();
        let decoded = ExecutionCoreResponse::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(execution_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::ProviderUnavailable as i32);
                assert_ne!(error.code, BusinessErrorCode::Internal as i32);
            }
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn policy_core_envelope_uses_field_six_and_round_trips() {
        use crate::proto::{
            control_request, policy_core_request, ApprovalDecision, DecideApprovalRequest,
            PolicyCoreRequest,
        };

        let request = ControlRequest {
            payload: Some(control_request::Payload::PolicyCore(PolicyCoreRequest {
                payload: Some(policy_core_request::Payload::DecideApproval(
                    DecideApprovalRequest {
                        request_id: "approval-1".to_owned(),
                        decision: ApprovalDecision::Approve as i32,
                    },
                )),
            })),
        };
        let bytes = request.encode_to_vec();
        // Field 6, wire type LEN: (6 << 3) | 2 = 0x32.
        assert_eq!(bytes[0], 0x32);
        let decoded = ControlRequest::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(control_request::Payload::PolicyCore(policy)) => match policy.payload {
                Some(policy_core_request::Payload::DecideApproval(decide)) => {
                    assert_eq!(decide.request_id, "approval-1");
                    assert_eq!(decide.decision, ApprovalDecision::Approve as i32);
                }
                other => panic!("expected decide approval, got {other:?}"),
            },
            other => panic!("expected policy_core envelope, got {other:?}"),
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

    #[test]
    fn finder_core_envelope_uses_field_ten_and_round_trips() {
        use crate::proto::{
            finder_core_request, finder_core_response, FinderCoreRequest, FinderCoreResponse,
            FinderErrorResponse, FinderPageResponse, FinderQueryRequest,
        };
        let request = FinderCoreRequest {
            payload: Some(finder_core_request::Payload::Query(FinderQueryRequest {
                repository_path: "/repo".to_owned(),
                text: "needle".to_owned(),
                scope: "src".to_owned(),
                limit: 10,
                continuation: String::new(),
            })),
        };
        let envelope = ControlRequest {
            payload: Some(control_request::Payload::FinderCore(request)),
        };
        let bytes = envelope.encode_to_vec();
        let decoded = ControlRequest::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(control_request::Payload::FinderCore(inner)) => match inner.payload {
                Some(finder_core_request::Payload::Query(query)) => {
                    assert_eq!(query.repository_path, "/repo");
                    assert_eq!(query.text, "needle");
                    assert_eq!(query.scope, "src");
                    assert_eq!(query.limit, 10);
                }
                other => panic!("expected finder query, got {other:?}"),
            },
            other => panic!("expected finder_core envelope, got {other:?}"),
        }
        let response = FinderCoreResponse {
            payload: Some(finder_core_response::Payload::Error(FinderErrorResponse {
                code: "stale".to_owned(),
                detail: String::new(),
                expected_rev: "r1".to_owned(),
                actual_rev: "r2".to_owned(),
            })),
        };
        let bytes = response.encode_to_vec();
        let decoded = FinderCoreResponse::decode(bytes.as_slice()).expect("must decode");
        match decoded.payload {
            Some(finder_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, "stale");
                assert_eq!(error.expected_rev, "r1");
                assert_eq!(error.actual_rev, "r2");
            }
            other => panic!("expected finder error, got {other:?}"),
        }
        let _ = FinderPageResponse::default();
    }
}
