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
//! Durability boundaries: each step below is its own atomic commit. The
//! executor runs outside every SQLite transaction (service calls return
//! before it is invoked), so a crash leaves a legitimate recoverable
//! intermediate state (`RUNNING` + `E1` + open `A1`) instead of a hidden
//! in-memory mega-transaction.
//!
//! ```text
//! readiness (derived, read-only)
//! → TaskService::claim → TaskService::start          (durable)
//! → RuntimeService::create_execution                 (durable)
//! → RuntimeService::create_attempt                   (durable)
//! → [executor runs outside any transaction]
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
use storage_sqlite::StorageError;
use task_domain::{Task, TaskId, TaskState};
use workspace::{GitPort, WorkspaceLease};

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
        let entered = match self.enter(task_id, ids)? {
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
        match self.enter(task_id, ids)? {
            Entered::NotRunnable(outcome) => Ok(ExecutionEntry::NotRunnable(outcome)),
            Entered::Runnable(entered) => Ok(ExecutionEntry::Runnable(entered.work)),
        }
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
        let executor = make_executor(&task, &ensured.lease);
        self.run_workspace_attempt(work, executor, evaluator)
    }

    /// Durable entry half of a cycle: readiness, claim, start, execution.
    /// Shared by [`Self::schedule`] and [`Self::schedule_in_workspace`]
    /// so both spell the entry boundary exactly once.
    fn enter(&mut self, task_id: &TaskId, ids: OrchestrationIds) -> ServiceResult<Entered> {
        // -- derived readiness (read-only) ---------------------------------
        let task = self.tasks.get_task(task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )))
        })?;
        let state = task.state();
        let blocking = if state == TaskState::Ready {
            self.blocking_dependencies(task_id)?
        } else {
            Vec::new()
        };
        match derive_readiness(state, blocking) {
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

        // -- durable entry path: READY → CLAIMED → RUNNING ------------------
        // Each call is its own atomic state+journal commit. A failure here
        // (e.g. a concurrent claim moved first) reports without creating
        // any runtime fact and without retrying.
        self.tasks.claim(task_id)?;
        self.tasks.start(task_id)?;

        // -- durable execution fact ------------------------------------------
        let execution_id = ids.execution_id;
        let attempt_id = ids.attempt_id;
        self.runtime
            .create_execution(execution_id.clone(), task_id.clone())?;

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
                let verdict = evaluator
                    .evaluate(&EvaluationRequest {
                        task_id: task_id.clone(),
                        execution_id: execution_id.clone(),
                        attempt_id: attempt_id.clone(),
                        summary,
                        evidence_ref,
                    })
                    .map_err(|error| {
                        // No verdict reached: the Task stays VERIFYING
                        // (durable) with its evidence intact. Reported, never
                        // retried inside this cycle.
                        ServiceError::Storage(StorageError::ConstraintViolation(format!(
                            "independent evaluation produced no verdict: {error}"
                        )))
                    })?;
                match verdict {
                    EvaluationVerdict::Verified => {
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

    /// Lists required dependencies that are not COMPLETED. A dependency
    /// row that cannot be resolved fails closed: an unresolvable graph
    /// must never execute.
    fn blocking_dependencies(&self, task_id: &TaskId) -> ServiceResult<Vec<TaskId>> {
        let mut blocking = Vec::new();
        for dependency_id in self.tasks.list_dependencies(task_id)? {
            let dependency = self.tasks.get_task(&dependency_id)?.ok_or_else(|| {
                ServiceError::Storage(StorageError::Corrupt(format!(
                    "task {task_id} depends on missing task {dependency_id}"
                )))
            })?;
            if dependency.state() != TaskState::Completed {
                blocking.push(dependency_id);
            }
        }
        Ok(blocking)
    }
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
