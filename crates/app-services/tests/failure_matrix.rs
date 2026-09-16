//! G3 end-to-end failure matrix (TASK-0038).
//!
//! Deterministic coverage for all eleven canonical FakeAgent scenarios
//! through the trusted path:
//!
//! ```text
//! Task → OrchestrationService::schedule → Execution → Attempt
//! → FakeAgent → conclude_attempt → submit/verify → Task state
//! ```
//!
//! Authority: `docs/architecture/fake-agent.md` (scenario identities),
//! `docs/architecture/task-engine.md` §§5–7/12–13 (lifecycle, Attempt
//! failure != Task failure, completion authority), `providers.md` §§9–10
//! (availability/failure vocabulary), ADR-0018 (independent evaluation),
//! ADR-0022 (three runtime events, four outcomes, service-minted
//! provenance). No new TaskState, EventType, AttemptOutcome, migration,
//! IPC, provider, runner, Git, policy, or G8 evaluator is introduced by
//! any test below.
//!
//! Mapping under test (see `docs/development/G3_FAILURE_MATRIX.md`):
//!
//! ```text
//! success (+Verified)              → Succeeded → COMPLETED
//! success/review_rejection (+Rejected) → Succeeded → RETRY_READY
//! crash/quota/network/forbidden/malformed/partial → Failed → RUNNING
//! timeout                           → TimedOut  → RUNNING
//! hang (cancelled)                  → Cancelled → RUNNING
//! hang (released)                   → Succeeded → verify path
//! ```
//!
//! Every database file is test-owned; every identity is deterministic.
//! Hang tests never block the suite: pre-decided controls return
//! immediately, and the one threaded regain-control test bounds every
//! wait with `recv_timeout`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use agent_runtime::{AttemptId, AttemptOutcome, ExecutionId, ProviderRef};
use app_services::{
    EvaluationPort, ExecutionPort, ExecutionProblem, FixedClock, OrchestrationIds,
    OrchestrationOutcome, OrchestrationService, RuntimeService, TaskService, WorkEvidence,
    WorkItem,
};
use fake_agent::{ExecutionControl, ExecutionRequest, FakeAgent, FakeScenario};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use storage_sqlite::EventType;
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-failure-matrix-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
    }

    fn orchestration(&self) -> OrchestrationService<FixedClock> {
        OrchestrationService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open service")
    }

    fn tasks(&self) -> TaskService<FixedClock> {
        TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open task service")
    }

    fn runtime(&self) -> RuntimeService<FixedClock> {
        RuntimeService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open runtime service")
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

/// Test-side executor: one FakeAgent scenario behind the provider-neutral
/// [`ExecutionPort`]. Adapter failures normalize at this boundary by
/// matching on `FakeAgentError` variants — never on scenario names — and
/// invocations are counted so blocked paths prove the executor stayed
/// silent.
struct FakeExecutor {
    agent: FakeAgent,
    control: ExecutionControl,
    calls: AtomicUsize,
}

impl FakeExecutor {
    fn for_scenario(scenario: FakeScenario) -> Self {
        Self::with_control(scenario, ExecutionControl::fresh())
    }

    fn with_control(scenario: FakeScenario, control: ExecutionControl) -> Self {
        Self {
            agent: FakeAgent::for_scenario(scenario),
            control,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ExecutionPort for FakeExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new(fake_agent::FAKE_PROVIDER_REF).unwrap()
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let request = ExecutionRequest::new(
            work.task_id.clone(),
            work.execution_id.clone(),
            work.attempt_id.clone(),
        );
        match self.agent.execute(&request, &self.control) {
            Ok(output) => Ok(WorkEvidence {
                summary: output.summary().to_owned(),
                evidence_ref: output.evidence_ref().to_owned(),
                partial: output.is_partial(),
            }),
            Err(fake_agent::FakeAgentError::TimedOut { budget }) => {
                Err(ExecutionProblem::TimedOut { detail: budget })
            }
            Err(fake_agent::FakeAgentError::Cancelled) => Err(ExecutionProblem::Cancelled),
            Err(other) => Err(ExecutionProblem::Failed {
                detail: other.to_string(),
            }),
        }
    }
}

fn seed_ready(service: &mut OrchestrationService<FixedClock>, project: &str, id: &str) -> TaskId {
    if service
        .tasks()
        .get_project(&ProjectId::new(project).unwrap())
        .unwrap()
        .is_none()
    {
        service
            .tasks_mut()
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
    service.tasks_mut().create_task(&task).unwrap();
    service.tasks_mut().mark_ready(&task.id().clone()).unwrap();
    task.id().clone()
}

fn ids(execution: &str, attempt: &str) -> OrchestrationIds {
    OrchestrationIds {
        execution_id: ExecutionId::new(execution).unwrap(),
        attempt_id: AttemptId::new(attempt).unwrap(),
    }
}

fn task_state(service: &OrchestrationService<FixedClock>, task_id: &TaskId) -> TaskState {
    service.tasks().get_task(task_id).unwrap().unwrap().state()
}

fn attempt_outcome(
    service: &OrchestrationService<FixedClock>,
    attempt_id: &AttemptId,
) -> Option<AttemptOutcome> {
    service
        .runtime()
        .get_attempt(attempt_id)
        .unwrap()
        .unwrap()
        .outcome()
}

fn event_types(service: &OrchestrationService<FixedClock>, task_id: &TaskId) -> Vec<EventType> {
    service
        .tasks()
        .list_events_for_task(task_id)
        .unwrap()
        .iter()
        .map(|event| event.fact.event_type)
        .collect()
}

fn count_runtime_events(
    service: &OrchestrationService<FixedClock>,
    task_id: &TaskId,
) -> (usize, usize, usize) {
    let types = event_types(service, task_id);
    let created_exec = types
        .iter()
        .filter(|t| **t == EventType::ExecutionCreated)
        .count();
    let created_attempt = types
        .iter()
        .filter(|t| **t == EventType::AttemptCreated)
        .count();
    let concluded = types
        .iter()
        .filter(|t| **t == EventType::AttemptConcluded)
        .count();
    (created_exec, created_attempt, concluded)
}

/// Schedules one scenario end to end and returns the outcome. The caller
/// owns assertions; this helper only removes setup repetition.
fn run_scenario(
    service: &mut OrchestrationService<FixedClock>,
    task_id: &TaskId,
    scenario: FakeScenario,
    control: ExecutionControl,
    evaluator: &FakeEvaluator,
    execution: &str,
    attempt: &str,
) -> OrchestrationOutcome {
    let executor = FakeExecutor::with_control(scenario, control);
    let outcome = service
        .schedule(task_id, ids(execution, attempt), &executor, evaluator)
        .expect("schedule must return a typed outcome");
    // The executor is dropped here; invocation counts are asserted by the
    // caller through the evaluator and durable state instead.
    let _ = executor.call_count();
    outcome
}

// -- 1. success ---------------------------------------------------------------

#[test]
fn success_verified_end_to_end_is_exactly_once_and_survives_reopen() {
    let db = TestDb::new("success");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let executor = FakeExecutor::for_scenario(FakeScenario::Success);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededAndVerified {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    );
    assert_eq!(executor.call_count(), 1);
    assert_eq!(evaluator.call_count(), 1);
    assert_eq!(task_state(&service, &task_id), TaskState::Completed);
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-1").unwrap()),
        Some(AttemptOutcome::Succeeded)
    );
    // Exactly one of each runtime fact; no duplicate submission.
    assert_eq!(count_runtime_events(&service, &task_id), (1, 1, 1));
    let types = event_types(&service, &task_id);
    assert_eq!(
        types,
        [
            EventType::TaskCreated,
            EventType::MarkedReady,
            EventType::Claimed,
            EventType::Started,
            EventType::ExecutionCreated,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
            EventType::Submitted,
            EventType::RoutedToReview,
            EventType::VerificationBegan,
            EventType::VerificationSucceeded,
        ]
    );
    drop(service);

    // Reopen: relational state is authoritative, read from canonical owners.
    let tasks = db.tasks();
    let runtime = db.runtime();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Completed
    );
    let execution = runtime
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(execution.task_id(), &task_id);
    assert_eq!(execution.attempts().len(), 1);
    assert_eq!(
        runtime
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Succeeded)
    );
}

// -- 2. review_rejection -------------------------------------------------------

#[test]
fn review_rejection_keeps_succeeded_attempt_and_never_completes_on_fixture() {
    // Rejected verdict: RETRY_READY, attempt stays Succeeded.
    let db = TestDb::new("review-rejected");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::ReviewRejection);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysRejected);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededButRejected {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    );
    assert_eq!(evaluator.call_count(), 1);
    let task = service.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::RetryReady);
    assert_ne!(task.state(), TaskState::Completed);
    // Successful execution history is not rewritten as failure.
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-1").unwrap()),
        Some(AttemptOutcome::Succeeded)
    );
    drop(service);
    // Durable after reopen.
    let tasks = db.tasks();
    let runtime = db.runtime();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::RetryReady
    );
    assert_eq!(
        runtime
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Succeeded)
    );

    // Verified verdict on the same fixture: the verdict — not the fixture
    // marker — decides. FakeAgent never reviews.
    let db2 = TestDb::new("review-verified");
    let mut service2 = db2.orchestration();
    let task2 = seed_ready(&mut service2, "P-1", "T-1");
    let outcome2 = service2
        .schedule(
            &task2,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::ReviewRejection),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert!(matches!(
        outcome2,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
    assert_eq!(task_state(&service2, &task2), TaskState::Completed);
}

// -- 3. crash -----------------------------------------------------------------

#[test]
fn crash_records_failed_attempt_with_no_evaluation_and_no_completion() {
    let db = TestDb::new("crash");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::Crash);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionFailed {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            outcome: AttemptOutcome::Failed,
        }
    );
    // No real process crashed (the suite is alive), no submission, no
    // verdict sought even though the evaluator would verify anything.
    assert_eq!(executor.call_count(), 1);
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-1").unwrap()),
        Some(AttemptOutcome::Failed)
    );
    let detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();
    assert!(
        detail.contains("no process"),
        "crash simulation must stay a simulation: {detail}"
    );
    assert_eq!(count_runtime_events(&service, &task_id), (1, 1, 1));
    let types = event_types(&service, &task_id);
    assert!(!types.contains(&EventType::Submitted));
    assert!(!types.contains(&EventType::VerificationSucceeded));
}

// -- 4. timeout ----------------------------------------------------------------

#[test]
fn timeout_records_timed_out_with_no_retry_and_no_evaluation() {
    let db = TestDb::new("timeout");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::Timeout);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionFailed {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            outcome: AttemptOutcome::TimedOut,
        }
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-1").unwrap()),
        Some(AttemptOutcome::TimedOut)
    );
    // Exactly one terminal attempt: no automatic Attempt 2, no sleep.
    let execution = service
        .runtime()
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(execution.attempts().len(), 1);
    drop(service);
    // Durable after reopen.
    assert_eq!(
        db.tasks().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );
    assert_eq!(
        db.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::TimedOut)
    );
}

// -- 5. hang -------------------------------------------------------------------

#[test]
fn hang_precancelled_is_cancelled_without_second_attempt() {
    let db = TestDb::new("hang-cancelled");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let control = ExecutionControl::fresh();
    control.cancel();
    let executor = FakeExecutor::with_control(FakeScenario::Hang, control);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::Cancelled {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-1").unwrap()),
        Some(AttemptOutcome::Cancelled)
    );
    assert_eq!(count_runtime_events(&service, &task_id), (1, 1, 1));
    drop(service);
    assert_eq!(
        db.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Cancelled)
    );
}

#[test]
fn hang_release_becomes_submittable_evidence_not_a_failure() {
    let db = TestDb::new("hang-released");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let control = ExecutionControl::fresh();
    control.release();
    let executor = FakeExecutor::with_control(FakeScenario::Hang, control);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        ),
        "released hang is complete evidence: {outcome:?}"
    );
    assert_eq!(evaluator.call_count(), 1);
    assert_eq!(task_state(&service, &task_id), TaskState::Completed);
}

#[test]
fn hang_threaded_cancel_regains_orchestrator_control_with_no_mega_transaction() {
    // Full regain-control proof: the orchestrator blocks inside hang while
    // a second connection observes the durable intermediate state (RUNNING
    // + open Attempt, no attempt.concluded yet) — so no giant transaction
    // spans FakeAgent execution — then cancellation releases it.
    let db = TestDb::new("hang-threaded");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let control = ExecutionControl::fresh();
    let worker_control = control.clone();
    let (tx, rx) = mpsc::channel();
    let task_clone = task_id.clone();
    let handle = std::thread::spawn(move || {
        let executor = FakeExecutor::with_control(FakeScenario::Hang, worker_control);
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        let outcome = service.schedule(&task_clone, ids("E-1", "A-1"), &executor, &evaluator);
        let _ = tx.send(outcome);
    });

    // Wait until the worker has committed Execution + open Attempt (poll a
    // second connection; 5s bound so a regression fails, never hangs).
    let probe_tasks = db.tasks();
    let probe_runtime = db.runtime();
    let execution_id = ExecutionId::new("E-1").unwrap();
    let attempt_id = AttemptId::new("A-1").unwrap();
    let mut observed_open = false;
    for _ in 0..500 {
        if let Some(execution) = probe_runtime.get_execution(&execution_id).unwrap() {
            assert_eq!(execution.task_id(), &task_id);
            if let Some(attempt) = execution.get_attempt(&attempt_id) {
                if attempt.is_open() {
                    // Intermediate is durably inspectable from another
                    // connection while the executor is still blocked.
                    assert_eq!(
                        probe_tasks.get_task(&task_id).unwrap().unwrap().state(),
                        TaskState::Running
                    );
                    let types: Vec<EventType> = probe_tasks
                        .list_events_for_task(&task_id)
                        .unwrap()
                        .iter()
                        .map(|event| event.fact.event_type)
                        .collect();
                    assert!(types.contains(&EventType::ExecutionCreated));
                    assert!(types.contains(&EventType::AttemptCreated));
                    assert!(!types.contains(&EventType::AttemptConcluded));
                    observed_open = true;
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        observed_open,
        "worker must reach open Attempt while blocked"
    );

    // Regain control: explicit cancellation releases the blocked execution.
    control.cancel();
    let outcome = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("cancelled hang must conclude promptly")
        .expect("schedule must return a typed outcome");
    assert_eq!(
        outcome,
        OrchestrationOutcome::Cancelled {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    );
    handle.join().expect("worker thread must exit after cancel");

    // Exactly one Attempt was ever created; no automatic second try.
    let check = db.runtime();
    let execution = check.get_execution(&execution_id).unwrap().unwrap();
    assert_eq!(execution.attempts().len(), 1);
    assert_eq!(
        check.get_attempt(&attempt_id).unwrap().unwrap().outcome(),
        Some(AttemptOutcome::Cancelled)
    );
    assert_eq!(
        db.tasks().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );
}

// -- 6. malformed_result --------------------------------------------------------

#[test]
fn malformed_result_is_rejected_at_the_boundary_and_cannot_be_blessed() {
    let db = TestDb::new("malformed");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    // Even an always-verifying evaluator must never see malformed output
    // as trusted work.
    let executor = FakeExecutor::for_scenario(FakeScenario::MalformedResult);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionFailed {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            outcome: AttemptOutcome::Failed,
        }
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    let types = event_types(&service, &task_id);
    assert!(!types.contains(&EventType::Submitted));
    assert!(!types.contains(&EventType::VerificationSucceeded));
    assert!(!types.contains(&EventType::VerificationFailed));
}

// -- 7. partial_result ----------------------------------------------------------

#[test]
fn partial_result_stays_failed_and_distinguishable_from_success() {
    let db = TestDb::new("partial");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::PartialResult);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                outcome: AttemptOutcome::Failed,
                ..
            }
        ),
        "partial_result != success: {outcome:?}"
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    let detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();
    assert!(
        detail.contains("partial"),
        "partial evidence must stay observable: {detail}"
    );
}

// -- 8. quota_reached ------------------------------------------------------------

#[test]
fn quota_reached_performs_no_retry_no_routing_and_no_completion() {
    let db = TestDb::new("quota");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::QuotaReached);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                outcome: AttemptOutcome::Failed,
                ..
            }
        ),
        "quota exhaustion is Attempt Failed, never Task failure: {outcome:?}"
    );
    // No retry (exactly one attempt), no provider switching (one opaque
    // provider ref), no evaluator, no completion.
    let execution = service
        .runtime()
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(execution.attempts().len(), 1);
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    let detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();
    assert!(
        detail.contains("quota"),
        "quota evidence must survive in the detail: {detail}"
    );
    // Quota is per-execution simulation state: an independent success on
    // another task is unaffected (no cross-execution quota latch).
    let second = seed_ready(&mut service, "P-1", "T-2");
    let outcome2 = service
        .schedule(
            &second,
            ids("E-2", "A-2"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert!(matches!(
        outcome2,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
}

// -- 9. network_failure ----------------------------------------------------------

#[test]
fn network_failure_touches_no_network_and_never_replays() {
    let db = TestDb::new("network");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::NetworkFailure);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                outcome: AttemptOutcome::Failed,
                ..
            }
        ),
        "network failure is Attempt Failed: {outcome:?}"
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    let detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();
    assert!(
        detail.contains("no connection"),
        "network simulation must open nothing: {detail}"
    );
    assert_eq!(count_runtime_events(&service, &task_id), (1, 1, 1));
}

// -- 10/11. forbidden scenarios --------------------------------------------------

#[test]
fn forbidden_path_write_mutates_no_filesystem_state() {
    let db = TestDb::new("forbidden-path");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::ForbiddenPathWrite);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                outcome: AttemptOutcome::Failed,
                ..
            }
        ),
        "forbidden path is Attempt Failed, not G6 enforcement: {outcome:?}"
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    // Simulation only: the forbidden path is never created.
    assert!(
        !std::path::Path::new("/forbidden/fake-agent-write").exists(),
        "forbidden simulation must not touch the filesystem"
    );
    let detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();
    assert!(detail.contains("not performed"), "detail: {detail}");
}

#[test]
fn forbidden_command_launches_no_process() {
    let db = TestDb::new("forbidden-command");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let executor = FakeExecutor::for_scenario(FakeScenario::ForbiddenCommand);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                outcome: AttemptOutcome::Failed,
                ..
            }
        ),
        "forbidden command is Attempt Failed, not execution: {outcome:?}"
    );
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    let detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();
    // The command text is evidence only; no shell ever ran (the suite is
    // intact and the detail marks the attempt as not performed).
    assert!(detail.contains("not performed"), "detail: {detail}");
    assert!(detail.contains("exec"), "detail: {detail}");
}

// -- evaluation eligibility -----------------------------------------------------

#[test]
fn evaluator_runs_only_for_eligible_complete_evidence() {
    // Submittable family: exactly one verdict. Everything else: zero.
    let submittable = [FakeScenario::Success, FakeScenario::ReviewRejection];
    for scenario in submittable {
        let db = TestDb::new("eligible");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        service
            .schedule(
                &task_id,
                ids("E-1", "A-1"),
                &FakeExecutor::for_scenario(scenario),
                &evaluator,
            )
            .unwrap();
        assert_eq!(
            evaluator.call_count(),
            1,
            "scenario {} must reach the evaluator once",
            scenario.as_str()
        );
    }

    let blocked_from_evaluation = [
        FakeScenario::Crash,
        FakeScenario::Timeout,
        FakeScenario::MalformedResult,
        FakeScenario::ForbiddenPathWrite,
        FakeScenario::ForbiddenCommand,
        FakeScenario::QuotaReached,
        FakeScenario::NetworkFailure,
        FakeScenario::PartialResult,
    ];
    for scenario in blocked_from_evaluation {
        let db = TestDb::new("ineligible");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        service
            .schedule(
                &task_id,
                ids("E-1", "A-1"),
                &FakeExecutor::for_scenario(scenario),
                &evaluator,
            )
            .unwrap();
        assert_eq!(
            evaluator.call_count(),
            0,
            "scenario {} must never reach the evaluator",
            scenario.as_str()
        );
        assert_ne!(
            task_state(&service, &task_id),
            TaskState::Completed,
            "scenario {} must not complete",
            scenario.as_str()
        );
    }

    // Hang-cancelled is likewise ineligible (pre-cancelled control keeps
    // the test bounded).
    let db = TestDb::new("ineligible-hang");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let control = ExecutionControl::fresh();
    control.cancel();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &FakeExecutor::with_control(FakeScenario::Hang, control),
            &evaluator,
        )
        .unwrap();
    assert_eq!(evaluator.call_count(), 0);
}

#[test]
fn always_verified_evaluator_cannot_bless_absent_or_malformed_work() {
    // The trusted layer enforces eligibility before applying any verdict:
    // an eager evaluator converts nothing into completion.
    for scenario in [
        FakeScenario::Crash,
        FakeScenario::Timeout,
        FakeScenario::MalformedResult,
        FakeScenario::ForbiddenPathWrite,
        FakeScenario::ForbiddenCommand,
        FakeScenario::QuotaReached,
        FakeScenario::NetworkFailure,
        FakeScenario::PartialResult,
    ] {
        let db = TestDb::new("bless-check");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        service
            .schedule(
                &task_id,
                ids("E-1", "A-1"),
                &FakeExecutor::for_scenario(scenario),
                &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            )
            .unwrap();
        assert_ne!(
            task_state(&service, &task_id),
            TaskState::Completed,
            "always-verified must not bless {}",
            scenario.as_str()
        );
        let types = event_types(&service, &task_id);
        assert!(
            !types.contains(&EventType::VerificationSucceeded),
            "no verified verdict for {}",
            scenario.as_str()
        );
    }
}

// -- completion authority --------------------------------------------------------

#[test]
fn completion_requires_eligible_work_plus_verdict_plus_trusted_transition() {
    let db = TestDb::new("authority");
    let mut tasks = db.tasks();
    let mut runtime = db.runtime();

    tasks
        .create_project(&Project::new(ProjectId::new("P-1").unwrap(), "project P-1").unwrap())
        .unwrap();
    let draft = Task::draft(
        TaskId::new("T-1").unwrap(),
        ProjectId::new("P-1").unwrap(),
        "objective T-1",
    )
    .unwrap();
    tasks.create_task(&draft).unwrap();
    let task_id = draft.id().clone();
    tasks.mark_ready(&task_id).unwrap();
    tasks.claim(&task_id).unwrap();
    tasks.start(&task_id).unwrap();
    runtime
        .create_execution(ExecutionId::new("E-1").unwrap(), task_id.clone())
        .unwrap();
    runtime
        .create_attempt(
            &ExecutionId::new("E-1").unwrap(),
            AttemptId::new("A-1").unwrap(),
            ProviderRef::new(fake_agent::FAKE_PROVIDER_REF).unwrap(),
        )
        .unwrap();

    // FakeAgent success alone != COMPLETED.
    let output = FakeAgent::for_scenario(FakeScenario::Success)
        .execute(
            &ExecutionRequest::new(
                task_id.clone(),
                ExecutionId::new("E-1").unwrap(),
                AttemptId::new("A-1").unwrap(),
            ),
            &ExecutionControl::fresh(),
        )
        .unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );

    // Attempt Succeeded alone != COMPLETED.
    runtime
        .conclude_attempt(
            &ExecutionId::new("E-1").unwrap(),
            &AttemptId::new("A-1").unwrap(),
            AttemptOutcome::Succeeded,
            output.summary(),
        )
        .unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );

    // FakeEvaluator object existence != COMPLETED.
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    assert_eq!(evaluator.call_count(), 0);
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );

    // submit_result alone != COMPLETED.
    tasks.submit_result(&task_id, output.summary()).unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Submitted
    );

    // VERIFYING alone != COMPLETED.
    tasks.route_to_review(&task_id).unwrap();
    tasks.begin_verification(&task_id).unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying
    );

    // Verified verdict not applied through the trusted path != COMPLETED.
    let verdict = evaluator
        .evaluate(&app_services::EvaluationRequest {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            summary: output.summary().to_owned(),
            evidence_ref: output.evidence_ref().to_owned(),
        })
        .unwrap();
    assert_eq!(verdict, app_services::EvaluationVerdict::Verified);
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying,
        "a verdict changes nothing until the trusted transition commits it"
    );

    // Only eligible work + Verified + trusted transition completes.
    tasks
        .verification_succeeded(&task_id, "VERIFIED via independent evaluation")
        .unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Completed
    );
}

// -- repeat invocation safety ----------------------------------------------------

#[test]
fn terminal_state_resists_blind_second_execution() {
    // COMPLETED second schedule creates nothing.
    let db = TestDb::new("repeat-completed");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    let events_before = service
        .tasks()
        .list_events_for_task(&task_id)
        .unwrap()
        .len();
    let second = service
        .schedule(
            &task_id,
            ids("E-2", "A-2"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert_eq!(
        second,
        OrchestrationOutcome::NotRunnable {
            task_id: task_id.clone(),
            state: TaskState::Completed,
        }
    );
    assert!(service
        .runtime()
        .get_execution(&ExecutionId::new("E-2").unwrap())
        .unwrap()
        .is_none());
    assert!(service
        .runtime()
        .get_attempt(&AttemptId::new("A-2").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(
        service
            .tasks()
            .list_events_for_task(&task_id)
            .unwrap()
            .len(),
        events_before,
        "repeat invocation appends no events"
    );

    // Failed RUNNING second schedule creates nothing (no hidden retry).
    let db2 = TestDb::new("repeat-failed");
    let mut service2 = db2.orchestration();
    let failed = seed_ready(&mut service2, "P-1", "T-1");
    service2
        .schedule(
            &failed,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Crash),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert_eq!(task_state(&service2, &failed), TaskState::Running);
    let events_before2 = service2
        .tasks()
        .list_events_for_task(&failed)
        .unwrap()
        .len();
    let retry = service2
        .schedule(
            &failed,
            ids("E-2", "A-2"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert_eq!(
        retry,
        OrchestrationOutcome::NotRunnable {
            task_id: failed.clone(),
            state: TaskState::Running,
        },
        "failure does not auto-retry: RUNNING is not runnable"
    );
    assert!(service2
        .runtime()
        .get_execution(&ExecutionId::new("E-2").unwrap())
        .unwrap()
        .is_none());
    assert_eq!(
        service2
            .tasks()
            .list_events_for_task(&failed)
            .unwrap()
            .len(),
        events_before2
    );

    // RETRY_READY second schedule via the scheduler creates nothing: there
    // is no scheduler retry entry point (explicit recovery uses the
    // Task/Runtime services directly; see the explicit-retry test).
    let db3 = TestDb::new("repeat-retry-ready");
    let mut service3 = db3.orchestration();
    let rejected = seed_ready(&mut service3, "P-1", "T-1");
    service3
        .schedule(
            &rejected,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysRejected),
        )
        .unwrap();
    assert_eq!(task_state(&service3, &rejected), TaskState::RetryReady);
    let again = service3
        .schedule(
            &rejected,
            ids("E-2", "A-2"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert_eq!(
        again,
        OrchestrationOutcome::NotRunnable {
            task_id: rejected.clone(),
            state: TaskState::RetryReady,
        }
    );
    assert!(service3
        .runtime()
        .get_execution(&ExecutionId::new("E-2").unwrap())
        .unwrap()
        .is_none());
}

// -- explicit retry ---------------------------------------------------------------

#[test]
fn explicit_retry_from_retry_ready_creates_a_new_attempt_and_preserves_history() {
    let db = TestDb::new("explicit-retry");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    // Attempt A1: valid evidence, evaluation rejects → RETRY_READY.
    service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysRejected),
        )
        .unwrap();
    assert_eq!(task_state(&service, &task_id), TaskState::RetryReady);
    let a1_detail = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap()
        .detail()
        .unwrap()
        .to_owned();

    // Explicit bounded recovery through the existing canonical APIs only:
    // RETRY_READY → RUNNING, then a new Execution/Attempt (never a reuse
    // of A1), then the normal submit → verify path.
    service.tasks_mut().start(&task_id).unwrap();
    assert_eq!(task_state(&service, &task_id), TaskState::Running);
    service
        .runtime_mut()
        .create_execution(ExecutionId::new("E-2").unwrap(), task_id.clone())
        .unwrap();
    service
        .runtime_mut()
        .create_attempt(
            &ExecutionId::new("E-2").unwrap(),
            AttemptId::new("A-2").unwrap(),
            ProviderRef::new(fake_agent::FAKE_PROVIDER_REF).unwrap(),
        )
        .unwrap();
    let output = FakeAgent::for_scenario(FakeScenario::Success)
        .execute(
            &ExecutionRequest::new(
                task_id.clone(),
                ExecutionId::new("E-2").unwrap(),
                AttemptId::new("A-2").unwrap(),
            ),
            &ExecutionControl::fresh(),
        )
        .unwrap();
    service
        .runtime_mut()
        .conclude_attempt(
            &ExecutionId::new("E-2").unwrap(),
            &AttemptId::new("A-2").unwrap(),
            AttemptOutcome::Succeeded,
            output.summary(),
        )
        .unwrap();
    service
        .tasks_mut()
        .submit_result(&task_id, output.summary())
        .unwrap();
    service.tasks_mut().route_to_review(&task_id).unwrap();
    service.tasks_mut().begin_verification(&task_id).unwrap();
    let verdict = FakeEvaluator::new(EvaluatorMode::AlwaysVerified)
        .evaluate(&app_services::EvaluationRequest {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-2").unwrap(),
            attempt_id: AttemptId::new("A-2").unwrap(),
            summary: output.summary().to_owned(),
            evidence_ref: output.evidence_ref().to_owned(),
        })
        .unwrap();
    assert_eq!(verdict, app_services::EvaluationVerdict::Verified);
    service
        .tasks_mut()
        .verification_succeeded(&task_id, "VERIFIED via independent evaluation")
        .unwrap();
    assert_eq!(task_state(&service, &task_id), TaskState::Completed);

    // History: A1 preserved untouched, A2 separate — never a replay of the
    // same side effects (evidence refs carry distinct identities).
    let a1 = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    let a2 = service
        .runtime()
        .get_attempt(&AttemptId::new("A-2").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(a1.outcome(), Some(AttemptOutcome::Succeeded));
    assert_eq!(a1.detail().unwrap(), a1_detail);
    assert_eq!(a2.outcome(), Some(AttemptOutcome::Succeeded));
    assert_ne!(a1.id(), a2.id());
    assert_eq!(a1.execution_id().as_str(), "E-1");
    assert_eq!(a2.execution_id().as_str(), "E-2");
    // Two executions, two attempts, two conclusions — no hidden extra work.
    assert_eq!(count_runtime_events(&service, &task_id), (2, 2, 2));
}

// -- durable reopen matrix ----------------------------------------------------------

#[test]
fn durable_reopen_matrix_for_each_outcome_class() {
    // Verified success.
    let db = TestDb::new("reopen-verified");
    let mut service = db.orchestration();
    let verified = seed_ready(&mut service, "P-1", "T-1");
    service
        .schedule(
            &verified,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    drop(service);
    assert_eq!(
        db.tasks().get_task(&verified).unwrap().unwrap().state(),
        TaskState::Completed
    );
    assert_eq!(
        db.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Succeeded)
    );

    // Review rejection.
    let db2 = TestDb::new("reopen-rejected");
    let mut service2 = db2.orchestration();
    let rejected = seed_ready(&mut service2, "P-1", "T-1");
    service2
        .schedule(
            &rejected,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysRejected),
        )
        .unwrap();
    drop(service2);
    assert_eq!(
        db2.tasks().get_task(&rejected).unwrap().unwrap().state(),
        TaskState::RetryReady
    );
    assert_eq!(
        db2.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Succeeded)
    );

    // Runtime Failed.
    let db3 = TestDb::new("reopen-failed");
    let mut service3 = db3.orchestration();
    let failed = seed_ready(&mut service3, "P-1", "T-1");
    service3
        .schedule(
            &failed,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Crash),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    drop(service3);
    assert_eq!(
        db3.tasks().get_task(&failed).unwrap().unwrap().state(),
        TaskState::Running
    );
    assert_eq!(
        db3.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Failed)
    );

    // TimedOut.
    let db4 = TestDb::new("reopen-timedout");
    let mut service4 = db4.orchestration();
    let timed_out = seed_ready(&mut service4, "P-1", "T-1");
    service4
        .schedule(
            &timed_out,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Timeout),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    drop(service4);
    assert_eq!(
        db4.tasks().get_task(&timed_out).unwrap().unwrap().state(),
        TaskState::Running
    );
    assert_eq!(
        db4.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::TimedOut)
    );

    // Cancelled.
    let db5 = TestDb::new("reopen-cancelled");
    let mut service5 = db5.orchestration();
    let cancelled = seed_ready(&mut service5, "P-1", "T-1");
    let control = ExecutionControl::fresh();
    control.cancel();
    service5
        .schedule(
            &cancelled,
            ids("E-1", "A-1"),
            &FakeExecutor::with_control(FakeScenario::Hang, control),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    drop(service5);
    assert_eq!(
        db5.tasks().get_task(&cancelled).unwrap().unwrap().state(),
        TaskState::Running
    );
    assert_eq!(
        db5.runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Cancelled)
    );
}

// -- event journal ---------------------------------------------------------------------

#[test]
fn event_journal_agrees_with_relational_state_and_reads_create_nothing() {
    // Failed path: 7 canonical events, runtime scope correct, trusted
    // provenance throughout.
    let db = TestDb::new("journal-failed");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Crash),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    let history = service.tasks().list_events_for_task(&task_id).unwrap();
    let types: Vec<EventType> = history.iter().map(|event| event.fact.event_type).collect();
    assert_eq!(
        types,
        [
            EventType::TaskCreated,
            EventType::MarkedReady,
            EventType::Claimed,
            EventType::Started,
            EventType::ExecutionCreated,
            EventType::AttemptCreated,
            EventType::AttemptConcluded,
        ]
    );
    for event in &history {
        assert!(
            event
                .fact
                .provenance
                .starts_with("task-application-service/")
                || event
                    .fact
                    .provenance
                    .starts_with("runtime-application-service/"),
            "trusted provenance only: {}",
            event.fact.provenance
        );
        // Task lifecycle columns stay Task-only on runtime facts.
        if matches!(
            event.fact.event_type,
            EventType::ExecutionCreated | EventType::AttemptCreated | EventType::AttemptConcluded
        ) {
            assert!(event.fact.from_state.is_none());
            assert!(event.fact.to_state.is_none());
            assert!(event.fact.execution_id.is_some());
        }
        if event.fact.event_type == EventType::AttemptConcluded {
            assert_eq!(
                event.fact.attempt_outcome,
                Some(storage_sqlite::AttemptOutcome::Failed)
            );
        }
    }
    // No evaluator-specific event type exists: the closed canonical codec
    // covers everything observed, including the independent G7 Plan fact.
    assert_eq!(EventType::all().len(), 18);

    // Reads create no events.
    let before = service
        .tasks()
        .list_events_for_task(&task_id)
        .unwrap()
        .len();
    let _ = service.tasks().get_task(&task_id).unwrap();
    let _ = service
        .runtime()
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap();
    let _ = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap();
    let _ = service.runtime().list_events_ordered().unwrap();
    assert_eq!(
        service
            .tasks()
            .list_events_for_task(&task_id)
            .unwrap()
            .len(),
        before,
        "reads must not append journal facts"
    );
}

// -- crash between phases -----------------------------------------------------------------

#[test]
fn crash_between_phases_leaves_valid_inspectable_state() {
    // Each breakpoint is driven through the existing trusted services only
    // (no new recovery logic): the state after an interruption must be
    // internally valid and inspectable, including after a DB reopen.
    let db = TestDb::new("crash-phases");
    let mut tasks = db.tasks();
    let mut runtime = db.runtime();
    tasks
        .create_project(&Project::new(ProjectId::new("P-1").unwrap(), "project P-1").unwrap())
        .unwrap();
    let draft = Task::draft(
        TaskId::new("T-1").unwrap(),
        ProjectId::new("P-1").unwrap(),
        "objective T-1",
    )
    .unwrap();
    tasks.create_task(&draft).unwrap();
    let task_id = draft.id().clone();
    tasks.mark_ready(&task_id).unwrap();

    // 1. After RUNNING, before Execution creation.
    tasks.claim(&task_id).unwrap();
    tasks.start(&task_id).unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );
    assert!(runtime
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_none());

    // 2. After Execution creation, before Attempt creation.
    runtime
        .create_execution(ExecutionId::new("E-1").unwrap(), task_id.clone())
        .unwrap();
    let execution = runtime
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(execution.task_id(), &task_id);
    assert!(execution.attempts().is_empty());
    assert!(execution.open_attempt().is_none());

    // 3. After Attempt creation, before FakeAgent result.
    runtime
        .create_attempt(
            &ExecutionId::new("E-1").unwrap(),
            AttemptId::new("A-1").unwrap(),
            ProviderRef::new(fake_agent::FAKE_PROVIDER_REF).unwrap(),
        )
        .unwrap();
    let attempt = runtime
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert!(attempt.is_open());
    assert_eq!(attempt.outcome(), None);

    // 4. After Attempt conclusion, before Task submission.
    runtime
        .conclude_attempt(
            &ExecutionId::new("E-1").unwrap(),
            &AttemptId::new("A-1").unwrap(),
            AttemptOutcome::Succeeded,
            "evidence for A-1",
        )
        .unwrap();
    assert_eq!(
        runtime
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap()
            .outcome(),
        Some(AttemptOutcome::Succeeded)
    );
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running,
        "conclusion alone submits nothing"
    );

    // 5. After submission, before evaluation.
    tasks.submit_result(&task_id, "evidence for A-1").unwrap();
    tasks.route_to_review(&task_id).unwrap();
    tasks.begin_verification(&task_id).unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying
    );
    drop(tasks);
    drop(runtime);

    // Every intermediate above was already durable: reopen and inspect.
    let tasks2 = db.tasks();
    let runtime2 = db.runtime();
    assert_eq!(
        tasks2.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying
    );
    let attempt2 = runtime2
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(attempt2.outcome(), Some(AttemptOutcome::Succeeded));
    // Journal agrees but is never the state source: state came from the
    // canonical owners above.
    let types: Vec<EventType> = tasks2
        .list_events_for_task(&task_id)
        .unwrap()
        .iter()
        .map(|event| event.fact.event_type)
        .collect();
    assert!(types.contains(&EventType::AttemptConcluded));
    assert!(types.contains(&EventType::VerificationBegan));
    assert!(!types.contains(&EventType::VerificationSucceeded));
}

// -- cancellation isolation ------------------------------------------------------------------

#[test]
fn cancellation_affects_only_the_intended_execution() {
    let db = TestDb::new("cancel-isolation");
    let mut service = db.orchestration();
    let task_a = seed_ready(&mut service, "P-1", "T-A");
    let task_b = seed_ready(&mut service, "P-1", "T-B");

    // Task A hangs and is cancelled; Task B succeeds independently. No
    // global FakeAgent cancellation state: separate controls, separate
    // executor/evaluator instances with per-instance counters.
    let control_a = ExecutionControl::fresh();
    control_a.cancel();
    let evaluator_a = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome_a = service
        .schedule(
            &task_a,
            ids("E-A", "A-A"),
            &FakeExecutor::with_control(FakeScenario::Hang, control_a),
            &evaluator_a,
        )
        .unwrap();
    assert!(matches!(outcome_a, OrchestrationOutcome::Cancelled { .. }));
    assert_eq!(evaluator_a.call_count(), 0);

    let evaluator_b = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome_b = service
        .schedule(
            &task_b,
            ids("E-B", "A-B"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &evaluator_b,
        )
        .unwrap();
    assert!(matches!(
        outcome_b,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
    assert_eq!(evaluator_b.call_count(), 1);

    assert_eq!(task_state(&service, &task_a), TaskState::Running);
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-A").unwrap()),
        Some(AttemptOutcome::Cancelled)
    );
    assert_eq!(task_state(&service, &task_b), TaskState::Completed);
    assert_eq!(
        attempt_outcome(&service, &AttemptId::new("A-B").unwrap()),
        Some(AttemptOutcome::Succeeded)
    );
}

// -- dependency blocking -----------------------------------------------------------------------

#[test]
fn unfinished_dependency_blocks_every_scenario_without_invocation() {
    for scenario in FakeScenario::all() {
        // `hang` would block forever if invoked; it is never invoked here
        // (readiness precedes the executor), so no control setup is needed.
        let db = TestDb::new("dependency-every");
        let mut service = db.orchestration();
        let blocked = seed_ready(&mut service, "P-1", "T-A");
        let prerequisite = seed_ready(&mut service, "P-1", "T-B");
        service
            .tasks_mut()
            .add_dependency(&blocked, &prerequisite)
            .unwrap();
        let executor = FakeExecutor::for_scenario(scenario);
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        let outcome = service
            .schedule(&blocked, ids("E-1", "A-1"), &executor, &evaluator)
            .unwrap();
        assert_eq!(
            outcome,
            OrchestrationOutcome::WaitingOnDependency {
                task_id: blocked.clone(),
                blocking: vec![prerequisite.clone()],
            },
            "scenario {} must not override readiness",
            scenario.as_str()
        );
        assert_eq!(executor.call_count(), 0);
        assert_eq!(evaluator.call_count(), 0);
        assert_eq!(task_state(&service, &blocked), TaskState::Ready);
        assert!(service
            .runtime()
            .get_execution(&ExecutionId::new("E-1").unwrap())
            .unwrap()
            .is_none());
        assert_eq!(count_runtime_events(&service, &blocked), (0, 0, 0));
    }
}

// -- invalid states -------------------------------------------------------------------------------

#[test]
fn non_ready_states_invoke_nothing_and_create_no_runtime_facts() {
    let db = TestDb::new("invalid-states");
    let mut service = db.orchestration();

    let draft_task = Task::draft(
        TaskId::new("T-DRAFT").unwrap(),
        ProjectId::new("P-1").unwrap(),
        "objective draft",
    )
    .unwrap();
    // Project must exist before task creation.
    service
        .tasks_mut()
        .create_project(&Project::new(ProjectId::new("P-1").unwrap(), "project P-1").unwrap())
        .unwrap();
    service.tasks_mut().create_task(&draft_task).unwrap();
    let draft = draft_task.id().clone();

    let claimed = seed_ready(&mut service, "P-1", "T-CLAIMED");
    service.tasks_mut().claim(&claimed).unwrap();
    let running = seed_ready(&mut service, "P-1", "T-RUNNING");
    service.tasks_mut().claim(&running).unwrap();
    service.tasks_mut().start(&running).unwrap();
    let blocked_task = seed_ready(&mut service, "P-1", "T-BLOCKED");
    service.tasks_mut().claim(&blocked_task).unwrap();
    service.tasks_mut().start(&blocked_task).unwrap();
    service
        .tasks_mut()
        .block(&blocked_task, "impediment")
        .unwrap();
    let submitted = seed_ready(&mut service, "P-1", "T-SUBMITTED");
    service.tasks_mut().claim(&submitted).unwrap();
    service.tasks_mut().start(&submitted).unwrap();
    service
        .tasks_mut()
        .submit_result(&submitted, "evidence")
        .unwrap();
    let awaiting = seed_ready(&mut service, "P-1", "T-AWAITING");
    service.tasks_mut().claim(&awaiting).unwrap();
    service.tasks_mut().start(&awaiting).unwrap();
    service
        .tasks_mut()
        .submit_result(&awaiting, "evidence")
        .unwrap();
    service.tasks_mut().route_to_review(&awaiting).unwrap();
    let verifying = seed_ready(&mut service, "P-1", "T-VERIFYING");
    service.tasks_mut().claim(&verifying).unwrap();
    service.tasks_mut().start(&verifying).unwrap();
    service
        .tasks_mut()
        .submit_result(&verifying, "evidence")
        .unwrap();
    service.tasks_mut().route_to_review(&verifying).unwrap();
    service.tasks_mut().begin_verification(&verifying).unwrap();
    let retry = seed_ready(&mut service, "P-1", "T-RETRY");
    service
        .schedule(
            &retry,
            ids("E-RETRY", "A-RETRY"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysRejected),
        )
        .unwrap();
    assert_eq!(task_state(&service, &retry), TaskState::RetryReady);
    let completed = seed_ready(&mut service, "P-1", "T-DONE");
    service
        .schedule(
            &completed,
            ids("E-DONE", "A-DONE"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        )
        .unwrap();
    assert_eq!(task_state(&service, &completed), TaskState::Completed);

    // FAILED/CANCELLED have no accepted incoming edge (task-engine.md §16)
    // so no service can construct them; the scheduler census unit test
    // covers those states purely. Every reachable non-READY state refuses.
    let cases = [
        (draft, TaskState::Draft),
        (claimed, TaskState::Claimed),
        (running, TaskState::Running),
        (blocked_task, TaskState::Blocked),
        (submitted, TaskState::Submitted),
        (awaiting, TaskState::AwaitingReview),
        (verifying, TaskState::Verifying),
        (retry, TaskState::RetryReady),
        (completed, TaskState::Completed),
    ];
    for (index, (task_id, state)) in cases.into_iter().enumerate() {
        let executor = FakeExecutor::for_scenario(FakeScenario::Success);
        let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
        let outcome = service
            .schedule(
                &task_id,
                ids(&format!("E-X{index}"), &format!("A-X{index}")),
                &executor,
                &evaluator,
            )
            .unwrap();
        assert_eq!(
            outcome,
            OrchestrationOutcome::NotRunnable {
                task_id: task_id.clone(),
                state,
            },
            "state {state} must refuse execution"
        );
        assert_eq!(executor.call_count(), 0);
        assert_eq!(evaluator.call_count(), 0);
        assert!(
            service
                .runtime()
                .get_execution(&ExecutionId::new(format!("E-X{index}")).unwrap())
                .unwrap()
                .is_none(),
            "state {state} must create no execution"
        );
    }
}

// -- durable taxonomy -------------------------------------------------------------------------------

#[test]
fn no_scenario_expands_the_durable_taxonomy() {
    // Closed vocabularies stay closed across the whole matrix.
    assert_eq!(TaskState::all().len(), 12);
    assert_eq!(EventType::all().len(), 18);
    assert_eq!(storage_sqlite::AttemptOutcome::all().len(), 4);
    assert_eq!(FakeScenario::all().len(), 11);

    // Every scenario end to end still journals only canonical types with
    // canonical outcomes — scenario names never become durable vocabulary.
    for scenario in FakeScenario::all() {
        let db = TestDb::new("taxonomy");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        let control = ExecutionControl::fresh();
        if scenario == FakeScenario::Hang {
            control.cancel();
        }
        service
            .schedule(
                &task_id,
                ids("E-1", "A-1"),
                &FakeExecutor::with_control(scenario, control),
                &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            )
            .unwrap();
        for event in service.tasks().list_events_for_task(&task_id).unwrap() {
            assert!(
                EventType::all().contains(&event.fact.event_type),
                "unknown event type after {}",
                scenario.as_str()
            );
            if let Some(outcome) = event.fact.attempt_outcome {
                assert!(
                    storage_sqlite::AttemptOutcome::all().contains(&outcome),
                    "unknown outcome after {}",
                    scenario.as_str()
                );
                assert!(
                    !FakeScenario::all()
                        .iter()
                        .any(|s| s.as_str() == outcome.as_str()),
                    "scenario name leaked into outcome after {}",
                    scenario.as_str()
                );
            }
        }
        // Schema version is asserted structurally by the storage adapter
        // (SUPPORTED_SCHEMA_VERSION); the closed workload-bearing migration
        // sequence through 0016 is proven by the storage census tests.
        assert_eq!(storage_sqlite::SUPPORTED_SCHEMA_VERSION, 16);
    }
}

// -- one schedule creates at most one attempt ----------------------------------------------------------

#[test]
fn one_orchestration_invocation_creates_at_most_one_attempt() {
    for scenario in FakeScenario::all() {
        let db = TestDb::new("at-most-one");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        let control = ExecutionControl::fresh();
        if scenario == FakeScenario::Hang {
            control.cancel();
        }
        // Hang-release is the only hang shape that submits; exercise the
        // cancelled shape here for the at-most-one bound.
        service
            .schedule(
                &task_id,
                ids("E-1", "A-1"),
                &FakeExecutor::with_control(scenario, control),
                &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            )
            .unwrap();
        let execution = service
            .runtime()
            .get_execution(&ExecutionId::new("E-1").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            execution.attempts().len(),
            1,
            "scenario {} must record exactly one attempt",
            scenario.as_str()
        );
        assert_eq!(count_runtime_events(&service, &task_id), (1, 1, 1));
    }
}

// -- unused-helper guard -------------------------------------------------------------------------------
// `run_scenario` documents the canonical schedule call shape for future
// matrix extensions; keep it referenced so intent stays compiled.
#[test]
fn schedule_helper_shape_matches_orchestration_contract() {
    let db = TestDb::new("helper-shape");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = run_scenario(
        &mut service,
        &task_id,
        FakeScenario::Success,
        ExecutionControl::fresh(),
        &evaluator,
        "E-1",
        "A-1",
    );
    assert!(matches!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
}
