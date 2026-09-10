# G4 Git Failure Matrix

> TASK-0044 — verification evidence. Architecture authority lives in
> ADR-0008, ADR-0023, and `docs/architecture/workspace.md`; this document
> maps every canonical failure mode to concrete executed tests and records
> the verdict. It decides nothing itself. Explicit boundary: **this task
> changed no production source** — new coverage lives in two test binaries
> plus this document; `git status` shows no `src/**` modification from
> TASK-0044.
>
> Prior evidence: `G4_WORKSPACE_GIT_CONTRACT.md` (TASK-0040),
> `G4_GIT_WORKTREE_CREATION.md` (TASK-0041),
> `G4_WORKSPACE_DURABILITY_RECONCILIATION.md` (TASK-0042),
> `G4_DIFF_SAFE_CLEANUP.md` (TASK-0043).

## Scope

Complete G4 Workspace/Git failure surface per ADR-0023 and
`docs/architecture/workspace.md` §§ Failure Taxonomy / Crash /
Reconciliation / Cleanup Safety, plus G2/G3 regression preservation.
Out of scope (deferred by design, verified as absent rather than tested
for behavior): merge/rebase/cherry-pick, remote Git, providers (G5),
Policy Engine/sandbox (G6).

## Architecture Authority

- ADR-0008 (isolated worktree/branch per autonomous execution).
- ADR-0023 (Execution-owned lease, immutable OID base, metadata-resolved
  default, Git-authoritative facts, dirtiness/cleanup/crash rules,
  `workspace.created/removed` taxonomy, argv-only CLI adapter, INV-145).
- `docs/architecture/workspace.md`, `docs/architecture/events.md` §6,
  INV-001/002/004/005/006/009/010/025/026/027/070/076/145.

## Matrix Method

Audit-first: production source (`crates/workspace`,
`adapters/git-cli/src`, `crates/app-services/src/workspace_service.rs`,
`adapters/storage-sqlite/src/workspace_store.rs`) was inspected before
any test was written; the baseline suite (489 passed, 0 failed) was
executed first. New tests use **real temporary local Git repositories**
for Git semantics; narrow `GitPort` wrappers over the real `GitCli`
inject only the deterministic fault (stale status read, live owner) for
crash/race seams. No production code was modified to make any test pass:
4 initial failures were all test-authoring bugs (wrong dirt-rank
expectation, non-canonicalized path compare, `--allow-empty` commit with
empty evidence layer, `branch -D` on a checked-out branch, stale-read
sequencing), each fixed in the test.

One scenario deserved a production-behavior ruling (not a fix):
external deletion of the managed branch followed by cleanup of a clean
at-base worktree finalizes `Removed`. Probes proved this loses no data
(the branch tip was the recorded base, still reachable; every mutation
stayed non-forced and observed) while the same deletion with a unique
commit fails closed via Git's own non-forced refusal. Both shapes are
pinned by tests below.

## Repository Inspection Failures

| Scenario | Test |
|---|---|
| valid repository | `worktree_creation::…success…`, `failure_matrix::repository_path_with_unicode…` |
| not a repository / missing path | `worktree_creation::not_repository_and_missing_paths_are_typed` |
| file path | `safety_regression::detached_head_and_file_paths_observe_as_invalid` |
| bare repository | `worktree_creation::bare_repository_fails_explicitly` |
| path with spaces | `worktree_creation::…leaves_primary_untouched` (repo `dir with spaces`), all service suites |
| path with Unicode | `failure_matrix::repository_path_with_unicode_inspects_and_creates` |
| linked worktree (`.git` file) | `failure_matrix::linked_worktree_dotgit_file_counts_as_repository` |
| non-standard default name | `worktree_creation::protected_branch_uses_semantics_not_literals`, service `trunkline` tests |
| default metadata unavailable | `failure_matrix::default_branch_without_metadata_fails_closed_without_guessing`, service `indeterminate_default_branch_creates_without_guessing` |

Fail-closed properties proven: indeterminate metadata → `GitBackendFailure`
naming no branch (no `main`/`master` guess, asserted on the detail text);
creation with indeterminate metadata still succeeds for `agent/`-scoped
branches (availability without a safety trade); no mutation on any
inspection failure.

## Revision Failures

| Scenario | Test |
|---|---|
| exact OID | `failure_matrix::resolve_revision_matrix`, service `base_revision_request_resolves…` |
| branch/ref/HEAD/short-OID | `failure_matrix::resolve_revision_matrix` |
| nonexistent / blank / tree object | `failure_matrix::resolve_revision_matrix` (all `InvalidRevision`) |
| non-commit base at creation | `worktree_creation::invalid_revision_creates_nothing`, service `invalid_and_nonexistent_revisions_allocate_nothing` (no lease row, no journal) |

Base persisted is always the full OID, never `HEAD`/branch/default tip.

## Creation Failures

| Scenario | Test |
|---|---|
| branch collision (incl. unexpected revision) | `worktree_creation::branch_collision…`, `failure_matrix::branch_at_unexpected_revision…` (ref OID unchanged, no worktree) |
| path collision / foreign content | `worktree_creation::worktree_path_collision…`, matrix `allocated_with_foreign_target_content_never_replays` (bytes preserved) |
| protected/default as managed | `worktree_creation::protected_branch_uses_semantics_not_literals` |
| invalid BranchRef / dash-led | `safety_regression::{dash_led…,space_branch…}` |
| managed-root escape / nesting | `safety_regression::target_inside_primary_checkout_is_refused`, service `managed_root_pins…`, `foreign_worktree_reference…`, `symlinked_managed_root…` |
| symlink escape | `failure_matrix::symlinked_target_is_collision_not_escape` |
| second lease, same Execution | service `repeated_calls…` (`StorageFailure`, one `created`) |
| second call, healthy lease | `AlreadyReady`, no event (same test) |
| partial branch without worktree | matrix `allocated_with_branch_at_unexpected_revision_never_replays` → `CheckoutFailed`, stays `Allocated`, zero events |
| Git failure post-conditions | `mutation_failure` path (re-observed flags) + reconciliation tests below |

## Path / Ownership Failures

Hostile `ExecutionId` (`../../../tmp/evil-;-$(x)-…`) derives an
`agent/`-scoped branch with exactly one `/` and no `..`, and a
root-contained worktree (`hostile_execution_id_derives_safe_branch_and_path`).
Foreign `worktree_ref` planted via storage is `OwnershipMismatch` and
touches nothing (`foreign_worktree_reference_is_rejected_never_used`).
All Git invocations remain fixed-executable + argv (rg sweep: no
`sh -c`/`bash -c`/`cmd /C`; `--` separates paths; absolute argv paths).

## Concurrent Workspace Failures

E1→W1 + E2→W2, same repo/base: distinct ids/paths/branches; same-file
edits coexist with no bleed and no global lock; primary unaffected; diffs
scoped per lease (`concurrent_workspaces_isolate_same_file_edits`).
Removing W1 with dirty W2 leaves W2 byte-identical (tracked + untracked),
branch and status unchanged
(`cleanup_of_one_lease_preserves_dirty_sibling_byte_identical`).
No merge is ever performed (`conflicting_branches_coexist_without_merge_or_rebase`).

## Diff / Status Failures

Clean/tracked/staged/untracked/deleted/HEAD-advanced/rename/scoping/
spaces+Unicode/absent/detached covered in `diff_cleanup.rs` (adapter) and
`workspace_diff_cleanup.rs` (service); matrix adds mixed
committed+working layers with baseline pinned after the primary advances
(`diff_mixed_state_scoped_to_base_after_primary_advances`) and the
untracked-ranks-above-tracked rule
(`failure_matrix::worktree_status_ranks_untracked_above_tracked`).
Diff mutates nothing (index/tree/refs/lease/Task), proven by
`diff_does_not_mutate_git` + service non-mutation tests.

## Cleanup Failures

Tracked/staged/untracked/mixed dirt, ownership mismatch, root escape,
moved worktree, branch mismatch, protected/default, external, and unmerged
branches all refuse without force (`diff_cleanup.rs` + service matrix).
Unique unmerged commits survive (`unmerged_managed_branch_survives…`).
Active owners refuse before any mutation (`live_owner_rejects_cleanup…`,
real-backend wrapper). Forbidden primitives absent from production
(rg sweep: no `reset --hard`, `clean -fd`, `checkout -- .`,
`restore .`, auto-stash, `worktree remove --force`, `branch -D`,
`rm -rf`, shell deletion — names appear only in comments/docs/tests).
TOCTOU user edit deterministically preserved via the re-check seam
(`user_edit_between_check_and_removal_is_preserved`).

## Crash / Reconciliation Failures

Creation Crash A/B/C (`workspace_service.rs`: resumed / recognized /
ambiguous-no-replay) re-proven plus partial-shape variants in the matrix
(.....
[truncated 5079 chars]