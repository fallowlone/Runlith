//! SQLite persistence adapter for the G2 Task domain core.
//!
//! Boundary discipline: SQL and `rusqlite` types never leave this crate.
//! The adapter depends on `task-domain` only through its public validated
//! API plus the [`TaskSnapshot`](task_domain::TaskSnapshot) storage port:
//! snapshots are persisted, tasks are rebuilt exclusively through
//! [`Task::restore`](task_domain::Task::restore), so stored content can
//! never bypass domain validation.
//!
//! Persisted current state only — projects, tasks, dependency edges, list
//! metadata, schema version. No event journal, no application mutation
//! service, no execution/scheduler/provider concepts.

mod error;
mod event;
mod migration;
mod task_store;

pub use error::{StorageError, StorageResult};
pub use event::{EventId, EventType, JournalEvent, NewJournalEvent, EVENT_SCHEMA_VERSION};
pub use migration::SUPPORTED_SCHEMA_VERSION;
pub use task_store::SqliteStore;
