# Workspace - derived navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived guidance only; canonical policy is [AGENT.md](../../AGENT.md).
Package: [workspace](Cargo.toml). Contract: [Workspace / Git](../../docs/architecture/workspace.md).

## Ownership and source map

This domain owns values and ports; [git-cli](../../adapters/git-cli/src/lib.rs)
executes Git, and [WorkspaceService](../app-services/src/workspace_service.rs)
coordinates durable intent, observations and cleanup admission.

| Concern | Primary source | Boundary |
| --- | --- | --- |
| Public surface | [lib](src/lib.rs) | Exported domain modules and identity re-exports. |
| Identity validation | [ids](src/ids.rs) | RepositoryRef, WorkspaceId, BranchRef and immutable Revision. |
| Durable ownership | [lease](src/lease.rs) | Execution owns the lease; Attempts run within it. |
| Git capabilities | [port](src/port.rs) | Narrow GitPort operations and bounded diff evidence. |
| Persistence contract | [store](src/store.rs) | WorkspaceStore and journal evidence, implemented outside this crate. |
| Trusted binding | [trusted](src/trusted.rs) | Canonical common-Git directory captured at trusted admission. |
| Live observations | [status](src/status.rs), [liveness](src/liveness.rs) | Worktree status and consumer-owned OwnerLiveness. |
| Typed failures | [failure](src/failure.rs) | Ownership, dirtiness, liveness and backend failures. |

## Local traps

- [Lease lifecycle](src/lease.rs) is `Allocated -> Ready -> CleanupPending -> Removed`.
  `CleanupPending` records intent, not permission to delete; dirtiness is live-derived.
- [TrustedCommonGit](src/trusted.rs) is distinct from durable `RepositoryRef`.
  Restore requires an absolute, existing canonical path and fails closed;
  never reconstruct a missing trusted anchor from provider-writable observations.
  [GitPort](src/port.rs) takes the expected anchor on repository/worktree operations.
- [Managed identity](../app-services/src/managed_identity.rs) derives lowercase
  `e2-`/`h2-` names from exact identity bytes. Reuse that derivation; case folding
  input identifiers would collapse distinct owners. Persisted identity uses the
  complete branch/path pair, not a prefix guess.
- [OwnerLiveness](src/liveness.rs) reflects actual process ownership. An unwired
  backend reports false; that does not bypass independent ownership/dirt checks.
- [Cleanup admission](../app-services/src/workspace_service.rs) checks managed
  path, repository binding, live owner, protected branch and current status.
  Missing `Ready` worktrees are drift; missing `CleanupPending` worktrees require
  history binding before continuation. [Adapter cleanup](../../adapters/git-cli/src/cleanup.rs)
  repeats safety observations and uses non-forced removal and `branch -d`.

## Targeted verification from repository root

```sh
rtk cargo test --locked -p workspace --lib
rtk cargo test --locked -p app-services --test workspace_diff_cleanup
rtk cargo test --locked -p git-cli --test diff_cleanup
```

The domain target covers inline tests in the [exported modules](src/lib.rs).
[Service cleanup tests](../app-services/tests/workspace_diff_cleanup.rs) cover admission;
[adapter diff/cleanup tests](../../adapters/git-cli/tests/diff_cleanup.rs) exercise real Git.
See [ADR-0023](../../docs/architecture/adr/ADR-0023-workspace-ownership-and-git-worktree-lifecycle.md)
for ownership/lifecycle contracts. Commands are entry points, not execution evidence.
