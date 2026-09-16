//! G5 execution surface through the shipped binaries (TASK-0047).
//!
//! Real `agentd` + `agentctl` processes against test-owned runtime/data
//! directories, with a fixture provider named `claude` prepended to
//! `PATH` (the exact production resolution — no daemon flags). Covers
//! the `main.rs` paths the in-process suite cannot: `execution run`
//! wait-mode polling and exit codes, `--detach`, `status` printing, and
//! `cancel` printing.
//!
//! Unix-only. No quota, no network, no real Claude.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn workspace_bin(name: &str) -> PathBuf {
    let mut dir = std::env::current_exe().expect("test executable path");
    dir.pop();
    dir.pop();
    dir.join(name)
}

struct Sandbox {
    root: PathBuf,
    repo: PathBuf,
    path_env: String,
}

impl Sandbox {
    fn new(suite: &str, provider_body: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root =
            PathBuf::from("/tmp").join(format!("g5-bin-{}-{suite}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("sandbox root");
        // Production endpoint/data resolvers require the immediate
        // parents to exist (they create only the final component with
        // the secure mode); the daemon owns the rest.
        std::fs::create_dir_all(root.join("run")).expect("runtime parent");
        std::fs::create_dir_all(root.join("data")).expect("data parent");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        // Fixture provider named exactly `claude`: production PATH
        // resolution finds it with no daemon-side override.
        let bindir = root.join("bin");
        std::fs::create_dir_all(&bindir).expect("bin dir");
        let exe = bindir.join("claude");
        let mut file = std::fs::File::create(&exe).expect("fixture file");
        file.write_all(b"#!/bin/sh\n").expect("shebang");
        file.write_all(provider_body.as_bytes())
            .expect("fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("fixture chmod");
        let path_env = format!(
            "{}:{}",
            bindir.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Self {
            root,
            repo,
            path_env,
        }
    }

    fn agentctl(&self, args: &[&str]) -> Output {
        Command::new(workspace_bin("agentctl"))
            .args(args)
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", self.root.join("run"))
            .env("AI_ENGINEERING_OS_DATA_DIR", self.root.join("data"))
            .env("PATH", &self.path_env)
            .output()
            .expect("run agentctl")
    }

    fn spawn_agentd(&self) -> Child {
        Command::new(workspace_bin("agentd"))
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", self.root.join("run"))
            .env("AI_ENGINEERING_OS_DATA_DIR", self.root.join("data"))
            .env("PATH", &self.path_env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn agentd")
    }

    fn replace_provider_body(&self, provider_body: &str) {
        let exe = self.root.join("bin/claude");
        let mut file = std::fs::File::create(&exe).expect("replace fixture provider");
        file.write_all(b"#!/bin/sh\n").expect("replacement shebang");
        file.write_all(provider_body.as_bytes())
            .expect("replacement fixture body");
        drop(file);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))
            .expect("replacement fixture chmod");
    }

    fn endpoint(&self) -> ipc_transport::platform::unix::UnixEndpoint {
        let runtime_dir = self.root.join("run/ai-engineering-os");
        ipc_transport::platform::unix::UnixEndpoint {
            socket_path: runtime_dir.join("agentd.sock"),
            runtime_dir,
        }
    }

    /// Probes only this sandbox's already-bound endpoint. Unlike the
    /// `agentctl ping` binary command, this path cannot launch a sibling
    /// daemon when a just-spawned daemon has not bound its socket yet.
    fn connect_ping(&self) -> bool {
        let endpoint = self.endpoint();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("readiness runtime")
            .block_on(async move {
                match agentctl::connect_and_handshake(&endpoint).await {
                    Ok(mut session) => session.ping().await.is_ok(),
                    Err(_) => false,
                }
            })
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if self.connect_ping() {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "daemon never became ready at {}",
                    self.endpoint().socket_path.display()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn planning_decision_id(output: &Output) -> String {
    stdout(output)
        .split_whitespace()
        .find_map(|field| field.strip_prefix("decision="))
        .filter(|value| !value.is_empty())
        .expect("planning output decision id")
        .to_owned()
}

const SUCCESS_BODY: &str = r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"binary fixture finished","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee"}'; exit 0
"#;

// The same fixture is a valid planner result and a successful execution
// result. The planner adapter reads the JSON string in `result`; the
// execution adapter only needs the outer successful provider event.
const PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":0,\"reason\":\"fixture decomposition\",\"node_additions\":[{\"id\":\"T-PLAN\",\"scope\":[\"repo:src\"],\"criteria\":[{\"id\":\"criterion-root\",\"method\":\"automated-test\"}],\"risk_class\":\"low\",\"required_gate\":null}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"fixture work is executable\",\"risk_delta\":[],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:src\"],\"coverage_map\":{\"criterion-root\":[\"criterion-root\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const RISK_PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":0,\"reason\":\"fixture risk escalation\",\"node_additions\":[{\"id\":\"T-RISK\",\"scope\":[\"repo:src\"],\"criteria\":[{\"id\":\"criterion-root\",\"method\":\"automated-test\"}],\"risk_class\":\"high\",\"required_gate\":\"human-review\"}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"fixture work requires human approval\",\"risk_delta\":[{\"node\":\"T-RISK\",\"from\":\"low\",\"to\":\"high\"}],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:src\"],\"coverage_map\":{\"criterion-root\":[\"criterion-root\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const AUTHORITY_PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":0,\"reason\":\"canonical authority fixture\",\"node_additions\":[{\"id\":\"T-AUTH\",\"scope\":[\"repo:canonical\"],\"criteria\":[{\"id\":\"criterion-canonical\",\"method\":\"automated-test\"}],\"risk_class\":\"medium\",\"required_gate\":null}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"canonical authority is preserved\",\"risk_delta\":[],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:canonical\"],\"coverage_map\":{\"criterion-canonical\":[\"criterion-canonical\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const SLOW_BODY: &str = "while :; do :; done\n";

fn seed_ready(sandbox: &Sandbox, project: &str, task: &str) {
    let created = sandbox.agentctl(&["project", "create", project, &format!("project {project}")]);
    assert!(
        created.status.success(),
        "seed project: {}",
        stderr(&created)
    );
    let created = sandbox.agentctl(&[
        "task",
        "create",
        task,
        project,
        &format!("objective {task}"),
    ]);
    assert!(created.status.success(), "seed task: {}", stderr(&created));
    let ready = sandbox.agentctl(&["task", "ready", task]);
    assert!(ready.status.success(), "seed ready: {}", stderr(&ready));
    seed_accepted_plan(sandbox, task);
}

fn seed_accepted_plan(sandbox: &Sandbox, task: &str) {
    let db = sandbox
        .root
        .join("data")
        .join("ai-engineering-os")
        .join("state.sqlite3");
    let mut planning = app_services::PlanningService::open(&db, app_services::SystemClock)
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

fn seed_planning_task_metadata(
    sandbox: &Sandbox,
    task: &str,
    scope: &str,
    criterion: &str,
    risk: &str,
) {
    let db = sandbox.root.join("data/ai-engineering-os/state.sqlite3");
    let mut store = storage_sqlite::SqliteStore::open(&db).expect("open canonical task store");
    let task_id = task_domain::TaskId::new(task).expect("task id");
    let task = store
        .get_task(&task_id)
        .expect("canonical task query")
        .expect("canonical task");
    let mut snapshot = task.snapshot();
    snapshot.allowed_paths = vec![scope.to_owned()];
    snapshot.acceptance = vec![criterion.to_owned()];
    snapshot.risk = Some(risk.to_owned());
    let task = task_domain::Task::restore(snapshot).expect("canonical planning task metadata");
    store
        .save_task(&task)
        .expect("persist canonical task metadata");
}

fn seed_human_decision(sandbox: &Sandbox, task: &str) -> String {
    let db = sandbox
        .root
        .join("data")
        .join("ai-engineering-os")
        .join("state.sqlite3");
    let branch = plan_domain::PlanBranchId::new(format!("fixture-plan-{task}")).unwrap();
    let planning = app_services::PlanningService::open(&db, app_services::SystemClock)
        .expect("open planning service");
    let accepted = planning
        .current_accepted_plan(&branch)
        .expect("accepted plan query")
        .expect("accepted plan");
    let mut decisions =
        app_services::HumanDecisionService::open(&db).expect("open human decision service");
    decisions
        .create(app_services::HumanDecisionCreate {
            task_id: task_domain::TaskId::new(task).unwrap(),
            plan_branch_id: branch,
            reason: plan_domain::HumanDecisionReason::PlannerBlocked,
            risk: plan_domain::RiskClass::Low,
            affected_scope: plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
            options: vec![
                plan_domain::HumanContinuation::Resume,
                plan_domain::HumanContinuation::Replan,
                plan_domain::HumanContinuation::Abort,
            ],
            recommendation: None,
            evidence: vec![plan_domain::EvidenceRef::new(format!("hdq:{task}")).unwrap()],
            proposal_hash: None,
            plan_head: accepted.revision().revision_seq,
            planning_input_snapshot_id: accepted.planning_input_snapshot_id().clone(),
            planning_input_fingerprint: accepted.planning_input_fingerprint().to_owned(),
            policy_fingerprint: format!("policy:fixture:{task}"),
            expires_at_ms: None,
        })
        .expect("create human decision")
        .id
        .as_str()
        .to_owned()
}

/// RAII daemon handle for the TASK-0056 keepers: kills and reaps the child
/// on drop so any test failure can never leak a daemon (TEST-H).
struct DaemonGuard {
    child: Option<Child>,
}

#[test]
fn readiness_probe_never_starts_a_daemon() {
    let sandbox = Sandbox::new("readiness-no-spawn", SUCCESS_BODY);

    assert!(
        !sandbox.connect_ping(),
        "a missing sandbox endpoint must not appear ready"
    );
    assert!(
        !sandbox.endpoint().socket_path.exists(),
        "connect-only readiness must not spawn agentd"
    );
}

impl DaemonGuard {
    fn spawn(sandbox: &Sandbox) -> Self {
        Self {
            child: Some(sandbox.spawn_agentd()),
        }
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn restart(&mut self, sandbox: &Sandbox) {
        self.kill();
        self.child = Some(sandbox.spawn_agentd());
        sandbox.wait_ready();
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// TASK-0056 (B-CR) binary keepers: the same live-ownership races as the
/// in-process suite, driven through the shipped `agentd`/`agentctl`
/// processes with parallel client processes racing one daemon.
///
/// Strand setup: `run --detach` commits the entry synchronously and
/// returns; an immediate SIGKILL of the daemon lands inside the
/// pre-attempt window (lease/Git/attempt work still pending), and the
/// restarted daemon reports the execution `RUNNING`, `live=false`, zero
/// attempts — the genuine crash shape. Retried with fresh identities when
/// the kill lands past attempt creation.
mod live_ownership {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ROUND: AtomicU64 = AtomicU64::new(0);

    fn repo(sandbox: &Sandbox) -> String {
        sandbox.repo.to_string_lossy().into_owned()
    }

    fn seed_task(sandbox: &Sandbox, project: &str, task: &str) {
        seed_ready(sandbox, project, task);
    }

    fn status_out(sandbox: &Sandbox, execution: &str) -> String {
        let status = sandbox.agentctl(&["execution", "status", execution]);
        assert!(status.status.success(), "status: {}", stderr(&status));
        stdout(&status)
    }

    fn cancel_out(sandbox: &Sandbox, execution: &str) -> String {
        let cancel = sandbox.agentctl(&["execution", "cancel", execution]);
        assert!(cancel.status.success(), "cancel: {}", stderr(&cancel));
        stdout(&cancel)
    }

    fn wait_no_live(sandbox: &Sandbox, execution: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if status_out(sandbox, execution).contains("live=false") {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "execution {execution} stayed live: {}",
                    status_out(sandbox, execution)
                );
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn wait_attempt_line(sandbox: &Sandbox, execution: &str, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let out = status_out(sandbox, execution);
            if out.contains(needle) {
                return;
            }
            if Instant::now() >= deadline {
                panic!("status never showed {needle:?} for {execution}: {out:?}");
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Commits a genuine pre-attempt strand through the shipped binaries.
    /// Returns the (task, execution) identities of the strand.
    fn strand_via_kill(sandbox: &Sandbox, daemon: &mut DaemonGuard) -> (String, String) {
        for _ in 0..6 {
            let n = ROUND.fetch_add(1, Ordering::SeqCst);
            let project = format!("P-S{n}");
            let task = format!("T-S{n}");
            let execution = format!("E-S{n}");
            let attempt = format!("A-S{n}");
            seed_task(sandbox, &project, &task);
            let run = sandbox.agentctl(&[
                "execution",
                "run",
                &task,
                &repo(sandbox),
                "--execution",
                &execution,
                "--attempt",
                &attempt,
                "--detach",
            ]);
            assert!(run.status.success(), "detach run: {}", stderr(&run));
            // Crash inside the pre-attempt window: the entry is committed,
            // the background half dies with the daemon.
            daemon.kill();
            daemon.restart(sandbox);
            let out = status_out(sandbox, &execution);
            assert!(out.contains(&execution), "status names it: {out:?}");
            if !out.contains("attempt ") {
                assert!(out.contains("live=false"), "strand is honest: {out:?}");
                return (task, execution);
            }
            // Missed the window (attempt already created): cancel the slow
            // run for cleanliness and retry with fresh identities.
            let cancelled = cancel_out(sandbox, &execution);
            assert!(
                cancelled.contains("LIVE_SIGNALLED") || cancelled.contains("ALREADY_CONCLUDED"),
                "cleanup cancel honest: {cancelled:?}"
            );
            wait_no_live(sandbox, &execution);
        }
        panic!("could not strand a pre-attempt entry in 6 tries");
    }

    fn resume_cmd(execution: &str, repo: &str, task: &str) -> Vec<String> {
        vec![
            "execution".to_owned(),
            "resume".to_owned(),
            execution.to_owned(),
            repo.to_owned(),
            "--ref".to_owned(),
            format!("repo-local:{task}"),
            "--rev".to_owned(),
            "HEAD".to_owned(),
        ]
    }

    /// Runs `n` `agentctl execution resume` client processes in parallel
    /// against one daemon. Returns (accepted, refused) by stdout/stderr.
    fn race_resumes(
        sandbox: &Sandbox,
        execution: &str,
        task: &str,
        n: usize,
    ) -> (Vec<String>, Vec<String>) {
        let repo = repo(sandbox);
        let mut accepted = Vec::new();
        let mut refused = Vec::new();
        std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(n);
            for _ in 0..n {
                let args = resume_cmd(execution, &repo, task);
                handles.push(scope.spawn(move || {
                    sandbox.agentctl(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                }));
            }
            for handle in handles {
                let output = handle.join().expect("client joins");
                let out = stdout(&output);
                let err = stderr(&output);
                if output.status.success() && out.contains("resumed execution") {
                    accepted.push(out);
                } else {
                    refused.push(format!("exit={} out={out:?} err={err:?}", output.status));
                }
            }
        });
        (accepted, refused)
    }
    #[test]
    fn binary_concurrent_resumes_elect_a_single_owner() {
        let sandbox = Sandbox::new("liveown-race", SLOW_BODY);
        let mut daemon = DaemonGuard::spawn(&sandbox);
        sandbox.wait_ready();

        // 2-client race on the first strand, then an 8-client race on a
        // fresh strand: each keeper observes a full claim → live →
        // cancel → release cycle through the real daemon.
        for clients in [2usize, 8usize] {
            let (task, execution) = strand_via_kill(&sandbox, &mut daemon);
            let (accepted, refused) = race_resumes(&sandbox, &execution, &task, clients);
            assert_eq!(
                accepted.len(),
                1,
                "{clients}-client race: exactly one owner, got {} accepted: {accepted:?} refused: {refused:?}",
                accepted.len()
            );
            for refusal in &refused {
                assert!(
                    refusal.contains("CONFLICT"),
                    "loser refused honestly, got {refusal:?}"
                );
            }
            // Winner stays live and cancellable through the real daemon.
            let status = status_out(&sandbox, &execution);
            assert!(status.contains("live=true"), "winner live: {status:?}");
            let cancelled = cancel_out(&sandbox, &execution);
            assert!(
                cancelled.contains("LIVE_SIGNALLED"),
                "cancel reaches the actual winner: {cancelled:?}"
            );
            wait_attempt_line(&sandbox, &execution, "CANCELLED");
            wait_no_live(&sandbox, &execution);
        }
    }

    #[test]
    fn binary_run_then_resume_keeps_the_run_owner() {
        let sandbox = Sandbox::new("liveown-runresume", SLOW_BODY);
        let _daemon = DaemonGuard::spawn(&sandbox);
        sandbox.wait_ready();

        // Several run+resume overlaps: the resume must NEVER report
        // `resumed` while the run owns the execution. Rounds where the
        // resume lands post-attempt refuse INVALID_TRANSITION (honest, but
        // not the race); at least one round must land the pre-attempt
        // CONFLICT overlap to prove the keeper bites.
        let mut conflict_rounds = 0;
        for _ in 0..5 {
            let n = ROUND.fetch_add(1, Ordering::SeqCst);
            let project = format!("P-R{n}");
            let task = format!("T-R{n}");
            let execution = format!("E-R{n}");
            let attempt = format!("A-R{n}");
            seed_task(&sandbox, &project, &task);
            let run = sandbox.agentctl(&[
                "execution",
                "run",
                &task,
                &repo(&sandbox),
                "--execution",
                &execution,
                "--attempt",
                &attempt,
                "--detach",
            ]);
            assert!(run.status.success(), "detach run: {}", stderr(&run));
            let resume_args = resume_cmd(&execution, &repo(&sandbox), &task);
            let resume =
                sandbox.agentctl(&resume_args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            let out = stdout(&resume);
            let err = stderr(&resume);
            assert!(
                !out.contains("resumed execution"),
                "resume must never overwrite the live run owner (B-CR): {out:?}"
            );
            if !resume.status.success() && err.contains("CONFLICT") {
                conflict_rounds += 1;
                // Ownership intact: the run is still the live owner.
                let status = status_out(&sandbox, &execution);
                assert!(status.contains("live=true"), "run stays live: {status:?}");
            }
            // Cleanup: cancel the slow run and wait for release.
            let cancelled = cancel_out(&sandbox, &execution);
            assert!(
                cancelled.contains("LIVE_SIGNALLED") || cancelled.contains("ALREADY_CONCLUDED"),
                "cleanup cancel honest: {cancelled:?}"
            );
            wait_no_live(&sandbox, &execution);
        }
        assert!(
            conflict_rounds >= 1,
            "at least one round must hit the pre-attempt overlap"
        );
    }
}

#[test]
fn run_wait_mode_reports_terminal_success_with_exit_zero() {
    let sandbox = Sandbox::new("wait", SUCCESS_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-1");

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-1",
        "--attempt",
        "A-1",
    ]);
    assert!(
        run.status.success(),
        "wait-mode run exits 0 on SUCCEEDED: stdout={} stderr={}",
        stdout(&run),
        stderr(&run)
    );
    let out = stdout(&run);
    assert!(
        out.contains("SUCCEEDED"),
        "terminal outcome printed, got {out:?}"
    );
    assert!(out.contains("VERIFYING"), "task state printed, got {out:?}");

    let task = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(task.status.success());
    assert!(
        stdout(&task).contains("VERIFYING"),
        "provider success verifies, never completes: {}",
        stdout(&task)
    );

    let status = sandbox.agentctl(&["execution", "status", "E-1"]);
    assert!(status.status.success());
    let out = stdout(&status);
    assert!(
        out.contains("E-1") && out.contains("claude-code"),
        "got {out:?}"
    );
}

#[test]
fn detach_then_cancel_reaches_the_live_run() {
    let sandbox = Sandbox::new("cancel", SLOW_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-1");

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-2",
        "--attempt",
        "A-2",
        "--detach",
    ]);
    assert!(run.status.success(), "detach starts: {}", stderr(&run));
    assert!(stdout(&run).contains("E-2"));

    // The cancel flag registers synchronously with the accepted run, so
    // an immediate cancel deterministically signals the live background.
    let cancel = sandbox.agentctl(&["execution", "cancel", "E-2"]);
    assert!(
        cancel.status.success(),
        "cancel prints: {}",
        stderr(&cancel)
    );
    assert!(
        stdout(&cancel).contains("LIVE_SIGNALLED"),
        "got {:?}",
        stdout(&cancel)
    );

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = sandbox.agentctl(&["execution", "status", "E-2"]);
        assert!(status.status.success());
        let out = stdout(&status);
        if out.contains("CANCELLED") {
            break;
        }
        if Instant::now() >= deadline {
            panic!("slow run never cancelled: {out:?}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let task = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(
        stdout(&task).contains("RUNNING"),
        "cancel is not completion: {}",
        stdout(&task)
    );

    let again = sandbox.agentctl(&["execution", "cancel", "E-2"]);
    assert!(again.status.success());
    assert!(
        stdout(&again).contains("ALREADY_CONCLUDED"),
        "got {:?}",
        stdout(&again)
    );
}

#[test]
fn human_decision_cli_lists_gets_and_decides_over_protocol_v4() {
    let sandbox = Sandbox::new("human-decision-cli", SUCCESS_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    seed_ready(&sandbox, "P-HD", "T-HD");
    let decision_id = seed_human_decision(&sandbox, "T-HD");

    let list = sandbox.agentctl(&["human-decision", "list"]);
    assert!(list.status.success(), "list: {}", stderr(&list));
    let listed = stdout(&list);
    assert!(listed.contains(&decision_id), "listed {listed:?}");
    assert!(listed.contains("PENDING"), "listed {listed:?}");

    let get = sandbox.agentctl(&["human-decision", "get", &decision_id]);
    assert!(get.status.success(), "get: {}", stderr(&get));
    let got = stdout(&get);
    assert!(got.contains(&decision_id), "get {got:?}");
    assert!(got.contains("RESUME,REPLAN,ABORT"), "get {got:?}");

    let decide = sandbox.agentctl(&["human-decision", "decide", &decision_id, "abort"]);
    assert!(decide.status.success(), "decide: {}", stderr(&decide));
    let decided = stdout(&decide);
    assert!(decided.contains("RESOLVED"), "decided {decided:?}");
    assert!(decided.contains("selected ABORT"), "decided {decided:?}");

    let list_after = sandbox.agentctl(&["human-decision", "list"]);
    assert!(list_after.status.success());
    assert!(
        !stdout(&list_after).contains(&decision_id),
        "resolved request must leave pending queue"
    );

    let missing = sandbox.agentctl(&["human-decision", "get", "hdr-does-not-exist"]);
    assert!(!missing.status.success());
    assert!(
        stderr(&missing).contains("NOT_FOUND"),
        "missing: {}",
        stderr(&missing)
    );

    let invalid = sandbox.agentctl(&["human-decision", "decide", &decision_id, "invalid-option"]);
    assert!(!invalid.status.success());
    assert!(stderr(&invalid).contains("usage: agentctl human-decision"));
}

#[test]
fn v5_planning_cli_publishes_an_accepted_plan_then_admits_execution() {
    let sandbox = Sandbox::new("v5-planning-cli", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();

    let project = sandbox.agentctl(&["project", "create", "P-PLAN", "planning fixture"]);
    assert!(project.status.success(), "project: {}", stderr(&project));
    let task = sandbox.agentctl(&["task", "create", "T-PLAN", "P-PLAN", "plan fixture"]);
    assert!(task.status.success(), "task: {}", stderr(&task));
    let ready = sandbox.agentctl(&["task", "ready", "T-PLAN"]);
    assert!(ready.status.success(), "ready: {}", stderr(&ready));
    seed_planning_task_metadata(&sandbox, "T-PLAN", "repo:src", "criterion-root", "low");

    let planning = sandbox.agentctl(&[
        "planning",
        "trigger",
        "T-PLAN",
        "B-PLAN",
        "fixture:planning",
    ]);
    assert!(planning.status.success(), "planning: {}", stderr(&planning));
    assert!(
        stdout(&planning).contains("planning ACCEPTED branch=B-PLAN revision=1"),
        "planning output: {}",
        stdout(&planning)
    );

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-PLAN",
        &sandbox.repo.display().to_string(),
        "--execution",
        "E-PLAN",
        "--attempt",
        "A-PLAN",
    ]);
    assert!(run.status.success(), "execution: {}", stderr(&run));
    assert!(
        stdout(&run).contains("execution E-PLAN"),
        "run: {}",
        stdout(&run)
    );

    // Assertion-only durable read: no planning state was seeded through
    // SQLite; every write above travelled through agentctl/agentd.
    let db = sandbox.root.join("data/ai-engineering-os/state.sqlite3");
    let planning = app_services::PlanningService::open(&db, app_services::SystemClock)
        .expect("open assertion planning service");
    assert!(planning
        .current_accepted_plan(&plan_domain::PlanBranchId::new("B-PLAN").unwrap())
        .expect("accepted plan query")
        .is_some());
}

#[test]
fn v5_planning_cli_applies_risk_resume_without_replaying_the_planner() {
    let sandbox = Sandbox::new("v5-risk-resume-cli", RISK_PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();

    let project = sandbox.agentctl(&["project", "create", "P-RISK", "risk fixture"]);
    assert!(project.status.success(), "project: {}", stderr(&project));
    let task = sandbox.agentctl(&["task", "create", "T-RISK", "P-RISK", "risk fixture"]);
    assert!(task.status.success(), "task: {}", stderr(&task));
    let ready = sandbox.agentctl(&["task", "ready", "T-RISK"]);
    assert!(ready.status.success(), "ready: {}", stderr(&ready));
    seed_planning_task_metadata(&sandbox, "T-RISK", "repo:src", "criterion-root", "low");

    let planning = sandbox.agentctl(&["planning", "trigger", "T-RISK", "B-RISK", "fixture:risk"]);
    assert!(planning.status.success(), "planning: {}", stderr(&planning));
    assert!(
        stdout(&planning).contains("planning WAITING_FOR_HUMAN branch=B-RISK"),
        "planning output: {}",
        stdout(&planning)
    );
    let decision_id = planning_decision_id(&planning);

    let decide = sandbox.agentctl(&["human-decision", "decide", &decision_id, "resume"]);
    assert!(decide.status.success(), "decide: {}", stderr(&decide));
    assert!(
        stdout(&decide).contains("selected RESUME"),
        "decide output: {}",
        stdout(&decide)
    );

    // Any second PlannerPort call would execute this failing provider. The
    // exact proposal approved by the human must already be durable.
    sandbox.replace_provider_body("echo 'planner replayed during RESUME' >&2\nexit 99\n");
    let applied = sandbox.agentctl(&[
        "planning",
        "apply",
        &decision_id,
        "T-RISK",
        "B-RISK",
        "repo:forged-resume",
        "criterion-forged-resume",
        "policy:forged-resume",
        "fixture:forged-resume",
    ]);
    assert!(applied.status.success(), "apply: {}", stderr(&applied));
    assert!(
        stdout(&applied).contains("planning ACCEPTED branch=B-RISK revision=1"),
        "apply output: {}",
        stdout(&applied)
    );

    let replay = sandbox.agentctl(&["planning", "apply", &decision_id]);
    assert!(
        !replay.status.success(),
        "consumed decision must not replay"
    );
}

#[test]
fn v5_planning_ignores_client_authored_authority_and_persists_canonical_task_policy() {
    let sandbox = Sandbox::new("v5-planning-authority", AUTHORITY_PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();

    let project = sandbox.agentctl(&["project", "create", "P-AUTH", "authority fixture"]);
    assert!(project.status.success(), "project: {}", stderr(&project));
    let task = sandbox.agentctl(&["task", "create", "T-AUTH", "P-AUTH", "authority fixture"]);
    assert!(task.status.success(), "task: {}", stderr(&task));
    let ready = sandbox.agentctl(&["task", "ready", "T-AUTH"]);
    assert!(ready.status.success(), "ready: {}", stderr(&ready));
    seed_planning_task_metadata(
        &sandbox,
        "T-AUTH",
        "repo:canonical",
        "criterion-canonical",
        "medium",
    );

    // Original protocol-v5 attack shape: all authority-bearing values are
    // deliberately forged. The server must decode them for compatibility but
    // never use them to construct PlanningPolicyContext.
    let planning = sandbox.agentctl(&[
        "planning",
        "trigger",
        "T-AUTH",
        "B-AUTH",
        "repo:client-wide",
        "criterion-client",
        "policy:client-forged",
        "fixture:authority-probe",
    ]);
    assert!(planning.status.success(), "planning: {}", stderr(&planning));
    assert!(
        stdout(&planning).contains("planning ACCEPTED branch=B-AUTH revision=1"),
        "planning output: {}",
        stdout(&planning)
    );

    let db = sandbox.root.join("data/ai-engineering-os/state.sqlite3");
    let planning = app_services::PlanningService::open(&db, app_services::SystemClock)
        .expect("open assertion planning service");
    let branch = plan_domain::PlanBranchId::new("B-AUTH").unwrap();
    let accepted = planning
        .current_accepted_plan(&branch)
        .expect("accepted plan query")
        .expect("accepted plan");
    let persisted_policy_fingerprint = accepted.revision().validated_by.policy_fingerprint.clone();
    assert_ne!(persisted_policy_fingerprint, "policy:client-forged");
    let canonical = planning
        .canonical_policy_context(
            &task_domain::TaskId::new("T-AUTH").unwrap(),
            persisted_policy_fingerprint.clone(),
        )
        .expect("canonical planning policy");
    assert_eq!(canonical.scope_bound.roots(), vec!["repo:canonical"]);
    assert_eq!(canonical.baseline_risk, plan_domain::RiskClass::Medium);
    assert_eq!(canonical.affected_parent_criteria.len(), 1);
    assert_eq!(
        canonical.affected_parent_criteria[0].id.as_str(),
        "criterion-canonical"
    );
    assert!(planning
        .store()
        .planning_input_policy_matches(
            accepted.planning_input_snapshot_id(),
            "T-AUTH",
            &canonical.scope_bound,
            canonical.baseline_risk,
            &canonical.affected_parent_criteria,
            &canonical.lineage_fingerprints,
            None,
            &persisted_policy_fingerprint,
        )
        .expect("durable canonical planning input"));
    assert_eq!(
        accepted.revision().validated_by.policy_fingerprint,
        persisted_policy_fingerprint
    );
}
