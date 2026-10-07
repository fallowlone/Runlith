# Architecture Index

> Navigation only. This file routes agents to the relevant contract;
> it defines no architecture. Canonical decisions live in the linked files.
> Source baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md`.

## Start here

- Agent protocol (authority order, scope, security rules): [`AGENT.md`](../../AGENT.md)
- Product charter (why, what, non-goals): [`PROJECT_CHARTER.md`](../../PROJECT_CHARTER.md)
- Architecture map (layers, contexts, ownership, statuses): [`ARCHITECTURE.md`](../../ARCHITECTURE.md)
- Binding constraints: [`INVARIANTS.md`](./INVARIANTS.md)
- Accepted decisions: [`adr/INDEX.md`](./adr/INDEX.md)
- Security contracts: [`../security/THREAT_MODEL.md`](../security/THREAT_MODEL.md),
  [`../security/TRUST_BOUNDARIES.md`](../security/TRUST_BOUNDARIES.md),
  [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md),
  [`../security/POLICY_CONTRACT.md`](../security/POLICY_CONTRACT.md)
- Privacy contracts: [`../privacy/PRIVACY_ARCHITECTURE.md`](../privacy/PRIVACY_ARCHITECTURE.md),
  [`../privacy/TECHNICAL_PRIVACY_CONTRACT.md`](../privacy/TECHNICAL_PRIVACY_CONTRACT.md),
  [`../privacy/DATA_FLOW_MAP.md`](../privacy/DATA_FLOW_MAP.md)
- Development roadmap (gates): [`../../planning/roadmap.yaml`](../../planning/roadmap.yaml)
- Near-term task queue: [`../../planning/tasks/queue.yaml`](../../planning/tasks/queue.yaml)
- External/community research adoption notes: [`../../EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md`](../../EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md)

> Changing an accepted invariant requires an ADR and explicit architecture review.

## Route by task type

```text
Core invariants (must not violate)
→ INVARIANTS.md

Development roadmap / gate sequencing / task queue
→ ../../planning/roadmap.yaml
→ ../../planning/tasks/queue.yaml

Architecture decisions
→ adr/INDEX.md
→ adr/COVERAGE.md

Planning / reconciliation work
→ adr/ADR-0017-adaptive-planning.md

Evaluation / completion-authority work
→ adr/ADR-0018-independent-evaluation.md
→ adr/ADR-0030-g8-observed-effect-claim-verification-contract.md
→ adr/ADR-0032-g8-test-integrity-evidence-authoring.md
→ adr/ADR-0034-g8-candidate-bound-test-execution.md
→ adr/ADR-0035-g8-immutable-git-candidate-proposal.md
→ adr/ADR-0036-g8-evidence-requirement-parameter-binding-authority.md

ADR governance / activation / coverage
→ adr/ADR-0019-gate-scoped-adr-activation.md
→ adr/COVERAGE.md

IPC / transport / contract work
→ ipc.md
→ ../../crates/ipc-contracts/proto/agentd_control.proto
→ adr/ADR-0003-local-ipc.md

Task model / state machine / durability work
→ task-engine.md
→ events.md
→ ../../contracts/json-schema/task.schema.json
→ ../../contracts/json-schema/event.schema.json
→ adr/ADR-0004-task-central-entity.md
→ adr/ADR-0005-task-execution-attempt-session.md
→ adr/ADR-0006-state-event-journal-event-sourcing.md

Provider adapter work
→ providers.md
→ fake-agent.md
→ adr/ADR-0009-provider-adapters-acl.md

Runner / isolation execution work
→ runners.md
→ fake-agent.md
→ adr/ADR-0013-policy-engine-os-isolation.md
→ adr/ADR-0008-worktree-isolation.md
→ adr/ADR-0031-g8-side-effect-execution-protocol.md

Context / continuity work
→ context-engine.md
→ continuity.md
→ adr/ADR-0010-context-derived-state.md
→ adr/ADR-0011-provider-independent-context-continuity.md
→ adr/ADR-0042-g9-context-engine-contract.md
→ adr/ADR-0043-g9-continuity-contract.md

Memory work
→ adr/ADR-0012-structured-memory-provenance.md

Security / runtime isolation work
→ adr/ADR-0013-policy-engine-os-isolation.md
→ adr/ADR-0026-g6-security-authority-and-capability-contracts.md
→ adr/ADR-0007-no-direct-frontend-shell.md
→ adr/ADR-0008-worktree-isolation.md
→ ../security/TRUST_BOUNDARIES.md
→ ../security/THREAT_MODEL.md
→ ../security/POLICY_CONTRACT.md

Local-first / privacy / data work
→ adr/ADR-0014-local-first-privacy.md
→ ../privacy/PRIVACY_ARCHITECTURE.md
→ ../privacy/TECHNICAL_PRIVACY_CONTRACT.md
→ ../privacy/DATA_FLOW_MAP.md

UI / frontend work
→ adr/ADR-0015-shadcn-primitives-only.md
→ adr/ADR-0016-agentctl-before-complex-ui.md

Local runtime shape (agentd, modular monolith)
→ adr/ADR-0001-local-runtime-rust-agentd.md

Future cloud control plane
→ adr/ADR-0002-cloud-control-plane-go.md
```
