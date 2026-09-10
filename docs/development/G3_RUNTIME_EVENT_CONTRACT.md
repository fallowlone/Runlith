# G3 Runtime Event Contract

> TASK-0034 — contract-only. Evidence/summary: ADR-0022,
> `docs/architecture/events.md` §4.2, and
> `contracts/json-schema/event.schema.json` remain architecture authority.
> No persistence, service, FakeAgent, Scheduler, Runner, or IPC was
> implemented here.

## Original TASK-0034 Blocker

The first TASK-0034 attempt stopped during mandatory preflight with no
files changed: no accepted source defined canonical Event Journal
semantics for Execution creation, Attempt creation/start, or Attempt
terminal outcome (`events.md` left naming, aggregate envelope, and
runtime scope open; the journal admitted only twelve task/project-scoped
G2 types; stuffing runtime identity into `reason`/`provenance`/fake
`TaskId` was forbidden). The blocker was accepted; this task replaces
the implementation objective with the missing contract.

## ADR-0022 Decision

`docs/architecture/adr/ADR-0022-runtime-event-journal-scope.md`
(Accepted): one Event Journal stays authoritative (ADR-0006); the
envelope gains first-class nullable `execution_id`, `attempt_id`, and
`attempt_outcome`; project remains the root scope
(`Project → Task → Execution → Attempt`, service-derived); Task
`from_state`/`to_state` stay Task-only; provenance stays service-owned;
completion authority stays with independent evaluation (ADR-0018).
Index and coverage register updated.

## Runtime Event Types

Exactly three, all `event_version = 1`:

```text
execution.created  — durable Execution created for a Task
attempt.created    — new durable open Attempt allocated (NOT a claim
                     that a process spawned, a session exists, a runner
                     started, or tool execution began)
attempt.concluded  — open Attempt transitioned exactly once to its
                     canonical terminal outcome (only terminal event)
```

No `execution.started/succeeded/failed/completed`, no
`attempt.started` split (the domain has one birth fact:
`Execution::start_attempt`), no Session events, no per-scenario types.

## Scope Envelope

Snake_case, matching the existing envelope convention. Forbidden
encoding targets (`reason`, `provenance`, `correlation_id`,
`causation_id`, free-form payloads, fake `TaskId`) carry no runtime
identity; no parallel journal table; no generic JSON payload bucket.

## Scope Matrix

| Event family | project_id | task_id | execution_id | attempt_id | attempt_outcome | Task from/to |
|---|---|---|---|---|---|---|
| `project.created` | required | null | null | null | null | null |
| `task.created`, `task.dependency_added` | required | required | null | null | null | initial / unchanged |
| Task lifecycle events | required | required | null | null | null | canonical Task states |
| `execution.created` | required | required | required | null | null | null |
| `attempt.created` | required | required | required | required | null | null |
| `attempt.concluded` | required | required | required | required | required | null |

## Attempt Outcome Semantics

Single authoritative outcome per `attempt.concluded`, serialized
lowercase (distinct from SCREAMING Task states):

```text
Succeeded -> succeeded | Failed -> failed
TimedOut -> timed_out  | Cancelled -> cancelled
```

`reason` is never the authoritative outcome; Attempt `detail` stays
out of the journal. All eleven FakeAgent scenarios later map onto
these four outcomes plus evidence — scenario identity is not event
identity, and the scenario→outcome mapping belongs to later tasks.

## Event Versions

Runtime types start at `event_version = 1`; no G2 version moves.
SQLite schema version, event version, IPC protocol version, and
product version are distinct. Typed reads fail closed on unknown types
(no `Other`) and unsupported versions (no silent v1-of-v2).

## Completion Authority

```text
attempt.concluded(succeeded) != Task COMPLETED
```

Path stays: Succeeded → evidence → `submit_result` → independent
verification → `verification_succeeded` → COMPLETED. FakeAgent reports
through the trusted service and never appends journal facts.

## Deferred Runtime Semantics

Execution terminal lifecycle, Session events (`session_id` never
enters the journal), richer evidence beyond the typed fields, and all
non-G3-runtime taxonomy stay open per `events.md` §18.

## Validation

- Contract matrix against the real schema file via a stdlib-only
  2020-12 subset evaluator (no new dependency; repo has no schema
  validator — the existing mechanism is the `service.rs`
  compile-time field-name check, which still passes unchanged):
  9 valid forms (incl. all four outcomes) accepted, 16 invalid forms
  (every matrix case in the task plus bogus outcome and missing
  envelope field) rejected — 25/25 green.
- `cargo fmt --all -- --check`, `cargo check --workspace`,
  `cargo test --workspace --no-fail-fast`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test -p product-version`: all green (contract-only change;
  no Rust code touched).
- Consistency: `Task != Execution != Attempt != Session` intact;
  journal ≠ Event Sourcing; journal ≠ Security Audit; no
  secret/session/provider-specific identity in the envelope; no
  migration 0004, no RuntimeService, no FakeAgent/Scheduler/Runner/IPC.

## Next Implementation Task

TASK-0035 (Execution/Attempt Persistence + Runtime Events) implements
exactly this contract: migration 0004, `EventType` codec extension,
envelope guards, and atomic `RuntimeService` commits — without
inventing architecture.
