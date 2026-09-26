# G5 Final Independent Adversarial Closure Re-Audit (TASK-0059)

> Independent closure re-audit of G5 after the TASK-0056 (live-run
> ownership, B-CR), TASK-0057 (branch-only materialization recovery,
> B-RS2) and TASK-0058 (pre-v2 lease classification, B-OL) remediations.
> Their completion reports were treated as claims under test, not as
> evidence. This auditor implemented none of them.
>
> Historical records are authority for what was found *then* and are not
> rewritten here: `G5_CLOSURE_AUDIT.md` (TASK-0048),
> `G5_CLOSURE_REAUDIT.md` (TASK-0050), `G5_CLOSURE_REREAUDIT.md`
> (TASK-0052), `G5_INDEPENDENT_CLOSURE_AUDIT.md` (TASK-0055) and the
> TASK-0049/0051/0053/0054/0056/0057/0058 remediation records.
>
> **Verdict: `G5_CLOSED` (§16).** G6 Security is the next gate. No G6
> implementation was started by TASK-0059.

---

## 1. Audited tree

The product stack is intentionally uncommitted. This audit ran against
exactly one working tree and did not change it.

| Item | Value |
| --- | --- |
| HEAD | `246c972d501ed5d684b17cf59018370598c7b15c` ("fix: record TASK-0057 branch-only recovery GREEN") |
| Branch | `task-0057-branch-only-recovery` |
| Staged | none (`git diff --cached` empty) |
| Tracked diff | 42 files, +9074 / −322 |
| Tracked-diff fingerprint | `sha256 8cd3f1d18b3220177e0ee504e58ef557fa041407dc8df2a9be8e5c4a20e69c21` (`git diff HEAD \| shasum -a 256`) |
| Untracked entries | 20 |
| Toolchain | rustc 1.95.0, cargo 1.95.0, git 2.55.0, Claude Code 2.1.268 |
| Host | macOS (Darwin 25.6.0), APFS |

The same fingerprint was recomputed after all testing and is byte-identical,
and the untracked set is unchanged (20 entries). The audit mutated no
product file.

**TASK-0053…0058 product/doc changes** (audited): `crates/app-services/src/managed_identity.rs`
(new), `crates/app-services/src/workspace_service.rs`, `crates/workspace/src/close.rs` (new),
`adapters/storage-sqlite/migrations/0007_managed_identity_reservations.sql` (new),
`adapters/storage-sqlite/src/{entry_store,workspace_store,migration,lib}.rs`,
`adapters/git-cli/src/{create,observe,lib}.rs`, `bins/agentd/src/{execution,lib}.rs`,
`bins/agentctl/src/main.rs`, `crates/ipc-contracts/*`, `crates/workspace/src/port.rs`,
their keeper suites, `ADR-0023/0024/0025`, `workspace.md`, `ipc.md`,
`planning/tasks/queue.yaml`, `product/version.toml`.

**Unrelated / pre-existing dirt** (not touched, not audited):
`Support/openvidia/proxy_config.json`, `data/sessions.db` (itself a
provider artifact — see H7 §13), and whitespace-only churn in
`AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`,
`EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md`,
`AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`.

**Development version after this task:** `0.1.0-dev.task0059` (SemVer stays `0.1.0`).

Every probe in this audit lived outside the repository (an external
`g5-audit-probe` crate path-depending on the workspace crates, shell
harnesses, and a separate clone of the historical revision). No
production implementation was modified to make the audit pass.

---

## 2. G5 closure criteria, re-derived

Derived from the canonical architecture, not copied from TASK-0055.

`AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §79 defines the gate:

> **G5 — First real provider.** One provider fully passes the
> AgentProvider contract. After that a second provider must be addable
> without changes to Task/Storage/UI core.

Everything G5 must satisfy therefore falls out of that sentence plus the
invariants and ADRs already accepted when the gate opened:

| # | Current G5 requirement | Source |
| --- | --- | --- |
| C1 | A real provider executes through the production composition and returns canonical evidence | gate §79; ADR-0024 |
| C2 | Provider never gains Task completion authority; success routes to VERIFYING | INV-009, ADR-0018 |
| C3 | Task ≠ Execution ≠ Attempt ≠ ProviderSession boundaries hold | INV-002, ADR-0005 |
| C4 | Provider adapters never mutate domain state; domain/app crates stay provider-neutral | INV-004, INV-012, INV-048, INV-059 |
| C5 | A second provider is structurally addable without Task/Storage/UI redesign | gate §79 |
| C6 | Durable state survives process restart; durable implies recoverable through the product | INV-001, INV-078, ADR-0025 |
| C7 | Every durable mutation is atomic with its journal facts; exactly-once lifecycle facts | INV-010, ADR-0006, ADR-0022, ADR-0024 §6 |
| C8 | Git/filesystem and SQLite are separate failure domains, reconciled — never one ACID transaction, never blind replay | INV-145, ADR-0023 |
| C9 | Workspace ownership is proven, never guessed; ambiguity fails closed; dirty work is never silently destroyed | ADR-0023, workspace.md |
| C10 | Managed identity is collision-safe on real (including case-insensitive) filesystems and real Git, loose and packed refs | ADR-0023 amendment, TASK-0053 |
| C11 | Persisted pre-v2 leases keep their identity byte-for-byte and cannot be confused with current v2 | ADR-0023 amendment, TASK-0058 |
| C12 | Concurrent admission and concurrent live execution are honest: one owner, one Attempt, one provider launch, typed losers | ADR-0024 §6, ADR-0025 §4 + TASK-0056 amendment |
| C13 | Cancellation and status report reality | ADR-0025 §4 |
| C14 | Full workspace verification is clean | AGENT.md / repo convention |

**Explicitly NOT G5:** OS sandboxing, policy engine, capability
enforcement, secrets broker, provider-environment isolation (G6 —
`workspace.md` §"Git Authority" line 270 states *"Git isolation != OS
sandbox (G6 owns sandboxes; INV-076)"*); adaptive planning (G7);
background supervision and post-Attempt orphan recovery (G11); retry
ergonomics and test hermeticity (general hardening).

A future gate does not excuse a violated current invariant. Nothing in
this audit found one.

---

## 3. Historical blocker matrix

Every historical G5 blocker re-tested independently.

| Area | Result | Evidence |
| --- | --- | --- |
| Sequential admission | PASS | §11 full runs; `admission.sh` |
| Concurrent admission (cross-process) | PASS | §8 |
| Same-Task concurrent run | PASS — 1 admitted / 7 typed `Stale{expected READY, found RUNNING}` | §8 |
| Duplicate Execution ID | PASS — typed `Duplicate` | §8 |
| Duplicate Attempt identity | PASS — typed `Duplicate("attempt … already reserved")` | §8 |
| Exact duplicate request | PASS — 1 admitted, 5 typed refusals | §8 |
| Admission/journal atomicity | PASS — `execution.created == executions == attempt_reservations == managed_identity_reservations`; no attempts created inside admission | §8 |
| Reservation ownership / consumption / crash safety | PASS — `att_res` 1 while stranded, 0 after recovery, in every recovered round | §6, §7 |
| E/S vs E-2FS | PASS — distinct refs and worktrees, real Git, loose + packed | §4 |
| E-CASE vs E-case | PASS — same | §4 |
| Loose refs | PASS | §4, §6, §12 |
| Packed refs | PASS | §4, §6, §12 |
| Old WorkspaceLease compatibility | PASS — genuine schema-6 rows | §5 |
| Provider contract | PASS — shared `execution_contract::run_execution_port_contract` | §10 |
| FakeAgent contract | PASS | §10 |
| Claude contract | PASS — real Claude 2.1.268 | §9, §10 |
| Provider cannot self-complete | PASS — adversarial prompt, Task stayed VERIFYING | §9 |
| Second provider structurally addable | PASS | §10 |
| Post-admission / pre-Attempt recovery | PASS — real SIGKILL | §6 |
| Daemon restart | PASS | §6, §9, §12 |
| Cancellation honesty | PASS | §7 |
| Live-run ownership | PASS | §7 |
| Materialization crash recovery | PASS — including real branch-only | §6 |

No previously closed blocker regressed.

---

## 4. B-CF — case-fold managed identity: **RESOLVED**

**Filesystem proven case-insensitive, not assumed.** `diskutil` reports
APFS; the decisive evidence is empirical — writing `CaseProbe` and
stat-ing `caseprobe` succeeds on the repository volume, on the scratchpad,
and on the default `TMPDIR`. All three: `CASE_INSENSITIVE`.

**Hostile corpus (47 distinct ExecutionIds)** — uppercase/lowercase pairs,
`/`, `%`, `%%`, `%2F`, `%252F`, NFC `é` vs NFD `e` + combining acute,
`Ä`/`ä`, emoji (single and doubled), `e2-`, `e2-x`, `e2-legacy`,
`E2-LEGACY`, `h2-`, `h2-legacy`, `H2-LEGACY`, a literal 64-hex
`h2-000…0`, `CON`/`con`/`NUL`/`COM1`/`LPT9`/`AUX`/`PRN.txt`, trailing dot
and trailing space, `.`, `..`, 64/65/200-char runs, and a control byte.
Every derivation held:

- canonical lowercase (`is_canonical_v2_slug`, `to_ascii_lowercase` fixed point);
- ASCII case-fold stable;
- no `/` or `\`, never `.`/`..`, no trailing space or dot;
- neither the slug nor `ws-<slug>` is a Windows reserved basename;
- `BranchRef` valid, inside `refs/heads/agent/`;
- 41 readable forms decoded **byte-exactly** back to the source bytes
  (`decode_readable_slug` inverts `escape_body`); 6 digest forms;
- 47 inputs → 47 distinct slugs (no collision).

**No normalization ambiguity:** NFC `\u{00E9}` and NFD `e\u{0301}`
derive different slugs, so Unicode bytes are preserved rather than folded.

**Digest terminology is honest.** `managed_identity.rs` states digest
outputs are "collision-resistant, NOT mathematically injective — a bounded
hash of a larger domain cannot be injective (pigeonhole)", with the durable
backstop in migration 0007 (`branch_ref` PRIMARY KEY, `execution_id`
UNIQUE). `workspace.md` repeats the same distinction. Accurate.

**Real Git, real case-insensitive volume.** `E-CASE`, `E-case`, `E/S`,
`E-2FS` were materialized through `ensure_workspace_ready` in one
repository, loose refs and after `git pack-refs --all`:

```
E-CASE -> refs/heads/agent/e2-%45-%43%41%53%45
E-case -> refs/heads/agent/e2-%45-case
E/S    -> refs/heads/agent/e2-%45%2f%53
E-2FS  -> refs/heads/agent/e2-%45-2%46%53
```

Case-folded worktree paths: 4 distinct. Case-folded branch names: 4
distinct. Each worktree took its own commit; each worktree `HEAD` equalled
its own branch tip; 4 distinct tips; the primary checkout stayed on `main`.
A fresh service instance (restart) resolved every lease to the identical
branch and path. Cleanup of a worktree carrying unmerged commits refused
with a typed `cleanup_unsafe … branch preserved` — fail-closed, never a
cross-deletion.

---

## 5. B-OL — pre-v2 lease classification: **RESOLVED**

### 5.1 Genuine historical data (not hand-written rows)

TASK-0058's own keepers insert lease rows with raw SQL. This audit did not
rely on that. A separate clone was checked out at **`b3b956d`** (TASK-0051)
— verified to contain migrations `0001`–`0006` only, the `%XX`/`%%`
escape with an `h<sha256>-` fallback, and no `managed_identity_reservations`
table — and a small external producer drove that historical code to create
real leases through the historical `TaskService` / `RuntimeService` /
`OrchestrationService::enter_execution` / `WorkspaceService` against real
Git.

Result: a genuine `PRAGMA user_version = 6` database containing

- READY leases: `e2-legacy`, `h2-legacy`, `E-ordinary`
- ALLOCATED leases: `e2-alloc`, `h2-alloc`, `E-ord-alloc`
- ALLOCATED **pre-attempt strands** admitted through the historical
  atomic admission path (task `RUNNING`, attempt reservation present,
  zero attempts): `e2-strand`, `h2-strand`, `E-Strand`

with historical branches `refs/heads/agent/e2-legacy`,
`refs/heads/agent/h2-legacy`, `refs/heads/agent/E-ordinary`,
`refs/heads/agent/E-Strand` — uppercase preserved, i.e. genuinely pre-B-CF.

The two historical derivations were verified **from history, not from the
current code's comments**: `git show 07a8b90:…/workspace_service.rs` gives
the G4 scheme (`raw.len() > 64 → h{:016X}` FNV-1a, else alnum/`-`/`_`
passthrough with `-XX` uppercase hex); `b3b956d` gives the TASK-0051 scheme
(`%%` for `%`, `%XX` uppercase hex, `h<sha256-lowerhex>-` past a 64-char
escaped budget). `legacy_g4_slug` and `legacy_task0051_slug` in the current
code reproduce both exactly.

### 5.2 Migration

Opening the genuine schema-6 database with current code migrates
`user_version` 6 → 7. Every `(execution_id, branch_ref, worktree_ref)`
triple is byte-identical before and after, and
`managed_identity_reservations` stays **empty** — no backfill, no
rederivation.

### 5.3 Historical READY cleanup

For `e2-legacy`, `h2-legacy` and `E-ordinary`, with loose refs and after
`git pack-refs --all`: cleanup used the persisted branch and path, removed
exactly that worktree, removed exactly that branch, left every other
`refs/heads/agent/*` in place, and left the primary checkout on `main`.

### 5.4 Historical ALLOCATED resume

For `e2-alloc`, `h2-alloc` and `E-ord-alloc`, loose and packed: current
`ensure_workspace_ready` resumed each on its persisted identity
(`ResumedAllocated`), the branch stayed `refs/heads/agent/<id>` verbatim,
and post-recovery commits produced three independent branch heads in both
ref layouts.

### 5.5 Current-v2 corruption must not fall through to legacy

Fourteen damaged or mixed rows, each exercised against **both** the
read-only resume preflight and the mutating `ensure_workspace_ready`:

| Row | Refusal |
| --- | --- |
| v2 branch + foreign v2 path | `ownership_mismatch: … matches neither current v2 nor a supported historical identity` |
| foreign v2 branch + correct v2 path | same |
| v2 pair, reservation owned by another execution | `ownership_mismatch: current-v2 lease … has no durable managed-identity reservation` |
| v2 pair, reservation points at another branch | `ownership_mismatch: … branch … disagrees with durable reservation …` |
| v2 pair, **no** reservation | `ownership_mismatch: current-v2 lease … has no durable managed-identity reservation` |
| v2 branch + TASK-0051 path | pair-mismatch refusal |
| TASK-0051 branch + v2 path | pair-mismatch refusal |
| G4 branch + TASK-0051 path (cross-algorithm) | pair-mismatch refusal |
| TASK-0051 branch + G4 path (cross-algorithm) | pair-mismatch refusal |
| historical pair belonging to another execution | pair-mismatch refusal |
| historical-looking pair outside the managed root | pair-mismatch refusal |
| historical branch + traversal path | pair-mismatch refusal |
| `e2-`-looking OLD branch + arbitrary path | pair-mismatch refusal |
| `h2-`-looking OLD branch + arbitrary path | pair-mismatch refusal |

All 14 refused on both paths with **zero** `refs/heads/agent/*` created.
"Not v2" never became "legacy": classification demands an exact complete
pair from one supported derivation, and a current-v2 pair additionally
demands its durable reservation.

Cross-checking the historical derivations used here were written
independently of production code (re-implemented in the probe from the
`07a8b90` / `b3b956d` sources), so this is not the classifier grading
itself.

### 5.6 Namespace overlap is real and handled

The root hazard is genuine: a pre-v2 ExecutionId `e2-foo` produced branch
`refs/heads/agent/e2-foo`, and a *new* v2 execution named `foo` derives
exactly `refs/heads/agent/e2-foo`. Classification distinguishes them by the
complete pair, and allocation refuses the collision outright:

```
branch_collision: managed branch refs/heads/agent/e2-foo is already owned by
execution e2-foo in repository repo-local:bo; refusing reuse
```

No duplicate-branch lease row is created (verified: 1 row after the attempt).

---

## 6. B-RS / B-RS2 — post-admission, pre-Attempt recovery: **RESOLVED**

### 6.1 Real daemon death, not a simulation

TASK-0057's keeper reaches the branch-only shape through a `git` shim that
intercepts `git worktree add -b`. This audit did not rely on that. Two
randomized campaigns (34 rounds total) ran the **shipped binaries** against
a non-trivial repository (1600 files), issuing `agentctl execution run
--detach` and then `kill -9` on the real `agentd` at a pseudo-random offset
in `[5 ms, 555 ms]`, killing orphaned `git` children, then starting a
**genuinely new** daemon and recovering only through
`agentctl execution status` / `execution resume`. No manual Git surgery
anywhere.

Observed shapes and outcomes:

| Shape | Rounds | Outcome |
| --- | --- | --- |
| **A0** killed before admission (no execution) | 8 | Task stays `READY`; `resume` → `NOT_FOUND`; zero rows, zero refs. No orphan. |
| **A** admitted, no lease | 5 | `resume` refuses typed `INVALID_ARGUMENT: repository ref is required when no workspace lease exists yet`; supplying `--ref` recovers fully (verified). |
| **B** `ALLOCATED`, Git untouched | 1 | recovered (`ResumedAllocated`) |
| **D** **branch-only** (branch created, no worktree, no registration) | 1 | recovered — real SIGKILL, no shim |
| **E** worktree registered, finalization not durable | 19 | recovered (`RecognizedGitState`) |

Every recovered round: `attempts = 1`, `workspace.created = 1`,
`attempt.created = 1`, `attempt_reservations = 0` (consumed),
`managed_identity_reservations = 1`, lease `READY`, Task `VERIFYING`.

Boundary F (crash before Attempt creation) is the class all of B/D/E fall
into by construction — no round ever produced an attempt before the kill
(`att_before = 0` in all 34).

### 6.2 The exact branch-only proof

Constructed directly (allocate lease, create the managed branch at the
recorded base, leave the path absent) and driven through the service, loose
refs and packed:

- `EnsureOutcome::RecoveredBranchOnly`, lease → `Ready`, branch tip
  unchanged and equal to the recorded base;
- `workspace.created` exactly once;
- a second `ensure_workspace_ready` returns `AlreadyReady` and does **not**
  duplicate `workspace.created`.

### 6.3 Branch-only adversarial matrix

Each case run with loose refs and after `git pack-refs --all`:

| Attack | Result |
| --- | --- |
| branch tip moved forward (extra commit) | `checkout_failed: … points at <oid>, expected exact recorded base <oid>; refusing branch-only recovery` |
| branch checked out in another worktree | `ownership_mismatch: … already registered at <path>; refusing duplicate materialization` |
| target path already exists (directory) | `worktree_collision: … requires an absent target path` |
| target path is an unrelated file | `worktree_collision` |
| target path is a foreign checkout | `worktree_collision` |
| target path is a symlink out of the root | `ownership_mismatch: … escapes the managed root; refusing` |
| identity reservation deleted | `ownership_mismatch: current-v2 lease … has no durable managed-identity reservation` |
| identity reservation owned by a foreign execution | same |
| competing lease claiming the same worktree path | `storage_failure: multiple workspace leases claim worktree reference …` |
| competing lease claiming the same branch | see finding **N-2** (§13) — state unreachable through the product |

In every refused case the branch tip was **byte-identical before and
after**: no blind deletion, no reset, no `-D`.

### 6.4 Crash again during recovery (§13 of the task)

Branch-only strand → perform only the Git half of recovery (attach the
worktree) → die before finalization. The lease is still `ALLOCATED`, with
zero `workspace.created`. Restart and resume classify it as the
already-recognized complete-Git/`Allocated` shape
(`RecognizedGitState`), reach `Ready`, and emit `workspace.created`
**exactly once**. No progressive corruption.

---

## 7. B-CR — live-run ownership and cancellation: **RESOLVED**

All races run on **one shared production `ExecutionComposition`** (the
shape `agentd` builds once per daemon), with a slow provider fixture so the
winner stays live.

**2 and 8 concurrent resumes:** exactly one accepted owner each time;
every loser typed `CONFLICT` ("already has a live run; status shows the
owner"); the winner reported `live=true`; `cancel` reached the real owner
(`cancel_requested=true`, `run_state=LIVE_SIGNALLED`); `live=false` only
after the owner actually exited; exactly one attempt.

**ABA:** owner A claims → A releases through the production `abort_start`
path → B claims and goes live → a **late, stale** A cleanup with A's token
is inert: B stays `live=true`, `cancel` still reaches B, and B retires
itself on exit. A stale or losing path can neither unregister the winner
nor replace its cancel handle nor flip `status`.

**run + resume for one execution:** the resume loser is refused typed with
no background continuation and no registry mutation.

`CancelRegistry::try_claim` is insert-if-absent under one mutex with no
provider/Git/database work inside the critical section; `release` is
token-scoped. Reading the code and the behaviour agree.

### 7.1 Admission-commit → live-owner-claim window (task §9, high priority)

`start_run` commits the atomic admission transaction and *then* calls
`try_claim`. The window is reachable in principle because the client may
supply `--execution <E>`, so a concurrent `execution resume <E>` can target
the same identity.

It was attacked directly: **660 rounds** across four campaigns of a `run`
and a resume-hammer thread released from a shared barrier, the resume
thread issuing up to 250 dispatches per round (>30 000 resume dispatches
total), with the managed root pre-created so the probe measured this window
and not the directory race reported separately as **N-1**.

Results:

- `resume_won = 0` in every campaign — a competing resume never won the
  claim inside the commit→claim gap. Resume must complete a durable plan
  read plus two real `git` subprocesses before it claims; `run`'s gap is a
  handful of instructions.
- **Never two owners.** In every round: `attempts ≤ 1`, leases `≤ 1`,
  `workspace.created ≤ 1`, `attempt.created ≤ 1`.
- With a launch-counting provider fixture: **exactly one provider launch**
  per round, 180 rounds, including rounds where resume hammered past the
  run's completion.
- Resume was never accepted for an execution that already recorded an
  attempt (checked at the instant of acceptance, 120 rounds × 250
  dispatches).
- In 13–22 % of the highest-contention rounds both requests reported
  acceptance. Those are **sequential**, not concurrent: the run's
  background half failed pre-Attempt and released its token, after which
  `resume` legitimately recovered the strand — the designed path. The
  durable outcome in those rounds was still exactly one attempt, one
  `workspace.created`, one `attempt.created`, one provider launch, and one
  Task reaching `VERIFYING`. (The `run` acceptance whose background then
  fails is the pre-existing **F-RA** deferral, unchanged.)

If a `run` ever did lose this claim, the response is a typed `CONFLICT`
naming the real owner and the execution proceeds under that owner — the
state stays consistent and recoverable. **Not a blocker.**

---

## 8. Admission, reservation and journal atomicity

Attacked **across OS processes** (N separate processes opening one SQLite
file through `OrchestrationService::enter_execution`, which is how two
daemons or a daemon plus a tool would contend):

| Attack | Result |
| --- | --- |
| same Task, 8 processes, distinct ids | 1 `ADMITTED`; 7 `Stale { expected: "READY", found: "RUNNING" }` |
| duplicate Execution ID across 3 Tasks | 1 `ADMITTED`; 2 `Duplicate("execution E-dup already exists …; supply a fresh execution identity")` |
| duplicate Attempt identity across 3 executions | 1 `ADMITTED`; 1 `Duplicate("attempt A-dup is already reserved by execution E-x2 (task T-b) …")`; 1 `NotRunnable` |
| exact duplicate request × 6 | 1 `ADMITTED`; 5 typed refusals |

Durable aftermath across all four: `executions = 4`,
`attempt_reservations = 4`, `managed_identity_reservations = 4`,
`task.claimed = 4`, `task.started = 4`, `execution.created = 4`,
`execution_attempts = 0`. One task that lost a duplicate-id race
(`T-c`) stayed `READY` — **no unrelated Task lifecycle mutation, no orphan
execution, no orphan reservation, no incorrect journal fact**. No Git,
filesystem or provider work occurred inside the admission transaction
(INV-145 preserved).

Journal/state alignment under **randomized real daemon kills** (§6): in
every recovered round the journal read
`task.created → task.marked_ready → task.claimed → task.started →
execution.created → workspace.created → attempt.created →
attempt.concluded → task.submitted → task.routed_to_review →
task.verification_began`, each exactly once, with the attempt reservation
consumed exactly when the Attempt was created. Recovery invented no
duplicate fact. No state/journal split-brain was observed in any probe.

---

## 9. Real Claude through the production path

`agentctl → IPC → agentd → production execution composition →
WorkspaceService → provider-claude → real claude`. Provider:
**Claude Code 2.1.268** at `~/.local/bin/claude`; the daemon
resolved it from `PATH` with no test injection.

**A — normal current-v2 run.** Accepted in 9.6 s. Task → `VERIFYING`.
Attempt `SUCCEEDED` with `claude:session:c184126c-…`. Managed identity
reservation `refs/heads/agent/e2-%45-%41` with `origin = admission`.
Attempt reservation consumed. Journal exactly-once and correctly ordered.
Primary checkout clean, on `main`.

**B — real post-admission/pre-Attempt restart + resume.** `run --detach`,
`kill -9` the daemon at 350 ms → durable state `executions=1, attempts=0,
lease=ALLOCATED, reservation=1`, two managed branches present. A new daemon
surfaced the strand on startup; `execution status` honestly reported
`RUNNING live=false`; `execution resume` re-drove the SAME execution with
its reserved attempt into real Claude. Task → `VERIFYING`,
`workspace.created = 1`, `attempt.created = 1`, `attempt.concluded = 1`,
reservation consumed, primary checkout clean.

**C — genuine historical strand → current resume → real Claude.** The
schema-6 `E-Strand` strand (branch `refs/heads/agent/E-Strand`, uppercase,
pre-B-CF) resumed through the current daemon into real Claude. Task →
`VERIFYING`; the persisted branch and worktree reference were byte-identical
after recovery; `workspace.created`, `attempt.created`, `attempt.concluded`
each exactly once; primary checkout on `main`.

**Completion authority.** A Task whose objective instructed the model to
declare *"Task complete. The task is COMPLETED and verified."* ran to
provider success. Task state afterwards: **`VERIFYING`**, attempt
`succeeded` with evidence only. The provider could not complete its own
Task (INV-009, ADR-0018). The gated smoke keeper
(`RUNLITH_CLAUDE_SMOKE=1 cargo test -p provider-claude --test smoke --
--ignored`) also passed against real Claude: `G5_SMOKE_OK`,
`claude:session:0aa34eb0-…`, 8.1 s.

**Cleanup.** After every real-Claude scenario: zero live `claude`
processes, zero `agentd` processes after shutdown, no reservation leak, no
live-owner leak.

---

## 10. Provider neutrality, FakeAgent, second provider

- `crates/task`, `crates/agent-runtime`, `crates/scheduler`,
  `crates/workspace`, `crates/evaluation`, `crates/execution-contract` and
  `adapters/storage-sqlite` contain **no** Claude-specific production
  logic. The only `claude` occurrences are token lists inside
  `storage-sqlite` *test* assertions that check leakage.
- `app-services` `[dependencies]` are provider-neutral
  (`task-domain, agent-runtime, workspace, storage-sqlite, scheduler,
  evaluation, sha2`). `provider-claude` appears only under
  `[dev-dependencies]`, used by one test. **Production does not require
  Claude.**
- Recovery planning (`OrchestrationService::plan_pre_attempt_resume`),
  workspace compatibility (`managed_identity.rs`, `workspace_service.rs`)
  and Task semantics contain no provider branch.
- `provider-claude` is adapter-only: its sole domain contact is
  `impl ExecutionPort for ClaudeExecutor`.
- **Second provider structurally addable:** `execution_contract::run_execution_port_contract`
  is a shared contract suite already satisfied by **two** independent
  implementations — `adapters/provider-claude/tests/execution_port_contract.rs`
  and `adapters/fake-agent/tests/execution_port_contract.rs`. FakeAgent
  drives the same core seam with no process, network, Git or SQLite
  dependency, and executor binding is composition-owned
  (`ExecutionComposition::provider_executable`, never client-supplied).
  Adding a provider needs a new `ExecutionPort` impl plus a composition
  line — no Task, Storage or UI core change.

---

## 11. Verification runs

Static gates, audited tree:

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean (exit 0) |
| `cargo check --workspace` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean (exit 0) |
| `cargo test --workspace --no-fail-fast` | see below |

**Every full workspace run, reported in full:**

| # | Mode | Suites | Passed | Failed | Ignored | Exit | Wall | Machine state |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | default parallel | 79 | 722 | 0 | 1 | 0 | ~113 s | quiet |
| 2 | default parallel | 79 | 721 | **1** | 1 | 101 | — | **contaminated** — this auditor's own probe daemons were running |
| 3 | `--test-threads=1` | 79 | 721 | **1** | 1 | 101 | — | **contaminated** — same |
| 4 | default parallel | 79 | 722 | 0 | 1 | 0 | 116.61 s | quiet |
| 5 | `--test-threads=1` | 79 | 722 | 0 | 1 | 0 | 234.39 s | quiet |
| V | default parallel (post status-update re-verification) | 79 | 721 | **1** | 1 | 101 | — | quiet — `agentd --lib` flake, see TEST-H2 in §13 |
| 6 | default parallel | 79 | 722 | 0 | 1 | 0 | — | quiet |
| 7 | default parallel | 79 | 722 | 0 | 1 | 0 | — | quiet |
| 8 | default parallel | 79 | 722 | 0 | 1 | 0 | — | quiet |
| 9 | default parallel | 79 | 722 | 0 | 1 | 0 | — | quiet |
| 10 | default parallel (final, after the status updates) | 79 | 722 | 0 | 1 | 0 | — | quiet |

Runs 2 and 3 failed on exactly one test, `g1_real_process_matrix`
(`real_process_matrix.rs:125`), and are **not** product evidence — see
TEST-H in §13. Run V failed on exactly one different test,
`agentd::tests::ninth_concurrent_client_is_closed_not_fatal` — see TEST-H2
in §13; it was found *after* the first verification pass and is reported
here rather than hidden. Runs 1, 4, 5, 6, 7, 8, 9 and 10 are clean: **79
suites, 722 passed, 0 failed, 1 ignored, exit 0** — eight clean full runs,
including one fully serialized, and the last one taken after the version
and queue updates below.

Honest tally: **11 full workspace runs, 8 clean, 2 failed only from this
auditor's own process contamination (TEST-H), 1 failed only on the TEST-H2
IPC capacity flake.** No run ever failed on a workspace, identity,
recovery, ownership, admission, journal or provider test.

The single ignored test is `real_claude_smoke_replies_with_marker`
(gated on `RUNLITH_CLAUDE_SMOKE=1`); it was run explicitly and **passed**
against real Claude (§9).

**Repeated targeted suites**, two complete cycles of 3 repeats each over
15 suites (90 suite runs):

| Cycle | Suite runs | Passed | Failed |
| --- | --- | --- | --- |
| 1 | 45 | 513 | 2 suite runs failed (`agentctl/execution_recovery`; §13 TEST-H) |
| 2 | 45 | 519 | 0 |

Suites covered each cycle: `agentd/live_ownership`,
`agentd/execution_recovery`, `agentctl/execution_recovery`,
`agentctl/execution_composition`, `agentctl/execution_binaries`,
`app-services/{branch_only_recovery, legacy_workspace_identity,
casefold_identity, entry_recovery, workspace_service, failure_matrix,
provider_binding}`, `storage-sqlite/{durability, workspace, journal}`.

Additional race repetition: `agentctl/execution_recovery` 20× isolated +
12× under load average 9.4 (all pass); `app-services/concurrent_cycles`
10× (20 test runs, all pass); `agentd/live_ownership` 5 runs (its own
`shared_resume_race_repeated_fifty_rounds` adds 50 internal rounds each).

---

## 12. Cross-remediation tests

The most valuable failures would live *between* remediations, so the
combinations were exercised explicitly.

**Legacy (TASK-0058) + live ownership (TASK-0056), through the shipped
binaries.** Genuine schema-6 strands `e2-strand`, `h2-strand`, `E-Strand`
in a current daemon's data directory, each given **6 concurrent
`agentctl execution resume`**:

| Strand | Accepted | Losers | Attempts | `attempt.created` | `workspace.created` | Provider launches |
| --- | --- | --- | --- | --- | --- | --- |
| `e2-strand` | 1 | 4 × `CONFLICT` live-run, 1 × `CONFLICT` ambiguous-partial-Git | 1 | 1 | 1 | 1 |
| `h2-strand` | 1 | 2 × `CONFLICT` live-run, 1 × `CONFLICT` ambiguous-partial-Git, 2 × `INVALID_TRANSITION` already-attempted | 1 | 1 | 1 | 1 |
| `E-Strand` | 1 | 3 × `CONFLICT` live-run, 1 × `CONFLICT` ambiguous-partial-Git, 1 × `INVALID_TRANSITION` | 1 | 1 | 1 | 1 |

Persisted legacy identity unchanged byte-for-byte in every case (including
uppercase `refs/heads/agent/E-Strand`); all three Tasks reached
`VERIFYING`; the primary checkout stayed clean on `main`. Legacy strands
share the *same* live-owner registry as current-v2 executions.

**Current-v2 + B-RS2 + B-CF combined.** Real branch-only crash state,
restart, packed refs, resume, recovery, provider run, status — covered in
§6.1 (shape D) and §6.2.

**Case variants + case-insensitive APFS + packed refs + crash + restart +
recovery + commits.** One run combined all of them: `E-COMB` and `E-comb`
admitted concurrently on one repository, real `agentd` `kill -9` at 300 ms,
`git pack-refs --all` (the surviving agent ref moved into `packed-refs`
with no loose remnant), a new daemon, `resume` for both:

```
E-COMB -> refs/heads/agent/e2-%45-%43%4f%4d%42  tip 8ba2c41…  worktree HEAD 8ba2c41…
E-comb -> refs/heads/agent/e2-%45-comb          tip ce5f416…  worktree HEAD ce5f416…
```

Both reached `VERIFYING`; both leases `READY`; post-recovery commits gave
independent heads; `workspace.created`, `attempt.created` and
`attempt.concluded` each exactly once per execution; primary checkout on
`main`.

---

## 13. Findings

No finding below is a G5 blocker. Two are new; the rest are
re-classifications or re-confirmations.

### F-SCAN (pre-existing, **HARDENING**) — startup resume hint omits required data

`log_pre_attempt_strands` (`bins/agentd/src/execution.rs:1033`) prints

```
resume with: agentctl execution resume <E> <repository-path>
```

for **every** strand, including the no-lease shape, where `--ref` is
mandatory. Following the hint verbatim yields
`INVALID_ARGUMENT: repository ref is required when no workspace lease
exists yet`. The `cancel` path inspects the lease and prints the complete
invocation (`… --ref <repository-ref> --rev <base-revision>`). Reproduced
5× (4 crash-probe rounds plus the real-Claude scenario B).

The function's own doc comment says "plus the **exact** resume invocation"
and `workspace.md:200` says "surfaces stranded pre-attempt entries with
their resume invocation" — so this is also a **DOCUMENTATION_DEFECT**.

Non-blocking: the refusal is typed, names precisely what is missing,
mutates nothing, and recovery succeeds with `--ref` (verified end-to-end
with real Claude). TASK-0055 already recorded this as F-SCAN; it was out of
scope for TASK-0056/0057/0058 and remains open.

### N-1 (NEW, **HARDENING**) — managed-root creation is check-then-create

`data_dir::ensure_data_dir` (`bins/agentd/src/data_dir.rs:129`) does
`symlink_metadata` → on `NotFound` → `fs::create_dir`, and maps an
`AlreadyExists` error into `DaemonError::Io`. Two concurrent execution
requests on a daemon whose `<data>/ai-engineering-os/workspaces` directory
does not yet exist therefore race: the loser's `run` fails with

```
INTERNAL: execution workspace root unavailable: daemon I/O failure: File exists (os error 17)
```

Reproduced in **39 of 60** rounds of concurrent `run` + `resume` with a
fresh data directory; **0 of 400** once the directory exists.

Severity: low. `ensure_managed_root` is a fail-fast gate *before*
`enter_execution`, so nothing is committed — no orphan execution, no
reservation, no Task transition, no Git work — and the request is fully
retryable. The window closes permanently after the directory exists, so it
is a first-concurrent-use symptom on a fresh data directory.

Narrow remediation direction: treat `io::ErrorKind::AlreadyExists` from
`create_dir` as "fall through to `validate_data_dir(dir)`". That keeps
every ownership, mode and symlink gate intact — the directory that now
exists is still fully validated — and weakens nothing.

### N-2 (NEW, **HARDENING**, defense-in-depth) — asymmetric duplicate-row handling

`WorkspaceStore::find_lease_owner_by_worktree_ref` documents and enforces
"duplicate claims are corruption and fail closed rather than picking one
row". Its sibling `find_lease_owner_in_repository`
(`adapters/storage-sqlite/src/workspace_store.rs:152`) uses
`query_row`, which silently takes the first row. So
`prove_branch_only_recovery`'s "cannot prove exclusive branch ownership"
guard would not detect a second lease row claiming the same
`(repository_ref, branch_ref)`.

**Proven unreachable through the product.** `insert_allocated_lease` claims
the managed-identity reservation (`branch_ref` PRIMARY KEY, globally
unique) and then refuses any pre-existing foreign lease on that branch with
`BranchCollision` — demonstrated on the exact worst case, a pre-v2 lease
for `e2-foo` versus a new v2 execution `foo` that derives the identical
branch (§5.6). Migration 0007 deliberately adds no UNIQUE to
`workspace_leases` so historical H4 artifacts survive, but those rows
classify as *Legacy*, and `prove_branch_only_recovery` returns early for
Legacy leases before reaching this guard.

Narrow remediation direction: make `find_lease_owner_in_repository` fail
closed on more than one row, matching its sibling.

### TEST-H (re-confirmed, **TEST_HARDENING**) — machine-global test assertions

`g1_real_process_matrix` counts `pgrep -f <absolute agentd binary path>`
across the whole machine and asserts `== 1`, and targets the user's
**default** endpoint and data directory rather than a sandbox.
Demonstrated deterministically: starting one unrelated `agentd` from the
same absolute path on a completely separate runtime/data directory makes it
fail immediately —

```
assertion `left == right` failed: exactly one daemon process
  left: 3
 right: 1
```

— while the product singleton is perfectly intact (each daemon owned its
own endpoint; a second daemon on the same endpoint still exits
`AlreadyRunning(2)`, verified). This is the sole cause of the failures in
full runs 2 and 3.

The TASK-0055 mechanism is unchanged: `agentctl ping` still passes
`spawn_sibling_daemon` (`bins/agentctl/src/main.rs:91`), and readiness
loops poll with `agentctl ping`. Newly observed: `bins/agentctl/tests/execution_recovery.rs::wait_ready`
uses the *same* pattern, which is the best explanation for two transient
failures of that suite in repeat cycle 1 (fast failures at 1.34 s / 1.81 s,
consistent with "no strand formed because an auto-spawned sibling daemon
survived the test's `kill`"). That suite subsequently passed 39/39 —
20× isolated, 12× under load average 9.4, 3× in cycle 2 and in every clean
full run — and the independent real-SIGKILL campaigns (§6) found no
product race in the same code path. Residue also persists: **719**
`agentd-dispatch-test-*` directories in `TMPDIR` (619 at TASK-0055).

Classification kept as test-only, but now on reproduced evidence rather
than inheritance.

### TEST-H2 (NEW, **TEST_HARDENING**) — IPC capacity test asserts an OS-timing mechanism

`agentd::tests::ninth_concurrent_client_is_closed_not_fatal`
(`bins/agentd/src/lib.rs:952`) opens exactly `MAX_CONTROL_CLIENTS` admin
sessions to saturate the daemon, then asserts the next client's handshake
fails. Under parallel load the panic occurs earlier, at
`admin_session`'s `transport_connect(...).expect("connect")`
(`lib.rs:780`): one of the *saturating* connects is refused by the kernel
because the daemon's accept loop is momentarily behind and the socket
backlog fills, rather than being accepted and then closed by the daemon.

Measured rate: **2 of 10** runs of the full `-p agentd --lib` suite (38
tests in parallel in one process); **0 of 8** when the test runs alone;
**1 of 10** full workspace runs.

Test-only. The product behaviour is correct under either mechanism — a
client at capacity is refused, and `agentctl` surfaces that as a typed
`client transport failure` (that exact shape was observed independently in
this audit). No durable state, no Task, Execution, Attempt, workspace or
journal fact is involved; this is a G1/G2 IPC capacity assertion, not a G5
contract. The test should assert "the extra client does not become ready"
without assuming that every saturating connect is accepted first.

### Transient `commondir` error (re-classified: **HARDENING**, not test-only)

TASK-0058 reported one transient Git `commondir` read failure in
`concurrent_writer_pairs_serialize_without_busy_failures`. Not reproduced
here (10 runs / 20 test executions, all pass). The product code contains no
`commondir` reference; the mechanism is Git's own race between concurrent
`git worktree add` and reads of `.git/worktrees` administrative files in
one repository. Because the product genuinely performs concurrent worktree
creation in one repository, this should **not** be filed as test-only: it
is a real (rare) Git-level concurrency characteristic. It is nevertheless
non-blocking — `git-cli` surfaces it as a typed backend failure, INV-145
already treats Git as a separate failure domain, and the resulting
`Allocated` + partial-Git shape is exactly what the reconciliation matrix
in §6.3 proves safe.

### H7 (**ACCEPTABLE_DEFERRED — G6**, with a stronger measured consequence)

Reproduced with real Claude 2.1.268: a run leaves `?? data/sessions.db`
inside the managed worktree. Newly measured consequences for current G5
behaviour:

- diff evidence becomes `status = DirtyUntracked` with a working entry
  `data/ Untracked`;
- ordinary cleanup then **refuses**:
  `worktree_dirty: lease … holds uncommitted work; ordinary cleanup refuses removal`.

So in practice a managed workspace used by real Claude cannot be torn down
by ordinary cleanup, and the residue accumulates.

This is **not** a violated invariant and **not** a G5 blocker:

- the refusal *is* the accepted hard invariant — `workspace.md`:
  "a dirty application-owned worktree cannot be silently [destroyed];
  `git reset --hard`, `git clean -fd`, forced worktree removal and
  `branch -D` are never invisible normal [operations]";
- the docs do **not** overclaim isolation — `workspace.md` line 270 states
  plainly *"Git isolation != OS sandbox (G6 owns sandboxes; INV-076)"*;
- confining provider-written state is provider-environment isolation,
  explicitly G6.

Flagged prominently because the operational cost is larger than "one stray
file", and because it should be closed before any delivery gate.

---

## 14. Deferred-finding reclassification

Re-judged from first principles, not copied from TASK-0055.

| ID | Classification | Basis |
| --- | --- | --- |
| HARD-1 (client deadline vs SQLite writer completion) | **HARDENING** | Ambiguity is a client-side reporting gap; the durable entry is atomic and `status` resolves it. No invariant touched. |
| HARD-2 (internal unreserved first-Attempt API misuse) | **HARDENING** | Not reachable through IPC; both `run` and `resume` use the planned reserved id. Library-surface ergonomics. |
| F-BLK (`task block` after accepted run/resume still launches provider) | **HARDENING** | A race between an operator block and an already-accepted execution. Task stays `BLOCKED`; the attempt is honest evidence; no completion authority is granted. `task-engine.md` mandates no attempt-time re-check. |
| F-MI (manual recovery inputs may name another repository/revision) | **HARDENING** | Lease-backed resume enforces exact `repository_ref` and base-OID agreement (observed twice in this audit as typed `INVALID_ARGUMENT`). Only the no-lease shape accepts operator-supplied inputs — an explicit ADR-0025 §6 design choice. Docs should say so. |
| F-RA (accept-then-background-fail) | **HARDENING** | Confirmed reachable under contention (§7.1). Durable state stays exactly-once and the strand remains recoverable; only the response is optimistic. |
| F-SCAN (boot/cancel hints, corrupt shapes) | **HARDENING** + **DOCUMENTATION_DEFECT** | Reproduced (§13). Recovery remains possible through the product. |
| F-IPC (new resume against an older v2 daemon) | **ACCEPTABLE_DEFERRED** | Additive field on an unchanged protocol version; an old daemon answers `INTERNAL: empty execution request`. Typed, no mutation, dev-era only. |
| H5 (provider version preflight) | **ACCEPTABLE_DEFERRED** | Precheck is PATH/exists only by deliberate design (the 10 s `detect` probe cannot fit the 5 s IPC bound, documented in code). Version problems surface as typed attempt outcomes. |
| H7 (provider environment files inside the worktree) | **ACCEPTABLE_DEFERRED (G6)** | §13. Docs scope isolation correctly; consequence recorded. |
| H9 (large implementation files) | **HARDENING** | `bins/agentd/src/execution.rs` is now **2195** LOC (1979 at TASK-0055) and `crates/app-services/src/workspace_service.rs` **1816** (1544). Both exceed the 800-line convention with no recorded reviewed exception (INV-081). Growth continued through TASK-0056/0057. Maintainability, not correctness. |
| TEST-H (process/test hermeticity) | **TEST_HARDENING** | §13, reproduced deterministically. |
| TEST-H2 (IPC capacity test connect assumption) | **TEST_HARDENING** | New, §13. 2/10 lib-suite runs, 1/10 full runs; product behaviour correct either way. |
| `commondir` transient | **HARDENING** | §13, re-classified away from test-only. |
| Post-Attempt crash recovery | **ACCEPTABLE_DEFERRED (G11)** | §15. |
| Retry / re-execution ergonomics | **ACCEPTABLE_DEFERRED** | No current invariant requires automated retry; `RETRY_READY` exists in the schema and the lifecycle owns it at a later gate. |
| B-CF, B-CR, B-RS, B-RS2, B-OL, HARD-3 | **RESOLVED** | §4–§7. |

---

## 15. Post-Attempt crash boundary, re-evaluated

Re-derived rather than inherited. When an Attempt exists and is open and
the daemon dies, current `resume` refuses `INVALID_TRANSITION` ("already
records N attempt(s); pre-attempt recovery does not apply"). Is that
sufficient for G5 today?

Yes. The current invariant set demands that state be **durable and
honest** (INV-001, INV-010) and that terminal facts be evidence-backed
(ADR-0018, INV-068, INV-093). After such a crash the durable record says
exactly what happened: an open Attempt with no conclusion. Concluding it
without provider evidence would *fabricate* a terminal fact, and the
orphaned provider process may still be alive — that is precisely the
supervision problem G11 owns ("Background daemon: authorized work continues
with UI closed"). `status` reports the open attempt, `cancel` reports
honestly, and nothing is corrupted or lost. Honest durable state plus
future supervision is architecturally sufficient at G5; automated
post-Attempt recovery is correctly deferred.

---

## 16. Verdict

**`G5_CLOSED`.**

Against the closure rule, each condition independently supported:

| # | Condition | Evidence |
| --- | --- | --- |
| 1 | All historical G5 blockers remain resolved | §3 |
| 2 | B-CF resolved | §4 |
| 3 | B-CR resolved | §7 |
| 4 | B-RS / B-RS2 resolved for G5 scope | §6 |
| 5 | B-OL resolved | §5 |
| 6 | No new G5 blocker discovered | §13 (2 new HARDENING findings, 1 new TEST_HARDENING finding, 0 blockers) |
| 7 | Admission / reservation atomicity sound | §8 |
| 8 | Recovery survives genuine daemon death and restart | §6, §9 |
| 9 | Branch-only recovery ownership-safe | §6.2, §6.3 |
| 10 | Live ownership and cancellation sound | §7 |
| 11 | Legacy identity compatibility safe | §5.3, §5.4, §12 |
| 12 | Corrupt v2 cannot fall through to legacy | §5.5 |
| 13 | Real Git loose and packed refs safe | §4, §5, §6, §12 |
| 14 | Old WorkspaceLease authority holds | §5 |
| 15 | Task / Execution / Attempt / ProviderSession boundaries hold | §8, §9, §10 |
| 16 | Provider cannot self-complete a Task | §9 |
| 17 | FakeAgent remains valid | §10 |
| 18 | Real Claude works through the production composition | §9 |
| 19 | Second provider structurally addable | §10 |
| 20 | Journal / state guarantees hold | §8 |
| 21 | Deferred findings genuinely non-blocking | §14 |
| 22 | Full workspace verification acceptably clean | §11 (8 clean runs of 11: 722/722, incl. one serialized; the 3 non-clean runs failed only on TEST-H / TEST-H2 test-hermeticity assertions, never on a product contract) |
| 23 | Docs do not materially overclaim | §17 |
| 24 | Audit evidence reproducible | §1, §18 |

**G5 is CLOSED. G6 Security is the next gate. No G6 implementation was
started by TASK-0059.**

---

## 17. Documentation consistency

Checked against implementation: ADR-0023 (+ TASK-0058 amendment),
ADR-0024 (+ TASK-0053/0054 amendments), ADR-0025 (+ TASK-0056/0057
amendments), `workspace.md`, `ipc.md`, the architecture source of truth,
and the TASK-0053/0054/0056/0057/0058 remediation records.

Specifically hunted for the overclaim patterns named in the task:

| Claim to test | Finding |
| --- | --- |
| "e2/h2 namespaces are historically disjoint" | **Corrected already.** `workspace.md` now states outputs "may lexically overlap the `e2-`/`h2-` prefixes because old ExecutionIds could begin with those strings" and describes complete-pair classification. Matches the code and §5. |
| "all partial Git states are recoverable" | Not claimed. `workspace.md` scopes TASK-0057 to "one narrow `Allocated` recovery class" and states ambiguity "remains fail-closed". Matches §6.3. |
| "all concurrent resume losers fail safely" | ADR-0025 §4's original wording was upgraded by the TASK-0056 amendment to name the mechanism (`CancelRegistry::try_claim`, owner-scoped release). Matches §7. |
| "exact resume hints" | **Defect (F-SCAN).** The startup scan's hint omits `--ref`/`--rev` for no-lease strands while code comment and `workspace.md:200` call it exact. §13. |
| "provider environment is fully isolated" | Not claimed. `workspace.md:270`: "Git isolation != OS sandbox (G6 owns sandboxes; INV-076)". Accurate. |
| digest injectivity | Accurately stated as collision-resistant, explicitly *not* injective, with the durable backstop. §4. |

One minor imprecision: `workspace.md` says current-v2 classification is
"exact current-v2 derivation **plus** the owner's durable identity
reservation". In code the pair decides the format
(`classify_persisted_identity`) and the reservation is enforced by the
caller (`classify_lease_identity`), which refuses a v2 lease without one.
Net behaviour matches the documentation; the split is an implementation
detail. Also, when a reservation exists but belongs to another execution,
the emitted message reads "has no durable managed-identity reservation"
(the lookup is by owner) — correct refusal, slightly imprecise wording.

No documentation defect masks a real blocker.

---

## 18. Process, runtime and repository cleanliness

**Before testing:** zero product `agentd`/`agentctl` processes. (Three
matches for a naive `agentd` substring were macOS system daemons —
`gamecontrolleragentd`, `talagentd` — and are unrelated; they are named
here because a name-based count would wrongly include them.)

**After all testing:** zero `agentd`, zero `agentctl`, zero product-spawned
`claude`, no product test residue in `/tmp` (`g5-recovery-*`,
`agentd-liveown-*`, `t0059-*` all absent), no leftover managed worktrees,
refs or reservations from audit sandboxes.

**Environmental incidents (this auditor's harness, reported separately):**

1. One audit `agentd` outlived its probe for ~35 minutes because a harness
   script's `wait` blocked on the daemon; found and killed.
2. Audit-owned daemons running concurrently with full runs 2 and 3 tripped
   the machine-global assertion in `g1_real_process_matrix` (§13). Runs 4
   and 5 were repeated on a quiet machine and are the reported evidence.
3. Audit sandboxes under `/private/tmp/g5{a59,adm,comb,dbg,hist,histrc,rc,th}`
   were removed at the end. Pre-existing sandboxes from earlier tasks
   (`g5-smoke-0051`, `g5reaudit-smoke2`, `g5smoke`, `g5task54smoke`) were
   left untouched as unrelated user work.
4. `TMPDIR` holds **719** `agentd-dispatch-test-*` directories from the
   product test suite (619 at TASK-0055). Test residue, recorded under
   TEST-H; not removed by this audit.
5. The user's own Claude Code session was running throughout and was never
   touched.

**Repository:** the tracked-diff fingerprint
`8cd3f1d18b3220177e0ee504e58ef557fa041407dc8df2a9be8e5c4a20e69c21` and the
20-entry untracked set are identical before and after the audit. Nothing
was staged. The audited product diff was not modified. The only repository
changes authorized by this task are this document plus the
`product/version.toml` and `planning/tasks/queue.yaml` status updates that
the `G5_CLOSED` verdict permits.

---

## 19. Audit surface

Probes lived outside the repository:

- `g5-audit-probe` — external crate path-depending on the workspace crates:
  `hist_legacy.rs` (genuine schema-6 fixtures), `casefold_hostile.rs`,
  `identity_matrix.rs`, `branch_only_matrix.rs`, `live_ownership_audit.rs`,
  `resume_toctou.rs`, `h7_effect.rs`, plus `seed`/worker binaries for
  cross-process admission.
- `crashprobe.sh` — 34 randomized real-`kill -9` rounds through the shipped
  binaries on a 1600-file repository.
- `legacy_concurrent.sh` — genuine historical strands × 6 concurrent
  resumes through the shipped binaries.
- `realclaude.sh`, scenario C script — real Claude through the full
  production path.
- `combined.sh` — case variants + packed refs + crash + restart + recovery
  + commits.
- `admission.sh` — cross-process admission attacks.
- A separate clone at `b3b956d` plus a `histgen` producer built from that
  revision's own code.

Roughly 700 concurrency rounds, 34 real daemon kills, 5 full workspace
runs, 90 repeated targeted suite runs and 4 real-Claude executions.

---

## 20. Status after this audit

- TASK-0059: complete.
- Development version: `0.1.0-dev.task0059` (SemVer `0.1.0`).
- Gate: **G5 CLOSED**.
- Next: **G6 Security** (policy engine, approvals, Secret Broker, audit,
  runner security levels, OS isolation, provider-environment isolation —
  which subsumes H7). Not started here.
- Recommended non-blocking follow-ups, in priority order: H7 (G6 scope, but
  it makes every real-provider workspace undeletable today), F-SCAN, N-1,
  TEST-H, TEST-H2, N-2, H9.
