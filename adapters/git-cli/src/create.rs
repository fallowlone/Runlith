//! Verified worktree creation: exact base commit + new managed branch +
//! separate worktree via one Git-native operation.
//!
//! Order of operations (fail closed, primary checkout never mutated):
//!
//! 1. repository classification (bare/non-repo fail before any mutation);
//! 2. base-revision existence proof (exact OID must name a commit);
//! 3. argv safety for the managed branch short name;
//! 4. protected/default-branch check (managed == default → `BranchCollision`);
//! 5. branch-collision preflight (any pre-existing ref → `BranchCollision`;
//!    TASK-0041 has no durable ownership store, so pre-existing ownership
//!    can never be proven and reuse is always refused — no `-B`, no reset);
//! 6. target-path preflight (must not exist, must not equal or nest inside
//!    the primary root, parent must exist — no `rm -rf`, no reuse);
//! 7. primary-checkout baseline observation (HEAD OID, HEAD ref, status);
//! 8. single mutation: `worktree add -b <branch> -- <path> <base-oid>`;
//! 9. independent postcondition verification (registration, exact HEAD,
//!    exact branch, primary unmoved); any contradiction → `CheckoutFailed`
//!    with an explicit partial-state note — never a false rollback claim,
//!    never silent repair.
//!
//! Preflight (5)–(6) narrow the window but the Git mutation stays
//! authoritative: a TOCTOU race surfaces as a postcondition contradiction,
//! not as silent success.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use workspace::{WorkspaceFailure, WorkspaceLease, WorktreeStatus};

use crate::observe;
use crate::runner::{
    absolutize, backend_failure, bounded_detail, check_branch_argv_safety, path_strictly_inside,
    run_git, run_git_text, stderr_excerpt,
};

/// Baseline of the coordinating checkout, captured before mutation and
/// re-observed after, proving the primary checkout was not moved.
struct PrimaryBaseline {
    head_oid: String,
    head_ref: Option<String>,
    status: WorktreeStatus,
}

/// Creates the lease's managed branch + worktree, then verifies Git truth.
pub fn create_worktree(
    executable: &OsStr,
    lease: &WorkspaceLease,
    repository_path: &str,
    worktree_path: &str,
) -> Result<(), WorkspaceFailure> {
    let target_abs =
        absolutize(worktree_path).map_err(|_| WorkspaceFailure::WorktreeCollision {
            detail: bounded_detail("git-cli: invalid worktree target path".to_owned()),
        })?;
    let root = observe::repository_root(executable, repository_path)?;

    let base_oid = lease.base_revision().as_str().to_owned();
    prove_base_revision(executable, &root, &base_oid)?;

    check_branch_argv_safety(lease)?;
    let managed = lease.branch().as_str().to_owned();

    // Protected/default branch first so the refusal names the protection;
    // indeterminate metadata does not block (fixtures always set the
    // canonical origin/HEAD, so protection stays exercised).
    match observe::default_branch(executable, root_to_str(&root)?) {
        Ok(default_ref) => {
            if default_ref == managed {
                return Err(WorkspaceFailure::BranchCollision {
                    detail: bounded_detail(format!(
                        "git-cli: managed branch {managed} is the protected/default branch \
                         ({default_ref}); autonomous leases never write the default branch"
                    )),
                });
            }
        }
        Err(WorkspaceFailure::GitBackendFailure { .. }) => {}
        Err(other) => return Err(other),
    }

    // Branch collision: any pre-existing ref collides. Ownership can never
    // be proven in TASK-0041 (no durable store), so reuse is always refused.
    if observe::branch_exists(executable, root_to_str(&root)?, lease.branch())? {
        return Err(WorkspaceFailure::BranchCollision {
            detail: bounded_detail(format!(
                "git-cli: managed branch {managed} already exists with unproven ownership; \
                 refusing reuse (no -B, no reset)"
            )),
        });
    }

    check_target_path(&root, &target_abs)?;

    let baseline = observe_primary(executable, &root)?;

    // The single Git-native mutation: exact base commit + new managed
    // branch + separate worktree. `--` separates the explicit path from
    // options; every value travels as its own argv element.
    let target_arg = target_abs.clone().into_os_string();
    let add = run_git(
        executable,
        &root,
        &[
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(lease.branch().short_name()),
            OsStr::new("--"),
            target_arg.as_os_str(),
            OsStr::new(&base_oid),
        ],
    )?;
    if !add.status.success() {
        return Err(mutation_failure(
            executable,
            &root,
            &target_abs,
            lease.branch(),
            &add,
        ));
    }

    verify_postconditions(
        executable,
        lease,
        &root,
        &target_abs,
        &base_oid,
        &managed,
        &baseline,
    )
}

/// Continues a proven branch-only materialization crash by attaching the
/// already-existing managed branch. No branch is created, reset, or deleted.
pub fn attach_worktree(
    executable: &OsStr,
    lease: &WorkspaceLease,
    repository_path: &str,
    worktree_path: &str,
) -> Result<(), WorkspaceFailure> {
    let target_abs =
        absolutize(worktree_path).map_err(|_| WorkspaceFailure::WorktreeCollision {
            detail: bounded_detail("git-cli: invalid worktree target path".to_owned()),
        })?;
    let root = observe::repository_root(executable, repository_path)?;
    let base_oid = lease.base_revision().as_str().to_owned();
    prove_base_revision(executable, &root, &base_oid)?;
    check_branch_argv_safety(lease)?;

    if !observe::branch_exists(executable, root_to_str(&root)?, lease.branch())? {
        return Err(WorkspaceFailure::BranchCollision {
            detail: bounded_detail(format!(
                "git-cli: managed branch {} disappeared before recovery",
                lease.branch().as_str()
            )),
        });
    }
    let (tip_out, tip) = run_git_text(
        executable,
        &root,
        &["rev-parse", "--verify", lease.branch().as_str()],
    )?;
    if !tip_out.status.success() || tip != base_oid {
        return Err(WorkspaceFailure::BranchCollision {
            detail: bounded_detail(format!(
                "git-cli: managed branch {} is not exactly at recorded base {}; refusing recovery",
                lease.branch().as_str(),
                base_oid
            )),
        });
    }
    if let Some(holder) =
        observe::worktree_for_branch(executable, root_to_str(&root)?, lease.branch())?
    {
        return Err(WorkspaceFailure::WorktreeCollision {
            detail: bounded_detail(format!(
                "git-cli: managed branch {} is already checked out at {holder}",
                lease.branch().as_str()
            )),
        });
    }

    check_target_path(&root, &target_abs)?;
    let baseline = observe_primary(executable, &root)?;
    let target_arg = target_abs.clone().into_os_string();
    let add = run_git(
        executable,
        &root,
        &[
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("--"),
            target_arg.as_os_str(),
            OsStr::new(lease.branch().short_name()),
        ],
    )?;
    if !add.status.success() {
        return Err(mutation_failure(
            executable,
            &root,
            &target_abs,
            lease.branch(),
            &add,
        ));
    }

    verify_postconditions(
        executable,
        lease,
        &root,
        &target_abs,
        &base_oid,
        lease.branch().as_str(),
        &baseline,
    )
}

/// Proves the exact base OID names a commit in this repository.
fn prove_base_revision(
    executable: &OsStr,
    root: &Path,
    base_oid: &str,
) -> Result<(), WorkspaceFailure> {
    let candidate = format!("{base_oid}^{{commit}}");
    let (output, resolved) = run_git_text(
        executable,
        root,
        &["rev-parse", "--verify", "--quiet", candidate.as_str()],
    )?;
    if !output.status.success() || resolved != base_oid {
        return Err(WorkspaceFailure::InvalidRevision {
            detail: bounded_detail(format!(
                "git-cli: base revision is not a commit in this repository: {base_oid}"
            )),
        });
    }
    Ok(())
}

/// Fail-closed target checks. Nothing is created, removed, or cleaned here.
///
/// The caller authorizes placement by providing the path (and provisioning
/// its parent, e.g. the future managed root); the adapter only refuses
/// unsafe targets. Comparisons use canonicalized paths so symlinked
/// prefixes (macOS `/var` → `/private/var`) cannot hide nesting.
fn check_target_path(root: &Path, target: &Path) -> Result<(), WorkspaceFailure> {
    if target.symlink_metadata().is_ok() {
        return Err(WorkspaceFailure::WorktreeCollision {
            detail: bounded_detail(format!(
                "git-cli: worktree target already exists; refusing overwrite: {}",
                target.display()
            )),
        });
    }
    let parent = target.parent().filter(|p| p.is_dir()).ok_or_else(|| {
        backend_failure(format!(
            "git-cli: worktree target parent is missing or not a directory: {}",
            target.display()
        ))
    })?;
    let root_canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let parent_canon = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    if parent_canon == root_canon || path_strictly_inside(&root_canon, &parent_canon) {
        return Err(WorkspaceFailure::WorktreeCollision {
            detail: bounded_detail(format!(
                "git-cli: worktree target must not equal or nest inside the primary checkout: {}",
                target.display()
            )),
        });
    }
    if target == root || path_strictly_inside(root, target) {
        return Err(WorkspaceFailure::WorktreeCollision {
            detail: bounded_detail(format!(
                "git-cli: worktree target must not equal or nest inside the primary checkout: {}",
                target.display()
            )),
        });
    }
    Ok(())
}

/// Observes the coordinating checkout before mutation.
fn observe_primary(executable: &OsStr, root: &Path) -> Result<PrimaryBaseline, WorkspaceFailure> {
    let (head_out, head_oid) = run_git_text(executable, root, &["rev-parse", "HEAD"])?;
    if !head_out.status.success() || head_oid.is_empty() {
        return Err(backend_failure(
            "git-cli: cannot resolve primary checkout HEAD before creation",
        ));
    }
    let ref_out = run_git(executable, root, &["symbolic-ref", "--quiet", "HEAD"])?;
    let head_ref = if ref_out.status.success() {
        Some(String::from_utf8_lossy(&ref_out.stdout).trim().to_owned())
    } else {
        None
    };
    let root_str = root_to_str(root)?;
    let status = observe::worktree_status(executable, root_str)?;
    Ok(PrimaryBaseline {
        head_oid,
        head_ref,
        status,
    })
}

/// Classifies a failed mutation without claiming rollback: re-observes
/// branch and registration state so the caller can see partial creation.
fn mutation_failure(
    executable: &OsStr,
    root: &Path,
    target: &Path,
    managed: &workspace::BranchRef,
    add_output: &std::process::Output,
) -> WorkspaceFailure {
    let root_str = root.to_string_lossy().into_owned();
    let branch_now = observe::branch_exists(executable, &root_str, managed).unwrap_or(true);
    let registered_now = worktree_registered(executable, root, target).unwrap_or(true);
    WorkspaceFailure::CheckoutFailed {
        detail: bounded_detail(format!(
            "git-cli: worktree creation failed ({}); partial state possible \
             (branch_exists={branch_now}, worktree_registered={registered_now}); \
             reconciliation deferred to TASK-0042",
            stderr_excerpt(add_output),
        )),
    }
}

/// Verifies every postcondition by independent re-observation.
#[allow(clippy::too_many_arguments)]
fn verify_postconditions(
    executable: &OsStr,
    lease: &WorkspaceLease,
    root: &Path,
    target: &Path,
    base_oid: &str,
    managed: &str,
    baseline: &PrimaryBaseline,
) -> Result<(), WorkspaceFailure> {
    let partial = |what: &str| WorkspaceFailure::CheckoutFailed {
        detail: bounded_detail(format!(
            "git-cli: postcondition failed ({what}); worktree/branch may be partially created; \
             no repair attempted, reconciliation deferred to TASK-0042"
        )),
    };

    if !worktree_registered(executable, root, target)
        .map_err(|_| partial("worktree list unreadable"))?
    {
        return Err(partial("new worktree not registered by git worktree list"));
    }
    let (head_out, head_oid) = run_git_text(executable, target, &["rev-parse", "HEAD"])
        .map_err(|_| partial("managed worktree HEAD unreadable"))?;
    if !head_out.status.success() || head_oid != base_oid {
        return Err(partial("managed worktree HEAD != exact base revision"));
    }
    let branch_out = run_git(executable, target, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(|_| partial("managed worktree branch unreadable"))?;
    if !branch_out.status.success() || String::from_utf8_lossy(&branch_out.stdout).trim() != managed
    {
        return Err(partial(
            "managed worktree branch != requested managed branch",
        ));
    }

    let (after_out, after_oid) = run_git_text(executable, root, &["rev-parse", "HEAD"])
        .map_err(|_| partial("primary HEAD unreadable after creation"))?;
    let after_ref_out = run_git(executable, root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(|_| partial("primary ref unreadable after creation"))?;
    let after_ref = if after_ref_out.status.success() {
        Some(
            String::from_utf8_lossy(&after_ref_out.stdout)
                .trim()
                .to_owned(),
        )
    } else {
        None
    };
    if !after_out.status.success()
        || after_oid != baseline.head_oid
        || after_ref != baseline.head_ref
    {
        return Err(partial("primary checkout was moved by creation"));
    }
    let root_str = root_to_str(root).map_err(|_| partial("primary root unreadable"))?;
    let after_status = observe::worktree_status(executable, root_str)
        .map_err(|_| partial("primary status unreadable"))?;
    if after_status != baseline.status {
        return Err(partial("primary checkout status changed during creation"));
    }

    let _ = lease;
    Ok(())
}

/// Whether `target` is registered in `git worktree list --porcelain`.
fn worktree_registered(
    executable: &OsStr,
    root: &Path,
    target: &Path,
) -> Result<bool, WorkspaceFailure> {
    let output = run_git(executable, root, &["worktree", "list", "--porcelain"])?;
    if !output.status.success() {
        return Err(backend_failure(format!(
            "git-cli: worktree list failed ({})",
            stderr_excerpt(&output)
        )));
    }
    let listed = String::from_utf8_lossy(&output.stdout);
    let target_canon = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    for line in listed.lines() {
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

fn root_to_str(root: &Path) -> Result<&str, WorkspaceFailure> {
    root.to_str()
        .ok_or_else(|| backend_failure("git-cli: repository root is not UTF-8; refusing"))
}
