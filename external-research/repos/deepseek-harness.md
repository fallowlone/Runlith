# deepseek-ai/deepseek-harness

## Source revision

- repository: `deepseek-ai/deepseek-harness`
- repository_url: `https://github.com/deepseek-ai/deepseek-harness`
- discovery_source: `https://habr.com/ru/articles/1079044/`
- default_branch: `master`
- reviewed_revision: `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`
- exact_git_tag: none observed at the reviewed commit
- review_date: `2026-09-15`
- supplemental_runtime_reference: `cordiverse/cordis@f8ea3cd50f1a5724e8e715995bcde131c9c12b2c` (`v4.0.0-rc.10`), used only to understand the plugin/runtime substrate where DeepSeek source depends on Cordis lifecycle semantics
- review_scope: repository structure, agent/session lifecycle, persistence, tool/capability execution, approvals, authorization, sandboxing, network/webhook security, provider retry, subagents/workflows, scheduled work, context retention/spill, architecture invariant machinery, and mandatory Runlith overlap analysis

All repository-specific findings below refer to the frozen DeepSeek Harness revision above. Cordis is supplemental evidence only; no separate adoption conclusions are attributed to Cordis when DeepSeek Harness source already establishes the behavior.

## Executive summary

DeepSeek Harness is one of the strongest external validations reviewed so far for several Runlith architectural choices: durable state is separated from live runtime state, provider/model identity is not the same thing as durable execution identity, side effects are preceded by explicit durability barriers, interrupted executions distinguish “not started” from “outcome unknown”, tool execution is policy-gated, approval fails closed, persistence has one-writer ownership and crash repair, and external content is treated as untrusted context rather than authority.

The most valuable implementation pattern is the **semantic durability barrier immediately before a model or tool side effect**. The session checkpoint policy flushes the already-recorded intent/input and refuses to invoke the model adapter or top-level tool body if the durability checkpoint fails. Crash repair then distinguishes a tool call that was never started from one whose result was never durably observed. For an unknown side-effect outcome the repair text explicitly requires external-state verification or user involvement before a non-idempotent retry. This is unusually close to Runlith's planned G8 Side-Effect Execution Protocol and independently validates `durable intent -> side effect -> reconcile ambiguous outcome`.

The tool runtime also contains several strong least-authority mechanics: parallel execution is opt-in only when a tool returns literal `true` from its concurrency-safety classifier; missing/throwing classifiers become exclusive execution; approval requests fail closed when no approval service or no agent context exists; around-dispatch middleware cannot detach the original caller cancellation signal; and final tool results are normalized/frozen before observer delivery.

The persistence subsystem is more disciplined than a conventional append-to-JSONL design. Session state has a single active writer, a serialized mutation chain, a cross-process kernel lease, torn-tail repair, retryable write-behind retention, explicit flush semantics, and future/unknown format refusal. Storage recovery is deliberately separated from lifecycle recovery: persistence returns a physically valid log, while the agent layer performs semantic repair of interrupted turns.

Security-sensitive implementation is also strong in the reviewed paths. The HTTP fetch backend resolves the complete DNS answer set, refuses non-public destinations, handles IPv4-mapped IPv6 and NAT64 translation, then pins the validated addresses into the actual connection to close the DNS-rebinding window. GitHub webhooks verify a bounded raw UTF-8 body and signature before JSON parsing/dispatch. Local sandbox selection fails closed when no usable backend exists, and the policy layer defaults to read-only confinement. No source-confirmed shell-injection, path-traversal, SSRF, webhook-authentication-bypass, unsafe-deserialization, or sandbox-bypass vulnerability was established in the reviewed revision.

The repository also demonstrates two semantics Runlith should deliberately reject for canonical orchestration. First, provider retry supports an explicit `mode: 'always'` that retries every model-request failure without a retry-count bound until success, cancellation, or disposal. Second, the core agent turn can continue model/tool steps without a configured maximum step count. The workflow PTC subsystem is better bounded (`maxTotalAgents`, item caps and concurrency caps), but its workflow run itself is holder/process-owned and has no durable workflow-resume contract. These choices are acceptable only for explicitly ephemeral/convenience execution; they conflict with Runlith's `INV-091` and durable Task/Execution/Attempt ownership if promoted to canonical orchestration semantics.

Most high-value ideas are therefore `EXTERNAL_VALIDATION`, not new scope. One real Runlith implementation gap remains after overlap search: DeepSeek Harness turns architecture contracts into executable repository checks and source-derived catalogs (dependency graph, persistence vocabulary/schema, per-package invariant companions, stale-generated-artifact checks). Runlith already declares architecture fitness checks as `INV-081` and a P1 architecture target, but this checkout contains no corresponding architecture-fitness checker or canonical task. Classification: `PARTIALLY_EXISTS`; decision: `ADAPT`; candidate work is limited to implementing the already-approved invariant, without copying DeepSeek's package granularity or TypeScript machinery.

## Project purpose

DeepSeek Harness is a modular TypeScript AI-agent runtime built around capability/service seams and composable plugins. The reviewed repository includes:

- agent and session lifecycles;
- provider/model routing and retry policy;
- tool execution and approval policy;
- persistence and session migrations;
- sandboxing and subprocess execution;
- continuable and one-shot subagents;
- scriptable workflows;
- background jobs and durable schedules;
- web acquisition and webhook ingress;
- context/session references, compaction and oversized-output spill;
- client/API packages;
- source-derived architecture, persistence and dependency catalogs;
- extensive repository invariant tests.

The system uses a very fine-grained package/capability architecture. A common shape is:

```text
capability/service definition
    -> implementation provider(s)
    -> policy plugin(s)
    -> model-facing tool adapter
    -> UI/client projection
    -> invariant/contract tests
```

This is useful evidence for Runlith's Ports & Adapters and provider-neutral capability design. The package granularity itself is not an adoption target; it has non-trivial dependency, configuration and lifecycle complexity.

## Architecture map

```text
Repository: deepseek-ai/deepseek-harness
├── Interfaces
│   ├── CLI/client/API packages
│   ├── session controllers and UI projections
│   ├── webhook ingress
│   └── model-facing tools
├── Orchestration
│   ├── core agent loop
│   ├── continuable subagent manager
│   ├── one-shot subagent runs
│   ├── PTC workflow engine
│   ├── schedules
│   └── process-local background jobs
├── Domain
│   ├── Session / turn / step / message events
│   ├── tool execution lifecycle
│   ├── approval decisions
│   ├── schedule records
│   └── subagent descriptors / lineage
├── State/Persistence
│   ├── append-only Session event log
│   ├── JSONL persistence generations + migrations
│   ├── projections / projection caches
│   ├── spill artifacts
│   └── process-local job/activation registries
├── Providers
│   ├── LLM provider seams/adapters
│   ├── provider-owned retry policy
│   ├── subagent providers
│   └── replaceable capability services
├── Tools/Capabilities
│   ├── tools/pre-execute authorization/approval
│   ├── tools/execute around-dispatch middleware
│   ├── tools/post-execute normalization/policy
│   └── tools/result observation/audit hooks
├── Execution
│   ├── local subprocess service
│   ├── sandbox-policy
│   ├── sandbox-local backends
│   └── confined PTC workflow execution
├── Recovery
│   ├── torn-tail physical recovery
│   ├── semantic interrupted-turn repair
│   ├── cold-resume continuable subagents
│   ├── retry projections
│   └── durable schedule replay
├── Security
│   ├── one-shot approval service
│   ├── credential authorization seam
│   ├── filesystem sandboxing
│   ├── SSRF/DNS-rebinding controls
│   └── signed/bounded webhook ingress
└── Observability
    ├── session events/projections
    ├── telemetry / OTEL packages
    ├── tool-result observers
    ├── workflow lifecycle events
    └── generated persistence/dependency catalogs
```

### Architecture questions

| Question | Reviewed conclusion |
| --- | --- |
| Where is business logic? | Distributed among capability/domain packages. Session/agent-loop owns turn semantics; tools owns tool lifecycle; schedule owns scheduled-reminder semantics; subagent owns delegation lineage/continuation. |
| Who owns lifecycle? | The agent loop owns agent turn/step lifecycle; capability-specific runtimes own their local lifecycles. Cordis provides plugin/service lifecycle underneath. |
| Where is canonical state? | For agent/session behavior, the append-only Session event log is canonical. Projections are derived. Other capabilities may have dedicated stores or deliberate process-local state. |
| How are jobs/tasks represented? | `jobs-local` is an in-memory background-job registry, not a durable product Task engine. Continuable subagents use durable child sessions. Workflow runs are live holder-owned runs. |
| Explicit state machine? | Turn/step/session behavior is explicit through typed events and phase/state checks, though it is not one centralized product state machine. |
| Scheduler? | `schedule` folds durable session schedule events into a disposable timer runtime. |
| Who performs recovery? | Persistence repairs physical storage; agent/session layer performs semantic interrupted-turn repair; capability-specific runtimes reconstruct derived state. |
| Retries? | LLM retry is provider-policy driven through `agent/request-error`; normal mode is bounded, `always` mode is unbounded. |
| Crash/restart? | Persisted sessions are reopened, physically repaired, semantically closed for interrupted turns, then resumed. Continuable children can cold-resume from durable session descriptors. Process-local jobs/workflow script state do not provide equivalent durable resume semantics. |
| Concurrency? | Tool calls are exclusive by default; literal opt-in marks safe parallel calls. Persistence uses single-writer ownership and serialized per-handle mutation. Workflows and jobs have explicit concurrency caps. |
| Atomic boundaries? | Session append acceptance is synchronous/immutable; durability is explicit through flush. Persistence append batches advance state only after successful durable write. Capability transactions are not one global atomic boundary. |
| Duplicate mutable truth? | Projections are explicitly derived rather than canonical. Process-local registries coexist with durable logs but are documented as live state. |
| Versioned contracts? | Session format and schedule changes are versioned; future/unsupported storage versions fail closed; persistence vocabulary/schema is source-derived. |
| Provider abstraction? | Strong: LLM and subagent provider routes are separate from session identity and higher-level lifecycle. |
| Can provider mutate business state? | Provider output flows through agent/tool/session contracts; reviewed provider paths do not directly own durable product workflow state. |
| Policy/authorization? | Tool pre-execute policy + user approval + sandbox policy; credential authorization is a separate seam. |
| Capabilities? | Capability services and plugins are explicit, frequently split into seam/provider/policy/tool/client packages. |
| Execution receipts/evidence? | Tool/session events and frozen normalized results provide durable/observable execution evidence; they are not a Runlith-style universal receipt authority. |
| Logs/audit/observability? | Session events, retry events, approval asked/decided events, workflow lifecycle events, telemetry plugins and generated catalogs. |
| UI/API separation? | Client/API packages consume services/projections; canonical session semantics are not owned by UI. |

## Canonical state ownership

### Session event log

The canonical conversational/agent state is an append-only event log. `Session.append()` validates relationships, snapshots payloads into lossless JSON, freezes accepted values and rejects non-serializable or structurally invalid events before commit. Model-visible history and other views are projections over that durable vocabulary rather than independent mutable truth.

This is a strong state-ownership design, but Runlith should not copy it as “full event sourcing everywhere.” Runlith explicitly keeps full Event Sourcing outside v1 and uses typed journal evidence plus canonical aggregate stores. The transferable rule is narrower: **accept immutable, validated facts once; make projections disposable; make durability boundaries explicit**.

### Physical persistence vs semantic recovery

The JSONL backend owns physical validity: format decoding/migration, contiguous sequence validation, torn-tail handling and write ownership. The agent/session layer owns semantic validity: after reopening a physically valid log it computes interrupted-turn closure events and resumes from a structurally closed lifecycle.

That separation is especially valuable for Runlith. A storage layer should not invent Task/Attempt semantic recovery simply because it can repair bytes or rows; application services/Reconciler own lifecycle meaning.

### Live/process-local state

Not every runtime registry is durable. `jobs-local` intentionally stores job records in memory. Continuable subagents have durable child Sessions plus at most one live Activation; the durable descriptor/session is the resume source, while Activation is process-local. PTC workflow runs are holder-owned live executions. These distinctions are mostly explicit in source and reinforce the rule that live execution handles must not become canonical workflow truth.

## Strong engineering decisions

### DSH-F01 — Immutable session acceptance with derived projections

**finding_id:** `DSH-F01`

**source:** implementation

**repository:** `deepseek-ai/deepseek-harness`

**revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**file:** `packages/core/session/src/index.ts`

**symbol:** `Session.append`

**region:** approximately `680-765`

**observation:** Session events are validated, losslessly snapshotted and frozen before append. Reentrant append is refused. Projections/model history are derived, and observer failure after commit cannot retroactively undo the accepted fact.

**confidence:** `HIGH`

**Problem:** mutable aliases and independently writable projections create duplicate truth and make replay/recovery unreliable.

**Mechanism:** one validated append boundary; immutable accepted values; derived views.

**Why it works:** once accepted, a fact has stable identity/content and consumers can rebuild projections without granting them write authority over canonical history.

**Limitations:** the event-sourced session model produces substantial schema/migration/event-lifecycle complexity and should not be generalized automatically to every Runlith bounded context.

**Failure modes:** schema evolution or incorrectly declared ignorable events can still break reconstruction if compatibility rules are wrong.

**Complexity:** high repository-wide discipline; mitigated by generated persistence catalogs and invariant tests.

**Runlith overlap:** `ALREADY_DESIGNED` / partly `ALREADY_IMPLEMENTED`. Runlith already separates canonical state from derived events/projections and requires versioned typed journal facts. Full event sourcing remains intentionally outside v1.

**Decision:** `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` for the exact session implementation.

### DSH-F02 — Durability barrier immediately before model/tool side effects

**finding_id:** `DSH-F02`

**source:** implementation

**file:** `packages/session/session-checkpoint-policy/src/index.ts`

**symbol:** checkpoint listeners around model dispatch, top-level tool execution and request/step boundaries

**observation:** the policy requires the current session prefix to flush before a model adapter or top-level tool body is invoked. A flush failure blocks the downstream side effect. Nested tool dispatch reuses the durable outer-call boundary rather than inventing another ambiguous commit model.

**confidence:** `HIGH`

**Problem:** if intent/input exists only in memory when an external side effect starts, a crash can leave the system unable to determine what it attempted.

**Mechanism:** record intent/input -> semantic flush barrier -> invoke side effect only after durable acknowledgement.

**Why it works:** post-crash recovery has a durable “before” boundary from which it can reason about whether an effect was attempted and what reconciliation is needed.

**Limitations:** durability-before-effect does not itself prove whether the external effect completed. The system still needs an `AMBIGUOUS` state/reconciliation rule.

**Failure modes:** an external effect may complete while its result fails to persist; the next layer must not blindly retry non-idempotent work.

**Complexity:** moderate and cross-cutting; centralizing the barrier reduces scattered ad-hoc flush calls.

**Runlith overlap:** `ALREADY_DESIGNED`. G8 already calls for durable intent, `AMBIGUOUS` recovery and `RECONCILE` before retry. `INV-066` forbids transparent replay of stateful side effects.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F03 — Physical recovery and semantic interrupted-turn repair are separate

**finding_id:** `DSH-F03`

**source:** implementation

**files / symbols / regions:**

- `packages/core/agent-loop/src/index.ts` — `resumeWith`, approximately `857-930`.
- `packages/core/session/src/repair.ts` — `interruptedTurnClosers`, complete module.

**observation:** resume obtains a write persistence handle, loads a physically valid log, then the agent layer appends synthetic lifecycle closers for an interrupted turn. Repair distinguishes `TOOL_NOT_STARTED` from `TOOL_OUTCOME_UNKNOWN`. For unknown outcomes the generated diagnostic says to retry only when read-only/idempotent or after external-state verification/user involvement, and explicitly says not to retry side effects blindly.

**confidence:** `HIGH`

**Problem:** a crash can leave structurally incomplete lifecycle state and ambiguous external effects.

**Mechanism:** storage repairs bytes/records; domain/runtime repairs lifecycle semantics; ambiguous side effects remain ambiguous rather than being normalized into “failed, retry.”

**Why it works:** ownership matches knowledge. Storage can prove physical integrity; the agent lifecycle understands turn/tool semantics.

**Limitations:** the repair event is guidance/evidence; correctness still depends on downstream recovery respecting idempotency and reconciliation.

**Failure modes:** any future recovery layer that treats `TOOL_OUTCOME_UNKNOWN` as an ordinary retryable failure would undo this safety property.

**Complexity:** moderate; requires typed lifecycle events and stable repair semantics.

**Runlith overlap:** `ALREADY_DESIGNED` and partially implemented in existing execution/workspace recovery. Runlith already treats ambiguous external outcome separately from retryable failure.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F04 — Conservative tool concurrency plus non-detachable cancellation

**finding_id:** `DSH-F04`

**source:** implementation

**file:** `packages/core/tools/src/index.ts`

**symbols / regions:** execution-mode classification around `1275-1290`; execute pipeline around `1334+`; around-dispatch signal fusion around `1537-1568`; approval bridge around `1698-1736`.

**observation:** tool execution is parallel-safe only when `isConcurrencySafe(args)` returns literal `true`. Missing classifier, false result or classifier exception yields exclusive execution. Around-dispatch middleware may replace the working cancellation signal, but the runtime fuses the original caller signal back in so middleware cannot detach cancellation. Approval absence or missing agent context becomes deny.

**confidence:** `HIGH`

**Problem:** optimistic concurrency and replaceable middleware can accidentally widen authority or detach lifecycle control.

**Mechanism:** least-trust concurrency default; immutable call identity; cancellation fusion; fail-closed approval fallback.

**Why it works:** safety-sensitive properties require explicit opt-in and cannot be silently removed by a wrapper.

**Limitations:** classifier correctness remains tool-owned. Literal `true` is only as trustworthy as the tool implementation and its argument-sensitive classification.

**Failure modes:** a buggy tool can falsely claim parallel safety; shared external resources can still race outside process-local classification.

**Complexity:** low-to-moderate and highly reusable.

**Runlith overlap:** provider/tool authority boundaries, Policy and governed Runner execution are `ALREADY_DESIGNED`; exact per-tool parallel-safety classification is future scheduling/tool-registry detail.

**Decision:** `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` for the literal API.

### DSH-F05 — One-shot approval decisions are fail-closed and durably paired

**finding_id:** `DSH-F05`

**source:** implementation

**file:** `packages/interaction/user-approval/src/index.ts`

**symbols / regions:** policy/open-turn checks around `50-98`; `ApprovalService.request` around `190-227`; decision normalization around `260-299`.

**observation:** approval requests must occur inside an open durable turn. The service appends `approval/asked`, obtains a one-shot decision and appends `approval/decided`; it refuses to return an unlogged decision. Missing answerer, answerer error, invalid/rogue result and cancellation normalize to unavailable/cancelled rather than allow. Tool approval with no service or no agent context becomes deny.

**confidence:** `HIGH`

**Problem:** approval that exists only as transient UI state is replayable/ambiguous after restart and can fail open when the UI/service disappears.

**Mechanism:** exact one-shot request/decision pair in durable session history plus conservative unavailable behavior.

**Why it works:** approval evidence and the operation lifecycle share an auditable boundary.

**Limitations:** Runlith requires a stronger product-level binding: exact operation/request, capabilities, policy fingerprint, provenance and validity window.

**Failure modes:** generic “session policy” must never substitute for Runlith's exact operation-bound decision where authority is privileged.

**Complexity:** moderate; most complexity is cancellation and lifecycle race handling.

**Runlith overlap:** `ALREADY_IMPLEMENTED`/designed through G6 approval enforcement and `INV-150`.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F06 — Single-writer persistence with kernel lease and retryable failed drain

**finding_id:** `DSH-F06`

**source:** implementation

**files:**

- `packages/session/session-persistence-jsonl/src/storage.ts`
- `packages/session/session-persistence-jsonl/src/lease.ts`
- `packages/session/session-persistence-jsonl/src/index.ts`

**observation:** each live write handle serializes mutation on one promise chain. The backend allows one writer per session and uses a cross-process kernel-held lease (POSIX flock / Windows semaphore). Crash releases the kernel lock. Buffered event batches are retained after write failure so a later explicit flush retries loudly instead of silently dropping them. Torn physical tails are repaired before new append. POSIX lease acquisition verifies the locked inode/path and retries boundedly when unlink/recreate races are detected.

**confidence:** `HIGH`

**Problem:** append-only files become corrupt or forked when multiple writers, partial writes and crash recovery are not one coherent ownership model.

**Mechanism:** process-local claim + kernel lease + contiguous sequence validation + durable append/truncation rules + retryable retained buffer.

**Why it works:** correctness is anchored in durable/OS ownership rather than an in-memory boolean.

**Limitations:** a live wedged writer intentionally retains its kernel lease; liveness/recovery then depends on process supervision rather than lease expiry.

**Failure modes:** this is local-machine ownership, not a distributed consensus protocol.

**Complexity:** high for a JSONL backend, but much of it buys explicit crash semantics.

**Runlith overlap:** `EXTERNAL_VALIDATION`. Runlith already separates process liveness from durable ownership and has SQLite/Git reconciliation plus ownership fencing in workspace/execution flows.

### DSH-F07 — Sandboxing is a capability service with fail-closed absence

**finding_id:** `DSH-F07`

**source:** implementation

**files:**

- `packages/sandbox/sandbox-policy/src/index.ts`
- `packages/sandbox/sandbox-local/src/index.ts`

**regions:** sandbox policy config/service around `64-179`; local provider construction/confinement around `250-345`; backend selection around `488-525`.

**observation:** the policy service defaults to read-only, resolves explicit/session/deployment policy and keeps enforcement in provider/executor code. The local provider canonicalizes workspace roots and refuses confinement when the platform chain has no usable runner. Linux can arbitrate bwrap/Landlock through functional probes; platform chains select Seatbelt on macOS and Windows ACL on Windows. A sole platform candidate is selected without a preflight probe, relying on execution-time failure detection to fail closed. Custom `runnerCommand` is explicitly an operator assertion and is not functionally probed.

**confidence:** `HIGH`

**Problem:** sandbox policy is meaningless if it is only metadata or silently falls back to host execution.

**Mechanism:** typed policy -> concrete backend -> wrapped argv + enforcement metadata; absence of a backend is an error.

**Why it works:** mandatory confinement is coupled to the spawn path rather than advisory prompt language.

**Limitations:** custom runner correctness is operator-trusted. Sole-candidate platforms detect unusability at execution rather than during selection. Windows ACL explicitly has partial-enforcement boundaries.

**Failure modes:** a wrongly configured custom runner that exits successfully without enforcing the asserted restrictions could violate the operator's expectation because the system does not semantically probe custom confinement.

**Complexity:** high and OS-specific.

**Runlith overlap:** `ALREADY_IMPLEMENTED`/active hardening in G6 for governed Runner/Seatbelt and fail-closed isolation. DeepSeek is external validation, not a reason to change Runlith's existing runner boundary.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F08 — DNS-rebinding-resistant web acquisition

**finding_id:** `DSH-F08`

**source:** implementation

**files:**

- `packages/web/web-fetch-http/src/policy.ts`
- `packages/web/web-fetch-http/src/network.ts`

**symbols:** `validateFetchUrl`, `resolvePublicAddresses`, `requestPinned`, `createPinnedLookup`

**observation:** fetch accepts only HTTP(S), refuses embedded URL credentials and cross-origin redirects, resolves a destination once, rejects the full answer set when any target is not globally reachable unicast, checks IPv4-mapped IPv6 and discovered NAT64 translation, and supplies only validated addresses to a per-request Undici lookup. The hostname remains intact for Host/TLS while DNS cannot silently re-resolve to a private address after validation.

**confidence:** `HIGH`

**Problem:** hostname validation followed by a second DNS resolution leaves a TOCTOU window for SSRF/DNS rebinding.

**Mechanism:** validate resolved addresses and bind that validated set to the actual connection.

**Why it works:** the network destination used by the socket is the same destination class evaluated by policy.

**Limitations:** proxy routing shifts DNS resolution to the proxy and therefore needs separate proxy trust/policy semantics.

**Failure modes:** future redirect/proxy code must preserve per-hop evaluation; otherwise the invariant can be bypassed outside this provider.

**Complexity:** moderate; NAT64 handling shows why “block RFC1918 strings” is insufficient.

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith's `NetworkAccessPolicy`, `INV-097` and source-of-truth explicitly require resolved-IP and every-redirect SSRF evaluation and state that URL validation alone is insufficient when DNS can later reach a forbidden target.

**Decision:** `EXTERNAL_VALIDATION`; the address-pinning implementation is useful future G10 engineering knowledge, not a new architecture task.

### DSH-F09 — Oversized context retains full evidence outside the prompt

**finding_id:** `DSH-F09`

**source:** implementation

**files:**

- `packages/spill/spill/src/index.ts`
- `packages/spill/spill-policy/src/index.ts`
- `packages/context/session-reference/src/spill.ts`

**observation:** spill storage persists the full text and returns an opaque locator, byte count and retrieval guidance. The spill policy replaces oversized model-facing plain text with a bounded preview + locator but preserves the original result inline if storage fails. Session-reference spill uses the same captured projection for preview/full snapshot and marks the referenced transcript as untrusted background that cannot grant instruction/permission/tool authority. When a full snapshot cannot be saved, the notice says it is unavailable rather than pretending retrieval exists.

**confidence:** `HIGH`

**Problem:** large outputs either explode context cost or are destructively truncated, losing recoverable evidence and provenance.

**Mechanism:** full external artifact + bounded preview + explicit omission metadata + locator, with trust labeling.

**Why it works:** context budget and evidence retention become separate concerns.

**Limitations:** artifact retention, access control, cleanup and sensitivity policy still matter; a locator itself is not authorization.

**Failure modes:** storage failure intentionally falls back to full inline output, so context caps are best-effort in this policy rather than a hard execution ceiling.

**Complexity:** moderate.

**Runlith overlap:** `ALREADY_DESIGNED`. Source of Truth already incorporates the large-tool-result-to-artifact pattern, G9 token/byte budgets, explicit provenance and `INV-071` protocol-valid compaction.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F10 — Durable schedules are replayed into disposable timers

**finding_id:** `DSH-F10`

**source:** implementation

**files:**

- `packages/schedule/schedule/src/domain.ts`
- `packages/schedule/schedule/src/persistence.ts`
- `packages/schedule/schedule/src/runtime.ts`

**observation:** schedule changes are versioned durable session events. The runtime is explicitly a disposable projection: it folds the durable schedule log, performs a persistence preflight before dispatch, injects the due reminder through agent maintenance, records dispatch, and flushes again before continuing.

**confidence:** `HIGH`

**Problem:** timers alone disappear on restart and cannot prove whether a scheduled occurrence was accepted/dispatched.

**Mechanism:** durable schedule state + reconstructed timer projection + explicit durability barriers around dispatch.

**Why it works:** wall-clock timers are replaceable runtime machinery, not state authority.

**Limitations:** exact delivery semantics and duplicate/missed occurrence behavior remain tied to this local session design.

**Runlith overlap:** `ALREADY_DESIGNED` in G11 Routine/Pulse Engine.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F11 — Continuable subagent identity survives provider/runtime changes

**finding_id:** `DSH-F11`

**source:** implementation

**files:**

- `packages/subagent/subagent/src/depth.ts`
- `packages/subagent/subagent/src/continuation.ts`

**observation:** continuable children have one durable Session and at most one process-local Activation. Child descriptors and delegation depth are persisted; runtime depth may deepen but cannot lower persisted depth after resume. A missing live child can cold-resume from persisted session state and descriptor after lineage authorization. Activation is deliberately process-local.

**confidence:** `HIGH`

**Problem:** restart can accidentally reset delegation limits or make a live handle the only source of child identity/configuration.

**Mechanism:** durable child session/descriptor + derived process activation + monotone persisted recursion depth + cold resume.

**Why it works:** runtime residency is replaceable and cannot erase durable delegation lineage.

**Limitations:** this remains session-centric orchestration rather than a Task/Execution/Attempt hierarchy; Runlith should keep subagent/provider sessions below durable work ownership.

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith already requires application-owned execution identity and replaceable provider sessions.

**Decision:** `EXTERNAL_VALIDATION`.

### DSH-F12 — Architecture contracts are executable and source-derived

**finding_id:** `DSH-F12`

**source:** implementation/tooling

**files / symbols:**

- `scripts/test-invariants.ts` — per-package invariant companion host.
- `scripts/verify-package-dependencies.ts` — source/manifests dependency-policy verification.
- `scripts/gen-module-graph.ts` — deterministic package dependency graph with `--check` stale-artifact failure.
- `scripts/gen-persistence-catalog.ts` — extracts typed durable-event vocabulary/schema, generates docs/runtime-known-event set/schema, and fails if committed artifacts are stale.
- multiple `packages/*/*/src/invariant.ts` companions.

**observation:** architecture and persistence documentation is partly generated from code, then verified for freshness. Package-specific runtime invariants are loaded automatically into ordinary tests, while a topology test covers all invariant companions. Dependency policy is checked against actual source imports/exports and manifests rather than maintained solely as prose.

**confidence:** `HIGH`

**Problem:** architectural rules and hand-maintained diagrams drift from source as package count and extension surfaces grow.

**Mechanism:** executable architecture rules + deterministic code-derived catalogs + checked-in generated artifacts + CI/test freshness gates.

**Why it works:** violations fail where code changes, and architecture maps/persistence vocabulary can be regenerated rather than manually reconciled.

**Limitations:** substantial tooling complexity; overfitting checkers to current package conventions can make refactors costly. Generated documentation is only as meaningful as the invariants encoded by the generator.

**Failure modes:** a large custom verifier suite can become another platform that requires maintenance and can create false confidence if semantic rules are omitted.

**Complexity:** medium-to-high, but scales better than manual architecture review for a large modular repository.

**Runlith overlap:** `PARTIALLY_EXISTS`. `INV-081` and Source of Truth explicitly require architecture fitness checks and propose `cargo metadata`/custom CI dependency checks, but no implemented checker/script/CI gate or canonical task was found in this checkout.

**Decision:** `ADAPT`.

## Implementation techniques

### Central semantic flush API

`SessionStore.flush()` is the single semantic durability entry point. It waits for all persistence listeners, uses all-settled behavior so one failure does not prevent the others from finishing, and still surfaces failure to the caller. This is preferable to scattered “best effort save” calls whose partial success cannot be reasoned about.

### Frozen execution/result observations

Tool execution identity and normalized final result are materialized before observation. Post-result observer failures are contained rather than mutating the already-established tool outcome. Runlith can reuse the principle for receipts/audit: observability must not silently become completion or authorization authority.

### Conservative defaults

Several security/reliability classifiers default toward less authority:

- tool parallel safety requires literal opt-in;
- missing approval support becomes deny;
- sandbox policy defaults read-only;
- unsupported/future durable formats refuse rather than reinterpret;
- unknown full referenced-session snapshot state is reported unavailable;
- unknown side-effect outcome remains unknown.

### Stable source-generated contracts

The repository derives persistence vocabulary, known event names, schemas and module dependency documentation from source and verifies committed artifacts with `--check`. This is a concrete version of “architecture as executable constraints,” not merely generated diagrams.

## AI / orchestration architecture

### Agent loop

The agent loop owns turn and step boundaries. It appends `turn/start`, repeatedly opens steps, prepares/render context, dispatches the model stream, commits assistant attempt/message state, executes tool calls and appends `step/end` / `turn/end`. If the model returns no tool calls, the turn completes; otherwise tool execution can produce another step.

Completion here means completion of the **agent turn**, not independent evidence that a software-engineering Task is correct. This distinction matters for Runlith: a model deciding to stop must never be the same thing as Task completion authority.

### Provider/model requests

Provider/model selection belongs to LLM routing/configuration, while session identity survives configuration changes. Request failures are normalized and passed through an `agent/request-error` recovery waterfall. The optional retry plugin owns retry scheduling and emits durable retry events/projection state.

### Tool calls

The tool pipeline is layered:

```text
validated tool call
  -> pre-execute policy / deny / ask
  -> execute around-dispatch wrappers
  -> tool body
  -> post-execute normalization / policy
  -> frozen result observers
```

The model can propose a tool call, but privileged behavior still crosses tool policy/approval and the configured execution/sandbox boundary. The model does not receive a direct canonical-state mutation primitive merely by emitting text.

### Subagents

There are two important classes:

- one-shot runs can be surfaced as background jobs and settle into process-local job records;
- continuable children have durable session identity/descriptor and process-local Activations that can be rebuilt through cold resume.

Delegation depth is persisted and monotone across resume, which prevents a resumed child from becoming a new top-level agent by losing its depth budget.

### Workflows

The workflow seam executes orchestration scripts through a confined PTC runtime. The implementation has useful local bounds: default `maxTotalAgents = 1000`, `maxItemsPerCall = 4096`, a concurrency ceiling and a synchronous VM timeout. Fatal host/schema/cap failures propagate; ordinary child failures can become per-item nulls according to combinator semantics.

The workflow run itself is holder-owned live state. Lifecycle events are explicitly observe-only and no durable workflow resume contract was found. This makes it unsuitable as a Runlith canonical Task Graph model even though it is useful as a bounded ephemeral orchestration facility.

### Long-running execution/restart

Session and continuable-subagent recovery is strong. Process-local jobs and holder-owned workflow scripts have weaker restart semantics. The repository therefore has multiple levels of durability rather than one uniform “agent workflow is durable” guarantee.

## Security analysis

### Security finding classification

| Area | Classification | Evidence / conclusion |
| --- | --- | --- |
| Tool approval failure behavior | `CONFIRMED` defensive control | Missing approval service/agent and unavailable/throwing answerers become denial/unavailable; no implicit allow. |
| Sandbox absence/failure | `CONFIRMED` defensive control | Platform chain with no usable backend raises `SANDBOX_UNAVAILABLE`; no trusted-host fallback in the reviewed confinement provider. |
| SSRF / DNS rebinding | `CONFIRMED` defensive control | public-address validation plus connection-time address pinning; IPv4-mapped IPv6/NAT64 considered. |
| GitHub webhook trust | `CONFIRMED` defensive control | bounded raw body, required unique headers, secret resolution, signature verification before parse/dispatch. |
| Unsafe JSON/deserialization on reviewed ingress | `NOT_SUPPORTED` as a finding | reviewed webhook/session paths validate/snapshot JSON; no unsafe object-instantiation path was established. |
| Shell/argument injection in reviewed sandbox wrapper | `NOT_SUPPORTED` as a confirmed vulnerability | confinement builds argv arrays; no source-confirmed interpolation vulnerability established in reviewed paths. |
| Path traversal/symlink escape | `NOT_SUPPORTED` as a confirmed vulnerability | no destructive proof was attempted and reviewed sandbox/web paths did not establish a bypass. |
| Ambient credential exposure | `NOT_SUPPORTED` as a confirmed vulnerability | credential and sandbox packages were reviewed selectively; no exact leakage path was established. |
| Custom sandbox runner trust | `PLAUSIBLE` deployment weakness | configured `runnerCommand` is explicitly “operator's assertion, no probe” and is reported as full enforcement; an incorrectly implemented runner that exits successfully could under-enforce policy. This is trusted-configuration risk, not an untrusted remote bypass demonstrated here. |
| Sole-candidate sandbox preflight | `PLAUSIBLE` availability/reliability weakness | a sole backend (e.g. platform chain with one candidate) skips preflight and relies on execution-time refusal; source says execution-time failure still fails closed. No authority bypass established. |

### Web acquisition

The reviewed HTTP backend is an unusually good reference implementation for the network TOCTOU portion of SSRF policy. The key property is not the specific `ipaddr.js` code. It is the invariant:

```text
policy validates address set
        ↓
connection can use only that validated set
```

Runlith already states the architecture requirement. Future implementation should retain this connection-binding property across redirects, proxies, IPv6/NAT64 and remote workers.

### Webhooks

GitHub webhook ingress verifies the exact raw body used for signature validation, applies byte and UTF-8 bounds before parse, rejects ambiguous/missing headers and only dispatches a typed verified delivery. This is good evidence for treating webhook events as authenticated **external evidence** rather than direct domain truth.

### Approval and authorization

Approval is explicit and auditable. Credential authorization additionally uses one in-flight authorization attempt per credential key and verifies that a credential-record update was actually observed before treating an authorization flow as committed. These are useful anti-TOCTOU patterns, though Runlith's Security/Policy model remains stronger and product-wide.

### Sandbox boundary

The local provider supports bwrap/Landlock/Seatbelt/Windows ACL with platform-specific enforcement metadata. Windows ACL is explicitly marked partial because of documented filesystem limitations. This honesty is preferable to claiming one uniform “sandbox=true” capability across OSes.

The main caution is custom runner configuration. Because it is intentionally trusted and unprobed, Runlith should keep its stricter rule that a backend may advertise only restrictions it can concretely prove/support; an operator override must not silently turn an unverified wrapper into a security attestation.

## Reliability / recovery analysis

### Interrupted tool outcomes

`TOOL_NOT_STARTED` and `TOOL_OUTCOME_UNKNOWN` are materially different failure classes. This is one of the most useful findings in the repository because it prevents a transport/persistence ambiguity from collapsing into a generic retry.

For Runlith the direct mapping is:

```text
durable intent, no launch evidence
  -> safely decide whether first execution is still needed

launch/dispatch evidence, no durable outcome
  -> AMBIGUOUS
  -> reconcile external state
  -> retry only after idempotency/reconciliation permits it
```

### Persistence ownership

The JSONL backend's single-writer + kernel lease model avoids relying on process-local exclusion for correctness. A crash releases the kernel ownership automatically; a live wedged process intentionally does not. This is a clear separation of crash recovery from live-owner liveness policy.

### Scheduled work

Schedule timers are disposable. Durable schedule events and persistence barriers own replay. This validates Runlith G11's choice that routines/runs are durable workflow state and timer/process state is derived runtime machinery.

### Continuable children

Cold resume reconstructs a child from persisted descriptor/session state and re-authorizes lineage. Runtime options cannot lower persisted delegation depth. This is a strong restart semantic for delegated agent sessions.

### Process-local jobs/workflows

`jobs-local` is explicit process memory. PTC workflow run control is holder-owned and no durable resume state machine was found. These facilities should therefore remain convenience/runtime constructs. Promoting them to canonical long-running workflow state would create a restart gap.

## Performance / scalability analysis

### Fine-grained package/capability topology

The repository has a very large number of small packages. Benefits are clear ownership, optional composition, narrow provider seams and isolated testing. Costs are also visible: dependency-policy tooling, generated graphs, optional-dependency verification, lifecycle/injection complexity and a large invariant harness are required to keep the topology coherent.

Runlith should keep its existing bounded-context/Ports & Adapters structure without copying package-per-mechanism granularity automatically.

### Tool concurrency

Default-exclusive tool execution is conservative. Parallelism can scale only where a tool can certify argument-specific concurrency safety, and the agent loop additionally bounds parallel-safe calls. This gives predictable correctness at the cost of throughput for tools without classifiers.

### Spill/output retention

Large tool output can be moved out of model context while remaining retrievable. This directly lowers context/token pressure without deleting source evidence. Storage failures preserve correctness by retaining the original output, trading budget guarantees for information preservation.

### Workflow caps

PTC workflows have total-agent, per-combinator-item and concurrency caps. This is a good bounded-execution design inside that subsystem even though the core agent loop and `llm-retry: always` can still run without equivalent global bounds.

### Architecture tooling scalability

The source-derived catalogs and invariant companions appear to be a response to repository scale: manual package/dependency/persistence documentation would be too error-prone. The important transferable technique is deterministic extraction + verification, not the exact TypeScript AST implementation.

## Anti-patterns

### DSH-AP01 — Explicit unbounded provider retry mode

**Anti-pattern:** `retryPolicy.mode = 'always'` retries every model-request failure until success, cancellation or plugin disposal, with no retry-count ceiling.

**Evidence:** `packages/llm/llm/src/retry-policy.ts` (`AlwaysRetryPolicyConfig`, `ResolvedAlwaysRetryPolicy`); `packages/llm/llm-retry/src/index.ts` (`recover` / `backoff`).

**Immediate impact:** persistent provider/auth/configuration failures can consume time, requests and money indefinitely unless some external cancellation/disposal occurs.

**Long-term impact:** operational liveness depends on an external stopper; failures can become invisible “still retrying” states instead of explicit exhausted/escalated outcomes.

**Runlith lesson:** every autonomous retry class needs a finite budget/exhaustion outcome even when backoff delay itself is bounded.

**Existing Runlith protection:** `INV-091`; AGENT.md explicitly forbids unbounded retry/debate/council loops; retry is checkpoint-aware/observable and side effects require idempotency/reconciliation.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE`.

**Decision:** `REJECT`.

### DSH-AP02 — Core agent turn has no maximum step budget

**Anti-pattern:** the core turn loop continues model -> tool -> next model step until the model/tool lifecycle produces a stop condition, cancellation or error; no `maxSteps` guard was found in the agent-loop configuration.

**Evidence:** `packages/core/agent-loop/src/agent.ts` — `kick()` loop around `226-238`, `turn()` `while (true)` around `270-350`, `step()` retry loop around `353-498`; `packages/core/agent-loop/src/index.ts` configuration exposes `maxParallelToolCalls` but no corresponding maximum step count in the reviewed configuration path.

**Immediate impact:** a model can keep producing non-concluding tool calls and consume an unbounded number of steps while still making “valid” progress transitions.

**Long-term impact:** autonomous loops need external cancellation/resource exhaustion instead of producing a deterministic escalation/exhaustion state.

**Runlith lesson:** bounded planning/reaction/recovery limits must exist at lifecycle level, not only per tool or per workflow helper.

**Existing Runlith protection:** `INV-050`, `INV-091`, bounded planning/reaction semantics.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE`.

**Decision:** `REJECT`.

### DSH-AP03 — Holder-owned workflow run is not durable orchestration state

**Anti-pattern:** using the PTC workflow run itself as canonical long-running work would make orchestration dependent on one process-held script/run object with observe-only lifecycle events and no reviewed durable resume state machine.

**Evidence:** `packages/workflow/workflow/src/index.ts` documents a live holder-owned run and observe-only lifecycle events; `packages/workflow/workflow-ptc/src/host.ts` owns run cancellation/result/child handles in process; no workflow persistence/resume seam was found in the reviewed workflow packages.

**Immediate impact:** process loss can discard script control state even if child sessions themselves have stronger persistence.

**Long-term impact:** canonical workflow continuation would become runtime/provider implementation dependent and could require duplicate relaunch or ad-hoc reconstruction.

**Runlith lesson:** scriptable workflow helpers can be useful inside an Attempt, but durable Task Graph/Execution/Attempt/Reconciler state must remain product-owned.

**Existing Runlith protection:** durable Task Graph; `Task != Execution != Attempt != Provider Session`; providers and agent sessions are replaceable executors.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE` if used as canonical workflow truth; otherwise an acceptable ephemeral facility.

**Decision:** `REJECT` for canonical orchestration; `INSPIRED_ONLY` for bounded convenience workflows.

### DSH-AP04 — Custom sandbox runner relies on operator assertion without functional proof

**Anti-pattern:** treating a configured custom wrapper as `full` confinement solely because the operator supplied it, without a functional enforcement probe.

**Evidence:** `packages/sandbox/sandbox-local/src/index.ts:305-326` explicitly describes configured `runnerCommand` as “the operator's assertion, no probe” and returns `enforcement: 'full'` with caller-provided failure signatures.

**Immediate impact:** a misconfigured wrapper can be reported as full enforcement while not enforcing the intended filesystem boundary, provided it does not trigger configured failure signatures.

**Long-term impact:** deployment configuration can become an unverified security attestation.

**Runlith lesson:** operator configuration may select an enforcement backend, but claimed capabilities/restrictions should be proven by backend contract/probe/verification and unsupported mandatory restrictions fail closed.

**Existing Runlith protection:** G6 governed Runner, capability-aware isolation planning and fail-closed unsupported restriction semantics.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE` as a security-attestation model.

**Decision:** `REJECT`.

## Runlith overlap analysis

### External idea: durability barrier before model/tool side effects

**External evidence:** `session-checkpoint-policy` flushes before model adapter and top-level tool execution; failure blocks invocation.

**Closest Runlith equivalent:** G8 Side-Effect Execution Protocol; durable intent; `AMBIGUOUS`; `RECONCILE`; provider Attempt durability.

**Runlith evidence:** `planning/roadmap.yaml` G8; `INV-066`; AGENT.md stateful-retry rules; Event Journal/recovery architecture.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none at architecture level. DeepSeek provides concrete implementation validation for placing the barrier immediately before the effect boundary.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: distinguish not-started from unknown-outcome after crash

**External evidence:** `repair.ts` has separate `TOOL_NOT_STARTED` and `TOOL_OUTCOME_UNKNOWN` repair outcomes.

**Closest Runlith equivalent:** G8 ambiguous side-effect recovery; `INV-066`; no blind replay.

**Runlith evidence:** Source of Truth G8; Event/continuity docs; execution recovery tests.

**Current status:** `ALREADY_DESIGNED` / narrower cases already implemented.

**Actual gap:** none attributable to this repository.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: one-shot fail-closed approval with durable asked/decided pair

**External evidence:** `user-approval` refuses unlogged decisions; missing/unavailable answerer does not allow.

**Closest Runlith equivalent:** G6 HumanDecision/Approval enforcement and exact operation-bound authority.

**Runlith evidence:** `INV-150`; approval service/agentd approval implementation; G6 task history.

**Current status:** `ALREADY_IMPLEMENTED` / `ALREADY_DESIGNED` for broader binding.

**Actual gap:** none. Runlith's target binding is stronger than the external model.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: conservative concurrency opt-in

**External evidence:** only literal `true` from a tool classifier permits parallel mode.

**Closest Runlith equivalent:** Scheduler dependencies, Policy/Runner authority, bounded concurrency principles.

**Runlith evidence:** durable DAG and scheduling architecture; bounded planning/reaction invariants.

**Current status:** `ALREADY_DESIGNED` at principle level.

**Actual gap:** exact tool-registry classifier API is not required yet and should be designed with future Tool/Skill Registry rather than copied.

**Decision:** `INSPIRED_ONLY` / `EXTERNAL_VALIDATION`.

### External idea: physical storage recovery separated from semantic lifecycle recovery

**External evidence:** JSONL layer repairs physical log; agent loop appends semantic interrupted-turn closers.

**Closest Runlith equivalent:** SQLite/Git are separate failure domains; trusted application services/Reconciler own lifecycle; adapters do not invent Task transitions.

**Runlith evidence:** `INV-145`, workspace architecture, execution recovery and app-service boundaries.

**Current status:** `ALREADY_IMPLEMENTED` in narrower current domains and `ALREADY_DESIGNED` generally.

**Actual gap:** none.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: address-pinned SSRF defense

**External evidence:** validate full DNS result then use a custom connection lookup restricted to those validated addresses.

**Closest Runlith equivalent:** centralized `NetworkAccessPolicy` and per-hop resolved-IP/redirect evaluation.

**Runlith evidence:** Source of Truth web acquisition section; `INV-097`, `INV-098`, `INV-099`, `INV-103`, `INV-121`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** implementation is future G10 work; DeepSeek's pinning mechanism is implementation knowledge, not an architectural gap.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: full-output spill plus bounded context locator

**External evidence:** `spill-policy` saves full output and returns preview/locator; session references carry explicit untrusted warnings and omission facts.

**Closest Runlith equivalent:** artifact-backed large results, Context Engine token/byte budgets, provenance-bearing context packs, protocol-valid tool-result compaction.

**Runlith evidence:** Source of Truth Context Engine/efficiency sections; `INV-071` and context provenance rules.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none from this repository.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: durable scheduled state with disposable timer projection

**External evidence:** schedule changes live in session log; timer runtime reconstructs and checkpoints around dispatch.

**Closest Runlith equivalent:** G11 Routine/Pulse Engine, durable RoutineDefinition/RoutineRun.

**Runlith evidence:** Source of Truth G11; bounded autonomous/recovery invariants.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** exact occurrence/delivery contracts remain future G11 design, but this repository does not expose a uniquely missing architecture decision beyond already-known G11 work.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: durable subagent descriptor with process-local activation

**External evidence:** continuable child has one durable Session and at most one live Activation; cold resume reconstructs from persisted descriptor/session.

**Closest Runlith equivalent:** durable Execution/Attempt plus replaceable provider session/agent runtime.

**Runlith evidence:** provider architecture and `INV-066`/provider-session non-authority principles.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none. Runlith intentionally keeps a stronger product-level hierarchy above sessions.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: executable architecture invariants + generated contract catalogs

**External evidence:** package invariant companions, dependency-policy verifier, generated module graph, generated persistence schema/catalog and stale-artifact verification.

**Closest Runlith equivalent:** `INV-081` and Source of Truth § architecture fitness budgets, including planned `cargo metadata`/custom dependency checks.

**Runlith evidence:** `docs/architecture/INVARIANTS.md` `INV-081`; Source of Truth architecture-fitness section. Targeted checkout search found no implemented checker/script/CI gate and no canonical queue task owning it.

**Current status:** `PARTIALLY_EXISTS`.

**Actual gap:** implement the already-approved architecture fitness invariant: dependency direction/boundary checks, crate/module growth budgets and source-derived/staleness-checked architecture contract artifacts appropriate to Rust/Runlith. This is an implementation gap, not a new architecture direction.

**Decision:** `ADAPT`.

## External validation of existing Runlith design

The review materially validates these Runlith decisions:

1. **Task/workflow truth must not live in provider/session/runtime state.** DeepSeek itself needs durable session descriptors and reconstructable activations; its process-local jobs/workflows show why live handles cannot be canonical work truth.
2. **Durability before side effect is a practical pattern, not only an abstract recovery rule.** The checkpoint policy implements it centrally and fail-closed.
3. **Ambiguous side effects must stay ambiguous.** `TOOL_OUTCOME_UNKNOWN` is distinct from “not started” and explicitly warns against blind retry.
4. **Policy/approval must guard the operation boundary.** Missing approval infrastructure does not become allow.
5. **Runtime wrappers must not erase caller authority/lifecycle controls.** Cancellation is re-fused after around-dispatch middleware.
6. **Derived state should be rebuildable.** Session projections, timer runtime and activations are explicitly derived from durable sources where continuity matters.
7. **Full event sourcing is not required to learn from event-sourced rigor.** Runlith can retain its aggregate stores while adopting immutable facts, versioned schemas, deterministic projections and explicit durability barriers.
8. **SSRF policy needs connection-level binding.** DeepSeek demonstrates the TOCTOU closure Runlith's existing network policy architecture anticipates.
9. **Large context should be reduced without deleting evidence.** Full artifact + bounded preview/locator validates Runlith's context/artifact direction.
10. **Architecture constraints should become executable as the repository grows.** This validates `INV-081` and shows a mature implementation shape.

## Novel gaps

### NG-01 — Architecture fitness checks are designed but not implemented

**External idea:** executable invariants and source-derived architecture/persistence catalogs.

**External evidence:** `scripts/test-invariants.ts`, `verify-package-dependencies.ts`, `gen-module-graph.ts`, `gen-persistence-catalog.ts`, package invariant companions.

**Closest Runlith equivalent:** `INV-081`; Source of Truth architecture-fitness budgets and proposed CI dependency checks.

**Runlith evidence:** documentation explicitly requires the mechanism, but targeted checkout search found no `scripts/`, `.github` architecture checker, xtask or canonical queue task implementing the invariant.

**Current status:** `PARTIALLY_EXISTS`.

**Actual gap:** turn `INV-081` into an executable, deterministic gate over the Rust workspace. At minimum it should detect forbidden dependency direction/boundary drift and monitor agreed crate/module growth budgets. Source-derived architecture graph/catalog output is valuable when it can be regenerated and checked for staleness.

**Differentiation:** Runlith's version should focus on architectural authority boundaries that matter to its product: core/domain must not depend on adapters/UI/provider specifics; provider/runner/policy boundaries must remain replaceable; generated output must link to stable crate/module evidence. It should avoid copying DeepSeek's extreme package granularity or building a generic TypeScript-style invariant platform.

**Decision:** `ADAPT`.

No other reviewed idea passed the novelty test. The strongest recovery, approval, context, SSRF, schedule and provider-neutrality mechanisms already have direct Runlith equivalents or accepted future architecture.

## Adoption matrix

| External idea | Runlith status | Decision | Meaningful Runlith delta |
| --- | --- | --- | --- |
| Immutable session acceptance / derived projections | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` | Keep aggregate stores; borrow immutable/versioned fact discipline. |
| Pre-side-effect durability barrier | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | G8 adds product-wide Attempt/Policy/AMBIGUOUS reconciliation semantics. |
| Physical vs semantic recovery split | `ALREADY_DESIGNED` / partly implemented | `EXTERNAL_VALIDATION` | Trusted application services/Reconciler own Task semantics. |
| `TOOL_NOT_STARTED` vs `TOOL_OUTCOME_UNKNOWN` | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Runlith applies it to filesystem/network/provider side effects through Attempt recovery. |
| Conservative tool concurrency classifier | `ALREADY_DESIGNED` principle | `INSPIRED_ONLY` | Future tool registry can make independence/effect class typed and policy-aware. |
| Fail-closed one-shot approval | `ALREADY_IMPLEMENTED` / designed | `EXTERNAL_VALIDATION` | Runlith binds exact request/capabilities/policy fingerprint/provenance/validity. |
| Kernel single-writer persistence lease | `ALREADY_DESIGNED` principle | `EXTERNAL_VALIDATION` | Runlith uses storage/lease mechanisms appropriate to SQLite + workspace ownership. |
| Local sandbox fail-closed selection | `ALREADY_IMPLEMENTED` / hardening | `EXTERNAL_VALIDATION` | Runlith keeps explicit IsolationPlan/Runner contract and stricter capability proof. |
| DNS-address pinning after SSRF validation | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Implement later behind provider-neutral NetworkAccessPolicy with per-hop receipts. |
| Full-output spill + preview locator | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Runlith artifacts/context carry stronger provenance/trust/sensitivity lifecycle. |
| Durable schedules + disposable timers | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | G11 uses RoutineDefinition/RoutineRun and explicit delivery/recovery policy. |
| Durable child descriptor + live activation | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Runlith keeps Task/Execution/Attempt above provider/subagent session. |
| Executable architecture invariants/catalogs | `PARTIALLY_EXISTS` | `ADAPT` | Rust/Runlith-specific dependency/size/authority fitness gate. |
| LLM retry `always` | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Runlith requires finite retry/exhaustion semantics. |
| Unbounded agent step loop | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Runlith requires bounded autonomous loops and escalation. |
| Holder-owned workflow as canonical work | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Durable Task Graph/Reconciler remains canonical. |
| Unprobed custom sandbox runner as full enforcement | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Security capability claims require enforceable/provable backend contract. |

## Ideas rejected for Runlith

### Full session event sourcing as the general Runlith persistence model

Rejected as a direct adoption. Runlith already made a deliberate architectural choice: full Event Sourcing is outside v1. The useful parts are deterministic/typed durable facts, explicit versioning, derived projections and recovery evidence.

### Unbounded `always` provider retry

Rejected. Backoff does not make an unbounded retry loop bounded. A finite budget, exhaustion state and escalation/recovery owner are required.

### Model/tool loop without a global lifecycle budget

Rejected for Runlith autonomous engineering Tasks. A model emitting another valid tool call is not proof that another iteration should always be allowed.

### Process-owned workflow script as durable orchestration truth

Rejected. Such scripts may exist as Attempt-local helpers, but Task Graph/Reconciler state must survive process/provider loss independently.

### Custom sandbox runner as unprobed full-security attestation

Rejected. Trusted operator configuration may select a backend, but unsupported/unproven mandatory restrictions must fail closed and capability claims must be testable.

### Copying the package-per-capability topology

Rejected as an implementation target. The useful idea is replaceable capability seams and executable dependency rules. Runlith should preserve its current Rust bounded contexts and introduce packages/crates only when they have independent ownership/contracts.

## Candidate ADRs

None.

The only novel/partial gap found already has accepted architectural direction in Source of Truth and `INV-081`. It needs implementation planning, not a new ADR. Side-effect durability, recovery, approval, provider neutrality, context provenance, web SSRF and scheduled autonomy already have accepted Runlith architecture or gate ownership.

## Candidate implementation tasks

### TASK-CANDIDATE — Architecture fitness and source-derived contract gate

**Type:** `TASK`

**Problem:** `INV-081` requires architecture fitness checks, but the current checkout has no implemented dependency/size fitness checker and no source-derived architecture freshness gate.

**Current Runlith state:** architecture boundaries and module/crate growth expectations exist in architecture docs/Source of Truth; implementation is absent from the reviewed checkout and no canonical queue task owns it.

**External inspiration:** DeepSeek Harness verifies actual source/manifests against dependency policy, mounts per-package invariant companions and generates/checks module/persistence catalogs from source.

**Actual delta:** implement the already-approved Runlith invariant with minimal Rust-workspace-specific tooling. Start with dependency-direction and forbidden-boundary checks plus crate/module growth budgets. Add generated architecture graph/catalog output only where it has stable source-of-truth value.

**Target subsystem:** repository engineering/CI architecture fitness; no production runtime dependency.

**Dependencies:** accepted dependency-direction rules and explicit initial size/budget thresholds derived from existing architecture; existing Cargo workspace metadata.

**Architecture impact:** positive enforcement of existing architecture; must not introduce a second hand-maintained architecture truth.

**Security impact:** helps preserve Policy/Runner/provider/core boundaries against accidental dependency drift; checker itself carries no runtime authority.

**Acceptance direction:**

- deterministic check from repository source/Cargo metadata;
- fails on explicitly forbidden dependency direction/boundary;
- monitors agreed module/crate growth budget with explicit reviewed exception path;
- if generated architecture artifacts are committed, `--check`/equivalent detects stale output;
- no production crate behavior changes required;
- useful diagnostics identify source and violated rule.

**Verification direction:** fixture/unit coverage for allowed/forbidden graphs; run against current workspace; prove generated output is stable; CI command exits non-zero on deliberate dependency violation and stale generated artifact.

**Suggested gate:** cross-cutting architecture quality / before further G9-G11 repository expansion.

**Priority:** `MEDIUM` now, increasing with crate/module/plugin growth.

## Source evidence index

| Finding | Repository source | Symbol / region | Confidence |
| --- | --- | --- | --- |
| DSH-F01 | `packages/core/session/src/index.ts` | `Session.append`, ~680-765 | `HIGH` |
| DSH-F02 | `packages/session/session-checkpoint-policy/src/index.ts` | model/tool/request checkpoint listeners | `HIGH` |
| DSH-F03 | `packages/core/agent-loop/src/index.ts`; `packages/core/session/src/repair.ts` | `resumeWith`; `interruptedTurnClosers` | `HIGH` |
| DSH-F04 | `packages/core/tools/src/index.ts` | execution-mode classifier; execute pipeline; `serviceAsk` | `HIGH` |
| DSH-F05 | `packages/interaction/user-approval/src/index.ts` | `ApprovalService.request`; decision normalization | `HIGH` |
| DSH-F06 | `packages/session/session-persistence-jsonl/src/storage.ts`; `lease.ts`; `index.ts` | writer chain, lease, flush, torn-tail recovery | `HIGH` |
| DSH-F07 | `packages/sandbox/sandbox-policy/src/index.ts`; `packages/sandbox/sandbox-local/src/index.ts` | policy resolution; `confine`; `chainVerdict` | `HIGH` |
| DSH-F08 | `packages/web/web-fetch-http/src/policy.ts`; `network.ts` | `validateFetchUrl`; `resolvePublicAddresses`; `requestPinned` | `HIGH` |
| DSH-F09 | `packages/spill/spill/src/index.ts`; `spill-policy/src/index.ts`; `context/session-reference/src/spill.ts` | `saveText`; post-execute spill; reference omission | `HIGH` |
| DSH-F10 | `packages/schedule/schedule/src/{domain,persistence,runtime}.ts` | schedule fold, flush barrier, `driveOnce` | `HIGH` |
| DSH-F11 | `packages/subagent/subagent/src/depth.ts`; `continuation.ts` | `delegationDepthOf`; `SubagentContinuationManager` | `HIGH` |
| DSH-F12 | `scripts/test-invariants.ts`; `verify-package-dependencies.ts`; `gen-module-graph.ts`; `gen-persistence-catalog.ts` | invariant host and source-derived verification | `HIGH` |
| DSH-AP01 | `packages/llm/llm/src/retry-policy.ts`; `packages/llm/llm-retry/src/index.ts` | `AlwaysRetryPolicyConfig`; `recover` | `HIGH` |
| DSH-AP02 | `packages/core/agent-loop/src/agent.ts`; `packages/core/agent-loop/src/index.ts` | turn/step loops and config | `HIGH` |
| DSH-AP03 | `packages/workflow/workflow/src/index.ts`; `packages/workflow/workflow-ptc/src/host.ts` | holder-owned `WorkflowRun`; live child/run control | `HIGH` |
| DSH-AP04 | `packages/sandbox/sandbox-local/src/index.ts:305-326` | configured `runnerCommand` path | `HIGH` for behavior; `PLAUSIBLE` for misconfiguration exploit consequence |

### Runlith evidence used for overlap

- `AGENT.md` — stateful retry/failover, bounded Reaction/Reconciler, typed retry rules.
- `docs/architecture/INVARIANTS.md` — especially `INV-066`, `INV-071`, `INV-075`, `INV-081`, `INV-091`, `INV-097..105`, `INV-121`, `INV-145`, `INV-150`.
- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` — G8 Side-Effect Execution Protocol; Context Engine/large-result artifact design; Web Acquisition/NetworkAccessPolicy; G11 routines; architecture-fitness budgets.
- `planning/roadmap.yaml` — G8 recovery/verification, G9 context continuity, G10/G11 future capability scope.
- `planning/tasks/queue.yaml` — current recovery/security/planning work and absence of a canonical architecture-fitness implementation task.
- `docs/architecture/events.md`, `continuity.md`, `providers.md`, `workspace.md` and accepted ADRs for durable ownership/recovery/provider boundaries.
- current `crates/`, `adapters/`, `bins/` read-only searches for existing implementation overlap.

## Research conclusion

DeepSeek Harness is highly useful as implementation evidence, but it does not justify a broad Runlith roadmap expansion. Its best recovery, security, context and capability ideas mostly confirm architecture Runlith already has. The one actionable new implementation item is to enforce the architecture-fitness invariant that Runlith has already documented but not yet implemented.

The most important negative lessons are equally clear: do not allow unbounded retry/agent loops, do not make holder/process workflow objects durable product truth, and do not treat a custom sandbox wrapper as proven enforcement merely because configuration says it is.

## Addendum 2026-10-10 — re-check at `d7432673` (release 0.2.1-alpha.2)

Re-fetched at `master@d743267388641bc76f17c45ce8b4c231aed1d32c` (2026-10-09). Landing page `https://deepseek.com/harness/` (via reader): desktop + `npx @deepseek-ai/dsh web`, Cordis plugin substrate, `SAFETY.md` (experimental, no audit, least-privilege + disposable-VM guidance). Source-read: `AGENTS.md` (package layout + gates), `packages/interaction/README.md`, `packages/guard/README.md`, `docs/subsystems/approval.md`, `docs/subsystems/permission-presets.md`.

New detail beyond the 2026-09-15 review:

- ADD-F01 — Approval outcome closed + fail-closed: `ApprovalOutcome = 'allowed-once' | 'rejected' | 'cancelled' | 'unavailable'`; missing/throwing/non-conforming answerer becomes `unavailable`, callers deny. Policy `ask | never`; `never` enforced inside service before waterfall dispatch (later `prepend` cannot bypass). Audit pair `approval/asked` + `approval/decided`, log-only, requires open turn. Request omits tool args, links via `callId` to already-streamed call (no drifted second copy).
- ADD-F02 — Permission presets bundle two independent knobs (`sandbox/mode` + `approval/policy`) into one selector: `workspace-write` (confine + ask), `danger-full-access` (+ never). `custom` derived-only (never a switch target); `auto` fixed current-session registration via `registerAuto(admit)` with effect-lifetime disposal. `permission/preset` event durable + log-only, precedes knob events. Requires confining shell executor, else load fails.
- ADD-F03 — Guard family: `repeat-tool-reminder` (advisory stuck-loop nudge) + `timeout-policy` (declared-limit timeout → clear error). Both ship enabled.
- ADD-F04 — `AGENTS.md` conventions worth noting: registrations are effects (`ctx.effect()`/disposer); waterfall listeners MUST call `next()`; model-visible ⟺ logged (new model-visible input requires session event); explicit `resolve(request): Spec` over hidden defaults; misconfiguration fails loud; branded opaque ids; `SESSION_FORMAT_VERSION` bump discipline; pre-stable APIs with upgrade-guide requirement.

Что лучше у них:

- Замкнутый approval-словарь с `unavailable`-по-умолчанию и `never`-до-waterfall — защита от позже-подключённого answerer. У Runlith fail-closed есть, но явного «политика применяется до диспетча, bypass невозможен» стоит зафиксировать.
- Связка sandbox+approval в один пресет с derived `custom` — удобно и честно (показывает дрейф от пресетов). У Runlith пресеты политики/изоляции разделены.
- `callId`-линковка аппрува к уже-показанному вызову вместо дублирования аргументов.
- Дисциплина `model-visible ⟺ logged` как явное правило репозитория.

Что лучше у меня:

- G6: exact-bound single-use approvals с fingerprint/provenance/validity против их `allowed-once`-без-границ; child-subset против их делегирования.
- Durability barrier до side-effect + `TOOL_OUTCOME_UNKNOWN` против их repair-текста.
- Хеш-цепочка журнала против их log-only audit-пары.
- DNS-pinning они делают, у меня запроектировано за NetworkAccessPolicy с per-hop receipts — паритет по намерению.

Вердикты аддендума:

- R-01 — ADAPT, P3: зафиксировать «policy-before-dispatch, no bypass» как G6-инвариант (нужен ли ADR — решить при ближайшем касании G6).
- R-02 — ADAPT, P3: derived `custom`-индикатор дрейфа для UI пресетов политики/изоляции.
- R-03 — EXTERNAL_VALIDATION, P3: `callId`-линковка как образец для approval-UX.
- R-04 — EXTERNAL_VALIDATION, P3: `model-visible ⟺ logged` подтверждает журнальную дисциплину G8.
- REJECT: Cordis-движок/пакеты/TS-код, `danger-full-access` как продуктовая норма, unbounded-конструкции (подтверждён прежний REJECT), `ee/`-аналогов нет, телеметрия хоста.

Evidence (addendum): `https://raw.githubusercontent.com/deepseek-ai/deepseek-harness/d743267388641bc76f17c45ce8b4c231aed1d32c/docs/subsystems/approval.md`, `.../docs/subsystems/permission-presets.md`, `.../AGENTS.md`, `.../SAFETY.md`, `.../packages/interaction/README.md`, `.../packages/guard/README.md`; commits `https://github.com/deepseek-ai/deepseek-harness/commits/master.atom` (head `d743267388641bc76f17c45ce8b4c231aed1d32c`, 2026-10-09). Landing: `https://deepseek.com/harness/` (reader 2026-10-08).
