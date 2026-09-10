//! Durable workspace-lease ownership model (contract types only).
//!
//! The lease is what later G4 persistence stores and what the Event Journal
//! evidences (`workspace.created` / `workspace.removed`). Live Git status
//! is observed separately ([`crate::WorktreeStatus`]) and never persisted
//! as authority here.

use std::fmt;

use agent_runtime::ExecutionId;

use crate::ids::{
    BranchRef, RepositoryRef, Revision, WorkspaceError, WorkspaceId, WorkspaceResult,
};

/// Small durable ownership lifecycle. Observations (clean/dirty/missing)
/// stay live-derived; only these markers persist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaseLifecycle {
    /// Intent recorded; the Git operation is not yet verified.
    Allocated,
    /// Worktree + branch verified mutually consistent with the lease.
    Ready,
    /// Teardown requested; safety checks still apply.
    CleanupPending,
    /// Terminal: application-owned worktree + branch torn down
    /// (tombstone enabling idempotent cleanup).
    Removed,
}

impl LeaseLifecycle {
    /// Canonical wire name for journal `from_state`/`to_state` semantics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allocated => "ALLOCATED",
            Self::Ready => "READY",
            Self::CleanupPending => "CLEANUP_PENDING",
            Self::Removed => "REMOVED",
        }
    }

    /// Whether the marker is terminal (no further Git work expected).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Strict decode of the canonical wire names from [`Self::as_str`].
    /// Anything else — including Task lifecycle states — is a domain
    /// input error, never a default. Added in TASK-0042 so persisted
    /// lifecycle markers round-trip through validation instead of raw
    /// field assignment.
    pub fn parse(raw: &str) -> WorkspaceResult<Self> {
        Ok(match raw {
            "ALLOCATED" => Self::Allocated,
            "READY" => Self::Ready,
            "CLEANUP_PENDING" => Self::CleanupPending,
            "REMOVED" => Self::Removed,
            other => {
                return Err(WorkspaceError::input(format!(
                    "unknown lease lifecycle {other:?}"
                )));
            }
        })
    }
}

impl fmt::Display for LeaseLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Durable ownership record for one Execution's isolated workspace.
///
/// Exactly one owner ExecutionId, fixed at allocation; Attempts never own
/// leases. `worktree_ref` is an opaque managed reference (path resolution
/// is adapter infrastructure, never domain identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLease {
    id: WorkspaceId,
    repository: RepositoryRef,
    owner: ExecutionId,
    base_revision: Revision,
    branch: BranchRef,
    worktree_ref: String,
    lifecycle: LeaseLifecycle,
}

impl WorkspaceLease {
    /// Allocates a lease intent. The caller must verify Git consistency
    /// before marking [`LeaseLifecycle::Ready`] via [`Self::mark_ready`].
    #[allow(clippy::too_many_arguments)]
    pub fn allocate(
        id: WorkspaceId,
        repository: RepositoryRef,
        owner: ExecutionId,
        base_revision: Revision,
        branch: BranchRef,
        worktree_ref: impl Into<String>,
    ) -> WorkspaceResult<Self> {
        let worktree_ref = worktree_ref.into();
        let trimmed = worktree_ref.trim();
        if trimmed.is_empty() {
            return Err(WorkspaceError::input("worktree ref must not be blank"));
        }
        Ok(Self {
            id,
            repository,
            owner,
            base_revision,
            branch,
            worktree_ref: trimmed.to_owned(),
            lifecycle: LeaseLifecycle::Allocated,
        })
    }

    /// Lease identity.
    #[must_use]
    pub fn id(&self) -> &WorkspaceId {
        &self.id
    }

    /// Owning Execution. Fixed at allocation, never rewritten: Attempts
    /// run inside this lease without owning it.
    #[must_use]
    pub fn owner(&self) -> &ExecutionId {
        &self.owner
    }

    /// Repository this lease belongs to.
    #[must_use]
    pub fn repository(&self) -> &RepositoryRef {
        &self.repository
    }

    /// Exact commit OID the worktree was based on.
    #[must_use]
    pub fn base_revision(&self) -> &Revision {
        &self.base_revision
    }

    /// Recorded managed branch ref (collision-detection baseline).
    #[must_use]
    pub fn branch(&self) -> &BranchRef {
        &self.branch
    }

    /// Opaque managed worktree reference (never domain identity).
    #[must_use]
    pub fn worktree_ref(&self) -> &str {
        &self.worktree_ref
    }

    /// Current durable lifecycle marker.
    #[must_use]
    pub fn lifecycle(&self) -> LeaseLifecycle {
        self.lifecycle
    }

    /// Records verified Git consistency (`Allocated → Ready`).
    pub fn mark_ready(&mut self) -> WorkspaceResult<()> {
        if self.lifecycle != LeaseLifecycle::Allocated {
            return Err(WorkspaceError::input(
                "lease must be Allocated before Ready is recorded",
            ));
        }
        self.lifecycle = LeaseLifecycle::Ready;
        Ok(())
    }

    /// Requests teardown (`Ready → CleanupPending`). Safety checks still
    /// apply at execution time; this marker authorizes nothing destructive.
    pub fn request_cleanup(&mut self) -> WorkspaceResult<()> {
        if self.lifecycle != LeaseLifecycle::Ready {
            return Err(WorkspaceError::input(
                "cleanup may only be requested from Ready",
            ));
        }
        self.lifecycle = LeaseLifecycle::CleanupPending;
        Ok(())
    }

    /// Records completed safe teardown (`CleanupPending → Removed`).
    pub fn mark_removed(&mut self) -> WorkspaceResult<()> {
        if self.lifecycle != LeaseLifecycle::CleanupPending {
            return Err(WorkspaceError::input(
                "lease must be CleanupPending before Removed is recorded",
            ));
        }
        self.lifecycle = LeaseLifecycle::Removed;
        Ok(())
    }

    /// Freezes this lease into plain validated strings for persistence.
    /// The snapshot carries the exact ADR-0023 durable facts — identities,
    /// immutable base revision, managed branch, worktree reference, and
    /// lifecycle marker — and nothing else (dirtiness is never snapshotted).
    #[must_use]
    pub fn snapshot(&self) -> WorkspaceLeaseSnapshot {
        WorkspaceLeaseSnapshot {
            workspace_id: self.id.as_str().to_owned(),
            repository_ref: self.repository.as_str().to_owned(),
            execution_id: self.owner.as_str().to_owned(),
            base_revision: self.base_revision.as_str().to_owned(),
            branch_ref: self.branch.as_str().to_owned(),
            worktree_ref: self.worktree_ref.clone(),
            lifecycle: self.lifecycle.as_str().to_owned(),
        }
    }

    /// Rebuilds a lease from a persisted snapshot, passing every field
    /// back through domain validation (never unchecked assignment) and
    /// replaying the lifecycle transitions in order, so an invalid stored
    /// row fails here instead of becoming a live lease.
    pub fn restore(snapshot: WorkspaceLeaseSnapshot) -> WorkspaceResult<Self> {
        let owner = ExecutionId::new(snapshot.execution_id).map_err(|error| {
            WorkspaceError::input(format!("stored execution reference: {error}"))
        })?;
        let mut lease = Self::allocate(
            WorkspaceId::new(snapshot.workspace_id)?,
            RepositoryRef::new(snapshot.repository_ref)?,
            owner,
            Revision::new(snapshot.base_revision)?,
            BranchRef::new(snapshot.branch_ref)?,
            snapshot.worktree_ref,
        )?;
        match LeaseLifecycle::parse(&snapshot.lifecycle)? {
            LeaseLifecycle::Allocated => {}
            LeaseLifecycle::Ready => lease.mark_ready()?,
            LeaseLifecycle::CleanupPending => {
                lease.mark_ready()?;
                lease.request_cleanup()?;
            }
            LeaseLifecycle::Removed => {
                lease.mark_ready()?;
                lease.request_cleanup()?;
                lease.mark_removed()?;
            }
        }
        Ok(lease)
    }
}

/// Persistence-neutral frozen form of a [`WorkspaceLease`]: the exact
/// ADR-0023 durable facts as validated strings plus the lifecycle wire
/// name. Storage adapters persist these fields and rebuild via
/// [`WorkspaceLease::restore`]; they never construct leases field by field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLeaseSnapshot {
    /// `WorkspaceId` text.
    pub workspace_id: String,
    /// `RepositoryRef` text (provider-neutral, never a URL or credential).
    pub repository_ref: String,
    /// Owning `ExecutionId` text (fixed at allocation).
    pub execution_id: String,
    /// Immutable base commit OID text.
    pub base_revision: String,
    /// Full managed branch ref text.
    pub branch_ref: String,
    /// Opaque managed worktree reference text.
    pub worktree_ref: String,
    /// Lifecycle wire name from [`LeaseLifecycle::as_str`].
    pub lifecycle: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lease() -> WorkspaceLease {
        WorkspaceLease::allocate(
            WorkspaceId::new("ws-1").unwrap(),
            RepositoryRef::new("repo-local:runlith").unwrap(),
            ExecutionId::new("E-1").unwrap(),
            Revision::new("a".repeat(40)).unwrap(),
            BranchRef::new("refs/heads/agent/e-1").unwrap(),
            "managed-worktree-ref:ws-1",
        )
        .unwrap()
    }

    #[test]
    fn lifecycle_advances_only_forward_through_observed_gates() {
        let mut lease = lease();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
        // Ready requires the Allocated gate (observation happens outside).
        lease.mark_ready().unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Ready);
        assert!(lease.mark_ready().is_err());
        lease.request_cleanup().unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::CleanupPending);
        lease.mark_removed().unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Removed);
        assert!(lease.lifecycle().is_terminal());
    }

    #[test]
    fn owner_and_base_revision_are_fixed() {
        let lease = lease();
        assert_eq!(lease.owner().as_str(), "E-1");
        assert_eq!(lease.base_revision().as_str(), &"a".repeat(40));
        assert_eq!(lease.branch().as_str(), "refs/heads/agent/e-1");
    }

    #[test]
    fn lifecycle_parse_round_trips_and_rejects_task_states() {
        for lifecycle in [
            LeaseLifecycle::Allocated,
            LeaseLifecycle::Ready,
            LeaseLifecycle::CleanupPending,
            LeaseLifecycle::Removed,
        ] {
            assert_eq!(
                LeaseLifecycle::parse(lifecycle.as_str()).unwrap(),
                lifecycle
            );
        }
        // Task lifecycle names are a different vocabulary, never a lease marker.
        for bogus in ["", "READY ", "ready", "RUNNING", "COMPLETED", "ALLOCATED "] {
            assert!(
                LeaseLifecycle::parse(bogus).is_err(),
                "{bogus:?} must not decode as a lease lifecycle"
            );
        }
    }

    #[test]
    fn snapshot_restore_round_trips_every_lifecycle() {
        for (advance, expected) in [
            (0, LeaseLifecycle::Allocated),
            (1, LeaseLifecycle::Ready),
            (2, LeaseLifecycle::CleanupPending),
            (3, LeaseLifecycle::Removed),
        ] {
            let mut lease = lease();
            for _ in 0..advance {
                match lease.lifecycle() {
                    LeaseLifecycle::Allocated => lease.mark_ready().unwrap(),
                    LeaseLifecycle::Ready => lease.request_cleanup().unwrap(),
                    LeaseLifecycle::CleanupPending => lease.mark_removed().unwrap(),
                    LeaseLifecycle::Removed => unreachable!(),
                }
            }
            let restored = WorkspaceLease::restore(lease.snapshot()).unwrap();
            assert_eq!(restored, lease);
            assert_eq!(restored.lifecycle(), expected);
        }
    }

    #[test]
    fn restore_rejects_every_invalid_field() {
        let good = lease().snapshot();
        // Each corrupted field fails through domain validation, never as a default.
        for mutate in [
            (|s: &mut WorkspaceLeaseSnapshot| s.workspace_id = "  ".to_owned()),
            (|s: &mut WorkspaceLeaseSnapshot| s.repository_ref = String::new()),
            (|s: &mut WorkspaceLeaseSnapshot| s.execution_id = "x".repeat(300)),
            (|s: &mut WorkspaceLeaseSnapshot| s.base_revision = "main".to_owned()),
            (|s: &mut WorkspaceLeaseSnapshot| s.branch_ref = "main".to_owned()),
            (|s: &mut WorkspaceLeaseSnapshot| s.worktree_ref = String::new()),
            (|s: &mut WorkspaceLeaseSnapshot| s.lifecycle = "RUNNING".to_owned()),
            (|s: &mut WorkspaceLeaseSnapshot| s.lifecycle = String::new()),
        ] {
            let mut bad = good.clone();
            mutate(&mut bad);
            assert!(
                WorkspaceLease::restore(bad).is_err(),
                "corrupt snapshot must not restore"
            );
        }
    }

    #[test]
    fn blank_worktree_ref_is_rejected() {
        assert!(WorkspaceLease::allocate(
            WorkspaceId::new("ws-1").unwrap(),
            RepositoryRef::new("r").unwrap(),
            ExecutionId::new("E-1").unwrap(),
            Revision::new("a".repeat(40)).unwrap(),
            BranchRef::new("refs/heads/b").unwrap(),
            "   ",
        )
        .is_err());
    }
}
