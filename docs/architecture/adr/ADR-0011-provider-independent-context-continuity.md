# ADR-0011 - Provider-independent context continuity

Status: Accepted
Date: 2026-09-09

## Context

Executions outlive any single provider context window, and Tasks must survive
provider switches (e.g. Claude → Codex) without losing authoritative state.
Provider-native features (Codex/Claude/OpenCode compaction, Astra-style memory)
are useful optimizations but are owned by vendors and cannot be the durability
story.

## Decision

Context continuity is durable application functionality with three levels:
L1 provider context (vendor optimization), L2 execution continuity (our epochs,
checkpoints, working notes, searchable task history), L3 durable project/task
memory (decisions, architecture, failures, skills, repository knowledge).
Work is organized in bounded Context Epochs; rollover follows a strict
algorithm (budget monitor → structured checkpoint → durable transaction →
index old epoch → verify persistence → only then create the new epoch).
The hard rule is NO CHECKPOINT → NO ROLLOVER. History is searched
(`history.search` then `history.read(exact_ref)`), never bulk-loaded.

## Alternatives considered

- Relying solely on provider-native compaction/continuation: rejected; it is a
  provider optimization, not durable application state (INV-019, INV-022).
- Unbounded context growth until hard-limit failure: rejected; the budget
  monitor checkpoints before the soft limit.

## Consequences

- A Task can migrate between providers mid-flight with authoritative state
  intact (INV-023).
- Prompt-cache layout discipline (stable prefix before dynamic content) and
  separate accounting of token cost vs attention load.

## Security impact

Not materially specified beyond the accepted decision.

## Migration / replacement impact

Not materially specified beyond the accepted decision.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §§32–36 (continuity engine, rollover, three levels, history search,
  prompt caching)
- `INVARIANTS.md`: INV-019, INV-020, INV-021, INV-022, INV-023
