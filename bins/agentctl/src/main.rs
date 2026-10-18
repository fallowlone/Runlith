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
    execution_core_request, execution_core_response, human_decision_core_request,
    human_decision_core_response, planning_core_request, planning_core_response,
    policy_core_request, policy_core_response, task_authoring_core_request,
    task_authoring_core_response, task_core_request, task_core_response, AddDependencyRequest,
    ApplyHumanContinuationRequest, ApprovalDecision, CancelExecutionRequest,
    CreatePlanningTaskRequest, CreateProjectRequest, CreateTaskRequest, DecideApprovalRequest,
    DecideHumanDecisionRequest, ExecutionCoreRequest, ExecutionCoreResponse, GetApprovalRequest,
    GetExecutionRequest, GetHumanDecisionRequest, GetProjectRequest, GetTaskRequest,
    HumanContinuationDecision, HumanDecisionCoreRequest, HumanDecisionCoreResponse,
    HumanDecisionDto, LifecycleRequest, ListDependenciesRequest, ListHumanDecisionsRequest,
    ListProjectsRequest, ListTasksForProjectRequest, PlanningContext, PlanningCoreRequest,
    PolicyCoreRequest, PolicyCoreResponse, ResumeExecutionRequest, RunExecutionRequest,
    TakeoverAuthoredField, TakeoverRecordDto, TaskAuthoringCoreRequest, TaskCoreRequest,
    TaskCoreResponse, TaskLifecycleOp, TriggerPlanningRequest,
};

mod vault;

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
        Some("approval") => cmd_approval(args.collect()).await,
        Some("human-decision") => cmd_human_decision(args.collect()).await,
        Some("planning") => cmd_planning(args.collect()).await,
        Some("vault") => vault::cmd(args.collect()).await,
        Some("find") => cmd_find(args.collect()).await,
        Some("memory") => cmd_memory(args.collect()).await,
        Some("routine") => cmd_routine(args.collect()).await,
        Some("daemon") => cmd_daemon(args.collect()).await,
        _ => {
            eprintln!("usage: agentctl <start|ping|shutdown|daemon|routine|project|task|execution|approval|human-decision|planning|vault|find|memory> ...");
            eprintln!("  {}", vault::USAGE);
            eprintln!("  agentctl project create <id> <name>");
            eprintln!("  agentctl project get <id>");
            eprintln!("  agentctl project list");
            eprintln!("  agentctl task create <id> <project-id> <objective>");
            eprintln!("  {TASK_CREATE_PLANNED_USAGE}");
            eprintln!("  agentctl task get <id>");
            eprintln!("  agentctl task list <project-id>");
            eprintln!("  agentctl task dependency add <task-id> <depends-on-id>");
            eprintln!("  agentctl task dependency list <task-id>");
            eprintln!("  agentctl task <ready|claim|start|resume|block|submit|review|verify-begin|verify-fail|verify-succeed> <id> [reason]");
            eprintln!("  agentctl execution run <task-id> <repository-path> [--rev <R>] [--ref <name>] [--execution <E>] [--attempt <A>] [--detach]");
            eprintln!("  agentctl execution status <execution-id>");
            eprintln!("  agentctl execution cancel <execution-id>");
            eprintln!("  agentctl execution resume <execution-id> <repository-path> [--ref <name>] [--rev <R>]");
            eprintln!("  agentctl approval get <approval-request-id>");
            eprintln!("  agentctl approval decide <approval-request-id> <approve|deny>");
            eprintln!("  agentctl human-decision list");
            eprintln!("  agentctl human-decision get <request-id>");
            eprintln!(
                "  agentctl human-decision decide <request-id> <resume|replan|abort|takeover>"
            );
            eprintln!("  agentctl human-decision takeover-apply <request-id> <expires-at-ms>");
            eprintln!("  agentctl human-decision takeover-append <record-id> [--actions <text>] [--uncertainty <text>] [--question <text>] [--resulting-revision <n>] [--sensitivity <label>]");
            eprintln!("  agentctl human-decision takeover-render <record-id>");
            eprintln!("  agentctl human-decision takeover-reconcile [<task-id>]");
            eprintln!("  agentctl planning trigger <task-id> <branch-id> <evidence-ref>");
            eprintln!("  agentctl planning apply <human-decision-id>");
            eprintln!("  agentctl find query <repository-path> <text...> [--scope <dir>] [--limit <n>] [--continuation <token>] [--format text|json]");
            eprintln!("  agentctl find show <repository-path> <evidence-id> [--context <n>] [--format text|json]");
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
        "usage: agentctl task <create|create-planned|get|list|dependency|ready|claim|start|resume|block|submit|review|verify-begin|verify-fail|verify-succeed> ..."
    );
    ExitCode::FAILURE
}

const TASK_CREATE_PLANNED_USAGE: &str = "agentctl task create-planned <id> <project-id> <objective> --scope <workspace:path>... --acceptance <criterion-id>... --risk <low|medium|high|critical>";

/// Parses the protocol-v6 authoring command. Positional words up to the first
/// flag form the objective; every flag takes exactly one value. The client
/// only carries intent: the daemon validates and canonicalizes all values.
#[cfg(unix)]
fn create_planned_task_request(args: &[String]) -> Result<TaskAuthoringCoreRequest, ()> {
    let [id, project_id, rest @ ..] = args else {
        return Err(());
    };
    let objective_len = rest
        .iter()
        .position(|arg| arg.starts_with("--"))
        .unwrap_or(rest.len());
    let (objective, mut flags) = rest.split_at(objective_len);
    let mut request = CreatePlanningTaskRequest {
        id: id.clone(),
        project_id: project_id.clone(),
        objective: objective.join(" "),
        ..CreatePlanningTaskRequest::default()
    };
    let mut risk = None;
    while let [flag, value, remaining @ ..] = flags {
        match flag.as_str() {
            "--scope" => request.allowed_paths.push(value.clone()),
            "--acceptance" => request.acceptance_criteria.push(value.clone()),
            "--risk" if risk.is_none() => risk = Some(value.clone()),
            _ => return Err(()),
        }
        flags = remaining;
    }
    if !flags.is_empty() || id.is_empty() || project_id.is_empty() || objective.is_empty() {
        return Err(());
    }
    request.baseline_risk = risk.ok_or(())?;
    Ok(TaskAuthoringCoreRequest {
        payload: Some(task_authoring_core_request::Payload::CreatePlanningTask(
            request,
        )),
    })
}

#[cfg(unix)]
async fn run_create_planned_task(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    args: &[String],
) -> ExitCode {
    let Ok(request) = create_planned_task_request(args) else {
        eprintln!("usage: {TASK_CREATE_PLANNED_USAGE}");
        return ExitCode::FAILURE;
    };
    let response =
        match agentctl::task_authoring_request(endpoint, spawn_sibling_daemon, request).await {
            Ok(response) => response,
            Err(error) => return fail(&error),
        };
    match response.payload {
        Some(task_authoring_core_response::Payload::CreatePlanningTask(created)) => {
            match created
                .task
                .and_then(|mut planned| Some((planned.task.take()?, planned)))
            {
                Some((task, planned)) => {
                    println!(
                        "task {} {} {} {:?} scope={} acceptance={} risk={}",
                        task.id,
                        task.project_id,
                        task.state,
                        task.objective,
                        planned.allowed_paths.join(","),
                        planned.acceptance_criteria.join(","),
                        planned.baseline_risk
                    );
                    ExitCode::SUCCESS
                }
                None => fail(&ClientError::BusinessAmbiguous(
                    "daemon omitted the planning task dto".to_owned(),
                )),
            }
        }
        Some(task_authoring_core_response::Payload::Error(error)) => {
            eprintln!(
                "error {}: {}",
                business_error_name(error.code),
                error.message
            );
            ExitCode::FAILURE
        }
        None => fail(&ClientError::BusinessAmbiguous(
            "daemon sent an empty task authoring response".to_owned(),
        )),
    }
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
    if args.first().map(String::as_str) == Some("create-planned") {
        return run_create_planned_task(&endpoint, &args[1..]).await;
    }
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
    if args.first().map(String::as_str) == Some("create-planned") {
        eprintln!("agentctl: task authoring is unavailable on this build");
        return ExitCode::FAILURE;
    }
    let request = match task_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_task(),
    };
    run_business(&pipe_name, request).await
}

fn usage_approval() -> ExitCode {
    eprintln!("usage: agentctl approval <get|decide> ...");
    eprintln!("  agentctl approval get <approval-request-id>");
    eprintln!("  agentctl approval decide <approval-request-id> <approve|deny>");
    ExitCode::FAILURE
}

fn approval_request(args: &[String]) -> Result<PolicyCoreRequest, ()> {
    match args {
        [verb, request_id] if verb == "get" && !request_id.trim().is_empty() => {
            Ok(PolicyCoreRequest {
                payload: Some(policy_core_request::Payload::GetApproval(
                    GetApprovalRequest {
                        request_id: request_id.clone(),
                    },
                )),
            })
        }
        [verb, request_id, decision] if verb == "decide" && !request_id.trim().is_empty() => {
            let decision = match decision.as_str() {
                "approve" => ApprovalDecision::Approve,
                "deny" => ApprovalDecision::Deny,
                _ => return Err(()),
            };
            Ok(PolicyCoreRequest {
                payload: Some(policy_core_request::Payload::DecideApproval(
                    DecideApprovalRequest {
                        request_id: request_id.clone(),
                        decision: decision as i32,
                    },
                )),
            })
        }
        _ => Err(()),
    }
}

#[cfg(unix)]
async fn cmd_approval(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    let request = match approval_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_approval(),
    };
    run_policy(&endpoint, request).await
}

#[cfg(windows)]
async fn cmd_approval(args: Vec<String>) -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    let request = match approval_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_approval(),
    };
    run_policy(&pipe_name, request).await
}

#[cfg(unix)]
async fn run_policy(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    request: PolicyCoreRequest,
) -> ExitCode {
    match agentctl::policy_request(endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => print_policy_response(response),
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn run_policy(pipe_name: &str, request: PolicyCoreRequest) -> ExitCode {
    match agentctl::policy_request(pipe_name, spawn_sibling_daemon, request).await {
        Ok(response) => print_policy_response(response),
        Err(error) => fail(&error),
    }
}

fn print_policy_response(response: PolicyCoreResponse) -> ExitCode {
    let approval = match response.payload {
        Some(policy_core_response::Payload::GetApproval(response)) => response.approval,
        Some(policy_core_response::Payload::DecideApproval(response)) => response.approval,
        Some(policy_core_response::Payload::Error(error)) => {
            eprintln!(
                "error {}: {}",
                business_error_name(error.code),
                error.message
            );
            return ExitCode::FAILURE;
        }
        None => {
            return fail(&ClientError::PolicyAmbiguous(
                "daemon sent an empty approval response".to_owned(),
            ))
        }
    };
    let Some(approval) = approval else {
        return fail(&ClientError::PolicyAmbiguous(
            "daemon omitted the approval dto".to_owned(),
        ));
    };
    println!(
        "approval {} {} operation={} requester={} task={} expires_at_ms={}",
        approval.request_id,
        approval.state,
        approval.operation_id,
        approval.requester,
        approval.task_id,
        approval.expires_at_ms
    );
    if let Some(execution_id) = approval.execution_id.as_deref() {
        println!("execution {execution_id}");
    }
    if let Some(attempt_id) = approval.attempt_id.as_deref() {
        println!("attempt {attempt_id}");
    }
    println!("policy {}", approval.policy_fingerprint);
    if let Some(approver) = approval.approver.as_deref() {
        println!("approver {approver}");
    }
    for capability in approval.requested {
        print_approval_capability("requested", capability);
    }
    for capability in approval.effective {
        print_approval_capability("effective", capability);
    }
    ExitCode::SUCCESS
}

fn print_approval_capability(label: &str, capability: ipc_contracts::proto::ApprovalCapabilityDto) {
    match capability.workspace {
        Some(workspace) => println!(
            "{label} {} workspace={} scope={}",
            capability.kind, workspace, capability.scope
        ),
        None => println!("{label} {} scope={}", capability.kind, capability.scope),
    }
}

fn usage_human_decision() -> ExitCode {
    eprintln!(
        "usage: agentctl human-decision <list|get|decide|takeover-apply|takeover-append|takeover-render|takeover-reconcile> ..."
    );
    eprintln!("  agentctl human-decision list");
    eprintln!("  agentctl human-decision get <request-id>");
    eprintln!("  agentctl human-decision decide <request-id> <resume|replan|abort|takeover>");
    eprintln!("  agentctl human-decision takeover-apply <request-id> <expires-at-ms>");
    eprintln!(
        "  agentctl human-decision takeover-append <record-id> [--actions <text>] [--uncertainty <text>] [--question <text>] [--resulting-revision <n>] [--sensitivity <label>]"
    );
    eprintln!("  agentctl human-decision takeover-render <record-id>");
    eprintln!("  agentctl human-decision takeover-reconcile [<task-id>]");
    ExitCode::FAILURE
}

fn takeover_append_request(args: &[String]) -> Result<HumanDecisionCoreRequest, ()> {
    let [verb, record_id, rest @ ..] = args else {
        return Err(());
    };
    if verb != "takeover-append" || record_id.trim().is_empty() {
        return Err(());
    }
    let mut actions = None;
    let mut uncertainty = None;
    let mut question = None;
    let mut resulting_revision = None;
    let mut sensitivity = "PROJECT".to_owned();
    let mut flags = rest;
    while let [flag, value, remaining @ ..] = flags {
        match flag.as_str() {
            "--actions" => actions = Some(value.clone()),
            "--uncertainty" => uncertainty = Some(value.clone()),
            "--question" => question = Some(value.clone()),
            "--resulting-revision" => {
                resulting_revision = Some(value.parse::<u64>().map_err(|_| ())?)
            }
            "--sensitivity" => sensitivity = value.clone(),
            _ => return Err(()),
        }
        flags = remaining;
    }
    if !flags.is_empty()
        || (actions.is_none()
            && uncertainty.is_none()
            && question.is_none()
            && resulting_revision.is_none())
    {
        return Err(());
    }
    let authored = |text: Option<String>| {
        text.map(|text| TakeoverAuthoredField {
            text,
            sensitivity: sensitivity.clone(),
            is_ref: false,
        })
    };
    Ok(HumanDecisionCoreRequest {
        payload: Some(human_decision_core_request::Payload::AppendTakeover(
            ipc_contracts::proto::AppendTakeoverRequest {
                record_id: record_id.clone(),
                actions: authored(actions),
                uncertainty: authored(uncertainty),
                question: authored(question),
                resulting_revision,
            },
        )),
    })
}

fn human_decision_request(args: &[String]) -> Result<HumanDecisionCoreRequest, ()> {
    if args.first().map(String::as_str) == Some("takeover-append") {
        return takeover_append_request(args);
    }
    match args {
        [verb] if verb == "list" => Ok(HumanDecisionCoreRequest {
            payload: Some(human_decision_core_request::Payload::ListPending(
                ListHumanDecisionsRequest {},
            )),
        }),
        [verb, request_id] if verb == "get" && !request_id.trim().is_empty() => {
            Ok(HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::Get(
                    GetHumanDecisionRequest {
                        request_id: request_id.clone(),
                    },
                )),
            })
        }
        [verb, request_id, continuation] if verb == "decide" && !request_id.trim().is_empty() => {
            let continuation = match continuation.as_str() {
                "resume" => HumanContinuationDecision::Resume,
                "replan" => HumanContinuationDecision::Replan,
                "abort" => HumanContinuationDecision::Abort,
                "takeover" => HumanContinuationDecision::Takeover,
                _ => return Err(()),
            };
            Ok(HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::Decide(
                    DecideHumanDecisionRequest {
                        request_id: request_id.clone(),
                        continuation: continuation as i32,
                    },
                )),
            })
        }
        [verb, request_id, expires]
            if verb == "takeover-apply" && !request_id.trim().is_empty() =>
        {
            Ok(HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::ApplyTakeover(
                    ipc_contracts::proto::ApplyTakeoverRequest {
                        request_id: request_id.clone(),
                        expires_at_ms: expires.parse::<u64>().map_err(|_| ())?,
                    },
                )),
            })
        }
        [verb, record_id] if verb == "takeover-render" && !record_id.trim().is_empty() => {
            Ok(HumanDecisionCoreRequest {
                payload: Some(human_decision_core_request::Payload::RenderHandoffPacket(
                    ipc_contracts::proto::RenderHandoffPacketRequest {
                        record_id: record_id.clone(),
                    },
                )),
            })
        }
        [verb] if verb == "takeover-reconcile" => Ok(HumanDecisionCoreRequest {
            payload: Some(
                human_decision_core_request::Payload::ReconcileTakeoverExpiry(
                    ipc_contracts::proto::ReconcileTakeoverExpiryRequest { task_id: None },
                ),
            ),
        }),
        [verb, task_id] if verb == "takeover-reconcile" && !task_id.trim().is_empty() => {
            Ok(HumanDecisionCoreRequest {
                payload: Some(
                    human_decision_core_request::Payload::ReconcileTakeoverExpiry(
                        ipc_contracts::proto::ReconcileTakeoverExpiryRequest {
                            task_id: Some(task_id.clone()),
                        },
                    ),
                ),
            })
        }
        _ => Err(()),
    }
}

#[cfg(unix)]
async fn cmd_human_decision(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(endpoint) => endpoint,
        Err(error) => return fail(&error),
    };
    let request = match human_decision_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_human_decision(),
    };
    run_human_decision(&endpoint, request).await
}

#[cfg(windows)]
async fn cmd_human_decision(args: Vec<String>) -> ExitCode {
    let pipe_name = match resolve_production_endpoint() {
        Ok(pipe_name) => pipe_name,
        Err(error) => return fail(&error),
    };
    let request = match human_decision_request(&args) {
        Ok(request) => request,
        Err(_) => return usage_human_decision(),
    };
    run_human_decision(&pipe_name, request).await
}

#[cfg(unix)]
async fn run_human_decision(
    endpoint: &ipc_transport::platform::unix::UnixEndpoint,
    request: HumanDecisionCoreRequest,
) -> ExitCode {
    match agentctl::human_decision_request(endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => print_human_decision_response(response),
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn run_human_decision(pipe_name: &str, request: HumanDecisionCoreRequest) -> ExitCode {
    match agentctl::human_decision_request(pipe_name, spawn_sibling_daemon, request).await {
        Ok(response) => print_human_decision_response(response),
        Err(error) => fail(&error),
    }
}

fn print_human_decision_response(response: HumanDecisionCoreResponse) -> ExitCode {
    match response.payload {
        Some(human_decision_core_response::Payload::ListPending(response)) => {
            for decision in response.decisions {
                print_human_decision(decision);
            }
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::Get(response)) => {
            let Some(decision) = response.decision else {
                return fail(&ClientError::HumanDecisionAmbiguous(
                    "daemon omitted the human decision dto".to_owned(),
                ));
            };
            print_human_decision(decision);
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::Decide(response)) => {
            let Some(decision) = response.decision else {
                return fail(&ClientError::HumanDecisionAmbiguous(
                    "daemon omitted the human decision dto".to_owned(),
                ));
            };
            print_human_decision(decision);
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::ApplyTakeover(response)) => {
            let Some(record) = response.record else {
                return fail(&ClientError::HumanDecisionAmbiguous(
                    "daemon omitted the takeover record dto".to_owned(),
                ));
            };
            print_takeover_record(record);
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::AppendTakeover(response)) => {
            let Some(record) = response.record else {
                return fail(&ClientError::HumanDecisionAmbiguous(
                    "daemon omitted the takeover record dto".to_owned(),
                ));
            };
            print_takeover_record(record);
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::RenderHandoffPacket(response)) => {
            let Some(packet) = response.packet else {
                return fail(&ClientError::HumanDecisionAmbiguous(
                    "daemon omitted the handoff packet dto".to_owned(),
                ));
            };
            println!(
                "handoff goal={:?} task={} seq={} branch={} head={} lease={}",
                packet.goal,
                packet.task_id,
                packet.task_event_seq,
                packet.plan_branch_id,
                packet.plan_head,
                packet.lease_id.as_deref().unwrap_or("-")
            );
            println!("blockers {}", packet.blockers.join(","));
            print_authored("actions", packet.actions);
            print_authored("uncertainty", packet.uncertainty);
            print_authored("question", packet.question);
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::ReconcileTakeoverExpiry(response)) => {
            for record in response.records {
                print_takeover_record(record);
            }
            ExitCode::SUCCESS
        }
        Some(human_decision_core_response::Payload::Error(error)) => {
            eprintln!(
                "error {}: {}",
                business_error_name(error.code),
                error.message
            );
            ExitCode::FAILURE
        }
        None => fail(&ClientError::HumanDecisionAmbiguous(
            "daemon sent an empty human-decision response".to_owned(),
        )),
    }
}

fn print_authored(label: &str, field: Option<TakeoverAuthoredField>) {
    if let Some(field) = field {
        println!(
            "{label} {} {} {:?}",
            field.sensitivity,
            if field.is_ref { "ref" } else { "inline" },
            field.text
        );
    }
}

fn print_takeover_record(record: TakeoverRecordDto) {
    println!(
        "takeover {} {} task={} branch={} hdr={} basis={} expires_at_ms={}",
        record.record_id,
        record.status,
        record.task_id,
        record.plan_branch_id,
        record.hdr_id,
        record.basis_plan_revision,
        record.expires_at_ms
    );
    print_authored("actions", record.actions);
    print_authored("uncertainty", record.uncertainty);
    print_authored("question", record.question);
    if let Some(revision) = record.resulting_revision {
        println!("resulting_revision {revision}");
    }
}

fn print_human_decision(decision: HumanDecisionDto) {
    println!(
        "human-decision {} {} task={} branch={} version={} risk={} plan_head={} snapshot={}",
        decision.request_id,
        decision.status,
        decision.task_id,
        decision.plan_branch_id,
        decision.request_version,
        decision.risk,
        decision.plan_head,
        decision.planning_input_snapshot_id
    );
    println!("scope {}", decision.affected_scope.join(","));
    println!(
        "options {}",
        decision
            .options
            .into_iter()
            .map(human_continuation_name)
            .collect::<Vec<_>>()
            .join(",")
    );
    if let Some(selected) = decision.selected {
        println!("selected {}", human_continuation_name(selected));
    }
    if let Some(consumed_at_ms) = decision.consumed_at_ms {
        println!("consumed_at_ms {consumed_at_ms}");
    }
}

fn planning_context(values: &[String]) -> Result<PlanningContext, ()> {
    match values {
        [task_id, plan_branch_id, evidence_ref] => Ok(PlanningContext {
            task_id: task_id.clone(),
            plan_branch_id: plan_branch_id.clone(),
            scope_roots: Vec::new(),
            acceptance_criterion_ids: Vec::new(),
            policy_fingerprint: String::new(),
            evidence_ref: evidence_ref.clone(),
        }),
        // Compatibility with the original protocol-v5 CLI shape. These
        // authority-bearing values are serialized only so old invocations
        // remain decodable; current agentd ignores them.
        [task_id, plan_branch_id, scope_root, criterion, policy_fingerprint, evidence_ref] => {
            Ok(PlanningContext {
                task_id: task_id.clone(),
                plan_branch_id: plan_branch_id.clone(),
                scope_roots: vec![scope_root.clone()],
                acceptance_criterion_ids: vec![criterion.clone()],
                policy_fingerprint: policy_fingerprint.clone(),
                evidence_ref: evidence_ref.clone(),
            })
        }
        _ => Err(()),
    }
}

fn planning_request(args: &[String]) -> Result<PlanningCoreRequest, ()> {
    match args {
        [verb, rest @ ..] if verb == "trigger" => Ok(PlanningCoreRequest {
            payload: Some(planning_core_request::Payload::Trigger(
                TriggerPlanningRequest {
                    context: Some(planning_context(rest)?),
                },
            )),
        }),
        [verb, id] if verb == "apply" => Ok(PlanningCoreRequest {
            payload: Some(planning_core_request::Payload::ApplyHumanContinuation(
                ApplyHumanContinuationRequest {
                    request_id: id.clone(),
                    context: None,
                },
            )),
        }),
        [verb, id, rest @ ..] if verb == "apply" => Ok(PlanningCoreRequest {
            payload: Some(planning_core_request::Payload::ApplyHumanContinuation(
                ApplyHumanContinuationRequest {
                    request_id: id.clone(),
                    context: Some(planning_context(rest)?),
                },
            )),
        }),
        _ => Err(()),
    }
}

#[cfg(unix)]
async fn cmd_planning(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let request = match planning_request(&args) {
        Ok(value) => value,
        Err(()) => {
            eprintln!("usage: agentctl planning trigger <task-id> <branch-id> <evidence-ref> | apply <human-decision-id>");
            return ExitCode::FAILURE;
        }
    };
    match agentctl::planning_request(&endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => match response.payload {
            Some(planning_core_response::Payload::Operation(operation)) => {
                println!(
                    "planning {} branch={} revision={} decision={} {}",
                    operation.outcome,
                    operation.plan_branch_id.unwrap_or_default(),
                    operation
                        .revision_seq
                        .map_or_else(|| "-".to_owned(), |value| value.to_string()),
                    operation.human_decision_request_id.unwrap_or_default(),
                    operation.detail
                );
                ExitCode::SUCCESS
            }
            Some(planning_core_response::Payload::Error(error)) => {
                fail(&ClientError::HumanDecisionAmbiguous(error.message))
            }
            None => fail(&ClientError::HumanDecisionAmbiguous(
                "empty planning response".to_owned(),
            )),
        },
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_planning(_args: Vec<String>) -> ExitCode {
    eprintln!("agentctl: planning control is unavailable on this build");
    ExitCode::FAILURE
}

#[cfg(unix)]
async fn cmd_find(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let (request, format) = match agentctl::parse_finder_args(&args) {
        Ok(value) => value,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    match agentctl::finder_request(&endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => match agentctl::format_finder_response(&response, format) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        },
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_find(_args: Vec<String>) -> ExitCode {
    eprintln!("agentctl: finder control is unavailable on this build");
    ExitCode::FAILURE
}

#[cfg(unix)]
async fn cmd_routine(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let request = match agentctl::parse_routine_args(&args) {
        Ok(value) => value,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    match agentctl::routine_request(&endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => match agentctl::format_routine_response(&response) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        },
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_routine(_args: Vec<String>) -> ExitCode {
    eprintln!("agentctl: routine control is unavailable on this build");
    ExitCode::FAILURE
}

async fn cmd_daemon(args: Vec<String>) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("start") => cmd_start().await,
        Some("status") => cmd_ping().await,
        Some("stop") => cmd_shutdown().await,
        _ => {
            eprintln!("usage: agentctl daemon <start|status|stop>");
            ExitCode::FAILURE
        }
    }
}

#[cfg(unix)]
async fn cmd_memory(args: Vec<String>) -> ExitCode {
    let endpoint = match resolve_production_endpoint() {
        Ok(value) => value,
        Err(error) => return fail(&error),
    };
    let request = match agentctl::parse_memory_args(&args) {
        Ok(value) => value,
        Err(usage) => {
            eprintln!("{usage}");
            return ExitCode::FAILURE;
        }
    };
    match agentctl::memory_request(&endpoint, spawn_sibling_daemon, request).await {
        Ok(response) => match agentctl::format_memory_response(&response) {
            Ok(text) => {
                println!("{text}");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::FAILURE
            }
        },
        Err(error) => fail(&error),
    }
}

#[cfg(windows)]
async fn cmd_memory(_args: Vec<String>) -> ExitCode {
    eprintln!("agentctl: memory control is unavailable on this build");
    ExitCode::FAILURE
}

fn human_continuation_name(value: i32) -> &'static str {
    match HumanContinuationDecision::try_from(value) {
        Ok(HumanContinuationDecision::Resume) => "RESUME",
        Ok(HumanContinuationDecision::Replan) => "REPLAN",
        Ok(HumanContinuationDecision::Abort) => "ABORT",
        Ok(HumanContinuationDecision::Takeover) => "TAKEOVER",
        _ => "UNSPECIFIED",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn decide_continuation(spelling: &str) -> i32 {
        let args = vec!["decide".to_owned(), "hdr-1".to_owned(), spelling.to_owned()];
        match human_decision_request(&args).unwrap().payload {
            Some(human_decision_core_request::Payload::Decide(request)) => request.continuation,
            other => panic!("expected decide payload, got {other:?}"),
        }
    }

    #[test]
    fn decide_maps_takeover_spelling() {
        assert_eq!(
            decide_continuation("takeover"),
            HumanContinuationDecision::Takeover as i32
        );
        assert_eq!(
            decide_continuation("abort"),
            HumanContinuationDecision::Abort as i32
        );
        assert!(human_decision_request(&[
            "decide".to_owned(),
            "hdr-1".to_owned(),
            "seize".to_owned()
        ])
        .is_err());
    }

    #[test]
    fn continuation_name_renders_takeover() {
        assert_eq!(
            human_continuation_name(HumanContinuationDecision::Takeover as i32),
            "TAKEOVER"
        );
        assert_eq!(human_continuation_name(99), "UNSPECIFIED");
    }
}
