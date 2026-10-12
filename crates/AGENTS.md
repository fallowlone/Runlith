# Shared Rust packages

Derived navigation generated 2026-09-30 from `5362f6e`; source links below
remain authoritative. Read the nearest child guide for module-specific work.

## Overview

This tree mixes domains, trusted application services, IPC infrastructure,
contract-test libraries and product-version support. `crates/` is not a
uniform domain layer: [app-services](./app-services/Cargo.toml) explicitly
integrates the concrete SQLite adapter.

## Where to look

| Responsibility | Source / local guide | Package name or boundary |
|---|---|---|
| Task lifecycle, prerequisites, evidence requirements | [task/src/lib.rs](./task/src/lib.rs) | Cargo package `task-domain`; fixed transition table in [state.rs](./task/src/state.rs) |
| Execution and Attempt snapshots | [agent-runtime/src/lib.rs](./agent-runtime/src/lib.rs) | Runtime identities/state, separate from Task lifecycle |
| Scheduling and evidence interpretation | [scheduler/src/lib.rs](./scheduler/src/lib.rs) | Owns `ExecutionPort`; does not persist decisions |
| Trusted lifecycle and integration services | [app-services/AGENTS.md](./app-services/AGENTS.md) | Canonical mutation, orchestration and authority checks |
| Planning graph, scope and proposal validation | [plan/AGENTS.md](./plan/AGENTS.md) | Cargo package `plan-domain` |
| Verification, observed effects, immutable candidate values | [evaluation/AGENTS.md](./evaluation/AGENTS.md) | `EvaluationPort` and capture contracts |
| Git/workspace contracts and ownership | [workspace/AGENTS.md](./workspace/AGENTS.md) | Domain values and ports; concrete Git work lives in adapters |
| Capabilities, approvals and test mutation policy | [policy/AGENTS.md](./policy/AGENTS.md) | Explicit authorization decisions |
| Isolation plan / backend contract | [runner/src/lib.rs](./runner/src/lib.rs) | Verifies policy-derived authority before process launch |
| Secret metadata, backend and release | [secrets/src/lib.rs](./secrets/src/lib.rs) | Separate `SecretMetadataStore`, `SecretBackend`, `SecretDelivery` |
| Security audit record contract | [security-audit/src/lib.rs](./security-audit/src/lib.rs) | Durable audit values, distinct from execution journal |
| IPC schemas / version capabilities | [ipc-contracts/src/lib.rs](./ipc-contracts/src/lib.rs) | Seven checked-in [proto files](./ipc-contracts/proto) |
| Handshake / negotiation / sessions | [ipc-control/src/lib.rs](./ipc-control/src/lib.rs) | Version/capability checks above byte transport |
| Framing / endpoints / peer identity | [ipc-transport/src/lib.rs](./ipc-transport/src/lib.rs) | Platform modules below control semantics |
| Adapter conformance runners | [execution-contract](./execution-contract/src/lib.rs), [planner-contract](./planner-contract/src/lib.rs), [effect-capture-contract](./effect-capture-contract/src/lib.rs) | Tests invoke these from adapter integration targets |
| Product build identity | [product-version/src/lib.rs](./product-version/src/lib.rs) | Reads the canonical [version file](../product/version.toml) |

## IPC distinctions

- [ipc-contracts/build.rs](./ipc-contracts/build.rs) uses vendored `protoc`
  and Prost to generate Rust into Cargo `OUT_DIR`. Edit `.proto` sources;
  generated Rust is disposable build output.
- [negotiation.rs](./ipc-control/src/negotiation.rs) owns negotiated version
  and capability checks. Handshake `client_kind` is descriptive metadata;
  peer authentication comes from transport and admission policy from daemon
  composition.
- [framing.rs](./ipc-transport/src/framing.rs) carries bounded bytes; it must
  not interpret Protobuf business requests. [peer.rs](./ipc-transport/src/platform/peer.rs)
  handles kernel/token-derived peer identity.
- Historical protocol versions remain separate capabilities; a newer
  request shape must not be smuggled into an older negotiated session.
  See the [IPC contract](../docs/architecture/ipc.md).

## Other boundaries

- `ExecutionPort` belongs to `scheduler`, `PlannerPort` to `plan-domain`,
  `EvaluationPort` and `EffectCapturePort` to `evaluation`. The similarly
  named `*-contract` packages contain reusable test runners, not port owners.
- [runner](./runner/src/lib.rs) consumes already-decided effective
  capabilities. [NativeRunner](../adapters/runner-native/src/lib.rs) offers
  no OS sandbox; [Seatbelt](../adapters/runner-macos-seatbelt/src/lib.rs)
  implements confinement. Required isolation cannot degrade to native execution.
- Secret values belong to a `SecretBackend`; SQLite owns references and
  bindings only. Use the [broker contract](./secrets/src/lib.rs), not a raw
  credential field in Task or provider evidence.
- Resolve stale module-header dependency claims against `Cargo.toml` and
  current implementation, especially when interpreting older gate comments.
