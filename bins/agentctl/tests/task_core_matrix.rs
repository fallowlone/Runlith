//! G2 business real-process verification (TASK-0028).
//!
//! Real `agentd`/`agentctl` binaries against a test-owned endpoint and a
//! test-owned database (per-test sandbox via `AI_ENGINEERING_OS_RUNTIME_DIR`
//! and `AI_ENGINEERING_OS_DATA_DIR`): project/task/dependency/lifecycle
//! E2E, invalid-input atomicity, restart and kill-9 durability, v0
//! backward compatibility, CLI surface discipline, and journal audit
//! completeness. Journal assertions read the test database through the
//! storage adapter in the harness only — `agentctl` itself never touches
//! SQLite (proven by the product dependency tree, not here).
//!
//! Unix-only: process signalling below uses `Child::kill` (SIGKILL).
//! Windows runtime execution is recorded as not exercised on this host.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use storage_sqlite::{JournalEvent, SqliteStore};

fn workspace_bin(name: &str) -> PathBuf {
    let mut dir = std::env::current_exe().expect("test executable path");
    dir.pop();
    dir.pop();
    dir.join(name)
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(suite: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root =
            PathBuf::from("/tmp").join(format!("g2-{}-{}-{suite}-{id}", std::process::id(), {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            }));
        std::fs::create_dir_all(&root).expect("sandbox root");
        Self { root }
    }

    fn agentctl(&self, args: &[&str]) -> Output {
        Command::new(workspace_bin("agentctl"))
            .args(args)
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", &self.root)
            .env("AI_ENGINEERING_OS_DATA_DIR", &self.root)
            .output()
            .expect("run agentctl")
    }

    fn spawn_agentd(&self) -> Child {
        Command::new(workspace_bin("agentd"))
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", &self.root)
            .env("AI_ENGINEERING_OS_DATA_DIR", &self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agentd")
    }

    fn db_path(&self) -> PathBuf {
        // Mirrors the daemon resolver: override parent +
        // ai-engineering-os/state.sqlite3.
        self.root.join("ai-engineering-os").join("state.sqlite3")
    }

    fn journal(&self) -> Vec<JournalEvent> {
        SqliteStore::open(self.db_path())
            .expect("open test db")
            .list_events_ordered()
            .expect("read journal")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
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

fn wait_ping_ok(sandbox: &Sandbox) {
    wait_for(
        || sandbox.agentctl(&["ping"]).status.success(),
        Duration::from_secs(20),
        "agentctl ping to succeed",
    );
}

/// Starts the daemon through the product flow (`agentctl start` spawns
/// the sibling when genuinely missing) and waits for readiness.
fn start_daemon(sandbox: &Sandbox) {
    let out = sandbox.agentctl(&["start"]);
    assert!(out.status.success(), "start failed: {out:?}");
    wait_ping_ok(sandbox);
}

fn shutdown_daemon(sandbox: &Sandbox) {
    let out = sandbox.agentctl(&["shutdown"]);
    assert!(out.status.success(), "shutdown failed: {out:?}");
}

fn task_state(stdout_line: &str) -> &str {
    // `task <id> <project> <STATE> "<objective>"`
    stdout_line.split_whitespace().nth(3).expect("state field")
}

#[test]
fn project_e2e_with_restart() {
    let sandbox = Sandbox::new("project");
    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["project", "create", "P-1", "Runlith"]);
    assert!(out.status.success(), "create failed: {out:?}");
    assert!(stdout(&out).contains("project P-1"), "got: {out:?}");
    let out = sandbox.agentctl(&["project", "get", "P-1"]);
    assert!(out.status.success(), "get failed: {out:?}");
    assert!(stdout(&out).contains("Runlith"), "got: {out:?}");
    let out = sandbox.agentctl(&["project", "list"]);
    assert!(out.status.success(), "list failed: {out:?}");
    assert!(stdout(&out).contains("P-1"), "got: {out:?}");
    let out = sandbox.agentctl(&["project", "get", "GHOST"]);
    assert!(!out.status.success(), "missing project must fail");
    assert!(stderr(&out).contains("NOT_FOUND"), "got: {out:?}");

    shutdown_daemon(&sandbox);
    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["project", "get", "P-1"]);
    assert!(
        out.status.success(),
        "project must survive restart: {out:?}"
    );
    assert!(stdout(&out).contains("Runlith"), "got: {out:?}");
    shutdown_daemon(&sandbox);
}

#[test]
fn task_e2e_with_restart() {
    let sandbox = Sandbox::new("task");
    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["project", "create", "P-1", "P"]);
    assert!(out.status.success(), "project create failed: {out:?}");
    let out = sandbox.agentctl(&["task", "create", "T-1", "P-1", "ship G2"]);
    assert!(out.status.success(), "task create failed: {out:?}");
    assert!(
        stdout(&out).contains("DRAFT"),
        "new task must be DRAFT: {out:?}"
    );
    let out = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(out.status.success(), "get failed: {out:?}");
    assert_eq!(task_state(stdout(&out).trim()), "DRAFT");
    let out = sandbox.agentctl(&["task", "list", "P-1"]);
    assert!(out.status.success(), "list failed: {out:?}");
    assert!(stdout(&out).contains("T-1"), "got: {out:?}");

    shutdown_daemon(&sandbox);
    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(out.status.success(), "task must survive restart: {out:?}");
    assert_eq!(task_state(stdout(&out).trim()), "DRAFT");
    shutdown_daemon(&sandbox);
}

#[test]
fn dependency_e2e_with_invalid_edges() {
    let sandbox = Sandbox::new("dependency");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["project", "create", "P-2", "Other"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "A", "P-1", "a"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "B", "P-1", "b"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "F", "P-2", "foreign"])
        .status
        .success());

    let out = sandbox.agentctl(&["task", "dependency", "add", "A", "B"]);
    assert!(out.status.success(), "add failed: {out:?}");
    let out = sandbox.agentctl(&["task", "dependency", "list", "A"]);
    assert!(out.status.success(), "list failed: {out:?}");
    assert_eq!(stdout(&out).trim(), "dependency B");

    // Self-edge, duplicate, cross-project, and cycle all fail through the
    // public surface and mutate nothing.
    for args in [
        vec!["task", "dependency", "add", "A", "A"],
        vec!["task", "dependency", "add", "A", "B"],
        vec!["task", "dependency", "add", "A", "F"],
        vec!["task", "dependency", "add", "B", "A"],
    ] {
        let out = sandbox.agentctl(&args);
        assert!(!out.status.success(), "{args:?} must fail, got: {out:?}");
        assert!(stderr(&out).contains("CONFLICT"), "{args:?} got: {out:?}");
    }
    let out = sandbox.agentctl(&["task", "dependency", "add", "A", "GHOST"]);
    assert!(!out.status.success(), "unknown target must fail");
    assert!(stderr(&out).contains("NOT_FOUND"), "got: {out:?}");
    let out = sandbox.agentctl(&["task", "dependency", "list", "A"]);
    assert_eq!(stdout(&out).trim(), "dependency B");

    shutdown_daemon(&sandbox);
    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["task", "dependency", "list", "A"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out).trim(),
        "dependency B",
        "edge must survive restart"
    );
    // Only the valid edge produced an event.
    let added = sandbox
        .journal()
        .into_iter()
        .filter(|event| event.fact.event_type == storage_sqlite::EventType::DependencyAdded)
        .count();
    assert_eq!(added, 1);
    shutdown_daemon(&sandbox);
}

#[test]
fn lifecycle_full_path_e2e() {
    let sandbox = Sandbox::new("lifecycle");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "T-1", "P-1", "work"])
        .status
        .success());
    for (command, state, reason) in [
        ("ready", "READY", ""),
        ("claim", "CLAIMED", ""),
        ("start", "RUNNING", ""),
        ("submit", "SUBMITTED", "diff plus tests"),
        ("review", "AWAITING_REVIEW", ""),
        ("verify-begin", "VERIFYING", ""),
        ("verify-succeed", "COMPLETED", "VERIFIED refs"),
    ] {
        let args: Vec<&str> = if reason.is_empty() {
            vec!["task", command, "T-1"]
        } else {
            vec!["task", command, "T-1", reason]
        };
        let out = sandbox.agentctl(&args);
        assert!(out.status.success(), "{command} failed: {out:?}");
        assert!(
            stdout(&out).contains(state),
            "{command} must reach {state}, got: {out:?}"
        );
    }
    let out = sandbox.agentctl(&["task", "get", "T-1"]);
    assert_eq!(task_state(stdout(&out).trim()), "COMPLETED");
    // Ordered lifecycle events exist, preceded by the creation event.
    let types: Vec<String> = sandbox
        .journal()
        .into_iter()
        .filter(|event| event.fact.task_id.as_ref().map(|id| id.as_str()) == Some("T-1"))
        .map(|event| event.fact.event_type.as_str().to_owned())
        .collect();
    assert_eq!(
        types,
        [
            "task.created",
            "task.marked_ready",
            "task.claimed",
            "task.started",
            "task.submitted",
            "task.routed_to_review",
            "task.verification_began",
            "task.verification_succeeded",
        ]
    );
    shutdown_daemon(&sandbox);
}

#[test]
fn invalid_lifecycle_is_an_atomic_noop() {
    let sandbox = Sandbox::new("invalid");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "T-1", "P-1", "work"])
        .status
        .success());
    let before = sandbox.journal().len();
    let out = sandbox.agentctl(&["task", "verify-succeed", "T-1", "self-report"]);
    assert!(!out.status.success(), "DRAFT verify must fail");
    assert!(stderr(&out).contains("INVALID_TRANSITION"), "got: {out:?}");
    let out = sandbox.agentctl(&["task", "get", "T-1"]);
    assert_eq!(task_state(stdout(&out).trim()), "DRAFT");
    assert_eq!(sandbox.journal().len(), before, "no event may be appended");
    shutdown_daemon(&sandbox);
}

#[test]
fn submit_is_not_completion() {
    let sandbox = Sandbox::new("submit");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "T-1", "P-1", "work"])
        .status
        .success());
    for command in ["ready", "claim", "start"] {
        assert!(sandbox.agentctl(&["task", command, "T-1"]).status.success());
    }
    let out = sandbox.agentctl(&["task", "submit", "T-1", "done?"]);
    assert!(out.status.success(), "submit failed: {out:?}");
    let out = sandbox.agentctl(&["task", "get", "T-1"]);
    assert_eq!(task_state(stdout(&out).trim()), "SUBMITTED");
    shutdown_daemon(&sandbox);
}

#[test]
fn no_generic_state_setter_exists() {
    let sandbox = Sandbox::new("nosetter");
    start_daemon(&sandbox);
    for args in [
        vec!["task", "set-state", "T-1", "COMPLETED"],
        vec!["task", "transition", "T-1", "COMPLETED"],
        vec!["task", "complete", "T-1"],
        vec!["task", "done", "T-1"],
    ] {
        let out = sandbox.agentctl(&args);
        assert!(!out.status.success(), "{args:?} must fail");
        assert!(stderr(&out).contains("usage:"), "{args:?} got: {out:?}");
    }
    shutdown_daemon(&sandbox);
}

#[test]
fn reads_emit_no_mutation_events() {
    let sandbox = Sandbox::new("readonly");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "T-1", "P-1", "work"])
        .status
        .success());
    let before = sandbox.journal().len();
    assert!(sandbox
        .agentctl(&["project", "get", "P-1"])
        .status
        .success());
    assert!(sandbox.agentctl(&["project", "list"]).status.success());
    assert!(sandbox.agentctl(&["task", "get", "T-1"]).status.success());
    assert!(sandbox.agentctl(&["task", "list", "P-1"]).status.success());
    assert!(sandbox
        .agentctl(&["task", "dependency", "list", "T-1"])
        .status
        .success());
    assert_eq!(sandbox.journal().len(), before);
    shutdown_daemon(&sandbox);
}

#[test]
fn business_audit_trail_is_complete_and_ordered() {
    let sandbox = Sandbox::new("audit");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "A", "P-1", "a"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "B", "P-1", "b"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "dependency", "add", "A", "B"])
        .status
        .success());
    assert!(sandbox.agentctl(&["task", "ready", "A"]).status.success());

    let history = sandbox.journal();
    let types: Vec<&str> = history
        .iter()
        .map(|event| event.fact.event_type.as_str())
        .collect();
    assert_eq!(
        types,
        [
            "project.created",
            "task.created",
            "task.created",
            "task.dependency_added",
            "task.marked_ready",
        ]
    );
    // Deterministic order: strictly increasing seq, stable evt-<seq> ids.
    assert!(
        history
            .windows(2)
            .all(|pair| pair[0].seq + 1 == pair[1].seq),
        "journal must be gapless in commit order"
    );
    for event in &history {
        assert_eq!(event.id.to_string(), format!("evt-{}", event.seq));
        assert!(!event.fact.correlation_id.is_empty());
        assert!(!event.fact.causation_id.is_empty());
        assert_ne!(event.fact.correlation_id, event.fact.causation_id);
    }
    // Project-scoped event carries no task reference — never a fake id.
    assert_eq!(history[0].fact.task_id, None);
    shutdown_daemon(&sandbox);
}

#[test]
fn restart_durability_e2e() {
    let sandbox = Sandbox::new("restart");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "A", "P-1", "a"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "B", "P-1", "b"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "dependency", "add", "A", "B"])
        .status
        .success());
    assert!(sandbox.agentctl(&["task", "ready", "A"]).status.success());
    assert!(sandbox.agentctl(&["task", "claim", "A"]).status.success());
    let journal_before = sandbox.journal();
    shutdown_daemon(&sandbox);

    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["project", "get", "P-1"]);
    assert!(out.status.success(), "project must survive: {out:?}");
    let out = sandbox.agentctl(&["task", "get", "A"]);
    assert_eq!(task_state(stdout(&out).trim()), "CLAIMED");
    let out = sandbox.agentctl(&["task", "get", "B"]);
    assert_eq!(task_state(stdout(&out).trim()), "DRAFT");
    let out = sandbox.agentctl(&["task", "dependency", "list", "A"]);
    assert_eq!(stdout(&out).trim(), "dependency B");
    assert_eq!(
        sandbox.journal(),
        journal_before,
        "journal must survive verbatim"
    );
    // The reopened daemon keeps committing with deterministic order.
    assert!(sandbox.agentctl(&["task", "start", "A"]).status.success());
    let history = sandbox.journal();
    assert_eq!(history.len(), journal_before.len() + 1);
    assert_eq!(
        history.last().expect("tip").fact.event_type.as_str(),
        "task.started"
    );
    shutdown_daemon(&sandbox);
}

#[test]
fn kill9_crash_preserves_committed_state() {
    let sandbox = Sandbox::new("crash");
    let mut daemon = sandbox.spawn_agentd();
    wait_ping_ok(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox
        .agentctl(&["task", "create", "T-1", "P-1", "work"])
        .status
        .success());
    assert!(sandbox.agentctl(&["task", "ready", "T-1"]).status.success());
    // Hard crash: SIGKILL gives the daemon no chance to flush anything
    // beyond what SQLite already committed. The socket node lingers as a
    // stale endpoint (expected Unix behavior); restart recovers it.
    daemon.kill().expect("kill daemon");
    let _ = daemon.wait();
    start_daemon(&sandbox);
    let out = sandbox.agentctl(&["project", "get", "P-1"]);
    assert!(
        out.status.success(),
        "project must survive kill -9: {out:?}"
    );
    let out = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(out.status.success(), "task must survive kill -9: {out:?}");
    assert_eq!(task_state(stdout(&out).trim()), "READY");
    assert_eq!(sandbox.journal().len(), 3);
    shutdown_daemon(&sandbox);
}

#[test]
fn v0_client_keeps_g1_surface_and_cannot_run_business() {
    use ipc_contracts::proto::{
        control_request, task_core_request, ControlRequest, CreateProjectRequest, TaskCoreRequest,
    };
    use ipc_transport::platform::unix::UnixEndpoint;
    use prost::Message;

    let sandbox = Sandbox::new("v0compat");
    start_daemon(&sandbox);
    // G1 surface on the v1 daemon: ping and shutdown path stay green.
    // (`agentctl ping` handshakes at protocol 0 by construction.)
    assert!(sandbox.agentctl(&["ping"]).status.success());

    // A v0-negotiated session that smuggles a business frame: the daemon
    // must close the connection without executing anything.
    let endpoint = UnixEndpoint {
        socket_path: sandbox.root.join("ai-engineering-os").join("agentd.sock"),
        runtime_dir: sandbox.root.join("ai-engineering-os"),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let stream = ipc_transport::platform::unix::connect(&endpoint)
            .await
            .expect("connect");
        let mut session = ipc_control::ClientSession::new(
            ipc_transport::FramedTransport::new(stream),
            ipc_control::ClientConfig::new("agentctl"),
        );
        let accepted = session.perform_handshake().await.expect("v0 handshake");
        assert_eq!(accepted.selected_version, 0);
        let mut transport = session.into_inner();
        let frame = bytes::Bytes::from(
            ControlRequest {
                payload: Some(control_request::Payload::TaskCore(TaskCoreRequest {
                    payload: Some(task_core_request::Payload::CreateProject(
                        CreateProjectRequest {
                            id: "SNEAKY".to_owned(),
                            name: "must not exist".to_owned(),
                        },
                    )),
                })),
            }
            .encode_to_vec(),
        );
        transport.write_frame(frame).await.expect("write");
        // No business reply may arrive: the version gate ends the session
        // without answering (a timeout and a closed connection both prove
        // the frame never executed).
        let reply = tokio::time::timeout(Duration::from_secs(5), transport.read_frame()).await;
        assert!(
            reply.is_err() || reply.expect("read outcome").is_err(),
            "v0 business frame must never be answered"
        );
    });
    // The daemon is unharmed and nothing was smuggled in.
    assert!(sandbox.agentctl(&["ping"]).status.success());
    let out = sandbox.agentctl(&["project", "get", "SNEAKY"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("NOT_FOUND"), "got: {out:?}");
    // The v0 handshake request shape itself stays valid on the new daemon
    // (proven above: the live [0]-only handshake negotiated 0).
    shutdown_daemon(&sandbox);
}

#[test]
fn database_file_is_state_sqlite3_distinct_from_socket() {
    // The durable database is a distinct `state.sqlite3` file, never the
    // runtime socket node and never a top-level /tmp file.
    let sandbox = Sandbox::new("datapath");
    start_daemon(&sandbox);
    assert!(sandbox
        .agentctl(&["project", "create", "P-1", "P"])
        .status
        .success());
    assert!(sandbox.db_path().is_file(), "state.sqlite3 must exist");
    assert_eq!(
        sandbox.db_path().file_name().and_then(|n| n.to_str()),
        Some("state.sqlite3")
    );
    let socket = sandbox.root.join("ai-engineering-os").join("agentd.sock");
    assert_ne!(sandbox.db_path(), socket);
    assert_ne!(
        sandbox.db_path().parent().expect("parent"),
        Path::new("/tmp"),
        "database must not sit in /tmp directly"
    );
    shutdown_daemon(&sandbox);
}
