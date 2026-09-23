//! Small Git capability port for the future concrete adapter.
//!
//! One narrow operation per capability — never a generic
//! `run_git(args: Vec<String>)` domain API. Raw command execution lives
//! only inside the adapter, invoked via argv arrays. All operations are
//! synchronous observations or bounded mutations returning
//! [`WorkspaceFailure`]; nothing here spawns a process.

use agent_runtime::ExecutionId;

use crate::failure::WorkspaceFailure;
use crate::ids::{BranchRef, RepositoryRef, Revision};
use crate::lease::WorkspaceLease;
use crate::status::WorktreeStatus;
use crate::trusted::TrustedCommonGit;

/// Upper bound for structured diff entries per evidence layer. Raw patch
/// text is never returned; large workspaces truncate with an explicit flag
/// instead of becoming an unbounded in-memory payload.
pub const MAX_DIFF_ENTRIES: usize = 512;

/// Maximum characters kept in any diff summary handed to higher layers.
pub const MAX_DIFF_SUMMARY_CHARS: usize = 500;

/// One file-level change class for diff evidence. Covers the material
/// workspace change classes the G4 contract requires visible: tracked
/// modifications (staged or unstaged), new/untracked files, deletions, and
/// renames where Git reports them. Not a patch-analysis model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffChangeKind {
    /// New tracked file (committed `A` or staged addition).
    Added,
    /// Tracked content changed (staged or unstaged `M`/`T`).
    Modified,
    /// Tracked file deleted.
    Deleted,
    /// Git-reported rename (new path carried in the entry).
    Renamed,
    /// Present but untracked (working layer only; always dirt).
    Untracked,
}

impl DiffChangeKind {
    /// Stable machine-readable name (never Git prose).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::Untracked => "untracked",
        }
    }
}

/// One bounded file entry inside a diff evidence layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    /// Repository-relative path as reported by Git (NUL-safe decoding via
    /// `-z`; lossy UTF-8 where the filesystem is non-UTF-8). For renames
    /// this is the new path.
    pub path: String,
    /// What changed.
    pub kind: DiffChangeKind,
}

/// Diff evidence layers for one workspace, all baselined at the recorded
/// base revision. Evidence for later verification — never completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeDiff {
    /// Recorded base revision the comparison starts from.
    pub base_revision: Revision,
    /// Workspace HEAD commit OID at inspection time, if resolvable.
    pub head_revision: Option<Revision>,
    /// Whether working-tree/index modifications were observed.
    pub has_working_tree_changes: bool,
    /// Live status observed alongside the diff (never persisted).
    pub status: WorktreeStatus,
    /// Committed file changes `base..HEAD` (`--name-status`, renames where
    /// Git reports them). Empty when HEAD equals the base.
    pub committed: Vec<DiffEntry>,
    /// Working-tree/index file changes (`status --porcelain`, untracked
    /// included). Empty when clean.
    pub working: Vec<DiffEntry>,
    /// True when either layer exceeded [`MAX_DIFF_ENTRIES`] and was
    /// truncated (explicit, never silent).
    pub truncated: bool,
}

/// Bounded diff handle returned by [`GitPort::compute_diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEvidence {
    /// The diff that was computed.
    pub diff: WorktreeDiff,
    /// Deterministic bounded summary (never raw file dumps).
    pub summary: String,
}

/// Exact bounded Git metadata needed by G8 `GIT_STATE` capture. Working-tree
/// file contents are captured separately by the filesystem observer; this
/// value owns the Git-only facts that filesystem traversal cannot prove:
/// exact HEAD/tree identity and the index manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStateSnapshot {
    pub head_revision: Revision,
    pub head_tree_oid: String,
    pub index_manifest: Vec<u8>,
}

/// Provider-neutral Git backend capabilities behind one narrow port.
///
/// The consumer (later G4 application services) owns orchestration and
/// durability; the adapter translates to Git mechanics and back, and it
/// must never mutate Task lifecycle, rewrite unowned refs, or silently
/// destroy dirty work.
pub trait GitPort {
    /// Opaque backend identity for diagnostics (never branched on).
    fn backend_ref(&self) -> &'static str;

    /// Captures the trusted common-Git anchor from trusted admission state.
    ///
    /// Sole anchor-free path, admission only (trusted-deposit before provider
    /// execution). Bare → `BareRepositoryUnsupported`, non-repo →
    /// `NotRepository`; symlink/canonicalize/config failures →
    /// `GitBackendFailure`.
    fn establish_trusted_anchor(
        &self,
        trusted_repository_path: &str,
    ) -> Result<TrustedCommonGit, WorkspaceFailure>;

    /// Whether `path` resolves inside a Git working repository.
    fn is_repository(
        &self,
        path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure>;

    /// Resolves the repository default branch from metadata (never by
    /// assuming `main`/`master`). Returns the full ref name.
    fn default_branch(
        &self,
        repository_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<String, WorkspaceFailure>;

    /// Resolves `rev` to an immutable commit OID ([`Revision`]).
    fn resolve_revision(
        &self,
        repository_path: &str,
        rev: &str,
        expected: &TrustedCommonGit,
    ) -> Result<Revision, WorkspaceFailure>;

    /// Creates an isolated worktree + managed branch for `lease` and
    /// verifies mutual consistency before returning. Records nothing
    /// durable itself — the caller reconciles.
    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure>;

    /// Live-observes worktree status ([`WorktreeStatus`]).
    fn worktree_status(
        &self,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<WorktreeStatus, WorkspaceFailure>;

    /// Computes the diff baselined at the lease's recorded base revision.
    fn compute_diff(
        &self,
        lease: &WorkspaceLease,
        expected: &TrustedCommonGit,
    ) -> Result<DiffEvidence, WorkspaceFailure>;

    /// Captures exact read-only Git metadata through the backend's normal
    /// trusted-anchor and hardened-runner boundary. Backends that cannot
    /// prove it fail closed.
    fn git_state_snapshot(
        &self,
        _worktree_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<GitStateSnapshot, WorkspaceFailure> {
        Err(WorkspaceFailure::GitBackendFailure {
            detail: "git backend cannot capture exact Git state".to_owned(),
        })
    }

    /// Removes the worktree registration (and only after the caller has
    /// enforced cleanup safety). Idempotent for already-absent owned
    /// workspaces only where ownership stays consistent.
    fn remove_worktree(
        &self,
        lease: &WorkspaceLease,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure>;

    /// Removes the managed branch ref after the worktree is gone, using
    /// only non-forced deletion (`branch -d`). Preserves branches with
    /// unique unmerged commits (returns `CleanupUnsafe`, branch survives),
    /// never deletes the protected/default branch, and never force-deletes
    /// (`-D`). Idempotent for already-absent refs only where the caller has
    /// already proven ownership (`AlreadyAbsent`).
    fn remove_branch(
        &self,
        repository_path: &str,
        branch: &BranchRef,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure>;

    /// Whether `branch` exists as a ref in the repository.
    fn branch_exists(
        &self,
        repository_path: &str,
        branch: &BranchRef,
        expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure>;

    /// Whether `worktree_path` is registered in `repository_path`'s
    /// `worktree list` (canonical comparison, symlinked prefixes resolved).
    ///
    /// Lease-to-repository binding (TASK-0046, G4 finding F1): destructive
    /// paths must prove the caller-supplied repository actually owns the
    /// lease's worktree before touching branch state, so a wrong-but-valid
    /// repository path fails closed as `OwnershipMismatch` with nothing
    /// mutated instead of finalizing success while the managed branch
    /// leaks in the true repository. Backend errors fail closed (`Err`,
    /// never assumed-absent); only a positively observed registration
    /// returns `Ok(true)`.
    fn is_registered_worktree(
        &self,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure>;

    /// Returns the registered worktree currently holding `branch`, if any.
    /// Backends that cannot prove absence fail closed.
    fn worktree_for_branch(
        &self,
        _repository_path: &str,
        _branch: &BranchRef,
        _expected: &TrustedCommonGit,
    ) -> Result<Option<String>, WorkspaceFailure> {
        Err(WorkspaceFailure::GitBackendFailure {
            detail: "git backend cannot prove branch worktree ownership".to_owned(),
        })
    }

    /// Continues materialization using an already-existing managed branch.
    /// The caller proves durable ownership; the adapter must independently
    /// verify exact base/branch/path postconditions. Unsupported backends
    /// fail closed instead of replaying branch creation.
    fn attach_worktree(
        &self,
        _lease: &WorkspaceLease,
        _repository_path: &str,
        _worktree_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        Err(WorkspaceFailure::GitBackendFailure {
            detail: "git backend cannot continue an existing managed branch".to_owned(),
        })
    }

    /// Whether `owner` still shows live activity for `repository`.
    /// Contractual in G4 (full Runner process authority arrives later and
    /// is never faked): adapters report what they can observe honestly.
    /// Since G5 a real provider process tracker may supply this through a
    /// consumer-owned [`crate::OwnerLiveness`] source wired at the
    /// composition root; unwired backends keep reporting `false`.
    fn owner_is_live(
        &self,
        repository: &RepositoryRef,
        owner: &ExecutionId,
    ) -> Result<bool, WorkspaceFailure>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WorkspaceId;

    /// Contract stub proving the port is implementable without process,
    /// filesystem, or storage surface.
    struct StubPort;

    impl GitPort for StubPort {
        fn backend_ref(&self) -> &'static str {
            "stub-git-port"
        }

        fn establish_trusted_anchor(
            &self,
            _trusted_repository_path: &str,
        ) -> Result<TrustedCommonGit, WorkspaceFailure> {
            Err(WorkspaceFailure::GitBackendFailure {
                detail: "stub cannot establish trusted anchor".to_owned(),
            })
        }

        fn is_repository(
            &self,
            _path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            Ok(true)
        }

        fn default_branch(
            &self,
            _repository_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<String, WorkspaceFailure> {
            Ok("refs/heads/trunk".to_owned())
        }

        fn resolve_revision(
            &self,
            _repository_path: &str,
            _rev: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<Revision, WorkspaceFailure> {
            Revision::new("a".repeat(40)).map_err(|e| WorkspaceFailure::GitBackendFailure {
                detail: e.to_string(),
            })
        }

        fn create_worktree(
            &self,
            _lease: &WorkspaceLease,
            _repository_path: &str,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<(), WorkspaceFailure> {
            Ok(())
        }

        fn worktree_status(
            &self,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<WorktreeStatus, WorkspaceFailure> {
            Ok(WorktreeStatus::Clean)
        }

        fn compute_diff(
            &self,
            lease: &WorkspaceLease,
            _expected: &TrustedCommonGit,
        ) -> Result<DiffEvidence, WorkspaceFailure> {
            Ok(DiffEvidence {
                diff: WorktreeDiff {
                    base_revision: lease.base_revision().clone(),
                    head_revision: None,
                    has_working_tree_changes: false,
                    status: WorktreeStatus::Clean,
                    committed: Vec::new(),
                    working: Vec::new(),
                    truncated: false,
                },
                summary: "no changes".to_owned(),
            })
        }

        fn remove_worktree(
            &self,
            _lease: &WorkspaceLease,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<(), WorkspaceFailure> {
            Ok(())
        }

        fn remove_branch(
            &self,
            _repository_path: &str,
            _branch: &BranchRef,
            _expected: &TrustedCommonGit,
        ) -> Result<(), WorkspaceFailure> {
            Ok(())
        }

        fn branch_exists(
            &self,
            _repository_path: &str,
            _branch: &BranchRef,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            Ok(false)
        }

        fn is_registered_worktree(
            &self,
            _repository_path: &str,
            _worktree_path: &str,
            _expected: &TrustedCommonGit,
        ) -> Result<bool, WorkspaceFailure> {
            Ok(false)
        }

        fn owner_is_live(
            &self,
            _repository: &RepositoryRef,
            _owner: &ExecutionId,
        ) -> Result<bool, WorkspaceFailure> {
            Ok(false)
        }
    }

    #[test]
    fn port_is_implementable_with_no_process_or_storage() {
        let port = StubPort;
        assert_eq!(port.backend_ref(), "stub-git-port");
        // Default branch comes from metadata, never assumed.
        let anchor = TrustedCommonGit::restore(
            std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            port.default_branch("/repo", &anchor).unwrap(),
            "refs/heads/trunk"
        );
        let lease = WorkspaceLease::allocate(
            WorkspaceId::new("ws-1").unwrap(),
            RepositoryRef::new("repo-local:x").unwrap(),
            ExecutionId::new("E-1").unwrap(),
            Revision::new("a".repeat(40)).unwrap(),
            BranchRef::new("refs/heads/agent/e-1").unwrap(),
            "ref:ws-1",
        )
        .unwrap();
        let evidence = port.compute_diff(&lease, &anchor).unwrap();
        assert_eq!(evidence.diff.base_revision, lease.base_revision().clone());
    }
}
