//! Direct-argv Git process invocation for the `git-cli` adapter.
//!
//! Every Git execution in TASK-0041 flows through [`run_git`]: a fixed
//! executable plus an argument array via [`std::process::Command`]. There is
//! no shell (`sh -c`, `bash -c`, `cmd /C`, `powershell -Command` are never
//! used) and caller data is never concatenated into a command string, so
//! paths or branch names containing spaces, Unicode, quotes, or
//! shell metacharacters remain ordinary argv values.
//!
//! Non-interactive behavior: each child gets `GIT_TERMINAL_PROMPT=0` (Git
//! fails fast instead of blocking on credential prompts) and
//! `GIT_OPTIONAL_LOCKS=0` (no background maintenance side effects), plus a
//! closed stdin. Both are process-local environment entries; the adapter
//! never writes `~/.gitconfig`, system config, or repository config.
//!
//! No remote operations exist in this module: the only subcommands spelled
//! here are `version`, `rev-parse`, `symbolic-ref`, `show-ref`, `worktree`
//! (`add`, `list`), and `status`. `fetch`/`pull`/`push`/`clone`/`ls-remote`
//! appear nowhere as executed commands.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use workspace::{WorkspaceFailure, WorkspaceLease};

/// Backend identity surfaced via `GitPort::backend_ref` (never branched on).
pub const BACKEND_REF: &str = "git-cli";

/// Upper bound for any captured Git stdout. Commands in this adapter are
/// expected to return identifiers or small metadata; absurdly large output
/// is rejected instead of becoming an unbounded in-memory payload.
pub const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;

/// Upper bound for single-line metadata (OIDs, refs, toplevel paths).
const MAX_SMALL_OUTPUT_BYTES: usize = 4096;

/// Maximum characters kept in any diagnostic detail handed to higher layers.
pub const MAX_DETAIL_CHARS: usize = 500;

/// Truncates diagnostic text to [`MAX_DETAIL_CHARS`] so Git output can only
/// ever travel upward as bounded evidence.
#[must_use]
pub fn bounded_detail(text: String) -> String {
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return text;
    }
    let truncated: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    format!("{truncated}…[truncated]")
}

/// Constructor for [`WorkspaceFailure::GitBackendFailure`] with bounded detail.
pub fn backend_failure(detail: impl Into<String>) -> WorkspaceFailure {
    WorkspaceFailure::GitBackendFailure {
        detail: bounded_detail(detail.into()),
    }
}

/// Short stderr excerpt for diagnostics (bounded; never used for domain
/// classification, which relies on preflight observations, exit status, and
/// plumbing output instead).
pub fn stderr_excerpt(output: &Output) -> String {
    bounded_detail(String::from_utf8_lossy(&output.stderr).trim().to_owned())
}

/// Resolves `raw` to an absolute path without touching the filesystem.
///
/// Relative inputs are joined onto the process working directory so every
/// argv path handed to Git is absolute. Absolute paths never start with `-`,
/// which additionally neutralizes dash-led relative inputs before they can
/// reach option parsing. Lexical `.`/`..` segments are normalized without
/// following symlinks (symlink-escape hardening belongs to durable path
/// authorization in TASK-0042, not to this adapter).
pub fn absolutize(raw: &str) -> Result<PathBuf, WorkspaceFailure> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(backend_failure("git-cli: blank path is not usable"));
    }
    if trimmed.contains('\0') {
        return Err(backend_failure("git-cli: path contains NUL"));
    }
    let path = PathBuf::from(trimmed);
    let absolute = if path.is_absolute() {
        path
    } else {
        let cwd = std::env::current_dir()
            .map_err(|e| backend_failure(format!("git-cli: cannot read cwd: {e}")))?;
        cwd.join(path)
    };
    Ok(lexical_normalize(&absolute))
}

/// Lexical `.`/`..` normalization without filesystem access.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Whether `child` lies strictly inside `parent` (both absolute, normalized).
#[must_use]
pub fn path_strictly_inside(parent: &Path, child: &Path) -> bool {
    child != parent && child.strip_prefix(parent).is_ok()
}

/// Invokes `git` with a fixed executable plus argv array — never a shell.
///
/// `cwd` is entered via [`Command::current_dir`], not via `git -C`, so even
/// unusual directory spellings never pass through option parsing. Exit
/// status is left to the caller: this helper only reports transport-level
/// problems (unspawnable binary, I/O, absurd output) as
/// [`WorkspaceFailure::GitBackendFailure`].
pub fn run_git<A: AsRef<OsStr>>(
    executable: &OsStr,
    cwd: &Path,
    args: &[A],
) -> Result<Output, WorkspaceFailure> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0");
    let output = command.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            backend_failure(format!(
                "git-cli: cannot spawn git executable (unavailable?) or working directory vanished: {e}"
            ))
        } else {
            backend_failure(format!("git-cli: cannot spawn git: {e}"))
        }
    })?;
    if output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(backend_failure(format!(
            "git-cli: git output exceeded {} bytes; refusing unbounded payload",
            MAX_GIT_OUTPUT_BYTES
        )));
    }
    Ok(output)
}

/// Runs Git and returns trimmed stdout, enforcing the small-metadata bound.
pub fn run_git_text<A: AsRef<OsStr>>(
    executable: &OsStr,
    cwd: &Path,
    args: &[A],
) -> Result<(Output, String), WorkspaceFailure> {
    let output = run_git(executable, cwd, args)?;
    if output.stdout.len() > MAX_SMALL_OUTPUT_BYTES {
        return Err(backend_failure(
            "git-cli: small-metadata git output exceeded bound",
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok((output, text))
}

/// Repository classification shared by every repo-scoped operation.
///
/// Uses Git facts only (`rev-parse`), never `.git` directory-name heuristics
/// (linked worktrees use `.git` files).
pub enum RepoClass {
    /// A working repository with this authoritative toplevel root.
    Normal { root: PathBuf },
}

/// Classifies `path` (absolute) as a working repo, bare repo, or non-repo.
///
/// - working repository → `Ok(RepoClass::Normal)`
/// - bare repository → `Err(BareRepositoryUnsupported)` (explicit, no clone)
/// - anything else (normal dir, file, missing path) → `Err(NotRepository)`
pub fn classify_repository(executable: &OsStr, path: &Path) -> Result<RepoClass, WorkspaceFailure> {
    match path.symlink_metadata() {
        Ok(meta) => {
            if meta.is_file() {
                return Err(WorkspaceFailure::NotRepository {
                    detail: bounded_detail(format!(
                        "git-cli: not a repository: {}",
                        path.display()
                    )),
                });
            }
        }
        Err(_) => {
            return Err(WorkspaceFailure::NotRepository {
                detail: bounded_detail(format!(
                    "git-cli: repository path missing: {}",
                    path.display()
                )),
            });
        }
    }
    let inside = run_git(executable, path, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.status.success() && String::from_utf8_lossy(&inside.stdout).trim() == "true" {
        let (_, root_text) = run_git_text(executable, path, &["rev-parse", "--show-toplevel"])?;
        if root_text.is_empty() {
            return Err(backend_failure(
                "git-cli: rev-parse --show-toplevel returned empty root",
            ));
        }
        return Ok(RepoClass::Normal {
            root: lexical_normalize(&PathBuf::from(root_text)),
        });
    }
    let bare = run_git(executable, path, &["rev-parse", "--is-bare-repository"])?;
    if bare.status.success() && String::from_utf8_lossy(&bare.stdout).trim() == "true" {
        return Err(WorkspaceFailure::BareRepositoryUnsupported {
            detail: bounded_detail(format!(
                "git-cli: bare repository unsupported (no implicit clone): {}",
                path.display()
            )),
        });
    }
    Err(WorkspaceFailure::NotRepository {
        detail: bounded_detail(format!("git-cli: not a repository: {}", path.display())),
    })
}

/// Proves at call time that `lease` carries values this adapter may act on.
///
/// The domain types already validate shape; this only rejects the one shape
/// that is unsafe at argv level: a managed branch whose short name starts
/// with `-`, which option parsing could mistake for flags.
pub fn check_branch_argv_safety(lease: &WorkspaceLease) -> Result<(), WorkspaceFailure> {
    let short = lease.branch().short_name();
    if short.starts_with('-') {
        return Err(backend_failure(format!(
            "git-cli: managed branch short name starts with '-'; refusing argv-unsafe ref {}",
            lease.branch()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_is_bounded() {
        let long = "x".repeat(MAX_DETAIL_CHARS + 100);
        let out = bounded_detail(long);
        assert!(out.chars().count() <= MAX_DETAIL_CHARS + "…[truncated]".chars().count());
        assert!(out.ends_with("…[truncated]"));
        assert_eq!(bounded_detail("short".to_owned()), "short");
    }

    #[test]
    fn absolutize_rejects_blank_and_nul() {
        assert!(absolutize("   ").is_err());
        assert!(absolutize("a\0b").is_err());
    }

    #[test]
    fn lexical_normalize_resolves_dots_without_fs() {
        let abs = absolutize("/a/b/../c/./d").unwrap();
        assert_eq!(abs, PathBuf::from("/a/c/d"));
        assert!(abs.is_absolute());
    }

    #[test]
    fn strict_inside_excludes_self_and_siblings() {
        let parent = PathBuf::from("/r");
        assert!(path_strictly_inside(&parent, &PathBuf::from("/r/wt")));
        assert!(!path_strictly_inside(&parent, &parent));
        assert!(!path_strictly_inside(&parent, &PathBuf::from("/other")));
        assert!(!path_strictly_inside(&parent, &PathBuf::from("/r2")));
    }
}
