//! Pre-migration safety: schema-vs-version verification, backup, and
//! post-migration integrity checks (audit F6).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::*;

/// Number of `*.pre-migrate-*` backups kept next to the database.
const KEEP_BACKUPS: usize = 3;

/// Every migration's SQL, in order, by the version it brings the schema to.
const MIGRATIONS: &[(u32, &[&str])] = &[
    (1, &[MIGRATION_0001]),
    (2, &[MIGRATION_0002]),
    (3, &[MIGRATION_0003]),
    (4, &[MIGRATION_0004]),
    (5, &[MIGRATION_0005]),
    (6, &[MIGRATION_0006]),
    (7, &[MIGRATION_0007]),
    (8, &[MIGRATION_0008]),
    (9, &[MIGRATION_0009]),
    (10, &[MIGRATION_0010]),
    (11, &[MIGRATION_0011]),
    (12, &[MIGRATION_0012]),
    (13, &[MIGRATION_0013]),
    (14, &[MIGRATION_0014]),
    (15, &[MIGRATION_0015]),
    (16, &[MIGRATION_0016]),
    (17, &[MIGRATION_0017]),
    (18, &[MIGRATION_0018]),
    (19, &[MIGRATION_0019]),
    (20, &[MIGRATION_0020]),
    (21, &[MIGRATION_0021]),
    (22, &[MIGRATION_0022]),
    (23, &[MIGRATION_0023]),
    (24, &[MIGRATION_0024]),
    (25, &[MIGRATION_0025]),
    (26, &[MIGRATION_0026]),
    (27, &[MIGRATION_0027]),
    (28, &[MIGRATION_0028]),
    (29, &[MIGRATION_0029]),
    (30, &[MIGRATION_0030]),
    (31, &[MIGRATION_0031]),
    (32, &[MIGRATION_0032, MIGRATION_0032_GUARD]),
    (33, &[MIGRATION_0033]),
    (34, &[MIGRATION_0034]),
    (35, &[MIGRATION_0035]),
    (36, &[MIGRATION_0036]),
    (37, &[MIGRATION_0037]),
];

fn objects(connection: &Connection) -> StorageResult<BTreeSet<String>> {
    let mut statement = connection.prepare(
        "SELECT type || ' ' || name FROM sqlite_master \
         WHERE type IN ('table','trigger','index') AND name NOT LIKE 'sqlite_%'",
    )?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Fails if the database claims `version` but lacks tables/triggers that
/// migrations `<= version` create. Nothing is modified.
pub(super) fn verify_schema_matches_version(
    connection: &Connection,
    version: u32,
) -> StorageResult<()> {
    let reference = Connection::open_in_memory()?;
    for (_, sql) in MIGRATIONS.iter().filter(|(v, _)| *v <= version) {
        for statement in *sql {
            reference.execute_batch(statement).map_err(|error| {
                StorageError::Migration(format!("reference schema build failed: {error}"))
            })?;
        }
    }
    // Later migrations may deliberately repair a known-bad historical shape
    // with `IF NOT EXISTS` (e.g. 0024 recreates 0022's operations table);
    // such objects are not "missing" from the migration's point of view.
    let pending: String = MIGRATIONS
        .iter()
        .filter(|(v, _)| *v > version)
        .flat_map(|(_, sql)| sql.iter())
        .map(|sql| {
            sql.to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ");
    let expected = objects(&reference)?;
    let actual = objects(connection)?;
    let mut missing: Vec<String> = expected
        .difference(&actual)
        .filter(|object| {
            let name = object.split(' ').nth(1).unwrap_or_default();
            !pending.contains(&format!("if not exists {name} "))
        })
        .cloned()
        .collect();
    for object in expected.intersection(&actual) {
        if let Some(table) = object.strip_prefix("table ") {
            if columns(&reference, table)? != columns(connection, table)? {
                missing.push(format!("columns of {table}"));
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(StorageError::Migration(format!(
        "database claims schema version {version} but is missing expected objects: {}. \
         The database is inconsistent (partially restored or hand-edited); no changes were made. \
         Restore a backup (state.sqlite3.pre-migrate-* if present) or recreate the database \
         to start fresh.",
        missing.join(", ")
    )))
}

type Column = (String, String, i64, Option<String>, i64, i64);

fn columns(connection: &Connection, table: &str) -> StorageResult<Vec<Column>> {
    let mut statement = connection.prepare(
        "SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?1) ORDER BY cid",
    )?;
    let rows = statement.query_map([table], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn main_db_file(connection: &Connection) -> StorageResult<Option<PathBuf>> {
    let file: String = connection.query_row(
        "SELECT file FROM pragma_database_list WHERE name = 'main'",
        [],
        |row| row.get(0),
    )?;
    Ok((!file.is_empty()).then(|| PathBuf::from(file)))
}

fn backup_error(error: impl std::fmt::Display) -> StorageError {
    StorageError::Migration(format!("pre-migration backup failed: {error}"))
}

/// Copies the database to `<file>.pre-migrate-v<version>-<millis>` (0600)
/// and prunes to the newest [`KEEP_BACKUPS`]. No-op for in-memory databases.
pub(super) fn backup_before_migrate(connection: &Connection, version: u32) -> StorageResult<()> {
    let Some(db) = main_db_file(connection)? else {
        return Ok(());
    };
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = db.file_name().and_then(|n| n.to_str()).unwrap_or("state");
    static BACKUP_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = BACKUP_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let target = db.with_file_name(format!(
        "{name}.pre-migrate-v{version}-{millis:016}-{}-{sequence}",
        std::process::id()
    ));
    create_private_empty(&target).map_err(backup_error)?;
    let result = connection.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()]);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&target);
        return Err(backup_error(error));
    }
    prune_backups(&db, name);
    Ok(())
}

fn create_private_empty(path: &Path) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).map(|_| ())
}

fn prune_backups(db: &Path, name: &str) {
    let Some(dir) = db.parent() else { return };
    let prefix = format!("{name}.pre-migrate-");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    // Names end in a zero-padded timestamp, so lexicographic order by that
    // suffix is chronological.
    let mut backups: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let mut suffix = file.strip_prefix(&prefix)?.split('-');
            suffix.next()?.strip_prefix('v')?.parse::<u32>().ok()?;
            let stamp = suffix.next()?;
            if stamp.len() < 16
                || !stamp.bytes().all(|byte| byte.is_ascii_digit())
                || !suffix
                    .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            {
                return None;
            }
            let stamp = stamp.to_owned();
            if !e.file_type().ok()?.is_file() {
                return None;
            }
            Some((stamp, e.path()))
        })
        .collect();
    backups.sort();
    let excess = backups.len().saturating_sub(KEEP_BACKUPS);
    for (_, path) in backups.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

/// Runs inside the migration transaction so a failure rolls everything back.
pub(super) fn foreign_key_violations(connection: &Connection) -> StorageResult<i64> {
    Ok(
        connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?,
    )
}

/// `fk_baseline` is the violation count before migrating: pre-existing
/// dangling rows are not this migration's doing, only new ones fail it.
pub(super) fn verify_integrity(connection: &Connection, fk_baseline: i64) -> StorageResult<()> {
    let verdict: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if verdict != "ok" {
        return Err(StorageError::Migration(format!(
            "integrity_check failed after migration: {verdict}"
        )));
    }
    let violations = foreign_key_violations(connection)?;
    if violations > fk_baseline {
        return Err(StorageError::Migration(format!(
            "foreign_key_check found {violations} violation(s) after migration \
             (was {fk_baseline} before)"
        )));
    }
    Ok(())
}
