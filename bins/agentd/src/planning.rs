//! G7 production planning composition (TASK-0078).
//!
//! `agentd` is the only composition root that instantiates the concrete
//! Claude `PlannerPort`. Every real planner call is authorized by the G6
//! Policy boundary and receives a Runner-owned `SandboxedLaunch` before the
//! provider adapter can spawn. ADR-0027 makes the typed PlannerPort request
//! the complete planner-readable product context, so the provider receives no
//! repository filesystem authority. Its cwd is an empty app-owned isolation
//! anchor; provider support paths stay adapter/backend-owned.

use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use app_services::{HumanDecisionService, PlanningProposalOutcome};
use ipc_contracts::proto::{
    planning_core_request, planning_core_response, ApplyHumanContinuationRequest, BusinessError,
    BusinessErrorCode, PlanningContext, PlanningCoreRequest, PlanningCoreResponse,
    PlanningOperationResponse,
};
use plan_domain::{
    AtomicityAssessment, AtomicityRequest, DecompositionRequest, EvidenceRef, HumanContinuation,
    HumanDecisionRequestId, PlanBranchId, PlanChangeSet, PlannerCallSpend, PlannerFault,
    PlannerPort, PlanningBudget, ReplanRequest, MAX_DECOMPOSITION_DEPTH,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};
use task_domain::TaskId;

use crate::execution::{
    build_provider_launch_security_for_context, prepare_provider_launch, ExecutionComposition,
    ProviderLaunchContext, ProviderWorkspaceAccess,
};
use crate::provider_credential::ProviderCredential;

const PLANNER_LAUNCH_REQUESTER: &str = "planning-service";
static PLANNER_OPERATION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Production Claude planner wrapper. The inner provider adapter remains
/// proposal-only; this wrapper owns only the G6/Runner launch boundary.
pub struct SecuredClaudePlanner {
    inner: provider_claude::ClaudePlanner,
    db_path: PathBuf,
    authority_task_ref: String,
    credential: Result<ProviderCredential, String>,
}

impl SecuredClaudePlanner {
    fn call_secured<T>(
        &self,
        kind: &str,
        request_task_ref: &str,
        invoke: impl FnOnce(&provider_claude::ClaudePlanner) -> Result<T, PlannerFault>,
    ) -> Result<T, PlannerFault> {
        if request_task_ref != self.authority_task_ref {
            return Err(not_started(
                "planner request task does not match the composition-bound task",
            ));
        }

        let call_id = PLANNER_OPERATION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let launch = build_provider_launch_security_for_context(ProviderLaunchContext {
            task_id: &self.authority_task_ref,
            execution_id: None,
            attempt_id: None,
            operation_id: format!("planner.launch:{kind}:{call_id}"),
            requester: PLANNER_LAUNCH_REQUESTER,
            workspace: self.inner.workdir(),
            executable: self.inner.executable(),
            workspace_access: ProviderWorkspaceAccess::ReadOnly,
            credential: self.credential.clone(),
        })
        .map_err(not_started)?;
        let support_read = self
            .inner
            .isolation_support_read_roots()
            .map_err(not_started)?;
        let support_write = self
            .inner
            .isolation_support_write_roots()
            .map_err(not_started)?;
        let environment = self.inner.child_environment().map_err(not_started)?;
        let prepared = prepare_provider_launch(
            &self.db_path,
            &launch,
            support_read,
            support_write,
            environment,
        )
        .map_err(not_started)?;

        // Inside the G6 boundary the inner adapter reports its own spend. A
        // boundary error before the adapter ran (isolation enforcement) is
        // zero spend; one after it ran (post-call audit) discarded that
        // evidence, so it is charged as unmeasured spend.
        let adapter_invoked = Cell::new(false);
        let result = prepared
            .invoke(|sandboxed_launch, secrets| {
                adapter_invoked.set(true);
                let planner = self
                    .inner
                    .clone()
                    .with_sandboxed_launch_and_secrets(sandboxed_launch, secrets);
                invoke(&planner)
            })
            .map_err(|detail| PlannerFault::ProviderFailure {
                typed_cause: detail,
                spend: if adapter_invoked.get() {
                    PlannerCallSpend::Unmeasured
                } else {
                    PlannerCallSpend::NotStarted
                },
            })?;
        if let (Ok(credential), Err(PlannerFault::ProviderFailure { typed_cause, .. })) =
            (&self.credential, &result)
        {
            credential.observe_failure(typed_cause);
        }
        result
    }
}

impl PlannerPort for SecuredClaudePlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        let task_ref = input.task_ref.clone();
        self.call_secured("assess-atomicity", &task_ref, |planner| {
            planner.assess_atomicity(input)
        })
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let task_ref = input.parent_task_ref.clone();
        self.call_secured("decomposition", &task_ref, |planner| {
            planner.propose_decomposition(input)
        })
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        let task_ref = input.branch_root_ref.clone();
        self.call_secured("replan", &task_ref, |planner| planner.propose_replan(input))
    }
}

impl ExecutionComposition {
    /// Opens the trusted Planning application service over the daemon-owned
    /// canonical database.
    pub fn planning_service(
        &self,
    ) -> Result<
        app_services::PlanningService<app_services::SystemClock>,
        app_services::PlanningServiceError,
    > {
        app_services::PlanningService::open(self.db_path(), app_services::SystemClock)
    }

    /// Instantiates the concrete production Claude planner for one durable
    /// Task. ADR-0027 makes the typed PlannerPort request the complete set of
    /// product inputs the planner may read, so this path deliberately does
    /// not grant repository filesystem access. The provider runs from an
    /// empty app-owned per-task directory used only as a sandbox/cwd anchor.
    /// The returned planner is still incapable of spawning until each call
    /// crosses Policy + Runner inside [`SecuredClaudePlanner`].
    pub fn secured_claude_planner(
        &self,
        authority_task_ref: impl Into<String>,
    ) -> Result<SecuredClaudePlanner, String> {
        let authority_task_ref = authority_task_ref.into();
        let task_id = TaskId::new(authority_task_ref.clone())
            .map_err(|error| format!("planner authority task ref is invalid: {error}"))?;
        let store = storage_sqlite::SqliteStore::open(self.db_path())
            .map_err(|error| format!("planner authority store unavailable: {error}"))?;
        if store
            .get_task(&task_id)
            .map_err(|error| format!("planner authority task lookup failed: {error}"))?
            .is_none()
        {
            return Err(format!(
                "planner authority task {} does not exist",
                task_id.as_str()
            ));
        }

        let workdir = planner_workdir(self.db_path(), &task_id)?;
        let executable = self.resolved_provider_executable()?;
        let credential = self.provider_credential(executable.as_os_str());
        let config = provider_claude::ClaudeConfig::default()
            .with_executable(executable)
            .map_err(|error| format!("planner provider configuration refused: {error}"))?;
        let inner = provider_claude::ClaudePlanner::bind(
            config,
            workdir,
            self.provider_environment_base()?,
        )?;
        Ok(SecuredClaudePlanner {
            inner,
            db_path: self.db_path().to_path_buf(),
            authority_task_ref,
            credential,
        })
    }

    /// Returns the fingerprint of the current product-owned G6 Policy that
    /// guards the production planner launch. The operation identity is not
    /// part of `CompiledPolicy` canonicalization, so this is the same policy
    /// identity used when an actual planner call crosses the launch boundary.
    pub fn planning_policy_fingerprint(&self, authority_task_ref: &str) -> Result<String, String> {
        let task_id = TaskId::new(authority_task_ref.to_owned())
            .map_err(|error| format!("planner authority task ref is invalid: {error}"))?;
        let workdir = planner_workdir(self.db_path(), &task_id)?;
        let executable = self.resolved_provider_executable()?;
        let launch = build_provider_launch_security_for_context(ProviderLaunchContext {
            task_id: task_id.as_str(),
            execution_id: None,
            attempt_id: None,
            operation_id: "planner.policy-context".to_owned(),
            requester: PLANNER_LAUNCH_REQUESTER,
            workspace: &workdir,
            executable: executable.as_os_str(),
            workspace_access: ProviderWorkspaceAccess::ReadOnly,
            credential: self.provider_credential(executable.as_os_str()),
        })?;
        Ok(launch.policy_fingerprint().to_owned())
    }
}

fn planner_workdir(db_path: &std::path::Path, task_id: &TaskId) -> Result<PathBuf, String> {
    let parent = db_path.parent().ok_or_else(|| {
        format!(
            "planner workspace unavailable: database path {} has no parent",
            db_path.display()
        )
    })?;
    let root = parent.join("planner-workdirs");
    crate::data_dir::ensure_data_dir(&root)
        .map_err(|error| format!("planner workspace root unavailable: {error}"))?;
    let workdir = root.join(format!(
        "task-{}",
        app_services::managed_slug(task_id.as_str())
    ));
    crate::data_dir::ensure_data_dir(&workdir)
        .map_err(|error| format!("planner workspace unavailable: {error}"))?;
    Ok(workdir)
}

/// Refusal before the G6 boundary let any provider process start.
fn not_started(detail: impl Into<String>) -> PlannerFault {
    PlannerFault::ProviderFailure {
        typed_cause: detail.into(),
        spend: PlannerCallSpend::NotStarted,
    }
}

/// Version-5 control-plane mapping. It reconstructs bounded domain input and
/// invokes trusted services; it never accepts a graph or PlanRevision from a
/// client.
pub fn dispatch_control(
    composition: &ExecutionComposition,
    decisions: &mut HumanDecisionService,
    request: PlanningCoreRequest,
) -> PlanningCoreResponse {
    match request.payload {
        Some(planning_core_request::Payload::Trigger(request)) => {
            let context = match request.context {
                Some(value) => value,
                None => return invalid("planning context is required"),
            };
            let (task, branch, evidence) = match planning_intent(context) {
                Ok(value) => value,
                Err(message) => return invalid(&message),
            };
            let mut planning = match composition.planning_service() {
                Ok(value) => value,
                Err(error) => return internal(error.to_string()),
            };
            let policy_fingerprint = match composition.planning_policy_fingerprint(task.as_str()) {
                Ok(value) => value,
                Err(error) => return internal(error),
            };
            let policy = match planning.canonical_policy_context(&task, policy_fingerprint) {
                Ok(value) => value,
                Err(error) => return invalid(&error.to_string()),
            };
            let decomposition = DecompositionRequest {
                parent_task_ref: String::new(),
                base_plan_revision: 0,
                acceptance_criteria_refs: Vec::new(),
                scope_bound: policy.scope_bound.clone(),
                evidence_refs: vec![evidence],
                // Filled by the trusted planning service (TASK-0175);
                // caller-attached content is never forwarded.
                resolved_evidence: Vec::new(),
                baseline_risk: policy.baseline_risk,
                depth_remaining: MAX_DECOMPOSITION_DEPTH,
                budget_remaining: PlanningBudget {
                    calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
                    input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
                },
            };
            let planner = match composition.secured_claude_planner(task.as_str()) {
                Ok(value) => value,
                Err(error) => return internal(error),
            };
            match planning.propose_decomposition(&planner, &branch, decomposition, &policy) {
                Ok(outcome) => outcome_response(outcome, &branch),
                Err(error) => internal(error.to_string()),
            }
        }
        Some(planning_core_request::Payload::ApplyHumanContinuation(request)) => {
            apply_continuation(composition, decisions, request)
        }
        None => invalid("empty planning request"),
    }
}

fn apply_continuation(
    composition: &ExecutionComposition,
    decisions: &mut HumanDecisionService,
    request: ApplyHumanContinuationRequest,
) -> PlanningCoreResponse {
    let id = match HumanDecisionRequestId::new(request.request_id.clone()) {
        Ok(value) => value,
        Err(_) => return invalid("human decision request id is invalid"),
    };
    let record = match decisions.get(&id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            return error(
                BusinessErrorCode::NotFound,
                &format!("human decision {} does not exist", id.as_str()),
            )
        }
        Err(error) => return internal(error.to_string()),
    };
    if record.reason == plan_domain::HumanDecisionReason::RiskEscalation
        && record.selected == Some(HumanContinuation::Resume)
    {
        let mut planning = match composition.planning_service() {
            Ok(value) => value,
            Err(error) => return internal(error.to_string()),
        };
        let policy_fingerprint =
            match composition.planning_policy_fingerprint(record.task_id.as_str()) {
                Ok(value) => value,
                Err(error) => return internal(error),
            };
        let policy = match planning.canonical_policy_context(&record.task_id, policy_fingerprint) {
            Ok(value) => value,
            Err(error) => return invalid(&error.to_string()),
        };
        return match planning.resume_approved_risk_proposal(&id, &policy) {
            Ok(outcome) => outcome_response(outcome, &record.plan_branch_id),
            Err(error) => internal(error.to_string()),
        };
    }
    if record.selected == Some(HumanContinuation::Replan) {
        let mut planning = match composition.planning_service() {
            Ok(value) => value,
            Err(error) => return internal(error.to_string()),
        };
        let policy_fingerprint =
            match composition.planning_policy_fingerprint(record.task_id.as_str()) {
                Ok(value) => value,
                Err(error) => return internal(error),
            };
        let policy = match planning.canonical_policy_context(&record.task_id, policy_fingerprint) {
            Ok(value) => value,
            Err(error) => return invalid(&error.to_string()),
        };
        let planner = match composition.secured_claude_planner(record.task_id.as_str()) {
            Ok(value) => value,
            Err(error) => return internal(error),
        };
        return match decisions.apply_replan(&mut planning, &planner, &id, &policy) {
            Ok(outcome) => outcome_response(outcome, &record.plan_branch_id),
            Err(error) => internal(error.to_string()),
        };
    }
    match decisions.apply_local(&id) {
        Ok(record) => success(
            "CONTINUATION_APPLIED",
            &record.plan_branch_id,
            None,
            Some(record.id.as_str().to_owned()),
            record.status.as_str().to_owned(),
        ),
        Err(error) => internal(error.to_string()),
    }
}

fn planning_intent(
    context: PlanningContext,
) -> Result<(TaskId, PlanBranchId, EvidenceRef), String> {
    let task = TaskId::new(context.task_id).map_err(|error| format!("invalid task id: {error}"))?;
    let branch = PlanBranchId::new(context.plan_branch_id)
        .map_err(|error| format!("invalid plan branch id: {error}"))?;
    let evidence = EvidenceRef::new(context.evidence_ref)
        .map_err(|error| format!("invalid planning evidence reference: {error}"))?;
    // Historical protocol-v5 authority-bearing fields remain decodable for
    // compatibility but are intentionally ignored here. Canonical Task and
    // Policy state are re-derived above before any planner/publication work.
    let _ = (
        context.scope_roots,
        context.acceptance_criterion_ids,
        context.policy_fingerprint,
    );
    Ok((task, branch, evidence))
}

fn outcome_response(
    outcome: PlanningProposalOutcome,
    branch: &PlanBranchId,
) -> PlanningCoreResponse {
    match outcome {
        PlanningProposalOutcome::Accepted(plan) => success(
            "ACCEPTED",
            branch,
            Some(plan.revision().revision_seq),
            None,
            plan.revision().changeset_hash.clone(),
        ),
        PlanningProposalOutcome::WaitingForHuman(decision) => success(
            "WAITING_FOR_HUMAN",
            branch,
            None,
            Some(decision.id.as_str().to_owned()),
            decision.reason.as_str().to_owned(),
        ),
        PlanningProposalOutcome::Refused(value) => {
            success("REFUSED", branch, None, None, format!("{value:?}"))
        }
        PlanningProposalOutcome::NotApplicable(value) => {
            success("NOT_APPLICABLE", branch, None, None, format!("{value:?}"))
        }
        PlanningProposalOutcome::GuardrailTripped(value) => success(
            "GUARDRAIL_TRIPPED",
            branch,
            None,
            None,
            format!("{value:?}"),
        ),
        PlanningProposalOutcome::Blocked(value) => {
            success("BLOCKED", branch, None, None, format!("{value:?}"))
        }
    }
}

fn success(
    outcome: &str,
    branch: &PlanBranchId,
    revision_seq: Option<u64>,
    decision: Option<String>,
    detail: String,
) -> PlanningCoreResponse {
    PlanningCoreResponse {
        payload: Some(planning_core_response::Payload::Operation(
            PlanningOperationResponse {
                outcome: outcome.to_owned(),
                plan_branch_id: Some(branch.as_str().to_owned()),
                revision_seq,
                human_decision_request_id: decision,
                detail,
            },
        )),
    }
}
fn invalid(message: &str) -> PlanningCoreResponse {
    error(BusinessErrorCode::InvalidArgument, message)
}
fn internal(message: String) -> PlanningCoreResponse {
    error(BusinessErrorCode::Internal, &message)
}
fn error(code: BusinessErrorCode, message: &str) -> PlanningCoreResponse {
    PlanningCoreResponse {
        payload: Some(planning_core_response::Payload::Error(BusinessError {
            code: code as i32,
            message: message.chars().take(512).collect(),
        })),
    }
}

pub fn unavailable() -> PlanningCoreResponse {
    error(BusinessErrorCode::Internal, "planning service unavailable")
}
