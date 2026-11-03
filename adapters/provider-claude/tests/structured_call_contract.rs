//! TASK-0230: the structured call passes the schema and role, removes every
//! tool, returns the answer untouched and reports each failure by type. A
//! fixture executable stands in for the provider.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use provider_claude::{CancelFlag, ClaudeConfig, ClaudeFailure, ClaudeStructuredCall};

const ROLE: &str = "Classify the request";
const SCHEMA: &str = r#"{"type":"object"}"#;

struct Case {
    dir: PathBuf,
}

impl Case {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rl-structured-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).expect("workdir");
        Self { dir }
    }

    fn fixture(&self, body: &str) -> PathBuf {
        let path = self.dir.join("claude-fixture");
        let mut file = std::fs::File::create(&path).expect("fixture");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(body.as_bytes()).expect("body");
        drop(file);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn call(&self, fixture: &Path, timeout: Duration) -> ClaudeStructuredCall {
        let config = ClaudeConfig::default()
            .with_executable(fixture)
            .expect("executable")
            .with_timeout(timeout)
            .expect("timeout");
        ClaudeStructuredCall::bind_trusted_host(
            config,
            ROLE,
            SCHEMA,
            self.dir.join("work"),
            self.dir.join("provider-env"),
        )
        .expect("bind")
    }
}

impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn answers(result: &str) -> String {
    let escaped = result.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"printf '%s' '{{"type":"result","subtype":"success","is_error":false,"result":"{escaped}","session_id":"s"}}'
"#
    )
}

#[test]
fn passes_schema_and_role_with_no_tools_and_returns_the_answer_untouched() {
    let case = Case::new("argv");
    let argv_file = case.dir.join("argv.txt");
    let body = format!(
        "for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\n{}",
        argv_file.display(),
        answers(r#"{"outcome":"refused"}"#)
    );
    let call = case.call(&case.fixture(&body), Duration::from_secs(10));
    assert_eq!(
        call.run("покажи погоду").expect("answer"),
        r#"{"outcome":"refused"}"#
    );
    let argv = std::fs::read_to_string(argv_file).expect("argv");
    let lines: Vec<&str> = argv.lines().collect();
    let after = |flag: &str| {
        let at = lines.iter().position(|l| *l == flag);
        at.map(|i| lines[i + 1])
            .unwrap_or_else(|| panic!("{flag} missing"))
    };
    assert_eq!(after("--json-schema"), SCHEMA);
    assert_eq!(after("--system-prompt"), ROLE);
    assert_eq!(after("--disallowedTools"), "mcp__*");
    assert!(lines.contains(&"--tools"), "tools flag missing");
    assert_eq!(
        lines.last().copied(),
        Some("покажи погоду"),
        "the description stays the final positional value"
    );
}

#[test]
fn an_unattached_call_fails_closed_before_any_child_exists() {
    let case = Case::new("closed");
    let marker = case.dir.join("ran");
    let fixture = case.fixture(&format!("touch '{}'\n", marker.display()));
    let config = ClaudeConfig::default()
        .with_executable(&fixture)
        .expect("executable");
    let call = ClaudeStructuredCall::bind(
        config,
        ROLE,
        SCHEMA,
        case.dir.join("work"),
        case.dir.join("provider-env"),
    )
    .expect("bind");
    assert!(matches!(
        call.run("звёзды"),
        Err(ClaudeFailure::LaunchUnmanaged { .. })
    ));
    assert!(!marker.exists(), "the provider must not have started");
}

#[test]
fn a_slow_provider_times_out_and_is_killed() {
    let case = Case::new("timeout");
    let call = case.call(&case.fixture("sleep 30\n"), Duration::from_millis(200));
    let started = Instant::now();
    assert!(matches!(
        call.run("звёзды"),
        Err(ClaudeFailure::TimedOut { .. })
    ));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn cancelling_kills_the_provider_child() {
    let case = Case::new("cancel");
    let cancel = CancelFlag::fresh();
    let call = case
        .call(&case.fixture("sleep 30\n"), Duration::from_secs(60))
        .with_cancel_flag(cancel.clone());
    let worker = std::thread::spawn(move || call.run("звёзды"));
    std::thread::sleep(Duration::from_millis(200));
    cancel.cancel();
    let started = Instant::now();
    assert!(matches!(
        worker.join().expect("worker"),
        Err(ClaudeFailure::Cancelled)
    ));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn output_that_is_not_a_result_object_is_a_typed_failure() {
    let case = Case::new("malformed");
    let call = case.call(
        &case.fixture("printf 'not json'\n"),
        Duration::from_secs(10),
    );
    assert!(matches!(
        call.run("звёзды"),
        Err(ClaudeFailure::MalformedOutput { .. })
    ));
}
