//! G1 gate real-process verification (TASK-0021).
//!
//! Uses the built `agentd`/`agentctl` binaries against the canonical
//! per-user endpoint in one ordered sequence: singleton ownership across
//! real processes, concurrent launcher convergence, idempotent reuse,
//! shutdown, no-spawn-on-shutdown, and kill-9 crash plus stale recovery.
//! Production sources are NOT touched by this task; this file is a
//! verification harness only.
//!
//! Unix-only: process signalling/counting below uses `kill`/`pgrep`.
//! Windows runtime execution is recorded as not exercised on this host;
//! the hard Windows gate item is target compilation (see closure audit).

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
        .args(args)
        .output()
        .expect("run agentctl")
}

fn canonical_socket() -> PathBuf {
    let tmpdir = std::env::var_os("TMPDIR").expect("TMPDIR on macOS test host");
    PathBuf::from(tmpdir).join("ai-engineering-os/agentd.sock")
}

fn endpoint_present() -> bool {
    std::fs::symlink_metadata(canonical_socket()).is_ok()
}

/// PIDs of processes whose command line contains our agentd binary path.
fn agentd_pids() -> Vec<String> {
    let pattern = agentd_bin().to_string_lossy().into_owned();
    let output = Command::new("pgrep").arg("-f").arg(&pattern).output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
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
        || agentctl(&["ping"]).status.success(),
        Duration::from_secs(15),
        "agentctl ping to succeed",
    );
}

fn spawn_daemon() -> Child {
    Command::new(agentd_bin())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn agentd")
}

fn wait_child_exit(child: &mut Child, budget: Duration) -> Option<i32> {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => return status.code(),
            None => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

#[test]
fn g1_real_process_matrix() {
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
    assert_eq!(agentd_pids().len(), 1, "exactly one daemon process");

    // 2. `agentctl ping` reuses the running daemon (no duplicate).
    let out = agentctl(&["ping"]);
    assert!(out.status.success(), "ping failed: {out:?}");
    assert_eq!(agentd_pids().len(), 1, "ping must not spawn");

    // 3. Second `start` is idempotent reuse.
    let out = agentctl(&["start"]);
    assert!(out.status.success(), "second start failed: {out:?}");
    assert_eq!(agentd_pids().len(), 1, "second start must not spawn");

    // 4. Real multi-process singleton: a second agentd loses.
    let mut second = spawn_daemon();
    let code = wait_child_exit(&mut second, Duration::from_secs(15));
    assert_eq!(code, Some(2), "second daemon must exit AlreadyRunning(2)");
    // Winner remains usable.
    let out = agentctl(&["ping"]);
    assert!(out.status.success(), "winner must stay usable");
    assert_eq!(agentd_pids().len(), 1);

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
    assert_eq!(agentd_pids().len(), 1, "launchers must converge");

    // 6. Graceful shutdown: ack, wind-down, endpoint gone, process exits.
    let out = agentctl(&["shutdown"]);
    assert!(out.status.success(), "shutdown failed: {out:?}");
    wait_for(
        || !endpoint_present() && agentd_pids().is_empty(),
        Duration::from_secs(15),
        "endpoint disappearance and daemon exit",
    );

    // 7. Shutdown with no daemon: explicit not-running, no spawn.
    let out = agentctl(&["shutdown"]);
    assert!(!out.status.success(), "shutdown without daemon must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not running"),
        "unexpected stderr: {stderr}"
    );
    assert!(agentd_pids().is_empty(), "shutdown must not spawn");
    assert!(!endpoint_present());

    // 8. Crash: kill -9 the running daemon, then recover.
    let out = agentctl(&["start"]);
    assert!(out.status.success());
    wait_ping_ok();
    let pids = agentd_pids();
    assert_eq!(pids.len(), 1);
    let kill = Command::new("kill")
        .arg("-9")
        .arg(&pids[0])
        .status()
        .expect("kill");
    assert!(kill.success());
    wait_for(
        || agentd_pids().is_empty(),
        Duration::from_secs(10),
        "killed daemon to exit",
    );
    assert!(
        endpoint_present(),
        "stale socket node must remain after SIGKILL"
    );
    // Next start inspects, probes, snapshots, removes-if-unchanged, binds.
    let out = agentctl(&["start"]);
    assert!(out.status.success(), "post-crash start failed: {out:?}");
    wait_ping_ok();
    assert_eq!(agentd_pids().len(), 1);

    // 9. Final shutdown leaves a clean host.
    let out = agentctl(&["shutdown"]);
    assert!(out.status.success(), "final shutdown failed: {out:?}");
    wait_for(
        || !endpoint_present() && agentd_pids().is_empty(),
        Duration::from_secs(15),
        "final cleanup",
    );
}
