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
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use workspace::{TrustedCommonGit, WorkspaceFailure, WorkspaceLease};

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

/// Validates the repository metadata entry before asking Git to interpret it.
///
/// Git accepts `.git` files for linked worktrees: `gitdir: <path>` pointing
/// at `<main-repo>/.git/worktrees/<id>`. Existence and directory-ness alone
/// are not enough: a caller could point `.git` at any other syntactically
/// valid Git metadata directory — another repository's `.git`, or another
/// worktree's metadata — and this adapter would silently observe and report
/// that repository's identity instead of the one at this path.
///
/// A genuine linked-worktree metadata directory always carries a `gitdir`
/// file holding the absolute path back to the *specific* worktree `.git`
/// file that points at it, plus a `commondir` file pointing back to the real
/// common `.git` directory — the exact back-reference Git itself relies on
/// (`git worktree list`/`repair` use it to detect a moved or foreign
/// worktree). A legitimate target must therefore satisfy both directions:
/// its reverse `gitdir` resolves to this exact `.git` file, and the target
/// itself is structurally owned by the resolved common repository at
/// `<commondir>/worktrees/<id>`. The structural check matters because a
/// forged local metadata shim could otherwise point its reverse `gitdir`
/// back here while redirecting `commondir` at a foreign repository.
///
/// This is invoked on every call that is about to run Git against `path`
/// (see `classify_repository`, `observe::worktree_status`,
/// `diff::compute_diff`, `cleanup::remove_worktree`): the metadata backing a
/// worktree is provider-writable content, so it is re-checked at each
/// operation rather than once and cached. The final step binds the observed
/// canonical commondir to the trusted `expected` anchor captured at
/// admission; mismatch fails closed as `GitBackendFailure`, never
/// `Ok(Invalid)`.
pub(crate) fn validate_repository_boundary(
    executable: &OsStr,
    path: &Path,
    expected: &TrustedCommonGit,
) -> Result<(), WorkspaceFailure> {
    if validate_repository_metadata(path)? {
        validate_repository_config_extensions(executable, path)?;
        validate_trusted_anchor(path, expected)?;
    }
    Ok(())
}

/// Anchor-free boundary for trusted admission capture only
/// (`establish_trusted_anchor`): metadata identity plus executable-config
/// checks, without anchor comparison (no expected anchor exists yet).
pub(crate) fn validate_repository_boundary_for_capture(
    executable: &OsStr,
    path: &Path,
) -> Result<(), WorkspaceFailure> {
    if validate_repository_metadata(path)? {
        validate_repository_config_extensions(executable, path)?;
    }
    Ok(())
}

/// Binds the observed canonical commondir to the trusted anchor.
///
/// Derives the observed commondir from the `.git → gitdir → commondir`
/// chain (reusing [`resolve_gitdir_pointer`]): `.git` directories observe
/// their own canonical path, `.git` files resolve through their linked
/// metadata `commondir`. Canonicalizes and compares byte-wise with
/// `expected`; mismatch → `Err(GitBackendFailure)`, never `Ok(Invalid)`.
fn validate_trusted_anchor(
    path: &Path,
    expected: &TrustedCommonGit,
) -> Result<(), WorkspaceFailure> {
    let Some(observed) = observe_canonical_commondir(path)? else {
        // No `.git` entry: not a repository shape here. Leave bare/non-repo
        // distinction to `classify_repository` (typed errors independent of
        // the anchor).
        return Ok(());
    };
    if observed != expected.canonical_commondir().to_path_buf() {
        return Err(backend_failure(format!(
            "git-cli: repository identity does not match the trusted anchor (observed {}, expected {}); refusing",
            observed.display(),
            expected.canonical_commondir().display(),
        )));
    }
    Ok(())
}

/// Observes the canonical commondir for the repository Git would discover
/// from `path`. Returns `None` when no `.git` entry exists on the ancestor
/// chain (non-repository shape; caller maps to typed errors).
pub(crate) fn observe_canonical_commondir(
    path: &Path,
) -> Result<Option<PathBuf>, WorkspaceFailure> {
    let mut cursor = match fs::canonicalize(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(backend_failure(format!(
                "git-cli: cannot canonicalize repository path {}: {e}",
                path.display()
            )));
        }
    };
    let git_path = loop {
        let candidate = cursor.join(".git");
        match fs::symlink_metadata(&candidate) {
            Ok(_) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(backend_failure(format!(
                    "git-cli: cannot inspect repository metadata {}: {e}",
                    candidate.display()
                )));
            }
        }
        if !cursor.pop() {
            return Ok(None);
        }
    };
    let meta = fs::symlink_metadata(&git_path).map_err(|e| {
        backend_failure(format!(
            "git-cli: cannot inspect repository metadata {}: {e}",
            git_path.display()
        ))
    })?;
    if meta.file_type().is_symlink() {
        return Err(backend_failure(
            "git-cli: symbolic-link .git metadata is not accepted",
        ));
    }
    if meta.is_dir() {
        let canonical = fs::canonicalize(&git_path).map_err(|e| {
            backend_failure(format!(
                "git-cli: common directory is unavailable {}: {e}",
                git_path.display()
            ))
        })?;
        return Ok(Some(canonical));
    }
    // `.git` file: resolve through the linked-worktree chain.
    let target = resolve_gitdir_pointer(&git_path)?;
    let commondir_pointer = target.join("commondir");
    let commondir = fs::read_to_string(&commondir_pointer).map_err(|_| {
        backend_failure(
            "git-cli: .git pointer target is missing the linked-worktree common-directory reference",
        )
    })?;
    let commondir_path = PathBuf::from(commondir.trim());
    let commondir_resolved = if commondir_path.is_absolute() {
        commondir_path
    } else {
        target.join(commondir_path)
    };
    let commondir_resolved = lexical_normalize(&commondir_resolved);
    let canonical = fs::canonicalize(&commondir_resolved).map_err(|e| {
        backend_failure(format!(
            "git-cli: linked-worktree common directory is unavailable {}: {e}",
            commondir_resolved.display()
        ))
    })?;
    Ok(Some(canonical))
}

/// Validates the nearest `.git` entry Git would discover from `path`.
///
/// Repository-scoped APIs accept paths below the repository root, and Git
/// walks ancestors while discovering repository metadata. The security
/// boundary must therefore follow the same shape instead of checking only
/// `<path>/.git`; otherwise a nested path can bypass validation of a hostile
/// root `.git` pointer. The starting directory is canonicalized so a symlink
/// in the caller path cannot make our discovery walk differ from the actual
/// directory Git enters via `current_dir`.
///
/// Returns `true` when a `.git` entry was found and validated. A missing
/// entry is not itself an error because `classify_repository` still needs to
/// distinguish ordinary directories from unsupported bare repositories.
fn validate_repository_metadata(path: &Path) -> Result<bool, WorkspaceFailure> {
    let mut cursor = match fs::canonicalize(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(backend_failure(format!(
                "git-cli: cannot canonicalize repository path {}: {e}",
                path.display()
            )));
        }
    };

    let git_path = loop {
        let candidate = cursor.join(".git");
        match fs::symlink_metadata(&candidate) {
            Ok(_) => break candidate,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(backend_failure(format!(
                    "git-cli: cannot inspect repository metadata {}: {e}",
                    candidate.display()
                )));
            }
        }
        if !cursor.pop() {
            return Ok(false);
        }
    };

    let metadata = match fs::symlink_metadata(&git_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(backend_failure(format!(
                "git-cli: cannot inspect repository metadata {}: {e}",
                git_path.display()
            )));
        }
    };

    if metadata.file_type().is_symlink() {
        return Err(backend_failure(
            "git-cli: symbolic-link .git metadata is not accepted",
        ));
    }

    if metadata.is_file() {
        let resolved = resolve_gitdir_pointer(&git_path)?;
        let target_meta = fs::symlink_metadata(&resolved).map_err(|e| {
            backend_failure(format!(
                "git-cli: .git pointer target is unavailable {}: {e}",
                resolved.display()
            ))
        })?;
        if target_meta.file_type().is_symlink() {
            return Err(backend_failure(
                "git-cli: .git pointer target must not be a symbolic link",
            ));
        }
        if !target_meta.is_dir() {
            return Err(backend_failure(
                "git-cli: .git pointer target is not a directory",
            ));
        }
        validate_linked_worktree_backreference(&git_path, &resolved)?;
    }

    Ok(true)
}

/// Refuses repository-local configuration that can make a read-only Git
/// observation spawn an arbitrary helper process.
///
/// Command-line `-c` entries in [`run_git`] safely override fixed singleton
/// extension points such as `core.fsmonitor`, `credential.helper`, and
/// `core.externalDiff`. Named clean/smudge/process filters and diff drivers
/// are different: the driver name is selected by repository-controlled
/// attributes, so there is no finite wildcard `-c` override that disables
/// every possible `filter.<name>.*` / `diff.<name>.*` command. Local config
/// can also include another trusted-host-readable config file, reintroducing
/// such commands outside the inspected file. Fail closed on those executable
/// surfaces before any normal repository operation.
fn validate_repository_config_extensions(
    executable: &OsStr,
    path: &Path,
) -> Result<(), WorkspaceFailure> {
    // F9 pattern, extended with repository-local `core.*` redirection and
    // helper-execution surfaces. `core.fsmonitor` / `credential.helper` /
    // `core.externalDiff` stay on `-c` neutralization (existing GREEN
    // keepers); named filters/diff-drivers plus includes plus these
    // `core.*` keys fail closed here.
    const DANGEROUS_CONFIG_PATTERN: &str = r"^(include.*\.path|filter\..*\.(clean|smudge|process)|diff\..*\.(command|textconv)|core\.(worktree|gitproxy|sshcommand|hookspath))$";

    validate_repository_config_scope(executable, path, "--local", DANGEROUS_CONFIG_PATTERN)?;

    // A distinct per-worktree config file exists only when the common
    // repository explicitly enables `extensions.worktreeConfig`. Query that
    // fact without includes before attempting `--worktree`; Git deliberately
    // rejects `--worktree` for ordinary multi-worktree repositories when the
    // extension is disabled.
    let worktree_config = run_git(
        executable,
        path,
        &[
            "config",
            "--local",
            "--no-includes",
            "--bool",
            "--get",
            "extensions.worktreeConfig",
        ],
    )?;
    let worktree_config_enabled = match worktree_config.status.code() {
        Some(0) => String::from_utf8_lossy(&worktree_config.stdout).trim() == "true",
        Some(1) => false,
        _ => {
            return Err(backend_failure(format!(
                "git-cli: cannot validate extensions.worktreeConfig: {}",
                stderr_excerpt(&worktree_config)
            )));
        }
    };
    if worktree_config_enabled {
        validate_repository_config_scope(executable, path, "--worktree", DANGEROUS_CONFIG_PATTERN)?;
    }

    Ok(())
}

fn validate_repository_config_scope(
    executable: &OsStr,
    path: &Path,
    scope: &str,
    pattern: &str,
) -> Result<(), WorkspaceFailure> {
    let output = run_git(
        executable,
        path,
        &[
            "config",
            scope,
            "--no-includes",
            "--name-only",
            "--get-regexp",
            pattern,
        ],
    )?;
    match output.status.code() {
        Some(0) => {
            let names = bounded_detail(String::from_utf8_lossy(&output.stdout).trim().to_owned());
            Err(backend_failure(format!(
                "git-cli: repository-local executable Git config is not accepted: {names}"
            )))
        }
        Some(1) => Ok(()),
        _ => Err(backend_failure(format!(
            "git-cli: cannot validate repository-local Git config ({scope}): {}",
            stderr_excerpt(&output)
        ))),
    }
}

/// Parses and resolves a `gitdir: <path>` `.git` file to an absolute,
/// lexically-normalized path, without touching the filesystem beyond the
/// read of `git_path` itself.
fn resolve_gitdir_pointer(git_path: &Path) -> Result<PathBuf, WorkspaceFailure> {
    let content = fs::read_to_string(git_path).map_err(|e| {
        backend_failure(format!(
            "git-cli: cannot read .git metadata {}: {e}",
            git_path.display()
        ))
    })?;
    let pointer = content
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| backend_failure("git-cli: invalid .git pointer metadata"))?;
    let target = PathBuf::from(pointer);
    let base = git_path.parent().unwrap_or(git_path);
    let resolved = if target.is_absolute() {
        target
    } else {
        base.join(target)
    };
    Ok(lexical_normalize(&resolved))
}

/// Confirms `target` is the linked-worktree metadata directory Git itself
/// created for the worktree whose `.git` file is `git_path` — never an
/// arbitrary directory, or another repository's metadata, that merely
/// happens to be a syntactically valid pointer target.
fn validate_linked_worktree_backreference(
    git_path: &Path,
    target: &Path,
) -> Result<(), WorkspaceFailure> {
    let reverse_pointer = target.join("gitdir");
    let reverse = fs::read_to_string(&reverse_pointer).map_err(|_| {
        backend_failure(
            "git-cli: .git pointer target is missing the linked-worktree back-reference; \
             refusing repository identity redirection",
        )
    })?;
    let reverse_path = lexical_normalize(&PathBuf::from(reverse.trim()));
    if !reverse_path.is_absolute() {
        return Err(backend_failure(
            "git-cli: linked-worktree back-reference must be absolute",
        ));
    }
    // Git canonicalizes (resolves symlinks in) the absolute path it records
    // in both the forward and reverse worktree pointers, so compare the
    // actual filesystem identities (e.g. macOS `/tmp` -> `/private/tmp`).
    let reverse_canonical = fs::canonicalize(&reverse_path).map_err(|e| {
        backend_failure(format!(
            "git-cli: linked-worktree back-reference is unavailable {}: {e}",
            reverse_path.display()
        ))
    })?;
    let git_canonical = fs::canonicalize(git_path).map_err(|e| {
        backend_failure(format!(
            "git-cli: worktree .git metadata is unavailable {}: {e}",
            git_path.display()
        ))
    })?;
    if reverse_canonical != git_canonical {
        return Err(backend_failure(
            "git-cli: .git pointer target does not reference this worktree; \
             refusing repository identity redirection",
        ));
    }

    let commondir_pointer = target.join("commondir");
    let commondir = fs::read_to_string(&commondir_pointer).map_err(|_| {
        backend_failure(
            "git-cli: .git pointer target is missing the linked-worktree common-directory \
             reference",
        )
    })?;
    let commondir_path = PathBuf::from(commondir.trim());
    let commondir_resolved = if commondir_path.is_absolute() {
        commondir_path
    } else {
        target.join(commondir_path)
    };
    let commondir_resolved = lexical_normalize(&commondir_resolved);
    let common_meta = fs::symlink_metadata(&commondir_resolved).map_err(|e| {
        backend_failure(format!(
            "git-cli: linked-worktree common directory is unavailable {}: {e}",
            commondir_resolved.display()
        ))
    })?;
    if common_meta.file_type().is_symlink() {
        return Err(backend_failure(
            "git-cli: linked-worktree common directory must not be a symbolic link",
        ));
    }
    if !common_meta.is_dir() {
        return Err(backend_failure(
            "git-cli: linked-worktree common directory is not a directory",
        ));
    }

    // A real linked-worktree gitdir is structurally owned by its common
    // repository: `<commondir>/worktrees/<id>`. Merely checking that the
    // `commondir` exists is insufficient because an attacker could forge a
    // local metadata shim whose reverse `gitdir` points back to this
    // worktree while `commondir` points at a different repository. Git would
    // then resolve refs/objects from that foreign repository. Bind the
    // forward target to the resolved common directory exactly as Git lays
    // out linked-worktree metadata.
    let target_canonical = fs::canonicalize(target).map_err(|e| {
        backend_failure(format!(
            "git-cli: linked-worktree metadata target is unavailable {}: {e}",
            target.display()
        ))
    })?;
    let common_canonical = fs::canonicalize(&commondir_resolved).map_err(|e| {
        backend_failure(format!(
            "git-cli: linked-worktree common directory is unavailable {}: {e}",
            commondir_resolved.display()
        ))
    })?;
    let expected_parent = common_canonical.join("worktrees");
    if target_canonical.parent() != Some(expected_parent.as_path()) {
        return Err(backend_failure(
            "git-cli: .git pointer target is not owned by its linked-worktree common directory; \
             refusing repository identity redirection",
        ));
    }
    Ok(())
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
    // The child process receives a controlled Git environment. Repository
    // configuration, hooks, filters, fsmonitor, helpers, and inherited
    // GIT_* variables are attacker-controlled input at this boundary.
    // Disable configuration discovery and executable extension points before
    // Git parses the untrusted repository.
    command
        .env_clear()
        .args([
            OsStr::new("-c"),
            OsStr::new("core.hooksPath=/dev/null"),
            OsStr::new("-c"),
            OsStr::new("core.fsmonitor=false"),
            OsStr::new("-c"),
            OsStr::new("core.sshCommand=false"),
            OsStr::new("-c"),
            OsStr::new("credential.helper="),
            OsStr::new("-c"),
            OsStr::new("core.externalDiff="),
            OsStr::new("-c"),
            OsStr::new("diff.external="),
            OsStr::new("-c"),
            OsStr::new("core.pager=cat"),
            OsStr::new("-c"),
            OsStr::new("pager.status=false"),
        ])
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        // Do not inherit host Git configuration locations. Repository-local
        // configuration is still handled by explicit `-c` policy above and
        // remains an area guarded by the metadata identity work in TASK-0071.
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("GIT_EDITOR", ":")
        .env("GIT_SEQUENCE_EDITOR", ":");
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
pub fn classify_repository(
    executable: &OsStr,
    path: &Path,
    expected: &TrustedCommonGit,
) -> Result<RepoClass, WorkspaceFailure> {
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
    validate_repository_boundary(executable, path, expected)?;
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

/// Captures the trusted common-Git anchor from trusted admission state.
///
/// Sole anchor-free path, admission only. Runs under hardened [`run_git`]
/// (`rev-parse --git-common-dir`), then `fs::canonicalize` plus symlink
/// refusal; otherwise `GitBackendFailure`. Bare →
/// `BareRepositoryUnsupported`, non-repo → `NotRepository`, independent of
/// any anchor.
pub(crate) fn establish_trusted_anchor(
    executable: &OsStr,
    trusted_repository_path: &Path,
) -> Result<TrustedCommonGit, WorkspaceFailure> {
    // Admission path must be a usable directory (not a file); missing →
    // NotRepository (typed, not backend failure).
    match trusted_repository_path.symlink_metadata() {
        Ok(meta) => {
            if meta.is_file() {
                return Err(WorkspaceFailure::NotRepository {
                    detail: bounded_detail(format!(
                        "git-cli: not a repository: {}",
                        trusted_repository_path.display()
                    )),
                });
            }
        }
        Err(_) => {
            return Err(WorkspaceFailure::NotRepository {
                detail: bounded_detail(format!(
                    "git-cli: repository path missing: {}",
                    trusted_repository_path.display()
                )),
            });
        }
    }
    // Refuse symlinked `.git` entries and hostile executable config before
    // Git interprets the trusted path (anchor-free capture validation).
    validate_repository_boundary_for_capture(executable, trusted_repository_path)?;
    // Typed bare/non-repo distinction before anchor capture.
    let inside = run_git(
        executable,
        trusted_repository_path,
        &["rev-parse", "--is-inside-work-tree"],
    )?;
    if !(inside.status.success() && String::from_utf8_lossy(&inside.stdout).trim() == "true") {
        let bare = run_git(
            executable,
            trusted_repository_path,
            &["rev-parse", "--is-bare-repository"],
        )?;
        if bare.status.success() && String::from_utf8_lossy(&bare.stdout).trim() == "true" {
            return Err(WorkspaceFailure::BareRepositoryUnsupported {
                detail: bounded_detail(format!(
                    "git-cli: bare repository unsupported (no implicit clone): {}",
                    trusted_repository_path.display()
                )),
            });
        }
        return Err(WorkspaceFailure::NotRepository {
            detail: bounded_detail(format!(
                "git-cli: not a repository: {}",
                trusted_repository_path.display()
            )),
        });
    }
    let (common_out, common_text) = run_git_text(
        executable,
        trusted_repository_path,
        &["rev-parse", "--git-common-dir"],
    )?;
    if !common_out.status.success() || common_text.is_empty() {
        return Err(backend_failure(
            "git-cli: cannot observe trusted common Git directory",
        ));
    }
    if common_text.contains('\0') {
        return Err(backend_failure(
            "git-cli: trusted common directory contains NUL",
        ));
    }
    let raw = PathBuf::from(&common_text);
    let joined = if raw.is_absolute() {
        raw
    } else {
        trusted_repository_path.join(raw)
    };
    let normalized = lexical_normalize(&joined);
    // Symlink refusal on the observed commondir itself.
    match fs::symlink_metadata(&normalized) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(backend_failure(
                    "git-cli: trusted common directory must not be a symbolic link",
                ));
            }
            if !meta.is_dir() {
                return Err(backend_failure(
                    "git-cli: trusted common directory is not a directory",
                ));
            }
        }
        Err(e) => {
            return Err(backend_failure(format!(
                "git-cli: trusted common directory unavailable {}: {e}",
                normalized.display()
            )));
        }
    }
    let canonical = fs::canonicalize(&normalized).map_err(|e| {
        backend_failure(format!(
            "git-cli: cannot canonicalize trusted common directory {}: {e}",
            normalized.display()
        ))
    })?;
    TrustedCommonGit::from_canonical_commondir(canonical)
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
