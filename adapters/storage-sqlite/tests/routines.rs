//! SQLite routine seam contract run (TASK-0191).
use background_contract::run_routine_store_contract;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-routines-{}-{id}.sqlite",
        std::process::id()
    ))
}

#[test]
fn sqlite_satisfies_routine_contract() {
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    assert_eq!(store.schema_version().unwrap(), 40);
    let path = db.clone();
    run_routine_store_contract(&mut store, || SqliteStore::open(&path).unwrap());
    drop(store);
    std::fs::remove_file(&db).ok();
}

#[test]
fn finish_run_is_atomic_when_run_update_fails() {
    use background::model::{CapabilityScope, RoutineDefinition, RoutineRun, RunStatus};
    use background::{
        mint::{mint_routine_id, mint_run_id},
        port::RoutineStorePort,
    };
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    let mut def = RoutineDefinition::define(
        mint_routine_id(1_000, 1),
        "r",
        "o",
        60,
        CapabilityScope::ReadOnly,
        3,
        "author",
        1_000,
    )
    .unwrap();
    def.authorize("operator", 1_000).unwrap();
    store.define(&def).unwrap();
    let mut run = RoutineRun::start(mint_run_id(2_000, 1), def.id.clone(), 2_000).unwrap();
    store.start_run(&run).unwrap();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER boom BEFORE UPDATE ON routine_runs BEGIN SELECT RAISE(ABORT,'boom'); END;",
        )
        .unwrap();
    run.complete("e", 2_000).unwrap();
    def.record_completion(2_000).unwrap();
    assert!(store.finish_run(&def, &run).is_err());
    drop(store);
    let store = SqliteStore::open(&db).unwrap();
    let stored = store.load_run(&run.id).unwrap().unwrap();
    assert_eq!(stored.status, RunStatus::Running, "run must roll back");
    assert_eq!(store.load(&def.id).unwrap().unwrap().runs_completed, 0);
    drop(store);
    std::fs::remove_file(&db).ok();
}

#[test]
fn completion_cannot_spend_the_same_remaining_budget_twice() {
    use background::model::{RoutineRun, RunStatus};
    use background::port::RoutineStorePort;
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    let mut definition = background_contract::fixtures::authorized(10);
    definition.budget_max_runs = 1;
    store.define(&definition).unwrap();
    let mut first = RoutineRun::start(
        background::mint::mint_run_id(2000, 1),
        definition.id.clone(),
        2000,
    )
    .unwrap();
    let mut second = RoutineRun::start(
        background::mint::mint_run_id(2000, 2),
        definition.id.clone(),
        2000,
    )
    .unwrap();
    store.start_run(&first).unwrap();
    store.start_run(&second).unwrap();
    first.complete("receipt:first", 2001).unwrap();
    second.complete("receipt:second", 2001).unwrap();
    definition.record_completion(2001).unwrap();
    store.finish_run(&definition, &first).unwrap();
    assert!(store.finish_run(&definition, &second).is_err());
    drop(store);
    let store = SqliteStore::open(&db).unwrap();
    assert_eq!(
        store.load(&definition.id).unwrap().unwrap().runs_completed,
        1
    );
    assert_eq!(
        store.load_run(&first.id).unwrap().unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(
        store.load_run(&second.id).unwrap().unwrap().status,
        RunStatus::Running
    );
    drop(store);
    std::fs::remove_file(db).unwrap();
}
