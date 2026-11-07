//! Per-execution workspace-cycle serialization across store
//! connections and processes (TASK-0229).
//!
//! `continue_workspace_cycle` assumes one active cycle per execution:
//! concurrent cycles reconstruct the workspace side-effect ledger from
//! journal snapshots and can poison each other with stale transitions
//! before any attempt exists. Inside one daemon the cancel-registry
//! claim serializes dispatch, but independent compositions (or
//! processes) over one database share no registry. This guard is the
//! durable-boundary equivalent: an OS advisory lock on a per-execution
//! lock file beside the database. The OS releases it on process death
//! (including SIGKILL), so a crashed cycle never wedges the strand its
//! resume is meant to recover.

use std::path::PathBuf;

use agent_runtime::ExecutionId;

use crate::error::{StorageError, StorageResult};

/// Held for the duration of one workspace cycle. Dropping releases the
/// lock; the lock file itself is never unlinked (unlinking would race
/// a contender that already opened it).
#[derive(Debug)]
pub struct ExecutionCycleGuard {
    _file: Option<std::fs::File>,
}

#[cfg(any(unix, windows))]
impl Drop for ExecutionCycleGuard {
    fn drop(&mut self) {
        if let Some(file) = &self._file {
            // Closing the handle would release the lock on its own;
            // unlock explicitly so intent survives refactors.
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                unsafe {
                    libc::flock(file.as_raw_fd(), libc::LOCK_UN);
                }
            }
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawHandle;
                use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
                use windows_sys::Win32::System::IO::OVERLAPPED;
                unsafe {
                    let mut overlapped: OVERLAPPED = std::mem::zeroed();
                    UnlockFileEx(
                        file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
                        0,
                        u32::MAX,
                        u32::MAX,
                        &mut overlapped,
                    );
                }
            }
        }
    }
}

impl crate::task_store::SqliteStore {
    /// Tries to become the one active workspace cycle for `execution`.
    ///
    /// `Ok(Some(guard))` — caller owns the cycle until the guard drops.
    /// `Ok(None)` — another connection or process already owns it; the
    /// caller must refuse without mutating anything. In-memory
    /// databases have no cross-connection existence, so they yield an
    /// uncontended guard.
    pub fn try_acquire_execution_cycle(
        &self,
        execution: &ExecutionId,
    ) -> StorageResult<Option<ExecutionCycleGuard>> {
        let Some(lock_path) = self.cycle_lock_path(execution)? else {
            return Ok(Some(ExecutionCycleGuard {
                #[cfg(any(unix, windows))]
                _file: None,
            }));
        };
        self.acquire_cycle_lock(lock_path)
    }

    fn cycle_lock_path(&self, execution: &ExecutionId) -> StorageResult<Option<PathBuf>> {
        let database_file: Option<String> = self
            .connection()
            .query_row(
                "SELECT file FROM pragma_database_list() WHERE name = 'main'",
                [],
                |row| row.get(0),
            )
            .map_err(StorageError::from)?;
        let Some(database_file) = database_file.filter(|file| !file.is_empty()) else {
            return Ok(None);
        };
        let directory = PathBuf::from(database_file)
            .parent()
            .map(|parent| parent.join("execution-cycle-locks"))
            .ok_or_else(|| {
                StorageError::Corrupt("database path has no parent directory".to_owned())
            })?;
        let sanitized: String = execution
            .as_str()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        Ok(Some(directory.join(format!("{sanitized}.lock"))))
    }

    fn acquire_cycle_lock(&self, lock_path: PathBuf) -> StorageResult<Option<ExecutionCycleGuard>> {
        if let Some(directory) = lock_path.parent() {
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(StorageError::UnsafePath(error.to_string())),
            }
            let metadata = std::fs::symlink_metadata(directory)
                .map_err(|error| StorageError::UnsafePath(error.to_string()))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(StorageError::UnsafePath(
                    "invalid execution lock directory".to_owned(),
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7022 != 0 {
                    return Err(StorageError::UnsafePath(
                        "execution lock directory must be owned by this user and not writable by other users"
                            .to_owned(),
                    ));
                }
            }
        }
        let file = lock_cycle_file(&lock_path)?;
        Ok(file.map(|file| ExecutionCycleGuard { _file: Some(file) }))
    }
}

/// Opens `lock_path` and takes the OS advisory lock without blocking.
/// `Ok(None)` means another holder owns it. Split from the store method
/// so lock semantics are testable on platforms where opening the
/// database itself is not yet implemented (Windows `path_security`).
#[cfg(unix)]
fn lock_cycle_file(lock_path: &std::path::Path) -> StorageResult<Option<std::fs::File>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(lock_path)
        .map_err(|error| {
            StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
        })?;
    let metadata = file
        .metadata()
        .map_err(|error| StorageError::UnsafePath(error.to_string()))?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || metadata.mode() & 0o7133 != 0
    {
        return Err(StorageError::UnsafePath(
            "invalid execution cycle lock file".to_owned(),
        ));
    }
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Ok(None);
        }
        return Err(StorageError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(error)),
        ));
    }
    Ok(Some(file))
}

#[cfg(windows)]
fn lock_cycle_file(lock_path: &std::path::Path) -> StorageResult<Option<std::fs::File>> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_LOCK_VIOLATION};
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| {
            StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
        })?;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let locked = unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        LockFileEx(
            file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if locked == 0 {
        let code = unsafe { GetLastError() };
        if code == ERROR_LOCK_VIOLATION {
            return Ok(None);
        }
        return Err(StorageError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::from_raw_os_error(
                code as i32,
            ))),
        ));
    }
    Ok(Some(file))
}

#[cfg(not(any(unix, windows)))]
fn lock_cycle_file(_lock_path: &std::path::Path) -> StorageResult<Option<std::fs::File>> {
    // No advisory-lock primitive is wired on this platform; fail
    // closed rather than pretend cycles are serialized.
    Err(StorageError::Corrupt(
        "execution cycle serialization is unavailable on this platform".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_directories_and_symlink_or_hardlinked_lock_files() {
        use std::os::unix::fs::{symlink, DirBuilderExt, PermissionsExt};
        let root = std::env::temp_dir().join(format!(
            "runlith-lock-admission-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let store = crate::SqliteStore::open(root.join("state.sqlite3")).unwrap();
        let outside = root.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let locks = root.join("execution-cycle-locks");
        let execution = ExecutionId::new("E-ADMISSION").unwrap();
        symlink(&outside, &locks).unwrap();
        assert!(store.try_acquire_execution_cycle(&execution).is_err());
        assert!(!outside.join("E-ADMISSION.lock").exists());
        std::fs::remove_file(&locks).unwrap();
        std::fs::create_dir(&locks).unwrap();
        let canary = root.join("canary");
        std::fs::write(&canary, b"unchanged").unwrap();
        std::fs::set_permissions(&canary, std::fs::Permissions::from_mode(0o600)).unwrap();
        let lock = locks.join("E-ADMISSION.lock");
        symlink(&canary, &lock).unwrap();
        assert!(store.try_acquire_execution_cycle(&execution).is_err());
        std::fs::remove_file(&lock).unwrap();
        std::fs::hard_link(&canary, &lock).unwrap();
        assert!(store.try_acquire_execution_cycle(&execution).is_err());
        assert_eq!(std::fs::read(&canary).unwrap(), b"unchanged");
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cycle_guard_excludes_second_holder_until_release() {
        let root = std::env::temp_dir().join(format!(
            "runlith-cycle-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp root");
        // Directly at the lock primitive: opening the database itself
        // is not implemented on Windows yet (path_security), which must
        // not mask whether LockFileEx semantics hold there.
        let lock_path = root.join("E-LOCK.lock");

        let first = ExecutionCycleGuard {
            _file: lock_cycle_file(&lock_path)
                .expect("first acquire")
                .expect("first holder wins")
                .into(),
        };
        assert!(
            lock_cycle_file(&lock_path)
                .expect("second acquire")
                .is_none(),
            "second holder must be refused while the first guard lives"
        );

        drop(first);
        assert!(
            lock_cycle_file(&lock_path).expect("reacquire").is_some(),
            "guard must be acquirable after release"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
