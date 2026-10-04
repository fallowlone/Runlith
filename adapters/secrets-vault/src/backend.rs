//! [`SecretBackend`] over the vault file with an explicit lock.
//!
//! Only the data key is held in memory, and only while unlocked. Every
//! operation re-reads the file; every change is written atomically.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use secrets::{SecretBackend, SecretBackendError, SecretRef};
use zeroize::Zeroizing;

use crate::envelope::{random_array, Dek, Entries, Entry, Envelope, KdfParams};
use crate::{Password, RecoveryPhrase, VaultError};

/// The v1 format tops out near 16.8 MiB; anything larger is not a vault.
const MAX_FILE_LEN: u64 = 32 * 1024 * 1024;

pub struct VaultBackend {
    path: PathBuf,
    dek: Mutex<Option<Dek>>,
}

impl VaultBackend {
    /// Writes a new vault and returns it unlocked with its recovery phrase,
    /// which the caller shows to the operator once. Refuses an existing file.
    pub fn create(
        path: impl Into<PathBuf>,
        password: &Password,
        word_count: usize,
    ) -> Result<(Self, RecoveryPhrase), VaultError> {
        Self::create_with_params(path.into(), password, word_count, KdfParams::V1)
    }

    fn create_with_params(
        path: PathBuf,
        password: &Password,
        word_count: usize,
        params: KdfParams,
    ) -> Result<(Self, RecoveryPhrase), VaultError> {
        let phrase = RecoveryPhrase::generate(word_count)?;
        let (envelope, dek) = Envelope::create(password, &phrase, params)?;
        write_new_file(&path, &envelope.to_bytes()).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                VaultError::AlreadyExists
            } else {
                VaultError::Io
            }
        })?;
        sync_parent(&path)?;
        let vault = Self {
            path,
            dek: Mutex::new(Some(dek)),
        };
        Ok((vault, phrase))
    }

    /// Opens an existing vault, locked.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, VaultError> {
        let path = path.into();
        read_envelope(&path)?;
        Ok(Self {
            path,
            dek: Mutex::new(None),
        })
    }

    pub fn unlock(&self, password: &Password) -> Result<(), VaultError> {
        let mut state = self.state();
        let dek = read_envelope(&self.path)?.unlock_with_password(password)?;
        *state = Some(dek);
        Ok(())
    }

    /// Unlocks with the recovery phrase and replaces the password.
    pub fn recover(&self, phrase: &RecoveryPhrase, password: &Password) -> Result<(), VaultError> {
        let mut state = self.state();
        let envelope = read_envelope(&self.path)?;
        let dek = envelope.unlock_with_recovery(phrase)?;
        replace_file(
            &self.path,
            &envelope.rewrap_password(&dek, password)?.to_bytes(),
        )?;
        *state = Some(dek);
        Ok(())
    }

    /// Replaces the password. Requires the current password even when
    /// unlocked.
    pub fn change_password(&self, current: &Password, new: &Password) -> Result<(), VaultError> {
        let mut state = self.state();
        let envelope = read_envelope(&self.path)?;
        let dek = envelope.unlock_with_password(current)?;
        replace_file(&self.path, &envelope.rewrap_password(&dek, new)?.to_bytes())?;
        *state = Some(dek);
        Ok(())
    }

    /// Drops (and so zeroizes) the data key.
    pub fn lock(&self) {
        *self.state() = None;
    }

    pub fn is_unlocked(&self) -> bool {
        self.state().is_some()
    }

    /// The key state stays coherent after a panic elsewhere (a single
    /// assignment), so a poisoned lock is still usable; `lock` must never
    /// fail to drop the key.
    fn state(&self) -> MutexGuard<'_, Option<Dek>> {
        self.dek.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Read-modify-write of the entries under the state lock, so writes from
    /// this process are serialized.
    fn update(
        &self,
        change: impl FnOnce(Entries) -> Result<Entries, SecretBackendError>,
    ) -> Result<(), SecretBackendError> {
        let state = self.state();
        let dek = state.as_ref().ok_or(SecretBackendError::Locked)?;
        let envelope = read_envelope(&self.path).map_err(backend_error)?;
        let entries = change(envelope.entries(dek).map_err(backend_error)?)?;
        let updated = envelope
            .with_entries(dek, &entries)
            .map_err(backend_error)?;
        replace_file(&self.path, &updated.to_bytes()).map_err(backend_error)
    }
}

impl SecretBackend for VaultBackend {
    fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError> {
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SecretBackendError::Failure)?
            .as_secs();
        self.update(|entries| {
            let mut kept: Vec<Entry> = entries
                .0
                .into_iter()
                .filter(|entry| entry.name != secret.as_str())
                .collect();
            kept.push(Entry {
                name: secret.as_str().to_owned(),
                created_at,
                value: Zeroizing::new(value.to_vec()),
            });
            Ok(Entries(kept))
        })
    }

    fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
        let state = self.state();
        let dek = state.as_ref().ok_or(SecretBackendError::Locked)?;
        let entries = read_envelope(&self.path)
            .and_then(|envelope| envelope.entries(dek))
            .map_err(backend_error)?;
        entries
            .0
            .iter()
            .find(|entry| entry.name == secret.as_str())
            .map(|entry| entry.value.to_vec())
            .ok_or(SecretBackendError::NotFound)
    }

    fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError> {
        self.update(|entries| {
            if !entries.0.iter().any(|entry| entry.name == secret.as_str()) {
                return Err(SecretBackendError::NotFound);
            }
            let kept = entries
                .0
                .into_iter()
                .filter(|entry| entry.name != secret.as_str())
                .collect();
            Ok(Entries(kept))
        })
    }
}

fn backend_error(error: VaultError) -> SecretBackendError {
    match error {
        VaultError::Locked => SecretBackendError::Locked,
        VaultError::Corrupt => SecretBackendError::Corrupt,
        VaultError::EntryRejected => SecretBackendError::Rejected,
        VaultError::NotFound | VaultError::Unavailable => SecretBackendError::Unavailable,
        VaultError::Io
        | VaultError::WeakPassword
        | VaultError::InvalidPhrase
        | VaultError::InvalidParams
        | VaultError::UnlockFailed
        | VaultError::AlreadyExists => SecretBackendError::Failure,
    }
}

fn read_envelope(path: &Path) -> Result<Envelope, VaultError> {
    let file = File::open(path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            VaultError::NotFound
        } else {
            VaultError::Io
        }
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_LEN + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| VaultError::Io)?;
    if bytes.len() as u64 > MAX_FILE_LEN {
        return Err(VaultError::Corrupt);
    }
    Envelope::parse(&bytes)
}

/// Atomic replace: private temp file in the same directory, flushed, renamed
/// over the vault, then the directory entry flushed.
fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let suffix: String = random_array::<8>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".tmp-{suffix}"));
    let temp = PathBuf::from(temp);
    let result = write_new_file(&temp, bytes)
        .and_then(|()| fs::rename(&temp, path))
        .map_err(|_| VaultError::Io)
        .and_then(|()| sync_parent(path));
    if result.is_err() {
        // Best effort: the original error is what the caller needs.
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Creates `path` (never replacing), owner read/write only, and flushes it.
/// A partial file is removed on failure.
#[cfg(unix)]
fn write_new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

/// No owner-only file mode is enforced off unix, so writes fail closed
/// (same rule as the durable store admission, F12).
#[cfg(not(unix))]
fn write_new_file(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

fn sync_parent(path: &Path) -> Result<(), VaultError> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| VaultError::Io)
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;

    const MATERIAL: &[u8] = b"material-0123456789";

    fn password(value: &str) -> Password {
        Password::new(value.to_owned()).unwrap()
    }

    fn secret(name: &str) -> SecretRef {
        SecretRef::new(name).unwrap()
    }

    fn create(path: &Path) -> (VaultBackend, RecoveryPhrase) {
        VaultBackend::create_with_params(
            path.to_owned(),
            &password("correct-horse-battery"),
            crate::DEFAULT_PHRASE_WORDS,
            KdfParams::new(8, 1, 1).unwrap(),
        )
        .unwrap()
    }

    fn directory_names(path: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn vault_satisfies_the_shared_secret_backend_contract() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _phrase) = create(&dir.path().join("vault"));
        secret_backend_contract::run_secret_backend_contract(&vault);
    }

    #[test]
    fn create_returns_an_unlocked_vault_with_a_default_phrase_and_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");

        let (vault, phrase) = create(&path);

        assert!(vault.is_unlocked());
        assert_eq!(phrase.word_count(), crate::DEFAULT_PHRASE_WORDS);
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn create_refuses_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        fs::write(&path, b"existing").unwrap();

        let error = VaultBackend::create_with_params(
            path.clone(),
            &password("correct-horse-battery"),
            crate::DEFAULT_PHRASE_WORDS,
            KdfParams::new(8, 1, 1).unwrap(),
        )
        .err()
        .unwrap();

        assert_eq!(error, VaultError::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"existing");
    }

    #[test]
    fn open_of_a_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            VaultBackend::open(dir.path().join("vault")).err(),
            Some(VaultError::NotFound)
        );
    }

    #[test]
    fn opened_vault_is_locked_and_refuses_every_operation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, _phrase) = create(&path);
        vault.store(&secret("claude-token"), MATERIAL).unwrap();

        let reopened = VaultBackend::open(&path).unwrap();

        assert!(!reopened.is_unlocked());
        let name = secret("claude-token");
        assert_eq!(reopened.load(&name), Err(SecretBackendError::Locked));
        assert_eq!(reopened.store(&name, b"x"), Err(SecretBackendError::Locked));
        assert_eq!(reopened.delete(&name), Err(SecretBackendError::Locked));
    }

    #[test]
    fn unlock_requires_the_right_password_and_persists_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, _phrase) = create(&path);
        vault.store(&secret("claude-token"), MATERIAL).unwrap();
        let reopened = VaultBackend::open(&path).unwrap();

        assert_eq!(
            reopened.unlock(&password("incorrect-horse")),
            Err(VaultError::UnlockFailed)
        );
        assert!(!reopened.is_unlocked());
        reopened.unlock(&password("correct-horse-battery")).unwrap();

        assert_eq!(reopened.load(&secret("claude-token")).unwrap(), MATERIAL);
    }

    #[test]
    fn store_load_overwrite_and_delete_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _phrase) = create(&dir.path().join("vault"));
        let name = secret("claude-token");

        assert_eq!(vault.load(&name), Err(SecretBackendError::NotFound));
        vault.store(&name, b"first-value").unwrap();
        vault.store(&secret("other"), b"other-value").unwrap();
        vault.store(&name, b"second-value").unwrap();
        assert_eq!(vault.load(&name).unwrap(), b"second-value");

        vault.delete(&name).unwrap();
        assert_eq!(vault.load(&name), Err(SecretBackendError::NotFound));
        assert_eq!(vault.delete(&name), Err(SecretBackendError::NotFound));
        assert_eq!(vault.load(&secret("other")).unwrap(), b"other-value");
    }

    #[test]
    fn lock_refuses_further_access() {
        let dir = tempfile::tempdir().unwrap();
        let (vault, _phrase) = create(&dir.path().join("vault"));
        vault.store(&secret("claude-token"), MATERIAL).unwrap();

        vault.lock();

        assert!(!vault.is_unlocked());
        assert_eq!(
            vault.load(&secret("claude-token")),
            Err(SecretBackendError::Locked)
        );
    }

    #[test]
    fn recovery_phrase_replaces_a_forgotten_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, phrase) = create(&path);
        vault.store(&secret("claude-token"), MATERIAL).unwrap();
        let reopened = VaultBackend::open(&path).unwrap();

        reopened
            .recover(&phrase, &password("a-new-long-password"))
            .unwrap();

        assert!(reopened.is_unlocked());
        assert_eq!(reopened.load(&secret("claude-token")).unwrap(), MATERIAL);
        let again = VaultBackend::open(&path).unwrap();
        assert_eq!(
            again.unlock(&password("correct-horse-battery")),
            Err(VaultError::UnlockFailed)
        );
        again.unlock(&password("a-new-long-password")).unwrap();
    }

    #[test]
    fn change_password_requires_the_current_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, _phrase) = create(&path);
        let new = password("a-new-long-password");

        assert_eq!(
            vault.change_password(&password("incorrect-horse"), &new),
            Err(VaultError::UnlockFailed)
        );
        vault
            .change_password(&password("correct-horse-battery"), &new)
            .unwrap();

        let reopened = VaultBackend::open(&path).unwrap();
        reopened.unlock(&new).unwrap();
    }

    #[test]
    fn oversized_value_is_rejected_and_the_file_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, _phrase) = create(&path);
        let before = fs::read(&path).unwrap();

        let result = vault.store(&secret("claude-token"), &vec![0; 64 * 1024 + 1]);

        assert_eq!(result, Err(SecretBackendError::Rejected));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn tampered_file_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, _phrase) = create(&path);
        vault.store(&secret("claude-token"), MATERIAL).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&path, &bytes).unwrap();

        assert_eq!(
            vault.load(&secret("claude-token")),
            Err(SecretBackendError::Corrupt)
        );
    }

    #[test]
    fn writes_leave_no_temporary_files_and_no_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault");
        let (vault, _phrase) = create(&path);

        vault.store(&secret("claude-token"), MATERIAL).unwrap();
        vault.store(&secret("claude-token"), MATERIAL).unwrap();

        assert_eq!(directory_names(&path), ["vault"]);
        let bytes = fs::read(&path).unwrap();
        assert!(!bytes
            .windows(MATERIAL.len())
            .any(|window| window == MATERIAL));
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
