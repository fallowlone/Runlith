//! SQLite continuity seam contract run (TASK-0171).
//!
//! Real files only: the suite owns a temporary database file, and the
//! reopen proof drops the store and reopens the same file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use continuity_contract::run_continuity_store_contract;
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-continuity-{}-{id}.sqlite",
        std::process::id()
    ))
}

#[test]
fn sqlite_satisfies_continuity_contract() {
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    let path = db.clone();
    run_continuity_store_contract(&mut store, || SqliteStore::open(&path).unwrap());
    drop(store);
    std::fs::remove_file(&db).ok();
}

// Budget events and session links have no read path through the seam
// (append-only evidence), so a raw-SQL probe asserts the rows land.
#[test]
fn append_only_rows_land() {
    use agent_runtime::AttemptId;
    use context_engine::model::ContextPlanId;
    use continuity::model::{BudgetEvent, SessionLink};
    use continuity::port::ContinuityStorePort;
    use continuity_contract::fixtures;

    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    let genesis = fixtures::epoch(900, "T-PROBE", "E-PROBE", 0);
    store.open_chain(&genesis).unwrap();
    store
        .link_session(&SessionLink {
            session_ref: "probe:ses".to_owned(),
            epoch_id: genesis.id.clone(),
            attempt_ref: AttemptId::new("A-PROBE").unwrap(),
            provider_id: agent_runtime::ProviderRef::new("probe").unwrap(),
            linked_at: 1000,
        })
        .unwrap();
    store
        .record_budget_event(&BudgetEvent {
            plan_ref: ContextPlanId::new(fixtures::PLAN_ID).unwrap(),
            hard: true,
            recorded_at: 1000,
        })
        .unwrap();
    drop(store);

    let raw = rusqlite::Connection::open(&db).unwrap();
    let links: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM continuity_session_links
             WHERE session_ref = 'probe:ses' AND attempt_ref = 'A-PROBE'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(links, 1);
    let events: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM continuity_budget_events WHERE hard = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
    std::fs::remove_file(&db).ok();
}

// CR-10: unknown-major rows refuse on read paths. Staging refuses v2
// at write, so the probe hand-inserts a committed v2 row.
#[test]
fn unknown_major_refuses_on_read() {
    use continuity::model::{CheckpointId, ContinuityError};
    use continuity::port::ContinuityStorePort;
    use continuity_contract::fixtures;

    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    let genesis = fixtures::epoch(910, "T-V2", "E-V2", 0);
    store.open_chain(&genesis).unwrap();
    drop(store);

    let raw = rusqlite::Connection::open(&db).unwrap();
    let id = "ctxckpt_01ARZ3NDEKTSV4RRFFQ69G5FAV";
    raw.execute(
        "INSERT INTO continuity_checkpoints
         (checkpoint_id, epoch_ref, task_ref, execution_ref, created_at,
          format_version, parent_checkpoint, continuation, context_plan_ref,
          sensitivity_ceiling, state, staged_hash, committed_at, store_revision)
         VALUES (?1, ?2, 'T-V2', 'E-V2', 1000, 131072, NULL, '', ?3,
                 'PUBLIC', 'COMMITTED', ?4, 1000, 1)",
        rusqlite::params![
            id,
            genesis.id.as_str(),
            fixtures::PLAN_ID,
            format!("sha256:{}", "a".repeat(64)),
        ],
    )
    .unwrap();
    drop(raw);

    let store = SqliteStore::open(&db).unwrap();
    let err = store
        .load_checkpoint(&CheckpointId::new(id).unwrap())
        .unwrap_err();
    assert!(
        matches!(err, ContinuityError::UnknownVersion(_)),
        "unknown majors must refuse on read, got {err}"
    );
    std::fs::remove_file(&db).ok();
}
