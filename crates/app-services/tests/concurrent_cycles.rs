//! Concurrent orchestration cycles (TASK-0047, G5 production posture).
//!
//! The daemon runs one `OrchestrationService` + `WorkspaceService` per
//! background execution, plus concurrent status readers, all on separate
//! SQLite connections over one file. Two regressions live here:
//!
//! 1. Lease-id collision: per-instance counters minted the identical id
//!    for same-millisecond allocations (fixed with a process-wide
//!    counter; see `workspace_service.rs`).
//! 2. DEFERRED upgrade deadlock: concurrent writers with DEFERRED
//!    transactions deadlocked on lock upgrade (both hold RESERVED, both
//!    want EXCLUSIVE) and SQLite returned BUSY immediately to break the
//!    tie, bypassing the busy timeout. All write transactions now use
//!    `TransactionBehavior::Immediate`, serializing contenders at BEGIN
//!    where the busy wait applies (see `task_store.rs`).
//!
//! Both tests fail fast without their fixes (wrong ids / `database is
//! locked` within a few rounds) and pass with them. Unix-only (fixture
//! Git repository setup matches the other workspace suites).

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use app_services::{
    FixedClock, ManagedWorkspaceRoot, OrchestrationIds, OrchestrationService, WorkspaceRequest,
    WorkspaceService,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use git_cli::GitCli;
use scheduler::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);
const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct StubExecutor;
impl ExecutionPort for StubExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("concurrency-stub").unwrap()
    }
    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        Ok(WorkEvidence {
            summary: format!("evidence {}", work.attempt_id),
            evidence_ref: format!("stub:{}", work.attempt_id),
            partial: false,
        })
    }
}

struct Env {
    dir: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    root: PathBuf,
}

impl Env {
    fn named(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-concurrent-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        let db = dir.join("test.db");
        let repo = dir.join("repo");
        let root = dir.join("roots");
        std::fs::create_dir_all(&root).expect("managed root");
        let root = std::fs::canonicalize(&root).expect("canonical root");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        Self {
            dir,
            db,
            repo,
            root,
        }
    }

    fn seed_pair(&self, round: u64) {
        let mut tasks =
            app_services::TaskService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("open");
        for (project, task) in [("P-A", "T-A"), ("P-B", "T-B")] {
            let project_id = ProjectId::new(format!("{project}-{round}")).unwrap();
            let task_id = TaskId::new(format!("{task}-{round}")).unwrap();
            tasks
                .create_project(&Project::new(project_id.clone(), "p".to_owned()).unwrap())
                .unwrap();
            let task = Task::draft(task_id.clone(), project_id, "objective".to_owned()).unwrap();
            tasks.create_task(&task).unwrap();
            tasks.mark_ready(&task_id).unwrap();
        }
    }

    /// Opens fresh services and runs one full lease-bound cycle for
    /// `tag` ("A"/"B") in `round`, exactly like one daemon background
    /// run does. Takes owned paths so worker threads never touch `Env`
    /// ownership (the real `Env` keeps sole cleanup ownership).
    fn run_cycle(
        db: PathBuf,
        repo: PathBuf,
        root: PathBuf,
        tag: &str,
        round: u64,
    ) -> Result<(), String> {
        let mut orchestration =
            OrchestrationService::open(&db, FixedClock(FIXED_MILLIS)).expect("open");
        let mut workspace = WorkspaceService::open(
            &db,
            GitCli::new(),
            ManagedWorkspaceRoot::new(&root).expect("managed root"),
            FixedClock(FIXED_MILLIS),
        )
        .expect("open workspace service");
        let task_id = TaskId::new(format!("T-{tag}-{round}")).unwrap();
        let execution_id = ExecutionId::new(format!("E-{tag}-{round}")).unwrap();
        let request = WorkspaceRequest {
            execution_id: execution_id.clone(),
            repository_ref: workspace::RepositoryRef::new("repo-local:x").unwrap(),
            repository_path: repo.to_string_lossy().into_owned(),
            base_revision: "HEAD".to_owned(),
        };
        let ids = OrchestrationIds {
            execution_id,
            attempt_id: AttemptId::new(format!("A-{tag}-{round}")).unwrap(),
        };
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        orchestration
            .schedule_in_workspace(
                &mut workspace,
                &task_id,
                ids,
                request,
                |_, _| StubExecutor,
                &evaluator,
                None,
                None,
            )
            .map(|_| ())
            .map_err(|error| format!("{error:?}"))
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(repo: &std::path::Path, args: &[&str]) {
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
    assert!(output.status.success(), "git {args:?} failed");
}

fn burst(env: &Env, rounds: std::ops::Range<u64>, context: &str) {
    for round in rounds {
        env.seed_pair(round);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut handles = Vec::new();
        for tag in ["A", "B"] {
            let barrier = barrier.clone();
            let db = env.db.clone();
            let repo = env.repo.clone();
            let root = env.root.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                Env::run_cycle(db, repo, root, tag, round)
            }));
        }
        for handle in handles {
            let result = handle.join().expect("worker thread");
            assert!(result.is_ok(), "{context} round {round}: {result:?}");
        }
    }
}

#[test]
fn concurrent_writer_pairs_serialize_without_busy_failures() {
    let env = Env::named("writers");
    burst(&env, 0..12, "writer pair");
}

#[test]
fn concurrent_writers_with_status_readers_serialize() {
    let env = Env::named("readers");
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let reader_db = env.db.clone();
    let reader_stop = stop.clone();
    let reader = std::thread::spawn(move || {
        while !reader_stop.load(Ordering::SeqCst) {
            if let Ok(store) = storage_sqlite::SqliteStore::open(&reader_db) {
                let _ = store.list_events_ordered();
            }
        }
    });
    burst(&env, 100..110, "writer pair under reads");
    stop.store(true, Ordering::SeqCst);
    reader.join().expect("reader thread");
}
