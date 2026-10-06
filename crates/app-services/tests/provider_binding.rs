//! TASK-0046 orchestration binding: `schedule_in_workspace` proves the
//! first-provider vertical slice through the durable provider-neutral
//! architecture without rewriting it.
//!
//! ```text
//! durable Task → Execution → Attempt → WorkspaceLease → executor
//! → canonical WorkEvidence → conclude → submit → independent verify
//! ```
//!
//! Portable tests use a pure-Rust stub executor (no process) to prove the
//! binding is provider-neutral — any `ExecutionPort` binds, not just
//! Claude. Unix tests drive the real `ClaudeExecutor` against fixture
//! executables to prove cwd binding, canonical translation, completion
//! authority, and failure preservation end to end.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use app_services::{
    FixedClock, ManagedWorkspaceRoot, OrchestrationIds, OrchestrationService, RuntimeService,
    TaskService, WorkspaceRequest, WorkspaceService,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use git_cli::GitCli;
use scheduler::{ExecutionPort, ExecutionProblem, OrchestrationOutcome, WorkEvidence, WorkItem};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(9000);
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
            "runlith-provider-binding-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        let db = dir.join("test.db");
        let repo = dir.join("repo");
        let root = dir.join("managed-roots");
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

fn seed_ready(tasks: &mut TaskService<FixedClock>, project: &str, id: &str) -> TaskId {
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
        repository_ref: workspace::RepositoryRef::new("repo-local:binding").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    }
}

/// Pure-Rust stub executor proving the binding is provider-neutral: no
/// Claude involved at all, yet the full lease-bound cycle runs.
struct StubExecutor {
    executes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl ExecutionPort for StubExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("binding-stub").unwrap()
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        self.executes.fetch_add(1, Ordering::SeqCst);
        Ok(WorkEvidence {
            summary: format!("stub evidence for attempt {}", work.attempt_id),
            evidence_ref: format!("stub-evidence:{}", work.attempt_id),
            partial: false,
        })
    }
}

#[test]
fn workspace_bound_cycle_verifies_through_independent_evaluation() {
    let env = TestEnv::new("stub-verify");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executor = StubExecutor {
        executes: calls.clone(),
    };

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |task, lease| {
                // The factory observes durable Task + ready lease only.
                assert_eq!(task.id(), &task_id);
                assert_eq!(lease.owner().as_str(), "E-1");
                assert!(Path::new(lease.worktree_ref()).is_dir());
                executor
            },
            &evaluator,
            None,
        )
        .expect("workspace-bound cycle");
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        ),
        "stub success must verify, got {outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(evaluator.call_count(), 1);
    // The lease outlives the cycle: attempts share it, evaluation owns
    // completion, teardown stays explicit.
    let lease = workspace
        .workspace_for_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .expect("lease persists after verification");
    assert_eq!(lease.lifecycle(), workspace::LeaseLifecycle::Ready);
}

#[test]
fn provider_success_without_verification_never_completes() {
    // G5 completion-authority regression through the new binding: a
    // successful provider result with a rejecting evaluator ends
    // RETRY_READY — never COMPLETED. Provider output is evidence, and
    // only the evaluation verdict grants completion.
    let env = TestEnv::new("stub-reject");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysRejected);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executor = StubExecutor {
        executes: calls.clone(),
    };

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect("workspace-bound cycle");
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededButRejected { .. }
        ),
        "rejecting evaluator must reject, got {outcome:?}"
    );
    let state = orchestration
        .tasks()
        .get_task(&task_id)
        .unwrap()
        .unwrap()
        .state();
    assert_eq!(
        state,
        TaskState::RetryReady,
        "rejection routes to retry, never completion"
    );
}

#[test]
fn provider_failure_is_attempt_failure_never_task_failure() {
    struct FailExecutor;
    impl ExecutionPort for FailExecutor {
        fn provider_ref(&self) -> ProviderRef {
            ProviderRef::new("fail-stub").unwrap()
        }
        fn execute(&self, _work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
            Err(ExecutionProblem::Failed {
                detail: "simulated provider failure".to_owned(),
            })
        }
    }

    let env = TestEnv::new("stub-fail");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let executor = FailExecutor;

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect("cycle reports failure");
    assert!(
        matches!(outcome, OrchestrationOutcome::ExecutionFailed { .. }),
        "provider failure must surface, got {outcome:?}"
    );
    assert_eq!(evaluator.call_count(), 0, "no evidence → no evaluation");
    let state = orchestration
        .tasks()
        .get_task(&task_id)
        .unwrap()
        .unwrap()
        .state();
    assert_eq!(
        state,
        TaskState::Running,
        "provider failure != task failure"
    );
}

#[test]
fn execution_id_mismatch_fails_before_any_mutation() {
    let env = TestEnv::new("mismatch");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executor = StubExecutor {
        executes: calls.clone(),
    };

    let bad_request = WorkspaceRequest {
        execution_id: ExecutionId::new("E-OTHER").unwrap(),
        repository_ref: workspace::RepositoryRef::new("repo-local:binding").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    };
    let err = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            bad_request,
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect_err("mismatched execution must fail");
    let state = orchestration
        .tasks()
        .get_task(&task_id)
        .unwrap()
        .unwrap()
        .state();
    assert_eq!(
        state,
        TaskState::Ready,
        "failed binding commits nothing, got {err:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn lease_failure_leaves_honest_recoverable_intermediate() {
    let env = TestEnv::new("lease-fail");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executor = StubExecutor {
        executes: calls.clone(),
    };

    let mut bad_request = workspace_request(&env, &ExecutionId::new("E-1").unwrap());
    bad_request.base_revision = "no-such-ref-anywhere".to_owned();
    let err = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            bad_request,
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect_err("bad base must fail at the lease step");
    // Honest intermediate: RUNNING + execution committed, no attempt, no
    // executor invocation, no silent rollback. A retry may create A-2
    // under E-1 or reconcile explicitly — never a hidden mega-state.
    let state = orchestration
        .tasks()
        .get_task(&task_id)
        .unwrap()
        .unwrap()
        .state();
    assert_eq!(
        state,
        TaskState::Running,
        "entry facts stay committed, got {err:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(evaluator.call_count(), 0);
    assert!(
        workspace
            .workspace_for_execution(&ExecutionId::new("E-1").unwrap())
            .unwrap()
            .is_none(),
        "no lease on lease failure"
    );
}

// -- real ClaudeExecutor through the binding (unix fixtures) -----------------

#[cfg(unix)]
mod unix {
    use super::*;
    use provider_claude::{build_task_prompt, ClaudeConfig, ClaudeExecutor};
    use std::ffi::OsString;
    use std::io::Write;
    use std::time::Duration;

    /// Explicit provider host environment for the fixture executables, so the
    /// tests do not depend on the runner's ambient login session: containers
    /// commonly have no USER, which the provider requires (TASK-0155).
    fn host_environment(env: &TestEnv) -> Vec<(OsString, OsString)> {
        vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), env.dir.clone().into_os_string()),
            ("USER".into(), "runlith-fixture".into()),
        ]
    }

    fn fixture(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(body.as_bytes()).expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        path
    }

    #[test]
    fn explicit_fixture_environment_refuses_runner_composition() {
        use plan_domain::{
            AtomicityRequest, PlannerCallSpend, PlannerFault, PlannerPort, ScopeBound,
        };
        use policy::{Capability, EffectiveCapabilities, ScopeId};
        use provider_claude::{ClaudeFailure, ClaudePlanner};
        use runner::{prepare_process_launch, ChildEnvironment, IsolationRequirements};
        use std::sync::Arc;

        let env = TestEnv::new("explicit-host-boundary");
        let workdir = std::fs::canonicalize(&env.repo).unwrap();
        let exe = fixture(
            &env.dir,
            "boundary.sh",
            r#"echo spawned > spawned
printf '{"type":"result","subtype":"success","is_error":false,"result":"fixture success","session_id":"boundary"}'
"#,
        );
        let config = ClaudeConfig::default().with_executable(&exe).unwrap();
        // A real Runner launch object, with honest zero-axis trusted-host
        // enforcement. This exercises attachment without requiring Seatbelt.
        let prepare = |pairs| {
            Arc::new(
                prepare_process_launch(
                    Arc::new(runner_native::NativeRunner),
                    &EffectiveCapabilities::new(vec![
                        Capability::process(exe.to_str().unwrap()).unwrap()
                    ]),
                    workdir.to_str().unwrap(),
                    ScopeId::new(exe.to_str().unwrap()).unwrap(),
                    IsolationRequirements::trusted_host(),
                    ChildEnvironment::from_explicit(pairs).unwrap(),
                )
                .unwrap(),
            )
        };
        let launch = prepare(host_environment(&env));
        let executor = ClaudeExecutor::bind_trusted_host_with_host_environment(
            config.clone(),
            "fixture prompt",
            &workdir,
            env.dir.join("provider-env"),
            host_environment(&env),
        )
        .unwrap();
        let planner = ClaudePlanner::bind_trusted_host_with_host_environment(
            config.clone(),
            &workdir,
            env.dir.join("planner-env"),
            host_environment(&env),
        )
        .unwrap();
        let refusal = "explicit host environment is restricted to trusted-host execution";
        // Test direct attachment first: a launch prepared elsewhere must not
        // bypass the boundary, and reattachment/cloning must not restore it.
        let forbidden = executor.clone().with_sandboxed_launch(launch.clone());
        let work = WorkItem {
            task_id: TaskId::new("T-BOUNDARY").unwrap(),
            execution_id: ExecutionId::new("E-BOUNDARY").unwrap(),
            attempt_id: AttemptId::new("A-BOUNDARY").unwrap(),
        };
        for executor in [
            forbidden.clone(),
            forbidden.with_sandboxed_launch(launch.clone()),
        ] {
            assert_eq!(
                executor.execute_detailed(&work).unwrap_err(),
                ClaudeFailure::EnvironmentIsolation {
                    detail: refusal.into()
                }
            );
            assert_eq!(executor.child_environment().unwrap_err(), refusal);
            assert_eq!(
                executor.isolation_support_read_roots().unwrap_err(),
                refusal
            );
            assert_eq!(
                executor.isolation_support_write_roots().unwrap_err(),
                refusal
            );
        }
        let forbidden_planner = planner.clone().with_sandboxed_launch(launch.clone());
        assert!(matches!(
            forbidden_planner.assess_atomicity(AtomicityRequest {
                task_ref: "T-BOUNDARY".into(),
                acceptance_criteria_refs: vec![],
                scope_bound: ScopeBound::new(vec!["repo:src".into()]).unwrap(),
                evidence_refs: vec![],
                execution_policy_ref: "fixture-policy".into(),
            }),
            Err(PlannerFault::ProviderFailure { typed_cause, spend: PlannerCallSpend::NotStarted })
                if typed_cause.contains(refusal)
        ));
        assert_eq!(executor.child_environment().unwrap_err(), refusal);
        assert_eq!(
            executor.isolation_support_read_roots().unwrap_err(),
            refusal
        );
        assert_eq!(
            executor.isolation_support_write_roots().unwrap_err(),
            refusal
        );
        for planner in [&planner, &forbidden_planner] {
            assert_eq!(planner.child_environment().unwrap_err(), refusal);
            assert_eq!(planner.isolation_support_read_roots().unwrap_err(), refusal);
            assert_eq!(
                planner.isolation_support_write_roots().unwrap_err(),
                refusal
            );
        }
        assert!(
            !env.repo.join("spawned").exists(),
            "no sandbox or trusted-host fallback spawn"
        );

        // Trusted-host fixtures still run; production bind remains ambient
        // and accepts Runner attachment only if HOME/USER are present.
        assert!(executor.execute(&work).is_ok());
        std::fs::remove_file(env.repo.join("spawned")).unwrap();
        let production = ClaudeExecutor::bind(
            config.clone(),
            "prompt",
            &workdir,
            env.dir.join("production-env"),
        )
        .unwrap();
        match production.child_environment() {
            Ok(pairs) => {
                let user = pairs.iter().find(|(name, _)| name == "USER").unwrap();
                assert_eq!(Some(user.1.clone()), std::env::var_os("USER"));
                let production_launch = prepare(pairs);
                assert!(production
                    .with_sandboxed_launch(production_launch)
                    .execute(&work)
                    .is_ok());
            }
            Err(detail) => {
                assert!(detail.contains("requires HOME") || detail.contains("requires USER"));
                assert!(production
                    .with_sandboxed_launch(launch)
                    .execute(&work)
                    .is_err());
            }
        }
    }

    #[test]
    fn claude_shaped_success_runs_in_lease_and_submits() {
        let env = TestEnv::new("claude-e2e");
        // Fixture Claude: drops a marker file in its cwd (proves lease
        // binding) and reports structured success.
        let fixture_path = fixture(
            &env.dir,
            "claude-fixture.sh",
            r#"echo proof > ./fixture-proof.txt; printf '{"type":"result","subtype":"success","is_error":false,"result":"fixture worked","session_id":"22222222-3333-4444-5555-666666666666","usage":{"input_tokens":3,"output_tokens":5}}'"#,
        );
        let mut orchestration = env.orchestration();
        let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
        let mut workspace = env.workspace_service();
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

        let outcome = orchestration
            .schedule_in_workspace(
                &mut workspace,
                &task_id,
                ids("E-9", "A-9"),
                workspace_request(&env, &ExecutionId::new("E-9").unwrap()),
                |task, lease| {
                    let prompt = build_task_prompt(task.objective(), task.acceptance());
                    let config = ClaudeConfig::default()
                        .with_executable(fixture_path.clone())
                        .expect("fixture exe")
                        .with_timeout(Duration::from_secs(30))
                        .expect("timeout");
                    ClaudeExecutor::bind_trusted_host_with_host_environment(
                        config,
                        prompt,
                        lease.worktree_ref().to_owned(),
                        env.dir.join("provider-env"),
                        host_environment(&env),
                    )
                    .expect("bind executor to lease path")
                },
                &evaluator,
                None,
            )
            .expect("claude-shaped cycle");
        assert!(
            matches!(
                outcome,
                OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
            ),
            "fixture success must verify, got {outcome:?}"
        );
        // The child provably ran inside the lease worktree (G4 isolation
        // through the real binding): the marker file exists exactly there
        // and nowhere else. The attempt evidence references the opaque
        // provider session — never a durable identity.
        let lease = workspace
            .workspace_for_execution(&ExecutionId::new("E-9").unwrap())
            .unwrap()
            .expect("lease");
        let marker = Path::new(lease.worktree_ref()).join("fixture-proof.txt");
        assert!(
            marker.is_file(),
            "fixture child must have run in the lease worktree"
        );
        assert_eq!(
            std::fs::read(&marker).expect("marker bytes"),
            b"proof\n",
            "marker content intact (no TOCTOU games in this path)"
        );
        let runtime = RuntimeService::open(&env.db, FixedClock(FIXED_MILLIS)).expect("runtime");
        let execution = runtime
            .get_execution(&ExecutionId::new("E-9").unwrap())
            .expect("read execution")
            .expect("execution exists");
        let attempt = execution
            .get_attempt(&AttemptId::new("A-9").unwrap())
            .expect("attempt");
        assert_eq!(
            attempt.outcome(),
            Some(agent_runtime::AttemptOutcome::Succeeded)
        );
        let detail = attempt.detail().expect("attempt detail");
        assert!(
            detail.contains("claude:session:22222222-3333-4444-5555-666666666666"),
            "got {detail:?}"
        );
        let state = orchestration
            .tasks()
            .get_task(&task_id)
            .unwrap()
            .unwrap()
            .state();
        assert_eq!(
            state,
            TaskState::Completed,
            "verified path completes via evaluation"
        );
    }

    #[test]
    fn h7_provider_state_outside_worktree_allows_normal_cleanup() {
        let env = TestEnv::new("claude-h7-cleanup");
        let fixture_path = fixture(
            &env.dir,
            "claude-h7-fixture.sh",
            r#"mkdir -p "$XDG_DATA_HOME"; echo provider-state > "$XDG_DATA_HOME/sessions.db"; printf '{"type":"result","subtype":"success","is_error":false,"result":"fixture worked","session_id":"99999999-9999-9999-9999-999999999999","usage":{"input_tokens":1,"output_tokens":1}}'"#,
        );
        let mut orchestration = env.orchestration();
        let task_id = seed_ready(orchestration.tasks_mut(), "P-H7", "T-H7");
        let mut workspace = env.workspace_service();
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        let environment_base = env.dir.join("provider-env");

        let outcome = orchestration
            .schedule_in_workspace(
                &mut workspace,
                &task_id,
                ids("E-H7", "A-H7"),
                workspace_request(&env, &ExecutionId::new("E-H7").unwrap()),
                |task, lease| {
                    let prompt = build_task_prompt(task.objective(), task.acceptance());
                    let config = ClaudeConfig::default()
                        .with_executable(fixture_path.clone())
                        .expect("fixture exe")
                        .with_timeout(Duration::from_secs(30))
                        .expect("timeout");
                    ClaudeExecutor::bind_trusted_host_with_host_environment(
                        config,
                        prompt,
                        lease.worktree_ref().to_owned(),
                        environment_base.clone(),
                        host_environment(&env),
                    )
                    .expect("bind isolated executor")
                },
                &evaluator,
                None,
            )
            .expect("isolated claude-shaped cycle");
        assert!(matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        ));

        let lease = workspace
            .workspace_for_execution(&ExecutionId::new("E-H7").unwrap())
            .unwrap()
            .expect("lease")
            .clone();
        let worktree = Path::new(lease.worktree_ref());
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(worktree)
            .output()
            .expect("worktree status");
        assert!(status.status.success());
        assert!(
            status.stdout.is_empty(),
            "provider runtime state must not dirty the managed worktree"
        );
        let provider_state = environment_base
            .join(worktree.file_name().expect("managed worktree leaf"))
            .join("data/sessions.db");
        assert!(
            provider_state.is_file(),
            "provider state must remain outside Git"
        );

        let cleaned = workspace
            .cleanup_workspace(lease.id(), &env.repo_path())
            .expect("ordinary cleanup stays safe after isolated provider run");
        assert_eq!(
            cleaned.lease.lifecycle(),
            workspace::LeaseLifecycle::Removed
        );
        assert!(!worktree.exists(), "managed worktree must be removed");
        assert!(
            provider_state.is_file(),
            "TASK-0065 keeps provider runtime evidence separate from Git cleanup"
        );
    }
}

// -- TASK-0135 (G8) prerequisite preflight gate ----------------------------
//
// Durable prerequisites on the task record are checked at workspace-attempt
// time against the live repository environment. A positively `Failed`
// check maps onto the journaled `Block` transition with the bounded
// blocker report as its durable reason; the provider is never invoked.

fn seed_ready_with_prerequisites(
    tasks: &mut TaskService<FixedClock>,
    project: &str,
    id: &str,
    prerequisites: Vec<task_domain::Prerequisite>,
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
    let mut task = Task::draft(
        TaskId::new(id).unwrap(),
        ProjectId::new(project).unwrap(),
        format!("objective {id}"),
    )
    .unwrap();
    task.define_prerequisites(prerequisites).unwrap();
    tasks.create_task(&task).unwrap();
    tasks.mark_ready(&task.id().clone()).unwrap();
    task.id().clone()
}

#[test]
fn failed_prerequisites_block_before_provider_invocation_with_durable_report() {
    let env = TestEnv::new("preflight-block");
    let mut orchestration = env.orchestration();
    // Two positively-failing checks: a missing repo file and a git ref
    // that does not resolve. Both must land in the blocker report.
    let task_id = seed_ready_with_prerequisites(
        orchestration.tasks_mut(),
        "P-1",
        "T-1",
        vec![
            task_domain::Prerequisite::new(
                task_domain::PrerequisiteKind::RepoFileExists,
                "missing/required-input.txt",
            )
            .unwrap(),
            task_domain::Prerequisite::new(
                task_domain::PrerequisiteKind::GitAncestorRef,
                "refs/heads/nonexistent-base",
            )
            .unwrap(),
        ],
    );
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executor = StubExecutor {
        executes: calls.clone(),
    };

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect("preflight block is a decided outcome, not an error");
    let OrchestrationOutcome::BlockedByPrerequisite {
        task_id: blocked,
        execution_id,
        reason,
    } = outcome
    else {
        panic!("failing prerequisites must block, got {outcome:?}");
    };
    assert_eq!(blocked, task_id);
    assert!(reason.contains("missing/required-input.txt"), "{reason}");
    assert!(reason.contains("refs/heads/nonexistent-base"), "{reason}");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "the provider is never invoked for a blocked task"
    );

    // The blocker report is durable: task state plus the journaled
    // `Blocked` fact both carry it, readable after the fact from the
    // canonical store.
    let task = orchestration.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Blocked);
    let events = orchestration
        .tasks()
        .store()
        .list_events_for_task(&task_id)
        .unwrap();
    let blocked_event = events
        .iter()
        .find(|event| event.fact.event_type == storage_sqlite::EventType::Blocked)
        .expect("journaled Blocked fact");
    assert_eq!(blocked_event.fact.from_state, Some(TaskState::Running));
    assert_eq!(blocked_event.fact.to_state, Some(TaskState::Blocked));
    assert_eq!(
        blocked_event.fact.reason.as_deref(),
        Some(reason.as_str()),
        "the journaled reason is the blocker report"
    );

    // Recovery shape: admitted-but-not-attempted (zero attempts), so the
    // existing pre-attempt resume path owns the recovery once the
    // prerequisite is satisfied.
    assert!(
        orchestration
            .runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .is_none(),
        "no attempt is created for a blocked task"
    );
    assert!(
        orchestration
            .runtime()
            .get_execution(&execution_id)
            .unwrap()
            .is_some(),
        "the admitted execution stays durable"
    );
}

#[test]
fn satisfied_prerequisites_let_the_workspace_cycle_proceed() {
    let env = TestEnv::new("preflight-pass");
    // The declared file exists in the repository working tree, and the
    // declared command is on PATH (the fixtures themselves invoke git).
    std::fs::write(env.repo.join("required-input.txt"), "present").expect("repo file");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready_with_prerequisites(
        orchestration.tasks_mut(),
        "P-1",
        "T-1",
        vec![
            task_domain::Prerequisite::new(
                task_domain::PrerequisiteKind::RepoFileExists,
                "required-input.txt",
            )
            .unwrap(),
            task_domain::Prerequisite::new(task_domain::PrerequisiteKind::CommandOnPath, "git")
                .unwrap(),
        ],
    );
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let executor = StubExecutor {
        executes: calls.clone(),
    };

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect("satisfied prerequisites never block the cycle");
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        ),
        "satisfied prerequisites let the cycle verify, got {outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn task_without_prerequisites_skips_the_gate_entirely() {
    // Legacy tasks (no declared prerequisites) behave exactly as before
    // TASK-0135: no check, no block, no journaled prerequisite fact.
    let env = TestEnv::new("preflight-none");
    let mut orchestration = env.orchestration();
    let task_id = seed_ready(orchestration.tasks_mut(), "P-1", "T-1");
    let mut workspace = env.workspace_service();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let executor = StubExecutor {
        executes: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };

    let outcome = orchestration
        .schedule_in_workspace(
            &mut workspace,
            &task_id,
            ids("E-1", "A-1"),
            workspace_request(&env, &ExecutionId::new("E-1").unwrap()),
            |_, _| executor,
            &evaluator,
            None,
        )
        .expect("empty declaration is not a blocker");
    assert!(matches!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
}
