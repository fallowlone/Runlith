//! Trusted Task mutation service: validated domain transition plus
//! authoritative journal record as one atomic commit.
//!
//! Authority split (ADR-0018): the agent-facing operation is
//! [`TaskService::submit_result`], which only reaches `SUBMITTED`.
//! [`TaskService::verification_succeeded`] is the trusted Evaluation-path
//! operation and the sole route to `COMPLETED`. There is no
//! `agent_complete_task()`, no `set_task_state()`, and no FAILED/CANCELLED
//! policy beyond what `task-domain` already decides (nothing — those edges
//! stay unresolved, so no such methods exist here either).

use std::path::Path;

use storage_sqlite::{EventType, JournalEvent, NewJournalEvent, SqliteStore, StorageError};
use task_domain::{Project, Task, TaskId, TransitionCommand};

use crate::clock::{format_rfc3339, Clock, SystemClock};
use crate::error::{ServiceError, ServiceResult};

/// Provenance marker for every fact this service commits. Fixed string —
/// never model narrative, never provider identity.
const PROVENANCE_SERVICE: &str = "task-application-service";

/// One committed mutation: the resulting current state plus its
/// authoritative journal record. Exactly one event per mutation — the two
/// are created by the same commit boundary.
#[derive(Debug, Clone)]
pub struct AppliedTransition {
    /// Current task state after the commit.
    pub task: Task,
    /// The journal record committed with it.
    pub event: JournalEvent,
}

/// Trusted owner of durable Task mutations.
pub struct TaskService<C: Clock = SystemClock> {
    store: SqliteStore,
    clock: C,
    op_counter: u64,
}

impl<C: Clock> TaskService<C> {
    /// Opens (creating/migrating) the database at `path`.
    pub fn open(path: impl AsRef<Path>, clock: C) -> ServiceResult<Self> {
        Ok(Self::attach(SqliteStore::open(path)?, clock))
    }

    /// Wraps an already-open store (setup fixtures, multi-connection
    /// tests). The store must be migrated, which [`SqliteStore::open`]
    /// guarantees.
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

    /// Persists a new project. Creation is service-owned so durable writes
    /// never bypass the trusted layer; it is not a lifecycle transition
    /// and appends no journal event.
    pub fn create_project(&mut self, project: &Project) -> ServiceResult<()> {
        Ok(self.store.create_project(project)?)
    }

    /// Persists a new task in its validated initial state. Same ownership
    /// rule as [`TaskService::create_project`]: no journal event, since no
    /// transition committed.
    pub fn create_task(&mut self, task: &Task) -> ServiceResult<()> {
        Ok(self.store.create_task(task)?)
    }

    /// `DRAFT → READY`.
    pub fn mark_ready(&mut self, task_id: &TaskId) -> ServiceResult<AppliedTransition> {
        self.mutate(task_id, TransitionCommand::MarkReady)
    }

    /// `READY → CLAIMED`.
    pub fn claim(&mut self, task_id: &TaskId) -> ServiceResult<AppliedTransition> {
        self.mutate(task_id, TransitionCommand::Claim)
    }

    /// `CLAIMED → RUNNING` (also blocked/retry resume).
    pub fn start(&mut self, task_id: &TaskId) -> ServiceResult<AppliedTransition> {
        self.mutate(task_id, TransitionCommand::Start)
    }

    /// `RUNNING → BLOCKED` with an explicit reason.
    pub fn block(
        &mut self,
        task_id: &TaskId,
        reason: impl Into<String>,
    ) -> ServiceResult<AppliedTransition> {
        self.mutate(
            task_id,
            TransitionCommand::Block {
                reason: reason.into(),
            },
        )
    }

    /// `RUNNING → SUBMITTED`: the agent-facing result submission. A claim
    /// of readiness — never completion.
    pub fn submit_result(
        &mut self,
        task_id: &TaskId,
        summary: impl Into<String>,
    ) -> ServiceResult<AppliedTransition> {
        self.mutate(
            task_id,
            TransitionCommand::SubmitResult {
                summary: summary.into(),
            },
        )
    }

    /// `SUBMITTED → AWAITING_REVIEW`.
    pub fn route_to_review(&mut self, task_id: &TaskId) -> ServiceResult<AppliedTransition> {
        self.mutate(task_id, TransitionCommand::RouteToReview)
    }

    /// `AWAITING_REVIEW → VERIFYING`.
    pub fn begin_verification(&mut self, task_id: &TaskId) -> ServiceResult<AppliedTransition> {
        self.mutate(task_id, TransitionCommand::BeginVerification)
    }

    /// `VERIFYING → RETRY_READY`: trusted REJECTED verdict. Evaluation
    /// path only.
    pub fn verification_failed(
        &mut self,
        task_id: &TaskId,
        verdict: impl Into<String>,
    ) -> ServiceResult<AppliedTransition> {
        self.mutate(
            task_id,
            TransitionCommand::VerificationFailed {
                verdict: verdict.into(),
            },
        )
    }

    /// `VERIFYING → COMPLETED`: trusted VERIFIED verdict. Evaluation path
    /// only — the sole route to `COMPLETED`.
    pub fn verification_succeeded(
        &mut self,
        task_id: &TaskId,
        verdict: impl Into<String>,
    ) -> ServiceResult<AppliedTransition> {
        self.mutate(
            task_id,
            TransitionCommand::VerificationSucceeded {
                verdict: verdict.into(),
            },
        )
    }

    /// Single choke point: load → validate via the canonical domain op →
    /// atomic guarded state write + journal append. Domain rejection fails
    /// before any SQL runs; storage failure rolls the whole transaction
    /// back. No blind retry: a failure is reported, never replayed.
    fn mutate(
        &mut self,
        task_id: &TaskId,
        command: TransitionCommand,
    ) -> ServiceResult<AppliedTransition> {
        let mut task = self.store.get_task(task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )))
        })?;
        let from = task.state();
        task.apply(command.clone())?;
        let to = task.state();
        let (event_type, reason) = classify(&command)?;
        let now = self.clock.now_millis();
        self.op_counter += 1;
        let event = NewJournalEvent {
            event_type,
            task_id: task_id.clone(),
            project_id: task.project_id().clone(),
            from_state: from,
            to_state: to,
            reason,
            // One trusted request mints both ids: the chain it belongs to
            // (correlation) and the request itself as the direct cause
            // (causation). Distinct values, real references — never derived
            // from adjacency, never fabricated from nothing.
            correlation_id: format!("op-{now}-{}", self.op_counter),
            causation_id: format!("req-{now}-{}", self.op_counter),
            occurred_at: format_rfc3339(now),
            provenance: format!("{PROVENANCE_SERVICE}/{}", op_name(&command)),
        };
        let committed = self
            .store
            .apply_lifecycle_transition(task_id, from, &task, &event)?;
        Ok(AppliedTransition {
            task,
            event: committed,
        })
    }
}

/// Maps a validated command to its canonical journal type plus the bounded
/// reason payload (block reason / submission summary / verdict). States
/// carry the transition; the reason carries the human/operator note.
/// Undecided transitions have no journal type: the domain rejects them
/// before this runs, so this errors closed if that ever changes.
fn classify(command: &TransitionCommand) -> ServiceResult<(EventType, Option<String>)> {
    Ok(match command {
        TransitionCommand::MarkReady => (EventType::MarkedReady, None),
        TransitionCommand::Claim => (EventType::Claimed, None),
        TransitionCommand::Start => (EventType::Started, None),
        TransitionCommand::Block { reason } => (EventType::Blocked, Some(reason.clone())),
        TransitionCommand::SubmitResult { summary } => {
            (EventType::Submitted, Some(summary.clone()))
        }
        TransitionCommand::RouteToReview => (EventType::RoutedToReview, None),
        TransitionCommand::BeginVerification => (EventType::VerificationBegan, None),
        TransitionCommand::VerificationSucceeded { verdict } => {
            (EventType::VerificationSucceeded, Some(verdict.clone()))
        }
        TransitionCommand::VerificationFailed { verdict } => {
            (EventType::VerificationFailed, Some(verdict.clone()))
        }
        TransitionCommand::Cancel { .. } | TransitionCommand::Fail { .. } => {
            return Err(ServiceError::Storage(StorageError::ConstraintViolation(
                "undecided transition has no journal type".to_owned(),
            )));
        }
    })
}

fn op_name(command: &TransitionCommand) -> &'static str {
    match command {
        TransitionCommand::MarkReady => "mark_ready",
        TransitionCommand::Claim => "claim",
        TransitionCommand::Start => "start",
        TransitionCommand::Block { .. } => "block",
        TransitionCommand::SubmitResult { .. } => "submit_result",
        TransitionCommand::RouteToReview => "route_to_review",
        TransitionCommand::BeginVerification => "begin_verification",
        TransitionCommand::VerificationSucceeded { .. } => "verification_succeeded",
        TransitionCommand::VerificationFailed { .. } => "verification_failed",
        TransitionCommand::Cancel { .. } => "cancel",
        TransitionCommand::Fail { .. } => "fail",
    }
}
