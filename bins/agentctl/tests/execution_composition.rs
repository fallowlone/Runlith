//! G5 execution composition over real IPC (TASK-0047).
//!
//! In-process `agentd` on a sandbox endpoint/database plus the real
//! `agentctl` client library (`execution_request` / `business_request`):
//! run → status → cancel through versioned IPC, the trusted application
//! services, workspace leases, and a fixture provider executable standing
//! in for the real Claude CLI (same `ExecutionPort` seam, same argv/cwd
//! mechanics, no quota). The fixture multiplexes on the prompt text (the
//! final argv): objectives containing SLOW sleep, BADJSON print garbage,
//! EXIT3 fail, and anything else reports structured success whose result
//! text claims completion — so the completion-authority test proves the
//! claim alone never completes a Task.
//!
//! Unix-only (fixture scripts, UDS transport). Test-owned sandbox
//! endpoint/database only; production directories are never touched.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ipc_contracts::proto::{
    execution_core_request, execution_core_response, task_core_request, task_core_response,
    CancelExecutionRequest, CreateProjectRequest, CreateTaskRequest, ExecutionCoreRequest,
    ExecutionCoreResponse, GetExecutionRequest, GetTaskRequest, LifecycleRequest,
    RunExecutionRequest, TaskCoreRequest, TaskCoreResponse, TaskLifecycleOp,
};
use ipc_transport::platform::unix::{UnixEndpoint, SOCKET_FILE_NAME};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Sandbox {
    root: PathBuf,
    endpoint: UnixEndpoint,
    db: PathBuf,
    repo: PathBuf,
}

impl Sandbox {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!(
            "g5-exec-{}-{}-{suite}-{id}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("sandbox root");
        // Neither root/db nor root/run is pre-created: the daemon owns
        // its 0700 data directory (creating it) and its 0700 endpoint
        // directory, failing closed on unexpected pre-existing objects.
        let endpoint = UnixEndpoint {
            socket_path: root.join("run").join(SOCKET_FILE_NAME),
            runtime_dir: root.join("run"),
        };
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("sandbox repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        Self {
            db: root.join("db").join("state.sqlite3"),
            root,
            endpoint,
            repo,
        }
    }

    /// Writes the multiplexing fixture provider and returns its path.
    fn provider_fixture(&self) -> PathBuf {
        let path = self.root.join("claude-fixture.sh");
        let mut file = std::fs::File::create(&path).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(
            br#"LAST=""; for P; do LAST="$P"; done
case "$LAST" in
  *SLOW*) sleep 45; printf '{"type":"result","subtype":"success","is_error":false,"result":"slow work finished","session_id":"bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"}'; exit 0 ;;
  *BADJSON*) printf 'this is not json{{{'; exit 0 ;;
  *EXIT3*) echo 'provider exploded' >&2; exit 3 ;;
  *) printf '{"type":"result","subtype":"success","is_error":false,"result":"fixture completed the work","session_id":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"}'; exit 0 ;;
esac
"#,
        )
        .expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        path
    }

    fn spawn_daemon(
        &self,
        provider: Option<PathBuf>,
    ) -> tokio::task::JoinHandle<Result<agentd::RunOutcome, agentd::DaemonError>> {
        let endpoint = self.endpoint.clone();
        let db = self.db.clone();
        tokio::spawn(async move {
            let config = agentd::DaemonConfig::with_execution(db);
            let config = match provider {
                Some(exe) => config.with_provider_executable(exe),
                None => config,
            };
            agentd::run_on_endpoint(&endpoint, config).await
        })
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    assert!(output.status.success(), "git {args:?} failed");
}

fn inert_spawn() -> std::io::Result<std::process::Child> {
    Command::new("true").spawn()
}

async fn wait_ready(endpoint: &UnixEndpoint) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if agentctl::connect_and_handshake(endpoint).await.is_ok() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("daemon never became ready");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn business(endpoint: &UnixEndpoint, request: TaskCoreRequest) -> TaskCoreResponse {
    agentctl::business_request(endpoint, inert_spawn, request)
        .await
        .expect("business exchange")
}

async fn execution(
    endpoint: &UnixEndpoint,
    request: ExecutionCoreRequest,
) -> ExecutionCoreResponse {
    agentctl::execution_request(endpoint, inert_spawn, request)
        .await
        .expect("execution exchange")
}

fn create_project(id: &str) -> TaskCoreRequest {
    TaskCoreRequest {
        payload: Some(task_core_request::Payload::CreateProject(
            CreateProjectRequest {
                id: id.to_owned(),
                name: format!("project {id}"),
            },
        )),
    }
}

fn create_task(id: &str, project: &str, objective: &str) -> TaskCoreRequest {
    TaskCoreRequest {
        payload: Some(task_core_request::Payload::CreateTask(CreateTaskRequest {
            id: id.to_owned(),
            project_id: project.to_owned(),
            objective: objective.to_owned(),
        })),
    }
}

fn lifecycle(task: &str, op: TaskLifecycleOp) -> TaskCoreRequest {
    TaskCoreRequest {
        payload: Some(task_core_request::Payload::Lifecycle(LifecycleRequest {
            task_id: task.to_owned(),
            op: op as i32,
            reason: String::new(),
        })),
    }
}

async fn seed_ready(endpoint: &UnixEndpoint, project: &str, task: &str, objective: &str) {
    let created = business(endpoint, create_project(project)).await;
    match &created.payload {
        Some(task_core_response::Payload::CreateProject(_)) => {}
        // Idempotent seeding: the second task of one test reuses its project.
        Some(task_core_response::Payload::Error(error))
            if error.code == ipc_contracts::proto::BusinessErrorCode::AlreadyExists as i32 => {}
        other => panic!("seed project, got {created:?} ({other:?})"),
    }
    let created = business(endpoint, create_task(task, project, objective)).await;
    assert!(
        matches!(
            created.payload,
            Some(task_core_response::Payload::CreateTask(_))
        ),
        "seed task, got {created:?}"
    );
    let ready = business(endpoint, lifecycle(task, TaskLifecycleOp::MarkReady)).await;
    assert!(
        matches!(
            ready.payload,
            Some(task_core_response::Payload::Lifecycle(_))
        ),
        "seed ready, got {ready:?}"
    );
}

async fn task_state(endpoint: &UnixEndpoint, task: &str) -> String {
    let response = business(
        endpoint,
        TaskCoreRequest {
            payload: Some(task_core_request::Payload::GetTask(GetTaskRequest {
                id: task.to_owned(),
            })),
        },
    )
    .await;
    match response.payload {
        Some(task_core_response::Payload::GetTask(get)) => get.task.expect("task dto").state,
        other => panic!("expected get_task, got {other:?}"),
    }
}

fn run_req(sandbox: &Sandbox, task: &str, execution: &str, attempt: &str) -> ExecutionCoreRequest {
    ExecutionCoreRequest {
        payload: Some(execution_core_request::Payload::Run(RunExecutionRequest {
            task_id: task.to_owned(),
            execution_id: execution.to_owned(),
            attempt_id: attempt.to_owned(),
            repository_ref: "repo-local:composition".to_owned(),
            repository_path: sandbox.repo.to_string_lossy().into_owned(),
            base_revision: "HEAD".to_owned(),
        })),
    }
}

fn get_req(execution: &str) -> ExecutionCoreRequest {
    ExecutionCoreRequest {
        payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
            execution_id: execution.to_owned(),
        })),
    }
}

fn cancel_req(execution: &str) -> ExecutionCoreRequest {
    ExecutionCoreRequest {
        payload: Some(execution_core_request::Payload::Cancel(
            CancelExecutionRequest {
                execution_id: execution.to_owned(),
            },
        )),
    }
}

/// Polls `status` until `wanted` returns true for the response, or the
/// budget expires. Returns the final response.
async fn poll_status(
    endpoint: &UnixEndpoint,
    execution_id: &str,
    budget: Duration,
    wanted: impl Fn(&ExecutionCoreResponse) -> bool,
) -> ExecutionCoreResponse {
    let deadline = Instant::now() + budget;
    loop {
        let response = execution(endpoint, get_req(execution_id)).await;
        if wanted(&response) {
            return response;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for status of {execution_id}; last: {response:?}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn get_payload(response: &ExecutionCoreResponse) -> ipc_contracts::proto::GetExecutionResponse {
    match &response.payload {
        Some(execution_core_response::Payload::Get(get)) => get.clone(),
        other => panic!("expected get, got {other:?}"),
    }
}

fn error_code(response: &ExecutionCoreResponse) -> i32 {
    match &response.payload {
        Some(execution_core_response::Payload::Error(error)) => error.code,
        other => panic!("expected error, got {other:?}"),
    }
}

#[tokio::test]
async fn success_leaves_task_verifying_never_completed() {
    let sandbox = Sandbox::new("authority");
    let daemon = sandbox.spawn_daemon(Some(sandbox.provider_fixture()));
    wait_ready(&sandbox.endpoint).await;
    // The fixture's result text claims completion ("fixture completed
    // the work"): the claim alone must never complete the Task.
    seed_ready(&sandbox.endpoint, "P-1", "T-1", "do the thing").await;

    let started = execution(&sandbox.endpoint, run_req(&sandbox, "T-1", "E-1", "A-1")).await;
    match &started.payload {
        Some(execution_core_response::Payload::Run(run)) => {
            assert_eq!(run.task_state, "RUNNING");
        }
        other => panic!("expected run accepted, got {other:?}"),
    }
    let status = poll_status(
        &sandbox.endpoint,
        "E-1",
        Duration::from_secs(60),
        |response| {
            let get = get_payload(response);
            get.attempts.iter().any(|attempt| attempt.outcome != "OPEN")
        },
    )
    .await;
    let get = get_payload(&status);
    assert_eq!(
        get.task_state, "VERIFYING",
        "provider success submits for verification"
    );
    assert_eq!(get.attempts.len(), 1);
    assert_eq!(get.attempts[0].outcome, "SUCCEEDED");
    assert_eq!(get.attempts[0].provider_ref, "claude-code");
    assert!(
        get.attempts[0].detail.contains("claude:session:"),
        "opaque session ref travels as evidence, got {:?}",
        get.attempts[0].detail
    );
    assert!(!get.execution_live);
    assert!(!get.workspace_path.is_empty(), "lease path is observable");
    assert!(
        Path::new(&get.workspace_path).is_dir(),
        "lease worktree survives the cycle (teardown stays explicit)"
    );
    // Completion authority: the Task is VERIFYING — never COMPLETED on
    // provider say-so. The operator verifies explicitly via task IPC.
    assert_eq!(task_state(&sandbox.endpoint, "T-1").await, "VERIFYING");

    // The journal carries the canonical runtime facts (plus the task
    // lifecycle facts); raw provider JSON never enters as events.
    let store = storage_sqlite::SqliteStore::open(sandbox.db.clone()).expect("open test db");
    let history = store.list_events_ordered().expect("journal");
    let kinds: Vec<String> = history
        .iter()
        .map(|event| event.fact.event_type.to_string())
        .collect();
    for expected in ["execution.created", "attempt.created", "attempt.concluded"] {
        assert!(
            kinds.iter().any(|kind| kind == expected),
            "journal must carry {expected}, got {kinds:?}"
        );
    }

    daemon.abort();
}

#[tokio::test]
async fn malformed_output_is_attempt_failure_never_task_failure() {
    let sandbox = Sandbox::new("malformed");
    let daemon = sandbox.spawn_daemon(Some(sandbox.provider_fixture()));
    wait_ready(&sandbox.endpoint).await;
    seed_ready(&sandbox.endpoint, "P-1", "T-1", "work BADJSON work").await;

    execution(&sandbox.endpoint, run_req(&sandbox, "T-1", "E-1", "A-1")).await;
    let status = poll_status(
        &sandbox.endpoint,
        "E-1",
        Duration::from_secs(60),
        |response| {
            get_payload(response)
                .attempts
                .iter()
                .any(|attempt| attempt.outcome != "OPEN")
        },
    )
    .await;
    let get = get_payload(&status);
    assert_eq!(get.attempts[0].outcome, "FAILED");
    assert!(
        get.attempts[0].detail.contains("malformed_output"),
        "typed failure survives to status, got {:?}",
        get.attempts[0].detail
    );
    assert_eq!(
        task_state(&sandbox.endpoint, "T-1").await,
        "RUNNING",
        "provider failure is attempt failure, never task failure"
    );
    daemon.abort();
}

#[tokio::test]
async fn nonzero_exit_is_attempt_failure() {
    let sandbox = Sandbox::new("exit3");
    let daemon = sandbox.spawn_daemon(Some(sandbox.provider_fixture()));
    wait_ready(&sandbox.endpoint).await;
    seed_ready(&sandbox.endpoint, "P-1", "T-1", "work EXIT3 work").await;

    execution(&sandbox.endpoint, run_req(&sandbox, "T-1", "E-1", "A-1")).await;
    let status = poll_status(
        &sandbox.endpoint,
        "E-1",
        Duration::from_secs(60),
        |response| {
            get_payload(response)
                .attempts
                .iter()
                .any(|attempt| attempt.outcome != "OPEN")
        },
    )
    .await;
    let get = get_payload(&status);
    assert_eq!(get.attempts[0].outcome, "FAILED");
    assert!(
        get.attempts[0].detail.contains('3'),
        "exit evidence survives, got {:?}",
        get.attempts[0].detail
    );
    daemon.abort();
}

#[tokio::test]
async fn cancel_reaches_only_the_target_execution() {
    let sandbox = Sandbox::new("cancel");
    let daemon = sandbox.spawn_daemon(Some(sandbox.provider_fixture()));
    wait_ready(&sandbox.endpoint).await;
    seed_ready(&sandbox.endpoint, "P-1", "T-slow", "slow work SLOW please").await;
    seed_ready(&sandbox.endpoint, "P-1", "T-fast", "fast work").await;

    execution(
        &sandbox.endpoint,
        run_req(&sandbox, "T-slow", "E-slow", "A-slow"),
    )
    .await;
    execution(
        &sandbox.endpoint,
        run_req(&sandbox, "T-fast", "E-fast", "A-fast"),
    )
    .await;
    // The fast execution concludes on its own while the slow one runs.
    let fast = poll_status(
        &sandbox.endpoint,
        "E-fast",
        Duration::from_secs(60),
        |response| {
            get_payload(response)
                .attempts
                .iter()
                .any(|attempt| attempt.outcome != "OPEN")
        },
    )
    .await;
    assert_eq!(get_payload(&fast).attempts[0].outcome, "SUCCEEDED");

    // Wait until the slow run is provably live, then cancel exactly it.
    poll_status(
        &sandbox.endpoint,
        "E-slow",
        Duration::from_secs(60),
        |response| {
            let get = get_payload(response);
            get.execution_live && !get.attempts.is_empty()
        },
    )
    .await;
    let cancelled = execution(&sandbox.endpoint, cancel_req("E-slow")).await;
    match &cancelled.payload {
        Some(execution_core_response::Payload::Cancel(cancel)) => {
            assert!(cancel.cancel_requested);
            assert_eq!(cancel.run_state, "LIVE_SIGNALLED");
        }
        other => panic!("expected cancel accepted, got {other:?}"),
    }
    let slow = poll_status(
        &sandbox.endpoint,
        "E-slow",
        Duration::from_secs(60),
        |response| {
            get_payload(response)
                .attempts
                .iter()
                .any(|attempt| attempt.outcome != "OPEN")
        },
    )
    .await;
    assert_eq!(get_payload(&slow).attempts[0].outcome, "CANCELLED");
    assert_eq!(
        task_state(&sandbox.endpoint, "T-slow").await,
        "RUNNING",
        "cancel is not completion"
    );
    // The unrelated fast execution is untouched by the cancel.
    let fast_again = execution(&sandbox.endpoint, get_req("E-fast")).await;
    assert_eq!(get_payload(&fast_again).attempts[0].outcome, "SUCCEEDED");
    // Repeat cancel is idempotent, not an error.
    let again = execution(&sandbox.endpoint, cancel_req("E-slow")).await;
    match &again.payload {
        Some(execution_core_response::Payload::Cancel(cancel)) => {
            assert!(!cancel.cancel_requested);
            assert_eq!(cancel.run_state, "ALREADY_CONCLUDED");
        }
        other => panic!("expected idempotent cancel, got {other:?}"),
    }
    daemon.abort();
}

#[tokio::test]
async fn missing_workspace_fails_closed_without_fallback() {
    let sandbox = Sandbox::new("noworkspace");
    let daemon = sandbox.spawn_daemon(Some(sandbox.provider_fixture()));
    wait_ready(&sandbox.endpoint).await;
    seed_ready(&sandbox.endpoint, "P-1", "T-1", "do the thing").await;

    // A repository path that is not a Git repository: entry commits
    // (RUNNING + execution), the background lease refuses, and nothing
    // ever falls back to the daemon cwd, $HOME, or another workspace.
    let mut bad = run_req(&sandbox, "T-1", "E-1", "A-1");
    if let Some(execution_core_request::Payload::Run(run)) = &mut bad.payload {
        run.repository_path = sandbox
            .root
            .join("not-a-repo")
            .to_string_lossy()
            .into_owned();
    }
    let started = execution(&sandbox.endpoint, bad).await;
    assert!(
        matches!(
            started.payload,
            Some(execution_core_response::Payload::Run(_))
        ),
        "entry commits before the lease step, got {started:?}"
    );
    let status = poll_status(
        &sandbox.endpoint,
        "E-1",
        Duration::from_secs(60),
        |response| !get_payload(response).execution_live,
    )
    .await;
    let get = get_payload(&status);
    assert!(
        get.attempts.is_empty(),
        "lease refusal creates no attempt, got {get:?}"
    );
    assert!(
        get.workspace_path.is_empty(),
        "no lease means no workspace path, got {get:?}"
    );
    assert_eq!(
        task_state(&sandbox.endpoint, "T-1").await,
        "RUNNING",
        "honest recoverable intermediate, never silent success"
    );
    daemon.abort();
}

#[tokio::test]
async fn run_rejects_a_task_that_is_not_ready() {
    let sandbox = Sandbox::new("notready");
    let daemon = sandbox.spawn_daemon(Some(sandbox.provider_fixture()));
    wait_ready(&sandbox.endpoint).await;
    // Seed without mark_ready: the task stays DRAFT.
    business(&sandbox.endpoint, create_project("P-1")).await;
    business(&sandbox.endpoint, create_task("T-1", "P-1", "do the thing")).await;

    let response = execution(&sandbox.endpoint, run_req(&sandbox, "T-1", "E-1", "A-1")).await;
    assert_eq!(
        error_code(&response),
        ipc_contracts::proto::BusinessErrorCode::InvalidTransition as i32
    );
    // Rejected entry commits nothing: status finds no execution.
    let status = execution(&sandbox.endpoint, get_req("E-1")).await;
    assert_eq!(
        error_code(&status),
        ipc_contracts::proto::BusinessErrorCode::NotFound as i32
    );
    daemon.abort();
}

#[tokio::test]
async fn missing_provider_fails_fast_without_mutation() {
    let sandbox = Sandbox::new("noprov");
    let daemon = sandbox.spawn_daemon(Some(sandbox.root.join("no-such-binary")));
    wait_ready(&sandbox.endpoint).await;
    seed_ready(&sandbox.endpoint, "P-1", "T-1", "do the thing").await;

    let response = execution(&sandbox.endpoint, run_req(&sandbox, "T-1", "E-1", "A-1")).await;
    assert_eq!(
        error_code(&response),
        ipc_contracts::proto::BusinessErrorCode::ProviderUnavailable as i32
    );
    let status = execution(&sandbox.endpoint, get_req("E-1")).await;
    assert_eq!(
        error_code(&status),
        ipc_contracts::proto::BusinessErrorCode::NotFound as i32
    );
    daemon.abort();
}

#[tokio::test]
async fn control_only_daemon_reports_execution_unavailable() {
    let sandbox = Sandbox::new("nounavailable");
    let endpoint = sandbox.endpoint.clone();
    let daemon = tokio::spawn(async move {
        agentd::run_on_endpoint(
            &endpoint,
            agentd::DaemonConfig {
                server_config: ipc_control::ServerConfig::g5_default(),
                database_path: None,
                provider_executable: None,
            },
        )
        .await
    });
    wait_ready(&sandbox.endpoint).await;
    // v2 handshake succeeds (version gate passes), handling answers
    // typed-unavailable without touching storage (none exists here).
    let response = execution(&sandbox.endpoint, get_req("E-1")).await;
    assert_eq!(
        error_code(&response),
        ipc_contracts::proto::BusinessErrorCode::Internal as i32
    );
    daemon.abort();
}
