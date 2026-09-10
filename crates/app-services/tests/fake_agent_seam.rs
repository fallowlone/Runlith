//! Runtime seam proof (TASK-0036): `RuntimeService` + `FakeAgent` composed
//! only from the test/application side.
//!
//! The direction under test is:
//!
//! ```text
//! test harness creates Execution + Attempt (RuntimeService)
//!   → FakeAgent(success/crash) reports a canonical result (pure values)
//!   → harness interprets the result and calls conclude_attempt(...)
//!   → durable state + journal verified
//! ```
//!
//! FakeAgent never calls `RuntimeService` itself: its API accepts only an
//! [`fake_agent::ExecutionRequest`] plus an [`fake_agent::ExecutionControl`]
//! and returns plain values. Every journal row below is minted by the
//! trusted service (provenance `runtime-application-service/...`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, AttemptOutcome, ExecutionId, ProviderRef};
use app_services::{FixedClock, RuntimeService, TaskService};
use fake_agent::{ExecutionControl, ExecutionRequest, FakeAgent, FakeScenario};
use storage_sqlite::EventType;
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

static COUNTER: AtomicU64 = AtomicU64::new(0);
const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-fake-agent-seam-test-{}-{suite}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("test dir");
        Self {
            path: dir.join("test.db"),
        }
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

fn seed_task(db: &TestDb, project: &str, id: &str) -> TaskId {
    let mut service = db.tasks();
    service
        .create_project(
            &Project::new(
                ProjectId::new(project).unwrap(),
                format!("project {project}"),
            )
            .unwrap(),
        )
        .unwrap();
    let task = Task::draft(
        TaskId::new(id).unwrap(),
        ProjectId::new(project).unwrap(),
        format!("objective {id}"),
    )
    .unwrap();
    service.create_task(&task).unwrap();
    task.id().clone()
}

fn provider() -> ProviderRef {
    ProviderRef::new(fake_agent::FAKE_PROVIDER_REF).unwrap()
}

fn canonical_request(task: &TaskId, execution: &str, attempt: &str) -> ExecutionRequest {
    ExecutionRequest::new(
        task.clone(),
        ExecutionId::new(execution).unwrap(),
        AttemptId::new(attempt).unwrap(),
    )
}

#[test]
fn fake_agent_success_flows_through_conclude_attempt_without_touching_task() {
    let db = TestDb::new("success-seam");
    let task_id = seed_task(&db, "P-1", "T-1");
    let mut runtime = db.runtime();

    runtime
        .create_execution(ExecutionId::new("E-1").unwrap(), task_id.clone())
        .unwrap();
    runtime
        .create_attempt(
            &ExecutionId::new("E-1").unwrap(),
            AttemptId::new("A-1").unwrap(),
            provider(),
        )
        .unwrap();

    // FakeAgent reports behavior only: pure values, no service access.
    let output = FakeAgent::for_scenario(FakeScenario::Success)
        .execute(
            &canonical_request(&task_id, "E-1", "A-1"),
            &ExecutionControl::fresh(),
        )
        .expect("success scenario must report output");
    assert!(!output.is_partial());

    // The harness — not FakeAgent — interprets the report and concludes.
    let concluded = runtime
        .conclude_attempt(
            &ExecutionId::new("E-1").unwrap(),
            &AttemptId::new("A-1").unwrap(),
            AttemptOutcome::Succeeded,
            output.summary(),
        )
        .unwrap();
    assert_eq!(concluded.event.fact.event_type, EventType::AttemptConcluded);
    assert_eq!(
        concluded.event.fact.attempt_outcome,
        Some(storage_sqlite::AttemptOutcome::Succeeded)
    );
    assert!(
        concluded
            .event
            .fact
            .provenance
            .starts_with("runtime-application-service/"),
        "journal authority stays with the trusted service"
    );

    let stored = runtime
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(stored.outcome(), Some(AttemptOutcome::Succeeded));

    // No scenario concludes the Task: it stays DRAFT with no lifecycle
    // event beyond creation.
    let task = db.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Draft);
    let lifecycle: Vec<_> = runtime
        .list_events_ordered()
        .unwrap()
        .into_iter()
        .filter(|event| {
            !matches!(
                event.fact.event_type,
                EventType::ProjectCreated
                    | EventType::TaskCreated
                    | EventType::ExecutionCreated
                    | EventType::AttemptCreated
                    | EventType::AttemptConcluded
            )
        })
        .collect();
    assert!(lifecycle.is_empty(), "no Task lifecycle mutation occurred");
}

#[test]
fn fake_agent_crash_is_recorded_as_failed_evidence() {
    let db = TestDb::new("crash-seam");
    let task_id = seed_task(&db, "P-1", "T-1");
    let mut runtime = db.runtime();

    runtime
        .create_execution(ExecutionId::new("E-1").unwrap(), task_id.clone())
        .unwrap();
    runtime
        .create_attempt(
            &ExecutionId::new("E-1").unwrap(),
            AttemptId::new("A-1").unwrap(),
            provider(),
        )
        .unwrap();

    let error = FakeAgent::for_scenario(FakeScenario::Crash)
        .execute(
            &canonical_request(&task_id, "E-1", "A-1"),
            &ExecutionControl::fresh(),
        )
        .unwrap_err();
    assert_eq!(error.kind(), "terminated");

    // Harness maps the provider-neutral report onto the durable outcome.
    runtime
        .conclude_attempt(
            &ExecutionId::new("E-1").unwrap(),
            &AttemptId::new("A-1").unwrap(),
            AttemptOutcome::Failed,
            error.to_string(),
        )
        .unwrap();
    let stored = runtime
        .get_attempt(&AttemptId::new("A-1").unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(stored.outcome(), Some(AttemptOutcome::Failed));

    // Attempt failure is not Task failure.
    let task = db.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Draft);
}

#[test]
fn no_scenario_appends_journal_or_completes_task_by_itself() {
    // Every scenario runs against a throwaway database with no service
    // calls at all: the journal cannot grow because FakeAgent owns no
    // journal handle, and the Task cannot move because FakeAgent owns no
    // Task handle. The only assertion available without a service is the
    // type-level one — `execute` returns plain values — plus the explicit
    // journal check in the tests above. This test pins the full matrix to
    // that contract: each scenario terminates on its own (hang via
    // immediate cancel) without any persistence to terminate into.
    let db = TestDb::new("no-persistence");
    let task_id = seed_task(&db, "P-1", "T-1");
    let runtime = db.runtime();
    let before = runtime.list_events_ordered().unwrap().len();

    for scenario in FakeScenario::all() {
        let control = ExecutionControl::fresh();
        if scenario == FakeScenario::Hang {
            control.cancel();
        }
        let request = ExecutionRequest::new(
            task_id.clone(),
            ExecutionId::new(format!("E-{}", scenario.as_str())).unwrap(),
            AttemptId::new(format!("A-{}", scenario.as_str())).unwrap(),
        );
        let _ = FakeAgent::for_scenario(scenario).execute(&request, &control);
    }

    assert_eq!(
        runtime.list_events_ordered().unwrap().len(),
        before,
        "FakeAgent executions must append no journal events by themselves"
    );
    let task = db.tasks().get_task(&task_id).unwrap().unwrap();
    assert_eq!(task.state(), TaskState::Draft);
}
