# Repository

`deeplethe/utopia`

- repository: `utopia`
- url: https://github.com/deeplethe/utopia
- default_branch: `main`
- reviewed_revision: `main@1a75be63140ff1a94147524352a8b74647830a5e`
- commit_sha: `1a75be63140ff1a94147524352a8b74647830a5e`
- review_date: `2026-10-10`
- resource_type: GitHub repository (Rust workspace + Postgres + web UI)
- version_observed: workspace `0.1.0`; v0.1 status; `linux/amd64+arm64` images to `0.1.0-rc7`

# Reviewed revision

Evidence collected 2026-10-10 without a local clone. Sources actually
inspected at source level via raw fetch at the pinned SHA:

- `README.md` via landing page (philosophy, use cases, features table,
  quick start, roadmap, status v0.1).
- `Cargo.toml` (workspace: 9 crates, sqlx postgres+mysql, tantivy,
  pgvector, AES-GCM/JWT/argon2, Oxigraph RDF parsers).
- Crates inventory via GitHub Contents API (`utopia-core/cli/store/
  server/ingest/extract/reason/search/llm`).
- `crates/utopia-core/src/lib.rs` (module map + review_reasons contract).
- `crates/utopia-core/src/secrets.rs` (full: AES-256-GCM seal/open,
  `enc:v1:` format, idempotent, JSON-key helpers, tests).
- `crates/utopia-reason/src/ontology.rs` (full: 8 Defect kinds,
  check_ontology, cycle/unsatisfiable walks, sorted-deterministic output).
- `web/src/docs/mcp.md` (full: per-KB MCP endpoint, 10 tools, `at`/`as_of`
  two clocks, structuredContent contract, RDF export boundary, remember
  human-gate).
- `docs/pipeline.md` (full: parse→chunk→embed→extract→resolution→facts→
  ontology-growth→check/inference, 11 drop codes, Holmes 3-layer lesson).
- `docs/` inventory via API (`decisions/`, `design/`, `pipeline.md`).
- `SECURITY.md` (full: sealed creds, JWT, loopback DB, least-privilege
  roles, read-only SQL gate, deactivation-not-deletion).
- Latest commit SHA/date/message via GitHub Commits API.

Not inspected at source level: `crates/utopia-core/src/models.rs` (73 KB,
field-level claims are README/mcp-inferred), `utopia-store` SQL,
`utopia-server` routes/auth, `utopia-extract` prompts, `utopia-search`
RRF, migrations content, `web/` UI, `scripts/bench/` runs. Claims about
exact table/column names below are therefore marked as doc-inferred, not
source-verified. No external code was copied into Runlith, per `AGENT.md`
§16.1.

# Research target

Evaluate Utopia's bitemporal ontology-governed knowledge substrate against
Runlith's G9 Context Engine + Continuity and G10 Memory Engine (ADR-0010,
ADR-0011, ADR-0012). Identify missing capabilities, improvements,
redesigns, replacements, inspirations, validations, and rejections.

# Executive summary

Utopia is a Rust monolith + Postgres (+ pgvector, Tantivy embedded, job
queue as table) building a **bitemporal knowledge graph**: every fact
carries world time (when true) and record time (when believed), corrections
close old versions and link to replacements instead of overwriting. Edges
are reified (attributes of their own). Ontology (schema.org/W3C-Org/PROV-O/
FOAF/IOF packs, OWL import, or empty) is a contract: extraction follows it,
direction-correction enforces signatures at write time, axioms compile into
forward-chaining rules (off by default, separate `derived_facts` table,
asserted overrides derived).

Ingest: PDF/DOCX/PPTX/XLSX/CSV/MD/HTML, OCR via MinerU/Ark vision, audio
transcription, web/RSS/GitHub/Jira/Notion/WebDAV/S3 sync, API push
(observation time, no model in path). Two-phase: searchable in seconds
(chunk 1200/150 + embed), graph in minutes (one LLM call per chunk + batched
adjudication). Entity resolution: equality → similarity tiers
(≥0.55 attach, 0.35–0.55 create+review, <0.35 create) → containment recall
→ LLM adjudication → reversible merge (redirect, not close). Type
resolution: two candidate routes unioned, in-subtree auto-retype,
cross-axis human review. Eleven `extraction_drops` codes; failures stored
in English in `documents.graph_error`.

Governance: review queue (low-confidence, duplicates, cardinality),
conflict trichotomy (fact-vs-fact: close/keep-both/reject; axiom-violation:
retract-fact/relax-axiom/accept-both; ontology-first because bad ontology
poisons fact verdicts), ontology self-check (8 defect kinds, sorted output),
decision ledger append-only outliving its base, accounts deactivated not
deleted. Agents read over per-KB MCP (scoped `read`/`write` PATs, ceiling
not grant, Activity-attributed); `remember` records sentence now, facts wait
for human nod (prompt-injection-aware). Reads are typed/dated with stable
UUIDs + evidence; RDF export (turtle/jsonld, reification, PROV-O) is the
machine-read contract; MCP structuredContent is versioned, UI shapes are not.

Relevance to Runlith is high and architectural: Utopia owns the
**provenance-first, time-aware, ontology-checked memory** layer Runlith's
G10 designed but never built, plus the **human-gated agent-write** pattern
Runlith's INV-029 requires. It does not own durable Task graphs,
policy-gated execution, or independent verification of *actions*; those
Runlith differentiators are absent here.

Most valuable, in priority order:

1. Bitemporal close-don't-overwrite + supersede links as G10 lifecycle
   shape (ADAPT).
2. Agent-writes-are-proposals + review queue + remember-gate as INV-029
   reference (ADAPT).
3. Ontology-as-contract (signature direction-correction, no-relation stays
   empty, asserted-overrides-derived, check-writes-nothing) as validation
   discipline (ADAPT).
4. Conflict trichotomy + ontology-first ordering as G10/Utopia-shared
   governance (ADAPT).
5. Decision ledger outliving objects + deactivation-not-deletion as audit
   wisdom (INSPIRED_ONLY).
6. Sealed-at-rest credentials (AES-256-GCM, key outside DB) as secrets
   validation (EXTERNAL_VALIDATION; Runlith already brokers).

# Resource information

- Type: Rust workspace (9 crates) + Postgres/pgvector +417 web UI; one
  binary + one Postgres; FTS embedded, vectors in pgvector, queue is table.
- Runtime: Docker compose (`db` + `app` :1516, frontend :5173 dev);
  Rust 1.85+, Node 20+, pnpm; images `linux/amd64+arm64` (≤rc7 amd64-only).
- License: Apache-2.0. Contributing requires DCO sign-off; security via
  security@deeplethe.com.
- Distribution: `git clone` + `docker compose --profile app up -d`;
  first account becomes admin + public KB; models configured under
  Administration → Models; any OpenAI-compatible endpoint (DeepSeek/Qwen/
  GLM/Ollama/vLLM), air-gappable.
- Status: v0.1, schema forward-only no rollback, pin `UTOPIA_IMAGE`,
  backup DB + `data/` before upgrade.

# Core concepts

## 1. Bitemporal ledger, not overwriting store

World time (`valid_from/valid_to` + precisions, interpreted `holds_*`)
vs record time (`recorded_at/invalidated_at`, `derived_at`). Correction =
close old + link new (`supersedes`), retraction/deletion preserved in
export. MCP exposes both clocks (`at` vs `as_of`/`before`, `changes` over
record time). Decision replay ("what held then + what we knew then") is the
product ask. Doc-inferred table names; clock semantics source-verified via
mcp.md + pipeline.md.

## 2. Ontology as contract, not suggestion

`docs/pipeline.md` §1 + decision 0012: signature check on all three write
paths (extract write, predicate reattach, merge-move re-check); mismatch →
swap once with `direction_corrected` record, else drop predicate keep
subject/object/evidence. OOV literal → `object_value` + original term in
`proposed_predicate`; OOV entity-object → predicate empty (never "related
to", decision 0010). No-type-is-a-type (0009): empty ontology still
extracts, typeless entities allowed.

## 3. Check writes nothing; inference writes aside

`utopia-reason`: fact-check (self-loop/asymmetry/transitive-cycle/
cardinality/signature) → `axiom_violations` with full path; ontology
self-check first (8 defects: symmetric+asymmetric, transitive+functional,
subclass/subproperty cycles, disjoint-with-ancestor, inherits-disjoint,
inverse-of-itself, inverse-not-mutual) → `ontology_defects`, sorted for
determinism. Inference (transitivity/symmetry/inverses/hierarchy) only from
axioms, depth cap + cycle guard + 20K/predicate cap with reported
truncation, validity = premise intersection, separate table, gold edges,
asserted wins, contradiction → `derived_contradiction` violation (not
silent since #238). Off by default, full rerun on timer (60 min), no
incremental yet.

## 4. Entity resolution prefers split over merge

Equality (canonical+aliases) → profile similarity tiers (0.55/0.35) →
containment recall (≥4 chars, ≤4 pairs, +16-row SQL scan, creation-only) →
one batched LLM adjudication → merge (name→aliases, facts moved,
`entity_merges` reversible via `revert_merge`, pending pairs *redirected*
not closed) or keep-apart. Holmes bench: 14→37→47→**57** across three
hidden layers (equal-type tier, alias bridge, redirect). Trigram gap known
(`pg_trgm` needs superuser, restricted role - won't fix).

## 5. Human is the merge/confirm authority

Review queue: low-confidence extractions, suspected duplicates,
cardinality conflicts, cross-axis retypes, `remember` proposals (shown
under source sentence). Class-pair approved once never re-asked; human-typed
entities excluded from engine (`type_source=human`). Rejections carry
reason (`left_alone` analyzed: all retrieval-side, none adjudication).

## 6. Agent writes are proposals (prompt-injection-aware)

`remember` (MCP, `write` scope + editor rights, invisible otherwise):
sentence searchable at once attributed to token owner; facts queue for
person, one-by-one confirm/reject. Rationale stated: agent reads base
documents that can say "remember X" — ungated assert would cost a wrong
edge indistinguishable from intent. Review card names agent, not just
owner. Never claim fact added — "recorded, awaits confirmation".

## 7. Scoped MCP + stable IDs + versioned structured surface

Per-KB `POST /api/v1/kbs/{id}/mcp` (JSON-RPC 2.0, MCP 2025-06-18,
stateless HTTP). PATs (`utp_pat_…`, shown once, 90-day default,
KB-narrowable, scope=ceiling, Activity-logged). 10 tools: search_chunks
(6×800ch, `as_of` with recall caveat), get_document (24K cap),
find_entities, entity_facts (`at`+`as_of` combinable), neighbors, timeline,
paths_between (≤3 hops, shortest-first), changes (≤40), search_docs,
remember. structuredContent on 4 tools (versioned, breaking-change needs
decision record); text is presentation. UUID IRIs
(`urn:utopia:kb:…:{entity|fact|derived|rule|document}`), stable across
exports; `?base=` mints https IRIs. RDF export (turtle/jsonld, reification,
PROV-O/schema.org, derivations→rules, asserted+retracted, both clocks,
quotes, source docs) is *the* read contract; UI shapes explicitly not.

## 8. Append-only decision memory + sealed secrets

Ledger: confirm/reject fact, merge/revert entity, rebuild graph — who/when/
object-at-time; append-only, outlives object and base. Users deactivated
(`deactivated_at` blocks sign-in, ledger stays attributable). Secrets:
AES-256-GCM `enc:v1:`+base64(nonce‖ct), key in `UTOPIA_SECRET_KEY` or
generated `secret.key` (never in DB), idempotent seal, legacy plaintext
backfilled at start, rotation = explicit re-seal migration. Threat model:
dump/read-only-role ≠ credential leak; host compromise out of scope
(server ACLs' job). JWT 32B CSPRNG in DB, argon2 passwords, Secure cookies
via X-Forwarded-Proto, loopback DB bind, optional least-privilege app role
(business RW + ledger append; migrations as owner), read-only Ask-the-Data
gate (parser allowlist + RO txn + row limit), source grants per workspace
+ read-only source roles (SQL gate is depth, not substitute).

# Architecture and implementation

## Component map (source-verified structure, doc-inferred internals)

- `utopia-core` — domain models (73 KB, not line-read), error
  (terminal/deferred), config, text, secrets (seal), review_reasons
  (`name_vector|` vs family prefixes as evidence strengths).
- `utopia-reason` — `derive.rs` (74 KB), `rules.rs` (54 KB), `lib.rs`
  (61 KB), `ontology.rs` (self-check, verified), `expressions.rs`.
- `utopia-store/ingest/extract/search/llm/server/cli` — structure only;
  behavior via pipeline.md/mcp.md/SECURITY.md.
- `migrations/` — forward-only chain (content not read).
- `docs/decisions/` + `docs/design/` + `docs/pipeline.md` — the real spec
  (0001/0002/0003/0006/0008/0009/0010/0012/0017 + bench).
- `web/src/docs/mcp.md` — agent contract (verified).
- `scripts/bench/corpora/{contracts,holmes,tech,pharma}` + temporal QA
  bench — measurement with answer-key discipline (Holmes deliberately
  keyless: model memorized book).

## Execution flow

Upload/sync → parse (encodings detected) → chunk (1200/150) → embed →
ready (searchable/askable at once) → extract (1 LLM/chunk, ontology
budget-split: full-layout vs 40-class/30-relation/30-attr retrieval +
ancestors) → entity resolution → adjudication → merge/keep-apart +
facts (bitemporal ledger) → type resolution (manual preview→apply) +
ontology growth (OOV counts → confirm → joins ontology → feeds back into
next extraction) → auto check (post-import + on-demand) / timed inference.
Drops counted in `extraction_drops`, surfaced in UI; chunk failures in
`documents.graph_error` verbatim.

## Authentication

First registered account = sysadmin; per-KB roles (owner/admin/editor/
viewer), open vs restricted bases. PATs per person (name/scope/KBs/expiry),
Bearer on per-KB MCP endpoint, revocation next-call. Session cookies,
JWT-CSPRNG, argon2. Agent identity = token owner + tool name in Activity;
`remember` card names agent client.

## Policy lifecycle

Ontology axioms + review queue + conflict trichotomy + decision ledger are
the policy. No numeric approval chains like Runlith; gates are: extraction
thresholds (confidence, similarity tiers), axiom checks (R0), human confirm
for cross-axis/duplicate/low-confidence/proposals, generation-gated
ontology growth. `materialize_inferences` switch, `inference_interval_`,
retrieval budgets (40/30/30), caps (6 chunks, 80/300 facts, 40 changes,
24K doc, 20K derivations/predicate) — doc-stated, code-unverified.

## Storage lifecycle

Postgres (ontology DB always postgres; Ask-the-Data mounts postgres/mysql-
protocol/ClickHouse/Trino/Databricks/Snowflake). pgvector vectors, Tantivy
embedded FTS with RRF fusion, job queue as table. Migrations forward-only.
Backup = DB + `data/` (+ `secret.key` or creds unreadable). Rebuild ≠
identity-preserving (UUIDs stable per stored object, not across rebuild).

## Observability

Review page (two tiers: violations + defects, ontology-first), extraction
drops UI, `documents.graph_error`, inference run reports (truncation
counted), Activity per base (agent tool uses attributed), bench runner
(fresh DB per run — "reuse saves minutes, costs conclusions"), usage
unclear (no cost ledger evidenced).

# Security analysis

## Enforced technically (source-verified)

- AES-256-GCM credential seal with `enc:v1:` versioning, fresh nonce per
  seal (round-trip + tamper tests in file), idempotent, legacy passthrough
  + backfill path, wrong-key/tamper hard-errors (never silent garbage to
  downstream dialers).
- Ontology self-check before fact verdicts (bad-axiom noise eliminated
  first); check writes nothing; inference structurally separated
  (forgotten UNION → invisible, not mixed).
- Direction-correction recorded (`direction_corrected`), never silent swap.
- Merge reversibility (`entity_merges` + `revert_merge`); redirect-not-close
  for pending pairs (no silent loss).
- Deterministic check output (sorted preds/classes/paths; HashMap order
  neutralized) — reviewable diffs.
- `remember` invisibility without `write`+editor (no tool enumeration
  leak); scope-as-ceiling; cross-base isolation ("No document with that id
  in this knowledge base").

## Documented but not source-verified here

- Loopback DB bind, least-privilege app/migration role split, RO SQL gate
  triple-layer, per-workspace source grants, Secure-cookie derivation,
  JWT storage, argon2 params, row limits, inference caps, RRF weights.

## Assumptions and limitations

- v0.1, forward-only migrations, no rollback; benchmarks to 100 docs not
  100K (enterprise roadmap); OIDC SSO/backup-restore/SPARQL/per-entity
  export/conflict-state export/chunk-identity/proof-snapshots all missing
  (#564/#902 noted in mcp.md).
- `as_of` FTS recall caveat (right but incomplete), proofs use current
  premise links, business-rule definitions not exported (edited in place).
- Type resolution manual-only (0001 P3a gap: extraction enqueues growth +
  adjudication only; nobody auto-runs retype).
- LLM-adjudicated merges + confidence-bimodal models (self-confidence is
  tone, tiered by subtree instead) — model-trust without independent
  verifier.
- MCP no SQL (`query_data` withheld: production-DB exposure question
  unanswered), no streaming.
- Secrets rotation manual; host-compromise out of threat model.

## Attack surfaces

- Imported documents → extraction → proposals (gated) vs `remember`
  sentence (searchable at once) — second-order prompt injection contained
  by human nod, but sentence itself is immediately retrievable.
- Ontology poisoning (malicious OWL import / confirmed growth term) → all
  downstream extractions + derivations; self-check catches contradictions,
  not malicious-but-consistent axioms.
- Agent over MCP with read token: enumeration via search_neighbors/paths
  within owner's visibility (by design); token theft = owner visibility
  (revocation next-call only).
- Mounted production DBs via Ask-the-Data: read-only gate is depth;
  source-role least-privilege is the real boundary (SECURITY.md says so).
- Default DB password + bind changes; `secret.key` backup discipline.

# Comparison with our project

## UT-01 — Time model

Utopia: two clocks first-class (world `at` + record `as_of`/`before`,
`changes` over record time, close-don't-overwrite, supersede links,
validity precisions, interpreted `holds_*`). Runlith: Event Journal
append-only + task state machine, no valid-time/record-time split for
knowledge. Overlap is append-only instinct; novelty is valid-time
semantics Runlith never designed.

## UT-02 — Provenance and trust

Utopia: source docs + quotes + `document_ids` per fact, confidence,
`recorded/invalidated_at`, derivations→rules→premises, review reasons with
evidence-strength prefixes, human `type_source`, ledger who/when/at-time.
Agent writes gated (proposals). Runlith ADR-0012 wants the same + staleness/
supersede/hash lifecycle; Utopia *ships* most of it (minus staleness scores
and hashes). Closest G10 reference so far.

## UT-03 — Retrieval model

Utopia: Tantivy FTS + pgvector fused RRF + graph walk (entity-facts-as-of,
timeline, neighbors, paths≤3hops, changes) + mounted-DB Ontology2SQL
(BIRD SOTA-claimed). Budgeted extraction retrieval (40/30/30 + ancestors).
Runlith G9: structural/JIT + replaceable retrievers, budgeted/filtered/
explainable. Utopia validates the fused+graph-walk shape; its budgets are
doc-stated, its explainability is citations+proof-links (premise-freshness
caveat noted).

## UT-04 — Identity and lifecycle

Utopia: UUIDs stable per stored object (not across rebuilds), merge moves
name→aliases + facts, revert restores, retype logged with events,
generations via close/link (no generations table evidenced). Runlith G10
needs stable entity IDs + CAPTURE→ARCHIVE + supersede. Utopia's
merge/revert/retype event set is the lifecycle to adapt.

## UT-05 — Conflict governance

Utopia trichotomy (fact-fact / axiom-violation / ontology-defect) +
ontology-first + three choices each + ledgered decision. Runlith: Policy
Engine for actions, Evaluation Port for verification — neither covers
knowledge-conflict adjudication. Utopia fills a governance gap Runlith has
no component for.

## UT-06 — Non-overlap (execution/policy/verification of actions)

Utopia has no Task/Execution/Attempt/Session, no capability approvals, no
sandbox/runner, no action verification (roadmap: execution gate *planned*,
decision reasoning *in development*, business rules #277 open). Its
"agent" is a read-mostly RAG harness + proposals. Runlith G6/G7/G8
differentiators untouched — adoption bounded to knowledge/continuity.

# Bilateral comparison (as in mercury-agent)

## What is better in Runlith

- Durable Task graph + Execution/Attempt/Session split vs document→graph
  pipeline with no action state. Tasks survive provider churn; Utopia
  replays decisions but executes nothing.
- Policy Engine (DENY>REQUIRE_APPROVAL>ALLOW, exact-bound single-use,
  fail-closed, child-subset) vs ontology-axiom governance (data-correct,
  action-blind). Runlith gates *doings*; Utopia gates *knowings*.
- Independent Evaluation Port vs LLM adjudication + self-reported
  confidence (bimodal tone). Runlith verification separates from proposal;
  Utopia adjudicates with the same substrate it extracts with.
- Local-first SQLite + journal + worktree isolation vs server Postgres +
  Docker compose (host-scale, backup=DB+data+key). Runlith runs offline on
  laptop; Utopia needs infra.
- Secret Broker (metadata/backend/delivery, OS keychain, bindings) vs
  sealed-at-rest strings (fixed correctly, but delivery/binding story
  thinner; key-backup discipline on operator).
- Hash-chained audit vs append-only ledger (outlives base — good — but no
  tamper-evidence chain evidenced).
- Provider neutrality (replaceable executors behind ports) vs
  OpenAI-compatible-endpoint coupling (portable across vendors, but LLM in
  extraction/adjudication/derivation path structurally).

## What is worse in Runlith (gaps Utopia exposes)

- No bitemporal knowledge: no valid-time/record-time split, no
  close-don't-overwrite, no supersede links, no `at`/`as_of` reads, no
  changes-over-record-time. Utopia ships all five.
- No ontology-checked memory: no signature enforcement, no
  empty-instead-of-vague discipline, no asserted-overrides-derived, no
  self-check-first. Runlith validation is per-action, not per-fact.
- No agent-write proposal gate: no remember-shape (searchable-now,
  graph-after-nod), no review queue with reasons, no per-agent attribution.
  INV-029 designed, not implemented.
- No conflict trichotomy + ledgered resolution for knowledge (Runlith
  conflicts are task/merge-level, not fact-level).
- No fused retrieval + graph-walk read surface (FTS+vectors+neighbors/
  paths/timeline/changes in one scoped API with citations).
- No decision ledger outliving objects, no deactivation-not-deletion
  identity discipline.
- No measurement bench discipline (fresh-DB-per-run, keyless-where-
  memorized, drop-code counting) for memory quality.

## What is better in Utopia

- Bitemporal close-link ledger with two-clock reads as the default mental
  model (world vs record never folded).
- Ontology-growth loop that never loses wording (OOV → proposed_* →
  confirm → next-batch extraction uses it) + cold-start packs + empty-
  ontology honesty (zero violations reported = truth, not fault).
- Extraction drop-code honesty (11 codes, per-item-not-chunk for malformed
  #127, sentence-name guard #143, costliest-drop documented:
  `attr_domain_mismatch` unrecoverable without re-extract).
- Resolution benchmark narrative (Holmes 14→57 with each layer hiding next;
  redirect-not-close; alias-bridge; equal-type tier) — a merge-correctness
  case study with numbers.
- Deterministic governance output (sorted defects/paths; one-ring-one-
  report; expanded-disjoint dedup) — reviewable by diff.
- MCP read contract done right: scoped PATs, ceiling semantics, invisible-
  without-grant tools, structuredContent versioning with breakage policy,
  RDF export as the *named* compatibility boundary (UI shapes explicitly
  excluded), `as_of`-recall and proof-freshness caveats stated in docs.
- Secrets sealed correctly for v0.1 (nonce-per-seal, idempotent, backfill,
  hard-fail on tamper/wrong-key, key-outside-DB, stated threat model).

## What is worse in Utopia

- Scale/ops: server Postgres + Docker, forward-only migrations, 100-doc
  bench (100K roadmap), manual retype trigger, full-inference rerun
  (no incremental), manual key rotation.
- Trust substrate: LLM extracts + LLM adjudicates + axioms from humans;
  no independent verifier; confidence bimodal; business-rule bodies not
  exported (#902); conflict/review state not exported (#564).
- Completeness: no OIDC, no backup commands, no SPARQL, no per-entity
  export, no SQL-over-MCP, no streaming, no chunk-identity in export,
  proofs use current premises.
- Action blindness: execution gate + decision reasoning still roadmap;
  business rules #277 open; agent memory over MCP (episodes/retrieve)
  roadmap — today's agent surface is read + propose.
- Language split: code comments largely Chinese (Cargo.toml rationale,
  core/secrets/reason docs) — reviewable but slower for non-readers;
  failure diagnostics stored in English by policy (good).

# Identified opportunities

- OPP-01 — Bitemporal fact lifecycle (close+link, supersedes, `at`/`as_of`/
  `before`, changes-over-record-time) as G10 lifecycle shape.
- OPP-02 — Proposal-gated agent writes (remember-shape + review queue +
  per-agent attribution + evidence-strength reasons) as INV-029 reference.
- OPP-03 — Ontology-as-contract checks (signature correction w/ record,
  OOV-empty discipline, asserted-overrides-derived, check-writes-nothing,
  self-check-first) as G10 validation checklist.
- OPP-04 — Conflict trichotomy + ledgered resolution as knowledge-
  governance component Runlith lacks.
- OPP-05 — Decision ledger outliving objects + deactivation-not-deletion
  as audit hardening.
- OPP-06 — Fused retrieval + graph-walk read tools + citation/proof links
  as G9 read-surface reference (with stated caveats).
- OPP-07 — Drop-code counting + fresh-DB bench + keyless-where-memorized
  as memory-quality measurement pattern.

# Recommendations

## R-01 — Adapt bitemporal close-don't-overwrite lifecycle for G10 (P1)

- Proposed change: G10 facts carry valid-time + record-time, corrections
  close + link (`supersedes`), reads support `at`/`as_of`, changes listed
  over record time; never overwrite.
- Problem: OPP-01; INV-029 lifecycle.
- Approach: schema + validation before any retriever work; reimplement in
  Rust/SQLite idiom (validity precisions where needed); staleness/hash
  added per ADR-0012 (Utopia omits both).
- Benefits: replayable decisions ("what held + what we knew"), refreshable
  memory, audit-compatible history.
- Security: filters (INV-008) apply to both clocks + export; supersede
  links preserve attribution.
- Complexity: Medium (owned by G10).
- Risks: two-clock query complexity; index cost; UI must teach two clocks
  (Utopia's `at`-vs-`as_of` confusion warning applies).
- Acceptance: correction test shows closed+linked pair; `at`/`as_of`
  matrix test; overwrite attempt fails.
- Priority: P1 within G10.
- Classification: `NEEDS_ADR` (time model) → `ADAPT`.

## R-02 — Adapt proposal-gated agent writes + review queue (P1 for trust)

- Proposed change: any agent memory write lands searchable-attributed now,
  graph-visible only after human confirm/reject (one-by-one, under source
  sentence, agent-named card, reasoned); scope-ceiling tokens; invisible-
  without-grant tools.
- Problem: OPP-02; INV-029.
- Approach: G10 write path splits record vs assert; review reasons carry
  evidence strengths (cf. `review_reasons` prefix contract); Decisions
  ledgered.
- Benefits: prompt-injection-aware memory (review costs click, ungated
  costs wrong edge); attributable automation.
- Complexity: Medium.
- Risks: review fatigue; queue design must batch + dedup (redirect-not-
  close lesson).
- Acceptance: agent-write-without-nod test shows no graph edge; confirm
  promotes with ledger row; reject leaves reasoned trace.
- Priority: P1 within G10 trust work.
- Classification: `NEEDS_ADR` → `ADAPT`.

## R-03 — Adapt ontology-as-contract validation checklist (P2)

- Proposed change: signature checks on every fact-write path with recorded
  correction-or-empty (never silent, never "related to"); OOV wording
  preserved in proposal fields; asserted overrides derived; checks write
  nothing; self-check runs before fact verdicts; deterministic (sorted)
  output.
- Problem: OPP-03.
- Approach: checklist in G10 validation task; deterministic ordering
  tests; correction counters surfaced (incl. costliest-drop analysis).
- Benefits: vague-free, direction-honest, diff-reviewable knowledge.
- Complexity: Low-Medium.
- Risks: over-strict signatures drop truth (like `attr_domain_mismatch`
  unrecoverable) — needs re-extract path + human override.
- Acceptance: swapped-subject test records correction; OOV test leaves
  predicate empty with term preserved; derived-never-closes-asserted test.
- Priority: P2.
- Classification: `NEEDS_ADR` → `ADAPT` as checklist.

## R-04 — Add conflict trichotomy + ledgered resolution (P2, new governance)

- Proposed change: fact-vs-fact / axiom-violation / ontology-defect tiers
  with 3 choices each, ontology-first presentation, human decision ledgered
  (who/when/at-time, outlives object).
- Problem: OPP-04, OPP-05.
- Approach: new G10 governance task (no Runlith counterpart); redirect-
  not-close for dependent reviews; contradiction-points-upstream rule
  (derivation-vs-assertion → violation row, not silent drop).
- Benefits: resolvable disagreement instead of last-write-wins or silent
  inference loss.
- Complexity: Medium.
- Risks: queue overload; needs batching + pair-once semantics.
- Acceptance: each tier produces ledgered resolution; dependent reviews
  redirect on merge; derived-contradiction surfaces.
- Priority: P2.
- Classification: `NOVEL_GAP` → `ADAPT`.

## R-05 — Use sealed-at-rest + deactivation discipline as validation (P2)

- Proposed change: treat Utopia's seal (`enc:v1:`, nonce-per-value,
  idempotent, backfill, hard-fail) + key-outside-DB + deactivation-not-
  deletion as the checklist any Runlith secrets/identity work is measured
  against.
- Problem: OPP-05.
- Approach: no code (Runlith brokers differently); criteria only.
- Benefits: dump-≠-leak posture + attributable history without resurrected
  accounts.
- Complexity: Low.
- Acceptance: criteria cited in secrets/identity reviews.
- Priority: P2.
- Classification: `ALREADY_IMPLEMENTED` (broker exists) →
  `EXTERNAL_VALIDATION`.

## R-06 — Reference fused retrieval + bench discipline (P3)

- Proposed change: FTS+vectors RRF + graph-walk tools + citations/proofs
  as G9 read-surface sketch (with Utopia's own caveats: `as_of` recall,
  current-premise proofs, caps); fresh-DB-per-run + drop-code counting +
  keyless-where-memorized as memory measurement pattern.
- Problem: OPP-06, OPP-07.
- Approach: notes only; spikes behind Context port after G9 contracts.
- Benefits: explainable reads; honest benchmarks.
- Complexity: Low as notes.
- Risks: Postgres/pgvector/Tantivy stack ≠ Runlith local-first; derive
  shape, not stack.
- Acceptance: notes cite tool/cap/caveat triples + bench rules.
- Priority: P3.
- Classification: `ALREADY_DESIGNED` (replaceable retrieval) →
  `INSPIRED_ONLY`.

# Rejected or inapplicable ideas

- **Adopt Utopia Postgres/pgvector/Tantivy stack as Runlith memory.**
  Reason: conflicts with local-first SQLite + journal monolith, offline
  laptop posture, and replaceable-retriever ADR-0010. Shape only (R-06).
  (REJECT as stack.)
- **Adopt Utopia as execution/policy layer.** Reason: no Task/Execution/
  approvals/sandbox/action-verification; execution gate + decision
  reasoning are roadmap, not code. (REJECT.)
- **Copy `models.rs`/migrations/`reason/*` into Runlith.** Reason:
  prohibited by AGENT.md §16.1; language is same (Rust) but schema/trust/
  ops mismatch stands alone. Reimplement adapted concepts. (REJECT.)
- **Add Utopia as dependency or sidecar.** Reason: reviewed competitor as
  implementation shortcut prohibited (§16.1); supply-chain + authority
  split unjustified for v1. (REJECT.)
- **Treat LLM adjudication/confidence as verification.** Reason: violates
  independent-verification posture (same-substrate judging, bimodal tone
  scores). Adjudication proposes; verification stays separate. (REJECT.)
- **Serve production DBs over agent MCP (query_data shape).** Reason: even
  Utopia withholds it (unanswered borrowed-agent question); Runlith has no
  read-only-to-production story either. (REJECT for v1.)
- **Web console + graph browser as Runlith UI.** Reason: UI follows
  daemon/verification gates; no parity merely for existence (§16.2).
  (REJECT for v1.)

# Proposed implementation plan

Independent, ordered by leverage/cost (no implementation authorized by this
research file):

1. R-01 bitemporal lifecycle (with G10 schema).
2. R-02 proposal gate + review queue (with G10 trust).
3. R-03 contract-validation checklist (with G10 validation).
4. R-04 conflict trichotomy + ledger (new governance task).
5. R-05 seal/deactivation criteria (cite in reviews).
6. R-06 retrieval/bench notes (with/after G9).

Each step needs provenance/validation tests, secrets-redaction keepers,
retrieval-explainability assertions, and docs updates. No step changes
public versioning or cross-context storage reads.

# Evidence and references

Directly verified (raw fetch at pinned SHA
`1a75be63140ff1a94147524352a8b74647830a5e`):

- https://github.com/deeplethe/utopia (landing: 8.2K stars / 1.1K forks /
  383 watchers snapshot, Apache-2.0, 15 topics).
- GitHub Commits API `main` →
  `1a75be63140ff1a94147524352a8b74647830a5e`, 2026-10-10T09:05:25Z,
  `Merge pull request #1181 from deeplethe/dev (Merge dev into main)`.
- GitHub Contents API crates + docs inventories at pinned SHA.
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/Cargo.toml
  (9 crates, sqlx pg+mysql, tantivy+pgvector, AES-GCM/JWT/argon2, oxttl/
  oxrdf parsers, reqwest gzip note with measured 62s→21s).
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/crates/utopia-core/src/lib.rs
  (module map, `review_reasons::NAME_VECTOR` evidence-strength contract).
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/crates/utopia-core/src/secrets.rs
  (AES-256-GCM `enc:v1:`, OnceLock key, seal/open + JSON helpers + 5 tests).
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/crates/utopia-reason/src/ontology.rs
  (8 Defects, check_ontology sorted, MAX_ANCESTRY=32, cycle/unsatisfiable
  walks + 15 tests).
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/web/src/docs/mcp.md
  (per-KB MCP, 10 tools, `at`/`as_of`/`before`, structuredContent schema,
  RDF export boundary, remember-gate, issues #564/#902 caveats).
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/docs/pipeline.md
  (two-phase ingest, 1200/150 chunks, 40/30/30 retrieval, 11 drop codes,
  0.55/0.35 tiers, Holmes 14→57, decisions 0001–0017).
- https://raw.githubusercontent.com/deeplethe/utopia/1a75be63140ff1a94147524352a8b74647830a5e/SECURITY.md
  (sealed creds, loopback DB, least-privilege roles, RO SQL gate,
  deactivation, argon2).

Inferred (not source-verified): exact table/column names (`facts`,
`derived_facts`, `axiom_violations`, `ontology_defects`, `entity_merges`,
`extraction_drops`, `documents.graph_error`), numeric caps (6 chunks,
80/300 facts, 40 changes, 24K doc, 20K derivations), timer defaults,
RRF weights, bench numbers. Marked as such in §Security.

Runlith references: `AGENT.md` §§4/7/16.1/16.2/20, `ARCHITECTURE.md`,
`docs/architecture/INVARIANTS.md` (INV-008/022/029/115),
`docs/architecture/context-engine.md`, `docs/architecture/continuity.md`,
`docs/architecture/adr/ADR-0010-context-derived-state.md`,
`docs/architecture/adr/ADR-0011-provider-independent-context-continuity.md`,
`docs/architecture/adr/ADR-0012-structured-memory-provenance.md`,
`planning/roadmap.yaml` (G9/G10 deferred), `planning/tasks/queue.yaml`,
`crates/AGENTS.md` (no context/memory crate yet).

# Final assessment

Utopia is the strongest G10 reference reviewed so far and the only one
that ships bitemporal, ontology-checked, human-gated knowledge instead of
describing it: close-don't-overwrite time ledger, contract-enforced
extraction, reversible resolution, trichotomic conflicts, proposal-only
agent writes, scoped MCP with a named RDF compatibility boundary, and
honest drop-code/bench/caveat documentation. Its value to Runlith is
lifecycle + governance + measurement wisdom (R-01…R-04) plus audit/cost
notes (R-05/R-06), not stack or code. Its operational model (server
Postgres + Docker, forward-only v0.1 migrations), trust substrate (LLM
extracts + LLM adjudicates), and action blindness (no Task/Policy/Runner/
Evaluation) are inapplicable to Runlith v1's local-first, policy-gated,
provider-neutral monolith.

Recommended next steps: keep this file as evidence; fold R-01/R-02 into
G10 acceptance (time model + proposal gate); bank R-03/R-04 checklists for
G10 validation/governance scope; cite R-05 in secrets reviews; keep R-06
as G9 read-surface/bench notes. Do not import Utopia code or add it as a
dependency.

## Novelty / adoption summary

| Finding | Novelty | Adoption |
|---|---|---|
| Bitemporal close+link lifecycle + two-clock reads | `NEEDS_ADR` (no time model) | `ADAPT` (R-01) |
| Proposal-gated agent writes + review queue + agent attribution | `NEEDS_ADR` | `ADAPT` (R-02) |
| Ontology-as-contract checks (signature/OOV/asserted-first/self-first) | `NEEDS_ADR` | `ADAPT` as checklist (R-03) |
| Conflict trichotomy + ledgered resolution | `NOVEL_GAP` (no knowledge governance) | `ADAPT` (R-04) |
| Sealed-at-rest + deactivation-not-deletion | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` (R-05) |
| Fused retrieval + graph-walk tools + bench discipline | `ALREADY_DESIGNED` | `INSPIRED_ONLY` (R-06) |
| Postgres/pgvector/Tantivy stack as memory engine | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |
| Utopia as executor/policy/verifier, code/dependency import | `CONFLICTS_WITH_ARCHITECTURE` or out-of-scope | `REJECT` |
