//! G7 Human Decision Queue dispatcher. The wire can list/get/resolve
//! canonical requests; it cannot create requests or consume continuation
//! authority.

use app_services::{
    AuthoredField, Clock, HandoffPacket, HumanDecisionService, NoteBody, Sensitivity, ServiceError,
    TakeoverAdmission, TakeoverAppends, TakeoverRecordId, TaskService,
};
use ipc_contracts::proto::{
    human_decision_core_request, human_decision_core_response, AppendTakeoverResponse,
    ApplyTakeoverResponse, BusinessError, BusinessErrorCode, DecideHumanDecisionResponse,
    GetHumanDecisionResponse, HandoffPacketDto, HumanContinuationDecision,
    HumanDecisionCoreRequest, HumanDecisionCoreResponse, HumanDecisionDto,
    ListHumanDecisionsResponse, ReconcileTakeoverExpiryResponse, RenderHandoffPacketResponse,
    TakeoverAuthoredField, TakeoverRecordDto,
};
use plan_domain::{HumanContinuation, HumanDecisionRequest, HumanDecisionRequestId};
use task_domain::TaskId;

use crate::dispatch::map_error;

const LOCAL_DECIDER_PROVENANCE: &str = "authenticated-local-control-plane";

pub fn dispatch<C: Clock>(
    service: &mut HumanDecisionService<C>,
    tasks: Option<&TaskService<C>>,
    request: HumanDecisionCoreRequest,
    negotiated_version: u32,
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
                Ok(HumanContinuationDecision::Takeover)
                    if ipc_contracts::supports_takeover_continuation(negotiated_version) =>
                {
                    HumanContinuation::Takeover
                }
                Ok(HumanContinuationDecision::Takeover) => {
                    return unsupported_protocol(
                        "takeover continuation requires protocol version 8 or higher",
                    )
                }
                _ => {
                    return invalid_argument(
                        "human continuation must be resume, replan, abort, or takeover",
                    )
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
        Some(human_decision_core_request::Payload::ApplyTakeover(request)) => {
            if let Some(refusal) = takeover_gate(negotiated_version) {
                return refusal;
            }
            let id = match request_id(&request.request_id) {
                Ok(id) => id,
                Err(()) => return invalid_argument("human decision request id is invalid"),
            };
            match service.apply_takeover(&id, request.expires_at_ms) {
                Ok(record) => success(human_decision_core_response::Payload::ApplyTakeover(
                    ApplyTakeoverResponse {
                        record: Some(record_dto(record)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(human_decision_core_request::Payload::AppendTakeover(request)) => {
            if let Some(refusal) = takeover_gate(negotiated_version) {
                return refusal;
            }
            let record_id = match takeover_record_id(&request.record_id) {
                Ok(id) => id,
                Err(()) => return invalid_argument("takeover record id is invalid"),
            };
            let appends = match takeover_appends(request) {
                Ok(appends) => appends,
                Err(message) => return invalid_argument(message),
            };
            match service.append_takeover(&record_id, appends) {
                Ok(record) => success(human_decision_core_response::Payload::AppendTakeover(
                    AppendTakeoverResponse {
                        record: Some(record_dto(record)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(human_decision_core_request::Payload::RenderHandoffPacket(request)) => {
            if let Some(refusal) = takeover_gate(negotiated_version) {
                return refusal;
            }
            let record_id = match takeover_record_id(&request.record_id) {
                Ok(id) => id,
                Err(()) => return invalid_argument("takeover record id is invalid"),
            };
            let Some(tasks) = tasks else {
                return internal_error("task service unavailable for handoff rendering");
            };
            match service.render_handoff_packet(tasks, &record_id) {
                Ok(packet) => success(human_decision_core_response::Payload::RenderHandoffPacket(
                    RenderHandoffPacketResponse {
                        packet: Some(packet_dto(packet)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(human_decision_core_request::Payload::ReconcileTakeoverExpiry(request)) => {
            if let Some(refusal) = takeover_gate(negotiated_version) {
                return refusal;
            }
            let task_id = match request.task_id {
                Some(value) => match TaskId::new(value) {
                    Ok(id) => Some(id),
                    Err(_) => return invalid_argument("task id is invalid"),
                },
                None => None,
            };
            match service.reconcile_takeover_expiry(task_id.as_ref()) {
                Ok(records) => success(
                    human_decision_core_response::Payload::ReconcileTakeoverExpiry(
                        ReconcileTakeoverExpiryResponse {
                            records: records.into_iter().map(record_dto).collect(),
                        },
                    ),
                ),
                Err(error) => error_response(error),
            }
        }
        None => invalid_argument("empty human-decision request"),
    }
}

fn takeover_gate(negotiated_version: u32) -> Option<HumanDecisionCoreResponse> {
    if ipc_contracts::supports_takeover_continuation(negotiated_version) {
        None
    } else {
        Some(unsupported_protocol(
            "takeover operations require protocol version 8 or higher",
        ))
    }
}

fn takeover_record_id(value: &str) -> Result<TakeoverRecordId, ()> {
    if value.trim().is_empty()
        || value.len() > ipc_contracts::MAX_WIRE_ID_LEN
        || value.chars().any(char::is_control)
    {
        return Err(());
    }
    TakeoverRecordId::new(value.to_owned()).map_err(|_| ())
}

fn takeover_appends(
    request: ipc_contracts::proto::AppendTakeoverRequest,
) -> Result<TakeoverAppends, &'static str> {
    Ok(TakeoverAppends {
        actions: authored_field(request.actions)?,
        uncertainty: authored_field(request.uncertainty)?,
        question: authored_field(request.question)?,
        resulting_revision: request.resulting_revision,
    })
}

fn authored_field(
    field: Option<TakeoverAuthoredField>,
) -> Result<Option<AuthoredField>, &'static str> {
    let Some(field) = field else {
        return Ok(None);
    };
    let sensitivity = Sensitivity::parse(&field.sensitivity)
        .map_err(|_| "takeover authored field sensitivity is invalid")?;
    let body = if field.is_ref {
        NoteBody::Ref(field.text)
    } else {
        NoteBody::Inline(field.text)
    };
    AuthoredField::new(body, sensitivity)
        .map(Some)
        .map_err(|_| "takeover authored field is invalid")
}

fn authored_dto(field: Option<AuthoredField>) -> Option<TakeoverAuthoredField> {
    field.map(|field| {
        let (text, is_ref) = match field.body {
            NoteBody::Inline(text) => (text, false),
            NoteBody::Ref(text) => (text, true),
        };
        TakeoverAuthoredField {
            text,
            sensitivity: field.sensitivity.as_str().to_owned(),
            is_ref,
        }
    })
}

fn record_dto(record: TakeoverAdmission) -> TakeoverRecordDto {
    TakeoverRecordDto {
        record_id: record.id.as_str().to_owned(),
        hdr_id: record.hdr_id.as_str().to_owned(),
        task_id: record.task_id.as_str().to_owned(),
        plan_branch_id: record.plan_branch_id.as_str().to_owned(),
        basis_plan_revision: record.basis_plan_revision,
        expires_at_ms: record.expires_at_ms,
        status: record.status.as_str().to_owned(),
        actions: authored_dto(record.actions),
        uncertainty: authored_dto(record.uncertainty),
        question: authored_dto(record.question),
        resulting_revision: record.resulting_revision,
        created_at_ms: record.created_at_ms,
        resolved_at_ms: record.resolved_at_ms,
    }
}

fn packet_dto(packet: HandoffPacket) -> HandoffPacketDto {
    HandoffPacketDto {
        goal: packet.goal,
        task_id: packet.state.task_id,
        task_event_seq: packet.state.task_event_seq,
        plan_branch_id: packet.state.plan_branch_id,
        plan_head: packet.state.plan_head,
        lease_id: packet.state.lease_id,
        actions: authored_dto(packet.actions),
        blockers: packet.blockers,
        uncertainty: authored_dto(packet.uncertainty),
        question: authored_dto(packet.question),
    }
}

fn internal_error(message: &str) -> HumanDecisionCoreResponse {
    HumanDecisionCoreResponse {
        payload: Some(human_decision_core_response::Payload::Error(
            BusinessError {
                code: BusinessErrorCode::Internal as i32,
                message: message.to_owned(),
            },
        )),
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
        HumanContinuation::Takeover => HumanContinuationDecision::Takeover as i32,
    }
}

fn unsupported_protocol(message: &str) -> HumanDecisionCoreResponse {
    HumanDecisionCoreResponse {
        payload: Some(human_decision_core_response::Payload::Error(
            BusinessError {
                code: BusinessErrorCode::UnsupportedProtocol as i32,
                message: message.to_owned(),
            },
        )),
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use app_services::{FixedClock, TaskService};
    use ipc_contracts::proto::{
        human_decision_core_request, AppendTakeoverRequest, ApplyTakeoverRequest,
        DecideHumanDecisionRequest, ReconcileTakeoverExpiryRequest, RenderHandoffPacketRequest,
    };

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDb(PathBuf);

    impl TestDb {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "runlith-agentd-human-decision-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.join("state.sqlite3"))
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            if let Some(parent) = self.0.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }

    fn decide_request(continuation: i32) -> HumanDecisionCoreRequest {
        HumanDecisionCoreRequest {
            payload: Some(human_decision_core_request::Payload::Decide(
                DecideHumanDecisionRequest {
                    request_id: "hdr-1".to_owned(),
                    continuation,
                },
            )),
        }
    }

    fn error_code(response: &HumanDecisionCoreResponse) -> i32 {
        match &response.payload {
            Some(human_decision_core_response::Payload::Error(error)) => error.code,
            other => panic!("expected typed error, got {other:?}"),
        }
    }

    #[test]
    fn takeover_decide_refuses_below_v8_without_touching_service() {
        let db = TestDb::new();
        let store = storage_sqlite::SqliteStore::open(&db.0).unwrap();
        let mut service = HumanDecisionService::attach(store, FixedClock(10));
        let response = dispatch(
            &mut service,
            None,
            decide_request(HumanContinuationDecision::Takeover as i32),
            ipc_contracts::IPC_PROTOCOL_VERSION_V4,
        );
        assert_eq!(
            error_code(&response),
            BusinessErrorCode::UnsupportedProtocol as i32
        );
    }

    #[test]
    fn unknown_continuation_stays_invalid_argument() {
        let db = TestDb::new();
        let store = storage_sqlite::SqliteStore::open(&db.0).unwrap();
        let mut service = HumanDecisionService::attach(store, FixedClock(10));
        let response = dispatch(&mut service, None, decide_request(99), 8);
        assert_eq!(
            error_code(&response),
            BusinessErrorCode::InvalidArgument as i32
        );
    }

    #[test]
    fn takeover_operations_refuse_below_v8_without_touching_service() {
        let requests = [
            HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::ApplyTakeover(
                    ApplyTakeoverRequest {
                        request_id: "hdr-1".to_owned(),
                        expires_at_ms: 9_999,
                    },
                )),
            },
            HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::AppendTakeover(
                    AppendTakeoverRequest {
                        record_id: "takeover-1".to_owned(),
                        actions: None,
                        uncertainty: None,
                        question: None,
                        resulting_revision: None,
                    },
                )),
            },
            HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::RenderHandoffPacket(
                    RenderHandoffPacketRequest {
                        record_id: "takeover-1".to_owned(),
                    },
                )),
            },
            HumanDecisionCoreRequest {
                payload: Some(
                    human_decision_core_request::Payload::ReconcileTakeoverExpiry(
                        ReconcileTakeoverExpiryRequest { task_id: None },
                    ),
                ),
            },
        ];
        for version in [
            ipc_contracts::IPC_PROTOCOL_VERSION_V0,
            ipc_contracts::IPC_PROTOCOL_VERSION_V4,
            ipc_contracts::IPC_PROTOCOL_VERSION_V7,
        ] {
            for request in requests.clone() {
                let db = TestDb::new();
                let store = storage_sqlite::SqliteStore::open(&db.0).unwrap();
                let mut service = HumanDecisionService::attach(store, FixedClock(10));
                let response = dispatch(&mut service, None, request, version);
                assert_eq!(
                    error_code(&response),
                    BusinessErrorCode::UnsupportedProtocol as i32,
                    "v{version} must refuse takeover operations"
                );
            }
        }
    }

    #[test]
    fn takeover_apply_reaches_service_on_v8() {
        let db = TestDb::new();
        let store = storage_sqlite::SqliteStore::open(&db.0).unwrap();
        let mut service = HumanDecisionService::attach(store, FixedClock(10));
        let tasks = TaskService::attach(
            storage_sqlite::SqliteStore::open(&db.0).unwrap(),
            FixedClock(10),
        );
        let response = dispatch(
            &mut service,
            Some(&tasks),
            HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::ApplyTakeover(
                    ApplyTakeoverRequest {
                        request_id: "hdr-1".to_owned(),
                        expires_at_ms: 9_999,
                    },
                )),
            },
            ipc_contracts::IPC_PROTOCOL_VERSION_V8,
        );
        assert_eq!(error_code(&response), BusinessErrorCode::NotFound as i32);
    }

    #[test]
    fn takeover_decide_reaches_service_on_v8() {
        let db = TestDb::new();
        let store = storage_sqlite::SqliteStore::open(&db.0).unwrap();
        let mut service = HumanDecisionService::attach(store, FixedClock(10));
        let response = dispatch(
            &mut service,
            None,
            decide_request(HumanContinuationDecision::Takeover as i32),
            8,
        );
        assert_eq!(error_code(&response), BusinessErrorCode::NotFound as i32);
    }
}
