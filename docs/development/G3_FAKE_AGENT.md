# G3 Deterministic FakeAgent

> TASK-0036 — implementation evidence. Architecture authority stays in
> `docs/architecture/fake-agent.md`, `docs/architecture/providers.md`,
> ADR-0005/0009/0018/0022, and the TASK-0033/0035 runtime foundation.
> This document evidences; it decides nothing.

## Contract Implemented

- Scenarios: exactly the eleven canonical names from fake-agent.md §3
  (`success`, `crash`, `hang`, `timeout`, `malformed_result`,
  `forbidden_path_write`, `forbidden_command`, `quota_reached`,
  `network_failure`, `partial_result`, `review_rejection`).
- Provider seam: ADR-0009 anti-corruption direction
  (`FakeAgent → provider/runtime contracts`, never → services/SQLite);
  conceptual `start/send/cancel` honored via `execute` + `ExecutionControl`.
- Durable outcomes: the TASK-0033 four-variant `AttemptOutcome`
  (`Succeeded | Failed | TimedOut | Cancelled`) — FakeAgent reports
  behavior; only `RuntimeService::conclude_attempt` concludes.
- Journal: ADR-0022 — three runtime types, service-minted provenance;
  FakeAgent appends nothing.
- Completion: ADR-0018 — `submit_result`, never `complete_task`;
  `attempt.concluded(succeeded) != Task COMPLETED`.

## Architecture Boundary

```text
Test harness / future Scheduler
 ↓  ExecutionRequest { task_id, execution_id, attempt_id } (identities only)
FakeAgent::for_scenario(s) + ExecutionControl
 ↓  Result<ExecutionOutput, FakeAgentError> (one shot, no retry)
Harness interprets → RuntimeService::conclude_attempt(...) → durable facts
```

Preflight (accepted sources only) answered:

- *What receives an execution request?* The provider-facing execution
  seam (ADR-0009 port: detect/capabilities/start/send/cancel). The final
  Rust `AgentProviderPort` shape is an open design item (providers.md
  §16; fake-agent.md §8), so no canonical port was frozen here: the
  adapter takes a minimal identity-only `ExecutionRequest` — the same
  shape future real adapters receive — with the `FakeScenario` selector
  wrapping it at the adapter/test layer.
- *What result is returned?* Adapter-scoped `Result<ExecutionOutput,
  FakeAgentError>` reusing the accepted failure vocabulary
  (providers.md §§9–10). `ExecutionOutput` is the smallest operational
  evidence (bounded summary ≤1024B, `partial` flag, deterministic
  evidence ref); no chain-of-thought, transcript, secret, or unbounded
  dump.
- *What is transient Session state?* ADR-0005 / providers.md §6: a
  temporary provider-specific entity inside an Attempt. `FakeSessionHandle`
  is an opaque in-memory value (`fake-session:<execution>:<attempt>`),
  minted per execution and dropped — no table, no journal event.
- *Who owns cancellation?* The provider level owns provider/session
  cancellation (providers.md §15); `ExecutionControl::cancel()` is its
  deterministic FakeAgent implementation (idempotent; hang → `Cancelled`).
- *Who maps failure into AttemptOutcome?* The trusted application
  service via `conclude_attempt` (events.md §4.2 provenance authority).
  FakeAgent performs no mapping with authority; seam tests pin the
  harness-owned interpretation.

No blocker: the open `AgentProviderPort` Rust-shape items stay open for
their gate; nothing here forecloses them. No new canonical crate was
created — result/request/session types are explicitly adapter-scoped
inside `adapters/fake-agent`, so future `ClaudeAdapter`/`CodexAdapter`/…
replace this adapter without touching Task domain semantics.

## Scenario Matrix

| Scenario | Behavior | Distinguisher |
|---|---|---|
| `success` | `Ok` complete output | `partial=false`, `rejectable=false` |
| `crash` | `Err(Terminated)` | kind `terminated` |
| `hang` | blocks until release/cancel | release→`Ok`, cancel→`Err(Cancelled)` |
| `timeout` | `Err(TimedOut)` synchronously | kind `timed_out`; needs no second thread |
| `malformed_result` | `Err(MalformedOutput)` always | never `Ok`; cannot become trusted success |
| `forbidden_path_write` | `Err(CapabilityDenied{fs.write,…})` | no write performed |
| `forbidden_command` | `Err(CapabilityDenied{exec,…})` | no shell launched |
| `quota_reached` | `Err(QuotaExhausted)` | no retry; no cross-execution state |
| `network_failure` | `Err(NetworkFailure)` | no connection attempted |
| `partial_result` | `Ok` with `partial=true` | typed flag + distinct evidence ref |
| `review_rejection` | `Ok` complete + `rejectable_fixture=true` | fixture data, never a verdict |

Scenario names never enter `AttemptOutcome`, `EventType`, Task
lifecycle, or SQLite (pinned by unit test). Error kinds are
provider-neutral diagnostics on a distinct type — six of seven are not
even parseable as scenario names.

## Determinism

Same scenario + request + control ⇒ identical result (asserted pairwise
for every scenario, including triple-run for `malformed_result` and
replay for `partial_result`). No randomness, no wall-clock races, no
network, no machine-specific behavior, no provider availability. Default
behavior is per-execution: `quota_reached` on one execution leaves an
independent `success` untouched (tested).

## Cancellation / Hang Semantics

`hang` parks on `ExecutionControl` (mutex + condvar + atomic cancel
flag, 5ms poll quantum for wake-up latency only — correctness comes
from the explicit test decision). `release()` → deterministic released
`Ok`; `cancel()` → `Err(Cancelled)`; repeated `cancel()` stays
cancelled. No uncontrolled infinite loop: every blocking test receive
uses `recv_timeout(10s)`, so a regression fails instead of hanging the
suite. `timeout` is the prompt counterpart — synchronous, no sleep —
hence always distinguishable from `hang`.

## Result / Error Semantics

`ExecutionOutput::new` validates: non-blank bounded summary and
evidence ref, and `rejectable_fixture` only on complete results (a
partial result is already non-success by construction). `FakeAgentError`
carries seven adapter-scoped variants; forbidden scenarios report the
attempted path/command as evidence text only. Sync only: no Tokio was
added (agent-runtime/app-services are sync; Tokio stays an IPC detail).

## Persistence Separation

`cargo tree -p fake-agent` ⇒ `fake-agent → agent-runtime, task-domain`
only. No `rusqlite`, `storage-sqlite`, `app-services`, `RuntimeService`,
`TaskService`, IPC, Tauri, HTTP, provider SDK, or git. Seam composition
(`RuntimeService` + `FakeAgent`) lives in
`crates/app-services/tests/fake_agent_seam.rs` — a dev-dependency of the
test target only — proving the harness-owned direction. No migration
(schema stays 4), no session persistence, no journal appends from any
scenario (pinned: full 11-scenario run leaves journal length unchanged).

## Completion Authority

FakeAgent holds no Task handle and exposes no
`submit_result`/`verification_*`/`complete_task` path (grep-verified:
none of `complete_task`, `verification_succeeded`, `submit_result`,
`set_state` appear in the crate). Seam tests conclude `Succeeded` via
the service and prove the Task stays `DRAFT` with zero lifecycle events
beyond creation; crash maps to `Failed` while the Task stays `DRAFT`.

## Side-Effect Safety

`forbidden_path_write` asserts `/forbidden/fake-agent-write` still
absent after execution; `forbidden_command` never spawns a process (the
command text carries `(not executed)`); `network_failure` opens no
socket; `crash` crashes no process. No Runner, container, sandbox,
worktree, branch, or IPC surface of any kind.

## Deferred Scheduler / Evaluation

Not implemented (TASK-0037 owns them): no READY-scan, claiming,
provider selection, retry, backoff, worker pool, reconciliation, test
runner, reviewer, or acceptance evaluator. `review_rejection` returns
valid work plus the rejectable marker precisely so TASK-0037 can wire
Submit → Verify without FakeAgent ever reviewing.

## Verification

- `cargo test -p fake-agent`: 10 unit + 14 matrix = 24/24.
- `cargo test -p app-services --test fake_agent_seam`: 3/3.
- Full workspace `fmt --check`, `check`, `test --no-fail-fast`,
  `clippy --all-targets --all-features -D warnings`: green (recorded
  in the task result); G1/G2/G3 persistence suites unbroken.
- `cargo tree -p fake-agent`: `agent-runtime` + `task-domain` only.
- Flake watch (TASK-0035 `restart_durability_e2e` daemon-startup
  timeout): noted in the task result `knownIssues` only if observed
  during validation runs.
