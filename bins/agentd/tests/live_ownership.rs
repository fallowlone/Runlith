//! TASK-0056 (B-CR) keepers: exclusive live-run ownership on ONE shared
//! production-style [`ExecutionComposition`].
//!
//! The TASK-0054 race test gave every racer its own composition (its own
//! `CancelRegistry`), so N resumes could all report acceptance without ever
//! colliding. These keepers share ONE composition across all racers — the
//! production daemon shape (`agentd/src/lib.rs` builds one composition for
//! every connection) — with a slow/blockable provider fixture so the winner
//! stays live long enough to inspect status, cancellation, and registry
//! ownership.
//!
//! RED expectation on the pre-TASK-0056 tree: every racer reports
//! `Resume` acceptance; the losers' background exits unregister the winner;
//! `status` reports `live=false` and `cancel` reports `NO_LIVE_RUN` while
//! the provider still runs to success.

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

/// Fast provider double: prints a Claude-shaped success payload, exits 0.
const SUCCESS_BODY: &str = r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"live-ownership fixture finished","session_id":"ffffffff-ffff-ffff-ffff-ffffffffffff"}'; exit 0
"#;

/// Slow provider double: sleeps so the winner stays live while the test
/// inspects status/cancel, then observes cancellation via the composition
/// flag (kill of the owned tree) like the real CLI supervision.
const SLOW_BODY: &str = "while :; do :; done\n";

struct Sandbox {
    root: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    fast: PathBuf,
    slow: PathBuf,
}

impl Sandbox {
    fn new(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = PathBuf::from("/tmp").join(format!(
            "agentd-liveown-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("data")).expect("data parent");
        let db = root.join("data").join("state.sqlite3");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let fast = fixture(&root, "fast-claude.sh", SUCCESS_BODY);
        let slow = fixture(&root, "slow-claude.sh", SLOW_BODY);
        Self {
            root,
            db,
            repo,
            fast,
            slow,
        }
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
        let task_id = task_domain::TaskId::new(task).unwrap();
        tasks
            .create_task(
                &task_domain::Task::draft(task_id.clone(), project_id, format!("objective {task}"))
                    .unwrap(),
            )
            .unwrap();
        tasks.mark_ready(&task_id).unwrap();
        seed_accepted_plan(&self.db, task);
    }

    fn repo_str(&self) -> String {
        self.repo.to_string_lossy().into_owned()
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

fn fixture(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).expect("fixture file");
    file.write_all(b"#!/bin/sh\n").expect("shebang");
    file.write_all(body.as_bytes()).expect("fixture body");
    drop(file);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

fn seed_accepted_plan(db: &Path, task: &str) {
    let mut planning = app_services::PlanningService::open(db, app_services::SystemClock)
        .expect("open planning service");
    let scope = plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap();
    let branch = plan_domain::PlanBranchId::new(format!("fixture-plan-{task}")).unwrap();
    let planner =
        fake_planner::FakePlanner::for_single_leaf(plan_domain::PlanNodeId::new(task).unwrap());
    let request = plan_domain::DecompositionRequest {
        parent_task_ref: task.to_owned(),
        base_plan_revision: 0,
        acceptance_criteria_refs: vec![],
        scope_bound: scope.clone(),
        evidence_refs: vec![plan_domain::EvidenceRef::new(format!("fixture-plan:{task}")).unwrap()],
        depth_remaining: plan_domain::MAX_DECOMPOSITION_DEPTH,
        budget_remaining: plan_domain::PlanningBudget {
            calls_remaining: plan_domain::MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: plan_domain::MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    };
    let policy = app_services::PlanningPolicyContext {
        scope_bound: scope,
        baseline_risk: plan_domain::RiskClass::Low,
        affected_parent_criteria: vec![],
        parent_task_ref: task.to_owned(),
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

fn run_req(task: &str, execution: &str, attempt: &str, repo: &str) -> RunExecutionRequest {
    RunExecutionRequest {
        task_id: task.to_owned(),
        execution_id: execution.to_owned(),
        attempt_id: attempt.to_owned(),
        repository_ref: format!("repo-local:{task}"),
        repository_path: repo.to_owned(),
        base_revision: "HEAD".to_owned(),
    }
}

fn resume_req(execution: &str, repo: &str, task: &str) -> ResumeExecutionRequest {
    ResumeExecutionRequest {
        execution_id: execution.to_owned(),
        repository_path: repo.to_owned(),
        repository_ref: format!("repo-local:{task}"),
        base_revision: "HEAD".to_owned(),
    }
}

fn dispatch_run(
    composition: &ExecutionComposition,
    req: RunExecutionRequest,
) -> (
    ExecutionCoreResponse,
    Option<agentd::execution::BackgroundJob>,
) {
    dispatch_execution(
        composition,
        ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Run(req)),
        },
    )
}

fn dispatch_resume(
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

fn is_resume(response: &ExecutionCoreResponse) -> bool {
    matches!(
        response.payload,
        Some(execution_core_response::Payload::Resume(_))
    )
}

fn error_code(response: &ExecutionCoreResponse) -> i32 {
    match &response.payload {
        Some(execution_core_response::Payload::Error(error)) => error.code,
        other => panic!("expected typed error, got {other:?}"),
    }
}

fn get_status(
    composition: &ExecutionComposition,
    execution: &str,
) -> ipc_contracts::proto::GetExecutionResponse {
    let (response, job) = dispatch_execution(
        composition,
        ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Get(GetExecutionRequest {
                execution_id: execution.to_owned(),
            })),
        },
    );
    assert!(job.is_none());
    match response.payload {
        Some(execution_core_response::Payload::Get(get)) => get,
        other => panic!("expected status, got {other:?}"),
    }
}

fn cancel(
    composition: &ExecutionComposition,
    execution: &str,
) -> ipc_contracts::proto::CancelExecutionResponse {
    let (response, job) = dispatch_execution(
        composition,
        ExecutionCoreRequest {
            payload: Some(execution_core_request::Payload::Cancel(
                CancelExecutionRequest {
                    execution_id: execution.to_owned(),
                },
            )),
        },
    );
    assert!(job.is_none());
    match response.payload {
        Some(execution_core_response::Payload::Cancel(cancel)) => cancel,
        other => panic!("expected cancel, got {other:?}"),
    }
}

/// Commits a genuine pre-Attempt strand (production run entry, background
/// never runs — the crash shape) and discards the temp registry, exactly
/// like a daemon SIGKILL after COMMIT followed by a restart.
fn strand_execution(sandbox: &Sandbox, task: &str, execution: &str, attempt: &str) {
    let temp = ExecutionComposition::new(sandbox.db.clone(), Some(sandbox.fast.clone()));
    let (response, job) = dispatch_run(
        &temp,
        run_req(task, execution, attempt, &sandbox.repo_str()),
    );
    assert!(
        matches!(
            response.payload,
            Some(execution_core_response::Payload::Run(_))
        ),
        "strand admission must accept, got {response:?}"
    );
    // Crash: the background half never runs.
    drop(job);
    drop(temp);
}

fn strand_branch_only(sandbox: &Sandbox, task: &str, execution: &str, attempt: &str) {
    strand_execution(sandbox, task, execution, attempt);
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
            execution_id: agent_runtime::ExecutionId::new(execution).unwrap(),
            repository_ref: workspace::RepositoryRef::new(format!("repo-local:{task}")).unwrap(),
            repository_path: sandbox.repo_str(),
            base_revision: "HEAD".to_owned(),
        })
        .expect("allocate branch-only lease");
    git(
        &sandbox.repo,
        &[
            "branch",
            lease.branch().short_name(),
            lease.base_revision().as_str(),
        ],
    );
    assert!(!Path::new(lease.worktree_ref()).exists());
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

fn journal_types(sandbox: &Sandbox, task: &str) -> Vec<String> {
    let store = storage_sqlite::SqliteStore::open(&sandbox.db).expect("open store");
    store
        .list_events_for_task(&task_domain::TaskId::new(task).unwrap())
        .expect("read journal")
        .iter()
        .map(|event| event.fact.event_type.as_str().to_owned())
        .collect()
}

fn wait_for_attempt(sandbox: &Sandbox, execution: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if !attempt_outcomes(sandbox, execution).is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("attempt for {execution} never appeared");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_terminal(sandbox: &Sandbox, execution: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let outcomes = attempt_outcomes(sandbox, execution);
        if !outcomes.is_empty() && outcomes.iter().all(|o| o != "OPEN") {
            return;
        }
        if Instant::now() >= deadline {
            panic!("execution {execution} never reached terminal: {outcomes:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Runs N resume dispatches concurrently against ONE shared composition,
/// starting from a barrier so all racers overlap inside the daemon.
fn race_resumes(
    composition: std::sync::Arc<ExecutionComposition>,
    execution: &str,
    repo: &str,
    task: &str,
    n: usize,
) -> Vec<(
    ExecutionCoreResponse,
    Option<agentd::execution::BackgroundJob>,
)> {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(n));
    let mut handles = Vec::with_capacity(n);
    for _ in 0..n {
        let composition = composition.clone();
        let barrier = barrier.clone();
        let req = resume_req(execution, repo, task);
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            dispatch_resume(&composition, req)
        }));
    }
    handles
        .into_iter()
        .map(|h| h.join().expect("racer joins"))
        .collect()
}

#[test]
fn two_shared_resumes_elect_a_single_live_owner() {
    let sandbox = Sandbox::new("two-resumes");
    sandbox.seed_ready("P-1", "T-1");
    strand_execution(&sandbox, "T-1", "E-1", "A-1");

    // ONE shared production-style composition, slow provider so the winner
    // stays live while status/cancel/ownership are inspected.
    let shared = std::sync::Arc::new(ExecutionComposition::new(
        sandbox.db.clone(),
        Some(sandbox.slow.clone()),
    ));
    let repo = sandbox.repo_str();
    let results = race_resumes(shared.clone(), "E-1", &repo, "T-1", 2);

    let accepted: Vec<_> = results.iter().filter(|(r, _)| is_resume(r)).collect();
    assert_eq!(
        accepted.len(),
        1,
        "exactly one resume may own the live continuation, got {} accepted of 2: {:?}",
        accepted.len(),
        results
            .iter()
            .map(|(r, _)| match &r.payload {
                Some(execution_core_response::Payload::Resume(v)) => {
                    format!("RESUMED attempt {}", v.attempt_id)
                }
                Some(execution_core_response::Payload::Error(e)) => {
                    format!("ERROR {} {}", e.code, e.message)
                }
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>(),
    );
    for (response, job) in &results {
        if !is_resume(response) {
            assert_eq!(
                error_code(response),
                BusinessErrorCode::Conflict as i32,
                "loser must receive an honest typed refusal, got {response:?}"
            );
            assert!(
                job.is_none(),
                "refused loser must not yield a background job"
            );
        }
    }

    // The winner is live before its background even spawns.
    let status = get_status(&shared, "E-1");
    assert!(status.execution_live, "winner must be live while running");

    // Spawn every yielded job (the winner; losers yield none): a stale
    // loser cleanup must never retire the winner.
    let mut bg = Vec::new();
    for (_, job) in results {
        if let Some(job) = job {
            bg.push(std::thread::spawn(move || run_background(job)));
        }
    }
    assert_eq!(bg.len(), 1, "exactly one background continuation may exist");
    wait_for_attempt(&sandbox, "E-1");
    let status = get_status(&shared, "E-1");
    assert!(
        status.execution_live,
        "winner must stay live while the provider runs"
    );

    // Cancel must reach the actual running provider — not an abandoned
    // flag, and never NO_LIVE_RUN while the owner is active.
    let cancelled = cancel(&shared, "E-1");
    assert_eq!(cancelled.run_state, "LIVE_SIGNALLED", "got {cancelled:?}");
    assert!(cancelled.cancel_requested);
    for handle in bg {
        handle.join().expect("background joins");
    }
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["CANCELLED"]);

    let status = get_status(&shared, "E-1");
    assert!(!status.execution_live, "no live owner after completion");

    // Durable exactly-once survives the race.
    assert_eq!(attempt_outcomes(&sandbox, "E-1").len(), 1);
    assert_eq!(
        journal_types(&sandbox, "T-1")
            .iter()
            .filter(|k| k.as_str() == "attempt.created")
            .count(),
        1
    );
    assert_eq!(
        journal_types(&sandbox, "T-1")
            .iter()
            .filter(|k| k.as_str() == "workspace.created")
            .count(),
        1
    );
}

#[test]
fn eight_shared_resumes_elect_a_single_owner() {
    let sandbox = Sandbox::new("eight-resumes");
    sandbox.seed_ready("P-1", "T-1");
    strand_execution(&sandbox, "T-1", "E-1", "A-1");

    let shared = std::sync::Arc::new(ExecutionComposition::new(
        sandbox.db.clone(),
        Some(sandbox.fast.clone()),
    ));
    let repo = sandbox.repo_str();
    let results = race_resumes(shared.clone(), "E-1", &repo, "T-1", 8);

    let accepted = results.iter().filter(|(r, _)| is_resume(r)).count();
    assert_eq!(
        accepted, 1,
        "exactly one of 8 resumes may own the live continuation, got {accepted}"
    );
    for (response, job) in &results {
        if !is_resume(response) {
            assert_eq!(
                error_code(response),
                BusinessErrorCode::Conflict as i32,
                "loser must be refused honestly, got {response:?}"
            );
            assert!(job.is_none());
        }
    }

    let mut bg = Vec::new();
    for (_, job) in results {
        if let Some(job) = job {
            bg.push(std::thread::spawn(move || run_background(job)));
        }
    }
    assert_eq!(bg.len(), 1);
    for handle in bg {
        handle.join().expect("background joins");
    }
    wait_terminal(&sandbox, "E-1");
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["SUCCEEDED"]);
    assert_eq!(
        journal_types(&sandbox, "T-1")
            .iter()
            .filter(|k| k.as_str() == "attempt.created")
            .count(),
        1,
        "exactly one attempt.created, got {:?}",
        journal_types(&sandbox, "T-1")
    );
    let status = get_status(&shared, "E-1");
    assert!(!status.execution_live);
}

#[test]
fn run_then_resume_same_execution_keeps_the_run_owner() {
    let sandbox = Sandbox::new("run-resume");
    sandbox.seed_ready("P-1", "T-1");
    // ONE shared composition: the run's live owner and the resume racer
    // meet in the same registry, the TASK-0055 P2-C shape.
    let shared = ExecutionComposition::new(sandbox.db.clone(), Some(sandbox.slow.clone()));

    let (run_response, run_job) =
        dispatch_run(&shared, run_req("T-1", "E-1", "A-1", &sandbox.repo_str()));
    assert!(
        matches!(
            run_response.payload,
            Some(execution_core_response::Payload::Run(_))
        ),
        "run must accept, got {run_response:?}"
    );
    let run_job = run_job.expect("accepted run yields a job");

    // The run owns liveness before its background spawns.
    assert!(get_status(&shared, "E-1").execution_live);

    let (resume_response, resume_job) =
        dispatch_resume(&shared, resume_req("E-1", &sandbox.repo_str(), "T-1"));
    assert!(
        !is_resume(&resume_response),
        "resume must not overwrite the live run owner, got {resume_response:?}"
    );
    assert_eq!(
        error_code(&resume_response),
        BusinessErrorCode::Conflict as i32,
        "run+resume loser is refused honestly, got {resume_response:?}"
    );
    assert!(resume_job.is_none(), "loser spawns no continuation");

    // The run winner is still the live, cancellable owner.
    assert!(get_status(&shared, "E-1").execution_live);
    let handle = std::thread::spawn(move || run_background(run_job));
    wait_for_attempt(&sandbox, "E-1");
    assert!(get_status(&shared, "E-1").execution_live);
    let cancelled = cancel(&shared, "E-1");
    assert_eq!(cancelled.run_state, "LIVE_SIGNALLED", "got {cancelled:?}");
    handle.join().expect("background joins");
    assert_eq!(attempt_outcomes(&sandbox, "E-1"), vec!["CANCELLED"]);
    assert!(!get_status(&shared, "E-1").execution_live);
}

#[test]
fn shared_resume_race_repeated_fifty_rounds() {
    let sandbox = Sandbox::new("fifty-rounds");
    let shared = std::sync::Arc::new(ExecutionComposition::new(
        sandbox.db.clone(),
        Some(sandbox.fast.clone()),
    ));
    let repo = sandbox.repo_str();
    for round in 0..50 {
        let project = format!("P-{round}");
        let task = format!("T-{round}");
        let execution = format!("E-{round}");
        let attempt = format!("A-{round}");
        sandbox.seed_ready(&project, &task);
        strand_execution(&sandbox, &task, &execution, &attempt);
        let results = race_resumes(shared.clone(), &execution, &repo, &task, 2);
        let accepted = results.iter().filter(|(r, _)| is_resume(r)).count();
        assert_eq!(
            accepted, 1,
            "round {round}: exactly one owner, got {accepted}"
        );
        for (response, job) in &results {
            if !is_resume(response) {
                assert_eq!(error_code(response), BusinessErrorCode::Conflict as i32);
                assert!(job.is_none());
            }
        }
        let mut bg = Vec::new();
        for (_, job) in results {
            if let Some(job) = job {
                bg.push(std::thread::spawn(move || run_background(job)));
            }
        }
        assert_eq!(bg.len(), 1, "round {round}: one continuation");
        for handle in bg {
            handle.join().expect("background joins");
        }
        wait_terminal(&sandbox, &execution);
        assert_eq!(attempt_outcomes(&sandbox, &execution), vec!["SUCCEEDED"]);
    }
}

#[test]
fn branch_only_shared_resume_race_repeated_fifty_rounds() {
    let sandbox = Sandbox::new("branch-only-fifty-rounds");
    let shared = std::sync::Arc::new(ExecutionComposition::new(
        sandbox.db.clone(),
        Some(sandbox.fast.clone()),
    ));
    let repo = sandbox.repo_str();
    for round in 0..50 {
        let project = format!("BP-{round}");
        let task = format!("BT-{round}");
        let execution = format!("BE-{round}");
        let attempt = format!("BA-{round}");
        sandbox.seed_ready(&project, &task);
        strand_branch_only(&sandbox, &task, &execution, &attempt);

        let results = race_resumes(shared.clone(), &execution, &repo, &task, 2);
        let accepted = results
            .iter()
            .filter(|(response, _)| is_resume(response))
            .count();
        assert_eq!(accepted, 1, "round {round}: exactly one branch-only owner");
        for (response, job) in &results {
            if !is_resume(response) {
                assert_eq!(error_code(response), BusinessErrorCode::Conflict as i32);
                assert!(job.is_none());
            }
        }
        let mut handles = Vec::new();
        for (_, job) in results {
            if let Some(job) = job {
                handles.push(std::thread::spawn(move || run_background(job)));
            }
        }
        assert_eq!(handles.len(), 1, "round {round}: one background recovery");
        for handle in handles {
            handle.join().expect("background joins");
        }
        wait_terminal(&sandbox, &execution);
        assert_eq!(attempt_outcomes(&sandbox, &execution), vec!["SUCCEEDED"]);
        let journal = journal_types(&sandbox, &task);
        assert_eq!(
            journal
                .iter()
                .filter(|kind| kind.as_str() == "attempt.created")
                .count(),
            1,
            "round {round}: exactly one attempt.created"
        );
        assert_eq!(
            journal
                .iter()
                .filter(|kind| kind.as_str() == "workspace.created")
                .count(),
            1,
            "round {round}: exactly one workspace.created"
        );
    }
}
