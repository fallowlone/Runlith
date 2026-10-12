//! Advisory verification auto-invocation (TASK-0165, G9): the
//! workspace cycle derives the verification plan for every concluded
//! attempt and executes V1..V3 plans through the TASK-0162 runner.
//! Results are evidence only — the TASK-0125 admission path proceeds
//! unchanged — and V0 skips by construction.
//!
//! The stub executor writes fixture changes into the lease worktree so
//! the advisory stage plans from a real `compute_diff`, exactly as in
//! production; the runner resolves derived programs to fixture scripts
//! so no real toolchain runs.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use app_services::{
    AdvisoryRunner, AdvisorySkipReason, AdvisoryVerification, AdvisoryVerificationPolicy,
    AdvisoryVerificationRequest, FixedClock, G6SecurityService, ManagedWorkspaceRoot,
    OrchestrationIds, OrchestrationService, VerificationRunner, WorkspaceRequest, WorkspaceService,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use git_cli::GitCli;
use scheduler::{ExecutionPort, ExecutionProblem, OrchestrationOutcome, WorkEvidence, WorkItem};
use storage_sqlite::SqliteStore;
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(16000);
const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct TestEnv {
    dir: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    root: PathBuf,
    tools: PathBuf,
}

impl TestEnv {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-advisory-cycle-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        let db = dir.join("test.db");
        let repo = dir.join("repo");
        let root = dir.join("managed-roots");
        let tools = dir.join("tools");
        std::fs::create_dir_all(&root).expect("managed root");
        std::fs::create_dir_all(&tools).expect("tools dir");
        let root = std::fs::canonicalize(&root).expect("canonical root");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        Self {
            dir,
            db,
            repo,
            root,
            tools,
        }
    }

    fn orchestration(&self) -> OrchestrationService<FixedClock> {
        OrchestrationService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("open")
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

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.tools.join(name);
        std::fs::write(&path, body).expect("fixture script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).unwrap();
        }
        path
    }

    fn runner(&self, resolve: app_services::ExecutableResolver) -> VerificationRunner<FixedClock> {
        let store = SqliteStore::open(&self.db).expect("store");
        let security =
            G6SecurityService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("security");
        VerificationRunner::new(
            store,
            security,
            Arc::new(runner_native::NativeRunner),
            runner::IsolationRequirements::trusted_host(),
            resolve,
        )
    }

    fn effect_count(&self) -> i64 {
        let connection = rusqlite::Connection::open(&self.db).unwrap();
        connection
            .query_row("SELECT COUNT(*) FROM observed_effects", [], |row| {
                row.get(0)
            })
            .unwrap()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(repo: &Path, args: &[&str]) {
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

fn seed_ready(
    tasks: &mut app_services::TaskService<FixedClock>,
    project: &str,
    id: &str,
) -> TaskId {
    if tasks
        .get_project(&ProjectId::new(project).unwrap())
        .unwrap()
        .is_none()
    {
        tasks
            .create_project(
                &Project::new(
                    ProjectId::new(project).unwrap(),
                    format!("project {project}"),
                )
                .unwrap(),
            )
            .unwrap();
    }
    let task = Task::draft(
        TaskId::new(id).unwrap(),
        ProjectId::new(project).unwrap(),
        format!("objective {id}"),
    )
    .unwrap();
    tasks.create_task(&task).unwrap();
    tasks.mark_ready(&task.id().clone()).unwrap();
    task.id().clone()
}

fn ids(execution: &str, attempt: &str) -> OrchestrationIds {
    OrchestrationIds {
        execution_id: ExecutionId::new(execution).unwrap(),
        attempt_id: AttemptId::new(attempt).unwrap(),
    }
}

fn workspace_request(env: &TestEnv, exec: &ExecutionId) -> WorkspaceRequest {
    WorkspaceRequest {
        execution_id: exec.clone(),
        repository_ref: workspace::RepositoryRef::new("repo-local:advisory").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    }
}

/// Pure-Rust stub executor: the candidate change is written into the
/// lease worktree by the factory (which observes the lease), the stub
/// itself only reports success.
struct StubExecutor;

impl ExecutionPort for StubExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("advisory-stub").unwrap()
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        Ok(WorkEvidence {
            summary: format!("stub evidence for attempt {}", work.attempt_id),
            evidence_ref: format!("stub-evidence:{}", work.attempt_id),
            partial: false,
        })
    }
}

/// Evaluator that reaches no verdict: the cycle errors after the
/// submit already committed (Task left VERIFYING).
struct FailingEvaluator;

impl app_services::EvaluationPort for FailingEvaluator {
    fn evaluate(
        &self,
        _request: &app_services::EvaluationRequest,
    ) -> Result<app_services::EvaluationVerdict, evaluation::EvaluationError> {
        Err(evaluation::EvaluationError::new("no verdict available"))
    }
}

fn verification_request(
    runner: &mut VerificationRunner<FixedClock>,
) -> AdvisoryVerificationRequest<'_, FixedClock> {
    AdvisoryVerificationRequest {
        runner: AdvisoryRunner::Ready(runner),
        policy: AdvisoryVerificationPolicy {
            timeout: Duration::from_secs(60),
            per_stream_cap: 65536,
        },
    }
}

#[test]
fn docs_only_cycle_skips_v0_and_verifies_as_before() {
    let env = TestEnv::new("v0-cycle");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let mut runner = env.runner(Arc::new(|program| {
        panic!("V0 cycle must never resolve an executable, got {program}")
    }));

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |task, lease| {
                assert_eq!(task.id(), &task_id);
                std::fs::write(
                    Path::new(lease.worktree_ref()).join("README.md"),
                    "# candidate docs\n",
                )
                .expect("candidate change");
                StubExecutor
            },
            &evaluator,
            None,
            Some(verification_request(&mut runner)),
        )
        .expect("workspace-bound cycle");

    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified {
                advisory: Some(AdvisoryVerification::Skipped {
                    reason: AdvisorySkipReason::LevelV0
                }),
                ..
            }
        ),
        "docs-only cycle must verify with a V0 skip, got {outcome:?}"
    );
    assert_eq!(env.effect_count(), 0);
}

#[test]
fn manifest_cycle_executes_v3_and_feeds_the_repair_loop() {
    let env = TestEnv::new("v3-cycle");
    let script = env.script("ok.sh", "#!/bin/sh\nexit 0\n");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let mut runner = env.runner(Arc::new(move |_| Ok(script.clone())));

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |task, lease| {
                assert_eq!(task.id(), &task_id);
                let worktree = Path::new(lease.worktree_ref());
                std::fs::write(worktree.join("Cargo.toml"), "[package]\n").expect("manifest");
                std::fs::create_dir_all(worktree.join("src")).expect("src dir");
                std::fs::write(worktree.join("src/main.rs"), "fn main() {}\n").expect("code");
                StubExecutor
            },
            &evaluator,
            None,
            Some(verification_request(&mut runner)),
        )
        .expect("workspace-bound cycle");

    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified {
                advisory: Some(AdvisoryVerification::Completed {
                    commands_executed: 3
                }),
                ..
            }
        ),
        "manifest cycle must verify with executed V3 evidence, got {outcome:?}"
    );
    assert_eq!(env.effect_count(), 3);
    // The repair loop binds verdicts to attempt effects: the advisory
    // evidence must be attributable to this attempt.
    let store = SqliteStore::open(&env.db).expect("store");
    let effects = store
        .list_observed_effects_for_attempt(
            &ExecutionId::new("E-1").unwrap(),
            &AttemptId::new("A-1").unwrap(),
        )
        .expect("attempt effects");
    assert_eq!(effects.len(), 3);
    assert!(
        effects
            .iter()
            .all(|effect| effect.subject_ref().as_str().starts_with("verification:")),
        "advisory effects must carry verification subjects"
    );
}

#[test]
fn evaluator_failure_after_submit_still_records_advisory_evidence() {
    // Trigger fidelity: the advisory trigger is the submit, not the
    // outcome. A verdict failure leaves the Task VERIFYING with the
    // submit committed — the stage still runs for its durable evidence
    // while the original error is preserved.
    let env = TestEnv::new("error-path");
    let script = env.script("ok.sh", "#!/bin/sh\nexit 0\n");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FailingEvaluator;
    let mut runner = env.runner(Arc::new(move |_| Ok(script.clone())));

    let error = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, lease| {
                let worktree = Path::new(lease.worktree_ref());
                std::fs::write(worktree.join("Cargo.toml"), "[package]\n").expect("manifest");
                StubExecutor
            },
            &evaluator,
            None,
            Some(verification_request(&mut runner)),
        )
        .expect_err("verdict failure must still error");

    assert!(
        format!("{error:?}").contains("no verdict"),
        "original error must be preserved, got {error:?}"
    );
    assert_eq!(env.effect_count(), 3);
    let state = orchestration
        .tasks()
        .get_task(&task_id)
        .unwrap()
        .unwrap()
        .state();
    assert_eq!(state, task_domain::TaskState::Verifying);
}

#[test]
fn cycle_without_verification_context_reports_no_stage() {
    let env = TestEnv::new("no-context");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, _| StubExecutor,
            &evaluator,
            None,
            None,
        )
        .expect("workspace-bound cycle");

    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified { advisory: None, .. }
        ),
        "context-less cycle must verify with no advisory stage, got {outcome:?}"
    );
    assert_eq!(env.effect_count(), 0);
}
