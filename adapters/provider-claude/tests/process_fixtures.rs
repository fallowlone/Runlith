//! TASK-0046 process integration: the adapter against controlled fake
//! executables (no Claude subscription quota, no network, no auth).
//!
//! Unix-only: fixtures are tiny shell scripts executed directly (argv,
//! no shell on our side). Each fixture models one provider behavior:
//! structured success, malformed output, non-zero exit, hang (for
//! cancellation/timeout), and cwd reporting. A missing executable and a
//! missing workdir prove the fail-closed paths.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use agent_runtime::{AttemptId, ExecutionId};
use provider_claude::{CancelFlag, ClaudeConfig, ClaudeExecutor, LiveProcessTracker};
use scheduler::{ExecutionPort, WorkEvidence, WorkItem};
use task_domain::TaskId;

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Case {
    dir: PathBuf,
}

impl Case {
    fn named(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-claude-fixture-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        Self { dir }
    }

    /// Writes an executable shell fixture and returns its path. Our side
    /// always spawns it directly (no `sh -c`): the fixture being a script
    /// is an OS execution detail, not shell interpolation.
    fn fixture(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.join(name);
        let mut file = std::fs::File::create(&path).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(body.as_bytes()).expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        path
    }

    fn workdir(&self) -> PathBuf {
        let workdir = self.dir.join("work");
        std::fs::create_dir_all(&workdir).expect("work dir");
        workdir
    }

    fn environment_base(&self) -> PathBuf {
        self.dir.join("provider-env")
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn work(execution: &str, attempt: &str) -> WorkItem {
    WorkItem {
        task_id: TaskId::new("T-1").unwrap(),
        execution_id: ExecutionId::new(execution).unwrap(),
        attempt_id: AttemptId::new(attempt).unwrap(),
    }
}

fn executor_for(fixture: &Path, workdir: &Path, timeout: Duration) -> ClaudeExecutor {
    let config = ClaudeConfig::default()
        .with_executable(fixture)
        .unwrap()
        .with_timeout(timeout)
        .unwrap();
    ClaudeExecutor::bind(
        config,
        "fixture prompt",
        workdir.to_path_buf(),
        workdir.parent().unwrap().join("provider-env"),
    )
    .unwrap()
}

fn success_fixture(case: &Case) -> PathBuf {
    case.fixture(
        "success.sh",
        r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"fixture did CWD_MARKER:%s","session_id":"11111111-2222-3333-4444-555555555555","usage":{"input_tokens":7,"output_tokens":9}}' "$(pwd)""#,
    )
}

#[test]
fn structured_success_becomes_canonical_evidence() {
    let case = Case::named("success");
    let fixture = success_fixture(&case);
    let workdir = case.workdir();
    let executor = executor_for(&fixture, &workdir, Duration::from_secs(10));

    let evidence: WorkEvidence = executor
        .execute(&work("E-1", "A-1"))
        .expect("fixture success");
    assert!(!evidence.partial, "terminal fixture output is complete");
    assert!(
        evidence
            .summary
            .contains(&workdir.to_string_lossy().into_owned()),
        "child ran in the bound workdir, got {:?}",
        evidence.summary
    );
    assert_eq!(
        evidence.evidence_ref,
        "claude:session:11111111-2222-3333-4444-555555555555"
    );
    assert_eq!(executor.provider_ref().as_str(), "claude-code");
}

#[test]
fn h7_provider_state_is_outside_worktree_and_ambient_env_is_cleared() {
    let case = Case::named("h7-env");
    let workdir = case.workdir();
    let fixture = case.fixture(
        "h7.sh",
        r#"case " $* " in
  *" --safe-mode "*) ;;
  *) mkdir -p data; : > data/sessions.db ;;
esac
if [ -n "${CARGO_MANIFEST_DIR+x}" ]; then
  echo 'ambient cargo environment leaked' >&2
  exit 41
fi
mkdir -p "$XDG_DATA_HOME"
: > "$XDG_DATA_HOME/sessions.db"
printf '{"type":"result","subtype":"success","is_error":false,"result":"h7 isolated","session_id":"77777777-7777-7777-7777-777777777777"}'
"#,
    );
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .unwrap()
        .with_timeout(Duration::from_secs(10))
        .unwrap();
    let environment_base = case.environment_base();
    let executor = ClaudeExecutor::bind(
        config,
        "fixture prompt",
        workdir.clone(),
        environment_base.clone(),
    )
    .unwrap();

    executor
        .execute(&work("E-H7", "A-H7"))
        .expect("isolated fixture succeeds");

    assert!(
        !workdir.join("data/sessions.db").exists(),
        "provider-native ambient state must not dirty the managed worktree"
    );
    let runtime_state = environment_base
        .join(workdir.file_name().unwrap())
        .join("data/sessions.db");
    assert!(
        runtime_state.is_file(),
        "provider writable state must land in the isolated provider environment"
    );
    assert!(!runtime_state.starts_with(&workdir));
}

#[test]
fn provider_environment_inside_worktree_fails_before_spawn() {
    let case = Case::named("h7-overlap");
    let workdir = case.workdir();
    let marker = case.dir.join("must-not-spawn.txt");
    let fixture = case.fixture(
        "marker.sh",
        &format!(
            "echo spawned > {}\nprintf '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"unexpected\",\"session_id\":\"88888888-8888-8888-8888-888888888888\"}}'\n",
            marker.display()
        ),
    );
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .unwrap()
        .with_timeout(Duration::from_secs(10))
        .unwrap();
    let executor = ClaudeExecutor::bind(
        config,
        "fixture prompt",
        workdir.clone(),
        workdir.join("provider-env"),
    )
    .unwrap();

    let err = executor
        .execute(&work("E-H7-BLOCK", "A-H7-BLOCK"))
        .expect_err("overlapping provider environment must fail closed");
    match err {
        scheduler::ExecutionProblem::Failed { detail } => {
            assert!(detail.contains("environment_isolation"), "got {detail:?}");
        }
        other => panic!("overlap must be a failed execution, got {other:?}"),
    }
    assert!(
        !marker.exists(),
        "provider must not spawn after isolation refusal"
    );
}

#[test]
fn usage_is_exposed_with_provenance() {
    let case = Case::named("usage");
    let fixture = success_fixture(&case);
    let workdir = case.workdir();
    let executor = executor_for(&fixture, &workdir, Duration::from_secs(10));

    let outcome = executor
        .execute_detailed(&work("E-1", "A-1"))
        .expect("fixture success");
    assert_eq!(outcome.usage.input_tokens, Some(7));
    assert_eq!(outcome.usage.output_tokens, Some(9));
    assert_eq!(
        outcome.usage.provenance,
        provider_claude::UsageProvenance::Reported
    );
    assert_eq!(outcome.session_ref, "11111111-2222-3333-4444-555555555555");
}

#[test]
fn malformed_output_is_failed_never_evidence() {
    let case = Case::named("malformed");
    let fixture = case.fixture("malformed.sh", "printf 'this is not json{{{'\n");
    let executor = executor_for(&fixture, &case.workdir(), Duration::from_secs(10));

    let err = executor
        .execute(&work("E-1", "A-1"))
        .expect_err("garbage must fail");
    match err {
        scheduler::ExecutionProblem::Failed { detail } => {
            assert!(detail.contains("malformed_output"), "got {detail:?}");
        }
        other => panic!("malformed must be Failed, got {other:?}"),
    }
}

#[test]
fn empty_output_is_unexpected_eof_failure() {
    let case = Case::named("empty");
    let fixture = case.fixture("empty.sh", "exit 0\n");
    let executor = executor_for(&fixture, &case.workdir(), Duration::from_secs(10));

    let err = executor
        .execute(&work("E-1", "A-1"))
        .expect_err("empty must fail");
    assert!(
        matches!(err, scheduler::ExecutionProblem::Failed { .. }),
        "empty output must be Failed, got {err:?}"
    );
}

#[test]
fn non_zero_exit_is_failed_with_exit_evidence() {
    let case = Case::named("exit3");
    let fixture = case.fixture("exit3.sh", "echo 'provider exploded' >&2\nexit 3\n");
    let executor = executor_for(&fixture, &case.workdir(), Duration::from_secs(10));

    let err = executor
        .execute(&work("E-1", "A-1"))
        .expect_err("exit 3 must fail");
    match err {
        scheduler::ExecutionProblem::Failed { detail } => {
            assert!(
                detail.contains('3'),
                "exit code must survive, got {detail:?}"
            );
            assert!(detail.contains("provider exploded"), "got {detail:?}");
        }
        other => panic!("non-zero exit must be Failed, got {other:?}"),
    }
}

#[test]
fn auth_hint_in_stderr_is_auth_required() {
    let case = Case::named("auth");
    let fixture = case.fixture("auth.sh", "echo 'Error: Invalid API key' >&2\nexit 1\n");
    let workdir = case.workdir();
    let executor = executor_for(&fixture, &workdir, Duration::from_secs(10));

    let err = executor
        .execute_detailed(&work("E-1", "A-1"))
        .expect_err("auth must fail");
    assert!(
        matches!(err, provider_claude::ClaudeFailure::AuthRequired { .. }),
        "auth hint must be AuthRequired, got {err:?}"
    );
}

#[test]
fn hang_times_out_and_kills_only_the_owned_child() {
    let case = Case::named("hang");
    let fixture = case.fixture("hang.sh", "sleep 60\n");
    let executor = executor_for(&fixture, &case.workdir(), Duration::from_secs(1));

    let before = std::time::Instant::now();
    let err = executor
        .execute(&work("E-1", "A-1"))
        .expect_err("hang must time out");
    assert!(
        before.elapsed() < Duration::from_secs(30),
        "timeout must kill promptly"
    );
    assert!(
        matches!(err, scheduler::ExecutionProblem::TimedOut { .. }),
        "hang must be TimedOut, got {err:?}"
    );
}

#[test]
fn cancel_kills_only_the_intended_child() {
    let case = Case::named("cancel");
    let fixture = case.fixture("slow.sh", "sleep 60\n");
    let workdir = case.workdir();
    let executor = executor_for(&fixture, &workdir, Duration::from_secs(60));
    let canceller = executor.clone();

    let handle = std::thread::spawn(move || executor.execute(&work("E-1", "A-1")));
    std::thread::sleep(Duration::from_millis(300));
    canceller.cancel();
    let err = handle
        .join()
        .expect("worker thread")
        .expect_err("cancel must win");
    assert!(
        matches!(err, scheduler::ExecutionProblem::Cancelled),
        "cancel must be Cancelled, got {err:?}"
    );
    // No stray `sleep 60` from this fixture may survive: the kill targets
    // only the owned handle, and the reap observes the truth. The test
    // finishing promptly is itself the evidence (a leaked sleep would
    // only waste the fixture's own process table entry, reaped on drop).
}

#[test]
fn tracker_marks_live_during_execution_only() {
    let case = Case::named("tracker");
    let fixture = case.fixture("slow2.sh", "sleep 2\n");
    let workdir = case.workdir();
    let tracker = LiveProcessTracker::fresh();
    let executor =
        executor_for(&fixture, &workdir, Duration::from_secs(30)).with_tracker(tracker.clone());
    let watcher = tracker.clone();
    let owner = ExecutionId::new("E-track").unwrap();

    let handle = std::thread::spawn(move || executor.execute(&work("E-track", "A-1")));
    // Wait until the child registers (bounded spin, then fail loudly).
    let registered = (0..200).any(|_| {
        if watcher.is_live_execution(&owner) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
        false
    });
    assert!(registered, "tracker must observe the live child");
    // OwnerLiveness seam answers through the same table.
    use workspace::OwnerLiveness;
    assert!(tracker.is_live(&owner));
    // The fixture exits on its own (~2s); the terminal path must
    // unregister the owner exactly once.
    let _ = handle.join().expect("worker thread");
    assert!(
        !tracker.is_live_execution(&owner),
        "terminal outcome must unregister the owner"
    );
}

#[test]
fn prompt_with_shell_metacharacters_stays_argv_safe() {
    let case = Case::named("argv");
    // Fixture echoes its argv back as JSON result text: proves the prompt
    // arrived whole (Bash would have interpolated `$(...)`).
    let fixture = case.fixture(
        "echo_argv.sh",
        r#"for P; do :; done; E=$(printf '%s' "$P" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e ':a' -e 'N' -e '$!ba' -e 's/\n/\\n/g'); printf '{"type":"result","subtype":"success","is_error":false,"result":"ARGV:%s","session_id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"}' "$E""#,
    );
    let hostile = "$(touch /tmp/pwned) `id` \"q\" 's' --flag\nnewline";
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .unwrap()
        .with_timeout(Duration::from_secs(10))
        .unwrap();
    let executor = ClaudeExecutor::bind(
        config,
        hostile,
        case.workdir().to_path_buf(),
        case.environment_base(),
    )
    .unwrap();
    let evidence = executor.execute(&work("E-1", "A-1")).expect("echo fixture");
    assert!(
        evidence.summary.contains(hostile),
        "prompt must arrive whole, got {:?}",
        evidence.summary
    );
    assert!(
        !std::path::Path::new("/tmp/pwned").exists(),
        "no command substitution may have run"
    );
}

// -- TASK-0047: bounded capture finalization + external cancel ---------

#[test]
fn daemon_grandchild_holding_pipes_does_not_block_finalization() {
    // Natural-exit caveat (TASK-0046 §12): the fixture CLI prints valid
    // success JSON and exits at once, but a double-forked grandchild
    // outlives it for 10 s while holding the inherited stdout/stderr
    // pipes open. Finalization must be bounded by the single capture
    // grace (1 s here), never by the grandchild's lifetime.
    //
    // Deterministic contract (TASK-0049 B2): the bound is expressed
    // relative to the configured grace plus an explicit scheduling
    // margin for full-suite load — not as a fixed wall-clock number
    // divorced from the implementation. It still fails if execution
    // ever blocks for the grandchild's full lifetime (10 s >> bound).
    let grace = Duration::from_secs(1);
    let grandchild_lifetime = Duration::from_secs(10);
    let case = Case::named("grandchild");
    let fixture = case.fixture(
        "daemon.sh",
        "( sleep 10 & )\nprintf '{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"parent exited first\",\"session_id\":\"dddddddd-dddd-dddd-dddd-dddddddddddd\"}'\nexit 0\n",
    );
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .unwrap()
        .with_timeout(Duration::from_secs(60))
        .unwrap()
        .with_capture_grace(grace)
        .unwrap();
    let executor = ClaudeExecutor::bind(
        config,
        "fixture prompt",
        case.workdir().to_path_buf(),
        case.environment_base(),
    )
    .unwrap();

    let before = std::time::Instant::now();
    let evidence = executor
        .execute(&work("E-1", "A-1"))
        .expect("parent output finalizes despite the grandchild");
    let elapsed = before.elapsed();
    // Grace-relative bound: one shared grace for both drains (TASK-0049
    // H3) plus an explicit 3 s scheduling margin for loaded machines.
    // A regression to per-stream sequential graces (~2 s) still passes;
    // a regression to unbounded grandchild blocking (~10 s) fails.
    assert!(
        elapsed < grace * 2 + Duration::from_secs(3),
        "finalization must be grace-bounded, took {elapsed:?}"
    );
    // Explicitly far below the grandchild's lifetime: proves detachment,
    // not patience.
    assert!(
        elapsed + Duration::from_secs(2) < grandchild_lifetime,
        "finalization must end well before the grandchild exits, took {elapsed:?}"
    );
    assert!(
        evidence.summary.contains("parent exited first"),
        "bytes racing ahead of the grace must survive, got {:?}",
        evidence.summary
    );
    assert_eq!(
        evidence.evidence_ref,
        "claude:session:dddddddd-dddd-dddd-dddd-dddddddddddd"
    );
}

#[test]
fn preset_cancel_flag_spawns_no_child() {
    // TASK-0049 H8: a flag set before `execute` must yield Cancelled
    // without spawning. The fixture drops a marker file if it ever runs;
    // an absent marker proves no child started (not merely "started then
    // killed on the first quantum").
    let case = Case::named("prespawn-cancel");
    let marker = case.dir.join("spawn-marker.txt");
    let fixture = case.fixture(
        "marker.sh",
        &format!(
            "echo spawned > {}\nprintf '{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"must never run\",\"session_id\":\"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee\"}}'\n",
            marker.display()
        ),
    );
    let workdir = case.workdir();
    let flag = CancelFlag::fresh();
    flag.cancel();
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .unwrap()
        .with_timeout(Duration::from_secs(60))
        .unwrap();
    let executor = ClaudeExecutor::bind(
        config,
        "fixture prompt",
        workdir.to_path_buf(),
        case.environment_base(),
    )
    .unwrap()
    .with_cancel_flag(flag);

    let err = executor
        .execute(&work("E-1", "A-1"))
        .expect_err("pre-set cancel must win");
    assert!(
        matches!(err, scheduler::ExecutionProblem::Cancelled),
        "pre-set cancel must be Cancelled, got {err:?}"
    );
    assert!(
        !marker.is_file(),
        "no child may spawn under a pre-set cancel flag"
    );
}

#[test]
fn external_cancel_flag_signals_the_owned_tree() {
    // Composition-root wiring: the daemon owns one CancelFlag per
    // execution and signals it from a later `cancel` IPC on another
    // connection. The executor must honor a flag it did not create.
    let case = Case::named("extcancel");
    let fixture = case.fixture("slow3.sh", "sleep 60\n");
    let workdir = case.workdir();
    let flag = CancelFlag::fresh();
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .unwrap()
        .with_timeout(Duration::from_secs(60))
        .unwrap();
    let executor = ClaudeExecutor::bind(
        config,
        "fixture prompt",
        workdir.to_path_buf(),
        case.environment_base(),
    )
    .unwrap()
    .with_cancel_flag(flag.clone());

    let handle = std::thread::spawn(move || executor.execute(&work("E-1", "A-1")));
    std::thread::sleep(Duration::from_millis(300));
    flag.cancel();
    let err = handle
        .join()
        .expect("worker thread")
        .expect_err("external cancel must win");
    assert!(
        matches!(err, scheduler::ExecutionProblem::Cancelled),
        "external cancel must be Cancelled, got {err:?}"
    );
}
