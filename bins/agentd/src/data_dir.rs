//! Durable application-data location for the G2 database (ADR-0021 §15–§17).
//!
//! The canonical database is `state.sqlite3` inside the internal
//! `ai-engineering-os` application directory:
//!
//! ```text
//! macOS:   ~/Library/Application Support/ai-engineering-os/state.sqlite3
//! Linux:   ${XDG_DATA_HOME}/ai-engineering-os/state.sqlite3
//!          fallback ~/.local/share/ai-engineering-os/state.sqlite3
//! Windows: %LOCALAPPDATA%\ai-engineering-os\state.sqlite3
//! ```
//!
//! The IPC runtime (socket/pipe) directory is explicitly rejected for
//! durable data, as are `/tmp`, the current working directory, any
//! repository directory, and beside-the-executable paths: those are
//! produced by the resolver below, never accepted as input.
//!
//! `AI_ENGINEERING_OS_DATA_DIR`, when set to an absolute path, overrides
//! the parent under which the application directory is resolved, so
//! real-process tests run against a test-owned database without touching
//! user data. Admission ([`ensure_data_dir`]) applies identically to
//! overridden and canonical paths: the override selects *which*
//! directory, never *whether* it is secure. Production never sets it.

use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::{DaemonError, DaemonResult};

/// Internal application directory name, independent of any marketing brand.
pub const APP_DATA_DIR_NAME: &str = "ai-engineering-os";

/// Canonical database filename inside the application directory.
pub const DATABASE_FILE_NAME: &str = "state.sqlite3";

/// Environment override for the data parent directory (test seam).
pub const DATA_DIR_OVERRIDE_ENV: &str = "AI_ENGINEERING_OS_DATA_DIR";

/// Required mode of the application-owned data directory on Unix.
const DATA_DIR_MODE: u32 = 0o700;

/// Resolves the canonical application data directory for this
/// platform/user, or the test override when set. Fails closed when no
/// location can be determined (missing home, relative override, unknown
/// platform configuration) — callers must not invent a fallback.
pub fn resolve_data_dir() -> DaemonResult<PathBuf> {
    if let Some(parent) = data_dir_override() {
        return Ok(parent.join(APP_DATA_DIR_NAME));
    }
    #[cfg(target_os = "macos")]
    {
        let home = home_dir()?;
        Ok(home
            .join("Library")
            .join("Application Support")
            .join(APP_DATA_DIR_NAME))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            let path = PathBuf::from(xdg);
            if !path.is_absolute() {
                return Err(DaemonError::DataDir(
                    "XDG_DATA_HOME is not absolute; refusing to resolve data directory".to_owned(),
                ));
            }
            return Ok(path.join(APP_DATA_DIR_NAME));
        }
        let home = home_dir()?;
        Ok(home.join(".local").join("share").join(APP_DATA_DIR_NAME))
    }
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty());
        match local {
            Some(dir) => Ok(PathBuf::from(dir).join(APP_DATA_DIR_NAME)),
            None => Err(DaemonError::DataDir(
                "LOCALAPPDATA is not set; refusing to resolve data directory".to_owned(),
            )),
        }
    }
}

/// Resolves the canonical database file path: data directory plus
/// `state.sqlite3`.
pub fn resolve_database_path() -> DaemonResult<PathBuf> {
    Ok(resolve_data_dir()?.join(DATABASE_FILE_NAME))
}

/// Explicit absolute-parent override for tests (see
/// [`DATA_DIR_OVERRIDE_ENV`]). Relative, empty, or missing values fall
/// back to canonical resolution.
fn data_dir_override() -> Option<PathBuf> {
    let dir = std::env::var_os(DATA_DIR_OVERRIDE_ENV)?;
    if dir.is_empty() {
        return None;
    }
    let path = PathBuf::from(dir);
    if !path.is_absolute() {
        return None;
    }
    Some(path)
}

/// Current-user home directory from the environment. Missing or empty
/// values fail closed: no fallback to `/tmp`, the working directory, or
/// any other writable location is acceptable for durable data.
#[cfg(unix)]
fn home_dir() -> DaemonResult<PathBuf> {
    match std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        Some(home) => Ok(PathBuf::from(home)),
        None => Err(DaemonError::DataDir(
            "HOME is not set; refusing to resolve data directory".to_owned(),
        )),
    }
}

/// Creates the application-owned data directory when absent and admits
/// an existing one only when it is owned by the current effective UID,
/// is not a symlink, is a directory, and has exactly mode `0700`.
/// Anything else fails closed: this function never chmods, repairs, or
/// replaces unexpected durable user-data paths — unlike stale-socket
/// recovery, destructive cleanup of a database directory is never safe.
/// (On Windows the current-user LocalAppData boundary plus normal ACL
/// inheritance applies; mode enforcement is Unix-only.)
pub fn ensure_data_dir(dir: &Path) -> DaemonResult<()> {
    match fs::symlink_metadata(dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(dir).map_err(|error| match error.kind() {
                io::ErrorKind::PermissionDenied => DaemonError::DataDir(format!(
                    "cannot create data directory {}: permission denied",
                    dir.display()
                )),
                io::ErrorKind::NotFound => DaemonError::DataDir(format!(
                    "data parent for {} does not exist",
                    dir.display()
                )),
                _ => DaemonError::Io(error),
            })?;
            #[cfg(unix)]
            {
                // Harden against umask: enforce 0700 explicitly, then
                // re-validate.
                fs::set_permissions(dir, fs::Permissions::from_mode(DATA_DIR_MODE))?;
            }
            validate_data_dir(dir)
        }
        Err(error) => Err(DaemonError::Io(error)),
        Ok(_) => validate_data_dir(dir),
    }
}

#[cfg(unix)]
fn validate_data_dir(dir: &Path) -> DaemonResult<()> {
    // `symlink_metadata` does not follow symlinks: a symlink is observed
    // as a symlink and rejected, never traversed.
    let metadata = fs::symlink_metadata(dir)?;
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return Err(DaemonError::DataDir(format!(
            "data directory {} is a symlink; refusing to follow",
            dir.display()
        )));
    }
    if !file_type.is_dir() {
        return Err(DaemonError::DataDir(format!(
            "data path {} exists but is not a directory",
            dir.display()
        )));
    }
    if metadata.uid() != ipc_transport::platform::unix::current_uid() {
        return Err(DaemonError::DataDir(format!(
            "data directory {} is not owned by the current user",
            dir.display()
        )));
    }
    if metadata.mode() & 0o7777 != DATA_DIR_MODE {
        return Err(DaemonError::DataDir(format!(
            "data directory {} must have mode 0700",
            dir.display()
        )));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_data_dir(dir: &Path) -> DaemonResult<()> {
    let metadata = fs::symlink_metadata(dir).map_err(DaemonError::Io)?;
    if !metadata.file_type().is_dir() {
        return Err(DaemonError::DataDir(format!(
            "data path {} exists but is not a directory",
            dir.display()
        )));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn scratch_root(suite: &str) -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root =
            PathBuf::from("/tmp").join(format!("add-data-{}-{suite}-{id}", std::process::id()));
        fs::create_dir(&root).expect("scratch root");
        root
    }

    fn dir_mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).expect("stat").mode() & 0o7777
    }

    #[test]
    fn override_selects_test_data_dir() {
        let _guard = ENV_GUARD.lock().expect("env guard");
        let root = scratch_root("override");
        let previous = std::env::var_os(DATA_DIR_OVERRIDE_ENV);
        unsafe {
            std::env::set_var(DATA_DIR_OVERRIDE_ENV, &root);
        }
        let resolved = resolve_data_dir();
        match previous {
            Some(value) => unsafe { std::env::set_var(DATA_DIR_OVERRIDE_ENV, value) },
            None => unsafe { std::env::remove_var(DATA_DIR_OVERRIDE_ENV) },
        }
        let dir = resolved.expect("resolve");
        assert_eq!(dir, root.join(APP_DATA_DIR_NAME));
        assert_eq!(
            resolve_database_path_for(&dir)
                .file_name()
                .and_then(|n| n.to_str()),
            Some(DATABASE_FILE_NAME)
        );
        let _ = fs::remove_dir_all(&root);
    }

    fn resolve_database_path_for(dir: &Path) -> PathBuf {
        dir.join(DATABASE_FILE_NAME)
    }

    #[test]
    fn absent_directory_is_created_securely() {
        let root = scratch_root("create");
        let dir = root.join(APP_DATA_DIR_NAME);
        ensure_data_dir(&dir).expect("create");
        assert!(dir.is_dir());
        assert_eq!(dir_mode(&dir), 0o700);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn existing_safe_directory_is_accepted() {
        let root = scratch_root("accept");
        let dir = root.join(APP_DATA_DIR_NAME);
        fs::create_dir(&dir).expect("setup");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("setup");
        ensure_data_dir(&dir).expect("accept");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn symlink_is_rejected_without_following() {
        let root = scratch_root("symlink");
        let target = root.join("target");
        fs::create_dir(&target).expect("setup");
        let link = root.join(APP_DATA_DIR_NAME);
        std::os::unix::fs::symlink(&target, &link).expect("setup symlink");
        match ensure_data_dir(&link) {
            Err(DaemonError::DataDir(_)) => {}
            other => panic!("expected DataDir rejection, got {other:?}"),
        }
        assert!(fs::symlink_metadata(&link)
            .expect("stat")
            .file_type()
            .is_symlink());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn regular_file_where_directory_expected_is_rejected() {
        let root = scratch_root("file");
        let file = root.join(APP_DATA_DIR_NAME);
        fs::write(&file, b"precious, not a dir").expect("setup");
        match ensure_data_dir(&file) {
            Err(DaemonError::DataDir(_)) => {}
            other => panic!("expected DataDir rejection, got {other:?}"),
        }
        // Rejected, never repaired or replaced.
        assert_eq!(fs::read(&file).expect("read"), b"precious, not a dir");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn insecure_mode_is_rejected_without_repair() {
        let root = scratch_root("mode");
        let dir = root.join(APP_DATA_DIR_NAME);
        fs::create_dir(&dir).expect("setup");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("setup");
        match ensure_data_dir(&dir) {
            Err(DaemonError::DataDir(_)) => {}
            other => panic!("expected DataDir rejection, got {other:?}"),
        }
        assert_eq!(dir_mode(&dir), 0o755, "rejected, not repaired");
        let _ = fs::remove_dir_all(&root);
    }
}
