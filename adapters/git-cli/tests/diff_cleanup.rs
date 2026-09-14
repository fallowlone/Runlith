//! TASK-0043 adapter matrix: base-revision diff evidence plus safe
//! non-forced worktree/branch removal over real temporary Git repositories.
//!
//! Read-only diff tests prove the baseline is the recorded `base_revision`
//! (never the live default tip), every material change class is visible,
//! and observation never mutates Git. Removal tests prove dirty, detached,
//! mismatched, default, and unmerged resources survive ordinary cleanup,
//! while clean merged resources remove idempotently.

mod common;

use std::path::PathBuf;

use git_cli::GitCli;
use workspace::{DiffChangeKind, GitPort, WorkspaceFailure, WorktreeStatus};

use common::{anchor_for, git, lease_for, target_path, Fixture};

fn lease_at(
    tag: &str,
    branch_short: &str,
    base: &str,
    target: &std::path::Path,
) -> workspace::WorkspaceLease {
    lease_for(tag, branch_short, base, target.to_str().unwrap())
}

fn canonical(p: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn worktree_head(target: &std::path::Path) -> String {
    git(target, &["rev-parse", "HEAD"])
}

fn worktree_branch(target: &std::path::Path) -> String {
    git(target, &["symbolic-ref", "--quiet", "HEAD"])
}

// -- diff --------------------------------------------------------------------

#[test]
fn diff_clean_worktree_is_empty_with_authoritative_base() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-clean");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-clean");
    adapter
        .create_worktree(
            &lease_for("dc-clean", "agent/e-dc-clean", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    let lease = lease_at("dc-clean", "agent/e-dc-clean", &base, &target);

    let before_head = worktree_head(&target);
    let before_branch = worktree_branch(&target);
    let before_status = adapter
        .worktree_status(target.to_str().unwrap(), &anchor)
        .unwrap();

    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert_eq!(evidence.diff.base_revision.as_str(), base);
    assert_eq!(evidence.diff.head_revision.as_ref().unwrap().as_str(), base);
    assert!(!evidence.diff.has_working_tree_changes);
    assert_eq!(evidence.diff.status, WorktreeStatus::Clean);
    assert!(evidence.diff.committed.is_empty());
    assert!(evidence.diff.working.is_empty());
    assert!(!evidence.diff.truncated);
    assert!(!evidence.summary.is_empty());

    // Observation mutates nothing.
    assert_eq!(worktree_head(&target), before_head);
    assert_eq!(worktree_branch(&target), before_branch);
    assert_eq!(
        adapter
            .worktree_status(target.to_str().unwrap(), &anchor)
            .unwrap(),
        before_status
    );
    assert_eq!(fx.primary_head(), base);
}

#[test]
fn diff_detects_tracked_unstaged_modification() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-tracked");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("tracked.txt", b"v1");
    git(&fx.dir, &["add", "tracked.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "tracked"]);
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-tracked");
    adapter
        .create_worktree(
            &lease_for("dc-tr", "agent/e-dc-tr", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::write(target.join("tracked.txt"), b"v2-managed").unwrap();

    let lease = lease_at("dc-tr", "agent/e-dc-tr", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert!(evidence.diff.has_working_tree_changes);
    assert_eq!(evidence.diff.status, WorktreeStatus::DirtyTracked);
    assert!(evidence.diff.committed.is_empty());
    assert!(
        evidence
            .diff
            .working
            .iter()
            .any(|e| e.path == "tracked.txt" && e.kind == DiffChangeKind::Modified),
        "tracked modification must be visible: {:?}",
        evidence.diff.working
    );
    assert_eq!(fx.read_tracked("tracked.txt"), b"v1");
}

#[test]
fn diff_detects_staged_change() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-staged");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-staged");
    adapter
        .create_worktree(
            &lease_for("dc-st", "agent/e-dc-st", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::write(target.join("staged-new.txt"), b"staged-bytes").unwrap();
    git(&target, &["add", "staged-new.txt"]);

    let lease = lease_at("dc-st", "agent/e-dc-st", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert!(evidence.diff.has_working_tree_changes);
    assert_eq!(evidence.diff.status, WorktreeStatus::DirtyTracked);
    assert!(
        evidence
            .diff
            .working
            .iter()
            .any(|e| e.path == "staged-new.txt" && e.kind == DiffChangeKind::Added),
        "staged addition must be visible: {:?}",
        evidence.diff.working
    );
}

#[test]
fn diff_detects_untracked_file() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-untracked");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-untracked");
    adapter
        .create_worktree(
            &lease_for("dc-un", "agent/e-dc-un", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::write(target.join("output-evidence.txt"), b"untracked-bytes").unwrap();

    let lease = lease_at("dc-un", "agent/e-dc-un", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert!(evidence.diff.has_working_tree_changes);
    assert_eq!(evidence.diff.status, WorktreeStatus::DirtyUntracked);
    assert!(
        evidence
            .diff
            .working
            .iter()
            .any(|e| e.path == "output-evidence.txt" && e.kind == DiffChangeKind::Untracked),
        "untracked file must be visible: {:?}",
        evidence.diff.working
    );
}

#[test]
fn diff_detects_deleted_tracked_file() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-deleted");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("doomed.txt", b"bye");
    git(&fx.dir, &["add", "doomed.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "doomed"]);
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-deleted");
    adapter
        .create_worktree(
            &lease_for("dc-del", "agent/e-dc-del", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::remove_file(target.join("doomed.txt")).unwrap();

    let lease = lease_at("dc-del", "agent/e-dc-del", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert!(evidence.diff.has_working_tree_changes);
    assert!(
        evidence
            .diff
            .working
            .iter()
            .any(|e| e.path == "doomed.txt" && e.kind == DiffChangeKind::Deleted),
        "deletion must be visible: {:?}",
        evidence.diff.working
    );
}

#[test]
fn diff_represents_head_advanced_from_base() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-head");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("base.txt", b"base");
    git(&fx.dir, &["add", "base.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "base"]);
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-head");
    adapter
        .create_worktree(
            &lease_for("dc-head", "agent/e-dc-head", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    // Execution work as a real commit on the managed branch.
    std::fs::write(target.join("work.txt"), b"execution-work").unwrap();
    git(&target, &["add", "work.txt"]);
    git(&target, &["commit", "-q", "-m", "execution work"]);
    let head = worktree_head(&target);
    assert_ne!(head, base);

    let lease = lease_at("dc-head", "agent/e-dc-head", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    // The baseline stays the original recorded revision, not the new HEAD.
    assert_eq!(evidence.diff.base_revision.as_str(), base);
    assert_eq!(evidence.diff.head_revision.as_ref().unwrap().as_str(), head);
    assert!(
        evidence
            .diff
            .committed
            .iter()
            .any(|e| e.path == "work.txt" && e.kind == DiffChangeKind::Added),
        "committed work must be visible: {:?}",
        evidence.diff.committed
    );
    assert!(evidence.diff.working.is_empty());
}

#[test]
fn diff_reports_renames_where_git_reports_them() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-rename");
    let anchor = anchor_for(&adapter, &fx.path_str());
    // Large similar body so rename detection fires.
    let body = "rename-similarity-body\n".repeat(40);
    std::fs::write(fx.dir.join("original.txt"), body.as_bytes()).unwrap();
    git(&fx.dir, &["add", "original.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "original"]);
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-diff-rename");
    adapter
        .create_worktree(
            &lease_for("dc-ren", "agent/e-dc-ren", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    git(&target, &["mv", "original.txt", "renamed.txt"]);
    git(&target, &["commit", "-q", "-m", "rename"]);

    let lease = lease_at("dc-ren", "agent/e-dc-ren", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert!(
        evidence
            .diff
            .committed
            .iter()
            .any(|e| e.kind == DiffChangeKind::Renamed),
        "rename must be visible where Git reports it: {:?}",
        evidence.diff.committed
    );
}

#[test]
fn diff_is_scoped_to_the_requested_workspace() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-scope");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("shared.txt", b"base");
    git(&fx.dir, &["add", "shared.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "shared"]);
    let base = fx.primary_head();
    let ta = target_path(&fx, "ws-scope-a");
    let tb = target_path(&fx, "ws-scope-b");
    adapter
        .create_worktree(
            &lease_for("dc-sa", "agent/e-dc-sa", &base, "ref:unused"),
            &fx.path_str(),
            ta.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    adapter
        .create_worktree(
            &lease_for("dc-sb", "agent/e-dc-sb", &base, "ref:unused"),
            &fx.path_str(),
            tb.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::write(ta.join("shared.txt"), b"edit-a").unwrap();
    std::fs::write(tb.join("only-b.txt"), b"b-work").unwrap();
    // Primary checkout change must never leak into a workspace diff.
    fx.write_tracked("shared.txt", b"primary-edit");

    let la = lease_at("dc-sa", "agent/e-dc-sa", &base, &ta);
    let lb = lease_at("dc-sb", "agent/e-dc-sb", &base, &tb);
    let ea = adapter.compute_diff(&la, &anchor).unwrap();
    let eb = adapter.compute_diff(&lb, &anchor).unwrap();
    assert!(ea.diff.working.iter().any(|e| e.path == "shared.txt"));
    assert!(!ea.diff.working.iter().any(|e| e.path == "only-b.txt"));
    assert!(eb.diff.working.iter().any(|e| e.path == "only-b.txt"));
    assert!(!eb.diff.working.iter().any(|e| e.path == "shared.txt"));
    assert_eq!(std::fs::read(tb.join("shared.txt")).unwrap(), b"base");
}

#[test]
fn diff_handles_spaces_and_unicode_paths() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-ünicode paths");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws with spaces-日本語");
    adapter
        .create_worktree(
            &lease_for("dc-sp", "agent/e-dc-sp", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::write(target.join("file with spaces.txt"), b"sp").unwrap();
    std::fs::write(target.join("ユニコード.txt"), b"uni").unwrap();

    let lease = lease_at("dc-sp", "agent/e-dc-sp", &base, &target);
    let evidence = adapter.compute_diff(&lease, &anchor).unwrap();
    assert_eq!(evidence.diff.status, WorktreeStatus::DirtyUntracked);
    assert!(
        evidence
            .diff
            .working
            .iter()
            .any(|e| e.path == "file with spaces.txt"),
        "space path must survive argv safely: {:?}",
        evidence.diff.working
    );
    assert!(
        evidence
            .diff
            .working
            .iter()
            .any(|e| e.path == "ユニコード.txt"),
        "unicode path must survive argv safely: {:?}",
        evidence.diff.working
    );
}

#[test]
fn diff_on_absent_or_detached_worktree_fails_typed() {
    let adapter = GitCli::new();
    let fx = Fixture::new("diff-missing");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let missing = target_path(&fx, "ws-never-created");
    let lease = lease_at("dc-miss", "agent/e-dc-miss", &base, &missing);
    let err = adapter
        .compute_diff(&lease, &anchor)
        .expect_err("absent must fail");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeMissing { .. }),
        "got {err:?}"
    );

    let target = target_path(&fx, "ws-detach-diff");
    adapter
        .create_worktree(
            &lease_for("dc-det", "agent/e-dc-det", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    git(&target, &["checkout", "-q", "--detach", "HEAD"]);
    let lease = lease_at("dc-det", "agent/e-dc-det", &base, &target);
    let err = adapter
        .compute_diff(&lease, &anchor)
        .expect_err("detached must fail");
    assert!(
        matches!(err, WorkspaceFailure::CheckoutFailed { .. }),
        "got {err:?}"
    );
}

// -- removal ---------------------------------------------------------------

#[test]
fn clean_worktree_removes_then_branch_deletes() {
    let adapter = GitCli::new();
    let fx = Fixture::new("remove-clean");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-remove-clean");
    adapter
        .create_worktree(
            &lease_for("rm-clean", "agent/e-rm-clean", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    let lease = lease_at("rm-clean", "agent/e-rm-clean", &base, &target);

    adapter
        .remove_worktree(&lease, target.to_str().unwrap(), &anchor)
        .unwrap();
    assert!(!target.exists());
    assert!(!fx
        .registered_worktrees()
        .iter()
        .any(|w| canonical(PathBuf::from(w).as_path()) == canonical(&target)));
    assert!(fx.ref_exists("refs/heads/agent/e-rm-clean"));

    let branch = workspace::BranchRef::new("refs/heads/agent/e-rm-clean").unwrap();
    adapter
        .remove_branch(&fx.path_str(), &branch, &anchor)
        .unwrap();
    assert!(!fx.ref_exists("refs/heads/agent/e-rm-clean"));
}

#[test]
fn dirty_worktrees_are_preserved_without_force() {
    let adapter = GitCli::new();
    for (tag, setup) in [
        ("tracked", "tracked"),
        ("staged", "staged"),
        ("untracked", "untracked"),
    ] {
        let fx = Fixture::new(&format!("remove-dirty-{tag}"));
        let anchor = anchor_for(&adapter, &fx.path_str());
        fx.write_tracked("keep.txt", b"v1");
        git(&fx.dir, &["add", "keep.txt"]);
        git(&fx.dir, &["commit", "-q", "-m", "keep"]);
        let base = fx.primary_head();
        let target = target_path(&fx, "ws-dirty");
        adapter
            .create_worktree(
                &lease_for("rm-d", "agent/e-rm-d", &base, "ref:unused"),
                &fx.path_str(),
                target.to_str().unwrap(),
                &anchor,
            )
            .unwrap();
        match setup {
            "tracked" => std::fs::write(target.join("keep.txt"), b"v2").unwrap(),
            "staged" => {
                std::fs::write(target.join("staged.txt"), b"s").unwrap();
                git(&target, &["add", "staged.txt"]);
            }
            _ => std::fs::write(target.join("untracked.txt"), b"u").unwrap(),
        }
        let kind = if setup == "untracked" {
            "untracked.txt"
        } else {
            "keep.txt"
        };
        let lease = lease_at("rm-d", "agent/e-rm-d", &base, &target);
        let err = adapter
            .remove_worktree(&lease, target.to_str().unwrap(), &anchor)
            .expect_err("dirty removal must refuse");
        assert!(
            matches!(
                err,
                WorkspaceFailure::WorktreeDirty { .. } | WorkspaceFailure::CleanupUnsafe { .. }
            ),
            "case {tag}: got {err:?}"
        );
        // Data preserved byte-identical; registration and branch intact.
        assert!(std::fs::read(target.join(kind)).is_ok());
        if setup == "untracked" {
            assert_eq!(std::fs::read(target.join("untracked.txt")).unwrap(), b"u");
        } else if setup == "tracked" {
            assert_eq!(std::fs::read(target.join("keep.txt")).unwrap(), b"v2");
        }
        assert!(target.is_dir());
        assert!(fx.ref_exists("refs/heads/agent/e-rm-d"));
    }
}

#[test]
fn branch_mismatch_and_detached_head_fail_closed() {
    let adapter = GitCli::new();
    let fx = Fixture::new("remove-mismatch");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-mismatch");
    adapter
        .create_worktree(
            &lease_for("rm-mm", "agent/e-rm-mm", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    git(&target, &["checkout", "-q", "--detach", "HEAD"]);
    let lease = lease_at("rm-mm", "agent/e-rm-mm", &base, &target);
    let err = adapter
        .remove_worktree(&lease, target.to_str().unwrap(), &anchor)
        .expect_err("detached must fail closed");
    assert!(
        matches!(
            err,
            WorkspaceFailure::OwnershipMismatch { .. } | WorkspaceFailure::CheckoutFailed { .. }
        ),
        "got {err:?}"
    );
    assert!(target.is_dir());
}

#[test]
fn unmerged_branch_survives_non_forced_deletion() {
    let adapter = GitCli::new();
    let fx = Fixture::new("remove-unmerged");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("f.txt", b"base");
    git(&fx.dir, &["add", "f.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "f"]);
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-unmerged");
    adapter
        .create_worktree(
            &lease_for("rm-un", "agent/e-rm-un", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    // Unique commit on the managed branch (clean worktree afterwards).
    std::fs::write(target.join("unique.txt"), b"unique-work").unwrap();
    git(&target, &["add", "unique.txt"]);
    git(&target, &["commit", "-q", "-m", "unique"]);
    let lease = lease_at("rm-un", "agent/e-rm-un", &base, &target);

    adapter
        .remove_worktree(&lease, target.to_str().unwrap(), &anchor)
        .unwrap();
    assert!(!target.exists());
    let branch = workspace::BranchRef::new("refs/heads/agent/e-rm-un").unwrap();
    let err = adapter
        .remove_branch(&fx.path_str(), &branch, &anchor)
        .expect_err("unmerged branch must survive");
    assert!(
        matches!(err, WorkspaceFailure::CleanupUnsafe { .. }),
        "got {err:?}"
    );
    assert!(fx.ref_exists("refs/heads/agent/e-rm-un"));
}

#[test]
fn default_branch_is_never_deleted() {
    let adapter = GitCli::new();
    let mut fx = Fixture::new("remove-default");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.rename_default("trunkline");
    let branch = workspace::BranchRef::new("refs/heads/trunkline").unwrap();
    let err = adapter
        .remove_branch(&fx.path_str(), &branch, &anchor)
        .expect_err("default must survive");
    assert!(
        matches!(err, WorkspaceFailure::CleanupUnsafe { .. }),
        "got {err:?}"
    );
    assert!(fx.ref_exists("refs/heads/trunkline"));
}

#[test]
fn absent_branch_deletion_is_idempotent() {
    let adapter = GitCli::new();
    let fx = Fixture::new("remove-absent");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let branch = workspace::BranchRef::new("refs/heads/agent/e-never-existed").unwrap();
    let err = adapter
        .remove_branch(&fx.path_str(), &branch, &anchor)
        .expect_err("absent must signal idempotency");
    assert!(
        matches!(err, WorkspaceFailure::AlreadyAbsent),
        "got {err:?}"
    );
}

#[test]
fn removal_leaves_siblings_and_primary_untouched() {
    let adapter = GitCli::new();
    let fx = Fixture::new("remove-isolation");
    let anchor = anchor_for(&adapter, &fx.path_str());
    fx.write_tracked("f.txt", b"base");
    git(&fx.dir, &["add", "f.txt"]);
    git(&fx.dir, &["commit", "-q", "-m", "f"]);
    let base = fx.primary_head();
    let ta = target_path(&fx, "ws-iso-a");
    let tb = target_path(&fx, "ws-iso-b");
    adapter
        .create_worktree(
            &lease_for("rm-ia", "agent/e-rm-ia", &base, "ref:unused"),
            &fx.path_str(),
            ta.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    adapter
        .create_worktree(
            &lease_for("rm-ib", "agent/e-rm-ib", &base, "ref:unused"),
            &fx.path_str(),
            tb.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    std::fs::write(tb.join("f.txt"), b"edit-b").unwrap();
    let sibling_before = std::fs::read(tb.join("f.txt")).unwrap();
    let primary_before = fx.read_tracked("f.txt");

    let la = lease_at("rm-ia", "agent/e-rm-ia", &base, &ta);
    adapter
        .remove_worktree(&la, ta.to_str().unwrap(), &anchor)
        .unwrap();
    let branch_a = workspace::BranchRef::new("refs/heads/agent/e-rm-ia").unwrap();
    adapter
        .remove_branch(&fx.path_str(), &branch_a, &anchor)
        .unwrap();

    assert!(!ta.exists());
    assert_eq!(std::fs::read(tb.join("f.txt")).unwrap(), sibling_before);
    assert_eq!(
        adapter
            .worktree_status(tb.to_str().unwrap(), &anchor)
            .unwrap(),
        WorktreeStatus::DirtyTracked
    );
    assert!(fx.ref_exists("refs/heads/agent/e-rm-ib"));
    assert_eq!(fx.read_tracked("f.txt"), primary_before);
    assert_eq!(fx.primary_head(), base);
}

// -- worktree registration binding probe -------------------------------------

#[test]
fn is_registered_worktree_binds_lease_to_calling_repository() {
    let adapter = GitCli::new();
    let fx = Fixture::new("reg-bind");
    let anchor = anchor_for(&adapter, &fx.path_str());
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-reg-bind");
    adapter
        .create_worktree(
            &lease_for("reg-bind", "agent/e-reg-bind", &base, "ref:unused"),
            &fx.path_str(),
            target.to_str().unwrap(),
            &anchor,
        )
        .unwrap();
    let target_str = target.to_str().unwrap();
    // Own repository positively registers the worktree.
    assert!(adapter
        .is_registered_worktree(&fx.path_str(), target_str, &anchor)
        .unwrap());
    // An unrelated valid repository does not — the F1 wrong-repo shape.
    // Uses that repository's own anchor so the check validates its view;
    // a lease-bound anchor for the true repository would fail closed as
    // GitBackendFailure here (proven by F7 keepers), which the service also
    // treats as binding refusal.
    let other = Fixture::new("reg-bind-other");
    let anchor_other = anchor_for(&adapter, &other.path_str());
    assert!(!adapter
        .is_registered_worktree(&other.path_str(), target_str, &anchor_other)
        .unwrap());
    // Backend errors fail closed, never as assumed-absent.
    assert!(adapter
        .is_registered_worktree("/no/such/repo", target_str, &anchor)
        .is_err());
}
