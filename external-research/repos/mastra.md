# Re-audit

mastra-ai/mastra — TypeScript agent/workflow framework with
durable suspend-resume, per-tool approval, scorers, and
OpenInference-style observability.

- repo: https://github.com/mastra-ai/mastra
- reviewed_revision: `2d177b025ddea32aa511c3a9d5a9ac78f7290bbc`
  (main, 2026-10-10T19:48:46Z — `fix(factory): close custom-board
  cards when their tracked source closes (#26621)`)
- version at pin: monorepo, no single version; `@mastra/evals`
  `1.10.6-alpha.0` per its `package.json`; engines `node >= 22.13.0`
- license: dual — Apache-2.0 for core and vast majority;
  Mastra Enterprise License for any `ee/` directory
  (`@mastra/core/auth/ee`, `agent-builder/ee`, `@mastra/editor/ee`)
- language/stack: TypeScript ESM monorepo (`packages/core`,
  `packages/memory`, `packages/evals`, `packages/server`,
  `packages/loggers`, `packages/mcp`, `packages/rag`, …);
  storage backends incl. LibSQL; docs site mastra.ai
- method: raw-file reads at pin + Atom commit feed. No clone.
  Scoped to suspend/resume types, tool approval + suspend,
  evals scorers, memory head, observability span types —
  execution-engine internals, agent loop, storage backends,
  tests, docs bodies not line-read.

# What it is (source summary)

A framework for agents (model loop + tools + memory + processors)
and graph workflows (`.then()/.branch()/.parallel()/.foreach()/
.sleep()/.map()`) on one durable substrate: runs persist step
snapshots to storage, steps suspend for human input and resume
indefinitely later, tools carry their own suspend/resume schemas
plus a `requireApproval` gate, scorers attach per-step or per-run,
and every subsystem emits typed spans (agent, model, tool, memory,
workflow, scorer, processor, RAG, skills) through a pluggable
observability instance. Suspend is a type-level brand (`InnerOutput`
only producible by `suspend()`), resume is schema-validated data
injected at the suspended step, and recovery (boot-time restart,
resume-claim dedup, time-travel restart) is configurable per
workflow. Memory layers conversation history, working memory,
semantic recall, and observational (background observe/reflect)
passes behind one `settled()` join handle.

# Architecture (as evidenced)

- Suspend/resume vocabulary (`workflows/types.ts`,
  `workflows/step.ts`, full reads): per-step `StepResult`
  `success | failed | suspended | running | waiting | paused |
  skipped | canceled`; run-level `WorkflowRunStatus` adds
  `tripwire | pending | bailed`. `suspend(payload?, {resumeLabel?})`
  returns branded `InnerOutput` (uninhabitable except via
  `suspend()`); companions `bail(result)` and `abort()`.
  Per-step `resumeSchema`/`suspendSchema` validated on both ends;
  `suspendedAt`/`resumedAt` timestamps on results; `tracingContext`
  persisted across suspend so resumed spans parent under the
  suspended span. `WorkflowOptions` documents hooks honestly:
  `onStart` is a pre-flight gate (throws reject `start()`, run
  stays `pending`, fires only on first start — never resume/
  restart); `onFinish`/`onError` errors are swallowed+logged.
  Recovery surface: `RestartExecutionParams`,
  `TimeTravelExecutionParams`, `autoRestartActiveRuns`,
  `shouldPersistSnapshot` predicate,
  `pruneSnapshot` pure-function requirement (JSON-safe, may cross
  pubsub codec), `allowUnclaimedResumes` (opts out of resume-claim
  dedup with eyes open). Nested-workflow results wrapped in
  `NESTED_WORKFLOW_RESULT_SYMBOL`, documented in-memory-only —
  "never serialized to storage."
- Tool approval + tool suspend (`core/src/tools/tool.ts`, full
  read): `requireApproval: boolean | ((ctx) => boolean)` per tool
  (doc example: skip approval for dry runs); `needsApprovalFn`
  runtime-resolved authoritative predicate, also set by the MCP
  client wrapping a server-level `requireToolApproval`. The
  `execute` wrapper captures `suspendData` via context-wrapped
  `suspend`, validates it against `suspendSchema`, skips output
  validation when suspended with `undefined` output, skips input
  re-validation on resume (args already validated), and validates
  `resumeData` against `resumeSchema`. Tools carry MCP annotations
  (`readOnlyHint`, `destructiveHint`) and `toModelOutput` /
  `transform` (model-facing vs stored output split; `MAPPING`
  spans trace the reshape).
- Scorers (`evals` package.json + `scorers/code/index.ts` +
  `tool-call-accuracy/index.ts`, full reads): `createScorer`
  pipeline (`preprocess` then `generateScore`); deterministic
  code scorers (completeness, textual-difference,
  keyword-coverage, content-similarity, tone,
  tool-call-accuracy, trajectory, checks) beside LLM scorers;
  tool-call-accuracy checks expected-tool presence or expected
  order with `strictMode` (exact sequence vs subsequence);
  scorers attach per-step (`scorers` on `Step`) or per-run;
  first-class spans `SCORER_RUN`/`SCORER_STEP` with judge-model
  attribution; vitest integration subpath.
- Memory (`memory/src/index.ts`, head + class skeleton read,
  body skimmed): `Memory extends MastraMemory` — history
  (count/token-bounded loading), working memory (resource |
  thread scope, mutex-guarded updates, atomic-merge support
  probe), semantic recall (thread | resource scope, topK /
  message-range / threshold, missing-`resourceId` throws for
  resource scope), observational memory (background observe/
  reflect model passes recorded as `MEMORY_OPERATION` spans,
  experimental Subconscious). Recall hides system-reminder and
  signal messages by default; thread delete drains in-flight
  observation cycles (10s cap) then deletes vectors in
  background; `settled()` joins all background work before
  storage close ("background statements can race the close").
  Working-memory helpers deliberately *copied* from core with
  keep-in-sync notes (peer-range compatibility over imports).
- Observability (`core/src/observability/types/tracing.ts`,
  partial read — 1414 of 2185 lines): ~30 `SpanType` values
  (agent/model-step/model-inference/tool/client-tool/provider-
  tool/mcp/memory/workflow-step/conditional/parallel/loop/sleep/
  wait-event/processor/skill-action/workspace/rag-*/mapping);
  per-phase processor input/output payloads with diff-not-
  snapshot rule (absent key means unchanged); `internalUsage`
  rolled up to the nearest exported ancestor when internal spans
  are filtered (cost attribution survives hidden spans);
  `toolChoice`/`responseFormat`/per-call tool definitions on
  inference spans; TTFT via `completionStartTime`; tripwire
  payloads on agent spans; `TracingOptions`/`TracingPolicy`
  threaded through `WorkflowRunStartOptions` and step params.
- Commit feed at pin: boot-time workflow recovery made faster +
  configurable (#26607) with `recoveryConfig` assertion fixes
  (#26617); suspending workflow tools serialized in tool-call
  concurrency gates (#26608); LibSQL local connections reopened
  after `SQLITE_BUSY` (#26261); provider-error retry for
  Gemini `MALFORMED_FUNCTION_CALL` (#26612); Upstash log
  transport buffer bounded during outages (#26605); slow email
  redaction on long log strings fixed (#26620 — redaction
  exists in the template path); client-js subscription stops
  reconnecting on terminal HTTP errors (#26611).

# Findings for Runlith

- F-01 — Suspend as type-level brand with schema-typed resume:
  `InnerOutput` producible only by `suspend()`; resume/suspend
  payloads validated against per-step schemas on both ends.
  Closest typed HITL contract seen; Runlith's approval resume
  path has no equivalent static shape.
- F-02 — Status vocabulary wider than Runlith's: `waiting`,
  `paused`, `bailed`, `tripwire`, `pending` alongside
  `suspended`; `suspendedAt`/`resumedAt` timestamps standard on
  step results. Direct input to journal recovery docs.
- F-03 — Conditional per-call approval (`requireApproval` as
  predicate, dry-run example) plus one authoritative resolved
  predicate (`needsApprovalFn`), with MCP server-level default
  flowing into per-tool resolution. Precedent for computed
  policy inputs without abandoning determinism.
- F-04 — Resume discipline: persisted resume-claim dedup,
  explicit opt-out (`allowUnclaimedResumes`), concurrency-gate
  serialization of suspending tools, separate concurrent-resume
  test. A checklist Runlith's resume path should mirror.
- F-05 — Scorer pipeline split (`preprocess` evidence shaping
  vs `generateScore`) with deterministic code scorers next to
  LLM judges, and strict-vs-subsequence order checks. Direct
  precedent for EvaluationPort evidence/verdict separation.
- F-06 — Observability honesty patterns: diff-not-snapshot
  processor payloads, `internalUsage` rollup surviving filtered
  spans, `onStart`-as-gate vs `onFinish`-as-observer error
  semantics documented on the type itself.
- F-07 — Shutdown discipline: `settled()` join over background
  cycles + vector cleanup before storage close; delete drains
  in-flight observers before removing vectors (late writes
  checked against deleted threads). "Statements can race the
  close" is stated, not implied.
- F-08 — Boot recovery configurability: per-workflow opt-out
  of blanket restart (`autoRestartActiveRuns: false` for
  workflows whose recovery is owned elsewhere), snapshot
  predicate + pure `pruneSnapshot` to bound write volume.

# Comparison with our project

## What is better in Runlith

- Policy is deterministic and total: DENY > REQUIRE_APPROVAL >
  ALLOW over exact scopes, single-use, child-subset,
  fail-closed, hash-chained. Mastra approval is a boolean (or
  an arbitrary async predicate) with no scope, expiry, or audit
  of the authorization itself, and MCP annotations are hints,
  not enforcement.
- Durable substrate is local and content-addressed: SQLite +
  event journal, no operated engine. Mastra durability is
  storage-backend snapshots plus engine-specific execution
  (default/evented/Inngest/Temporal) — a deployment decision
  per install.
- Secrets boundary (G6: OS keychain, blind delivery) vs no
  evidenced secret story on the Mastra side beyond log-string
  email redaction and detectors-free traces carrying tool
  args, prompts, and message payloads on spans by default.
- Isolation is structural (worktree admission + runner
  boundary) vs Mastra workspace actions over filesystem/
  sandbox providers selected per call, with no admission
  concept in the evidence read.

## What is worse in Runlith (gaps this exposes)

- No waiting/paused/bailed/tripwire run states and no
  suspended/resumed timestamps on journal entries (P2, R-01).
- No resume-claim dedup and no stated concurrent-resume
  serialization rule (P2, R-02).
- No conditional-approval predicate shape (e.g. dry-run
  bypass) and no static risk annotations on tools feeding
  policy (P3, R-03/R-04).
- No evidence/verdict split in evaluation; G7 verification has
  no runtime scorer counterpart with strict-vs-subsequence
  checks (P3, R-05).
- No `settled()`-style shutdown join or per-workflow boot-
  recovery ownership flag (P3, R-06/R-07).

## What is better in the source

- Suspend/resume is the most completely typed HITL contract
  reviewed: brand, schemas, labels, timestamps, trace
  continuity, claim dedup, concurrency tests.
- Tool wrapper validates at every transition (input on first
  run, resume data on resume, suspend payload on suspend)
  while skipping exactly the re-validation that would break
  resume — each skip reasoned in comments.
- Scorer packaging (prebuilt/code/llm/utils/checks/vitest
  subpaths) plus span-attributed judge identity is the
  cleanest eval attach story reviewed.
- `WorkflowOptions` documents hook failure semantics
  (gate-vs-observer) where most frameworks leave it to
  folklore.

## What is worse in the source

- Approval is enforcement-thin: boolean-or-callback with no
  scope ladder, no expiry, no authorization audit; an async
  predicate can smuggle nondeterminism into the gate.
- Durability correctness depends on backend + engine choice
  and caller discipline (deterministic predicates, JSON-safe
  prune functions, shared-graph mutation races called out in
  `ForeachConcurrencyResolver` docs).
- Traces default to carrying prompts, tool schemas, and
  message payloads; hygiene is exporter/policy configuration,
  not a boundary.
- Dual-license `ee/` fence (auth, agent-builder, editor) means
  part of the platform surface is source-available, not open
  source — relevant if borrowing concepts near those areas.

# Opportunities and recommendations

- R-01 — Adopt waiting/paused/bailed/tripwire vocabulary +
  `suspendedAt`/`resumedAt` timestamps for journal recovery
  docs (F-02). P2, `PARTIALLY_EXISTS` → `ADAPT` (docs and
  event schema; cf. pydantic-ai R-05).
- R-02 — Add resume-claim dedup with explicit opt-out and a
  concurrent-resume serialization rule (F-04). P2,
  `NOVEL_GAP` → `ADAPT` (needs ADR; cite langgraph R-01
  checkpoint discipline).
- R-03 — Evaluate a pure conditional-approval predicate shape
  (static context such as dry-run flags only, never model
  output) feeding the deterministic policy engine (F-03).
  P3, `NOVEL_GAP` → `ADAPT` (requirements-level; predicate
  inputs must stay enumerable for audit).
- R-04 — Evaluate static tool risk annotations
  (read-only/destructive hints) as advisory policy inputs
  (F-03). P3, `NOVEL_GAP` → `ADAPT` (metadata only; hints
  never substitute for scopes).
- R-05 — Cite scorer preprocess/generate split + strict-vs-
  subsequence order checks (F-05) as `EXTERNAL_VALIDATION`
  for EvaluationPort evidence/verdict separation. P3.
- R-06 — Adopt `settled()`-style shutdown join + boot-
  recovery ownership flag for agentd (F-07/F-08). P3,
  `PARTIALLY_EXISTS` → `ADAPT`.
- R-07 — Cite diff-not-snapshot payloads + gate-vs-observer
  hook error semantics (F-06) as `EXTERNAL_VALIDATION` for
  audit/journal hygiene. P3.
- REJECT: TypeScript framework dependency; engine/storage-
  backend coupling (Inngest/Temporal/LibSQL-remote/Upstash);
  MCP server/client code; model-provider registry; voice/
  realtime; Studio/playground; observational-memory LLM
  passes as policy input; async approval predicates as
  enforcement; `ee/`-fenced auth/agent-builder code.

# Evidence and references

- Pin: `main@2d177b02…` via Atom feed `commits/main.atom`
  (2026-10-10T19:48:46Z; default branch `main` confirmed —
  `main.atom` live, no fallback needed).
- Full pinned raw URLs (verified at pin):
  - README (framework/agents/workflows/HITL overview):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/README.md
  - license mapping (Apache-2.0 + `ee/` fence):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/LICENSE.md
  - workflow constants (pubsub/stream symbols, nested-
    workflow in-memory-only marker):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/workflows/constants.ts
  - step types (suspend brand, bail/abort, resume/suspend
    schemas, resume labels):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/workflows/step.ts
  - workflow types (StepResult/WorkflowRunStatus vocab,
    restart/time-travel params, hook gate semantics,
    snapshot/prune options):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/workflows/types.ts
  - tool class (requireApproval/needsApprovalFn, suspend
    capture + validation, MCP annotations):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/tools/tool.ts
  - evals package surface (version, subpath exports):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/evals/package.json
  - evals entry (subpath-only note):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/evals/src/index.ts
  - code scorer index (deterministic scorer set):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/evals/src/scorers/code/index.ts
  - tool-call-accuracy scorer (strict vs subsequence):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/evals/src/scorers/code/tool-call-accuracy/index.ts
  - memory head (history/working/semantic/observational
    layers, settled join, delete drain):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/memory/src/index.ts
  - observability entry (core types + context):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/observability/index.ts
  - observability types index:
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/observability/types/index.ts
  - tracing taxonomy (span types, attributes, payloads;
    partial read):
    https://raw.githubusercontent.com/mastra-ai/mastra/2d177b025ddea32aa511c3a9d5a9ac78f7290bbc/packages/core/src/observability/types/tracing.ts
- Tree pages read (2, the allowed maximum):
  - `packages/core/src/workflows` (file inventory incl.
    concurrent-resume, nested-suspend, timetravel tests)
  - `packages` (package inventory; no `observability`
    package — observability lives in core + `@mastra/
    observability` installable)
- Commit evidence (feed bodies): #26607 boot recovery +
  #26617 its assertion fixes; #26608 suspending-tool
  concurrency serialization; #26261 LibSQL SQLITE_BUSY
  reopen; #26612 Gemini error-processor retry; #26605
  Upstash buffer bound; #26620 email-redaction perf;
  #26611 terminal-error reconnect stop.
- Not inspected: execution-engine.ts, workflow.ts resume/
  recovery implementation, agent loop, processors, storage
  backends, server/deployer, docs bodies, tests (names only
  via tree page), `ee/` sources.
- Runlith references: AGENT.md §§4/7; G6/G7 notes;
  pydantic-ai R-02/R-03/R-05; langgraph R-01.

# Final assessment

Highest-yield TypeScript source so far for durable HITL
mechanics: the suspend brand + schema-validated resume (R-01
context, F-01), resume-claim dedup with opt-out (R-02), and
the conditional-approval predicate shape (R-03) are the items
that genuinely pressure Runlith's thinner resume and static
policy vocabulary — two P2 ADAPTs and restrained P3s. Scorer
and observability findings mostly validate G7/EvaluationPort
direction (R-05/R-07 EXTERNAL_VALIDATION) rather than opening
new gaps. Nothing structural transfers: framework, engines,
backends, MCP, providers, and LLM-judged gates stay REJECT,
with the source's own async-predicate nondeterminism and
trace-payload defaults as the citations. No IKQ entry:
requirements-level deltas only.
