# Project Charter — AI Engineering OS

> Product direction, scope and non-goals. This file answers *why* and *what*;
> *how the system is divided* lives in [`ARCHITECTURE.md`](./ARCHITECTURE.md),
> non-negotiable rules in [`docs/architecture/INVARIANTS.md`](./docs/architecture/INVARIANTS.md).

## Purpose

AI Engineering OS is a **local-first control plane for durable AI-assisted
software engineering**.

Claude Code, Codex, OpenCode, Gemini and future runtimes are **replaceable
executors**. The product's value is not another window onto those tools — it is
the durable layer above them: tasks that survive restarts, plans that adapt
with evidence, context assembled just in time, memory with provenance,
verification before delivery, policy before execution, and routing that learns
what works for each repository.

## Who it is for

Developers and small engineering teams who delegate real implementation work to
AI agents and need that work to be **tracked, verified, recoverable and
private by default** — on their own machine, without a cloud account.

## User outcome

The user manages **outcomes, work and verification** rather than terminal
windows, chat sessions and individual agents:

```text
User intent → Plan → Durable Task Graph → Scheduler/Reconciler →
Claude / Codex / OpenCode → Verification → Human gate → Ship
```

A task survives model switches, context rollover, provider quota exhaustion,
process crashes, UI restarts and handoffs. An agent never declares "done" —
the system proves readiness through evidence.

## Core differentiators

- **Durable AI team** — tasks persist across providers, sessions and crashes.
- **Verified delivery** — tests, review, security and acceptance evidence gate
  completion; reliability is measured, not claimed.
- **Adaptive model/context routing** — the system learns which
  provider/model/context strategy fits each repository and task type.
- **Agent Firewall** — security as product (policy + isolation), not warning text.
- **Context continuity** — provider-independent epochs, checkpoints and history
  enable very long tasks untied from any single context window.


## Product differentiation and prioritization

AI Engineering OS must always have a crisp answer to **why it exists** and why a
user should choose it instead of using Claude Code, Codex, OpenCode, Gemini or
another provider harness directly.

The distinction is structural:

```text
Provider harnesses
→ execute work inside one provider/runtime/session model

AI Engineering OS
→ durable provider-neutral control plane around replaceable executors
   with Task/Execution/Attempt state, policy, isolation, verification,
   continuity, cross-provider handoff, memory, recovery, attention management,
   background routines and later team/enterprise governance
```

A feature is strategically weak when its main value can be reproduced by simply
opening another coding-agent terminal. The product should win on the layer that
single-provider tools do not own well: **coordination, reliability, continuity,
verification, safety and outcome management across providers and time**.

### 1% improvement principle

When we adopt an idea already demonstrated by a competitor or adjacent product,
feature parity is not the final target. The adopted capability must improve at
least one meaningful dimension without regressing critical ones. Useful
improvement dimensions include:

- reliability and recovery;
- correctness and verification quality;
- provider neutrality and portability;
- safety, isolation and least privilege;
- continuity across sessions/providers;
- latency, token/resource efficiency and operational cost;
- usability and human-attention cost;
- observability, auditability and explainability;
- privacy and configurability;
- team/enterprise governance.

"1%" is a minimum-improvement mindset, not a requirement that every advantage
be literally measurable as one percent. Where measurement is practical, define
a metric; where it is not, define the concrete qualitative improvement. This
principle does **not** authorize random scope growth or cosmetic differentiation.

### Pareto / high-leverage principle

Use the Pareto principle as a planning heuristic: a minority of capabilities
usually drives a majority of product value. Identify the approximate **20% of
problems/capabilities that can drive roughly 80% of user value, reliability,
differentiation or business value**, then invest a disproportionate share of
discretionary engineering effort there.

This is not a rigid law. Correctness, security, durable-state integrity,
migrations, recovery and verification remain mandatory even when they are not
visible product features.

Practical decision rule:

```text
1. Protect correctness, security and durable state first.
2. Identify the highest-leverage user/product bottlenecks.
3. Prefer deep excellence in differentiating workflows over broad shallow parity.
4. For competitor-inspired work, state how our outcome is meaningfully better.
5. Defer low-leverage polish/speculation until the core advantage is clear.
```

Before committing to a major capability, the project should be able to answer:

> **What important job can AI Engineering OS perform more reliably, continuously
> or efficiently than using individual coding-agent products separately?**

## Non-negotiable principles

- Local-first; cloud and telemetry are opt-in only.
- `Task != Execution != Attempt != Session`; Task is the central durable entity.
- Provider-neutral core: no provider-specific type in domain semantics.
- Context is derived disposable state; prompts are never the system of record.
- Prompt instructions are never the only security boundary.
- Evidence over claims: completion requires verification, not self-report.

## Explicit non-goals for v1

Not building in v1 (architecture must allow adding later without a core rewrite):

- another multi-terminal GUI; generic AI chat app; fancy agent avatars.
- a Claude-specific, Codex-specific or any single-provider product.
- a chat-session database pretending to be project state.
- microservices for the local runtime (modular monolith instead).
- full Event Sourcing in v1 (relational state + append-only Event Journal instead).
- cloud-required or telemetry-required operation; cloud sync; remote fleet.
- an autonomous CTO; unrestricted autonomous host execution.
- UI-first architecture (core becomes usable through `agentctl` first).
- Tournament Mode, Consensus Mode, Shadow Agents, Semantic Conflict Detection.
- voice orchestration, dictation, image generation, plugin marketplace,
  microVM runner, Neo4j, Kafka/NATS, Redis without concrete need.

## References

- Architecture map: [`ARCHITECTURE.md`](./ARCHITECTURE.md)
- Decisions: [`docs/architecture/adr/INDEX.md`](./docs/architecture/adr/INDEX.md)
- Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§0–3, 80, 83–85, 100
