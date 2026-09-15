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
    PlannedOrchestrationOutcome, PlanningCycle, PlanningPolicyContext, PlanningService,
};
use fake_evaluator::{EvaluatorMode, FakeEvaluator};
use fake_planner::{FakePlanner, FakePlannerScenario};
use plan_domain::{
    AffectedParentCriterion, CriterionId, DecompositionRequest, EvidenceRef, PlanBranchId,
    PlanningBudget, RiskClass, ScopeBound, VerificationMethod,
    MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};
use policy::CapabilityKind;
use scheduler::{ExecutionPort, ExecutionProblem, WorkEvidence, WorkItem};
use task_domain::{Project, ProjectId, Task, TaskId, TaskState};

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

fn seed_ready_task(orchestration: &mut OrchestrationService<FixedClock>) -> TaskId {
    let project_id = ProjectId::new("P-PLANNING").expect("project id");
    orchestration
        .tasks_mut()
        .create_project(&Project::new(project_id.clone(), "planning fixture".to_owned()).unwrap())
        .expect("create project");
    let task = Task::draft(
        TaskId::new("planned-child-0").expect("task id"),
        project_id,
        "execute the planned leaf".to_owned(),
    )
    .expect("draft task");
    orchestration
        .tasks_mut()
        .create_task(&task)
        .expect("create task");
    orchestration
        .tasks_mut()
        .mark_ready(task.id())
        .expect("mark ready");
    task.id().clone()
}

fn seed_ready_chain_tasks(
    orchestration: &mut OrchestrationService<FixedClock>,
) -> (TaskId, TaskId) {
    let project_id = ProjectId::new("P-PLANNING-CHAIN").expect("project id");
    orchestration
        .tasks_mut()
        .create_project(
            &Project::new(project_id.clone(), "planning chain fixture".to_owned()).unwrap(),
        )
        .expect("create project");
    let first = Task::draft(
        TaskId::new("step-a").expect("task id"),
        project_id.clone(),
        "first planned step".to_owned(),
    )
    .expect("draft first task");
    let second = Task::draft(
        TaskId::new("step-b").expect("task id"),
        project_id,
        "dependent planned step".to_owned(),
    )
    .expect("draft second task");
    for task in [&first, &second] {
        orchestration
            .tasks_mut()
            .create_task(task)
            .expect("create task");
        orchestration
            .tasks_mut()
            .mark_ready(task.id())
            .expect("mark ready");
    }
    (first.id().clone(), second.id().clone())
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
    let task_id = seed_ready_task(&mut orchestration);
    let branch = PlanBranchId::new("task-0078-agentd").expect("branch");
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysRejected);
    let executor = DeterministicExecutor;
    let mut cycle = PlanningCycle::new();
    let scope = ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope");
    let policy = PlanningPolicyContext {
        scope_bound: scope.clone(),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: vec![AffectedParentCriterion {
            id: CriterionId::new("criterion-root").expect("criterion"),
            method: VerificationMethod::AutomatedTest,
        }],
        parent_task_ref: "TASK-0078".to_owned(),
        lineage_fingerprints: vec![],
        risk_grant: None,
        policy_fingerprint: "policy:agentd-planning-test".to_owned(),
    };
    let request = DecompositionRequest {
        parent_task_ref: "caller-value-is-overwritten".to_owned(),
        base_plan_revision: u64::MAX,
        acceptance_criteria_refs: vec![],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).expect("caller scope"),
        evidence_refs: vec![EvidenceRef::new("fixture:initial").expect("evidence")],
        depth_remaining: 4,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    };

    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &planner,
            &mut cycle,
            &branch,
            request,
            policy,
            &task_id,
            OrchestrationIds {
                execution_id: ExecutionId::new("E-PLANNING").expect("execution id"),
                attempt_id: AttemptId::new("A-PLANNING").expect("attempt id"),
            },
            &executor,
            &evaluator,
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
    let (first, second) = seed_ready_chain_tasks(&mut orchestration);
    let branch = PlanBranchId::new("task-0078-plan-chain").expect("branch");
    let planner = FakePlanner::for_scenario(FakePlannerScenario::DependencyChain);
    let executor = CountingExecutor::new();
    let evaluator = FakeEvaluator::new(EvaluatorMode::AlwaysVerified);
    let mut cycle = PlanningCycle::new();
    let scope = ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope");
    let policy = PlanningPolicyContext {
        scope_bound: scope.clone(),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: vec![AffectedParentCriterion {
            id: CriterionId::new("criterion-root").expect("criterion"),
            method: VerificationMethod::AutomatedTest,
        }],
        parent_task_ref: "TASK-0078-CHAIN".to_owned(),
        lineage_fingerprints: vec![],
        risk_grant: None,
        policy_fingerprint: "policy:agentd-plan-chain".to_owned(),
    };
    let request = DecompositionRequest {
        parent_task_ref: "caller-value-is-overwritten".to_owned(),
        base_plan_revision: u64::MAX,
        acceptance_criteria_refs: vec![],
        scope_bound: scope,
        evidence_refs: vec![EvidenceRef::new("fixture:chain").expect("evidence")],
        depth_remaining: 4,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    };

    let outcome = orchestration
        .schedule_with_planning(
            &mut planning,
            &planner,
            &mut cycle,
            &branch,
            request,
            policy,
            &second,
            OrchestrationIds {
                execution_id: ExecutionId::new("E-PLANNING-CHAIN").expect("execution id"),
                attempt_id: AttemptId::new("A-PLANNING-CHAIN").expect("attempt id"),
            },
            &executor,
            &evaluator,
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

#[cfg(target_os = "macos")]
#[test]
fn production_claude_planner_composition_crosses_g6_runner_before_provider_call() {
    let db = TestDb::new();
    let workspace = db.dir.join("planner-workspace");
    std::fs::create_dir_all(&workspace).expect("planner workspace");
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
    let mut planning = composition.planning_service().expect("planning service");
    let planner = composition
        .secured_claude_planner("TASK-0078", &workspace)
        .expect("secured planner composition");
    let branch = PlanBranchId::new("task-0078-secured-claude").expect("branch");
    let scope = ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope");
    let policy = PlanningPolicyContext {
        scope_bound: scope.clone(),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: vec![AffectedParentCriterion {
            id: CriterionId::new("criterion-root").expect("criterion"),
            method: VerificationMethod::AutomatedTest,
        }],
        parent_task_ref: "TASK-0078".to_owned(),
        lineage_fingerprints: vec![],
        risk_grant: None,
        policy_fingerprint: "policy:secured-claude-planner".to_owned(),
    };
    let request = DecompositionRequest {
        parent_task_ref: "caller-value-is-overwritten".to_owned(),
        base_plan_revision: u64::MAX,
        acceptance_criteria_refs: vec![],
        scope_bound: scope,
        evidence_refs: vec![EvidenceRef::new("fixture:secured").expect("evidence")],
        depth_remaining: 4,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    };
    let mut cycle = PlanningCycle::new();

    let outcome = planning
        .propose_decomposition(&planner, &mut cycle, &branch, request, &policy)
        .expect("secured planner proposal");
    let accepted = match outcome {
        app_services::PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("expected accepted secured planner proposal, got {other:?}"),
    };
    assert_eq!(accepted.revision().revision_seq, 1);
    assert_eq!(planning.store().get_plan_head(&branch).unwrap(), Some(1));

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
        "proposal-only planner must not receive workspace write authority"
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
