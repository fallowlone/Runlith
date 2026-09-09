//! OS-local transport backends.
//!
//! Exactly one backend compiles per target: UDS on Unix, Named Pipes on
//! Windows. Both present the same framing contract upward; neither knows
//! any Protobuf protocol semantics.

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

/// OS-authenticated peer identity mechanics (ADR-0020). Extraction and
/// comparison only; allow/reject policy belongs to `agentd`.
pub mod peer;
