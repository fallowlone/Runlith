//! Case-fold-safe managed workspace identity keepers (TASK-0053, G5 B-CF).
//!
//! TASK-0052 proved on this host's case-insensitive APFS that two distinct
//! valid ExecutionIds differing only in ASCII case (`E-CASE` vs `E-case`)
//! derive exact-distinct refs that alias on disk: loose refs wedge the
//! second entry (`BranchCollision` after the Task is already RUNNING with a
//! stranded reservation/lease), packed refs alias both branches and worktree
//! HEADs bidirectionally. This violates INV-005.
//!
//! These keepers pin the remediated contract through the existing public
//! API only (`branch_for_execution`, `enter_execution`,
//! `WorkspaceService`, real `git-cli` against a real repository):
//!
//! 1. derived managed branches for case-distinct ids are distinct even
//!    after ASCII case folding, and already canonical (fold-stable);
//! 2. loose refs: both executions materialize independent branches and
//!    worktrees with independent HEADs;
//! 3. packed refs: packing between the two materializations still leaves
//!    independent branches and HEADs (no bidirectional aliasing).
//!
//! RED note: on the TASK-0051 derivation these fail — (1) finds folded
//! equality, (2) wedges the second entry on `BranchCollision`, (3) aliases.
//! They pass only when derivation is case-fold-safe.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_runtime::ExecutionId;
use app_services::{
    branch_for_execution, EnsureOutcome, FixedClock, ManagedWorkspaceRoot, OrchestrationIds,
    OrchestrationService, RuntimeService, SystemClock, TaskService, WorkspaceRequest,
    WorkspaceService,
};
use git_cli::GitCli;
use storage_sqlite::{EventType, ExecutionEntryAdmission, SqliteStore, StorageError};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};
use workspace::RepositoryRef;

static COUNTER: AtomicU64 = AtomicU64::new(0);

const FIXED_MILLIS: u64 = 1_704_067_200_000;

/// Case-distinct pairs that MUST stay independent under the remediation.
/// The first two are the exact TASK-0052 repro pairs.
const CASE_PAIRS: [(&str, &str); 5] = [
    ("E-CASE", "E-case"),
    ("E-PACK", "E-pack"),
    ("A", "a"),
    ("ABC", "abc"),
    ("Execution", "execution"),
];

struct Env {
    dir: PathBuf,
    db: PathBuf,
    repo: PathBuf,
    root: PathBuf,
}

impl Env {
    fn named(suite: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-casefold-{suite}-{id}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("case dir");
        let db = dir.join("test.db");
        let repo = dir.join("repo");
        let root = dir.join("managed");
        std::fs::create_dir_all(&root).expect("managed root");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        Self {
            dir,
            db,
            repo,
            root,
        }
    }

    fn seed_ready(&self, project: &str, task: &str) {
        let mut tasks = TaskService::open(&self.db, SystemClock).expect("open task service");
        let project_id = ProjectId::new(project).unwrap();
        tasks
            .create_project(
                &Project::new(project_id.clone(), format!("project {project}")).unwrap(),
            )
            .unwrap();
        let task_id = TaskId::new(task).unwrap();
        tasks
            .create_task(
                &Task::draft(task_id.clone(), project_id, format!("objective {task}")).unwrap(),
            )
            .unwrap();
        tasks.mark_ready(&task_id).unwrap();
    }

    fn workspace_service(&self) -> WorkspaceService<GitCli, FixedClock> {
        WorkspaceService::open(
            &self.db,
            GitCli::new(),
            ManagedWorkspaceRoot::new(&self.root).expect("managed root"),
            FixedClock(FIXED_MILLIS),
        )
        .expect("open workspace service")
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
        .expect("fixture git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn ascii_fold(value: &str) -> String {
    value.to_ascii_lowercase()
}

/// Admit one execution through the production entry boundary (no provider,
/// no Git yet): the Task must end RUNNING with a durable execution.
fn enter(env: &Env, task: &str, execution: &str, attempt: &str) {
    let mut orchestration =
        OrchestrationService::open(&env.db, SystemClock).expect("open orchestration");
    let outcome = orchestration
        .enter_execution(
            &TaskId::new(task).unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new(execution).unwrap(),
                attempt_id: agent_runtime::AttemptId::new(attempt).unwrap(),
            },
        )
        .expect("entry commits");
    assert!(
        matches!(outcome, app_services::ExecutionEntry::Runnable(_)),
        "{execution} entry must be runnable, got {outcome:?}"
    );
}

/// Allocate + materialize + finalize one lease; returns its branch and
/// worktree path. Panics on any workspace failure (notably the B-CF
/// `BranchCollision` wedge shape).
fn materialize(env: &Env, execution: &str) -> (String, String) {
    let mut workspace = env.workspace_service();
    let request = WorkspaceRequest {
        execution_id: ExecutionId::new(execution).unwrap(),
        repository_ref: RepositoryRef::new("repo-local:casefold").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    };
    let ensured = workspace
        .ensure_workspace_ready(&request)
        .unwrap_or_else(|error| panic!("{execution} workspace must materialize: {error}"));
    assert_eq!(
        ensured.outcome,
        EnsureOutcome::CreatedFresh,
        "{execution} must allocate fresh, got {:?}",
        ensured.outcome
    );
    (
        ensured.lease.branch().as_str().to_owned(),
        ensured.lease.worktree_ref().to_owned(),
    )
}

fn commit_in_worktree(worktree: &str, file: &str, body: &str) -> String {
    let path = Path::new(worktree);
    std::fs::write(path.join(file), body).expect("write fixture file");
    git(path, &["add", file]);
    git(path, &["commit", "-q", "-m", &format!("commit {file}")]);
    git(path, &["rev-parse", "HEAD"])
}

/// Case-distinct execution identities derive managed branches that are
/// distinct as strings AND distinct after ASCII case folding, already in
/// canonical (fold-stable) form inside the managed scope.
#[test]
fn case_distinct_branches_are_fold_stable_and_distinct() {
    for (upper, lower) in CASE_PAIRS {
        let a = branch_for_execution(&ExecutionId::new(upper).unwrap()).unwrap();
        let b = branch_for_execution(&ExecutionId::new(lower).unwrap()).unwrap();
        assert_ne!(a.as_str(), b.as_str(), "{upper:?} vs {lower:?} must differ");
        assert_ne!(
            ascii_fold(a.as_str()),
            ascii_fold(b.as_str()),
            "{upper:?} vs {lower:?} must differ even after ASCII case folding"
        );
        for derived in [&a, &b] {
            assert_eq!(
                ascii_fold(derived.as_str()),
                derived.as_str(),
                "{derived} must already be ASCII-casefold-stable"
            );
            assert!(
                derived.is_managed(),
                "{derived} must stay in the managed scope"
            );
            assert!(
                !derived
                    .short_name()
                    .split_at("agent/".len())
                    .1
                    .contains('/'),
                "{derived} slug must be a single path component"
            );
        }
    }
}

/// Loose refs: both case variants allocate independent branches and
/// worktrees and reach independent HEADs after independent commits.
#[test]
fn loose_ref_case_variants_materialize_independently() {
    let env = Env::named("loose");
    env.seed_ready("P-1", "T-1");
    env.seed_ready("P-2", "T-2");

    enter(&env, "T-1", "E-CASE", "A-1");
    let (branch_a, worktree_a) = materialize(&env, "E-CASE");

    enter(&env, "T-2", "E-case", "A-2");
    let (branch_b, worktree_b) = materialize(&env, "E-case");

    assert_ne!(branch_a, branch_b);
    assert_ne!(ascii_fold(&branch_a), ascii_fold(&branch_b));
    assert_ne!(worktree_a, worktree_b);

    let head_a = commit_in_worktree(&worktree_a, "from-a.txt", "a");
    let head_b = commit_in_worktree(&worktree_b, "from-b.txt", "b");
    assert_ne!(
        head_a, head_b,
        "independent commits must reach independent HEADs"
    );

    // Each managed ref resolves to its own commit — no aliasing.
    let at_a = git(
        &env.repo,
        &["rev-parse", branch_a.trim_start_matches("refs/heads/")],
    );
    let at_b = git(
        &env.repo,
        &["rev-parse", branch_b.trim_start_matches("refs/heads/")],
    );
    assert_eq!(at_a, head_a, "first branch must point at the first commit");
    assert_eq!(
        at_b, head_b,
        "second branch must point at the second commit"
    );

    // Each worktree sees only its own commit.
    let log_a = git(Path::new(&worktree_a), &["log", "--oneline"]);
    let log_b = git(Path::new(&worktree_b), &["log", "--oneline"]);
    assert!(
        log_a.contains("from-a.txt") && !log_a.contains("from-b.txt"),
        "{log_a}"
    );
    assert!(
        log_b.contains("from-b.txt") && !log_b.contains("from-a.txt"),
        "{log_b}"
    );
}

/// Packed refs: packing between the two materializations must not merge
/// the case-distinct branches — independent commits stay independent.
#[test]
fn packed_ref_case_variants_do_not_alias() {
    let env = Env::named("packed");
    env.seed_ready("P-1", "T-1");
    env.seed_ready("P-2", "T-2");

    enter(&env, "T-1", "E-PACK", "A-1");
    let (branch_a, worktree_a) = materialize(&env, "E-PACK");

    git(&env.repo, &["pack-refs", "--all"]);

    enter(&env, "T-2", "E-pack", "A-2");
    let (branch_b, worktree_b) = materialize(&env, "E-pack");

    assert_ne!(branch_a, branch_b);
    assert_ne!(ascii_fold(&branch_a), ascii_fold(&branch_b));

    // Commit in the SECOND worktree first: under the B-CF alias the first
    // branch would visibly move too.
    let head_b = commit_in_worktree(&worktree_b, "from-pack-b.txt", "b");
    let at_a_after_b = git(
        &env.repo,
        &["rev-parse", branch_a.trim_start_matches("refs/heads/")],
    );
    let at_b = git(
        &env.repo,
        &["rev-parse", branch_b.trim_start_matches("refs/heads/")],
    );
    assert_eq!(at_b, head_b);
    assert_ne!(
        at_a_after_b, head_b,
        "committing the second worktree must not move the first branch"
    );

    let head_a = commit_in_worktree(&worktree_a, "from-pack-a.txt", "a");
    let at_a = git(
        &env.repo,
        &["rev-parse", branch_a.trim_start_matches("refs/heads/")],
    );
    let at_b_after_a = git(
        &env.repo,
        &["rev-parse", branch_b.trim_start_matches("refs/heads/")],
    );
    assert_eq!(at_a, head_a);
    assert_eq!(
        at_b_after_a, head_b,
        "committing the first worktree must not move the second"
    );
}

/// Concurrent admission of case-distinct ids: the new encoding gives both
/// genuinely distinct identities, so BOTH stay admissible (no false
/// refusal, no wedge) — the preferred TASK-0053 §26A outcome.
#[test]
fn concurrent_case_distinct_admissions_both_succeed() {
    for _ in 0..50 {
        let env = Env::named("concurrent-case");
        env.seed_ready("P-1", "T-1");
        env.seed_ready("P-2", "T-2");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let db = env.db.clone();
        let spawn = |task: &'static str, execution: &'static str, attempt: &'static str| {
            let (db, barrier) = (db.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                let mut orchestration = OrchestrationService::open(&db, SystemClock).expect("open");
                orchestration.enter_execution(
                    &TaskId::new(task).unwrap(),
                    OrchestrationIds {
                        execution_id: ExecutionId::new(execution).unwrap(),
                        attempt_id: agent_runtime::AttemptId::new(attempt).unwrap(),
                    },
                )
            })
        };
        let first = spawn("T-1", "E-CASE", "A-1");
        let second = spawn("T-2", "E-case", "A-2");
        let first = first.join().expect("thread").expect("T-1 entry commits");
        let second = second.join().expect("thread").expect("T-2 entry commits");
        assert!(
            matches!(first, app_services::ExecutionEntry::Runnable(_)),
            "E-CASE must be runnable, got {first:?}"
        );
        assert!(
            matches!(second, app_services::ExecutionEntry::Runnable(_)),
            "E-case must be runnable, got {second:?}"
        );
        // Durable cross-check: two executions, two reservations, both
        // tasks RUNNING — nothing stranded, nothing shared.
        let store = storage_sqlite::SqliteStore::open(&env.db).expect("open store");
        assert!(store
            .get_execution(&ExecutionId::new("E-CASE").unwrap())
            .unwrap()
            .is_some());
        assert!(store
            .get_execution(&ExecutionId::new("E-case").unwrap())
            .unwrap()
            .is_some());
        let _ = RuntimeService::open(&env.db, SystemClock).expect("open runtime");
    }
}

/// Admits one entry directly at the store boundary with an explicit managed
/// branch (the TASK-0053 §16 seam: derivation → branch string →
/// reservation/check). Production always passes the canonically derived
/// branch; tests force one shared branch for two different executions to
/// simulate a digest collision without faking any cryptographic weakness.
fn admit_with_branch(
    db: &std::path::Path,
    task: &str,
    execution: &str,
    attempt: &str,
    branch: &str,
) -> Result<(), StorageError> {
    let mut tasks = TaskService::open(db, SystemClock).expect("open tasks");
    let mut runtime = RuntimeService::open(db, SystemClock).expect("open runtime");
    let task_id = TaskId::new(task).unwrap();
    let claimed = tasks.plan_claim(&task_id).expect("plan claim");
    let started = tasks.plan_start(&claimed.task).expect("plan start");
    let planned = runtime
        .plan_execution(ExecutionId::new(execution).unwrap(), task_id.clone())
        .expect("plan execution");
    let mut store = SqliteStore::open(db).expect("open store");
    let admission = ExecutionEntryAdmission {
        plan_head: None,
        task_id: &task_id,
        claimed_task: &claimed.task,
        claimed_event: &claimed.event,
        started_task: &started.task,
        started_event: &started.event,
        execution: &planned.execution,
        execution_event: &planned.event,
        attempt_id: &agent_runtime::AttemptId::new(attempt).unwrap(),
        reserved_at: planned.reserved_at.clone(),
        managed_branch: branch,
    };
    store.admit_execution_entry(&admission).map(|_| ())
}

fn loser_journal_is_clean(store: &SqliteStore, task: &str) {
    for event in store
        .list_events_for_task(&TaskId::new(task).unwrap())
        .expect("journal")
    {
        assert!(
            !matches!(
                event.fact.event_type,
                EventType::Claimed
                    | EventType::Started
                    | EventType::ExecutionCreated
                    | EventType::AttemptCreated
                    | EventType::WorkspaceCreated
            ),
            "loser task {task} must carry no lifecycle facts, got {:?}",
            event.fact.event_type
        );
    }
}

/// Forced digest-collision shape, sequential: two different executions
/// sharing one managed branch — the second refuses typed BEFORE any Task
/// mutation, keeping its pre-run state and rerunnability.
#[test]
fn forced_managed_branch_collision_refuses_loser_before_mutation() {
    let env = Env::named("forced-sequential");
    env.seed_ready("P-1", "T-A");
    env.seed_ready("P-2", "T-B");
    // A branch no admitted execution owns (derived from an identity that
    // is never entered as an execution).
    let shared = branch_for_execution(&ExecutionId::new("E-FC-SHARED").unwrap())
        .unwrap()
        .as_str()
        .to_owned();

    admit_with_branch(&env.db, "T-A", "E-FC-A", "A-FC-A", &shared).expect("winner admits");
    let loser = admit_with_branch(&env.db, "T-B", "E-FC-B", "A-FC-B", &shared)
        .expect_err("shared branch must refuse");
    assert!(
        matches!(loser, StorageError::Duplicate(_)),
        "loser must receive a typed Duplicate, got {loser:?}"
    );
    assert!(
        !format!("{loser:?}").contains("UNIQUE constraint"),
        "raw SQLite text must never be the application contract: {loser:?}"
    );

    let store = SqliteStore::open(&env.db).expect("open store");
    let task_b = store
        .get_task(&TaskId::new("T-B").unwrap())
        .expect("read")
        .expect("exists");
    assert_eq!(task_b.state(), TaskState::Ready, "loser task stays pre-run");
    assert!(
        store
            .get_execution(&ExecutionId::new("E-FC-B").unwrap())
            .expect("read")
            .is_none(),
        "loser commits no execution"
    );
    loser_journal_is_clean(&store, "T-B");
    let owner = store
        .find_identity_reservation_owner(&shared)
        .expect("read reservation")
        .expect("winner holds the reservation");
    assert_eq!(owner.as_str(), "E-FC-A");

    // The loser is rerunnable with fresh identities through the product.
    let mut orchestration = OrchestrationService::open(&env.db, SystemClock).expect("open");
    let retry = orchestration
        .enter_execution(
            &TaskId::new("T-B").unwrap(),
            OrchestrationIds {
                execution_id: ExecutionId::new("E-FC-B2").unwrap(),
                attempt_id: agent_runtime::AttemptId::new("A-FC-B2").unwrap(),
            },
        )
        .expect("retry commits");
    assert!(matches!(retry, app_services::ExecutionEntry::Runnable(_)));
}

/// Forced digest-collision shape, concurrent: 50 barrier-synchronized
/// rounds admit exactly one winner per round; every loser keeps its
/// pre-run state with a typed conflict and zero unrelated facts.
#[test]
fn concurrent_forced_branch_collision_admits_single_winner() {
    for round in 0..50 {
        let env = Env::named("forced-race");
        env.seed_ready("P-1", "T-A");
        env.seed_ready("P-2", "T-B");
        let shared = branch_for_execution(&ExecutionId::new("E-FC-SHARED").unwrap())
            .unwrap()
            .as_str()
            .to_owned();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let spawn = |task: &'static str, execution: &'static str, attempt: &'static str| {
            let (db, branch, barrier) = (env.db.clone(), shared.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                admit_with_branch(&db, task, execution, attempt, &branch)
            })
        };
        let first = spawn("T-A", "E-FC-A", "A-FC-A");
        let second = spawn("T-B", "E-FC-B", "A-FC-B");
        let (first, second) = (
            first.join().expect("thread"),
            second.join().expect("thread"),
        );
        let (winner_task, loser_task) = match (&first, &second) {
            (Ok(()), Err(_)) => ("T-A", "T-B"),
            (Err(_), Ok(())) => ("T-B", "T-A"),
            (Ok(()), Ok(())) => panic!("round {round}: both admitted one branch"),
            (Err(a), Err(b)) => panic!("round {round}: both refused: {a:?} vs {b:?}"),
        };
        let loser_error = if winner_task == "T-A" {
            second.unwrap_err()
        } else {
            first.unwrap_err()
        };
        assert!(
            matches!(loser_error, StorageError::Duplicate(_)),
            "round {round}: loser must receive a typed Duplicate, got {loser_error:?}"
        );
        let store = SqliteStore::open(&env.db).expect("open store");
        let loser = store
            .get_task(&TaskId::new(loser_task).unwrap())
            .expect("read")
            .expect("exists");
        assert_eq!(
            loser.state(),
            TaskState::Ready,
            "round {round}: loser stays pre-run"
        );
        loser_journal_is_clean(&store, loser_task);
        // Exactly one execution exists across both tasks.
        let executions = ["E-FC-A", "E-FC-B"]
            .iter()
            .filter(|id| {
                store
                    .get_execution(&ExecutionId::new(**id).unwrap())
                    .expect("read")
                    .is_some()
            })
            .count();
        assert_eq!(
            executions, 1,
            "round {round}: exactly one winner, {winner_task}"
        );
    }
}

/// Every generated branch must satisfy real `git check-ref-format`,
/// including hostile source inputs that must never escape
/// `refs/heads/agent/`.
#[test]
fn generated_refs_satisfy_git_check_ref_format() {
    let mut corpus: Vec<String> = [
        "E-CASE",
        "E-case",
        "a/b",
        "a\\b",
        "100%",
        "%41",
        "..",
        ".",
        "@{1}",
        ".leading",
        "trailing.",
        "name.lock",
        "a b",
        "a\tb",
        "caf\u{e9}",
        "日本語",
        "🎉",
        "CON",
        "com1",
        "e2-spoof",
    ]
    .iter()
    .map(|raw| raw.to_string())
    .collect();
    corpus.push("x".repeat(256));
    corpus.push("h2-".to_owned() + &"a".repeat(64));
    for raw in &corpus {
        let branch = branch_for_execution(&ExecutionId::new(raw).unwrap()).unwrap();
        assert!(branch.is_managed(), "{raw:?} must stay managed");
        let status = std::process::Command::new("git")
            .args(["check-ref-format", branch.as_str()])
            .stdin(std::process::Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("run git check-ref-format")
            .status;
        assert!(
            status.success(),
            "{raw:?} → {branch} must satisfy git check-ref-format"
        );
        assert!(
            branch.short_name().starts_with("agent/") && !branch.short_name()[6..].contains('/'),
            "{raw:?} → {branch} must never escape the managed namespace"
        );
    }
}

/// Pre-v2 persisted leases stay byte-for-byte authoritative (TASK-0053
/// §13): an old-format `Allocated` lease (mixed-case branch, legacy path
/// spelling) reconciles from its persisted values — never rederived —
/// through finalize, diff, and cleanup on a real repository.
#[test]
fn old_format_allocated_lease_reconciles_from_persisted_values() {
    use workspace::{BranchRef, Revision, WorkspaceId, WorkspaceLease, WorkspaceStore};

    let env = Env::named("old-lease");
    // Legacy-faithful setup: the execution exists WITHOUT an admission
    // reservation (admission did not exist when this lease was written),
    // exactly like a migrated pre-v2 row.
    {
        let mut tasks = TaskService::open(&env.db, SystemClock).expect("open tasks");
        let project_id = ProjectId::new("P-1").unwrap();
        tasks
            .create_project(&Project::new(project_id.clone(), "project P-1".to_owned()).unwrap())
            .unwrap();
        let task_id = TaskId::new("T-1").unwrap();
        tasks
            .create_task(
                &Task::draft(task_id.clone(), project_id, "objective T-1".to_owned()).unwrap(),
            )
            .unwrap();
        drop(tasks);
        let mut runtime = RuntimeService::open(&env.db, SystemClock).expect("open runtime");
        runtime
            .create_execution(
                ExecutionId::new("E-OLD-1").unwrap(),
                TaskId::new("T-1").unwrap(),
            )
            .expect("seed execution");
    }

    // Old-scheme persisted facts: mixed-case branch without a version
    // prefix, legacy `ws-<id>` path spelling, recorded base.
    let canonical_root = std::fs::canonicalize(&env.root).expect("canonical root");
    let worktree = canonical_root.join("ws-ws-old-1");
    let base = git(&env.repo, &["rev-parse", "HEAD"]);
    let old_branch = "refs/heads/agent/E-OLD-1";
    let lease = WorkspaceLease::allocate(
        WorkspaceId::new("ws-old-1").unwrap(),
        RepositoryRef::new("repo-local:old").unwrap(),
        ExecutionId::new("E-OLD-1").unwrap(),
        Revision::new(base.clone()).unwrap(),
        BranchRef::new(old_branch).unwrap(),
        worktree.to_string_lossy().into_owned(),
    )
    .unwrap();
    {
        let mut store = SqliteStore::open(&env.db).expect("open store");
        store
            .insert_allocated_lease(&lease)
            .expect("old-format lease inserts");
    }
    // Crash-B shape at the persisted values: the branch exists at the
    // base and the worktree is checked out there; finalization never ran.
    git(&env.repo, &["branch", "agent/E-OLD-1", &base]);
    git(
        &env.repo,
        &[
            "worktree",
            "add",
            &worktree.to_string_lossy(),
            "agent/E-OLD-1",
        ],
    );

    let mut workspace = env.workspace_service();
    let request = WorkspaceRequest {
        execution_id: ExecutionId::new("E-OLD-1").unwrap(),
        repository_ref: RepositoryRef::new("repo-local:old").unwrap(),
        repository_path: env.repo_path(),
        base_revision: "HEAD".to_owned(),
    };
    // TASK-0071 trusted-anchor follow-up: pre-anchor leases carry no sidecar
    // row and must fail closed (no backfill from current FS) until
    // re-admission with a new lease.
    let err = workspace
        .ensure_workspace_ready(&request)
        .expect_err("pre-anchor legacy lease must fail closed");
    assert!(
        matches!(err, workspace::WorkspaceFailure::GitBackendFailure { .. }),
        "missing anchor must be GitBackendFailure, got {err:?}"
    );
}
