//! TASK-0078 production-composition integration proof without external LLM
//! access. The daemon-owned database path composes PlanningService,
//! PlannerPort, validation/persistence, scheduler execution, independent
//! evaluation, and one bounded replan using deterministic adapters.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_runtime::{AttemptId, ExecutionId, ProviderRef};
use agentd::{execution::ExecutionComposition, DaemonConfig};
use app_services::{
    FixedClock, OrchestrationIds, OrchestrationOutcome, OrchestrationService,
    PlannedOrchestrationError, PlannedOrchestrationOutcome, PlanningPolicyContext, PlanningService,
    ServiceError, TaskService,
};
use evaluation::{
    subject_fingerprint, ClaimVerdict, ClaimedOutcome, ClaimedSubject, EffectKind, EffectPayload,
    EffectProvenance, EffectSubjectRef, EvidenceRefs, Sha256Digest,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use fake_planner::{FakePlanner, FakePlannerScenario};
use plan_domain::{
    AffectedParentCriterion, CriterionId, DecompositionRequest, EvidenceRef, PlanBranchId,
    PlannerCallSpend, PlannerFault, PlanningBudget, RiskClass, ScopeBound, VerificationMethod,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};
use policy::CapabilityKind;
use scheduler::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};
use task_domain::{DomainError, Project, ProjectId, Task, TaskId, TaskState};

const FIXED_MILLIS: u64 = 1_704_067_200_000;
static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new() -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-agentd-planning-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
        }
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(unix)]
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

struct DeterministicExecutor;

impl ExecutionPort for DeterministicExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("agentd-planning-fixture").expect("provider ref")
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        Ok(WorkEvidence {
            summary: format!("fixture executed {}", work.task_id.as_str()),
            evidence_ref: format!("fixture:{}", work.attempt_id.as_str()),
            partial: false,
        })
    }
}

struct CountingExecutor {
    calls: AtomicUsize,
}

impl CountingExecutor {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ExecutionPort for CountingExecutor {
    fn provider_ref(&self) -> ProviderRef {
        ProviderRef::new("agentd-planning-counting-fixture").expect("provider ref")
    }

    fn execute(&self, work: &WorkItem) -> Result<WorkEvidence, ExecutionProblem> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(WorkEvidence {
            summary: format!("fixture executed {}", work.task_id.as_str()),
            evidence_ref: format!("fixture:{}", work.attempt_id.as_str()),
            partial: false,
        })
    }
}

/// Creates one canonical planning parent and returns the ids of the children
/// the planner proposes for it. Children are never pre-seeded: an accepted
/// revision materializes them as new READY Tasks under the parent (TASK-0087).
fn seed_planning_parent(
    orchestration: &mut OrchestrationService<FixedClock>,
    project: &str,
    parent: &str,
    planned: &[&str],
) -> Vec<TaskId> {
    let project_id = ProjectId::new(project).expect("project id");
    orchestration
        .tasks_mut()
        .create_project(&Project::new(project_id.clone(), "planning fixture".to_owned()).unwrap())
        .expect("create project");
    let parent_id = TaskId::new(parent).expect("parent task id");
    let parent_task = Task::draft(
        parent_id.clone(),
        project_id.clone(),
        format!("planning parent {parent}"),
    )
    .expect("draft planning parent");
    orchestration
        .tasks_mut()
        .create_task(&parent_task)
        .expect("create planning parent");
    // Publication admits a decomposition only while the parent is doing work
    // (ADR-0027 TASK-0091 amendment), so the fixture declares the parent
    // eligible through the lifecycle service, exactly as the real flow must.
    orchestration
        .tasks_mut()
        .mark_ready(&parent_id)
        .expect("mark planning parent ready");

    planned
        .iter()
        .map(|name| TaskId::new(*name).expect("planned child id"))
        .collect()
}

fn ids(suffix: &str) -> OrchestrationIds {
    OrchestrationIds {
        execution_id: ExecutionId::new(format!("E-{suffix}")).expect("execution id"),
        attempt_id: AttemptId::new(format!("A-{suffix}")).expect("attempt id"),
    }
}

fn policy(parent: &str, fingerprint: &str) -> PlanningPolicyContext {
    PlanningPolicyContext {
        scope_bound: ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope"),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: vec![AffectedParentCriterion {
            id: CriterionId::new("criterion-root").expect("criterion"),
            method: VerificationMethod::AutomatedTest,
            requirement: None,
        }],
        parent_task_ref: parent.to_owned(),
        canonical_task_authority: false,
        lineage_fingerprints: vec![],
        risk_grant: None,
        risk_decision_id: None,
        policy_fingerprint: fingerprint.to_owned(),
    }
}

fn request(evidence: &str) -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: "caller-value-is-overwritten".to_owned(),
        base_plan_revision: u64::MAX,
        acceptance_criteria_refs: vec![],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).expect("caller scope"),
        evidence_refs: vec![EvidenceRef::new(evidence).expect("evidence")],
        depth_remaining: 4,
        baseline_risk: RiskClass::Low,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    }
}

#[test]
fn agentd_composes_planning_validation_persistence_execution_and_bounded_replan() {
    let db = TestDb::new();
    let config = DaemonConfig::with_execution(&db.path);
    let database_path = config
        .database_path
        .as_ref()
        .expect("database-backed agentd config");

    let mut orchestration =
        OrchestrationService::open(database_path, FixedClock(FIXED_MILLIS)).expect("orchestration");
    let mut planning =
        PlanningService::open(database_path, FixedClock(FIXED_MILLIS)).expect("planning");
    // The bounded replan proposes `replanned-child` as replacement work; its
    // acceptance materializes it READY, so ADR-0027 L4 still sees runnable
    // canonical work, not merely a structurally valid node.
    let tasks = seed_planning_parent(
        &mut orchestration,
        "P-PLANNING",
        "TASK-0078",
        &["planned-child-0", "replanned-child"],
    );
    let task_id = tasks[0].clone();
    let branch = PlanBranchId::new("task-0078-agentd").expect("branch");
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysRejected);
    let executor = DeterministicExecutor;

    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &planner,
            &branch,
            request("fixture:initial"),
            policy("TASK-0078", "policy:agentd-planning-test"),
            &task_id,
            ids("PLANNING"),
            &executor,
            &evaluator,
            None,
        )
        .expect("bounded planning + execution path");

    match outcome {
        PlannedOrchestrationOutcome::Executed {
            plan,
            execution,
            replan: Some(Ok(replan)),
        } => {
            assert_eq!(plan.revision().revision_seq, 1);
            assert!(matches!(
                execution,
                OrchestrationOutcome::ExecutionSucceededButRejected { .. }
            ));
            let replanned = match replan {
                app_services::PlanningProposalOutcome::Accepted(replanned) => replanned,
                other => panic!("expected accepted bounded replan, got {other:?}"),
            };
            assert_eq!(replanned.revision().revision_seq, 2);
        }
        other => panic!("expected executed path with bounded replan, got {other:?}"),
    }

    assert_eq!(
        planner.calls(),
        vec!["propose_decomposition", "propose_replan"]
    );
    assert_eq!(planning.store().get_plan_head(&branch).unwrap(), Some(2));
    let task = orchestration
        .tasks()
        .get_task(&task_id)
        .expect("read task")
        .expect("task exists");
    assert_eq!(task.state(), TaskState::RetryReady);
}

#[test]
fn plan_dependency_direction_runs_predecessor_before_successor() {
    let db = TestDb::new();
    let mut orchestration =
        OrchestrationService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("orchestration");
    let mut planning = PlanningService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("planning");
    let tasks = seed_planning_parent(
        &mut orchestration,
        "P-PLANNING-CHAIN",
        "TASK-0078-CHAIN",
        &["step-a", "step-b"],
    );
    let (first, second) = (tasks[0].clone(), tasks[1].clone());
    let branch = PlanBranchId::new("task-0078-plan-chain").expect("branch");
    let planner = FakePlanner::for_scenario(FakePlannerScenario::DependencyChain);
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &planner,
            &branch,
            request("fixture:chain"),
            policy("TASK-0078-CHAIN", "policy:agentd-plan-chain"),
            &second,
            ids("PLANNING-CHAIN"),
            &executor,
            &evaluator,
            None,
        )
        .expect("dependent task is held by plan readiness");

    match outcome {
        PlannedOrchestrationOutcome::WaitingOnPlanDependency { plan, blocking } => {
            assert_eq!(blocking, vec![first.clone()]);
            assert_eq!(
                plan.predecessor_task_ids(&first)
                    .expect("first plan node")
                    .expect("first represented"),
                Vec::<TaskId>::new()
            );
            assert_eq!(
                plan.predecessor_task_ids(&second)
                    .expect("second plan node")
                    .expect("second represented"),
                vec![first]
            );
        }
        other => panic!("expected plan dependency wait, got {other:?}"),
    }
    assert_eq!(executor.calls(), 0, "successor must not execute early");
    let successor = orchestration
        .tasks()
        .get_task(&second)
        .expect("read successor")
        .expect("successor exists");
    assert_eq!(successor.state(), TaskState::Ready);
}

/// TASK-0125 fixture: admits completion through the trusted gate with
/// durable, subject-bound evidence — one captured observed effect for the
/// concluded attempt, one claim plus a current VERIFIED verdict per
/// criterion, then the admissible transition under the declaring policy
/// fingerprint. Stands in for the capture/evaluation pipeline the product
/// still has to wire (TASK-0125 follow-up).
fn complete_with_captured_evidence(
    tasks: &mut TaskService<FixedClock>,
    task_id: &TaskId,
    execution_id: &ExecutionId,
    attempt_id: &AttemptId,
    policy_fingerprint: &str,
) {
    let task = tasks
        .store()
        .get_task(task_id)
        .expect("task")
        .expect("task");
    let digest = format!("sha256:{}", "a".repeat(64));
    let subject_ref =
        EffectSubjectRef::new(format!("attempt:{}", attempt_id.as_str())).expect("ref");
    let fingerprint = subject_fingerprint(
        EffectKind::ArtifactHash,
        subject_ref.as_str(),
        &[("content", digest.as_str())],
    )
    .expect("fingerprint");
    let effect = tasks
        .store_mut()
        .append_observed_effect(
            subject_ref,
            fingerprint.clone(),
            EffectPayload::ArtifactHash {
                bytes: 3,
                sha256: Sha256Digest::new(digest).expect("digest"),
            },
            "2024-01-01T00:00:00Z",
            EffectProvenance::new(
                execution_id.clone(),
                attempt_id.clone(),
                "runlith.local-observer.v1",
            )
            .expect("provenance"),
        )
        .expect("captured effect");
    for requirement in task.evidence_requirements().to_vec() {
        let claim = tasks
            .store_mut()
            .append_agent_claim(
                task_id,
                &requirement.criterion_id,
                ClaimedOutcome::Satisfied,
                ClaimedSubject {
                    subject_ref: format!("subject:{}", requirement.criterion_id),
                    subject_fingerprint: fingerprint.clone(),
                },
                EvidenceRefs::default(),
                "agent:fixture-builder",
                "2024-01-01T00:00:00Z",
            )
            .expect("claim");
        tasks
            .store_mut()
            .append_claim_verification(
                &claim.claim_id,
                requirement.requirement_kind,
                fingerprint.clone(),
                evaluation::requirement_revision(&requirement),
                requirement.policy_fingerprint.clone(),
                ClaimVerdict::Verified,
                EvidenceRefs::new(vec![], vec![effect.effect_id().as_str().to_owned()], vec![])
                    .expect("refs"),
                None,
                "2024-01-01T00:00:00Z",
            )
            .expect("verdict");
    }
    tasks
        .verification_succeeded_under_policy(
            task_id,
            "verified with durable captured evidence",
            Some(policy_fingerprint),
        )
        .expect("evidence-backed completion is admitted");
}

/// TASK-0125: a completion attempt refused by the admission gate — the
/// task stays VERIFYING with typed per-criterion outcomes.
fn assert_completion_admission_refusal(error: PlannedOrchestrationError) {
    match error {
        PlannedOrchestrationError::Orchestration(ServiceError::Domain(
            DomainError::InvalidTransition(reason),
        )) => {
            assert!(
                reason.contains("completion admission refused"),
                "expected a completion-admission refusal, got {reason:?}"
            );
            assert!(reason.contains("UNVERIFIED"), "got {reason:?}");
        }
        other => panic!("expected a completion-admission refusal, got {other:?}"),
    }
}

/// R2-02: an accepted multi-step plan is executed step by step against its
/// durable head. Advancing to the next node never calls the planner and
/// never creates a Plan revision.
#[test]
fn accepted_plan_chain_runs_successor_after_predecessor_without_replanning() {
    let db = TestDb::new();
    let mut orchestration =
        OrchestrationService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("orchestration");
    let mut planning = PlanningService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("planning");
    let tasks = seed_planning_parent(
        &mut orchestration,
        "P-PLAN-E2E",
        "TASK-0078-E2E",
        &["step-a", "step-b"],
    );
    let (step_a, step_b) = (tasks[0].clone(), tasks[1].clone());
    let branch = PlanBranchId::new("task-0078-accepted-chain").expect("branch");
    let planner = FakePlanner::for_scenario(FakePlannerScenario::DependencyChain);
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);

    // 1. The planner creates and persists revision 1 (step-a -> step-b).
    let planned = orchestration
        .schedule_with_planning(
            &mut planning,
            &planner,
            &branch,
            request("fixture:e2e"),
            policy("TASK-0078-E2E", "policy:agentd-plan-e2e"),
            &step_b,
            ids("E2E-PLAN"),
            &executor,
            &evaluator,
            None,
        )
        .expect("plan accepted");
    assert!(matches!(
        planned,
        PlannedOrchestrationOutcome::WaitingOnPlanDependency { .. }
    ));
    assert_eq!(planner.call_count(), 1);
    assert_eq!(planning.store().get_plan_head(&branch).unwrap(), Some(1));

    // 2. step-b cannot run before step-a through the accepted-plan path.
    let early = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch,
            &step_b,
            ids("E2E-B-EARLY"),
            &executor,
            &evaluator,
            None,
        )
        .expect("plan gate outcome");
    match early {
        PlannedOrchestrationOutcome::WaitingOnPlanDependency { blocking, .. } => {
            assert_eq!(blocking, vec![step_a.clone()]);
        }
        other => panic!("step-b must wait on step-a, got {other:?}"),
    }
    assert_eq!(executor.calls(), 0);

    // 3 + 4. step-a executes; TASK-0125 refuses to complete it on the
    // evaluator verdict alone, and the trusted boundary admits it only
    // after durable, subject-bound captured evidence exists.
    let refused_a = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch,
            &step_a,
            ids("E2E-A"),
            &executor,
            &evaluator,
            None,
        )
        .expect_err("a claim without durable captured evidence must not complete");
    assert_completion_admission_refusal(refused_a);
    let a_state = orchestration
        .tasks()
        .get_task(&step_a)
        .unwrap()
        .expect("step-a")
        .state();
    assert_eq!(a_state, TaskState::Verifying);
    complete_with_captured_evidence(
        orchestration.tasks_mut(),
        &step_a,
        &ExecutionId::new("E-E2E-A").unwrap(),
        &AttemptId::new("A-E2E-A").unwrap(),
        "policy:agentd-plan-e2e",
    );
    let a_state = orchestration
        .tasks()
        .get_task(&step_a)
        .unwrap()
        .expect("step-a")
        .state();
    assert_eq!(a_state, TaskState::Completed);

    // 5 + 6. The plan-gated path runs step-b; the same evidence-gated
    // completion applies to it.
    let refused_b = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch,
            &step_b,
            ids("E2E-B"),
            &executor,
            &evaluator,
            None,
        )
        .expect_err("a claim without durable captured evidence must not complete");
    assert_completion_admission_refusal(refused_b);
    assert_eq!(
        orchestration
            .tasks()
            .get_task(&step_b)
            .unwrap()
            .expect("step-b")
            .state(),
        TaskState::Verifying
    );
    complete_with_captured_evidence(
        orchestration.tasks_mut(),
        &step_b,
        &ExecutionId::new("E-E2E-B").unwrap(),
        &AttemptId::new("A-E2E-B").unwrap(),
        "policy:agentd-plan-e2e",
    );
    assert_eq!(
        orchestration
            .tasks()
            .get_task(&step_b)
            .unwrap()
            .expect("step-b")
            .state(),
        TaskState::Completed
    );
    assert_eq!(executor.calls(), 2);

    // 7-9. No planner call, head still 1, no revision created to run step-b.
    assert_eq!(planner.call_count(), 1);
    assert_eq!(planning.store().get_plan_head(&branch).unwrap(), Some(1));
    assert!(planning
        .store()
        .get_plan_revision(&branch, 2)
        .unwrap()
        .is_none());
    assert_eq!(planning.store().get_plan_lineage(&branch).unwrap().len(), 1);

    // 10. Canonical Task readiness stays authoritative after the plan gate.
    let again = orchestration
        .schedule_from_current_plan(
            &planning,
            &branch,
            &step_b,
            ids("E2E-B-AGAIN"),
            &executor,
            &evaluator,
            None,
        )
        .expect("scheduler outcome");
    assert!(matches!(
        again,
        PlannedOrchestrationOutcome::Executed {
            execution: OrchestrationOutcome::NotRunnable {
                state: TaskState::Completed,
                ..
            },
            ..
        }
    ));
    assert_eq!(executor.calls(), 2);
    assert_eq!(planner.call_count(), 1);
}

#[cfg(target_os = "macos")]
#[test]
fn production_claude_planner_composition_crosses_g6_runner_before_provider_call() {
    let db = TestDb::new();
    let provider = fixture(
        &db.dir,
        "planner-provider.sh",
        concat!(
            "printf '%s' '",
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,",
            "\"result\":\"{\\\"base_plan_revision\\\":0,\\\"reason\\\":\\\"secured fixture decomposition\\\",",
            "\\\"node_additions\\\":[{\\\"id\\\":\\\"secured-child\\\",\\\"scope\\\":[\\\"repo:src\\\"],",
            "\\\"criteria\\\":[{\\\"id\\\":\\\"criterion-child\\\",\\\"method\\\":\\\"automated-test\\\"}],",
            "\\\"risk_class\\\":\\\"low\\\",\\\"required_gate\\\":null}],",
            "\\\"node_updates\\\":[],\\\"node_removals\\\":[],\\\"edge_additions\\\":[],\\\"edge_removals\\\":[],",
            "\\\"expected_outcome\\\":\\\"secured child leaf\\\",\\\"risk_delta\\\":[],",
            "\\\"budget_delta\\\":{\\\"planning_cost_movement\\\":0,\\\"expected_execution_cost_movement\\\":0},",
            "\\\"scope_delta\\\":[\\\"repo:src\\\"],\\\"coverage_map\\\":{\\\"criterion-root\\\":[\\\"criterion-child\\\"]}}\",",
            "\"session_id\":\"secured-planner-session\",\"usage\":{\"input_tokens\":100,\"output_tokens\":100}}",
            "'\n",
        ),
    );
    let composition = ExecutionComposition::new(db.path.clone(), Some(provider));
    {
        // Canonical parent; acceptance materializes `secured-child` (L4).
        let mut orchestration =
            OrchestrationService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("orchestration");
        seed_planning_parent(
            &mut orchestration,
            "P-SECURED",
            "TASK-0078",
            &["secured-child"],
        );
    }
    let mut planning = composition.planning_service().expect("planning service");
    let planner = composition
        .secured_claude_planner("TASK-0078")
        .expect("secured planner composition");
    let branch = PlanBranchId::new("task-0078-secured-claude").expect("branch");

    let outcome = planning
        .propose_decomposition(
            &planner,
            &branch,
            request("fixture:secured"),
            &policy("TASK-0078", "policy:secured-claude-planner"),
        )
        .expect("secured planner proposal");
    let accepted = match outcome {
        app_services::PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("expected accepted secured planner proposal, got {other:?}"),
    };
    assert_eq!(accepted.revision().revision_seq, 1);
    assert_eq!(planning.store().get_plan_head(&branch).unwrap(), Some(1));
    let trigger = [EvidenceRef::new("fixture:secured").unwrap()];
    let guardrails = planning.guardrail_state(&branch, &trigger).unwrap();
    assert_eq!(
        (guardrails.planner_calls, guardrails.charged_input_tokens),
        (1, 100)
    );
    assert_eq!(
        planning
            .planner_call_receipt(&branch, &trigger, 1)
            .unwrap()
            .as_ref(),
        Some(accepted.planning_cost_receipt()),
        "the production planner call leaves durable measured cost evidence"
    );

    let audit = planning
        .store()
        .list_security_audit_ordered()
        .expect("security audit");
    let planner_policy = audit
        .iter()
        .find(|record| {
            record.fact.event == storage_sqlite::SecurityAuditEvent::PolicyDecision
                && record.fact.task_id.as_deref() == Some("TASK-0078")
        })
        .expect("planner policy decision");
    assert!(
        planner_policy
            .fact
            .capabilities
            .iter()
            .all(|capability| capability.kind != CapabilityKind::FilesystemWrite),
        "proposal-only planner must not receive filesystem write authority"
    );
    for event in [
        storage_sqlite::SecurityAuditEvent::PolicyDecision,
        storage_sqlite::SecurityAuditEvent::IsolationEnforcement,
        storage_sqlite::SecurityAuditEvent::ProviderInvocation,
    ] {
        assert!(
            audit.iter().any(|record| {
                record.fact.event == event
                    && record.fact.task_id.as_deref() == Some("TASK-0078")
                    && record.fact.execution_id.is_none()
                    && record.fact.attempt_id.is_none()
            }),
            "secured planner call must emit {event:?} through the production G6 boundary"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn authenticated_claude_planner_refuses_with_not_started_spend_before_provider_work() {
    let db = TestDb::new();
    let marker = db.dir.join("planner-provider-work-started");
    let provider = fixture(
        &db.dir,
        "planner-authenticated.sh",
        &format!(
            "if [ \"$1\" = \"--version\" ]; then printf '2.1.277 (Claude Code)\\n'; exit 0; fi\n\
             if [ \"$1\" = \"auth\" ] && [ \"$2\" = \"status\" ]; then printf '{{\"loggedIn\":true}}\\n'; exit 0; fi\n\
             touch {:?}\n\
             exit 99\n",
            marker
        ),
    );
    let composition = ExecutionComposition::new(db.path.clone(), Some(provider));
    {
        let mut orchestration =
            OrchestrationService::open(&db.path, FixedClock(FIXED_MILLIS)).expect("orchestration");
        seed_planning_parent(
            &mut orchestration,
            "P-AUTH-CONFINEMENT",
            "TASK-0114",
            &["auth-child"],
        );
    }
    let mut planning = composition.planning_service().expect("planning service");
    let planner = composition
        .secured_claude_planner("TASK-0114")
        .expect("secured planner composition");
    let branch = PlanBranchId::new("task-0114-auth-confinement").expect("branch");

    let error = planning
        .propose_decomposition(
            &planner,
            &branch,
            request("fixture:auth-confinement"),
            &policy("TASK-0114", "policy:auth-confinement"),
        )
        .expect_err("confinement-incompatible auth must refuse before planner work");
    match error {
        app_services::PlanningServiceError::Planner(PlannerFault::ProviderFailure {
            typed_cause,
            spend,
        }) => {
            assert!(
                typed_cause
                    .contains("provider_authentication_unsupported_in_mandatory_confinement"),
                "unexpected typed cause: {typed_cause}"
            );
            assert_eq!(spend, PlannerCallSpend::NotStarted);
        }
        other => panic!("unexpected planner refusal: {other:?}"),
    }
    assert!(!marker.exists(), "planner provider work must not start");
    let audit = planning
        .store()
        .list_security_audit_ordered()
        .expect("security audit");
    assert!(
        !audit.iter().any(|record| {
            record.fact.event == storage_sqlite::SecurityAuditEvent::ProviderInvocation
                && record.fact.task_id.as_deref() == Some("TASK-0114")
        }),
        "pre-launch planner refusal must not record provider invocation"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn production_claude_planner_refuses_unknown_authority_task_before_provider_setup() {
    let db = TestDb::new();
    let provider = fixture(&db.dir, "unused-planner-provider.sh", "exit 99\n");
    let composition = ExecutionComposition::new(db.path.clone(), Some(provider));

    let error = composition
        .secured_claude_planner("missing-task")
        .err()
        .expect("unknown task must be refused");
    assert!(
        error.contains("does not exist"),
        "unexpected authority refusal: {error}"
    );
}
