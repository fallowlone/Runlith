# CR-01 alternative: claude-2.1.283-system-prompt-api-key-v1

**Blocked evidence, not an admission candidate.** The sole alternative launch
refutes the [recorded hypothesis](HYPOTHESIS.md): replacing the default system
prompt retains all ten beta values, the dynamic system-role message, its
per-message effort, and cache/context/thinking features. CR-01 remains OPEN.
Owner B remains bounded remote delegation; the remote set is empty until P3.

## Actual sequence and provenance

[run.json](run.json) records CLI 2.1.283, executable SHA-256
`d8cb1e5c79684cc12a8bfc813e3a2073406921b6245744b3009be3ab5651d21e`,
UTC start `2026-09-30T17:02:28.620868+00:00`, exact argv, limits, source hashes
and artifact hashes. [launch_capture.py](launch_capture.py) reuses the unchanged
historical capture harness, adds the existing `--system-prompt role` option,
and records its own source and pre-capture hypothesis hashes separately.
It changes no historical manifest or harness. The adapter configuration,
capability probe and existing argv test already permit this launch shape.
The hypothesis's compatibility-source identity claim is corrected in the
checkpoint: config/argv/capabilities match main exactly; the planner launch
expression matches, while its full file has accepted TASK-0155 fixture changes.

The run exits 0 in 0.571 seconds, emits the marker, and records exactly:

1. HEAD `/api/hello`, empty body, local synthetic health response.
2. POST `/v1/messages?beta=true`, streaming, original body length 1435 bytes.

[sanitized-capture.jsonl](sanitized-capture.jsonl) retains ordered headers and
the complete UTF-8 body after in-memory sanitization.
[normalized.json](normalized.json) is derived from that actual capture, not
an authored positive fixture. [synthetic-responses.jsonl](synthetic-responses.jsonl)
contains six synthetic SSE events and is not a provider receipt. No tools,
tool-result continuation, retry, or live authentication were observed.

The historical request was 4914 bytes. The only normalized body difference
is replacement of the third top-level system text with `role`; the environment
message remains. Headers are identical except original Content-Length.
Model `claude-opus-5-5`, integer `max_tokens: 128000`, `stream: true`, empty
tools, attribution metadata, ephemeral cache markers, adaptive omitted thinking,
keep-all thinking context and both effort positions are unchanged. Every header,
field and beta remains subject to the complete classification and limitations in
the [original profile](../claude-2.1.283-proposal-api-key-v1/PROFILE.md), with
the updated exact semantic evidence in the [bounded checkpoint](../cr01-bounded-pass-20260930/EVIDENCE.md).
Smaller text and an empty tools array do not establish provider semantics.

## Isolation boundary

The capture ran before the session returned to managed permissions. Worktree
and Git metadata write checks and a harmless Seatbelt true launch passed first.
A native diagnostic helper using the unchanged generator then observed seven
checks: permitted filesystem write, denied unrelated filesystem read/write,
denied unrelated executable, permitted local port, denied other local port,
and denied TEST-NET outbound TCP 443. Exact helper source/executable/generator
hashes and logs are retained in the checkpoint evidence.

The child environment was constructed from scratch with fresh private
HOME/config/runtime/workspace, an in-memory dummy API-key-class value, and a
local synthetic endpoint with no upstream forwarding. The profile grants no
Keychain, SecurityServer, `/usr/bin/security`, mDNS or broad HTTPS rule. Limits
remain 45 seconds, eight requests, 2 MiB per body, 8 MiB total bodies and 64 KiB
child output. This proves the tested diagnostic cases, not production Runner
enforcement or D11's entire admitted address set/firewall/dynamic-address case.
No real credential, auth/config directory or real provider API was accessed.

## Offline result and test integrity

```text
rtk proxy python3 -I -B docs/development/task0152-probes/claude-2.1.283-system-prompt-api-key-v1/check.py
rtk proxy python3 -I -B docs/development/task0152-probes/claude-2.1.283-system-prompt-api-key-v1/check.py --accept
```

The default command verifies the source/artifact/hypothesis chain, exact argv,
whole-capture normalization, and 28 specimen/parser negatives. It reuses the
unchanged original checker, including 12 mutations, nine ambiguous/malformed
JSON cases and five token cases, plus sequence and scope widening. Negatives
must return `profile_mismatch` or `invalid_fixture_or_json` before the baseline
semantic refusal. They prove exact-specimen rejection and strict parsing;
they do not prove a general protocol classifier or canonical authorization.

`--accept` exits 1: local health is accepted, Messages is refused as
`unsupported_beta_contract`, `actual_sequence_accepted: false`. The new checker
is agent-authored diagnostic evidence and needs independent review. Neither the
original checker nor its four-element unresolved-combination list was changed.
New version-pinned primary material partially explains the effort alias; it
does not prove the entire captured combination. P1/P2/P3, independent ADR review
and owner ratification remain pending.
