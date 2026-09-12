use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use policy::EffectiveCapabilities;
use secrets::{
    SecretBackend, SecretBackendError, SecretBinding, SecretBindingId, SecretDelivery,
    SecretDeliveryError, SecretDeliveryTarget, SecretMetadataStore, SecretRef, SecretsBroker,
};
use storage_sqlite::SqliteStore;

struct TestDb {
    path: PathBuf,
}

impl TestDb {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "runlith-secret-store-{label}-{}-{nonce}.sqlite",
                std::process::id()
            )),
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).unwrap()
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Default)]
struct MemoryBackend {
    values: Arc<Mutex<HashMap<String, Vec<u8>>>>,
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
        self.values.lock().unwrap().remove(secret.as_str());
        Ok(())
    }
}

#[derive(Default)]
struct CaptureDelivery(Vec<Vec<u8>>);

impl SecretDelivery for CaptureDelivery {
    fn deliver(
        &mut self,
        _target: &SecretDeliveryTarget,
        value: &[u8],
    ) -> Result<(), SecretDeliveryError> {
        self.0.push(value.to_vec());
        Ok(())
    }
}

fn binding(secret: &SecretRef) -> SecretBinding {
    SecretBinding::new(
        SecretBindingId::new("binding-1").unwrap(),
        secret.clone(),
        SecretDeliveryTarget::tool("github-api").unwrap(),
    )
}

fn test_material() -> Vec<u8> {
    (0_u8..48)
        .map(|value| value.wrapping_mul(23).wrapping_add(13))
        .collect()
}

#[test]
fn opaque_refs_and_bindings_round_trip_without_journal_events() {
    let db = TestDb::new("roundtrip");
    let mut store = db.open();
    let secret = SecretRef::new("github-token").unwrap();
    let binding = binding(&secret);

    store.ensure_ref(&secret).unwrap();
    store.put_binding(&binding).unwrap();

    assert!(store.has_ref(&secret).unwrap());
    assert_eq!(store.get_binding(binding.id()).unwrap(), Some(binding));
    assert!(store.list_events_ordered().unwrap().is_empty());
}

#[test]
fn secret_metadata_tables_have_no_value_or_payload_columns() {
    let db = TestDb::new("schema");
    let _store = db.open();
    let connection = rusqlite::Connection::open(&db.path).unwrap();
    let columns = |table: &str| -> Vec<String> {
        let mut statement = connection
            .prepare(&format!(
                "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
            ))
            .unwrap();
        statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };

    assert_eq!(columns("secret_refs"), ["ref_id"]);
    assert_eq!(
        columns("secret_bindings"),
        ["binding_id", "secret_ref_id", "target_kind", "target_id"]
    );
}

#[test]
fn authorized_release_does_not_persist_material_or_append_event_journal() {
    let db = TestDb::new("leakage");
    let store = db.open();
    let backend = MemoryBackend::default();
    let secret = SecretRef::new("github-token").unwrap();
    let material = test_material();
    backend.store(&secret, &material).unwrap();
    let binding = binding(&secret);
    let mut broker = SecretsBroker::new(store, backend);
    broker.register_ref(&secret).unwrap();
    broker.bind(&binding).unwrap();
    let authority =
        EffectiveCapabilities::new(vec![policy::Capability::secret(secret.as_str()).unwrap()]);
    let mut delivery = CaptureDelivery::default();

    broker
        .release(binding.id(), binding.target(), &authority, &mut delivery)
        .unwrap();

    assert_eq!(delivery.0.as_slice(), std::slice::from_ref(&material));
    let (store, _backend) = broker.into_parts();
    assert!(store.list_events_ordered().unwrap().is_empty());
    drop(store);
    let bytes = fs::read(&db.path).unwrap();
    assert!(
        !bytes
            .windows(material.len())
            .any(|window| window == material),
        "secret material must never enter SQLite bytes"
    );
}
