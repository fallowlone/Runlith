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
    /// Canonical Task identities that Runlith authorized for this planning
    /// scope. This is trusted application input, never planner output.
    pub trusted_candidate_task_refs: Vec<String>,
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
        affected_parent_criteria: Vec<AffectedParentCriterion>,
        triggering_evidence: Vec<EvidenceRef>,
        lineage_fingerprints: Vec<String>,
        risk_grant: Option<RiskEscalationGrant>,
        policy_fingerprint: String,
    ) -> Result<Self, PlanDomainError> {
        Self::new_with_candidates(
            plan_branch_id,
            task_ref,
            Vec::new(),
            expected_plan_head,
            scope_bound,
            baseline_risk,
            affected_parent_criteria,
            triggering_evidence,
            lineage_fingerprints,
            risk_grant,
            policy_fingerprint,
        )
    }

    /// Builds a snapshot whose planning identities are derived by a trusted
    /// application service. The pure planning domain treats these as opaque
    /// references; it never resolves or mints canonical Tasks.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_candidates(
        plan_branch_id: PlanBranchId,
        task_ref: String,
        mut trusted_candidate_task_refs: Vec<String>,
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
            (
                a.id.as_str(),
                a.method.as_str(),
                requirement_identity(a.requirement.as_ref()),
            )
                .cmp(&(
                    b.id.as_str(),
                    b.method.as_str(),
                    requirement_identity(b.requirement.as_ref()),
                ))
        });
        affected_parent_criteria.dedup();
        triggering_evidence.sort();
        triggering_evidence.dedup();
        lineage_fingerprints.sort();
        lineage_fingerprints.dedup();
        trusted_candidate_task_refs.sort();
        trusted_candidate_task_refs.dedup();
        if trusted_candidate_task_refs
            .iter()
            .any(|task_ref| task_ref.trim().is_empty() || task_ref.chars().any(char::is_control))
        {
            return Err(PlanDomainError::InvalidInput(
                "trusted candidate task refs must be non-blank with no control characters"
                    .to_owned(),
            ));
        }
        for fingerprint in &lineage_fingerprints {
            require_sha256("lineage fingerprint", fingerprint)?;
        }
        if let Some(grant) = &risk_grant {
            require_sha256("risk grant proposal hash", &grant.proposal_hash)?;
        }

        let input_fingerprint = fingerprint(
            &plan_branch_id,
            &task_ref,
            &trusted_candidate_task_refs,
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
            trusted_candidate_task_refs,
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
    candidate_task_refs: &[String],
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
    hasher.update((candidate_task_refs.len() as u64).to_be_bytes());
    for task_ref in candidate_task_refs {
        push_str(&mut hasher, task_ref);
    }
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
        push_str(
            &mut hasher,
            &requirement_identity(criterion.requirement.as_ref()),
        );
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

/// Stable full-fidelity representation of the optional canonical Task proof
/// obligation carried by a planning criterion. Values are length-delimited so
/// planner input identity cannot collide on parameter boundaries.
pub fn requirement_identity(requirement: Option<&task_domain::EvidenceRequirement>) -> String {
    let mut identity = String::new();
    let mut push = |value: &str| identity.push_str(&format!("{}:{value}", value.len()));
    match requirement {
        None => push("none"),
        Some(requirement) => {
            push("some");
            push(&requirement.criterion_id);
            push(requirement.requirement_kind.as_str());
            push(requirement.subject_binding.as_str());
            push(if requirement.required { "1" } else { "0" });
            push(requirement.policy_fingerprint.as_deref().unwrap_or(""));
            for parameter in &requirement.params {
                push(parameter.key());
                push(parameter.value());
            }
        }
    }
    identity
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
                requirement: None,
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

    #[test]
    fn snapshot_identity_binds_the_full_typed_requirement() {
        let base = snapshot(vec!["attempt:a"]);
        let requirement = task_domain::EvidenceRequirement::new(
            "criterion-a",
            task_domain::RequirementKind::ArtifactHash,
            task_domain::SubjectBinding::Content,
            vec![task_domain::RequirementParam::new("path", "dist/app").unwrap()],
            Some("policy:requirement-a".to_owned()),
            true,
        )
        .unwrap();
        let typed = PlanningInputSnapshot::new(
            base.plan_branch_id.clone(),
            base.task_ref.clone(),
            base.expected_plan_head,
            base.scope_bound.clone(),
            base.baseline_risk,
            vec![AffectedParentCriterion {
                id: CriterionId::new("criterion-a").unwrap(),
                method: VerificationMethod::AutomatedTest,
                requirement: Some(requirement.clone()),
            }],
            base.triggering_evidence.clone(),
            vec![],
            None,
            base.policy_fingerprint.clone(),
        )
        .unwrap();
        for changed in [
            task_domain::EvidenceRequirement::new(
                "criterion-a",
                task_domain::RequirementKind::FsState,
                task_domain::SubjectBinding::Content,
                requirement.params.clone(),
                requirement.policy_fingerprint.clone(),
                true,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "criterion-a",
                requirement.requirement_kind,
                task_domain::SubjectBinding::ContentAndToolchain,
                requirement.params.clone(),
                requirement.policy_fingerprint.clone(),
                true,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "criterion-a",
                requirement.requirement_kind,
                requirement.subject_binding,
                vec![task_domain::RequirementParam::new("path", "dist/other").unwrap()],
                requirement.policy_fingerprint.clone(),
                true,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "criterion-a",
                requirement.requirement_kind,
                requirement.subject_binding,
                requirement.params.clone(),
                requirement.policy_fingerprint.clone(),
                false,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "criterion-a",
                requirement.requirement_kind,
                requirement.subject_binding,
                requirement.params.clone(),
                Some("policy:requirement-b".to_owned()),
                true,
            )
            .unwrap(),
        ] {
            let candidate = PlanningInputSnapshot::new(
                base.plan_branch_id.clone(),
                base.task_ref.clone(),
                base.expected_plan_head,
                base.scope_bound.clone(),
                base.baseline_risk,
                vec![AffectedParentCriterion {
                    id: CriterionId::new("criterion-a").unwrap(),
                    method: VerificationMethod::AutomatedTest,
                    requirement: Some(changed),
                }],
                base.triggering_evidence.clone(),
                vec![],
                None,
                base.policy_fingerprint.clone(),
            )
            .unwrap();
            assert_ne!(typed.input_fingerprint, candidate.input_fingerprint);
        }
    }
}
