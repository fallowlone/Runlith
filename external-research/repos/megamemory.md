# Repository

`0xK3vin/MegaMemory`

- repository: `MegaMemory`
- url: https://github.com/0xK3vin/MegaMemory
- default_branch: `main`
- reviewed_revision: `main@e0bb3c270d7fb4f6f280ae4685e0c538eb225d93`
- commit_sha: `e0bb3c270d7fb4f6f280ae4685e0c538eb225d93`
- review_date: `2026-10-10`
- resource_type: GitHub repository (TypeScript MCP server + docs in README)
- version_observed: npm `1.6.2`; `package.json` `1.6.2`; DB `SCHEMA_VERSION = 4`

# Reviewed revision

Evidence collected 2026-10-10 without a local clone (shell unavailable in this
environment). Sources actually inspected at source level via raw fetch at the
pinned SHA:

- `README.md` via landing page (loop, tools table, install targets, merge flow).
- `package.json` (name, bin, deps, engines).
- `CHANGELOG.md` (1.0.0 entry only at this revision).
- `src/types.ts` (Node/Edge/merge/timeline row types, full).
- `src/db.ts` (full: schema v1-v4, WAL, CRUD, merge-flag helpers, timeline queries).
- `src/embeddings.ts` (full: Xenova/all-MiniLM-L6-v2, 384 dims, cosine top-K).
- `src/tools.ts` (full: makeId, buildNodeWithContext, 9 tool handlers).
- `src/merge.ts` (full: MergeEngine two-pass, suffixes, identical checks).
- `src/index.ts` (full: CLI routing + MCP stdio server + 9 tool registrations + timeline logging).
- `src/timeline.ts` (full: best-effort logger).
- `src/install.ts` (full: 4 targets, managed-file markers, JSONC merge).
- `plugin/megamemory.ts` (full: opencode skill plugin, 4 actions).
- `commands/bootstrap-memory.md` (full bootstrap prompt).
- Latest commit SHA/date/message via GitHub Commits API.

Not inspected at source level: `src/web.ts`, `src/merge-cli.ts`,
`src/stats.ts`, `src/cli-utils.ts`, `web/index.html`, `debug/`, `scripts/`,
tests (`vitest`), `assets/*.svg`. Claims about web explorer internals,
conflict CLI flags, and test coverage below are therefore marked as
README-claimed, not source-verified. No external code was copied into
Runlith, per `AGENT.md` §16.1.

# Research target

Evaluate MegaMemory's persistent concept-graph memory for coding agents
against Runlith's G9 Context Engine + Continuity and G10 Memory Engine
(ADR-0010, ADR-0011, ADR-0012). Identify missing capabilities, improvements,
redesigns, replacements, inspirations, validations, and rejections.

# Executive summary

MegaMemory is a per-project SQLite knowledge graph written by the agent in
its own words. It is an MCP stdio server (`megamemory`, no args) with 9
tools: `understand`, `get_concept`, `create_concept`, `update_concept`,
`link`, `remove_concept`, `list_roots`, `list_conflicts`, `resolve_conflict`.
Storage is `.megamemory/knowledge.db` (libsql, WAL, schema v4). Search is
in-process ONNX embeddings (`Xenova/all-MiniLM-L6-v2`, 384 dims, quantized,
~23MB first download, brute-force cosine, top-K default 10). IDs are
deterministic slugs (`parent/child-name`, lowercased, hyphen-normalized).
Deletes are soft (`removed_at` + reason, history preserved, edges cascade).
Branches merge via two-way file merge with `::left`/`::right` suffixed copies
under a merge-group UUID and AI-verified resolution. A timeline table logs
every tool call. Install targets opencode, Claude Code, Antigravity, Codex.

Relevance to Runlith is medium and narrow: MegaMemory owns the
**agent-authored concept continuity** layer that Runlith's G9/G10
deliberately left deferred (structured memory with provenance/lifecycle,
provider-independent continuity). It does not own durable Task graphs,
policy-gated execution, planning revisions, or independent verification;
those Runlith differentiators are absent here.

Most valuable, in priority order:

1. Concept-not-code discipline + `understand → work → update` loop as a
   usage contract for G10 capture (ADAPT, not a schema replacement).
2. Deterministic slug IDs with parent prefixing as a cheap stable-identity
   convention (ADAPT with Runlith hardening).
3. Two-way `.db` merge with conflict-group + verify-against-code resolution
   as a worktree/branch lesson for any future local knowledge DB (ADAPT
   narrowly, or INSPIRED_ONLY until G10 storage lands).
4. Timeline audit of memory tool calls as validation of Runlith's
   append-only journal instinct (EXTERNAL_VALIDATION).
5. Managed-file installer markers + JSONC-safe config merge as a negative/
   positive lesson for any future `agentctl` setup (INSPIRED_ONLY).

No wholesale adoption is justified. MegaMemory's trust model (agent writes
are truth, no validation gate, no provenance/confidence/staleness fields,
no secrets filtering evidenced), retrieval model (pure embedding top-K, no
token budget, no provenance envelope, no structural JIT retrieval), and
operational model (per-project SQLite file in repo, in-process ONNX model,
brute-force search) conflict with Runlith's ADR-0010/0011/0012 + INV-008/
022/029/115.

# Resource information

- Original URL: https://github.com/0xK3vin/MegaMemory
- License: MIT (`LICENSE`), per landing page.
- Scale signals: ~723 stars, ~75 forks, 5 watchers, 54 commits on `main`,
  4 issues (landing-page snapshot; not a quality claim).
- Language/stack: TypeScript ESM (`src/*`, `tsc` build to `dist/`),
  `@modelcontextprotocol/sdk ^1.12.1`, `@xenova/transformers ^2.17.2`,
  `libsql ^0.5.22`, `zod ^3.24.2`, `picocolors`, `vitest ^4.0.18`,
  Node >= 18.
- Research date: 2026-10-10.

# Core concepts

## 1. LLM is the indexer

No AST parsing, no static analysis. The agent reads code, writes concepts in
its own words (`name/kind/summary/why/file_refs/edges`), and queries before
future tasks. The graph stores features/modules/patterns/decisions, not code
symbols. Quality is bounded by agent discipline, not by extraction precision.

## 2. Concept graph with fixed vocabularies

Six node kinds (`feature/module/pattern/config/decision/component`), five
relations (`connects_to/depends_on/implements/calls/configured_by`), one
parent pointer (max ~3 levels by convention, not by constraint). Children,
outgoing/incoming edges, and parent are always returned together as
`NodeWithContext`.

## 3. Semantic search over concept summaries

Embedding text is `"{kind}: {name} — {summary}"`. Query is embedded the same
way; brute-force cosine over all active nodes with embeddings; top-K sliced
after full sort. `why` and `file_refs` do not enter the embedding. Fast
enough by assertion for <10k nodes; no index, no FTS, no hybrid retrieval.

## 4. Deterministic file-local persistence

One SQLite file per project at `.megamemory/knowledge.db` (or
`MEGAMEMORY_DB_PATH` override). WAL + `busy_timeout 5000` + `synchronous
NORMAL` + `BEGIN IMMEDIATE` migration with re-check + `runWithRetry` on
`SQLITE_BUSY`. Schema v4: `nodes`, `edges`, `timeline`, unique
`(from_id,to_id,relation)`.

## 5. Soft delete as history

`remove_concept` sets `removed_at/removed_reason`, deletes all edges
touching the node, keeps the row. `getNode` filters removed; merge
comparison treats removed-state as content. Already-removed second delete
is an error.

## 6. Branch merge as first-class operation

`megamemory merge main.db feature.db --into merged.db` dedups identical
nodes (content compare incl. `file_refs` deep compare + removed-state),
carries one-sided nodes as clean, and forks conflicting nodes into
`id::left` / `id::right` under one merge-group UUID with `needs_merge=1`.
Edges of conflicting nodes are deferred to pass 2 and remapped to the
suffixed target on the same origin side. Pre-existing conflicts carry
forward. CLI `conflicts` / `resolve --keep left|right|both` plus MCP
`list_conflicts` / `resolve_conflict` (AI must verify against current files,
then write resolved `summary/why/file_refs` + `reason`; base prefers the
active copy, renames back to canonical ID, clears flags in a transaction).

## 7. Timeline as tool audit

Every MCP call logs `tool/params/result_summary/is_write/is_error/
affected_ids` into `timeline` (best-effort, never breaks the tool).
Supports `getTimelineBounds/Entries/Ticks` sampling and `getNodesAtTime/
getEdgesAtTime` point-in-time reads. No hash chain evidenced.

## 8. Installer as product surface

`megamemory install [--target opencode|claudecode|antigravity|codex]`
merges MCP configs without overwriting unmanaged files
(`MegaMemory-managed file` marker + JSONC comment stripping + nested-key
object check), appends `## Project Knowledge Graph` workflow snippet to
`AGENTS.md`/`CLAUDE.md` once, installs opencode skill plugin + two commands
(`/user:bootstrap-memory`, `/user:save-memory`). Critical-step failure
skips remaining steps for that target.

# Architecture and implementation

## Component map (source-verified)

```text
editor/agent (opencode/claude/codex/antigravity)
  -> megamemory (stdio MCP server, 9 tools, zod schemas)
    -> tools.ts (understand/get/create/update/link/remove/roots/conflicts/resolve)
    -> embeddings.ts (Xenova MiniLM-L6-v2, 384f, cosine top-K)
    -> db.ts (KnowledgeDB: nodes/edges/timeline, v4 migrations, tx+retry)
    -> merge.ts (MergeEngine two-pass, ::left/::right, group UUID)
    -> timeline.ts (best-effort logger)
    -> install.ts / merge-cli.ts / web.ts / stats.ts (CLI surfaces)
  -> .megamemory/knowledge.db (WAL SQLite, per-project)
  -> web explorer (megamemory serve, d3-force+Canvas per README; not source-verified)
```

Source paths (names only, not copied): `src/index.ts`, `src/tools.ts`,
`src/db.ts`, `src/embeddings.ts`, `src/merge.ts`, `src/merge-cli.ts`,
`src/types.ts`, `src/cli-utils.ts`, `src/install.ts`, `src/web.ts`,
`src/stats.ts`, `src/timeline.ts`, `plugin/megamemory.ts`,
`commands/bootstrap-memory.md`, `commands/save-memory.md`, `web/index.html`.

## Execution flow

1. `npm install -g megamemory`, first use downloads ~23MB ONNX model.
2. `megamemory install --target opencode`, restart editor.
3. Session start: `list_roots` → roots + children names + stats; empty
   graph returns hint to run bootstrap.
4. Before task: `understand(query, top_k=10)` → embed query → cosine over
   all active embeddings → full `NodeWithContext` per hit (children 1
   level, both edge directions, parent).
5. After task: `create_concept` (slug `parent/name`, parent must exist,
   duplicate is error, embedding regenerated from name/kind/summary) /
   `update_concept` (re-embeds on name/summary change) / `link` (both ends
   must exist, `INSERT OR IGNORE`, duplicate reports already-exists) /
   `remove_concept` (soft delete + edge cascade).
6. Branch divergence: `merge` two `.db` files → clean vs `::left`/`::right`
   conflict copies → `conflicts [--json] [--db]` → agent reads both
   versions + referenced files → `resolve_conflict(merge_group, resolved,
   reason)` → hard-deletes losers, renames winner to canonical, applies
   resolved content + fresh embedding, clears flags.
7. `serve [--port 4321]` visualizes (README-claimed); `stats` summarizes.

## Authentication

No authentication evidenced in inspected sources. DB path is CWD-relative
or env override; any local process with FS access reads/writes the graph.
MCP is local stdio; no user identity, no capability check, no approval
coupling. Web server auth not verified (file not inspected).

## Policy lifecycle

No policy engine. Write guards are structural only: parent must exist,
duplicate IDs rejected, link ends must exist, removed nodes invisible to
reads, empty update is a no-op message. Agent discipline (`understand →
work → update`, max 3 levels, be specific) is documentation, not
enforcement.

## Storage lifecycle

v1: `nodes` + `edges` + indexes. v2: merge columns on both tables. v3:
`timeline` table + indexes. v4: dedup edges + unique
`(from_id,to_id,relation)`. Migrations run under `BEGIN IMMEDIATE` with a
post-lock re-check. `insertNodeAndEdges` validates edge targets exist and
silently skips dangling edges. `renameNodeId` disables FKs inside a
transaction to handle self-referencing `parent_id`.

## Observability

`stats` (nodes/edges/removed), `getKindsBreakdown`, timeline bounds/entries/
ticks, point-in-time node/edge reads, web explorer (claimed). Timeline
`is_error` distinguishes failed calls. No metrics/tracing evidenced in
inspected files.

# Security analysis

## Enforced technically (source-verified)

- Structural referential guards: link/create with missing parent/target
  fails; duplicate create fails; double-remove fails.
- Removed nodes excluded from all read paths (`getNode`, children,
  edges, roots, embeddings, stats); removal cascades edges in a
  transaction.
- Unique edge constraint prevents relation-spam duplicates at the DB
  layer (v4).
- Installer never overwrites unmanaged files; JSONC configs are
  read-then-merge with type checks; critical-step failure stops the
  target.
- Timeline logging is infallible-by-design (try/catch, capped retries),
  so audit failure cannot break memory writes.
- `BUSY` retry + `BEGIN IMMEDIATE` reduces (not eliminates) concurrent-
  writer corruption on a shared `.db` file.

## Documented but not source-verified here

Web explorer access control, `merge-cli --keep both` semantics,
`--db` path traversal handling, model-download integrity (no checksum/
pinning evidenced in `embeddings.ts`), web `index.html` XSS handling,
test coverage of merge/rename/timeline paths.

## Assumptions and limitations

- Agent writes are trusted truth: no provenance, confidence, validator,
  staleness, supersedes-link, or content-hash fields (all required by
  ADR-0012). `created_by_task` is a free string, not a Task ref.
- No secrets hygiene evidenced: no redaction, no sensitivity metadata,
  no INV-008 filter before embedding/storage. `summary/why` can carry
  anything the agent pastes, and embeddings make exact redaction harder.
- Retrieval is embedding-only: `why`/`file_refs`/edges do not influence
  ranking; no FTS/BM25, no structural JIT, no token budget, no provenance
  envelope, no stale-pruning, no provider rendering (all required by
  ADR-0010/ADR-0011).
- IDs are human-slugs: rename of `name` does not rename `id` (update
  changes `name` but keeps slug), collisions after normalization are
  possible (`My Module` vs `my_module` → same slug), no namespace per
  worktree/execution.
- Merge compares file content by ID only, not by history: two
  independently created same-slug concepts with different meanings
  collide; `file_refs` order matters in the current `JSON.stringify`
  compare; edge description `NULL` vs `""` handling is implicit.
- One `.db` per directory: concurrent agents/branches share one file
  with only SQLite locking; no journal, no event sourcing, no hash
  chain.
- In-process ML model in the daemon path: ~23MB download, ONNX runtime
  weight, cold-start latency, platform-specific binary risk — heavy for
  Runlith's local-first single-daemon monolith.

## Attack surfaces

Malicious repo content indexed verbatim into trusted memory; prompt-
injected `resolve_conflict` writing attacker-chosen summaries as
"verified truth"; cross-branch merge smuggling a `::right` concept the
reviewer rubber-stamps; secrets pasted into `summary` persisting in
SQLite + embeddings + timeline `params`; local-process DB tampering
(no signing); web explorer (unverified) exposing the graph over HTTP;
supply chain (`@xenova/transformers`, `libsql`, ONNX model download).

Unverified: whether `serve` binds localhost-only, whether `params` in
timeline redacts secrets, whether the model fetch pins hashes.

# Comparison with our project

## MM-01 — Memory vs Context separation

- External: one graph serves both retrieval ("understand before task")
  and persistence ("record after task"). No Context/Memory split, no
  derived-vs-authoritative distinction.
- Ours: ADR-0010 Context is a disposable materialized view (rebuildable,
  budgeted, filtered, rendered); ADR-0012 Memory is a structured subsystem
  (semantic/episodic/procedural/decision) with provenance and lifecycle
  `CAPTURE → … → ARCHIVE`; unbounded append forbidden.
  `ADR-0010-context-derived-state.md`, `ADR-0012-structured-memory-provenance.md`.
- Difference: MegaMemory optimizes for agent convenience (one call does
  both); Runlith optimizes for losslessness (context loss ≠ state loss)
  and auditability.
- Implication: complementary usage pattern, competing storage semantics.
  Adopt the loop as a contract, not the table as a schema.

## MM-02 — Provenance and trust

- External: `Node` has `created_by_task` string, timestamps, remove
  reason, merge group/labels. No source refs model beyond free-form
  `file_refs: string[]`, no confidence, no `last_verified_at`, no
  supersedes links, no content hash, no validator identity.
- Ours: every entry requires source refs, confidence, created/verified
  timestamps, supersedes links, staleness, content hash where applicable;
  agent annotations are untrusted until provenance/validation policy
  promotes them (INV-029); secrets never enter context/logs/events
  (INV-008).
- Difference: MegaMemory records *what the agent claims*; Runlith
  requires *why it should be believed*.
- Implication: direct schema gap. Any G10 adapter inspired by MegaMemory
  must add provenance columns first.

## MM-03 — Retrieval model

- External: pure dense retrieval (MiniLM 384, cosine, top-K). No budget,
  no plan, no dedup/prune, no cache boundaries, no explainability ("why
  did this file enter context?").
- Ours: Context Engine owns planning, budgeting, JIT structural
  retrieval, dedup, stale pruning, compaction, security filtering,
  provider rendering, cache planning, observability; default is
  structural JIT, whole-repo packing is a bounded fallback (INV-115);
  provider compaction is optimization, never persistence (INV-022).
  `context-engine.md`, `continuity.md`.
- Difference: MegaMemory answers "what is semantically similar?";
  Runlith must answer "what is authorized, fresh, budgeted, and
  explainable?".
- Implication: embedding top-K is at most one replaceable retriever
  behind the Context port (ADR-0010 explicitly allows optional
  embeddings), never the engine.

## MM-04 — Identity and lifecycle

- External: slug IDs (`parent/name`), soft delete with reason, merge
  groups with branch labels, timeline audit, point-in-time reads.
- Ours: Task/Execution/Attempt/Session identities are distinct and
  stable; memory lifecycle is a state machine with dedup/link/validate/
  refresh/supersede/archive; no graph DB in v1 (relational tables
  suffice until a graph workload justifies an adapter).
- Difference: MegaMemory's slug + parent-prefix convention is pragmatic
  and readable; Runlith's lifecycle is rigorous but unimplemented
  (G9/G10 deferred, no memory tables in `crates/` evidenced — only test
  `MemoryBackend` doubles for secrets).
- Implication: slug convention is adaptable with hardening (collision
  handling, immutable IDs on rename, worktree namespacing); lifecycle
  state machine stays canonical.

## MM-05 — Branch/worktree story

- External: strongest part. Two-pass merge, identical/one-sided/conflict
  trichotomy, `::left`/`::right` + group UUID, edge remap by origin
  side, pre-existing conflict carry-forward, verify-against-code
  resolution (not blind side-pick), transactional rename-back.
- Ours: worktree isolation exists for execution; no memory-branch merge
  design evidenced; Event Journal is the durability pattern, not `.db`
  file merge.
- Difference: MegaMemory solves "two agents learned different things on
  two branches"; Runlith has not yet faced it because G10 storage does
  not exist.
- Implication: narrow, high-value lesson to bank for G10: if memory ever
  lives in a per-worktree SQLite file, specify merge before shipping.

## MM-06 — Installer/UX discipline

- External: `understand → work → update`, session-start `list_roots`,
  bootstrap L150-biggest-files + README walk, `save-memory` command,
  managed-file markers, JSONC-safe merge, per-target critical steps.
- Ours: no equivalent bootstrap/save loop; agent workflow is governed
  by Task/Plan/Policy contracts, not by a memory loop.
- Difference: vocabulary differs, intent converges (continuity across
  sessions without transcript-as-truth).
- Implication: process inspiration for G9/G10 dogfooding, not a contract
  import.

## MM-07 — Non-overlap (execution/policy/verification)

- External: no scheduler, no isolation backend, no policy engine, no
  planner port, no evaluation/verification, no secrets broker, no audit
  chain evidenced.
- Ours: Task/Execution/Attempt/Session, `IsolationPlan → IsolationBackend
  → EnforcementReport`, deterministic layered Policy, PlannerPort/
  PlanChangeSet, G8 claim/effect verification, hash-chained audit.
- Implication: no lesson to import; confirms Runlith differentiation
  (durable coordination + gated execution + verification across
  providers/time).

# Bilateral comparison (as in mercury-agent)

## What is better in Runlith

- Provenance-first memory design (ADR-0012): source refs, confidence,
  created/verified timestamps, supersedes links, staleness, content hash,
  lifecycle `CAPTURE → … → ARCHIVE`. MegaMemory has none of this —
  only a free-string `created_by_task`.
- Context/Memory separation (ADR-0010/0011): disposable budgeted
  materialized view vs durable structured store; loss of prompt never
  means loss of state (INV-022). MegaMemory merges both into one graph.
- Trust gate (INV-029): agent notes are untrusted until validated.
  MegaMemory treats agent writes as truth with no promotion policy.
- Secrets hygiene (INV-008): secrets never enter context/logs/events;
  brokered credentials with OS store + approval atomicity. No redaction
  or sensitivity metadata evidenced in MegaMemory; `summary/why` and
  embeddings can persist pasted secrets.
- Retrieval contract: structural JIT default, token budget, dedup/stale
  pruning, security filtering, explainability, cache planning (INV-115).
  MegaMemory is embedding-top-K only with no budget or explanation.
- Durability: SQLite + Event Journal (hash-chained, append-only) vs a
  single per-project `.db` file with only SQLite locking and a
  non-chained `timeline` table.
- Execution/verification differentiation: Task/Execution/Attempt/Session,
  layered Policy, PlannerPort, G8 verification — all absent in
  MegaMemory (MM-07).

## What is worse in Runlith (gaps MegaMemory exposes)

- No shipped memory loop: MegaMemory's `understand → work → update` +
  `list_roots` at session start + `/bootstrap-memory` + `/save-memory`
  is a working continuity habit. Runlith G9/G10 are deferred; no
  equivalent bootstrap/save workflow exists.
- No prompt-friendly memory IDs: MegaMemory slugs (`parent/name`) are
  readable in prompts. Runlith has no memory entity IDs at all yet.
- No branch-merge story for knowledge: MegaMemory's identical /
  one-sided / conflict-fork trichotomy with `::left`/`::right` + group
  UUID + verify-against-code resolution is the reference design Runlith
  lacks for any future file-local memory.
- No dense-retrieval option: MegaMemory ships paraphrase-tolerant search
  (`auth` vs `login`) out of the box. Runlith's structural/JIT
  retrieval is designed but unimplemented, with embeddings only listed
  as an option.
- Installer safety shape: managed-file markers, JSONC-safe merge, never
  overwrite unmanaged files, critical-step gating — small but real UX
  hygiene Runlith has no equivalent of yet.

## What is better in MegaMemory

- Shipped minimalism: one binary, 9 MCP tools, per-project `.db`,
  ~23MB ONNX model, no external API keys — install-to-value in minutes
  across 4 editors (opencode/Claude/Codex/Antigravity).
- Concept-not-code discipline: fixed 6-kind / 5-relation vocabulary +
  max-3-levels convention keeps the graph useful instead of exhaustive.
- Deterministic readable IDs with parent scoping; duplicate/orphan
  guards at write time (`parent must exist`, `both link ends must
  exist`, duplicate create fails loudly).
- Branch merge engine (strongest part): two-pass node-then-edge insert
  (FK-safe), content compare incl. `file_refs` deep compare +
  removed-state, origin-side edge remap, pre-existing conflict
  carry-forward, transactional rename-back to canonical ID.
- Resolution rule that forbids blind side-picking: AI must verify
  against current files and write resolved content + `reason`.
- Best-effort timeline audit that can never break the tool call;
  point-in-time node/edge reads.

## What is worse in MegaMemory

- Trust model: agent claims are truth. No provenance, confidence,
  validator, freshness, supersedes, or hash — i.e. everything ADR-0012
  exists to require. Prompt injection into memory becomes persistent
  "knowledge".
- Retrieval model: dense-only ranking over `kind+name+summary`; `why`,
  `file_refs`, and edges do not influence ranking. No FTS/BM25, no
  budget, no filtering, no explainability, brute-force ceiling (~10k
  nodes by assertion).
- Identity fragility: rename of `name` keeps the old slug (stale ID);
  normalization collisions (`My Module` vs `my_module`) possible; no
  worktree/execution namespacing; FK-OFF rename transaction is a
  cautionary pattern, not a model.
- Merge-by-ID fragility: independently created same-slug concepts with
  different meanings collide; `file_refs` order affects the current
  `JSON.stringify` compare; `NULL` vs `""` edge-description handling is
  implicit.
- Storage/ops: in-repo SQLite with file locking only (concurrent agents
  race); in-process ONNX weight in the daemon path (download integrity
  unpinned, cold start, platform binaries); web `serve` surface
  unverified (bind scope/auth/XSS unconfirmed); no policy engine, no
  isolation, no verification, no secrets broker.

# Identified opportunities

- OPP-01 — Agent memory loop as an explicit G10 usage contract.
  Problem: future memory storage without a write discipline becomes
  unbounded append / transcript dump.
- OPP-02 — Readable stable IDs with parent scoping for memory entities.
  Problem: UUIDs are opaque in prompts; slugs are prompt-friendly but
  need collision/rename semantics.
- OPP-03 — Branch-aware merge for any file-local knowledge store.
  Problem: per-worktree `.db` files cannot be git-merged; silent
  last-writer-wins loses learnings.
- OPP-04 — Provenance-first schema (the actual G10 gap MegaMemory
  exposes by omission). Problem: agent claims without source/confidence/
  freshness cannot be trusted or refreshed.
- OPP-05 — Embedding retriever as one replaceable Context port
  implementation. Problem: structural/JIT retrieval may miss
  paraphrased concepts ("auth" vs "login") that dense search catches.
- OPP-06 — Bootstrap/save commands as dogfood workflow. Problem: empty
  memory stays empty without a guided first pass.
- OPP-07 — Timeline-style tool audit already exists as Event Journal
  instinct; MegaMemory's per-tool `is_write/is_error/affected_ids` is
  validation, not a new gap.

# Recommendations

## R-01 — Adapt the concept-loop as G10 process, not schema (P2)

- Proposed change: when G10 lands, require `understand → work → update`
  phrasing in the memory workflow docs: query before task, record
  specific concepts after task (param names, defaults, file paths,
  rationale), max ~3 levels, concepts-not-code.
- Problem: OPP-01, OPP-06.
- Approach: documentation + bootstrap/save command equivalents inside
  `agentctl`; no MegaMemory dependency, no table import. Storage follows
  ADR-0012 (`memories/memory_links/entities/entity_links`), not
  MegaMemory's `nodes/edges`.
- Benefits: bounded, useful memory from day one; prevents
  transcript-as-memory drift.
- Security: must pass INV-008 (no secrets into memory/context) and
  INV-029 (agent notes untrusted until validated) — both absent in
  MegaMemory and non-negotiable in Runlith.
- Complexity: Low (docs + workflow; storage is the real cost, owned by
  G10).
- Risks: agent non-compliance without enforcement; needs sampling
  review, not blind trust.
- Acceptance: G10 task shows pre-task query + post-task record with
  file refs; unbounded append test fails; secrets-paste test fails
  closed.
- Priority: P2 (process only; storage/scheduling after G8).
- Classification: `ALREADY_DESIGNED` (loop is new phrasing of accepted
  lifecycle) → `ADAPT` as workflow, `REJECT` as schema.

## R-02 — Adapt slug-ID convention with Runlith hardening (P3)

- Proposed change: if G10 entities need prompt-visible IDs, use
  `parent/name-slug` readability on top of immutable internal keys;
  normalize once, never rename the key on `name` change; disambiguate
  collisions with suffixes; namespace by worktree/execution where
  needed.
- Problem: OPP-02.
- Approach: ID formatter behind the memory port; property tests for
  normalization collisions (`My Module` vs `my_module`), rename
  stability, and FK-safe rename (cf. MegaMemory's FK-OFF rename
  transaction as a cautionary example).
- Benefits: readable prompts without identity churn.
- Complexity: Low.
- Risks: slug collisions masking distinct concepts; over-nesting past
  3 levels.
- Acceptance: rename test keeps ID stable; collision test yields
  distinct IDs; children resolve under parent prefix.
- Priority: P3.
- Classification: `NOVEL_GAP` (no memory IDs exist) → `ADAPT`.

## R-03 — Bank the two-way `.db` merge design for future file-local memory (P3, design-only)

- Proposed change: record MegaMemory's merge trichotomy (identical /
  one-sided-clean / conflict-fork with `::left`/`::right` + group UUID +
  origin-side edge remap + verify-against-code resolution) as the
  reference design if G10 ever ships per-worktree SQLite memory.
- Problem: OPP-03.
- Approach: design note only; no code until G10 storage ADR selects a
  file-local backend. If selected, reimplement inside Runlith contracts
  (journal-compatible, hash-chained, provenance-preserving); do not
  import `merge.ts`.
- Benefits: avoids last-writer-wins learning loss across branches.
- Complexity: Medium if built; Low as a banked design.
- Risks: file-merge is the wrong primitive if G10 lands on
  journal/event-sourced memory (then this design is moot — prefer
  event merge over file merge).
- Acceptance: design note cites trichotomy + resolution rule (verify,
  don't side-pick) + transactional rename-back.
- Priority: P3.
- Classification: `NEEDS_ADR` → `INSPIRED_ONLY` until storage ADR.

## R-04 — Require provenance columns MegaMemory omits (P1 for G10 scope)

- Proposed change: treat MegaMemory's missing fields as G10 acceptance
  criteria: every memory entry carries source refs, confidence,
  created/verified timestamps, supersedes links, staleness, content hash
  where applicable; lifecycle `CAPTURE → NORMALIZE → DEDUPLICATE → LINK
  → VALIDATE → USE → REFRESH/SUPERSEDE → ARCHIVE`.
- Problem: OPP-04; INV-029.
- Approach: schema + validation policy before any embedding work.
  Agent writes stay untrusted until promoted.
- Benefits: memory that can be refreshed, superseded, and audited
  instead of believed.
- Complexity: Medium (owned by G10).
- Acceptance: memory write without source refs fails validation;
  stale entry surfaces refresh; supersede preserves link.
- Priority: P1 within G10 (not a standalone task before G10).
- Classification: `ALREADY_DESIGNED` (ADR-0012) → `EXTERNAL_VALIDATION`
  (MegaMemory's omission validates why the ADR requires these fields).

## R-05 — Spike embedding retriever behind the Context port only (P3, after G9 contracts)

- Proposed change: bounded spike of MiniLM-class dense retrieval as one
  optional retriever (with FTS5/BM25/AST/symbol alternatives) behind
  the ADR-0010 retrieval port; embeddings never become the context
  authority, budget, or filter.
- Problem: OPP-05.
- Approach: replaceable adapter; measure paraphrase recall vs cost
  (model download, cold start, ONNX weight, brute-force ceiling);
  keep structural JIT default + INV-115 fallback; enforce INV-008
  filtering before embedding and before prompt render.
- Benefits: catches paraphrased concepts cheaply if the spike proves
  out.
- Security: model supply chain (no pinning evidenced upstream) needs
  its own review; embeddings of secrets must be impossible by
  construction, not by agent discipline.
- Complexity: Medium (spike Low).
- Risks:检索质量 without provenance ranking; <10k-node brute force
  does not scale to monorepo memory; platform binary variance.
- Acceptance: spike reports recall/latency/binary-size vs FTS baseline
  on a fixed corpus; no context-budget or policy change.
- Priority: P3.
- Classification: `ALREADY_DESIGNED` (optional embeddings in ADR-0010)
  → `INSPIRED_ONLY`.

## R-06 — Use installer markers + bootstrap discipline as implementation hygiene (P3)

- Proposed change: if Runlith ever ships a setup/bootstrap command,
  copy the safety shape (managed-file marker, read-then-merge, never
  overwrite unmanaged files, critical-step gating), not the content.
- Problem: OPP-06.
- Approach: checklist in the relevant ADR/task; JSONC comment handling
  and TOML section replace are the concrete tricks worth stealing.
- Benefits: setup that composes with user configs instead of clobbering
  them.
- Complexity: Low.
- Acceptance: setup test proves unmanaged file untouched + managed file
  updated + invalid JSON left unchanged with error.
- Priority: P3.
- Classification: `ALREADY_DESIGNED` in spirit → `INSPIRED_ONLY`.

# Rejected or inapplicable ideas

- **Replace G10 schema with MegaMemory `nodes/edges` tables.**
  Reason: drops provenance/confidence/staleness/supersedes/hash and the
  four-type model (semantic/episodic/procedural/decision) that ADR-0012
  requires; trades auditability for convenience. (REJECT /
  CONFLICTS_WITH_ARCHITECTURE.)
- **Adopt in-process ONNX embeddings as the Context Engine.**
  Reason: contradicts ADR-0010 (retrieval is replaceable, budgeted,
  filtered, explainable; structural JIT default) and adds heavyweight
  model dependency to the daemon path. At most one optional retriever
  (R-05). (REJECT as engine; INSPIRED_ONLY as adapter spike.)
- **Adopt per-project `.megamemory/knowledge.db` in-repo storage.**
  Reason: repo-committed SQLite with only file locking conflicts with
  Runlith's SQLite + Event Journal durability, hash-chained audit, and
  secrets-hygiene (INV-008) posture. Revisit only inside G10 storage
  ADR. (REJECT for now.)
- **Treat agent-written concepts as trusted truth (no validation gate).**
  Reason: violates INV-029 directly. (REJECT.)
- **Import MegaMemory as a dependency or copy `merge.ts`/`db.ts`/`tools.ts`.**
  Reason: prohibited by AGENT.md §16.1; license is permissive (MIT) but
  architecture/trust-model mismatch stands on its own. Reimplement
  adapted concepts inside Runlith contracts. (REJECT.)
- **Timeline table as Runlith audit.**
  Reason: weaker than the hash-chained Event Journal (no tamper
  evidence evidenced); useful as a per-tool audit shape only.
  (REJECT as replacement; EXTERNAL_VALIDATION as instinct.)
- **Web explorer (`serve`) as memory UI.**
  Reason: unauthenticated local HTTP surface (unverified) with no
  provenance/secrets story; Runlith UI comes after daemon/verification
  gates. (REJECT for v1.)

# Proposed implementation plan

Independent, ordered by leverage/cost (no implementation authorized by this
research file):

1. R-04 provenance acceptance (fold into G10 scope; no standalone code).
2. R-01 memory loop wording (docs-only, with G10).
3. R-02 slug-ID hardening (with G10 entity design).
4. R-05 embedding retriever spike (after G9 retrieval contracts).
5. R-03 file-merge design note (bank; build only if G10 picks file-local).
6. R-06 installer hygiene checklist (whenever a setup command is scoped).

Each step needs provenance/validation tests, secrets-redaction keepers,
retrieval-explainability assertions, and docs updates. No step changes
public versioning or cross-context storage reads.

# Evidence and references

Directly verified (raw fetch at pinned SHA `e0bb3c270d7fb4f6f280ae4685e0c538eb225d93`):

- https://github.com/0xK3vin/MegaMemory (landing: description, stars/
  forks/watchers snapshot, MIT, topics).
- GitHub Commits API `main` → `e0bb3c270d7fb4f6f280ae4685e0c538eb225d93`,
  2026-05-03, `chore: bump version to 1.6.2`.
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/package.json
  (npm `1.6.2`, MCP SDK, xenova, libsql, zod, Node >= 18).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/CHANGELOG.md (1.0.0 entry).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/types.ts (Node/Edge/ConflictGroup/MergeResult/timeline rows).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/db.ts (`SCHEMA_VERSION = 4`, WAL, v1–v4 migrations, tx+retry,
  node/edge CRUD, merge-flag helpers, timeline queries, FK-OFF rename).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/embeddings.ts (`Xenova/all-MiniLM-L6-v2`, 384 dims, `kind: name —
  summary`, cosine top-K).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/tools.ts (`makeId` slug, `buildNodeWithContext`, 9 handlers).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/merge.ts (`::left`/`::right`, content compare, two-pass engine,
  origin-side edge remap, pre-existing carry-forward).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/index.ts (CLI routing, 9 MCP tools with zod, timeline logging,
  `MEGAMEMORY_DB_PATH` default `.megamemory/knowledge.db`).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/timeline.ts (best-effort logger).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/src/install.ts (4 targets, managed markers, JSONC merge, critical
  gating).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/plugin/megamemory.ts (opencode `query/record/overview/merge` skill).
- https://raw.githubusercontent.com/0xK3vin/MegaMemory/e0bb3c270d7fb4f6f280ae4685e0c538eb225d93/commands/bootstrap-memory.md (L150 + README bootstrap).

Inferred (not source-verified): web explorer behavior, merge-cli `--keep`
semantics, test coverage, model-download integrity, serve bind scope.
Marked as such in §Security.

Runlith references: `AGENT.md` §§4/7/16.1/16.2, `ARCHITECTURE.md`,
`docs/architecture/INVARIANTS.md`
(INV-008/022/029/115), `docs/architecture/context-engine.md`,
`docs/architecture/continuity.md`,
`docs/architecture/adr/ADR-0010-context-derived-state.md`,
`docs/architecture/adr/ADR-0011-provider-independent-context-continuity.md`,
`docs/architecture/adr/ADR-0012-structured-memory-provenance.md`,
`planning/roadmap.yaml` (G9/G10), `planning/tasks/queue.yaml`.

# Final assessment

MegaMemory is the most directly G9/G10-adjacent external reference so
far: a shipped, minimal, agent-usable concept memory with a clear loop,
readable IDs, and a genuinely thoughtful branch-merge story. Its value to
Runlith is process and edge-case wisdom (loop discipline, slug
conventions, merge trichotomy, installer safety), not storage or
retrieval architecture. Its trust model (agent writes are truth),
retrieval model (embeddings-only, no budget/provenance/filtering), and
operational model (in-repo SQLite + in-process ONNX) are inapplicable to
Runlith v1's provenance-first, budget-aware, secrets-closed monolith.

Recommended next steps: keep this file as evidence; fold R-04 provenance
criteria into G10 acceptance; adopt R-01 loop wording when G10 lands;
bank R-03 merge design; spike R-05 embeddings only behind the Context
port after G9 contracts. Do not import MegaMemory code or add it as a
dependency.

## Novelty / adoption summary

| Finding | Novelty | Adoption |
|---|---|---|
| Concept loop `understand → work → update` + bootstrap/save | `ALREADY_DESIGNED` (lifecycle) | `ADAPT` as workflow (R-01) |
| Readable slug IDs with parent prefix | `NOVEL_GAP` (no memory IDs) | `ADAPT` with hardening (R-02) |
| Two-way `.db` merge + verify-against-code resolution | `NEEDS_ADR` | `INSPIRED_ONLY` until storage ADR (R-03) |
| Provenance/confidence/staleness requirements | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` (R-04) |
| Embedding top-K as optional retriever | `ALREADY_DESIGNED` | `INSPIRED_ONLY` spike (R-05) |
| Managed-file installer + JSONC merge | `ALREADY_DESIGNED` | `INSPIRED_ONLY` (R-06) |
| Timeline per-tool audit | `ALREADY_IMPLEMENTED` (journal instinct) | `EXTERNAL_VALIDATION` |
| `nodes/edges` schema as G10 replacement | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |
| In-process ONNX as context engine / in-repo DB / trusted writes | `CONFLICTS_WITH_ARCHITECTURE` or out-of-scope | `REJECT` |
