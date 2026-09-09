# ADR-0010 - Context is derived disposable state

Status: Accepted
Date: 2026-09-09

## Context

Model context windows are bounded and provider-managed, while authoritative
project state (Task DB, files, Git, events, memory, decisions, artifacts) is
durable. Treating the prompt as the system of record would make compaction,
rollover and provider switches lossy.

## Decision

Model context is a disposable materialized view, fully rebuildable from
authoritative sources. Loss of a prompt never means loss of business state.
The Context Engine — equal in importance to the Task Engine — owns context
planning, token budgeting, retrieval, navigation, deduplication, stale-result
pruning, compaction, security filtering, provider rendering, cache planning and
observability, and owns none of scheduling, execution, persistent state or UI.
Management follows the funnel: don't load → just-in-time retrieval →
deduplicate → prune stale output → structured state over raw history →
relevant memory only → compact history → prompt caching → serialization
optimization. TOON/compact formats are a last-layer renderer choice, never the
canonical format.

## Alternatives considered

- Prompt-as-record with provider compaction as persistence: rejected; provider
  compaction is an optimization, never the persistence mechanism (INV-022).
- Whole-repository packing as the default context strategy: rejected; default
  construction uses structural just-in-time retrieval (INV-115).

## Consequences

- Context plans (required/selected/deferred/rejected, budget, cache boundaries,
  renderer, reasons, source hashes) are persisted per execution and explainable
  in the UI ("why did this file enter context?").
- Canonical formats stay fixed (domain: Rust structs; persistence: SQLite;
  IPC: Protobuf; tool schema: JSON Schema; docs: Markdown); model-facing
  renderers (Markdown, JSON, compact JSON, CSV, TOON, future) are chosen per
  model/task benchmark.

## Security impact

Security filtering is a Context Engine responsibility; secrets never enter
context (INV-008), and repository content stays untrusted data (INV-038).

## Migration / replacement impact

Retrieval engines (SQLite FTS5/BM25, optional embeddings, AST/symbol index) and
renderers are replaceable behind ports without touching domain semantics.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §§23–31 (Context Engine, principles, materialized view, retrieval),
  §§37–38 (serialization, TOON)
- `INVARIANTS.md`: INV-008, INV-016, INV-017, INV-018, INV-022, INV-030,
  INV-031, INV-038, INV-115
