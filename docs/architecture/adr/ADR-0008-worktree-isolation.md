# ADR-0008 - Isolated worktrees for autonomous execution

Status: Accepted
Date: 2026-09-09

## Context

Autonomous agents modify code. Running them directly in the user's checkout
risks dirty-tree corruption, concurrent edits to the same files, branch
collisions and unreviewed writes to protected branches.

## Decision

Git is a first-class subsystem: each autonomous execution gets an isolated
worktree/branch derived from its Task/attempt, flowing Task → Worktree →
Branch → Execution → Diff → Verification → Merge Queue. Agents never write
directly to main/master; autonomous execution never runs in the user's working
tree.

## Alternatives considered

- In-place execution in the user checkout with post-hoc diffing: rejected;
  it cannot isolate concurrent tasks or guarantee a clean merge base.
- Container-only isolation without Git isolation: rejected; container and Git
  isolation are complementary layers, not substitutes.

## Consequences

- The Workspace/Git subsystem must handle dirty repositories, dirty worktrees,
  branch collisions, concurrent file touches, untracked files, process crashes,
  rebase/merge conflicts, stale worktrees, failed checkouts, deleted branches
  and user edits during agent work — these failure modes must be tested.
- Merge happens only through the verification-gated merge queue.

## Security impact

Worktree isolation is one layer of the isolation story alongside Policy Engine
decisions and runner sandboxing (see ADR-0013); a required isolation profile
fails closed when its backend is unavailable (INV-076).

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §18 (Workspace/Git), §19 (Git failure modes)
- `INVARIANTS.md`: INV-005, INV-006
