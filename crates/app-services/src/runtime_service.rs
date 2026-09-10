//! Trusted runtime mutation service: validated domain operation plus
//! authoritative ADR-0022 journal record as one atomic commit.
//!
//! Authority split: this service owns durable Execution/Attempt facts
//! only. It takes no scheduling decision (no readiness, provider
//! selection, retry policy, or concurrency), grants no Task completion,
//! and never touches Task lifecycle state — runtime success is evidence,
//! never a verdict. FakeAgent later reports results through this
//! boundary; it never writes storage or journals directly.
//!
//! Event authority mirrors [`TaskService`](super::TaskService): the
//! caller supplies identities and outcome intent, while this service
//! derives project scope from authoritative Task ownership and mints
//! correlation, causation, timestamp, provenance, aggregate references,
//! and the outcome field. Provenance is fixed
//! `runtime-application-service/<op>` — never model narrative, never
//! provider identity.

use std::path::Path;

use agent_runtime::{AttemptId, AttemptOutcome, Execution, ExecutionId, ProviderRef};
use storage_sqlite::{EventType, JournalEvent, NewJournalEvent, SqliteStore, StorageError};
use task_domain::TaskId;

use crate::clock::{format_rfc3339, Clock, SystemClock};
use crate::error::{ServiceError, ServiceResult};

/// Provenance marker for every fact this service commits. Fixed string —
/// never model narrative, never provider identity.
const PROVENANCE_SERVICE: &str = "runtime-application-service";

/// One committed runtime mutation: the resulting aggregate plus its
/// authoritative journal record. Exactly one event per mutation — the
/// two are created by the same commit boundary.
#[derive(Debug, Clone)]
pub struct AppliedExecution {
    /// Current execution aggregate after the commit.
    pub execution: Execution,
    /// The journal record committed with it.
    pub event: JournalEvent,
}

/// Trusted owner of durable runtime mutations.
pub struct RuntimeService<C: Clock = SystemClock> {
    store: SqliteStore,
    clock: C,
    op_counter: u64,
}

impl<C: Clock> RuntimeService<C> {
    /// Opens (creating/migrating) the database at `path`.
    pub fn open(path: impl AsRef<Path>, clock: C) -> ServiceResult<Self> {
        Ok(Self::attach(SqliteStore::open(path)?, clock))
    }

    /// Wraps an already-open store. The store must be migrated, which
    /// [`SqliteStore::open`] guarantees.
    pub fn attach(store: SqliteStore, clock: C) -> Self {
        Self {
            store,
            clock,
            op_counter: 0,
        }
    }

    /// Read access to the underlying store (current-state and journal
    /// queries for callers/tests).
    pub fn store(&self) -> &SqliteStore {
        &self.store
    }

    /// Persists a new execution for an existing task together with its
    /// authoritative `execution.created` journal evidence in one commit.
    /// The owning project is derived from the stored task — the caller
    /// supplies no project scope, so contradictory ownership is
    /// unrepresentable.
    pub fn create_execution(
        &mut self,
        execution_id: ExecutionId,
        task_id: TaskId,
    ) -> ServiceResult<AppliedExecution> {
        let task = self.store.get_task(&task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist for execution {execution_id}"
            )))
        })?;
        let execution = Execution::begin(execution_id.clone(), task_id.clone());
        let event = self.runtime_event(
            EventType::ExecutionCreated,
            task_id,
            task.project_id().clone(),
            Some(execution_id),
            None,
            None,
            "create_execution",
        );
        let committed = self.store.create_execution_with_event(&execution, &event)?;
        Ok(AppliedExecution {
            execution,
            event: committed,
        })
    }

    /// Persists a new open attempt inside an existing execution together
    /// with its authoritative `attempt.created` evidence in one commit.
    /// Sequential-attempt discipline comes from the domain: a still-open
    /// sibling rejects the call with no attempt and no event.
    pub fn create_attempt(
        &mut self,
        execution_id: &ExecutionId,
        attempt_id: AttemptId,
        provider: ProviderRef,
    ) -> ServiceResult<AppliedExecution> {
        let mut execution = self.load_execution(execution_id)?;
        execution.start_attempt(attempt_id.clone(), provider)?;
        let attempt = execution.get_attempt(&attempt_id).cloned().ok_or_else(|| {
            ServiceError::Storage(StorageError::Corrupt(format!(
                "attempt {attempt_id} vanished immediately after domain creation"
            )))
        })?;
        let project_id = self.project_for(&execution)?;
        let event = self.runtime_event(
            EventType::AttemptCreated,
            execution.task_id().clone(),
            project_id,
            Some(execution_id.clone()),
            Some(attempt_id),
            None,
            "create_attempt",
        );
        let committed = self.store.create_attempt_with_event(&attempt, &event)?;
        Ok(AppliedExecution {
            execution,
            event: committed,
        })
    }

    /// Records the single terminal outcome of an open attempt together
    /// with its authoritative `attempt.concluded` evidence in one
    /// commit. Conclude-once comes from the domain: a second conclusion
    /// fails explicitly with state and history unchanged and no event
    /// appended.
    pub fn conclude_attempt(
        &mut self,
        execution_id: &ExecutionId,
        attempt_id: &AttemptId,
        outcome: AttemptOutcome,
        detail: impl Into<String>,
    ) -> ServiceResult<AppliedExecution> {
        let mut execution = self.load_execution(execution_id)?;
        execution.conclude_attempt(attempt_id, outcome, detail)?;
        let stored_detail = execution
            .get_attempt(attempt_id)
            .and_then(|attempt| attempt.detail().map(str::to_owned))
            .ok_or_else(|| {
                ServiceError::Storage(StorageError::Corrupt(format!(
                    "attempt {attempt_id} vanished immediately after domain conclusion"
                )))
            })?;
        let project_id = self.project_for(&execution)?;
        let event = self.runtime_event(
            EventType::AttemptConcluded,
            execution.task_id().clone(),
            project_id,
            Some(execution_id.clone()),
            Some(attempt_id.clone()),
            Some(outcome),
            "conclude_attempt",
        );
        let committed =
            self.store
                .conclude_attempt_with_event(attempt_id, outcome, &stored_detail, &event)?;
        Ok(AppliedExecution {
            execution,
            event: committed,
        })
    }

    /// Reads one execution with its attempt history. Read-only: emits
    /// no journal evidence.
    pub fn get_execution(&self, id: &ExecutionId) -> ServiceResult<Option<Execution>> {
        Ok(self.store.get_execution(id)?)
    }

    /// Reads one attempt by identity. Read-only.
    pub fn get_attempt(&self, id: &AttemptId) -> ServiceResult<Option<agent_runtime::Attempt>> {
        Ok(self.store.get_attempt(id)?)
    }

    /// Traverses the whole journal in commit order. Read-only.
    pub fn list_events_ordered(&self) -> ServiceResult<Vec<JournalEvent>> {
        Ok(self.store.list_events_ordered()?)
    }

    /// Loads a stored execution or fails as not-found (never a default).
    fn load_execution(&self, id: &ExecutionId) -> ServiceResult<Execution> {
        self.store.get_execution(id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "execution {id} does not exist"
            )))
        })
    }

    /// Derives the owning project through trusted state
    /// (`Execution → TaskId → Task → ProjectId`). Caller-supplied
    /// project scope does not exist on this API, so a contradictory
    /// project cannot enter an event.
    fn project_for(&self, execution: &Execution) -> ServiceResult<task_domain::ProjectId> {
        let task = self.store.get_task(execution.task_id())?.ok_or_else(|| {
            ServiceError::Storage(StorageError::Corrupt(format!(
                "execution {} references missing task {}",
                execution.id(),
                execution.task_id()
            )))
        })?;
        Ok(task.project_id().clone())
    }

    /// Mints the journal evidence for one runtime mutation. Same
    /// provenance discipline as `TaskService`: the trusted service owns
    /// correlation, causation, timestamp, provenance, aggregate
    /// references, and the outcome field — never the caller, never the
    /// wire, never FakeAgent.
    #[allow(clippy::too_many_arguments)]
    fn runtime_event(
        &mut self,
        event_type: EventType,
        task_id: TaskId,
        project_id: task_domain::ProjectId,
        execution_id: Option<ExecutionId>,
        attempt_id: Option<AttemptId>,
        attempt_outcome: Option<AttemptOutcome>,
        op: &str,
    ) -> NewJournalEvent {
        let now = self.clock.now_millis();
        self.op_counter += 1;
        NewJournalEvent {
            event_type,
            task_id: Some(task_id),
            project_id,
            // Task lifecycle columns stay Task-only: always null here.
            from_state: None,
            to_state: None,
            reason: None,
            correlation_id: format!("op-{now}-{}", self.op_counter),
            causation_id: format!("req-{now}-{}", self.op_counter),
            occurred_at: format_rfc3339(now),
            provenance: format!("{PROVENANCE_SERVICE}/{op}"),
            execution_id,
            attempt_id,
            attempt_outcome: attempt_outcome.map(storage_sqlite::AttemptOutcome::from),
            // Runtime facts never carry workspace scope.
            workspace_id: None,
            from_workspace_state: None,
            to_workspace_state: None,
        }
    }
}
