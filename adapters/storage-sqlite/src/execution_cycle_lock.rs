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
    #[cfg(any(unix, windows))]
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
                unsafe {
                    UnlockFileEx(
                        file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
                        0,
                        u32::MAX,
                        u32::MAX,
                        std::ptr::null_mut(),
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

    #[cfg(unix)]
    fn acquire_cycle_lock(&self, lock_path: PathBuf) -> StorageResult<Option<ExecutionCycleGuard>> {
        use std::os::unix::io::AsRawFd;

        if let Some(directory) = lock_path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| {
                StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
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
        Ok(Some(ExecutionCycleGuard { _file: Some(file) }))
    }

    #[cfg(windows)]
    fn acquire_cycle_lock(&self, lock_path: PathBuf) -> StorageResult<Option<ExecutionCycleGuard>> {
        use std::os::windows::io::AsRawHandle;

        use windows_sys::Win32::Foundation::{GetLastError, ERROR_LOCK_VIOLATION};
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };

        if let Some(directory) = lock_path.parent() {
            std::fs::create_dir_all(directory).map_err(|error| {
                StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                StorageError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
            })?;
        let locked = unsafe {
            LockFileEx(
                file.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                std::ptr::null_mut(),
            )
        };
        if locked == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_LOCK_VIOLATION {
                return Ok(None);
            }
            return Err(StorageError::Sqlite(
                rusqlite::Error::ToSqlConversionFailure(Box::new(
                    std::io::Error::from_raw_os_error(code as i32),
                )),
            ));
        }
        Ok(Some(ExecutionCycleGuard { _file: Some(file) }))
    }

    #[cfg(not(any(unix, windows)))]
    fn acquire_cycle_lock(
        &self,
        _lock_path: PathBuf,
    ) -> StorageResult<Option<ExecutionCycleGuard>> {
        // No advisory-lock primitive is wired on this platform; fail
        // closed rather than pretend cycles are serialized.
        Err(StorageError::Corrupt(
            "execution cycle serialization is unavailable on this platform".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let db = root.join("state.sqlite3");
        let execution = ExecutionId::new("E-LOCK").expect("execution id");

        let first_store = crate::task_store::SqliteStore::open(&db).expect("open first");
        let first = first_store
            .try_acquire_execution_cycle(&execution)
            .expect("first acquire")
            .expect("first holder wins");

        let second_store = crate::task_store::SqliteStore::open(&db).expect("open second");
        assert!(
            second_store
                .try_acquire_execution_cycle(&execution)
                .expect("second acquire")
                .is_none(),
            "second holder must be refused while the first guard lives"
        );

        drop(first);
        assert!(
            second_store
                .try_acquire_execution_cycle(&execution)
                .expect("reacquire")
                .is_some(),
            "guard must be acquirable after release"
        );

        drop(second_store);
        drop(first_store);
        let _ = std::fs::remove_dir_all(&root);
    }
}
