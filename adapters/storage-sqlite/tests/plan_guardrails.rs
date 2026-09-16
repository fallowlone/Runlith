//! TASK-0078 round 2: durable planning guardrail counters (schema 14).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use plan_domain::{
    planning_evidence_key, EvidenceRef, PlanBranchId, PlanningCostReceipt, PlanningCostReceiptRef,
};
use storage_sqlite::{PlanGuardrailEvent, PlannerCallSettlement, SqliteStore, StorageError};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-plan-guardrails-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
        }
    }

    fn open(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("store")
    }

    fn raw(&self) -> rusqlite::Connection {
        let connection = rusqlite::Connection::open(&self.path).expect("raw sqlite");
        #[cfg(unix)]
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600))
            .expect("secure raw sqlite fixture");
        connection
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn branch() -> PlanBranchId {
    PlanBranchId::new("guardrail-branch").unwrap()
}

fn key(evidence: &str) -> String {
    planning_evidence_key(&[EvidenceRef::new(evidence).unwrap()])
}

fn receipt(input: u32) -> PlanningCostReceipt {
    PlanningCostReceipt::new(input, 10, 5, "provider-reported".to_owned()).unwrap()
}

#[test]
fn reservations_are_bounded_and_counters_survive_reopen() {
    let db = TestDb::new("reopen");
    let event_key = key("attempt:A1");
    let receipt_ref = PlanningCostReceiptRef::new("planning-call:guardrail:1").unwrap();
    {
        let mut store = db.open();
        assert_eq!(
            store
                .get_plan_guardrail_event(&branch(), &event_key)
                .unwrap(),
            None
        );
        let first = store
            .reserve_plan_planner_call(&branch(), &event_key, 2, 32_000)
            .unwrap()
            .expect("first call reserved");
        assert_eq!(
            (first.planner_calls, first.charged_input_tokens),
            (1, 32_000)
        );
        let settled = store
            .settle_plan_planner_call(
                &branch(),
                &event_key,
                &PlannerCallSettlement {
                    reserved_input_tokens: 32_000,
                    charged_input_tokens: 40_005,
                    measured: Some((&receipt_ref, &receipt(40_005))),
                },
            )
            .unwrap();
        assert_eq!(settled.charged_input_tokens, 40_005);
        store
            .reserve_plan_planner_call(&branch(), &event_key, 2, 32_000)
            .unwrap()
            .expect("second call reserved");
        assert_eq!(
            store
                .reserve_plan_planner_call(&branch(), &event_key, 2, 32_000)
                .unwrap(),
            None,
            "third call exceeds the bound and writes nothing"
        );
        store
            .record_plan_proposal_rejection(&branch(), &event_key)
            .unwrap();
    }

    let store = db.open();
    assert_eq!(
        store
            .get_plan_guardrail_event(&branch(), &event_key)
            .unwrap(),
        Some(PlanGuardrailEvent {
            planner_calls: 2,
            charged_input_tokens: 72_005,
            consecutive_rejections: 1,
        }),
        "an in-flight reservation stays charged across restart"
    );
    assert_eq!(
        store.get_plan_cost_receipt(&receipt_ref).unwrap(),
        Some(receipt(40_005))
    );
    assert_eq!(
        store
            .get_plan_guardrail_event(&branch(), &key("attempt:A2"))
            .unwrap(),
        None,
        "a different evidence event has independent counters"
    );
}

#[test]
fn settlement_cannot_release_unreserved_tokens_or_rebind_a_receipt() {
    let db = TestDb::new("settlement");
    let mut store = db.open();
    let event_key = key("attempt:A1");

    let missing = store
        .settle_plan_planner_call(
            &branch(),
            &event_key,
            &PlannerCallSettlement {
                reserved_input_tokens: 32_000,
                charged_input_tokens: 0,
                measured: None,
            },
        )
        .expect_err("settlement without reservation");
    assert!(matches!(missing, StorageError::NotFound(_)));

    store
        .reserve_plan_planner_call(&branch(), &event_key, 8, 32_000)
        .unwrap()
        .expect("reserved");
    let over_release = store
        .settle_plan_planner_call(
            &branch(),
            &event_key,
            &PlannerCallSettlement {
                reserved_input_tokens: 64_000,
                charged_input_tokens: 0,
                measured: None,
            },
        )
        .expect_err("cannot release more than reserved");
    assert!(matches!(over_release, StorageError::ConstraintViolation(_)));

    let receipt_ref = PlanningCostReceiptRef::new("planning-call:guardrail:rebind").unwrap();
    store
        .settle_plan_planner_call(
            &branch(),
            &event_key,
            &PlannerCallSettlement {
                reserved_input_tokens: 32_000,
                charged_input_tokens: 100,
                measured: Some((&receipt_ref, &receipt(100))),
            },
        )
        .unwrap();
    store
        .reserve_plan_planner_call(&branch(), &event_key, 8, 32_000)
        .unwrap()
        .expect("reserved again");
    let rebind = store
        .settle_plan_planner_call(
            &branch(),
            &event_key,
            &PlannerCallSettlement {
                reserved_input_tokens: 32_000,
                charged_input_tokens: 200,
                measured: Some((&receipt_ref, &receipt(200))),
            },
        )
        .expect_err("receipt identity is immutable");
    assert!(matches!(rebind, StorageError::ConstraintViolation(_)));
    let event = store
        .get_plan_guardrail_event(&branch(), &event_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        event.charged_input_tokens,
        100 + 32_000,
        "failed settlement rolls back and keeps the conservative reservation"
    );
    assert_eq!(
        store.get_plan_cost_receipt(&receipt_ref).unwrap(),
        Some(receipt(100))
    );
}

#[test]
fn malformed_keys_and_corrupt_counters_fail_closed() {
    let db = TestDb::new("corrupt");
    let mut store = db.open();
    assert!(matches!(
        store.reserve_plan_planner_call(&branch(), "attempt:A1", 8, 32_000),
        Err(StorageError::ConstraintViolation(_))
    ));

    let event_key = key("attempt:A1");
    store
        .reserve_plan_planner_call(&branch(), &event_key, 8, 32_000)
        .unwrap()
        .expect("reserved");
    drop(store);
    db.raw()
        .execute(
            "UPDATE plan_guardrail_events SET planner_calls = 5000000000",
            [],
        )
        .unwrap();

    let store = db.open();
    assert!(matches!(
        store.get_plan_guardrail_event(&branch(), &event_key),
        Err(StorageError::Corrupt(_))
    ));
}
