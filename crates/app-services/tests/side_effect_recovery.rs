//! ADR-0031 dogfood on worktree materialization (TASK-0126 phase c):
//! crash injected between the Git effect and the state commit recovers
//! through RECONCILE with no duplicate external mutation and no silent
//! retry. Real Git + real file-backed SQLite + fault-injecting GitPort
//! wrappers; restart is a dropped-and-reopened service.
//!
//! Rule IDs (SE-1..SE-8) appear verbatim in implementing test names
//! per ADR-0031 §5.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use agent_runtime::ExecutionId;
use app_services::{
    FixedClock, ManagedWorkspaceRoot, RuntimeService, TaskService, WorkspaceRequest,
    WorkspaceService,
};
use git_cli::GitCli;
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{
    BranchRef, GitPort, LeaseLifecycle, RepositoryRef, Revision, SideEffectKind, SideEffectState,
    TrustedCommonGit, WorkspaceLease, WorktreeStatus,
};

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
            "runlith-side-effect-{suite}-{id}-{pid}",
            pid = std::process::id()
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

/// Git backend that performs the real mutation, then reports the
/// outcome as unknown (crash/timeout between effect and commit).
/// Dispatch counting proves no-duplicate-mutation across resume.
struct CrashAfterDispatch {
    inner: GitCli,
    creates: std::sync::Arc<AtomicUsize>,
    crash_next: AtomicUsize,
}

impl CrashAfterDispatch {
    fn new(crash_next: usize) -> Self {
        Self::with_counter(std::sync::Arc::new(AtomicUsize::new(0)), crash_next)
    }

    fn with_counter(creates: std::sync::Arc<AtomicUsize>, crash_next: usize) -> Self {
        Self {
            inner: GitCli::new(),
            creates,
            crash_next: AtomicUsize::new(crash_next),
        }
    }
}

/// Git backend that fails the first dispatch without touching Git
/// (crash between INTENT_RECORDED and dispatch), then delegates.
struct FailFirstDispatch {
    inner: GitCli,
    calls: AtomicUsize,
}

impl FailFirstDispatch {
    fn new() -> Self {
        Self {
            inner: GitCli::new(),
            calls: AtomicUsize::new(0),
        }
    }
}

macro_rules! delegate {
    ($name:ident ( $($arg:ident : $ty:ty),* ) -> $ret:ty) => {
        fn $name(&self, $($arg: $ty),*) -> $ret {
            self.inner.$name($($arg),*)
        }
    };
}

macro_rules! delegate_all {
    () => {
        delegate!(establish_trusted_anchor(trusted_repository_path: &str) -> Result<TrustedCommonGit, workspace::WorkspaceFailure>);
        delegate!(is_repository(path: &str, expected: &TrustedCommonGit) -> Result<bool, workspace::WorkspaceFailure>);
        delegate!(default_branch(repository_path: &str, expected: &TrustedCommonGit) -> Result<String, workspace::WorkspaceFailure>);
        delegate!(resolve_revision(repository_path: &str, rev: &str, expected: &TrustedCommonGit) -> Result<Revision, workspace::WorkspaceFailure>);
        delegate!(worktree_status(worktree_path: &str, expected: &TrustedCommonGit) -> Result<WorktreeStatus, workspace::WorkspaceFailure>);
        delegate!(compute_diff(lease: &WorkspaceLease, expected: &TrustedCommonGit) -> Result<workspace::DiffEvidence, workspace::WorkspaceFailure>);
        delegate!(remove_worktree(lease: &WorkspaceLease, worktree_path: &str, expected: &TrustedCommonGit) -> Result<(), workspace::WorkspaceFailure>);
        delegate!(remove_branch(repository_path: &str, branch: &BranchRef, expected: &TrustedCommonGit) -> Result<(), workspace::WorkspaceFailure>);
        delegate!(branch_exists(repository_path: &str, branch: &BranchRef, expected: &TrustedCommonGit) -> Result<bool, workspace::WorkspaceFailure>);
        delegate!(is_registered_worktree(repository_path: &str, worktree_path: &str, expected: &TrustedCommonGit) -> Result<bool, workspace::WorkspaceFailure>);
        delegate!(attach_worktree(lease: &WorkspaceLease, repository_path: &str, worktree_path: &str, expected: &TrustedCommonGit) -> Result<(), workspace::WorkspaceFailure>);
        delegate!(owner_is_live(repository: &RepositoryRef, owner: &ExecutionId) -> Result<bool, workspace::WorkspaceFailure>);
    };
}

impl GitPort for CrashAfterDispatch {
    fn backend_ref(&self) -> &'static str {
        self.inner.backend_ref()
    }

    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), workspace::WorkspaceFailure> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        let outcome = self
            .inner
            .create_worktree(lease, repository_path, worktree_path, expected);
        if self
            .crash_next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
            && outcome.is_ok()
        {
            // The effect landed; the caller never learns it (SE-2).
            return Err(workspace::WorkspaceFailure::GitBackendFailure {
                detail: "injected crash: dispatch outcome unknown".to_owned(),
            });
        }
        outcome
    }

    delegate_all!();
}

impl GitPort for FailFirstDispatch {
    fn backend_ref(&self) -> &'static str {
        self.inner.backend_ref()
    }

    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), workspace::WorkspaceFailure> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            // Crash before the first dispatch: the service recorded
            // the intent but Git never ran (indistinguishable from a
            // mid-dispatch crash from the service's view).
            return Err(workspace::WorkspaceFailure::GitBackendFailure {
                detail: "injected crash before first dispatch".to_owned(),
            });
        }
        self.inner
            .create_worktree(lease, repository_path, worktree_path, expected)
    }

    delegate_all!();
}

fn intent_state<G: GitPort>(
    service: &WorkspaceService<G, FixedClock>,
    lease_id: &workspace::WorkspaceId,
) -> SideEffectState {
    service
        .store()
        .get_side_effect_intent_for_lease(lease_id.as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .expect("materialize intent exists")
        .state()
}

#[test]
fn se_1_materialize_records_intent_before_git_dispatch() {
    let env = TestEnv::new("se-1");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
    let git = CrashAfterDispatch::new(1);
    let mut service = WorkspaceService::attach(
        store,
        git,
        ManagedWorkspaceRoot::new(&env.root).unwrap(),
        FixedClock(FIXED_MILLIS),
    );
    let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
    // Crash after the real dispatch: the caller sees failure, but the
    // durable intent must already exist (SE-1: never an effect without
    // a prior durable intent).
    assert!(service
        .materialize_allocated(&lease, &env.repo_path())
        .is_err());
    let intent = service
        .store()
        .get_side_effect_intent_for_lease(lease.id().as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .expect("SE-1: intent is durable before any dispatch");
    assert_eq!(intent.state(), SideEffectState::Ambiguous);
}

#[test]
fn se_2_crash_yields_ambiguous_never_failed() {
    let env = TestEnv::new("se-2");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
    let mut service = WorkspaceService::attach(
        store,
        CrashAfterDispatch::new(1),
        ManagedWorkspaceRoot::new(&env.root).unwrap(),
        FixedClock(FIXED_MILLIS),
    );
    let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
    assert!(service
        .materialize_allocated(&lease, &env.repo_path())
        .is_err());
    // Unknown outcome is AMBIGUOUS: FAILED would assert the target is
    // unchanged, which nothing has proven.
    assert_eq!(
        intent_state(&service, lease.id()),
        SideEffectState::Ambiguous
    );
}

#[test]
fn se_3_ambiguous_intent_refuses_redispatch_until_reconcile() {
    let env = TestEnv::new("se-3");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
    let mut service = WorkspaceService::attach(
        store,
        CrashAfterDispatch::new(1),
        ManagedWorkspaceRoot::new(&env.root).unwrap(),
        FixedClock(FIXED_MILLIS),
    );
    let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
    assert!(service
        .materialize_allocated(&lease, &env.repo_path())
        .is_err());
    // A second direct dispatch is a blind retry: refused until
    // RECONCILE proves the target state (SE-3).
    let error = service
        .materialize_allocated(&lease, &env.repo_path())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("reconcile"),
        "refusal must name reconciliation, got {error:?}"
    );
}

#[test]
fn se_5_crash_between_effect_and_commit_confirms_via_reconcile() {
    let env = TestEnv::new("se-5");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let creates = std::sync::Arc::new(AtomicUsize::new(0));
    let lease_id;
    {
        let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
        let mut service = WorkspaceService::attach(
            store,
            CrashAfterDispatch::with_counter(creates.clone(), 1),
            ManagedWorkspaceRoot::new(&env.root).unwrap(),
            FixedClock(FIXED_MILLIS),
        );
        let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
        lease_id = lease.id().clone();
        assert!(service
            .materialize_allocated(&lease, &env.repo_path())
            .is_err());
    }
    // Restart: drop everything, reopen, resume through the product
    // (still counting dispatches).
    let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
    let mut service = WorkspaceService::attach(
        store,
        CrashAfterDispatch::with_counter(creates.clone(), 0),
        ManagedWorkspaceRoot::new(&env.root).unwrap(),
        FixedClock(FIXED_MILLIS),
    );
    let ensured = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("resume reconciles and finalizes");
    assert_eq!(ensured.lease.lifecycle(), LeaseLifecycle::Ready);
    let intent = service
        .store()
        .get_side_effect_intent_for_lease(lease_id.as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .unwrap();
    assert_eq!(intent.state(), SideEffectState::Confirmed);
    // No duplicate mutation: the fused dispatch ran exactly once
    // across crash + resume (crash-B reconciles; it never
    // re-dispatches).
    assert_eq!(creates.load(Ordering::SeqCst), 1);
    let trail = service
        .store()
        .list_side_effect_transitions(intent.intent_id())
        .unwrap();
    assert!(
        trail
            .iter()
            .any(|row| row.to_state == SideEffectState::Reconcile),
        "confirmation runs through RECONCILE, got {trail:?}"
    );
}

/// Git backend that creates only the managed branch, then reports
/// the fused dispatch as unknown (deterministic branch-only crash).
struct BranchOnlyCrash {
    inner: GitCli,
}

impl BranchOnlyCrash {
    fn new() -> Self {
        Self {
            inner: GitCli::new(),
        }
    }
}

impl GitPort for BranchOnlyCrash {
    fn backend_ref(&self) -> &'static str {
        self.inner.backend_ref()
    }

    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
        expected: &TrustedCommonGit,
    ) -> Result<(), workspace::WorkspaceFailure> {
        let output = std::process::Command::new("git")
            .args([
                "branch",
                lease.branch().short_name(),
                lease.base_revision().as_str(),
            ])
            .current_dir(repository_path)
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("fixture git branch");
        assert!(output.status.success(), "branch-only crash setup");
        let _ = (worktree_path, expected);
        Err(workspace::WorkspaceFailure::GitBackendFailure {
            detail: "injected crash mid-fused-dispatch".to_owned(),
        })
    }

    delegate_all!();
}

#[test]
fn branch_only_crash_continues_through_attach_not_recreate() {
    let env = TestEnv::new("branch-only");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let lease_id;
    {
        let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
        let mut service = WorkspaceService::attach(
            store,
            BranchOnlyCrash::new(),
            ManagedWorkspaceRoot::new(&env.root).unwrap(),
            FixedClock(FIXED_MILLIS),
        );
        let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
        lease_id = lease.id().clone();
        assert!(service
            .materialize_allocated(&lease, &env.repo_path())
            .is_err());
    }
    let mut service = env.workspace_service();
    let ensured = service
        .ensure_workspace_ready(&request(&env, &exec))
        .expect("branch-only resumes through attach");
    assert_eq!(ensured.lease.lifecycle(), LeaseLifecycle::Ready);
    let intent = service
        .store()
        .get_side_effect_intent_for_lease(lease_id.as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .unwrap();
    assert_eq!(intent.state(), SideEffectState::Confirmed);
    let trail = service
        .store()
        .list_side_effect_transitions(intent.intent_id())
        .unwrap();
    assert!(
        trail
            .iter()
            .any(|row| row.to_state == SideEffectState::SafeToContinue),
        "branch-only resumes through SAFE_TO_CONTINUE, got {trail:?}"
    );
}

#[test]
fn se_8_crash_c_escalates_with_typed_blocker_and_same_error() {
    let env = TestEnv::new("se-8");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
    // Crash-C shape: foreign bytes occupy the target (no branch, no
    // worktree) — unrecognized, so RECONCILE proves nothing.
    let target = PathBuf::from(lease.worktree_ref());
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("foreign.txt"), b"not ours").unwrap();
    drop(service);
    let mut service = env.workspace_service();
    let error = service
        .ensure_workspace_ready(&request(&env, &exec))
        .unwrap_err();
    // Externally proven behavior is unchanged: the same refusal.
    assert!(matches!(
        error,
        workspace::WorkspaceFailure::CheckoutFailed { .. }
    ));
    // Plus the durable ESCALATE trail (SE-8).
    let intent = service
        .store()
        .get_side_effect_intent_for_lease(lease.id().as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .expect("crash-C records an intent");
    assert_eq!(intent.state(), SideEffectState::Escalate);
}

#[test]
fn materialize_collision_records_failed_without_effect() {
    let env = TestEnv::new("collision");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
    // Pre-existing branch: the adapter preflight refuses before any
    // mutation, so the target is provably unchanged (FAILED).
    git(&env.repo, &["branch", lease.branch().short_name(), "HEAD"]);
    let error = service
        .materialize_allocated(&lease, &env.repo_path())
        .unwrap_err();
    assert!(matches!(
        error,
        workspace::WorkspaceFailure::BranchCollision { .. }
    ));
    assert_eq!(intent_state(&service, lease.id()), SideEffectState::Failed);
}

#[test]
fn se_4_redispatch_represents_same_idempotency_key() {
    let env = TestEnv::new("se-4");
    let exec = seed_execution(&env, "P-1", "T-1", "E-1");
    let store = storage_sqlite::SqliteStore::open(&env.db).unwrap();
    let mut service = WorkspaceService::attach(
        store,
        FailFirstDispatch::new(),
        ManagedWorkspaceRoot::new(&env.root).unwrap(),
        FixedClock(FIXED_MILLIS),
    );
    let lease = service.allocate_workspace(&request(&env, &exec)).unwrap();
    // First dispatch crashes pre-effect; the service cannot tell that
    // from a mid-dispatch crash, so the intent is AMBIGUOUS.
    assert!(service
        .materialize_allocated(&lease, &env.repo_path())
        .is_err());
    let key_before = service
        .store()
        .get_side_effect_intent_for_lease(lease.id().as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .unwrap()
        .idempotency_key()
        .to_owned();
    drop(service);
    // Resume proves crash-A (nothing present) and re-dispatches under
    // the same idempotency key — one lineage, no silent new identity.
    let mut service = env.workspace_service();
    let ensured = service
        .ensure_workspace_ready(&request(&env, &exec))
        .unwrap();
    assert_eq!(ensured.lease.lifecycle(), LeaseLifecycle::Ready);
    let intent = service
        .store()
        .get_side_effect_intent_for_lease(lease.id().as_str(), SideEffectKind::WorktreeMaterialize)
        .unwrap()
        .unwrap();
    assert_eq!(intent.state(), SideEffectState::Confirmed);
    assert_eq!(intent.idempotency_key(), key_before);
    let trail = service
        .store()
        .list_side_effect_transitions(intent.intent_id())
        .unwrap();
    assert!(
        trail
            .iter()
            .any(|row| row.to_state == SideEffectState::SafeToRetry),
        "crash-A resumes through SAFE_TO_RETRY, got {trail:?}"
    );
}
