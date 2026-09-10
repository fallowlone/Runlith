//! Deterministic checked-in migrations with an explicit schema version.
//!
//! Versioning uses SQLite `PRAGMA user_version`: 0 means a fresh database,
//! 5 is the supported version (`0001_task_core.sql`, then
//! `0002_event_journal.sql`, then `0003_event_scope.sql`, then
//! `0004_runtime_core.sql`, then `0005_workspace_leases.sql`). Newer
//! versions fail closed; re-opening the current version is a no-op (never
//! destructively re-applied). Executing migrations inside one SQLite
//! transaction is storage consistency only — not TASK-0024 event atomicity.

use rusqlite::Connection;

use crate::error::{StorageError, StorageResult};

/// Schema version this adapter understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 5;

const MIGRATION_0001: &str = include_str!("../migrations/0001_task_core.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_event_journal.sql");
const MIGRATION_0003: &str = include_str!("../migrations/0003_event_scope.sql");
const MIGRATION_0004: &str = include_str!("../migrations/0004_runtime_core.sql");
const MIGRATION_0005: &str = include_str!("../migrations/0005_workspace_leases.sql");

/// Applies pending migrations to `connection`, which must already have
/// `PRAGMA foreign_keys = ON`. Migrations run in order inside one SQLite
/// transaction; each applies at most once. Historical migrations are never
/// modified. Returns the resulting schema version. Concurrent first-time
/// openers serialize inside (see body).
pub(crate) fn migrate(connection: &mut Connection) -> StorageResult<u32> {
    let found: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if found > SUPPORTED_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchemaVersion {
            found,
            supported: SUPPORTED_SCHEMA_VERSION,
        });
    }
    if found == SUPPORTED_SCHEMA_VERSION {
        return Ok(found);
    }
    // IMMEDIATE like every other write transaction in this adapter:
    // concurrent opens on a fresh database must serialize at BEGIN.
    // The version is re-checked *inside*: the loser observes the
    // winner's version and stops instead of re-applying DDL onto
    // existing tables (TASK-0047 found two racing opens both running
    // 0001 on a fresh database).
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: u32 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if current > SUPPORTED_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchemaVersion {
            found: current,
            supported: SUPPORTED_SCHEMA_VERSION,
        });
    }
    if current == SUPPORTED_SCHEMA_VERSION {
        transaction.rollback()?;
        return Ok(current);
    }
    if current < 1 {
        transaction.execute_batch(MIGRATION_0001).map_err(|error| {
            StorageError::Migration(format!("0001_task_core.sql failed: {error}"))
        })?;
    }
    if current < 2 {
        transaction.execute_batch(MIGRATION_0002).map_err(|error| {
            StorageError::Migration(format!("0002_event_journal.sql failed: {error}"))
        })?;
    }
    if current < 3 {
        transaction.execute_batch(MIGRATION_0003).map_err(|error| {
            StorageError::Migration(format!("0003_event_scope.sql failed: {error}"))
        })?;
    }
    if current < 4 {
        transaction.execute_batch(MIGRATION_0004).map_err(|error| {
            StorageError::Migration(format!("0004_runtime_core.sql failed: {error}"))
        })?;
    }
    if current < 5 {
        transaction.execute_batch(MIGRATION_0005).map_err(|error| {
            StorageError::Migration(format!("0005_workspace_leases.sql failed: {error}"))
        })?;
    }
    transaction.execute_batch("PRAGMA user_version = 5")?;
    transaction.commit()?;
    Ok(SUPPORTED_SCHEMA_VERSION)
}

/// Reads the schema version without migrating.
pub(crate) fn schema_version(connection: &Connection) -> StorageResult<u32> {
    Ok(connection.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

/// Confirms foreign-key enforcement is really active on this connection.
/// SQLite does not enable it by default; assuming otherwise would silently
/// lose every relational guarantee below.
pub(crate) fn verify_foreign_keys(connection: &Connection) -> StorageResult<()> {
    let enabled: u32 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if enabled != 1 {
        return Err(StorageError::Migration(
            "PRAGMA foreign_keys is not active; refusing to proceed".to_owned(),
        ));
    }
    Ok(())
}
