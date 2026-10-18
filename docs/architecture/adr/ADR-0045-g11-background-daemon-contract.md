# ADR-0045 — G11 Background daemon contract

- Status: **Accepted** (TASK-0189; G11 plan ratified by owner `go`)
- Date: 2026-10-18
- Gate: G11
- Extends: source-of-truth G11 (authorized work continues with UI closed),
  routine/watchdog guidance (§G11 Routine/Pulse Engine + Watchdogs)

## Problem

`agentd` is a foreground control server. Nothing defines what may keep
running when no UI or client is connected, how scheduled background work
is authorized, or what happens to in-flight work across a daemon restart.
Without a frozen vocabulary, background execution would become prompts
on timers with implicit authority — violating the rule that autonomous
execution never relies on prompt instructions as its security boundary.

## Decision

### 1. RoutineDefinition (normative fields)

| field | type contract |
|---|---|
| `id` | `rou_` + 26-char Crockford Base32 (ULID shape, time-sortable) |
| `name` | bounded text, 1..=1024 chars |
| `objective` | bounded text, 1..=16384 chars; the authorized work statement |
| `interval_seconds` | u64, 60..=2592000 (1 min..30 days); interval scheduling only in v1 |
| `capability_scope` | closed v1 enum: `READ_ONLY`, `WORKSPACE_WRITE`, `EXECUTION` |
| `budget_max_runs` | u32, 1..=10000; hard cap on total runs |
| `author_ref` | bounded text; creator of the definition |
| `authorized_by` | optional bounded text; set only by authorization (B-3) |
| `state` | closed enum: `PROPOSED`, `AUTHORIZED`, `DISABLED`, `EXHAUSTED` |
| `created_at`, `next_run_at` | Unix millis UTC; `next_run_at >= created_at` |
| `runs_completed` | u32, never exceeds `budget_max_runs` |

### 2. RoutineRun (normative fields)

| field | type contract |
|---|---|
| `id` | `run_` + 26-char Crockford Base32 |
| `routine_id` | owning `RoutineId` |
| `status` | closed enum: `RUNNING`, `COMPLETED`, `FAILED`, `BLOCKED`, `INTERRUPTED` |
| `started_at` | Unix millis UTC |
| `finished_at` | optional Unix millis UTC, `>= started_at` when present |
| `evidence_ref` | optional bounded text; what the run produced |

### 3. Authorization lifecycle (normative transitions)

- DEFINE: any author creates a `PROPOSED` definition. Proposed work
  never runs (B-1).
- AUTHORIZE: `PROPOSED -> AUTHORIZED` only by an authorizer different
  from `author_ref` (B-2, INV-029 analogue). Self-authorization refuses.
- DISABLE: `AUTHORIZED|PROPOSED -> DISABLED` (terminal for scheduling;
  history retained).
- EXHAUST: when `runs_completed == budget_max_runs`, state becomes
  `EXHAUSTED` automatically; no further runs (B-4).

### 4. Background eligibility (B-5)

A routine is due at `now` iff: state is `AUTHORIZED`, `next_run_at <= now`,
and `runs_completed < budget_max_runs`. Eligibility is a pure predicate;
the store and the daemon both fail closed on any other state. Work that
would need a new approval while running becomes a `BLOCKED` run and
escalates — it never self-approves (B-6).

### 5. Execution, supervision, reconciliation

- TICK: one due routine produces at most one run per tick (B-7).
  Completion advances `next_run_at = completion + interval` and
  increments `runs_completed`.
- DRAINING: a draining daemon refuses new runs; in-flight runs finish
  or are marked on shutdown (B-8).
- RESTART RECONCILIATION: runs left `RUNNING` by a dead daemon become
  `INTERRUPTED` at next startup, never silently re-executed (B-9).
  Side-effecting retries go through existing durable intent/reconcile
  machinery, not routine re-runs.
- WATCHDOG: `BLOCKED` and `INTERRUPTED` runs are operator-visible via
  list; there is no unbounded retry loop (B-10).

### 6. Daemon lifecycle

`agentd` remains the single local orchestration owner. Single-instance
is enforced by the existing endpoint ownership. `agentctl daemon
start|status|stop` maps to the existing start/ping/shutdown lifecycle;
routines execute inside `agentd` ticks, not in the CLI or UI.

### 7. Storage and IPC

SQLite tables `routines`, `routine_runs` (schema v35). Additive IPC
protocol V10 with `routine_core = 13`; sessions below V10 get a typed
refusal and never execute routine requests.

## Alternatives considered

- Cron expressions: rejected for v1; interval + run-now covers the
  gate objective without a new parser/policy surface.
- launchd/systemd registration in this gate: rejected; manual CLI
  lifecycle first, OS service registration is a follow-up.
- Re-running `RUNNING` runs after restart: rejected; duplicates side
  effects. `INTERRUPTED` + operator decision is the safe default.

## Consequences

Background work is durable, auditable and bounded. Operators can see
every run and its evidence. Unauthorized or self-authorized work can
never execute with the UI closed.

## Security impact

Authorization is a durable state transition by a distinct authorizer,
checked at tick time in the trusted service — not a prompt instruction.
Capability scope is recorded per routine and enforced as eligibility
metadata; execution itself still passes through existing Policy and
execution ports. No new network surface; no remote worker (G12) scope.

## Migration impact

Additive: schema v35 adds two tables; IPC V10 adds one envelope.
Older clients and daemons interoperate by typed refusal, as with V9.
