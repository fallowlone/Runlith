//! Post-admission / pre-Attempt entry recovery keepers (TASK-0054, G5 B-RS).
//!
//! TASK-0052 proved the production execution can durably commit admission
//! (Task RUNNING + Execution + Attempt reservation + journal triple) and
//! then strand: a crash or deterministic background failure before
//! `create_attempt` leaves a Task that ordinary `run` refuses
//! (`INVALID_TRANSITION`), `cancel` misreports (`ALREADY_CONCLUDED`),
//! and no production operation recovers.
//!
//! These keepers pin the recovery contract at the trusted-service layer:
//!
//! ```text
//! strand (RUNNING + execution + reservation, zero attempts)
//!   → OrchestrationService::plan_pre_attempt_resume detects it (read-only)
//!   → production resume continues the SAME execution with its reserved
//!     attempt (no Task transition, no new event types, no provider history)
//! ```
//!
//! Resume performs no Task lifecycle transition, so the canonical state
//! machine (`task-domain`, `ACCEPTED_TRANSITIONS`) is untouched: recovery
//! resumes the admitted Execution in place instead of inventing a
//! RUNNING → READY edge.
//!
//! RED → GREEN note: `plan_pre_attempt_resume` does not exist before
//! TASK-0054, so every keeper calling it fails (compile error) on the old
//! tree. The characterization keeper (`stranded_task_stays_unrunnable_
//! through_ordinary_run`) passes before and after: it documents the old
//! failure shape that recovery must fix without changing `run` semantics.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, ExecutionId};
use app_services::{
    ExecutionEntry, ManagedWorkspaceRoot, OrchestrationIds, OrchestrationService, SystemClock,
    TaskService, WorkspaceRequest, WorkspaceService,
};
use storage_sqlite::{EventType, SqliteStore};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};
use workspace::WorkspaceStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Env {
    dir: PathBuf,
    db: PathBuf,
}

impl Env {
    fn named(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-entry-recovery-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        Self {
            db: dir.join("test.db"),
            dir,
        }
    }

    fn seed_ready(&self, project: &str, task: &str) {
        let mut tasks = TaskService::open(&self.db, SystemClock).expect("open task service");
        let project_id = ProjectId::new(project).unwrap();
        tasks
            .create_project(&Project::new(project_id.clone(), "p".to_owned()).unwrap())
            .unwrap();
        let task_id = TaskId::new(task).unwrap();
        tasks
            .create_task(&Task::draft(task_id.clone(), project_id, "objective".to_owned()).unwrap())
            .unwrap();
        tasks.mark_ready(&task_id).unwrap();
    }

    fn open_orchestration(&self) -> OrchestrationService<SystemClock> {
        OrchestrationService::open(&self.db, SystemClock).expect("open orchestration")
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.db).expect("open store")
    }

    fn task_state(&self, task: &str) -> TaskState {
        self.store()
            .get_task(&TaskId::new(task).unwrap())
            .expect("read task")
            .expect("task exists")
            .state()
    }

    fn journal_types(&self, task: &str) -> Vec<String> {
        self.store()
            .list_events_for_task(&TaskId::new(task).unwrap())
            .expect("read journal")
            .iter()
            .map(|event| event.fact.event_type.as_str().to_owned())
            .collect()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    assert!(output.status.success(), "git {args:?} failed");
}

/// Commits the durable entry half and then "crashes": the returned
/// `WorkItem` is dropped without running any background half. The durable
/// rows left behind are exactly the post-`COMMIT` crash shape from the
/// audit (RUNNING + execution + reservation + journal triple, no lease,
/// no attempt).
fn admit_and_crash(env: &Env, task: &str, execution: &str, attempt: &str) -> scheduler::WorkItem {
    let mut orchestration = env.open_orchestration();
    match orchestration
        .enter_execution(
            &TaskId::new(task).unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new(execution).unwrap(),
                attempt_id: AttemptId::new(attempt).unwrap(),
            },
        )
        .expect("entry commits")
    {
        ExecutionEntry::Runnable(work) => work,
        ExecutionEntry::NotRunnable(outcome) => panic!("entry must commit, got {outcome:?}"),
    }
    // `orchestration` (and the un-driven `work`) drops here: the crash.
}

fn workspace_request(execution: &str, repo: &std::path::Path, reference: &str) -> WorkspaceRequest {
    WorkspaceRequest {
        execution_id: ExecutionId::new(execution).unwrap(),
        repository_ref: workspace::RepositoryRef::new(reference).unwrap(),
        repository_path: repo.to_string_lossy().into_owned(),
        base_revision: "HEAD".to_owned(),
    }
}

fn workspace_service(env: &Env) -> WorkspaceService<git_cli::GitCli, SystemClock> {
    let root_dir = env.dir.join("workspaces");
    std::fs::create_dir_all(&root_dir).expect("managed root dir");
    let root = ManagedWorkspaceRoot::new(&root_dir).expect("managed root");
    WorkspaceService::open(&env.db, git_cli::GitCli::new(), root, SystemClock)
        .expect("open workspace service")
}

#[test]
fn crash_after_admission_leaves_the_exact_stranded_shape() {
    let env = Env::named("strand-shape");
    env.seed_ready("P-1", "T-1");
    let _work = admit_and_crash(&env, "T-1", "E-1", "A-1");

    assert_eq!(env.task_state("T-1"), TaskState::Running);
    let store = env.store();
    let execution = store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .expect("read execution")
        .expect("execution exists");
    assert!(execution.attempts().is_empty(), "no attempt yet");
    let reservation = store
        .get_attempt_reservation(&AttemptId::new("A-1").unwrap())
        .expect("read reservation")
        .expect("reservation exists");
    assert_eq!(reservation.0.as_str(), "E-1");
    assert_eq!(reservation.1.as_str(), "T-1");
    assert!(
        store
            .get_workspace_for_execution(&ExecutionId::new("E-1").unwrap())
            .expect("read lease")
            .is_none(),
        "no lease yet"
    );
    assert_eq!(
        env.journal_types("T-1"),
        vec![
            "task.created",
            "task.marked_ready",
            "task.claimed",
            "task.started",
            "execution.created",
        ]
    );
}

#[test]
fn stranded_task_stays_unrunnable_through_ordinary_run() {
    // Characterization of the B-RS failure shape (passes before and
    // after TASK-0054): ordinary entry refuses a stranded RUNNING task,
    // with durable state unchanged. Recovery must arrive via the resume
    // path, never by weakening `run`.
    let env = Env::named("still-not-runnable");
    env.seed_ready("P-1", "T-1");
    let _work = admit_and_crash(&env, "T-1", "E-1", "A-1");

    let mut orchestration = env.open_orchestration();
    // Fresh identities: not runnable from RUNNING, nothing committed.
    match orchestration
        .enter_execution(
            &TaskId::new("T-1").unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new("E-2").unwrap(),
                attempt_id: AttemptId::new("A-2").unwrap(),
            },
        )
        .expect("entry reports")
    {
        ExecutionEntry::NotRunnable(_) => {}
        ExecutionEntry::Runnable(work) => panic!("stranded task must not run, got {work:?}"),
    }
    // Same identities: not runnable from RUNNING (never a second
    // admission), with durable state unchanged.
    match orchestration
        .enter_execution(
            &TaskId::new("T-1").unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new("E-1").unwrap(),
                attempt_id: AttemptId::new("A-1").unwrap(),
            },
        )
        .expect("entry reports")
    {
        ExecutionEntry::NotRunnable(_) => {}
        ExecutionEntry::Runnable(work) => {
            panic!("same-identity re-entry must never re-admit, got {work:?}")
        }
    }
    assert_eq!(env.task_state("T-1"), TaskState::Running);
    assert_eq!(
        env.journal_types("T-1"),
        vec![
            "task.created",
            "task.marked_ready",
            "task.claimed",
            "task.started",
            "execution.created",
        ],
        "refused re-entry must append no lifecycle facts"
    );
}

#[test]
fn resume_plan_detects_a_stranded_pre_attempt_entry() {
    let env = Env::named("plan-detects");
    env.seed_ready("P-1", "T-1");
    let _work = admit_and_crash(&env, "T-1", "E-1", "A-1");

    // A freshly reopened service (new connections, no memory of the
    // crashed half — the restart shape) detects the strand read-only.
    let journal_before = env.journal_types("T-1");
    let orchestration = env.open_orchestration();
    let plan = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect("strand must be recoverable");
    assert_eq!(plan.work.task_id.as_str(), "T-1");
    assert_eq!(plan.work.execution_id.as_str(), "E-1");
    assert_eq!(plan.work.attempt_id.as_str(), "A-1");
    assert!(
        plan.repository_ref.is_none() && plan.base_revision.is_none(),
        "no lease exists, so the operator must supply workspace inputs"
    );
    assert_eq!(
        env.journal_types("T-1"),
        journal_before,
        "planning is read-only: no journal facts"
    );
    assert_eq!(env.task_state("T-1"), TaskState::Running);
}

#[test]
fn resume_plan_is_unknown_for_a_missing_execution() {
    let env = Env::named("plan-missing");
    env.seed_ready("P-1", "T-1");
    let orchestration = env.open_orchestration();
    let error = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-ghost").unwrap())
        .expect_err("missing execution must not plan");
    assert!(
        error.to_string().contains("E-ghost"),
        "refusal names the execution, got {error}"
    );
}

#[test]
fn resume_plan_refuses_once_an_attempt_exists() {
    let env = Env::named("plan-attempted");
    env.seed_ready("P-1", "T-1");
    let work = admit_and_crash(&env, "T-1", "E-1", "A-1");
    // The background half ran exactly once: attempt consumed, concluded.
    let mut orchestration = env.open_orchestration();
    orchestration
        .runtime_mut()
        .create_attempt(
            &work.execution_id,
            work.attempt_id.clone(),
            agent_runtime::ProviderRef::new("test-executor").unwrap(),
        )
        .expect("owner consumes its reservation");
    orchestration
        .runtime_mut()
        .conclude_attempt(
            &work.execution_id,
            &work.attempt_id,
            agent_runtime::AttemptOutcome::Failed,
            "fixture evidence",
        )
        .expect("conclude");

    let error = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect_err("attempted executions are out of pre-attempt scope");
    let message = error.to_string();
    assert!(
        message.contains("E-1"),
        "refusal names the execution, got {message}"
    );
}

#[test]
fn resume_plan_refuses_a_blocked_task_until_it_resumes() {
    let env = Env::named("plan-blocked");
    env.seed_ready("P-1", "T-1");
    let _work = admit_and_crash(&env, "T-1", "E-1", "A-1");

    let mut orchestration = env.open_orchestration();
    orchestration
        .tasks_mut()
        .block(
            &TaskId::new("T-1").unwrap(),
            "operator hold during incident",
        )
        .expect("RUNNING → BLOCKED is accepted");
    let error = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect_err("blocked task must not resume the execution half");
    assert!(
        error.to_string().contains("BLOCKED"),
        "refusal names the actual state, got {error}"
    );
    // The accepted BLOCKED → RUNNING resume re-arms recovery with no new
    // execution and no invented edge.
    orchestration
        .tasks_mut()
        .start(&TaskId::new("T-1").unwrap())
        .expect("BLOCKED → RUNNING is accepted");
    let plan = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect("re-armed after task resume");
    assert_eq!(plan.work.attempt_id.as_str(), "A-1");
}

#[test]
fn resume_plan_fails_closed_on_reservation_mismatch() {
    let env = Env::named("plan-mismatch");
    env.seed_ready("P-1", "T-1");
    env.seed_ready("P-2", "T-2");
    let _work = admit_and_crash(&env, "T-1", "E-1", "A-1");
    // Corrupt the durable state behind the services: the reservation now
    // names a task that does not own the execution. Production never
    // writes this shape; recovery must fail closed, never guess.
    {
        let connection = rusqlite::Connection::open(&env.db).expect("raw open");
        connection
            .execute(
                "UPDATE attempt_reservations SET task_id = ?1 WHERE attempt_id = ?2",
                rusqlite::params!["T-2", "A-1"],
            )
            .expect("corrupt reservation");
    }
    let orchestration = env.open_orchestration();
    let error = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect_err("mismatched reservation must fail closed");
    let message = error.to_string();
    assert!(
        message.contains("E-1"),
        "refusal names the execution, got {message}"
    );
    // Nothing healed, nothing consumed: the strand is untouched.
    let store = env.store();
    assert!(store
        .get_execution(&ExecutionId::new("E-1").unwrap())
        .expect("read")
        .expect("execution")
        .attempts()
        .is_empty());
    assert_eq!(env.task_state("T-1"), TaskState::Running);
}

#[test]
fn resume_plan_reports_allocated_then_ready_lease_state() {
    let env = Env::named("plan-lease");
    env.seed_ready("P-1", "T-1");
    let repo = env.dir.join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let _work = admit_and_crash(&env, "T-1", "E-1", "A-1");

    let mut workspace = workspace_service(&env);
    let lease = workspace
        .allocate_workspace(&workspace_request("E-1", &repo, "repo-local:plan"))
        .expect("allocate");
    assert_eq!(lease.lifecycle(), workspace::LeaseLifecycle::Allocated);

    let orchestration = env.open_orchestration();
    let plan = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect("allocated lease stays recoverable");
    assert_eq!(
        plan.repository_ref.as_deref(),
        Some("repo-local:plan"),
        "lease-backed recovery reuses the recorded repository identity"
    );
    assert!(
        plan.base_revision
            .as_deref()
            .is_some_and(|rev| rev.len() == 40),
        "lease-backed recovery reuses the recorded base OID"
    );

    workspace
        .materialize_allocated(&lease, &repo.to_string_lossy())
        .expect("materialize");
    workspace
        .finalize_ready(lease.id(), &repo.to_string_lossy())
        .expect("finalize");
    let orchestration = env.open_orchestration();
    let plan = orchestration
        .plan_pre_attempt_resume(&ExecutionId::new("E-1").unwrap())
        .expect("ready lease stays recoverable");
    assert_eq!(plan.work.attempt_id.as_str(), "A-1");
    assert!(plan.repository_ref.is_some() && plan.base_revision.is_some());
}

#[test]
fn stranded_scan_lists_only_pre_attempt_entries() {
    let env = Env::named("scan");
    env.seed_ready("P-1", "T-1");
    env.seed_ready("P-2", "T-2");
    // T-1/E-1: stranded (no attempt). T-2/E-2: admitted then attempted
    // (reservation consumed) — the scan must not report it.
    let _stranded = admit_and_crash(&env, "T-1", "E-1", "A-1");
    let work = admit_and_crash(&env, "T-2", "E-2", "A-2");
    let mut orchestration = env.open_orchestration();
    orchestration
        .runtime_mut()
        .create_attempt(
            &work.execution_id,
            work.attempt_id.clone(),
            agent_runtime::ProviderRef::new("test-executor").unwrap(),
        )
        .expect("consume");

    let store = env.store();
    let stranded = store.list_pre_attempt_stranded_executions().expect("scan");
    assert_eq!(
        stranded.len(),
        1,
        "only the pre-attempt entry, got {stranded:?}"
    );
    assert_eq!(stranded[0].execution_id.as_str(), "E-1");
    assert_eq!(stranded[0].task_id.as_str(), "T-1");
    assert_eq!(stranded[0].attempt_id.as_str(), "A-1");

    // Event-type census: recovery planning adds no event types; the
    // strand carries exactly the admission triple.
    for event_type in env.journal_types("T-1") {
        assert!(
            [
                "task.created",
                "task.marked_ready",
                "task.claimed",
                "task.started",
                "execution.created",
            ]
            .contains(&event_type.as_str()),
            "unexpected journal fact on a strand: {event_type}"
        );
    }
    let kinds: Vec<EventType> = EventType::all().to_vec();
    assert_eq!(
        kinds.len(),
        18,
        "recovery adds no event types beyond the canonical taxonomy"
    );
}
