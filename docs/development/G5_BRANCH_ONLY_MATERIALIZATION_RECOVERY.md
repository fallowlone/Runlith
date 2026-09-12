# G5 Branch-Only Materialization Recovery (TASK-0057, B-RS2)

> TASK-0057 fixes only B-RS2: a daemon crash after managed branch creation
> but before worktree registration/finalization. B-OL remains TASK-0058.
> G5 remains in progress and G6 must not start.

## 1. Root cause and durable crash shape

The atomic execution entry can commit Task `RUNNING`, the Execution, the
reserved Attempt identity, the managed-identity reservation and an
`Allocated` `WorkspaceLease`. `git worktree add -b` can then create the
managed branch before the daemon is killed. The durable/live shape is:

- same Task and Execution remain authoritative;
- zero Attempts, reserved Attempt still present;
- managed identity reservation owned by that Execution;
- `WorkspaceLease = Allocated` with recorded repository/base/branch/path;
- managed branch exists exactly at the recorded base OID;
- target path is absent and Git has no worktree registration for it or the
  managed branch;
- no `workspace.created` fact exists.

Before TASK-0057 reconciliation treated any branch-without-worktree shape
as ambiguous, so product `resume` could never progress without external
branch deletion.

## 2. Recovery rule

Branch-only continuation is deliberately narrower than "branch exists".
For a current v2 lease, recovery proves all of the following before any
mutation:

1. the persisted lease owner is the requested Execution;
2. its persisted branch is exactly the v2 branch derived for that owner;
3. the durable managed-identity reservation is owned by the same Execution;
4. no different lease owns that repository/branch;
5. no different lease owns the persisted worktree path;
6. the persisted base OID is still known in the repository;
7. the managed branch tip equals that exact base OID;
8. the persisted managed path still passes containment validation;
9. the target path is absent (including no empty directory, foreign checkout
   or symlink occupying it);
10. Git does not register the target path as a worktree;
11. Git does not register the managed branch in any other worktree.

Every missing or contradictory proof fails closed. Packed refs are observed
through Git plumbing/worktree porcelain, never `.git/refs` filesystem
assumptions.

The only recovery mutation is continuation using the proven existing branch:

```text
git worktree add <managed-path> <existing-managed-branch>
```

The adapter independently rechecks the exact base/branch/path conditions and
postconditions. No `branch -D`, branch reset, branch recreation, forced
cleanup or ownership guessing is used. The ordinary `Allocated -> Ready`
finalization then emits the existing `workspace.created` fact exactly once.

If the daemon dies after the worktree attachment but before finalization, a
fresh service observes the already-complete Git state and finalizes through
the pre-existing recognized-Git reconciliation path. This makes a second
crash in the recovery window idempotently recoverable.

## 3. Resume dispatch and TASK-0056 ownership

`execution resume` now performs the same workspace classification read-only
before claiming live ownership. A deterministically ambiguous branch/path/
ownership state therefore returns the existing typed
`CONFLICT`/`CORRUPT_STATE` taxonomy synchronously and spawns no background
job. The background path repeats the authoritative proof before mutation, so
the preflight is diagnostic rather than a TOCTOU correctness boundary.

TASK-0056 ownership remains the execution guard: `run` and `resume` share one
process-local insert-if-absent owner, losers cannot replace the winner, and
release remains owner-token-scoped. A racing resume that observes the
winner's newly-created Attempt while that winner is still live is classified
as the same typed `CONFLICT`, preserving the live-owner contract.

## 4. RED evidence

Actual pre-fix evidence was captured before/against a controlled pre-fix
classifier snapshot, not inferred from the final code:

- RED-1: exact-base branch-only state failed with
  `CheckoutFailed: ... ambiguous partial Git state; refusing blind replay`.
  In the isolated baseline run
  `branch_only_at_recorded_base_recovers_through_product` exited 101.
- RED-2: on the same durable shape, external deletion of only the managed
  branch made the strand recover as the existing untouched/crash-A path;
  `manual_branch_deletion_recovers_same_strand` passed in the same baseline.
  This proves the branch-only artifact was the obstruction.
- RED-3: a branch advanced away from the recorded base remained refused in
  that baseline; `branch_only_unexpected_tip_stays_fail_closed` passed as a
  fail-closed safety keeper.
- Dispatch RED: before the synchronous B-RS2 preflight, a resume of the
  unexpected-tip shape returned a background job instead of refusing before
  acceptance. The new dispatch keeper failed on that behavior and is green
  after the fix.

The baseline was an isolated copy of the working tree with only the new
branch-only `reconcile_existing` arm disabled; it did not mutate the active
repository.

## 5. GREEN and adversarial evidence

- `crates/app-services/tests/branch_only_recovery.rs`: 14/14 green with real
  Git and file-backed SQLite: exact-base continuation, manual-deletion
  contrast, unexpected-tip refusal, packed refs, foreign worktree holder,
  empty/non-empty/foreign-checkout/symlink targets, missing/foreign identity
  reservation, second crash after attachment, cleanup, and unchanged fresh
  materialization.
- Deterministic exact-base branch-only keeper: 50/50 repeated rounds green.
- Shipped `agentd` + `agentctl` SIGKILL seam: 10/10 repeated rounds green.
  The test intercepts only `git worktree add -b`, creates the real branch at
  the recorded base, kills the daemon before registration/path completion,
  restarts a fresh daemon, and resumes the same Execution/reservation through
  the production control plane.
- Packed-ref recovery: 10/10 repeated rounds green.
- Unexpected-OID refusal: 10/10 repeated rounds green.
- Branch-only concurrent resume keeper: 50/50 rounds on one shared
  `ExecutionComposition`; exactly one owner continues, losers are typed
  conflicts, and journal/Attempt creation remains exactly once.
- `bins/agentd/tests/live_ownership.rs`: 5/5 green; existing TASK-0056
  ownership keepers remain intact.
- `bins/agentd/tests/execution_recovery.rs`: 14/14 green, including a fresh
  composition restart for branch-only recovery and synchronous refusal of an
  ambiguous branch tip.
- Case-fold identity suite: 8/8 green on the current case-insensitive macOS
  host, including the existing E-CASE/E-case and E/S/E-2FS protections.
- Existing non-B-OL old-format `Allocated` lease reconciliation keeper: 1/1
  green. The known e2-/h2-prefixed legacy classification defect remains B-OL
  and is intentionally not repaired here.

The restart keeper verifies the same reserved Attempt identity is consumed by
canonical Attempt creation, exactly one `attempt.created`, exactly one
`workspace.created`, Task reaches `VERIFYING`, and the primary checkout does
not move. The second-crash keeper attaches the existing branch while the lease
is still `Allocated`, then opens a fresh service and proves recognized-Git
finalization emits `workspace.created` once.

## 6. Provider neutrality

Workspace recovery remains below provider execution. No Claude-specific
condition was added to the workspace service or Git adapter. Fresh targeted
contract runs were green:

- FakeAgent `ExecutionPort` contract: 1/1;
- Claude `ExecutionPort` contract: 1/1;
- `app-services` normal dependency tree has no `provider-claude` edge.

Real Claude Code was available as `2.1.268 (Claude Code)`. The env-gated real
provider adapter smoke passed 1/1 and returned the expected marker. A fresh
product-level smoke through shipped `agentd` + `agentctl` also passed: Attempt
`A-T57C` concluded `SUCCEEDED`, the Task remained `VERIFYING`, status reported
`live=false`, the provider ref stayed `claude-code`, and the isolated runtime
was removed after shutdown. The deterministic shipped-binary B-RS2 crash test
still uses the fixture provider so the SIGKILL seam is controllable.

## 7. Canonical verification and process-test incident log

Final static verification is green:

- `cargo fmt --all -- --check`;
- `cargo check --workspace`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

Every full-workspace run is recorded, including failures:

| Run | Mode | Passed | Failed | Ignored | Result |
| --- | --- | ---: | ---: | ---: | --- |
| 1 | default parallel, before final regression fixes | 712 | 2 | 1 | failed: stale Crash-C expectation + resume race taxonomy |
| 2 | default parallel, after fixes | 714 | 0 | 1 | green |
| 3 | default parallel, after fixes | 712 | 2 | 1 | process-test interference: concurrent daemon keepers; global `pgrep` saw 4 daemons |
| 4 | serialized (`-j 1 -- --test-threads=1`) | 714 | 0 | 1 | green |
| 5 | serialized (`-j 1 -- --test-threads=1`) | 714 | 0 | 1 | green |

Run 1 produced two actionable findings and both were fixed: the old workspace
service keeper now uses a genuinely ambiguous unexpected tip, and a live
resume winner keeps loser classification at `CONFLICT` even if the winner has
already created its Attempt.

Run 3 exposed the existing process-test isolation problem rather than a
B-RS2 product failure: `real_process_matrix` counts every workspace
`target/debug/agentd` with a global `pgrep`, while other shipped-binary tests
also launch isolated daemons; the ownership keeper simultaneously observed
cross-test daemon interference during its kill/restart seam. No daemon was
left after the test run. Two subsequent fully serialized workspace runs were
identical and green (714/0/1 each). TEST-H/process-test isolation remains
outside TASK-0057.

## 8. Version, documentation and deferrals

Product semver stays `0.1.0`; development identity is
`0.1.0-dev.task0057`, task `TASK-0057`, gate `G5`. ADR-0025,
`workspace.md` and `ipc.md` contain additive B-RS2 recovery semantics; the
TASK-0055 audit history is unchanged.

Deferred and unchanged: B-OL (TASK-0058), general F-RA outside workspace
reconciliation, post-Attempt G11 recovery, TEST-H process-test isolation and
all G6 security architecture.

G5 remains IN PROGRESS. G6 must not start. Next task is TASK-0058 (B-OL).
