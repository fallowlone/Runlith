//! TASK-0043 service matrix: base-revision diff evidence plus conservative
//! idempotent cleanup over a real file-backed SQLite database and a real
//! temporary Git repository.
//!
//! Covers: diff baseline/visibility/scoping/non-mutation; clean full
//! lifecycle (`Ready → CleanupPending → Removed`, `workspace.removed`
//! exactly once, event ordering); dirty/staged/untracked refusal with the
//! `Ready`-stays-`Ready` pre-`CleanupPending` gate; dirty-`CleanupPending`
//! fail-closed; unmerged-branch preservation; idempotency; crash recovery
//! before/after Git removal and before DB finalization; `Ready`-missing
//! drift; `Removed` corruption handling; multi-workspace isolation; primary
//! preservation; non-standard default-branch safety; branch drift and moved
//! worktrees; active-owner rejection (stub port); and Task-lifecycle
//! separation. Every database file and Git repo is test-owned.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use app_services::{
    CleanupOutcome, EnsureOutcome, FixedClock, ManagedWorkspaceRoot, RuntimeService, TaskService,
    WorkspaceRequest, WorkspaceService,
};
use git_cli::GitCli;
use storage_sqlite::EventType;
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{
    DiffChangeKind, GitPort, LeaseLifecycle, RepositoryRef, TrustedCommonGit, WorkspaceFailure,
};

static COUNTER: AtomicU64 = AtomicU64::new(1000);

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
            "runlith-workspace-diff-cleanup-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        let db = dir.join("test.db");
        let repo = dir.join("repo dir with spaces");
        let root = dir.join("managed roots");
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

fn seed_execution(
    env: &TestEnv,
    project: &str,
    task_id: &str,
    exec_id: &str,
) -> (ProjectId, TaskId, ExecutionId) {
    let mut tasks = env.task_service();
    let project_id = ProjectId::new(project).unwrap();
    tasks
        .create_project(&Project::new(project_id.clone(), format!("project {project}")).unwrap())
        .unwrap();
    let task_id = TaskId::new(task_id).unwrap();
    let task = Task::draft(
        task_id.clone(),
        project_id.clone(),
        format!("objective {task_id}"),
    )
    .unwrap();
    tasks.create_task(&task).unwrap();
    drop(tasks);
    let exec_id = ExecutionId::new(exec_id).unwrap();
    env.runtime_service()
        .create_execution(exec_id.clone(), task_id.clone())
        .unwrap();
    (project_id, task_id, exec_id)
}

fn request(env: &TestEnv, exec: &ExecutionId, ref_id: &str) -> WorkspaceRequest {
    WorkspaceRequest {
        execution_id: exec.clone(),
        repository_ref: RepositoryRef::new(ref_id).unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    }
}

fn resolved_base(env: &TestEnv) -> String {
    git(&env.repo, &["rev-parse", "HEAD"])
}

fn ensure_ready(
    env: &TestEnv,
    service: &mut WorkspaceService<GitCli, FixedClock>,
    exec: &ExecutionId,
    ref_id: &str,
) -> workspace::WorkspaceLease {
    service
        .ensure_workspace_ready(&request(env, exec, ref_id))
        .unwrap()
        .lease
}

fn created_count(service: &WorkspaceService<GitCli, FixedClock>, ws: &str) -> usize {
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

fn removed_count(service: &WorkspaceService<GitCli, FixedClock>, ws: &str) -> usize {
    service
        .store()
        .list_events_ordered()
        .unwrap()
        .iter()
        .filter(|event| {
            event.fact.event_type == EventType::WorkspaceRemoved
                && event.fact.workspace_id.as_ref().map(|id| id.as_str()) == Some(ws)
        })
        .count()
}

fn event_order(service: &WorkspaceService<GitCli, FixedClock>, ws: &str) -> Vec<EventType> {
    service
        .store()
        .list_events_ordered()
        .unwrap()
        .iter()
        .filter(|event| event.fact.workspace_id.as_ref().map(|id| id.as_str()) == Some(ws))
        .map(|event| event.fact.event_type)
        .collect()
}

// -- diff --------------------------------------------------------------------

#[test]
fn diff_clean_workspace_is_empty_and_authoritative() {
    let env = TestEnv::new("diff-clean");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = resolved_base(&env);
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");

    let evidence = service.compute_diff(lease.id()).unwrap();
    assert_eq!(evidence.diff.base_revision.as_str(), base);
    assert_eq!(evidence.diff.base_revision, *lease.base_revision());
    assert_eq!(evidence.diff.head_revision.as_ref().unwrap().as_str(), base);
    assert!(!evidence.diff.has_working_tree_changes);
    assert!(evidence.diff.committed.is_empty());
    assert!(evidence.diff.working.is_empty());
    // Diff is evidence only: Task lifecycle untouched.
    let tasks = env.task_service();
    let task = tasks
        .get_task(&lease_owner_task(&service, &exec))
        .unwrap()
        .unwrap();
    assert_eq!(task.state(), task_domain::TaskState::Draft);
}

fn lease_owner_task(service: &WorkspaceService<GitCli, FixedClock>, exec: &ExecutionId) -> TaskId {
    let lease = service.workspace_for_execution(exec).unwrap().unwrap();
    let execution = service
        .store()
        .get_execution(lease.owner())
        .unwrap()
        .unwrap();
    execution.task_id().clone()
}

#[test]
fn diff_detects_every_material_change_class() {
    // Tracked unstaged modification.
    {
        let env = TestEnv::new("diff-tracked");
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        std::fs::write(Path::new(lease.worktree_ref()).join("tracked.txt"), b"v1").unwrap();
        git(Path::new(lease.worktree_ref()), &["add", "tracked.txt"]);
        git(
            Path::new(lease.worktree_ref()),
            &["commit", "-q", "-m", "seed tracked"],
        );
        // Advance the recorded base past the seed so the modification below
        // is working-tree dirt, not committed history.
        let _ = resolved_base(&env);
        std::fs::write(Path::new(lease.worktree_ref()).join("tracked.txt"), b"v2").unwrap();
        let evidence = service.compute_diff(lease.id()).unwrap();
        assert!(
            evidence
                .diff
                .working
                .iter()
                .any(|e| e.path == "tracked.txt" && e.kind == DiffChangeKind::Modified),
            "tracked modification missing: {:?}",
            evidence.diff.working
        );
    }
    // Staged-only addition.
    {
        let env = TestEnv::new("diff-staged");
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        std::fs::write(
            Path::new(lease.worktree_ref()).join("staged.txt"),
            b"staged",
        )
        .unwrap();
        git(Path::new(lease.worktree_ref()), &["add", "staged.txt"]);
        let evidence = service.compute_diff(lease.id()).unwrap();
        assert!(
            evidence
                .diff
                .working
                .iter()
                .any(|e| e.path == "staged.txt" && e.kind == DiffChangeKind::Added),
            "staged addition missing: {:?}",
            evidence.diff.working
        );
    }
    // Untracked-only file.
    {
        let env = TestEnv::new("diff-untracked");
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        std::fs::write(
            Path::new(lease.worktree_ref()).join("notes.txt"),
            b"untracked evidence",
        )
        .unwrap();
        let evidence = service.compute_diff(lease.id()).unwrap();
        assert!(
            evidence
                .diff
                .working
                .iter()
                .any(|e| e.path == "notes.txt" && e.kind == DiffChangeKind::Untracked),
            "untracked file missing: {:?}",
            evidence.diff.working
        );
    }
    // Deleted tracked file.
    {
        let env = TestEnv::new("diff-deleted");
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        let victim = Path::new(lease.worktree_ref()).join("victim.txt");
        std::fs::write(&victim, b"doomed").unwrap();
        git(Path::new(lease.worktree_ref()), &["add", "victim.txt"]);
        git(
            Path::new(lease.worktree_ref()),
            &["commit", "-q", "-m", "seed victim"],
        );
        std::fs::remove_file(&victim).unwrap();
        let evidence = service.compute_diff(lease.id()).unwrap();
        assert!(
            evidence
                .diff
                .working
                .iter()
                .any(|e| e.path == "victim.txt" && e.kind == DiffChangeKind::Deleted),
            "deletion missing: {:?}",
            evidence.diff.working
        );
    }
    // HEAD advanced from the recorded base.
    {
        let env = TestEnv::new("diff-head");
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        let base = lease.base_revision().as_str().to_owned();
        std::fs::write(
            Path::new(lease.worktree_ref()).join("work.txt"),
            b"execution work",
        )
        .unwrap();
        git(Path::new(lease.worktree_ref()), &["add", "work.txt"]);
        git(
            Path::new(lease.worktree_ref()),
            &["commit", "-q", "-m", "execution work"],
        );
        let evidence = service.compute_diff(lease.id()).unwrap();
        assert_eq!(evidence.diff.base_revision.as_str(), base);
        assert_ne!(evidence.diff.head_revision.as_ref().unwrap().as_str(), base);
        assert!(
            evidence.diff.committed.iter().any(|e| e.path == "work.txt"),
            "committed work missing: {:?}",
            evidence.diff.committed
        );
    }
}

#[test]
fn diff_does_not_mutate_git() {
    let env = TestEnv::new("diff-nomut");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let wt = PathBuf::from(lease.worktree_ref());
    std::fs::write(wt.join("dirty.txt"), b"dirty").unwrap();
    let head_before = git(&wt, &["rev-parse", "HEAD"]);
    let branch_before = git(&wt, &["symbolic-ref", "--quiet", "HEAD"]);
    let status_before = git(
        &wt,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    );

    let first = service.compute_diff(lease.id()).unwrap();
    let second = service.compute_diff(lease.id()).unwrap();
    assert_eq!(first, second);

    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git(&wt, &["symbolic-ref", "--quiet", "HEAD"]),
        branch_before
    );
    assert_eq!(
        git(
            &wt,
            &["status", "--porcelain=v1", "--untracked-files=normal"]
        ),
        status_before
    );
}

#[test]
fn diff_is_scoped_to_the_requested_lease() {
    let env = TestEnv::new("diff-scope");
    let (_, _, e1) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (_, _, e2) = seed_execution(&env, "P-2", "T-2", "E-2");
    let mut service = env.workspace_service();
    let w1 = ensure_ready(&env, &mut service, &e1, "repo-local:runlith");
    let w2 = ensure_ready(&env, &mut service, &e2, "repo-local:runlith");
    std::fs::write(Path::new(w1.worktree_ref()).join("only-w1.txt"), b"w1").unwrap();
    // Primary change must never leak into a workspace diff.
    std::fs::write(env.repo.join("primary-only.txt"), b"primary").unwrap();

    let d1 = service.compute_diff(w1.id()).unwrap();
    let d2 = service.compute_diff(w2.id()).unwrap();
    assert!(d1.diff.working.iter().any(|e| e.path == "only-w1.txt"));
    assert!(!d1.diff.working.iter().any(|e| e.path == "primary-only.txt"));
    assert!(d2.diff.working.is_empty());
    assert!(std::fs::read(Path::new(w2.worktree_ref()).join("only-w1.txt")).is_err());
}

// -- clean lifecycle ---------------------------------------------------------

#[test]
fn clean_cleanup_runs_the_full_lifecycle_exactly_once() {
    let env = TestEnv::new("clean-life");
    let (_, task_id, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().as_str().to_owned();
    let branch = lease.branch().as_str().to_owned();
    let worktree = lease.worktree_ref().to_owned();
    assert_eq!(created_count(&service, &ws), 1);

    let cleaned = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .unwrap();
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    assert_eq!(cleaned.lease.lifecycle(), LeaseLifecycle::Removed);
    assert!(!Path::new(&worktree).exists());
    assert!(!GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &workspace::BranchRef::new(&branch).unwrap(),
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
    assert_eq!(removed_count(&service, &ws), 1);
    assert_eq!(
        event_order(&service, &ws),
        vec![EventType::WorkspaceCreated, EventType::WorkspaceRemoved]
    );

    // Restart durability: Removed survives reopen.
    drop(service);
    let mut service = env.workspace_service();
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Removed);
    assert_eq!(removed_count(&service, &ws), 1);

    // Idempotent second cleanup: no duplicate event, no recreation.
    let again = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .unwrap();
    assert_eq!(again.outcome, CleanupOutcome::AlreadyRemoved);
    assert_eq!(removed_count(&service, &ws), 1);
    assert_eq!(created_count(&service, &ws), 1);
    assert!(!Path::new(&worktree).exists());

    // Task lifecycle untouched by workspace teardown.
    let tasks = env.task_service();
    let task = tasks.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), task_domain::TaskState::Draft);
}

// -- dirtiness protection ----------------------------------------------------

#[test]
fn dirty_workspaces_are_preserved_and_stay_ready() {
    for (suite, setup) in [
        ("dirty-tracked", "tracked"),
        ("dirty-staged", "staged"),
        ("dirty-untracked", "untracked"),
    ] {
        let env = TestEnv::new(suite);
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        let ws = lease.id().as_str().to_owned();
        let wt = PathBuf::from(lease.worktree_ref());
        // Seed a committed file so the tracked case modifies (not adds).
        std::fs::write(wt.join("keep.txt"), b"v1").unwrap();
        git(&wt, &["add", "keep.txt"]);
        git(&wt, &["commit", "-q", "-m", "seed keep"]);
        match setup {
            "tracked" => std::fs::write(wt.join("keep.txt"), b"v2-user-edit").unwrap(),
            "staged" => {
                std::fs::write(wt.join("staged.txt"), b"staged-bytes").unwrap();
                git(&wt, &["add", "staged.txt"]);
            }
            _ => std::fs::write(wt.join("untracked.txt"), b"user-evidence").unwrap(),
        }
        let err = service
            .cleanup_workspace(lease.id(), &env.repo_path())
            .expect_err("dirty cleanup must refuse");
        assert!(
            matches!(err, WorkspaceFailure::WorktreeDirty { .. }),
            "case {suite}: got {err:?}"
        );
        // Preferred ordering: dirtiness is checked before CleanupPending, so
        // the lease is still Ready — nothing was committed, nothing deleted.
        let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Ready);
        assert_eq!(removed_count(&service, &ws), 0);
        assert_eq!(created_count(&service, &ws), 1);
        match setup {
            "tracked" => assert_eq!(std::fs::read(wt.join("keep.txt")).unwrap(), b"v2-user-edit"),
            "staged" => assert_eq!(
                std::fs::read(wt.join("staged.txt")).unwrap(),
                b"staged-bytes"
            ),
            _ => assert_eq!(
                std::fs::read(wt.join("untracked.txt")).unwrap(),
                b"user-evidence"
            ),
        }
        assert!(wt.is_dir());
    }
}

#[test]
fn dirty_cleanup_pending_fails_closed_without_implying_deletion() {
    let env = TestEnv::new("dirty-pending");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().as_str().to_owned();
    // Operational intent first (no event), then the workspace becomes dirty
    // before Git deletion — the classic TOCTOU shape.
    service.request_cleanup(lease.id()).unwrap();
    let wt = PathBuf::from(lease.worktree_ref());
    std::fs::write(wt.join("late-user-edit.txt"), b"late").unwrap();

    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("dirty CleanupPending must stop");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeDirty { .. }),
        "got {err:?}"
    );
    // Recoverable and honest: still CleanupPending (no invented
    // CleanupPending → Ready), nothing deleted, no removed event.
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::CleanupPending);
    assert_eq!(removed_count(&service, &ws), 0);
    assert_eq!(
        std::fs::read(wt.join("late-user-edit.txt")).unwrap(),
        b"late"
    );
}

// -- branch preservation -----------------------------------------------------

#[test]
fn unmerged_managed_branch_survives_ordinary_cleanup() {
    let env = TestEnv::new("unmerged");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().as_str().to_owned();
    let branch = lease.branch().clone();
    let worktree = lease.worktree_ref().to_owned();
    // Unique execution commit, worktree otherwise clean.
    let wt = PathBuf::from(lease.worktree_ref());
    std::fs::write(wt.join("unique.txt"), b"unique execution work").unwrap();
    git(&wt, &["add", "unique.txt"]);
    git(&wt, &["commit", "-q", "-m", "unique work"]);

    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("unmerged branch must survive");
    assert!(
        matches!(err, WorkspaceFailure::CleanupUnsafe { .. }),
        "got {err:?}"
    );
    // Contract-defined state: worktree removed (it was clean), branch
    // preserved with its unique commits, lease still CleanupPending, no
    // workspace.removed. Future delivery owns the branch; G4 never
    // force-deletes it.
    assert!(!Path::new(&worktree).exists());
    assert!(GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &branch,
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::CleanupPending);
    assert_eq!(removed_count(&service, &ws), 0);
    assert_eq!(created_count(&service, &ws), 1);
}

// -- crash recovery ----------------------------------------------------------

#[test]
fn crash_after_cleanup_pending_recovers_to_removed() {
    let env = TestEnv::new("crash-pending");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let ws = {
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        service.request_cleanup(lease.id()).unwrap();
        lease.id().clone()
    };
    // Crash before any Git removal: reopen sees CleanupPending + clean
    // worktree and completes safely with exactly one removed event.
    let mut service = env.workspace_service();
    let cleaned = service.cleanup_workspace(&ws, &env.repo_path()).unwrap();
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    assert_eq!(removed_count(&service, ws.as_str()), 1);
}

#[test]
fn crash_after_worktree_removal_continues_through_branch() {
    let env = TestEnv::new("crash-wt-gone");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (ws, worktree, branch) = {
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        service.request_cleanup(lease.id()).unwrap();
        (
            lease.id().clone(),
            lease.worktree_ref().to_owned(),
            lease.branch().as_str().to_owned(),
        )
    };
    // Crash between worktree removal and branch handling: emulate the
    // completed worktree step directly through Git (no SQLite touched).
    git(&env.repo, &["worktree", "remove", &worktree]);
    assert!(!Path::new(&worktree).exists());

    let mut service = env.workspace_service();
    let cleaned = service.cleanup_workspace(&ws, &env.repo_path()).unwrap();
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    assert_eq!(removed_count(&service, ws.as_str()), 1);
    let branch_ref = workspace::BranchRef::new(&branch).unwrap();
    assert!(!GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &branch_ref,
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
}

#[test]
fn crash_after_full_git_cleanup_finalizes_exactly_once() {
    let env = TestEnv::new("crash-finalize");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (ws, worktree, branch) = {
        let mut service = env.workspace_service();
        let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
        service.request_cleanup(lease.id()).unwrap();
        (
            lease.id().clone(),
            lease.worktree_ref().to_owned(),
            lease.branch().as_str().to_owned(),
        )
    };
    // All Git resources already in their final absent state, SQLite still
    // CleanupPending: the SQLite atomicity seam. No removed event exists
    // yet (neither half committed), and Git is never touched blindly again.
    git(&env.repo, &["worktree", "remove", &worktree]);
    git(
        &env.repo,
        &["branch", "-d", branch.trim_start_matches("refs/heads/")],
    );
    {
        let service = env.workspace_service();
        assert_eq!(removed_count(&service, ws.as_str()), 0);
        let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::CleanupPending);
    }
    let mut service = env.workspace_service();
    let cleaned = service.cleanup_workspace(&ws, &env.repo_path()).unwrap();
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    // Exactly once across the restart: no Removed without event, no event
    // without Removed, no second event on retry.
    assert_eq!(removed_count(&service, ws.as_str()), 1);
    let again = service.cleanup_workspace(&ws, &env.repo_path()).unwrap();
    assert_eq!(again.outcome, CleanupOutcome::AlreadyRemoved);
    assert_eq!(removed_count(&service, ws.as_str()), 1);
}

// -- drift and corruption ----------------------------------------------------

#[test]
fn ready_missing_worktree_is_drift_never_removed() {
    let env = TestEnv::new("ready-missing");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().as_str().to_owned();
    // External deletion without cleanup intent (stale registration left
    // behind on purpose: no prune, no silent repair).
    std::fs::remove_dir_all(lease.worktree_ref()).unwrap();

    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("Ready + missing must not become Removed");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeMissing { .. }),
        "got {err:?}"
    );
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(removed_count(&service, &ws), 0);
}

#[test]
fn removed_lease_with_live_worktree_is_corruption() {
    let env = TestEnv::new("removed-drift");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().clone();
    let worktree = lease.worktree_ref().to_owned();
    service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .unwrap();
    // Plant unexpected live state under a terminal lease.
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(Path::new(&worktree).join("rogue.txt"), b"rogue").unwrap();

    let err = service
        .cleanup_workspace(&ws, &env.repo_path())
        .expect_err("Removed must never resurrect or blindly delete");
    assert!(
        matches!(err, WorkspaceFailure::CheckoutFailed { .. }),
        "got {err:?}"
    );
    // No resurrection, no deletion, no duplicate event.
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Removed);
    assert_eq!(removed_count(&service, ws.as_str()), 1);
    assert_eq!(
        std::fs::read(Path::new(&worktree).join("rogue.txt")).unwrap(),
        b"rogue"
    );
    std::fs::remove_dir_all(&worktree).unwrap();
}

// -- isolation ---------------------------------------------------------------

#[test]
fn cleaning_one_lease_leaves_siblings_and_primary_untouched() {
    let env = TestEnv::new("iso");
    let (_, _, e1) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (_, _, e2) = seed_execution(&env, "P-2", "T-2", "E-2");
    // Dirty primary checkout: never cleaned, never blocks managed cleanup.
    std::fs::write(env.repo.join("primary-dirty.txt"), b"dirty-primary").unwrap();
    let primary_status_before = git(
        &env.repo,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    );
    let mut service = env.workspace_service();
    let w1 = ensure_ready(&env, &mut service, &e1, "repo-local:runlith");
    let w2 = ensure_ready(&env, &mut service, &e2, "repo-local:runlith");
    // Dirty sibling: cleaning W1 must not reset, clean, or remove W2.
    std::fs::write(
        Path::new(w2.worktree_ref()).join("sibling-evidence.txt"),
        b"sibling",
    )
    .unwrap();
    let w1_branch = w1.branch().as_str().to_owned();
    let w1_worktree = w1.worktree_ref().to_owned();

    let cleaned = service
        .cleanup_workspace(w1.id(), &env.repo_path())
        .unwrap();
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    assert!(!Path::new(&w1_worktree).exists());
    let w1_ref = workspace::BranchRef::new(&w1_branch).unwrap();
    assert!(!GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &w1_ref,
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());

    // Sibling intact: files, branch, dirtiness.
    assert_eq!(
        std::fs::read(Path::new(w2.worktree_ref()).join("sibling-evidence.txt")).unwrap(),
        b"sibling"
    );
    assert!(GitCli::new()
        .branch_exists(
            &env.repo_path(),
            w2.branch(),
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
    let w2_after = service.workspace_for_execution(&e2).unwrap().unwrap();
    assert_eq!(w2_after.lifecycle(), LeaseLifecycle::Ready);

    // Primary intact: bytes and status byte-identical.
    assert_eq!(
        std::fs::read(env.repo.join("primary-dirty.txt")).unwrap(),
        b"dirty-primary"
    );
    assert_eq!(
        git(
            &env.repo,
            &["status", "--porcelain=v1", "--untracked-files=normal"]
        ),
        primary_status_before
    );
    // Sibling cleanup now correctly refuses (dirty), preserving evidence.
    let err = service
        .cleanup_workspace(w2.id(), &env.repo_path())
        .expect_err("dirty sibling must be preserved");
    assert!(matches!(err, WorkspaceFailure::WorktreeDirty { .. }));
}

#[test]
fn default_branch_named_trunkline_survives_cleanup() {
    let env = TestEnv::new("trunkline");
    git(&env.repo, &["branch", "-m", "trunkline"]);
    git(
        &env.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/heads/trunkline",
        ],
    );
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    assert_ne!(lease.branch().as_str(), "refs/heads/trunkline");
    service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .unwrap();
    assert!(GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &workspace::BranchRef::new("refs/heads/trunkline").unwrap(),
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
}

// -- ownership ---------------------------------------------------------------

#[test]
fn branch_drift_fails_closed_without_deleting_either_branch() {
    let env = TestEnv::new("drift");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().as_str().to_owned();
    let managed = lease.branch().as_str().to_owned();
    // The live worktree moves to an unexpected branch: durable and live
    // facts disagree, so neither branch may be deleted.
    let wt = PathBuf::from(lease.worktree_ref());
    git(&wt, &["checkout", "-q", "-b", "unexpected-branch"]);
    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("branch drift must fail closed");
    assert!(
        matches!(
            err,
            WorkspaceFailure::OwnershipMismatch { .. } | WorkspaceFailure::CheckoutFailed { .. }
        ),
        "got {err:?}"
    );
    assert!(GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &workspace::BranchRef::new(&managed).unwrap(),
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
    assert!(GitCli::new()
        .branch_exists(
            &env.repo_path(),
            &workspace::BranchRef::new("refs/heads/unexpected-branch").unwrap(),
            &GitCli::new()
                .establish_trusted_anchor(&env.repo_path())
                .unwrap()
        )
        .unwrap());
    assert!(wt.is_dir());
    assert_eq!(removed_count(&service, &ws), 0);
}

#[test]
fn moved_worktree_is_never_chased_and_deleted() {
    let env = TestEnv::new("moved");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    let ws = lease.id().as_str().to_owned();
    let stale = lease.worktree_ref().to_owned();
    let relocated = env.root.join("relocated-elsewhere");
    git(
        &env.repo,
        &["worktree", "move", &stale, relocated.to_str().unwrap()],
    );
    assert!(Path::new(&relocated.join(".git")).exists() || relocated.is_dir());

    // The durable path is now absent: Ready + missing is drift, and the
    // service must not chase the relocated path and delete it.
    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("moved worktree must not be chased");
    assert!(
        matches!(
            err,
            WorkspaceFailure::WorktreeMissing { .. }
                | WorkspaceFailure::OwnershipMismatch { .. }
                | WorkspaceFailure::CheckoutFailed { .. }
        ),
        "got {err:?}"
    );
    assert!(relocated.is_dir(), "relocated worktree must survive");
    assert_eq!(removed_count(&service, &ws), 0);
}

#[test]
fn invalid_worktree_path_fails_closed() {
    let env = TestEnv::new("invalid-path");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = ensure_ready(&env, &mut service, &exec, "repo-local:runlith");
    // Replace the worktree directory with a plain file: present but Invalid.
    std::fs::remove_dir_all(lease.worktree_ref()).unwrap();
    std::fs::write(lease.worktree_ref(), b"not a directory").unwrap();
    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("file-as-worktree must fail closed");
    assert!(
        matches!(
            err,
            WorkspaceFailure::CheckoutFailed { .. } | WorkspaceFailure::OwnershipMismatch { .. }
        ),
        "got {err:?}"
    );
    assert_eq!(removed_count(&service, lease.id().as_str()), 0);
    std::fs::remove_file(lease.worktree_ref()).unwrap();
}

// -- active owner (stub port) --------------------------------------------------

/// Minimal stub proving the service rejects cleanup for live owners without
/// inventing process heuristics. Every other behavior is honest and static.
struct LiveOwnerPort;

impl GitPort for LiveOwnerPort {
    fn backend_ref(&self) -> &'static str {
        "stub-live-owner"
    }
    fn establish_trusted_anchor(
        &self,
        _trusted_repository_path: &str,
    ) -> Result<TrustedCommonGit, WorkspaceFailure> {
        Err(WorkspaceFailure::GitBackendFailure {
            detail: "stub cannot establish".to_owned(),
        })
    }
    fn is_repository(
        &self,
        _path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure> {
        Ok(true)
    }
    fn default_branch(
        &self,
        _repository_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<String, WorkspaceFailure> {
        Ok("refs/heads/trunk".to_owned())
    }
    fn resolve_revision(
        &self,
        _repository_path: &str,
        _rev: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<workspace::Revision, WorkspaceFailure> {
        workspace::Revision::new("a".repeat(40)).map_err(|e| WorkspaceFailure::GitBackendFailure {
            detail: e.to_string(),
        })
    }
    fn create_worktree(
        &self,
        _lease: &workspace::WorkspaceLease,
        _repository_path: &str,
        _worktree_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        Ok(())
    }
    fn worktree_status(
        &self,
        _worktree_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<workspace::WorktreeStatus, WorkspaceFailure> {
        Ok(workspace::WorktreeStatus::Clean)
    }
    fn compute_diff(
        &self,
        lease: &workspace::WorkspaceLease,
        _expected: &TrustedCommonGit,
    ) -> Result<workspace::DiffEvidence, WorkspaceFailure> {
        Ok(workspace::DiffEvidence {
            diff: workspace::WorktreeDiff {
                base_revision: lease.base_revision().clone(),
                head_revision: None,
                has_working_tree_changes: false,
                status: workspace::WorktreeStatus::Clean,
                committed: Vec::new(),
                working: Vec::new(),
                truncated: false,
            },
            summary: "stub".to_owned(),
        })
    }
    fn remove_worktree(
        &self,
        _lease: &workspace::WorkspaceLease,
        _worktree_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        panic!("live owner must be rejected before any Git mutation");
    }
    fn remove_branch(
        &self,
        _repository_path: &str,
        _branch: &workspace::BranchRef,
        _expected: &TrustedCommonGit,
    ) -> Result<(), WorkspaceFailure> {
        panic!("live owner must be rejected before any Git mutation");
    }
    fn branch_exists(
        &self,
        _repository_path: &str,
        _branch: &workspace::BranchRef,
        _expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure> {
        Ok(true)
    }
    fn is_registered_worktree(
        &self,
        _repository_path: &str,
        _worktree_path: &str,
        _expected: &TrustedCommonGit,
    ) -> Result<bool, WorkspaceFailure> {
        // Static stub: binding assumed so the test reaches the live-owner
        // rejection it exists to prove.
        Ok(true)
    }
    fn owner_is_live(
        &self,
        _repository: &RepositoryRef,
        _owner: &ExecutionId,
    ) -> Result<bool, WorkspaceFailure> {
        Ok(true)
    }
}

#[test]
fn active_owner_rejects_cleanup_before_any_mutation() {
    let env = TestEnv::new("live-owner");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    // Reach Ready through the real backend first.
    let lease_id = {
        let mut service = env.workspace_service();
        ensure_ready(&env, &mut service, &exec, "repo-local:runlith")
            .id()
            .clone()
    };
    // Reopen against the stub liveness source: same database, live owner.
    let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
    let mut service: WorkspaceService<LiveOwnerPort, FixedClock> = WorkspaceService::attach(
        store,
        LiveOwnerPort,
        ManagedWorkspaceRoot::new(&env.root).unwrap(),
        FixedClock(FIXED_MILLIS),
    );
    let err = service
        .cleanup_workspace(&lease_id, &env.repo_path())
        .expect_err("live owner must reject cleanup");
    assert!(
        matches!(err, WorkspaceFailure::ActiveOwnerRejectsCleanup { .. }),
        "got {err:?}"
    );
    // No state change, no event (the panic in the stub mutation methods
    // would also fail the test if reached).
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Ready);
}

// -- creation reconciliation still green -------------------------------------

#[test]
fn creation_reconciliation_matrix_survives_cleanup_changes() {
    // Crash-A creation shape still resumes (TASK-0042 regression anchor).
    let env = TestEnv::new("creation-still-green");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    {
        let mut service = env.workspace_service();
        let lease = service
            .allocate_workspace(&request(&env, &exec, "repo-local:runlith"))
            .unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
    }
    let mut service = env.workspace_service();
    let result = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(result.outcome, EnsureOutcome::ResumedAllocated);
    assert_eq!(result.lease.lifecycle(), LeaseLifecycle::Ready);
}
