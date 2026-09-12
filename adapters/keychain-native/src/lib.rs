//! Local OS credential-store adapter for TASK-0063.
//!
//! `keyring` selects the platform-native store: Apple Keychain on macOS,
//! Windows Credential Manager on Windows and Secret Service on Linux/*nix.
//! There is no file fallback and no cloud backend.

use std::fmt;

use keyring::{Entry, Error as KeyringError};
use secrets::{SecretBackend, SecretBackendError, SecretRef};

const SERVICE_NAME: &str = "runlith";

trait CredentialDriver: Send + Sync {
    fn store(&self, account: &str, value: &[u8]) -> Result<(), SecretBackendError>;
    fn load(&self, account: &str) -> Result<Vec<u8>, SecretBackendError>;
    fn delete(&self, account: &str) -> Result<(), SecretBackendError>;
}

struct PlatformCredentialDriver;

impl PlatformCredentialDriver {
    fn entry(account: &str) -> Result<Entry, SecretBackendError> {
        Entry::new(SERVICE_NAME, account).map_err(map_keyring_error)
    }
}

impl CredentialDriver for PlatformCredentialDriver {
    fn store(&self, account: &str, value: &[u8]) -> Result<(), SecretBackendError> {
        Self::entry(account)?
            .set_secret(value)
            .map_err(map_keyring_error)
    }

    fn load(&self, account: &str) -> Result<Vec<u8>, SecretBackendError> {
        Self::entry(account)?
            .get_secret()
            .map_err(map_keyring_error)
    }

    fn delete(&self, account: &str) -> Result<(), SecretBackendError> {
        Self::entry(account)?
            .delete_credential()
            .map_err(map_keyring_error)
    }
}

/// Cross-platform native credential-store adapter.
pub struct NativeKeychain {
    driver: Box<dyn CredentialDriver>,
}

impl NativeKeychain {
    #[must_use]
    pub fn new() -> Self {
        Self {
            driver: Box::new(PlatformCredentialDriver),
        }
    }

    /// Initializes the platform store without reading or writing a credential.
    pub fn probe() -> Result<(), SecretBackendError> {
        Entry::store_status()
            .as_ref()
            .map(|_| ())
            .map_err(map_keyring_error_ref)
    }

    #[cfg(test)]
    fn with_driver(driver: impl CredentialDriver + 'static) -> Self {
        Self {
            driver: Box::new(driver),
        }
    }
}

impl Default for NativeKeychain {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NativeKeychain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeKeychain")
            .field("service", &SERVICE_NAME)
            .finish_non_exhaustive()
    }
}

impl SecretBackend for NativeKeychain {
    fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError> {
        self.driver.store(secret.as_str(), value)
    }

    fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
        self.driver.load(secret.as_str())
    }

    fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError> {
        self.driver.delete(secret.as_str())
    }
}

fn map_keyring_error(error: KeyringError) -> SecretBackendError {
    match error {
        KeyringError::NoEntry => SecretBackendError::NotFound,
        KeyringError::NoDefaultStore => SecretBackendError::Unavailable,
        _ => SecretBackendError::Rejected,
    }
}

fn map_keyring_error_ref(error: &KeyringError) -> SecretBackendError {
    match error {
        KeyringError::NoEntry => SecretBackendError::NotFound,
        KeyringError::NoDefaultStore => SecretBackendError::Unavailable,
        _ => SecretBackendError::Rejected,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    fn test_material(seed: u8) -> Vec<u8> {
        (0_u8..32)
            .map(|value| value.wrapping_mul(17).wrapping_add(seed))
            .collect()
    }

    #[derive(Default)]
    struct MemoryDriver {
        values: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl CredentialDriver for MemoryDriver {
        fn store(&self, account: &str, value: &[u8]) -> Result<(), SecretBackendError> {
            self.values
                .lock()
                .unwrap()
                .insert(account.to_owned(), value.to_vec());
            Ok(())
        }

        fn load(&self, account: &str) -> Result<Vec<u8>, SecretBackendError> {
            self.values
                .lock()
                .unwrap()
                .get(account)
                .cloned()
                .ok_or(SecretBackendError::NotFound)
        }

        fn delete(&self, account: &str) -> Result<(), SecretBackendError> {
            if self.values.lock().unwrap().remove(account).is_some() {
                Ok(())
            } else {
                Err(SecretBackendError::NotFound)
            }
        }
    }

    #[test]
    fn native_adapter_contract_store_load_delete_is_exact_by_ref() {
        let backend = NativeKeychain::with_driver(MemoryDriver::default());
        let first = SecretRef::new("credential-a").unwrap();
        let second = SecretRef::new("credential-b").unwrap();
        let first_material = test_material(11);
        let second_material = test_material(29);

        backend.store(&first, &first_material).unwrap();
        backend.store(&second, &second_material).unwrap();

        assert_eq!(backend.load(&first).unwrap(), first_material);
        assert_eq!(backend.load(&second).unwrap(), second_material);
        backend.delete(&first).unwrap();
        assert_eq!(backend.load(&first), Err(SecretBackendError::NotFound));
        assert_eq!(backend.load(&second).unwrap(), second_material);
    }

    #[test]
    fn adapter_debug_and_errors_do_not_render_secret_material() {
        let backend = NativeKeychain::with_driver(MemoryDriver::default());
        let secret = SecretRef::new("credential-a").unwrap();
        let material = test_material(41);
        backend.store(&secret, &material).unwrap();

        let rendered = format!("{backend:?} {}", SecretBackendError::Rejected);
        assert!(!rendered
            .as_bytes()
            .windows(material.len())
            .any(|window| window == material));
    }

    #[test]
    fn keyring_error_mapping_is_redacted_and_fail_closed() {
        assert_eq!(
            map_keyring_error(KeyringError::NoEntry),
            SecretBackendError::NotFound
        );
        assert_eq!(
            map_keyring_error(KeyringError::NoDefaultStore),
            SecretBackendError::Unavailable
        );
    }
}
