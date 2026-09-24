//! Deterministic checked-in migrations with an explicit schema version.
//!
//! Versioning uses SQLite `PRAGMA user_version`: 0 means a fresh database,
//! 20 is the supported version (`0001_task_core.sql`, then
//! `0002_event_journal.sql`, then `0003_event_scope.sql`, then
//! `0004_runtime_core.sql`, then `0005_workspace_leases.sql`, then
//! `0006_attempt_reservations.sql`, then
//! `0007_managed_identity_reservations.sql`, then `0008_approvals.sql`, then
//! `0009_secret_refs.sql`, then `0010_security_audit.sql`, then
//! `0011_workspace_trusted_anchor.sql`, then `0012_plan_persistence.sql`, then
//! `0013_plan_cost_receipts.sql`, then `0014_plan_guardrails.sql`, then
//! `0015_human_decisions.sql`, then `0016_g7_closure_remediation.sql`, then
//! `0017_evidence_requirements.sql`, then
//! `0018_planning_input_evidence_requirements.sql`, then
//! `0019_observed_effects.sql`, then `0020_task_prerequisites.sql`, then
//! `0021_test_mutation_policy.sql`, then `0022_candidate_input_assessments.sql`). Newer
//! versions fail closed; re-opening the current version is a no-op (never
//! destructively re-applied). Executing migrations inside one SQLite
//! transaction is storage consistency only — not TASK-0024 event atomicity.

use rusqlite::Connection;

use crate::error::{StorageError, StorageResult};

/// Schema version this adapter understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 22;

const MIGRATION_0001: &str = include_str!("../migrations/0001_task_core.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_event_journal.sql");
const MIGRATION_0003: &str = include_str!("../migrations/0003_event_scope.sql");
const MIGRATION_0004: &str = include_str!("../migrations/0004_runtime_core.sql");
const MIGRATION_0005: &str = include_str!("../migrations/0005_workspace_leases.sql");
const MIGRATION_0006: &str = include_str!("../migrations/0006_attempt_reservations.sql");
const MIGRATION_0007: &str = include_str!("../migrations/0007_managed_identity_reservations.sql");
const MIGRATION_0008: &str = include_str!("../migrations/0008_approvals.sql");
const MIGRATION_0009: &str = include_str!("../migrations/0009_secret_refs.sql");
const MIGRATION_0010: &str = include_str!("../migrations/0010_security_audit.sql");
const MIGRATION_0011: &str = include_str!("../migrations/0011_workspace_trusted_anchor.sql");
const MIGRATION_0012: &str = include_str!("../migrations/0012_plan_persistence.sql");
const MIGRATION_0013: &str = include_str!("../migrations/0013_plan_cost_receipts.sql");
const MIGRATION_0014: &str = include_str!("../migrations/0014_plan_guardrails.sql");
const MIGRATION_0015: &str = include_str!("../migrations/0015_human_decisions.sql");
const MIGRATION_0016: &str = include_str!("../migrations/0016_g7_closure_remediation.sql");
const MIGRATION_0017: &str = include_str!("../migrations/0017_evidence_requirements.sql");
const MIGRATION_0018: &str =
    include_str!("../migrations/0018_planning_input_evidence_requirements.sql");
const MIGRATION_0019: &str = include_str!("../migrations/0019_observed_effects.sql");
const MIGRATION_0020: &str = include_str!("../migrations/0020_task_prerequisites.sql");
const MIGRATION_0021: &str = include_str!("../migrations/0021_test_mutation_policy.sql");
const MIGRATION_0022: &str = include_str!("../migrations/0022_candidate_input_assessments.sql");

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
    if current < 6 {
        transaction.execute_batch(MIGRATION_0006).map_err(|error| {
            StorageError::Migration(format!("0006_attempt_reservations.sql failed: {error}"))
        })?;
    }
    if current < 7 {
        transaction.execute_batch(MIGRATION_0007).map_err(|error| {
            StorageError::Migration(format!(
                "0007_managed_identity_reservations.sql failed: {error}"
            ))
        })?;
    }
    if current < 8 {
        transaction.execute_batch(MIGRATION_0008).map_err(|error| {
            StorageError::Migration(format!("0008_approvals.sql failed: {error}"))
        })?;
    }
    if current < 9 {
        transaction.execute_batch(MIGRATION_0009).map_err(|error| {
            StorageError::Migration(format!("0009_secret_refs.sql failed: {error}"))
        })?;
    }
    if current < 10 {
        transaction.execute_batch(MIGRATION_0010).map_err(|error| {
            StorageError::Migration(format!("0010_security_audit.sql failed: {error}"))
        })?;
    }
    if current < 11 {
        transaction.execute_batch(MIGRATION_0011).map_err(|error| {
            StorageError::Migration(format!("0011_workspace_trusted_anchor.sql failed: {error}"))
        })?;
    }
    if current < 12 {
        transaction.execute_batch(MIGRATION_0012).map_err(|error| {
            StorageError::Migration(format!("0012_plan_persistence.sql failed: {error}"))
        })?;
    }
    if current < 13 {
        transaction.execute_batch(MIGRATION_0013).map_err(|error| {
            StorageError::Migration(format!("0013_plan_cost_receipts.sql failed: {error}"))
        })?;
    }
    if current < 14 {
        transaction.execute_batch(MIGRATION_0014).map_err(|error| {
            StorageError::Migration(format!("0014_plan_guardrails.sql failed: {error}"))
        })?;
    }
    if current < 15 {
        transaction.execute_batch(MIGRATION_0015).map_err(|error| {
            StorageError::Migration(format!("0015_human_decisions.sql failed: {error}"))
        })?;
    }
    if current < 16 {
        transaction.execute_batch(MIGRATION_0016).map_err(|error| {
            StorageError::Migration(format!("0016_g7_closure_remediation.sql failed: {error}"))
        })?;
    }
    if current < 17 {
        transaction.execute_batch(MIGRATION_0017).map_err(|error| {
            StorageError::Migration(format!("0017_evidence_requirements.sql failed: {error}"))
        })?;
    }
    if current < 18 {
        transaction.execute_batch(MIGRATION_0018).map_err(|error| {
            StorageError::Migration(format!(
                "0018_planning_input_evidence_requirements.sql failed: {error}"
            ))
        })?;
    }
    if current < 19 {
        transaction.execute_batch(MIGRATION_0019).map_err(|error| {
            StorageError::Migration(format!("0019_observed_effects.sql failed: {error}"))
        })?;
    }
    if current < 20 {
        transaction.execute_batch(MIGRATION_0020).map_err(|error| {
            StorageError::Migration(format!("0020_task_prerequisites.sql failed: {error}"))
        })?;
    }
    if current < 21 {
        transaction.execute_batch(MIGRATION_0021).map_err(|error| {
            StorageError::Migration(format!("0021_test_mutation_policy.sql failed: {error}"))
        })?;
    }
    if current < 22 {
        transaction.execute_batch(MIGRATION_0022).map_err(|error| {
            StorageError::Migration(format!(
                "0022_candidate_input_assessments.sql failed: {error}"
            ))
        })?;
    }
    transaction.execute_batch("PRAGMA user_version = 22")?;
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
