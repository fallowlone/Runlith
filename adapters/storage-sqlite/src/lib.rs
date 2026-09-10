//! SQLite persistence adapter for the G2 Task domain core plus the G3
//! provider-neutral runtime domain (Executions/Attempts, ADR-0022).
//!
//! Boundary discipline: SQL and `rusqlite` types never leave this crate.
//! The adapter depends on `task-domain` and `agent-runtime` only through
//! their public validated APIs plus the snapshot storage ports:
//! snapshots are persisted, domain objects are rebuilt exclusively
//! through `restore`, so stored content can never bypass validation.
//!
//! Persisted current state only — projects, tasks, dependency edges,
//! list metadata, executions, attempt rows, schema version — plus the
//! single append-only Event Journal. No application mutation service,
//! no scheduler/provider/session concepts.

mod error;
mod event;
mod migration;
mod runtime_store;
mod task_store;

pub use error::{StorageError, StorageResult};
pub use event::{
    AttemptOutcome, EventId, EventType, JournalEvent, NewJournalEvent, EVENT_SCHEMA_VERSION,
};
pub use migration::SUPPORTED_SCHEMA_VERSION;
pub use task_store::SqliteStore;
