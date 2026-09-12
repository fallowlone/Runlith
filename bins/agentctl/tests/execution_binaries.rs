//! G5 execution surface through the shipped binaries (TASK-0047).
//!
//! Real `agentd` + `agentctl` processes against test-owned runtime/data
//! directories, with a fixture provider named `claude` prepended to
//! `PATH` (the exact production resolution — no daemon flags). Covers
//! the `main.rs` paths the in-process suite cannot: `execution run`
//! wait-mode polling and exit codes, `--detach`, `status` printing, and
//! `cancel` printing.
//!
//! Unix-only. No quota, no network, no real Claude.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn workspace_bin(name: &str) -> PathBuf {
    let mut dir = std::env::current_exe().expect("test executable path");
    dir.pop();
    dir.pop();
    dir.join(name)
}

struct Sandbox {
    root: PathBuf,
    repo: PathBuf,
    path_env: String,
}

impl Sandbox {
    fn new(suite: &str, provider_body: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root =
            PathBuf::from("/tmp").join(format!("g5-bin-{}-{suite}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("sandbox root");
        // Production endpoint/data resolvers require the immediate
        // parents to exist (they create only the final component with
        // the secure mode); the daemon owns the rest.
        std::fs::create_dir_all(root.join("run")).expect("runtime parent");
        std::fs::create_dir_all(root.join("data")).expect("data parent");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        // Fixture provider named exactly `claude`: production PATH
        // resolution finds it with no daemon-side override.
        let bindir = root.join("bin");
        std::fs::create_dir_all(&bindir).expect("bin dir");
        let exe = bindir.join("claude");
        let mut file = std::fs::File::create(&exe).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(provider_body.as_bytes())
            .expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        let path_env = format!(
            "{}:{}",
            bindir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Self {
            root,
            repo,
            path_env,
        }
    }

    fn agentctl(&self, args: &[&str]) -> Output {
        Command::new(workspace_bin("agentctl"))
            .args(args)
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", self.root.join("run"))
            .env("AI_ENGINEERING_OS_DATA_DIR", self.root.join("data"))
            .env("PATH", &self.path_env)
            .output()
            .expect("run agentctl")
    }

    fn spawn_agentd(&self) -> Child {
        Command::new(workspace_bin("agentd"))
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", self.root.join("run"))
            .env("AI_ENGINEERING_OS_DATA_DIR", self.root.join("data"))
            .env("PATH", &self.path_env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agentd")
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let output = self.agentctl(&["ping"]);
            if output.status.success() {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "daemon never became ready: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
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
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    assert!(output.status.success(), "git {args:?} failed");
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const SUCCESS_BODY: &str = r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"binary fixture finished","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee"}'; exit 0
"#;

const SLOW_BODY: &str = "sleep 45\n";

fn seed_ready(sandbox: &Sandbox, project: &str, task: &str) {
    let created = sandbox.agentctl(&["project", "create", project, &format!("project {project}")]);
    assert!(
        created.status.success(),
        "seed project: {}",
        stderr(&created)
    );
    let created = sandbox.agentctl(&[
        "task",
        "create",
        task,
        project,
        &format!("objective {task}"),
    ]);
    assert!(created.status.success(), "seed task: {}", stderr(&created));
    let ready = sandbox.agentctl(&["task", "ready", task]);
    assert!(ready.status.success(), "seed ready: {}", stderr(&ready));
}

fn stop_daemon(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// RAII daemon handle for the TASK-0056 keepers: kills and reaps the child
/// on drop so a keeper failure can never leak a daemon (TEST-H).
struct DaemonGuard {
    child: Option<Child>,
}

impl DaemonGuard {
    fn spawn(sandbox: &Sandbox) -> Self {
        Self {
            child: Some(sandbox.spawn_agentd()),
        }
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn restart(&mut self, sandbox: &Sandbox) {
        self.kill();
        self.child = Some(sandbox.spawn_agentd());
        sandbox.wait_ready();
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// TASK-0056 (B-CR) binary keepers: the same live-ownership races as the
/// in-process suite, driven through the shipped `agentd`/`agentctl`
/// processes with parallel client processes racing one daemon.
///
/// Strand setup: `run --detach` commits the entry synchronously and
/// returns; an immediate SIGKILL of the daemon lands inside the
/// pre-attempt window (lease/Git/attempt work still pending), and the
/// restarted daemon reports the execution `RUNNING`, `live=false`, zero
/// attempts — the genuine crash shape. Retried with fresh identities when
/// the kill lands past attempt creation.
mod live_ownership {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ROUND: AtomicU64 = AtomicU64::new(0);

    fn repo(sandbox: &Sandbox) -> String {
        sandbox.repo.to_string_lossy().into_owned()
    }

    fn seed_task(sandbox: &Sandbox, project: &str, task: &str) {
        seed_ready(sandbox, project, task);
    }

    fn status_out(sandbox: &Sandbox, execution: &str) -> String {
        let status = sandbox.agentctl(&["execution", "status", execution]);
        assert!(status.status.success(), "status: {}", stderr(&status));
        stdout(&status)
    }

    fn cancel_out(sandbox: &Sandbox, execution: &str) -> String {
        let cancel = sandbox.agentctl(&["execution", "cancel", execution]);
        assert!(cancel.status.success(), "cancel: {}", stderr(&cancel));
        stdout(&cancel)
    }

    fn wait_no_live(sandbox: &Sandbox, execution: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if status_out(sandbox, execution).contains("live=false") {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "execution {execution} stayed live: {}",
                    status_out(sandbox, execution)
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn wait_attempt_line(sandbox: &Sandbox, execution: &str, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let out = status_out(sandbox, execution);
            if out.contains(needle) {
                return;
            }
            if Instant::now() >= deadline {
                panic!("status never showed {needle:?} for {execution}: {out:?}");
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Commits a genuine pre-attempt strand through the shipped binaries.
    /// Returns the (task, execution) identities of the strand.
    fn strand_via_kill(sandbox: &Sandbox, daemon: &mut DaemonGuard) -> (String, String) {
        for _ in 0..6 {
            let n = ROUND.fetch_add(1, Ordering::SeqCst);
            let project = format!("P-S{n}");
            let task = format!("T-S{n}");
            let execution = format!("E-S{n}");
            let attempt = format!("A-S{n}");
            seed_task(sandbox, &project, &task);
            let run = sandbox.agentctl(&[
                "execution",
                "run",
                &task,
                &repo(sandbox),
                "--execution",
                &execution,
                "--attempt",
                &attempt,
                "--detach",
            ]);
            assert!(run.status.success(), "detach run: {}", stderr(&run));
            // Crash inside the pre-attempt window: the entry is committed,
            // the background half dies with the daemon.
            daemon.kill();
            daemon.restart(sandbox);
            let out = status_out(sandbox, &execution);
            assert!(out.contains(&execution), "status names it: {out:?}");
            if !out.contains("attempt ") {
                assert!(out.contains("live=false"), "strand is honest: {out:?}");
                return (task, execution);
            }
            // Missed the window (attempt already created): cancel the slow
            // run for cleanliness and retry with fresh identities.
            let cancelled = cancel_out(sandbox, &execution);
            assert!(
                cancelled.contains("LIVE_SIGNALLED") || cancelled.contains("ALREADY_CONCLUDED"),
                "cleanup cancel honest: {cancelled:?}"
            );
            wait_no_live(sandbox, &execution);
        }
        panic!("could not strand a pre-attempt entry in 6 tries");
    }

    fn resume_cmd(execution: &str, repo: &str, task: &str) -> Vec<String> {
        vec![
            "execution".to_owned(),
            "resume".to_owned(),
            execution.to_owned(),
            repo.to_owned(),
            "--ref".to_owned(),
            format!("repo-local:{task}"),
            "--rev".to_owned(),
            "HEAD".to_owned(),
        ]
    }

    /// Runs `n` `agentctl execution resume` client processes in parallel
    /// against one daemon. Returns (accepted, refused) by stdout/stderr.
    fn race_resumes(
        sandbox: &Sandbox,
        execution: &str,
        task: &str,
        n: usize,
    ) -> (Vec<String>, Vec<String>) {
        let repo = repo(sandbox);
        let mut accepted = Vec::new();
        let mut refused = Vec::new();
        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(n);
            for _ in 0..n {
                let args = resume_cmd(execution, &repo, task);
                handles.push(scope.spawn(move || {
                    sandbox.agentctl(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                }));
            }
            for handle in handles {
                let output = handle.join().expect("client joins");
                let out = stdout(&output);
                let err = stderr(&output);
                if output.status.success() && out.contains("resumed execution") {
                    accepted.push(out);
                } else {
                    refused.push(format!("exit={} out={out:?} err={err:?}", output.status));
                }
            }
        });
        (accepted, refused)
    }
    #[test]
    fn binary_concurrent_resumes_elect_a_single_owner() {
        let sandbox = Sandbox::new("liveown-race", SLOW_BODY);
        let mut daemon = DaemonGuard::spawn(&sandbox);
        sandbox.wait_ready();

        // 2-client race on the first strand, then an 8-client race on a
        // fresh strand: each keeper observes a full claim → live →
        // cancel → release cycle through the real daemon.
        for clients in [2usize, 8usize] {
            let (task, execution) = strand_via_kill(&sandbox, &mut daemon);
            let (accepted, refused) = race_resumes(&sandbox, &execution, &task, clients);
            assert_eq!(
                accepted.len(),
                1,
                "{clients}-client race: exactly one owner, got {} accepted: {accepted:?} refused: {refused:?}",
                accepted.len()
            );
            for refusal in &refused {
                assert!(
                    refusal.contains("CONFLICT"),
                    "loser refused honestly, got {refusal:?}"
                );
            }
            // Winner stays live and cancellable through the real daemon.
            let status = status_out(&sandbox, &execution);
            assert!(status.contains("live=true"), "winner live: {status:?}");
            let cancelled = cancel_out(&sandbox, &execution);
            assert!(
                cancelled.contains("LIVE_SIGNALLED"),
                "cancel reaches the actual winner: {cancelled:?}"
            );
            wait_attempt_line(&sandbox, &execution, "CANCELLED");
            wait_no_live(&sandbox, &execution);
        }
    }

    #[test]
    fn binary_run_then_resume_keeps_the_run_owner() {
        let sandbox = Sandbox::new("liveown-runresume", SLOW_BODY);
        let _daemon = DaemonGuard::spawn(&sandbox);
        sandbox.wait_ready();

        // Several run+resume overlaps: the resume must NEVER report
        // `resumed` while the run owns the execution. Rounds where the
        // resume lands post-attempt refuse INVALID_TRANSITION (honest, but
        // not the race); at least one round must land the pre-attempt
        // CONFLICT overlap to prove the keeper bites.
        let mut conflict_rounds = 0;
        for _ in 0..5 {
            let n = ROUND.fetch_add(1, Ordering::SeqCst);
            let project = format!("P-R{n}");
            let task = format!("T-R{n}");
            let execution = format!("E-R{n}");
            let attempt = format!("A-R{n}");
            seed_task(&sandbox, &project, &task);
            let run = sandbox.agentctl(&[
                "execution",
                "run",
                &task,
                &repo(&sandbox),
                "--execution",
                &execution,
                "--attempt",
                &attempt,
                "--detach",
            ]);
            assert!(run.status.success(), "detach run: {}", stderr(&run));
            let resume_args = resume_cmd(&execution, &repo(&sandbox), &task);
            let resume =
                sandbox.agentctl(&resume_args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            let out = stdout(&resume);
            let err = stderr(&resume);
            assert!(
                !out.contains("resumed execution"),
                "resume must never overwrite the live run owner (B-CR): {out:?}"
            );
            if !resume.status.success() && err.contains("CONFLICT") {
                conflict_rounds += 1;
                // Ownership intact: the run is still the live owner.
                let status = status_out(&sandbox, &execution);
                assert!(status.contains("live=true"), "run stays live: {status:?}");
            }
            // Cleanup: cancel the slow run and wait for release.
            let cancelled = cancel_out(&sandbox, &execution);
            assert!(
                cancelled.contains("LIVE_SIGNALLED") || cancelled.contains("ALREADY_CONCLUDED"),
                "cleanup cancel honest: {cancelled:?}"
            );
            wait_no_live(&sandbox, &execution);
        }
        assert!(
            conflict_rounds >= 1,
            "at least one round must hit the pre-attempt overlap"
        );
    }
}

#[test]
fn run_wait_mode_reports_terminal_success_with_exit_zero() {
    let sandbox = Sandbox::new("wait", SUCCESS_BODY);
    let daemon = sandbox.spawn_agentd();
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-1");

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-1",
        "--attempt",
        "A-1",
    ]);
    assert!(
        run.status.success(),
        "wait-mode run exits 0 on SUCCEEDED: stdout={} stderr={}",
        stdout(&run),
        stderr(&run)
    );
    let out = stdout(&run);
    assert!(
        out.contains("SUCCEEDED"),
        "terminal outcome printed, got {out:?}"
    );
    assert!(out.contains("VERIFYING"), "task state printed, got {out:?}");

    let task = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(task.status.success());
    assert!(
        stdout(&task).contains("VERIFYING"),
        "provider success verifies, never completes: {}",
        stdout(&task)
    );

    let status = sandbox.agentctl(&["execution", "status", "E-1"]);
    assert!(status.status.success());
    let out = stdout(&status);
    assert!(
        out.contains("E-1") && out.contains("claude-code"),
        "got {out:?}"
    );
    stop_daemon(daemon);
}

#[test]
fn detach_then_cancel_reaches_the_live_run() {
    let sandbox = Sandbox::new("cancel", SLOW_BODY);
    let daemon = sandbox.spawn_agentd();
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-1");

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-2",
        "--attempt",
        "A-2",
        "--detach",
    ]);
    assert!(run.status.success(), "detach starts: {}", stderr(&run));
    assert!(stdout(&run).contains("E-2"));

    // The cancel flag registers synchronously with the accepted run, so
    // an immediate cancel deterministically signals the live background.
    let cancel = sandbox.agentctl(&["execution", "cancel", "E-2"]);
    assert!(
        cancel.status.success(),
        "cancel prints: {}",
        stderr(&cancel)
    );
    assert!(
        stdout(&cancel).contains("LIVE_SIGNALLED"),
        "got {:?}",
        stdout(&cancel)
    );

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = sandbox.agentctl(&["execution", "status", "E-2"]);
        assert!(status.status.success());
        let out = stdout(&status);
        if out.contains("CANCELLED") {
            break;
        }
        if Instant::now() >= deadline {
            panic!("slow run never cancelled: {out:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let task = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(
        stdout(&task).contains("RUNNING"),
        "cancel is not completion: {}",
        stdout(&task)
    );

    let again = sandbox.agentctl(&["execution", "cancel", "E-2"]);
    assert!(again.status.success());
    assert!(
        stdout(&again).contains("ALREADY_CONCLUDED"),
        "got {:?}",
        stdout(&again)
    );
    stop_daemon(daemon);
}
