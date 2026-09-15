//! G2 business dispatcher: one bounded typed match from `TaskCoreRequest`
//! to `TaskCoreResponse` through the trusted application services.
//!
//! No generic reflection, no dynamic method strings, no raw storage
//! access: every arm calls a `TaskService` method (which owns validation,
//! journal evidence, and atomicity) and maps the result onto the wire
//! DTOs or the typed `BusinessError` codes accepted in ADR-0021 §11.
//! The dispatcher creates no events itself — exactly-once commit
//! evidence belongs to the application service.
//!
//! Error sanitization: client-facing messages carry only static text plus
//! already-validated identifiers and state names, truncated defensively.
//! Raw SQLite errors, filesystem paths, SQL strings, Rust debug output,
//! and backtraces never cross this boundary (internal logs may retain
//! safe diagnostics).

use app_services::{ServiceError, TaskService};
use ipc_contracts::proto::{
    task_core_request, task_core_response, AddDependencyResponse, BusinessError, BusinessErrorCode,
    CreateProjectResponse, CreateTaskResponse, GetProjectResponse, GetTaskResponse,
    LifecycleResponse, ListDependenciesResponse, ListProjectsResponse, ListTasksResponse,
    ProjectDto, TaskCoreRequest, TaskCoreResponse, TaskDto, TaskLifecycleOp,
};
use storage_sqlite::StorageError;
use task_domain::{DomainError, Project, ProjectId, Task, TaskId};

/// Maximum client-facing error message length in bytes. Longer details
/// are truncated: wire errors stay bounded and sanitized.
const MAX_ERROR_MESSAGE_LEN: usize = 512;

/// Maximum client-supplied `reason` length enforced before the service
/// layer (mirrors `ipc_contracts::MAX_REASON_LEN` at the dispatch
/// boundary; the trusted service re-validates non-blank where required).
const MAX_REASON_LEN: usize = 4096;

/// Dispatches one decoded business request through the trusted service.
/// Total function for well-formed requests: every path yields exactly
/// one success or typed-error response, never a transport failure.
pub fn dispatch<C: app_services::Clock>(
    service: &mut TaskService<C>,
    request: TaskCoreRequest,
) -> TaskCoreResponse {
    match request.payload {
        Some(task_core_request::Payload::CreateProject(req)) => {
            let project = match domain_project(&req.id, &req.name) {
                Ok(project) => project,
                Err(error) => return error_response(error),
            };
            match service.create_project(&project) {
                Ok(_) => success(task_core_response::Payload::CreateProject(
                    CreateProjectResponse {
                        project: Some(project_dto(&project)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::GetProject(req)) => {
            let id = match ProjectId::new(req.id) {
                Ok(id) => id,
                Err(error) => return error_response(ServiceError::Domain(error)),
            };
            match service.get_project(&id) {
                Ok(Some(project)) => success(task_core_response::Payload::GetProject(
                    GetProjectResponse {
                        project: Some(project_dto(&project)),
                    },
                )),
                Ok(None) => not_found(format!("project {} does not exist", id)),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::ListProjects(_)) => match service.list_projects() {
            Ok(projects) => success(task_core_response::Payload::ListProjects(
                ListProjectsResponse {
                    projects: projects.iter().map(project_dto).collect(),
                },
            )),
            Err(error) => error_response(error),
        },
        Some(task_core_request::Payload::CreateTask(req)) => {
            let task = match domain_task(&req.id, &req.project_id, &req.objective) {
                Ok(task) => task,
                Err(error) => return error_response(error),
            };
            match service.create_task(&task) {
                Ok(_) => success(task_core_response::Payload::CreateTask(
                    CreateTaskResponse {
                        task: Some(task_dto(&task)),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::GetTask(req)) => {
            let id = match TaskId::new(req.id) {
                Ok(id) => id,
                Err(error) => return error_response(ServiceError::Domain(error)),
            };
            match service.get_task(&id) {
                Ok(Some(task)) => success(task_core_response::Payload::GetTask(GetTaskResponse {
                    task: Some(task_dto(&task)),
                })),
                Ok(None) => not_found(format!("task {} does not exist", id)),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::ListTasksForProject(req)) => {
            let id = match ProjectId::new(req.project_id) {
                Ok(id) => id,
                Err(error) => return error_response(ServiceError::Domain(error)),
            };
            match service.list_tasks_for_project(&id) {
                Ok(tasks) => success(task_core_response::Payload::ListTasks(ListTasksResponse {
                    tasks: tasks.iter().map(task_dto).collect(),
                })),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::AddDependency(req)) => {
            let (task_id, depends_on_id) =
                match (TaskId::new(req.task_id), TaskId::new(req.depends_on_id)) {
                    (Ok(a), Ok(b)) => (a, b),
                    _ => {
                        return invalid_argument(
                            "dependency ids must be non-blank bounded identifiers",
                        );
                    }
                };
            match service.add_dependency(&task_id, &depends_on_id) {
                Ok(_) => success(task_core_response::Payload::AddDependency(
                    AddDependencyResponse {},
                )),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::ListDependencies(req)) => {
            let id = match TaskId::new(req.task_id) {
                Ok(id) => id,
                Err(error) => return error_response(ServiceError::Domain(error)),
            };
            match service.list_dependencies(&id) {
                Ok(ids) => success(task_core_response::Payload::ListDependencies(
                    ListDependenciesResponse {
                        depends_on_ids: ids.iter().map(|id| id.to_string()).collect(),
                    },
                )),
                Err(error) => error_response(error),
            }
        }
        Some(task_core_request::Payload::Lifecycle(req)) => dispatch_lifecycle(service, req),
        None => internal("empty business request"),
    }
}

fn dispatch_lifecycle<C: app_services::Clock>(
    service: &mut TaskService<C>,
    req: ipc_contracts::proto::LifecycleRequest,
) -> TaskCoreResponse {
    use std::convert::TryFrom;
    let task_id = match TaskId::new(req.task_id) {
        Ok(id) => id,
        Err(error) => return error_response(ServiceError::Domain(error)),
    };
    if req.reason.len() > MAX_REASON_LEN {
        return invalid_argument("reason exceeds the bounded length");
    }
    let op = match TaskLifecycleOp::try_from(req.op) {
        Ok(op) => op,
        Err(_) => return invalid_argument("unknown lifecycle operation"),
    };
    let applied = match op {
        TaskLifecycleOp::Unspecified => return invalid_argument("unknown lifecycle operation"),
        TaskLifecycleOp::MarkReady => service.mark_ready(&task_id),
        TaskLifecycleOp::Claim => service.claim(&task_id),
        TaskLifecycleOp::Start => service.start(&task_id),
        TaskLifecycleOp::Block => service.block(&task_id, req.reason),
        TaskLifecycleOp::SubmitResult => service.submit_result(&task_id, req.reason),
        TaskLifecycleOp::RouteToReview => service.route_to_review(&task_id),
        TaskLifecycleOp::BeginVerification => service.begin_verification(&task_id),
        TaskLifecycleOp::VerificationFailed => service.verification_failed(&task_id, req.reason),
        TaskLifecycleOp::VerificationSucceeded => {
            service.verification_succeeded(&task_id, req.reason)
        }
    };
    match applied {
        Ok(transition) => {
            let from = transition
                .event
                .fact
                .from_state
                .map(|state| state.as_str().to_owned())
                .unwrap_or_default();
            let to = transition
                .event
                .fact
                .to_state
                .map(|state| state.as_str().to_owned())
                .unwrap_or_default();
            success(task_core_response::Payload::Lifecycle(LifecycleResponse {
                task: Some(task_dto(&transition.task)),
                from_state: from,
                to_state: to,
            }))
        }
        Err(error) => error_response(error),
    }
}

fn domain_project(id: &str, name: &str) -> Result<Project, ServiceError> {
    Ok(Project::new(ProjectId::new(id)?, name)?)
}

fn domain_task(id: &str, project_id: &str, objective: &str) -> Result<Task, ServiceError> {
    Ok(Task::draft(
        TaskId::new(id)?,
        ProjectId::new(project_id)?,
        objective,
    )?)
}

fn project_dto(project: &Project) -> ProjectDto {
    ProjectDto {
        id: project.id().to_string(),
        name: project.name().to_owned(),
    }
}

fn task_dto(task: &Task) -> TaskDto {
    TaskDto {
        id: task.id().to_string(),
        project_id: task.project_id().to_string(),
        objective: task.objective().to_owned(),
        state: task.state().as_str().to_owned(),
    }
}

/// Typed unavailable response for control-only daemon compositions
/// (no database): the version gate passed, but no trusted service exists
/// to handle the request. Production daemons always compose a database;
/// only bespoke control-only compositions observe this.
#[must_use]
pub fn unavailable() -> TaskCoreResponse {
    TaskCoreResponse {
        payload: Some(task_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::Internal as i32,
            message: "business service unavailable".to_owned(),
        })),
    }
}

fn success(payload: task_core_response::Payload) -> TaskCoreResponse {
    TaskCoreResponse {
        payload: Some(payload),
    }
}

fn error_response(error: ServiceError) -> TaskCoreResponse {
    let (code, message) = map_error(&error);
    TaskCoreResponse {
        payload: Some(task_core_response::Payload::Error(BusinessError {
            code: code as i32,
            message,
        })),
    }
}

fn invalid_argument(message: &str) -> TaskCoreResponse {
    TaskCoreResponse {
        payload: Some(task_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::InvalidArgument as i32,
            message: message.to_owned(),
        })),
    }
}

fn not_found(message: String) -> TaskCoreResponse {
    // `message` is static text plus an already-validated identifier.
    TaskCoreResponse {
        payload: Some(task_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::NotFound as i32,
            message: truncate(&message),
        })),
    }
}

fn internal(message: &str) -> TaskCoreResponse {
    TaskCoreResponse {
        payload: Some(task_core_response::Payload::Error(BusinessError {
            code: BusinessErrorCode::Internal as i32,
            message: message.to_owned(),
        })),
    }
}

/// Maps service failures onto the wire taxonomy (ADR-0021 §11). Details
/// that embed validated identifiers or state names are preserved
/// (truncated); everything that could carry SQL, paths, stored garbage,
/// or debug output is replaced with static text. Shared with the G5
/// execution dispatcher, which reports the same taxonomy.
pub(crate) fn map_error(error: &ServiceError) -> (BusinessErrorCode, String) {
    match error {
        ServiceError::Domain(domain) => match domain {
            DomainError::InvalidInput(detail) => {
                (BusinessErrorCode::InvalidArgument, truncate(detail))
            }
            DomainError::InvalidTransition(detail) => {
                (BusinessErrorCode::InvalidTransition, truncate(detail))
            }
            DomainError::SelfDependency(detail)
            | DomainError::DuplicateDependency(detail)
            | DomainError::DependencyCycle(detail)
            | DomainError::ProjectMismatch(detail) => {
                (BusinessErrorCode::Conflict, truncate(detail))
            }
        },
        ServiceError::Storage(storage) => match storage {
            StorageError::NotFound(detail) => (BusinessErrorCode::NotFound, truncate(detail)),
            StorageError::Duplicate(detail) => (BusinessErrorCode::AlreadyExists, truncate(detail)),
            StorageError::InvalidGraph(detail) => (BusinessErrorCode::Conflict, truncate(detail)),
            StorageError::ConstraintViolation(_) => (
                BusinessErrorCode::InvalidArgument,
                "request violates a storage constraint".to_owned(),
            ),
            StorageError::Stale {
                task,
                expected,
                found,
            } => (
                BusinessErrorCode::StaleState,
                format!(
                    "task {} changed concurrently: expected {}, found {}",
                    truncate_id(task),
                    truncate_id(expected),
                    truncate_id(found)
                ),
            ),
            StorageError::PlanStale {
                branch,
                expected,
                found,
            } => (
                BusinessErrorCode::StaleState,
                format!(
                    "plan branch {} changed concurrently: expected revision {}, found {}",
                    truncate_id(branch),
                    expected,
                    found
                ),
            ),
            StorageError::Corrupt(_)
            | StorageError::DomainReconstruction(_)
            | StorageError::UnsupportedSchemaVersion { .. } => (
                BusinessErrorCode::CorruptState,
                "stored data failed validation".to_owned(),
            ),
            StorageError::Sqlite(_) | StorageError::Migration(_) | StorageError::UnsafePath(_) => (
                BusinessErrorCode::Internal,
                "internal storage failure".to_owned(),
            ),
        },
        ServiceError::Policy(_) => (
            BusinessErrorCode::Conflict,
            "policy enforcement refused the request".to_owned(),
        ),
    }
}

/// Floors a byte bound to a UTF-8 char boundary so slicing an
/// externally influenced string never panics (TASK-0049 H1: validated
/// identifiers may be multibyte, and a raw `[..N]` cut can land
/// mid-char). Shared by this module and the execution dispatcher, which
/// reports the same taxonomy over the same identifier shapes.
pub(crate) fn floor_char_boundary(value: &str, max_bytes: usize) -> usize {
    let mut end = max_bytes.min(value.len());
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Defensive bound on client-facing detail: static text plus validated
/// identifiers only, never raw internals.
fn truncate(detail: &str) -> String {
    let mut message = detail.to_owned();
    let end = floor_char_boundary(&message, MAX_ERROR_MESSAGE_LEN);
    message.truncate(end);
    message
}

/// Tighter bound for identifiers and state names embedded in messages.
fn truncate_id(value: &str) -> String {
    const MAX_ID_MESSAGE_LEN: usize = 64;
    if value.len() > MAX_ID_MESSAGE_LEN {
        format!(
            "{}…",
            &value[..floor_char_boundary(value, MAX_ID_MESSAGE_LEN)]
        )
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_services::FixedClock;
    use storage_sqlite::EventType;
    use task_domain::TaskState;

    fn service() -> TaskService<FixedClock> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("agentd-dispatch-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test dir");
        let store = storage_sqlite::SqliteStore::open(dir.join("test.db")).expect("open test db");
        // Best-effort cleanup is handled by the OS temp dir; the store
        // owns its file for the test lifetime.
        TaskService::attach(store, FixedClock(1_704_067_200_000))
    }

    fn create_project_request(id: &str, name: &str) -> TaskCoreRequest {
        TaskCoreRequest {
            payload: Some(task_core_request::Payload::CreateProject(
                ipc_contracts::proto::CreateProjectRequest {
                    id: id.to_owned(),
                    name: name.to_owned(),
                },
            )),
        }
    }

    #[test]
    fn create_and_get_project_round_trip() {
        let mut service = service();
        let created = dispatch(&mut service, create_project_request("P-1", "Runlith"));
        match created.payload {
            Some(task_core_response::Payload::CreateProject(response)) => {
                let project = response.project.expect("project dto");
                assert_eq!(project.id, "P-1");
                assert_eq!(project.name, "Runlith");
            }
            other => panic!("expected create_project, got {other:?}"),
        }
        let get = dispatch(
            &mut service,
            TaskCoreRequest {
                payload: Some(task_core_request::Payload::GetProject(
                    ipc_contracts::proto::GetProjectRequest {
                        id: "P-1".to_owned(),
                    },
                )),
            },
        );
        assert!(matches!(
            get.payload,
            Some(task_core_response::Payload::GetProject(_))
        ));
        let missing = dispatch(
            &mut service,
            TaskCoreRequest {
                payload: Some(task_core_request::Payload::GetProject(
                    ipc_contracts::proto::GetProjectRequest {
                        id: "GHOST".to_owned(),
                    },
                )),
            },
        );
        match missing.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::NotFound as i32);
            }
            other => panic!("expected NOT_FOUND, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_create_maps_to_already_exists() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "One"));
        let again = dispatch(&mut service, create_project_request("P-1", "Two"));
        match again.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::AlreadyExists as i32);
                assert!(!error.message.is_empty());
            }
            other => panic!("expected ALREADY_EXISTS, got {other:?}"),
        }
    }

    #[test]
    fn invalid_lifecycle_maps_to_invalid_transition_without_mutation() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "P"));
        let task = Task::draft(
            TaskId::new("T-1").unwrap(),
            ProjectId::new("P-1").unwrap(),
            "objective",
        )
        .unwrap();
        service.create_task(&task).unwrap();
        let bad = dispatch(
            &mut service,
            TaskCoreRequest {
                payload: Some(task_core_request::Payload::Lifecycle(
                    ipc_contracts::proto::LifecycleRequest {
                        task_id: "T-1".to_owned(),
                        op: TaskLifecycleOp::VerificationSucceeded as i32,
                        reason: "self-report".to_owned(),
                    },
                )),
            },
        );
        match bad.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::InvalidTransition as i32);
            }
            other => panic!("expected INVALID_TRANSITION, got {other:?}"),
        }
        assert_eq!(
            service
                .get_task(&TaskId::new("T-1").unwrap())
                .unwrap()
                .unwrap()
                .state(),
            TaskState::Draft
        );
    }

    #[test]
    fn unknown_operation_and_empty_request_are_invalid_argument() {
        let mut service = service();
        let unknown = dispatch(
            &mut service,
            TaskCoreRequest {
                payload: Some(task_core_request::Payload::Lifecycle(
                    ipc_contracts::proto::LifecycleRequest {
                        task_id: "T-1".to_owned(),
                        op: 999,
                        reason: String::new(),
                    },
                )),
            },
        );
        assert!(matches!(
            unknown.payload,
            Some(task_core_response::Payload::Error(_))
        ));
        let empty = dispatch(&mut service, TaskCoreRequest { payload: None });
        match empty.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::Internal as i32);
            }
            other => panic!("expected INTERNAL, got {other:?}"),
        }
    }

    #[test]
    fn error_messages_carry_no_paths_or_sql() {
        // Raw sqlite errors must surface as static INTERNAL text. A
        // migration failure stands in for the whole Sqlite/Migration
        // family without needing the C-level error type in this crate.
        let response = error_response(ServiceError::Storage(StorageError::Migration(
            "0003_event_scope.sql failed: near \"SELECT\": syntax error at /tmp/x/state.sqlite3"
                .to_owned(),
        )));
        match response.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::Internal as i32);
                assert_eq!(error.message, "internal storage failure");
            }
            other => panic!("expected INTERNAL, got {other:?}"),
        }
        // Corrupt-state details (which may echo stored garbage) are static.
        let corrupt = error_response(ServiceError::Storage(StorageError::Corrupt(
            "unknown persisted task state 'EVIL'; path=/tmp/x; SELECT 1".to_owned(),
        )));
        match corrupt.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::CorruptState as i32);
                assert!(!error.message.contains("/tmp"));
                assert!(!error.message.contains("SELECT"));
            }
            other => panic!("expected CORRUPT_STATE, got {other:?}"),
        }
        let unsafe_path = error_response(ServiceError::Storage(StorageError::UnsafePath(
            "/tmp/attacker/state.sqlite3 is a symlink".to_owned(),
        )));
        match unsafe_path.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::Internal as i32);
                assert_eq!(error.message, "internal storage failure");
            }
            other => panic!("expected INTERNAL, got {other:?}"),
        }
    }

    #[test]
    fn lifecycle_success_returns_from_and_to_states() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "P"));
        let task = Task::draft(
            TaskId::new("T-1").unwrap(),
            ProjectId::new("P-1").unwrap(),
            "objective",
        )
        .unwrap();
        service.create_task(&task).unwrap();
        let response = dispatch(
            &mut service,
            TaskCoreRequest {
                payload: Some(task_core_request::Payload::Lifecycle(
                    ipc_contracts::proto::LifecycleRequest {
                        task_id: "T-1".to_owned(),
                        op: TaskLifecycleOp::MarkReady as i32,
                        reason: String::new(),
                    },
                )),
            },
        );
        match response.payload {
            Some(task_core_response::Payload::Lifecycle(lifecycle)) => {
                assert_eq!(lifecycle.from_state, "DRAFT");
                assert_eq!(lifecycle.to_state, "READY");
                assert_eq!(lifecycle.task.expect("task").state, "READY");
            }
            other => panic!("expected lifecycle, got {other:?}"),
        }
        // Exactly one event for the one mutation: the dispatcher creates
        // no events of its own.
        let history = service
            .list_events_for_task(&TaskId::new("T-1").unwrap())
            .unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].fact.event_type, EventType::TaskCreated);
        assert_eq!(history[1].fact.event_type, EventType::MarkedReady);
    }

    // -- H1 multibyte truncation safety (TASK-0049) --------------------------
    //
    // Client-facing truncation must cut on char boundaries: validated
    // identifiers and details may be multibyte, and a raw byte cut
    // panics the connection handler.

    #[test]
    fn floor_char_boundary_never_splits_a_char() {
        assert_eq!(floor_char_boundary("abcdef", 4), 4);
        assert_eq!(floor_char_boundary("abc", 99), 3);
        assert_eq!(floor_char_boundary("", 64), 0);
        // 2/3/4-byte chars straddling the bound floor to the char start.
        assert_eq!(floor_char_boundary(&format!("{}é", "A".repeat(63)), 64), 63);
        assert_eq!(floor_char_boundary(&format!("{}€", "A".repeat(62)), 64), 62);
        assert_eq!(
            floor_char_boundary(&format!("{}😀", "A".repeat(63)), 64),
            63
        );
        for (input, max) in [
            (format!("{}é{}", "A".repeat(63), "B".repeat(10)), 64),
            (format!("{}€{}", "A".repeat(62), "B".repeat(10)), 64),
            (format!("{}😀{}", "A".repeat(63), "B".repeat(10)), 64),
            ("é".repeat(300), MAX_ERROR_MESSAGE_LEN),
        ] {
            let end = floor_char_boundary(&input, max);
            assert!(input.is_char_boundary(end), "cut must be a boundary");
            assert!(end <= max, "cut must respect the bound");
        }
    }

    #[test]
    fn truncate_and_truncate_id_are_multibyte_safe_and_bounded() {
        for id in [
            format!("E-{}", "x".repeat(100)),
            format!("{}é{}", "A".repeat(63), "B".repeat(10)),
            format!("{}€{}", "A".repeat(62), "B".repeat(10)),
            format!("{}😀{}", "A".repeat(63), "B".repeat(10)),
            format!("{}é€😀{}", "A".repeat(61), "B".repeat(10)),
        ] {
            let cut = truncate_id(&id);
            assert!(cut.ends_with('…'), "long id must truncate, got {cut:?}");
            let kept = cut.strip_suffix('…').expect("marker");
            assert!(kept.len() <= 64, "kept part must fit, got {cut:?}");
            assert!(id.starts_with(kept), "must keep a prefix, got {cut:?}");
        }
        assert_eq!(truncate_id("E-short"), "E-short");

        let long_detail = "é".repeat(400);
        let cut = truncate(&long_detail);
        assert!(
            cut.len() <= MAX_ERROR_MESSAGE_LEN,
            "detail must stay bounded, got {} bytes",
            cut.len()
        );
        assert!(long_detail.starts_with(&cut), "must keep a prefix");
        assert_eq!(truncate("short"), "short");
    }
}
