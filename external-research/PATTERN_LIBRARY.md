# External Pattern Library

Reusable architecture concepts observed in external repositories. This is not an implementation backlog. `Runlith status` records overlap with current Runlith architecture before any adoption decision.

## P-001 — Metadata-first lazy skill loading

**Pattern:** Discover compact skill metadata globally; materialize full instructions only when selected for the current task.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/skills/loader.ts:42-109,151-206`; `src/capabilities/skills/use-skill.ts:6-34`.

**Problem:** Installed workflow count can grow much faster than the useful context for one execution.

**Mechanism:** Metadata discovery/index → compact summaries/routing → on-demand body load.

**Runlith status:** `ALREADY_DESIGNED` — `INV-028`, Context Engine progressive disclosure, deferred `SkillRegistryPort`/Tools-Skills Registry target.

**Runlith adaptation:** Preserve source/trust metadata, retrieval provenance and Policy separation. Skill content may request capability but cannot grant authority.

**Constraints:** No full skill-body prompt bloat; no repository/provider/skill text promotion to system/policy authority.

**Relevant invariant:** `INV-028`, `INV-090`, `INV-098`.

## P-002 — Capability registry separated from authorization

**Pattern:** Compose executable tools in one inspectable registry while keeping authority decisions in a separate policy boundary.

**Source:** `cosmicstack-labs/mercury-agent` (registry pattern); Runlith adaptation deliberately rejects Mercury's skill-elevation coupling.

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/capabilities/registry.ts:79-103,174-291`; `src/capabilities/permissions.ts:283-361`.

**Problem:** Distributed tool registration obscures what an agent can invoke; combining registry membership with authorization creates accidental authority.

**Mechanism:** Central capability/tool inventory plus a separate typed Policy decision/enforcement path.

**Runlith status:** Tool/Skill Registry `ALREADY_DESIGNED`; typed G6 capability/Policy authority `ALREADY_IMPLEMENTED`.

**Runlith adaptation:** Registry describes availability and contracts. `PolicyRequest -> ALLOW | DENY | REQUIRE_APPROVAL` decides authority; Runner/operation owner enforces effective capabilities.

**Constraints:** Registry membership, provider flags, skill metadata and UI state never grant authority.

**Relevant invariant:** G6 `INV-146..153`; `INV-090`.

## P-003 — Bounded autonomous loop termination

**Pattern:** Put hard numeric bounds and repeated-failure/no-progress detection around autonomous model/tool loops.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/core/agent.ts:148-355,1401-1412,2388-2401`; `src/core/supervisor.ts:242-305`.

**Problem:** Agent loops can consume unbounded time/tokens, repeat failed actions or hide incomplete work behind endless continuation.

**Mechanism:** Step/call/failure ceilings, loop detection, explicit pause and bounded continuation.

**Runlith status:** `PARTIALLY_EXISTS` / `ALREADY_DESIGNED` — G7 planner guardrails and budget envelopes are materialized; other autonomous loops remain owned by their subsystem contracts.

**Runlith adaptation:** Bounds live outside provider self-report and produce durable reason/evidence when work pauses, fails, or replans.

**Constraints:** A loop timeout is not Task failure or completion by itself; retry/recovery policy remains canonical.

**Relevant invariant:** `INV-009`, `INV-043`, `INV-045`, `INV-066`.

## P-004 — Structured memory with merge, conflict and provenance metadata

**Pattern:** Store typed memory records with evidence metadata and merge/supersession behavior rather than raw transcript append.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/memory/user-memory.ts:19-41,211-335,430-535,893-930`.

**Problem:** Unbounded conversation memory accumulates duplicates, contradictions, stale facts and summary-of-summary drift.

**Mechanism:** Typed records, bounded retrieval, candidate thresholds, dedup/merge, conflict resolution, confidence/evidence count and supersession.

**Runlith status:** `ALREADY_DESIGNED` by ADR-0012.

**Runlith adaptation:** Use real source refs/content hashes/validation status. Model confidence is metadata only; agent annotations stay untrusted until promoted under validation policy.

**Constraints:** Raw evidence remains authoritative; memory is not automatically injected into every context.

**Relevant invariant:** `INV-029`, `INV-049`.

## P-005 — Deterministic domain fold with derived projections

**Pattern:** Express business decisions as deterministic functions over validated inputs, then derive ledger/projection state from those decisions.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/domain/settlement.ts:3-80`; `src/lib/domain/ledger.ts:1-46`; `src/lib/moderation/transitions.ts:15-29`.

**Problem:** Repeating business semantics across routes/UI/workers creates duplicated canonical state and inconsistent transitions.

**Mechanism:** Pure typed decision functions and deterministic projections from accepted facts.

**Runlith status:** `ALREADY_IMPLEMENTED` as a core architecture principle in Task/Application Service/Event Journal flows; projection details remain subsystem-specific.

**Runlith adaptation:** Keep canonical Task/domain mutation in application/domain services; projections are rebuildable and never competing truth.

**Constraints:** Determinism does not promote untrusted external evidence to authority.

**Relevant invariant:** `INV-009`, `INV-043`, `INV-070`.

## P-006 — Separate reconciliation evidence from synchronization ownership

**Pattern:** Persist what was observed separately from the worker/version/scan state used to coordinate rederivation.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/fold/reconciliation-evidence.ts:3-34`; `src/lib/fold/reconciliation-jobs.ts:1-18`.

**Problem:** If evidence payload and worker ownership are merged, retry/reclaim mechanics can silently rewrite the meaning of the observation.

**Mechanism:** Versioned upstream evidence/checkpoint plus separate expected-version, dirty-subject, reason, attempt, lease-token and generation metadata.

**Runlith status:** Evidence/authority separation `ALREADY_DESIGNED`; general external-signal job ownership is `NEEDS_ADR`.

**Runlith adaptation:** External signal/evidence enters Journal/Reconciler with provenance; durable claim/generation coordinates processing but never changes the evidence's trust level.

**Constraints:** Event replay and job reclaim do not authorize replay of side effects.

**Relevant invariant:** `INV-066`, `INV-070`, `INV-091`.

## P-007 — Durable bounded reconciliation claim with stale-writer fencing

**Pattern:** Coordinate asynchronous reconciliation using durable claims, bounded lease renewal/retries/concurrency, and require a current fencing generation on every authoritative publication.

**Source:** `Nitjsefnie/Overflow` for claim/retry mechanics; the fencing requirement is the Runlith improvement derived from Overflow's documented limitation.

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/fold/reconciliation-worker.ts:40-149,200-353`; documented missing write fencing at `127-134`.

**Problem:** A crashed, partitioned or superseded worker can continue writing after ownership moved to a reclaimer; hidden retries can also run forever or monopolize capacity.

**Mechanism:** Durable claim token/generation, finite lease/renewal horizon, bounded retries/defer/concurrency, plus compare/fence condition on authoritative commit.

**Runlith status:** `NEEDS_ADR` for generic external-lifecycle reconciliation. G4/G5 already implement narrower workspace/execution ownership and recovery.

**Runlith adaptation:** Source-neutral Reconciler claim, Event Journal correlation, expected generation/version and stale-writer fencing at publication. Lease heartbeat proves liveness only.

**Constraints:** No implementation task before an active gate needs the generic contract and the ADR is accepted.

**Relevant invariant:** `INV-001`, `INV-066`, `INV-091`, `INV-145`.

## P-008 — Revalidate current authorization at the operation boundary

**Pattern:** Resolve acting identity, then read current canonical authorization state instead of trusting role/permission claims embedded in an older session.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/moderation/route-auth.ts:33-79`.

**Problem:** Long-lived or stale client/session claims can outlive revocation or policy changes.

**Mechanism:** Authenticate the credential, reload current role/state, fail closed before privileged operation.

**Runlith status:** `ALREADY_IMPLEMENTED` conceptually and concretely through G6 Policy/approval operation-boundary enforcement.

**Runlith adaptation:** Re-present current exact approval/effective capability at the trusted operation boundary; never treat prior UI/provider/session state as bearer authority.

**Constraints:** Backing-store failure cannot silently become allow.

**Relevant invariant:** G6 `INV-146..153`.

## P-009 — Compatibility-gated native snapshot reuse with replay fallback

**Pattern:** Treat opaque model/provider-native cached state as reusable only when an explicit compatibility envelope matches; otherwise invalidate it and reconstruct from authoritative history/checkpoints.

**Source:** `yifanzhang-pro/recurrent-looped-tranformer`

**Revision:** `9c40dec78a6d7da73e1226310fd6c8edaa32577e`

**Source evidence:** `Recurrent_Looped_Transformer.pdf` §5.4 (pp. 9-10), §6 (p. 10), Appendix C (pp. 18-19). The report binds exact prefix reuse to current parameters/model version, consumed prefix, positions, initialization, attention-window semantics, complete cached state, and execution settings; incompatible state is rebuilt/replayed.

**Problem:** A session/cache handle can survive after the assumptions that produced its hidden state changed, making a technically resumable state semantically stale.

**Mechanism:** Define compatibility inputs, persist enough provenance to evaluate them, reuse only on a match, and fall back to reconstruction from a valid checkpoint/history on mismatch.

**Tradeoffs:** More compatibility metadata and occasional replay cost in exchange for deterministic resume semantics. Native-state reuse remains an optimization and can be disabled without losing durable work.

**Runlith status:** General cache invalidation and provider-independent continuity are `ALREADY_DESIGNED`; the exact provider-native resume compatibility/invalidation contract is `NEEDS_ADR` (`continuity.md` explicitly leaves provider migration resume protocol, Session ↔ Epoch linkage and checkpoint version compatibility open).

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** `ADR-0011`, `docs/architecture/continuity.md`, `docs/architecture/providers.md`, `INV-019`, `INV-022`, `INV-023`, `INV-066`, `INV-110`.

**Runlith differentiation:** Native resume is provider-neutral in semantics, fail-closed on incompatible state, auditable, and always has an application-owned continuity fallback. Native session state never carries Task, Policy, side-effect, or completion authority.

## P-010 — Activity-derived liveness for durable worker claims

**Pattern:** Refresh durable worker liveness from runtime/tool activity observed by trusted infrastructure rather than requiring the model to emit a heartbeat action.

**Source repository:** `NousResearch/hermes-agent`

**Source revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `tools/kanban_tools.py:415-463` (`heartbeat_current_worker_from_env`, run-pinned auto-heartbeat); `hermes_cli/kanban_db.py` stale-claim recovery around `2329+`.

**Problem solved:** A healthy autonomous worker can be falsely reclaimed when lease extension depends on prompt/model compliance rather than actual execution activity.

**Mechanism:** Runtime activity triggers a rate-limited durable heartbeat/claim refresh; the current run id fences the update so a reclaimed stale run cannot revive itself.

**Tradeoffs:** Heartbeat is liveness evidence, not proof of useful progress or correctness. Best-effort write failure still requires conservative recovery policy.

**Runlith status:** `PARTIALLY_EXISTS` — `LiveProcessTracker` already supplies provider-neutral `OwnerLiveness` for workspace cleanup, while durable post-Attempt/orphan recovery is explicitly deferred to G11.

**Decision:** `ADAPT` as a future G11/recovery implementation technique; no standalone implementation task yet.

**Relevant Runlith architecture:** `docs/architecture/providers.md`, `ADR-0024`, `ADR-0025`, G11 Background daemon, `INV-001`, `INV-078`, `INV-091`.

## P-011 — Failure-scope-aware provider fallback

**Pattern:** Classify provider failure by the backend-identity dimension it invalidates, then skip only fallback candidates that repeat that failed surface.

**Source repository:** `NousResearch/hermes-agent`

**Source revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `agent/backend_identity.py:23-153` (`FailureScope`, `BackendIdentity`, `should_skip_candidate`, `classify_failure_scope`); consumers in `agent/chat_completion_helpers.py` around `1750+` and `agent/auxiliary_client.py` around `3927+`; retry ownership in `agent/agent_runtime_helpers.py` and reviewed adapters with SDK retries disabled.

**Problem solved:** Naive fallback either repeats the same broken deployment under another alias or excludes healthy alternatives too broadly.

**Mechanism:** Normalize backend identity, classify the failure axis (for example model/deployment, credential surface, endpoint), compare candidates only along that axis, and keep outer orchestration responsible for retry/cooldown/fallback.

**Tradeoffs:** Failure classification can be wrong or incomplete; route invalidation therefore needs provenance, bounded cooldowns and conservative unknown handling. Stateful side effects still require independent reconciliation before retry.

**Runlith status:** `NEEDS_ADR` — Adaptive Model Router / `RoutePlan` is already designed, but `docs/architecture/adr/COVERAGE.md` still marks it deferred with no accepted ADR, and no typed failure-scope invalidation contract was found.

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** `docs/architecture/providers.md`; Source of Truth Adaptive Model Router; `INV-059`, `INV-060`, `INV-066`, `INV-067`, `INV-085`, `INV-093`.

## P-012 — Same-blocker recurrence escalation

**Pattern:** Preserve blocker identity across unblock/retry cycles, count recurrence of the same blocker separately from other failures, and escalate when the recurrence budget is exhausted.

**Source repository:** `NousResearch/hermes-agent`

**Source revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `hermes_cli/kanban_db.py:3048-3072` (`_route_block`, `block_recurrences`, `block_loop_detected`).

**Problem solved:** Individually valid `BLOCKED -> RUNNING -> BLOCKED` transitions can form an infinite autonomous loop when the underlying impediment never changes.

**Mechanism:** Stable blocker kind/identity, durable recurrence accounting, explicit escalation outcome after a finite budget.

**Tradeoffs:** The blocker taxonomy, equality rules, reset rules, budget owner and escalation destination are product semantics and cannot be copied mechanically.

**Runlith status:** `NEEDS_ADR` — planning loops are bounded (`INV-050`), but `docs/architecture/task-engine.md` explicitly leaves typed blocker taxonomy and retry-budget ownership/exhaustion unresolved.

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** `docs/architecture/task-engine.md`, `ADR-0017`, `ADR-0027`, G8 Verification/Recovery, `INV-050`, `INV-091`.

## P-013 — Durable scheduled-occurrence claim with explicit delivery semantics

**Pattern:** Give each scheduled occurrence a durable identity/claim and document whether the scheduler prefers duplicate avoidance or guaranteed eventual execution under crash.

**Source repository:** `NousResearch/hermes-agent`

**Source revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `cron/jobs.py` — `_jobs_lock` around `272+`, `_fire_job_lock` around `326+`, `_save_jobs_unlocked` around `1467+`, `advance_next_run` `2682-2687`, `claim_job_for_fire` `2704+`.

**Problem solved:** Multiple scheduler instances or crash/restart windows can double-fire one occurrence or produce a restart burst.

**Mechanism:** Durable occurrence stamp/claim TTL, a separate fire fence, atomic state publication, and an explicit at-most-once tradeoff for recurring jobs by advancing the next occurrence before execution.

**Tradeoffs:** At-most-once can miss work. Different routine classes may require reconciliation or at-least-once processing instead.

**Runlith status:** `ALREADY_DESIGNED` at principle level — G11 already defines durable/auditable `RoutineDefinition` / `RoutineRun` workflows. Exact occurrence/delivery semantics remain future G11 contract work.

**Decision:** `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` for the exact implementation.

**Relevant Runlith architecture:** Source of Truth G11 Routine/Pulse Engine; `INV-001`, `INV-066`, `INV-078`, `INV-091`.

## P-014 — Least-trust tool metadata before external transport

**Pattern:** Treat missing or malformed read-only metadata as write-capable, require approval for untrusted write-capable tools, and perform the trust decision before spawning/connecting to the external tool transport.

**Source repository:** `NousResearch/hermes-agent`

**Source revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**Evidence:** `tools/mcp_tool.py` trust/write gate around `466-516+`; `tools/registry.py:623-680` schema validation and override/ownership checks.

**Problem solved:** Optimistically trusting incomplete tool metadata can turn malformed discovery data into unintended authority or external effects.

**Mechanism:** Conservative capability classification, policy/approval before transport, and registry ownership rules that prevent accidental shadowing/replacement.

**Tradeoffs:** Conservative defaults can create extra approvals for poorly described tools. The remedy is better signed/versioned metadata, not silent trust widening.

**Runlith status:** G6 typed Policy/approval semantics are `ALREADY_IMPLEMENTED`; detailed Tool/Skill Registry mechanics are `ALREADY_DESIGNED` / deferred.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** `ADR-0026`, future Tool/Skill Registry target, `INV-090`, G6 `INV-146..153`.

## P-015 — Semantic durability barrier immediately before external side effects

**Pattern:** Persist the accepted request/intent boundary before invoking a model, tool, or other external side effect; fail closed when the durability barrier cannot be established.

**Source repository:** `deepseek-ai/deepseek-harness`

**Source revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `packages/session/session-checkpoint-policy/src/index.ts` installs flush barriers before model adapter dispatch, top-level tool execution and request/step boundaries; `packages/core/session/src/index.ts` owns the canonical session flush entry point.

**Problem solved:** A crash after an external effect begins but before the caller's intent/input is durable can leave recovery unable to prove what was attempted.

**Mechanism:** append validated intent/input -> await one semantic persistence barrier -> invoke the external effect only after successful flush. A failed flush blocks the effect.

**Tradeoffs:** The barrier does not eliminate ambiguous outcomes after dispatch; it only gives recovery a durable pre-effect boundary. Non-idempotent external effects still require explicit outcome reconciliation.

**Runlith status:** `ALREADY_DESIGNED` by G8 Side-Effect Execution Protocol, `INV-066`, durable Attempt/journal recovery and `AMBIGUOUS -> RECONCILE` semantics.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** `planning/roadmap.yaml` G8; Source of Truth G8 Side-Effect Execution Protocol; `INV-066`, `INV-091`; `docs/architecture/events.md`.

## P-016 — Split physical persistence repair from semantic lifecycle recovery

**Pattern:** Let the storage layer repair/validate physical records and let the owning domain/runtime repair interrupted lifecycle semantics from that physically valid state.

**Source repository:** `deepseek-ai/deepseek-harness`

**Source revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `packages/session/session-persistence-jsonl/src/{storage,lease,index}.ts` owns format, torn-tail, contiguous-write and writer-lease recovery; `packages/core/agent-loop/src/index.ts` `resumeWith` and `packages/core/session/src/repair.ts` `interruptedTurnClosers` own semantic interrupted-turn closure. The latter distinguishes `TOOL_NOT_STARTED` from `TOOL_OUTCOME_UNKNOWN` and refuses to normalize an unknown side effect into a blind retry.

**Problem solved:** Storage knows whether bytes/rows are valid but usually cannot know the business meaning of an interrupted operation. Mixing both layers encourages storage adapters to invent lifecycle transitions or silently retry ambiguous effects.

**Mechanism:** recover a physically valid prefix first; then apply domain-specific reconciliation with typed ambiguity and idempotency rules.

**Tradeoffs:** More explicit recovery layers and typed failure states; the benefit is clearer ownership and safer restart behavior.

**Runlith status:** `ALREADY_DESIGNED` and partly `ALREADY_IMPLEMENTED` through SQLite/Git separate failure domains, trusted application services/Reconciler ownership and existing execution/workspace recovery.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** `INV-066`, `INV-145`, `docs/architecture/workspace.md`, `docs/architecture/events.md`, G8 recovery.

## P-017 — Bind SSRF validation to the actual network connection

**Pattern:** Resolve and policy-check the destination address set once, then constrain the actual connection to that validated set so DNS cannot change the destination after policy evaluation.

**Source repository:** `deepseek-ai/deepseek-harness`

**Source revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `packages/web/web-fetch-http/src/policy.ts` validates HTTP(S) URLs and redirect origin; `packages/web/web-fetch-http/src/network.ts` `resolvePublicAddresses`, `requestPinned` and `createPinnedLookup` validate public IPv4/IPv6/NAT64 destinations and pass only the validated addresses to the request's connection lookup.

**Problem solved:** URL/hostname checks followed by an independent DNS lookup leave a DNS-rebinding/TOCTOU window where the socket can reach a private or otherwise forbidden target.

**Mechanism:** centralized address policy + per-request address pinning; reevaluate policy on every permitted redirect hop.

**Tradeoffs:** Proxy routing needs separate trust semantics because the proxy owns DNS resolution. NAT64/IPv6 classification makes the implementation more involved than blocking a few IPv4 ranges.

**Runlith status:** `ALREADY_DESIGNED` by `NetworkAccessPolicy`, `INV-097` and the Source of Truth SSRF boundary. The concrete pinning mechanism is future implementation knowledge for G10, not a new architecture gap.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** Web Acquisition/Browser section in Source of Truth; `INV-097`, `INV-098`, `INV-099`, `INV-103`, `INV-121`.

## P-018 — Source-derived architecture contracts with stale-artifact verification

**Pattern:** Derive architecture/dependency/persistence catalogs from source, encode critical boundary rules as executable invariants, and fail CI/tests when source violates the rule or a committed generated artifact is stale.

**Source repository:** `deepseek-ai/deepseek-harness`

**Source revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**Evidence:** `scripts/test-invariants.ts`, `scripts/verify-package-dependencies.ts`, `scripts/gen-module-graph.ts`, `scripts/gen-persistence-catalog.ts`, plus package-local `src/invariant.ts` companions.

**Problem solved:** architecture documentation and dependency rules drift as a modular repository grows; review alone cannot reliably detect every forbidden dependency or stale contract catalog.

**Mechanism:** deterministic source inspection -> machine-checkable rules/catalog -> checked-in output where useful -> `--check`/test gate for drift.

**Tradeoffs:** Custom verification machinery has its own maintenance cost and must encode meaningful boundaries rather than generate decorative diagrams.

**Runlith status:** `PARTIALLY_EXISTS`. `INV-081` and Source of Truth already mandate architecture fitness checks, including `cargo metadata`/equivalent dependency checks, but no implemented checker/CI gate or canonical task was found in the reviewed checkout.

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** Source of Truth architecture-fitness budgets; `INV-081`.

## P-019 — Topology-aware placement from measured locality and contention evidence

**Pattern:** Treat nominal device/worker capacity as incomplete placement input. Represent the locality and communication relationship between resources, retain measurement provenance, and use measured contention/interference evidence when deciding where tightly coupled work should run.

**Source repository:** `nanotron/ultrascale-playbook`

**Source revision:** `c93890dd4b22356ee18ed9b863e6f2d0909729bc`

**Evidence:** `ultra_blog.md:1371-1410,2008-2027,2117-2197,2290-2301` shows materially different outcomes when communication crosses slower physical boundaries and when supposedly overlapped communication consumes compute resources. Companion implementation evidence: `huggingface/nanotron@2411b022a75fb7f7561a1bb4166706da5e1b76de`, `src/nanotron/parallel/context.py:12-140` and `scripts/scaling_benchmarks.py:177-224`.

**Problem solved:** Two resources with the same nominal compute/memory capacity can have different effective cost because link bandwidth/latency, host locality and shared-resource contention change the critical path.

**Mechanism:** Resource adapters expose provider-neutral locality/link evidence and its provenance; benchmark/runtime evidence is bound to a topology fingerprint; Router/Scheduler consume a derived placement cost instead of importing platform-specific NVIDIA/Apple/cluster types.

**Tradeoffs:** Topology models become stale when hardware, networking, runtime or workload shape changes. Static estimates can be wrong, so confidence/expiry and empirical validation remain necessary. The contract should stay dormant until a real multi-device or remote-worker workload needs it.

**Runlith status:** `NEEDS_ADR`. `DeviceResourceManagerPort`, `ModelResidencyManagerPort`, `ModelRuntimeProfile`, G12 worker lifecycle/capacity and hardware-fingerprinted measurements are already designed, but no accepted resource-locality/interconnect/contention evidence contract was found.

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** Adaptive Model Router and Device Resource Management in `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`; G12 Remote Worker; `INV-063`, `INV-064`, `INV-080`.

**Runlith differentiation:** Keep placement provider-neutral, policy/privacy constrained, evidence-bearing and replayable. Topology evidence may change cost or feasibility, but never widens execution authority or overrides Policy/Verification.

## P-020 — Optimize selectors against the downstream outcome, not a proxy ranking

**Pattern:** When a selector/router controls a scarce execution budget, evaluate or learn its policy against the downstream outcome that actually matters rather than assuming a convenient surrogate ranking is equivalent.

**Source repository:** `Tencent-Hunyuan/Simple-Attention-Sparsification`

**Source revision:** `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`

**Evidence:** `src/sas/core/attn/simple_sparse_attention.py:295-340,403-530`; `src/sas/train/trainer.py:57-61,441-447`; `README.md:9-26`; linked `arXiv:2609.13141v1` ablations.

**Problem solved:** A proxy objective can rank candidates like the baseline while still spending a fixed budget on choices that do not maximize the final useful outcome.

**Mechanism:** Keep a hard execution budget/selection boundary, preserve a continuous preference signal, and train/evaluate that preference against the final objective. At Runlith's layer this translates to route/context/efficiency promotion based on verified task outcomes, not gradient training.

**Tradeoffs:** The final objective can be expensive or delayed, and one metric can itself be gamed. Hard correctness/security/privacy floors and multi-objective cost remain necessary.

**Runlith status:** `ALREADY_DESIGNED` through `VerifiedDeliveryCost`, the Efficiency Observatory, local verified routing feedback, and benchmark-as-evidence rules.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** Source of Truth Execution Efficiency Architecture + Adaptive Model Router; `INV-059`, `INV-060`, `INV-062`, `INV-080`.

## P-021 — Compatibility-gate every optimization-specific auxiliary state

**Pattern:** If an optimized execution path adds derived state beyond the owner resource, define the lifecycle assumptions explicitly and reject execution modes that cannot allocate, transfer, update, invalidate, or free that state correctly.

**Source repository:** `Tencent-Hunyuan/Simple-Attention-Sparsification` with pinned `rayleizhu/sglang@2a090d525cc52dc8eb70059543271baef34c5696` inference backend.

**Source revision:** parent `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`; inference `2a090d525cc52dc8eb70059543271baef34c5696`.

**Evidence:** SGLang `seer_attn/backend.py:84-180` rejects incompatible page layout, disaggregated serving, mixed prefill/decode, streaming sessions and speculative decoding; `mem_cache/blocksparse_summary.py:35-122,175-200` couples summary-slot allocation/free to KV/request lifecycle and fails hard on impossible exhaustion.

**Problem solved:** Derived optimization state can silently drift from the canonical resource and corrupt results while the main execution path still appears healthy.

**Mechanism:** One owner lifecycle, explicit compatibility predicate, fail-closed admission, and deterministic rebuild/transfer before enabling new execution modes.

**Tradeoffs:** Some high-performance modes remain unavailable until their state-transfer/recovery semantics are implemented.

**Runlith status:** `ALREADY_DESIGNED` and security eligibility principles partly `ALREADY_IMPLEMENTED`. Provider/session/cache state is derived; hard eligibility precedes routing preference; unsupported mandatory enforcement fails closed.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** Context Continuity/cache invalidation, Provider hierarchy, Adaptive Model Router hard gates, G6 Runner/Policy; `INV-019`, `INV-022`, `INV-023`, `INV-060`, `INV-066`.

## P-022 — Re-profile the full path after removing the dominant cost

**Pattern:** After an optimization bounds or removes one dominant stage, re-measure the entire execution path because selector/index/ranking/coordination overhead can become the new bottleneck.

**Source repository:** `Tencent-Hunyuan/Simple-Attention-Sparsification`

**Source revision:** `c7d7101bbf5d52edf89f6b02de86f6a0fbde4d6e`; linked paper `arXiv:2609.13141v1`.

**Evidence:** sparse decode reads a fixed selected block budget, while block scoring and Top-K still scan/rank candidates (`src/sas/core/attn/simple_sparse_attention.py:471-524`, `src/sas/core/kernels/block_scoring.py`). Paper §6.3 reports Top-K growing to about 90% of sparse decode cost at 512K and identifies selection as the next optimization target.

**Problem solved:** A successful micro-optimization can produce misleading efficiency claims when its support machinery now dominates end-to-end latency/cost.

**Mechanism:** stage-level profiling plus end-to-end quality/cost measurement under the real workload; promote only when total outcome improves.

**Tradeoffs:** More benchmark dimensions and provenance are required; bottlenecks can change across hardware, batch, context, runtime, and workload.

**Runlith status:** `ALREADY_DESIGNED` through `EffectiveContextCost`, `VerifiedDeliveryCost`, `Token Cost` vs `Attention Load`, Efficiency Observatory, and reproducible runtime profiling.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** Source of Truth `Execution Efficiency Architecture`, `Prompt caching`, Adaptive Model Router; `INV-063`, `INV-080`.

## P-023 — Delivery-state-aware recovery around ambiguous provider submission

**Pattern:** Persist whether an external submission is known unsent/rejected, known accepted, or outcome-unknown, and make retry legality depend on that durable delivery state.

**Source repository:** `alphaXiv/OpenResearch`

**Source revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/local/chat/mod.rs:2258-2281` maps `NotSent`/`Rejected` to retry and `Accepted`/`Unknown` to continue; `src/local/harness/claude.rs:1841-1847` persists `Unknown` before writing the user message; `src/commands/supervise.rs:59-77` refuses blind external-job resubmission when a provider handle cannot be recovered.

**Problem solved:** A transport/process crash can happen after an external system accepted work but before the caller persisted proof, so generic "request failed -> retry" can duplicate stateful effects.

**Mechanism:** durable intent/delivery classification, known-safe retry only for proven non-delivery, and reconciliation/inspection for uncertain outcome.

**Tradeoffs:** Adapters must classify transitions carefully and reconciliation still needs provider-specific evidence. `Unknown` preserves safety but can require slower inspection or human resolution.

**Runlith status:** `ALREADY_DESIGNED` by G8 Side-Effect Execution Protocol, `AMBIGUOUS -> RECONCILE`, `INV-066` and durable Attempt/Reconciler ownership.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** `planning/roadmap.yaml` G8; `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` Side-Effect Execution Protocol; `docs/architecture/events.md`; `INV-066`, `INV-091`.

## P-024 — Provider-native session fallback to application-owned bounded history

**Pattern:** Treat provider-native conversation/session state as a resumable optimization; when it disappears, reconstruct a new provider session from bounded application-owned durable history.

**Source repository:** `alphaXiv/OpenResearch`

**Source revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/local/harness/mod.rs:107-155` reconstructs a recovery snapshot from locally stored messages when native state is unavailable; `48-59` caps the snapshot and provider retry/watchdog behavior.

**Problem solved:** Provider session loss, migration, CLI corruption or cache eviction should not erase the application's durable continuity.

**Mechanism:** provider session reference remains replaceable; local transcript/state is rendered into a bounded recovery context for a fresh native session.

**Tradeoffs:** Reconstructed text cannot reproduce opaque hidden provider state exactly. Compatibility/invalidation rules are still needed before native resume itself is considered equivalent.

**Runlith status:** `ALREADY_DESIGNED`; the narrower unresolved native-resume compatibility/invalidation contract is already tracked independently as IKQ-002, so OpenResearch creates no new gap.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** ADR-0005, ADR-0011, `docs/architecture/continuity.md`, `docs/architecture/providers.md`, `INV-019`, `INV-022`, `INV-023`, `INV-110`.

## P-025 — Persist immutable source snapshot provenance for delayed/restarted execution

**Pattern:** Bind an external execution handle to an immutable source snapshot identity so delayed start or supervisor restart cannot silently switch to the current mutable working tree.

**Source repository:** `alphaXiv/OpenResearch`

**Source revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**Evidence:** `src/jobs/mod.rs:46-95` persists `source_digest`, `source_path` and `source_size` in the backend descriptor, explicitly to keep delayed/restarted supervisors independent of the working tree.

**Problem solved:** Long-running/remote execution can start or recover after local source changed, making provenance and reproducibility ambiguous.

**Mechanism:** materialize an immutable source artifact and persist its identity/provenance with the execution's provider handle.

**Tradeoffs:** Snapshot storage/cleanup and integrity verification cost; the snapshot identifier still has to be included in verification/evidence contracts.

**Runlith status:** `ALREADY_DESIGNED` through Workspace/Attempt revision identity, source/worktree fingerprints, execution receipts and verification evidence binding.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** `docs/architecture/workspace.md`, Context Engine, Attempt/Provider receipt architecture and source fingerprint invariants.

## P-026 — Explicit temporary ownership for user-controlled browser targets

**Pattern:** Treat control of a pre-existing user browser target as an explicit temporary capability with one owner, a recorded origin, bounded grant semantics, and compensating return/cleanup. Do not infer authority from window placement, focus or opener relationships.

**Source repository:** `Tencent/BrowserSkill`

**Source revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**Evidence:** `apps/extension/src/session-manager/manager.ts:159-213` reserves a tab before awaited borrow work and prevents cross-session double ownership; `apps/extension/src/tools/tabs.ts:631-911` validates, optionally confirms, moves, commits and rolls back the borrow; `apps/extension/src/tools/session.ts:173-193,225-358` returns borrowed user tabs before task-created cleanup and preserves the window when return fails; `docs/remote-extension-connection.md:162-168` states that mere placement/opener relationships do not grant control.

**Problem solved:** Browser automation can otherwise confuse application-created targets with pre-existing user state, operate on the wrong tab, or destroy user state during cleanup.

**Mechanism:** explicit ownership metadata; reservation before asynchronous mutation; scoped grant/approval; recorded original location; compensating return; unknown outcome when rollback cannot be proven; ownership-aware cleanup.

**Tradeoffs:** More lifecycle state and recovery logic are required. Physical browser state can still become ambiguous after crash or failed compensation, so restart reconciliation is necessary in a durable orchestrator.

**Runlith status:** `NEEDS_ADR`. Runlith already treats stateful browser sessions/cookies as scoped capabilities (`INV-099`) and Web Acquisition is a deferred architecture target, but no accepted browser-target ownership/borrow/return contract was found.

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** future `BrowserAutomationPort`; Web Acquisition ADR slot in `docs/architecture/adr/COVERAGE.md`; `INV-033`, `INV-099`, `INV-104`, `INV-105`, `INV-146`-`INV-153`.

**Runlith differentiation:** Bind target grants to durable Attempt/Policy/approval identity, keep adapter-specific tab IDs below the port, make ambiguous return/restart state reconcilable, and ensure device/browser identity never widens site/target authority by itself.

## P-027 — Bind actionable browser refs to observation/document identity

**Pattern:** An agent-visible element/reference handle is valid only for the observation/document/frame scope that produced it. Replace or invalidate refs when document identity changes and fail closed when the controller cannot prove the ref still addresses the observed document.

**Source repository:** `Tencent/BrowserSkill`

**Source revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**Evidence:** `apps/extension/src/session-manager/ref-store.ts:3-153` replaces the ref map on fresh observation, tracks generation, invalidates all refs for a tab when its document changes, and requires verified DOM identity for visual refs; `apps/extension/src/tools/vom/capture-coordinator.ts:237-338` verifies attachment/document-root identity before producing actionable observation data.

**Problem solved:** DOM/CDP node identities and short agent refs can become stale or be reused after navigation/frame replacement, causing an action intended for document A to hit an unrelated target in document B.

**Mechanism:** observation generation + tab/document/frame identity, explicit invalidation on document transition, and identity verification before action-capable refs enter the model-facing observation.

**Tradeoffs:** Highly dynamic pages may require more re-observation. Ref provenance must remain compact enough for model use while durable enough for action validation and receipts.

**Runlith status:** `PARTIALLY_EXISTS`. `INV-103` already requires provenance/freshness for web snapshots and Source of Truth defines browser action receipt target/ref hashes, but no explicit stale actionable-ref contract was found.

**Decision:** `ADAPT`.

**Relevant Runlith architecture:** Source of Truth Web/Browser §§12-13; `INV-103`, `INV-104`; future `BrowserAutomationPort` ADR.

**Runlith differentiation:** Make the ref contract provider-neutral and restart-safe by binding actions to immutable snapshot/document identity plus session/Attempt provenance, with stale or unprovable refs rejected by trusted application/adapter code.

## P-028 — Linearize cancellation with side-effect dispatch

**Pattern:** When cancellation races the transition from queued work to an external side-effect request, serialize the canonical cancel decision, dispatch transition and request publication so either cancellation prevents dispatch or the request is ordered before the cancellation signal.

**Source repository:** `Tencent/BrowserSkill`

**Source revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**Evidence:** `crates/bsk-cli/src/daemon/inflight.rs:91-169,211-306` puts cancellation state and `Queued -> Forwarded` promotion behind the same mutex and invokes the dispatch closure while holding that critical section; cancel-first never publishes a browser request, while promote-first establishes request-before-cancel wire ordering.

**Problem solved:** Without one serialization boundary, a caller can observe a successful cancellation while a concurrent worker still emits the side-effecting request.

**Mechanism:** shared canonical critical section for cancel state, forwarding correlation and request publication; after dispatch, uncertainty is handled separately instead of pretending cancellation undid the effect.

**Tradeoffs:** The dispatch closure must stay small/non-blocking enough for the critical section, and post-dispatch cancellation still needs effect-state reconciliation.

**Runlith status:** `ALREADY_DESIGNED` through governed side-effect execution, bounded cancellation/retry, ambiguous-outcome reconciliation and enforcement at the privileged operation boundary.

**Decision:** `EXTERNAL_VALIDATION`.

**Relevant Runlith architecture:** G8 Side-Effect Execution Protocol; `INV-066`, `INV-091`, `INV-145`, `INV-153`.
