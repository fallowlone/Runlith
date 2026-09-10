# G4 Diff + Safe Cleanup

> TASK-0043 — implementation evidence. Architecture authority lives in
> ADR-0023 and `docs/architecture/workspace.md`; this document evidences
> the preflight, the implementation, and verification, and decides nothing
> itself. Explicit boundary: **base-revision diff observation plus
> conservative idempotent cleanup now exist; merge/rebase/delivery,
> remote Git, and policy overrides do not** — TASK-0044 owns the wider
> failure matrix, and `OrchestrationService` was not wired to
> `WorkspaceService`.

## Contract Authority

- ADR-0023 §§4–6, 8–10 (Git-authoritative facts, dirtiness/cleanup/crash
  rules, base-revision diff, INV-145 separate failure domains, two-event
  taxonomy, argv-only CLI adapter, managed branches).
- `docs/architecture/workspace.md` (ownership model, lifecycle
  `Allocated → Ready → CleanupPending → Removed`, failure taxonomy,
  port boundary, deferred scope).
- `docs/architecture/events.md` §6 (state+journal one atomic persistence
  outcome) + §4.2 precedent (closed taxonomy, service-minted authority).
- Prior evidence: `G4_WORKSPACE_GIT_CONTRACT.md` (TASK-0040),
  `G4_GIT_WORKTREE_CREATION.md` (TASK-0041),
  `G4_WORKSPACE_DURABILITY_RECONCILIATION.md` (TASK-0042).

## Architecture Preflight

All 14 destructive cases resolve in accepted architecture; no STOP fired:

| # | Question | Answer (authority) |
|---|---|---|
| 1 | Diff baseline | Recorded `base_revision` OID, never live default tip (ADR-0023 §6) |
| 2 | Dirtiness semantics | Live `Clean \| DirtyTracked \| DirtyUntracked \| Missing \| Invalid`, never persisted (ADR-0023 §5) |
| 3 | `CleanupPending` | Teardown requested; safety checks still apply; no event; authorizes nothing destructive (lease/store docs, TASK-0042) |
| 4 | Ordinary cleanup safety | Fails closed on dirt; no `reset --hard` / `clean` / force removal / `branch -D` as normal cleanup (ADR-0023 §5) |
| 5 | Dirty tracked files | `WorktreeDirty`, preserved (same) |
| 6 | Untracked files | Count as dirt, protected as evidence (ADR-0023 §5, workspace.md) |
| 7 | Active owner | `ActiveOwnerRejectsCleanup` while known live; liveness stays contractual in G4, never faked (`GitPort::owner_is_live`) |
| 8 | Worktree ownership | Lease + registration + attached branch must agree or `OwnershipMismatch`, never silent reuse/deletion (ADR-0023 §7) |
| 9 | Branch ownership | Recorded ref vs actual ref; unproven external branches never deleted (same) |
| 10 | Worktree removal order | Validate → eligibility → observe → reject dirt → commit `CleanupPending` → remove → observe (ADR-0023 §8) |
| 11 | Branch removal order | Only after safe worktree removal, only the recorded lease branch, only `branch -d` (ADR-0023 §§7/9) |
| 12 | `workspace.removed` point | Only at `CleanupPending → Removed` with both Git resources absent (TASK-0042 + store guard) |
| 13 | Crash Git-vs-SQLite | Separate domains (INV-145); reconcile `CleanupPending` + live Git, finalize exactly once |
| 14 | Already-absent | Idempotent `AlreadyAbsent`/`AlreadyRemoved` for ownership-consistent absence, distinct from drift |

Two readings needed conservative (not invented) resolution, both fail-closed:
dirty-`CleanupPending` stays blocked with no event and no `Removed`
implication (no `CleanupPending → Ready` invented); unmerged managed
branches survive ordinary cleanup via non-forced `branch -d` refusal with
the lease held at `CleanupPending` (see Managed Branch Safety).

## Diff Baseline

The immutable `WorkspaceLease.base_revision` is the only baseline.
`WorkspaceService::compute_diff` rejects a diff whose baseline drifted
from the recorded revision; the adapter proves the base OID names a
commit visible from the worktree and resolves `HEAD` independently.
Branch names, `HEAD`, and default-branch tips are never baselines
(`Revision` validation already forbids them).

## Live Git Status

`worktree_status` (TASK-0041, unchanged) observes
`Clean | DirtyTracked | DirtyUntracked | Missing | Invalid` on demand —
detached HEAD is `Invalid`, `??` lines rank above tracked dirt, missing
paths are `Missing`. No dirtiness column exists on `workspace_leases`
(structurally asserted by existing storage tests); schema stays v5, no
migration 0006.

## Diff Evidence

`WorktreeDiff` (`crates/workspace/src/port.rs`): `base_revision`,
`head_revision`, `has_working_tree_changes`, live `status`, plus two
bounded evidence layers — `committed` (`base..HEAD` via
`diff --name-status -z --find-renames`) and `working`
(`status --porcelain=v1 -z`) — capped at `MAX_DIFF_ENTRIES` (512) per
layer with an explicit `truncated` flag. `DiffEvidence.summary` is a
deterministic bounded count line (never raw file dumps, never patch
text). Kinds cover added/modified/deleted/renamed (where Git reports
them)/untracked; staged and unstaged changes are both visible in the
working layer. Diff runs read-only commands only (`rev-parse`, `diff`,
`status`), holds no SQLite transaction, mints no event, and never
touches Task lifecycle — `non-empty diff != success != verification !=
COMPLETED`, and empty diff alone means nothing about failure.

## Cleanup Preconditions

`cleanup_workspace(workspace_id, repository_path)` — addressed by
`WorkspaceId`/execution ownership, never by caller path — enforces, in
order: lease exists and is `Ready`/`CleanupPending` (`Allocated` and
unknown ids fail; `Removed` takes the idempotent read path); derived
`<root>/ws-<slug>` equals the durable worktree reference; live paths
canonicalize inside the managed root (traversal/symlink escape fails
closed) and outside the primary checkout; the repository path is a
working repository; the lease branch is service-derived `agent/` scope;
the branch is not the metadata-resolved default; the owner shows no
live activity; live dirtiness is observed. Any disagreement fails
closed with no mutation and no event.

## Dirtiness Protection

Dirtiness is checked **before** `Ready → CleanupPending`, so a dirty
`Ready` lease stays `Ready` with nothing written — then re-checked
immediately before destructive removal (TOCTOU), with Git's own
non-forced `worktree remove` refusal as the second boundary. Dirty
(tracked, staged, or untracked-only) refuses as `WorktreeDirty`; user
edits are indistinguishable from agent output and equally protected.
There is no `reset --hard`, `clean -fd`, `stash`, automatic commit,
trash/archive move, or force flag anywhere in the path.

## Managed Branch Safety

Branch deletion is a separate step after verified worktree removal,
restricted to the recorded lease branch via non-forced `branch -d`
only (never `-D`). The protected/default branch (metadata-resolved,
proven by the `trunkline` test — no `main`/`master` literals),
external/user branches (non-`agent/` refs refused), branches attached
in another worktree, and branches with unique unmerged commits all
survive: Git's refusal surfaces as `CleanupUnsafe` with the branch
preserved. Removing the worktree never implies its commits are
disposable.

## Worktree Removal

`GitCli::remove_worktree` (new `adapters/git-cli/src/cleanup.rs`):
re-validates dirtiness (dirty → `WorktreeDirty`), proves the attached
`HEAD` equals the lease branch (detached/unexpected → `OwnershipMismatch`),
proves registration in `worktree list` (canonical compare), runs one
`worktree remove <abs-path>` from the discovered primary checkout
(never the removed directory as cwd, never a shell, non-interactive,
bounded diagnostics), then verifies path-gone plus registration-gone.
Absent paths report `WorktreeMissing` (registration without repo
context cannot prove full absence — the service reconciles via
lifecycle plus branch state). No prune is ever executed; stale
filesystem-vs-registration drift (manual `rm -rf`) surfaces as a
refused branch deletion (`used by worktree`), never silent success.

## Cleanup Lifecycle

`Ready → CleanupPending` commits operationally with no event
(`request_cleanup` / `mark_lease_cleanup_pending`); `CleanupPending →
Removed` commits atomically with `workspace.removed`
(`mark_lease_removed`, guarded `WHERE lifecycle = 'CLEANUP_PENDING'`).
`workspace.created` history is never altered; a blocked
`CleanupPending` lease (dirty, unmerged branch) remains explicitly
blocked — never re-armed to `Ready`, never implying deletion.

## workspace.removed Authority

Only `WorkspaceService` mints `workspace.removed`, deriving
project/task/execution/workspace scope through the trusted
`Execution → Task → Project` chain plus service-minted
correlation/causation/timestamp/provenance — callers supply none of it.
The store writes the row transition and the journal fact in one SQLite
transaction; the guarded transition makes concurrent finalizers fail
instead of double-appending. No new event names were added
(`workspace.created` / `workspace.removed` stay the closed taxonomy).

## Cross-System Failure Boundary

No SQLite transaction spans a Git subprocess: each store commit is
short-lived; Git runs between commits; observations decide the next
step. Partial states (intent without Git, Git without finalization)
are first-class and recoverable, never reported as nothing-happened.

## Cleanup Reconciliation Matrix

| Durable | Live Git | Action | Event |
|---|---|---|---|
| `Ready` | clean worktree, branch as recorded | commit `CleanupPending`, remove, finalize | one `removed` |
| `Ready` | dirty worktree | `WorktreeDirty`; stays `Ready` | none |
| `Ready` | worktree missing | `WorktreeMissing` drift; stays `Ready` | none |
| `Ready` | unusable/mismatched | fail closed; stays `Ready` | none |
| `CleanupPending` | clean, branch as recorded | remove worktree + branch, finalize | one `removed` |
| `CleanupPending` | dirty | `WorktreeDirty`; stays `CleanupPending`, preserved | none |
| `CleanupPending` | worktree absent, branch merged/absent | delete branch if present, finalize | one `removed` |
| `CleanupPending` | worktree absent, branch unmerged/stale-held | `CleanupUnsafe`; branch preserved, stays `CleanupPending` | none |
| `CleanupPending` | worktree removed, branch preserved (unique) | `CleanupUnsafe`; stays `CleanupPending` | none |
| `Removed` | both resources absent + fact present | `AlreadyRemoved` no-op | none (fact already present) |
| `Removed` | worktree or branch present | corruption; never resurrected, never deleted | none |
| `Removed` | `removed` fact missing | `StorageFailure` corruption | none |
| `Allocated` | any | cleanup does not apply | none |

## Crash Recovery

- **After `CleanupPending`, before Git removal:** reopen completes the
  saga normally (one `removed`).
- **Worktree removed, branch remains:** reopen observes the absent
  worktree, deletes the branch only if safely authorized (merged), and
  finalizes; unmerged/stale-held branches stay preserved at
  `CleanupPending`.
- **Full Git cleanup done, SQLite still `CleanupPending`:** reopen
  observes both resources absent and finalizes `Removed` + one
  `workspace.removed` with no further Git mutation (atomicity seam:
  either both halves committed or neither — verified by zero `removed`
  facts before finalization and exactly one after).
- **Finalization-transaction failure:** the guarded
  `CleanupPending → Removed` + journal transaction rolls back as one
  outcome (storage-layer atomicity, pre-existing); reopen retries to
  exactly one `removed`.

## Idempotency

One lease removal emits exactly one `workspace.removed`: the guarded
store transition plus the `AlreadyRemoved` read path (lease `Removed`,
both resources absent, fact present) make repeats no-ops with no Git
mutation, no event, no recreation, and no resurrection.

## User-Data Preservation

Dirty tracked, staged-only, and untracked-only workspaces all refuse
with byte-identical user data left in place (dedicated regressions for
each, including the easy-to-miss untracked-only case). Unique managed
commits survive ordinary cleanup. The primary checkout is never
cleaned/reset/stashed — a dirty primary neither blocks managed cleanup
nor is touched (byte-identical bytes + status proven). Sibling
worktrees are never reset/cleaned/removed by another lease's cleanup.

## Multi-Workspace Isolation

`E1 → W1`, `E2 → W2`: removing `W1` leaves `W2` (files, branch,
dirtiness) and the primary checkout unchanged; verified with a dirty
sibling in place. No repository-wide side effects; no global locks.

## Security Boundary

Bounded local diff observation plus conservative application-owned
worktree/branch teardown. Dirty or ambiguously owned work fails closed;
no force deletion, shell interpolation (direct argv throughout, spaces/
Unicode/metachar paths literal), remote Git, provider execution,
sandbox claim, secret access, or Task completion authority is
introduced. Git/worktree isolation remains Git-state isolation only
(G6 owns sandboxes). No `force_*` / `ignore_dirty` / policy-override
flags exist — G6 policy does not exist yet, and unsafe cleanup fails
closed.

## Deferred Merge / Delivery

Not implemented: `merge`, `rebase`, `cherry-pick`, `commit`, `add`,
`push`/`fetch`/`pull`/`clone`, automatic cleanup on Task completion,
orchestration workspace planning, and delivery reconciliation. A
`CleanupPending` lease with a preserved unmerged branch is the
intentional handoff state for future delivery work.

## Verification

```text
cargo fmt --all -- --check                              # clean
cargo check --workspace                                 # clean
cargo test --workspace --no-fail-fast                   # 489 passed, 0 failed
cargo clippy --workspace --all-targets --all-features -- -D warnings  # clean
cargo test -p workspace -p git-cli -p app-services -p storage-sqlite -p product-version
cargo test -p git-cli --test diff_cleanup               # 17 passed
cargo test -p app-services --test workspace_diff_cleanup # 20 passed
cargo test -p app-services --test workspace_service     # 10 passed (TASK-0042 regression green)
cargo tree -p workspace      # agent-runtime + task-domain only
cargo tree -p git-cli        # workspace only
cargo tree -p storage-sqlite # task-domain, agent-runtime, workspace, rusqlite (no git)
```

New coverage: `adapters/git-cli/tests/diff_cleanup.rs` (17 tests:
clean/tracked/staged/untracked/deleted/HEAD-advanced/rename/scoping/
spaces-unicode/absent-detached diff; clean removal + branch deletion;
tracked/staged/untracked refusal; branch mismatch; unmerged/default/
absent branch; sibling isolation) and
`crates/app-services/tests/workspace_diff_cleanup.rs` (20 tests: clean
diff + all change classes + non-mutation + scoping; full lifecycle +
exactly-once + ordering + reopen + idempotency + Task separation;
tracked/staged/untracked refusal staying `Ready`; dirty-`CleanupPending`
fail-closed; unmerged preservation; three crash seams; `Ready`-missing
drift; `Removed` corruption; isolation + dirty sibling + dirty primary;
`trunkline` default safety; branch drift; moved worktree; invalid path;
live-owner stub rejection; creation-reconciliation regression).
Schema `SUPPORTED_SCHEMA_VERSION = 5`; migrations `0001`–`0005` only.
No `TestMutationPolicy` violation: the superseded TASK-0041 deferral
refusal test (`diff_and_removal_are_deferred_without_mutation`) was
repurposed to `diff_and_removal_are_implemented_safely` because this
task explicitly implements the deferred operations; all other existing
tests are byte-identical and green.
