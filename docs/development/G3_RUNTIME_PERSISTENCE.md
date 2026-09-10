# G3 Runtime Persistence

> TASK-0035 — implementation evidence. Architecture authority stays in
> ADR-0022, `docs/architecture/events.md` §4.2, and
> `contracts/json-schema/event.schema.json` (all frozen by TASK-0034;
> this task redesigned none of them).

## Contract Authority

ADR-0022 (`docs/architecture/adr/ADR-0022-runtime-event-journal-scope.md`):
one Event Journal; first-class `execution_id`/`attempt_id`/
`attempt_outcome` scope; exactly three runtime types
(`execution.created`, `attempt.created`, `attempt.concluded`, all
`event_version = 1`); lowercase outcome serialization
(`succeeded`/`failed`/`timed_out`/`cancelled`); Task `from_state`/
`to_state` stay Task-only; provenance stays service-owned; success
never completes a Task. The TASK-0033 domain (`Execution::begin`,
`start_attempt`, `conclude_attempt`, four-variant `AttemptOutcome`) is
authority for runtime semantics; this task persisted it unchanged.

## Migration 0004

`adapters/storage-sqlite/migrations/0004_runtime_core.sql`, schema
`3 → 4` (`SUPPORTED_SCHEMA_VERSION = 4`). Migrations 0001–0003
untouched. Creates `executions` and `execution_attempts`; rebuilds
`event_journal` (new nullable columns + extended type/scope CHECKs)
with verbatim history copy (`seq` preserved, new columns NULL) and the
`sqlite_sequence` pin from the 0003 pattern. A real v3 database built
from the canonical 0001–0003 files migrates with every G2 fact
bit-for-bit intact and the sequence continuing at `evt-3`.

## Execution Persistence

`executions(id, task_id)`: identity plus single Task owner only — no
Task title/description/lifecycle copy, no provider columns
(`ProviderRef` is per-Attempt in TASK-0033), no session/scheduler
columns. `task_id REFERENCES tasks(id)`; restrictive FK default (no
cascade invented — Task deletion/retention is not this task's design).

## Attempt Persistence

`execution_attempts(attempt_id, execution_id, position, provider_ref,
outcome, detail)`: identity, owner, explicit deterministic position
(`Vec` order, never rowid; `UNIQUE(execution_id, position)` plus
contiguity validation on load), opaque `ProviderRef` (identifier, never
credential), canonical outcome text (never ordinal), opaque evidence
note. Outcome/detail commit together in the domain, so a stored row
carrying exactly one of them is corruption. `detail` is diagnostic
relational state, never journal taxonomy; nothing truncates it, and no
transcript/secret surface exists.

## Runtime Event Materialization

The closed Rust `EventType` gains exactly the three ADR-0022 names; a
journal-scoped `AttemptOutcome` codec mirrors the domain 1:1 in both
directions (a future domain variant breaks compilation, never silently
drifts). `NewJournalEvent`/`JournalEvent` carry first-class optional
`execution_id: Option<ExecutionId>`, `attempt_id: Option<AttemptId>`,
`attempt_outcome: Option<AttemptOutcome>` — strong domain types, never
raw SQL strings. G2 rows decode unchanged with runtime fields absent.

## Scope Constraints

Enforced at three layers, all mirroring the ADR-0022 matrix: SQL CHECK
(migration 0004, incl. G2 rows keeping `execution_id/attempt_id/
attempt_outcome` NULL), write-path guards (`require_scope` +
per-commit envelope validators), and read-path re-checks
(`decode_runtime_scope`). Lifecycle commits reject runtime events;
`execution.created`/`attempt.created`/`attempt.concluded` each carry
exactly their family's scope; `from_state`/`to_state` on a runtime row
is corruption. Unknown types, unknown outcomes (including scenario
names like `crash`), and unsupported versions fail explicitly — no
`Other`, no silent default.

## RuntimeService

`crates/app-services/src/runtime_service.rs`: the only trusted runtime
mutation authority, mirroring `TaskService` conventions (Clock,
`op-`/`req-` correlation/causation, fixed
`runtime-application-service/<op>` provenance, `AppliedExecution`
result). Operations: `create_execution`, `create_attempt`,
`conclude_attempt` — plus read-only `get_execution`/`get_attempt`/
`list_events_ordered`. The caller supplies identities and outcome
intent; the service derives `project_id` from authoritative Task
ownership (`Execution → TaskId → Task → ProjectId`), so contradictory
scope is unrepresentable. No scheduling, no completion authority, no
second style. Runtime discipline rejections surface as explicit
`ConstraintViolation`s with verbatim domain reasons (no new
`ServiceError` variant: `bins/agentd` matches exhaustively under the
frozen G2 wire taxonomy, which this task does not touch).

## Atomicity

One SQLite transaction per mutation: `create_execution_with_event`
(Task-exists check + row + event), `create_attempt_with_event`
(execution-exists + no-open-sibling guard + positioned insert +
event), `conclude_attempt_with_event` (guarded `UPDATE ... WHERE
outcome IS NULL` + event). Six trigger-injected failure tests prove
both halves roll back together; mismatched envelopes write nothing;
the sequential guard also holds against callers that bypass the
domain.

## Attempt History

Retry is a new row, never a reset: conclude-once is enforced by the
domain, the guarded UPDATE, and tests (`Failed → Succeeded` rewrite
fails with state, history, and event count unchanged). Ordering is an
explicit validated column, not insertion accident.

## Current-State Authority

Reads (`get_execution`, `get_attempt`) come from `executions`/
`execution_attempts` rebuilt through `Execution::restore`/
`Attempt::restore` — never from journal replay (proven by the
journal-loss test: history deleted, state intact). The journal stays
history/audit evidence in the same `seq` order G2 uses; runtime facts
join that sequence.

## Restart Durability

Real-file fixtures: `E-1` with `A-1 Failed` + `A-2 Succeeded` survives
close/reopen with order, outcomes, providers, notes, events, and
event order intact and the Task still DRAFT; crash-style drop with `A-2`
open reopens with `A-1 Failed` and `A-2` open. Reads and reopens emit
no events.

## Corruption Handling

Deterministic matrix, all failing explicitly with no silent repair:
blank/invalid ids, bad provider refs, unknown outcomes, orphan
attempts/executions, gapped orders, duplicate positions (UNIQUE),
unsupported event versions, unknown event types, and every invalid
scope combination (outcome-less conclusion, outcome-ful creation,
Task states on runtime facts, runtime scope on G2 facts).

## G2 Regression

Full workspace suite green: Task persistence/lifecycle/dependencies,
journal, business IPC, v0/v1, peer auth, agentctl, completion
authority. Legacy migration tests now assert the `3 → 4` advance
(version literals updated, semantics unchanged); G2 rows survive the
rebuild verbatim.

## Deferred FakeAgent / Scheduler Work

No FakeAgent scenarios, no Scheduler decisions, no Runner/process
execution, no Git/worktrees, no IPC expansion (protocol stays `0, 1`),
no Session persistence, no Execution terminal lifecycle, no generic
payload bucket, no secrets. TASK-0036 drives this persistence through
`RuntimeService`; FakeAgent can never become `FakeAgent → SqliteStore`.

## Verification

- `cargo fmt --all -- --check`, `cargo check --workspace`,
  `cargo test --workspace --no-fail-fast`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Focused: `agent-runtime` (18), `app-services` (3 + 9 runtime + 14
  service), `storage-sqlite` (5 + 22 durability + 21 journal + 32
  runtime), `product-version` (4)
- `cargo tree -p agent-runtime` (task-domain only),
  `-p app-services`, `-p storage-sqlite` (no cycles; domain stays
  infrastructure-free)
