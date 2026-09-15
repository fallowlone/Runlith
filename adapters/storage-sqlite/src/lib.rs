//! SQLite persistence adapter for the G2 Task domain core, G3
//! provider-neutral runtime domain (Executions/Attempts, ADR-0022), and G7
//! durable Plan revisions (ADR-0027).
//!
//! Boundary discipline: SQL and `rusqlite` types never leave this crate.
//! The adapter depends on its domain crates only through their public
//! validated APIs plus snapshot/storage boundaries:
//! snapshots are persisted, domain objects are rebuilt exclusively
//! through `restore`, so stored content can never bypass validation.
//!
//! Persisted current state only — projects, tasks, dependency edges,
//! list metadata, executions, attempt rows, Plan revision snapshots, opaque
//! secret refs/bindings, schema version — plus the single append-only Event
//! Journal. Secret values never enter this adapter. No application mutation
//! service and no scheduler/provider/session concepts.

mod approval_store;
mod audit_store;
mod entry_store;
mod error;
mod event;
mod journal_store;
mod migration;
mod path_security;
mod plan_store;
mod runtime_store;
mod secret_store;
mod task_store;
mod workspace_store;

pub use approval_store::{
    ApprovalCapabilityRecord, ApprovalRecord, ApprovalState, NewApprovalRecord,
};
pub use entry_store::{AdmittedExecutionEntry, ExecutionEntryAdmission, StrandedPreAttemptEntry};
pub use error::{StorageError, StorageResult};
pub use event::{
    AttemptOutcome, EventId, EventType, JournalEvent, NewJournalEvent, PlanRevisionJournalPayload,
    EVENT_SCHEMA_VERSION,
};
pub use migration::SUPPORTED_SCHEMA_VERSION;
pub use plan_store::PlanCommitJournalEvidence;
pub use security_audit::{
    AuditCapability, NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditEvent,
    SecurityAuditId, SecurityAuditOutcome, SecurityAuditRecord, SecurityAuditSubject,
    SecurityAuditSubjectKind, SECURITY_AUDIT_RECORD_VERSION,
};
pub use task_store::SqliteStore;
