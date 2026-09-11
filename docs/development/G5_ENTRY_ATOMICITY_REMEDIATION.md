# G5 Entry-Atomicity Remediation (TASK-0051)

> Remediation evidence for the two TASK-0050 blockers (B1-concurrent,
> H4-promoted) against `docs/development/G5_CLOSURE_REAUDIT.md`
> (verdict `G5_REMEDIATION_REQUIRED`). `G5_CLOSURE_REAUDIT.md` stays the
> historical record of failure; this document records what changed and
> the evidence that each blocker no longer exists. Authority: `AGENT.md`,
> `INVARIANTS.md`, ADR-0005/0009/0018/0021/0022/0023/0024.
>
> Verdict: **`G5_ENTRY_REMEDIATION_READY_FOR_REAUDIT`**. G5 stays
> `in_progress`; TASK-0052 (re-re-audit) is next. G5 is NOT closed here.

## 1. B1-concurrent — root cause

`OrchestrationService::enter` (TASK-0049 shape) ran:

```text
preflight read (execution absent? attempt absent?)
  → TaskService::claim      (txn 1: READY → CLAIMED + journal)
  → TaskService::start      (txn 2: CLAIMED → RUNNING + journal)
  → RuntimeService::create_execution (txn 3: execution + journal)
```

on separate connections/transactions. Two barrier-synchronized entrants
with the same new execution identity both observed absence, then both
committed claim/start; the loser died on the execution `UNIQUE`
constraint with `RUNNING` + `Claimed`/`Started` facts and no Execution —
non-rerunnable. The constraint detected the conflict too late to
preserve honest state. The attempt-ID variant was worse: both entries
succeeded outright (the attempt does not exist at entry time) and the
collision detonated later at background `create_attempt`, stranding
`RUNNING` + Execution + lease/worktree with no attempt.

## 2. B1-concurrent — new atomic boundary

Entry admission is now ONE `IMMEDIATE` transaction
(`SqliteStore::admit_execution_entry`, `adapters/storage-sqlite/src/entry_store.rs`):

```text
BEGIN IMMEDIATE
  UPDATE tasks → CLAIMED   WHERE id AND state = READY      (0 rows → Stale/NotFound)
  UPDATE tasks → RUNNING   WHERE id AND state = CLAIMED     (guard chain)
  execution id absent?     (else typed Duplicate, naming the owning task)
  attempt id uncreated?    (else typed Duplicate, naming the owning execution)
  attempt id unreserved?   (else typed Duplicate, naming the reserving execution)
  INSERT executions
  INSERT attempt_reservations
  INSERT journal task.claimed + task.started + execution.created
COMMIT
```

Design points (per the task contract):

- **Narrow seam, no UnitOfWork.** One concrete operation
  (`ExecutionEntryAdmission` → `AdmittedExecutionEntry`) on
  `SqliteStore`; no generic transaction manager, no SQL above the
  adapter. Dependency direction unchanged (`app-services →`
  `storage-sqlite`, concrete store as before).
- **Domain validation stays authoritative.** Services plan first
  (`TaskService::plan_claim`/`plan_start` via `task-domain`
  `Task::apply`; `RuntimeService::plan_execution` via
  `Execution::begin`), minting journal evidence; the store re-checks
  coherence plus expected-state guards *inside* the transaction, so a
  stale validation fails instead of overwriting. No second Task state
  machine in SQL.
- **Attempt reservation without ghost semantics.** The reservation is
  an intent row (`attempt_reservations`, migration 0006: PK
  `attempt_id`, owner execution/task, timestamp) — never an Attempt
  fact, no state-machine expansion. `create_attempt` consumes the
  reservation for the owning execution in its own transaction and
  refuses an identity reserved by (or created under) another execution
  with typed `Duplicate`. Reservation-free creation (legacy tests,
  direct service use) keeps historical behavior behind the global
  attempt PRIMARY KEY.
- **Journal atomicity.** Accepted entry commits state + all three
  facts together; rejected entry commits neither state nor events (no
  compensating events).
- **Short transaction.** Only SQLite row work; no Git, filesystem,
  provider, or network inside. `IMMEDIATE` + 30 s busy discipline
  preserved; single-writer serialization, no lock-upgrade deadlock.
- **Crash semantics.** Crash before `COMMIT` → zero admitted
  lifecycle facts. Crash after → complete entry (claim + start +
  execution + reservation + journal). Git/provider effects happen
  later as recoverable sagas (INV-145 untouched).
- **Same-request replay.** No idempotent-replay path (unchanged
  contract): same task + same execution + same attempt re-entry is a
  refusal (typed conflict or not-runnable), pinned by the
  exact-duplicate keeper.

## 3. B1-concurrent — keeper evidence

New `crates/app-services/tests/entry_atomicity.rs` (5 tests; every
race iteration asserts durable loser state — task, execution, attempt,
reservation, lease, journal — plus loser rerunnability):

| Keeper | Iterations | Loser contract |
|---|---|---|
| `concurrent_duplicate_execution_id_leaves_loser_honest` | 50 | exactly one admitted; loser `Duplicate`, READY, clean journal, retry with fresh ids → `Runnable` |
| `concurrent_duplicate_attempt_id_leaves_loser_honest` | 50 | exactly one owns the reservation; loser `Duplicate`, READY, no execution/lease, retry works |
| `concurrent_same_task_entry_admits_single_winner` | 50 | one winner; loser `Stale`/domain-refusal/`NotRunnable`; exactly one claim/start pair, one execution |
| `concurrent_exact_duplicate_entry_is_honest` | 25 | one winner; no facts beyond the single triple; same-identity retry stays refused |
| `sequential_identity_conflicts_refuse_before_mutation` | — | TASK-0048 shapes (existing + reserved attempt ids) still refuse typed before mutation |

RED → GREEN: a temporary barrier probe using only pre-remediation
APIs failed on the stashed TASK-0050 implementation (loser left
`RUNNING`, the exact audit shape) and passed with admission; the probe
was removed afterwards, the permanent keepers above subsume it.

## 4. H4 — root cause

`slug_body` passed `-` through raw while encoding other bytes as
`-XX`: `E/S` → `E-2FS`, and `E-2FS` → `E-2FS`. Both are valid
production IPC inputs, so the second task committed claim/start/
execution and then wedged `RUNNING` on `BranchCollision` —
non-rerunnable, a synchronously detectable deterministic self-collision.

## 5. H4 — injective managed identity

New `slug_body` (`crates/app-services/src/workspace_service.rs`):
`%`-escaping over the exact identity bytes — alphanumerics/`-`/`_`
pass through, `%` → `%%`, every other byte → `%XX` uppercase hex —
with overlong identities (escaped > 64) in the digest form
`h<sha256-lowerhex>-` (66 chars, longer than any readable form, so the
ranges are disjoint by length; injective modulo SHA-256 via the small
pure-Rust `sha2` dependency, resolved offline from the local cargo
cache — `Cargo.lock` delta is purely additive).

Properties: valid Git ref syntax (verified via `git check-ref-format`
plus real branch creation in keepers), deterministic, stable,
bounded (≤ 66-char slug), no path/ref injection (no `/`, never
`.`/`..`), stays under `refs/heads/agent/` (`is_managed` unchanged),
no protected-ref escape. Unicode: no normalization — derivation hashes
the exact bytes, following `ExecutionId` exact-equality (§31).

Compatibility: lease rows persist branch + path facts and are never
re-derived, so old-format leases (`agent/E-2FS` etc.) stay observable,
managed-scoped, and reconcilable — proven by the new v5→v6 migration
test with an old-format `Allocated` lease. No data migration of
existing branches.

Entry-time workspace preflight (§32): entry derives the managed branch
and refuses (typed `Duplicate`) before its first mutation when the
branch is already recorded for another execution. Git state itself can
still change afterwards — that stays a recoverable saga concern.

Allocated-lease disposition (§34): unchanged G4 semantics — an
`Allocated` lease after a failed materialization is intentional
recoverable intent (crash-A/B/C reconciliation), not poison; entry
refusal (the H4 path) now happens before any lease exists, and
post-entry workspace failures keep the documented recoverable
intermediate (ADR-0024 §6).

## 6. H4 — keeper evidence

- `slugs_are_single_components_without_traversal` (updated: digest
  form is 66 chars, not < 32 — disclosed test change for the
  authorized derivation redesign).
- `known_h4_collision_pair_is_resolved`: `E/S` → `agent/E%2FS` vs
  `E-2FS` → `agent/E-2FS`.
- `escape_mimic_families_stay_injective`: 12 adversarial `%`/`-`/space/dot pairs.
- `branch_derivation_is_injective_over_corpus`: ~40-input corpus
  (ASCII, digits, hyphens, underscores, slashes, dots, Unicode,
  escape-like sequences, control bytes, long/prefix-related IDs, HEAD/main/refs)
  with uniqueness + scope + bound assertions, plus long-ID digest distinction.
- `unicode_branches_are_byte_exact`: no normalization (documents §31).
- `h4_collision_pair_allocates_independent_workspaces` (service
  level, real Git): both entries allocate `CreatedFresh` with distinct
  branches and materialized worktrees in one repository.

## 7. Amendments and doc fixes

- ADR-0024 §6: replaced the "store-level uniqueness backstop"
  sentence with the atomic-admission contract (diagnostic preflight,
  single-transaction admission, reservation consume, crash semantics,
  INV-145 boundary). No other ADR section touched.
- `docs/architecture/workspace.md` (Branch Semantics):
  injective-derivation paragraph.
- `docs/architecture/ipc.md` (§16 `run`): atomic-entry sentence.
  (`task-engine.md`/`providers.md` carry no entry-ordering prose;
  there is no `storage.md` — persistence entry contract lives in
  ADR-0024 + `ipc.md`.)
- `docs/development/G5_CLAUDE_PROVIDER.md`: corrected the stale
  "never spawned blindly" line to the actual semantics
  (adapter-owned compatibility policy; production fast precheck is
  PATH-presence-only; parser fails closed; bounded-fast gate still
  deferred). H5 stays `ACCEPTABLE_DEFERRED`.

## 8. What was NOT touched (no regressions by construction)

- Provider process supervision (B2), shared contract suite (B3),
  completion authority, `ExecutionPort` shape: untouched; targeted
  suites re-run green (§10).
- Schema history: migrations 0001–0005 untouched; 0006 adds one table
  + index (justified data-model change). No journal rebuild.
- `TaskService::claim/start` remain for explicit operator lifecycle
  ops (`agentd` task dispatch); execution entry no longer uses them.
- `RuntimeService::create_execution` remains for direct/test use;
  production execution creation flows only through admission
  (source-audited: no other production caller).
- Deferred findings stay deferred: H5 (version gate), H7
  (`sessions.db`/worktree hygiene), H9 (fitness signal), same-binary
  test hermeticity (the `g1_real_process_matrix` scoping stays a
  HARDENING_FOLLOWUP — no production change), no re-execution path,
  daemon-restart orphan recovery → G11.

## 9. Commands and census

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --no-fail-fast (×3 clean runs)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo tree direction audit (app-services/storage-sqlite/provider isolation)
rg sweeps: provider == "claude" in core (none), BEGIN DEFERRED/transaction() (none),
  production create_execution callers (admission only)
keeper stress: exec-id race 50 + attempt-id race 50 + same-task 50 + exact-dup 25
  per full run, plus entry_atomicity repetitions
RED probe (temporary, removed): FAIL on TASK-0050 code (loser RUNNING) → PASS after
```

One transient to record honestly: during verification,
`agentctl task_core_matrix::restart_durability_e2e` failed twice with
`daemon startup timed out` (10 s budget) and then passed 3/3 isolated
plus all full runs with zero stale daemons and zero code changes in
between — environmental contention during the initial build+test
storm, not a product regression (base-vs-head isolation runs and the
clean-run census below are the authority).

Real Claude smoke (optional, attempted): Claude Code 2.1.268 via
`agentctl → IPC → agentd → atomic entry → WorkspaceLease → Claude`
with a tool-free one-line prompt. Result: Task `VERIFYING`, attempt
`A-SMOKE SUCCEEDED` with opaque `claude:session:…` evidence ref,
managed branch `agent/E-SMOKE` + materialized worktree under the
managed root, no provider self-completion, no supervised-process leak
(`live=false`), daemon shut down via the product flow with zero
residue. `data/sessions.db` again observed inside the managed worktree
(H7, still deferred — recorded, not fixed).

## 10. Remaining follow-ups

1. TASK-0052 G5 Closure Re-Re-Audit (next; G5 stays `in_progress`).
2. Same-binary test hermeticity hardening (optional, test-only).
3. H5 bounded-fast production version gate (dedicated follow-up).
4. H7 provider-artifact worktree hygiene (G6 isolation scope).
5. G6 design ADRs (Policy Engine, Secrets Broker, Security Audit)
   per the TASK-0048 forward check — required before G6 implementation.
