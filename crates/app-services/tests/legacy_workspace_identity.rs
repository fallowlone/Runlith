//! TASK-0058 keepers for persisted pre-v2 WorkspaceLease identity.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use app_services::{
    branch_for_execution, EnsureOutcome, FixedClock, ManagedWorkspaceRoot, RuntimeService,
    SystemClock, TaskService, WorkspaceRequest, WorkspaceService,
};
use git_cli::GitCli;
use rusqlite::{params, Connection};
use task_domain::{Project, ProjectId, Task, TaskId};
use workspace::{LeaseLifecycle, RepositoryRef};

static COUNTER: AtomicU64 = AtomicU64::new(0);
const FIXED_MILLIS: u64 = 1_704_067_200_000;

struct Env {
    dir: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    root: PathBuf,
}

impl Env {
    fn new(label: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-task0058-{label}-{id}-{}",
            std::process::id()
        ));
        let db = dir.join("state.db");
        let repo = dir.join("repo");
        let root = dir.join("managed");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        Self {
            dir,
            db,
            repo,
            root,
        }
    }

    fn seed_execution_named(&self, project: &str, task: &str, execution: &str) {
        let mut tasks = TaskService::open(&self.db, SystemClock).unwrap();
        let project_id = ProjectId::new(project).unwrap();
        tasks
            .create_project(&Project::new(project_id.clone(), "project".to_owned()).unwrap())
            .unwrap();
        let task_id = TaskId::new(task).unwrap();
        tasks
            .create_task(&Task::draft(task_id.clone(), project_id, "objective".to_owned()).unwrap())
            .unwrap();
        drop(tasks);

        let mut runtime = RuntimeService::open(&self.db, SystemClock).unwrap();
        runtime
            .create_execution(ExecutionId::new(execution).unwrap(), task_id)
            .unwrap();
    }

    fn seed_execution(&self, execution: &str) {
        self.seed_execution_named("P-1", "T-1", execution);
    }

    fn service(&self) -> WorkspaceService<GitCli, FixedClock> {
        WorkspaceService::open(
            &self.db,
            GitCli::new(),
            ManagedWorkspaceRoot::new(&self.root).unwrap(),
            FixedClock(FIXED_MILLIS),
        )
        .unwrap()
    }

    fn repo_path(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
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
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn insert_task0051_lease(
    env: &Env,
    execution: &str,
    workspace_id: &str,
    lifecycle: &str,
) -> (String, String) {
    let branch = format!("refs/heads/agent/{execution}");
    let root = std::fs::canonicalize(&env.root).unwrap();
    let worktree = root.join(format!("ws-{workspace_id}"));
    let worktree = worktree.to_string_lossy().into_owned();
    let base = git(&env.repo, &["rev-parse", "HEAD"]);

    let connection = Connection::open(&env.db).unwrap();
    connection
        .execute(
            "INSERT INTO workspace_leases
             (workspace_id, execution_id, repository_ref, base_revision,
              branch_ref, worktree_ref, lifecycle)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                workspace_id,
                execution,
                "repo-local:legacy",
                base,
                branch,
                worktree,
                lifecycle
            ],
        )
        .unwrap();
    (branch, worktree)
}

fn request(env: &Env, execution: &str) -> WorkspaceRequest {
    WorkspaceRequest {
        execution_id: ExecutionId::new(execution).unwrap(),
        repository_ref: RepositoryRef::new("repo-local:legacy").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    }
}

#[test]
fn task0051_prefixed_allocated_leases_resume_from_persisted_identity() {
    for execution in ["e2-legacy", "h2-legacy"] {
        let env = Env::new(execution);
        env.seed_execution(execution);
        let workspace_id = format!("ws-{execution}");
        let (branch, worktree) = insert_task0051_lease(&env, execution, &workspace_id, "ALLOCATED");

        let mut service = env.service();
        let ensured = service
            .ensure_workspace_ready(&request(&env, execution))
            .unwrap_or_else(|error| panic!("{execution} legacy lease must resume: {error}"));

        assert_eq!(ensured.outcome, EnsureOutcome::ResumedAllocated);
        assert_eq!(ensured.lease.branch().as_str(), branch);
        assert_eq!(ensured.lease.worktree_ref(), worktree);
        assert_eq!(ensured.lease.lifecycle(), LeaseLifecycle::Ready);
        assert!(Path::new(&worktree).is_dir());
    }
}

#[test]
fn task0051_prefixed_ready_leases_cleanup_from_persisted_identity() {
    for execution in ["e2-legacy", "h2-legacy"] {
        let env = Env::new(&format!("ready-{execution}"));
        env.seed_execution(execution);
        let workspace_id = format!("ws-{execution}");
        let (branch, worktree) = insert_task0051_lease(&env, execution, &workspace_id, "READY");
        let short_branch = branch.strip_prefix("refs/heads/").unwrap();
        git(
            &env.repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                short_branch,
                &worktree,
                "HEAD",
            ],
        );
        git(&env.repo, &["pack-refs", "--all"]);

        let mut service = env.service();
        let id = workspace::WorkspaceId::new(workspace_id).unwrap();
        let cleaned = service
            .cleanup_workspace(&id, &env.repo_path())
            .unwrap_or_else(|error| panic!("{execution} legacy lease must clean up: {error}"));

        assert_eq!(cleaned.lease.lifecycle(), LeaseLifecycle::Removed);
        assert!(!Path::new(&worktree).exists());
        assert!(!git(&env.repo, &["show-ref"]).contains(&branch));
    }
}

#[test]
fn corrupted_current_v2_lease_cannot_fall_back_to_legacy() {
    let execution = "e2-modern";
    let env = Env::new("corrupt-v2-fallback");
    env.seed_execution(execution);

    let lease = env
        .service()
        .allocate_workspace(&request(&env, execution))
        .unwrap();
    let reserved_v2_branch = lease.branch().as_str().to_owned();
    assert_eq!(
        reserved_v2_branch,
        branch_for_execution(&ExecutionId::new(execution).unwrap())
            .unwrap()
            .as_str()
    );

    // Mutate only the lease row into a complete historical-looking pair.
    // The durable v2 reservation remains authoritative evidence of the
    // format that was actually admitted.
    let legacy_branch = format!("refs/heads/agent/{execution}");
    let legacy_worktree = std::fs::canonicalize(&env.root)
        .unwrap()
        .join(format!("ws-{}", lease.id().as_str()))
        .to_string_lossy()
        .into_owned();
    let connection = Connection::open(&env.db).unwrap();
    connection
        .execute(
            "UPDATE workspace_leases SET branch_ref = ?1, worktree_ref = ?2 WHERE workspace_id = ?3",
            params![legacy_branch, legacy_worktree, lease.id().as_str()],
        )
        .unwrap();

    let error = env
        .service()
        .ensure_workspace_ready(&request(&env, execution))
        .unwrap_err();
    assert!(
        matches!(error, workspace::WorkspaceFailure::OwnershipMismatch { .. }),
        "corrupted current-v2 row must fail closed, got {error:?}"
    );
    assert!(error.to_string().contains("durable reservation"));

    let reserved: String = connection
        .query_row(
            "SELECT branch_ref FROM managed_identity_reservations WHERE execution_id = ?1",
            [execution],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reserved, reserved_v2_branch);
}

#[test]
fn mixed_historical_and_v2_identity_components_fail_closed() {
    let execution = "e2-mixed";
    let env = Env::new("mixed-pair");
    env.seed_execution(execution);
    let workspace_id = "ws-e2-mixed";
    let (_legacy_branch, legacy_worktree) =
        insert_task0051_lease(&env, execution, workspace_id, "ALLOCATED");
    let v2_branch = branch_for_execution(&ExecutionId::new(execution).unwrap()).unwrap();
    let connection = Connection::open(&env.db).unwrap();
    connection
        .execute(
            "UPDATE workspace_leases SET branch_ref = ?1 WHERE workspace_id = ?2",
            params![v2_branch.as_str(), workspace_id],
        )
        .unwrap();

    let error = env
        .service()
        .ensure_workspace_ready(&request(&env, execution))
        .unwrap_err();
    assert!(
        matches!(error, workspace::WorkspaceFailure::OwnershipMismatch { .. }),
        "mixed identity must fail closed, got {error:?}"
    );
    assert!(Path::new(&legacy_worktree).parent().is_some());
}

#[test]
fn compatibility_specific_corruption_matrix_fails_closed() {
    fn expect_mismatch(
        result: Result<app_services::EnsuredWorkspace, workspace::WorkspaceFailure>,
    ) {
        let error = result.unwrap_err();
        assert!(
            matches!(error, workspace::WorkspaceFailure::OwnershipMismatch { .. }),
            "compatibility corruption must fail as OwnershipMismatch, got {error:?}"
        );
    }

    // Current-v2 branch + wrong path.
    {
        let env = Env::new("v2-wrong-path");
        env.seed_execution("E-V2-PATH");
        let lease = env
            .service()
            .allocate_workspace(&request(&env, "E-V2-PATH"))
            .unwrap();
        let wrong = std::fs::canonicalize(&env.root)
            .unwrap()
            .join("ws-impossible")
            .to_string_lossy()
            .into_owned();
        Connection::open(&env.db)
            .unwrap()
            .execute(
                "UPDATE workspace_leases SET worktree_ref = ?1 WHERE workspace_id = ?2",
                params![wrong, lease.id().as_str()],
            )
            .unwrap();
        expect_mismatch(
            env.service()
                .ensure_workspace_ready(&request(&env, "E-V2-PATH")),
        );
    }

    // Current-v2 path + wrong branch.
    {
        let env = Env::new("v2-wrong-branch");
        env.seed_execution("E-V2-BRANCH");
        let lease = env
            .service()
            .allocate_workspace(&request(&env, "E-V2-BRANCH"))
            .unwrap();
        Connection::open(&env.db)
            .unwrap()
            .execute(
                "UPDATE workspace_leases SET branch_ref = 'refs/heads/agent/e2-impossible' WHERE workspace_id = ?1",
                [lease.id().as_str()],
            )
            .unwrap();
        expect_mismatch(
            env.service()
                .ensure_workspace_ready(&request(&env, "E-V2-BRANCH")),
        );
    }

    // Persisted owner changed underneath a valid current-v2 pair.
    {
        let env = Env::new("v2-owner-mismatch");
        env.seed_execution_named("P-A", "T-A", "E-V2-A");
        env.seed_execution_named("P-B", "T-B", "E-V2-B");
        let lease = env
            .service()
            .allocate_workspace(&request(&env, "E-V2-A"))
            .unwrap();
        Connection::open(&env.db)
            .unwrap()
            .execute(
                "UPDATE workspace_leases SET execution_id = 'E-V2-B' WHERE workspace_id = ?1",
                [lease.id().as_str()],
            )
            .unwrap();
        expect_mismatch(
            env.service()
                .ensure_workspace_ready(&request(&env, "E-V2-B")),
        );
    }

    // Historical-looking branch with an impossible in-root path.
    {
        let env = Env::new("legacy-impossible-path");
        env.seed_execution("e2-impossible-path");
        let workspace_id = "ws-e2-impossible-path";
        insert_task0051_lease(&env, "e2-impossible-path", workspace_id, "ALLOCATED");
        let wrong = std::fs::canonicalize(&env.root)
            .unwrap()
            .join("ws-other")
            .to_string_lossy()
            .into_owned();
        Connection::open(&env.db)
            .unwrap()
            .execute(
                "UPDATE workspace_leases SET worktree_ref = ?1 WHERE workspace_id = ?2",
                params![wrong, workspace_id],
            )
            .unwrap();
        expect_mismatch(
            env.service()
                .ensure_workspace_ready(&request(&env, "e2-impossible-path")),
        );
    }

    // Historical branch paired with another WorkspaceId's historical path.
    {
        let env = Env::new("legacy-cross-path");
        env.seed_execution("h2-cross-path");
        let workspace_id = "ws-h2-cross-path";
        insert_task0051_lease(&env, "h2-cross-path", workspace_id, "ALLOCATED");
        let foreign = std::fs::canonicalize(&env.root)
            .unwrap()
            .join("ws-ws-someone-else")
            .to_string_lossy()
            .into_owned();
        Connection::open(&env.db)
            .unwrap()
            .execute(
                "UPDATE workspace_leases SET worktree_ref = ?1 WHERE workspace_id = ?2",
                params![foreign, workspace_id],
            )
            .unwrap();
        expect_mismatch(
            env.service()
                .ensure_workspace_ready(&request(&env, "h2-cross-path")),
        );
    }
}

#[cfg(unix)]
#[test]
fn legacy_symlink_escape_is_rejected_before_cleanup_mutation() {
    use std::os::unix::fs::symlink;

    let env = Env::new("legacy-symlink-escape");
    env.seed_execution("e2-symlink");
    let workspace_id = "ws-e2-symlink";
    let (_branch, worktree) = insert_task0051_lease(&env, "e2-symlink", workspace_id, "READY");
    let outside = env.dir.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, &worktree).unwrap();

    let id = workspace::WorkspaceId::new(workspace_id).unwrap();
    let error = env
        .service()
        .cleanup_workspace(&id, &env.repo_path())
        .unwrap_err();
    assert!(matches!(
        error,
        workspace::WorkspaceFailure::OwnershipMismatch { .. }
    ));
    assert!(
        outside.is_dir(),
        "cleanup must not touch the symlink target"
    );
    assert!(Path::new(&worktree).is_symlink());
}
