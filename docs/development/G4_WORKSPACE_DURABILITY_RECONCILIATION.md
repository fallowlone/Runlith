# G4 Durable Workspace Ownership + Reconciliation

> TASK-0042 — implementation evidence. Architecture authority lives in
> ADR-0023 and `docs/architecture/workspace.md`; this document evidences the
> preflight, the migration, the service, and verification, and decides
> nothing itself. Explicit boundary: **durable ownership and non-destructive
> reconciliation now exist; safe cleanup (diff/removal) and orchestration
> integration do not** — TASK-0043 owns diff + safe cleanup, and
> `OrchestrationService` was not wired to `WorkspaceService`.

## Contract Authority

- ADR-0023 §9 (frozen `workspace.created` / `workspace.removed` taxonomy +
  lease lifecycle in the journal state columns), §8 (INV-145 separate
  failure domains + reconcile), §2/§4/§5 (durable facts, Git-authoritative
  observation, dirtiness never persisted).
- `docs/architecture/workspace.md` (ownership model, failure taxonomy, port
  boundary, deferred cleanup).
- `docs/architecture/events.md` §6 (state+journal one atomic persistence
  outcome) + §4.2 precedent (closed taxonomy, workspace-fact authority).

## Preflight Answers (from the accepted contract, not from event names)

| Question | Answer (ADR-0023 authority) |
|---|---|
| `workspace.created` means | lease allocated **and** verified consistent — `Allocated → Ready`, state row + Git verified, atomically journaled |
| `workspace.removed` means | application-owned worktree + branch torn down after safety checks — `CleanupPending → Removed` |
| Journal-evidenced transitions | exactly `Allocated → Ready` (`workspace.created`) and `CleanupPending → Removed` (`workspace.removed`) |
| Operational (no event) transitions | `insert` of the `Allocated` intent, and `Ready → CleanupPending` |
| Allocated retry-safety | all durable fields fixed at allocation; no Git side effects ran, so resume = observe-then-mutate with a guarded state write |
| Worktree path authorization | application-owned `ManagedWorkspaceRoot`; every target is root-derived `<root>/ws-<slug>`; no API accepts a caller path |
| Durable facts | workspace id, repository ref, owner ExecutionId, base revision, branch ref, worktree reference, lifecycle |
| Live Git facts | clean/dirty/missing/invalid status, HEAD, branch existence — never persisted as lease authority |

No STOP condition triggered: the two-event taxonomy and the four-state
lifecycle coexist exactly — lifecycle in `from_state`/`to_state` (their
ADR-0023 §9 defined home), events only at the two transitions ADR-0023 §9
freezes. No `workspace.allocated`/`ready`/`cleanup_pending` was invented
(asserted by the closed-taxonomy test).

## Migration 0005

`adapters/storage-sqlite/migrations/0005_workspace_leases.sql`. Schema `4 →
5`; migrations 0001–0004 byte-identical. Adds `workspace_leases`
(`workspace_id` PK, `execution_id` UNIQUE FK → `executions.id`, `repository_ref`,
`base_revision`, `branch_ref`, `worktree_ref`, `lifecycle` CHECK of the four
wire names — no dirtiness column). Rebuilds `event_journal` with a nullable
`workspace_id`, the two frozen types, and a CHECK matrix: every pre-existing
family requires `workspace_id IS NULL`; `workspace.created` requires
task + execution + workspace with from/to `ALLOCATED`/`READY`; `workspace.removed`
requires task + execution + workspace with from/to `CLEANUP_PENDING`/`REMOVED`;
both forbid attempt scope. Historical rows copied verbatim (`seq` included,
`workspace_id` NULL), `AUTOINCREMENT` sequence pinned — no re-numbering, no
regenerated Event IDs.

## Durable Lease Facts

`WorkspaceLease::snapshot()` / `::restore()` persist and rebuild the exact
seven facts through domain validation (never unchecked assignment). One
Execution owns exactly one lease (`UNIQUE(execution_id)` + service-level
guard). Dirtiness is never persisted: the lease table has no dirty/status/count
column (asserted structurally).

## Live Git Facts

Observed on demand through `GitPort` only: branch existence + tip, worktree
HEAD, worktree status (`Clean`/`DirtyTracked`/`DirtyUntracked`/`Missing`/`Invalid`).
Never mirrored as authoritative lease state.

## Workspace Event Contract

`storage_sqlite::EventType` extended with exactly `WorkspaceCreated` and
`WorkspaceRemoved` — no `Other(_)`, no `workspace.*`, known-type + unknown
version fail closed. `NewJournalEvent` gained `workspace_id`,
`from_workspace_state`, `to_workspace_state`; the write path enforces the
ADR-0023 §9 scope matrix and maps lease lifecycles into the shared
`from_state`/`to_state` columns (never mixing with Task vocabulary — the
decode path refuses a Task state in a workspace row and vice versa).
`event_version = 1`. G2/G3 forms preserved; invalid mixed scopes rejected at
the write path, the SQL CHECK backstop, and read-path decode (three
independent layers). `contracts/json-schema/event.schema.json` extended with
`workspace_id` + the two `const`-bound workspace families.

## WorkspaceService

`crates/app-services/src/workspace_service.rs` — generic over `G: GitPort`
(production backend stays `git-cli`; `app-services` has no production
git-cli dependency). Coordinates domain + `WorkspaceStore` + `GitPort` +
journal + clock. Never runs shell, never knows git argv, never mutates Task
lifecycle/Attempt outcome, never selects providers, never performs cleanup
destruction. The caller supplies execution ownership + repository identity +
paths + base revision request; the service mints workspace id, derives branch
(`refs/heads/agent/<slug>` from the ExecutionId) and the root-bound target,
resolves the immutable base OID, and derives project/task journal scope
through `Execution → Task → Project` trusted state — never caller-controlled.

## Managed Workspace Root

`ManagedWorkspaceRoot`: absolute + existing + canonicalized directory. Every
worktree target is `<root>/ws-<slug>` with a traversal-safe slug (ASCII
alnum/`-`/`_` passthrough, other bytes hex-encoded, `>`64-byte input digested);
containment re-checked before use. Callers cannot request an arbitrary
target — no API accepts a worktree path. Managed ownership is application
path policy, **not** a process sandbox (documented; G6 owns sandboxes).

## Cross-System Failure Boundary

No SQLite transaction spans a Git subprocess. The saga is: (1) validate, (2)
commit `Allocated` intent, (3) Git mutation, (4) observe Git, (5) commit
`Ready` + `workspace.created`. Each SQLite write is short-lived. Step seams
(`allocate_workspace`, `materialize_allocated`, `finalize_ready`) are public
and independently testable; partial states are first-class (`CheckoutFailed`
with evidence), never "nothing happened" and never a false rollback claim.

## Creation Saga

`ensure_workspace_ready` = allocate → materialize → observe → finalize for a
fresh execution. `workspace.created` is emitted exactly at the
`Allocated → Ready` commit (the ADR-0023 §9 semantic point: verified
consistent), guarded by `WHERE lifecycle = 'ALLOCATED'` so a concurrent or
replayed transition cannot double-append.

## Reconciliation Matrix

| Durable | Live Git | Action | Event |
|---|---|---|---|
| `Allocated` | branch absent AND worktree absent | resume creation (`ResumedAllocated`) | one `created` |
| `Allocated` | exact branch/base + worktree HEAD present | recognize, finalize (`RecognizedGitState`) | one `created` |
| `Allocated` | partial/ambiguous side effects | `CheckoutFailed`; no replay, no cleanup | none |
| `Ready` | exact Git state (dirty allowed) | no-op (`AlreadyReady`) | none (created already present) |
| `Ready` | worktree missing/invalid or drift | `CheckoutFailed`; never auto-recreate | none |
| `Ready` | missing `created` event | `StorageFailure` (corrupt) | none |
| `CleanupPending` / `Removed` | any | typed refusal (cleanup is TASK-0043) | none |
| request | repository or base mismatch | `OwnershipMismatch` | none |

## Crash Recovery

- **Crash A** (intent committed, no Git): reopen → `ResumedAllocated`, no
  duplicate lease (UNIQUE + guarded).
- **Crash B** (Git done, finalization missed): reopen → `RecognizedGitState`,
  exactly one `workspace.created` across the restart.
- **Crash C** (partial Git): `side_effects_present` distinguishes "untouched"
  from "ambiguous"; ambiguous fails without replay or destruction.

## Idempotency

Repeated `ensure_workspace_ready` after a healthy workspace returns
`AlreadyReady` with no second lease, branch, worktree, or event. `Ready`
implies its `created` fact (guarded transition); reconciliation never
re-appends.

## Ownership Validation

Two attempts of one Execution resolve to the one lease (attempts never own
workspaces — branch derives from the ExecutionId). Distinct Executions own
distinct leases/worktrees/branches. Repository and base mismatches fail as
`OwnershipMismatch` before any mutation.

## Event Exactly-Once

One successful lifecycle creation emits exactly one `workspace.created`;
recognition/restart never reappends (asserted by count across crash-B and
reopen tests). `workspace.removed` codec exists but no service path emits it
without successful cleanup observation — TASK-0043 adds that observation;
until then no production call reaches `mark_lease_removed`.

## Corruption Handling

Representative invalid persisted states fail explicitly (never silently
repaired): unknown lifecycle, non-OID revision, non-ref branch, blank
worktree ref, dangling owner, duplicate execution ownership, event version 2,
Task vocabulary in a workspace row, missing `workspace_id`, attempt scope on a
workspace fact, workspace scope on a G2/G3 fact. Three layers: write-path
validation, SQL CHECK, read-path decode.

## Deferred Diff / Cleanup

`compute_diff`/`remove_worktree` remain non-mutating refusals in `git-cli`
(unchanged from TASK-0041). No diff; no `git worktree remove` / branch
deletion / reset / clean / prune in `WorkspaceService`. `CleanupPending` is
representable (storage supports it, no event) but no destructive path exists.

## Deferred Orchestration Integration

`OrchestrationService` untouched; `WorkspaceService` is production-ready but
not composed. TASK-0043/0044 can integrate once diff/cleanup lifecycle exists.

## Verification

```text
cargo fmt --all -- --check                              # clean
cargo check --workspace                                 # clean
cargo clippy --workspace --all-targets --all-features -- -D warnings  # clean
cargo test --workspace --no-fail-fast                   # 448 passed, 0 failed
cargo test -p workspace -p git-cli -p app-services -p storage-sqlite -p product-version  # green
cargo tree -p workspace     # agent-runtime + task-domain only (no SQLite/process/providers)
cargo tree -p git-cli       # workspace only
cargo tree -p storage-sqlite # task-domain, agent-runtime, workspace, rusqlite (no git)
```

New coverage: `adapters/storage-sqlite/tests/workspace.rs` (13 tests: CRUD,
guards, atomic Ready/Removed, exactly-once, SQL CHECK backstop, read-path
corruption, v4→v5 preservation, no-dirtiness, closed taxonomy, schema
pairing) and `crates/app-services/tests/workspace_service.rs` (10 tests: E2E
+ idempotency, crash A/B/C, ownership, restart durability, path
authorization, service partition), plus workspace-crate snapshot/restore +
slug unit tests.