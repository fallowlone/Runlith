# ADR-0007 - No direct shell execution from the frontend

Status: Accepted
Date: 2026-09-09

## Context

The React frontend communicates with the system through the Tauri shell, which
could in principle expose arbitrary command execution. Granting the WebView
shell capability would collapse the TB1 trust boundary and let any frontend
compromise — or injected repository content rendered in the UI — become host
compromise.

## Decision

The frontend never receives direct arbitrary shell execution capability.
UI actions travel only as validated requests:

```text
React → Tauri DTO → agentd request
```

Tauri commands only validate the request, call the local client and return
a DTO; they contain no orchestration business logic. The browser pane is a
separate trust zone denied Tauri IPC, filesystem, shell, agentd and secrets
access, and never shares a capability set with the main app WebView.

## Alternatives considered

- Direct shell/process/Git invocation from React: rejected; it bypasses Policy
  Engine decisions and OS isolation.

## Consequences

- Every privileged operation is routable through Policy Engine and isolation
  enforcement (see ADR-0013).
- UI work must define explicit DTO contracts instead of ad-hoc commands.

## Security impact

This decision upholds trust boundary TB1 (WebView → Tauri host) and invariant
INV-003. Repository content is untrusted input and must never gain instruction
or execution authority through the UI.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §4.5 (no business logic
  in Tauri commands), §8.2 (no shell capability), §8.3 (browser trust zone),
  §53 (trust boundaries)
- `INVARIANTS.md`: INV-003, INV-094
