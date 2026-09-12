# Architecture — AI Engineering OS

> Concise repository-level map: layers, bounded contexts, ownership, allowed
> dependencies, statuses. Defines no new architecture.
>
> - *Why the product exists:* [`PROJECT_CHARTER.md`](./PROJECT_CHARTER.md)
> - *Non-negotiable rules:* [`docs/architecture/INVARIANTS.md`](./docs/architecture/INVARIANTS.md)
> - *Why each decision was made:* [`docs/architecture/adr/INDEX.md`](./docs/architecture/adr/INDEX.md)
> - *Full baseline:* `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`

## 1. System layers

```text
Desktop Client (React + TS + Vite)
 |
Thin Tauri Host (Rust, native shell only)
 |
typed local IPC — UDS macOS/Linux, Named Pipe Windows, versioned Protobuf
 |
agentd (Rust, modular monolith, owns orchestration)
 |
Ports / Adapters (explicit, consumer-owned)
 |
Providers / Runners / OS services (Claude, Codex, OpenCode, …)
```

### Desktop Client

Owns: presentation, interaction, product UX, client-side view state.

Forbidden: shell execution, Git mutation, Task transitions, provider
orchestration, security policy decisions, secret access. UI reaches privileged
operations only as `React → Tauri DTO → agentd request`.

### Thin Tauri Host

Owns: window lifecycle, native dialogs, notifications, updater, OS integration,
agentd lifecycle/connectivity.

Forbidden: orchestration business logic, Task Engine, Context Engine, provider
selection, Git merge logic, security policy. Commands validate, call the local
client and return a DTO.

### agentd

Owns local application/business orchestration across the bounded contexts
below. Concrete adapters are assembled **only** by the composition root; no
domain crate instantiates infrastructure.

Status: workspace bootstrap exists (`bins/agentd`, `bins/agentctl`,
`crates/ipc-contracts`); IPC transport and engines arrive per gates G1+.

## 2. Core bounded-context registry

Architecture boundary ≠ implementation requirement. No crates are created by
this document; physical decomposition happens incrementally.

### Task — Early core (G2)

- Purpose: durable work as the central business entity.
- Owns: Task identity, lifecycle, dependencies, acceptance-contract references,
  durable task state.
- Forbidden: provider sessions, CLI process state, model prompts, Git
  implementation, verification self-approval.
- Inbound: orchestration coordination, evaluation verdicts, snapshot reads.
- Outbound: event journal appends; snapshot ports for consumers.
- Authoritative state: task rows, dependency edges, lifecycle status.
- Refs: ADR-0004, ADR-0005; INV-001, INV-002, INV-009.

### Planning / Orchestration — Later core (G3 basic, G7 adaptive)

- Purpose: execution coordination over the durable graph.
- Owns: execution coordination, planning proposals, plan revisions,
  scheduling/reconciliation coordination.
- Forbidden: direct canonical Task persistence; Planner output is a proposal —
  only validated application services mutate durable graphs.
- Inbound: task state, execution evidence, policy decisions.
- Outbound: `PlannerPort` proposals, `PlanChangeSet` validation, typed agent-bus events.
- Authoritative state: plan revisions with reason and provenance.
- Refs: INV-043…050. ADR status: Not yet materialized.

### Agent Runtime — Early core (G3 FakeAgent, G5 first real provider)

- Purpose: provider-neutral execution lifecycle.
- Owns: attempt/session execution mechanics, adapter invocation, tool
  interaction coordination.
- Forbidden: deciding Task completion, security policy, architecture policy.
- Inbound: execution assignments, policy approvals, context packages.
- Outbound: `AgentProviderPort`, `RunnerPort`; result submissions (never
  self-completion).
- Authoritative state: none durable beyond attempt/session records owned via
  Task/Events; execution receipts as provenance evidence.
- Refs: ADR-0009; INV-004, INV-066, INV-072.

### Workspace / Git — Early core (G4)

- Purpose: isolated repository workspaces for autonomous execution.
- Owns: worktrees, branches, workspace isolation, diff state, authorized merge
  mechanics.
- Forbidden: Task lifecycle decisions, provider logic.
- Inbound: execution workspace requests.
- Outbound: diffs/artifacts to Evaluation; paths via explicit handles, never
  ambient filesystem inheritance.
- Authoritative state: worktree/branch bookkeeping for live executions.
- Refs: ADR-0008; INV-005, INV-006.

### Context Engine — Later core (G9)

- Purpose: smallest sufficient context per execution.
- Owns: retrieve, rank, budget, select, security/filter preparation,
  model-facing rendering plan.
- Forbidden: source parsing, durable memory truth, Task truth, treating
  provider-native conversation as application truth.
- Inbound: execution scope, task envelope, policy constraints.
- Outbound: context plans + rendered packages; `TaskSnapshotPort`-style reads
  via application bridges (never direct table reads).
- Authoritative state: none — context is derived/disposable (INV-016).
- Refs: ADR-0010; INV-016…022, INV-030, INV-031, INV-115.

### Context Continuity — Later core (G9)

- Purpose: execution survival across windows, rollovers and provider switches.
- Owns: execution epochs, checkpoints, history continuity, old-evidence lookup,
  rollover continuity (NO CHECKPOINT → NO ROLLOVER).
- Forbidden: making provider-native compaction authoritative.
- Inbound: budget-monitor signals, durable task/attempt state.
- Outbound: checkpoint records, history search/read handles.
- Authoritative state: epochs, checkpoints, working notes.
- Refs: ADR-0011; INV-019…023.

### Memory — Later core (G10)

- Purpose: durable learned knowledge with trust.
- Owns: semantic, episodic, procedural and decision-memory references,
  provenance and lifecycle (capture → archive; no unbounded append).
- Forbidden: owning future work (future work stays Task Graph state);
  promoting summaries to truth over raw evidence.
- Inbound: committed execution evidence via consolidation pipeline.
- Outbound: provenance-bearing memory reads for retrieval.
- Authoritative state: memory entries + links with confidence/version/hash.
- Refs: ADR-0012; INV-029.

### Resource Engine — Later core (G9 retrieval support)

- Purpose: turn raw inputs into normalized, addressable resources.
- Owns: ingest, parse, normalize, semantic segmentation, representation
  generation, provenance, structural metadata, resource registry.
- Forbidden: context selection, Task scheduling, provider policy, UI rendering
  policy.
- Boundary: `Resource Engine → normalized/addressable resources → Context
  Engine`. Neither side reads the other's storage implementation.
- Inbound: raw sources (files, documents, feeds) plus re-parse triggers on
  content change.
- Outbound: normalized resources with stable URIs, hashes and trust metadata
  for Context Engine retrieval; parser-capability declarations for routing.
- Authoritative state: resource registry (ids, hashes, parser versions,
  trust, representations).
- Refs: INV-037…042. ADR status: Not yet materialized.

### Policy / Security — Later core (G6)

- Purpose: authorization decisions before privileged execution.
- Owns: ALLOW / DENY / REQUIRE_APPROVAL, capability requirements, security
  constraints, typed Rules of Engagement where applicable.
- Forbidden: providing OS isolation itself. Policy decision ≠ sandbox
  enforcement — both layers are required.
- Inbound: action requests with capability/scope claims.
- Outbound: decisions consumed by Agent Runtime, runners, web/document
  pipelines; denials are never retried as transient errors.
- Authoritative state: policies, bindings, approvals.
- Refs: ADR-0013, ADR-0026; `docs/security/POLICY_CONTRACT.md`;
  INV-007, INV-013, INV-146…153.

### Evaluation / Verification — Later core (G8)

- Purpose: prove readiness through evidence, not claims.
- Owns: acceptance evaluation, test/review/security evidence aggregation,
  completion decision support. Only this pipeline grants COMPLETED.
- Forbidden: letting agents mark their own tasks complete; accepting model
  self-report as proof.
- Inbound: submitted artifacts, diffs, policy attestations.
- Outbound: VERIFIED / REJECTED verdicts with evidence refs.
- Authoritative state: evaluations, evaluation results.
- Refs: INV-068, INV-092. ADR status: Not yet materialized.

### Events / Event Journal — Early core (G2)

- Purpose: durable, ordered record of domain change.
- Owns: append-only application event journal; typed/versioned domain-event
  records with correlation, causation and timestamps.
- Forbidden: becoming full Event Sourcing in v1 — relational state stays the
  authoritative current state where specified; staged migration only.
- Inbound: transactional appends alongside state mutations.
- Outbound: replay/debug/analytics reads; future projection feeds.
- Authoritative state: the journal itself (history), not current state.
- Refs: ADR-0006; INV-010, INV-044.

### Security Audit — Later core (G6)

- Purpose: tamper-evident record of security/privileged decisions and actions.
- Owns: audit entries for privileged/security-relevant transitions.
- Forbidden: merging with operational logs, the Event Journal or model-visible
  session history — three separate concerns.
- Inbound: policy decisions, execution receipts and privileged-action reports
  from runtime, runners and security workflows.
- Outbound: append-only audit reads for incident review and compliance; never
  into model context.
- Authoritative state: audit records (hashes/references, not secret payloads).
- Refs: ADR-0006, ADR-0013, ADR-0026; persistence remains TASK-0066.
  INV-073, INV-152.

### Secrets — Later core (G6)

- Purpose: secret reference brokerage without ever holding plaintext in
  application state.
- Owns: secret refs, scoped credential bindings.
- Forbidden: placing secrets in model context, ordinary logs, events or
  SQLite plaintext. Actual secrets live only in OS Keychain / Credential
  Manager / Secret Service.
- Inbound: scoped credential requests from authorized runtime components.
- Outbound: secret references/handles only — plaintext never crosses the
  broker boundary into application state.
- Authoritative state: secret values live only in the external OS store; the
  context owns refs/bindings, never plaintext.
- Refs: ADR-0013, ADR-0014, ADR-0026; implementation remains TASK-0063.
  INV-008, INV-099, INV-106, INV-151.

### Usage / Model Routing — Later core (gate unassigned)

- Purpose: learn what works and route accordingly, without touching domain
  semantics.
- Owns: measurement/profile evidence collection, routing policy evaluation,
  versioned RoutePlan production. Separation preserved: measurement/profile
  evidence → routing policy → provider execution.
- Forbidden: becoming Task domain semantics; transparent stateful retries
  across models; semantic-similarity cache reuse for stateful work; leaking
  hardware-specific VRAM logic into generic routing policy.
- Inbound: task metadata, capability requirements, privacy policy, live
  capacity and profile evidence.
- Outbound: versioned RoutePlan (primary → fallbacks → escalation) consumed
  by orchestration; outcome feedback recorded back into profiles.
- Authoritative state: usage samples, runtime/quality/capacity profiles with
  provenance (measured vs estimated always distinguished).
- Refs: INV-059…068, INV-084, INV-085. ADR status: Not yet materialized.

### Tools / Skills — Later core (gate unassigned)

- Purpose: capability and workflow extension surfaces.
- Owns: tool registry + just-in-time schema delivery, lazy skill resolution,
  typed agent definitions, deterministic provider renderers.
- Forbidden: overriding Task scope, Policy, architecture, ADRs or acceptance
  criteria. Skills load on demand; only metadata is baseline context.
- Inbound: capability/skill lookup requests from Agent Runtime and Context Engine.
- Outbound: just-in-time schemas, skill bodies and rendered definitions;
  install-ledger records for provider-native artifacts.
- Authoritative state: registry entries, definition versions, install ledger
  with drift detection.
- Refs: INV-028, INV-032, INV-090. ADR status: Not yet materialized.

## 3. Cross-context communication

Allowed: explicit application service/API calls; typed ports; typed/versioned
events; immutable snapshots/DTOs.

Forbidden: direct reads of another context's SQLite tables; shared mutable
global state; provider adapters mutating Task state; UI invoking Git/processes
directly; Context Engine querying another context's DB schema; domain crates
instantiating infrastructure adapters (INV-025, INV-026, INV-027).

## 4. Interface ownership

A port belongs to the consumer that requires the capability — never to the
infrastructure that implements it:

```text
Context Engine → TaskSnapshotPort → adapter/application bridge → Task service
```

No giant global interfaces owned by infrastructure; factories/registries
compose adapters outside the domain (composition root only).

## 5. Canonical-state ownership

| Fact | Canonical owner |
|------|-----------------|
| Task status, dependencies, acceptance refs | Task context |
| Plan revision + provenance | Planning / Orchestration |
| Attempt/session execution record | Agent Runtime (via Task/Events) |
| Epoch, checkpoint, working note | Context Continuity |
| Memory fact + confidence | Memory context |
| Resource metadata + registry | Resource Engine |
| Policy, binding, approval | Policy / Security |
| Evaluation verdict + evidence | Evaluation / Verification |
| Journal record | Events / Event Journal |
| Audit record | Security Audit |
| Usage measurement, route profile | Usage / Model Routing |
| Tool/skill definition version | Tools / Skills |
| Secret value | External OS store via Secrets Broker |

Derived and never authoritative: UI views, ContextPlan packages, documentation
projections, caches, routing scores, summaries (INV-070 — every derived copy
is ephemeral, invalidatable, hashed-snapshot or rebuildable).

## 6. Deferred / later architecture targets

| Target | Classification |
|--------|----------------|
| Documentation Engine (generated/curated layers, provenance, Architecture Explorer) | Accepted boundary, implementation deferred |
| Web Acquisition (SSRF-policed fetch, snapshot store) | Accepted boundary, implementation deferred |
| Document Worker (restricted conversion boundary) | Accepted boundary, implementation deferred |
| Security Runner (restricted mutative-test boundary) | Accepted boundary, implementation deferred |
| Procedure Engine (deterministic versioned workflows) | Accepted boundary, implementation deferred |
| Council Engine (typed veto, minority preservation, diversity) | Accepted boundary, implementation deferred |
| Device Resource / Model Residency ports | Accepted boundary, implementation deferred |
| Artifact Store (content-addressed tool outputs) | Accepted boundary, implementation deferred |
| Local Inference port (utility models) | Optional adapter, future |
| Mission / Role Profiles (`RoleProfile`, `MissionSpec`) | Accepted future orchestration vocabulary; G7 design materialization deferred |
| Human Decision Queue (`HumanDecisionRequest`) | Accepted future coordination/verification primitive; G7/G8 |
| Typed Agent Messaging (`AgentMessage`) | Accepted future provider-neutral coordination primitive; G9 |
| Playbooks / Procedures / Routines | Accepted future workflow hierarchy (`Tool → Playbook → Procedure → Routine`); G6/G10/G11 |
| Advisory Work Claims (`WorkClaim`) | Accepted coordination hint; never replaces worktree isolation; G7 |
| MicroAgents / cheap watcher → strong reasoner escalation | Optional background optimization; G11 / Router |
| Provider Capacity / quota state | Accepted routing input, never Task semantics; future Router |
| Context Disclosure Preview | Accepted privacy UX projection over Context/Policy decisions; G6/G9 |
| Tournament Mode, Consensus Mode, Shadow Agents, Semantic Conflict Detection | Post-v1 concepts (explicitly not v1) |
| Context Replay / Time Travel / Architecture Guardian | Future research |

All rows above: ADR status Not yet materialized. Tournament/Consensus/Shadow
entries must not be promoted into v1 requirements.

## 7. Status legend

- **G0 contract** — frozen interface, exists now (`ipc-contracts`).
- **Early core** — gates G1–G5 sequencing (IPC transport, Task Core, FakeAgent,
  Workspace/Git, first provider).
- **Later core** — gates G6–G11 (security, planning, verification, context,
  memory, background daemon).
- **Deferred** — accepted boundary, no gate assigned yet.
- **Optional adapter** — pluggable, never required for core.
- **Future research** — hypothesis to benchmark before adoption.

Architecturally accepted ≠ must be implemented now.

## References

- Decisions: [`docs/architecture/adr/INDEX.md`](./docs/architecture/adr/INDEX.md)
- Rules: [`docs/architecture/INVARIANTS.md`](./docs/architecture/INVARIANTS.md)
- Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§5–7,
  9–10, 15–18, 20–21, 23–27, 32–43, 47–48, 52–58, 79, 100
