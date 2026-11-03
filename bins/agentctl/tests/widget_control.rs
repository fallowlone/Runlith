//! A slow widget draft must not block the daemon: `connection_status` opens a
//! new session, and that session has to stay quick while the provider runs.
//!
//! The fixture makes the `--version` probe take seconds, so the draft is really
//! running on its own thread inside a real daemon on a real socket.
#![cfg(unix)]

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ipc_contracts::proto::WidgetCoreResponse;
use ipc_transport::platform::unix::{UnixEndpoint, SOCKET_FILE_NAME};

const SLOW_PROVIDER: &[u8] = b"#!/bin/sh\nsleep 8\necho '2.1.316 (Claude Code)'\n";
const OBSERVE_FOR: Duration = Duration::from_secs(3);
/// The desktop host's `RESPONSE_TIMEOUT` is 5 s; a healthy session is far below it.
const SESSION_BOUND: Duration = Duration::from_secs(1);

fn no_spawn() -> io::Result<std::process::Child> {
    Err(io::Error::other("the test daemon is already running"))
}

async fn request(endpoint: &UnixEndpoint, args: &[&str]) -> WidgetCoreResponse {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (parsed, _) = agentctl::parse_widget_args(&args).expect("widget arguments");
    agentctl::widget_request(endpoint, no_spawn, parsed)
        .await
        .expect("widget request")
}

#[tokio::test]
async fn connection_status_stays_quick_during_a_long_draft() {
    let root = PathBuf::from("/tmp").join(format!("rl-wc-{}", std::process::id()));
    // Neither run/ nor db/ is created here: the daemon owns both and refuses
    // pre-existing ones.
    std::fs::create_dir_all(&root).expect("sandbox");
    let endpoint = UnixEndpoint {
        socket_path: root.join("run").join(SOCKET_FILE_NAME),
        runtime_dir: root.join("run"),
    };
    let provider = root.join("claude-slow.sh");
    std::fs::write(&provider, SLOW_PROVIDER).expect("fixture");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let config = agentd::DaemonConfig::with_execution(root.join("db").join("state.sqlite3"))
        .with_provider_executable(provider);
    let daemon = {
        let endpoint = endpoint.clone();
        tokio::spawn(async move { agentd::run_on_endpoint(&endpoint, config).await })
    };
    let ready = Instant::now();
    while agentctl::connect_and_handshake(&endpoint).await.is_err() {
        assert!(
            ready.elapsed() < Duration::from_secs(10),
            "daemon not ready"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let asked = Instant::now();
    let started = request(&endpoint, &["start", "сколько звёзд"]).await;
    assert!(
        asked.elapsed() < SESSION_BOUND,
        "start waited for the provider"
    );
    assert!(agentctl::widget_is_running(&started), "{started:?}");

    let mut slowest = Duration::ZERO;
    let mut sessions = 0;
    let observing = Instant::now();
    while observing.elapsed() < OBSERVE_FOR {
        let opened = Instant::now();
        agentctl::connect_and_handshake(&endpoint)
            .await
            .expect("a new session during the draft");
        slowest = slowest.max(opened.elapsed());
        sessions += 1;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(slowest < SESSION_BOUND, "slowest session took {slowest:?}");
    assert!(
        sessions >= 10,
        "only {sessions} sessions in {OBSERVE_FOR:?}"
    );

    let status = agentctl::widget_status_request(&started);
    let still = agentctl::widget_request(&endpoint, no_spawn, status)
        .await
        .expect("status during the draft");
    assert!(
        agentctl::widget_is_running(&still),
        "the draft ended early: {still:?}"
    );

    daemon.abort();
    let _ = std::fs::remove_dir_all(&root);
}
