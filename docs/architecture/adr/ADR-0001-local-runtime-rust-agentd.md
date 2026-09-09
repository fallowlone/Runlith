# ADR-0001 - Local runtime is separate Rust `agentd`

Status: Accepted
Date: 2026-09-09

## Context

The product needs a local backend/runtime that owns orchestration business logic,
while the Tauri desktop host stays a thin native shell. An early variant considered
a Go daemon; the accepted baseline selects Rust for the local daemon.

## Decision

The local runtime is a separate Rust `agentd` process, structured as a modular
monolith with Ports & Adapters / Hexagonal Architecture. Only the `agentd`
composition root knows concrete implementations; domain crates never construct
adapters themselves.

## Alternatives considered

- Go local daemon: rejected for the local runtime because Tauri already uses Rust,
  the runtime is a systems program (PTY/process/filesystem/IPC/sandbox integration),
  crates can be shared between the Tauri host, daemon and CLI, and Rust gives
  stronger compile-time guarantees for security-critical orchestration.
  Go remains the choice for the future cloud control server.
- Microservices: rejected; local architecture is a modular monolith.
- Merging orchestration into the Tauri process: rejected; the Tauri host must not
  own orchestration business logic.

## Consequences

- One native toolchain across Tauri host, daemon and CLI.
- Bounded contexts communicate via explicit application APIs, typed events and
  versioned contracts; no cross-module table reads, no shared mutable singletons.
- `agentd` owns local orchestration business logic (INV-094 keeps UI/MCP/IDE
  adapters thin).

## Security impact

Security-critical orchestration benefits from Rust compile-time guarantees;
Policy Engine plus OS/container/VM isolation remain the enforcement layers
(see ADR-0013). No new network surface is introduced by this decision alone.

## Migration / replacement impact

Component replaceability is an explicit fitness rule: SQLite, providers, retrieval,
renderers, runners, UI clients, scheduler and memory implementations must each be
replaceable without rewriting unrelated parts (INV-027, INV-082).

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §2.1 (Go vs Rust), §5 (system structure), §7 (Composition Root)
- `INVARIANTS.md`: INV-025, INV-026, INV-027, INV-069, INV-082
