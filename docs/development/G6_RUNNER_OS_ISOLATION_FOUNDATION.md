# G6 Runner and OS Isolation Foundation (TASK-0064)

> Implementation/evidence record for TASK-0064.
>
> Development version: `0.1.0-dev.task0064`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`.

## Canonical scope and acceptance

TASK-0064 implements the bounded foundation named by the canonical queue:

- capability-to-runner enforcement using already-computed
  `EffectiveCapabilities`;
- deterministic provider-neutral isolation plans;
- replaceable isolation backend/report contracts;
- exact process authority enforcement adjacent to the privileged process
  operation;
- independent filesystem/network/process enforcement axes;
- fail-closed behavior when required enforcement is missing or unavailable;
- a bounded NativeRunner adapter that is honestly `Trusted Host`, not a
  sandbox.

Acceptance from `planning/tasks/queue.yaml` is therefore: required isolation
cannot silently downgrade; filesystem/network/process restrictions remain
distinct; platform adapters consume canonical effective authority.

Out of scope: selecting or implementing a concrete container/macOS/Linux/
Windows sandbox backend, RemoteRunner/VMRunner, provider HOME/XDG/state/scratch
construction and H7 closure (TASK-0065), Security Audit persistence
(TASK-0066), integrated production-path G6 adversarial wiring (TASK-0067), G6
closure (TASK-0068), cloud/remote worker isolation or UI.

## Authority and enforcement boundary

Canonical flow implemented by `crates/runner`:

```text
Policy / approval
  -> EffectiveCapabilities
  + exact launch executable
  + policy-derived IsolationRequirements
  -> IsolationPlan::compile
  -> IsolationBackend::enforce
  -> exact-plan EnforcementReport
  -> verify backend identity + required isolation axes
  -> privileged process operation
```

The Runner never evaluates policy, grants authority or interprets provider
permissions. `IsolationRequirements` describe required mechanical restriction
strength only; they are not a second authorization model.

`IsolationPlan::compile` rechecks exact `ProcessExecute(executable)` authority
for the process about to run. It extracts only runner-owned filesystem,
process and network capabilities; Git mutation, secret access, tool invocation
and external-action authority stay with their existing operation owners.
Filesystem capability referring to another managed workspace is rejected.

## Capability-to-isolation compilation

The compiled plan contains:

```text
managed workspace identity
exact launch executable
filesystem read roots
filesystem write roots
allowed process identities
allowed network destinations
required filesystem/network/process enforcement axes
```

Collections are deterministic (sorted/deduplicated). The backend report embeds
the exact plan plus backend identity and only the axes it actually enforced.
Before the operation callback can run, the trusted boundary rejects:

- missing exact process authority;
- cross-workspace filesystem authority;
- backend unavailability;
- backend identity mismatch;
- stale or widened plan reports;
- any missing required filesystem/network/process axis.

This makes the current operation boundary the enforcement point rather than a
reusable preflight token. It does not claim to solve filesystem symlink/race
atomicity; THR-021 remains open until a concrete filesystem sandbox backend can
couple those restrictions to OS operations.

## Platform behavior

`adapters/runner-native` implements the first bounded adapter:

```text
RunnerKind: Native
Label: native-trusted-host
Filesystem sandbox enforcement: NO
Network sandbox enforcement: NO
Process sandbox enforcement: NO
```

It succeeds only when isolation requirements explicitly allow trusted-host
execution and exact process authority exists. Requiring any isolation axis
causes fail-closed refusal; there is no silent fallback.

The canonical architecture lists future product-compatible mechanisms for
macOS, Linux and Windows, but the per-OS backend matrix is explicitly an open
design item. TASK-0064 therefore does not choose Docker/Podman, Seatbelt,
namespace/seccomp/cgroup, restricted token/Job Object/AppContainer or any
equivalent.

## Process adjacency and TOCTOU

`enforce_process_operation` accepts current `EffectiveCapabilities`, compiles
the exact plan, obtains backend enforcement evidence, verifies it and invokes
the process operation inside the same call. No bearer authorization object is
returned to callers. Tests prove stale/widened backend reports cannot be reused
and the operation callback remains uncalled on authorization/backend/isolation
failure.

Concrete filesystem symlink/race atomicity and platform process containment
remain backend-specific open work; this task records that limit rather than
claiming preflight solves it.

## Interactions with existing boundaries

- **Approvals / Policy:** only canonical effective authority produced by the
  existing G6 path is consumed; approval state itself is not accepted.
- **Secrets:** runner plans contain no secret bytes and do not replace the
  TASK-0063 exact SecretRef/target delivery boundary.
- **Workspaces:** the runner consumes one managed workspace identity and rejects
  filesystem capability belonging to another workspace. It does not own Git
  worktree lifecycle, branch/merge authority or G5 recovery.
- **Providers:** provider-native sandbox/permission controls remain defense in
  depth and cannot broaden the plan. TASK-0064 adds no provider-specific core
  type.
- **Recovery:** no durable Runner state/schema/event is added. Existing G5
  Execution/Attempt/workspace recovery ownership is unchanged.

## TASK-0065 / H7 boundary

TASK-0064 intentionally does not change Claude/provider HOME, XDG variables,
inherited environment, authentication/session directories, writable provider
state or scratch paths. H7 (`data/sessions.db` in a managed worktree) remains
owned by TASK-0065 after this runner authority foundation exists.

Likewise, production-wide wiring of Policy/approval/Secrets/Runner enforcement
for all privileged operation owners is TASK-0067. Current production Claude
execution must not be described as OS-sandboxed by TASK-0064 alone.

## Targeted acceptance evidence

Initial completed implementation checks before canonical completion metadata:

```text
cargo test -p runner -p runner-native
10 passed; 0 failed; 6 suites

cargo clippy -p runner -p runner-native --all-targets --all-features -- -D warnings
PASS

cargo test -p policy -p secrets -p runner -p runner-native -p provider-claude -p app-services
325 passed; 0 failed; 1 ignored; 34 suites

cargo check --workspace
PASS (90 crates compiled in that run)

cargo check -p runner -p runner-native --target x86_64-pc-windows-msvc
PASS (12 crates compiled)
```

The 10 runner tests cover deterministic compilation, exact process authority,
cross-workspace refusal, independent isolation axes, stale/widened report
refusal, backend-unavailable refusal, operation adjacency, NativeRunner
trusted-host success and mandatory-isolation downgrade refusal.

An initial `cargo fmt --all -- --check` before applying rustfmt reported only
formatting differences in newly added files. After `cargo fmt --all`, the check
is green; this was not a behavioral/product failure.

## Final pre-merge verification

Before canonical queue/version promotion, the completed implementation/docs
tree passed the full mandatory suite:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS

cargo test --workspace --no-fail-fast
798 passed; 0 failed; 1 ignored; 92 suites; 98.60s

cargo check -p runner -p runner-native --target x86_64-pc-windows-msvc
PASS
```

No TEST-H/process-hermeticity flake reproduced in this pass. The mandatory
suite was then rerun after canonical queue/version promotion on the completed
tree:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS (4 crates compiled in the final cached run)

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS (no issues)

cargo test --workspace --no-fail-fast
798 passed; 0 failed; 1 ignored; 92 suites; 97.66s

cargo check -p runner -p runner-native --target x86_64-pc-windows-msvc
PASS
```

TEST-H did not reproduce in either full pre-merge pass. This evidence record
is followed by one final exact mandatory suite on the unchanged completed tree;
that final run is the merge-acceptance evidence and is intentionally not
rewritten with timing-only output afterwards.

## Post-merge verification

Pending merge and mandatory verification on `main`.

## G5 regression status

G5 remains `CLOSED`. TASK-0064 does not change workspace lease semantics,
branch/merge ownership, provider result interpretation, cancellation/recovery
contracts or Task completion authority. Full workspace verification remains
required before merge and after merge.

## Deferred findings / remaining G6

N-1, N-2, F-SCAN, TEST-H, TEST-H2, Git `commondir`, H9 and H7 remain deferred
unless separately owned by their canonical tasks. TASK-0064 does not reinterpret
or close them.

After TASK-0064 is accepted in `main`, the next canonical task is TASK-0065 —
Provider Environment Isolation and H7 Closure, target
`0.1.0-dev.task0065`, recommended GPT-5.6 Sol / High unless the canonical
queue/docs change before it starts. TASK-0065 is not started here.
