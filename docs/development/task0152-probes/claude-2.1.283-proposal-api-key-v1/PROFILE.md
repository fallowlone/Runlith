# CR-01 candidate: claude-2.1.283-proposal-api-key-v1

**Blocked, not an admissible B profile.** A complete actual sequence exists;
the Messages request fails closed on four unresolved exact beta contracts.
No whole-sequence positive acceptance is claimed. B remains selected, ADR-0037
remains Proposed, and the remote operation set remains empty until P3.

The later [bounded checkpoint](../cr01-bounded-pass-20260930/EVIDENCE.md)
adds exact version-pinned vendor evidence for the older per-turn effort alias.
That is partial progress over the historical source assessment below; the
text-bearing system-message case and complete enclosing combination remain
unproved. Exactly one existing-contract system-prompt alternative was captured
separately and retained all ten betas. No historical run or source hash changed.

The subject is one proposal-only text inference turn. Its argv is exactly the
existing `ClaudeConfig::proposal_only()` branch of
`adapters/provider-claude/src/argv.rs::build_argv`:
`-p --safe-mode --tools "" --disallowedTools "mcp__*" --output-format json`
followed by the benign marker prompt in [run.json](run.json). No `--bare`,
beta suppression, model alias, custom system prompt or error-driven protocol
downgrade was used. This is a supported adapter configuration, not full coding
or interactive compatibility. No tools were declared/called, so there is no
tool-result continuation to omit or claim proven. Client-tool admission remains
unverified rather than categorically prohibited by B.

## Actual subject and isolation

- CLI: `2.1.283 (Claude Code)`, native executable identity
  `claude/versions/2.1.283`, SHA-256
  `d8cb1e5c79684cc12a8bfc813e3a2073406921b6245744b3009be3ab5651d21e`.
- [run.json](run.json) pins exact UTC start, argv, controlled environment names,
  byte/time/request/output limits, generator/harness hashes and capture hashes.
  API-key-class dummy bearer material is memory-only and never included there.
- Child environment is constructed from scratch. HOME, Claude config, XDG and
  scratch point into a fresh application-owned temporary runtime; cwd is an
  empty benign temporary workspace. No owner credential/config/repository read
  roots, proxy, SSH, cloud or provider session variables are inherited.
- The existing `gen_profile.sh` diagnostic Seatbelt approach grants only the
  exact CLI executable, temporary runtime/workspace and one `localhost:port`
  network rule. No port 443/mDNS/SecurityServer/`security` grant is added. Even
  the version command runs inside that profile. No auth-status/login/provision
  operation runs. The endpoint binds 127.0.0.1, never forwards and returns only
  synthetic responses. This is not the production Runner or full D11 address
  hold/filter implementation; the broader admitted local-address-set residual
  remains. No production confinement certification follows.
- Final capture: exit 0, marker present, two requests, no timeout/handler error;
  HEAD `/api/hello`, then POST `/v1/messages?beta=true` with `stream: true`.
  Synthetic SSE has six events ending in `end_turn`/`message_stop`. No retry or
  continuation was emitted in this run. [synthetic-responses.jsonl](synthetic-responses.jsonl)
  is synthetic endpoint output, not an authenticated provider receipt.

## Complete request classification

The exact candidate is represented by every captured header and recursively
every body field in [normalized.json](normalized.json). The checker compares
the entire typed specimen; it is not a generic Messages validator or a
production authorization gate. Specimen equality cannot prove provider
semantics. Therefore even equality leaves this POST unsupported.

| Position | Classification and admissibility rule | Evidence / remaining limit |
| --- | --- | --- |
| HEAD `/api/hello`, empty body, Bun user-agent/accept/encoding/connection/Host | Local unauthenticated health only; no upstream dispatch | Actual capture; D4/D27; [gateway guide](https://code.claude.com/docs/en/llm-gateway-protocol) |
| POST `/v1/messages?beta=true`, JSON type/finite original Content-Length, API `2023-06-01` | Inference candidate, must satisfy complete beta contract before any credential attachment | Actual bytes and D16/D24; original length is recorded separately from sanitized bytes |
| `x-api-key` | Credential class/presence category only; not authority | Sanitized in memory before persistence; no value/hash retained |
| `accept`, `content-type`, `user-agent`, `x-app`, eight `x-stainless-*` fields | Exact captured non-dispatch SDK metadata; D15 allowlist | Full names/values preserved, including retry-count 0 and SDK timeout 600 (not an attempt deadline) |
| Session ID, dangerous-direct-browser flag, Host, connection, accept-encoding, original length | Known ingress fields, dropped/rebuilt by D15/D5; no capability grant | Full sanitized representation retained; session ID correlated to metadata; D15 exclusion unchanged |
| `model`, integer `max_tokens`, boolean `stream` | Exact captured model `claude-opus-5-5`, 128000 declared output tokens, SSE | Evidence cap 128000 is a checker limit, not owner-selected production policy or spend accounting |
| Top-level `system[]`, user/system `messages[]`, text/cache markers | Prompt/attribution data; text cannot dispatch or grant authority | [gateway guide](https://code.claude.com/docs/en/llm-gateway-protocol), [system messages](https://platform.claude.com/docs/en/build-with-claude/mid-conversation-system-messages); exact legacy beta below remains unresolved |
| `tools: []` | No tool declarations in this named workflow | Actual full body; does not prove that unknown beta semantics are safe |
| `metadata.user_id` | Attribution string containing device/account/session fields; not authority | Actual string kept structurally, device/session values categorized; no provider-owned identity read |
| `thinking: {type: adaptive, display: omitted}` | Documented reasoning/display controls | [thinking](https://platform.claude.com/docs/en/build-with-claude/thinking); not proof of all enclosing betas |
| `context_management.edits: [{type: clear_thinking_20251015, keep: all}]` | Preserve prior thinking context; no server tool invocation in this form | Exact form documented in [context editing](https://platform.claude.com/docs/en/build-with-claude/context-editing) |
| Top-level and system-message `output_config: {effort: medium}` | Reasoning-effort controls | [effort](https://platform.claude.com/docs/en/build-with-claude/effort); current per-message header differs from captured alias |

All ten actual betas are retained, in original order:

| Exact beta | Evidence classification | Admission status for this combination |
| --- | --- | --- |
| `claude-code-20250219` | CLI-specific capability; no sufficient complete upstream semantics established | Unsupported; missing contract |
| `context-1m-2025-08-07` | Extended context, [gateway feature guide](https://code.claude.com/docs/en/llm-gateway-protocol) | Recognized feature; no whole-combination admission |
| `interleaved-thinking-2025-05-14` | [Extended thinking](https://platform.claude.com/docs/en/build-with-claude/extended-thinking) documents ignored legacy header on adaptive models | Recognized feature; no whole-combination admission |
| `thinking-token-count-2026-05-13` | [Messages response reference](https://platform.claude.com/docs/en/api/cli/beta/messages) describes coarse streaming thinking progress | Recognized feature; no whole-combination admission |
| `context-management-2025-06-27` | [Context editing](https://platform.claude.com/docs/en/build-with-claude/context-editing), exact keep-all form | Recognized feature; no whole-combination admission |
| `prompt-caching-scope-2026-01-05` | Ordinary ephemeral caching is documented; this exact scope-beta contract is not established | Unsupported; missing contract |
| `mid-conversation-system-2026-04-07` | Current [system-message guide](https://platform.claude.com/docs/en/build-with-claude/mid-conversation-system-messages) documents GA text messages without a beta | Exact legacy-header behavior not established; unsupported |
| `per-turn-control-2026-07-01` | [AWS adaptive-thinking documentation](https://docs.aws.amazon.com/bedrock/latest/userguide/claude-messages-adaptive-thinking.html) describes an effort alias, but not this Anthropic API combination | Exact upstream alias/interaction contract missing; unsupported |
| `mid-conversation-tool-changes-2026-07-01` | [System-message guide](https://platform.claude.com/docs/en/build-with-claude/mid-conversation-system-messages) documents tool changes by reference | No tool-change blocks present; unknown additions remain rejected |
| `effort-2025-11-24` | [Gateway guide](https://code.claude.com/docs/en/llm-gateway-protocol) documents effort/header pairing; [effort guide](https://platform.claude.com/docs/en/build-with-claude/effort) describes current controls | Recognized feature; per-turn alias gap remains |

Sources were retrieved read-only on 2026-09-30; they are mutable primary
documentation, not authority or a version-pinned server implementation.
The gateway guide recommends open forwarding lists for compatibility. That is
not Runlith B authority and cannot override its fail-closed classification.
Search results or names do not prove no hidden dispatch. The smallest missing
evidence is an authoritative, reviewable definition of those four exact beta
values and their interaction with this complete request (including per-message
effort), or a separately admitted real launch shape that does not require them.
No suppression/downgrade is silently substituted for that evidence.

Follow-up primary-source inspection of Anthropic's public
[TypeScript beta types](https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/beta/beta.ts)
and [Messages types](https://github.com/anthropics/anthropic-sdk-typescript/blob/main/src/resources/beta/messages/messages.ts)
does not resolve admission. `AnthropicBeta` accepts arbitrary strings in
addition to named literals; SDK acceptance is therefore not a closed semantic
allowlist. The Messages types describe per-system-message effort and ordinary
ephemeral cache markers, but do not establish the required exact legacy/CLI
beta interaction contract. These mutable client-side definitions are supporting
protocol evidence, not an attestation of server behavior. Third-party issue
reports in vendor repositories likewise do not supply that contract.

## Reproduction, sanitization and checks

Run `python3 -I -B ../capture_profile.py /exact/installed/claude` from this
directory to create a new private capture directory. The binary must have the
pinned hash for comparison; drift requires a new profile review.

[sanitized-capture.jsonl](sanitized-capture.jsonl) is the captured representation:
complete UTF-8 body text, ordered headers (including duplicates if emitted),
method/target and original body length. Before persistence the harness replaces
credential values, temporary root, UUIDs, device identity and Host by fixed or
correlated categories. No execution-enabling field/beta is removed or rewritten.
It is not a raw wire archive. `normalized.json` is derived by strict JSON parse,
with body text replaced by the complete body object; it is not separately
constructed positive evidence. `profile_check.py --normalize` reproduces it.
`run.json` links the actual run and exact harness/generator to capture/response
hashes; normalization pins the source capture hash.

`python3 -I -B ../profile_check.py` verifies that chain and all 26 mutations:
12 complete-fixture changes (unknown endpoint/destination/header/beta/nested
feature, duplicate header, remote tool, client declaration outside this named
profile, uncorrelated continuation, model, boolean token value, removed betas),
nine malformed/duplicate/ambiguous JSON cases and five token-shape/range cases.
Distinct mismatch/parse failures must occur before the known compatibility
refusal, so the negatives cannot pass merely because the baseline is blocked.
No positive client-tool/continuation claim is made from synthetic mutations.

`python3 -I -B ../profile_check.py --accept` exits **1**: local health accepted,
Messages `unsupported_beta_contract`, `actual_sequence_accepted: false`.
The default safety/evidence checks pass, but CR-01's whole-sequence positive
requirement does not. No live authentication proof, production confinement
certification, P1/P2 implementation or P3 remote admission exists here.
