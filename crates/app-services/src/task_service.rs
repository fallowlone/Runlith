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

use agent_runtime::ExecutionId;
use plan_domain::{PlanDomainError, TaskPlanningAuthority};
use storage_sqlite::{EventType, JournalEvent, NewJournalEvent, SqliteStore, StorageError};
use task_domain::{DomainError, Project, ProjectId, Task, TaskId, TaskState, TransitionCommand};
use workspace::{LeaseLifecycle, WorkspaceStore};

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

/// A domain-validated Task transition that is minted but not yet
/// committed: the resulting task plus its authoritative journal record.
/// The caller must commit both through one atomic store boundary (single
/// transition via `commit_planned`, or the multi-fact entry transaction
/// for claim+start). Nothing is persisted and no event is appended here.
#[derive(Debug, Clone)]
pub struct PlannedTransition {
    /// Task after the validated command.
    pub task: Task,
    /// Journal record to commit with it.
    pub event: NewJournalEvent,
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

    /// Mutable access to the underlying store for multi-fact atomic
    /// boundaries owned elsewhere (the execution-entry admission commits
    /// through exactly one store handle so its transaction is truly one
    /// transaction). No domain rule lives in the caller's use of this:
    /// every fact committed through it must already be domain-validated.
    pub fn store_mut(&mut self) -> &mut SqliteStore {
        &mut self.store
    }

    /// Persists a new project together with its authoritative
    /// `project.created` journal evidence in one commit. Creation is
    /// service-owned so durable writes never bypass the trusted layer.
    pub fn create_project(&mut self, project: &Project) -> ServiceResult<JournalEvent> {
        let event = self.creation_event(
            EventType::ProjectCreated,
            None,
            project.id().clone(),
            None,
            None,
            None,
            "create_project",
        );
        Ok(self.store.create_project_with_event(project, &event)?)
    }

    /// Persists a new task in its validated initial state together with
    /// its authoritative `task.created` journal evidence in one commit.
    pub fn create_task(&mut self, task: &Task) -> ServiceResult<JournalEvent> {
        let state = task.state();
        let event = self.creation_event(
            EventType::TaskCreated,
            Some(task.id().clone()),
            task.project_id().clone(),
            Some(state),
            Some(state),
            None,
            "create_task",
        );
        Ok(self.store.create_task_with_event(task, &event)?)
    }

    /// Binds the current policy to the base OID in the durable lease owned by
    /// this Task's Execution. No base revision is accepted from the caller.
    pub fn bind_evaluated_policy_base(
        &mut self,
        execution_id: &ExecutionId,
    ) -> ServiceResult<storage_sqlite::EvaluatedPolicyBase> {
        let execution = self.store.get_execution(execution_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "execution {execution_id} does not exist"
            )))
        })?;
        let lease = self
            .store
            .get_workspace_for_execution(execution_id)
            .map_err(|error| {
                ServiceError::Storage(StorageError::ConstraintViolation(error.to_string()))
            })?
            .ok_or_else(|| {
                ServiceError::Storage(StorageError::NotFound(format!(
                    "execution {execution_id} has no workspace lease"
                )))
            })?;
        if lease.lifecycle() != LeaseLifecycle::Ready {
            return Err(ServiceError::Storage(StorageError::ConstraintViolation(
                "evaluated policy base requires a READY workspace lease".into(),
            )));
        }
        Ok(self.store.bind_evaluated_policy_base(
            execution_id.as_str(),
            execution.task_id().as_str(),
            lease.id().as_str(),
            lease.base_revision().as_str(),
        )?)
    }

    /// Creates a DRAFT Task whose canonical planning metadata — allowed
    /// scope, acceptance obligations and baseline risk — commits atomically
    /// with its `task.created` evidence (TASK-0084). The metadata must derive
    /// into G7 planning authority through the same derivation that planning
    /// and Plan publication use, so nothing underivable is ever persisted.
    /// The control plane exposes no later edit of this intent.
    pub fn create_planning_task(
        &mut self,
        id: TaskId,
        project_id: ProjectId,
        objective: impl Into<String>,
        allowed_paths: Vec<String>,
        acceptance: Vec<String>,
        baseline_risk: impl Into<String>,
    ) -> ServiceResult<(Task, JournalEvent)> {
        let mut task = Task::draft(id, project_id, objective)?;
        task.define_planning_metadata(allowed_paths, acceptance, baseline_risk)?;
        let facts = task.snapshot();
        TaskPlanningAuthority::derive(
            &facts.allowed_paths,
            &facts.forbidden_paths,
            &facts.acceptance,
            &facts.evidence_requirements,
            facts.risk.as_deref(),
        )
        .map_err(|PlanDomainError::InvalidInput(detail)| {
            DomainError::InvalidInput(format!("planning metadata {detail}"))
        })?;
        let event = self.create_task(&task)?;
        Ok((task, event))
    }

    /// Adds `task_id depends on depends_on_id` together with its
    /// authoritative `task.dependency_added` journal evidence in one
    /// commit. Domain pre-validation runs first so self-edge, duplicate,
    /// cross-project, and cycle rejections surface as domain errors;
    /// storage re-validates before writing.
    pub fn add_dependency(
        &mut self,
        task_id: &TaskId,
        depends_on_id: &TaskId,
    ) -> ServiceResult<JournalEvent> {
        let task = self
            .store
            .get_task(task_id)?
            .ok_or_else(|| missing_task(task_id))?;
        let target = self
            .store
            .get_task(depends_on_id)?
            .ok_or_else(|| missing_task(depends_on_id))?;
        let mut candidate = task.clone();
        candidate.add_dependency(&target)?;
        let mut graph = self.store.load_project_graph(task.project_id())?;
        if let Some(position) = graph.iter().position(|t| t.id() == task_id) {
            graph[position] = candidate;
        }
        task_domain::validate_acyclic(&graph)?;
        let state = task.state();
        let event = self.creation_event(
            EventType::DependencyAdded,
            Some(task_id.clone()),
            task.project_id().clone(),
            Some(state),
            Some(state),
            None,
            "add_dependency",
        );
        Ok(self
            .store
            .add_dependency_with_event(task_id, depends_on_id, &event)?)
    }

    /// Reads one project by id. Read-only: emits no journal evidence.
    pub fn get_project(&self, id: &ProjectId) -> ServiceResult<Option<Project>> {
        Ok(self.store.get_project(id)?)
    }

    /// Lists all projects in id order. Read-only.
    pub fn list_projects(&self) -> ServiceResult<Vec<Project>> {
        Ok(self.store.list_projects()?)
    }

    /// Reads one task by id. Read-only.
    pub fn get_task(&self, id: &TaskId) -> ServiceResult<Option<Task>> {
        Ok(self.store.get_task(id)?)
    }

    /// Lists every task of one project in id order. Read-only.
    pub fn list_tasks_for_project(&self, project_id: &ProjectId) -> ServiceResult<Vec<Task>> {
        Ok(self.store.list_tasks_for_project(project_id)?)
    }

    /// Lists the direct dependencies of one task in id order. Read-only.
    pub fn list_dependencies(&self, task_id: &TaskId) -> ServiceResult<Vec<TaskId>> {
        Ok(self.store.list_dependencies(task_id)?)
    }

    /// Reads one task's journal records in commit order. Read-only.
    pub fn list_events_for_task(&self, task_id: &TaskId) -> ServiceResult<Vec<JournalEvent>> {
        Ok(self.store.list_events_for_task(task_id)?)
    }

    /// Reads one project's journal records in commit order. Read-only.
    pub fn list_events_for_project(
        &self,
        project_id: &ProjectId,
    ) -> ServiceResult<Vec<JournalEvent>> {
        Ok(self.store.list_events_for_project(project_id)?)
    }

    /// Validates `READY → CLAIMED` against the stored task and mints its
    /// journal evidence without committing anything. The returned plan
    /// must be committed through one atomic store boundary (the
    /// execution-entry admission). Domain rejection fails before any SQL
    /// runs. Nothing is persisted and no event is appended here.
    pub fn plan_claim(&mut self, task_id: &TaskId) -> ServiceResult<PlannedTransition> {
        let task = self.store.get_task(task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )))
        })?;
        self.plan_on(&task, TransitionCommand::Claim)
    }

    /// Validates `CLAIMED → RUNNING` against an already-claimed task
    /// object (normally the [`PlannedTransition::task`] from
    /// [`Self::plan_claim`]) and mints its journal evidence without
    /// committing anything. Same no-persistence contract as
    /// [`Self::plan_claim`].
    pub fn plan_start(&mut self, claimed: &Task) -> ServiceResult<PlannedTransition> {
        self.plan_on(claimed, TransitionCommand::Start)
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

    /// F1 startup sweep for tasks stranded by a crash between lifecycle
    /// steps. Only `SUBMITTED → AWAITING_REVIEW` is routed: it is pure
    /// bookkeeping through the validated transition and decides nothing.
    /// `AWAITING_REVIEW`, `VERIFYING` and `RETRY_READY` are reported for
    /// the operator: leaving them needs a trusted verdict or a new
    /// execution, which a sweep must never invent. Idempotent; run once
    /// at startup, when the single-instance daemon has no live runs.
    pub fn reconcile_stranded(&mut self) -> ServiceResult<ReconcileReport> {
        let mut report = ReconcileReport::default();
        for project in self.store.list_projects()? {
            for task in self.store.list_tasks_for_project(project.id())? {
                match task.state() {
                    TaskState::Submitted => {
                        let mut planned = self.plan_on(&task, TransitionCommand::RouteToReview)?;
                        planned.event.provenance =
                            format!("{PROVENANCE_SERVICE}/reconcile/route_to_review");
                        self.store.apply_lifecycle_transition(
                            task.id(),
                            TaskState::Submitted,
                            &planned.task,
                            &planned.event,
                        )?;
                        report.routed.push(task.id().clone());
                    }
                    state @ (TaskState::AwaitingReview
                    | TaskState::Verifying
                    | TaskState::RetryReady) => {
                        report.needs_operator.push((task.id().clone(), state));
                    }
                    _ => {}
                }
            }
        }
        Ok(report)
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
        let task = self.store.get_task(task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )))
        })?;
        let from = task.state();
        let planned = self.plan_on(&task, command)?;
        let committed =
            self.store
                .apply_lifecycle_transition(task_id, from, &planned.task, &planned.event)?;
        Ok(AppliedTransition {
            task: planned.task,
            event: committed,
        })
    }

    /// Validates one command against one task object via the canonical
    /// domain op and mints its journal evidence. Shared by the immediate
    /// [`Self::mutate`] path and the deferred entry-admission planners
    /// ([`Self::plan_claim`], [`Self::plan_start`]) so both spell the
    /// validation boundary exactly once. Nothing is persisted here.
    fn plan_on(
        &mut self,
        task: &Task,
        command: TransitionCommand,
    ) -> ServiceResult<PlannedTransition> {
        let mut next = task.clone();
        let from = next.state();
        next.apply(command.clone())?;
        let to = next.state();
        let (event_type, reason) = classify(&command)?;
        let now = self.clock.now_millis();
        self.op_counter += 1;
        let event = NewJournalEvent {
            event_type,
            task_id: Some(next.id().clone()),
            project_id: Some(next.project_id().clone()),
            from_state: Some(from),
            to_state: Some(to),
            reason,
            // One trusted request mints both ids: the chain it belongs to
            // (correlation) and the request itself as the direct cause
            // (causation). Distinct values, real references — never derived
            // from adjacency, never fabricated from nothing.
            correlation_id: crate::op_id::correlation_id(now, self.op_counter),
            causation_id: crate::op_id::causation_id(now, self.op_counter),
            occurred_at: format_rfc3339(now),
            provenance: format!("{PROVENANCE_SERVICE}/{op}", op = op_name(&command)),
            // Task lifecycle facts never carry runtime scope.
            execution_id: None,
            attempt_id: None,
            attempt_outcome: None,
            // Task lifecycle facts never carry workspace scope.
            workspace_id: None,
            from_workspace_state: None,
            to_workspace_state: None,
            plan_revision: None,
        };
        Ok(PlannedTransition { task: next, event })
    }

    /// Mints the journal evidence for one creation/dependency mutation.
    /// Same provenance discipline as [`TaskService::mutate`]: the trusted
    /// service owns correlation, causation, timestamp, and provenance —
    /// never the caller, never the wire.
    #[allow(clippy::too_many_arguments)]
    fn creation_event(
        &mut self,
        event_type: EventType,
        task_id: Option<TaskId>,
        project_id: ProjectId,
        from_state: Option<task_domain::TaskState>,
        to_state: Option<task_domain::TaskState>,
        reason: Option<String>,
        op: &str,
    ) -> NewJournalEvent {
        let now = self.clock.now_millis();
        self.op_counter += 1;
        NewJournalEvent {
            event_type,
            task_id,
            project_id: Some(project_id),
            from_state,
            to_state,
            reason,
            correlation_id: crate::op_id::correlation_id(now, self.op_counter),
            causation_id: crate::op_id::causation_id(now, self.op_counter),
            occurred_at: format_rfc3339(now),
            provenance: format!("{PROVENANCE_SERVICE}/{op}"),
            // Creation/dependency facts never carry runtime scope.
            execution_id: None,
            attempt_id: None,
            attempt_outcome: None,
            // Creation/dependency facts never carry workspace scope.
            workspace_id: None,
            from_workspace_state: None,
            to_workspace_state: None,
            plan_revision: None,
        }
    }
}

/// Storage-shaped not-found for a task id the service expected to exist.
fn missing_task(task_id: &TaskId) -> ServiceError {
    ServiceError::Storage(StorageError::NotFound(format!(
        "task {task_id} does not exist"
    )))
}

/// Outcome of [`TaskService::reconcile_stranded`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReconcileReport {
    /// `SUBMITTED` tasks the sweep routed to `AWAITING_REVIEW`.
    pub routed: Vec<TaskId>,
    /// Tasks left in a state only an operator or evaluator can advance.
    pub needs_operator: Vec<(TaskId, TaskState)>,
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
