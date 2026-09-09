# ADR-0019 - ADR Materialization Is Gate-Scoped

Status: Accepted
Date: 2026-09-09

## Context

The architecture spans far beyond the current gate: security internals,
context engine details, memory backends, remote workers, cloud
collaboration. Writing implementation-specific ADRs for all of it during G0
would freeze guesses as false certainty and create migration cost when real
implementation constraints arrive. But leaving future architecture
undocumented invites silent invention by implementation agents. A rule is
needed for what gets an ADR, when, and what governs the gaps meanwhile.

## Decision

Architecture material uses three governance classifications. These are
governance labels, not Task lifecycle states:

- **ACTIVE DECISION** — constrains implementation now or before the next
  affected gate. Requires an accepted ADR before that gate opens. No
  implementation task may invent a missing architecture decision merely
  because its gate has begun.
- **DEFERRED ARCHITECTURE TARGET** — accepted direction for a later gate
  whose detailed implementation decision is intentionally not yet
  activated. It needs no speculative ADR now; it stays constrained by
  existing invariants, stays visible in architecture/coverage
  documentation, and must receive an ADR before the first implementation
  task that materially depends on it.
- **RESEARCH / HYPOTHESIS** — not implementation authority. It must not
  silently become implementation behavior, and an implementation agent may
  not cite a research note as authorization for new core architecture.
  Promotion requires explicit architecture review/ADR when appropriate.

Gate-scoped activation never weakens invariants: deferring a detailed ADR
(e.g. Resource Engine internals until G9) never suspends the invariants
already governing that area (e.g. INV-040). ADR timing controls when
implementation-specific choices freeze, not whether existing invariants
bind.

This ADR must not be used to reclassify an already active decision as
deferred to bypass review — classification changes require architecture
review and provenance.

Every gate audit verifies: which architecture decisions become active in
the next gate, and do accepted ADRs exist for them. This is the repeatable
governance check.

## Alternatives considered

- Write every foreseeable ADR during G0: rejected; false certainty plus
  migration cost, and most would rot before their gate.
- No coverage tracking until gates arrive: rejected; gaps would be
  discovered mid-implementation, when invention pressure is highest.

## Consequences

- The canonical coverage matrix lives in
  [`COVERAGE.md`](./COVERAGE.md): every significant area ends classified,
  none unclassified.
- G1 can open with no active decision lacking ADR coverage; deferred
  targets do not block it for want of future detailed ADRs.
- Deferred items get ADRs on a just-in-time schedule tied to gates, not
  paperwork for its own sake.

## Security impact

Deferral is not relaxation: deferred security-relevant targets (Policy
internals, Secrets Broker detail, audit finalization) remain bound by
INV-007, INV-008, INV-013, INV-076 and the TB1–TB7 boundary set until
their dedicated ADRs land. Research material grants no security authority.

## Migration / replacement impact

Classification changes are versioned governance edits with provenance;
activating a deferred target means writing its ADR before dependent
implementation, never rewriting history to pretend it was always active.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §79 (Development
  Gates), §94 (Definition of G0 complete), §98 (Architecture change
  process)
- `INVARIANTS.md`: INV-043, INV-111
- [`COVERAGE.md`](./COVERAGE.md)
