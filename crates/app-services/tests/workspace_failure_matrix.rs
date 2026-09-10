//! TASK-0044 service failure matrix: the Workspace/Git failure surface
//! exercised end to end through `WorkspaceService` over a real file-backed
//! SQLite database and a real temporary Git repository (real `git-cli`
//! backend unless a fault-injection wrapper is explicitly named).
//!
//! Complements `workspace_service.rs` (creation saga, crash A/B/C,
//! idempotency, ownership) and `workspace_diff_cleanup.rs` (diff + cleanup
//! matrices): this binary pins the remaining required G4 rows — revision
//! handling, partial-creation reconciliation, concurrent same-file
//! isolation, TOCTOU user-edit protection, live-owner refusal, external
//! branch deletion, stale semantics without wall-clock age, bounded failure
//! classification, deferred merge/rebase, journal scope and exactly-once
//! ordering, path/injection derivation, foreign-reference rejection, and
//! Task completion-authority separation. Every database file and Git
//! repository is test-owned.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, ExecutionId};
use app_services::{
    CleanupOutcome, EnsureOutcome, FixedClock, ManagedWorkspaceRoot, RuntimeService, TaskService,
    WorkspaceRequest, WorkspaceService,
};
use git_cli::GitCli;
use storage_sqlite::{EventType, SqliteStore};
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{
    BranchRef, GitPort, LeaseLifecycle, RepositoryRef, Revision, WorkspaceFailure, WorkspaceId,
    WorkspaceLease, WorktreeStatus,
};

static COUNTER: AtomicU64 = AtomicU64::new(5000);

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
            "runlith-workspace-matrix-{suite}-{id}-{}",
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

fn journal_count(service: &WorkspaceService<GitCli, FixedClock>) -> usize {
    service.store().list_events_ordered().unwrap().len()
}

fn workspace_facts<G: GitPort, C: app_services::Clock>(
    service: &WorkspaceService<G, C>,
    ws: &str,
    kind: EventType,
) -> usize {
    service
        .store()
        .list_events_ordered()
        .unwrap()
        .iter()
        .filter(|event| {
            event.fact.event_type == kind
                && event.fact.workspace_id.as_ref().map(|id| id.as_str()) == Some(ws)
        })
        .count()
}

// -- revision handling ---------------------------------------------------------

#[test]
fn base_revision_request_resolves_to_immutable_oid_never_head() {
    let env = TestEnv::new("base-oid");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let head = git(&env.repo, &["rev-parse", "HEAD"]);
    let mut service = env.workspace_service();
    let ensured = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap();
    assert_eq!(ensured.outcome, EnsureOutcome::CreatedFresh);
    assert_eq!(ensured.lease.base_revision().as_str(), head);
    assert_ne!(ensured.lease.base_revision().as_str(), "HEAD");
}

#[test]
fn invalid_and_nonexistent_revisions_allocate_nothing() {
    for (suite, rev) in [
        ("rev-garbage", "no-such-ref-anywhere"),
        ("rev-empty", ""),
        ("rev-tree", "HEAD^{tree}"),
    ] {
        let env = TestEnv::new(suite);
        let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
        let mut service = env.workspace_service();
        let before = journal_count(&service);
        let req = WorkspaceRequest {
            base_revision: rev.to_owned(),
            ..request(&env, &exec, "repo-local:x")
        };
        let err = service
            .ensure_workspace_ready(&req)
            .expect_err("bad revision must fail");
        assert!(
            matches!(err, WorkspaceFailure::InvalidRevision { .. }),
            "revision {rev:?} must be InvalidRevision, got {err:?}"
        );
        assert!(service.workspace_for_execution(&exec).unwrap().is_none());
        assert_eq!(journal_count(&service), before, "no journal leakage");
    }
}

// -- partial creation reconciliation -------------------------------------------

#[test]
fn allocated_with_branch_at_unexpected_revision_never_replays() {
    let env = TestEnv::new("partial-rev");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let first = git(&env.repo, &["rev-parse", "HEAD"]);
    let mut service = env.workspace_service();
    // Commit intent only (crash-A seam), then plant the derived branch at a
    // *different* commit behind the service's back.
    let lease = service
        .allocate_workspace(&request(&env, &exec, "repo-local:x"))
        .unwrap();
    git(&env.repo, &["commit", "-q", "--allow-empty", "-m", "drift"]);
    git(&env.repo, &["branch", lease.branch().short_name(), "HEAD"]);
    git(&env.repo, &["checkout", "-q", &first]);

    let err = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .expect_err("ambiguous partial state must fail");
    assert!(
        matches!(err, WorkspaceFailure::CheckoutFailed { .. }),
        "unexpected-revision partial must be CheckoutFailed, got {err:?}"
    );
    let stored = service.workspace(lease.id()).unwrap().unwrap();
    assert_eq!(stored.lifecycle(), LeaseLifecycle::Allocated);
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceCreated),
        0
    );
}

#[test]
fn allocated_with_foreign_target_content_never_replays() {
    let env = TestEnv::new("partial-path");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .allocate_workspace(&request(&env, &exec, "repo-local:x"))
        .unwrap();
    // Unexpected data at the derived target: not the "branch absent AND
    // target absent" crash-A shape, so no resume may run.
    std::fs::create_dir_all(lease.worktree_ref()).expect("plant foreign dir");
    std::fs::write(
        Path::new(lease.worktree_ref()).join("foreign.txt"),
        b"not ours",
    )
    .expect("plant foreign file");

    let err = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .expect_err("foreign target content must fail");
    assert!(
        matches!(
            err,
            WorkspaceFailure::CheckoutFailed { .. } | WorkspaceFailure::WorktreeCollision { .. }
        ),
        "foreign target must fail closed, got {err:?}"
    );
    assert_eq!(
        service.workspace(lease.id()).unwrap().unwrap().lifecycle(),
        LeaseLifecycle::Allocated
    );
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceCreated),
        0
    );
    // Foreign bytes preserved, never adopted or deleted.
    assert_eq!(
        std::fs::read(Path::new(lease.worktree_ref()).join("foreign.txt")).unwrap(),
        b"not ours"
    );
}

#[test]
fn materialize_failure_writes_nothing_to_durable_state() {
    // INV-145 at the seam: `materialize_allocated` borrows `&self` (no
    // store write is possible across the Git subprocess) and a failing
    // mutation leaves durable state byte-identical.
    let env = TestEnv::new("materialize-clean");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let service = env.workspace_service();
    let mut service = service;
    let lease = service
        .allocate_workspace(&request(&env, &exec, "repo-local:x"))
        .unwrap();
    let before = journal_count(&service);
    // Plant the branch so materialization collides.
    git(&env.repo, &["branch", lease.branch().short_name(), "HEAD"]);
    let err = service
        .materialize_allocated(&lease, &env.repo_path())
        .expect_err("collision must fail materialization");
    assert!(
        matches!(err, WorkspaceFailure::BranchCollision { .. }),
        "expected BranchCollision, got {err:?}"
    );
    assert_eq!(journal_count(&service), before);
    assert_eq!(
        service.workspace(lease.id()).unwrap().unwrap().lifecycle(),
        LeaseLifecycle::Allocated
    );
}

// -- duplicate calls ------------------------------------------------------------

#[test]
fn repeated_calls_emit_exactly_one_created_and_one_removed() {
    let env = TestEnv::new("exactly-once");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let first = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap();
    assert_eq!(first.outcome, EnsureOutcome::CreatedFresh);
    for _ in 0..2 {
        let again = service
            .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
            .unwrap();
        assert_eq!(again.outcome, EnsureOutcome::AlreadyReady);
        assert_eq!(again.lease.id(), first.lease.id());
    }
    // A direct second allocation for the same execution is refused, not duplicated.
    let dup = service.allocate_workspace(&request(&env, &exec, "repo-local:x"));
    assert!(
        matches!(dup, Err(WorkspaceFailure::StorageFailure { .. })),
        "duplicate lease must be StorageFailure, got {dup:?}"
    );
    let ws = first.lease.id().as_str().to_owned();
    assert_eq!(
        workspace_facts(&service, &ws, EventType::WorkspaceCreated),
        1
    );

    let cleaned = service
        .cleanup_workspace(first.lease.id(), &env.repo_path())
        .unwrap();
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    let again = service
        .cleanup_workspace(first.lease.id(), &env.repo_path())
        .unwrap();
    assert_eq!(again.outcome, CleanupOutcome::AlreadyRemoved);
    assert_eq!(
        workspace_facts(&service, &ws, EventType::WorkspaceCreated),
        1
    );
    assert_eq!(
        workspace_facts(&service, &ws, EventType::WorkspaceRemoved),
        1
    );
}

// -- concurrent workspaces -------------------------------------------------------

#[test]
fn concurrent_workspaces_isolate_same_file_edits() {
    let env = TestEnv::new("same-file");
    let (_, _, e1) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (_, _, e2) = seed_execution(&env, "P-2", "T-2", "E-2");
    let mut service = env.workspace_service();
    let w1 = service
        .ensure_workspace_ready(&request(&env, &e1, "repo-local:x"))
        .unwrap()
        .lease;
    let w2 = service
        .ensure_workspace_ready(&request(&env, &e2, "repo-local:x"))
        .unwrap()
        .lease;
    assert_ne!(w1.id(), w2.id());
    assert_ne!(w1.worktree_ref(), w2.worktree_ref());
    assert_ne!(w1.branch(), w2.branch());

    // Same logical file, independent bytes: no global lock, no bleed.
    std::fs::write(Path::new(w1.worktree_ref()).join("shared.txt"), b"from W1").unwrap();
    std::fs::write(Path::new(w2.worktree_ref()).join("shared.txt"), b"from W2").unwrap();
    assert_eq!(
        std::fs::read(Path::new(w1.worktree_ref()).join("shared.txt")).unwrap(),
        b"from W1"
    );
    assert_eq!(
        std::fs::read(Path::new(w2.worktree_ref()).join("shared.txt")).unwrap(),
        b"from W2"
    );
    assert!(
        !env.repo.join("shared.txt").exists(),
        "primary checkout must not see either edit"
    );

    // Diffs stay scoped: W1 sees only its own file and vice versa.
    let d1 = service.compute_diff(w1.id()).unwrap();
    let d2 = service.compute_diff(w2.id()).unwrap();
    assert!(d1.diff.working.iter().any(|e| e.path == "shared.txt"));
    assert!(d2.diff.working.iter().any(|e| e.path == "shared.txt"));
    assert_eq!(d1.diff.base_revision, d2.diff.base_revision);
    assert_eq!(d1.diff.base_revision, *w1.base_revision());
}

#[test]
fn cleanup_of_one_lease_preserves_dirty_sibling_byte_identical() {
    let env = TestEnv::new("sibling-dirty");
    let (_, _, e1) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (_, _, e2) = seed_execution(&env, "P-2", "T-2", "E-2");
    let mut service = env.workspace_service();
    let w1 = service
        .ensure_workspace_ready(&request(&env, &e1, "repo-local:x"))
        .unwrap()
        .lease;
    let w2 = service
        .ensure_workspace_ready(&request(&env, &e2, "repo-local:x"))
        .unwrap()
        .lease;

    // Sibling holds tracked dirt plus an untracked file.
    git(
        Path::new(w2.worktree_ref()),
        &["commit", "-q", "--allow-empty", "-m", "seed"],
    );
    std::fs::write(Path::new(w2.worktree_ref()).join("seed.txt"), b"v1").unwrap();
    git(Path::new(w2.worktree_ref()), &["add", "seed.txt"]);
    git(
        Path::new(w2.worktree_ref()),
        &["commit", "-q", "-m", "seed"],
    );
    std::fs::write(
        Path::new(w2.worktree_ref()).join("seed.txt"),
        b"sibling-dirty",
    )
    .unwrap();
    std::fs::write(
        Path::new(w2.worktree_ref()).join("untracked.txt"),
        b"sibling-untracked",
    )
    .unwrap();
    let tracked_before = std::fs::read(Path::new(w2.worktree_ref()).join("seed.txt")).unwrap();
    let untracked_before =
        std::fs::read(Path::new(w2.worktree_ref()).join("untracked.txt")).unwrap();
    let branch_before = git(&env.repo, &["rev-parse", w2.branch().as_str()]);

    service
        .cleanup_workspace(w1.id(), &env.repo_path())
        .unwrap();

    assert_eq!(
        std::fs::read(Path::new(w2.worktree_ref()).join("seed.txt")).unwrap(),
        tracked_before
    );
    assert_eq!(
        std::fs::read(Path::new(w2.worktree_ref()).join("untracked.txt")).unwrap(),
        untracked_before
    );
    assert_eq!(
        git(&env.repo, &["rev-parse", w2.branch().as_str()]),
        branch_before
    );
    assert_eq!(
        GitCli::new().worktree_status(w2.worktree_ref()).unwrap(),
        WorktreeStatus::DirtyUntracked
    );
}

// -- diff -------------------------------------------------------------------------

#[test]
fn diff_mixed_state_scoped_to_base_after_primary_advances() {
    let env = TestEnv::new("diff-mixed");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    let base = lease.base_revision().as_str().to_owned();

    // Advance the primary: the lease baseline must not follow it.
    git(
        &env.repo,
        &["commit", "-q", "--allow-empty", "-m", "primary moves on"],
    );
    // Mixed workspace state: one committed file plus working dirt.
    let wt = Path::new(lease.worktree_ref()).to_path_buf();
    std::fs::write(wt.join("committed.txt"), b"committed work").unwrap();
    git(&wt, &["add", "committed.txt"]);
    git(&wt, &["commit", "-q", "-m", "workspace commit"]);
    std::fs::write(wt.join("work.txt"), b"working dirt").unwrap();

    let evidence = service.compute_diff(lease.id()).unwrap();
    assert_eq!(evidence.diff.base_revision.as_str(), base);
    assert!(
        !evidence.diff.committed.is_empty(),
        "committed layer visible"
    );
    assert!(!evidence.diff.working.is_empty(), "working layer visible");
    assert!(evidence.diff.has_working_tree_changes);
    // Primary-only movement never leaks into the lease diff baseline.
    assert_ne!(git(&env.repo, &["rev-parse", "HEAD"]), base);
}

// -- user edit during cleanup (TOCTOU seam) ----------------------------------------

/// Git backend that reports `Clean` once, then `DirtyTracked` forever after:
/// deterministically models a user edit landing between the pre-`CleanupPending`
/// check and the pre-destruction re-check.
struct DirtAfterFirstCheck {
    inner: GitCli,
    calls: std::sync::atomic::AtomicUsize,
}

impl DirtAfterFirstCheck {
    fn wrap() -> Self {
        Self {
            inner: GitCli::new(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl GitPort for DirtAfterFirstCheck {
    fn backend_ref(&self) -> &'static str {
        self.inner.backend_ref()
    }
    fn is_repository(&self, path: &str) -> Result<bool, WorkspaceFailure> {
        self.inner.is_repository(path)
    }
    fn default_branch(&self, repository_path: &str) -> Result<String, WorkspaceFailure> {
        self.inner.default_branch(repository_path)
    }
    fn resolve_revision(
        &self,
        repository_path: &str,
        rev: &str,
    ) -> Result<Revision, WorkspaceFailure> {
        self.inner.resolve_revision(repository_path, rev)
    }
    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        self.inner
            .create_worktree(lease, repository_path, worktree_path)
    }
    fn worktree_status(&self, worktree_path: &str) -> Result<WorktreeStatus, WorkspaceFailure> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        // Calls 0 (finalize verification) and 1 (cleanup entry check)
        // observe a clean tree — the entry check's view is stale by
        // construction. Every later observation reports the true live
        // state, so the pre-destruction re-check sees the simulated user
        // edit. The test plants a real untracked file, so live state is
        // genuinely dirty from call 2 on.
        if n <= 1 {
            Ok(WorktreeStatus::Clean)
        } else {
            self.inner.worktree_status(worktree_path)
        }
    }
    fn compute_diff(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<workspace::DiffEvidence, WorkspaceFailure> {
        self.inner.compute_diff(lease)
    }
    fn remove_worktree(
        &self,
        lease: &WorkspaceLease,
        worktree_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        self.inner.remove_worktree(lease, worktree_path)
    }
    fn remove_branch(
        &self,
        repository_path: &str,
        branch: &BranchRef,
    ) -> Result<(), WorkspaceFailure> {
        self.inner.remove_branch(repository_path, branch)
    }
    fn branch_exists(
        &self,
        repository_path: &str,
        branch: &BranchRef,
    ) -> Result<bool, WorkspaceFailure> {
        self.inner.branch_exists(repository_path, branch)
    }
    fn is_registered_worktree(
        &self,
        repository_path: &str,
        worktree_path: &str,
    ) -> Result<bool, WorkspaceFailure> {
        self.inner
            .is_registered_worktree(repository_path, worktree_path)
    }
    fn owner_is_live(
        &self,
        repository: &RepositoryRef,
        owner: &ExecutionId,
    ) -> Result<bool, WorkspaceFailure> {
        self.inner.owner_is_live(repository, owner)
    }
}

#[test]
fn user_edit_between_check_and_removal_is_preserved() {
    let env = TestEnv::new("toctou");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = WorkspaceService::open(
        &env.db,
        DirtAfterFirstCheck::wrap(),
        ManagedWorkspaceRoot::new(&env.root).expect("managed root"),
        FixedClock(FIXED_MILLIS),
    )
    .expect("open workspace service");
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    // Real data at stake: the worktree actually holds a file.
    std::fs::write(
        Path::new(lease.worktree_ref()).join("precious.txt"),
        b"precious",
    )
    .unwrap();

    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("late dirt must stop cleanup");
    assert!(
        matches!(err, WorkspaceFailure::WorktreeDirty { .. }),
        "TOCTOU dirt must be WorktreeDirty, got {err:?}"
    );
    // Still CleanupPending (never re-armed, never Removed), data intact,
    // no removed event.
    assert_eq!(
        service.workspace(lease.id()).unwrap().unwrap().lifecycle(),
        LeaseLifecycle::CleanupPending
    );
    assert_eq!(
        std::fs::read(Path::new(lease.worktree_ref()).join("precious.txt")).unwrap(),
        b"precious"
    );
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        0
    );
}

/// Backend reporting a live owner: cleanup must refuse before any mutation.
struct LiveOwner(GitCli);

impl GitPort for LiveOwner {
    fn backend_ref(&self) -> &'static str {
        self.0.backend_ref()
    }
    fn is_repository(&self, path: &str) -> Result<bool, WorkspaceFailure> {
        self.0.is_repository(path)
    }
    fn default_branch(&self, repository_path: &str) -> Result<String, WorkspaceFailure> {
        self.0.default_branch(repository_path)
    }
    fn resolve_revision(
        &self,
        repository_path: &str,
        rev: &str,
    ) -> Result<Revision, WorkspaceFailure> {
        self.0.resolve_revision(repository_path, rev)
    }
    fn create_worktree(
        &self,
        lease: &WorkspaceLease,
        repository_path: &str,
        worktree_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        self.0
            .create_worktree(lease, repository_path, worktree_path)
    }
    fn worktree_status(&self, worktree_path: &str) -> Result<WorktreeStatus, WorkspaceFailure> {
        self.0.worktree_status(worktree_path)
    }
    fn compute_diff(
        &self,
        lease: &WorkspaceLease,
    ) -> Result<workspace::DiffEvidence, WorkspaceFailure> {
        self.0.compute_diff(lease)
    }
    fn remove_worktree(
        &self,
        lease: &WorkspaceLease,
        worktree_path: &str,
    ) -> Result<(), WorkspaceFailure> {
        self.0.remove_worktree(lease, worktree_path)
    }
    fn remove_branch(
        &self,
        repository_path: &str,
        branch: &BranchRef,
    ) -> Result<(), WorkspaceFailure> {
        self.0.remove_branch(repository_path, branch)
    }
    fn branch_exists(
        &self,
        repository_path: &str,
        branch: &BranchRef,
    ) -> Result<bool, WorkspaceFailure> {
        self.0.branch_exists(repository_path, branch)
    }
    fn is_registered_worktree(
        &self,
        repository_path: &str,
        worktree_path: &str,
    ) -> Result<bool, WorkspaceFailure> {
        self.0
            .is_registered_worktree(repository_path, worktree_path)
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
fn live_owner_rejects_cleanup_before_any_mutation() {
    let env = TestEnv::new("live-owner");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    // Create with the honest backend first (LiveOwner would still create
    // fine, but creation-then-swap keeps the test focused on cleanup).
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    drop(service);
    let mut service = WorkspaceService::open(
        &env.db,
        LiveOwner(GitCli::new()),
        ManagedWorkspaceRoot::new(&env.root).expect("managed root"),
        FixedClock(FIXED_MILLIS),
    )
    .expect("reopen with live-owner backend");

    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("live owner must refuse cleanup");
    assert!(
        matches!(err, WorkspaceFailure::ActiveOwnerRejectsCleanup { .. }),
        "expected ActiveOwnerRejectsCleanup, got {err:?}"
    );
    assert_eq!(
        service.workspace(lease.id()).unwrap().unwrap().lifecycle(),
        LeaseLifecycle::Ready,
        "refused cleanup writes nothing"
    );
    assert!(GitCli::new()
        .branch_exists(&env.repo_path(), lease.branch())
        .unwrap());
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        0
    );
}

// -- external drift -----------------------------------------------------------------

#[test]
fn externally_deleted_branch_with_clean_base_worktree_finalizes_safely() {
    // The managed branch is gone but the worktree is clean at the recorded
    // base: nothing unique can be lost (HEAD == base, which stays reachable
    // from the repository), so the non-forced saga reaches the
    // contract-defined final state. This is legitimate cleanup progression,
    // not drift-masquerading-as-absence: every mutation was still observed
    // and non-destructive.
    let env = TestEnv::new("deleted-branch");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    let base = lease.base_revision().as_str().to_owned();
    git(&env.repo, &["update-ref", "-d", lease.branch().as_str()]);
    assert!(
        !GitCli::new()
            .branch_exists(&env.repo_path(), lease.branch())
            .unwrap(),
        "precondition: managed branch is gone"
    );

    let cleaned = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect("clean-at-base with deleted branch must finalize");
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        1
    );
    // No data loss possible: the base commit is still reachable.
    assert!(git(&env.repo, &["cat-file", "-t", &base]) == "commit");
}

#[test]
fn externally_deleted_branch_with_unique_commits_fails_closed() {
    // Same external deletion, but the worktree holds a unique commit: Git's
    // own non-forced `worktree remove` refuses the broken-HEAD state, so
    // the service fails closed with the work and the lease preserved.
    let env = TestEnv::new("deleted-branch-unique");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    let wt = Path::new(lease.worktree_ref()).to_path_buf();
    std::fs::write(wt.join("unique.txt"), b"unique work").unwrap();
    git(&wt, &["add", "unique.txt"]);
    git(&wt, &["commit", "-q", "-m", "unique"]);
    git(&env.repo, &["update-ref", "-d", lease.branch().as_str()]);

    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("unique commits with deleted branch must fail closed");
    assert!(
        !matches!(err, WorkspaceFailure::AlreadyAbsent),
        "drift must not masquerade as idempotent absence, got {err:?}"
    );
    let lifecycle = service.workspace(lease.id()).unwrap().unwrap().lifecycle();
    assert!(
        lifecycle == LeaseLifecycle::Ready || lifecycle == LeaseLifecycle::CleanupPending,
        "lease must stay non-terminal, got {lifecycle:?}"
    );
    assert_eq!(
        std::fs::read(wt.join("unique.txt")).unwrap(),
        b"unique work",
        "unique work preserved"
    );
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        0,
        "no removed event without observed final state"
    );
}

#[test]
fn restart_with_far_future_clock_never_auto_deletes() {
    // Stale is semantic (durable lease + Execution lifecycle + Git state),
    // never wall-clock age: reopening decades later deletes nothing.
    let env = TestEnv::new("stale-clock");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    drop(service);
    let future = FixedClock(FIXED_MILLIS + 10 * 365 * 24 * 60 * 60 * 1000);
    let mut service = WorkspaceService::open(
        &env.db,
        GitCli::new(),
        ManagedWorkspaceRoot::new(&env.root).expect("managed root"),
        future,
    )
    .expect("reopen in the far future");
    let again = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap();
    assert_eq!(again.outcome, EnsureOutcome::AlreadyReady);
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceCreated),
        1
    );
    assert!(Path::new(lease.worktree_ref()).is_dir());
}

// -- failure classification -----------------------------------------------------------

#[test]
fn failure_classification_stays_bounded_and_domain_level() {
    let env = TestEnv::new("bounded");
    // Missing backend surfaces as typed GitBackendFailure through the port.
    let port_err = GitCli::with_executable("/nonexistent/git-binary-xyz")
        .is_repository(&env.repo_path())
        .unwrap_err();
    assert_eq!(port_err.kind(), "git_backend_failure");

    // The taxonomy is closed: every kind is a known domain class.
    let kinds = [
        WorkspaceFailure::NotRepository { detail: "x".into() }.kind(),
        WorkspaceFailure::BareRepositoryUnsupported { detail: "x".into() }.kind(),
        WorkspaceFailure::DirtyPrimaryWorkspace { detail: "x".into() }.kind(),
        WorkspaceFailure::BranchCollision { detail: "x".into() }.kind(),
        WorkspaceFailure::WorktreeCollision { detail: "x".into() }.kind(),
        WorkspaceFailure::WorktreeDirty { detail: "x".into() }.kind(),
        WorkspaceFailure::WorktreeMissing { detail: "x".into() }.kind(),
        WorkspaceFailure::CheckoutFailed { detail: "x".into() }.kind(),
        WorkspaceFailure::OwnershipMismatch { detail: "x".into() }.kind(),
        WorkspaceFailure::InvalidRevision { detail: "x".into() }.kind(),
        WorkspaceFailure::CleanupUnsafe { detail: "x".into() }.kind(),
        WorkspaceFailure::ActiveOwnerRejectsCleanup { detail: "x".into() }.kind(),
        WorkspaceFailure::ActiveOwnerRejectsCheckout { detail: "x".into() }.kind(),
        WorkspaceFailure::GitBackendFailure { detail: "x".into() }.kind(),
        WorkspaceFailure::StorageFailure { detail: "x".into() }.kind(),
        WorkspaceFailure::AlreadyAbsent.kind(),
    ];
    assert_eq!(
        kinds.len(),
        16,
        "taxonomy grows only by ADR, never by stderr"
    );
}

// -- deferred merge/rebase ---------------------------------------------------------------

#[test]
fn conflicting_branches_coexist_without_merge_or_rebase() {
    let env = TestEnv::new("no-merge");
    let (_, _, e1) = seed_execution(&env, "P-1", "T-1", "E-1");
    let (_, _, e2) = seed_execution(&env, "P-2", "T-2", "E-2");
    let mut service = env.workspace_service();
    let w1 = service
        .ensure_workspace_ready(&request(&env, &e1, "repo-local:x"))
        .unwrap()
        .lease;
    let w2 = service
        .ensure_workspace_ready(&request(&env, &e2, "repo-local:x"))
        .unwrap()
        .lease;
    // Conflicting content on the two managed branches.
    for (lease, bytes) in [(&w1, b"side A"), (&w2, b"side B")] {
        let wt = Path::new(lease.worktree_ref()).to_path_buf();
        std::fs::write(wt.join("conflict.txt"), bytes).unwrap();
        git(&wt, &["add", "conflict.txt"]);
        git(&wt, &["commit", "-q", "-m", "conflicting work"]);
    }
    // Full lifecycle on W1 must not integrate, merge, or rebase anything.
    service
        .cleanup_workspace(w1.id(), &env.repo_path())
        .expect_err("W1 now holds unique unmerged commits and must be preserved, not integrated");
    assert!(
        !env.repo.join("MERGE_HEAD").exists() && !env.repo.join("REBASE_HEAD").exists(),
        "no merge/rebase state may exist in the primary checkout"
    );
    assert!(
        !Path::new(w2.worktree_ref()).join("MERGE_HEAD").exists(),
        "no merge/rebase state may exist in any worktree"
    );
    assert_eq!(
        std::fs::read(Path::new(w2.worktree_ref()).join("conflict.txt")).unwrap(),
        b"side B",
        "W2 content untouched by W1 lifecycle"
    );
}

// -- event journal scope -------------------------------------------------------------------

#[test]
fn workspace_events_carry_exact_scope_and_ordering() {
    let env = TestEnv::new("journal-scope");
    let (project_id, task_id, exec) = seed_execution(&env, "P-9", "T-9", "E-9");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    // Make cleanup eligible: nothing dirty, no unique commits (empty
    // commits are still unique — use a fresh lease and remove directly).
    std::fs::write(Path::new(lease.worktree_ref()).join("tmp.txt"), b"x").unwrap();
    git(Path::new(lease.worktree_ref()), &["add", "tmp.txt"]);
    // Committing inside the worktree advances its HEAD past base, which
    // makes the branch unmerged; instead drop the file and clean up.
    git(Path::new(lease.worktree_ref()), &["reset", "-q", "HEAD"]);
    std::fs::remove_file(Path::new(lease.worktree_ref()).join("tmp.txt")).unwrap();
    service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .unwrap();

    let facts: Vec<_> = service
        .store()
        .list_events_ordered()
        .unwrap()
        .into_iter()
        .filter(|event| {
            event.fact.workspace_id.as_ref().map(|id| id.as_str()) == Some(lease.id().as_str())
        })
        .collect();
    assert_eq!(facts.len(), 2, "exactly created + removed, got {facts:?}");
    assert_eq!(facts[0].fact.event_type, EventType::WorkspaceCreated);
    assert_eq!(facts[1].fact.event_type, EventType::WorkspaceRemoved);
    assert_eq!(facts[0].event_version, 1);
    assert_eq!(facts[1].event_version, 1);
    for fact in &facts {
        assert_eq!(fact.fact.project_id, project_id);
        assert_eq!(fact.fact.task_id.as_ref(), Some(&task_id));
        assert_eq!(fact.fact.execution_id.as_ref(), Some(&exec));
        assert!(
            fact.fact.attempt_id.is_none(),
            "workspace facts carry no attempt scope"
        );
        assert!(
            fact.fact.attempt_outcome.is_none(),
            "workspace facts carry no attempt outcome"
        );
        assert!(fact.fact.from_state.is_none() && fact.fact.to_state.is_none());
    }
    assert_eq!(
        facts[0].fact.from_workspace_state,
        Some(LeaseLifecycle::Allocated)
    );
    assert_eq!(
        facts[0].fact.to_workspace_state,
        Some(LeaseLifecycle::Ready)
    );
    assert_eq!(
        facts[1].fact.from_workspace_state,
        Some(LeaseLifecycle::CleanupPending)
    );
    assert_eq!(
        facts[1].fact.to_workspace_state,
        Some(LeaseLifecycle::Removed)
    );
}

// -- path / injection -------------------------------------------------------------------------

#[test]
fn hostile_execution_id_derives_safe_branch_and_path() {
    let env = TestEnv::new("hostile-id");
    let hostile = "E-../../../tmp/evil-;-$(x)-`y`-\"q\"";
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", hostile);
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    // The derived branch carries no traversal: every hostile byte is encoded.
    assert!(lease.branch().as_str().starts_with("refs/heads/agent/"));
    let short = lease.branch().short_name();
    assert_eq!(
        short.matches('/').count(),
        1,
        "only the agent/ separator, got {short:?}"
    );
    assert!(!short.contains(".."), "no traversal, got {short:?}");
    // The derived worktree stays inside the managed root.
    let canon_root = std::fs::canonicalize(&env.root).unwrap();
    let canon_target = std::fs::canonicalize(lease.worktree_ref()).unwrap();
    assert!(canon_target.strip_prefix(&canon_root).is_ok());
}

#[test]
fn symlinked_managed_root_still_contains_worktrees() {
    let env = TestEnv::new("root-symlink");
    let link = env.dir.join("root-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&env.root, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&env.root, &link).unwrap();
    // The root canonicalizes through the symlink; containment still holds.
    let root = ManagedWorkspaceRoot::new(&link).expect("symlinked root");
    assert_eq!(
        root.path(),
        std::fs::canonicalize(&env.root).unwrap().as_path()
    );
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service =
        WorkspaceService::open(&env.db, GitCli::new(), root, FixedClock(FIXED_MILLIS))
            .expect("open with symlinked root");
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    assert!(Path::new(lease.worktree_ref()).is_dir());
}

#[test]
fn foreign_worktree_reference_is_rejected_never_used() {
    let env = TestEnv::new("foreign-ref");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let base = git(&env.repo, &["rev-parse", "HEAD"]);
    // Plant a well-formed lease whose worktree reference lives outside the
    // managed root, bypassing the service (the service can never mint this).
    let foreign = WorkspaceLease::allocate(
        WorkspaceId::new("ws-foreign").unwrap(),
        RepositoryRef::new("repo-local:x").unwrap(),
        exec.clone(),
        Revision::new(base).unwrap(),
        BranchRef::new(format!("refs/heads/agent/{}", exec.as_str())).unwrap(),
        "/tmp/definitely-outside-managed-root-ws-foreign",
    )
    .unwrap();
    {
        let mut store = SqliteStore::open(&env.db).expect("open store");
        use workspace::WorkspaceStore as _;
        store.insert_allocated_lease(&foreign).unwrap();
    }
    let service = env.workspace_service();
    let err = service
        .compute_diff(foreign.id())
        .expect_err("foreign ref must fail");
    assert!(
        matches!(err, WorkspaceFailure::OwnershipMismatch { .. }),
        "foreign worktree ref must be OwnershipMismatch, got {err:?}"
    );
    assert!(!Path::new("/tmp/definitely-outside-managed-root-ws-foreign").exists());
}

// -- default-branch metadata missing ---------------------------------------------------------------

#[test]
fn indeterminate_default_branch_creates_without_guessing() {
    let env = TestEnv::new("no-default-meta");
    git(
        &env.repo,
        &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"],
    );
    assert_eq!(
        GitCli::new()
            .default_branch(&env.repo_path())
            .unwrap_err()
            .kind(),
        "git_backend_failure"
    );
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    // Indeterminate metadata blocks nothing: the managed branch is proven
    // `agent/`-scoped, and guessing a default name would be the real risk.
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    assert!(lease.branch().as_str().starts_with("refs/heads/agent/"));
}

// -- completion authority -------------------------------------------------------------------------------

#[test]
fn workspace_lifecycle_never_completes_tasks() {
    use task_domain::TaskState;
    let env = TestEnv::new("no-completion");
    let (_, task_id, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let attempt = AttemptId::new("A-1").unwrap();
    env.runtime_service()
        .create_attempt(
            &exec,
            attempt,
            agent_runtime::ProviderRef::new("fake").unwrap(),
        )
        .unwrap();
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    let state = || {
        env.task_service()
            .get_task(&task_id)
            .unwrap()
            .unwrap()
            .state()
    };
    assert_eq!(state(), TaskState::Draft);
    let _ = service.compute_diff(lease.id()).unwrap();
    assert_eq!(
        state(),
        TaskState::Draft,
        "diff is evidence, never completion"
    );
    service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .unwrap();
    assert_eq!(
        state(),
        TaskState::Draft,
        "removed is teardown, never completion"
    );
    // Attempts reuse the lease; the lease never concludes attempts either.
    let stored = service.workspace(lease.id()).unwrap().unwrap();
    assert_eq!(stored.lifecycle(), LeaseLifecycle::Removed);
    assert_eq!(stored.owner(), &exec);
}

// -- lease-to-repository binding (TASK-0046, G4 finding F1) -------------------

#[test]
fn cleanup_with_wrong_but_valid_repository_fails_closed_before_mutation() {
    let env = TestEnv::new("wrong-repo");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    // A second, valid, unrelated repository: the exact F1 shape.
    let wrong = env.dir.join("unrelated repo");
    std::fs::create_dir_all(&wrong).expect("wrong repo dir");
    git(&wrong, &["init", "-q"]);
    git(
        &wrong,
        &["commit", "-q", "--allow-empty", "-m", "unrelated"],
    );
    let wrong_path = wrong.to_string_lossy().into_owned();

    let err = service
        .cleanup_workspace(lease.id(), &wrong_path)
        .expect_err("wrong-repo cleanup must fail closed");
    assert!(
        matches!(err, WorkspaceFailure::OwnershipMismatch { .. }),
        "wrong-repo cleanup must be OwnershipMismatch, got {err:?}"
    );
    // Nothing mutated: lease still Ready, worktree intact and still
    // registered in the true repository, managed branch alive there, no
    // removed event.
    let stored = service.workspace(lease.id()).unwrap().unwrap();
    assert_eq!(stored.lifecycle(), LeaseLifecycle::Ready);
    assert!(
        Path::new(lease.worktree_ref()).symlink_metadata().is_ok(),
        "worktree must survive wrong-repo cleanup"
    );
    assert!(
        GitCli::new()
            .is_registered_worktree(&env.repo_path(), lease.worktree_ref())
            .unwrap(),
        "worktree still registered in the true repository"
    );
    assert!(
        GitCli::new()
            .branch_exists(&env.repo_path(), lease.branch())
            .unwrap(),
        "managed branch must not leak-delete nor leak-away"
    );
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        0
    );
    // Retry with the correct repository succeeds cleanly — the failed
    // attempt left no corruption behind.
    let cleaned = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect("correct-repo retry must succeed");
    assert_eq!(cleaned.outcome, CleanupOutcome::RemovedFresh);
}

#[test]
fn cleanup_after_external_worktree_loss_with_wrong_repo_fails_closed() {
    // G4 F1, absent-worktree shape: the lease is CleanupPending and its
    // worktree was deleted outside the service (crash-continuation shape),
    // but the caller passes a wrong-but-valid repository. History binding
    // (recorded base commit unknown there) must fail closed instead of
    // finalizing RemovedFresh while the managed branch leaks elsewhere.
    let env = TestEnv::new("wrong-repo-absent");
    let (_, _, exec) = seed_execution(&env, "P-1", "T-1", "E-1");
    let mut service = env.workspace_service();
    let lease = service
        .ensure_workspace_ready(&request(&env, &exec, "repo-local:x"))
        .unwrap()
        .lease;
    service.request_cleanup(lease.id()).unwrap();
    // External loss: the worktree directory vanishes behind our back.
    std::fs::remove_dir_all(lease.worktree_ref()).expect("external worktree loss");

    let wrong = env.dir.join("unrelated repo absent");
    std::fs::create_dir_all(&wrong).expect("wrong repo dir");
    git(&wrong, &["init", "-q"]);
    git(
        &wrong,
        &["commit", "-q", "--allow-empty", "-m", "unrelated"],
    );
    let wrong_path = wrong.to_string_lossy().into_owned();

    let err = service
        .cleanup_workspace(lease.id(), &wrong_path)
        .expect_err("wrong-repo continuation must fail closed");
    assert!(
        matches!(err, WorkspaceFailure::OwnershipMismatch { .. }),
        "absent-shape wrong repo must be OwnershipMismatch, got {err:?}"
    );
    let stored = service.workspace(lease.id()).unwrap().unwrap();
    assert_eq!(stored.lifecycle(), LeaseLifecycle::CleanupPending);
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        0
    );
    // Same shape against the true repository refuses conservatively:
    // Git still holds a stale registration for the deleted path, so
    // branch deletion is refused (never forced) and the lease stays
    // CleanupPending with its branch preserved — recovery needs explicit
    // repair, never silent finalization.
    let err = service
        .cleanup_workspace(lease.id(), &env.repo_path())
        .expect_err("stale registration must refuse");
    assert!(
        matches!(err, WorkspaceFailure::CleanupUnsafe { .. }),
        "stale registration must be CleanupUnsafe, got {err:?}"
    );
    let stored = service.workspace(lease.id()).unwrap().unwrap();
    assert_eq!(stored.lifecycle(), LeaseLifecycle::CleanupPending);
    assert!(
        GitCli::new()
            .branch_exists(&env.repo_path(), lease.branch())
            .unwrap(),
        "managed branch preserved through refused continuation"
    );
    assert_eq!(
        workspace_facts(&service, lease.id().as_str(), EventType::WorkspaceRemoved),
        0
    );
}
