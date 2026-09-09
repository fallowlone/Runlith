//! Protocol-local error model for `ipc-control`.
//!
//! Deliberately small: enough to keep handshake rejection distinguishable
//! from timeouts, malformed payloads, transport failures, and ordering
//! violations. This is NOT the application-wide IPC taxonomy, and it never
//! enters Protobuf payloads.

use std::fmt;
use std::time::Duration;

use ipc_transport::TransportError;

/// Errors produced by the bootstrap control protocol layer.
#[derive(Debug)]
pub enum ControlError {
    /// The underlying byte transport failed. The original
    /// [`TransportError`] is preserved (not flattened) so permission,
    /// endpoint-security, and frame-limit signals stay observable.
    Transport(TransportError),
    /// A complete frame could not decode as the expected Protobuf control
    /// message. The session is terminated; no resynchronization.
    MalformedProtobuf(String),
    /// A structurally valid message arrived where another was required
    /// (wrong response variant, empty envelope, ...). Session terminated.
    UnexpectedMessage(String),
    /// A non-handshake request arrived before a successful handshake, or a
    /// post-handshake operation ran on a non-ready session.
    HandshakeRequired,
    /// A second `HandshakeRequest` arrived on an already-ready connection.
    DuplicateHandshake,
    /// Handshake fields are unacceptable in a way negotiation cannot express
    /// (empty version list, empty build version, ...).
    InvalidHandshake(String),
    /// `client_kind` is not in the G1 registry. Descriptive rejection only:
    /// `client_kind` is never authentication and never authorization.
    UnknownClientKind(String),
    /// Client and daemon share no protocol version.
    IncompatibleProtocol {
        /// Versions the client advertised.
        client: Vec<u32>,
        /// Versions the daemon supports.
        daemon: Vec<u32>,
    },
    /// The peer rejected the handshake. Carries the diagnostic reason and
    /// the daemon version set so a wrong-version peer stays observable.
    HandshakeRejected {
        /// Diagnostic reason; not domain authority.
        reason: String,
        /// Versions the daemon supports.
        daemon_versions: Vec<u32>,
    },
    /// A protocol operation exceeded its deadline.
    Timeout {
        /// Operation name (`"handshake"` or `"ping"`).
        op: &'static str,
        /// The deadline that was exceeded.
        after: Duration,
    },
    /// A business (`task_core`) request arrived on a session whose
    /// negotiated protocol version does not enable business IPC (G2 v1
    /// only). The request never executes: decoding is not authority.
    UnsupportedProtocol {
        /// Version negotiated for this session.
        negotiated: u32,
        /// Minimum version that enables business IPC (1).
        required: u32,
    },
    /// The session already terminated (fatal protocol violation, rejection,
    /// or transport failure) and cannot continue.
    SessionTerminated,
}

/// Convenience alias for protocol results.
pub type ControlResult<T> = Result<T, ControlError>;

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "control transport failure: {error}"),
            Self::MalformedProtobuf(detail) => write!(f, "malformed protobuf: {detail}"),
            Self::UnexpectedMessage(detail) => write!(f, "unexpected message: {detail}"),
            Self::HandshakeRequired => write!(f, "handshake required before control requests"),
            Self::DuplicateHandshake => write!(f, "duplicate handshake on a ready connection"),
            Self::InvalidHandshake(detail) => write!(f, "invalid handshake: {detail}"),
            Self::UnknownClientKind(kind) => write!(f, "unknown client kind: {kind}"),
            Self::IncompatibleProtocol { client, daemon } => write!(
                f,
                "no common protocol version (client {client:?}, daemon {daemon:?})"
            ),
            Self::HandshakeRejected { reason, .. } => write!(f, "handshake rejected: {reason}"),
            Self::Timeout { op, after } => {
                write!(f, "{op} timed out after {}s", after.as_secs())
            }
            Self::UnsupportedProtocol {
                negotiated,
                required,
            } => write!(
                f,
                "business IPC unsupported for negotiated protocol version {negotiated} (requires {required})"
            ),
            Self::SessionTerminated => write!(f, "protocol session already terminated"),
        }
    }
}

impl std::error::Error for ControlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TransportError> for ControlError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}
