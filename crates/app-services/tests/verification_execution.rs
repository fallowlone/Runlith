//! Verification-plan execution tests (TASK-0162): plans execute only
//! through the G6 runner boundary, argv-only, with bounded capture and
//! digest-only evidence. These tests drive the shipped service
//! (`VerificationRunner` + durable store), not the pure vocabulary.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent_runtime::{AttemptId, Execution, ExecutionId, ProviderRef};
use app_services::{
    derive_commands, Ecosystem, ExecutableResolver, FixedClock, G6SecurityService, TaskService,
    VerificationCommand, VerificationExecutionError, VerificationExecutionRequest,
    VerificationLevel, VerificationPlan, VerificationRunner,
};
use evaluation::EffectPayload;
use storage_sqlite::{EventType, NewJournalEvent, SqliteStore};
use task_domain::{Project, ProjectId, Task, TaskId};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const FIXED_MILLIS: u64 = 1_704_067_200_000;
const FIXED_STAMP: &str = "2024-01-01T00:00:00Z";

struct Fixture {
    dir: PathBuf,
    db: PathBuf,
    workspace: PathBuf,
}

impl Fixture {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-verify-exec-test-{}-{suite}-{id}",
            std::process::id()
        ));
        let workspace = dir.join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace dir");
        Self {
            db: dir.join("test.db"),
            dir,
            workspace,
        }
    }

    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.workspace.join(name);
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

    fn seed_lineage(&self) -> (TaskId, ExecutionId, AttemptId) {
        let mut service = TaskService::open(&self.db, FixedClock(FIXED_MILLIS)).expect("service");
        let project_id = ProjectId::new("P-1").unwrap();
        service
            .create_project(&Project::new(project_id.clone(), "project P-1").unwrap())
            .unwrap();
        let task_id = TaskId::new("T-1").unwrap();
        let task = Task::draft(task_id.clone(), project_id, "verify me").unwrap();
        service.create_task(&task).unwrap();
        let task = service.store().get_task(&task_id).unwrap().unwrap();
        let execution_id = ExecutionId::new("E-1").unwrap();
        let mut execution = Execution::begin(execution_id.clone(), task_id.clone());
        service
            .store_mut()
            .create_execution_with_event(
                &execution,
                &journal(
                    EventType::ExecutionCreated,
                    &task,
                    Some(execution_id.clone()),
                    None,
                ),
            )
            .unwrap();
        let attempt_id = AttemptId::new("A-1").unwrap();
        execution
            .start_attempt(
                attempt_id.clone(),
                ProviderRef::new("test-provider").unwrap(),
            )
            .unwrap();
        let attempt = execution.get_attempt(&attempt_id).unwrap().clone();
        service
            .store_mut()
            .create_attempt_with_event(
                &attempt,
                &journal(
                    EventType::AttemptCreated,
                    &task,
                    Some(execution_id.clone()),
                    Some(attempt_id.clone()),
                ),
            )
            .unwrap();
        (task_id, execution_id, attempt_id)
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

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn journal(
    kind: EventType,
    task: &Task,
    execution_id: Option<ExecutionId>,
    attempt_id: Option<AttemptId>,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: kind,
        task_id: Some(task.id().clone()),
        project_id: Some(task.project_id().clone()),
        from_state: None,
        to_state: None,
        reason: None,
        correlation_id: "verification-execution-test".to_owned(),
        causation_id: "verification-execution-test".to_owned(),
        occurred_at: FIXED_STAMP.to_owned(),
        provenance: "app-services/test".to_owned(),
        execution_id,
        attempt_id,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

struct RecordingBackend {
    inner: runner_native::NativeRunner,
    enforcements: AtomicUsize,
}

impl RecordingBackend {
    fn new() -> Self {
        Self {
            inner: runner_native::NativeRunner,
            enforcements: AtomicUsize::new(0),
        }
    }
}

impl runner::IsolationBackend for RecordingBackend {
    fn identity(&self) -> runner::BackendIdentity {
        self.inner.identity()
    }

    fn enforce(
        &self,
        plan: &runner::IsolationPlan,
    ) -> Result<runner::EnforcementReport, runner::RunnerError> {
        self.enforcements.fetch_add(1, Ordering::SeqCst);
        self.inner.enforce(plan)
    }

    fn sandbox(
        &self,
        plan: &runner::IsolationPlan,
        spec: &runner::CommandSpec,
    ) -> Result<runner::LaunchRecipe, runner::RunnerError> {
        self.inner.sandbox(plan, spec)
    }
}

fn runner_with(
    fixture: &Fixture,
    backend: Arc<dyn runner::IsolationBackend>,
    resolve: ExecutableResolver,
) -> VerificationRunner<FixedClock> {
    runner_with_requirements(
        fixture,
        backend,
        runner::IsolationRequirements::trusted_host(),
        resolve,
    )
}

fn runner_with_requirements(
    fixture: &Fixture,
    backend: Arc<dyn runner::IsolationBackend>,
    requirements: runner::IsolationRequirements,
    resolve: ExecutableResolver,
) -> VerificationRunner<FixedClock> {
    let store = SqliteStore::open(&fixture.db).expect("store");
    let security =
        G6SecurityService::open(&fixture.db, FixedClock(FIXED_MILLIS)).expect("security");
    VerificationRunner::new(store, security, backend, requirements, resolve)
}

/// Test double claiming full OS enforcement (what a sandboxing backend
/// reports): proves the strict-requirements path launches when a
/// backend provides it.
struct StrictBackend {
    inner: runner_native::NativeRunner,
}

impl runner::IsolationBackend for StrictBackend {
    fn identity(&self) -> runner::BackendIdentity {
        runner::BackendIdentity {
            name: "test-strict",
            runner: runner::RunnerKind::Native,
            platform: runner::Platform::current(),
        }
    }

    fn enforce(
        &self,
        plan: &runner::IsolationPlan,
    ) -> Result<runner::EnforcementReport, runner::RunnerError> {
        Ok(runner::EnforcementReport::new(
            self.identity(),
            plan.clone(),
            runner::EnforcementAxes::all(),
            true,
        ))
    }

    fn sandbox(
        &self,
        plan: &runner::IsolationPlan,
        spec: &runner::CommandSpec,
    ) -> Result<runner::LaunchRecipe, runner::RunnerError> {
        self.inner.sandbox(plan, spec)
    }
}

fn request(
    fixture: &Fixture,
    plan: VerificationPlan,
    task_id: &TaskId,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
) -> VerificationExecutionRequest {
    VerificationExecutionRequest {
        plan,
        task_id: task_id.as_str().to_owned(),
        execution_id: execution_id.clone(),
        attempt_id: attempt_id.clone(),
        workspace: fixture.workspace.clone(),
        captured_at: FIXED_STAMP.to_owned(),
        timeout: Duration::from_secs(60),
        per_stream_cap: 65536,
    }
}

fn cargo_v1() -> VerificationPlan {
    VerificationPlan {
        level: VerificationLevel::V1,
        ecosystem: Ecosystem::Cargo,
        scope: vec!["src/main.rs".to_owned()],
        commands: derive_commands(Ecosystem::Cargo, VerificationLevel::V1),
    }
}

fn panic_resolver() -> ExecutableResolver {
    Arc::new(|program| panic!("must not resolve {program}"))
}

#[test]
fn smuggled_plan_refuses_before_any_execution() {
    let fixture = Fixture::new("smuggled");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        panic_resolver(),
    );
    let mut plan = cargo_v1();
    plan.commands.push(VerificationCommand {
        program: "sh".to_owned(),
        args: vec!["-c".to_owned(), "evil".to_owned()],
    });
    let error = runner
        .execute(request(
            &fixture,
            plan,
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect_err("smuggled command must refuse");
    assert!(matches!(error, VerificationExecutionError::PlanRefused(_)));
    assert_eq!(backend.enforcements.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.effect_count(), 0);
}

#[test]
fn v0_plan_executes_nothing() {
    let fixture = Fixture::new("v0");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        panic_resolver(),
    );
    let plan = VerificationPlan {
        level: VerificationLevel::V0,
        ecosystem: Ecosystem::Cargo,
        scope: vec![],
        commands: vec![],
    };
    let executed = runner
        .execute(request(
            &fixture,
            plan,
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect("v0 executes nothing successfully");
    assert!(executed.is_empty());
    assert_eq!(backend.enforcements.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.effect_count(), 0);
}

#[test]
fn executes_command_under_runner_and_records_digest_effect() {
    let fixture = Fixture::new("happy");
    // Marker visible to the test process must not reach the child:
    // the runner clears the ambient environment (the shell itself may
    // still add SHLVL/PWD, so the script counts markers, not lines).
    std::env::set_var("RUNLITH_VERIFY_MARKER", "must-not-propagate");
    let script = fixture.script(
        "success.sh",
        "#!/bin/sh\necho ok-output\necho \"markers=$(/usr/bin/env | /usr/bin/grep -c RUNLITH_VERIFY_MARKER || true)\"\nexit 0\n",
    );
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(move |program| {
            assert_eq!(program, "cargo");
            Ok(script.clone())
        }),
    );
    let executed = runner
        .execute(request(
            &fixture,
            cargo_v1(),
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect("derived command executes");
    assert_eq!(executed.len(), 1);
    assert_eq!(backend.enforcements.load(Ordering::SeqCst), 1);
    let store = SqliteStore::open(&fixture.db).expect("reopen");
    let effect = store
        .get_observed_effect(&executed[0].effect_id)
        .unwrap()
        .expect("effect recorded");
    match effect.payload().clone() {
        EffectPayload::ProcessExit {
            exit_code,
            stdout_bytes,
            stdout_sha256,
            stderr_bytes,
            ..
        } => {
            assert_eq!(exit_code, Some(0));
            // "ok-output\nmarkers=0\n": zero markers proves the ambient
            // environment was cleared (no inherited variables).
            assert_eq!(stdout_bytes, 20);
            assert_eq!(stderr_bytes, 0);
            let expected = evaluation::candidate_sha256(b"ok-output\nmarkers=0\n");
            assert_eq!(stdout_sha256.as_str().to_owned(), expected);
        }
        other => panic!("expected ProcessExit payload, got {other:?}"),
    }
}

#[test]
fn nonzero_exit_records_effect_and_returns_typed_error_without_bytes() {
    let fixture = Fixture::new("nonzero");
    let secret = "nvapi-SECRET-MUST-NOT-PERSIST-abcdef0123456789";
    let script = fixture.script("fail.sh", &format!("#!/bin/sh\necho {secret}\nexit 3\n"));
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(move |_| Ok(script.clone())),
    );
    let error = runner
        .execute(request(
            &fixture,
            cargo_v1(),
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect_err("nonzero exit must error");
    assert!(matches!(
        error,
        VerificationExecutionError::NonZeroExit { code: Some(3) }
    ));
    assert!(!format!("{error:?}").contains(secret));
    assert!(!error.to_string().contains(secret));
    assert_eq!(fixture.effect_count(), 1);
    let raw = std::fs::read(&fixture.db).expect("read db bytes");
    assert!(
        !raw.windows(secret.len()).any(|w| w == secret.as_bytes()),
        "secret bytes must not persist anywhere in the database file"
    );
}

#[test]
fn timeout_kills_and_reports_typed() {
    let fixture = Fixture::new("timeout");
    let script = fixture.script("sleep.sh", "#!/bin/sh\n/bin/sleep 30\nexit 0\n");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(move |_| Ok(script.clone())),
    );
    let mut args = request(&fixture, cargo_v1(), &task_id, &execution_id, &attempt_id);
    args.timeout = Duration::from_millis(300);
    let started = std::time::Instant::now();
    let error = runner.execute(args).expect_err("sleep must time out");
    assert!(matches!(error, VerificationExecutionError::Timeout));
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "timeout must kill, not wait out the sleep"
    );
    assert_eq!(fixture.effect_count(), 1);
}

#[test]
fn output_over_cap_truncates_and_completes() {
    let fixture = Fixture::new("flood");
    let script = fixture.script(
        "flood.sh",
        "#!/bin/sh\n/usr/bin/yes A | /usr/bin/head -c 200000\n",
    );
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(move |_| Ok(script.clone())),
    );
    let executed = runner
        .execute(request(
            &fixture,
            cargo_v1(),
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect("flood truncates and completes");
    assert_eq!(executed.len(), 1);
    let store = SqliteStore::open(&fixture.db).expect("reopen");
    let effect = store
        .get_observed_effect(&executed[0].effect_id)
        .unwrap()
        .expect("effect recorded");
    match effect.payload().clone() {
        EffectPayload::ProcessExit {
            stdout_bytes,
            stdout_sha256,
            ..
        } => {
            assert_eq!(stdout_bytes, 65536);
            let full: Vec<u8> = "A\n".repeat(100000).into_bytes();
            let expected = evaluation::candidate_sha256(&full[..65536]);
            assert_eq!(stdout_sha256.as_str().to_owned(), expected);
        }
        other => panic!("expected ProcessExit payload, got {other:?}"),
    }
}

#[test]
fn strict_requirements_launch_when_backend_enforces() {
    let fixture = Fixture::new("strict-ok");
    let script = fixture.script("success.sh", "#!/bin/sh\nexit 0\n");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend: Arc<dyn runner::IsolationBackend> = Arc::new(StrictBackend {
        inner: runner_native::NativeRunner,
    });
    let store = SqliteStore::open(&fixture.db).expect("store");
    let security =
        G6SecurityService::open(&fixture.db, FixedClock(FIXED_MILLIS)).expect("security");
    let mut runner = VerificationRunner::new(
        store,
        security,
        backend,
        runner::IsolationRequirements::new(true, true, true),
        Arc::new(move |_| Ok(script.clone())),
    );
    let executed = runner
        .execute(request(
            &fixture,
            cargo_v1(),
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect("strict backend launches");
    assert_eq!(executed.len(), 1);
}

#[test]
fn strict_requirements_refuse_when_backend_cannot_enforce() {
    let fixture = Fixture::new("strict-no");
    let script = fixture.script("success.sh", "#!/bin/sh\nexit 0\n");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with_requirements(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        runner::IsolationRequirements::new(true, true, true),
        Arc::new(move |_| Ok(script.clone())),
    );
    let error = runner
        .execute(request(
            &fixture,
            cargo_v1(),
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect_err("native backend cannot meet strict requirements");
    assert!(matches!(error, VerificationExecutionError::LaunchRefused));
    assert_eq!(fixture.effect_count(), 0);
}

#[test]
fn v3_plan_executes_all_commands_in_order() {
    let fixture = Fixture::new("v3");
    let script = fixture.script("success.sh", "#!/bin/sh\nexit 0\n");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(move |_| Ok(script.clone())),
    );
    let plan = VerificationPlan {
        level: VerificationLevel::V3,
        ecosystem: Ecosystem::Cargo,
        scope: vec!["src/main.rs".to_owned()],
        commands: derive_commands(Ecosystem::Cargo, VerificationLevel::V3),
    };
    assert_eq!(plan.commands.len(), 3);
    let executed = runner
        .execute(request(
            &fixture,
            plan,
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect("v3 executes all");
    assert_eq!(executed.len(), 3);
    assert_eq!(executed[2].position, 2);
    assert_eq!(backend.enforcements.load(Ordering::SeqCst), 3);
    assert_eq!(fixture.effect_count(), 3);
}

#[test]
fn failing_command_stops_the_plan_fail_fast() {
    let fixture = Fixture::new("failfast");
    let script = fixture.script(
        "selective.sh",
        "#!/bin/sh\nif [ \"$1\" = \"clippy\" ]; then exit 7; fi\nexit 0\n",
    );
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(move |_| Ok(script.clone())),
    );
    let plan = VerificationPlan {
        level: VerificationLevel::V3,
        ecosystem: Ecosystem::Cargo,
        scope: vec!["src/main.rs".to_owned()],
        commands: derive_commands(Ecosystem::Cargo, VerificationLevel::V3),
    };
    let error = runner
        .execute(request(
            &fixture,
            plan,
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect_err("clippy failure must stop the plan");
    assert!(matches!(
        error,
        VerificationExecutionError::NonZeroExit { code: Some(7) }
    ));
    // fmt recorded ok, clippy recorded failed, test never launched.
    assert_eq!(backend.enforcements.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.effect_count(), 2);
}

#[test]
fn unresolved_executable_refuses_without_enforcement() {
    let fixture = Fixture::new("unresolved");
    let (task_id, execution_id, attempt_id) = fixture.seed_lineage();
    let backend = Arc::new(RecordingBackend::new());
    let mut runner = runner_with(
        &fixture,
        Arc::clone(&backend) as Arc<dyn runner::IsolationBackend>,
        Arc::new(|_| Err("not found".to_owned())),
    );
    let error = runner
        .execute(request(
            &fixture,
            cargo_v1(),
            &task_id,
            &execution_id,
            &attempt_id,
        ))
        .expect_err("unresolved executable must refuse");
    assert!(matches!(
        error,
        VerificationExecutionError::ExecutableUnresolved
    ));
    assert_eq!(backend.enforcements.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.effect_count(), 0);
}
