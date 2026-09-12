//! Bootstrap control protocol for local `agentd` IPC: handshake,
//! protocol-version negotiation, client-kind validation, capability
//! negotiation, and ping.
//!
//! Protocol layer between [`ipc-contracts`](ipc_contracts) (wire DTOs) and
//! [`ipc-transport`](ipc_transport) (bytes/frames):
//!
//! ```text
//! ipc-control
//!  ├── ipc-contracts   (Protobuf DTOs)
//!  └── ipc-transport   (framing + byte streams)
//! ```
//!
//! Data-plane notes: `client_kind` (`tauri-host`/`agentctl`) is descriptive
//! handshake metadata, never authentication and never authorization
//! (TB2/THR-003). `ShutdownRequest` is parsed and exposed without any
//! lifecycle semantics — TASK-0020 owns shutdown. No daemon spawning, no
//! reconnect orchestration, no business IPC.

pub mod error;
pub mod negotiation;
pub mod session;

pub use error::{ControlError, ControlResult};
pub use negotiation::{
    default_client_build_version, default_daemon_build_version, evaluate_handshake,
    is_business_allowed, is_execution_allowed, is_policy_allowed, is_recognized_client_kind,
    negotiate_capabilities, rejection_for, select_highest_common_version,
    validate_business_allowed, validate_execution_allowed, validate_policy_allowed,
    AcceptedParameters, ServerConfig, CLIENT_KIND_AGENTCTL, CLIENT_KIND_TAURI_HOST,
};
pub use session::{
    ClientConfig, ClientSession, HandshakeAcceptedInfo, ReadyRequest, ServerHandshakeInfo,
    ServerSession, HANDSHAKE_TIMEOUT, PING_TIMEOUT,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_deadlines_match_accepted_profile() {
        assert_eq!(HANDSHAKE_TIMEOUT, std::time::Duration::from_secs(5));
        assert_eq!(PING_TIMEOUT, std::time::Duration::from_secs(2));
        assert_eq!(HANDSHAKE_TIMEOUT, ipc_transport::FRAME_IO_TIMEOUT);
    }
}
