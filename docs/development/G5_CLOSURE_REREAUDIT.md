# G5 Closure Re-Re-Audit (TASK-0052)

> Independent adversarial closure audit of G5 after the TASK-0051
> entry-atomicity + managed-identity remediation. Authority: `AGENT.md`,
> `INVARIANTS.md`, ADR-0005/0008/0009/0018/0022/0023/0024, the TASK-0052
> brief (closure criteria §30). `G5_CLOSURE_AUDIT.md` (TASK-0048) and
> `G5_CLOSURE_REAUDIT.md` (TASK-0050) are historical evidence and are not
> modified; `G5_ENTRY_ATOMICITY_REMEDIATION.md` (TASK-0051) is evidence under
> test, not authority.
>
> Verdict: **`G5_REMEDIATION_REQUIRED`** (§17). Two G5 blockers: managed
> branch identity is not safe on case-insensitive filesystems (B-CF, new),
> and admitted-but-not-attempted entries have no concrete recovery path
> (B-RS, reclassified by TASK-0052 §7B/§30). G5 stays `in_progress`; G6
> must not start.

## 1. Scope and method

Audit only. No production file was changed. Nine throwaway probe files were
executed and removed (copies kept outside the repository):

```text
crates/app-services/tests/zz_audit_cross_process.rs     cross-PROCESS entry races (service + store mode)
crates/app-services/tests/zz_audit_reservation_crash.rs reservation lifecycle; SIGKILL inside txns; post-commit abort
crates/app-services/tests/zz_audit_casefold.rs          real Git, case-insensitive FS, loose + packed refs
crates/app-services/tests/zz_audit_slug.rs              first-principles slug audit, 40k-input census
crates/app-services/tests/zz_audit_old_lease.rs         real v5 DB with old-scheme leases → v6, real Git
adapters/storage-sqlite/tests/zz_audit_migration.rs     fresh/v5 concurrent opens (threads+processes), interruption
adapters/fake-agent/tests/zz_audit_neg_control.rs       shared ExecutionPort contract negative controls
bins/agentctl/tests/zz_audit_recovery.rs                shipped binaries: post-admission strands, true SIGKILL+restart
bins/agentctl/tests/zz_audit_deadline.rs                shipped binaries: SQLite write lock vs IPC deadline
```

Every probe asserts the EXPECTED contract; a failing probe reproduces a
defect. Durable state was inspected (SQLite rows, journal, Git refs), never
return codes alone.

## 2. Repository and version inspected

- HEAD `b3b956d` (TASK-0050..0051), `product/version.toml` at
  `0.1.0-dev.task0051`, schema `SUPPORTED_SCHEMA_VERSION = 6`.
- Host: macOS, APFS **case-insensitive** (`core.ignorecase=true`), Git
  2.55.0 (files ref backend), Claude Code 2.1.268.
- Repository-state note: at 13:54:03 during this audit, six root documents
  (`AGENT.md`, `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`,
  `AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`,
  `ARCHITECTURE.md`, `EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md`,
  `PROJECT_CHARTER.md`) were modified outside this task (+6342 lines, no
  code). They were left untouched and are not part of TASK-0052.

## 3. Previous blockers and TASK-0051 claims

| TASK-0050 item | TASK-0051 claim | This audit |
|---|---|---|
| B1-concurrent (loser left RUNNING + Claimed/Started, no Execution) | one `IMMEDIATE` admission txn | **RESOLVED** (§4, cross-process) |
| B1 attempt-ID race detonating in background | `attempt_reservations` (0006) | **RESOLVED** (§5) |
| H4 `E/S` ≡ `E-2FS` | `%`-escaping + SHA-256 long form, "injective" | exact-byte pair **RESOLVED**; case-fold **NOT SAFE** (B-CF); "injective" overclaimed (§7) |
| ADR-0024 §6 backstop sentence | amended to atomic admission | accurate for admission; "recoverable" overclaimed (§6, §13) |
| B1 sequential | unchanged keepers | **RESOLVED** (keepers + probes) |

## 4. Atomic admission — independent reconstruction

`SqliteStore::admit_execution_entry` (`adapters/storage-sqlite/src/entry_store.rs:96-186`):

```text
require_admission_coherence (pure, pre-txn)
BEGIN IMMEDIATE                                   (transaction_with_behavior(Immediate))
  UPDATE tasks SET state=CLAIMED WHERE id AND state=READY    0 rows → Stale | NotFound
  UPDATE tasks SET state=RUNNING WHERE id AND state=CLAIMED  0 rows → Corrupt (unreachable)
  execution id present?        → Duplicate (names owning task)
  attempt id in execution_attempts? → Duplicate
  attempt id in attempt_reservations? → Duplicate
  INSERT executions; INSERT attempt_reservations
  INSERT journal task.claimed, task.started, execution.created  (same connection)
COMMIT                          (any Err → Transaction dropped → ROLLBACK)
```

Verified from code: single transaction, no nested write transaction, no
Git/filesystem/provider/network/sleep inside; expected-state guards inside
the transaction; PK/`UNIQUE` (`executions.id`, `attempt_reservations.attempt_id`,
`execution_attempts.attempt_id`) remain final guards; the only process-local
shared state in `app-services`/`storage-sqlite` is `LEASE_ID_COUNTER`
(lease-id minting, unrelated to admission). `OrchestrationService::enter`
preflights (execution, attempt, reservation, derived branch) are
read-before-write diagnostics; the production start path
(`bins/agentd/src/execution.rs:504`) reaches only this admission. Typed
conflicts map to `ALREADY_EXISTS` / `STALE_STATE` / `INVALID_TRANSITION`
(`dispatch.rs:301-350`); raw SQLite text never is the public contract.

### Concurrency results (TASK-0050 race shapes)

Two long-lived child OS processes, own SQLite connections, file barrier per
round, one durable DB. `service` = production `enter_execution`; `store` =
validated plans minted from a READY snapshot BEFORE the barrier, then
`admit_execution_entry` directly (bypasses every preflight, so only the
transaction decides). Every round inspected task state, execution owner,
reservation owner, attempt rows, lease rows, both journals, then retried the
loser.

| Scenario | Mode | Rounds | Overlapping | Violations | Loser outcome |
|---|---|---|---|---|---|
| A two tasks, same execution id | service / store | 100 / 100 | 100% / 100% | 0 / 0 | `Duplicate` ×100 / ×100; READY; rerunnable |
| B two tasks, same attempt id | service / store | 100 / 100 | 100% / 100% | 0 / 0 | `Duplicate`; no execution, no lease; rerunnable |
| C same task, distinct ids | service / store | 50 / 50 | 100% / 100% | 0 / 0 | `Stale`; exactly 1 claim/start/execution |
| D exact duplicate | service / store | 50 / 50 | 100% / 100% | 0 / 0 | `Stale`; journal exactly the triple; same-id retry refused |

Conclusion: correctness comes from SQLite `IMMEDIATE` serialization plus
in-transaction guards, across processes. Journal atomicity (INV-010): the
winner commits exactly `task.claimed → task.started → execution.created`;
losers commit nothing. Single winner ⇒ single background job ⇒ no double
lease or provider spawn.

## 5. Attempt reservations (migration 0006)

Schema (PRAGMA-verified): `attempt_id TEXT PRIMARY KEY`,
`execution_id NOT NULL → executions(id)`, `task_id NOT NULL → tasks(id)`
(both `NO ACTION`), `reserved_at NOT NULL CHECK(length>0)`, index
`attempt_reservations_execution`. Reservation/execution task agreement is
application-enforced (admission writes both from one plan), not a SQL
constraint.

`create_attempt_with_event` (`runtime_store.rs:118-213`): one `IMMEDIATE`
txn reads the reservation owner, refuses another execution with typed
`Duplicate`, inserts the attempt, deletes the reservation, appends
`attempt.created`, commits.

| # | Case | Result |
|---|---|---|
| 1 | owner consumes its reservation | Ok; reservation gone; attempt row present |
| 2/3 | foreign execution (hence foreign task) uses it | `Duplicate`; reservation intact; no attempt; no event |
| 4 | owner creates an **unreserved** id | Ok; own reservation left orphaned (HARD-2; unreachable via production) |
| 5 | re-create after consumption | refused; one `attempt.created` |
| 6 | two concurrent creates, same reserved id | exactly 1 Ok; 1 attempt row; 1 event |
| 7 | journal insert aborts inside the txn (trigger) | Err; reservation intact; no attempt |
| 8 | retry after 7 | Ok; consumed |
| 7C | **SIGKILL inside the consumption txn** (rollback journal present, child blocked in trigger) | after reopen: reservation intact, no attempt, no event; retry Ok |

No half-consumed identity is possible: consumption and insertion share one
commit.

## 6. Crash / restart boundaries (§7)

| Boundary | Evidence | Durable result | Recovery |
|---|---|---|---|
| A: SIGKILL inside admission txn | child blocked in `execution.created` trigger, `-journal` present | READY, no execution, no reservation, journal `created, marked_ready` | same-id retry Runnable ✔ |
| **B: crash after admission COMMIT, before `create_attempt`** | `abort()` after commit; shipped-binary SIGKILL during `git worktree add` + restart | RUNNING + execution (0 attempts) + reservation (+ `ALLOCATED` lease) | **none through the product** ✗ (B-RS) |
| B′: deterministic post-entry failure, no crash | shipped binaries: non-repository path; `--rev no-such-branch` | same as B | **none** ✗ (B-RS) |
| C: crash inside consumption txn | §5 7C | reservation intact | retry Ok ✔ |
| D: crash after attempt creation | production order is lease → attempt; abort after `create_attempt` | RUNNING + Ready lease + OPEN attempt | none (TASK-0048 P11, G11 deferral; same class as B) |
| E: crash after lease allocation, before materialization | abort after `allocate_workspace` | `ALLOCATED` lease | G4 `ensure_workspace_ready` → `ResumedAllocated` ✔ (library) |

Case B/B′ control-plane attempts (shipped binaries, after restart for B):
`status` → `RUNNING live=false`; `cancel` → `ALREADY_CONCLUDED … no attempts
recorded`; `run` same or fresh ids → `INVALID_TRANSITION` (run requires
READY); `task ready`/`claim` → invalid from RUNNING and BLOCKED; `block` then
`resume` returns to RUNNING; no resume operation exists in IPC v2; the
restarted daemon performs no reconciliation (lease stays `ALLOCATED`). The
only working path is an in-process library call
(`OrchestrationService::continue_workspace_cycle`, which resumed the stranded
entry to `Succeeded` in the probe); nothing in production invokes it after
restart. Non-repository paths and unresolvable base revisions are
synchronously detectable (read-only Git) yet are validated only after
admission, in the background (`start_run` checks lengths only).

## 7. Managed identity (H4) — slug, ref, path, digest

`slug_body` (`workspace_service.rs:97-113`): ASCII alnum/`-`/`_` pass through
with case preserved; `%` → `%%`; other bytes → `%XX` (uppercase); escaped
length > 64 → `h<sha256-lowerhex>-` (66). Branch `refs/heads/agent/<slug>`;
worktree `<root>/ws-<slug(workspace_id)>` with server-minted
`ws-<ms>-<pid>-<n>` ids.

First-principles results (40,048 generated/boundary inputs, 39,739 distinct
valid identities, independent decoder written from the grammar):

- readable form: decode(slug) == exact identity bytes for all 4,323 readable
  identities; alphabet `[A-Za-z0-9_%-]`; `%` never ambiguous (escape digits
  are never `%`); `/`, `\`, space, control, `.`, `@{`, `..`, `.lock`,
  trailing `.`/`/` impossible; Unicode escaped byte-exact (NFC ≠ NFD);
- digest form: 35,710 identities, all `h[0-9a-f]{64}-`; namespaces disjoint
  by length (≤ 64 vs 66) — a literal digest-shaped id is itself hashed;
- boundaries: escaped 63/64 readable, 65/66 digest, 256-byte ASCII / slash /
  percent / Unicode digest, shared-prefix long ids distinct;
- 0 exact collisions; `git check-ref-format` 442/442 accepted;
- `E/S` → `agent/E%2FS`, `E-2FS` → `agent/E-2FS` (TASK-0051 real-Git keeper
  also green).

**Injectivity conclusion (statement B + partial C).** The readable form is
provably injective over exact bytes. The long form is collision-resistant,
not injective: the valid `ExecutionId` domain (≤ 256-byte UTF-8) is far larger
than 2^256, so collisions exist by pigeonhole. A digest collision is detected
fail-closed where observable — sequential entry preflight
(`find_lease_owner_by_branch`) and git-cli `BranchCollision` (never silent
reuse) — but concurrent colliding entries would wedge, not alias. At ~2^-128
birthday probability this is an acceptable probabilistic identity **only if
documented as such**; `workspace.md` ("maps … injectively", "distinct
execution identities never share a managed branch"), the
`branch_for_execution` doc ("Injective across the whole valid ExecutionId
domain") and the `enter` comment ("Derivation is injective, so a hit means
foreign interference") overclaim (DOC-1).

**Case-insensitive filesystem conclusion — NOT SAFE (B-CF).** Distinct
identities differing only in ASCII case derive exact-distinct refs whose
loose-ref files collide (census: `A`/`a`, `ABC`/`abc`, `Execution`/`execution`,
`E/S`/`E/s`). Real Git on this host, through the real services:

```text
loose refs:   enter T-1/E-CASE → CreatedFresh (agent/E-CASE)
              enter T-2/E-case → Runnable (exact-string branch preflight misses)
              ensure E-case → BranchCollision  (show-ref --verify agent/E-case hits file E-CASE)
              → T-2 RUNNING, lease ALLOCATED, reservation stranded, re-entry NotRunnable,
                reconcile → CheckoutFailed(ambiguous)                         [wedge]
packed refs:  E-PACK Ready; `git pack-refs --all`; E-pack → CreatedFresh
              E-pack commits in its worktree → refs/heads/agent/E-PACK and W1 HEAD = E-pack's commit
              E-PACK commits in its worktree → refs/heads/agent/E-pack and W2 HEAD = E-PACK's commit
              loose files under refs/heads/agent: ["E-pack"]; E-PACK reconcile → CheckoutFailed(drift)
                                                                              [isolation breach]
```

Windows (NTFS case-insensitive; reserved device names `CON`/`NUL`/`AUX`/
`COM1`/`LPT9` pass through the slug unchanged) is exposed by the same design
argument; not runnable here.

## 8. Old WorkspaceLease compatibility

Real v5 DB (canonical 0001–0005 SQL) with old-scheme rows, real Git:
`ALLOCATED` `E/S` → `agent/E-2FS`; `ALLOCATED` 70-byte id → old FNV branch
`agent/h00112233AABBCCDD`; `READY` `E-OLD/R` → `agent/E-OLD-2FR` with real
worktree + `workspace.created`. Opened by current services: schema 6;
identity columns byte-identical after migration, after reconcile
(`ResumedAllocated`, `ResumedAllocated`, `AlreadyReady` — Git created
`agent/E-2FS`, never `agent/E%2FS`) and after restart (`AlreadyReady` ×3); new
identity `E-2FS` refused typed against the old owner (task stays READY);
cleanup of the old `READY` lease removed worktree and branch. Reconcile and
cleanup use persisted branch/path; the path check re-derives
`ws-<slug(id)>`, identical under both schemes for server-minted ids.
**Compatible.**

## 9. Migration 0006

Fresh → 6 and real v5 → 6 (rows preserved): 10 rounds × 8 concurrent thread
openers and 5 rounds × 6 concurrent **process** openers, each on fresh and v5
databases — 0 errors, all version 6. Fresh vs migrated schema: 14 objects
identical. Interrupted migration (pre-existing conflicting
`attempt_reservations`): typed `Migration` error, `user_version` stays 5,
nothing partially applied; retry after clearing → 6. TASK-0047 re-check
inside `IMMEDIATE` holds. **Safe.**

## 10. Provider-neutral contract and seam

Contract (`crates/execution-contract`) asserts only seam promises. FakeAgent
and Claude fixture invocations green. Negative controls: a correct inline
executor passes; all 8 broken executors are rejected (blank / unbounded /
partial success, failure-as-success, malformed-as-success,
timeout-as-Failed, ref embedding work ids, unstable ref). `cargo tree -i
provider-claude`: normal → `agentd` only; dev → `app-services` tests only.
No Claude semantics in Task/Storage/IPC domain code (only test fixtures and
provider-name guard tests). Admission and reservations carry no provider
data. A second provider still needs only an adapter crate, a composition
factory, and a contract invocation. **Neutral, meaningful, second provider
structurally addable.**

## 11. Provider process / cancel

Targeted keepers (§16): process-tree cancellation, pre-spawn cancel, timeout,
natural-exit grandchild pipe, output bounds, parser linearity units,
`LiveProcessTracker` cleanup, argv-only/no permission bypass — see census.
No TASK-0051 change touched these paths; TASK-0048/0050 resolutions stand.

## 12. Real Claude Code smoke — PASS

Shipped `agentd`/`agentctl`, isolated runtime/data dirs, Claude Code
2.1.268 (subscription auth, provider-owned), tool-free prompt that also
claims completion:

```text
task T-RRA  execution E-RRA-SMOKE  attempt A-RRA-SMOKE   run exit 0, 8 s wall
branch refs/heads/agent/E-RRA-SMOKE   worktree <data>/ai-engineering-os/workspaces/ws-ws-1789128348428-37679-0
evidence claude:session:6081fe22-fa6e-4e79-a604-c0ad34875827
attempt SUCCEEDED; Task VERIFYING (provider text "Task complete." did not complete it)
journal created > marked_ready > claimed > started > execution.created > workspace.created >
        attempt.created > attempt.concluded > submitted > routed_to_review > verification_began
reservations left 0; cancel → ALREADY_CONCLUDED
primary HEAD/branch unchanged, primary status clean; worktree HEAD == base
worktree porcelain: ?? data/sessions.db ; no process references the worktree
secret markers in DB 0; data dir 0700; shutdown complete, no daemon left
```

## 13. Deadline vs lock contention (§8)

Store busy timeout 30 s. Effective `run` client deadline: **5 s** frame I/O
timeout (`EXECUTION_RESPONSE_TIMEOUT` = 15 s is never reached). Shipped
binaries, external `BEGIN IMMEDIATE` holder:

- 20 s lock: client exits at 5.0 s with "ambiguous execution result (run not
  replayed) … re-probe status E-DL"; `ping` during the stall took 15.2 s
  (current-thread serve loop blocked in synchronous SQLite wait); lock
  released at 20.0 s → status `NOT_FOUND` (20.02 s) → `RUNNING` (20.27 s) →
  `VERIFYING` (21.32 s). The entry committed and the provider ran after the
  client gave up; discoverable via the client-minted id; not replayed.
- 40 s lock: client ambiguous at 5.0 s; server rolled back at the busy
  bound; status `NOT_FOUND`, task READY, same-id rerun later admitted.

Classification: late commit after client timeout is possible and starts
work, but is explicitly reported as ambiguous and discoverable → not a
blocker. Daemon-wide stall and the unreachable 15 s constant →
HARDENING_FOLLOWUP (HARD-1). Raw IPC clients leaving ids blank would lose a
server-minted identity on ambiguity (agentctl always mints).

## 14. Deferred findings reassessment

- **H5 — ACCEPTABLE_DEFERRED.** Production precheck is PATH presence only;
  `G5_CLAUDE_PROVIDER.md:139` now states exactly that; incompatible CLIs
  fail closed at parse. No 10 s probe on the 5 s path.
- **H7 — ACCEPTABLE_DEFERRED (root cause refined).** `data/sessions.db` is a
  user-level MCP server database (tables `card`, `drillquestion`, `session`,
  `turn`, …) created in the worktree through the inherited provider
  environment. It is untracked, blocks ordinary cleanup (`WorktreeDirty`,
  fail-closed) and would appear in G4 diff evidence (porcelain includes
  untracked files). G5 has no delivery/landing, so no delivered change set is
  contaminated today; provider-environment isolation (G6) must close it
  before any delivery gate. Privacy note: personal tool data lands inside
  the managed worktree.
- **H9 — HARDENING_FOLLOWUP (was deferred).** TASK-0051 took
  `workspace_service.rs` to 1552 LOC (`agentd/src/execution.rs` 1581); AGENT.md
  §20 requires decomposition or a documented reviewed exception above ~1500.
  No correctness defect traced to size.
- **Test hermeticity — HARDENING_FOLLOWUP (confirmed).** This audit's first
  probe run panicked before stopping five `target/debug/agentd` daemons
  (attributed by `lsof` to probe sandboxes, killed before verification);
  `g1_real_process_matrix` counts same-binary-path processes and would fail
  on such leftovers. Test-only; production unaffected.

## 15. Authority, isolation, test mutation, dependency

- Task ≠ Execution ≠ Attempt ≠ Session: session id only inside opaque
  `evidence_ref`; attempt `Succeeded` leaves Task VERIFYING (`PendingEvaluator`,
  smoke, `provider_binding`); no completion op on IPC v2; provider adapters
  hold no storage port. INV-002/004/009/012 hold.
- Isolation: managed worktree per execution, primary checkout untouched,
  protected/default branch refused, branch scope `refs/heads/agent/*`, race
  losers create no lease/worktree, `E/S`/`E-2FS` fixed — INV-006 holds;
  **INV-005 violated on case-insensitive filesystems (B-CF).**
- Test mutations (TASK-0051): schema expectations 5 → 6 follow migration
  0006; long-slug test `len < 32` → `len <= 80` + digest shape follows the
  authorized redesign and collision keepers were added — no acceptance
  weakening. `Cargo.lock` delta is the additive `sha2 0.10.9` subtree
  (`digest`, `block-buffer`, `crypto-common`, `generic-array`, `typenum`,
  `cpufeatures`) matching `sha2 = "0.10"`.
- `sha2`: single use (`sha256_hex`, managed digest identity), no prior hash
  stack, `MIT OR Apache-2.0`, no external project code imported.

## 16. Commands and census

```text
cargo fmt --all -- --check                                              clean (pre and post)
cargo check --workspace                                                 clean
cargo clippy --workspace --all-targets --all-features -- -D warnings    clean
baseline cargo test --workspace --no-fail-fast (pre-probe)              647 passed / 0 failed / 1 ignored
full runs ×3 (final tree, probes removed, no stale daemons before/after)
  run 1: 647 passed / 0 failed / 1 ignored (72 suites)
  run 2: 647 passed / 0 failed / 1 ignored (72 suites)
  run 3: 647 passed / 0 failed / 1 ignored (72 suites)
targeted repetitions: 46 runs, 0 failed
  entry_atomicity 3× (5/0/0)        concurrent_cycles 3× (2/0/0)     workspace_service 2× (12/0/0)
  slug units 2× (7/0/0)             provider_binding 2× (6/0/0)      failure matrices 2× (51/0/0)
  contract fake 2× / claude 2× (1/0/0)                                process_fixtures 5× (13/0/0)
  grandchild pipe 10× (1/0/0)       parser json units 2× (10/0/0)
  storage durability+runtime+workspace+journal 2× (89/0/0)            restart_durability_e2e 3× (1/0/0)
  agentd execution units 2× (9/0/0) execution_composition 2× (8/0/0) execution_binaries 2× (2/0/0)
probes (removed): cross_process 9/9 ok; slug 1/1 ok; migration 3/3 ok; neg_control 1/1 ok;
  old_lease 1/1 ok; reservation_crash 6 ok + 1 expected-contract failure (post-commit recovery);
  casefold 2/2 reproduce defect; recovery 3/3 reproduce defect; deadline 2/2 observational
real smoke: claude 2.1.268, PASS (§12)
```

Ignored: `provider-claude/tests/smoke.rs` (env-gated live test).

## 17. Findings and verdict

| ID | Classification | Scenario | Expected | Actual | Invariant / contract | Evidence |
|---|---|---|---|---|---|---|
| B-CF | **G5_BLOCKER** | valid ids differing only in ASCII case, same repo, case-insensitive FS | independent workspaces or typed refusal before mutation | loose refs: committed entry wedges RUNNING on `BranchCollision`; packed refs: two executions' branches and worktree HEADs alias bidirectionally | INV-005, INV-001, ADR-0023 lease/branch ownership, TASK-0050 H4 promotion rule, TASK-0052 §11/§21 | §7, zz_audit_casefold |
| B-RS | **G5_BLOCKER** | admission committed, then crash/restart or deterministic failure (non-repo path, bad `--rev`, B-CF) before `create_attempt` | truthful AND recoverable via a concrete product path | RUNNING forever; `run` INVALID_TRANSITION; no READY path, no resume op, no restart reconciliation; only an internal library call resumes | TASK-0052 §7B/§30, ADR-0024 §6 "recoverable", INV-001, INV-145 reconciliation | §6, zz_audit_recovery, zz_audit_reservation_crash |
| DOC-1 | DOCUMENTATION_DEFECT | identity docs | technically honest terms | "injective" for SHA-256 form and for FS identity | AGENT.md §5/§18 | §7 |
| DOC-2 | DOCUMENTATION_DEFECT | ADR-0024 §4/§6, remediation §2, `workspace.md` Crash/Reconciliation | describe implemented recovery | "recoverable sagas", "after restart the system reconciles" not implemented | ADR accuracy | §6 |
| HARD-1 | HARDENING_FOLLOWUP | external write lock during `run` | bounded, responsive daemon | serve loop stalls ≤30 s; late commit after 5 s ambiguity; 15 s constant unreachable | ipc.md deadlines | §13 |
| HARD-2 | HARDENING_FOLLOWUP | owner creates unreserved attempt id | reservation bound to first attempt | allowed, reservation orphaned (not reachable via production) | ADR-0024 §6 | §5 #4 |
| HARD-3 | HARDENING_FOLLOWUP | `cancel` on stranded pre-attempt execution | honest wording | `ALREADY_CONCLUDED … no attempts recorded` | ipc.md §16 | §6 |
| H5 | ACCEPTABLE_DEFERRED | per-request version probe | documented policy | PATH-only, documented | ADR-0024 §7 | §14 |
| H7 | ACCEPTABLE_DEFERRED | provider-environment artifacts in worktree | isolated before delivery | `?? data/sessions.db` (user MCP) | INV-005 (G6) | §12, §14 |
| H9 | HARDENING_FOLLOWUP | file size | ≤1500 LOC or reviewed exception | 1552 / 1581 LOC | AGENT.md §20, INV-081 | §14 |
| TEST-H | HARDENING_FOLLOWUP | stale same-path daemons | hermetic tests | process-count test sensitive | AGENT.md §5 | §14 |

Resolved and re-verified: B1 sequential, B1 concurrent (cross-process),
attempt-id race, journal atomicity, reservation ownership/consumption/crash
safety, exact `E/S`/`E-2FS` collision, readable-form reversibility, old-lease
compatibility, migration 0006, contract neutrality and negative controls,
completion authority, real provider.

**Verdict: `G5_REMEDIATION_REQUIRED`.** TASK-0051 genuinely fixed the
concurrent entry race and the exact-byte H4 collision, but managed identity
is unsafe on the host's own case-insensitive filesystem (up to cross-execution
branch aliasing), and an admitted entry that fails or crashes before its
attempt cannot be recovered through the product. G5 stays `in_progress`.

## 18. Follow-up (narrow, not bundled with deferred hardening)

1. **TASK-0053 — G5 Case-Fold-Safe Managed Workspace Identity.** Make the
   managed branch derivation injective under ASCII case folding (and reject
   or escape Windows reserved device names), compare/record managed branches
   case-insensitively at the entry preflight and store level, correct DOC-1
   terminology (readable form injective; digest form collision-resistant with
   fail-closed detection), preserve persisted old leases verbatim; real-Git
   keepers on a case-insensitive filesystem for loose and packed refs.
2. **TASK-0054 — G5 Post-Admission Entry Recovery Path.** Validate repository
   path and resolve base revision before admission (read-only, typed refusal,
   no mutation); provide a bounded, journaled, control-plane recovery for
   admitted entries with no attempt and no provider side effects (resume or
   abandon-to-rerunnable, decided by ADR since no RUNNING→READY edge exists),
   exercised through a real daemon restart; record an explicit ADR decision
   for stranded OPEN attempts (bounded conclusion vs documented G11
   deferral); amend ADR-0024 §4/§6 and `workspace.md` (DOC-2).
3. Then a G5 closure re-audit. G6 (and its design ADRs) must not start
   before `G5_CLOSED`.

## 19. Planning / version

`product/version.toml` → `0.1.0-dev.task0052` (gate G5). Queue:
TASK-0052 completed; TASK-0053 and TASK-0054 queued. Roadmap G5 stays
`in_progress`; G6 not started.
