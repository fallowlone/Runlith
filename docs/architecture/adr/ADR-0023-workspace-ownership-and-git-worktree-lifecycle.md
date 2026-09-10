# ADR-0023 - Workspace Ownership and Git Worktree Lifecycle

Status: Accepted
Date: 2026-09-10

## Context

ADR-0008 made Git a first-class subsystem (isolated worktree/branch per
autonomous execution, never direct writes to the default branch) and listed
the failure modes G4 must handle, but left the ownership model, identities,
lifecycle, cleanup safety, and crash semantics open. The SoT decision table
(§1) still phrases ownership as "worktree per execution/task attempt",
which is ambiguous about whether a worktree belongs to an Execution or an
Attempt. G3 closed with retry creating a new Attempt inside one Execution
(INV-066, TASK-0038 explicit-retry proof) and no terminal Execution
lifecycle. TASK-0040 must resolve exactly one ownership model plus the
identities and safety contracts a Git implementation needs, without
implementing Git mutations, persistence, or events itself.

## Decision

### 1. Ownership: Execution owns one workspace lease; Attempts run inside it

```text
Execution → owns one WorkspaceLease (one worktree + one managed branch)
Attempt   → runs inside its Execution's workspace (no workspace of its own)
```

A retry Attempt inside the same Execution reuses the lease and may build on
prior filesystem state; a new Execution (e.g. Task-level `RETRY_READY →
RUNNING` recovery) allocates a new lease. This preserves ADR-0008 ("each
autonomous execution gets an isolated worktree") and the SoT ("each
autonomous execution gets a separate worktree"), and is consistent with
runners.md §8 ordering (a workspace is prepared, then the Execution runs in
it): creation flow runs Task → workspace → Execution, while durable
ownership is Execution-scoped. The ambiguous "Execution/Attempt" phrasing
must not appear in authoritative G4 contracts.

### 2. Identities

- `RepositoryRef`: provider-neutral durable repository identity, distinct
  from `RepositoryRoot` (the local path, which is infrastructure data and
  may change across machines). G4 covers local repositories only.
- `WorkspaceId`: strong provider-neutral workspace identity; never a path,
  never a branch name, never an ExecutionId
  (`WorkspaceId != path != BranchRef != ExecutionId`).
- `Revision`: immutable Git object identity (commit OID; SHA-1 40-hex or
  SHA-256 64-hex, lowercase). Branch names, `HEAD`, and `main` are never
  base revisions.
- `BranchRef`: full ref (`refs/heads/<name>`) recorded on the lease,
  separate from `WorkspaceId`. Managed-branch naming is a mechanism
  detail: the adapter derives a branch-safe slug from the ExecutionId and
  records the exact ref durably. Collision detection compares the recorded
  ref against actual ref state — a new lease is never silently attached to
  an unrelated existing branch.
- Every autonomous workspace records its exact `base_revision` (the commit
  OID the worktree was created from) for diff provenance, reproducibility,
  stale detection, and review.

### 3. Default-branch protection

Autonomous execution MUST NOT mutate the repository default-branch working
tree. "Default branch" is resolved from repository metadata, never by
string-matching `main`/`master` (INV-005/INV-006 intent preserved with
correct wording). Agents work only in the supplied isolated
worktree/branch (AGENT.md §10).

### 4. Git authority vs application state

Git is authoritative for Git facts (commit objects, refs, working-tree and
index status, tracked/untracked files). AI Engineering OS persists only
ownership/provenance (lease: workspace id, repository ref, owner
ExecutionId, base revision, managed branch ref, worktree reference,
lifecycle marker). No second mutable pseudo-Git state machine: live Git
observations outrank persisted expectations on conflict, and conflicts
surface as typed failures.

### 5. Dirtiness, cleanup, crash

- Observed status is at least `Clean | DirtyTracked | DirtyUntracked |
  Missing | Invalid`; status is live-derived, never persisted as authority.
  `dirty != safe to delete`; untracked files count toward dirtiness.
- Cleanup fails closed on any dirty application-owned worktree
  (`CleanupUnsafe`/`WorktreeDirty`); `git reset --hard`, `git clean -fd`,
  forced worktree removal, and `branch -D` are never invisible normal
  cleanup. Cleanup of an already-absent owned workspace is idempotent
  (`AlreadyAbsent`), distinguished from ownership-inconsistent absence.
- Cleanup is rejected while the owning execution is known live. A crash
  implies inspection, never destruction: reconcile → inspect → propose or
  perform the next safe action; startup must not delete unknown worktrees.
- Stale is semantic, never wall-clock age: owner shows no live activity,
  the owning Task is terminal (or the lease superseded), the worktree is
  still registered/present, and cleanup is incomplete. Staleness proposes
  safe action; it authorizes nothing destructive by itself.

### 6. Diff and completion

The first G4 diff compares against the recorded `base_revision` (workspace
HEAD plus working-tree/index modifications as evidence layers), never
against the current default-branch tip. `non-empty diff != success !=
verification != COMPLETED`; completion authority stays with ADR-0018.

### 7. Concurrency and collisions

Separate worktrees per Execution for one repository must have distinct
writable paths. Two worktrees may touch the same logical file; conflict
surfaces at integration, never via a global file lock. An existing
application-owned branch ref that does not match recorded ownership is an
`OwnershipMismatch`/collision typed failure, never a silent reuse. User or
external branches are never deleted or rewritten as cleanup — ownership
must be explicit or independently proven. A dirty primary checkout neither
blocks worktree creation (creation never mutates the primary checkout) nor
is ever cleaned by the system; its status is observed and recorded as
creation provenance.

### 8. Cross-system atomicity (new invariant INV-145)

SQLite transactions and Git filesystem/ref operations are separate failure
domains and are never represented as one ACID transaction. The required
pattern is intent/durable ownership → Git operation → observe result →
durable reconciliation/finalization. Partial creation is detectable and
reconcilable; a "ready workspace" is recorded only after the Git operation
verifiably succeeded.

### 9. Persistence and Event Journal preflight

Crash recovery (ZeroClaw-derived §9, INV-078) requires durable lease facts
(workspace id, repository ref, owner ExecutionId, base revision, managed
branch ref, worktree reference, lifecycle marker) — contract defined in
`docs/architecture/workspace.md`; tables arrive in a later G4 task, never
in TASK-0040. Workspace lifecycle mutations are significant durable state
changes (INV-010) and require journal evidence with this minimal taxonomy
contract (names frozen here, implementation in TASK-0042):

```text
workspace.created   — lease allocated + verified consistent (scope:
                      project + task + execution + workspace required)
workspace.removed   — application-owned worktree + branch torn down after
                      safety checks (same scope; no dirty destruction)
```

`from_state`/`to_state` carry the small lease lifecycle
(`Allocated → Ready → CleanupPending → Removed`) as task-state-style
semantics owned by the Workspace context; no new Task lifecycle states.
No other workspace event names are canonical. TASK-0041 must not invent
durable event vocabulary.

### 10. Ports, adapter, deferred scope

The Git backend sits behind a small capability port (inspect repository,
resolve revision, create worktree, inspect status, compute diff, remove
worktree, inspect refs) — never a generic `run_git(args)` domain API.
Recommended initial backend: git CLI adapter invoked via argv arrays (no
shell, explicit paths, validated worktree roots, `--` separators, bounded
output capture): zero new dependencies, reference behavior parity,
mature worktree support. `git2`/libgit2 stays a future option. Managed
worktrees live on managed branches (unexpected detached HEAD is `Invalid`,
never silently continued). Bare repositories fail explicitly; no remote
operations (`fetch`/`pull`/`push`/PRs); submodules and LFS are
DEFERRED_BY_DESIGN with explicit limitations. Worktree isolation is Git
state isolation only — never an OS sandbox (INV-076/G6 boundary
preserved).

## Alternatives considered

- Worktree-per-Attempt: rejected; it discards useful filesystem state on
  every provider failure, contradicts the Execution as the durable
  provider-neutral unit above Attempts, and fights G3 retry semantics.
- Worktree-per-Task: rejected; concurrent executions of one Task (and
  Task-level retry with a new Execution) need distinct writable paths.
- `libgit2`/`git2` first: deferred, not rejected; CLI wins initially on
  zero-dependency, reference-parity, and testability grounds.
- Blocking worktree creation on dirty primary checkout: rejected as the
  default; creation never touches the primary checkout, so unrelated user
  dirt blocks nothing (observed and recorded instead).

## Consequences

- TASK-0041/0042/0043 implement inspection/creation, durable ownership +
  reconciliation (incl. the two journal types), and diff + safe cleanup
  against this contract without inventing vocabulary.
- `crates/workspace` holds the pure domain/contract types (IDs, refs,
  status, failure taxonomy, port) with no process, filesystem, SQLite, or
  provider surface.
- New permanent constraint INV-145 (separate failure domains +
  reconciliation, never one ACID transaction).

## Security impact

Worktree isolation remains Git-state isolation only; it grants no
filesystem/network/credential/process protection (G6 owns sandboxes).
Branch/ref handling avoids shell interpolation by construction (argv-only
rule for the future adapter). Ownership proof prevents deletion of user or
external branches. No credentials, network access, or remote operations in
G4 scope.

## Migration / replacement impact

Additive contract: no existing event row, type, version, or scope rule
changes; the two workspace event types are new names at version 1 when
TASK-0042 implements them. Swapping the git CLI adapter for `git2` later
must not touch domain semantics (port boundary).

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§1, 18–19, G4,
  ZeroClaw-derived §§8–9
- `INVARIANTS.md`: INV-005, INV-006, INV-010, INV-070, INV-076, INV-078,
  INV-145 (new)
- `docs/architecture/workspace.md`, ADR-0005 (separation), ADR-0006
  (state + journal), ADR-0008 (worktree isolation), ADR-0009 (provider
  ACL), ADR-0018 (independent evaluation), ADR-0019 (gate-scoped
  activation), ADR-0022 (runtime scope precedent)
