#![cfg(target_os = "macos")]

#[test]
fn direct_candidate_snapshot_write_has_no_filesystem_authority() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let root = tempfile::tempdir().unwrap();
    let mut input = b"runlith-candidate-snapshot/v1\n".to_vec();
    input.extend_from_slice(&1u32.to_be_bytes());
    input.extend_from_slice(&9u32.to_be_bytes());
    input.extend_from_slice(&0o100644u32.to_be_bytes());
    input.extend_from_slice(&7u64.to_be_bytes());
    input.extend_from_slice(b"proof.txtallowed");
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentd"))
        .arg("__candidate-snapshot-write")
        .arg(root.path())
        .arg("candidate-direct")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(&input);
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "direct helper accepted an ungoverned write"
    );
    assert!(!root
        .path()
        .join("candidate-snapshots/candidate-direct")
        .exists());
}

#[test]
fn caller_forged_authorization_channel_cannot_write_snapshot() {
    use std::io::{Read, Write};
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let args = [
        "__candidate-snapshot-write",
        root.path().to_str().unwrap(),
        "candidate-forged",
    ];
    let mut input = b"runlith-candidate-launch/v1\n".to_vec();
    input.extend_from_slice(&[7; 32]);
    input.push(args.len() as u8);
    for arg in &args {
        input.extend_from_slice(&(arg.len() as u32).to_be_bytes());
        input.extend_from_slice(arg.as_bytes());
    }
    input.extend_from_slice(b"runlith-candidate-snapshot/v1\n");
    input.extend_from_slice(&1u32.to_be_bytes());
    input.extend_from_slice(&9u32.to_be_bytes());
    input.extend_from_slice(&0o100644u32.to_be_bytes());
    input.extend_from_slice(&7u64.to_be_bytes());
    input.extend_from_slice(b"proof.txtallowed");
    let (mut server, client) = UnixStream::pair().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let client: OwnedFd = client.into();
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentd"))
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(client))
        .spawn()
        .unwrap();
    let issuer = std::thread::spawn(move || {
        let mut request = [0; 64];
        if server.read_exact(&mut request).is_ok() {
            let _ = server.write_all(&[1]);
        }
    });
    let _ = child.stdin.take().unwrap().write_all(&input);
    let output = child.wait_with_output().unwrap();
    issuer.join().unwrap();
    assert!(
        !output.status.success(),
        "helper trusted a caller-created permit issuer"
    );
    assert!(!root.path().join("candidate-snapshots").exists());
}

#[test]
fn direct_candidate_evaluation_and_removal_are_denied() {
    use std::process::{Command, Stdio};

    let root = tempfile::tempdir().unwrap();
    let files = [evaluation::CandidateFile {
        path: b"proof.txt".to_vec(),
        mode: 0o100644,
        git_oid: None,
        bytes: b"allowed".to_vec(),
    }];
    let snapshot = effect_capture_local::materialize_candidate_snapshot(
        root.path(),
        "candidate-private",
        &files,
    )
    .unwrap();
    let manifest = evaluation::CandidateManifest::from_files(&files).unwrap();
    for args in [
        vec![
            "__candidate-evaluate",
            snapshot.to_str().unwrap(),
            "candidate.file-contains.v1",
            &manifest.sha256,
            "proof.txt",
            "allowed",
        ],
        vec![
            "__candidate-snapshot-remove",
            root.path().to_str().unwrap(),
            "candidate-private",
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agentd"))
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(
            std::fs::read(snapshot.join("proof.txt")).unwrap(),
            b"allowed"
        );
    }
}
