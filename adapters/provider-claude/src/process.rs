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
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::bounded_detail;
use crate::failure::ClaudeFailure;

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
/// - Environment is inherited unchanged: provider authentication
///   (subscription session, keychain helpers, `PATH`) lives there, and
///   the adapter must not strip it nor inject secrets into it.
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
pub fn spawn_and_supervise(
    executable: &OsStr,
    argv: &[OsString],
    workdir: &Path,
    cancel: &CancelFlag,
    deadline: Duration,
    max_stream_bytes: usize,
    capture_grace: Duration,
) -> Result<ChildOutcome, ClaudeFailure> {
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
    let mut command = Command::new(executable);
    command
        .args(argv)
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Own process group (unix): timeout/cancel kills the whole child
    // tree (see kill_tree_and_reap), so forked grandchildren can neither
    // outlive supervision nor hold our capture pipes open. Harmless for
    // normal exits; no terminal job control in non-interactive use.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child: Child = command.spawn().map_err(|error| {
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
    })?;

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

/// Best-effort kill of the owned child *tree*, then reap. Sequence:
///
/// 1. `kill` the direct child through its handle (exact, never by name).
/// 2. `wait` to reap it (observes the truth through the handle).
/// 3. Signal the child's process group (unix: every spawn runs as a
///    group leader via `process_group(0)`) so forked grandchildren —
///    which otherwise survive as orphans holding our stdout/stderr pipes
///    open and stalling capture — die too.
///
/// Step 3 runs after the reap, narrowing the PID-reuse window to the
/// microseconds between our child's death and the group signal; errors
/// (empty group, races) are ignored. Kill errors in step 1
/// (already-exited races) are likewise ignored: `wait` observes the
/// truth. Never `pkill`/`killall`/name matching on any path.
fn kill_tree_and_reap(child: &mut Child) {
    let pid = child.id();
    let _ = child.kill();
    let _ = child.wait();
    kill_process_group(pid);
}

/// Signals the process group `group_id` (a group-leader PID from our own
/// spawn) with SIGKILL. Unix only: on other platforms the direct
/// `kill` + `wait` above is the whole tree kill.
#[cfg(unix)]
fn kill_process_group(group_id: u32) {
    const SIGKILL: i32 = 9;
    // Direct libc binding without a new dependency: libc is always
    // linked on unix targets, and this is the only symbol needed.
    extern "C" {
        fn killpg(pgrp: i32, sig: i32) -> i32;
    }
    let _ = unsafe { killpg(group_id as i32, SIGKILL) };
}

#[cfg(not(unix))]
fn kill_process_group(_group_id: u32) {}

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

    #[test]
    fn cancel_flag_is_idempotent() {
        let flag = CancelFlag::fresh();
        assert!(!flag.is_cancelled());
        flag.cancel();
        flag.cancel();
        assert!(flag.is_cancelled());
        assert!(flag.clone().is_cancelled());
    }

    #[test]
    fn missing_workdir_fails_closed_without_spawning() {
        let outcome = spawn_and_supervise(
            OsStr::new("runlith-definitely-not-a-binary-xyz"),
            &[],
            Path::new("/no/such/workdir-anywhere"),
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
            OsStr::new("runlith-definitely-not-a-binary-xyz"),
            &[],
            &dir,
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
            OsStr::new("runlith-definitely-not-a-binary-xyz"),
            &[],
            &dir,
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
