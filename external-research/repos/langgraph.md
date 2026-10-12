# Re-audit

langchain-ai/langgraph — low-level orchestration framework for
stateful, long-running agents (Pregel-inspired).

- repo: https://github.com/langchain-ai/langgraph
- reviewed_revision: `b4991f1ba36a2449ab59c38ab71828ee865bff84`
  (main, 2026-10-10T11:57:46Z — `fix(langgraph): store an
  update_state input's writes on the checkpoint it updates (#9258)`)
- package version at pin: `langgraph 1.2.14` (`libs/langgraph/pyproject.toml`)
- license: MIT
- language/stack: Python 3.10+ (`langgraph`), 9 libs in monorepo:
  `langgraph`, `prebuilt`, `checkpoint`, `checkpoint-sqlite`,
  `checkpoint-postgres`, `checkpoint-conformance`, `cli`, `sdk-py`,
  `sdk-js`. JS twin `langgraphjs` is a separate repo.
- method: raw-file reads at pin + Atom commit feed (GitHub REST API
  403 rate-limited). No clone. Framework-scale repo; inspection is
  scoped to persistence, interrupts, store, and error surfaces —
  pregel loop internals, channels, prebuilt agents, CLI deploy, and
  SDKs were not line-read.

# What it is (source summary)

A graph-execution engine where nodes communicate through channels and
every superstep persists. From `README.md` at pin: durable execution
(resume exactly where it left off), human-in-the-loop (inspect and
modify state at any point), short-term + long-term memory, LangSmith
debugging/deployment. Public interface draws from NetworkX; execution
model from Pregel/Beam. Usable standalone or with LangChain products.

# Architecture (as evidenced)

- Thread/checkpoint model (`libs/checkpoint/.../base/__init__.py`):
  `thread_id` is the primary key; checkpoint addressed by
  `(thread_id, checkpoint_ns, checkpoint_id)`. `Checkpoint` carries
  `channel_values`, `channel_versions`, `versions_seen`
  (node → channel versions, drives next-node scheduling),
  `pending_sends`. `CheckpointMetadata.source` ∈
  `input | loop | update | fork`; step `-1` (input), `0` (first loop).
  Pending writes are `(task_id, channel, value)` tuples ordered by
  `writes_sort_key(task_path, task_id, idx)`; special writes
  (ERROR/SCHEDULED/INTERRUPT/RESUME) use negative indices via
  `WRITES_IDX_MAP`.
- `BaseCheckpointSaver` contract: `get_tuple / list / put /
  put_writes / delete_thread / delete_for_runs / copy_thread / prune`
  (+ async twins) plus beta `get_delta_channel_history`. Backends:
  `InMemorySaver` (explicitly debug/test-only), SQLite, Postgres,
  Redis. A `checkpoint-conformance` package runs capability suites
  against each saver; CI gaps there caused real bugs (#9261).
- Durability modes (`types.py: Durability`): `sync` (persist before
  next step), `async` (persist while next step executes, default),
  `exit` (persist only on graph exit). A whole recent fix series
  concerns `exit`/`async` write/checkpoint ordering (#9235, #9229).
- Interrupts (`types.py: interrupt`, `errors.py`): `interrupt(value,
  response_schema=...)` raises `GraphInterrupt` on first call in a
  node, surfaces `Interrupt(value, id, response_schema)` to the
  client; resume via `Command(resume=...)` re-executes the node.
  Requires a checkpointer. Invalid resume values raise
  `pydantic.ValidationError`, save nothing, and stay answerable;
  `is_invalid_resume()` marks them so `ToolNode` re-raises instead of
  blaming the model (#9232). `interrupt_id` is namespaced-hash
  (`xxh3_128` of checkpoint ns).
- State mutation from outside: `update_state(..., as_node=...)`,
  `Command(update, goto, resume)`, `Send(node, arg)` for map-reduce
  fan-out, `Overwrite` to bypass reducers on
  `BinaryOperatorAggregate` channels.
- Subgraphs: nested graphs with own namespaces; `Command.PARENT`
  routes upward; per-subgraph `Checkpointer = True | False | None`
  (on / off / inherit). Root-lifecycle rule: a run ends only on a
  *root* completed/failed event, subgraphs must not terminate the
  parent stream (#9228).
- Node policies (`types.py`): `RetryPolicy` (interval, backoff,
  attempts, retry_on), `TimeoutPolicy` (run_timeout wall-clock cap +
  idle_timeout progress watchdog, `refresh_on: auto | heartbeat`,
  cooperative asyncio cancellation), `CachePolicy` (key_func + ttl),
  `TracePolicy` (process_inputs/process_outputs transforms — "not
  intended to redact secrets", points at LangSmith
  hide_inputs/outputs instead). Node failures can route to
  `error_handler(state, NodeError) -> Command` (`errors.py:
  NodeError{node, error}`).
- Store = long-term memory (`libs/checkpoint/langgraph/store/base/`):
  `BaseStore.batch/abatch` over `GetOp | SearchOp | PutOp |
  ListNamespacesOp`; hierarchical tuple namespaces with prefix/suffix
  match + wildcards; per-item TTL (minutes, refresh-on-read, sweep);
  optional embedding index (`dims`, embed fn/provider string, JSON-path
  `fields` incl. `array[*]`). Namespace labels validated at every
  entry point *and* pre-batch (`validate_op_namespace`), because
  SQLite/Postgres join labels with `.` — `("foo.bar",)` vs
  `("foo","bar")` collided in the same row (#9217). `put` additionally
  rejects empty namespaces and the `"langgraph"` root label.
- Streaming (`types.py: StreamMode`): `values | updates |
  messages | checkpoints | tasks | debug | custom`, v1/v2/v3
  envelopes; a regression silently dropped `interrupt_before/after`
  and `control` on v1/v2 (#9219) — fail-open observability loss.
- Cooperative shutdown: `RunControl.request_drain()` → node raises
  `GraphDrained(reason="shutdown")` at a superstep boundary;
  checkpoint saved, run resumable.
- Serialization hygiene (#9251): objects that cannot be rebuilt on
  load keep their serialized payload (instead of silently becoming
  `None`) with a one-time warning naming the exception *type* only —
  messages may echo stored values.
- DeltaChannel (beta, dominant recent churn): append-only per-channel
  writes + periodic `_DeltaSnapshot` blobs; reconstruction walks the
  parent chain. Every retention op carries a warning: naive
  `keep_latest` prune / run deletion / partial thread copy silently
  yields empty channels. Lesson, not a feature to copy.

# Findings for Runlith

- F-01 — Durability vocabulary: sync/async/exit names the exact
  persistence-timing trade-off agentd's journal + checkpointing must
  decide per operation class.
- F-02 — Interrupt-as-exception with schema-validated resume:
  pause → typed human answer → re-execute node; invalid answers save
  nothing and stay answerable. Closest external shape to Runlith's
  approval gate, minus any policy layer (the human *is* the policy).
- F-03 — Saver contract + conformance suites: persistence backends
  behind one abstract API, verified by shared capability tests —
  the pattern Runlith's contract suites already follow.
- F-04 — Namespace-injection class: delimiter-joined namespaces need
  validation at *every* entry point including batched/direct-op
  paths, or distinct names address the same row.
- F-05 — Cooperative drain: shutdown as a first-class run outcome
  (reason string, checkpoint saved, resumable) rather than a signal
  handler bolted on.
- F-06 — Retention vs reconstruction: any append-only history with
  periodic snapshots must constrain prune/copy/delete, or retention
  silently corrupts reads. Directly relevant to journal retention
  design.
- F-07 — Serializer failure posture: keep the payload, warn once,
  never echo stored values into logs.
- F-08 — Stream-mode taxonomy: named review surfaces (values,
  updates, checkpoints, tasks, debug, custom) as a checklist for
  what an overseer can subscribe to.

# Comparison with our project

## What is better in Runlith

- Trust boundary: approvals are exact-bound, single-use, child-subset,
  fail-closed with DENY > REQUIRE_APPROVAL > ALLOW. LangGraph
  interrupts have no policy object at all — any answer the human
  gives is accepted (modulo schema shape); there is no scope,
  expiry, or audit of *what was authorized*, only of what happened.
- Local-first integrity: hash-chained audit + append-only journal are
  the durable substrate; LangGraph persistence is a resume mechanism
  with no tamper evidence and a beta channel type whose retention
  ops corrupt silently.
- Execution isolation: worktree admission + runner boundary vs
  LangGraph's in-process nodes with cooperative cancellation only
  (asyncio timeouts do not preempt blocking/CPU-bound code — stated
  in `TimeoutPolicy` docs).
- No cloud control plane: agentd negotiates locally; LangGraph's
  deploy path (CLI → listeners → K8s/external registry, LangSmith)
  is fleet machinery Runlith deliberately defers.

## What is worse in Runlith (gaps this exposes)

- No resume-from-checkpoint UX: Runlith can re-run, but has no
  named equivalent of thread/checkpoint/fork/update_state for a
  human to inspect, edit, and resume a paused run (P3, R-02).
- No per-node execution policies: retry/backoff, run vs idle
  timeouts with heartbeat refresh, and node error-handlers are
  unowned concepts (P3, R-08 INSPIRED_ONLY).
- No cooperative drain semantics: shutdown outcome
  (reason/checkpoint/resumable) is unspecified (P2, R-03).
- Persistence timing is one-mode: no named durability levels
  distinguishing must-survive-crash from may-recompute (P2, R-01).

## What is better in the source

- Checkpoint model is precise and battle-tested: versions_seen
  scheduling, ordered pending writes, namespaced forks, metadata
  sources — the vocabulary Runlith's lifecycle docs could borrow
  without importing semantics.
- Invalid-resume handling is careful: nothing saved, answer again,
  error routed to the human not the model — a UX detail worth
  stealing for approval re-prompts.
- Conformance suites per backend caught real divergence; the
  namespace-collision fix validates at dispatch, not just at API
  surface.
- Failure postures are documented, not implied (serializer keeps
  payload; drain saves checkpoint; prune warnings on beta paths).

## What is worse in the source

- Scale of trusted surface: nodes run arbitrary code with secrets
  in scope; `TracePolicy` explicitly disclaims redaction; memory
  Store ships values + embeddings to configured providers by
  design. No secrets boundary comparable to G6.
- Fail-open observability: the v1/v2 stream regression silently
  dropped interrupts — monitoring loss with no error.
- Beta-at-core: DeltaChannel churn dominates recent history
  (snapshots, seals, fork seals, write-failure ordering); retention
  operations on it are documented foot-guns.
- Cloud-coupled roadmap: SDKs, RemoteGraph, LangSmith deploy —
  the standalone framework increasingly assumes the platform.

# Opportunities and recommendations

- R-01 — Name durability levels for agentd persistence paths
  (must-survive-crash journal vs may-recompute caches), using
  sync/async/exit as vocabulary only. P2, `PARTIALLY_EXISTS` →
  `ADAPT` (docs/requirements; no behavior change).
- R-02 — Design human resume of paused runs (inspect → edit →
  resume-from-checkpoint) behind the existing approval boundary;
  invalid answers save nothing and re-prompt (cf. F-02/`is_invalid_
  resume`). P3, `NOVEL_GAP` → `ADAPT` (needs ADR; Zitieren
  OpenShell R-02 credential-blind delivery for secret handling).
- R-03 — Specify cooperative drain for agentd shutdown: named
  outcome, checkpoint/journal position saved, resumability
  stated. P2, `NOVEL_GAP` → `ADAPT`.
- R-04 — Cite saver-contract + conformance-suite pattern as
  `EXTERNAL_VALIDATION` for Runlith's contract suites and any
  future SQLite-alternative backend. P3.
- R-05 — Cite entry-point + dispatch namespace validation (F-04)
  as `EXTERNAL_VALIDATION` for worktree/slug/identifier validation
  (cf. MegaMemory R-02 slug IDs). P3.
- R-06 — Cite keep-payload + type-only-warning serializer posture
  (F-07) as `EXTERNAL_VALIDATION` for Runlith serde/logging
  hygiene. P3.
- R-07 — Cite stream-mode taxonomy (F-08) as `INSPIRED_ONLY`
  reference for agentctl/audit review surfaces (with Karpathy
  ladder R-02). P3.
- R-08 — Cite per-node retry/timeout/error-handler shape as
  `INSPIRED_ONLY` for future ExecutionPort policy fields. P3.
- REJECT: LangGraph/LangChain as dependency or execution engine;
  DeltaChannel mechanics; Store as memory backend (untrusted
  writes + provider-coupled embeddings — cf. MegaMemory REJECT);
  subgraphs-as-tools topology; LangSmith/CLI-deploy/SDK/JS-twin
  parity; copying checkpoint binary format.

# Evidence and references

- Pin: `main@b4991f1b…` via Atom feed `commits/main.atom`
  (2026-10-10T11:57:46Z; REST API 403 rate-limited, feed used).
- Full pinned raw URLs (all `.../blob/` paths below verified at
  pin unless noted):
  - README:
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/README.md
  - version/deps:
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/libs/langgraph/pyproject.toml
  - durability/interrupt/Command/streaming/policies:
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/libs/langgraph/langgraph/types.py
  - error taxonomy/is_invalid_resume/drain:
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/libs/langgraph/langgraph/errors.py
  - saver contract/metadata/prune warnings:
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/libs/checkpoint/langgraph/checkpoint/base/__init__.py
  - InMemorySaver (debug-only):
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/libs/checkpoint/langgraph/checkpoint/memory/__init__.py
  - store contract/namespace validation/TTL/index:
    https://raw.githubusercontent.com/langchain-ai/langgraph/b4991f1ba36a2449ab59c38ab71828ee865bff84/libs/checkpoint/langgraph/store/base/__init__.py
- Commit evidence (Atom feed bodies): #9258 update_state writes;
  #9257/#9262 delta-history latest; #9260 run-input task ids; #9263
  overwrite snapshot; #9264 fork seals; #9261 conformance gap;
  #9235 failed-save stops later saves; #9248 postgres walk perf;
  #9217 namespace validation; #9251 keep-payload serde; #9232
  ToolNode invalid-resume; #9229 exit-mode NULL_TASK_ID; #9228
  root-lifecycle; #9219 stream kwarg regression; #9222/#9221 CLI
  deploy.
- Not inspected: `pregel/` loop, `channels/`, `graph/`,
  `managed/`, `prebuilt/`, `cli/`, `sdk-py/`, `sdk-js`,
  checkpoint-sqlite/postgres SQL, conformance bodies, tests.
- Runlith references: `AGENT.md` §§4/7; G6 approval/audit notes;
  MegaMemory R-02/R-05; OpenShell R-02; utopia R-02 proposal-gated
  writes; Karpathy study R-02/R-03; G8 verification scope.

# Final assessment

The most transferable content is vocabulary and failure posture,
not mechanism: durability levels (R-01), invalid-resume UX (R-02),
cooperative drain (R-03) as P2/P3 ADAPT items, plus four
EXTERNAL_VALIDATION citations (R-04…R-06 and namespace hygiene)
for designs Runlith already owns. Everything structural —
engine, Store, DeltaChannel, cloud deploy — is REJECT: LangGraph
optimizes for builder velocity on trusted single-tenant workflows,
while Runlith's differentiator stays the trust boundary (approvals,
audit, isolation) around replaceable executors. No IKQ entry: no
evidenced product delta, only requirements-level vocabulary and
validations.
