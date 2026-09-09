# ADR-0004 - Task is the central durable entity

Status: Accepted
Date: 2026-09-09

## Context

Early product thinking explored persistent AI employees and chat-centric UX.
The accepted model centers the user on outcomes and work rather than terminal
windows and chat sessions.

## Decision

`Task` is the central durable business entity of the system — not Agent, Chat
or Session. Agents are assignable executors; sessions are transient execution
details. The Task descriptor carries objective, parent/epic, dependencies,
acceptance criteria, scope, risk, required verification, required review,
artifacts and status.

## Alternatives considered

- Agent-centric model (persistent AI employees as the core entity): rejected as
  the system core; agent profiles exist but Task does not depend on any
  specific agent.
- Chat/session-centric model: rejected; sessions are provider-specific details
  inside an attempt.

## Consequences

- Scheduling may reassign a Task across providers (e.g. Codex → Claude after
  failure) without changing Task semantics.
- Orchestrator, scheduler and reconciler manage work, not provider details.

## Security impact

Not materially specified beyond the accepted decision.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §2.4 (agent-centric vs task-centric), §11 (Task Engine), §14 (Agent Model)
- `INVARIANTS.md`: INV-001, INV-002
