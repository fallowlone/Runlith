# Recurrent Looped Transformer

## Source revision

- repository: `recurrent-looped-tranformer`
- repository_url: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer`
- default_branch: `master`
- reviewed_revision: `master@9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- commit_sha: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- commit_date: `2026-09-13 13:46:51 -0700`
- commit_subject: `docs: rename preliminary synthetic experiments section`
- tag_version: none
- review_date: `2026-09-15`

The review used a fresh clone and detached checkout of the exact commit above. The repository contains a technical report, a static project page, README, figures, and license. It does **not** contain the model implementation, an application runtime, manifests, tests, CI, persistence, provider integrations, migrations, deployment configuration, or an agent orchestration system. Claims about executable behavior are therefore limited to behavior formally specified in the report; runtime/security implementation claims are not inferred from absent code.

## Executive summary

The strongest Runlith-relevant contribution is not the proposed recurrent model architecture itself. It is the paper's unusually explicit treatment of **state compatibility, replay, and stale cached state**. RLT specifies that an exact reusable prefix state is a compound object whose validity depends on the model parameters, consumed prefix, positions, initialization, attention-window semantics, and execution settings. When those conditions change, old hidden/KV state is stale and exact behavior requires reconstruction from a compatible checkpoint or replay.

That maps cleanly to Runlith's existing principle that provider-native context/session state is an optimization rather than durable application truth. Runlith already has the stronger high-level architecture: provider-independent checkpoints, explicit cache invalidation, durable Attempt recovery, no blind side-effect replay, and provider/model neutrality. The reviewed repository therefore validates much more than it adds.

One actual gap remains. Runlith explicitly leaves `provider migration resume protocol`, `checkpoint version compatibility`, `provider Session ↔ Epoch linkage`, and parts of provider version/configuration identity open. The current Claude adapter also exposes opaque `--resume` capability/configuration. There is no accepted contract yet defining when a provider-native session/snapshot may be reused versus when it must be invalidated and rebuilt from Runlith-owned continuity. RLT provides good external evidence for making that compatibility decision explicit. Classification: `NEEDS_ADR`; decision: `ADAPT`; natural owner: G9 Context Engine + Continuity with the Provider boundary.

The repository does not support application-security findings such as shell injection, path traversal, credential leakage, SSRF, IPC authorization bypass, or sandbox escape because there is no executable application/runtime path to inspect. Those categories are classified `NOT_SUPPORTED`, not silently treated as safe.

The synthetic results also provide useful negative evidence against equating a longer latent/recurrent computation path with reliable long-horizon reasoning. The small RLT model fits the training horizon but degrades substantially at 2× and 4× sequence lengths. That externally validates Runlith's decision to use bounded decomposition, durable checkpoints, independent verification, and benchmark-gated adaptive planning rather than assuming that a model's internal recurrence or context length solves long-running engineering work.

## Project purpose

RLT is a research architecture for causal language modeling. A causal encoder constructs prefix-restricted global KV memory, while a recurrent decoder carries a continuous hidden output and layer-local sliding-window KV state across every prompt and response token. The report co-designs this transition with training, inference, hardware scheduling, and RL replay semantics.

The repository is a publication artifact rather than a software product. Its primary engineering value for Runlith is in the precision of its state/replay contracts and the limitations it states explicitly.

## Architecture map

```text
Repository
├── Interfaces
│   ├── README.md
│   ├── index.html static project page
│   └── English / Chinese technical-report PDFs
├── Orchestration
│   └── none: no agent/workflow orchestrator
├── Domain
│   └── mathematical RLT transition and training/RL semantics
├── State/Persistence
│   ├── recurrent decoder output
│   ├── decoder SWA KV state
│   ├── encoder continuation/cache state
│   ├── encoder-derived memory
│   └── execution metadata; no durable application database
├── Providers
│   └── none
├── Tools/Capabilities
│   └── none
├── Execution
│   ├── causal encoder work over known tokens
│   ├── ordered recurrent decoder transition per consumed token
│   └── batching across independent sequences
├── Recovery
│   ├── replay/reconstruction rules specified in the paper
│   └── no executable crash/restart recovery implementation
├── Security
│   └── no application security subsystem in repository
└── Observability
    ├── equations / execution schedules
    └── preliminary synthetic experiment figure/results
```

### Repository-level architecture answers

| Question | Reviewed evidence / conclusion |
|---|---|
| Where is business logic? | There is no application business layer. The core artifact is the mathematical model transition in the report. |
| Who owns lifecycle? | No Task/job/application lifecycle exists. Sequence/model execution follows the specified recurrent transition. |
| Where is canonical state? | For the model computation, the report defines a complete recurrent decoder state and separately required encoder/cache/metadata state. There is no durable software state owner. |
| Jobs/tasks/state machine/scheduler? | Absent. The recurrence is a model transition, not an application workflow state machine. |
| Recovery/retries/crash restart? | Exact reconstruction and snapshot compatibility are specified; process crash/restart persistence and retries are not implemented in this repository. |
| Concurrency? | Ordered recurrence inside one sequence; independent ready sequence transitions may be batched. |
| Atomic boundaries? | No persistent transaction boundary exists. |
| Duplicate mutable truth? | No application persistence exists to assess. The paper explicitly distinguishes behavior probabilities from current-policy state, which helps prevent semantic conflation. |
| Versioned schemas/contracts? | No application schemas. The report requires model/execution compatibility for cached-state reuse. |
| Provider abstraction? | None. |
| Policy/authorization/capabilities? | None. |
| Execution receipts/audit/logging? | None. |
| UI/API vs domain? | The static project page presents the research and has no runtime domain authority. |

## Canonical state ownership

RLT's useful notion of "complete state" is computation-local, not a durable application state model. The decoder state contains the recurrent output plus every decoder layer's retained SWA KV. Exact continuation additionally depends on encoder continuation/cache state, encoder-derived memory, consumed token prefix, position metadata, window semantics, model version/parameters, and relevant execution conventions.

For Runlith this is a **compatibility lesson**, not a candidate canonical state representation. Runlith already defines the opposite ownership boundary for application durability: provider sessions/native context remain temporary and non-authoritative; Task/Execution/Attempt, Context Continuity, Event Journal, Policy, worktree state, and verification evidence remain product-owned.

The safe translation is:

```text
RLT compatible model snapshot
  -> evidence for strict cache/resume compatibility

Runlith provider-native session/cache
  -> optional L1 optimization
  -> never Task / Execution / Context Continuity truth
  -> invalidate on compatibility mismatch
  -> reconstruct from Runlith-owned L2 continuity
```

## Strong engineering decisions

### RLT-F01 — Complete-state compatibility is explicit

**finding_id:** `RLT-F01`

**source:**

- repository: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer`
- revision: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- file: `Recurrent_Looped_Transformer.pdf`
- symbol: `§6 Multi-turn Serving and Cache Semantics`; `Appendix C Exactness of Cached States`
- region: pages 10, 18-19

**observation:** A reusable prefix snapshot is valid only when the complete execution state is compatible with the current model parameters/version, consumed token prefix, positions, initialization, attention-window semantics, and execution/policy conventions. Editing/truncating prior history or changing weights invalidates exact reuse and requires reconstruction from a valid earlier point.

**confidence:** `HIGH`

**Problem:** Opaque cached state can appear resumable while encoding assumptions that are no longer true.

**Mechanism:** Treat reuse as a compatibility predicate over all state that can affect the next transition; replay/reconstruct on mismatch.

**Why it works:** It makes hidden state an explicitly derived optimization with identifiable validity conditions instead of assuming that possession of a session/cache reference proves correctness.

**Limitations:** The paper defines model-computation exactness, not Runlith workflow durability, authorization, filesystem side effects, or cross-provider semantics.

**Failure modes:** Partial compatibility checks can retain stale conditioning; version-only checks can miss edited history/configuration; text-only restore loses hidden state and therefore needs replay.

**Complexity:** Compatibility metadata and invalidation are cheap relative to provider execution, while exact replay can be expensive for long histories.

**Runlith overlap:** General invalidation/fingerprint principles are already designed. The exact provider-native resume compatibility contract is open in `docs/architecture/continuity.md` and `docs/architecture/providers.md`.

**Runlith status:** `NEEDS_ADR` for provider-native resume reuse; `ALREADY_DESIGNED` for general cache invalidation.

**Decision:** `ADAPT`.

### RLT-F02 — Sampling/training/replay share one state-transition meaning

**finding_id:** `RLT-F02`

**source:**

- repository: same as above
- revision: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- file: `README.md`; `Recurrent_Looped_Transformer.pdf`
- symbol: `One execution across training and inference`; `§3.1`; `§5.3`
- region: `README.md:47-57`; paper pages 5, 8-10

**observation:** Prompt processing, generated-token consumption, SFT, and current-policy replay preserve one transition meaning. The report warns that matching a high-level transition does not itself prove numerical equivalence if kernels, precision, stochastic choices, or cache conventions differ.

**confidence:** `HIGH`

**Problem:** Different paths for "equivalent" work can silently diverge in hidden state or semantics.

**Mechanism:** Define one reference transition and make optimized execution preserve it; record the execution details that can break equivalence.

**Why it works:** Optimizations become testable against a reference meaning instead of becoming a second implicit semantics.

**Limitations:** Runlith does not train the provider model, so BPTT/RL details do not transfer directly.

**Failure modes:** Treating provider-native resume, compacted sessions, or renderer changes as equivalent without compatibility evidence can reintroduce the same semantic drift at the orchestration boundary.

**Complexity:** Mostly contract/verification complexity rather than a new subsystem.

**Runlith overlap:** Provider anti-corruption, execution contract suites, versioned RoutePlan/context semantics, and provider-output distrust already establish this principle at the product layer.

**Runlith status:** `ALREADY_DESIGNED` / current provider seam partly `ALREADY_IMPLEMENTED`.

**Decision:** `EXTERNAL_VALIDATION`.

### RLT-F03 — Stale parameter-dependent state is rejected, while historical behavior evidence is retained

**finding_id:** `RLT-F03`

**source:**

- repository: same as above
- revision: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- file: `Recurrent_Looped_Transformer.pdf`
- symbol: `§5.3 Model-RL algorithm co-design`; `§5.4 Gradients, checkpointing, and state staleness`
- region: pages 9-10

**observation:** After parameters change, old encoder/recurrent/SWA state cannot stand in for current-policy replay, while recorded behavior probabilities remain evidence of the policy that actually sampled earlier actions. The report therefore separates stale derived state from historical provenance that remains meaningful.

**confidence:** `HIGH`

**Problem:** Systems often either over-trust stale derived state or throw away historical evidence when recalculating current state.

**Mechanism:** Invalidate parameter-dependent derived state but preserve immutable provenance about the original action-generation process.

**Why it works:** Current computation and historical evidence have distinct owners and freshness semantics.

**Limitations:** Behavior log-probabilities are RL-specific; Runlith needs its own execution/evidence vocabulary.

**Failure modes:** Rewriting historical evidence during recomputation destroys auditability; keeping stale state as current produces incorrect continuation.

**Complexity:** Requires explicit provenance and version/fingerprint identity.

**Runlith overlap:** Event Journal/receipts/history are evidence; caches/projections are derived; provider failure recovery creates/resumes a durable Attempt rather than replaying effects blindly.

**Runlith status:** `ALREADY_DESIGNED`.

**Decision:** `EXTERNAL_VALIDATION`.

### RLT-F04 — Parallelism is placed around the ordered dependency chain

**finding_id:** `RLT-F04`

**source:**

- repository: same as above
- revision: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- file: `Recurrent_Looped_Transformer.pdf`
- symbol: `§4.1 Parallel work around a recurrent core`
- region: pages 6-7

**observation:** Decoder updates inside one sequence remain ordered, while ready updates from independent sequences may be batched. The paper does not pretend that an internal dependency disappeared merely because surrounding work can run in parallel.

**confidence:** `HIGH`

**Problem:** Aggressive parallelization can violate causal/state dependencies or force hidden synchronization.

**Mechanism:** Preserve local ordering where state depends on the previous transition; extract concurrency only across independent work.

**Why it works:** Correctness boundary stays explicit while throughput can scale with independent units.

**Limitations:** This is a model/hardware schedule; Runlith scheduler dependencies and side effects are a different domain.

**Failure modes:** Small/uneven batches can underutilize hardware; falsely parallelizing the recurrent core changes semantics.

**Complexity:** Scheduling/batching logic must know independence and readiness.

**Runlith overlap:** Durable DAG dependencies, Scheduler/Reconciler readiness, separate Attempts, and bounded parallel work already encode the analogous product-level rule.

**Runlith status:** `ALREADY_DESIGNED`, with current G7 implementation in progress.

**Decision:** `EXTERNAL_VALIDATION`.

### RLT-F05 — Resume cursor semantics prevent skipped or duplicate consumption

**finding_id:** `RLT-F05`

**source:**

- repository: same as above
- revision: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- file: `Recurrent_Looped_Transformer.pdf`
- symbol: `Appendix A.2 Generation and new external inputs`
- region: page 16

**observation:** If a generated token has been emitted but has not yet been incorporated into the recurrent state, the cache API must represent that pending token separately and consume it exactly once before later input. The paper calls out double/skip prevention as part of resume correctness.

**confidence:** `HIGH`

**Problem:** A crash/resume boundary can make an external effect or transition outcome ambiguous: was it emitted/applied/consumed already?

**Mechanism:** Persist an explicit progress boundary rather than infer it from an incomplete cache.

**Why it works:** Resume is driven by recorded transition position rather than best-effort repetition.

**Limitations:** A token-state cursor is much simpler than filesystem/network/tool side effects.

**Failure modes:** Assuming idempotency can duplicate effects; assuming completion can skip required processing.

**Complexity:** Small for token processing; Runlith's external side effects require stronger durable intent and reconciliation.

**Runlith overlap:** G8 already plans a Side-Effect Execution Protocol with durable intent, `AMBIGUOUS` recovery, and `RECONCILE` before retry; `INV-066` already prohibits transparent stateful replay.

**Runlith status:** `ALREADY_DESIGNED`.

**Decision:** `EXTERNAL_VALIDATION`.

### RLT-F06 — Longer latent paths are not evidence of reliable long-horizon reasoning

**finding_id:** `RLT-F06`

**source:**

- repository: same as above
- revision: `9c40dec78a6d7da73e1226310fd6c8edaa32577e`
- file: `README.md`; `Recurrent_Looped_Transformer.pdf`
- symbol: `Preliminary synthetic experiments`; conclusion
- region: `README.md:59-85`; paper conclusion page 12

**observation:** The reported small-model state-tracking results are strong at the training horizon but degrade at longer horizons. At 128 operations the five-state task is near its 20% chance level; the report explicitly states that large-scale reasoning quality, hardware efficiency, and RL scaling still require validation.

**confidence:** `HIGH` for the reported experiment; `LOW` for extrapolation to large production models.

**Problem:** Architectural depth/context/state capacity can be mistaken for demonstrated reliability on long-running work.

**Mechanism:** Treat architectural capacity as a hypothesis and promote it only after representative benchmarks.

**Why it works:** It separates mechanism from measured outcome.

**Limitations:** Approximately 79K parameters, three seeds, synthetic tasks, unmatched FLOPs, and no large-scale coding benchmark.

**Failure modes:** Using this result to predict production LLM quality in either direction would overstate the evidence.

**Complexity:** Benchmark/evaluation cost rather than runtime architecture cost.

**Runlith overlap:** ADR-0017 already keeps adaptive planning benchmark-gated and bounded; independent verification remains completion authority.

**Runlith status:** `ALREADY_DESIGNED`.

**Decision:** `EXTERNAL_VALIDATION` for evidence discipline; `REJECT` any inference that provider-native recurrence/long context can replace durable orchestration and verification.

## Implementation techniques

The report describes several techniques but does not provide their code in this repository:

- parallel causal encoder work over known tokens while preserving ordered recurrent decoder updates;
- batching ready transitions across independent sequences;
- separate global encoder-derived KV and local decoder SWA KV;
- compatible weight sharing between encoder/decoder stages;
- activation checkpointing to trade recomputation for activation storage without intentionally truncating the reference gradient path;
- explicit cache invalidation after parameter updates;
- reconstructing from an exact compatible prefix checkpoint instead of trusting stale hidden state;
- explicit representation of a pending emitted-but-not-consumed token at resume boundaries.

These are report-level mechanisms. There is no source implementation here to review for memory ownership, thread safety, cache mutation races, numerical fidelity, or actual throughput.

## AI / orchestration architecture

This repository is about model architecture, not an AI-agent orchestration product.

| Area | Finding |
|---|---|
| Agent loop | none |
| Planner | none |
| Subagents | none |
| Tools/skills | none |
| Agent memory | none; model hidden/KV state is not durable agent memory |
| Prompt construction | token-history semantics discussed, no application prompt builder |
| Provider routing/model selection | none |
| Retry/fallback | none |
| Cost accounting | computational formulas/qualitative costs only; no product accounting |
| Tool results | external/user/tool tokens are modeled as observed context; no privileged tool runtime |
| Long-running execution | model state extends across token history; no durable Task engine |
| Human approval | none |
| Verification/completion | no application completion authority |

Answers to the mandatory agent questions:

- **Who owns workflow state?** No workflow exists. The model transition owns transient computational state only.
- **Who owns completion authority?** Not applicable; no Task/completion model exists.
- **Can the model mutate canonical application state?** No canonical application state is present.
- **Can model output directly cause privileged action?** No tool/privileged-action runtime exists in the reviewed repository.
- **What happens after crash/provider timeout?** No executable crash/provider recovery is implemented. The report specifies what an exact compatible snapshot/replay would require at the model-computation level.
- **How does execution resume?** Conceptually from compatible complete state or replay/reconstruction; there is no durable application resume implementation.

For Runlith, model-native recurrence should stay below the Provider anti-corruption boundary. It may improve provider quality or native-session efficiency, but it must not acquire Task lifecycle, Policy, recovery, or completion authority.

## Security analysis

The reviewed revision contains no server/runtime implementation, shell/process runner, filesystem API, credential handling, webhook receiver, provider API integration, IPC endpoint, serializer for untrusted persistent data, or sandbox. Therefore the requested vulnerability classes cannot be meaningfully proven or disproven as application behaviors.

| Class | Status | Evidence / reason |
|---|---|---|
| shell / argument injection | `NOT_SUPPORTED` | no command execution path |
| path traversal / symlink / filesystem authority | `NOT_SUPPORTED` | no filesystem runtime path |
| ambient credentials / environment / secrets exposure | `NOT_SUPPORTED` | no credential/runtime environment handling |
| network authority / SSRF / webhook trust | `NOT_SUPPORTED` | static project page only; no request-processing backend |
| provider trust | `NOT_SUPPORTED` | no provider adapter |
| unsafe deserialization | `NOT_SUPPORTED` | no application deserialization path |
| race / TOCTOU | `NOT_SUPPORTED` | no concurrent mutable application state implementation |
| IPC authentication / authorization bypass | `NOT_SUPPORTED` | no IPC |
| sandbox bypass | `NOT_SUPPORTED` | no sandbox/runtime executor |
| audit weaknesses / mutable security state | `NOT_SUPPORTED` | no product audit/security state |

The static `index.html` contains presentation/navigation and clipboard behavior. No reviewed path takes untrusted remote content and converts it into execution authority. This is not a security certification of a separate hosted infrastructure; it is only a statement about the repository revision.

## Reliability / recovery analysis

The most mature reliability concept in the report is **exactness of restored state**. It distinguishes four concerns that systems often collapse:

1. the token/history/effect that actually occurred;
2. historical evidence describing the behavior that produced it;
3. current derived/cached state that is valid only under a compatibility envelope;
4. replay/reconstruction when that envelope no longer matches.

That directly supports Runlith's existing separation among Event Journal/evidence, durable Task/Attempt state, derived Context/provider state, and recovery policy.

The report does not implement process crash durability, persistence transactions, idempotent external effects, retry budgets, distributed ownership, or recovery fencing. Runlith should therefore take the state-validity principle while retaining its own much stronger recovery model.

## Performance / scalability analysis

The architecture exposes meaningful optimization opportunities, but the reviewed revision is careful about what it has not measured:

- recurrent decoder dependencies remain sequential inside each sequence;
- independent sequences provide batching opportunities;
- known-token encoder work can be parallelized;
- weight/KV reuse may improve residency/memory traffic;
- attention work still grows with context length;
- full prompt recurrence has real compute cost;
- small batches or uneven lengths can reduce utilization;
- the report does not claim measured production latency/throughput gains;
- preliminary synthetic comparisons match parameter/data budgets but not FLOPs.

For Runlith the transferable performance rule is to optimize **around irreducible correctness dependencies**, then measure the result. This aligns with Verified Delivery Cost and the existing requirement that performance claims be reproducible rather than inferred from architecture.

## Anti-patterns

### RLT-A01 — Treating opaque provider-native state as durable execution truth

**Anti-pattern:** Persist or resume a provider session/hidden cache as though possession of the reference proves it is still semantically compatible with the current execution.

**Evidence:** RLT §5.4, §6, and Appendix C explicitly require reconstruction after incompatible weight/history/execution changes and specify a multi-part exact snapshot. This is a negative lesson derived from the report's safeguards; it is **not** a claim that RLT itself makes this mistake.

**Immediate impact:** continuation can be conditioned on stale/deleted/changed context; actions may be justified by state that no longer corresponds to current input/configuration.

**Long-term impact:** provider lock-in, unreproducible recovery, inability to explain why resumed behavior changed, and temptation to elevate a vendor session into product truth.

**Runlith lesson:** provider-native resume is a compatibility-gated optimization. If compatibility is not proven, create/recover the appropriate Attempt/Session and reconstruct context from product-owned continuity.

**Existing Runlith protection:** `INV-019`, `INV-022`, `INV-023`, `INV-066`, `INV-110`; `continuity.md` and `providers.md` already forbid provider-native state from becoming authoritative continuity.

### RLT-A02 — Equating latent temporal depth with reliable long-running work

**Anti-pattern:** Assume that a recurrent/long-context model can safely own a long-running engineering workflow because its internal computation path can extend across many tokens.

**Evidence:** README synthetic results degrade beyond the training horizon and the report explicitly leaves large-scale reasoning/scaling unvalidated.

**Immediate impact:** hidden no-progress loops or quality decay can be mistaken for durable reasoning.

**Long-term impact:** workflow correctness becomes coupled to a provider/model property that is hard to audit, migrate, or recover.

**Runlith lesson:** keep planning bounded, state durable outside the model, and completion independently verified.

**Existing Runlith protection:** ADR-0017 planning bounds/benchmark gate; `INV-009`, `INV-045`, `INV-050`, `INV-091`.

## Runlith overlap analysis

| External idea | External evidence | Closest Runlith equivalent | Runlith evidence | Current status | Actual gap |
|---|---|---|---|---|---|
| Complete compatibility predicate for cached/native state | RLT §6, Appendix C | Provider-independent Context Continuity + cache invalidation | `continuity.md:124-153,204-228`; `AGENT.md` cache/snapshot rules; SoT semantic cache policy | `NEEDS_ADR` for native resume; general rule `ALREADY_DESIGNED` | Exact provider-native resume compatibility/invalidation envelope is undecided |
| Shared transition/reference semantics | README 47-57; paper §3.1/§5.3 | Provider ACL + execution contract + versioned context/routing | `providers.md`; execution contract suite | `ALREADY_DESIGNED` / partly `ALREADY_IMPLEMENTED` | none |
| Invalidate stale derived state, retain immutable historical evidence | paper §5.4 | Event Journal/evidence vs caches/projections | `events.md`, `continuity.md`, `INV-070` | `ALREADY_DESIGNED` | none |
| Parallelism around ordered dependency chain | paper §4.1 | Task DAG + Scheduler/Reconciler readiness | ADR-0017/G7 contracts | `ALREADY_DESIGNED`, G7 in progress | none from this repo |
| Explicit pending transition at resume boundary | Appendix A.2 | G8 durable side-effect intent + AMBIGUOUS/RECONCILE | `planning/roadmap.yaml` G8; `INV-066` | `ALREADY_DESIGNED` | none attributable to RLT |
| Long horizon requires empirical proof | README 59-85, conclusion | benchmark-gated adaptive planning + independent verification | ADR-0017; verification architecture | `ALREADY_DESIGNED` | none |

The narrow gap is supported by Runlith's own open-design list rather than inferred from absence alone: `continuity.md` explicitly leaves provider migration resume protocol, checkpoint version compatibility, provider Session ↔ Epoch linkage, and checkpoint supersession semantics undecided. `providers.md` likewise leaves session identity/version/configuration details open. The Claude adapter currently has an opaque `--resume` capability/config field, but the reviewed production orchestration does not promote that opaque reference into durable truth or define a general compatibility gate.

## External validation of existing Runlith design

1. **Provider-native context must remain non-authoritative.** RLT's exactness conditions show why an opaque hidden/cache state cannot replace application-owned continuity.
2. **Caches need explicit invalidation semantics.** RLT provides a concrete model-level example of stale state after parameter/history/execution changes.
3. **Historical evidence and current state are different things.** Rebuilding current state need not erase evidence describing earlier behavior.
4. **Resume needs an explicit progress boundary.** The pending-token example independently validates Runlith's G8 design for durable intent and ambiguous-effect reconciliation.
5. **Parallelism should respect state dependencies.** Independent units can batch; dependent transitions stay ordered.
6. **Model capability is not delivery evidence.** The synthetic degradation and report caveats support Runlith's benchmark-gated planning and independent verification.

## Novel gaps

### RLT-G01 — Provider-native resume compatibility and invalidation contract

**Classification:** `NEEDS_ADR`

**External evidence:** RLT §6 and Appendix C define exact cached-state reuse as compatibility across the complete state, current parameters/model version, consumed history, positional state, initialization, window semantics, and execution settings. §5.4 rejects old parameter-dependent hidden/KV state as a substitute for current replay.

**Closest Runlith equivalent:** L1 provider context/native session + L2 provider-independent Context Continuity.

**Runlith evidence:**

- `docs/architecture/continuity.md:66-73` keeps native context/session behavior as L1 optimization and L2 continuity application-owned.
- `docs/architecture/continuity.md:144-153` makes provider migration independent of native session truth.
- `docs/architecture/continuity.md:204-228` explicitly leaves provider migration resume protocol, provider Session ↔ Epoch linkage, checkpoint version compatibility and supersession open.
- `docs/architecture/providers.md:146-158` makes provider Session temporary/non-authoritative.
- `adapters/provider-claude/src/config.rs` and `argv.rs` expose optional opaque `--resume`, but no general durable compatibility contract was found.

**Actual gap:** Before Runlith uses provider-native resume as an orchestration optimization, it needs an accepted rule for when opaque provider session/native cached state is reusable and when it is discarded. The compatibility envelope should be provider-neutral in semantics while allowing adapter-specific evidence. Candidate inputs include provider/runtime/model identity/version, provider-native configuration/capabilities, rendered-context/history fingerprint, relevant tool/context contract version, and any provider-specific state-generation convention required to establish safe reuse. Policy/approval state must be revalidated independently; a native session can never carry authorization forward by itself.

**Differentiation test — why implement it?** Native session resume can reduce latency/context reconstruction cost, but without a compatibility contract it undermines the exact provider-neutral continuity that differentiates Runlith. The feature is useful only when optimization cannot silently weaken recovery correctness.

**Why prefer Runlith's version?** Runlith can make resume provider-neutral, auditable, fail-closed on mismatch, and recoverable through application-owned checkpoints. RLT's concept is model-local; Runlith adds durable Attempt semantics, Policy, side-effect reconciliation, cross-provider fallback, and independent verification.

**Improvement dimensions:** `safer`, `more durable`, `provider-neutral`, `more recoverable`, `better auditability`.

**Decision:** `ADAPT`.

## Adoption matrix

| Finding | Decision | Runlith status | Differentiated Runlith outcome |
|---|---|---|---|
| Complete-state compatibility / native resume invalidation | `ADAPT` | `NEEDS_ADR` | provider-neutral compatibility gate + durable fallback + Policy revalidation |
| One reference transition / execution fidelity | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | provider contract/version/evidence seams already stronger at product layer |
| Stale-derived-state invalidation with retained history | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | Event Journal/provenance survives cache reconstruction |
| Parallel independent work around ordered state chain | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | durable DAG/readiness rather than model-local schedule |
| Explicit pending transition on resume | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | G8 durable side-effect intent + reconciliation |
| Recurrent depth as long-horizon solution | `REJECT` as orchestration principle | `CONFLICTS_WITH_ARCHITECTURE` | durable checkpoints + bounded planning + independent verification |

## Ideas rejected for Runlith

- Persisting provider hidden/KV state as canonical Context Continuity.
- Treating provider session resume as sufficient recovery proof.
- Treating a longer recurrent path or larger context as a replacement for durable Task/Attempt state.
- Using model-native hidden state to carry Policy/approval/side-effect authority across resume.
- Adding RLT as a Runlith dependency or copying model implementation from an external source; this repository is research evidence only.
- Creating immediate G7 implementation scope from this research. The gap belongs to G9/provider-continuity design.
- Claiming RLT performance gains for Runlith or production LLM workloads from the small synthetic experiments.

## Candidate ADRs

### Candidate: Provider-native Resume Compatibility and Invalidation Contract

**Type:** `ADR`

**Problem:** Runlith permits provider-native context/session continuation as an optimization, and at least one adapter exposes an opaque resume handle. An opaque handle alone cannot prove that native state still corresponds to the current provider/runtime/model configuration, rendered history/context, execution conventions, or continuation boundary.

**Current Runlith state:** L1 native context is non-authoritative; L2 continuity is product-owned; general cache invalidation exists; exact provider migration resume/version-compatibility/session-linkage semantics remain open.

**External inspiration:** RLT's exact cached-prefix contract and stale-state invalidation rules.

**Actual delta:** Define the provider-neutral semantic compatibility envelope, adapter evidence contract, invalidation triggers, fail-closed behavior, and fallback reconstruction path for native resume.

**Target subsystem:** `Context Continuity` + `Provider Adapter` boundary.

**Dependencies:** G9 checkpoint schema/version compatibility, provider Session ↔ Epoch linkage, provider capability/version evidence; existing Task/Execution/Attempt and Policy boundaries remain unchanged.

**Architecture impact:** moderate contract impact; no new canonical state owner. Native state remains derived/optional.

**Security impact:** native session state must not preserve revoked/changed authorization; Policy/approval/effective capability is re-evaluated at the current operation boundary. Mismatch must not fall back to unsafe blind continuation.

**Acceptance direction:** compatibility decision is deterministic/explainable; every reused native session names the evidence/fingerprints that made it eligible; every mismatch produces a typed invalidate/rebuild path; provider switch works without native state; native state never becomes Task/Memory/Event/Policy truth.

**Verification direction:** contract tests vary provider/runtime/model version, context/history fingerprint, provider config/capabilities, provider switch, edited/truncated history, checkpoint version, and policy state. Mismatches must reject native reuse while preserving provider-independent continuation. Side effects must never replay merely because context/session was reconstructed.

**Suggested gate:** `G9 — Context Engine + Continuity`, before native-session resume becomes a production orchestration optimization.

**Priority:** `HIGH when G9/native resume is activated`; no current G7 priority change.

## Candidate implementation tasks

### Candidate: Provider-native resume compatibility gate

**Type:** `TASK-CANDIDATE`

**Problem:** Native provider session reuse needs an executable compatibility/invalidation gate once the ADR and G9 checkpoint/session-linkage contracts exist.

**Current Runlith state:** optional Claude `--resume` is adapter-local/opaque; no canonical orchestration resume path is being introduced by this research.

**External inspiration:** RLT complete-state compatibility and replay fallback.

**Actual delta:** Implement only the accepted ADR predicate/evidence/fallback contract; do not persist provider hidden state as canonical continuity.

**Target subsystem:** G9 continuity application services + provider capability/adaptation seam.

**Dependencies:** accepted candidate ADR, G9 checkpoint schema/versioning, provider Session ↔ Epoch linkage.

**Architecture impact:** derived optimization gate only; Task/Execution/Attempt semantics unchanged.

**Security impact:** fail closed on stale/incompatible native state; authorization remains Policy-owned and is revalidated.

**Acceptance direction:** compatible native state may be reused; each individually incompatible fingerprint dimension rejects reuse; reconstruction from Runlith-owned checkpoint/context remains functional; provider migration succeeds without the native handle.

**Verification direction:** deterministic provider fixtures covering compatible resume, model/runtime/config drift, changed history, superseded checkpoint, provider switch, and native-session disappearance; assert no blind tool/filesystem side-effect replay.

**Suggested gate:** `G9`.

**Priority:** `DEFERRED until ADR accepted and G9 active`.

No canonical `TASK-XXXX` is assigned by this research.

## Source evidence index

All repository links below pin the reviewed revision.

- README architecture and state: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer/blob/9c40dec78a6d7da73e1226310fd6c8edaa32577e/README.md#L17-L57`
- README preliminary synthetic results and limitations: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer/blob/9c40dec78a6d7da73e1226310fd6c8edaa32577e/README.md#L59-L85`
- Technical report: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer/blob/9c40dec78a6d7da73e1226310fd6c8edaa32577e/Recurrent_Looped_Transformer.pdf`
  - §2 Method, especially §2.1-2.4: complete state and one transition across prompt/response.
  - §3.1-3.3: prompt-response consistency, work/depth properties.
  - §4.1-4.3: batching around sequential recurrence, memory reuse, checkpointing/execution fidelity.
  - §5.3-5.4, pages 9-10: current-policy replay and stale-state invalidation.
  - §6, page 10: exact prefix snapshot and multi-turn cache semantics.
  - Appendix A.2, page 16: pending-token resume boundary.
  - Appendix A.4, page 17: replay schedule/invalidation.
  - Appendix C, pages 18-19: exact cached-state compatibility conditions.
- Static project page: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer/blob/9c40dec78a6d7da73e1226310fd6c8edaa32577e/index.html`
- Repository tree at reviewed revision: `https://github.com/yifanzhang-pro/recurrent-looped-tranformer/tree/9c40dec78a6d7da73e1226310fd6c8edaa32577e`

No source code from the external repository was copied into Runlith. Equations/implementation pseudocode are intentionally not reproduced as Runlith implementation artifacts; the report records concepts, evidence locations, decisions, and gaps only.
