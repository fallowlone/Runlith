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

## Amendment — TASK-0086 child materialization and publication fencing (2026-09-17)

### Existing references and proposed identities

The publication fence distinguishes two cases:

- References to existing Tasks remain restricted to the trusted candidate set
  captured from canonical Task authority. A planner cannot name unrelated
  roots, siblings, another project's Tasks, or arbitrary existing IDs.
- A proposed new child identity from `node_additions` is not required to exist
  in the pre-planning candidate snapshot. It is validated as a new identity at
  the trusted materialization boundary.

The absence of an existing ID is evidence for a possible new child, not
authority to reuse an existing Task. The trusted boundary must prove the ID is
available before materialization.

### Publication fencing additions

The authoritative write boundary must fence concurrent changes to:

1. owning parent Task authority;
2. existing descendant membership;
3. proposed child ID availability;
4. project/task state used for derived authority;
5. Plan branch head and planning-run generation.

Any mismatch refuses publication. TASK-0086 does not weaken TASK-0084's
Task-derived authority checks.

### Planner input contract

New child materialization does not require adding existing candidate Task IDs
to `DecompositionRequest`. New additions are independent from existing Task
references.

If future replan/update/remove operations require existing references, they
must use the already established trusted candidate-set semantics rather than
exposing unrestricted identifiers. Any protocol expansion must preserve the
rule that clients and providers cannot supply authority.

### Recovery implications

The implementation must define durable reconciliation for interrupted
materialization. A daemon restart, duplicate proposal, stale base revision,
partial Task creation, or concurrent ID claim must produce an observable
recoverable state. The system must distinguish a replay of the same accepted
materialization from a conflicting attempt by durable provenance.

## Amendment — TASK-0084 canonical Task authority (2026-09-16)

### Problem

After TASK-0083, `PlanningInputSnapshot` captured canonical Task scope,
acceptance, baseline risk and trusted candidate Tasks, but §2 only fenced
snapshot identity, run generation and Plan head. `PlannerPort` runs outside
SQLite, so a Task write during the call left all three unchanged and a stale
proposal still published. Separately, the shipped control plane could create
only objective-only Tasks, so G7 planning authority could only be seeded by
test-side SQLite writes, contrary to ADR-0016.

### Decision

1. §2 gains a sixth publication precondition for every Task-derived planning
   context (all production paths use `canonical_policy_context`): inside the
   same `BEGIN IMMEDIATE` transaction as the revision commit, storage reloads
   the owning Task and its project through the Task repository, re-derives
   planning authority, and requires scope roots, baseline risk, acceptance
   criteria and the trusted candidate Task set to equal the persisted
   snapshot. A missing Task, underivable facts or any difference is
   `PlanningInputStale`; nothing is written and no Human Decision is consumed.
   Task tables remain the only owner of these facts.
2. The derivation (`plan_domain::TaskPlanningAuthority` and
   `trusted_candidate_task_refs`) is one pure function over plain Task facts,
   shared by policy derivation, the publication fence and authoring
   validation.
3. Protocol v6 adds `task_authoring_core.CreatePlanningTask`: `TaskService`
   creates a DRAFT Task whose scope/acceptance/risk are validated (bounds,
   DRAFT-only domain operation, planning derivability) and committed with the
   server-minted `task.created` event. No edit operation exists; historical
   `CreateTaskRequest` and v0-v5 sessions are unchanged.

### Alternatives considered

- Fence on a Task version counter: rejected; out-of-band writes that bypass the
  counter would still publish, and descendant membership spans many Tasks.
- DRAFT-only metadata edit operation: rejected for G7; it needs a new journal
  event type (CHECK-constrained journal rebuild) and adds a mutation path
  planning must race against, while creation-time authoring is sufficient.
- Adding fields to protocol-v1 `CreateTaskRequest`: rejected; an older daemon
  would silently drop them and historical wire authority would change.

### Consequences, security and migration impact

Stale Task authority can no longer publish, and the shipped CLI supports
`task create-planned -> task ready -> planning trigger -> execution run`
without direct database access. Hand-built trusted fixture contexts (tests
only) are not Task-derived and are not fenced by this precondition. Clients
still cannot supply Policy fingerprints, snapshots, graphs, provenance or
state. No schema migration: authoring writes existing Task columns.

## References

- ADR-0017 — adaptive planning is proposal-driven and revisioned.
- ADR-0024 — provider execution seam and production composition-root pattern.
- ADR-0027 — G7 planning/orchestration contract amended by this decision.
- `docs/architecture/INVARIANTS.md` — INV-043..047, INV-050, INV-112,
  INV-146, INV-148, INV-152.
- External research evidence: durable expected-run fencing and write-boundary
  prerequisite rechecks; compatibility fingerprints and independent Policy
  revalidation. External implementations are evidence only and are not copied.
