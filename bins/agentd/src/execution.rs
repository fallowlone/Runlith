//! G5 execution composition root (TASK-0047): the production wiring from
//! versioned IPC to the real provider through the trusted application
//! services.
//!
//! ```text
//! agentctl
//!  → versioned local IPC (`execution_core`, protocol v2 only)
//!  → agentd `serve_connection` (this composition root)
//!  → PlanningService current accepted Plan resolution (no planner call)
//!  → OrchestrationService::enter_from_current_plan (durable entry, synchronous)
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
//! reconcilable open state — never as silent success. Pre-attempt strands
//! (entry committed, no attempt yet) recover through the explicit `resume`
//! operation (TASK-0054); post-attempt orphan recovery stays G11 work,
//! explicitly deferred.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_runtime::{AttemptOutcome, ExecutionId};
use app_services::{
    ExecutionEntry, G6Authorization, G6SecurityService, ManagedWorkspaceRoot, OrchestrationIds,
    OrchestrationService, PlannedExecutionEntryOutcome, PlannedOrchestrationError, PlanningService,
    PlanningServiceError, PrivilegedOperation, ProcessSecuritySpec, ServiceError, WorkspaceRequest,
    WorkspaceService,
};
use evaluation::{EvaluationPort, EvaluationRequest, EvaluationResult, EvaluationVerdict};
use ipc_contracts::proto::{
    execution_core_request, execution_core_response, AttemptView, BusinessError, BusinessErrorCode,
    CancelExecutionRequest, CancelExecutionResponse, ExecutionCoreRequest, ExecutionCoreResponse,
    GetExecutionRequest, GetExecutionResponse, ResumeExecutionRequest, ResumeExecutionResponse,
    RunExecutionRequest, RunExecutionResponse,
};
use scheduler::OrchestrationOutcome;
use task_domain::{TaskId, TaskState};
use workspace::{GitPort, RepositoryRef, Revision, WorkspaceFailure, WorkspaceStore};

use policy::{
    Capability, CompiledPolicy, EffectiveCapabilities, FileScope, PolicyLayer, PolicyOutcome,
    PolicyRequest, PolicyRule, RequestOrigin, RequestedCapabilities, ScopeId,
};
use runner::{ChildEnvironment, IsolationBackend, IsolationRequirements};

/// Wall-clock bound for one production provider execution. Matches the
/// adapter default: the daemon does not invent its own timeout policy.
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(300);

/// Capture-finalization grace after natural provider exit. Matches the
/// adapter default; see `provider-claude` process supervision.
pub const CAPTURE_GRACE: Duration = Duration::from_secs(5);

const PROVIDER_LAUNCH_REQUESTER: &str = "agent-runtime";

pub(crate) fn production_runner_backend(
    support_read: Vec<PathBuf>,
    support_write: Vec<PathBuf>,
) -> Result<Arc<dyn IsolationBackend>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(Arc::new(
            runner_macos_seatbelt::MacOsSeatbeltRunner::with_support_roots(
                support_read,
                support_write,
            ),
        ))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (support_read, support_write);
        Ok(Arc::new(runner_native::NativeRunner))
    }
}

/// Bound for server-local path inputs on the wire (repository paths).
const MAX_PATH_LEN: usize = 4096;

/// Bound for revision-request inputs on the wire.
const MAX_REVISION_LEN: usize = 1024;

/// Bound for attempt detail echoed in status responses.
const MAX_STATUS_DETAIL_CHARS: usize = 1024;

/// Monotonic mint for server-generated execution/attempt identities.
static MINT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Monotonic mint for process-local live-run owner tokens. Starts at 1 so
/// the zero value never names an owner; tokens are opaque runtime identity
/// only (never persisted, never wall-clock derived) and distinguish
/// successive owners of one execution for ABA-safe release.
static OWNER_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Opaque process-local identity of one live-run owner (TASK-0056, G5
/// B-CR). At most one owner exists per execution per daemon: the registry
/// hands a fresh token to the single claim winner, and only that token can
/// retire the entry. Tokens die with the daemon process — durable
/// restart/recovery stays governed by TASK-0054 semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LiveOwnerToken(u64);

/// One live owner's registry entry: the cancellation handle that belongs
/// to the owner identity, so `cancel` always signals the current owner's
/// provider path and a rejected loser can neither replace nor clear it.
#[derive(Debug, Clone)]
struct LiveOwner {
    token: LiveOwnerToken,
    flag: provider_claude::CancelFlag,
}

/// Per-execution live-run ownership owned by the composition root
/// (TASK-0056, G5 B-CR). One entry per live background run, from the
/// atomic claim until the owning run's terminal outcome is committed.
/// Signalling kills only the registered owner's child tree (via the
/// executor's shared flag) — never by process name.
///
/// Claim is insert-if-absent and release is owner-scoped: a second
/// run/resume for a live execution is refused without touching the entry,
/// and a stale/losing path can never unregister the current owner.
#[derive(Debug, Clone, Default)]
pub struct CancelRegistry {
    inner: Arc<Mutex<HashMap<String, LiveOwner>>>,
}

impl CancelRegistry {
    /// Empty registry. A fresh daemon always starts empty: process-local
    /// ownership never survives a restart by construction.
    #[must_use]
    pub fn fresh() -> Self {
        Self::default()
    }

    /// Atomically claims exclusive live ownership of `execution` for one
    /// run/resume path. Returns the owner token on success; returns `None`
    /// — leaving any current owner untouched — when another live owner
    /// already exists. Never overwrites. The critical section is one
    /// `HashMap` lookup/insert only: no provider, Git, or database work
    /// under the lock.
    pub fn try_claim(
        &self,
        execution: &ExecutionId,
        flag: provider_claude::CancelFlag,
    ) -> Option<LiveOwnerToken> {
        let token = LiveOwnerToken(OWNER_COUNTER.fetch_add(1, Ordering::SeqCst));
        if let Ok(mut inner) = self.inner.lock() {
            use std::collections::hash_map::Entry;
            match inner.entry(execution.as_str().to_owned()) {
                Entry::Vacant(vacant) => {
                    vacant.insert(LiveOwner { token, flag });
                    Some(token)
                }
                Entry::Occupied(_) => None,
            }
        } else {
            None
        }
    }

    /// Retires the entry for `execution` only when `token` is still the
    /// current owner. Returns true when the entry was retired. A stale
    /// token (loser cleanup, delayed exit after a newer owner claimed)
    /// returns false and removes nothing.
    pub fn release(&self, execution: &ExecutionId, token: LiveOwnerToken) -> bool {
        self.release_id(execution.as_str(), token)
    }

    /// Token-scoped retire for one raw execution identity. See [`Self::release`].
    pub fn release_id(&self, execution_id: &str, token: LiveOwnerToken) -> bool {
        if let Ok(mut inner) = self.inner.lock() {
            let owned = inner
                .get(execution_id)
                .is_some_and(|owner| owner.token == token);
            if owned {
                inner.remove(execution_id);
                return true;
            }
        }
        false
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
    /// Always reaches the CURRENT owner's flag: rejected losers never
    /// replace the entry, so a `LIVE_SIGNALLED` report always names the
    /// provider path that actually observes the signal.
    pub fn signal(&self, execution: &ExecutionId) -> bool {
        let flag = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.get(execution.as_str()).cloned());
        match flag {
            Some(owner) => {
                owner.flag.cancel();
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
    /// Canonical Git executable resolved once when the daemon composition is
    /// built. A poisoned later PATH cannot redirect privileged Git work.
    git_executable: Result<PathBuf, String>,
    /// Composition-owned provider executable override. `None` resolves
    /// `claude` via `PATH`; `Some` points at an explicit binary (tests
    /// inject fixture doubles). Never client-supplied — no IPC field
    /// maps here, so a client cannot turn the daemon into an arbitrary
    /// process launcher.
    provider_executable: Option<PathBuf>,
}

impl ExecutionComposition {
    /// Builds the composition for one database and resolves the trusted Git
    /// executable once. The managed workspace root is still ensured per
    /// `run` (fail fast, before mutation).
    pub fn new(db_path: PathBuf, provider_executable: Option<PathBuf>) -> Self {
        Self {
            db_path,
            cancels: CancelRegistry::fresh(),
            tracker: provider_claude::LiveProcessTracker::fresh(),
            git_executable: resolve_git_executable(),
            provider_executable,
        }
    }

    /// The durable database this composition works against.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn git_cli(&self) -> Result<git_cli::GitCli, String> {
        self.git_executable
            .as_ref()
            .map(|path| git_cli::GitCli::with_executable(path.clone()))
            .map_err(Clone::clone)
    }

    pub(crate) fn resolved_provider_executable(&self) -> Result<PathBuf, String> {
        let executable = self
            .provider_executable
            .clone()
            .or_else(|| provider_claude::find_on_path(std::ffi::OsStr::new("claude")))
            .ok_or_else(|| {
                "provider unavailable: claude executable not found on PATH".to_owned()
            })?;
        let executable = std::fs::canonicalize(&executable).map_err(|error| {
            format!(
                "provider executable cannot be canonicalized ({}): {error}",
                executable.display()
            )
        })?;
        if !executable.is_file() {
            return Err("provider executable is not a regular file".to_owned());
        }
        Ok(executable)
    }

    pub(crate) fn provider_environment_base(&self) -> Result<PathBuf, String> {
        provider_environment_base(&self.db_path)
    }
}

fn resolve_git_executable() -> Result<PathBuf, String> {
    let executable = provider_claude::find_on_path(OsStr::new("git"))
        .ok_or_else(|| "git executable not found on PATH".to_owned())?;
    let executable = std::fs::canonicalize(executable)
        .map_err(|_| "git executable cannot be canonicalized".to_owned())?;
    if !executable.is_file() {
        return Err("git executable is not a regular file".to_owned());
    }
    Ok(executable)
}

/// Shared handle type for the serve loop.
pub type ExecutionHandle = Arc<ExecutionComposition>;

/// Production G6 wrapper around the replaceable provider executor. It owns no
/// provider behavior: its only job is to re-present trusted product state to
/// Policy, enforce exact launch authority through Runner immediately around
/// the spawn boundary, and persist product-authored Security Audit evidence.
struct SecuredClaudeExecutor {
    inner: provider_claude::ClaudeExecutor,
    db_path: PathBuf,
    launch: Result<ProviderLaunchSecurity, String>,
}

pub(crate) struct ProviderLaunchSecurity {
    request: PolicyRequest,
    parent: EffectiveCapabilities,
    policy: CompiledPolicy,
    workspace: String,
    executable: ScopeId,
}

impl ProviderLaunchSecurity {
    pub(crate) fn policy_fingerprint(&self) -> &str {
        self.policy.fingerprint()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ProviderWorkspaceAccess {
    ReadOnly,
    ReadWrite,
}

pub(crate) struct ProviderLaunchContext<'a> {
    pub(crate) task_id: &'a str,
    pub(crate) execution_id: Option<String>,
    pub(crate) attempt_id: Option<String>,
    pub(crate) operation_id: String,
    pub(crate) requester: &'a str,
    pub(crate) workspace: &'a Path,
    pub(crate) executable: &'a std::ffi::OsStr,
    pub(crate) workspace_access: ProviderWorkspaceAccess,
}

pub(crate) struct PreparedProviderLaunch {
    security: G6SecurityService<app_services::SystemClock>,
    operation: PrivilegedOperation,
    backend: Arc<dyn IsolationBackend>,
    effective: EffectiveCapabilities,
    spec: ProcessSecuritySpec,
    launch: Arc<runner::SandboxedLaunch>,
}

impl PreparedProviderLaunch {
    pub(crate) fn invoke<T, E>(
        mut self,
        invoke: impl FnOnce(Arc<runner::SandboxedLaunch>) -> Result<T, E>,
    ) -> Result<Result<T, E>, String> {
        let launch = Arc::clone(&self.launch);
        self.security
            .enforce_process_operation(
                &self.operation,
                self.backend.as_ref(),
                &self.effective,
                &self.spec,
                || invoke(launch),
            )
            .map_err(|error| format!("G6 security boundary refused provider launch: {error}"))
    }
}

pub(crate) fn prepare_provider_launch(
    db_path: &Path,
    launch: &ProviderLaunchSecurity,
    support_read: Vec<PathBuf>,
    support_write: Vec<PathBuf>,
    environment_pairs: Vec<(std::ffi::OsString, std::ffi::OsString)>,
) -> Result<PreparedProviderLaunch, String> {
    let mut security = G6SecurityService::open(db_path, app_services::SystemClock)
        .map_err(|error| format!("G6 security boundary refused provider launch: {error}"))?;
    let authorization = security
        .authorize(&launch.request, &launch.parent, &launch.policy, u64::MAX)
        .map_err(|error| format!("G6 security boundary refused provider launch: {error}"))?;
    let effective = match authorization {
        G6Authorization::Authorized { effective, .. } => effective,
        G6Authorization::ApprovalRequired { .. } => {
            return Err("G6 approval required before provider launch".to_owned());
        }
        G6Authorization::Denied { .. } => {
            return Err("G6 policy denied provider launch".to_owned());
        }
    };
    let operation = PrivilegedOperation::from_policy_request(&launch.request);
    let backend = production_runner_backend(support_read, support_write)?;
    let environment = ChildEnvironment::from_explicit(environment_pairs)
        .map_err(|error| format!("G6 child environment refused: {error}"))?;
    let spec = ProcessSecuritySpec::new(
        launch.workspace.clone(),
        launch.executable.clone(),
        IsolationRequirements::new(true, true, true),
        environment.clone(),
        provider_claude::CLAUDE_PROVIDER_REF,
    );
    let prepared_launch = runner::prepare_process_launch(
        Arc::clone(&backend),
        &effective,
        launch.workspace.clone(),
        launch.executable.clone(),
        IsolationRequirements::new(true, true, true),
        environment,
    )
    .map_err(|error| format!("G6 runtime launch preparation refused: {error}"))?;

    Ok(PreparedProviderLaunch {
        security,
        operation,
        backend,
        effective,
        spec,
        launch: Arc::new(prepared_launch),
    })
}

impl scheduler::ExecutionPort for SecuredClaudeExecutor {
    fn provider_ref(&self) -> agent_runtime::ProviderRef {
        self.inner.provider_ref()
    }

    fn execute(
        &self,
        work: &scheduler::WorkItem,
    ) -> Result<scheduler::WorkEvidence, scheduler::ExecutionProblem> {
        let launch =
            self.launch
                .as_ref()
                .map_err(|detail| scheduler::ExecutionProblem::Failed {
                    detail: format!("G6 provider launch setup refused: {detail}"),
                })?;
        let support_read = self
            .inner
            .isolation_support_read_roots()
            .map_err(|detail| scheduler::ExecutionProblem::Failed {
                detail: format!("G6 runtime backend setup refused: {detail}"),
            })?;
        let support_write = self
            .inner
            .isolation_support_write_roots()
            .map_err(|detail| scheduler::ExecutionProblem::Failed {
                detail: format!("G6 runtime backend setup refused: {detail}"),
            })?;
        let environment = self.inner.child_environment().map_err(|detail| {
            scheduler::ExecutionProblem::Failed {
                detail: format!("G6 provider environment refused: {detail}"),
            }
        })?;
        let prepared = prepare_provider_launch(
            &self.db_path,
            launch,
            support_read,
            support_write,
            environment,
        )
        .map_err(|detail| scheduler::ExecutionProblem::Failed { detail })?;
        prepared
            .invoke(|prepared_launch| {
                self.inner
                    .clone()
                    .with_sandboxed_launch(prepared_launch)
                    .execute(work)
            })
            .map_err(|detail| scheduler::ExecutionProblem::Failed { detail })?
    }
}

fn secure_provider_executor(
    db_path: &Path,
    task: &task_domain::Task,
    lease: &workspace::WorkspaceLease,
    work: &scheduler::WorkItem,
    inner: provider_claude::ClaudeExecutor,
    executable: &std::ffi::OsStr,
) -> SecuredClaudeExecutor {
    let launch = build_provider_launch_security(task, lease, work, executable);
    SecuredClaudeExecutor {
        inner,
        db_path: db_path.to_path_buf(),
        launch,
    }
}

fn build_provider_launch_security(
    task: &task_domain::Task,
    lease: &workspace::WorkspaceLease,
    work: &scheduler::WorkItem,
    executable: &std::ffi::OsStr,
) -> Result<ProviderLaunchSecurity, String> {
    build_provider_launch_security_for_context(ProviderLaunchContext {
        task_id: task.id().as_str(),
        execution_id: Some(work.execution_id.to_string()),
        attempt_id: Some(work.attempt_id.to_string()),
        operation_id: format!(
            "provider.launch:{}:{}",
            work.execution_id.as_str(),
            work.attempt_id.as_str()
        ),
        requester: PROVIDER_LAUNCH_REQUESTER,
        workspace: Path::new(lease.worktree_ref()),
        executable,
        workspace_access: ProviderWorkspaceAccess::ReadWrite,
    })
}

pub(crate) fn build_provider_launch_security_for_context(
    context: ProviderLaunchContext<'_>,
) -> Result<ProviderLaunchSecurity, String> {
    let workspace = context
        .workspace
        .canonicalize()
        .map_err(|_| "provider launch workspace cannot be canonicalized".to_owned())?;
    if !workspace.is_dir() {
        return Err("provider launch workspace is not a directory".to_owned());
    }
    let workspace = workspace
        .to_str()
        .ok_or_else(|| "provider launch workspace is not valid UTF-8".to_owned())?
        .to_owned();
    let executable = context
        .executable
        .to_str()
        .ok_or_else(|| "provider launch executable is not valid UTF-8".to_owned())?;
    let executable = ScopeId::new(executable.to_owned())
        .map_err(|error| format!("provider launch executable scope: {error}"))?;
    let mut capabilities = vec![Capability::FilesystemRead(
        FileScope::new(&workspace, ".")
            .map_err(|error| format!("provider launch read scope: {error}"))?,
    )];
    if matches!(context.workspace_access, ProviderWorkspaceAccess::ReadWrite) {
        capabilities.push(Capability::FilesystemWrite(
            FileScope::new(&workspace, ".")
                .map_err(|error| format!("provider launch write scope: {error}"))?,
        ));
    }
    capabilities.extend([
        Capability::ProcessExecute(executable.clone()),
        Capability::NetworkAccess(
            ScopeId::new(runner::NETWORK_SCOPE_OUTBOUND_TCP_443)
                .map_err(|error| format!("provider launch network scope: {error}"))?,
        ),
    ]);
    let requested = RequestedCapabilities::new(capabilities.clone());
    let request = PolicyRequest::new(
        context.operation_id,
        context.requester,
        RequestOrigin::new(context.task_id, context.execution_id, context.attempt_id)
            .map_err(|error| format!("provider launch origin: {error}"))?,
        requested,
    )
    .map_err(|error| format!("provider launch request: {error}"))?;
    let rules = capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| {
            PolicyRule::new(
                format!("builtin.provider-launch.{index}"),
                "trusted_local_provider_launch",
                PolicyLayer::BuiltInHard,
                PolicyOutcome::Allow,
                capability.clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("provider launch policy: {error}"))?;
    let policy = CompiledPolicy::compile(rules)
        .map_err(|error| format!("provider launch policy: {error}"))?;

    Ok(ProviderLaunchSecurity {
        request,
        parent: EffectiveCapabilities::new(capabilities),
        policy,
        workspace,
        executable,
    })
}

/// A committed background run: the daemon entry is durable, the provider
/// half still has to execute. Spawned detached by the serve loop. Carries
/// the owner token won at claim time so the terminal path retires exactly
/// the entry it owns — never a newer owner's.
pub struct BackgroundJob {
    composition: ExecutionComposition,
    work: scheduler::WorkItem,
    workspace_request: WorkspaceRequest,
    cancel: provider_claude::CancelFlag,
    owner: LiveOwnerToken,
}

impl BackgroundJob {
    /// Durable execution identity this job works for (thread naming,
    /// logging). The composition already committed it.
    #[must_use]
    pub fn work_execution_id(&self) -> String {
        self.work.execution_id.to_string()
    }

    /// The owner token this job won at claim time. The serve loop passes
    /// it to [`abort_start`] when the background thread cannot spawn; the
    /// background half passes it to the token-scoped release on exit.
    #[must_use]
    pub fn owner_token(&self) -> LiveOwnerToken {
        self.owner
    }
}

/// Builds the error response when the serve loop cannot spawn the
/// background thread (resource exhaustion): the durable entry is already
/// committed, so the response names the execution for recovery via
/// `status` instead of pretending nothing happened. Also retires the
/// owner's registry entry — there is no live run to signal — scoped to
/// the claiming token so a newer owner is never disturbed.
pub fn abort_start(
    composition: &ExecutionComposition,
    execution_id: &str,
    owner: LiveOwnerToken,
) -> ExecutionCoreResponse {
    composition.cancels.release_id(execution_id, owner);
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
/// job for the caller to spawn detached; `resume` re-drives a stranded
/// pre-attempt entry the same way; `get`/`cancel` are fully synchronous
/// reads/signals.
pub fn dispatch_execution(
    composition: &ExecutionComposition,
    request: ExecutionCoreRequest,
) -> (ExecutionCoreResponse, Option<BackgroundJob>) {
    match request.payload {
        Some(execution_core_request::Payload::Run(req)) => start_run(composition, req),
        Some(execution_core_request::Payload::Resume(req)) => resume_run(composition, req),
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
/// Retires the owner's registry entry on every path (success, failure,
/// lease refusal, bind refusal) so `status`/`cancel` observe honest state.
/// Release is scoped to the job's owner token: a stale exit path can never
/// retire a newer owner that claimed the execution afterwards.
pub fn run_background(job: BackgroundJob) {
    let BackgroundJob {
        composition,
        work,
        workspace_request,
        cancel,
        owner,
    } = job;
    let execution_id = workspace_request.execution_id.clone();
    let attempt_id = work.attempt_id.clone();
    match run_background_inner(&composition, work, &workspace_request, &cancel) {
        Ok(outcome) => {
            composition.cancels.release(&execution_id, owner);
            eprintln!(
                "agentd: execution {execution_id} concluded: {}",
                outcome_tag(&outcome)
            );
        }
        Err(detail) => {
            composition.cancels.release(&execution_id, owner);
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
    let git = composition
        .git_cli()?
        .with_liveness(Arc::new(composition.tracker.clone()) as Arc<dyn workspace::OwnerLiveness>);
    let mut workspace =
        WorkspaceService::open(&composition.db_path, git, root, app_services::SystemClock)
            .map_err(|error| format!("open workspace service: {error}"))?;
    let tracker = composition.tracker.clone();
    let executable = composition.resolved_provider_executable()?;
    let db_path = composition.db_path.clone();
    let security_work = work.clone();
    let cancel = cancel.clone();
    let environment_base = composition.provider_environment_base()?;
    orchestration
        .continue_workspace_cycle(
            &mut workspace,
            work,
            workspace_request.clone(),
            |task, lease| {
                let prompt =
                    provider_claude::build_task_prompt(task.objective(), task.acceptance());
                let config = provider_claude::ClaudeConfig::default()
                    .with_executable(executable.clone())
                    .expect("canonical provider executable is non-blank")
                    .with_timeout(PROVIDER_TIMEOUT)
                    .expect("constant provider timeout is non-zero")
                    .with_capture_grace(CAPTURE_GRACE)
                    .expect("constant capture grace is non-zero");
                let launch_executable = config.executable().to_owned();
                let executor = provider_claude::ClaudeExecutor::bind(
                    config,
                    prompt,
                    lease.worktree_ref().to_owned(),
                    environment_base,
                )
                .expect("lease-bound prompt and workdir are non-blank")
                .with_tracker(tracker)
                .with_cancel_flag(cancel);
                secure_provider_executor(
                    &db_path,
                    task,
                    lease,
                    &security_work,
                    executor,
                    &launch_executable,
                )
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

/// Read-only Git preflight for execution-entry workspace inputs
/// (TASK-0054, G5 B-RS boundary B′).
///
/// A repository path that is not a working repository, or a base revision
/// that does not resolve to a commit, is synchronously detectable with two
/// read-only Git observations — no mutation, no lease, no worktree. Such
/// deterministic failures refuse here with typed `INVALID_ARGUMENT`
/// before admission, instead of wedging a Task `RUNNING` in the
/// background where no production path could recover it.
///
/// Preflight is a diagnostic fast path, not a concurrency or security
/// boundary: Git state can still change afterwards, and that stays a
/// recoverable-saga concern. Client-facing messages are static text —
/// never the supplied path, which is untrusted operational input.
/// Small refusal for the entry preflight: a wire code plus static text.
/// Kept small (instead of returning the full response) so the helper
/// stays `Result`-shaped without tripping the large-error lint; callers
/// render it with [`exec_error`].
struct EntryPreflightFailure {
    code: BusinessErrorCode,
    message: &'static str,
}

fn resolve_entry_revision(
    git: &git_cli::GitCli,
    repository_path: &str,
    base_revision: &str,
) -> Result<Revision, EntryPreflightFailure> {
    // Admission preflight (trusted-deposit before provider execution):
    // establish the anchor first via the sole anchor-free path, then bind
    // subsequent observations to it.
    let anchor = match git.establish_trusted_anchor(repository_path) {
        Ok(a) => a,
        Err(WorkspaceFailure::NotRepository { .. })
        | Err(WorkspaceFailure::BareRepositoryUnsupported { .. }) => {
            return Err(EntryPreflightFailure {
                code: BusinessErrorCode::InvalidArgument,
                message: "repository path is not a usable git repository",
            });
        }
        Err(_) => {
            return Err(EntryPreflightFailure {
                code: BusinessErrorCode::Internal,
                message: "execution repository precheck unavailable",
            });
        }
    };
    match git.is_repository(repository_path, &anchor) {
        Ok(true) => {}
        Ok(false)
        | Err(WorkspaceFailure::NotRepository { .. })
        | Err(WorkspaceFailure::BareRepositoryUnsupported { .. }) => {
            return Err(EntryPreflightFailure {
                code: BusinessErrorCode::InvalidArgument,
                message: "repository path is not a usable git repository",
            });
        }
        Err(_) => {
            return Err(EntryPreflightFailure {
                code: BusinessErrorCode::Internal,
                message: "execution repository precheck unavailable",
            });
        }
    }
    match git.resolve_revision(repository_path, base_revision, &anchor) {
        Ok(revision) => Ok(revision),
        Err(WorkspaceFailure::InvalidRevision { .. } | WorkspaceFailure::NotRepository { .. }) => {
            Err(EntryPreflightFailure {
                code: BusinessErrorCode::InvalidArgument,
                message: "base revision is not resolvable to a commit in the repository",
            })
        }
        Err(_) => Err(EntryPreflightFailure {
            code: BusinessErrorCode::Internal,
            message: "execution repository precheck unavailable",
        }),
    }
}

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

    // Fail fast before any mutation: deterministic repository/revision
    // failures are synchronously detectable (read-only Git) and must
    // refuse here — never as a stranded RUNNING entry in the background
    // (TASK-0054, G5 B-RS boundary B′).
    let git = match composition.git_cli() {
        Ok(git) => git,
        Err(_) => {
            return (
                exec_error(BusinessErrorCode::Internal, "git executable unavailable"),
                None,
            );
        }
    };
    if let Err(failure) = resolve_entry_revision(&git, &req.repository_path, &base_revision) {
        return (exec_error(failure.code, failure.message), None);
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
    let planning = match PlanningService::open(&composition.db_path, app_services::SystemClock) {
        Ok(service) => service,
        Err(error) => {
            return (
                exec_error(
                    BusinessErrorCode::Internal,
                    format!("execution planning gate unavailable: {error}"),
                ),
                None,
            );
        }
    };
    match orchestration.enter_from_current_plan(&planning, &task_id, ids) {
        Ok(PlannedExecutionEntryOutcome::Entered {
            entry: ExecutionEntry::Runnable(work),
            ..
        }) => {
            // Exclusive live ownership (TASK-0056, G5 B-CR): the run owns
            // the live provider/cancellation lifecycle only after winning
            // the atomic claim. A live owner for the same execution can
            // only exist when the execution already committed elsewhere,
            // which `enter_execution` refuses first — so a lost claim here
            // is unreachable in practice and fails closed without touching
            // the current owner's entry or spawning a competing run.
            let cancel = provider_claude::CancelFlag::fresh();
            let Some(owner) = composition.cancels.try_claim(&execution_id, cancel.clone()) else {
                return (
                    exec_error(
                        BusinessErrorCode::Conflict,
                        format!(
                            "execution {} already has a live run; status shows the owner",
                            truncate_id(execution_id.as_str())
                        ),
                    ),
                    None,
                );
            };
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
                owner,
            };
            (response, Some(job))
        }
        Ok(PlannedExecutionEntryOutcome::Entered {
            entry: ExecutionEntry::NotRunnable(outcome),
            ..
        }) => (not_runnable_error(&task_id, outcome), None),
        Ok(PlannedExecutionEntryOutcome::NoAcceptedPlan) => (
            exec_error(
                BusinessErrorCode::StaleState,
                "task has no current accepted plan; complete planning before execution",
            ),
            None,
        ),
        Ok(PlannedExecutionEntryOutcome::AmbiguousAcceptedPlan { .. }) => (
            exec_error(
                BusinessErrorCode::Conflict,
                "task is represented by multiple current accepted plans; reconcile planning before execution",
            ),
            None,
        ),
        Ok(PlannedExecutionEntryOutcome::WaitingOnPlanDependency { .. }) => (
            exec_error(
                BusinessErrorCode::Conflict,
                "task is waiting on an accepted-plan predecessor",
            ),
            None,
        ),
        Ok(PlannedExecutionEntryOutcome::WaitingForHuman { decision, .. }) => (
            exec_error(
                BusinessErrorCode::Conflict,
                format!(
                    "task is waiting for human decision {}",
                    truncate_id(decision.id.as_str())
                ),
            ),
            None,
        ),
        Ok(PlannedExecutionEntryOutcome::AbortedByHuman { decision, .. }) => (
            exec_error(
                BusinessErrorCode::Conflict,
                format!(
                    "bounded task scope was aborted by human decision {}",
                    truncate_id(decision.id.as_str())
                ),
            ),
            None,
        ),
        Ok(
            PlannedExecutionEntryOutcome::PlanHeadMoved { .. }
            | PlannedExecutionEntryOutcome::PlanningInputMoved { .. },
        ) => (
            exec_error(
                BusinessErrorCode::StaleState,
                "accepted planning state changed before execution admission; reconcile and retry",
            ),
            None,
        ),
        Err(PlannedOrchestrationError::Orchestration(error)) => {
            let (code, message) = crate::dispatch::map_error(&error);
            (exec_error(code, message), None)
        }
        Err(PlannedOrchestrationError::Planning(error)) => {
            let (code, message) = match error {
                PlanningServiceError::Storage(storage) => {
                    crate::dispatch::map_error(&ServiceError::Storage(storage))
                }
                PlanningServiceError::Planner(_) => (
                    BusinessErrorCode::ProviderUnavailable,
                    "planning provider failed while resolving execution admission".to_owned(),
                ),
                PlanningServiceError::Domain(_) | PlanningServiceError::InvalidContext(_) => (
                    BusinessErrorCode::CorruptState,
                    "accepted planning state failed validation".to_owned(),
                ),
            };
            (exec_error(code, message), None)
        }
    }
}

// -- resume --------------------------------------------------------------
///
/// Recovers one admitted-but-not-attempted execution (TASK-0054, G5 B-RS)
/// through the production control plane: a crash (or deterministic
/// failure) after admission `COMMIT` but before `create_attempt` leaves
/// Task `RUNNING` + one execution + one Attempt reservation with zero
/// attempts, which ordinary `run` honestly refuses.
///
/// Resume re-drives the SAME execution with its reserved attempt identity
/// on a detached background thread (the same [`run_background`] half
/// `run` uses): no Task transition, no new execution, no fresh attempt
/// identity, no provider history. Refusals are typed with no mutation:
/// missing execution (`NOT_FOUND`), an execution that already records
/// attempts or whose task left `RUNNING` (`INVALID_TRANSITION`), a live
/// owner already running this execution in this daemon (`CONFLICT` — the
/// loser spawns nothing and disturbs no registry entry), and unusable
/// workspace inputs (`INVALID_ARGUMENT`). Corrupt reservation or lease
/// state fails closed (`CORRUPT_STATE`).
///
/// `repository_path` is always required (server-local operational input,
/// never persisted). When no lease exists yet, `repository_ref` and
/// `base_revision` are required; when a lease exists they default to the
/// recorded lease values and supplied values must agree (fail fast here,
/// not in the background).
fn resume_run(
    composition: &ExecutionComposition,
    req: ResumeExecutionRequest,
) -> (ExecutionCoreResponse, Option<BackgroundJob>) {
    let execution_id = match ExecutionId::new(req.execution_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                exec_error(
                    BusinessErrorCode::InvalidArgument,
                    "execution id must be a non-blank bounded identifier",
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

    // Fail fast before any mutation: same operational gates as `run`.
    if let Err(detail) = provider_precheck(composition) {
        return (
            exec_error(BusinessErrorCode::ProviderUnavailable, detail),
            None,
        );
    }
    let resume_root = match ensure_managed_root(&composition.db_path) {
        Ok(root) => root,
        Err(detail) => return (exec_error(BusinessErrorCode::Internal, detail), None),
    };

    // Read-only recovery plan: exactly one healthy pre-Attempt entry, or
    // a typed refusal with nothing mutated.
    let orchestration =
        match OrchestrationService::open(&composition.db_path, app_services::SystemClock) {
            Ok(service) => service,
            Err(error) => {
                return (
                    exec_error(
                        BusinessErrorCode::Internal,
                        format!("execution recovery unavailable: {error}"),
                    ),
                    None,
                );
            }
        };
    let plan = match orchestration.plan_pre_attempt_resume(&execution_id) {
        Ok(plan) => plan,
        Err(error) => {
            // A concurrent winner may have claimed this execution and
            // created its Attempt after this request began planning. Live
            // ownership is then the current refusal reason (TASK-0056),
            // even though the durable snapshot now also says recovery is
            // past the pre-Attempt boundary.
            if composition.cancels.is_live(&execution_id) {
                return (
                    exec_error(
                        BusinessErrorCode::Conflict,
                        format!(
                            "execution {} already has a live run; status shows the owner",
                            truncate_id(execution_id.as_str())
                        ),
                    ),
                    None,
                );
            }
            let (code, message) = crate::dispatch::map_error(&error);
            return (exec_error(code, message), None);
        }
    };

    // Workspace inputs: lease-recorded defaults, operator overrides must
    // agree. Supplied values that disagree with a recorded lease refuse
    // here — not as a second strand in the background.
    let repository_ref = match plan.repository_ref.as_deref() {
        Some(recorded) => {
            if !req.repository_ref.trim().is_empty() && req.repository_ref != recorded {
                return (
                    exec_error(
                        BusinessErrorCode::InvalidArgument,
                        "repository ref disagrees with the recorded lease; omit it to reuse the recorded value",
                    ),
                    None,
                );
            }
            recorded.to_owned()
        }
        None => {
            if req.repository_ref.trim().is_empty() {
                return (
                    exec_error(
                        BusinessErrorCode::InvalidArgument,
                        "repository ref is required when no workspace lease exists yet",
                    ),
                    None,
                );
            }
            req.repository_ref.clone()
        }
    };
    let repository_ref = match RepositoryRef::new(repository_ref) {
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
    let base_revision = match plan.base_revision.as_deref() {
        Some(recorded) => {
            if req.base_revision.trim().is_empty() || req.base_revision == recorded {
                recorded.to_owned()
            } else {
                if req.base_revision.len() > MAX_REVISION_LEN {
                    return (
                        exec_error(
                            BusinessErrorCode::InvalidArgument,
                            "base revision exceeds the bounded length",
                        ),
                        None,
                    );
                }
                req.base_revision.clone()
            }
        }
        None => {
            if req.base_revision.trim().is_empty() {
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
            }
        }
    };

    // Synchronous read-only Git validation, exactly like `run` entry:
    // deterministic failures refuse here with no mutation. For a
    // lease-backed resume the recorded base OID must additionally still
    // resolve in the supplied repository (wrong-repository paths fail
    // closed here); an explicitly supplied revision must resolve to that
    // same recorded base (bases are immutable).
    let git = match composition.git_cli() {
        Ok(git) => git,
        Err(_) => {
            return (
                exec_error(BusinessErrorCode::Internal, "git executable unavailable"),
                None,
            );
        }
    };
    let resolved = match resolve_entry_revision(&git, &req.repository_path, &base_revision) {
        Ok(revision) => revision,
        Err(failure) => return (exec_error(failure.code, failure.message), None),
    };
    if let Some(recorded) = plan.base_revision.as_deref() {
        if resolved.as_str() != recorded {
            return (
                exec_error(
                    BusinessErrorCode::InvalidArgument,
                    "base revision disagrees with the recorded lease base; omit it to reuse the recorded value",
                ),
                None,
            );
        }
    }

    let workspace_request = WorkspaceRequest {
        execution_id: execution_id.clone(),
        repository_ref,
        repository_path: req.repository_path.clone(),
        base_revision,
    };

    // TASK-0057 B-RS2: refuse deterministic workspace ambiguity before
    // reporting resume acceptance or claiming live ownership. This is a
    // read-only early diagnostic only; the background cycle re-runs the
    // authoritative reconciliation immediately before mutation, so races
    // between this check and execution remain fail-closed.
    let workspace_preflight = match WorkspaceService::open(
        &composition.db_path,
        git,
        resume_root,
        app_services::SystemClock,
    ) {
        Ok(service) => service,
        Err(error) => {
            return (
                exec_error(
                    BusinessErrorCode::CorruptState,
                    format!("workspace recovery preflight unavailable: {error}"),
                ),
                None,
            );
        }
    };
    if let Err(error) = workspace_preflight.validate_resume_workspace(&workspace_request) {
        let code = match error {
            WorkspaceFailure::NotRepository { .. }
            | WorkspaceFailure::BareRepositoryUnsupported { .. }
            | WorkspaceFailure::InvalidRevision { .. } => BusinessErrorCode::InvalidArgument,
            WorkspaceFailure::StorageFailure { .. } => BusinessErrorCode::CorruptState,
            _ => BusinessErrorCode::Conflict,
        };
        return (
            exec_error(code, format!("workspace recovery refused: {error}")),
            None,
        );
    }

    // Exclusive live ownership (TASK-0056, G5 B-CR): resume shares the
    // SAME ownership domain as run. All validation above is read-only, so
    // a lost claim refuses with a typed conflict and zero mutation: no
    // competing background continuation, no replaced cancel handle, no
    // unregistration of the winner. A request reports acceptance of the
    // live continuation only after winning this claim.
    let cancel = provider_claude::CancelFlag::fresh();
    let Some(owner) = composition.cancels.try_claim(&execution_id, cancel.clone()) else {
        return (
            exec_error(
                BusinessErrorCode::Conflict,
                format!(
                    "execution {} already has a live run; status shows the owner",
                    truncate_id(execution_id.as_str())
                ),
            ),
            None,
        );
    };
    let response = ExecutionCoreResponse {
        payload: Some(execution_core_response::Payload::Resume(
            ResumeExecutionResponse {
                task_id: plan.work.task_id.to_string(),
                execution_id: execution_id.to_string(),
                attempt_id: plan.work.attempt_id.to_string(),
                task_state: TaskState::Running.as_str().to_owned(),
            },
        )),
    };
    let job = BackgroundJob {
        composition: composition.clone(),
        work: plan.work,
        workspace_request,
        cancel,
        owner,
    };
    (response, Some(job))
}

// -- recovery scan -------------------------------------------------------
///
/// One stranded pre-attempt entry for operator triage: durable identities
/// only, no Git or provider state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrandedPreAttemptEntry {
    /// Execution admitted without an attempt.
    pub execution_id: String,
    /// Task owning the execution.
    pub task_id: String,
    /// Reserved Attempt identity owned by the execution.
    pub attempt_id: String,
}

/// Lists the stranded pre-attempt entries in one database. Read-only:
/// opens the store, answers from committed rows, emits no events. An
/// unreadable database yields an empty list — this scan is triage
/// reporting, never a correctness boundary (the resume dispatch
/// re-validates everything before acting).
#[must_use]
pub fn scan_pre_attempt_strands(db_path: &Path) -> Vec<StrandedPreAttemptEntry> {
    let store = match storage_sqlite::SqliteStore::open(db_path) {
        Ok(store) => store,
        Err(_) => return Vec::new(),
    };
    match store.list_pre_attempt_stranded_executions() {
        Ok(entries) => entries
            .into_iter()
            .map(|entry| StrandedPreAttemptEntry {
                execution_id: entry.execution_id.to_string(),
                task_id: entry.task_id.to_string(),
                attempt_id: entry.attempt_id.to_string(),
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Best-effort daemon-startup report of stranded pre-attempt entries:
/// each line names the execution, its task and reserved attempt, plus the
/// exact resume invocation. Emits nothing when no strand exists. Never
/// mutates, never spawns provider work: automatic background resurrection
/// is rejected (the workspace inputs are operator-supplied operational
/// data, and surprise provider spawns after a restart are the G11 orphan
/// class) — the architecturally correct startup action is to surface the
/// strand honestly and await the explicit, auditable resume.
pub fn log_pre_attempt_strands(db_path: &Path) {
    for strand in &scan_pre_attempt_strands(db_path) {
        eprintln!(
            "agentd: pre-attempt recovery: execution {} (task {}, attempt {}) committed entry but recorded no attempt; resume with: agentctl execution resume {} <repository-path>",
            strand.execution_id, strand.task_id, strand.attempt_id, strand.execution_id,
        );
    }
}
/// Cheap provider precheck without spawning: the configured executable
/// must resolve. Version/auth surface later as typed attempt outcomes —
/// this gate only refuses the structurally hopeless case before entry.
///
/// NOTE (TASK-0049 H5, evaluated and DEFERRED): wiring the adapter's
/// `detect` version gate here would be the natural fast-fail for an
/// incompatible CLI, but `detect` can block up to its 10 s probe timeout
/// while `start_run` must answer within the 5 s IPC exchange bound — a
/// hanging executable wedges `run` past the client deadline (proven when
/// the gate was trialled: the cancel-path binary test timed out on its
/// 5 s frame). A production version gate needs a bounded-fast probe
/// design (cached/background detection) owned by a dedicated follow-up,
/// not a synchronous spawn on the entry path. The deferral is safe: an
/// incompatible CLI still fails closed at parse time with honest state
/// and no corruption (parse rejects it as `MalformedOutput`; it is never
/// granted authority).
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

/// Provider-writable runtime state lives beside other application-owned
/// execution state, never under the managed Git worktree. The Claude adapter
/// adds the durable worktree leaf below this base so restart/resume reuses the
/// same environment without re-deriving historical workspace identity.
fn provider_environment_base(db_path: &Path) -> Result<PathBuf, String> {
    let parent = db_path.parent().ok_or_else(|| {
        format!(
            "provider environment unavailable: database path {} has no parent",
            db_path.display()
        )
    })?;
    Ok(parent
        .join("provider-environments")
        .join(provider_claude::CLAUDE_PROVIDER_REF))
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
            if execution.attempts().is_empty() {
                return handle_cancel_no_attempt(&store, &execution_id);
            }
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

/// Honest cancel posture for an execution with zero attempts (TASK-0054,
/// HARD-3): the old code reported `ALREADY_CONCLUDED … no attempts
/// recorded` for a strand that never concluded anything. A committed
/// entry with its reservation intact is recoverable work awaiting an
/// explicit resume — never a conclusion. Zero attempts with no (or an
/// ambiguous) reservation is unexpected durable state: reported as no
/// live run with an inspection pointer, never as concluded.
fn handle_cancel_no_attempt(
    store: &storage_sqlite::SqliteStore,
    execution_id: &ExecutionId,
) -> ExecutionCoreResponse {
    let reservations = match store.list_attempt_reservations_for_execution(execution_id) {
        Ok(reservations) => reservations,
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    if reservations.len() != 1 {
        return ExecutionCoreResponse {
            payload: Some(execution_core_response::Payload::Cancel(
                CancelExecutionResponse {
                    execution_id: execution_id.to_string(),
                    cancel_requested: false,
                    run_state: "NO_LIVE_RUN".to_owned(),
                    detail: format!(
                        "no live provider run for execution {}; no attempts recorded and no pending reservation (unexpected durable state) — inspect status",
                        truncate_id(execution_id.as_str())
                    ),
                },
            )),
        };
    }
    let hint = match store.get_workspace_for_execution(execution_id) {
        Ok(None) => format!(
            "resume with: agentctl execution resume {} <repository-path> --ref <repository-ref> --rev <base-revision>",
            truncate_id(execution_id.as_str())
        ),
        Ok(Some(lease)) => format!(
            "resume with: agentctl execution resume {} <repository-path> (lease {})",
            truncate_id(execution_id.as_str()),
            lease.lifecycle(),
        ),
        Err(_) => {
            return exec_error(BusinessErrorCode::Internal, "internal storage failure");
        }
    };
    ExecutionCoreResponse {
        payload: Some(execution_core_response::Payload::Cancel(
            CancelExecutionResponse {
                execution_id: execution_id.to_string(),
                cancel_requested: false,
                run_state: "AWAITING_RESUME".to_owned(),
                detail: format!(
                    "execution {} committed entry but recorded no attempt; {}",
                    truncate_id(execution_id.as_str()),
                    hint,
                ),
            },
        )),
    }
}

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
/// the domain already bounds. Cuts on a char boundary: detail is
/// operator-facing text and may be multibyte (TASK-0049 H1).
fn truncate_detail(detail: &str) -> String {
    const MAX: usize = MAX_STATUS_DETAIL_CHARS;
    if detail.len() <= MAX {
        return detail.to_owned();
    }
    let end = crate::dispatch::floor_char_boundary(detail, MAX);
    format!("{}…", &detail[..end])
}

/// Tighter bound for identifiers embedded in messages. Cuts on a char
/// boundary: validated execution/attempt identities may be multibyte,
/// and a raw byte cut would panic the connection handler (TASK-0049 H1).
fn truncate_id(value: &str) -> String {
    const MAX_ID_MESSAGE_LEN: usize = 64;
    if value.len() > MAX_ID_MESSAGE_LEN {
        format!(
            "{}…",
            &value[..crate::dispatch::floor_char_boundary(value, MAX_ID_MESSAGE_LEN)]
        )
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
        seed_ready_with_objective(db, project, task, format!("objective {task}"));
    }

    /// Authors a planning parent `<task>-PARENT` (carrying `objective`) and
    /// publishes an accepted plan whose only child is `task`. Acceptance
    /// materializes `task` as a new READY Task under the parent (TASK-0087);
    /// its objective is the materialization placeholder.
    fn seed_ready_with_objective(
        db: &Path,
        project: &str,
        task: &str,
        objective: impl Into<String>,
    ) {
        let mut tasks = app_services::TaskService::open(db, app_services::SystemClock)
            .expect("open task service");
        let project_id = task_domain::ProjectId::new(project).unwrap();
        tasks
            .create_project(
                &task_domain::Project::new(project_id.clone(), format!("project {project}"))
                    .unwrap(),
            )
            .unwrap();
        let parent_id = task_domain::TaskId::new(fixture_parent(task)).unwrap();
        tasks
            .create_task(
                &task_domain::Task::draft(parent_id.clone(), project_id, objective).unwrap(),
            )
            .unwrap();
        // Publication admits a decomposition only while the parent is doing
        // work (ADR-0027 TASK-0091 amendment).
        tasks.mark_ready(&parent_id).unwrap();
        seed_accepted_plan(db, task);
    }

    fn fixture_parent(task: &str) -> String {
        format!("{task}-PARENT")
    }

    fn seed_accepted_plan(db: &Path, task: &str) {
        let mut planning = app_services::PlanningService::open(db, app_services::SystemClock)
            .expect("open planning service");
        let scope = plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap();
        let evidence = plan_domain::EvidenceRef::new(format!("fixture-plan:{task}")).unwrap();
        let branch = plan_domain::PlanBranchId::new(format!("fixture-plan-{task}")).unwrap();
        let planner =
            fake_planner::FakePlanner::for_single_leaf(plan_domain::PlanNodeId::new(task).unwrap());
        let request = plan_domain::DecompositionRequest {
            parent_task_ref: fixture_parent(task),
            base_plan_revision: 0,
            acceptance_criteria_refs: vec![],
            scope_bound: scope.clone(),
            evidence_refs: vec![evidence],
            depth_remaining: plan_domain::MAX_DECOMPOSITION_DEPTH,
            budget_remaining: plan_domain::PlanningBudget {
                calls_remaining: plan_domain::MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
                input_tokens_remaining: plan_domain::MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
            },
        };
        let policy = app_services::PlanningPolicyContext {
            scope_bound: scope,
            baseline_risk: plan_domain::RiskClass::Low,
            affected_parent_criteria: vec![],
            parent_task_ref: fixture_parent(task),
            canonical_task_authority: false,
            lineage_fingerprints: vec![],
            risk_grant: None,
            risk_decision_id: None,
            policy_fingerprint: format!("policy:fixture:{task}"),
        };
        let outcome = planning
            .propose_decomposition(&planner, &branch, request, &policy)
            .expect("seed accepted plan");
        assert!(matches!(
            outcome,
            app_services::PlanningProposalOutcome::Accepted(_)
        ));
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

    fn custom_run(task: &str, execution: &str, attempt: &str, repo: &Path) -> RunExecutionRequest {
        RunExecutionRequest {
            task_id: task.to_owned(),
            execution_id: execution.to_owned(),
            attempt_id: attempt.to_owned(),
            repository_ref: "repo-local:keeper".to_owned(),
            repository_path: repo.to_string_lossy().into_owned(),
            base_revision: "HEAD".to_owned(),
        }
    }

    fn run_via_dispatch(
        composition: &ExecutionComposition,
        req: RunExecutionRequest,
    ) -> (ExecutionCoreResponse, Option<BackgroundJob>) {
        dispatch_execution(
            composition,
            ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Run(req)),
            },
        )
    }

    fn already_exists(response: &ExecutionCoreResponse) {
        match &response.payload {
            Some(execution_core_response::Payload::Error(error)) => {
                assert_eq!(
                    error.code,
                    BusinessErrorCode::AlreadyExists as i32,
                    "expected ALREADY_EXISTS, got {error:?}"
                );
            }
            other => panic!("expected ALREADY_EXISTS, got {other:?}"),
        }
    }

    fn run_payload(response: &ExecutionCoreResponse) -> RunExecutionResponse {
        match &response.payload {
            Some(execution_core_response::Payload::Run(run)) => run.clone(),
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn cancel_registry_is_per_execution_and_owner_scoped() {
        let registry = CancelRegistry::fresh();
        let live = ExecutionId::new("E-live").unwrap();
        let other = ExecutionId::new("E-other").unwrap();
        assert!(!registry.is_live(&live));
        assert!(!registry.signal(&live));
        let owner = registry
            .try_claim(&live, provider_claude::CancelFlag::fresh())
            .expect("first claim wins");
        assert!(registry.is_live(&live));
        assert!(!registry.is_live(&other));
        assert!(registry.signal(&live));
        assert!(
            registry.signal(&live),
            "repeat signal stays true while live"
        );
        // A foreign token retires nothing.
        assert!(!registry.release(&live, LiveOwnerToken(u64::MAX)));
        assert!(registry.is_live(&live));
        assert!(registry.release(&live, owner));
        assert!(!registry.is_live(&live));
        assert!(!registry.signal(&live));
        // Releasing twice is a safe no-op, never an error.
        assert!(!registry.release(&live, owner));
    }

    #[test]
    fn cancel_registry_claim_never_overwrites_and_stale_release_is_safe() {
        // ABA keeper (TASK-0056 B-CR): A claims, A completes, B claims, a
        // delayed cleanup carrying A's token runs — B must stay registered
        // with its own cancel handle intact.
        let registry = CancelRegistry::fresh();
        let execution = ExecutionId::new("E-aba").unwrap();
        let flag_a = provider_claude::CancelFlag::fresh();
        let token_a = registry
            .try_claim(&execution, flag_a.clone())
            .expect("A claims");
        // A second claim for the live execution loses without disturbing A.
        assert!(
            registry
                .try_claim(&execution, provider_claude::CancelFlag::fresh())
                .is_none(),
            "live execution refuses a second claim"
        );
        assert!(registry.signal(&execution));
        assert!(flag_a.is_cancelled(), "signal reaches A's handle");
        assert!(registry.release(&execution, token_a));
        assert!(!registry.is_live(&execution));

        let flag_b = provider_claude::CancelFlag::fresh();
        let token_b = registry
            .try_claim(&execution, flag_b.clone())
            .expect("B claims after A released");
        assert_ne!(token_a, token_b, "successive owners differ");
        // Stale cleanup from A is a no-op: B stays live and signallable.
        assert!(!registry.release(&execution, token_a));
        assert!(registry.is_live(&execution));
        assert!(registry.signal(&execution));
        assert!(
            flag_b.is_cancelled(),
            "signal still reaches the current owner's handle"
        );
        assert!(registry.release(&execution, token_b));
        assert!(!registry.is_live(&execution));
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
    fn provider_launch_security_rejects_unresolvable_workspace() {
        let sandbox = Sandbox::named("g6-unresolvable-workspace");
        let task = task_domain::Task::draft(
            task_domain::TaskId::new("T-1").unwrap(),
            task_domain::ProjectId::new("P-1").unwrap(),
            "objective",
        )
        .unwrap();
        let missing_workspace = sandbox.dir.join("missing-worktree");
        let lease = workspace::WorkspaceLease::allocate(
            workspace::WorkspaceId::new("WS-1").unwrap(),
            workspace::RepositoryRef::new("repo-local:g6-unresolvable").unwrap(),
            ExecutionId::new("E-1").unwrap(),
            workspace::Revision::new("a".repeat(40)).unwrap(),
            workspace::BranchRef::new("refs/heads/task-sec-r1").unwrap(),
            missing_workspace.to_string_lossy().into_owned(),
        )
        .unwrap();
        let work = scheduler::WorkItem {
            task_id: task_domain::TaskId::new("T-1").unwrap(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: agent_runtime::AttemptId::new("A-1").unwrap(),
        };

        let result = build_provider_launch_security(
            &task,
            &lease,
            &work,
            std::ffi::OsStr::new("/usr/bin/true"),
        );
        assert!(result.is_err(), "unresolvable workspace must fail closed");
        assert!(
            result.err().unwrap().contains("provider launch workspace"),
            "failure should identify the bounded launch-workspace refusal"
        );
    }

    #[test]
    fn provider_launch_emits_g6_security_audit() {
        let sandbox = Sandbox::named("g6-security-audit");
        seed_ready(&sandbox.db, "P-1", "T-1");
        let fixture = fixture_script(
            &sandbox.dir,
            "success.sh",
            "printf '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"security keeper finished\",\"session_id\":\"ffffffff-ffff-ffff-ffff-ffffffffffff\"}'; exit 0\n",
        );
        let composition = sandbox.composition(Some(fixture));
        let (response, job) = run_via_dispatch(&composition, run_request("T-1", &sandbox.repo));
        let run = run_payload(&response);

        run_background(job.expect("accepted run yields a background job"));

        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let audit = store
            .list_security_audit_ordered()
            .expect("security audit must be readable");
        assert!(
            audit.iter().any(|record| {
                record.fact.event == storage_sqlite::SecurityAuditEvent::PolicyDecision
                    && record.fact.execution_id.as_deref() == Some(run.execution_id.as_str())
            }),
            "production provider launch must record the canonical policy decision"
        );
        assert!(
            audit.iter().any(|record| {
                record.fact.event == storage_sqlite::SecurityAuditEvent::IsolationEnforcement
                    && record.fact.execution_id.as_deref() == Some(run.execution_id.as_str())
            }),
            "production provider launch must record runner enforcement"
        );
        assert!(
            audit.iter().any(|record| {
                record.fact.event == storage_sqlite::SecurityAuditEvent::ProviderInvocation
                    && record.fact.execution_id.as_deref() == Some(run.execution_id.as_str())
            }),
            "production provider launch must record provider invocation"
        );
    }

    /// Genuine environment facts a real-provider smoke may pass through:
    /// the account or network is unavailable, not the integration. Config
    /// drift (an unknown model), authentication gaps and undiagnosed exits
    /// are defects and must fail.
    fn is_tolerable_provider_environment_failure(detail: &str) -> bool {
        const ENVIRONMENT_MARKERS: &[&str] = &[
            "rate limit",
            "rate_limit",
            "usage limit",
            "quota",
            "overloaded",
            "network",
            "enotfound",
            "econnrefused",
            "timed out",
            "timeout",
        ];
        let haystack = detail.to_ascii_lowercase();
        ENVIRONMENT_MARKERS
            .iter()
            .any(|marker| haystack.contains(marker))
    }

    #[test]
    #[ignore = "real provider: needs RUNLITH_CLAUDE_SMOKE=1 plus authenticated claude CLI"]
    fn real_claude_runs_through_production_g6_isolation() {
        if std::env::var("RUNLITH_CLAUDE_SMOKE").as_deref() != Ok("1") {
            eprintln!("smoke skipped: set RUNLITH_CLAUDE_SMOKE=1");
            return;
        }
        let detection = provider_claude::detect(std::ffi::OsStr::new("claude"));
        assert_eq!(
            detection.status,
            provider_claude::ProviderAvailability::Available,
            "enabled production smoke requires an available Claude CLI: {}",
            detection.detail
        );

        let sandbox = Sandbox::named("g6-real-claude");
        seed_ready_with_objective(
            &sandbox.db,
            "P-G6-REAL",
            "T-G6-REAL",
            "Do not use any tools. Reply with exactly G6_PRODUCTION_SANDBOX_OK and nothing else.",
        );
        let composition = sandbox.composition(None);
        let (response, job) =
            run_via_dispatch(&composition, run_request("T-G6-REAL", &sandbox.repo));
        let _ = run_payload(&response);
        run_background(job.expect("accepted real-provider run yields a background job"));

        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let attempt = store
            .get_attempt(&agent_runtime::AttemptId::new("A-T-G6-REAL").unwrap())
            .expect("read real-provider attempt")
            .expect("real-provider attempt exists");
        let task = store
            .get_task(&TaskId::new("T-G6-REAL").unwrap())
            .expect("read real-provider task")
            .expect("real-provider task exists");
        match attempt.outcome() {
            Some(AttemptOutcome::Succeeded) => assert_eq!(
                task.state(),
                task_domain::TaskState::Verifying,
                "provider success must reach independent verification, never self-complete"
            ),
            Some(AttemptOutcome::Failed) => {
                let detail = attempt.detail().unwrap_or_default();
                assert!(
                    !detail.contains("operation not permitted")
                        && !detail.contains("EPERM")
                        && !detail.contains("sandbox-exec")
                        && !detail.contains("runtime launch preparation refused"),
                    "production sandbox blocked the real provider: {detail}"
                );
                // Tolerate only named environment facts. An earlier revision
                // tolerated `unrecognized_model`, which made a stale model pin
                // the pass condition: the smoke stayed green for two gates
                // while the real call never succeeded. Anything unrecognized,
                // including a bare exit with no diagnostic, is a defect until
                // proven otherwise.
                assert!(
                    is_tolerable_provider_environment_failure(detail),
                    "real provider failed for a non-environment reason: {detail}"
                );
            }
            other => panic!("unexpected real-provider outcome: {other:?}"),
        }

        let audit = store
            .list_security_audit_ordered()
            .expect("read production security audit");
        assert!(audit.iter().any(|record| {
            record.fact.event == storage_sqlite::SecurityAuditEvent::IsolationEnforcement
                && record.fact.execution_id.as_deref() == Some("E-T-G6-REAL")
        }));
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
        let fixture = fixture_script(&sandbox.dir, "slow.sh", "while :; do :; done\n");
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

    // -- B1 identity-safety keepers (TASK-0049) ------------------------------
    //
    // A synchronously detectable execution/attempt identity conflict must
    // refuse with ALREADY_EXISTS before committing any unrelated durable
    // fact: no claim, no start, no execution, no lease, no attempt, no
    // worktree, and no misleading journal lifecycle facts. The rejected
    // Task stays in its pre-run state and is rerunnable with fresh ids.

    #[test]
    fn run_execution_id_collision_refuses_without_mutation() {
        let sandbox = Sandbox::named("exec-collision");
        seed_ready(&sandbox.db, "P-A", "T-A");
        seed_ready(&sandbox.db, "P-B", "T-B");
        let fixture = fixture_script(&sandbox.dir, "unused.sh", "exit 0\n");
        let composition = sandbox.composition(Some(fixture));

        // Task A owns execution E-A (entry committed; background not spawned).
        let (accepted, job_a) =
            run_via_dispatch(&composition, custom_run("T-A", "E-A", "A-A", &sandbox.repo));
        assert_eq!(run_payload(&accepted).execution_id, "E-A");
        assert!(job_a.is_some());

        // Task B requests the execution identity owned by Task A.
        let (refused, job_b) =
            run_via_dispatch(&composition, custom_run("T-B", "E-A", "A-B", &sandbox.repo));
        assert!(
            job_b.is_none(),
            "refused run must not yield a background job"
        );
        already_exists(&refused);

        // Task B is untouched: still READY, owns nothing, no lease, no
        // worktree, and no lifecycle facts in the journal.
        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let task_b = store
            .get_task(&TaskId::new("T-B").unwrap())
            .expect("read task")
            .expect("T-B exists");
        assert_eq!(
            task_b.state(),
            TaskState::Ready,
            "refused run must not claim/start Task B"
        );
        let execution_a = store
            .get_execution(&ExecutionId::new("E-A").unwrap())
            .expect("read execution")
            .expect("E-A exists");
        assert_eq!(
            execution_a.task_id().to_string(),
            "T-A",
            "E-A must stay owned by Task A"
        );
        assert!(
            store
                .get_attempt(&agent_runtime::AttemptId::new("A-B").unwrap())
                .expect("read attempt")
                .is_none(),
            "no attempt may be created for the refused run"
        );
        assert!(
            store
                .get_workspace_for_execution(&ExecutionId::new("E-A").unwrap())
                .expect("read lease")
                .is_none(),
            "entry alone creates no lease"
        );
        let workspaces = sandbox.db.parent().unwrap().join("workspaces");
        if workspaces.is_dir() {
            let entries = std::fs::read_dir(&workspaces).expect("read workspaces");
            assert_eq!(
                entries.count(),
                0,
                "refused run must not allocate a worktree"
            );
        }
        for event in store
            .list_events_for_task(&TaskId::new("T-B").unwrap())
            .expect("journal")
        {
            assert!(
                !matches!(
                    event.fact.event_type,
                    storage_sqlite::EventType::Claimed
                        | storage_sqlite::EventType::Started
                        | storage_sqlite::EventType::ExecutionCreated
                        | storage_sqlite::EventType::AttemptCreated
                        | storage_sqlite::EventType::WorkspaceCreated
                ),
                "refused run must leave no lifecycle facts, got {:?}",
                event.fact.event_type
            );
        }

        // Correcting the identities lets Task B start normally: the
        // rejected call did not poison its lifecycle.
        let (retry, job_retry) =
            run_via_dispatch(&composition, custom_run("T-B", "E-B", "A-B", &sandbox.repo));
        assert_eq!(run_payload(&retry).task_state, "RUNNING");
        assert!(job_retry.is_some());
    }

    #[test]
    fn run_attempt_id_collision_refuses_without_mutation() {
        let sandbox = Sandbox::named("attempt-collision");
        seed_ready(&sandbox.db, "P-A", "T-A");
        seed_ready(&sandbox.db, "P-B", "T-B");
        let fixture = fixture_script(&sandbox.dir, "unused.sh", "exit 0\n");
        let composition = sandbox.composition(Some(fixture));

        // Task A owns execution E-A; attempt A-1 exists under it.
        let (accepted, job_a) =
            run_via_dispatch(&composition, custom_run("T-A", "E-A", "A-1", &sandbox.repo));
        assert_eq!(run_payload(&accepted).execution_id, "E-A");
        assert!(job_a.is_some());
        let mut runtime =
            app_services::RuntimeService::open(&sandbox.db, app_services::SystemClock)
                .expect("open runtime service");
        runtime
            .create_attempt(
                &ExecutionId::new("E-A").unwrap(),
                agent_runtime::AttemptId::new("A-1").unwrap(),
                agent_runtime::ProviderRef::new("keeper-stub").unwrap(),
            )
            .expect("setup attempt A-1");

        // Task B requests a fresh execution but a reused attempt identity.
        let (refused, job_b) =
            run_via_dispatch(&composition, custom_run("T-B", "E-B", "A-1", &sandbox.repo));
        assert!(
            job_b.is_none(),
            "refused run must not yield a background job"
        );
        already_exists(&refused);

        // Task B is untouched and E-B was never created: no execution, no
        // lease, no worktree, no journal lifecycle facts. A-1 still
        // belongs to E-A.
        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let task_b = store
            .get_task(&TaskId::new("T-B").unwrap())
            .expect("read task")
            .expect("T-B exists");
        assert_eq!(
            task_b.state(),
            TaskState::Ready,
            "refused run must not claim/start Task B"
        );
        assert!(
            store
                .get_execution(&ExecutionId::new("E-B").unwrap())
                .expect("read execution")
                .is_none(),
            "refused run must not create execution E-B"
        );
        let attempt = store
            .get_attempt(&agent_runtime::AttemptId::new("A-1").unwrap())
            .expect("read attempt")
            .expect("A-1 exists");
        assert_eq!(
            attempt.execution_id().to_string(),
            "E-A",
            "A-1 must stay owned by E-A"
        );
        assert!(
            store
                .get_workspace_for_execution(&ExecutionId::new("E-B").unwrap())
                .expect("read lease")
                .is_none(),
            "refused run must not allocate a lease"
        );
        let workspaces = sandbox.db.parent().unwrap().join("workspaces");
        if workspaces.is_dir() {
            let entries = std::fs::read_dir(&workspaces).expect("read workspaces");
            assert_eq!(
                entries.count(),
                0,
                "refused run must not allocate a worktree"
            );
        }
        for event in store
            .list_events_for_task(&TaskId::new("T-B").unwrap())
            .expect("journal")
        {
            assert!(
                !matches!(
                    event.fact.event_type,
                    storage_sqlite::EventType::Claimed
                        | storage_sqlite::EventType::Started
                        | storage_sqlite::EventType::ExecutionCreated
                        | storage_sqlite::EventType::AttemptCreated
                        | storage_sqlite::EventType::WorkspaceCreated
                ),
                "refused run must leave no lifecycle facts, got {:?}",
                event.fact.event_type
            );
        }

        // Correcting the identities lets Task B start normally.
        let (retry, job_retry) = run_via_dispatch(
            &composition,
            custom_run("T-B", "E-B2", "A-2", &sandbox.repo),
        );
        assert_eq!(run_payload(&retry).task_state, "RUNNING");
        assert!(job_retry.is_some());
    }

    // -- H1 multibyte identifier robustness (TASK-0049) ----------------------
    //
    // Validated execution identities may be multibyte (up to 256 bytes).
    // Error rendering must cut on char boundaries: a long multibyte id
    // on the NOT_FOUND paths must yield a typed error, never panic the
    // connection handler, and leave the composition usable.

    fn multibyte_ids() -> Vec<(&'static str, String, bool)> {
        vec![
            ("ascii", format!("E-{}", "x".repeat(100)), false),
            (
                "two-byte",
                format!("{}é{}", "A".repeat(63), "B".repeat(10)),
                true,
            ),
            (
                "three-byte",
                format!("{}€{}", "A".repeat(62), "B".repeat(10)),
                true,
            ),
            (
                "four-byte",
                format!("{}😀{}", "A".repeat(63), "B".repeat(10)),
                true,
            ),
            (
                "mixed",
                format!("{}é€😀{}", "A".repeat(61), "B".repeat(10)),
                true,
            ),
        ]
    }

    #[test]
    fn multibyte_execution_ids_never_panic_status_or_cancel() {
        let sandbox = Sandbox::named("multibyte-ids");
        seed_ready(&sandbox.db, "P-1", "T-1");
        let fixture = fixture_script(&sandbox.dir, "unused.sh", "exit 0\n");
        let composition = sandbox.composition(Some(fixture));

        for (name, id, mid_char) in multibyte_ids() {
            // Sanity: the id is valid, long enough to truncate, and (for
            // the Unicode shapes) byte 64 sits strictly inside a char —
            // the exact shape that panicked a raw byte cut.
            assert!(
                id.len() > 64,
                "{name}: test id must exceed the message bound"
            );
            assert!(
                id.len() <= 256,
                "{name}: test id must stay a valid identifier"
            );
            assert!(
                ExecutionId::new(&id).is_ok(),
                "{name}: test id must validate"
            );
            assert_eq!(
                !id.is_char_boundary(64),
                mid_char,
                "{name}: byte-64 boundary expectation"
            );

            // Status for the absent execution: typed NOT_FOUND, no panic.
            let (status, status_job) = dispatch_execution(
                &composition,
                ExecutionCoreRequest {
                    payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                        execution_id: id.clone(),
                    })),
                },
            );
            assert!(status_job.is_none());
            match &status.payload {
                Some(execution_core_response::Payload::Error(error)) => {
                    assert_eq!(
                        error.code,
                        BusinessErrorCode::NotFound as i32,
                        "{name} status: got {error:?}"
                    );
                    assert!(
                        error.message.contains("does not exist"),
                        "{name} status message must stay meaningful, got {:?}",
                        error.message
                    );
                }
                other => panic!("{name} status: expected NOT_FOUND, got {other:?}"),
            }

            // Cancel for the absent execution: typed NOT_FOUND, no panic.
            let (cancel, cancel_job) = dispatch_execution(
                &composition,
                ExecutionCoreRequest {
                    payload: Some(execution_core_request::Payload::Cancel(
                        CancelExecutionRequest {
                            execution_id: id.clone(),
                        },
                    )),
                },
            );
            assert!(cancel_job.is_none());
            match &cancel.payload {
                Some(execution_core_response::Payload::Error(error)) => {
                    assert_eq!(
                        error.code,
                        BusinessErrorCode::NotFound as i32,
                        "{name} cancel: got {error:?}"
                    );
                }
                other => panic!("{name} cancel: expected NOT_FOUND, got {other:?}"),
            }
        }

        // The composition is still fully usable after all ten error paths:
        // the handler survived every multibyte refusal.
        let (retry, job) = run_via_dispatch(&composition, run_request("T-1", &sandbox.repo));
        assert_eq!(run_payload(&retry).task_state, "RUNNING");
        assert!(job.is_some());
    }

    #[test]
    fn truncate_id_cuts_multibyte_on_char_boundaries() {
        for (_, id, _) in multibyte_ids() {
            let cut = truncate_id(&id);
            assert!(
                cut.ends_with('…'),
                "long id must be marked truncated, got {cut:?}"
            );
            // Stripping the marker leaves valid UTF-8 within the bound.
            let kept = cut.strip_suffix('…').expect("marker");
            assert!(kept.len() <= 64, "kept {kept:?} must fit the bound");
            assert!(
                id.starts_with(kept),
                "truncation must keep a prefix, got {cut:?}"
            );
        }
        assert_eq!(truncate_id("E-short"), "E-short");
    }
}
