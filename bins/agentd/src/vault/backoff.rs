use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{wire, VaultError, VaultErrorCode, WireError};

#[derive(Clone, Copy, Default)]
pub(super) struct State {
    pub failures: u32,
    pub retry_at_ms: u64,
}

pub(super) struct Guard {
    lock: File,
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // A flock belongs to the open file description, which a concurrent
        // fork shares with its child until exec closes the copy. Closing our
        // descriptor alone would leave the lock held for that whole window, so
        // release it explicitly. Failure is harmless: closing still releases
        // it once the last copy goes away.
        let _ = self.lock.unlock();
    }
}

impl Guard {
    pub fn open(vault: &Path) -> Result<Self, WireError> {
        admit_parent(vault.parent().unwrap_or_else(|| Path::new(".")))
            .map_err(|_| unavailable())?;
        let lock_path = sidecar(vault, ".unlock.lock");
        let mut options = private_options().map_err(|_| unavailable())?;
        let (lock, created) = match options.write(true).create_new(true).open(&lock_path) {
            Ok(file) => (file, true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (
                options
                    .create_new(false)
                    .open(&lock_path)
                    .map_err(|_| unavailable())?,
                false,
            ),
            Err(_) => return Err(unavailable()),
        };
        admit_file(&lock).map_err(|_| unavailable())?;
        // A concurrent owner is refused after a short bounded retry, so an
        // owner that is just finishing does not read as a real concurrent
        // attempt. (A forked child holding our descriptor is handled by the
        // explicit unlock in `Drop`, not by this wait.)
        // ponytail: fixed 10 x 10 ms wait, event-driven wakeup if it matters.
        let mut attempts = 0;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if attempts < 10 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(TryLockError::WouldBlock) => return Err(limited(1000)),
                Err(TryLockError::Error(_)) => return Err(unavailable()),
            }
        }
        let guard = Self {
            lock,
            path: sidecar(vault, ".unlock-state"),
        };
        match guard.read_state() {
            Ok(_) => {}
            Err(error) if created && error.kind() == io::ErrorKind::NotFound => {
                guard.save(State::default())?
            }
            Err(_) => return Err(unavailable()),
        }
        Ok(guard)
    }

    pub fn load(&self) -> Result<State, WireError> {
        self.read_state().map_err(|_| unavailable())
    }

    fn read_state(&self) -> io::Result<State> {
        let file = private_options()?.open(&self.path)?;
        admit_file(&file)?;
        let mut bytes = Vec::with_capacity(21);
        file.take(21).read_to_end(&mut bytes)?;
        let bytes: [u8; 20] = bytes
            .try_into()
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        if &bytes[..8] != b"RLVBAK01" {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(State {
            failures: u32::from_be_bytes(
                bytes[8..12]
                    .try_into()
                    .map_err(|_| io::ErrorKind::InvalidData)?,
            ),
            retry_at_ms: u64::from_be_bytes(
                bytes[12..20]
                    .try_into()
                    .map_err(|_| io::ErrorKind::InvalidData)?,
            ),
        })
    }

    pub fn save(&self, state: State) -> Result<(), WireError> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let temp = sidecar(
            &self.path,
            &format!(
                ".tmp-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
        );
        let mut bytes = [0; 20];
        bytes[..8].copy_from_slice(b"RLVBAK01");
        bytes[8..12].copy_from_slice(&state.failures.to_be_bytes());
        bytes[12..20].copy_from_slice(&state.retry_at_ms.to_be_bytes());
        let mut file = private_options()
            .map_err(|_| unavailable())?
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| unavailable())?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            File::open(self.path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()
        })();
        if result.is_err() {
            match fs::remove_file(&temp) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => eprintln!(
                    "agentd: vault backoff staging cleanup failed ({:?})",
                    error.kind()
                ),
            }
        }
        result.map_err(|_: io::Error| unavailable())
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(unix)]
fn private_options() -> io::Result<OpenOptions> {
    use std::os::unix::fs::OpenOptionsExt;
    let flags = i32::try_from((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits())
        .map_err(|_| io::ErrorKind::Unsupported)?;
    let mut options = OpenOptions::new();
    options.read(true).mode(0o600).custom_flags(flags);
    Ok(options)
}

#[cfg(not(unix))]
fn private_options() -> io::Result<OpenOptions> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(unix)]
fn admit_file(file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != ipc_transport::platform::unix::current_uid()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn admit_file(_: &File) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(unix)]
fn admit_parent(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != ipc_transport::platform::unix::current_uid()
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn admit_parent(_: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

pub(super) fn limited(delay: u64) -> WireError {
    WireError {
        code: VaultErrorCode::VaultErrorRateLimited as i32,
        message: "too many failed attempts; retry later".to_owned(),
        retry_after_ms: Some(delay),
    }
}

fn unavailable() -> WireError {
    wire(VaultError::Io)
}

#[cfg(all(test, unix))]
mod tests;
