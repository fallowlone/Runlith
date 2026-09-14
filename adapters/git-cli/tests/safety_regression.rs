//! TASK-0041 argv/shell-injection regression, unavailable-Git handling,
//! deferred-operation refusal, and Unicode/space path coverage.

mod common;

use git_cli::GitCli;
use workspace::{GitPort, WorkspaceFailure, WorktreeStatus};

use common::{git, lease_for, target_path, Fixture};

#[test]
fn hostile_git_metadata_is_rejected_before_git_discovery() {
    let adapter = GitCli::new();
    let fx = Fixture::new("git-metadata-hostile");

    let original = fx.dir.join(".git");
    let moved = fx.case_dir.join("real-git-dir");
    std::fs::rename(&original, &moved).expect("move git metadata");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&moved, &original).expect("create .git symlink");

    #[cfg(unix)]
    {
        let err = adapter
            .is_repository(&fx.path_str())
            .expect_err(".git symlink metadata must fail closed");
        assert!(matches!(err, WorkspaceFailure::GitBackendFailure { .. }));
    }
}

#[test]
fn malicious_git_pointer_target_is_rejected() {
    let adapter = GitCli::new();
    let fx = Fixture::new("git-pointer-hostile");

    std::fs::remove_dir_all(fx.dir.join(".git")).expect("remove git dir");
    std::fs::write(fx.dir.join(".git"), "gitdir: /path/that/does/not/exist\n")
        .expect("write pointer");

    let err = adapter
        .is_repository(&fx.path_str())
        .expect_err("unavailable gitdir target must fail closed");
    assert!(matches!(err, WorkspaceFailure::GitBackendFailure { .. }));
}

#[test]
fn git_extension_execution_surfaces_are_disabled() {
    let adapter = GitCli::new();
    let fx = Fixture::new("git-extension-surfaces");
    let marker = fx.case_dir.join("extension-marker");

    // These values would execute helper commands with an unguarded Git
    // invocation. The adapter should clear config inheritance and apply its
    // explicit policy before Git reads repository state.
    std::fs::write(
        fx.dir.join(".git/config"),
        format!(
            "[core]\n\texternalDiff = touch {}\n\tfsmonitor = touch {}\n[credential]\n\thelper = touch {}\n",
            marker.display(),
            marker.display(),
            marker.display()
        ),
    )
    .expect("write hostile config");

    adapter
        .is_repository(&fx.path_str())
        .expect("hostile config must not execute extensions");
    assert!(!marker.exists(), "git extension must not execute helper");
}

/// TASK-0071 remediation of independent-review Finding 4: the keeper above
/// exercises `is_repository` (`rev-parse --is-inside-work-tree`), which
/// never invokes Git's fsmonitor hook and therefore passed even against the
/// vulnerable e9009a8 baseline (no `run_git` hardening at all). This is the
/// real production path the TASK-0070 supplemental evidence used to
/// reproduce F-003: `GitCli::worktree_status` runs `git status`, which is
/// what triggers `core.fsmonitor`. On e9009a8 this creates the marker file
/// (RED); with the current `run_git` hardening (`-c core.fsmonitor=false`
/// plus `env_clear`) it must not (GREEN).
#[test]
fn worktree_status_production_path_disables_fsmonitor_execution() {
    let adapter = GitCli::new();
    let fx = Fixture::new("fsmonitor-production-path");
    let marker = fx.case_dir.join("fsmonitor-marker");

    std::fs::write(
        fx.dir.join(".git/config"),
        format!("[core]\n\tfsmonitor = touch {}\n", marker.display()),
    )
    .expect("write hostile fsmonitor config");

    adapter
        .worktree_status(&fx.path_str())
        .expect("hostile fsmonitor config must not break worktree_status");
    assert!(
        !marker.exists(),
        "core.fsmonitor helper must not execute via worktree_status"
    );
}

/// TASK-0071 remediation of independent-review Finding 1 (BLOCKER): a
/// `.git` pointer that is syntactically valid and targets a real, existing
/// Git metadata directory must still be refused when that directory belongs
/// to a different repository. Without this check, repository A's `.git`
/// could point at repository B's real `.git`, and every observation made
/// while operating on path A (branch, HEAD, revision) would silently
/// reflect B's identity instead.
#[test]
fn valid_but_wrong_gitdir_pointer_redirection_is_refused() {
    let adapter = GitCli::new();
    let fx_a = Fixture::new("identity-redirect-a");
    let fx_b = Fixture::new("identity-redirect-b");
    // `Fixture::new`'s initial empty commit can collide in OID with another
    // fixture created in the same second (identical tree, message, and
    // second-granularity timestamp); a second, differently-worded commit on
    // B makes the two identities observably distinct beyond doubt.
    git(
        &fx_b.dir,
        &["commit", "-q", "--allow-empty", "-m", "identity-b-marker"],
    );
    let b_head = fx_b.primary_head();
    assert_ne!(
        fx_a.base_oid, b_head,
        "fixtures must have observably distinct identities"
    );

    std::fs::remove_dir_all(fx_a.dir.join(".git")).expect("remove A's real .git directory");
    std::fs::write(
        fx_a.dir.join(".git"),
        format!("gitdir: {}\n", fx_b.dir.join(".git").display()),
    )
    .expect("redirect A's .git at B's real, valid metadata");

    let err = adapter
        .is_repository(&fx_a.path_str())
        .expect_err("a valid-but-foreign gitdir target must fail closed");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "got {err:?}"
    );

    let err = adapter
        .resolve_revision(&fx_a.path_str(), "HEAD")
        .expect_err("resolve_revision must not resolve identity through a foreign redirect");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "got {err:?}"
    );
}

/// TASK-0071 self-review regression: a forged linked-worktree-shaped
/// metadata directory must not be able to satisfy the reverse `gitdir`
/// check locally while redirecting `commondir` at a different repository.
/// Git resolves refs/objects through `commondir`, so accepting that shape
/// still redirects repository identity even though the target's `gitdir`
/// file points back to this worktree.
#[test]
fn forged_linked_worktree_commondir_redirection_is_refused() {
    let adapter = GitCli::new();
    let fx_a = Fixture::new("identity-shim-a");
    let fx_b = Fixture::new("identity-shim-b");
    git(
        &fx_b.dir,
        &[
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "identity-shim-b-marker",
        ],
    );
    let b_head = fx_b.primary_head();
    assert_ne!(
        fx_a.base_oid, b_head,
        "fixtures must have distinct identities"
    );

    let original = fx_a.dir.join(".git");
    let moved = fx_a.case_dir.join("identity-shim-a-original-git");
    std::fs::rename(&original, &moved).expect("move A's real .git directory");

    let shim = fx_a.dir.join(".git-shim");
    std::fs::create_dir(&shim).expect("create forged worktree metadata directory");
    std::fs::write(&original, format!("gitdir: {}\n", shim.display()))
        .expect("point A at forged metadata");
    std::fs::write(shim.join("gitdir"), format!("{}\n", original.display()))
        .expect("forge reverse worktree back-reference");
    std::fs::write(
        shim.join("commondir"),
        format!("{}\n", fx_b.dir.join(".git").display()),
    )
    .expect("redirect forged metadata at B's common Git directory");
    let b_head_ref = git(&fx_b.dir, &["symbolic-ref", "HEAD"]);
    std::fs::write(shim.join("HEAD"), format!("ref: {b_head_ref}\n"))
        .expect("forge linked-worktree HEAD");

    let err = adapter
        .resolve_revision(&fx_a.path_str(), "HEAD")
        .expect_err("forged metadata must not redirect commondir to a foreign repository");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "got {err:?}"
    );
}

/// TASK-0071 remediation of independent-review Finding 2 (HIGH):
/// `worktree_status` previously called Git directly, bypassing the same
/// `.git` metadata validation `classify_repository` performs for every other
/// entry point. This proves both hostile shapes fail closed through
/// `worktree_status` specifically, never silently producing status evidence
/// for a symlinked or redirected repository identity.
#[test]
fn worktree_status_refuses_symlinked_and_redirected_git_metadata() {
    let adapter = GitCli::new();

    let fx = Fixture::new("worktree-status-symlink-hostile");
    let original = fx.dir.join(".git");
    let moved = fx.case_dir.join("real-git-dir");
    std::fs::rename(&original, &moved).expect("move git metadata");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&moved, &original).expect("create .git symlink");
        let err = adapter
            .worktree_status(&fx.path_str())
            .expect_err(".git symlink metadata must fail closed via worktree_status");
        assert!(
            matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
            "got {err:?}"
        );
    }

    let fx_a = Fixture::new("worktree-status-redirect-a");
    let fx_b = Fixture::new("worktree-status-redirect-b");
    std::fs::remove_dir_all(fx_a.dir.join(".git")).expect("remove A's real .git directory");
    std::fs::write(
        fx_a.dir.join(".git"),
        format!("gitdir: {}\n", fx_b.dir.join(".git").display()),
    )
    .expect("redirect A's .git at B's real, valid metadata");
    let err = adapter
        .worktree_status(&fx_a.path_str())
        .expect_err("redirected .git metadata must not silently produce status evidence");
    assert!(
        matches!(err, WorkspaceFailure::GitBackendFailure { .. }),
        "got {err:?}"
    );
}

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
