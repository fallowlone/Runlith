# Runner — Architecture Contract

> Repository-local contract for the execution Runner: where and how
> executable work runs, what isolation is actually enforced, and how runtime
> evidence flows back. Documents accepted architecture; invents none;
> implements nothing.
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

## 12. Replacement test

Can NativeRunner be replaced with ContainerRunner without modifying Provider
Adapter business logic? Expected architectural answer: yes — apart from
explicit capability negotiation/composition.

## 13. Open Runner Design Items

Unresolved by accepted architecture; listed, not decided:

```text
final RunnerPort Rust shape
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
isolation-profile representation
sandbox capability representation
network-policy enforcement integration
runner cancellation ordering with provider cancellation
runner timeout semantics
container backend choice
rootless container backend compatibility
workspace mount implementation
filesystem/symlink escape handling
platform-specific sandbox matrix
RemoteRunner protocol
VMRunner backend
```

## 14. Non-goals

This document does NOT define: container implementation, Docker/Podman
selection, VM backend, remote worker protocol, Scheduler, Policy Engine or
Secrets Broker implementations, Task retry policy, Task cancellation
semantics, Tool Receipt or Artifact Store implementations.
