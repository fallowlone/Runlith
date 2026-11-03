//! A task that declares a write scope (allowed paths) cannot be reported
//! as a successful attempt when the provider left the worktree untouched
//! (e.g. "I couldn't complete this task, and I made no changes").

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use app_services::{
    FixedClock, ManagedWorkspaceRoot, OrchestrationIds, OrchestrationService, WorkspaceRequest,
    WorkspaceService,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use git_cli::GitCli;
use scheduler::{ExecutionPort, ExecutionProblem, OrchestrationOutcome, WorkEvidence, WorkItem};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

const MILLIS: u64 = 1_704_067_200_000;

struct Env {
    dir: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(repo: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=f", "-c", "user.email=f@example.invalid"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .status()
        .expect("git")
        .success();
    assert!(ok, "git {args:?}");
}

struct Stub;

impl ExecutionPort for Stub {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("no-change-stub").unwrap()
    }
    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        Ok(WorkEvidence {
            summary: "I couldn't complete this task, and I made no changes".to_owned(),
            evidence_ref: format!("stub:{}", work.attempt_id),
            partial: false,
        })
    }
}

/// Runs one cycle; `write` decides whether the stub touches the worktree.
fn run_cycle(
    tag: &str,
    scoped: bool,
    write: bool,
) -> (
    Result<OrchestrationOutcome, app_services::ServiceError>,
    TaskState,
) {
    let dir = std::env::temp_dir().join(format!("runlith-nochange-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let env = Env { dir: dir.clone() };
    let (db, repo, root) = (dir.join("t.db"), dir.join("repo"), dir.join("roots"));
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let root = std::fs::canonicalize(&root).unwrap();

    let mut orch = OrchestrationService::open(&db, FixedClock(MILLIS)).unwrap();
    let project = ProjectId::new("P-1").unwrap();
    orch.tasks_mut()
        .create_project(&Project::new(project.clone(), "p").unwrap())
        .unwrap();
    let mut task = Task::draft(TaskId::new("T-1").unwrap(), project, "make hello").unwrap();
    if scoped {
        task.define_planning_metadata(vec!["repo:src".to_owned()], vec!["c1".to_owned()], "low")
            .unwrap();
    }
    orch.tasks_mut().create_task(&task).unwrap();
    orch.tasks_mut().mark_ready(task.id()).unwrap();
    let mut ws = WorkspaceService::open(
        &db,
        GitCli::new(),
        ManagedWorkspaceRoot::new(&root).unwrap(),
        FixedClock(MILLIS),
    )
    .unwrap();
    let exec = ExecutionId::new("E-1").unwrap();
    let request = WorkspaceRequest {
        execution_id: exec.clone(),
        repository_ref: workspace::RepositoryRef::new("repo-local:nochange").unwrap(),
        repository_path: repo.to_string_lossy().into_owned(),
        base_revision: "HEAD".to_owned(),
    };
    let outcome = orch.schedule_in_workspace(
        &mut ws,
        task.id(),
        OrchestrationIds {
            execution_id: exec,
            attempt_id: AttemptId::new("A-1").unwrap(),
        },
        request,
        |_, lease| {
            if write {
                std::fs::write(Path::new(lease.worktree_ref()).join("hello.txt"), "hi").unwrap();
            }
            Stub
        },
        &FakeEvaluator::new(EvaluatorMode::AlwaysVerified),
        None,
        None,
    );
    let state = orch
        .tasks_mut()
        .get_task(task.id())
        .unwrap()
        .unwrap()
        .state();
    drop(env);
    (outcome, state)
}

#[test]
fn scoped_task_with_no_worktree_change_is_not_a_success() {
    let (outcome, state) = run_cycle("scoped-empty", true, false);
    assert!(
        matches!(outcome, Ok(OrchestrationOutcome::ExecutionFailed { .. })),
        "{outcome:?}"
    );
    assert_ne!(state, TaskState::Verifying);
}

#[test]
fn scoped_task_with_a_change_still_succeeds() {
    // No verifier evidence is bound here, so completion admission refuses
    // after the attempt succeeded and the task is left VERIFYING (G5).
    let (_, state) = run_cycle("scoped-write", true, true);
    assert_eq!(state, TaskState::Verifying);
}

#[test]
fn unscoped_task_without_change_keeps_existing_behaviour() {
    let (outcome, _) = run_cycle("unscoped-empty", false, false);
    assert!(
        matches!(
            outcome,
            Ok(OrchestrationOutcome::ExecutionSucceededAndVerified { .. })
        ),
        "{outcome:?}"
    );
}
