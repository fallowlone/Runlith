# G6 Production Runtime Isolation Remediation (TASK-0069)

> TASK-0069 remediates TASK-0068 blocker B-ISO-1. It does **not** close G6;
> TASK-0070 remains the independent closure re-audit authority.

## Result

Production autonomous Claude execution no longer asks Runner for Trusted Host.
`SecuredClaudeExecutor` requires filesystem, network and process isolation and,
on macOS, uses `adapters/runner-macos-seatbelt` to enter the provider through
`/usr/bin/sandbox-exec`. `runner-native` still reports zero sandbox axes, so a
platform without a concrete backend cannot satisfy the production requirement
and fails before the provider operation.

The effective production authority is currently:

```text
FilesystemRead(current managed worktree, ".")
FilesystemWrite(current managed worktree, ".")
ProcessExecute(canonical Claude executable)
NetworkAccess("tcp:443")
```

The macOS backend maps that authority to real Seatbelt restrictions. Provider
runtime state prepared by TASK-0065 is an explicit non-task support root:
read/write is confined to the isolated provider runtime directory, while the
existing `~/.claude` and `~/.claude.json` authentication anchors are read-only.
Standard immutable macOS process/runtime support comes from the Seatbelt system
profile and does not grant task workspace mutation authority.

The current network contract intentionally matches the concrete primitive that
is proved here: outbound remote TCP on port 443 plus the local macOS resolver
socket. Arbitrary hostnames/ports or other requested network scopes are not
silently approximated; the backend rejects unsupported scopes before spawn.

## Adversarial evidence

The Seatbelt adapter keepers exercise the real OS boundary rather than a mock:

- authorized in-workspace read/write succeeds;
- direct read/write outside the authorized worktree fails;
- a symlink inside the worktree targeting an outside file cannot escape the
  read boundary;
- an authorized process cannot `exec` an unauthorized child binary;
- a local TCP listener on an unauthorized port receives no connection;
- a network scope the backend cannot represent is rejected before spawn.

Fresh task verification:

```text
cargo test -p runner-macos-seatbelt
4 passed; 0 failed

cargo test -p agentd provider_launch_emits_g6_security_audit
1 passed; 0 failed

RUNLITH_CLAUDE_SMOKE=1 \
  cargo test -p agentd real_claude_runs_through_production_g6_isolation \
  -- --ignored --nocapture
1 passed; 0 failed

cargo test --workspace --no-fail-fast -- --test-threads=1
825 passed; 0 failed; 2 ignored

cargo clippy --workspace --all-targets -- -D warnings
PASS
```

The installed Claude CLI reaches its remote provider under the production
Seatbelt path but currently reports an `unrecognized_model` provider error.
The identical error is reproduced by invoking the same Claude CLI directly
outside Runlith, so it is not classified as a sandbox denial. The production
keeper rejects EPERM/`operation not permitted`, `sandbox-exec` and runtime
launch-preparation failures and verifies correlated isolation audit evidence.

Several pre-existing G5 slow-provider fixtures previously used external
`sleep` binaries. Exact process confinement correctly rejected those child
execs, so the fixtures now stay slow using a shell builtin loop instead of
weakening the production process allowlist. Their cancellation/live-owner
keepers pass after that correction.

## Scope and remaining work

TASK-0069 does not implement Linux or Windows sandbox backends, a general
hostname-aware egress proxy, VM/container isolation, G7 work, or a G6 closure
verdict. The architectural fail-closed rule makes those omissions explicit:
unsupported mandatory confinement is refusal, never NativeRunner fallback.

The next canonical task is TASK-0070 — Independent Adversarial G6 Closure
Re-Audit. It must re-test B-ISO-1 and the complete G6 acceptance set before any
`G6_CLOSED` claim.
