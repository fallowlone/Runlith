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

Coverage register (not an ADR): [COVERAGE.md](./COVERAGE.md).
