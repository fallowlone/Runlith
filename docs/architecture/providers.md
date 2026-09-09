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

## 2. Conceptual interface

Accepted conceptual shape (SoT §15):

```text
AgentProvider
├── detect
├── capabilities
├── start
├── send
└── cancel
```

> Conceptual interface examples are architectural guidance, not the final
> language-level API.

No Rust trait signature is frozen here. The final `AgentProviderPort` shape,
sync/async boundary, and configuration representation are open design items.

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
(runners.md); exact ordering/race semantics are an open design item and this
document does not resolve TASK-0006 cancellation authority. Timeout layers
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
Process mechanics (environment variables, PATH, working directory, PTY,
stdio framing, shell strategy) are undecided unless already accepted, and
user/model-controlled values must never become unsafe shell interpolation.

## 16. Open Provider Design Items

Unresolved by accepted architecture; listed, not decided:

```text
final AgentProviderPort Rust shape
sync vs async trait boundary
provider installation identity format
provider capability taxonomy
provider-specific extension mechanism
canonical normalized provider-result shape
provider error taxonomy
provider version compatibility policy
provider session identity format
provider availability canonical owner/persistence/freshness
model identity representation
provider/model configuration representation
provider auth-state detection semantics
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
