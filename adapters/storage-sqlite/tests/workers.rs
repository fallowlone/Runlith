//! SQLite worker seam contract run (TASK-0200).
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use storage_sqlite::SqliteStore;
use worker_contract::run_worker_store_contract;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-workers-{}-{id}.sqlite",
        std::process::id()
    ))
}

#[test]
fn sqlite_satisfies_worker_contract() {
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    assert_eq!(store.schema_version().unwrap(), 37);
    let path = db.clone();
    run_worker_store_contract(&mut store, || SqliteStore::open(&path).unwrap());
    drop(store);
    std::fs::remove_file(&db).ok();
}
