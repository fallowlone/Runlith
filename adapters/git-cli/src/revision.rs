//! [`RepoRevisionPort`](repo_finder::RepoRevisionPort) backend over the
//! local `git` executable (TASK-0147, G9).
//!
//! Binds a repository root to its `HEAD` commit OID plus a dirtiness
//! bit derived from `status --porcelain`. Unbindable roots are
//! [`FinderError::NotARepository`](repo_finder::FinderError); bound
//! roots whose revision cannot resolve (a fresh `git init` with no
//! commits, a broken backend) are
//! [`FinderError::RevisionUnresolvable`](repo_finder::FinderError).
//! Detached `HEAD` still binds: `HEAD` names a commit, and the
//! finder reads history, not branch placement.

use repo_finder::{FinderError, RepoProvenance, RepoRevisionPort, RepoRoot, Revision};
use workspace::{GitPort, WorkspaceFailure, WorktreeStatus};

use crate::GitCli;

impl RepoRevisionPort for GitCli {
    fn resolve(&self, root: &RepoRoot) -> Result<RepoProvenance, FinderError> {
        let path = root
            .as_path()
            .to_str()
            .ok_or_else(|| FinderError::NotARepository {
                root: root.as_path().to_path_buf(),
            })?;
        let anchor = self
            .establish_trusted_anchor(path)
            .map_err(|failure| map_failure(&failure, root))?;
        let status = crate::observe::worktree_status(self.exe(), path, &anchor)
            .map_err(|failure| map_failure(&failure, root))?;
        let oid = crate::observe::resolve_revision(self.exe(), path, "HEAD", &anchor)
            .map_err(|failure| map_failure(&failure, root))?;
        let revision =
            Revision::new(oid.as_str()).map_err(|_| FinderError::RevisionUnresolvable {
                detail: "git-cli: unexpected revision shape".to_string(),
            })?;
        Ok(RepoProvenance {
            root: root.clone(),
            revision,
            dirty: matches!(
                status,
                WorktreeStatus::DirtyTracked | WorktreeStatus::DirtyUntracked
            ),
        })
    }
}

fn map_failure(failure: &WorkspaceFailure, root: &RepoRoot) -> FinderError {
    match failure {
        WorkspaceFailure::NotRepository { .. } => FinderError::NotARepository {
            root: root.as_path().to_path_buf(),
        },
        // git-cli bounds detail at construction (500 chars); the
        // Display rendering stays far under the seam bound.
        other => FinderError::RevisionUnresolvable {
            detail: other.to_string(),
        },
    }
}
