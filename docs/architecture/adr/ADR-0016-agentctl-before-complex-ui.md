# ADR-0016 - Core usable through `agentctl` before complex UI

Status: Accepted
Date: 2026-09-09

## Context

Building a rich GUI before the core works risks cementing backend design around
UI convenience and hiding orchestration flaws behind presentation. Early
face-to-face validation must target the engine, not the chrome.

## Decision

Core functionality becomes fully usable through the `agentctl` CLI before the
GUI grows complex. Tauri does not define backend architecture; the UI is a thin
client over agentd contracts, and no orchestration logic moves into the
frontend to meet a UI deadline.

## Alternatives considered

- UI-first development with CLI added later: rejected; an untestable core
  behind a polished UI inverts the verification order.
- Letting Tauri commands accumulate business logic for UI velocity: rejected
  (see ADR-0007).

## Consequences

- Every core capability needs a CLI-invocable, contract-tested path, which
  doubles as the headless/automation interface.
- GUI milestones gate on CLI-demonstrated core behavior plus evaluation evidence.

## Security impact

A CLI-first core keeps the privileged surface small and contract-shaped while
the WebView attack surface is still maturing.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §2.3 (UI-first vs core-first)
- `INVARIANTS.md`: INV-094
