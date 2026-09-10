//! TASK-0044 adapter failure matrix: repository inspection, revision
//! resolution, and path/ownership edge cases over real temporary Git
//! repositories (offline, no remotes, hermetic fixtures).
//!
//! Complements `worktree_creation.rs` (happy path + core collisions),
//! `safety_regression.rs` (argv/injection argv safety), and
//! `diff_cleanup.rs` (diff + removal): this binary pins the remaining
//! required G4 rows — Unicode repository paths, linked-worktree `.git`
//! files, indeterminate default-branch metadata (fail closed, no guessing),
//! the full revision-resolution matrix, branch-at-unexpected-revision,
//! symlink/missing-parent targets, untracked-vs-tracked dirt ranking, and
//! primary-checkout preservation across every failure. Every repository is
//! test-owned; `Drop` reaps the whole fixture tree (test teardown, never
//! product cleanup semantics).

mod common;

use git_cli::GitCli;
use workspace::{GitPort, WorkspaceFailure, WorktreeStatus};

use common::{git, lease_for, target_path, Fixture};

#[test]
fn repository_path_with_unicode_inspects_and_creates() {
    let adapter = GitCli::new();
    let fx = Fixture::new("ünicode-répo-日本語");
    assert!(
        adapter.is_repository(&fx.path_str()).unwrap(),
        "unicode repo path must inspect as a repository"
    );
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-unicode");
    let lease = lease_for(
        "unicode",
        "agent/e-unicode",
        &base,
        target.to_str().unwrap(),
    );
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .unwrap();
    assert!(
        target.is_dir(),
        "worktree must exist at the unicode sibling path"
    );
    let target_canon = std::fs::canonicalize(&target).unwrap_or(target.clone());
    assert!(
        fx.registered_worktrees()
            .iter()
            .any(|w| { std::fs::canonicalize(w).unwrap_or_else(|_| w.into()) == target_canon }),
        "worktree must be registered: {:?}",
        fx.registered_worktrees()
    );
}

#[test]
fn linked_worktree_dotgit_file_counts_as_repository() {
    // A linked worktree carries a `.git` *file*, not a directory. Inspection
    // must not rely on `.git`-directory heuristics.
    let adapter = GitCli::new();
    let fx = Fixture::new("linked-dotgit");
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-linked");
    let lease = lease_for("linked", "agent/e-linked", &base, target.to_str().unwrap());
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .unwrap();
    let dotgit = target.join(".git");
    assert!(dotgit.is_file(), "linked worktree must carry a .git file");
    let target_str = target.to_str().unwrap().to_owned();
    assert!(
        adapter.is_repository(&target_str).unwrap(),
        ".git-file worktree must inspect as a repository"
    );
    assert_eq!(
        adapter.worktree_status(&target_str).unwrap(),
        WorktreeStatus::Clean
    );
}

#[test]
fn default_branch_without_metadata_fails_closed_without_guessing() {
    let adapter = GitCli::new();
    let fx = Fixture::new("no-origin-head");
    // Remove the single canonical metadata fact.
    git(&fx.dir, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]);
    let err = adapter
        .default_branch(&fx.path_str())
        .expect_err("indeterminate default branch must fail");
    let detail = err.to_string();
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "indeterminate metadata must be a backend failure, got {err:?}"
    );
    assert!(
        !detail.contains("main") && !detail.contains("master"),
        "failure must not guess a branch name, got {detail:?}"
    );
    // Creation does not block on indeterminate metadata: the managed
    // `agent/` branch cannot be the default by construction, and refusing
    // all creation would trade availability for no safety gain.
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-indeterminate");
    let lease = lease_for(
        "indeterminate",
        "agent/e-indeterminate",
        &base,
        target.to_str().unwrap(),
    );
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .unwrap();
}

#[test]
fn resolve_revision_matrix() {
    let adapter = GitCli::new();
    let fx = Fixture::new("rev-matrix");
    let repo = fx.path_str();
    let head = fx.primary_head();

    // Valid forms resolve to the exact immutable OID.
    assert_eq!(
        adapter.resolve_revision(&repo, &head).unwrap().as_str(),
        head
    );
    assert_eq!(
        adapter.resolve_revision(&repo, "HEAD").unwrap().as_str(),
        head
    );
    assert_eq!(
        adapter
            .resolve_revision(&repo, &fx.branch)
            .unwrap()
            .as_str(),
        head
    );
    assert_eq!(
        adapter
            .resolve_revision(&repo, &head[..12])
            .unwrap()
            .as_str(),
        head,
        "short OID must resolve to the full OID"
    );

    // Invalid forms fail as InvalidRevision with no fallback.
    for bad in [
        "does-not-exist-anywhere",
        "",
        "   ",
        "HEAD^{tree}",
        "ffffffffffffffffffffffffffffffffffffffff",
    ] {
        let err = adapter
            .resolve_revision(&repo, bad)
            .expect_err(&format!("revision {bad:?} must fail"));
        assert!(
            matches!(err, WorkspaceFailure::InvalidRevision { .. }),
            "revision {bad:?} must be InvalidRevision, got {err:?}"
        );
    }
}

#[test]
fn branch_at_unexpected_revision_is_collision_not_reuse() {
    let adapter = GitCli::new();
    let fx = Fixture::new("unexpected-rev");
    let first = fx.primary_head();
    // Plant the managed branch name at a *different* commit.
    git(&fx.dir, &["commit", "-q", "--allow-empty", "-m", "second"]);
    let second = fx.primary_head();
    assert_ne!(first, second);
    git(&fx.dir, &["branch", "agent/e-planted", "HEAD"]);
    // Back the primary branch up so the lease base differs from the plant.
    git(&fx.dir, &["checkout", "-q", &first]);

    let target = target_path(&fx, "ws-planted");
    let lease = lease_for(
        "planted",
        "agent/e-planted",
        &first,
        target.to_str().unwrap(),
    );
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .expect_err("branch at unexpected revision must collide");
    assert!(
        matches!(err, WorkspaceFailure::BranchCollision { .. }),
        "unexpected-revision branch must be BranchCollision, got {err:?}"
    );
    // No reuse: the planted ref still points at the unexpected commit, and
    // no worktree was registered for the target.
    assert_eq!(git(&fx.dir, &["rev-parse", "agent/e-planted"]), second);
    assert!(!target.exists());
    assert!(!fx
        .registered_worktrees()
        .iter()
        .any(|w| w == target.to_str().unwrap()));
}

#[test]
fn symlinked_target_is_collision_not_escape() {
    let adapter = GitCli::new();
    let fx = Fixture::new("symlink-target");
    let base = fx.primary_head();
    // A symlink under the managed root pointing outside must never be
    // followed into an overwrite: the pre-existence check fires first.
    let outside = fx.case_dir.join("outside-target");
    std::fs::create_dir_all(&outside).expect("outside dir");
    let link = target_path(&fx, "ws-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&outside, &link).expect("symlink");
    let lease = lease_for("link", "agent/e-link", &base, link.to_str().unwrap());
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), link.to_str().unwrap())
        .expect_err("symlinked target must be refused");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeCollision { .. }),
        "symlinked target must be WorktreeCollision, got {err:?}"
    );
    // The link itself is untouched and the outside directory gained no Git
    // registration.
    assert!(link.is_symlink());
    assert!(
        fx.registered_worktrees().is_empty()
            || !fx
                .registered_worktrees()
                .iter()
                .any(|w| w.contains("outside"))
    );
}

#[test]
fn target_with_missing_parent_is_typed_backend_failure() {
    let adapter = GitCli::new();
    let fx = Fixture::new("missing-parent");
    let base = fx.primary_head();
    let target = fx.roots.join("no-such-dir").join("ws-orphan");
    let lease = lease_for("orphan", "agent/e-orphan", &base, target.to_str().unwrap());
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .expect_err("missing parent must fail");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "missing parent must be GitBackendFailure, got {err:?}"
    );
    assert!(!fx.ref_exists("refs/heads/agent/e-orphan"));
}

#[test]
fn worktree_status_ranks_untracked_above_tracked() {
    let adapter = GitCli::new();
    let fx = Fixture::new("dirt-ranking");
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-dirt");
    let lease = lease_for("dirt", "agent/e-dirt", &base, target.to_str().unwrap());
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .unwrap();
    let target_str = target.to_str().unwrap().to_owned();
    // Tracked file first: DirtyTracked.
    git(&target, &["commit", "-q", "--allow-empty", "-m", "seed"]);
    std::fs::write(target.join("seed.txt"), b"v1").expect("seed file");
    git(&target, &["add", "seed.txt"]);
    git(&target, &["commit", "-q", "-m", "seed"]);
    std::fs::write(target.join("seed.txt"), b"v2").expect("dirty tracked");
    assert_eq!(
        adapter.worktree_status(&target_str).unwrap(),
        WorktreeStatus::DirtyTracked
    );
    // Adding one untracked file must escalate to DirtyUntracked.
    std::fs::write(target.join("notes.txt"), b"untracked").expect("untracked file");
    assert_eq!(
        adapter.worktree_status(&target_str).unwrap(),
        WorktreeStatus::DirtyUntracked,
        "untracked evidence must rank above tracked dirt"
    );
}

#[test]
fn primary_checkout_survives_every_matrix_failure() {
    let adapter = GitCli::new();
    let fx = Fixture::new("primary-preserved");
    let base = fx.primary_head();
    let status_before = fx.primary_status();
    // Dirty the primary first: failures must neither clean nor reset it.
    // (A brand-new file is untracked dirt — still protected, still blocking
    // nothing, never cleaned.)
    fx.write_tracked("user.txt", b"user data");
    let dirty_bytes = fx.read_tracked("user.txt");
    assert_eq!(
        adapter.worktree_status(&fx.path_str()).unwrap(),
        WorktreeStatus::DirtyUntracked
    );

    // Collision failure.
    git(&fx.dir, &["branch", "agent/e-clash", "HEAD"]);
    let clash_target = target_path(&fx, "ws-clash");
    let clash = lease_for(
        "clash",
        "agent/e-clash",
        &base,
        clash_target.to_str().unwrap(),
    );
    let _ = adapter.create_worktree(&clash, &fx.path_str(), clash_target.to_str().unwrap());

    // Invalid-revision failure.
    let bad_target = target_path(&fx, "ws-badrev");
    let bad = lease_for(
        "badrev",
        "agent/e-badrev",
        &"0".repeat(40),
        bad_target.to_str().unwrap(),
    );
    let _ = adapter.create_worktree(&bad, &fx.path_str(), bad_target.to_str().unwrap());

    assert_eq!(fx.primary_head(), base, "primary HEAD must not move");
    assert_eq!(
        fx.read_tracked("user.txt"),
        dirty_bytes,
        "primary bytes identical"
    );
    assert_ne!(
        status_before,
        fx.primary_status(),
        "sanity: primary is dirty"
    );
    assert!(
        fx.primary_status().contains("user.txt"),
        "primary dirt must be the user's file, untouched"
    );
}

#[test]
fn failure_taxonomy_stays_bounded() {
    // Every exercised matrix failure must map to the documented bounded
    // kind set — one class per cause family, never one per stderr string.
    let adapter = GitCli::new();
    let fx = Fixture::new("bounded");
    git(&fx.dir, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]);
    let indeterminate = adapter.default_branch(&fx.path_str()).unwrap_err();
    assert_eq!(indeterminate.kind(), "git_backend_failure");
    let invalid = adapter
        .resolve_revision(&fx.path_str(), "nope")
        .unwrap_err();
    assert_eq!(invalid.kind(), "invalid_revision");
    let not_repo = adapter.is_repository("/definitely/not/a/repo/path-xyz");
    assert!(matches!(not_repo, Ok(false)));
    for failure in [indeterminate, invalid] {
        assert!(
            failure.to_string().len() <= 600,
            "diagnostic must stay bounded, got {}",
            failure.to_string().len()
        );
    }
}
