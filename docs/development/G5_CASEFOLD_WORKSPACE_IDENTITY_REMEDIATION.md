# G5 Case-Fold-Safe Managed Workspace Identity Remediation (TASK-0053)

> Remediation evidence for the TASK-0052 blocker B-CF (case-fold managed
> workspace identity) against `docs/development/G5_CLOSURE_REREAUDIT.md`
> (verdict `G5_REMEDIATION_REQUIRED`). `G5_CLOSURE_REREAUDIT.md` stays the
> historical record of failure; this document records what changed and the
> evidence that the blocker no longer exists. Authority: `AGENT.md`,
> `INVARIANTS.md`, ADR-0005/0008/0009/0018/0022/0023/0024.
>
> Verdict: **`G5_CASEFOLD_REMEDIATION_READY`**. G5 stays `in_progress`
> because B-RS (post-admission recovery) belongs to TASK-0054. G5 is NOT
> closed here.

## 1. TASK-0052 finding B-CF

On the host's case-insensitive APFS (`core.ignorecase=true`), two distinct
valid ExecutionIds differing only in ASCII case (`E-CASE` vs `E-case`,
`E-PACK` vs `E-pack`) derived exact-distinct refs that alias on disk:

- loose refs: the first entry committed, the second passed the
  exact-string branch preflight, committed claim/start/execution, then
  wedged `RUNNING` on Git `BranchCollision` — stranded lease
  (`ALLOCATED`), stranded attempt reservation, re-entry `NotRunnable`;
- packed refs: both workspaces materialized, then branch and worktree
  HEAD identities aliased bidirectionally — cross-execution commit
  visibility (isolation breach, INV-005).

## 2. Root cause

Three compounding defects, all in the managed-identity layer (no provider
involvement):

1. **Case-preserving derivation.** `slug_body` passed ASCII case through,
   so `E-CASE` → `agent/E-CASE` and `E-case` → `agent/E-case`: distinct as
   Rust strings, one file on a case-insensitive filesystem. Git logical
   ref identity is not filesystem identity.
2. **Exact-string preflight only.** `find_lease_owner_by_branch` compared
   `branch_ref = ?1` (binary), so the folded collision was invisible
   before admission — and TASK-0050 had already proved preflight-only
   checks cannot close TOCTOU races anyway.
3. **No durable identity backstop.** `workspace_leases` carries no branch
   uniqueness, and nothing reserved the branch at admission time, so the
   collision surfaced only in the post-admission Git saga — after the Task
   was already `RUNNING`.

Additionally, TASK-0052 established (DOC-1) that the docs overclaimed
mathematical injectivity for the SHA-256 digest form.

## 3. Old identity algorithm (frozen history)

TASK-0051 `slug_body`: ASCII alphanumerics plus `-`/`_` passed through
with case preserved; `%` → `%%`; every other byte → `%XX` uppercase hex;
escaped length > 64 → `h<sha256-lowerhex>-`. Readable outputs could hold
uppercase; digest outputs were described as injective.

## 4. New identity algorithm (v2)

`crates/app-services/src/managed_identity.rs` (new module; the derivation
boundary owns the format, no provider data travels here):

```text
readable:  e2-<escaped>          (reversible, injective over exact bytes)
digest:    h2-<sha256-lowerhex>  (deterministic, collision-resistant)
```

Escape rule over the exact UTF-8 bytes (no normalization): `[a-z0-9-_]`
pass through; every other byte becomes `%` + two **lowercase** hex digits
(`A` → `%41`, `/` → `%2f`, `%` → `%25`). Reference outputs:

```text
a      → e2-a                A      → e2-%41
E/S    → e2-%45%2f%53         E-2FS  → e2-%45-2%46%53
```

## 5. ASCII case-fold argument

The output alphabet is `[a-z0-9%-_]` plus the `e2-`/`h2-` prefixes — no
uppercase byte can ever be emitted. Hence for every generated identity:

```text
ascii_casefold(output) == output
```

so case-insensitive filesystem semantics cannot merge two outputs. The
property is asserted as a test invariant (`is_ascii_casefold_stable`),
not by example: the corpus keeper checks every derived branch for
folded-distinctness, and the `E-CASE`/`E-case`, `E-PACK`/`E-pack`,
`A`/`a`, `ABC`/`abc`, `Execution`/`execution` pairs are pinned keepers
(`casefold_identity.rs`).

## 6. Unicode / exact-byte strategy

No Unicode normalization is applied anywhere: the exact UTF-8 bytes are
escaped to canonical ASCII (Cyrillic, accented Latin, composed vs
decomposed forms, emoji all round-trip byte-exact through
`decode_readable_slug`). Non-ASCII bytes therefore never reach
filesystem/Git naming, so no second class of Unicode case-folding or
normalization behavior is introduced through APFS/NTFS naming.

## 7. Windows-safe basename strategy

Every generated slug starts with the letter-led `e2-`/`h2-` prefix and
every worktree basename adds the outer `ws-` prefix. Windows reserved
device names require an exact (case-insensitive) match against
`CON/PRN/AUX/NUL/COM1–9/LPT1–9`; a prefixed basename can never equal one
— demonstrated by `generated_basenames_never_hit_windows_reserved_names`
plus the `is_windows_reserved_basename` predicate. The alphabet further
excludes every Windows-invalid character and trailing space/dot, and
total lengths stay bounded (slug ≤ 67, ref ≤ 84, basename ≤ 70).

## 8. Short / readable vs long / digest semantics (terminology)

- Readable (`e2-…`): exact-byte-preserving, reversible
  (`decode_readable_slug` inverts the escape), hence **injective** over
  the supported input domain. Readable and digest ranges are disjoint by
  prefix, so a literal digest-shaped identity is itself hashed and can
  never be mistaken for a digest.
- Digest (`h2-…`): deterministic, **collision-resistant**, bounded —
  explicitly NOT mathematically injective (pigeonhole). A real SHA-256
  collision is never fabricated for tests; instead the
  derivation → branch-string → reservation/check seam lets a stub digest
  force one shared managed identity (`forced_digest_collision…` unit
  proof) while production keeps real SHA-256.

DOC-1 corrections: `docs/architecture/workspace.md` Branch Semantics
rewritten in exactly these terms; the `branch_for_execution` docstring no
longer claims domain-wide injectivity; the orchestration preflight
comment corrected. Historical audits (`G5_CLOSURE_REREAUDIT.md`,
`G5_ENTRY_ATOMICITY_REMEDIATION.md`) are untouched evidence.

## 9. Durable collision backstop

Migration 0007 adds `managed_identity_reservations`:

```text
branch_ref    TEXT PRIMARY KEY   (global branch uniqueness — see §10)
execution_id  TEXT NOT NULL UNIQUE REFERENCES executions(id)
origin        TEXT NOT NULL CHECK (origin IN ('admission', 'lease'))
```

- The PRIMARY KEY is the concurrency guard: two concurrent claimants for
  one branch serialize on the constraint; the loser fails closed even
  after passing the read-before-write checks.
- Binary equality suffices (no `NOCASE`): rows are v2-canonical lowercase
  by construction, so binary coincides with the ASCII fold. Historical
  mixed-case leases (which never enter this table) are met with an
  explicit `COLLATE NOCASE` comparison — ASCII-only, matching the
  ASCII-only managed alphabet.
- Typed contract: admission conflicts surface as `StorageError::Duplicate`
  → IPC `ALREADY_EXISTS`; lease-layer conflicts as
  `WorkspaceFailure::BranchCollision`. Raw `UNIQUE constraint failed: …`
  text is never the contract (pinned by keeper assertion).
- Rejected conflicts journal nothing: the loser commits zero lifecycle
  facts (no `TaskClaimed`/`TaskStarted`/`ExecutionCreated`/
  `WorkspaceLeaseAllocated`); the reservation row belongs to the winner
  only.

## 10. Transaction / admission interaction

`SqliteStore::admit_execution_entry` now commits, in ONE `IMMEDIATE`
transaction: guarded READY → CLAIMED → RUNNING, Execution insert,
Attempt-identity reservation, **managed-identity reservation** (branch
derived purely before the transaction), and the three journal facts — or
rolls all of them back. No Git, filesystem, or provider work occurs
inside (INV-145 unchanged). The in-transaction checks cover the
reservation table (binary) and the lease table (explicit `NOCASE` fold);
the PRIMARY KEY is the final guard.

Repository scoping: the admission reservation is deliberately global
(conservative, fail-closed) — the same branch string collides in any
scope, so two textual repository paths for one physical repository
cannot bypass it. Precise per-repository enforcement additionally runs at
lease allocation (`find_lease_owner_in_repository`, same-repo `NOCASE`).
True physical-repository canonicalization (same repo under different
spellings admitted with *different* branches is unaffected — branches
are per-execution and distinct) stays with the entry-validation work of
TASK-0054; the residual case is documented there, not fixed here.

## 11. Migration

0007 is additive: one table plus one index. 0001–0006 untouched;
`SUPPORTED_SCHEMA_VERSION` 6 → 7. Fresh DB, v5 → v7 (rows preserved,
both reservation tables start empty), concurrent opens, and
interrupted/retry behavior follow the established harness; the v5
migration keeper additionally asserts the identity table migrates in
empty with the old mixed-case branch untouched.

## 12. Old lease compatibility

TASK-0058 correction: the original TASK-0053 remediation incorrectly
classified persisted leases from the `agent/e2-`/`agent/h2-` branch prefix
and claimed the v2 namespace was disjoint from every historical output.
That claim is false: historical derivations passed ASCII alphanumerics,
`-`, and `_` through, so old ExecutionIds such as `e2-legacy` and
`h2-legacy` produced branches with modern-looking prefixes.

Existing leases are still never renamed or converted. Current-v2 leases
are identified only when the complete persisted branch/worktree pair
equals the current v2 derivation and the durable managed-identity
reservation agrees with that owner/ref. Supported pre-v2 leases must match
one complete historical derivation and then use the persisted branch/path
byte-for-byte with containment checks. Mixed or unprovable pairs fail
closed; a corrupted current-v2 row cannot use compatibility fallback.
`old_format_allocated_lease_reconciles_from_persisted_values` remains the
ordinary legacy control; TASK-0058 adds `e2-`/`h2-` collision keepers and
genuine schema-6 cross-version evidence.

## 13. Loose refs RED → GREEN

Keeper `loose_ref_case_variants_materialize_independently` (`E-CASE` vs
`E-case`): RED on the old derivation (second `ensure` →
`BranchCollision` after entry committed); GREEN now — two independent
branches, two worktrees, independent commits reaching independent HEADs,
each worktree's log containing only its own commit.

## 14. Packed refs RED → GREEN

Keeper `packed_ref_case_variants_do_not_alias` (`E-PACK` vs `E-pack`,
`git pack-refs --all` between materializations): RED on the old
derivation (folded-equal refs); GREEN now — committing in either
worktree moves only its own branch, verified in both directions.

## 15. Concurrency

- `concurrent_case_distinct_admissions_both_succeed` (50 rounds): both
  case-distinct entries admit — the preferred outcome, since the new
  identities are genuinely independent. No false refusal, no wedge.
- `concurrent_forced_branch_collision_admits_single_winner` (50 rounds,
  forced shared branch at the store boundary): exactly one winner per
  round; every loser receives typed `Duplicate`, keeps `READY`, commits
  zero lifecycle facts, and stays rerunnable (sequential keeper proves
  the retry through the product).

## 16. Real Git evidence

All Git keepers run on this host's case-insensitive APFS
(`core.ignorecase=true`): real repositories, real `git-cli` adapter,
real `pack-refs`, real worktrees, real commits. `E/S` vs `E-2FS` remains
resolved under the new format (`e2-%45%2f%53` vs `e2-%45-2%46%53`,
additionally folded-distinct). Corpus `git check-ref-format` passes for
slash/backslash/`%`/`..`/`@{`/leading-dot/trailing-dot/`.lock`/
space/control/Unicode/long/case-variant inputs.

## 17. Regression results

Full workspace suite (`cargo fmt --check`, `cargo check --workspace`,
`cargo test --workspace --no-fail-fast`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`) with freshly built
binaries before binary-dependent suites:

```text
run 1: 665 passed / 0 failed / 1 ignored (73 suites)
run 2: 665 passed / 0 failed / 1 ignored (73 suites)
run 3: 665 passed / 0 failed / 1 ignored (73 suites)
```

(Ignored: the env-gated live provider smoke.) Baseline before this task
was 647 passed; the delta is the 10 `managed_identity` unit tests plus
the 8 `casefold_identity` keepers. Targeted repetitions: entry
atomicity, workspace service, migration/open, provider binding, and the
new keepers re-run green; the exact `E/S`/`E-2FS` keepers remain green
under the new expectations (disclosed string updates, same behavior).

Two environment notes recorded honestly, neither a product regression:
during verification a stale `target/debug/agentd` (predating the
rebuild) made one binary-dependent restart test time out, and three
orphaned daemons from manual smoke runs tripped the stale-daemon
sensitive process-matrix test (known TEST-H). Both resolved by rebuilding
bins and clearing session strays; the suites pass clean with no leftover
processes.

Real Claude smoke (Claude Code 2.1.268, production path): one execution
with an uppercase identity (`E-SMOKE-Case`) admitted, managed branch
`refs/heads/agent/e2-%45-%53%4d%4f%4b%45-%43ase`, worktree materialized
under the v2 path, attempt `SUCCEEDED` with opaque `claude:session:…`
evidence, Task `VERIFYING` (no provider self-completion), primary
checkout clean, worktree HEAD at base with only the known deferred H7
`data/sessions.db` untracked. Daemon shut down with zero residue.

## 18. Deferred findings (unchanged)

HARD-1 (SQLite/client deadline), HARD-2 (unreserved attempt misuse),
HARD-3 (stranded cancel wording), H5 (PATH-only precheck), H7
(`sessions.db` hygiene), H9 (file fitness), TEST-H (stale-daemon test
hermeticity) keep their TASK-0052 classifications. No unrelated
hardening was bundled: the only test-expectation changes are the
authorized v2 string updates and the 6 → 7 schema-version rollover.

## 19. B-RS remains for TASK-0054

This task PREVENTS known identity collisions before Task mutation, but
provides no generic recovery for admitted entries that fail or crash
before `create_attempt` (invalid repository, unresolvable revision,
process crash — including their identity reservations, which recovery
must release). That is TASK-0054, still queued. G5 stays `in_progress`.

## 20. Version

`product/version.toml` → `0.1.0-dev.task0053` (gate G5). Queue:
TASK-0053 completed; TASK-0054 queued; independent G5 closure audit
follows TASK-0054. G6 not started.
