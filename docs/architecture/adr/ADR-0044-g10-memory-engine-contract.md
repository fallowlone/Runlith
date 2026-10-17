# ADR-0044 — G10 Memory Engine contract

- Status: **Accepted** (TASK-0180; G10 plan ratified by owner `go`)
- Date: 2026-10-17
- Gate: G10
- Supersedes/extends: [ADR-0012](./ADR-0012-structured-memory-provenance.md) (decision unchanged; this ADR pins normative shapes)

## Problem

ADR-0012 decided structured memory with provenance but fixed no normative
record shape, lifecycle transition table, deduplication identity, staleness
rule, or context-eligibility predicate. Without a frozen vocabulary,
TASK-0181 through TASK-0185 would each invent their own memory shape — the
same class of drift ADR-0042/ADR-0043 prevented in G9. Memory also touches
two hard invariants: agent-generated notes are untrusted until validated
(INV-029), and secrets never enter model context, logs, or event payloads
(INV-008).

## Decision

### 1. MemoryRecord (normative fields)

| field | type contract |
|---|---|
| `id` | opaque string, `mem_` + 26-char Crockford Base32 (ULID shape, time-sortable). ULID crate not mandated. |
| `kind` | closed v1 enum: `Semantic`, `Episodic`, `Procedural`, `Decision`. Unknown kinds fail closed (M-1). |
| `content` | bounded UTF-8 text (max 16384 chars). The fact itself, never a transcript or summary chain. |
| `content_hash` | `sha256:<64 lowercase hex>` over canonical content bytes. |
| `source_refs` | non-empty list of typed refs (`TaskRef`, `AttemptRef`, `GitRef`, `DecisionRef`, `EventRef`, `FileRef`), each revision-bound. Bare unbound strings refused (M-2). |
| `confidence` | `u16` per-mille, 0..=1000. Relevance/score never mutates confidence. |
| `sensitivity` | ADR-0042 levels (`Public`/`Project`/`Private`/`Restricted`/`Secret`). `Secret` content capture is refused outright (M-3); secrets are never stored as memory content. |
| `trust_state` | closed enum: `Proposed`, `Confirmed`, `Contradicted`, `Stale`, `Superseded`, `Retired` (see §3). |
| `anchors` | list of `AnchorFingerprint { anchor_kind, ref, fingerprint }` binding memory to code/task/decision revisions it describes. May be empty only for kind `Episodic` (M-4). |
| `created_at` / `last_verified_at` | Unix millis UTC. `last_verified_at >= created_at` always. |
| `expires_at` | optional Unix millis UTC. Expired memory is context-ineligible (M-5). |
| `supersedes` | optional `MemoryId` of the record this one replaces. |

### 2. Storage shape (v1)

SQLite tables `memories`, `memory_links` (`supersedes`, `derives_from`,
`contradicts`, `related`), `entities`, `entity_links` per ADR-0012 §42.
No graph database, no vector database, no embeddings in v1 (M-6). Raw
events/files/Git remain the evidence; memory never replaces them (M-7).
Provider-native memory files are import/view inputs only, never canonical
state (M-8).

### 3. Trust lifecycle (normative transitions)

ADR-0012 lifecycle maps to trust-state transitions:

```text
CAPTURE      -> Proposed
VALIDATE     -> Proposed  -> Confirmed     (trusted validator only, M-9)
             -> Proposed  -> Contradicted  (with contradicting source refs)
REFRESH      -> Stale     -> Confirmed     (revalidation, updates last_verified_at)
             -> Confirmed -> Confirmed     (revalidation)
ANCHOR DRIFT -> Proposed|Confirmed -> Stale (anchor fingerprint mismatch, M-10)
SUPERSEDE    -> Confirmed|Stale -> Superseded (successor must be Confirmed, M-11)
ARCHIVE      -> any non-Retired -> Retired (terminal)
```

`Contradicted`, `Superseded`, and `Retired` are ineligible history: retained,
retrievable by exact id, never served as current. All other transitions
refuse with a typed error. No unbounded append: capture runs NORMALIZE and
DEDUPLICATE before insert (§4).

### 4. Deduplication identity

Dedup key = `sha256(kind || content_hash || primary_source_scope)`.
A byte-identical re-capture under the same key returns the existing id and
persists nothing new (idempotent); the same id with a different payload
refuses as a conflict (M-12).

### 5. Context eligibility

A record may enter model context iff: `trust_state == Confirmed`, not
expired, `sensitivity != Secret` (structurally guaranteed by M-3), and no
anchor is stale. Eligibility is computed by the domain predicate; the store
never invents it, and selection admission (ADR-0042) still applies on top.

### 6. Error taxonomy

`InvalidInput`, `UnknownKind`, `UnboundSourceRef`, `SensitivityRefusal`,
`UntrustedPromotion` (non-validator or author attempting VALIDATE),
`StaleAnchor`, `AlreadySuperseded`, `Conflict`, `NotFound`,
`BackendFailure`. Errors are seam-local and typed; no panics on input.

## Alternatives considered

- Transcript/summary-chain as memory: rejected (ADR-0012; M-7).
- Vector DB / embeddings by default: rejected for v1; keyword + provenance
  retrieval first, replaced later only behind the port with measured need.
- Graph database: rejected for v1 (ADR-0012); relational link tables suffice.
- Agent self-validation: rejected (INV-029); author and validator must differ.
- Storing SECRET content with redaction on read: rejected; capture refuses
  (M-3), so no read path can leak what was never stored.

## Consequences

- New pure crates `memory` (package `memory-domain`) and `memory-contract`
  (shared conformance suite), following `continuity`/`continuity-contract`.
- SQLite migration `0034`; IPC protocol V9 additive; V1–V8 gates unchanged.
- Retention durations stay policy-owned (G6/G14); G10 builds enforcement
  mechanics only. Entity-graph expansion stays conditional (roadmap
  ENTITY_GRAPH), not a v1 dependency.

## Security impact

INV-008 enforced at capture (M-3) and eligibility (§5); INV-029 enforced by
validator-identity check (M-9); INV-082 preserved — backend swappable behind
`MemoryStorePort` without domain changes. Sensitivity/trust/freshness remain
separate axes, never merged.

## Migration / replacement impact

Additive only: new tables, new crates, new protocol version. Existing
contexts without memory behave identically. A future graph/vector backend
arrives through `MemoryStorePort`, not a domain rewrite.

## References

- [ADR-0012](./ADR-0012-structured-memory-provenance.md); Source of Truth
  §§39–42 and G10 reinforcements; INV-008, INV-029, INV-082;
  [ADR-0042](./ADR-0042-g9-context-engine-contract.md) (sensitivity levels,
  selection admission).
