# Provider Adapter — Architecture Contract

> Repository-local contract for the AI Provider Adapter: a provider-neutral
> anti-corruption layer between orchestration and replaceable AI runtimes.
> Documents accepted architecture; invents none; implements nothing.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Task lifecycle: [`task-engine.md`](./task-engine.md)
> - Event semantics: [`events.md`](./events.md)
> - Runner contract: [`runners.md`](./runners.md)
> - Test double: [`fake-agent.md`](./fake-agent.md)
> - Decisions: [`adr/ADR-0005-task-execution-attempt-session.md`](./adr/ADR-0005-task-execution-attempt-session.md),
>   [`adr/ADR-0009-provider-adapters-acl.md`](./adr/ADR-0009-provider-adapters-acl.md)
> - Trust boundary: [`../security/TRUST_BOUNDARIES.md`](../security/TRUST_BOUNDARIES.md) (TB3)
> - Provider data flows: [`../privacy/DATA_FLOW_MAP.md`](../privacy/DATA_FLOW_MAP.md) (FLOW-002, FLOW-003, FLOW-004)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§4, 7, 13–16,
>   47, 57, Routing, ZeroClaw-derived §§1, 4–5

The Provider answers which AI/runtime performs the work. It is one of three
independent dimensions — Provider (who), Runner (where/how, see
[`runners.md`](./runners.md)), isolation capability (what is enforced).
`Codex + NativeRunner` and `Codex + ContainerRunner` must be conceptually
possible without changing Task semantics, and replacing `Codex → Claude`
must not require replacing the Runner contract.

## 1. Purpose

The Provider contract answers:

```text
What AI/runtime capabilities are available?
How is provider installation/availability detected?
How is a canonical execution request translated?
How is a provider session started?
How is provider input sent?
How is execution cancelled?
How are provider results/statuses normalized?
What is provider-specific vs canonical?
```

The Provider Adapter is an anti-corruption layer: it translates canonical
requests to provider runtimes and back so no provider shapes the domain
model (ADR-0009).

## 2. Canonical execution seam

The execution half of the conceptual interface is frozen as
`scheduler::ExecutionPort` (ADR-0024 §1) — the provider-neutral seam
the orchestrator injects, one executor per cycle:

```text
WorkItem { task_id, execution_id, attempt_id }   (durable identities only)
  → ExecutionPort::execute
  → Ok(WorkEvidence { summary, evidence_ref, partial })
  → Err(ExecutionProblem::{TimedOut, Cancelled, Failed})
```

`provider_ref()` is opaque identity only; orchestration never branches
on it. There is no `cancel` method on the seam — cancellation reaches
the provider out-of-band (composition-owned signal → executor wait loop
→ owned-tree kill), and provider wiring differs (§15).

The remainder of the conceptual `AgentProvider` shape (detect,
capabilities negotiation, session send, capability taxonomy, routing)
stays open for later gates; G5 freezes the execution half only. No
second Rust trait was created: future real adapters implement this
same seam without touching orchestration.

## 2a. First implementations (not the architecture)

Claude is the first implementation, not the architecture. Two
executors pass the same seam today:

- **FakeAgent** (test double): deterministic per-scenario behavior
  behind the seam via a test-side wrapper; virtual timeouts, no
  process. Remains valid alongside real providers; its scenario matrix
  is provider-specific behavior tests, not the shared contract.
- **ClaudeExecutor** (`provider-claude`): the installed Claude Code
  CLI (argv-only, cwd-bound to the attempt's lease, no shell),
  adapter-local version policy (tested major 2, minor ≥ 1), bounded
  hand-rolled JSON parsing, structured-signal failure mapping with a
  narrow auth-marker fallback. Subscription-native authentication stays
  provider-owned (§7): the adapter never reads, persists, logs,
  forwards, or repairs credentials. Detection answers installation and
  version compatibility only and never probes auth; authentication state
  is a separate pre-launch precondition (§9a).

A future `provider-codex` implements the same trait behind its own
factory closure; orchestration, storage, workspace, and completion
semantics do not change (§13a).

## 3. Provider Adapter owns

```text
installation detection
provider/runtime capability discovery
provider-specific command invocation translation
provider-specific session/process protocol translation
provider-specific input/output normalization
provider-specific cancellation translation
provider-specific error normalization
provider-native authentication handoff where applicable
```

## 4. Provider Adapter forbidden responsibilities

The adapter must NOT:

```text
mutate Task lifecycle
mutate Task graph
write Memory directly
decide scheduling
select itself as the winning provider
grant Task completion
make merge decisions
perform Git merge
bypass Policy Engine
weaken Task scope
weaken acceptance criteria
change required review
change required verification
own canonical Context
own canonical Event Journal semantics
own secrets
decide sandbox/isolation policy
```

In particular: no Task-state writes (INV-004), no direct Memory writes, no
merges, no Policy bypass, no scheduling decisions (ADR-0009).

## 5. Provider neutrality

Core domain semantics must never require provider-specific variants such as:

```text
ClaudeTask
CodexTask
GeminiExecution
OpenCodeReview
```

Provider identity belongs to execution/attempt/routing evidence where
appropriate — never to Task meaning (INV-002, INV-012, INV-059).
Provider-specific behavior lives behind capability/adaptation interfaces,
never behind `if provider == "…"` branches in domain code.

## 6. Provider session

```text
Session
=
temporary provider-specific entity inside Attempt
```

A provider session is not Task, Execution truth, Memory, Context Continuity
truth, or completion evidence (task-engine.md §4; INV-019).
Provider-native conversation state may serve optimization but remains
non-authoritative application state. External references travel only as
opaque `ProviderRef` / `external_session_ref`.

## 7. Authentication boundary

Where a provider CLI already owns authentication (Claude Code, Codex, other
CLIs), AI Engineering OS leaves provider-native authentication
provider-owned where possible: it must not scrape or extract credentials
merely because the CLI is authenticated. The adapter may invoke the
authorized provider runtime but never becomes owner of the provider
credential. Actual secrets follow the Secrets Broker path (reference →
broker → trusted execution boundary), never prompts or canonical provider
requests — except where explicitly required at a trusted execution boundary.

## 8. Capabilities

Provider-neutral capability concept describing facts relevant to:

```text
reasoning/tool capabilities
context limits
supported execution modes
structured output
tool support
availability/runtime features
local vs external inference characteristics
```

No final closed capability enum is invented. Benchmark quality is never
encoded as hard capability. Kept separate:

```text
capability
!=
quality score / routing preference / provider reputation / current capacity
```

## 9. Availability

Accepted provider-neutral availability vocabulary — exactly these statuses,
no additions, no removals:

```text
AVAILABLE
MISSING
DEGRADED
QUOTA_EXHAUSTED
RATE_LIMITED
POLICY_BLOCKED
INCOMPATIBLE
UNKNOWN
```

Clarified:

```text
provider availability
!=
Task lifecycle state
```

and:

```text
provider availability
!=
Task completion
```

Availability is observable provider/runtime state. Exact canonical
ownership, persistence, and freshness semantics are open design items where
current architecture does not define them precisely.

## 9a. Authentication state

Authentication state is **not** availability and never enters the §9
vocabulary. The original reasoning holds: whether a provider is installed and
version-compatible is an installation fact, while whether its owner is signed
in is not. `AUTH_REQUIRED` remains absent from §9.

What changed is only the second half of that reasoning. The earlier contract
also held that auth could not be observed at all without burning quota or
risking an interactive prompt, and therefore had to surface as execution-time
failure evidence. For the current Claude CLI that is measurably false:
`claude auth status` answers from local state with machine-readable output,
consumes no quota, and prompts for nothing. The vocabulary decision stands on
its own merits; the unobservability premise does not.

Accepted provider-neutral authentication vocabulary — exactly these states,
no additions, no removals:

```text
AUTHENTICATED
UNAUTHENTICATED
UNKNOWN
```

Binding rules:

- Authentication is a **precondition verified before launch**, not a failure
  discovered during one. A provider launch requiring authentication refuses
  closed when the state is `UNAUTHENTICATED`, and the refusal names the
  operator action rather than reporting an opaque provider exit.
- `UNKNOWN` is not permission to proceed. It refuses like `UNAUTHENTICATED`
  and says which of the two it could not distinguish.
- The runtime never performs, completes, repairs, or automates a login, and
  never presents a credential prompt from inside an execution. Authentication
  is an interactive act performed by the operator, outside any sandboxed
  launch, against the provider's own flow.
- A probe obeys the §2 detection discipline: one fixed executable, fixed
  argv, no shell, bounded capture, short timeout. A probe that could consume
  quota, mutate provider state, or trigger an interactive prompt is not a
  permitted probe.
- The adapter still never reads, persists, logs, forwards, or repairs a
  credential (§7). Observing *that* a provider is signed in is not observing
  *what* it is signed in with.

Rationale and external corroboration: the surveyed orchestrators and the
Agent Client Protocol all separate an interactive login phase from the
working launch, and all represent an unauthenticated provider as a typed
condition rather than an exit code — see
`external-research/studies/2026-09-19-provider-authentication-boundary.md`.
Runlith needs the separation more sharply than they do, because it is the
only one of them that genuinely confines the working launch.

## 10. Errors

Provider/runtime failures are eventually normalized into provider-neutral
failure evidence (no final error enum invented here), for example:

```text
missing executable
quota exhaustion
rate limiting
network failure
provider timeout
malformed provider output
process crash
```

Critical:

```text
Provider failure
!=
Task FAILED automatically
```

Provider failure affects Attempt/Execution orchestration; the Task may retry
or migrate to another provider (task-engine.md §12; INV-023).

## 11. Output trust

Provider/model output is untrusted, even from an authenticated local or
external CLI (INV-093). A provider response cannot directly mark Task
COMPLETED, mint authoritative domain events, authorize privileged actions,
override Policy, rewrite architecture, or weaken acceptance criteria.
Trusted application layers validate outcomes; only the Evaluation pipeline
grants COMPLETED.

## 12. Data flow

Per [`../privacy/DATA_FLOW_MAP.md`](../privacy/DATA_FLOW_MAP.md), the
adapter preserves observability sufficient to distinguish local execution,
external inference, data leaving the device, our cloud receiving data, and
the provider receiving data (FLOW-002/003/004, PRIV-010). The full Data
Flow Map is not duplicated here.

## 13. Replacement test

Architecture fitness question: can one Provider Adapter be replaced without
changing unrelated Task, Memory, Policy, Workspace, or Runner business
logic (INV-027, INV-082)? If no — boundary violation, unless explicitly
approved by architecture review. Provider implementations must not hard-code
`Docker`, `Podman`, native shell, or VM mechanics into provider-domain
semantics; that belongs to the Runner side.

## 13a. Shared contract suite and second-provider recipe

Proof mechanism (ADR-0009 consequence, AGENT.md §6):
`crates/execution-contract` is the reusable provider-neutral contract
suite against `ExecutionPort`. It asserts only seam-promised behavior —
success yields submittable bounded evidence (never completion),
failure yields canonical `Failed`, malformed output never becomes
success, timeout yields canonical `TimedOut`, provider refs stay opaque
— using deterministic executors only (FakeAgent scenarios; Claude
fixture executables; never live provider access). Cancellation,
cwd-binding, partial results, and Task mutation are deliberately out of
the shared contract: not on the seam (see the crate docs for why each
is excluded and where it is proven instead).

FakeAgent and Claude fixture executors both pass it from
provider-owned test targets; the contract crate depends only on
neutral crates, and no domain crate depends on a provider in
production. Adding a provider validates mechanically: new adapter crate
implementing `ExecutionPort` → dev-depend on the contract crate →
build success/failure/malformed/timeout executors → invoke the shared
suite → register the factory in the `agentd` composition root. No
Task/Storage/Workspace/orchestration/completion/journal/IPC change.

## 14. Composition and contract size

Only the application/composition root knows concrete implementations;
domain components never instantiate adapters themselves:

```text
ExecutionService
 |
 +-- AgentProviderPort -> CodexAdapter
 |
 +-- RunnerPort -> ContainerRunner
```

A port belongs to the consumer requiring the capability — no giant global
`EverythingProvider` or `RuntimeManager` spanning providers, Git, policy,
memory, routing, execution, and security. Per INV-069, stable contracts
stay small and provider-neutral; capability discovery and narrow operations
beat speculative mega-interfaces. Provider-specific extensions (optional
capabilities, opaque adapter configuration, extension contracts) may appear
where justified; the exact mechanism is open.

## 15. Coordination notes

Cancellation spans two levels — Provider owns provider/session cancellation
semantics, Runner owns process/environment termination mechanics
(runners.md). Accepted G5 wiring (ADR-0024 §9): cancellation reaches the
provider out-of-band, never through a seam method — the `agentd`
composition root owns one `CancelFlag` per execution, signals it from a
later `cancel` IPC, and the executor's wait loop kills only its owned
child tree (direct kill + reap + process-group kill; never by name).
`CancelFlag` and `LiveProcessTracker` live in `provider-claude` for now
even though they are provider-neutral in shape: no extraction until a
second provider proves the abstraction. The tracker feeds
workspace-cleanup liveness through the neutral `OwnerLiveness` seam
(execution id only — no provider types cross it). Timeout layers
(provider response timeout, process timeout, Task retry policy, workflow
deadline) are distinct concepts; no single global timeout field is defined.
Provider CLI/runtime versions change independently of our application, so
detection must eventually support missing/incompatible/degraded states; a
complete version-range policy is not created here. Secrets stay outside
model context and ordinary contracts (§7). Large outputs use
references/artifacts conceptually rather than living inside Task, journal,
or session metadata; no ArtifactStore is created. Provider result (what the
runtime returned) and Runner evidence (what the trusted environment
observed) are different concepts and must not merge — a provider "command
succeeded" claim never replaces trusted runtime/tool evidence. FakeAgent or
provider output cannot fabricate a valid trusted Tool Execution Receipt;
receipt generation belongs to the trusted runtime/tool boundary (INV-072).
No provider may gain stronger OS access by emitting text; capability
escalation goes through the Policy Engine with required approval/isolation.
The canonical G6 authority is the effective capability set from ADR-0026.
Provider adapters may translate that set into provider-native flags/modes, but
must never decide ALLOW/DENY, grant capabilities, broaden scope, approve an
operation, or reinterpret a hard denial. Provider-native permission/sandbox
features are defense in depth only; an unrepresentable mandatory restriction
must fail closed or be enforced by the selected Runner/isolation backend.
Process mechanics (environment variables, PATH, working directory, PTY,
stdio framing, shell strategy) are undecided unless already accepted, and
user/model-controlled values must never become unsafe shell interpolation.

TASK-0065 closes H7 at the provider-process environment boundary. The Claude
adapter clears inherited environment, rebuilds a narrow launch allowlist and
binds provider-writable XDG config/data/state/cache plus scratch/temp paths to
an application-owned runtime root outside the managed worktree. The production
composition derives that root beside application state and keys the provider
environment by the existing managed worktree identity, so restart/recovery does
not invent a second workspace identity.

Provider-native authentication remains provider-owned. For the current Claude
CLI, the real `HOME`/profile and the POSIX login principal `USER` are together
the explicit authentication anchor: the CLI resolves its stored credentials
through the login principal, so a child given `HOME` without `USER` reports
`Not logged in` and exits with no diagnostic (TASK-0101). Credentials are
neither copied nor scraped; `USER` carries no credential material and is
already implied by `HOME`'s path. Ambient API-key/cloud/Git/SSH/provider/plugin
environment variables are not inherited. `--safe-mode` disables user/project
customizations as defense in depth. These mechanics grant no authority:
Policy/approval/Runner remain canonical and unsupported mandatory isolation
still fails closed.

## 16. Open Provider Design Items

Unresolved by accepted architecture; listed, not decided. The G5
execution half is closed (ADR-0024): `ExecutionPort` shape, canonical
`WorkEvidence` / `ExecutionProblem` vocabulary, provider-owned auth,
adapter-local version policy, out-of-band cancellation, and the shared
contract suite are decided. Still open:

```text
full AgentProviderPort Rust shape (detect/capabilities/send halves)
sync vs async trait boundary
provider installation identity format
provider capability taxonomy
provider-specific extension mechanism
provider version compatibility policy (beyond the Claude-first line)
provider session identity format
provider availability canonical owner/persistence/freshness
model identity representation
provider/model configuration representation
provider cancellation race semantics
streaming output contract
provider tool-call normalization
provider usage/cost evidence contract
```

## 17. Non-goals

This document does NOT define: real Claude/Codex/OpenCode/Gemini
integrations, provider credentials, OAuth flows, Model Router, Scheduler,
Policy Engine or Secrets Broker implementations, Task retry policy, Task
cancellation semantics, Tool Receipt or Artifact Store implementations.
