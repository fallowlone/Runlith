# ADR-0015 - shadcn/ui is a primitive layer, not the design system

Status: Accepted
Date: 2026-09-09

## Context

The desktop client uses React + TypeScript + Vite under Tauri 2 and needs UI
building blocks fast, without inheriting a generic AI/SaaS look or letting a
third-party library define product identity.

## Decision

shadcn/ui is used strictly as a primitive component layer. Product components
go through the project's own design system layered on top, with durable product
truth in PRODUCT.md kept separate from visual truth in DESIGN.md.

## Alternatives considered

- shadcn/ui as the de-facto design language: rejected; UI primitives must not
  define the product design system (INV-024).
- Merging product and visual decisions into one document: rejected; they evolve
  on different cadences and need separate review.

## Consequences

- Frontend tasks follow PRODUCT.md for durable product truth and DESIGN.md for
  visual language, and verify loading, empty, error, blocked,
  permission-required, running and success states where relevant.
- Design-quality gates (static/a11y detectors, visual regression, optional LLM
  critique) attach to the project's system, not to the primitive library.

## Security impact

Not materially specified beyond the accepted decision.

## Migration / replacement impact

The primitive layer stays swappable: product components depend on the project's
design-system contracts, not on shadcn/ui internals.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table), §8
  (Tauri Desktop)
- `INVARIANTS.md`: INV-024, INV-034
