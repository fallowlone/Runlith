# G6 Final Independent Adversarial Closure Audit (TASK-0068)

> Independent closure audit of G6 after TASK-0061..0067.
>
> Audited tree: `9ed1d938d0ccb83e6825935649ab7a3da61d6aab` on
> `task-0068-g6-closure-audit` before this audit record was written.
>
> **Verdict: `G6_REMEDIATION_REQUIRED`.** Policy, approval, secrets, provider
> environment/H7 and Security Audit contracts independently re-verify, but the
> production provider path still executes with `NativeRunner` / explicit
> `Trusted Host` requirements and therefore does not mechanically enforce the
> workspace-scoped filesystem/process/network authority described by the G6
> contract. This conflicts with ADR-0013's mandatory two-layer security model
> and prevents a truthful G6 closure claim.

## 1. Audit scope and authority

TASK-0068 is audit-only. No production remediation is implemented here and no
G7 work is started.

The audit re-derived closure criteria from the repository authority order rather
than treating TASK-0061..0067 completion records as proof. In particular,
`planning/tasks/queue.yaml` cannot weaken architecture, ADRs or invariants.
Relevant authority:

- ADR-0013: Policy authorization **and** runtime OS/container/VM isolation are
  mandatory security layers; `NativeRunner` is explicitly "Trusted Host, not a
  sandbox".
- ADR-0026 / `POLICY_CONTRACT.md`: requested and effective capability scopes
  are distinct; operation owners enforce effective authority adjacent to the
  privileged operation; provider-native controls are defense in depth only.
- INV-146..153, especially INV-149 (unsupported mandatory restrictions fail
  closed), INV-152 (product-authored Security Audit) and INV-153 (preflight is
  not the enforcement boundary).
- G6 gate: Policy Engine, approvals, Secret Broker, audit and runner security
  levels.

## 2. Executive matrix

| Area | Verdict | Independent evidence |
| --- | --- | --- |
| Canonical Policy authority | PASS | Typed provider-neutral policy contracts, deterministic precedence/default deny, repository/session non-widening and parent/request containment tests green |
| Hard DENY / malformed / unknown scope | PASS | Hard denial remains final; unknown/malformed scope and unprovable child authority fail closed |
| Durable approvals | PASS | Exact request/scope/policy/provenance/expiry binding; restart durability; denial/expiry/rebound/widening/replay refusal; single-use consumption |
| Secrets Broker | PASS | Exact `SecretAccess(ref)` + exact delivery target before backend read; opaque metadata only; no value columns / Event Journal payloads |
| Runner contract | PASS **as a contract foundation** | Exact process/workspace recheck, stale report refusal and independent enforcement-axis verification are green |
| Production runtime isolation | **BLOCKER B-ISO-1** | Production `SecuredClaudeExecutor` explicitly requests `IsolationRequirements::trusted_host()` and invokes `runner_native::NativeRunner`, which reports zero filesystem/network/process enforcement axes |
| H7 provider environment | PASS | Sanitized/allowlisted environment; provider-writable state outside worktree; unsafe environment roots refuse before spawn; real Claude smoke leaves Git clean |
| Security Audit | PASS | Typed separate SQLite store, append-only guards, verified SHA-256 chain/head, bounded correlation, no generic/provider/secret payload fields; production launch emits policy/runner/provider evidence |
| G5 regression | PASS in targeted re-runs | Workspace identity/recovery/binary execution suites re-run green; no G5 reopening evidence |
| Real Codex provider | N/A | No `provider-codex` crate/production adapter exists in the audited workspace; repository docs explicitly describe it as future work |

## 3. Policy, delegation and approval

The policy lane independently passes the G6 authority requirements:

- `BuiltInHard -> UserManaged -> Workspace -> Repository -> RequestSession`
  is restrictive composition. Repository/session layers cannot manufacture
  positive authority.
- `DENY > REQUIRE_APPROVAL > ALLOW`; a mixed request exposes no partial
  effective authority.
- `EffectiveCapabilities::narrow` prevents child capability or workspace
  expansion before policy evaluation.
- filesystem scope uses typed workspace identity plus path-component
  containment rather than string-prefix matching.
- no matching positive authorization defaults to denial; malformed/unknown
  capabilities fail closed.

Approval binding is not a bearer token. The durable binding covers the exact
canonical request (operation/requester/Task/Execution/Attempt provenance and
requested capabilities), policy fingerprint, expiry and
`effective_if_approved`. Current authorization must re-present that exact
`REQUIRE_APPROVAL` decision; the matching approved row is then consumed once.
Restart does not reset this state. Widened/rebound, expired, denied and replayed
uses fail closed.

Independent targeted evidence included:

```text
cargo test -p policy -p runner -p runner-native -p secrets -p security-audit --no-fail-fast
60 passed; 0 failed; 13 suites

cargo test -p app-services --test g6_security_integration
7 passed; 0 failed

cargo test -p app-services --test approval_service
4 passed; 0 failed
```

An additional independent policy/approval/secrets lane re-ran 67 tests across
10 suites with zero failures. Its cases included default deny, hard-deny
precedence, repository self-grant refusal, parent/cross-workspace widening,
approval exact binding/restart/replay and secret target/authority refusal.

## 4. Secrets and leakage boundary

`SecretsBroker::release()` resolves binding metadata, checks exact delivery
target, checks exact `SecretAccess(ref)` effective authority and only then
loads backend material. Unauthorized or target-mismatched release therefore
does not touch the credential backend.

SQLite schema v9 stores only opaque secret references and binding target
metadata. The Security Audit stores only opaque `SecretRef` identity and typed
capability metadata. No secret value, generic payload, provider text,
environment dump or transcript field exists in the audited persistence path.

Independent storage evidence:

```text
storage-sqlite security_audit  9 passed
storage-sqlite approvals       4 passed
storage-sqlite secrets         3 passed
storage-sqlite journal        21 passed
storage-sqlite runtime        32 passed
```

No secret-leakage blocker was found.

## 5. Runner contract: foundation passes

The provider-neutral Runner contract itself is sound for what it claims:

- exact `ProcessExecute` is rechecked before backend enforcement and callback;
- filesystem capabilities from another workspace are refused;
- backend identity and exact `IsolationPlan` are revalidated;
- stale/widened enforcement reports cannot authorize another operation;
- filesystem, network and process axes remain independent;
- required enforcement absent from a backend fails before the callback;
- `NativeRunner` honestly reports `EnforcementAxes::none()` and fails if any
  sandbox axis is actually required.

Fresh independent re-run:

```text
cargo test -p runner --no-fail-fast
7 passed; 0 failed; 3 suites

cargo test -p runner-native --no-fail-fast
3 passed; 0 failed; 3 suites
```

This verifies the **foundation**, not production confinement.

## 6. B-ISO-1 — production authority is not mechanically confined

**Severity: HIGH / G6 CLOSURE BLOCKER.**

The production `agentd` provider path constructs effective authority for:

```text
FilesystemRead(current managed workspace, ".")
FilesystemWrite(current managed workspace, ".")
ProcessExecute(exact configured Claude executable)
```

It then builds the Runner spec with:

```text
IsolationRequirements::trusted_host()
```

and calls:

```text
runner_native::NativeRunner
```

`NativeRunner::enforce()` returns an enforcement report with
`EnforcementAxes::none()`. Consequently the plan's filesystem roots,
process-executable set and network-destination set are structured authority
metadata but are not installed as OS restrictions on the provider process.

This has three material consequences:

1. **Filesystem scope is not an enforcement boundary.** The canonical
   authority says the provider may read/write the managed workspace, but the
   spawned process is not OS-confined to those roots. `current_dir(worktree)`
   is location, not access control.
2. **Exact process authority covers launch identity, not child process
   confinement.** The Runner verifies that the launched executable is the
   authorized Claude binary, but zero process-isolation axes means the child is
   not mechanically constrained to the process capabilities represented by the
   plan.
3. **No network sandbox axis is installed.** The audited launch does not gain
   network confinement merely from policy/environment hygiene or provider
   `--safe-mode`; provider-native controls are explicitly non-authoritative.

This conflicts with the higher-authority contracts:

- ADR-0013: security has two mandatory layers — Policy and OS/container/VM
  isolation.
- `POLICY_CONTRACT.md`: filesystem mutation is enforced at the sandbox/fs
  operation boundary; child/process execution at the Runner/process backend;
  network access at a network-capable runner/proxy boundary.
- INV-153: policy preflight is not itself the enforcement boundary.
- TRUST_BOUNDARIES TB4: NativeRunner is not a sandbox and required privileged
  execution controls cannot be replaced by prompt/provider behavior.

TASK-0064/0067 explicitly scoped concrete OS/container backends out, but the
queue is lower authority than ADR-0013 and therefore cannot make the missing
mandatory layer acceptable for **gate closure**. This is exactly the kind of
gap an independent closure audit is required to surface.

No live exploit is required to preserve this finding. INV-131 distinguishes
an analytical security risk from a runtime-proven exploit and forbids erasing
the former merely because an exploit was not reproduced. Here the absence of
filesystem/network/process enforcement is directly observable in the shipped
composition and backend implementation.

### What is *not* the blocker

The blocker is not that `NativeRunner` lies about its strength — it does not.
Its fail-closed contract is correct. The blocker is that the **production
provider path intentionally asks only for Trusted Host**, so the stronger
mandatory isolation layer is never required and therefore never enforced.

## 7. Production-path and real-provider evidence

The production composition keeper remains useful and passes:

```text
cargo test -p agentd provider_launch_emits_g6_security_audit -- --nocapture
1 passed; 0 failed
```

It proves the shipped `agentd -> SecuredClaudeExecutor` path performs canonical
Policy evaluation, Runner plan/report verification and correlated Security
Audit emission before/around the provider callback. It does **not** prove
filesystem/process/network sandbox confinement, because its Runner requirement
is Trusted Host.

Provider environment/H7 evidence independently passes:

```text
cargo test -p provider-claude --test process_fixtures -- --nocapture
15 passed; 0 failed

RUNLITH_CLAUDE_SMOKE=1 cargo test -p provider-claude --test smoke -- --ignored --nocapture
1 passed; 0 failed
```

Installed real provider observed during the audit:

```text
Claude Code 2.1.268
```

The real smoke asserts `git status --porcelain` is empty after execution, so
H7 worktree pollution remains closed. This does not substitute for OS
confinement; provider environment hygiene and sandbox isolation are distinct
controls.

No production Codex adapter exists in the audited workspace. `provider-codex`
appears only in documentation/comments as a future adapter, so a real Codex
keeper is not applicable to TASK-0068 and is not a G6 closure requirement.

## 8. Security Audit

The Security Audit contract independently re-verifies:

- separate typed persistence from Event Journal, operational logs and
  model-visible receipts;
- stable `aud-<seq>` identity and ordered/correlation reads;
- normal UPDATE/DELETE trigger rejection;
- SHA-256 predecessor chain plus persisted head;
- read-time detection of row tampering and tail truncation;
- bounded structured provenance and typed capability scopes;
- no generic payload, secret value, provider/model text, transcript or
  environment dump fields;
- audit records never grant authority.

Fresh evidence:

```text
cargo test -p storage-sqlite --test security_audit -- --nocapture
9 passed; 0 failed

cargo test -p app-services --test g6_security_integration -- --nocapture
7 passed; 0 failed
```

Approval mutation + approval Security Audit append sharing one SQLite
transaction remains covered by the TASK-0067 integration/store contracts.
External process execution is correctly **not** claimed atomic with SQLite.

No Security Audit closure blocker was found. The documented residual that a
fully compromised local host can replace/recompute local audit state remains an
accepted limitation of the current threat model, not a false tamper-proofing
claim.

## 9. G5 regression evidence

Targeted regression re-runs over previously closure-sensitive workspace and
process paths remained green:

```text
cargo test -p app-services \
  --test branch_only_recovery \
  --test legacy_workspace_identity \
  --test casefold_identity --no-fail-fast
28 passed; 0 failed

cargo test -p agentd --test execution_recovery -- --test-threads=1
14 passed; 0 failed

cargo test -p agentctl --test execution_binaries -- --test-threads=1
4 passed; 0 failed
```

No evidence reopens G5.

## 10. Cross-platform/build evidence

The Claude provider adapter still cross-checks for the Windows target:

```text
cargo check -p provider-claude --target x86_64-pc-windows-msvc
PASS
```

This is a compile check for the adapter, not evidence that a Windows sandbox
backend exists. The absence of a concrete production isolation backend is the
same closure blocker described in B-ISO-1.

## 11. Deferred findings classification

Historical non-G6 findings (N-1, N-2, F-SCAN, TEST-H, TEST-H2, Git
`commondir`, H9) remain outside this audit unless independently reproduced as
G6 defects. None was promoted to a G6 blocker by TASK-0068.

Open threat-model items that depend on concrete runtime confinement (notably
filesystem/symlink escape and hostile repository/process escalation) reinforce
B-ISO-1 and must not be represented as closed by a G6 gate verdict.

## 12. Required remediation before re-audit

The narrow remediation direction is:

1. implement a concrete production-capable isolation backend (or backend set)
   that can mechanically enforce the axes required by the supported execution
   mode;
2. make autonomous production provider execution require the relevant
   filesystem/process/network isolation axes rather than
   `IsolationRequirements::trusted_host()`;
3. map the current `IsolationPlan` workspace roots/process/network authority to
   real backend restrictions and fail before spawn when unavailable;
4. add adversarial keepers proving a provider fixture cannot read/write outside
   the authorized workspace, execute an unauthorized child/process path, or use
   unauthorized network authority according to the chosen policy profile;
5. preserve provider authentication/H7 semantics without turning HOME/provider
   login into broad ambient host authority;
6. run a real Claude production-path keeper under the enforced backend and the
   complete G6 + G5 regression suite.

If project governance instead intends Trusted Host to satisfy final G6, that is
not a code fix: ADR-0013 / trust-boundary / capability-enforcement contracts
would need an explicit architecture decision changing the mandatory two-layer
security promise. TASK-0068 does not make that change implicitly.

## 13. Final verdict

**`G6_REMEDIATION_REQUIRED`.**

The audit found one closure-blocking root cause, **B-ISO-1**, in production
runtime isolation. All independently exercised Policy, approval, Secrets,
H7/provider-environment and Security Audit requirements are otherwise green,
and targeted G5 regression remains green.

G6 stays `IN_PROGRESS`. G7 must not start from this audit result. The next
canonical work is a bounded G6 isolation remediation followed by a fresh
independent G6 closure re-audit.

Final completed-tree mandatory verification is green:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS - no issues

cargo test --workspace --no-fail-fast
821 passed; 0 failed; 1 ignored; 96 suites

cargo check -p provider-claude --target x86_64-pc-windows-msvc
PASS
```

The first combined mandatory run also reached the full workspace suite after
check/Clippy passed, but its outer session output channel did not return the
terminal summary. The exact `cargo test --workspace --no-fail-fast` command was
therefore rerun on the unchanged tree with its output/exit status captured
independently; that authoritative rerun returned exit code 0 and the
`821/0/1/96` aggregate above. No product file changed between those runs.

TASK-0068 is complete as an audit task with a green verification suite. A
completed audit does **not** mean the audited gate is closed: G6 remains
`IN_PROGRESS` because its independent verdict is `G6_REMEDIATION_REQUIRED`.
