# Implementation Knowledge Queue

This file contains only evidence-backed Runlith deltas discovered through external research. It is not the canonical TASK queue. `ALREADY_DESIGNED`, `ALREADY_IMPLEMENTED`, `SUPERSEDED` and rejected ideas do not belong here.

## IKQ-001 — Generic Reconciler work ownership and stale-writer fencing

**id:** `IKQ-001`

**origin_repository:** `https://github.com/Nitjsefnie/Overflow`

**origin_revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**idea:** Define a source-neutral durable work-ownership contract for future reconciliation of external lifecycle signals: bounded claim/lease renewal, retry/defer semantics, generation/version, idempotent recovery, and stale-writer fencing on every authoritative publication.

**novelty:** Runlith already owns Task readiness/recovery in Scheduler/Reconciler, external signals are already evidence routed to Journal/Reconciler, and G4/G5 already implement narrow workspace/execution reconciliation. The unresolved delta is the generic durable worker/delivery/concurrent-writer contract for external lifecycle signals.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- External lifecycle signals are observations/evidence and feed Event Journal/Reconciler.
- Task lifecycle assigns claim/retry/readiness transitions to Scheduler/Reconciler through the Task Application Service.
- Workspace and pre-attempt execution crash recovery are implemented with explicit ownership/reconciliation semantics.
- Event architecture explicitly leaves exact external-signal mappings, delivery semantics and concurrent-writer mechanism open.

**files:**

- `docs/architecture/events.md`
- `docs/architecture/task-engine.md`
- `docs/architecture/workspace.md`
- `docs/architecture/providers.md`

**adrs:**

- `ADR-0006` State + Event Journal
- `ADR-0018` Independent Evaluation
- `ADR-0023` Workspace Ownership and Git Worktree Lifecycle
- `ADR-0025` G5 Post-Admission Recovery

**invariants:**

- `INV-001`
- `INV-066`
- `INV-070`
- `INV-091`
- `INV-145`

**tasks:** existing narrow implementation/recovery evidence includes `TASK-0041/0042/0043` and `TASK-0054/0056/0057/0058`; no canonical TASK exists or is proposed here for a generic external-signal reconciler contract.

**delta:** No accepted generic contract currently defines normalized external reconciliation work identity, durable claim generation, lease expiry/reclaim, retry/defer budgets, stale-writer fencing, or the publication condition that prevents a superseded worker from mutating authoritative state. Overflow demonstrates useful bounded lease/retry mechanics but explicitly documents that its described mechanisms do not fence writes after session loss (`src/lib/fold/reconciliation-worker.ts:127-134`).

**decision:** `ADAPT`

**target_subsystem:** `Scheduler / Reconciler`, with Event Journal/external-signal ingestion and application-service publication boundaries.

**priority:** `MEDIUM`

**architecture_impact:** High when the first active gate requires general external lifecycle reconciliation; low immediate impact because current G4/G5 recovery is already scoped and functional.

**security_impact:** A stale worker must not retain mutation authority after claim generation/lease ownership moves. The fence must be checked at the authoritative publication boundary, not only during queue bookkeeping.

**dependencies:**

- An active product/gate requirement for generic external lifecycle reconciliation.
- Event identity/provenance contract for the relevant external signal source.
- Existing Task/Application Service authority must remain the only route for canonical Task mutation.

**proposed_gate:** `UNASSIGNED` — assign only when a concrete external-lifecycle/reconciler feature enters an active gate.

**requires_adr:** `yes`

**candidate_task:** `none before ADR`. After an accepted ADR and active gate exist, derive the smallest implementation task from the decided contract rather than copying Overflow's worker.

**verification_direction:**

- Two workers race on the same reconciliation key; exactly one generation can publish canonical state.
- Lease expiry/reclaim while the first worker is still running; stale publication fails closed.
- Crash after claim, during evidence fetch, and immediately before/after publication; restart converges without blind side-effect replay.
- Retry/defer budget is finite and observable.
- Duplicate external deliveries coalesce/idempotently rederive without granting authority from delivery alone.
- Event Journal records/correlates evidence and outcomes without becoming a queue authority or side-effect replay log.

**one_percent_improvement:** Require durable stale-writer fencing from the first Runlith contract. This is a concrete reliability/security improvement over the reference behavior and preserves Runlith's product differentiation around durable, auditable recovery rather than merely retrying background jobs.

## Removed by the 2026-09-15 re-audit

The previous queue entries below were deleted because overlap analysis disproved novelty:

- Mercury "Capability Execution Evidence Contract" → `ALREADY_DESIGNED` by `INV-072` and Event/Provider receipt architecture → `EXTERNAL_VALIDATION`.
- Mercury "Skill Metadata Discovery Index" → `ALREADY_DESIGNED` by Context Engine progressive disclosure, `INV-028`, `SkillRegistryPort` target → `EXTERNAL_VALIDATION`.
- Overflow "Evidence-backed Task Completion" → `ALREADY_IMPLEMENTED` semantically by ADR-0018 + completion authority tests → `EXTERNAL_VALIDATION`.
- Overflow generic "Reconciliation Workflow for External Integrations" → too broad; existing Runlith reconciliation/recovery already covers substantial semantics. Only the narrower fencing/delivery gap above remains, and it is `NEEDS_ADR`.
