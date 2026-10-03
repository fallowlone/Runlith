# app-services: derived navigation

> Generated: 2026-09-30. Source baseline: `5362f6e`.
> Derived guidance only; canonical policy: [AGENT.md](../../AGENT.md).

Trusted service composition with concrete SQLite storage, not a pure domain
crate; dependency and export evidence: [Cargo.toml](Cargo.toml), [lib.rs](src/lib.rs).

## Source map

| Work | Primary source |
| --- | --- |
| Task lifecycle and journal evidence | [TaskService](src/task_service.rs) |
| Execution/Attempt facts | [RuntimeService](src/runtime_service.rs) |
| Admission, scheduling, independent evaluation | [OrchestrationService](src/orchestration_service.rs) |
| Canonical planning authority and child creation | [PlanningService](src/planning_service.rs) |
| Current-plan gates and predecessor readiness | [planned_orchestration](src/planned_orchestration.rs) |
| Capture intent to durable observed evidence | [ObservedEffectService](src/observed_effect_service.rs) |

## Transaction and authority boundaries

- `TaskService::mutate` validates a domain transition before calling
  [apply_lifecycle_transition](../../adapters/storage-sqlite/src/task_store.rs):
  expected-state update and journal append share one immediate transaction.
- `plan_claim` and `plan_start` only prepare transitions. The
  [entry transaction](../../adapters/storage-sqlite/src/entry_store.rs) commits
  admission facts together; see [entry_atomicity](tests/entry_atomicity.rs).
- `submit_result` stops at SUBMITTED. The orchestration service routes through
  review and verification, then calls `revalidate_before_completion` before
  `verification_succeeded`. An evaluation error leaves VERIFYING; rejection
  routes to RETRY_READY. Neither agent output nor evaluator persistence is a
  completion shortcut: [completion contract](../../docs/architecture/adr/ADR-0018-independent-evaluation.md).
- Planner calls run outside storage transactions. Acceptance materializes new
  children READY together with the revision and journal in
  [plan_store](../../adapters/storage-sqlite/src/plan_store.rs); existing child
  identities require exact materialization-retry provenance and content.
- Publication checks parent admission, input/run freshness and expected head.
  Execution admission carries accepted-head/input preconditions after checking
  predecessors; validation alone does not authorize execution. See
  [fencing contract](../../docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md).
- Observed-effect capture resolves durable attempt/workspace lineage, checks
  returned effect kinds, and requires a matching candidate assessment for test
  results before persistence; caller-supplied capture intent is not evidence.

## Focused checks

Run from the repository root; targets are existing [service](tests/service.rs),
[entry atomicity](tests/entry_atomicity.rs), and [planning failure matrix](tests/planning_failure_matrix.rs) suites.

```sh
rtk cargo test -p app-services --test service
rtk cargo test -p app-services --test entry_atomicity
rtk cargo test -p app-services --test planning_failure_matrix
```

Contract entrypoints: [Task lifecycle](../../docs/architecture/task-engine.md),
[state plus journal](../../docs/architecture/adr/ADR-0006-state-event-journal-event-sourcing.md).
