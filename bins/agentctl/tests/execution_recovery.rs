//! Post-admission recovery across a real daemon restart (TASK-0054).
//!
//! Shipped `agentd` + `agentctl` processes against test-owned runtime/data
//! directories, with a fixture provider named `claude` on `PATH`. A
//! detached `run` followed by an immediate `SIGSTOP` + `SIGKILL` strands
//! the entry pre-attempt (the background half never runs — the frozen
//! process is killed); the restarted daemon then reports the honest
//! intermediate, and `execution resume` drives the SAME execution to its
//! terminal outcome through the production control plane.
//!
//! Unix-only. No quota, no network, no real Claude.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use workspace::WorkspaceStore;

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
            PathBuf::from("/tmp").join(format!("g5-recovery-{}-{suite}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).expect("sandbox root");
        std::fs::create_dir_all(root.join("run")).expect("runtime parent");
        std::fs::create_dir_all(root.join("data")).expect("data parent");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
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

    fn install_branch_only_git_shim(&self) -> PathBuf {
        let real = Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .expect("locate real git");
        assert!(real.status.success());
        let real_git = String::from_utf8_lossy(&real.stdout).trim().to_owned();
        assert!(!real_git.is_empty());
        let marker = self.root.join("branch-only.marker");
        let shim = self.root.join("bin").join("git");
        let body = format!(
            r#"#!/bin/sh
REAL_GIT='{real_git}'
MARKER='{marker}'
# Production run_git prefixes global `-c k=v` hardening flags before the
# subcommand (TASK-0071). Peek past those pairs WITHOUT consuming "$@",
# so unmatched invocations still exec faithfully with every flag intact.
# What the test proves is unchanged.
stripped=0
sub1=''
sub2=''
sub3=''
skip=0
for arg in "$@"; do
  if [ "$skip" -gt 0 ]; then skip=$((skip - 1)); continue; fi
  case "$arg" in
    -c) skip=1; stripped=$((stripped + 2)); continue ;;
  esac
  if [ -z "$sub1" ]; then sub1="$arg";
  elif [ -z "$sub2" ]; then sub2="$arg";
  elif [ -z "$sub3" ]; then sub3="$arg"; break; fi
done
if [ "$sub1" = "worktree" ] && [ "$sub2" = "add" ] && [ "$sub3" = "-b" ]; then
  shift "$stripped"
  shift 2
  branch=''
  target=''
  base=''
  while [ "$#" -gt 0 ]; do
    case "$1" in
      -b) branch="$2"; shift 2 ;;
      --) shift; target="$1"; shift; base="$1"; shift ;;
      *) shift ;;
    esac
  done
  if [ -n "$branch" ] && [ -n "$target" ] && [ -n "$base" ]; then
    "$REAL_GIT" branch "$branch" "$base" || exit $?
    echo $$ > "$MARKER"
    while :; do sleep 1; done
  fi
fi
exec "$REAL_GIT" "$@"
"#,
            marker = marker.display(),
        );
        std::fs::write(&shim, body).expect("write git shim");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))
            .expect("git shim chmod");
        marker
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let output = self.agentctl(&["ping"]);
            if output.status.success() {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "daemon never became ready: {}",
                    String::from_utf8_lossy(&output.stderr)
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

fn git_stdout(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("fixture git output");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Freeze then kill: `SIGSTOP` halts every daemon thread first, so the
/// detached background half can never reach attempt creation afterwards;
/// `SIGKILL` then ends the process. Reaps the child.
fn freeze_and_kill(mut child: Child) {
    let pid = child.id().to_string();
    let stopped = Command::new("kill")
        .args(["-STOP", &pid])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(stopped, "SIGSTOP must reach the daemon");
    let _ = Command::new("kill")
        .args(["-KILL", &pid])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.wait();
}

fn kill_pid_file(marker: &Path) {
    let Ok(raw) = std::fs::read_to_string(marker) else {
        return;
    };
    let pid = raw.trim();
    if pid.is_empty() {
        return;
    }
    let _ = Command::new("kill")
        .args(["-KILL", pid])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn stop_daemon(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn seed_ready(sandbox: &Sandbox, project: &str, task: &str) {
    let created = sandbox.agentctl(&["project", "create", project, &format!("project {project}")]);
    if !created.status.success() && !stderr(&created).contains("ALREADY_EXISTS") {
        panic!("seed project: {}", stderr(&created));
    }
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

const SUCCESS_BODY: &str = r#"printf '{"type":"result","subtype":"success","is_error":false,"result":"binary recovery fixture finished","session_id":"dddddddd-dddd-dddd-dddd-dddddddddddd"}'; exit 0
"#;

/// Stranded status shape: RUNNING, nothing live, zero attempt lines.
fn is_stranded(status_out: &str, execution: &str) -> bool {
    status_out.contains(execution)
        && status_out.contains("RUNNING")
        && status_out.contains("live=false")
        && !status_out.lines().any(|line| line.starts_with("attempt "))
}

#[test]
fn daemon_restart_recovers_stranded_entry_through_resume() {
    let sandbox = Sandbox::new("restart", SUCCESS_BODY);
    let mut daemon = sandbox.spawn_agentd();
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-BASE");

    // Boundedly many strand attempts: each round admits a fresh
    // task/execution and freezes the daemon immediately after the
    // detached accept. A round whose background already created an
    // attempt is discarded and retried with fresh identities — only the
    // exact pre-attempt shape proceeds to recovery.
    let mut stranded: Option<(String, String)> = None;
    for round in 0..6 {
        let task = format!("T-R{round}");
        let execution = format!("E-R{round}");
        let attempt = format!("A-R{round}");
        seed_ready(&sandbox, "P-1", &task);
        let run = sandbox.agentctl(&[
            "execution",
            "run",
            &task,
            &sandbox.repo.to_string_lossy(),
            "--execution",
            &execution,
            "--attempt",
            &attempt,
            "--detach",
        ]);
        assert!(
            run.status.success(),
            "detach starts (round {round}): {}",
            stderr(&run)
        );
        freeze_and_kill(daemon);
        daemon = sandbox.spawn_agentd();
        sandbox.wait_ready();
        let status = sandbox.agentctl(&["execution", "status", &execution]);
        assert!(status.status.success(), "status reads: {}", stderr(&status));
        let out = stdout(&status);
        if is_stranded(&out, &execution) {
            stranded = Some((task, execution));
            break;
        }
    }
    let (task, execution) = stranded.expect(
        "a pre-attempt strand must be observable within 6 freeze rounds (background needs Git subprocess time; STOP arrives first)",
    );

    // Honest intermediate through the restarted control plane.
    let status = sandbox.agentctl(&["execution", "status", &execution]);
    assert!(is_stranded(&stdout(&status), &execution));
    let task_get = sandbox.agentctl(&["task", "get", &task]);
    assert!(stdout(&task_get).contains("RUNNING"));

    // cancel no longer claims the strand already concluded.
    let cancelled = sandbox.agentctl(&["execution", "cancel", &execution]);
    assert!(
        cancelled.status.success(),
        "cancel prints: {}",
        stderr(&cancelled)
    );
    let out = stdout(&cancelled);
    assert!(
        out.contains("AWAITING_RESUME"),
        "cancel names the recovery posture, got {out:?}"
    );
    assert!(
        out.contains("resume"),
        "cancel points at resume, got {out:?}"
    );

    // Recovery through the product: resume re-drives the same execution
    // with its reserved attempt.
    let resumed = sandbox.agentctl(&[
        "execution",
        "resume",
        &execution,
        &sandbox.repo.to_string_lossy(),
        "--ref",
        &format!("repo-local:{task}"),
        "--rev",
        "HEAD",
    ]);
    assert!(
        resumed.status.success(),
        "resume accepts: stdout={} stderr={}",
        stdout(&resumed),
        stderr(&resumed)
    );
    let out = stdout(&resumed);
    assert!(
        out.contains("resumed") && out.contains(&execution),
        "got {out:?}"
    );

    // The resumed background reaches the terminal provider outcome; the
    // Task verifies (never provider-completes).
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let status = sandbox.agentctl(&["execution", "status", &execution]);
        assert!(status.status.success());
        let out = stdout(&status);
        if out.contains("SUCCEEDED") {
            break;
        }
        if Instant::now() >= deadline {
            panic!("resumed execution never concluded: {out:?}");
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let task_get = sandbox.agentctl(&["task", "get", &task]);
    assert!(
        stdout(&task_get).contains("VERIFYING"),
        "provider success verifies, never completes: {}",
        stdout(&task_get)
    );
    let again = sandbox.agentctl(&["execution", "cancel", &execution]);
    assert!(stdout(&again).contains("ALREADY_CONCLUDED"));
    stop_daemon(daemon);
}

#[test]
fn daemon_sigkill_after_branch_creation_recovers_branch_only_through_binaries() {
    let sandbox = Sandbox::new("branch-only-sigkill", SUCCESS_BODY);
    let marker = sandbox.install_branch_only_git_shim();
    let mut daemon = sandbox.spawn_agentd();
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-BRANCH");

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-BRANCH",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-BRANCH",
        "--attempt",
        "A-BRANCH",
        "--detach",
    ]);
    assert!(run.status.success(), "detach starts: {}", stderr(&run));

    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        if Instant::now() >= deadline {
            panic!("git shim never reached branch-only crash seam");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let base = git_stdout(&sandbox.repo, &["rev-parse", "HEAD"]);
    let refs = git_stdout(
        &sandbox.repo,
        &["for-each-ref", "--format=%(refname)", "refs/heads/agent/"],
    );
    let managed: Vec<_> = refs.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(managed.len(), 1, "exactly one managed branch: {managed:?}");
    assert_eq!(git_stdout(&sandbox.repo, &["rev-parse", managed[0]]), base);
    let listed = git_stdout(&sandbox.repo, &["worktree", "list", "--porcelain"]);
    assert!(
        !listed
            .lines()
            .any(|line| line == format!("branch {}", managed[0])),
        "managed branch must not yet be registered: {listed}"
    );

    let db = sandbox
        .root
        .join("data")
        .join("ai-engineering-os")
        .join("state.sqlite3");
    let store = storage_sqlite::SqliteStore::open(&db).expect("open store");
    let execution = agent_runtime::ExecutionId::new("E-BRANCH").unwrap();
    let lease = store
        .get_workspace_for_execution(&execution)
        .expect("read lease")
        .expect("allocated lease");
    assert_eq!(lease.lifecycle(), workspace::LeaseLifecycle::Allocated);
    assert_eq!(lease.base_revision().as_str(), base);
    assert_eq!(lease.branch().as_str(), managed[0]);
    assert!(!Path::new(lease.worktree_ref()).exists());
    assert_eq!(
        store
            .find_identity_reservation_owner(lease.branch().as_str())
            .expect("reservation owner"),
        Some(execution.clone())
    );
    assert!(
        store
            .get_attempt_reservation(&agent_runtime::AttemptId::new("A-BRANCH").unwrap())
            .expect("attempt reservation")
            .is_some(),
        "reserved attempt must survive the crash seam"
    );
    assert_eq!(
        store
            .list_events_ordered()
            .expect("journal")
            .iter()
            .filter(|event| event.fact.event_type.as_str() == "workspace.created")
            .count(),
        0
    );
    drop(store);

    freeze_and_kill(daemon);
    kill_pid_file(&marker);
    daemon = sandbox.spawn_agentd();
    sandbox.wait_ready();

    let status = sandbox.agentctl(&["execution", "status", "E-BRANCH"]);
    let out = stdout(&status);
    assert!(status.status.success());
    assert!(is_stranded(&out, "E-BRANCH"), "after restart: {out}");

    let resumed = sandbox.agentctl(&[
        "execution",
        "resume",
        "E-BRANCH",
        &sandbox.repo.to_string_lossy(),
    ]);
    assert!(
        resumed.status.success(),
        "branch-only resume: stdout={} stderr={}",
        stdout(&resumed),
        stderr(&resumed)
    );

    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let status = sandbox.agentctl(&["execution", "status", "E-BRANCH"]);
        assert!(status.status.success(), "status: {}", stderr(&status));
        let out = stdout(&status);
        if out.contains("SUCCEEDED") {
            assert_eq!(
                out.lines()
                    .filter(|line| line.starts_with("attempt "))
                    .count(),
                1,
                "exactly one Attempt: {out}"
            );
            break;
        }
        if Instant::now() >= deadline {
            panic!("branch-only recovery never concluded: {out}");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let task = sandbox.agentctl(&["task", "get", "T-BRANCH"]);
    assert!(stdout(&task).contains("VERIFYING"));

    let store = storage_sqlite::SqliteStore::open(&db).expect("reopen store");
    assert!(store
        .get_attempt_reservation(&agent_runtime::AttemptId::new("A-BRANCH").unwrap())
        .expect("reservation consumed")
        .is_none());
    let events = store.list_events_ordered().expect("journal");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.fact.event_type.as_str() == "attempt.created")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.fact.event_type.as_str() == "workspace.created")
            .count(),
        1
    );
    stop_daemon(daemon);
}

#[test]
fn run_refuses_bad_inputs_before_mutation_through_binaries() {
    let sandbox = Sandbox::new("refuse", SUCCESS_BODY);
    let daemon = sandbox.spawn_agentd();
    sandbox.wait_ready();
    seed_ready(&sandbox, "P-1", "T-1");

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        "/nonexistent-repository-path",
        "--execution",
        "E-BAD",
        "--attempt",
        "A-BAD",
        "--detach",
    ]);
    assert!(
        !run.status.success(),
        "non-repository path must refuse: {}",
        stdout(&run)
    );
    assert!(
        stderr(&run).contains("INVALID_ARGUMENT"),
        "typed refusal, got {:?}",
        stderr(&run)
    );

    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        &sandbox.repo.to_string_lossy(),
        "--rev",
        "no-such-branch",
        "--execution",
        "E-BAD2",
        "--attempt",
        "A-BAD2",
        "--detach",
    ]);
    assert!(!run.status.success(), "bad revision must refuse");
    assert!(
        stderr(&run).contains("INVALID_ARGUMENT"),
        "typed refusal, got {:?}",
        stderr(&run)
    );

    // Both refusals committed nothing: the task is still READY and a
    // valid run with fresh identities proceeds.
    let task = sandbox.agentctl(&["task", "get", "T-1"]);
    assert!(
        stdout(&task).contains("READY"),
        "refused runs mutate nothing: {}",
        stdout(&task)
    );
    let run = sandbox.agentctl(&[
        "execution",
        "run",
        "T-1",
        &sandbox.repo.to_string_lossy(),
        "--execution",
        "E-OK",
        "--attempt",
        "A-OK",
        "--detach",
    ]);
    assert!(run.status.success(), "valid run proceeds: {}", stderr(&run));
    stop_daemon(daemon);
}
