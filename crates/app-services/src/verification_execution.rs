//! Verification-plan execution through the G6 runner boundary
//! (TASK-0162): runner-adapter consumer of [`VerificationPlan`].
//!
//! Every command must exactly equal a [`derive_commands`] derivation
//! for the plan's (ecosystem, level) — anything else (including a
//! hand-built plan smuggling `sh -c`) is refused before any process
//! exists. Execution is argv-only (never a shell), stdout/stderr are
//! captured bounded, and only digests+sizes persist as
//! `ProcessExit` observed effects — raw output bytes never reach the
//! database, the journal, or a typed error, so a secret-shaped output
//! cannot leak through this path by construction.

use crate::verification_levels::{derive_commands, VerificationCommand, VerificationPlan};
use crate::{G6Authorization, G6SecurityService};

/// Why a plan was refused before any process existed. Carries positions
/// and derived counts only — never command bytes beyond the static
/// derivation table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanRefusal {
    NotDerived { position: usize },
}

/// Every plan command must exactly equal a [`derive_commands`]
/// derivation for the plan's (ecosystem, level), in order. Any
/// deviation — smuggled program, appended/swapped arg, command under
/// V0/Unknown — refuses the whole plan before anything executes.
pub fn validate_plan_commands(plan: &VerificationPlan) -> Result<(), PlanRefusal> {
    let derived = derive_commands(plan.ecosystem, plan.level);
    if plan.commands.len() != derived.len() {
        return Err(PlanRefusal::NotDerived {
            position: plan.commands.len().min(derived.len()),
        });
    }
    for (position, (actual, expected)) in plan.commands.iter().zip(derived.iter()).enumerate() {
        if !commands_equal(actual, expected) {
            return Err(PlanRefusal::NotDerived { position });
        }
    }
    Ok(())
}

fn commands_equal(actual: &VerificationCommand, expected: &VerificationCommand) -> bool {
    actual.program == expected.program && actual.args == expected.args
}

/// Default wall-clock budget per verification command.
pub const DEFAULT_VERIFICATION_TIMEOUT_SECS: u64 = 600;
/// Default retained prefix per output stream. Digests cover exactly the
/// retained bytes; excess output is drained and dropped, never stored.
pub const DEFAULT_STREAM_CAP_BYTES: u64 = 65536;
/// Provenance identity stamped on verification-runner effects.
pub const VERIFICATION_RUNNER_REF: &str = "runlith.verification-runner.v1";

/// Owned inputs to one plan execution.
#[derive(Debug)]
pub struct VerificationExecutionRequest {
    pub plan: VerificationPlan,
    pub task_id: String,
    pub execution_id: agent_runtime::ExecutionId,
    pub attempt_id: agent_runtime::AttemptId,
    pub workspace: std::path::PathBuf,
    pub captured_at: String,
    pub timeout: std::time::Duration,
    pub per_stream_cap: u64,
}

/// One executed command plus its recorded `ProcessExit` effect.
#[derive(Debug)]
pub struct VerifiedCommand {
    pub position: usize,
    pub effect_id: evaluation::EffectId,
}

/// Typed execution failures. Variants carry positions, exit codes, and
/// static reasons only — never command output bytes, so a secret-shaped
/// output cannot leak through an error, a log line, or a journal fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationExecutionError {
    PlanRefused(PlanRefusal),
    ExecutableUnresolved,
    InvalidWorkspace,
    AuthorizationFailed,
    LaunchRefused,
    SpawnFailed,
    Timeout,
    NonZeroExit { code: Option<i32> },
    StoreFailed,
}

impl std::fmt::Display for VerificationExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PlanRefused(refusal) => {
                write!(f, "verification plan refused: {refusal:?}")
            }
            Self::ExecutableUnresolved => write!(f, "verification executable unresolved"),
            Self::InvalidWorkspace => write!(f, "verification workspace invalid"),
            Self::AuthorizationFailed => write!(f, "verification authorization failed"),
            Self::LaunchRefused => write!(f, "runner refused verification launch"),
            Self::SpawnFailed => write!(f, "verification spawn failed"),
            Self::Timeout => write!(f, "verification command timed out"),
            Self::NonZeroExit { code } => {
                write!(f, "verification command failed with exit {code:?}")
            }
            Self::StoreFailed => write!(f, "verification evidence store failed"),
        }
    }
}

impl std::error::Error for VerificationExecutionError {}

/// Executes verification plans through the G6 runner boundary.
/// The backend and isolation requirements are injected: callers on a
/// sandboxing backend pass strict requirements, callers on the native
/// trusted-host backend pass [`runner::IsolationRequirements::trusted_host`]
/// (exact process authorization still mandatory, ambient environment
/// still cleared). No network capability is ever granted.
/// Program-name resolver injected for testability; production uses
/// [`default_resolve_verification_program`].
pub type ExecutableResolver =
    std::sync::Arc<dyn Fn(&str) -> Result<std::path::PathBuf, String> + Send + Sync>;

pub struct VerificationRunner<C> {
    store: storage_sqlite::SqliteStore,
    security: G6SecurityService<C>,
    backend: std::sync::Arc<dyn runner::IsolationBackend>,
    requirements: runner::IsolationRequirements,
    resolve_executable: ExecutableResolver,
}

impl<C: crate::Clock + Clone> VerificationRunner<C> {
    pub fn new(
        store: storage_sqlite::SqliteStore,
        security: G6SecurityService<C>,
        backend: std::sync::Arc<dyn runner::IsolationBackend>,
        requirements: runner::IsolationRequirements,
        resolve_executable: ExecutableResolver,
    ) -> Self {
        Self {
            store,
            security,
            backend,
            requirements,
            resolve_executable,
        }
    }

    /// Executes every plan command in order, fail-fast: the first
    /// non-executing or failing command returns its typed error. A
    /// command that ran always records its `ProcessExit` effect first —
    /// including nonzero exits and timeouts (killed runs record a
    /// signal exit) — so evidence is complete for whatever executed.
    /// Pre-execution failures record nothing.
    pub fn execute(
        &mut self,
        request: VerificationExecutionRequest,
    ) -> Result<Vec<VerifiedCommand>, VerificationExecutionError> {
        validate_plan_commands(&request.plan).map_err(VerificationExecutionError::PlanRefused)?;
        let workspace = canonical_workspace(&request.workspace)?;
        let mut verified = Vec::with_capacity(request.plan.commands.len());
        for (position, command) in request.plan.commands.iter().enumerate() {
            let effect_id = self.execute_command(&request, &workspace, position, command)?;
            verified.push(VerifiedCommand {
                position,
                effect_id,
            });
        }
        Ok(verified)
    }

    fn execute_command(
        &mut self,
        request: &VerificationExecutionRequest,
        workspace: &str,
        position: usize,
        command: &VerificationCommand,
    ) -> Result<evaluation::EffectId, VerificationExecutionError> {
        let executable = (self.resolve_executable)(&command.program)
            .map_err(|_| VerificationExecutionError::ExecutableUnresolved)?;
        let executable = executable
            .canonicalize()
            .map_err(|_| VerificationExecutionError::ExecutableUnresolved)?;
        if !executable.is_file() {
            return Err(VerificationExecutionError::ExecutableUnresolved);
        }
        let executable = executable
            .to_str()
            .ok_or(VerificationExecutionError::ExecutableUnresolved)?;
        let launch = self.authorize_command(request, workspace, position, executable)?;
        let spec = runner::CommandSpec::new(
            std::ffi::OsString::from(executable),
            command.args.iter().map(std::ffi::OsString::from),
        );
        let prepared = runner::prepare_process_launch(
            std::sync::Arc::clone(&self.backend),
            &launch.effective,
            workspace.to_owned(),
            launch.executable_scope,
            self.requirements,
            runner::ChildEnvironment::empty(),
        )
        .map_err(|_| VerificationExecutionError::LaunchRefused)?;
        let bound = prepared
            .bind(spec)
            .map_err(|_| VerificationExecutionError::LaunchRefused)?;
        let workspace_path = std::path::PathBuf::from(workspace);
        let mut child = bound
            .spawn(|wire| {
                wire.current_dir(&workspace_path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
            })
            .map_err(|_| VerificationExecutionError::SpawnFailed)?;
        let outcome = wait_bounded(&mut child, request.timeout, request.per_stream_cap);
        let timed_out = matches!(outcome.status, ChildStatus::TimedOut);
        let exit_code = match outcome.status {
            ChildStatus::Exited(status) => status.code(),
            ChildStatus::TimedOut => None,
        };
        let subject = format!(
            "verification:{}:{position}:{}",
            request.plan.level.as_str(),
            command.program
        );
        let effect = self.record_effect(request, &subject, command, exit_code, &outcome)?;
        if timed_out {
            return Err(VerificationExecutionError::Timeout);
        }
        match exit_code {
            Some(0) => Ok(effect),
            _ => Err(VerificationExecutionError::NonZeroExit { code: exit_code }),
        }
    }

    fn authorize_command(
        &mut self,
        request: &VerificationExecutionRequest,
        workspace: &str,
        position: usize,
        executable: &str,
    ) -> Result<AuthorizedLaunch, VerificationExecutionError> {
        let executable_scope = policy::ScopeId::new(executable.to_owned())
            .map_err(|_| VerificationExecutionError::ExecutableUnresolved)?;
        let capabilities = vec![
            policy::Capability::FilesystemRead(
                policy::FileScope::new(workspace, ".")
                    .map_err(|_| VerificationExecutionError::InvalidWorkspace)?,
            ),
            policy::Capability::FilesystemWrite(
                policy::FileScope::new(workspace, ".")
                    .map_err(|_| VerificationExecutionError::InvalidWorkspace)?,
            ),
            policy::Capability::ProcessExecute(executable_scope.clone()),
        ];
        let policy_request = policy::PolicyRequest::new(
            format!(
                "verification.{}:{}:{}:{}:{position}",
                request.plan.level.as_str(),
                request.task_id,
                request.execution_id.as_str(),
                request.attempt_id.as_str(),
            ),
            "verification-runner",
            policy::RequestOrigin::new(
                request.task_id.clone(),
                Some(request.execution_id.as_str().to_owned()),
                Some(request.attempt_id.as_str().to_owned()),
            )
            .map_err(|_| VerificationExecutionError::AuthorizationFailed)?,
            policy::RequestedCapabilities::new(capabilities.clone()),
        )
        .map_err(|_| VerificationExecutionError::AuthorizationFailed)?;
        let rules = capabilities
            .iter()
            .enumerate()
            .map(|(index, capability)| {
                policy::PolicyRule::new(
                    format!("builtin.verification.{index}"),
                    "trusted_verification_runner_operation",
                    policy::PolicyLayer::BuiltInHard,
                    policy::PolicyOutcome::Allow,
                    capability.clone(),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| VerificationExecutionError::AuthorizationFailed)?;
        let policy = policy::CompiledPolicy::compile(rules)
            .map_err(|_| VerificationExecutionError::AuthorizationFailed)?;
        let parent = policy::EffectiveCapabilities::new(capabilities);
        match self
            .security
            .authorize(&policy_request, &parent, &policy, u64::MAX)
            .map_err(|_| VerificationExecutionError::AuthorizationFailed)?
        {
            G6Authorization::Authorized { effective, .. } => Ok(AuthorizedLaunch {
                effective,
                executable_scope,
            }),
            _ => Err(VerificationExecutionError::AuthorizationFailed),
        }
    }

    fn record_effect(
        &mut self,
        request: &VerificationExecutionRequest,
        subject: &str,
        command: &VerificationCommand,
        exit_code: Option<i32>,
        outcome: &ChildOutcome,
    ) -> Result<evaluation::EffectId, VerificationExecutionError> {
        let stdout_digest = digest_bytes(&outcome.stdout);
        let stderr_digest = digest_bytes(&outcome.stderr);
        let command_digest =
            digest_text(&format!("{} {}", command.program, command.args.join(" ")));
        let payload = evaluation::EffectPayload::ProcessExit {
            exit_code,
            stdout_bytes: outcome.stdout.len() as u64,
            stdout_sha256: evaluation::Sha256Digest::new(stdout_digest.clone())
                .map_err(|_| VerificationExecutionError::StoreFailed)?,
            stderr_bytes: outcome.stderr.len() as u64,
            stderr_sha256: evaluation::Sha256Digest::new(stderr_digest.clone())
                .map_err(|_| VerificationExecutionError::StoreFailed)?,
        };
        let subject_ref = evaluation::EffectSubjectRef::new(subject)
            .map_err(|_| VerificationExecutionError::StoreFailed)?;
        let fingerprint = evaluation::subject_fingerprint(
            evaluation::EffectKind::ProcessExit,
            subject,
            &[
                ("command", command_digest.as_str()),
                ("stdout", stdout_digest.as_str()),
                ("stderr", stderr_digest.as_str()),
            ],
        )
        .map_err(|_| VerificationExecutionError::StoreFailed)?;
        let provenance = evaluation::EffectProvenance::new(
            request.execution_id.clone(),
            request.attempt_id.clone(),
            VERIFICATION_RUNNER_REF,
        )
        .map_err(|_| VerificationExecutionError::StoreFailed)?;
        let observed = self
            .store
            .append_observed_effect(
                subject_ref,
                fingerprint,
                payload,
                request.captured_at.clone(),
                provenance,
            )
            .map_err(|_| VerificationExecutionError::StoreFailed)?;
        Ok(observed.effect_id().clone())
    }
}

struct AuthorizedLaunch {
    effective: policy::EffectiveCapabilities,
    executable_scope: policy::ScopeId,
}

/// Production executable resolver: bounded program name, `PATH` lookup,
/// canonicalized file. Anything else fails closed.
pub fn default_resolve_verification_program(program: &str) -> Result<std::path::PathBuf, String> {
    if program.is_empty()
        || program.len() > 256
        || program.contains('/')
        || program.contains('\\')
        || program.contains('\0')
    {
        return Err("verification program name is invalid".to_owned());
    }
    let path_var = std::env::var_os("PATH").ok_or_else(|| "PATH is unavailable".to_owned())?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(program);
        if let Ok(canonical) = candidate.canonicalize() {
            if canonical.is_file() {
                return Ok(canonical);
            }
        }
    }
    Err("verification program not found on PATH".to_owned())
}

fn canonical_workspace(workspace: &std::path::Path) -> Result<String, VerificationExecutionError> {
    let canonical = workspace
        .canonicalize()
        .map_err(|_| VerificationExecutionError::InvalidWorkspace)?;
    if !canonical.is_absolute() || !canonical.is_dir() {
        return Err(VerificationExecutionError::InvalidWorkspace);
    }
    canonical
        .to_str()
        .map(str::to_owned)
        .ok_or(VerificationExecutionError::InvalidWorkspace)
}

#[derive(Debug)]
enum ChildStatus {
    Exited(std::process::ExitStatus),
    TimedOut,
}

struct ChildOutcome {
    status: ChildStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Waits for the child up to `timeout`, draining both pipes on
/// detached threads (sequential reads could deadlock once a pipe
/// fills) while retaining only the first `cap` bytes per stream. On
/// timeout the child is killed and reaped; the drain threads are NOT
/// joined because orphaned grandchildren may hold the pipes open —
/// output collected within a short grace still lands in the
/// signal-exit evidence record, the rest is dropped.
fn wait_bounded(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
    cap: u64,
) -> ChildOutcome {
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel();
    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = stdout_tx.send(drain_capped(&mut stdout, cap));
    });
    std::thread::spawn(move || {
        let _ = stderr_tx.send(drain_capped(&mut stderr, cap));
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break ChildStatus::Exited(status),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break ChildStatus::TimedOut;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break ChildStatus::TimedOut;
            }
        }
    };
    // Exited children close their pipes, so the drains always finish;
    // killed children may leave orphaned grandchildren holding them,
    // so the timeout path only grants a short collection grace.
    let grace = std::time::Duration::from_millis(500);
    let stdout = match status {
        ChildStatus::Exited(_) => stdout_rx.recv().unwrap_or_default(),
        ChildStatus::TimedOut => stdout_rx.recv_timeout(grace).unwrap_or_default(),
    };
    let stderr = match status {
        ChildStatus::Exited(_) => stderr_rx.recv().unwrap_or_default(),
        ChildStatus::TimedOut => stderr_rx.recv_timeout(grace).unwrap_or_default(),
    };
    ChildOutcome {
        status,
        stdout,
        stderr,
    }
}

fn drain_capped<T: std::io::Read>(pipe: &mut Option<T>, cap: u64) -> Vec<u8> {
    let Some(reader) = pipe else {
        return Vec::new();
    };
    let mut retained = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let room = cap.saturating_sub(retained.len() as u64) as usize;
                retained.extend_from_slice(&chunk[..read.min(room)]);
            }
            Err(_) => break,
        }
    }
    retained
}

fn digest_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn digest_text(text: &str) -> String {
    digest_bytes(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verification_levels::{derive_commands, Ecosystem, VerificationLevel};

    fn plan_with(
        ecosystem: Ecosystem,
        level: VerificationLevel,
        commands: Vec<VerificationCommand>,
    ) -> VerificationPlan {
        VerificationPlan {
            level,
            ecosystem,
            scope: vec!["src/main.rs".to_owned()],
            commands,
        }
    }

    #[test]
    fn derived_commands_validate() {
        for level in [
            VerificationLevel::V1,
            VerificationLevel::V2,
            VerificationLevel::V3,
        ] {
            let commands = derive_commands(Ecosystem::Cargo, level);
            assert!(!commands.is_empty());
            validate_plan_commands(&plan_with(Ecosystem::Cargo, level, commands)).unwrap();
        }
        validate_plan_commands(&plan_with(Ecosystem::Cargo, VerificationLevel::V0, vec![]))
            .unwrap();
    }

    #[test]
    fn non_derived_commands_refuse() {
        let smuggled = VerificationCommand {
            program: "sh".to_owned(),
            args: vec!["-c".to_owned(), "evil".to_owned()],
        };
        assert!(matches!(
            validate_plan_commands(&plan_with(
                Ecosystem::Cargo,
                VerificationLevel::V2,
                vec![smuggled]
            )),
            Err(PlanRefusal::NotDerived { .. })
        ));
        // Exact argv: appended or swapped args refuse even for the right program.
        let tampered = VerificationCommand {
            program: "cargo".to_owned(),
            args: vec![
                "test".to_owned(),
                "--locked".to_owned(),
                "--".to_owned(),
                "evil".to_owned(),
            ],
        };
        assert!(matches!(
            validate_plan_commands(&plan_with(
                Ecosystem::Cargo,
                VerificationLevel::V2,
                vec![tampered]
            )),
            Err(PlanRefusal::NotDerived { .. })
        ));
        // V0 and unknown ecosystems derive nothing: any command refuses.
        let any = VerificationCommand {
            program: "cargo".to_owned(),
            args: vec!["check".to_owned()],
        };
        assert!(validate_plan_commands(&plan_with(
            Ecosystem::Cargo,
            VerificationLevel::V0,
            vec![any.clone()]
        ))
        .is_err());
        assert!(validate_plan_commands(&plan_with(
            Ecosystem::Unknown,
            VerificationLevel::V2,
            vec![any]
        ))
        .is_err());
    }
}
