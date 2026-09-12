# G5 Live-Run Ownership Remediation (TASK-0056, B-CR)

> Fixes the TASK-0055 blocker B-CR only: overlapping run/resume requests
> corrupting live-run ownership and cancellation. B-RS2 → TASK-0057,
> B-OL → TASK-0058. Historical audits (`G5_INDEPENDENT_CLOSURE_AUDIT.md`)
> stay unmodified evidence.

## 1. Root cause (confirmed in tree)

One shared `ExecutionComposition` per daemon (`agentd/src/lib.rs`). Its
`CancelRegistry` was `HashMap<String, CancelFlag>` with blind
`register` (= `insert`, overwrite) and blind `unregister` (= `remove`,
whoever owns it). Neither `run` nor `resume` checked liveness before
accepting, so N concurrent resumes all reported acceptance, the last
registration won, and every losing background exit unretired the winner:
`live=false` / `NO_LIVE_RUN` / ineffectual `LIVE_SIGNALLED` while the
provider ran to `PROVIDER_TIMEOUT`.

## 2. Decision

Exclusive process-local live ownership, one domain shared by `run` and
`resume` (`bins/agentd/src/execution.rs`):

- `CancelRegistry` maps `ExecutionId → LiveOwner { token, flag }`.
- `try_claim(execution, flag) -> Option<LiveOwnerToken>`: atomic
  insert-if-absent under one small mutex section (no provider/Git/DB
  work under the lock). `None` leaves the current owner untouched.
- `release(execution, token) -> bool` / `release_id`: removes only when
  the token is still current. No blind unregister exists anymore.
- `LiveOwnerToken`: opaque `u64` from a process-local atomic counter
  (never persisted, never wall-clock); successive owners differ, so
  stale cleanup is ABA-safe.
- `BackgroundJob` carries its claim token; `run_background` releases
  with it on every exit path; `abort_start` (spawn failure) takes the
  token too.
- `start_run` claims after the atomic durable admission; `resume_run`
  claims after all read-only validation. Acceptance of a live
  continuation is reported only after winning the claim.
- Loser outcome: typed `CONFLICT`, `execution {id} already has a live
  run; status shows the owner` — existing taxonomy, no protocol change.
- `cancel`/`status` semantics unchanged except they can no longer
  observe a replaced or prematurely retired entry: `signal` always hits
  the current owner's flag.
- No new Task state, no persisted owner rows; a fresh daemon starts
  with an empty registry (restart recovery still TASK-0054).

## 3. RED evidence (pre-fix tree)

New keepers failed on the old implementation as required:

- RED-1 (`live_ownership.rs`, one shared composition): 2 resumes →
  2 accepted (`["RESUMED attempt A-1", "RESUMED attempt A-1"]`); 8
  resumes → 8 accepted.
- RED-2 (`run_then_resume…`): `resume` after an accepted `run`
  returned `Resume(…RUNNING)` instead of a refusal.
- RED-3 (temporary probe on the old registry API, since removed):
  `register; register; unregister` left `is_live == false` — a stale
  cleanup removes the current owner.

## 4. GREEN evidence

- `bins/agentd/tests/live_ownership.rs` (one shared composition):
  2-resume single owner + live/cancel/release + exactly-once attempt /
  `attempt.created` / `workspace.created`; 8-resume single owner;
  run+resume keeps the run owner; 50-round 2-resume repetition (50/50
  exactly one owner, all `SUCCEEDED`, journals exactly once).
- `bins/agentd/src/execution.rs` unit: per-execution owner scoping +
  ABA stale-release keeper.
- `bins/agentctl/tests/execution_binaries.rs::live_ownership` (shipped
  binaries, parallel `agentctl` clients, `DaemonGuard` RAII against
  TEST-H leaks): 2-client and 8-client concurrent resumes → exactly one
  `resumed`, losers `CONFLICT`, winner `live=true`, `LIVE_SIGNALLED`
  cancel reaching the real provider, `CANCELLED`, then `live=false`;
  5 run+resume overlap rounds → resume never `resumed` (≥1 pre-attempt
  `CONFLICT` round observed per run), run ownership intact.
- Real Claude Code smoke (.supporting only): normal run → `SUCCEEDED`,
  Task `VERIFYING`, `live=false`, repeat cancel `ALREADY_CONCLUDED`.
- Full workspace: 696 passed / 0 failed / 1 ignored (env-gated live
  Claude smoke); `fmt --check` clean; `clippy -D warnings` clean.
  Targeted repeats: live_ownership 4/4, binaries 2/2 ×3 runs, recovery
  12/12, registry units 2/2. No stray daemons/providers/sockets/tmp
  residue.

## 5. Deferred (untouched)

B-RS2 (TASK-0057), B-OL (TASK-0058), F-BLK/F-MI/F-RA/F-SCAN/F-IPC,
HARD-1/2, H5/H7/H9, TEST-H (new keepers use isolated dirs + RAII
guards), post-Attempt G11 recovery. DOC-3's "losers fail safely" line
(ADR-0025 §4) is now implemented; the ADR carries the additive
TASK-0056 amendment and `ipc.md` §16 names the `CONFLICT` behavior.

## 6. Gate status

G5 remains IN PROGRESS; G6 must not start. Next: TASK-0057 (B-RS2).
