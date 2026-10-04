//! Canonical-seam executor over the Claude CLI (TASK-0046, G5).
//!
//! [`ClaudeExecutor`] implements [`scheduler::ExecutionPort`], the same
//! provider-neutral seam the test-side `FakeExecutor` wraps — one injected
//! executor per orchestration cycle, no provider comparison, no routing,
//! no retry. It binds one attempt's execution to one CLI invocation:
//!
//! ```text
//! WorkItem (durable identities only)
//!  + bound prompt (built at construction from Task facts)
//!  + bound workdir (the attempt's WorkspaceLease path, fail-closed)
//!  → spawn_and_supervise (owned child, timeout, cancel)
//!  → parse_provider_output (untrusted → canonical)
//!  → WorkEvidence / ExecutionProblem
//! ```
//!
//! What this executor never does (tested where observable): mutate Task
//! state, write storage or the journal, touch Git, schedule, retry,
//! verify, merge, select models, or mark anything complete. A successful
//! provider result becomes submittable *evidence* through the normal
//! orchestration interpretation — exactly like FakeAgent success — and
//! only the independent evaluation path can complete a Task.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_runtime::ProviderRef;
use evaluation::{ProcessExitObservation, ProcessExitObservationSink};
use runner::{ChildSecretEnvironment, SandboxedLaunch};
use scheduler::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};

use crate::argv::build_argv;
use crate::config::{bounded_detail, ClaudeConfig};
use crate::environment::ProviderEnvironment;
use crate::failure::{looks_like_auth_failure, looks_like_transient_failure, ClaudeFailure};
use crate::output::{parse_provider_output, ParsedOutput};
use crate::process::{
    spawn_and_supervise, CancelFlag, ChildOutcome, ProcessInvocation, ProviderLaunch,
};
use crate::tracker::LiveProcessTracker;
use crate::usage::UsageSample;
use crate::CLAUDE_PROVIDER_REF;

const TRUSTED_HOST_ENVIRONMENT_ONLY: &str =
    "explicit host environment is restricted to trusted-host execution";

/// Full adapter outcome for consumers that want usage alongside evidence.
/// [`ExecutionPort::execute`] returns the evidence half; this shape keeps
/// the usage half available for the future Usage Ledger contract without
/// widening the canonical seam today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeOutcome {
    /// Canonical evidence for the orchestration pipeline.
    pub evidence: WorkEvidence,
    /// Provider usage observation (possibly unknown).
    pub usage: UsageSample,
    /// Opaque provider session reference (evidence only).
    pub session_ref: String,
    /// Full capture-bounded provider result retained inside this adapter for
    /// structured consumers such as the proposal-only planner. Ordinary
    /// execution continues to expose only the bounded evidence summary.
    pub(crate) provider_result: String,
}

/// Adapter-internal observation of one prompt run (see
/// [`ClaudeExecutor::run_prompt_observed`]).
pub(crate) enum ObservedPromptRun {
    /// Refused strictly before any provider child existed: zero spend.
    NotStarted(ClaudeFailure),
    /// A provider child may have run. `usage` is whatever the provider
    /// reported in its final output (possibly unknown), even on failure.
    /// The success payload is boxed to keep the enum small.
    Started {
        result: Result<Box<ClaudeOutcome>, ClaudeFailure>,
        usage: UsageSample,
    },
}

/// One bound Claude execution: config plus the attempt's prompt and
/// workspace directory. Constructed per attempt by the composition layer
/// (or tests); single-flight (one in-flight `execute` at a time).
#[derive(Clone)]
pub struct ClaudeExecutor {
    config: ClaudeConfig,
    prompt: String,
    workdir: PathBuf,
    environment: Result<ProviderEnvironment, String>,
    trusted_host_only: bool,
    tracker: Option<LiveProcessTracker>,
    process_exit_observer: Option<Arc<dyn ProcessExitObservationSink>>,
    cancel: CancelFlag,
    launch: ProviderLaunch,
}

impl ClaudeExecutor {
    /// Binds one execution. `prompt` must be non-blank bounded text (use
    /// [`crate::build_task_prompt`]); `workdir` must be non-blank (its
    /// existence as a directory is enforced fail-closed at execute time,
    /// so a workspace torn down between construction and execution fails
    /// instead of falling back anywhere).
    ///
    /// The executor starts with no Runner sandbox attached and therefore
    /// refuses to spawn (TASK-SEC-R1 R1-H1). Production composition attaches
    /// one with [`Self::with_sandboxed_launch`]; the explicit unmanaged
    /// supervisor used by provider contract tests is
    /// [`Self::bind_trusted_host`].
    pub fn bind(
        config: ClaudeConfig,
        prompt: impl Into<String>,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        Self::bind_with_launch(
            config,
            prompt,
            workdir,
            environment_base,
            ProviderLaunch::Unattached,
            &ambient_host_variable,
        )
    }

    /// Binds one execution that will run under the trusted-host supervisor:
    /// no Runner sandbox is attached and the provider spawns the configured
    /// executable itself. This is an explicit trust decision for provider
    /// contract tests and trusted-host development runs — never the
    /// production G6 path, which goes through
    /// [`Self::with_sandboxed_launch`].
    #[cfg(any(test, feature = "trusted-host"))]
    pub fn bind_trusted_host(
        config: ClaudeConfig,
        prompt: impl Into<String>,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        Self::bind_with_launch(
            config,
            prompt,
            workdir,
            environment_base,
            ProviderLaunch::TrustedHost,
            &ambient_host_variable,
        )
    }

    /// [`Self::bind_trusted_host`] with the provider's host variables (PATH,
    /// HOME, USER, ...) taken only from `host_environment` instead of the
    /// ambient process environment, so deterministic fixtures do not depend
    /// on the runner's login session (TASK-0155). The HOME/USER requirement
    /// is unchanged. Runner environment/support accessors refuse this mode;
    /// attaching a sandboxed launch permanently invalidates its environment.
    #[cfg(any(test, feature = "trusted-host"))]
    pub fn bind_trusted_host_with_host_environment(
        config: ClaudeConfig,
        prompt: impl Into<String>,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
        host_environment: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<Self, String> {
        let host_environment: Vec<(OsString, OsString)> = host_environment.into_iter().collect();
        let mut executor = Self::bind_with_launch(
            config,
            prompt,
            workdir,
            environment_base,
            ProviderLaunch::TrustedHost,
            &|name| {
                host_environment
                    .iter()
                    .find(|(key, _)| *key == *name)
                    .map(|(_, value)| value.clone())
            },
        )?;
        executor.trusted_host_only = true;
        Ok(executor)
    }

    fn bind_with_launch(
        config: ClaudeConfig,
        prompt: impl Into<String>,
        workdir: impl Into<PathBuf>,
        environment_base: impl Into<PathBuf>,
        launch: ProviderLaunch,
        host: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, String> {
        let prompt = prompt.into();
        if prompt.trim().is_empty() {
            return Err("claude prompt must not be blank".to_owned());
        }
        let workdir = workdir.into();
        if workdir.as_os_str().is_empty() {
            return Err("claude workdir must not be blank".to_owned());
        }
        let environment_base = environment_base.into();
        if environment_base.as_os_str().is_empty() {
            return Err("claude provider environment base must not be blank".to_owned());
        }
        let environment = ProviderEnvironment::prepare_from(&workdir, &environment_base, host);
        Ok(Self {
            config,
            prompt,
            workdir,
            environment,
            trusted_host_only: false,
            tracker: None,
            process_exit_observer: None,
            cancel: CancelFlag::fresh(),
            launch,
        })
    }

    /// Attaches a live-process tracker (composition-root wiring). The
    /// tracker observes spawn/terminal transitions for workspace-cleanup
    /// liveness; it grants no authority of any other kind.
    #[must_use]
    pub fn with_tracker(mut self, tracker: LiveProcessTracker) -> Self {
        self.tracker = Some(tracker);
        self
    }

    /// Attaches the trusted raw process-exit observer used by G8 capture.
    /// The provider reports only facts from its owned supervised child; it
    /// does not mint an `ObservedEffect` or touch persistence.
    #[must_use]
    pub fn with_process_exit_observer(
        mut self,
        observer: Arc<dyn ProcessExitObservationSink>,
    ) -> Self {
        self.process_exit_observer = Some(observer);
        self
    }

    /// Attaches an externally owned cancellation flag (composition-root
    /// wiring). The production daemon registers one flag per execution so
    /// a later `cancel` IPC on another connection signals exactly this
    /// run's child tree; the flag is otherwise identical to the
    /// freshly created one from [`Self::bind`].
    #[must_use]
    pub fn with_cancel_flag(mut self, cancel: CancelFlag) -> Self {
        self.cancel = cancel;
        self
    }

    /// Attaches the verified Runner launch (TASK-SEC-R1 R1-H1). The provider
    /// child can then only be spawned through the backend-installed sandbox
    /// recipe: there is no pre-sandbox command the provider could spawn by
    /// forgetting a step.
    #[must_use]
    pub fn with_sandboxed_launch(self, launch: Arc<SandboxedLaunch>) -> Self {
        self.with_sandboxed_launch_and_secrets(launch, None)
    }

    /// [`Self::with_sandboxed_launch`] plus late-bound secrets (ADR-0039)
    /// installed into the first spawned child only, outside the plan.
    #[must_use]
    pub fn with_sandboxed_launch_and_secrets(
        mut self,
        launch: Arc<SandboxedLaunch>,
        secrets: Option<ChildSecretEnvironment>,
    ) -> Self {
        if self.trusted_host_only {
            // Discard fixture values and refuse before spawn; never retain
            // the trusted-host launch as a fallback after this composition.
            self.environment = Err(TRUSTED_HOST_ENVIRONMENT_ONLY.to_owned());
        }
        self.launch = ProviderLaunch::sandboxed(launch, secrets);
        self
    }

    /// Canonical cancellation: kills only the owned in-flight child (or
    /// no-ops when nothing runs). Idempotent; safe on exited children.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// The bound working directory (the attempt's lease path).
    #[must_use]
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Exact configured executable identity used by Runner authorization.
    #[must_use]
    pub fn executable(&self) -> &std::ffi::OsStr {
        self.config.executable()
    }

    /// Narrow provider-owned paths required by the concrete OS sandbox.
    /// Environment preparation has already validated the runtime root before
    /// these paths are exposed to the composition layer.
    pub fn isolation_support_read_roots(&self) -> Result<Vec<PathBuf>, String> {
        self.runner_environment()?.isolation_support_read_roots()
    }

    pub fn isolation_support_write_roots(&self) -> Result<Vec<PathBuf>, String> {
        Ok(self.runner_environment()?.isolation_support_write_roots())
    }

    /// The exact explicit child environment this execution would install,
    /// for the Runner to plan and enforce (TASK-SEC-R1 R1-H2). Values stay
    /// composition-resolved directories; no ambient variable is read here.
    pub fn child_environment(&self) -> Result<Vec<(OsString, OsString)>, String> {
        Ok(self.runner_environment()?.child_environment())
    }

    fn runner_environment(&self) -> Result<&ProviderEnvironment, String> {
        if self.trusted_host_only {
            return Err(TRUSTED_HOST_ENVIRONMENT_ONLY.to_owned());
        }
        self.environment.as_ref().map_err(Clone::clone)
    }

    /// Runs one bounded execution, returning the full adapter outcome
    /// (evidence + usage). The provider-neutral seam uses [`Self::execute`].
    pub fn execute_detailed(&self, work: &WorkItem) -> Result<ClaudeOutcome, ClaudeFailure> {
        if let Some(tracker) = &self.tracker {
            tracker.mark_live(&work.execution_id);
        }
        let result = self.run(work);
        if let Some(tracker) = &self.tracker {
            tracker.mark_finished(&work.execution_id);
        }
        result
    }

    fn run(&self, work: &WorkItem) -> Result<ClaudeOutcome, ClaudeFailure> {
        match self.run_prompt_observed_for_work(&self.prompt, Some(work)) {
            ObservedPromptRun::NotStarted(failure) => Err(failure),
            ObservedPromptRun::Started { result, .. } => result.map(|outcome| *outcome),
        }
    }

    /// Runs one prompt and additionally reports whether a provider child may
    /// have started plus any provider-reported usage recoverable from its
    /// final output, including failed runs. Used by the proposal-only planner
    /// so post-spawn outcomes never become invisible spend (ADR-0027 §6).
    pub(crate) fn run_prompt_observed(&self, prompt: &str) -> ObservedPromptRun {
        self.run_prompt_observed_for_work(prompt, None)
    }

    fn run_prompt_observed_for_work(
        &self,
        prompt: &str,
        work: Option<&WorkItem>,
    ) -> ObservedPromptRun {
        let environment = match self.environment.as_ref() {
            Ok(environment) => environment,
            Err(detail) => {
                return ObservedPromptRun::NotStarted(ClaudeFailure::EnvironmentIsolation {
                    detail: bounded_detail(detail.clone()),
                })
            }
        };
        let argv = build_argv(&self.config, prompt);
        let outcome = match spawn_and_supervise(
            ProcessInvocation {
                executable: self.config.executable(),
                argv: &argv,
                workdir: &self.workdir,
                environment,
                launch: &self.launch,
            },
            &self.cancel,
            self.config.timeout(),
            self.config.max_output_bytes(),
            self.config.capture_grace(),
        ) {
            Ok(outcome) => outcome,
            // Only failures that are raised strictly before a child exists
            // are zero-spend. `SpawnFailed` also covers post-spawn wait
            // failures, so it stays conservatively "started".
            Err(
                failure @ (ClaudeFailure::WorkspaceBinding { .. }
                | ClaudeFailure::LaunchUnmanaged { .. }
                | ClaudeFailure::CliMissing { .. }),
            ) => return ObservedPromptRun::NotStarted(failure),
            Err(failure) => {
                return ObservedPromptRun::Started {
                    result: Err(failure),
                    usage: UsageSample::unknown(),
                }
            }
        };
        match outcome {
            ChildOutcome::Cancelled => ObservedPromptRun::Started {
                result: Err(ClaudeFailure::Cancelled),
                usage: UsageSample::unknown(),
            },
            ChildOutcome::TimedOut => ObservedPromptRun::Started {
                result: Err(ClaudeFailure::TimedOut {
                    detail: bounded_detail(format!(
                        "claude execution exceeded {:?}; child killed",
                        self.config.timeout()
                    )),
                }),
                usage: UsageSample::unknown(),
            },
            ChildOutcome::Exited {
                code,
                stdout,
                stderr,
            } => {
                if let (Some(observer), Some(work)) = (&self.process_exit_observer, work) {
                    observer.observe_process_exit(ProcessExitObservation::from_captured_streams(
                        work.execution_id.clone(),
                        work.attempt_id.clone(),
                        code,
                        &stdout,
                        &stderr,
                    ));
                }
                let result = self.interpret_exit(code, &stdout, &stderr).map(Box::new);
                let usage = match &result {
                    Ok(outcome) => outcome.usage.clone(),
                    Err(_) => crate::output::reported_usage(&String::from_utf8_lossy(&stdout)),
                };
                ObservedPromptRun::Started { result, usage }
            }
        }
    }

    /// Maps one observed child exit onto canonical evidence or failure.
    /// Classification uses structured signals (exit code, JSON fields);
    /// the auth-marker table is the only textual fallback, and it can
    /// only ever produce `AuthRequired` (unknown-safe).
    fn interpret_exit(
        &self,
        code: Option<i32>,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<ClaudeOutcome, ClaudeFailure> {
        let stdout_text = String::from_utf8_lossy(stdout);
        let stderr_text = String::from_utf8_lossy(stderr);
        match code {
            Some(0) => {
                let parsed = parse_provider_output(stdout_text.trim())?;
                Ok(canonical_outcome(&parsed, self.config.max_summary_chars()))
            }
            other => {
                let excerpt = bounded_detail(stderr_text.trim().to_owned());
                if looks_like_auth_failure(&excerpt) {
                    return Err(ClaudeFailure::AuthRequired {
                        detail: bounded_detail(format!(
                            "claude reports missing/invalid authentication: {excerpt}"
                        )),
                    });
                }
                // F5: rate-limit/overload/5xx prose classifies as
                // Transient (distinguishable from hard failure); the
                // parseable-error path below is checked the same way.
                if looks_like_transient_failure(&excerpt) {
                    return Err(ClaudeFailure::transient(format!(
                        "claude reports transient upstream failure: {excerpt}"
                    )));
                }
                // A parseable error object upgrades the detail; an
                // unparseable one keeps exit-code evidence. Neither path
                // can become success.
                let detail = match parse_provider_output(stdout_text.trim()) {
                    Ok(parsed) => format!(
                        "claude exited with code {other:?} (provider subtype {:?}): {}",
                        parsed.subtype,
                        truncate(&parsed.result_text, self.config.max_summary_chars())
                    ),
                    Err(_) => format!("claude exited with code {other:?}: {excerpt}"),
                };
                if looks_like_transient_failure(&detail) {
                    return Err(ClaudeFailure::transient(format!(
                        "claude reports transient upstream failure: {detail}"
                    )));
                }
                Err(ClaudeFailure::failed(detail))
            }
        }
    }
}

impl ExecutionPort for ClaudeExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new(CLAUDE_PROVIDER_REF).expect("static provider ref is valid")
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        match self.execute_detailed(work) {
            Ok(outcome) => Ok(outcome.evidence),
            Err(ClaudeFailure::TimedOut { detail }) => Err(ExecutionProblem::TimedOut { detail }),
            Err(ClaudeFailure::Cancelled) => Err(ExecutionProblem::Cancelled),
            Err(ClaudeFailure::Transient { detail }) => Err(ExecutionProblem::Transient { detail }),
            Err(other) => Err(ExecutionProblem::Failed {
                detail: other.to_string(),
            }),
        }
    }
}

/// Host-variable source for the production and default trusted-host paths.
fn ambient_host_variable(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// Maps one parsed provider result onto canonical evidence. The session
/// reference travels only inside `evidence_ref` as opaque provider
/// state; orchestration ties the evidence to the durable attempt when it
/// interprets the outcome (never the adapter's job).
fn canonical_outcome(parsed: &ParsedOutput, max_summary: usize) -> ClaudeOutcome {
    let summary = truncate(&parsed.result_text, max_summary);
    ClaudeOutcome {
        evidence: WorkEvidence {
            summary: summary.clone(),
            evidence_ref: format!("claude:session:{}", parsed.session_ref),
            partial: false,
        },
        usage: parsed.usage.clone(),
        session_ref: parsed.session_ref.clone(),
        provider_result: parsed.result_text.clone(),
    }
}

/// Bounds evidence text with an explicit marker. Blank-after-trim input
/// is unreachable (the parser rejects it) but maps to a placeholder
/// rather than an empty summary.
fn truncate(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "(empty provider result)".to_owned();
    }
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }
    let kept: String = trimmed.chars().take(max_chars).collect();
    format!("{kept}…[truncated]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{AttemptId, ExecutionId};
    use std::sync::atomic::{AtomicU64, Ordering};
    use task_domain::TaskId;

    static ENV_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn work() -> WorkItem {
        WorkItem {
            task_id: TaskId::new("T-1").unwrap(),
            execution_id: ExecutionId::new("E-1").unwrap(),
            attempt_id: AttemptId::new("A-1").unwrap(),
        }
    }

    fn bind_default(prompt: &str) -> ClaudeExecutor {
        ClaudeExecutor::bind(
            ClaudeConfig::default(),
            prompt,
            std::env::temp_dir(),
            test_environment_base(),
        )
        .unwrap()
    }

    fn test_environment_base() -> PathBuf {
        let id = ENV_COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir().join(format!(
            "runlith-provider-env-unit-{}-{id}",
            std::process::id()
        ))
    }

    #[test]
    fn provider_ref_is_opaque_identity() {
        let executor = bind_default("do work");
        assert_eq!(executor.provider_ref().as_str(), "claude-code");
        assert_eq!(executor.provider_ref().as_str(), CLAUDE_PROVIDER_REF);
    }

    #[test]
    fn bind_rejects_blank_prompt_and_workdir() {
        assert!(ClaudeExecutor::bind(
            ClaudeConfig::default(),
            "  ",
            std::env::temp_dir(),
            test_environment_base()
        )
        .is_err());
        assert!(ClaudeExecutor::bind(
            ClaudeConfig::default(),
            "prompt",
            PathBuf::from(""),
            test_environment_base()
        )
        .is_err());
    }

    #[test]
    fn missing_cli_maps_to_failed_never_success() {
        let config = ClaudeConfig::default()
            .with_executable("runlith-definitely-not-a-binary-xyz")
            .unwrap();
        let executor = ClaudeExecutor::bind(
            config,
            "prompt",
            std::env::temp_dir(),
            test_environment_base(),
        )
        .unwrap();
        let err = executor
            .execute(&work())
            .expect_err("missing CLI must fail");
        assert!(
            matches!(err, ExecutionProblem::Failed { .. }),
            "missing CLI must be Failed, got {err:?}"
        );
    }

    #[test]
    fn missing_workdir_maps_to_failed_without_fallback() {
        let executor = ClaudeExecutor::bind(
            ClaudeConfig::default(),
            "prompt",
            PathBuf::from("/no/such/workdir-anywhere"),
            test_environment_base(),
        )
        .unwrap();
        let err = executor
            .execute(&work())
            .expect_err("missing workdir must fail");
        match err {
            ExecutionProblem::Failed { detail } => {
                assert!(detail.contains("workspace_binding"), "got {detail:?}");
            }
            other => panic!("missing workdir must be Failed, got {other:?}"),
        }
    }

    #[test]
    fn overloaded_stderr_maps_to_transient_never_success() {
        // F5 RED: overload prose on the exit path must classify as
        // Transient (distinguishable), never success, never plain Failed.
        let executor = bind_default("do work");
        let err = executor
            .interpret_exit(Some(1), b"", b"Error: model overloaded, try again later")
            .expect_err("non-zero exit must fail");
        assert!(
            matches!(err, crate::failure::ClaudeFailure::Transient { .. }),
            "overload must be Transient, got {err:?}"
        );
    }

    #[test]
    fn truncation_is_explicit_and_bounded() {
        assert_eq!(truncate("  padded  ", 100), "padded");
        assert_eq!(truncate("   ", 100), "(empty provider result)");
        let long = "z".repeat(2000);
        let out = truncate(&long, 100);
        assert!(out.ends_with("…[truncated]"));
        assert!(out.chars().count() <= 100 + "…[truncated]".chars().count());
    }
}
