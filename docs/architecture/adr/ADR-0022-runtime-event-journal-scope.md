# ADR-0022 - Runtime Events Use Typed Execution/Attempt Journal Scope

Status: Accepted
Date: 2026-09-10

## Context

The first TASK-0034 implementation attempt stopped correctly during its
mandatory architecture preflight: no accepted source defined canonical
Event Journal semantics for Execution creation, Attempt creation/start,
or Attempt terminal outcome. `docs/architecture/events.md` explicitly
left event naming, the aggregate/entity reference envelope, and runtime
scope open; the journal schema (migrations `0002`/`0003`) and the
closed `EventType` codec admit only the twelve task/project-scoped G2
types; and stuffing runtime identities into `reason`/`provenance`/fake
`TaskId` was forbidden. Per ADR-0019, the missing decision must be
materialized before implementation — exactly as TASK-0027 defined the G2
gap types before TASK-0028 implemented them (ADR-0021 §20).

The `agent-runtime` domain (TASK-0033) is authority for the runtime
facts this contract journals: one birth fact per Attempt
(`Execution::start_attempt` → `Attempt::begin`, the only constructor),
exactly-once terminal conclusion with a four-variant outcome
(`Succeeded | Failed | TimedOut | Cancelled`), sequential attempts,
immutable history, and no Task lifecycle access. This ADR decides only
how those facts appear in the Event Journal; it designs no tables, no
Rust codec, no service, and no Scheduler/FakeAgent behavior.

## Decision

One Event Journal stays authoritative (ADR-0006): no `runtime_events`
or sibling table is introduced. The canonical envelope gains three
first-class nullable scope fields in snake_case, following the existing
envelope convention (`event_type`, `correlation_id`, …):

```text
execution_id   — ExecutionId reference, null unless the fact is execution-scoped or narrower
attempt_id     — AttemptId reference, null unless the fact is attempt-scoped
attempt_outcome — canonical terminal outcome, required only on attempt.concluded
```

Project remains the root scope: every runtime event carries required
`project_id` and required `task_id`, derived and validated from
authoritative Task ownership down the chain
`Project → Task → Execution → Attempt`. Callers, providers, Runners,
and FakeAgent cannot supply a contradictory `project_id`; the trusted
application service resolves it from the referenced Task.

Exactly three initial G3 runtime event types exist — no more:

```text
execution.created  — a durable Execution aggregate was created for a Task.
                      Scope: project + task + execution required;
                      attempt_id null; attempt_outcome null.
attempt.created    — a new durable open Attempt was allocated inside an
                      Execution. Scope: project + task + execution +
                      attempt required; attempt_outcome null.
                      This records allocation/opening only. It does NOT
                      claim a provider process spawned, a provider
                      session exists, a runner started, or tool
                      execution began. The domain exposes a single
                      birth fact, so creation and start share this one
                      event; a future runtime-start event may be added
                      if Runner/provider semantics require it.
attempt.concluded  — an open Attempt transitioned exactly once to its
                      canonical terminal outcome. Scope: project + task
                      + execution + attempt + attempt_outcome required.
                      The only terminal runtime event. Failure
                      scenarios are never separate event types.
```

`attempt.concluded` carries exactly one outcome serialized in
lowercase snake, mapped from the accepted domain spelling:

| Domain (`agent-runtime`) | Journal (`attempt_outcome`) |
|---|---|
| `Succeeded` | `succeeded` |
| `Failed` | `failed` |
| `TimedOut` | `timed_out` |
| `Cancelled` | `cancelled` |

Lowercase keeps outcomes visually distinct from SCREAMING Task states,
reinforcing that `from_state`/`to_state` stay Task-only: both are null
on all three runtime types and remain semantically `TaskState?`, never
generic aggregate state. `reason` is never the authoritative outcome.
Attempt `detail` (the opaque adapter-minted note) is not journal state;
it may live in relational Attempt state or later structured evidence,
never as unbounded provider output, transcript, prompt, or secret in
the journal.

FakeAgent scenario identity is not event identity: the eleven
scenarios (`success`, `crash`, `hang`, `timeout`,
`malformed_result`, `forbidden_path_write`, `forbidden_command`,
`quota_reached`, `network_failure`, `partial_result`,
`review_rejection`) map to canonical outcomes plus evidence in later
tasks; no per-scenario event type exists.

Canonical scope matrix (G2 rows unchanged):

| Event family | project_id | task_id | execution_id | attempt_id | attempt_outcome | Task from/to |
|---|---|---|---|---|---|---|
| `project.created` | required | null | null | null | null | null |
| `task.created`, `task.dependency_added` | required | required | null | null | null | initial / unchanged state per contract |
| Task lifecycle events | required | required | null | null | null | canonical Task states |
| `execution.created` | required | required | required | null | null | null |
| `attempt.created` | required | required | required | required | null | null |
| `attempt.concluded` | required | required | required | required | required | null |

All three runtime types use `event_version = 1`; existing G2 versions
do not move. Schema version, event version, IPC protocol version, and
product version stay distinct concepts. Typed reads fail closed on
unknown event types (no `Other` escape hatch) and on known types with
unsupported versions (no silent v1 interpretation of v2 semantics).

Provenance authority is unchanged: providers, FakeAgent, Runners, and
clients cannot mint `event_id`, `event_type`, `event_version`,
timestamps, correlation/causation, provenance, aggregate references, or
`attempt_outcome`. The trusted application service constructs every
authoritative field; FakeAgent later reports results through that
boundary and never appends `attempt.concluded` directly.

Completion authority is unchanged (ADR-0018):
`attempt.concluded(succeeded) != Task COMPLETED`. Runtime events never
grant completion; the required path stays
Attempt Succeeded → evidence → `submit_result` → independent
verification → `verification_succeeded` → COMPLETED.

Deliberately absent: Execution terminal events (`execution.started /
succeeded / failed / completed` — TASK-0033 defines no terminal
Execution lifecycle), Session events (`session_id` /
`provider_session_id` never enter the journal; Session stays
transient), and any generic authoritative JSON payload bucket
(`payload_json` / `metadata_json` / … would create a second weakly
typed domain inside the journal; richer evidence later uses explicit
contracts or artifact references).

## Alternatives considered

- Parallel `runtime_events` table: rejected; it splits ADR-0006's one
  journal into competing histories and forecloses the staged
  Event Sourcing migration.
- Encoding runtime identity in `reason`/`provenance`/`correlation_id`
  or a fake `TaskId`: rejected; untyped stuffing defeats typed scope,
  auditability, and the future migration's ability to project on
  aggregate references.
- `attempt.started` as a second birth event: rejected; the domain has
  exactly one Attempt birth fact, and a second name would falsely imply
  process-spawn semantics the runtime cannot yet evidence.
- Per-scenario event types mirroring the eleven FakeAgent scenarios:
  rejected; scenario identity is adapter/test behavior, and eleven
  durable types would freeze simulation vocabulary into the journal.
- Generic `payload_json` extensibility: rejected; explicit typed
  scope/outcome fields preserve the staged-migration discipline that
  untyped buckets destroy.

## Consequences

- TASK-0035 (migration 0004 + RuntimeService) can be implemented
  without inventing architecture: tables, the `EventType` codec
  extension, envelope guards, and atomic commit paths follow this
  contract exactly.
- `events.md` resolves its G3 runtime open items narrowly; unrelated
  future taxonomy stays open.
- The machine-readable envelope schema gains optional
  `execution_id`/`attempt_id`/`attempt_outcome` with conditional scope
  validation; G2 validation is unchanged.

## Security impact

No runtime execution, persistence, network access, credential, or AI
behavior is introduced — contract only. The envelope gains no secret,
session, transcript, or provider-specific identity surface; provenance
authority stays with the trusted service, so provider/FakeAgent output
remains untrusted data that cannot mint journal facts (INV-008,
INV-093). Journal/audit separation (INV-073) is preserved.

## Migration / replacement impact

Additive contract: no existing event row, type, version, or scope rule
changes. Migration 0004 (TASK-0035) extends the journal's accepted type
and scope set; historical G2 rows must remain readable, and future
semantic changes to one runtime type increment that type's event
version independently.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§2.2, 10.2, 22
- `INVARIANTS.md`: INV-002, INV-004, INV-006 (via ADR-0006), INV-008,
  INV-009, INV-010, INV-025, INV-044, INV-066, INV-070, INV-073, INV-093
- `docs/architecture/events.md` (§4.2 runtime contract, scope matrix),
  `docs/architecture/task-engine.md` (§§4, 12–13),
  `docs/architecture/providers.md` (§§6, 11),
  `docs/architecture/fake-agent.md` (§6)
- ADR-0005 (Task != Execution != Attempt != Session), ADR-0006 (state +
  journal), ADR-0009 (provider ACL), ADR-0018 (independent evaluation),
  ADR-0019 (gate-scoped activation), ADR-0021 §20 (contract-first
  precedent for new event types)
- `contracts/json-schema/event.schema.json` (runtime scope + conditionals)
- Code authority: `crates/agent-runtime` (`AttemptOutcome`,
  `Execution::start_attempt`, `Attempt::conclude`), existing
  `adapters/storage-sqlite` journal codec (G2 behavior unchanged)
