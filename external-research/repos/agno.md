# Re-audit

agno-agi/agno — Python agent framework + AgentOS runtime
(multi-agent teams, sessions/memory/knowledge stores, HITL
approvals, evals, run persistence/resume over SQLite/Postgres).

- repo: https://github.com/agno-agi/agno
- reviewed_revision: `24bc39f137f7f5fb5b1df89e3065914e01c0e901`
  (main, 2026-10-10T18:08:18Z — `feat: store run events for
  external agents (#10957)`)
- version at pin: 3.1.2 (`libs/agno/pyproject.toml`)
- license: Apache-2.0 (`LICENSE`, copyright 2025-2026 Agno Inc.)
- language/stack: Python >=3.9, monorepo: `libs/agno/agno`
  (SDK: agent/team/workflow/run/session/memory/knowledge/
  approval/guardrails/eval/tools), AgentOS server bundle
  (FastAPI, SQLAlchemy, JWT, OpenTelemetry), 100+ toolkits
  and 20+ vector-DB/storage extras
- method: raw-file reads at pin + Atom commit feed (no REST;
  feed sufficed). No clone. Scoped to approval/HITL, teams,
  session/run stores, memory, knowledge, guardrails, evals —
  agent loop internals, db backends/schemas, run event enums,
  memory strategies, eval bodies, job queue, cookbook not
  line-read.

# What it is (source summary)

A framework where an `Agent` or `Team` (both giant
flag-surfaced dataclasses) runs model turns with tools,
persists sessions and runs to a user-owned database
(SQLite/Postgres/Mongo/Redis/…), and serves through the
AgentOS runtime (50+ endpoints, SSE resume, background jobs,
cron scheduling, JWT RBAC). Teams coordinate members via
modes (`coordinate|route|broadcast|tasks`) and delegation
flags. Tools carry HITL flags (`requires_confirmation`,
`requires_user_input`, `external_execution`) plus an
`@approval(type="required"|"audit")` decorator; paused runs
resume via `continue_run`, and step-replay via
`continue_from`/`fork`. Runs optionally store events with
skip-lists and indexed replay (`last_event_index`); retries
carry a retryability classification the queue worker honors
fail-closed. Memory (MemoryManager/UserMemory, session
summaries, LearningMachine), knowledge (Knowledge protocol,
agentic RAG, references), guardrails (exception-raising
pre/post hooks), evals (Accuracy/AgentAsJudge/Performance/
Reliability suites), and per-run/per-session metrics round
out the platform. Telemetry is on by default
(`AGNO_TELEMETRY=false` opts out).

# Architecture (as evidenced)

- Approval/HITL (`approval/types.py`, `approval/decorator.py`,
  full read; `tools/function.py` head): `ApprovalType =
  required|audit`. `required` is blocking: run cannot continue
  until resolved via approvals API; decorator sets
  `requires_confirmation=True` unless another HITL flag is
  present. `audit` is non-blocking: approval record created
  after the HITL interaction resolves, for compliance; it
  *requires* at least one HITL flag or raises. `Function`
  serializes only `name/description/parameters/strict/
  requires_confirmation/external_execution/approval_type`
  (`SERIALIZED_FIELDS`); behavior flags (hooks, caching,
  user-input schema, side-effect traits) are runtime-only and
  rehydrated from the live registry. `continue_run`
  (`team.py`) supports `fork`, `regenerate`,
  `replace_original`, `additional_instructions`; unsupported
  options on some adapters return 400 (per commit bodies).
- Run persistence/resume (commit bodies #10957/#10937/#10925/
  #10917/#10905 + `Team` flags): `store_events` (default
  False) + `events_to_skip` (default skips content deltas);
  terminal event (`RunCompleted|RunError|RunCancelled`) built
  *before* persistence so stored runs end with it. Stored
  events keep queue-stamped `event_index`; `/resume` replays
  from DB honoring a `last_event_index` floor. Retried runs
  keep failed attempts' tools, emit a `retry` warning event,
  and store `metadata{attempts, retryable, error_type}`;
  queue worker fails the ticket at once on non-retryable
  rows and fails closed on row-read failure. Checkpoint
  granularity: `"runs"` (terminal-only) vs `"tool-batch"`
  vs `"tools"` (reserved, raises). `fork_session` deep-copies
  runs into a new session with fresh run IDs; step-replay
  records transcript positions in
  `Message.provider_data["claude_sdk"]`. Stated non-goal:
  "neither a resumed session nor a queue re-drive after a
  crash makes tools exactly-once, so side effects outside
  the workspace must be idempotent."
- Teams (`team/team.py` head, `team/mode.py` full):
  `TeamMode = coordinate|route|broadcast|tasks`; flags
  `respond_directly`, `delegate_to_all_members`,
  `determine_input_for_members`, `max_iterations`,
  `share_member_interactions`, `add_team_history_to_members`,
  `store_member_responses`, `stream_member_events`.
  Cross-cutting managers mirror Agent: db, memory_manager,
  session_summary_manager, learning, compression_manager,
  offload (`offload_tool_results`), skills, reasoning,
  followups, fallback models. Members can be nested teams.
- Sessions/memory (`session/__init__.py`,
  `memory/__init__.py`): `AgentSession|TeamSession|
  WorkflowSession` + `SessionSummaryManager`; `session_state`
  dict with agentic-state tools and overwrite controls;
  `MemoryManager|UserMemory` with optimization strategies
  (`SummarizeStrategy` default); `search_past_sessions` and
  `read_chat_history` as tools; explicit warning that two
  memory writers registering `update_user_memory` silently
  drop one (tool parsing keeps the first name seen).
- Knowledge (`knowledge/__init__.py`): `Knowledge|
  FileSystemKnowledge|KnowledgeProtocol` behind lazy
  PEP-562 imports (leaf imports skip cloud clients);
  `search_knowledge` agentic-RAG flag, `knowledge_filters`,
  `update_knowledge` tool, `references_format: json|yaml`,
  per-reader extras (chonkie/pdf-ocr/unstructured gated).
- Guardrails (`guardrails/__init__.py`,
  `guardrails/base.py`, full): `BaseGuardrail.check/
  async_check(run_input) -> None` — exception-raising,
  input-only, wired as `pre_hooks|post_hooks` alongside
  callables and `BaseEval`. Built-ins: OpenAI moderation,
  PII detection, prompt injection.
- Evals (`eval/__init__.py`): `AccuracyEval`,
  `AgentAsJudgeEval`, `PerformanceEval`, `ReliabilityEval`,
  `Case/Suite/run_cases` CLI — lazy-loaded (circular-import
  avoidance with Agent noted in code).
- Metrics/telemetry (`team.py` flags, commit #10925):
  `RunMetrics|SessionMetrics` accumulated per run with
  per-model entries; adapter-parsed usage (cache-read/write
  conventions differ per provider and are documented);
  per-run wall-clock `duration`/`time_to_first_token`.
  Telemetry event per agent run, on by default.
- Tool caching (`tools/function.py` head): `cache_results/
  cache_dir/cache_ttl` with `CACHE_FORMAT=2` versioning,
  ownership/permission refusal on shared cache dirs, media
  excluded from keys, deepcopy-faithfulness checks before
  serving a hit.
- Commit feed at pin: stored run events for external agents
  (#10957); compaction for Claude/Codex adapters (#10924);
  retry tool-keeping + streaming warnings (#10937); harness
  metrics parity (#10925); step-replay continuation (#10917)
  — i.e. current upstream work is all durability/resume.

# Findings for Runlith

- F-01 — Approval split `required` (blocking, run ends until
  resolved) vs `audit` (non-blocking compliance record).
  Two HITL depths where Runlith has one REQUIRE_APPROVAL;
  the audit-after-resolve record maps to journal-as-trail.
- F-02 — Step-replay vocabulary: `continue_from =
  end|last_user|index` + `fork` with `forked_from_*`
  lineage, `fork_session` (fresh IDs, deep-copied runs),
  transcript positions in provider metadata. Richer resume
  naming than Runlith's.
- F-03 — Stored-event discipline: skip-lists defaulting off
  content deltas, terminal-event-before-persist, stamped
  indices, `last_event_index` replay floor, DB replay after
  live-buffer expiry. Direct precedent for journal replay
  semantics.
- F-04 — Retry classification done honestly: per-error
  retryability taxonomy, `attempts/retryable/error_type`
  metadata, fail-closed worker, and a user-facing
  no-exactly-once disclaimer as an idempotency precondition.
- F-05 — Per-component session types (Agent/Team/Workflow)
  plus `session_state` with agentic writes and overwrite
  controls; memory as manager + strategies + summaries.
  Schema surface Runlith's session model can be checked
  against.
- F-06 — Team modes as a closed enum with documented
  fan-out semantics (broadcast: async-concurrent vs
  sequential; tasks: shared list + iteration cap). A crisp
  delegation vocabulary Runlith's orchestration docs lack.
- F-07 — Evals + guardrails share one hook slot
  (pre/post), exception-contract guards, four named suites
  with Case/Suite runner. Minimal portable shape for G7
  advisory checks.
- F-08 — Metrics honesty (provider-divergent cache-token
  conventions documented per adapter) undercut by
  telemetry-on-by-default. The first half is worth copying;
  the second is a posture Runlith must not copy.

# Comparison with our project

## What is better in Runlith

- Policy is deterministic and scoped: DENY >
  REQUIRE_APPROVAL > ALLOW over exact scopes, single-use,
  child-subset, fail-closed, hash-chained. Agno approval is
  a per-tool boolean with no scope/expiry/audit-of-grant;
  `audit` records the interaction, not the authorization.
- Durable substrate is local: SQLite + event journal, no
  operated services. Agno durability assumes Postgres/
  Redis/queue workers and an AgentOS deployment.
- Secrets boundary (G6: OS keychain, blind delivery) vs
  PII-regex detectors plus moderation APIs that ship call
  content to third parties pre-verdict.
- No silent-drop traps: Runlith has no equivalent of the
  duplicate-`update_user_memory` tool-name shadowing or
  telemetry-on-by-default.
- Isolation is a boundary (runner + worktree admission),
  not a flag on an unsandboxed workspace.

## What is worse in Runlith (gaps this exposes)

- Single approval depth: no blocking-vs-audit-record split
  (P3, R-01).
- Resume vocabulary is thinner: no continue-from-step /
  fork-with-lineage / fresh-IDs-on-fork naming (P2, R-02).
- No stored-event replay discipline: skip-lists, terminal-
  first persistence, index floors (P3, R-03).
- No retryability taxonomy with fail-closed re-drive and a
  stated idempotency precondition (P3, R-04).
- No delegation-mode enum or fan-out semantics in
  orchestration docs (P3, R-06).
- No eval-suite checklist shape for G7 runtime advisory
  use (P3, R-07).

## What is better in the source

- Approval decorator composes in either order with
  `@tool` (sentinel attribute), fails fast on invalid
  combos, and separates serialized config from runtime-
  only behavior with documented ownership.
- Resume engineering is unusually careful: terminal-first
  persistence, failed-attempt tool retention on both
  paths, streaming retry warnings, worker-restart reclaim
  honoring classifications, mutation-tested guards.
- Non-goals are stated where they hurt most (exactly-once,
  side-effect rewind on fork, telemetry scope), including
  in user-facing docs as preconditions.
- `SERIALIZED_FIELDS` vs `RUNTIME_ONLY_FIELDS` is the
  cleanest config-vs-behavior split seen in this survey.

## What is worse in the source

- Trust model is per-tool booleans + model judgement over
  an unsandboxed workspace; guardrails are exception-hooks
  over inputs only, blind to outputs and tool results.
- Moderation/PII/judge paths exfiltrate call content to
  providers pre-verdict by construction.
- Durable correctness depends on caller discipline (same-
  process IDs, explicit skip-lists, adapter-set flags that
  AgentOS flips on silently).
- Local tool-result caching (shared temp dirs, JSON round-
  trips, deepcopy faithfulness) is a whole correctness
  subsystem Runlith's journal makes unnecessary — and its
  presence signals how much can go wrong.
- Framework surface is enormous (hundreds of Agent/Team
  flags); flag interaction bugs are documented in-code
  (memory-tool shadowing) rather than designed out.

# Opportunities and recommendations

- R-01 — Cite required-vs-audit approval split (F-01) as
  `EXTERNAL_VALIDATION` for journal-as-compliance-trail;
  evaluate a log-only audit record alongside
  REQUIRE_APPROVAL. P3.
- R-02 — Adopt continue-from-step / fork-with-lineage /
  fresh-IDs-on-fork vocabulary for journal recovery docs
  (cf. pydantic-ai R-05, langgraph R-01). P2,
  `PARTIALLY_EXISTS` → `ADAPT`.
- R-03 — Adopt stored-event replay discipline: content-
  skip defaults, terminal-record-before-persist, replay
  index floor (F-03). P3, `NOVEL_GAP` → `ADAPT`
  (requirements-level; journal already stores events).
- R-04 — Add retryability taxonomy + fail-closed re-drive
  + user-facing idempotency precondition, mirroring F-04
  (cf. pydantic-ai R-05/R-06 non-goal discipline). P3,
  `NOVEL_GAP` → `ADAPT` (requirements-level).
- R-05 — Cite stated no-exactly-once disclaimers (F-04) as
  `EXTERNAL_VALIDATION` for Runlith's journal honesty
  docs. P2.
- R-06 — Name delegation fan-out semantics (mode enum +
  concurrent-vs-sequential + iteration cap, F-06) in
  orchestration docs. P3, `NOVEL_GAP` → `INSPIRED_ONLY`
  (vocabulary, not the supervisor pattern).
- R-07 — Use the four-suite eval shape
  (accuracy/judge/performance/reliability + Case/Suite,
  F-07) as the G7 EvaluationPort checklist. P3,
  `NOVEL_GAP` → `ADAPT` (checklist only; judges stay
  advisory, never enforcement).
- R-08 — Require per-tool side-effect declarations tested
  against behavior (cf. `has_side_effects` +
  readOnlyHint review discipline, F-05 context). P3,
  `NOVEL_GAP` → `ADAPT` (manifest-level).
- REJECT: SDK/framework dependency; AgentOS + Postgres/
  Redis/queue/cloud backends; DB-backed stores, managers,
  and 100+ toolkits as code; Python import of any kind;
  model-judged evals or moderation as enforcement;
  telemetry-on-by-default posture; local tool-result
  caching subsystem; JWT/RBAC/OpenFGA server coupling;
  exception-hook guardrails as a security boundary.

# Evidence and references

- Pin: `main@24bc39f1…` via Atom feed `commits/main.atom`
  (2026-10-10T18:08:18Z; feed sufficed, no REST needed).
- Full pinned raw URLs (verified at pin):
  - README (framework/runtime overview, features, license
    pointer, telemetry notice):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/README.md
  - license (Apache-2.0, Agno Inc.):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/LICENSE
  - version + extras (3.1.2, storage/vector/knowledge):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/pyproject.toml
  - approval types (required/audit, full read):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/approval/types.py
  - approval decorator (composition, fail-fast, full read):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/approval/decorator.py
  - Function model (HITL flags, serialize/runtime split,
    cache discipline; head ~1240/2981 lines):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/tools/function.py
  - Team (modes, delegation, checkpoint, continue_run,
    managers; head ~1140/2070 lines):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/team/team.py
  - team modes (closed enum, full read):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/team/mode.py
  - session index (Agent/Team/Workflow sessions):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/session/__init__.py
  - run index (RunContext/RunStatus/cancellation):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/run/__init__.py
  - eval index (four suites + Case/Suite runner):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/eval/__init__.py
  - knowledge index (lazy protocol surface):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/knowledge/__init__.py
  - guardrail index + base (hook contract, full read):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/guardrails/__init__.py
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/guardrails/base.py
  - memory index (manager/strategies):
    https://raw.githubusercontent.com/agno-agi/agno/24bc39f137f7f5fb5b1df89e3065914e01c0e901/libs/agno/agno/memory/__init__.py
  - tree pages (structure only, 2 used): `libs/agno/agno`
    and `libs/agno/agno/approval` at pin.
- Commit evidence (feed bodies): #10957 stored run events;
  #10924 compaction; #10937 retry tool-keeping + warnings;
  #10925 harness metrics; #10917 step-replay; #10905
  background lifecycle/cancellation.
- Not inspected: agent loop internals, db backends/schemas,
  run/agent.py + run/team.py event enums, knowledge
  readers/ingestion, memory manager/strategies bodies, eval
  bodies, os/job_queue, cookbook, tests, docs site.
- Runlith references: AGENT.md §§4/7; G6 secrets/policy;
  G7 planning/EvaluationPort; pydantic-ai R-01–R-07;
  langgraph R-01/R-06.

# Final assessment

Best resume-engineering source in the survey so far: the
terminal-first persistence, failed-attempt tool retention,
indexed replay, and fail-closed retry taxonomy (R-02–R-05)
are requirements-level ADAPT/EXTERNAL_VALIDATION items
with the source's own non-goal statements doing half the
ADR work. Approval contributes one portable idea (audit
record, R-01); teams contribute vocabulary only (R-06);
evals contribute a checklist (R-07). Nothing structural
transfers: framework, AgentOS, operated backends,
model-judged enforcement, telemetry default, and the
tool-cache subsystem stay REJECT. No IKQ entry:
requirements-level deltas only.
