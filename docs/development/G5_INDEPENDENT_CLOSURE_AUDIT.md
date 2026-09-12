# G5 Independent Adversarial Closure Audit (TASK-0055)

> Independent closure audit of G5 after the TASK-0053 (case-fold managed
> identity) and TASK-0054 (post-admission recovery) remediations.
> Authority: the TASK-0055 brief, `INVARIANTS.md`, ADR-0005/0008/0009/
> 0018/0022/0023/0024/0025, `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`
> §79. `G5_CLOSURE_AUDIT.md` (TASK-0048), `G5_CLOSURE_REAUDIT.md`
> (TASK-0050), `G5_CLOSURE_REREAUDIT.md` (TASK-0052) and the TASK-0049/
> 0051/0053/0054 remediation documents are historical evidence under test,
> not authority, and are not modified.
>
> Verdict: **`G5_REMEDIATION_REQUIRED`** (§16). Three G5 blockers:
> overlapping resumes corrupt live-run ownership so the running provider
> becomes uncancellable and falsely reported (B-CR); a daemon crash inside
> workspace materialization leaves a pre-attempt strand that no product
> path recovers (B-RS2, B-RS not fully closed); pre-v2 leases whose
> ExecutionId begins with `e2-`/`h2-` are misclassified as v2 and
> rederived, so they can neither resume nor clean up (B-OL). G5 stays
> `in_progress`; G6 must not start.

## 1. Scope and method

Audit only. No production, test, or architecture file was changed. All
probes live OUTSIDE the repository (session scratchpad crate `g5audit`
with its own target dir, path-dependent on the audited crates, plus shell
harnesses driving the shipped `target/debug/agentd`/`agentctl`); the
audited tree's `git status` was identical before and after. A genuine
TASK-0051 build (`git archive b3b956d`, schema 6, old slug scheme) was
compiled separately for migration/old-lease/IPC cross-version probes.
Every probe asserts the EXPECTED contract; a failing probe reproduces a
defect. Durable state (SQLite rows, journal, Git refs/worktrees) was
inspected, never return codes alone.

```text
P1  shared-registry resume races (one ExecutionComposition, as in agentd)   6 tests: 4 reproduce B-CR, 2 record F-BLK
P2  shipped binaries: SIGKILL in pre-attempt window, restart, boot scan,
    double resume, run+resume, die-during-recovery                            reproduces B-CR; recovery path otherwise works
P2b 60 randomized daemon SIGKILLs across the run pipeline + census + resume  0 invariant violations; reproduces B-RS2 (3/38)
P2c materialization crashes: 20 graceful shutdowns; 16 SIGKILLs, 20k-file repo  20/20 recovered; B-RS2 11/16 unrecoverable
P3  real Git, case-insensitive APFS: case variants + E/S vs E-2FS, pack-refs,
    crash, restart, resume, commits; 14 fold-pair admissions                 pass
P4  genuine TASK-0051 binaries → current daemon (migration, old leases,
    old strands); P4b old READY lease cleanup                                reproduces B-OL
P5  first-principles identity corpus (134 hostile ids), loose+packed refs    pass
P6  manual recovery inputs, repository spellings, clones, stale HEAD        observations (F-MI, F-RA, DOC-3)
P7  IPC cross-version (old client/new daemon, new resume/old v2 daemon)      additive ok; F-IPC
P8  real Claude Code 2.1.268: normal run + SIGKILL strand → restart → resume  pass
P9  reservation corruption, cancel honesty, boot-scan robustness            fail-closed; F-SCAN
P10 cross-PROCESS admission races incl. forced managed-branch collision      120 races, 0 violations
P11 provider-neutral recovery with FakeAgent through the same planning seam  pass
TEST-H leak mechanism reproduction; HARD-1 re-measurement
```

## 2. Repository, host, versions

- HEAD `b3b956d` (TASK-0051 commit) plus the uncommitted TASK-0053/0054
  working tree; `product/version.toml` `0.1.0-dev.task0054`;
  `SUPPORTED_SCHEMA_VERSION = 7`; IPC protocols `0,1,2`.
- macOS APFS, **case-insensitive proven** (`CaseProbe` resolves as
  `caseprobe`; test repos `core.ignorecase=true`), Git 2.55.0, rustc/cargo
  1.95.0, Claude Code **2.1.268** (`~/.local/bin/claude`, subscription
  auth, provider-owned).
- Environment incident (not product): the first full run left two
  `agentd` processes (TEST-H, §14); both were identified by `lsof`/env
  and stopped before any probe. The audited repository root carries a
  pre-existing untracked `data/sessions.db` from the user's own tool
  environment (not created by the product).

## 3. Derived G5 closure criteria

Roadmap G5 (§79): one real provider fully passes the provider contract
through the durable core; a second provider adds without Task/Storage/UI
core changes. Applying the accepted invariants/ADRs that are mandatory
now (INV-001/002/004/005/006/009/010/012/025/027/082/093/145, ADR-0005/
0018/0023/0024/0025), and the TASK-0052 standard that durable must mean
recoverable through the product for pre-attempt strands:

| # | Criterion | Result |
|---|---|---|
| 1 | Historical G5 blockers stay resolved | **FAIL** (old-lease compatibility regressed: B-OL) |
| 2 | B-CF independently verified | PASS for derivation/refs/FS (§7–9); B-OL is an adjacent TASK-0053 regression |
| 3 | B-RS independently verified | **FAIL** (B-RS2; B-CR in the recovery path) |
| 4 | No new G5 architecture blocker | **FAIL** (B-CR) |
| 5 | Task/Execution/Attempt/Session boundaries | PASS |
| 6 | Real Claude through production composition | PASS |
| 7 | Workspace isolation (real Git, case-fold, packed refs) | PASS |
| 8 | Admission/reservation concurrency | PASS |
| 9 | Recovery survives genuine death/restart | **FAIL** for materialization crashes (B-RS2) |
| 10 | Recovery concurrency sound | **FAIL** (B-CR) |
| 11 | Journal/state guarantees | PASS |
| 12 | Provider cannot self-complete | PASS |
| 13 | Old WorkspaceLease compatibility | **FAIL** (B-OL) |
| 14 | FakeAgent valid | PASS |
| 15 | Second provider structurally addable | PASS |
| 16 | Docs do not materially overclaim | **FAIL** (DOC-3) |
| 17 | Deferred findings non-blocking | PASS (§14) |
| 18 | Full workspace tests clean | PASS with one TEST-H incident (§15) |
| 19 | Reproducible evidence | PASS |

Scope separation used below: **G6** owns Policy Engine, OS isolation,
secrets broker, provider-environment isolation and redaction; **G11** owns
supervision of orphaned provider processes and post-attempt crash
recovery; **G3/G7** own retry/re-execution ergonomics. None of those is
promoted here; each blocker violates an already-mandatory contract.

## 4. Historical blocker matrix

| Item | Evidence this audit | Result |
|---|---|---|
| Sequential admission | keepers 3×; P3/P10 service mode | resolved |
| Concurrent admission, same execution id (two tasks) | P10-A 40 cross-process rounds | one winner; loser READY, zero facts |
| Same-task concurrent run | P10-B 40 rounds | exactly one full admission |
| Duplicate attempt id / exact duplicate request | `entry_atomicity` 3× green (not re-probed cross-process) | resolved (keeper) |
| Journal atomicity | P2b 60 random SIGKILLs: 0 census violations; P10 | resolved |
| Reservation ownership / consumption / crash safety | P9 (foreign → CORRUPT), P2b/P8 (0 leaked), code | resolved |
| E/S vs E-2FS | P3b real Git + pack-refs + crash + resume + commits | resolved |
| E-CASE vs E-case (B-CF) | P3a, P3c (14 pairs), P5 | resolved |
| Loose / packed refs | P5 134 loose + 134 packed distinct; P3 | resolved |
| Old WorkspaceLease compatibility | P4/P4b with genuine TASK-0051 data | **regressed for `e2-`/`h2-` ids (B-OL)** |
| Provider / FakeAgent / Claude contracts | contract suites 2× each; P8; P11 | resolved |
| No provider self-completion | P8 ("Task complete." → VERIFYING); P11 | resolved |
| Second provider addable | `cargo tree`; P11 | resolved |
| Post-admission / pre-attempt recovery (B-RS) | P2, P2b, P2c, P8 | **partially open (B-RS2, B-CR)** |
| Daemon restart | P2/P8 real SIGKILL + restart | works except B-RS2 shapes |
| HARD-3 cancel wording | P2/P8 `AWAITING_RESUME` | resolved (residuals under corruption: F-SCAN) |

## 5. Boundaries, completion authority, provider neutrality

- Adapters hold no store: `ClaudeExecutor` is built per attempt from Task
  objective/acceptance and lease worktree only; orchestration performs
  every Task/Attempt mutation through `TaskService`/`RuntimeService`.
- Completion: `PendingEvaluator` returns no verdict; real Claude claimed
  "Task complete." twice and both Tasks stayed `VERIFYING`; with FakeAgent
  the same recovery seam reaches `COMPLETED` only on an evaluator
  `Verified` verdict and `RETRY_READY` on `Rejected` (P11).
- `cargo tree -e normal -i provider-claude` → `agentd` only; dev-deps
  only in `app-services` tests; every `claude` string in shared crates is
  `#[cfg(test)]`. Recovery planning (`plan_pre_attempt_resume`) is
  provider-neutral; the composition root owns executor binding. A second
  provider still needs an adapter crate, a composition factory, and a
  contract invocation.
- No hidden production path: `schedule`, `tasks_mut`/`runtime_mut`, and
  `RuntimeService::create_attempt` with an unreserved id (HARD-2) are not
  reachable from IPC.

## 6. Atomic admission and reservations

`SqliteStore::admit_execution_entry` (`entry_store.rs:108-234`): one
`IMMEDIATE` transaction — guarded READY→CLAIMED→RUNNING, execution,
attempt, reservation and managed-identity conflict checks inside the
transaction, inserts, three journal facts, commit. The branch is derived
before the transaction; no Git/filesystem/provider work inside (INV-145).
P10: 120 cross-process races (same execution id; same task; forced shared
managed branch at the store boundary) → exactly one full winner per race,
every loser `READY` with zero executions/reservations/identity rows/
lifecycle facts, forced collision refused as typed
`Duplicate("managed branch …")`. P2b census over 60 random SIGKILLs: every
Task is either untouched `READY` or carries exactly one
claimed/started/execution.created triple with one identity reservation and
either one attempt reservation (0 attempts) or 0 reservations (1 attempt);
`workspace.created` present iff lease `READY`. **0 violations.**

`create_attempt_with_event` consumes the reservation in the attempt
transaction; concurrent creators converge on one attempt (P1d, P2). P9:
missing, duplicate, or foreign-task reservations make resume refuse
`CORRUPT_STATE`; reservation + attempt refuses `INVALID_TRANSITION`; no
guessing, no mutation. `AWAITING_RESUME` is never persisted (0
non-canonical task states).

## 7. Managed identity v2 (B-CF)

First principles (P5, independent decoder): 134 distinct hostile ids
(ASCII case variants, `/ % . .. @{ \ : ? * [ ~ ^`, spaces/tab/newline/
control, NFC vs NFD, emoji and ZWJ, `ß`/`SS`/`İ`/Kelvin, ligatures,
Windows `CON`/`nul.txt`/`COM1`/`LPT9`, `e2-…`/`h2-…` lookalikes, repeated
escapes, 20–256-byte and 256-byte UTF-8 ids): 94 readable / 40 digest;
every slug in `[a-z0-9_%-]` with `e2-`/`h2-` prefix, ≤ 67 chars; readable
decodes to the exact bytes; digests equal SHA-256 of the exact bytes; no
Windows reserved basename or trailing dot/space; `git check-ref-format`
passes; **0 fold collisions**; 134 loose refs and 134 packed refs remain
distinct on the case-insensitive filesystem. Terminology (readable
injective, digest collision-resistant, not injective) is honest.

## 8. Real Git on the case-insensitive filesystem

P3a `E-CASE`/`E-case` and P3b `E/S`/`E-2FS`, each: first execution runs
fully → `pack-refs --all` → second admitted then crashed → `pack-refs` →
fresh composition → boot scan lists it → resume → `pack-refs` → a commit
in each worktree with `pack-refs` between → independent branch heads,
worktree HEADs, `symbolic-ref`, and logs (each log holds only its own
commit plus base); packed-refs holds both refs; primary checkout `main`,
clean; both Tasks `VERIFYING`. P3c: 14 fold-pair executions
(`A/a`, `ABC/abc`, `Execution/execution`, `E/S`/`E/s`, `e2-x`/`E2-X`, …)
all admit and materialize with 14 fold-distinct branches. **Isolation
holds.**

## 9. Old WorkspaceLease compatibility and migration (B-OL)

Genuine TASK-0051 daemon (schema 6) created: READY `e2-legacy`
(`agent/e2-legacy`, `ws-ws-…`), READY `E-LEG`, ALLOCATED strands
`e2-strand` and `E-STRAND` (old branch collision, branch then removed), and
a no-lease strand `e2-nolease` (old code admitted a non-repository path).
The current daemon then opened the same database:

```text
schema 6 → 7; workspace_leases byte-identical; identity reservations empty; boot scan lists 3 strands
resume E-STRAND (old, non-e2 id)   → persisted agent/E-STRAND used → SUCCEEDED → VERIFYING        ✔
resume e2-nolease (--ref/--rev)    → new v2 lease agent/e2-e2-nolease (origin lease) → SUCCEEDED   ✔
resume e2-strand (old, e2- id)     → "resumed …" then background: managed root changed under lease; refusing
                                      → stays RUNNING/ALLOCATED forever (every resume repeats)       ✗
cleanup E-LEG (old READY)          → Removed                                                          ✔
cleanup e2-legacy (old READY)      → OwnershipMismatch: worktree reference disagrees with managed root ✗
new run id "legacy"                → ALREADY_EXISTS vs old lease e2-legacy (false collision, fail-closed)
old TASK-0051 daemon on schema 7   → "unsupported schema version 7 (adapter supports 6)"             ✔
```

Root cause: `is_v2_managed_branch` classifies a lease by branch prefix
(`agent/e2-`/`agent/h2-`), but both historical schemes passed
`[A-Za-z0-9_-]` through verbatim, so any old ExecutionId beginning with
`e2-`/`h2-` produced a v2-looking branch. Such leases take the v2 path in
`materialize_allocated` (`workspace_service.rs:1031`) and
`validate_managed_path` (`:695`), which rederive `ws-e2-<id>` and compare
it with the persisted `ws-ws-<id>`. TASK-0053 §12 ("never rederived", "v2
namespaces are disjoint from every old output") is false for this class.

## 10. Recovery: resume-in-place, restart, crash matrix (B-RS2)

Resume-in-place is implemented as decided: the SAME execution and the
reserved attempt identity; no Task transition, no replacement execution,
no second `execution.created`, no boot-time provider spawn (P2, P8, P11).
Real SIGKILL + restart (P2-A, P8-B): honest `RUNNING live=false` with zero
attempts, boot line per strand, `cancel` → `AWAITING_RESUME`, `run` →
`INVALID_TRANSITION`, `resume` → `SUCCEEDED` → `VERIFYING`, journal
`created > marked_ready > claimed > started > execution.created >
workspace.created > attempt.created > attempt.concluded > submitted >
routed_to_review > verification_began` exactly once, reservations
consumed, repeat resume `INVALID_TRANSITION` with unchanged journal.
Die-during-recovery (P2-D, SIGKILL inside resume's `git worktree add`):
state stays resumable (ALLOCATED, 0 attempts, reservation intact, no
`workspace.created`); after another restart resume succeeds.

Crash matrix (A–L):

| Boundary | Evidence | Result |
|---|---|---|
| A before admission | P2b 3/60 | READY, zero facts |
| B inside admission txn | code (single IMMEDIATE); P2b/P10 | atomic |
| C after COMMIT, no lease | P2b 2 strands; P2c; P8 | resumable ✔ |
| D before lease / E lease ALLOCATED, Git untouched | P2b, P2c, P2-D | resumable ✔ |
| **E/F crash inside `git worktree add`** | P2b 3/38 strands; P2c 11/16 (20k files) | **unrecoverable ✗ (B-RS2)** |
| F ALLOCATED + complete Git / READY lease | P2c (dir+registration+20000/20000 files) | recognized, resumable ✔ |
| G/H before/inside attempt txn | reservation consumed atomically (code, P2b) | resumable ✔ |
| I–L attempt exists (before spawn, running, exit before persist) | P2b 19/60 `RUNNING` + OPEN attempt | refused `INVALID_TRANSITION`; G11 deferral (§12) |

B-RS2 shape (always the same): managed branch present at exactly the lease
base OID, worktree directory absent, no worktree registration, lease
`ALLOCATED`, identity reservation owned by the execution (`origin
admission`). `git worktree add -b` creates the branch first; the child dies
with its parent before creating the worktree. `reconcile_existing` treats
it as crash-C "ambiguous partial Git state" (`workspace_service.rs:1187`),
`cleanup_workspace` refuses `ALLOCATED` (`:349`), no IPC exposes any other
path, and `agentctl execution resume` prints `resumed … RUNNING` while the
background refuses. Only out-of-product `git branch -D` followed by resume
recovered it (verified). Graceful `agentctl shutdown` inside the same
window recovered 20/20 (Git either untouched or complete).

## 11. Concurrent recovery (B-CR)

The production daemon builds ONE `ExecutionComposition` shared by every
connection (`agentd/src/lib.rs:259`). `resume_run` never checks
`cancels.is_live(execution)` and `CancelRegistry::register` is a plain
`HashMap::insert` keyed by execution id; every background exit calls
`unregister(execution)` regardless of which run registered the flag.

```text
P1a  run admitted (flag1, job pending); resume accepted (flag2 overwrites flag1); job1 runs provider
     cancel → LIVE_SIGNALLED "provider tree is being terminated" → provider NOT cancelled → SUCCEEDED → VERIFYING
P1b  run + resume both spawned (serve-loop shape): loser exits → status live=false while provider runs;
     cancel → NO_LIVE_RUN; provider runs to SUCCEEDED
P1c  genuine strand, 2 resumes: both RESUME_ACCEPTED; winner live=false; cancel NO_LIVE_RUN; uncancellable
P1d  8 barrier resumes: 8/8 RESUME_ACCEPTED; 1 attempt, 1 launch; winner live=false; uncancellable
P2-A shipped binaries after SIGKILL+restart: two parallel `agentctl execution resume` → both "resumed";
     provider running: live=false, cancel NO_LIVE_RUN, still OPEN 5 s later → SUCCEEDED
P2-C shipped binaries: `run --detach` (status live=true, 0 attempts) then `resume` → "resumed";
     provider running: live=false, cancel NO_LIVE_RUN → SUCCEEDED
```

What still holds: at most one attempt and one provider launch; one
`attempt.created`/`workspace.created`; losers never destroy Git state
(git-cli failure paths are non-destructive). What fails: N accepted
responses for one run, a false `LIVE_SIGNALLED`, `live=false` for a live
provider, and loss of control-plane cancellation for up to
`PROVIDER_TIMEOUT` (300 s). The pre-attempt window is wide in practice
(lease still `ALLOCATED` 0.3 s after `run --detach` returns on a one-file
repo; seconds on a 20k-file repo).

## 12. BLOCKED, manual inputs, repository spellings, cancellation

- **BLOCKED → RUNNING.** Only operators reach `BLOCKED` today
  (`task block`, RUNNING→BLOCKED); resume refuses a `BLOCKED` task
  (`INVALID_TRANSITION`) and never performs BLOCKED→RUNNING itself, so it
  cannot bypass an unresolved block. F-BLK: neither `run` nor `resume`
  re-checks the Task lifecycle before `create_attempt`; a Task blocked
  after acceptance still gets an attempt and a provider launch (P1e resume,
  P1f run: `task.blocked > workspace.created > attempt.created >
  attempt.concluded`, Task stays `BLOCKED`, evidence unsubmittable).
  Pre-existing since TASK-0047, identical for `run`, no accepted invariant
  requires attempt-time lifecycle re-validation (blocker taxonomy open,
  task-engine §16) → HARDENING, not a blocker.
- **Manual inputs (F-MI).** No-lease strands resume against any repository
  and `repository_ref` (P6a: an unrelated repository received the branch
  and provider run); blank `--rev` re-resolves HEAD at resume time, not
  the admission-time OID (P6b). ALLOCATED lease + a clone containing the
  base OID materializes in the clone (P6c) although the code comment says
  "wrong-repository paths fail closed". ADR-0025 explicitly chose not to
  persist workspace inputs; no invariant is violated → HARDENING + DOC-3.
  Lease-backed ref/rev disagreement refuses synchronously (keeper).
- **Accept-then-fail (F-RA).** READY lease + wrong clone (P6d), B-OL and
  B-RS2 shapes return `resumed …` and fail only in the background (logs).
- **Same physical repository, different spellings.** Symlink, trailing
  slash, `..`, case-variant path, and subdirectory all resume READY and
  ALLOCATED leases correctly (P6e); distinct `repository_ref` spellings
  (`repo-local:X`/`REPO-LOCAL:x`/symlink/trailing slash) allocate distinct
  branches with independent commits (P6f). The conservative global
  identity reservation plus per-execution branches make canonicalization
  unnecessary for safety — ADR-0025 §6 verified.
- **Cancellation honesty.** Live run → `LIVE_SIGNALLED` then `CANCELLED`
  (keepers 5×) — except under B-CR; strand → `AWAITING_RESUME`; concluded
  → `ALREADY_CONCLUDED`; zero attempts + zero reservations → `NO_LIVE_RUN`
  "unexpected durable state"; after recovery → `ALREADY_CONCLUDED`.
  F-SCAN (under corruption only): two reservations are reported as "no
  pending reservation"; a foreign-task reservation is advertised
  `AWAITING_RESUME` although resume refuses `CORRUPT_STATE`.
- **Boot scan (F-SCAN).** Read-only, never gates startup, never spawns
  providers (P2, P4, P8), logs identities only (0 secret markers). The
  printed invocation omits `--ref/--rev`, so for a no-lease strand it
  fails as printed (`repository ref is required …`, P4). A foreign-task
  reservation and a duplicated reservation are listed as resumable, and
  one undecodable stored row makes the whole scan return empty (2 → 0
  strands, P9b).

## 13. IPC, pre-admission validation, migration

- `execution_core.proto`: `resume = 5` in both oneofs, `error = 4`
  unchanged; round-trip keepers green. Old TASK-0051 `agentctl` →
  current daemon: run/status/cancel work. F-IPC: current `agentctl
  execution resume` → TASK-0051 v2 daemon returns `INTERNAL: empty
  execution request` (no mutation) rather than a typed unsupported error;
  the "v2 surface is exactly" contract grew without a version signal.
  Static error text, no path echo on resume refusals.
- Pre-admission: non-repository paths and unresolvable revisions refuse
  `INVALID_ARGUMENT` with zero mutation (keepers 5×). Bare repositories
  refuse there too, consistent with ADR-0023 ("Bare repositories fail
  explicitly") — not a regression. The disclosed
  `missing_workspace_fails_closed_without_fallback` rewrite strengthens the
  contract. Preflight stays diagnostic (admission remains the correctness
  boundary).
- Migration: genuine v6 data → v7 with rows byte-identical, empty
  identity table, reopen/restart clean (P4); TASK-0051 daemon fails closed
  on v7. Pre-0006 strands are unreachable by construction for
  resume (no reservation) — dev-era, fail-closed, consistent with
  ADR-0025 consequences. 0006-era strands without identity reservations
  resume and claim `origin lease` (P4).

## 14. Deferred findings reclassification

| ID | Claim re-verified | Reachability / TASK-0053-54 effect | Classification |
|---|---|---|---|
| HARD-1 | `run` under external write lock: client ambiguous at 5.02 s; entry commits after release, task reached VERIFYING | unchanged; resume adds only read-only Git preflight to the sync path | HARDENING |
| HARD-2 | `RuntimeService::create_attempt` accepts an unreserved id, orphaning the reservation (P9) | not reachable via IPC; resume uses the planned reserved id | HARDENING |
| HARD-3 | strand cancel wording | fixed (`AWAITING_RESUME`); residual corrupt-shape wording → F-SCAN | RESOLVED |
| H5 | precheck is PATH/exists only (`provider_precheck`), shared by run and resume | unchanged | ACCEPTABLE_DEFERRED |
| H7 | real smoke worktrees show `?? data/sessions.db` | unchanged; must close before any delivery gate (G6 provider environment isolation) | ACCEPTABLE_DEFERRED |
| H9 | `agentd/src/execution.rs` 1979 LOC (was 1581 at TASK-0052), `workspace_service.rs` 1544; no reviewed exception recorded (INV-081) | risk increased by TASK-0054 | HARDENING |
| TEST-H | run 1: `g1_real_process_matrix` saw 2 daemons. Mechanism reproduced 10/10: `execution_binaries::wait_ready` pings with connect-first `agentctl`, which auto-spawns a sibling daemon when it races ahead of the test's own spawn; the test child exits `AlreadyRunning`, `stop_daemon(child)` kills nothing, the sibling survives across test binaries. `g1_real_process_matrix` also targets the user's default endpoint/data dir and leaks its daemon on assertion failure. TMPDIR holds 619 `agentd-dispatch-test-*` dirs. | test-only; product singleton held (exactly one daemon) | TEST_HARDENING |
| Retry after failed/cancelled attempt | Task stays RUNNING, `run` needs READY, resume refuses attempted executions | no accepted RUNNING→terminal/READY edge (task-engine §7/§16) | ACCEPTABLE_DEFERRED (G3/G7) |
| Post-attempt strands (I–L) | 19/60 random kills; honest `RUNNING` + OPEN attempt; resume refuses | INV-001/078 hold (state durable, honest); INV-066 expects a new/recovered attempt, which needs orphan supervision; concluding without provider evidence would fabricate history | ACCEPTABLE_DEFERRED (G11, documented in ADR-0025 §4) |

## 15. Commands and census

```text
cargo fmt --all -- --check                                              clean
cargo check --workspace                                                 clean
cargo clippy --workspace --all-targets --all-features -- -D warnings    clean
cargo test --workspace --no-fail-fast
  run 1: 688 passed / 1 failed / 1 ignored (76 suites)  failed: g1_real_process_matrix (TEST-H, §14)
  run 2: 689 passed / 0 failed / 1 ignored (76 suites)  no stray daemons
  run 3: 689 passed / 0 failed / 1 ignored (76 suites)  no stray daemons
  run 4: 689 passed / 0 failed / 1 ignored (76 suites)  final tree (version + queue updated), no stray daemons
targeted repetitions (51 runs, 0 failed, 0 strays):
  app-services entry_recovery 5× (9/0/0)   casefold_identity 3× (8/0/0)   entry_atomicity 3× (5/0/0)
  app-services concurrent_cycles 3× (2/0/0) workspace_service 2× (12/0/0) provider_binding 2× (6/0/0)
  agentd execution_recovery 5× (12/0/0)    agentctl execution_recovery 3× (2/0/0)
  agentctl execution_binaries 5× (2/0/0)   execution_composition 2× (8/0/0) real_process_matrix 3× (1/0/0)
  storage durability 2× (22/0/0) runtime 2× (32/0/0) workspace 2× (14/0/0) journal 2× (21/0/0)
  provider-claude process_fixtures 3× (13/0/0) execution_port_contract 2× (1/0/0); fake-agent contract 2× (1/0/0)
probes (outside repository): P1 6 (4 B-CR, 2 F-BLK); P2 A/C/D; P2b 60 kills; P2c 20 + 16; P3 3/3;
  P4 + P4b; P5 1/1; P6 6 observations; P7 2; P8 2 halves; P9 2; P10 120 races; P11 1/1
real Claude Code 2.1.268: normal 9 s, resume 10 s — PASS
```

Ignored test: `provider-claude/tests/smoke.rs` (env-gated live smoke).
Cleanliness after the audit: no `agentd`/fixture/provider processes, no
probe sockets, audited tree unchanged, no `agent/*` branches or extra
worktrees in the repository; 0 attempt reservations left for concluded
executions; identity reservations persist by design.

## 16. Findings and verdict

| ID | Classification | Severity | Scenario | Expected | Actual | Violated contract | Evidence |
|---|---|---|---|---|---|---|---|
| B-CR | **G5_BLOCKER** | HIGH | two resumes, or `run` then `resume`, for one execution in its pre-attempt window, one daemon | one accepted owner; losers typed; winner stays live and cancellable | every request accepted; last registration overwrites, every loser's exit unregisters; `LIVE_SIGNALLED` for a flag nobody reads; `live=false`; `NO_LIVE_RUN` while the provider runs to completion | ADR-0025 §4 ("losers fail safely"), ADR-0024 §4/§9 (cancel signals the live run), ipc.md §16 status/cancel, TASK-0047 liveness/cancellation exit, TASK-0055 §17/§20 | §11; P1a–d; P2-A, P2-C |
| B-RS2 | **G5_BLOCKER** | HIGH | daemon SIGKILL inside `git worktree add -b` after admission | strand recoverable through the product | branch-only partial state → resume accepted, background `CheckoutFailed` forever; ALLOCATED cleanup refused; only manual Git surgery recovers | TASK-0054 exit ("daemon crash between admission and attempt creation is recoverable through the product"), ADR-0025 §1, INV-001 as applied by TASK-0052 B-RS, INV-145, workspace.md "partial creation is … reconcilable" | §10; P2b 3/38, P2c 11/16, manual-remedy check |
| B-OL | **G5_BLOCKER** | MEDIUM | pre-v2 lease whose ExecutionId starts `e2-`/`h2-` | persisted branch/path authoritative, never rederived | classified v2 by branch prefix; path rederived; ALLOCATED strand never resumes; READY lease never cleans up | ADR-0023 persisted-identity authority, TASK-0053 exit ("old leases unchanged"), TASK-0055 §10, workspace.md "never rederived" | §9; P4, P4b |
| F-BLK | HARDENING | MEDIUM | `task block` after run/resume acceptance, before attempt | no provider work for a BLOCKED task | attempt + provider launch; Task stays BLOCKED | task-engine §6 BLOCKED semantics (no attempt-time check mandated) | §12; P1e, P1f |
| F-MI | HARDENING | MEDIUM | resume inputs for no-lease strands / ALLOCATED + clone | bind to admitted intent | any repository/ref; HEAD re-resolved; clone accepted | ADR-0025 design choice; docs silent | §12; P6a–c |
| F-RA | HARDENING | LOW | deterministic lease/Git inconsistency on resume | typed synchronous refusal | `resumed …`, background-only failure | ADR-0025 §3 spirit (deterministic failures refuse before acting) | §12; P4, P6d, P2c |
| F-SCAN | HARDENING | LOW | boot/cancel hints; corrupt shapes | executable, truthful triage | no-lease hint not executable; corrupt shapes advertised; one bad row hides all strands | TASK-0055 §16 | §12; P4, P9 |
| F-IPC | HARDENING | LOW | new resume against an older v2 daemon | typed unsupported | `INTERNAL: empty execution request` | ipc.md §16 surface statement | §13; P7 |
| DOC-3 | DOCUMENTATION_DEFECT | — | recovery/identity docs | describe implemented behavior | ADR-0025 §2 "exact resume invocation", §4 "losers fail safely"; workspace.md "Allocated lease resumes … or is recognized", "pre-v2 leases … never rederived"; TASK-0053 §12 disjointness; TASK-0054 §2 row E and §7 (8-way race used separate compositions); `execution.rs` "wrong-repository paths fail closed" | ADR accuracy, AGENT.md evidence rules | §9–12 |
| HARD-1/2, H5, H7, H9, TEST-H, retry, post-attempt | see §14 | — | — | — | — | — | §14 |

**Verdict: `G5_REMEDIATION_REQUIRED`.** TASK-0053's v2 derivation is sound
on the host's case-insensitive filesystem (loose and packed refs, crash,
restart, resume), admission and reservations are atomic across processes,
journal/state atomicity survives 60 random daemon kills, real Claude runs
and recovers through the production path without self-completion, and a
second provider remains structurally addable. G5 does not close: recovery
concurrency breaks live-run ownership (B-CR), the dominant crash position
on real repositories still strands a pre-attempt entry beyond product
recovery (B-RS2), and old-lease compatibility regressed for a class of
identities (B-OL).

**G5 remains IN PROGRESS. G6 must not start. A separate remediation task
is required.**

## 17. Blocker detail and narrow remediation direction

### B-CR — overlapping resume corrupts live-run ownership

- Reproduction: strand an execution (SIGKILL after `run --detach`),
  restart, run `agentctl execution resume <E> <repo>` twice in parallel
  (or `run --detach` then `resume`); while the provider runs, `status`
  shows `live=false` and `cancel` returns `NO_LIVE_RUN` (or `LIVE_SIGNALLED`
  with no effect); the provider runs to `SUCCEEDED`.
- Durable state: consistent (one attempt, one `attempt.created`, Task
  `VERIFYING`); the defect is control-plane ownership and honesty.
- Why tests missed it: `concurrent_resumes_converge_on_exactly_one_attempt`
  builds a separate `ExecutionComposition` (separate `CancelRegistry`) per
  racer, uses an instant provider fixture, and asserts only attempt count;
  no keeper checks liveness/cancellation of the winner or a second request
  against a live run through the shared daemon composition.
- Direction: resume must claim exclusive live ownership in the shared
  composition before accepting (typed refusal while
  `cancels.is_live(execution)`; insert-if-absent instead of overwrite), and
  unregistration must be scoped to the registering run (owner token/flag
  identity) so a loser can never retire a winner. Keeper: one shared
  composition, slow provider, parallel resumes and run+resume, asserting a
  single acceptance, `live=true`, and effective cancel.

### B-RS2 — materialization-crash strand has no product recovery

- Reproduction: 20k-file repository; `run --detach`; SIGKILL the daemon
  during worktree creation; restart; resume → `resumed …`; daemon log
  `ambiguous partial Git state; refusing blind replay`; Task stays
  `RUNNING`, lease `ALLOCATED`, reservation intact, forever.
- Durable state: branch at exactly the lease base OID, no worktree
  directory, no registration, identity reservation owned by the execution.
- Why tests missed it: TASK-0054 crashes are dropped jobs (before any Git)
  or signal timings that never land inside `git worktree add -b`; the
  ALLOCATED keeper allocates through the library without Git side effects;
  G4 matrix covers a branch at an unexpected revision only.
- Direction: reconcile the provably-owned branch-only shape (durable
  identity reservation for this execution, branch at the recorded base, no
  commits beyond base, target path absent, not registered) as a resumable
  crash-A equivalent instead of ambiguous, or expose a bounded, journaled
  control-plane disposition for it; validate lease/Git consistency in the
  resume dispatch so irreducibly ambiguous shapes refuse typed instead of
  `resumed`. Keeper: randomized or deterministic kill inside
  `git worktree add` through the shipped binaries.

### B-OL — pre-v2 lease misclassified as v2

- Reproduction: TASK-0051 daemon with `--execution e2-strand` leaving an
  ALLOCATED lease (or a READY `e2-legacy` lease); open with the current
  daemon; resume → background `managed root changed under lease`;
  `cleanup_workspace` → `OwnershipMismatch`.
- Durable state: lease rows byte-identical (`agent/e2-…`, `ws-ws-…`); no
  Git mutation; Task stranded `RUNNING` / lease uncleanable.
- Why tests missed it: old-lease keepers use ids like `E/S`, `E-OLD/R`,
  or uppercase ids whose old outputs never start with `e2-`/`h2-`; the
  disjointness claim was argued, not enumerated.
- Direction: classify lease format from durable evidence that cannot
  collide with old outputs (e.g. v2 only when the persisted branch equals
  `branch_for_execution(owner)` AND the persisted worktree equals the v2
  rederivation; otherwise the compatibility path), and correct the
  disjointness documentation. Keeper: genuine old-scheme rows for
  `e2-`/`h2-` ids through resume and cleanup.

## 18. Gate status and next step

G5 stays `in_progress`; roadmap and `current_gate` unchanged. Follow-up
(narrow, not bundled with deferred hardening): TASK-0056 (B-CR),
TASK-0057 (B-RS2), TASK-0058 (B-OL), then an independent G5 closure
re-audit. F-BLK, F-MI, F-RA, F-SCAN, F-IPC, and DOC-3 may ride along only
where they touch the same code; otherwise they stay hardening.

`product/version.toml` → `0.1.0-dev.task0055` (gate G5). No G6 design or
implementation was started by this audit.
