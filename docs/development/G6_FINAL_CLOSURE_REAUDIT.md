# G6 Final Independent Adversarial Closure Re-Audit (TASK-0070)

> Independent re-audit of G6 after TASK-0069 production runtime isolation
> remediation. Re-tests TASK-0068 blocker **B-ISO-1** and every G6 acceptance
> criterion.
>
> **Verdict: `G6_REMEDIATION_REQUIRED`.**
>
> Production autonomous Claude execution on macOS is now mechanically confined
> by a concrete Seatbelt backend across filesystem, process and network axes;
> unsupported platforms fail closed before provider spawn. Policy authority,
> exact-bound approvals, Secrets Broker leakage/target controls, H7 provider
> environment isolation and the product-authored Security Audit independently
> re-verify, and G5 plus the mandatory workspace suite remain green. B-ISO-1 is
> remediated by directly observable OS-level evidence, not by trusting the
> TASK-0069 completion record.

## 1. Repository identity and audit environment

| Field | Value |
| --- | --- |
| Source main commit | `88d63d494b1251f44943f015a6c324608efaa223` |
| Audit branch | `task-0070-independent-adversarial-g6-closure-reaudit` |
| Audit worktree | `/Users/artemmac/dev/runlith-task0070` |
| Audited tree (before this record) | `88d63d494b1251f44943f015a6c324608efaa223` |
| Host | macOS 26.6.2 (build 25G83), arm64 (aarch64-apple-darwin) |
| Toolchain | rustc 1.95.0 / cargo 1.95.0 |
| Sandbox primitive | `/usr/bin/sandbox-exec` (102 KB, root-owned, canonical system binary) |
| Installed real provider | Claude Code 2.1.268 |

Baseline repository state captured before any test run and reconfirmed after
the entire evidence pass:

```text
git status --porcelain=v1 -z | shasum -a 256
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  -   (empty working tree)

git diff --binary HEAD | shasum -a 256
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  -   (no diff)
```

The working tree remained byte-for-byte unchanged throughout evidence
collection. No production code, repository test, ADR, historical audit record
(`G6_FINAL_CLOSURE_AUDIT.md`, `G6_PRODUCTION_RUNTIME_ISOLATION_REMEDIATION.md`)
or TASK-0068 history was modified during the technical audit. The only
repository writes (audit record + governance/security status) were made after
the technical verdict was fixed.

## 2. Audit authority and independence

This is an audit-only task. Closure criteria were re-derived from the repository
authority order (TASK-0070 descriptor → `INVARIANTS.md` → ADRs → architecture
source of truth → security docs), not from TASK-0069's completion claim.
`planning/tasks/queue.yaml` cannot weaken ADR-0013/ADR-0026 or INV-146..153.

Primary authority re-read:

- ADR-0013 (Policy Engine + mandatory OS isolation; NativeRunner = "Trusted
  Host, not a sandbox");
- ADR-0026 (canonical authority, requested≠effective, monotonic delegation,
  fail-closed unknown/malformed, approval exact-binding, Secrets Broker,
  Runner enforcement, Security Audit separation; TASK-0069 clarification that
  production macOS Claude maps exact authority to Seatbelt with explicit
  `tcp:443`);
- `POLICY_CONTRACT.md`, `THREAT_MODEL.md`, `TRUST_BOUNDARIES.md`;
- INV-005/006/007/008/013, INV-072/073/076, INV-131, INV-146..153.

Historical records are treated as immutable evidence. TASK-0068's
`G6_REMEDIATION_REQUIRED` verdict is preserved unchanged; this re-audit
independently re-tests its blocker rather than rewriting it.

## 3. Executive matrix

| Area | Verdict | Evidence | Blocker ID |
| --- | --- | --- | --- |
| Canonical Policy authority | PASS | `policy` 44 tests; restriction-only Workspace/Repository/RequestSession layers; BuiltInHard/UserManaged positive roots | — |
| Hard DENY / malformed / unknown scope | PASS | Hard deny non-approvable; unknown capability/malformed scope/unprovable child authority fail closed (`policy` suite) | — |
| Parent / request / child containment | PASS | `EffectiveCapabilities::narrow`; `Capability::is_subset_of`; FileScope workspace+path-component containment (no string-prefix) | — |
| Durable approvals | PASS | Exact request/scope/policy-fingerprint/provenance/expiry binding; restart durability; single-use; replay/widen/rebind refusal (`app-services g6_security_integration` 7) | — |
| Secrets Broker | PASS | Exact `SecretAccess(ref)` + exact delivery target before backend read; opaque metadata only (`secrets` 4; schema v9) | — |
| Runner contract | PASS | Exact `ProcessExecute` recheck, workspace-scope match, backend-identity + exact-plan bind, stale/widened report refusal, independent axes (`runner` 7) | — |
| Production Seatbelt filesystem confinement | PASS | Repo adapter keeper + **independent external probe** (sibling-workspace prefix confusion, `~/.ssh` read, absolute/`..`/symlink escape all denied; in-workspace allowed) | — |
| Production process confinement | PASS | `process-exec` restricted to exact executable; unauthorized child exec denied (`Operation not permitted`); `process-fork` allowed; descendants inherit sandbox | — |
| Production network confinement | PASS | Unauthorized remote/local port denied; unsupported scope rejected pre-spawn; accepted effective scope is coarse `tcp:443` (see R1) | — |
| Fail-closed backend selection | PASS | Non-macOS requires 3 axes; `NativeRunner` reports `none()` → `InsufficientEnforcement` before spawn; no silent downgrade | — |
| Provider environment / H7 | PASS | Cleared/allowlisted env; provider-writable roots outside worktree; auth anchor read-only; worktree clean after real run | — |
| Security Audit | PASS | Separate typed store; append-only triggers; SHA-256 chain + persisted head; no payload/secret/provider-text/transcript/env columns; author enum excludes provider/model (`storage-sqlite security_audit` 9) | — |
| Provider completion authority | PASS | Success routes to `Verifying` (never self-complete); provider output untrusted; cannot author audit facts | — |
| G5 regression | PASS | branch_only_recovery/legacy_workspace_identity/casefold_identity (28), execution_recovery (14), execution_binaries (4), execution_composition (8), live_ownership (5), real_process_matrix (1) | — |
| Mandatory workspace verification | PASS | diff-check/fmt/check/clippy -D warnings all clean; `cargo test --workspace` 825 passed / 0 failed / 2 ignored / 98 suites | — |
| Cross-platform claim accuracy | PASS | macOS concrete; Windows compile-check only; Linux NOT EXERCISED; docs claim no cross-platform runtime confinement | — |

## 4. B-ISO-1 re-test

- **Previous finding (TASK-0068):** the production `SecuredClaudeExecutor`
  requested `IsolationRequirements::trusted_host()` and ran under
  `NativeRunner`, which reports `EnforcementAxes::none()`. The
  `IsolationPlan`'s filesystem/process/network authority was metadata only,
  never installed as OS restrictions — a missing mandatory ADR-0013 layer.
- **Current production behaviour (audited tree):**
  `bins/agentd/src/execution.rs` now builds the launch security with
  `IsolationRequirements::new(true, true, true)` and selects the backend via
  `production_runner_backend()` → `MacOsSeatbeltRunner::with_support_roots(..)`
  on macOS. `runner::prepare_process_launch` compiles the plan, the Seatbelt
  backend returns `EnforcementAxes::all()` only after it successfully builds a
  concrete profile for the exact plan, `verify_report` requires all three axes,
  and the returned `PreparedLaunch` rewrites the child into
  `/usr/bin/sandbox-exec -p <profile> <exe> <argv>`. The provider process layer
  (`provider-claude/src/process.rs`) applies that handoff before spawn, so the
  provider cannot bypass the boundary with a raw `Command::spawn`.
  On non-macOS, `NativeRunner` reports `none()` and the three-axis requirement
  makes `verify_report` return `InsufficientEnforcement` **before** any spawn.
- **Independent runtime evidence:** see §5. The shipped adapter mechanically
  denies filesystem/process/network escape on the real OS boundary.
- **Remediated:** yes.
- **Residual risk:** coarse `tcp:443` network scope (R1), macOS-only concrete
  backend (R2); both explicitly scoped and documented, non-blocking.
- **Closure impact:** B-ISO-1 no longer blocks G6.

## 5. Independent OS-boundary probe

Beyond the repository's own adapter keepers, an audit-owned external probe
(temporary cargo crate with path dependencies on the TASK-0070 worktree's
`runner`, `policy`, `runner-macos-seatbelt`) drove the **shipped**
`MacOsSeatbeltRunner` and executed real `sandbox-exec` launches. The probe was
deleted after evidence capture; it contained no secrets and used only synthetic
sentinel files.

```text
[PASS] in_workspace_read_allowed            got_success=true  want=true
[PASS] sibling_workspace_read_denied        got_success=false want=false   (work vs work-evil prefix confusion)
[PASS] ssh_private_key_read_denied          got_success=false want=false   (~/.ssh/id_rsa, HOME/.claude is a support root)
[PASS] auth_anchor_read_allowed             got_success=true  want=true    (~/.claude.json read-only support)
[PASS] auth_anchor_write_denied             got_success=false want=false
[PASS] sibling_workspace_write_denied       got_success=false want=false
[PASS] sibling_workspace_write_absent       exists=false
[PASS] absolute_escape_write_denied         got_success=false want=false
[PASS] absolute_escape_write_absent         exists=false
[PASS] in_workspace_write_allowed           got_success=true  want=true
[PASS] runtime_root_write_allowed           got_success=true  want=true
PROBE_RESULT=ALL_PASS
```

This proves the production confinement is a real OS boundary, not a mock:
workspace read/write is allowed, the isolated runtime root is writable, the
Claude auth anchor is read-only, and every tested escape (sibling workspace via
string-prefix confusion, `~/.ssh`, absolute path) fails. The repo adapter
keeper additionally proves symlink-escape denial and unauthorized-child-exec
denial on the same boundary.

## 6. Real production provider path

`RUNLITH_CLAUDE_SMOKE=1 cargo test -p agentd
real_claude_runs_through_production_g6_isolation -- --ignored` passed (exit 0).
The keeper drives the shipped `agentd` path: Policy evaluation → Seatbelt
backend → `sandbox-exec` → Claude → Security Audit. It fails if the attempt
outcome shows `operation not permitted` / `EPERM` / `sandbox-exec` /
`runtime launch preparation refused`, and requires an `IsolationEnforcement`
audit record for the real execution.

Outcome: the real provider reached the remote endpoint under Seatbelt and
returned `unrecognized_model` (the locally configured model id is not
recognized by Claude CLI 2.1.268). Per INV-131 and TASK-0070 §15, this is
classifiable as external provider state only if reproduced outside the sandbox.
Independent reproduction (same CLI, same argv shape, no Runlith, no sandbox, in
a throwaway dir, no credentials printed):

```text
command claude -p --safe-mode --output-format json "<trivial ping>"
cli_exit=1
stderr contains: unrecognized_model
json result: {"type":"result","subtype":"success","is_error":true, ... model (sonnet-4-5-...[1m]). It may not ...}
sandbox-block markers (operation not permitted / EPERM / sandbox-exec): 0
```

The identical error occurs outside the sandbox, so it is external provider
state, not a sandbox denial. The production keeper correctly classified it and
confirmed the isolation audit record. Confinement itself is independently
proven by §5; the degraded model configuration does not weaken that. The
success→`Verifying` path is therefore not exercised end-to-end live — recorded
as environmental gap R3, non-blocking.

## 7. Policy, approval, secrets, audit (independent re-verification)

- **Policy** (`policy` 44 tests): restrictive precedence
  `DENY > REQUIRE_APPROVAL > ALLOW`; `BuiltInHard → UserManaged → Workspace →
  Repository → RequestSession` with the three weakest layers restriction-only;
  default-deny with stable provenance; typed filesystem overlap by
  workspace+path-component containment (not string prefix); unknown
  capability/malformed scope/unprovable child authority fail closed as
  contract errors distinct from runtime errors; insertion-order-independent
  SHA-256 fingerprint.
- **Approval** (`app-services g6_security_integration` 7): approval is not a
  bearer token; durable binding covers exact request/provenance/scope/policy
  fingerprint/expiry and `effective_if_approved`; consumption requires
  re-presenting the current exact `REQUIRE_APPROVAL`; widened/rebound/expired/
  denied/replayed/post-restart uses fail closed; approval mutation and its
  audit append share one SQLite transaction; hard DENY has no approval path.
- **Secrets** (`secrets` 4; schema v9): release consumes already-computed
  effective authority and verifies exact `SecretAccess(ref)` + exact delivery
  target before any backend read; durable state stores only opaque `SecretRef`
  + `SecretBinding` (no value/payload column); native OS backend with no
  file/cloud fallback; buffer cleared after delivery.
- **Security Audit** (`storage-sqlite security_audit` 9; schema v10): separate
  typed store; stable `aud-<seq>`; enumerated event/author/subject kinds; the
  `author` enum is `{policy-engine, approval-service, secrets-broker, runner,
  provider-environment, runtime, tool-service}` — no `provider`/`model`, so
  provider text cannot author a fact; `previous_hash`/`record_hash` SHA-256
  chain (71-char `sha256:`+64); persisted `security_audit_head`; UPDATE/DELETE
  triggers on both tables; no generic payload/secret/transcript/env column.

## 8. Verification evidence

All commands run from `/Users/artemmac/dev/runlith-task0070`.

| Command | Exit | Result |
| --- | --- | --- |
| `cargo test -p runner --no-fail-fast` | 0 | 7 passed |
| `cargo test -p runner-native --no-fail-fast` | 0 | 3 passed |
| `cargo test -p runner-macos-seatbelt --no-fail-fast -- --test-threads=1` | 0 | 4 passed |
| `cargo test -p policy --no-fail-fast` | 0 | 44 passed (19+25) |
| `cargo test -p secrets --no-fail-fast` | 0 | 4 passed |
| `cargo test -p security-audit --no-fail-fast` | 0 | 2 passed |
| `cargo test -p storage-sqlite --test security_audit -- --test-threads=1` | 0 | 9 passed |
| `cargo test -p app-services --test g6_security_integration -- --test-threads=1` | 0 | 7 passed |
| `cargo test -p provider-claude --test process_fixtures -- --test-threads=1` | 0 | 15 passed |
| `cargo test -p agentd provider_launch_emits_g6_security_audit -- --test-threads=1` | 0 | 1 passed |
| `RUNLITH_CLAUDE_SMOKE=1 cargo test -p agentd real_claude_runs_through_production_g6_isolation -- --ignored` | 0 | 1 passed (external `unrecognized_model`) |
| `cargo test -p app-services --test branch_only_recovery --test legacy_workspace_identity --test casefold_identity -- --test-threads=1` | 0 | 28 passed |
| `cargo test -p agentd --test execution_recovery -- --test-threads=1` | 0 | 14 passed |
| `cargo test -p agentctl --test execution_binaries -- --test-threads=1` | 0 | 4 passed |
| `cargo test -p agentctl --test execution_composition -- --test-threads=1` | 0 | 8 passed |
| `cargo test -p agentd --test live_ownership -- --test-threads=1` | 0 | 5 passed (98 s) |
| `cargo test -p agentctl --test real_process_matrix -- --test-threads=1` | 0 | 1 passed |
| `cargo check -p provider-claude --target x86_64-pc-windows-msvc` | 0 | compile PASS (not runtime confinement) |
| external OS-boundary probe (shipped adapter + `sandbox-exec`) | 0 | ALL_PASS (11 checks) |
| `git diff --check` | 0 | clean |
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo check --workspace` | 0 | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | 0 | clean |
| `cargo test --workspace --no-fail-fast` | 0 | **825 passed / 0 failed / 2 ignored / 98 suites** |

Evidence limitations: the `cargo check --target x86_64-pc-windows-msvc` result
is a compile check only; no Windows/Linux runtime isolation backend exists and
none is claimed. The two ignored workspace tests are the env-gated real-provider
smokes; the agentd one was run explicitly above.

## 9. RED history

No command produced a RED result at any point. Every targeted suite, the
real-provider keeper, every process/restart/concurrency suite, the cross-target
compile check, the independent probe and the full mandatory workspace suite
passed on first execution. The documented historical transient
`ninth_concurrent_client_is_closed_not_fatal` did not appear (live_ownership:
5 passed). No audit-owned process leaked; no process-group kill or broad
pattern kill was used.

## 10. Security claims

| Claim | Verdict |
| --- | --- |
| Canonical authorization in core | PASS |
| Requested/effective distinction | PASS |
| Child authority monotonicity | PASS |
| Hard-deny precedence | PASS |
| Malformed/unknown fail-closed | PASS |
| Approval exact binding | PASS |
| Approval replay resistance | PASS |
| Approval concurrency | PASS |
| Secret authorization | PASS |
| Secret non-persistence | PASS |
| Filesystem confinement | PASS |
| Process confinement | PASS |
| Network confinement (coarse `tcp:443` scope) | PASS (R1 residual) |
| Backend no-fallback | PASS |
| Provider environment / H7 | PASS |
| Audit append-only | PASS |
| Audit tamper detection | PASS |
| Audit separation | PASS |
| Provider cannot complete Task | PASS |
| G5 remains closed | PASS |

## 11. Findings

No closure-blocking findings.

## 12. Residual risks (accepted / non-blocking)

- **R1 — Coarse network scope (HARDENING / ACCEPTED_RESIDUAL).** The Seatbelt
  backend enforces outbound remote TCP on port 443 to *any* host plus the local
  mDNS resolver socket; it does not restrict destination host. This matches the
  explicitly accepted effective capability `tcp:443` in ADR-0026 (TASK-0069
  clarification) and `POLICY_CONTRACT.md`; the docs do not claim
  hostname/destination isolation. Future hardening: a hostname-aware egress
  proxy (listed as an open Runner design item). Analytical risk only; not a
  closure blocker.
- **R2 — macOS-only concrete backend (ENVIRONMENTAL_GAP).** Only macOS has a
  production isolation backend. Linux/Windows autonomous production execution
  fails closed before spawn (no silent NativeRunner downgrade). Windows is
  compile-checked only; Linux runtime was NOT EXERCISED (target not installed).
  Matches canonical supported semantics; not a blocker.
- **R3 — Provider model configuration (ENVIRONMENTAL_GAP).** The configured
  model id is unrecognized by the installed Claude CLI, so the live
  success→`Verifying` path is not exercised end-to-end. Confinement is proven
  independently (§5); the degraded configuration is environmental. Not a
  blocker.
- **R4 — Local-host audit tamper bound (ACCEPTED_RESIDUAL).** The SHA-256 chain
  is tamper-evidence for the local application threat model; a fully compromised
  host can recompute it. External process execution and SQLite are explicitly
  not one atomic transaction. Documented accepted limitation.
- **R5 — Provider auth anchor visibility (ACCEPTED_RESIDUAL, THR-023).** The
  provider-owned `~/.claude` / `~/.claude.json` auth anchors are sandbox-readable
  (read-only) and remain host-visible. Documented accepted residual until
  stronger isolation exists.

## 13. Corrected verdict after supplemental evidence

**`G6_REMEDIATION_REQUIRED`.**

The initial audit result recorded by commit `2402ce2` was a provisional
`G6_CLOSED` verdict. Supplemental adversarial evidence discovered F-003 and
invalidated that conclusion. Claude independently reproduced the same issue
through the production execution path, confirming that the Trusted Host Git
boundary remained a G6 blocker.

The historical provisional result is preserved as audit history. The corrected
TASK-0070 outcome is `G6_REMEDIATION_REQUIRED`, and G6 remains `IN_PROGRESS`
while the Git boundary remediation is handled by a future canonical task.

G7 was not started. This audit performs no remediation and changes no
production implementation. The next canonical action is review and acceptance
of this corrected audit state, followed by a separate remediation task allocation.
