//! Product-owned planning control evidence (ADR-0028).
//!
//! These are immutable value objects. They carry no storage handle, provider
//! session, Task mutation capability, approval authority, or scheduler
//! authority. Persistence and run claiming belong to the application/storage
//! boundary; this module only defines the exact snapshot identity that those
//! layers bind to.

use sha2::{Digest, Sha256};

use crate::{
    AffectedParentCriterion, EvidenceRef, PlanBranchId, PlanDomainError, PlanningInputSnapshotId,
    RiskClass, RiskEscalationGrant, ScopeBound,
};

/// Immutable trusted inputs for one planning decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningInputSnapshot {
    pub id: PlanningInputSnapshotId,
    pub plan_branch_id: PlanBranchId,
    pub task_ref: String,
    pub expected_plan_head: u64,
    pub scope_bound: ScopeBound,
    pub baseline_risk: RiskClass,
    pub affected_parent_criteria: Vec<AffectedParentCriterion>,
    pub triggering_evidence: Vec<EvidenceRef>,
    pub lineage_fingerprints: Vec<String>,
    pub risk_grant: Option<RiskEscalationGrant>,
    pub policy_fingerprint: String,
    pub input_fingerprint: String,
}

impl PlanningInputSnapshot {
    /// Builds the canonical immutable snapshot. Set-like inputs are sorted and
    /// de-duplicated before hashing so caller ordering cannot create a false
    /// freshness change.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        plan_branch_id: PlanBranchId,
        task_ref: String,
        expected_plan_head: u64,
        scope_bound: ScopeBound,
        baseline_risk: RiskClass,
        mut affected_parent_criteria: Vec<AffectedParentCriterion>,
        mut triggering_evidence: Vec<EvidenceRef>,
        mut lineage_fingerprints: Vec<String>,
        risk_grant: Option<RiskEscalationGrant>,
        policy_fingerprint: String,
    ) -> Result<Self, PlanDomainError> {
        if task_ref.trim().is_empty() || task_ref.chars().any(char::is_control) {
            return Err(PlanDomainError::InvalidInput(
                "planning input task_ref must be non-blank with no control characters".to_owned(),
            ));
        }
        if policy_fingerprint.trim().is_empty() || policy_fingerprint.chars().any(char::is_control)
        {
            return Err(PlanDomainError::InvalidInput(
                "planning input policy_fingerprint must be non-blank with no control characters"
                    .to_owned(),
            ));
        }
        if triggering_evidence.is_empty() {
            return Err(PlanDomainError::InvalidInput(
                "planning input requires triggering evidence".to_owned(),
            ));
        }

        affected_parent_criteria.sort_by(|a, b| {
            (a.id.as_str(), a.method.as_str()).cmp(&(b.id.as_str(), b.method.as_str()))
        });
        affected_parent_criteria.dedup_by(|a, b| a.id == b.id && a.method == b.method);
        triggering_evidence.sort();
        triggering_evidence.dedup();
        lineage_fingerprints.sort();
        lineage_fingerprints.dedup();
        for fingerprint in &lineage_fingerprints {
            require_sha256("lineage fingerprint", fingerprint)?;
        }
        if let Some(grant) = &risk_grant {
            require_sha256("risk grant proposal hash", &grant.proposal_hash)?;
        }

        let input_fingerprint = fingerprint(
            &plan_branch_id,
            &task_ref,
            expected_plan_head,
            &scope_bound,
            baseline_risk,
            &affected_parent_criteria,
            &triggering_evidence,
            &lineage_fingerprints,
            risk_grant.as_ref(),
            &policy_fingerprint,
        );
        let id = PlanningInputSnapshotId::new(format!(
            "pis:{}",
            input_fingerprint
                .strip_prefix("sha256:")
                .expect("canonical fingerprint prefix")
        ))?;

        Ok(Self {
            id,
            plan_branch_id,
            task_ref,
            expected_plan_head,
            scope_bound,
            baseline_risk,
            affected_parent_criteria,
            triggering_evidence,
            lineage_fingerprints,
            risk_grant,
            policy_fingerprint,
            input_fingerprint,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn fingerprint(
    branch: &PlanBranchId,
    task_ref: &str,
    expected_plan_head: u64,
    scope_bound: &ScopeBound,
    baseline_risk: RiskClass,
    criteria: &[AffectedParentCriterion],
    evidence: &[EvidenceRef],
    lineage: &[String],
    risk_grant: Option<&RiskEscalationGrant>,
    policy_fingerprint: &str,
) -> String {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"runlith-planning-input-v1");
    push_str(&mut hasher, branch.as_str());
    push_str(&mut hasher, task_ref);
    hasher.update(expected_plan_head.to_be_bytes());
    let roots = scope_bound.roots();
    hasher.update((roots.len() as u64).to_be_bytes());
    for root in roots {
        push_str(&mut hasher, &root);
    }
    push_str(&mut hasher, risk_name(baseline_risk));
    hasher.update((criteria.len() as u64).to_be_bytes());
    for criterion in criteria {
        push_str(&mut hasher, criterion.id.as_str());
        push_str(&mut hasher, criterion.method.as_str());
    }
    hasher.update((evidence.len() as u64).to_be_bytes());
    for item in evidence {
        push_str(&mut hasher, item.as_str());
    }
    hasher.update((lineage.len() as u64).to_be_bytes());
    for item in lineage {
        push_str(&mut hasher, item);
    }
    match risk_grant {
        Some(grant) => {
            hasher.update([1]);
            push_str(&mut hasher, &grant.proposal_hash);
            hasher.update(grant.expires_at_ms.to_be_bytes());
            hasher.update([u8::from(grant.consumed)]);
        }
        None => hasher.update([0]),
    }
    push_str(&mut hasher, policy_fingerprint);
    format!("sha256:{:x}", hasher.finalize())
}

fn push_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn push_str(hasher: &mut Sha256, value: &str) {
    push_field(hasher, value.as_bytes());
}

fn risk_name(risk: RiskClass) -> &'static str {
    match risk {
        RiskClass::Low => "low",
        RiskClass::Medium => "medium",
        RiskClass::High => "high",
        RiskClass::Critical => "critical",
    }
}

fn require_sha256(field: &str, value: &str) -> Result<(), PlanDomainError> {
    let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(PlanDomainError::InvalidInput(format!(
            "{field} must use canonical sha256:<hex> format"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CriterionId, VerificationMethod};

    fn snapshot(evidence: Vec<&str>) -> PlanningInputSnapshot {
        PlanningInputSnapshot::new(
            PlanBranchId::new("branch-a").unwrap(),
            "TASK-1".to_owned(),
            3,
            ScopeBound::new(vec!["repo:src".to_owned()]).unwrap(),
            RiskClass::Low,
            vec![AffectedParentCriterion {
                id: CriterionId::new("criterion-a").unwrap(),
                method: VerificationMethod::AutomatedTest,
            }],
            evidence
                .into_iter()
                .map(|value| EvidenceRef::new(value).unwrap())
                .collect(),
            vec![],
            None,
            "policy:v1".to_owned(),
        )
        .unwrap()
    }

    #[test]
    fn snapshot_identity_is_order_independent_but_input_sensitive() {
        let a = snapshot(vec!["attempt:b", "attempt:a", "attempt:a"]);
        let b = snapshot(vec!["attempt:a", "attempt:b"]);
        assert_eq!(a.id, b.id);
        assert_eq!(a.input_fingerprint, b.input_fingerprint);

        let mut changed = snapshot(vec!["attempt:a", "attempt:b"]);
        changed.policy_fingerprint = "policy:v2".to_owned();
        let c = PlanningInputSnapshot::new(
            changed.plan_branch_id,
            changed.task_ref,
            changed.expected_plan_head,
            changed.scope_bound,
            changed.baseline_risk,
            changed.affected_parent_criteria,
            changed.triggering_evidence,
            changed.lineage_fingerprints,
            changed.risk_grant,
            changed.policy_fingerprint,
        )
        .unwrap();
        assert_ne!(a.input_fingerprint, c.input_fingerprint);
    }
}
