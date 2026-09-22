//! Trusted validation service: V1–V9 applied in order (ADR-0027 §3).
//!
//! First failing rule refuses the proposal with its typed code; no partial
//! application ever persists (persistence itself is TASK-0077 — this
//! function only judges). Malformed proposals (blank reason/outcome/hash,
//! duplicate addition ids) are rejected *before* validation with
//! `MALFORMED_PROPOSAL`, per ADR-0027 §2.
//!
//! V5 compile-time half: [`PlanChangeSet`](crate::model::PlanChangeSet) has
//! no policy-rule, approval-binding, or gate-definition delta field, so a
//! planner cannot even express that weakening; V5 at runtime enforces the
//! `required_gate` annotation above the branch baseline.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::guardrails::{
    MAX_CHILDREN_PER_DECOMPOSITION, MAX_DECOMPOSITION_DEPTH, MAX_NODES_PER_PLAN_BRANCH,
    MAX_REVISIONS_PER_CYCLE_PER_BRANCH,
};
use crate::hash::{decomposition_fingerprint, verify_proposal_hash};
use crate::ids::CriterionId;
use crate::model::{GateUpdate, PlanChangeSet, PlanGraph};
use crate::risk::{RiskClass, VerificationMethod};
use crate::scope::ScopeBound;

/// One affected parent criterion: identity plus the proof method the
/// decomposition must not silently weaken (V3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AffectedParentCriterion {
    /// Parent criterion identity.
    pub id: CriterionId,
    /// Proof method the parent criterion demands.
    pub method: VerificationMethod,
    /// Canonical Task requirement, when this criterion originates from a
    /// Task. Durable plan-only criteria predate the typed Task boundary and
    /// therefore carry no comparable obligation shape.
    pub requirement: Option<task_domain::EvidenceRequirement>,
}

/// Minimal bound-grant view consumed by V9: a bound, unexpired, single-use
/// grant referencing the exact `proposal_hash`. The durable
/// `HumanDecisionRequest` lifecycle is TASK-0079; this struct is the exact
/// evidence V9 needs and nothing more.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskEscalationGrant {
    /// Exact `proposal_hash` this grant authorizes.
    pub proposal_hash: String,
    /// Expiry, milliseconds.
    pub expires_at_ms: u64,
    /// Single-use: an already-consumed grant no longer satisfies V9.
    pub consumed: bool,
}

/// Validation inputs owned by the trusted service (never by the planner).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationContext {
    /// Current branch head: `base_plan_revision` must equal this (V8).
    pub branch_head: u64,
    /// Branch authorized scope (V4).
    pub scope_bound: ScopeBound,
    /// Branch risk baseline (V5).
    pub baseline_risk: RiskClass,
    /// Parent criteria the decomposition must preserve (V3).
    pub affected_parent_criteria: Vec<AffectedParentCriterion>,
    /// Trusted policy derivation identity stamped on a materialized child.
    pub policy_fingerprint: String,
    /// Depth the merged branch would reach, in edges (V6).
    pub depth_after_merge: u32,
    /// Revisions already accepted this reconcile cycle on this branch (V6).
    pub revisions_accepted_this_cycle: u32,
    /// Parent task ref feeding the V7 fingerprint.
    pub parent_task_ref: String,
    /// Fingerprints of prior decompositions in the branch lineage (V7).
    pub lineage_fingerprints: Vec<String>,
    /// Service clock, milliseconds (V9 expiry).
    pub now_ms: u64,
    /// Bound risk-escalation grant, if one exists (V9).
    pub risk_grant: Option<RiskEscalationGrant>,
}

/// `LIMIT_EXCEEDED` detail: which bound, the bound, the observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LimitDetail {
    /// Which bound tripped: `depth`, `fan-out`, `branch-nodes`, `revisions-per-cycle`.
    pub which: &'static str,
    /// The ADR-0027 bound.
    pub bound: u64,
    /// The observed value.
    pub observed: u64,
}

/// Typed proposal refusal. Codes are stable: `MALFORMED_PROPOSAL` for
/// pre-validation rejection, then one per V-rule in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationRefusal {
    /// Pre-validation (§2): blank reason/outcome/hash or duplicate addition.
    Malformed {
        /// What is malformed.
        detail: String,
    },
    /// V1: merged graph is not a DAG.
    CyclicProposal {
        /// Where the cycle check failed.
        detail: String,
    },
    /// V2: unresolvable edge endpoint, unknown update/remove target, or a
    /// surviving edge on a removed node.
    DanglingRef {
        /// What dangles.
        detail: String,
    },
    /// V3: dropped, uncovered, or undeclared acceptance-criteria coverage.
    CriterionDropped {
        /// Which criterion is uncovered.
        detail: String,
    },
    /// V4: node or delta scope outside the branch bound.
    ScopeEscape {
        /// What escaped.
        detail: String,
    },
    /// V5: risk above baseline without a `required_gate` annotation.
    PolicyWeakening {
        /// Which node weakens policy.
        detail: String,
    },
    /// V6: a numeric liveness bound is breached.
    LimitExceeded {
        /// Which bound tripped.
        which: &'static str,
        /// The ADR-0027 bound.
        bound: u64,
        /// The observed value.
        observed: u64,
    },
    /// V7: fingerprint replays a lineage decomposition.
    DuplicateDecomposition {
        /// The replayed fingerprint.
        fingerprint: String,
    },
    /// V8: optimistic-concurrency mismatch (no silent auto-rebase).
    StaleBase {
        /// Current branch head.
        expected: u64,
        /// Proposal's claimed base.
        observed: u64,
    },
    /// V9: risk escalation without a bound, unexpired, single-use grant
    /// over the exact `proposal_hash`.
    RiskGateOpen {
        /// Why the gate stays open.
        detail: String,
    },
}

impl ValidationRefusal {
    /// Stable refusal code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Malformed { .. } => "MALFORMED_PROPOSAL",
            Self::CyclicProposal { .. } => "CYCLIC_PROPOSAL",
            Self::DanglingRef { .. } => "DANGLING_REF",
            Self::CriterionDropped { .. } => "CRITERION_DROPPED",
            Self::ScopeEscape { .. } => "SCOPE_ESCAPE",
            Self::PolicyWeakening { .. } => "POLICY_WEAKENING",
            Self::LimitExceeded { .. } => "LIMIT_EXCEEDED",
            Self::DuplicateDecomposition { .. } => "DUPLICATE_DECOMPOSITION",
            Self::StaleBase { .. } => "STALE_BASE",
            Self::RiskGateOpen { .. } => "RISK_GATE_OPEN",
        }
    }

    /// `LIMIT_EXCEEDED` detail, if this refusal is one.
    #[must_use]
    pub const fn limit_detail(&self) -> Option<LimitDetail> {
        match self {
            Self::LimitExceeded {
                which,
                bound,
                observed,
            } => Some(LimitDetail {
                which,
                bound: *bound,
                observed: *observed,
            }),
            _ => None,
        }
    }

    /// `DUPLICATE_DECOMPOSITION` fingerprint, if this refusal is one.
    #[must_use]
    pub fn duplicate_fingerprint(&self) -> Option<&str> {
        match self {
            Self::DuplicateDecomposition { fingerprint } => Some(fingerprint),
            _ => None,
        }
    }

    /// `STALE_BASE` `(expected_head, observed_base)`, if this refusal is one.
    #[must_use]
    pub const fn stale_base_detail(&self) -> Option<(u64, u64)> {
        match self {
            Self::StaleBase { expected, observed } => Some((*expected, *observed)),
            _ => None,
        }
    }
}

/// Validation verdict: acceptance reports whether a risk escalation rode
/// along on a valid grant; refusal carries the first failing rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationOutcome {
    /// Proposal is valid. `risk_escalated` is true only when V3/V9
    /// escalation was present and covered by a valid grant.
    Accepted {
        /// Whether a covered risk escalation is part of this revision.
        risk_escalated: bool,
    },
    /// Proposal refused by the first failing rule.
    Refused(ValidationRefusal),
}

/// Applies V1–V9 in order to one proposal against its base-revision
/// snapshot. Pure: no I/O, no durable writes, deterministic.
#[must_use]
pub fn validate_proposal(
    graph: &PlanGraph,
    changeset: &PlanChangeSet,
    ctx: &ValidationContext,
) -> ValidationOutcome {
    if let Err(reason) = changeset.check_wellformed() {
        return ValidationOutcome::Refused(ValidationRefusal::Malformed {
            detail: reason.to_string(),
        });
    }
    if !verify_proposal_hash(changeset) {
        return ValidationOutcome::Refused(ValidationRefusal::Malformed {
            detail: "proposal_hash does not match canonical PlanChangeSet bytes".to_owned(),
        });
    }

    // Merged vertex/edge sets. V1 runs before ref resolution per ADR order:
    // referenced-but-unknown ids are vertices here and are judged by V2.
    let mut vertices: BTreeSet<&str> = BTreeSet::new();
    for node in &graph.nodes {
        vertices.insert(node.id.as_str());
    }
    for addition in &changeset.node_additions {
        vertices.insert(addition.id.as_str());
    }
    for edge in &changeset.edge_additions {
        vertices.insert(edge.from.as_str());
        vertices.insert(edge.to.as_str());
    }
    let removed: BTreeSet<&str> = changeset.node_removals.iter().map(AsRef::as_ref).collect();
    for id in &removed {
        vertices.remove(*id);
    }
    let removed_edges: BTreeSet<(&str, &str)> = changeset
        .edge_removals
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    let mut merged_edges: BTreeSet<(&str, &str)> = graph
        .edges
        .iter()
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .filter(|edge| !removed_edges.contains(edge))
        .collect();
    for edge in &changeset.edge_additions {
        merged_edges.insert((edge.from.as_str(), edge.to.as_str()));
    }

    // V1 ACYCLICITY.
    if let Some(detail) = first_cycle(&vertices, &merged_edges) {
        return ValidationOutcome::Refused(ValidationRefusal::CyclicProposal { detail });
    }

    // V2 REFERENTIAL INTEGRITY.
    if let Some(refusal) = check_referential_integrity(graph, changeset, &merged_edges) {
        return ValidationOutcome::Refused(refusal);
    }

    // V3 ACCEPTANCE-CRITERIA PRESERVATION (flags method changes for V9).
    let method_changed = match check_criteria_preservation(changeset, ctx) {
        Ok(flag) => flag,
        Err(refusal) => return ValidationOutcome::Refused(refusal),
    };

    // V4 SCOPE CONTAINMENT.
    if let Some(refusal) = check_scope_containment(changeset, ctx) {
        return ValidationOutcome::Refused(refusal);
    }

    // V5 NO POLICY/APPROVAL WEAKENING.
    if let Some(refusal) = check_no_policy_weakening(graph, changeset, ctx) {
        return ValidationOutcome::Refused(refusal);
    }

    // V6 BUDGET/DEPTH/FAN-OUT LIMITS.
    if let Some(refusal) = check_limits(graph, changeset, ctx, vertices.len()) {
        return ValidationOutcome::Refused(refusal);
    }

    // V7 DUPLICATE-DECOMPOSITION REJECTION.
    let fingerprint = decomposition_fingerprint(&ctx.parent_task_ref, changeset);
    if ctx.lineage_fingerprints.contains(&fingerprint) {
        return ValidationOutcome::Refused(ValidationRefusal::DuplicateDecomposition {
            fingerprint,
        });
    }

    // V8 STALE BASE.
    if changeset.base_plan_revision != ctx.branch_head {
        return ValidationOutcome::Refused(ValidationRefusal::StaleBase {
            expected: ctx.branch_head,
            observed: changeset.base_plan_revision,
        });
    }

    // V9 RISK-ESCALATION APPROVAL GATE.
    let risk_escalated = method_changed
        || has_derived_update_risk_escalation(graph, changeset)
        || has_derived_addition_risk_escalation(changeset, ctx)
        || changeset
            .risk_delta
            .iter()
            .any(|delta| delta.to > delta.from);
    if risk_escalated {
        if let Some(detail) = check_risk_grant(changeset, ctx) {
            return ValidationOutcome::Refused(ValidationRefusal::RiskGateOpen { detail });
        }
    }

    ValidationOutcome::Accepted { risk_escalated }
}

/// Deterministic Kahn topological check over sorted vertices; returns a
/// description of the cyclic remainder, if any.
fn first_cycle(vertices: &BTreeSet<&str>, edges: &BTreeSet<(&str, &str)>) -> Option<String> {
    let mut indegree: BTreeMap<&str, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for vertex in vertices {
        indegree.insert(*vertex, 0);
    }
    let mut sorted_edges: Vec<(&str, &str)> = edges.iter().copied().collect();
    sorted_edges.sort_unstable();
    for (from, to) in sorted_edges {
        if !vertices.contains(from) || !vertices.contains(to) {
            continue;
        }
        outgoing.entry(from).or_default().push(to);
        *indegree.entry(to).or_default() += 1;
    }
    let mut ready: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(vertex, degree)| (*degree == 0).then_some(*vertex))
        .collect();
    let mut visited = 0_usize;
    while let Some(vertex) = ready.pop_front() {
        visited += 1;
        if let Some(next) = outgoing.get(vertex) {
            let mut sorted = next.clone();
            sorted.sort_unstable();
            for target in sorted {
                // Untracked targets are impossible here (`outgoing` only
                // contains vertices); if the invariant ever broke, fail
                // closed by reporting a cycle, never by proceeding.
                let Some(degree) = indegree.get_mut(target) else {
                    return Some(format!(
                        "merged graph is cyclic near untracked vertex {target:?}"
                    ));
                };
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(target);
                }
            }
        }
    }
    if visited == vertices.len() {
        return None;
    }
    let cyclic: Vec<&&str> = indegree
        .iter()
        .filter_map(|(vertex, degree)| (*degree > 0).then_some(vertex))
        .collect();
    Some(format!("merged graph is cyclic near {:?}", cyclic))
}

fn check_referential_integrity(
    graph: &PlanGraph,
    changeset: &PlanChangeSet,
    merged_edges: &BTreeSet<(&str, &str)>,
) -> Option<ValidationRefusal> {
    let base: BTreeSet<&str> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
    let added: BTreeSet<&str> = changeset
        .node_additions
        .iter()
        .map(|addition| addition.id.as_str())
        .collect();
    let removed: BTreeSet<&str> = changeset.node_removals.iter().map(AsRef::as_ref).collect();
    let unioned: BTreeSet<&str> = base.union(&added).copied().collect();
    let live: BTreeSet<&str> = &unioned - &removed;

    for addition in &changeset.node_additions {
        if base.contains(addition.id.as_str()) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!(
                    "addition {} collides with a base node",
                    addition.id.as_str()
                ),
            });
        }
    }
    for update in &changeset.node_updates {
        if !base.contains(update.target.as_str()) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!(
                    "update target {} is not in the base revision",
                    update.target.as_str()
                ),
            });
        }
        if removed.contains(update.target.as_str()) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!(
                    "update target {} is removed by the same changeset",
                    update.target.as_str()
                ),
            });
        }
    }
    for removal in &removed {
        if !base.contains(*removal) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!("removal target {removal} is not in the base revision"),
            });
        }
    }
    for edge in &changeset.edge_additions {
        if !live.contains(edge.from.as_str()) || !live.contains(edge.to.as_str()) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!(
                    "edge {} -> {} resolves outside (base ∪ added) ∖ removed",
                    edge.from.as_str(),
                    edge.to.as_str()
                ),
            });
        }
    }
    for edge in &changeset.edge_removals {
        let known: BTreeSet<&str> = base.union(&added).copied().collect();
        if !known.contains(edge.from.as_str()) || !known.contains(edge.to.as_str()) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!(
                    "edge removal {} -> {} references an unknown node",
                    edge.from.as_str(),
                    edge.to.as_str()
                ),
            });
        }
    }
    for (from, to) in merged_edges {
        if !live.contains(*from) || !live.contains(*to) {
            return Some(ValidationRefusal::DanglingRef {
                detail: format!(
                    "merged edge {from} -> {to} resolves outside (base ∪ added) ∖ removed"
                ),
            });
        }
    }
    None
}

/// Returns `Ok(requirement_changed)` where the flag routes to V9.
fn check_criteria_preservation(
    changeset: &PlanChangeSet,
    ctx: &ValidationContext,
) -> Result<bool, ValidationRefusal> {
    let mut declared: BTreeMap<&str, VerificationMethod> = BTreeMap::new();
    for addition in &changeset.node_additions {
        for criterion in &addition.criteria {
            declared.insert(criterion.id.as_str(), criterion.method);
        }
    }
    let mut requirement_changed = false;
    for parent in &ctx.affected_parent_criteria {
        let children = changeset.coverage_map.get(&parent.id);
        let Some(children) = children else {
            return Err(ValidationRefusal::CriterionDropped {
                detail: format!("parent criterion {} has no coverage", parent.id.as_str()),
            });
        };
        if children.is_empty() {
            return Err(ValidationRefusal::CriterionDropped {
                detail: format!(
                    "parent criterion {} covers zero children",
                    parent.id.as_str()
                ),
            });
        }
        for child in children {
            match declared.get(child.as_str()) {
                None => {
                    return Err(ValidationRefusal::CriterionDropped {
                        detail: format!(
                            "covering child {} is declared by no added node",
                            child.as_str()
                        ),
                    });
                }
                Some(method) => {
                    if *method != parent.method
                        || parent.requirement.as_ref().is_some_and(|requirement| {
                            let kind = crate::risk::requirement_kind_for_method(*method);
                            requirement.requirement_kind != kind
                                || requirement.subject_binding != kind.default_subject_binding()
                                || !requirement.params.is_empty()
                                // An authored/migrated default has no policy fingerprint. Its
                                // unchanged shape may be stamped by this trusted planning pass;
                                // `None -> current` is not an obligation escalation.
                                || requirement
                                    .policy_fingerprint
                                    .as_deref()
                                    .is_some_and(|fingerprint| {
                                        fingerprint != ctx.policy_fingerprint
                                    })
                                || !requirement.required
                        })
                    {
                        requirement_changed = true;
                    }
                }
            }
        }
    }
    Ok(requirement_changed)
}

fn check_scope_containment(
    changeset: &PlanChangeSet,
    ctx: &ValidationContext,
) -> Option<ValidationRefusal> {
    for addition in &changeset.node_additions {
        if !addition.scope.is_subset_of(&ctx.scope_bound) {
            return Some(ValidationRefusal::ScopeEscape {
                detail: format!(
                    "added node {} escapes the branch scope",
                    addition.id.as_str()
                ),
            });
        }
    }
    for update in &changeset.node_updates {
        if let Some(scope) = &update.scope {
            if !scope.is_subset_of(&ctx.scope_bound) {
                return Some(ValidationRefusal::ScopeEscape {
                    detail: format!(
                        "updated node {} escapes the branch scope",
                        update.target.as_str()
                    ),
                });
            }
        }
    }
    if !changeset.scope_delta.is_subset_of(&ctx.scope_bound) {
        return Some(ValidationRefusal::ScopeEscape {
            detail: "scope_delta escapes the branch scope".to_owned(),
        });
    }
    None
}

fn check_no_policy_weakening(
    graph: &PlanGraph,
    changeset: &PlanChangeSet,
    ctx: &ValidationContext,
) -> Option<ValidationRefusal> {
    let base: BTreeMap<&str, &crate::model::PlanNode> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    for addition in &changeset.node_additions {
        if addition.risk_class > ctx.baseline_risk && addition.required_gate.is_none() {
            return Some(ValidationRefusal::PolicyWeakening {
                detail: format!(
                    "added node {} exceeds baseline risk with no required_gate",
                    addition.id.as_str()
                ),
            });
        }
    }
    for update in &changeset.node_updates {
        let Some(node) = base.get(update.target.as_str()) else {
            continue;
        };
        let risk = update.risk_class.unwrap_or(node.risk_class);
        let gate = match &update.required_gate {
            Some(GateUpdate::Set(reference)) => Some(reference),
            Some(GateUpdate::Clear) => None,
            None => node.required_gate.as_ref(),
        };
        if risk > ctx.baseline_risk && gate.is_none() {
            return Some(ValidationRefusal::PolicyWeakening {
                detail: format!(
                    "updated node {} exceeds baseline risk with no required_gate",
                    update.target.as_str()
                ),
            });
        }
    }
    None
}

fn check_limits(
    graph: &PlanGraph,
    changeset: &PlanChangeSet,
    ctx: &ValidationContext,
    merged_vertex_count: usize,
) -> Option<ValidationRefusal> {
    if ctx.depth_after_merge > MAX_DECOMPOSITION_DEPTH {
        return Some(ValidationRefusal::LimitExceeded {
            which: "depth",
            bound: u64::from(MAX_DECOMPOSITION_DEPTH),
            observed: u64::from(ctx.depth_after_merge),
        });
    }
    if changeset.node_additions.len() > MAX_CHILDREN_PER_DECOMPOSITION {
        return Some(ValidationRefusal::LimitExceeded {
            which: "fan-out",
            bound: MAX_CHILDREN_PER_DECOMPOSITION as u64,
            observed: changeset.node_additions.len() as u64,
        });
    }
    // Total nodes in the affected branch after merge.
    let _ = graph;
    if merged_vertex_count > MAX_NODES_PER_PLAN_BRANCH {
        return Some(ValidationRefusal::LimitExceeded {
            which: "branch-nodes",
            bound: MAX_NODES_PER_PLAN_BRANCH as u64,
            observed: merged_vertex_count as u64,
        });
    }
    let observed = ctx.revisions_accepted_this_cycle + 1;
    if observed > MAX_REVISIONS_PER_CYCLE_PER_BRANCH {
        return Some(ValidationRefusal::LimitExceeded {
            which: "revisions-per-cycle",
            bound: u64::from(MAX_REVISIONS_PER_CYCLE_PER_BRANCH),
            observed: u64::from(observed),
        });
    }
    None
}

/// Derives risk escalation for proposed new nodes from the trusted branch
/// baseline. V5 already refuses an above-baseline addition that carries no
/// `required_gate`, but `GateRef` is an opaque annotation that no policy,
/// scheduler or runner consumes, and it is never carried onto the canonical
/// child Task — only the elevated risk class is. Without this check the gate
/// annotation would be the only thing between a proposal and a higher-risk
/// canonical Task, and V9 would fire only when the planner chose to declare a
/// `risk_delta`, which is planner-supplied input rather than trusted state.
fn has_derived_addition_risk_escalation(
    changeset: &PlanChangeSet,
    ctx: &ValidationContext,
) -> bool {
    changeset
        .node_additions
        .iter()
        .any(|addition| addition.risk_class > ctx.baseline_risk)
}

/// Derives risk escalation from trusted base state plus planner-proposed
/// node updates. V2 has already rejected unknown update targets before this
/// helper is consulted, so planner-declared `risk_delta` cannot suppress an
/// actual Low→High (or other upward) transition.
fn has_derived_update_risk_escalation(graph: &PlanGraph, changeset: &PlanChangeSet) -> bool {
    let base: BTreeMap<&str, RiskClass> = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.risk_class))
        .collect();
    changeset.node_updates.iter().any(|update| {
        let Some(to) = update.risk_class else {
            return false;
        };
        base.get(update.target.as_str())
            .is_some_and(|from| to > *from)
    })
}

/// Returns `Some(detail)` when the escalation lacks a valid grant.
fn check_risk_grant(changeset: &PlanChangeSet, ctx: &ValidationContext) -> Option<String> {
    let Some(grant) = &ctx.risk_grant else {
        return Some("risk escalation has no bound grant".to_owned());
    };
    if grant.consumed {
        return Some("risk grant is already consumed (single-use)".to_owned());
    }
    if ctx.now_ms > grant.expires_at_ms {
        return Some("risk grant is expired".to_owned());
    }
    if grant.proposal_hash != changeset.proposal_hash {
        return Some("risk grant does not reference the exact proposal_hash".to_owned());
    }
    None
}
