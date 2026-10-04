//! Real child-process ownership for the Claude CLI (TASK-0046, G5).
//!
//! Makes OS process state explicit: exactly which child belongs to which
//! execution, whether it is still alive, how it concluded, and whether it
//! was cancelled. Cancellation kills only the owned child handle — never
//! `pkill`/`killall` or any process-name matching — and an already-exited
//! child is handled safely.
//!
//! Capture is deadlock-free and bounded: stdout/stderr drain on dedicated
//! threads into `take(bound + 1)` buffers while the wait loop polls, so a
//! chatty child can neither block on a full pipe nor grow memory without
//! limit. PIDs are never durable authority: the live [`RunningChild`]
//! handle owns the process, and completion is observed through `wait`,
//! not through PID reuse-prone bookkeeping.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::Path;
#[cfg(any(test, feature = "trusted-host"))]
use std::process::Command;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::config::bounded_detail;
use crate::environment::ProviderEnvironment;
use crate::failure::ClaudeFailure;
use runner::{ChildSecretEnvironment, CommandSpec, SandboxedLaunch, SpawnWiring};

/// Poll quantum for the wait loop. Correctness never depends on its
/// value: exit, cancellation, and deadline are all level-checked every
/// iteration; this only bounds wake-up latency.
const WAIT_POLL_QUANTUM: Duration = Duration::from_millis(5);

/// How a child process concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildOutcome {
    /// Exited on its own: exit code plus bounded captured streams.
    Exited {
        /// Process exit code (`None` = terminated by signal).
        code: Option<i32>,
        /// Bounded stdout bytes.
        stdout: Vec<u8>,
        /// Bounded stderr bytes.
        stderr: Vec<u8>,
    },
    /// Killed after the configured deadline.
    TimedOut,
    /// Killed through the canonical cancellation path.
    Cancelled,
}

/// Shared cancellation flag for one execution. Cloned into the executor
/// and into any thread allowed to cancel; idempotent and thread-safe.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag {
    flag: Arc<AtomicBool>,
}

impl CancelFlag {
    /// Fresh unset flag.
    #[must_use]
    pub fn fresh() -> Self {
        Self::default()
    }

    /// Requests cancellation. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Spawns `<executable> <argv>` with `workdir` as its working directory
/// and supervises it to exactly one [`ChildOutcome`].
///
/// - `executable` + `argv` travel as direct argv (no shell, ever).
/// - `stdin` is closed (`null`); the prompt travels as argv.
/// - Environment is cleared and rebuilt from [`ProviderEnvironment`].
///   Provider-owned authentication keeps only the explicit home/profile
///   anchor; ambient credentials and provider/plugin variables do not pass.
/// - `workdir` must exist as a directory or spawning refuses with
///   `WorkspaceBinding` — the provider never falls back to another
///   directory (G4 isolation preserved).
/// - `cancel` is polled every quantum; `deadline` bounds total runtime.
/// - The child is always reaped exactly once; kills target only the
///   owned child tree (unix process group), never by name or PID scan.
/// - Bounded capture finalization (TASK-0047): on the natural-exit path
///   the drain threads get `capture_grace` past the reap to observe pipe
///   EOF; a double-forked daemon grandchild outliving the CLI while
///   holding an inherited pipe then degrades to truncated tail output,
///   never to an unbounded join. The timeout/cancel paths kill the tree
///   first, so their joins stay prompt without a grace.
///
/// How one provider invocation reaches its child process (TASK-SEC-R1 R1-H1).
///
/// The production composition always attaches [`Self::Sandboxed`]: the child
/// can then only be spawned through the Runner backend's verified recipe, so
/// forgetting a handoff step cannot silently produce an unmanaged child. The
/// unmanaged supervisor state is a separate, explicitly named choice used by
/// the provider's own contract tests and trusted-host development runs; an
/// unattached executor refuses to spawn (see [`Self::Unattached`]).
#[derive(Clone, Default)]
pub(crate) enum ProviderLaunch {
    /// No Runner launch attached. Spawning refuses: the executor cannot prove
    /// where its child would run.
    #[default]
    Unattached,
    /// Explicit trusted-host supervisor: the provider spawns the configured
    /// executable directly with its own allowlisted environment. This is the
    /// test/development path and never the production G6 path; compiled
    /// only for tests or the `trusted-host` feature (audit F6).
    #[cfg(any(test, feature = "trusted-host"))]
    TrustedHost,
    /// Verified Runner launch: the only spawnable child is the one the backend
    /// installed restrictions for.
    Sandboxed {
        launch: Arc<SandboxedLaunch>,
        secrets: SecretSlot,
    },
}

/// Late-bound secrets released for one verified launch (ADR-0039). The first
/// spawn takes them; a later spawn of the same launch runs without them.
pub(crate) type SecretSlot = Arc<Mutex<Option<ChildSecretEnvironment>>>;

impl ProviderLaunch {
    pub(crate) fn sandboxed(
        launch: Arc<SandboxedLaunch>,
        secrets: Option<ChildSecretEnvironment>,
    ) -> Self {
        Self::Sandboxed {
            launch,
            secrets: Arc::new(Mutex::new(secrets)),
        }
    }
}

pub(crate) struct ProcessInvocation<'a> {
    pub(crate) executable: &'a OsStr,
    pub(crate) argv: &'a [OsString],
    pub(crate) workdir: &'a Path,
    pub(crate) environment: &'a ProviderEnvironment,
    pub(crate) launch: &'a ProviderLaunch,
}

/// Maps one spawn failure onto the adapter taxonomy, keeping the
/// "missing binary" class distinguishable.
fn spawn_failure(error: std::io::Error, executable: &OsStr) -> ClaudeFailure {
    if error.kind() == std::io::ErrorKind::NotFound {
        ClaudeFailure::CliMissing {
            detail: bounded_detail(format!(
                "claude executable not found at spawn: {}",
                executable.to_string_lossy()
            )),
        }
    } else {
        ClaudeFailure::SpawnFailed {
            detail: bounded_detail(format!("claude process spawn failed: {error}")),
        }
    }
}

/// Installs the provider's ordinary process wiring on a child that is already
/// backed by a verified Runner sandbox recipe.
fn wire_sandboxed_child(
    launch: &SandboxedLaunch,
    secrets: &SecretSlot,
    executable: &OsStr,
    argv: &[OsString],
    workdir: &Path,
) -> Result<Child, ClaudeFailure> {
    let spec = CommandSpec::new(executable.to_owned(), argv.iter().cloned());
    let sandboxed = launch
        .bind(spec)
        .map_err(|error| ClaudeFailure::SpawnFailed {
            detail: bounded_detail(format!("runner refused the sandboxed launch: {error}")),
        })?;
    // The child environment travels inside the verified recipe (ambient state
    // cleared, exactly the plan's explicit pairs installed): the provider does
    // not re-apply its own environment on this path.
    let wiring = |wire: &mut SpawnWiring<'_>| {
        wire.current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Own process group (unix): timeout/cancel kills the whole child
        // tree; harmless for normal exits.
        #[cfg(unix)]
        wire.process_group(0);
    };
    // Taken, not copied: the secrets are zeroized when this spawn returns.
    let secrets = secrets
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    match &secrets {
        Some(secrets) => sandboxed.spawn_with_secrets(secrets, wiring),
        None => sandboxed.spawn(wiring),
    }
    .map_err(|error| spawn_failure(error, executable))
}

pub(crate) fn spawn_and_supervise(
    invocation: ProcessInvocation<'_>,
    cancel: &CancelFlag,
    deadline: Duration,
    max_stream_bytes: usize,
    capture_grace: Duration,
) -> Result<ChildOutcome, ClaudeFailure> {
    #[cfg_attr(not(any(test, feature = "trusted-host")), allow(unused_variables))]
    let ProcessInvocation {
        executable,
        argv,
        workdir,
        environment,
        launch,
    } = invocation;
    if !workdir.is_dir() {
        return Err(ClaudeFailure::WorkspaceBinding {
            detail: bounded_detail(format!(
                "claude workspace binding unusable (missing directory): {}",
                workdir.display()
            )),
        });
    }
    // Already cancelled before spawn: report without spawning a child at
    // all (TASK-0049 H8). Structural failures above still beat the cancel
    // check; there is simply no child to cancel here.
    if cancel.is_cancelled() {
        return Ok(ChildOutcome::Cancelled);
    }
    let mut child: Child = match launch {
        ProviderLaunch::Sandboxed { launch, secrets } => {
            wire_sandboxed_child(launch, secrets, executable, argv, workdir)?
        }
        ProviderLaunch::Unattached => {
            return Err(ClaudeFailure::LaunchUnmanaged {
                detail: bounded_detail(
                    "no Runner sandbox is attached to this provider execution; refusing to \
                     spawn an unmanaged child"
                        .to_owned(),
                ),
            });
        }
        #[cfg(any(test, feature = "trusted-host"))]
        ProviderLaunch::TrustedHost => {
            // Explicit trusted-host supervisor (provider contract tests and
            // trusted-host development runs). No policy authority exists to
            // present here, so no Runner plan can be built; the environment is
            // still cleared and rebuilt from the allowlisted provider set.
            let mut command = Command::new(executable);
            command.args(argv);
            command
                .current_dir(workdir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            environment.apply(&mut command);
            // Own process group (unix): timeout/cancel kills the whole child
            // tree (see kill_tree_and_reap), so forked grandchildren can neither
            // outlive supervision nor hold our capture pipes open.
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            command
                .spawn()
                .map_err(|error| spawn_failure(error, executable))?
        }
    };

    // Drain threads: bounded shared buffers so a chatty child can
    // neither deadlock on a full pipe nor exceed the capture bound. The
    // +1 byte detects over-bound output for an explicit refusal.
    let (stdout_buf, stdout_handle) = drain_thread(child.stdout.take(), max_stream_bytes);
    let (stderr_buf, stderr_handle) = drain_thread(child.stderr.take(), max_stream_bytes);

    let start = Instant::now();
    let outcome = loop {
        if cancel.is_cancelled() {
            kill_tree_and_reap(&mut child);
            // Drain threads joined (not detached): the tree kill closed
            // every pipe end the child tree held, so joining is prompt.
            join_drain(stdout_buf, stdout_handle);
            join_drain(stderr_buf, stderr_handle);
            break ChildOutcome::Cancelled;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // Natural exit: the child is reaped, but a double-forked
                // daemon grandchild may still hold an inherited pipe end.
                // Finalize with a bounded grace instead of an unbounded
                // join: provider plumbing must never block daemon
                // reconciliation indefinitely.
                let (stdout, stderr) = join_drains_graceful(
                    (stdout_buf, stdout_handle),
                    (stderr_buf, stderr_handle),
                    capture_grace,
                );
                break ChildOutcome::Exited {
                    code: status.code(),
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if start.elapsed() > deadline {
                    kill_tree_and_reap(&mut child);
                    // Drain threads still joined to avoid leaking them;
                    // their bytes are discarded on timeout.
                    join_drain(stdout_buf, stdout_handle);
                    join_drain(stderr_buf, stderr_handle);
                    break ChildOutcome::TimedOut;
                }
                std::thread::sleep(WAIT_POLL_QUANTUM);
            }
            Err(error) => {
                kill_tree_and_reap(&mut child);
                join_drain(stdout_buf, stdout_handle);
                join_drain(stderr_buf, stderr_handle);
                return Err(ClaudeFailure::SpawnFailed {
                    detail: bounded_detail(format!("claude process wait failed: {error}")),
                });
            }
        }
    };
    Ok(outcome)
}

/// Best-effort kill of the owned child *tree*, then reap. Sequence (F7):
///
/// 1. `SIGTERM` the child's process group (unix), giving a well-behaved
///    CLI a bounded grace to flush and exit instead of dying mid-write.
/// 2. Poll briefly; a child that exits on SIGTERM is reaped as-is, so its
///    real exit disposition (not a kill we inflicted) is what supervision
///    observes.
/// 3. Past the grace, `kill` the direct child through its handle (exact,
///    never by name) and `SIGKILL` the group, then `wait` to reap.
/// 4. Signal the child's process group once more after the reap (narrowing
///    the PID-reuse window to the microseconds between our child's death
///    and the group signal) so forked grandchildren — which otherwise
///    survive as orphans holding our stdout/stderr pipes open and stalling
///    capture — die too.
///
/// Kill errors (already-exited races) are ignored: `wait` observes the
/// truth. Never `pkill`/`killall`/name matching on any path.
fn kill_tree_and_reap(child: &mut Child) {
    let pid = child.id();
    terminate_gracefully(child, pid);
    let _ = child.kill();
    let _ = child.wait();
    signal_process_group(pid, SIGKILL);
}

/// F7 first stage (unix): SIGTERM the group, then allow up to TERM_GRACE
/// for the child to exit on its own. Non-unix: no signal API — the
/// SIGKILL in [`kill_tree_and_reap`] is the whole tree kill.
#[cfg(unix)]
fn terminate_gracefully(child: &mut Child, group_id: u32) {
    signal_process_group(group_id, SIGTERM);
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => std::thread::sleep(WAIT_POLL_QUANTUM),
        }
    }
}

/// Grace period between SIGTERM and SIGKILL escalation: long enough for a
/// well-behaved CLI to flush and exit, bounded so cancel/timeout paths
/// stay prompt even when the child ignores SIGTERM.
#[cfg(unix)]
const TERM_GRACE: Duration = Duration::from_secs(2);

/// Signals the process group `group_id` (a group-leader PID from our own
/// spawn) with `sig`. Unix only: on other platforms the direct
/// `kill` + `wait` is the whole tree kill.
#[cfg(unix)]
fn signal_process_group(group_id: u32, sig: i32) {
    // Direct libc binding without a new dependency: libc is always
    // linked on unix targets, and this is the only symbol needed.
    extern "C" {
        fn killpg(pgrp: i32, sig: i32) -> i32;
    }
    let _ = unsafe { killpg(group_id as i32, sig) };
}

#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(unix)]
const SIGTERM: i32 = 15;

#[cfg(not(unix))]
fn signal_process_group(_group_id: u32, _sig: i32) {}

#[cfg(not(unix))]
fn terminate_gracefully(_child: &mut Child, _group_id: u32) {}

#[cfg(not(unix))]
const SIGKILL: i32 = 9;

/// Shared capture buffer: the drain thread appends bounded chunks here,
/// so the supervisor can finalize with the bytes collected so far even
/// when the thread itself is still blocked on a pipe held open by a
/// surviving grandchild.
type SharedCapture = std::sync::Arc<std::sync::Mutex<Vec<u8>>>;

fn drain_thread(
    pipe: Option<impl Read + Send + 'static>,
    bound: usize,
) -> (SharedCapture, Option<std::thread::JoinHandle<()>>) {
    let shared: SharedCapture = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let Some(pipe) = pipe else {
        return (shared, None);
    };
    let target = std::sync::Arc::clone(&shared);
    let handle = std::thread::spawn(move || {
        // Chunked reads into the shared buffer: bounded overall, and
        // every chunk is visible to the supervisor before EOF.
        let mut pipe = pipe.take((bound as u64).saturating_add(1));
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut buf) = target.lock() {
                        let room = bound.saturating_add(1).saturating_sub(buf.len());
                        buf.extend_from_slice(&chunk[..n.min(room)]);
                        if buf.len() > bound {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
    (shared, Some(handle))
}

fn take_shared(shared: &SharedCapture) -> Vec<u8> {
    shared.lock().map(|buf| buf.clone()).unwrap_or_default()
}

/// Blocking join for paths where the whole child tree is already dead
/// (kill paths): every pipe end is closed, so EOF is prompt.
fn join_drain(shared: SharedCapture, handle: Option<std::thread::JoinHandle<()>>) -> Vec<u8> {
    if let Some(handle) = handle {
        let _ = handle.join();
    }
    take_shared(&shared)
}

/// Bounded joint join for the natural-exit path: waits up to ONE `grace`
/// total for BOTH drain threads to observe EOF, then finalizes with the
/// bytes collected so far. A still-blocked thread is detached (its handle
/// dropped) — it exits on its own when the surviving pipe finally closes,
/// and it never blocks execution finalization.
///
/// Single deadline, not per-stream (TASK-0049 H3): the drains already run
/// concurrently on dedicated threads, so sequential per-stream graces
/// would double the bound for no reason. Total natural-exit
/// finalization is bounded by `grace` plus spawn/scheduling overhead —
/// never by a surviving grandchild's lifetime.
fn join_drains_graceful(
    stdout_drain: (SharedCapture, Option<std::thread::JoinHandle<()>>),
    stderr_drain: (SharedCapture, Option<std::thread::JoinHandle<()>>),
    grace: Duration,
) -> (Vec<u8>, Vec<u8>) {
    let deadline = Instant::now() + grace;
    loop {
        let stdout_done = stdout_drain
            .1
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished);
        let stderr_done = stderr_drain
            .1
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished);
        if (stdout_done && stderr_done) || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(WAIT_POLL_QUANTUM);
    }
    (take_drain(stdout_drain), take_drain(stderr_drain))
}

/// Takes one drain's bytes: joins a finished thread promptly, detaches a
/// still-blocked one (dropping its handle leaks no lock and blocks
/// nothing — the thread owns only its pipe end and the shared buffer).
fn take_drain((shared, handle): (SharedCapture, Option<std::thread::JoinHandle<()>>)) -> Vec<u8> {
    if let Some(handle) = handle {
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            drop(handle);
        }
    }
    take_shared(&shared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ENV_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_environment() -> ProviderEnvironment {
        let workdir = std::env::current_dir().expect("test current dir");
        let id = ENV_COUNTER.fetch_add(1, Ordering::SeqCst);
        let base =
            std::env::temp_dir().join(format!("runlith-process-env-{}-{id}", std::process::id()));
        ProviderEnvironment::prepare(&workdir, &base).expect("test provider environment")
    }

    #[test]
    fn cancel_flag_is_idempotent() {
        let flag = CancelFlag::fresh();
        assert!(!flag.is_cancelled());
        flag.cancel();
        flag.cancel();
        assert!(flag.is_cancelled());
        assert!(flag.clone().is_cancelled());
    }

    /// F7 (unix): a child that traps SIGTERM and exits promptly must be
    /// reaped with its own exit disposition — proof the SIGTERM grace ran
    /// instead of an immediate SIGKILL (which would report signal 9, never
    /// code 42). A child ignoring SIGTERM still dies past the grace; that
    /// path is exercised by the timeout keepers, not duplicated here.
    #[cfg(unix)]
    #[test]
    fn sigterm_grace_reaps_a_cooperative_child_unescalated() {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};
        let mut command = Command::new("sh");
        command
            .args(["-c", "trap 'exit 42' TERM; sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        let mut child = command.spawn().expect("spawn TERM-trapping sleep");
        // Let sh install its SIGTERM trap before signalling: a TERM that
        // lands during interpreter startup kills with default disposition
        // and would prove nothing about the grace path.
        std::thread::sleep(std::time::Duration::from_secs(1));
        kill_tree_and_reap(&mut child);
        let status = child.try_wait().expect("reaped child reports status");
        assert_eq!(
            status.and_then(|status| status.code()),
            Some(42),
            "cooperative child must exit 42 on SIGTERM, not die of SIGKILL"
        );
    }

    /// Host backend for the secret-slot test: no OS restriction, explicit
    /// environment only (the shape of `runner-native`).
    struct HostBackend;

    impl runner::IsolationBackend for HostBackend {
        fn identity(&self) -> runner::BackendIdentity {
            runner::BackendIdentity {
                name: "provider-claude-test-host",
                runner: runner::RunnerKind::Native,
                platform: runner::Platform::current(),
            }
        }

        fn enforce(
            &self,
            plan: &runner::IsolationPlan,
        ) -> Result<runner::EnforcementReport, runner::RunnerError> {
            Ok(runner::EnforcementReport::new(
                self.identity(),
                plan.clone(),
                runner::EnforcementAxes::none(),
                true,
            ))
        }

        fn sandbox(
            &self,
            plan: &runner::IsolationPlan,
            spec: &CommandSpec,
        ) -> Result<runner::LaunchRecipe, runner::RunnerError> {
            Ok(runner::LaunchRecipe::new(
                spec.program(),
                spec.args().iter().cloned(),
                plan.environment().vars().to_vec(),
            ))
        }
    }

    fn host_launch(workdir: &Path, executable: &str) -> Arc<SandboxedLaunch> {
        let scope = policy::ScopeId::new(executable).unwrap();
        Arc::new(
            runner::prepare_process_launch(
                Arc::new(HostBackend),
                &policy::EffectiveCapabilities::new(vec![policy::Capability::ProcessExecute(
                    scope.clone(),
                )]),
                workdir.to_string_lossy().into_owned(),
                scope,
                runner::IsolationRequirements::trusted_host(),
                runner::ChildEnvironment::empty(),
            )
            .expect("host launch"),
        )
    }

    /// ADR-0039 P5d: the released credential reaches exactly the first child
    /// of a verified launch; a second spawn of the same launch runs without it.
    #[test]
    fn launch_secret_reaches_only_the_first_spawn() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let mut secrets = ChildSecretEnvironment::new();
        secrets
            .insert(crate::OAUTH_TOKEN_ENV, b"p5d-slot-test-value".to_vec())
            .unwrap();
        let launch = ProviderLaunch::sandboxed(host_launch(&dir, "/bin/sh"), Some(secrets));
        let argv = [
            OsString::from("-c"),
            OsString::from(
                "if [ \"$CLAUDE_CODE_OAUTH_TOKEN\" = p5d-slot-test-value ]; then echo present; \
                 else echo absent; fi",
            ),
        ];
        let spawn = || {
            spawn_and_supervise(
                ProcessInvocation {
                    executable: OsStr::new("/bin/sh"),
                    argv: &argv,
                    workdir: &dir,
                    environment: &test_environment(),
                    launch: &launch,
                },
                &CancelFlag::fresh(),
                Duration::from_secs(5),
                1024,
                crate::config::DEFAULT_CAPTURE_GRACE,
            )
        };
        for expected in [&b"present\n"[..], &b"absent\n"[..]] {
            match spawn() {
                Ok(ChildOutcome::Exited {
                    code: Some(0),
                    stdout,
                    ..
                }) => {
                    assert_eq!(stdout, expected);
                }
                other => panic!("unexpected outcome: {other:?}"),
            }
        }
    }

    #[test]
    fn missing_workdir_fails_closed_without_spawning() {
        let outcome = spawn_and_supervise(
            ProcessInvocation {
                executable: OsStr::new("runlith-definitely-not-a-binary-xyz"),
                argv: &[],
                workdir: Path::new("/no/such/workdir-anywhere"),
                environment: &test_environment(),
                launch: &ProviderLaunch::TrustedHost,
            },
            &CancelFlag::fresh(),
            Duration::from_secs(5),
            1024,
            crate::config::DEFAULT_CAPTURE_GRACE,
        );
        assert!(
            matches!(outcome, Err(ClaudeFailure::WorkspaceBinding { .. })),
            "missing workdir must fail before spawn, got {outcome:?}"
        );
    }

    #[test]
    fn missing_executable_is_cli_missing() {
        let dir = std::env::temp_dir();
        let outcome = spawn_and_supervise(
            ProcessInvocation {
                executable: OsStr::new("runlith-definitely-not-a-binary-xyz"),
                argv: &[],
                workdir: &dir,
                environment: &test_environment(),
                launch: &ProviderLaunch::TrustedHost,
            },
            &CancelFlag::fresh(),
            Duration::from_secs(5),
            1024,
            crate::config::DEFAULT_CAPTURE_GRACE,
        );
        assert!(
            matches!(outcome, Err(ClaudeFailure::CliMissing { .. })),
            "missing binary must be CliMissing, got {outcome:?}"
        );
    }

    /// TASK-SEC-R1 R1-H1 (adversarial): an executor with no Runner launch
    /// attached must refuse to spawn, even when the executable exists. The old
    /// handoff could be silently skipped; this state cannot produce a child.
    #[test]
    fn r1_h1_unattached_provider_launch_refuses_to_spawn() {
        let dir = std::env::temp_dir();
        let outcome = spawn_and_supervise(
            ProcessInvocation {
                executable: OsStr::new("/usr/bin/env"),
                argv: &[],
                workdir: &dir,
                environment: &test_environment(),
                launch: &ProviderLaunch::Unattached,
            },
            &CancelFlag::fresh(),
            Duration::from_secs(5),
            1024,
            crate::config::DEFAULT_CAPTURE_GRACE,
        );
        assert!(
            matches!(outcome, Err(ClaudeFailure::LaunchUnmanaged { .. })),
            "unattached launch must refuse to spawn, got {outcome:?}"
        );
    }

    #[test]
    fn preset_cancel_beats_spawn_attempt() {
        // TASK-0049 H8 reorder (explicitly authorized behavior change):
        // an already-cancelled flag reports Cancelled WITHOUT spawning —
        // no child starts just to be killed on the first quantum. A
        // missing binary under a pre-set flag is therefore Cancelled, not
        // CliMissing: the cancellation won before any spawn was attempted,
        // and the missing binary is irrelevant to a run that never started.
        // (Previously this pinned spawn-failure-beats-cancel; the new
        // ordering removes the spawn-then-kill side effect.)
        let dir = std::env::temp_dir();
        let cancel = CancelFlag::fresh();
        cancel.cancel();
        let outcome = spawn_and_supervise(
            ProcessInvocation {
                executable: OsStr::new("runlith-definitely-not-a-binary-xyz"),
                argv: &[],
                workdir: &dir,
                environment: &test_environment(),
                launch: &ProviderLaunch::TrustedHost,
            },
            &cancel,
            Duration::from_secs(5),
            1024,
            crate::config::DEFAULT_CAPTURE_GRACE,
        );
        assert!(
            matches!(outcome, Ok(ChildOutcome::Cancelled)),
            "pre-set cancel must win without spawning, got {outcome:?}"
        );
    }
}
