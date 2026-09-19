# Runner — Architecture Contract

> Repository-local contract for the execution Runner: where and how
> executable work runs, what isolation is actually enforced, and how runtime
> evidence flows back. TASK-0064 materializes the bounded provider-neutral
> isolation/enforcement foundation described here; TASK-0069 adds the first
> concrete production backend (macOS Seatbelt), while the full Runner lifecycle
> and additional platform/container backends remain incremental work.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Task lifecycle: [`task-engine.md`](./task-engine.md)
> - Event semantics: [`events.md`](./events.md)
> - Provider contract: [`providers.md`](./providers.md)
> - Test double: [`fake-agent.md`](./fake-agent.md)
> - Decisions: [`adr/ADR-0008-worktree-isolation.md`](./adr/ADR-0008-worktree-isolation.md),
>   [`adr/ADR-0013-policy-engine-os-isolation.md`](./adr/ADR-0013-policy-engine-os-isolation.md)
> - Trust boundaries: [`../security/TRUST_BOUNDARIES.md`](../security/TRUST_BOUNDARIES.md) (TB4)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§7, 17–18,
>   55, G3, ZeroClaw-derived §§1, 8

The Runner answers where/how executable work runs. It never chooses which AI
provider is used — that is routing/Provider business. Runner (process and
environment execution) and sandbox/isolation capability (what is enforced)
are related but distinct concepts: a Runner may use an isolation backend,
and Policy requirements are evaluated against declared enforcement
capabilities, never against a bare `sandboxed = true` boolean unless
architecture later explicitly chooses that model.

## 1. Purpose

The Runner answers:

```text
Where does executable work run?
How is a process started/stopped?
What filesystem/workspace does it see?
What runtime isolation capabilities exist?
What resources are constrained?
What observable execution evidence exists?
```

## 2. Runner abstraction

Accepted abstraction — exactly these, no additions:

```text
Runner
├── NativeRunner
├── ContainerRunner
├── RemoteRunner later
└── VMRunner later
```

## 3. NativeRunner

```text
NativeRunner
=
interactive trusted-host execution
```

The product honestly represents:

```text
Isolation: Trusted Host
```

Critical rule:

```text
NativeRunner
!=
sandbox
```

NativeRunner is never documented as a security sandbox merely because Policy
restrictions exist. It may suit trusted interactive work, but it must not
automatically satisfy a Task requiring stronger isolation. If Policy
requires `ContainerOrBetter` and only NativeRunner is available, the result
is BLOCK / REQUIRE USER ACTION — never silent downgrade (INV-076).

## 4. ContainerRunner

```text
ContainerRunner
=
primary autonomous-work runner direction
```

Preferred rootless. Accepted conceptual mount:

```text
/workspace -> rw
```

Must not ambiently mount:

```text
~/.ssh
~/.aws
~/Documents
/
```

No complete mount policy is invented here. Container never equals perfect
security: capabilities must accurately describe what is actually enforced,
with filesystem isolation, network isolation, process isolation, and
resource limits as distinct capabilities. If network isolation is not
enforced, the Runner must not report that it is.

## 5. RemoteRunner and VMRunner

```text
RemoteRunner
=
later
```

No worker protocol, remote authentication, mTLS, queue, or cloud scheduler
is designed here — RemoteRunner is preserved only as a future Runner
implementation behind the same conceptual boundary.

```text
VMRunner
=
later high-risk / enterprise hardening direction
```

No backend (Firecracker, QEMU, Apple Virtualization, Hyper-V, or other) is
selected.

## 6. Fail-closed isolation

Required isolation fails closed (INV-076):

```text
Task requires: ContainerOrBetter
Container backend unavailable
Result: BLOCKED / REQUIRE USER ACTION
```

Never fallback to NativeRunner silently.

A platform whose production backend enforces mandatory confinement must also
state whether the provider's own authentication mechanism can operate inside
that confinement; a platform that enforces nothing must state that instead.
The two are one decision, and leaving the authentication question unanswered
is not a neutral default — it permits the launch. See
`docs/architecture/providers.md` §9a for the provider-side contract and
TASK-0117 for the drift guard that checks the stated verdict against the
backend's reported enforcement axes.

## 7. Network isolation

```text
filesystem isolation
!=
network isolation
```

A runner/backend declares the capabilities it actually enforces.
Filesystem sandboxing must never be presented as blocking exfiltration.

## 8. Workspace relationship

The Runner consumes an authorized workspace/worktree prepared by
Workspace/Git architecture; it does not own canonical Git semantics. For
autonomous work:

```text
Task
 ↓
worktree
 ↓
Execution
 ↓
Runner
```

The Runner never decides which branch to merge, whether main may be
modified, Task acceptance, or Task completion (ADR-0008; INV-005, INV-006).

## 9. Process execution and evidence

The Runner owns execution mechanics, conceptually: start, observe,
terminate/cancel a process; collect exit status; capture bounded execution
outputs; enforce declared resource/isolation capabilities. No final API
signatures are frozen. The future Runner produces trusted runtime evidence
sufficient to distinguish started, running, exited, cancelled, timed out,
crashed, and isolation-backend-unavailable — no final status enum is created
unless already accepted, and an exact runner-result taxonomy stays open
where unresolved. Resource limits (CPU, memory, disk, time, network,
process) may be enforced where supported; no values are defined and no
Runner is claimed to support every limit — capabilities stay explicit.

## 10. Process success vs Task success

```text
exit code 0
!=
Task COMPLETED
```

Likewise process exit never equals verification passed. The Runner supplies
evidence; Evaluation decides acceptance. Therefore the Runner does not
determine Task completion — and neither does the Provider; completion
belongs to the Evaluation path alone (task-engine.md §13). A Runner failure
is likewise not an automatic Task failure: it feeds Attempt/Execution
orchestration, and only trusted policy/state-machine outcomes later decide
the Task.

## 11. Authority boundaries

Policy decides what is allowed, what approval is required, and what
isolation capability is required; the Runner executes and enforces the
selected runtime profile without defining business/security policy itself.
Provider and Runner compose but never fuse:

```text
Canonical Execution
 |
 +---- Provider Adapter: Codex
 |
 +---- Runner: ContainerRunner
```

Provider handles AI/runtime protocol; Runner handles process/environment
execution. No `CodexContainerRunner` / `ClaudeNativeRunner` core types;
canonical contracts stay separate even if concrete adapter composition
later needs internal integration glue.

G6 uses scoped capabilities as canonical authority (ADR-0026). If named
"security levels" or runner profiles are introduced, they are presets that
compile to explicit capability/isolation requirements. They cannot authorize
an operation or bypass Policy, and a Runner reports only restrictions it can
actually enforce.

OS-specific adapters consume those effective requirements behind this Runner
boundary: product-compatible macOS sandbox mechanisms, Linux
namespace/seccomp/cgroup/filesystem/network mechanisms, and Windows restricted
token/Job Object/AppContainer-equivalent mechanisms as appropriate.

TASK-0064 materializes the provider-neutral enforcement foundation in
`crates/runner`. `IsolationPlan::compile` consumes only already-computed
`EffectiveCapabilities` plus non-authorizing `IsolationRequirements`, requires
the exact `ProcessExecute` capability for the process about to run, and
extracts filesystem/process/network scope without absorbing Git, secret, tool
or external-action authority. Filesystem capabilities for a different managed
workspace fail closed.

`IsolationBackend` returns an `EnforcementReport` bound to the exact compiled
plan and backend identity. The operation boundary rejects stale/mismatched
reports and rejects any backend that cannot prove every required isolation
axis before the process operation is invoked. Filesystem, network and process
enforcement stay independent axes.

`adapters/runner-native` is the first bounded adapter. It is honestly a
`NativeRunner`: `Trusted Host`, with zero OS-sandbox enforcement axes. It can
therefore satisfy only an explicit trusted-host requirement; any required
filesystem/network/process isolation fails closed rather than silently
downgrading. TASK-0064 deliberately selects no concrete macOS/Linux/Windows
sandbox or container backend because that matrix remains an accepted open
design item.

TASK-0069 adds the first production-capable concrete backend,
`adapters/runner-macos-seatbelt`. Autonomous Claude execution on macOS now
requires all three filesystem/network/process axes and enters the provider
through `/usr/bin/sandbox-exec`; it no longer requests Trusted Host. The
backend derives workspace read/write and executable allowlists from the exact
`IsolationPlan`, adds only provider-runtime support roots supplied by the
trusted Claude adapter, keeps the provider-owned `~/.claude` / `.claude.json`
authentication anchors read-only, and currently supports one explicit network
scope: outbound TCP/443. Any other network scope fails before spawn. The
platform runtime baseline imported by Seatbelt is support required for native
process startup, not task authority. On platforms without a concrete backend,
production still selects NativeRunner but the mandatory three-axis requirement
makes execution fail closed before the provider operation.

TASK-0065 now materializes provider-process environment hygiene at the Claude
adapter/composition boundary: inherited environment is cleared, provider-
writable XDG/state/cache/scratch paths live outside managed worktrees, and
unsafe environment preparation fails before provider spawn. This is launch
hygiene, not Runner authorization or an OS sandbox. TASK-0067 composes the
current production provider launch through canonical Policy evaluation and
exact Runner enforcement immediately around provider execution. TASK-0069
changes that production launch from explicit Trusted Host to mandatory
filesystem/network/process confinement and supplies the macOS Seatbelt backend.

## 12. Replacement test

Can NativeRunner be replaced with ContainerRunner without modifying Provider
Adapter business logic? Expected architectural answer: yes — apart from
explicit capability negotiation/composition.

## 13. Open Runner Design Items

Unresolved by accepted architecture; listed, not decided:

```text
full Runner process-lifecycle/result shape beyond the TASK-0064 isolation boundary
RunnerResult taxonomy
process identity format
process lifecycle state model
process-group/child-process termination semantics
PTY vs non-PTY execution
stdout/stderr framing
output size/backpressure policy
environment construction
working-directory enforcement
resource-limit capability taxonomy
user-facing runner profile/preset representation
network-policy enforcement beyond the current outbound TCP/443 Seatbelt scope
runner cancellation ordering with provider cancellation
runner timeout semantics
container backend choice
rootless container backend compatibility
workspace mount implementation
additional platform-specific sandbox backends (Linux/Windows)
RemoteRunner protocol
VMRunner backend
```

## 14. Non-goals

This document does NOT define: container implementation, Docker/Podman
selection, VM backend, remote worker protocol, Scheduler, Policy Engine or
Secrets Broker implementations, Task retry policy, Task cancellation
semantics, Tool Receipt or Artifact Store implementations.
