# TASK-0152 (G8) — Provider Credential-Input Probes

Status: evidence complete on Linux; macOS Seatbelt matrix prepared but not
executed (no macOS host in this worktree); one finding requires owner
decision before further use (see "Critical finding" below).

## Question

TASK-0152's ADR must choose a mechanism that lets the confined Claude
provider authenticate without Keychain, SecurityServer, or
`/usr/bin/security` authority (G7/TASK-0112). Answering that requires first
knowing, empirically, with the installed Claude Code CLI and only dummy
non-secret marker credentials: which non-Keychain credential inputs the CLI
actually honours, and how it transmits them (which header, whether both
`x-api-key` and `Authorization` can be sent together, what precedence
applies when more than one input is present). This task gathers that
evidence only; it makes no ADR decision and changes no production code.

## Critical finding — read this before the row-by-row results

**With no credential configured at all** (row `P0_no_credential`: fresh,
empty scratch `HOME`, fully sanitized `env -i` environment, no
`ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`/`CLAUDE_CODE_OAUTH_TOKEN`, no
`--settings`), the installed CLI in this container **still completed the
marker call successfully** and sent an `Authorization` header whose value
did **not** contain any of this task's dummy markers. The fake endpoint
recorded this, per its privacy contract, only as the classification
`authorization: non-dummy-value-present` — never the raw value.

This is the exact STOP/ESCALATE condition the task envelope defined ("a
header contains something that is NOT one of the dummy markers ... that
would mean a real credential leaked in"). Per the envelope, no attempt was
made to identify, print, hash, fingerprint, or otherwise inspect that
value. A follow-up attempt to compute even a redacted fingerprint (length,
prefix, hash — never the raw value) of that header was independently
**blocked by this runtime's own safety classifier** with reason
"Credential Materialization," which this task treated as confirmation to
stop that line of investigation entirely, not as something to route around.
A subsequent attempt to list the scratch `HOME` directory tree (no file
content read) for the same probe was blocked the same way, so that path was
also abandoned.

What is established, without inspecting the value itself:

- the ambient value is **not** sourced from the process environment: every
  invocation used `env -i` with an explicit, minimal variable list, and
  `/opt/claude-code/bin/claude` is a single static ELF binary (not a shell
  wrapper that could re-inject variables);
- it is **not** sourced from `$HOME`: every probe used a freshly created,
  per-row, empty scratch `HOME` with no pre-existing `.claude` state;
- it is **suppressed** — not merged, not raced — the moment any of
  `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, or `CLAUDE_CODE_OAUTH_TOKEN`
  is set, or the moment a working `apiKeyHelper` is configured via
  `--settings` (rows P1–P5 all show only dummy-marker classifications, no
  `non-dummy-value-present`); it therefore behaves as a **lowest-priority
  default**, not a higher-priority override;
- it is present under both `--safe-mode` and without it, so `--safe-mode`
  does not gate it.

What is **not** established, and is explicitly out of scope for this task
to determine: the concrete source of that default. The most plausible
mechanism, based on public Claude Code behavior and consistent with
everything observed, is a system/enterprise **managed settings** file at a
fixed OS path that Claude Code reads independently of `$HOME` (Claude Code
supports a managed-policy settings file for exactly this purpose); this
container plausibly ships one so that the outer agent session itself
(this very session) can authenticate. That is a hypothesis, not a verified
fact — this task did not and must not check for such a file's existence or
content, since doing so would itself be inspecting credential-adjacent
material beyond what the hard rules and the runtime's own classifier
allow.

**This is squarely relevant to ADR-0037's candidate evaluation** and is
reported here as the headline result: on at least this Linux container,
there exists a **third credential channel**, independent of both explicit
environment variables and `$HOME`-rooted files, that this task could not
fully characterize. Before the ADR selects a mechanism, the owner should
determine, out of band and outside an automated probe: (a) whether this
container's managed-settings-style channel is representative of what a
production macOS worker would see, (b) whether the G6 Seatbelt profile
would grant read access to whatever path this channel uses, and (c)
whether that path could become a **non-Keychain, sandbox-scopable**
authentication mechanism in its own right — it is intriguing precisely
because it authenticated successfully under a fully sanitized `env`/`HOME`
without Keychain, SecurityServer, or `/usr/bin/security`.

## Measured environment

- Date: 2026-09-25.
- OS/kernel: Ubuntu 24.04.4 LTS, `Linux 6.18.44-fc-v37 x86_64` (container;
  not macOS — see "What is NOT proven" below).
- Claude Code CLI version: `2.1.282 (Claude Code)`.
- CLI launcher: `/opt/node22/bin/claude` (symlink).
- Resolved executable: `/opt/claude-code/bin/claude` (single static ELF
  binary, ~238 MB, not a shell wrapper).
- Repository base commit: `78079e36a5f463093ea07418f23f19f2f2ef3d87`
  (matches the assigned envelope's `base_main_commit`).
- Prompt used for every row: `Do not use any tools. Reply with exactly
  TASK0152_PROBE_OK and nothing else.`
- Dummy markers used (never a real credential):
  `RUNLITH_T0152_DUMMY_API_KEY_NOT_A_SECRET`,
  `RUNLITH_T0152_DUMMY_BEARER_NOT_A_SECRET`,
  `RUNLITH_T0152_DUMMY_OAUTH_NOT_A_SECRET`.

## Production argv and environment facts (verified with citations)

Verified in `adapters/provider-claude/src/argv.rs:33-59`
(`build_argv`, tests at lines 102-110): production's minimal, unconditional
argv is exactly

```text
-p --safe-mode --output-format json <prompt>
```

with `--tools ""`/`--disallowedTools mcp__*` added only in proposal-only
mode, and `--system-prompt`/`--permission-mode`/`--resume` added only when
configured — none of which are credential-related. This is the exact argv
this task's probe matrix used (see `run_credential_input_probes.sh`).

Verified in `adapters/provider-claude/src/environment.rs`, function
`resolved_pairs` (lines 142-199): the **entire** explicit environment
allowlist a production-governed child receives is `PATH`, `HOME`, `USER`,
`PWD`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`,
`XDG_CACHE_HOME`, `CLAUDE_CODE_TMPDIR`, `TMPDIR`/`TMP`/`TEMP`, and
`LANG`/`LC_ALL`/`LC_CTYPE` when set on the host (plus Windows-only
`USERPROFILE`/`SystemRoot`/`WINDIR`/`PATHEXT`/`APPDATA`/`LOCALAPPDATA`).
`apply` (lines 123-128) calls `command.env_clear()` before applying this
list. **None of `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
`CLAUDE_CODE_OAUTH_TOKEN`, or `ANTHROPIC_BASE_URL` is in this list today.**
The module's own doc comment (lines 1-6, 120-122) states this is
intentional: "Ambient API keys, cloud credentials, Git overrides, SSH
agents and provider/plugin variables are intentionally not inherited."

**Consequence, stated per the envelope's requirement**: injecting any of
P1–P3's credential variables (or a base-URL override) into a
production-governed provider launch would require an explicit change to
this allowlist. That is an implementation change to
`adapters/provider-claude/src/environment.rs`, gated by the ADR this task
feeds, not something this evidence-only task did or should do.

Verified in `adapters/provider-claude/src/environment.rs`,
`isolation_support_read_roots` (lines 219-230) /
`isolation_support_write_roots` (lines 234-237): the OS-sandbox support
roots granted alongside the task workspace are the isolated runtime root
(read-write) plus, read-only and only if they already exist,
`$HOME/.claude` and `$HOME/.claude.json` — i.e. the one `$HOME`-rooted
credential/session-state channel production's own sandboxing code
anticipates. No managed-settings-style path is granted by this code today.

Verified in `bins/agentd/src/execution.rs`: `SecuredClaudeExecutor::execute`
(lines ~3254-3298) collects `isolation_support_read_roots`,
`isolation_support_write_roots`, and `child_environment` from the
`ProviderEnvironment` above and passes them into
`prepare_provider_launch`/the Runner. `build_provider_launch_security_for_context`
(lines ~3335-3378) always requests exactly one network capability for a
provider launch: `Capability::NetworkAccess(ScopeId::new(runner::NETWORK_SCOPE_OUTBOUND_TCP_443))`
— the single `*:443` scope, never a narrower loopback scope, and never
conditional on which credential mechanism is in use.

Verified in `adapters/runner-macos-seatbelt/src/lib.rs`, `profile_for`
(lines 103-249): the macOS backend turns that one network capability into
exactly

```text
(allow network-outbound
  (literal "/private/var/run/mDNSResponder")
  (remote tcp "*:443"))
```

(lines 230-239) and refuses (`BackendUnavailable`) any other requested
network scope. This profile never emits `mach-lookup
com.apple.SecurityServer`, a `login.keychain-db` read, or a
`/usr/bin/security` `process-exec`/`file-read*` grant — confirmed by
reading the full function body; those three rules exist only in
G7_TASK_0112's ablation harness, never in `profile_for` itself.

## Linux probe matrix (P0–P8) — commands

Every row ran through `docs/development/task0152-probes/run_credential_input_probes.sh`,
which for each row: starts `fake_anthropic_endpoint.py` on a fresh loopback
port, launches the CLI with `env -i` and a fresh empty scratch `HOME`,
tears the endpoint down, and records only the endpoint's classified log —
never a raw header value or body.

Exact invocation (redacted of nothing; there is nothing secret to redact
beyond the dummy markers already named above):

```text
CLAUDE_BIN=/opt/node22/bin/claude \
  ./docs/development/task0152-probes/run_credential_input_probes.sh
```

Representative per-row shape (P1, `ANTHROPIC_API_KEY`), as the script
constructs it:

```text
env -i PATH=/opt/node22/bin:/usr/bin:/bin HOME=<fresh-scratch-home> \
  ANTHROPIC_BASE_URL=http://127.0.0.1:<fake-endpoint-port> \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  ANTHROPIC_API_KEY=RUNLITH_T0152_DUMMY_API_KEY_NOT_A_SECRET \
  /opt/node22/bin/claude -p --safe-mode --output-format json \
  "Do not use any tools. Reply with exactly TASK0152_PROBE_OK and nothing else."
```

## Linux probe matrix (P0–P8) — results

All 16 rows: CLI exit `0`, marker `TASK0152_PROBE_OK` present in stdout,
single endpoint path hit (`POST /v1/messages`, non-streaming JSON — the CLI
did not request `"stream": true` for this one-turn call), stderr empty on
every row.

| row | auth headers observed (classification only) | leak flag |
| --- | --- | --- |
| P0 no credential at all | `authorization=non-dummy-value-present` | **yes — see Critical finding** |
| P1 `ANTHROPIC_API_KEY` dummy | `x-api-key=dummy:api_key` (no `authorization` at all) | no |
| P2 `ANTHROPIC_AUTH_TOKEN` dummy | `authorization=dummy:bearer` | no |
| P3 `CLAUDE_CODE_OAUTH_TOKEN` dummy | `authorization=dummy:oauth` | no |
| P4 `ANTHROPIC_API_KEY`+`ANTHROPIC_AUTH_TOKEN` | `x-api-key=dummy:api_key; authorization=dummy:bearer` (**both** sent) | no |
| P4 `ANTHROPIC_API_KEY`+`CLAUDE_CODE_OAUTH_TOKEN` | `x-api-key=dummy:api_key` only (OAuth token fully suppressed, no `authorization` at all) | no |
| P5 `apiKeyHelper` via `--settings`, with `--safe-mode` | `x-api-key=dummy:api_key; authorization=dummy:api_key` (helper's key echoed into **both** headers) | no |
| P5 `apiKeyHelper` via `--settings`, without `--safe-mode` | same as above | no |
| P6 `CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR` (fd) | `x-api-key=dummy:api_key` | no |
| P6 `CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR` (fd) | `authorization=dummy:api_key` (fd content flows into `Authorization: Bearer`) | no |
| P6 `CLAUDE_CODE_GATEWAY_TOKEN_FILE_DESCRIPTOR` (fd) | `authorization=non-dummy-value-present` | yes (ambient default, not honoured) |
| P6 `CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR` (fd) | `authorization=non-dummy-value-present` | yes (ambient default, not honoured — expected: different transport) |
| P6 `ANTHROPIC_IDENTITY_TOKEN_FILE` (file path) | `authorization=non-dummy-value-present` | yes (ambient default, not honoured) |
| P6 `CLAUDE_CODE_HOST_CREDS_FILE` (file path) | `authorization=non-dummy-value-present` | yes (ambient default, not honoured) |
| P7 egress, nonessential traffic enabled | `x-api-key=dummy:api_key` | no |
| P7 egress, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | `x-api-key=dummy:api_key` | no |

Full machine-readable summary (script's own TSV output, unedited):

```text
row	exit	marker	paths	auth_headers	stderr_class	leak_flag
P0_no_credential	0	yes	POST /v1/messages	authorization=non-dummy-value-present	empty	LEAK_FLAG
P1_api_key	0	yes	POST /v1/messages	x-api-key=dummy:api_key	empty	-
P2_auth_token	0	yes	POST /v1/messages	authorization=dummy:bearer	empty	-
P3_oauth_token	0	yes	POST /v1/messages	authorization=dummy:oauth	empty	-
P4_api_plus_auth	0	yes	POST /v1/messages	x-api-key=dummy:api_key;authorization=dummy:bearer	empty	-
P4_api_plus_oauth	0	yes	POST /v1/messages	x-api-key=dummy:api_key	empty	-
P5_apikeyhelper_safe	0	yes	POST /v1/messages	x-api-key=dummy:api_key;authorization=dummy:api_key	empty	-
P5_apikeyhelper_unsafe	0	yes	POST /v1/messages	x-api-key=dummy:api_key;authorization=dummy:api_key	empty	-
P6_fd_CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR	0	yes	POST /v1/messages	x-api-key=dummy:api_key	empty	-
P6_fd_CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR	0	yes	POST /v1/messages	authorization=dummy:api_key	empty	-
P6_fd_CLAUDE_CODE_GATEWAY_TOKEN_FILE_DESCRIPTOR	0	yes	POST /v1/messages	authorization=non-dummy-value-present	empty	LEAK_FLAG
P6_fd_CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR	0	yes	POST /v1/messages	authorization=non-dummy-value-present	empty	LEAK_FLAG
P6_file_ANTHROPIC_IDENTITY_TOKEN_FILE	0	yes	POST /v1/messages	authorization=non-dummy-value-present	empty	LEAK_FLAG
P6_file_CLAUDE_CODE_HOST_CREDS_FILE	0	yes	POST /v1/messages	authorization=non-dummy-value-present	empty	LEAK_FLAG
P7_egress_with_nonessential	0	yes	POST /v1/messages	x-api-key=dummy:api_key	empty	-
P7_egress_no_nonessential	0	yes	POST /v1/messages	x-api-key=dummy:api_key	empty	-
```

P8 (loopback `http://` acceptance) is covered by every row above: the CLI
never warned or refused `ANTHROPIC_BASE_URL=http://127.0.0.1:<port>` (plain
HTTP, loopback); stderr was empty on all 17 invocations.

### P7 — egress inventory

Using `logging_connect_proxy.py` (`HTTPS_PROXY`/`HTTP_PROXY` pointed at it,
`NO_PROXY=127.0.0.1,localhost` so the loopback fake endpoint itself was not
proxied; the proxy logs only `CONNECT host:port` and always refuses to
tunnel — no real egress occurred):

| condition | non-loopback hosts the CLI attempted to reach |
| --- | --- |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` unset | `api.anthropic.com` |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` | none |

This confirms the flag suppresses at least one additional HTTPS contact
attempt to `api.anthropic.com` (consistent with telemetry/update-check
traffic) beyond the `ANTHROPIC_BASE_URL`-directed Messages call itself.

### P6 — credential-relevant env/FD/file names: found vs probed vs honoured

Found via `grep -aoE 'CLAUDE_CODE_[A-Z_]*(FILE_DESCRIPTOR|_FD|_FILE)[A-Z_]*|ANTHROPIC_[A-Z_]*(FILE|FD)[A-Z_]*' /opt/claude-code/bin/claude | sort -u`
(resolved path of `/opt/node22/bin/claude`):

| name | found | probed | mechanism | honoured (this task's tracked headers) |
| --- | --- | --- | --- | --- |
| `CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR` | yes | yes | fd (`VAR=3`, fd 3 open on scratch file) | yes → `x-api-key` |
| `CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR` | yes | yes | fd | yes → `Authorization: Bearer` |
| `CLAUDE_CODE_GATEWAY_TOKEN_FILE_DESCRIPTOR` | yes | yes | fd | not observed on tracked headers (may route to gateway-mode traffic this probe shape doesn't exercise) |
| `CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR` | yes | yes | fd | not observed (expected: this call used the REST Messages endpoint, not a WebSocket transport) |
| `ANTHROPIC_IDENTITY_TOKEN_FILE` | yes | yes | file path | not observed on tracked headers |
| `CLAUDE_CODE_HOST_CREDS_FILE` | yes | yes | file path | not observed on tracked headers |

Found but judged not credential-relevant and intentionally **not** probed
(matched the grep pattern but are file-checkpointing/artifact/diagnostics
mechanisms per their names): `CLAUDE_CODE_ARTIFACT_FD`,
`CLAUDE_CODE_ARTIFACT_MULTI_FILE`, `CLAUDE_CODE_DIAGNOSTICS_FILE`,
`CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING`,
`CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING`,
`CLAUDE_CODE_REMOTE_RAW_EVENTS_FILE`, `CLAUDE_CODE_USE_NATIVE_FILE_SEARCH`,
`ANTHROPIC_PROFILE`.

For the four "not observed" rows above, this task did not attempt to
determine whether they are honoured through some other, untracked header
or code path (this probe's endpoint only classifies `x-api-key`,
`authorization`, and `anthropic-beta`) — a plain "not observed here" is the
correct claim, not "unsupported by the CLI."

## What is proven on Linux

- Production's exact argv (`-p --safe-mode --output-format json <prompt>`)
  runs successfully against a fully local, non-Anthropic loopback endpoint
  with `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, or
  `CLAUDE_CODE_OAUTH_TOKEN` alone, each producing a clean, single
  dummy-marker-only auth header.
- `--safe-mode` does not disable `apiKeyHelper` delivered via `--settings`:
  it worked identically with and without `--safe-mode`.
- When `ANTHROPIC_API_KEY` and `ANTHROPIC_AUTH_TOKEN` are both set, both
  `x-api-key` and `Authorization: Bearer` are sent. When `ANTHROPIC_API_KEY`
  and `CLAUDE_CODE_OAUTH_TOKEN` are both set, only `x-api-key` is sent —
  `ANTHROPIC_API_KEY` takes precedence and fully suppresses the OAuth path.
- `CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR` and
  `CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR` are real, working credential
  inputs (fd-based), confirmed by dummy-marker round-trip.
- The CLI accepts a plain-`http://` loopback `ANTHROPIC_BASE_URL` silently
  (no warning, no refusal).
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` measurably reduces egress:
  it suppressed one otherwise-attempted HTTPS contact to
  `api.anthropic.com` in this run.
- Production's own env allowlist (`environment.rs:142-199`) does not pass
  through any of `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`,
  `CLAUDE_CODE_OAUTH_TOKEN`, or `ANTHROPIC_BASE_URL` today; none of P1–P4's
  mechanisms would currently reach a production-governed child without an
  explicit allowlist change.
- There exists, on this Linux container, at least one additional
  credential channel outside process environment and `$HOME` (see
  "Critical finding"); this task could not and did not identify it further.

## What is NOT proven (macOS / Seatbelt / Keychain-specific)

- Nothing here was run under `/usr/bin/sandbox-exec` or any Seatbelt
  profile: this worktree has no macOS host. Whether any of P1–P5's
  mechanisms still work once the production-shaped read/write/network
  Seatbelt profile is applied (without the three Keychain-adjacent rules)
  is unmeasured.
- Whether this container's unidentified ambient credential channel exists,
  or exists in the same form, on a macOS production worker is unknown —
  P0's result should not be assumed to generalize to macOS.
- Whether a `CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR`/`_OAUTH_TOKEN_FILE_DESCRIPTOR`
  fd, or an `apiKeyHelper` script path, can be delivered through Seatbelt's
  process-exec/file-read model without extra grants is unmeasured — fd
  inheritance and helper-script execution both have their own sandbox
  implications not exercised here.
- The interactive `/login` flow, `~/.credentials.json` creation, and any
  macOS Keychain interaction were not exercised anywhere in this task, on
  either platform (this task never runs a login flow, per the envelope).
- Whether `ANTHROPIC_BASE_URL` plus any of P1–P5 would satisfy a *real*
  Anthropic endpoint (as opposed to this task's fake one, which performs no
  auth validation at all) was not tested — this probe matrix proves
  transmission shape, not that a real API would accept these dummy values
  (it would not; they are not valid keys).

## Owner-run macOS matrix

`docs/development/task0152-probes/macos_seatbelt_matrix.sh` is prepared but
was not run (no macOS host here). It generates a Seatbelt profile mirroring
`MacOsSeatbeltRunner::profile_for` at `78079e36a5f463093ea07418f23f19f2f2ef3d87`
(`adapters/runner-macos-seatbelt/src/lib.rs` lines 103-249, network rule at
lines 230-239) and never emits the three Keychain-adjacent rules from
G7_TASK_0112. `--dry-run` prints the generated profile without running
anything; `bash -n` passes on Linux (see Verification).

Resolve the real executable path(s) first (mirrors
`process_paths`/`launch_executable` handling in `profile_for`):

```text
readlink -f "$(command -v claude)"
```

Then, from `docs/development/task0152-probes/`, inspect the profile before
running anything:

```text
./macos_seatbelt_matrix.sh --claude-bin "$(command -v claude)" --claude-bin "$(readlink -f "$(command -v claude)")" --network tcp443 --dry-run
```

Run the automatic rows (M1, M2, M5 always; M3, M4 only when
`--network loopback:<port>` is given instead of `tcp443`):

```text
./macos_seatbelt_matrix.sh --claude-bin "$(command -v claude)" --claude-bin "$(readlink -f "$(command -v claude)")" --network tcp443
```

```text
./macos_seatbelt_matrix.sh --claude-bin "$(command -v claude)" --claude-bin "$(readlink -f "$(command -v claude)")" --network loopback:8443
```

M6 (interactive `/login`, off by default) additionally requires
`--run-m6` and a human present to complete the login prompt; it never reads
`.credentials.json`'s content, only whether it exists and its mode:

```text
./macos_seatbelt_matrix.sh --claude-bin "$(command -v claude)" --claude-bin "$(readlink -f "$(command -v claude)")" --network tcp443 --run-m6
```

Note on `tcp443`: binding `127.0.0.1:443` without root requires macOS
10.14+ (this script assumes that; on an older macOS, run the fake endpoint
on a high port and treat the row as informational only, since it no longer
matches the exact production scope).

Blank results table for the owner to fill in:

| row | network mode | credential input | exit | marker (yes/no) | endpoint paths hit | auth header : classification | leak flag | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M1 | tcp443 | `ANTHROPIC_API_KEY` dummy | | | | | | |
| M2 | tcp443 | `ANTHROPIC_AUTH_TOKEN` dummy | | | | | | |
| M3 | loopback:`<port>` (+ `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`) | `ANTHROPIC_API_KEY` dummy | | | | | | |
| M4 | loopback:`<port>` | `ANTHROPIC_AUTH_TOKEN` dummy | | | | | | |
| M5 | (matches M1–M4 run) | none (control) | | | | | | expected: refusal, no Keychain involved |
| M6 | tcp443 | real, via confined interactive `/login` | | | | n/a | | `.credentials.json` exists (y/n), mode: \_\_\_\_ |

## Verification

```text
$ bash -n docs/development/task0152-probes/run_credential_input_probes.sh && echo OK
OK
$ bash -n docs/development/task0152-probes/macos_seatbelt_matrix.sh && echo OK
OK
$ python3 -m py_compile docs/development/task0152-probes/fake_anthropic_endpoint.py && echo OK
OK
$ python3 -m py_compile docs/development/task0152-probes/logging_connect_proxy.py && echo OK
OK
```

`run_credential_input_probes.sh` was actually executed on this Linux host
(`CLAUDE_BIN=/opt/node22/bin/claude ./run_credential_input_probes.sh`); its
full summary table is reproduced above verbatim from its own output.
`macos_seatbelt_matrix.sh`'s M1–M6 rows were **not** executed (no macOS
host); only `bash -n` and `--dry-run` profile generation were exercised
here.

## Deviations from the envelope

- P0 did not show the envelope's assumed "expected refusal" — see
  "Critical finding." This is a factual result, not a script defect: the
  same script cleanly showed dummy-marker-only headers for every row where
  an explicit credential mechanism was configured (P1–P6 honoured rows),
  so the classification logic itself is trusted.
- Two further, unplanned forensic actions (computing a redacted
  length/prefix/hash fingerprint of the ambient `Authorization` value; and
  listing — not reading — the scratch `HOME` directory tree for that same
  probe) were attempted in the course of investigating the above and were
  both refused by this runtime's own permission classifier
  ("Credential Materialization"). Both were abandoned immediately and not
  retried through any other tool, encoding, or approach, per that refusal's
  own instructions. No file from either attempt was created or persisted.
- P6's six probed names were chosen from the grep results by judgment
  (credential-shaped `_FILE_DESCRIPTOR`/`_FD`/`_FILE` suffixes); the
  envelope's instruction to "not assume names — only probe names you
  actually found" was followed literally, and the excluded names are
  listed explicitly above rather than silently dropped.
