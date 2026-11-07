use std::io::{BufRead, Read};

use zeroize::Zeroizing;

pub fn read_key_input(args: &[String], file_flag: &str) -> Result<Zeroizing<String>, String> {
    if args
        .iter()
        .any(|arg| arg == "--secret" || arg == "--org-key")
    {
        return Err(
            "raw key arguments are unsupported; use a private key file or stdin".to_owned(),
        );
    }
    let stdin_flag = file_flag.replace("-file", "-stdin");
    let stdin = args.iter().filter(|arg| *arg == &stdin_flag).count();
    let files: Vec<_> = args
        .windows(2)
        .filter(|pair| pair[0] == file_flag)
        .collect();
    let selectors = args.iter().filter(|arg| arg.as_str() == file_flag).count();
    if stdin + selectors != 1 || selectors != files.len() {
        return Err(format!(
            "use exactly one of {file_flag} <path> or {stdin_flag}"
        ));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    if stdin == 1 {
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() {
            return Err(
                "key stdin requires a pipe; use a private key file for interactive input"
                    .to_owned(),
            );
        }
        if args
            .iter()
            .filter(|arg| arg.as_str() == "--secret-stdin" || arg.as_str() == "--org-key-stdin")
            .count()
            != 1
        {
            return Err(
                "only one key may come from stdin; use a file for the other key".to_owned(),
            );
        }
        std::io::stdin()
            .lock()
            .take(67)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "could not read key from stdin".to_owned())?;
    } else {
        read_private_file(std::path::Path::new(&files[0][1]), &mut bytes)?;
    }
    if bytes.len() > 66 {
        return Err("key must be 64 hex characters".to_owned());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "key must be 64 hex characters".to_owned())?
        .trim_end_matches(['\r', '\n']);
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("key must be 64 hex characters".to_owned());
    }
    Ok(Zeroizing::new(text.to_owned()))
}

#[cfg(unix)]
fn read_private_file(path: &std::path::Path, bytes: &mut Vec<u8>) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let flags = i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits())
        .map_err(|_| "private key file flags are unavailable".to_owned())?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(path)
        .map_err(|_| "cannot open private key file".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect private key file".to_owned())?;
    if !metadata.is_file()
        || metadata.uid() != ipc_transport::platform::unix::current_uid()
        || metadata.nlink() != 1
        || !matches!(metadata.mode() & 0o7777, 0o600 | 0o400)
        || metadata.len() > 66
    {
        return Err(
            "key file must be a private regular file owned by this user (mode 0600 or 0400)"
                .to_owned(),
        );
    }
    file.take(67)
        .read_to_end(bytes)
        .map_err(|_| "cannot read private key file".to_owned())?;
    Ok(())
}

#[cfg(not(unix))]
fn read_private_file(_: &std::path::Path, _: &mut Vec<u8>) -> Result<(), String> {
    Err("private key file admission is unavailable on this platform".to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};

    #[test]
    fn reads_private_keys_and_refuses_exposed_or_malformed_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        let marker = "ab".repeat(32);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        writeln!(file, "{marker}").unwrap();
        let args = vec![
            "--secret-file".to_owned(),
            path.to_string_lossy().into_owned(),
        ];
        assert!(!args.iter().any(|arg| arg.contains(&marker)));
        assert_eq!(&*read_key_input(&args, "--secret-file").unwrap(), &marker);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_key_input(&args, "--secret-file").is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        file.set_len(1024).unwrap();
        assert!(read_key_input(&args, "--secret-file").is_err());
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(read_key_input(
            &[
                "--secret-file".to_owned(),
                link.to_string_lossy().into_owned()
            ],
            "--secret-file"
        )
        .is_err());
        assert!(read_key_input(&["--secret".to_owned(), marker.clone()], "--secret-file").is_err());
        assert!(read_key_input(
            &["--secret-stdin".to_owned(), "--org-key-stdin".to_owned()],
            "--secret-file"
        )
        .is_err());
    }
}
