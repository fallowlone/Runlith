# ADR Index

> Compact JIT navigation. Bodies live in the linked files; this index copies
> no decision text.

| ID | Decision | Status | Relevant areas |
|----|----------|--------|----------------|
| [ADR-0001](./ADR-0001-local-runtime-rust-agentd.md) | Local runtime is separate Rust `agentd` | Accepted | runtime, agentd, modular monolith, composition root |
| [ADR-0002](./ADR-0002-cloud-control-plane-go.md) | Future cloud control plane uses Go | Accepted | cloud (future), strategy |
| [ADR-0003](./ADR-0003-local-ipc.md) | Local IPC over UDS / Named Pipe, versioned Protobuf | Accepted | ipc, transport, contracts, tauri-host, agentctl |
| [ADR-0004](./ADR-0004-task-central-entity.md) | Task is the central durable entity | Accepted | task model, domain, orchestration |
| [ADR-0005](./ADR-0005-task-execution-attempt-session.md) | Task != Execution != Attempt != Session | Accepted | task model, executions, sessions, provider refs |
| [ADR-0006](./ADR-0006-state-event-journal-event-sourcing.md) | v1 SQLite state + Event Journal; staged Event Sourcing path | Accepted | storage, events, audit, migrations |
| [ADR-0007](./ADR-0007-no-direct-frontend-shell.md) | No direct shell execution from the frontend | Accepted | ui, tauri, trust boundary TB1, security |
| [ADR-0008](./ADR-0008-worktree-isolation.md) | Isolated worktrees for autonomous execution | Accepted | git, workspaces, merge queue, isolation |
| [ADR-0009](./ADR-0009-provider-adapters-acl.md) | Provider adapters are anti-corruption layers | Accepted | providers, ports, FakeAgent, contracts |
| [ADR-0010](./ADR-0010-context-derived-state.md) | Context is derived disposable state | Accepted | context engine, retrieval, renderers |
| [ADR-0011](./ADR-0011-provider-independent-context-continuity.md) | Provider-independent context continuity | Accepted | context epochs, checkpoints, history, providers |
| [ADR-0012](./ADR-0012-structured-memory-provenance.md) | Structured memory with provenance | Accepted | memory, provenance, lifecycle |
| [ADR-0013](./ADR-0013-policy-engine-os-isolation.md) | Policy Engine plus OS isolation | Accepted | security, policy, runners, trust boundaries, secrets |
| [ADR-0014](./ADR-0014-local-first-privacy.md) | Local-first privacy | Accepted | privacy, telemetry, data control, storage |
| [ADR-0015](./ADR-0015-shadcn-primitives-only.md) | shadcn/ui is primitive layer, not design system | Accepted | ui, design system, PRODUCT.md, DESIGN.md |
| [ADR-0016](./ADR-0016-agentctl-before-complex-ui.md) | Core usable through `agentctl` before complex UI | Accepted | cli, ui sequencing, verification |
| [ADR-0017](./ADR-0017-adaptive-planning.md) | Adaptive planning is proposal-driven and revisioned | Accepted | planning, reconciliation, PlanChangeSet, liveness |
| [ADR-0018](./ADR-0018-independent-evaluation.md) | Task completion requires independent evaluation | Accepted | evaluation, verification, completion authority, test integrity |
| [ADR-0019](./ADR-0019-gate-scoped-adr-activation.md) | ADR materialization is gate-scoped | Accepted | adr governance, activation policy, coverage |
| [ADR-0020](./ADR-0020-local-ipc-peer-authentication.md) | Local IPC uses OS-authenticated user identity | Accepted | ipc, peer authentication, trust boundary TB2, agentctl sequencing |
| [ADR-0021](./ADR-0021-g2-business-ipc-and-local-data-composition.md) | G2 business IPC is versioned and composed in agentd | Accepted | ipc protocol v1, task business contract, local data composition, event taxonomy, g2 sequencing |
| [ADR-0022](./ADR-0022-runtime-event-journal-scope.md) | Runtime events use typed Execution/Attempt journal scope | Accepted | events, runtime event taxonomy, execution/attempt scope, g3 sequencing |
| [ADR-0023](./ADR-0023-workspace-ownership-and-git-worktree-lifecycle.md) | Workspace ownership and Git worktree lifecycle | Accepted | workspace, git, worktrees, branches, diff, cleanup, reconciliation, g4 sequencing |
| [ADR-0024](./ADR-0024-g5-provider-execution-seam.md) | G5 provider execution seam (`ExecutionPort`), first real provider, IPC v2 execution | Accepted | providers, execution seam, contract suite, ipc v2, composition root, g5 sequencing |
| [ADR-0025](./ADR-0025-g5-post-admission-recovery.md) | G5 post-admission entry recovery path (explicit resume, pre-admission validation) | Accepted | recovery, reconciliation, resume, entry validation, ipc v2 additive, g5 sequencing |
| [ADR-0026](./ADR-0026-g6-security-authority-and-capability-contracts.md) | G6 security authority and capability contracts | Accepted | policy, capabilities, approvals, secrets, audit, runners, g6 sequencing |
| [ADR-0027](./ADR-0027-g7-planning-orchestration-contract.md) | G7 planning/orchestration contract (`PlannerPort`, `PlanChangeSet`, validation, revision provenance, guardrails, vocabulary reconcile, G6 follow-up re-homing) | Accepted | planning, orchestration, planner seam, plan revisions, guardrails, human decisions, g7 sequencing |
| [ADR-0028](./ADR-0028-g7-planning-input-run-and-execution-fencing.md) | G7 planning input/run separation, stale-writer fencing, durable spend accounting, and mandatory accepted-Plan execution admission | Accepted | planning inputs, planning runs, plan revisions, execution admission, stale fencing, g7 sequencing |
| [ADR-0029](./ADR-0029-planner-evidence-resolution.md) | Planner evidence resolution: identity-only `evidence_refs` accepted as the v1 semantic | Accepted | planner evidence, evidence refs, cycle identity, provenance, g7 |
| [ADR-0030](./ADR-0030-g8-observed-effect-claim-verification-contract.md) | G8 observed effect / claim verification contract (`AgentClaim`, `EvidenceRequirement`, `ObservedEffect`, `ClaimVerification`, CV-1..CV-9) | Accepted | verification, claims, evidence requirements, observed effects, claim verification, completion authority, g8 sequencing |
| [ADR-0031](./ADR-0031-g8-side-effect-execution-protocol.md) | G8 side-effect execution protocol (durable `SideEffectIntent`, AMBIGUOUS/RECONCILE recovery, idempotency identity, SE-1..SE-8) | Accepted | side effects, durable intent, recovery, reconciliation, idempotency, workspace, git, g8 sequencing |
| [ADR-0032](./ADR-0032-g8-test-integrity-evidence-authoring.md) | G8 test integrity and trusted verification evidence (TASK-0143) | Accepted | test mutation policy, CV-9, evidence producer, completion authority |
| [ADR-0033](./ADR-0033-g8-independent-test-mutation-review-authority.md) | G8 independent test-mutation review authority | Proposed | CV-9, reviewer identity, scoped test/check mutation |

Coverage register (not an ADR): [COVERAGE.md](./COVERAGE.md).
