//! Safe non-forced worktree and managed-branch removal (TASK-0043).
//!
//! Conservative teardown only:
//!
//! - `remove_worktree`: `git worktree remove <path>` without `--force`.
//!   Dirty, locked, or otherwise unremovable worktrees fail instead of being
//!   destroyed. Ownership is re-validated before mutation (expected path is
//!   registered, attached branch equals the lease branch); disagreement fails
//!   closed as `OwnershipMismatch` with no deletion. Already-absent paths
//!   report `WorktreeMissing` (registration without repo context cannot prove
//!   full absence); the service decides legitimate reconciliation vs drift
//!   from its lifecycle plus branch observations. No `prune`, no `reset`,
//!   no `clean`, no stash, no commit.
//! - `remove_branch`: `git branch -d <short>` without `-D`. Unmerged
//!   branches (unique commits), branches checked out in another worktree,
//!   and the protected/default branch are preserved via typed refusal —
//!   never force-deleted. Already-absent refs report `AlreadyAbsent`.
//!
//! All Git invocation is direct argv (no shell), non-interactive, local
//! only, with bounded diagnostics. Destructive decisions never rest on
//! localized stderr prose: preflight observations plus exit status plus
//! postcondition re-observation decide; stderr travels only as bounded
//! detail.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use workspace::{BranchRef, TrustedCommonGit, WorkspaceFailure, WorkspaceLease, WorktreeStatus};

use crate::observe::{self, worktree_registered};
use crate::runner::{
    absolutize, backend_failure, bounded_detail, run_git, stderr_excerpt,
    validate_repository_boundary,
};

/// Removes the lease's worktree registration without force.
pub fn remove_worktree(
    executable: &OsStr,
    lease: &WorkspaceLease,
    worktree_path: &str,
    expected: &TrustedCommonGit,
) -> Result<(), WorkspaceFailure> {
    let target_abs =
        absolutize(worktree_path).map_err(|_| WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(format!(
                "git-cli: worktree path unusable for lease {}; refusing",
                lease.id()
            )),
        })?;
    // Filesystem absence: report missing (the service reconciles via its
    // lifecycle + branch state; only the service knows cleanup intent).
    if target_abs.symlink_metadata().is_err() {
        return Err(WorkspaceFailure::WorktreeMissing {
            detail: bounded_detail(format!(
                "git-cli: worktree path absent for lease {}; no removal attempted",
                lease.id()
            )),
        });
    }
    if target_abs.is_file() {
        return Err(WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(format!(
                "git-cli: worktree path is a file for lease {}; refusing",
                lease.id()
            )),
        });
    }
    // Same repository-identity check as every other Git entry point: this
    // function goes on to call Git directly (`attached_branch`,
    // `discover_primary`, the removal mutation itself) against this path.
    validate_repository_boundary(executable, &target_abs, expected)?;
    // Dirtiness pre-check: ordinary cleanup never destroys uncommitted or
    // untracked work. Git's own non-forced refusal below is the second
    // boundary (TOCTOU); this pre-check gives the typed dirty signal.
    match observe::worktree_status(executable, worktree_path, expected)? {
        WorktreeStatus::Clean => {}
        WorktreeStatus::DirtyTracked | WorktreeStatus::DirtyUntracked => {
            return Err(WorkspaceFailure::WorktreeDirty {
                detail: bounded_detail(format!(
                    "git-cli: worktree holds uncommitted work for lease {}; refusing removal",
                    lease.id()
                )),
            });
        }
        WorktreeStatus::Missing => {
            return Err(WorkspaceFailure::WorktreeMissing {
                detail: bounded_detail(format!(
                    "git-cli: worktree path absent for lease {}; no removal attempted",
                    lease.id()
                )),
            });
        }
        WorktreeStatus::Invalid => {
            return Err(WorkspaceFailure::CheckoutFailed {
                detail: bounded_detail(format!(
                    "git-cli: worktree unusable for lease {}; refusing removal",
                    lease.id()
                )),
            });
        }
    }
    // Ownership: the attached branch must equal the recorded managed branch.
    // Managed worktrees live on managed branches; detached or unexpected
    // branches fail closed with no deletion.
    let attached = attached_branch(executable, &target_abs)?;
    if attached != lease.branch().as_str() {
        return Err(WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(format!(
                "git-cli: worktree attached at {} for lease {}, expected {}; refusing",
                attached,
                lease.id(),
                lease.branch()
            )),
        });
    }
    // Ownership: the expected path must be registered in `worktree list`.
    // Canonical comparison so symlinked prefixes cannot hide disagreement.
    // Stale Filesystem-vs-registration drift without repo context cannot be
    // repaired here (no prune); report explicitly instead.
    let primary = discover_primary(executable, &target_abs, &target_abs)?;
    if !worktree_registered(executable, &primary, &target_abs)? {
        return Err(WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(format!(
                "git-cli: worktree path not registered for lease {}; refusing",
                lease.id()
            )),
        });
    }
    // The single non-forced mutation, run from the primary checkout so the
    // removed directory is never the subprocess cwd.
    let target_arg = target_abs.clone().into_os_string();
    let output = run_git(
        executable,
        &primary,
        &[
            OsStr::new("worktree"),
            OsStr::new("remove"),
            target_arg.as_os_str(),
        ],
    )?;
    if !output.status.success() {
        return Err(WorkspaceFailure::CleanupUnsafe {
            detail: bounded_detail(format!(
                "git-cli: safe worktree removal refused for lease {} ({}); work preserved",
                lease.id(),
                stderr_excerpt(&output),
            )),
        });
    }
    // Postconditions: path gone and registration gone.
    if target_abs.symlink_metadata().is_ok() {
        return Err(backend_failure(format!(
            "git-cli: worktree path still present after removal for lease {}",
            lease.id()
        )));
    }
    if worktree_registered(executable, &primary, &target_abs)? {
        return Err(backend_failure(format!(
            "git-cli: worktree still registered after removal for lease {}",
            lease.id()
        )));
    }
    Ok(())
}

/// Removes the managed branch ref without force (`branch -d` only).
pub fn remove_branch(
    executable: &OsStr,
    repository_path: &str,
    branch: &BranchRef,
    expected: &TrustedCommonGit,
) -> Result<(), WorkspaceFailure> {
    if branch.short_name().starts_with('-') {
        return Err(backend_failure(format!(
            "git-cli: managed branch short name starts with '-'; refusing argv-unsafe ref {branch}"
        )));
    }
    let abs = absolutize(repository_path).map_err(|_| WorkspaceFailure::NotRepository {
        detail: bounded_detail("git-cli: invalid repository path for branch removal".to_owned()),
    })?;
    let root = observe::repository_root(
        executable,
        abs.to_str().unwrap_or(repository_path),
        expected,
    )?;
    let root_str = root.to_str().ok_or_else(|| {
        backend_failure("git-cli: repository root is not UTF-8; refusing branch removal")
    })?;
    // Idempotent absent: already gone with consistent ownership view.
    if !observe::branch_exists(executable, root_str, branch, expected)? {
        return Err(WorkspaceFailure::AlreadyAbsent);
    }
    // Protected/default branch is never deleted.
    match observe::default_branch(executable, root_str, expected) {
        Ok(default_ref) => {
            if default_ref == branch.as_str() {
                return Err(WorkspaceFailure::CleanupUnsafe {
                    detail: bounded_detail(format!(
                        "git-cli: refusing to delete the protected/default branch {branch}"
                    )),
                });
            }
        }
        Err(WorkspaceFailure::GitBackendFailure { .. }) => {}
        Err(other) => return Err(other),
    }
    let output = run_git(
        executable,
        &root,
        &[
            OsStr::new("branch"),
            OsStr::new("-d"),
            OsStr::new(branch.short_name()),
        ],
    )?;
    if !output.status.success() {
        // Non-forced refusal (unmerged unique commits, branch checked out
        // elsewhere): preserve the branch, fail closed. Never escalate to
        // `-D`. Stderr is diagnostic detail only; the decision is the
        // non-zero exit plus the preserved postcondition below.
        return Err(WorkspaceFailure::CleanupUnsafe {
            detail: bounded_detail(format!(
                "git-cli: safe branch deletion refused for {branch} ({}); branch preserved",
                stderr_excerpt(&output),
            )),
        });
    }
    if observe::branch_exists(executable, root_str, branch, expected)? {
        return Err(backend_failure(format!(
            "git-cli: branch still present after deletion for {branch}"
        )));
    }
    Ok(())
}

/// Reads the attached `HEAD` ref inside the worktree (fails closed when
/// detached or unreadable).
fn attached_branch(executable: &OsStr, worktree: &Path) -> Result<String, WorkspaceFailure> {
    let output = run_git(executable, worktree, &["symbolic-ref", "--quiet", "HEAD"])?;
    if !output.status.success() {
        return Err(WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(
                "git-cli: worktree HEAD is detached or unreadable; refusing removal".to_owned(),
            ),
        });
    }
    let head_ref = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !head_ref.starts_with("refs/heads/") || head_ref.len() <= "refs/heads/".len() {
        return Err(WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(
                "git-cli: worktree HEAD ref is outside refs/heads; refusing removal".to_owned(),
            ),
        });
    }
    Ok(head_ref)
}

/// Discovers the primary checkout path from `worktree list`, run with cwd
/// inside the existing worktree. The main worktree is listed first; entries
/// whose `.git` is a directory confirm it (linked worktrees carry `.git`
/// files). Falls back to the first listed entry.
fn discover_primary(
    executable: &OsStr,
    cwd: &Path,
    target: &Path,
) -> Result<PathBuf, WorkspaceFailure> {
    let output = run_git(executable, cwd, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(backend_failure(format!(
            "git-cli: worktree list failed ({})",
            stderr_excerpt(&output)
        )));
    }
    let mut first: Option<PathBuf> = None;
    let mut primary: Option<PathBuf> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(entry) = line.strip_prefix("worktree ") {
            let entry_path = PathBuf::from(entry);
            if first.is_none() {
                first = Some(entry_path.clone());
            }
            if entry_path.join(".git").is_dir() && primary.is_none() {
                primary = Some(entry_path.clone());
            }
            let _ = target;
        }
    }
    if let Some(primary) = primary {
        return Ok(primary);
    }
    first.ok_or_else(|| backend_failure("git-cli: worktree list is empty; refusing removal"))
}
