//! Post-admission / pre-Attempt recovery through the production dispatch
//! surface (TASK-0054, G5 B-RS).
//!
//! Black-box keepers over the real `agentd` execution composition:
//! `dispatch_execution` is the only entry point, a dropped `BackgroundJob`
//! is the crash, and a fresh `ExecutionComposition` over the same database
//! is the daemon restart (new cancel registry, new process tracker, new
//! connections — no in-memory state survives).
//!
//! RED → GREEN note: the `Resume` IPC variant, the resume dispatch, the
//! pre-admission Git preflight, the `AWAITING_RESUME` cancel posture, and
//! the boot scan do not exist before TASK-0054, so this file fails to
//! compile (and the pre-admission keepers fail behaviorally) on the old
//! tree.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use agentd::execution::{dispatch_execution, run_background, ExecutionComposition};
use ipc_contracts::proto::{
    execution_core_request, execution_core_response, BusinessErrorCode, CancelExecutionRequest,
    ExecutionCoreRequest, ExecutionCoreResponse, GetExecutionRequest, ResumeExecutionRequest,
    RunExecutionRequest,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

const SUCCESS_BODY: &str = r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"recovery fixture finished","session_id":"ffffffff-ffff-ffff-ffff-ffffffffffff"}'; exit 0
"#;

struct Sandbox {
    root: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    fixture: PathBuf,
}

impl Sandbox {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!(
            "agentd-recovery-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("data")).expect("data parent");
        let db = root.join("data").join("state.sqlite3");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let fixture = root.join("claude-fixture.sh");
        let mut file = std::fs::File::create(&fixture).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(SUCCESS_BODY.as_bytes())
            .expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fixture, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        Self {
            root,
            db,
            repo,
            fixture,
        }
    }

    fn composition(&self) -> ExecutionComposition {
        ExecutionComposition::new(self.db.clone(), Some(self.fixture.clone()))
    }

    fn seed_ready(&self, project: &str, task: &str) {
        let mut tasks =
            app_services::TaskService::open(&self.db, app_services::SystemClock).expect("open");
        let project_id = task_domain::ProjectId::new(project).unwrap();
        tasks
            .create_project(
                &task_domain::Project::new(project_id.clone(), format!("project {project}"))
                    .unwrap(),
            )
            .unwrap();
        // `task` itself is materialized READY by the accepted plan below.
        let parent_id = task_domain::TaskId::new(fixture_parent(task)).unwrap();
        tasks
            .create_task(
                &task_domain::Task::draft(
                    parent_id.clone(),
                    project_id,
                    format!("objective {task}"),
                )
                .unwrap(),
            )
            .unwrap();
        // Publication admits a decomposition only while the parent is doing
        // work (ADR-0027 TASK-0091 amendment).
        tasks.mark_ready(&parent_id).unwrap();
        seed_accepted_plan(&self.db, task);
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
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
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git");
    assert!(output.status.success(), "git {args:?} failed");
}

fn git_text(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(std::process::Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git text");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn fixture_parent(task: &str) -> String {
    format!("{task}-PARENT")
}

/// Publishes an accepted plan whose only child is `task`; acceptance
/// materializes it as a new READY Task under `<task>-PARENT` (TASK-0087).
fn seed_accepted_plan(db: &Path, task: &str) {
    let mut planning = app_services::PlanningService::open(db, app_services::SystemClock)
        .expect("open planning service");
    let scope = plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap();
    let branch = plan_domain::PlanBranchId::new(format!("fixture-plan-{task}")).unwrap();
    let planner =
        fake_planner::FakePlanner::for_single_leaf(plan_domain::PlanNodeId::new(task).unwrap());
    let request = plan_domain::DecompositionRequest {
        parent_task_ref: fixture_parent(task),
        base_plan_revision: 0,
        acceptance_criteria_refs: vec![],
        scope_bound: scope.clone(),
        evidence_refs: vec![plan_domain::EvidenceRef::new(format!("fixture-plan:{task}")).unwrap()],
        resolved_evidence: Vec::new(),
        depth_remaining: plan_domain::MAX_DECOMPOSITION_DEPTH,
        baseline_risk: plan_domain::RiskClass::Low,
        budget_remaining: plan_domain::PlanningBudget {
            calls_remaining: plan_domain::MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: plan_domain::MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    };
    let policy = app_services::PlanningPolicyContext {
        scope_bound: scope,
        baseline_risk: plan_domain::RiskClass::Low,
        affected_parent_criteria: vec![],
        parent_task_ref: fixture_parent(task),
        canonical_task_authority: false,
        lineage_fingerprints: vec![],
        risk_grant: None,
        risk_decision_id: None,
        policy_fingerprint: format!("policy:fixture:{task}"),
    };
    let outcome = planning
        .propose_decomposition(&planner, &branch, request, &policy)
        .expect("seed accepted plan");
    assert!(matches!(
        outcome,
        app_services::PlanningProposalOutcome::Accepted(_)
    ));
}

fn run_request(
    _sandbox: &Sandbox,
    task: &str,
    execution: &str,
    attempt: &str,
    repository_path: &str,
    base_revision: &str,
) -> RunExecutionRequest {
    RunExecutionRequest {
        task_id: task.to_owned(),
        execution_id: execution.to_owned(),
        attempt_id: attempt.to_owned(),
        repository_ref: format!("repo-local:{task}"),
        repository_path: repository_path.to_owned(),
        base_revision: base_revision.to_owned(),
    }
}

fn dispatch(
    composition: &ExecutionComposition,
    payload: execution_core_request::Payload,
) -> ExecutionCoreResponse {
    let (response, _) = dispatch_execution(
        composition,
        ExecutionCoreRequest {
            payload: Some(payload),
        },
    );
    response
}

fn error_code(response: &ExecutionCoreResponse) -> i32 {
    match &response.payload {
        Some(execution_core_response::Payload::Error(error)) => error.code,
        other => panic!("expected typed error, got {other:?}"),
    }
}

fn run_accepted(
    composition: &ExecutionComposition,
    req: RunExecutionRequest,
) -> agentd::execution::BackgroundJob {
    let (response, job) = dispatch_execution(
        composition,
        ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Run(req)),
        },
    );
    match &response.payload {
        Some(execution_core_response::Payload::Run(_)) => job.expect("accepted run yields a job"),
        other => panic!("expected accepted run, got {other:?}"),
    }
}

fn get_status(
    composition: &ExecutionComposition,
    execution: &str,
) -> ipc_contracts::proto::GetExecutionResponse {
    match &dispatch(
        composition,
        execution_core_request::Payload::Get(GetExecutionRequest {
            execution_id: execution.to_owned(),
        }),
    )
    .payload
    {
        Some(execution_core_response::Payload::Get(get)) => get.clone(),
        other => panic!("expected status, got {other:?}"),
    }
}

fn cancel(
    composition: &ExecutionComposition,
    execution: &str,
) -> ipc_contracts::proto::CancelExecutionResponse {
    match &dispatch(
        composition,
        execution_core_request::Payload::Cancel(CancelExecutionRequest {
            execution_id: execution.to_owned(),
        }),
    )
    .payload
    {
        Some(execution_core_response::Payload::Cancel(cancel)) => cancel.clone(),
        other => panic!("expected cancel, got {other:?}"),
    }
}

fn resume(
    composition: &ExecutionComposition,
    req: ResumeExecutionRequest,
) -> (
    ExecutionCoreResponse,
    Option<agentd::execution::BackgroundJob>,
) {
    dispatch_execution(
        composition,
        ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Resume(req)),
        },
    )
}

fn resume_request(execution: &str, repository_path: &str) -> ResumeExecutionRequest {
    ResumeExecutionRequest {
        execution_id: execution.to_owned(),
        repository_path: repository_path.to_owned(),
        repository_ref: String::new(),
        base_revision: String::new(),
    }
}

fn resume_request_full(
    execution: &str,
    repository_path: &str,
    repository_ref: &str,
    base_revision: &str,
) -> ResumeExecutionRequest {
    ResumeExecutionRequest {
        execution_id: execution.to_owned(),
        repository_path: repository_path.to_owned(),
        repository_ref: repository_ref.to_owned(),
        base_revision: base_revision.to_owned(),
    }
}

fn task_state(sandbox: &Sandbox, task: &str) -> String {
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
    store
        .get_task(&task_domain::TaskId::new(task).unwrap())
        .expect("read task")
        .expect("task exists")
        .state()
        .as_str()
        .to_owned()
}

fn attempt_outcomes(sandbox: &Sandbox, execution: &str) -> Vec<String> {
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
    store
        .get_execution(&agent_runtime::ExecutionId::new(execution).unwrap())
        .expect("read execution")
        .expect("execution exists")
        .attempts()
        .iter()
        .map(|attempt| match attempt.outcome() {
            None => "OPEN".to_owned(),
            Some(outcome) => format!("{outcome:?}").to_uppercase(),
        })
        .collect()
}

fn journal_types_for(sandbox: &Sandbox, task: &str) -> Vec<String> {
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
    store
        .list_events_for_task(&task_domain::TaskId::new(task).unwrap())
        .expect("read journal")
        .iter()
        .map(|event| event.fact.event_type.as_str().to_owned())
        .collect()
}

fn wait_until_terminal(sandbox: &Sandbox, execution: &str, task: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let outcomes = attempt_outcomes(sandbox, execution);
        let concluded = outcomes.iter().all(|outcome| outcome != "OPEN") && !outcomes.is_empty();
        if concluded {
            return;
        }
        if Instant::now() >= deadline {
            panic!(
                "execution {execution} never reached a terminal attempt: outcomes={outcomes:?} task={} journal={:?}",
                task_state(sandbox, task),
                journal_types_for(sandbox, task),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn pre_admission_refuses_a_non_repository_path_without_mutation() {
    let sandbox = Sandbox::new("bad-path");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let missing = sandbox.root.join("no-such-dir");

    let response = dispatch(
        &composition,
        execution_core_request::Payload::Run(run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &missing.to_string_lossy(),
            "HEAD",
        )),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidArgument as i32,
        "deterministic repo failure must refuse before admission, got {response:?}"
    );
    assert_eq!(task_state(&sandbox, "T-1"), "READY");
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
    assert!(
        store
            .get_execution(&agent_runtime::ExecutionId::new("E-1").unwrap())
            .expect("read")
            .is_none(),
        "refused run commits no execution"
    );
    assert_eq!(
        journal_types_for(&sandbox, "T-1"),
        vec!["task.created", "task.marked_ready"],
        "refused run appends no lifecycle facts"
    );
}

#[test]
fn pre_admission_refuses_an_unresolvable_revision_without_mutation() {
    let sandbox = Sandbox::new("bad-rev");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();

    let response = dispatch(
        &composition,
        execution_core_request::Payload::Run(run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "no-such-branch",
        )),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidArgument as i32,
        "deterministic revision failure must refuse before admission, got {response:?}"
    );
    assert_eq!(task_state(&sandbox, "T-1"), "READY");
    assert_eq!(
        journal_types_for(&sandbox, "T-1"),
        vec!["task.created", "task.marked_ready"],
    );
}

#[test]
fn stranded_entry_is_honest_across_restart_and_recovers_via_resume() {
    let sandbox = Sandbox::new("resume");
    sandbox.seed_ready("P-1", "T-1");

    // Production admission, then a crash before the background half runs:
    // the job is dropped, exactly like a daemon SIGKILL after COMMIT.
    let composition = sandbox.composition();
    let _crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(_crashed);
    drop(composition);

    // Restart: a fresh composition over the same database (new cancel
    // registry, new process tracker, new connections).
    let restarted = sandbox.composition();

    // Honest intermediate: RUNNING, no attempts, nothing live.
    let status = get_status(&restarted, "E-1");
    assert_eq!(status.task_state, "RUNNING");
    assert!(status.attempts.is_empty());
    assert!(!status.execution_live);
    assert!(status.workspace_path.is_empty());

    // cancel no longer claims ALREADY_CONCLUDED for a strand that never
    // recorded an attempt.
    let cancelled = cancel(&restarted, "E-1");
    assert!(!cancelled.cancel_requested);
    assert_eq!(
        cancelled.run_state, "AWAITING_RESUME",
        "got {:?}",
        cancelled
    );
    assert!(
        cancelled.detail.contains("E-1") && cancelled.detail.contains("resume"),
        "cancel names the recovery path, got {:?}",
        cancelled.detail
    );

    // The boot scan reports the strand for operator attention.
    let strands = agentd::execution::scan_pre_attempt_strands(&sandbox.db);
    assert_eq!(strands.len(), 1, "got {strands:?}");
    assert_eq!(strands[0].execution_id, "E-1");

    // Recovery through the production control plane: resume re-drives the
    // SAME execution with its reserved attempt on a detached background.
    // No lease exists, so the original workspace inputs travel again.
    let (response, job) = resume(
        &restarted,
        resume_request_full(
            "E-1",
            &sandbox.repo.to_string_lossy(),
            "repo-local:T-1",
            "HEAD",
        ),
    );
    let resumed = match &response.payload {
        Some(execution_core_response::Payload::Resume(resumed)) => resumed.clone(),
        other => panic!("expected resume acceptance, got {other:?}"),
    };
    assert_eq!(resumed.task_id, "T-1");
    assert_eq!(resumed.execution_id, "E-1");
    assert_eq!(resumed.attempt_id, "A-1");
    assert_eq!(resumed.task_state, "RUNNING");
    let job = job.expect("accepted resume yields a background job");
    let handle = std::thread::spawn(move || run_background(job));
    handle.join().expect("background joins");
    wait_until_terminal(&sandbox, "E-1", "T-1");

    // Exactly one attempt, concluded by the provider evidence; the Task
    // reaches VERIFYING (never provider-completed); the reservation is
    // consumed; lifecycle facts are exactly once.
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    assert_eq!(task_state(&sandbox, "T-1"), "VERIFYING");
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
    assert!(
        store
            .get_attempt_reservation(&agent_runtime::AttemptId::new("A-1").unwrap())
            .expect("read reservation")
            .is_none(),
        "reservation consumed exactly once"
    );
    let journal = journal_types_for(&sandbox, "T-1");
    assert_eq!(
        journal
            .iter()
            .filter(|kind| kind.as_str() == "attempt.created")
            .count(),
        1,
        "exactly one attempt.created, got {journal:?}"
    );
    assert!(
        journal.contains(&"workspace.created".to_owned()),
        "got {journal:?}"
    );

    // The strand is gone from the scan; cancel is honestly concluded.
    assert!(
        agentd::execution::scan_pre_attempt_strands(&sandbox.db).is_empty(),
        "recovered entry leaves the scan"
    );
    let again = cancel(&sandbox.composition(), "E-1");
    assert_eq!(again.run_state, "ALREADY_CONCLUDED");
}

#[test]
fn resume_reuses_an_allocated_lease_without_new_inputs() {
    let sandbox = Sandbox::new("resume-allocated");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    // Crash lands after lease allocation but before materialization: the
    // durable intent exists, Git is untouched.
    let execution_id = crashed.work_execution_id();
    assert_eq!(execution_id, "E-1");
    drop(crashed);
    {
        let root_dir = sandbox.db.parent().unwrap().join("workspaces");
        std::fs::create_dir_all(&root_dir).expect("managed root dir");
        let root = app_services::ManagedWorkspaceRoot::new(&root_dir).expect("managed root");
        let mut workspace = app_services::WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            root,
            app_services::SystemClock,
        )
        .expect("open workspace");
        workspace
            .allocate_workspace(&app_services::WorkspaceRequest {
                execution_id: agent_runtime::ExecutionId::new("E-1").unwrap(),
                repository_ref: workspace::RepositoryRef::new("repo-local:T-1").unwrap(),
                repository_path: sandbox.repo.to_string_lossy().into_owned(),
                base_revision: "HEAD".to_owned(),
            })
            .expect("allocate");
    }

    let restarted = sandbox.composition();
    let (response, job) = resume(
        &restarted,
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    assert!(
        matches!(
            &response.payload,
            Some(execution_core_response::Payload::Resume(_))
        ),
        "allocated lease stays resumable with the path alone, got {response:?}"
    );
    let handle = std::thread::spawn(move || run_background(job.expect("job")));
    handle.join().expect("background joins");
    wait_until_terminal(&sandbox, "E-1", "T-1");
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    assert_eq!(task_state(&sandbox, "T-1"), "VERIFYING");
}

#[test]
fn branch_only_strand_recovers_across_restart_with_reserved_attempt() {
    let sandbox = Sandbox::new("resume-branch-only");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);

    let (branch, base, worktree_ref) = {
        let root_dir = sandbox.db.parent().unwrap().join("workspaces");
        std::fs::create_dir_all(&root_dir).expect("managed root dir");
        let root = app_services::ManagedWorkspaceRoot::new(&root_dir).expect("managed root");
        let mut workspace = app_services::WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            root,
            app_services::SystemClock,
        )
        .expect("open workspace");
        let lease = workspace
            .allocate_workspace(&app_services::WorkspaceRequest {
                execution_id: agent_runtime::ExecutionId::new("E-1").unwrap(),
                repository_ref: workspace::RepositoryRef::new("repo-local:T-1").unwrap(),
                repository_path: sandbox.repo.to_string_lossy().into_owned(),
                base_revision: "HEAD".to_owned(),
            })
            .expect("allocate");
        (
            lease.branch().short_name().to_owned(),
            lease.base_revision().as_str().to_owned(),
            lease.worktree_ref().to_owned(),
        )
    };
    git(&sandbox.repo, &["branch", &branch, &base]);
    assert!(!Path::new(&worktree_ref).exists());

    let restarted = sandbox.composition();
    let (response, job) = resume(
        &restarted,
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    let resumed = match &response.payload {
        Some(execution_core_response::Payload::Resume(resumed)) => resumed,
        other => panic!("expected branch-only resume acceptance, got {other:?}"),
    };
    assert_eq!(
        resumed.attempt_id, "A-1",
        "reserved attempt identity is reused"
    );
    run_background(job.expect("accepted resume yields job"));
    wait_until_terminal(&sandbox, "E-1", "T-1");
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    assert_eq!(task_state(&sandbox, "T-1"), "VERIFYING");
    assert!(Path::new(&worktree_ref).is_dir());
    assert_eq!(
        git_text(Path::new(&worktree_ref), &["rev-parse", "HEAD"]),
        base
    );
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("store");
    assert!(
        store
            .get_attempt_reservation(&agent_runtime::AttemptId::new("A-1").unwrap())
            .expect("reservation")
            .is_none(),
        "same reserved attempt is consumed exactly once"
    );
    let journal = journal_types_for(&sandbox, "T-1");
    assert_eq!(
        journal
            .iter()
            .filter(|kind| kind.as_str() == "attempt.created")
            .count(),
        1
    );
    assert_eq!(
        journal
            .iter()
            .filter(|kind| kind.as_str() == "workspace.created")
            .count(),
        1
    );
}

#[test]
fn resume_refuses_ambiguous_branch_tip_before_accepting_background() {
    let sandbox = Sandbox::new("resume-branch-tip-refusal");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);

    let branch = {
        let root_dir = sandbox.db.parent().unwrap().join("workspaces");
        std::fs::create_dir_all(&root_dir).expect("managed root dir");
        let root = app_services::ManagedWorkspaceRoot::new(&root_dir).expect("managed root");
        let mut workspace = app_services::WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            root,
            app_services::SystemClock,
        )
        .expect("open workspace");
        workspace
            .allocate_workspace(&app_services::WorkspaceRequest {
                execution_id: agent_runtime::ExecutionId::new("E-1").unwrap(),
                repository_ref: workspace::RepositoryRef::new("repo-local:T-1").unwrap(),
                repository_path: sandbox.repo.to_string_lossy().into_owned(),
                base_revision: "HEAD".to_owned(),
            })
            .expect("allocate")
            .branch()
            .short_name()
            .to_owned()
    };
    git(
        &sandbox.repo,
        &["commit", "-q", "--allow-empty", "-m", "foreign-work"],
    );
    let wrong = git_text(&sandbox.repo, &["rev-parse", "HEAD"]);
    git(&sandbox.repo, &["branch", &branch, &wrong]);

    let restarted = sandbox.composition();
    let (response, job) = resume(
        &restarted,
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    assert!(
        job.is_none(),
        "deterministically ambiguous workspace must spawn nothing"
    );
    assert!(
        matches!(
            &response.payload,
            Some(execution_core_response::Payload::Error(_))
        ),
        "ambiguous branch tip must refuse synchronously, got {response:?}"
    );
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), Vec::<String>::new());
}

#[test]
fn resume_reuses_a_ready_lease_without_rematerializing() {
    let sandbox = Sandbox::new("resume-ready");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);
    // Crash lands after the lease is Ready but before the attempt: the
    // full Git state exists, only the provider half is missing.
    let lease_id = {
        let root_dir = sandbox.db.parent().unwrap().join("workspaces");
        std::fs::create_dir_all(&root_dir).expect("managed root dir");
        let root = app_services::ManagedWorkspaceRoot::new(&root_dir).expect("managed root");
        let mut workspace = app_services::WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            root,
            app_services::SystemClock,
        )
        .expect("open workspace");
        let request = app_services::WorkspaceRequest {
            execution_id: agent_runtime::ExecutionId::new("E-1").unwrap(),
            repository_ref: workspace::RepositoryRef::new("repo-local:T-1").unwrap(),
            repository_path: sandbox.repo.to_string_lossy().into_owned(),
            base_revision: "HEAD".to_owned(),
        };
        let lease = workspace.allocate_workspace(&request).expect("allocate");
        workspace
            .materialize_allocated(&lease, &sandbox.repo.to_string_lossy())
            .expect("materialize");
        let ready = workspace
            .finalize_ready(lease.id(), &sandbox.repo.to_string_lossy())
            .expect("finalize");
        ready.id().clone()
    };

    let restarted = sandbox.composition();
    let (response, job) = resume(
        &restarted,
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    assert!(
        matches!(
            &response.payload,
            Some(execution_core_response::Payload::Resume(_))
        ),
        "ready lease stays resumable, got {response:?}"
    );
    let handle = std::thread::spawn(move || run_background(job.expect("job")));
    handle.join().expect("background joins");
    wait_until_terminal(&sandbox, "E-1", "T-1");
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    // Exactly one workspace.created across the crash: finalization is
    // idempotent, never duplicated by recovery.
    let journal = journal_types_for(&sandbox, "T-1");
    assert_eq!(
        journal
            .iter()
            .filter(|kind| kind.as_str() == "workspace.created")
            .count(),
        1,
        "got {journal:?}"
    );
    let _ = lease_id;
}

#[test]
fn resume_refuses_disagreeing_inputs_against_a_recorded_lease() {
    let sandbox = Sandbox::new("resume-disagree");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);
    {
        let root_dir = sandbox.db.parent().unwrap().join("workspaces");
        std::fs::create_dir_all(&root_dir).expect("managed root dir");
        let root = app_services::ManagedWorkspaceRoot::new(&root_dir).expect("managed root");
        let mut workspace = app_services::WorkspaceService::open(
            &sandbox.db,
            git_cli::GitCli::new(),
            root,
            app_services::SystemClock,
        )
        .expect("open workspace");
        workspace
            .allocate_workspace(&app_services::WorkspaceRequest {
                execution_id: agent_runtime::ExecutionId::new("E-1").unwrap(),
                repository_ref: workspace::RepositoryRef::new("repo-local:T-1").unwrap(),
                repository_path: sandbox.repo.to_string_lossy().into_owned(),
                base_revision: "HEAD".to_owned(),
            })
            .expect("allocate");
    }

    // A different repository spelling for the same execution refuses
    // before any mutation (exact-string agreement, fail closed).
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request_full(
            "E-1",
            &sandbox.repo.to_string_lossy(),
            "repo-local:other-spelling",
            "HEAD",
        ),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidArgument as i32,
        "got {response:?}"
    );
    assert!(job.is_none());
    // A resolvable-but-different revision refuses the same way (bases
    // are immutable). An empty commit gives a second resolvable OID.
    git(
        &sandbox.repo,
        &["commit", "-q", "--allow-empty", "-m", "second"],
    );
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request_full(
            "E-1",
            &sandbox.repo.to_string_lossy(),
            "repo-local:T-1",
            "HEAD",
        ),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidArgument as i32,
        "moved HEAD must not rebind the recorded base, got {response:?}"
    );
    assert!(job.is_none());
    assert_eq!(task_state(&sandbox, "T-1"), "RUNNING");
}

#[test]
fn resume_refuses_an_attempted_execution_without_mutation() {
    let sandbox = Sandbox::new("resume-attempted");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let job = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    let handle = std::thread::spawn(move || run_background(job));
    handle.join().expect("background joins");
    wait_until_terminal(&sandbox, "E-1", "T-1");
    assert_eq!(task_state(&sandbox, "T-1"), "VERIFYING");

    let journal_before = journal_types_for(&sandbox, "T-1");
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidTransition as i32,
        "attempted executions are out of pre-attempt scope, got {response:?}"
    );
    assert!(job.is_none(), "refused resume spawns nothing");
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    assert_eq!(
        journal_types_for(&sandbox, "T-1"),
        journal_before,
        "refused resume appends no facts"
    );
}

#[test]
fn resume_refuses_an_unknown_execution() {
    let sandbox = Sandbox::new("resume-missing");
    sandbox.seed_ready("P-1", "T-1");
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request("E-ghost", &sandbox.repo.to_string_lossy()),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::NotFound as i32,
        "got {response:?}"
    );
    assert!(job.is_none());
}

#[test]
fn resume_without_inputs_names_what_is_missing() {
    let sandbox = Sandbox::new("resume-inputs");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);

    // No lease exists, so the workspace inputs must travel again: each
    // missing half refuses typed with no mutation.
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidArgument as i32,
        "missing repository ref must refuse, got {response:?}"
    );
    assert!(job.is_none());
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request_full("E-1", &sandbox.repo.to_string_lossy(), "repo-local:T-1", ""),
    );
    assert!(
        matches!(
            &response.payload,
            Some(execution_core_response::Payload::Resume(_))
        ),
        "blank revision means HEAD when no lease exists, got {response:?}"
    );
    assert!(job.is_some());
    assert_eq!(task_state(&sandbox, "T-1"), "RUNNING");
}

#[test]
fn resume_refuses_a_blocked_task_until_the_task_resumes() {
    let sandbox = Sandbox::new("resume-blocked");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);
    let mut tasks =
        app_services::TaskService::open(&sandbox.db, app_services::SystemClock).expect("open");
    tasks
        .block(
            &task_domain::TaskId::new("T-1").unwrap(),
            "operator hold during incident",
        )
        .expect("RUNNING → BLOCKED");

    let (response, job) = resume(
        &sandbox.composition(),
        resume_request("E-1", &sandbox.repo.to_string_lossy()),
    );
    assert_eq!(
        error_code(&response),
        BusinessErrorCode::InvalidTransition as i32,
        "got {response:?}"
    );
    assert!(job.is_none());
    if let Some(execution_core_response::Payload::Error(error)) = response.payload {
        assert!(
            error.message.contains("BLOCKED"),
            "refusal names the actual state, got {:?}",
            error.message
        );
    }

    tasks
        .start(&task_domain::TaskId::new("T-1").unwrap())
        .expect("BLOCKED → RUNNING");
    let (response, job) = resume(
        &sandbox.composition(),
        resume_request_full(
            "E-1",
            &sandbox.repo.to_string_lossy(),
            "repo-local:T-1",
            "HEAD",
        ),
    );
    assert!(
        matches!(
            &response.payload,
            Some(execution_core_response::Payload::Resume(_))
        ),
        "task resume re-arms recovery, got {response:?}"
    );
    let handle = std::thread::spawn(move || run_background(job.expect("job")));
    handle.join().expect("background joins");
    wait_until_terminal(&sandbox, "E-1", "T-1");
    assert_eq!(task_state(&sandbox, "T-1"), "VERIFYING");
}

#[test]
fn concurrent_resumes_converge_on_exactly_one_attempt() {
    let sandbox = Sandbox::new("resume-race");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);

    // Eight recoveries race on one barrier: every check passes for every
    // racer (reads only), then exactly one background may consume the
    // reservation and run the provider.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let shared = std::sync::Arc::new(sandbox.composition());
    let mut handles = Vec::new();
    for _ in 0..8 {
        let barrier = barrier.clone();
        let composition = shared.clone();
        let repo = sandbox.repo.to_string_lossy().into_owned();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let (response, job) = resume(
                &composition,
                resume_request_full("E-1", &repo, "repo-local:T-1", "HEAD"),
            );
            match &response.payload {
                Some(execution_core_response::Payload::Resume(_)) => {
                    run_background(job.expect("accepted resume yields a job"));
                    "resumed"
                }
                Some(execution_core_response::Payload::Error(_)) => "refused",
                other => panic!("unexpected resume reply: {other:?}"),
            }
        }));
    }
    let mut resumed = 0;
    for handle in handles {
        if handle.join().expect("racer joins") == "resumed" {
            resumed += 1;
        }
    }
    assert!(resumed >= 1, "at least one racer must drive recovery");
    wait_until_terminal(&sandbox, "E-1", "T-1");
    assert_eq!(
        attempt_outcomes(&sandbox, "E-1"),
        vec!["SUCCEEDED"],
        "exactly one attempt despite {resumed} resumed racers"
    );
    let journal = journal_types_for(&sandbox, "T-1");
    assert_eq!(
        journal
            .iter()
            .filter(|kind| kind.as_str() == "attempt.created")
            .count(),
        1,
        "exactly one attempt.created, got {journal:?}"
    );
}

#[test]
fn resume_is_idempotent_after_recovery() {
    let sandbox = Sandbox::new("resume-idempotent");
    sandbox.seed_ready("P-1", "T-1");
    let composition = sandbox.composition();
    let crashed = run_accepted(
        &composition,
        run_request(
            &sandbox,
            "T-1",
            "E-1",
            "A-1",
            &sandbox.repo.to_string_lossy(),
            "HEAD",
        ),
    );
    drop(crashed);
    let restarted = sandbox.composition();
    let (response, job) = resume(
        &restarted,
        resume_request_full(
            "E-1",
            &sandbox.repo.to_string_lossy(),
            "repo-local:T-1",
            "HEAD",
        ),
    );
    assert!(matches!(
        &response.payload,
        Some(execution_core_response::Payload::Resume(_))
    ));
    let handle = std::thread::spawn(move || run_background(job.expect("job")));
    handle.join().expect("background joins");
    wait_until_terminal(&sandbox, "E-1", "T-1");

    let journal_before = journal_types_for(&sandbox, "T-1");
    for _ in 0..2 {
        let (response, job) = resume(
            &sandbox.composition(),
            resume_request_full(
                "E-1",
                &sandbox.repo.to_string_lossy(),
                "repo-local:T-1",
                "HEAD",
            ),
        );
        assert_eq!(
            error_code(&response),
            BusinessErrorCode::InvalidTransition as i32
        );
        assert!(job.is_none());
    }
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    assert_eq!(journal_types_for(&sandbox, "T-1"), journal_before);
}
