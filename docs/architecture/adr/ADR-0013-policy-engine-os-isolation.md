# ADR-0013 - Policy Engine plus OS isolation

Status: Accepted
Date: 2026-09-09

## Context

Agents execute untrusted repository content, wield powerful tools (shell, Git,
network, providers) and serve mutually distrusting trust zones (WebView, Tauri
host, agentd, CLIs, repository, internet). Prompt-based restrictions alone
cannot contain a model that is instructed — or injected — to misbehave.

## Decision

Security rests on two mandatory layers: an explicit Policy Engine that decides
before privileged execution, and OS/container/VM isolation that enforces at
runtime. Prompt instructions are never the sole security boundary. Minimum
trust boundaries TB1–TB7 (WebView→Tauri host→agentd→Agent CLI→OS, plus agent→
repository, agent→internet, app→plugins/cloud) each get a dedicated threat-model
section. Runners form a replaceable hierarchy (Native for interactive trusted
work, honestly labeled "Trusted Host, not a sandbox"; rootless-preferred
Container for autonomous work with minimal mounts; VM later for high-risk
repositories), and required isolation fails closed when its backend is
unavailable.

## Alternatives considered

- Prompt-only restrictions: rejected as a security boundary.
- Isolation-only without explicit policy decisions: rejected; authorization
  (Policy Engine, before execution) and enforcement (isolation, during
  execution) answer different questions.

## Consequences

- Every privileged/destructive action, browser capability, web fetch/redirect
  hop and remote operation routes through typed Policy Engine constraints.
- Secrets live in OS Keychain/Credential Manager/Secret Service, never SQLite.

## Security impact

This ADR is the core security posture: INV-007, INV-008, INV-013, INV-076,
INV-097 and the TB1–TB7 boundary set all trace to it.

## Migration / replacement impact

Sandbox/runner backends are replaceable behind Runner/Sandbox ports; each
declares the capabilities it actually enforces (INV-077).

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §17 (Runner abstraction), §§52–54 (security model, trust boundaries,
  untrusted repository content)
- `INVARIANTS.md`: INV-007, INV-008, INV-013, INV-072, INV-073, INV-076,
  INV-077, INV-097
