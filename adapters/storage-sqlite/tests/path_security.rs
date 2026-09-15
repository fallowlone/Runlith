#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{symlink, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use storage_sqlite::{SqliteStore, StorageError};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "runlith-storage-path-security-{}-{name}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }

    fn db(&self) -> PathBuf {
        self.0.join("state.sqlite3")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & 0o7777
}

#[test]
fn fresh_database_is_created_private_and_reopens() {
    let dir = TestDir::new("fresh-private");
    let db = dir.db();
    SqliteStore::open(&db).unwrap();
    assert_eq!(mode(&db), 0o600);
    SqliteStore::open(&db).unwrap();
}

#[test]
fn existing_insecure_database_is_rejected_without_repair() {
    let dir = TestDir::new("insecure-existing");
    let db = dir.db();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&db)
        .unwrap();
    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(matches!(
        SqliteStore::open(&db),
        Err(StorageError::UnsafePath(_))
    ));
    assert_eq!(
        mode(&db),
        0o644,
        "unsafe existing file must not be chmod-repaired"
    );
}

#[test]
fn database_symlink_is_rejected_without_following_target() {
    let dir = TestDir::new("db-symlink");
    let target = dir.0.join("target");
    fs::write(&target, b"sentinel").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let db = dir.db();
    symlink(&target, &db).unwrap();

    assert!(matches!(
        SqliteStore::open(&db),
        Err(StorageError::UnsafePath(_))
    ));
    assert_eq!(fs::read(&target).unwrap(), b"sentinel");
}

#[test]
fn symlinked_or_writable_parent_is_rejected_before_database_creation() {
    let root = TestDir::new("unsafe-parent");
    let real = root.0.join("real");
    fs::create_dir(&real).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
    let alias = root.0.join("alias");
    symlink(&real, &alias).unwrap();
    assert!(matches!(
        SqliteStore::open(alias.join("state.sqlite3")),
        Err(StorageError::UnsafePath(_))
    ));
    assert!(!real.join("state.sqlite3").exists());

    let writable = root.0.join("writable");
    fs::create_dir(&writable).unwrap();
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        SqliteStore::open(writable.join("state.sqlite3")),
        Err(StorageError::UnsafePath(_))
    ));
    assert!(!writable.join("state.sqlite3").exists());
}

#[test]
fn sqlite_sidecar_symlink_is_rejected() {
    let dir = TestDir::new("sidecar-symlink");
    let db = dir.db();
    SqliteStore::open(&db).unwrap();
    let target = dir.0.join("sidecar-target");
    fs::write(&target, b"sentinel").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    symlink(&target, &wal).unwrap();

    assert!(matches!(
        SqliteStore::open(&db),
        Err(StorageError::UnsafePath(_))
    ));
    assert_eq!(fs::read(&target).unwrap(), b"sentinel");
}

#[test]
fn insecure_sqlite_sidecar_mode_is_rejected() {
    let dir = TestDir::new("sidecar-mode");
    let db = dir.db();
    SqliteStore::open(&db).unwrap();
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&wal)
        .unwrap();
    fs::set_permissions(&wal, fs::Permissions::from_mode(0o644)).unwrap();

    assert!(matches!(
        SqliteStore::open(&db),
        Err(StorageError::UnsafePath(_))
    ));
    assert_eq!(mode(&wal), 0o644);
}

#[test]
fn sqlite_wal_and_shm_created_for_private_db_remain_private() {
    let dir = TestDir::new("wal-private");
    let db = dir.db();
    SqliteStore::open(&db).unwrap();
    let raw = rusqlite::Connection::open(&db).unwrap();
    let journal_mode: String = raw
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    raw.execute_batch(
        "CREATE TABLE IF NOT EXISTS r1_wal_probe (value INTEGER NOT NULL);\n\
         INSERT INTO r1_wal_probe (value) VALUES (1);",
    )
    .unwrap();

    let wal = PathBuf::from(format!("{}-wal", db.display()));
    let shm = PathBuf::from(format!("{}-shm", db.display()));
    assert!(
        wal.exists(),
        "WAL sidecar must exist while WAL connection is live"
    );
    assert!(
        shm.exists(),
        "SHM sidecar must exist while WAL connection is live"
    );
    assert_eq!(mode(&wal), 0o600);
    assert_eq!(mode(&shm), 0o600);
    SqliteStore::open(&db).unwrap();
}

#[test]
fn hard_linked_database_is_rejected() {
    let dir = TestDir::new("hard-link");
    let db = dir.db();
    SqliteStore::open(&db).unwrap();
    let alias = dir.0.join("alias.sqlite3");
    fs::hard_link(&db, &alias).unwrap();
    assert_eq!(fs::symlink_metadata(&db).unwrap().nlink(), 2);
    assert!(matches!(
        SqliteStore::open(&db),
        Err(StorageError::UnsafePath(_))
    ));
}

#[test]
fn concurrent_first_openers_share_the_secure_created_file() {
    let dir = TestDir::new("concurrent-create");
    let db = dir.db();
    let mut threads = Vec::new();
    for _ in 0..4 {
        let path = db.clone();
        threads.push(std::thread::spawn(move || SqliteStore::open(path)));
    }
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    assert_eq!(mode(&db), 0o600);
}
