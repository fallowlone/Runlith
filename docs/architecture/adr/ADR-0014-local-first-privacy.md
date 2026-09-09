# ADR-0014 - Local-first privacy

Status: Accepted
Date: 2026-09-09

## Context

The product handles source code, credentials and proprietary project knowledge.
Users must be able to run the full core loop on their own machine without
accounts, network dependencies or data exfiltration.

## Decision

The core product is local-first and fully usable offline: no account is required
for core functionality, cloud is optional, telemetry is off by default
(opt-in), and the user can delete all app-owned local project data. Secrets
are held in OS credential stores, not in SQLite. Any future cloud
synchronization is opt-in and independent of local engines.

## Alternatives considered

- Cloud-required core with local cache: rejected; it violates INV-011.
- Opt-out telemetry: rejected; the default is off.

## Consequences

- All v1 architecture (SQLite, local runners, local inference options) must
  assume no network and no account.
- Features that move data off-device (doc inference, sync, external benchmarks)
  must be privacy-observable, showing what leaves the device and to whom.

## Security impact

Local-first narrows the default attack surface to the device; every
off-device flow becomes an explicit, reviewable exception rather than the norm.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §10 (Storage)
- `INVARIANTS.md`: INV-008, INV-011, INV-014, INV-015, INV-058, INV-130
