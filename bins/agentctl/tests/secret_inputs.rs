#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::process::{Command, Stdio};

fn binary() -> std::path::PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.join("agentctl")
}

#[test]
fn worker_sign_accepts_private_file_and_stdin_and_rejects_raw_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.key");
    let key = "ab".repeat(32);
    let nonce = "cd".repeat(32);
    let job = worker::mint::mint_job_id(1000, 1).to_string();
    let expected = agentctl::worker_sign(&key, &nonce, &job).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    writeln!(file, "{key}").unwrap();
    let output = Command::new(binary())
        .args(["worker", "sign", "--secret-file"])
        .arg(&path)
        .args(["--nonce", &nonce, "--job", &job])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    let mut child = Command::new(binary())
        .args([
            "worker",
            "sign",
            "--secret-stdin",
            "--nonce",
            &nonce,
            "--job",
            &job,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{key}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    let refused = Command::new(binary())
        .args([
            "worker", "sign", "--secret", &key, "--nonce", &nonce, "--job", &job,
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(&key));
    assert!(agentctl::worker_sign(&key, &"\u{1f600}".repeat(16), &job).is_err());
}
