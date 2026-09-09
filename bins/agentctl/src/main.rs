//! `agentctl` entry point: G1 lifecycle (`start`, `ping`, `shutdown`)
//! plus G2 business commands (`project ...`, `task ...`).
//!
//! Thin shell over the client lifecycle in the library. Concise,
//! deterministic stdout/stderr; no protobuf/frame/security-descriptor dumps.
//! Business commands speak protocol v1 through the trusted daemon
//! dispatcher; lifecycle commands stay semantic (no set-state, no
//! complete/done shortcut). Exit codes are G1-local, not a permanent
//! standard.

use std::process::ExitCode;

use agentctl::{resolve_production_endpoint, spawn_sibling_daemon, ClientError};
use ipc_contracts::proto::{
    task_core_request, task_core_response, AddDependencyRequest, CreateProjectRequest,
    CreateTaskRequest, GetProjectRequest, GetTaskRequest, LifecycleRequest,
    ListDependenciesRequest, ListProjectsRequest, ListTasksForProjectRequest, TaskCoreRequest,
    TaskCoreResponse, TaskLifecycleOp,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("start") => cmd_start().await,
        Some("ping") => cmd_ping().await,
        Some("shutdown") => cmd_shutdown().await,
        Some("project") => cmd_project(args.collect()).await,
        Some("task") => cmd_task(args.collect()).await,
        _ => {
            eprintln!("usage: agentctl <start|ping|shutdown|project|task> ...");
            eprintln!("  agentctl project create <id> <name>");
            eprintln!("  agentctl project get <id>");
            eprintln!("  agentctl project list");
            eprintln!("  agentctl task create <id> <project-id> <objective>");
            eprintln!("  agentctl task get <id>");
            eprintln!("  agentctl task list <project-id>");
            eprintln!("  agentctl task dependency add <task-id> <depends-on-id>");
            eprintln!("  agentctl task dependency list <task-id>");
            eprintln!("  agentctl task <ready|claim|start|resume|block|submit|review|verify-begin|verify-fail|verify-succeed> <id> [reason]");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
async fn cmd_start() -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match agentctl::connect_or_start(&endpoint, spawn_sibling_daemon).await {
        Ok(session) => {
            let version = session.negotiated_version().unwrap_or(0);
            println!("agentctl: daemon ready (protocol version {version})");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_start() -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match agentctl::connect_or_start(&pipe_name, spawn_sibling_daemon).await {
        Ok(session) => {
            let version = session.negotiated_version().unwrap_or(0);
            println!("agentctl: daemon ready (protocol version {version})");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(unix)]
async fn cmd_ping() -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match agentctl::ping(&endpoint, spawn_sibling_daemon).await {
        Ok(()) => {
            println!("agentctl: pong");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_ping() -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match agentctl::ping(&pipe_name, spawn_sibling_daemon).await {
        Ok(()) => {
            println!("agentctl: pong");
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

#[cfg(unix)]
async fn cmd_shutdown() -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match agentctl::shutdown(&endpoint).await {
        Ok(()) => {
            println!("agentctl: shutdown complete");
            ExitCode::SUCCESS
        }
        Err(ClientError::NotRunning) => {
            eprintln!("agentctl: daemon not running");
            ExitCode::FAILURE
        }
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_shutdown() -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match agentctl::shutdown(&pipe_name).await {
        Ok(()) => {
            println!("agentctl: shutdown complete");
            ExitCode::SUCCESS
        }
        Err(ClientError::NotRunning) => {
            eprintln!("agentctl: daemon not running");
            ExitCode::FAILURE
        }
        Err(error) => fail(&error),
    }
}

fn fail(error: &ClientError) -> ExitCode {
    eprintln!("agentctl: failed: {error}");
    ExitCode::FAILURE
}

/// Runs one business request and prints the deterministic response.
/// Success payloads print to stdout; typed business errors print to
/// stderr; both are single-line-per-record and stable for tests.
#[cfg(unix)]
async fn run_business(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    request: TaskCoreRequest,
) -> ExitCode {
    match agentctl::business_request(endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => print_response(response),
        Err(error) => fail(&error),
    }
}

/// Runs one business request and prints the deterministic response.
#[cfg(windows)]
async fn run_business(pipe_name: &str, request: TaskCoreRequest) -> ExitCode {
    match agentctl::business_request(pipe_name, spawn_sibling_daemon, request).await {
        Ok(response) => print_response(response),
        Err(error) => fail(&error),
    }
}

fn print_response(response: TaskCoreResponse) -> ExitCode {
    match response.payload {
        Some(task_core_response::Payload::CreateProject(response)) => match response.project {
            Some(project) => {
                println!("project {} {:?}", project.id, project.name);
                ExitCode::SUCCESS
            }
            None => fail(&ClientError::BusinessAmbiguous(
                "daemon omitted the project dto".to_owned(),
            )),
        },
        Some(task_core_response::Payload::GetProject(response)) => match response.project {
            Some(project) => {
                println!("project {} {:?}", project.id, project.name);
                ExitCode::SUCCESS
            }
            None => fail(&ClientError::BusinessAmbiguous(
                "daemon omitted the project dto".to_owned(),
            )),
        },
        Some(task_core_response::Payload::ListProjects(response)) => {
            for project in &response.projects {
                println!("project {} {:?}", project.id, project.name);
            }
            ExitCode::SUCCESS
        }
        Some(task_core_response::Payload::CreateTask(response)) => match response.task {
            Some(task) => {
                println!(
                    "task {} {} {} {:?}",
                    task.id, task.project_id, task.state, task.objective
                );
                ExitCode::SUCCESS
            }
            None => fail(&ClientError::BusinessAmbiguous(
                "daemon omitted the task dto".to_owned(),
            )),
        },
        Some(task_core_response::Payload::GetTask(response)) => match response.task {
            Some(task) => {
                println!(
                    "task {} {} {} {:?}",
                    task.id, task.project_id, task.state, task.objective
                );
                ExitCode::SUCCESS
            }
            None => fail(&ClientError::BusinessAmbiguous(
                "daemon omitted the task dto".to_owned(),
            )),
        },
        Some(task_core_response::Payload::ListTasks(response)) => {
            for task in &response.tasks {
                println!(
                    "task {} {} {} {:?}",
                    task.id, task.project_id, task.state, task.objective
                );
            }
            ExitCode::SUCCESS
        }
        Some(task_core_response::Payload::AddDependency(_)) => {
            println!("ok dependency added");
            ExitCode::SUCCESS
        }
        Some(task_core_response::Payload::ListDependencies(response)) => {
            for id in &response.depends_on_ids {
                println!("dependency {id}");
            }
            ExitCode::SUCCESS
        }
        Some(task_core_response::Payload::Lifecycle(response)) => match response.task {
            Some(task) => {
                println!("ok {} {}", task.id, task.state);
                ExitCode::SUCCESS
            }
            None => fail(&ClientError::BusinessAmbiguous(
                "daemon omitted the task dto".to_owned(),
            )),
        },
        Some(task_core_response::Payload::Error(error)) => {
            eprintln!(
                "error {}: {}",
                business_error_name(error.code),
                error.message
            );
            ExitCode::FAILURE
        }
        None => fail(&ClientError::BusinessAmbiguous(
            "daemon sent an empty business response".to_owned(),
        )),
    }
}

/// Wire error code rendered in its canonical UPPER_SNAKE form
/// (ADR-0021 §11), never Rust debug formatting.
fn business_error_name(code: i32) -> String {
    use ipc_contracts::proto::BusinessErrorCode;
    match BusinessErrorCode::try_from(code) {
        Ok(BusinessErrorCode::InvalidArgument) => "INVALID_ARGUMENT".to_owned(),
        Ok(BusinessErrorCode::NotFound) => "NOT_FOUND".to_owned(),
        Ok(BusinessErrorCode::AlreadyExists) => "ALREADY_EXISTS".to_owned(),
        Ok(BusinessErrorCode::InvalidTransition) => "INVALID_TRANSITION".to_owned(),
        Ok(BusinessErrorCode::Conflict) => "CONFLICT".to_owned(),
        Ok(BusinessErrorCode::StaleState) => "STALE_STATE".to_owned(),
        Ok(BusinessErrorCode::CorruptState) => "CORRUPT_STATE".to_owned(),
        Ok(BusinessErrorCode::UnsupportedProtocol) => "UNSUPPORTED_PROTOCOL".to_owned(),
        Ok(BusinessErrorCode::Internal) => "INTERNAL".to_owned(),
        _ => format!("UNKNOWN({code})"),
    }
}

fn usage_project() -> ExitCode {
    eprintln!("usage: agentctl project <create|get|list|events> ...");
    ExitCode::FAILURE
}

fn usage_task() -> ExitCode {
    eprintln!(
        "usage: agentctl task <create|get|list|dependency|ready|claim|start|resume|block|submit|review|verify-begin|verify-fail|verify-succeed> ..."
    );
    ExitCode::FAILURE
}

#[cfg(unix)]
async fn cmd_project(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    let request = match project_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_project(),
    };
    run_business(&endpoint, request).await
}

#[cfg(windows)]
async fn cmd_project(args: Vec<String>) -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    let request = match project_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_project(),
    };
    run_business(&pipe_name, request).await
}

#[cfg(unix)]
async fn cmd_task(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    let request = match task_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_task(),
    };
    run_business(&endpoint, request).await
}

#[cfg(windows)]
async fn cmd_task(args: Vec<String>) -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    let request = match task_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_task(),
    };
    run_business(&pipe_name, request).await
}

fn project_request(args: &[String]) -> Result<TaskCoreRequest, ()> {
    let verb = args.first().map(String::as_str).ok_or(())?;
    match verb {
        "create" => {
            let [_, id, name @ ..] = args else {
                return Err(());
            };
            if id.is_empty() || name.is_empty() {
                return Err(());
            }
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::CreateProject(
                    CreateProjectRequest {
                        id: id.clone(),
                        name: name.join(" "),
                    },
                )),
            })
        }
        "get" => {
            let [_, id] = args else {
                return Err(());
            };
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::GetProject(GetProjectRequest {
                    id: id.clone(),
                })),
            })
        }
        "list" if args.len() == 1 => Ok(TaskCoreRequest {
            payload: Some(task_core_request::Payload::ListProjects(
                ListProjectsRequest {},
            )),
        }),
        _ => Err(()),
    }
}

fn task_request(args: &[String]) -> Result<TaskCoreRequest, ()> {
    let verb = args.first().map(String::as_str).ok_or(())?;
    match verb {
        "create" => {
            let [_, id, project_id, objective @ ..] = args else {
                return Err(());
            };
            if id.is_empty() || project_id.is_empty() || objective.is_empty() {
                return Err(());
            }
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::CreateTask(CreateTaskRequest {
                    id: id.clone(),
                    project_id: project_id.clone(),
                    objective: objective.join(" "),
                })),
            })
        }
        "get" => {
            let [_, id] = args else {
                return Err(());
            };
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::GetTask(GetTaskRequest {
                    id: id.clone(),
                })),
            })
        }
        "list" => {
            let [_, project_id] = args else {
                return Err(());
            };
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::ListTasksForProject(
                    ListTasksForProjectRequest {
                        project_id: project_id.clone(),
                    },
                )),
            })
        }
        "dependency" => dependency_request(&args[1..]),
        "ready" | "claim" | "start" | "resume" | "block" | "submit" | "review" | "verify-begin"
        | "verify-fail" | "verify-succeed" => lifecycle_request(verb, &args[1..]),
        // No set-state / transition / complete / done shortcut exists, by
        // design: anything unlisted here is a usage failure.
        _ => Err(()),
    }
}

fn dependency_request(args: &[String]) -> Result<TaskCoreRequest, ()> {
    let verb = args.first().map(String::as_str).ok_or(())?;
    match verb {
        "add" => {
            let [_, task_id, depends_on_id] = args else {
                return Err(());
            };
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::AddDependency(
                    AddDependencyRequest {
                        task_id: task_id.clone(),
                        depends_on_id: depends_on_id.clone(),
                    },
                )),
            })
        }
        "list" => {
            let [_, task_id] = args else {
                return Err(());
            };
            Ok(TaskCoreRequest {
                payload: Some(task_core_request::Payload::ListDependencies(
                    ListDependenciesRequest {
                        task_id: task_id.clone(),
                    },
                )),
            })
        }
        _ => Err(()),
    }
}

fn lifecycle_request(verb: &str, args: &[String]) -> Result<TaskCoreRequest, ()> {
    // `resume` is the accepted BLOCKED/RETRY_READY → RUNNING resume
    // spelled as operators expect; the wire carries START in both cases.
    // There is deliberately no cancel/fail: those edges are unresolved.
    let op = match verb {
        "ready" => TaskLifecycleOp::MarkReady,
        "claim" => TaskLifecycleOp::Claim,
        "start" | "resume" => TaskLifecycleOp::Start,
        "block" => TaskLifecycleOp::Block,
        "submit" => TaskLifecycleOp::SubmitResult,
        "review" => TaskLifecycleOp::RouteToReview,
        "verify-begin" => TaskLifecycleOp::BeginVerification,
        "verify-fail" => TaskLifecycleOp::VerificationFailed,
        "verify-succeed" => TaskLifecycleOp::VerificationSucceeded,
        _ => return Err(()),
    };
    let (task_id, reason) = match args {
        [id] => (id.clone(), String::new()),
        [id, rest @ ..] if !rest.is_empty() => (id.clone(), rest.join(" ")),
        _ => return Err(()),
    };
    Ok(TaskCoreRequest {
        payload: Some(task_core_request::Payload::Lifecycle(LifecycleRequest {
            task_id,
            op: op as i32,
            reason,
        })),
    })
}
