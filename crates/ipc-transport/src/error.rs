//! Transport-local error model for `ipc-transport`.
//!
//! Deliberately small: enough for TASK-0020 to distinguish endpoint
//! collision from permission/security failure, and for framing callers to
//! distinguish oversized/empty/malformed frames from plain I/O failure.
//! This is NOT the product-wide IPC error taxonomy, and these errors never
//! enter Protobuf payloads.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::FRAME_IO_TIMEOUT;

/// Errors produced by the local transport/framing foundation.
#[derive(Debug)]
pub enum TransportError {
    /// Underlying OS/runtime I/O failure with no more specific meaning.
    Io(io::Error),
    /// A per-user endpoint/security admission check failed. Fail closed:
    /// callers must not degrade to a less secure endpoint.
    EndpointSecurity(String),
    /// The endpoint path is unusable (missing parent, wrong object kind,
    /// non-absolute path, ...). No deletion was performed.
    InvalidEndpoint(String),
    /// The canonical endpoint is already owned — another daemon instance
    /// won. TASK-0020 interprets this as `AlreadyRunning`.
    EndpointOwned {
        /// Human-readable endpoint identity (socket path or pipe name).
        endpoint: String,
    },
    /// No listener is reachable at the endpoint (absent or refused).
    EndpointUnavailable {
        /// Human-readable endpoint identity (socket path or pipe name).
        endpoint: String,
    },
    /// The resolved endpoint path cannot be represented as a local
    /// socket/pipe address. Never truncated or hashed.
    PathTooLong {
        /// The offending path.
        path: PathBuf,
    },
    /// A frame exceeds [`MAX_FRAME_BYTES`](crate::MAX_FRAME_BYTES),
    /// on encode or as declared by an incoming length prefix.
    FrameTooLarge {
        /// Declared payload length when known (incoming prefix), else `None`.
        declared: Option<u32>,
        /// The enforced limit in bytes.
        limit: usize,
    },
    /// A zero-length application frame. Valid envelopes always carry a
    /// Protobuf message; empty payloads are protocol violations.
    EmptyFrame,
    /// Framing or connection broke mid-frame (truncation, unexpected EOF,
    /// connection loss). No resynchronization is attempted: the connection
    /// is local and cheap to re-establish.
    MalformedFrame(String),
    /// A bounded frame read/write exceeded its deadline.
    Timeout {
        /// The deadline that was exceeded.
        after: std::time::Duration,
    },
    /// OS peer-identity extraction or impersonation mechanics failed
    /// (unavailable credentials, impersonation/token/SID failure, revert
    /// failure). Internal diagnostic only: unauthenticated peers receive
    /// no protocol response, just a closed connection.
    PeerAuth(String),
}

/// Convenience alias for transport results.
pub type TransportResult<T> = Result<T, TransportError>;

impl TransportError {
    /// Builds the canonical frame-I/O timeout error.
    pub fn timeout() -> Self {
        Self::Timeout {
            after: FRAME_IO_TIMEOUT,
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "transport I/O failure: {e}"),
            Self::EndpointSecurity(detail) => {
                write!(f, "endpoint security violation: {detail}")
            }
            Self::InvalidEndpoint(detail) => write!(f, "invalid endpoint: {detail}"),
            Self::EndpointOwned { endpoint } => {
                write!(f, "endpoint already owned: {endpoint}")
            }
            Self::EndpointUnavailable { endpoint } => {
                write!(f, "endpoint unavailable: {endpoint}")
            }
            Self::PathTooLong { path } => {
                write!(f, "endpoint path too long: {}", path.display())
            }
            Self::FrameTooLarge { declared, limit } => match declared {
                Some(n) => write!(f, "frame of {n} bytes exceeds limit of {limit} bytes"),
                None => write!(f, "frame exceeds limit of {limit} bytes"),
            },
            Self::EmptyFrame => write!(f, "zero-length frame is invalid"),
            Self::MalformedFrame(detail) => write!(f, "malformed frame: {detail}"),
            Self::Timeout { after } => {
                write!(f, "frame I/O timed out after {}s", after.as_secs())
            }
            Self::PeerAuth(detail) => {
                write!(f, "peer authentication failure: {detail}")
            }
        }
    }
}

impl std::error::Error for TransportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
