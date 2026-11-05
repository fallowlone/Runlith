//! Migration integrity (audit F6): schema is verified against the claimed
//! version, a backup precedes real migrations, and healthy DBs are untouched.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use storage_sqlite::SqliteStore;

struct TempDir(PathBuf);

static DIRECTORY_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "runlith-migration-integrity-{}-{nanos}-{}",
            std::process::id(),
            DIRECTORY_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fresh_db(dir: &Path) -> PathBuf {
    let path = dir.join("state.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    path
}

fn raw(path: &Path, sql: &str) {
    Connection::open(path).unwrap().execute_batch(sql).unwrap();
}

fn version(path: &Path) -> u32 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains(".pre-migrate-"))
        .collect()
}

#[test]
fn stale_version_with_missing_tables_fails_clearly_without_changes() {
    let dir = TempDir::new();
    let path = fresh_db(dir.0.as_path());
    raw(
        &path,
        "DROP TRIGGER binding_proof_consumption_no_update; \
         DROP TRIGGER binding_proof_consumption_no_delete; \
         DROP TABLE binding_proof_consumption; PRAGMA user_version = 27;",
    );

    let error = SqliteStore::open(&path)
        .err()
        .expect("must refuse")
        .to_string();

    assert!(error.contains("binding_proof_consumption"), "{error}");
    assert!(error.contains("version 27"), "{error}");
    assert_eq!(version(&path), 27);
    assert!(backups(dir.0.as_path()).is_empty());
}

#[test]
fn index_and_column_drift_refuse_before_backup_or_migration() {
    for (sql, object) in [
        (
            "DROP INDEX event_journal_task_seq",
            "event_journal_task_seq",
        ),
        (
            "ALTER TABLE projects ADD COLUMN unexpected TEXT",
            "columns of projects",
        ),
    ] {
        let dir = TempDir::new();
        let path = fresh_db(&dir.0);
        raw(&path, &format!("{sql}; PRAGMA user_version=36;"));
        let error = SqliteStore::open(&path)
            .err()
            .expect("must refuse drift")
            .to_string();
        assert!(error.contains(object), "{error}");
        assert_eq!(version(&path), 36);
        assert!(backups(&dir.0).is_empty());
    }
}

#[test]
fn healthy_old_version_migrates_with_private_backup_and_keeps_three() {
    let dir = TempDir::new();
    let path = fresh_db(dir.0.as_path());
    for round in 0..5 {
        raw(
            &path,
            "DROP TABLE org_usage_entries; DROP TABLE org_sync_envelopes; DROP TABLE org_policies; \
             DROP TABLE task_acceptance_statements; ALTER TABLE tasks DROP COLUMN requires_workspace_change; ALTER TABLE task_evidence_requirements DROP COLUMN statement; DROP TABLE org_memberships; DROP TABLE org_teams; DROP TABLE organizations; \
             DROP TABLE org_accounts; PRAGMA user_version = 36;",
        );
        drop(SqliteStore::open(&path).unwrap());
        assert_eq!(version(&path), 40, "round {round}");
        std::thread::sleep(std::time::Duration::from_millis(3));
    }

    let found = backups(dir.0.as_path());
    assert_eq!(found.len(), 3);
    #[cfg(unix)]
    for backup in &found {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(backup).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(backup.to_string_lossy().contains("pre-migrate-v36-"));
    }
}

#[test]
fn current_version_database_is_untouched() {
    let dir = TempDir::new();
    let path = fresh_db(dir.0.as_path());
    drop(SqliteStore::open(&path).unwrap());
    assert_eq!(version(&path), 40);
    assert!(backups(dir.0.as_path()).is_empty());
}
