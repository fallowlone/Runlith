# G4 Workspace/Git Contract

> TASK-0040 — contract-only evidence. Architecture authority lives in
> ADR-0008, ADR-0023, and `docs/architecture/workspace.md`; this document
> evidences the preflight and decisions, and decides nothing itself. No Git
> mutation, persistence migration, event implementation, provider work, or
> production code of any kind was added by the contract decision itself
> (the pure `crates/workspace` domain types are covered separately below).

## Preflight

| Question | Existing authority | Resolved? | Conflict? | Action |
|---|---|---|---|---|
| Git first-class subsystem | ADR-0008; SoT §18 | Yes | No | Preserve |
| Default-branch protection | ADR-0008; INV-006; AGENT.md §10 | Yes (wording modernized to metadata-resolved default) | No | Preserve intent in ADR-0023 §3 |
| Worktree owner (Execution vs Attempt) | ADR-0008 "each autonomous execution"; SoT §18 "each autonomous execution"; SoT table "per execution/task attempt" (ambiguous) | Partial | No — ordering vs ownership, reconciled | ADR-0023 §1: Execution owns lease; Attempts run inside |
| Base revision explicit OID | Implied by diff-provenance needs; not explicit | No | No | ADR-0023 §2: `Revision` OID required |
| Repository/Workspace/Branch identity | Missing | No | No | ADR-0023 §2 + `crates/workspace` types |
| Dirty/cleanup/crash/stale semantics | ADR-0008 consequences; AGENT.md §10; doc-workspace rules | Partial | No | ADR-0023 §§4–5 + workspace.md |
| SQLite/Git non-atomicity | INV-078/INV-070 (parts); ZeroClaw §9 | Partial | No | New INV-145 + ADR-0023 §8 |
| Durable lease facts needed? | INV-078 incremental persistence; recovery needs worktree state | Decided yes | No | Contract in workspace.md; tables in TASK-0042 |
| New journal types needed? | INV-010 auditable mutations | Decided yes, minimal | No | `workspace.created/removed` taxonomy frozen in ADR-0023 §9; implementation in TASK-0042 |
| Adapter strategy | No prior choice | Decided | No | git CLI recommended, ADR-0023 §10 |
| Provider coupling | ADR-0009 | Yes (forbidden) | No | Enforced in port + crate deps |
| Runner/process/Git absence in G3 | Verified (no such surface) | Yes | No | Nothing to remove |

No STOP condition triggered: nothing accepted contradicts the contract,
and every gap is explicitly TASK-0040's chartered scope.

## Existing Authority

ADR-0008 (isolated worktree/branch per autonomous execution; failure-mode
list; merge only via verification-gated queue), SoT §§18–19 (subsystem
order Task → Worktree → Branch → Execution → Diff → Verification → Merge
Queue; per-execution worktrees; 13 mandatory failure modes), runners.md §8
(Runner consumes a prepared workspace, owns no Git semantics), INV-005/
INV-006 (isolation + default-branch protection), INV-010 (auditability),
INV-066 (no transparent side-effect replay), INV-076 (fail-closed
isolation), INV-078 (incremental persistence), AGENT.md §§10/20 (worktree
rules; canonical-state ownership; no recursive temp cleanup).

## Resolved Ambiguities

1. "Worktree per execution/task attempt" → exactly one owner: the
   Execution holds the lease; Attempts run inside it (§Ownership Decision).
2. runners.md ordering (worktree above Execution) vs ownership → creation
   flow vs durable ownership; both preserved, distinguished explicitly.
3. `main`/`master` literals → metadata-resolved repository default branch.
4. Dirty primary checkout → observed + recorded, blocks nothing, cleaned
   never.
5. Stale → semantic (no live owner, terminal owning Task or superseded
   lease, worktree present, cleanup incomplete); age is at most a
   prioritization hint.

## Ownership Decision

Execution owns one `WorkspaceLease` (one worktree + one managed branch);
Attempts run inside it; new Executions allocate new leases. Rejected:
worktree-per-Attempt (destroys useful state on provider failure, fights
G3 retry semantics) and worktree-per-Task (concurrent executions need
distinct paths). Provider adapters and Runners own no Git lifecycle.

## Persistence Decision

Durable lease facts required for crash recovery: workspace id, repository
ref, owner ExecutionId, base revision, managed branch ref, worktree
reference, lifecycle marker (`Allocated → Ready → CleanupPending →
Removed`). Observed dirtiness is never persisted as authority. Contract
defined now (`workspace.md`); tables scheduled in TASK-0042. No migration
in TASK-0040 (schema stays v4).

## Event Journal Decision

Minimal taxonomy frozen as contract (no code touched): `workspace.created`
(allocation verified consistent) and `workspace.removed` (safe teardown),
scoped project + task + execution + workspace at version 1, with the small
lease lifecycle in `from_state`/`to_state`. No other workspace event names
are canonical; TASK-0041 must not invent vocabulary; TASK-0042 implements.

## Adapter Recommendation

Git CLI via argv arrays (no shell, explicit paths, validated roots, `--`
separators, bounded capture): zero new dependencies, reference behavior
parity, mature worktree support, easy fixtures. `git2` stays a future
port-compatible option. Evaluation criteria applied: parity, worktree
support, argument safety, cross-platform behavior, dependency cost,
testability — CLI wins initially on all but raw in-process speed, which
G4 does not require.

## Failure Model

Bounded domain taxonomy (`WorkspaceFailure` in `crates/workspace`):
`NotRepository | BareRepositoryUnsupported | DirtyPrimaryWorkspace |
BranchCollision | WorktreeCollision | WorktreeDirty | WorktreeMissing |
CheckoutFailed | OwnershipMismatch | InvalidRevision | CleanupUnsafe |
ActiveOwnerRejectsCheckout | ActiveOwnerRejectsCleanup |
GitBackendFailure | AlreadyAbsent`. SoT §19's thirteen failure modes map
onto these classes plus the already-tested orchestration layer (user
edits → dirty observation; rebase/merge conflicts → integration-time
surface, out of initial scope but classified). Dirty-primary policy,
detached-HEAD (`Invalid`), bare-repo, and remote-op exclusions decided
per contract.

## Deferred Scope

Submodules, LFS, remote operations, detached workflows, Execution
terminal lifecycle, Session persistence, scheduler retry entry points,
merge queue, conflict resolution, G6 sandboxes/policy, G5 providers —
each fenced to its gate; none is debt.

## Validation

- `cargo fmt --all -- --check`, `cargo check --workspace`,
  `cargo test --workspace --no-fail-fast`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test -p product-version` — all green (recorded in task result).
- `cargo test -p workspace` — pure domain validation green.
- `cargo tree -p workspace` — `agent-runtime` + `task-domain` only (no
  SQLite, process, provider, Tokio surface; asserted in §Next).
- 18 architecture validation questions answered in `workspace.md` + ADR
  with no remaining "Execution/Attempt" ambiguity (verified by grep:
  the only occurrences are this sentence and historical quotes).

## Next Implementation Task

Bounded G4 sequence materialized in `planning/tasks/queue.yaml`:
TASK-0041 (inspection + worktree creation over the port), TASK-0042
(durable ownership + reconciliation, incl. the two journal types),
TASK-0043 (diff + safe cleanup), TASK-0044 (G4 Git failure matrix),
TASK-0045 (G4 closure audit). No G5 tasks created.
