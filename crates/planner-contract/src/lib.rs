//! Shared provider-neutral `PlannerPort` contract suite (TASK-0078, G7).
//!
//! The suite is intentionally about the proposal-only seam. It never grants
//! mutation authority and performs no persistence. Concrete planners invoke
//! it from adapter-owned tests with deterministic fixtures.

use plan_domain::{
    call_within_envelope, verify_proposal_hash, AtomicityRequest, AtomicityVerdict, CriterionId,
    DecompositionRequest, EvidenceRef, PlannerCallSpend, PlannerFault, PlannerPort, PlanningBudget,
    ReplanRequest, ScopeBound, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
};

/// Concrete planner behaviors supplied by one adapter's test target.
pub struct ContractPlanners<'a> {
    /// Deterministic valid behavior for assessment, decomposition, and replan.
    pub valid: &'a dyn PlannerPort,
    /// Deterministic provider/planner failure.
    pub failure: &'a dyn PlannerPort,
    /// Malformed/unusable proposal behavior.
    pub malformed: &'a dyn PlannerPort,
}

/// Runs the provider-neutral `PlannerPort` contract.
pub fn run_planner_port_contract(planners: &ContractPlanners<'_>) {
    atomicity_is_proposal_only_data(planners.valid);
    decomposition_is_bounded_untrusted_data(planners.valid);
    replan_is_branch_scoped_bounded_data(planners.valid);
    guardrail_faults_are_typed_before_proposal(planners.valid);
    provider_failure_is_typed(planners.failure);
    malformed_never_becomes_a_valid_proposal(planners.malformed);
}

fn bound() -> ScopeBound {
    ScopeBound::new(vec!["repo:src".to_owned()]).expect("contract scope")
}

fn criterion() -> CriterionId {
    CriterionId::new("criterion-root").expect("contract criterion")
}

fn evidence() -> EvidenceRef {
    EvidenceRef::new("evidence-contract").expect("contract evidence")
}

fn budget() -> PlanningBudget {
    PlanningBudget {
        calls_remaining: MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
        input_tokens_remaining: MAX_PLANNER_CUMULATIVE_INPUT_TOKENS,
    }
}

/// Shared deterministic assessment request.
#[must_use]
pub fn contract_atomicity_request() -> AtomicityRequest {
    AtomicityRequest {
        task_ref: "TASK-CONTRACT".to_owned(),
        acceptance_criteria_refs: vec![criterion()],
        scope_bound: bound(),
        evidence_refs: vec![evidence()],
        execution_policy_ref: "policy:contract".to_owned(),
    }
}

/// Shared deterministic decomposition request.
#[must_use]
pub fn contract_decomposition_request() -> DecompositionRequest {
    DecompositionRequest {
        parent_task_ref: "TASK-CONTRACT".to_owned(),
        base_plan_revision: 0,
        acceptance_criteria_refs: vec![criterion()],
        scope_bound: bound(),
        evidence_refs: vec![evidence()],
        depth_remaining: 4,
        budget_remaining: budget(),
    }
}

/// Shared deterministic replan request.
#[must_use]
pub fn contract_replan_request() -> ReplanRequest {
    ReplanRequest {
        branch_root_ref: "TASK-CONTRACT".to_owned(),
        base_plan_revision: 1,
        triggering_evidence_refs: vec![evidence()],
        scope_bound: bound(),
        depth_remaining: 3,
        budget_remaining: budget(),
    }
}

fn atomicity_is_proposal_only_data(planner: &dyn PlannerPort) {
    let first = planner
        .assess_atomicity(contract_atomicity_request())
        .expect("valid planner must assess atomicity");
    let second = planner
        .assess_atomicity(contract_atomicity_request())
        .expect("assessment must remain callable without mutation authority");
    assert!(!first.rationale.trim().is_empty());
    assert!(!second.rationale.trim().is_empty());
    assert!(
        call_within_envelope(&first.planning_cost_receipt),
        "assessment cost evidence must be inside the accepted call envelope"
    );
    assert!(matches!(
        first.verdict,
        AtomicityVerdict::Atomic
            | AtomicityVerdict::Decomposable
            | AtomicityVerdict::Blocked { .. }
    ));
}

fn decomposition_is_bounded_untrusted_data(planner: &dyn PlannerPort) {
    let request = contract_decomposition_request();
    let proposal = planner
        .propose_decomposition(request.clone())
        .expect("valid planner must propose decomposition");
    assert_eq!(proposal.base_plan_revision, request.base_plan_revision);
    proposal
        .check_wellformed()
        .expect("valid planner proposal must be structurally well formed");
    assert!(
        verify_proposal_hash(&proposal),
        "valid planner must bind the canonical proposal hash"
    );
    assert!(
        call_within_envelope(&proposal.planning_cost_receipt),
        "planner cost evidence must be inside the accepted call envelope"
    );
    assert!(!proposal.proposed_by.adapter_id.as_str().trim().is_empty());
    assert!(!proposal.proposed_by.model_ref.trim().is_empty());
}

fn replan_is_branch_scoped_bounded_data(planner: &dyn PlannerPort) {
    let request = contract_replan_request();
    let proposal = planner
        .propose_replan(request.clone())
        .expect("valid planner must propose a bounded replan");
    assert_eq!(proposal.base_plan_revision, request.base_plan_revision);
    proposal
        .check_wellformed()
        .expect("valid replan must be structurally well formed");
    assert!(verify_proposal_hash(&proposal));
    assert!(call_within_envelope(&proposal.planning_cost_receipt));
}

fn provider_failure_is_typed(planner: &dyn PlannerPort) {
    let error = planner
        .propose_decomposition(contract_decomposition_request())
        .expect_err("failure behavior must not produce a proposal");
    assert!(
        matches!(error, PlannerFault::ProviderFailure { .. }),
        "provider/planner failure must stay typed, got {error:?}"
    );
    assert_ne!(
        error.spend(),
        PlannerCallSpend::NotStarted,
        "a failure observed after invoking the provider must never claim zero spend"
    );
}

fn guardrail_faults_are_typed_before_proposal(planner: &dyn PlannerPort) {
    // Pre-call refusals happen before any provider spend and must say so.
    let mut depth = contract_decomposition_request();
    depth.depth_remaining = 0;
    assert!(matches!(
        planner.propose_decomposition(depth),
        Err(PlannerFault::DepthExhausted)
    ));

    let mut call_budget = contract_decomposition_request();
    call_budget.budget_remaining.calls_remaining = 0;
    assert!(matches!(
        planner.propose_decomposition(call_budget),
        Err(PlannerFault::BudgetExhausted {
            spend: PlannerCallSpend::NotStarted
        })
    ));

    let mut token_budget = contract_replan_request();
    token_budget.budget_remaining.input_tokens_remaining = 0;
    assert!(matches!(
        planner.propose_replan(token_budget),
        Err(PlannerFault::BudgetExhausted {
            spend: PlannerCallSpend::NotStarted
        })
    ));
}

fn malformed_never_becomes_a_valid_proposal(planner: &dyn PlannerPort) {
    match planner.propose_decomposition(contract_decomposition_request()) {
        Err(PlannerFault::ProviderFailure { .. }) => {}
        Err(other) => {
            panic!("malformed behavior must be provider failure or invalid data, got {other:?}")
        }
        Ok(proposal) => {
            assert!(
                proposal.check_wellformed().is_err() || !verify_proposal_hash(&proposal),
                "malformed planner data must never satisfy the trusted proposal checks"
            );
        }
    }
}
