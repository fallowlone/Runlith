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
fn schema_12_database_upgrades_to_current_without_plan_cost_data() {
    let db = TestDb::new("upgrade-12-current");
    {
        let store = SqliteStore::open(&db.path).expect("fresh current store");
        assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
    }
    {
        let connection = rusqlite::Connection::open(&db.path).expect("raw sqlite");
        connection
            .execute_batch(
                "DROP TABLE task_evidence_requirement_params;
                 DROP TABLE task_evidence_requirements;
                 DROP TABLE planning_input_candidate_tasks;
                 DROP TABLE human_decision_evidence;
                 DROP TABLE human_decision_options;
                 DROP TABLE human_decision_scope_roots;
                 DROP TABLE human_decision_requests;
                 DROP TABLE planning_run_heads;
                 DROP TABLE planning_runs;
                 DROP TABLE planning_input_heads;
                 DROP TABLE planning_input_lineage;
                 DROP TABLE planning_input_evidence;
                 DROP TABLE planning_input_criteria;
                 DROP TABLE planning_input_scope_roots;
                 DROP TABLE planning_input_snapshots;
                 DROP TABLE plan_guardrail_events;
                 DROP TABLE plan_revision_lineage;
                 DROP TABLE plan_cost_receipts;
                 PRAGMA user_version = 12;",
            )
            .expect("simulate historical v12 schema");
    }

    let mut store = SqliteStore::open(&db.path).expect("migrate v12 to current");
    assert_eq!(SUPPORTED_SCHEMA_VERSION, 17);
    assert_eq!(store.schema_version().unwrap(), SUPPORTED_SCHEMA_VERSION);
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
