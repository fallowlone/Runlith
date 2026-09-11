//! Atomic execution-entry admission (TASK-0051, G5).
//!
//! The production start boundary (`OrchestrationService::enter`) used to
//! commit claim, then start, then execution creation as separate durable
//! transactions. Two barrier-synchronized entrants with the same new
//! execution (or attempt) identity could both pass the read-before-write
//! preflight and then poison the loser: `RUNNING` + `Claimed`/`Started`
//! journal facts with no Execution (or a background attempt collision
//! later). Store-level uniqueness fired too late to preserve honest state.
//!
//! This module is the single atomic commit for that boundary: both Task
//! lifecycle transitions (READY → CLAIMED → RUNNING), the Execution row,
//! the Attempt-identity reservation (migration 0006), and the three
//! matching journal facts commit in ONE `IMMEDIATE` transaction, or none
//! of them does. A conflict loser rolls everything back and keeps its
//! pre-run state with a typed [`StorageError::Duplicate`] or
//! [`StorageError::Stale`] — never a half-started Task.
//!
//! Discipline notes:
//!
//! - Task lifecycle *rules* still live in `task-domain` and are applied by
//!   the trusted application services before this call. This method
//!   re-checks coherence (right task, right states, right event scope)
//!   and enforces the expected-state guards *inside* the transaction, so
//!   a validation result that went stale before the write fails instead
//!   of overwriting newer state. No second Task state machine lives here.
//! - Git/filesystem effects stay outside by construction (INV-145): this
//!   transaction touches SQLite rows only — no lease, no worktree, no
//!   branch, no provider. External materialization remains a recoverable
//!   saga after admission.
//! - Crash before `COMMIT` admits zero lifecycle facts; crash after
//!   admits the complete entry (claim + start + execution + reservation +
//!   journal). There is no partial admission to reconcile.

use agent_runtime::{AttemptId, Execution};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use task_domain::{Task, TaskId, TaskState};

use crate::error::StorageError;
use crate::error::StorageResult;
use crate::event::{EventType, JournalEvent, NewJournalEvent};
use crate::runtime_store::require_execution_created_envelope;
use crate::task_store::{insert_journal_event, require_envelope};

/// The durable facts for one execution-entry admission. Everything here is
/// already domain-validated by the trusted application services (Task
/// transitions applied via `task-domain`, Execution begun, journal events
/// minted with service-owned provenance); this struct only carries them to
/// the single commit boundary. Provider-neutral: no provider, model,
/// session, or protocol data travels here.
pub struct ExecutionEntryAdmission<'a> {
    /// Task being entered (must be READY at commit time).
    pub task_id: &'a TaskId,
    /// Task after the validated `Claim` (READY → CLAIMED).
    pub claimed_task: &'a Task,
    /// Authoritative `task.claimed` evidence for the claim.
    pub claimed_event: &'a NewJournalEvent,
    /// Task after the validated `Start` (CLAIMED → RUNNING).
    pub started_task: &'a Task,
    /// Authoritative `task.started` evidence for the start.
    pub started_event: &'a NewJournalEvent,
    /// Execution begun for the task (no attempts yet).
    pub execution: &'a Execution,
    /// Authoritative `execution.created` evidence.
    pub execution_event: &'a NewJournalEvent,
    /// Future Attempt identity reserved for this entry's background half.
    /// Committed as an intent row, never as an Attempt fact.
    pub attempt_id: &'a AttemptId,
    /// Reservation timestamp minted by the calling service (services own
    /// clocks; the adapter never mints time).
    pub reserved_at: String,
}

/// The journal facts committed by one admission, in commit order.
pub struct AdmittedExecutionEntry {
    /// Committed `task.claimed` record.
    pub claimed_event: JournalEvent,
    /// Committed `task.started` record.
    pub started_event: JournalEvent,
    /// Committed `execution.created` record.
    pub execution_event: JournalEvent,
}

impl crate::task_store::SqliteStore {
    /// Commits one execution entry atomically: guarded READY → CLAIMED,
    /// guarded CLAIMED → RUNNING, Execution insert, Attempt-identity
    /// reservation, and the three journal facts — one `IMMEDIATE`
    /// transaction, all or nothing.
    ///
    /// Conflict contract (all rollback-clean, loser keeps pre-run state):
    ///
    /// - execution identity already taken → [`StorageError::Duplicate`];
    /// - attempt identity already created or reserved → [`StorageError::Duplicate`];
    /// - task no longer READY (a concurrent entrant moved first) →
    ///   [`StorageError::Stale`].
    pub fn admit_execution_entry(
        &mut self,
        admission: &ExecutionEntryAdmission<'_>,
    ) -> StorageResult<AdmittedExecutionEntry> {
        require_admission_coherence(admission)?;
        let transaction = self
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // -- guarded lifecycle transitions (expected-state compare) -------
        let claimed = transaction.execute(
            "UPDATE tasks SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![
                admission.claimed_task.state().as_str(),
                admission.task_id.as_str(),
                TaskState::Ready.as_str(),
            ],
        )?;
        if claimed == 0 {
            return Err(stale_or_missing(
                &transaction,
                admission.task_id,
                TaskState::Ready,
            )?);
        }
        let started = transaction.execute(
            "UPDATE tasks SET state = ?1 WHERE id = ?2 AND state = ?3",
            params![
                admission.started_task.state().as_str(),
                admission.task_id.as_str(),
                TaskState::Claimed.as_str(),
            ],
        )?;
        if started == 0 {
            // Unreachable through the public flow: the same transaction
            // just moved this row READY → CLAIMED. A zero here means the
            // row changed under the transaction, i.e. storage corruption.
            return Err(StorageError::Corrupt(format!(
                "task {} left READY but did not reach CLAIMED inside the entry transaction",
                admission.task_id
            )));
        }
        // -- identity conflicts (checked inside the transaction, after the
        // writer serialized at BEGIN IMMEDIATE, so no TOCTOU window) ------
        if let Some(owner) = execution_owner(&transaction, admission.execution.id())? {
            return Err(StorageError::Duplicate(format!(
                "execution {} already exists for task {}; supply a fresh execution identity",
                admission.execution.id(),
                owner
            )));
        }
        if let Some(owner) = attempt_owner(&transaction, admission.attempt_id)? {
            return Err(StorageError::Duplicate(format!(
                "attempt {} already exists for execution {}; supply a fresh attempt identity",
                admission.attempt_id, owner
            )));
        }
        if let Some((owner, task)) = reservation_owner(&transaction, admission.attempt_id)? {
            return Err(StorageError::Duplicate(format!(
                "attempt {} is already reserved by execution {owner} (task {task}); \
                 supply a fresh attempt identity",
                admission.attempt_id,
            )));
        }
        // -- admitted writes ----------------------------------------------
        transaction.execute(
            "INSERT INTO executions (id, task_id) VALUES (?1, ?2)",
            params![
                admission.execution.id().as_str(),
                admission.execution.task_id().as_str(),
            ],
        )?;
        transaction.execute(
            "INSERT INTO attempt_reservations (attempt_id, execution_id, task_id, reserved_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                admission.attempt_id.as_str(),
                admission.execution.id().as_str(),
                admission.task_id.as_str(),
                admission.reserved_at,
            ],
        )?;
        let claimed_event = insert_journal_event(&transaction, admission.claimed_event)?;
        let started_event = insert_journal_event(&transaction, admission.started_event)?;
        let execution_event = insert_journal_event(&transaction, admission.execution_event)?;
        transaction.commit()?;
        Ok(AdmittedExecutionEntry {
            claimed_event,
            started_event,
            execution_event,
        })
    }

    /// Reads the reservation owner for one Attempt identity, if any.
    /// Read-only helper for diagnostics and tests.
    pub fn get_attempt_reservation(
        &self,
        id: &AttemptId,
    ) -> StorageResult<Option<(agent_runtime::ExecutionId, TaskId)>> {
        let row: Option<(String, String)> = self
            .connection()
            .query_row(
                "SELECT execution_id, task_id FROM attempt_reservations WHERE attempt_id = ?1",
                params![id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(execution, task)| {
            let execution = agent_runtime::ExecutionId::new(execution).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored reservation owner: {error}"))
            })?;
            let task = TaskId::new(task).map_err(|error| {
                StorageError::DomainReconstruction(format!("stored reservation task: {error}"))
            })?;
            Ok((execution, task))
        })
        .transpose()
    }
}

/// Owning task of one stored execution, if any.
fn execution_owner(
    connection: &rusqlite::Connection,
    id: &agent_runtime::ExecutionId,
) -> StorageResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT task_id FROM executions WHERE id = ?1",
            params![id.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

/// Owning execution of one stored attempt, if any.
fn attempt_owner(
    connection: &rusqlite::Connection,
    id: &AttemptId,
) -> StorageResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT execution_id FROM execution_attempts WHERE attempt_id = ?1",
            params![id.as_str()],
            |row| row.get(0),
        )
        .optional()?)
}

/// Owning (execution, task) of one attempt reservation, if any.
fn reservation_owner(
    connection: &rusqlite::Connection,
    id: &AttemptId,
) -> StorageResult<Option<(String, String)>> {
    Ok(connection
        .query_row(
            "SELECT execution_id, task_id FROM attempt_reservations WHERE attempt_id = ?1",
            params![id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// Maps a zero-row guarded claim to missing-task vs moved-first.
fn stale_or_missing(
    connection: &rusqlite::Connection,
    task_id: &TaskId,
    expected: TaskState,
) -> StorageResult<StorageError> {
    let current: Option<String> = connection
        .query_row(
            "SELECT state FROM tasks WHERE id = ?1",
            params![task_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match current {
        None => StorageError::NotFound(format!("task {task_id} does not exist")),
        Some(found) => StorageError::Stale {
            task: task_id.to_string(),
            expected: expected.as_str().to_owned(),
            found,
        },
    })
}

/// Pre-transaction coherence: the admission must describe exactly one
/// READY → CLAIMED → RUNNING chain for one task, one fresh execution for
/// that task, and well-scoped journal evidence. Lifecycle *rules* were
/// already enforced by `task-domain` in the services; these checks only
/// prove the bundle is self-consistent before the transaction owns it.
fn require_admission_coherence(admission: &ExecutionEntryAdmission<'_>) -> StorageResult<()> {
    if admission.claimed_task.id() != admission.task_id
        || admission.started_task.id() != admission.task_id
    {
        return Err(StorageError::ConstraintViolation(
            "entry admission carries tasks for a different task than the entry target".to_owned(),
        ));
    }
    if admission.claimed_task.state() != TaskState::Claimed
        || admission.started_task.state() != TaskState::Running
    {
        return Err(StorageError::ConstraintViolation(
            "entry admission must carry a CLAIMED task and a RUNNING task".to_owned(),
        ));
    }
    if admission.claimed_task.project_id() != admission.started_task.project_id() {
        return Err(StorageError::ConstraintViolation(
            "entry admission claim and start disagree on project scope".to_owned(),
        ));
    }
    if !admission.execution.attempts().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "entry admission execution must carry no attempts yet".to_owned(),
        ));
    }
    if admission.execution.task_id() != admission.task_id {
        return Err(StorageError::ConstraintViolation(
            "entry admission execution targets a different task than the entry".to_owned(),
        ));
    }
    if admission.reserved_at.trim().is_empty() {
        return Err(StorageError::ConstraintViolation(
            "entry admission requires a reservation timestamp".to_owned(),
        ));
    }
    require_lifecycle_envelope(
        admission.claimed_event,
        EventType::Claimed,
        admission.task_id,
        admission.claimed_task.project_id(),
        TaskState::Ready,
        TaskState::Claimed,
    )?;
    require_lifecycle_envelope(
        admission.started_event,
        EventType::Started,
        admission.task_id,
        admission.started_task.project_id(),
        TaskState::Claimed,
        TaskState::Running,
    )?;
    require_execution_created_envelope(
        admission.execution,
        admission.started_task.project_id(),
        admission.execution_event,
    )?;
    Ok(())
}

/// Mirrors the write-path scope checks of `apply_lifecycle_transition`
/// for one admission event: right type, right task/project, exact
/// expected edge. The journal CHECK constraints enforce the same shape
/// in SQL; this fails fast with a typed error before the transaction.
fn require_lifecycle_envelope(
    event: &NewJournalEvent,
    expected_type: EventType,
    task_id: &TaskId,
    project_id: &task_domain::ProjectId,
    from: TaskState,
    to: TaskState,
) -> StorageResult<()> {
    if event.event_type != expected_type {
        return Err(StorageError::ConstraintViolation(format!(
            "entry admission requires a {expected_type} event"
        )));
    }
    if event.task_id.as_ref() != Some(task_id) || event.project_id != *project_id {
        return Err(StorageError::ConstraintViolation(
            "entry admission event references a different task or project than the state write"
                .to_owned(),
        ));
    }
    if event.from_state != Some(from) || event.to_state != Some(to) {
        return Err(StorageError::ConstraintViolation(
            "entry admission event states do not match the guarded state write".to_owned(),
        ));
    }
    require_envelope(event)
}
