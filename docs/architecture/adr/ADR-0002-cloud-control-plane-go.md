# ADR-0002 - Future cloud control plane uses Go

Status: Accepted
Date: 2026-09-09

## Context

The v1 product is local-first with no cloud requirement. A future cloud/control
server may still be needed later for collaboration, sync or hosted workloads.

## Decision

When/if a cloud backend is built, it uses Go with PostgreSQL and object storage.
No cloud backend is built in v1.

## Alternatives considered

- Rust cloud backend: not selected; Go is preferred for server-side development
  speed and straightforward concurrency in a future control server.
- Building cloud in v1: rejected; cloud is optional for core functionality
  and telemetry is off by default.

## Consequences

- Local IPC contracts must stay transport-independent and versioned so a future
  Go service can interoperate (Protobuf direction, see ADR-0003).
- No cloud work may block or complicate v1 local-first delivery.

## Security impact

None in v1 (no cloud exists). Any future cloud design requires its own threat
model and privacy review; local-first guarantees (see ADR-0014) must not regress.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table), §2.1
- `INVARIANTS.md`: INV-011
