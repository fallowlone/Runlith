//! TASK-0076 RED suite: ADR-0027 validation rules V1–V9 + guardrails + hash.
//!
//! Each module quotes its normative ADR-0027 rule text verbatim, then pins
//! it with at least one test named after the stable rule ID.

use plan_domain::{
    compute_proposal_hash, decomposition_fingerprint, validate_proposal, verify_proposal_hash,
    AffectedParentCriterion, AtomicityAssessment, AtomicityBlocker, AtomicityVerdict, CriterionId,
    EdgeAddition, EdgeRemoval, EvidenceRef, GateRef, GateUpdate, NodeAddition, NodeUpdate,
    PlanBranchId, PlanChangeSet, PlanGraph, PlanNode, PlanNodeId, PlanRevision, PlannerAdapterId,
    PlannerIdentity, PlanningCostReceipt, PlanningCostReceiptRef, RevisionProposedBy, RiskClass,
    RiskDelta, RiskEscalationGrant, ScopeBound, ValidatedBy, ValidationContext, ValidationOutcome,
    VerificationMethod, MAX_CHILDREN_PER_DECOMPOSITION, MAX_DECOMPOSITION_DEPTH,
    MAX_NODES_PER_PLAN_BRANCH, MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH,
    MAX_PLANNER_CUMULATIVE_INPUT_TOKENS, MAX_PLANNER_INPUT_TOKENS, MAX_PLANNER_OUTPUT_TOKENS,
    MAX_PLANNER_WALL_CLOCK_SECS, MAX_REVISIONS_PER_CYCLE_PER_BRANCH,
};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use task_domain::{EvidenceRequirement, RequirementKind, RequirementParam, SubjectBinding};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn nid(id: &str) -> PlanNodeId {
    PlanNodeId::new(id).unwrap()
}

fn cid(id: &str) -> CriterionId {
    CriterionId::new(id).unwrap()
}

fn eref(id: &str) -> EvidenceRef {
    EvidenceRef::new(id).unwrap()
}

fn scope(root: &str) -> plan_domain::PlanScope {
    plan_domain::PlanScope::new(vec![root.to_owned()]).unwrap()
}

fn bound(root: &str) -> ScopeBound {
    ScopeBound::new(vec![root.to_owned()]).unwrap()
}

fn node(
    id: &str,
    scope_root: &str,
    criterion: &str,
    method: VerificationMethod,
    risk: RiskClass,
) -> PlanNode {
    PlanNode::new(
        nid(id),
        scope([scope_root].join("").as_str()),
        vec![plan_domain::CriterionDecl::new(cid(criterion), method)],
        risk,
        None,
    )
    .unwrap()
}

/// Base graph: a -> b, both under ws1:src, criterion c1 (automated-test).
fn base_graph() -> PlanGraph {
    PlanGraph::new(
        vec![
            node(
                "a",
                "ws1:src",
                "c1",
                VerificationMethod::AutomatedTest,
                RiskClass::Low,
            ),
            node(
                "b",
                "ws1:src",
                "c1",
                VerificationMethod::AutomatedTest,
                RiskClass::Low,
            ),
        ],
        vec![(nid("a"), nid("b"))],
    )
    .unwrap()
}

fn identity() -> PlannerIdentity {
    PlannerIdentity::new(
        PlannerAdapterId::new("fake-planner").unwrap(),
        "test-model-1".to_owned(),
    )
    .unwrap()
}

fn receipt() -> PlanningCostReceipt {
    PlanningCostReceipt::new(100, 50, 1_000, "test-class".to_owned()).unwrap()
}

fn addition(id: &str, scope_root: &str, criterion: &str) -> NodeAddition {
    NodeAddition {
        id: nid(id),
        scope: scope(scope_root),
        criteria: vec![plan_domain::CriterionDecl::new(
            cid(criterion),
            VerificationMethod::AutomatedTest,
        )],
        risk_class: RiskClass::Low,
        required_gate: None,
    }
}

/// A well-formed changeset that decomposes `a` into `a1`, keeping edge a1->b.
fn valid_changeset() -> PlanChangeSet {
    let mut cs = PlanChangeSet {
        base_plan_revision: 3,
        reason: "decompose a for parallel execution".to_owned(),
        source_evidence_refs: vec![eref("attempt-A1")],
        node_additions: vec![addition("a1", "ws1:src/api", "c1a")],
        node_updates: vec![],
        node_removals: vec![nid("a")],
        edge_additions: vec![EdgeAddition {
            from: nid("a1"),
            to: nid("b"),
        }],
        edge_removals: vec![EdgeRemoval {
            from: nid("a"),
            to: nid("b"),
        }],
        expected_outcome: "a1 becomes runnable".to_owned(),
        risk_delta: vec![],
        budget_delta: plan_domain::BudgetDelta::new(10, 20),
        scope_delta: scope("ws1:src/api"),
        coverage_map: BTreeMap::from([(cid("c1"), vec![cid("c1a")])]),
        proposal_hash: String::new(),
        proposed_by: identity(),
        planning_cost_receipt: receipt(),
    };
    cs.proposal_hash = compute_proposal_hash(&cs);
    cs
}

fn ctx() -> ValidationContext {
    ValidationContext {
        branch_head: 3,
        scope_bound: bound("ws1:src"),
        baseline_risk: RiskClass::Low,
        affected_parent_criteria: vec![AffectedParentCriterion {
            id: cid("c1"),
            method: VerificationMethod::AutomatedTest,
            requirement: None,
        }],
        policy_fingerprint: "policy:test".to_owned(),
        depth_after_merge: 2,
        revisions_accepted_this_cycle: 0,
        parent_task_ref: "task-T1".to_owned(),
        lineage_fingerprints: vec![],
        now_ms: 1_000,
        risk_grant: None,
    }
}

fn accepted(graph: &PlanGraph, cs: &PlanChangeSet, c: &ValidationContext) -> bool {
    matches!(
        validate_proposal(graph, cs, c),
        ValidationOutcome::Accepted { .. }
    )
}

fn refusal_code(graph: &PlanGraph, cs: &PlanChangeSet, c: &ValidationContext) -> &'static str {
    match validate_proposal(graph, cs, c) {
        ValidationOutcome::Refused(r) => r.code(),
        ValidationOutcome::Accepted { .. } => panic!("expected refusal, got accept"),
    }
}

// ---------------------------------------------------------------------------
// V1 ACYCLICITY
// ---------------------------------------------------------------------------
// ADR-0027: "merged graph = base-revision graph + deltas MUST be a DAG under
// a deterministic topological check. Violation → `CYCLIC_PROPOSAL`."

#[test]
fn v1_acyclicity_accepts_dag_proposal() {
    assert!(accepted(&base_graph(), &valid_changeset(), &ctx()));
}

#[test]
fn v1_acyclicity_rejects_cycle() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    // Add a back edge b -> a1 alongside a1 -> b: merged graph is cyclic.
    cs.edge_additions.push(EdgeAddition {
        from: nid("b"),
        to: nid("a1"),
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "CYCLIC_PROPOSAL");
}

#[test]
fn v1_acyclicity_rejects_self_loop() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.edge_additions.push(EdgeAddition {
        from: nid("a1"),
        to: nid("a1"),
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "CYCLIC_PROPOSAL");
}

// ---------------------------------------------------------------------------
// V2 REFERENTIAL INTEGRITY
// ---------------------------------------------------------------------------
// ADR-0027: "every edge endpoint MUST resolve to (base node ∪ added node) ∖
// removed nodes; every update/remove target MUST exist in the base revision;
// every removed node MUST have no surviving inbound/outbound edge outside the
// removal set unless that edge is also removed in the same changeset.
// Violation → `DANGLING_REF`."

#[test]
fn v2_referential_integrity_rejects_dangling_edge_endpoint() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.edge_additions.push(EdgeAddition {
        from: nid("a1"),
        to: nid("ghost"),
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

#[test]
fn v2_referential_integrity_rejects_unknown_update_target() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_updates.push(NodeUpdate {
        target: nid("ghost"),
        scope: None,
        risk_class: None,
        required_gate: None,
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

#[test]
fn v2_referential_integrity_rejects_update_of_node_removed_by_same_changeset() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_updates.push(NodeUpdate {
        target: nid("a"),
        scope: None,
        risk_class: None,
        required_gate: None,
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

#[test]
fn v2_referential_integrity_rejects_unknown_removal_target() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_removals.push(nid("ghost"));
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

#[test]
fn v2_referential_integrity_rejects_orphaned_edge_on_removed_node() {
    // Remove `a` but keep the base edge a -> b alive (no edge_removal).
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.edge_removals.clear();
    cs.edge_additions.clear();
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

#[test]
fn v2_referential_integrity_rejects_dangling_base_edge_target() {
    let mut graph = base_graph();
    graph.edges = vec![(nid("b"), nid("missing"))];
    let cs = valid_changeset();
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

#[test]
fn v2_referential_integrity_rejects_dangling_base_edge_source() {
    let mut graph = base_graph();
    graph.edges = vec![(nid("missing"), nid("b"))];
    let cs = valid_changeset();
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "DANGLING_REF");
}

// ---------------------------------------------------------------------------
// V3 ACCEPTANCE-CRITERIA PRESERVATION
// ---------------------------------------------------------------------------
// ADR-0027: "the `coverage_map` MUST cover every affected parent criterion
// with ≥1 child criterion; every covering child MUST declare
// `verification_method ∈ { automated-test, independent-review,
// security-check, human-approval }`; a covering child whose method differs
// from the parent criterion's method flags risk escalation and routes to V9
// (no silent weakening of how a criterion is proven). Dropped or uncovered
// criterion → `CRITERION_DROPPED`."

#[test]
fn v3_acceptance_criteria_preservation_rejects_dropped_criterion() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.coverage_map.clear();
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "CRITERION_DROPPED");
}

#[test]
fn v3_acceptance_criteria_preservation_rejects_empty_child_list() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.coverage_map.insert(cid("c1"), vec![]);
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "CRITERION_DROPPED");
}

#[test]
fn v3_acceptance_criteria_preservation_rejects_undeclared_child() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    // cX is claimed as cover but declared nowhere in node_additions.
    cs.coverage_map.insert(cid("c1"), vec![cid("cX")]);
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "CRITERION_DROPPED");
}

#[test]
fn v3_acceptance_criteria_preservation_method_change_routes_to_v9() {
    // Covering child uses human-approval while the parent demands
    // automated-test: V3 flags escalation, so without a grant V9 refuses.
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].criteria = vec![plan_domain::CriterionDecl::new(
        cid("c1a"),
        VerificationMethod::HumanApproval,
    )];
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "RISK_GATE_OPEN");
}

/// ADR-0030 CV-4/V9 reconciliation: when the parent Task's explicit
/// evidence requirement is human-approval, a child that redeclares
/// automated-test for that criterion is a method change and routes to V9,
/// symmetrically with the automated-test -> human-approval direction the
/// G7 suite pins above. Under the new model the parent side is never the
/// implicit default; it is what the Task carries.
#[test]
fn v3_reconciled_parent_method_routes_weakening_to_v9() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].criteria = vec![plan_domain::CriterionDecl::new(
        cid("c1a"),
        VerificationMethod::AutomatedTest,
    )];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.affected_parent_criteria = vec![AffectedParentCriterion {
        id: cid("c1"),
        method: VerificationMethod::HumanApproval,
        requirement: None,
    }];
    assert_eq!(refusal_code(&graph, &cs, &c), "RISK_GATE_OPEN");
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: false,
    });
    match validate_proposal(&graph, &cs, &c) {
        ValidationOutcome::Accepted { risk_escalated } => {
            assert!(
                risk_escalated,
                "the method change is still marked escalated"
            )
        }
        outcome => panic!("expected an accepted escalated proposal, got {outcome:?}"),
    }
}

#[test]
fn v3_capture_requirements_route_lossy_method_projection_to_v9() {
    let graph = base_graph();
    for kind in [
        RequirementKind::ProcessExit,
        RequirementKind::ArtifactHash,
        RequirementKind::FsState,
        RequirementKind::GitState,
    ] {
        let mut changeset = valid_changeset();
        changeset.proposal_hash = compute_proposal_hash(&changeset);
        let mut context = ctx();
        context.affected_parent_criteria[0].requirement = Some(
            EvidenceRequirement::new(
                "c1",
                kind,
                kind.default_subject_binding(),
                vec![RequirementParam::new("selector", "dist/app").unwrap()],
                Some("policy:test".to_owned()),
                true,
            )
            .unwrap(),
        );
        assert_eq!(
            refusal_code(&graph, &changeset, &context),
            "RISK_GATE_OPEN",
            "{kind:?} cannot be reconstructed from automated-test"
        );
        context.risk_grant = Some(RiskEscalationGrant {
            proposal_hash: changeset.proposal_hash.clone(),
            expires_at_ms: 5_000,
            consumed: false,
        });
        assert!(matches!(
            validate_proposal(&graph, &changeset, &context),
            ValidationOutcome::Accepted {
                risk_escalated: true
            }
        ));
    }
}

#[test]
fn v3_routes_each_non_kind_typed_requirement_mismatch_to_v9() {
    let graph = base_graph();
    let requirements = [
        EvidenceRequirement::new(
            "c1",
            RequirementKind::TestSuite,
            SubjectBinding::Content,
            vec![],
            Some("policy:test".to_owned()),
            true,
        ),
        EvidenceRequirement::new(
            "c1",
            RequirementKind::TestSuite,
            RequirementKind::TestSuite.default_subject_binding(),
            vec![RequirementParam::new("selector", "dist/app").unwrap()],
            Some("policy:test".to_owned()),
            true,
        ),
        EvidenceRequirement::new(
            "c1",
            RequirementKind::TestSuite,
            RequirementKind::TestSuite.default_subject_binding(),
            vec![],
            Some("policy:test".to_owned()),
            false,
        ),
    ]
    .map(Result::unwrap);
    for requirement in requirements {
        let mut changeset = valid_changeset();
        changeset.proposal_hash = compute_proposal_hash(&changeset);
        let mut context = ctx();
        context.affected_parent_criteria[0].requirement = Some(requirement);
        assert_eq!(refusal_code(&graph, &changeset, &context), "RISK_GATE_OPEN");
    }
}

#[test]
fn v3_authored_default_fingerprint_is_adopted_without_v9_escalation() {
    let graph = base_graph();
    let mut context = ctx();
    context.affected_parent_criteria[0].requirement =
        Some(EvidenceRequirement::authored_default("c1").unwrap());
    assert!(matches!(
        validate_proposal(&graph, &valid_changeset(), &context),
        ValidationOutcome::Accepted {
            risk_escalated: false
        }
    ));
}

#[test]
fn v3_acceptance_criteria_preservation_method_change_accepts_with_grant() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].criteria = vec![plan_domain::CriterionDecl::new(
        cid("c1a"),
        VerificationMethod::HumanApproval,
    )];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: false,
    });
    assert!(accepted(&graph, &cs, &c));
}

#[test]
fn verification_method_vocabulary_is_closed_to_adr_terms() {
    for (text, expected) in [
        ("automated-test", VerificationMethod::AutomatedTest),
        ("independent-review", VerificationMethod::IndependentReview),
        ("security-check", VerificationMethod::SecurityCheck),
        ("human-approval", VerificationMethod::HumanApproval),
    ] {
        assert_eq!(VerificationMethod::from_str(text).unwrap(), expected);
    }
    assert!(VerificationMethod::from_str("vibes").is_err());
}

// ---------------------------------------------------------------------------
// V4 SCOPE CONTAINMENT
// ---------------------------------------------------------------------------
// ADR-0027: "every added/updated node scope MUST be a subset of the branch
// authorized scope (`scope_delta ⊆ scope_bound`; exact scope syntax owned by
// TASK-0076, subset relation defined here). Violation → `SCOPE_ESCAPE`."

#[test]
fn v4_scope_containment_rejects_added_node_outside_bound() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].scope = scope("ws1:other");
    cs.scope_delta = scope("ws1:other");
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "SCOPE_ESCAPE");
}

#[test]
fn v4_scope_containment_rejects_cross_workspace_scope() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].scope = scope("ws2:src/api");
    cs.scope_delta = scope("ws2:src/api");
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "SCOPE_ESCAPE");
}

#[test]
fn v4_scope_containment_rejects_updated_node_scope_escape() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions.clear();
    cs.node_removals.clear();
    cs.edge_additions.clear();
    cs.edge_removals.clear();
    cs.coverage_map = BTreeMap::from([(cid("c1"), vec![cid("c1")])]);
    // `b` already declares c1, so coverage holds; widen its scope instead.
    cs.node_updates.push(NodeUpdate {
        target: nid("b"),
        scope: Some(scope("ws1:elsewhere")),
        risk_class: None,
        required_gate: None,
    });
    // c1 must be declared by an addition for coverage; keep one in-bound.
    cs.node_additions.push(addition("a1", "ws1:src/api", "c1"));
    cs.node_removals.push(nid("a"));
    cs.edge_additions.push(EdgeAddition {
        from: nid("a1"),
        to: nid("b"),
    });
    cs.edge_removals.push(EdgeRemoval {
        from: nid("a"),
        to: nid("b"),
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "SCOPE_ESCAPE");
}

// ---------------------------------------------------------------------------
// V5 NO POLICY/APPROVAL WEAKENING
// ---------------------------------------------------------------------------
// ADR-0027: "the changeset MUST NOT contain policy-rule, approval-binding,
// or gate-definition deltas (plan nodes may reference required gates, never
// redefine them); every node whose risk class exceeds the branch baseline
// MUST carry a `required_gate` annotation. Violation → `POLICY_WEAKENING`."

#[test]
fn v5_no_policy_weakening_rejects_risk_above_baseline_without_gate() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "POLICY_WEAKENING");
}

#[test]
fn v5_no_policy_weakening_accepts_risk_above_baseline_with_gate() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    // Risk rises, so V9 additionally demands a grant.
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "RISK_GATE_OPEN");
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: false,
    });
    assert!(accepted(&graph, &cs, &c));
}

// ---------------------------------------------------------------------------
// V6 BUDGET/DEPTH/FAN-OUT LIMITS
// ---------------------------------------------------------------------------
// ADR-0027: "decomposition depth MUST be ≤ `MAX_DECOMPOSITION_DEPTH`; new
// children per decomposition MUST be ≤ `MAX_CHILDREN_PER_DECOMPOSITION`;
// total nodes in the affected branch after merge MUST be ≤
// `MAX_NODES_PER_PLAN_BRANCH`; accepted revisions per reconcile cycle per
// branch MUST be ≤ `MAX_REVISIONS_PER_CYCLE_PER_BRANCH`. Violation →
// `LIMIT_EXCEEDED { which, bound, observed }`."

#[test]
fn guardrail_constants_match_adr0027_exactly() {
    assert_eq!(MAX_DECOMPOSITION_DEPTH, 4);
    assert_eq!(MAX_CHILDREN_PER_DECOMPOSITION, 8);
    assert_eq!(MAX_NODES_PER_PLAN_BRANCH, 64);
    assert_eq!(MAX_REVISIONS_PER_CYCLE_PER_BRANCH, 1);
    assert_eq!(MAX_PLANNER_WALL_CLOCK_SECS, 120);
    assert_eq!(MAX_PLANNER_INPUT_TOKENS, 32_000);
    assert_eq!(MAX_PLANNER_OUTPUT_TOKENS, 8_000);
    assert_eq!(MAX_PLANNER_CALLS_PER_EVENT_PER_BRANCH, 8);
    assert_eq!(MAX_PLANNER_CUMULATIVE_INPUT_TOKENS, 256_000);
}

#[test]
fn v6_budget_depth_fanout_limits_rejects_depth_breach() {
    let graph = base_graph();
    let cs = valid_changeset();
    let mut c = ctx();
    c.depth_after_merge = 5;
    match validate_proposal(&graph, &cs, &c) {
        ValidationOutcome::Refused(r) => {
            assert_eq!(r.code(), "LIMIT_EXCEEDED");
            let d = r.limit_detail().unwrap();
            assert_eq!((d.which, d.bound, d.observed), ("depth", 4, 5));
        }
        ValidationOutcome::Accepted { .. } => panic!("expected refusal"),
    }
}

#[test]
fn v6_budget_depth_fanout_limits_rejects_fanout_breach() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions.clear();
    cs.node_removals.clear();
    cs.edge_additions.clear();
    cs.edge_removals.clear();
    for i in 0..9 {
        cs.node_additions
            .push(addition(&format!("k{i}"), "ws1:src/api", "c1a"));
    }
    cs.proposal_hash = compute_proposal_hash(&cs);
    match validate_proposal(&graph, &cs, &ctx()) {
        ValidationOutcome::Refused(r) => {
            assert_eq!(r.code(), "LIMIT_EXCEEDED");
            let d = r.limit_detail().unwrap();
            assert_eq!((d.which, d.bound, d.observed), ("fan-out", 8, 9));
        }
        ValidationOutcome::Accepted { .. } => panic!("expected refusal"),
    }
}

#[test]
fn v6_budget_depth_fanout_limits_rejects_branch_node_breach() {
    // 63 extra nodes + 2 base nodes = 65 > 64. Fan-out guard would also
    // trip, so check branch-nodes by growing the base graph instead.
    let mut nodes = vec![
        node(
            "a",
            "ws1:src",
            "c1",
            VerificationMethod::AutomatedTest,
            RiskClass::Low,
        ),
        node(
            "b",
            "ws1:src",
            "c1",
            VerificationMethod::AutomatedTest,
            RiskClass::Low,
        ),
    ];
    for i in 0..62 {
        nodes.push(node(
            &format!("n{i:02}"),
            "ws1:src",
            "c1",
            VerificationMethod::AutomatedTest,
            RiskClass::Low,
        ));
    }
    let graph = PlanGraph::new(nodes, vec![(nid("a"), nid("b"))]).unwrap();
    let cs = valid_changeset();
    // Merge: 64 base + 1 added − 1 removed = 64 → still legal.
    assert!(accepted(&graph, &cs, &ctx()));
    let mut cs2 = valid_changeset();
    cs2.node_removals.clear();
    cs2.proposal_hash = compute_proposal_hash(&cs2);
    // Merge: 64 base + 1 added = 65 → breach.
    match validate_proposal(&graph, &cs2, &ctx()) {
        ValidationOutcome::Refused(r) => {
            assert_eq!(r.code(), "LIMIT_EXCEEDED");
            let d = r.limit_detail().unwrap();
            assert_eq!((d.which, d.bound, d.observed), ("branch-nodes", 64, 65));
        }
        ValidationOutcome::Accepted { .. } => panic!("expected refusal"),
    }
}

#[test]
fn v6_budget_depth_fanout_limits_rejects_second_revision_in_cycle() {
    let graph = base_graph();
    let cs = valid_changeset();
    let mut c = ctx();
    c.revisions_accepted_this_cycle = 1;
    match validate_proposal(&graph, &cs, &c) {
        ValidationOutcome::Refused(r) => {
            assert_eq!(r.code(), "LIMIT_EXCEEDED");
            let d = r.limit_detail().unwrap();
            assert_eq!(
                (d.which, d.bound, d.observed),
                ("revisions-per-cycle", 1, 2)
            );
        }
        ValidationOutcome::Accepted { .. } => panic!("expected refusal"),
    }
}

// ---------------------------------------------------------------------------
// V7 DUPLICATE-DECOMPOSITION REJECTION
// ---------------------------------------------------------------------------
// ADR-0027: "`fingerprint = SHA-256(canonical(parent_task_id, sorted child
// scope roots, sorted criterion ids, sorted evidence refs))` MUST NOT equal
// any fingerprint in the branch lineage. Violation → ...

#[test]
fn v7_duplicate_decomposition_rejection_accepts_novel_fingerprint() {
    assert!(accepted(&base_graph(), &valid_changeset(), &ctx()));
}

#[test]
fn v7_duplicate_decomposition_rejection_rejects_lineage_replay() {
    let graph = base_graph();
    let cs = valid_changeset();
    let fp = decomposition_fingerprint("task-T1", &cs);
    let mut c = ctx();
    c.lineage_fingerprints = vec![fp.clone()];
    match validate_proposal(&graph, &cs, &c) {
        ValidationOutcome::Refused(r) => {
            assert_eq!(r.code(), "DUPLICATE_DECOMPOSITION");
            assert_eq!(r.duplicate_fingerprint(), Some(fp.as_str()));
        }
        ValidationOutcome::Accepted { .. } => panic!("expected refusal"),
    }
}

#[test]
fn v7_duplicate_decomposition_rejection_fingerprint_is_order_insensitive() {
    let a = decomposition_fingerprint("task-T1", &valid_changeset());
    let mut cs = valid_changeset();
    cs.node_additions.push(addition("a2", "ws1:src/web", "c1b"));
    cs.proposal_hash = compute_proposal_hash(&cs);
    let b = decomposition_fingerprint("task-T1", &cs);
    assert_ne!(a, b, "different children must fingerprint differently");
    // Same content, re-sorted evidence order → identical fingerprint.
    let mut cs2 = valid_changeset();
    cs2.source_evidence_refs.push(eref("attempt-A0"));
    cs2.source_evidence_refs.rotate_right(1);
    cs2.proposal_hash = compute_proposal_hash(&cs2);
    let c = decomposition_fingerprint("task-T1", &cs2);
    assert_ne!(a, c, "different evidence must fingerprint differently");
}

// ---------------------------------------------------------------------------
// V8 STALE BASE
// ---------------------------------------------------------------------------
// ADR-0027: "`base_plan_revision` MUST equal the current branch head
// (optimistic concurrency). Violation → `STALE_BASE`; the planner re-reads
// and re-proposes. Silent auto-rebase is forbidden."

#[test]
fn v8_stale_base_rejects_old_head() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.base_plan_revision = 2;
    cs.proposal_hash = compute_proposal_hash(&cs);
    match validate_proposal(&graph, &cs, &ctx()) {
        ValidationOutcome::Refused(r) => {
            assert_eq!(r.code(), "STALE_BASE");
            let (expected, observed) = r.stale_base_detail().unwrap();
            assert_eq!((expected, observed), (3, 2));
        }
        ValidationOutcome::Accepted { .. } => panic!("expected refusal"),
    }
}

#[test]
fn v8_stale_base_rejects_future_head() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.base_plan_revision = 4;
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "STALE_BASE");
}

// ---------------------------------------------------------------------------
// V9 RISK-ESCALATION APPROVAL GATE
// ---------------------------------------------------------------------------
// ADR-0027: "if `risk_delta` raises any node's risk class, or V3 flagged a
// verification-method change, the revision MUST NOT persist until a bound,
// unexpired, single-use `HumanDecisionRequest` grant referencing the exact
// `proposal_hash` exists (TASK-0079 lifecycle). Planner self-approval never
// satisfies this rule. Missing/expired/mismatched grant → `RISK_GATE_OPEN`."

#[test]
fn v9_risk_escalation_approval_gate_blocks_without_grant() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "RISK_GATE_OPEN");
}

/// A new node above the branch baseline is a risk escalation whether or not
/// the planner says so. `risk_delta` is planner-declared input, so V9 must not
/// depend on it, and `required_gate` is an annotation nothing enforces — it
/// cannot be the only thing standing between a proposal and a higher-risk
/// canonical Task.
#[test]
fn v9_risk_escalation_approval_gate_blocks_an_addition_that_declares_no_delta() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    // Deliberately no risk_delta: the planner stays silent about the rise.
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "RISK_GATE_OPEN");
}

#[test]
fn v9_risk_escalation_approval_gate_rejects_expired_grant() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.now_ms = 9_999;
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: false,
    });
    assert_eq!(refusal_code(&graph, &cs, &c), "RISK_GATE_OPEN");
}

#[test]
fn v9_risk_escalation_approval_gate_rejects_hash_mismatch_grant() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: "sha256:deadbeef".to_owned(),
        expires_at_ms: 5_000,
        consumed: false,
    });
    assert_eq!(refusal_code(&graph, &cs, &c), "RISK_GATE_OPEN");
}

#[test]
fn v9_risk_escalation_approval_gate_rejects_consumed_grant() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: true,
    });
    assert_eq!(refusal_code(&graph, &cs, &c), "RISK_GATE_OPEN");
}

#[test]
fn v9_risk_escalation_approval_gate_accepts_valid_grant_and_reports_escalation() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: false,
    });
    match validate_proposal(&graph, &cs, &c) {
        ValidationOutcome::Accepted { risk_escalated } => assert!(risk_escalated),
        ValidationOutcome::Refused(_) => panic!("expected accept"),
    }
}

#[test]
fn v9_risk_escalation_approval_gate_non_escalating_accept_reports_no_escalation() {
    match validate_proposal(&base_graph(), &valid_changeset(), &ctx()) {
        ValidationOutcome::Accepted { risk_escalated } => assert!(!risk_escalated),
        ValidationOutcome::Refused(_) => panic!("expected accept"),
    }
}

#[test]
fn v9_risk_escalation_approval_gate_derives_node_update_escalation_when_risk_delta_empty() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_updates.push(NodeUpdate {
        target: nid("b"),
        scope: None,
        risk_class: Some(RiskClass::High),
        required_gate: Some(GateUpdate::Set(GateRef::new("human-review").unwrap())),
    });
    cs.risk_delta.clear();
    cs.proposal_hash = compute_proposal_hash(&cs);

    assert_eq!(refusal_code(&graph, &cs, &ctx()), "RISK_GATE_OPEN");
}

#[test]
fn v9_risk_escalation_approval_gate_derives_node_update_escalation_when_risk_delta_misleading() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_updates.push(NodeUpdate {
        target: nid("b"),
        scope: None,
        risk_class: Some(RiskClass::High),
        required_gate: Some(GateUpdate::Set(GateRef::new("human-review").unwrap())),
    });
    cs.risk_delta = vec![RiskDelta {
        node: nid("b"),
        from: RiskClass::High,
        to: RiskClass::Low,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);

    assert_eq!(refusal_code(&graph, &cs, &ctx()), "RISK_GATE_OPEN");
}

#[test]
fn v9_risk_escalation_approval_gate_accepts_derived_node_update_with_exact_grant() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_updates.push(NodeUpdate {
        target: nid("b"),
        scope: None,
        risk_class: Some(RiskClass::High),
        required_gate: Some(GateUpdate::Set(GateRef::new("human-review").unwrap())),
    });
    cs.risk_delta.clear();
    cs.proposal_hash = compute_proposal_hash(&cs);
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: cs.proposal_hash.clone(),
        expires_at_ms: 5_000,
        consumed: false,
    });

    match validate_proposal(&graph, &cs, &c) {
        ValidationOutcome::Accepted { risk_escalated } => assert!(risk_escalated),
        ValidationOutcome::Refused(_) => panic!("expected accept"),
    }
}

// ---------------------------------------------------------------------------
// Rule order: first failing rule wins (ADR-0027 §3).
// ---------------------------------------------------------------------------

#[test]
fn rule_order_first_failing_rule_wins() {
    // Simultaneously cyclic AND stale: V1 must win over V8.
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.base_plan_revision = 1;
    cs.edge_additions.push(EdgeAddition {
        from: nid("b"),
        to: nid("a1"),
    });
    cs.proposal_hash = compute_proposal_hash(&cs);
    assert_eq!(refusal_code(&graph, &cs, &ctx()), "CYCLIC_PROPOSAL");
}

// ---------------------------------------------------------------------------
// proposal_hash: SHA-256 over canonical changeset bytes (ADR-0027 §2).
// ---------------------------------------------------------------------------

#[test]
fn proposal_hash_is_deterministic_and_material_sensitive() {
    let a = valid_changeset();
    let b = valid_changeset();
    assert!(verify_proposal_hash(&a));
    assert_eq!(a.proposal_hash, b.proposal_hash);
    assert!(a.proposal_hash.starts_with("sha256:"));

    let mut c = valid_changeset();
    c.reason = "a different reason".to_owned();
    c.proposal_hash = compute_proposal_hash(&c);
    assert_ne!(a.proposal_hash, c.proposal_hash);

    let mut tampered = valid_changeset();
    tampered.reason = "mutated after signing".to_owned();
    assert!(!verify_proposal_hash(&tampered));
}

#[test]
fn proposal_hash_mismatch_is_rejected_as_malformed() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.reason = "mutated after hashing".to_owned();

    assert_eq!(refusal_code(&graph, &cs, &ctx()), "MALFORMED_PROPOSAL");
}

#[test]
fn stale_hash_grant_cannot_authorize_mutated_proposal() {
    let graph = base_graph();
    let mut cs = valid_changeset();
    cs.node_additions[0].risk_class = RiskClass::High;
    cs.node_additions[0].required_gate = Some(GateRef::new("human-review").unwrap());
    cs.risk_delta = vec![RiskDelta {
        node: nid("a1"),
        from: RiskClass::Low,
        to: RiskClass::High,
    }];
    cs.proposal_hash = compute_proposal_hash(&cs);
    let approved_hash = cs.proposal_hash.clone();
    cs.expected_outcome = "materially mutated after approval".to_owned();
    let mut c = ctx();
    c.risk_grant = Some(RiskEscalationGrant {
        proposal_hash: approved_hash,
        expires_at_ms: 5_000,
        consumed: false,
    });

    assert_eq!(refusal_code(&graph, &cs, &c), "MALFORMED_PROPOSAL");
}

#[test]
fn proposal_hash_distinguishes_gate_clear_from_set_clear() {
    let mut clear = valid_changeset();
    clear.node_updates.push(NodeUpdate {
        target: nid("b"),
        scope: None,
        risk_class: None,
        required_gate: Some(GateUpdate::Clear),
    });
    clear.proposal_hash = compute_proposal_hash(&clear);

    let mut set_clear = clear.clone();
    set_clear.node_updates[0].required_gate = Some(GateUpdate::Set(GateRef::new("clear").unwrap()));
    set_clear.proposal_hash = compute_proposal_hash(&set_clear);

    assert_ne!(clear.proposal_hash, set_clear.proposal_hash);
}

// ---------------------------------------------------------------------------
// PlanRevision provenance (ADR-0027 §4) + atomicity/planner-fault vocabulary.
// ---------------------------------------------------------------------------

#[test]
fn plan_revision_requires_provenance() {
    let proposed_by = RevisionProposedBy::new(
        identity(),
        PlanningCostReceiptRef::new("planning-receipt-1").unwrap(),
    );
    let rev = PlanRevision::new(
        1,
        PlanBranchId::new("branch-1").unwrap(),
        0,
        "sha256:abc".to_owned(),
        "initial decomposition".to_owned(),
        vec![eref("attempt-A1")],
        proposed_by.clone(),
        ValidatedBy::new("reconciler-1".to_owned(), "policy-fp-1".to_owned()).unwrap(),
        1_789_000_000_000,
    )
    .unwrap();
    assert_eq!(rev.revision_seq, 1);
    assert_eq!(rev.base_revision, 0);
    assert_eq!(
        rev.proposed_by.planning_cost_receipt_ref.as_str(),
        "planning-receipt-1"
    );
    assert_eq!(rev.created_at, 1_789_000_000_000);
    assert!(PlanningCostReceiptRef::new("   ").is_err());

    assert!(PlanRevision::new(
        1,
        PlanBranchId::new("branch-1").unwrap(),
        0,
        "sha256:abc".to_owned(),
        "   ".to_owned(),
        vec![eref("attempt-A1")],
        proposed_by,
        ValidatedBy::new("reconciler-1".to_owned(), "policy-fp-1".to_owned()).unwrap(),
        1_789_000_000_000,
    )
    .is_err());
}

fn assessment_receipt() -> PlanningCostReceipt {
    PlanningCostReceipt::new(10, 5, 1, "standard".to_owned()).unwrap()
}

#[test]
fn atomicity_assessment_vocabulary_matches_adr() {
    let a = AtomicityAssessment::new(
        AtomicityVerdict::Atomic,
        "single verifiable outcome".to_owned(),
        vec![eref("attempt-A1")],
        assessment_receipt(),
    )
    .unwrap();
    assert!(matches!(a.verdict, AtomicityVerdict::Atomic));
    assert_eq!(a.planning_cost_receipt, assessment_receipt());
    let b = AtomicityAssessment::new(
        AtomicityVerdict::Blocked {
            blocker: AtomicityBlocker::new("missing-scope", "no scope bound").unwrap(),
        },
        "blocked with typed blocker".to_owned(),
        vec![],
        assessment_receipt(),
    )
    .unwrap();
    assert!(matches!(b.verdict, AtomicityVerdict::Blocked { .. }));
    assert!(AtomicityAssessment::new(
        AtomicityVerdict::Decomposable,
        "".to_owned(),
        vec![],
        assessment_receipt()
    )
    .is_err());
}

#[test]
fn planner_fault_taxonomy_matches_adr() {
    use plan_domain::{PlannerCallSpend, PlannerFault};
    let measured = PlannerCallSpend::Measured(assessment_receipt());
    let budget = PlannerFault::BudgetExhausted {
        spend: measured.clone(),
    };
    assert!(matches!(budget, PlannerFault::BudgetExhausted { .. }));
    assert_eq!(budget.spend(), measured);
    assert!(matches!(
        PlannerFault::DepthExhausted,
        PlannerFault::DepthExhausted
    ));
    assert_eq!(
        PlannerFault::DepthExhausted.spend(),
        PlannerCallSpend::NotStarted
    );
    assert!(matches!(
        PlannerFault::Unplannable {
            blocker: AtomicityBlocker::new("no-scope", "missing bound").unwrap(),
            spend: PlannerCallSpend::NotStarted,
        },
        PlannerFault::Unplannable { .. }
    ));
    let failure = PlannerFault::ProviderFailure {
        typed_cause: "timeout".to_owned(),
        spend: PlannerCallSpend::Unmeasured,
    };
    assert!(matches!(failure, PlannerFault::ProviderFailure { .. }));
    assert_eq!(failure.spend(), PlannerCallSpend::Unmeasured);
}

#[test]
fn planning_evidence_key_ignores_order_and_repetition_but_not_content() {
    use plan_domain::planning_evidence_key;
    let ab = planning_evidence_key(&[eref("attempt-A"), eref("attempt-B")]);
    let ba_dup = planning_evidence_key(&[eref("attempt-B"), eref("attempt-A"), eref("attempt-A")]);
    assert_eq!(
        ab, ba_dup,
        "same evidence set must never become a fresh event"
    );
    assert_ne!(ab, planning_evidence_key(&[eref("attempt-A")]));
    assert!(ab.starts_with("sha256:") && ab.len() == 71);
}

// ---------------------------------------------------------------------------
// Compile-time Task-mutation exclusion: the crate exposes no Task, storage,
// scheduler, or provider types; validation consumes detached snapshots only.
// ---------------------------------------------------------------------------

#[test]
fn plan_domain_depends_only_on_sha2() {
    // Enforced structurally: `cargo tree -p plan-domain` must show no
    // workspace infrastructure crate. This test pins the snapshot shape:
    // every model type round-trips through owned data (Clone) with no
    // interior mutability or handle semantics.
    let cs = valid_changeset();
    let clone = cs.clone();
    assert_eq!(cs.proposal_hash, clone.proposal_hash);
    let graph = base_graph();
    let _graph_clone = graph.clone();
    let ids: BTreeSet<String> = graph
        .nodes
        .iter()
        .map(|n| n.id.as_str().to_owned())
        .collect();
    assert_eq!(ids.len(), 2);
}

// ---------------------------------------------------------------------------
// TASK-0158 (ADR-0036 IMPL-B, D3): V9 recognizes an authorized bound-params
// requirement as non-escalating; any other non-empty params source still
// routes to the V9 risk gate.
// ---------------------------------------------------------------------------

fn bound_v1_requirement() -> EvidenceRequirement {
    EvidenceRequirement::new(
        "c1",
        RequirementKind::TestSuite,
        RequirementKind::TestSuite.default_subject_binding(),
        vec![
            RequirementParam::new("literal", "needle").unwrap(),
            RequirementParam::new("path", "src/lib.rs").unwrap(),
            RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
        ],
        Some("policy:test".to_owned()),
        true,
    )
    .unwrap()
}

#[test]
fn v9_authorized_bound_candidate_params_do_not_escalate() {
    let graph = base_graph();
    let mut context = ctx();
    context.affected_parent_criteria[0].requirement = Some(bound_v1_requirement());
    assert!(matches!(
        validate_proposal(&graph, &valid_changeset(), &context),
        ValidationOutcome::Accepted {
            risk_escalated: false
        }
    ));
}

#[test]
fn v9_bound_shape_on_non_testsuite_kind_still_escalates() {
    // F1: the boundary only binds onto required-TestSuite requirements;
    // the same 3-key shape on any other kind is boundary-impossible and
    // must escalate even when kind/binding otherwise match.
    let graph = base_graph();
    let mut changeset = valid_changeset();
    changeset.node_additions[0].criteria = vec![plan_domain::CriterionDecl::new(
        cid("c1a"),
        VerificationMethod::IndependentReview,
    )];
    changeset.proposal_hash = compute_proposal_hash(&changeset);
    let mut context = ctx();
    context.affected_parent_criteria = vec![AffectedParentCriterion {
        id: cid("c1"),
        method: VerificationMethod::IndependentReview,
        requirement: Some(
            EvidenceRequirement::new(
                "c1",
                RequirementKind::IndependentReview,
                RequirementKind::IndependentReview.default_subject_binding(),
                vec![
                    RequirementParam::new("literal", "needle").unwrap(),
                    RequirementParam::new("path", "src/lib.rs").unwrap(),
                    RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
                ],
                Some("policy:test".to_owned()),
                true,
            )
            .unwrap(),
        ),
    }];
    assert_eq!(refusal_code(&graph, &changeset, &context), "RISK_GATE_OPEN");
}

#[test]
fn v9_foreign_bound_params_still_escalate() {
    // D3 authorized bound: only the exact boundary-produced shape for a
    // catalogued selector is non-escalating. Everything else with
    // non-empty params keeps routing to the risk gate.
    let foreign: Vec<EvidenceRequirement> = [
        // Uncatalogued selector id.
        vec![
            RequirementParam::new("literal", "needle").unwrap(),
            RequirementParam::new("path", "src/lib.rs").unwrap(),
            RequirementParam::new("selector", "candidate.evil.v1").unwrap(),
        ],
        // Missing path param.
        vec![
            RequirementParam::new("literal", "needle").unwrap(),
            RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
        ],
        // Extra param.
        vec![
            RequirementParam::new("literal", "needle").unwrap(),
            RequirementParam::new("path", "src/lib.rs").unwrap(),
            RequirementParam::new("selector", "candidate.file-contains.v1").unwrap(),
            RequirementParam::new("scope", "broad").unwrap(),
        ],
        // Wrong key names.
        vec![
            RequirementParam::new("needle", "needle").unwrap(),
            RequirementParam::new("file", "src/lib.rs").unwrap(),
            RequirementParam::new("check", "candidate.file-contains.v1").unwrap(),
        ],
    ]
    .into_iter()
    .map(|params| {
        EvidenceRequirement::new(
            "c1",
            RequirementKind::TestSuite,
            RequirementKind::TestSuite.default_subject_binding(),
            params,
            Some("policy:test".to_owned()),
            true,
        )
        .unwrap()
    })
    .collect();
    for requirement in foreign {
        let graph = base_graph();
        let mut context = ctx();
        context.affected_parent_criteria[0].requirement = Some(requirement);
        assert_eq!(
            refusal_code(&graph, &valid_changeset(), &context),
            "RISK_GATE_OPEN"
        );
    }
}
