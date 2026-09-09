# ADR-0006 - v1 relational state plus Event Journal; staged path to Event Sourcing

Status: Accepted
Date: 2026-09-09

## Context

Full Event Sourcing is the strategic long-term direction (reproducibility,
time-travel debugging, durable replay, projection rebuilds, distributed workers,
deep audit), but the v1 domain model, events and bounded-context boundaries are
still stabilizing, and premature Event Sourcing would sharply raise migration
costs and development complexity.

## Decision

- v1 persistence: SQLite relational state plus an append-only Event Journal plus
  a separate Security Audit — not full Event Sourcing.
- Every state-changing transaction updates relational state and appends the
  journal record atomically.
- All significant domain changes already use stable typed event records with
  correlation/causation IDs, schema versions and timestamps, so selected bounded
  contexts can migrate to event-sourced persistence incrementally.
- Event Sourcing is adopted per bounded context only when that context gains
  from replay, reconstruction, temporal queries, distributed processing or
  stronger audit guarantees — never for architectural fashion.

## Alternatives considered

- Full Event Sourcing in v1: rejected as premature given model instability.
- Plain relational state without a journal: rejected; it would foreclose replay,
  debug history and analytics and complicate the later migration.

## Consequences

- v1 ships replay, debug history and analytics without full event-sourced
  complexity.
- Migration path: relational + journal → stable domain events + projections →
  event-sourced selected contexts → full Event Sourcing only where justified.

## Security impact

Security-relevant transitions are covered by the separate Security Audit;
durable security/audit evidence stays independent of model-visible receipts
(INV-073).

## Migration / replacement impact

This ADR is itself the migration plan: journal schema discipline
(versioned typed events) is what makes the staged migration possible without
rewriting the application.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §2.2 (Event Sourcing), §10.2 (state + journal)
- `INVARIANTS.md`: INV-010, INV-044
