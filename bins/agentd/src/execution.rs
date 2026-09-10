//! G5 execution composition root (TASK-0047): the production wiring from
//! versioned IPC to the real provider through the trusted application
//! services.
//!
//! ```text
//! agentctl
//!  → versioned local IPC (`execution_core`, protocol v2 only)
//!  → agentd `serve_connection` (this composition root)
//!  → OrchestrationService::enter_execution (durable entry, synchronous)
//!  → background thread: WorkspaceService lease → ClaudeExecutor
//!    (default real provider) → attempt → submit → deferred verification
//!  → durable state observable via status, signallable via cancel
//! ```
//!
//! Composition owns: the [`LiveProcessTracker`] shared between the
//! provider executor and the Git backend, the per-execution
//! [`CancelRegistry`], the [`PendingEvaluator`] (G5 performs no automated
//! verification — provider success leaves the Task VERIFYING for the
//! operator/G8 path), and the provider executable/timeout configuration.
//!
//! Composition never owns: Task lifecycle rules, scheduling decisions,
//! completion authority, prompt construction policy beyond the accepted
//! adapter derivation, or secrets of any kind. Provider identity never
//! branches domain logic: the configured default [`ExecutionPort`]
//! implementation happens to be Claude today; a second provider changes
//! only this module plus its adapter crate.
//!
//! Sync vs async (TASK-0047 §11): `run` commits the fast durable entry
//! synchronously and returns the identities immediately; the slow
//! provider half runs on a detached background thread. The IPC
//! connection never carries the provider session: a client disconnect
//! cannot redefine the outcome, `status` is independently queryable, and
//! `cancel` targets the durable execution identity. The background thread
//! is intentionally detached (not joined on shutdown): daemon shutdown or
//! restart orphans the provider child and strands the attempt as
//! reconcilable open state — never as silent success. Full recovery is
//! G11 work, explicitly deferred.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_runtime::{AttemptOutcome, ExecutionId};
use app_services::{
    ExecutionEntry, ManagedWorkspaceRoot, OrchestrationIds, OrchestrationService, WorkspaceRequest,
    WorkspaceService,
};
use evaluation::{EvaluationPort, EvaluationRequest, EvaluationResult, EvaluationVerdict};
use ipc_contracts::proto::{
    execution_core_request, execution_core_response, AttemptView, BusinessError, BusinessErrorCode,
    CancelExecutionRequest, CancelExecutionResponse, ExecutionCoreRequest, ExecutionCoreResponse,
    GetExecutionRequest, GetExecutionResponse, RunExecutionRequest, RunExecutionResponse,
};
use scheduler::OrchestrationOutcome;
use task_domain::{TaskId, TaskState};
use workspace::{RepositoryRef, WorkspaceStore};

/// Wall-clock bound for one production provider execution. Matches the
/// adapter default: the daemon does not invent its own timeout policy.
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(300);

/// Capture-finalization grace after natural provider exit. Matches the
/// adapter default; see `provider-claude` process supervision.
pub const CAPTURE_GRACE: Duration = Duration::from_secs(5);

/// Bound for server-local path inputs on the wire (repository paths).
const MAX_PATH_LEN: usize = 4096;

/// Bound for revision-request inputs on the wire.
const MAX_REVISION_LEN: usize = 1024;

/// Bound for attempt detail echoed in status responses.
const MAX_STATUS_DETAIL_CHARS: usize = 1024;

/// Monotonic mint for server-generated execution/attempt identities.
static MINT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Per-execution cancellation flags owned by the composition root. One
/// entry per background run, from `run` entry until the terminal outcome
/// is committed. Signalling kills only the registered run's owned child
/// tree (via the executor's shared flag) — never by process name.
#[derive(Debug, Clone, Default)]
pub struct CancelRegistry {
    inner: Arc<Mutex<HashMap<String, provider_claude::CancelFlag>>>,
}

impl CancelRegistry {
    /// Empty registry.
    #[must_use]
    pub fn fresh() -> Self {
        Self::default()
    }

    /// Registers the live flag for one execution.
    pub fn register(&self, execution: &ExecutionId, flag: provider_claude::CancelFlag) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.insert(execution.as_str().to_owned(), flag);
        }
    }

    /// Retires the flag for one execution. Idempotent.
    pub fn unregister(&self, execution: &ExecutionId) {
        self.unregister_id(execution.as_str());
    }

    /// Retires the flag for one raw execution identity. Idempotent.
    pub fn unregister_id(&self, execution_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.remove(execution_id);
        }
    }

    /// Whether a background run is currently registered for `execution`.
    #[must_use]
    pub fn is_live(&self, execution: &ExecutionId) -> bool {
        self.inner
            .lock()
            .is_ok_and(|inner| inner.contains_key(execution.as_str()))
    }

    /// Signals the live run for `execution`. Returns false when no live
    /// run is registered (already concluded, never started, or stranded
    /// by a restart) — the caller then reports durable state instead.
    pub fn signal(&self, execution: &ExecutionId) -> bool {
        let flag = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.get(execution.as_str()).cloned());
        match flag {
            Some(flag) => {
                flag.cancel();
                true
            }
            None => false,
        }
    }
}

/// G5 verification posture: there is no automated verifier yet (G8 owns
/// real verifiers behind this same seam). Returning `Err` means no
/// verdict was reached, so a provider success leaves the Task VERIFYING
/// with its evidence intact — the operator verifies explicitly via the
/// trusted `task verify-succeed/verify-fail` path. Provider output can
/// therefore never complete a Task, no matter what it claims.
#[derive(Debug, Clone, Copy, Default)]
pub struct PendingEvaluator;

impl EvaluationPort for PendingEvaluator {
    fn evaluate(&self, request: &EvaluationRequest) -> EvaluationResult<EvaluationVerdict> {
        Err(evaluation::EvaluationError::new(format!(
            "no automated verifier in G5; task {} awaits operator verification (execution {}, attempt {})",
            request.task_id, request.execution_id, request.attempt_id,
        )))
    }
}

/// Shared production execution composition, built once per daemon from
/// [`crate::DaemonConfig`]. Clone-shares the tracker and cancel table.
#[derive(Debug, Clone)]
pub struct ExecutionComposition {
    db_path: PathBuf,
    cancels: CancelRegistry,
    tracker: provider_claude::LiveProcessTracker,
    /// Composition-owned provider executable override. `None` resolves
    /// `claude` via `PATH`; `Some` points at an explicit binary (tests
    /// inject fixture doubles). Never client-supplied — no IPC field
    /// maps here, so a client cannot turn the daemon into an arbitrary
    /// process launcher.
    provider_executable: Option<PathBuf>,
}

impl ExecutionComposition {
    /// Builds the composition for one database. No I/O: the managed
    /// workspace root is ensured per `run` (fail fast, before mutation).
    pub fn new(db_path: PathBuf, provider_executable: Option<PathBuf>) -> Self {
        Self {
            db_path,
            cancels: CancelRegistry::fresh(),
            tracker: provider_claude::LiveProcessTracker::fresh(),
            provider_executable,
        }
    }

    /// The durable database this composition works against.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }
}

/// Shared handle type for the serve loop.
pub type ExecutionHandle = Arc<ExecutionComposition>;

/// A committed background run: the daemon entry is durable, the provider
/// half still has to execute. Spawned detached by the serve loop.
pub struct BackgroundJob {
    composition: ExecutionComposition,
    work: scheduler::WorkItem,
    workspace_request: WorkspaceRequest,
    cancel: provider_claude::CancelFlag,
}

impl BackgroundJob {
    /// Durable execution identity this job works for (thread naming,
    /// logging). The composition already committed it.
    #[must_use]
    pub fn work_execution_id(&self) -> String {
        self.work.execution_id.to_string()
    }
}

/// Builds the error response when the serve loop cannot spawn the
/// background thread (resource exhaustion): the durable entry is already
/// committed, so the response names the execution for recovery via
/// `status` instead of pretending nothing happened. Also unregisters
/// the cancel flag — there is no live run to signal.
pub fn abort_start(
    composition: &ExecutionComposition,
    execution_id: &str,
) -> ExecutionCoreResponse {
    composition.cancels.unregister_id(execution_id);
    eprintln!(
        "agentd: background spawn failed for execution {execution_id}; entry committed without a run"
    );
    exec_error(
        BusinessErrorCode::Internal,
        format!(
            "background run could not start for execution {}; inspect status to recover",
            truncate_id(execution_id)
        ),
    )
}

/// Dispatches one decoded execution request. `run` may yield a background
/// job for the caller to spawn detached; `get`/`cancel` are fully
/// synchronous reads/signals.
pub fn dispatch_execution(
    composition: &ExecutionComposition,
    request: ExecutionCoreRequest,
) -> (ExecutionCoreResponse, Option<BackgroundJob>) {
    match request.payload {
        Some(execution_core_request::Payload::Run(req)) => start_run(composition, req),
        Some(execution_core_request::Payload::Get(req)) => (handle_get(composition, req), None),
        Some(execution_core_request::Payload::Cancel(req)) => {
            (handle_cancel(composition, req), None)
        }
        None => (
            exec_error(BusinessErrorCode::Internal, "empty execution request"),
            None,
        ),
    }
}

/// Typed unavailable response for control-only daemon compositions (no
/// database): the version gate passed, but no execution composition
/// exists to handle the request.
#[must_use]
pub fn execution_unavailable() -> ExecutionCoreResponse {
    exec_error(
        BusinessErrorCode::Internal,
        "execution service unavailable (daemon has no database)",
    )
}

/// Runs the background half of one committed run to its terminal outcome.
/// Blocking: call on a dedicated thread, never on the async serve loop.
/// Unregisters the cancel flag on every path (success, failure, lease
/// refusal, bind refusal) so `status`/`cancel` observe honest state.
pub fn run_background(job: BackgroundJob) {
    let BackgroundJob {
        composition,
        work,
        workspace_request,
        cancel,
    } = job;
    let execution_id = workspace_request.execution_id.clone();
    let attempt_id = work.attempt_id.clone();
    match run_background_inner(&composition, work, &workspace_request, &cancel) {
        Ok(outcome) => {
            composition.cancels.unregister(&execution_id);
            eprintln!(
                "agentd: execution {execution_id} concluded: {}",
                outcome_tag(&outcome)
            );
        }
        Err(detail) => {
            composition.cancels.unregister(&execution_id);
            // G5 posture, not a failure of evidence: provider success
            // with no automated verifier commits attempt Succeeded plus
            // Task VERIFYING, then reports the absent verdict as an
            // error. Re-read durable state instead of trusting the
            // message: a concluded-Succeeded attempt means the cycle did
            // its job and awaits the operator.
            if attempt_concluded_succeeded(&composition.db_path, &execution_id, &attempt_id) {
                eprintln!(
                    "agentd: execution {execution_id} concluded: SUCCEEDED_AWAITING_OPERATOR_VERIFICATION"
                );
            } else {
                eprintln!(
                    "agentd: execution {execution_id} concluded with error: {}",
                    truncate_detail(&detail)
                );
            }
        }
    }
}

/// Reads back whether one attempt concluded Succeeded. Best-effort
/// log-classification only: any read failure means "not provably
/// succeeded", which is the safe direction for an error log.
fn attempt_concluded_succeeded(
    db_path: &Path,
    execution_id: &ExecutionId,
    attempt_id: &agent_runtime::AttemptId,
) -> bool {
    let store = match storage_sqlite::SqliteStore::open(db_path) {
        Ok(store) => store,
        Err(_) => return false,
    };
    match store.get_execution(execution_id) {
        Ok(Some(execution)) => execution
            .get_attempt(attempt_id)
            .is_some_and(|attempt| attempt.outcome() == Some(AttemptOutcome::Succeeded)),
        _ => false,
    }
}

fn run_background_inner(
    composition: &ExecutionComposition,
    work: scheduler::WorkItem,
    workspace_request: &WorkspaceRequest,
    cancel: &provider_claude::CancelFlag,
) -> Result<OrchestrationOutcome, String> {
    let mut orchestration =
        OrchestrationService::open(&composition.db_path, app_services::SystemClock)
            .map_err(|error| format!("open orchestration: {error}"))?;
    let root = ensure_managed_root(&composition.db_path)?;
    let git = git_cli::GitCli::new()
        .with_liveness(Arc::new(composition.tracker.clone()) as Arc<dyn workspace::OwnerLiveness>);
    let mut workspace =
        WorkspaceService::open(&composition.db_path, git, root, app_services::SystemClock)
            .map_err(|error| format!("open workspace service: {error}"))?;
    let tracker = composition.tracker.clone();
    let executable = composition.provider_executable.clone();
    let cancel = cancel.clone();
    orchestration
        .continue_workspace_cycle(
            &mut workspace,
            work,
            workspace_request.clone(),
            |task, lease| {
                let prompt =
                    provider_claude::build_task_prompt(task.objective(), task.acceptance());
                let mut config = provider_claude::ClaudeConfig::default();
                if let Some(exe) = executable {
                    config = config
                        .with_executable(exe)
                        .expect("composition executable override is validated at startup");
                }
                let config = config
                    .with_timeout(PROVIDER_TIMEOUT)
                    .expect("constant provider timeout is non-zero")
                    .with_capture_grace(CAPTURE_GRACE)
                    .expect("constant capture grace is non-zero");
                provider_claude::ClaudeExecutor::bind(
                    config,
                    prompt,
                    lease.worktree_ref().to_owned(),
                )
                .expect("lease-bound prompt and workdir are non-blank")
                .with_tracker(tracker)
                .with_cancel_flag(cancel)
            },
            &PendingEvaluator,
        )
        .map_err(|error| format!("orchestration cycle: {error}"))
}

/// Short outcome tag for daemon logs: identities plus the variant only.
/// Provider text never enters daemon logs.
fn outcome_tag(outcome: &OrchestrationOutcome) -> &'static str {
    match outcome {
        OrchestrationOutcome::NotRunnable { .. } => "NOT_RUNNABLE",
        OrchestrationOutcome::WaitingOnDependency { .. } => "WAITING_ON_DEPENDENCY",
        OrchestrationOutcome::Cancelled { .. } => "CANCELLED",
        OrchestrationOutcome::ExecutionFailed { .. } => "EXECUTION_FAILED",
        OrchestrationOutcome::ExecutionSucceededAndVerified { .. } => {
            "EXECUTION_SUCCEEDED_AND_VERIFIED"
        }
        OrchestrationOutcome::ExecutionSucceededButRejected { .. } => {
            "EXECUTION_SUCCEEDED_BUT_REJECTED"
        }
    }
}

// -- run -----------------------------------------------------------------

/// Validates a `run` request, fails the provider precheck fast, commits
/// the durable entry, registers the cancel flag, and returns the accepted
/// identities plus the background job. Nothing provider-side runs here.
fn start_run(
    composition: &ExecutionComposition,
    req: RunExecutionRequest,
) -> (ExecutionCoreResponse, Option<BackgroundJob>) {
    let task_id = match TaskId::new(req.task_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                exec_error(
                    BusinessErrorCode::InvalidArgument,
                    "task id must be a non-blank bounded identifier",
                ),
                None,
            );
        }
    };
    let execution_id = match mint_or_validate_execution(&req.execution_id) {
        Ok(id) => id,
        Err(detail) => return (exec_error(BusinessErrorCode::InvalidArgument, detail), None),
    };
    let attempt_id = match mint_or_validate_attempt(&req.attempt_id) {
        Ok(id) => id,
        Err(detail) => return (exec_error(BusinessErrorCode::InvalidArgument, detail), None),
    };
    let repository_ref = match RepositoryRef::new(req.repository_ref) {
        Ok(reference) => reference,
        Err(_) => {
            return (
                exec_error(
                    BusinessErrorCode::InvalidArgument,
                    "repository ref must be a non-blank bounded identifier",
                ),
                None,
            );
        }
    };
    if req.repository_path.trim().is_empty() || req.repository_path.len() > MAX_PATH_LEN {
        return (
            exec_error(
                BusinessErrorCode::InvalidArgument,
                "repository path must be non-blank bounded text",
            ),
            None,
        );
    }
    let base_revision = if req.base_revision.trim().is_empty() {
        "HEAD".to_owned()
    } else if req.base_revision.len() > MAX_REVISION_LEN {
        return (
            exec_error(
                BusinessErrorCode::InvalidArgument,
                "base revision exceeds the bounded length",
            ),
            None,
        );
    } else {
        req.base_revision.clone()
    };

    // Fail fast before any mutation: a missing provider executable is a
    // typed operational failure, never a daemon crash and never a
    // committed execution that can only fail in the background.
    if let Err(detail) = provider_precheck(composition) {
        return (
            exec_error(BusinessErrorCode::ProviderUnavailable, detail),
            None,
        );
    }

    // Fail fast before any mutation: the managed workspace root must be
    // usable, or no lease could ever materialize.
    if let Err(detail) = ensure_managed_root(&composition.db_path) {
        return (exec_error(BusinessErrorCode::Internal, detail), None);
    }

    let ids = OrchestrationIds {
        execution_id: execution_id.clone(),
        attempt_id: attempt_id.clone(),
    };
    let workspace_request = WorkspaceRequest {
        execution_id: execution_id.clone(),
        repository_ref,
        repository_path: req.repository_path.clone(),
        base_revision,
    };
    let mut orchestration =
        match OrchestrationService::open(&composition.db_path, app_services::SystemClock) {
            Ok(service) => service,
            Err(error) => {
                return (
                    exec_error(
                        BusinessErrorCode::Internal,
                        format!("execution entry unavailable: {error}"),
                    ),
                    None,
                );
            }
        };
    match orchestration.enter_execution(&task_id, ids) {
        Ok(ExecutionEntry::Runnable(work)) => {
            let cancel = provider_claude::CancelFlag::fresh();
            composition.cancels.register(&execution_id, cancel.clone());
            let response = ExecutionCoreResponse {
                payload: Some(execution_core_response::Payload::Run(
                    RunExecutionResponse {
                        task_id: task_id.to_string(),
                        execution_id: execution_id.to_string(),
                        attempt_id: attempt_id.to_string(),
                        task_state: TaskState::Running.as_str().to_owned(),
                    },
                )),
            };
            let job = BackgroundJob {
                composition: composition.clone(),
                work,
                workspace_request,
                cancel,
            };
            (response, Some(job))
        }
        Ok(ExecutionEntry::NotRunnable(outcome)) => (not_runnable_error(&task_id, outcome), None),
        Err(error) => {
            let (code, message) = crate::dispatch::map_error(&error);
            (exec_error(code, message), None)
        }
    }
}

/// Cheap provider precheck without spawning: the configured executable
/// must resolve. Version/auth surface later as typed attempt outcomes —
/// this gate only refuses the structurally hopeless case before entry.
fn provider_precheck(composition: &ExecutionComposition) -> Result<(), String> {
    if let Some(exe) = &composition.provider_executable {
        if Path::new(exe).exists() {
            return Ok(());
        }
        return Err(format!(
            "provider unavailable: configured executable {} does not exist",
            truncate_id(&exe.to_string_lossy())
        ));
    }
    match provider_claude::find_on_path(std::ffi::OsStr::new("claude")) {
        Some(_) => Ok(()),
        None => Err("provider unavailable: claude executable not found on PATH".to_owned()),
    }
}

/// Ensures the application-owned managed workspace root
/// (`<db-parent>/workspaces`) with the same fail-closed directory
/// discipline as the data directory. No I/O beyond directory creation.
fn ensure_managed_root(db_path: &Path) -> Result<ManagedWorkspaceRoot, String> {
    let parent = db_path.parent().ok_or_else(|| {
        format!(
            "execution workspace root unavailable: database path {} has no parent",
            db_path.display()
        )
    })?;
    let root = parent.join("workspaces");
    crate::data_dir::ensure_data_dir(&root)
        .map_err(|error| format!("execution workspace root unavailable: {error}"))?;
    ManagedWorkspaceRoot::new(&root)
        .map_err(|error| format!("execution workspace root unusable: {error}"))
}

fn not_runnable_error(task_id: &TaskId, outcome: OrchestrationOutcome) -> ExecutionCoreResponse {
    match outcome {
        OrchestrationOutcome::NotRunnable { state, .. } => exec_error(
            BusinessErrorCode::InvalidTransition,
            format!("task {task_id} is not runnable from state {state}"),
        ),
        OrchestrationOutcome::WaitingOnDependency { blocking, .. } => {
            let mut ids: Vec<String> = blocking
                .iter()
                .map(|id| truncate_id(&id.to_string()))
                .collect();
            ids.truncate(8);
            exec_error(
                BusinessErrorCode::Conflict,
                format!("task {task_id} waits on dependencies: {}", ids.join(", ")),
            )
        }
        // The entry half never produces these; unreachable in practice.
        // Map defensively rather than panicking on a trusted-enum shape.
        other => exec_error(
            BusinessErrorCode::Internal,
            format!(
                "unexpected entry outcome for task {task_id}: {}",
                outcome_tag(&other)
            ),
        ),
    }
}

fn mint_or_validate_execution(raw: &str) -> Result<ExecutionId, String> {
    if raw.trim().is_empty() {
        return ExecutionId::new(mint_id("E"))
            .map_err(|error| format!("minted execution id rejected: {error}"));
    }
    ExecutionId::new(raw)
        .map_err(|_| "execution id must be a non-blank bounded identifier".to_owned())
}

fn mint_or_validate_attempt(raw: &str) -> Result<agent_runtime::AttemptId, String> {
    if raw.trim().is_empty() {
        return agent_runtime::AttemptId::new(mint_id("A"))
            .map_err(|error| format!("minted attempt id rejected: {error}"));
    }
    agent_runtime::AttemptId::new(raw)
        .map_err(|_| "attempt id must be a non-blank bounded identifier".to_owned())
}

fn mint_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let counter = MINT_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}-{millis}-{counter}")
}

// -- get -----------------------------------------------------------------

/// Reads durable execution state. Read-only: opens the store, answers
/// from committed rows, emits no events, touches no provider.
fn handle_get(
    composition: &ExecutionComposition,
    req: GetExecutionRequest,
) -> ExecutionCoreResponse {
    let execution_id = match ExecutionId::new(req.execution_id) {
        Ok(id) => id,
        Err(_) => {
            return exec_error(
                BusinessErrorCode::InvalidArgument,
                "execution id must be a non-blank bounded identifier",
            );
        }
    };
    let store = match storage_sqlite::SqliteStore::open(&composition.db_path) {
        Ok(store) => store,
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    let execution = match store.get_execution(&execution_id) {
        Ok(Some(execution)) => execution,
        Ok(None) => {
            return exec_error(
                BusinessErrorCode::NotFound,
                format!(
                    "execution {} does not exist",
                    truncate_id(execution_id.as_str())
                ),
            );
        }
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    let task_state = match store.get_task(execution.task_id()) {
        Ok(Some(task)) => task.state().as_str().to_owned(),
        Ok(None) => "UNKNOWN".to_owned(),
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    let workspace_path = match store.get_workspace_for_execution(&execution_id) {
        Ok(Some(lease)) => lease.worktree_ref().to_owned(),
        Ok(None) => String::new(),
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    let attempts = execution
        .attempts()
        .iter()
        .map(|attempt| AttemptView {
            attempt_id: attempt.id().to_string(),
            outcome: attempt_outcome_name(attempt.outcome()),
            provider_ref: attempt.provider().as_str().to_owned(),
            detail: truncate_detail(attempt.detail().unwrap_or_default()),
        })
        .collect::<Vec<_>>();
    let provider_ref = attempts
        .first()
        .map(|attempt| attempt.provider_ref.clone())
        .unwrap_or_default();
    ExecutionCoreResponse {
        payload: Some(execution_core_response::Payload::Get(
            GetExecutionResponse {
                task_id: execution.task_id().to_string(),
                execution_id: execution_id.to_string(),
                task_state,
                attempts,
                provider_ref,
                workspace_path,
                execution_live: composition.cancels.is_live(&execution_id),
            },
        )),
    }
}

fn attempt_outcome_name(outcome: Option<AttemptOutcome>) -> String {
    match outcome {
        None => "OPEN".to_owned(),
        Some(AttemptOutcome::Succeeded) => "SUCCEEDED".to_owned(),
        Some(AttemptOutcome::Failed) => "FAILED".to_owned(),
        Some(AttemptOutcome::TimedOut) => "TIMED_OUT".to_owned(),
        Some(AttemptOutcome::Cancelled) => "CANCELLED".to_owned(),
    }
}

// -- cancel ---------------------------------------------------------------

/// Signals the live background run for one execution, or reports the
/// durable terminal state idempotently when nothing is live.
fn handle_cancel(
    composition: &ExecutionComposition,
    req: CancelExecutionRequest,
) -> ExecutionCoreResponse {
    let execution_id = match ExecutionId::new(req.execution_id) {
        Ok(id) => id,
        Err(_) => {
            return exec_error(
                BusinessErrorCode::InvalidArgument,
                "execution id must be a non-blank bounded identifier",
            );
        }
    };
    if composition.cancels.signal(&execution_id) {
        return ExecutionCoreResponse {
            payload: Some(execution_core_response::Payload::Cancel(
                CancelExecutionResponse {
                    execution_id: execution_id.to_string(),
                    cancel_requested: true,
                    run_state: "LIVE_SIGNALLED".to_owned(),
                    detail: format!(
                        "cancellation signalled for execution {}; the provider tree is being terminated and the terminal outcome will be durable",
                        truncate_id(execution_id.as_str())
                    ),
                },
            )),
        };
    }
    // No live run: answer from durable state (idempotent by construction).
    let store = match storage_sqlite::SqliteStore::open(&composition.db_path) {
        Ok(store) => store,
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    let execution = match store.get_execution(&execution_id) {
        Ok(Some(execution)) => execution,
        Ok(None) => {
            return exec_error(
                BusinessErrorCode::NotFound,
                format!(
                    "execution {} does not exist",
                    truncate_id(execution_id.as_str())
                ),
            );
        }
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    let open = execution
        .attempts()
        .iter()
        .find(|attempt| attempt.is_open());
    match open {
        Some(attempt) => ExecutionCoreResponse {
            payload: Some(execution_core_response::Payload::Cancel(
                CancelExecutionResponse {
                    execution_id: execution_id.to_string(),
                    cancel_requested: false,
                    run_state: "NO_LIVE_RUN".to_owned(),
                    detail: format!(
                        "no live provider run for execution {}; attempt {} remains open (stranded or concluding) — inspect status",
                        truncate_id(execution_id.as_str()),
                        attempt.id()
                    ),
                },
            )),
        },
        None => {
            let last = execution
                .attempts()
                .last()
                .map(|attempt| {
                    format!("attempt {} {}", attempt.id(), attempt_outcome_name(attempt.outcome()))
                })
                .unwrap_or_else(|| "no attempts recorded".to_owned());
            ExecutionCoreResponse {
                payload: Some(execution_core_response::Payload::Cancel(
                    CancelExecutionResponse {
                        execution_id: execution_id.to_string(),
                        cancel_requested: false,
                        run_state: "ALREADY_CONCLUDED".to_owned(),
                        detail: format!(
                            "execution {} already concluded: {last}",
                            truncate_id(execution_id.as_str())
                        ),
                    },
                )),
            }
        }
    }
}

// -- wire helpers ----------------------------------------------------------

fn exec_error(code: BusinessErrorCode, message: impl Into<String>) -> ExecutionCoreResponse {
    ExecutionCoreResponse {
        payload: Some(execution_core_response::Payload::Error(BusinessError {
            code: code as i32,
            message: truncate_detail(&message.into()),
        })),
    }
}

/// Defensive bound on status/error detail: validated identifiers only,
/// never raw internals, never provider output beyond the attempt detail
/// the domain already bounds.
fn truncate_detail(detail: &str) -> String {
    const MAX: usize = MAX_STATUS_DETAIL_CHARS;
    if detail.len() <= MAX {
        return detail.to_owned();
    }
    // Truncate on a char boundary; detail is operator-facing text.
    let mut end = MAX;
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &detail[..end])
}

/// Tighter bound for identifiers embedded in messages.
fn truncate_id(value: &str) -> String {
    const MAX_ID_MESSAGE_LEN: usize = 64;
    if value.len() > MAX_ID_MESSAGE_LEN {
        format!("{}…", &value[..MAX_ID_MESSAGE_LEN])
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct Sandbox {
        dir: PathBuf,
        db: PathBuf,
        repo: PathBuf,
    }

    impl Sandbox {
        fn named(suite: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "runlith-agentd-exec-{suite}-{id}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(dir.join("db")).expect("sandbox db dir");
            let db = dir.join("db").join("state.sqlite3");
            let repo = dir.join("repo");
            std::fs::create_dir_all(&repo).expect("sandbox repo dir");
            git(&repo, &["init", "-q"]);
            git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
            Self { dir, db, repo }
        }

        fn composition(&self, fixture: Option<PathBuf>) -> ExecutionComposition {
            ExecutionComposition::new(self.db.clone(), fixture)
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn git(repo: &Path, args: &[&str]) {
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
            .current_dir(repo)
            .stdin(std::process::Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("fixture git");
        assert!(output.status.success(), "git {args:?} failed");
    }

    fn fixture_script(dir: &Path, name: &str, body: &str) -> PathBuf {
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

    fn seed_ready(db: &Path, project: &str, task: &str) {
        let mut tasks = app_services::TaskService::open(db, app_services::SystemClock)
            .expect("open task service");
        let project_id = task_domain::ProjectId::new(project).unwrap();
        tasks
            .create_project(
                &task_domain::Project::new(project_id.clone(), format!("project {project}"))
                    .unwrap(),
            )
            .unwrap();
        let task_id = task_domain::TaskId::new(task).unwrap();
        tasks
            .create_task(
                &task_domain::Task::draft(task_id.clone(), project_id, format!("objective {task}"))
                    .unwrap(),
            )
            .unwrap();
        tasks.mark_ready(&task_id).unwrap();
    }

    fn run_request(task: &str, repo: &Path) -> RunExecutionRequest {
        RunExecutionRequest {
            task_id: task.to_owned(),
            execution_id: format!("E-{task}"),
            attempt_id: format!("A-{task}"),
            repository_ref: "repo-local:composition".to_owned(),
            repository_path: repo.to_string_lossy().into_owned(),
            base_revision: "HEAD".to_owned(),
        }
    }

    fn run_payload(response: &ExecutionCoreResponse) -> RunExecutionResponse {
        match &response.payload {
            Some(execution_core_response::Payload::Run(run)) => run.clone(),
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn cancel_registry_is_per_execution_and_idempotent() {
        let registry = CancelRegistry::fresh();
        let live = ExecutionId::new("E-live").unwrap();
        let other = ExecutionId::new("E-other").unwrap();
        assert!(!registry.is_live(&live));
        assert!(!registry.signal(&live));
        let flag = provider_claude::CancelFlag::fresh();
        registry.register(&live, flag);
        assert!(registry.is_live(&live));
        assert!(!registry.is_live(&other));
        assert!(registry.signal(&live));
        assert!(
            registry.signal(&live),
            "repeat signal stays true while live"
        );
        registry.unregister(&live);
        assert!(!registry.is_live(&live));
        assert!(!registry.signal(&live));
        registry.unregister(&live);
    }

    #[test]
    fn pending_evaluator_reaches_no_verdict() {
        let evaluator = PendingEvaluator;
        let request = EvaluationRequest {
            task_id: TaskId::new("T-1").unwrap(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: agent_runtime::AttemptId::new("A-1").unwrap(),
            summary: "evidence".to_owned(),
            evidence_ref: "ref".to_owned(),
        };
        let error = evaluator.evaluate(&request).expect_err("never verifies");
        assert!(
            error.detail().contains("no automated verifier"),
            "got {:?}",
            error.detail()
        );
    }

    #[test]
    fn run_rejects_unknown_task_without_mutation() {
        let sandbox = Sandbox::named("unknown-task");
        let fixture = fixture_script(&sandbox.dir, "unused.sh", "exit 0\n");
        let composition = sandbox.composition(Some(fixture));
        let (response, job) = start_run(
            &composition,
            RunExecutionRequest {
                task_id: "T-ghost".to_owned(),
                ..run_request("T-ghost", &sandbox.repo)
            },
        );
        assert!(job.is_none());
        match &response.payload {
            Some(execution_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::NotFound as i32);
            }
            other => panic!("expected NOT_FOUND, got {other:?}"),
        }
        // No execution row was committed for the ghost task.
        let get = handle_get(
            &composition,
            GetExecutionRequest {
                execution_id: "E-T-ghost".to_owned(),
            },
        );
        assert!(
            matches!(
                get.payload,
                Some(execution_core_response::Payload::Error(_))
            ),
            "ghost execution must stay absent, got {get:?}"
        );
    }

    #[test]
    fn run_refuses_missing_provider_before_mutation() {
        let sandbox = Sandbox::named("no-provider");
        seed_ready(&sandbox.db, "P-1", "T-1");
        // Override points at nothing: fail fast, commit nothing.
        let composition = sandbox.composition(Some(sandbox.dir.join("no-such-binary")));
        let (response, job) = start_run(&composition, run_request("T-1", &sandbox.repo));
        assert!(job.is_none());
        match &response.payload {
            Some(execution_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::ProviderUnavailable as i32);
            }
            other => panic!("expected PROVIDER_UNAVAILABLE, got {other:?}"),
        }
        let get = handle_get(
            &composition,
            GetExecutionRequest {
                execution_id: "E-T-1".to_owned(),
            },
        );
        assert!(
            matches!(
                get.payload,
                Some(execution_core_response::Payload::Error(_))
            ),
            "refused run must commit no execution, got {get:?}"
        );
    }

    #[test]
    fn live_provider_run_protects_its_workspace_then_releases() {
        let sandbox = Sandbox::named("liveness");
        seed_ready(&sandbox.db, "P-1", "T-1");
        let fixture = fixture_script(&sandbox.dir, "slow.sh", "sleep 30\n");
        let composition = sandbox.composition(Some(fixture));

        let (response, job) = dispatch_execution(
            &composition,
            ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Run(run_request(
                    "T-1",
                    &sandbox.repo,
                ))),
            },
        );
        let run = run_payload(&response);
        assert_eq!(run.task_state, "RUNNING");
        let job = job.expect("slow fixture must start");
        assert!(composition
            .cancels
            .is_live(&ExecutionId::new(&run.execution_id).unwrap()));

        let handle = std::thread::spawn(move || run_background(job));
        // Wait until the attempt exists and the provider child is live:
        // the lease worktree must exist by then.
        let execution_id = ExecutionId::new(&run.execution_id).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let lease = loop {
            let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
            if let Ok(Some(lease)) = store.get_workspace_for_execution(&execution_id) {
                if composition.tracker.is_live_execution(&execution_id) {
                    break lease;
                }
            }
            if std::time::Instant::now() >= deadline {
                panic!("provider child never went live");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert!(
            Path::new(lease.worktree_ref()).is_dir(),
            "lease worktree must exist during the live run"
        );

        // Cleanup while the owner is live is refused — and destroys
        // nothing. This wires the composition-root tracker through
        // GitCli::with_liveness, the same seam production uses.
        let root = ManagedWorkspaceRoot::new(sandbox.db.parent().unwrap().join("workspaces"))
            .expect("managed root");
        let mut workspace = WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new().with_liveness(std::sync::Arc::new(composition.tracker.clone())
                as std::sync::Arc<dyn workspace::OwnerLiveness>),
            root,
            app_services::SystemClock,
        )
        .expect("open workspace service");
        match workspace.cleanup_workspace(lease.id(), &sandbox.repo.to_string_lossy()) {
            Err(workspace::WorkspaceFailure::ActiveOwnerRejectsCleanup { .. }) => {}
            other => panic!("live owner must refuse cleanup, got {other:?}"),
        }
        assert!(
            Path::new(lease.worktree_ref()).is_dir(),
            "refused cleanup must destroy nothing"
        );

        // Cancel through the composition registry: only this run's tree.
        assert!(composition.cancels.signal(&execution_id));
        handle.join().expect("background joins");
        assert!(
            !composition.tracker.is_live_execution(&execution_id),
            "terminal outcome must unregister the owner"
        );

        // Post-terminal status is durable and observable.
        let status = handle_get(
            &composition,
            GetExecutionRequest {
                execution_id: run.execution_id.clone(),
            },
        );
        match &status.payload {
            Some(execution_core_response::Payload::Get(get)) => {
                assert_eq!(get.attempts.len(), 1);
                assert_eq!(get.attempts[0].outcome, "CANCELLED");
                assert!(!get.execution_live);
                assert!(!get.workspace_path.is_empty());
            }
            other => panic!("expected terminal status, got {other:?}"),
        }
        // Repeat cancel is idempotent: already concluded, not an error.
        let again = handle_cancel(
            &composition,
            CancelExecutionRequest {
                execution_id: run.execution_id.clone(),
            },
        );
        match &again.payload {
            Some(execution_core_response::Payload::Cancel(cancel)) => {
                assert!(!cancel.cancel_requested);
                assert_eq!(cancel.run_state, "ALREADY_CONCLUDED");
            }
            other => panic!("expected idempotent cancel, got {other:?}"),
        }

        // After the terminal outcome the normal cleanup rules proceed.
        let mut workspace = WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            ManagedWorkspaceRoot::new(sandbox.db.parent().unwrap().join("workspaces"))
                .expect("managed root"),
            app_services::SystemClock,
        )
        .expect("open workspace service");
        workspace
            .cleanup_workspace(lease.id(), &sandbox.repo.to_string_lossy())
            .expect("post-terminal cleanup proceeds");
    }
}
