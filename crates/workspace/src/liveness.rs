//! Provider-neutral execution-owner liveness seam (TASK-0046, G5).
//!
//! Consumer-owned port: the Workspace context requires the capability
//! ("is this execution owner still live?") and infrastructure implements
//! it. No provider types appear here — liveness travels as a plain `bool`
//! keyed by the durable [`ExecutionId`], so any current or future provider
//! (Claude, Codex, OpenCode, …) can supply it without the Workspace
//! context depending on a provider adapter (INV-025/026/027).
//!
//! Backends with no liveness source report `false` honestly (exactly the G4
//! posture: never faked `true`); cleanup then treats owners as not-live,
//! which is the safe direction only because dirtiness and ownership checks
//! still apply independently.

use agent_runtime::ExecutionId;

/// Provider-neutral liveness source for execution owners.
///
/// Implemented by runtime-side infrastructure that actually tracks live
/// provider processes (e.g. the Claude adapter's process tracker). The
/// Workspace context consumes it through [`crate::GitPort::owner_is_live`]
/// to refuse cleanup of a live execution's workspace.
pub trait OwnerLiveness: Send + Sync {
    /// Whether `owner` currently shows live execution activity.
    ///
    /// Must be derived from real process/handle ownership — never from
    /// process-name guessing, never a constant `true`.
    fn is_live(&self, owner: &ExecutionId) -> bool;
}
