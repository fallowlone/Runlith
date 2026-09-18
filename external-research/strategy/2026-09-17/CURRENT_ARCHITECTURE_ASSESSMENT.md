# Runlith: current architecture assessment

Research date: 17 September 2026. Status: strategic assessment, not an accepted architecture decision or implementation task. Read with [evidence and limitations](EVIDENCE_REGISTER.md).

**Assessment:** Runlith has a credible foundation for a durable engineering control plane. Its strongest property is the separation of intent, execution, permission, and completion authority. Its largest weakness is the distance between this infrastructure and a demonstrated, useful delivery loop. Preserve the foundation; narrow the product claim; validate the complete loop before extending the platform.

## 1. What was actually assessed

The implementation baseline is the main checkout at commit **891c36f4b30a030ee581344fb718403712c8bbf1**, including inspection of its two already modified planning files. The research checkout starts at **d3a656b47b34e800935b182e93b6eefe5431d87b**. They are different snapshots. Research-branch code is not used to declare current main functionality absent.

During final reconciliation, main advanced to **f063af90e75e3bd7f8ba0ece78943d7808ff7431**. The only committed delta was a new [desktop UI/UX research document](../../../../runlith/docs/ui/research/2026-09-17-runlith-ui-ux-research.md); no implementation changed. Its contract gaps, IA proposals and recommendations were cross-checked and incorporated. The code assessment therefore still applies to the implementation baseline.

This review traced manifests, domain state, scheduling, provider execution, planning publication, evaluation, workspace ownership, human decisions, and the architecture/ADR/gate evidence. It is a source-based strategic audit. It did not rerun runtime tests, conduct a penetration test, benchmark latency, or validate a production release.

An important evidence discrepancy remains: main's uncommitted queue metadata says G7 is CLOSED following an independent audit of 891c36f, while the checked-in G7 closure report is the earlier TASK-0081 remediation-required report. TASK-0084 and ADR-0028 contain subsequent remediation. The final audit artifact referenced by the metadata was not found in the inspected documentation. Therefore: **G7 implementation through TASK-0084 is present; final closure is metadata-reported, not independently re-established here.** Earlier findings must not be presented as still-open defects without checking the remediation.

Sources: [queue](../../../../runlith/planning/tasks/queue.yaml), [G7 historical audit](../../../../runlith/docs/development/G7_CLOSURE_AUDIT.md), [ADR-0028 and TASK-0084 amendment](../../../../runlith/docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md).

## 2. Maturity: implemented, contracted, and aspirational

| Capability | Evidence-based state | Product implication |
|---|---|---|
| Local daemon and CLI | Rust workspace, versioned IPC, authenticated local control and business operations exist | A usable control surface exists without a desktop application |
| Durable Task and dependency state | Canonical lifecycle, SQLite persistence, journaled mutations exist | Work can have identity outside chat |
| Execution and Attempt history | Separate identities; one open Attempt per Execution; terminal attempt history preserved | Retry history need not rewrite business intent |
| Workspace isolation and recovery | Managed Git worktrees, ownership checks, bounded cleanup and pre-attempt recovery exist | Valuable substrate; worktrees alone are not a security sandbox |
| Real provider | Claude adapter plus deterministic fake; no second production provider found | Provider neutrality is a design property, not yet demonstrated by two real integrations |
| Policy, approvals, secrets, audit | G6 contracts and implementations exist; historical closure evidence available | Substantial trust infrastructure; not equivalent to an enterprise certification |
| OS confinement | Concrete macOS Seatbelt adapter; unsupported required enforcement refuses execution | Cross-platform IPC does not imply cross-platform production isolation |
| Adaptive planning | Versioned proposals, validation, run/input fencing, durable spend, human continuations exist | Strong bounded planning mechanics, not evidence of a self-managing engineering organization |
| Independent completion authority | Evaluation port and trusted completion transition exist | Correct authority split |
| Real acceptance verification | Production PendingEvaluator defers verification; G8 remains planned | Automated verified delivery is not yet a shipped outcome |
| Context continuity and memory | ADRs/contracts; G9 and G10 planned | Cross-provider continuity and engineering memory are architectural intentions |
| Background autonomy and remote workers | Background execution plumbing exists; broader G11/G12 lifecycle remains planned | Do not promise unattended, crash-resilient overnight delivery yet |
| Desktop product | No frontend package manifest or implemented desktop app found in main | A proposed UI stack is not an existing UX |
| Team cloud | G13 planned | Shared governance, billing and collaboration are future capabilities |

Sources: [workspace manifest](../../../../runlith/Cargo.toml), [production evaluator](../../../../runlith/bins/agentd/src/execution.rs), [provider contract](../../../../runlith/docs/architecture/providers.md), [roadmap](../../../../runlith/planning/roadmap.yaml), [G6 closure evidence](../../../../runlith/docs/development/G6_TASK_0074_G6_CLOSURE_REAUDIT.md).

## 3. Domain model: keep the separations, reconsider the product center

### Task, Execution, Attempt, Session

This distinction is sound. A task is an obligation; an execution is a pursuit of that obligation; an attempt is one bounded try; a session is a provider interaction. A new model, process, or conversation should not erase acceptance requirements or previously observed failures.

The code supports the important parts of this separation. Attempts conclude without completing Tasks. Scheduling consumes identities rather than provider prompts. Keeping one open Attempt inside an Execution simplifies ownership. Parallel candidate solutions, if ever justified, should be explicit separate executions or child work, not concurrent mutations of one Attempt.

Sources: [Execution](../../../../runlith/crates/agent-runtime/src/execution.rs), [Attempt](../../../../runlith/crates/agent-runtime/src/attempt.rs), [execution port](../../../../runlith/crates/scheduler/src/execution_port.rs).

**Strategic change:** keep Task as the central executable unit, but make a reviewable engineering change the primary user object. A customer buys an accepted fix, migration, or maintenance result. The product needs a stable relationship among intent, requirement revision, affected repository revisions, tasks, evidence, and delivery. Existing ChangeSpec/Mission vocabulary already anticipates parts of this; do not invent a second task system.

### Task lifecycle

The lifecycle correctly prevents a direct agent-to-COMPLETED transition. However, twelve exposed internal states would be excessive as the user's default navigation model. Also, FAILED and CANCELLED are declared terminal states with no accepted incoming transitions in the current table. Cancelling an execution or aborting a planning continuation is therefore not the same operation as cancelling the durable Task.

This is an explicitly incomplete contract, not a newly discovered runtime exploit. It becomes a product problem when users need to stop, abandon, supersede, or safely resume work. Resolve those meanings before presenting a universal Stop/Cancel action.

Source: [canonical transition table](../../../../runlith/crates/task/src/state.rs).

### Requirements and acceptance

Task metadata contains acceptance, verification, review, scope and risk fields, but much of the representation is strings. In current G7 authority derivation, every acceptance obligation maps to AutomatedTest, and tasks with forbidden-path constraints that ScopeBound cannot represent fail closed.

That is honest bounded behavior. It is insufficient for the full engineering lifecycle: documentation, accessibility review, operational acceptance, architectural review and performance evidence have different proof methods. G8 should materialize the already intended typed requirement-to-evidence relationship. A list of test commands does not establish that the original user problem is solved.

Sources: [Task metadata](../../../../runlith/crates/task/src/task.rs), [TaskPlanningAuthority::derive](../../../../runlith/crates/plan/src/task_authority.rs), [independent evaluation ADR](../../../../runlith/docs/architecture/adr/ADR-0018-independent-evaluation.md).

## 4. Orchestration and durable planning

The strongest recent decision is separating immutable planning input, the planning operation, and the accepted plan. Run identity/generation, input fingerprint and plan-head checks prevent a slow planner response from becoming current merely because it arrived last. The TASK-0084 amendment additionally re-derives canonical Task authority at publication. Budget reservation before the provider call protects against losing possible spend across a crash.

These are valuable reliability mechanisms because the planner is untrusted proposal generation, not a state owner. The human decision model similarly records durable continuation input without making a UI click itself mutation authority.

Sources: [ADR-0028](../../../../runlith/docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md), [publication transaction](../../../../runlith/adapters/storage-sqlite/src/plan_store.rs), [publication authority checks](../../../../runlith/adapters/storage-sqlite/src/planning_control_store.rs), [human decision model](../../../../runlith/crates/plan/src/human_decision.rs).

Three limitations matter:

1. **A durable DAG is not an autonomous scheduler.** Basic readiness is derived from READY and completed dependencies. The executor port represents one injected, bounded execution, not a demonstrated resource-aware fleet. Generic external-event reconciliation and post-attempt/orphan recovery need their own lifecycle contracts.
2. **Every production execution currently needs a current accepted plan.** This preserves authority, but may create excessive ceremony for a small, already specified change. Investigate an explicitly accepted, minimal deterministic plan for such work; this is a future product/ADR option, never a bypass of current admission.
3. **Planning correctness is not plan quality.** A bounded, acyclic, authorized plan can still be unnecessary or poorly decomposed. Judge plans by downstream review effort and accepted outcomes, not by graph size, number of roles, or validator pass rate.

Sources: [readiness](../../../../runlith/crates/scheduler/src/readiness.rs), [plan-gated orchestration](../../../../runlith/crates/app-services/src/planned_orchestration.rs), [research gaps IKQ-001 and IKQ-004](../../IMPLEMENTATION_KNOWLEDGE_QUEUE.md).

## 5. Provider abstraction: strong boundary, unproven portability

The existing ExecutionPort is appropriately small for the first implementation: identity in; bounded evidence or a normalized problem out. Provider-specific behavior stays in the adapter. Provider and runner are conceptually separate.

The tradeoff is loss of recovery information: quota, authentication, network, incompatibility and malformed-output failures largely arrive at the seam as Failed with opaque detail. Cancellation also travels through composition-owned machinery rather than the port. That is acceptable at the present gate; reliable routing will eventually need typed evidence of failure scope, operation delivery state, cancellation acknowledgement, and resume compatibility.

Do not respond by designing a universal model API. Distinguish **model provider**, **agent harness**, **execution environment**, and **user/account entitlement**. Each has different capabilities and failure modes. An ACP or MCP integration may standardize transport or tools; it does not supply Runlith's durable acceptance and recovery semantics. [ACP introduction](https://agentclientprotocol.com/get-started/introduction), [Zed external-agent ownership](https://zed.dev/docs/ai/external-agents).

The decisive portability test is a second real harness completing the same bounded workflow while preserving task identity, scope, evidence and recovery behavior. A fake provider proves contract testability, but does not establish real-world interchangeability.

Sources: [provider contract](../../../../runlith/docs/architecture/providers.md), [execution seam](../../../../runlith/crates/scheduler/src/execution_port.rs), [research IKQ-002/003/009](../../IMPLEMENTATION_KNOWLEDGE_QUEUE.md).

## 6. Verification is the next product-critical boundary

Runlith already makes the correct authority decision: the implementer submits; an independent evaluation path decides; the trusted Task service commits completion. In production, PendingEvaluator returns an error that leaves the task VERIFYING. This avoids false success, but the customer still lacks the promised completed workflow.

The current EvaluationRequest carries task/execution/attempt identities, a summary and an evidence reference. It is not yet a complete acceptance dossier tied to an exact code revision and evaluator environment. The two terminal verdicts, Verified and Rejected, also need an evidence model that preserves incomplete, unavailable, stale or contaminated checks without calling them success.

Before expanding autonomous output, establish:

- which requirement was checked and against which revision;
- which trusted process or reviewer produced the evidence;
- what environment, toolchain, fixtures, network and tool visibility affected the check;
- whether the implementer changed its own acceptance tests;
- which checks did not run;
- when an edit, merge or environment change invalidates prior evidence;
- the difference between verified acceptance, human acceptance, merged code and observed deployment.

These extend existing ADR-0018 and the research queue's evaluator-environment gap; they are not a new competing verification architecture. Passing tests remain evidence for specified claims, not a proof of general correctness.

Sources: [evaluation request](../../../../runlith/crates/evaluation/src/request.rs), [verdict](../../../../runlith/crates/evaluation/src/verdict.rs), [PendingEvaluator](../../../../runlith/bins/agentd/src/execution.rs), [IKQ-010](../../IMPLEMENTATION_KNOWLEDGE_QUEUE.md).

## 7. Memory and context: good design, excessive implied scope

Separating current context, continuity, durable knowledge and raw evidence is a long-lived strength. Provenance, supersession and refresh are more valuable than a large store of chat summaries.

The risk is treating the designed Memory Engine, Resource Engine, Context Engine, entity graph, documentation explorer and adaptive router as simultaneous launch requirements. They are not. For the initial maintenance workflow, repository-owned instructions, current source, previous verification receipts and a small set of validated decisions may be sufficient.

Memory should earn its place by reducing repeated investigation or recurring failures. Evaluate retrieval against a no-memory baseline. Preserve permission boundaries and invalidate repository facts when their source changes. Do not promote model-written notes automatically into future authority.

Sources: [memory ADR](../../../../runlith/docs/architecture/adr/ADR-0012-structured-memory-provenance.md), [continuity contract](../../../../runlith/docs/architecture/continuity.md), [architecture context registry](../../../../runlith/ARCHITECTURE.md).

## 8. Isolation and local-first: valuable, qualified advantages

Policy decides authority; the runner enforces an available isolation profile; approval binds a particular decision; secret delivery has a separate boundary. This is stronger than relying solely on model instructions.

Keep the claim precise. Historical G6 evidence records macOS confinement, a coarse HTTPS network scope, platform limitations, and a real-provider smoke that encountered an external model error. Local audit hash chaining is useful for detecting certain alterations while the anchor survives; it is not a signature against a fully compromised host. No fresh security certification is provided by this review.

Local-first means the user controls canonical local state. It does not mean inference is offline or that no code reaches a chosen external provider. Nor does a local laptop automatically keep working when asleep. Privacy and unattended availability are different design decisions.

Sources: [G6 residuals](../../../../runlith/docs/development/G6_TASK_0074_G6_CLOSURE_REAUDIT.md), [privacy architecture](../../../../runlith/docs/privacy/PRIVACY_ARCHITECTURE.md), [runner contract](../../../../runlith/docs/architecture/runners.md).

## 9. Architectural complexity and documentation drift

The 28-member Rust workspace is not itself evidence of overengineering. Core domain manifests show useful dependency discipline. However, application services depend directly on storage-sqlite, TaskService owns a concrete SqliteStore, and multi-fact transactions expose storage coordination. The accepted G2 composition documents this flow. Consequently, storage replacement is more expensive than the broad replaceability vision suggests; this is a coupling tradeoff, not a claim that all domain isolation is broken.

Source inspection also found large handwritten modules: execution.rs is about 2,900 lines, planning_service.rs about 1,930, and workspace_service.rs about 1,891, including embedded tests where present. These counts are review signals, not proof of defects. The future risk is adding every new workflow branch to the same coordination surfaces.

The 7,603-line architecture baseline includes business strategy, detailed mechanisms and many research adoptions. ARCHITECTURE.md still describes some already implemented capabilities as future bootstrap work, and refers to some decisions as unmaterialized despite later ADRs. PRODUCT.md and DESIGN.md, referenced by policy, were not found in main.

The strategic remedy is a small, current capability map and explicit separation of accepted constraints, shipped capability, research options and product hypotheses. Preserve historical audits as history. Do not make another large strategy document the canonical implementation contract.

Sources: [application dependency manifest](../../../../runlith/crates/app-services/Cargo.toml), [TaskService](../../../../runlith/crates/app-services/src/task_service.rs), [ADR-0021](../../../../runlith/docs/architecture/adr/ADR-0021-g2-business-ipc-and-local-data-composition.md), [architecture map](../../../../runlith/ARCHITECTURE.md), [baseline](../../../../runlith/AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md).

The new UI research also exposes an important product seam: command capability is ahead of rich read capability. The current wire has basic Task reads, execution lookup by known identity and bounded planning operations; it does not yet provide a complete revision/evidence/history browser. The control protocol remains one request in flight per connection, without streaming or multiplexing. A thin desktop client cannot safely fill these gaps by reading SQLite, invoking Git itself or treating agent prose as authoritative state. These are product read-contract needs, not a reason to replace the transport prematurely.

Sources: [Task wire](../../../../runlith/crates/ipc-contracts/proto/task_core.proto), [Execution wire](../../../../runlith/crates/ipc-contracts/proto/execution_core.proto), [Planning wire](../../../../runlith/crates/ipc-contracts/proto/planning_core.proto), [IPC contract](../../../../runlith/docs/architecture/ipc.md), [UI research, contract gaps](../../../../runlith/docs/ui/research/2026-09-17-runlith-ui-ux-research.md).

## 10. Comparison with relevant agent systems

These comparisons use documented external behavior, not an audit of proprietary internals. Absence from a reviewed page is not proof that a competitor lacks a capability.

| System | What the evidence demonstrates | Architectural lesson and remaining Runlith test |
|---|---|---|
| Claude Code | Parallel sessions, subagents, experimental agent teams, workflows and worktrees; checkpointing has documented limits | Orchestration is already part of the harness. Demonstrate better durable cross-harness acceptance and recovery, not merely multiple agents. [Agents](https://code.claude.com/docs/en/agents), [checkpoints](https://code.claude.com/docs/en/checkpointing) |
| Cursor | Cloud VMs, browser interaction, test artifacts and event/scheduled automations | Artifacts and background execution are competitive baseline features. Runlith must measure the quality and freshness of acceptance evidence. [Cloud agents](https://cursor.com/docs/cloud-agent), [automations](https://cursor.com/changelog/03-05-26) |
| Devin | Managed autonomous work and desktop coordination across ACP agents | A neutral-looking control center is already competitive territory. Local canonical ownership must create an observable workflow benefit. [Devin Desktop](https://devin.ai/desktop) |
| OpenHands | Composable SDK, model choice, sandboxed execution and lifecycle controls | Durable, open, model-agnostic infrastructure is not unique to Runlith. Product packaging and independent outcome evidence matter. [SDK paper](https://arxiv.org/abs/2511.03690) |
| SWE-agent / mini-SWE-agent | Research emphasis on effective agent-computer interfaces; maintainers now favor the simpler successor | Use the smallest adequate execution harness; do not equate harness complexity with capability. [Maintainer guidance](https://github.com/SWE-agent/SWE-agent/blob/main/docs/index.md) |
| Copilot Workspace | Editable intent-to-plan experience; technical preview ended 30 May 2025 | A plan is a useful review artifact; a discontinued preview is not the current competitive baseline. [Historical product page](https://githubnext.com/projects/copilot-workspace/) |
| Current GitHub Copilot and Codex | Repository-integrated cloud agents, multiple workflow surfaces, parallel work and review flows | Distribution and existing user identity are major incumbent advantages. [Copilot](https://github.com/features/copilot), [Codex app](https://openai.com/index/introducing-the-codex-app/) |

## 11. Decisions this review supports

| Remain | Change | Simplify or remove from near-term scope |
|---|---|---|
| Durable work outside chat | User center from internal Task machinery to accepted engineering changes | Default DAG wall and agent roster |
| Independent completion authority | Materialize real, revision-bound acceptance evidence | Universal reliability score |
| Policy and enforcement separation | Make operating guarantees and platform gaps visible | Security branding broader than measured enforcement |
| Versioned plan/input/run separation | Add measured planning value and a minimal path for simple work | Mandatory LLM ceremony for every task |
| Local daemon and SQLite | Admit concrete storage coupling; evolve transaction boundaries when needed | Premature distributed architecture or full event sourcing |
| Provider-neutral domain | Prove interoperability with two real harnesses | A large provider catalog before workflow evidence |
| Provenance-bearing memory direction | Start from a few validated, useful knowledge types | General knowledge graph, document/OCR platform, council engine |

The next architecture milestone should be the smallest complete, evidence-backed customer journey. Another independently sound subsystem is not enough if the user still has to reconstruct the delivery state by hand.
