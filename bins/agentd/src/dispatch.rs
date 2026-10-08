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

use crate::execution::ExecutionComposition;
use app_services::{ServiceError, TaskService};
use evaluation::CandidateSelector;
use ipc_contracts::proto::{
    task_authoring_core_request, task_authoring_core_response, task_core_request,
    task_core_response, AddDependencyResponse, BusinessError, BusinessErrorCode,
    CreatePlanningTaskResponse, CreateProjectResponse, CreateTaskResponse, GetProjectResponse,
    GetTaskResponse, LifecycleResponse, ListDependenciesResponse, ListProjectsResponse,
    ListTasksResponse, PlanningTaskDto, ProjectDto, TaskAuthoringCoreRequest,
    TaskAuthoringCoreResponse, TaskCoreRequest, TaskCoreResponse, TaskDto, TaskLifecycleOp,
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
    composition: Option<&ExecutionComposition>,
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
        Some(task_core_request::Payload::Lifecycle(req)) => {
            dispatch_lifecycle(service, req, composition)
        }
        None => internal("empty business request"),
    }
}

/// Routing shape check: any required candidate-selector requirement routes
/// the task through the candidate-input producer, whose evidence the
/// operator path cannot revalidate (TASK-0151 finding 13). Shape-only on
/// purpose — over-matching refuses more operator completions, which is
/// the fail-closed direction; evaluation-time reconciliation stays in the
/// producer (`CandidateSelector::from_requirement_under_policy`).
fn task_is_candidate_routed(task: &Task) -> bool {
    task.evidence_requirements()
        .iter()
        .filter(|requirement| requirement.required)
        .any(|requirement| CandidateSelector::from_requirement(requirement).is_ok())
}

fn dispatch_lifecycle<C: app_services::Clock>(
    service: &mut TaskService<C>,
    req: ipc_contracts::proto::LifecycleRequest,
    composition: Option<&ExecutionComposition>,
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
            // TASK-0151 finding 13: the operator path never revalidates
            // live candidate evidence, so candidate-input tasks complete
            // exclusively through the production cycle (which revalidates
            // before `verification_succeeded`). Refuse here, before the
            // service call. A store failure reading the task fails closed
            // through the service error mapping.
            match service.get_task(&task_id) {
                Err(error) => return error_response(error),
                Ok(Some(task)) if task_is_candidate_routed(&task) => {
                    return invalid_argument(
                        "candidate-input tasks complete only through the production cycle",
                    );
                }
                Ok(_) => {}
            }
            // CV-7 current policy, derived from the same composition the
            // planner stamped with. Derivation failure (or a control-only
            // daemon, where nothing stamps) yields `None`, which fails
            // closed for policy-derived requirements and stays neutral
            // for authored ones.
            let current_policy =
                composition.and_then(|c| c.planning_policy_fingerprint(task_id.as_str()).ok());
            service.verification_succeeded_under_policy(
                &task_id,
                req.reason,
                current_policy.as_deref(),
            )
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

/// Dispatches one protocol-v6 Task authoring request. The wire carries only
/// client intent; `TaskService` validates DRAFT creation, bounds and G7
/// planning derivability and mints the `task.created` evidence. The
/// response echoes the committed canonical Task, never client values.
pub fn dispatch_task_authoring<C: app_services::Clock>(
    service: &mut TaskService<C>,
    request: TaskAuthoringCoreRequest,
) -> TaskAuthoringCoreResponse {
    let authoring_error = |error: ServiceError| {
        let (code, message) = map_error(&error);
        TaskAuthoringCoreResponse {
            payload: Some(task_authoring_core_response::Payload::Error(
                BusinessError {
                    code: code as i32,
                    message,
                },
            )),
        }
    };
    let Some(task_authoring_core_request::Payload::CreatePlanningTask(req)) = request.payload
    else {
        return TaskAuthoringCoreResponse {
            payload: Some(task_authoring_core_response::Payload::Error(
                BusinessError {
                    code: BusinessErrorCode::InvalidArgument as i32,
                    message: "empty task authoring request".to_owned(),
                },
            )),
        };
    };
    let ids = TaskId::new(req.id).and_then(|id| Ok((id, ProjectId::new(req.project_id)?)));
    let (id, project_id) = match ids {
        Ok(ids) => ids,
        Err(error) => return authoring_error(ServiceError::Domain(error)),
    };
    match service.create_planning_task(
        id,
        project_id,
        req.objective,
        req.allowed_paths,
        req.acceptance_criteria,
        req.baseline_risk,
    ) {
        Ok((task, _)) => {
            let facts = task.snapshot();
            TaskAuthoringCoreResponse {
                payload: Some(task_authoring_core_response::Payload::CreatePlanningTask(
                    CreatePlanningTaskResponse {
                        task: Some(PlanningTaskDto {
                            task: Some(task_dto(&task)),
                            allowed_paths: facts.allowed_paths,
                            acceptance_criteria: facts.acceptance,
                            baseline_risk: facts.risk.unwrap_or_default(),
                        }),
                    },
                )),
            }
        }
        Err(error) => authoring_error(error),
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

/// Typed unavailable response for control-only compositions (no database).
#[must_use]
pub fn task_authoring_unavailable() -> TaskAuthoringCoreResponse {
    TaskAuthoringCoreResponse {
        payload: Some(task_authoring_core_response::Payload::Error(
            BusinessError {
                code: BusinessErrorCode::Internal as i32,
                message: "business service unavailable".to_owned(),
            },
        )),
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
            StorageError::PlanningRunStale {
                branch,
                expected_generation,
                found_generation,
            } => (
                BusinessErrorCode::StaleState,
                format!(
                    "planning run for branch {} changed concurrently: expected generation {}, found {}",
                    truncate_id(branch),
                    expected_generation,
                    found_generation
                ),
            ),
            StorageError::PlanningInputStale { branch, .. } => (
                BusinessErrorCode::StaleState,
                format!(
                    "planning input for branch {} changed concurrently",
                    truncate_id(branch)
                ),
            ),
            StorageError::HumanDecisionStale { request_id, .. } => (
                BusinessErrorCode::StaleState,
                format!(
                    "human decision {} is stale; refresh the canonical request",
                    truncate_id(request_id)
                ),
            ),
            StorageError::HumanDecisionState { request_id, .. } => (
                BusinessErrorCode::Conflict,
                format!(
                    "human decision {} is already resolved, consumed, or superseded",
                    truncate_id(request_id)
                ),
            ),
            StorageError::TakeoverAnchorMismatch { record_id, .. } => (
                BusinessErrorCode::StaleState,
                format!(
                    "takeover {} workspace anchor changed concurrently; reconcile and retry",
                    truncate_id(record_id)
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
    use app_services::{FixedClock, PresenceAuthority};
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

    fn lifecycle_request(task_id: &str, op: TaskLifecycleOp, reason: &str) -> TaskCoreRequest {
        TaskCoreRequest {
            payload: Some(task_core_request::Payload::Lifecycle(
                ipc_contracts::proto::LifecycleRequest {
                    task_id: task_id.to_owned(),
                    op: op as i32,
                    reason: reason.to_owned(),
                },
            )),
        }
    }

    #[test]
    fn operator_ipc_cannot_bind_or_widen_requirement_params() {
        // TASK-0160 (IMPL-C): lifecycle IPC carries no binding fields
        // and no proof minter, so no sequence of operator requests —
        // even with attacker-shaped reason text — binds params or
        // widens an unbound requirement. Binding happens only through
        // the presence-authorized control-surface service.
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "P"), None);
        let mut task = Task::draft(
            TaskId::new("T-1").unwrap(),
            ProjectId::new("P-1").unwrap(),
            "objective",
        )
        .unwrap();
        task.add_acceptance("criterion-1").unwrap();
        task.define_evidence_requirements(vec![task_domain::EvidenceRequirement::new(
            "criterion-1",
            task_domain::RequirementKind::TestSuite,
            task_domain::RequirementKind::TestSuite.default_subject_binding(),
            Vec::new(),
            Some("policy:test-seed".to_owned()),
            true,
        )
        .unwrap()])
            .unwrap();
        service.create_task(&task).unwrap();
        for (op, reason) in [
            (TaskLifecycleOp::MarkReady, ""),
            (TaskLifecycleOp::Claim, ""),
            (TaskLifecycleOp::Start, ""),
            (
                TaskLifecycleOp::SubmitResult,
                "selector=candidate.evil.v1 path=/etc/passwd literal=x",
            ),
            (
                TaskLifecycleOp::RouteToReview,
                "candidate-binding:00 proof=stolen",
            ),
        ] {
            dispatch(&mut service, lifecycle_request("T-1", op, reason), None);
        }
        let task_id = TaskId::new("T-1").unwrap();
        let stored = service.get_task(&task_id).unwrap().unwrap();
        assert_eq!(stored.evidence_requirements().len(), 1);
        assert!(
            stored.evidence_requirements()[0].params.is_empty(),
            "operator IPC must never bind requirement params"
        );
    }

    #[test]
    fn create_and_get_project_round_trip() {
        let mut service = service();
        let created = dispatch(&mut service, create_project_request("P-1", "Runlith"), None);
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
            None,
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
            None,
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
        dispatch(&mut service, create_project_request("P-1", "One"), None);
        let again = dispatch(&mut service, create_project_request("P-1", "Two"), None);
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
        dispatch(&mut service, create_project_request("P-1", "P"), None);
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
            None,
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
            None,
        );
        assert!(matches!(
            unknown.payload,
            Some(task_core_response::Payload::Error(_))
        ));
        let empty = dispatch(&mut service, TaskCoreRequest { payload: None }, None);
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
    fn planning_fences_map_to_stale_state() {
        let (run_code, _) = map_error(&ServiceError::Storage(StorageError::PlanningRunStale {
            branch: "branch-a".to_owned(),
            expected_generation: 1,
            found_generation: 2,
        }));
        assert_eq!(run_code, BusinessErrorCode::StaleState);

        let (input_code, _) = map_error(&ServiceError::Storage(StorageError::PlanningInputStale {
            branch: "branch-a".to_owned(),
            expected_fingerprint: format!("sha256:{}", "0".repeat(64)),
        }));
        assert_eq!(input_code, BusinessErrorCode::StaleState);
    }

    #[test]
    fn lifecycle_success_returns_from_and_to_states() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "P"), None);
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
            None,
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

    fn planning_task_request(
        id: &str,
        allowed_paths: &[&str],
        acceptance: &[&str],
        risk: &str,
    ) -> TaskAuthoringCoreRequest {
        TaskAuthoringCoreRequest {
            payload: Some(task_authoring_core_request::Payload::CreatePlanningTask(
                ipc_contracts::proto::CreatePlanningTaskRequest {
                    id: id.to_owned(),
                    project_id: "P-1".to_owned(),
                    objective: "authored planning task".to_owned(),
                    allowed_paths: allowed_paths.iter().map(ToString::to_string).collect(),
                    acceptance_criteria: acceptance.iter().map(ToString::to_string).collect(),
                    baseline_risk: risk.to_owned(),
                },
            )),
        }
    }

    fn authoring_error(response: TaskAuthoringCoreResponse) -> BusinessError {
        match response.payload {
            Some(task_authoring_core_response::Payload::Error(error)) => error,
            other => panic!("expected typed authoring error, got {other:?}"),
        }
    }

    #[test]
    fn v6_authoring_commits_canonical_planning_metadata_with_server_evidence() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "Runlith"), None);
        let response = dispatch_task_authoring(
            &mut service,
            planning_task_request(
                "T-PLAN",
                &[" repo:src ", "repo:docs", "repo:src"],
                &["criterion-a", "criterion-a"],
                "medium",
            ),
        );
        let dto = match response.payload {
            Some(task_authoring_core_response::Payload::CreatePlanningTask(created)) => {
                created.task.expect("planning task dto")
            }
            other => panic!("expected created planning task, got {other:?}"),
        };
        assert_eq!(dto.task.as_ref().unwrap().state, "DRAFT");
        assert_eq!(dto.allowed_paths, ["repo:src", "repo:docs"]);
        assert_eq!(dto.acceptance_criteria, ["criterion-a"]);
        assert_eq!(dto.baseline_risk, "medium");

        let task_id = TaskId::new("T-PLAN").unwrap();
        let stored = service.get_task(&task_id).unwrap().expect("stored task");
        let facts = stored.snapshot();
        assert_eq!(facts.allowed_paths, dto.allowed_paths);
        assert_eq!(facts.acceptance, dto.acceptance_criteria);
        assert_eq!(facts.risk.as_deref(), Some("medium"));
        assert_eq!(stored.state(), TaskState::Draft);
        let events = service.list_events_for_task(&task_id).unwrap();
        assert_eq!(events.len(), 1, "authoring is exactly one creation fact");
        assert_eq!(events[0].fact.event_type, EventType::TaskCreated);
        assert_eq!(
            events[0].fact.provenance,
            "task-application-service/create_task"
        );
    }

    #[test]
    fn v6_authoring_refuses_invalid_metadata_without_persistence() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "Runlith"), None);
        for (id, paths, acceptance, risk) in [
            ("T-NO-SCOPE", &[][..], &["c"][..], "low"),
            ("T-NO-ACCEPTANCE", &["repo:src"][..], &[][..], "low"),
            ("T-NO-RISK", &["repo:src"][..], &["c"][..], ""),
            ("T-BAD-RISK", &["repo:src"][..], &["c"][..], "LOW"),
            ("T-ESCAPE", &["repo:../outside"][..], &["c"][..], "low"),
            ("T-ABSOLUTE", &["repo:/etc"][..], &["c"][..], "low"),
            ("T-NO-WORKSPACE", &["src"][..], &["c"][..], "low"),
            ("T-CONTROL", &["repo:src"][..], &["bad\u{1b}"][..], "low"),
        ] {
            let error = authoring_error(dispatch_task_authoring(
                &mut service,
                planning_task_request(id, paths, acceptance, risk),
            ));
            assert_eq!(
                error.code,
                BusinessErrorCode::InvalidArgument as i32,
                "{id}: {}",
                error.message
            );
            let task_id = TaskId::new(id).unwrap();
            assert!(service.get_task(&task_id).unwrap().is_none(), "{id}");
            assert!(service.list_events_for_task(&task_id).unwrap().is_empty());
        }

        let oversized: Vec<String> = (0..=task_domain::MAX_TASK_ALLOWED_PATHS)
            .map(|index| format!("repo:src/{index}"))
            .collect();
        let oversized: Vec<&str> = oversized.iter().map(String::as_str).collect();
        let error = authoring_error(dispatch_task_authoring(
            &mut service,
            planning_task_request("T-OVERSIZED", &oversized, &["c"], "low"),
        ));
        assert_eq!(error.code, BusinessErrorCode::InvalidArgument as i32);

        let missing_project = TaskAuthoringCoreRequest {
            payload: Some(task_authoring_core_request::Payload::CreatePlanningTask(
                ipc_contracts::proto::CreatePlanningTaskRequest {
                    id: "T-ORPHAN".to_owned(),
                    project_id: "P-MISSING".to_owned(),
                    objective: "orphan".to_owned(),
                    allowed_paths: vec!["repo:src".to_owned()],
                    acceptance_criteria: vec!["c".to_owned()],
                    baseline_risk: "low".to_owned(),
                },
            )),
        };
        authoring_error(dispatch_task_authoring(&mut service, missing_project));
        assert!(service
            .get_task(&TaskId::new("T-ORPHAN").unwrap())
            .unwrap()
            .is_none());

        dispatch_task_authoring(
            &mut service,
            planning_task_request("T-ONCE", &["repo:src"], &["c"], "low"),
        );
        let duplicate = authoring_error(dispatch_task_authoring(
            &mut service,
            planning_task_request("T-ONCE", &["repo:other"], &["d"], "critical"),
        ));
        assert_eq!(duplicate.code, BusinessErrorCode::AlreadyExists as i32);
        let unchanged = service
            .get_task(&TaskId::new("T-ONCE").unwrap())
            .unwrap()
            .unwrap()
            .snapshot();
        assert_eq!(unchanged.allowed_paths, ["repo:src"]);
        assert_eq!(unchanged.risk.as_deref(), Some("low"));

        let empty = authoring_error(dispatch_task_authoring(
            &mut service,
            TaskAuthoringCoreRequest { payload: None },
        ));
        assert_eq!(empty.code, BusinessErrorCode::InvalidArgument as i32);
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

    // TASK-0151 finding 13: the operator IPC path never revalidates live
    // candidate evidence, so it must refuse candidate-input tasks outright;
    // only the production cycle (which revalidates before completing) may
    // complete them.
    fn candidate_requirement_for_dispatch() -> task_domain::EvidenceRequirement {
        let catalog = policy::CandidateSelectorCatalogRevision::new(
            1,
            policy::CandidateSelectorCatalog::v1(),
        )
        .unwrap();
        let unbound = task_domain::EvidenceRequirement::new(
            "criterion-candidate",
            task_domain::RequirementKind::TestSuite,
            task_domain::RequirementKind::TestSuite.default_subject_binding(),
            Vec::new(),
            Some("policy:test-seed".to_owned()),
            true,
        )
        .unwrap();
        let proposed = app_services::ProposedCandidateBinding {
            selector: "candidate.file-contains.v1".to_owned(),
            path: "src/example.rs".to_owned(),
            literal: "fn accepted()".to_owned(),
        };
        let policy_fingerprint = format!("sha256:{}", "7".repeat(64));
        let proof = app_services::TestPresenceAuthority::new(1_700_000_000_000)
            .prove(
                &app_services::binding_operation(
                    "criterion-candidate",
                    &proposed.selector,
                    &proposed.path,
                ),
                &app_services::binding_input_digest(
                    "criterion-candidate",
                    &proposed.selector,
                    &proposed.path,
                    &proposed.literal,
                    &policy_fingerprint,
                ),
            )
            .unwrap();
        app_services::bind_candidate_input_params(
            &catalog,
            &unbound,
            &proposed,
            &proof,
            &policy_fingerprint,
            1_700_000_000_000,
        )
        .unwrap()
        .requirement
    }

    fn verify_succeed_request(task_id: &str) -> TaskCoreRequest {
        TaskCoreRequest {
            payload: Some(task_core_request::Payload::Lifecycle(
                ipc_contracts::proto::LifecycleRequest {
                    task_id: task_id.to_owned(),
                    op: TaskLifecycleOp::VerificationSucceeded as i32,
                    reason: "operator close".to_owned(),
                },
            )),
        }
    }

    fn drive_to_verifying(service: &mut TaskService<FixedClock>, task_id: &TaskId) {
        service.mark_ready(task_id).unwrap();
        service.claim(task_id).unwrap();
        service.start(task_id).unwrap();
        service.submit_result(task_id, "summary").unwrap();
        service.route_to_review(task_id).unwrap();
        service.begin_verification(task_id).unwrap();
        assert_eq!(
            service.get_task(task_id).unwrap().unwrap().state(),
            TaskState::Verifying
        );
    }

    #[test]
    fn operator_verification_succeeded_refuses_candidate_tasks() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "P"), None);
        let task_id = TaskId::new("T-CANDIDATE").unwrap();
        let mut task = Task::draft(
            task_id.clone(),
            ProjectId::new("P-1").unwrap(),
            "candidate objective",
        )
        .unwrap();
        task.add_acceptance("criterion-candidate").unwrap();
        task.define_evidence_requirements(vec![candidate_requirement_for_dispatch()])
            .unwrap();
        service.create_task(&task).unwrap();
        drive_to_verifying(&mut service, &task_id);

        let response = dispatch(&mut service, verify_succeed_request("T-CANDIDATE"), None);
        match response.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert_eq!(error.code, BusinessErrorCode::InvalidArgument as i32);
                assert!(
                    error.message.contains("candidate-input"),
                    "refusal must name the candidate-input path, got {:?}",
                    error.message
                );
            }
            other => panic!("expected candidate-input refusal, got {other:?}"),
        }
        assert_eq!(
            service.get_task(&task_id).unwrap().unwrap().state(),
            TaskState::Verifying,
            "refused operator completion must not mutate the task"
        );
    }

    #[test]
    fn operator_verification_succeeded_still_routes_plain_tasks_to_service() {
        let mut service = service();
        dispatch(&mut service, create_project_request("P-1", "P"), None);
        let task_id = TaskId::new("T-PLAIN").unwrap();
        let mut task = Task::draft(
            task_id.clone(),
            ProjectId::new("P-1").unwrap(),
            "plain objective",
        )
        .unwrap();
        task.add_acceptance("criterion-plain").unwrap();
        service.create_task(&task).unwrap();
        drive_to_verifying(&mut service, &task_id);

        let response = dispatch(&mut service, verify_succeed_request("T-PLAIN"), None);
        match response.payload {
            Some(task_core_response::Payload::Error(error)) => {
                assert!(
                    !error.message.contains("candidate-input"),
                    "plain tasks must reach the service, got {:?}",
                    error.message
                );
            }
            other => panic!("expected the service verdict refusal, got {other:?}"),
        }
    }
}
