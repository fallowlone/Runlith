//! F6: a daemon that dies during startup must leave its real error visible.
//! Single test in this binary because it sets process-wide env overrides.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use agentctl::{ping, spawn_daemon, ClientError};

#[tokio::test(flavor = "current_thread")]
async fn early_exit_reports_the_daemon_failure_before_the_startup_timeout() {
    // Short base path: the unix socket path length is bounded.
    let base = tempfile::Builder::new()
        .prefix("rl")
        .tempdir_in("/tmp")
        .unwrap();
    let data_parent = base.path().join("d");
    let data_dir = data_parent.join("ai-engineering-os");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    // A file that is not a SQLite database makes agentd fail at store open.
    let db = data_dir.join("state.sqlite3");
    std::fs::write(&db, b"this is not a sqlite database, at all, no header").unwrap();
    std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::env::set_var("AI_ENGINEERING_OS_DATA_DIR", &data_parent);
    std::env::set_var("AI_ENGINEERING_OS_RUNTIME_DIR", base.path().join("r"));
    std::fs::create_dir_all(base.path().join("r")).unwrap();

    // target/debug/deps/<this test> -> target/debug/agentd (build bins first).
    let exe = std::env::current_exe().unwrap();
    let agentd = exe.parent().unwrap().parent().unwrap().join("agentd");
    assert!(agentd.exists(), "build agentd first: {}", agentd.display());

    let endpoint = agentctl::resolve_production_endpoint().unwrap();
    let started = std::time::Instant::now();
    let error = ping(&endpoint, || spawn_daemon(&agentd))
        .await
        .expect_err("daemon cannot start");

    let ClientError::SpawnFailed(detail) = error else {
        panic!("expected SpawnFailed, got {error:?}");
    };
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(detail.contains("agentd: failed"), "{detail}");
    assert!(detail.lines().count() <= 22, "{detail}");
}
