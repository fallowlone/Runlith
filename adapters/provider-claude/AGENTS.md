# Claude Provider - Derived Navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived navigation under [adapters](../AGENTS.md); canonical policy: [AGENT.md](../../AGENT.md).

## Source map

| Concern | Source and contract |
| --- | --- |
| Execution binding and canonical evidence | [executor.rs](src/executor.rs), [scheduler::ExecutionPort](../../crates/scheduler/src/execution_port.rs) |
| Atomicity, decomposition, replan proposals | [planner.rs](src/planner.rs), [plan_domain::PlannerPort](../../crates/plan/src/model.rs) |
| CLI arguments, role and task prompt | [argv.rs](src/argv.rs), [config.rs](src/config.rs), [prompt.rs](src/prompt.rs) |
| Child ownership and liveness | [process.rs](src/process.rs), [tracker.rs](src/tracker.rs), [OwnerLiveness](../../crates/workspace/src/liveness.rs) |
| Structured output and failures | [output.rs](src/output.rs), [json.rs](src/json.rs), [failure.rs](src/failure.rs) |
| Controlled child environment | [environment.rs](src/environment.rs), [Runner launch contract](../../crates/runner/src/lib.rs) |
| Installation, capabilities, sign-in status | [detect.rs](src/detect.rs), [capabilities.rs](src/capabilities.rs), [authentication.rs](src/authentication.rs) |

## Local details

- [Argument construction](src/argv.rs) uses direct argv with `-p --safe-mode` and JSON output.
  Proposal-only `--tools ""` and `--disallowedTools "mcp__*"` precede `--output-format`:
  those variadic flags would otherwise swallow the positional prompt.
- [Configuration](src/config.rs) rejects permission-mode strings containing `bypass`, case-insensitively.
  [Planner configuration](src/planner.rs) forces proposal-only tool removal and its role prompt.
- [Process supervision](src/process.rs) owns cancellation, timeout, bounded stream capture and
  capture-finalization grace; governed execution consumes a verified Runner launch.
- [Environment preparation](src/environment.rs) puts runtime/XDG state outside the managed worktree
  and clears ambient variables before installing its explicit set. Unix HOME and USER are required
  for provider-owned authentication; environment control alone is not an OS sandbox.
- [Authentication probing](src/authentication.rs) asks `claude auth status` and classifies `loggedIn`;
  missing CLI or unusable probe output is `Unknown`, not proof of a signed-out session.
- [Output translation](src/output.rs) rejects malformed critical records and provider-declared errors.
  [UsageSample](src/usage.rs) keeps absent measurements unknown; cache token classes count toward input.
  [Planner spend](src/planner.rs) is `NotStarted` before launch, or `Unmeasured` when a started call
  lacks usable usage, rather than inventing zero cost.

## Existing verification targets

Run from the repository root; package identity is [provider-claude](Cargo.toml).

- `rtk cargo test -p provider-claude --lib` covers local configuration, parsing and environment tests.
- `rtk cargo test -p provider-claude --test process_fixtures` runs [Unix fake executables](tests/process_fixtures.rs).
- `rtk cargo test -p provider-claude --test execution_port_contract` invokes the
  [shared execution suite](../../crates/execution-contract/src/lib.rs) through [adapter fixtures](tests/execution_port_contract.rs).
- `rtk cargo test -p provider-claude --test planner_port_contract` invokes the
  [shared planner suite](../../crates/planner-contract/src/lib.rs) through [adapter fixtures](tests/planner_port_contract.rs).

Those integration fixtures are deterministic and Unix-only, without live authentication or network.
The [real-provider smoke target](tests/smoke.rs) is ignored and requires explicit opt-in:
`rtk proxy env RUNLITH_CLAUDE_SMOKE=1 cargo test -p provider-claude --test smoke -- --ignored --nocapture`.
Its early skip paths mean a green test exit alone does not prove a real provider call occurred.
