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
    execution_core_request, execution_core_response, task_core_request, task_core_response,
    AddDependencyRequest, CancelExecutionRequest, CreateProjectRequest, CreateTaskRequest,
    ExecutionCoreRequest, ExecutionCoreResponse, GetExecutionRequest, GetProjectRequest,
    GetTaskRequest, LifecycleRequest, ListDependenciesRequest, ListProjectsRequest,
    ListTasksForProjectRequest, ResumeExecutionRequest, RunExecutionRequest, TaskCoreRequest,
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
        Some("execution") => cmd_execution(args.collect()).await,
        _ => {
            eprintln!("usage: agentctl <start|ping|shutdown|project|task|execution> ...");
            eprintln!("  agentctl project create <id> <name>");
            eprintln!("  agentctl project get <id>");
            eprintln!("  agentctl project list");
            eprintln!("  agentctl task create <id> <project-id> <objective>");
            eprintln!("  agentctl task get <id>");
            eprintln!("  agentctl task list <project-id>");
            eprintln!("  agentctl task dependency add <task-id> <depends-on-id>");
            eprintln!("  agentctl task dependency list <task-id>");
            eprintln!("  agentctl task <ready|claim|start|resume|block|submit|review|verify-begin|verify-fail|verify-succeed> <id> [reason]");
            eprintln!("  agentctl execution run <task-id> <repository-path> [--rev <R>] [--ref <name>] [--execution <E>] [--attempt <A>] [--detach]");
            eprintln!("  agentctl execution status <execution-id>");
            eprintln!("  agentctl execution cancel <execution-id>");
            eprintln!("  agentctl execution resume <execution-id> <repository-path> [--ref <name>] [--rev <R>]");
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
/// (ADR-0021 §11 plus the G5 provider code), never Rust debug formatting.
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
        Ok(BusinessErrorCode::ProviderUnavailable) => "PROVIDER_UNAVAILABLE".to_owned(),
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

// -- execution surface (G5, protocol v2) ---------------------------------

/// Interval between `status` polls while waiting for a terminal attempt.
const STATUS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Budget for `run` waiting: provider timeout (300 s daemon-side) plus
/// lease/verify margin. Expiry never cancels the provider — re-query
/// `status` (or `cancel` explicitly).
const RUN_WAIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(420);

fn usage_execution() -> ExitCode {
    eprintln!("usage: agentctl execution <run|status|cancel|resume> ...");
    eprintln!("  agentctl execution run <task-id> <repository-path> [--rev <R>] [--ref <name>] [--execution <E>] [--attempt <A>] [--detach]");
    eprintln!("  agentctl execution status <execution-id>");
    eprintln!("  agentctl execution cancel <execution-id>");
    eprintln!(
        "  agentctl execution resume <execution-id> <repository-path> [--ref <name>] [--rev <R>]"
    );
    ExitCode::FAILURE
}

/// Client-minted identity: known even when the `run` reply is lost, so
/// an ambiguous start re-probes `status` instead of replaying `run`.
fn mint_execution_id(prefix: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{millis}-{}", std::process::id())
}

#[cfg(unix)]
async fn cmd_execution(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    match execution_action(&args) {
        Ok(ExecutionAction::Run {
            request,
            execution_id,
            detach,
        }) => run_and_maybe_wait(&endpoint, request, execution_id, detach).await,
        Ok(ExecutionAction::Once(request)) => run_execution(&endpoint, request).await,
        Err(()) => usage_execution(),
    }
}

#[cfg(windows)]
async fn cmd_execution(args: Vec<String>) -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    match execution_action(&args) {
        Ok(ExecutionAction::Run {
            request,
            execution_id,
            detach,
        }) => run_and_maybe_wait(&pipe_name, request, execution_id, detach).await,
        Ok(ExecutionAction::Once(request)) => run_execution(&pipe_name, request).await,
        Err(()) => usage_execution(),
    }
}

enum ExecutionAction {
    Run {
        request: ExecutionCoreRequest,
        execution_id: String,
        detach: bool,
    },
    Once(ExecutionCoreRequest),
}

fn execution_action(args: &[String]) -> Result<ExecutionAction, ()> {
    let verb = args.first().map(String::as_str).ok_or(())?;
    match verb {
        "run" => {
            let rest = &args[1..];
            if rest.len() < 2 {
                return Err(());
            }
            let task_id = rest[0].clone();
            let repository_path = rest[1].clone();
            if task_id.trim().is_empty() || repository_path.trim().is_empty() {
                return Err(());
            }
            let mut base_revision = "HEAD".to_owned();
            let mut repository_ref = String::new();
            let mut execution_id = mint_execution_id("E");
            let mut attempt_id = mint_execution_id("A");
            let mut detach = false;
            let mut index = 2;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--rev" => {
                        index += 1;
                        base_revision = rest.get(index).cloned().ok_or(())?;
                    }
                    "--ref" => {
                        index += 1;
                        repository_ref = rest.get(index).cloned().ok_or(())?;
                    }
                    "--execution" => {
                        index += 1;
                        execution_id = rest.get(index).cloned().ok_or(())?;
                    }
                    "--attempt" => {
                        index += 1;
                        attempt_id = rest.get(index).cloned().ok_or(())?;
                    }
                    "--detach" => {
                        detach = true;
                    }
                    _ => return Err(()),
                }
                index += 1;
            }
            if repository_ref.trim().is_empty() {
                repository_ref = format!("repo-local:{task_id}");
            }
            Ok(ExecutionAction::Run {
                request: ExecutionCoreRequest {
                    payload: Some(execution_core_request::Payload::Run(RunExecutionRequest {
                        task_id,
                        execution_id: execution_id.clone(),
                        attempt_id,
                        repository_ref,
                        repository_path,
                        base_revision,
                    })),
                },
                execution_id,
                detach,
            })
        }
        "status" => {
            let [_, execution_id] = args else {
                return Err(());
            };
            Ok(ExecutionAction::Once(ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                    execution_id: execution_id.clone(),
                })),
            }))
        }
        "cancel" => {
            let [_, execution_id] = args else {
                return Err(());
            };
            Ok(ExecutionAction::Once(ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Cancel(
                    CancelExecutionRequest {
                        execution_id: execution_id.clone(),
                    },
                )),
            }))
        }
        "resume" => {
            // Recovery for an admitted-but-not-attempted execution: the
            // daemon re-drives the same execution with its reserved
            // attempt. `--ref`/`--rev` are required only when no lease
            // exists yet; otherwise the recorded lease values apply.
            let rest = &args[1..];
            if rest.len() < 2 {
                return Err(());
            }
            let execution_id = rest[0].clone();
            let repository_path = rest[1].clone();
            if execution_id.trim().is_empty() || repository_path.trim().is_empty() {
                return Err(());
            }
            let mut repository_ref = String::new();
            let mut base_revision = String::new();
            let mut index = 2;
            while index < rest.len() {
                match rest[index].as_str() {
                    "--ref" => {
                        index += 1;
                        repository_ref = rest.get(index).cloned().ok_or(())?;
                    }
                    "--rev" => {
                        index += 1;
                        base_revision = rest.get(index).cloned().ok_or(())?;
                    }
                    _ => return Err(()),
                }
                index += 1;
            }
            Ok(ExecutionAction::Once(ExecutionCoreRequest {
                payload: Some(execution_core_request::Payload::Resume(
                    ResumeExecutionRequest {
                        execution_id,
                        repository_path,
                        repository_ref,
                        base_revision,
                    },
                )),
            }))
        }
        _ => Err(()),
    }
}

/// Runs one execution request and prints the deterministic response.
/// Success payloads print to stdout; typed errors print to stderr.
#[cfg(unix)]
async fn run_execution(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    request: ExecutionCoreRequest,
) -> ExitCode {
    match agentctl::execution_request(endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => print_execution_response(&response, true),
        Err(error) => fail(&error),
    }
}

/// Runs one execution request and prints the deterministic response.
#[cfg(windows)]
async fn run_execution(pipe_name: &str, request: ExecutionCoreRequest) -> ExitCode {
    match agentctl::execution_request(pipe_name, spawn_sibling_daemon, request).await {
        Ok(response) => print_execution_response(&response, true),
        Err(error) => fail(&error),
    }
}

/// Starts a run and, unless detached, polls `status` until the attempt
/// is terminal. A local Ctrl-C (or any client exit) never cancels the
/// provider: the background cycle keeps its durable outcome, and
/// `status` re-observes it later.
#[cfg(unix)]
async fn run_and_maybe_wait(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    request: ExecutionCoreRequest,
    execution_id: String,
    detach: bool,
) -> ExitCode {
    let response = match agentctl::execution_request(endpoint, spawn_sibling_daemon, request).await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("agentctl: failed: {error}");
            eprintln!("agentctl: re-probe with: agentctl execution status {execution_id}");
            return ExitCode::FAILURE;
        }
    };
    match &response.payload {
        Some(execution_core_response::Payload::Run(run)) => {
            println!(
                "execution {} attempt {} task {} {}",
                run.execution_id, run.attempt_id, run.task_id, run.task_state
            );
        }
        _ => return print_execution_response(&response, true),
    }
    if detach {
        return ExitCode::SUCCESS;
    }
    wait_for_terminal(endpoint, &execution_id).await
}

/// Starts a run and, unless detached, polls `status` until terminal.
#[cfg(windows)]
async fn run_and_maybe_wait(
    pipe_name: &str,
    request: ExecutionCoreRequest,
    execution_id: String,
    detach: bool,
) -> ExitCode {
    let response = match agentctl::execution_request(pipe_name, spawn_sibling_daemon, request).await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("agentctl: failed: {error}");
            eprintln!("agentctl: re-probe with: agentctl execution status {execution_id}");
            return ExitCode::FAILURE;
        }
    };
    match &response.payload {
        Some(execution_core_response::Payload::Run(_)) => {}
        _ => return print_execution_response(&response, true),
    }
    if detach {
        println!("execution {execution_id} started (detached)");
        return ExitCode::SUCCESS;
    }
    wait_for_terminal(pipe_name, &execution_id).await
}

/// Polls `status` until the attempt is terminal, the background is gone
/// without an attempt, or the budget expires. Never cancels: expiry
/// reports the execution id for later re-query.
#[cfg(unix)]
async fn wait_for_terminal(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    execution_id: &str,
) -> ExitCode {
    let deadline = std::time::Instant::now() + RUN_WAIT_BUDGET;
    loop {
        if std::time::Instant::now() >= deadline {
            eprintln!("agentctl: wait budget expired; execution still running");
            eprintln!("agentctl: re-query with: agentctl execution status {execution_id}");
            return ExitCode::FAILURE;
        }
        tokio::time::sleep(STATUS_POLL_INTERVAL).await;
        let request = ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                execution_id: execution_id.to_owned(),
            })),
        };
        match agentctl::execution_request(endpoint, spawn_sibling_daemon, request).await {
            Ok(response) => {
                if let Some(code) = terminal_status(&response) {
                    return code;
                }
            }
            Err(error) => {
                eprintln!("agentctl: status poll failed: {error}");
                eprintln!("agentctl: re-query with: agentctl execution status {execution_id}");
                return ExitCode::FAILURE;
            }
        }
    }
}

/// Polls `status` until terminal, stranded, or budget expiry.
#[cfg(windows)]
async fn wait_for_terminal(pipe_name: &str, execution_id: &str) -> ExitCode {
    let deadline = std::time::Instant::now() + RUN_WAIT_BUDGET;
    loop {
        if std::time::Instant::now() >= deadline {
            eprintln!("agentctl: wait budget expired; execution still running");
            eprintln!("agentctl: re-query with: agentctl execution status {execution_id}");
            return ExitCode::FAILURE;
        }
        tokio::time::sleep(STATUS_POLL_INTERVAL).await;
        let request = ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                execution_id: execution_id.to_owned(),
            })),
        };
        match agentctl::execution_request(pipe_name, spawn_sibling_daemon, request).await {
            Ok(response) => {
                if let Some(code) = terminal_status(&response) {
                    return code;
                }
            }
            Err(error) => {
                eprintln!("agentctl: status poll failed: {error}");
                eprintln!("agentctl: re-query with: agentctl execution status {execution_id}");
                return ExitCode::FAILURE;
            }
        }
    }
}

/// Inspects one `status` response: prints and returns an exit code when
/// terminal (or stranded), or `None` to keep polling. Exit 0 only when
/// an attempt concluded SUCCEEDED (evidence submitted; the Task itself
/// is VERIFYING, never completed by the provider).
fn terminal_status(response: &ExecutionCoreResponse) -> Option<ExitCode> {
    match &response.payload {
        Some(execution_core_response::Payload::Get(get)) => {
            let terminal = get.attempts.iter().find(|attempt| {
                matches!(
                    attempt.outcome.as_str(),
                    "SUCCEEDED" | "FAILED" | "TIMED_OUT" | "CANCELLED"
                )
            });
            match terminal {
                Some(attempt) => {
                    let code = print_execution_response(response, false);
                    if attempt.outcome == "SUCCEEDED" {
                        Some(ExitCode::SUCCESS)
                    } else {
                        let _ = code;
                        Some(ExitCode::FAILURE)
                    }
                }
                None if !get.execution_live => {
                    // Background gone without a terminal attempt: entry
                    // committed, provider half never concluded (lease
                    // refusal, bind failure, or daemon restart).
                    print_execution_response(response, false);
                    if get.attempts.is_empty() {
                        eprintln!(
                            "agentctl: execution {} ended before its attempt; task {} is {}; resume with: agentctl execution resume {} <repository-path> [--ref <name>] [--rev <R>]",
                            get.execution_id, get.task_id, get.task_state, get.execution_id
                        );
                    } else {
                        eprintln!(
                            "agentctl: execution {} has no live run with attempt still open (stranded or concluding); re-query status",
                            get.execution_id
                        );
                    }
                    Some(ExitCode::FAILURE)
                }
                None => None,
            }
        }
        _ => {
            print_execution_response(response, false);
            Some(ExitCode::FAILURE)
        }
    }
}

fn print_execution_response(response: &ExecutionCoreResponse, direct: bool) -> ExitCode {
    match &response.payload {
        Some(execution_core_response::Payload::Run(run)) => {
            println!(
                "execution {} attempt {} task {} {}",
                run.execution_id, run.attempt_id, run.task_id, run.task_state
            );
            ExitCode::SUCCESS
        }
        Some(execution_core_response::Payload::Resume(resumed)) => {
            println!(
                "resumed execution {} attempt {} task {} {}",
                resumed.execution_id, resumed.attempt_id, resumed.task_id, resumed.task_state
            );
            ExitCode::SUCCESS
        }
        Some(execution_core_response::Payload::Get(get)) => {
            println!(
                "execution {} task {} {} live={}",
                get.execution_id, get.task_id, get.task_state, get.execution_live
            );
            if get.workspace_path.is_empty() {
                println!("workspace -");
            } else {
                println!("workspace {}", get.workspace_path);
            }
            if !get.provider_ref.is_empty() {
                println!("provider {}", get.provider_ref);
            }
            for attempt in &get.attempts {
                if attempt.detail.is_empty() {
                    println!(
                        "attempt {} {} {}",
                        attempt.attempt_id, attempt.outcome, attempt.provider_ref
                    );
                } else {
                    println!(
                        "attempt {} {} {} {}",
                        attempt.attempt_id, attempt.outcome, attempt.provider_ref, attempt.detail
                    );
                }
            }
            let _ = direct;
            ExitCode::SUCCESS
        }
        Some(execution_core_response::Payload::Cancel(cancel)) => {
            println!(
                "cancel {} {} requested={}",
                cancel.execution_id, cancel.run_state, cancel.cancel_requested
            );
            if !cancel.detail.is_empty() {
                println!("{}", cancel.detail);
            }
            ExitCode::SUCCESS
        }
        Some(execution_core_response::Payload::Error(error)) => {
            eprintln!(
                "error {}: {}",
                business_error_name(error.code),
                error.message
            );
            ExitCode::FAILURE
        }
        None => fail(&ClientError::ExecutionAmbiguous(
            "daemon sent an empty execution response".to_owned(),
        )),
    }
}
