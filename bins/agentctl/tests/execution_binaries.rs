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
