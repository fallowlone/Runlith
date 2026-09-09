//! Shared local IPC transport/framing foundation for `agentd` clients.
//!
//! This crate owns the accepted G1 byte-stream mechanics only:
//!
//! - wire framing (`u32` big-endian length + payload, 1 MiB cap),
//! - local endpoint resolution (UDS path / Named Pipe name),
//! - endpoint admission/security primitives (private-dir checks, no blind
//!   unlink, singleton-ownership errors),
//! - byte-stream accept/connect plus timeout-bounded frame I/O.
//!
//! It transports bytes/frames and never interprets Protobuf control
//! messages: handshake, ping, and shutdown semantics belong to TASK-0019,
//! and daemon lifecycle/reconnect belongs to TASK-0020. `local != trusted`
//! (TB2/THR-003): this layer establishes the per-user endpoint boundary
//! and extracts kernel/token-backed peer identity
//! ([`platform::peer`], ADR-0020); allow/reject policy belongs to `agentd`.

pub mod error;
pub mod framing;
pub mod platform;

pub use error::{TransportError, TransportResult};
pub use framing::{ControlFrameCodec, FramedTransport};
pub use platform::peer::{same_local_principal, PeerIdentity};

/// Maximum IPC frame payload in bytes: `1 MiB = 1_048_576`.
/// Control messages are tiny; larger artifacts must never travel as giant
/// control frames. Raising this bound requires explicit protocol evolution.
pub const MAX_FRAME_BYTES: usize = 1_048_576;

/// Maximum simultaneous local control clients (e.g. Tauri host +
/// `agentctl`) for the local listener. A ninth concurrent client may be
/// rejected/closed; no cross-client session state exists.
pub const MAX_CONTROL_CLIENTS: usize = 8;

/// Complete frame read/write deadline: 5 seconds each. Healthy idle
/// connections carry no timeout; liveness uses explicit ping (TASK-0019).
pub const FRAME_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_transport_constants_hold() {
        assert_eq!(MAX_FRAME_BYTES, 1_048_576);
        assert_eq!(MAX_CONTROL_CLIENTS, 8);
        assert_eq!(FRAME_IO_TIMEOUT, std::time::Duration::from_secs(5));
    }
}
