//! Read-only workspace diff observation baselined at the recorded
//! `base_revision` (ADR-0023 §6).
//!
//! Evidence layers: workspace HEAD history (`base..HEAD` via
//! `diff --name-status`) plus working-tree/index modifications (`status
//! --porcelain`). Never against the live default-branch tip. Read-only:
//! only `rev-parse`, `diff --name-status`, and `status` run here — no
//! staging, commit, checkout, reset, clean, or ref mutation.
//!
//! Output discipline: structured [`workspace::WorktreeDiff`] entries only,
//! bounded to [`workspace::MAX_DIFF_ENTRIES`] per layer with an explicit
//! truncation flag. Raw patch text is never returned, never logged, and
//! never persisted. All Git invocation is direct argv (no shell),
//! non-interactive, local only, with bounded capture via `runner`.

use std::ffi::OsStr;
use std::path::PathBuf;

use workspace::{
    DiffChangeKind, DiffEntry, DiffEvidence, Revision, WorkspaceFailure, WorkspaceLease,
    WorktreeStatus, MAX_DIFF_ENTRIES, MAX_DIFF_SUMMARY_CHARS,
};

use crate::observe;
use crate::runner::{absolutize, backend_failure, bounded_detail, run_git, run_git_text};

/// Computes the workspace diff baselined at the lease's recorded base.
pub fn compute_diff(
    executable: &OsStr,
    lease: &WorkspaceLease,
) -> Result<DiffEvidence, WorkspaceFailure> {
    let worktree_ref = lease.worktree_ref();
    let status = observe::worktree_status(executable, worktree_ref)?;
    let usable = match status {
        WorktreeStatus::Clean | WorktreeStatus::DirtyTracked | WorktreeStatus::DirtyUntracked => {
            true
        }
        WorktreeStatus::Missing => {
            return Err(WorkspaceFailure::WorktreeMissing {
                detail: bounded_detail(format!(
                    "git-cli: cannot diff absent worktree for lease {}",
                    lease.id()
                )),
            });
        }
        WorktreeStatus::Invalid => {
            return Err(WorkspaceFailure::CheckoutFailed {
                detail: bounded_detail(format!(
                    "git-cli: cannot diff unusable worktree for lease {}",
                    lease.id()
                )),
            });
        }
    };
    debug_assert!(usable);
    let abs = absolutize(worktree_ref).map_err(|_| WorkspaceFailure::CheckoutFailed {
        detail: bounded_detail(format!(
            "git-cli: worktree reference unusable for lease {}",
            lease.id()
        )),
    })?;
    let worktree_str = abs
        .to_str()
        .ok_or_else(|| backend_failure("git-cli: worktree path is not UTF-8; refusing diff"))?;

    let head =
        observe::resolve_revision(executable, worktree_str, "HEAD").map_err(|e| match e {
            WorkspaceFailure::InvalidRevision { detail } => {
                WorkspaceFailure::GitBackendFailure { detail }
            }
            other => other,
        })?;
    prove_base_present(executable, &abs, lease.base_revision())?;

    let mut truncated = false;
    let mut committed = if head.as_str() == lease.base_revision().as_str() {
        Vec::new()
    } else {
        let entries = committed_changes(
            executable,
            &abs,
            lease.base_revision().as_str(),
            head.as_str(),
        )?;
        truncate_entries(entries, &mut truncated)
    };
    let mut working = working_changes(executable, &abs)?;
    if working.len() > MAX_DIFF_ENTRIES {
        working.truncate(MAX_DIFF_ENTRIES);
        truncated = true;
    }
    let _ = &mut committed;

    let has_working_tree_changes = !working.is_empty();
    let summary = diff_summary(
        lease.base_revision(),
        Some(&head),
        status,
        committed.len(),
        working.len(),
        truncated,
    );
    Ok(DiffEvidence {
        diff: workspace::WorktreeDiff {
            base_revision: lease.base_revision().clone(),
            head_revision: Some(head),
            has_working_tree_changes,
            status,
            committed,
            working,
            truncated,
        },
        summary,
    })
}

/// Proves the recorded base OID names a commit visible from the worktree.
fn prove_base_present(
    executable: &OsStr,
    worktree: &std::path::Path,
    base: &Revision,
) -> Result<(), WorkspaceFailure> {
    let candidate = format!("{}^{{commit}}", base.as_str());
    let (output, resolved) = run_git_text(
        executable,
        worktree,
        &["rev-parse", "--verify", "--quiet", candidate.as_str()],
    )?;
    if !output.status.success() || resolved != base.as_str() {
        return Err(WorkspaceFailure::InvalidRevision {
            detail: bounded_detail(format!(
                "git-cli: recorded base revision is not a commit in this repository: {}",
                base.as_str()
            )),
        });
    }
    Ok(())
}

/// Committed `base..HEAD` file changes via `diff --name-status -z`.
fn committed_changes(
    executable: &OsStr,
    worktree: &std::path::Path,
    base: &str,
    head: &str,
) -> Result<Vec<DiffEntry>, WorkspaceFailure> {
    let output = run_git(
        executable,
        worktree,
        &[
            OsStr::new("diff"),
            OsStr::new("--name-status"),
            OsStr::new("-z"),
            OsStr::new("--find-renames"),
            OsStr::new(base),
            OsStr::new(head),
            OsStr::new("--"),
        ],
    )?;
    if !output.status.success() {
        return Err(backend_failure(format!(
            "git-cli: cannot compute committed diff for base {base}"
        )));
    }
    parse_diff_name_status_z(&output.stdout)
}

/// Working-tree/index changes via `status --porcelain=v1 -z`.
fn working_changes(
    executable: &OsStr,
    worktree: &std::path::Path,
) -> Result<Vec<DiffEntry>, WorkspaceFailure> {
    let output = run_git(
        executable,
        worktree,
        &[
            OsStr::new("status"),
            OsStr::new("--porcelain=v1"),
            OsStr::new("-z"),
            OsStr::new("--untracked-files=normal"),
            OsStr::new("--"),
        ],
    )?;
    if !output.status.success() {
        return Err(backend_failure(
            "git-cli: cannot observe working-tree status for diff",
        ));
    }
    parse_status_porcelain_z(&output.stdout)
}

fn truncate_entries(mut entries: Vec<DiffEntry>, truncated: &mut bool) -> Vec<DiffEntry> {
    if entries.len() > MAX_DIFF_ENTRIES {
        entries.truncate(MAX_DIFF_ENTRIES);
        *truncated = true;
    }
    entries
}

fn diff_summary(
    base: &Revision,
    head: Option<&Revision>,
    status: WorktreeStatus,
    committed: usize,
    working: usize,
    truncated: bool,
) -> String {
    let head_text = head.map_or_else(|| "-".to_owned(), |h| h.as_str().to_owned());
    let raw = format!(
        "base={} head={} status={:?} committed={} working={} truncated={}",
        base.as_str(),
        head_text,
        status,
        committed,
        working,
        truncated
    );
    if raw.chars().count() <= MAX_DIFF_SUMMARY_CHARS {
        raw
    } else {
        bounded_detail(raw)
    }
}

/// Parses `diff --name-status -z` output (NUL-separated).
///
/// Records: `<status>\0<path>\0` for single-path changes, where `<status>`
/// is one letter (`A`/`M`/`D`/`T`/`U`/unknown) — or `<R-score>\0<old>\0<new>\0`
/// / `<C-score>\0<old>\0<new>\0` for renames/copies (old first, new second).
fn parse_diff_name_status_z(bytes: &[u8]) -> Result<Vec<DiffEntry>, WorkspaceFailure> {
    let text = String::from_utf8_lossy(bytes);
    let tokens: Vec<&str> = text.split('\0').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if token.is_empty() {
            i += 1;
            continue;
        }
        let first = token.as_bytes()[0] as char;
        if first == 'R' || first == 'C' {
            let old = tokens.get(i + 1).copied().unwrap_or("");
            let new = tokens.get(i + 2).copied().unwrap_or("");
            if old.is_empty() || new.is_empty() {
                return Err(backend_failure(
                    "git-cli: unexpected rename record in diff output",
                ));
            }
            let kind = if first == 'R' {
                DiffChangeKind::Renamed
            } else {
                DiffChangeKind::Added
            };
            out.push(DiffEntry {
                path: new.to_owned(),
                kind,
            });
            i += 3;
        } else {
            let path = tokens.get(i + 1).copied().unwrap_or("");
            if path.is_empty() {
                return Err(backend_failure(
                    "git-cli: unexpected change record in diff output",
                ));
            }
            let kind = match first {
                'A' => DiffChangeKind::Added,
                'D' => DiffChangeKind::Deleted,
                'M' | 'T' | 'U' => DiffChangeKind::Modified,
                _ => DiffChangeKind::Modified,
            };
            out.push(DiffEntry {
                path: path.to_owned(),
                kind,
            });
            i += 2;
        }
        if out.len() > MAX_DIFF_ENTRIES * 2 {
            break;
        }
    }
    Ok(out)
}

/// Parses `status --porcelain=v1 -z` output (NUL-separated).
///
/// Records: `XY SP path\0` for single-path entries (`XY` are the index /
/// worktree codes, `SP` a literal space). Renames/copies in the index are
/// `R  new\0old\0` (new first, old second — reversed vs `diff`).
fn parse_status_porcelain_z(bytes: &[u8]) -> Result<Vec<DiffEntry>, WorkspaceFailure> {
    let text = String::from_utf8_lossy(bytes);
    let tokens: Vec<&str> = text.split('\0').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if token.is_empty() {
            i += 1;
            continue;
        }
        let bytes = token.as_bytes();
        if bytes.len() < 4 || bytes[2] != b' ' {
            return Err(backend_failure(
                "git-cli: unexpected status record in diff output",
            ));
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let path = &token[3..];
        if path.is_empty() {
            return Err(backend_failure("git-cli: empty path in status output"));
        }
        if x == 'R' || x == 'C' {
            let _old = tokens.get(i + 1).copied().unwrap_or("");
            if _old.is_empty() {
                return Err(backend_failure(
                    "git-cli: unexpected rename record in status output",
                ));
            }
            let kind = if x == 'R' {
                DiffChangeKind::Renamed
            } else {
                DiffChangeKind::Added
            };
            out.push(DiffEntry {
                path: path.to_owned(),
                kind,
            });
            i += 2;
            continue;
        }
        if x == '?' && y == '?' {
            out.push(DiffEntry {
                path: path.to_owned(),
                kind: DiffChangeKind::Untracked,
            });
        } else if x == '!' || y == '!' {
            // Ignored (not shown with normal untracked mode); skip.
        } else if x == 'A' || y == 'A' {
            out.push(DiffEntry {
                path: path.to_owned(),
                kind: DiffChangeKind::Added,
            });
        } else if x == 'D' || y == 'D' {
            out.push(DiffEntry {
                path: path.to_owned(),
                kind: DiffChangeKind::Deleted,
            });
        } else {
            out.push(DiffEntry {
                path: path.to_owned(),
                kind: DiffChangeKind::Modified,
            });
        }
        i += 1;
        if out.len() > MAX_DIFF_ENTRIES * 2 {
            break;
        }
    }
    let _ = PathBuf::new();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_name_status_z_parses_modify_and_rename() {
        // M path, then R-score old new (old first, new second).
        let raw = b"M\0file.txt\0R071\0old.txt\0new.txt\0A\0added.txt\0D\0gone.txt\0";
        let entries = parse_diff_name_status_z(raw).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].path, "file.txt");
        assert_eq!(entries[0].kind, DiffChangeKind::Modified);
        assert_eq!(entries[1].path, "new.txt");
        assert_eq!(entries[1].kind, DiffChangeKind::Renamed);
        assert_eq!(entries[2].kind, DiffChangeKind::Added);
        assert_eq!(entries[3].kind, DiffChangeKind::Deleted);
    }

    #[test]
    fn status_porcelain_z_parses_dirty_and_untracked() {
        // XY SP path records; rename is new-then-old across two NUL fields.
        let raw = b" M file.txt\0?? new.txt\0R  renamed2.txt\0file.txt\0";
        let entries = parse_status_porcelain_z(raw).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, DiffChangeKind::Modified);
        assert_eq!(entries[1].kind, DiffChangeKind::Untracked);
        assert_eq!(entries[1].path, "new.txt");
        assert_eq!(entries[2].kind, DiffChangeKind::Renamed);
        assert_eq!(entries[2].path, "renamed2.txt");
    }

    #[test]
    fn summary_is_bounded_and_deterministic() {
        let base = Revision::new("a".repeat(40)).unwrap();
        let head = Revision::new("b".repeat(40)).unwrap();
        let first = diff_summary(
            &base,
            Some(&head),
            WorktreeStatus::DirtyTracked,
            2,
            1,
            false,
        );
        let second = diff_summary(
            &base,
            Some(&head),
            WorktreeStatus::DirtyTracked,
            2,
            1,
            false,
        );
        assert_eq!(first, second);
        assert!(first.chars().count() <= MAX_DIFF_SUMMARY_CHARS);
        assert!(first.contains("committed=2"));
    }
}
