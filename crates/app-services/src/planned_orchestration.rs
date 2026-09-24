//! Plan-gated scheduling (TASK-0078).
//!
//! Two distinct product operations share one gate:
//!
//! - [`OrchestrationService::schedule_with_planning`] plans: it may call
//!   `PlannerPort`, validates V1-V9 plus canonical Task applicability/L4,
//!   and commits a new accepted revision before scheduling.
//! - [`OrchestrationService::schedule_from_current_plan`] only executes an
//!   already accepted plan: it loads the canonical head, never calls the
//!   planner and never creates a revision merely to advance to the next node.
//!
//! Both gate on Plan predecessors being COMPLETED Tasks, confirm the head the
//! decision was based on is still canonical, and then delegate to the
//! existing `schedule` path, whose Task readiness/lifecycle stays
//! authoritative.

use std::fmt;

use plan_domain::{EvidenceRef, PlanBranchId, PlannerPort, ReplanRequest};
use scheduler::{ExecutionPort, OrchestrationOutcome};
use task_domain::{TaskId, TaskState};

use crate::clock::Clock;
use crate::error::ServiceError;
use crate::orchestration_service::{OrchestrationIds, OrchestrationService};
use crate::planning_service::{
    AcceptedPlanRevision, AcceptedTaskPlanResolution, PlanningPolicyContext,
    PlanningProposalOutcome, PlanningService, PlanningServiceError,
};
use crate::{EvaluationPort, ExecutionEntry};
use plan_domain::{
    DecompositionRequest, HumanContinuation, HumanDecisionRequest, HumanDecisionStatus,
    PlanningBudget, MAX_DECOMPOSITION_DEPTH, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};

/// Result of one bounded plan-gated scheduling cycle.
#[derive(Debug)]
pub enum PlannedOrchestrationOutcome {
    /// No revision was accepted (refused, not applicable, guardrail, or
    /// blocked). Nothing was scheduled and the Plan head is unchanged.
    PlanningStopped(PlanningProposalOutcome),
    /// The branch has no accepted revision to schedule from.
    NoAcceptedPlan,
    /// The selected Task is not represented by the accepted revision.
    TaskNotInAcceptedPlan { plan: Box<AcceptedPlanRevision> },
    /// The selected plan node exists but one or more Plan predecessors have
    /// not reached canonical Task COMPLETED state yet. No Task claim,
    /// Execution, Attempt, or provider invocation has happened.
    WaitingOnPlanDependency {
        plan: Box<AcceptedPlanRevision>,
        blocking: Vec<TaskId>,
    },
    /// Only this Task/Plan scope is waiting for durable human input.
    WaitingForHuman {
        plan: Box<AcceptedPlanRevision>,
        decision: Box<HumanDecisionRequest>,
    },
    /// A consumed bounded ABORT stops this owner scope without changing
    /// unrelated Tasks/Executions.
    AbortedByHuman {
        plan: Box<AcceptedPlanRevision>,
        decision: Box<HumanDecisionRequest>,
    },
    /// The canonical head moved away from `plan` before execution started.
    /// Nothing was scheduled; the caller must reconcile against the new head.
    PlanHeadMoved {
        plan: Box<AcceptedPlanRevision>,
        found: Option<u64>,
    },
    /// The Plan head stayed put but its product-owned planning input was
    /// superseded. Execution must reconcile before any Task mutation.
    PlanningInputMoved { plan: Box<AcceptedPlanRevision> },
    /// Accepted revision gated execution. A failed/rejected execution may
    /// have produced exactly one bounded replan attempt from fresh evidence.
    Executed {
        plan: Box<AcceptedPlanRevision>,
        execution: OrchestrationOutcome,
        replan: Option<Result<PlanningProposalOutcome, PlanningServiceError>>,
    },
    /// The revision is durably accepted; the scheduling/execution step that
    /// followed failed. The accepted plan stays canonical.
    SchedulingFailed {
        plan: Box<AcceptedPlanRevision>,
        error: PlannedOrchestrationError,
    },
}

/// Production `RunExecution` admission result. This operation never owns a
/// PlannerPort and therefore cannot create a Plan or spend LLM tokens.
#[derive(Debug)]
pub enum PlannedExecutionEntryOutcome {
    NoAcceptedPlan,
    AmbiguousAcceptedPlan {
        branches: Vec<PlanBranchId>,
    },
    WaitingOnPlanDependency {
        plan: Box<AcceptedPlanRevision>,
        blocking: Vec<TaskId>,
    },
    WaitingForHuman {
        plan: Box<AcceptedPlanRevision>,
        decision: Box<HumanDecisionRequest>,
    },
    AbortedByHuman {
        plan: Box<AcceptedPlanRevision>,
        decision: Box<HumanDecisionRequest>,
    },
    PlanHeadMoved {
        plan: Box<AcceptedPlanRevision>,
        found: Option<u64>,
    },
    PlanningInputMoved {
        plan: Box<AcceptedPlanRevision>,
    },
    Entered {
        plan: Box<AcceptedPlanRevision>,
        entry: ExecutionEntry,
    },
}

/// Plan-gated scheduling failure. Never returned after this call committed a
/// Plan revision (that case is [`PlannedOrchestrationOutcome::SchedulingFailed`]).
#[derive(Debug)]
pub enum PlannedOrchestrationError {
    Planning(PlanningServiceError),
    Orchestration(ServiceError),
}

impl fmt::Display for PlannedOrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Planning(error) => write!(f, "planning cycle failed: {error}"),
            Self::Orchestration(error) => write!(f, "execution cycle failed: {error}"),
        }
    }
}

impl std::error::Error for PlannedOrchestrationError {}

/// One scheduling step against an accepted revision.
enum PlanStep {
    NotRepresented,
    Waiting(Vec<TaskId>),
    WaitingForHuman(Box<HumanDecisionRequest>),
    AbortedByHuman(Box<HumanDecisionRequest>),
    HeadMoved(Option<u64>),
    InputMoved,
    Executed(OrchestrationOutcome),
}

impl<C: Clock> OrchestrationService<C> {
    /// Product-owned bounded `propose -> validate -> applicability/L4 ->
    /// persist -> execute -> replan` path. Execution is delegated to the
    /// existing canonical scheduler service and therefore retains
    /// Task/Policy/Evaluation authority boundaries.
    #[allow(clippy::too_many_arguments)]
    pub fn schedule_with_planning(
        &mut self,
        planning: &mut PlanningService<C>,
        planner: &impl PlannerPort,
        branch: &PlanBranchId,
        decomposition: DecompositionRequest,
        policy: PlanningPolicyContext,
        task_id: &TaskId,
        ids: OrchestrationIds,
        executor: &impl ExecutionPort,
        evaluator: &impl EvaluationPort,
    ) -> Result<PlannedOrchestrationOutcome, PlannedOrchestrationError> {
        let planned = planning
            .decompose_for_task(planner, branch, decomposition, &policy, task_id)
            .map_err(PlannedOrchestrationError::Planning)?;
        let plan = match planned {
            PlanningProposalOutcome::Accepted(accepted) => accepted,
            stopped => return Ok(PlannedOrchestrationOutcome::PlanningStopped(stopped)),
        };

        // The revision is durable from here on: every failure is reported as
        // an outcome that carries the accepted plan, never as `Err`.
        let step = match self.step_accepted_plan(planning, &plan, task_id, ids, executor, evaluator)
        {
            Ok(step) => step,
            Err(error) => {
                return Ok(PlannedOrchestrationOutcome::SchedulingFailed { plan, error });
            }
        };
        let execution = match step {
            PlanStep::NotRepresented => {
                return Ok(PlannedOrchestrationOutcome::TaskNotInAcceptedPlan { plan })
            }
            PlanStep::Waiting(blocking) => {
                return Ok(PlannedOrchestrationOutcome::WaitingOnPlanDependency { plan, blocking })
            }
            PlanStep::WaitingForHuman(decision) => {
                return Ok(PlannedOrchestrationOutcome::WaitingForHuman { plan, decision })
            }
            PlanStep::AbortedByHuman(decision) => {
                return Ok(PlannedOrchestrationOutcome::AbortedByHuman { plan, decision })
            }
            PlanStep::HeadMoved(found) => {
                return Ok(PlannedOrchestrationOutcome::PlanHeadMoved { plan, found })
            }
            PlanStep::InputMoved => {
                return Ok(PlannedOrchestrationOutcome::PlanningInputMoved { plan })
            }
            PlanStep::Executed(execution) => execution,
        };

        let replan = replan_evidence(&execution).map(|evidence| {
            let mut replan_policy = policy.clone();
            // Replanning after execution evidence changes the affected branch
            // without claiming to replace parent acceptance criteria. Existing
            // criteria stay on the durable base nodes; V3 still derives the
            // criteria of any node the replan removes.
            replan_policy.affected_parent_criteria.clear();
            planning.propose_replan(
                planner,
                branch,
                ReplanRequest {
                    branch_root_ref: policy.parent_task_ref.clone(),
                    base_plan_revision: plan.revision().revision_seq,
                    triggering_evidence_refs: vec![evidence],
                    scope_bound: policy.scope_bound.clone(),
                    baseline_risk: policy.baseline_risk,
                    depth_remaining: MAX_DECOMPOSITION_DEPTH,
                    budget_remaining: PlanningBudget {
                        calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
                        input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
                    },
                },
                &replan_policy,
            )
        });
        Ok(PlannedOrchestrationOutcome::Executed {
            plan,
            execution,
            replan,
        })
    }

    /// Advances an already accepted plan: loads the canonical head of
    /// `branch`, gates `task_id` on its Plan predecessors and delegates to the
    /// canonical scheduler. Never calls a planner and never creates a Plan
    /// revision. Nothing durable is written by the plan gate itself.
    pub fn schedule_from_current_plan(
        &mut self,
        planning: &PlanningService<C>,
        branch: &PlanBranchId,
        task_id: &TaskId,
        ids: OrchestrationIds,
        executor: &impl ExecutionPort,
        evaluator: &impl EvaluationPort,
    ) -> Result<PlannedOrchestrationOutcome, PlannedOrchestrationError> {
        let Some(accepted) = planning
            .current_accepted_plan(branch)
            .map_err(PlannedOrchestrationError::Planning)?
        else {
            return Ok(PlannedOrchestrationOutcome::NoAcceptedPlan);
        };
        let plan = Box::new(accepted);
        Ok(
            match self.step_accepted_plan(planning, &plan, task_id, ids, executor, evaluator)? {
                PlanStep::NotRepresented => {
                    PlannedOrchestrationOutcome::TaskNotInAcceptedPlan { plan }
                }
                PlanStep::Waiting(blocking) => {
                    PlannedOrchestrationOutcome::WaitingOnPlanDependency { plan, blocking }
                }
                PlanStep::WaitingForHuman(decision) => {
                    PlannedOrchestrationOutcome::WaitingForHuman { plan, decision }
                }
                PlanStep::AbortedByHuman(decision) => {
                    PlannedOrchestrationOutcome::AbortedByHuman { plan, decision }
                }
                PlanStep::HeadMoved(found) => {
                    PlannedOrchestrationOutcome::PlanHeadMoved { plan, found }
                }
                PlanStep::InputMoved => PlannedOrchestrationOutcome::PlanningInputMoved { plan },
                PlanStep::Executed(execution) => PlannedOrchestrationOutcome::Executed {
                    plan,
                    execution,
                    replan: None,
                },
            },
        )
    }

    /// Production entry gate for `RunExecution` (ADR-0028 §5/§6). Branch
    /// identity is derived from current accepted Plan membership; callers do
    /// not supply it. Every refusal happens before Task/Execution mutation.
    pub fn enter_from_current_plan(
        &mut self,
        planning: &PlanningService<C>,
        task_id: &TaskId,
        ids: OrchestrationIds,
    ) -> Result<PlannedExecutionEntryOutcome, PlannedOrchestrationError> {
        // Preserve the canonical G5 not-found contract before interpreting
        // G7 planning state. A missing Task is not a stale/missing Plan: it
        // has no lifecycle authority to admit in the first place.
        if self
            .tasks()
            .get_task(task_id)
            .map_err(PlannedOrchestrationError::Orchestration)?
            .is_none()
        {
            return Err(PlannedOrchestrationError::Orchestration(
                crate::ServiceError::Storage(storage_sqlite::StorageError::NotFound(format!(
                    "task {task_id} does not exist"
                ))),
            ));
        }

        let (branch, plan) = match planning
            .current_accepted_plan_for_task(task_id)
            .map_err(PlannedOrchestrationError::Planning)?
        {
            AcceptedTaskPlanResolution::Missing => {
                return Ok(PlannedExecutionEntryOutcome::NoAcceptedPlan)
            }
            AcceptedTaskPlanResolution::Ambiguous { branches } => {
                return Ok(PlannedExecutionEntryOutcome::AmbiguousAcceptedPlan { branches })
            }
            AcceptedTaskPlanResolution::Accepted { branch, plan } => (branch, plan),
        };

        let head = durable_head(planning, &plan)?;
        if head != Some(plan.revision().revision_seq) {
            return Ok(PlannedExecutionEntryOutcome::PlanHeadMoved { plan, found: head });
        }
        if !planning
            .store()
            .current_planning_input_matches(
                &branch,
                plan.planning_input_snapshot_id(),
                plan.planning_input_fingerprint(),
            )
            .map_err(|error| PlannedOrchestrationError::Planning(error.into()))?
        {
            return Ok(PlannedExecutionEntryOutcome::PlanningInputMoved { plan });
        }

        if let Some(decision) = human_gate(planning, task_id, &branch)? {
            return Ok(match decision.status {
                HumanDecisionStatus::Consumed
                    if decision.selected == Some(HumanContinuation::Abort) =>
                {
                    PlannedExecutionEntryOutcome::AbortedByHuman {
                        plan,
                        decision: Box::new(decision),
                    }
                }
                _ => PlannedExecutionEntryOutcome::WaitingForHuman {
                    plan,
                    decision: Box::new(decision),
                },
            });
        }

        let predecessors = plan
            .predecessor_task_ids(task_id)
            .map_err(PlannedOrchestrationError::Planning)?
            .ok_or_else(|| {
                PlannedOrchestrationError::Planning(PlanningServiceError::InvalidContext(format!(
                    "task {} disappeared from resolved accepted plan {}",
                    task_id.as_str(),
                    branch.as_str()
                )))
            })?;
        let mut blocking = Vec::new();
        for predecessor in predecessors {
            let task = self
                .tasks()
                .get_task(&predecessor)
                .map_err(PlannedOrchestrationError::Orchestration)?
                .ok_or_else(|| {
                    PlannedOrchestrationError::Planning(PlanningServiceError::InvalidContext(
                        format!(
                            "plan predecessor {} has no canonical Task",
                            predecessor.as_str()
                        ),
                    ))
                })?;
            if task.state() != TaskState::Completed {
                blocking.push(predecessor);
            }
        }
        if !blocking.is_empty() {
            return Ok(PlannedExecutionEntryOutcome::WaitingOnPlanDependency { plan, blocking });
        }

        let head = durable_head(planning, &plan)?;
        if head != Some(plan.revision().revision_seq) {
            return Ok(PlannedExecutionEntryOutcome::PlanHeadMoved { plan, found: head });
        }
        let precondition = storage_sqlite::PlanHeadPrecondition {
            plan_branch_id: branch,
            revision_seq: plan.revision().revision_seq,
            planning_input: Some(storage_sqlite::PlanningInputPrecondition {
                snapshot_id: plan.planning_input_snapshot_id().clone(),
                input_fingerprint: plan.planning_input_fingerprint().to_owned(),
            }),
        };
        match self.enter_execution_with_plan_head(task_id, ids, Some(precondition)) {
            Ok(entry) => Ok(PlannedExecutionEntryOutcome::Entered { plan, entry }),
            Err(crate::ServiceError::Storage(storage_sqlite::StorageError::PlanStale {
                found,
                ..
            })) => Ok(PlannedExecutionEntryOutcome::PlanHeadMoved {
                plan,
                found: (found != 0).then_some(found),
            }),
            Err(crate::ServiceError::Storage(
                storage_sqlite::StorageError::PlanningInputStale { .. },
            )) => Ok(PlannedExecutionEntryOutcome::PlanningInputMoved { plan }),
            Err(error) => Err(PlannedOrchestrationError::Orchestration(error)),
        }
    }

    fn step_accepted_plan(
        &mut self,
        planning: &PlanningService<C>,
        plan: &AcceptedPlanRevision,
        task_id: &TaskId,
        ids: OrchestrationIds,
        executor: &impl ExecutionPort,
        evaluator: &impl EvaluationPort,
    ) -> Result<PlanStep, PlannedOrchestrationError> {
        // Every gate decision below is based on `plan`. If the canonical head
        // is no longer `plan`, applicability may differ: report it instead of
        // answering (waiting, not represented, or running) from stale state.
        let head = durable_head(planning, plan)?;
        if head != Some(plan.revision().revision_seq) {
            return Ok(PlanStep::HeadMoved(head));
        }
        if !planning
            .store()
            .current_planning_input_matches(
                &plan.revision().plan_branch_id,
                plan.planning_input_snapshot_id(),
                plan.planning_input_fingerprint(),
            )
            .map_err(|error| PlannedOrchestrationError::Planning(error.into()))?
        {
            return Ok(PlanStep::InputMoved);
        }
        if let Some(decision) = human_gate(planning, task_id, &plan.revision().plan_branch_id)? {
            return Ok(
                if decision.status == HumanDecisionStatus::Consumed
                    && decision.selected == Some(HumanContinuation::Abort)
                {
                    PlanStep::AbortedByHuman(Box::new(decision))
                } else {
                    PlanStep::WaitingForHuman(Box::new(decision))
                },
            );
        }
        let Some(predecessors) = plan
            .predecessor_task_ids(task_id)
            .map_err(PlannedOrchestrationError::Planning)?
        else {
            return Ok(PlanStep::NotRepresented);
        };

        let mut blocking = Vec::new();
        for predecessor in predecessors {
            let task = self
                .tasks()
                .get_task(&predecessor)
                .map_err(PlannedOrchestrationError::Orchestration)?
                .ok_or_else(|| {
                    PlannedOrchestrationError::Planning(PlanningServiceError::InvalidContext(
                        format!(
                            "plan predecessor {} has no canonical Task",
                            predecessor.as_str()
                        ),
                    ))
                })?;
            if task.state() != TaskState::Completed {
                blocking.push(predecessor);
            }
        }
        if !blocking.is_empty() {
            return Ok(PlanStep::Waiting(blocking));
        }

        // Re-check immediately before admission: the head may have moved
        // while predecessor state was read. The exact accepted head is also
        // carried into the scheduler's SQLite admission transaction, where it
        // is checked before any Task lifecycle mutation.
        let head = durable_head(planning, plan)?;
        if head != Some(plan.revision().revision_seq) {
            return Ok(PlanStep::HeadMoved(head));
        }

        match self.schedule_with_plan_head(
            task_id,
            ids,
            Some(storage_sqlite::PlanHeadPrecondition {
                plan_branch_id: plan.revision().plan_branch_id.clone(),
                revision_seq: plan.revision().revision_seq,
                planning_input: Some(storage_sqlite::PlanningInputPrecondition {
                    snapshot_id: plan.planning_input_snapshot_id().clone(),
                    input_fingerprint: plan.planning_input_fingerprint().to_owned(),
                }),
            }),
            executor,
            evaluator,
        ) {
            Ok(execution) => Ok(PlanStep::Executed(execution)),
            Err(crate::ServiceError::Storage(storage_sqlite::StorageError::PlanStale {
                found,
                ..
            })) => Ok(PlanStep::HeadMoved((found != 0).then_some(found))),
            Err(crate::ServiceError::Storage(
                storage_sqlite::StorageError::PlanningInputStale { .. },
            )) => Ok(PlanStep::InputMoved),
            Err(error) => Err(PlannedOrchestrationError::Orchestration(error)),
        }
    }
}

fn human_gate<C: Clock>(
    planning: &PlanningService<C>,
    task_id: &TaskId,
    branch: &PlanBranchId,
) -> Result<Option<HumanDecisionRequest>, PlannedOrchestrationError> {
    let Some(record) = planning
        .store()
        .latest_human_decision_for_scope(task_id, branch)
        .map_err(|error| PlannedOrchestrationError::Planning(error.into()))?
    else {
        return Ok(None);
    };
    let current = planning
        .store()
        .validate_human_decision_current(&record.id, planning.now_millis())
        .map_err(|error| PlannedOrchestrationError::Planning(error.into()))?;
    Ok(Some(current))
}

fn durable_head<C: Clock>(
    planning: &PlanningService<C>,
    plan: &AcceptedPlanRevision,
) -> Result<Option<u64>, PlannedOrchestrationError> {
    planning
        .store()
        .get_plan_head(&plan.revision().plan_branch_id)
        .map_err(|error| PlannedOrchestrationError::Planning(error.into()))
}

fn replan_evidence(outcome: &OrchestrationOutcome) -> Option<EvidenceRef> {
    let attempt = match outcome {
        OrchestrationOutcome::ExecutionFailed { attempt_id, .. }
        | OrchestrationOutcome::ExecutionSucceededButRejected { attempt_id, .. } => attempt_id,
        OrchestrationOutcome::NotRunnable { .. }
        | OrchestrationOutcome::WaitingOnDependency { .. }
        | OrchestrationOutcome::ExecutionSucceededAndVerified { .. }
        | OrchestrationOutcome::Cancelled { .. }
        | OrchestrationOutcome::BlockedByPrerequisite { .. } => return None,
    };
    EvidenceRef::new(format!("attempt:{}", attempt.as_str())).ok()
}
