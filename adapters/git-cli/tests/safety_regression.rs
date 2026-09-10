//! TASK-0041 argv/shell-injection regression, unavailable-Git handling,
//! deferred-operation refusal, and Unicode/space path coverage.

mod common;

use git_cli::GitCli;
use workspace::{GitPort, WorkspaceFailure, WorktreeStatus};

use common::{git, lease_for, target_path, Fixture};

/// Every value below must either fail typed validation or reach Git as one
/// literal argv element — never shell interpretation. A shell would execute
/// the `;`/`$()`/backtick payloads with cwd inside the fixture; success here
/// means the exact ref was created and no stray files appeared.
#[test]
fn shell_metachar_branch_names_stay_literal_argv() {
    let adapter = GitCli::new();
    let fx = Fixture::new("injection");
    let base = fx.primary_head();
    let entries_before = fixture_entry_names(&fx);

    for (tag, short) in [
        ("semi", "feat;touch-CANARY-SEMI"),
        ("dollar", "feat-$(touch-CANARY-DOLLAR)"),
        ("backtick", "feat-`touch-CANARY-BT`"),
        ("quotes", "feat-'single'-\"double\""),
        ("pipe", "feat-a|b"),
        ("unicode", "fëature-日本語-🚀"),
    ] {
        let target = target_path(&fx, &format!("ws-inj-{tag}"));
        let lease = lease_for(
            &format!("inj-{tag}"),
            short,
            &base,
            &format!("ref:ws-inj-{tag}"),
        );
        adapter
            .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
            .unwrap_or_else(|e| panic!("literal branch {short} must succeed, got {e:?}"));
        let full = format!("refs/heads/{short}");
        assert!(fx.ref_exists(&full), "exact ref {full} must exist");
        assert_eq!(git(&target, &["rev-parse", "HEAD"]), base);
    }

    // No payload fragment may have materialized as a file anywhere in the fixture:
    // every new path must live under the managed root or Git's own admin dir.
    let entries_after = fixture_entry_names(&fx);
    for entry in entries_after.difference(&entries_before) {
        assert!(
            entry.starts_with("managed-roots/") || entry.starts_with("repo/.git/"),
            "unexpected fixture path (possible shell interpretation): {entry}"
        );
    }
    for canary in [
        "CANARY-SEMI",
        "CANARY-DOLLAR",
        "CANARY-BT",
        "touch-CANARY-SEMI",
    ] {
        assert!(
            !fx.dir.join(canary).exists() && !target_parent_has(&fx, canary),
            "shell payload must not execute: {canary}"
        );
    }
}

#[test]
fn dash_led_branch_is_rejected_before_spawn() {
    let adapter = GitCli::new();
    let fx = Fixture::new("dashled");
    let base = fx.primary_head();

    // Domain shape is valid (prefix, no spaces/..), so this reaches the
    // adapter, which must refuse it as argv-unsafe — typed, no execution.
    let lease = lease_for("dash", "-evil", &base, "ref:ws-dash");
    let target = target_path(&fx, "ws-dash");
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .expect_err("dash-led branch must be refused");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "expected typed backend refusal, got {err:?}"
    );
    assert!(!target.exists());
    assert!(!fx.ref_exists("refs/heads/-evil"));
}

#[test]
fn space_branch_is_rejected_by_domain_validation() {
    // Spaces never reach Git: the domain contract refuses them first.
    assert!(workspace::BranchRef::new("refs/heads/has space").is_err());
}

#[test]
fn paths_with_spaces_unicode_and_metachars_work_literally() {
    let adapter = GitCli::new();
    let fx = Fixture::new("sp ace-ünicode-日本語");
    let base = fx.primary_head();

    for (tag, name) in [
        ("sp", "ws with spaces"),
        ("uni", "ws-ünicode-日本語"),
        ("meta", "ws;name-$x"),
    ] {
        let target = target_path(&fx, name);
        let lease = lease_for(
            &format!("paths-{tag}"),
            &format!("agent/e-paths-{tag}"),
            &base,
            &format!("ref:ws-paths-{tag}"),
        );
        adapter
            .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
            .unwrap_or_else(|e| panic!("unusual path {name} must work literally, got {e:?}"));
        assert_eq!(git(&target, &["rev-parse", "HEAD"]), base);
    }
}

#[test]
fn target_inside_primary_checkout_is_refused() {
    let adapter = GitCli::new();
    let fx = Fixture::new("nested");
    let base = fx.primary_head();

    let nested = fx.dir.join("nested-wt");
    let lease = lease_for("nested", "agent/e-nested", &base, "ref:ws-nested");
    let err = adapter
        .create_worktree(&lease, &fx.path_str(), nested.to_str().unwrap())
        .expect_err("nested target must be refused");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeCollision { .. }),
        "got {err:?}"
    );
    assert!(!nested.exists());
    assert!(!fx.ref_exists("refs/heads/agent/e-nested"));
}

#[test]
fn missing_git_executable_is_typed_not_panic() {
    let adapter = GitCli::with_executable("/nonexistent/git-binary-for-task0041-test");
    let dir = std::env::temp_dir();
    let path = dir.to_string_lossy().into_owned();

    let err = adapter
        .is_repository(&path)
        .expect_err("missing git must fail");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "expected GitBackendFailure (conceptually GitUnavailable), got {err:?}"
    );
    assert_eq!(adapter.backend_ref(), "git-cli");
}

#[test]
fn diff_and_removal_are_implemented_safely() {
    // TASK-0043: `compute_diff` observes without mutating and
    // `remove_worktree` performs safe non-forced removal. The old
    // TASK-0041 deferral refusal is superseded; this anchor keeps the
    // safety-regression binary covering the new behavior while the full
    // diff/cleanup matrices live in `diff_cleanup.rs`.
    let adapter = GitCli::new();
    let fx = Fixture::new("implemented");
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-implemented");
    let lease = lease_for(
        "implemented",
        "agent/e-implemented",
        &base,
        "ref:ws-implemented",
    );
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .unwrap();

    // Read-only diff on a clean worktree: empty layers, base authoritative.
    // Note: `compute_diff` resolves the worktree from the lease's
    // worktree reference, so the lease must carry the real target path.
    let real_lease = lease_for(
        "implemented2",
        "agent/e-implemented",
        &base,
        target.to_str().unwrap(),
    );
    let evidence = adapter.compute_diff(&real_lease).unwrap();
    assert_eq!(evidence.diff.base_revision.as_str(), base);
    assert!(evidence.diff.committed.is_empty());
    assert!(evidence.diff.working.is_empty());
    assert!(!evidence.diff.has_working_tree_changes);

    // Safe removal of the clean worktree succeeds; the branch survives
    // `remove_worktree` (branch teardown is the separate `remove_branch`
    // step, exercised in `diff_cleanup.rs`).
    adapter
        .remove_worktree(&real_lease, target.to_str().unwrap())
        .unwrap();
    assert!(!target.exists());
    assert!(
        fx.ref_exists("refs/heads/agent/e-implemented"),
        "branch must survive worktree removal"
    );
}

#[test]
fn detached_head_and_file_paths_observe_as_invalid() {
    let adapter = GitCli::new();
    let fx = Fixture::new("invalid-status");
    let base = fx.primary_head();
    let target = target_path(&fx, "ws-detach");
    let lease = lease_for("detach", "agent/e-detach", &base, "ref:ws-detach");
    adapter
        .create_worktree(&lease, &fx.path_str(), target.to_str().unwrap())
        .unwrap();

    git(&target, &["checkout", "-q", "--detach", "HEAD"]);
    assert_eq!(
        adapter.worktree_status(target.to_str().unwrap()).unwrap(),
        WorktreeStatus::Invalid,
        "detached managed worktree must observe Invalid, never silently continue"
    );

    let file = fx.dir.join("plain-file.txt");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(
        adapter.worktree_status(file.to_str().unwrap()).unwrap(),
        WorktreeStatus::Invalid
    );
}

#[test]
fn owner_liveness_is_honest_and_backend_named() {
    let adapter = GitCli::new();
    let fx = Fixture::new("live");
    let repo = workspace::RepositoryRef::new("repo-local:live").unwrap();
    let owner = workspace::ExecutionId::new("E-live").unwrap();
    // No liveness source in TASK-0041: honestly false, never faked true.
    assert!(!adapter.owner_is_live(&repo, &owner).unwrap());
    assert_eq!(adapter.backend_ref(), "git-cli");
    let _ = fx;
}

// --- helpers ---------------------------------------------------------------

/// All fixture paths relative to the case dir, as a set.
fn fixture_entry_names(fx: &Fixture) -> std::collections::BTreeSet<String> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read fixture dir") {
            let entry = entry.expect("entry");
            let rel = entry
                .path()
                .strip_prefix(root)
                .expect("prefix")
                .to_string_lossy()
                .into_owned();
            out.push(rel);
            if entry.file_type().expect("ftype").is_dir() {
                walk(&entry.path(), root, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&fx.case_dir, &fx.case_dir, &mut out);
    out.into_iter().collect()
}

fn target_parent_has(fx: &Fixture, name: &str) -> bool {
    fx.roots.join(name).exists()
}
