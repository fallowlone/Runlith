# G6 Integration and Adversarial Security Verification (TASK-0067)

> Implementation/evidence record for TASK-0067.
>
> Development version: `0.1.0-dev.task0067`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`; TASK-0068 owns independent
> closure.

## Canonical scope and acceptance

TASK-0067 integrates the G6 foundations delivered by TASK-0061..0066 at the
current privileged execution boundaries and exercises the deterministic /
adversarial failure matrix required by `planning/tasks/queue.yaml`:

- current production provider launch must receive a canonical Policy Engine
  decision and exact effective authority before invocation;
- Runner enforcement must occur immediately around the process operation and
  refuse unsupported isolation, exact-target mismatch and cross-workspace
  authority before the callback runs;
- durable approvals must remain exact-bound, survive restart and refuse stale,
  rebound, widened or replayed use;
- Secrets Broker release must require current exact authority and target
  binding before backend material is loaded or delivered;
- provider environment isolation from TASK-0065 must remain effective and a
  real subscription-authenticated Claude run must keep its worktree clean;
- trusted product actions must emit bounded correlated Security Audit evidence,
  with approval mutation + audit persistence sharing one SQLite transaction;
- repository/model/provider data cannot create authority, approve itself,
  release secrets or forge canonical Security Audit facts;
- workspace and existing G5 behavior must remain green.

Out of scope: G6 closure (TASK-0068), a concrete macOS/Linux/Windows/container
sandbox backend, cloud/team RBAC, remote workers, UI/analytics/enterprise audit,
N-1/N-2/F-SCAN/TEST-H/TEST-H2/Git-commondir/H9 unless independently reproduced
as a TASK-0067 product defect, and G7+ work.

## Production integration

`crates/app-services/src/security_service.rs` adds the trusted G6 composition
surface. Policy remains the sole authorization authority; the service only
composes already-accepted Policy/Approval/Runner/Secrets/Audit contracts.

The production `agentd` Claude boundary is wrapped by `SecuredClaudeExecutor`:

```text
trusted Task/Execution/Attempt/Workspace state
  -> exact PolicyRequest + parent authority
  -> PolicyEngine
  -> EffectiveCapabilities
  -> exact Runner process/workspace recheck
  -> NativeRunner Trusted Host enforcement
  -> provider invocation
```

The current local provider launch is an explicit Trusted Host operation.
`NativeRunner` reports zero filesystem/network/process sandbox axes. Any future
request that requires one of those axes still fails closed; TASK-0067 does not
pretend NativeRunner is a sandbox or add a speculative backend.

Provider-environment construction stays the TASK-0065 boundary. The secured
wrapper does not move provider-owned authentication into Runlith state and does
not add provider/model text, environment dumps or secret values to Security
Audit.

## Security Audit integration and transaction boundaries

Policy request/decision/effective-authority, Runner enforcement/refusal and
provider invocation records are authored by trusted product code with the same
operation correlation plus Task/Execution/Attempt provenance.

Approval registration, human decision and single-use consumption now append
their Security Audit record inside the same SQLite transaction as the approval
state mutation. `audit_store` exposes the existing append primitive to an
already-open transaction rather than introducing another persistence model.

Secrets integration resolves only the opaque `SecretBinding` metadata needed
for the audit subject. Unauthorized or target-mismatched release is rejected by
the Secrets Broker before backend material can be delivered. The audit schema
still has no field capable of carrying secret bytes or provider free-form text.

External process execution and SQLite cannot be one atomic transaction. The
current boundary therefore commits canonical policy/enforcement and
provider-requested evidence before invoking the external process, then appends
the trusted observed provider outcome afterward. TASK-0067 makes no stronger
atomicity claim.

## Production-path defect: RED -> GREEN

The first keeper added directly against the shipped `agentd` composition was:

```text
provider_launch_emits_g6_security_audit
```

RED on the TASK-0066 tree: the provider fixture executed successfully and the
attempt reached `SUCCEEDED_AWAITING_OPERATOR_VERIFICATION`, but the keeper then
failed with:

```text
production provider launch must record the canonical policy decision
```

Root cause: the production `agentd -> ClaudeExecutor` path bypassed the G6
Policy/Runner/Security Audit integration even though each subsystem existed in
isolation.

GREEN after the fix: the same keeper passes and observes correlated
`PolicyDecision`, `IsolationEnforcement` and `ProviderInvocation` records for
the production execution. No provider adapter was made authoritative; the
integration remains in the product composition boundary.

## Adversarial scenario matrix

| Scenario | Evidence | What it proves |
|---|---|---|
| Trusted provider launch | `policy_runner_and_audit_form_one_trusted_launch_boundary` + production keeper | Policy -> effective authority -> Runner -> provider is one product-controlled boundary with correlated audit |
| Unsupported isolation | `unsupported_isolation_fails_closed_before_privileged_operation` | NativeRunner cannot silently downgrade a stronger filesystem isolation requirement; callback is not invoked |
| Child/cross-workspace escalation | `child_and_cross_workspace_authority_expansion_fail_closed` | requested authority cannot exceed parent capability or cross workspace |
| Repository self-grant | `repository_allow_cannot_create_privileged_authority` | repository-layer `ALLOW` cannot manufacture positive authority |
| Exact target / TOCTOU recheck | `runner_revalidates_exact_process_target_adjacent_to_operation` | authority for `claude` cannot be reused to execute another process target; refusal occurs adjacent to the callback |
| Approval restart + replay | `approval_survives_restart_consumes_once_and_replay_is_refused` plus existing `widened_or_rebound_replay_is_refused` | approval survives reopen, must match current exact binding and policy fingerprint, consumes once and cannot be replayed/widened |
| Secret authority/leakage | `secret_release_requires_exact_authority_and_audits_only_opaque_reference` plus existing broker target-binding tests | unauthorized release performs no backend load; authorized audit stores only opaque ref/capability identity; SQLite does not contain generated secret bytes |
| H7 provider state | provider process fixtures + `provider_binding` + real Claude smoke | provider writable state stays outside managed worktree; unsafe environment roots refuse before spawn; real provider leaves Git clean |

Existing lower-level contract tests remain part of the evidence rather than
being reimplemented in this suite: Policy repository/session non-widening,
approval rebound/expiry/single-use checks, Runner stale/widened report refusal,
Secrets Broker exact target binding and Security Audit tamper evidence.

## Restart, replay, cross-workspace and correlation evidence

The approval integration test closes and reopens the database between request,
human decision, consume and replay. The authorized operation succeeds once;
the next attempt with the same durable approval is refused.

Cross-workspace/process-target requests are rejected before the privileged
callback. Repository policy remains restriction-only and cannot self-authorize.

The integrated audit tests assert one operation correlation across policy,
effective authority, Runner and provider evidence. Approval records carry the
approval request identity and policy fingerprint. Secret audit records identify
only the opaque `SecretRef`/capability; the exact delivery target is enforced
by the broker before backend read and is not a v1 Security Audit field.

## H7 and real-provider evidence

Deterministic provider-environment fixtures remain green, including refusal of
an environment root inside the worktree before spawn. Existing provider/workspace
integration proves external provider state does not block normal workspace
cleanup.

The env-gated live keeper ran against installed Claude Code `2.1.268` with the
existing subscription-authenticated CLI:

```text
RUNLITH_CLAUDE_SMOKE=1 cargo test -p provider-claude --test smoke -- --ignored --nocapture
1 passed; 0 failed
```

The keeper itself performs `git status --porcelain` after execution and requires
an empty result, so the real provider did not reintroduce H7 worktree pollution.

## Targeted verification

Final targeted results before canonical promotion:

```text
cargo test -p app-services --test g6_security_integration
7 passed; 0 failed; 1 suite

cargo test -p agentd provider_launch_emits_g6_security_audit
1 passed; 0 failed; 58 filtered out

cargo test -p app-services --no-fail-fast
207 passed; 0 failed; 19 suites

cargo test -p storage-sqlite --no-fail-fast
110 passed; 0 failed; 9 suites

cargo test -p agentd --no-fail-fast
59 passed; 0 failed; 5 suites

cargo test -p policy -p runner -p runner-native -p secrets -p security-audit --no-fail-fast
60 passed; 0 failed; 13 suites

cargo test -p provider-claude --no-fail-fast
72 passed; 0 failed; 1 ignored; 5 suites

cargo test -p provider-claude --test process_fixtures
15 passed; 0 failed; 1 suite

cargo test -p app-services --test provider_binding
7 passed; 0 failed; 1 suite

cargo test -p app-services --test workspace_diff_cleanup
20 passed; 0 failed; 1 suite
```

TEST-H-family process overlap reproduced once during the first exact full
mandatory rerun. `binary_concurrent_resumes_elect_a_single_owner` observed an
otherwise honest strand still `RUNNING live=true` and panicked before its
normal cleanup; the later `g1_real_process_matrix` then observed three daemon
processes instead of one. Process inspection after the suite found no
TASK-0067 test-owned `agentd` left running (only unrelated macOS processes whose
names end in `agentd`). Both failed keepers then passed independently on quiet
state with one test thread:

```text
cargo test -p agentctl --test execution_binaries \
  live_ownership::binary_concurrent_resumes_elect_a_single_owner \
  -- --exact --nocapture --test-threads=1
1 passed; 0 failed

cargo test -p agentctl --test real_process_matrix g1_real_process_matrix \
  -- --exact --nocapture --test-threads=1
1 passed; 0 failed
```

This is retained as TEST-H-family process-test interference, not a confirmed
product regression. The final full suite still must pass on quiet state.

## Final merge verification

The first full candidate pass after code/docs/version promotion is green:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS - No issues found

cargo test --workspace --no-fail-fast
821 passed; 0 failed; 1 ignored; 96 suites

cargo check -p provider-claude --target x86_64-pc-windows-msvc
PASS
```

The first Clippy candidate exposed a nine-argument G6 process helper. It was
refactored to a typed `ProcessSecuritySpec` rather than suppressed. A second
candidate found one unnecessary test-only clone; that was removed. The green
Clippy result above is after both fixes.

One exact mandatory suite is rerun on the unchanged finalized tree after this
record is frozen. That rerun is the authoritative pre-merge evidence and is
reported with the implementation/merge commits; post-merge verification runs
again on `main`. TASK-0067 is not accepted while either suite is red.

## Gate state and deferred work

TASK-0067 advances the development identity to `0.1.0-dev.task0067` and
`completed_through: TASK-0067`. G5 remains `CLOSED`; G6 remains `IN_PROGRESS`.

The next canonical recommendation is TASK-0068 — Independent Adversarial G6
Closure Audit, expected development version `0.1.0-dev.task0068`, recommended
GPT-5.6 Sol / High unless current repository guidance changes. TASK-0068 is not
started by TASK-0067.
