# Runlith: a 3–5 year architecture strategy

Research date: 17 September 2026. Status: strategic proposal; no ADR, gate, scope, or implementation authority is changed. Horizon dates below are scenarios, not forecasts or delivery commitments.

**Direction:** evolve Runlith into a user-controlled system of record for delegated engineering changes, with replaceable execution and verifiable acceptance. An “AI engineering organization” should mean explicit responsibilities, budgets, evidence and escalation, rather than simulated employees.

## 1. Design for what should survive better models

Model-specific prompt scaffolding, hand-tuned context tricks and some decomposition heuristics may become obsolete. Responsibility for authorization, durable intent, external side effects, ownership, acceptance and audit does not disappear when model quality improves.

Preserve that distinction in architecture investment. Every model-compensating optimization should have a retirement condition. Security, durability and completion authority should not be retired because an agent performs well on a benchmark.

The architecture must support three plausible futures:

| Future | What changes | What Runlith must still contribute |
|---|---|---|
| Agents become dramatically more capable | Less human steering and less custom planning | Acceptance, integration, policy and accountable delegation across systems |
| Progress is uneven | Frequent escalation and repository-specific failures | Good recovery, bounded retries and excellent human takeover |
| Agents become commodities inside IDEs and trackers | Execution becomes cheap and bundled | Portable project/workflow evidence that measurably lowers oversight and rework |

This is not an empty market. Warp advertises factories with harness choice, replay, evaluation and self-improvement, while Linear runs coding sessions using Claude Code or Codex. Those are direct challenges to the long-term thesis, not merely inspirations. [Warp Factories](https://www.warp.dev/factories), [Linear coding sessions](https://linear.app/docs/coding-sessions).

## 2. A compact set of core primitives

The table specifies semantic ownership, not a request for a new crate or database table per row.

| Primitive | Owns | Must not become | Current overlap |
|---|---|---|---|
| Engineering Change / ChangeSpec | Versioned intended outcome, non-goals, acceptance and delivery target | A second independent Task lifecycle | Already anticipated by source-of-truth requirements and Mission/ChangeSpec vocabulary |
| Task | Executable obligation and dependencies | Provider chat or agent persona | Implemented |
| PlanRevision | Accepted decomposition and dependency proposal history | Execution permission by itself | Implemented |
| PlanningInputSnapshot / PlanningRun | Inputs, claim generation and planning spend | Mutable combined planning truth | Implemented |
| Execution / Attempt / Session binding | Pursuit, try and disposable provider interaction | Business completion | Execution/Attempt implemented; richer portability remains open |
| Workspace / Artifact identity | Exact repository state and produced material | Authority inferred from a mutable path | Workspace foundations implemented; artifact contract expands later |
| Acceptance contract / Evidence / Verdict | What must be established, observations and assessment | A model's confidence score | Authority designed; full G8 evidence pending |
| Action intent / receipt / reconciliation | Whether an external effect was requested and actually occurred | Automatic replay of an uncertain action | G8 direction already designed |
| Delegation grant / HumanDecision | Scope, spending limits, responsibility, expiry, continuation | A blanket “trust this agent” switch | Policy/approval implemented; planning decisions bounded |
| Knowledge assertion | Validated reusable fact, source, scope, freshness and supersession | Unbounded transcript accumulation | G10 designed |
| Procedure / Routine | Versioned repeatable workflow and one occurrence | An always-on planning loop | Future boundary already recognized |
| Delivery record | Accepted revision, review, merge/deploy observations | “Agent finished” | Needs concrete product semantics |

The missing work is primarily **executable semantics and end-to-end integration**, not naming more entities. Map proposals to existing accepted vocabulary before adopting them. [Current assessment](CURRENT_ARCHITECTURE_ASSESSMENT.md), [existing architecture registry](../../../../runlith/ARCHITECTURE.md).

## 3. Logical architecture

~~~mermaid
flowchart TB
  H["Human or external work request"] --> I["Intent and acceptance contract"]
  I --> P["Planning proposals and validation"]
  P --> D["Durable work and orchestration"]
  D --> A["Authority and budget check"]
  A --> X["Replaceable harness and isolated execution"]
  X --> E["Artifacts, observations and receipts"]
  E --> V["Independent verification"]
  V --> R["Human decision and delivery"]
  E --> C["Recovery and reconciliation"]
  C --> D
  V --> K["Validated project knowledge"]
  K -. "Scoped derived context" .-> P
  K -. "Scoped derived context" .-> X
  R -. "Observed delivery outcome" .-> K
~~~

The UI, CLI, IDE extension and integrations are clients of the same semantics. This is a future logical map, not a claim that every box exists today. Its foundation is Runlith's existing Task/Runtime/Policy/Evaluation separation; the acceptance, knowledge and delivery path is the proposed product evolution.

### Authority boundary

Trusted deterministic services own mutations, policy decisions and acceptance transitions. Models propose plans, produce work and offer judgments. Model review may contribute evidence; it cannot grant itself permissions or substitute for required checks.

### Execution boundary

The agent harness decides how to use its capabilities within a bounded assignment. Runlith decides whether an assignment is admissible, what state it belongs to, how it is observed, and what must happen before the result is accepted.

Do not take over every tool call in every external harness merely to create an illusion of uniformity. Explicitly advertise the enforcement and observation fidelity supported by each integration. Unsupported mandatory guarantees block that mode.

### Evidence boundary

Verification binds to the exact submitted artifact, acceptance revision, environment and evaluator identity. A successful check against yesterday's commit cannot automatically approve today's diff. Verification runs may be parallel; acceptance remains an explicit decision over required evidence.

### Knowledge boundary

Repository code, task state, decisions, logs, transcripts and learned assertions have different owners. Indexing and context packages are rebuildable. Knowledge is usable only within its access scope and freshness policy.

## 4. What must be designed now

These decisions are costly to reverse because they affect stored meaning or user trust:

1. **Outcome and acceptance identity.** Bind a change to a revision of its requirements. Preserve supersession when the human changes the objective.
2. **Evidence identity and invalidation.** Name the code/artifact state, environment, evaluator and actually executed checks. Preserve missing evidence.
3. **Execution ownership.** Define who can continue a run after loss of contact; an expired owner must not publish as current.
4. **External-effect ambiguity.** Distinguish no action, observed action and uncertain action. Reconcile before retry.
5. **Cancellation and takeover.** Distinguish stopping a process, suspending a workflow, abandoning intent, and transferring control. Preserve partial work.
6. **Human delegation.** Separate “do this work” from “publish externally,” and bind decisions to the actual proposal and current state.
7. **Portability and export.** Durable work, evidence and knowledge must remain intelligible outside one provider or paid account.
8. **Privacy destinations.** Show what leaves the device and who receives it. Optional Runlith cloud and an external inference provider are separate recipients.
9. **Resource and spend accounting.** A run budget includes planning, retries, verification and environment use. Missing usage must be explicit, not zero.
10. **Stable responsibility boundaries.** Keep app clients, product orchestration, harnesses, runners and evidence stores separate without prematurely standardizing every internal API.

Some are already accepted principles. The recommendation is to make them concrete at their next active gate, not reopen sound decisions. In particular, the evaluation-environment contract is an existing [research candidate, IKQ-010](../../IMPLEMENTATION_KNOWLEDGE_QUEUE.md).

## 5. Capability/module seam: preserve now, productize later

A future module ecosystem is compatible with the architecture, but the product should not pay the full platform cost before the core workflow is validated. Preserve a seam in which a module has typed input/output, declares capabilities, produces artifacts/evidence with provenance, and receives authority only through Runlith policy and runtime boundaries.

The recommended evolution is **core workflow → first-party modules → paid first-party modules → bounded SDK → third-party ecosystem → marketplace**. Prompt Intelligence and Research are plausible first-party experiments because their incremental value can be measured without redesigning the execution authority model. A module must not gain ambient repository, network, secret or completion authority merely because it is installed.

See [Capability & module ecosystem](CAPABILITY_MODULE_ECOSYSTEM.md) for contracts, commercial hypotheses and evidence gates.

## 6. What can wait

Defer a universal plugin SDK, a general graph database, self-hosted model residency optimization, an agent marketplace, full event sourcing, arbitrary multi-region scheduling, a second cloud implementation language decision, and a generic document/browser automation platform. This does not mean forbidding first-party modules; it means proving bounded module value before standardizing a public extension platform.

Preserve extension seams, but do not pay every possible platform cost in advance. SQLite remains appropriate for a local authority. Move a bounded context to a different persistence or deployment model only when an observed workload or ownership requirement demands it.

The source baseline names Go for a future cloud plane. Treat that as a revisitable implementation choice when cloud work is justified, not a product strategy or a current reason to create services.

## 7. Evolution by evidence gates

| Horizon | Customer capability | Architecture emphasis | Evidence needed to proceed |
|---|---|---|---|
| Next 0–3 months, proposed validation window | One bounded maintenance change can be delegated, interrupted and reviewed | Existing core plus real acceptance evidence and a thin user journey | Paid pilots show lower supervision/rework; no false verified state in evaluated cases |
| 3–12 months, conditional | Reliable repeated work in one repository; selective handoff between two harnesses | Recovery, second-provider compatibility, limited continuity, usable evidence history | Returning users repeatedly use it without founder assistance; recovery is measured on declared failure classes |
| 12–24 months, conditional | Small teams share delegation and acceptance policy | One-owner execution claims, team identity, access-controlled knowledge and synchronization | Team buyers pay for shared workflow, not only a demo dashboard |
| 24–36 months, conditional | Repeated maintenance across repositories and remote workers | Durable routine occurrences, capacity scheduling, integration and delivery reconciliation | Positive unit economics; multi-repository demand; manageable operational support |
| 36–60 months, conditional | Governed engineering operations across people and agents | Federated execution, policy distribution, organizational learning and audited delivery | Measurable outcome advantage over bundled incumbent platforms |

These are product horizons layered over the existing G8–G13 gates. They do not reorder security prerequisites or authorize work in the queue. A narrow product experiment may validate the right UX before every future subsystem is built.

## 8. Multi-agent collaboration without an agent bureaucracy

Use collaboration where independence or parallelism has measurable value:

- parallel read-only investigation for genuinely distinct uncertainties;
- bounded workers on separable changes with explicit integration responsibility;
- independent review where another perspective can find errors;
- a human owner for ambiguous tradeoffs and final risk acceptance.

Start with one worker for a converged implementation task. Additional agents increase compute, context duplication, coordination and review burden. Claude's own documentation distinguishes subagents, agent views, teams and deterministic workflows, and recommends choosing according to the coordination need. [Claude parallel approaches](https://code.claude.com/docs/en/agents).

Require an accountable integration step after parallel changes. Non-overlapping paths do not imply semantic compatibility. Verify the integrated revision; preserve which worker produced which artifact and what the integrator changed.

Avoid universal councils, majority voting and fixed reviewer counts. Correlated model errors are not independent confirmation. A dissenting observation remains reviewable; a veto needs policy authority or evidence, not a persona title.

## 9. Long-running work and continuous verification

Long-running work is a sequence of bounded, durable episodes. It is not a single conversation that never ends.

A continuation should reconstruct the current accepted objective, workspace/artifact identity, last observed effects, remaining evidence obligations, budget and authority. Provider-native resume is an optimization after compatibility checks. It is never the sole surviving record of work.

Continuous verification should be selective:

| Change | Consequence |
|---|---|
| Artifact changed | Invalidate checks whose subject changed |
| Acceptance changed | Reassess coverage, even if code is unchanged |
| Toolchain or verification environment changed | Re-run affected checks or mark evidence non-comparable |
| Policy changed | Re-evaluate permission and required gates |
| External delivery status uncertain | Observe/reconcile before publishing or retrying |

Do not run the whole test universe after every keystroke. Evidence reuse needs a stated validity rule, and invalidation needs an explanation visible to the reviewer.

## 10. Engineering memory and self-improving workflows

Organizational memory is a set of scoped assertions: “this command is the supported test entry point,” “this module owns authorization,” or “this prior migration failed for this reason.” Each assertion needs provenance and a refresh condition.

A learning loop should be:

**Observed outcome → candidate improvement → offline comparison → human/policy review → versioned workflow change → monitored rollout.**

Learning may adjust retrieval, model preference, task selection or a procedure. It may not silently weaken acceptance, grant permission or replace the evidence standard. Keep a holdout of tasks that were not used to tune the workflow, and compare on review time, accepted outcomes and cost.

Local learning can increase value for an individual repository. It does not automatically create a company-wide data network effect: customers' repositories and workflows differ, and local-first data is not automatically available for central training.

## 11. Local-first to team/enterprise

The future system must avoid two devices independently authorizing the same side effect. For execution and acceptance, prefer one authoritative owner with explicit delegation and fencing. A disconnected worker may retain already authorized bounded work, but reconnection must reconcile before new publication.

Synchronizing documents or derived views is different from synchronizing permissions, spend and mutable execution state. Do not solve all of them with a generic last-writer-wins policy or a blanket CRDT claim.

Choose cloud modes deliberately:

- **Local-only control:** no Runlith account needed; provider-bound inference remains separately disclosed.
- **Optional coordination service:** identity, shared policies and metadata under explicit consent.
- **Customer-controlled remote execution:** worker registration, scoped credentials and declared data locations.
- **Enterprise operations:** audit export, access lifecycle, support and deployment agreements after demonstrated demand.

End-to-end encrypted synchronization and server-side inspection are different products with different trust tradeoffs. Do not promise that the server can neither see data nor require any decryption while also promising unrestricted server-side analysis.

## 12. Platform boundaries that should remain stable

| Boundary | Durable contract | Replaceable choices |
|---|---|---|
| Human ↔ system | Intent, authority, explanation, control and acceptance | Desktop layout, CLI, IDE client |
| Orchestration ↔ harness | Assignment, capabilities, evidence and lifecycle | Claude, Codex, OpenHands, future harnesses |
| Runtime ↔ environment | Required confinement, observed enforcement, ownership | Native constrained runner, container, remote worker |
| Verification ↔ artifact | Subject identity, check identity, provenance, verdict | Test frameworks, static tools, model reviewers |
| Knowledge ↔ context | Scoped references, freshness and trust | Search, embeddings, symbol indexes |
| Core ↔ capability module | Typed intent/input, declared capability request, artifacts/evidence, cost and result | Prompt intelligence, research, verification, delivery and future third-party modules |
| Local ↔ team service | Ownership, synchronization, deletion and export | Hosting provider and storage technology |

The five-year advantage would be a deeply learned and trusted engineering workflow. A large architecture diagram is neither a moat nor a reason to delay the first paying customer.
