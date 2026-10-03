# Threat Model — Claude Code Integration — 2026-10-03

Read-only assessment. Companion to
`SECURITY_ARCHITECTURE_AUDIT_2026-10-03.md` (module audit,
findings F1-F20). Baseline commit `5362f6e`.

## Existing controls (verified mechanically)

- Per-launch Seatbelt profile: filesystem read/write roots
  scoped to workspace + support roots; relative roots only
  (`workspace_root` refuses absolute).
- Network default-deny. Empty `network_destinations` emits no
  `allow network-outbound`. The only supported scope is
  `tcp 443` + mDNSResponder; anything else fails closed with
  `RunnerError::BackendUnavailable`.
- Provider environment: `env_clear()` + explicit allowlist.
  Ambient API keys, cloud credentials, `GIT_*` overrides,
  `SSH_AUTH_SOCK`, and provider/plugin variables are stripped.
  XDG config/data/state/cache redirected to private
  `runtime_root` children.
- `permission-mode` values containing `bypass` rejected at
  config. `--tools ""` and `--disallowedTools "mcp__*"`
  available as proposal-only argv.
- `checked_absolute` (seatbelt lib.rs:273-287) rejects
  non-absolute paths, `"`, `\`, and control chars, so Seatbelt
  profile string injection is impossible by construction.
- Typed security-audit events (PolicyRequest/Decision,
  ApprovalRequested/Decided, HumanDecision*, SecretAccess,
  IsolationEnforcement, IsolationViolation, ProviderInvocation,
  ToolInvocation). No secrets, transcripts, or provider text.
- `IsolationViolation` IS emitted (`security_service.rs:388`):
  `author: Runner`, `outcome: Violated`,
  `reason_code: "runner_enforcement_refused"`, full
  correlation/causation/execution/attempt lineage, whenever the
  guarded runner invocation returns `Err`.
- Completion authority independent of executor (ADR-0018);
  provider output is evidence, never authority.
- Process-group kill (`killpg`) covers double-forked
  descendants; bounded capture threads.
- `Unattached` launch default refuses to spawn (adversarially
  tested).

## Attack vectors

| # | Vector | Status |
|---|---|---|
| A1 | Indirect prompt injection via repo content | Open (inherent). Write roots stay writable; injection can edit in-scope files, run in-scope git. Policy bounds but does not eliminate. Injected text can also reach durable evidence (A9). |
| A2 | Seatbelt profile string injection | Closed. Charset rejection in `checked_absolute`. Defense-in-depth: add `[A-Za-z0-9._/-]` allowlist to remove the class entirely. |
| A3 | Linux/Windows containment | Open gap. `NativeRunner` only off macOS; no OS sandbox. Production containment is macOS-only today. |
| A4 | Egress over 443 to arbitrary endpoints | Open. `remote tcp "*:443"` allows any TLS host. Seatbelt cannot do hostname policy. Egress proxy is the only real control. |
| A5 | Host config inheritance via HOME | Partially open. XDG dirs isolated; ambient env stripped. But `HOME` passes through (`environment.rs:94`, resolved at `:158`), so `~/.claude` (settings, MCP configs, stored auth) and `~/.ssh` file-based keys remain readable. Deliberate for provider-owned auth ("provider environment requires HOME for provider-owned authentication", `:96-99`). |
| A6 | Audit trail visibility | Partially open. Launch-time runner refusal is audited. Runtime sandbox denials (EACCES/EPERM inside the sandbox, e.g. a blocked `open()`) are NOT audited — they surface as provider output failures (`Failed`/`MalformedOutput`), invisible in the audit stream. |
| A7 | Nested agent sessions | Open. Subagents inherit the sandbox (good), but bounded only by the same capability policy; no per-depth budget. |
| A8 | Approval fatigue | Open. Human gates exist; no rate or anomaly signal on ApprovalRequested. |
| A9 | Provider output as durable evidence and human-review input | Open (inherent), bounded. Second-order indirect prompt injection: see "A9 detail" below. |

## A9 detail — provider output as durable evidence (audit F8)

Path: Claude stdout (untrusted) -> `output.rs` parser -> `result_text` ->
truncated to `max_summary_chars` (default 1024, `executor.rs:453`/`:496`) ->
`WorkEvidence.summary` -> `interpret.rs:54` -> `submit_result`
(`orchestration_service.rs:725`) -> `reason` of the `Submitted` journal event
(`task_service.rs:549`). Failure detail is capped at 500 chars
(`MAX_DETAIL_CHARS`) into the attempt note.

Risk: a repo-content injection (A1) shapes what the model writes, and that text
becomes append-only, hash-chained evidence. The chain proves the text was not
edited afterwards, not that it is honest. A reviewer or tool that reads it can
still be steered.

Existing controls: bounded length; never authority (ADR-0018, INV-043;
`"success"` never means completion; completion needs independent verification);
audit events carry no provider text.

Rules for any consumer (UI, planner context, approval prompt):

- Render it as quoted data, labelled provider-reported, with control
  characters escaped.
- Never place it in a model prompt as instructions.
- Never put it in the authority-bearing fields of an approval or decision.

No code change: there is no verified human-facing renderer of
`Submitted.reason` yet, so there is nothing to sanitize against.

## Hardening recommendations

**H1 — Runtime denial visibility (A6).** Map provider-captured
stderr patterns (`Operation not permitted`, `denied by
sandbox`, EPERM/EACCES exit codes) to an `IsolationViolation`
record with `reason_code: "runtime_denial"`, same lineage
fields as launch-time refusal. Closes the only silent failure
class in the audit stream.

**H2 — Egress control (A4).** Pin `HTTPS_PROXY` in the launch
allowlist to a local egress proxy with a domain allowlist
(`api.anthropic.com`, `statsig.anthropic.com`, `sentry.io`),
deny-by-default. Seatbelt cannot express hostname policy; the
proxy is the enforcement point.

**H3 — Home isolation (A5).** Keep HOME passthrough for
provider auth, but add a policy axis: agent launches that do
not need provider-owned auth get `HOME=$CLAUDE_CODE_TMPDIR/home`
(empty skeleton). When HOME is inherited, assert the Seatbelt
read roots exclude `~/.ssh` and `~/.claude` unless explicitly
granted by an `EffectiveCapabilities` scope.

**H4 — Depth and budget caps (A7).** Per-execution
`max_agent_depth = 1` and tool-invocation budget in the policy
engine; exceed → `PolicyDecision::deny` with a typed
`hard.agent_depth` rule.

**H5 — Linux backend (A3).** Landlock + seccomp (or bubblewrap)
`IsolationBackend` so production containment is not
macOS-only. `EnforcementAxes` already models the three axes;
only the backend is missing.

**H6 — Audit tamper evidence.** Append-only triggers + hash
chain on the audit/journal store (finding F10). Verify chain
on daemon start; fail closed on mismatch.

**H7 — Approval anomaly signal (A8).** Alert on
ApprovalRequested rate spikes and approval/deny ratio drift;
feed from the audit stream, no new instrumentation.

## Integration blueprint

Deployment shape:

```
agentd (sole entrypoint; claude CLI never invoked directly)
  ├─ policy engine — capability model, approval gates, depth/budget
  ├─ runner — Seatbelt per launch (macOS) / landlock (Linux, H5)
  ├─ egress proxy — domain allowlist, deny-by-default (H2)
  └─ audit store — append-only, hash-chained, correlation-linked (H6)
```

Containerization (shared hosts): read-only rootfs, tmpfs
`/workspace`, network only via egress proxy, bind-mount only
the project worktree, agentd binds to `127.0.0.1`, non-root
`--user`, `no-new-privileges`.

Permission defaults:

```
permission-mode: default          # never bypass (enforced)
allowed-tools: []                 # none beyond built-in file ops
disallowed-tools: ["mcp__*"]      # MCP denied by default
network: outbound-443-only        # + proxy domain allowlist
max-agent-depth: 1
```

Wrapper pattern: `agentctl` negotiates; every provider launch
flows through `ExecutionPort` → policy → runner. Enforce with a
CI check failing on `Command::new("claude")` outside
`adapters/provider-claude`.

Monitoring: IsolationViolation count > 0 pages;
ApprovalRequested/ProviderInvocation ratio anomaly; audit
hash-chain verify on every daemon start; secret-pattern scan
hits quarantine the submission.

Velocity note: defaults block only MCP, exotic networks, and
bypass modes. The common happy path (file ops and git inside
the worktree, 443 to the Anthropic API) is untouched.

## Residual uncertainty

- Whether Seatbelt read roots exclude `~/.ssh`/`~/.claude`
  under HOME passthrough depends on the composition-time
  `IsolationPlan` roots; not traced end-to-end here (H3 asserts
  it mechanically).
- `effect-capture-local` candidate snapshot paths sampled, not
  exhaustively traced.
- Live provider behavior untested; deterministic fixtures do not
  establish live-provider support.
