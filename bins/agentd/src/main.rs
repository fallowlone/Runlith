//! `agentd` entry point: thin CLI shell over the G2 control + business server.
//!
//! Running `agentd` resolves the canonical durable database path, opens
//! (creating/migrating) the store, and starts the daemon on its canonical
//! endpoint. An already-running compatible daemon produces a
//! distinguishable AlreadyRunning result (exit code 2). A database or
//! data-directory failure fails clearly without serving. Exit codes are
//! G1-local, not a permanent standard.

use std::process::ExitCode;

use agentd::{DaemonConfig, DaemonError, RunOutcome};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "__candidate-snapshot-write")
    {
        return candidate_snapshot_write(&args[1..]);
    }
    if args
        .first()
        .is_some_and(|arg| arg == "__candidate-evaluate")
    {
        return candidate_evaluate(&args[1..]);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("usage: agentd");
        println!("  Starts the local G2 daemon on its canonical endpoint.");
        println!("  The durable database resolves per ADR-0021 section 15;");
        println!("  AI_ENGINEERING_OS_DATA_DIR overrides the data parent for tests.");
        return ExitCode::SUCCESS;
    }
    if !args.is_empty() {
        eprintln!("agentd: unexpected arguments (usage: agentd)");
        return ExitCode::FAILURE;
    }
    let database_path = match agentd::resolve_database_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("agentd: cannot resolve durable database: {error}");
            return ExitCode::FAILURE;
        }
    };
    match agentd::run(DaemonConfig::with_execution(database_path)).await {
        Ok(RunOutcome::ShutdownCompleted { connections_served }) => {
            println!("agentd: shutdown complete (connections served: {connections_served})");
            ExitCode::SUCCESS
        }
        Err(DaemonError::AlreadyRunning) => {
            eprintln!("agentd: already running (endpoint owned)");
            ExitCode::from(2)
        }
        Err(error) => {
            eprintln!("agentd: failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn candidate_snapshot_write(args: &[String]) -> ExitCode {
    use std::io::Read;
    if args.len() != 2 {
        return ExitCode::FAILURE;
    }
    let parent = std::path::Path::new(&args[0]);
    if !args[1].starts_with("candidate-")
        || args[1].len() > 96
        || !args[1]
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return ExitCode::FAILURE;
    }
    let mut input = Vec::new();
    if std::io::stdin()
        .take(evaluation::MAX_CANDIDATE_BLOB_BYTES + 64 * 1024 * 1024 + 1)
        .read_to_end(&mut input)
        .is_err()
        || input.len() as u64 > evaluation::MAX_CANDIDATE_BLOB_BYTES + 64 * 1024 * 1024
    {
        return ExitCode::FAILURE;
    }
    let files = match decode_snapshot_input(&input) {
        Ok(files) => files,
        Err(()) => return ExitCode::FAILURE,
    };
    match effect_capture_local::materialize_candidate_snapshot(parent, &args[1], &files) {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn candidate_evaluate(args: &[String]) -> ExitCode {
    if args.len() != 5 || args[1] != "candidate.file-contains.v1" {
        return ExitCode::FAILURE;
    }
    let root = std::path::Path::new(&args[0]);
    let snapshot = match effect_capture_local::read_candidate_snapshot(root) {
        Ok(snapshot) => snapshot,
        Err(_) => return ExitCode::FAILURE,
    };
    let manifest = match evaluation::CandidateManifest::from_files(&snapshot.files) {
        Ok(manifest) => manifest,
        Err(_) => return ExitCode::FAILURE,
    };
    let expected_directories = manifest_directories(&manifest.entries);
    if manifest.sha256 != args[2] || snapshot.directories != expected_directories {
        return ExitCode::FAILURE;
    }
    let result =
        match evaluation::CandidateTestResult::file_contains(&snapshot.files, &args[3], &args[4]) {
            Ok(result) => result,
            Err(_) => return ExitCode::FAILURE,
        };
    print!("{}", result.encode(&manifest.sha256));
    ExitCode::SUCCESS
}

fn manifest_directories(entries: &[evaluation::CandidateManifestEntry]) -> Vec<Vec<u8>> {
    let mut directories = std::collections::BTreeSet::new();
    for entry in entries {
        let mut path = entry.path.as_slice();
        while let Some(index) = path.iter().rposition(|byte| *byte == b'/') {
            directories.insert(path[..index].to_vec());
            path = &path[..index];
        }
    }
    directories.into_iter().collect()
}

fn decode_snapshot_input(input: &[u8]) -> Result<Vec<evaluation::CandidateFile>, ()> {
    const HEADER: &[u8] = b"runlith-candidate-snapshot/v1\n";
    if !input.starts_with(HEADER) {
        return Err(());
    }
    let mut cursor = HEADER.len();
    let count = take_u32(input, &mut cursor)? as usize;
    if count > evaluation::MAX_CANDIDATE_ENTRIES {
        return Err(());
    }
    let mut files = Vec::with_capacity(count);
    let mut total = 0u64;
    for _ in 0..count {
        let path_len = take_u32(input, &mut cursor)? as usize;
        let mode = take_u32(input, &mut cursor)?;
        let length = take_u64(input, &mut cursor)?;
        if path_len == 0 || path_len > 4096 || length > evaluation::MAX_CANDIDATE_BLOB_BYTES {
            return Err(());
        }
        total = total.checked_add(length).ok_or(())?;
        if total > evaluation::MAX_CANDIDATE_BLOB_BYTES {
            return Err(());
        }
        let path_end = cursor.checked_add(path_len).ok_or(())?;
        let data_end = path_end
            .checked_add(usize::try_from(length).map_err(|_| ())?)
            .ok_or(())?;
        if data_end > input.len() {
            return Err(());
        }
        files.push(evaluation::CandidateFile {
            path: input[cursor..path_end].to_vec(),
            mode,
            git_oid: None,
            bytes: input[path_end..data_end].to_vec(),
        });
        cursor = data_end;
    }
    if cursor != input.len() {
        return Err(());
    }
    Ok(files)
}

fn take_u32(input: &[u8], cursor: &mut usize) -> Result<u32, ()> {
    let end = cursor.checked_add(4).ok_or(())?;
    let bytes: [u8; 4] = input
        .get(*cursor..end)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())?;
    *cursor = end;
    Ok(u32::from_be_bytes(bytes))
}

fn take_u64(input: &[u8], cursor: &mut usize) -> Result<u64, ()> {
    let end = cursor.checked_add(8).ok_or(())?;
    let bytes: [u8; 8] = input
        .get(*cursor..end)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())?;
    *cursor = end;
    Ok(u64::from_be_bytes(bytes))
}
