# ADR-0017 - Adaptive Planning Is Proposal-Driven and Revisioned

Status: Accepted
Date: 2026-09-09

## Context

Execution over the durable Task graph needs coordination: decomposing coarse
goals into executable work, replanning after new evidence, and reconciling
parallel or sequential efforts. A single LLM planner with direct write access
to the graph would couple durable business state to transient model behavior
and make every plan mutation unreviewable. Long-term the architecture leans
toward Kubernetes-style reconciliation (desired state → actual state →
Reconciler → next actions) rather than one omniscient planner, with adaptive
hierarchical planning as a supported strategy for just-in-time decomposition.

## Decision

The Planner is a proposal engine, never Task/graph mutation authority. It
lives behind a replaceable boundary and conceptually may assess atomicity,
propose decomposition, and propose replanning — but it never commits durable
graph changes.

A `PlanChangeSet` is a declarative, versioned proposal carrying at least:
base plan revision, reason, source evidence refs, node
additions/updates/removals, edge additions/removals, expected outcome, risk
delta, budget delta, and scope delta.

A `PlanChangeSet` becomes durable only after trusted
application/reconciliation validation preserving at least: DAG acyclicity,
valid dependency references, parent acceptance criteria, authorized scope,
Policy/human-approval requirements, budget/depth/fan-out constraints, no
duplicate/self-similar recursion, and risk-escalation requirements.
Planner self-validation is insufficient.

Every accepted structural Task/Plan graph mutation creates a versioned
`PlanRevision` with reason and provenance, plus corresponding durable Event
Journal evidence.

Planning may happen just in time and may revise only the affected branch
after new execution evidence; regenerating the entire project plan after
every failure is not required.

Adaptive hierarchical planning is accepted as a supported strategy, not as
the unconditional default: it must be benchmarked against simpler/static
planning on our own workloads before becoming the default planner strategy.

Recursive/adaptive planning requires hard liveness bounds conceptually
covering depth, fan-out, total nodes, plan mutations per reconciliation
cycle, time/token budget, no-progress, duplicate decomposition, cycle
detection, and scope/risk escalation. Exact numeric defaults remain
policy/implementation work. Planning must terminate in executable work, an
explicit blocker, or escalation — never silent infinite recursion.

Creating or decomposing child Tasks never completes the parent; parent
acceptance still requires independent verification. Task lifecycle stays
distinct from derived scheduler readiness: planning/scheduler mechanics
cannot redefine Task business lifecycle. Task kinds remain extensible
provider-neutral metadata for planning, routing, context, and evaluator
selection — no frozen enum, no provider-specific kinds. The durable
Task/Plan graph is a retrieval source for the Context Engine, never
automatically the model prompt. Planning mechanism exposes capabilities;
policy decides when replanning is allowed, budgets, risk thresholds, and
approval requirements — never hidden in provider adapters or persistence.

Explicitly left open (not decided here): final PlannerPort Rust API,
PlanChangeSet schema, PlanRevision storage format, default adaptive planner
model/provider, exact decomposition limits, scheduler algorithm, readiness
enum implementation, replanning policy, LLM-vs-deterministic planner
selection algorithm.

## Alternatives considered

- Planner with direct Task/graph write access: rejected; it violates
  bounded-context ownership and makes mutations unreviewable (INV-004,
  INV-043).
- Full upfront plan regenerated after every failure: rejected; wasteful and
  discards unaffected-branch evidence.
- Adaptive planning as unconditional default without benchmarking: rejected;
  current evidence is strong for long-form writing, not proven for software
  engineering — evidence discipline first.

## Consequences

- Scheduler/Reconciler coordinate work against validated revisions; the
  orchestrator manages work, not provider details.
- Every structural mutation is auditable, replayable, and explainable
  ("why was this task created?") via revision reason and provenance.
- Static planning remains legitimate until benchmarks say otherwise.

## Security impact

Unvalidated plan mutations are a privilege-escalation path: decomposition
may not weaken parent acceptance criteria, security policy, scope, or
human-approval requirements (INV-046), and risk/scope escalation triggers
the required approval gate. Planning proposals themselves carry no
authority.

## Migration / replacement impact

Planner, scheduler, and reconciler implementations stay replaceable behind
ports without domain rewrites; static and adaptive strategies can coexist
behind the same proposal/validation boundary.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §20 (Orchestrator), §21 (Reconciler model, Adaptive Hierarchical Planning)
- `INVARIANTS.md`: INV-002, INV-043, INV-044, INV-045, INV-046, INV-047,
  INV-048, INV-049, INV-050
