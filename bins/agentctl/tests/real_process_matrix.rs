//! G1 gate real-process verification (TASK-0021).
//!
//! Uses the built `agentd`/`agentctl` binaries against the canonical
//! per-user endpoint in one ordered sequence: singleton ownership across
//! real processes, concurrent launcher convergence, idempotent reuse,
//! shutdown, no-spawn-on-shutdown, and kill-9 crash plus stale recovery.
//! Production sources are NOT touched by this task; this file is a
//! verification harness only.
//!
//! Unix-only: process signalling below uses `kill`.
//! Windows runtime execution is recorded as not exercised on this host;
//! the hard Windows gate item is target compilation (see closure audit).

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TEST_RUNTIME_ENV: &str = "AI_ENGINEERING_OS_RUNTIME_DIR";
const TEST_DATA_ENV: &str = "AI_ENGINEERING_OS_DATA_DIR";

fn workspace_bin(name: &str) -> PathBuf {
    let mut dir = std::env::current_exe().expect("test executable path");
    dir.pop();
    dir.pop();
    dir.join(name)
}

fn agentd_bin() -> PathBuf {
    workspace_bin("agentd")
}

fn agentctl_bin() -> PathBuf {
    workspace_bin("agentctl")
}

fn agentctl(args: &[&str]) -> std::process::Output {
    Command::new(agentctl_bin())
        .env(TEST_RUNTIME_ENV, test_runtime_parent())
        .env(TEST_DATA_ENV, test_data_parent())
        .args(args)
        .output()
        .expect("run agentctl")
}

fn test_runtime_parent() -> PathBuf {
    PathBuf::from(format!("/tmp/rl0069-{}", std::process::id()))
}

fn test_data_parent() -> PathBuf {
    test_runtime_parent().join("data")
}

fn canonical_socket() -> PathBuf {
    test_runtime_parent().join("ai-engineering-os/agentd.sock")
}

fn endpoint_present() -> bool {
    std::fs::symlink_metadata(canonical_socket()).is_ok()
}

fn wait_for(cond: impl Fn() -> bool, budget: Duration, what: &str) {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for: {what}");
}

fn wait_ping_ok() {
    wait_for(
        connect_ping,
        Duration::from_secs(15),
        "connect-only ping to succeed",
    );
}

fn endpoint() -> ipc_transport::platform::unix::UnixEndpoint {
    let runtime_dir = test_runtime_parent().join("ai-engineering-os");
    ipc_transport::platform::unix::UnixEndpoint {
        socket_path: runtime_dir.join("agentd.sock"),
        runtime_dir,
    }
}

/// Connects only to this test's endpoint and pings it. This deliberately
/// avoids the `agentctl ping` command, which is a launcher and may spawn.
fn connect_ping() -> bool {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("readiness runtime")
        .block_on(async {
            match agentctl::connect_and_handshake(&endpoint()).await {
                Ok(mut session) => session.ping().await.is_ok(),
                Err(_) => false,
            }
        })
}

/// Owns a directly-spawned daemon and always kills and reaps it on failure.
struct DaemonGuard {
    child: Option<Child>,
}

impl DaemonGuard {
    fn spawn() -> Self {
        let child = Command::new(agentd_bin())
            .env(TEST_RUNTIME_ENV, test_runtime_parent())
            .env(TEST_DATA_ENV, test_data_parent())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agentd");
        Self { child: Some(child) }
    }

    fn wait_for_exit(&mut self, budget: Duration) -> Option<i32> {
        let deadline = Instant::now() + budget;
        loop {
            match self.child.as_mut()?.try_wait().expect("try_wait") {
                Some(status) => {
                    self.child.take();
                    return status.code();
                }
                None if Instant::now() >= deadline => return None,
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn kill_9(&mut self) {
        if let Some(mut child) = self.child.take() {
            let status = Command::new("kill")
                .arg("-9")
                .arg(child.id().to_string())
                .status()
                .expect("kill -9");
            assert!(status.success(), "kill -9 must signal the owned daemon");
            let _ = child.wait();
        }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

#[test]
fn g1_real_process_matrix() {
    let _ = std::fs::remove_dir_all(test_runtime_parent());
    std::fs::create_dir_all(test_runtime_parent()).expect("create test runtime parent");
    std::fs::create_dir_all(test_data_parent()).expect("create test data parent");

    // Phase 0 — prescribed precondition: no test daemon running. If a live
    // daemon owns the endpoint, shut it down through the product flow.
    if endpoint_present() && agentctl(&["ping"]).status.success() {
        let out = agentctl(&["shutdown"]);
        assert!(out.status.success(), "pre-existing daemon must shut down");
    }
    // A stale node (if any) is recovered by the first start below.

    // 1. Daemon missing: `agentctl start` launches, handshakes, reports ready.
    let out = agentctl(&["start"]);
    assert!(out.status.success(), "start failed: {out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("daemon ready"),
        "unexpected start output: {out:?}"
    );
    wait_ping_ok();
    assert!(endpoint_present(), "start must bind the scoped endpoint");

    // 2. `agentctl ping` reuses the running daemon (no duplicate).
    let out = agentctl(&["ping"]);
    assert!(out.status.success(), "ping failed: {out:?}");
    assert!(
        endpoint_present(),
        "ping must keep the scoped endpoint usable"
    );

    // 3. Second `start` is idempotent reuse.
    let out = agentctl(&["start"]);
    assert!(out.status.success(), "second start failed: {out:?}");
    assert!(
        endpoint_present(),
        "second start must reuse the scoped endpoint"
    );

    // 4. Real multi-process singleton: a second agentd loses.
    let mut second = DaemonGuard::spawn();
    let code = second.wait_for_exit(Duration::from_secs(15));
    assert_eq!(code, Some(2), "second daemon must exit AlreadyRunning(2)");
    // Winner remains usable.
    let out = agentctl(&["ping"]);
    assert!(out.status.success(), "winner must stay usable");
    assert!(endpoint_present());

    // 5. Concurrent real launchers converge on the one daemon.
    let launches: Vec<std::process::Output> = (0..3)
        .map(|_| std::thread::spawn(|| agentctl(&["start"])))
        .collect::<Vec<_>>()
        .into_iter()
        .map(|handle| handle.join().expect("launcher thread"))
        .collect();
    for out in &launches {
        assert!(out.status.success(), "convergent start failed: {out:?}");
    }
    let out = agentctl(&["ping"]);
    assert!(out.status.success());
    assert!(
        endpoint_present(),
        "launchers must converge on the scoped endpoint"
    );

    // 6. Graceful shutdown: ack, wind-down, endpoint gone, process exits.
    let out = agentctl(&["shutdown"]);
    assert!(out.status.success(), "shutdown failed: {out:?}");
    wait_for(
        || !endpoint_present(),
        Duration::from_secs(15),
        "endpoint disappearance",
    );

    // 7. Shutdown with no daemon: explicit not-running, no spawn.
    let out = agentctl(&["shutdown"]);
    assert!(!out.status.success(), "shutdown without daemon must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not running"),
        "unexpected stderr: {stderr}"
    );
    assert!(!endpoint_present());

    // 8. Crash: kill -9 the running daemon, then recover.
    let mut crashed = DaemonGuard::spawn();
    wait_ping_ok();
    crashed.kill_9();
    assert!(
        endpoint_present(),
        "stale socket node must remain after SIGKILL"
    );
    // Next start inspects, probes, snapshots, removes-if-unchanged, binds.
    let out = agentctl(&["start"]);
    assert!(out.status.success(), "post-crash start failed: {out:?}");
    wait_ping_ok();
    assert!(endpoint_present(), "recovery must bind the scoped endpoint");

    // 9. Final shutdown leaves a clean host.
    let out = agentctl(&["shutdown"]);
    assert!(out.status.success(), "final shutdown failed: {out:?}");
    wait_for(
        || !endpoint_present(),
        Duration::from_secs(15),
        "final endpoint cleanup",
    );
}
