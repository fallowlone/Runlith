# Re-audit

getzep/graphiti — temporal knowledge-graph framework (episodes →
entities/edges with validity windows; invalidate-don't-delete;
hybrid retrieval) behind Zep's managed context-graph platform.

- repo: https://github.com/getzep/graphiti
- reviewed_revision: `964bb11d1e7020e3cc67fab244268b0736b6c219`
  (main, 2026-10-10T15:14:15Z — `@akushonkamen has signed the CLA…#1987`;
  last code commit `a9ef13f7` 2026-10-09T16:47:25Z —
  `chore: release graphiti-core 0.31.0…`)
- version at pin: graphiti-core 0.31.0 (per release-commit title)
- license: Apache-2.0 (per SPDX header in every source file read;
  LICENSE file not separately fetched)
- language/stack: Python 3.10+, `graphiti_core` package + MCP server
  + FastAPI REST service; backends Neo4j 5.26 / FalkorDB / Neptune(+AOSS)
  / Kuzu (deprecated); LLM via OpenAI default, Anthropic/Gemini/Groq/
  Azure/OpenAI-compatible transports
- method: raw-file reads at pin + Atom commit feed. No clone, no shell.
  Scoped to episode/node/edge schemas, contradiction/invalidation,
  entity resolution, search recipes, backend seams — prompts bodies,
  driver internals, MCP/REST bodies, tests not line-read.

# What it is (source summary)

A pipeline that turns raw intake (**episodes**: messages, text, JSON,
fact triples) into a temporal property graph and queries it back.
LLM calls extract entities per episode, extract edges against the
episode's entity set (with per-episode attribution in bulk mode), and
resolve both against the existing graph: nodes dedupe through an
escalation ladder, edges dedupe through LLM judgement plus a separate
date-arithmetic invalidation pass. Contradicted facts are expired,
never deleted — each edge carries when it became true (`valid_at`),
when it stopped being true (`invalid_at`), when the system recorded
the expiry (`expired_at`), and which episode time the claim was read
against (`reference_time`). Retrieval is hybrid by construction
(BM25 + cosine, optional BFS) with pluggable rerankers (RRF, MMR,
cross-encoder, node-distance, episode-mentions). Ontology is
optional-but-typed: developer Pydantic models prescribe entity/edge
types and signatures, or structure emerges (learned). `group_id`
partitions every node and edge (multi-tenancy namespace). Zep (the
company) sells the managed/proprietary-engine form; this repo is the
self-hosted OSS core with bring-your-own graph database.

# Architecture (as evidenced)

- Episode + node model (`graphiti_core/nodes.py`, full read):
  `EpisodicNode{source: message|json|text|fact_triple,
  source_description, content, valid_at, entity_edges[],
  episode_metadata?}`; `EntityNode{name_embedding, summary,
  attributes, labels}`; `CommunityNode`; `SagaNode{summary,
  first/last_episode_uuid, last_summarized_at (ingestion watermark),
  last_summarized_episode_valid_at (episode-time semantics)}`.
  All nodes carry `uuid` (default uuid4) + `group_id` + `created_at`;
  hash/eq by uuid. `EpisodeType.from_str` raises on unknown (fail-closed
  ingest typing). Label validation on assignment. Provenance direction:
  episodes are ground truth; `MENTIONS` edges link episodes to entities;
  entity edges list their source episode uuids.
- Edge temporal schema (`graphiti_core/edges.py`, full read):
  `EntityEdge{name, fact, fact_embedding?, episodes[],
  expired_at?, valid_at?, invalid_at?, reference_time?, attributes}`.
  Field-doc discipline: future-tense plans/promises take the *episode
  reference time* as `valid_at` (not the future occurrence date);
  soft deadlines are *not* `invalid_at`. `expired_at` = record-time of
  invalidation, distinct from `invalid_at` = world-time end of truth —
  a two-clock split on the edge itself (cf. utopia.md's
  valid/record-time ledger). Kuzu stores edges as `RelatesToNode_`
  intermediate nodes (every query has a Kuzu branch). Record helpers
  strip system keys out of `attributes` on read (attribute/system
  separation enforced at the boundary).
- Contradiction/invalidation (`graphiti_core/utils/maintenance/
  edge_operations.py`, full read): `resolve_extracted_edges` returns
  `(resolved, invalidated, new)` and optionally fills
  `invalidated_by: {expired_uuid: [resolver_uuids]}` (attribution of
  every expiry). Pipeline per edge: verbatim fast path (exact
  normalized fact + same endpoints → reuse edge, append episode uuid,
  skip LLM) → LLM `resolve_edge` over duplicate candidates (same-pair
  endpoints) + invalidation candidates (global hybrid search) with
  continuous indexing across both lists → `_apply_temporal_invalidation`
  (date arithmetic only): arrived-with-end-date expires immediately;
  later-starting candidate supersedes (`invalid_at=candidate.valid_at`,
  `expired_at=now`); `resolve_edge_contradictions` skips
  non-overlapping windows, else sets earlier edge
  `invalid_at=new.valid_at`, `expired_at=now`. Malformed LLM indices
  (out-of-range duplicates/contradictions, duplicate IDs) are logged
  and ignored, never applied. Bulk path invalidates + dates too
  (0.31.0 docstring correction — stated parity, not assumed).
  Timestamps come from extraction or a small-model `EdgeTimestamps`
  call gated on available `reference_time`; future bounds normalized
  (`normalize_future_temporal_bounds`).
- Entity resolution (`graphiti_core/utils/maintenance/node_operations.py`,
  full read): extract → `_collapse_exact_duplicate_extracted_nodes`
  (same normalized name; keep more-specific type label) → candidates
  per node: identity-property lookup → exact-name `get_by_names` →
  cosine similarity (`NODE_DEDUP_CANDIDATE_LIMIT=15`,
  `NODE_DEDUP_COSINE_MIN_SCORE=0.6`) → resolution ladder: identity,
  then similarity, then LLM escalation (`NodeResolutions`) with
  defensive guards (missing/extra/duplicate resolution IDs
  warned-and-skipped; invalid `duplicate_candidate_id` treated as
  no-duplicate). Resolved-as-duplicate promotes the durable node and
  records `(extracted, resolved)` pairs; `IS_DUPLICATE_OF` edges
  filtered before re-adding (`filter_existing_duplicate_of_edges`).
  Summaries batched (`MAX_NODES=30`), short summaries take edge-fact
  append shortcut, long ones go to LLM batch summarizer capped at
  sentence boundary (`MAX_SUMMARY_CHARS`).
- Retrieval (`graphiti_core/search/search_config_recipes.py`, full read):
  recipes compose per-kind configs; canonical hybrid is BM25 + cosine
  reranked by RRF (`EDGE_HYBRID_SEARCH_RRF` is what dedup and
  invalidation both query with). Reranker menu: rrf / mmr / cross_encoder
  / node_distance / episode_mentions; cross-encoder variants add BFS
  traversal. Dedup-time optimization worth noting: endpoint-prefilter
  (`_pair_search_filter`: src/tgt IN {a,b} both orientations, one query)
  + `_same_pair_edges` postfilter (drops self-loops the prefilter
  admits) — retrieval cost bounded before LLM ranking.
- Backends + runtime posture (README + match-arms in nodes/edges):
  every persistence method branches `Neo4j | FalkorDB | Kuzu | Neptune`
  behind a `graph_operations_interface` seam (custom ops override,
  `NotImplementedError` falls through to provider Cypher). Concurrency
  `SEMAPHORE_LIMIT=10` default (429-avoidance, raise deliberately).
  Prompts overridable per client instance (`ChatPrompt` return contract,
  fixed response schemas); opt-in `LLMRuntime` routes prompts across
  provider transports. Telemetry is opt-out PostHog anonymous
  (disabled via `GRAPHITI_TELEMETRY_ENABLED=false`).

# Findings for Runlith

- F-01 — Edge-local two clocks: `valid_at`/`invalid_at` (world time)
  vs `expired_at` (record time) on every fact edge, plus
  `reference_time` pinning which episode clock the claim was read
  against. Sharper than utopia.md's table-level bitemporality: the
  split lives on the fact itself with documented edge cases
  (future-tense → reference time; soft deadline ≠ end of truth).
- F-02 — Invalidate-don't-delete with resolver attribution:
  `invalidated_by` maps each expired edge to the resolving edges that
  killed it; contradictions resolve by window arithmetic (overlap
  required, earlier edge closed at new edge's start), LLM proposes,
  dates dispose.
- F-03 — Episodes as first-class provenance: raw intake preserved as
  nodes, every fact lists source episode uuids, `MENTIONS` edges make
  fact→source traversable, bulk extraction carries per-episode
  attribution indices. Utopia.md has quotes+doc-links; episodes add
  the *ingest-stream* half of provenance.
- F-04 — Dedup retrieval bounding: endpoint prefilter + same-pair
  postfilter keeps LLM judgement over a small candidate set; two
  searches per edge (pair-scoped duplicates, global invalidation
  candidates) separate "same thing said again" from "different thing
  that conflicts."
- F-05 — Resolution escalation ladder (identity → exact-name → cosine
  ≥0.6 top-15 → LLM) with LLM-output guards (range-check indices,
  ignore duplicate IDs, invalid→no-duplicate). Deterministic layers
  absorb the cheap cases; the model only sees the residue.
- F-06 — Reranker menu as configuration, not code: five rerankers ×
  per-kind recipes; cross-encoder is the only variant that pays for
  BFS. Precedent for G9 replaceable-retriever config surface.
- F-07 — Stated parity maintenance (0.31.0 docstring fix: bulk path
  *does* invalidate and date) and honest deprecation (Kuzu
  unmaintained → `DeprecationWarning`, removal announced). Small-scale
  version of the docs-honesty posture Runlith wants.
- F-08 — Group-id partitioning as the single namespace primitive:
  every node/edge/query carries it; candidate merges preserve search
  order with override-union. No per-fact ACLs — ceiling is the group.

# Comparison with our project

## What is better in Runlith

- Policy gates *actions* deterministically (DENY > REQUIRE_APPROVAL >
  ALLOW, exact scopes, single-use, child-subset, fail-closed,
  hash-chained). Graphiti gates nothing: ingestion is LLM-judged
  throughout (extraction, dedup, contradiction) with no authorization,
  no approval shape, no audit chain — `invalidated_by` attributes,
  it does not authorize.
- Durable substrate is local SQLite + event journal, offline-capable.
  Graphiti requires an operated graph database (Neo4j/FalkorDB/Neptune)
  plus LLM + embedder providers at both write and read time.
- Secrets posture (G6 broker, blind delivery) vs API keys in
  environment with no broker, no redaction story, telemetry exfiltrating
  config choices by default (opt-out, not opt-in).
- Verification is an independent port (G7/EvaluationPort). Graphiti's
  closest analogue (LLM contradiction judgement) is same-substrate
  adjudication: the model that extracted also judges duplicates.
- No irreversible-action ambiguity: runner boundary + worktree
  admission vs episodes ingested and merged into the graph with no
  human-confirm gate anywhere on the write path.

## What is worse in Runlith (gaps this exposes)

- No fact-level temporal schema: no valid/invalid/expired/reference
  quadruple, no future-tense or soft-deadline rules, no
  close-at-new-start arithmetic (P2, R-01/R-02).
- No invalidate-with-attribution lifecycle: no expired-not-deleted
  discipline, no resolver→invalidated map, no overlap-required rule
  (P2, R-02).
- No ingest-stream provenance: no episode-shaped raw-intake nodes,
  no fact→source-episode traversal, no per-item attribution in bulk
  writes (P3, R-03).
- No retrieval-bounding pattern for judgement calls: no
  prefilter-then-LLM-rank split, no duplicate-vs-invalidation
  candidate separation (P3, R-04).
- No escalation ladder for identity: deterministic layers before
  model judgement with guarded outputs (P3, R-05/R-06).
- No reranker-menu configuration vocabulary for future G9 retriever
  options (P3, R-04).

## What is better in the source

- Temporal edge schema with documented hard cases is the most
  implementable time-model seen so far: four fields + two rules
  (future-tense, soft-deadline) cover what utopia.md leaves as
  clock semantics.
- Invalidation pipeline separates proposal (LLM indices) from disposal
  (date arithmetic) — the model can be wrong about indices (guarded)
  without corrupting windows, because expiry requires overlap.
- Episode provenance is structural, not decorative: bulk attribution
  indices, MENTIONS edges, episode uuid lists on facts, saga watermarks
  distinguishing ingestion time from episode time.
- Retrieval bounding before judgement (endpoint prefilter + postfilter)
  is a cost-and-correctness pattern, not just an optimization.
- Commit-feed honesty: docstring parity fix shipped as a versioned
  release note rather than silently corrected.

## What is worse in the source

- Trust model is LLM judgement all the way down over a server-side
  graph; no policy, no approval, no human gate, no independent
  verifier. Same-substrate adjudication (extractor == judge) with
  bimodal confidence by construction.
- Availability couples to three externals (graph DB, LLM, embedder)
  on the write path; timestamp extraction fails open (logged warning,
  edge persists undated — `ensure_utc(None)` comparisons then skip
  invalidation silently downstream).
- Dated edges depend on model-emitted ISO dates parsed with
  try/except-skip; malformed dates degrade to `None` (no timestamp)
  rather than quarantining the edge.
- Group-id is the only isolation primitive: no per-fact visibility,
  no scope ceilings, no redaction; telemetry defaults to on.
- Kuzu backend rots in-tree (deprecated upstream, removal pending)
  while every query still carries its branch — dead-weight seams.

# Opportunities and recommendations

- R-01 — Adopt the fact-temporal quadruple (`valid_at`, `invalid_at`,
  `expired_at`, `reference_time`) + the two field rules (future-tense
  claims pin reference time; soft deadlines never close truth) as the
  G10 edge schema. P2, `NEEDS_ADR` → `ADAPT` (schema-level; SQLite
  idiom; cite utopia R-01 as the lifecycle parent, F-01 as the
  field-level refinement).
- R-02 — Adopt invalidate-don't-delete with resolver attribution
  (`invalidated_by` shape) and the overlap-required close rule
  (earlier edge closes at new edge's start; non-overlap never
  expires). P2, `NOVEL_GAP` → `ADAPT` (needs ADR; complements utopia
  R-01 close+link with the arithmetic utopia leaves unspecified).
- R-03 — Adopt episode-shaped ingest provenance (raw-intake record +
  fact→source links + per-item bulk attribution) as the G10
  provenance half utopia.md does not cover (it covers doc-quotes,
  not streams). P3, `NEEDS_ADR` → `ADAPT` (ingestion-time only; no
  graph-DB shape implied).
- R-04 — Reference the retrieval-bounding + reranker-menu patterns
  (endpoint prefilter/postfilter, duplicate-vs-invalidation candidate
  split, five-reranker config surface) as G9 design notes. P3,
  `ALREADY_DESIGNED` (replaceable retrievers) → `INSPIRED_ONLY`.
- R-05 — Cite LLM-output guardrails (index range-checks, duplicate-ID
  ignore, invalid→no-op defaults, bulk/write-path parity assertions)
  as `EXTERNAL_VALIDATION` for any future LLM-touching Runlith code
  (cf. pydantic-ai R-03/R-06 blind-spot discipline). P3.
- R-06 — Note the escalation ladder (identity → exact → similarity
  threshold → model) + verbatim fast path as the shape any future
  G10 dedup spike should follow: deterministic layers first, model
  last, over bounded candidates. P3, `NOVEL_GAP` → `INSPIRED_ONLY`
  (no G10 dedup component exists to adapt into yet).
- REJECT: Python engine and `graphiti-core` dependency; Neo4j/FalkorDB/
  Neptune/Kuzu backends and Cypher/query seams; LLM extraction/dedup/
  timestamp prompts and Pydantic ontology import; MCP server and REST
  service as code; same-substrate LLM adjudication as verification
  (violates independent-verifier posture); opt-out telemetry pattern;
  group-id as authorization model.

# Evidence and references

- Pin: `main@964bb11d…` via Atom feed `commits/main.atom`
  (2026-10-10T15:14:15Z; HEAD is a CLA-bot signature; last code
  commit `a9ef13f7` 2026-10-09T16:47:25Z, graphiti-core 0.31.0).
- Full pinned raw URLs (verified at pin):
  - README (context-graph definition, Zep-vs-Graphiti, backends,
    concurrency, prompts, telemetry):
    https://raw.githubusercontent.com/getzep/graphiti/964bb11d1e7020e3cc67fab244268b0736b6c219/README.md
  - episode + node schemas (EpisodicNode/EntityNode/CommunityNode/
    SagaNode, group_id, EpisodeType):
    https://raw.githubusercontent.com/getzep/graphiti/964bb11d1e7020e3cc67fab244268b0736b6c219/graphiti_core/nodes.py
  - edge schemas (EntityEdge temporal quadruple, Kuzu branches,
    attribute/system separation):
    https://raw.githubusercontent.com/getzep/graphiti/964bb11d1e7020e3cc67fab244268b0736b6c219/graphiti_core/edges.py
  - contradiction/invalidation (resolve flow, temporal arithmetic,
    LLM guards, invalidated_by):
    https://raw.githubusercontent.com/getzep/graphiti/964bb11d1e7020e3cc67fab244268b0736b6c219/graphiti_core/utils/maintenance/edge_operations.py
  - entity resolution (ladder, thresholds, LLM escalation guards,
    summaries):
    https://raw.githubusercontent.com/getzep/graphiti/964bb11d1e7020e3cc67fab244268b0736b6c219/graphiti_core/utils/maintenance/node_operations.py
  - search recipes (hybrid + five rerankers):
    https://raw.githubusercontent.com/getzep/graphiti/964bb11d1e7020e3cc67fab244268b0736b6c219/graphiti_core/search/search_config_recipes.py
- Commit evidence (feed bodies): 0.31.0 bulk-path parity fix (#1985);
  Zep-fork upstream of extraction/dedup/temporal fixes (#1964);
  LLMRuntime prompt/model routing (#1778); FalkorDB 6.x index DDL fix
  (#1963); dependency security bumps (#1972/#1965/#1936).
- Not inspected: prompt bodies, driver internals, `graphiti.py`
  orchestration, MCP/REST servers, community/saga builders, tests.
  Numeric claims (thresholds, limits, summary caps) are source-verified
  in files read; backend behavior beyond match-arms is doc-inferred.
- Runlith references: AGENT.md §§4/7; G9/G10 notes; utopia R-01/R-02
  (bitemporal lifecycle parent, proposal-gate parent — this file adds
  field-level and arithmetic detail, not a second lifecycle);
  pydantic-ai R-03/R-06 (advisory-gate and blind-spot discipline).

# Final assessment

Highest-yield temporal-graph reference after utopia.md, and
deliberately non-overlapping with it: utopia owns the bitemporal
ledger + governance argument; graphiti owns the field-level schema
(R-01), the expiry arithmetic with attribution (R-02), the
ingest-stream provenance half (R-03), and the bounded-retrieval
patterns (R-04/R-06). Two P2 ADAPT candidates (schema, invalidation)
attach to utopia R-01 as refinements; the rest are P3 notes and
validation. Nothing structural transfers: the Python engine, graph
backends, LLM pipelines, and group-id-as-auth stay REJECT, with the
source's own fail-open timestamp path as the standing caution. No IKQ
entry: schema- and requirements-level deltas only.
