//! Detached Plan graph computations for the trusted planning service
//! (TASK-0078): depth, validated changeset materialization, and the
//! cross-domain applicability / ADR-0027 L4 checks that resolve a candidate
//! graph against canonical Task state before durable acceptance.
//!
//! Plan node ids name canonical Tasks. Task knowledge stays at this
//! application boundary; the pure plan-domain crate never sees Task types.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use plan_domain::{
    GateUpdate, PlanChangeSet, PlanDomainError, PlanGraph, PlanNode, PlanNodeId,
    MAX_DECOMPOSITION_DEPTH,
};
use scheduler::ScheduleReadiness;
use storage_sqlite::{SqliteStore, StorageError};
use task_domain::{TaskId, TaskState};

use crate::orchestration_service::derive_task_readiness;
use crate::planning_service::PlanApplicabilityRefusal;

pub(crate) fn remaining_depth(graph: &PlanGraph) -> u32 {
    MAX_DECOMPOSITION_DEPTH.saturating_sub(graph_depth(graph))
}

fn graph_depth(graph: &PlanGraph) -> u32 {
    graph_depth_from_parts(
        graph.nodes.iter().map(|node| node.id.as_str()),
        graph
            .edges
            .iter()
            .map(|(from, to)| (from.as_str(), to.as_str())),
    )
}

pub(crate) fn merged_depth(graph: &PlanGraph, proposal: &PlanChangeSet) -> u32 {
    let removed_nodes: BTreeSet<&str> = proposal
        .node_removals
        .iter()
        .map(|node| node.as_str())
        .collect();
    let removed_edges: BTreeSet<(&str, &str)> = proposal
        .edge_removals
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    let mut nodes: BTreeSet<String> = graph
        .nodes
        .iter()
        .map(|node| node.id.as_str().to_owned())
        .chain(
            proposal
                .node_additions
                .iter()
                .map(|node| node.id.as_str().to_owned()),
        )
        .collect();
    for edge in &proposal.edge_additions {
        nodes.insert(edge.from.as_str().to_owned());
        nodes.insert(edge.to.as_str().to_owned());
    }
    nodes.retain(|node| !removed_nodes.contains(node.as_str()));
    let mut edges: Vec<(String, String)> = graph
        .edges
        .iter()
        .filter(|(from, to)| !removed_edges.contains(&(from.as_str(), to.as_str())))
        .map(|(from, to)| (from.as_str().to_owned(), to.as_str().to_owned()))
        .collect();
    edges.extend(
        proposal
            .edge_additions
            .iter()
            .map(|edge| (edge.from.as_str().to_owned(), edge.to.as_str().to_owned())),
    );
    graph_depth_from_parts(
        nodes.iter().map(String::as_str),
        edges.iter().map(|(from, to)| (from.as_str(), to.as_str())),
    )
}

fn graph_depth_from_parts<'a>(
    nodes: impl Iterator<Item = &'a str>,
    edges: impl Iterator<Item = (&'a str, &'a str)>,
) -> u32 {
    let nodes: BTreeSet<&str> = nodes.collect();
    if nodes.is_empty() {
        return 0;
    }
    let mut indegree: BTreeMap<&str, usize> = nodes.iter().map(|node| (*node, 0)).collect();
    let mut outgoing: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in edges {
        if nodes.contains(from) && nodes.contains(to) {
            outgoing.entry(from).or_default().push(to);
            *indegree.entry(to).or_default() += 1;
        }
    }
    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect();
    let mut depth: BTreeMap<&str, u32> = nodes.iter().map(|node| (*node, 0)).collect();
    let mut visited = 0_usize;
    while let Some(node) = queue.pop_front() {
        visited += 1;
        let current = *depth.get(node).unwrap_or(&0);
        if let Some(next) = outgoing.get(node) {
            for target in next {
                let candidate = current.saturating_add(1);
                let entry = depth.entry(*target).or_insert(0);
                *entry = (*entry).max(candidate);
                if let Some(degree) = indegree.get_mut(target) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(target);
                    }
                }
            }
        }
    }
    if visited != nodes.len() {
        return u32::MAX;
    }
    depth.values().copied().max().unwrap_or(0)
}

/// Materializes the detached candidate graph of an already V1-V9-validated
/// changeset. Nothing durable is touched.
pub(crate) fn apply_validated_changeset(
    graph: &PlanGraph,
    proposal: &PlanChangeSet,
) -> Result<PlanGraph, PlanDomainError> {
    let removed: BTreeSet<&str> = proposal
        .node_removals
        .iter()
        .map(|node| node.as_str())
        .collect();
    let mut nodes: BTreeMap<String, PlanNode> = graph
        .nodes
        .iter()
        .filter(|node| !removed.contains(node.id.as_str()))
        .map(|node| (node.id.as_str().to_owned(), node.clone()))
        .collect();
    for update in &proposal.node_updates {
        let node = nodes.get_mut(update.target.as_str()).ok_or_else(|| {
            PlanDomainError::InvalidInput(format!(
                "validated changeset update target {} is not live after removals",
                update.target.as_str()
            ))
        })?;
        if let Some(scope) = &update.scope {
            node.scope = scope.clone();
        }
        if let Some(risk) = update.risk_class {
            node.risk_class = risk;
        }
        match &update.required_gate {
            Some(GateUpdate::Set(gate)) => node.required_gate = Some(gate.clone()),
            Some(GateUpdate::Clear) => node.required_gate = None,
            None => {}
        }
    }
    for addition in &proposal.node_additions {
        let node = PlanNode::new(
            addition.id.clone(),
            addition.scope.clone(),
            addition.criteria.clone(),
            addition.risk_class,
            addition.required_gate.clone(),
        )?;
        nodes.insert(addition.id.as_str().to_owned(), node);
    }

    let removed_edges: BTreeSet<(&str, &str)> = proposal
        .edge_removals
        .iter()
        .map(|edge| (edge.from.as_str(), edge.to.as_str()))
        .collect();
    let mut edges = graph
        .edges
        .iter()
        .filter(|(from, to)| !removed_edges.contains(&(from.as_str(), to.as_str())))
        .cloned()
        .collect::<Vec<_>>();
    for addition in &proposal.edge_additions {
        if !edges
            .iter()
            .any(|(from, to)| from == &addition.from && to == &addition.to)
        {
            edges.push((addition.from.clone(), addition.to.clone()));
        }
    }
    edges.sort_by(|(left_from, left_to), (right_from, right_to)| {
        (left_from.as_str(), left_to.as_str()).cmp(&(right_from.as_str(), right_to.as_str()))
    });
    PlanGraph::new(nodes.into_values().collect(), edges)
}

/// Plan predecessors of `node`: every `from` of an edge `from -> node`
/// (`from` MUST complete before `node` may run).
pub(crate) fn plan_predecessors<'a>(graph: &'a PlanGraph, node: &str) -> Vec<&'a PlanNodeId> {
    graph
        .edges
        .iter()
        .filter(|(_, to)| to.as_str() == node)
        .map(|(from, _)| from)
        .collect()
}

/// Applicability of a candidate graph to one selected Task, judged against
/// canonical Task state before durable acceptance. `None` means applicable.
pub(crate) fn target_applicability(
    store: &SqliteStore,
    candidate: &PlanGraph,
    task_id: &TaskId,
) -> Result<Option<PlanApplicabilityRefusal>, StorageError> {
    if !candidate
        .nodes
        .iter()
        .any(|node| node.id.as_str() == task_id.as_str())
    {
        return Ok(Some(PlanApplicabilityRefusal::TaskNotInPlan {
            task_id: task_id.clone(),
        }));
    }
    if store.get_task(task_id)?.is_none() {
        return Ok(Some(PlanApplicabilityRefusal::TaskMissing {
            task_id: task_id.clone(),
        }));
    }
    for predecessor in plan_predecessors(candidate, task_id.as_str()) {
        if canonical_task(store, predecessor)?.is_none() {
            return Ok(Some(PlanApplicabilityRefusal::PredecessorWithoutTask {
                node: predecessor.as_str().to_owned(),
            }));
        }
    }
    Ok(None)
}

/// ADR-0027 L4 at the product boundary: the candidate branch leaves at least
/// one node that will be executable-backed once the accept-path commit lands:
/// either a canonical Task whose derived readiness is RUNNABLE today, or a
/// validated new child in `pending_materializations` whose canonical Task the
/// same atomic revision commit creates (plan revision, child rows and journal
/// evidence share one transaction, so the representation is guaranteed before
/// the revision becomes executable). A structurally valid DAG alone never
/// satisfies this.
///
/// `pending_materializations` holds candidate node ids that trusted identity
/// resolution already proved to be brand-new Tasks. Plan predecessors of any
/// counted node must still be canonical COMPLETED Tasks — a pending child
/// behind an incomplete predecessor does not count. Scheduling itself still
/// derives readiness from durable Task state (DRAFT is never runnable), so
/// this admits representation, not premature execution.
pub(crate) fn has_runnable_node(
    store: &SqliteStore,
    candidate: &PlanGraph,
    pending_materializations: &BTreeSet<String>,
) -> Result<bool, StorageError> {
    'nodes: for node in &candidate.nodes {
        if !pending_materializations.contains(node.id.as_str()) {
            let Ok(task_id) = TaskId::new(node.id.as_str().to_owned()) else {
                continue;
            };
            if derive_task_readiness(store, &task_id)? != Some(ScheduleReadiness::Runnable) {
                continue;
            }
        }
        for predecessor in plan_predecessors(candidate, node.id.as_str()) {
            let completed = canonical_task(store, predecessor)?
                .is_some_and(|task| task.state() == TaskState::Completed);
            if !completed {
                continue 'nodes;
            }
        }
        return Ok(true);
    }
    Ok(false)
}

fn canonical_task(
    store: &SqliteStore,
    node: &PlanNodeId,
) -> Result<Option<task_domain::Task>, StorageError> {
    match TaskId::new(node.as_str().to_owned()) {
        Ok(task_id) => store.get_task(&task_id),
        Err(_) => Ok(None),
    }
}
