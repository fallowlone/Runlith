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
use task_domain::{TaskId, TaskState};

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
                return Ok(OrchestrationOutcome::NotRunnable {
                    task_id: task_id.clone(),
                    state,
                });
            }
            ScheduleReadiness::WaitingOnDependency { blocking } => {
                return Ok(OrchestrationOutcome::WaitingOnDependency {
                    task_id: task_id.clone(),
                    blocking,
                });
            }
            ScheduleReadiness::Runnable => {}
        }

        // -- durable entry path: READY → CLAIMED → RUNNING ------------------
        // Each call is its own atomic state+journal commit. A failure here
        // (e.g. a concurrent claim moved first) reports without creating
        // any runtime fact and without retrying.
        self.tasks.claim(task_id)?;
        self.tasks.start(task_id)?;

        // -- durable runtime facts ------------------------------------------
        let execution_id = ids.execution_id;
        let attempt_id = ids.attempt_id;
        self.runtime
            .create_execution(execution_id.clone(), task_id.clone())?;
        self.runtime
            .create_attempt(&execution_id, attempt_id.clone(), executor.provider_ref())?;

        // -- executor runs outside every SQLite transaction ------------------
        let work = WorkItem {
            task_id: task_id.clone(),
            execution_id: execution_id.clone(),
            attempt_id: attempt_id.clone(),
        };
        let behavior = executor.execute(&work);

        // -- conclude the Attempt exactly once -------------------------------
        match interpret(behavior, &work) {
            AttemptInterpretation::NotSubmittable { outcome, detail } => {
                self.runtime
                    .conclude_attempt(&execution_id, &attempt_id, outcome, detail)?;
                if outcome == agent_runtime::AttemptOutcome::Cancelled {
                    Ok(OrchestrationOutcome::Cancelled {
                        task_id: task_id.clone(),
                        execution_id,
                        attempt_id,
                    })
                } else {
                    Ok(OrchestrationOutcome::ExecutionFailed {
                        task_id: task_id.clone(),
                        execution_id,
                        attempt_id,
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
                    &execution_id,
                    &attempt_id,
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
                            execution_id,
                            attempt_id,
                        })
                    }
                    EvaluationVerdict::Rejected => {
                        self.tasks
                            .verification_failed(task_id, "REJECTED via independent evaluation")?;
                        Ok(OrchestrationOutcome::ExecutionSucceededButRejected {
                            task_id: task_id.clone(),
                            execution_id,
                            attempt_id,
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
