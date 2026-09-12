# G5 Post-Admission Entry Recovery Remediation (TASK-0054)

> Remediation evidence for the TASK-0052 blocker B-RS (admitted entries
> with no concrete recovery path) against
> `docs/development/G5_CLOSURE_REREAUDIT.md` (verdict
> `G5_REMEDIATION_REQUIRED`). `G5_CLOSURE_REREAUDIT.md` stays the
> historical record of failure; this document records what changed and the
> evidence that the blocker no longer exists. Authority: `AGENT.md`,
> `INVARIANTS.md`, ADR-0005/0006/0009/0018/0022/0023/0024 and new ADR-0025
> (which decides the recovery semantics).
>
> Verdict: **`G5_RECOVERY_REMEDIATION_READY`**. G5 stays `in_progress`
> pending the independent G5 closure audit. G5 is NOT closed here. G6
> must not start.

## 1. TASK-0052 finding B-RS

After admission `COMMIT` but before `create_attempt`, a crash (or a
deterministic background failure such as a non-repository path or an
unresolvable base revision) left `RUNNING` + execution + Attempt
reservation with zero attempts. Through the product the Task was
permanently stranded: `run` required `READY` (`INVALID_TRANSITION`),
`cancel` claimed `ALREADY_CONCLUDED … no attempts recorded`, no resume
operation existed, and the restarted daemon performed no reconciliation.
The only working path was the in-process library call
`OrchestrationService::continue_workspace_cycle`, which nothing in
production invokes after a restart. Durable did not mean recoverable
(INV-001).

TASK-0053 intentionally did not fix this.

## 2. Crash-boundary map (verified from source before implementation)

| Boundary | Durable result | Before | After |
|---|---|---|---|
| A: inside admission txn | zero facts, READY, same-id retry Runnable | safe | unchanged |
| B: post-`COMMIT`, pre-`create_attempt`, no lease | RUNNING + execution + both reservations + journal triple | stranded (B-RS) | explicit `resume` re-drives the same execution |
| B′: deterministic repo/rev failure in background | same as B | stranded (B-RS) | refused before admission (`INVALID_ARGUMENT`, zero mutation) |
| E: post-lease-allocate, pre/post-materialize | `ALLOCATED` lease (+ Git state or not) | library-only recovery | `resume` with the repository path reconciles (`ResumedAllocated` / `RecognizedGitState`) |
| post-lease-Ready, pre-attempt | `READY` lease, no attempt | library-only recovery | `resume` continues idempotently (`AlreadyReady`, exactly-once `workspace.created`) |
| C: inside reservation-consume txn | reservation intact, no attempt | safe | unchanged |
| D/F: attempt exists (OPEN or concluded) | attempt row present | stranded, G11-deferred | still refused by resume (`INVALID_TRANSITION`); G11 deferral recorded in ADR-0025 |

No `RUNNING → READY` edge was invented: resume performs no Task
lifecycle transition (`ACCEPTED_TRANSITIONS` untouched). A `BLOCKED`
task resumes first through the accepted `BLOCKED → RUNNING` operator
path, which re-arms recovery with no new execution.

## 3. Recovery semantics (decided in ADR-0025)

Resume-in-place: the SAME execution continues with its reserved Attempt
identity through the unchanged background half (lease → attempt →
provider → submit → verify). Planning
(`OrchestrationService::plan_pre_attempt_resume`) is read-only and
fail-closed: missing execution → `NotFound`; attempts present or task
not `RUNNING` → `InvalidTransition`; missing/ambiguous reservation,
ownership disagreement, missing task, or terminal lease → `Corrupt`.
Corruption is never healed by guessing ownership.

No new event types (still 17), no schema migration (still schema 7), no
reservation lifecycle change: consumption stays atomic with Attempt
creation, managed-identity discipline unchanged. Journal/state atomicity
is preserved because recovery reuses the existing commit boundaries.

## 4. Production control-plane entry points

- `agentctl execution resume <execution-id> <repository-path> [--ref …]
  [--rev …]` → additive IPC `ResumeExecution` (protocol v2 field 5;
  error stays field 4). No lease: ref/rev required. Lease-backed: they
  default to the recorded lease values and supplied values must agree
  (fail fast, not in the background). Acceptance spawns the same
  detached `run_background` half; the provider result arrives via
  `status`.
- Daemon startup runs a read-only scan and logs one line per strand with
  the exact resume invocation. Startup never spawns provider work
  (workspace inputs are operator-supplied operational data; surprise
  spawns are the G11 orphan class).
- `cancel` on a strand reports `AWAITING_RESUME` with the resume
  invocation (HARD-3 fixed); concluded executions still report
  `ALREADY_CONCLUDED`; `status` output is unchanged and already honest
  (`RUNNING`, `live=false`, zero attempts).

## 5. Pre-admission validation

`start_run` performs a read-only Git preflight (`is_repository` +
`resolve_revision`, no mutation) before `enter_execution`: non-working
paths (including bare repositories) and unresolvable revisions refuse
with static `INVALID_ARGUMENT` detail (supplied paths are never echoed).
Preflight is diagnostic, not a concurrency/security boundary; the
atomic admission transaction stays the correctness boundary, and
Git/provider effects stay outside it (INV-145).

`repository_ref` canonicalization: unchanged by design. Admission holds
a conservative global managed-identity reservation, lease allocation
additionally enforces per-repository ownership, and lease-backed resume
requires exact-string agreement with the recorded lease (fail closed).
Different spellings allocate distinct per-execution branches and can
never alias; precise physical-repository canonicalization stays
deferred, with no recovery-correctness gap (ADR-0025 §6).

## 6. RED → GREEN evidence

RED (on the pre-TASK-0054 tree):

- `crates/app-services/tests/entry_recovery.rs`: 8 errors — no
  `plan_pre_attempt_resume`, no stranded scan.
- `bins/agentd/tests/execution_recovery.rs`: 10 errors — no `Resume`
  IPC variant, no resume dispatch, no boot scan.
- Temporary dispatch probe (removed after execution): a
  non-repository path was ADMITTED into `RUNNING` (`PROBE-RED: bad repo
  ADMITTED into RUNNING`), reproducing the B′ strand through the
  production dispatch.

GREEN (post-implementation):

- `entry_recovery` 9/9 (strand shape, run-refusal characterization,
  plan detection, unknown execution, attempted refusal, BLOCKED
  interplay, reservation-mismatch fail-closed via raw-SQL corruption,
  ALLOCATED→READY lease reporting, scan census + no-new-event-types).
- agentd `execution_recovery` 12/12 (both pre-admission refusals with
  zero mutation, restart-shaped strand honesty + scan + cancel wording +
  full resume to `VERIFYING` with exactly-once facts, ALLOCATED and
  READY lease reuse with path-only inputs, missing-input refusals,
  lease-disagreement refusals, attempted/unknown/BLOCKED refusals,
  8-way concurrent resume convergence on exactly one attempt,
  post-recovery idempotence).
- agentctl `execution_recovery` 2/2 through the shipped binaries,
  including a genuine daemon `SIGSTOP` + `SIGKILL` + restart cycle:
  strand observed (`RUNNING`, `live=false`, zero attempts),
  `AWAITING_RESUME` cancel, `resume` to `SUCCEEDED`/`VERIFYING`, then
  `ALREADY_CONCLUDED`.
- `execution_composition::missing_workspace_fails_closed_without_
  fallback` updated to the mandated pre-admission contract (disclosed
  test change: the old body pinned the pre-TASK-0054 background-strand
  shape that §5 requires gone; the new body asserts the strictly
  stronger refusal — `INVALID_ARGUMENT`, task stays `READY`, no
  execution, no fallback).
- `ipc-contracts` resume request/response round-trip keepers.

## 7. Concurrency / idempotence evidence

- 8-way barrier-raced resumes (dispatch + background): exactly one
  attempt row, exactly one `attempt.created`, terminal `SUCCEEDED`,
  task `VERIFYING`; losers fail before provider execution.
- Repeated resume after recovery: `INVALID_TRANSITION`, no new facts
  (journal byte-identical across repeats).
- Restart repeated: strand → restart → restart → resume still works
  (plan is pure reads; resume re-validates before acting).
- Die-during-recovery: a crash after resume's attempt creation lands in
  the documented G11 post-attempt class (further resume refuses
  honestly); a crash before lands back in the resumable shape.
- Reservation/reporting races: lease allocation and reservation
  consumption keep their existing `UNIQUE`/transaction guards; the scan
  is read-only triage, never a correctness boundary.

## 8. Regression results

Full workspace suite (`cargo fmt --check`, `cargo check --workspace`,
`cargo test --workspace --no-fail-fast`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`):

```text
run 1: 689 passed / 0 failed / 1 ignored
run 2: 689 passed / 0 failed / 1 ignored
```

(Ignored: the env-gated live provider smoke. Baseline before this task
was 665 passed; the delta is the 9 + 12 + 2 + 1 new keepers plus the
updated composition keeper. One additional full run observed a single
transient failure under parallel-target load with no failing assertion
reproduced on the immediate rerun or any targeted repeat; see §10
TEST-H.)

Targeted repetitions: `entry_recovery` 5× (9/0/0), agentd
`execution_recovery` 3× (12/0/0), agentctl `execution_recovery` 3×
(2/0/0) — all green with zero stray daemons or sockets afterwards.

Preserved: all TASK-0053 case-fold keepers green (managed identity v2,
canonical-lowercase derivation, digest semantics, durable backstop,
old-lease verbatim compatibility); FakeAgent suites green; Claude
fixture contract suites green; second provider still needs only an
adapter crate + composition factory + contract invocation (no core
change — resume reuses the composition-owned executor binding).

Real Claude smoke (Claude Code 2.1.268, shipped binaries, isolated
runtime/data dirs, subscription auth provider-owned) — PASS, both halves:

- Normal path: `run` → 8 s wall → attempt `A-SMOKE SUCCEEDED` with opaque
  `claude:session:16a6151a-…` evidence → Task `VERIFYING` (provider text
  did not complete it); primary HEAD unchanged and clean; data dir 0700;
  0 secret markers in DB.
- Recovery path: second task admitted via `--detach`, daemon `SIGSTOP` +
  `SIGKILL`, daemon restarted → `status` showed `RUNNING live=false`
  with zero attempts and no lease; `cancel` reported `AWAITING_RESUME`
  with the resume invocation; the boot log carried the pre-attempt scan
  line; `execution resume` with the original ref/rev → attempt
  `A-RESUME SUCCEEDED` with a fresh `claude:session:6ab5caf0-…`
  evidence → Task `VERIFYING`. Journal for the resumed task is exactly
  `created > marked_ready > claimed > started > execution.created >
  workspace.created > attempt.created > attempt.concluded > submitted >
  routed_to_review > verification_began` (exactly once each, ordered);
  0 reservations left; worktree porcelain shows only the known deferred
  H7 `?? data/sessions.db`. Clean shutdown with zero residue
  (a later on-demand `cancel` correctly reports `ALREADY_CONCLUDED`).

## 9. Cancellation behavior change (HARD-3)

`handle_cancel` distinguishes three zero-live shapes: open attempt →
`NO_LIVE_RUN` (unchanged); concluded history → `ALREADY_CONCLUDED`
(unchanged); zero attempts with an intact reservation →
`AWAITING_RESUME` with the resume invocation (lease-aware hint). Zero
attempts with no/ambiguous reservation (unreachable through production)
reports `NO_LIVE_RUN` with an inspection pointer — never a false
conclusion.

## 10. Deferred findings (unchanged, plus one note)

HARD-1 (SQLite/client deadline), HARD-2 (unreserved attempt misuse),
H5 (PATH-only precheck), H7 (`sessions.db` hygiene), H9 (file fitness),
TEST-H (stale-daemon test hermeticity) keep their TASK-0052
classifications. No unrelated hardening was bundled.

Note: this task adds one more parallel daemon-spawning binary suite,
which raises the collision odds for the TEST-H-sensitive
`g1_real_process_matrix` same-binary-path count under full-suite
parallelism. That test is untouched (test-only hardening stays
deferred); the full-suite census above is the authority.

## 11. Remaining G5 posture

B-RS is closed by this task (recovery path proven across a real
restart). B-CF stays closed from TASK-0053. The remaining G5 question —
whether anything else blocks closure — belongs to the subsequent
independent G5 closure audit, which alone may declare `G5_CLOSED`.

## 12. Version

`product/version.toml` → `0.1.0-dev.task0054` (gate G5). Queue:
TASK-0054 completed; independent G5 closure audit follows. G5 stays
`in_progress`. G6 not started.
