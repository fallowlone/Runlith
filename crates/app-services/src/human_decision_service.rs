//! Trusted bounded Human Decision Queue application boundary (TASK-0079).

use std::path::Path;

use plan_domain::{
    EvidenceRef, HumanContinuation, HumanDecisionReason, HumanDecisionRequest,
    HumanDecisionRequestId, HumanDecisionStatus, PlanBranchId, PlannerPort, PlanningBudget,
    ReplanRequest, RiskEscalationGrant, ScopeBound, MAX_DECOMPOSITION_DEPTH,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};
use storage_sqlite::{
    AbandonTakeover, AdmitTakeover, AppendTakeover, NewHumanDecisionRequest, NewJournalEvent,
    SqliteStore, StorageError,
};
use takeover::{
    validate_takeover_expiry, AuthoredField, HandoffPacket, TakeoverAdmission, TakeoverRecordId,
    TakeoverStateRefs,
};
use task_domain::{Task, TaskId, TaskState, TransitionCommand};

use crate::{
    format_rfc3339, Clock, PlanningPolicyContext, PlanningProposalOutcome, PlanningService,
    PlanningServiceError, ServiceResult, SystemClock, TaskService,
};

#[derive(Clone, Debug)]
pub struct HumanDecisionCreate {
    pub task_id: TaskId,
    pub plan_branch_id: PlanBranchId,
    pub reason: HumanDecisionReason,
    pub risk: plan_domain::RiskClass,
    pub affected_scope: ScopeBound,
    pub options: Vec<HumanContinuation>,
    pub recommendation: Option<HumanContinuation>,
    pub evidence: Vec<EvidenceRef>,
    pub proposal_hash: Option<String>,
    pub plan_head: u64,
    pub planning_input_snapshot_id: plan_domain::PlanningInputSnapshotId,
    pub planning_input_fingerprint: String,
    pub policy_fingerprint: String,
    pub expires_at_ms: Option<u64>,
}

/// Human-authored appends to an ACTIVE takeover record. Each provided
/// field overwrites its column; absent fields are untouched.
#[derive(Clone, Debug)]
pub struct TakeoverAppends {
    pub actions: Option<AuthoredField>,
    pub uncertainty: Option<AuthoredField>,
    pub question: Option<AuthoredField>,
    pub resulting_revision: Option<u64>,
}

pub struct HumanDecisionService<C: Clock = SystemClock> {
    store: SqliteStore,
    clock: C,
}

impl HumanDecisionService<SystemClock> {
    pub fn open(path: impl AsRef<Path>) -> ServiceResult<Self> {
        Ok(Self {
            store: SqliteStore::open(path)?,
            clock: SystemClock,
        })
    }
}

impl<C: Clock> HumanDecisionService<C> {
    pub fn attach(store: SqliteStore, clock: C) -> Self {
        Self { store, clock }
    }

    pub fn create(&mut self, request: HumanDecisionCreate) -> ServiceResult<HumanDecisionRequest> {
        Ok(self.store.create_human_decision_request(
            NewHumanDecisionRequest {
                task_id: request.task_id,
                plan_branch_id: request.plan_branch_id,
                reason: request.reason,
                risk: request.risk,
                affected_scope: request.affected_scope,
                options: request.options,
                recommendation: request.recommendation,
                evidence: request.evidence,
                proposal_hash: request.proposal_hash,
                approved_proposal: None,
                plan_head: request.plan_head,
                planning_input_snapshot_id: request.planning_input_snapshot_id,
                planning_input_fingerprint: request.planning_input_fingerprint,
                policy_fingerprint: request.policy_fingerprint,
                expires_at_ms: request.expires_at_ms,
            },
            self.clock.now_millis(),
        )?)
    }

    pub fn get(&self, id: &HumanDecisionRequestId) -> ServiceResult<Option<HumanDecisionRequest>> {
        Ok(self.store.get_human_decision(id)?)
    }

    pub fn list_pending(&self) -> ServiceResult<Vec<HumanDecisionRequest>> {
        Ok(self.store.list_pending_human_decisions()?)
    }

    pub fn decide(
        &mut self,
        id: &HumanDecisionRequestId,
        selected: HumanContinuation,
        decided_by: &str,
    ) -> ServiceResult<HumanDecisionRequest> {
        Ok(self
            .store
            .resolve_human_decision(id, selected, decided_by, self.clock.now_millis())?)
    }

    /// Applies RESUME or ABORT when no planner call is required. A V9 risk
    /// RESUME stays RESOLVED until an exact PlanRevision atomically consumes
    /// it; this method therefore refuses to consume risk escalation grants.
    pub fn apply_local(
        &mut self,
        id: &HumanDecisionRequestId,
    ) -> ServiceResult<HumanDecisionRequest> {
        let record = self
            .store
            .validate_human_decision_current(id, self.clock.now_millis())?;
        if record.status != HumanDecisionStatus::Resolved {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            }
            .into());
        }
        let selected = record.selected.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "resolved human decision {} has no selection",
                id.as_str()
            ))
        })?;
        match selected {
            HumanContinuation::Resume if record.reason == HumanDecisionReason::RiskEscalation => {
                Err(StorageError::HumanDecisionState {
                    request_id: id.as_str().to_owned(),
                    state: "RESOLVED_RISK_GRANT_AWAITS_PLAN_COMMIT".to_owned(),
                }
                .into())
            }
            HumanContinuation::Resume | HumanContinuation::Abort => Ok(self
                .store
                .consume_human_decision(id, selected, self.clock.now_millis())?),
            HumanContinuation::Replan => Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: "RESOLVED_REPLAN_REQUIRES_PLANNING_BOUNDARY".to_owned(),
            }
            .into()),
            HumanContinuation::Takeover => Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: "RESOLVED_TAKEOVER_REQUIRES_TAKEOVER_BOUNDARY".to_owned(),
            }
            .into()),
        }
    }

    /// Admits a Resolved TAKEOVER decision: the store checks every
    /// precondition and applies the park plus the ACTIVE record
    /// atomically. Refusals leave zero state change and never
    /// supersede: the operator retries the same decision once the
    /// blocking run or attempt clears.
    pub fn apply_takeover(
        &mut self,
        id: &HumanDecisionRequestId,
        expires_at_ms: u64,
    ) -> ServiceResult<TakeoverAdmission> {
        let now = self.clock.now_millis();
        validate_takeover_expiry(now, expires_at_ms)
            .map_err(|error| StorageError::ConstraintViolation(error.to_string()))?;
        let record = self.store.validate_human_decision_current(id, now)?;
        if record.status != HumanDecisionStatus::Resolved
            || record.selected != Some(HumanContinuation::Takeover)
        {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            }
            .into());
        }
        let task = self
            .store
            .get_task(&record.task_id)?
            .ok_or_else(|| missing_takeover_task(&record.task_id))?;
        let mut next = task.clone();
        next.apply(TransitionCommand::Block {
            reason: format!("takeover for human decision {}", id.as_str()),
        })?;
        let park = takeover_park_event(now, &task, &next, id);
        Ok(self.store.admit_takeover(AdmitTakeover {
            hdr_id: id.clone(),
            expires_at_ms,
            park_event: park,
            now_ms: now,
        })?)
    }

    /// Appends human-authored fields to an ACTIVE takeover record.
    pub fn append_takeover(
        &mut self,
        record_id: &TakeoverRecordId,
        appends: TakeoverAppends,
    ) -> ServiceResult<TakeoverAdmission> {
        Ok(self.store.append_takeover(AppendTakeover {
            record_id: record_id.clone(),
            actions: appends.actions,
            uncertainty: appends.uncertainty,
            question: appends.question,
            resulting_revision: appends.resulting_revision,
            now_ms: self.clock.now_millis(),
        })?)
    }

    /// Abandons every expired ACTIVE takeover record (optionally scoped
    /// to one task), unparking each scope. An ACTIVE record on a
    /// non-Blocked task is impossible through the shipped transitions
    /// and fails closed instead of abandoning.
    pub fn reconcile_takeover_expiry(
        &mut self,
        task_id: Option<&TaskId>,
    ) -> ServiceResult<Vec<TakeoverAdmission>> {
        let now = self.clock.now_millis();
        let mut abandoned = Vec::new();
        for record in self.store.expired_active_takeovers(task_id, now)? {
            let task = self
                .store
                .get_task(&record.task_id)?
                .ok_or_else(|| missing_takeover_task(&record.task_id))?;
            if task.state() != TaskState::Blocked {
                return Err(StorageError::Corrupt(format!(
                    "takeover {} is ACTIVE but task {} is {}",
                    record.id.as_str(),
                    record.task_id.as_str(),
                    task.state().as_str(),
                ))
                .into());
            }
            let mut next = task.clone();
            next.apply(TransitionCommand::Start)?;
            let unpark = takeover_unpark_event(
                now,
                &task,
                &next,
                &record.id,
                &format!("takeover-expiry:{}:{now}", record.id.as_str()),
            );
            abandoned.push(self.store.abandon_takeover(AbandonTakeover {
                record_id: record.id.clone(),
                unpark_event: unpark,
                now_ms: now,
            })?);
        }
        Ok(abandoned)
    }

    /// Renders the human-facing handoff packet over the HDR plus the
    /// takeover record: the task goal, canonical state refs, authored
    /// fields and blockers (unresolved dependencies plus the HDR
    /// reason). Read-only.
    pub fn render_handoff_packet(
        &self,
        tasks: &TaskService<C>,
        record_id: &TakeoverRecordId,
    ) -> ServiceResult<HandoffPacket> {
        let record = self
            .store
            .get_takeover(record_id)?
            .ok_or_else(|| missing_takeover_record(record_id))?;
        let hdr = self
            .store
            .get_human_decision(&record.hdr_id)?
            .ok_or_else(|| {
                StorageError::Corrupt(format!(
                    "takeover {} references missing human decision {}",
                    record.id.as_str(),
                    record.hdr_id.as_str(),
                ))
            })?;
        let task = tasks
            .get_task(&record.task_id)?
            .ok_or_else(|| missing_takeover_task(&record.task_id))?;
        let mut blockers = Vec::new();
        for dependency in tasks.list_dependencies(&record.task_id)? {
            let target = tasks
                .get_task(&dependency)?
                .ok_or_else(|| missing_takeover_task(&dependency))?;
            if !target.state().is_terminal() {
                blockers.push(dependency.as_str().to_owned());
            }
        }
        blockers.push(hdr.reason.as_str().to_owned());
        let anchors = self.store.takeover_anchors(record_id)?;
        Ok(HandoffPacket {
            goal: task.objective().to_owned(),
            state: TakeoverStateRefs {
                task_id: record.task_id.as_str().to_owned(),
                task_event_seq: self.store.latest_task_event_seq(&record.task_id)?,
                plan_branch_id: record.plan_branch_id.as_str().to_owned(),
                plan_head: self
                    .store
                    .get_plan_head(&record.plan_branch_id)?
                    .unwrap_or(0),
                lease_id: anchors.into_iter().next().map(|anchor| anchor.lease_id),
            },
            actions: record.actions.clone(),
            blockers,
            uncertainty: record.uncertainty.clone(),
            question: record.question.clone(),
        })
    }

    /// Returns a V9 validation grant only from a fresh durable human RESUME.
    /// The grant remains non-bearer evidence: Plan commit re-validates and
    /// consumes the source request atomically.
    pub fn risk_grant(&self, id: &HumanDecisionRequestId) -> ServiceResult<RiskEscalationGrant> {
        let record = self
            .store
            .validate_human_decision_current(id, self.clock.now_millis())?;
        if record.reason != HumanDecisionReason::RiskEscalation
            || record.status != HumanDecisionStatus::Resolved
            || record.selected != Some(HumanContinuation::Resume)
        {
            return Err(StorageError::HumanDecisionState {
                request_id: id.as_str().to_owned(),
                state: record.status.as_str().to_owned(),
            }
            .into());
        }
        let proposal_hash = record.proposal_hash.ok_or_else(|| {
            StorageError::Corrupt(format!(
                "risk decision {} has no proposal hash",
                id.as_str()
            ))
        })?;
        let expires_at_ms = record.expires_at_ms.ok_or_else(|| {
            StorageError::Corrupt(format!("risk decision {} has no expiry", id.as_str()))
        })?;
        Ok(RiskEscalationGrant {
            proposal_hash,
            expires_at_ms,
            consumed: false,
        })
    }

    /// Applies REPLAN through the ordinary PlanningService boundary. The
    /// caller supplies current trusted policy; it must still match the exact
    /// policy/risk/scope/task facts bound to the human request.
    pub fn apply_replan(
        &mut self,
        planning: &mut PlanningService<C>,
        planner: &impl PlannerPort,
        id: &HumanDecisionRequestId,
        policy: &PlanningPolicyContext,
    ) -> Result<PlanningProposalOutcome, PlanningServiceError>
    where
        C: Clone,
    {
        let record = self
            .store
            .validate_human_decision_current(id, self.clock.now_millis())
            .map_err(PlanningServiceError::Storage)?;
        if record.status != HumanDecisionStatus::Resolved
            || record.selected != Some(HumanContinuation::Replan)
        {
            return Err(PlanningServiceError::Storage(
                StorageError::HumanDecisionState {
                    request_id: id.as_str().to_owned(),
                    state: record.status.as_str().to_owned(),
                },
            ));
        }
        if policy.parent_task_ref != record.task_id.as_str()
            || !self
                .store
                .planning_input_policy_matches(
                    &record.precondition.planning_input_snapshot_id,
                    &policy.parent_task_ref,
                    &policy.scope_bound,
                    policy.baseline_risk,
                    &policy.affected_parent_criteria,
                    &policy.lineage_fingerprints,
                    policy.risk_grant.as_ref(),
                    &policy.policy_fingerprint,
                )
                .map_err(PlanningServiceError::Storage)?
        {
            return Err(PlanningServiceError::Storage(
                StorageError::HumanDecisionStale {
                    request_id: id.as_str().to_owned(),
                    reason: "current planning policy no longer matches the human decision"
                        .to_owned(),
                },
            ));
        }
        let evidence = EvidenceRef::new(format!("human-decision:{}", id.as_str()))?;
        let outcome = planning.propose_replan_with_human_decision(
            planner,
            &record.plan_branch_id,
            ReplanRequest {
                branch_root_ref: policy.parent_task_ref.clone(),
                base_plan_revision: record.precondition.plan_head,
                triggering_evidence_refs: vec![evidence],
                resolved_evidence: Vec::new(),
                scope_bound: policy.scope_bound.clone(),
                baseline_risk: policy.baseline_risk,
                depth_remaining: MAX_DECOMPOSITION_DEPTH,
                budget_remaining: PlanningBudget {
                    calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
                    input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
                },
            },
            policy,
            Some(id),
        );
        match outcome {
            Ok(PlanningProposalOutcome::Accepted(accepted)) => {
                self.store
                    .mark_human_decision_consumed_after_application(
                        id,
                        HumanContinuation::Replan,
                        self.clock.now_millis(),
                    )
                    .map_err(PlanningServiceError::Storage)?;
                Ok(PlanningProposalOutcome::Accepted(accepted))
            }
            Ok(outcome) => {
                // A continuation that did not publish has not succeeded.
                // Fence this exact authority so a later trusted planning
                // pass may create a replacement; retain the row as evidence.
                self.supersede_after_failed_application(id)?;
                Ok(outcome)
            }
            Err(error) => {
                // Planner/persistence failures after a human chose REPLAN
                // must not leave a resolved row permanently wedging scope.
                // The supersession itself is durable and fail-closed.
                self.supersede_after_failed_application(id)?;
                Err(error)
            }
        }
    }

    fn supersede_after_failed_application(
        &mut self,
        id: &HumanDecisionRequestId,
    ) -> Result<(), PlanningServiceError> {
        let Some(current) = self
            .store
            .get_human_decision(id)
            .map_err(PlanningServiceError::Storage)?
        else {
            return Err(PlanningServiceError::Storage(StorageError::NotFound(
                format!("human decision {}", id.as_str()),
            )));
        };
        if matches!(
            current.status,
            HumanDecisionStatus::Pending | HumanDecisionStatus::Resolved
        ) {
            self.store
                .supersede_unusable_human_decision(id)
                .map_err(PlanningServiceError::Storage)?;
        }
        Ok(())
    }

    /// Current bounded control-plane blocker for one Task/Plan scope.
    pub fn blocker_for_scope(
        &self,
        task_id: &TaskId,
        branch: &PlanBranchId,
    ) -> ServiceResult<Option<HumanDecisionRequest>> {
        let Some(record) = self
            .store
            .latest_human_decision_for_scope(task_id, branch)?
        else {
            return Ok(None);
        };
        // A stale request cannot authorize execution. Surface the typed
        // stale refusal to the orchestration owner instead of ignoring it.
        let current = self
            .store
            .validate_human_decision_current(&record.id, self.clock.now_millis())?;
        Ok(Some(current))
    }
}

fn missing_takeover_task(id: &TaskId) -> StorageError {
    StorageError::NotFound(format!("takeover task {id} does not exist"))
}

fn missing_takeover_record(id: &TakeoverRecordId) -> StorageError {
    StorageError::NotFound(format!("takeover {} does not exist", id.as_str()))
}

/// Mints the RUNNING→BLOCKED park journal evidence for admission.
/// States derive from the domain-validated edge; the HDR identity is
/// both the human-readable reason and the structured causation.
pub(crate) fn takeover_park_event(
    now_ms: u64,
    from: &Task,
    next: &Task,
    hdr_id: &HumanDecisionRequestId,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: storage_sqlite::EventType::Blocked,
        task_id: Some(next.id().clone()),
        project_id: Some(next.project_id().clone()),
        from_state: Some(from.state()),
        to_state: Some(next.state()),
        reason: Some(format!("takeover for human decision {}", hdr_id.as_str())),
        correlation_id: format!("takeover-admit:{}", hdr_id.as_str()),
        causation_id: format!("human-decision:{}", hdr_id.as_str()),
        occurred_at: format_rfc3339(now_ms),
        provenance: "human-decision-service/apply-takeover".to_owned(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}

/// Mints the BLOCKED→RUNNING unpark journal evidence for return and
/// abandonment. The takeover record id is the correlation (the
/// resolution reference); the caller names the trigger as causation.
pub(crate) fn takeover_unpark_event(
    now_ms: u64,
    from: &Task,
    next: &Task,
    record_id: &TakeoverRecordId,
    causation: &str,
) -> NewJournalEvent {
    NewJournalEvent {
        event_type: storage_sqlite::EventType::Started,
        task_id: Some(next.id().clone()),
        project_id: Some(next.project_id().clone()),
        from_state: Some(from.state()),
        to_state: Some(next.state()),
        reason: None,
        correlation_id: record_id.as_str().to_owned(),
        causation_id: causation.to_owned(),
        occurred_at: format_rfc3339(now_ms),
        provenance: "human-decision-service/takeover-unpark".to_owned(),
        execution_id: None,
        attempt_id: None,
        attempt_outcome: None,
        workspace_id: None,
        from_workspace_state: None,
        to_workspace_state: None,
        plan_revision: None,
    }
}
