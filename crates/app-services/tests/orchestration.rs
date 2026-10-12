//! Minimal scheduler + evaluation flow (TASK-0037, G3): the first complete
//! deterministic `Task → schedule → Execution → Attempt → FakeAgent →
//! submit → verify → COMPLETED | RETRY_READY` path.
//!
//! Composition under test:
//!
//! ```text
//! OrchestrationService (trusted sequencing only)
//!   → TaskService for every Task mutation
//!   → RuntimeService for every Execution/Attempt mutation
//!   → injected ExecutionPort (test-side FakeAgent wrapper here)
//!   → independent EvaluationPort (FakeEvaluator, never FakeAgent)
//! ```
//!
//! Every database file is test-owned; every identity is deterministic.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_runtime::{AttemptId, AttemptOutcome, ExecutionId, ProviderRef};
use app_services::{
    EvaluationPort, ExecutionPort, ExecutionProblem, FixedClock, OrchestrationIds,
    OrchestrationOutcome, OrchestrationService, RuntimeService, TaskService, WorkEvidence,
    WorkItem,
};
use fake_agent::{ExecutionControl, ExecutionRequest, FakeAgent, FakeScenario};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use storage_sqlite::EventType;
use task_domain::{EvidenceRequirement, Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicUsize = AtomicUsize::new(0);
const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-orchestration-test-{}-{suite}-{id}",
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

/// Test-side executor: wraps one `FakeAgent` scenario behind the
/// provider-neutral [`ExecutionPort`]. Normalizes adapter failures at this
/// boundary (matching on `FakeAgentError` variants, never on scenario
/// names) and counts invocations so blocked paths can prove the executor
/// was never called.
struct FakeExecutor {
    agent: FakeAgent,
    control: ExecutionControl,
    calls: AtomicUsize,
}

impl FakeExecutor {
    fn for_scenario(scenario: FakeScenario) -> Self {
        Self {
            agent: FakeAgent::for_scenario(scenario),
            control: ExecutionControl::fresh(),
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

/// Test-local stub proving orchestration is provider-neutral: no FakeAgent
/// involved at all.
struct CancelStub;

impl ExecutionPort for CancelStub {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("cancel-stub").unwrap()
    }

    fn execute(&self, _work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        Err(ExecutionProblem::Cancelled)
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

struct CompletionBoundaryEvaluator {
    db_path: PathBuf,
    revalidated: std::sync::atomic::AtomicBool,
}

impl EvaluationPort for CompletionBoundaryEvaluator {
    fn evaluate(
        &self,
        _request: &app_services::EvaluationRequest,
    ) -> evaluation::EvaluationResult<app_services::EvaluationVerdict> {
        Ok(app_services::EvaluationVerdict::Verified)
    }

    fn revalidate_before_completion(
        &self,
        request: &app_services::EvaluationRequest,
    ) -> evaluation::EvaluationResult<()> {
        let tasks = TaskService::open(&self.db_path, FixedClock(FIXED_MILLIS))
            .map_err(|error| evaluation::EvaluationError::new(error.to_string()))?;
        let task = tasks
            .get_task(&request.task_id)
            .map_err(|error| evaluation::EvaluationError::new(error.to_string()))?
            .ok_or_else(|| evaluation::EvaluationError::new("task disappeared"))?;
        if task.state() != TaskState::Verifying {
            return Err(evaluation::EvaluationError::new(
                "completion revalidation did not run while task was VERIFYING",
            ));
        }
        self.revalidated.store(true, Ordering::SeqCst);
        Ok(())
    }
}

fn is_runtime_event(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::ExecutionCreated | EventType::AttemptCreated | EventType::AttemptConcluded
    )
}

// -- verified path -----------------------------------------------------------

#[test]
fn verified_end_to_end_flow_survives_db_reopen() {
    let db = TestDb::new("verified-e2e");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let executor = FakeExecutor::for_scenario(FakeScenario::Success);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator, None)
        .expect("runnable task must schedule");
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededAndVerified {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            advisory: None,
        }
    );
    assert_eq!(executor.call_count(), 1);
    assert_eq!(evaluator.call_count(), 1);
    drop(service);

    // Reopen: every durable fact survives, from canonical owners only.
    let tasks = db.tasks();
    let runtime = db.runtime();
    let task = tasks.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Completed);
    let execution = runtime
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(execution.task_id(), &task_id);
    let attempt = runtime
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(attempt.outcome(), Some(AttemptOutcome::Succeeded));

    // Canonical history: full Task lifecycle plus the three ADR-0022
    // runtime facts, and nothing else. No event replay is used for state.
    let history = tasks.list_events_for_task(&task_id).unwrap();
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
            EventType::Submitted,
            EventType::RoutedToReview,
            EventType::VerificationBegan,
            EventType::VerificationSucceeded,
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
            "every fact is minted by a trusted service: {}",
            event.fact.provenance
        );
    }
}

#[test]
fn production_orchestration_revalidates_before_verification_succeeded() {
    let db = TestDb::new("completion-revalidation-order");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-COMPLETION", "T-COMPLETION");
    let evaluator = CompletionBoundaryEvaluator {
        db_path: db.path.clone(),
        revalidated: std::sync::atomic::AtomicBool::new(false),
    };

    let outcome = service
        .schedule(
            &task_id,
            ids("E-COMPLETION", "A-COMPLETION"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &evaluator,
            None,
        )
        .expect("verified production flow");
    assert!(matches!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));
    assert!(evaluator.revalidated.load(Ordering::SeqCst));
    assert_eq!(
        service.tasks().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Completed
    );
}

// -- rejected path -----------------------------------------------------------

#[test]
fn rejected_evaluation_preserves_succeeded_attempt_history() {
    let db = TestDb::new("rejected-e2e");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let executor = FakeExecutor::for_scenario(FakeScenario::Success);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysRejected);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator, None)
        .expect("runnable task must schedule");
    assert_eq!(
        outcome,
        OrchestrationOutcome::ExecutionSucceededButRejected {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
            advisory: None,
        }
    );
    drop(service);

    // Attempt Succeeded + Evaluation Rejected is valid: the Task is not
    // COMPLETED, and the successful attempt history is preserved.
    let tasks = db.tasks();
    let runtime = db.runtime();
    let task = tasks.get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::RetryReady);
    assert_ne!(task.state(), TaskState::Completed);
    let attempt = runtime
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(attempt.outcome(), Some(AttemptOutcome::Succeeded));

    let types: Vec<EventType> = tasks
        .list_events_for_task(&task_id)
        .unwrap()
        .iter()
        .map(|event| event.fact.event_type)
        .collect();
    assert!(types.contains(&EventType::VerificationFailed));
    assert!(!types.contains(&EventType::VerificationSucceeded));
}

#[test]
fn review_rejection_fixture_grants_no_authority_without_the_evaluator() {
    // The `review_rejection` FakeAgent scenario is deterministic execution
    // evidence only. The verdict still belongs to the evaluator: reject
    // with a rejecting evaluator, complete with a verifying one.
    for (mode, expect_completed) in [
        (EvaluatorMode::AlwaysRejected, false),
        (EvaluatorMode::AlwaysVerified, true),
    ] {
        let db = TestDb::new("review-fixture");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        let executor = FakeExecutor::for_scenario(FakeScenario::ReviewRejection);
        let evaluator = FakeEvaluator::new(mode);
        let outcome = service
            .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator, None)
            .expect("runnable task must schedule");
        let task = service.tasks().get_task(&task_id).unwrap().unwrap();
        if expect_completed {
            assert!(
                matches!(
                    outcome,
                    OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
                ),
                "evaluator verdict owns completion, not the fixture"
            );
            assert_eq!(task.state(), TaskState::Completed);
        } else {
            assert!(
                matches!(
                    outcome,
                    OrchestrationOutcome::ExecutionSucceededButRejected { .. }
                ),
                "evaluator verdict owns rejection, not the fixture"
            );
            assert_eq!(task.state(), TaskState::RetryReady);
        }
        // Either way the attempt itself concluded Succeeded: evidence that
        // Attempt success != Task success.
        let attempt = service
            .runtime()
            .get_attempt(&AttemptId::new("A-1").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(attempt.outcome(), Some(AttemptOutcome::Succeeded));
    }
}

// -- dependency readiness ----------------------------------------------------

#[test]
fn unfinished_dependency_blocks_without_runtime_facts() {
    let db = TestDb::new("dependency-block");
    let mut service = db.orchestration();
    let blocked = seed_ready(&mut service, "P-1", "T-A");
    let prerequisite = seed_ready(&mut service, "P-1", "T-B");
    service
        .tasks_mut()
        .add_dependency(&blocked, &prerequisite)
        .unwrap();

    let executor = FakeExecutor::for_scenario(FakeScenario::Success);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&blocked, ids("E-1", "A-1"), &executor, &evaluator, None)
        .expect("schedule must report readiness, not fail");
    assert_eq!(
        outcome,
        OrchestrationOutcome::WaitingOnDependency {
            task_id: blocked.clone(),
            blocking: vec![prerequisite.clone()],
        }
    );

    // READY + unfinished dependency: no claim, no Execution, no Attempt,
    // no executor invocation, no runtime event.
    let task = service.tasks().get_task(&blocked).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Ready);
    assert_eq!(executor.call_count(), 0);
    assert_eq!(evaluator.call_count(), 0);
    assert!(service
        .runtime()
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .is_none());
    let runtime_events: Vec<_> = service
        .tasks()
        .list_events_for_task(&blocked)
        .unwrap()
        .into_iter()
        .filter(|event| is_runtime_event(event.fact.event_type))
        .collect();
    assert!(runtime_events.is_empty());
}

#[test]
fn completed_dependency_is_runnable() {
    let db = TestDb::new("dependency-satisfied");
    let mut service = db.orchestration();
    let prerequisite = seed_ready(&mut service, "P-1", "T-B");
    let executor_b = FakeExecutor::for_scenario(FakeScenario::Success);
    service
        .schedule(
            &prerequisite,
            ids("E-B", "A-B"),
            &executor_b,
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            None,
        )
        .unwrap();
    assert_eq!(
        service
            .tasks()
            .get_task(&prerequisite)
            .unwrap()
            .unwrap()
            .state(),
        TaskState::Completed
    );

    let task_id = seed_ready(&mut service, "P-1", "T-A");
    service
        .tasks_mut()
        .add_dependency(&task_id, &prerequisite)
        .unwrap();
    let executor = FakeExecutor::for_scenario(FakeScenario::Success);
    let outcome = service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &executor,
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            None,
        )
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        ),
        "READY + COMPLETED dependency must be runnable"
    );
}

// -- wrong lifecycle ---------------------------------------------------------

#[test]
fn non_ready_lifecycle_states_are_not_runnable() {
    // Reachable non-READY states, driven through the semantic Task APIs.
    // DRAFT is fresh; CLAIMED/RUNNING/SUBMITTED/AWAITING_REVIEW/VERIFYING
    // walk the canonical path; RETRY_READY comes from a rejected cycle;
    // COMPLETED from a verified cycle. FAILED/CANCELLED have no accepted
    // incoming edge yet (task-engine.md §16), so no service can construct
    // them — the scheduler census unit test covers those states purely.
    let db = TestDb::new("wrong-lifecycle");
    let mut service = db.orchestration();

    let draft = seed_ready(&mut service, "P-1", "T-DRAFT-SEED");
    service
        .tasks_mut()
        .list_tasks_for_project(&ProjectId::new("P-1").unwrap())
        .unwrap();
    // Rewind: create a genuinely DRAFT task (seed_ready marks READY).
    let draft_task = Task::draft(
        TaskId::new("T-DRAFT2").unwrap(),
        ProjectId::new("P-1").unwrap(),
        "objective draft",
    )
    .unwrap();
    service.tasks_mut().create_task(&draft_task).unwrap();
    let _ = draft;

    let claimed = seed_ready(&mut service, "P-1", "T-CLAIMED");
    service.tasks_mut().claim(&claimed).unwrap();

    let running = seed_ready(&mut service, "P-1", "T-RUNNING");
    service.tasks_mut().claim(&running).unwrap();
    service.tasks_mut().start(&running).unwrap();

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
            None,
        )
        .unwrap();
    assert_eq!(
        service.tasks().get_task(&retry).unwrap().unwrap().state(),
        TaskState::RetryReady
    );

    let completed = seed_ready(&mut service, "P-1", "T-DONE");
    service
        .schedule(
            &completed,
            ids("E-DONE", "A-DONE"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            None,
        )
        .unwrap();

    let cases = [
        (TaskId::new("T-DRAFT2").unwrap(), TaskState::Draft),
        (claimed, TaskState::Claimed),
        (running, TaskState::Running),
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
                None,
            )
            .expect("refusal must be a typed outcome, not an error");
        assert_eq!(
            outcome,
            OrchestrationOutcome::NotRunnable {
                task_id: task_id.clone(),
                state,
            },
            "state {state} must refuse execution"
        );
        assert_eq!(executor.call_count(), 0, "state {state} must not invoke");
        assert_eq!(evaluator.call_count(), 0, "state {state} needs no verdict");
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

// -- duplicate scheduling ----------------------------------------------------

#[test]
fn second_schedule_observes_state_and_duplicates_nothing() {
    let db = TestDb::new("duplicate");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let first = service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            None,
        )
        .unwrap();
    assert!(matches!(
        first,
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
    ));

    let second = service
        .schedule(
            &task_id,
            ids("E-2", "A-2"),
            &FakeExecutor::for_scenario(FakeScenario::Success),
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            None,
        )
        .unwrap();
    assert_eq!(
        second,
        OrchestrationOutcome::NotRunnable {
            task_id: task_id.clone(),
            state: TaskState::Completed,
        }
    );
    // No second authoritative attempt exists under either identity.
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
    let executions: Vec<_> = service
        .runtime()
        .list_events_ordered()
        .unwrap()
        .into_iter()
        .filter(|event| event.fact.event_type == EventType::ExecutionCreated)
        .collect();
    assert_eq!(executions.len(), 1);
}

// -- failure behavior: no retry, no verification ------------------------------

#[test]
fn malformed_result_concludes_failed_and_never_reaches_verification() {
    let db = TestDb::new("malformed");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let executor = FakeExecutor::for_scenario(FakeScenario::MalformedResult);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator, None)
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
    // Malformed provider output is never trusted success: the Task is not
    // submitted, no verdict is sought, and no evaluator can bless it.
    let task = service.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
    assert_eq!(evaluator.call_count(), 0);
    let types: Vec<EventType> = service
        .tasks()
        .list_events_for_task(&task_id)
        .unwrap()
        .iter()
        .map(|event| event.fact.event_type)
        .collect();
    assert!(!types.contains(&EventType::Submitted));
    assert!(!types.contains(&EventType::VerificationSucceeded));
    // Exactly one terminal attempt: no hidden retry.
    let attempts = service
        .runtime()
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(attempts.attempts().len(), 1);
}

#[test]
fn partial_result_is_not_silent_success() {
    let db = TestDb::new("partial");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");

    let executor = FakeExecutor::for_scenario(FakeScenario::PartialResult);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let outcome = service
        .schedule(&task_id, ids("E-1", "A-1"), &executor, &evaluator, None)
        .unwrap();
    assert!(
        matches!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                outcome: AttemptOutcome::Failed,
                ..
            }
        ),
        "partial_result != normal successful submitted result"
    );
    let task = service.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Running);
    assert_eq!(evaluator.call_count(), 0);
}

#[test]
fn crash_and_timeout_map_to_canonical_outcomes_without_retry() {
    for (scenario, expected) in [
        (FakeScenario::Crash, AttemptOutcome::Failed),
        (FakeScenario::Timeout, AttemptOutcome::TimedOut),
        (FakeScenario::QuotaReached, AttemptOutcome::Failed),
        (FakeScenario::NetworkFailure, AttemptOutcome::Failed),
    ] {
        let db = TestDb::new("failure-map");
        let mut service = db.orchestration();
        let task_id = seed_ready(&mut service, "P-1", "T-1");
        let outcome = service
            .schedule(
                &task_id,
                ids("E-1", "A-1"),
                &FakeExecutor::for_scenario(scenario),
                &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
                None,
            )
            .unwrap();
        assert_eq!(
            outcome,
            OrchestrationOutcome::ExecutionFailed {
                task_id: task_id.clone(),
                execution_id: ExecutionId::new("E-1").unwrap(),
                attempt_id: AttemptId::new("A-1").unwrap(),
                outcome: expected,
            },
            "scenario {}",
            scenario.as_str()
        );
        // One terminal attempt, task still RUNNING: no automatic Attempt 2.
        let execution = service
            .runtime()
            .get_execution(&ExecutionId::new("E-1").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(execution.attempts().len(), 1);
        assert_eq!(
            service.tasks().get_task(&task_id).unwrap().unwrap().state(),
            TaskState::Running
        );
    }
}

#[test]
fn cancellation_records_cancelled_without_submission() {
    // Provider-neutral proof: this executor is not FakeAgent at all.
    let db = TestDb::new("cancelled");
    let mut service = db.orchestration();
    let task_id = seed_ready(&mut service, "P-1", "T-1");
    let outcome = service
        .schedule(
            &task_id,
            ids("E-1", "A-1"),
            &CancelStub,
            &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
            None,
        )
        .unwrap();
    assert_eq!(
        outcome,
        OrchestrationOutcome::Cancelled {
            task_id: task_id.clone(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    );
    let attempt = service
        .runtime()
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(attempt.outcome(), Some(AttemptOutcome::Cancelled));
    assert_eq!(
        service.tasks().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );
}

// -- completion authority -----------------------------------------------------

#[test]
fn completion_requires_each_step_and_only_the_verdict_completes() {
    // Stepwise proof through the trusted services: FakeAgent success,
    // Attempt success, submission, and VERIFYING each leave the Task
    // uncompleted. Only the independent Verified verdict plus the trusted
    // verification-success transition completes it.
    let db = TestDb::new("completion-authority");
    let mut tasks = db.tasks();
    let mut runtime = db.runtime();

    let project = Project::new(ProjectId::new("P-1").unwrap(), "project P-1").unwrap();
    tasks.create_project(&project).unwrap();
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

    // FakeAgent success alone: NOT COMPLETED.
    let output = FakeAgent::for_scenario(FakeScenario::Success)
        .execute(
            &ExecutionRequest::new(
                task_id.clone(),
                ExecutionId::new("E-1").unwrap(),
                AttemptId::new("A-1").unwrap(),
            ),
            &ExecutionControl::fresh(),
        )
        .expect("success scenario reports output");
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Running
    );

    // Attempt Succeeded alone: NOT COMPLETED.
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

    // submit_result alone: NOT COMPLETED.
    tasks.submit_result(&task_id, output.summary()).unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Submitted
    );

    // VERIFYING alone: NOT COMPLETED.
    tasks.route_to_review(&task_id).unwrap();
    tasks.begin_verification(&task_id).unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying
    );

    // Independent Verified verdict + trusted transition: COMPLETED.
    let verdict = FakeEvaluator::new(EvaluatorMode::AlwaysVerified)
        .evaluate(&fake_evaluator_request(
            &task_id,
            output.summary(),
            output.evidence_ref(),
        ))
        .unwrap();
    assert_eq!(verdict, app_services::EvaluationVerdict::Verified);
    tasks
        .verification_succeeded_under_policy(&task_id, "VERIFIED via independent evaluation", None)
        .unwrap();
    assert_eq!(
        tasks.get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Completed
    );
}

fn fake_evaluator_request(
    task_id: &TaskId,
    summary: &str,
    evidence_ref: &str,
) -> app_services::EvaluationRequest {
    app_services::EvaluationRequest {
        task_id: task_id.clone(),
        execution_id: ExecutionId::new("E-1").unwrap(),
        attempt_id: AttemptId::new("A-1").unwrap(),
        summary: summary.to_owned(),
        evidence_ref: evidence_ref.to_owned(),
    }
}

// -- isolation ----------------------------------------------------------------

#[test]
fn two_tasks_execute_through_separate_adapters_without_interference() {
    let db = TestDb::new("two-tasks");
    let mut service = db.orchestration();
    let first = seed_ready(&mut service, "P-1", "T-1");
    let second = seed_ready(&mut service, "P-1", "T-2");

    let executor_a = FakeExecutor::for_scenario(FakeScenario::Success);
    let executor_b = FakeExecutor::for_scenario(FakeScenario::Success);
    let evaluator_a = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let evaluator_b = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

    for (task_id, exec_ids, executor, evaluator) in [
        (&first, ids("E-1", "A-1"), &executor_a, &evaluator_a),
        (&second, ids("E-2", "A-2"), &executor_b, &evaluator_b),
    ] {
        let outcome = service
            .schedule(task_id, exec_ids, executor, evaluator, None)
            .unwrap();
        assert!(
            matches!(
                outcome,
                OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
            ),
            "both tasks complete independently"
        );
    }
    assert_eq!(executor_a.call_count(), 1);
    assert_eq!(executor_b.call_count(), 1);
    assert_eq!(evaluator_a.call_count(), 1);
    assert_eq!(evaluator_b.call_count(), 1);
    assert_eq!(
        service.tasks().get_task(&first).unwrap().unwrap().state(),
        TaskState::Completed
    );
    assert_eq!(
        service.tasks().get_task(&second).unwrap().unwrap().state(),
        TaskState::Completed
    );
}

// -- identity preflight (TASK-0049, G5) --------------------------------------
//
// `enter_execution` refuses synchronously detectable execution/attempt
// identity conflicts before the first mutation (claim/start), so a
// rejected entry commits no lifecycle fact and leaves the Task
// rerunnable with fresh identities. The store-level uniqueness guards
// stay the backstop for concurrent races.

#[test]
fn enter_execution_refuses_duplicate_identities_before_any_mutation() {
    use app_services::ExecutionEntry;

    let db = TestDb::new("identity-preflight");
    let mut service = db.orchestration();
    let first = seed_ready(&mut service, "P-1", "T-1");
    let second = seed_ready(&mut service, "P-1", "T-2");

    // First entry commits: T-1 RUNNING + E-1, no attempt yet.
    match service
        .enter_execution(&first, ids("E-1", "A-1"))
        .expect("first entry")
    {
        ExecutionEntry::Runnable(_) => {}
        ExecutionEntry::NotRunnable(outcome) => {
            panic!("T-1 must be runnable, got {outcome:?}")
        }
    }
    // Attempt A-1 exists under E-1 (created here directly; the daemon
    // creates it later in the background half).
    service
        .runtime_mut()
        .create_attempt(
            &ExecutionId::new("E-1").unwrap(),
            AttemptId::new("A-1").unwrap(),
            ProviderRef::new("preflight-stub").unwrap(),
        )
        .expect("setup attempt");

    // Execution-id collision (owned by T-1) refuses before claim/start.
    let err = service
        .enter_execution(&second, ids("E-1", "A-2"))
        .expect_err("duplicate execution must refuse");
    assert!(
        matches!(
            err,
            app_services::ServiceError::Storage(storage_sqlite::StorageError::Duplicate(_))
        ),
        "duplicate execution must be typed, got {err:?}"
    );
    // Attempt-id collision refuses before claim/start.
    let err = service
        .enter_execution(&second, ids("E-2", "A-1"))
        .expect_err("duplicate attempt must refuse");
    assert!(
        matches!(
            err,
            app_services::ServiceError::Storage(storage_sqlite::StorageError::Duplicate(_))
        ),
        "duplicate attempt must be typed, got {err:?}"
    );

    // T-2 is untouched and rerunnable: still READY, no runtime facts,
    // and only its setup journal facts (created + ready).
    assert_eq!(
        service.tasks().get_task(&second).unwrap().unwrap().state(),
        TaskState::Ready,
        "refused entries must not claim/start T-2"
    );
    assert!(
        service
            .runtime()
            .get_execution(&ExecutionId::new("E-2").unwrap())
            .unwrap()
            .is_none(),
        "refused entry must not create E-2"
    );
    assert!(
        service
            .runtime()
            .get_attempt(&AttemptId::new("A-2").unwrap())
            .unwrap()
            .is_none(),
        "refused entry must not create A-2"
    );
    let types: Vec<EventType> = service
        .tasks()
        .list_events_for_task(&second)
        .unwrap()
        .iter()
        .map(|event| event.fact.event_type)
        .collect();
    assert_eq!(
        types,
        vec![EventType::TaskCreated, EventType::MarkedReady],
        "refused entries must leave no lifecycle facts, got {types:?}"
    );

    // Fresh identities enter normally: the rejections did not poison T-2.
    match service
        .enter_execution(&second, ids("E-2", "A-2"))
        .expect("retry with fresh identities")
    {
        ExecutionEntry::Runnable(_) => {}
        ExecutionEntry::NotRunnable(outcome) => {
            panic!("retry must be runnable, got {outcome:?}")
        }
    }
}

// TASK-0125 phase (c) guard: without capture-pipeline effects there is
// nothing a verdict could bind, so the shared concluding boundary must
// refuse completion and mint no claim/verdict rows — even when the
// independent evaluation says Verified. Minting without bound evidence
// would be authority without measurement.
#[test]
fn required_task_without_capture_effects_cannot_complete_and_mints_nothing() {
    let db = TestDb::new("candidate-mints-verdict");
    let mut service = db.orchestration();
    let project = ProjectId::new("P-CV-MINT").unwrap();
    service
        .tasks_mut()
        .create_project(&Project::new(project.clone(), "project P-CV-MINT").unwrap())
        .unwrap();
    let criterion = "tests pass";
    let requirement = EvidenceRequirement::authored_default(criterion).unwrap();
    let mut task = Task::draft(
        TaskId::new("T-CV-MINT").unwrap(),
        project,
        "objective T-CV-MINT".to_owned(),
    )
    .unwrap();
    task.define_planning_metadata(vec!["src/**".to_owned()], vec![criterion.to_owned()], "low")
        .unwrap();
    task.define_evidence_requirements(vec![requirement])
        .unwrap();
    service.tasks_mut().create_task(&task).unwrap();
    let task_id = task.id().clone();
    service.tasks_mut().mark_ready(&task_id).unwrap();

    let outcome = service.schedule(
        &task_id,
        ids("E-CV-MINT", "A-CV-MINT"),
        &FakeExecutor::for_scenario(FakeScenario::Success),
        &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        None,
    );
    assert!(
        outcome.is_err(),
        "admission must refuse without bound evidence, got {outcome:?}"
    );
    assert_eq!(
        service.tasks().get_task(&task_id).unwrap().unwrap().state(),
        TaskState::Verifying,
        "a refused completion leaves the task VERIFYING"
    );
    assert!(
        service
            .tasks()
            .store()
            .list_claim_verifications_for_task(&task_id)
            .unwrap()
            .is_empty(),
        "no evidence means no minted verdicts"
    );
    assert!(
        service
            .tasks()
            .store()
            .list_agent_claims_for_task(&task_id)
            .unwrap()
            .is_empty(),
        "no evidence means no minted claims either"
    );
}
