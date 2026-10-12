# Re-audit

langfuse/langfuse — open source LLM engineering platform
(tracing/observability, prompt management, evaluations,
datasets/experiments) for LLM app teams; TypeScript monorepo
with Postgres + ClickHouse + Redis/Valkey + S3 self-host stack.

- repo: https://github.com/langfuse/langfuse
- reviewed_revision: `a416d22f9534609b8850d17ea2e8f2531fbcbfde`
  (main, 2026-10-10T20:21:03Z — `chore(worker): log the row a
  ClickHouse parse error names (#18577)`)
- version at pin: tagged semver releases with a stated
  versioning policy (`/self-hosting/upgrade/versioning`); no
  static version field read at pin
- license: MIT (core); `ee/`, `web/src/ee/`, `worker/src/ee/`
  licensed separately under `ee/LICENSE`; copyright
  2023–2026 ClickHouse, Inc. (part of ClickHouse since Jan 2026)
- language/stack: TypeScript monorepo (`web` Next.js app +
  `worker`), PostgreSQL (transactional), ClickHouse
  (observability), Redis/Valkey (cache + queue), S3/blob
  (raw events + media); Python + JS/TS SDKs, OTel-native
  ingestion
- method: raw-file reads at pin (README, LICENSE) + Atom
  commit feed + docs pages (docs live in the separate
  `langfuse-docs` repo, unpinned — see scope note). No clone.
  Scoped to trace/observation model, scores, datasets/
  experiments, evaluator hooks, prompt versioning, self-host
  architecture — implementation bodies, Prisma schema,
  worker code not line-read.

# What it is (source summary)

Langfuse is an LLM-observability and evaluation backend.
Apps instrument with SDKs (`@observe()` decorator,
`startObservation`, framework integrations) or plain
OpenTelemetry; the SDKs batch in the background and flush.
The backend stores one denormalized observations table
(trace attributes copied onto every observation row) in
ClickHouse, transactional data in Postgres, raw events in
S3, queues in Redis. On top sit prompt management
(immutable versions + movable labels, client/server cached),
evaluations (LLM-as-a-judge, decision-model, code, human
annotation, API/SDK scores unified as one `Score` object),
datasets + experiments (offline regression loop), and a
playground. Same codebase ships Langfuse Cloud and
self-hosted Docker/Helm. Judge executions run in the
worker, and each judge execution itself emits a full trace.

# Architecture (as evidenced)

- Ingestion pipeline (self-hosting docs, full read):
  SDK → Web (batch intake, immediate S3 write, Redis queue
  ref) → Worker (S3 → ClickHouse). Optimizations stated
  plainly: queued ingestion, in-memory API-key cache,
  prompt read-through Redis cache, wide immutable
  observations table (no read-time joins), media direct-
  to-S3, S3-first recoverability ("even if the database is
  temporarily unavailable, the events are not lost").
  Short-lived apps must call `flush()` or buffered traces
  die with the process — documented with sequence diagrams.
- Trace model (data-model docs, full read): `Session 1—n
  Trace 1—n Observation`. Langfuse calls spans
  observations; `span` is also one observation type.
  Observation types: `event | span | generation | agent |
  tool | chain | retriever | evaluator | embedding |
  guardrail`. Observation-level evaluators see only the
  matched observation (no siblings/children); "logical
  root observation" (`isRootObservation` filter) is the
  named workaround. v4 is observations-first; trace-level
  evaluators deprecated with a Nov 2026 cutover.
- Scores (scores overview, full read): the universal eval
  object — `name + value + data type + optional comment`,
  attached to traces, observations, sessions, or dataset
  runs. Four types: `NUMERIC` (float), `CATEGORICAL`
  (predefined labels), `BOOLEAN` (0/1), `TEXT` (free-form,
  explicitly excluded from experiments, judges, and
  analytics). Scores-vs-tags rule: tags describe *what*
  (set at trace time, immutable); scores measure *how
  good* (addable any time later). Comments carry judge
  reasoning.
- Evaluators + rules (core-concepts + llm-as-a-judge,
  full reads): evaluators define *how* (judge prompt +
  model + output definition + variable mappings);
  rules define *which* (observation filters + sampling
  rate → trigger evaluators). Evaluator definitions are
  versioned; active rules use latest. Online (live,
  observation-level), batch (selected history), backfill
  (up to 6 months / 25k observations with cost estimate).
  Decision-model evaluators give typed verdicts; code
  evaluators run sandboxed in a separate worker queue.
  Score/cost threshold alerts; human-vs-AI agreement
  (Cohen's kappa, confusion matrix) in score analytics.
- Datasets/experiments (data-model docs, full read):
  `Dataset{name, description, metadata}` 1—n
  `DatasetItem{input, expectedOutput, metadata,
  sourceTraceId, sourceObservationId, status
  ACTIVE|ARCHIVED}`; item IDs are project-unique with
  upsert semantics. `DatasetRun` 1—n `DatasetRunItem`
  linking item→trace (+observationId for compat).
  Task / evaluator / run-evaluator function defs for
  SDK runners. Stated constraint: no repetitions (one
  item once per experiment, #5855). Local (non-hosted)
  datasets supported in v4.
- Prompt management (data-model + get-started, full
  reads): text vs chat types (immutable after creation);
  immutable version history; labels as pointers
  (`production` default, `latest` auto, custom incl.
  tenant/A-B). Runtime resolves via label; deploy =
  move label, rollback = move it back, no code change.
  Client-side caching + server read-through; "not on the
  critical path"; prompts linkable to traces for
  per-version analysis.
- Commit feed at pin: ClickHouse poison-row diagnostics
  (#18577); `X-RateLimit-Reset` epoch-seconds fix
  (#18573); decision-model evaluators graduated (#18541);
  RBAC role-assignment storage reshape (#18490); FIPS
  PBKDF2 password hashing (#18444); API keys drop bcrypt
  for SHA-256 fast hash (#18442); ClickHouse
  read-in-order tuning (#18542).

# Findings for Runlith

- F-01 — Denormalized observations table (trace attrs on
  every row) trades write amplification for join-free
  reads. Runlith's journal makes the opposite trade
  (normalized, hash-chained). Both state the reason.
- F-02 — Observation-type taxonomy (`agent | tool |
  generation | guardrail | evaluator | …`) is a step-
  classification vocabulary Runlith's journal lacks;
  execution steps are untyped by comparison.
- F-03 — Score quadruple (`NUMERIC | CATEGORICAL |
  BOOLEAN | TEXT` + comment-as-reasoning) with an
  explicit aggregation boundary (TEXT excluded) is the
  cleanest eval data object seen so far; directly shapes
  an EvaluationPort verdict type.
- F-04 — Evaluator/rule split (*how* vs *which*,
  sampling, per-assignment mapping overrides,
  latest-wins versioning) is the reusable half of
  online evaluation; the running half is worker queue
  machinery Runlith must not import.
- F-05 — Dataset item lineage (`sourceTraceId /
  sourceObservationId`, `ACTIVE|ARCHIVED`, project-
  unique upsert IDs) plus a stated no-repetition
  constraint is honest fixture design with the limits
  printed on the label.
- F-06 — Prompt version/label split (immutable
  versions, movable labels, label-driven deploy/
  rollback without code change) plus cache-off-
  critical-path posture is the transferable pattern
  for versioned prompts/config in Runlith.
- F-07 — Ingestion durability honesty: S3-first
  persistence, poison-row diagnostics that never log
  payload values, explicit `flush()` requirement —
  "recoverability of events" as a goal with no
  exactly-once claim. Closest public analogue to
  Runlith's journal-first stance.
- F-08 — Evaluation-of-evaluation discipline:
  judge executions emit full debug traces, judges
  calibrated against human labels (agreement metrics),
  cost tracked per evaluator with threshold alerts.
  Runlith's EvaluationPort has no calibration story.

# Comparison with our project

## What is better in Runlith

- Durable substrate is local and tamper-evident: SQLite
  + hash-chained event journal in one daemon. Langfuse
  needs Postgres + ClickHouse + Redis + S3 to stand up,
  and its analytics rows are mutable with no
  tamper-evidence story.
- Policy enforcement is deterministic (DENY >
  REQUIRE_APPROVAL > ALLOW, fail-closed) while Langfuse
  "guardrails" are observation types and score checks —
  post-hoc judgments, some shipped to third-party LLM
  providers pre-verdict, never a boundary.
- Secrets boundary (G6: OS keychain, blind delivery)
  vs traces designed to carry full inputs/outputs, with
  masking as configuration rather than blindness.
- No cloud dependency and no license-gated core: `ee/`
  dirs, org management, and several admin features sit
  behind a license key; Langfuse Cloud is the primary
  artifact, self-hosting the secondary.

## What is worse in Runlith (gaps this exposes)

- No step-type taxonomy: journal/audit events lack
  `agent/tool/generation/guardrail/evaluator`-style
  classification (P3, R-02).
- No universal verdict object: EvaluationPort outputs
  have no `name/value/type/comment` shape with an
  aggregation boundary (P2, R-01).
- No dataset/experiment shape: regression fixtures lack
  `input/expectedOutput/metadata/source-refs/status`
  structure and production→dataset feedback loop (P2,
  R-03).
- No version/label split for prompts or policy text:
  immutable history vs movable deploy pointers
  unmodelled (P3, R-04).
- No evaluator/rule separation, sampling, or judge
  calibration practice for advisory gates (P3, R-05/R-06).

## What is better in the source

- Score object design (F-03): name/value/type/comment,
  attach-anywhere, TEXT aggregation boundary, tags-vs-
  scores rule — the most reusable eval datum reviewed.
- Dataset lineage fields + stated constraints (F-05);
  limits (no repetitions) filed as issues, not hidden.
- Prompt label mechanics (F-06): deploy/rollback by
  moving a pointer, with caching that keeps prompts off
  the critical path.
- Pipeline honesty (F-07): S3-first recoverability,
  payload-free diagnostics, documented flush
  obligation, per-query ClickHouse tuning notes.
- Evaluator versioning with latest-wins rules and
  pre-save model-validity checks (structured-output
  probe before an evaluator can be saved).

## What is worse in the source

- Analytics store is mutable and provider-coupled:
  scores/judgments carry no authorization, expiry, or
  audit-of-verdict; judges exfiltrate call content to
  model providers by design.
- Four infrastructure dependencies for what Runlith
  does in one SQLite file; self-hosting is operate-it-
  yourself (backups, HA, upgrades, background
  migrations all on the operator).
- Migration churn: v4 trace→observation cutover
  deprecates trace-level evaluators on a deadline;
  observation-scoping caveats (no sibling/child data)
  need a "logical root" workaround.
- External audit UX is dashboard/analytics-oriented
  (Bitdefender R-audit lenses cover investigation
  surfaces more directly); no hash-chained or
  tamper-evident construct anywhere.

# Opportunities and recommendations

- R-01 — Adopt a Score-shaped verdict object for
  EvaluationPort outputs: `name + value + type
  (NUMERIC|CATEGORICAL|BOOLEAN|TEXT) + comment`, with
  TEXT excluded from aggregation and comments carrying
  judge reasoning (F-03). P2, `NOVEL_GAP` → `ADAPT`
  (schema-level; no provider coupling).
- R-02 — Adopt a step-type taxonomy subset
  (`agent|tool|generation|guardrail|evaluator`) as
  labels on journal/audit steps for filtering and
  review (F-02). P3, `NOVEL_GAP` → `ADAPT` (vocabulary
  only; cf. pydantic-ai R-06 blind-spot docs).
- R-03 — Adopt dataset-item shape
  (`input/expectedOutput/metadata/source-refs/
  ACTIVE|ARCHIVED`) for Runlith regression fixtures,
  including production→dataset feedback (failed runs
  become items) (F-05). P2, `NOVEL_GAP` → `ADAPT`
  (cf. pydantic-ai R-05 continue/fork vocabulary).
- R-04 — Adopt version/label split for versioned
  prompts/config: immutable versions, movable
  `production|latest|custom` labels, deploy by moving
  a pointer (F-06). P3, `NOVEL_GAP` → `ADAPT` (needs
  ADR; cache-off-critical-path posture included).
- R-05 — Cite evaluator/rule split + sampling +
  per-assignment mapping overrides (F-04) as
  `EXTERNAL_VALIDATION` for advisory EvaluationPort
  wiring (cf. pydantic-ai R-03 fail-closed gate). P3.
- R-06 — Take judge-executions-are-traces +
  human-agreement calibration (F-08) as
  `INSPIRED_ONLY` for EvaluationPort self-observation
  and calibration practice. P3 (spike, not schema).
- R-07 — Cite S3-first recoverability + payload-free
  poison-row diagnostics + flush obligation (F-07) as
  `EXTERNAL_VALIDATION` for journal-first durability
  posture (cf. pydantic-ai R-05/R-07). P3.
- REJECT: TypeScript/Next.js code; ClickHouse/
  Postgres/Redis/S3 stack; web/worker split and queue
  machinery; Cloud service and SDK/OTel ingestion;
  LLM-as-a-judge as enforcement or security boundary;
  playground; EE-gated features; usage/cost tracking
  as implemented; trace-level evaluator APIs (already
  deprecated upstream).

# Evidence and references

- Pin: `main@a416d22f…` via Atom feed
  `commits/main.atom` (2026-10-10T20:21:03Z; REST API
  not attempted — Atom sufficed).
- Full pinned raw URLs (verified at pin):
  - README (platform overview, features, self-host):
    https://raw.githubusercontent.com/langfuse/langfuse/a416d22f9534609b8850d17ea2e8f2531fbcbfde/README.md
  - LICENSE (MIT core + `ee/` separation):
    https://raw.githubusercontent.com/langfuse/langfuse/a416d22f9534609b8850d17ea2e8f2531fbcbfde/LICENSE
- Docs pages (live docs, separate `langfuse-docs`
  repo — unpinned, v4-era; concepts quoted, not code):
  - data model (traces/observations/sessions):
    https://langfuse.com/docs/observability/data-model
  - observation types (span taxonomy):
    https://langfuse.com/docs/observability/features/observation-types
  - scores overview (universal eval object):
    https://langfuse.com/docs/evaluation/scores/overview
  - core concepts (evaluators/rules, online/batch):
    https://langfuse.com/docs/evaluation/core-concepts
  - LLM-as-a-judge (evaluator setup, worker exec):
    https://langfuse.com/docs/evaluation/evaluation-methods/llm-as-a-judge
  - experiments data model (dataset/run shapes):
    https://langfuse.com/docs/evaluation/experiments/data-model
  - prompt data model (versions/labels/caching):
    https://langfuse.com/docs/prompt-management/data-model
  - prompt get-started (lifecycle workflow):
    https://langfuse.com/docs/prompt-management/get-started
  - self-hosting (containers, storage, scaling):
    https://langfuse.com/self-hosting
- Commit evidence (feed bodies): #18577 poison-row
  diagnostics; #18573 rate-limit epoch fix; #18541
  decision-model graduation; #18490 RBAC reshape;
  #18444 FIPS PBKDF2; #18442 API-key bcrypt removal;
  #18542 ClickHouse buffering tuning.
- Not inspected: implementation bodies, Prisma
  schema, worker/web sources, SDK sources, EE dirs,
  tests, playground internals.
- Runlith references: AGENT.md §§4/7; G6/G7 notes;
  pydantic-ai R-03/R-05/R-06/R-07; Bitdefender audit
  lenses (investigation-surface contrast only).
- Scope note: docs describe Langfuse v4
  (observations-first, trace-level evaluators
  deprecated); data-model claims are doc-stated and
  were not verified against the pinned schema.

# Final assessment

Highest-yield source so far for eval data models: the
Score object (R-01), dataset-item shape (R-03), and
prompt version/label split (R-04) are all schema-level
ADAPT candidates with no stack coupling — the transfer
is vocabulary and shapes, never code. The step taxonomy
(R-02) and evaluator/rule split (R-05) are P3 validations
of directions Runlith already owns (typed audit steps,
advisory EvaluationPort). Nothing structural transfers:
four-dependency platform, worker machinery, cloud
service, and model-judged enforcement stay REJECT, with
the source's own "scores, not boundaries" posture as the
citation. No IKQ entry: requirements-level deltas only.
