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

    /// Like [`Self::spawn_agentd`], with the daemon's stderr kept in `log`.
    fn spawn_agentd_logging(&self, log: &Path) -> Child {
        Command::new(workspace_bin("agentd"))
            .env("AI_ENGINEERING_OS_RUNTIME_DIR", self.root.join("run"))
            .env("AI_ENGINEERING_OS_DATA_DIR", self.root.join("data"))
            .env("PATH", &self.path_env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(log).expect("daemon log"))
            .spawn()
            .expect("spawn agentd")
    }

    /// One protocol-v7 vault request against this sandbox's running daemon.
    fn vault(
        &self,
        request: ipc_contracts::proto::vault_core_request::Payload,
    ) -> ipc_contracts::proto::vault_core_response::Payload {
        let endpoint = self.endpoint();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("vault runtime")
            .block_on(agentctl::vault_request(
                &endpoint,
                || Err(std::io::Error::other("test never starts a daemon here")),
                ipc_contracts::proto::VaultCoreRequest {
                    payload: Some(request),
                },
            ))
            .expect("vault request")
            .payload
            .expect("vault payload")
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
const PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":0,\"reason\":\"fixture decomposition\",\"node_additions\":[{\"id\":\"T-PLAN-CHILD\",\"scope\":[\"repo:src\"],\"criteria\":[{\"id\":\"criterion-root\",\"method\":\"automated-test\"}],\"risk_class\":\"low\",\"required_gate\":null}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"fixture work is executable\",\"risk_delta\":[],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:src\"],\"coverage_map\":{\"criterion-root\":[\"criterion-root\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const RISK_PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":0,\"reason\":\"fixture risk escalation\",\"node_additions\":[{\"id\":\"T-RISK-CHILD\",\"scope\":[\"repo:src\"],\"criteria\":[{\"id\":\"criterion-root\",\"method\":\"automated-test\"}],\"risk_class\":\"high\",\"required_gate\":\"human-review\"}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"fixture work requires human approval\",\"risk_delta\":[{\"node\":\"T-RISK-CHILD\",\"from\":\"low\",\"to\":\"high\"}],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:src\"],\"coverage_map\":{\"criterion-root\":[\"criterion-root\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const AUTHORITY_PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":0,\"reason\":\"canonical authority fixture\",\"node_additions\":[{\"id\":\"T-AUTH-CHILD\",\"scope\":[\"repo:canonical\"],\"criteria\":[{\"id\":\"criterion-canonical\",\"method\":\"automated-test\"}],\"risk_class\":\"medium\",\"required_gate\":null}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"canonical authority is preserved\",\"risk_delta\":[],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:canonical\"],\"coverage_map\":{\"criterion-canonical\":[\"criterion-canonical\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const RETURN_PLANNING_BODY: &str = r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"{\"base_plan_revision\":1,\"reason\":\"fixture takeover return\",\"node_additions\":[{\"id\":\"T-RETURN-CHILD\",\"scope\":[\"repo:src\"],\"criteria\":[{\"id\":\"criterion-root\",\"method\":\"automated-test\"}],\"risk_class\":\"low\",\"required_gate\":null}],\"node_updates\":[],\"node_removals\":[],\"edge_additions\":[],\"edge_removals\":[],\"expected_outcome\":\"takeover return is executable\",\"risk_delta\":[],\"budget_delta\":{\"planning_cost_movement\":0,\"expected_execution_cost_movement\":0},\"scope_delta\":[\"repo:src\"],\"coverage_map\":{\"criterion-root\":[\"criterion-root\"]}}","session_id":"eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee","usage":{"input_tokens":1,"output_tokens":1}}'; exit 0
"#;

const SLOW_BODY: &str = "while :; do :; done\n";

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_millis() as u64
}

fn takeover_record_id(output: &Output) -> String {
    stdout(output)
        .lines()
        .find_map(|line| line.strip_prefix("takeover "))
        .and_then(|rest| rest.split_whitespace().next().map(str::to_owned))
        .expect("takeover record id")
}

/// Authors a planning task, publishes its first plan, then drives it to
/// RUNNING, so a TAKEOVER decision can be seeded against current state.
fn setup_running_planned_task(sandbox: &Sandbox, project: &str, task: &str, branch: &str) {
    author_ready_planning_task(sandbox, project, task, "repo:src", "criterion-root", "low");
    let planning =
        sandbox.agentctl(&["planning", "trigger", task, branch, "fixture:takeover-seed"]);
    assert!(planning.status.success(), "planning: {}", stderr(&planning));
    assert!(
        stdout(&planning).contains("planning ACCEPTED"),
        "planning output: {}",
        stdout(&planning)
    );
    for verb in ["claim", "start"] {
        let out = sandbox.agentctl(&["task", verb, task]);
        assert!(out.status.success(), "task {verb}: {}", stderr(&out));
    }
}

fn seed_takeover_decision(sandbox: &Sandbox, task: &str, branch: &str) -> String {
    let db = sandbox
        .root
        .join("data")
        .join("ai-engineering-os")
        .join("state.sqlite3");
    let branch_id = plan_domain::PlanBranchId::new(branch).unwrap();
    let planning = app_services::PlanningService::open(&db, app_services::SystemClock)
        .expect("open planning service");
    let accepted = planning
        .current_accepted_plan(&branch_id)
        .expect("accepted plan query")
        .expect("accepted plan");
    let mut decisions =
        app_services::HumanDecisionService::open(&db).expect("open human decision service");
    decisions
        .create(app_services::HumanDecisionCreate {
            task_id: task_domain::TaskId::new(task).unwrap(),
            plan_branch_id: branch_id,
            reason: plan_domain::HumanDecisionReason::PlannerBlocked,
            risk: plan_domain::RiskClass::Low,
            affected_scope: plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
            options: vec![plan_domain::HumanContinuation::Takeover],
            recommendation: None,
            evidence: vec![plan_domain::EvidenceRef::new(format!("hdq:{task}")).unwrap()],
            proposal_hash: None,
            plan_head: accepted.revision().revision_seq,
            planning_input_snapshot_id: accepted.planning_input_snapshot_id().clone(),
            planning_input_fingerprint: accepted.planning_input_fingerprint().to_owned(),
            policy_fingerprint: format!("policy:fixture:{task}"),
            expires_at_ms: None,
        })
        .expect("create takeover decision")
        .id
        .as_str()
        .to_owned()
}

fn sandbox_db(sandbox: &Sandbox) -> PathBuf {
    sandbox
        .root
        .join("data")
        .join("ai-engineering-os")
        .join("state.sqlite3")
}

fn seed_ready(sandbox: &Sandbox, project: &str, task: &str) {
    let created = sandbox.agentctl(&["project", "create", project, &format!("project {project}")]);
    assert!(
        created.status.success(),
        "seed project: {}",
        stderr(&created)
    );
    // `task` itself is materialized READY by the accepted plan below.
    let created = sandbox.agentctl(&[
        "task",
        "create",
        &fixture_parent(task),
        project,
        &format!("objective {task}"),
    ]);
    assert!(
        created.status.success(),
        "seed parent: {}",
        stderr(&created)
    );
    // Publication admits a decomposition only while the parent is doing work
    // (ADR-0027 TASK-0091 amendment), so the parent is declared eligible
    // through the shipped CLI before planning — the real operator sequence.
    let ready = sandbox.agentctl(&["task", "ready", &fixture_parent(task)]);
    assert!(
        ready.status.success(),
        "seed parent ready: {}",
        stderr(&ready)
    );
    seed_accepted_plan(sandbox, task);
}

fn fixture_parent(task: &str) -> String {
    format!("{task}-PARENT")
}

/// Publishes an accepted plan whose only child is `task`; acceptance
/// materializes it as a new READY Task under `<task>-PARENT` (TASK-0087).
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

/// Authors a planning Task through the shipped protocol-v6 control plane
/// (`agentctl task create-planned`) and declares it READY. No SQLite access:
/// the daemon's TaskService validates and commits the canonical metadata.
fn author_ready_planning_task(
    sandbox: &Sandbox,
    project: &str,
    task: &str,
    scope: &str,
    criterion: &str,
    risk: &str,
) {
    let created = sandbox.agentctl(&["project", "create", project, &format!("project {project}")]);
    assert!(created.status.success(), "project: {}", stderr(&created));
    let authored = sandbox.agentctl(&[
        "task",
        "create-planned",
        task,
        project,
        "planning",
        "fixture",
        "--scope",
        scope,
        "--acceptance",
        criterion,
        "--risk",
        risk,
    ]);
    assert!(authored.status.success(), "author: {}", stderr(&authored));
    assert_eq!(
        stdout(&authored).trim(),
        format!(
            "task {task} {project} DRAFT \"planning fixture\" scope={scope} acceptance={criterion} risk={risk}"
        )
    );
    let ready = sandbox.agentctl(&["task", "ready", task]);
    assert!(ready.status.success(), "ready: {}", stderr(&ready));
}

fn seed_human_decision(sandbox: &Sandbox, task: &str) -> String {
    seed_human_decision_with_options(
        sandbox,
        task,
        vec![
            plan_domain::HumanContinuation::Resume,
            plan_domain::HumanContinuation::Replan,
            plan_domain::HumanContinuation::Abort,
        ],
    )
}

fn seed_human_decision_with_options(
    sandbox: &Sandbox,
    task: &str,
    options: Vec<plan_domain::HumanContinuation>,
) -> String {
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
            options,
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
fn human_decision_cli_takeover_decides_on_v8() {
    // TASK-0177: shipped agentctl+agentd negotiate v8, so TAKEOVER now
    // decides. Below-v8 refusal stays pinned by the ipc-control and
    // agentd dispatch unit suites (a shipped v8 client cannot be forced
    // down to v4 through the CLI).
    let sandbox = Sandbox::new("human-decision-takeover-v8", SUCCESS_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    seed_ready(&sandbox, "P-TO", "T-TO");
    let decision_id = seed_human_decision_with_options(
        &sandbox,
        "T-TO",
        vec![plan_domain::HumanContinuation::Takeover],
    );

    let get = sandbox.agentctl(&["human-decision", "get", &decision_id]);
    assert!(get.status.success(), "get: {}", stderr(&get));
    assert!(stdout(&get).contains("TAKEOVER"), "get {:?}", stdout(&get));

    let decide = sandbox.agentctl(&["human-decision", "decide", &decision_id, "takeover"]);
    assert!(decide.status.success(), "decide: {}", stderr(&decide));
    let decided = stdout(&decide);
    assert!(decided.contains("RESOLVED"), "decided {decided:?}");
    assert!(decided.contains("selected TAKEOVER"), "decided {decided:?}");
}

fn task_state(sandbox: &Sandbox, task: &str) -> String {
    let out = sandbox.agentctl(&["task", "get", task]);
    assert!(out.status.success(), "task get: {}", stderr(&out));
    stdout(&out)
}

fn durable_takeover_status(sandbox: &Sandbox, record_id: &str) -> String {
    let store = storage_sqlite::SqliteStore::open(sandbox_db(sandbox)).expect("assertion store");
    let id = app_services::TakeoverRecordId::new(record_id.to_owned()).expect("record id");
    store
        .get_takeover(&id)
        .expect("read takeover")
        .expect("record exists")
        .status
        .as_str()
        .to_owned()
}

/// Fixture seeding only: claims a durable ACTIVE planning run for the
/// branch (mirrors the service-level TAKEOVER_ACTIVE_RUN fixture), so
/// the shipped `takeover-apply` must refuse typed.
fn seed_active_planning_run(sandbox: &Sandbox, task: &str, branch: &str) {
    let mut store = storage_sqlite::SqliteStore::open(sandbox_db(sandbox)).expect("run store");
    let branch_id = plan_domain::PlanBranchId::new(branch).unwrap();
    let planning =
        app_services::PlanningService::open(sandbox_db(sandbox), app_services::SystemClock)
            .expect("open planning service");
    let accepted = planning
        .current_accepted_plan(&branch_id)
        .expect("accepted plan query")
        .expect("accepted plan");
    let evidence = vec![plan_domain::EvidenceRef::new("fixture:active-run").unwrap()];
    let snapshot = plan_domain::PlanningInputSnapshot::new(
        branch_id,
        task.to_owned(),
        accepted.revision().revision_seq,
        plan_domain::ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
        plan_domain::RiskClass::Low,
        vec![],
        evidence.clone(),
        vec![],
        None,
        "policy:fixture:active-run".to_owned(),
    )
    .unwrap();
    store
        .start_human_gate_planning_run(
            &snapshot,
            &plan_domain::planning_evidence_key(&evidence),
            now_ms(),
        )
        .expect("start active planning run");
}

/// Fixture seeding only: gives the task a leased execution whose anchor
/// was not captured at TAKEOVER admission, so the return must refuse
/// as stale (mirrors the service-level moved-anchor fixture).
fn seed_leased_execution(sandbox: &Sandbox, task: &str, exec: &str, ws: &str) {
    use workspace::WorkspaceStore as _;
    let mut store = storage_sqlite::SqliteStore::open(sandbox_db(sandbox)).expect("lease store");
    let task_id = task_domain::TaskId::new(task).unwrap();
    let project = store
        .get_task(&task_id)
        .unwrap()
        .expect("task exists")
        .project_id()
        .clone();
    let execution = agent_runtime::Execution::begin(
        agent_runtime::ExecutionId::new(exec).unwrap(),
        task_id.clone(),
    );
    store
        .create_execution_with_event(
            &execution,
            &storage_sqlite::NewJournalEvent {
                event_type: storage_sqlite::EventType::ExecutionCreated,
                task_id: Some(task_id),
                project_id: Some(project),
                from_state: None,
                to_state: None,
                reason: None,
                correlation_id: format!("op-takeover-stale-{exec}"),
                causation_id: format!("req-takeover-stale-{exec}"),
                occurred_at: "2024-01-01T00:00:00Z".to_owned(),
                provenance: "agentctl-binary-test/fixture".to_owned(),
                execution_id: Some(execution.id().clone()),
                attempt_id: None,
                attempt_outcome: None,
                workspace_id: None,
                from_workspace_state: None,
                to_workspace_state: None,
                plan_revision: None,
            },
        )
        .expect("create execution");
    let lease = workspace::WorkspaceLease::allocate(
        workspace::WorkspaceId::new(ws).unwrap(),
        workspace::RepositoryRef::new("repo-local:test").unwrap(),
        execution.id().clone(),
        workspace::Revision::new("a".repeat(40)).unwrap(),
        workspace::BranchRef::new(format!("refs/heads/agent/{exec}")).unwrap(),
        sandbox
            .root
            .join(format!("lease-{ws}"))
            .display()
            .to_string(),
    )
    .expect("allocate lease");
    let anchor_dir = sandbox.root.join(format!("anchor-{ws}"));
    std::fs::create_dir_all(&anchor_dir).expect("anchor dir");
    let canonical = std::fs::canonicalize(&anchor_dir).expect("canonical anchor");
    let anchor = workspace::TrustedCommonGit::restore(canonical.to_str().expect("anchor path"))
        .expect("restore anchor");
    store
        .insert_allocated_lease_with_anchor(&lease, &anchor, now_ms())
        .expect("insert lease");
}

fn decide_takeover(sandbox: &Sandbox, decision_id: &str) {
    let decide = sandbox.agentctl(&["human-decision", "decide", decision_id, "takeover"]);
    assert!(decide.status.success(), "decide: {}", stderr(&decide));
    assert!(stdout(&decide).contains("selected TAKEOVER"));
}

fn apply_takeover(sandbox: &Sandbox, decision_id: &str, expires_at_ms: u64) -> Output {
    sandbox.agentctl(&[
        "human-decision",
        "takeover-apply",
        decision_id,
        &expires_at_ms.to_string(),
    ])
}

#[test]
fn takeover_assume_append_render_return_cycle_through_shipped_binaries() {
    let sandbox = Sandbox::new("takeover-cycle", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    setup_running_planned_task(&sandbox, "P-TO-CYCLE", "T-TO-CYCLE", "B-TO-CYCLE");
    assert!(task_state(&sandbox, "T-TO-CYCLE").contains("RUNNING"));
    let decision_id = seed_takeover_decision(&sandbox, "T-TO-CYCLE", "B-TO-CYCLE");
    decide_takeover(&sandbox, &decision_id);

    let applied = apply_takeover(&sandbox, &decision_id, now_ms() + 3_600_000);
    assert!(applied.status.success(), "apply: {}", stderr(&applied));
    assert!(
        stdout(&applied).contains("ACTIVE"),
        "apply {}",
        stdout(&applied)
    );
    let record_id = takeover_record_id(&applied);
    assert!(task_state(&sandbox, "T-TO-CYCLE").contains("BLOCKED"));

    // Consumed decision: a second apply of the same HDR refuses typed.
    let replay = apply_takeover(&sandbox, &decision_id, now_ms() + 3_600_000);
    assert!(!replay.status.success(), "consumed HDR must refuse");
    assert!(
        stderr(&replay).contains("STALE_STATE"),
        "consumed refusal: {}",
        stderr(&replay)
    );

    let appended = sandbox.agentctl(&[
        "human-decision",
        "takeover-append",
        &record_id,
        "--actions",
        "restart the workers",
        "--question",
        "which queue depth?",
    ]);
    assert!(appended.status.success(), "append: {}", stderr(&appended));
    assert!(stdout(&appended).contains("restart the workers"));

    let rendered = sandbox.agentctl(&["human-decision", "takeover-render", &record_id]);
    assert!(rendered.status.success(), "render: {}", stderr(&rendered));
    let packet = stdout(&rendered);
    assert!(packet.contains("T-TO-CYCLE"), "packet {packet:?}");
    assert!(packet.contains("restart the workers"), "packet {packet:?}");

    // Return: a fresh planning cycle on the parked scope completes the
    // takeover (BLOCKED -> RUNNING, record COMPLETED) on Accepted.
    sandbox.replace_provider_body(RETURN_PLANNING_BODY);
    let returned = sandbox.agentctl(&[
        "planning",
        "trigger",
        "T-TO-CYCLE",
        "B-TO-CYCLE",
        "fixture:takeover-return",
    ]);
    assert!(returned.status.success(), "return: {}", stderr(&returned));
    assert!(
        stdout(&returned).contains("planning ACCEPTED"),
        "return output: {}",
        stdout(&returned)
    );
    assert!(task_state(&sandbox, "T-TO-CYCLE").contains("RUNNING"));
    assert_eq!(durable_takeover_status(&sandbox, &record_id), "COMPLETED");
}

#[test]
fn takeover_apply_refuses_active_run_through_shipped_binaries() {
    let sandbox = Sandbox::new("takeover-active-run", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    setup_running_planned_task(&sandbox, "P-TO-RUN", "T-TO-RUN", "B-TO-RUN");
    let decision_id = seed_takeover_decision(&sandbox, "T-TO-RUN", "B-TO-RUN");
    decide_takeover(&sandbox, &decision_id);
    seed_active_planning_run(&sandbox, "T-TO-RUN", "B-TO-RUN");

    let applied = apply_takeover(&sandbox, &decision_id, now_ms() + 3_600_000);
    assert!(!applied.status.success(), "active run must refuse apply");
    assert!(
        stderr(&applied).contains("CONFLICT"),
        "active-run refusal: {}",
        stderr(&applied)
    );
    assert!(task_state(&sandbox, "T-TO-RUN").contains("RUNNING"));
}

#[test]
fn takeover_apply_refuses_stale_decision_through_shipped_binaries() {
    let sandbox = Sandbox::new("takeover-stale-decision", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    setup_running_planned_task(&sandbox, "P-TO-STALE", "T-TO-STALE", "B-TO-STALE");
    let decision_id = seed_takeover_decision(&sandbox, "T-TO-STALE", "B-TO-STALE");
    decide_takeover(&sandbox, &decision_id);

    // Drift the task under the resolved decision through the shipped
    // lifecycle CLI, then apply must refuse STALE_STATE and park
    // nothing. (A planning trigger cannot move the head here: the
    // gate correctly returns the Resolved decision as WAITING.)
    let blocked = sandbox.agentctl(&["task", "block", "T-TO-STALE", "drift for stale test"]);
    assert!(blocked.status.success(), "block: {}", stderr(&blocked));

    let applied = apply_takeover(&sandbox, &decision_id, now_ms() + 3_600_000);
    assert!(!applied.status.success(), "stale decision must refuse");
    assert!(
        stderr(&applied).contains("STALE_STATE"),
        "stale refusal: {}",
        stderr(&applied)
    );
    assert!(task_state(&sandbox, "T-TO-STALE").contains("BLOCKED"));
}

#[test]
fn takeover_return_refuses_stale_lease_through_shipped_binaries() {
    let sandbox = Sandbox::new("takeover-stale-lease", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    setup_running_planned_task(&sandbox, "P-TO-LEASE", "T-TO-LEASE", "B-TO-LEASE");
    let decision_id = seed_takeover_decision(&sandbox, "T-TO-LEASE", "B-TO-LEASE");
    decide_takeover(&sandbox, &decision_id);
    let applied = apply_takeover(&sandbox, &decision_id, now_ms() + 3_600_000);
    assert!(applied.status.success(), "apply: {}", stderr(&applied));
    let record_id = takeover_record_id(&applied);
    seed_leased_execution(&sandbox, "T-TO-LEASE", "E-TO-LEASE", "WS-TO-LEASE");

    sandbox.replace_provider_body(RETURN_PLANNING_BODY);
    let returned = sandbox.agentctl(&[
        "planning",
        "trigger",
        "T-TO-LEASE",
        "B-TO-LEASE",
        "fixture:takeover-return",
    ]);
    assert!(
        !returned.status.success(),
        "stale lease must refuse return: {}",
        stdout(&returned)
    );
    assert!(
        stderr(&returned).contains("gained execution"),
        "stale-lease refusal: {}",
        stderr(&returned)
    );
    assert!(task_state(&sandbox, "T-TO-LEASE").contains("BLOCKED"));
    assert_eq!(durable_takeover_status(&sandbox, &record_id), "ACTIVE");
}

#[test]
fn takeover_expiry_tick_reconciles_without_a_service_call() {
    let sandbox = Sandbox::new("takeover-expiry-tick", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    setup_running_planned_task(&sandbox, "P-TO-TICK", "T-TO-TICK", "B-TO-TICK");
    let decision_id = seed_takeover_decision(&sandbox, "T-TO-TICK", "B-TO-TICK");
    decide_takeover(&sandbox, &decision_id);
    let applied = apply_takeover(&sandbox, &decision_id, now_ms() + 1_500);
    assert!(applied.status.success(), "apply: {}", stderr(&applied));
    let record_id = takeover_record_id(&applied);
    assert!(task_state(&sandbox, "T-TO-TICK").contains("BLOCKED"));

    // No takeover-reconcile call: the daemon tick alone must abandon
    // the expired record and unpark the task.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if durable_takeover_status(&sandbox, &record_id) == "EXPIRED"
            && task_state(&sandbox, "T-TO-TICK").contains("RUNNING")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "daemon tick never reconciled expiry"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[test]
fn v5_planning_cli_publishes_an_accepted_plan_then_admits_execution() {
    let sandbox = Sandbox::new("v5-planning-cli", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();

    author_ready_planning_task(
        &sandbox,
        "P-PLAN",
        "T-PLAN",
        "repo:src",
        "criterion-root",
        "low",
    );

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

    // The accepted plan's work is the materialized child, never the parent.
    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-PLAN-CHILD",
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
fn v6_authored_task_plans_and_executes_without_sqlite_authority_seeding() {
    let sandbox = Sandbox::new("v6-authored-flow", PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();

    let project = sandbox.agentctl(&["project", "create", "P-V6", "authoring fixture"]);
    assert!(project.status.success(), "project: {}", stderr(&project));

    // Invalid authoring is refused by the daemon's TaskService and persists
    // nothing: the id stays free for a valid request.
    for invalid in [
        &[
            "--scope",
            "repo:src",
            "--acceptance",
            "criterion-root",
            "--risk",
            "severe",
        ][..],
        &[
            "--scope",
            "repo:../escape",
            "--acceptance",
            "criterion-root",
            "--risk",
            "low",
        ][..],
        &["--acceptance", "criterion-root", "--risk", "low"][..],
        &["--scope", "repo:src", "--risk", "low"][..],
    ] {
        let mut args = vec!["task", "create-planned", "T-PLAN", "P-V6", "flow"];
        args.extend_from_slice(invalid);
        let refused = sandbox.agentctl(&args);
        assert!(!refused.status.success(), "{invalid:?} must be refused");
        assert!(
            stderr(&refused).contains("INVALID_ARGUMENT"),
            "{invalid:?}: {}",
            stderr(&refused)
        );
        let missing = sandbox.agentctl(&["task", "get", "T-PLAN"]);
        assert!(
            stderr(&missing).contains("NOT_FOUND"),
            "{invalid:?} persisted"
        );
    }
    let usage = sandbox.agentctl(&[
        "task",
        "create-planned",
        "T-PLAN",
        "P-V6",
        "flow",
        "--scope",
    ]);
    assert!(!usage.status.success());
    assert!(stderr(&usage).contains("usage: agentctl task create-planned"));

    // A historical objective-only Task has no planning authority to derive.
    let legacy = sandbox.agentctl(&["task", "create", "T-LEGACY", "P-V6", "legacy"]);
    assert!(legacy.status.success(), "legacy: {}", stderr(&legacy));
    let unplannable = sandbox.agentctl(&["planning", "trigger", "T-LEGACY", "B-LEGACY", "e:1"]);
    assert!(!unplannable.status.success());
    assert!(
        stderr(&unplannable).contains("has no authorized scope"),
        "legacy planning: {}",
        stderr(&unplannable)
    );

    author_ready_planning_task(
        &sandbox,
        "P-V6-FLOW",
        "T-PLAN",
        "repo:src",
        "criterion-root",
        "low",
    );
    let duplicate = sandbox.agentctl(&[
        "task",
        "create-planned",
        "T-PLAN",
        "P-V6",
        "rewrite",
        "--scope",
        "repo:everything",
        "--acceptance",
        "criterion-root",
        "--risk",
        "critical",
    ]);
    assert!(
        !duplicate.status.success(),
        "authored intent cannot be rewritten"
    );
    assert!(
        stderr(&duplicate).contains("ALREADY_EXISTS"),
        "{}",
        stderr(&duplicate)
    );
    let read = sandbox.agentctl(&["task", "get", "T-PLAN"]);
    assert!(read.status.success(), "get: {}", stderr(&read));
    assert!(
        stdout(&read).contains("task T-PLAN P-V6-FLOW READY"),
        "{}",
        stdout(&read)
    );

    let planning = sandbox.agentctl(&["planning", "trigger", "T-PLAN", "B-V6", "fixture:v6"]);
    assert!(planning.status.success(), "planning: {}", stderr(&planning));
    assert!(
        stdout(&planning).contains("planning ACCEPTED branch=B-V6 revision=1"),
        "planning output: {}",
        stdout(&planning)
    );

    // The accepted plan's work is the materialized child, never the parent.
    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-PLAN-CHILD",
        &sandbox.repo.display().to_string(),
        "--execution",
        "E-V6",
        "--attempt",
        "A-V6",
    ]);
    assert!(run.status.success(), "execution: {}", stderr(&run));
    let status = sandbox.agentctl(&["execution", "status", "E-V6"]);
    assert!(status.status.success(), "status: {}", stderr(&status));
    assert!(
        stdout(&status).contains("execution E-V6"),
        "{}",
        stdout(&status)
    );

    // Read-only durable assertions. Every authority write above travelled
    // through agentctl -> agentd -> TaskService/PlanningService.
    let db = sandbox.root.join("data/ai-engineering-os/state.sqlite3");
    let store = storage_sqlite::SqliteStore::open(&db).expect("assertion store");
    let task_id = task_domain::TaskId::new("T-PLAN").unwrap();
    let facts = store.get_task(&task_id).unwrap().expect("task").snapshot();
    assert_eq!(facts.allowed_paths, ["repo:src"]);
    assert_eq!(facts.acceptance, ["criterion-root"]);
    assert_eq!(facts.risk.as_deref(), Some("low"));
    let task_events: Vec<_> = store
        .list_events_for_task(&task_id)
        .unwrap()
        .into_iter()
        .map(|event| event.fact.event_type.as_str())
        .collect();
    assert_eq!(
        &task_events[..2],
        ["task.created", "task.marked_ready"],
        "authoring is creation evidence; no out-of-band metadata write exists"
    );
    let planning = app_services::PlanningService::open(&db, app_services::SystemClock)
        .expect("assertion planning service");
    let branch = plan_domain::PlanBranchId::new("B-V6").unwrap();
    let accepted = planning
        .current_accepted_plan(&branch)
        .unwrap()
        .expect("accepted plan");
    let fingerprint = accepted.revision().validated_by.policy_fingerprint.clone();
    let canonical = planning
        .canonical_policy_context(&task_id, fingerprint.clone())
        .expect("canonical authority");
    assert!(planning
        .store()
        .planning_input_policy_matches(
            accepted.planning_input_snapshot_id(),
            "T-PLAN",
            &canonical.scope_bound,
            canonical.baseline_risk,
            &canonical.affected_parent_criteria,
            &canonical.lineage_fingerprints,
            None,
            &fingerprint,
        )
        .unwrap());
}

#[test]
fn v5_planning_cli_applies_risk_resume_without_replaying_the_planner() {
    let sandbox = Sandbox::new("v5-risk-resume-cli", RISK_PLANNING_BODY);
    let _daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();

    author_ready_planning_task(
        &sandbox,
        "P-RISK",
        "T-RISK",
        "repo:src",
        "criterion-root",
        "low",
    );

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

    author_ready_planning_task(
        &sandbox,
        "P-AUTH",
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

/// ADR-0039 P6c: through the shipped binaries, the production macOS daemon
/// (no verdict override) releases the vault credential into a real-like
/// provider child; the value never appears in the daemon's stderr or in any
/// file under the data directory, and the host login is never probed.
#[cfg(target_os = "macos")]
#[test]
fn brokered_launch_keeps_the_credential_out_of_daemon_logs_and_state() {
    use ipc_contracts::proto::{
        vault_core_request::Payload as Request, vault_core_response::Payload as Response,
        VaultCreateRequest, VaultStoreRequest,
    };
    const TOKEN: &str = "sk-ant-oat01-P6C-BINARY-MARKER-91d2";
    const PASSWORD: &str = "p6c-binary-vault-password";

    let probe_marker = std::env::temp_dir().join(format!(
        "runlith-p6c-host-login-probed-{}",
        std::process::id()
    ));
    let body = format!(
        "if [ \"$1\" = \"--version\" ]; then printf '2.1.277 (Claude Code)\\n'; exit 0; fi\n\
         if [ \"$1\" = \"auth\" ]; then touch {probe_marker:?}; exit 0; fi\n\
         if [ \"$CLAUDE_CODE_OAUTH_TOKEN\" != '{TOKEN}' ]; then exit 97; fi\n\
         {SUCCESS_BODY}"
    );
    let sandbox = Sandbox::new("brokered", &body);
    let log = sandbox.root.join("agentd.stderr");
    let mut daemon = DaemonGuard {
        child: Some(sandbox.spawn_agentd_logging(&log)),
    };
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-BR", "T-BR");
    for request in [
        Request::Create(VaultCreateRequest {
            password: PASSWORD.into(),
            idle_minutes: 0,
        }),
        Request::Store(VaultStoreRequest {
            password: PASSWORD.into(),
            name: ipc_contracts::VAULT_PROVIDER_CREDENTIAL_ENTRY.to_owned(),
            value: TOKEN.into(),
        }),
    ] {
        if let Response::Error(error) = sandbox.vault(request) {
            panic!("vault setup failed: {}", error.message);
        }
    }

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-BR",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-BR",
        "--attempt",
        "A-BR",
    ]);
    assert!(
        run.status.success() && stdout(&run).contains("SUCCEEDED"),
        "brokered run succeeds only with the vault token: stdout={} stderr={}",
        stdout(&run),
        stderr(&run)
    );
    daemon.kill();

    let daemon_log = std::fs::read(&log).expect("daemon log");
    assert!(
        !daemon_log
            .windows(TOKEN.len())
            .any(|window| window == TOKEN.as_bytes()),
        "credential value in daemon stderr"
    );
    assert!(!stdout(&run).contains(TOKEN) && !stderr(&run).contains(TOKEN));
    assert_absent_under(&sandbox.root.join("data"), TOKEN.as_bytes());
    assert!(
        !probe_marker.exists(),
        "brokered mode must not probe the host login"
    );
}

fn assert_absent_under(root: &Path, needle: &[u8]) {
    for entry in std::fs::read_dir(root).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            assert_absent_under(&path, needle);
        } else if let Ok(bytes) = std::fs::read(&path) {
            assert!(
                !bytes.windows(needle.len()).any(|window| window == needle),
                "credential value found in {}",
                path.display()
            );
        }
    }
}

#[test]
fn memory_capture_validate_list_cycle_through_shipped_binaries() {
    let sandbox = Sandbox::new("memory-cycle", SUCCESS_BODY);
    let mut daemon = DaemonGuard::spawn(&sandbox);
    sandbox.wait_ready();
    let fingerprint = format!("sha256:{}", "ab".repeat(32));
    let anchor = format!("CODE:crates/x/src/lib.rs:{fingerprint}");
    let captured = sandbox.agentctl(&[
        "memory",
        "capture",
        "--kind",
        "SEMANTIC",
        "--content",
        "run codegen after protobuf changes",
        "--source",
        "TASK:TASK-1:rev1",
        "--confidence",
        "900",
        "--sensitivity",
        "PROJECT",
        "--anchor",
        &anchor,
        "--author",
        "agent-a",
    ]);
    assert!(captured.status.success(), "capture: {}", stderr(&captured));
    let out = stdout(&captured);
    assert!(out.contains("PROPOSED"), "capture {out}");
    let id = out.split_whitespace().next().expect("memory id").to_owned();
    assert!(id.starts_with("mem_"), "id {id}");

    // Author self-validation refuses through the shipped CLI.
    let refused = sandbox.agentctl(&["memory", "validate", &id, "--validator", "agent-a"]);
    assert!(!refused.status.success(), "author validation must refuse");
    assert!(
        stderr(&refused).contains("untrusted_promotion"),
        "refusal: {}",
        stderr(&refused)
    );

    // Proposed memory is not eligible; after reviewer validation it is.
    let listed = sandbox.agentctl(&["memory", "list"]);
    assert!(
        stdout(&listed).contains("eligible 0"),
        "list {}",
        stdout(&listed)
    );
    let validated = sandbox.agentctl(&["memory", "validate", &id, "--validator", "reviewer"]);
    assert!(
        validated.status.success(),
        "validate: {}",
        stderr(&validated)
    );
    assert!(stdout(&validated).contains("CONFIRMED"));
    let listed = sandbox.agentctl(&["memory", "list"]);
    assert!(
        stdout(&listed).contains("eligible 1"),
        "list {}",
        stdout(&listed)
    );

    // Durability: kill and restart the daemon; memory survives.
    daemon.restart(&sandbox);
    let got = sandbox.agentctl(&["memory", "get", &id]);
    assert!(got.status.success(), "get: {}", stderr(&got));
    assert!(stdout(&got).contains("CONFIRMED"), "get {}", stdout(&got));

    // SECRET capture refuses typed through the shipped binaries.
    let secret = sandbox.agentctl(&[
        "memory",
        "capture",
        "--kind",
        "SEMANTIC",
        "--content",
        "hidden",
        "--source",
        "TASK:TASK-1:rev1",
        "--confidence",
        "1",
        "--sensitivity",
        "SECRET",
        "--anchor",
        &anchor,
        "--author",
        "agent-a",
    ]);
    assert!(!secret.status.success(), "SECRET capture must refuse");
    assert!(
        stderr(&secret).contains("sensitivity_refusal"),
        "secret refusal: {}",
        stderr(&secret)
    );
}
