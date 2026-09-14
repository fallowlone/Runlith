# G6 Final Closure Re-Audit (TASK-0074)

> Independent adversarial re-audit of the full G6 gate after TASK-0071
> (trusted-anchor Git remediation), TASK-0072 (F7 re-audit, F7_CLOSED),
> and TASK-0073 (recovery-shim fix). Audit only: no repairs, no
> implementation. One audit-owned external probe crate was executed and
> deleted; it is not part of the change.

## 0. Verdict

**`G6_CLOSED`.**

Every G6 acceptance row re-verified PASS at the audited tree. F-003 is
remediated and re-proven (F8/F9 fixed, F7 anchored — TASK-0071/0072).
F-002, the sole `NEEDS_MORE_EVIDENCE` remainder from TASK-0070, is now
enumerated by direct measurement (§4) and closed as a documented
residual: the `system.sb`-imported baseline grants no write, no private
read, and no credential path outside declared roots. Residuals R1–R5
carry over unchanged; R3 re-confirmed environmental. G7 not started.

## 1. Repository identity and audit environment

| Field | Value |
| --- | --- |
| Worktree | `/Users/artemmac/dev/runlith-task0074` |
| Branch | `task-0074-g6-closure-reaudit` |
| Audited HEAD | `126292e74b0ebd15f4a42072d45ab71999d6cf0f` (clean tree) |
| Baseline of prior verdict | `e9009a8` (TASK-0070 `G6_REMEDIATION_REQUIRED`) |
| Host | macOS arm64 |
| Toolchain | rustc/cargo (workspace default) |
| Residue | this report only (probe crate deleted, §4) |

Independence notes: no criterion was accepted from a completion record.
Suites were re-run (not quoted); the two historical blockers were
re-attacked through their keepers plus auditor-owned probes; production
code touched since `e9009a8` was diff-scoped (§2).

## 2. What changed since TASK-0070 (diff scope)

```text
$ git log --oneline e9009a8..HEAD -- crates/policy crates/secrets \
    crates/security-audit crates/runner adapters/runner-macos-seatbelt \
    adapters/provider-claude crates/app-services/src/approval_service.rs \
    crates/app-services/src/security_service.rs adapters/keychain-native
(empty)
```

Zero production changes in Policy, Secrets, Security Audit, Runner,
Seatbelt, provider-Claude, approval/security services. Prior
verification for those areas carries; the full regression below proves
no interaction regressions from the Git-boundary work. Changed areas
(TASK-0071/72/73: `workspace`, `git-cli` adapter, workspace store
migration 0011, workspace service, agentd preflight, one test shim)
were re-verified directly (§§3–5).

## 3. Executive matrix (re-verified)

| Area | Verdict | Fresh evidence |
| --- | --- | --- |
| Canonical Policy authority | PASS | `policy` 44 (19+25); precedence, restriction-only layers, fingerprint — code untouched since TASK-0070, suite green |
| Hard DENY / malformed / unknown scope | PASS | same suite; fail-closed contract errors distinct from runtime errors |
| Parent / request / child containment | PASS | `EffectiveCapabilities::narrow`, `FileScope` component containment; suite green |
| Durable approvals | PASS | `g6_security_integration` 7/7 re-ran: exact binding, replay/widen/rebind refusal, restart durability, single-use, DENY-has-no-path |
| Secrets Broker | PASS | `secrets` 4/4; opaque refs only (schema refs), exact-target delivery, buffer clearing; code untouched |
| Runner contract | PASS | `runner` 7/7: exact exec recheck, report binding, stale/widened refusal, independent axes |
| Seatbelt filesystem confinement | PASS | keepers green + §4 external enumeration (24 checks ALL_PASS): writes denied outside declared roots; reads denied for SSH keys, home, tmp, siblings, live app DB, live keychain file |
| Seatbelt process confinement | PASS | exact-exe `process-exec`; `env /usr/bin/true` denied (`Operation not permitted` shape); descendants inherit |
| Seatbelt network confinement | PASS | keepers green (unauthorized port denied; unsupported scope fails pre-spawn); scope still coarse `tcp:443` (R1) |
| Trusted Host Git boundary (F-003/F7) | PASS | 7/7 targeted keepers: fsmonitor-disabled, clean-filter-disabled, include-refused, worktree/gitproxy/sshcommand/hookspath-refused, valid-but-wrong refused, forged-commondir refused, fully-forged refused |
| Fail-closed backend selection | PASS | `NativeRunner::none()` → `InsufficientEnforcement` pre-spawn; code untouched |
| Provider environment / H7 | PASS | `process_fixtures` 15/15; cleared/allowlisted env keepers in full run; code untouched |
| Security Audit | PASS | `security_audit` 9/9 + lib 2/2: separate store, stable ids, hash chain + head, append-only triggers, no provider authorship, no secret/payload columns; schema v10 intact beside v11 anchor sidecar |
| Provider completion authority | PASS | success → `Verifying` only; full composition suites green |
| G5 regression | PASS | branch_only/legacy/casefold, execution_recovery 14/14 (agentd) + 3/3 (agentctl, incl. fixed shim keeper), binaries, composition, live_ownership 5/5, real_process_matrix 1/1 |
| Mandatory workspace verification | PASS | fmt clean; clippy `--workspace --all-targets -D warnings` clean; full suite **102 suites / 843 passed / 0 failed / 2 ignored** |
| Cross-platform claim accuracy | PASS | `cargo check -p provider-claude --target x86_64-pc-windows-msvc` clean (compile only); Linux runtime not exercised; docs claim macOS-only confinement (R2) |
| Real provider path | PASS (environmental) | smoke runs against real CLI 2.1.268, same external `unrecognized_model` as TASK-0070 (model id not recognized out-of-sandbox either); correctly classified, non-blocking (R3) |

The 2 ignored workspace tests are the env-gated real-provider smokes.

## 4. F-002 closure: enumerated `system.sb` baseline

F-002 (TASK-0070): the mandatory `(import "system.sb")` grants reads to
selected `/System` resources outside declared roots; the baseline was
unenumerated and `EnforcementReport` does not describe it.

Method: audit-owned temporary crate driving the SHIPPED
`MacOsSeatbeltRunner` + real `/usr/bin/sandbox-exec` with a
production-shaped plan (workspace read/write + exact exe). 24 checks:
declared-scope controls, Apple-system reads, sensitive reads, outside
writes, symlink escape, child exec, keychain reachability (nonexistent
item only — no secrets touched) and keychain write refusal. Departures
from initial expectations were each independently confirmed with raw
`sandbox-exec` (deny-default + system.sb + exec-allow only), never by
editing expectations to fit. Full output hashed below; crate deleted.

Measured baseline (`PROBE_RESULT=ALL_PASS`):

| Class | Result |
| --- | --- |
| Declared workspace read/write | ALLOW (controls pass) |
| `/bin/*`, `/usr/lib/*` reads | DENY (execute-vs-read separation: processes run, binaries unreadable) |
| Selected `/System` reads, `/` listing, `/dev` nodes | ALLOW (Apple baseline) |
| `/etc/passwd` | ALLOW — verified skeleton-only (142 lines, all `*`, system accounts, no hashes; identity via Open Directory). Standard libc requirement. |
| `/etc/hosts`, `/var/log`, `$HOME`, `~/.ssh`, sibling dirs, `/tmp`-outside | DENY |
| Live app DB (`state.sqlite3`: approvals/audit/leases/anchor sidecar) | DENY (exit status only, never printed) |
| Live login keychain FILE | DENY (exit status only, never printed) |
| All writes outside declared roots (sibling, `/tmp`, home) | DENY |
| Symlink escape read | DENY |
| Non-exact child exec | DENY |
| `securityd` reachable for metadata-shaped queries | YES — nonexistent-item query returns normal not-found. Keychain WRITE denied. Direct keychain-file reads denied (above), so values stay behind securityd ACL/user-prompt, unreachable prompt-free from the sandbox. |

Evidence record: probe output sha256
`95a1356d055a45e6f3e25670c2278905812a5711fac50490309a088aca835c44`,
probe source sha256
`4197ed7d4c10f9c50d769516e384a8b7b965c6252af9e1d50ec4b09050ace7c0`.

**F-002 verdict: CLOSED as documented residual.** The imported
baseline contains no write, no private-data read, and no credential
path. Remaining polish (an `EnforcementReport`/docs field describing
the imported baseline) is a non-blocking hardening suggestion (O3),
not a closure condition: the security requirement is confinement of
authority, which is measured, not merely reported.

## 5. F-003/F7 confirmation at this tree

Original F-003 chain (provider → `.git` rewrite + helper → Trusted
Host Git executes with agentd authority) is dead at two layers:
executable helper surfaces refused before Git runs (F9 keepers), and
forged repository identity refused by admission-anchor mismatch (F7
keeper + TASK-0072 re-audit, F7_CLOSED). All 7 keepers re-ran green
above; no new Git-boundary paths were found in this audit.

## 6. Verification evidence (commands run in this worktree)

| Command | Exit | Result |
| --- | --- | --- |
| `cargo test --workspace --no-fail-fast` | 0 | 102 suites / 843 passed / 0 failed / 2 ignored |
| `cargo fmt --check` | 0 | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | clean |
| `cargo test -p policy -p runner -p secrets -p security-audit` | 0 | 44 / 7 / 4 / 2 |
| `cargo test -p storage-sqlite --test security_audit` | 0 | 9 |
| `cargo test -p app-services --test g6_security_integration` | 0 | 7 |
| `cargo test -p git-cli` | 0 | 74 (8+18+10+2+25+11) |
| F-003/F7 keeper subset | 0 | 7/7 |
| `cargo test -p provider-claude --test process_fixtures` | 0 | 15 |
| `cargo test -p agentd provider_launch_emits_g6_security_audit` | 0 | 1 |
| `cargo test -p agentd --test live_ownership` | 0 | 5 (84 s) |
| `cargo test -p agentd --test execution_recovery` | 0 | 14 |
| `cargo test -p agentctl` (full package) | 0 | all suites ok incl. 3/3 recovery, real_process_matrix |
| `cargo test -p product-version -p ipc-control` | 0 | 41+4 |
| `cargo check -p provider-claude --target x86_64-pc-windows-msvc` | 0 | compile PASS |
| `RUNLITH_CLAUDE_SMOKE=1 ... --ignored` (provider smoke) | 0 | external `unrecognized_model`, correctly classified |
| F-002 external probe (shipped adapter + sandbox-exec) | 0 | ALL_PASS (24 checks) |

One intermediate full-parallel run showed 841 passed / 2 failed with
uncaptured names; the immediately following isolated re-runs of the
timing-sensitive class (live_ownership, execution_recovery,
real_process_matrix) were all green, orphan daemons from the first run
were present, and two subsequent full runs were green (final:
843/0/2). Root cause: cross-run orphan interference (O1), not product.

## 7. Findings

- **F-003 — CLOSED.** Remediated by TASK-0071, independently re-audited
  (TASK-0072 F7_CLOSED), re-proven here.
- **F-002 — CLOSED (documented residual).** Baseline enumerated in §4;
  no confinement-relevant authority imported.

## 8. Residuals and observations

Carried (accepted, non-blocking): R1 coarse `tcp:443` egress scope;
R2 macOS-only concrete backend (fail-closed elsewhere); R3 degraded
provider model config (re-confirmed external, same CLI/error as
TASK-0070); R4 local-host audit tamper bound; R5 provider auth-anchor
visibility (THR-023).

New observations (non-blocking):
- **O1 — binary-suite orphan hygiene.** Crash/SIGKILL suites orphan
  real daemons on failure; back-to-back full runs without cleanup can
  poison machine-global assertions (`real_process_matrix`). Mitigation:
  kill leftover `target/debug/agentd` between runs. Test-hygiene only.
- **O2 — stale binary trap (recorded in TASK-0073).** `cargo test`
  never rebuilds the spawned `agentd` binary; build bins explicitly
  before binary-level suites.
- **O3 — report the imported baseline.** Consider an
  `EnforcementReport`/docs field naming the `system.sb` import as
  read-baseline input. Polish, not a condition.

## 9. Canonical state

- `completed_through`: `TASK-0074` (this audit; queue/version update
  follows in the acceptance commit).
- `current_gate`: `G6`, `gate_status`: `CLOSED`.
- Verdict: **`G6_CLOSED`**. G7 not started. No remediation required;
  no production changes made or needed by this audit.
