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
mod approved_proposal_codec;
mod audit_store;
mod candidate_assessment_store;
mod claim_verification_store;
mod context_store;
mod continuity_store;
mod entry_store;
mod error;
mod event;
mod human_decision_store;
mod journal_store;
mod memory_store;
mod migration;
mod observed_effect_store;
#[cfg(unix)]
mod path_security;
#[cfg(not(unix))]
#[path = "path_security_unsupported.rs"]
mod path_security;
mod plan_cost_store;
mod plan_guardrail_store;
mod plan_store;
mod planning_control_store;
mod review_authority_store;
mod routine_store;
mod runtime_store;
mod secret_store;
mod side_effect_store;
mod takeover_store;
mod task_store;
mod test_mutation_store;
mod workspace_store;

pub use approval_store::{
    ApprovalCapabilityRecord, ApprovalRecord, ApprovalState, NewApprovalRecord,
};
pub use candidate_assessment_store::{CandidateEvidence, CandidateSelectionOutcome};
pub use claim_verification_store::CompletionAdmissionRecord;
pub use entry_store::{
    AdmittedExecutionEntry, ExecutionEntryAdmission, PlanHeadPrecondition,
    PlanningInputPrecondition, StrandedPreAttemptEntry,
};
pub use error::{StorageError, StorageResult};
pub use evaluation::CandidateIntegrityAssessment;
pub use event::{
    AttemptOutcome, EventId, EventType, JournalEvent, NewJournalEvent, PlanRevisionJournalPayload,
    EVENT_SCHEMA_VERSION,
};
pub use human_decision_store::{ApprovedPlanProposal, NewHumanDecisionRequest};
pub use migration::SUPPORTED_SCHEMA_VERSION;
pub use plan_guardrail_store::{PlanGuardrailEvent, PlannerCallSettlement};
pub use plan_store::{
    ChildTaskMaterialization, CommitPlanRevisionRequest, PlanCommitJournalEvidence,
    PlanRevisionLineage, PlanRevisionLineageRecord, CHILD_TASK_MATERIALIZATION_PROVENANCE,
};
pub use planning_control_store::{PlanningRunRecord, PlanningRunState};
pub use review_authority_store::ReviewRecordState;
pub use security_audit::{
    AuditCapability, NewSecurityAuditRecord, SecurityAuditAuthor, SecurityAuditEvent,
    SecurityAuditId, SecurityAuditOutcome, SecurityAuditRecord, SecurityAuditSubject,
    SecurityAuditSubjectKind, SECURITY_AUDIT_RECORD_VERSION,
};
pub use side_effect_store::{SideEffectEscalation, SideEffectTransition};
pub use takeover_store::{
    AbandonTakeover, AdmitTakeover, AppendTakeover, CompleteTakeoverReturn, TakeoverCompletion,
};
pub use task_store::{BindingProofConsumption, SqliteStore};
pub use test_mutation_store::EvaluatedPolicyBase;

/// Fixture support for tamper tests. Re-chains the journal after a test has
/// planted rows with raw SQL, so decode-level corruption stays reachable
/// past the chain check. Not a production API: the chain is unkeyed, so
/// anyone with raw DB access can do this too (see journal_store.rs).
#[doc(hidden)]
pub mod test_support {
    use std::path::Path;

    use crate::{StorageError, StorageResult};

    pub fn reseal_journal_chain(path: &Path) -> StorageResult<()> {
        if !path.exists() {
            return Ok(());
        }
        let connection = rusqlite::Connection::open(path)?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 32 {
            return Ok(());
        }
        connection.execute_batch("DROP TRIGGER IF EXISTS event_journal_no_update")?;
        crate::journal_store::backfill_journal_chain(&connection)?;
        connection
            .execute_batch(crate::migration::MIGRATION_0032_GUARD)
            .map_err(|error| StorageError::Migration(error.to_string()))
    }
}
