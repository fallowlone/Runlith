//! Shared hermetic Git fixtures for `git-cli` integration tests.
//!
//! Every fixture is a wholly test-owned temporary repository under the OS
//! temp dir (unique per test via pid + atomic counter + test name), driven
//! only through direct-argv [`std::process::Command`] — no shell, no global
//! or repository config writes (identity and signing guards travel as
//! process-local `-c` flags), no remotes, no network. `Drop` removes the
//! whole fixture tree; that is test-teardown destruction of test-owned
//! state, never product cleanup semantics.

//! Shared across test binaries: each binary uses a subset, so dead-code
//! allowance keeps per-binary compilation warning-free.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use workspace::{BranchRef, ExecutionId, RepositoryRef, Revision, WorkspaceId, WorkspaceLease};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-local Git config guards: hermetic identity, no commit signing
/// (a user-global `commit.gpgsign=true` must not block fixture commits).
fn config_guards(args: &mut Vec<String>) {
    args.push("-c".to_owned());
    args.push("user.name=git-cli-fixture".to_owned());
    args.push("-c".to_owned());
    args.push("user.email=fixture@example.invalid".to_owned());
    args.push("-c".to_owned());
    args.push("commit.gpgsign=false".to_owned());
}

/// Runs `git` with argv only in `cwd`; panics with bounded stderr on failure.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let mut full: Vec<String> = Vec::with_capacity(args.len() + 6);
    config_guards(&mut full);
    full.extend(args.iter().map(|s| s.to_string()));
    let output = Command::new("git")
        .args(&full)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .expect("fixture: cannot spawn git");
    assert!(
        output.status.success(),
        "fixture git failed: git {full:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Runs `git`, returning trimmed stdout even on failure with the exit code.
pub fn git_checked(cwd: &Path, args: &[&str]) -> (bool, String) {
    let mut full: Vec<String> = Vec::with_capacity(args.len() + 6);
    config_guards(&mut full);
    full.extend(args.iter().map(|s| s.to_string()));
    let output = Command::new("git")
        .args(&full)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture: cannot spawn git");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    )
}

/// A wholly test-owned local repository fixture.
///
/// Layout (siblings — the managed root is never nested inside the primary
/// checkout, mirroring the adapter's fail-closed nesting refusal):
///
/// ```text
/// <case>/repo            the primary working repository
/// <case>/managed-roots/  caller-provisioned worktree targets
/// ```
pub struct Fixture {
    /// Case directory owning the whole fixture tree (removed on drop).
    pub case_dir: PathBuf,
    /// Fixture root (the repository working tree).
    pub dir: PathBuf,
    /// Caller-provisioned managed root for worktree targets.
    pub roots: PathBuf,
    /// Current branch short name, discovered — never assumed.
    pub branch: String,
    /// Full ref of the current (default) branch.
    pub default_ref: String,
    /// OID of the initial commit.
    pub base_oid: String,
}

impl Fixture {
    /// Creates `git init` + one commit + canonical `origin/HEAD` metadata.
    /// `name` may contain spaces to prove argv safety.
    pub fn new(name: &str) -> Self {
        let id = FIXTURE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let case_dir =
            std::env::temp_dir().join(format!("git-cli-{}-{id}-{name}", std::process::id()));
        let dir = case_dir.join("repo");
        let roots = case_dir.join("managed-roots");
        std::fs::create_dir_all(&dir).expect("fixture: mkdir repo");
        std::fs::create_dir_all(&roots).expect("fixture: mkdir managed root");
        git(&dir, &["init", "-q"]);
        git(&dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let branch = git(&dir, &["symbolic-ref", "--short", "HEAD"]);
        let default_ref = format!("refs/heads/{branch}");
        git(
            &dir,
            &["symbolic-ref", "refs/remotes/origin/HEAD", &default_ref],
        );
        let base_oid = git(&dir, &["rev-parse", "HEAD"]);
        Self {
            case_dir,
            dir,
            roots,
            branch,
            default_ref,
            base_oid,
        }
    }

    /// Renames the default branch to a non-standard name and repoints the
    /// canonical `origin/HEAD` metadata (proves semantic, not literal,
    /// default-branch protection).
    pub fn rename_default(&mut self, new_short: &str) {
        git(&self.dir, &["branch", "-m", new_short]);
        self.branch = new_short.to_owned();
        self.default_ref = format!("refs/heads/{new_short}");
        git(
            &self.dir,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                &self.default_ref,
            ],
        );
    }

    pub fn path_str(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }

    /// Current primary HEAD OID via direct Git (adapter-independent check).
    pub fn primary_head(&self) -> String {
        git(&self.dir, &["rev-parse", "HEAD"])
    }

    /// Porcelain status of the primary checkout via direct Git.
    pub fn primary_status(&self) -> String {
        git(
            &self.dir,
            &["status", "--porcelain=v1", "--untracked-files=normal"],
        )
    }

    /// Whether `refname` exists, via direct Git (adapter-independent check).
    pub fn ref_exists(&self, refname: &str) -> bool {
        git_checked(&self.dir, &["show-ref", "--verify", "--quiet", refname]).0
    }

    /// Registered worktree paths via direct Git (adapter-independent check).
    pub fn registered_worktrees(&self) -> Vec<String> {
        git(&self.dir, &["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|l| l.strip_prefix("worktree ").map(str::to_owned))
            .collect()
    }

    pub fn write_tracked(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.dir.join(name), bytes).expect("fixture: write tracked");
    }

    pub fn read_tracked(&self, name: &str) -> Vec<u8> {
        std::fs::read(self.dir.join(name)).expect("fixture: read tracked")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Test-owned case tree only; failures ignored (temp reaping is best-effort).
        let _ = std::fs::remove_dir_all(&self.case_dir);
    }
}

/// Builds a lease with test-unique identities.
pub fn lease_for(
    tag: &str,
    branch_short: &str,
    base_oid: &str,
    worktree_ref: &str,
) -> WorkspaceLease {
    WorkspaceLease::allocate(
        WorkspaceId::new(format!("ws-{tag}")).expect("id"),
        RepositoryRef::new(format!("repo-local:{tag}")).expect("repo"),
        ExecutionId::new(format!("E-{tag}")).expect("exec"),
        Revision::new(base_oid.to_owned()).expect("revision"),
        BranchRef::new(format!("refs/heads/{branch_short}")).expect("branch"),
        worktree_ref.to_owned(),
    )
    .expect("lease")
}

/// Unique worktree target path that does not exist yet, under the
/// caller-provisioned managed root (sibling of the repo, never nested in the
/// primary checkout); the adapter itself never creates parent directories.
pub fn target_path(fixture: &Fixture, name: &str) -> PathBuf {
    fixture.roots.join(name)
}
