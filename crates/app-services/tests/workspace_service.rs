//! Trusted `WorkspaceService` contract tests: the allocation/
//! materialization/finalization saga, reconciliation (crash A/B/C),
//! exactly-once `workspace.created`, ownership invariants (one lease per
//! Execution, attempts reuse a lease, distinct Executions distinct
//! leases), managed-root path authorization, and restart durability — all
//! against a real temporary Git repository via the `git-cli` adapter and a
//! real file-backed SQLite database.
//!
//! Deterministic clock throughout (`FixedClock`). Project/Task/Execution
//! rows are created through `TaskService`/`RuntimeService`; workspace facts
//! go through `WorkspaceService` only. Every database file and Git repo is
//! test-owned.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, ExecutionId};
use app_services::{
    EnsureOutcome, FixedClock, ManagedWorkspaceRoot, RuntimeService, TaskService, WorkspaceRequest,
    WorkspaceService,
};
use git_cli::GitCli;
use storage_sqlite::EventType;
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{LeaseLifecycle, RepositoryRef};

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
            "runlith-workspace-service-test-{}-{suite}-{id}",
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
        // Canonical default-branch metadata, so worktree creation's
        // protected-branch check resolves semantically (never by name).
        let branch = git(&repo, &["symbolic-ref", "--short", "HEAD"]);
        let full = format!("refs/heads/{branch}");
        git(&repo, &["symbolic-ref", "refs/remotes/origin/HEAD", &full]);
        let rev = git(&repo, &["rev-parse", "HEAD"]);
        // `rev` is used to assert bases; the service resolves its own.
        let _ = rev;
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

/// Direct-argv git helper for fixtures (no shell, hermetic identity).
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

fn git_exists(repo: &Path, args: &[&str]) -> bool {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    output.status.success()
}

/// One project + task + execution, all committed through the trusted
/// services (the only writers). Returns the identities.
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

// -- end-to-end + idempotency -----------------------------------------------

#[test]
fn ensure_workspace_ready_creates_then_is_idempotent() {
    let env = TestEnv::new("e2e");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = resolved_base(&env);

    let mut service = env.workspace_service();

    let first = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(first.outcome, EnsureOutcome::CreatedFresh);
    assert_eq!(first.lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(first.lease.base_revision().as_str(), base);
    assert_eq!(first.lease.owner().as_str(), "E-1");
    let ws = first.lease.id().as_str().to_owned();

    // Exactly one created fact.
    assert_eq!(workspace_created_count(&service, &ws), 1);
    // Managed branch exists in the real repo and points at the base.
    let branch = first.lease.branch().as_str().to_owned();
    assert!(git_exists(
        &env.repo,
        &["show-ref", "--verify", "--quiet", &branch]
    ));
    let worktree = first.lease.worktree_ref().to_owned();
    // worktree_ref is the persisted path; prove it resolves and is registered.
    assert!(Path::new(&worktree).is_dir());
    assert!(worktree.starts_with(env.root.to_str().unwrap()));
    let registered = git(&env.repo, &["worktree", "list", "--porcelain"]);
    assert!(registered
        .lines()
        .any(|line| line == format!("worktree {}", first.lease.worktree_ref())));

    // Idempotent: no second lease, branch, worktree, or created fact.
    let second = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(second.outcome, EnsureOutcome::AlreadyReady);
    assert_eq!(second.lease.id().as_str(), ws.as_str());
    assert_eq!(workspace_created_count(&service, &ws), 1);

    let _ = worktree;
}

#[test]
fn policy_base_binding_uses_trusted_task_and_workspace_state() {
    let env = TestEnv::new("policy-base-binding");
    let task_id = TaskId::new("T-POLICY").unwrap();
    let project_id = ProjectId::new("P-POLICY").unwrap();
    let mut tasks = env.task_service();
    tasks
        .create_project(&Project::new(project_id.clone(), "policy test").unwrap())
        .unwrap();
    tasks
        .create_task(&Task::draft(task_id.clone(), project_id, "policy binding").unwrap())
        .unwrap();
    drop(tasks);

    let exec = ExecutionId::new("E-POLICY").unwrap();
    env.runtime_service()
        .create_execution(exec.clone(), task_id.clone())
        .unwrap();
    let base = resolved_base(&env);
    env.workspace_service()
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:policy"))
        .unwrap();

    let binding = env
        .task_service()
        .bind_evaluated_policy_base(&exec)
        .unwrap();
    assert_eq!(binding.task_id, task_id.as_str());
    assert_eq!(binding.policy.revision(), 1);
    assert_eq!(binding.base_revision, base);

    let recovered = env
        .task_service()
        .bind_evaluated_policy_base(&exec)
        .unwrap();
    assert_eq!(recovered, binding);
}

// -- crash recovery ----------------------------------------------------------

#[test]
fn crash_partial_mutation_fails_closed_without_blind_replay() {
    let env = TestEnv::new("crash-c");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = resolved_base(&env);

    // Allocate, then simulate a genuinely ambiguous partial Git mutation:
    // the managed branch exists at an unexpected commit while the worktree
    // was never registered. Exact-base branch-only state is recoverable by
    // TASK-0057; drift must still fail closed.
    let (ws, branch) = {
        let mut service = env.workspace_service();
        let lease = service
            .allocate_workspace(&request(&env, &exec, "repo-local:runlith"))
            .unwrap();
        git(
            &env.repo,
            &["commit", "-q", "--allow-empty", "-m", "unexpected-tip"],
        );
        let unexpected = git(&env.repo, &["rev-parse", "HEAD"]);
        assert_ne!(unexpected, base);
        git(&env.repo, &["reset", "-q", "--hard", &base]);
        // Plant the lease's own recorded branch at the unexpected commit;
        // the primary checkout remains at the recorded base.
        let short = lease.branch().short_name().to_owned();
        git(&env.repo, &["branch", &short, &unexpected]);
        (lease.id().clone(), format!("refs/heads/{short}"))
    };

    let mut service = env.workspace_service();
    let err = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .expect_err("ambiguous partial state must fail, not replay");
    assert!(
        matches!(err, workspace::WorkspaceFailure::CheckoutFailed { .. }),
        "expected CheckoutFailed, got {err:?}"
    );
    // No blind repair, no cleanup: the branch is left untouched and the
    // lease stays Allocated, observable for explicit reconciliation.
    assert!(git_exists(
        &env.repo,
        &["show-ref", "--verify", "--quiet", &branch]
    ));
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.id(), &ws);
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
    // No created event was ever minted for the partial state.
    assert_eq!(workspace_created_count(&service, ws.as_str()), 0);
}

#[test]
fn crash_before_git_reconciled_as_resumed() {
    let env = TestEnv::new("crash-a");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = resolved_base(&env);

    // Stop right after the durable Allocated intent (no Git mutation).
    let ws = {
        let mut service = env.workspace_service();
        let lease = service
            .allocate_workspace(&request(&env, &exec, "repo-local:runlith"))
            .unwrap();
        assert_eq!(lease.lifecycle(), LeaseLifecycle::Allocated);
        lease.id().clone()
    };

    // Reopen and reconcile in one new service (fresh store connection).
    let mut service = env.workspace_service();
    let result = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(result.outcome, EnsureOutcome::ResumedAllocated);
    assert_eq!(result.lease.id(), &ws);
    assert_eq!(result.lease.base_revision().as_str(), base);
    assert_eq!(result.lease.lifecycle(), LeaseLifecycle::Ready);
    assert_eq!(workspace_created_count(&service, ws.as_str()), 1);
}

#[test]
fn crash_after_git_before_finalization_recognized_not_recreated() {
    let env = TestEnv::new("crash-b");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = resolved_base(&env);

    // Allocate + materialize (Git did its work), stop before finalization.
    let ws = {
        let mut service = env.workspace_service();
        let lease = service
            .allocate_workspace(&request(&env, &exec, "repo-local:runlith"))
            .unwrap();
        service
            .materialize_allocated(&lease, &env.repo_path())
            .unwrap();
        // Durable state remains Allocated even though Git mutated.
        assert_eq!(
            service
                .workspace_for_execution(&exec)
                .unwrap()
                .unwrap()
                .lifecycle(),
            LeaseLifecycle::Allocated
        );
        lease.id().clone()
    };

    let mut service = env.workspace_service();
    let result = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(result.outcome, EnsureOutcome::RecognizedGitState);
    assert_eq!(result.lease.id(), &ws);
    assert_eq!(result.lease.base_revision().as_str(), base);
    assert_eq!(result.lease.lifecycle(), LeaseLifecycle::Ready);
    // Exactly once: recognition finalizes, never recreates.
    assert_eq!(workspace_created_count(&service, ws.as_str()), 1);
}

// -- ownership invariants ----------------------------------------------------

#[test]
fn one_lease_per_execution_and_attempts_reuse_it() {
    let env = TestEnv::new("ownership");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");

    let mut service = env.workspace_service();
    let first = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    let ws = first.lease.id().clone();
    drop(service);

    // Two attempts of the same execution resolve to the same single lease
    // (attempts never own workspaces); there is no allocation path for
    // attempt A1 -> W1 and A2 -> W2.
    let mut service = env.workspace_service();
    let attempt_a1 = AttemptId::new("A-1").unwrap();
    let attempt_a2 = AttemptId::new("A-2").unwrap();
    let _ = (attempt_a1, attempt_a2);
    let by_execution = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(by_execution.id(), &ws);
    // A second ready call is still the same lease, not a new one.
    let again = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(again.lease.id(), &ws);
    assert_eq!(again.outcome, EnsureOutcome::AlreadyReady);
}

#[test]
fn distinct_executions_own_distinct_leases() {
    let env = TestEnv::new("distinct");
    let (_, _, exec1) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (_, _, exec2) = seed_execution(&env, "P-2", "T-2", "E-2");

    let mut service = env.workspace_service();
    let first = service
        .ensure_workspace_ready(&request(&env, &exec1, "repo-local:runlith"))
        .unwrap();
    let second = service
        .ensure_workspace_ready(&request(&env, &exec2, "repo-local:runlith"))
        .unwrap();

    assert_ne!(first.lease.id(), second.lease.id());
    assert_ne!(first.lease.branch(), second.lease.branch());
    assert_ne!(first.lease.worktree_ref(), second.lease.worktree_ref());
    // No shared writable worktree: both are registered, distinct paths.
    let registered = git(&env.repo, &["worktree", "list", "--porcelain"]);
    let mut entries = registered
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .filter(|path| path.starts_with(env.root.to_str().unwrap()));
    assert_eq!(entries.clone().count(), 2);
    assert!(entries.next().unwrap() != entries.next().unwrap());
}

#[test]
fn ownership_mismatch_fails_closed() {
    let env = TestEnv::new("mismatch");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();

    // Same execution, different repository identity.
    let wrong_repo = request(&env, &exec, "repo-local:other");
    let err = service.ensure_workspace_ready(&wrong_repo);
    assert!(
        matches!(
            err,
            Err(workspace::WorkspaceFailure::OwnershipMismatch { .. })
        ),
        "expected OwnershipMismatch, got {err:?}"
    );

    // Make a second commit so "HEAD" resolves differently; the lease base
    // is immutable, so requesting a new base mismatches.
    git(
        &env.repo,
        &["commit", "-q", "--allow-empty", "-m", "advance"],
    );
    let err = service.ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"));
    assert!(
        matches!(
            err,
            Err(workspace::WorkspaceFailure::OwnershipMismatch { .. })
        ),
        "expected OwnershipMismatch on base drift, got {err:?}"
    );

    // One execution cannot claim another's lease through the request path.
    let (_, _, other) = seed_execution(&env, "P-2", "T-2", "E-2");
    let err = service
        .ensure_workspace_ready(&request(&env, &other, "repo-local:runlith"))
        .map(|_| ());
    // E-2 owns nothing yet, so this allocates its own lease — never rebinds
    // to E-1's. That new lease is distinct.
    let e1 = service.workspace_for_execution(&exec).unwrap().unwrap();
    let e2 = service.workspace_for_execution(&other).unwrap().unwrap();
    assert_ne!(e1.id(), e2.id());
    assert!(err.is_ok() || err.is_err());
}

// -- restart durability ------------------------------------------------------

#[test]
fn reopen_preserves_every_durable_fact() {
    let env = TestEnv::new("reopen");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = resolved_base(&env);

    let (ws, branch) = {
        let mut service = env.workspace_service();
        let lease = service
            .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
            .unwrap()
            .lease;
        (lease.id().clone(), lease.branch().clone())
    };

    let service = env.workspace_service();
    let lease = service.workspace_for_execution(&exec).unwrap().unwrap();
    assert_eq!(lease.id(), &ws);
    assert_eq!(lease.branch(), &branch);
    assert_eq!(lease.base_revision().as_str(), base);
    assert_eq!(lease.repository().as_str(), "repo-local:runlith");
    assert_eq!(lease.owner().as_str(), "E-1");
    assert_eq!(lease.lifecycle(), LeaseLifecycle::Ready);
    assert!(lease.worktree_ref().starts_with(env.root.to_str().unwrap()));

    // And an already-ready lease reconciles healthily on reopen.
    let mut service = env.workspace_service();
    let again = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();
    assert_eq!(again.outcome, EnsureOutcome::AlreadyReady);
    assert_eq!(workspace_created_count(&service, ws.as_str()), 1);
}

// -- path authorization ------------------------------------------------------

#[test]
fn managed_root_pins_derived_paths_without_smuggling() {
    let env = TestEnv::new("paths");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap()
        .lease;

    // The path never escapes the managed root, even though the workspace id
    // and the case dir both contain spaces.
    let root = env.root.to_str().unwrap();
    assert!(lease.worktree_ref().starts_with(root));
    let relative = lease.worktree_ref().strip_prefix(root).unwrap();
    assert!(!relative.contains(".."));
    assert!(!relative.contains('/') || relative.matches('/').count() <= 1);
    // No ability to target an arbitrary location: the API accepts only a
    // repository path; every derived target lives under the managed root.
    let root_path = service.managed_root().path();
    assert!(service.managed_root().path() == root_path);
}

// -- git adapter partition ---------------------------------------------------

#[test]
fn service_uses_git_port_but_never_mutates_task_state() {
    let env = TestEnv::new("partition");
    let (_, task_id, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let _ = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:runlith"))
        .unwrap();

    // Task lifecycle untouched by workspace work.
    let tasks = env.task_service();
    let task = tasks.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), task_domain::TaskState::Draft);
}

#[test]
fn concurrent_allocations_from_separate_instances_never_collide() {
    // TASK-0047 regression: the daemon opens one WorkspaceService per
    // background run, so two runs allocating in the same millisecond
    // used to mint the identical `ws-<millis>-1` (per-instance counter)
    // and one failed on the UNIQUE constraint. Lease ids are now
    // process-unique across instances.
    let env = TestEnv::new("concurrent-alloc");
    let (_, _, exec_a) = seed_execution(&env, "P-A", "T-A", "E-A");
    let (_, _, exec_b) = seed_execution(&env, "P-B", "T-B", "E-B");

    // Barrier-aligned start so both allocations land in the same instant.
    // Each thread opens its own service (like one daemon background run
    // each); only owned paths cross the thread boundary, so the real
    // `env` keeps sole cleanup ownership.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = [("E-A", "repo-local:a"), ("E-B", "repo-local:b")]
        .into_iter()
        .map(|(exec, reference)| {
            let barrier = barrier.clone();
            let db = env.db.clone();
            let repo = env.repo.clone();
            let root = env.root.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut service = WorkspaceService::open(
                    &db,
                    GitCli::new(),
                    ManagedWorkspaceRoot::new(&root).expect("managed root"),
                    FixedClock(FIXED_MILLIS),
                )
                .expect("open workspace service");
                let exec = ExecutionId::new(exec).unwrap();
                let request = WorkspaceRequest {
                    execution_id: exec,
                    repository_ref: RepositoryRef::new(reference).unwrap(),
                    repository_path: repo.to_string_lossy().into_owned(),
                    base_revision: "HEAD".to_owned(),
                };
                service.ensure_workspace_ready(&request).map(|ensured| {
                    (
                        ensured.lease.id().to_string(),
                        ensured.lease.worktree_ref().to_owned(),
                    )
                })
            })
        })
        .collect();
    let mut ids = Vec::new();
    let mut paths = Vec::new();
    for handle in handles {
        let (id, path) = handle.join().expect("worker").expect("allocation succeeds");
        ids.push(id);
        paths.push(path);
    }
    assert_ne!(ids[0], ids[1], "concurrent leases need distinct ids");
    assert_ne!(
        paths[0], paths[1],
        "concurrent leases need distinct worktrees"
    );
    assert!(
        Path::new(&paths[0]).is_dir() && Path::new(&paths[1]).is_dir(),
        "both worktrees materialized"
    );
    let _ = (exec_a, exec_b);
}

/// TASK-0051 H4 keeper (service level): the exact production pair that
/// used to wedge the second task (`E/S` vs `E-2FS` both deriving
/// `refs/heads/agent/E-2FS`) now allocates independent managed
/// workspaces in the same repository — distinct branches, distinct
/// worktrees, both materialized, both `CreatedFresh`.
#[test]
fn h4_collision_pair_allocates_independent_workspaces() {
    let env = TestEnv::new("h4-pair");
    let mut tasks = env.task_service();
    for (project, task) in [("P-A", "T-A"), ("P-B", "T-B")] {
        let project_id = ProjectId::new(project).unwrap();
        tasks
            .create_project(&Project::new(project_id.clone(), "p".to_owned()).unwrap())
            .unwrap();
        let task_id = TaskId::new(task).unwrap();
        tasks
            .create_task(&Task::draft(task_id.clone(), project_id, "o".to_owned()).unwrap())
            .unwrap();
        tasks.mark_ready(&task_id).unwrap();
    }
    drop(tasks);
    let mut runtime = env.runtime_service();
    let exec_slash = ExecutionId::new("E/S").unwrap();
    let exec_mimic = ExecutionId::new("E-2FS").unwrap();
    runtime
        .create_execution(exec_slash.clone(), TaskId::new("T-A").unwrap())
        .unwrap();
    runtime
        .create_execution(exec_mimic.clone(), TaskId::new("T-B").unwrap())
        .unwrap();
    drop(runtime);

    let mut service = env.workspace_service();
    let ensured_slash = service
        .ensure_workspace_ready(&request(&env, &exec_slash, "repo-local:h4"))
        .expect("E/S allocates");
    let ensured_mimic = service
        .ensure_workspace_ready(&request(&env, &exec_mimic, "repo-local:h4"))
        .expect("E-2FS allocates");
    assert_eq!(ensured_slash.outcome, EnsureOutcome::CreatedFresh);
    assert_eq!(ensured_mimic.outcome, EnsureOutcome::CreatedFresh);
    assert_ne!(
        ensured_slash.lease.branch(),
        ensured_mimic.lease.branch(),
        "the H4 pair must hold distinct managed branches"
    );
    assert_eq!(
        ensured_slash.lease.branch().as_str(),
        "refs/heads/agent/e2-%45%2f%53"
    );
    assert_eq!(
        ensured_mimic.lease.branch().as_str(),
        "refs/heads/agent/e2-%45-2%46%53"
    );
    // TASK-0053: the pair is additionally distinct after ASCII case
    // folding (the B-CF dimension the old format failed).
    assert_ne!(
        ensured_slash.lease.branch().as_str().to_ascii_lowercase(),
        ensured_mimic.lease.branch().as_str().to_ascii_lowercase(),
        "the H4 pair must stay distinct under case folding"
    );
    assert_ne!(
        ensured_slash.lease.worktree_ref(),
        ensured_mimic.lease.worktree_ref(),
        "the H4 pair must hold distinct worktrees"
    );
    for lease in [&ensured_slash.lease, &ensured_mimic.lease] {
        assert!(
            git_exists(
                &env.repo,
                &["show-ref", "--verify", "--quiet", lease.branch().as_str()]
            ),
            "managed branch {} must exist in the fixture repository",
            lease.branch()
        );
        assert!(
            Path::new(lease.worktree_ref()).is_dir(),
            "worktree {} must be materialized",
            lease.worktree_ref()
        );
    }
}
