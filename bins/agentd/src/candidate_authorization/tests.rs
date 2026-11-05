use super::*;
use std::process::{Command, Output};

fn snapshot_input() -> Vec<u8> {
    let mut input = b"runlith-candidate-snapshot/v1\n".to_vec();
    input.extend_from_slice(&1u32.to_be_bytes());
    input.extend_from_slice(&9u32.to_be_bytes());
    input.extend_from_slice(&0o100644u32.to_be_bytes());
    input.extend_from_slice(&7u64.to_be_bytes());
    input.extend_from_slice(b"proof.txtallowed");
    input
}

fn run_fixture(mut permit: Permit, prefix: Vec<u8>, input: Vec<u8>) -> (Output, io::Result<()>) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "candidate_helper::tests::subprocess",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(permit.take_stdio().unwrap())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        stdin
            .write_all(&prefix)
            .and_then(|()| stdin.write_all(&input))
    });
    let server = std::thread::spawn(move || permit.serve());
    let output = child.wait_with_output().unwrap();
    let _ = writer.join().unwrap();
    (output, server.join().unwrap())
}

fn invoke(args: &[OsString], input: Vec<u8>) -> Output {
    let permit = Permit::issue(args, &input).unwrap();
    let prefix = permit.prefix.clone();
    let (output, result) = run_fixture(permit, prefix, input);
    result.expect("one-shot permit consumed");
    output
}

#[test]
fn authenticated_snapshot_write_evaluate_and_remove() {
    let root = tempfile::tempdir().unwrap();
    let name = "candidate-authorized";
    let args = [
        "__candidate-snapshot-write".into(),
        root.path().into(),
        name.into(),
    ];
    assert!(invoke(&args, snapshot_input()).status.success());
    let snapshot = root.path().join("candidate-snapshots").join(name);
    assert_eq!(
        std::fs::read(snapshot.join("proof.txt")).unwrap(),
        b"allowed"
    );
    let manifest = evaluation::CandidateManifest::from_files(&[evaluation::CandidateFile {
        path: b"proof.txt".to_vec(),
        mode: 0o100644,
        git_oid: None,
        bytes: b"allowed".to_vec(),
    }])
    .unwrap();
    let evaluate = [
        "__candidate-evaluate".into(),
        snapshot.into(),
        "candidate.file-contains.v1".into(),
        manifest.sha256.into(),
        "proof.txt".into(),
        "allowed".into(),
    ];
    let evaluated = invoke(&evaluate, Vec::new());
    assert!(evaluated.status.success());
    assert!(String::from_utf8(evaluated.stdout)
        .unwrap()
        .contains("passed=1\nfailed=0\nskipped=0\n"));
    let remove = [
        "__candidate-snapshot-remove".into(),
        root.path().into(),
        name.into(),
    ];
    assert!(invoke(&remove, Vec::new()).status.success());
    assert!(!root.path().join("candidate-snapshots").join(name).exists());
}

#[test]
fn nonce_argument_and_input_tampering_cannot_write_snapshot() {
    for damage in ["nonce", "argument", "input"] {
        let root = tempfile::tempdir().unwrap();
        let args = [
            "__candidate-snapshot-write".into(),
            root.path().into(),
            "candidate-authorized".into(),
        ];
        let mut input = snapshot_input();
        let permit = Permit::issue(&args, &input).unwrap();
        let mut prefix = permit.prefix.clone();
        match damage {
            "nonce" => prefix[HEADER.len()] ^= 1,
            "argument" => *prefix.last_mut().unwrap() = b'x',
            "input" => *input.last_mut().unwrap() = b'x',
            _ => unreachable!(),
        }
        let (output, result) = run_fixture(permit, prefix, input);
        assert!(!output.status.success(), "accepted tampered {damage}");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(!root.path().join("candidate-snapshots").exists());
    }
}

#[test]
fn permit_expires_and_cannot_be_replayed() {
    let args = [
        "__candidate-snapshot-remove".into(),
        "/tmp".into(),
        "candidate-unused".into(),
    ];
    let mut expired = Permit::issue(&args, &[]).unwrap();
    expired.deadline = Instant::now();
    assert_eq!(expired.serve().unwrap_err().kind(), io::ErrorKind::TimedOut);

    let mut permit = Permit::issue(&args, &[]).unwrap();
    let request = permit.expected;
    let mut client = permit.child.take().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let server = std::thread::spawn(move || permit.serve());
    client.write_all(&request).unwrap();
    let mut reply = [0];
    client.read_exact(&mut reply).unwrap();
    assert_eq!(reply, [1]);
    server.join().unwrap().unwrap();
    let _ = client.write_all(&request);
    assert_eq!(client.read(&mut reply).unwrap(), 0);
}
