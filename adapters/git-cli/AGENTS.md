# Git CLI - Derived Navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived navigation under [adapters](../AGENTS.md); canonical policy: [AGENT.md](../../AGENT.md).

## Source map

| Concern | Source |
| --- | --- |
| Port implementation and injected owner liveness | [lib.rs](src/lib.rs), [workspace::GitPort](../../crates/workspace/src/port.rs), [OwnerLiveness](../../crates/workspace/src/liveness.rs) |
| Command runner, config admission, repository identity | [runner.rs](src/runner.rs), [TrustedCommonGit](../../crates/workspace/src/trusted.rs) |
| Worktree creation, attachment, postconditions | [create.rs](src/create.rs) |
| Repository, branch, status and ancestry observations | [observe.rs](src/observe.rs) |
| Committed and working-tree diff evidence | [diff.rs](src/diff.rs) |
| Worktree and branch removal | [cleanup.rs](src/cleanup.rs) |

## Implemented boundaries

- [run_git](src/runner.rs) supplies executable plus argv, clears the environment, disables
  system/global config discovery and neutralizes hooks, fsmonitor, credential helpers and external diff.
  Git is non-interactive with closed stdin; inherited Git overrides do not pass through.
- [Repository boundary validation](src/runner.rs) walks to the discovered `.git`, validates linked
  worktree backreferences and compares canonical common-directory identity with `TrustedCommonGit`.
  This validation runs at operation time, including status, diff and cleanup.
- [Config admission](src/runner.rs) scans local and enabled per-worktree config without includes;
  executable filters/diff drivers, includes and selected core redirections fail closed.
- [Runner bounds](src/runner.rs) cap each output stream at 1 MiB and child runtime at 30 seconds;
  excess output or timeout is an error. Diagnostics are bounded to 500 characters plus truncation marker.
- [Diff evidence](src/diff.rs) separates committed changes from working changes, bounds entries
  using [workspace limits](../../crates/workspace/src/port.rs), and records truncation explicitly.
- [Creation](src/create.rs) verifies branch/base/path preconditions and observes postconditions;
  option-like managed branch names are refused by [argv validation](src/runner.rs).
- [Cleanup](src/cleanup.rs) rechecks clean status, attached branch and registration before removal.
  It uses non-forced `worktree remove` and `branch -d`; dirty work and unmerged branches are preserved.
  Failed removal never escalates to force, reset, clean or prune.
- [Missing worktree paths](src/cleanup.rs) produce `WorktreeMissing`; lifecycle reconciliation
  belongs to the caller, while an absent branch reports `AlreadyAbsent`.

## Existing verification targets

Run from the repository root; package identity is [git-cli](Cargo.toml).

| Scenario | Invocation / source |
| --- | --- |
| Metadata redirection, config helpers, hostile argv | `rtk cargo test -p git-cli --test safety_regression` - [tests](tests/safety_regression.rs) |
| Create and attach worktrees | `rtk cargo test -p git-cli --test worktree_creation` - [tests](tests/worktree_creation.rs) |
| Typed failure paths | `rtk cargo test -p git-cli --test failure_matrix` - [tests](tests/failure_matrix.rs) |
| Diff and conservative cleanup | `rtk cargo test -p git-cli --test diff_cleanup` - [tests](tests/diff_cleanup.rs) |
| Local ancestry proof | `rtk cargo test -p git-cli --test ancestry` - [tests](tests/ancestry.rs) |
| Injected execution liveness | `rtk cargo test -p git-cli --test liveness_binding` - [tests](tests/liveness_binding.rs) |

Integration targets share [temporary Git repository fixtures](tests/common/mod.rs).
Runner output-bound and timeout checks are also in `rtk cargo test -p git-cli --lib`.
