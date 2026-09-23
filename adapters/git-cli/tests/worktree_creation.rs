//! TASK-0041 creation-path integration tests over real temporary Git
//! repositories (offline, no remotes, hermetic fixtures).

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as Ord};

use git_cli::GitCli;
use workspace::{GitPort, WorkspaceFailure, WorktreeStatus};

use common::{anchor_for, git, lease_for, target_path, Fixture};

static NONCE: AtomicU64 = AtomicU64::new(0);
fn fx_nonce() -> u64 {
    NONCE.fetch_add(1, Ord::SeqCst)
}

fn canonical(p: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[test]
fn cv_3_git_state_snapshot_binds_head_tree_and_index() {
    let adapter = GitCli::new();
    let fx = Fixture::new("observed git state");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("state.txt", b"v1");
    git(&fx.dir, &["add", "state.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "state-v1"]);

    let first = adapter
        .git_state_snapshot(&fx.path_str(), &anchor)
        .expect("capture git state");
    assert_eq!(first.head_revision.as_str(), fx.primary_head());
    assert_eq!(
        first.head_tree_oid,
        git(&fx.dir, &["rev-parse", "HEAD^{tree}"])
    );

    fx.write_tracked("state.txt", b"v2");
    git(&fx.dir, &["add", "state.txt"]);
    let second = adapter
        .git_state_snapshot(&fx.path_str(), &anchor)
        .expect("capture changed index");
    assert_eq!(second.head_revision, first.head_revision);
    assert_eq!(second.head_tree_oid, first.head_tree_oid);
    assert_ne!(second.index_manifest, first.index_manifest);
}

#[test]
fn worktree_creation_success_verifies_identity_and_leaves_primary_untouched() {
    let adapter = GitCli::new();
    // Spaces in the fixture path prove argv handling without shell quoting.
    let fx = Fixture::new("success case");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("sentinel.txt", b"primary-bytes-v1");
    git(&fx.dir, &["add", "sentinel.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "sentinel"]);
    let base = fx.primary_head();
    let primary_status_before = fx.primary_status();
    assert_eq!(
        adapter.worktree_status(&fx.path_str(), &anchor).unwrap(),
        WorktreeStatus::Clean
    );

    // Canonical inspection before creation.
    assert!(adapter.is_repository(&fx.path_str(), &anchor).unwrap());
    assert_eq!(
        adapter.default_branch(&fx.path_str(), &anchor).unwrap(),
        fx.default_ref
    );
    let resolved = adapter
        .resolve_revision(&fx.path_str(), "HEAD", &anchor)
        .unwrap();
    assert_eq!(resolved.as_str(), base);

    let target = target_path(&fx, "ws-one");
    let lease = lease_for("success", "agent/e-success", &base, "ref:ws-success");
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .expect("creation must succeed");

    // Second worktree exists and is registered by Git (adapter-independent).
    assert!(target.join("sentinel.txt").is_file());
    let registered = fx.registered_worktrees();
    assert!(
        registered
            .iter()
            .any(|w| canonical(PathBuf::from(w).as_path()) == canonical(&target)),
        "target must be registered: {registered:?}"
    );
    // Managed branch exists; managed HEAD is exactly the base revision.
    assert!(fx.ref_exists("refs/heads/agent/e-success"));
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), base);
    assert_eq!(
        git(&target, &["symbolic-ref", "--quiet", "HEAD"]),
        "refs/heads/agent/e-success"
    );
    // Primary checkout unmoved, files and status untouched.
    assert_eq!(fx.primary_head(), base);
    assert_eq!(fx.read_tracked("sentinel.txt"), b"primary-bytes-v1");
    assert_eq!(fx.primary_status(), primary_status_before);
    assert_eq!(
        adapter.worktree_status(&fx.path_str(), &anchor).unwrap(),
        WorktreeStatus::Clean
    );
}

#[test]
fn managed_worktree_is_isolated_from_primary() {
    let adapter = GitCli::new();
    let fx = Fixture::new("isolation");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("shared.txt", b"v1");
    git(&fx.dir, &["add", "shared.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "shared"]);
    let base = fx.primary_head();

    let target = target_path(&fx, "ws-iso");
    let lease = lease_for("iso", "agent/e-iso", &base, "ref:ws-iso");
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .unwrap();

    // No commit/merge: modify a tracked file inside the new worktree only.
    std::fs::write(target.join("shared.txt"), b"v2-managed").unwrap();
    assert_eq!(
        adapter
            .worktree_status(target.to_str().unwrap(), &anchor)
            .unwrap(),
        WorktreeStatus::DirtyTracked
    );
    assert_eq!(fx.read_tracked("shared.txt"), b"v1");
    assert_eq!(
        adapter.worktree_status(&fx.path_str(), &anchor).unwrap(),
        WorktreeStatus::Clean
    );
}

#[test]
fn multiple_workspaces_stay_isolated() {
    let adapter = GitCli::new();
    let fx = Fixture::new("multi");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("f.txt", b"base");
    git(&fx.dir, &["add", "f.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "f"]);
    let base = fx.primary_head();

    let ta = target_path(&fx, "ws-a");
    let tb = target_path(&fx, "ws-b");
    assert_ne!(ta, tb);
    let la = lease_for("multi-a", "agent/e-a", &base, "ref:ws-a");
    let lb = lease_for("multi-b", "agent/e-b", &base, "ref:ws-b");
    adapter
        .create_worktree(&la, &fx.path_str(), ta.to_str().unwrap(), &anchor)
        .unwrap();
    adapter
        .create_worktree(&lb, &fx.path_str(), tb.to_str().unwrap(), &anchor)
        .unwrap();

    std::fs::write(ta.join("f.txt"), b"edit-a").unwrap();
    assert_eq!(
        adapter
            .worktree_status(ta.to_str().unwrap(), &anchor)
            .unwrap(),
        WorktreeStatus::DirtyTracked
    );
    assert_eq!(
        adapter
            .worktree_status(tb.to_str().unwrap(), &anchor)
            .unwrap(),
        WorktreeStatus::Clean
    );
    assert_eq!(
        adapter.worktree_status(&fx.path_str(), &anchor).unwrap(),
        WorktreeStatus::Clean
    );
    assert_eq!(std::fs::read(tb.join("f.txt")).unwrap(), b"base");
    assert_eq!(fx.read_tracked("f.txt"), b"base");
}

#[test]
fn branch_collision_fails_without_force_or_reuse() {
    let adapter = GitCli::new();
    let fx = Fixture::new("branch-collision");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    // Pre-existing unrelated ref with different content.
    git(&fx.dir, &["branch", "agent/e-taken", &base]);
    // Advance the colliding branch so reuse would be observable.
    git(&fx.dir, &["commit", "-q", "--allow-empty", "-m", "other"]);
    git(&fx.dir, &["branch", "-f", "agent/e-taken", "HEAD"]);
    let taken_oid = git(&fx.dir, &["rev-parse", "agent/e-taken"]);
    let worktrees_before = fx.registered_worktrees().len();

    let target = target_path(&fx, "ws-collide");
    let lease = lease_for("collide", "agent/e-taken", &base, "ref:ws-collide");
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .expect_err("collision must fail");
    assert!(
        matches!(err, WorkspaceFailure::BranchCollision { .. }),
        "expected BranchCollision, got {err:?}"
    );
    // Existing branch unchanged, no new worktree, no target created.
    assert_eq!(git(&fx.dir, &["rev-parse", "agent/e-taken"]), taken_oid);
    assert_eq!(fx.registered_worktrees().len(), worktrees_before);
    assert!(!target.exists());
}

#[test]
fn worktree_path_collision_fails_without_deletion() {
    let adapter = GitCli::new();
    let fx = Fixture::new("path-collision");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();

    let target = target_path(&fx, "ws-blocked");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), b"do-not-delete").unwrap();

    let lease = lease_for("pathcol", "agent/e-pathcol", &base, "ref:ws-pathcol");
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .expect_err("occupied target must fail");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeCollision { .. }),
        "expected WorktreeCollision, got {err:?}"
    );
    // Existing content byte-identical; no branch leaked by the attempt.
    assert_eq!(
        std::fs::read(target.join("keep.txt")).unwrap(),
        b"do-not-delete"
    );
    assert!(!fx.ref_exists("refs/heads/agent/e-pathcol"));
}

#[test]
fn invalid_revision_creates_nothing() {
    let adapter = GitCli::new();
    let fx = Fixture::new("invalid-rev");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let missing_oid = "0".repeat(40);

    let err = adapter
        .resolve_revision(&fx.path_str(), "does-not-exist", &anchor)
        .expect_err("bad ref must fail");
    assert!(
        matches!(err, WorkspaceFailure::InvalidRevision { .. }),
        "got {err:?}"
    );

    let target = target_path(&fx, "ws-badrev");
    let worktrees_before = fx.registered_worktrees().len();
    let lease = lease_for("badrev", "agent/e-badrev", &missing_oid, "ref:ws-badrev");
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .expect_err("missing commit must fail");
    assert!(
        matches!(err, WorkspaceFailure::InvalidRevision { .. }),
        "got {err:?}"
    );
    assert!(!fx.ref_exists("refs/heads/agent/e-badrev"));
    assert!(!target.exists());
    assert_eq!(fx.registered_worktrees().len(), worktrees_before);
}

#[test]
fn bare_repository_fails_explicitly() {
    let adapter = GitCli::new();
    let anchor_dummy_canon = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let anchor = workspace::TrustedCommonGit::from_canonical_commondir(anchor_dummy_canon).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "git-cli-{}-bare-{}",
        std::process::id(),
        fx_nonce()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "--bare"]);
    let path = dir.to_string_lossy().into_owned();

    let err = adapter
        .is_repository(&path, &anchor)
        .expect_err("bare must classify");
    assert!(
        matches!(err, WorkspaceFailure::BareRepositoryUnsupported { .. }),
        "got {err:?}"
    );
    let base = "a".repeat(40);
    let lease = lease_for("bare", "agent/e-bare", &base, "ref:ws-bare");
    let err = adapter
        .create_worktree(&lease, &path, dir.join("wt").to_str().unwrap(), &anchor)
        .expect_err("bare must not create");
    assert!(
        matches!(err, WorkspaceFailure::BareRepositoryUnsupported { .. }),
        "got {err:?}"
    );
    assert!(!dir.join("wt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn not_repository_and_missing_paths_are_typed() {
    let adapter = GitCli::new();
    let anchor_dummy_canon = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let anchor = workspace::TrustedCommonGit::from_canonical_commondir(anchor_dummy_canon).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "git-cli-{}-notrepo-{}",
        std::process::id(),
        fx_nonce()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.to_string_lossy().into_owned();

    assert!(!adapter.is_repository(&path, &anchor).unwrap());
    let err = adapter
        .resolve_revision(&path, "HEAD", &anchor)
        .expect_err("normal dir must fail");
    assert!(
        matches!(err, WorkspaceFailure::NotRepository { .. }),
        "got {err:?}"
    );

    let missing = dir.join("no-such-dir");
    assert!(!adapter
        .is_repository(missing.to_str().unwrap(), &anchor)
        .unwrap());
    let base = "b".repeat(40);
    let lease = lease_for("notrepo", "agent/e-nr", &base, "ref:ws-nr");
    let err = adapter
        .create_worktree(&lease, &path, dir.join("wt").to_str().unwrap(), &anchor)
        .expect_err("non-repo must not create");
    assert!(
        matches!(err, WorkspaceFailure::NotRepository { .. }),
        "got {err:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dirty_primary_neither_blocks_nor_gets_cleaned() {
    let adapter = GitCli::new();
    let fx = Fixture::new("dirty-primary");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("t.txt", b"clean-v1");
    git(&fx.dir, &["add", "t.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "t"]);
    let base = fx.primary_head();
    // Tracked modification + untracked file (ADR-0023: observed, never cleaned).
    fx.write_tracked("t.txt", b"dirty-v2");
    std::fs::write(fx.dir.join("untracked-evidence.txt"), b"evidence").unwrap();
    let dirty_before = fx.read_tracked("t.txt");

    let target = target_path(&fx, "ws-dirty");
    let lease = lease_for("dirty", "agent/e-dirty", &base, "ref:ws-dirty");
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .expect("dirty primary must not block creation");

    assert_eq!(fx.read_tracked("t.txt"), dirty_before);
    assert_eq!(
        std::fs::read(fx.dir.join("untracked-evidence.txt")).unwrap(),
        b"evidence"
    );
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), base);
}

#[test]
fn protected_branch_uses_semantics_not_literals() {
    let adapter = GitCli::new();
    let mut fx = Fixture::new("protected");
    let anchor = anchor_for(&adapter, &fx.path_str());
    // Non-standard default-branch name: no main/master literal involved.
    fx.rename_default("trunkline");
    assert_eq!(
        adapter.default_branch(&fx.path_str(), &anchor).unwrap(),
        "refs/heads/trunkline"
    );
    let base = fx.primary_head();
    let worktrees_before = fx.registered_worktrees().len();

    let target = target_path(&fx, "ws-protected");
    let lease = lease_for("prot", "trunkline", &base, "ref:ws-prot");
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap(), &anchor)
        .expect_err("protected branch must be refused");
    assert!(
        matches!(err, WorkspaceFailure::BranchCollision { .. }),
        "expected BranchCollision naming the protection, got {err:?}"
    );
    assert!(matches!(
        adapter.branch_exists(&fx.path_str(), &lease.branch().clone(), &anchor),
        Ok(true)
    ));
    assert_eq!(fx.registered_worktrees().len(), worktrees_before);
    assert!(!target.exists());
}

#[test]
fn repository_inspection_reports_canonical_fields() {
    let adapter = GitCli::new();
    let fx = Fixture::new("inspect");
    let anchor = anchor_for(&adapter, &fx.path_str());

    assert!(adapter.is_repository(&fx.path_str(), &anchor).unwrap());
    let sub = fx.dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    assert!(adapter
        .is_repository(sub.to_str().unwrap(), &anchor)
        .unwrap());

    let head = adapter
        .resolve_revision(&fx.path_str(), "HEAD", &anchor)
        .unwrap();
    assert_eq!(head.as_str(), fx.base_oid);
    // Short OID input still resolves to the immutable full OID.
    let short = &fx.base_oid[..12];
    assert_eq!(
        adapter
            .resolve_revision(&fx.path_str(), short, &anchor)
            .unwrap()
            .as_str(),
        fx.base_oid
    );
    assert_eq!(
        adapter.default_branch(&fx.path_str(), &anchor).unwrap(),
        fx.default_ref
    );
    let present = workspace::BranchRef::new(fx.default_ref.clone()).unwrap();
    assert!(adapter
        .branch_exists(&fx.path_str(), &present, &anchor)
        .unwrap());
    let absent = workspace::BranchRef::new("refs/heads/no-such-branch-here").unwrap();
    assert!(!adapter
        .branch_exists(&fx.path_str(), &absent, &anchor)
        .unwrap());
    assert_eq!(
        adapter.worktree_status(&fx.path_str(), &anchor).unwrap(),
        WorktreeStatus::Clean
    );
    assert_eq!(
        adapter
            .worktree_status(fx.dir.join("missing-wt").to_str().unwrap(), &anchor)
            .unwrap(),
        WorktreeStatus::Missing
    );
}
