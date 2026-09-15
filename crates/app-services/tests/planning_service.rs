//! TASK-0078 regression coverage for the trusted proposal -> validation ->
//! persistence boundary.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use app_services::{
    FixedClock, PlanningCycle, PlanningPolicyContext, PlanningProposalOutcome, PlanningService,
    PlanningServiceError,
};
use fake_planner::{FakePlanner, FakePlannerScenario};
use plan_domain::{
    compute_proposal_hash, AffectedParentCriterion, AtomicityAssessment, AtomicityRequest,
    AtomicityVerdict, BudgetDelta, CriterionDecl, CriterionId, DecompositionRequest, EdgeAddition,
    EvidenceRef, NodeAddition, PlanBranchId, PlanChangeSet, PlanNodeId, PlanScope,
    PlannerAdapterId, PlannerFault, PlannerIdentity, PlannerPort, PlanningBudget,
    PlanningCostReceipt, ReplanRequest, RiskClass, ScopeBound, ValidationRefusal,
    VerificationMethod, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS, MAX_PLANNER_INPUT_TOKENS,
};
use storage_sqlite::SqliteStore;

const FIXED_MILLIS: u64 = 1_704_067_200_000;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    dir: PathBuf,
    path: PathBuf,
}

impl TestDb {
    fn new(name: &str) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "runlith-planning-service-{}-{name}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create test directory");
        Self {
            path: dir.join("runlith.db"),
            dir,
        }
    }

    fn service(&self) -> PlanningService<FixedClock> {
        PlanningService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("open planning service")
    }

    fn store(&self) -> SqliteStore {
        SqliteStore::open(&self.path).expect("open store")
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Clone, Copy, Debug)]
enum ProposalCase {
    Valid,
    Cyclic,
    InvalidHash,
    MissingCriteria,
    InvalidScope,
    PolicyWeakening,
    OversizedTokenReceipt,
    UpdateRemovedNode,
    DropRemovedCriteria,
    StaleBase(u64),
}

struct TestPlanner {
    case: ProposalCase,
    calls: AtomicUsize,
}

impl TestPlanner {
    fn new(case: ProposalCase) -> Self {
        Self {
            case,
            calls: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn proposal(&self, input: &DecompositionRequest) -> PlanChangeSet {
        let scope_root = input
            .scope_bound
            .roots()
            .into_iter()
            .next()
            .expect("test scope");
        let child_id = PlanNodeId::new(format!("node-{}", input.base_plan_revision + 1))
            .expect("derived node id");
        let child_criterion =
            CriterionId::new(format!("criterion-child-{}", input.base_plan_revision + 1))
                .expect("derived criterion");
        let criteria = vec![CriterionDecl::new(
            child_criterion.clone(),
            VerificationMethod::AutomatedTest,
        )];
        let coverage_map = input
            .acceptance_criteria_refs
            .first()
            .cloned()
            .map(|parent| BTreeMap::from([(parent, vec![child_criterion.clone()])]))
            .unwrap_or_default();

        let mut proposal = PlanChangeSet {
            base_plan_revision: match self.case {
                ProposalCase::StaleBase(base) => base,
                _ => input.base_plan_revision,
            },
            reason: "deterministic planning-service proposal".to_owned(),
            source_evidence_refs: input.evidence_refs.clone(),
            node_additions: vec![NodeAddition {
                id: child_id.clone(),
                scope: PlanScope::new(vec![scope_root.clone()]).expect("test node scope"),
                criteria,
                risk_class: RiskClass::Low,
                required_gate: None,
            }],
            node_updates: vec![],
            node_removals: vec![],
            edge_additions: vec![],
            edge_removals: vec![],
            expected_outcome: "one executable leaf is available".to_owned(),
            risk_delta: vec![],
            budget_delta: BudgetDelta::new(0, 0),
            scope_delta: PlanScope::new(vec![scope_root]).expect("test scope delta"),
            coverage_map,
            proposal_hash: String::new(),
            proposed_by: PlannerIdentity::new(
                PlannerAdapterId::new("planning-service-test").expect("adapter id"),
                "deterministic-test-model".to_owned(),
            )
            .expect("planner identity"),
            planning_cost_receipt: PlanningCostReceipt::new(100, 50, 5, "test".to_owned())
                .expect("receipt"),
        };

        match self.case {
            ProposalCase::Cyclic => {
                let second =
                    PlanNodeId::new(format!("node-cycle-{}", input.base_plan_revision + 1))
                        .expect("cycle node id");
                proposal.node_additions.push(NodeAddition {
                    id: second.clone(),
                    scope: proposal.node_additions[0].scope.clone(),
                    criteria: vec![],
                    risk_class: RiskClass::Low,
                    required_gate: None,
                });
                proposal.edge_additions = vec![
                    EdgeAddition {
                        from: child_id.clone(),
                        to: second.clone(),
                    },
                    EdgeAddition {
                        from: second,
                        to: child_id,
                    },
                ];
            }
            ProposalCase::MissingCriteria => {
                proposal.node_additions[0].criteria.clear();
            }
            ProposalCase::InvalidScope => {
                let escaped =
                    PlanScope::new(vec!["repo:outside".to_owned()]).expect("escaped scope");
                proposal.node_additions[0].scope = escaped.clone();
                proposal.scope_delta = escaped;
            }
            ProposalCase::PolicyWeakening => {
                proposal.node_additions[0].risk_class = RiskClass::High;
            }
            ProposalCase::OversizedTokenReceipt => {
                proposal.planning_cost_receipt =
                    PlanningCostReceipt::new(MAX_PLANNER_INPUT_TOKENS + 1, 1, 1, "test".to_owned())
                        .expect("oversized receipt remains representable evidence");
            }
            ProposalCase::UpdateRemovedNode => {
                let target = PlanNodeId::new(format!("node-{}", input.base_plan_revision))
                    .expect("derived existing node id");
                proposal.node_updates.push(plan_domain::NodeUpdate {
                    target: target.clone(),
                    scope: None,
                    risk_class: None,
                    required_gate: None,
                });
                proposal.node_removals.push(target);
            }
            ProposalCase::DropRemovedCriteria => {
                let target = PlanNodeId::new(format!("node-{}", input.base_plan_revision))
                    .expect("derived existing node id");
                proposal.node_removals.push(target);
                proposal.coverage_map.clear();
            }
            ProposalCase::Valid | ProposalCase::InvalidHash | ProposalCase::StaleBase(_) => {}
        }

        proposal.proposal_hash = compute_proposal_hash(&proposal);
        if matches!(self.case, ProposalCase::InvalidHash) {
            proposal.proposal_hash = "0".repeat(64);
        }
        proposal
    }
}

struct ConcurrentAdvancePlanner {
    path: PathBuf,
}

impl PlannerPort for ConcurrentAdvancePlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        TestPlanner::new(ProposalCase::Valid).assess_atomicity(input)
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        let mut writer =
            PlanningService::open(&self.path, FixedClock(FIXED_MILLIS)).expect("concurrent writer");
        let mut writer_cycle = PlanningCycle::new();
        let committed = writer
            .propose_decomposition(
                &TestPlanner::new(ProposalCase::Valid),
                &mut writer_cycle,
                &branch(),
                request("concurrent-writer"),
                &policy(false),
            )
            .expect("concurrent revision");
        assert!(matches!(committed, PlanningProposalOutcome::Accepted(_)));

        let mut forged = input;
        forged.base_plan_revision = forged.base_plan_revision.saturating_add(1);
        Ok(TestPlanner::new(ProposalCase::Valid).proposal(&forged))
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        let request = DecompositionRequest {
            parent_task_ref: input.branch_root_ref,
            base_plan_revision: input.base_plan_revision,
            acceptance_criteria_refs: vec![],
            scope_bound: input.scope_bound,
            evidence_refs: input.triggering_evidence_refs,
            depth_remaining: input.depth_remaining,
            budget_remaining: input.budget_remaining,
        };
        self.propose_decomposition(request)
    }
}

impl PlannerPort for TestPlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        AtomicityAssessment::new(
            AtomicityVerdict::Decomposable,
            "deterministic test assessment".to_owned(),
            input.evidence_refs,
        )
        .map_err(|error| PlannerFault::ProviderFailure {
            typed_cause: error.to_string(),
        })
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.proposal(&input))
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        let request = DecompositionRequest {
            parent_task_ref: input.branch_root_ref,
            base_plan_revision: input.base_plan_revision,
            acceptance_criteria_refs: vec![],
            scope_bound: input.scope_bound,
            evidence_refs: input.triggering_evidence_refs,
            depth_remaining: input.depth_remaining,
            budget_remaining: input.budget_remaining,
        };
        self.propose_decomposition(request)
    }
}

fn branch() -> PlanBranchId {
    PlanBranchId::new("task-0078-branch").expect("branch id")
}

fn policy(with_criterion: bool) -> PlanningPolicyContext {
    PlanningPolicyContext {
        scope_bound: ScopeBound::new(vec!["repo:src".to_owned()]).expect("scope bound"),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: if with_criterion {
            vec![AffectedParentCriterion {
                id: CriterionId::new("criterion-root").expect("criterion id"),
                method: VerificationMethod::AutomatedTest,
            }]
        } else {
            vec![]
        },
        parent_task_ref: "TASK-0078".to_owned(),
        lineage_fingerprints: vec![],
        risk_grant: None,
        policy_fingerprint: "policy:test".to_owned(),
    }
}

fn request(evidence: &str) -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: "untrusted-caller-value".to_owned(),
        base_plan_revision: u64::MAX,
        acceptance_criteria_refs: vec![],
        scope_bound: ScopeBound::new(vec!["caller:ignored".to_owned()]).expect("caller scope"),
        evidence_refs: vec![EvidenceRef::new(evidence).expect("evidence")],
        depth_remaining: 4,
        budget_remaining: PlanningBudget {
            calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
            input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
        },
    }
}

fn assert_no_plan_persisted(db: &TestDb) {
    assert_eq!(
        db.store().get_plan_head(&branch()).expect("read head"),
        None
    );
}

fn seed_head(service: &mut PlanningService<FixedClock>, target: u64) {
    let planner = TestPlanner::new(ProposalCase::Valid);
    let mut cycle = PlanningCycle::new();
    for revision in 1..=target {
        if revision > 1 {
            cycle.begin_new_evidence_event();
        }
        let outcome = service
            .propose_decomposition(
                &planner,
                &mut cycle,
                &branch(),
                request(&format!("seed-evidence-{revision}")),
                &policy(false),
            )
            .expect("seed planning call");
        assert!(matches!(outcome, PlanningProposalOutcome::Accepted(_)));
    }
}

#[test]
fn valid_proposal_validates_and_persists_one_revision() {
    let db = TestDb::new("valid");
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::Valid);
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("evidence-valid"),
            &policy(true),
        )
        .expect("valid proposal");
    let accepted = match outcome {
        PlanningProposalOutcome::Accepted(accepted) => accepted,
        other => panic!("expected accepted proposal, got {other:?}"),
    };

    assert_eq!(accepted.revision().revision_seq, 1);
    assert_eq!(accepted.revision().base_revision, 0);
    assert_eq!(cycle.revisions_accepted(), 1);
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).expect("head"), Some(1));
    assert_eq!(
        store.get_plan_graph(&branch(), 1).expect("graph"),
        Some(accepted.graph().clone())
    );
    assert_eq!(
        store
            .get_plan_cost_receipt(&accepted.revision().proposed_by.planning_cost_receipt_ref)
            .expect("receipt read"),
        Some(accepted.planning_cost_receipt().clone())
    );
}

#[test]
fn stale_base_uses_canonical_stored_head_and_never_persists() {
    let db = TestDb::new("stale-base");
    let mut service = db.service();
    seed_head(&mut service, 5);
    let planner = TestPlanner::new(ProposalCase::StaleBase(4));
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("stale-evidence"),
            &policy(false),
        )
        .expect("stale proposal is a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::StaleBase { expected, observed }) => {
            assert_eq!(expected, 5);
            assert_eq!(observed, 4);
        }
        other => panic!("expected STALE_BASE, got {other:?}"),
    }
    assert_eq!(service.store().get_plan_head(&branch()).unwrap(), Some(5));
    assert!(service
        .store()
        .get_plan_revision(&branch(), 6)
        .unwrap()
        .is_none());
}

#[test]
fn concurrent_head_advance_refuses_stale_graph_even_if_proposal_claims_new_head() {
    let db = TestDb::new("concurrent-head");
    let mut service = db.service();
    seed_head(&mut service, 1);
    let planner = ConcurrentAdvancePlanner {
        path: db.path.clone(),
    };
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("outer-proposal"),
            &policy(false),
        )
        .expect("concurrent head movement is a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::StaleBase { expected, observed }) => {
            assert_eq!(expected, 2);
            assert_eq!(observed, 1);
        }
        other => panic!("expected STALE_BASE after concurrent writer, got {other:?}"),
    }
    let store = db.store();
    assert_eq!(store.get_plan_head(&branch()).unwrap(), Some(2));
    assert!(store.get_plan_revision(&branch(), 3).unwrap().is_none());
    let graph = store
        .get_plan_graph(&branch(), 2)
        .unwrap()
        .expect("concurrent writer graph remains canonical");
    assert!(graph.nodes.iter().any(|node| node.id.as_str() == "node-2"));
    assert!(!graph.nodes.iter().any(|node| node.id.as_str() == "node-3"));
}

#[test]
fn malformed_and_invalid_proposals_refuse_before_persistence() {
    let cases = [
        (ProposalCase::Cyclic, "CYCLIC_PROPOSAL"),
        (ProposalCase::InvalidHash, "MALFORMED_PROPOSAL"),
        (ProposalCase::MissingCriteria, "CRITERION_DROPPED"),
        (ProposalCase::InvalidScope, "SCOPE_ESCAPE"),
    ];

    for (case, expected_code) in cases {
        let db = TestDb::new(expected_code);
        let mut service = db.service();
        let planner = TestPlanner::new(case);
        let mut cycle = PlanningCycle::new();
        let outcome = service
            .propose_decomposition(
                &planner,
                &mut cycle,
                &branch(),
                request(expected_code),
                &policy(true),
            )
            .expect("invalid proposal must be a typed refusal");
        match outcome {
            PlanningProposalOutcome::Refused(refusal) => assert_eq!(refusal.code(), expected_code),
            other => panic!("expected {expected_code}, got {other:?}"),
        }
        assert_no_plan_persisted(&db);
    }
}

#[test]
fn update_and_removal_of_same_node_is_typed_refusal_never_panic_or_persistence() {
    let db = TestDb::new("update-removed-node");
    let mut service = db.service();
    seed_head(&mut service, 1);
    let planner = TestPlanner::new(ProposalCase::UpdateRemovedNode);
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("update-removed-node"),
            &policy(false),
        )
        .expect("update/removal overlap must be a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::DanglingRef { detail }) => {
            assert!(detail.contains("removed by the same changeset"));
        }
        other => panic!("expected DANGLING_REF, got {other:?}"),
    }
    assert_eq!(service.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert!(service
        .store()
        .get_plan_revision(&branch(), 2)
        .unwrap()
        .is_none());
}

#[test]
fn removing_criterion_bearing_node_requires_derived_v3_coverage() {
    let db = TestDb::new("removed-criterion");
    let mut service = db.service();
    seed_head(&mut service, 1);
    let planner = TestPlanner::new(ProposalCase::DropRemovedCriteria);
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("drop-existing-criterion"),
            &policy(false),
        )
        .expect("criterion loss must be a typed refusal");

    match outcome {
        PlanningProposalOutcome::Refused(ValidationRefusal::CriterionDropped { detail }) => {
            assert!(detail.contains("criterion-child-1"));
        }
        other => panic!("expected CRITERION_DROPPED, got {other:?}"),
    }
    assert_eq!(service.store().get_plan_head(&branch()).unwrap(), Some(1));
    assert!(service
        .store()
        .get_plan_revision(&branch(), 2)
        .unwrap()
        .is_none());
}

#[test]
fn planner_call_budget_is_hard_and_prevents_ninth_call() {
    let db = TestDb::new("call-budget");
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::InvalidHash);
    let mut cycle = PlanningCycle::new();

    for call in 0..MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        let outcome = service
            .propose_decomposition(
                &planner,
                &mut cycle,
                &branch(),
                request(&format!("unique-evidence-{call}")),
                &policy(false),
            )
            .expect("malformed proposal refusal");
        assert!(matches!(
            outcome,
            PlanningProposalOutcome::Refused(ValidationRefusal::Malformed { .. })
        ));
    }
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );

    let blocked = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("ninth-call"),
            &policy(false),
        )
        .expect("budget trip");
    match blocked {
        PlanningProposalOutcome::GuardrailTripped(trip) => {
            assert_eq!(trip.bound, "planner-calls");
        }
        other => panic!("expected planner call guardrail, got {other:?}"),
    }
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );
    assert_no_plan_persisted(&db);
}

#[test]
fn provider_failure_consumes_conservative_budget_without_fabricating_usage_evidence() {
    let db = TestDb::new("provider-failure-budget");
    let mut service = db.service();
    let planner = FakePlanner::for_scenario(FakePlannerScenario::ProviderFailure);
    let mut cycle = PlanningCycle::new();

    for expected_calls in 1..=MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH {
        let error = service
            .propose_decomposition(
                &planner,
                &mut cycle,
                &branch(),
                request("provider-failure"),
                &policy(false),
            )
            .expect_err("provider failure remains explicit");
        assert!(matches!(error, PlanningServiceError::Planner(_)));
        assert_eq!(cycle.calls_made(), expected_calls);
        assert_eq!(
            cycle.cumulative_input_tokens(),
            expected_calls.saturating_mul(MAX_PLANNER_INPUT_TOKENS)
        );
    }

    let stopped = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("provider-failure"),
            &policy(false),
        )
        .expect("ninth attempt is stopped before provider call");
    assert!(matches!(
        stopped,
        PlanningProposalOutcome::GuardrailTripped(_)
    ));
    assert_eq!(
        planner.call_count(),
        MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH as usize
    );
    assert_eq!(
        cycle.cumulative_input_tokens(),
        MAX_PLANNER_CUMULATIVE_INPUT_TOKENS
    );
}

#[test]
fn token_budget_exhaustion_refuses_and_never_persists() {
    let db = TestDb::new("token-budget");
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::OversizedTokenReceipt);
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("token-budget"),
            &policy(false),
        )
        .expect("token guardrail");
    match outcome {
        PlanningProposalOutcome::GuardrailTripped(trip) => {
            assert_eq!(trip.bound, "planner-input-tokens-per-call");
        }
        other => panic!("expected token budget guardrail, got {other:?}"),
    }
    assert_eq!(planner.call_count(), 1);
    assert_no_plan_persisted(&db);
}

#[test]
fn persistence_failure_leaves_no_partial_plan_state() {
    let db = TestDb::new("persistence-failure");
    let impossible_sqlite_millis = i64::MAX as u64 + 1;
    let mut service =
        PlanningService::open(&db.path, FixedClock(impossible_sqlite_millis)).expect("open");
    let planner = TestPlanner::new(ProposalCase::Valid);
    let mut cycle = PlanningCycle::new();

    let error = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("persistence-failure"),
            &policy(false),
        )
        .expect_err("commit must fail after validation");
    assert!(matches!(error, PlanningServiceError::Storage(_)));
    assert_eq!(cycle.revisions_accepted(), 0);
    drop(service);
    assert_no_plan_persisted(&db);
}

#[test]
fn policy_weakening_is_rejected_before_persistence() {
    let db = TestDb::new("policy-rejection");
    let mut service = db.service();
    let planner = TestPlanner::new(ProposalCase::PolicyWeakening);
    let mut cycle = PlanningCycle::new();

    let outcome = service
        .propose_decomposition(
            &planner,
            &mut cycle,
            &branch(),
            request("policy-rejection"),
            &policy(false),
        )
        .expect("policy weakening must be a typed refusal");
    assert!(matches!(
        outcome,
        PlanningProposalOutcome::Refused(ValidationRefusal::PolicyWeakening { .. })
    ));
    assert_no_plan_persisted(&db);
}
