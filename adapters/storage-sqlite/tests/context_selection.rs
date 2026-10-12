//! G9 context-selection durability tests (TASK-0170).
//!
//! Real files only: every test owns a temporary database file, and the
//! contract proof closes the store and reopens the same file.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use context_engine_contract::run_context_store_contract;
use storage_sqlite::SqliteStore;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn fresh_db() -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "runlith-context-selection-{}-{}.sqlite",
        std::process::id(),
        id
    ))
}

#[test]
fn sqlite_store_satisfies_context_store_contract() {
    let db = fresh_db();
    let mut store = SqliteStore::open(&db).unwrap();
    run_context_store_contract(&mut store, || SqliteStore::open(&db).unwrap());
    std::fs::remove_file(&db).ok();
}
