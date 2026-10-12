# AGENTS.md (compatibility entrypoint)

This repository's canonical coding-agent policy is `AGENT.md`.

Read `AGENT.md` before performing any work.

- Canonical policy: [`AGENT.md`](./AGENT.md)
- Architecture navigation: [`docs/architecture/INDEX.md`](./docs/architecture/INDEX.md)
- Project orchestration/planning mode: [`ORCHESTRATOR.md`](./ORCHESTRATOR.md)

If the current user request explicitly asks you to act as the project
orchestrator/planner, read `ORCHESTRATOR.md` after `AGENT.md`.  The
orchestrator contract is subordinate to the authority order in `AGENT.md`.

## Generated repository map

Derived navigation, generated 2026-09-30 from commit `5362f6e` on
`docs/init-deep-agent-guidance`. Recheck claims against linked sources when
those sources change; this map does not add to the canonical policy.

Runlith is a local Rust modular monolith with Ports & Adapters. The current
[Cargo workspace](./Cargo.toml) contains 30 packages: 18 shared crates,
10 adapters, and the `agentd` daemon / `agentctl` CLI. UI and cloud concepts
in architecture documents are not implemented workspace packages.

### Structure and routing

| Task | Start here | Purpose |
|---|---|---|
| Package ownership / shared APIs | [crates/AGENTS.md](./crates/AGENTS.md) | Domains, application services, IPC, reusable contract suites |
| Concrete I/O or provider integration | [adapters/AGENTS.md](./adapters/AGENTS.md) | SQLite, Git, provider, capture, secrets, runner implementations |
| Daemon composition / CLI requests | [bins/AGENTS.md](./bins/AGENTS.md) | Startup, admission, negotiated request routing, process tests |
| Architecture / binding contracts | [architecture index](./docs/architecture/INDEX.md) | Load only the contracts relevant to the task |
| Development gate / task definition | [roadmap](./planning/roadmap.yaml), [queue](./planning/tasks/queue.yaml) | Versioned scope, dependencies, acceptance and verification |
| Product version identity | [product/version.toml](./product/version.toml) | Policy: [VERSIONING.md](./docs/development/VERSIONING.md) |
| JSON representation schemas | [task](./contracts/json-schema/task.schema.json), [event](./contracts/json-schema/event.schema.json) | Distinct from the Protobuf IPC contract |
| Prior gate evidence | [docs/development](./docs/development) | Historical audits and remediation records; check source freshness |

### Code map

Declaration/import shapes were checked with Rust ast-grep; outlines and the
three numeric reference counts below were checked with Rust LSP. Counts
include test references, exclude declarations, and describe this baseline.
`Unmeasured` does not mean zero.

| Symbol | Location | References | Role |
|---|---|---|---|
| `agentd::run` | [daemon library](./bins/agentd/src/lib.rs) | Unmeasured | Local listener and service composition |
| `agentctl::execution_request` | [CLI library](./bins/agentctl/src/lib.rs) | Unmeasured | Negotiated execution IPC client |
| `TaskService` | [task_service.rs](./crates/app-services/src/task_service.rs) | 85 | Trusted Task lifecycle mutation |
| `OrchestrationService` | [orchestration_service.rs](./crates/app-services/src/orchestration_service.rs) | Unmeasured | Connect scheduling, workspace admission, execution and evaluation |
| `ExecutionPort` | [execution_port.rs](./crates/scheduler/src/execution_port.rs) | 46 | Provider-neutral execution seam |
| `PlannerPort` | [model.rs](./crates/plan/src/model.rs) | 41 | Planning proposals without durable mutation authority |
| `EvaluationPort` | [port.rs](./crates/evaluation/src/port.rs) | Unmeasured | Independent verification verdicts |
| `SqliteStore` | [task_store.rs](./adapters/storage-sqlite/src/task_store.rs) | Unmeasured | Durable state and append-only journal boundary |

### Verification commands

Run from the repository root. Package-local integration targets live under
each package's `tests/`; unit tests also live beside implementations.

```sh
cargo test --locked -p task-domain
cargo test --locked -p plan-domain --test validation_rules
cargo build --locked --bin agentd --bin agentctl
cargo test --locked -p agentctl --test execution_binaries
```

The binary build is required for tests that locate sibling executables;
match the test profile and target to avoid exercising stale binaries.

[CI](./.github/workflows/verification.yml) runs the following checks on macOS
with Rust 1.95.0. Its report is subsequent evidence, not Task acceptance.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
```

Live Claude smokes are ignored and require `RUNLITH_CLAUDE_SMOKE=1` plus an
authenticated CLI; deterministic fixture tests do not establish live-provider
support. Read the local provider guide before selecting a smoke test.
