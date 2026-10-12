# Re-audit

pydantic/pydantic-ai — typed Python agent SDK + capability harness
for long-running work (coding agents, durable workflows, voice).

- repo: https://github.com/pydantic/pydantic-ai
- reviewed_revision: `69ea1e57201ed59e7d30fbe4a7590d193acecce0`
  (main, 2026-10-10T13:18:50Z — `Bump genai-prices to 0.1.10…`)
- version at pin: dynamic from git tags (`uv-dynamic-versioning`);
  no static version field in `pydantic_ai_slim/pyproject.toml`
- license: MIT (core + harness)
- language/stack: Python 3.11+, monorepo: `pydantic_ai_slim`
  (core SDK), `src/pydantic_ai_harness` (30+ capabilities),
  `pydantic_graph`, `pydantic_evals`, `clai`/`pydantic_clai2` (CLI/TUI)
- method: raw-file reads at pin + Atom commit feed (REST API
  rate-limited). No clone. Scoped to approval/deferred tools,
  guardrails, spend, persistence, judge, shell — models, realtime,
  evals bodies, graph, CLI internals not line-read.

# What it is (source summary)

A typed agent loop (`Agent` + `RunContext` deps + `output_type`
validation) where everything around the model is one primitive: the
*capability* (tools + instructions + hooks + settings). Core ships
loop-deep capabilities (MCP, thinking, compaction, tool search);
the Harness ships the rest as composable blocks, and complete agents
(`Coder`, `Researcher`) are just combined capabilities that come
apart the way they went together. Durable execution attaches the
same agent to Temporal/DBOS/Prefect/Restate/Lambda/Kitaru/Airflow/
Absurd. Observability is plain OpenTelemetry (Logfire easiest);
pricing via `genai-prices`; model-written code runs in `Monty`
(sandboxed interpreter, separate repo).

# Architecture (as evidenced)

- Deferred tools / HITL (`_deferred.py`, full read):
  `DeferredToolRequests{calls, approvals, metadata}` as agent
  `output_type`; run ends, resumes later with
  `DeferredToolResults{approvals, calls}` keyed by `tool_call_id`.
  Approval results: `ToolApproved(override_args?) | ToolDenied(message)
  | bool`. `build_results` rejects unknown IDs, supports
  `approve_all`; `remaining()` computes unresolved set;
  `filter_deferred_results` keeps categories matched. Two approval
  shapes: deferred (run ends, serializable, cross-process/durable)
  vs in-process async guard (run stays open; CLI/TUI/desktop).
  Approved-then-changed-policy still re-evaluated on resume —
  `approve` becomes no-op for cleared calls, other verdicts apply.
- Guardrails (`guardrails/README.md`, full read): `InputGuardrail |
  OutputGuardrail | ToolGuardrail(guard + result_guard)`, verdicts
  `allow | block | replace | retry | approve`.
  Input-block is graceful refusal (`SkipModelRequest`, no tokens
  spent); output-block raises (`OutputBlocked`, caller decides).
  `replace` = redaction, threads through guard chains (order
  meaningful; redactor first). Tool-guard maps onto loop control
  flow (`SkipToolExecution`, `ModelRetry`, `ApprovalRequired`),
  sees validated args pre-run and raw results pre-truncation
  (`position='innermost'`). `tools=[...]` scoping + `hidden=[...]`
  (withheld from model definitions; typos warned). Guards never see
  output tools, external/deferred tools, or provider built-ins —
  documented blind spots. Streaming caveat: output guard inspects
  final output only, partial chunks already sent. Tracing:
  redaction/refusal spans carry content only under
  `trace_include_content`. Ready-made detectors: `redact_secrets`,
  `redact_personal_data` (with honest per-pattern precision notes —
  Luhn-checked cards, mod-97 IBANs, AWS-secret-key deliberately
  absent), `blocked_keywords`; explicit non-goals (no injection
  detection by regex, no `NoRefusals`).
- ToolCallJudge (`tool_call_judge/README.md`, full read): second
  model answers one risk question per call (`yes|no|unsure`),
  post-validation pre-execution, `on_uncertain: block|allow|ask`
  (default block — a down judge must not admit calls; `ask`
  escalates to `DeferredToolRequests`). Human approval +
  judge compose as two gates in fixed order (defer first, judge
  second; both must allow; judge can block approved calls, never
  release denied ones). Default judge input is call-only (no
  conversation — keeps prompt-injection surface small);
  `include_conversation` opt-in with escaping + warning. Verdicts
  durable-checkpointed on replay (billed once, stable). Stated
  safety boundary: "a filter, not a security boundary."
- SpendLimits (`spend/README.md`, full read): cross-window
  USD/token budgets (`run|conversation|day|month|total`, per-tenant
  `scope`, `warn_at`, shared-counter rules), priced per response
  via `ModelResponse.cost()` (per-request pricing, never summed
  usage). Honest guarantees section: crosser completes, concurrent
  runs can over-admit, abandoned streams invisible — "a brake on a
  runaway loop, not an accounting ledger." `RedisSpendStore`: one
  Lua script per response across windows, integer-billionth USD
  arithmetic (float residue doesn't average out on repeated
  shapes), cluster hash-tag keys, replay-stable dedup tokens,
  fail-closed store errors (read error refuses, write error
  propagates). Refusal mid-run resumes from `capture_run_messages`
  (no side-effect replay). `exhausted()` for admission vs
  `status()` for display (the latter omits unresolvable budgets —
  `any()` over it is a brake that passes having inspected
  nothing). Single-run `UsageLimits` vs cross-run `SpendLimits`
  layering documented.
- StepPersistence (`step_persistence/README.md`, full read):
  per-settled-step snapshots + append-only step events + tool-effect
  ledger keyed by `(run_id, tool_call_id)`, backends
  memory/file/SQLite/Mongo/Postgres (WAL, advisory-locked DDL,
  caller-owned pools, `anyio.to_thread` for sync backends).
  `complete` vs `interrupted` snapshots; default read path skips
  interrupted; `unknown_after_crash` for started-without-terminal
  effects; reuse of explicit `run_id` refused (deterministic
  provider tool-call IDs would collide). Three-level identity
  (`conversation_id` / `run_id` / `step_index`), `parent_run_id`
  lineage via ContextVar, `continue_run` vs `fork_run`, opt-in
  `max_snapshots_per_run` (retain set keeps both read modes
  correct), media externalization (content-addressed
  `media+sha256://`, 64 KiB threshold, per-backend stores, no blob
  GC — stated). Kill-tested frontier capture; explicit non-goals
  (no auto side-effect dedup, no capability-state restore, no
  exactly-once claim — "No Harness event can make an external side
  effect atomic with a local SQLite write").
- Coder composition (harness README): `FileSystem(root, traversal-
  checked, secrets read-only) + Shell(allow/deny lists, timeouts,
  credential-stripping, persistent) + RepoContext + SubAgents +
  ClearToolResults + WarnNearLimits + ToolOutputLimits +
  RepairToolArguments`. `LocalWorkspace` explicitly "not a sandbox";
  isolation via Modal/E2B/Sprites/Bubblewrap/SSH swap. `FileSystem
  root_dir` limits file tools only, not shell — stated.
- Commit feed at pin: MCPToolset applies `auth` through
  user-supplied http_client and stops closing it (#10051);
  200-with-`text/plain` → `ModelAPIError` (#10065); transport-error
  classification across 7 providers (#9336); Bedrock retention
  opt-in explanation (#10050); per-test budgets + shard balancing
  for CI cost (#10054/#10080); Anthropic cache-breakpoint TTL
  (#10057); `FallbackModel` charged once on retry (#10059).

# Findings for Runlith

- F-01 — Two-shape approval: serializable deferred (ends run,
  cross-process, durable) vs in-process await (CLI/TUI). Matches
  Runlith's agentd/CLI split; validates keeping both.
- F-02 — Guardrail verdict algebra (allow/block/replace/retry/
  approve) with input/output asymmetry and chain-ordered
  redaction. Richer than Runlith's ternary; `replace` and `retry`
  are unowned concepts.
- F-03 — Judge-as-filter with fail-closed uncertainty and fixed
  gate ordering (human first, model second; both must allow).
  Direct precedent for G7 planning/EvaluationPort advisory shape.
- F-04 — Spend brake semantics: per-response pricing, shared
  atomic counters, stated non-ledger guarantees, admission-vs-
  display split. Runlith has no cost dimension at all.
- F-05 — Persistence honesty: complete/interrupted snapshots,
  unknown-after-crash effects, no exactly-once claims, retention
  bounds that keep both read modes correct. Closest to Runlith's
  journal philosophy, with sharper vocabulary.
- F-06 — Documented control blind spots (output/external/provider
  tools invisible to guards; streaming pre-empts output screening;
  root_dir ≠ shell containment). A checklist Runlith's policy docs
  should mirror for its own seams.
- F-07 — Secret hygiene in traces: content attributes gated
  behind explicit opt-in; detectors rewrite rather than refuse
  (work preserved, history still holds the secret — stated).
- F-08 — Capability composition: whole agents decomposable to
  blocks with exact configuration published. Precedent for
  documenting Runlith service composition the same way.

# Comparison with our project

## What is better in Runlith

- Policy is deterministic and auditable: DENY > REQUIRE_APPROVAL >
  ALLOW over exact scopes, single-use, child-subset, fail-closed,
  hash-chained. Pydantic's gates are model-judged (calibration
  disclaimed — "no severity scale… deliberately") or human-
  judged with no scope/expiry/audit of the authorization itself.
- Durable substrate is local and content-addressed: SQLite + event
  journal with no external engine. Pydantic durability *is*
  Temporal/DBOS/Prefect — an operated distributed system per
  deployment.
- Secrets boundary (G6: OS keychain, blind delivery) vs detectors-
  as-regex plus judge disclosure ("the whole validated argument
  dict goes to the judging model's provider before the verdict").
- No irreversible-action ambiguity: runner boundary + worktree
  admission vs `Shell` allowlists over an explicitly non-sandbox
  workspace.

## What is worse in Runlith (gaps this exposes)

- No `replace`/`retry` verdicts: redact-and-continue and ask-
  model-to-redo don't exist in the policy language (P3, R-02).
- No second-model advisory gate before tool execution; G7
  planning verification has no runtime counterpart (P3, R-03).
- No cost dimension: no per-run/per-tenant budgets, no brake,
  no admission-vs-display split (P3, R-04).
- Resume vocabulary is thinner: no complete/interrupted
  distinction, no unknown-after-crash effect state, no
  continue-vs-fork naming (P2, R-05).
- Policy docs don't enumerate their own blind spots the way
  guardrails docs do (P3, R-06).

## What is better in the source

- Approval round-trip is the most careful seen so far: ID-keyed
  results, unknown-ID rejection, remaining-set computation,
  override_args, policy-change re-evaluation on resume.
- SpendLimits is the rare budget doc that states what it does
  *not* guarantee, down to float-residue arithmetic and cluster
  slot costs.
- StepPersistence names every non-goal (dedup, GC, exactly-once,
  capability-state) instead of implying them.
- Detectors document per-pattern precision failures (SSN-years,
  IBAN shapes, absent AWS-secret pattern) rather than claiming
  coverage.

## What is worse in the source

- Trust model is additive allowlists + model judgement over an
  unsandboxed workspace; `hidden` tools and prompt-injection
  defender are mitigations, not a boundary.
- Judge ships call arguments to a third-party provider pre-
  verdict; conversation-inclusion widens injection surface by
  design (both stated, both accepted).
- Durable correctness depends on caller discipline
  (deterministic price/scope callables, explicit judge ids,
  same-process `run_id` for dedup tokens).
- Harness is 0.x with moving APIs; capability IDs and spec
  schemas are still stabilizing.

# Opportunities and recommendations

- R-01 — Cite deferred-vs-in-process approval shapes (F-01) as
  `EXTERNAL_VALIDATION` for agentd/CLI approval split. P3.
- R-02 — Evaluate `replace` (redact-and-continue) and `retry`
  verdicts for the policy language; `replace` threads before
  re-checks. P3, `NOVEL_GAP` → `ADAPT` (needs ADR; cite
  OpenShell R-02 for secret paths, Bitdefender R-03 checklist
  discipline).
- R-03 — Evaluate advisory second-model gate before tool
  execution with fail-closed uncertainty + fixed ordering
  (human first, advisory second, both must allow), purely
  advisory to EvaluationPort. P3, `NOVEL_GAP` → `ADAPT`
  (spike behind port; cf. langgraph R-08, utopia R-02).
- R-04 — Add cost dimension: per-run/per-tenant budgets as
  brake-not-ledger with admission-vs-display split (F-04).
  P3, `NOVEL_GAP` → `ADAPT` (requirements-level; no provider
  coupling).
- R-05 — Adopt continue-vs-fork + complete/interrupted +
  unknown-after-crash vocabulary for journal recovery docs;
  retention bounds must keep all read modes correct (cf.
  langgraph R-01/F-06). P2, `PARTIALLY_EXISTS` → `ADAPT`.
- R-06 — Document policy blind spots explicitly (which tool
  classes bypass which gates; streaming vs screening order;
  containment limits), mirroring F-06. P3, `NOVEL_GAP` →
  `ADAPT` (docs-only).
- R-07 — Cite trace content-gating + rewrite-over-refuse
  detector posture (F-07) as `EXTERNAL_VALIDATION` for audit/
  logging hygiene (cf. langgraph R-06). P3.
- REJECT: SDK/framework dependency; Temporal/DBOS/Prefect
  durability engines; Gateway/Logfire coupling; Harness
  capabilities as code; Monty/models/realtime/voice/evals
  import; judge-as-security-boundary (source itself disclaims);
  regex detectors as injection defense.

# Evidence and references

- Pin: `main@69ea1e57…` via Atom feed `commits/main.atom`
  (2026-10-10T13:18:50Z; REST API rate-limited as with langgraph).
- Full pinned raw URLs (verified at pin):
  - README (agent/capability/durability overview):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/README.md
  - deferred types (HITL round-trip, full read):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/pydantic_ai_slim/pydantic_ai/_deferred.py
  - deps/versioning (dynamic version):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/pydantic_ai_slim/pyproject.toml
  - guardrails (verdicts, chains, detectors, HITL, tracing):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/src/pydantic_ai_harness/pydantic_ai_harness/guardrails/README.md
  - spend (budgets, Redis store, guarantees):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/src/pydantic_ai_harness/pydantic_ai_harness/spend/README.md
  - step persistence (snapshots, ledger, backends, non-goals):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/src/pydantic_ai_harness/pydantic_ai_harness/step_persistence/README.md
  - tool-call judge (filter posture, ordering, durability):
    https://raw.githubusercontent.com/pydantic/pydantic-ai/69ea1e57201ed59e7d30fbe4a7590d193acecce0/src/pydantic_ai_harness/pydantic_ai_harness/tool_call_judge/README.md
  - harness index (coder composition, workspaces, 50+ caps):
    https://github.com/pydantic/pydantic-ai/tree/69ea1e57201ed59e7d30fbe4a7590d193acecce0/src/pydantic_ai_harness
- Commit evidence (feed bodies): #10051 MCPToolset auth +
  no-close-user-client; #10065/#9336 provider error
  classification; #10050 Bedrock retention opt-in; #10054/#10080
  CI cost guardrails; #10057 cache TTL; #10059 fallback
  single-charge.
- Not inspected: models/*, realtime/*, evals bodies,
  pydantic_graph, clai internals, shell/memory/compaction
  sources, tests.
- Runlith references: AGENT.md §§4/7; G6/G7 notes; langgraph
  R-01/R-02/R-08; OpenShell R-02; Bitdefender R-03; MegaMemory
  R-02; utopia R-02; Karpathy study R-02.

# Final assessment

Highest-yield source so far for policy language: the verdict
algebra (R-02), fail-closed advisory gate (R-03), and brake-
not-ledger budgets (R-04) are all P3 ADAPT candidates with the
source's own caveats doing half the ADR work — especially its
habit of documenting non-guarantees. StepPersistence vocabulary
(R-05) is the one P2 item. Nothing structural transfers: engines,
SDK, gateway, and model-judged enforcement stay REJECT, with the
source's own "filter, not a security boundary" as the citation.
No IKQ entry: requirements-level deltas only.
