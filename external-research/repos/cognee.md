# Re-audit

topoteretes/cognee — open-source AI memory platform (knowledge-graph
+ vector + session memory for agents: `remember` / `recall` /
`improve` / `forget` over `add` + `cognify` ingestion).

- repo: https://github.com/topoteretes/cognee
- reviewed_revision: `0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1`
  (main, 2026-10-08T07:36:40Z — `chore: Sync cognee-mcp lock to
  cognee 1.6.3 [release] (#5525)`)
- version at pin: 1.6.3 (release PR #5458 merged 2026-10-07, lock-sync
  follow-ups #5525/#5529; `cognee/version.py` not line-read)
- license: Apache-2.0 (Copyright 2024 Topoteretes UG)
- language/stack: Python 3.10–3.14 (`uv`/`pip`); local GLiNER
  extraction + local embeddings when no LLM key, OpenAI default
  otherwise; `dlt` core dependency; backends Neo4j/Ladybug (Cypher),
  Postgres/pgvector (demo graph store), SQLite/Turso relational,
  Redis/FS session cache, Modal distributed mode, Docker images,
  MCP server, Claude Code/Codex plugins
- method: raw-file reads at pin + Atom commit feed (no REST; tree
  pages used twice: `cognee/modules` HTML, `cognee/` Markdown). No
  clone. Scoped to memory entries, session lifecycle, provenance,
  retrieval contracts, prune/retention, ontology exports, eval
  honesty — chunking bodies, store adapters, cognify/memify task
  graphs, API routers, MCP/plugin code not line-read.

# What it is (source summary)

Two ingestion primitives: `add` registers source data as documents,
`cognify` processes them into memory — chunks, LLM-extracted
entities/relationships into a knowledge graph, summaries, embeddings
for vector search. V2 memory API (`remember`/`recall`/`improve`/
`forget`) layers session-scoped typed entries on top: raw data still
flows through the permanent add+cognify path, while Q&A turns, agent
trace steps, feedback, and skill-run records dispatch to
`SessionManager` methods via a `type`-discriminated union. Session
processing distills updates/preferences from completed conversations
into the graph (`improve` bridge, background fire-and-forget tasks
awaited at exit). Retrieval is per-scope (`graph`, `session`,
`trace`, `code`, `tools`, …) over chunk-vector, graph, summary, and
global-context-index views, with raw retrievers opting out of the
LLM pre-retrieval turn analysis. Deletion is explicit per-store
`prune` flags; evals are documented with held-out vs exploratory
labels in a BEAM technical report.

# Architecture (as evidenced)

- API layering (`cognee/__init__.py`, full read): V1
  (`add`/`cognify`/`memify`/`delete`/`update`/`prune`/`search`/
  `SearchType`/`validate`/`visualize`/`session`/`datasets`/
  `agents`/`tools`) beside V2 memory API (`remember`/
  `RememberResult`/`recall`/`improve`/`forget`/`serve`/
  `disconnect`/`push`/`export` + results). Provenance and session
  models self-register on import (`session_lifecycle`,
  `migrations`, `tools`, `provenance.edge_evidence`).
- Memory entries (`cognee/memory/entries.py`, full read):
  `MemoryEntry = QAEntry | TraceEntry | FeedbackEntry |
  SkillRunEntry`, each with a literal `type` discriminator routing
  to a `SessionManager` method. `QAEntry{question, answer,
  context, feedback_text?, feedback_score?,
  used_graph_element_ids?}`; `TraceEntry{origin_function,
  success|error, method_params?, method_return_value?,
  memory_query, memory_context, error_message,
  generate_feedback_with_llm}`; `FeedbackEntry{qa_id,
  feedback_text?, feedback_score?}` — "semantically an update
  rather than a new memory"; `SkillRunEntry` is graph-backed
  (not session-cache-backed) with bounded validators
  (`success_score` in [0,1], `feedback` in [-1,1],
  non-negative ms timestamps) plus `router_version`,
  `candidate_skill_ids`, `tool_trace`.
- Recall scopes (`entries.py`, same read): `RecallScope =
  auto|graph|session|session_first|trace|graph_context|
  session_context|all|tools|code`. `tools` (external DB) and
  `code` (deterministic code-graph query with snapshot cost) are
  explicit opt-in — deliberately excluded from `all`/`auto`
  expansion, with comments stating why. `graph_context`
  deprecated → `graph` with warning. `normalize_scope` dedupes,
  preserves order, raises `ValueError` on unknown names.
- Session lifecycle (`modules/session_lifecycle/models.py`, full
  read): `SessionRecord` per (user, session_id) is narrow —
  lifecycle + aggregate counters only. Status stored
  (`running`/`completed`/`failed`); `abandoned` is *inferred at
  read time* from `last_activity_at` — "without any sweeper
  touching the row." QA/trace detail lives in the session cache
  (Redis/FS); the record references it only via string
  `session_id` ("cache IDs aren't FK-able"). Counters:
  `tokens_in/out`, `cost_usd`, `error_count`, `last_model`;
  `SessionModelUsage` normalizes per-(session, user, model)
  attribution for mixed-model sessions. `agent_id` stamped
  best-effort when an agent connection claims the session.
- Provenance (`modules/provenance/edge_evidence/models.py`, full
  read): durable representations named in the module docstring —
  revisions (`Data`), segments (`DocumentChunk`), activities
  (`PipelineRun`), assertions (graph edges with `edge_object_id`).
  `ProvenanceEdgeEvidence` is the append-only many-to-many sidecar:
  `{tenant?, user, dataset, data, pipeline_run?,
  chunk, chunk_index?, edge_id, source_node, destination_node,
  relationship_name, evidence_kind='extracted', source_task?,
  confidence?}`, indexed by (dataset, edge), (dataset, data,
  chunk), (pipeline_run). "Never consulted by graph-native
  deletion." README v1.6.1 notes: document `external_metadata`
  stamped on every chunk and surfaced in hybrid retrieval.
- Retrieval contract (`modules/retrieval/
  cypher_search_retriever.py`, full read): `BaseRetriever` trio
  `get_retrieved_objects` / `get_context_from_objects` /
  `get_completion_from_context`. Raw retrievers return rows via
  `result_object: Any` and `None` context — `SearchResultPayload.
  context` is typed `str | list[str] | None`, so raw rows must not
  flow through it (regression #5485). `supports_session_turn_
  preparation = False` opts raw retrievers (cypher, natural
  language, chunks, lexical, summaries, code, skills,
  graph-report) out of pre-retrieval LLM turn analysis, which had
  rewritten Cypher into prose or answered it with an
  acknowledgement. Cypher gates on adapter capability
  (`GraphDBInterface.supports_cypher_queries`; Neo4j, Ladybug)
  and warns on empty-graph search.
- Ontology (`low_level.py`, full read — engine/graph-model
  re-exports, not ops): `Edge`, `ExtendableDataPoint`
  (`DataPoint`), `EntitySpec`, `GraphSchemaSpec`,
  `graph_model_from_spec`, `graph_spec_to_json_schema` (from
  `cognee.modules.graph_models`); docs offer custom data models
  and ontologies per application. API also exposes
  `get_memory_provenance_graph` / `visualize_memory_provenance`.
- Retention (`api/v1/prune/prune.py` + `__init__.py`, full
  reads): `prune.prune_data()` and `prune.prune_system(graph=
  True, vector=True, metadata=False, cache=True)` delegating to
  `cognee.modules.data.deletion.{prune_data, prune_system}`.
  Relational-metadata wipe is opt-in (default `False`). Feed
  fix #5501: Turso open-errors raised as `OperationalError` for
  SQLite parity so `prune_system(metadata=True)` skips cleanly
  on fresh installs.
- Eval honesty (`eval_framework/beam/REPORT.md`, full read):
  BEAM 0.79 fixed-hybrid @100K (4 QA rounds, 20 questions,
  held-out conversation, run std 0.005) vs 0.67 routed @10M
  labeled exploratory (selection and scoring on the same
  question set — "in-sample"). Per-ability breakdown (2 Qs
  each; extraction/updates/preferences/instructions 1.0,
  multi-hop/temporal/abstention 0.50), preprocessing audit
  (<0.2% noisy turns LLM-compressed with published prompt),
  archived configs/prompts/routing maps, stated non-transfer
  ("no hidden test set"), no ablations claimed. Reported
  100K config did *not* query the global context index
  (`include_global_context_index=false`) though ingestion
  built it.
- Commit feed at pin: release v1.6.3 (#5458) + lock syncs;
  `dlt<1.31` pin (#5486) after a yanked-helper ImportError broke
  `import cognee` (module on the top-level import chain); NL
  search payload fix (#5485); cypher opt-out + git-in-Docker
  runtime fix (#5483); `pull_request_target` checkout removal
  for OSSF Scorecard (SDK-568).
- Degradation posture (README): with no LLM key, local GLiNER +
  local embeddings run ingestion/retrieval while "LLM-dependent
  improvement stages skip automatically" — silent capability
  reduction, stated once.

# Findings for Runlith

- F-01 — Discriminated-union entry vocabulary with per-type
  dispatch and bounded validators; feedback modeled as update
  chained by ID, not as new memory. Direct schema precedent
  for G9/G10 entry types.
- F-02 — Scope algebra with principled exclusions: external
  (`tools`) and costly (`code`) sources never ride along with
  `all`/`auto`; unknown scopes fail loud (`ValueError`);
  deprecated alias warns. Matches Runlith's explicit-scope
  instinct, with the opt-in-exclusion rule Runlith lacks.
- F-03 — Narrow session record (lifecycle + counters) with
  read-time-inferred `abandoned` (no sweeper) and cache/
  relational split via non-FK string IDs. Precedent for
  journal-adjacent session bookkeeping without migrations
  per transition.
- F-04 — Four-level durable vocabulary (revision / segment /
  activity / assertion) plus append-only evidence sidecar the
  deleter never consults. Closest external analog to
  Runlith's journal + content-addressed thinking.
- F-05 — Per-chunk `external_metadata` stamping plus per-edge
  `{evidence_kind, source_task, confidence, pipeline_run}`
  fields. A concrete provenance field list to crib.
- F-06 — Raw-vs-generative retriever contract: typed `context`
  vs `Any result_object`, turn-prep opt-out flag. Precedent
  for keeping raw evidence out of typed completion paths —
  and a cautionary tale about LLM stages rewriting queries.
- F-07 — Retention as explicit per-store prune flags with
  metadata wipe default-off. A retention-policy shape with
  the safe default in the right place.
- F-08 — Eval-report honesty pattern: held-out vs exploratory
  labels, per-ability tables with CIs, archived configs,
  stated non-ablations. Template for future G10 eval docs.

# Comparison with our project

## What is better in Runlith

- Policy is deterministic and auditable: DENY > REQUIRE_APPROVAL
  > ALLOW over exact scopes, single-use, child-subset,
  fail-closed, hash-chained. Cognee has no policy language;
  its "judgement" is an LLM (extraction, turn analysis,
  distillation, routing) or nothing.
- Durable substrate is local and content-addressed: SQLite +
  event journal, no operated stack. Cognee memory *is* a
  stack — graph DB + vector store + Redis/FS cache +
  relational DB (+ Modal for scale).
- Secrets boundary (G6: OS keychain, blind delivery) vs a
  pipeline that ships document text, chunks, and trace
  parameters to embedding/LLM providers by design, including
  local models that download on first use.
- No silent degradation: Runlith fails closed where cognee
  "skips" LLM-dependent improvement stages when no key is
  configured and keeps serving thinner memory.
- No irreversible-action ambiguity: runner boundary + worktree
  admission vs raw `CypherSearchRetriever` executing caller-
  supplied query strings against the graph engine.

## What is worse in Runlith (gaps this exposes)

- No entry-type vocabulary for future memory: no
  qa/trace/feedback/skill-run discriminators, no
  feedback-as-update chaining, no bounded score validators
  (P2, R-01).
- No recall-scope algebra: no named-source scoping, no
  opt-in-exclusion rule for external/costly sources, no
  fail-loud unknown-scope normalization (P2, R-02).
- No session lifecycle vocabulary: no running/completed/
  failed/abandoned-inferred states, no narrow-record vs
  cache-detail split (P3, R-03).
- No provenance sidecar fields: no evidence_kind/source_task/
  confidence/run-link per assertion, no per-chunk external
  metadata stamping rule (P3, R-04).
- No per-session cost attribution: no token/cost counters or
  per-model usage split (P3, R-05).
- No retention API shape: no per-store prune flags with
  safe-default metadata handling (P3, R-08).

## What is better in the source

- Entry union is the cleanest memory-write vocabulary seen so
  far: one `remember()` surface, `type`-routed dispatch,
  update-vs-new distinction carried in the schema, validators
  with documented ranges.
- Scope normalization is fail-loud with a commented rationale
  per exclusion (`tools` = external reach, `code` = snapshot
  cost) — the rare allowlist that says why each member is out.
- Session record narrowness is enforced by schema, and the
  inferred-`abandoned` comment ("without any sweeper touching
  the row") is the exact no-background-mutation discipline
  worth copying.
- BEAM report states in-sample vs held-out, per-ability
  weakness (multi-hop/temporal/abstention 0.50), and what was
  *not* queried — more candid than most eval writeups.
- Payload/result_object split plus turn-prep opt-out shows a
  team learning, in public, where LLM stages corrupt
  deterministic paths.

## What is worse in the source

- Every memory stage is LLM-judged: extraction, summarization,
  session distillation, question-type routing, turn analysis.
  Non-determinism is the architecture, not a component.
- Top-level import chain fragility: one upstream helper
  removal (`dlt` 1.31) made `import cognee` fail fleet-wide;
  the fix is a version pin, the follow-up a stdlib rewrite.
- LLM-in-the-loop corrupted its own deterministic surfaces
  twice at this pin's window (NL payload 500s, Cypher
  rewriting) — opt-outs treat symptoms; the loop stays.
- Postgres-as-graph-store is a demo upsell ("production-ready
  version … licensed product"); multi-tenant auth defaults on
  with a single-user escape flag — inverse of local-first
  posture.
- Python Engine + graph/vector/cache stack, dlt ingestion,
  Modal distribution, Docker/git runtime coupling: nothing
  below the idea layer fits a Rust control plane.

# Opportunities and recommendations

- R-01 — ADAPT entry-type vocabulary for G9/G10 design: typed
  discriminators (qa / trace / feedback / skill-run shaped),
  feedback-as-update chained by entry ID, bounded score
  ranges ([0,1] success, [-1,1] feedback), graph-vs-cache
  backing declared per type (F-01). P2, `NOVEL_GAP` → `ADAPT`
  (schema/docs-level; no engine import).
- R-02 — ADAPT recall-scope algebra: named sources, `all`/
  `auto` expansions that never include external or costly
  sources without explicit opt-in, fail-loud unknown names,
  deprecated-alias warnings (F-02). P2, `NOVEL_GAP` → `ADAPT`.
- R-03 — ADAPT session lifecycle vocabulary for journal
  recovery docs: stored running/completed/failed +
  read-time-inferred abandoned from last-activity (no
  sweeper), narrow record vs detail-store split with
  non-FK string references (F-03). P3, `NOVEL_GAP` → `ADAPT`.
- R-04 — ADAPT provenance sidecar fields for journal/event
  schema: per-assertion `{evidence_kind, source_task,
  confidence, pipeline_run}` + per-segment external metadata
  stamping + append-only table the deleter never consults
  (F-04, F-05). P3, `NOVEL_GAP` → `ADAPT`.
- R-05 — ADAPT per-session cost attribution shape: aggregate
  counters on the session record + per-model usage split for
  mixed-model runs (F-03; cf. pydantic-ai R-04 brake
  semantics, but here schema-only). P3, `NOVEL_GAP` →
  `ADAPT` (requirements-level).
- R-06 — Cite typed-context vs raw-rows split (F-06) as
  `INSPIRED_ONLY` precedent for keeping raw evidence out of
  typed completion paths in future recall design. P3.
- R-07 — Cite BEAM report honesty pattern (F-08) as
  `EXTERNAL_VALIDATION` for future G10 eval-doc standards:
  held-out vs exploratory labels, per-ability tables,
  archived configs, stated non-ablations. P3.
- R-08 — ADAPT retention-policy shape: explicit per-store
  prune flags with relational-metadata wipe default-off
  (F-07). P3, `NOVEL_GAP` → `ADAPT` (docs/requirements
  only; no deletion code yet).
- REJECT: Python engine and all LLM pipelines (extraction,
  summarization, distillation, turn analysis, question-type
  routing, BEAM harness code); graph DBs (Neo4j/Ladybug) and
  Cypher raw-query surface; vector stores and embedding
  coupling; Redis/FS session cache; dlt ingestion; Modal
  distribution; GLiNER/local-model download posture;
  MCP/plugin/CLI/UI code; licensed Postgres-graph path;
  silent stage-skipping as degradation model.

# Evidence and references

- Pin: `main@0ec7a9fa…` via Atom feed `commits/main.atom`
  (2026-10-08T07:36:40Z, `#5525` lock sync; prior: release
  v1.6.3 #5458 2026-10-07, MCP bump #5529, turso fix #5501,
  dlt pin #5486, NL payload #5485, cypher/git-Docker #5483).
- Full pinned raw URLs (verified at pin):
  - README (memory platform overview, ops table, backends):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/README.md
  - license (Apache-2.0, Topoteretes UG 2024):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/LICENSE
  - public API surface (V1 + V2, model registration):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/__init__.py
  - engine/graph-model re-exports (Edge, DataPoint,
    EntitySpec/GraphSchemaSpec):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/low_level.py
  - memory entries + recall scopes (full read):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/memory/entries.py
  - memory package exports (trivial re-export):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/memory/__init__.py
  - session lifecycle models (full read):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/modules/session_lifecycle/models.py
  - provenance evidence sidecar (full read):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/modules/provenance/edge_evidence/models.py
  - prune API + package export (full reads):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/api/v1/prune/prune.py
  - cypher retriever (turn-prep opt-out, capability gate):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/modules/retrieval/cypher_search_retriever.py
  - BEAM eval report (scores, methodology, limits):
    https://raw.githubusercontent.com/topoteretes/cognee/0ec7a9fa61c9ff04bf7e02e0d57af363a993a5e1/cognee/eval_framework/beam/REPORT.md
- Tree pages (2, the allowed max): `cognee/modules` (HTML)
  and `cognee/` (Markdown) listings — used only to confirm
  `memory/`, `memify_pipelines/`, `pipelines/`, `api/`,
  `infrastructure/`, `shared/`, `tasks/` layout.
- Guessed-but-absent (404): `cognee/modules/data/models/
  Document.py` — data-model bodies not located; `Data` /
  `DocumentChunk` / `PipelineRun` shapes known only from the
  provenance module docstring, not line-read.
- Not inspected: chunking/cognify/memify task bodies, store
  adapters (graph/vector/relational/session-cache), API
  routers, search payload models, ontology/spec builders,
  migration sources (COGX), MCP/plugin/CLI/UI, tests, skills.
- Runlith references: AGENT.md §§4/7; G6/G9/G10 notes;
  pydantic-ai R-01/R-04/R-05 (approval split, spend brake,
  persistence vocabulary).

# Final assessment

Highest-yield source so far for G9/G10 *schema* thinking:
entry discriminators (R-01), scope algebra with opt-in
exclusions (R-02), and the revision/segment/activity/assertion
vocabulary with its evidence sidecar (R-04) are all ADAPT-grade
ideas that transfer as docs and field lists — no code crosses
the Rust boundary. Session lifecycle (R-03) and cost/counters
(R-05) follow at P3. The engine is entirely REJECT: every
memory stage is an LLM call, the stack is graph+vector+cache,
and the pin window itself shows LLM stages corrupting
deterministic paths twice. Take the vocabularies, leave the
machine. No IKQ entry: requirements/schema-level deltas only.
