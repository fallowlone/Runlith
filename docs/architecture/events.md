# Event Journal — Domain Contract

> Repository-local contract for the Event Journal and domain-event semantics:
> purpose, authority, envelope requirements, persistence separations, and
> explicitly open schema questions. Documents accepted architecture; invents
> none; implements nothing.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Task lifecycle: [`task-engine.md`](./task-engine.md)
> - Decisions: [`adr/ADR-0004-task-central-entity.md`](./adr/ADR-0004-task-central-entity.md),
>   [`adr/ADR-0005-task-execution-attempt-session.md`](./adr/ADR-0005-task-execution-attempt-session.md),
>   [`adr/ADR-0006-state-event-journal-event-sourcing.md`](./adr/ADR-0006-state-event-journal-event-sourcing.md)
> - Sensitivity rules: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§2.2, 10.2,
>   22, 55–59, Reaction Policy, ZeroClaw-derived §§4–6

## 1. Core persistence model

Accepted v1 architecture:

```text
Authoritative relational current state
+
Append-only Event Journal
+
Separate Security Audit
```

This is deliberately NOT full Event Sourcing in v1 (ADR-0006). The Event
Journal is a migration-friendly foundation for stronger replay and
selectively event-sourced architecture later. This document must not be read
as redesigning v1 into an event-sourced system.

## 2. Current state remains authoritative in v1

Task current status, Plan current revision, and other current
bounded-context state are read from the authoritative relational model in v1
where that context uses relational persistence. Normal v1 operation must NOT
require rebuilding current application state by replaying every historical
event. Replay-as-truth may become valid for selected bounded contexts later;
it is not current architecture.

## 3. Event Journal purpose

The Event Journal preserves durable historical evidence of meaningful
application/domain changes. Its roles:

```text
auditability
debugging
causal tracing
recovery support
historical inspection
projection generation
future replay capability
future selective Event Sourcing migration
```

Not every role is fully implemented in v1; the list states purpose, not
delivery status.

## 4. Domain event semantics

A domain event represents a durable fact that the trusted application layer
accepted and committed. Conceptually, facts such as "Task status changed",
"Plan revision accepted", "Execution attempt started", or "Verification
outcome recorded" may later yield domain events. No exhaustive canonical
event taxonomy is created here.

### 4.1 G2 canonical mutation events (ADR-0021)

The G2 audit (ADR-0021 §19) classifies every persistent G2 mutation.
Lifecycle transitions already carry full typed coverage through the nine
committed journal types (`task.marked_ready`, `task.claimed`,
`task.started`, `task.blocked`, `task.submitted`, `task.routed_to_review`,
`task.verification_began`, `task.verification_succeeded`,
`task.verification_failed`). Creation and dependency mutations had no
coverage, so three exact canonical types close the gap:

```text
project.created        — a Project row committed (project-scoped)
task.created           — a Task row committed in its initial state
task.dependency_added  — a dependency edge committed; lifecycle state
                         unchanged (from_state == to_state == current)
```

No generic `EntityChanged` / `MutationOccurred` exists: typed events
stay the rule. These three are contract in this task (defined here and
in the envelope schema's canonical examples); their SQLite/application
handling is NOT implemented here — TASK-0028 implements all three
atomically (state mutation + journal evidence in one transaction, §6)
before exposing the corresponding CLI commands. Dependency removal is
not a G2 wire operation, so no `task.dependency_removed` exists yet; a
future removal API must define it first. `task_strings` list metadata
stays bounded persistence machinery (§9 evidence-reference discipline),
never a business IPC surface.

## 5. Event is not command

```text
Command / Request:
"Move Task to RUNNING"

Domain Event:
"Task transition to RUNNING was committed"
```

Events describe accepted facts, never requests for permission. An Event
Journal record is not a Policy authorization mechanism.

## 6. Transactional state + journal invariant

Accepted mutation pattern:

```text
BEGIN
validate mutation
UPDATE authoritative current state
INSERT corresponding Event Journal record
COMMIT
```

Semantic invariant: a significant durable state change that requires a
domain event must not commit current state while silently losing its
corresponding Event Journal record; conversely, an event representing a
state mutation must not claim the mutation committed when the authoritative
mutation rolled back. Atomicity scope is semantic only:

```text
state mutation + required journal evidence
=
one atomic persistence outcome
```

No database implementation beyond the accepted SQLite direction is chosen,
and no Rust transaction traits or SQL tables are defined here.

## 7. Append-only semantics

The Event Journal is append-only during normal application operation. Past
records must not be silently rewritten because a later interpretation
changed, a projection changed, a bug was fixed, or the UI wants cleaner
history. Corrections occur conceptually through later facts/events or
migration mechanisms where appropriate. No canonical correction-event type
is invented here, and no v1 compaction/snapshot semantics are created:
future snapshots, stream compaction, or archival belong to later
architecture, while the v1 journal remains conceptually append-only history.

Append-only does NOT mean undeletable forever. It is compatible with the
privacy contract (user can delete app-owned local data, PRIV-008): history
is not mutated in place during normal lifecycle operations, but that is
neither cryptographic immutability nor exemption from authorized
project/app-data deletion. Retention, compaction, archival, and
project-deletion behavior remain privacy/storage design work; no durations
are invented here.

## 8. Minimum event envelope

Every persistent domain event requires semantic equivalents of:

```text
typed identity
schema version
timestamp
correlation metadata
causation metadata
```

Conceptual names such as `event_type`, `schema_version`, `occurred_at`,
`correlation_id`, `causation_id` may be used illustratively, but concrete
serialization and field names are pending schema design. Requirements:

- **Event identity** — stable record identity for references, replay, and
  audit is expected, but the ID format (UUID, ULID, Snowflake, integer
  sequence, or other) is NOT selected here; it is an open design item.
- **Event type** — provider-neutral, runtime-neutral, stable enough to
  persist, and versionable. Provider-specific types such as
  `ClaudeTaskCompleted`, `CodexAttemptFailed`, or `GeminiReviewFinished`
  must never appear; domain semantics govern. No types are created now.
- **Schema version** — every persistent event type must be versionable. The
  journal must never rely on "whatever Rust struct currently deserializes"
  as its compatibility strategy. Event schema version is distinct from
  application build version, IPC protocol version, and provider version;
  no version-negotiation mechanics are invented.
- **Timestamp** — timestamp metadata is required, but clock source,
  monotonic behavior, application- vs database-generated origin, and
  precision are NOT decided here; clock semantics are open unless accepted
  architecture specifies them.
- **Correlation** — events belonging to the same larger
  operation/workflow/execution chain share correlation context (e.g. user
  starts operation → Task transition → Execution created → Attempt
  started → Verification). Exact correlation-ID generation is not defined.
- **Causation** — which prior action/event/request directly caused this
  fact. Correlation and causation are different dimensions and must never
  collapse into one identifier. Exact causation-ID generation is not
  defined.
- **Provenance** — events should preserve enough provenance to explain
  where a durable mutation came from (conceptually: user action,
  application command, Task, Execution, Attempt, PlanRevision, Policy
  decision, Evaluation result) without trusting model narrative. No
  mandatory provenance fields are invented; the final envelope structure is
  pending schema design.

## 9. Secrets, sensitivity, and evidence references

Event Journal payloads must not contain SECRET values, raw API keys,
passwords, private keys, or provider credentials (INV-008), and the journal
must not become a convenient dump of prompts, source files, full tool
outputs, or raw filesystem contents. All event metadata and payloads remain
subject to the PUBLIC / PROJECT / PRIVATE / RESTRICTED / SECRET rules;
SECRET is prohibited from journal payloads, and derived event information
does not automatically become PUBLIC. See
[`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md);
nothing there is duplicated here. Where durable events reference
large/sensitive evidence, future design should prefer artifact/reference
IDs, hashes, and bounded metadata over raw duplication — without creating
an ArtifactStore or inventing hash algorithms in this task.

## 10. Persistence separations

### Event Journal vs Security Audit

The journal tracks meaningful durable application/domain changes; the audit
tracks security-relevant facts (who requested a privileged action, Policy
decision, approval, privileged-action result). One real-world operation may
produce both a domain event and a security audit record, but they are
different persistence categories with different semantics — never one
generic audit/events table conceptually (INV-073).

### Event Journal vs Operational Log

Operational logs carry diagnostics (process started, retry failed, socket
reconnected) under logging policy. They are NOT automatically durable
domain history. Not every log line is an event.

### Event Journal vs Agent Transcript

Agent Transcript is provider/user-controlled conversation history, not
authoritative application state. Entire transcript messages must not be
copied into the journal merely to preserve history; agent claims are not
domain facts (INV-110).

### Event Journal vs Memory

Memory holds selected durable learned knowledge; the journal holds
historical application/domain facts. An event is not automatically promoted
to Memory, and Memory is not the journal.

### Event Journal vs Usage Ledger

Tokens, cost, quota, and runtime measurements belong to Usage. The journal
may reference relevant usage evidence where needed but never becomes the
canonical usage accounting store.

### Event Journal vs Tool Execution Receipt

A `ToolExecutionReceipt` is runtime evidence that a tool execution
occurred — not authorization, correctness, safety, or Task completion
(INV-072). A receipt may be referenced by durable evidence, but receipt
mechanics, domain events, and security audit entries are three types that
must never merge into one.

## 11. Typed Agent Bus distinction

The Typed Agent Bus coordinates runtime actors with messages such as
`TaskAssigned`, `TaskClaimed`, `TaskBlocked`, `ClarificationRequested`,
`DecisionProposed`, `ArtifactProduced`, `ReviewRequested`,
`ReviewRejected`, `ReviewApproved`, `TestFailed`, `TestPassed`,
`MergeRequested`, and `TaskSubmitted`. A bus message is NOT automatically a
persisted Event Journal event. Three conceptual categories with different
persistence semantics:

```text
Coordination Message  — transport delivery coordinates actors; delivery
                       itself is not necessarily the authoritative fact.
Domain Event          — committed durable fact in the journal.
External Lifecycle Signal — outside observation normalized into a fact.
```

A message such as `ReviewRequested` may result from or cause durable state
changes, but transport delivery is not the authoritative domain fact. No
message-bus implementation is designed here, and the exact mapping from bus
messages to persisted domain events is an open design item.

External lifecycle signals (`CI_FAILED`, `REVIEW_CHANGES_REQUESTED`,
`PROVIDER_TIMEOUT`, `ATTEMPT_STALLED`, `PR_APPROVED`) feed the Event
Journal and Reconciler rather than hidden shell loops (Reaction Policy,
INV-091). They are external observations that may be normalized into
durable facts; exact canonical event mappings are NOT defined here.

## 12. Validation, plans, tasks, and invalid mutations

Model statements, provider outputs, external webhook-like signals, and tool
textual output must never become authoritative durable state merely because
they were received; accepted application/domain validation precedes
authoritative mutations and events (planner output is a proposal, INV-043).
Validated Task/Plan graph mutations persist transactionally and append
corresponding domain evidence, with every structural Plan mutation already
requiring a versioned PlanRevision with reason and provenance (INV-044) —
but no event types are invented. Future Task lifecycle implementation emits
journal history for committed significant transitions, with one guardrail:
TASK-0006 left some Task transitions unresolved, and any event
corresponding to an unresolved transition is equally unresolved — event
design must not sneak in Task-state decisions. If a Task transition
validation fails, authoritative state does not change and no domain event
may falsely claim it committed (a rejected-command diagnostic may exist
separately in operational logs; failure logging is not defined here).

## 13. Projections

Independent projections derived from domain events are permitted. A
projection is a rebuildable/derived view — never competing canonical
mutable truth (INV-070). Eventual examples may include timelines, activity
feeds, historical analytics, and debug/recovery views; no concrete
projection tables are defined. A failed or stale projection must NOT
corrupt or rewrite authoritative current state or journal history:
projection failure != domain mutation rollback, unless part of the same
explicitly required atomic operation, which is not currently specified. No
projection-update transaction rules are invented.

## 14. Ordering, concurrency, delivery

Ordering matters for historical interpretation, but global total order,
per-project sequence, per-aggregate sequence, database row order, and
timestamp order are NOT selected — the ordering model is open. Concurrent
writers are not solved here; the contract must stay compatible with future
optimistic concurrency, Task revision/version checks, multi-worker
execution, and remote workers, with the exact mechanism open. No delivery
semantics (at-most-once / at-least-once / exactly-once, Kafka/NATS
semantics) are invented for journal consumers: the v1 journal is local
persistence architecture, not a distributed message broker, and local v1
needs no Kafka, NATS, Redis Streams, or RabbitMQ — future cloud/remote
architecture may add adapters only when justified.

## 15. Recovery and failover

The journal is one recovery-evidence source alongside authoritative current
state, worktree state, Execution checkpoints, Context Continuity,
idempotency information, and Policy state. Event replay alone is NOT
claimed sufficient for v1 recovery. During provider failover, tool calls,
filesystem writes, and network side effects must never be blindly replayed
from journal history: event history is evidence, not an instruction to
repeat previous side effects (INV-066).

## 16. Migration and compatibility

Preserved evolution path:

```text
v1 Relational State + Event Journal
 ↓
Stable Domain Events + Projections
 ↓
Event-sourced selected bounded contexts
 ↓
Full Event Sourcing only where justified
```

Migration is justified by functional value (replay, reconstruction,
temporal queries, distributed processing, stronger audit), never fashion
(ADR-0006). Persistent records must survive application evolution: typed
event identity, schema version, and an eventual backward/forward migration
strategy are required, but no canonical persistent representation (JSON,
Protobuf, MessagePack, Serde enum, Avro) is selected here — serialization
is open. Behavior on encountering unknown or newer schema versions (fail
closed, projection-only skip, opaque payload preservation, mandatory
migration) is NOT invented; it is an open design item. Cross-boundary
communication may use typed events, explicit application contracts, and
immutable snapshots (never direct reads of another context's tables,
INV-025), but no global enterprise event bus is defined.

## 17. Event ownership and authority

A bounded context owns the semantic meaning of event types describing its
own authoritative facts; infrastructure/storage owns persistence mechanics.
The Task domain owns what a Task lifecycle fact means — the SQLite adapter
does not decide it, and provider adapters cannot invent authoritative
domain-event meaning. No event names are created here. Provider/model/agent
output is untrusted: a model cannot mint an authoritative event by emitting
JSON resembling an event; only trusted validated application services cause
durable domain facts to persist (INV-004, INV-093). Privileged actions may
still need traceability across journal, audit, receipts, and evaluation
evidence via correlation identifiers — without merging the records into one
schema; exact cross-ledger correlation format remains schema work. History
records what the trusted application recorded as occurring at that time: a
later event may supersede interpretation, but earlier events are never
rewritten because later state changed — current truth in v1 stays the
authoritative current-state model.

## 18. Open Event Design Items

Unresolved by accepted architecture; listed, not decided. Each item below
is included only because no accepted source resolves it:

- event record ID format;
- canonical persistent serialization format;
- event naming convention;
- aggregate/entity reference envelope;
- global vs per-project vs per-entity ordering model;
- sequence number semantics;
- clock/timestamp generation and precision;
- exact correlation-ID generation;
- exact causation-ID generation;
- actor/origin provenance representation;
- command/request ID linkage;
- schema compatibility/upcasting strategy;
- unknown/newer event-version behavior;
- projection checkpoint semantics;
- projection rebuild API;
- cross-context event subscription semantics;
- delivery/idempotency semantics for consumers;
- mapping Typed Agent Bus messages → persisted domain events;
- mapping external lifecycle signals → persisted domain events;
- event payload data-classification metadata representation;
- event retention/archival/deletion mechanics;
- transaction strategy when one logical operation mutates multiple bounded
  contexts;
- optimistic-concurrency interaction with event append.

## 19. Non-goals

This document does NOT define: SQLite tables; SQL schema; Rust event enums;
JSON Schema; Protobuf event messages; distributed broker; Kafka/NATS; Event
Sourcing implementation; projection storage; Task transition semantics not
already accepted; Security Audit schema; Tool Receipt schema; telemetry
schema.
