use std::process::ExitCode;

pub(crate) fn run(args: &[String]) -> ExitCode {
    match super::candidate_authorization::authenticate(Some(args)) {
        Ok((authorized_args, input)) => dispatch(&authorized_args, &input),
        Err(_) => ExitCode::FAILURE,
    }
}

fn dispatch(args: &[String], input: &[u8]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("__candidate-snapshot-write") => candidate_snapshot_write(&args[1..], input),
        Some("__candidate-snapshot-remove") if input.is_empty() => {
            candidate_snapshot_remove(&args[1..])
        }
        Some("__candidate-evaluate") if input.is_empty() => candidate_evaluate(&args[1..]),
        _ => ExitCode::FAILURE,
    }
}

fn candidate_snapshot_write(args: &[String], input: &[u8]) -> ExitCode {
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
    let files = match decode_snapshot_input(input) {
        Ok(files) => files,
        Err(()) => return ExitCode::FAILURE,
    };
    match effect_capture_local::materialize_candidate_snapshot(parent, &args[1], &files) {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn candidate_snapshot_remove(args: &[String]) -> ExitCode {
    if args.len() != 2
        || !args[1].starts_with("candidate-")
        || args[1].len() > 96
        || !args[1]
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return ExitCode::FAILURE;
    }
    match effect_capture_local::remove_candidate_snapshot(std::path::Path::new(&args[0]), &args[1])
    {
        Ok(()) => ExitCode::SUCCESS,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    #[ignore = "subprocess fixture; invoked only by authorization tests"]
    fn subprocess() {
        let result = super::super::candidate_authorization::authenticate(None);
        let (args, input) = match result {
            Ok(value) => value,
            Err(_) => std::process::exit(1),
        };
        std::io::stdout()
            .write_all(super::super::candidate_authorization::OUTPUT_MARKER)
            .unwrap();
        std::io::stdout().flush().unwrap();
        let status = dispatch(&args, &input);
        std::io::stdout().flush().unwrap();
        std::process::exit(if status == ExitCode::SUCCESS { 0 } else { 1 });
    }
}
