# G6 Provider Environment Isolation and H7 Closure (TASK-0065)

> Implementation/evidence record for TASK-0065.
>
> Target development version: `0.1.0-dev.task0065`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`.

## Canonical scope and acceptance

TASK-0065 owns the provider-process environment half of G6 H7:

- move provider-writable HOME/XDG/data/state/cache/scratch/temp activity away
  from managed Git worktrees where the provider permits it;
- clear ambient inherited environment and rebuild only the variables required
  for the provider process to start and authenticate;
- keep provider adapters replaceable executors rather than policy authorities;
- fail before spawn when mandatory environment isolation cannot be prepared;
- prove real Claude and deterministic fixtures no longer create
  `data/sessions.db` or equivalent provider state in the managed worktree;
- preserve normal workspace cleanup/recovery and historical `WorkspaceLease`
  identity semantics.

Out of scope: Security Audit persistence (TASK-0066), full production
Policy/approval/Secrets/Runner integration and adversarial G6 matrix
(TASK-0067), G6 closure audit (TASK-0068), concrete OS/container sandbox
backends, remote/cloud workers, team RBAC and a second provider.

## H7 reproduction before remediation

The accepted G5 evidence already contains a real-provider reproduction of the
defect. `G5_CLOSURE_REAUDIT.md` recorded Claude Code `2.1.268` producing:

```text
?? data/sessions.db
```

inside the managed worktree. Ordinary cleanup then refused the dirty worktree
with `WorktreeDirty`, intentionally preserving user/provider-created data.
This is the canonical RED baseline for H7; historical G5 audit files remain
unchanged.

The TASK-0065 deterministic fixture keeps the same failure shape: provider
state named `sessions.db` is written through the provider data location. The
post-remediation contract requires that location to resolve outside the
managed worktree and verifies Git status stays clean.

## Provider environment architecture

The Claude adapter now owns a bounded `ProviderEnvironment` launch object.
Production composition derives the provider base from the existing database
parent:

```text
<app-state>/provider-environments/claude-code/<managed-worktree-leaf>/
  config/
  data/
  state/
  cache/
  scratch/
```

The worktree leaf comes from the existing `WorkspaceLease` path. No provider
identifier is added to Task/Execution/Attempt/Workspace domain state and no
new durable workspace identity is created. Provider runtime data therefore
survives independently of Git worktree teardown while workspace recovery keeps
its existing ownership rules.

The environment base and managed worktree must both be absolute. The runtime
root must not overlap the worktree in either direction. Existing symlinks or
non-directory environment paths are rejected. On Unix, created environment
directories are mode `0700`; an existing path with broader permissions is
rejected rather than silently accepted.

## HOME, XDG, cache, state, scratch and temp semantics

Before spawn the adapter calls `env_clear()` and constructs the child
environment explicitly:

```text
PATH                  <- explicit host launch path
HOME                  <- provider-owned authentication anchor (Unix)
PWD                   <- exact managed worktree
XDG_CONFIG_HOME       <- isolated config/
XDG_DATA_HOME         <- isolated data/
XDG_STATE_HOME        <- isolated state/
XDG_CACHE_HOME        <- isolated cache/
CLAUDE_CODE_TMPDIR    <- isolated scratch/
TMPDIR / TMP / TEMP   <- isolated scratch/
LANG / LC_ALL / LC_CTYPE <- selected locale only when present
```

On Windows, `USERPROFILE` remains the provider-owned authentication/profile
anchor, `SystemRoot`/`WINDIR` and `PATHEXT` are retained for process startup,
and `APPDATA` / `LOCALAPPDATA` point at isolated config/cache directories.

The current Claude CLI cannot authenticate with a completely synthetic HOME:
a direct TASK-0065 probe using an empty isolated HOME returned `Not logged in ·
Please run /login`. Setting `CLAUDE_CONFIG_DIR` to an empty isolated directory
also removed the existing subscription login. Runlith therefore does not copy,
parse or mirror provider credentials; the real HOME/profile remains an
explicit provider-owned auth anchor. This is outside the managed worktree and
is not canonical application state. Stronger filesystem confinement of that
anchor belongs to a real OS isolation backend, not this task.

The Claude launch also uses `--safe-mode`. It disables user/project
customizations, plugins, hooks, MCP configuration and similar provider-native
extensions as defense in depth. Capability discovery now accounts for the
emitted flag. Provider-native safe mode grants no Runlith authority.

## Sanitized environment and secret handling

Environment inheritance is allowlist-only. Ambient API keys, cloud provider
credentials, Git overrides, SSH agent variables and arbitrary provider/plugin
variables are absent from the child environment. The H7 fixture asserts an
ambient Cargo variable does not cross the boundary.

No secret value is added to `ProviderEnvironment`, SQLite, Event Journal,
ordinary logs, frontend state or model context. TASK-0063 remains the only
canonical product path for `SecretRef` release after current policy/approval
authorization. Provider-native subscription login stays provider-owned and is
never treated as `SecretAccess` authority.

## Policy, Runner and provider trust boundary

Provider environment preparation is mechanical launch hygiene:

```text
Policy / approval -> EffectiveCapabilities -> Runner requirements
                                      |
                                      +-> provider launch environment hygiene
```

`ProviderEnvironment` cannot evaluate policy, approve an operation, widen
`EffectiveCapabilities`, release a secret, authorize a tool or satisfy a
Runner isolation axis. TASK-0064 `NativeRunner` remains `Trusted Host` with
zero filesystem/network/process sandbox axes. If stronger OS isolation is
required, the Runner boundary must still fail closed.

The shared `ExecutionPort` contract remains the provider-neutral execution
seam. TASK-0065 does not add Claude types to Task, Workspace, Storage, IPC or
completion authority.

## H7 closure evidence

Deterministic provider fixture:

- provider `sessions.db` appears under isolated `XDG_DATA_HOME`;
- no `data/sessions.db` appears under the managed worktree;
- ambient environment marker is absent;
- an environment root inside the worktree returns
  `environment_isolation` before provider spawn (marker file proves no child
  launched).

Workspace integration:

- provider-shaped execution writes `sessions.db` only under the external
  provider environment root;
- `git status --porcelain` for the managed worktree is empty;
- ordinary `WorkspaceService::cleanup_workspace` succeeds and transitions the
  lease to `Removed`;
- provider runtime state remains separate from Git cleanup.

Real Claude keeper (`RUNLITH_CLAUDE_SMOKE=1`) passes with the sanitized launch
environment and asserts the temporary Git worktree remains clean after the
real subscription-authenticated execution.

## Targeted verification obtained before canonical promotion

```text
cargo test -p provider-claude --no-fail-fast
72 passed; 0 failed; 1 ignored; 5 suites

RUNLITH_CLAUDE_SMOKE=1 cargo test -p provider-claude --test smoke
1 passed; 0 failed

cargo test -p app-services --test provider_binding
7 passed; 0 failed

cargo test -p app-services --test workspace_diff_cleanup
20 passed; 0 failed

cargo test -p agentd --no-fail-fast
58 passed; 0 failed

cargo clippy -p provider-claude -p agentd -p app-services \
  --all-targets --all-features -- -D warnings
PASS

cargo check -p provider-claude --target x86_64-pc-windows-msvc
PASS
```

The first targeted Clippy pass found only an eight-argument helper seam. It
was refactored into a typed invocation structure; no lint allow was added.

An attempted combined Windows cross-check including `agentd` reached the host
C toolchain while building `libsqlite3-sys` and failed because the macOS host
compiler could not provide MSVC target headers (`stdlib.h` missing). The
provider adapter itself cross-checks successfully for
`x86_64-pc-windows-msvc`; this record does not claim a full `agentd` Windows
cross-build.

## Final pre-merge verification

After implementation, canonical docs, queue and version promotion, the first
full mandatory pass on the completed tree was green:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS (5 crates compiled in the cached run)

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS (no issues)

cargo test --workspace --no-fail-fast
802 passed; 0 failed; 1 ignored; 92 suites; 81.18s

cargo check -p provider-claude --target x86_64-pc-windows-msvc
PASS
```

One final exact mandatory suite is rerun on the unchanged completed tree after
this evidence record is frozen; that run is the merge-acceptance evidence and
is intentionally not rewritten here for timing-only output.

## Post-merge verification

Pending normal merge and mandatory verification on `main`.

## TEST-H history

No TEST-H/TEST-H2 failure reproduced. During a targeted `cargo test -p agentd
--no-fail-fast`, `tests::ninth_concurrent_client_is_closed_not_fatal` failed
once with `MalformedFrame("connection closed before a frame completed")` while
the remaining agentd suites completed. No task-owned `agentd` process remained.
The exact test rerun immediately passed `1 passed; 57 filtered out`, and a
fresh full `cargo test -p agentd --no-fail-fast` then passed `58 passed; 0
failed; 5 suites`. The subsequent full workspace pre-merge pass was also green,
so this is recorded as transient test concurrency evidence rather than a
TASK-0065 product failure.

## Gate status and deferred work

TASK-0065 closes H7 worktree pollution but does not claim the provider process
is OS-sandboxed. Residual visibility of the provider-owned HOME/profile auth
anchor is constrained only by the selected Runner backend; concrete platform
sandbox containment remains separate accepted work.

G5 remains `CLOSED`. G6 remains `IN_PROGRESS`. After TASK-0065 is accepted in
`main`, the next canonical task is TASK-0066 — Security Audit Persistence,
target `0.1.0-dev.task0066`, recommended GPT-5.6 Sol / High unless current
canonical docs change. TASK-0066 is not started here.
