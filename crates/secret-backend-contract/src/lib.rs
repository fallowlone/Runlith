//! Shared `SecretBackend` contract (ADR-0039 P4).
//!
//! Every adapter implementing `secrets::SecretBackend` must satisfy these
//! semantics. The backend must be ready (for a lockable store: unlocked) and
//! must not hold any `contract-*` entry. Adapters bound to a real user store
//! (the OS Keychain) are exempt because the run would touch that store.

use secrets::{SecretBackend, SecretBackendError, SecretRef};

pub fn run_secret_backend_contract<B: SecretBackend>(backend: &B) {
    let missing = name("contract-missing");
    let first = name("contract-first");
    let second = name("contract-second");
    let empty = name("contract-empty");

    assert_eq!(
        backend.load(&missing),
        Err(SecretBackendError::NotFound),
        "load of a missing entry"
    );
    assert_eq!(
        backend.delete(&missing),
        Err(SecretBackendError::NotFound),
        "delete of a missing entry"
    );

    backend.store(&first, b"contract-value-1").expect("store");
    assert_eq!(
        backend.load(&first).expect("load"),
        b"contract-value-1",
        "store then load"
    );

    backend
        .store(&first, b"contract-value-2")
        .expect("overwrite");
    assert_eq!(
        backend.load(&first).expect("load"),
        b"contract-value-2",
        "overwrite replaces"
    );

    backend
        .store(&second, b"contract-value-3")
        .expect("store second");
    assert_eq!(
        backend.load(&first).expect("load"),
        b"contract-value-2",
        "entries are independent"
    );

    backend.delete(&first).expect("delete");
    assert_eq!(
        backend.load(&first),
        Err(SecretBackendError::NotFound),
        "load after delete"
    );
    assert_eq!(
        backend.load(&second).expect("load"),
        b"contract-value-3",
        "delete keeps others"
    );

    backend.store(&empty, b"").expect("store empty");
    assert_eq!(
        backend.load(&empty).expect("load"),
        b"",
        "empty value round trips"
    );
}

fn name(value: &str) -> SecretRef {
    SecretRef::new(value).expect("contract secret name is valid")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    /// Reference implementation; `strict_delete: false` reproduces a backend
    /// that silently accepts deleting a missing entry.
    struct MemoryBackend {
        values: Mutex<HashMap<String, Vec<u8>>>,
        strict_delete: bool,
    }

    impl MemoryBackend {
        fn new(strict_delete: bool) -> Self {
            Self {
                values: Mutex::new(HashMap::new()),
                strict_delete,
            }
        }
    }

    impl SecretBackend for MemoryBackend {
        fn store(&self, secret: &SecretRef, value: &[u8]) -> Result<(), SecretBackendError> {
            self.values
                .lock()
                .unwrap()
                .insert(secret.as_str().to_owned(), value.to_vec());
            Ok(())
        }

        fn load(&self, secret: &SecretRef) -> Result<Vec<u8>, SecretBackendError> {
            self.values
                .lock()
                .unwrap()
                .get(secret.as_str())
                .cloned()
                .ok_or(SecretBackendError::NotFound)
        }

        fn delete(&self, secret: &SecretRef) -> Result<(), SecretBackendError> {
            let removed = self.values.lock().unwrap().remove(secret.as_str());
            if removed.is_none() && self.strict_delete {
                return Err(SecretBackendError::NotFound);
            }
            Ok(())
        }
    }

    #[test]
    fn reference_backend_satisfies_the_contract() {
        run_secret_backend_contract(&MemoryBackend::new(true));
    }

    #[test]
    #[should_panic(expected = "delete of a missing entry")]
    fn silently_accepting_a_missing_delete_violates_the_contract() {
        run_secret_backend_contract(&MemoryBackend::new(false));
    }
}
