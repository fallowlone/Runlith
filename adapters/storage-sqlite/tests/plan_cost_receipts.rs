use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use plan_domain::{PlanningCostReceipt, PlanningCostReceiptRef};
use storage_sqlite::{SqliteStore, StorageError, SUPPORTED_SCHEMA_VERSION};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-plan-cost-receipts-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn receipt(input: u32) -> PlanningCostReceipt {
    PlanningCostReceipt::new(input, 40, 850, "standard".to_owned()).expect("receipt")
}

#[test]
fn plan_cost_receipt_round_trips_and_identity_is_immutable() {
    let db = TestDb::new("round-trip");
    let mut store = SqliteStore::open(&db.path).expect("store");
    let receipt_ref = PlanningCostReceiptRef::new("planning-receipt:test-1").expect("ref");
    let evidence = receipt(120);

    store
        .record_plan_cost_receipt(&receipt_ref, &evidence)
        .expect("record receipt");
    store
        .record_plan_cost_receipt(&receipt_ref, &evidence)
        .expect("exact replay is idempotent");
    assert_eq!(
        store.get_plan_cost_receipt(&receipt_ref).expect("read"),
        Some(evidence)
    );

    let error = store
        .record_plan_cost_receipt(&receipt_ref, &receipt(121))
        .expect_err("receipt identity cannot be rebound");
    assert!(matches!(error, StorageError::ConstraintViolation(_)));
}

#[test]
fn schema_12_database_upgrades_to_13_without_plan_cost_data() {
    let db = TestDb::new("upgrade-12-13");
    {
        let store = SqliteStore::open(&db.path).expect("fresh current store");
        assert_eq!(store.schema_version().unwrap(), 13);
    }
    {
        let connection = rusqlite::Connection::open(&db.path).expect("raw sqlite");
        connection
            .execute_batch("DROP TABLE plan_cost_receipts; PRAGMA user_version = 12;")
            .expect("simulate historical v12 schema");
    }

    let mut store = SqliteStore::open(&db.path).expect("migrate v12 to v13");
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 13);
    assert_eq!(store.schema_version().unwrap(), 13);
    let receipt_ref = PlanningCostReceiptRef::new("planning-receipt:after-upgrade").unwrap();
    let evidence = receipt(1);
    store
        .record_plan_cost_receipt(&receipt_ref, &evidence)
        .expect("new table is usable after migration");
    assert_eq!(
        store.get_plan_cost_receipt(&receipt_ref).unwrap(),
        Some(evidence)
    );
}
