//! Persistence-neutral WorkspaceLease storage boundary.
//!
//! The Workspace context owns lease semantics; SQLite (or any future
//! backend) implements this trait behind it. Methods that take journal
//! evidence commit the durable state change together with its authoritative
//! event atomically — a backend must never persist one half without the
//! other. Guarded transitions fail when the stored lifecycle is not the
//! expected predecessor, so concurrent movers surface instead of silently
//! overwriting newer state.
//!
//! Error model: every failure surfaces as [`WorkspaceFailure`]; storage
//! backends map their I/O, constraint, and corruption failures to
//! [`WorkspaceFailure::StorageFailure`] with precise detail. Git-domain
//! failures never originate here.

use crate::failure::WorkspaceFailure;
use crate::ids::WorkspaceId;
use crate::lease::{LeaseLifecycle, WorkspaceLease};
use crate::trusted::TrustedCommonGit;
use agent_runtime::ExecutionId;
use task_domain::{ProjectId, TaskId};

/// The two canonical workspace journal facts (ADR-0023 §9). Closed: no
/// other workspace event kind exists, and implementations must reject
/// unknown kinds rather than coining names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkspaceEventKind {
    /// Lease allocated + verified consistent (`Allocated → Ready`).
    Created,
    /// Application-owned worktree + branch torn down after safety checks
    /// (`CleanupPending → Removed`).
    Removed,
}

impl WorkspaceEventKind {
    /// Stable persisted name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "workspace.created",
            Self::Removed => "workspace.removed",
        }
    }

    /// The lifecycle transition this fact evidences. Fixed by the contract:
    /// callers never choose it.
    #[must_use]
    pub const fn transition(self) -> (LeaseLifecycle, LeaseLifecycle) {
        match self {
            Self::Created => (LeaseLifecycle::Allocated, LeaseLifecycle::Ready),
            Self::Removed => (LeaseLifecycle::CleanupPending, LeaseLifecycle::Removed),
        }
    }
}

/// Persistence-neutral journal evidence for one workspace fact. The trusted
/// service mints correlation, causation, timestamp, and provenance; the
/// backend translates this into its journal row. The lifecycle
/// from/to pair always comes from [`WorkspaceEventKind::transition`] —
/// never from caller input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceJournalEvidence {
    /// Which canonical fact this evidences.
    pub kind: WorkspaceEventKind,
    /// Owning task, derived through Execution ownership (never caller scope).
    pub task_id: TaskId,
    /// Owning project, derived through Execution ownership.
    pub project_id: ProjectId,
    /// Owning execution (fixed lease owner).
    pub execution_id: ExecutionId,
    /// Lease this fact is about.
    pub workspace_id: WorkspaceId,
    /// Chain this mutation belongs to (service-minted).
    pub correlation_id: String,
    /// Prior request that directly caused this fact (service-minted).
    pub causation_id: String,
    /// Wall-clock timestamp, RFC 3339 UTC (service-minted).
    pub occurred_at: String,
    /// Fixed service provenance (never model narrative).
    pub provenance: String,
}

/// Durable lease storage behind one narrow port. No generic repository:
/// exactly the reads and trusted transitions `WorkspaceService` needs.
/// `crates/workspace` stays SQLite-independent — backends live in adapters.
pub trait WorkspaceStore {
    /// Loads one lease by identity, or `None` when absent. Corrupt rows
    /// fail as [`WorkspaceFailure::StorageFailure`], never as defaults.
    fn get_workspace(&self, id: &WorkspaceId) -> Result<Option<WorkspaceLease>, WorkspaceFailure>;

    /// Loads the single lease owned by `owner`, or `None` when the
    /// execution owns none. At most one row ever matches (UNIQUE owner).
    fn get_workspace_for_execution(
        &self,
        owner: &ExecutionId,
    ) -> Result<Option<WorkspaceLease>, WorkspaceFailure>;

    /// Whether the canonical fact `kind` is already journaled for `id`.
    /// Powers exactly-once guards: a `Ready` lease must already have its
    /// `Created` fact, and reconciliation must never append it again.
    fn has_workspace_event(
        &self,
        id: &WorkspaceId,
        kind: WorkspaceEventKind,
    ) -> Result<bool, WorkspaceFailure>;

    /// Persists an `Allocated` lease intent. No journal event: intent is
    /// operational reconciliation state, not yet a verified fact. Fails
    /// when the id exists, the execution already owns a lease, or the
    /// owning execution is absent.
    fn insert_allocated_lease(&mut self, lease: &WorkspaceLease) -> Result<(), WorkspaceFailure>;

    /// Persists an `Allocated` lease intent together with its trusted
    /// common-Git anchor in one short SQLite transaction (no Git subprocess
    /// inside, INV-145). Git observations must precede this call. No journal
    /// event for the anchor.
    fn insert_allocated_lease_with_anchor(
        &mut self,
        lease: &WorkspaceLease,
        anchor: &TrustedCommonGit,
        captured_at_millis: u64,
    ) -> Result<(), WorkspaceFailure>;

    /// Reads the durable trusted anchor for `id`, validating absolute,
    /// NUL-free, canonicalize-compatible form. Invalid rows fail closed as
    /// `GitBackendFailure` (never backfilled from current FS). `None` means
    /// no sidecar row (pre-existing lease, machine move, loss) — callers
    /// must fail closed until re-admission with a new lease.
    fn get_trusted_anchor(
        &self,
        id: &WorkspaceId,
    ) -> Result<Option<TrustedCommonGit>, WorkspaceFailure>;

    /// Records verified Git consistency (`Allocated → Ready`) together
    /// with the authoritative `workspace.created` fact in one atomic
    /// commit. Fails when the stored lease is not `Allocated` (a
    /// concurrent mover won) — the caller re-reads instead of overwriting.
    fn mark_lease_ready(
        &mut self,
        id: &WorkspaceId,
        evidence: &WorkspaceJournalEvidence,
    ) -> Result<(), WorkspaceFailure>;

    /// Records teardown request (`Ready → CleanupPending`). No journal
    /// event: the request authorizes nothing destructive and no new
    /// taxonomy exists for it. Fails unless currently `Ready`.
    fn mark_lease_cleanup_pending(&mut self, id: &WorkspaceId) -> Result<(), WorkspaceFailure>;

    /// Records completed safe teardown (`CleanupPending → Removed`)
    /// together with the authoritative `workspace.removed` fact in one
    /// atomic commit. Persistence-level support for the frozen contract:
    /// no service path may call this without successful cleanup
    /// observation (TASK-0043 owns that observation). Fails unless
    /// currently `CleanupPending`.
    fn mark_lease_removed(
        &mut self,
        id: &WorkspaceId,
        evidence: &WorkspaceJournalEvidence,
    ) -> Result<(), WorkspaceFailure>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_event_kinds_carry_fixed_transitions() {
        assert_eq!(WorkspaceEventKind::Created.as_str(), "workspace.created");
        assert_eq!(WorkspaceEventKind::Removed.as_str(), "workspace.removed");
        assert_eq!(
            WorkspaceEventKind::Created.transition(),
            (LeaseLifecycle::Allocated, LeaseLifecycle::Ready)
        );
        assert_eq!(
            WorkspaceEventKind::Removed.transition(),
            (LeaseLifecycle::CleanupPending, LeaseLifecycle::Removed)
        );
    }
}
