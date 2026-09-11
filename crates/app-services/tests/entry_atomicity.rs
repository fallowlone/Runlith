//! Atomic execution-entry keepers (TASK-0051, G5).
//!
//! TASK-0050 proved the TASK-0049 preflight is TOCTOU-prone: two
//! barrier-synchronized `enter_execution` calls with the same new
//! execution (or attempt) identity both passed the read-before-write
//! check, and the loser was left `RUNNING` with committed
//! `Claimed`/`Started` journal facts but no Execution — non-rerunnable.
//! Entry is now one `IMMEDIATE` transaction (claim + start + execution +
//! attempt reservation + journal), so exactly one entrant wins and every
//! loser rolls back completely.
//!
//! Each keeper runs its race repeatedly (bounded iterations) through two
//! separate service handles over separate SQLite connections — the same
//! shape as two concurrent `run` IPCs — and asserts DURABLE loser state
//! every iteration, never return codes alone. SQLite-only: no Git, no
//! provider, portable.
//!
//! RED → GREEN note: these keepers fail on the TASK-0050 implementation
//! (loser left RUNNING, 6/6 in the audit probe) and pass with atomic
//! admission.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::{AttemptId, ExecutionId};
use app_services::{
    ExecutionEntry, OrchestrationIds, OrchestrationService, ServiceError, SystemClock, TaskService,
};
use storage_sqlite::{EventType, SqliteStore, StorageError};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};
use workspace::WorkspaceStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Bounded race repetitions. Enough to exercise serialization every run
/// without dominating suite time (each iteration is a few milliseconds
/// of SQLite on a fresh file).
const EXECUTION_ID_RACE_ITERATIONS: u64 = 50;
const ATTEMPT_ID_RACE_ITERATIONS: u64 = 50;
const SAME_TASK_RACE_ITERATIONS: u64 = 50;
const EXACT_DUPLICATE_RACE_ITERATIONS: u64 = 25;

struct Env {
    dir: PathBuf,
    db: PathBuf,
}

impl Env {
    fn named(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-entry-atomicity-{suite}-{id}-{}",
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
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

type RaceResult = Result<ExecutionEntry, ServiceError>;

/// One racer's entry attempt through a private service handle (its own
/// SQLite connection), released by the shared barrier together with its
/// rival — the same shape as two concurrent `run` IPCs.
fn race_enter(
    db: PathBuf,
    task: String,
    execution: String,
    attempt: String,
    barrier: std::sync::Arc<std::sync::Barrier>,
) -> RaceResult {
    barrier.wait();
    let mut orchestration = OrchestrationService::open(&db, SystemClock).expect("open");
    orchestration.enter_execution(
        &TaskId::new(task).unwrap(),
        OrchestrationIds {
            execution_id: ExecutionId::new(execution).unwrap(),
            attempt_id: AttemptId::new(attempt).unwrap(),
        },
    )
}

fn spawn_racer(
    db: PathBuf,
    task: String,
    execution: String,
    attempt: String,
    barrier: &std::sync::Arc<std::sync::Barrier>,
) -> std::thread::JoinHandle<RaceResult> {
    let barrier = barrier.clone();
    std::thread::spawn(move || race_enter(db, task, execution, attempt, barrier))
}

fn is_runnable(result: &RaceResult) -> bool {
    matches!(result, Ok(ExecutionEntry::Runnable(_)))
}

fn is_typed_conflict(error: &ServiceError) -> bool {
    // Loser contract: a classified conflict (duplicate identity or stale
    // task state) or a domain lifecycle refusal — never a raw SQLite
    // failure surfacing as the primary contract.
    matches!(
        error,
        ServiceError::Storage(StorageError::Duplicate(_))
            | ServiceError::Storage(StorageError::Stale { .. })
            | ServiceError::Domain(_)
    )
}

/// Asserts the loser of an entry race kept honest durable state:
/// pre-run task, no attempt, no lifecycle journal facts — while the
/// winner's execution stayed owned by the winner task. Attempt
/// reservations are asserted by the caller: a distinct loser identity
/// must be unreserved, while a shared attempt identity is owned by the
/// winner (see below).
fn assert_loser_honest(
    env: &Env,
    round: u64,
    loser_task: &str,
    loser_attempt: &str,
    winner_execution: &str,
    winner_task: &str,
) {
    let store = env.store();
    let task = store
        .get_task(&TaskId::new(loser_task).unwrap())
        .expect("read task")
        .expect("loser task exists");
    assert_eq!(
        task.state(),
        TaskState::Ready,
        "round {round}: loser task {loser_task} must stay pre-run"
    );
    assert!(
        store
            .get_attempt(&AttemptId::new(loser_attempt).unwrap())
            .expect("read attempt")
            .is_none(),
        "round {round}: no attempt may exist for the loser identity {loser_attempt}"
    );
    for event in store
        .list_events_for_task(&TaskId::new(loser_task).unwrap())
        .expect("journal")
    {
        assert!(
            !matches!(
                event.fact.event_type,
                EventType::Claimed
                    | EventType::Started
                    | EventType::ExecutionCreated
                    | EventType::AttemptCreated
                    | EventType::WorkspaceCreated
            ),
            "round {round}: loser task {loser_task} must carry no lifecycle facts, got {:?}",
            event.fact.event_type
        );
    }
    let execution = store
        .get_execution(&ExecutionId::new(winner_execution).unwrap())
        .expect("read execution")
        .expect("winner execution exists");
    assert_eq!(
        execution.task_id().to_string(),
        winner_task,
        "round {round}: winner execution must stay owned by the winner task"
    );
}

/// Asserts a task journal is exactly the admitted entry triple on top of
/// its creation facts.
fn assert_winner_journal(env: &Env, round: u64, winner_task: &str) {
    let store = env.store();
    let kinds: Vec<EventType> = store
        .list_events_for_task(&TaskId::new(winner_task).unwrap())
        .expect("journal")
        .into_iter()
        .map(|event| event.fact.event_type)
        .collect();
    assert_eq!(
        kinds,
        vec![
            EventType::TaskCreated,
            EventType::MarkedReady,
            EventType::Claimed,
            EventType::Started,
            EventType::ExecutionCreated,
        ],
        "round {round}: winner journal must be exactly the admitted entry triple"
    );
}

fn task_state(env: &Env, task: &str) -> TaskState {
    env.store()
        .get_task(&TaskId::new(task).unwrap())
        .expect("read")
        .expect("exists")
        .state()
}

/// Splits two race results into exactly one runnable winner plus the
/// loser's typed conflict. Panics otherwise — both-admitted and
/// both-refused are entry-boundary failures.
fn split_race(
    round: u64,
    context: &str,
    result_a: RaceResult,
    result_b: RaceResult,
) -> (RaceResult, ServiceError) {
    match (result_a, result_b) {
        (Ok(entry), Err(error)) if matches!(entry, ExecutionEntry::Runnable(_)) => {
            assert!(
                is_typed_conflict(&error),
                "round {round} ({context}): loser must receive a typed conflict, got {error:?}"
            );
            (Ok(entry), error)
        }
        (Err(error), Ok(entry)) if matches!(entry, ExecutionEntry::Runnable(_)) => {
            assert!(
                is_typed_conflict(&error),
                "round {round} ({context}): loser must receive a typed conflict, got {error:?}"
            );
            (Ok(entry), error)
        }
        (first, second) => {
            panic!("round {round} ({context}): entry boundary failure: {first:?} vs {second:?}")
        }
    }
}

/// TASK-0051 B1-concurrent keeper: two tasks race on the SAME new
/// execution identity. Exactly one entry is admitted; the loser keeps a
/// runnable task and retries cleanly with fresh identities.
#[test]
fn concurrent_duplicate_execution_id_leaves_loser_honest() {
    let env = Env::named("exec-race");
    for round in 0..EXECUTION_ID_RACE_ITERATIONS {
        let task_a = format!("T-A-{round}");
        let task_b = format!("T-B-{round}");
        let shared_execution = format!("E-SHARED-{round}");
        let attempt_a = format!("A-A-{round}");
        let attempt_b = format!("A-B-{round}");
        env.seed_ready(&format!("P-A-{round}"), &task_a);
        env.seed_ready(&format!("P-B-{round}"), &task_b);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handle_a = spawn_racer(
            env.db.clone(),
            task_a.clone(),
            shared_execution.clone(),
            attempt_a.clone(),
            &barrier,
        );
        let handle_b = spawn_racer(
            env.db.clone(),
            task_b.clone(),
            shared_execution.clone(),
            attempt_b.clone(),
            &barrier,
        );
        let result_a = handle_a.join().expect("racer A");
        let result_b = handle_b.join().expect("racer B");

        // Winner is whoever admitted the shared execution; the loser is
        // the task that does NOT own it afterwards.
        let (_winner, _loser_error) = split_race(round, "exec-id", result_a, result_b);
        let store = env.store();
        let owner = store
            .get_execution(&ExecutionId::new(&shared_execution).unwrap())
            .expect("read execution")
            .expect("winner execution exists");
        let (winner_task, loser_task, loser_attempt) = if owner.task_id().to_string() == task_a {
            (task_a.clone(), task_b.clone(), attempt_b.clone())
        } else {
            (task_b.clone(), task_a.clone(), attempt_a.clone())
        };
        assert_loser_honest(
            &env,
            round,
            &loser_task,
            &loser_attempt,
            &shared_execution,
            &winner_task,
        );
        assert_winner_journal(&env, round, &winner_task);
        // The loser's own attempt identity was never reserved.
        assert!(
            env.store()
                .get_attempt_reservation(&AttemptId::new(&loser_attempt).unwrap())
                .expect("read reservation")
                .is_none(),
            "round {round}: no attempt reservation may be owned by the loser identity {loser_attempt}"
        );

        // The loser retries cleanly with fresh identities.
        let mut orchestration = env.open_orchestration();
        let retry = orchestration.enter_execution(
            &TaskId::new(&loser_task).unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new(format!("E-RETRY-{round}")).unwrap(),
                attempt_id: AttemptId::new(format!("A-RETRY-{round}")).unwrap(),
            },
        );
        assert!(
            matches!(retry, Ok(ExecutionEntry::Runnable(_))),
            "round {round}: loser must be rerunnable with fresh identities, got {retry:?}"
        );
        assert_eq!(task_state(&env, &loser_task), TaskState::Running);
    }
}

/// TASK-0051 attempt-ID keeper: two tasks race on the SAME new attempt
/// identity with distinct execution identities. Exactly one entry owns
/// the attempt reservation; the loser is refused before any workspace or
/// Git effect with a runnable task.
#[test]
fn concurrent_duplicate_attempt_id_leaves_loser_honest() {
    let env = Env::named("attempt-race");
    for round in 0..ATTEMPT_ID_RACE_ITERATIONS {
        let task_a = format!("T-A-{round}");
        let task_b = format!("T-B-{round}");
        let execution_a = format!("E-A-{round}");
        let execution_b = format!("E-B-{round}");
        let shared_attempt = format!("A-SHARED-{round}");
        env.seed_ready(&format!("P-A-{round}"), &task_a);
        env.seed_ready(&format!("P-B-{round}"), &task_b);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handle_a = spawn_racer(
            env.db.clone(),
            task_a.clone(),
            execution_a.clone(),
            shared_attempt.clone(),
            &barrier,
        );
        let handle_b = spawn_racer(
            env.db.clone(),
            task_b.clone(),
            execution_b.clone(),
            shared_attempt.clone(),
            &barrier,
        );
        let result_a = handle_a.join().expect("racer A");
        let result_b = handle_b.join().expect("racer B");
        let (_winner, _loser_error) = split_race(round, "attempt-id", result_a, result_b);

        let store = env.store();
        let (owner, _) = store
            .get_attempt_reservation(&AttemptId::new(&shared_attempt).unwrap())
            .expect("read reservation")
            .expect("winner reservation exists");
        let (winner_task, winner_execution, loser_task, loser_execution) =
            if owner.as_str() == execution_a {
                (
                    task_a.clone(),
                    execution_a.clone(),
                    task_b.clone(),
                    execution_b.clone(),
                )
            } else {
                (
                    task_b.clone(),
                    execution_b.clone(),
                    task_a.clone(),
                    execution_a.clone(),
                )
            };
        // Loser owns nothing: no execution, no lease, clean journal.
        assert_loser_honest(
            &env,
            round,
            &loser_task,
            &shared_attempt,
            &winner_execution,
            &winner_task,
        );
        assert!(
            store
                .get_execution(&ExecutionId::new(&loser_execution).unwrap())
                .expect("read execution")
                .is_none(),
            "round {round}: refused entry must not create execution {loser_execution}"
        );
        assert!(
            store
                .get_workspace_for_execution(&ExecutionId::new(&loser_execution).unwrap())
                .expect("read lease")
                .is_none(),
            "round {round}: refused entry must not allocate a lease"
        );

        // The loser retries cleanly with a fresh attempt identity.
        let mut orchestration = env.open_orchestration();
        let retry = orchestration.enter_execution(
            &TaskId::new(&loser_task).unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new(format!("E-RETRY-{round}")).unwrap(),
                attempt_id: AttemptId::new(format!("A-RETRY-{round}")).unwrap(),
            },
        );
        assert!(
            matches!(retry, Ok(ExecutionEntry::Runnable(_))),
            "round {round}: loser must be rerunnable with fresh identities, got {retry:?}"
        );
    }
}

/// TASK-0051 same-task keeper: one task with two concurrent run requests
/// and distinct identities. Only one transitions the task out of READY;
/// the loser carries no duplicate lifecycle facts.
#[test]
fn concurrent_same_task_entry_admits_single_winner() {
    let env = Env::named("same-task");
    for round in 0..SAME_TASK_RACE_ITERATIONS {
        let task = format!("T-{round}");
        env.seed_ready(&format!("P-{round}"), &task);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handle_a = spawn_racer(
            env.db.clone(),
            task.clone(),
            format!("E-A-{round}"),
            format!("A-A-{round}"),
            &barrier,
        );
        let handle_b = spawn_racer(
            env.db.clone(),
            task.clone(),
            format!("E-B-{round}"),
            format!("A-B-{round}"),
            &barrier,
        );
        let result_a = handle_a.join().expect("racer A");
        let result_b = handle_b.join().expect("racer B");

        // Exactly one winner. The loser is either still-observable
        // pre-commit (typed conflict) or post-commit (not-runnable) —
        // both are honest: neither commits facts for the loser.
        let runnable = [&result_a, &result_b]
            .iter()
            .filter(|result| is_runnable(result))
            .count();
        assert_eq!(
            runnable, 1,
            "round {round}: exactly one entry may leave READY: {result_a:?} vs {result_b:?}"
        );
        for result in [&result_a, &result_b] {
            match result {
                Ok(ExecutionEntry::Runnable(_)) => {}
                Ok(ExecutionEntry::NotRunnable(_)) => {}
                Err(error) => assert!(
                    is_typed_conflict(error),
                    "round {round}: loser must be typed, got {error:?}"
                ),
            }
        }
        // The task carries exactly one claim/start pair and exactly one
        // execution exists across both candidate identities.
        let store = env.store();
        let kinds: Vec<EventType> = store
            .list_events_for_task(&TaskId::new(&task).unwrap())
            .expect("journal")
            .into_iter()
            .map(|event| event.fact.event_type)
            .collect();
        let claims = kinds
            .iter()
            .filter(|kind| **kind == EventType::Claimed)
            .count();
        let starts = kinds
            .iter()
            .filter(|kind| **kind == EventType::Started)
            .count();
        assert_eq!(
            (claims, starts),
            (1, 1),
            "round {round}: duplicate lifecycle facts: {kinds:?}"
        );
        let executions = [format!("E-A-{round}"), format!("E-B-{round}")]
            .into_iter()
            .filter(|id| {
                store
                    .get_execution(&ExecutionId::new(id).unwrap())
                    .expect("read")
                    .is_some()
            })
            .count();
        assert_eq!(
            executions, 1,
            "round {round}: exactly one execution may exist"
        );
        assert_eq!(task_state(&env, &task), TaskState::Running);
    }
}

/// TASK-0051 exact-duplicate keeper: same task, same execution, same
/// attempt, two simultaneous requests. One wins; the loser introduces no
/// additional durable mutation. There is no idempotent-replay path — a
/// same-identity retry stays a refusal, documented here by contract.
#[test]
fn concurrent_exact_duplicate_entry_is_honest() {
    let env = Env::named("exact-dup");
    for round in 0..EXACT_DUPLICATE_RACE_ITERATIONS {
        let task = format!("T-{round}");
        let execution = format!("E-{round}");
        let attempt = format!("A-{round}");
        env.seed_ready(&format!("P-{round}"), &task);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handle_a = spawn_racer(
            env.db.clone(),
            task.clone(),
            execution.clone(),
            attempt.clone(),
            &barrier,
        );
        let handle_b = spawn_racer(
            env.db.clone(),
            task.clone(),
            execution.clone(),
            attempt.clone(),
            &barrier,
        );
        let result_a = handle_a.join().expect("racer A");
        let result_b = handle_b.join().expect("racer B");
        let (_winner, _loser_error) = split_race(round, "exact-dup", result_a, result_b);
        // No additional facts beyond the single admitted entry.
        assert_winner_journal(&env, round, &task);

        // Same-identity retry stays a refusal (no transparent replay).
        let mut orchestration = env.open_orchestration();
        let retry = orchestration.enter_execution(
            &TaskId::new(&task).unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new(&execution).unwrap(),
                attempt_id: AttemptId::new(&attempt).unwrap(),
            },
        );
        assert!(
            !matches!(retry, Ok(ExecutionEntry::Runnable(_))),
            "round {round}: same-identity retry must not admit a second entry, got {retry:?}"
        );
    }
}

/// Sequential regression pin (TASK-0048 shapes, still enforced): an
/// already-taken execution identity and an already-taken attempt identity
/// refuse before any mutation, through the same atomic boundary.
#[test]
fn sequential_identity_conflicts_refuse_before_mutation() {
    let env = Env::named("sequential");
    env.seed_ready("P-A", "T-A");
    env.seed_ready("P-B", "T-B");

    let mut orchestration = env.open_orchestration();
    let admitted = orchestration
        .enter_execution(
            &TaskId::new("T-A").unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new("E-A").unwrap(),
                attempt_id: AttemptId::new("A-A").unwrap(),
            },
        )
        .expect("first entry admits");
    assert!(matches!(admitted, ExecutionEntry::Runnable(_)));

    // Existing execution identity.
    let refused = orchestration.enter_execution(
        &TaskId::new("T-B").unwrap(),
        OrchestrationIds {
            execution_id: ExecutionId::new("E-A").unwrap(),
            attempt_id: AttemptId::new("A-B").unwrap(),
        },
    );
    assert!(
        matches!(
            refused,
            Err(ServiceError::Storage(StorageError::Duplicate(_)))
        ),
        "existing execution identity must refuse typed, got {refused:?}"
    );
    assert_eq!(task_state(&env, "T-B"), TaskState::Ready);

    // Reserved (not yet created) attempt identity.
    let refused = orchestration.enter_execution(
        &TaskId::new("T-B").unwrap(),
        OrchestrationIds {
            execution_id: ExecutionId::new("E-B").unwrap(),
            attempt_id: AttemptId::new("A-A").unwrap(),
        },
    );
    assert!(
        matches!(
            refused,
            Err(ServiceError::Storage(StorageError::Duplicate(_)))
        ),
        "reserved attempt identity must refuse typed, got {refused:?}"
    );
    assert_eq!(task_state(&env, "T-B"), TaskState::Ready);
    let store = env.store();
    assert!(
        store
            .get_execution(&ExecutionId::new("E-B").unwrap())
            .expect("read")
            .is_none(),
        "refused entry must not create its execution"
    );
}
