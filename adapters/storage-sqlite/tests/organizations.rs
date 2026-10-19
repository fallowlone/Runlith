//! SQLite organization seam contract run (TASK-0209).
use org_contract::run_org_store_contract;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-organizations-{}-{id}.sqlite",
        std::process::id()
    ))
}

#[test]
fn sqlite_satisfies_org_contract() {
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    assert_eq!(store.schema_version().unwrap(), 37);
    let path = db.clone();
    run_org_store_contract(&mut store, || SqliteStore::open(&path).unwrap());
    drop(store);
    std::fs::remove_file(&db).ok();
}
