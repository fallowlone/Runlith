//! Trusted single-task orchestration (TASK-0037, G3): the smallest
//! deterministic `Task → Execution → Attempt → submit → verify` cycle.
//!
//! This service sequences the existing trusted mutations only — every Task
//! lifecycle change goes through [`TaskService`], every Execution/Attempt
//! change through [`RuntimeService`]. It writes no table directly, invents
//! no transition, retries nothing, plans nothing, routes no provider, and
//! enforces no policy. The executor ([`scheduler::ExecutionPort`]) and the
//! independent evaluator ([`evaluation::EvaluationPort`]) are injected per
//! call; for G3 tests those are the FakeAgent wrapper and `FakeEvaluator`.
//!
//! Durability boundaries: entry (readiness, claim, start, execution,
//! attempt reservation, managed-identity reservation, journal) is ONE atomic
//! commit
//! ([`SqliteStore::admit_execution_entry`](storage_sqlite::SqliteStore::admit_execution_entry)).
//! The executor runs outside every SQLite transaction (service calls
//! return before it is invoked), so a crash leaves a legitimate
//! recoverable intermediate state (`RUNNING` + `E1` + reserved `A1`)
//! instead of a hidden in-memory mega-transaction.
//!
//! ```text
//! readiness (derived, read-only)
//! → plan claim + plan start + plan execution (validated, uncommitted)
//! → atomic admission: claim + start + execution + attempt reservation
//!   + journal facts (one durable commit)
//! → [executor runs outside any transaction]
//! → RuntimeService::create_attempt (consumes the reservation) (durable)
//! → RuntimeService::conclude_attempt                 (durable)
//! → TaskService::submit_result                       (durable, if submittable)
//! → TaskService::route_to_review → begin_verification (durable)
//! → [independent evaluator judges]
//! → TaskService::verification_succeeded | _failed    (durable)
//! ```

use std::path::Path;

use evaluation::{EvaluationPort, EvaluationRequest, EvaluationVerdict};
use scheduler::{
    derive_readiness, interpret, AttemptInterpretation, ExecutionPort, OrchestrationOutcome,
    ScheduleReadiness, WorkItem,
};
use storage_sqlite::{SqliteStore, StorageError};
use task_domain::{DomainError, Task, TaskId, TaskState};
use workspace::{GitPort, WorkspaceLease, WorkspaceStore};

use crate::workspace_service::{WorkspaceRequest, WorkspaceService};

use crate::clock::{Clock, SystemClock};
use crate::error::{ServiceError, ServiceResult};
use crate::runtime_service::RuntimeService;
use crate::task_service::TaskService;

/// Caller-supplied deterministic identities for the one Execution/Attempt
/// this cycle may create. The domain validates them; the orchestrator
/// invents no random/UUID scheme (IDs stay reproducible in tests).
#[derive(Debug, Clone)]
pub struct OrchestrationIds {
    /// Identity for the Execution created on the runnable path.
    pub execution_id: agent_runtime::ExecutionId,
    /// Identity for the Attempt created on the runnable path.
    pub attempt_id: agent_runtime::AttemptId,
}

/// Durable entry state shared by both cycle flavors. The non-runnable
/// shape carries its terminal outcome (nothing was committed); the
/// runnable shape carries the work item for exactly one Execution whose
/// Attempt is still to be created (claim + start + execution committed).
#[derive(Debug)]
enum Entered {
    /// Nothing runnable: no claim, no Execution, no Attempt committed.
    NotRunnable(OrchestrationOutcome),
    /// Entry committed: `RUNNING` + one Execution, no Attempt yet.
    Runnable(EnteredCycle),
}

/// Daemon-visible entry outcome (TASK-0047, G5): the same boundary as
/// [`Entered`], but public so the `agentd` composition root can commit
/// the fast durable entry synchronously on `run` and continue the slow
/// provider half in the background. Both halves stay inside this trusted
/// service — the daemon sequences them, it never reimplements them.
#[derive(Debug)]
pub enum ExecutionEntry {
    /// Nothing runnable: no claim, no Execution, no Attempt committed.
    NotRunnable(OrchestrationOutcome),
    /// Entry committed: `RUNNING` + one Execution, no Attempt yet.
    /// Continue exactly once with [`OrchestrationService::continue_workspace_cycle`].
    Runnable(WorkItem),
}

/// Runnable entry state: the single work item for the committed Execution.
#[derive(Debug)]
struct EnteredCycle {
    /// Durable identities for the one attempt this cycle may create.
    work: WorkItem,
}

/// Read-only recovery plan for one admitted-but-not-attempted execution
/// (TASK-0054, G5 B-RS).
///
/// A production crash (or deterministic background failure) after the
/// atomic admission `COMMIT` but before `create_attempt` leaves Task
/// `RUNNING` + one execution + one Attempt reservation + the journal
/// triple, with zero attempts. This plan detects exactly that shape and
/// carries the identities the resume half needs to continue the SAME
/// execution with its reserved attempt.
///
/// Read-only: emits no journal evidence, performs no Task transition, and
/// creates no lease, attempt, or provider work. Resuming performs no Task
/// lifecycle transition either, so the canonical state machine is
/// untouched — recovery never invents a `RUNNING → READY` edge.
///
/// Fail-closed contract: anything that is not exactly one healthy
/// pre-Attempt entry refuses — a missing execution (`NotFound`), an
/// execution that already records attempts or whose task left `RUNNING`
/// (`InvalidTransition`), a missing or ambiguous reservation and any
/// ownership disagreement (`Corrupt`). Corruption is never healed by
/// guessing ownership.
#[derive(Debug, Clone)]
pub struct PreAttemptResumePlan {
    /// The work item the resume half must continue: the admitted
    /// execution with its reserved attempt identity.
    pub work: WorkItem,
    /// Recorded repository identity when a lease already exists; `None`
    /// when no lease exists and the operator must supply it.
    pub repository_ref: Option<String>,
    /// Recorded base commit OID when a lease already exists; `None` when
    /// no lease exists and the operator must supply the revision.
    pub base_revision: Option<String>,
    /// Lease lifecycle when a lease already exists (`Allocated` or
    /// `Ready` only — anything else refuses above).
    pub lease_lifecycle: Option<workspace::LeaseLifecycle>,
}

/// Trusted owner of the G3 orchestration cycle. Composes the two existing
/// services over one database; both `open` handles point at the same path
/// (SQLite serializes the short per-step transactions).
pub struct OrchestrationService<C: Clock = SystemClock> {
    tasks: TaskService<C>,
    runtime: RuntimeService<C>,
}

impl<C: Clock> OrchestrationService<C> {
    /// Wraps two already-open services over the same database (fixtures,
    /// multi-connection tests).
    pub fn compose(tasks: TaskService<C>, runtime: RuntimeService<C>) -> Self {
        Self { tasks, runtime }
    }

    /// Read access to the composed Task service (current-state and journal
    /// queries for callers/tests).
    pub fn tasks(&self) -> &TaskService<C> {
        &self.tasks
    }

    /// Read access to the composed runtime service.
    pub fn runtime(&self) -> &RuntimeService<C> {
        &self.runtime
    }

    /// Mutable access to the composed Task service.
    pub fn tasks_mut(&mut self) -> &mut TaskService<C> {
        &mut self.tasks
    }

    /// Mutable access to the composed runtime service.
    pub fn runtime_mut(&mut self) -> &mut RuntimeService<C> {
        &mut self.runtime
    }

    /// Runs one bounded orchestration cycle for `task_id`.
    ///
    /// At most one Execution and one Attempt are created, and only on the
    /// runnable path. Blocked tasks return a typed outcome with no claim,
    /// no Execution, no Attempt, no executor invocation, and no runtime
    /// event. Provider/runtime failure records one terminal Attempt and
    /// returns — never a hidden retry.
    pub fn schedule(
        &mut self,
        task_id: &TaskId,
        ids: OrchestrationIds,
        executor: &impl ExecutionPort,
        evaluator: &impl EvaluationPort,
    ) -> ServiceResult<OrchestrationOutcome> {
        self.schedule_with_plan_head(task_id, ids, None, executor, evaluator)
    }

    /// Runs one bounded cycle authorized by an optional exact Plan head.
    /// The predicate is enforced by the SQLite admission transaction, before
    /// it mutates Task lifecycle state. This is crate-visible solely for the
    /// plan-gated orchestration facade; ordinary scheduling remains
    /// unconditioned.
    pub(crate) fn schedule_with_plan_head(
        &mut self,
        task_id: &TaskId,
        ids: OrchestrationIds,
        plan_head: Option<storage_sqlite::PlanHeadPrecondition>,
        executor: &impl ExecutionPort,
        evaluator: &impl EvaluationPort,
    ) -> ServiceResult<OrchestrationOutcome> {
        let entered = match self.enter(task_id, ids, plan_head)? {
            Entered::NotRunnable(outcome) => return Ok(outcome),
            Entered::Runnable(entered) => entered,
        };
        self.runtime.create_attempt(
            &entered.work.execution_id,
            entered.work.attempt_id.clone(),
            executor.provider_ref(),
        )?;

        // -- executor runs outside every SQLite transaction ------------------
        let behavior = executor.execute(&entered.work);

        self.finish(entered.work, behavior, evaluator)
    }

    /// Runs one bounded orchestration cycle with the attempt bound to an
    /// isolated workspace lease (TASK-0046, G5).
    ///
    /// Provider-neutral: the executor is built by `make_executor` from the
    /// durable [`Task`] and its ready [`WorkspaceLease`], so any current
    /// or future provider (Claude, Codex, OpenCode, …) binds the same way
    /// without changing orchestration. The lease is per-Execution —
    /// attempts of one execution share it, never own it.
    ///
    /// Ordering preserves every durability boundary of [`Self::schedule`]:
    /// readiness → claim → start → create execution → ensure lease →
    /// build executor → create attempt → execute (outside transactions)
    /// → conclude → submit → verify. A lease failure reports as an error
    /// with the pre-lease facts (`RUNNING` + execution, no attempt)
    /// committed — a legitimate recoverable intermediate, never a hidden
    /// rollback or retry.
    pub fn schedule_in_workspace<G: GitPort, E: ExecutionPort>(
        &mut self,
        workspace: &mut WorkspaceService<G, C>,
        task_id: &TaskId,
        ids: OrchestrationIds,
        workspace_request: WorkspaceRequest,
        make_executor: impl FnOnce(&Task, &WorkspaceLease) -> E,
        evaluator: &impl EvaluationPort,
    ) -> ServiceResult<OrchestrationOutcome> {
        // Fail before any mutation: a workspace request for another
        // execution must not claim, start, or allocate anything.
        if workspace_request.execution_id != ids.execution_id {
            return Err(ServiceError::Storage(StorageError::ConstraintViolation(
                format!(
                    "workspace request targets execution {} but orchestration uses {}",
                    workspace_request.execution_id.as_str(),
                    ids.execution_id.as_str()
                ),
            )));
        }
        match self.enter_execution(task_id, ids)? {
            ExecutionEntry::NotRunnable(outcome) => Ok(outcome),
            ExecutionEntry::Runnable(work) => self.continue_workspace_cycle(
                workspace,
                work,
                workspace_request,
                make_executor,
                evaluator,
            ),
        }
    }

    /// Continues a committed entry through lease-bound attempt execution:
    /// create-attempt → execute (outside transactions) → conclude →
    /// submit → verify. The executor arrives already built (the daemon
    /// binds it to the attempt's lease in its own composition); the
    /// non-workspace [`Self::schedule`] path builds it inline instead.
    /// Shared by [`Self::schedule_in_workspace`] and by the daemon
    /// background half so all three spell the concluding boundary once.
    pub fn run_workspace_attempt<E: ExecutionPort>(
        &mut self,
        work: WorkItem,
        executor: E,
        evaluator: &impl EvaluationPort,
    ) -> ServiceResult<OrchestrationOutcome> {
        self.runtime.create_attempt(
            &work.execution_id,
            work.attempt_id.clone(),
            executor.provider_ref(),
        )?;

        // -- executor runs outside every SQLite transaction ------------------
        let behavior = executor.execute(&work);

        self.finish(work, behavior, evaluator)
    }

    /// Commits the durable entry half of a cycle without running any
    /// provider: readiness, claim, start, execution. Daemon-visible so
    /// `run` can return durable identities immediately; the provider half
    /// must follow exactly once via [`Self::continue_workspace_cycle`]
    /// (lease → attempt → execute → conclude → submit → verify).
    pub fn enter_execution(
        &mut self,
        task_id: &TaskId,
        ids: OrchestrationIds,
    ) -> ServiceResult<ExecutionEntry> {
        self.enter_execution_with_plan_head(task_id, ids, None)
    }

    pub(crate) fn enter_execution_with_plan_head(
        &mut self,
        task_id: &TaskId,
        ids: OrchestrationIds,
        plan_head: Option<storage_sqlite::PlanHeadPrecondition>,
    ) -> ServiceResult<ExecutionEntry> {
        match self.enter(task_id, ids, plan_head)? {
            Entered::NotRunnable(outcome) => Ok(ExecutionEntry::NotRunnable(outcome)),
            Entered::Runnable(entered) => Ok(ExecutionEntry::Runnable(entered.work)),
        }
    }

    /// Plans recovery for one admitted-but-not-attempted execution
    /// (TASK-0054, G5 B-RS).
    ///
    /// Read-only: validates the exact stranded shape (execution exists,
    /// zero attempts, exactly one owned reservation, reservation task
    /// agrees with execution ownership, task is `RUNNING`, lease absent
    /// or `Allocated`/`Ready`) and returns the work item plus any
    /// lease-recorded workspace inputs the resume half needs. Emits no
    /// journal evidence and performs no Task transition.
    ///
    /// Every refusal is fail-closed: `NotFound` for a missing execution,
    /// `InvalidTransition` for an execution that already records attempts
    /// (attempt recovery belongs to G11, never to this path) or whose
    /// task left `RUNNING`, `Corrupt` for a missing/ambiguous reservation,
    /// ownership disagreement, a missing task row, or a terminal lease.
    pub fn plan_pre_attempt_resume(
        &self,
        execution_id: &agent_runtime::ExecutionId,
    ) -> ServiceResult<PreAttemptResumePlan> {
        let execution = self
            .runtime
            .store()
            .get_execution(execution_id)?
            .ok_or_else(|| {
                ServiceError::Storage(StorageError::NotFound(format!(
                    "execution {execution_id} does not exist"
                )))
            })?;
        if !execution.attempts().is_empty() {
            return Err(ServiceError::Domain(DomainError::InvalidTransition(
                format!(
                    "execution {execution_id} already records {} attempt(s); \
                     pre-attempt recovery does not apply",
                    execution.attempts().len()
                ),
            )));
        }
        let reservations = self
            .tasks
            .store()
            .list_attempt_reservations_for_execution(execution_id)?;
        let (attempt_id, reservation_task) = match reservations.as_slice() {
            [(attempt_id, reservation_task)] => (attempt_id, reservation_task),
            _ => {
                return Err(ServiceError::Storage(StorageError::Corrupt(format!(
                    "execution {execution_id} holds {} attempt reservations with no attempt; \
                     expected exactly one owned reservation",
                    reservations.len()
                ))));
            }
        };
        if reservation_task != execution.task_id() {
            return Err(ServiceError::Storage(StorageError::Corrupt(format!(
                "execution {execution_id} reservation for attempt {attempt_id} names task \
                 {reservation_task} but the execution belongs to task {}; refusing",
                execution.task_id()
            ))));
        }
        let task = self
            .tasks
            .store()
            .get_task(execution.task_id())?
            .ok_or_else(|| {
                ServiceError::Storage(StorageError::Corrupt(format!(
                    "execution {execution_id} references missing task {}",
                    execution.task_id()
                )))
            })?;
        if task.state() != TaskState::Running {
            let state = task.state();
            let remedy = if state == TaskState::Blocked {
                " resume the task to RUNNING before execution recovery"
            } else {
                ""
            };
            return Err(ServiceError::Domain(DomainError::InvalidTransition(
                format!(
                    "task {} is {state}; pre-attempt recovery requires RUNNING.{remedy}",
                    task.id()
                ),
            )));
        }
        let lease = self
            .tasks
            .store()
            .get_workspace_for_execution(execution_id)
            .map_err(|error| {
                ServiceError::Storage(StorageError::Corrupt(format!(
                    "execution {execution_id} lease read failed: {error}"
                )))
            })?;
        let (repository_ref, base_revision, lease_lifecycle) = match &lease {
            None => (None, None, None),
            Some(lease)
                if lease.lifecycle() == workspace::LeaseLifecycle::Allocated
                    || lease.lifecycle() == workspace::LeaseLifecycle::Ready =>
            {
                (
                    Some(lease.repository().as_str().to_owned()),
                    Some(lease.base_revision().as_str().to_owned()),
                    Some(lease.lifecycle()),
                )
            }
            Some(lease) => {
                return Err(ServiceError::Storage(StorageError::Corrupt(format!(
                    "execution {execution_id} lease {} is {}; pre-attempt recovery \
                     cannot proceed",
                    lease.id().as_str(),
                    lease.lifecycle(),
                ))));
            }
        };
        Ok(PreAttemptResumePlan {
            work: WorkItem {
                task_id: execution.task_id().clone(),
                execution_id: execution_id.clone(),
                attempt_id: attempt_id.clone(),
            },
            repository_ref,
            base_revision,
            lease_lifecycle,
        })
    }

    /// Continues a committed [`ExecutionEntry::Runnable`] work item
    /// through the workspace-bound provider half: ensure lease → build
    /// executor → create attempt → execute (outside transactions) →
    /// conclude → submit → verify. Call exactly once per committed entry;
    /// a second call for the same execution fails on the durable
    /// attempt discipline instead of duplicating provider work silently.
    pub fn continue_workspace_cycle<G: GitPort, E: ExecutionPort>(
        &mut self,
        workspace: &mut WorkspaceService<G, C>,
        work: WorkItem,
        workspace_request: WorkspaceRequest,
        make_executor: impl FnOnce(&Task, &WorkspaceLease) -> E,
        evaluator: &impl EvaluationPort,
    ) -> ServiceResult<OrchestrationOutcome> {
        if workspace_request.execution_id != work.execution_id {
            return Err(ServiceError::Storage(StorageError::ConstraintViolation(
                format!(
                    "workspace request targets execution {} but orchestration uses {}",
                    workspace_request.execution_id.as_str(),
                    work.execution_id.as_str()
                ),
            )));
        }
        let task = self.tasks.get_task(&work.task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {} vanished after entry",
                work.task_id
            )))
        })?;
        let ensured = workspace.ensure_workspace_ready(&workspace_request)?;
        // TASK-0135 (G8): durable prerequisites are checked at
        // workspace-attempt time, after admission (the journaled `Block`
        // transition requires RUNNING) and before any provider invocation.
        // Preflight informs, the orchestrator decides: positively `Failed`
        // findings map onto the journaled `Block` with the bounded blocker
        // report as the durable reason; `Indeterminate` findings are
        // recorded nowhere and never block (backend outages must not wedge
        // a task), and an empty declaration skips the check entirely.
        // Recovery is the existing admitted-but-not-attempted resume path
        // (zero attempts recorded).
        if !task.prerequisites().is_empty() {
            let report = workspace.check_task_prerequisites(
                &ensured.lease,
                &workspace_request.repository_path,
                task.prerequisites(),
            )?;
            let failures = report.blocking_failures();
            if !failures.is_empty() {
                let reason = bounded_blocker_reason(&failures);
                self.tasks.block(&work.task_id, reason.clone())?;
                return Ok(OrchestrationOutcome::BlockedByPrerequisite {
                    task_id: work.task_id.clone(),
                    execution_id: work.execution_id.clone(),
                    reason,
                });
            }
        }
        let executor = make_executor(&task, &ensured.lease);
        self.run_workspace_attempt(work, executor, evaluator)
    }

    /// Durable entry half of a cycle: readiness, then ONE atomic
    /// admission (claim + start + execution + attempt reservation +
    /// managed-identity reservation + journal). Shared by [`Self::schedule`] and
    /// [`Self::schedule_in_workspace`] so both spell the entry boundary
    /// exactly once.
    ///
    /// Concurrency contract (TASK-0051, G5): the pre-entry reads below
    /// are diagnostics only. Correctness comes from the single
    /// `IMMEDIATE` admission transaction: a concurrent duplicate
    /// execution/attempt identity, or a concurrent claim of the same
    /// task, rolls the loser back with zero unrelated mutations — the
    /// loser keeps its pre-run state and a typed conflict.
    fn enter(
        &mut self,
        task_id: &TaskId,
        ids: OrchestrationIds,
        plan_head: Option<storage_sqlite::PlanHeadPrecondition>,
    ) -> ServiceResult<Entered> {
        // -- derived readiness (read-only) ---------------------------------
        let readiness = derive_task_readiness(self.tasks.store(), task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )))
        })?;
        match readiness {
            ScheduleReadiness::NotRunnable { state } => {
                return Ok(Entered::NotRunnable(OrchestrationOutcome::NotRunnable {
                    task_id: task_id.clone(),
                    state,
                }));
            }
            ScheduleReadiness::WaitingOnDependency { blocking } => {
                return Ok(Entered::NotRunnable(
                    OrchestrationOutcome::WaitingOnDependency {
                        task_id: task_id.clone(),
                        blocking,
                    },
                ));
            }
            ScheduleReadiness::Runnable => {}
        }

        // -- identity preflight (TASK-0049, G5): synchronously detectable
        // identity conflicts refuse before the first mutation with a typed
        // error naming the owner. This is a diagnostic fast path only —
        // it cannot close the concurrent race (read-before-write). The
        // atomic admission below is the correctness boundary: under
        // concurrency the loser fails there with zero mutations.
        // There is no idempotent-replay path: a duplicate identity is
        // always a refusal, whether it collides with another Task's
        // execution or reuses an attempt, because the entry boundary
        // never re-enters an existing Execution/Attempt.
        if let Some(existing) = self.runtime.get_execution(&ids.execution_id)? {
            return Err(ServiceError::Storage(StorageError::Duplicate(format!(
                "execution {} already exists for task {}; supply a fresh execution identity",
                ids.execution_id.as_str(),
                existing.task_id()
            ))));
        }
        if let Some(existing) = self.runtime.get_attempt(&ids.attempt_id)? {
            return Err(ServiceError::Storage(StorageError::Duplicate(format!(
                "attempt {} already exists for execution {}; supply a fresh attempt identity",
                ids.attempt_id.as_str(),
                existing.execution_id()
            ))));
        }
        if let Some((owner, _)) = self
            .tasks
            .store()
            .get_attempt_reservation(&ids.attempt_id)?
        {
            return Err(ServiceError::Storage(StorageError::Duplicate(format!(
                "attempt {} is already reserved by execution {}; supply a fresh attempt identity",
                ids.attempt_id.as_str(),
                owner.as_str()
            ))));
        }

        // -- managed workspace identity preflight (TASK-0053 v2, G5 B-CF):
        // the derived branch for this entry must not already be recorded
        // for another execution. Readable derivation is reversible over
        // exact bytes and canonical-lowercase (fold-stable), and the
        // digest fallback is collision-resistant with a fail-closed
        // durable backstop — so a hit means either foreign interference
        // or a digest collision, never a second valid entry. Refuse
        // before the first mutation either way. (Git state itself can
        // still change afterwards; that stays a recoverable saga
        // concern, not an entry concern.) This preflight is diagnostic
        // only: the atomic admission below re-checks inside its
        // transaction, which is the correctness boundary under
        // concurrency.
        let branch = crate::managed_identity::branch_for_execution(&ids.execution_id)?;
        if let Some(owner) = self
            .tasks
            .store()
            .find_lease_owner_by_branch(branch.as_str())
            .map_err(ServiceError::from)?
        {
            if owner != ids.execution_id {
                return Err(ServiceError::Storage(StorageError::Duplicate(format!(
                    "managed branch {} is already owned by execution {}; \
                     supply a fresh execution identity",
                    branch.as_str(),
                    owner.as_str()
                ))));
            }
        }
        if let Some(owner) = self
            .tasks
            .store()
            .find_identity_reservation_owner(branch.as_str())
            .map_err(ServiceError::from)?
        {
            if owner != ids.execution_id {
                return Err(ServiceError::Storage(StorageError::Duplicate(format!(
                    "managed branch {} is already reserved by execution {}; \
                     supply a fresh execution identity",
                    branch.as_str(),
                    owner.as_str()
                ))));
            }
        }

        // -- validated plans (domain rules applied, nothing committed) ----
        // Task lifecycle rules come from `task-domain` via the service
        // planners; the execution intent is begun by `agent-runtime`.
        // Each plan carries its own journal evidence, minted here.
        let claimed = self.tasks.plan_claim(task_id)?;
        let started = self.tasks.plan_start(&claimed.task)?;
        let planned = self
            .runtime
            .plan_execution(ids.execution_id.clone(), task_id.clone())?;

        // -- atomic admission: READY → CLAIMED → RUNNING + execution +
        // attempt reservation + managed-identity reservation + journal,
        // one IMMEDIATE transaction -------------------------------------
        // A failure here (duplicate identity, concurrent claim moved
        // first, managed-branch collision) reports without creating any
        // runtime fact and without retrying. Exactly one winner exists per
        // identity; every loser rolls back completely. The branch was
        // derived purely above (no Git/filesystem/provider work inside the
        // transaction — INV-145); the transaction owns the reservation.
        let admission = storage_sqlite::ExecutionEntryAdmission {
            plan_head,
            task_id,
            claimed_task: &claimed.task,
            claimed_event: &claimed.event,
            started_task: &started.task,
            started_event: &started.event,
            execution: &planned.execution,
            execution_event: &planned.event,
            attempt_id: &ids.attempt_id,
            reserved_at: planned.reserved_at.clone(),
            managed_branch: branch.as_str(),
        };
        self.tasks.store_mut().admit_execution_entry(&admission)?;

        // -- durable execution fact ------------------------------------------
        let execution_id = ids.execution_id;
        let attempt_id = ids.attempt_id;

        Ok(Entered::Runnable(EnteredCycle {
            work: WorkItem {
                task_id: task_id.clone(),
                execution_id,
                attempt_id,
            },
        }))
    }

    /// Concluding half of a cycle: interpret, conclude, submit, verify.
    /// Shared by [`Self::schedule`] and [`Self::schedule_in_workspace`].
    fn finish(
        &mut self,
        work: WorkItem,
        behavior: Result<scheduler::WorkEvidence, scheduler::ExecutionProblem>,
        evaluator: &impl EvaluationPort,
    ) -> ServiceResult<OrchestrationOutcome> {
        // -- conclude the Attempt exactly once -------------------------------
        let WorkItem {
            task_id,
            execution_id,
            attempt_id,
        } = &work;
        match interpret(behavior, &work) {
            AttemptInterpretation::NotSubmittable { outcome, detail } => {
                self.runtime
                    .conclude_attempt(execution_id, attempt_id, outcome, detail)?;
                if outcome == agent_runtime::AttemptOutcome::Cancelled {
                    Ok(OrchestrationOutcome::Cancelled {
                        task_id: task_id.clone(),
                        execution_id: execution_id.clone(),
                        attempt_id: attempt_id.clone(),
                    })
                } else {
                    Ok(OrchestrationOutcome::ExecutionFailed {
                        task_id: task_id.clone(),
                        execution_id: execution_id.clone(),
                        attempt_id: attempt_id.clone(),
                        outcome,
                    })
                }
            }
            AttemptInterpretation::Submittable {
                detail,
                summary,
                evidence_ref,
            } => {
                self.runtime.conclude_attempt(
                    execution_id,
                    attempt_id,
                    agent_runtime::AttemptOutcome::Succeeded,
                    detail,
                )?;
                // Attempt Succeeded is evidence, never completion: the Task
                // is still RUNNING here by construction.
                self.tasks.submit_result(task_id, summary.clone())?;
                self.tasks.route_to_review(task_id)?;
                self.tasks.begin_verification(task_id)?;

                // -- independent verdict (the executor never judges) ---------
                let evaluation_request = EvaluationRequest {
                    task_id: task_id.clone(),
                    execution_id: execution_id.clone(),
                    attempt_id: attempt_id.clone(),
                    summary,
                    evidence_ref,
                };
                let verdict = evaluator.evaluate(&evaluation_request).map_err(|error| {
                    // No verdict reached: the Task stays VERIFYING
                    // (durable) with its evidence intact. Reported, never
                    // retried inside this cycle.
                    ServiceError::Storage(StorageError::ConstraintViolation(format!(
                        "independent evaluation produced no verdict: {error}"
                    )))
                })?;
                match verdict {
                    EvaluationVerdict::Verified => {
                        evaluator
                            .revalidate_before_completion(&evaluation_request)
                            .map_err(|error| {
                                ServiceError::Storage(StorageError::ConstraintViolation(format!(
                                    "completion freshness revalidation failed: {error}"
                                )))
                            })?;
                        self.tasks.verification_succeeded(
                            task_id,
                            "VERIFIED via independent evaluation",
                        )?;
                        Ok(OrchestrationOutcome::ExecutionSucceededAndVerified {
                            task_id: task_id.clone(),
                            execution_id: execution_id.clone(),
                            attempt_id: attempt_id.clone(),
                        })
                    }
                    EvaluationVerdict::Rejected => {
                        self.tasks
                            .verification_failed(task_id, "REJECTED via independent evaluation")?;
                        Ok(OrchestrationOutcome::ExecutionSucceededButRejected {
                            task_id: task_id.clone(),
                            execution_id: execution_id.clone(),
                            attempt_id: attempt_id.clone(),
                        })
                    }
                }
            }
        }
    }
}

/// Canonical derived scheduling readiness of one Task (read-only): the Task
/// state plus its required Task dependencies that are not COMPLETED. `None`
/// when the Task does not exist. A dependency row that cannot be resolved
/// fails closed: an unresolvable graph must never execute. Shared by the
/// scheduler entry and plan applicability checks so both judge readiness
/// with exactly one rule.
pub(crate) fn derive_task_readiness(
    store: &SqliteStore,
    task_id: &TaskId,
) -> Result<Option<ScheduleReadiness>, StorageError> {
    let Some(task) = store.get_task(task_id)? else {
        return Ok(None);
    };
    let state = task.state();
    let mut blocking = Vec::new();
    if state == TaskState::Ready {
        for dependency_id in store.list_dependencies(task_id)? {
            let dependency = store.get_task(&dependency_id)?.ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "task {task_id} depends on missing task {dependency_id}"
                ))
            })?;
            if dependency.state() != TaskState::Completed {
                blocking.push(dependency_id);
            }
        }
    }
    Ok(Some(derive_readiness(state, blocking)))
}

/// Upper bound in bytes on the composed blocker report journaled as the
/// `Block` reason: keeps durable task content bounded even under the
/// maximum prerequisite count (each contributing value is itself capped
/// by the `Prerequisite` vocabulary).
const MAX_BLOCKER_REASON_LEN: usize = 2048;

/// Composes the bounded, human-readable blocker report from positively
/// failed preflight findings (TASK-0135). Declaration order is preserved;
/// truncation is explicit. Indeterminate findings are not blockers and
/// never appear here.
fn bounded_blocker_reason(failures: &[&crate::preflight_service::PreflightFinding]) -> String {
    let mut reason = String::from("prerequisite preflight failed:");
    for finding in failures {
        let crate::preflight_service::PrerequisiteStatus::Failed { reason: detail } =
            &finding.status
        else {
            continue;
        };
        let part = format!(" {} {:?} ({detail});", finding.kind.as_str(), finding.value);
        if reason.len() + part.len() > MAX_BLOCKER_REASON_LEN {
            reason.push_str(" ... (truncated)");
            break;
        }
        reason.push_str(&part);
    }
    reason
}

impl<C: Clock + Clone> OrchestrationService<C> {
    /// Opens both composed services on the database at `path`
    /// (creating/migrating it).
    pub fn open(path: impl AsRef<Path>, clock: C) -> ServiceResult<Self> {
        Ok(Self::compose(
            TaskService::open(&path, clock.clone())?,
            RuntimeService::open(&path, clock)?,
        ))
    }
}
