//! Bounded typed Workspace/Git failure taxonomy.
//!
//! One class per cause family — never one per stderr string. Infrastructure
//! diagnostics travel as opaque detail text; domain code matches on the
//! variant, never on Git's prose.

use std::fmt;

/// Typed workspace failure for the first Git adapter and its callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceFailure {
    /// Path is not inside a Git repository.
    NotRepository { detail: String },
    /// Bare repositories are unsupported: fail explicitly, no implicit clone.
    BareRepositoryUnsupported { detail: String },
    /// Creation policy required a clean primary checkout and observed dirt.
    DirtyPrimaryWorkspace { detail: String },
    /// Intended managed branch ref already exists with different ownership.
    BranchCollision { detail: String },
    /// Intended worktree path already registered or occupied.
    WorktreeCollision { detail: String },
    /// Worktree holds uncommitted or untracked work where clean was required.
    WorktreeDirty { detail: String },
    /// Registered worktree path absent from the filesystem.
    WorktreeMissing { detail: String },
    /// Worktree creation or checkout did not reach a consistent state.
    CheckoutFailed { detail: String },
    /// Actual branch/ref state does not match recorded lease ownership —
    /// never silently reused, never deleted as cleanup.
    OwnershipMismatch { detail: String },
    /// Revision does not resolve to a commit object.
    InvalidRevision { detail: String },
    /// Destruction refused: dirty work and no destructive authorization.
    CleanupUnsafe { detail: String },
    /// Cleanup rejected: the owning execution is known live.
    ActiveOwnerRejectsCleanup { detail: String },
    /// Creation rejected: an overlapping operation owns the target.
    ActiveOwnerRejectsCheckout { detail: String },
    /// Backend-level failure (exit status, I/O, unexpected output).
    GitBackendFailure { detail: String },
    /// Durable-ownership failure: the lease store (SQLite) rejected the
    /// operation or the persisted lease bytes are unusable. Added in
    /// TASK-0042 alongside durable ownership — the second failure domain
    /// (INV-145) needs its own typed class rather than borrowing Git's.
    /// Corrupt rows, guarded-transition conflicts, and SQL/IO failures
    /// all surface here with precise detail text.
    StorageFailure { detail: String },
    /// Idempotent cleanup hit: already absent and ownership-consistent.
    AlreadyAbsent,
}

impl WorkspaceFailure {
    /// Stable machine-readable class name (never Git prose).
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::NotRepository { .. } => "not_repository",
            Self::BareRepositoryUnsupported { .. } => "bare_repository_unsupported",
            Self::DirtyPrimaryWorkspace { .. } => "dirty_primary_workspace",
            Self::BranchCollision { .. } => "branch_collision",
            Self::WorktreeCollision { .. } => "worktree_collision",
            Self::WorktreeDirty { .. } => "worktree_dirty",
            Self::WorktreeMissing { .. } => "worktree_missing",
            Self::CheckoutFailed { .. } => "checkout_failed",
            Self::OwnershipMismatch { .. } => "ownership_mismatch",
            Self::InvalidRevision { .. } => "invalid_revision",
            Self::CleanupUnsafe { .. } => "cleanup_unsafe",
            Self::ActiveOwnerRejectsCleanup { .. } => "active_owner_rejects_cleanup",
            Self::ActiveOwnerRejectsCheckout { .. } => "active_owner_rejects_checkout",
            Self::GitBackendFailure { .. } => "git_backend_failure",
            Self::StorageFailure { .. } => "storage_failure",
            Self::AlreadyAbsent => "already_absent",
        }
    }
}

impl fmt::Display for WorkspaceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyAbsent => write!(f, "workspace already absent"),
            Self::NotRepository { detail }
            | Self::BareRepositoryUnsupported { detail }
            | Self::DirtyPrimaryWorkspace { detail }
            | Self::BranchCollision { detail }
            | Self::WorktreeCollision { detail }
            | Self::WorktreeDirty { detail }
            | Self::WorktreeMissing { detail }
            | Self::CheckoutFailed { detail }
            | Self::OwnershipMismatch { detail }
            | Self::InvalidRevision { detail }
            | Self::CleanupUnsafe { detail }
            | Self::ActiveOwnerRejectsCleanup { detail }
            | Self::ActiveOwnerRejectsCheckout { detail }
            | Self::GitBackendFailure { detail }
            | Self::StorageFailure { detail } => {
                write!(f, "{}: {detail}", self.kind())
            }
        }
    }
}

impl std::error::Error for WorkspaceFailure {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taxonomy_is_bounded_with_stable_kinds() {
        let failures = [
            WorkspaceFailure::NotRepository { detail: "x".into() },
            WorkspaceFailure::BranchCollision { detail: "x".into() },
            WorkspaceFailure::OwnershipMismatch { detail: "x".into() },
            WorkspaceFailure::CleanupUnsafe { detail: "x".into() },
            WorkspaceFailure::AlreadyAbsent,
        ];
        let kinds: Vec<&str> = failures.iter().map(|f| f.kind()).collect();
        assert_eq!(
            kinds,
            [
                "not_repository",
                "branch_collision",
                "ownership_mismatch",
                "cleanup_unsafe",
                "already_absent",
            ]
        );
    }
}
