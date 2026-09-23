//! Concrete [`workspace::GitPort`] backend over the local `git` executable.
//!
//! TASK-0041 implements the first Git backend behind the
//! `crates/workspace` contract: repository inspection, exact revision
//! resolution, metadata-derived default/protected-branch observation,
//! managed worktree creation, and post-create verification. TASK-0042 adds
//! durable ownership above it (the adapter still stores nothing).
//! TASK-0043 implements diff observation plus safe non-forced removal.
//!
//! Authority: ADR-0008, ADR-0023, `docs/architecture/workspace.md`.
//! Scope fences (all `DEFERRED_BY_DESIGN` unless noted):
//!
//! - no persistence, no Event Journal writes (TASK-0042 service layer);
//! - no remote operations, no merge/rebase/commit/push;
//! - no orchestration integration;
//! - removal is non-forced only: no `worktree remove --force`, no
//!   `branch -D`, no `reset --hard`, no `clean`, no stash, no prune.
//!
//! Trust boundary: Git worktree isolation is Git-state isolation only, never
//! an OS sandbox. Repository-local hooks, filters, or config may execute as
//! part of normal Git operation; hostile-repository hardening is
//! `DEFERRED_BY_DESIGN → G6`. This adapter keeps Git non-interactive and
//! argv-only but claims no sandbox property.

use std::ffi::{OsStr, OsString};
use std::sync::Arc;

use workspace::{
    BranchRef, DiffEvidence, ExecutionId, GitPort, OwnerLiveness, RepositoryRef, Revision,
    TrustedCommonGit, WorkspaceFailure, WorkspaceLease, WorktreeStatus,
};

mod cleanup;
mod create;
mod diff;
mod observe;
mod runner;

pub use runner::{BACKEND_REF, MAX_DETAIL_CHARS, MAX_GIT_OUTPUT_BYTES};

/// `git` CLI implementation of [`workspace::GitPort`].
///
/// Construct with [`GitCli::new`] (resolves `git` via `PATH`);
/// [`GitCli::with_executable`] exists so tests can point at a missing binary
/// and prove explicit [`WorkspaceFailure::GitBackendFailure`] instead of a
/// panic or silent fallback. [`GitCli::with_liveness`] attaches a
/// provider-neutral [`OwnerLiveness`] source (wired by the composition root,
/// e.g. a real provider's process tracker); without one `owner_is_live`
/// honestly reports no observed live activity, exactly the G4 posture.
#[derive(Clone)]
pub struct GitCli {
    executable: OsString,
    liveness: Option<Arc<dyn OwnerLiveness>>,
}

impl std::fmt::Debug for GitCli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitCli")
            .field("executable", &self.executable)
            .field("liveness", &self.liveness.is_some())
            .finish()
    }
}

impl Default for GitCli {
    fn default() -> Self {
        Self::new()
    }
}

impl GitCli {
    /// Adapter using `git` from `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            executable: OsString::from("git"),
            liveness: None,
        }
    }

    /// Adapter using an explicit executable path (testing only).
    #[must_use]
    pub fn with_executable(executable: impl Into<OsString>) -> Self {
        Self {
            executable: executable.into(),
            liveness: None,
        }
    }

    /// Adapter with a provider-neutral owner-liveness source.
    ///
    /// Composition-root wiring only: the source must derive liveness from
    /// real process/handle ownership (never name-guessing, never constant
    /// `true`). Only the liveness answer changes — every other operation
    /// behaves exactly as the unwired adapter.
    #[must_use]
    pub fn with_liveness(mut self, liveness: Arc<dyn OwnerLiveness>) -> Self {
        self.liveness = Some(liveness);
        self
    }

    fn exe(&self) -> &OsStr {
        &self.executable
    }
}

impl GitPort for GitCli {
    fn backend_ref(&self) -> &'static str {
        BACKEND_REF
    }

    fn establish_trusted_anchor(
        &self,
        trusted_repository_path: &str,
    ) -> Result<TrustedCommonGit, WorkspaceFailure> {
        let abs = match runner::absolutize(trusted_repository_path) {
            Ok(abs) => abs,
            Err(_) => {
                return Err(WorkspaceFailure::NotRepository {
                    detail: "git-cli: invalid repository path".to_owned(),
                });
            }
        };
        runner::establish_trusted_anchor(self.exe(), &abs)
    }

    fn is_repository(
        &self,
        path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure> {
        observe::is_repository(self.exe(), path, expected)
    }

    fn default_branch(
        &self,
        repository_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<String, WorkspaceFailure> {
        observe::default_branch(self.exe(), repository_path, expected)
    }

    fn resolve_revision(
        &self,
        repository_path: &str,
        rev: &str,
        expected: &TrustedCommonGit,
    ) -> Result<Revision, WorkspaceFailure> {
        observe::resolve_revision(self.exe(), repository_path, rev, expected)
    }

    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        create::create_worktree(self.exe(), lease, repository_path, worktree_path, expected)
    }

    fn worktree_status(
        &self,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<WorktreeStatus, WorkspaceFailure> {
        observe::worktree_status(self.exe(), worktree_path, expected)
    }

    /// Read-only diff baselined at the lease's recorded base revision
    /// (ADR-0023 §6): HEAD history plus working-tree/index layers. Never
    /// mutates Git; bounded structured entries, no raw patch text.
    fn compute_diff(
        &self,
        lease: &WorkspaceLease,
        expected: &TrustedCommonGit,
    ) -> Result<DiffEvidence, WorkspaceFailure> {
        diff::compute_diff(self.exe(), lease, expected)
    }

    fn git_state_snapshot(
        &self,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<workspace::GitStateSnapshot, WorkspaceFailure> {
        observe::git_state_snapshot(self.exe(), worktree_path, expected)
    }

    /// Safe non-forced worktree removal (no `--force`). Dirty, detached,
    /// or ownership-inconsistent worktrees fail closed with work preserved.
    fn remove_worktree(
        &self,
        lease: &WorkspaceLease,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        cleanup::remove_worktree(self.exe(), lease, worktree_path, expected)
    }

    /// Safe non-forced branch deletion (`branch -d` only, never `-D`).
    /// Unmerged branches and the protected/default branch survive.
    fn remove_branch(
        &self,
        repository_path: &str,
        branch: &BranchRef,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        cleanup::remove_branch(self.exe(), repository_path, branch, expected)
    }

    fn branch_exists(
        &self,
        repository_path: &str,
        branch: &BranchRef,
        expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure> {
        observe::branch_exists(self.exe(), repository_path, branch, expected)
    }

    /// Lease-to-repository binding probe (TASK-0046, G4 finding F1): the
    /// caller-supplied repository must positively register the lease's
    /// worktree before destructive paths touch branch state.
    fn is_registered_worktree(
        &self,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure> {
        observe::is_registered_worktree(self.exe(), repository_path, worktree_path, expected)
    }

    fn worktree_for_branch(
        &self,
        repository_path: &str,
        branch: &BranchRef,
        expected: &TrustedCommonGit,
    ) -> Result<Option<String>, WorkspaceFailure> {
        observe::worktree_for_branch(self.exe(), repository_path, branch, expected)
    }

    fn attach_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        create::attach_worktree(self.exe(), lease, repository_path, worktree_path, expected)
    }

    /// Owner liveness from the wired [`OwnerLiveness`] source when present
    /// (G5: a real provider process tracker supplied by the composition
    /// root); honestly `false` when unwired (the G4 posture — full Runner
    /// process authority arrives later and is never faked).
    fn owner_is_live(
        &self,
        _repository: &RepositoryRef,
        owner: &ExecutionId,
    ) -> Result<bool, WorkspaceFailure> {
        Ok(self
            .liveness
            .as_ref()
            .is_some_and(|live| live.is_live(owner)))
    }
}
