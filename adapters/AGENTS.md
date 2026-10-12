# Adapters - Derived Navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived navigation only; canonical policy is [AGENT.md](../AGENT.md).

## Package and port map

| Package / implementation | Owning contract and local entry |
| --- | --- |
| `storage-sqlite` | [WorkspaceStore](../crates/workspace/src/store.rs), [SecretMetadataStore](../crates/secrets/src/lib.rs); [SqliteStore](storage-sqlite/src/lib.rs) also exposes concrete persistence APIs. See [local map](storage-sqlite/AGENTS.md). |
| `provider-claude` | [scheduler::ExecutionPort](../crates/scheduler/src/execution_port.rs) and [plan_domain::PlannerPort](../crates/plan/src/model.rs); [executor](provider-claude/src/executor.rs), [planner](provider-claude/src/planner.rs), [local map](provider-claude/AGENTS.md). |
| `git-cli` | [workspace::GitPort](../crates/workspace/src/port.rs); [GitCli](git-cli/src/lib.rs), [local map](git-cli/AGENTS.md). |
| `fake-agent` | [FakeAgent](fake-agent/src/agent.rs) is bridged to [scheduler::ExecutionPort](../crates/scheduler/src/execution_port.rs) by the test-side [ContractFakeExecutor](fake-agent/tests/execution_port_contract.rs), not a production trait impl. |
| `fake-planner` | [plan_domain::PlannerPort](../crates/plan/src/model.rs); [FakePlanner](fake-planner/src/lib.rs). |
| `fake-evaluator` | [evaluation::EvaluationPort](../crates/evaluation/src/port.rs); [FakeEvaluator](fake-evaluator/src/lib.rs). |
| `effect-capture-local` | [evaluation::EffectCapturePort and ProcessExitObservationSink](../crates/evaluation/src/capture.rs); [LocalEffectCapture](effect-capture-local/src/lib.rs). |
| `keychain-native` | [secrets::SecretBackend](../crates/secrets/src/lib.rs); [NativeKeychain](keychain-native/src/lib.rs). |
| `runner-native` | [runner::IsolationBackend](../crates/runner/src/lib.rs); [NativeRunner](runner-native/src/lib.rs). |
| `runner-macos-seatbelt` | [runner::IsolationBackend](../crates/runner/src/lib.rs); [MacOsSeatbeltRunner](runner-macos-seatbelt/src/lib.rs), compiled only on macOS. |

## Shared suites and their invocation sites

The suites live under `crates/`; adapter integration targets supply implementations.
Commands below run from the repository root.

| Suite source | Adapter integration targets |
| --- | --- |
| [execution-contract](../crates/execution-contract/src/lib.rs) | [fake-agent](fake-agent/tests/execution_port_contract.rs): `rtk cargo test -p fake-agent --test execution_port_contract`; [Claude](provider-claude/tests/execution_port_contract.rs): `rtk cargo test -p provider-claude --test execution_port_contract`. |
| [planner-contract](../crates/planner-contract/src/lib.rs) | [fake-planner](fake-planner/tests/planner_port_contract.rs): `rtk cargo test -p fake-planner --test planner_port_contract`; [Claude](provider-claude/tests/planner_port_contract.rs): `rtk cargo test -p provider-claude --test planner_port_contract`. |
| [effect-capture-contract](../crates/effect-capture-contract/src/lib.rs) | [local capture](effect-capture-local/tests/capture_contract.rs): `rtk cargo test -p effect-capture-local --test capture_contract`. |

## Other focused entry points

- [Fake scenarios](fake-agent/tests/scenario_matrix.rs): `rtk cargo test -p fake-agent --test scenario_matrix`.
- [Evaluator fixtures](fake-evaluator/src/lib.rs): `rtk cargo test -p fake-evaluator --lib`.
- [Credential driver tests](keychain-native/src/lib.rs): `rtk cargo test -p keychain-native --lib`.
- [Native runner tests](runner-native/tests/native_runner.rs): `rtk cargo test -p runner-native --test native_runner`.
- [Seatbelt tests](runner-macos-seatbelt/src/lib.rs): `rtk cargo test -p runner-macos-seatbelt --lib` on macOS.

The [native backend](runner-native/src/lib.rs) reports trusted-host execution,
with explicit child environment isolation; it does not supply OS sandbox restrictions.
The [Seatbelt backend](runner-macos-seatbelt/src/lib.rs) constructs the sandboxed launch recipe.
