# plan-domain: derived navigation

> Generated: 2026-09-30. Source baseline: `5362f6e`.
> Derived guidance only; canonical policy: [AGENT.md](../../AGENT.md).

Package `plan-domain` holds proposal data and deterministic validation.
[Cargo.toml](Cargo.toml) depends on `task-domain` and `sha2`; the dependency
description in [lib.rs](src/lib.rs) is stale. There is no storage dependency.

## Source map

| Work | Primary source |
| --- | --- |
| Graphs, changesets, revisions, PlannerPort and spend | [model.rs](src/model.rs) |
| Task-owned scope, risk and acceptance derivation | [task_authority.rs](src/task_authority.rs) |
| Trusted input snapshot identity and fingerprint | [control.rs](src/control.rs) |
| Ordered proposal rules and typed refusals | [validation.rs](src/validation.rs) |
| Proposal hash and decomposition identity | [hash.rs](src/hash.rs) |
| Scope containment and proof-method mapping | [scope.rs](src/scope.rs), [risk.rs](src/risk.rs) |
| Bounded planning and human continuations | [guardrails.rs](src/guardrails.rs), [human_decision.rs](src/human_decision.rs) |

## Local boundaries and gotchas

- `TaskPlanningAuthority::derive` requires explicit scope, risk, acceptance and
  exactly one evidence requirement per criterion. Unknown/duplicate requirements
  and forbidden-path constraints unrepresentable by `ScopeBound` fail closed.
  Proof methods come from those requirements, not an implicit test default.
- `validate_proposal` checks well-formedness and canonical hash, then V1-V9 in
  order; its first refusal wins. V3 detects changed obligations and V9 requires
  the bound escalation grant. See [validation tests](tests/validation_rules.rs)
  and the [planning contract](../../docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md).
- A `PlannerPort` result carries proposal data, not Task mutation authority.
  [PlanningService](../app-services/src/planning_service.rs) derives trusted
  context and materializes child Tasks; [plan_store](../../adapters/storage-sqlite/src/plan_store.rs)
  commits children, accepted revision and journal atomically with publication
  fences. Input snapshot, run generation and revision remain distinct under
  [ADR-0028](../../docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md).
- Guardrail constants encode depth 4, fan-out 8, branch nodes 64 and one accepted
  revision per cycle. Two same-evidence consecutive rejections trip L3;
  per-call and cumulative spend checks live beside these constants.
- `merged_graph_has_leaf` is only a structural precheck. Durable runnable
  readiness and accepted-plan execution fences live in
  [planned_orchestration](../app-services/src/planned_orchestration.rs).
- Materialization maps proof methods into explicit child evidence requirements
  under trusted policy; the [observed-effect contract](../../docs/architecture/adr/ADR-0030-g8-observed-effect-claim-verification-contract.md)
  supersedes historical automated-test-only child restrictions. It grants no
  completion authority to a planner or to completed children.

## Focused checks

Run from the repository root; [Cargo.toml](Cargo.toml) supplies the package name
and [validation_rules.rs](tests/validation_rules.rs) is the integration target.

```sh
rtk cargo test -p plan-domain --test validation_rules
rtk cargo test -p plan-domain --lib task_authority::tests
rtk cargo test -p plan-domain --lib guardrails::tests
```
