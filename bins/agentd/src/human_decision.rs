//! G7 Human Decision Queue dispatcher. The wire can list/get/resolve
//! canonical requests; it cannot create requests or consume continuation
//! authority.

use app_services::{Clock, HumanDecisionService, ServiceError};
use ipc_contracts::proto::{
    human_decision_core_request, human_decision_core_response, BusinessError, BusinessErrorCode,
    DecideHumanDecisionResponse, GetHumanDecisionResponse, HumanContinuationDecision,
    HumanDecisionCoreRequest, HumanDecisionCoreResponse, HumanDecisionDto,
    ListHumanDecisionsResponse,
};
use plan_domain::{HumanContinuation, HumanDecisionRequest, HumanDecisionRequestId};

use crate::dispatch::map_error;

const LOCAL_DECIDER_PROVENANCE: &str = "authenticated-local-control-plane";

pub fn dispatch<C: Clock>(
    service: &mut HumanDecisionService<C>,
    request: HumanDecisionCoreRequest,
) -> HumanDecisionCoreResponse {
    match request.payload {
        Some(human_decision_core_request::Payload::ListPending(_)) => {
            match service.list_pending() {
                Ok(records) => success(human_decision_core_response::Payload::ListPending(
                    ListHumanDecisionsResponse {
                        decisions: records.into_iter().map(dto).collect(),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(human_decision_core_request::Payload::Get(request)) => {
            let id = match request_id(&request.request_id) {
                Ok(id) => id,
                Err(()) => return invalid_argument("human decision request id is invalid"),
            };
            match service.get(&id) {
                Ok(Some(record)) => success(human_decision_core_response::Payload::Get(
                    GetHumanDecisionResponse {
                        decision: Some(dto(record)),
                    },
                )),
                Ok(None) => not_found(id.as_str()),
                Err(error) => error_response(error),
            }
        }
        Some(human_decision_core_request::Payload::Decide(request)) => {
            let id = match request_id(&request.request_id) {
                Ok(id) => id,
                Err(()) => return invalid_argument("human decision request id is invalid"),
            };
            let continuation = match HumanContinuationDecision::try_from(request.continuation) {
                Ok(HumanContinuationDecision::Resume) => HumanContinuation::Resume,
                Ok(HumanContinuationDecision::Replan) => HumanContinuation::Replan,
                Ok(HumanContinuationDecision::Abort) => HumanContinuation::Abort,
                _ => {
                    return invalid_argument("human continuation must be resume, replan, or abort")
                }
            };
            match service.decide(&id, continuation, LOCAL_DECIDER_PROVENANCE) {
                Ok(record) => success(human_decision_core_response::Payload::Decide(
                    DecideHumanDecisionResponse {
                        decision: Some(dto(record)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        None => invalid_argument("empty human-decision request"),
    }
}

pub fn unavailable() -> HumanDecisionCoreResponse {
    HumanDecisionCoreResponse {
        payload: Some(human_decision_core_response::Payload::Error(
            BusinessError {
                code: BusinessErrorCode::Internal as i32,
                message: "human decision service unavailable".to_owned(),
            },
        )),
    }
}

fn request_id(value: &str) -> Result<HumanDecisionRequestId, ()> {
    if value.trim().is_empty()
        || value.len() > ipc_contracts::MAX_WIRE_ID_LEN
        || value.chars().any(char::is_control)
    {
        return Err(());
    }
    HumanDecisionRequestId::new(value.to_owned()).map_err(|_| ())
}

fn dto(record: HumanDecisionRequest) -> HumanDecisionDto {
    HumanDecisionDto {
        request_id: record.id.as_str().to_owned(),
        request_version: record.version,
        task_id: record.task_id.as_str().to_owned(),
        plan_branch_id: record.plan_branch_id.as_str().to_owned(),
        reason: record.reason.as_str().to_owned(),
        risk: record.risk.as_str().to_owned(),
        affected_scope: record.affected_scope.roots(),
        options: record.options.into_iter().map(continuation_wire).collect(),
        recommendation: record.recommendation.map(continuation_wire),
        evidence: record
            .evidence
            .into_iter()
            .map(|value| value.as_str().to_owned())
            .collect(),
        proposal_hash: record.proposal_hash,
        task_state_at_creation: record.precondition.task_state.as_str().to_owned(),
        task_event_seq: record.precondition.task_event_seq,
        plan_head: record.precondition.plan_head,
        planning_input_snapshot_id: record
            .precondition
            .planning_input_snapshot_id
            .as_str()
            .to_owned(),
        planning_input_fingerprint: record.precondition.planning_input_fingerprint,
        policy_fingerprint: record.precondition.policy_fingerprint,
        created_at_ms: record.created_at_ms,
        expires_at_ms: record.expires_at_ms,
        status: record.status.as_str().to_owned(),
        selected: record.selected.map(continuation_wire),
        decided_by: record.decided_by,
        decided_at_ms: record.decided_at_ms,
        consumed_at_ms: record.consumed_at_ms,
    }
}

fn continuation_wire(value: HumanContinuation) -> i32 {
    match value {
        HumanContinuation::Resume => HumanContinuationDecision::Resume as i32,
        HumanContinuation::Replan => HumanContinuationDecision::Replan as i32,
        HumanContinuation::Abort => HumanContinuationDecision::Abort as i32,
    }
}

fn success(payload: human_decision_core_response::Payload) -> HumanDecisionCoreResponse {
    HumanDecisionCoreResponse {
        payload: Some(payload),
    }
}

fn error_response(error: ServiceError) -> HumanDecisionCoreResponse {
    let (code, message) = map_error(&error);
    HumanDecisionCoreResponse {
        payload: Some(human_decision_core_response::Payload::Error(
            BusinessError {
                code: code as i32,
                message,
            },
        )),
    }
}

fn invalid_argument(message: &str) -> HumanDecisionCoreResponse {
    HumanDecisionCoreResponse {
        payload: Some(human_decision_core_response::Payload::Error(
            BusinessError {
                code: BusinessErrorCode::InvalidArgument as i32,
                message: message.to_owned(),
            },
        )),
    }
}

fn not_found(id: &str) -> HumanDecisionCoreResponse {
    HumanDecisionCoreResponse {
        payload: Some(human_decision_core_response::Payload::Error(
            BusinessError {
                code: BusinessErrorCode::NotFound as i32,
                message: format!("human decision {id} does not exist"),
            },
        )),
    }
}
