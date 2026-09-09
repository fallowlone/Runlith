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
