# Binaries - derived navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived guidance only; canonical policy is [AGENT.md](../AGENT.md).
This unified guide covers both binaries; package manifests are
[agentd](agentd/Cargo.toml) and [agentctl](agentctl/Cargo.toml).

## Composition and dispatch

| Surface | Primary source | Role |
| --- | --- | --- |
| Daemon entry | [agentd main](agentd/src/main.rs) | Data path resolution, execution-enabled configuration, internal candidate subcommands. |
| Daemon composition | [agentd library](agentd/src/lib.rs) | Opens services, authenticates peers, negotiates sessions, routes READY requests. |
| Task dispatch | [dispatch](agentd/src/dispatch.rs) | Typed Task and authoring DTOs enter application services; errors are bounded and sanitized. |
| Execution wiring | [execution](agentd/src/execution.rs) | Provider, Git, policy, runner and candidate evaluation composition. |
| Control helpers | [approval](agentd/src/approval.rs), [human decision](agentd/src/human_decision.rs), [planning](agentd/src/planning.rs) | Separate typed control routes and service wiring. |
| Durable location | [data directory](agentd/src/data_dir.rs) | Canonical database location and validated override. |
| CLI shell | [agentctl main](agentctl/src/main.rs) | Argument parsing, rendering, exit status and execution wait mode. |
| Negotiated clients | [agentctl library](agentctl/src/lib.rs) | Connect-first launch, fresh handshakes, versioned request exchange. |

Protocol contracts live in [ipc-contracts](../crates/ipc-contracts/src/lib.rs);
session admission lives in [ipc-control](../crates/ipc-control/src/lib.rs).
The daemon dispatchers call [application services](../crates/app-services/src/lib.rs),
which own durable transitions and journal evidence.

## Local traps

- In [agentctl clients](agentctl/src/lib.rs), reconnect means a new transport
  and handshake. A lost, malformed or timed-out mutation reply is ambiguous;
  business, execution, approval, human-decision and planning mutations are not replayed.
- [CLI wait mode](agentctl/src/main.rs) re-probes execution status after an
  ambiguous start instead of issuing `run` again.
- [Candidate helpers](agentd/src/main.rs) dispatch `__candidate-snapshot-write`,
  `__candidate-snapshot-remove` and `__candidate-evaluate` before ordinary startup.
  Their caller and `candidate_test_evaluator_executable` are in
  [execution](agentd/src/execution.rs); snapshot mechanics are in
  [candidate_snapshot](../adapters/effect-capture-local/src/candidate_snapshot.rs).
- [Process tests](agentctl/tests/real_process_matrix.rs) use `workspace_bin`
  to locate sibling executables under `target/<profile>`. Candidate library
  tests override the evaluator with real `agentd`: the test harness has no
  hidden candidate dispatch. Existing binaries may be stale.

## Targeted verification from repository root

Freshly build both siblings before agentctl process or agentd candidate tests:

```sh
rtk cargo build --locked --bin agentd --bin agentctl
rtk cargo test --locked -p agentctl --test real_process_matrix
rtk cargo test --locked -p agentctl --test execution_binaries
rtk cargo test --locked -p agentd --lib candidate_evaluator_admits_bound_evidence_without_completing_the_task
```

Target sources: [process matrix](agentctl/tests/real_process_matrix.rs),
[execution binaries](agentctl/tests/execution_binaries.rs),
[candidate scenario](agentd/src/execution.rs).
For mutation retry changes, use `rtk cargo test --locked -p agentctl --test business_no_replay`
([single-send ambiguity assertion](agentctl/tests/business_no_replay.rs)).
These are source-backed verification entry points, not a record of executed tests.
