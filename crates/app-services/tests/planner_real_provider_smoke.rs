//! TASK-0106 real-provider planner smoke (G7 acceptance).
//!
//! The G7 capability is a planner proposal that becomes a durable plan
//! revision. Every existing test proves that path against a deterministic
//! `#!/bin/sh` fixture standing in for the provider: the adapter contract
//! suite in `adapters/provider-claude/tests/planner_port_contract.rs` and the
//! `claude_fixture` harness in `planning_service.rs` both substitute the
//! executable. The real `claude` CLI has therefore never once been asked for
//! a proposal, and the path real CLI -> provider parsing -> `PlanChangeSet`
//! -> durable revision has never executed end to end.
//!
//! This test closes that gap with one bounded real call.
//!
//! Separation (TASK-0046 §27, as in `provider-claude/tests/smoke.rs`): the
//! test is `#[ignore]`d AND requires `RUNLITH_CLAUDE_SMOKE=1`, so
//! deterministic CI (no CLI, no auth, no network, no quota) never runs it and
//! never fails on its absence.
//!
//! Scope limit, stated rather than implied: like the G5 executor smoke this
//! binds a trusted host planner, so it proves the provider and planning path,
//! not the Runner/sandbox launch. The confined-child path is the G6 smoke in
//! `bins/agentd/src/execution.rs` and remains blocked on TASK-0102.
//!
//! What may and may not make this test pass, because two real-provider smokes
//! previously stayed green across two gate closures without the provider call
//! ever succeeding (TASK-0101):
//!
//! - A missing CLI, an unavailable CLI or an unauthenticated CLI returns
//!   early. Those are absences of an environment, not results.
//! - A genuine environment failure (quota, rate limit, network, timeout)
//!   passes through, named explicitly.
//! - Everything else fails, including a well-formed proposal the validator
//!   refuses and a per-call budget the real CLI's reported usage cannot fit.
//!   Those are findings about the prompt or the bound, not noise to tolerate.

#![cfg(unix)]

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use app_services::{
    FixedClock, PlanningPolicyContext, PlanningProposalOutcome, PlanningService, TaskService,
};
use plan_domain::{
    AffectedParentCriterion, CriterionId, DecompositionRequest, EvidenceRef, PlanBranchId,
    PlanningBudget, RiskClass, ScopeBound, VerificationMethod,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};
use provider_claude::{AuthenticationState, ClaudeConfig, ClaudePlanner, ProviderAvailability};
use storage_sqlite::SqliteStore;
use task_domain::{Project, ProjectId, Task, TaskId};

const FIXED_MILLIS: u64 = 1_704_067_200_000;
const PARENT_TASK: &str = "TASK-0106";
const SCOPE_ROOT: &str = "repo:src";
const PARENT_CRITERION: &str = "criterion-root";

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-planner-smoke-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        let db = Self {
            path: dir.join("runlith.db"),
            dir,
        };
        db.seed_parent();
        db
    }

    fn service(&self) -> PlanningService<FixedClock> {
        PlanningService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("planning service")
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("store")
    }

    fn seed_parent(&self) {
        let mut tasks = TaskService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("tasks");
        let project_id = ProjectId::new(format!(
            "P-PLANNER-SMOKE-{}",
            TEST_COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
        .expect("project id");
        tasks
            .create_project(
                &Project::new(project_id.clone(), "planner smoke fixture".to_owned())
                    .expect("project"),
            )
            .expect("create project");
        let parent = Task::draft(
            TaskId::new(PARENT_TASK).expect("parent task id"),
            project_id,
            "planner smoke root".to_owned(),
        )
        .expect("draft parent");
        tasks.create_task(&parent).expect("create parent");
        tasks.mark_ready(parent.id()).expect("mark parent ready");
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn branch() -> PlanBranchId {
    PlanBranchId::new("task-0106-planner-smoke").expect("branch")
}

fn evidence() -> EvidenceRef {
    EvidenceRef::new("evidence-planner-smoke").expect("evidence")
}

/// A deliberately trivial policy: one low-risk parent criterion inside one
/// scope root. A live planner that cannot produce an in-bounds proposal for
/// this is reporting a real defect in `proposal_prompt`, not bad luck.
fn policy() -> PlanningPolicyContext {
    PlanningPolicyContext {
        scope_bound: ScopeBound::new(vec![SCOPE_ROOT.to_owned()]).expect("scope bound"),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: vec![AffectedParentCriterion {
            id: CriterionId::new(PARENT_CRITERION).expect("criterion id"),
            method: VerificationMethod::AutomatedTest,
        }],
        parent_task_ref: PARENT_TASK.to_owned(),
        canonical_task_authority: false,
        lineage_fingerprints: vec![],
        risk_grant: None,
        risk_decision_id: None,
        policy_fingerprint: "policy:task-0106-smoke".to_owned(),
    }
}

fn request() -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: PARENT_TASK.to_owned(),
        base_plan_revision: 0,
        acceptance_criteria_refs: vec![CriterionId::new(PARENT_CRITERION).expect("criterion id")],
        scope_bound: ScopeBound::new(vec![SCOPE_ROOT.to_owned()]).expect("scope bound"),
        evidence_refs: vec![evidence()],
        depth_remaining: 4,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    }
}

fn smoke_enabled() -> bool {
    std::env::var("RUNLITH_CLAUDE_SMOKE").is_ok_and(|value| value == "1")
}

/// Environment facts that are properties of the host and the account, not of
/// the integration under test. Kept identical in spirit to the G5 smoke so
/// the two agree on what an environment failure is.
const ENVIRONMENT_MARKERS: &[&str] = &[
    "rate limit",
    "rate_limit",
    "usage limit",
    "quota",
    "overloaded",
    "network",
    "enotfound",
    "econnrefused",
    "timed out",
    "timeout",
];

#[test]
#[ignore = "real provider: needs RUNLITH_CLAUDE_SMOKE=1 plus an authenticated claude CLI"]
fn real_claude_planner_proposal_becomes_a_durable_plan_revision() {
    if !smoke_enabled() {
        eprintln!("smoke skipped: set RUNLITH_CLAUDE_SMOKE=1 to run against the real CLI");
        return;
    }
    let executable = OsStr::new("claude");
    let detection = provider_claude::detect(executable);
    if detection.status != ProviderAvailability::Available {
        eprintln!(
            "smoke skipped: claude CLI not available ({:?}: {})",
            detection.status, detection.detail
        );
        return;
    }
    let authentication = provider_claude::probe_authentication(executable);
    if authentication.state != AuthenticationState::Authenticated {
        eprintln!(
            "smoke skipped: claude CLI not authenticated ({})",
            authentication.state.as_str()
        );
        return;
    }
    let capabilities = provider_claude::probe_capabilities(executable, detection.version);
    assert!(capabilities.non_interactive, "smoke needs print mode");
    assert!(capabilities.structured_json, "smoke needs json output");

    let db = TestDb::new("proposal");
    let workdir = db.dir.join("planner-work");
    std::fs::create_dir_all(&workdir).expect("planner workdir");
    let planner = ClaudePlanner::bind_trusted_host(
        ClaudeConfig::default(),
        workdir,
        db.dir.join("provider-env"),
    )
    .expect("bind real claude planner");

    let mut service = db.service();
    let started = std::time::Instant::now();
    let outcome = service.propose_decomposition(&planner, &branch(), request(), &policy());
    eprintln!("smoke planner call concluded in {:?}", started.elapsed());

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            // A tolerant default previously let a stale model pin and a
            // missing login principal keep two smokes green while the real
            // call never succeeded. Only host/account facts pass through.
            let detail = format!("{error:?}");
            eprintln!("smoke planner error: {detail}");
            // The exact `typed_cause` prefix produced by
            // `malformed_after_spend`. Matching the real string matters: an
            // approximate marker falls through to the environment check and
            // reports contract drift as "a non-environment reason".
            assert!(
                !detail.contains("malformed planner output"),
                "real CLI output does not parse as a proposal; \
                 this is prompt or CLI contract drift, not an environment fact: {detail}"
            );
            let haystack = detail.to_ascii_lowercase();
            assert!(
                ENVIRONMENT_MARKERS
                    .iter()
                    .any(|marker| haystack.contains(marker)),
                "real planner failed for a non-environment reason: {detail}"
            );
            eprintln!("smoke passed through an environment failure without a proposal");
            return;
        }
    };

    let accepted = match outcome {
        PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!(
            "the real CLI must produce a proposal this trivially-bounded request accepts; \
             got {other:?}"
        ),
    };

    // The revision is durable, not just returned.
    assert_eq!(accepted.revision().base_revision, 0);
    assert_eq!(accepted.revision().revision_seq, 1);
    let store = db.store();
    assert_eq!(
        store.get_plan_head(&branch()).expect("plan head"),
        Some(1),
        "the accepted proposal must advance the durable plan head"
    );
    assert_eq!(
        store.get_plan_graph(&branch(), 1).expect("plan graph"),
        Some(accepted.graph().clone()),
        "the persisted graph must reconstruct the accepted capability"
    );
    assert_eq!(
        service.current_accepted_plan(&branch()).expect("head read"),
        Some(*accepted.clone()),
        "reopening the durable head must reproduce the accepted revision"
    );

    // The receipt proves a real call happened. A skipped or fabricated path
    // cannot produce provider-reported usage.
    let receipt = accepted.planning_cost_receipt();
    eprintln!(
        "smoke measured spend: input={} output={} wall_ms={} class={}",
        receipt.input_tokens, receipt.output_tokens, receipt.wall_time_ms, receipt.price_class
    );
    assert_eq!(
        receipt.price_class, "provider-reported",
        "the receipt must carry measured provider usage, never an estimate"
    );
    assert!(
        receipt.input_tokens > 0 && receipt.output_tokens > 0,
        "a real provider call reports non-zero usage; got {receipt:?}"
    );
    assert_eq!(
        store
            .get_plan_cost_receipt(&accepted.revision().proposed_by.planning_cost_receipt_ref)
            .expect("receipt read"),
        Some(receipt.clone()),
        "the accepted revision must reference its durable call receipt"
    );

    let graph = accepted.graph();
    eprintln!("smoke accepted {} plan node(s)", graph.nodes.len());
    assert!(
        !graph.nodes.is_empty(),
        "an accepted decomposition must add at least one plan node"
    );
}
