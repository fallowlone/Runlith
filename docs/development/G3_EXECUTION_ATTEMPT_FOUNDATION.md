# G3 Execution/Attempt Foundation

> TASK-0033 — pure provider-neutral runtime-domain foundation
> (`crates/agent-runtime`). Implementation evidence, not architecture
> authority: task-engine.md, ADR-0005, providers.md, and fake-agent.md
> remain canonical.

## Ownership Model

```text
Task (task-domain: durable desired work, lifecycle, acceptance)
 └── Execution (agent-runtime: one durable run under an assignment)
      ├── Attempt 1 (one concrete try: provider ref + outcome, immutable)
      └── Attempt 2 (retry = NEW attempt, never a reset)
```

- `Execution { id: ExecutionId, task_id: TaskId, attempts: Vec<Attempt> }`.
  Exactly one Task owner, fixed at `begin()`; the Project follows
  indirectly through Task ownership (no `ProjectId` field).
- `Attempt { id, execution_id, provider: ProviderRef, outcome, detail }`.
  Exactly one Execution owner, fixed at `start_attempt()`; attempts
  never migrate.
- Attempts are sequential: at most one open attempt per execution
  (`AttemptAlreadyOpen` otherwise); identities are unique per
  execution (`DuplicateAttempt` rejected).

## Task vs Execution vs Attempt vs Session

- **Task** (task-domain): durable desired work; survives agent, provider,
  attempt, session, and process loss. Sole owner of lifecycle state.
- **Execution** (new): durable run container; owns identity + Task
  reference + ordered append-only attempt history. No lifecycle of its
  own beyond structure (later gates may add scheduler-visible state).
- **Attempt** (new): one try; open → concluded exactly once
  (`AttemptAlreadyConcluded` on rewrite, `AttemptNotFound` on unknown
  id). Concluded history is immutable: Attempt 1 stays failed after
  Attempt 2 succeeds (tested).
- **Session**: intentionally absent. ADR-0005’s `external_session_ref`
  is a persistence-level opaque concept; with no consumer yet, no
  session type was created. TASK-0034/0035 may add an attempt-scoped
  opaque ref without domain changes (Attempt identity is stable).

## IDs

`ExecutionId`, `AttemptId`, `ProviderRef`: validated opaque newtypes
(trim, non-blank, 256-byte bound) following the task-domain
conventions; no UUID/random generation (generation policy stays
outside the domain). `ProviderRef` uses ADR-0005’s vocabulary and
names the executor without branching on it. `TypeId` test proves
`TaskId != ExecutionId != AttemptId` at the type boundary.

## Runtime Lifecycle / Outcomes Implemented

Only the structural minimum plus one small outcome enum:

```text
AttemptOutcome = Succeeded | Failed | TimedOut | Cancelled
```

`Succeeded` = evidence produced (NOT Task completion); `Failed` =
try failed with adapter-minted cause in `detail`; `TimedOut` = no
progress within bound (timeout policy itself belongs to
orchestration); `Cancelled` = externally cancelled. `detail` is a
required non-blank opaque note (never transcript/secret). Conclusion
is exactly-once with outcome + detail committing together.

## Completion Authority

Hard rule, structurally enforced and tested: the crate holds no Task
handles, takes no `&mut Task`, and depends on task-domain for
identifiers only (no app-services/storage in the tree). A test runs a
full flow to `Succeeded` and proves the associated Task stays `DRAFT`
— and that even the test cannot complete it except through the
trusted evaluation transition (which the crate cannot invoke).
`Execution::complete_task()` / `Attempt::complete_task()` do not
exist and must never be added.

## Provider Neutrality

`cargo tree -p agent-runtime` = `agent-runtime → task-domain` only:
no Tokio, no IPC, no storage, no SDKs. No `if provider == …`
branches; no `ClaudeSession`/`CodexThread`-style types; outcome enum
carries no provider axis. Cause detail is adapter-owned opaque text.

## FakeAgent Compatibility

All 11 canonical scenarios remain representable (proven by test, each
recorded as an opaque detail on the fitting outcome — scenario names
stay adapter-owned, never domain variants):

```text
success → Succeeded; crash/hang/timeout → Failed/TimedOut;
malformed_result, forbidden_path_write, forbidden_command,
quota_reached, network_failure, partial_result,
review_rejection → Failed (+detail)
```

No FakeAgent fields, states, bypasses, or direct Task mutation exist.

## Deferred to TASK-0034+

Persistence (tables/migration 0004), runtime Event Journal types,
opaque session/provider refs at rest, Scheduler (claiming, retry
policy, reconciliation), FakeAgent adapter, evaluation wiring,
runners, Git/worktrees, IPC expansion, real providers. The
`outcome_vocabulary_stays_small_and_neutral` test pins the 4-variant
enum so later tasks extend it only with architecture authority.

## Verification

- `cargo test -p agent-runtime`: 15/15 (IDs, ownership, sequential
  discipline, retry-history immutability, completion separation,
  type distinctness, scenario representability, error display).
- `cargo tree -p agent-runtime`: task-domain only.
- Full workspace fmt/check/test/clippy green; G1/G2 suites unbroken.
