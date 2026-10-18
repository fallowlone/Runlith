//! Routine IPC dispatch (TASK-0193, G11): RoutineCoreRequest in,
//! RoutineCoreResponse out over BackgroundService (ADR-0045).
use app_services::background_service::{BackgroundService, DefineRequest};
use background::model::{
    BackgroundError, CapabilityScope, RoutineDefinition, RoutineId, RoutineRun,
};
use background::port::RoutineStorePort;
use ipc_contracts::proto::{
    routine_core_request, routine_core_response, RoutineCoreRequest, RoutineCoreResponse,
    RoutineErrorResponse, RoutineListResponse, RoutineRecordResponse, RoutineRunResponse,
};

fn error_response(code: &str, detail: &str) -> RoutineCoreResponse {
    RoutineCoreResponse {
        payload: Some(routine_core_response::Payload::Error(
            RoutineErrorResponse {
                code: code.to_owned(),
                detail: detail.chars().take(2048).collect(),
            },
        )),
    }
}

pub fn unavailable() -> RoutineCoreResponse {
    error_response("unavailable", "background composition is not available")
}

fn map_error(error: &BackgroundError) -> RoutineCoreResponse {
    let code = match error {
        BackgroundError::InvalidInput(_) => "invalid_input",
        BackgroundError::UnknownKind(_) => "unknown_kind",
        BackgroundError::SelfAuthorization(_) => "self_authorization",
        BackgroundError::Unauthorized(_) => "unauthorized",
        BackgroundError::NotDue(_) => "not_due",
        BackgroundError::BudgetExhausted(_) => "budget_exhausted",
        BackgroundError::Draining(_) => "draining",
        BackgroundError::Conflict(_) => "conflict",
        BackgroundError::NotFound(_) => "not_found",
        BackgroundError::BackendFailure { .. } => "backend_failure",
    };
    error_response(code, &error.to_string())
}

fn record_response(d: &RoutineDefinition) -> RoutineRecordResponse {
    RoutineRecordResponse {
        routine_id: d.id.to_string(),
        name: d.name.clone(),
        objective: d.objective.clone(),
        interval_seconds: d.interval_seconds,
        capability_scope: d.capability_scope.as_str().to_owned(),
        budget_max_runs: d.budget_max_runs,
        author_ref: d.author_ref.clone(),
        authorized_by: d.authorized_by.clone().unwrap_or_default(),
        state: d.state.as_str().to_owned(),
        created_at: d.created_at,
        next_run_at: d.next_run_at,
        runs_completed: d.runs_completed,
    }
}

fn run_response(r: &RoutineRun) -> RoutineRunResponse {
    RoutineRunResponse {
        run_id: r.id.to_string(),
        routine_id: r.routine_id.to_string(),
        status: r.status.as_str().to_owned(),
        started_at: r.started_at,
        finished_at: r.finished_at.unwrap_or(0),
        evidence_ref: r.evidence_ref.clone().unwrap_or_default(),
    }
}

fn parse_id(raw: &str) -> Result<RoutineId, BackgroundError> {
    RoutineId::new(raw.to_owned())
}

pub fn dispatch<S: RoutineStorePort>(
    service: &mut BackgroundService<S>,
    request: RoutineCoreRequest,
) -> RoutineCoreResponse {
    match request.payload {
        Some(routine_core_request::Payload::Define(define)) => {
            let scope = match CapabilityScope::parse(&define.capability_scope) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.define(
                DefineRequest {
                    name: define.name,
                    objective: define.objective,
                    interval_seconds: define.interval_seconds,
                    capability_scope: scope,
                    budget_max_runs: define.budget_max_runs,
                    author_ref: define.author_ref,
                },
                define.now_millis,
            ) {
                Ok(def) => RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::Record(record_response(
                        &def,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(routine_core_request::Payload::Authorize(auth)) => {
            let id = match parse_id(&auth.routine_id) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.authorize(&id, &auth.authorizer_ref, auth.now_millis) {
                Ok(def) => RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::Record(record_response(
                        &def,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(routine_core_request::Payload::Disable(req)) => {
            let id = match parse_id(&req.routine_id) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.disable(&id) {
                Ok(def) => RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::Record(record_response(
                        &def,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(routine_core_request::Payload::Get(req)) => {
            let id = match parse_id(&req.routine_id) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.get(&id) {
                Ok(def) => RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::Record(record_response(
                        &def,
                    ))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(routine_core_request::Payload::Run(req)) => {
            let id = match parse_id(&req.routine_id) {
                Ok(v) => v,
                Err(e) => return map_error(&e),
            };
            match service.run_now(&id, req.now_millis, &req.evidence_ref) {
                Ok(run) => RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::Run(run_response(&run))),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(routine_core_request::Payload::Tick(req)) => {
            match service.tick(req.now_millis, &req.evidence_ref) {
                Ok(runs) => RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::List(RoutineListResponse {
                        routines: Vec::new(),
                        runs: runs.iter().map(run_response).collect(),
                    })),
                },
                Err(e) => map_error(&e),
            }
        }
        Some(routine_core_request::Payload::List(_)) => match service.list() {
            Ok(defs) => {
                let mut runs = Vec::new();
                for def in &defs {
                    match service.list_runs(&def.id) {
                        Ok(mut r) => runs.append(&mut r),
                        Err(e) => return map_error(&e),
                    }
                }
                RoutineCoreResponse {
                    payload: Some(routine_core_response::Payload::List(RoutineListResponse {
                        routines: defs.iter().map(record_response).collect(),
                        runs: runs.iter().map(run_response).collect(),
                    })),
                }
            }
            Err(e) => map_error(&e),
        },
        None => error_response("invalid_input", "missing routine payload"),
    }
}
