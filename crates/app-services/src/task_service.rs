//! Trusted Task mutation service: validated domain transition plus
//! authoritative journal record as one atomic commit.
//!
//! Authority split (ADR-0018): the agent-facing operation is
//! [`TaskService::submit_result`], which only reaches `SUBMITTED`.
//! [`TaskService::verification_succeeded_under_policy`] is the trusted
//! Evaluation-path operation and the sole route to `COMPLETED`. There is no
//! `agent_complete_task()`, no `set_task_state()`, and no FAILED/CANCELLED
//! policy beyond what `task-domain` already decides (nothing — those edges
//! stay unresolved, so no such methods exist here either).

use std::path::Path;

use agent_runtime::{AttemptId, ExecutionId};
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

    /// Current clock reading as RFC3339, for trusted minting boundaries
    /// composed above this service (candidate verdicts stamp their
    /// assertion time from the same clock as lifecycle transitions).
    #[must_use]
    pub fn now_stamp(&self) -> String {
        format_rfc3339(self.clock.now_millis())
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
    ///
    /// TASK-0125 gate (ADR-0030 CV-1..CV-8, as implemented): the verdict
    /// string is journal context, never authority. Admission is derived from
    /// durable state only — the task's own `EvidenceRequirement`s, the
    /// current trusted subject fingerprint read from observed effects, and
    /// the durable `ClaimVerification` / exemption / approval-discharge
    /// records. Nothing a caller supplies can substitute for durable
    /// evidence (CV-2). A criterion without a current VERIFIED verdict (or
    /// an eligible CV-4 exemption / CV-8 discharge) refuses the transition
    /// with a domain error and the task stays VERIFYING.
    /// The trusted Evaluation-path operation and the sole route to
    /// `COMPLETED`, under an explicit current policy fingerprint (`None`
    /// = no policy derivation in force, which fails closed for any
    /// requirement or exemption derived under a policy). The fingerprint
    /// participates in CV-7 staleness. There is no policy-free alias: a
    /// caller that cannot derive the current policy says so with `None`
    /// and accepts the fail-closed consequence.
    pub fn verification_succeeded_under_policy(
        &mut self,
        task_id: &TaskId,
        verdict: impl Into<String>,
        policy_fp: Option<&str>,
    ) -> ServiceResult<AppliedTransition> {
        let verdict_text = verdict.into();
        let task = self.store.get_task(task_id)?.ok_or_else(|| {
            ServiceError::Storage(StorageError::NotFound(format!(
                "task {task_id} does not exist"
            )))
        })?;
        let from = task.state();
        let planned = self.plan_on(
            &task,
            TransitionCommand::VerificationSucceeded {
                verdict: verdict_text,
            },
        )?;
        let now = format_rfc3339(self.clock.now_millis());
        let (gate, snapshot) = self.admission_report(&task, policy_fp, &now)?;
        if !gate.is_admit() {
            return Err(ServiceError::Domain(DomainError::InvalidTransition(
                gate.refusal_summary(),
            )));
        }
        let consumed = self.consumed_discharges(task_id)?;
        let (event, _admission) = self.store.admit_completion(
            task_id,
            from,
            &planned.task,
            &planned.event,
            policy_fp,
            &gate.outcomes,
            &consumed,
            &snapshot,
            &now,
        )?;
        Ok(AppliedTransition {
            task: planned.task,
            event,
        })
    }
    /// Derives the pure admission report from durable state only
    /// (CV-1..CV-8): the task's own requirements, the latest trusted
    /// observed-effect fingerprint as the current subject reading, and the
    /// durable claim/verdict/exemption/discharge rows. No caller-supplied
    /// value participates — the in-memory shape of an evidence record can
    /// never disagree with what admission actually read. Returns the
    /// sealed snapshot of the same reads alongside the report, so the
    /// commit can re-verify the read set inside its transaction
    /// (TASK-0161, strict rule-5).
    fn admission_report(
        &self,
        task: &Task,
        policy_fp: Option<&str>,
        now: &str,
    ) -> ServiceResult<(evaluation::AdmissionReport, evaluation::AdmissionSnapshot)> {
        use evaluation::{
            AdmissionContext, AdmissionSnapshot, CandidateSelector, ClaimVerdict,
            CriterionEvidence, CriterionIntegrity, EffectId, ResolvedEffect,
        };
        let task_risk = task.snapshot().risk;
        let current = self
            .store
            .latest_observed_subject_fingerprint_for_task(task.id())?;
        let durable_claims = self.store.list_agent_claims_for_task(task.id())?;
        let durable_verdicts = self.store.list_claim_verifications_for_task(task.id())?;
        let durable_exemptions = self.store.list_policy_exemptions_for_task(task.id())?;
        let durable_discharges = self.store.list_approval_discharges_for_task(task.id())?;
        let mut criteria = Vec::with_capacity(task.acceptance().len());
        let mut snapshot_resolved: Vec<ResolvedEffect> = Vec::new();
        let mut snapshot_executions: Vec<(ExecutionId, TaskId)> = Vec::new();
        let mut snapshot_integrity: Vec<(
            String,
            ExecutionId,
            AttemptId,
            evaluation::IntegrityBasis,
        )> = Vec::new();
        for criterion in task.acceptance() {
            let requirement = task
                .evidence_requirements()
                .iter()
                .find(|req| req.criterion_id == *criterion)
                .ok_or_else(|| {
                    ServiceError::Storage(StorageError::Corrupt(format!(
                        "task {} has no requirement for {criterion}",
                        task.id()
                    )))
                })?;
            // CV-2: a verdict is eligible only against a durable claim for
            // the same criterion whose asserter differs from the verifier
            // (the trusted identity). Loading the durable claims here is
            // what makes that check meaningful at the service boundary.
            let claims: Vec<&evaluation::AgentClaim> = durable_claims
                .iter()
                .filter(|claim| claim.criterion_id == criterion.as_str())
                .collect();
            let claim_ids: Vec<&str> = claims.iter().map(|claim| claim.claim_id.as_str()).collect();
            let verdicts: Vec<&evaluation::ClaimVerification> = durable_verdicts
                .iter()
                .filter(|verdict| claim_ids.contains(&verdict.claim_id.as_str()))
                .collect();
            let exemptions: Vec<&evaluation::PolicyExemption> = durable_exemptions
                .iter()
                .filter(|e| {
                    e.criterion_id
                        .as_deref()
                        .is_none_or(|c| c == criterion.as_str())
                })
                .collect();
            let discharge = durable_discharges.iter().find(|d| {
                d.criterion_id == *criterion
                    && d.consumed_by.is_none()
                    && current
                        .as_ref()
                        .is_some_and(|fp| fp == &d.subject_fingerprint)
            });
            // CV-9 rule 4: resolve the basis of every relied-on VERIFIED
            // verdict to task-bound observed effects. Unresolvable,
            // malformed, or foreign refs are skipped, never substituted —
            // the core then fails closed for the verdicts that named them.
            // Store failures propagate loud (fail closed with a hard
            // error, not a quiet refusal).
            let mut resolved_effects = Vec::new();
            let mut relied_on = Vec::new();
            for verdict in verdicts
                .iter()
                .filter(|v| v.verdict == ClaimVerdict::Verified && v.superseded_by.is_none())
            {
                for reference in &verdict.basis.effect_refs {
                    let Ok(effect_id) = EffectId::new(reference) else {
                        continue;
                    };
                    let Some(effect) = self.store.get_observed_effect(&effect_id)? else {
                        continue;
                    };
                    let owner = match self
                        .store
                        .get_execution(&effect.provenance().execution_id)?
                    {
                        Some(execution) if execution.task_id() == task.id() => {
                            execution.task_id().clone()
                        }
                        _ => continue,
                    };
                    if !relied_on.iter().any(|(execution_id, attempt_id)| {
                        *execution_id == effect.provenance().execution_id
                            && *attempt_id == effect.provenance().attempt_id
                    }) {
                        relied_on.push((
                            effect.provenance().execution_id.clone(),
                            effect.provenance().attempt_id.clone(),
                        ));
                    }
                    if !snapshot_executions
                        .iter()
                        .any(|(execution_id, _)| *execution_id == effect.provenance().execution_id)
                    {
                        snapshot_executions.push((effect.provenance().execution_id.clone(), owner));
                    }
                    if resolved_effects
                        .iter()
                        .all(|resolved: &ResolvedEffect| resolved.effect_id != *reference)
                    {
                        resolved_effects.push(ResolvedEffect {
                            effect_id: reference.clone(),
                            subject_fingerprint: effect.subject_fingerprint().clone(),
                        });
                    }
                }
            }
            for resolved in &resolved_effects {
                if !snapshot_resolved
                    .iter()
                    .any(|known: &ResolvedEffect| known.effect_id == resolved.effect_id)
                {
                    snapshot_resolved.push(resolved.clone());
                }
            }
            // The gate mirrors the core's: empty params or non-candidate
            // selectors never consult integrity, so `None` here only
            // reaches the core where the core ignores it.
            let integrity = if requirement.params.is_empty()
                || CandidateSelector::from_requirement(requirement).is_err()
            {
                None
            } else {
                match self.integrity_basis(&relied_on)? {
                    Some((execution_id, attempt_id, basis)) => {
                        snapshot_integrity.push((
                            criterion.clone(),
                            execution_id.clone(),
                            attempt_id.clone(),
                            basis.clone(),
                        ));
                        Some(basis)
                    }
                    None => None,
                }
            };
            criteria.push(CriterionEvidence {
                requirement: Some(requirement),
                current_fingerprint: current.as_ref(),
                claims,
                verdicts,
                exemptions,
                discharge,
                integrity,
                resolved_effects,
            });
        }
        let context = AdmissionContext {
            task_risk: task_risk.as_deref(),
            current_policy_fingerprint: policy_fp,
            now,
        };
        let report = evaluation::evaluate_admission(&criteria, &context);
        let integrity_inputs: Vec<CriterionIntegrity> = snapshot_integrity
            .iter()
            .map(
                |(criterion_id, execution_id, attempt_id, basis)| CriterionIntegrity {
                    criterion_id: criterion_id.as_str(),
                    execution_id,
                    attempt_id,
                    basis,
                },
            )
            .collect();
        let relied_refs: Vec<(&ExecutionId, &TaskId)> = snapshot_executions
            .iter()
            .map(|(execution_id, owner)| (execution_id, owner))
            .collect();
        let snapshot = AdmissionSnapshot::capture(
            task,
            current.as_ref(),
            &durable_claims,
            &durable_verdicts,
            &durable_exemptions,
            &durable_discharges,
            &snapshot_resolved,
            &relied_refs,
            &integrity_inputs,
            policy_fp,
            now,
        );
        Ok((report, snapshot))
    }
    /// Current integrity basis for one candidate-shaped criterion (CV-9
    /// rule 4): the integrity assessment of the relied-on candidate
    /// execution, bound to its evaluated base. `candidates` arrive in
    /// basis order; the first execution with both an assessment and a
    /// bound base wins, and its keys travel with the basis so the commit
    /// can re-read the same rows (TASK-0161, strict rule-5). `None` (no
    /// assessment, no base) fails closed in the core.
    fn integrity_basis(
        &self,
        candidates: &[(ExecutionId, AttemptId)],
    ) -> ServiceResult<Option<(ExecutionId, AttemptId, evaluation::IntegrityBasis)>> {
        use evaluation::{CandidateIntegrityOutcome, IntegrityBasis};
        for (execution_id, attempt_id) in candidates {
            let assessment = self
                .store
                .get_candidate_integrity_assessment(execution_id.as_str(), attempt_id.as_str())?;
            let base = self
                .store
                .get_evaluated_policy_base(execution_id.as_str())?;
            if let (Some(assessment), Some(base)) = (assessment, base) {
                return Ok(Some((
                    execution_id.clone(),
                    attempt_id.clone(),
                    IntegrityBasis {
                        clean: assessment.outcome == CandidateIntegrityOutcome::Intact,
                        requirement_revision: assessment.requirement_revision.clone(),
                        policy_fingerprint: assessment.policy_fingerprint.clone(),
                        evaluated_base: base.policy.fingerprint().to_owned(),
                    },
                )));
            }
        }
        Ok(None)
    }
    /// Unconsumed discharges the atomic commit must consume, if any.
    fn consumed_discharges(&self, task_id: &TaskId) -> ServiceResult<Vec<evaluation::DischargeId>> {
        let Some(current) = self
            .store
            .latest_observed_subject_fingerprint_for_task(task_id)?
        else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for discharge in self.store.list_approval_discharges_for_task(task_id)? {
            if discharge.consumed_by.is_none() && discharge.subject_fingerprint == current {
                found.push(discharge.discharge_id.clone());
            }
        }
        Ok(found)
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
