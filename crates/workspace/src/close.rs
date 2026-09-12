//! Attachment-close shape vocabulary (workspace crate).
//!
//! `WorkspaceLease::attachment_close(&self, &Path) -> CloseOutcome` is the
//! pure lease-side step of the attachment flow: it classifies the *shape* of
//! the request — lifecycle state of the in-memory lease and the layout of
//! the absolute target path — without touching Git, the filesystem, or
//! durable ownership records (INV-025-ish: the workspace crate never reads
//! another context's state). Every observed cause maps to one enum; refusals
//! are spelled out, low-level diagnostics stay in adapters.
//!
//! `Live` and `HeldByDeadProcess` exist for completeness of the vocabulary
//! (an adapter that *can* observe liveness produces them when seating an
//! existing registration). The pure [`AttachmentClose`] implementation can
//! never produce them — it has no observatory — and yields only the
//! structural subset:
//!
//! - [`CloseOutcome::ReadyToClose`] — path shape is seatable; the adapter
//!   proceeds to its observed refinements (missing dir → `Orphaned`,
//!   registered entry → `ReadyToClose` post-mutation, and so on).
//! - [`CloseOutcome::EntryUnparseable`] — the path has no final component.
//! - [`CloseOutcome::ConstructionForbidden`] — the path shape attacks the
//!   seat (currently: traverses out of its designated root).
//!
//! No continuation is ever auto-picked from the pure classification alone;
//! only after adapter-observed evidence refines the shape may a mutation
//! run, and always under the postcondition guards in `attach.rs`.

use std::path::{Component, Path};

use crate::lease::WorkspaceLease;

/// Why a renewed seat was refused before any mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    /// The supplied path wanders inside/above its designated target root via
    /// `..` components — Git would silently escape the lease's seat.
    InsideTarget,
}

/// Attachment-close outcome: one enum per observed cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// The seat is occupied by this lease and the owner is live; the adapter
    /// seats the live lease without materializing anything.
    Live,
    /// The seat's recorded owner process is dead; the previous lease is
    /// garbage and the renewal may safely re-seat (adapter-produced only).
    HeldByDeadProcess,
    /// Registration exists but the directory is gone; the adapter removes
    /// the ledger entry and re-materializes (adapter-produced only).
    OrphanedUnregistered,
    /// Nothing about the seat proved unsafe; ready for the close mutation
    /// under the usual postconditions.
    ReadyToClose,
    /// The target path could not be read as a directory entry.
    EntryUnparseable,
    /// Lease lifecycle or path shape forbids construction outright.
    ConstructionForbidden {
        /// Spelled-out refusal reason.
        reason: CloseReason,
    },
}

/// Pure lease-side attachment classification.
pub trait AttachmentClose {
    /// Classifies the requested seat for this lease.
    ///
    /// Pure: no Git, no filesystem, no liveness. Produces the structural
    /// subset of [`CloseOutcome`]; adapters refine it with observation.
    fn attachment_close(&self, worktree_path: &Path) -> CloseOutcome;
}

impl AttachmentClose for WorkspaceLease {
    fn attachment_close(&self, worktree_path: &Path) -> CloseOutcome {
        if worktree_path.file_name().is_none() {
            return CloseOutcome::EntryUnparseable;
        }
        for component in worktree_path.components() {
            if matches!(component, Component::ParentDir) {
                return CloseOutcome::ConstructionForbidden {
                    reason: CloseReason::InsideTarget,
                };
            }
        }
        CloseOutcome::ReadyToClose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BranchRef, ExecutionId, RepositoryRef, Revision, WorkspaceId};

    fn lease() -> WorkspaceLease {
        WorkspaceLease::allocate(
            WorkspaceId::new("ws-close-1").unwrap(),
            RepositoryRef::new("/repo").unwrap(),
            ExecutionId::new("E-close-1").unwrap(),
            Revision::new("a".repeat(40)).unwrap(),
            BranchRef::new("refs/heads/agent/e-close-1").unwrap(),
            "managed-worktree-ref:ws-close-1",
        )
        .unwrap()
    }

    #[test]
    fn plain_absolute_path_is_ready() {
        let lease = lease();
        assert_eq!(
            lease.attachment_close(Path::new("/tmp/seat")),
            CloseOutcome::ReadyToClose
        );
    }

    #[test]
    fn root_only_path_is_unparseable() {
        let lease = lease();
        assert_eq!(
            lease.attachment_close(Path::new("/")),
            CloseOutcome::EntryUnparseable
        );
    }

    #[test]
    fn parent_traversal_is_forbidden() {
        let lease = lease();
        assert_eq!(
            lease.attachment_close(Path::new("/tmp/../etc/seat")),
            CloseOutcome::ConstructionForbidden {
                reason: CloseReason::InsideTarget
            }
        );
    }
}
