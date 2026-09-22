//! Canonical Task-owned planning authority (TASK-0084, ADR-0028 amendment).
//!
//! The Task context owns scope paths, acceptance obligations, baseline risk
//! and parent links. G7 planning interprets those plain facts exactly once,
//! here, so trusted policy derivation, Task authoring validation and the
//! Plan publication fence cannot drift apart. Inputs are plain data; no Task
//! entity, storage handle or mutation capability crosses this boundary.

use std::collections::BTreeSet;

use crate::error::PlanDomainError;
use crate::ids::CriterionId;
use crate::risk::RiskClass;
use crate::scope::ScopeBound;
use crate::validation::AffectedParentCriterion;

/// Normalized planning authority derived from one canonical Task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskPlanningAuthority {
    pub scope_bound: ScopeBound,
    pub baseline_risk: RiskClass,
    /// Sorted by criterion id and de-duplicated.
    pub affected_parent_criteria: Vec<AffectedParentCriterion>,
}

impl TaskPlanningAuthority {
    /// Derives planning authority from canonical Task facts. Missing scope,
    /// acceptance or risk, forbidden-path constraints the G7 `ScopeBound`
    /// cannot represent, and malformed values all fail closed.
    ///
    /// Every acceptance criterion must carry exactly one explicit
    /// `EvidenceRequirement` (ADR-0030 §2/§6): the planning-side proof method
    /// is read from the requirement kind, never assumed. A criterion without a
    /// requirement, a duplicate, or a requirement naming an unknown criterion
    /// is underivable and fails closed — the TASK-0093 implicit default no
    /// longer exists on this path.
    pub fn derive(
        allowed_paths: &[String],
        forbidden_paths: &[String],
        acceptance: &[String],
        requirements: &[task_domain::EvidenceRequirement],
        risk: Option<&str>,
    ) -> Result<Self, PlanDomainError> {
        if allowed_paths.is_empty() {
            return Err(PlanDomainError::input("has no authorized scope"));
        }
        if !forbidden_paths.is_empty() {
            return Err(PlanDomainError::input(
                "has forbidden-path constraints that the G7 ScopeBound cannot represent",
            ));
        }
        if acceptance.is_empty() {
            return Err(PlanDomainError::input("has no acceptance obligations"));
        }
        let risk = risk.ok_or_else(|| PlanDomainError::input("has no baseline risk"))?;
        let baseline_risk = RiskClass::parse(risk).map_err(|error| {
            PlanDomainError::input(format!("has invalid baseline risk: {error}"))
        })?;
        let scope_bound = ScopeBound::new(allowed_paths.to_vec())?;
        let mut affected_parent_criteria = acceptance
            .iter()
            .map(|criterion| {
                let matches: Vec<&task_domain::EvidenceRequirement> = requirements
                    .iter()
                    .filter(|requirement| requirement.criterion_id == *criterion)
                    .collect();
                let requirement = match matches.as_slice() {
                    [requirement] => *requirement,
                    [] => {
                        return Err(PlanDomainError::input(format!(
                            "has no evidence requirement for criterion {criterion:?}"
                        )))
                    }
                    _ => {
                        return Err(PlanDomainError::input(format!(
                            "has multiple evidence requirements for criterion {criterion:?}"
                        )))
                    }
                };
                Ok(AffectedParentCriterion {
                    id: CriterionId::new(criterion.clone())?,
                    // ADR-0030 §3 mapping, read from explicit Task data.
                    method: crate::risk::method_for_requirement_kind(requirement.requirement_kind),
                    requirement: Some(requirement.clone()),
                })
            })
            .collect::<Result<Vec<_>, PlanDomainError>>()?;
        for requirement in requirements {
            if !acceptance.contains(&requirement.criterion_id) {
                return Err(PlanDomainError::input(format!(
                    "has an evidence requirement for unknown criterion {:?}",
                    requirement.criterion_id
                )));
            }
        }
        affected_parent_criteria.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        affected_parent_criteria.dedup_by(|a, b| a.id == b.id);
        Ok(Self {
            scope_bound,
            baseline_risk,
            affected_parent_criteria,
        })
    }
}

/// Resolves the canonical Task identities a planner may name for one owning
/// Task: the root plus every descendant reachable through Task parent links
/// of the same project. Sorted and de-duplicated.
pub fn trusted_candidate_task_refs<'a>(
    root: &str,
    parent_links: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Vec<String> {
    let links: Vec<(&str, Option<&str>)> = parent_links.into_iter().collect();
    let mut candidates = BTreeSet::from([root.to_owned()]);
    let mut frontier = BTreeSet::from([root.to_owned()]);
    while !frontier.is_empty() {
        let next: BTreeSet<String> = links
            .iter()
            .filter(|(_, parent)| parent.is_some_and(|parent| frontier.contains(parent)))
            .map(|(id, _)| (*id).to_owned())
            .filter(|id| candidates.insert(id.clone()))
            .collect();
        frontier = next;
    }
    candidates.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    fn defaults(criteria: &[&str]) -> Vec<task_domain::EvidenceRequirement> {
        criteria
            .iter()
            .map(|criterion| {
                task_domain::EvidenceRequirement::authored_default(*criterion).unwrap()
            })
            .collect()
    }

    #[test]
    fn derivation_normalizes_scope_and_criteria_order() {
        let a = TaskPlanningAuthority::derive(
            &strings(&["repo:src/./api", "repo:docs"]),
            &[],
            &strings(&["b", "a", "b"]),
            &defaults(&["a", "b"]),
            Some("medium"),
        )
        .unwrap();
        let b = TaskPlanningAuthority::derive(
            &strings(&["repo:docs", "repo:src/api"]),
            &[],
            &strings(&["a", "b"]),
            &defaults(&["a", "b"]),
            Some("medium"),
        )
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.scope_bound.roots(), vec!["repo:docs", "repo:src/api"]);
        assert_eq!(a.baseline_risk, RiskClass::Medium);
    }

    #[test]
    fn derivation_fails_closed_on_missing_or_unrepresentable_facts() {
        let scope = strings(&["repo:src"]);
        let acceptance = strings(&["criterion"]);
        let complete = defaults(&["criterion"]);
        for (allowed, forbidden, acceptance, requirements, risk) in [
            (
                vec![],
                vec![],
                acceptance.clone(),
                complete.clone(),
                Some("low"),
            ),
            (
                scope.clone(),
                strings(&["repo:src/x"]),
                acceptance.clone(),
                complete.clone(),
                Some("low"),
            ),
            (scope.clone(), vec![], vec![], vec![], Some("low")),
            (
                scope.clone(),
                vec![],
                acceptance.clone(),
                complete.clone(),
                None,
            ),
            (
                scope.clone(),
                vec![],
                acceptance.clone(),
                complete.clone(),
                Some("LOW"),
            ),
            (
                strings(&["repo:../escape"]),
                vec![],
                acceptance.clone(),
                complete.clone(),
                Some("low"),
            ),
            (
                strings(&["no-workspace"]),
                vec![],
                acceptance.clone(),
                complete.clone(),
                Some("low"),
            ),
            (
                scope.clone(),
                vec![],
                strings(&["bad\u{7}id"]),
                complete.clone(),
                Some("low"),
            ),
            // ADR-0030 §2: a criterion without its explicit requirement is
            // underivable — the implicit AutomatedTest default is gone.
            (
                scope.clone(),
                vec![],
                acceptance.clone(),
                vec![],
                Some("low"),
            ),
            // A requirement for a criterion that is not an acceptance
            // obligation is underivable.
            (
                scope.clone(),
                vec![],
                acceptance.clone(),
                defaults(&["other"]),
                Some("low"),
            ),
            // Duplicate coverage is underivable.
            (
                scope.clone(),
                vec![],
                acceptance.clone(),
                vec![
                    task_domain::EvidenceRequirement::authored_default("criterion").unwrap(),
                    task_domain::EvidenceRequirement::authored_default("criterion").unwrap(),
                ],
                Some("low"),
            ),
        ] {
            assert!(
                TaskPlanningAuthority::derive(
                    &allowed,
                    &forbidden,
                    &acceptance,
                    &requirements,
                    risk
                )
                .is_err(),
                "{allowed:?} {forbidden:?} {acceptance:?} {risk:?}"
            );
        }
    }

    /// Anchors the ADR-0030 §6 replacement of the TASK-0093 boundary: the
    /// planning-side proof method is read from the Task's explicit evidence
    /// requirement, so a strengthened obligation derives as itself instead of
    /// being refused or downgraded.
    #[test]
    fn proof_method_comes_from_the_explicit_requirement() {
        let requirements = vec![
            task_domain::EvidenceRequirement::authored_default("plain").unwrap(),
            task_domain::EvidenceRequirement::new(
                "needs-human-sign-off",
                task_domain::RequirementKind::HumanApproval,
                task_domain::RequirementKind::HumanApproval.default_subject_binding(),
                Vec::new(),
                Some("policy:v1".to_owned()),
                true,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "needs-security-check",
                task_domain::RequirementKind::SecurityCheck,
                task_domain::RequirementKind::SecurityCheck.default_subject_binding(),
                Vec::new(),
                Some("policy:v1".to_owned()),
                true,
            )
            .unwrap(),
            task_domain::EvidenceRequirement::new(
                "needs-artifact-hash",
                task_domain::RequirementKind::ArtifactHash,
                task_domain::RequirementKind::ArtifactHash.default_subject_binding(),
                Vec::new(),
                Some("policy:v1".to_owned()),
                true,
            )
            .unwrap(),
        ];
        let authority = TaskPlanningAuthority::derive(
            &strings(&["repo:src"]),
            &[],
            &strings(&[
                "plain",
                "needs-human-sign-off",
                "needs-security-check",
                "needs-artifact-hash",
            ]),
            &requirements,
            Some("critical"),
        )
        .unwrap();
        let methods: Vec<(&str, crate::risk::VerificationMethod)> = authority
            .affected_parent_criteria
            .iter()
            .map(|criterion| (criterion.id.as_str(), criterion.method))
            .collect();
        assert_eq!(
            methods,
            vec![
                (
                    "needs-artifact-hash",
                    crate::risk::VerificationMethod::AutomatedTest
                ),
                (
                    "needs-human-sign-off",
                    crate::risk::VerificationMethod::HumanApproval
                ),
                (
                    "needs-security-check",
                    crate::risk::VerificationMethod::SecurityCheck
                ),
                ("plain", crate::risk::VerificationMethod::AutomatedTest),
            ]
        );
    }

    #[test]
    fn method_and_kind_mappings_round_trip_for_the_closed_vocabulary() {
        for method in [
            crate::risk::VerificationMethod::AutomatedTest,
            crate::risk::VerificationMethod::IndependentReview,
            crate::risk::VerificationMethod::SecurityCheck,
            crate::risk::VerificationMethod::HumanApproval,
        ] {
            let kind = crate::risk::requirement_kind_for_method(method);
            assert_eq!(crate::risk::method_for_requirement_kind(kind), method);
        }
        // Capture kinds other than test-suite read back as automated-test.
        for kind in [
            task_domain::RequirementKind::ProcessExit,
            task_domain::RequirementKind::ArtifactHash,
            task_domain::RequirementKind::FsState,
            task_domain::RequirementKind::GitState,
        ] {
            assert_eq!(
                crate::risk::method_for_requirement_kind(kind),
                crate::risk::VerificationMethod::AutomatedTest
            );
        }
    }

    #[test]
    fn candidates_follow_parent_links_transitively_only() {
        let links = [
            ("child", Some("root")),
            ("grandchild", Some("child")),
            ("sibling-root", None),
            ("unrelated", Some("sibling-root")),
        ];
        assert_eq!(
            trusted_candidate_task_refs("root", links),
            vec!["child", "grandchild", "root"]
        );
    }
}
