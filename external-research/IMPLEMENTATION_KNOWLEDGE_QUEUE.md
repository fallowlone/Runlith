# Implementation Knowledge Queue

This file contains only evidence-backed Runlith deltas discovered through external research. It is not the canonical TASK queue. `ALREADY_DESIGNED`, `ALREADY_IMPLEMENTED`, `SUPERSEDED` and rejected ideas do not belong here.

## IKQ-001 — Generic Reconciler work ownership and stale-writer fencing

**id:** `IKQ-001`

**origin_repository:** `https://github.com/Nitjsefnie/Overflow`

**origin_revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**idea:** Define a source-neutral durable work-ownership contract for future reconciliation of external lifecycle signals: bounded claim/lease renewal, retry/defer semantics, generation/version, idempotent recovery, and stale-writer fencing on every authoritative publication.

**novelty:** Runlith already owns Task readiness/recovery in Scheduler/Reconciler, external signals are already evidence routed to Journal/Reconciler, and G4/G5 already implement narrow workspace/execution reconciliation. The unresolved delta is the generic durable worker/delivery/concurrent-writer contract for external lifecycle signals.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- External lifecycle signals are observations/evidence and feed Event Journal/Reconciler.
- Task lifecycle assigns claim/retry/readiness transitions to Scheduler/Reconciler through the Task Application Service.
- Workspace and pre-attempt execution crash recovery are implemented with explicit ownership/reconciliation semantics.
- Event architecture explicitly leaves exact external-signal mappings, delivery semantics and concurrent-writer mechanism open.

**files:**

- `docs/architecture/events.md`
- `docs/architecture/task-engine.md`
- `docs/architecture/workspace.md`
- `docs/architecture/providers.md`

**adrs:**

- `ADR-0006` State + Event Journal
- `ADR-0018` Independent Evaluation
- `ADR-0023` Workspace Ownership and Git Worktree Lifecycle
- `ADR-0025` G5 Post-Admission Recovery

**invariants:**

- `INV-001`
- `INV-066`
- `INV-070`
- `INV-091`
- `INV-145`

**tasks:** existing narrow implementation/recovery evidence includes `TASK-0041/0042/0043` and `TASK-0054/0056/0057/0058`; no canonical TASK exists or is proposed here for a generic external-signal reconciler contract.

**delta:** No accepted generic contract currently defines normalized external reconciliation work identity, durable claim generation, lease expiry/reclaim, retry/defer budgets, stale-writer fencing, or the publication condition that prevents a superseded worker from mutating authoritative state. Overflow demonstrates useful bounded lease/retry mechanics but explicitly documents that its described mechanisms do not fence writes after session loss (`src/lib/fold/reconciliation-worker.ts:127-134`).

**decision:** `ADAPT`

**target_subsystem:** `Scheduler / Reconciler`, with Event Journal/external-signal ingestion and application-service publication boundaries.

**priority:** `MEDIUM`

**architecture_impact:** High when the first active gate requires general external lifecycle reconciliation; low immediate impact because current G4/G5 recovery is already scoped and functional.

**security_impact:** A stale worker must not retain mutation authority after claim generation/lease ownership moves. The fence must be checked at the authoritative publication boundary, not only during queue bookkeeping.

**dependencies:**

- An active product/gate requirement for generic external lifecycle reconciliation.
- Event identity/provenance contract for the relevant external signal source.
- Existing Task/Application Service authority must remain the only route for canonical Task mutation.

**proposed_gate:** `UNASSIGNED` — assign only when a concrete external-lifecycle/reconciler feature enters an active gate.

**requires_adr:** `yes`

**candidate_task:** `none before ADR`. After an accepted ADR and active gate exist, derive the smallest implementation task from the decided contract rather than copying Overflow's worker.

**verification_direction:**

- Two workers race on the same reconciliation key; exactly one generation can publish canonical state.
- Lease expiry/reclaim while the first worker is still running; stale publication fails closed.
- Crash after claim, during evidence fetch, and immediately before/after publication; restart converges without blind side-effect replay.
- Retry/defer budget is finite and observable.
- Duplicate external deliveries coalesce/idempotently rederive without granting authority from delivery alone.
- Event Journal records/correlates evidence and outcomes without becoming a queue authority or side-effect replay log.

**one_percent_improvement:** Require durable stale-writer fencing from the first Runlith contract. This is a concrete reliability/security improvement over the reference behavior and preserves Runlith's product differentiation around durable, auditable recovery rather than merely retrying background jobs.

## IKQ-002 — Provider-native resume compatibility and invalidation

**id:** `IKQ-002`

**origin_repository:** `https://github.com/yifanzhang-pro/recurrent-looped-tranformer`

**origin_revision:** `9c40dec78a6d7da73e1226310fd6c8edaa32577e`

**idea:** Define when provider-native session/cache state may be reused as a performance/continuation optimization and when it must be invalidated and reconstructed from Runlith-owned continuity.

**novelty:** Runlith already defines provider-native context as non-authoritative, already requires explicit cache invalidation/fingerprints, and already forbids blind side-effect replay. The real unresolved delta is narrower: `continuity.md` explicitly leaves provider migration resume protocol, provider Session ↔ Epoch linkage, checkpoint version compatibility and supersession open; `providers.md` leaves provider session/version/configuration identity details open. The Claude adapter exposes optional opaque `--resume`, but no accepted general compatibility gate was found.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- L1 provider-native context/session behavior is optimization only.
- L2 execution continuity is application-owned and provider-independent.
- Task/Execution/Attempt state, Event Journal, Policy, worktree state and verification remain canonical outside provider sessions.
- Semantic cache policy already binds reusable results to authoritative context/task-plan/provider-model inputs where relevant.
- Stateful failover may not transparently replay filesystem/tool/network effects.

**files:**

- `docs/architecture/continuity.md`
- `docs/architecture/providers.md`
- `docs/architecture/context-engine.md`
- `AGENT.md`
- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`
- `adapters/provider-claude/src/config.rs` (existing adapter evidence only; no production change authorized by this queue item)
- `adapters/provider-claude/src/argv.rs` (existing adapter evidence only; no production change authorized by this queue item)

**adrs:**

- `ADR-0005` Task ≠ Execution ≠ Attempt ≠ Session
- `ADR-0009` Provider adapters as anti-corruption layer
- `ADR-0011` Provider-independent Context Continuity
- `ADR-0024` G5 provider execution seam

**invariants:**

- `INV-019`
- `INV-022`
- `INV-023`
- `INV-066`
- `INV-070`
- `INV-093`
- `INV-110`

**tasks:** `TASK-0013` materialized the baseline Context/Continuity contracts but intentionally left resume/version-compatibility mechanics open. No canonical G9 implementation task currently owns this delta, and this research does not modify the task queue.

**delta:** Define a provider-neutral semantic compatibility envelope for native resume, the adapter evidence needed to evaluate it, invalidation reasons, and deterministic fallback to a new/recovered provider Session rendered from Runlith-owned checkpoint/history. Candidate compatibility inputs include provider/runtime/model identity/version, provider-native configuration/capabilities, rendered context/history fingerprint, checkpoint/schema version, and provider-specific execution convention evidence. Policy/approval/effective capability must be checked independently and may never be inherited from the native session merely because resume succeeded.

**decision:** `ADAPT`

**target_subsystem:** `G9 Context Continuity + Provider Adapter boundary`.

**priority:** `HIGH when native resume/G9 is activated`; no current G7 reprioritization.

**architecture_impact:** Moderate contract/ADR impact. It must not create a second canonical continuity store or make provider session state durable Task truth.

**security_impact:** Fail closed on stale/incompatible native state and prevent old session state from carrying obsolete policy/configuration assumptions. Side-effect authorization/replay remains governed by Policy and recovery contracts, not context compatibility.

**dependencies:**

- Accepted ADR for native resume compatibility/invalidation.
- G9 checkpoint schema/version compatibility.
- Provider Session ↔ Epoch linkage and provider capability/version evidence.
- Existing Attempt recovery and Policy boundaries.

**proposed_gate:** `G9`.

**requires_adr:** `yes`

**candidate_task:** `TASK-CANDIDATE — Provider-native resume compatibility gate`, only after the ADR is accepted and G9 is active.

**verification_direction:**

- Compatible provider-native session evidence permits reuse without changing Task/Execution truth.
- Provider/runtime/model version drift rejects native reuse.
- Changed/truncated/superseded history or checkpoint rejects native reuse.
- Provider configuration/capability incompatibility rejects native reuse.
- Provider switch or missing native session reconstructs from Runlith-owned continuity without state loss.
- Policy/approval change is re-evaluated independently; native resume never grants authority.
- Reconstructing context never blindly replays filesystem, tool, network, or privileged side effects.

**runlith_improvement:** RLT demonstrates exactness inside one model architecture. Runlith can make the same principle provider-neutral, restart-durable, auditable, policy-safe, and recoverable across provider switches.

**one_percent_improvement:** A user can take the latency/cost benefit of a compatible native resume without making the provider session a single point of continuity or silently accepting stale state. When compatibility cannot be proven, Runlith falls back to its own durable checkpoint/history instead of guessing.

## Removed by the 2026-09-15 re-audit

The previous queue entries below were deleted because overlap analysis disproved novelty:

- Mercury "Capability Execution Evidence Contract" → `ALREADY_DESIGNED` by `INV-072` and Event/Provider receipt architecture → `EXTERNAL_VALIDATION`.
- Mercury "Skill Metadata Discovery Index" → `ALREADY_DESIGNED` by Context Engine progressive disclosure, `INV-028`, `SkillRegistryPort` target → `EXTERNAL_VALIDATION`.
- Overflow "Evidence-backed Task Completion" → `ALREADY_IMPLEMENTED` semantically by ADR-0018 + completion authority tests → `EXTERNAL_VALIDATION`.
- Overflow generic "Reconciliation Workflow for External Integrations" → too broad; existing Runlith reconciliation/recovery already covers substantial semantics. Only the narrower fencing/delivery gap above remains, and it is `NEEDS_ADR`.

## IKQ-003 — Provider failure-scope identity for fallback routing

**id:** `IKQ-003`

**origin_repository:** `https://github.com/NousResearch/hermes-agent`

**origin_revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**idea:** Define a provider-neutral failure-scope contract for the future Adaptive Model Router so observed failure invalidates only the backend identity surface justified by evidence (for example deployment/model, credential surface, endpoint/transport) and fallback stays explainable.

**novelty:** Runlith already designs provider-neutral `RoutePlan` selection, hard capability/security/privacy eligibility, explicit provider degradation and new/recovered Attempts for stateful failover. The actual unresolved delta is failure-scope identity: no accepted ADR defines how one failed call changes the eligibility/cooldown of candidate routes, and the Adaptive Model Router remains a deferred architecture target.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- `INV-059` keeps model/provider selection outside Task identity.
- `INV-060` applies hard eligibility gates before preference scoring.
- `INV-066` requires stateful failover to create a new/recovered Attempt and forbids blind side-effect replay.
- `INV-067` requires explainable/replayable routing through versioned RoutePlan inputs.
- `INV-085` makes provider availability/degradation explicit runtime state.
- G5 `ExecutionPort` intentionally contains no routing/retry semantics; provider routing remains future work.

**files:**

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` (Adaptive Model Router / RoutePlan)
- `docs/architecture/providers.md`
- `docs/architecture/INVARIANTS.md`
- `docs/architecture/adr/COVERAGE.md`

**invariants:**

- `INV-059`
- `INV-060`
- `INV-061`
- `INV-066`
- `INV-067`
- `INV-085`
- `INV-093`

**adrs:** no accepted Adaptive Model Router ADR yet; `ADR-0024` deliberately leaves routing/retry outside the G5 execution seam.

**tasks:** no canonical Adaptive Model Router implementation task currently owns this delta. This research does not modify `planning/tasks/queue.yaml`.

**actual_delta:** Define typed normalized failure evidence, backend identity dimensions, scope-specific invalidation/equality, cooldown provenance/expiry, unknown-failure behavior and interaction with RoutePlan history. A credential failure must not automatically invalidate an otherwise healthy endpoint/model; an endpoint outage must not be hidden by an alias; a model/deployment incompatibility must not poison unrelated deployments. Every fallback for stateful work still starts/reconstructs an Attempt and must reconcile ambiguous side effects before retry.

**decision:** `ADAPT`

**target_subsystem:** `Adaptive Model Router + Scheduler/Recovery + Provider failure normalization`.

**priority:** `HIGH when Adaptive Model Router work starts`; no current G7 reprioritization.

**dependencies:**

- accepted Adaptive Model Router failure-scope ADR;
- active routing/fallback product requirement;
- normalized provider failure evidence;
- existing Policy/capability/privacy eligibility gates;
- Attempt/recovery and G8 ambiguous-side-effect contracts.

**security_impact:** A fallback route must re-evaluate Policy/effective capabilities and may not inherit authority from a failed provider/session. Classification must never be allowed to bypass hard eligibility gates.

**architecture_impact:** Moderate. Failure scope belongs to routing/recovery evidence, not core Task identity and not provider-specific branches in domain code.

**candidate_task:** `TASK-CANDIDATE — Failure-scope-aware RoutePlan recovery/fallback`, only after the ADR is accepted and the router enters an active gate.

**verification_direction:**

- model/deployment failure skips only equivalent deployments;
- credential failure can rotate to an independent credential surface when Policy permits;
- endpoint failure avoids equivalent endpoint/base-url candidates, including aliases;
- unknown classifications are conservative and observable;
- cooldown expiry/re-entry is deterministic and provenance-bearing;
- hard capability/security/privacy gates always outrank fallback preference;
- every stateful fallback produces a new/recovered Attempt and never blindly replays filesystem/tool/network effects;
- RoutePlan/failure evidence is sufficient to explain and replay why each candidate was kept or skipped.

**runlith_improvement:** Hermes provides a useful local implementation pattern; Runlith can make it provider-neutral, durable, policy-aware, Attempt-aware and independently reconcilable for stateful side effects.

## IKQ-004 — Runtime blocker recurrence and retry-exhaustion escalation

**id:** `IKQ-004`

**origin_repository:** `https://github.com/NousResearch/hermes-agent`

**origin_revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**idea:** Define durable blocker identity/recurrence and bounded escalation semantics for runtime `BLOCKED -> RUNNING -> BLOCKED` loops, separately from provider/runner failure budgets and planning-recursion limits.

**novelty:** Runlith already requires planning loops to terminate and has G7 numeric planning guardrails. However, `docs/architecture/task-engine.md` explicitly leaves typed blocker taxonomy and retry-budget ownership/exhaustion unresolved. Hermes demonstrates that same-blocker recurrence needs its own durable counter and escalation path rather than being conflated with dispatcher failures.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- Task lifecycle already supports `RUNNING -> BLOCKED -> RUNNING` with explicit blocker/resolution evidence.
- `INV-050` forbids silent infinite planning loops.
- G7 implements bounded planning recursion/no-progress guardrails.
- G8 is planned to own verification/recovery and ambiguous-side-effect reconciliation.
- HumanDecisionRequest is the accepted durable human-attention primitive for bounded escalation in G7 planning scope.

**files:**

- `docs/architecture/task-engine.md`
- `docs/architecture/adr/ADR-0017-adaptive-planning.md`
- `docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md`
- `planning/roadmap.yaml` (G8 scope; read-only evidence)
- `docs/architecture/INVARIANTS.md`

**invariants:**

- `INV-001`
- `INV-044`
- `INV-047`
- `INV-050`
- `INV-066`
- `INV-091`

**adrs:** Task-domain blocker/retry semantics remain open; a new ADR is required before implementation.

**tasks:** no canonical G8 task currently materializes this runtime blocker recurrence contract. This research does not modify the task queue.

**actual_delta:** Define blocker taxonomy/identity, equality and supersession rules, durable recurrence accounting, reset conditions, budget ownership, and exhaustion destinations. Keep at least four failure classes separate unless the ADR explicitly combines them: repeated external blocker, provider/transport failure, runner/environment failure, and ambiguous side-effect outcome. Escalation must produce reason/evidence and may route to HumanDecisionRequest/triage/replan/failure according to policy; it never grants success or wider authority.

**decision:** `ADAPT`

**target_subsystem:** `Task Engine + Scheduler/Reconciler + G8 Recovery`, with HumanDecisionRequest integration if selected by the ADR.

**priority:** `MEDIUM-HIGH before autonomous recovery/retry expands`.

**dependencies:**

- accepted Task blocker/retry ADR;
- G8 recovery contract;
- clear interaction with HumanDecisionRequest and Verification;
- Event Journal evidence for blocker occurrence/resolution/exhaustion.

**security_impact:** Repeated failures must not weaken Policy, approval, scope, isolation or verification requirements. A model cannot escape the recurrence budget by relabeling semantically identical blockers without trusted normalization.

**architecture_impact:** High at Task/recovery semantics, but localized if blocker identity remains provider-neutral and lifecycle mutation stays behind trusted application services.

**candidate_task:** `TASK-CANDIDATE — Durable blocker recurrence accounting and escalation`, only after the ADR is accepted.

**verification_direction:**

- same normalized blocker across unblock/reblock increments durably across restart;
- a distinct blocker does not accidentally consume the same recurrence budget unless policy says it should;
- provider/runner failures remain separate from blocker recurrence accounting;
- stale/concurrent worker cannot increment or resolve the current owner's blocker incorrectly;
- exhaustion produces exactly one durable escalation outcome and cannot loop silently;
- manual/human resolution and supersession reset behavior follows the ADR exactly;
- ambiguous side effects reconcile before retry and never masquerade as a simple blocker retry.

**runlith_improvement:** Runlith can combine Hermes' loop breaker with stronger product-level authority: typed blocker evidence, durable application-owned lifecycle, bounded human attention, independent Verification and no model-controlled completion/escalation authority.

## IKQ-005 — Executable architecture fitness and source-derived contract catalogs

**id:** `IKQ-005`

**origin_repository:** `https://github.com/deepseek-ai/deepseek-harness`

**origin_revision:** `0d1f50007f9bca3f52b06e1c3074fa14d5fb0720`

**idea:** Implement Runlith's already-declared architecture-fitness invariant as deterministic source/Cargo-workspace checks, with source-derived dependency/architecture artifacts where they provide durable review value.

**novelty:** The architecture direction is not new: Source of Truth already calls for architecture dependency checks and crate/module fitness budgets, and `INV-081` makes the requirement explicit. The real gap is implementation. Targeted checkout search found the documentation but no architecture-fitness checker/script/CI gate and no canonical queue task owning it. DeepSeek Harness supplies mature implementation evidence that these checks can scale with a large modular repository.

**status:** `PARTIALLY_EXISTS`

**runlith_existing_state:**

- `INV-081` requires Rust crate/module growth to be monitored by architecture fitness checks.
- Source of Truth proposes CI dependency checks via `cargo metadata` or an equivalent checker.
- Provider/Runner/Policy/core dependency boundaries and replacement tests are already architectural requirements.
- No corresponding executable checker or generated architecture freshness gate was found in this checkout.

**files:**

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` — architecture fitness budgets/checks.
- `docs/architecture/INVARIANTS.md` — `INV-081`.
- Cargo workspace metadata and crate manifests as future checker input; no production modification is implied by this research item.

**invariants:**

- `INV-081`
- provider/core adapter replacement and dependency-direction invariants already documented elsewhere in architecture.

**adrs:** no new ADR required for the initial checker because the architectural direction is already accepted. A future rule that changes dependency ownership rather than enforcing existing ownership would require its normal architecture review/ADR.

**tasks:** no canonical task matching the architecture-fitness implementation was found in `planning/tasks/queue.yaml`; this research does not modify the canonical queue.

**actual_delta:** Build a small deterministic Rust-workspace fitness gate that reads `cargo metadata`/source structure, enforces explicitly documented forbidden dependency directions, monitors agreed module/crate size budgets with a reviewed exception mechanism, and optionally generates a dependency/architecture catalog whose committed copy can be checked for staleness. Keep the rule set small and tied to real Runlith authority boundaries rather than recreating DeepSeek's package-invariant framework.

**decision:** `ADAPT`

**target_subsystem:** repository engineering / architecture CI; no production runtime dependency.

**priority:** `MEDIUM`, rising before G9-G11/plugin/provider expansion increases repository topology.

**dependencies:**

- explicit mapping of existing architecture boundaries to machine-checkable dependency rules;
- initial crate/module growth thresholds and exception process;
- current Cargo workspace metadata.

**security_impact:** Preserves critical Policy/Runner/provider/core boundaries against accidental dependency drift. The checker itself must not become runtime authority.

**architecture_impact:** Positive enforcement of existing architecture. Generated artifacts must remain derived from source and must not become a second hand-maintained source of truth.

**candidate_task:** `TASK-CANDIDATE — Architecture fitness and source-derived contract gate`.

**verification_direction:**

- a fixture or temporary graph with an allowed dependency passes;
- a forbidden dependency direction fails with the owning rule and source crate identified;
- agreed crate/module growth threshold violations fail unless an explicit reviewed exception exists;
- generated architecture/catalog output is deterministic;
- check mode fails on a stale committed generated artifact;
- the current workspace passes without production behavior changes;
- checker execution is suitable for CI and has bounded runtime.

**runlith_improvement:** DeepSeek Harness proves source-derived architecture contracts can scale, but Runlith can keep the implementation much smaller and align it to its strongest differentiators: trusted application-service ownership, replaceable provider/runner adapters, Policy authority boundaries and explicit Rust bounded contexts.

## IKQ-006 — Provider-neutral hardware topology and contention evidence contract

**id:** `IKQ-006`

**origin_repository:** `https://huggingface.co/spaces/nanotron/ultrascale-playbook`

**origin_revision:** `c93890dd4b22356ee18ed9b863e6f2d0909729bc`

**idea:** Define how future Runlith resource adapters can describe hardware/worker locality, interconnect cost and shared-resource contention as provider-neutral, provenance-bearing evidence for placement decisions.

**novelty:** `NEEDS_ADR`. Runlith already defines `DeviceResourceManagerPort`, `ModelResidencyManagerPort`, `ModelRuntimeProfile`, hardware/backend/time provenance, capacity/feasibility gating and G12 worker identity/lifecycle. The missing delta is narrower: no accepted contract was found for resource-locality groups, link bandwidth/latency evidence, contention/interference observations, a topology fingerprint, or a provider-neutral placement-cost projection. Repository-wide search found only generic scheduling topology terms and no hardware-interconnect model in `crates/`, `adapters/`, `bins/`, accepted ADRs, roadmap or canonical task queue.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- `ModelRuntimeProfile` is bound to a concrete runtime/hardware fingerprint and already carries latency, throughput, memory, concurrency, reliability and confidence/timestamp evidence.
- `DeviceResourceManagerPort` owns platform resource observations without leaking platform-specific APIs into router business logic.
- `ModelResidencyManagerPort` owns loaded-model state and load/unload/eviction cost.
- Router hard gates already reject impossible resource requirements and preference scoring already consumes residency/load cost and measured runtime evidence.
- G12 already defines remote-worker identity, scoped credentials, controlled egress, lifecycle/health state, heartbeat, version negotiation and durable reconnect/reconciliation.

**files:**

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` — Adaptive Model Router, Model Runtime Profile, Device Resource Management, G12 Remote Worker.
- `docs/architecture/INVARIANTS.md` — measurement/resource isolation invariants.
- `planning/roadmap.yaml` and `planning/tasks/queue.yaml` — read-only overlap check; no canonical owner for this delta was found.
- Future resource adapters and worker scheduler are implementation targets only after an ADR is accepted; this research item authorizes no production change.

**invariants:**

- `INV-063` — measured vs estimated runtime/resource values retain hardware/backend/time provenance.
- `INV-064` — device/VRAM/unified-memory handling stays behind resource/residency ports and platform details do not leak into router domain logic.
- `INV-080` — runtime performance claims require reproducible benchmark methodology and hardware/build provenance.

**adrs:** no accepted ADR defining hardware topology/interconnect/contention evidence or placement-cost semantics was found.

**tasks:** no canonical roadmap/task-queue item currently owns this delta. This research does not modify `planning/roadmap.yaml` or `planning/tasks/queue.yaml`.

**actual_delta:** Define ownership and data contracts for resource locality (for example same device/host/fabric/network boundary), measured or estimated link bandwidth/latency with provenance/confidence, shared-resource interference/contention observations, topology/version fingerprinting and expiry/invalidation, and a provider-neutral placement-cost projection consumed by future Router/Scheduler logic. Platform-specific probes remain adapters; Task/Execution/Attempt identity and Policy/Verification semantics remain placement-independent.

**decision:** `ADAPT`

**target_subsystem:** `Device Resource Manager + Model Residency + Adaptive Model Router + future G12 Worker Scheduler`.

**priority:** `LOW now; MEDIUM before multi-device local-model scheduling or G12 placement becomes an active product requirement`.

**dependencies:**

- a concrete multi-device or multi-worker Runlith workload where placement can materially differ;
- accepted ADR for topology/contention evidence ownership and schema;
- bounded measurement/profiling methodology;
- resource adapters capable of exposing observations without widening authority or leaking sensitive host details.

**security_impact:** Topology/capacity evidence is scheduling evidence only. It cannot grant filesystem, network, secret or process authority; every remote/local placement still re-evaluates Policy, privacy, isolation, scoped secrets and egress rules. Hardware/network identifiers should be minimized where full topology disclosure would expose sensitive infrastructure data.

**architecture_impact:** Moderate and future-facing. The contract extends resource evidence without introducing GPU-training concepts into Task semantics or coupling domain code to NVIDIA/Apple/cluster APIs.

**candidate_task:** `TASK-CANDIDATE — Topology/contended-resource profiling research spike`, gated on the triggering workload and preferably preceded by an ADR/research decision.

**verification_direction:**

- fixed workload plus hardware/runtime/topology fingerprint produces reproducible observations with measured/estimated distinction;
- capacity-only and topology-aware placement are compared on a representative workload;
- a demonstrated topology/link/contention change can alter placement only when evidence crosses an explicit cost/feasibility threshold;
- stale topology fingerprints invalidate or lower confidence in dependent runtime evidence;
- concurrent workloads demonstrate whether measured interference is significant enough to model;
- provider/platform-specific probe types do not leak into Router/Scheduler domain contracts;
- missing/low-confidence topology evidence degrades conservatively without widening authority;
- remote-worker selection still enforces Policy/privacy/secret/network/isolation constraints independently of performance score.

**runlith_improvement:** The Playbook optimizes distributed training throughput for known accelerator topology. Runlith can generalize only the evidence discipline: provider-neutral placement inputs, durable provenance, explicit uncertainty, policy/privacy constraints and explainable scheduling for heterogeneous local/remote resources.

## IKQ-007 — Browser target ownership and temporary control lifecycle

**id:** `IKQ-007`

**origin_repository:** `https://github.com/Tencent/BrowserSkill`

**origin_revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**idea:** Define a provider-neutral Runlith contract for ownership of browser targets and temporary control of pre-existing user tabs/windows, including exact grant/revocation, popup/opener inheritance rules, cleanup/return compensation and restart reconciliation.

**novelty:** `NEEDS_ADR`. Runlith already defines stateful browser sessions as capability boundaries, scoped browser credentials, browser-neutral ports, audit receipts and a deferred Web Acquisition ADR requirement. What is absent is the narrower target-ownership contract: no accepted architecture was found for user-owned versus task-created targets, temporary borrow grants, original-target provenance, implicit authority inheritance, or reconciliation of a failed/ambiguous return after browser-worker restart.

**status:** `NEEDS_ADR`

**runlith_existing_state:**

- Source of Truth defines `BrowserAutomationPort` and browser worker/session isolation.
- `INV-099` treats browser sessions/cookies/auth storage as scoped credentials/capabilities.
- `INV-104` makes material browser actions auditable.
- `INV-105` prevents browser runtime infrastructure from owning durable Task state.
- `docs/architecture/adr/COVERAGE.md` already requires a dedicated ADR before first Web Acquisition implementation.
- No explicit user-tab/target ownership, borrow/return, popup/opener inheritance or ownership-reconciliation contract was found in architecture docs, accepted ADRs, roadmap, task queue or current implementation.

**files:**

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` — future `BrowserAutomationPort`, browser isolation, browser receipts and network policy.
- `docs/architecture/INVARIANTS.md` — `INV-033`, `INV-095`-`INV-106`, security authority invariants.
- `docs/architecture/adr/COVERAGE.md` — Web Acquisition deferred ADR slot.
- `docs/security/THREAT_MODEL.md` — browser credential/network trust threats.
- `planning/roadmap.yaml` and `planning/tasks/queue.yaml` — read-only overlap check; no canonical task currently owns this exact delta.

**invariants:**

- `INV-002` — browser target/session identity must not collapse into Task/Execution/Attempt identity.
- `INV-033` — browser automation remains behind a browser-neutral port.
- `INV-099` — browser session/auth state is a scoped capability.
- `INV-104` — browser actions remain auditable.
- `INV-105` — browser runtime cannot own durable Task state.
- `INV-146`-`INV-153` — canonical authorization, delegated scope, exact approval, secret and enforcement rules.

**adrs:** Web Acquisition/Browser ADR is already required before implementation; this ownership model should be included there or in a tightly scoped companion ADR rather than introduced ad hoc in an adapter.

**tasks:** no canonical task matching the target-ownership/borrow lifecycle delta was found. This research does not modify canonical planning.

**actual_delta:** Define browser target identity and ownership class (`task-created`, `user-owned`, future shared/remote worker variants); exact temporary grant bound to Attempt/Policy/approval and validity; no implicit authority widening from window placement/focus/opener/popup; origin/state provenance for return; cleanup ordering; typed ambiguous physical outcomes; reconciliation after worker/browser/agentd restart; and provider-neutral receipts/evidence. Chrome tab/window IDs remain adapter data.

**decision:** `ADAPT`

**target_subsystem:** future `BrowserAutomationPort` + Browser Runtime + Policy/Approval + Attempt/Reconciler.

**priority:** `MEDIUM` now; `HIGH` before Runlith controls a user's normal browser profile or any shared authenticated browser worker.

**dependencies:**

- activation/acceptance of the dedicated Web Acquisition/Browser ADR;
- target/session identity model below Task/Execution/Attempt;
- Policy capability and approval bindings for browser actions;
- Secret Broker representation of site/browser credentials;
- restart reconciliation semantics for browser workers.

**security_impact:** High positive. Prevents authority from being inferred from browser location/opener relationships and separates authenticated device/worker identity from exact user-target/site capability.

**architecture_impact:** Moderate. Adds a browser-specific resource-ownership contract while preserving browser/provider neutrality and existing durable orchestration ownership.

**candidate_task:** `TASK-CANDIDATE — Browser target ownership/borrow contract spike`, only after the Web/Browser ADR accepts the contract direction.

**verification_direction:**

- two sessions cannot control/borrow the same user target concurrently;
- moving a user tab into an Agent/Runlith window without a grant does not authorize it;
- popup/opener/focus/window placement never widens authority implicitly;
- approval delay revalidates current target identity and effective Policy before mutation;
- task-created targets and user-owned targets have different cleanup semantics;
- failed return/rollback is represented as ambiguous/reconcilable rather than success;
- restart/reconnect does not silently adopt stale physical browser targets;
- adapter-specific tab/window IDs never enter durable Task domain semantics;
- device authentication alone cannot grant unrelated site/account/target authority.

**runlith_improvement:** BrowserSkill demonstrates careful runtime tab ownership. Runlith can make the same concept durable, provider-neutral, Policy-bound and recoverable, preserving exact authority across Attempt lifecycle and browser-worker restarts.

## IKQ-008 — Observation/document-bound actionable browser references

**id:** `IKQ-008`

**origin_repository:** `https://github.com/Tencent/BrowserSkill`

**origin_revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**idea:** Extend Runlith's existing web snapshot provenance/freshness design with an explicit contract for actionable browser refs: every click/type/target ref must be bound to the observation/document/frame/session identity that produced it and must fail closed when that identity is stale or unprovable.

**novelty:** `PARTIALLY_EXISTS`. Runlith already requires provenance-bearing browser/web snapshots with freshness (`INV-103`) and browser action receipts with target/ref hashes (`INV-104`), but targeted search found no accepted rule defining when an element/action ref remains valid across navigation, document/frame replacement, reconnect or adapter restart.

**status:** `PARTIALLY_EXISTS`

**runlith_existing_state:**

- Web snapshots include source/final URL, retrieval time, content hash, backend/profile, policy decision and freshness semantics.
- Browser actions can emit receipts carrying target/ref hash and correlated artifacts.
- Browser/CDP/Playwright types are prohibited from leaking into core Task/Context/Memory contracts.
- Context remains derived state and external web content remains untrusted evidence.
- No provider-neutral `ActionableBrowserRef` validity/invalidation contract was found.

**files:**

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` — Web/Browser §§12-13 and `BrowserAutomationPort`.
- `docs/architecture/INVARIANTS.md` — `INV-033`, `INV-095`-`INV-104`.
- `docs/architecture/adr/COVERAGE.md` — Web Acquisition deferred ADR target.
- `planning/roadmap.yaml` and `planning/tasks/queue.yaml` — read-only overlap check; no canonical task owns stale browser ref semantics.

**invariants:**

- `INV-016`-`INV-018` — model-facing context is derived, observable and reproducible.
- `INV-033` / `INV-096` — browser backend specifics remain behind the port.
- `INV-103` — browser/web snapshots carry provenance/freshness.
- `INV-104` — browser actions emit correlated evidence.
- `INV-146` / `INV-153` — model/controller references do not grant authority without trusted current enforcement.

**adrs:** should be incorporated into the future Web Acquisition/Browser ADR or a narrowly scoped browser action-reference contract accepted at the same gate.

**tasks:** no canonical task matching stale actionable browser refs was found. This research does not modify canonical planning.

**actual_delta:** Define a provider-neutral actionable-ref value/handle that is bound to immutable observation/snapshot identity plus target/session/document/frame revision as needed; trusted action resolution verifies the binding immediately before mutation; navigation/document/frame replacement invalidates or requires re-resolution; missing identity fails closed; receipts preserve which observation/ref identity was used. Short model-facing labels may remain derived display handles, not durable identity.

**decision:** `ADAPT`

**target_subsystem:** future `BrowserAutomationPort` observation/action contracts + Resource snapshot identity + browser action receipts.

**priority:** `MEDIUM`, becoming `HIGH` before click/type/evaluate-style interactive actions are production-enabled.

**dependencies:**

- Web/Browser ADR and browser session/target ownership contract;
- immutable snapshot/document identity shape;
- adapter capability to observe navigation/frame/document transitions;
- receipt/evidence schema for the action-to-observation binding.

**security_impact:** Positive. Prevents confused-target browser mutation where an old model-visible ref resolves against a different document or frame after navigation/replacement.

**architecture_impact:** Small-to-moderate. Strengthens the browser port/resource contract without introducing browser-specific node identifiers into Task semantics.

**candidate_task:** `TASK-CANDIDATE — Stale browser action-ref contract tests`, after the browser ADR establishes identity ownership.

**verification_direction:**

- a ref produced for document A cannot mutate document B after same-tab navigation;
- frame/document replacement invalidates refs scoped to the old identity;
- a short ref reused by a later observation cannot be confused with the earlier ref provenance;
- cross-session/target refs fail closed;
- reconnect/restart cannot silently revive unverified refs from process memory;
- fresh re-observation creates a new actionable binding;
- action receipt identifies the exact observation/ref provenance used;
- backend-specific CDP node IDs remain adapter-private.

**runlith_improvement:** BrowserSkill protects refs inside one browser session with generation/document checks. Runlith can generalize this into a durable, provider-neutral, evidence-bearing action reference that remains safe across adapter replacement and restart.

## IKQ-009 — Registry-derived provider protocol conformance matrix

**id:** `IKQ-009`

**origin_repository:** `https://github.com/decolua/9router`

**origin_revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**idea:** When Runlith activates the broader AgentProvider protocol surface, derive shared conformance cases from the same provider/model capability and wire-format declarations used by runtime composition so declared support automatically creates executable request/response/tool/stream compatibility obligations.

**novelty:** `PARTIALLY_EXISTS`. Runlith already has a reusable provider-neutral `ExecutionPort` contract suite in `crates/execution-contract`, and the provider anti-corruption/replacement boundary is accepted architecture. The narrower missing delta is explicitly outside that seam: `docs/architecture/providers.md` still leaves the full `AgentProviderPort` detect/capabilities/send shape, capability taxonomy, streaming output contract and provider tool-call normalization open, so no accepted cross-provider protocol conformance matrix exists yet.

**status:** `PARTIALLY_EXISTS`

**existing_runlith:**

- `crates/execution-contract` already proves the G5 execution seam for deterministic success/failure/malformed/timeout behavior and opaque provider references.
- `docs/architecture/providers.md` requires provider-specific behavior behind the adapter boundary and defines provider replacement as an architecture fitness test.
- The same provider document explicitly excludes streaming/tool/capability behavior from the current `ExecutionPort` suite and lists those contracts as open design items.
- Policy, approval, Runner evidence, Task/Execution/Attempt state and completion authority already remain outside provider output/translation semantics.

**files:**

- `crates/execution-contract/src/lib.rs` — current shared provider-neutral execution contract suite.
- `docs/architecture/providers.md` — §§13a and 16 define the closed execution seam and open broader provider protocol items.
- `docs/architecture/INVARIANTS.md` — provider neutrality, provider output trust, capability/authority and verification invariants.
- `planning/roadmap.yaml` and `planning/tasks/queue.yaml` — read-only overlap check; this research does not modify canonical planning or allocate a real task ID.

**invariants:**

- provider-specific types/semantics remain behind provider adaptation boundaries;
- provider/model output remains untrusted evidence and cannot mark a Task complete;
- capability declarations describe support/eligibility and never grant Policy authority;
- provider replacement cannot require unrelated Task, Memory, Workspace, Policy or Runner business-logic changes.

**adrs:** no new ADR is created by this research. The broader AgentProvider contract is already an explicit open provider-design area; if its owning gate needs to freeze new canonical protocol semantics, that gate should decide whether an ADR is required.

**tasks:** no canonical roadmap/task-queue item was changed. Candidate remains `TASK-CANDIDATE` until accepted by orchestration/planning.

**actual_delta:** Define a deterministic shared matrix generated from accepted Runlith provider/model capability and wire-format declarations. Declaring support should automatically require fixtures for canonical request/response semantics, structured tool call/result identity, streaming assembly/termination, reasoning metadata, supported media/modality handling, provider-native passthrough equivalence where applicable, malformed/unsupported inputs, and explicit failure when a mandatory semantic cannot be represented without loss. Keep Task/Execution/Attempt lifecycle, Policy authority, Secrets and trusted execution receipts outside this translation matrix.

**decision:** `ADAPT`

**target_subsystem:** future AgentProvider contract + provider adapter conformance-test infrastructure.

**priority:** `MEDIUM` now; `HIGH` when the full AgentProvider send/capability/stream/tool-call surface or a second materially different production provider protocol becomes active.

**dependencies:**

- accepted broader AgentProvider capability/send/stream/tool-call contracts;
- a canonical provider capability/format declaration source suitable for generating test cases;
- deterministic provider fixtures/adapters rather than live-provider tests;
- at least two materially different protocol paths, or one provider plus a canonical translation fixture corpus, to prove the abstraction is real.

**security_impact:** Positive when used as a fail-closed compatibility gate. Malformed/provider-specific output remains untrusted; translation support cannot widen effective capabilities, bypass Policy/Approval, expose secrets, or substitute provider claims for trusted Runner/Tool receipts.

**architecture_impact:** Low-to-moderate and bounded to provider contracts/tests. It strengthens the existing provider anti-corruption layer without adding provider semantics to Task identity or moving orchestration authority into adapters.

**candidate_task:** `TASK-CANDIDATE — Provider protocol translation conformance matrix`.

**verification_direction:**

- adding/changing a declared provider/model capability or wire-format surface automatically expands required conformance cases;
- deterministic native and translated paths preserve equivalent canonical meaning for supported message/tool/reasoning/media concerns;
- tool-call and tool-result identities survive translation and streaming assembly without cross-call confusion;
- malformed provider frames and unsupported mandatory semantics fail explicitly and cannot become success/completion evidence;
- streaming termination/partial-output/error behavior is deterministic and provider-neutral at the accepted contract boundary;
- golden protocol-shape tests are limited to adapter wire contracts, while semantic round-trip/property tests verify provider-neutral meaning;
- no live provider/network dependency is needed for the shared suite;
- CI runs the shared matrix for every adapter declaring the relevant contract.

**runlith_improvement:** 9Router demonstrates that registry-derived matrices keep compatibility tests synchronized with provider/model support. Runlith can make that mechanism safer and more durable by deriving tests from accepted provider-neutral contracts, separating capability facts from authority, rejecting lossy mandatory translations explicitly, and preserving independent Verification/Runner evidence outside provider protocol handling.
