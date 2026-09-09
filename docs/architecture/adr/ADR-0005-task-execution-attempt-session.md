# ADR-0005 - Task != Execution != Attempt != Session

Status: Accepted
Date: 2026-09-09

## Context

A Task may need several executions across different providers, each execution
may need several attempts, and each attempt involves provider-specific sessions.
Conflating these levels would couple durable business state to transient
provider mechanics.

## Decision

Task, Execution, Attempt and Session are distinct durable concepts:

```text
Task AUTH-42
  ├── Execution E1
  │      ├── Attempt 1 / Codex / failed
  │      └── Attempt 2 / Codex / rejected
  └── Execution E2
         └── Attempt 1 / Claude / passed
```

Session is a temporary provider-specific entity inside an attempt. Provider
references are stored as opaque `ProviderRef` / `external_session_ref`, never as
provider-specific fields on the Task.

## Alternatives considered

- Single flattened execution record per Task: rejected; it cannot represent
  cross-provider retries or provider-specific session details without leaking
  provider types into the Task.
- Provider session ID stored on the Task (e.g. `claude_session_id`): explicitly
  rejected as a domain-model violation.

## Consequences

- Task lifecycle (DRAFT → READY → CLAIMED → RUNNING → SUBMITTED →
  AWAITING_REVIEW → VERIFYING → COMPLETED, plus BLOCKED/FAILED/CANCELLED) stays
  independent of provider behavior.
- Only the verification/evaluation pipeline grants COMPLETED; agents expose
  `submit_result()`, never `complete_task()`.

## Security impact

Not materially specified beyond the accepted decision.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §4.4 (provider-specific types), §12 (state machine), §13 (Task ≠ Execution)
- `INVARIANTS.md`: INV-002, INV-009, INV-012, INV-047
