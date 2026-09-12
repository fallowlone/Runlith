//! ClaudeExecutor participation in the shared `ExecutionPort` contract
//! suite (TASK-0049, G5).
//!
//! The same [`execution_contract::run_execution_port_contract`] every
//! provider must pass, driven here by the real `ClaudeExecutor` against
//! deterministic fixture executables (no Claude subscription, no
//! network, no auth). Claude-specific behavior (argv, version policy,
//! parser battery, supervision timing) stays in the existing unit and
//! process-fixture suites; this file answers only: does the
//! Claude-backed executor satisfy the common execution seam?

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use execution_contract::{run_execution_port_contract, ContractExecutors};
use provider_claude::{ClaudeConfig, ClaudeExecutor};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Case {
    dir: PathBuf,
}

impl Case {
    fn named(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-claude-contract-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        Self { dir }
    }

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
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
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
        "contract prompt",
        workdir.to_path_buf(),
        workdir.parent().unwrap().join("provider-env"),
    )
    .unwrap()
}

#[test]
fn claude_executor_satisfies_the_shared_execution_port_contract() {
    let case = Case::named("seam");
    let workdir = case.workdir();
    let timeout = Duration::from_secs(30);

    let success = executor_for(
        &case.fixture(
            "success.sh",
            r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"contract evidence","session_id":"cccccccc-cccc-cccc-cccc-cccccccccccc"}'"#,
        ),
        &workdir,
        timeout,
    );
    let failure = executor_for(
        &case.fixture("failure.sh", "echo 'contract failure' >&2\nexit 3\n"),
        &workdir,
        timeout,
    );
    let malformed = executor_for(
        &case.fixture("malformed.sh", "printf 'not provider json{{{'\n"),
        &workdir,
        timeout,
    );
    // Hanging fixture with a short bound: must report canonical
    // `TimedOut` deterministically (the sleep never exits on its own).
    let hang = executor_for(
        &case.fixture("hang.sh", "sleep 60\n"),
        &workdir,
        Duration::from_secs(1),
    );

    run_execution_port_contract(&ContractExecutors {
        success: &success,
        failure: &failure,
        malformed: &malformed,
        timeout: &hang,
    });
}
