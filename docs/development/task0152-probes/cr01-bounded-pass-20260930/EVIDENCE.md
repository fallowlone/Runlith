# TASK-0152 CR-01: one bounded feasibility pass

**Disposition: blocked evidence; CR-01 OPEN.** No complete admissible actual
CLI profile was established. Exact semantics and one existing-contract launch
alternative were evaluated. This checkpoint is not a submitted acceptance
candidate. ADR-0037 remains Proposed, owner B remains bounded remote delegation,
and the server-mediated remote operation set remains empty until P3.

## Subject and inherited evidence

Worktree: `/Users/artemmac/dev/runlith-task0152`; branch:
`task-0152-confined-provider-auth-adr`. Starting HEAD:
`84fee71111e64acb7b9a77bdaefe154e0bf53378`; task base/merge-base:
`78079e36a5f463093ea07418f23f19f2f2ef3d87`; compatibility main:
`5362f6efb7003b8c0dd24ede9bd5eb904791f525`. The index started clean with
three modified tracked and eight untracked files, exactly as the envelope
describes. All 17 source/artifact hashes and six verification-log hashes from
the follow-up session were verified before any edits in this pass.

Origin: `/var/folders/m6/xbrzl4wj1x574rlvxzz57fx00000gn/T/runlith-task0152-cr01-ldcx4l42/`.
Follow-up: `/private/tmp/runlith-task0152-evidence-session-5jzd0m7m/`.
Current collection: `/private/tmp/runlith-task0152-bounded-pass-20260930/`.
The original actual capture was inherited, not reproduced. Its historical
run/capture/normalization/synthetic-response files and harness hashes are kept.
RR-N1 is the inherited eight-line parser-refusal correction, not a new fix by
this pass. Original remediation tests and the original profile checker are kept.

## A: exact/versioned semantic evidence

The inherited mutable vendor guides and arbitrary-string SDK beta types are
the baseline, not fresh semantic proof. This pass retrieved and hashed four
files from exact vendor Git revisions on 2026-09-30:

- **S1**: [Claude Code changelog](https://github.com/anthropics/claude-code/blob/732e167ee9d71296b4b63d6f529ac1334513826a/CHANGELOG.md),
  revision `732e167ee9d71296b4b63d6f529ac1334513826a`, revision date
  `2026-09-30T06:30:04Z`, retrieved `16:56:43.397180Z`.
- **S2**: [vendor Claude API skill](https://github.com/anthropics/skills/blob/8a1541c4a3ffa5a20a5a91de0dcf3f0bab1d1ef4/skills/claude-api/SKILL.md),
  revision `8a1541c4a3ffa5a20a5a91de0dcf3f0bab1d1ef4`, revision date
  `2026-09-29T02:20:03Z`, retrieved `16:56:43.524784Z`.
- **S3**: [vendor prompt-caching material](https://github.com/anthropics/skills/blob/8a1541c4a3ffa5a20a5a91de0dcf3f0bab1d1ef4/skills/claude-api/shared/prompt-caching.md),
  same revision/date, retrieved `16:56:43.780617Z`.
- **S4**: [vendor model-migration material](https://github.com/anthropics/skills/blob/8a1541c4a3ffa5a20a5a91de0dcf3f0bab1d1ef4/skills/claude-api/shared/model-migration.md),
  same revision/date, retrieved `16:56:44.071409Z`, especially lines 1692-1708.

These are version-pinned vendor-authored documentation, not a server
implementation attestation. Exact retrieval URLs, SHA-256, sizes, revisions,
dates and local snapshot provenance are in [CHECKPOINT.json](CHECKPOINT.json).
The bounded searches yielded third-party reports in vendor repositories and
unofficial mirrors; those were excluded as semantic authority. An exact value
not found in the inspected sources proves that bounded search result only.
It does not prove that no vendor contract exists anywhere.

| Exact gap / required claim | Primary evidence actually obtained | Remaining limitation / admission impact |
| --- | --- | --- |
| `claude-code-20250219`: full feature/default and dispatch semantics with `x-app: cli`, billing attribution and API-key credential class | No exact definition in S1-S4; prior SDK accepts arbitrary beta strings | Unknown provider feature/default effects cannot be classified as inference merely from its name or empty tools. Reject the complete request. |
| `prompt-caching-scope-2026-01-05`: default scope and side effects when ephemeral markers omit scope/TTL, including attribution and system/message placement | S3 explains ordinary caching and cache-preserving forms; none of S1-S4 defines this exact beta | Need exact default scope, feature interactions and possible remote effects. Ordinary cache syntax is insufficient. |
| `mid-conversation-system-2026-04-07`: legacy enablement/defaults for the actual terminal text system message, cache marker and per-message output config | S2/S3 describe current GA system messages and newer extensions; exact legacy value absent | Current GA behavior does not establish this legacy header's equivalence or combination behavior. |
| `per-turn-control-2026-07-01`: exact alias and behavior in the captured text-bearing system message with top-level effort | S4 explicitly states that this older spelling resolves to the same per-message effort feature; S4 includes Opus 5.5 support and next-user-turn persistence | **Partial exact evidence gained.** S4's example is an empty-content effort-only message. It does not establish the captured terminal text-bearing/cached system message or all enclosing beta interactions. No list entry was removed to obtain positive acceptance. |

The complete combination still needs classification with the other six exact
betas: `context-1m-2025-08-07`, `interleaved-thinking-2025-05-14`,
`thinking-token-count-2026-05-13`, `context-management-2025-06-27`,
`mid-conversation-tool-changes-2026-07-01`, and `effort-2025-11-24`.
The inherited profile records partial documentation for these six. This pass
does not promote recognition into full-combination admission. Required coverage
also includes all ordered headers and API version `2023-06-01`, model/output
token constraints, `beta=true`, metadata/session attribution, all cache markers,
adaptive thinking with omitted display, keep-all thinking context, and top-level
and per-message effort. Free text remains untrusted data, not local authority.

**Precise missing external contract:** a versioned vendor definition or exact
vendor clarification covering the first three legacy/CLI values and the
text-bearing per-message effort case, together with the complete ten-beta
API-key/Opus-5.5 request. It must define feature/default enablement, precedence,
cache/context/thinking/continuation interactions and whether any shape enables
provider-side execution or downstream actions beyond inference. Unknown possible
actions cannot acquire authority under B. A citation, SDK literal, transport
success or absence of observed tools is insufficient.

## B: exactly one existing-contract launch alternative

[Hypothesis](../claude-2.1.283-system-prompt-api-key-v1/HYPOTHESIS.md):
`ClaudeConfig::default().proposal_only().with_system_prompt("role")` might
remove the built-in dynamic system message/effort/cache requirements. The
existing config constructor, argv builder, capability probe, argv test and
production planner establish that this flag combination already exists.
The config/argv/capability files are identical at starting HEAD and compatibility
main. The planner's relevant `bound_planner_config` launch expression is also
unchanged, but its complete file differs because of accepted TASK-0155's
trusted-host fixture environment work. The pre-capture hypothesis's statement
that all provider sources are byte-identical was too broad. That historical
hypothesis and its recorded hash are preserved; this is the explicit correction.
TASK-0155 is not reopened and supplies no confined-launch alternative here.

Observed CLI 2.1.283, same exact executable hash, unchanged capture limits:
exit 0, marker present, HEAD `/api/hello` then POST `/v1/messages?beta=true`.
All ten betas and the dynamic system-role/per-message-effort message remain.
Only the third top-level system text changes to `role`; body length changes
from 4914 to 1435. **Hypothesis refuted; no gap removed.** Complete sanitized
actual artifacts and separate wrapper/hypothesis provenance are in the
[alternative profile](../claude-2.1.283-system-prompt-api-key-v1/PROFILE.md).
No other launch was tried. No suppression, stripping, model/provider
substitution, new environment switch or production change was made.

## Verification and limitations

The two offline whole-sequence acceptance commands refuse Messages with
`unsupported_beta_contract` and exit 1. Default profile checks verify provenance,
strict complete normalization and negative mutations; they are not a positive
semantic admission test. The alternative adds source/hypothesis/argv checks and
sequence/scope widening to the inherited 26 specimen/parser negatives. A
negative must fail as mismatch or malformed JSON before the semantic refusal.
The checker, new wrapper/checks and historical author-added tests require
independent review; no independent acceptance PASS is claimed.

Before the alternative capture, actual harmless write probes succeeded for
both worktree and Git metadata, Seatbelt true exited 0, and all seven native
mechanical isolation cases passed. All relevant logs and helper/source hashes
are preserved. This is diagnostic isolation of tested paths, not production
Runner or full D11 proof. After the user changed the session to managed
permissions, Git metadata became read-only; current commit capability must be
reported independently from the earlier successful capture. No new capture is
needed or authorized to repeat the same result.

On 2026-09-30 the owner restored unrestricted filesystem access for the local
checkpoint commit. `CHECKPOINT.json` is the pre-commit evidence snapshot;
its HEAD and commit-verification fields describe that phase. The final
post-commit report and `result.json` in
`/private/tmp/runlith-task0152-bounded-pass-20260930-committed/` record the actual
commit identity and verify its complete file scope and hashes. The earlier
managed-session staging failure is retained as historical execution evidence.

Fresh offline verification, inherited exact-source checks and final scope/hash
review are recorded in CHECKPOINT.json and the final report/result. Full Rust
suites and owner live probes are outside this envelope. No credentials,
Keychain/auth/config directories, owner scripts or real provider APIs were used.

Pending: final exact ADR independent review and owner ratification; explicit
P1 request-integrity and P2 current-authority/attempt dispatch coupling
amendments; D11 all-address/firewall/dynamic-address residual; in-process
SecretDelivery/shared-process blast radius; signed-agentd ACL/core-dump trade-offs;
owner-only OAuth restriction. P3 still precedes the first remote operation.
OAuth revocation confirmation remains a separate incident, not a new CR-01 gate.
TASK-0114 refusal and accepted TASK-0155 are preserved. ADR INDEX reconciliation
is deferred; eventual integration must retain both ADR-0037 and ADR-0038 rows.

## Concrete owner decision package

1. **Recommended: record suitability as not established.** Preserve this
   blocked checkpoint, CR-01 OPEN, Proposed ADR, B and empty remote set. Submit
   the task-permitted negative conclusion for independent review and an explicit
   owner decision; this checkpoint itself is not ratification or task closure.
2. **Supply the exact missing vendor contract.** Provide an authoritative,
   versioned answer covering the complete recorded combination and precise
   cases above. Reopen only semantic review against that new input. No fresh
   authentication or repeat capture is needed to assess that contract.
3. **Authorize a separate launch-contract change.** If the vendor cannot
   supply that contract, a new scoped task may review a different pinned CLI
   protocol/launch configuration and its enforcement implications while keeping
   B. This needs new authorization and new actual evidence; the current envelope
   does not allow flags, suppression, stripping or provider substitution.

Do not start another identical research/capture loop without new input.
