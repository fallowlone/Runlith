# Resource

Bitdefender AI Guardian (docs)

- url: https://ai.bitdefender.com/docs/ai-guardian
- product_url: https://ai.bitdefender.com/ai-guardian
- resource_type: vendor documentation site (no public repo; no commit SHA)
- doc_status: BETA documentation
- last_updated_stated: `2026-10-09`
- review_date: `2026-10-10`
- version_observed: install script 1.1.0 (334 lines, SHA-256 stated);
  agents: Claude Code ≥2.1.121, OpenClaw ≥2026.6.6, Hermes Agent
  ≥2026.5.28; macOS 13+ Apple silicon; free during BETA

# Reviewed revision

Evidence collected 2026-10-10 via page fetch (no local install, no
package inspection, no code). Sources actually inspected:

- `docs/ai-guardian` (full: model, 5 watches, 3 agents, hook+service+CLI,
  7-step onboarding, 16 commands, PASS/WARN/BLOCK, MCP allow/remove,
  audit calendar, allow-rule builder, DM alerts, telemetry).
- `ai-guardian` product page (full: 6 protections, with/without table,
  OWASP-informed risk map, install/run/telemetry rows, FAQ, legal).

Not inspected: installer script body, `.dmg`/package contents, hook and
service binaries, policy/audit file formats, scanner internals, cloud
reputation calls, DM integrations, `llms-full.txt`. All mechanism claims
are doc-stated. No external code copied into Runlith (§16.1).

# Research target

Evaluate Bitdefender AI Guardian's agent-security layer against Runlith's
G6 Policy/Secrets/Runner/Audit + agentd/agentctl isolation. Identify
missing capabilities, improvements, redesigns, replacements,
inspirations, validations, rejections.

# Executive summary

AI Guardian is a proprietary macOS BETA that guards the *user's own*
coding agents (Claude Code, OpenClaw, Hermes) where they already run: a
small per-agent hook (gateway plugin / Code hook / agent plugin) forwards
lifecycle events (prompt build, tool-about-to-run, skill install) over a
local user-private socket to a per-user background service; the service
evaluates against onboarding-built policy, writes a local encrypted audit
trail, watches credential paths, and DMs alerts 1:1. One CLI
(`aiguardian`) owns onboarding, scans, reviews, status, audit browser,
allowlist, MCP/skill/credential/notification management, hook reinstall.

Policy vocabulary is minimal and consistent: PASS (allowed), WARN
(flagged-through), BLOCK (stopped-before-run); MCP servers carry standing
allow/remove (originals backed up). Onboarding never changes anything
silently: detect agents → local skill scan (flagged default-block, allow
scoped to exact findings so new findings re-flag) → credential-path block
(default list + user globs) → per-server MCP verification (verified
defaults allow, failed/unresponsive defaults remove, in-place browser
auth+re-verify) → DM alert target → apply with backup, then scan-and-fix
(auto-fix only on selection, manual with exact guidance). Exceptions are
narrow by construction (tool + server + path/glob + exact findings,
checklist-built with plain-language preview, hit-count/last-fired shown
for stale-rule pruning).

Relevance to Runlith is direct on G6 Policy/Audit UX: Guardian ships the
operator loop Runlith's Policy Engine + audit lack as user experience —
guided setup with safe defaults, finding-scoped exceptions that re-fire
on novelty, plain-language rules with usage counters, severity-colored
audit calendar grouped by run with Blocked/Flagged/Allowed lenses and
one-key allow-from-event. It does not own durable tasks, approvals with
single-use exact bounds, independent verification, or execution isolation
— Runlith differentiators untouched. It is also a closed artifact (signed
package, account sign-in, beta telemetry without off switch, per-agent
plugins, macOS-only) — requirements and UX patterns only, never stack.

Most valuable, in priority order:

1. Finding-scoped exceptions that re-flag on novelty (ADAPT).
2. Safe-default onboarding with backup + scan-and-fix (ADAPT).
3. Exceptions-as-narrow-checklists with plain preview + usage counters
   (ADAPT).
4. Audit calendar grouped by run with lenses + allow-from-event
   (INSPIRED_ONLY).
5. Credential-path blocklist as Secrets UX complement (EXTERNAL_VALIDATION).
6. Local-first verdicts + encrypted local log as posture validation
   (EXTERNAL_VALIDATION).

# Resource information

- Type: signed macOS package (script or `.dmg`, notarization + Developer
  ID checks, exit 31 on failure, root-only temp dir, HTTPS-only,
  dry-run, idempotent-healthy) + per-agent hooks + per-user service +
  `aiguardian` CLI + Bitdefender-account activation (updates/uninstall
  owned by Bitdefender agent).
- Distribution: `curl …/install.sh | sudo sh` (with published SHA-256
  verify-before-run flow) or `.dmg` (31 MB); macOS 13+ Apple silicon;
  SSH install OK, desktop sign-in after.
- License/pricing: proprietary; free during BETA, stays free for early
  adopters after; age 16+, EULA + Privacy Policy; no completeness
  guarantee (stated).
- Status: open BETA; 3 agents; IDE agents coming; Windows/Linux planned;
  analysis on-device, prompts never leave Mac, some checks (URL
  reputation) use Bitdefender cloud, some features offline; overhead
  "minimal, not yet measured" (their words).
- Support: `aiguardian feedback` in-terminal form; hook reinstall command
  for stale/disabled hooks.

# Core concepts

## 1. Hook inside, service beside, CLI over both

Untrusted side (hook) never trusted blindly; decisions happen in the
per-user service over a user-private socket; nothing about a decision
leaves the machine (beta telemetry excepted — decisions metadata only,
§Privacy). Mirrors Runlith's distrust-the-edge instinct with a
dumber edge: the hook forwards, the service decides.

## 2. Three verdicts everywhere

PASS / WARN / BLOCK in wizard, log, and alerts; MCP standing actions
orthogonal (allow/remove = config membership, not event verdict). WARN
lets-through-but-flagged is the pressure valve Runlith's DENY/APPROVE/
ALLOW lacks as a first-class review state (approval is a gate, not a
flag).

## 3. Onboarding as policy construction

Detection → skill scan → credential block → MCP verify → alerts →
apply-with-backup + scan-and-fix. Safe defaults point the safe way
(flagged→block, unverified→remove, verified→allow), every override is
explicit, nothing applies before confirmation. Finding-scoped allows mean
old exceptions don't bless new behavior.

## 4. Exceptions are narrow checklists, not grants

Rule = tool + server + path-or-glob + exact findings, built at the
decision moment (audit event or review), previewed in plain language,
counted and aged in the allowlist. Stale-rule hygiene is a feature
(match counts + last-fired), not documentation advice.

## 5. Audit as investigation surface

Encrypted local log → calendar colored by worst-day → day stream grouped
by agent run → event detail (reason + triggering request + result) →
allow-from-event (`a`). Attention items also top the calendar so review
doesn't depend on DM delivery.

## 6. Six watches, stated plainly

MCP verify-before-trust (poisoned/unresponsive → remove); local skill
scan (hidden instructions, secret access, unsafe commands); credential
paths out of reach; per-tool-call execution checks; prompt screening
before model; plus DM alerts 1:1 (never group).

# Architecture and implementation

## Component map (doc-stated, binaries unread)

- Per-agent hook: OpenClaw gateway plugin / Claude Code hook / Hermes
  plugin; sees prompt-build, tool-pre-run, skill-install; forwards over
  local user-private socket; recoverable via `hook reinstall`.
- Per-user service: policy evaluation, audit encrypt+write, credential-
  path watch, alert dispatch; owns nothing outside the user.
- `aiguardian` CLI (16 commands): default `scan`, `onboard`, `status`,
  `audit`, `allowlist`, `mcp status|review|login|logout`, `skills`,
  `agents`, `credmon`, `notifications`, `hook reinstall`, `feedback`,
  `about`. Every subcommand answers `--help`; interactive browsers exit
  on `Esc`.

## Execution flow

Install (verify → root once → notarized+signed → healthy-skip) → sign in
→ onboard (detect → scan skills → block creds → verify MCP → alerts →
confirm-apply with backup → scan-and-fix) → everyday (scan/status/review
→ audit lenses → allow-from-event or review-allow → allowlist prune) →
reconfigure (agents/credmon/notifications/hook reinstall) anytime.

## Authentication

Bitdefender-account activation; per-server MCP browser auth (`mcp login/
logout`, re-verify on the spot); side's trust is socket-locality + hook-
distrust, not a token model evidenced here.

## Policy lifecycle

Built at onboarding, mutated at three points (audit event, skill review,
MCP review), always narrow + previewed + counted. Backups precede apply
and MCP removal. No expiry evidenced; staleness handled by visibility
(counts/last-fired), not TTL.

## Storage lifecycle

Policy + backups + encrypted audit all local on Mac; DM is notify-only
(same items in audit). No export/sync/backup-restore story evidenced.

## Observability

`status` one-screen (protection, cred-path count, MCP tally, hook armed,
DM target, subscription); `scan` findings auto-fix/manual-labeled;
`mcp status` per-server per-scope (incl. per-project); audit calendar/
lenses/detail. OTLP/metrics story absent — terminal-first, not
fleet-first (inverse of OpenSandbox).

# Security analysis

## Enforced technically (doc-stated, binary-unverified)

- Execution-time per-tool-call check with BLOCK-before-run; prompt
  screening before model; skill/MCP gating before use.
- Credential paths blocked from agent reach (default SSH/cloud/token
  list + user globs); blocked-path event shows reason + args + result.
- Unverified/unresponsive MCP → remove (backup first); verified → allow;
  per-project scope evidenced in status art.
- Local encrypted audit; decisions local; prompts never leave Mac.
- Installer verifies (HTTPS-only, root-only temp, notarization +
  signature, exit 31, dry-run, healthy-skip) with a published SHA-256
  out-of-band check.
- Hook never trusted blindly; service decides over private socket.

## Documented but not source-verified here

- Everything mechanism-level: hook event fidelity, service evaluator,
  scanner rules (prompt-injection patterns, hidden-instruction/secret/
  unsafe-command detectors), MCP verification protocol, audit crypto,
  socket auth, DM integrations, cloud reputation scope, overhead.

## Assumptions and limitations

- Single vendor BETA source; screenshots newer/older than text in places
  (agent picker predates Hermes support — captioned as such, good sign).
- Coverage is per-agent-plugin: unlisted agents unprotected; IDE agents,
  Windows/Linux pending. Protection follows integration, not the machine.
- Beta telemetry mandatory (no off switch): decisions-only metadata
  (rates, scanner IDs), never prompt/file/credential contents — stated,
  unaudited here.
- "Minimal overhead" unmeasured (their disclaimer); cloud-reputation
  dependency means offline ≠ full coverage ("some features").
- No adversarial evaluation published; with/without table is marketing
  illustration, not a test report (contrast OpenSandbox raw JSON).

## Attack surfaces

- Hook as code inside the agent: disable/stale hook = silent
  unprotection (reinstall command + status "armed" mitigate, detection
  latency unevidenced).
- Beta telemetry endpoint: metadata-only by policy, exfil-by-bug
  possible (contents-never-leave is a claim, not code reviewed).
- DM alerts to chat platform: notification content + target account
  become a leak/phish surface (audit-calendar parity mitigates
  dependence, not content scope).
- Allow-rule creep: narrow-by-construction + counts mitigate; no TTL/
  review-workflow evidenced.
- Installer pipe-to-sudo: SHA-pinned flow mitigates; server-compromise
  between fetch and compare is the residual (standard TOFU).
- Account sign-in + vendor updater own the box's guard: Bitdefender-
  account compromise or malicious update = guard compromise (proprietary
  update chain, no reproducible-build story here).

# Comparison with our project

## BD-01 — Enforcement point

Guardian: in-agent hook + local service on the user's Mac, per-tool-call
verdicts on agents it integrates. Runlith: agentd admission + RunnerPort
execution + Seatbelt on macOS. Same machine, adjacent layers: Guardian
watches the agent's tool calls from inside the agent loop; Runlith owns
the execution and its policy gate. Complementary positions, overlapping
vocabulary (allow/flag/block ≈ allow/approve/deny).

## BD-02 — Policy construction UX

Guardian: wizard with detection, safe defaults, confirm-before-apply,
backup, scan-and-fix. Runlith: Policy Engine semantics (exact-bound,
single-use, child-subset, fail-closed) without an onboarding narrative.
Runlith's semantics are stricter; Guardian's construction is kinder.

## BD-03 — Exceptions

Guardian: finding-scoped, checklist-built, previewed, counted, prunable.
Runlith: exact-bound single-use approvals (stronger: bound + consumable).
Guardian's re-flag-on-novelty and usage-aging are the portable ideas;
Runlith's single-use bound is the stronger primitive.

## BD-04 — Audit

Guardian: encrypted local log + severity calendar + run grouping +
lenses + allow-from-event. Runlith: hash-chained audit + journal
(tamper-evidence) with thinner review UX. Integrity vs investigability.

## BD-05 — Secrets

Guardian: credential-path blocklist (keep agents away from `~/.aws/…`,
`~/.ssh/…`) + leak detection before egress. Runlith: broker with
keychain + bindings + (planned) blind delivery per OpenSandbox R-02.
Guardian keeps agents away from secrets; Runlith must additionally hand
secrets to sandboxes safely. Different halves of one problem.

## BD-06 — Non-overlap

No durable tasks, no execution isolation (agents run natively on the
Mac), no independent verifier, no memory. Closed source, account-bound,
macOS-only, per-agent coverage. Runlith's local-first open monolith +
provider-neutral execution + evaluation stand apart.

# Bilateral comparison (as in mercury-agent)

## What is better in Runlith

- Approvals are exact-bound and single-use with child-subset and
  DENY-first; Guardian allows are finding-scoped but no evidenced
  consumption bound or hierarchical attenuation.
- Audit is hash-chained (tamper-evidence) over a journal; Guardian's log
  is encrypted-local with superior browsing but no evidenced chain.
- Execution owned (worktrees, RunnerPort, Seatbelt posture) vs agents
  running natively with a hook watching — Runlith contains, Guardian
  observes-and-vetoes.
- Independent Evaluation Port separates verdict from execution; Guardian
  verdicts are inline gates without a second judge.
- Open, local-first Rust monolith with offline posture vs proprietary
  package + account + vendor updater + mandatory beta telemetry.
- Provider-neutral durable Tasks vs per-agent plugins covering 3 agents.

## What is worse in Runlith (gaps Guardian exposes)

- No guided onboarding that builds policy from detection with safe
  defaults, confirm-before-apply, backup, and scan-and-fix (P1, R-01).
- Exceptions lack finding-scope re-fire: an approval that stays valid
  when the underlying findings change is a standing grant wearing a
  narrow name (P1, R-02).
- No plain-language exception preview + hit-count/last-fired aging for
  stale-rule pruning (P2, R-03).
- No severity-calendar audit browsing grouped by run with lenses and
  allow-from-event (P3, R-04).
- No credential-path blocklist UX keeping agents away from SSH/cloud/
  token paths by default (P2, R-05).
- No installer-verification narrative (published SHA + dry-run +
  healthy-skip + exit codes) for agentd/agentctl distribution (P3, R-06).

## What is better in AI Guardian

- Onboarding that never changes anything silently and ends in a
  verified state (apply-with-backup then scan-what-was-written).
- Safe defaults pointing the safe way at every step, overrides always
  explicit — secure-by-default as a wizard, not a paragraph.
- Finding-scoped allows: re-flag on novelty kills exception-creep at
  the primitive level.
- Narrow-by-construction rules with preview + counts: exceptions stay
  legible and prunable months later.
- Audit as calendar-first investigation (worst-day coloring, run
  grouping, lenses, reason+request+result detail) with DM parity that
  doesn't create DM dependence.
- Local-first verdict + encrypted log + prompts-never-leave posture
  with the beta-telemetry scope stated up front (mandatory-on disclosed,
  contents excluded).

## What is worse in AI Guardian

- Closed, account-bound, macOS-only BETA with per-agent coverage (3
  agents); no IDE agents yet; no Windows/Linux; unlisted agents run
  naked.
- Mandatory beta telemetry without off switch; cloud-reputation calls;
  offline is partial; overhead unmeasured.
- No consumption-bound approvals, no hierarchical attenuation, no
  verifier, no memory, no isolation — vetoes, not containment.
- No rule TTL/review workflow (visibility instead of expiry — weaker
  than single-use).
- No published adversarial evaluation; marketing table instead of raw
  evidence (contrast OpenSandbox's JSON + warnings).
- Vendor updater + account own the guard; no reproducible-build or
  digest-pin story for the guard itself (only the install script gets
  a SHA).

# Identified opportunities

- OPP-01 — Detection-built onboarding (safe defaults, confirm, backup,
  scan-and-fix) for agentd/agentctl setup.
- OPP-02 — Finding-scoped exceptions with re-fire on novelty for Policy
  approvals.
- OPP-03 — Plain-language exception preview + usage aging for allowlist
  hygiene.
- OPP-04 — Calendar/run-grouped audit browsing with lenses + decide-
  from-event for audit review.
- OPP-05 — Credential-path blocklist defaults for Secrets UX.
- OPP-06 — Installer verification narrative for distribution.

# Recommendations

## R-01 — Adapt detection-built onboarding with safe defaults (P1)

- Proposed change: agentd/agentctl first-run builds policy from what it
  finds (agents, skills, MCP servers, credential paths), proposes safe
  defaults, changes nothing until confirmed, backs up prior policy, then
  scans what it wrote with one-key fixes.
- Problem: OPP-01; policy construction UX gap.
- Approach: requirements + CLI flow design; safe defaults (flagged→deny,
  unverified→exclude); backup-before-apply; post-apply self-scan.
- Benefits: secure-by-default without operator expertise; verified end
  state instead of assumed.
- Complexity: Medium (owns G6 policy-setup scope).
- Risks: detection coverage lies (missed agent = unprotected); wizard
  fatigue → click-through (keep steps few, defaults safe).
- Acceptance: fresh-machine run ends protected with backup present and
  post-apply scan clean-or-actionable; no silent mutation test.
- Priority: P1 within policy-setup scope.
- Classification: `NOVEL_GAP` → `ADAPT`.

## R-02 — Adapt finding-scoped exceptions with re-fire (P1)

- Proposed change: every approval binds the exact findings reviewed;
  any new finding on the same subject re-fires review; old exceptions
  never bless new behavior.
- Problem: OPP-02; standing-grant creep.
- Approach: extend approval-bound model (exact-bound + single-use
  already) with finding-identity in the bound; re-fire on novelty.
- Benefits: exception-creep killed at the primitive; review moments
  stay meaningful.
- Complexity: Medium.
- Risks: finding-identity stability (noisy scanners → re-fire fatigue);
  mitigate with finding normalization before identity.
- Acceptance: new-finding-on-allowed-subject test re-fires; old-approval
  does not cover it.
- Priority: P1 within G6 Policy.
- Classification: `NEEDS_ADR` → `ADAPT`.

## R-03 — Adapt narrow-checklist exceptions with preview + aging (P2)

- Proposed change: allow-rule builder as checklist (tool + server +
  path/glob + exact findings) with plain-language preview and
  match-count/last-fired aging in the allowlist view.
- Problem: OPP-03.
- Approach: UX requirements for approval/policy review surfaces.
- Benefits: legible, prunable exceptions months later.
- Complexity: Low-Medium.
- Acceptance: rule-preview test; stale-rule surfacing test.
- Priority: P2.
- Classification: `NOVEL_GAP` → `ADAPT`.

## R-04 — Borrow calendar/run-grouped audit review (P3)

- Proposed change: audit review groups by run, colors by worst event,
  lenses Blocked/Flagged/Allowed, detail shows reason+request+result,
  decide-from-event (approve/deny) inline.
- Problem: OPP-04.
- Approach: notes for audit-view work; no storage change (chain stays).
- Benefits: investigability without weakening tamper-evidence.
- Complexity: Low (view layer).
- Acceptance: notes cited in audit-view scope.
- Priority: P3.
- Classification: `ALREADY_DESIGNED` (chain) + view gap →
  `INSPIRED_ONLY`.

## R-05 — Cite credential-path blocklist as Secrets UX (P2)

- Proposed change: default credential-path blocklist (SSH/cloud/token)
  + user globs, blocked-from-agent by default, managed alongside broker
  bindings.
- Problem: OPP-05; complements OpenSandbox R-02 (blind delivery keeps
  secrets from sandboxes; this keeps agents from secret paths).
- Approach: checklist for Secrets scope; pairs with broker (away-from +
  safe-handoff).
- Benefits: cheapest exfil reduction available (don't reach, don't leak).
- Complexity: Low.
- Acceptance: review cites default list + glob management.
- Priority: P2.
- Classification: `ALREADY_DESIGNED` → `EXTERNAL_VALIDATION`.

## R-06 — Cite installer verification for distribution (P3)

- Proposed change: published SHA + verify-before-run + dry-run +
  healthy-skip + explicit failure exits for agentd/agentctl artifacts.
- Problem: OPP-06.
- Approach: checklist in distribution docs; pairs with OpenSandbox R-05
  (image signing) at artifact layer.
- Benefits: supply-chain posture without new code.
- Complexity: Low.
- Acceptance: docs carry SHA flow + dry-run + exit codes.
- Priority: P3.
- Classification: `ALREADY_IMPLEMENTED` (discipline) →
  `EXTERNAL_VALIDATION`.

# Rejected or inapplicable ideas

- **Adopt AI Guardian as Runlith's policy/audit layer.** Reason:
  proprietary closed BETA, account-bound, per-agent plugins, macOS-only;
  conflicts with open local-first Rust monolith + offline posture.
  (REJECT as stack.)
- **Copy hooks/service/CLI into Runlith.** Reason: §16.1 prohibits
  external code import; binaries unread; vendor-owned update chain.
  Requirements/UX only. (REJECT.)
- **Treat WARN as sufficient for dangerous actions.** Reason: flagged-
  through is review UX, not a containment verdict; Runlith's DENY-first
  + exact-bound approvals remain the authority for irreversible scope.
  WARN-equivalent needs an ADR before any adoption as a verdict.
  (REJECT as verdict; ADAPT as review lens only.)
- **Mandatory telemetry without off switch.** Reason: conflicts with
  local-first offline posture; decisions-metadata-only still phones
  home. Runlith telemetry (if any) stays opt-in. (REJECT.)
- **DM alerts as primary channel.** Reason: chat-target account becomes
  leak/phish surface; audit-calendar parity is the portable part, DM is
  optional transport. (REJECT as primary; optional transport only.)
- **Rely on vendor cloud reputation offline story.** Reason: "some
  features" offline means coverage degrades silently; Runlith guards
  must fail closed or state degradation explicitly. (REJECT.)

# Proposed implementation plan

Independent, ordered by leverage/cost (no implementation authorized by
this research file):

1. R-01 detection-built onboarding (with policy-setup scope).
2. R-02 finding-scoped re-fire (with G6 Policy).
3. R-03 exception preview + aging (with policy review surfaces).
4. R-05 credential-path blocklist (with Secrets scope).
5. R-04 audit review notes (audit-view scope).
6. R-06 installer verification (distribution docs).

Each step needs finding-identity tests, narrow-exception keepers,
backup-before-apply assertions, stale-rule surfacing checks, and docs
updates. No step changes public versioning or cross-context storage
reads.

# Evidence and references

Directly verified (page fetch 2026-10-10):

- https://ai.bitdefender.com/docs/ai-guardian (BETA docs, last updated
  2026-10-09: model, 5 watches, 3-agent table, hook+service+CLI, 7-step
  onboarding with 4 captioned screens, 16-command table, PASS/WARN/BLOCK
  + MCP allow/remove, audit calendar/day/detail screens, rule-builder +
  allowlist screens, DM alerts, telemetry scope).
- https://ai.bitdefender.com/ai-guardian (product page: 6 protections,
  with/without table, OWASP-informed risk map, install/run/telemetry
  rows, FAQ incl. per-agent plugin model + macOS-only + offline-partial
  + unmeasured overhead, install SHA-256
  `2e6063dd…d2e883` script v1.1.0, `.dmg` link, legal).

Inferred (not verified): all binaries, scanners, protocols, crypto,
sockets, integrations. Marked as such in §Security.

Runlith references: `AGENT.md` §§4/7/16.1/16.2/20, `ARCHITECTURE.md`,
`docs/architecture/INVARIANTS.md` (INV-008/022/029/115),
`docs/architecture/adr/ADR-0012-structured-memory-provenance.md`,
`planning/roadmap.yaml` (G6 REMEDIATION_REQUIRED, G9/G10 deferred),
`planning/tasks/queue.yaml`, `crates/AGENTS.md`, OpenShell/OpenSandbox/
Utopia reports (R-02 binding/delivery, G6 network scope).

# Final assessment

AI Guardian is the strongest Policy/Audit-UX reference reviewed so far:
detection-built onboarding with safe defaults and verified end state,
finding-scoped exceptions that re-fire on novelty, narrow-checklist
rules with preview and aging, calendar-first audit with decide-from-
event, credential-path defaults, local-first verdicts with stated
telemetry scope. Its value to Runlith is UX + requirements (R-01…R-03
adapt, R-04…R-06 cite), not stack or code. Its closed BETA nature
(account, vendor updater, mandatory telemetry, per-agent coverage,
macOS-only, no approvals-with-bounds, no verifier, no isolation) is
inapplicable to Runlith v1's open, local-first, policy-gated monolith.

Recommended next steps: keep this file as evidence; fold R-01/R-02 into
policy-setup/G6 scope (requirements, then CLI flow); cite R-03/R-05 in
policy/Secrets reviews; keep R-04/R-06 as audit-view/distribution notes.
Do not depend on, integrate, or import AI Guardian artifacts.

## Novelty / adoption summary

| Finding | Novelty | Adoption |
|---|---|---|
| Detection-built onboarding (defaults, confirm, backup, self-scan) | `NOVEL_GAP` (no setup narrative) | `ADAPT` (R-01) |
| Finding-scoped exceptions with re-fire | `NEEDS_ADR` | `ADAPT` (R-02) |
| Narrow-checklist exceptions + preview + aging | `NOVEL_GAP` | `ADAPT` (R-03) |
| Calendar/run-grouped audit + decide-from-event | view gap on chained store | `INSPIRED_ONLY` (R-04) |
| Credential-path blocklist defaults | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` (R-05) |
| Installer verification narrative | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` (R-06) |
| Guardian stack/hooks as policy layer, WARN-as-verdict, mandatory telemetry, DM-primary, cloud-offline | conflicts or out-of-scope | `REJECT` |
