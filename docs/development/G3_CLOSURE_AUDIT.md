# G3 Closure Audit

> TASK-0039 — independent gate verification for G3 (FakeAgent Runtime).
> Audit, not implementation: no production code was changed, no defect was
> repaired to manufacture the verdict, and no G4 work was begun. Evidence
> comes from current source, tests, contracts, migrations, planning
> metadata, dependency graphs, and executed validation — prior task reports
> were treated as historical pointers only and every material claim below
> was re-verified against the working tree.

## Verdict

```text
G3_COMPLETE
```

All G3_COMPLETE criteria hold (§Gate Transition). No BLOCKER was found.
One OBSERVATION is recorded (§Findings); it describes G2-accepted behavior
and does not affect closure.

## Gate Objective

The canonical G3 target is satisfied end to end with no shortcut in the
authority chain:

```text
Task → scheduling readiness (derived) → trusted Task lifecycle mutation
→ Execution → Attempt → FakeAgent → provider/runtime result
→ Attempt conclusion (RuntimeService) → submit_result (TaskService)
→ independent deterministic evaluation (FakeEvaluator)
→ trusted verification transition → COMPLETED / RETRY_READY
```

Proven by `crates/app-services/tests/orchestration.rs`
(`verified_end_to_end_flow_survives_db_reopen`, 11-event journal, reopen
from canonical owners) and the 27-test
`crates/app-services/tests/failure_matrix.rs` matrix over all eleven
scenarios. No real AI provider exists anywhere in the tree (§Dependency
Boundaries, §Security / Side-Effect Scope).

## Tasks Audited

| Task | Scope | Evidence re-verified |
|---|---|---|
| TASK-0033 Execution/Attempt Domain Foundation | `crates/agent-runtime` pure domain | ownership, sequential discipline, 4-variant outcome, completion separation (18 unit tests) |
| TASK-0034 Runtime Event Journal Contract | ADR-0022 + schema + `events.md` §4.2 | three runtime types, scope matrix, version/provenance rules; contract-only, no code |
| TASK-0035 Persistence + Runtime Events | migration 0004 + `RuntimeService` + codec | atomic commits, trigger-injected rollback (6 tests), journal-loss state authority, corruption matrix |
| TASK-0036 Deterministic FakeAgent | `adapters/fake-agent` + scenario matrix | 11 scenarios behavioral (14 tests), determinism, cancellation, side-effect safety |
| TASK-0037 Minimal Scheduler + Evaluation | `crates/scheduler`, `crates/evaluation`, `OrchestrationService`, `adapters/fake-evaluator` | readiness derivation, interpret mapping, verified/rejected paths, completion authority (13 flow tests) |
| TASK-0038 Failure Matrix | `failure_matrix.rs` (27 tests) + `G3_FAILURE_MATRIX.md` | all scenarios end to end, eligibility, retry-as-new-attempt, reopen matrix, intermediate states |

## Task Lifecycle

The canonical lifecycle is exactly the accepted twelve states with the
eleven accepted edges — verified in `crates/task/src/state.rs`
(`ACCEPTED_TRANSITIONS: [(TaskState, TaskState); 11]`, exhaustive
matrix test, `only_verifying_reaches_completed`,
`failed_and_cancelled_have_no_incoming_edges`,
`terminal_states_have_no_outgoing_edges`). No FakeAgent/scheduler-specific
`TaskState` exists (census pins 12; `no_scenario_expands_the_durable_
taxonomy` re-pins after every scenario). Lifecycle state != scheduling
readiness: `derive_readiness` (`crates/scheduler/src/readiness.rs`) is a
pure function returning `Runnable` only for `Ready` with no blockers;
readiness is never persisted into the Task row (unit test
`readiness_derivation_never_mutates_the_task`).

## Completion Authority

Hard criterion — proven in source and tests. `TaskState::Completed` is
reachable only through the `VerificationSucceeded` command gated by
`can_transition` (only from `Verifying`), committed solely by
`TaskService::verification_succeeded`. Repository-wide search for
shortcuts (`complete_task`, `set_state`/`set_task_state`,
`Succeeded → Completed` mappings, `FakeScenario::Success → Completed`)
returns only doc comments describing their absence plus the single
legitimate `UPDATE tasks SET state … WHERE state = ?` compare-and-swap in
`task_store.rs`. The only production caller completing tasks on the G3
path is `OrchestrationService::schedule`, and only after an independent
`EvaluationVerdict::Verified`. Stepwise test proof
(`completion_requires_each_step_and_only_the_verdict_completes` in both
`orchestration.rs` and `failure_matrix.rs`): FakeAgent success alone,
Attempt `Succeeded` alone, evaluator-object existence alone,
`submit_result` alone, `VERIFYING` alone, and an unapplied `Verified`
verdict each leave the Task uncompleted. The `verify-succeed`
agentctl/agentd surface pre-existed at the G2 baseline commit (verified via
`git show HEAD`) and belongs to the accepted local-user trust model
(task-engine.md §15); it is recorded as an OBSERVATION, not a G3 finding.

## Execution / Attempt Model

`Task != Execution != Attempt != Session` is structural, not prose.
`Execution { id, task_id, attempts }` fixes one Task owner at `begin()`;
`Attempt { id, execution_id, provider, outcome, detail }` fixes one
Execution owner at `start_attempt()`; attempts never migrate; at most one
open attempt per execution (`AttemptAlreadyOpen`); identities unique per
execution (`DuplicateAttempt`); conclusion exactly-once
(`AttemptAlreadyConcluded` — history can never be rewritten, proven by
`Failed → Succeeded` rewrite tests at domain, storage, and service
levels). Retry is a new row, never a reset (explicit-retry test preserves
A1 byte-for-byte beside A2). Session is transient by construction:
`FakeSessionHandle` is an in-memory opaque value; no session table exists
(migrations create only `projects/tasks/task_dependencies/task_strings/
executions/execution_attempts/event_journal*`); no Session event type
exists; the JSON schema contains no session surface.

## Runtime Persistence

Schema version `4` (`SUPPORTED_SCHEMA_VERSION = 4`); migration history
exactly `0001/0002/0003/0004` (0004 added by G3; 0001–0003 byte-identical
to the G2 baseline — `git diff HEAD` on the migrations directory is
empty). `executions(id, task_id)` and
`execution_attempts(attempt_id, execution_id, position, provider_ref,
outcome, detail)` are the relational current-state authority with FK
ownership, explicit validated position order, and outcome/detail committed
together (a row with exactly one is corruption). No parallel authoritative
runtime journal exists — one Event Journal (ADR-0006/0022). Normal loads
(`get_execution`/`get_attempt` via `Execution::restore`/`Attempt::restore`)
never touch the journal (proven by the journal-loss test: history deleted,
state intact; plus reads-emit-no-events assertions).

## Event Journal Contract

ADR-0022 is reflected consistently across ADR text, `events.md` §4.2, the
JSON schema conditionals (per-family required/null matrix plus the
`succeeded/failed/timed_out/cancelled` outcome enum), SQL CHECKs
(migration 0004), the closed Rust `EventType` (exactly 15 variants: 9
lifecycle + `project.created`/`task.created`/`task.dependency_added` +
`execution.created`/`attempt.created`/`attempt.concluded`), the journal
`AttemptOutcome` codec (4 variants, lowercase, 1:1 with the domain), and
tests at every layer. Runtime types all start at `event_version = 1`;
unknown types/versions fail closed (no `Other`, no silent v1-of-v2).
No FakeAgent scenario name appears in `EventType`, `AttemptOutcome`, or
`TaskState` (pinned by unit tests and the full-matrix taxonomy sweep).

## Scheduler / Readiness

`derive_readiness`: `READY` + no blockers → `Runnable`; `READY` +
unfinished dependency → `WaitingOnDependency` (Task row untouched);
anything else → `NotRunnable{state}` (12-state census unit-tested).
Blocked/not-runnable paths produce no Execution, no Attempt, no executor
invocation, no runtime event — proven for all eleven scenarios
(`unfinished_dependency_blocks_every_scenario_without_invocation`) and for
every reachable non-READY state including `BLOCKED` and `RETRY_READY`.
Repeat `schedule` observes state (`NotRunnable`) instead of duplicating
work, for `COMPLETED`, failed-`RUNNING`, and `RETRY_READY` alike — the
single-process deterministic boundary never blindly duplicates; no
distributed lease semantics are claimed. The scheduler is minimal (five
small files; grep for planner/router/cost/decompose/DAG/daemon returns
nothing): no adaptive planner, provider ranking, decomposition, or
background reconciliation.

## FakeAgent

`cargo tree -p fake-agent` ⇒ `agent-runtime` + `task-domain` only. Source
search over the adapter confirms: no `storage-sqlite`/`rusqlite`/
`app-services` dependency, no shell/process/network/Git surface, no
session persistence, no journal append (pinned: an 11-scenario run leaves
journal length unchanged), no Task mutation, no authoritative conclusion
(the adapter returns `Result<ExecutionOutput, FakeAgentError>`; only
`RuntimeService::conclude_attempt` concludes), no internal retry (one
terminal behavior per call). Hang parks on `ExecutionControl` with
idempotent release/cancel and every blocking test bounded by
`recv_timeout`; `timeout` is synchronous and needs no thread.

## Independent Evaluation

`evaluation` defines the port + request + two verdicts only; its doc
header states it implements no judge/score/test-runner. `cargo tree -p
fake-evaluator` ⇒ the `evaluation` chain only — no `fake-agent`, storage,
services, or `rusqlite`. One type never implements both roles. Verdicts
are per-instance counted, with no shared global state (two evaluators in
one process never interfere — tested). No G8 engine exists: source search
for LLM-judge/test-runner/security-evaluator/benchmark/council/
reliability-scoring implementations returns only doc words. Evaluator `Err`
leaves the Task `VERIFYING` with evidence intact — reported, never
retried inside the cycle.

## Eleven-Scenario Matrix

All eleven canonical scenarios are behaviorally exercised through real
`FakeAgent::execute` (never mere enum parsing): 14 adapter-level tests
(determinism pairwise per scenario, cancellation idempotence, session
transience, error taxonomy) + 27 orchestration-level `failure_matrix`
tests + 13 `orchestration` flow tests. Mapping: success/review-rejection/
hang-released → `Succeeded` (evaluator decides); crash/quota/network/
forbidden×2/malformed/partial → `Failed`; timeout → `TimedOut`;
hang-cancelled → `Cancelled`; every failure leaves the Task `RUNNING`
with exactly `(1, 1, 1)` runtime facts and zero evaluator calls.

## Failure / Retry Semantics

No false completion (eligibility enforced before any verdict is sought or
applied; always-verified evaluator cannot bless malformed/absent work —
dedicated tests). No uncontrolled hang (explicit control decisions +
timeouts; threaded regain proven). No real network call, forbidden command
execution, or filesystem side effect (absence assertions + no such surface
in source). No blind replay (repeat-invocation and dependency proofs). No
implicit provider failover or automatic unbounded retry (at-most-one-
Attempt swept over all scenarios; quota cross-execution independence
tested). Retry ⇒ new Attempt with new identities and new evidence
(INV-066 honored; A1 preserved, A2 separate, `(2, 2, 2)` facts).
Malformed and partial outputs can never become trusted success (§Completion
Authority, dedicated tests). Review rejection keeps `Succeeded` history
while the Task follows `VERIFYING → RETRY_READY` (both verdict directions
tested, durable after reopen).

## Durable Intermediate States

Five interruption points verified through existing services with no new
recovery logic: `RUNNING` without Execution; Execution without Attempt;
open Attempt before result; concluded Attempt before submission (Task still
`RUNNING`); `SUBMITTED → AWAITING_REVIEW → VERIFYING` before verdict —
each internally valid, inspectable, and durable across reopen. The
threaded hang test proves no giant transaction spans FakeAgent execution
(second connection reads the open Attempt mid-block). Reopen matrix covers
verified success, review rejection, `Failed`, `TimedOut`, `Cancelled`,
and multi-attempt explicit retry — relational tables authoritative, journal
as history/audit only. Corruption fails explicitly across the matrix (bad
ids, unknown outcomes/types/versions, broken ownership, gapped/duplicate
ordering, every invalid scope combination); no silent defaults, no replay
repair.

## Dependency Boundaries

All seven trees verify hexagonal direction: `agent-runtime → task-domain`
only; `scheduler`/`evaluation` → domain contracts only;
`fake-agent` → runtime + task-domain; `fake-evaluator` → evaluation only;
`storage-sqlite` adds only `rusqlite`; `app-services` alone composes
services + storage + scheduler + evaluation (the composition root side).
No domain crate depends on SQLite, providers, Tauri, or HTTP. No
`FakeAgent ↔ FakeEvaluator` edge in either direction. No `SqliteStore`
usage in scheduler/evaluation/fake crates (grep-verified; only doc
comments mention the forbidden targets).

## G2 Regression

G3 damaged nothing from G2: full workspace suites green across two runs —
`agentctl` 25 (unit 10 + `task_core_matrix` 13 + `business_no_replay` +
`real_process_matrix`), `ipc-contracts` 16, `ipc-control` 35,
`ipc-transport` 32, storage `durability` 22 / `journal` 21, task-domain
42, product-version 4. IPC protocol unchanged (no new RPCs; the sole
`negotiation.rs` diff vs baseline is the TASK-0031 test-only literal
removal). Migrations 0001–0003 untouched; v3→v4 migration preserves G2
facts bit-for-bit (dedicated test). G1/G2 compatibility suites pass
unmodified.

## Security / Side-Effect Scope

Secret/credential search over all G3 production source returns only
"never credentials/secrets" doc assertions — no `api_key`, tokens,
OAuth, passwords, cookies, telemetry, account, or cloud-sync surface;
FakeAgent requires no credentials. Forbidden scenarios are simulations
with absence proofs; no Policy Engine, runner, sandbox, or container code
was introduced and none is claimed. The `microsoft/tgrep` research files
were left untouched per task instructions; their content (provider/CLI
research notes) does not intersect G3 runtime behavior and affects no
verdict. Journal payloads carry bounded evidence refs, never transcripts,
prompts, or secrets.

## Test Integrity

No weakened assertions found: the TASK-0031 literal removals replaced
historical task-number literals with strictly stronger dynamic
self-consistency checks (reviewed diff — semantics unchanged, still
failing on real drift). No `#[ignore]`, no retry-until-pass, no flaky
tolerance in G3 tests; the only sleeps are bounded coordination quanta
(5–50ms polls, 10s `recv_timeout` fail-loud bounds). Coverage is
behavioral, not mirrored internals: orchestration tests assert durable
state, journal facts, invocation counts, and reopen survival rather than
implementation structure. Quantity is not the argument — the authority,
eligibility, atomicity, and durability proofs above are.

## Flake Review

History: TASK-0035 `restart_durability_e2e` daemon-startup timeout fired
once; TASK-0036/0037/0038 reported no recurrence (2+3 full runs). TASK-0039
ran the full workspace suite twice more (383/383 both runs, including the
5s `business_no_replay` suite) plus all focused suites — no timeout, no
flake, no ordering sensitivity observed. The historical event is therefore
classified as a non-blocking isolated test flake (single occurrence,
daemon-startup timing under load, unrelated to G3 runtime paths), not a
G3/G2 reliability blocker. No timeout value was weakened in any task.

## Deferred By Design

G4 Git/worktrees, G5 real providers (`AgentProviderPort` Rust shape stays
deliberately open per providers.md §16 / fake-agent.md §8 — explicitly not
a G3 blocker), G6 Policy/Secrets/Audit detail, G7 planning, G8 full
verification, G9+ context/memory/daemon/remote/cloud, plus ReactionPolicy
generalization, Execution terminal lifecycle, Session persistence, and
scheduler retry entry points. None is debt; each has its gate and several
have coverage-register entries (COVERAGE.md) with just-in-time ADR
scheduling.

## Findings

| Finding | Classification | Evidence | Closure impact |
|---|---|---|---|
| `verify-succeed` / `VerificationSucceeded` reachable via agentd dispatch + agentctl CLI by the local OS user | OBSERVATION | `bins/agentd/src/dispatch.rs:181-183`, `bins/agentctl/src/main.rs:480`; present at G2 baseline (`git show HEAD`); consistent with task-engine.md §15 user authority; G6 policy may later restrict | None — G2-accepted trust model, FakeAgent path unaffected |
| Single historical daemon-startup timeout (TASK-0035) never recurred across 7+ subsequent full runs | OBSERVATION | flake history + two 383/383 TASK-0039 runs | None — isolated flake, no action |
| `AgentProviderPort` final Rust shape still open | DEFERRED_BY_DESIGN | providers.md §16, fake-agent.md §8, COVERAGE.md | None — G5 gate owns it |
| No scheduler retry entry point (`RETRY_READY` → `NotRunnable` from `schedule`; recovery via existing `TaskService`/`RuntimeService` APIs) | DEFERRED_BY_DESIGN | `readiness.rs:41-50`, repeat-invocation + explicit-retry tests | None — later orchestration gates own ergonomic retry |
| No BLOCKERs, no NON_BLOCKING_DEBT | — | all audits above | G3_COMPLETE |

## Verification

```bash
cargo fmt --all -- --check                                    # clean
cargo check --workspace                                      # clean
cargo test --workspace --no-fail-fast                        # 383/383, twice
cargo clippy --workspace --all-targets --all-features -- -D warnings  # clean
cargo test -p agent-runtime        # 18
cargo test -p scheduler            # 9
cargo test -p evaluation           # 1
cargo test -p fake-agent           # 24 (10 unit + 14 matrix)
cargo test -p fake-evaluator       # 2
cargo test -p app-services         # 69 (3 + 27 failure_matrix + 3 seam + 13 orchestration + 9 runtime + 14 service)
cargo test -p storage-sqlite       # 80 (5 + 22 durability + 21 journal + 32 runtime)
cargo test -p product-version      # 4
cargo test -p agentctl             # 25
cargo tree -p agent-runtime -p scheduler -p evaluation -p fake-agent -p fake-evaluator -p app-services -p storage-sqlite
```

Workspace total 383 passed / 0 failed on both runs (16 zero-test doc-target lines excluded from the sum; every real suite green). Dedicated suites (`orchestration`, `failure_matrix`, `runtime`, `journal`, `durability`) all execute inside the package runs above with per-suite counts confirmed. Per-scenario, per-outcome, reopen, and taxonomy assertions are enumerated in §§Eleven-Scenario Matrix–Durable Intermediate States.

## Gate Transition

```text
TASK-0039 complete
G3 = complete
G4 = in_progress
current_gate = G4
product development = 0.1.0-dev.task0039
product task = TASK-0039
product gate = G4
```

Applied to `product/version.toml` (SemVer stays `0.1.0`),
`planning/roadmap.yaml` (G3 `completed`, G4 `in_progress`, `current_gate:
G4`), and `planning/tasks/queue.yaml` (`completed_through: TASK-0039`,
`current_gate: G4`; TASK-0038/0039 items preserved). No G4 implementation
was begun — no worktree/branch/diff/commit/merge surface was added.
