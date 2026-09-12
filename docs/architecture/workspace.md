# Workspace / Git

> Repository-local contract for the Workspace/Git subsystem: ownership,
> identities, lifecycle, safety, and boundaries for autonomous execution on
> isolated worktrees. Documents accepted architecture (ADR-0008, ADR-0023);
> invents nothing beyond them; implements nothing.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md) (INV-005, INV-006,
>   INV-010, INV-070, INV-076, INV-078, INV-145)
> - Decisions: [`adr/ADR-0008-worktree-isolation.md`](./adr/ADR-0008-worktree-isolation.md),
>   [`adr/ADR-0023-workspace-ownership-and-git-worktree-lifecycle.md`](./adr/ADR-0023-workspace-ownership-and-git-worktree-lifecycle.md)
> - Task lifecycle: [`task-engine.md`](./task-engine.md)
> - Runner relationship: [`runners.md`](./runners.md) §8
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§18–19, G4

## Purpose

Give every autonomous execution an isolated Git worktree and branch with
explicit ownership, an immutable base revision, and safe lifecycle
semantics — so agents never touch the user's checkout or the repository
default branch, diffs carry provenance, crashes stay inspectable, and
cleanup never destroys uncommitted work. The subsystem answers:

```text
Who owns a worktree? What revision is it based on? What may be cleaned up?
What happens after a crash? What is Git truth vs application record?
```

## Ownership Model

```text
Task
 ↓
Workspace / Worktree (one lease per Execution)
 ↓
Branch (one managed branch per lease)
 ↓
Execution (lease owner; Attempts run inside)
 ↓
Diff (against recorded base_revision)
 ↓
Verification (independent; ADR-0018)
```

One Execution owns exactly one `WorkspaceLease`; every Attempt of that
Execution runs inside the lease's worktree. A new Execution allocates a
new lease. Provider adapters never own Git lifecycle (ADR-0009); the
Runner consumes a prepared workspace and owns no Git semantics
(runners.md §8).

## Repository Identity

`RepositoryRef` is the provider-neutral durable repository identity;
`RepositoryRoot` is the local path (infrastructure data, may change
across machines). G4 covers local repositories only — no remote
GitHub/GitLab identity, no cloud account, no networked operations
(`fetch`/`pull`/`push`/PRs are out of scope).

## Workspace Identity

`WorkspaceId` is a strong provider-neutral identity:

```text
WorkspaceId != path != BranchRef != ExecutionId
```

A lease conceptually carries only what G4 requires: workspace id,
repository ref, owner ExecutionId, base revision, managed branch ref,
worktree reference, lifecycle marker. Absolute local paths never enter
provider-neutral Task semantics.

## Base Revision

Every autonomous workspace records the exact commit OID it was created
from (`Revision`: SHA-1 40-hex or SHA-256 64-hex, lowercase). Branch
names, `HEAD`, and default-branch tips are never base revisions. The OID
grounds diff provenance, reproducibility, stale detection, and review.

## Branch Semantics

`BranchRef` (`refs/heads/<name>`) is recorded on the lease, separate from
`WorkspaceId`. The managed-branch derivation maps the owner ExecutionId
to `refs/heads/agent/<v2-slug>` (TASK-0053): a versioned,
canonical-lowercase encoding over the exact identity bytes — `e2-…` for
the reversible readable form (`[a-z0-9-_]` pass through, every other byte
as `%` + lowercase hex), `h2-…` for the SHA-256 digest fallback past the
readable budget. Readable outputs are exact-byte-preserving and
reversible, hence injective over their supported input domain, and
already ASCII-casefold-stable, so case-insensitive filesystems cannot
merge two outputs. Digest outputs are deterministic and
collision-resistant — explicitly NOT mathematically injective — and a
digest collision fails closed through the durable identity reservation
(migration 0007, committed atomically with execution entry) instead of
silently reusing a workspace. The exact ref is recorded durably; branch
collisions are detectable because a new lease is never silently attached
to an unrelated existing branch — a ref that exists without matching
recorded ownership is an `OwnershipMismatch` typed failure. Execution
entry additionally refuses before its first mutation when the derived
branch is already reserved or recorded for another execution. Pre-v2
persisted leases keep their recorded branch/path byte-for-byte (never
rederived) and reconcile through containment checks. Historical outputs
may lexically overlap the `e2-`/`h2-` prefixes because old ExecutionIds
could begin with those strings. TASK-0058 therefore classifies a persisted
lease from the complete branch/worktree pair: exact current-v2 derivation
plus the owner's durable identity reservation is current v2; an exact pair
from a supported historical derivation is legacy; mixed, impossible, or
reservation-conflicting pairs fail closed. Application-created
vs user/external branches are distinguished by explicit recorded
ownership; an unproven external branch is never deleted or rewritten as
cleanup. Managed worktrees live on managed branches; an unexpected
detached HEAD is `Invalid`, never silently continued.

## Default Branch Protection

Autonomous execution MUST NOT mutate the repository default-branch working
tree. The default branch is resolved from repository metadata — never by
string-matching `main`/`master` (this phrasing preserves INV-005/INV-006
intent). Agents work only in the supplied isolated worktree/branch.

## Git Authority vs Application State

Git is authoritative for Git facts: commit objects, refs, working-tree and
index status, tracked/untracked files. The application persists only
ownership/provenance. Live Git observations outrank persisted expectations;
disagreement surfaces as typed failure, never as a rewritten pseudo-Git
state.

## Workspace Lifecycle

Small durable ownership lifecycle plus live observations (never mirrored
as persisted state):

```text
Allocated → Ready → CleanupPending → Removed
```

`Allocated`: intent recorded, Git operation not yet verified. `Ready`:
worktree + branch verified mutually consistent with the lease. Transitions
require observation, and SQLite/Git non-atomicity (INV-145) is handled by
intent → Git operation → observe → reconcile/finalize. Partial creation is
detectable and reconcilable; `Ready` is recorded only after success is
observed.

## Dirtiness

Live-observed status, at minimum:

```text
Clean | DirtyTracked | DirtyUntracked | Missing | Invalid
```

Untracked files count toward dirtiness and are protected as evidence.
`dirty != safe to delete`. A dirty primary checkout neither blocks
worktree creation (creation never mutates the primary checkout) nor is
ever cleaned by the system; its status is observed and recorded as
creation provenance. User edits inside a managed worktree surface as typed
dirty/conflict observation — never silent overwrite, never automatic
conflict resolution.

## Diff Semantics

The G4 diff compares against the recorded `base_revision`: workspace HEAD
history plus working-tree/index modifications as evidence layers — never
against the live default-branch tip. Diff is Git evidence for later
verification: `non-empty diff != success != verification != COMPLETED`
(ADR-0018 authority unchanged); a clean diff alone means nothing about
failure.

## Cleanup Safety

Hard invariant: a dirty application-owned worktree cannot be silently
destroyed. Normal cleanup fails closed (`CleanupUnsafe`/`WorktreeDirty`)
on uncommitted or untracked work; `git reset --hard`, `git clean -fd`,
forced worktree removal, and `branch -D` are never invisible normal
cleanup. Cleanup of an already-absent owned workspace is idempotent
(`AlreadyAbsent`), distinguished from ownership-inconsistent absence.
Cleanup is rejected while the owning execution is known live
(`ActiveOwnerRejectsCleanup`); process tracking stays contractual in G4
(full Runner process authority arrives later — never faked).

## Crash / Reconciliation

A crash implies inspection, never destruction. After restart the system
reconciles durable ownership, the Git worktree registry, filesystem,
branch, and Execution state, then classifies each lease and proposes or
performs the next safe action. Startup must not delete unknown worktrees.
Stale is semantic, never wall-clock age: no live owner activity, owning
Task terminal (or lease superseded), worktree still registered/present,
cleanup incomplete. Staleness proposes action; it authorizes nothing
destructive alone. Concurrent executions of one repository hold distinct
writable paths; same-file touches across worktrees surface at
integration, never via global file locks.

Pre-attempt entry recovery (TASK-0054, decided in ADR-0025) reuses the
`ensure_workspace_ready` reconciliation paths: a resumed execution with
no lease allocates fresh, an `Allocated` lease resumes (`ResumedAllocated`)
or is recognized (`RecognizedGitState`) without replaying Git mutation,
and a `Ready` lease continues idempotently (`AlreadyReady`) with
exactly-once `workspace.created`. The daemon-startup scan surfaces
stranded pre-attempt entries with their resume invocation; startup itself
spawns no provider work. Post-attempt crash/orphan recovery stays G11
work.

TASK-0057 adds one narrow `Allocated` recovery class for current v2
leases. If the managed branch already exists exactly at the recorded base,
the lease and durable identity reservation prove the same Execution owns
it, no competing lease claims the branch/path, the target path is absent,
and Git registers neither the target nor that branch in any worktree, the
service continues the interrupted materialization using the existing
branch (`git worktree add <path> <existing-branch>`). It never deletes or
resets the branch. Any missing proof, unexpected branch tip, existing
target, foreign registration, or ownership disagreement remains
fail-closed. A second crash after attachment but before finalization is
reconciled as the already-supported complete-Git/`Allocated` shape.
TASK-0058 keeps this branch-only continuation current-v2-only. A genuine
pre-v2 `Allocated` lease uses its persisted historical identity through the
ordinary reconciliation/materialization path instead of entering the v2
branch-only proof. Ambiguous or corrupt format evidence is refused before
either path mutates Git.

## Failure Taxonomy

Bounded typed classes (domain-level; infrastructure diagnostics stay
separate, one class per cause family — never one per stderr string):

```text
NotRepository | BareRepositoryUnsupported | DirtyPrimaryWorkspace
BranchCollision | WorktreeCollision | WorktreeDirty | WorktreeMissing
CheckoutFailed | OwnershipMismatch | InvalidRevision | CleanupUnsafe
ActiveOwnerRejectsCheckout | ActiveOwnerRejectsCleanup
GitBackendFailure | AlreadyAbsent
```

## Port Boundary

The backend sits behind a small capability port — inspect repository,
resolve default branch and revisions, create worktree, inspect status,
compute diff, remove worktree, inspect refs. No generic
`run_git(args: Vec<String>)` domain API; raw command execution lives only
inside the concrete adapter, invoked via argv arrays (no shell, explicit
paths, validated roots, `--` separators, bounded output capture).

## Initial Adapter Decision

Git CLI adapter first: zero new dependencies, reference behavior parity,
mature worktree support, straightforward fixture testing. `git2`/libgit2
remains a future option behind the same port. Bare repositories fail
explicitly; submodules and LFS are DEFERRED_BY_DESIGN with documented
limitations.

## Deferred Git Features

Submodule management, LFS orchestration, remote operations, detached-HEAD
workflows, Execution terminal lifecycle, Session persistence, ergonomic
scheduler retry entry points, merge-queue mechanics — each owned by its
later gate. Post-v1 concepts (Tournament/Consensus/Shadow) stay excluded.

## Critical Invariants

```text
INV-005  isolated worktrees/workspaces for autonomous execution
INV-006  never write directly to the default branch (wording modernized;
         intent unchanged)
INV-010  lease mutations are auditable (workspace.created/removed contract)
INV-145  SQLite and Git are separate failure domains; reconcile, never one
         ACID transaction
default-branch protection resolves from metadata, not branch-name strings
dirty work is never silently destroyed; crashes are inspected, not cleaned
diff is evidence only; completion authority never moves (ADR-0018)
Git isolation != OS sandbox (G6 owns sandboxes; INV-076)
```

## Non-goals

This document does NOT define: CLI argv spellings, table schemas,
journal codec bytes, merge-queue design, rebase/merge conflict
resolution, review workflows, or any remote/networked Git behavior.
