//! B-RS2 branch-only materialization crash recovery (TASK-0057).
//!
//! RED → GREEN keepers for the durable/filesystem/Git shape TASK-0055
//! reproduced through shipped binaries:
//!
//! ```text
//! Task RUNNING-equivalent (execution committed, zero attempts)
//! + Attempt reservation intact
//! + managed identity reservation owned by this Execution
//! + WorkspaceLease ALLOCATED
//! + managed branch exists at EXACTLY the recorded base OID
//! + target worktree path absent
//! + no Git worktree registration for that branch/path
//! + no workspace.created fact
//! ```
//!
//! The old implementation classifies this as "ambiguous partial Git state"
//! and refuses replay forever (RED-1); only manual out-of-product
//! `git branch -D` recovers the same strand (RED-2). An unexpected branch
//! tip must stay refused (RED-3, fail-closed guard for blind replay).
//!
//! All cases use real Git + a real file-backed SQLite database.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use app_services::{
    FixedClock, ManagedWorkspaceRoot, RuntimeService, TaskService, WorkspaceRequest,
    WorkspaceService,
};
use git_cli::GitCli;
use storage_sqlite::EventType;
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{GitPort, LeaseLifecycle, RepositoryRef, WorkspaceLease};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct TestEnv {
    dir: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    root: PathBuf,
}

impl TestEnv {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-branch-only-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        let db = dir.join("test.db");
        let repo = dir.join("repo");
        let root = dir.join("managed");
        std::fs::create_dir_all(&root).expect("managed root");
        let root = std::fs::canonicalize(&root).expect("canonicalize managed root");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let branch = git(&repo, &["symbolic-ref", "--short", "HEAD"]);
        let full = format!("refs/heads/{branch}");
        git(&repo, &["symbolic-ref", "refs/remotes/origin/HEAD", &full]);
        Self {
            dir,
            db,
            repo,
            root,
        }
    }

    fn task_service(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("open task service")
    }

    fn runtime_service(&self) -> RuntimeService<FixedClock> {
        RuntimeService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("open runtime service")
    }

    fn workspace_service(&self) -> WorkspaceService<GitCli, FixedClock> {
        WorkspaceService::open(
            &self.db,
            GitCli::new(),
            ManagedWorkspaceRoot::new(&self.root).expect("managed root"),
            FixedClock(FIXED_MILLIS),
        )
        .expect("open workspace service")
    }

    fn repo_path(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn git_ok(repo: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn seed_execution(env: &TestEnv, project: &str, task_id: &str, exec_id: &str) -> ExecutionId {
    let mut tasks = env.task_service();
    let project_id = ProjectId::new(project).unwrap();
    tasks
        .create_project(&Project::new(project_id.clone(), format!("project {project}")).unwrap())
        .unwrap();
    let task_id = TaskId::new(task_id).unwrap();
    tasks
        .create_task(
            &Task::draft(task_id.clone(), project_id, format!("objective {task_id}")).unwrap(),
        )
        .unwrap();
    drop(tasks);
    let exec_id = ExecutionId::new(exec_id).unwrap();
    env.runtime_service()
        .create_execution(exec_id.clone(), task_id)
        .unwrap();
    exec_id
}

fn request(env: &TestEnv, exec: &ExecutionId) -> WorkspaceRequest {
    WorkspaceRequest {
        execution_id: exec.clone(),
        repository_ref: RepositoryRef::new("repo-local:runlith").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    }
}

fn workspace_created_count(service: &WorkspaceService<GitCli, FixedClock>, ws: &str) -> usize {
    service
        .store()
        .list_events_ordered()
        .unwrap()
        .iter()
        .filter(|event| {
            event.fact.event_type == EventType::WorkspaceCreated
                && event.fact.workspace_id.as_ref().map(|id| id.as_str()) == Some(ws)
        })
        .count()
}

fn allocate_branch_only(env: &TestEnv, exec: &ExecutionId) -> WorkspaceLease {
    let mut service = env.workspace_service();
    let lease = service.allocate_workspace(&request(env, exec)).unwrap();
    let base = lease.base_revision().as_str().to_owned();
    git(&env.repo, &["branch", lease.branch().short_name(), &base]);
    lease
}

fn assert_allocated_without_created(env: &TestEnv, exec: &ExecutionId) {
    let service = env.workspace_service();
    let lease = service.workspace_for_execution(exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
    assert_eq!(workspace_created_count(&service, lease.id().as_str()), 0);
}

/// RED-1: the provably-owned branch-only shape must recover through the
/// product (same lease, same owner, same base, exactly one
/// workspace.created, real registered worktree at the base).
#[test]
fn branch_only_at_recorded_base_recovers_through_product() {
    let env = TestEnv::new("red1");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = git(&env.repo, &["rev-parse", "HEAD"]);

    // Crash seam: durable ALLOCATED intent + branch at the base, no
    // worktree registration, no path — exactly what `git worktree add -b`
    // leaves behind when the daemon dies after branch creation.
    let (ws, branch, worktree_ref) = {
        let mut service = env.workspace_service();
        let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
        let short = lease.branch().short_name().to_owned();
        git(&env.repo, &["branch", &short, &base]);
        (
            lease.id().clone(),
            lease.branch().clone(),
            lease.worktree_ref().to_owned(),
        )
    };

    // Durable/Git shape assertions (not merely an error message).
    {
        let service = env.workspace_service();
        let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
        assert_eq!(lease.id(), &ws);
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
        assert_eq!(lease.owner(), &exec);
        assert_eq!(lease.base_revision().as_str(), base);
        assert_eq!(lease.branch(), &branch);
        // Identity reservation owned by this execution.
        let owner = service
            .store()
            .find_identity_reservation_owner(branch.as_str())
            .unwrap();
        assert_eq!(owner, Some(exec.clone()));
        // Branch points EXACTLY at the recorded base.
        assert_eq!(git(&env.repo, &["rev-parse", branch.as_str()]), base);
        // Target path absent.
        assert!(
            Path::new(&worktree_ref).symlink_metadata().is_err(),
            "target path must be absent in the branch-only shape"
        );
        // No worktree registration for branch or path.
        let listed = git(&env.repo, &["worktree", "list", "--porcelain"]);
        assert!(
            !listed
                .lines()
                .any(|line| line == format!("worktree {worktree_ref}")),
            "no registration expected for the target path"
        );
        assert!(
            !listed
                .lines()
                .any(|line| line == format!("branch {}", branch.as_str())),
            "no registration expected for the branch"
        );
        // No workspace.created fact yet.
        assert_eq!(workspace_created_count(&service, ws.as_str()), 0);
    }

    // Recovery through the product (no manual Git surgery).
    let mut service = env.workspace_service();
    let result = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("provably-owned branch-only state must recover");
    assert_eq!(result.lease.id(), &ws);
    assert_eq!(result.lease.owner(), &exec);
    assert_eq!(result.lease.base_revision().as_str(), base);
    assert_eq!(result.lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(workspace_created_count(&service, ws.as_str()), 1);
    // The worktree is real, registered, attached to the owned branch at
    // the base.
    let listed = git(&env.repo, &["worktree", "list", "--porcelain"]);
    assert!(
        listed
            .lines()
            .any(|line| line == format!("worktree {worktree_ref}")),
        "recovered worktree must be registered"
    );
    assert_eq!(git(Path::new(&worktree_ref), &["rev-parse", "HEAD"]), base);
    assert_eq!(
        git(
            Path::new(&worktree_ref),
            &["symbolic-ref", "--quiet", "HEAD"]
        )
        .trim(),
        branch.as_str()
    );
    // Repeat reconciliation stays exactly-once with no new mutation.
    let again = service
        .ensure_workspace_ready(&request(&env, &exec))
        .unwrap();
    assert_eq!(again.lease.id(), &ws);
    assert_eq!(workspace_created_count(&service, ws.as_str()), 1);
}

/// RED-2: the obstruction is the branch itself — manual out-of-product
/// branch deletion makes the identical strand recover. Documents WHY the
/// old refusal is the branch-only shape (and why the fix must continue
/// the owned branch, not require surgery).
#[test]
fn manual_branch_deletion_recovers_same_strand() {
    let env = TestEnv::new("red2");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = git(&env.repo, &["rev-parse", "HEAD"]);
    let ws = {
        let mut service = env.workspace_service();
        let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
        let short = lease.branch().short_name().to_owned();
        git(&env.repo, &["branch", &short, &base]);
        lease.id().clone()
    };
    // Out-of-product surgery (the TASK-0055 workaround).
    let branch = {
        let service = env.workspace_service();
        service
            .workspace_for_execution(&exec)
            .unwrap()
            .unwrap()
            .branch()
            .clone()
    };
    git(&env.repo, &["branch", "-D", branch.short_name()]);
    assert!(!git_ok(
        &env.repo,
        &["show-ref", "--verify", "--quiet", branch.as_str()]
    ));
    // The same durable strand now resumes as crash-A.
    let mut service = env.workspace_service();
    let result = service
        .ensure_workspace_ready(&request(&env, &exec))
        .unwrap();
    assert_eq!(result.lease.id(), &ws);
    assert_eq!(result.lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(workspace_created_count(&service, ws.as_str()), 1);
}

/// RED-3: an unexpected branch tip must stay refused — establishes why
/// blind replay ("branch exists → retry") would be unsafe. Passes before
/// and after the fix.
#[test]
fn branch_only_unexpected_tip_stays_fail_closed() {
    let env = TestEnv::new("red3");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = git(&env.repo, &["rev-parse", "HEAD"]);
    {
        let mut service = env.workspace_service();
        let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
        // Advance the repo so the planted branch can point at a DIFFERENT
        // commit than the recorded base.
        git(
            &env.repo,
            &["commit", "-q", "--allow-empty", "-m", "advance"],
        );
        let other = git(&env.repo, &["rev-parse", "HEAD"]);
        assert_ne!(other, base);
        let short = lease.branch().short_name().to_owned();
        git(&env.repo, &["branch", &short, &other]);
    }
    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("branch at an unexpected OID must fail closed");
    assert!(
        matches!(
            err,
            workspace::WorkspaceFailure::CheckoutFailed { .. }
                | workspace::WorkspaceFailure::OwnershipMismatch { .. }
                | workspace::WorkspaceFailure::StorageFailure { .. }
        ),
        "expected typed fail-closed refusal, got {err:?}"
    );
    // Nothing finalized, lease stays ALLOCATED, no created fact.
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
    assert_eq!(workspace_created_count(&service, lease.id().as_str()), 0);
}

#[test]
fn packed_branch_at_recorded_base_recovers() {
    let env = TestEnv::new("packed");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    let primary_head = git(&env.repo, &["rev-parse", "HEAD"]);
    git(&env.repo, &["pack-refs", "--all"]);

    let mut service = env.workspace_service();
    let recovered = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("packed managed ref at exact base remains recoverable");
    assert_eq!(recovered.lease.id(), lease.id());
    assert_eq!(recovered.lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(git(&env.repo, &["rev-parse", "HEAD"]), primary_head);
    assert_eq!(workspace_created_count(&service, lease.id().as_str()), 1);
}

#[test]
fn branch_registered_in_another_worktree_refuses() {
    let env = TestEnv::new("registered-elsewhere");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    let other = env.dir.join("other-worktree");
    let other_text = other.to_string_lossy().into_owned();
    git(
        &env.repo,
        &[
            "worktree",
            "add",
            "--",
            &other_text,
            lease.branch().short_name(),
        ],
    );

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("branch registered elsewhere must fail closed");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::OwnershipMismatch { .. }
    ));
    assert_allocated_without_created(&env, &exec);
}

#[test]
fn unexpected_empty_target_directory_refuses() {
    let env = TestEnv::new("empty-target");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    std::fs::create_dir_all(lease.worktree_ref()).unwrap();

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("even an empty unexpected target must be preserved");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::WorktreeCollision { .. }
    ));
    assert!(Path::new(lease.worktree_ref()).is_dir());
    assert_allocated_without_created(&env, &exec);
}

#[test]
fn unexpected_nonempty_target_directory_refuses_without_deletion() {
    let env = TestEnv::new("nonempty-target");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    std::fs::create_dir_all(lease.worktree_ref()).unwrap();
    let sentinel = Path::new(lease.worktree_ref()).join("do-not-delete.txt");
    std::fs::write(&sentinel, "owned elsewhere").unwrap();

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("non-empty target must fail closed");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::WorktreeCollision { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(sentinel).unwrap(),
        "owned elsewhere"
    );
    assert_allocated_without_created(&env, &exec);
}

#[test]
fn unregistered_git_checkout_at_target_refuses() {
    let env = TestEnv::new("foreign-checkout");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    std::fs::create_dir_all(lease.worktree_ref()).unwrap();
    git(Path::new(lease.worktree_ref()), &["init", "-q"]);

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("foreign checkout at expected path must fail closed");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::WorktreeCollision { .. }
    ));
    assert!(Path::new(lease.worktree_ref()).join(".git").exists());
    assert_allocated_without_created(&env, &exec);
}

#[cfg(unix)]
#[test]
fn symlink_target_refuses_without_following_or_deleting_it() {
    use std::os::unix::fs::symlink;

    let env = TestEnv::new("symlink-target");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    let foreign = env.dir.join("foreign-dir");
    std::fs::create_dir_all(&foreign).unwrap();
    std::fs::write(foreign.join("sentinel"), "keep").unwrap();
    symlink(&foreign, lease.worktree_ref()).unwrap();

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("symlink target must fail closed");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::OwnershipMismatch { .. }
            | workspace::WorkspaceFailure::WorktreeCollision { .. }
    ));
    assert_eq!(
        std::fs::read_to_string(foreign.join("sentinel")).unwrap(),
        "keep"
    );
    assert_allocated_without_created(&env, &exec);
}

#[test]
fn missing_identity_reservation_refuses() {
    let env = TestEnv::new("missing-reservation");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    let connection = rusqlite::Connection::open(&env.db).unwrap();
    connection
        .execute(
            "DELETE FROM managed_identity_reservations WHERE branch_ref = ?1",
            rusqlite::params![lease.branch().as_str()],
        )
        .unwrap();

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("missing ownership reservation must fail closed");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::OwnershipMismatch { .. }
    ));
    assert_allocated_without_created(&env, &exec);
}

#[test]
fn foreign_identity_reservation_refuses() {
    let env = TestEnv::new("foreign-reservation");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let foreign = seed_execution(&env, "P-2", "T-2", "E-2");
    let lease = allocate_branch_only(&env, &exec);
    let connection = rusqlite::Connection::open(&env.db).unwrap();
    connection
        .execute(
            "UPDATE managed_identity_reservations SET execution_id = ?1 WHERE branch_ref = ?2",
            rusqlite::params![foreign.as_str(), lease.branch().as_str()],
        )
        .unwrap();

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect_err("foreign reservation must fail closed");
    assert!(matches!(
        err,
        workspace::WorkspaceFailure::OwnershipMismatch { .. }
    ));
    assert_allocated_without_created(&env, &exec);
}

#[test]
fn second_crash_after_existing_branch_attachment_is_reconciled() {
    let env = TestEnv::new("second-crash");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);

    // First recovery reached the Git mutation but the daemon dies before
    // `Allocated -> Ready` + workspace.created can commit.
    GitCli::new()
        .attach_worktree(&lease, &env.repo_path(), lease.worktree_ref())
        .expect("continue existing branch");
    assert_eq!(
        env.workspace_service()
            .workspace_for_execution(&exec)
            .unwrap()
            .unwrap()
            .lifecycle(),
        LeaseLifecycle::Allocated
    );

    // A fresh service sees the complete Git state as Crash-B and finalizes.
    let mut restarted = env.workspace_service();
    let ready = restarted
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("second restart must remain recoverable");
    assert_eq!(ready.lease.id(), lease.id());
    assert_eq!(ready.lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(workspace_created_count(&restarted, lease.id().as_str()), 1);
}

#[test]
fn recovered_workspace_cleans_up_normally() {
    let env = TestEnv::new("cleanup");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease = allocate_branch_only(&env, &exec);
    let branch = lease.branch().clone();
    let path = lease.worktree_ref().to_owned();
    let mut service = env.workspace_service();
    service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("recover");
    let removed = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect("ordinary cleanup after recovery");
    assert_eq!(removed.lease.lifecycle(), LeaseLifecycle::Removed);
    assert!(!Path::new(&path).exists());
    assert!(!git_ok(
        &env.repo,
        &["show-ref", "--verify", "--quiet", branch.as_str()]
    ));
}

#[test]
fn fresh_materialization_is_unchanged() {
    let env = TestEnv::new("fresh");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let primary_head = git(&env.repo, &["rev-parse", "HEAD"]);
    let mut service = env.workspace_service();
    let created = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("fresh workspace still materializes");
    assert_eq!(created.lease.lifecycle(), LeaseLifecycle::Ready);
    assert!(Path::new(created.lease.worktree_ref()).is_dir());
    assert_eq!(git(&env.repo, &["rev-parse", "HEAD"]), primary_head);
    assert_eq!(
        workspace_created_count(&service, created.lease.id().as_str()),
        1
    );
}
