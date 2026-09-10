# G3 End-to-End Failure Matrix

> TASK-0038 evidence document. Proves the deterministic G3 runtime behaves
> safely for all eleven canonical FakeAgent scenarios through the trusted
> path `Task → schedule → Execution → Attempt → FakeAgent → conclude →
> submit/verify → Task state`. Architecture authority stays in
> `docs/architecture/task-engine.md`, `fake-agent.md`, `providers.md`,
> `runners.md`, `events.md`, ADR-0005/0009/0018/0022, and the TASK-0033/0035/
> 0036/0037 foundation. This document evidences; it decides nothing. No new
> TaskState, EventType, AttemptOutcome, migration, IPC, provider, runner,
> Git, policy, or G8 evaluator is introduced.

## Architecture Preflight

Sources read: `AGENT.md`, `product/version.toml`, `planning/roadmap.yaml`,
`planning/tasks/queue.yaml`, `INVARIANTS.md`, `task-engine.md`,
`providers.md`, `runners.md`, `fake-agent.md`, `events.md`,
ADR-0005/0009/0018/0019/0022, `crates/agent-runtime`, `crates/scheduler`,
`crates/evaluation`, `crates/app-services`, `adapters/fake-agent`,
`adapters/fake-evaluator`, `adapters/storage-sqlite`,
`docs/development/G3_FAKE_AGENT.md`,
`docs/development/G3_MINIMAL_ORCHESTRATION.md`.

Authoritative handling (accepted sources only):

- *Provider/runtime failure* — `providers.md` §10: provider failure != Task
  FAILED automatically; it feeds Attempt/Execution orchestration and the Task
  may retry or migrate (task-engine §12, INV-023). G3 shape:
  `Failed` Attempt + Task stays `RUNNING` (no transition invented, so no new
  edge is required).
- *Timeout* — canonical `AttemptOutcome::TimedOut` (ADR-0022, agent-runtime);
  `scheduler::interpret` maps `TimedOut → TimedOut`, no submission, no
  evaluator, Task stays `RUNNING`.
- *Cancellation* — canonical `Cancelled`; provider owns provider/session
  cancellation (providers §15), FakeAgent implements it via
  `ExecutionControl::cancel` (idempotent); `interpret` maps
  `Cancelled → Cancelled`, Task stays `RUNNING`, no second Attempt.
- *Malformed result* — `FakeAgentError::MalformedOutput` at the adapter
  boundary is never trusted success; `interpret` maps every non-timeout/
  non-cancel failure (incl. malformed) to `Failed`, not submittable, no
  evaluator, Task stays `RUNNING`.
- *Partial result* — `WorkEvidence.partial == true` is explicitly incomplete;
  `interpret` maps it to `Failed` (never silent success), detail preserved,
  no evaluator, Task stays `RUNNING`.
- *Quota/capacity failure* — accepted failure vocabulary (providers §§9–10:
  `QUOTA_EXHAUSTED` availability, quota-exhaustion failure evidence);
  `FakeAgentError::QuotaExhausted → Failed`, no retry, no provider switching
  or capacity routing (later gates), Task stays `RUNNING`. No distinct
  `BLOCKED` transition is defined for quota in G3, and none is invented:
  staying `RUNNING` claims no transition.
- *Policy-like simulated failure* — `forbidden_* → CapabilityDenied → Failed`
  as simulation only (fake-agent §5). No Policy Engine exists in G3; no
  enforcement is claimed. Task stays `RUNNING`.
- *Evaluation rejection* — accepted edge `VERIFYING → RETRY_READY` via
  `verification_failed` (task-engine §7, `ACCEPTED_TRANSITIONS`). Attempt
  stays `Succeeded`; success history is never rewritten as failure.
- *Retry eligibility* — accepted edge `RETRY_READY → RUNNING` via
  `TaskService::start` (domain + `ACCEPTED_TRANSITIONS`). The G3
  `OrchestrationService::schedule` exposes no retry entry point by design:
  `derive_readiness` admits only `READY`, so a second `schedule` from
  `RETRY_READY` (or `RUNNING`, or any terminal state) returns typed
  `NotRunnable` and creates nothing. Explicit recovery uses the existing
  `TaskService`/`RuntimeService` APIs directly (proven below); no new API
  was invented.

No architecture blocker: every required failure semantic above is already
defined narrowly for G3. No generalized `ReactionPolicy` engine was
introduced (future gates own it); only the existing `interpret` mapping +
trusted services are exercised.

## Scenario Mapping

| Scenario | Provider/runtime result | Attempt | Evaluator | Task reaction |
|---|---|---|---|---|
| success | valid complete evidence | Succeeded | yes (1 verdict) | verdict decides: Verified → COMPLETED, Rejected → RETRY_READY |
| crash | `Terminated` (no process crashed) | Failed | no | stays RUNNING, no retry |
| hang (cancelled) | `Cancelled` via explicit control | Cancelled | no | stays RUNNING, no second Attempt |
| hang (released) | complete evidence via release | Succeeded | yes | verdict decides (as success) |
| timeout | `TimedOut`, prompt, no sleep | TimedOut | no | stays RUNNING, no retry |
| malformed_result | `MalformedOutput`, never `Ok` | Failed | no | stays RUNNING, never submitted |
| forbidden_path_write | `CapabilityDenied{fs.write}` (not performed) | Failed | no | stays RUNNING, simulation only |
| forbidden_command | `CapabilityDenied{exec}` (not executed) | Failed | no | stays RUNNING, simulation only |
| quota_reached | `QuotaExhausted` (no retry) | Failed | no | stays RUNNING, no routing |
| network_failure | `NetworkFailure` (no connection) | Failed | no | stays RUNNING, no replay |
| partial_result | `Ok` with `partial=true` | Failed | no | stays RUNNING, never silent success |
| review_rejection | valid evidence + rejectable fixture (no authority) | Succeeded | yes | verdict decides (as success) |

Retry columns for every row: retry allowed only via explicit bounded
recovery through existing services (`RETRY_READY → RUNNING` + new
Execution/Attempt); retry automatic: never (one `schedule` creates at most
one Attempt). Durable evidence for every row: `execution.created` +
`attempt.created` + `attempt.concluded` (exactly one each) with
service-minted provenance, plus the Task lifecycle events for the path
taken (§Event Journal Evidence).

## Success

Full verified path reconfirmed
(`success_verified_end_to_end_is_exactly_once_and_survives_reopen`):
`READY → CLAIMED → RUNNING → E1/A1 → FakeAgent(success) → Succeeded →
SUBMITTED → AWAITING_REVIEW → VERIFYING → Verified → COMPLETED` with exactly
one Execution, one Attempt, one `attempt.concluded`, one evaluator call, the
11-event canonical journal, and full survival across DB reopen from canonical
owners (Task + Execution/Attempt reads, never journal replay).

## Crash

`crash_records_failed_attempt_with_no_evaluation_and_no_completion`:
`Terminated` (detail pins "no process crashed") → `Failed`, evaluator 0 even
with `AlwaysVerified`, Task stays `RUNNING`, no `Submitted`/verification
events, exactly `(1, 1, 1)` runtime facts. No real process crash (the suite
itself is the witness), no side effect, no submission.

## Hang

Three proofs: `hang_precancelled_is_cancelled_without_second_attempt`
(pre-cancelled control → `Cancelled`, evaluator 0, Task `RUNNING`,
`(1, 1, 1)` facts, durable after reopen);
`hang_release_becomes_submittable_evidence_not_a_failure` (pre-released
control → `Succeeded` → verified `COMPLETED`, evaluator 1 — release is
complete evidence, cancellation is the failure shape);
`hang_threaded_cancel_regains_orchestrator_control_with_no_mega_transaction`
(full regain proof: the worker blocks inside hang while a second connection
observes `RUNNING` + open `A1` with `execution.created`/`attempt.created`
present and `attempt.concluded` absent — so no giant transaction spans
FakeAgent execution — then `control.cancel()` concludes `Cancelled`
promptly within a 10s `recv_timeout` bound; exactly one Attempt exists
afterwards, Task stays `RUNNING`). No permanently hanging test: every block
is bounded by explicit control decisions plus receive timeouts.

## Timeout

`timeout_records_timed_out_with_no_retry_and_no_evaluation`: synchronous
`TimedOut` (no wall-clock sleep) → `TimedOut`, evaluator 0, Task `RUNNING`,
exactly one Attempt, no automatic Attempt 2, durable after reopen
(`RUNNING` + `TimedOut`).

## Malformed Result

`malformed_result_is_rejected_at_the_boundary_and_cannot_be_blessed`:
`MalformedOutput` never becomes `Ok`, never submitted, evaluator 0 even with
`AlwaysVerified`, no `Submitted`/`VerificationSucceeded`/`VerificationFailed`
events, Task stays `RUNNING`. The boundary rejects; nothing downstream can
bless it (§Evaluation Eligibility).

## Forbidden Path Write

`forbidden_path_write_mutates_no_filesystem_state`: `CapabilityDenied`
(fs.write, detail pins "not performed") → `Failed`, evaluator 0, Task
`RUNNING`, and `/forbidden/fake-agent-write` provably absent afterwards. No
Policy Engine instantiated, no shell, no G6 claim — simulation only.

## Forbidden Command

`forbidden_command_launches_no_process`: `CapabilityDenied` (exec, detail
pins "not performed" + "exec" + the `(not executed)` command text) →
`Failed`, evaluator 0, Task `RUNNING`. No process launched (no
`std::process`/runner surface anywhere in scheduler/app-services/
fake-agent/fake-evaluator; grep-verified), no Policy Engine claim.

## Quota Reached

`quota_reached_performs_no_retry_no_routing_and_no_completion`: `Failed`
with "quota" detail preserved, exactly one Attempt, evaluator 0, Task
`RUNNING`. No retry, no provider switching, no capacity routing. Per-execution
simulation state only: an independent `success` on another task in the same
test completes normally (no cross-execution quota latch).

## Network Failure

`network_failure_touches_no_network_and_never_replays`: `Failed` with "no
connection" detail, evaluator 0, Task `RUNNING`, `(1, 1, 1)` facts. No socket
opened (no `std::net`/HTTP surface in the four crates; error text pins "no
connection attempted"). No automatic replay: exactly one Attempt.

## Partial Result

`partial_result_stays_failed_and_distinguishable_from_success`: `Ok` with
`partial=true` → `Failed` (never silent success), evaluator 0, Task
`RUNNING`, detail pins "partial" so the incomplete shape stays observable.
The `interpret` unit test additionally pins `partial → Failed` at the mapping
layer.

## Review Rejection

`review_rejection_keeps_succeeded_attempt_and_never_completes_on_fixture`:
the `review_rejection` scenario is execution evidence only (rejectable
fixture marker, never a verdict). With `AlwaysRejected` → `RETRY_READY`,
Attempt stays `Succeeded`, never `COMPLETED`, durable after reopen; with
`AlwaysVerified` → `COMPLETED`. The verdict — not the fixture — decides, in
both directions. History is preserved: rejection never rewrites a `Succeeded`
Attempt into `Failed`.

## Evaluation Eligibility

`evaluator_runs_only_for_eligible_complete_evidence`: `Success` and
`ReviewRejection` (and hang-released) reach the evaluator exactly once;
`Crash`, `Timeout`, `MalformedResult`, both forbidden scenarios,
`QuotaReached`, `NetworkFailure`, `PartialResult`, and hang-cancelled reach
it zero times and never complete. `always_verified_evaluator_cannot_bless_
absent_or_malformed_work`: every non-submittable scenario paired with
`AlwaysVerified` still ends `RUNNING` with no `VerificationSucceeded` event.
The trusted orchestration layer enforces eligibility before invoking/applying
evaluation; absence of valid work can never be converted into completion.

## Completion Authority

`completion_requires_eligible_work_plus_verdict_plus_trusted_transition`
proves stepwise: FakeAgent success alone != COMPLETED (still `RUNNING`);
Attempt `Succeeded` alone != COMPLETED; `FakeEvaluator` object existence !=
COMPLETED (counter still 0, state unchanged); `submit_result` alone !=
COMPLETED (`SUBMITTED`); `VERIFYING` alone != COMPLETED; a `Verified` verdict
not yet applied through the trusted path != COMPLETED (still `VERIFYING`).
Only eligible submitted work + independent `Verified` verdict + trusted
`verification_succeeded` transition completes the Task. No code maps any
`AttemptOutcome` to `COMPLETED`.

## Retry Semantics

Hard G3 rule holds (`one_orchestration_invocation_creates_at_most_one_
attempt` over all eleven scenarios; every outcome asserts a single Attempt):
no `while failed { retry }`, no backoff, no failover, no hidden second
Attempt. `terminal_state_resists_blind_second_execution` proves a second
`schedule` with fresh identities creates nothing from `COMPLETED`
(`NotRunnable{Completed}`), from failed `RUNNING` (`NotRunnable{Running}` —
failure does not auto-retry), and from `RETRY_READY`
(`NotRunnable{RetryReady}` — the scheduler exposes no retry entry point, by
design). `explicit_retry_from_retry_ready_creates_a_new_attempt_and_
preserves_history` proves the one authorized bounded recovery: `A1 Succeeded`
+ rejected → `RETRY_READY`, then `start` (`RETRY_READY → RUNNING`) + new
`E2`/`A2` + FakeAgent success + `Succeeded` + submit/verify + `Verified` →
`COMPLETED`, with `A1` preserved byte-for-byte, `A2` separate (`E-2`/`A-2`
evidence), and `(2, 2, 2)` runtime facts. Stateful failover invariant
(INV-066) holds: the new Attempt carries new identities and new evidence —
never a silent replay of prior side effects (FakeAgent has none, and the
evidence refs prove distinctness for future real providers to inherit).

## Duplicate / Replay Protection

Exactly-once per terminal scenario: one `execution.created`, one
`attempt.created`, one `attempt.concluded` (asserted per scenario and swept
in `one_orchestration_invocation_creates_at_most_one_attempt` and
`no_scenario_expands_the_durable_taxonomy`). Repeat invocation appends no
events and creates no `E-2`/`A-2` rows for `COMPLETED`, failed-`RUNNING`, or
`RETRY_READY` (§Retry Semantics). Dependency blocking (`unfinished_
dependency_blocks_every_scenario_without_invocation`) holds for all eleven
scenarios: `READY` + unfinished dependency → `WaitingOnDependency` with zero
executor/evaluator calls, Task row untouched, `(0, 0, 0)` runtime facts.
Invalid states (`non_ready_states_invoke_nothing_and_create_no_runtime_
facts`) refuse from every reachable non-`READY` state (`DRAFT`, `CLAIMED`,
`RUNNING`, `BLOCKED`, `SUBMITTED`, `AWAITING_REVIEW`, `VERIFYING`,
`RETRY_READY`, `COMPLETED`) with zero invocations and no executions;
`FAILED`/`CANCELLED` have no accepted incoming edge (task-engine §16) so no
service can construct them — the scheduler census unit test covers those
states purely.

## Durable Intermediate States

`crash_between_phases_leaves_valid_inspectable_state` drives the existing
services to each interruption point with no new recovery logic: after
`RUNNING` (no Execution yet) → after `Execution` (zero attempts, no open
attempt) → after open `Attempt` (no outcome) → after conclusion (Task still
`RUNNING`, submission absent) → after submission/routing/verification entry
(`VERIFYING`, verdict absent). Each state is internally valid and inspectable,
and the final reopen proves every intermediate was already durable. The
threaded hang test independently proves the executor runs outside every
SQLite transaction (second connection reads the open Attempt mid-block).
`durable_reopen_matrix_for_each_outcome_class` reopens five classes
(verified success → `COMPLETED`/`Succeeded`; review rejection →
`RETRY_READY`/`Succeeded`; runtime `Failed` → `RUNNING`; `TimedOut` →
`RUNNING`; `Cancelled` → `RUNNING`) from canonical owners only — state is
never rebuilt from journal replay.

## Event Journal Evidence

Verified path journals exactly the 11 canonical events (`task.created` …
`verification.succeeded`, incl. the three ADR-0022 runtime facts); every
failure path journals exactly 7 (`task.created`, `marked_ready`, `claimed`,
`started`, `execution.created`, `attempt.created`, `attempt.concluded`) with
no submit/verify types (`event_journal_agrees_with_relational_state_and_
reads_create_nothing`). Runtime facts carry execution/attempt scope with
`from_state`/`to_state` null (Task-only columns), `attempt.concluded` carries
the canonical outcome, and every record bears trusted-service provenance
(`task-application-service/…` or `runtime-application-service/…` — never
adapter narrative). Reads (`get_task`, `get_execution`, `get_attempt`,
`list_events_ordered`) append nothing. No evaluator-specific event type
exists: the closed 15-type codec covers everything observed, and
`no_scenario_expands_the_durable_taxonomy` pins 12 TaskStates / 15 EventTypes
/ 4 outcomes / 11 scenarios with scenario names appearing in neither outcomes
nor events.

## Side-Effect Safety

`forbidden_path_write` leaves `/forbidden/fake-agent-write` absent;
`forbidden_command` launches nothing (no process/runner surface in the four
crates; command text carries `(not executed)`, detail carries
`(not performed)`); `network_failure` opens no socket (detail pins "no
connection attempted"); `crash` crashes no process (detail pins "no process
crashed"; the suite itself survives). Cancellation isolation
(`cancellation_affects_only_the_intended_execution`): cancelled hang on Task
A leaves Task B's success→verified completion untouched, with per-instance
evaluator counters (`0` vs `1`) and separate controls — no global FakeAgent
cancellation state. Mutation authority preserved: all Task mutations go
through `TaskService`, all Execution/Attempt mutations through
`RuntimeService` (orchestrator calls only those; grep over
`crates/scheduler`, `adapters/fake-agent`, `adapters/fake-evaluator` shows no
`rusqlite`/`storage-sqlite`/`SqliteStore`/journal writes outside doc
comments). Dependency structure: `cargo tree -p scheduler` and `-p fake-agent`
show only `agent-runtime` + `task-domain`; `-p fake-evaluator` shows only the
`evaluation` chain; neither fake crate depends on the other, on storage, or
on any SDK/IPC/Git/HTTP/Tauri surface.

## Known Issues

Flake watch: TASK-0035 `restart_durability_e2e` timed out once; TASK-0036 no
recurrence; TASK-0037 two full workspace runs green. TASK-0038: three full
workspace runs green (383/383 each, incl. the 5.0s `business_no_replay`
suite passing without timeout adjustment) plus focused per-crate runs green.
No recurrence observed → `knownIssue` stays non-recurring. No timeout was
weakened and no IPC/startup code was touched. Recommendation: no separate
reliability task required before G3 closure on this evidence; TASK-0039
re-verifies.

## G3 Closure Readiness

Classification: `READY_FOR_G3_CLOSURE_AUDIT` (assessment only; the gate is
not transitioned here — TASK-0039 owns closure).

- All 11 scenarios deterministically exercised end to end: yes (27-test
  `failure_matrix` suite + existing 13-test `orchestration` suite).
- No false completion: proven (§Completion Authority, §Evaluation
  Eligibility).
- No uncontrolled hang: pre-decided controls + 10s receive bounds; threaded
  regain proven.
- No real forbidden side effects: filesystem/process/network absence proven.
- No blind replay: repeat-invocation and dependency-block proofs.
- No automatic unbounded retry: at-most-one-Attempt proofs.
- Durable Attempt history correct incl. explicit-retry preservation.
- Evaluation authority independent (separate crate, counted invocations,
  verdict-not-fixture).
- Task/Runtime mutation boundaries preserved (service-only writes,
  dependency trees).
- All required failure semantics architecturally defined (§Architecture
  Preflight; no blocker, no `ReactionPolicy`, no new taxonomy).
- Workspace green (fmt, check, 383 tests, clippy).

## Verification

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p fake-agent
cargo test -p fake-evaluator
cargo test -p scheduler
cargo test -p evaluation
cargo test -p app-services
cargo test -p agent-runtime
cargo test -p storage-sqlite
cargo test -p product-version
cargo test -p app-services --test failure_matrix
cargo tree -p scheduler
cargo tree -p fake-agent
cargo tree -p fake-evaluator
```

Results (this task): `fmt` clean; `check` clean; workspace
`cargo test --no-fail-fast` 383 passed / 0 failed (agent-runtime 18,
agentctl 10, agentd 26, app-services lib 3 + failure_matrix 27 +
fake_agent_seam 3 + orchestration 13 + runtime_service 9 + service 14,
evaluation 1, fake-agent 10 + scenario_matrix 14, fake-evaluator 2,
ipc-contracts 16, ipc-control 35, ipc-transport 32, product-version 4,
scheduler 9, storage-sqlite 5 + durability 22 + journal 21 + runtime 32,
task-domain 42, plus small IPC suites); `clippy -D warnings` clean.
Focused: fake-agent 24, fake-evaluator 2, scheduler 9, evaluation 1,
app-services 69, agent-runtime 18, storage-sqlite 80, product-version 4.
Dependency trees: scheduler → agent-runtime + task-domain only; fake-agent →
agent-runtime + task-domain only; fake-evaluator → evaluation chain only
(no fake-agent/storage/rusqlite cross-edges). Schema stays v4
(`SUPPORTED_SCHEMA_VERSION == 4`, migrations `0001`–`0004` only); EventType
15, AttemptOutcome 4, TaskState 12, FakeScenario 11 — all pinned by
`no_scenario_expands_the_durable_taxonomy`. Product version advanced to
`0.1.0-dev.task0038` (public SemVer unchanged); queue
`completed_through: TASK-0038`, `current_gate: G3`, G3 `in_progress`;
TASK-0039 not started.
