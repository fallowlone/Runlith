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
use evaluation::{
    EffectCapturePort, EvaluationPort, EvaluationRequest, EvaluationResult, EvaluationVerdict,
};
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
static NEXT_CANDIDATE_OPERATION: AtomicU64 = AtomicU64::new(1);
const PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED: &str =
    "provider_authentication_unsupported_in_mandatory_confinement";

/// What this platform's production isolation backend implies for the
/// provider's own authentication mechanism.
///
/// Every platform build constructs exactly one of these, so the remaining
/// variants are necessarily unconstructed in that build.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
enum ConfinementAuthVerdict {
    /// No mandatory confinement is installed here. The shared runner verifier
    /// refuses a confined launch on its own axis before any provider work, so
    /// there is no separate authentication question to answer.
    NoMandatoryConfinement,
    /// Measured: the provider can authenticate from inside this platform's
    /// confinement. No platform reports this today; the variant exists so a
    /// confining platform must choose it deliberately rather than inherit
    /// silence.
    Compatible,
    /// Measured: the provider's mechanism needs authority the confinement
    /// withholds, so the launch is refused before any provider work.
    Unsupported(&'static str),
}

/// The production isolation backend and the provider-authentication verdict
/// are two halves of one platform decision, so they are defined together
/// rather than at two independent `cfg` sites. A newly added `cfg` arm cannot
/// omit the verdict, because `prepare_provider_launch` reads it unconditionally.
///
/// Co-location is a review aid, not an enforced coupling: an author could
/// still make an existing arm return a confining backend while leaving its
/// verdict untouched, and a backend could start reporting enforcement axes
/// without this module changing at all. Those two paths are covered at test
/// time by `production_confinement_and_provider_auth_verdict_stay_paired`,
/// which checks the verdict against the backend's *reported axes* rather than
/// against the shape of the `cfg`.
///
/// What that guard does not prove: it compares presence of confinement against
/// presence of a verdict, never the verdict's continued truth. A verdict that
/// goes stale — a Seatbelt profile later permitting the authentication path
/// while the reported axes stay unchanged — keeps it green. The verdict is a
/// hand-maintained TASK-0112 measurement, and nothing here re-measures it.
/// A backend selected at runtime rather than by `cfg` would also be measured
/// on the test host, which need not match production.
#[cfg(target_os = "macos")]
mod production_platform {
    use super::{ConfinementAuthVerdict, IsolationBackend, PathBuf, RunnerLaunchPurpose};
    use std::sync::Arc;

    pub(super) fn runner_backend(
        support_read: Vec<PathBuf>,
        support_write: Vec<PathBuf>,
        purpose: RunnerLaunchPurpose,
    ) -> Result<Arc<dyn IsolationBackend>, String> {
        let backend = runner_macos_seatbelt::MacOsSeatbeltRunner::with_support_roots(
            support_read,
            support_write,
        );
        let backend = match purpose {
            // Owner-ratified 2026-09-25 (R1): only a candidate-input
            // Runner launch (Git-read, snapshot-write, evaluator-read-
            // only) gets the ancestor metadata/existence grant. This
            // branch is the only place that decides it, from a compiled
            // `RunnerLaunchPurpose` value this module's own call sites
            // pass — never a caller/IPC string.
            RunnerLaunchPurpose::CandidateInput => backend.with_candidate_ancestor_metadata_grant(),
            RunnerLaunchPurpose::Provider => backend,
        };
        Ok(Arc::new(backend))
    }

    /// TASK-0112 measured the provider-owned macOS authentication path as
    /// requiring Keychain authority G6 deliberately withholds, so TASK-0114
    /// refuses an otherwise valid authenticated CLI before provider work.
    pub(super) const AUTH_CONFINEMENT_VERDICT: ConfinementAuthVerdict =
        ConfinementAuthVerdict::Unsupported(
            "authenticated Claude CLI state is host-valid, but TASK-0112 measured that the \
             current provider-owned macOS authentication path requires Keychain authority \
             intentionally unavailable to the mandatory G6 sandbox",
        );
}

#[cfg(not(target_os = "macos"))]
mod production_platform {
    use super::{ConfinementAuthVerdict, IsolationBackend, PathBuf, RunnerLaunchPurpose};
    use std::sync::Arc;

    pub(super) fn runner_backend(
        _support_read: Vec<PathBuf>,
        _support_write: Vec<PathBuf>,
        _purpose: RunnerLaunchPurpose,
    ) -> Result<Arc<dyn IsolationBackend>, String> {
        Ok(Arc::new(runner_native::NativeRunner))
    }

    /// `NativeRunner` reports no enforcement axes, so the shared runner
    /// verifier refuses a confined launch on its own axis before any provider
    /// work and no separate authentication verdict applies.
    pub(super) const AUTH_CONFINEMENT_VERDICT: ConfinementAuthVerdict =
        ConfinementAuthVerdict::NoMandatoryConfinement;
}

/// Owner-ratified 2026-09-25 (R1): which Runner surface a launch is for,
/// decided only by this module's own trusted call sites — never derived
/// from caller/IPC input. Governs whether the macOS Seatbelt backend
/// grants the narrow ancestor metadata/existence read (candidate-input
/// launches only; see ADR-0035 "Amendment 2026-09-25").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunnerLaunchPurpose {
    /// Provider/tool process launches: exactly main's Seatbelt behavior,
    /// no ancestor grant.
    Provider,
    /// Candidate-input Git-read, snapshot-write, or evaluator-read-only
    /// launches: the narrow ancestor metadata/existence grant applies.
    CandidateInput,
}

pub(crate) fn production_runner_backend(
    support_read: Vec<PathBuf>,
    support_write: Vec<PathBuf>,
    purpose: RunnerLaunchPurpose,
) -> Result<Arc<dyn IsolationBackend>, String> {
    production_platform::runner_backend(support_read, support_write, purpose)
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
    /// Canonical Git executable for candidate-input plumbing reads only.
    /// On macOS, resolves to Apple's toolchain locations and rejects symlink
    /// redirection. On other platforms, falls back to PATH resolution since
    /// candidate input is unsupported there.
    candidate_git_executable: Result<PathBuf, String>,
    /// Composition-owned provider executable override. `None` resolves
    /// `claude` via `PATH`; `Some` points at an explicit binary (tests
    /// inject fixture doubles). Never client-supplied — no IPC field
    /// maps here, so a client cannot turn the daemon into an arbitrary
    /// process launcher.
    provider_executable: Option<PathBuf>,
    /// Composition-owned override for the binary the candidate-input
    /// evaluator re-execs (see
    /// `CandidateInputEvaluator::evaluator_executable_override`). `None`
    /// (every production composition) resolves the real running `agentd`
    /// binary via `current_exe()`. Never client-supplied.
    candidate_evaluator_executable_override: Option<PathBuf>,
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
            candidate_git_executable: resolve_candidate_git_executable(),
            provider_executable,
            candidate_evaluator_executable_override: None,
        }
    }

    /// Test-only builder: overrides the binary the candidate-input
    /// evaluator re-execs. Production never calls this — the field stays
    /// `None` and resolves `current_exe()`, the real running `agentd`
    /// binary. Under `cargo test`, `current_exe()` is the test-harness
    /// binary (no `__candidate-*` dispatch), so a test exercising the real
    /// snapshot-write/evaluate re-exec must supply the actual built
    /// `agentd` binary here.
    #[cfg(test)]
    #[must_use]
    fn with_candidate_evaluator_executable_override(mut self, path: PathBuf) -> Self {
        self.candidate_evaluator_executable_override = Some(path);
        self
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

/// ADR-0035 pinned trusted Git for candidate-input plumbing reads.
/// Resolves a Git executable from a list of candidate paths, rejecting
/// symlink redirection and requiring each candidate to be a regular file
/// within its own install prefix. This ensures candidate Git reads use
/// a trusted, pinned executable regardless of PATH or sandbox state.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn resolve_pinned_git(candidates: &[&Path]) -> Result<PathBuf, String> {
    for candidate in candidates {
        // Must be metadata-accessible and resolve to a regular file.
        if std::fs::symlink_metadata(candidate).is_err() {
            continue;
        }

        let canonical = match std::fs::canonicalize(candidate) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if !canonical.is_file() {
            continue;
        }

        // Reject symlink redirection: canonical path must equal the
        // candidate's filename joined to its parent's canonical path.
        let candidate_parent = match candidate.parent() {
            Some(p) => p,
            None => continue,
        };

        let canonical_parent = match candidate_parent.canonicalize() {
            Ok(p) => p,
            Err(_) => continue,
        };

        let expected_canonical = match candidate.file_name() {
            Some(name) => canonical_parent.join(name),
            None => continue,
        };

        if canonical == expected_canonical {
            return Ok(canonical);
        }
    }

    Err("no pinned trusted Git executable is available for candidate-input reads".to_owned())
}

/// Platform-specific resolver for candidate Git executable.
/// On macOS, restricts to Apple's toolchain locations and rejects symlink
/// redirection. On other platforms, falls back to PATH resolution since
/// candidate input is unsupported there and the candidate Seatbelt
/// restriction does not apply.
fn resolve_candidate_git_executable() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        resolve_pinned_git(&[
            Path::new("/Library/Developer/CommandLineTools/usr/bin/git"),
            Path::new("/Applications/Xcode.app/Contents/Developer/usr/bin/git"),
        ])
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Candidate-input Git reads are unsupported off macOS. Keep the
        // prior PATH-resolved behavior to avoid breaking non-candidate
        // operations on platforms where candidate evaluation is unavailable.
        resolve_git_executable()
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateG6Operation {
    GitRead,
    SnapshotWrite,
    EvaluatorReadOnly,
}

struct CandidateProcessRequest {
    operation: CandidateG6Operation,
    task_id: String,
    execution_id: String,
    attempt_id: String,
    invocation_id: String,
    workspace: PathBuf,
    filesystem_roots: Vec<PathBuf>,
    executable: PathBuf,
    args: Vec<std::ffi::OsString>,
    input: Vec<u8>,
    support_read: Vec<PathBuf>,
    support_write: Vec<PathBuf>,
    max_stdout: u64,
}

impl CandidateG6Operation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::GitRead => "git-read",
            Self::SnapshotWrite => "snapshot-write",
            Self::EvaluatorReadOnly => "evaluator-read-only",
        }
    }

    fn file_capability(self, scope: FileScope) -> Capability {
        match self {
            Self::SnapshotWrite => Capability::FilesystemWrite(scope),
            Self::GitRead | Self::EvaluatorReadOnly => Capability::FilesystemRead(scope),
        }
    }
}

fn build_candidate_launch_security(
    request: &CandidateProcessRequest,
) -> Result<ProviderLaunchSecurity, String> {
    let mut capabilities = Vec::new();
    for root in &request.filesystem_roots {
        let root = root
            .canonicalize()
            .map_err(|_| "candidate process filesystem root is unavailable".to_owned())?;
        let root = root
            .to_str()
            .ok_or_else(|| "candidate process filesystem root is not UTF-8".to_owned())?;
        capabilities.push(
            request.operation.file_capability(
                FileScope::new(root, ".")
                    .map_err(|error| format!("candidate filesystem scope: {error}"))?,
            ),
        );
    }
    if capabilities.is_empty() {
        return Err("candidate operation has no filesystem scope".to_owned());
    }
    let executable = request
        .executable
        .canonicalize()
        .map_err(|_| "candidate process executable is unavailable".to_owned())?;
    let executable = executable
        .to_str()
        .ok_or_else(|| "candidate process executable is not UTF-8".to_owned())?;
    let executable = ScopeId::new(executable.to_owned())
        .map_err(|error| format!("candidate executable scope: {error}"))?;
    capabilities.push(Capability::ProcessExecute(executable.clone()));
    let workspace = request
        .workspace
        .canonicalize()
        .map_err(|_| "candidate process workspace unavailable".to_owned())?
        .to_str()
        .ok_or_else(|| "candidate process workspace is not UTF-8".to_owned())?
        .to_owned();
    let policy_request = PolicyRequest::new(
        format!(
            "candidate.{}:{}:{}:{}",
            request.operation.as_str(),
            request.execution_id,
            request.attempt_id,
            request.invocation_id
        ),
        "candidate-input-evaluator",
        RequestOrigin::new(
            request.task_id.clone(),
            Some(request.execution_id.clone()),
            Some(request.attempt_id.clone()),
        )
        .map_err(|error| format!("candidate request origin: {error}"))?,
        RequestedCapabilities::new(capabilities.clone()),
    )
    .map_err(|error| format!("candidate policy request: {error}"))?;
    let rules = capabilities
        .iter()
        .enumerate()
        .map(|(index, capability)| {
            PolicyRule::new(
                format!("builtin.candidate.{}.{}", request.operation.as_str(), index),
                "trusted_candidate_input_operation",
                PolicyLayer::BuiltInHard,
                PolicyOutcome::Allow,
                capability.clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("candidate built-in policy: {error}"))?;
    let policy = CompiledPolicy::compile(rules)
        .map_err(|error| format!("candidate compiled policy: {error}"))?;
    Ok(ProviderLaunchSecurity {
        request: policy_request,
        parent: EffectiveCapabilities::new(capabilities),
        policy,
        workspace,
        executable,
    })
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
    // providers.md §9a: authentication is verified before the launch, never
    // discovered during it. Only a provider detection recognizes as the real,
    // compatible CLI is asked: a fixture or an unrecognized binary has not
    // claimed to be the working provider and owns no credential, and §9
    // already answers those. Scope limit: this observes the operator's state
    // on the host, not a confined child's ability to reach the credential
    // mechanism from inside an isolation profile.
    let provider_executable = std::ffi::OsStr::new(launch.executable.as_str());
    if provider_claude::detect(provider_executable).status
        == provider_claude::ProviderAvailability::Available
    {
        let authentication = provider_claude::probe_authentication(provider_executable);
        if !authentication.state.permits_launch() {
            return Err(authentication.refusal());
        }
        if let ConfinementAuthVerdict::Unsupported(reason) =
            production_platform::AUTH_CONFINEMENT_VERDICT
        {
            return Err(format!("{PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED}: {reason}"));
        }
    }

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
    let backend =
        production_runner_backend(support_read, support_write, RunnerLaunchPurpose::Provider)?;
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

fn prepare_candidate_process(
    db_path: &Path,
    request: &CandidateProcessRequest,
) -> Result<PreparedProviderLaunch, String> {
    let executable_scope = request
        .executable
        .canonicalize()
        .map_err(|_| "candidate process executable cannot be canonicalized".to_owned())?;
    let executable_scope = executable_scope
        .to_str()
        .ok_or_else(|| "candidate process executable is not UTF-8".to_owned())?;
    let executable_scope = ScopeId::new(executable_scope.to_owned())
        .map_err(|error| format!("candidate executable scope: {error}"))?;
    let workspace = request
        .workspace
        .canonicalize()
        .map_err(|_| "candidate process workspace cannot be canonicalized".to_owned())?;
    let workspace = workspace
        .to_str()
        .ok_or_else(|| "candidate process workspace is not UTF-8".to_owned())?
        .to_owned();
    let launch = build_candidate_launch_security(request)?;
    let mut security = G6SecurityService::open(db_path, app_services::SystemClock)
        .map_err(|error| format!("G6 candidate authorization unavailable: {error}"))?;
    let authorization = security
        .authorize(&launch.request, &launch.parent, &launch.policy, u64::MAX)
        .map_err(|error| format!("G6 candidate authorization failed: {error}"))?;
    let effective = match authorization {
        G6Authorization::Authorized { effective, .. } => effective,
        G6Authorization::ApprovalRequired { .. } => {
            return Err("G6 approval required for candidate input operation".to_owned());
        }
        G6Authorization::Denied { .. } => {
            return Err("G6 denied candidate input operation".to_owned());
        }
    };
    let operation_record = PrivilegedOperation::from_policy_request(&launch.request);
    let requirements = IsolationRequirements::new(true, true, false);
    let environment = if request.operation == CandidateG6Operation::GitRead {
        ChildEnvironment::from_explicit([(
            std::ffi::OsString::from("GIT_CONFIG_NOSYSTEM"),
            std::ffi::OsString::from("1"),
        )])
        .map_err(|error| format!("candidate Git environment is invalid: {error}"))?
    } else {
        ChildEnvironment::empty()
    };
    let backend = production_runner_backend(
        request.support_read.clone(),
        request.support_write.clone(),
        RunnerLaunchPurpose::CandidateInput,
    )?;
    let spec = ProcessSecuritySpec::new(
        workspace.clone(),
        executable_scope.clone(),
        requirements,
        environment.clone(),
        "runlith.candidate-input-evaluator.v1",
    );
    let prepared = runner::prepare_process_launch(
        Arc::clone(&backend),
        &effective,
        workspace,
        executable_scope,
        requirements,
        environment,
    )
    .map_err(|error| format!("Runner refused candidate input operation: {error}"))?;
    Ok(PreparedProviderLaunch {
        security,
        operation: operation_record,
        backend,
        effective,
        spec,
        launch: Arc::new(prepared),
    })
}

struct CandidateProcessOutput {
    stdout: Vec<u8>,
    runner: CandidateRunnerReceipt,
}

struct CandidateRunnerReceipt {
    operation_id: String,
    backend: String,
    plan_sha256: String,
    enforced_axes: u8,
    environment_isolated: bool,
}

#[derive(Clone)]
struct CandidateInputEvaluator {
    db_path: PathBuf,
    git_executable: PathBuf,
    snapshot_parent: PathBuf,
    capture: effect_capture_local::LocalEffectCapture,
    /// The trusted binary the Runner re-execs (with hidden
    /// `__candidate-snapshot-write`/`__candidate-evaluate`/
    /// `__candidate-snapshot-remove` arguments) to materialize and
    /// evaluate the sealed candidate snapshot. Production always uses
    /// `None`, resolving to `std::env::current_exe()` — the real running
    /// `agentd` binary, whose `main()` owns that dispatch. Tests that
    /// exercise this real re-exec path (not just the pure-Rust helper
    /// functions) must override it with the actual built `agentd` binary:
    /// under `cargo test --lib`, `current_exe()` resolves to the test
    /// harness binary, which has no such dispatch and cannot stand in for
    /// production here.
    evaluator_executable_override: Option<PathBuf>,
}

enum ProductionEvaluator {
    Candidate(CandidateInputEvaluator),
    Pending(PendingEvaluator),
}

impl EvaluationPort for ProductionEvaluator {
    fn evaluate(&self, request: &EvaluationRequest) -> EvaluationResult<EvaluationVerdict> {
        match self {
            Self::Candidate(evaluator) => {
                // Owner decision 2026-09-25 (adversarial review finding TASK-0144):
                // production candidate-input evaluation records typed integrity and
                // captures evidence (durable RESULT_ADMITTED), but then reports "no
                // verdict" instead of releasing a completion verdict. The queue scope
                // explicitly excludes completion verdicts and new completion authority.
                // The Task stays VERIFYING for TASK-0125 claim verification.
                let verdict = evaluator.evaluate(request)?;
                match verdict {
                    EvaluationVerdict::Verified => Err(
                        evaluation::EvaluationError::new(
                            "candidate-input evidence admitted (check passed); completion awaits claim verification"
                        )
                    ),
                    EvaluationVerdict::Rejected => Err(
                        evaluation::EvaluationError::new(
                            "candidate-input evidence admitted (check failed); completion awaits claim verification"
                        )
                    ),
                }
            }
            Self::Pending(evaluator) => evaluator.evaluate(request),
        }
    }

    // Without this override, `EvaluationPort`'s default no-op
    // `revalidate_before_completion` silently wins for every
    // `ProductionEvaluator` variant, including `Candidate`: the durable
    // Completion freshness checkpoint and the `RESULT_ADMITTED` ->
    // `COMPLETE` operation-status transition would never run. For `Pending`,
    // this call is needed, so we still delegate (though `finish()` in
    // orchestration_service.rs will never invoke this for `Pending` unless
    // a future completion-stage caller (TASK-0125) explicitly requests it).
    // For `Candidate`: owner decision 2026-09-25 makes `evaluate()` return
    // no verdict, so `finish()` never reaches this override — the Task stays
    // VERIFYING. The delegation is kept so a future claim-verification caller
    // can request the Completion freshness gate.
    fn revalidate_before_completion(&self, request: &EvaluationRequest) -> EvaluationResult<()> {
        match self {
            Self::Candidate(evaluator) => evaluator.revalidate_before_completion(request),
            Self::Pending(evaluator) => evaluator.revalidate_before_completion(request),
        }
    }
}

struct CandidateFreshnessRequest<'a> {
    request: &'a EvaluationRequest,
    invocation_id: &'a str,
    root: &'a Path,
    anchor: &'a workspace::TrustedCommonGit,
    candidate: &'a evaluation::VerifiedCandidate,
    format: evaluation::GitObjectFormat,
    phase: evaluation::CandidateCheckpointPhase,
    started_at_ms: u64,
}

struct CandidateGitContext<'a> {
    request: &'a EvaluationRequest,
    invocation_id: &'a str,
    workspace: &'a Path,
    gitdir: &'a Path,
    roots: Vec<PathBuf>,
}

impl EvaluationPort for CandidateInputEvaluator {
    fn evaluate(&self, request: &EvaluationRequest) -> EvaluationResult<EvaluationVerdict> {
        self.evaluate_candidate(request)
            .map_err(evaluation::EvaluationError::new)
    }

    fn revalidate_before_completion(&self, request: &EvaluationRequest) -> EvaluationResult<()> {
        self.revalidate_candidate_completion(request)
            .map_err(evaluation::EvaluationError::new)
    }
}

impl CandidateInputEvaluator {
    fn supports_task(task: &task_domain::Task) -> bool {
        let requirements = task.evidence_requirements();
        if task.acceptance().len() != 1 || requirements.len() != 1 {
            return false;
        }
        evaluation::CandidateSelector::from_requirement(&requirements[0])
            .is_ok_and(|selector| selector.criterion_id == task.acceptance()[0])
    }

    fn evaluate_candidate(&self, request: &EvaluationRequest) -> Result<EvaluationVerdict, String> {
        let result = self.evaluate_candidate_inner(request);
        if let Err(error) = &result {
            if let Ok(mut store) = storage_sqlite::SqliteStore::open(&self.db_path) {
                let status = if error.contains("changed") || error.contains("mismatch") {
                    "MISMATCH"
                } else if error.contains("object graph") || error.contains("tamper") {
                    "TAMPERED"
                } else {
                    "BLOCKED"
                };
                let _ = store.record_candidate_operation_failure(
                    request.execution_id.as_str(),
                    request.attempt_id.as_str(),
                    status,
                    &status.to_ascii_lowercase(),
                    now_millis(),
                );
            }
        }
        result
    }

    fn evaluate_candidate_inner(
        &self,
        request: &EvaluationRequest,
    ) -> Result<EvaluationVerdict, String> {
        let invocation_id = format!(
            "candidate-{}-{}-{}",
            request.execution_id.as_str(),
            request.attempt_id.as_str(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let mut store = storage_sqlite::SqliteStore::open(&self.db_path)
            .map_err(|error| format!("open candidate evidence store: {error}"))?;
        let task = store
            .get_task(&request.task_id)
            .map_err(|error| format!("load candidate task: {error}"))?
            .ok_or_else(|| "candidate task does not exist".to_owned())?;
        let execution = store
            .get_execution(&request.execution_id)
            .map_err(|error| format!("load candidate execution: {error}"))?
            .ok_or_else(|| "candidate execution does not exist".to_owned())?;
        if execution.task_id() != &request.task_id {
            return Err("candidate task/execution binding mismatch".to_owned());
        }
        let attempt = execution
            .get_attempt(&request.attempt_id)
            .ok_or_else(|| "candidate attempt does not belong to execution".to_owned())?;
        if attempt.outcome() != Some(AttemptOutcome::Succeeded)
            || task.state() != TaskState::Verifying
        {
            return Err("candidate task or attempt is not in the evaluatable state".to_owned());
        }
        let requirements = task
            .evidence_requirements()
            .iter()
            .filter(|requirement| requirement.required)
            .map(evaluation::CandidateSelector::from_requirement)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("candidate requirement is unsupported or stale: {error}"))?;
        if requirements.len() != 1 || task.acceptance().len() != 1 {
            return Err(
                "candidate evaluator supports exactly one explicitly authorized criterion"
                    .to_owned(),
            );
        }
        if task.acceptance()[0] != requirements[0].criterion_id {
            return Err("candidate selector criterion does not match task acceptance".to_owned());
        }
        let base = store
            .get_evaluated_policy_base(request.execution_id.as_str())
            .map_err(|error| format!("load evaluated policy base: {error}"))?
            .ok_or_else(|| "candidate execution has no evaluated base binding".to_owned())?;
        if base.task_id != request.task_id.as_str()
            || base.policy.policy() != &policy::TestMutationPolicy::Forbidden
        {
            return Err("candidate base or test-mutation policy binding is stale".to_owned());
        }
        let lease = store
            .get_workspace_for_execution(&request.execution_id)
            .map_err(|error| format!("load candidate workspace: {error}"))?
            .ok_or_else(|| "candidate execution has no workspace lease".to_owned())?;
        if lease.lifecycle() != workspace::LeaseLifecycle::Ready
            || base.execution_id != request.execution_id.as_str()
            || base.workspace_id != lease.id().as_str()
            || base.base_revision != lease.base_revision().as_str()
        {
            return Err("candidate workspace lease is not READY".to_owned());
        }
        let anchor = store
            .get_trusted_anchor(lease.id())
            .map_err(|error| format!("load trusted Git anchor: {error}"))?
            .ok_or_else(|| "candidate workspace has no trusted Git anchor".to_owned())?;
        let root = PathBuf::from(lease.worktree_ref());
        let selected_pointer =
            read_authorized_worktree_pointer(&self.db_path, request, &invocation_id, &root)?;
        let linked_gitdir = authenticated_worktree_gitdir(
            &self.db_path,
            request,
            &invocation_id,
            &root,
            &selected_pointer,
            &anchor,
        )?;
        let git_context = CandidateGitContext {
            request,
            invocation_id: &invocation_id,
            workspace: anchor.canonical_commondir(),
            gitdir: &linked_gitdir,
            roots: vec![anchor.canonical_commondir().to_path_buf()],
        };
        let git_version = self.git_output(&git_context, &["--version"], 128)?;
        let git_version = std::str::from_utf8(&git_version)
            .map_err(|_| "candidate Git version is not UTF-8".to_owned())?
            .strip_suffix('\n')
            .ok_or_else(|| "candidate Git version is incomplete".to_owned())?;
        if !git_version.starts_with("git version ") || git_version.len() > 128 {
            return Err("candidate Git version is unsupported".to_owned());
        }
        let object_format =
            self.git_output(&git_context, &["rev-parse", "--show-object-format"], 64)?;
        let object_format = std::str::from_utf8(&object_format)
            .map_err(|_| "Git object format output is unsupported".to_owned())?
            .strip_suffix('\n')
            .ok_or_else(|| "Git object format output is incomplete".to_owned())?;
        let object_format = evaluation::GitObjectFormat::from_repository_format(object_format)
            .map_err(|error| format!("unsupported Git object format: {error}"))?;
        let head = self.git_output(&git_context, &["rev-parse", "--verify", "HEAD"], 128)?;
        let candidate_oid = std::str::from_utf8(&head)
            .map_err(|_| "candidate HEAD is not UTF-8".to_owned())?
            .strip_suffix('\n')
            .ok_or_else(|| "candidate HEAD output is incomplete".to_owned())?;
        let selector = requirements
            .first()
            .ok_or_else(|| "candidate selector missing".to_owned())?;
        let _new_selection = store
            .record_candidate_selection(
                request.task_id.as_str(),
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
                &invocation_id,
                &base.base_revision,
                candidate_oid,
                &selector.requirement_revision,
                base.policy.revision(),
                base.policy.fingerprint(),
                &selector.criterion_id,
                &selector.selector,
                &selector.path,
                &selector.literal,
                now_millis(),
            )
            .map_err(|error| {
                format!("persist candidate selection before object resolution: {error}")
            })?;
        self.validate_git_object_sources(&git_context)?;
        let candidate = self.verify_commit(&git_context, candidate_oid, object_format)?;
        let base_candidate =
            self.verify_commit(&git_context, &base.base_revision, object_format)?;
        // The completion-independent getter: it requires only the four
        // pre-completion checkpoints, so it returns durable evidence for
        // both a bare RESULT_ADMITTED row and a COMPLETE one. R2 (owner
        // review 2026-09-25): re-invoking `evaluate()` while the operation
        // is RESULT_ADMITTED must be a safe idempotent path — never the
        // `StorageError::Corrupt` the completion-gated getter raises on a
        // row that has not reached Completion yet.
        if let Some(evidence) = store
            .get_candidate_evidence_for_completion(
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
            )
            .map_err(|error| format!("load prior candidate evidence: {error}"))?
        {
            let stored = &evidence.integrity;
            if stored.task_id != request.task_id.as_str()
                || stored.execution_id != request.execution_id.as_str()
                || stored.attempt_id != request.attempt_id.as_str()
                || stored.subject_ref != format!("attempt:{}", request.attempt_id.as_str())
                || stored.requirement_revision != requirements[0].requirement_revision
                || stored.policy_revision != base.policy.revision()
                || stored.policy_fingerprint != base.policy.fingerprint()
                || stored.criterion_id != requirements[0].criterion_id
                || stored.selector != requirements[0].selector
                || stored.selector_path != requirements[0].path
                || stored.selector_literal != requirements[0].literal
                || stored.base_commit_oid != base_candidate.commit_oid
                || stored.base_tree_oid != base_candidate.tree_oid
                || stored.base_manifest_sha256 != base_candidate.manifest.sha256
                || stored.candidate_commit_oid != candidate.commit_oid
                || stored.candidate_tree_oid != candidate.tree_oid
                || stored.candidate_manifest_sha256 != candidate.manifest.sha256
            {
                return Err("durable candidate evidence binding changed; reuse refused".to_owned());
            }
            let operation_status = store
                .candidate_operation_status(
                    request.execution_id.as_str(),
                    request.attempt_id.as_str(),
                )
                .map_err(|error| format!("load candidate operation status: {error}"))?;
            // Only a COMPLETE row's reuse is architecturally a "restart":
            // `append_candidate_checkpoint` only accepts a `RestartReuse`
            // checkpoint once a matching `Completion` checkpoint already
            // exists. A RESULT_ADMITTED row (completion has not run yet)
            // instead simply re-reports its already-admitted verdict below
            // without touching checkpoints at all — "reuse admitted result
            // pending completion revalidation" (R2) — and leaves the actual
            // freshness re-check to whichever `revalidate_before_completion`
            // call eventually runs.
            if operation_status.as_deref() == Some("COMPLETE") {
                let checkpoint = self.freshness_checkpoint(CandidateFreshnessRequest {
                    request,
                    invocation_id: &invocation_id,
                    root: &root,
                    anchor: &anchor,
                    candidate: &candidate,
                    format: object_format,
                    phase: evaluation::CandidateCheckpointPhase::RestartReuse,
                    started_at_ms: now_millis().min(stored.completed_at_ms),
                })?;
                if !checkpoint.matched {
                    return Err(
                        "candidate inputs changed; durable evidence reuse refused".to_owned()
                    );
                }
                store
                    .append_candidate_checkpoint(
                        request.execution_id.as_str(),
                        request.attempt_id.as_str(),
                        &checkpoint,
                    )
                    .map_err(|error| format!("persist candidate reuse checkpoint: {error}"))?;
            }
            return match evidence.effect.payload() {
                evaluation::EffectPayload::TestSuiteResult {
                    passed: _, failed, ..
                } => Ok(if *failed == 0 {
                    EvaluationVerdict::Verified
                } else {
                    EvaluationVerdict::Rejected
                }),
                _ => Err("durable candidate result payload is unsupported".to_owned()),
            };
        }
        let started_at_ms = now_millis();
        let mut checkpoints = Vec::new();
        for phase in [
            evaluation::CandidateCheckpointPhase::Selection,
            evaluation::CandidateCheckpointPhase::BeforeEvaluation,
        ] {
            checkpoints.push(self.freshness_checkpoint(CandidateFreshnessRequest {
                request,
                invocation_id: &invocation_id,
                root: &root,
                anchor: &anchor,
                candidate: &candidate,
                format: object_format,
                phase,
                started_at_ms,
            })?);
        }
        if checkpoints.iter().any(|checkpoint| !checkpoint.matched) {
            return Err("candidate inputs changed before evaluation".to_owned());
        }
        let snapshot_name = format!(
            "candidate-snapshot-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let snapshot_input = encode_snapshot_files(&candidate.files)?;
        let snapshot_writer = match &self.evaluator_executable_override {
            Some(path) => path.clone(),
            None => std::env::current_exe()
                .map_err(|_| "agentd evaluator executable unavailable".to_owned())?,
        };
        let evaluator_executable = snapshot_writer
            .canonicalize()
            .map_err(|_| "agentd evaluator executable cannot be canonicalized".to_owned())?;
        let evaluator_executable_ref = evaluator_executable
            .to_str()
            .ok_or_else(|| "agentd evaluator executable path is unsupported".to_owned())?
            .to_owned();
        let snapshot_path = self
            .snapshot_parent
            .join("candidate-snapshots")
            .join(&snapshot_name);
        run_candidate_process(
            &self.db_path,
            CandidateProcessRequest {
                operation: CandidateG6Operation::SnapshotWrite,
                task_id: request.task_id.as_str().to_owned(),
                execution_id: request.execution_id.as_str().to_owned(),
                attempt_id: request.attempt_id.as_str().to_owned(),
                invocation_id: format!("{invocation_id}.snapshot-write"),
                workspace: self.snapshot_parent.clone(),
                filesystem_roots: vec![self.snapshot_parent.clone()],
                executable: snapshot_writer,
                args: vec![
                    "__candidate-snapshot-write".into(),
                    self.snapshot_parent.as_os_str().into(),
                    snapshot_name.clone().into(),
                ],
                input: snapshot_input,
                support_read: vec![self.snapshot_parent.clone()],
                support_write: vec![self.snapshot_parent.clone()],
                max_stdout: 0,
            },
        )?;
        let output = run_candidate_process(
            &self.db_path,
            CandidateProcessRequest {
                operation: CandidateG6Operation::EvaluatorReadOnly,
                task_id: request.task_id.as_str().to_owned(),
                execution_id: request.execution_id.as_str().to_owned(),
                attempt_id: request.attempt_id.as_str().to_owned(),
                invocation_id: format!("{invocation_id}.evaluator"),
                // `workspace` must match the sole filesystem root below
                // exactly (the Runner refuses a launch whose authorized
                // scope targets a different workspace than the plan's
                // own): the evaluator is confined to this one sealed
                // snapshot directory, never the whole snapshot parent.
                workspace: snapshot_path.clone(),
                filesystem_roots: vec![snapshot_path.clone()],
                executable: evaluator_executable.clone(),
                args: vec![
                    "__candidate-evaluate".into(),
                    snapshot_path.as_os_str().into(),
                    selector.selector.clone().into(),
                    candidate.manifest.sha256.clone().into(),
                    selector.path.clone().into(),
                    selector.literal.clone().into(),
                ],
                input: Vec::new(),
                support_read: vec![snapshot_path.clone()],
                support_write: Vec::new(),
                max_stdout: 4096,
            },
        )?;
        let result = parse_candidate_result(&output.stdout, &candidate.manifest.sha256)?;
        validate_candidate_result(&result, &candidate, selector)?;
        run_candidate_process(
            &self.db_path,
            CandidateProcessRequest {
                operation: CandidateG6Operation::SnapshotWrite,
                task_id: request.task_id.as_str().to_owned(),
                execution_id: request.execution_id.as_str().to_owned(),
                attempt_id: request.attempt_id.as_str().to_owned(),
                invocation_id: format!("{invocation_id}.snapshot-remove"),
                workspace: self.snapshot_parent.clone(),
                filesystem_roots: vec![self.snapshot_parent.clone()],
                executable: evaluator_executable.clone(),
                args: vec![
                    "__candidate-snapshot-remove".into(),
                    self.snapshot_parent.as_os_str().into(),
                    snapshot_name.into(),
                ],
                input: Vec::new(),
                support_read: vec![self.snapshot_parent.clone()],
                support_write: vec![self.snapshot_parent.clone()],
                max_stdout: 0,
            },
        )?;
        for phase in [
            evaluation::CandidateCheckpointPhase::AfterEvaluation,
            evaluation::CandidateCheckpointPhase::ResultAdmission,
        ] {
            checkpoints.push(self.freshness_checkpoint(CandidateFreshnessRequest {
                request,
                invocation_id: &invocation_id,
                root: &root,
                anchor: &anchor,
                candidate: &candidate,
                format: object_format,
                phase,
                started_at_ms,
            })?);
        }
        let completed_at_ms = now_millis();
        if checkpoints.iter().any(|checkpoint| !checkpoint.matched) {
            return Err("candidate inputs changed during evaluation".to_owned());
        }
        let mut assessment = evaluation::CandidateIntegrityAssessment {
            task_id: request.task_id.as_str().to_owned(),
            execution_id: request.execution_id.as_str().to_owned(),
            attempt_id: request.attempt_id.as_str().to_owned(),
            subject_ref: format!("attempt:{}", request.attempt_id.as_str()),
            requirement_revision: selector.requirement_revision.clone(),
            policy_revision: base.policy.revision(),
            policy_fingerprint: base.policy.fingerprint().to_owned(),
            criterion_id: selector.criterion_id.clone(),
            selector: selector.selector.clone(),
            selector_path: selector.path.clone(),
            selector_literal: selector.literal.clone(),
            invocation_id: invocation_id.clone(),
            evaluator_ref: "runlith.agentd.candidate-evaluator.v1".to_owned(),
            runtime_ref: format!(
                "agentd:{};target={}-{}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            enforcement_ref: output.runner.operation_id,
            invocation_descriptor_version: 1,
            invocation_descriptor_sha256: String::new(),
            evaluator_executable: evaluator_executable_ref,
            toolchain_ref: format!(
                "git-executable={};{git_version}",
                self.git_executable
                    .canonicalize()
                    .map_err(|_| "candidate Git executable identity unavailable".to_owned())?
                    .to_str()
                    .ok_or_else(|| "candidate Git executable identity is unsupported".to_owned())?
            ),
            runner_backend: output.runner.backend,
            runner_plan_sha256: output.runner.plan_sha256,
            runner_enforced_axes: output.runner.enforced_axes,
            runner_env_isolated: output.runner.environment_isolated,
            base_commit_oid: base_candidate.commit_oid,
            base_tree_oid: base_candidate.tree_oid,
            base_manifest_sha256: base_candidate.manifest.sha256.clone(),
            candidate_commit_oid: candidate.commit_oid.clone(),
            candidate_tree_oid: candidate.tree_oid.clone(),
            candidate_manifest_sha256: candidate.manifest.sha256.clone(),
            deletions: evaluation::CandidateDeletion::between(
                &base_candidate.manifest,
                &candidate.manifest,
            ),
            checkpoints,
            outcome: evaluation::CandidateIntegrityOutcome::Intact,
            started_at_ms,
            completed_at_ms,
        };
        assessment.refresh_invocation_descriptor_digest();
        assessment
            .validate()
            .map_err(|error| format!("candidate assessment invalid: {error}"))?;
        self.capture
            .observe_candidate_result(
                request.execution_id.clone(),
                request.attempt_id.clone(),
                result.clone(),
                assessment.clone(),
            )
            .map_err(|error| format!("candidate result capture refused: {error}"))?;
        let capture_store = storage_sqlite::SqliteStore::open(&self.db_path)
            .map_err(|error| format!("open candidate capture store: {error}"))?;
        let capture_git = git_cli::GitCli::with_executable(self.git_executable.clone());
        let mut capture_service = app_services::ObservedEffectService::new(
            capture_store,
            app_services::SystemClock,
            self.capture.clone(),
            capture_git,
        );
        capture_service
            .capture(
                &evaluation::CaptureRequest::TestSuiteResult {
                    subject_ref: assessment.subject_ref.clone(),
                },
                request.execution_id.clone(),
                request.attempt_id.clone(),
            )
            .map_err(|error| {
                format!("persist trusted candidate result before verdict: {error:?}")
            })?;
        drop(capture_service);
        let mut operation_store = storage_sqlite::SqliteStore::open(&self.db_path)
            .map_err(|error| format!("reopen candidate operation store: {error}"))?;
        operation_store
            .update_candidate_operation_status(
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
                "PENDING",
                "RESULT_ADMITTED",
                None,
                completed_at_ms,
            )
            .map_err(|error| format!("persist candidate operation admission: {error}"))?;
        Ok(if result.failed == 0 {
            EvaluationVerdict::Verified
        } else {
            EvaluationVerdict::Rejected
        })
    }

    fn revalidate_candidate_completion(&self, request: &EvaluationRequest) -> Result<(), String> {
        let mut store = storage_sqlite::SqliteStore::open(&self.db_path)
            .map_err(|error| format!("open candidate completion store: {error}"))?;
        let evidence = store
            .get_candidate_evidence_for_completion(
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
            )
            .map_err(|error| format!("load candidate completion evidence: {error}"))?
            .ok_or_else(|| "candidate completion evidence is unavailable".to_owned())?;
        let assessment = evidence.integrity;
        let task = store
            .get_task(&request.task_id)
            .map_err(|error| format!("load candidate completion task: {error}"))?
            .ok_or_else(|| "candidate completion task is unavailable".to_owned())?;
        if task.state() != TaskState::Verifying
            || assessment.task_id != request.task_id.as_str()
            || assessment.execution_id != request.execution_id.as_str()
            || assessment.attempt_id != request.attempt_id.as_str()
        {
            return Err("candidate completion lineage or Task state changed".to_owned());
        }
        let selector = task
            .evidence_requirements()
            .iter()
            .filter(|requirement| requirement.required)
            .map(evaluation::CandidateSelector::from_requirement)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("candidate completion requirement is stale: {error}"))?;
        if selector.len() != 1
            || selector[0].requirement_revision != assessment.requirement_revision
            || selector[0].criterion_id != assessment.criterion_id
            || selector[0].selector != assessment.selector
            || selector[0].path != assessment.selector_path
            || selector[0].literal != assessment.selector_literal
        {
            return Err("candidate completion requirement binding changed".to_owned());
        }
        let base = store
            .get_evaluated_policy_base(request.execution_id.as_str())
            .map_err(|error| format!("load candidate completion policy: {error}"))?
            .ok_or_else(|| "candidate completion policy is unavailable".to_owned())?;
        if base.policy.revision() != assessment.policy_revision
            || base.policy.fingerprint() != assessment.policy_fingerprint
        {
            return Err("candidate completion policy binding changed".to_owned());
        }
        let lease = store
            .get_workspace_for_execution(&request.execution_id)
            .map_err(|error| format!("load candidate completion workspace: {error}"))?
            .ok_or_else(|| "candidate completion workspace is unavailable".to_owned())?;
        let anchor = store
            .get_trusted_anchor(lease.id())
            .map_err(|error| format!("load candidate completion Git anchor: {error}"))?
            .ok_or_else(|| "candidate completion Git anchor is unavailable".to_owned())?;
        let root = PathBuf::from(lease.worktree_ref());
        let pointer = read_authorized_worktree_pointer(
            &self.db_path,
            request,
            &assessment.invocation_id,
            &root,
        )?;
        let linked_gitdir = authenticated_worktree_gitdir(
            &self.db_path,
            request,
            &assessment.invocation_id,
            &root,
            &pointer,
            &anchor,
        )?;
        let format = evaluation::GitObjectFormat::from_oid(&assessment.candidate_commit_oid)
            .map_err(|error| format!("candidate completion object format invalid: {error}"))?;
        let git_context = CandidateGitContext {
            request,
            invocation_id: &assessment.invocation_id,
            workspace: anchor.canonical_commondir(),
            gitdir: &linked_gitdir,
            roots: vec![anchor.canonical_commondir().to_path_buf()],
        };
        let candidate =
            self.verify_commit(&git_context, &assessment.candidate_commit_oid, format)?;
        if candidate.manifest.sha256 != assessment.candidate_manifest_sha256
            || candidate.tree_oid != assessment.candidate_tree_oid
        {
            return Err("candidate completion immutable subject changed".to_owned());
        }
        let checkpoint = self.freshness_checkpoint(CandidateFreshnessRequest {
            request,
            invocation_id: &assessment.invocation_id,
            root: &root,
            anchor: &anchor,
            candidate: &candidate,
            format,
            phase: evaluation::CandidateCheckpointPhase::Completion,
            started_at_ms: assessment.started_at_ms,
        })?;
        if !checkpoint.matched {
            return Err("candidate inputs changed before Task completion".to_owned());
        }
        store
            .append_candidate_checkpoint(
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
                &checkpoint,
            )
            .map_err(|error| format!("persist candidate completion checkpoint: {error}"))?;
        store
            .update_candidate_operation_status(
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
                "RESULT_ADMITTED",
                "COMPLETE",
                None,
                checkpoint.observed_at_ms,
            )
            .or_else(|error| {
                // A process may have crashed after persisting Completion but
                // before advancing the operation row. A same-subject retry
                // revalidates freshness above; permit this one recovery.
                let status = store.candidate_operation_status(
                    request.execution_id.as_str(),
                    request.attempt_id.as_str(),
                )?;
                if status.as_deref() == Some("COMPLETE") {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|error| format!("persist candidate operation completion: {error}"))
    }

    fn git_output(
        &self,
        context: &CandidateGitContext<'_>,
        args: &[&str],
        max_stdout: u64,
    ) -> Result<Vec<u8>, String> {
        self.git_output_with_input(context, args, Vec::new(), max_stdout)
    }

    fn git_output_with_input(
        &self,
        context: &CandidateGitContext<'_>,
        args: &[&str],
        input: Vec<u8>,
        max_stdout: u64,
    ) -> Result<Vec<u8>, String> {
        let request = context.request;
        let invocation_id = context.invocation_id;
        let workspace = context.workspace;
        let gitdir = context.gitdir;
        let roots = context.roots.clone();
        // No `-C <workspace>` argument: the Runner already places the
        // child's CWD at `workspace` (below, `current_dir`/`Command`
        // spawn wiring). Git's own `-C` additionally calls `getcwd()` to
        // remember the pre-`-C` directory before it chdir()s, which walks
        // every ancestor of the sandboxed working directory and fails
        // closed under the Seatbelt profile's narrow, workspace-scoped
        // `subpath` grant (macOS Seatbelt denies reading ancestors of an
        // allowed subpath; it only grants the subpath itself and its
        // descendants). Passing `--git-dir` alone with an
        // already-positioned CWD avoids that redundant internal chdir and
        // its getcwd() call entirely, so the narrow common-dir-only scope
        // ADR-0035's "Scoped G6 operation plans" requires keeps working.
        let mut process_args = vec![
            "--git-dir".into(),
            gitdir.as_os_str().into(),
            "--no-replace-objects".into(),
            "--no-lazy-fetch".into(),
        ];
        process_args.extend(args.iter().map(std::ffi::OsString::from));
        Ok(run_candidate_process(
            &self.db_path,
            CandidateProcessRequest {
                operation: CandidateG6Operation::GitRead,
                task_id: request.task_id.as_str().to_owned(),
                execution_id: request.execution_id.as_str().to_owned(),
                attempt_id: request.attempt_id.as_str().to_owned(),
                invocation_id: format!(
                    "{invocation_id}.git.{}",
                    NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed)
                ),
                workspace: workspace.to_path_buf(),
                filesystem_roots: roots.clone(),
                executable: self.git_executable.clone(),
                args: process_args,
                input,
                support_read: roots.clone(),
                support_write: Vec::new(),
                max_stdout,
            },
        )?
        .stdout)
    }

    fn verify_commit(
        &self,
        context: &CandidateGitContext<'_>,
        commit_oid: &str,
        format: evaluation::GitObjectFormat,
    ) -> Result<evaluation::VerifiedCandidate, String> {
        evaluation::VerifiedCandidate::from_commit_with_batch(commit_oid, format, |oids| {
            let mut input = Vec::new();
            for oid in oids {
                input.extend_from_slice(oid.as_bytes());
                input.push(b'\n');
            }
            let output = self
                .git_output_with_input(
                    context,
                    &["cat-file", "--batch"],
                    input,
                    evaluation::MAX_CANDIDATE_BLOB_BYTES + 64 * 1024 * 1024,
                )
                .map_err(|_| evaluation::CandidateError::MissingObject)?;
            parse_git_batch(&output, oids)
        })
        .map_err(|error| format!("candidate Git object graph rejected: {error}"))
    }

    fn validate_git_object_sources(&self, context: &CandidateGitContext<'_>) -> Result<(), String> {
        let config = self.git_output(
            context,
            &["config", "--no-includes", "--local", "--null", "--list"],
            1024 * 1024,
        )?;
        for entry in config
            .split(|byte| *byte == 0)
            .filter(|entry| !entry.is_empty())
        {
            let lower = entry.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
            if lower.starts_with(b"extensions.partialclone")
                || (lower.starts_with(b"remote.")
                    && lower.windows(9).any(|window| window == b".promisor"))
            {
                return Err("partial-clone/promisor Git configuration is unsupported".to_owned());
            }
        }
        let common = context
            .workspace
            .to_str()
            .ok_or_else(|| "trusted common directory is not UTF-8".to_owned())?
            .to_owned();
        let capability = Capability::FilesystemRead(
            FileScope::new(&common, ".")
                .map_err(|error| format!("object metadata scope: {error}"))?,
        );
        let root_request = PolicyRequest::new(
            format!("{}.object-metadata", context.invocation_id),
            "candidate-object-source-metadata",
            RequestOrigin::new(
                context.request.task_id.as_str(),
                Some(context.request.execution_id.as_str().to_owned()),
                Some(context.request.attempt_id.as_str().to_owned()),
            )
            .map_err(|error| format!("object metadata origin: {error}"))?,
            RequestedCapabilities::new(vec![capability.clone()]),
        )
        .map_err(|error| format!("object metadata request: {error}"))?;
        let policy = CompiledPolicy::compile(vec![PolicyRule::new(
            "builtin.candidate.object-source-metadata",
            "candidate_git_object_source_validation",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            capability.clone(),
        )
        .map_err(|error| format!("object metadata rule: {error}"))?])
        .map_err(|error| format!("object metadata policy: {error}"))?;
        let mut security = G6SecurityService::open(&self.db_path, app_services::SystemClock)
            .map_err(|error| format!("object metadata G6 unavailable: {error}"))?;
        let effective = match security
            .authorize(
                &root_request,
                &EffectiveCapabilities::new(vec![capability]),
                &policy,
                u64::MAX,
            )
            .map_err(|error| format!("object metadata G6 authorization failed: {error}"))?
        {
            G6Authorization::Authorized { effective, .. } => effective,
            G6Authorization::ApprovalRequired { .. } | G6Authorization::Denied { .. } => {
                return Err("G6 denied Git object metadata access".to_owned());
            }
        };
        let mut authorize = |path: &[u8], operation| {
            let path = std::str::from_utf8(path).map_err(|_| {
                evaluation::EffectError::CaptureFailed("Git metadata scope path invalid")
            })?;
            let operation = match operation {
                effect_capture_local::DescriptorReadOperation::OpenDirectory => "open-directory",
                effect_capture_local::DescriptorReadOperation::ReadDirectory => "read-directory",
                effect_capture_local::DescriptorReadOperation::Metadata => "metadata",
                effect_capture_local::DescriptorReadOperation::OpenFile => "open-file",
                effect_capture_local::DescriptorReadOperation::ReadFile => "read-file",
            };
            let scoped =
                Capability::FilesystemRead(FileScope::new(&common, path).map_err(|_| {
                    evaluation::EffectError::CaptureFailed("Git metadata scope invalid")
                })?);
            let request = PolicyRequest::new(
                format!(
                    "{}.object-metadata.{}.{}",
                    context.invocation_id,
                    NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed),
                    operation
                ),
                "candidate-object-source-metadata",
                RequestOrigin::new(
                    context.request.task_id.as_str(),
                    Some(context.request.execution_id.as_str().to_owned()),
                    Some(context.request.attempt_id.as_str().to_owned()),
                )
                .map_err(|_| evaluation::EffectError::CaptureFailed("metadata origin invalid"))?,
                RequestedCapabilities::new(vec![scoped]),
            )
            .map_err(|_| evaluation::EffectError::CaptureFailed("metadata request invalid"))?;
            match security
                .authorize_filesystem_read(&request, &effective, common.clone(), path)
                .map_err(|_| evaluation::EffectError::CaptureFailed("G6 denied metadata access"))?
            {
                G6Authorization::Authorized { .. } => Ok(()),
                G6Authorization::ApprovalRequired { .. } | G6Authorization::Denied { .. } => Err(
                    evaluation::EffectError::CaptureFailed("G6 denied metadata access"),
                ),
            }
        };
        for relative in [
            "objects/info/alternates",
            "objects/info/http-alternates",
            "info/grafts",
            "shallow",
        ] {
            let bytes = effect_capture_local::read_linked_metadata_file_if_present(
                context.workspace,
                relative.as_bytes(),
                &mut authorize,
            )
            .map_err(|error| format!("Git object-source metadata read failed: {error}"))?;
            if bytes.is_some() {
                return Err(format!(
                    "unsupported Git object source metadata: {relative}"
                ));
            }
        }
        Ok(())
    }

    fn freshness_checkpoint(
        &self,
        freshness: CandidateFreshnessRequest<'_>,
    ) -> Result<evaluation::CandidateFreshnessCheckpoint, String> {
        let CandidateFreshnessRequest {
            request,
            invocation_id,
            root,
            anchor,
            candidate,
            format,
            phase,
            started_at_ms,
        } = freshness;
        let pointer_before =
            read_authorized_worktree_pointer(&self.db_path, request, invocation_id, root)?;
        let authenticated_gitdir = authenticated_worktree_gitdir(
            &self.db_path,
            request,
            invocation_id,
            root,
            &pointer_before,
            anchor,
        )?;
        let canonical_root = root
            .canonicalize()
            .map_err(|_| "leased candidate worktree is unavailable".to_owned())?;
        if canonical_root != root {
            return Err("leased candidate worktree path is no longer canonical".to_owned());
        }
        // Plan A only: this reads the linked HEAD/index/common-dir through
        // the read-only Git plumbing plan, scoped to the trusted common Git
        // directory alone (ADR-0035 "Scoped G6 operation plans" — this plan
        // "receives no ... live-source ... authority"). `root` (the leased
        // worktree) is authorized separately below for Plan B's live
        // filesystem scan; folding it into this Git-read plan's
        // `filesystem_roots` would authorize a second, unrelated workspace
        // under the same launch, which the Runner correctly refuses
        // (`WorkspaceScopeMismatch`) since this plan's `workspace` is the
        // common dir, not the worktree.
        let git_context = CandidateGitContext {
            request,
            invocation_id,
            workspace: anchor.canonical_commondir(),
            gitdir: &authenticated_gitdir,
            roots: vec![anchor.canonical_commondir().to_path_buf()],
        };
        let head = self.git_output(&git_context, &["rev-parse", "--verify", "HEAD"], 128)?;
        let index = self.git_output(
            &git_context,
            &["ls-files", "--stage", "-z"],
            16 * 1024 * 1024,
        )?;
        let common = self.git_output(
            &git_context,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            4096,
        )?;
        let common_path = std::str::from_utf8(&common)
            .map_err(|_| "Git common directory is invalid".to_owned())?
            .trim_end_matches('\n');
        let common_path = PathBuf::from(common_path);
        let mut descriptor_authority =
            G6SecurityService::open(&self.db_path, app_services::SystemClock)
                .map_err(|error| format!("G6 descriptor read authority unavailable: {error}"))?;
        let workspace_root = root
            .to_str()
            .ok_or_else(|| "candidate worktree root is not UTF-8".to_owned())?
            .to_owned();
        let root_read = Capability::FilesystemRead(
            FileScope::new(&workspace_root, ".")
                .map_err(|error| format!("candidate descriptor root scope: {error}"))?,
        );
        let root_request = PolicyRequest::new(
            format!("{invocation_id}.git-read.fs-root"),
            "candidate-live-worktree-descriptor-scan",
            RequestOrigin::new(
                request.task_id.as_str(),
                Some(request.execution_id.as_str().to_owned()),
                Some(request.attempt_id.as_str().to_owned()),
            )
            .map_err(|error| format!("candidate descriptor origin: {error}"))?,
            RequestedCapabilities::new(vec![root_read.clone()]),
        )
        .map_err(|error| format!("candidate descriptor request: {error}"))?;
        let root_policy = CompiledPolicy::compile(vec![PolicyRule::new(
            "builtin.candidate.git-read.exact-worktree-root",
            "accepted_candidate_git_read_scope",
            PolicyLayer::BuiltInHard,
            PolicyOutcome::Allow,
            root_read.clone(),
        )
        .map_err(|error| format!("candidate descriptor policy rule: {error}"))?])
        .map_err(|error| format!("candidate descriptor policy: {error}"))?;
        let root_parent = EffectiveCapabilities::new(vec![root_read]);
        let filesystem_authority = match descriptor_authority
            .authorize(&root_request, &root_parent, &root_policy, u64::MAX)
            .map_err(|error| format!("candidate descriptor root authority failed: {error}"))?
        {
            G6Authorization::Authorized { effective, .. } => effective,
            G6Authorization::ApprovalRequired { .. } | G6Authorization::Denied { .. } => {
                return Err("G6 denied candidate worktree root read".to_owned());
            }
        };
        let mut authorize_descriptor = |relative_path: &[u8], operation| {
            let relative_path = std::str::from_utf8(relative_path).map_err(|_| {
                evaluation::EffectError::CaptureFailed(
                    "candidate descriptor scope path is not supported",
                )
            })?;
            let operation_kind = match operation {
                effect_capture_local::DescriptorReadOperation::OpenDirectory => "open-directory",
                effect_capture_local::DescriptorReadOperation::ReadDirectory => "read-directory",
                effect_capture_local::DescriptorReadOperation::Metadata => "metadata",
                effect_capture_local::DescriptorReadOperation::OpenFile => "open-file",
                effect_capture_local::DescriptorReadOperation::ReadFile => "read-file",
            };
            let sequence = NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed);
            let capability = Capability::FilesystemRead(
                FileScope::new(&workspace_root, relative_path).map_err(|_| {
                    evaluation::EffectError::CaptureFailed("candidate descriptor scope is invalid")
                })?,
            );
            let descriptor_request = PolicyRequest::new(
                format!("{invocation_id}.git-read.fs.{sequence}.{operation_kind}"),
                "candidate-live-worktree-descriptor-scan",
                RequestOrigin::new(
                    request.task_id.as_str(),
                    Some(request.execution_id.as_str().to_owned()),
                    Some(request.attempt_id.as_str().to_owned()),
                )
                .map_err(|_| {
                    evaluation::EffectError::CaptureFailed(
                        "candidate descriptor request origin is invalid",
                    )
                })?,
                RequestedCapabilities::new(vec![capability]),
            )
            .map_err(|_| {
                evaluation::EffectError::CaptureFailed("candidate descriptor request is invalid")
            })?;
            let authorization = descriptor_authority
                .authorize_filesystem_read(
                    &descriptor_request,
                    &filesystem_authority,
                    workspace_root.clone(),
                    relative_path,
                )
                .map_err(|_| {
                    evaluation::EffectError::CaptureFailed(
                        "G6 denied candidate descriptor filesystem read",
                    )
                })?;
            match authorization {
                G6Authorization::Authorized { .. } => Ok(()),
                G6Authorization::ApprovalRequired { .. } | G6Authorization::Denied { .. } => {
                    Err(evaluation::EffectError::CaptureFailed(
                        "G6 denied candidate descriptor filesystem read",
                    ))
                }
            }
        };
        let pointer_before_scan =
            read_authorized_worktree_pointer(&self.db_path, request, invocation_id, root)?;
        let authenticated_before_scan = authenticated_worktree_gitdir(
            &self.db_path,
            request,
            invocation_id,
            root,
            &pointer_before_scan,
            anchor,
        )?;
        let scan = effect_capture_local::scan_live_worktree(root, &mut authorize_descriptor)
            .map_err(|error| format!("live worktree scan failed: {error}"))?;
        let pointer_after =
            read_authorized_worktree_pointer(&self.db_path, request, invocation_id, root)?;
        let authenticated_after = authenticated_worktree_gitdir(
            &self.db_path,
            request,
            invocation_id,
            root,
            &pointer_after,
            anchor,
        )?;
        let live_manifest = evaluation::CandidateManifest::from_files(&scan.files)
            .map_err(|error| error.to_string())?;
        let dirs_match = scan.directories == manifest_directories(&candidate.manifest.entries);
        let head_matches =
            std::str::from_utf8(&head).is_ok_and(|value| value.trim_end() == candidate.commit_oid);
        let index_matches = index_matches_candidate(&index, candidate);
        let pointer_matches = pointer_before_scan == pointer_before
            && scan.git_pointer.as_deref() == Some(pointer_before_scan.as_slice())
            && pointer_after == pointer_before_scan
            && authenticated_before_scan == authenticated_gitdir
            && authenticated_after == authenticated_gitdir;
        let metadata = [
            head.as_slice(),
            index.as_slice(),
            common.as_slice(),
            pointer_before.as_slice(),
            pointer_before_scan.as_slice(),
            pointer_after.as_slice(),
            authenticated_gitdir.as_os_str().as_encoded_bytes(),
        ]
        .concat();
        let git_metadata_fingerprint = evaluation::candidate_sha256(&metadata);
        let matched = format
            == evaluation::GitObjectFormat::from_oid(&candidate.commit_oid)
                .map_err(|error| error.to_string())?
            && common_path == anchor.canonical_commondir()
            && head_matches
            && index_matches
            && dirs_match
            && pointer_matches
            && live_manifest.sha256 == candidate.manifest.sha256;
        let observed_at_ms = now_millis().max(started_at_ms);
        Ok(evaluation::CandidateFreshnessCheckpoint {
            phase,
            observed_at_ms,
            git_metadata_fingerprint,
            live_worktree_manifest_sha256: live_manifest.sha256,
            matched,
        })
    }
}

fn candidate_operation_label(
    operation: CandidateG6Operation,
    args: &[std::ffi::OsString],
) -> &'static str {
    match operation {
        CandidateG6Operation::GitRead => "git-read",
        CandidateG6Operation::SnapshotWrite => {
            // Check if this is a snapshot-remove by looking at the first argument
            if args
                .first()
                .and_then(|arg| arg.to_str())
                .is_some_and(|s| s == "__candidate-snapshot-remove")
            {
                "snapshot-remove"
            } else {
                "snapshot-write"
            }
        }
        CandidateG6Operation::EvaluatorReadOnly => "evaluator",
    }
}

fn run_candidate_process(
    db_path: &Path,
    request: CandidateProcessRequest,
) -> Result<CandidateProcessOutput, String> {
    const MAX_STDERR: u64 = 64 * 1024;
    const TIMEOUT: Duration = Duration::from_secs(30);
    let prepared = prepare_candidate_process(db_path, &request)?;
    let operation_id = prepared.operation.operation_id.clone();
    let program = request.executable.as_os_str().to_owned();
    let workspace = request.workspace.clone();
    let args = request.args;
    let input = request.input;
    let max_stdout = request.max_stdout;
    let operation = request.operation;
    let op_label = candidate_operation_label(operation, &args);
    let output = prepared.invoke(|launch| {
        let backend = launch.backend();
        let axes = launch.enforced_axes();
        let runner = CandidateRunnerReceipt {
            operation_id,
            // Must equal exactly what `enforce_process_operation`
            // (crates/app-services/src/security_service.rs) records as the
            // durable `IsolationEnforcement` audit record's
            // `subject.id` (`backend_identity.name` alone — never a
            // compound string), since `get_candidate_evidence_for_completion`
            // requires an exact match between `runner_backend` and that
            // audit subject to prove the enforcement record binds this
            // exact invocation before completion can proceed.
            backend: backend.name.to_owned(),
            plan_sha256: candidate_runner_plan_digest(launch.plan()),
            enforced_axes: u8::from(axes.requires(runner::IsolationAxis::Filesystem))
                | (u8::from(axes.requires(runner::IsolationAxis::Network)) << 1)
                | (u8::from(axes.requires(runner::IsolationAxis::Process)) << 2),
            environment_isolated: launch.environment_isolated(),
        };
        let command = launch
            .bind(runner::CommandSpec::new(program, args))
            .map_err(|error| format!("Runner refused candidate command: {error}"))?;
        let mut child = command
            .spawn(|wire| {
                wire.current_dir(&workspace)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
            })
            .map_err(|_| "candidate process spawn failed".to_owned())?;
        let mut child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| "candidate process stdin unavailable".to_owned())?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| "candidate process stdout unavailable".to_owned())?;
        let child_stderr = child
            .stderr
            .take()
            .ok_or_else(|| "candidate process stderr unavailable".to_owned())?;
        let stdin_writer = std::thread::spawn(move || {
            use std::io::Write;
            child_stdin.write_all(&input)
        });
        let stdout_reader = std::thread::spawn(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            child_stdout.take(max_stdout + 1).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let stderr_reader = std::thread::spawn(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            child_stderr.take(MAX_STDERR + 1).read_to_end(&mut bytes)?;
            Ok::<_, std::io::Error>(bytes)
        });
        let deadline = std::time::Instant::now() + TIMEOUT;
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| "candidate process wait failed".to_owned())?
            {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("candidate process timed out".to_owned());
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        stdin_writer
            .join()
            .map_err(|_| "candidate process stdin task failed".to_owned())?
            .map_err(|_| "candidate process input write failed".to_owned())?;
        let stdout = stdout_reader
            .join()
            .map_err(|_| "candidate process stdout task failed".to_owned())?
            .map_err(|_| "candidate process stdout read failed".to_owned())?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| "candidate process stderr task failed".to_owned())?
            .map_err(|_| "candidate process stderr read failed".to_owned())?;
        if !status.success() || stdout.len() as u64 > max_stdout || stderr.len() as u64 > MAX_STDERR
        {
            let exit_info = if let Some(code) = status.code() {
                format!("exit: {}", code)
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    if let Some(signal) = status.signal() {
                        format!("signal {}", signal)
                    } else {
                        "unknown".to_owned()
                    }
                }
                #[cfg(not(unix))]
                {
                    "unknown".to_owned()
                }
            };
            let stdout_bytes = stdout.len() as u64;
            let stderr_bytes = stderr.len() as u64;
            return Err(format!(
                "candidate {op_label} process returned unsupported output or status ({exit_info}, stdout_bytes: {stdout_bytes}/{max_stdout}, stderr_bytes: {stderr_bytes}/{MAX_STDERR})"
            ));
        }
        Ok(CandidateProcessOutput { stdout, runner })
    })?;
    output
}

fn candidate_runner_plan_digest(plan: &runner::IsolationPlan) -> String {
    fn add(bytes: &mut Vec<u8>, value: &[u8]) {
        bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
        bytes.extend_from_slice(value);
    }
    let mut canonical = b"runlith:candidate-runner-plan:v1\0".to_vec();
    add(&mut canonical, plan.workspace().as_bytes());
    add(&mut canonical, plan.launch_executable().as_str().as_bytes());
    for root in plan.filesystem_read_roots() {
        add(&mut canonical, root.as_os_str().as_encoded_bytes());
    }
    canonical.push(0xff);
    for root in plan.filesystem_write_roots() {
        add(&mut canonical, root.as_os_str().as_encoded_bytes());
    }
    canonical.push(0xfe);
    for executable in plan.process_executables() {
        add(&mut canonical, executable.as_str().as_bytes());
    }
    canonical.push(0xfd);
    for destination in plan.network_destinations() {
        add(&mut canonical, destination.as_str().as_bytes());
    }
    let required = plan.requirements().axes();
    canonical.extend_from_slice(&[
        u8::from(required.requires(runner::IsolationAxis::Filesystem)),
        u8::from(required.requires(runner::IsolationAxis::Network)),
        u8::from(required.requires(runner::IsolationAxis::Process)),
    ]);
    for (name, value) in plan.environment().vars() {
        add(&mut canonical, name.as_encoded_bytes());
        add(&mut canonical, value.as_encoded_bytes());
    }
    evaluation::candidate_sha256(&canonical)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn encode_snapshot_files(files: &[evaluation::CandidateFile]) -> Result<Vec<u8>, String> {
    let mut bytes = b"runlith-candidate-snapshot/v1\n".to_vec();
    bytes.extend_from_slice(
        &u32::try_from(files.len())
            .map_err(|_| "candidate snapshot entry count exceeded".to_owned())?
            .to_be_bytes(),
    );
    for file in files {
        bytes.extend_from_slice(
            &u32::try_from(file.path.len())
                .map_err(|_| "candidate path exceeded bound".to_owned())?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&file.mode.to_be_bytes());
        bytes.extend_from_slice(
            &u64::try_from(file.bytes.len())
                .map_err(|_| "candidate blob exceeded bound".to_owned())?
                .to_be_bytes(),
        );
        bytes.extend_from_slice(&file.path);
        bytes.extend_from_slice(&file.bytes);
    }
    Ok(bytes)
}

fn parse_candidate_result(
    bytes: &[u8],
    expected_manifest: &str,
) -> Result<evaluation::CandidateTestResult, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "candidate evaluator output is not UTF-8".to_owned())?;
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() != 7
        || lines[0] != "candidate-test-result/v1"
        || lines[1] != "selector=candidate.file-contains.v1"
        || lines[2] != format!("manifest={expected_manifest}")
        || !lines[6].starts_with("result=sha256:")
    {
        return Err("candidate evaluator output is malformed or unbound".to_owned());
    }
    let parse = |line: &str, key: &str| -> Result<u32, String> {
        line.strip_prefix(key)
            .ok_or_else(|| "candidate evaluator output field mismatch".to_owned())?
            .parse()
            .map_err(|_| "candidate evaluator count invalid".to_owned())
    };
    let result_sha256 = lines[6]["result=".len()..].to_owned();
    if result_sha256.strip_prefix("sha256:").is_none_or(|hex| {
        hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }) {
        return Err("candidate evaluator result digest invalid".to_owned());
    }
    let result = evaluation::CandidateTestResult {
        selector: "candidate.file-contains.v1".to_owned(),
        passed: parse(lines[3], "passed=")?,
        failed: parse(lines[4], "failed=")?,
        skipped: parse(lines[5], "skipped=")?,
        result_sha256,
    };
    if result
        .passed
        .checked_add(result.failed)
        .and_then(|total| total.checked_add(result.skipped))
        != Some(1)
    {
        return Err("candidate evaluator result counts are inconsistent".to_owned());
    }
    Ok(result)
}

fn validate_candidate_result(
    result: &evaluation::CandidateTestResult,
    candidate: &evaluation::VerifiedCandidate,
    requirement: &evaluation::CandidateSelector,
) -> Result<(), String> {
    let expected = evaluation::CandidateTestResult::file_contains(
        &candidate.files,
        &requirement.path,
        &requirement.literal,
    )
    .map_err(|error| format!("candidate result could not be recomputed: {error}"))?;
    if result != &expected {
        return Err("candidate evaluator result differs from selected immutable bytes".to_owned());
    }
    Ok(())
}

fn parse_git_batch(
    bytes: &[u8],
    requested: &[String],
) -> Result<Vec<(String, String, Vec<u8>)>, evaluation::CandidateError> {
    let mut cursor = 0usize;
    let mut objects = Vec::with_capacity(requested.len());
    for expected_oid in requested {
        let header_end = bytes[cursor..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| cursor + index)
            .ok_or(evaluation::CandidateError::MissingObject)?;
        let header = std::str::from_utf8(&bytes[cursor..header_end])
            .map_err(|_| evaluation::CandidateError::UnsupportedObject)?;
        cursor = header_end + 1;
        let mut fields = header.split(' ');
        let oid = fields
            .next()
            .ok_or(evaluation::CandidateError::MissingObject)?;
        let kind = fields
            .next()
            .ok_or(evaluation::CandidateError::MissingObject)?;
        let size = fields
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(evaluation::CandidateError::MissingObject)?;
        if fields.next().is_some() || oid != expected_oid {
            return Err(evaluation::CandidateError::MissingObject);
        }
        let end = cursor
            .checked_add(size)
            .ok_or(evaluation::CandidateError::LimitExceeded)?;
        let body = bytes
            .get(cursor..end)
            .ok_or(evaluation::CandidateError::MissingObject)?
            .to_vec();
        if bytes.get(end) != Some(&b'\n') {
            return Err(evaluation::CandidateError::MissingObject);
        }
        cursor = end + 1;
        objects.push((oid.to_owned(), kind.to_owned(), body));
    }
    if cursor != bytes.len() {
        return Err(evaluation::CandidateError::UnsupportedObject);
    }
    Ok(objects)
}

fn index_matches_candidate(bytes: &[u8], candidate: &evaluation::VerifiedCandidate) -> bool {
    let mut entries = Vec::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return false;
        };
        let Ok(header) = std::str::from_utf8(&record[..tab]) else {
            return false;
        };
        let mut fields = header.split(' ');
        let (Some(mode), Some(oid), Some(stage), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        if stage != "0" {
            return false;
        }
        let Ok(mode) = u32::from_str_radix(mode, 8) else {
            return false;
        };
        entries.push((record[tab + 1..].to_vec(), mode, oid.to_owned()));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries.len() == candidate.files.len()
        && entries.iter().zip(&candidate.files).all(|(entry, file)| {
            entry.0 == file.path
                && entry.1 == file.mode
                && file.git_oid.as_deref() == Some(entry.2.as_str())
        })
}

fn manifest_directories(entries: &[evaluation::CandidateManifestEntry]) -> Vec<Vec<u8>> {
    let mut directories = std::collections::BTreeSet::new();
    for entry in entries {
        let mut path = entry.path.as_slice();
        while let Some(index) = path.iter().rposition(|byte| *byte == b'/') {
            directories.insert(path[..index].to_vec());
            path = &path[..index];
        }
    }
    directories.into_iter().collect()
}

fn authenticated_worktree_gitdir(
    db_path: &Path,
    request: &EvaluationRequest,
    invocation_id: &str,
    root: &Path,
    pointer: &[u8],
    anchor: &workspace::TrustedCommonGit,
) -> Result<PathBuf, String> {
    let text = std::str::from_utf8(pointer)
        .map_err(|_| "linked worktree pointer is not UTF-8".to_owned())?;
    let raw_gitdir = text
        .strip_prefix("gitdir: ")
        .and_then(|text| text.strip_suffix('\n'))
        .ok_or_else(|| "linked worktree pointer framing is invalid".to_owned())?;
    if raw_gitdir.contains('\n') || raw_gitdir.contains('\0') {
        return Err("linked worktree pointer contains unsupported bytes".to_owned());
    }
    let gitdir = lexical_absolute(root, Path::new(raw_gitdir))?;
    let worktrees_root = anchor.canonical_commondir().join("worktrees");
    let relative = gitdir
        .strip_prefix(&worktrees_root)
        .map_err(|_| "linked Git metadata escapes the trusted common directory".to_owned())?;
    if relative.as_os_str().is_empty() {
        return Err("linked Git metadata directory is ambiguous".to_owned());
    }
    let relative_bytes = relative
        .to_str()
        .ok_or_else(|| "linked Git metadata path is not UTF-8".to_owned())?
        .replace(std::path::MAIN_SEPARATOR, "/");
    let common = anchor
        .canonical_commondir()
        .to_str()
        .ok_or_else(|| "trusted common directory is not UTF-8".to_owned())?
        .to_owned();
    let capability = Capability::FilesystemRead(
        FileScope::new(&common, ".").map_err(|error| format!("linked metadata scope: {error}"))?,
    );
    let policy_request = PolicyRequest::new(
        format!(
            "{invocation_id}.linked-metadata:{}",
            NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed)
        ),
        "candidate-linked-worktree-metadata",
        RequestOrigin::new(
            request.task_id.as_str(),
            Some(request.execution_id.as_str().to_owned()),
            Some(request.attempt_id.as_str().to_owned()),
        )
        .map_err(|error| format!("linked metadata origin: {error}"))?,
        RequestedCapabilities::new(vec![capability.clone()]),
    )
    .map_err(|error| format!("linked metadata request: {error}"))?;
    let policy = CompiledPolicy::compile(vec![PolicyRule::new(
        "builtin.candidate.linked-metadata",
        "authenticated_linked_worktree_metadata",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        capability.clone(),
    )
    .map_err(|error| format!("linked metadata rule: {error}"))?])
    .map_err(|error| format!("linked metadata policy: {error}"))?;
    let mut security = G6SecurityService::open(db_path, app_services::SystemClock)
        .map_err(|error| format!("linked metadata G6 unavailable: {error}"))?;
    let parent = EffectiveCapabilities::new(vec![capability]);
    let effective = match security
        .authorize(&policy_request, &parent, &policy, u64::MAX)
        .map_err(|error| format!("linked metadata G6 authorization failed: {error}"))?
    {
        G6Authorization::Authorized { effective, .. } => effective,
        G6Authorization::ApprovalRequired { .. } | G6Authorization::Denied { .. } => {
            return Err("G6 denied linked metadata access".to_owned());
        }
    };
    let mut authorize = |path: &[u8], operation| {
        let path = std::str::from_utf8(path)
            .map_err(|_| evaluation::EffectError::CaptureFailed("linked metadata path invalid"))?;
        let operation_name = match operation {
            effect_capture_local::DescriptorReadOperation::OpenDirectory => "open-directory",
            effect_capture_local::DescriptorReadOperation::ReadDirectory => "read-directory",
            effect_capture_local::DescriptorReadOperation::Metadata => "metadata",
            effect_capture_local::DescriptorReadOperation::OpenFile => "open-file",
            effect_capture_local::DescriptorReadOperation::ReadFile => "read-file",
        };
        let capability =
            Capability::FilesystemRead(FileScope::new(&common, path).map_err(|_| {
                evaluation::EffectError::CaptureFailed("linked metadata scope invalid")
            })?);
        let request = PolicyRequest::new(
            format!(
                "{invocation_id}.linked-metadata.{}.{}",
                NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed),
                operation_name
            ),
            "candidate-linked-worktree-metadata",
            RequestOrigin::new(
                request.task_id.as_str(),
                Some(request.execution_id.as_str().to_owned()),
                Some(request.attempt_id.as_str().to_owned()),
            )
            .map_err(|_| {
                evaluation::EffectError::CaptureFailed("linked metadata origin invalid")
            })?,
            RequestedCapabilities::new(vec![capability]),
        )
        .map_err(|_| evaluation::EffectError::CaptureFailed("linked metadata request invalid"))?;
        security
            .authorize_filesystem_read(&request, &effective, common.clone(), path)
            .map_err(|_| {
                evaluation::EffectError::CaptureFailed("G6 denied linked metadata operation")
            })?;
        Ok(())
    };
    let gitdir_file = effect_capture_local::read_linked_metadata_file(
        anchor.canonical_commondir(),
        format!("worktrees/{relative_bytes}/gitdir").as_bytes(),
        &mut authorize,
    )
    .map_err(|error| format!("linked worktree back-reference read failed: {error}"))?;
    let commondir_file = effect_capture_local::read_linked_metadata_file(
        anchor.canonical_commondir(),
        format!("worktrees/{relative_bytes}/commondir").as_bytes(),
        &mut authorize,
    )
    .map_err(|error| format!("linked worktree common-dir read failed: {error}"))?;
    let backref = std::str::from_utf8(&gitdir_file)
        .map_err(|_| "linked worktree back-reference is invalid".to_owned())?
        .strip_suffix('\n')
        .ok_or_else(|| "linked worktree back-reference framing is invalid".to_owned())?;
    let common_ref = std::str::from_utf8(&commondir_file)
        .map_err(|_| "linked worktree common-dir reference is invalid".to_owned())?
        .strip_suffix('\n')
        .ok_or_else(|| "linked worktree common-dir framing is invalid".to_owned())?;
    if lexical_absolute(&gitdir, Path::new(backref))? != root.join(".git")
        || lexical_absolute(&gitdir, Path::new(common_ref))? != anchor.canonical_commondir()
    {
        return Err("linked worktree metadata does not authenticate the root pointer".to_owned());
    }
    Ok(gitdir)
}

fn read_authorized_worktree_pointer(
    db_path: &Path,
    request: &EvaluationRequest,
    invocation_id: &str,
    root: &Path,
) -> Result<Vec<u8>, String> {
    let workspace = root
        .to_str()
        .ok_or_else(|| "candidate worktree root is not UTF-8".to_owned())?
        .to_owned();
    let capability = Capability::FilesystemRead(
        FileScope::new(&workspace, ".")
            .map_err(|error| format!("candidate pointer root scope: {error}"))?,
    );
    let policy_request = PolicyRequest::new(
        format!(
            "{invocation_id}.pointer-root:{}",
            NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed)
        ),
        "candidate-live-worktree-pointer",
        RequestOrigin::new(
            request.task_id.as_str(),
            Some(request.execution_id.as_str().to_owned()),
            Some(request.attempt_id.as_str().to_owned()),
        )
        .map_err(|error| format!("candidate pointer origin: {error}"))?,
        RequestedCapabilities::new(vec![capability.clone()]),
    )
    .map_err(|error| format!("candidate pointer request: {error}"))?;
    let policy = CompiledPolicy::compile(vec![PolicyRule::new(
        "builtin.candidate.worktree-pointer",
        "accepted_candidate_git_read_scope",
        PolicyLayer::BuiltInHard,
        PolicyOutcome::Allow,
        capability.clone(),
    )
    .map_err(|error| format!("candidate pointer rule: {error}"))?])
    .map_err(|error| format!("candidate pointer policy: {error}"))?;
    let mut security = G6SecurityService::open(db_path, app_services::SystemClock)
        .map_err(|error| format!("candidate pointer G6 unavailable: {error}"))?;
    let parent = EffectiveCapabilities::new(vec![capability]);
    let effective = match security
        .authorize(&policy_request, &parent, &policy, u64::MAX)
        .map_err(|error| format!("candidate pointer G6 authorization failed: {error}"))?
    {
        G6Authorization::Authorized { effective, .. } => effective,
        G6Authorization::ApprovalRequired { .. } | G6Authorization::Denied { .. } => {
            return Err("G6 denied candidate worktree pointer read".to_owned());
        }
    };
    let mut authorize = |path: &[u8], operation| {
        let path = std::str::from_utf8(path)
            .map_err(|_| evaluation::EffectError::CaptureFailed("pointer path invalid"))?;
        let capability = Capability::FilesystemRead(
            FileScope::new(&workspace, path)
                .map_err(|_| evaluation::EffectError::CaptureFailed("pointer scope invalid"))?,
        );
        let request = PolicyRequest::new(
            format!(
                "{invocation_id}.pointer.{}",
                NEXT_CANDIDATE_OPERATION.fetch_add(1, Ordering::Relaxed)
            ),
            "candidate-live-worktree-pointer",
            RequestOrigin::new(
                request.task_id.as_str(),
                Some(request.execution_id.as_str().to_owned()),
                Some(request.attempt_id.as_str().to_owned()),
            )
            .map_err(|_| evaluation::EffectError::CaptureFailed("pointer origin invalid"))?,
            RequestedCapabilities::new(vec![capability]),
        )
        .map_err(|_| evaluation::EffectError::CaptureFailed("pointer request invalid"))?;
        let _operation = operation;
        security
            .authorize_filesystem_read(&request, &effective, workspace.clone(), path)
            .map_err(|_| {
                evaluation::EffectError::CaptureFailed("G6 denied worktree pointer operation")
            })?;
        Ok(())
    };
    effect_capture_local::read_worktree_git_pointer(root, &mut authorize)
        .map_err(|error| format!("authorized worktree pointer read failed: {error}"))
}

fn lexical_absolute(base: &Path, path: &Path) -> Result<PathBuf, String> {
    let source = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in source.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err("linked metadata path escapes filesystem root".to_owned());
                }
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    if !normalized.is_absolute() {
        return Err("linked metadata path is not absolute".to_owned());
    }
    Ok(normalized)
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
            // its job and awaits the operator. Detail is still logged
            // because a failing automated evaluator is otherwise
            // indistinguishable from the no-verifier G5 posture.
            if attempt_concluded_succeeded(&composition.db_path, &execution_id, &attempt_id) {
                eprintln!(
                    "agentd: execution {execution_id} concluded: SUCCEEDED_AWAITING_OPERATOR_VERIFICATION (verification not completed: {})",
                    truncate_detail(&detail)
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
    let effect_git = git.clone();
    let mut workspace =
        WorkspaceService::open(&composition.db_path, git, root, app_services::SystemClock)
            .map_err(|error| format!("open workspace service: {error}"))?;
    let tracker = composition.tracker.clone();
    let executable = composition.resolved_provider_executable()?;
    let db_path = composition.db_path.clone();
    let security_work = work.clone();
    let capture_work = work.clone();
    let effect_capture = effect_capture_local::LocalEffectCapture::default();
    let process_exit_observer = effect_capture.clone();
    let candidate_capture = effect_capture.clone();
    let cancel = cancel.clone();
    let environment_base = composition.provider_environment_base()?;
    // R4 (owner review 2026-09-25): decide evaluator selection first,
    // through the `OrchestrationService`'s own already-open store handle
    // rather than opening a second `SqliteStore` connection just for this
    // check — `SqliteStore::open` runs `verify_foreign_keys` and `migrate`
    // on every call, real per-connection overhead this hot path paid on
    // every single run regardless of whether it was candidate-input, which
    // widened the window between a concluding attempt's DB commit and this
    // cycle's live-owner release in `run_background`. The
    // candidate-snapshot-authority filesystem setup below (create/verify/
    // secure/canonicalize the snapshot directory) is real but comparatively
    // cheap OS work; it is still skipped entirely for the common
    // non-candidate case by gating it on the same check.
    let supports_candidate_input = orchestration
        .tasks()
        .get_task(&work.task_id)
        .ok()
        .flatten()
        .is_some_and(|task| CandidateInputEvaluator::supports_task(&task));
    let selected_evaluator = if supports_candidate_input {
        let snapshot_parent = db_path
            .parent()
            .ok_or_else(|| "candidate snapshot parent unavailable".to_owned())?
            .join("candidate-input-snapshots");
        std::fs::create_dir_all(&snapshot_parent)
            .map_err(|_| "candidate snapshot authority root unavailable".to_owned())?;
        let snapshot_metadata = std::fs::symlink_metadata(&snapshot_parent)
            .map_err(|_| "candidate snapshot authority root unavailable".to_owned())?;
        if snapshot_metadata.file_type().is_symlink() || !snapshot_metadata.is_dir() {
            return Err("candidate snapshot authority root is not a real directory".to_owned());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if snapshot_metadata.permissions().mode() & 0o777 != 0o700 {
                std::fs::set_permissions(&snapshot_parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| {
                        "candidate snapshot authority root cannot be secured".to_owned()
                    })?;
            }
        }
        let snapshot_parent = snapshot_parent
            .canonicalize()
            .map_err(|_| "candidate snapshot authority root cannot be resolved".to_owned())?;
        ProductionEvaluator::Candidate(CandidateInputEvaluator {
            db_path: db_path.clone(),
            git_executable: composition
                .candidate_git_executable
                .as_ref()
                .map_err(Clone::clone)?
                .clone(),
            snapshot_parent,
            capture: candidate_capture,
            evaluator_executable_override: composition
                .candidate_evaluator_executable_override
                .clone(),
        })
    } else {
        ProductionEvaluator::Pending(PendingEvaluator)
    };
    let orchestration_result = orchestration
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
                .with_cancel_flag(cancel)
                .with_process_exit_observer(Arc::new(process_exit_observer));
                secure_provider_executor(
                    &db_path,
                    task,
                    lease,
                    &security_work,
                    executor,
                    &launch_executable,
                )
            },
            &selected_evaluator,
        )
        .map_err(|error| format!("orchestration cycle: {error}"));

    persist_process_exit_before_return(
        orchestration_result,
        &composition.db_path,
        effect_capture,
        effect_git,
        capture_work,
    )
}

/// Persists a governed child exit before returning the rest of the cycle.
/// A later verifier/storage error does not erase runtime evidence already
/// observed from the supervised provider process.
fn persist_process_exit_before_return(
    orchestration_result: Result<OrchestrationOutcome, String>,
    db_path: &Path,
    effect_capture: effect_capture_local::LocalEffectCapture,
    effect_git: git_cli::GitCli,
    work: scheduler::WorkItem,
) -> Result<OrchestrationOutcome, String> {
    if effect_capture.has_process_exit(&work.execution_id, &work.attempt_id) {
        let store = storage_sqlite::SqliteStore::open(db_path)
            .map_err(|error| format!("open observed-effect store: {error}"))?;
        let mut effects = app_services::ObservedEffectService::new(
            store,
            app_services::SystemClock,
            effect_capture,
            effect_git,
        );
        effects
            .capture(
                &evaluation::CaptureRequest::ProcessExit {
                    subject_ref: format!("attempt:{}", work.attempt_id.as_str()),
                },
                work.execution_id,
                work.attempt_id,
            )
            .map_err(|error| format!("persist observed process exit: {error}"))?;
    }
    orchestration_result
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
        OrchestrationOutcome::BlockedByPrerequisite { .. } => "BLOCKED_BY_PREREQUISITE",
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

    /// Like [`git`], but returns trimmed stdout for independent test-side
    /// verification of what the real evaluator recorded (e.g. exact
    /// commit/tree OIDs), instead of trusting the evaluator's own output.
    fn git_stdout(repo: &Path, args: &[&str]) -> String {
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
        String::from_utf8(output.stdout)
            .expect("fixture git stdout is UTF-8")
            .trim()
            .to_owned()
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
            baseline_risk: plan_domain::RiskClass::Low,
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
    fn cv_2_process_exit_is_persisted_before_later_cycle_error_returns() {
        let sandbox = Sandbox::named("observed-before-cycle-error");
        seed_ready(&sandbox.db, "P-1", "T-1");
        let fixture = fixture_script(&sandbox.dir, "unused.sh", "exit 0\n");
        let composition = sandbox.composition(Some(fixture));
        let (response, job) = run_via_dispatch(
            &composition,
            custom_run("T-1", "E-OBSERVED", "A-OBSERVED", &sandbox.repo),
        );
        assert_eq!(run_payload(&response).execution_id, "E-OBSERVED");
        assert!(job.is_some());

        let execution_id = ExecutionId::new("E-OBSERVED").unwrap();
        let attempt_id = agent_runtime::AttemptId::new("A-OBSERVED").unwrap();
        let mut runtime =
            app_services::RuntimeService::open(&sandbox.db, app_services::SystemClock)
                .expect("open runtime service");
        runtime
            .create_attempt(
                &execution_id,
                attempt_id.clone(),
                agent_runtime::ProviderRef::new("test-provider").unwrap(),
            )
            .expect("create attempt");

        let capture = effect_capture_local::LocalEffectCapture::default();
        let work = scheduler::WorkItem {
            task_id: TaskId::new("T-1").unwrap(),
            execution_id: execution_id.clone(),
            attempt_id: attempt_id.clone(),
        };
        let provider = fixture_script(
            &sandbox.dir,
            "observed-provider.sh",
            r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"observed","session_id":"dddddddd-dddd-dddd-dddd-dddddddddddd"}'"#,
        );
        let executor = provider_claude::ClaudeExecutor::bind_trusted_host(
            provider_claude::ClaudeConfig::default()
                .with_executable(provider)
                .unwrap()
                .with_timeout(Duration::from_secs(5))
                .unwrap(),
            "capture a real local process exit",
            sandbox.repo.clone(),
            sandbox.dir.join("provider-env-observed"),
        )
        .unwrap()
        .with_process_exit_observer(Arc::new(capture.clone()));
        scheduler::ExecutionPort::execute(&executor, &work).expect("provider fixture succeeds");

        let error = persist_process_exit_before_return(
            Err("no verifier verdict".to_owned()),
            &sandbox.db,
            capture,
            git_cli::GitCli::new(),
            work,
        )
        .expect_err("later cycle error must still be returned");
        assert_eq!(error, "no verifier verdict");

        // The helper's store is already closed here. Re-opening proves the
        // effect is durable rather than merely retained by the observer.
        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let effect = store
            .get_observed_effect(&evaluation::EffectId::new("effect-1").unwrap())
            .expect("read observed effect")
            .expect("process exit persisted before returning the cycle error");
        assert_eq!(effect.effect_kind(), evaluation::EffectKind::ProcessExit);
        assert_eq!(effect.provenance().execution_id, execution_id);
        assert_eq!(effect.provenance().attempt_id, attempt_id);
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

    /// Drift guard for the pairing documented on `production_platform`. The
    /// verdict is checked against what the production backend actually
    /// reports, not against the shape of the `cfg`, so a platform that gains
    /// mandatory confinement without choosing a verdict fails here even when
    /// it reuses an existing `cfg` arm or changes only the backend adapter.
    #[test]
    fn production_confinement_and_provider_auth_verdict_stay_paired() {
        let backend =
            production_runner_backend(Vec::new(), Vec::new(), RunnerLaunchPurpose::Provider)
                .expect("production backend");
        let plan = runner::IsolationPlan::compile(
            &EffectiveCapabilities::new(vec![Capability::process("/usr/bin/true").unwrap()]),
            "/tmp",
            ScopeId::new("/usr/bin/true").unwrap(),
            IsolationRequirements::new(true, true, true),
            ChildEnvironment::from_explicit([(
                std::ffi::OsString::from("PATH"),
                std::ffi::OsString::from("/usr/bin:/bin"),
            )])
            .expect("child environment"),
        )
        .expect("isolation plan");
        let enforces_confinement = !backend
            .enforce(&plan)
            .expect("enforcement report")
            .enforced()
            .is_empty();

        match production_platform::AUTH_CONFINEMENT_VERDICT {
            ConfinementAuthVerdict::NoMandatoryConfinement => assert!(
                !enforces_confinement,
                "this platform's production backend enforces mandatory confinement, so it must \
                 state whether provider authentication can operate inside it instead of \
                 inheriting NoMandatoryConfinement"
            ),
            ConfinementAuthVerdict::Compatible | ConfinementAuthVerdict::Unsupported(_) => assert!(
                enforces_confinement,
                "a provider-authentication confinement verdict is stated for a platform whose \
                 production backend enforces no axes, so the verdict could never apply"
            ),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn authenticated_real_like_claude_refuses_before_provider_work() {
        let sandbox = Sandbox::named("g6-auth-confinement-refusal");
        seed_ready(&sandbox.db, "P-AUTH", "T-AUTH");
        let marker = sandbox.dir.join("provider-work-started");
        let fixture = fixture_script(
            &sandbox.dir,
            "claude-authenticated.sh",
            &format!(
                "if [ \"$1\" = \"--version\" ]; then printf '2.1.277 (Claude Code)\\n'; exit 0; fi\n\
                 if [ \"$1\" = \"auth\" ] && [ \"$2\" = \"status\" ]; then printf '{{\"loggedIn\":true}}\\n'; exit 0; fi\n\
                 touch {:?}\n\
                 exit 99\n",
                marker
            ),
        );
        let composition = sandbox.composition(Some(fixture));
        let (response, job) = run_via_dispatch(&composition, run_request("T-AUTH", &sandbox.repo));
        let _ = run_payload(&response);
        run_background(job.expect("accepted run yields a background job"));

        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let attempt = store
            .get_attempt(&agent_runtime::AttemptId::new("A-T-AUTH").unwrap())
            .expect("read attempt")
            .expect("attempt exists");
        assert_eq!(attempt.outcome(), Some(AttemptOutcome::Failed));
        assert!(
            attempt
                .detail()
                .unwrap_or_default()
                .contains(PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED),
            "refusal must carry the stable confinement-auth cause"
        );
        assert!(!marker.exists(), "provider work must not start");
        let audit = store
            .list_security_audit_ordered()
            .expect("read security audit");
        assert!(
            !audit.iter().any(|record| {
                record.fact.event == storage_sqlite::SecurityAuditEvent::ProviderInvocation
                    && record.fact.execution_id.as_deref() == Some("E-T-AUTH")
            }),
            "pre-launch refusal must not record a provider invocation"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "real provider: needs RUNLITH_CLAUDE_SMOKE=1 plus authenticated claude CLI"]
    fn real_claude_refuses_before_provider_work_under_production_g6_confinement() {
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
        assert_eq!(attempt.outcome(), Some(AttemptOutcome::Failed));
        let detail = attempt.detail().unwrap_or_default();
        assert!(
            detail.contains(PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED),
            "authenticated macOS provider must fail closed with the diagnosed typed cause: {detail}"
        );

        let audit = store
            .list_security_audit_ordered()
            .expect("read production security audit");
        assert!(!audit.iter().any(|record| {
            record.fact.event == storage_sqlite::SecurityAuditEvent::ProviderInvocation
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

    #[test]
    fn candidate_git_batch_parser_uses_exact_framed_lengths_and_oid_order() {
        let oid = "a".repeat(40);
        let body = b"tree\0body with newline\ninside";
        let mut output = format!("{oid} blob {}\n", body.len()).into_bytes();
        output.extend_from_slice(body);
        output.push(b'\n');
        assert_eq!(
            parse_git_batch(&output, std::slice::from_ref(&oid)).unwrap(),
            vec![(oid.clone(), "blob".to_owned(), body.to_vec())]
        );
        assert_eq!(
            parse_git_batch(&output, &["b".repeat(40)]),
            Err(evaluation::CandidateError::MissingObject)
        );
        output.push(b'!');
        assert_eq!(
            parse_git_batch(&output, &[oid]),
            Err(evaluation::CandidateError::UnsupportedObject)
        );
    }

    #[test]
    fn governed_git_batch_launch_forwards_requested_oids_on_stdin() {
        let git_executable = candidate_test_git_executable();
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-agentd-exec-candidate-git-batch-stdin-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(dir.join("db")).expect("sandbox db dir");
        let db = dir.join("db").join("state.sqlite3");
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).expect("sandbox repo dir");
        let init = std::process::Command::new(&git_executable)
            .args(["init", "-q"])
            .current_dir(&repo)
            .output()
            .expect("fixture Git init");
        assert!(init.status.success(), "fixture Git init failed");
        let commit = std::process::Command::new(&git_executable)
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ])
            .current_dir(&repo)
            .output()
            .expect("fixture Git commit");
        assert!(commit.status.success(), "fixture Git commit failed");
        let sandbox = Sandbox { dir, db, repo };
        seed_ready(&sandbox.db, "P-CANDIDATE-GIT", "T-CANDIDATE-GIT");
        let oid = std::process::Command::new(&git_executable)
            .args(["-C", sandbox.repo.to_str().unwrap(), "rev-parse", "HEAD"])
            .output()
            .expect("fixture Git rev-parse");
        assert!(oid.status.success());
        let oid = String::from_utf8(oid.stdout).unwrap().trim().to_owned();
        let mut input = oid.as_bytes().to_vec();
        input.push(b'\n');
        let output = run_candidate_process(
            &sandbox.db,
            CandidateProcessRequest {
                operation: CandidateG6Operation::GitRead,
                task_id: "T-CANDIDATE-GIT".to_owned(),
                execution_id: "E-CANDIDATE-GIT".to_owned(),
                attempt_id: "A-CANDIDATE-GIT".to_owned(),
                invocation_id: "candidate-git-batch-stdin-test".to_owned(),
                workspace: sandbox.repo.clone(),
                filesystem_roots: vec![sandbox.repo.clone()],
                executable: git_executable,
                args: vec![
                    "-C".into(),
                    sandbox.repo.as_os_str().into(),
                    "--no-replace-objects".into(),
                    "--no-lazy-fetch".into(),
                    "cat-file".into(),
                    "--batch".into(),
                ],
                input,
                support_read: vec![sandbox.repo.clone()],
                support_write: Vec::new(),
                max_stdout: 1024 * 1024,
            },
        )
        .expect("G6 and Runner governed batch process");
        let objects = parse_git_batch(&output.stdout, &[oid]).expect("framed commit response");
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].1, "commit");
    }

    #[test]
    fn candidate_object_source_modes_are_checked_through_g6_and_runner() {
        let sandbox = Sandbox::named("candidate-object-sources");
        seed_ready(&sandbox.db, "P-CANDIDATE-SOURCES", "T-CANDIDATE-SOURCES");
        let git_executable = candidate_test_git_executable();
        let common = sandbox.repo.join(".git").canonicalize().unwrap();
        let request = EvaluationRequest {
            task_id: TaskId::new("T-CANDIDATE-SOURCES").unwrap(),
            execution_id: ExecutionId::new("E-CANDIDATE-SOURCES").unwrap(),
            attempt_id: agent_runtime::AttemptId::new("A-CANDIDATE-SOURCES").unwrap(),
            summary: "fixture".to_owned(),
            evidence_ref: "fixture".to_owned(),
        };
        let evaluator = CandidateInputEvaluator {
            db_path: sandbox.db.clone(),
            git_executable,
            snapshot_parent: sandbox.dir.join("snapshots"),
            capture: effect_capture_local::LocalEffectCapture::default(),
            evaluator_executable_override: None,
        };
        let context = CandidateGitContext {
            request: &request,
            invocation_id: "candidate-object-sources-test",
            workspace: &common,
            gitdir: &common,
            roots: vec![common.clone()],
        };
        evaluator
            .validate_git_object_sources(&context)
            .expect("ordinary local repository is supported");

        let alternates = common.join("objects/info/alternates");
        std::fs::write(&alternates, b"/untrusted/objects\n").unwrap();
        assert!(evaluator.validate_git_object_sources(&context).is_err());
        std::fs::remove_file(&alternates).unwrap();

        let grafts = common.join("info/grafts");
        std::fs::write(&grafts, b"0000000000000000000000000000000000000000\n").unwrap();
        assert!(evaluator.validate_git_object_sources(&context).is_err());
        std::fs::remove_file(&grafts).unwrap();

        std::fs::write(
            common.join("shallow"),
            b"0000000000000000000000000000000000000000\n",
        )
        .unwrap();
        assert!(evaluator.validate_git_object_sources(&context).is_err());
        std::fs::remove_file(common.join("shallow")).unwrap();

        git(
            &sandbox.repo,
            &["config", "extensions.partialClone", "origin"],
        );
        assert!(evaluator.validate_git_object_sources(&context).is_err());
        git(
            &sandbox.repo,
            &["config", "--unset", "extensions.partialClone"],
        );
        git(&sandbox.repo, &["config", "remote.origin.promisor", "true"]);
        assert!(evaluator.validate_git_object_sources(&context).is_err());
    }

    fn candidate_test_git_executable() -> PathBuf {
        #[cfg(target_os = "macos")]
        let selected = PathBuf::from("/Library/Developer/CommandLineTools/usr/bin/git");
        #[cfg(not(target_os = "macos"))]
        let selected = std::env::var_os("PATH")
            .and_then(|path| {
                std::env::split_paths(&path)
                    .map(|directory| directory.join("git"))
                    .find(|path| path.is_file())
            })
            .expect("Git is present on PATH");
        let selected = selected.canonicalize().expect("fixture Git canonicalizes");
        assert!(selected.is_file(), "fixture Git is a regular file");
        selected
    }

    /// Locates the real `agentd` binary next to this test binary
    /// (`target/<profile>/deps/agentd-<hash>` -> `target/<profile>/agentd`).
    /// Production candidate evaluation re-execs the running `agentd`
    /// binary itself (with hidden `__candidate-*` arguments) to
    /// materialize and evaluate the sealed snapshot; under `cargo test
    /// --lib`, `std::env::current_exe()` resolves to the test-harness
    /// binary instead, which has no such dispatch. Tests exercising that
    /// real re-exec path must run `cargo build --bin agentd` first and
    /// pass this path as `CandidateInputEvaluator::evaluator_executable_override`.
    fn candidate_test_evaluator_executable() -> PathBuf {
        let mut path = std::env::current_exe().expect("test binary path is resolvable");
        path.pop();
        if path.ends_with("deps") {
            path.pop();
        }
        let bin_name = if cfg!(windows) {
            "agentd.exe"
        } else {
            "agentd"
        };
        let bin = path.join(bin_name);
        assert!(
            bin.is_file(),
            "expected the real agentd binary at {bin:?}; run `cargo build --bin agentd` first"
        );
        bin.canonicalize()
            .expect("agentd binary path canonicalizes")
    }

    /// Builds the trusted exact-param candidate-input requirement through
    /// the real `task-domain` API (not the planner: TASK-0125 planner
    /// authorship of these params is a separate, unmerged, out-of-scope
    /// decision per the TASK-0144 envelope).
    fn candidate_requirement(
        criterion_id: &str,
        path: &str,
        literal: &str,
    ) -> task_domain::EvidenceRequirement {
        task_domain::EvidenceRequirement::new(
            criterion_id,
            task_domain::RequirementKind::TestSuite,
            task_domain::SubjectBinding::ContentToolchainAndConfig,
            vec![
                task_domain::RequirementParam::new("selector", "candidate.file-contains.v1")
                    .unwrap(),
                task_domain::RequirementParam::new("path", path).unwrap(),
                task_domain::RequirementParam::new("literal", literal).unwrap(),
            ],
            Some(format!("sha256:{}", "7".repeat(64))),
            true,
        )
        .unwrap()
    }

    /// Seeds a project/Task reaching READY with exactly one acceptance
    /// criterion bound to the trusted candidate-input requirement.
    ///
    /// `dispatch_execution`'s `Run` request refuses a Task with no current
    /// accepted plan (`PlannedExecutionEntryOutcome::NoAcceptedPlan`), so a
    /// bare `task-domain` construction cannot reach the real production
    /// dispatch path on its own. This first materializes the Task through
    /// the real planner (`seed_ready_with_objective`) so that
    /// infrastructure gate is genuinely satisfied — the planner still
    /// writes only the current default, empty-params requirement
    /// (`planning_service.rs` `materialize_child_tasks`; authorship of
    /// selector/path/literal params is the open ADR-0029 Option B decision,
    /// out of scope here per the TASK-0144 envelope). It then overwrites
    /// the task's trusted `EvidenceRequirement` revision with the exact
    /// authorized candidate-input params through the real storage/domain
    /// API (`task_domain::Task::define_evidence_requirements` plus
    /// `SqliteStore::save_task`), never through the planner. The
    /// separately established accepted-plan binding is untouched: it is
    /// keyed by task id in its own table, not by requirement content.
    fn seed_candidate_ready(db: &Path, project: &str, task: &str, path: &str, literal: &str) {
        seed_ready_with_requirement(
            db,
            project,
            task,
            candidate_requirement("criterion-candidate", path, literal),
        );
    }

    /// Generalizes `seed_candidate_ready` over an arbitrary (possibly
    /// malformed) `EvidenceRequirement`, so R3 can seed a Task whose
    /// requirement names the candidate-input selector but fails
    /// `CandidateSelector::from_requirement` (missing/extra param, wrong
    /// selector value) and still reach the real production dispatch path.
    fn seed_ready_with_requirement(
        db: &Path,
        project: &str,
        task: &str,
        requirement: task_domain::EvidenceRequirement,
    ) {
        seed_ready_with_objective(db, project, task, format!("objective {task}"));

        let task_id = task_domain::TaskId::new(task).unwrap();
        let mut tasks = app_services::TaskService::open(db, app_services::SystemClock)
            .expect("open task service");
        let existing = tasks
            .get_task(&task_id)
            .unwrap()
            .expect("planner materialized the task");
        let mut replacement = task_domain::Task::draft(
            task_id.clone(),
            existing.project_id().clone(),
            existing.objective().to_owned(),
        )
        .unwrap();
        if let Some(parent) = existing.parent() {
            replacement.set_parent(parent.clone()).unwrap();
        }
        replacement
            .add_acceptance(requirement.criterion_id.clone())
            .unwrap();
        replacement
            .define_evidence_requirements(vec![requirement])
            .unwrap();
        replacement.mark_ready().unwrap();
        tasks.store_mut().save_task(&replacement).unwrap();
    }

    /// TASK-0144 blocker (d): the full production candidate-input path —
    /// `dispatch_execution` → `run_background` → `ProductionEvaluator`
    /// selection → the real `CandidateInputEvaluator` under G6/Runner —
    /// records typed integrity and captures evidence at RESULT_ADMITTED
    /// (operation status), leaving the Task VERIFYING for TASK-0125 claim
    /// verification (owner decision 2026-09-25). The durably persisted evidence
    /// binds the *exact* evaluated commit/tree, independently cross-checked
    /// against `git rev-parse` on the real repository (never a placeholder
    /// digest). A single green run is not evidence on its own: this test
    /// also proves the sealed candidate manifest is reproducible by
    /// recomputing it a second time from the same commit (`verify_commit`
    /// is deterministic over immutable Git objects) and that the captured
    /// `TestSuiteResult` reflects the real file content, not a stub.
    #[test]
    fn candidate_evaluator_admits_bound_evidence_without_completing_the_task() {
        let sandbox = Sandbox::named("candidate-e2e");
        std::fs::create_dir_all(sandbox.repo.join("src")).unwrap();
        std::fs::write(sandbox.repo.join("src/example.rs"), b"fn accepted() {}\n").unwrap();
        git(&sandbox.repo, &["add", "src/example.rs"]);
        git(&sandbox.repo, &["commit", "-q", "-m", "candidate content"]);
        let expected_head = git_stdout(&sandbox.repo, &["rev-parse", "HEAD"]);
        let expected_tree = git_stdout(&sandbox.repo, &["rev-parse", "HEAD^{tree}"]);

        seed_candidate_ready(
            &sandbox.db,
            "P-CANDIDATE-E2E",
            "T-CANDIDATE-E2E",
            "src/example.rs",
            "fn accepted()",
        );

        let fixture = fixture_script(
            &sandbox.dir,
            "success.sh",
            "printf '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"candidate content ready\",\"session_id\":\"11111111-1111-1111-1111-111111111111\"}'; exit 0\n",
        );
        let composition = sandbox
            .composition(Some(fixture))
            .with_candidate_evaluator_executable_override(candidate_test_evaluator_executable());
        let (response, job) =
            run_via_dispatch(&composition, run_request("T-CANDIDATE-E2E", &sandbox.repo));
        let run = run_payload(&response);
        run_background(job.expect("accepted run yields a background job"));

        let execution_id = ExecutionId::new(run.execution_id.as_str()).unwrap();
        let attempt_id = agent_runtime::AttemptId::new(run.attempt_id.as_str()).unwrap();

        let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
        let task = store
            .get_task(&task_domain::TaskId::new("T-CANDIDATE-E2E").unwrap())
            .unwrap()
            .expect("task exists");
        assert_eq!(
            task.state(),
            TaskState::Verifying,
            "candidate-input evaluation must never complete the Task (owner decision 2026-09-25)"
        );
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("RESULT_ADMITTED"),
            "evidence must be durably admitted before completion runs"
        );

        let evidence = store
            .get_candidate_evidence_for_completion(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .expect("candidate evidence persisted at RESULT_ADMITTED");
        assert_eq!(
            evidence.integrity.candidate_commit_oid, expected_head,
            "assessment must bind the exact evaluated commit"
        );
        assert_eq!(
            evidence.integrity.candidate_tree_oid, expected_tree,
            "assessment must bind the exact evaluated tree"
        );
        assert!(evidence
            .integrity
            .candidate_manifest_sha256
            .starts_with("sha256:"));
        assert_eq!(
            evidence.integrity.checkpoints.len(),
            4,
            "RESULT_ADMITTED state has exactly 4 checkpoints (Selection, BeforeEvaluation, AfterEvaluation, ResultAdmission)"
        );
        match evidence.effect.payload() {
            evaluation::EffectPayload::TestSuiteResult { passed, failed, .. } => {
                assert_eq!(
                    (*passed, *failed),
                    (1, 0),
                    "the captured result must reflect the sealed snapshot's real bytes"
                );
            }
            other => panic!("expected TestSuiteResult payload, got {other:?}"),
        }

        // Independent recomputation: resolving the same immutable commit a
        // second time (a fresh evaluator instance, as a restarted daemon
        // would) must reproduce byte-identical manifest/tree digests — this
        // is not the same code path that produced the original assessment.
        // Object reads need only the shared common Git directory (never a
        // worktree-specific gitdir): commits/trees/blobs are content
        // addressed and shared across every linked worktree.
        let anchor = store
            .get_trusted_anchor(
                store
                    .get_workspace_for_execution(&execution_id)
                    .unwrap()
                    .unwrap()
                    .id(),
            )
            .unwrap()
            .expect("trusted anchor recorded");
        let request = EvaluationRequest {
            task_id: task_domain::TaskId::new("T-CANDIDATE-E2E").unwrap(),
            execution_id: execution_id.clone(),
            attempt_id: attempt_id.clone(),
            summary: "verify".to_owned(),
            evidence_ref: "verify".to_owned(),
        };
        let fresh_evaluator = CandidateInputEvaluator {
            db_path: sandbox.db.clone(),
            git_executable: candidate_test_git_executable(),
            snapshot_parent: sandbox.dir.join("candidate-input-snapshots"),
            capture: effect_capture_local::LocalEffectCapture::default(),
            evaluator_executable_override: Some(candidate_test_evaluator_executable()),
        };
        std::fs::create_dir_all(&fresh_evaluator.snapshot_parent).unwrap();
        let format =
            evaluation::GitObjectFormat::from_oid(&evidence.integrity.candidate_commit_oid)
                .unwrap();
        let context = CandidateGitContext {
            request: &request,
            invocation_id: "candidate-e2e-recompute",
            workspace: anchor.canonical_commondir(),
            gitdir: anchor.canonical_commondir(),
            roots: vec![anchor.canonical_commondir().to_path_buf()],
        };
        let recomputed = fresh_evaluator
            .verify_commit(&context, &evidence.integrity.candidate_commit_oid, format)
            .expect("recomputing the sealed manifest over the same immutable commit");
        assert_eq!(
            recomputed.manifest.sha256, evidence.integrity.candidate_manifest_sha256,
            "the manifest digest must be reproducible from the immutable Git objects alone"
        );
        assert_eq!(recomputed.tree_oid, evidence.integrity.candidate_tree_oid);

        // Re-evaluation on RESULT_ADMITTED must still report "no verdict":
        // the Task is VERIFYING (not Completed), so a re-entry (e.g. after
        // a restart while the operation is still RESULT_ADMITTED) must again
        // return an error from the production composition, not replay the
        // original evidence as a fresh verdict.
        let replay = fresh_evaluator.evaluate(&request);
        assert!(
            replay.is_err(),
            "re-evaluating during VERIFYING state must return no verdict (RESULT_ADMITTED state reuse)"
        );
    }

    /// TASK-0144 R3 (owner review 2026-09-25): a Task whose requirement
    /// names the candidate-input selector but has malformed params
    /// (missing, extra, or a wrong selector value) must never reach
    /// `Verified` through the real production evaluator-selection wiring
    /// (`run_background_inner`'s `CandidateInputEvaluator::supports_task`
    /// check feeding `ProductionEvaluator`) — it must stay on exactly the
    /// same pending/manual path a Task on `main` (which has no candidate
    /// evaluator at all) follows: `supports_task` returns `false`,
    /// `ProductionEvaluator::Pending` is selected, `PendingEvaluator`
    /// always errors "no automated verifier", and the Task is left at
    /// `Verifying` forever — never a false `Completed`, never any
    /// candidate-input durable evidence at all.
    #[test]
    fn malformed_candidate_requirement_never_verifies_through_production_wiring() {
        for (label, requirement) in [
            (
                "missing_literal_param",
                task_domain::EvidenceRequirement::new(
                    "criterion-malformed",
                    task_domain::RequirementKind::TestSuite,
                    task_domain::SubjectBinding::ContentToolchainAndConfig,
                    vec![
                        task_domain::RequirementParam::new(
                            "selector",
                            "candidate.file-contains.v1",
                        )
                        .unwrap(),
                        task_domain::RequirementParam::new("path", "src/example.rs").unwrap(),
                    ],
                    Some(format!("sha256:{}", "7".repeat(64))),
                    true,
                )
                .unwrap(),
            ),
            (
                "extra_unreviewed_param",
                task_domain::EvidenceRequirement::new(
                    "criterion-malformed",
                    task_domain::RequirementKind::TestSuite,
                    task_domain::SubjectBinding::ContentToolchainAndConfig,
                    vec![
                        task_domain::RequirementParam::new(
                            "selector",
                            "candidate.file-contains.v1",
                        )
                        .unwrap(),
                        task_domain::RequirementParam::new("path", "src/example.rs").unwrap(),
                        task_domain::RequirementParam::new("literal", "fn accepted()").unwrap(),
                        task_domain::RequirementParam::new("unreviewed", "x").unwrap(),
                    ],
                    Some(format!("sha256:{}", "7".repeat(64))),
                    true,
                )
                .unwrap(),
            ),
            (
                "wrong_selector_value",
                task_domain::EvidenceRequirement::new(
                    "criterion-malformed",
                    task_domain::RequirementKind::TestSuite,
                    task_domain::SubjectBinding::ContentToolchainAndConfig,
                    vec![
                        task_domain::RequirementParam::new(
                            "selector",
                            "candidate.unreviewed-selector.v1",
                        )
                        .unwrap(),
                        task_domain::RequirementParam::new("path", "src/example.rs").unwrap(),
                        task_domain::RequirementParam::new("literal", "fn accepted()").unwrap(),
                    ],
                    Some(format!("sha256:{}", "7".repeat(64))),
                    true,
                )
                .unwrap(),
            ),
        ] {
            let sandbox = Sandbox::named(&format!("candidate-malformed-{label}"));
            std::fs::create_dir_all(sandbox.repo.join("src")).unwrap();
            std::fs::write(sandbox.repo.join("src/example.rs"), b"fn accepted() {}\n").unwrap();
            git(&sandbox.repo, &["add", "src/example.rs"]);
            git(&sandbox.repo, &["commit", "-q", "-m", "candidate content"]);

            let task = format!("T-CANDIDATE-MALFORMED-{}", label.to_ascii_uppercase());
            seed_ready_with_requirement(
                &sandbox.db,
                &format!("P-CANDIDATE-MALFORMED-{}", label.to_ascii_uppercase()),
                &task,
                requirement,
            );

            let fixture = fixture_script(
                &sandbox.dir,
                "success.sh",
                "printf '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"candidate content ready\",\"session_id\":\"22222222-2222-2222-2222-222222222222\"}'; exit 0\n",
            );
            let composition =
                sandbox
                    .composition(Some(fixture))
                    .with_candidate_evaluator_executable_override(
                        candidate_test_evaluator_executable(),
                    );
            let (response, job) = run_via_dispatch(&composition, run_request(&task, &sandbox.repo));
            let run = run_payload(&response);
            run_background(job.expect("accepted run yields a background job"));

            let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
            let stored_task = store
                .get_task(&task_domain::TaskId::new(&task).unwrap())
                .unwrap()
                .expect("task exists");
            assert_eq!(
                stored_task.state(),
                TaskState::Verifying,
                "[{label}] a malformed candidate requirement must never verify; the Task must stay \
                 on the pending/manual path exactly like main"
            );
            let execution_id = ExecutionId::new(run.execution_id.as_str()).unwrap();
            let attempt_id = agent_runtime::AttemptId::new(run.attempt_id.as_str()).unwrap();
            assert_eq!(
                store
                    .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                    .unwrap(),
                None,
                "[{label}] a malformed requirement must never route through the candidate \
                 evaluator at all (supports_task() must be false), so no candidate operation \
                 row is ever created"
            );
            assert!(
                store
                    .get_candidate_evidence(execution_id.as_str(), attempt_id.as_str())
                    .unwrap()
                    .is_none(),
                "[{label}] no candidate-input durable evidence may exist for a malformed requirement"
            );
        }
    }

    /// Shared scaffolding for the restart/idempotency/no-regression tests
    /// below: a real Task/Execution/Attempt/linked-worktree reaching
    /// exactly the state a production cycle stands in immediately before
    /// evaluation, built through the same public TaskService/
    /// RuntimeService/WorkspaceService primitives `continue_workspace_cycle`
    /// itself calls (never `dispatch_execution`/`run_background`, so the
    /// caller can drive the evaluator's own methods directly and
    /// repeatedly, simulating a daemon restart between phases with a fresh
    /// evaluator instance each time).
    struct CandidateRestartFixture {
        sandbox: Sandbox,
        request: EvaluationRequest,
        lease: workspace::WorkspaceLease,
    }

    fn setup_candidate_restart_fixture(
        suite: &str,
        project: &str,
        task: &str,
        execution: &str,
        attempt: &str,
    ) -> CandidateRestartFixture {
        let sandbox = Sandbox::named(suite);
        std::fs::create_dir_all(sandbox.repo.join("src")).unwrap();
        std::fs::write(sandbox.repo.join("src/example.rs"), b"fn accepted() {}\n").unwrap();
        git(&sandbox.repo, &["add", "src/example.rs"]);
        git(&sandbox.repo, &["commit", "-q", "-m", "candidate content"]);

        let project_id = task_domain::ProjectId::new(project).unwrap();
        let task_id = task_domain::TaskId::new(task).unwrap();
        let execution_id = ExecutionId::new(execution).unwrap();
        let attempt_id = agent_runtime::AttemptId::new(attempt).unwrap();

        let mut tasks = app_services::TaskService::open(&sandbox.db, app_services::SystemClock)
            .expect("open task service");
        tasks
            .create_project(
                &task_domain::Project::new(project_id.clone(), format!("{project} project"))
                    .unwrap(),
            )
            .unwrap();
        let criterion = format!("criterion-{task}");
        let mut built =
            task_domain::Task::draft(task_id.clone(), project_id, "prove restart safety").unwrap();
        built.add_acceptance(&criterion).unwrap();
        built
            .define_evidence_requirements(vec![candidate_requirement(
                &criterion,
                "src/example.rs",
                "fn accepted()",
            )])
            .unwrap();
        tasks.create_task(&built).unwrap();
        tasks.mark_ready(&task_id).unwrap();
        tasks.claim(&task_id).unwrap();
        tasks.start(&task_id).unwrap();

        let mut runtime =
            app_services::RuntimeService::open(&sandbox.db, app_services::SystemClock)
                .expect("open runtime service");
        runtime
            .create_execution(execution_id.clone(), task_id.clone())
            .unwrap();
        runtime
            .create_attempt(
                &execution_id,
                attempt_id.clone(),
                agent_runtime::ProviderRef::new("fixture").unwrap(),
            )
            .unwrap();

        std::fs::create_dir_all(sandbox.dir.join("workspaces")).unwrap();
        let mut workspace = app_services::WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            ManagedWorkspaceRoot::new(sandbox.dir.join("workspaces")).unwrap(),
            app_services::SystemClock,
        )
        .expect("open workspace service");
        let ensured = workspace
            .ensure_workspace_ready(&app_services::WorkspaceRequest {
                execution_id: execution_id.clone(),
                repository_ref: workspace::RepositoryRef::new(format!("repo-local:{suite}"))
                    .unwrap(),
                repository_path: sandbox.repo.to_string_lossy().into_owned(),
                base_revision: "HEAD".to_owned(),
            })
            .expect("real linked worktree comes up READY");

        runtime
            .conclude_attempt(
                &execution_id,
                &attempt_id,
                AttemptOutcome::Succeeded,
                "fixture attempt succeeded",
            )
            .unwrap();
        tasks
            .submit_result(&task_id, "fixture summary".to_owned())
            .unwrap();
        tasks.route_to_review(&task_id).unwrap();
        tasks.begin_verification(&task_id).unwrap();
        tasks.bind_evaluated_policy_base(&execution_id).unwrap();

        let request = EvaluationRequest {
            task_id,
            execution_id,
            attempt_id,
            summary: "fixture summary".to_owned(),
            evidence_ref: "fixture-evidence".to_owned(),
        };
        CandidateRestartFixture {
            sandbox,
            request,
            lease: ensured.lease,
        }
    }

    fn candidate_restart_evaluator(fixture: &CandidateRestartFixture) -> CandidateInputEvaluator {
        let evaluator = CandidateInputEvaluator {
            db_path: fixture.sandbox.db.clone(),
            git_executable: candidate_test_git_executable(),
            snapshot_parent: fixture.sandbox.dir.join("candidate-input-snapshots"),
            capture: effect_capture_local::LocalEffectCapture::default(),
            evaluator_executable_override: Some(candidate_test_evaluator_executable()),
        };
        std::fs::create_dir_all(&evaluator.snapshot_parent).unwrap();
        evaluator
    }

    /// TASK-0144 blockers (a)/(d): restart-safety at the durable boundary
    /// between "trusted result admitted" and "Task completion", exercised
    /// through the real `CandidateInputEvaluator`/`EvaluationPort` entry
    /// points production uses — not a synthetic storage-only replay.
    ///
    /// The production cycle (`finish()` in `orchestration_service.rs`)
    /// calls `evaluate()` then `revalidate_before_completion()` back to
    /// back inside one Rust call with no externally triggerable pause in
    /// between, so a literal process-kill mid-call cannot be injected from
    /// a same-process test without OS-level fault injection this codebase
    /// does not provide; see `setup_candidate_restart_fixture` for how this
    /// drives the evaluator's own methods directly instead. It proves:
    /// (1) `revalidate_before_completion()` is idempotent across a
    /// repeated call, including through the same-subject Completion-
    /// checkpoint replay path in `candidate_assessment_store.rs`; (2) once
    /// Completion is durable, a further `evaluate()` validly reuses the
    /// sealed evidence (exactly one RestartReuse checkpoint) instead of
    /// re-running capture; and (3) once the candidate commit changes
    /// underneath (tamper) *after* completion, `evaluate()` refuses to
    /// reuse it and — per R2 (owner review 2026-09-25) — the durable
    /// `COMPLETE` status is never regressed by that later failure; the
    /// failure is journaled in `status_history` instead.
    #[test]
    fn candidate_evaluator_restart_reuse_is_idempotent_and_tamper_is_refused() {
        let fixture = setup_candidate_restart_fixture(
            "candidate-restart",
            "P-CANDIDATE-RESTART",
            "T-CANDIDATE-RESTART",
            "E-CANDIDATE-RESTART",
            "A-CANDIDATE-RESTART",
        );
        let execution_id = fixture.request.execution_id.clone();
        let attempt_id = fixture.request.attempt_id.clone();
        let evaluator = candidate_restart_evaluator(&fixture);

        let verdict = evaluator
            .evaluate(&fixture.request)
            .expect("the real production evaluator admits durable evidence");
        assert_eq!(verdict, EvaluationVerdict::Verified);
        let store = storage_sqlite::SqliteStore::open(&fixture.sandbox.db).expect("open store");
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("RESULT_ADMITTED"),
            "evidence must be durably admitted before completion runs"
        );
        let admitted = store
            .get_candidate_evidence_for_completion(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(admitted.integrity.checkpoints.len(), 4);

        // -- simulated restart: a fresh evaluator instance (new process)
        // revalidates and completes.
        let restarted = candidate_restart_evaluator(&fixture);
        restarted
            .revalidate_before_completion(&fixture.request)
            .expect("completion revalidation succeeds after restart");
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("COMPLETE")
        );
        // Idempotent replay across a simulated crash immediately after
        // Completion persisted but before the caller observed success.
        restarted
            .revalidate_before_completion(&fixture.request)
            .expect("a repeated completion revalidation on the same subject is idempotent");
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("COMPLETE")
        );

        // Now that Completion is durable, a further `evaluate()` on the
        // same (execution, attempt) — e.g. a stray re-entry after the Task
        // itself failed to observe completion — validly reuses the sealed
        // evidence instead of re-running capture, appending exactly one
        // RestartReuse checkpoint.
        let reuse_verdict = restarted
            .evaluate(&fixture.request)
            .expect("re-evaluating an already-completed attempt reuses durable evidence");
        assert_eq!(reuse_verdict, EvaluationVerdict::Verified);
        let reused = store
            .get_candidate_evidence(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            reused.integrity.checkpoints.len(),
            6,
            "reuse appends exactly one RestartReuse checkpoint on top of Completion, it does not re-capture"
        );

        // -- tamper/mismatch negative: HEAD moves in the linked worktree
        // after durable evidence was sealed --
        let worktree = PathBuf::from(fixture.lease.worktree_ref());
        std::fs::write(worktree.join("src/example.rs"), b"fn tampered() {}\n").unwrap();
        git(
            &worktree,
            &["commit", "-aq", "-m", "tamper after admission"],
        );

        let tampered = restarted.evaluate(&fixture.request);
        assert!(
            tampered.is_err(),
            "a changed candidate commit must never silently reuse a prior successful verdict"
        );
        // R2 (owner review 2026-09-25, MAJOR): a failure classified from a
        // later, tampered re-entry must never regress an already-COMPLETE
        // row — it is journaled in status_history instead.
        let status_after_tamper = store
            .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
            .unwrap();
        assert_eq!(
            status_after_tamper.as_deref(),
            Some("COMPLETE"),
            "a failure after COMPLETE must never regress the durable status"
        );
        let history_after_tamper = store
            .candidate_operation_history(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .unwrap();
        assert!(
            history_after_tamper.contains("BLOCKED") || history_after_tamper.contains("MISMATCH"),
            "the tamper refusal must still be durably journaled, got {history_after_tamper:?}"
        );
    }

    /// TASK-0144 R2 (owner review 2026-09-25, MAJOR): the crash-window
    /// deadlock the review flagged — re-invoking `evaluate()` while the
    /// operation is RESULT_ADMITTED (before completion) must be a safe
    /// idempotent path, and a failure classified after RESULT_ADMITTED
    /// must never regress it — exercised through the real
    /// `CandidateInputEvaluator` entry point, not only store calls.
    /// "Completion then succeeds exactly once" is covered by
    /// `candidate_evaluator_restart_reuse_is_idempotent_and_tamper_is_refused`'s
    /// single `revalidate_before_completion` -> COMPLETE transition; this
    /// test focuses on the RESULT_ADMITTED window itself.
    #[test]
    fn candidate_evaluator_result_admitted_retry_is_idempotent_and_never_regresses() {
        let fixture = setup_candidate_restart_fixture(
            "candidate-cas",
            "P-CANDIDATE-CAS",
            "T-CANDIDATE-CAS",
            "E-CANDIDATE-CAS",
            "A-CANDIDATE-CAS",
        );
        let execution_id = fixture.request.execution_id.clone();
        let attempt_id = fixture.request.attempt_id.clone();
        let evaluator = candidate_restart_evaluator(&fixture);

        let first = evaluator
            .evaluate(&fixture.request)
            .expect("first evaluate admits durable evidence");
        assert_eq!(first, EvaluationVerdict::Verified);
        let store = storage_sqlite::SqliteStore::open(&fixture.sandbox.db).expect("open store");
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("RESULT_ADMITTED")
        );
        let after_first = store
            .get_candidate_evidence_for_completion(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(after_first.integrity.checkpoints.len(), 4);

        // (i) Re-invoking evaluate() with no completion in between, and no
        // change to the subject, is a safe idempotent no-op: it neither
        // errors nor corrupts state (previously this hit
        // `StorageError::Corrupt` from the completion-gated getter).
        let second = evaluator
            .evaluate(&fixture.request)
            .expect("re-invoking evaluate() before completion must be idempotent, not Corrupt");
        assert_eq!(second, EvaluationVerdict::Verified);
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("RESULT_ADMITTED"),
            "an idempotent pre-completion retry must not change the durable status"
        );
        let after_second = store
            .get_candidate_evidence_for_completion(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            after_second.integrity.checkpoints.len(),
            4,
            "an idempotent pre-completion retry must not append a checkpoint or re-run capture"
        );

        // (ii) A failure classified after RESULT_ADMITTED must never
        // downgrade it: tamper the worktree (still before completion) and
        // re-invoke the real evaluator entry point again.
        let worktree = PathBuf::from(fixture.lease.worktree_ref());
        std::fs::write(worktree.join("src/example.rs"), b"fn tampered() {}\n").unwrap();
        git(
            &worktree,
            &["commit", "-aq", "-m", "tamper before completion"],
        );
        let tampered = evaluator.evaluate(&fixture.request);
        assert!(
            tampered.is_err(),
            "a changed candidate commit must never be silently reused"
        );
        assert_eq!(
            store
                .candidate_operation_status(execution_id.as_str(), attempt_id.as_str())
                .unwrap()
                .as_deref(),
            Some("RESULT_ADMITTED"),
            "a failure classified after RESULT_ADMITTED must never regress the durable status"
        );
        let history = store
            .candidate_operation_history(execution_id.as_str(), attempt_id.as_str())
            .unwrap()
            .unwrap();
        assert!(
            history.contains("BLOCKED") || history.contains("MISMATCH"),
            "the failure must still be durably journaled in status_history, got {history:?}"
        );
    }

    #[test]
    fn candidate_result_parser_requires_exact_manifest_and_one_outcome() {
        let manifest = format!("sha256:{}", "a".repeat(64));
        let result = format!(
            "candidate-test-result/v1\nselector=candidate.file-contains.v1\nmanifest={manifest}\npassed=1\nfailed=0\nskipped=0\nresult=sha256:{}\n",
            "b".repeat(64)
        );
        let parsed = parse_candidate_result(result.as_bytes(), &manifest).unwrap();
        let candidate = evaluation::VerifiedCandidate {
            commit_oid: "a".repeat(40),
            tree_oid: "c".repeat(40),
            manifest: evaluation::CandidateManifest {
                version: 1,
                entries: vec![],
                sha256: manifest.clone(),
                total_bytes: 0,
            },
            files: vec![evaluation::CandidateFile {
                path: b"README.md".to_vec(),
                mode: 0o100644,
                git_oid: None,
                bytes: b"expected literal".to_vec(),
            }],
        };
        let requirement = evaluation::CandidateSelector {
            criterion_id: "criterion-1".to_owned(),
            selector: "candidate.file-contains.v1".to_owned(),
            path: "README.md".to_owned(),
            literal: "expected literal".to_owned(),
            requirement_revision:
                "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_owned(),
        };
        assert!(validate_candidate_result(&parsed, &candidate, &requirement).is_err());
        assert!(
            parse_candidate_result(result.as_bytes(), &format!("sha256:{}", "c".repeat(64)))
                .is_err()
        );
        let inconsistent = result.replace("passed=1\nfailed=0", "passed=1\nfailed=1");
        assert!(parse_candidate_result(inconsistent.as_bytes(), &manifest).is_err());
    }

    #[test]
    fn resolve_pinned_git_first_existing_candidate_wins() {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let test_dir = std::env::temp_dir().join(format!(
            "runlith-pinned-git-first-{id}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("test dir");

        let git1 = test_dir.join("git1");
        let git2 = test_dir.join("git2");

        std::fs::write(&git1, b"#!/bin/sh\necho git1\n").expect("git1 file");
        std::fs::write(&git2, b"#!/bin/sh\necho git2\n").expect("git2 file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&git1, std::fs::Permissions::from_mode(0o755))
                .expect("git1 chmod");
            std::fs::set_permissions(&git2, std::fs::Permissions::from_mode(0o755))
                .expect("git2 chmod");
        }

        let result = resolve_pinned_git(&[git1.as_path(), git2.as_path()]);
        assert!(result.is_ok(), "first existing candidate should succeed");
        let path = result.unwrap();
        assert_eq!(
            path,
            git1.canonicalize().expect("canonical git1"),
            "first candidate should win"
        );

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn resolve_pinned_git_falls_through_missing_first_candidate() {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let test_dir = std::env::temp_dir().join(format!(
            "runlith-pinned-git-missing-{id}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("test dir");

        let git1 = test_dir.join("missing");
        let git2 = test_dir.join("git2");

        std::fs::write(&git2, b"#!/bin/sh\necho git2\n").expect("git2 file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&git2, std::fs::Permissions::from_mode(0o755))
                .expect("git2 chmod");
        }

        let result = resolve_pinned_git(&[git1.as_path(), git2.as_path()]);
        assert!(
            result.is_ok(),
            "missing first candidate should fall through"
        );
        let path = result.unwrap();
        assert_eq!(
            path,
            git2.canonicalize().expect("canonical git2"),
            "second candidate should be used when first missing"
        );

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn resolve_pinned_git_none_present_returns_error() {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let test_dir = std::env::temp_dir().join(format!(
            "runlith-pinned-git-none-{id}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("test dir");

        let git1 = test_dir.join("missing1");
        let git2 = test_dir.join("missing2");

        let result = resolve_pinned_git(&[git1.as_path(), git2.as_path()]);
        assert!(result.is_err(), "no candidates should error");
        let err = result.unwrap_err();
        assert_eq!(
            err,
            "no pinned trusted Git executable is available for candidate-input reads"
        );
        assert!(!err.contains("/"), "error must not leak paths");

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    #[cfg(unix)]
    fn resolve_pinned_git_rejects_symlink_redirection() {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let test_dir = std::env::temp_dir().join(format!(
            "runlith-pinned-git-symlink-{id}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("test dir");

        let other_dir = test_dir.join("other");
        std::fs::create_dir(&other_dir).expect("other dir");

        let target = other_dir.join("git_target");
        std::fs::write(&target, b"#!/bin/sh\necho git\n").expect("git target");

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .expect("git target chmod");

        let symlink = test_dir.join("git_link");
        std::os::unix::fs::symlink(&target, &symlink).expect("create symlink");

        let result = resolve_pinned_git(&[symlink.as_path()]);
        assert!(
            result.is_err(),
            "symlink redirecting outside parent should be rejected"
        );

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn resolve_pinned_git_rejects_directory() {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let test_dir = std::env::temp_dir().join(format!(
            "runlith-pinned-git-dir-{id}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).expect("test dir");

        let git_dir = test_dir.join("git_dir");
        std::fs::create_dir(&git_dir).expect("git_dir");

        let result = resolve_pinned_git(&[git_dir.as_path()]);
        assert!(result.is_err(), "directory should be rejected");

        let _ = std::fs::remove_dir_all(&test_dir);
    }
}
