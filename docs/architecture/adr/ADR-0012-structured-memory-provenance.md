# ADR-0012 - Structured memory with provenance

Status: Accepted
Date: 2026-09-09

## Context

Agents need durable knowledge (project facts, past execution experience,
repeatable procedures, architectural rationale), but conversation transcripts
and session summaries are unreliable as a knowledge store and degrade through
summary-of-summary chains.

## Decision

Memory is a structured subsystem distinct from Context, with four types:
semantic (project facts), episodic (past execution experience), procedural
(how to perform repeatable actions) and decision (ADRs and rationale).
Every memory entry carries provenance: source refs, confidence, created_at,
last_verified_at, supersedes links, staleness and content hash where
applicable. Lifecycle is CAPTURE → NORMALIZE → DEDUPLICATE → LINK → VALIDATE →
USE → REFRESH/SUPERSEDE → ARCHIVE; unbounded append is forbidden and raw
events/files/Git remain the evidence. v1 stores `memories`, `memory_links`,
`entities` and `entity_links` in SQLite; no graph database in v1.

## Alternatives considered

- Transcript-as-memory: rejected; memory is not conversation history.
- Summary-of-summary chains as truth: rejected; raw evidence stays authoritative.
- Graph database (e.g. Neo4j) in v1: rejected; relational tables suffice until a
  graph workload justifies a replaceable adapter.

## Consequences

- Agent-generated annotations and learned notes are untrusted until
  provenance/validation policy promotes them (INV-029).
- Memory backends stay replaceable behind ports without domain rewrites.

## Security impact

Memory entries with trust/sensitivity metadata must respect the same secrecy
rules as other durable state: secrets never enter model context, logs or event
payloads (INV-008).

## Migration / replacement impact

Memory implementations are swappable adapters; a future graph backend would
arrive through the established port, not a domain rewrite.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §§39–42 (Memory Engine, provenance, lifecycle, graph memory)
- `INVARIANTS.md`: INV-029
