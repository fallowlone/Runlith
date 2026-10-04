//! `agentctl vault` secret commands refuse a non-terminal stdin before any
//! daemon contact: no secret is read from a pipe, and no daemon is started.

#![cfg(unix)]

use std::process::{Command, Stdio};

const TEST_RUNTIME_ENV: &str = "AI_ENGINEERING_OS_RUNTIME_DIR";
const TEST_DATA_ENV: &str = "AI_ENGINEERING_OS_DATA_DIR";

#[test]
fn secret_commands_refuse_a_piped_stdin_before_contacting_the_daemon() {
    let runtime = tempfile::tempdir().expect("runtime dir");
    let data = tempfile::tempdir().expect("data dir");

    for command in [
        "init",
        "unlock",
        "store",
        "change-password",
        "recover",
        "rotate",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agentctl"))
            .args(["vault", command])
            .env(TEST_RUNTIME_ENV, runtime.path())
            .env(TEST_DATA_ENV, data.path())
            .stdin(Stdio::piped())
            .output()
            .expect("run agentctl");

        assert!(!output.status.success(), "vault {command} must refuse");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("stdin is not a terminal"),
            "vault {command}: {stderr}"
        );
    }
    let runtime_entries = std::fs::read_dir(runtime.path()).expect("list").count();
    let data_entries = std::fs::read_dir(data.path()).expect("list").count();
    assert_eq!(runtime_entries, 0, "no daemon endpoint was created");
    assert_eq!(data_entries, 0, "no daemon data was created");
}

#[test]
fn secrets_are_never_accepted_as_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_agentctl"))
        .args(["vault", "store", "sk-ant-oat01-argv-value"])
        .stdin(Stdio::null())
        .output()
        .expect("run agentctl");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: agentctl vault"));
}
