//! Durable storage for provider-neutral Executions and Attempts plus
//! their ADR-0022 journal evidence.
//!
//! Write discipline mirrors the Task core: only validated domain values
//! are persisted, and every authoritative runtime mutation commits its
//! relational state change together with its journal record in one
//! SQLite transaction. There is no untyped `INSERT INTO executions`
//! path outside these methods and no journal append outside the same
//! transaction. Reads rebuild aggregates exclusively through
//! [`Execution::restore`](agent_runtime::Execution::restore), so stored
//! content can never bypass domain validation; current state stays
//! authoritative in `executions` / `execution_attempts` and is never
//! reconstructed by replaying the journal.

use agent_runtime::{
    Attempt, AttemptId, AttemptSnapshot, Execution, ExecutionId, ExecutionSnapshot, ProviderRef,
};
use rusqlite::{params, OptionalExtension};

use crate::error::{map_constraint, StorageError, StorageResult};
use crate::event::{AttemptOutcome, EventType, JournalEvent, NewJournalEvent};
use crate::task_store::insert_journal_event;

impl crate::task_store::SqliteStore {
    // -- reads ----------------------------------------------------------

    /// Loads one execution with its full ordered attempt history, or
    /// `None` when absent. Read-only: emits no journal evidence.
    /// Stored content that cannot form a valid domain object fails as
    /// corruption/reconstruction error — never silently repaired.
    pub fn get_execution(&self, id: &ExecutionId) -> StorageResult<Option<Execution>> {
        Ok(match self.load_execution_snapshot(id.as_str())? {
            None => None,
            Some(snapshot) => Some(Execution::restore(snapshot).map_err(|error| {
                StorageError::Corrupt(format!("stored execution {id}: {error}"))
            })?),
        })
    }

    /// Loads one attempt by identity, or `None` when absent. Read-only.
    /// An attempt whose execution row is gone (possible only when the
    /// database was mutated behind the adapter's back — foreign keys
    /// stop this on write) is corruption, never a detached attempt.
    pub fn get_attempt(&self, id: &AttemptId) -> StorageResult<Option<Attempt>> {
        let row: Option<AttemptRow> = self
            .connection()
            .query_row(
                "SELECT attempt_id, execution_id, position, provider_ref, outcome, detail
                 FROM execution_attempts WHERE attempt_id = ?1",
                params![id.as_str()],
                AttemptRow::read,
            )
            .optional()?;
        let row = match row {
            None => return Ok(None),
            Some(row) => row,
        };
        let execution_exists: bool = self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE id = ?1)",
            params![row.execution_id],
            |row| row.get(0),
        )?;
        if !execution_exists {
            return Err(StorageError::Corrupt(format!(
                "attempt {} references missing execution {}",
                row.attempt_id, row.execution_id
            )));
        }
        self.restore_attempt(row).map(Some)
    }

    // -- atomic commits ---------------------------------------------------

    /// Persists a new validated execution together with its authoritative
    /// `execution.created` journal evidence in one transaction. The
    /// referenced Task must already be stored; the event scope must
    /// describe exactly this execution under that task's project.
    pub fn create_execution_with_event(
        &mut self,
        execution: &Execution,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        let task = self.get_task(execution.task_id())?.ok_or_else(|| {
            StorageError::NotFound(format!(
                "task {} does not exist for execution {}",
                execution.task_id(),
                execution.id()
            ))
        })?;
        if self.get_execution(execution.id())?.is_some() {
            return Err(StorageError::Duplicate(format!(
                "execution {} already exists",
                execution.id()
            )));
        }
        require_execution_created_envelope(execution, &task.project_id().clone(), event)?;
        let transaction = self.connection_mut().transaction()?;
        transaction
            .execute(
                "INSERT INTO executions (id, task_id) VALUES (?1, ?2)",
                params![execution.id().as_str(), execution.task_id().as_str()],
            )
            .map_err(|error| map_constraint(error, "create execution"))?;
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Persists a new validated open attempt together with its
    /// authoritative `attempt.created` journal evidence in one
    /// transaction. Sequential-attempt discipline is enforced inside the
    /// transaction: an open sibling rejects the write with no attempt
    /// and no event. Position is assigned from the stored history
    /// length, so aggregate order survives regardless of identifier
    /// spelling.
    pub fn create_attempt_with_event(
        &mut self,
        attempt: &Attempt,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        if !attempt.is_open() {
            return Err(StorageError::ConstraintViolation(format!(
                "attempt {} is not open; only an open attempt can be created",
                attempt.id()
            )));
        }
        require_attempt_created_envelope(attempt, event)?;
        let transaction = self.connection_mut().transaction()?;
        let execution_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM executions WHERE id = ?1)",
            params![attempt.execution_id().as_str()],
            |row| row.get(0),
        )?;
        if !execution_exists {
            return Err(StorageError::NotFound(format!(
                "execution {} does not exist for attempt {}",
                attempt.execution_id(),
                attempt.id()
            )));
        }
        let open: Option<String> = transaction
            .query_row(
                "SELECT attempt_id FROM execution_attempts
                 WHERE execution_id = ?1 AND outcome IS NULL",
                params![attempt.execution_id().as_str()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(open) = open {
            return Err(StorageError::ConstraintViolation(format!(
                "execution {} still runs open attempt {open}; conclude it first",
                attempt.execution_id()
            )));
        }
        let position: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM execution_attempts WHERE execution_id = ?1",
            params![attempt.execution_id().as_str()],
            |row| row.get(0),
        )?;
        transaction
            .execute(
                "INSERT INTO execution_attempts
                 (attempt_id, execution_id, position, provider_ref, outcome, detail)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL)",
                params![
                    attempt.id().as_str(),
                    attempt.execution_id().as_str(),
                    position,
                    attempt.provider().as_str(),
                ],
            )
            .map_err(|error| map_constraint(error, "create attempt"))?;
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    /// Records the single terminal outcome of an open attempt together
    /// with its authoritative `attempt.concluded` journal evidence in
    /// one transaction. The write is guarded: only an open row updates,
    /// so a second conclusion fails explicitly with state and history
    /// unchanged and no event appended.
    pub fn conclude_attempt_with_event(
        &mut self,
        attempt_id: &AttemptId,
        outcome: agent_runtime::AttemptOutcome,
        detail: &str,
        event: &NewJournalEvent,
    ) -> StorageResult<JournalEvent> {
        if detail.trim().is_empty() {
            return Err(StorageError::ConstraintViolation(
                "attempt conclusion requires a non-blank detail".to_owned(),
            ));
        }
        require_attempt_concluded_envelope(attempt_id, outcome, event)?;
        let transaction = self.connection_mut().transaction()?;
        let updated = transaction.execute(
            "UPDATE execution_attempts SET outcome = ?1, detail = ?2
             WHERE attempt_id = ?3 AND outcome IS NULL",
            params![
                AttemptOutcome::from(outcome).as_str(),
                detail.trim(),
                attempt_id.as_str(),
            ],
        )?;
        if updated == 0 {
            let current: Option<Option<String>> = transaction
                .query_row(
                    "SELECT outcome FROM execution_attempts WHERE attempt_id = ?1",
                    params![attempt_id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            return Err(match current {
                None => StorageError::NotFound(format!("attempt {attempt_id} does not exist")),
                Some(_) => StorageError::ConstraintViolation(format!(
                    "attempt {attempt_id} is already concluded; history is immutable"
                )),
            });
        }
        let committed = insert_journal_event(&transaction, event)?;
        transaction.commit()?;
        Ok(committed)
    }

    // -- internals --------------------------------------------------------

    fn load_execution_snapshot(&self, id: &str) -> StorageResult<Option<ExecutionSnapshot>> {
        let row: Option<(String, String)> = self
            .connection()
            .query_row(
                "SELECT id, task_id FROM executions WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (execution_id, task_id) = match row {
            None => return Ok(None),
            Some(row) => row,
        };
        let execution_id = ExecutionId::new(execution_id).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored execution id: {error}"))
        })?;
        let task_id = task_domain::TaskId::new(task_id).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored execution task reference: {error}"))
        })?;
        // An execution in a task that has no row is corrupt data
        // (foreign keys stop this on write; this stops it on read when
        // the database was mutated behind the adapter's back).
        if self.get_task(&task_id)?.is_none() {
            return Err(StorageError::Corrupt(format!(
                "execution {execution_id} references missing task {task_id}"
            )));
        }
        let mut statement = self.connection().prepare(
            "SELECT attempt_id, execution_id, position, provider_ref, outcome, detail
             FROM execution_attempts WHERE execution_id = ?1 ORDER BY position",
        )?;
        let rows = statement.query_map(params![execution_id.as_str()], AttemptRow::read)?;
        let mut attempts = Vec::new();
        for (expected_position, row) in (0_i64..).zip(rows) {
            let row = row?;
            if row.position != expected_position {
                return Err(StorageError::Corrupt(format!(
                    "execution {execution_id} stores non-contiguous attempt order \
                     (expected position {expected_position}, found {})",
                    row.position
                )));
            }
            attempts.push(self.restore_attempt_snapshot(row)?);
        }
        Ok(Some(ExecutionSnapshot {
            id: execution_id,
            task_id,
            attempts,
        }))
    }

    fn restore_attempt(&self, row: AttemptRow) -> StorageResult<Attempt> {
        let snapshot = self.restore_attempt_snapshot(row)?;
        Attempt::restore(snapshot).map_err(|error| {
            StorageError::Corrupt(format!("stored attempt violates discipline: {error}"))
        })
    }

    fn restore_attempt_snapshot(&self, row: AttemptRow) -> StorageResult<AttemptSnapshot> {
        let id = AttemptId::new(row.attempt_id).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored attempt id: {error}"))
        })?;
        let execution_id = ExecutionId::new(row.execution_id).map_err(|error| {
            StorageError::DomainReconstruction(format!(
                "stored attempt execution reference: {error}"
            ))
        })?;
        let provider = ProviderRef::new(row.provider_ref).map_err(|error| {
            StorageError::DomainReconstruction(format!("stored provider reference: {error}"))
        })?;
        let outcome = row
            .outcome
            .map(|raw| AttemptOutcome::parse(&raw).map(agent_runtime::AttemptOutcome::from))
            .transpose()?;
        Ok(AttemptSnapshot {
            id,
            execution_id,
            provider,
            outcome,
            detail: row.detail,
        })
    }
}

struct AttemptRow {
    attempt_id: String,
    execution_id: String,
    position: i64,
    provider_ref: String,
    outcome: Option<String>,
    detail: Option<String>,
}

impl AttemptRow {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            attempt_id: row.get(0)?,
            execution_id: row.get(1)?,
            position: row.get(2)?,
            provider_ref: row.get(3)?,
            outcome: row.get(4)?,
            detail: row.get(5)?,
        })
    }
}

/// Validates that `event` is the `execution.created` evidence for
/// exactly `execution` under the authoritative owning project.
fn require_execution_created_envelope(
    execution: &Execution,
    project_id: &task_domain::ProjectId,
    event: &NewJournalEvent,
) -> StorageResult<()> {
    if event.event_type != EventType::ExecutionCreated {
        return Err(StorageError::ConstraintViolation(
            "execution creation requires an execution.created event".to_owned(),
        ));
    }
    if event.execution_id.as_ref() != Some(execution.id())
        || event.task_id.as_ref() != Some(execution.task_id())
        || event.project_id != *project_id
    {
        return Err(StorageError::ConstraintViolation(
            "execution.created event references a different execution, task, or project \
             than the state write"
                .to_owned(),
        ));
    }
    if event.attempt_id.is_some()
        || event.attempt_outcome.is_some()
        || event.from_state.is_some()
        || event.to_state.is_some()
    {
        return Err(StorageError::ConstraintViolation(
            "execution.created event must not carry attempt scope, an outcome, \
             or Task lifecycle states"
                .to_owned(),
        ));
    }
    super::task_store::require_envelope(event)
}

/// Validates that `event` is the `attempt.created` evidence for exactly
/// `attempt` (which must still be open).
fn require_attempt_created_envelope(
    attempt: &Attempt,
    event: &NewJournalEvent,
) -> StorageResult<()> {
    if event.event_type != EventType::AttemptCreated {
        return Err(StorageError::ConstraintViolation(
            "attempt creation requires an attempt.created event".to_owned(),
        ));
    }
    if event.execution_id.as_ref() != Some(attempt.execution_id())
        || event.attempt_id.as_ref() != Some(attempt.id())
    {
        return Err(StorageError::ConstraintViolation(
            "attempt.created event references a different execution or attempt \
             than the state write"
                .to_owned(),
        ));
    }
    if event.attempt_outcome.is_some() || event.from_state.is_some() || event.to_state.is_some() {
        return Err(StorageError::ConstraintViolation(
            "attempt.created event must not carry an outcome or Task lifecycle states".to_owned(),
        ));
    }
    super::task_store::require_envelope(event)
}

/// Validates that `event` is the `attempt.concluded` evidence recording
/// exactly `outcome` for `attempt_id`.
fn require_attempt_concluded_envelope(
    attempt_id: &AttemptId,
    outcome: agent_runtime::AttemptOutcome,
    event: &NewJournalEvent,
) -> StorageResult<()> {
    if event.event_type != EventType::AttemptConcluded {
        return Err(StorageError::ConstraintViolation(
            "attempt conclusion requires an attempt.concluded event".to_owned(),
        ));
    }
    if event.attempt_id.as_ref() != Some(attempt_id)
        || event.attempt_outcome != Some(AttemptOutcome::from(outcome))
    {
        return Err(StorageError::ConstraintViolation(
            "attempt.concluded event records a different attempt or outcome \
             than the state write"
                .to_owned(),
        ));
    }
    if event.execution_id.is_none() || event.from_state.is_some() || event.to_state.is_some() {
        return Err(StorageError::ConstraintViolation(
            "attempt.concluded event requires execution scope and must not carry \
             Task lifecycle states"
                .to_owned(),
        ));
    }
    super::task_store::require_envelope(event)
}
