//! Deterministic `PlannerPort` test double for TASK-0078.
//!
//! Scenario selection is adapter-local test behavior. All returned values use
//! the canonical plan-domain types and carry no storage, scheduler, policy, or
//! Task mutation capability.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use plan_domain::{
    compute_proposal_hash, resolved_estimated_input_tokens, AtomicityAssessment, AtomicityRequest,
    AtomicityVerdict, BudgetDelta, CriterionDecl, CriterionId, DecompositionRequest, EdgeAddition,
    EvidenceRef, NodeAddition, PlanChangeSet, PlanNodeId, PlanScope, PlannerAdapterId,
    PlannerCallSpend, PlannerFault, PlannerIdentity, PlannerPort, PlanningCostReceipt,
    ReplanRequest, ResolvedEvidence, RiskClass, VerificationMethod, MAX_CHILDREN_PER_DECOMPOSITION,
};

/// Deterministic behavior selected by tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FakePlannerScenario {
    Valid,
    MalformedProposal,
    ProviderFailure,
    GuardrailFanOut,
    StaleBase,
    DependencyChain,
}

/// Deterministic provider-neutral planner double.
#[derive(Clone, Debug)]
pub struct FakePlanner {
    scenario: FakePlannerScenario,
    fixed_leaf: Option<PlanNodeId>,
    calls: Arc<Mutex<Vec<&'static str>>>,
    /// Refs of resolved items received, in arrival order (`<method>:<ref>`).
    received_resolved: Arc<Mutex<Vec<String>>>,
}

impl FakePlanner {
    #[must_use]
    pub fn for_scenario(scenario: FakePlannerScenario) -> Self {
        Self {
            scenario,
            fixed_leaf: None,
            calls: Arc::new(Mutex::new(Vec::new())),
            received_resolved: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Builds the smallest accepted-plan fixture: one proposed child node
    /// `node_id`, which acceptance materializes as a new READY Task under the
    /// planning parent. `node_id` must not name the parent or any existing
    /// Task (TASK-0087). Useful for execution-boundary tests whose subject is
    /// G5 behavior but which must satisfy the G7 accepted-plan precondition.
    #[must_use]
    pub fn for_single_leaf(node_id: PlanNodeId) -> Self {
        Self {
            scenario: FakePlannerScenario::Valid,
            fixed_leaf: Some(node_id),
            calls: Arc::new(Mutex::new(Vec::new())),
            received_resolved: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.lock().expect("fake planner calls lock").len()
    }

    #[must_use]
    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("fake planner calls lock").clone()
    }

    /// `<method>:<ref>` for every resolved item received, in order.
    #[must_use]
    pub fn received_resolved_refs(&self) -> Vec<String> {
        self.received_resolved
            .lock()
            .expect("fake planner resolved lock")
            .clone()
    }

    fn record_resolved(&self, method: &'static str, items: &[ResolvedEvidence]) {
        let mut log = self
            .received_resolved
            .lock()
            .expect("fake planner resolved lock");
        for item in items {
            log.push(format!("{method}:{}", item.evidence_ref.as_str()));
        }
    }

    fn record(&self, method: &'static str) {
        self.calls
            .lock()
            .expect("fake planner calls lock")
            .push(method);
    }

    fn identity() -> PlannerIdentity {
        PlannerIdentity::new(
            PlannerAdapterId::new("fake-planner").expect("static adapter id"),
            "deterministic-v1".to_owned(),
        )
        .expect("static planner identity")
    }

    fn receipt() -> PlanningCostReceipt {
        PlanningCostReceipt::new(100, 50, 5, "test".to_owned()).expect("static receipt")
    }

    /// Simulates a provider that was invoked but failed without reporting
    /// usage: spend is unmeasured, never zero and never fabricated.
    fn failure<T>(&self) -> Result<T, PlannerFault> {
        Err(PlannerFault::ProviderFailure {
            typed_cause: "FAKE_PROVIDER_FAILURE".to_owned(),
            spend: PlannerCallSpend::Unmeasured,
        })
    }

    fn proposal_for_decomposition(&self, input: &DecompositionRequest) -> PlanChangeSet {
        let fan_out = if self.scenario == FakePlannerScenario::GuardrailFanOut {
            MAX_CHILDREN_PER_DECOMPOSITION + 1
        } else if self.scenario == FakePlannerScenario::DependencyChain {
            2
        } else {
            1
        };
        let scope_root = input
            .scope_bound
            .roots()
            .into_iter()
            .next()
            .expect("request scope is non-empty");
        let parent_criterion = input
            .acceptance_criteria_refs
            .first()
            .cloned()
            .unwrap_or_else(|| CriterionId::new("criterion-root").expect("static criterion"));
        let mut additions = Vec::with_capacity(fan_out);
        let mut child_ids = Vec::with_capacity(fan_out);
        for index in 0..fan_out {
            let child_criterion =
                CriterionId::new(format!("{}-child-{index}", parent_criterion.as_str()))
                    .expect("derived criterion");
            let node_id = if let Some(fixed) = &self.fixed_leaf {
                fixed.as_str().to_owned()
            } else if self.scenario == FakePlannerScenario::DependencyChain {
                if index == 0 { "step-a" } else { "step-b" }.to_owned()
            } else {
                format!("planned-child-{index}")
            };
            additions.push(NodeAddition {
                id: PlanNodeId::new(node_id).expect("derived node id"),
                scope: PlanScope::new(vec![scope_root.clone()]).expect("request scope"),
                criteria: vec![CriterionDecl::new(
                    child_criterion.clone(),
                    VerificationMethod::AutomatedTest,
                )],
                risk_class: RiskClass::Low,
                required_gate: None,
            });
            child_ids.push(child_criterion);
        }
        let edge_additions = if self.scenario == FakePlannerScenario::DependencyChain {
            vec![EdgeAddition {
                from: PlanNodeId::new("step-a").expect("static node id"),
                to: PlanNodeId::new("step-b").expect("static node id"),
            }]
        } else {
            vec![]
        };
        let mut proposal = PlanChangeSet {
            base_plan_revision: if self.scenario == FakePlannerScenario::StaleBase {
                input.base_plan_revision.saturating_add(1)
            } else {
                input.base_plan_revision
            },
            reason: "deterministic decomposition".to_owned(),
            source_evidence_refs: input.evidence_refs.clone(),
            node_additions: additions,
            node_updates: vec![],
            node_removals: vec![],
            edge_additions,
            edge_removals: vec![],
            expected_outcome: "validated child work becomes available".to_owned(),
            risk_delta: vec![],
            budget_delta: BudgetDelta::new(0, 0),
            scope_delta: PlanScope::new(vec![scope_root]).expect("request scope"),
            coverage_map: BTreeMap::from([(parent_criterion, child_ids)]),
            proposal_hash: String::new(),
            proposed_by: Self::identity(),
            planning_cost_receipt: Self::receipt(),
        };
        proposal.proposal_hash = compute_proposal_hash(&proposal);
        if self.scenario == FakePlannerScenario::MalformedProposal {
            proposal.reason.clear();
        }
        proposal
    }

    fn proposal_for_replan(&self, input: &ReplanRequest) -> PlanChangeSet {
        let scope_root = input
            .scope_bound
            .roots()
            .into_iter()
            .next()
            .expect("request scope is non-empty");
        let criterion = CriterionId::new("replan-criterion").expect("static criterion");
        let mut proposal = PlanChangeSet {
            base_plan_revision: input.base_plan_revision,
            reason: "deterministic replan".to_owned(),
            source_evidence_refs: input.triggering_evidence_refs.clone(),
            node_additions: vec![NodeAddition {
                id: PlanNodeId::new("replanned-child").expect("static node id"),
                scope: PlanScope::new(vec![scope_root.clone()]).expect("request scope"),
                criteria: vec![CriterionDecl::new(
                    criterion.clone(),
                    VerificationMethod::AutomatedTest,
                )],
                risk_class: RiskClass::Low,
                required_gate: None,
            }],
            node_updates: vec![],
            node_removals: vec![],
            edge_additions: vec![],
            edge_removals: vec![],
            expected_outcome: "replacement leaf becomes available".to_owned(),
            risk_delta: vec![],
            budget_delta: BudgetDelta::new(0, 0),
            scope_delta: PlanScope::new(vec![scope_root]).expect("request scope"),
            coverage_map: BTreeMap::new(),
            proposal_hash: String::new(),
            proposed_by: Self::identity(),
            planning_cost_receipt: Self::receipt(),
        };
        proposal.proposal_hash = compute_proposal_hash(&proposal);
        proposal
    }
}

fn budget_exhausted<T>() -> Result<T, PlannerFault> {
    Err(PlannerFault::BudgetExhausted {
        spend: PlannerCallSpend::NotStarted,
    })
}

impl PlannerPort for FakePlanner {
    fn assess_atomicity(
        &self,
        input: AtomicityRequest,
    ) -> Result<AtomicityAssessment, PlannerFault> {
        self.record("assess_atomicity");
        self.record_resolved("assess_atomicity", &input.resolved_evidence);
        if self.scenario == FakePlannerScenario::ProviderFailure {
            return self.failure();
        }
        AtomicityAssessment::new(
            AtomicityVerdict::Decomposable,
            "deterministic fixture requires decomposition".to_owned(),
            input.evidence_refs,
            Self::receipt(),
        )
        .map_err(|error| PlannerFault::ProviderFailure {
            typed_cause: error.to_string(),
            spend: PlannerCallSpend::Measured(Self::receipt()),
        })
    }

    fn propose_decomposition(
        &self,
        input: DecompositionRequest,
    ) -> Result<PlanChangeSet, PlannerFault> {
        self.record("propose_decomposition");
        self.record_resolved("propose_decomposition", &input.resolved_evidence);
        if self.scenario == FakePlannerScenario::ProviderFailure {
            return self.failure();
        }
        if input.depth_remaining == 0 {
            return Err(PlannerFault::DepthExhausted);
        }
        if input.budget_remaining.calls_remaining == 0
            || input.budget_remaining.input_tokens_remaining == 0
            || resolved_estimated_input_tokens(&input.resolved_evidence)
                > input.budget_remaining.input_tokens_remaining
        {
            return budget_exhausted();
        }
        Ok(self.proposal_for_decomposition(&input))
    }

    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet, PlannerFault> {
        self.record("propose_replan");
        self.record_resolved("propose_replan", &input.resolved_evidence);
        if self.scenario == FakePlannerScenario::ProviderFailure {
            return self.failure();
        }
        if input.depth_remaining == 0 {
            return Err(PlannerFault::DepthExhausted);
        }
        if input.budget_remaining.calls_remaining == 0
            || input.budget_remaining.input_tokens_remaining == 0
            || resolved_estimated_input_tokens(&input.resolved_evidence)
                > input.budget_remaining.input_tokens_remaining
        {
            return budget_exhausted();
        }
        Ok(self.proposal_for_replan(&input))
    }
}

/// Convenience evidence ref for application-service fixtures.
#[must_use]
pub fn evidence_ref(value: &str) -> EvidenceRef {
    EvidenceRef::new(value).expect("fake planner fixture evidence")
}
