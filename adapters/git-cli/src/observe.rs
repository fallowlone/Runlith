//! Read-only [`workspace::GitPort`] operations: repository inspection,
//! revision resolution, default-branch observation, branch existence, and
//! live worktree status.
//!
//! Classification never depends on English Git stderr prose: every decision
//! below comes from preflight observations, process exit status, or plumbing
//! output (`rev-parse`, `symbolic-ref`, `show-ref`, `status --porcelain`).
//! Stderr travels only as bounded diagnostic detail.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use workspace::{BranchRef, WorkspaceFailure, WorktreeStatus};

use crate::runner::{
    absolutize, backend_failure, bounded_detail, classify_repository, run_git, run_git_text,
    stderr_excerpt, RepoClass,
};

/// Canonical metadata source for the protected/default branch.
///
/// ADR-0023 resolves the default branch from repository metadata, never by
/// string-matching `main`/`master` (the local current branch is not
/// automatically the default). This adapter reads exactly one metadata
/// fact — the `refs/remotes/origin/HEAD` symbolic ref — and refuses to guess
/// when it is absent. No `main`/`master` literal (or any other name
/// heuristic) exists anywhere in this adapter.
const ORIGIN_HEAD_REF: &str = "refs/remotes/origin/HEAD";

/// `true` if `path` resolves inside a Git working repository.
///
/// - working repository (root or any path inside it) → `Ok(true)`
/// - bare repository → `Err(BareRepositoryUnsupported)` (explicit kind)
/// - normal directory, file, or missing path → `Ok(false)`
/// - unusable Git backend → `Err(GitBackendFailure)`
pub fn is_repository(executable: &OsStr, path: &str) -> Result<bool, WorkspaceFailure> {
    let abs = match absolutize(path) {
        Ok(abs) => abs,
        Err(_) => return Ok(false),
    };
    match classify_repository(executable, &abs) {
        Ok(RepoClass::Normal { .. }) => Ok(true),
        Err(WorkspaceFailure::NotRepository { .. }) => Ok(false),
        Err(other) => Err(other),
    }
}

/// Resolves `repository_path` to its authoritative toplevel root.
///
/// Shares [`classify_repository`] semantics (bare → explicit failure,
/// non-repo → `NotRepository`).
pub fn repository_root(
    executable: &OsStr,
    repository_path: &str,
) -> Result<PathBuf, WorkspaceFailure> {
    let abs = absolutize(repository_path).map_err(|_| WorkspaceFailure::NotRepository {
        detail: bounded_detail("git-cli: invalid repository path".to_owned()),
    })?;
    match classify_repository(executable, &abs)? {
        RepoClass::Normal { root } => Ok(root),
    }
}

/// Resolves `rev` to an immutable commit OID ([`workspace::Revision`]).
///
/// Implemented as `rev-parse --verify --quiet <rev>^{commit}`: the
/// `^{commit}` suffix proves the input names a commit object (trees and
/// blobs fail), `--verify --quiet` reduces the outcome to exit status, and
/// the returned [`workspace::Revision`] carries the full OID — never `HEAD`,
/// a branch name, or a default-branch tip.
pub fn resolve_revision(
    executable: &OsStr,
    repository_path: &str,
    rev: &str,
) -> Result<workspace::Revision, WorkspaceFailure> {
    let trimmed = rev.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err(WorkspaceFailure::InvalidRevision {
            detail: bounded_detail("git-cli: blank revision cannot resolve to a commit".to_owned()),
        });
    }
    let abs = absolutize(repository_path).map_err(|_| WorkspaceFailure::NotRepository {
        detail: bounded_detail("git-cli: invalid repository path".to_owned()),
    })?;
    let root = match classify_repository(executable, &abs)? {
        RepoClass::Normal { root } => root,
    };
    let candidate = format!("{trimmed}^{{commit}}");
    let (output, oid) = run_git_text(
        executable,
        &root,
        &["rev-parse", "--verify", "--quiet", candidate.as_str()],
    )?;
    if !output.status.success() {
        return Err(WorkspaceFailure::InvalidRevision {
            detail: bounded_detail(format!(
                "git-cli: revision does not resolve to a commit: {trimmed}"
            )),
        });
    }
    workspace::Revision::new(oid.clone())
        .map_err(|_| backend_failure(format!("git-cli: unexpected non-OID from rev-parse: {oid}")))
}

/// Resolves the repository default branch from metadata.
///
/// Reads `refs/remotes/origin/HEAD` and returns the full ref it points at
/// (e.g. `refs/heads/trunk`). When that metadata is absent the default
/// branch is reported indeterminate via [`WorkspaceFailure::GitBackendFailure`];
/// the adapter never falls back to name heuristics.
pub fn default_branch(
    executable: &OsStr,
    repository_path: &str,
) -> Result<String, WorkspaceFailure> {
    let abs = absolutize(repository_path).map_err(|_| WorkspaceFailure::NotRepository {
        detail: bounded_detail("git-cli: invalid repository path".to_owned()),
    })?;
    let root = match classify_repository(executable, &abs)? {
        RepoClass::Normal { root } => root,
    };
    let (output, target) = run_git_text(
        executable,
        &root,
        &["symbolic-ref", "--quiet", ORIGIN_HEAD_REF],
    )?;
    if !output.status.success() {
        return Err(backend_failure(
            "git-cli: default branch indeterminate from repository metadata \
             (no refs/remotes/origin/HEAD); refusing to guess",
        ));
    }
    if !target.starts_with("refs/heads/") || target.len() <= "refs/heads/".len() {
        return Err(backend_failure(format!(
            "git-cli: origin HEAD points outside refs/heads: {target}"
        )));
    }
    Ok(target)
}

/// Whether `branch` exists as a ref in the repository.
///
/// `show-ref --verify --quiet` reduces existence to exit status: `0` →
/// exists, `1` → absent, anything else → backend failure (a corrupt backend
/// must not read as "absent").
pub fn branch_exists(
    executable: &OsStr,
    repository_path: &str,
    branch: &BranchRef,
) -> Result<bool, WorkspaceFailure> {
    let abs = absolutize(repository_path).map_err(|_| WorkspaceFailure::NotRepository {
        detail: bounded_detail("git-cli: invalid repository path".to_owned()),
    })?;
    let root = match classify_repository(executable, &abs)? {
        RepoClass::Normal { root } => root,
    };
    let output = run_git(
        executable,
        &root,
        &["show-ref", "--verify", "--quiet", branch.as_str()],
    )?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(backend_failure(format!(
            "git-cli: cannot determine branch existence for {}",
            branch
        ))),
    }
}

/// Whether `worktree_path` is registered in the `worktree list` of the
/// repository at `repository_path`.
///
/// Canonical comparison: both sides are canonicalized where possible so
/// symlinked prefixes cannot hide disagreement; an uncanonicalizable
/// target falls back to the lexical spelling (same policy as removal
/// ownership). Any backend error fails closed as `Err` — absence is only
/// ever reported from a positively observed list that lacks the entry.
/// Both paths are resolved through [`classify_repository`], so bare or
/// non-repository inputs fail as their typed classes, never as `false`.
pub fn is_registered_worktree(
    executable: &OsStr,
    repository_path: &str,
    worktree_path: &str,
) -> Result<bool, WorkspaceFailure> {
    let repo_abs = absolutize(repository_path).map_err(|_| WorkspaceFailure::NotRepository {
        detail: bounded_detail("git-cli: invalid repository path".to_owned()),
    })?;
    let root = match classify_repository(executable, &repo_abs)? {
        RepoClass::Normal { root } => root,
    };
    let target_abs =
        absolutize(worktree_path).map_err(|_| WorkspaceFailure::OwnershipMismatch {
            detail: bounded_detail(
                "git-cli: worktree path unusable for registration check; refusing".to_owned(),
            ),
        })?;
    worktree_registered(executable, &root, &target_abs)
}

/// Whether `target` is registered in `worktree list --porcelain` run from
/// `primary`. Canonical comparison so symlinked prefixes cannot hide
/// disagreement.
pub fn worktree_registered(
    executable: &OsStr,
    primary: &Path,
    target: &Path,
) -> Result<bool, WorkspaceFailure> {
    let output = run_git(executable, primary, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(backend_failure(format!(
            "git-cli: worktree list failed ({})",
            stderr_excerpt(&output)
        )));
    }
    let target_canon = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(entry) = line.strip_prefix("worktree ") {
            if entry == target.to_string_lossy().as_ref() {
                return Ok(true);
            }
            let entry_path = PathBuf::from(entry);
            if std::fs::canonicalize(&entry_path).is_ok_and(|c| c == target_canon) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Returns the registered worktree path holding `branch`, if any.
/// Uses Git porcelain rather than `.git/refs`, so packed refs are handled.
pub fn worktree_for_branch(
    executable: &OsStr,
    repository_path: &str,
    branch: &BranchRef,
) -> Result<Option<String>, WorkspaceFailure> {
    let root = repository_root(executable, repository_path)?;
    let output = run_git(executable, &root, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(backend_failure(format!(
            "git-cli: worktree list failed ({})",
            stderr_excerpt(&output)
        )));
    }

    let mut current = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(path.to_owned());
        } else if line.strip_prefix("branch ") == Some(branch.as_str()) {
            return Ok(current);
        }
    }
    Ok(None)
}

/// Live-observes worktree status at `worktree_path`.
///
/// - missing path → `Missing`
/// - present but not inside a working tree, or with an unexpected detached
///   `HEAD` (managed worktrees live on managed branches; detached is
///   `Invalid`, never silently continued), or with unreadable status →
///   `Invalid`
/// - otherwise `status --porcelain=v1` decides: any `??` line →
///   `DirtyUntracked` (untracked files count as dirt), any other entry →
///   `DirtyTracked`, empty → `Clean`
pub fn worktree_status(
    executable: &OsStr,
    worktree_path: &str,
) -> Result<WorktreeStatus, WorkspaceFailure> {
    let abs = match absolutize(worktree_path) {
        Ok(abs) => abs,
        Err(_) => return Ok(WorktreeStatus::Invalid),
    };
    if abs.symlink_metadata().is_err() {
        return Ok(WorktreeStatus::Missing);
    }
    if Path::new(&abs).is_file() {
        return Ok(WorktreeStatus::Invalid);
    }
    let inside = run_git(executable, &abs, &["rev-parse", "--is-inside-work-tree"])?;
    if !inside.status.success() || String::from_utf8_lossy(&inside.stdout).trim() != "true" {
        return Ok(WorktreeStatus::Invalid);
    }
    let head_ref = run_git(executable, &abs, &["symbolic-ref", "--quiet", "HEAD"])?;
    if !head_ref.status.success() {
        return Ok(WorktreeStatus::Invalid);
    }
    let status = run_git(
        executable,
        &abs,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    )?;
    if !status.status.success() {
        return Ok(WorktreeStatus::Invalid);
    }
    Ok(classify_porcelain(
        String::from_utf8_lossy(&status.stdout).as_ref(),
    ))
}

/// Porcelain classifier: `??` counts as untracked dirt, any other entry as
/// tracked dirt, empty output as clean.
fn classify_porcelain(output: &str) -> WorktreeStatus {
    let mut tracked = false;
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("??") {
            return WorktreeStatus::DirtyUntracked;
        }
        tracked = true;
    }
    if tracked {
        WorktreeStatus::DirtyTracked
    } else {
        WorktreeStatus::Clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_classifier_ranks_untracked_above_tracked() {
        assert_eq!(classify_porcelain(""), WorktreeStatus::Clean);
        assert_eq!(
            classify_porcelain(" M file\n"),
            WorktreeStatus::DirtyTracked
        );
        assert_eq!(
            classify_porcelain("A  staged\n"),
            WorktreeStatus::DirtyTracked
        );
        assert_eq!(
            classify_porcelain(" M a\n?? new\n"),
            WorktreeStatus::DirtyUntracked
        );
        assert_eq!(
            classify_porcelain("?? new\n"),
            WorktreeStatus::DirtyUntracked
        );
    }
}
