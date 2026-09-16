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
use crate::risk::{RiskClass, VerificationMethod};
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
    pub fn derive(
        allowed_paths: &[String],
        forbidden_paths: &[String],
        acceptance: &[String],
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
                Ok(AffectedParentCriterion {
                    id: CriterionId::new(criterion.clone())?,
                    // G7 v1 has no criterion-to-proof-method field in the Task
                    // contract; the product-owned v1 default lives here only.
                    method: VerificationMethod::AutomatedTest,
                })
            })
            .collect::<Result<Vec<_>, PlanDomainError>>()?;
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

    #[test]
    fn derivation_normalizes_scope_and_criteria_order() {
        let a = TaskPlanningAuthority::derive(
            &strings(&["repo:src/./api", "repo:docs"]),
            &[],
            &strings(&["b", "a", "b"]),
            Some("medium"),
        )
        .unwrap();
        let b = TaskPlanningAuthority::derive(
            &strings(&["repo:docs", "repo:src/api"]),
            &[],
            &strings(&["a", "b"]),
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
        for (allowed, forbidden, acceptance, risk) in [
            (vec![], vec![], acceptance.clone(), Some("low")),
            (
                scope.clone(),
                strings(&["repo:src/x"]),
                acceptance.clone(),
                Some("low"),
            ),
            (scope.clone(), vec![], vec![], Some("low")),
            (scope.clone(), vec![], acceptance.clone(), None),
            (scope.clone(), vec![], acceptance.clone(), Some("LOW")),
            (
                strings(&["repo:../escape"]),
                vec![],
                acceptance.clone(),
                Some("low"),
            ),
            (
                strings(&["no-workspace"]),
                vec![],
                acceptance.clone(),
                Some("low"),
            ),
            (scope.clone(), vec![], strings(&["bad\u{7}id"]), Some("low")),
        ] {
            assert!(
                TaskPlanningAuthority::derive(&allowed, &forbidden, &acceptance, risk).is_err(),
                "{allowed:?} {forbidden:?} {acceptance:?} {risk:?}"
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
