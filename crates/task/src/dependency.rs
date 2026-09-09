//! Project dependency-graph validation: deterministic cycle detection.
//!
//! Edges are owned by [`Task`](crate::Task) (`depends_on`); this module
//! validates a proposed project task set as a whole. Dependencies
//! contribute to derived scheduling readiness only — validating or changing
//! them never mutates any task's lifecycle state, and no scheduler lives
//! here.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{DomainError, DomainResult};
use crate::task::Task;

/// Validates a proposed project task graph: every edge must be
/// project-consistent (same project on both ends — cross-project edges are
/// rejected unless the accepted contract explicitly permits them, which it
/// currently does not), and the graph must be acyclic. A dependency cycle
/// is invalid. Deterministic: tasks and edges are visited in sorted id
/// order, so the reported cycle is stable for a given input.
pub fn validate_acyclic(tasks: &[Task]) -> DomainResult<()> {
    let mut by_id: BTreeMap<&str, &Task> = BTreeMap::new();
    for task in tasks {
        by_id.insert(task.id().as_str(), task);
    }
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for task in tasks {
        let from = task.id().as_str();
        let entry = edges.entry(from).or_default();
        for dep in task.depends_on() {
            let to = dep.as_str();
            let Some(target) = by_id.get(to) else {
                return Err(DomainError::input(format!(
                    "task {from} depends on unknown task {to}"
                )));
            };
            if target.project_id() != task.project_id() {
                return Err(DomainError::ProjectMismatch(format!(
                    "task {from} (project {}) cannot depend on task {to} (project {})",
                    task.project_id(),
                    target.project_id()
                )));
            }
            entry.insert(to);
        }
    }
    // Iterative depth-first search with explicit colors; neighbors in sorted
    // order keep the first reported cycle deterministic.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Visiting,
        Done,
    }
    let mut mark: BTreeMap<&str, Mark> = BTreeMap::new();
    for from in edges.keys() {
        if mark.contains_key(from) {
            continue;
        }
        let mut stack: Vec<(&str, bool)> = vec![(from, false)];
        while let Some((node, exiting)) = stack.pop() {
            if exiting {
                mark.insert(node, Mark::Done);
                continue;
            }
            match mark.get(node) {
                Some(Mark::Done) => continue,
                Some(Mark::Visiting) => {
                    return Err(DomainError::DependencyCycle(format!(
                        "dependency cycle involving task {node}"
                    )));
                }
                None => {}
            }
            mark.insert(node, Mark::Visiting);
            stack.push((node, true));
            if let Some(neighbors) = edges.get(node) {
                // Push reversed so the smallest neighbor is visited first.
                for next in neighbors.iter().rev() {
                    match mark.get(*next) {
                        Some(Mark::Done) => {}
                        Some(Mark::Visiting) => {
                            return Err(DomainError::DependencyCycle(format!(
                                "dependency cycle involving task {next}"
                            )));
                        }
                        None => stack.push((next, false)),
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{ProjectId, TaskId};

    fn task_in(project: &str, id: &str) -> Task {
        Task::draft(
            TaskId::new(id).unwrap(),
            ProjectId::new(project).unwrap(),
            format!("objective {id}"),
        )
        .unwrap()
    }

    #[test]
    fn unrelated_tasks_are_valid() {
        let tasks = [task_in("P", "A"), task_in("P", "B")];
        assert!(validate_acyclic(&tasks).is_ok());
    }

    #[test]
    fn simple_chain_is_valid() {
        let mut a = task_in("P", "A");
        let mut b = task_in("P", "B");
        let c = task_in("P", "C");
        b.add_dependency(&c).unwrap();
        a.add_dependency(&b).unwrap();
        assert!(validate_acyclic(&[a, b, c]).is_ok());
    }

    #[test]
    fn diamond_is_valid() {
        let mut a = task_in("P", "A");
        let mut b = task_in("P", "B");
        let mut c = task_in("P", "C");
        let d = task_in("P", "D");
        b.add_dependency(&d).unwrap();
        c.add_dependency(&d).unwrap();
        a.add_dependency(&b).unwrap();
        a.add_dependency(&c).unwrap();
        assert!(validate_acyclic(&[a, b, c, d]).is_ok());
    }

    #[test]
    fn three_cycle_is_rejected() {
        // A→B, B→C, C→A. add_dependency enforces local invariants only, so
        // the cycle is built edge by edge and caught by graph validation.
        let mut a = task_in("P", "A");
        let mut b = task_in("P", "B");
        let mut c = task_in("P", "C");
        a.add_dependency(&b).unwrap();
        b.add_dependency(&c).unwrap();
        c.add_dependency(&a).unwrap();
        assert!(matches!(
            validate_acyclic(&[a, b, c]),
            Err(DomainError::DependencyCycle(_))
        ));
    }

    #[test]
    fn two_cycle_is_rejected() {
        let mut a = task_in("P", "A");
        let mut b = task_in("P", "B");
        a.add_dependency(&b).unwrap();
        b.add_dependency(&a).unwrap();
        assert!(matches!(
            validate_acyclic(&[a, b]),
            Err(DomainError::DependencyCycle(_))
        ));
    }

    #[test]
    fn unknown_dependency_target_is_rejected() {
        let ghost = task_in("P", "GHOST");
        let mut a = task_in("P", "A");
        a.add_dependency(&ghost).unwrap();
        // GHOST is not part of the proposed set.
        assert!(validate_acyclic(&[a]).is_err());
    }

    #[test]
    fn cross_project_edge_is_rejected() {
        let foreign = task_in("OTHER", "F");
        let mut a = task_in("P", "A");
        // Bypass add_dependency's own project check is impossible (it
        // rejects too); validate_acyclic re-checks for defense in depth.
        assert!(a.add_dependency(&foreign).is_err());
        assert!(validate_acyclic(&[a, foreign]).is_ok());
    }
}
