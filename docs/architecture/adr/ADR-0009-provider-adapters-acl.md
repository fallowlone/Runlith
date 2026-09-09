# ADR-0009 - Provider adapters are anti-corruption layers

Status: Accepted
Date: 2026-09-09

## Context

Claude Code, Codex, OpenCode, Gemini and future runtimes are replaceable
executors with divergent CLIs, sessions and capabilities. Letting any of them
shape the domain model would couple durable Task semantics to a vendor.

## Decision

Provider adapters are anti-corruption layers behind a canonical provider port
(detect, capabilities, start, send, cancel). They translate canonical requests
to provider runtimes and back, and they cannot mutate Task state, write memory
directly, merge, bypass the Policy Engine or decide scheduling. No
provider-specific type (e.g. `claude_session_id` on Task) may enter core domain
semantics; external references travel as opaque `ProviderRef` /
`external_session_ref`. Provider-specific behavior belongs behind capability /
adaptation interfaces, never behind `if provider == "…"` branches in domain code.

## Alternatives considered

- Provider-native domain model with per-provider Task variants: rejected; it
  makes every provider swap a domain rewrite.
- Adapters with direct Task-state write access for efficiency: rejected; it
  violates bounded-context ownership.

## Consequences

- FakeAgent and contract tests precede real providers; every adapter passes the
  shared port contract suite.
- The orchestrator, scheduler and Task Engine remain provider-agnostic.

## Security impact

Adapters sit behind trust boundary TB3 (agentd → Agent CLI) and cannot bypass
Policy Engine authorization; provider outputs remain untrusted data (INV-093).

## Migration / replacement impact

Adding or replacing a provider, runner or backend must not require modifying
unrelated domain business logic (INV-082).

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §4.4 (provider-specific types), §15 (Provider Adapters), §16 (FakeAgent)
- `INVARIANTS.md`: INV-004, INV-012, INV-048, INV-082, INV-093
