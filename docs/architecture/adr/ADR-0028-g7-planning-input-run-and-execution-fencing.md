# ADR-0028 - G7 Planning Input, Planning Run, and Execution Fencing

Status: Accepted
Date: 2026-09-16

## Context

TASK-0078 materializes ADR-0027's real planner and scheduler integration. The
first implementation rounds proved that a durable `PlanRevision` plus
in-memory planning-cycle state is insufficient for the production boundary:

- planner-call budgets and no-progress state must survive restarts;
- a planner result can become stale even when its provider call finishes
  successfully;
- `RunExecution` must not bypass the accepted Plan by entering the legacy
  scheduler directly;
- the exact product inputs a proposal was based on must remain distinguishable
  from both the planning operation that consumed budget and the accepted Plan
  that resulted from it.

External architecture research independently reinforces the same ownership
and fencing principles: durable run identity must fence stale workers at the
authoritative write boundary, and reusable derived state is valid only while
its compatibility fingerprint still matches. That research is supporting
evidence only; the contracts below are Runlith-owned decisions derived from
ADR-0017, ADR-0027 and the project invariants.

Binding constraints include INV-043..047, INV-050, INV-112, INV-146,
INV-148 and INV-152.

## Decision

### 1. Planning input, planning operation, and accepted Plan are distinct

G7 uses three durable concepts with separate ownership:

`PlanningInputSnapshot` is an immutable, versioned product-authored snapshot
of the trusted inputs used for one planning decision. It carries the Plan
branch and Task binding, scope/risk/criteria inputs, Policy fingerprint,
triggering evidence, inherited lineage evidence, relevant approval binding,
the expected Plan head, and a canonical SHA-256 input fingerprint. It is
data and provenance only. Possessing or replaying a snapshot grants no
execution, Policy, approval, Task, or Plan mutation authority.

`PlanningRun` is the durable lifecycle of one planning operation. It binds to
one exact input snapshot/fingerprint and expected Plan head, owns a monotonic
claim generation for its Plan branch, and owns planner-call budget/spend and
recovery state. A process-local service object or provider session is never a
substitute for this identity.

`PlanRevision` remains the accepted structural Plan result from ADR-0027.
Every newly accepted revision is additionally bound to the exact
`PlanningInputSnapshot` and `PlanningRun` generation that produced it.

These records must not be collapsed into one mutable "planning state" row.
Their lifecycles differ: inputs are immutable evidence, runs represent
operational ownership, and revisions represent accepted Plan history.

### 2. Snapshot freshness is explicit and fail closed

Each Plan branch has at most one current planning-input snapshot pointer.
Publishing a newer product-owned snapshot supersedes the previous pointer but
does not delete history. Snapshot identity and fingerprint are deterministic
over the complete trusted input set relevant to the planning decision.

A planner result may be published only while all of these still hold at the
authoritative SQLite write boundary:

1. the proposal's expected Plan head is still the durable branch head;
2. the branch's current input snapshot id/fingerprint still equals the run's
   bound snapshot;
3. the current `PlanningRun` generation still equals the publishing run;
4. the run is still in its publishable state;
5. the proposal passes ADR-0027 V1-V9 and its measured receipt provenance
   resolves durably.

Any mismatch is a typed stale/refusal outcome. There is no silent rebase,
snapshot substitution, generation takeover, or best-effort publication.

### 3. PlanningRun generation fences stale planner workers

Starting a new planning operation for a branch advances the branch's durable
planning-run generation and binds the new run to its exact snapshot. An older
provider call may finish later, but it cannot publish after that generation
has moved. The generation check is repeated inside the same transaction that
commits the accepted `PlanRevision`.

Provider session identity does not participate in this authority. Providers
remain replaceable proposal executors.

### 4. Planner spend belongs to PlanningRun accounting

ADR-0027 §6 is amended as follows.

`PlannerCallSpend` is trusted adapter/application control-flow evidence:

- `NOT_STARTED`: no provider call crossed its execution boundary;
- `MEASURED { receipt }`: provider-reported usage was recovered;
- `UNMEASURED`: a call may have started but exact usage is unavailable.

Before each `PlannerPort` invocation, the trusted planning service durably
reserves one call slot and the maximum per-call input charge for the exact
branch/evidence/run. The call slot remains consumed after reservation.
Settlement changes the token charge only: `NOT_STARTED` becomes zero tokens,
`MEASURED` uses the exact durable receipt, and `UNMEASURED` keeps the
conservative reservation. A crash after reservation therefore cannot erase
possible spend.

Spend, call counters, and L3 no-progress accounting belong to the planning
operation/guardrail boundary. They are not fields of `PlanningInputSnapshot`.

### 5. Production RunExecution is accepted-Plan gated

`RunExecution` is an EXECUTE operation. It must never invoke `PlannerPort`
and must never synthesize a Plan or planning defaults as a side effect of an
execution request.

Before the first Task lifecycle mutation, the production composition root:

1. resolves the current accepted Plan that represents the requested Task;
2. refuses if there is no such Plan or if the mapping is ambiguous;
3. verifies that the accepted revision is bound to the branch's current
   planning-input snapshot/fingerprint;
4. checks Plan predecessor readiness against canonical Task state;
5. carries the exact accepted Plan head as `PlanHeadPrecondition` into the
   existing atomic execution-entry transaction.

The transaction rechecks that Plan head before `READY -> CLAIMED -> RUNNING`
or any Execution/Attempt reservation is written. Missing, stale, ambiguous,
or superseded planning state therefore produces zero Task mutation and zero
provider invocation.

Initial planning is a separate control-plane operation. The synchronous
`RunExecution` path does not spend LLM tokens trying to make itself
executable.

After successful execution admission, the durable `Execution` identity owns
that admitted run. Explicit pre-Attempt resume may continue the already
admitted Execution under the existing recovery contract; it does not create a
new planning decision. Current Policy/effective capability and Runner
requirements are still re-evaluated at the privileged provider-operation
boundary, so an accepted Plan or old snapshot never carries authorization
forward by itself.

### 6. Plan branch selection is product-derived

The G5 protocol-v2 `RunExecutionRequest` is not extended with a caller-chosen
Plan branch or snapshot id. The daemon resolves the current accepted Plan from
durable product state. Exactly one current accepted branch may represent the
Task for this operation; zero or multiple matches fail closed.

This avoids turning a client-supplied branch name, default branch convention,
or provider output into execution authority and preserves the frozen G5 wire
shape.

## Alternatives considered

- **One mutable planning-control row**: rejected because input provenance,
  operational ownership/spend, and accepted Plan history have different
  lifecycles and would become competing authority in one record.
- **Keep planning-cycle state only in memory**: rejected because restart or a
  second service handle could reset budget, L3 history, or stale-writer
  ownership.
- **Planner call inside `RunExecution`**: rejected because execution would
  acquire hidden planning/spend semantics and could mutate Plan state merely
  because a user requested execution.
- **Caller supplies the Plan branch/snapshot to execute**: rejected because a
  client identifier is not proof that the Plan is current or applicable.
- **Plan-head check only before the transaction**: rejected because a
  concurrent revision can move the head between the read and Task mutation.
- **Treat a provider session or successful resume as authority**: rejected by
  provider neutrality and current Policy revalidation requirements.

## Consequences

- TASK-0078 persists immutable planning-input snapshots, durable planning-run
  generation, and revision bindings in the planning storage boundary.
- Planner publication gains input-fingerprint and run-generation fencing in
  addition to ADR-0027's stale Plan-head check.
- Production execution uses the already accepted Plan path and never calls a
  planner. Lower-level scheduler services may remain reusable internally, but
  the production `RunExecution` composition cannot bypass the G7 gate.
- Existing G5 IPC field numbers and request shape remain compatible; G7 adds
  server-side admission semantics rather than client-authored authority.
- TASK-0079 remains the owner of the durable `HumanDecisionRequest` lifecycle;
  this ADR does not implement HDQ UI or broaden V9 authority.

## Security impact

This decision closes two stale-authority paths: late planner workers cannot
publish after their input/run ownership is superseded, and execution cannot
claim a Task without a current accepted Plan. Snapshot, planner identity,
provider output and receipt data remain provenance rather than authority.
Policy/approval semantics are not copied into planning state; current
authorization is still evaluated at the operation boundary. All stale or
ambiguous cases fail before Task mutation.

## Migration / replacement impact

TASK-0078 may add planning-owned relational tables and revision-binding
columns alongside the existing Plan persistence. Historical Plan snapshots
remain immutable. The migration must be restart-safe and fail closed on
missing/corrupt snapshot or run bindings for revisions created under this
contract. No Task, Policy, Approval, Execution, Workspace, provider-session,
or Security Audit table becomes a second source of planning truth.

Planner adapters remain replaceable behind `PlannerPort`; the durable
snapshot/run contracts are product-owned and provider-neutral.

## References

- ADR-0017 — adaptive planning is proposal-driven and revisioned.
- ADR-0024 — provider execution seam and production composition-root pattern.
- ADR-0027 — G7 planning/orchestration contract amended by this decision.
- `docs/architecture/INVARIANTS.md` — INV-043..047, INV-050, INV-112,
  INV-146, INV-148, INV-152.
- External research evidence: durable expected-run fencing and write-boundary
  prerequisite rechecks; compatibility fingerprints and independent Policy
  revalidation. External implementations are evidence only and are not copied.
