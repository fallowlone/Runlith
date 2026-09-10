# G3 Minimal Scheduler + Evaluation Flow

> TASK-0037 evidence document. First complete deterministic G3 execution
> path with no real AI, no Git/worktree, no Runner, no external IPC, and no
> G8 Evaluation Engine. Architecture decisions: ADR-0017 (proposal-only
> planning), ADR-0018 (independent evaluation), ADR-0022 (runtime events).

## Scheduling Readiness

Derived, never persisted. `scheduler::derive_readiness(state, blocking)`
maps authoritative Task/dependency state onto the canonical
`task-engine.md` §8 vocabulary:

```text
READY + all dependencies COMPLETED → RUNNABLE
READY + unfinished dependency       → WAITING_ON_DEPENDENCY
any other lifecycle state           → NOT_RUNNABLE (incl. TERMINAL)
```

A `WAITING_ON_DEPENDENCY` task keeps its `READY` row: no claim, no
Execution, no Attempt, no executor invocation, no runtime event (proven by
`unfinished_dependency_blocks_without_runtime_facts`). The full 12-state
census is unit-tested in `crates/scheduler`.

## Orchestration Boundary

`OrchestrationService::schedule(task_id, ids, executor, evaluator)` runs
one bounded cycle for one explicitly requested Task. It mutates nothing
directly: every Task change goes through `TaskService`, every
Execution/Attempt change through `RuntimeService`. There is no planner
(no task creation/decomposition), no provider router (one injected
`ExecutionPort` + its `provider_ref`), no retry loop, no policy engine.
Outcome is a bounded `OrchestrationOutcome` (evidence for the caller),
never a second lifecycle authority.

## Runtime Integration

Runnable path commits incrementally, each step its own transaction:

```text
claim (READY→CLAIMED) → start (CLAIMED→RUNNING)
→ create_execution → create_attempt
→ [executor runs outside every SQLite transaction]
→ conclude_attempt → submit_result → route_to_review
→ begin_verification → verification_succeeded | verification_failed
```

Caller-supplied deterministic `OrchestrationIds` reuse the existing
validated ID types; no new ID scheme, no random/UUID dependency.
Duplicate `schedule` calls observe state (`NotRunnable`) instead of
duplicating work (`second_schedule_observes_state_and_duplicates_nothing`).

## FakeAgent Boundary

FakeAgent stays persistence-independent: the test-side `FakeExecutor`
adapts `FakeAgent::{execute, FakeAgentError}` onto the provider-neutral
`ExecutionPort` (`WorkEvidence` / `ExecutionProblem`), matching on error
variants — never on scenario names. `scheduler::interpret` maps behavior
to `AttemptOutcome` using the existing four-variant contract only:

```text
complete evidence → Succeeded (submittable)
partial evidence  → Failed (preserved, not submitted)
TimedOut          → TimedOut
Cancelled         → Cancelled
any other failure → Failed (detail preserved)
```

## Evaluation Independence

`evaluation::EvaluationPort` is the provider-neutral seam with exactly two
verdicts (`Verified` / `Rejected`). `FakeEvaluator` (`adapters/`) is a
separate type with its own deterministic mode (`AlwaysVerified` /
`AlwaysRejected`) plus a per-instance call counter. Structural separation:
`fake-evaluator → evaluation` only — no dependency on `fake-agent`,
storage, services, or `rusqlite` (`cargo tree -p fake-evaluator`).
One type never implements both roles.

## Verified Path

```text
READY → CLAIMED → RUNNING → E1/A1 → FakeAgent success
→ A1 Succeeded → SUBMITTED → AWAITING_REVIEW → VERIFYING
→ FakeEvaluator Verified → verification_succeeded → COMPLETED
```

Proven by `verified_end_to_end_flow_survives_db_reopen`: exact 11-event
journal (`task.created` … `verification.succeeded`, incl. the three
ADR-0022 runtime facts from trusted-service provenance), and all facts
survive DB reopen from canonical owners.

## Rejected Path

Valid success evidence with a `Rejected` verdict routes
`VERIFYING → RETRY_READY` via `verification_failed`; the Attempt stays
`Succeeded` (`rejected_evaluation_preserves_succeeded_attempt_history`,
also reopened). The `review_rejection` FakeAgent scenario is execution
evidence only: paired with either evaluator mode, the verdict — not the
fixture — decides the outcome
(`review_rejection_fixture_grants_no_authority_without_the_evaluator`).

## Completion Authority

`completion_requires_each_step_and_only_the_verdict_completes` proves the
hard G3 invariant stepwise: FakeAgent success alone, Attempt `Succeeded`
alone, `submit_result` alone, and `VERIFYING` alone each leave the Task
uncompleted. Only `Verified` + `verification_succeeded` completes. No code
maps `AttemptOutcome::Succeeded` (or any agent output) to `COMPLETED`.

## Durability Boundaries

No mega-transaction: the executor runs between durable commits, so a crash
leaves legitimate recoverable state (`RUNNING` + `E1` + open `A1`).
Evaluator failure (`Err`) leaves the Task `VERIFYING` with evidence
intact — reported, never retried inside the cycle. Failure outcomes
(`Failed`/`TimedOut`/`Cancelled`, malformed, partial) record one terminal
Attempt and return; the Task stays `RUNNING` for later recovery policy.

## Deferred Failure Recovery

TASK-0038 owns the full eleven-scenario matrix and retry/recovery policy.
TASK-0037 proves the mapping shape (crash/timeout/quota/network →
canonical outcomes, exactly one attempt, no hidden retry) and the
`Cancelled` outcome via a provider-neutral stub.

## Deferred G8 Evaluation

No `evaluations` table, no score/judge/scanner/runner, no new event type,
no migration 0005 (schema stays v4). The verdict drives the
already-authorized verification lifecycle only.

## Verification

```bash
cargo test -p app-services --test orchestration   # 13 flow tests
cargo test -p scheduler -p evaluation -p fake-evaluator
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo tree -p scheduler      # task-domain + agent-runtime only
cargo tree -p fake-evaluator # evaluation chain only, no fake-agent/storage
```
