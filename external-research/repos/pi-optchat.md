# Repository

`jonaslsaa/pi-optchat`

- repository: `pi-optchat`
- url: https://github.com/jonaslsaa/pi-optchat
- default_branch: `main`
- reviewed_revision: `main@0aa45406325f610f3106510218d8cc69deb0cccc`
- commit_sha: `0aa45406325f610f3106510218d8cc69deb0cccc`
- review_date: `2026-10-10`
- resource_type: GitHub repository (TypeScript Pi extension + docs in README)
- version_observed: npm `0.8.3`; `package.json` `0.8.3`; Node `>=22.19.0`

# Reviewed revision

Evidence collected 2026-10-10 without a local clone. Sources actually
inspected at source level via raw fetch at the pinned SHA:

- `README.md` via landing page (memory, profiles, subagents, import,
  connected windows, storage, settings, recipe deltas).
- `package.json` (name, version, Pi extension entry, scripts, Pi peer deps).
- Git tree via GitHub Trees API at pinned SHA (30 `src/` files, `src/import/`,
  30+ tests, docs).
- `src/memory.ts` (full: NODE/VIEW/CAP/PAGE constants, Sawtooth, Memory,
  mostDue, zoom/search/date, settle, queueParent).
- `src/store.ts` (full: Store/Lease/FileStore, atomicWrite, appendJson,
  torn-write handling, single-writer enforcement).
- `src/prompts.ts` (full: COMPACT/MASTER/VIEW_DOC/SUBAGENT/CONTINUITY/
  SEARCH_DOC/IMPORT_GUIDANCE, compaction task, tooLong retry, HANDOFF).
- `src/tools.ts` (full: zoom/date/search tools, run transcript paging,
  image attachments).
- `src/compactor.ts` (full: createCompressor, primeFirst cache primer,
  5-try size loop).
- `src/cache.ts` (full: splitView 4-line blocks, Anthropic 4-mark scheme,
  status-line cache handling).
- `src/profiles.ts` (full: profilePath validation, lockProfile socket,
  holdProfile lease-or-lock, Windows pipe hash).
- `src/import/sources.ts` (full: scanLocal/scanChatGPT/scanClaudeMemories,
  readConversation/readMemory, receipt/dedup, branch labels, replay filters).
- `docs/victor-recipe.md` (recipe reference + implementation mapping).
- Latest commit SHA/date/message via GitHub Commits API.

Not inspected at source level: `src/index.ts`, `src/agents.ts`,
`src/transcript.ts`, `src/window-bridge.ts`, `src/connected-window.ts`,
`src/handoff.ts`, `src/runs.ts`, `src/inbox.ts`, `src/usage.ts`,
`src/browser.ts`, `src/import/job.ts`, `src/import/ui.ts`, `test/`
(except via README claims). Claims about subagent scheduling details,
connected-window UX, and test coverage below are therefore marked as
README-claimed or partially verified. No external code was copied into
Runlith, per `AGENT.md` §16.1.

# Research target

Evaluate pi-optchat's persistent summary-tree memory for coding agents
against Runlith's G9 Context Engine + Continuity and G10 Memory Engine
(ADR-0010, ADR-0011, ADR-0012). Identify missing capabilities,
improvements, redesigns, replacements, inspirations, validations, and
rejections.

# Executive summary

pi-optchat is a Pi extension implementing Victor Taelin's OptChat recipe:
one endless chat per profile, remembered through a binary summary tree
instead of compaction. Every message is logged (dated JSONL, no reasoning)
and compressed into 512-byte lines; adjacent lines merge in pairs. Each turn
starts from fresh context holding a bounded memory view (128 KB, sawtooth
merged to 64 KB); the agent uses `zoom`/`date` (and optional `search`) to
read originals.

Storage is `~/.optchat/profiles/<name>/` (`main/` + `tree/` JSONL,
`view.json`, `images/`, `config.json`, `AGENTS.md`, `runs/`,
`usage.jsonl`), each profile a local Git repo committed per turn. Memory
reaches disk through one seam, `Store` (`src/store.ts`); `FileStore` is the
only store today, with a `Lease` (take/heartbeat/release) slot for a future
Postgres-class backend. Profiles are single-writer via OS socket lock
(POSIX `0600` socket in profile, Windows named-pipe hash) or store lease.

Subagents run inside the Pi process via Pi SDK with frozen memory view,
read-only zoom/date, `tell`/`tell_parent` steering, journaled reports, and
resume-after-restart. Connected windows turn a second Pi on the same profile
into a live subagent over a local user-restricted socket, closed by handoff
summary. Imports bring history from Claude Code (conversations + auto-memory
topic files), Codex, Pi/OMP, or ChatGPT export as `Historical` records with
stable receipts, dedup, generational switch, and paused-resume.

Relevance to Runlith is high and narrow: pi-optchat owns the **durable
conversational continuity** layer Runlith's G9/G10 deliberately left
deferred (long-horizon memory that survives sessions, with bounded working
view + full-fidelity drill-down). It does not own durable Task graphs,
policy-gated execution, planning revisions, or independent verification;
those Runlith differentiators are absent here.

Most valuable, in priority order:

1. Summary-tree with sawtooth bounded view + saved (not refolded) view as
   the continuity primitive (ADAPT as design reference for G9/G10).
2. Single-seam `Store` with atomic view writes + torn-append repair +
   single-writer refusal as the file-local durability shape (ADAPT).
3. Import pipeline's historical-receipt + replay-filter + alternate-branch
   discipline as the provenance-negative-example turned positive (ADAPT
   narrowly; Runlith already requires more).
4. Cache-conscious view layout (4-line blocks, staggered marks, primer gate)
   as a cost lesson for any cached continuity view (INSPIRED_ONLY).
5. Socket-lock profile exclusivity + journaled subagent reports + handoff
   pattern as execution-recovery wisdom (INSPIRED_ONLY).

# Resource information

- Type: Pi extension (TypeScript ESM, `src/index.ts` as Pi extension entry).
- Runtime: Node `>=22.19.0`, Pi `1.0.2+`, Git. Tested macOS; offline tests
  also Linux/Windows.
- License: MIT (`LICENSE`, `THIRD_PARTY_NOTICES.md` attributes
  Taelin recipe + OptMem).
- Distribution: `pi install npm:pi-optchat` (npm `pi-package` keyword) or
  `pi install git:github.com/jonaslsaa/pi-optchat`; GitHub release → npm
  trusted publishing (`publish.yml` checks tag == `package.json`).
- Upstream identity: independent Pi implementation of Taelin's recipe, not
  Victor's official OptChat (README Credits). Revised gist 2026-10-08 calls
  his version UniiChat.

# Core concepts

## 1. Log is authoritative, tree and view follow

`src/memory.ts`: `root: Entry[]` is truth; `tree: Map<key,Summary>` and
`chat: Sawtooth` derive from it. View lines tile the log oldest-first
(`id+n|text`). Constructor validates contiguity (`entry.i === index`) and
refuses to mutate a broken log. This matches Runlith's canonical-state rule
(AGENT.md §20) in spirit: one owner, derived views rebuildable.

## 2. Fixed-size summary nodes with ceiling-not-target discipline

`NODE = 512` bytes; compactor asked for 512, profile tolerates 640
(`summaryAcceptBytes`) if smaller than source. Prompts (`COMPACT`) state
limit as ceiling, value-ordered (user words > lasting effects > findings >
tool steps), bare approvals recorded as decisions, tool results described by
shown output not guessed cause. A/B notes (#129: instructions add nothing to
compactor; #112/#101: placeholder-instead-of-cut scores higher) are cited in
README — process evidence, not just claims.

## 3. Sawtooth bounded view with most-due merges

`VIEW = 128_000`, merged in one batch to `64_000` when over `high`.
`mostDue` picks oldest-ended sibling pair whose parent is built (recipe
§3.2 / Taelin rollback order). Between batches view only grows at end, so
each turn reads last view from cache. Saved to `view.json`, reloaded if it
still tiles from start, else rebuilt with warning — refold avoided because
refold misses cache.

## 4. Compactions get their own narrower view

`compaction: Sawtooth(budget/4, budget/8)` = 32 KB → 16 KB, re-fit whenever
chat merges. `compactionView(part)` ends at node boundary but shows the
boundary line as `(not summarized yet: zoom it)` placeholder instead of
cutting (A/B #101: cutting hid short built lines after it, e.g. echo's own
tool call). Up to 8 compactions run at once on one cached view; message node
starts once fewer than `jobs` lines before it are unbuilt; merge once both
halves built — scheduler never scans tree (`unbuilt` set + `merges` map).

## 5. Zoom/date/search as the only navigation

`zoom(id,n)`: `n=1` whole message (paged at 25 KB with surrogate-safe
slicing if longer, or if offset/limit given); `n>1` power-of-2 opens the two
halves (must be summarized). `date(id)` returns message time. `search`
(optional, off by default): case-insensitive plain-text over originals only,
never summaries, skipping logged zoom/search calls; 20 hits newest-first
with `id (in line) · date · kind: snippet`, `before:` pages back. View doc
tells model zoom(-and-search) is the only allowed mechanism — prompt-level
enforcement, not technical.

## 6. Store seam with file-local correctness

`src/store.ts`: `Store { load/append/appendNode/saveView/putImage/image,
lock? }`. `FileStore`: `main/<day>.jsonl` + `tree/<day>.jsonl` appended
with fsync per line; `view.json` + images atomically replaced (tmp +
fsync + rename + dir fsync, POSIX-only parts skipped Windows). Torn write
(missing trailing newline) repaired on next append; cut-short write
truncated away. `lastSeenBytes` + `checkLog` refuse interleaved writers
(`Another process wrote …; nothing was written`). Images deduped by hash,
shrunk to 2048px/~1.5 MB via Pi resizer; log holds `[image <hash>]`.

## 7. Profiles as isolation + settings scope

Lowercase `[a-z0-9_-]{1,64}` names under `~/.optchat/profiles/`
(`OPTCHAT_HOME` override). Per-profile `AGENTS.md` (appended last, wins),
`config.json` (compactor + subagent model/thinking + numeric settings),
`runs/`, `usage.jsonl`, `memory.html` snapshot. Lock: POSIX socket file in
profile (`0600`) or Windows pipe `optchat-<sha16>-<purpose>` from lowercased
realpath; `sun_path`/pipe length pre-checked (Node 22 truncates silently);
stale-socket removal only after `readOwner` probe fails; no timeout stealing.
`holdProfile` prefers store `Lease` (30 s lapse, 10 s heartbeat, fencing is
store's job) else machine-local lock.

## 8. Imports as generational history ingest

`src/import/sources.ts`: scan (no model calls) → select (busiest-first,
date filter, Tab/Space multi-select) → mode (append vs rebuild-by-start-date)
→ preview (counts, bytes, rough tokens, ~3× compactor-input warning) →
staged generation switched only when whole tree ready; previous generation
kept; sources never modified. Keeps user messages + final replies with dates
and source labels; drops tool calls/results, reasoning, replayed context
(Codex AGENTS.md/env, OMP reminders), slash-command output (keeps typed
`/name args`/`!cmd`/`$code`). ChatGPT branches labeled selected/alternate;
Pi rewinds marked `[alternate branch]`; OptChat-run Pi sessions skipped
(already in memory); forks add only new messages. Receipts
(`import:<sha256>`) dedup across moved files/re-exports; `copyKey` matches
Claude resumes by uuid+text. Claude auto-memory: one dated `note` per topic
file (not `MEMORY.md` index); edited file re-imports as newer note. While
import pending, profile chat blocked; needs main + subagents idle.

# Architecture and implementation

## Component map (source-verified)

- `src/memory.ts` — log/tree/views/scheduler/waiters/zoom/search/date.
- `src/store.ts` — Store/Lease/FileStore, atomic + append primitives.
- `src/prompts.ts` — all model prompts with adaptation comments; recipe §4
  task verbatim + 512-dash ruler + `tooLong` retry.
- `src/compactor.ts` — model lookup, historical-guidance prefix, Anthropic
  cache payload, primer gate, 5-attempt shrink loop returning shortest.
- `src/cache.ts` — `splitView` (4-line blocks), 4-mark Anthropic scheme
  (last-whole + end + 20/40 back; 60-block/240-line coverage), Pi-mark
  stripping, status-line mark placement.
- `src/tools.ts` — zoom/date/search tool definitions, run-transcript pager,
  image rehydration on `n=1`.
- `src/profiles.ts` — paths, config load/save, socket/pipe lock, lease
  holder with heartbeat.
- `src/import/sources.ts` — all source adapters + filters + receipts.
- `docs/victor-recipe.md` — recipe SHA pin (`8f69…6e43`, fetched 2026-10-04)
  + per-file implementation mapping.

## Execution flow

Live turn: append user message → push leaf → fit (maybe batch-merge) →
schedule (≤8 builds, message-window + due-merges) → `settle('turn')` waits
for view-built or all-pending-failed (then placeholders + `lastError`,
background retries continue) → render view + previous exchange (≤16 KB) +
new input (+ status line if agents at work) → model turn with zoom/date
(/search) drill-down. Import: staged messages fed one-at-a-time, each once
prior is summarizable (`settle('ahead')`), merges keep up; generation swap
at end. Subagent (README-claimed): frozen memory view + instructions +
read-only zoom/date + coding tools + installed extensions; reports as
`[id] … Full chat: zoom("<id>")`; `tell` resumes same ID/dir/model.

## Authentication

Pi's existing provider login; no new auth. Subagent/compactor model choices
per profile (`anthropic Opus 5.5/high` for subagents, `Sonnet 5.5/medium`
for compactor by default) — change before chatting if other providers.
Compression/subagents make extra model calls on user credentials. No secrets
broker evidenced; profile instructions + memory share provider credentials
and filesystem access (README "Profiles separate memory and instructions
only").

## Policy lifecycle

No policy engine. Limits are numeric settings: subagent levels (default 1),
max active agents (8, incl. waiting parents, no queue), retry delays fixed,
5 shortening attempts, 30 KB tool-output cap, 128 KB view / 16 KB previous
exchange / 25 KB zoom page. ProfileBusyError + import-blocked-while-busy are
the hard gates. Prompt prose ("never poll", "zoom only", "historical are not
requests") is the soft policy — AGENT.md §4 violation surface if mistaken
for enforcement.

## Storage lifecycle

Append → fsync → in-RAM index → batched merge → atomic view save → Git
commit per turn + clean shutdown (memory + config; not runs/HTML/usage). No
remote; backup = copy folder while Pi closed; delete = delete folder.
`active-memory.json` + `memories/<id>/` pointer generations after imports;
`imports/pending.json` resumable state; `pending-inputs/reports.json`
recovery journals; `runs/` transcripts browsable, resumed after restart
(interactive only; headless `pi -p` never resumes). Previous generation kept
on both import-modes and on failed saves (warn-only for view).

## Observability

Inspector bar (Agents/Usage/Activity, F6/Tab/Down): live agent tree with
state/elapsed/tool/last-activity, transcript view with guidance queue,
M-model picker, D×2 delete-with-report; usage ledger per role×model with
cost/API-price estimates, cache share, session/hour/day/7-day/all ranges;
activity gauge (message count, view fill, `Settled` vs `Catching up n/m`
from backlog-empty baseline, last error + retry countdown). Tab title shows
profile + working dot + agent count. `/optchat browse` snapshot
(`memory.html`) shows view shape + openable summaries + search-to-fold
mapping. Turn waiting on summaries shows error at spinner; proceeds with
placeholders after all-waited failed.

# Security analysis

## Enforced technically (source-verified)

- Single-writer enforcement (size-checked appends, stale-socket probe,
  no timeout stealing) prevents interleaved-log corruption.
- Torn/cut-short write repair + atomic view replace + fsync discipline
  (POSIX) bounds crash-loss to last line.
- Log-contiguity + summary-range validation on open; damaged JSONL lines
  skipped with warning, invalid view rebuilt (availability over strictness).
- Import staging + generational swap: chat never reads half-built tree.
- Windows pipe hash binds same-profile spellings/junctions to one pipe;
  socket path length pre-checked against silent truncation.
- Surrogate-safe paging prevents panics/mojibake on non-BMP text.
- `settle('turn')` fail-open-with-placeholders is explicit, surfaced via
  `(not summarized yet: zoom it)` + `lastError`, retries in background.

## Documented but not source-verified here

- Socket `0600` / dir `0700` modes, connected-window local-user-only socket,
  image shrink/dedup, usage/cost math, handoff chunking (128 K-token /
  16 K-token / 5-min), ZIP via system `unzip`/`tar.exe` with CRC checks.

## Assumptions and limitations

- Agent-written summaries are trusted memory (no provenance/confidence/
  verification gate; IMPORT_GUIDANCE is prompt prose).
- No secrets hygiene evidenced (no redaction before log/compactor/view;
  images stored alongside text; INV-008 absent).
- No tamper-evident audit (Git commits per turn, no hash chain, no remote;
  weaker than Runlith journal).
- Machine-local single writer; cross-machine `Lease` is interface-only
  (FileStore has none); second machine re-offers same sources for import.
- Prompt-as-policy (zoom-only, historical-not-requests, don't-poll) with no
  technical confinement of filesystem/MCP in subagents (read-only memory
  tools, but coding tools + user extensions fully loaded).
- Provider-coupled defaults (Anthropic models/thinking, Anthropic cache
  marks; OpenAI path is fallback with implicit caching, `prompt_cache_
  breakpoint` rejected 400 per README).
- Pi extension: no daemon, no isolation backend, agents die with Pi close.

## Attack surfaces

- Prompt injection via imported history (mitigated by prose only).
- Stale/false summaries (wrong line hides truth until zoomed; search skips
  summaries by design, but view still leads).
- Profile `AGENTS.md` wins over global/repo instructions (documented;
  profile compromise steers main + subagents + compactions except compactor
  excludes instructions per A/B #112 — partial mitigation).
- Local socket/pipe takeover if OS user compromised (POSIX perms help;
  Windows pipes machine-wide, no perms).
- Model-supply + cost: compactor/subagent/import burn caller credentials;
  big imports ~3× estimate; no budget caps evidenced beyond fixed worker
  counts.
- ZIP/import parsers over untrusted exports (system-tool CRC + size caps
  help; content still becomes memory).

# Comparison with our project

## PO-01 — Memory vs Context separation

pi-optchat intentionally fuses them: the summary tree *is* the context
(view + zoom). Runlith separates (AGENT.md §4: Context is derived/disposable,
never authoritative; provider compaction is optimization, never
persistence). Overlap is the continuity problem, not the layering answer.
pi-optchat validates the need; Runlith's split remains the constraint.

## PO-02 — Provenance and trust

pi-optchat: origin labels (`Historical <src> · date · id · title`), receipts
for dedup, alternate-branch marks — good ingest hygiene, but no confidence,
verification timestamps, supersede links, staleness, or validation gate.
Agent text becomes memory. Runlith ADR-0012 requires all of those + agent
notes untrusted until validated (INV-029). pi-optchat is a negative example
that sharpens R-03.

## PO-03 — Retrieval model

pi-optchat: bounded full-view scan (128 KB) + deterministic zoom-down +
optional plain-text search over originals (never summaries). No embeddings,
no FTS ranking, no budget-aware selection beyond sawtooth. Runlith G9 plans
structural/JIT + replaceable retrievers with budgets/filters/explainability.
pi-optchat's "view is truth, zoom only way through" is a usable continuity
contract but an unbudgeted, unfiltered one.

## PO-04 — Identity and lifecycle

pi-optchat: positional IDs (`id+n`, `start=i·2^l`) + content receipts; no
rename problem (IDs are positions), no entity resolution. Lifecycle is
append → summarize → merge → (import generations, Git commits). Runlith G10
needs stable entity IDs + CAPTURE→…→ARCHIVE with supersede. Positional
addressing is elegant for chat continuity, inapplicable to entity memory.

## PO-05 — Branch/worktree story

pi-optchat: none for live memory (single-writer profile lock; second Pi
becomes subagent, not peer writer). Imports handle forks by new-messages-
only + generational rebuild-by-date. MegaMemory's file-merge trichotomy has
no counterpart here — and pi-optchat's refusal to multi-write is itself a
lesson: serialize or subordinate writers; don't merge logs by hand.

## PO-06 — Execution/policy/verification non-overlap

pi-optchat has no Task/Execution/Attempt/Session split, no Policy Engine,
no Evaluation Port, no claim verification. Subagent reports end with
`Full chat: zoom("<id>")` so parent can check work against transcript —
a transcript-audit instinct, not independent verification. Runlith's G6/G7/G8
differentiators are entirely absent, which bounds adoption to continuity.

## PO-07 — Durability UX

pi-optchat: per-turn Git commits, recovery journals (unsent inputs,
pending reports, interrupted subagents resumed, killed-window handoffs
recovered), resumable imports, browsable transcripts. No hash chain, no
remote, no cross-machine story. Runlith journal + audit are stronger; the
recovery-journal *set* (inputs/reports/handoffs/imports as separate files)
is still a useful checklist.

# Bilateral comparison (as in mercury-agent)

## What is better in Runlith

- Durable Task state machine + Execution/Attempt/Session split vs one
  endless chat log. Task lifecycle survives provider/context churn;
  pi-optchat continuity dies with profile Cheers.
- Policy Engine (DENY > REQUIRE_APPROVAL > ALLOW, exact-bound approvals,
  fail-closed) vs numeric caps + prompt prose. Runlith gates are technical;
  pi-optchat's are advisory.
- Independent Evaluation Port vs self-graded compactor (same-provider
  summaries, A/B by same loop). Runlith verification is separable; here
  memory quality is self-attested.
- Secrets Broker (metadata/backend/delivery split, OS keychain) vs shared
  credentials + unredacted memory/images. Runlith closes INV-008;
  pi-optchat leaves it open.
- Hash-chained audit + Event Journal vs per-turn Git commits. Runlith
  tamper-evidence is structural; here history is rewritable.
- Provider neutrality (replaceable executors behind ports) vs Pi-extension
  + Anthropic-tuned prompts/cache/models. Runlith ports survive provider
  churn; pi-optchat inherits Pi's.
- Structured memory provenance design (ADR-0012 four-type + lifecycle +
  confidence/staleness) vs positional summaries + prompt trust. Runlith
  memory is refreshable/auditable by design; here by inspection.

## What is worse in Runlith (gaps pi-optchat exposes)

- No shipped long-horizon continuity: sessions end, compaction discards,
  no bounded-view-plus-drill-down working set. pi-optchat chats for months
  in 128 KB + zoom.
- No import path for Claude/Codex/ChatGPT/Pi history with dedup/receipts/
  branch labels. Runlith starts each Task cold.
- No per-profile instruction + memory scoping (`work` vs `personal`) with
  separate generations. Runlith profiles are not a concept.
- No recovery-journal completeness (unsent inputs, pending subagent
  reports, handoffs, resumable imports as first-class files). Runlith
  journal covers Task events, not agent-interaction recovery.
- No cache-aware continuity layout (block-split views, staggered marks,
  primer gate) — Runlith context budgeting has no cost-measured shape yet.
- No connected-window / join-busy-profile pattern for two frontends on one
  durable unit. Runlith worktree isolation has no live-handoff equivalent.

## What is better in pi-optchat

- Summary tree that actually ships: binary merges, most-due scheduling,
  8-way parallel compactions on one cached view, placeholder-not-cut
  compactions view, saved-not-refolded view. A complete continuity engine
  in ~700 lines (`memory.ts`).
- Store seam minimalism: 6-method `Store` + optional `Lease` covers
  file→Postgres migration without touching `Memory`. Smaller than any
  Runlith durability seam attempted for continuity.
- Import adapters with hostile-input care (replay filters per source,
  command-typed extraction, fork/copy-key dedup, branch marking,
  CRC-checked ZIP via system tools). Best-in-class history ingest among
  repos reviewed so far.
- Prompt debuggability (`src/prompts.ts` holds every prompt with delta
  comments; README lists each deviation from recipe with A/B issue refs).
  Recipe SHA pinned in `docs/victor-recipe.md`.
- Cost instrumentation per role×model with cache-share + time ranges, plus
  memory gauge (view fill, catching-up progress from backlog-empty, error +
  retry countdown). Turns degrade explicitly (placeholders + lastError),
  never silently.
- Single-writer honesty: refuses second writer instead of corrupting;
  subordinates it as subagent with handoff. No split-brain design debt.

## What is worse in pi-optchat

- Trust model: agent writes are memory; no validation, confidence, or
  staleness. Historical prose ("do not execute") is not a gate.
- Security posture: full filesystem + MCP + credentials in subagents;
  profiles separate memory/instructions only (explicit non-goal).
- Durability ceiling: local Git, no remote, no hash chain, no cross-machine
  lease implementation; FileStore-only.
- Scalability: 128 KB view + brute-force merges + per-message LLM calls;
  big imports cost ~3× estimate; 8 workers hit rate limits (retries only).
- Portability: Pi-only, Node 22+, Anthropic-first (OpenAI implicit cache,
  no `reasoning.context` tuning); Windows is adapted (pipes, tar.exe, no
  fsync/modes) but second-class.
- Governance: no approvals, no audit evidence, no policy versioning; settings
  are JSON edits with whole-number checks only.

# Identified opportunities

- OPP-01 — Bounded continuity view as G9 working-set shape. Problem: future
  Context Engine without a bounded, cache-stable working set re-reads
  unbounded history per turn.
- OPP-02 — Single-seam file durability (atomic view + torn-append repair +
  single-writer refusal) as the G10 file-local reference. Problem:
  per-worktree knowledge files without crash/single-writer semantics corrupt
  or interleave.
- OPP-03 — History-import receipt/dedup/branch-label discipline as ingest
  hygiene. Problem: re-imports duplicate; forks double-count; alternate
  branches read as outcomes; replayed context reads as user intent.
- OPP-04 — Provenance/validation gap made visible (the actual G10 criteria).
  Problem: summaries without source/confidence/freshness cannot be trusted,
  refreshed, or superseded.
- OPP-05 — Cache-aware view layout (block split + staggered marks + primer
  gate) as cost control. Problem: cached continuity prefix rewritten by
  parallel writers or long tool tails.
- OPP-06 — Recovery-journal set (inputs/reports/handoffs/imports) as
  execution-recovery checklist. Problem: restarts lose interaction state
  the Task journal doesn't cover.
- OPP-07 — Subordinate-don't-merge writer policy as worktree lesson.
  Problem: two writers on one memory unit need serialization, not file merge.

# Recommendations

## R-01 — Adapt sawtooth bounded view + saved-view discipline as G9/G10 design reference (P2)

- Proposed change: when G9 continuity lands, adopt pi-optchat's shape:
  log-authoritative, bounded view (budget/high→low batch merge, most-due
  oldest-first), view saved on merge (not refolded), drill-down by
  position, placeholders on pending with explicit error + background retry.
- Problem: OPP-01, OPP-05 (partially).
- Approach: design note + pseudocode reference to `memory.ts`
  (Sawtooth/mostDue/compactionView/settle); reimplement inside Runlith
  Context/Memory contracts with budgets, filters (INV-008), and
  provenance ranks. No Pi/Anthropic coupling.
- Benefits: cache-stable working set; explicit degraded mode; no
  unbounded prompt growth.
- Security: must add secret filtering before view render + before compactor
  input (absent upstream); zoom must respect same filters.
- Complexity: Medium (design Low).
- Risks: 128 KB/512 B numbers are LLM-tuning, not Runlith budgets; must be
  re-derived from context budgets, not copied.
- Acceptance: continuity design cites tiling + most-due + saved-view +
  placeholder-degradation rules; test proves refold-avoidance + pending
  placeholders + filtered secrets.
- Priority: P2 (with G9 scope).
- Classification: `NEEDS_ADR` → `ADAPT` as design, `REJECT` as numbers/code.

## R-02 — Adapt Store-seam durability shape for file-local memory (P2, design-only)

- Proposed change: bank `Store { load/append/appendNode/saveView/putImage/
  image, lock? }` + FileStore correctness (atomic view, torn-append repair,
  size-checked single-writer refusal, POSIX fsync / Windows degrade) as the
  reference if G10 ever picks file-local backends.
- Problem: OPP-02.
- Approach: design note only; no code until G10 storage ADR. If selected,
  reimplement in Rust inside Runlith journal/audit posture (hash chain where
  audit-relevant); do not port TS.
- Benefits: crash-safe, corruption-refusing file memory without a DB.
- Complexity: Low as note; Medium if built.
- Risks: file-JSONL is wrong primitive if G10 lands event-sourced (then
  merge/append design moot — prefer event merge).
- Acceptance: note cites 6-method seam + 4 correctness rules + lock/lease
  split.
- Priority: P2 design bank.
- Classification: `NEEDS_ADR` → `INSPIRED_ONLY` until storage ADR.

## R-03 — Require provenance MegaMemory/pi-optchat both omit (P1 for G10 scope)

- Proposed change: treat both repos' missing fields as G10 acceptance:
  every memory entry carries source refs, confidence, created/verified
  timestamps, supersedes links, staleness, content hash; lifecycle
  `CAPTURE → NORMALIZE → DEDUPLICATE → LINK → VALIDATE → USE →
  REFRESH/SUPERSEDE → ARCHIVE`; agent writes untrusted until promoted.
- Problem: OPP-04; INV-029.
- Approach: schema + validation policy before any retriever/summarizer
  work. Import receipts/branch labels (OPP-03) are ingest attributes, not
  trust substitutes.
- Benefits: refreshable, auditable memory instead of believed text.
- Complexity: Medium (owned by G10).
- Acceptance: sourceless write fails validation; stale surfaces refresh;
  supersede preserves link.
- Priority: P1 within G10.
- Classification: `ALREADY_DESIGNED` (ADR-0012) → `EXTERNAL_VALIDATION`.

## R-04 — Adapt import hygiene (receipts/replay-filters/branch-labels) as ingest checklist (P3)

- Proposed change: when any history-import lands, require pi-optchat's
  checklist: stable per-message receipts surviving moves/re-exports,
  resumed-session copy-key dedup, per-source replay filters (slash-output,
  injected context, reminders), fork new-messages-only, alternate-branch
  marking, generational swap, paused-resume, damaged-record pre-list.
- Problem: OPP-03.
- Approach: checklist in G10 import task; reimplement per Runlith source
  format; content never executes (imported instructions are data).
- Benefits: idempotent, branch-honest imports.
- Complexity: Low-Medium.
- Risks: over-fitting to Claude/Codex/Pi/ChatGPT shapes; keep adapter-split.
- Acceptance: re-import idempotence test + fork test + alternate-branch
  label test + replay-filter test.
- Priority: P3.
- Classification: `NOVEL_GAP` (no importer) → `ADAPT` as checklist.

## R-05 — Note cache-aware layout as cost lesson, not architecture (P3)

- Proposed change: record 4-line block split + staggered Anthropic marks
  (last-whole + end + 20/40 back = 240-line coverage) + primer gate
  (one call warms prefix, rest wait for answer-start) as the cost pattern
  if G9 continuity views use prompt caching.
- Problem: OPP-05.
- Approach: measurement note; no coupling to Anthropic APIs in domain.
  OpenAI fallback (implicit cache, no breakpoint) is the portability
  caution.
- Benefits: avoids whole-prefix rewrites from tool-tail growth or
  parallel writers.
- Complexity: Low.
- Risks: provider-specific; numbers rot with model/cache policy changes.
- Acceptance: note cites block/marks/primer + fallback; spike measures
  hit-rate vs rewrite bytes if pursued.
- Priority: P3.
- Classification: `ALREADY_DESIGNED` (budgeted context) →
  `INSPIRED_ONLY`.

## R-06 — Bank recovery-journal + subordinate-writer patterns (P3)

- Proposed change: keep `pending-inputs/reports.json` + handoff-on-close +
  import-pending + transcript-resume + busy-join-as-subagent as the
  execution-recovery checklist for daemon/agent restarts; keep
  single-writer-refusal as the worktree-memory writer policy.
- Problem: OPP-06, OPP-07.
- Approach: checklist for G8/G11 restart work; handoff prompt shape
  (`HANDOFF`: goals/decisions/changes/evidence/failures/outstanding,
  attempts≠successes) as prose reference only.
- Benefits: restarts deliver instead of dropping; no split-brain memory.
- Complexity: Low.
- Acceptance: restart test shows queued inputs + pending reports delivered,
  killed-window handoff recovered, busy-profile join (not steal).
- Priority: P3.
- Classification: `PARTIALLY_EXISTS` (journal exists; interaction recovery
  doesn't) → `INSPIRED_ONLY`.

# Rejected or inapplicable ideas

- **Adopt summary-tree numbers/code as G9 engine.** Reason: 512 B/128 KB/
  8-worker/Anthropic-mark tuning is Pi+LLM-specific; Runlith budgets,
  filters, and Rust contracts govern. Reference only (R-01). (REJECT as
  code/numbers.)
- **Adopt FileStore/JSONL as Runlith memory storage.** Reason: conflicts
  with SQLite + Event Journal durability + hash-chained audit posture until
  G10 storage ADR decides; FileStore-only + no lease impl today. (REJECT
  for now; R-02 banks the shape.)
- **Treat summaries/zoom-view as trusted truth.** Reason: violates INV-029;
  view-is-truth is prompt prose, and summaries can be wrong (upstream admits
  search must skip them). (REJECT.)
- **Adopt prompt-as-policy (zoom-only, historical-guidance, don't-poll).**
  Reason: violates AGENT.md §4 (autonomous execution never relies on prompt
  alone as security boundary). Technical gates required. (REJECT as
  boundary; keep as UX wording.)
- **Import pi-optchat as dependency or copy `memory.ts/store.ts/prompts.ts`.**
  Reason: prohibited by AGENT.md §16.1; Pi-extension coupling + trust-model
  mismatch stand on their own. Reimplement adapted concepts in Rust.
  (REJECT.)
- **Subagents with full FS/MCP/credentials as Runlith execution model.**
  Reason: contradicts Policy/Secrets/Runner isolation + least-privilege;
  at most a UX reference for steering (`tell_parent` progress notes,
  `Full chat: zoom(id)` audit trailer). (REJECT as model.)
- **Connected windows over local socket as multi-frontend sync.**
  Reason: no daemon/auth story beyond OS user; Runlith IPC needs
  negotiated capabilities + admission policy. Handoff-on-close prose only.
  (REJECT for v1.)
- **Usage-ledger cost math as billing.** Reason: API-price estimates, not
  subscription truth; unknown rates zero. Observability shape only.
  (REJECT as accounting.)

# Proposed implementation plan

Independent, ordered by leverage/cost (no implementation authorized by this
research file):

1. R-03 provenance acceptance (fold into G10 scope; no standalone code).
2. R-01 sawtooth/saved-view design note (with G9 continuity).
3. R-02 Store-seam durability note (bank; build only if file-local chosen).
4. R-04 import checklist (whenever history-import scoped).
5. R-05 cache-layout note (if prompt-cached views pursued).
6. R-06 recovery-journal checklist (with restart/worktree work).

Each step needs provenance/validation tests, secrets-redaction keepers,
retrieval-explainability assertions, and docs updates. No step changes
public versioning or cross-context storage reads.

# Evidence and references

Directly verified (raw fetch at pinned SHA
`0aa45406325f610f3106510218d8cc69deb0cccc`):

- https://github.com/jonaslsaa/pi-optchat (landing: description, 236 stars /
  21 forks snapshot, MIT, topics memory/optchat/pi-extension).
- GitHub Commits API `main` → `0aa45406325f610f3106510218d8cc69deb0cccc`,
  2026-10-10T18:37:33Z, `Release 0.8.3 (#148)`.
- GitHub Trees API at pinned SHA (src/test/docs inventory, 30 src files).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/package.json
  (npm `0.8.3`, Pi extension `./src/index.ts`, Pi `1.0.2` compat, Node
  `>=22.19.0`).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/memory.ts
  (`NODE=512`, `VIEW=128_000`, `CAP=30_000`, `PAGE=25_000`, Sawtooth,
  mostDue, Memory, zoom/search/date, settle turn/tree/ahead).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/store.ts
  (Store/Lease/FileStore, atomicWrite, appendJson torn-repair, single-writer
  refusal, view.json/images).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/prompts.ts
  (COMPACT/MASTER/VIEW_DOC/SUBAGENT/CONTINUITY/SEARCH_DOC/
  IMPORT_GUIDANCE, 512-dash ruler task, tooLong retry, HANDOFF).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/tools.ts
  (zoom/date/search definitions, SEARCH_PAGE=20, run transcript pager,
  image rehydration).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/compactor.ts
  (createCompressor, primeFirst gate, 5-try shrink loop, historical prefix).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/cache.ts
  (splitView 4-line blocks, MARKS [0,20,40], status-line mark, Pi-mark
  stripping).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/profiles.ts
  (name validation, dataHome/OPTCHAT_HOME, socket-vs-pipe lock, stale probe,
  holdProfile lease-or-lock, HEARTBEAT_MS).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/src/import/sources.ts
  (scanLocal/scanChatGPT/scanClaudeMemories, readConversation/readMemory,
  copyKey/copy-dedup, replay filters, branch labels, receipts).
- https://raw.githubusercontent.com/jonaslsaa/pi-optchat/0aa45406325f610f3106510218d8cc69deb0cccc/docs/victor-recipe.md
  (recipe URL, content SHA-256 `8f6997e8…6e430`, per-file mapping).

Inferred (not source-verified): subagent spawn/report/resume internals,
connected-window bridge/handoff chunking, inspector key handling, usage-cost
math, Windows pipe ACL posture. Marked as such in §Security.

Runlith references: `AGENT.md` §§4/7/16.1/16.2/20, `ARCHITECTURE.md`,
`docs/architecture/INVARIANTS.md` (INV-008/022/029/115),
`docs/architecture/context-engine.md`, `docs/architecture/continuity.md`,
`docs/architecture/adr/ADR-0010-context-derived-state.md`,
`docs/architecture/adr/ADR-0011-provider-independent-context-continuity.md`,
`docs/architecture/adr/ADR-0012-structured-memory-provenance.md`,
`planning/roadmap.yaml` (G9/G10 deferred), `planning/tasks/queue.yaml`
(Context contracts queued), `crates/AGENTS.md` (no context/memory crate yet).

# Final assessment

pi-optchat is the most complete continuity engine reviewed so far: a
shipped binary summary tree with bounded cache-stable view, crash-honest
file store, hostile-input-aware history import, and subordinate-writer
discipline — all in a small TypeScript extension. Its value to Runlith is
design and checklist wisdom (sawtooth/saved-view shape, Store-seam
correctness, import receipts/filters/branch labels, cache layout, recovery
journals), not storage, retrieval, or trust architecture. Its trust model
(agent text is memory), security model (full FS/credentials, prompt-as-
policy), and operational model (machine-local single writer, Pi-coupled,
Anthropic-tuned) are inapplicable to Runlith v1's provenance-first,
policy-gated, provider-neutral monolith.

Recommended next steps: keep this file as evidence; fold R-03 provenance
criteria into G10 acceptance; bank R-01/R-02 design notes for G9/G10 scope;
use R-04 import checklist whenever history ingest is scoped; treat R-05/R-06
as cost/recovery notes. Do not import pi-optchat code or add it as a
dependency.

## Novelty / adoption summary

| Finding | Novelty | Adoption |
|---|---|---|
| Sawtooth bounded view + saved view + zoom drill-down + placeholder degrade | `NEEDS_ADR` (no continuity view) | `ADAPT` as design (R-01) |
| Store seam + atomic view + torn-repair + single-writer refusal | `NEEDS_ADR` | `INSPIRED_ONLY` until storage ADR (R-02) |
| Provenance/confidence/staleness requirements | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` (R-03) |
| Import receipts / replay filters / branch labels / generational swap | `NOVEL_GAP` (no importer) | `ADAPT` as checklist (R-04) |
| Cache block-split + staggered marks + primer gate | `ALREADY_DESIGNED` (budgeted context) | `INSPIRED_ONLY` (R-05) |
| Recovery journals + handoff + busy-join-as-subagent | `PARTIALLY_EXISTS` | `INSPIRED_ONLY` (R-06) |
| 512 B / 128 KB / 8-worker numbers as engine | `CONFLICTS_WITH_ARCHITECTURE` (budgets govern) | `REJECT` |
| File-JSONL as memory store / trusted summaries / prompt-as-policy / Pi-coupled subagents | `CONFLICTS_WITH_ARCHITECTURE` or out-of-scope | `REJECT` |
