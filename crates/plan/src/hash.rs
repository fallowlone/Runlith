//! Canonical hashing: `proposal_hash` (ADR-0027 §2) and the V7
//! duplicate-decomposition fingerprint.
//!
//! Both hashes are SHA-256 over deterministic canonical bytes
//! (length-prefixed fields, sorted collections — same construction style as
//! the policy crate's fingerprints). `proposal_hash` extends the TASK-0062
//! exact-binding principle into planning: EXECUTE authorization binds to the
//! exact hash, and any material proposal mutation changes it.

use sha2::{Digest, Sha256};

use crate::model::PlanChangeSet;

fn push_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn push_str(hasher: &mut Sha256, value: &str) {
    push_field(hasher, value.as_bytes());
}

fn push_u64(hasher: &mut Sha256, value: u64) {
    hasher.update(value.to_be_bytes());
}

fn risk_name(risk: crate::risk::RiskClass) -> &'static str {
    match risk {
        crate::risk::RiskClass::Low => "low",
        crate::risk::RiskClass::Medium => "medium",
        crate::risk::RiskClass::High => "high",
        crate::risk::RiskClass::Critical => "critical",
    }
}

/// Canonical material of one node addition, sorted by node id upstream.
type CanonicalAddition<'a> = (
    &'a str,
    Vec<u8>,
    Vec<(&'a str, &'a str)>,
    &'a str,
    Option<&'a str>,
);
/// Canonical material of one node update, sorted by target id upstream.
type CanonicalGateUpdate<'a> = (&'static str, Option<&'a str>);
type CanonicalUpdate<'a> = (
    &'a str,
    Option<Vec<u8>>,
    Option<&'a str>,
    CanonicalGateUpdate<'a>,
);

/// Canonical digest of a changeset, excluding `proposal_hash` itself (the
/// hash covers material content; including it would be self-referential).
fn canonical_changeset_digest(changeset: &PlanChangeSet) -> sha2::digest::Output<Sha256> {
    let mut hasher = Sha256::new();
    push_u64(&mut hasher, changeset.base_plan_revision);
    push_str(&mut hasher, &changeset.reason);
    let mut evidence: Vec<&str> = changeset
        .source_evidence_refs
        .iter()
        .map(AsRef::as_ref)
        .collect();
    evidence.sort_unstable();
    push_u64(&mut hasher, evidence.len() as u64);
    for item in evidence {
        push_str(&mut hasher, item);
    }

    let mut additions: Vec<CanonicalAddition<'_>> = changeset
        .node_additions
        .iter()
        .map(|addition| {
            let mut criteria: Vec<(&str, &str)> = addition
                .criteria
                .iter()
                .map(|criterion| (criterion.id.as_str(), criterion.method.as_str()))
                .collect();
            criteria.sort_unstable();
            (
                addition.id.as_str(),
                addition.scope.canonical_bytes(),
                criteria,
                risk_name(addition.risk_class),
                addition.required_gate.as_ref().map(|gate| gate.as_str()),
            )
        })
        .collect();
    additions.sort_by(|a, b| a.0.cmp(b.0));
    push_u64(&mut hasher, additions.len() as u64);
    for (id, scope, criteria, risk, gate) in additions {
        push_str(&mut hasher, id);
        push_field(&mut hasher, &scope);
        push_u64(&mut hasher, criteria.len() as u64);
        for (criterion, method) in criteria {
            push_str(&mut hasher, criterion);
            push_str(&mut hasher, method);
        }
        push_str(&mut hasher, risk);
        push_str(&mut hasher, gate.unwrap_or(""));
    }

    let mut updates: Vec<CanonicalUpdate<'_>> = changeset
        .node_updates
        .iter()
        .map(|update| {
            (
                update.target.as_str(),
                update.scope.as_ref().map(|scope| scope.canonical_bytes()),
                update.risk_class.map(|risk| match risk {
                    crate::risk::RiskClass::Low => "low",
                    crate::risk::RiskClass::Medium => "medium",
                    crate::risk::RiskClass::High => "high",
                    crate::risk::RiskClass::Critical => "critical",
                }),
                match update.required_gate.as_ref() {
                    None => ("none", None),
                    Some(crate::model::GateUpdate::Set(reference)) => {
                        ("set", Some(reference.as_str()))
                    }
                    Some(crate::model::GateUpdate::Clear) => ("clear", None),
                },
            )
        })
        .collect();
    updates.sort_by(|a, b| a.0.cmp(b.0));
    push_u64(&mut hasher, updates.len() as u64);
    for (target, scope, risk, (gate_variant, gate_value)) in updates {
        push_str(&mut hasher, target);
        push_field(&mut hasher, scope.as_deref().unwrap_or(&[]));
        push_str(&mut hasher, risk.unwrap_or(""));
        push_str(&mut hasher, gate_variant);
        if let Some(value) = gate_value {
            push_str(&mut hasher, value);
        }
    }

    let mut removals: Vec<&str> = changeset.node_removals.iter().map(AsRef::as_ref).collect();
    removals.sort_unstable();
    push_u64(&mut hasher, removals.len() as u64);
    for removal in removals {
        push_str(&mut hasher, removal);
    }

    let mut edge_additions: Vec<(&str, &str)> = changeset
        .edge_additions
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    edge_additions.sort_unstable();
    push_u64(&mut hasher, edge_additions.len() as u64);
    for (from, to) in edge_additions {
        push_str(&mut hasher, from);
        push_str(&mut hasher, to);
    }

    let mut edge_removals: Vec<(&str, &str)> = changeset
        .edge_removals
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    edge_removals.sort_unstable();
    push_u64(&mut hasher, edge_removals.len() as u64);
    for (from, to) in edge_removals {
        push_str(&mut hasher, from);
        push_str(&mut hasher, to);
    }

    push_str(&mut hasher, &changeset.expected_outcome);
    let mut risk_delta: Vec<(&str, &str, &str)> = changeset
        .risk_delta
        .iter()
        .map(|delta| {
            let class = |risk| match risk {
                crate::risk::RiskClass::Low => "low",
                crate::risk::RiskClass::Medium => "medium",
                crate::risk::RiskClass::High => "high",
                crate::risk::RiskClass::Critical => "critical",
            };
            (delta.node.as_str(), class(delta.from), class(delta.to))
        })
        .collect();
    risk_delta.sort_unstable();
    push_u64(&mut hasher, risk_delta.len() as u64);
    for (node, from, to) in risk_delta {
        push_str(&mut hasher, node);
        push_str(&mut hasher, from);
        push_str(&mut hasher, to);
    }

    push_field(
        &mut hasher,
        &changeset.budget_delta.planning_cost_movement.to_be_bytes(),
    );
    push_field(
        &mut hasher,
        &changeset
            .budget_delta
            .expected_execution_cost_movement
            .to_be_bytes(),
    );
    push_field(&mut hasher, &changeset.scope_delta.canonical_bytes());

    let mut coverage: Vec<(&str, Vec<&str>)> = changeset
        .coverage_map
        .iter()
        .map(|(parent, children)| {
            let mut sorted: Vec<&str> = children.iter().map(AsRef::as_ref).collect();
            sorted.sort_unstable();
            (parent.as_str(), sorted)
        })
        .collect();
    coverage.sort_by(|a, b| a.0.cmp(b.0));
    push_u64(&mut hasher, coverage.len() as u64);
    for (parent, children) in coverage {
        push_str(&mut hasher, parent);
        push_u64(&mut hasher, children.len() as u64);
        for child in children {
            push_str(&mut hasher, child);
        }
    }

    push_str(&mut hasher, changeset.proposed_by.adapter_id.as_str());
    push_str(&mut hasher, &changeset.proposed_by.model_ref);
    push_u64(
        &mut hasher,
        u64::from(changeset.planning_cost_receipt.input_tokens),
    );
    push_u64(
        &mut hasher,
        u64::from(changeset.planning_cost_receipt.output_tokens),
    );
    push_u64(&mut hasher, changeset.planning_cost_receipt.wall_time_ms);
    push_str(&mut hasher, &changeset.planning_cost_receipt.price_class);

    hasher.finalize()
}

/// Computes `proposal_hash` (`sha256:<hex>`) over the canonical changeset
/// bytes. The proposer calls this before submitting; any material mutation
/// afterwards changes the hash and breaks exact-hash authorization binding.
#[must_use]
pub fn compute_proposal_hash(changeset: &PlanChangeSet) -> String {
    format!("sha256:{:x}", canonical_changeset_digest(changeset))
}

/// Verifies that the carried `proposal_hash` matches the canonical bytes.
#[must_use]
pub fn verify_proposal_hash(changeset: &PlanChangeSet) -> bool {
    changeset.proposal_hash == compute_proposal_hash(changeset)
}

/// V7 fingerprint: `SHA-256(canonical(parent_task_id, sorted child scope
/// roots, sorted criterion ids, sorted evidence refs))`.
#[must_use]
pub fn decomposition_fingerprint(parent_task_ref: &str, changeset: &PlanChangeSet) -> String {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"plan-decomposition-v1");
    push_str(&mut hasher, parent_task_ref);
    let mut scopes: Vec<Vec<u8>> = changeset
        .node_additions
        .iter()
        .map(|addition| addition.scope.canonical_bytes())
        .collect();
    scopes.sort();
    push_u64(&mut hasher, scopes.len() as u64);
    for scope in scopes {
        push_field(&mut hasher, &scope);
    }
    let mut criteria: Vec<&str> = changeset
        .node_additions
        .iter()
        .flat_map(|addition| {
            addition
                .criteria
                .iter()
                .map(|criterion| criterion.id.as_str())
        })
        .collect();
    criteria.sort_unstable();
    push_u64(&mut hasher, criteria.len() as u64);
    for criterion in criteria {
        push_str(&mut hasher, criterion);
    }
    let mut evidence: Vec<&str> = changeset
        .source_evidence_refs
        .iter()
        .map(AsRef::as_ref)
        .collect();
    evidence.sort_unstable();
    push_u64(&mut hasher, evidence.len() as u64);
    for item in evidence {
        push_str(&mut hasher, item);
    }
    format!("sha256:{:x}", hasher.finalize())
}
