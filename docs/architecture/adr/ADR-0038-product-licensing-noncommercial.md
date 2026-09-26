# ADR-0038 — Product licensing: noncommercial-only core

Status: **Proposed** (TASK-0153; awaiting owner ratification after independent review)
Gate: cross-gate (governance)

ADR-0037 is reserved for TASK-0152 (confined Claude provider authentication),
being drafted on branch `claude/adr-0037-provider-confinement-7qvn2z`; it is
unrelated to this ADR's licensing decision.

> Changes the repository default license from PolyForm Shield 1.0.0 to
> PolyForm Noncommercial 1.0.0: free use becomes noncommercial-use-only (all
> features included), any commercial use requires a separately executed
> commercial license, and the core is described as source-available, never
> open source. This is not legal advice; it is a product/governance decision
> pending owner ratification, recorded here so the review and ratification
> have a fixed text to evaluate.

## Context

The repository's current default license, verified byte-identical to
upstream, is [PolyForm Shield 1.0.0](../../../LICENSE). Shield permits broad
use — including ordinary internal business use — subject only to a
noncompete restriction on providing a *competing* product. It does not
require payment for internal commercial use that does not compete with the
licensor's own offering. That does not meet the owner's goal: the owner wants
**all** commercial use of the product, by any individual or company of any
size, to require a paid commercial license, while noncommercial use (personal
use without any anticipated commercial application, and use by noncommercial
organizations) remains free with the full feature set. Evaluation for
commercial adoption is commercial use, not personal use; see Open questions
for whether a separate commercial evaluation/trial license should be offered
(none exists today).

Today's monetization strategy (`AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`
§3) also gates *features* by tier (Community vs. Pro/Teams/Enterprise)
independently of Shield. Under the new decision, tiers stop gating **software
features** for noncommercial users: every software feature in the licensed
product — including team/organizational/governance features — is available
to all noncommercial users, including noncommercial organizations. What the
paid tiers sell is (a) the commercial-use license itself, and (b) separately
contracted services (support, SLAs, rollout assistance) and hosted/cloud
add-ons, neither of which are software features granted by the license.

The owner separately wants future eligibility for AI-vendor open-source
maintainer programs (e.g. Claude for Open Source, Codex for Open Source).
Eligibility is decided by each program, not asserted here.
**Judgement/secondary-source, not independently verified by this task:** a
secondary source states such programs require an OSI-approved license.
Anthropic's program page (read 2026-09-25, orchestrator-verified) lists
adoption thresholds (500+ dependent repos, or 200k+ monthly downloads, or 20+
external contributors in 12 months, or OpenSSF criticality ≥ 0.4); that list
is non-exhaustive — the same page also describes maintainer/contributor-based
tracks (e.g. 100+ merged pull requests to repositories the applicant does not
own, or being a core contributor of a recognized foundation project). That
page is about adoption thresholds, not license approval itself; the
OSI-approval requirement is carried here as an unverified secondary claim.
PolyForm Noncommercial is not an OSI-approved license, and a
noncommercial-only core is, at best, unlikely to qualify as an "open-source
project" for such a program — a judgement, not a verified program ruling.

## Decision

**D1 — Default license.** The repository default license becomes
[PolyForm Noncommercial 1.0.0](https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Noncommercial-1.0.0.md),
replacing PolyForm Shield 1.0.0. `LICENSE` is a byte-identical copy (verified
by `sha256sum`, see Verification below). The licensor is Artem Hrechuk, a
natural person, not a legal entity; the root `NOTICE` file (new, plain text)
carries the plain-text line beginning `Required Notice:` that the license's
Notices clause requires recipients to also receive, and must be redistributed
together with the software for that reason (see References). Whether a
verbatim, unamended adoption of PolyForm Noncommercial 1.0.0 remains the
final license choice is itself open — see Open questions,
"Government-institution use and the noncommercial-organization exclusion."

**D2 — Permitted free use, exactly per license text.** Free use is limited to
the license's own permitted purposes: any noncommercial purpose; personal use
for research, experiment, and testing for the benefit of public knowledge,
personal study, private entertainment, hobby projects, amateur pursuits or
religious observance without any anticipated commercial application; and use
by a charitable organization, educational institution, public research
organization, public safety or health organization, environmental protection
organization, or government institution, **regardless of the source of
funding or obligations resulting from that funding.** As drafted, this ADR
does not narrow that text: a publicly funded university or a government
agency's use would be free under a verbatim adoption exactly as the license
states, even though it is a consequence some readers might not expect from a
"noncommercial" label. **This is now contested and unresolved:** the owner
wants government-institution use to require payment, which a verbatim
PolyForm Noncommercial adoption cannot express — see Open questions,
"Government-institution use and the noncommercial-organization exclusion."
Until that is resolved, D2 as stated is the current Proposed text, not a
ratified position.

**D3 — Commercial use requires a paid license; no feature gating for
noncommercial users.** Any commercial use of the product — by an individual
commercial user (e.g. a freelancer) or a company of any size, including
ordinary internal business use — requires a separately executed commercial
license from the licensor. That commercial license is what Pro Local, Pro
Subscription, Teams and Enterprise sell; it is not an additional feature
unlock layered on top of a free "Community" feature set. Every software
feature in the licensed product — including team/organizational/governance
features — is available to all noncommercial users, including noncommercial
organizations (Community tier); tiers stop gating functionality for
noncommercial users. What is paid is (a) the commercial-use license itself,
required for any commercial use, and (b) separately contracted services such
as support, SLAs, rollout assistance and hosted/cloud add-ons, which are not
software features granted by the license. No commercial license or EULA is
executed as of this ADR.

**D4 — Source-available, never open source.** The core must always be
described as "source-available." It must never be described as "open
source," "OSS," or similar, in documentation, marketing or code comments,
because PolyForm Noncommercial is not an OSI-approved license.

**D5 — OSS-program strategy via separately scoped OSI components only.** The
owner's interest in AI-vendor open-source maintainer-program eligibility is
pursued only through genuinely independent, separately scoped components
released under an OSI-approved license — the already-selected Apache-2.0
direction for a future Plugin SDK, public schemas and examples. Each such
component must earn its own adoption independently; none of it changes the
Runlith core's license, and the core is not expected to become OSI open
source under this decision.

**D6 — No enforcement code authorized by this ADR.** This ADR authorizes no
license-enforcement, entitlement-checking or phone-home code. It is a
documentation/governance decision only. Any future entitlement mechanism
(license-key validation, seat counting, online activation, etc.) requires its
own ADR, and that ADR must be consistent with
[ADR-0014](./ADR-0014-local-first-privacy.md)'s local-first, no-account,
no-network-dependency, no-exfiltration commitments — an enforcement mechanism
that silently phones home or requires an account for core local functionality
would contradict ADR-0014 and is not pre-approved by this decision.

**D7 — Contributor terms.** Before accepting substantial external
contributions, establish contributor terms sufficient for the intended
noncommercial/commercial licensing model, e.g. an appropriate CLA; a DCO
alone is not an additional grant of commercial relicensing rights. No CLA is
established by this ADR; drafting one is a follow-up (see Migration impact).

**D8 — Transition.** The GitHub repository (`fallowlone/Runlith`) is private;
git history as of `69b2221` (181 commits) shows no external human
contributors: authors are the owner (147 commits, across two owner
identities), `Claude <noreply@anthropic.com>` (33 commits) and `Runlith
Orchestrator` (1 commit); 66 of 181 commits carry a Claude co-author trailer,
and 68 of 181 commits involve AI authorship or co-authorship (orchestrator-
verified 2026-09-26). Trailer counts measure commit attribution, not the
share of AI-generated code. No third-party Shield licensee is known to exist
from public distribution of this repository. This ADR does not attempt to
retroactively alter any grant: if any copy of this repository was already
distributed under PolyForm Shield to a third party before this change, that
copy's Shield grant is unaffected by this decision; the license change
applies going forward to the repository default as of ratification.

**Merge sequencing.** The LICENSE/NOTICE change described by this ADR takes
effect only when the branch carrying it is merged into the repository
default branch. Merge happens only together with, or after, owner
ratification of this ADR; this ADR being Proposed and its files existing on
a branch is not itself a license change. `planning/tasks/queue.yaml`'s
TASK-0153 `exit_criteria_summary` records the same gate (see Migration
impact).

**D9 — Hosted/cloud add-on constraints.** Anthropic's legal/compliance page
(read 2026-09-25) states that preinstalling or running Claude Code in a
product or service requires Commercial Terms, that each end user must
authenticate with their own credential (API key, subscription or third-party
credential), and that the operator may not pay for, resell or intermediate
Claude usage — **unless Anthropic and the operator have mutually agreed
otherwise.** Separately, and generally for developers (not limited to
preinstalled/hosted products), the same page bans collecting, storing or
intermediating Claude.ai credentials or session tokens. **Judgement, not a
verified requirement:** whether and how these conditions would apply to a
future Runlith hosted/cloud add-on built on a Claude-based provider
integration (e.g. relay, hosted model capacity, remote workers) is this ADR's
own inference from the source page, not something the source states about
Runlith specifically. This is a recorded external constraint, not a Runlith
licensing term, and does not itself change this ADR's decisions D1–D8.

## Alternatives considered

- **Keep PolyForm Shield 1.0.0.** Rejected: Shield does not require payment
  for ordinary internal business use that does not compete with the
  licensor's product; it fails the owner's "all commercial use pays" goal.
- **PolyForm Small Business 1.0.0.** Free for organizations under 100 people
  and under $1,000,000 (2019 USD) revenue. Rejected: the owner wants all
  commercial use, including small commercial users, to require payment; a
  revenue/headcount carve-out contradicts that.
- **FSL-1.1-ALv2 (Fair Source, converts to Apache-2.0 after 2 years).**
  Competing-use ban only; ordinary internal business use is free. Rejected:
  same gap as Shield for the owner's goal, plus an unwanted automatic future
  conversion to a permissive license for old versions.
- **BSL 1.1 with an Additional Use Grant.** Possible in principle, but the
  owner does not want a timed change-date conversion to an open license, and
  judges the "you'll eventually be open source, but not yet" positioning to
  carry reputational cost the owner prefers to avoid.
- **Elastic License 2.0.** Prohibits offering the software as a hosted
  service and provides a license-key/feature-restriction clause, but does
  not require payment for ordinary internal business use. Rejected for the
  same reason as Shield and FSL.
- **AGPL-3.0 core + commercial dual license.** OSI-approved, and would make
  the core eligible for OSS maintainer programs. Rejected: AGPL cannot oblige
  ordinary internal business use to pay (a company may run AGPL software
  internally, without distributing it, and owe nothing) — it fails the "all
  commercial use pays" requirement at its foundation, not just at the
  margins.
- **Apache-2.0/MIT open core + proprietary `ee/` directory (open-core
  model).** The open core itself would be free for commercial use too
  (Apache-2.0 grants that); only the separately walled-off proprietary
  features/services would be paid. Rejected: contradicts "commercial use of
  the product and **all its features** requires a paid commercial license" —
  an open-core split makes the core itself free for commercial use, which is
  exactly what the owner does not want.

## Consequences

- Any individual freelancer or company using Runlith commercially must obtain
  a paid commercial license; there is no free commercial-use tier at any
  company size, unlike Shield or Small Business.
- Noncommercial adoption (personal, academic, charitable-organization,
  government) is maximally easy under the current Proposed text: full
  features, free, license text alone is sufficient — but government-use
  eligibility is itself an open, contested question (see Open questions).
- Eligibility for OSI-scoped/open-source-only program benefits (e.g.
  AI-vendor open-source maintainer programs) is decided by each program.
  PolyForm Noncommercial is not an OSI-approved license, so a
  noncommercial-only core is, at best, unlikely to qualify while it carries
  this license; only separately scoped Apache-2.0 components (Plugin SDK,
  schemas, examples) can pursue that path independently, and only if those
  components independently earn adoption.
- The monetization doc's tier semantics change: Community stops being a
  feature-limited free trial and becomes the full-featured noncommercial-use
  license; Pro/Teams/Enterprise become the commercial-use licensing vehicle,
  not merely a feature unlock.
- No runtime, code or enforcement change ships with this ADR; it is a
  documentation/governance decision only, pending ratification.

## Security impact

None on runtime trust boundaries. This ADR authorizes no code change and no
enforcement mechanism (D6); it does not touch `agentd`, IPC, the Policy
Engine, or any trust boundary defined by ADR-0007/0013/0020/0026. If a future
entitlement-enforcement ADR is proposed, that ADR — not this one — must carry
its own security-impact analysis for whatever mechanism it selects.

## Privacy impact

None beyond what ADR-0014 already commits to; this ADR explicitly preserves
ADR-0014 (D6) and authorizes no telemetry, licensing-account requirement, or
phone-home check. A future entitlement mechanism must be evaluated against
ADR-0014 in its own ADR before implementation.

## Migration impact

This task changes only documentation/governance files: `LICENSE`,
`LICENSING.md`, `README.md`'s License section, targeted sections of
`AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`, this ADR, the ADR
index, and `planning/tasks/queue.yaml`. No code, `product/version.toml`, or
Cargo manifest changes are made here. Follow-up work required before this
decision is fully realized, none of it authorized to happen automatically by
this ADR:

- Add `license = "PolyForm-Noncommercial-1.0.0"` — this identifier exists on
  the SPDX License List (SPDX is not the same as OSI approval; the license is
  not OSI-approved) — to workspace Cargo manifests; no workspace crate
  currently declares a `license` field, verified 2026-09-26.
- Draft an actual commercial license/EULA with qualified legal counsel; none
  exists today, and this ADR does not create one.
- Draft and adopt a CLA before accepting the first external contribution
  (D7); none exists today.
- Establish a trademark policy for the "Runlith" name; none exists today,
  and this ADR does not assert a trademark registration.
- Obtain qualified legal review before public distribution, including of the
  final license text, the commercial-license/EULA draft, and the copyright
  status of AI-assisted authorship in this repository. The U.S. Copyright
  Office's *Copyright and Artificial Intelligence* report, Part 2 (January
  2025), states that material generated solely by AI is not copyrightable
  while human-authored selection, arrangement or modification can be; with
  66 of 181 commits in this repository carrying an AI co-author trailer
  (verified 2026-09-26), this repository's copyright ownership over
  AI-assisted portions is a genuine legal-review question, stated here as a
  risk requiring legal review, not resolved as a legal conclusion.
- Check `PROJECT_CHARTER.md` and
  `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` for license-related
  wording that may need to follow this decision once ratified; both files
  are outside this task's allowed file list and are not edited here.
- Reconcile sections of `AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`
  that still reflect the pre-ADR-0038 model and were not in this task's
  allowed edit scope: §5 "Community rights concept" (~line 1205, lists
  "evaluation" as a potential free/community right), §6 "Commercial use of
  the product should remain allowed" (~line 1228), §7/§12/§33 (PolyForm
  Shield recommendations/ranking, ~lines 1251, 1322, 1795), and §34 "Critical
  unresolved questions" Q1/Q2 (~lines 1826-1827, which ask whether commercial
  internal/small-company use should be free — already answered "no" by this
  ADR). Also reconcile
  `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §86 "Monetization
  strategy" → "Community — Free" (~line 4058), which predates this decision.
  A follow-up reconciliation task should update or annotate all of these;
  none of them are edited by this ADR or this task.
- Merge sequencing: this ADR's LICENSE/NOTICE change takes effect only when
  merged into the repository default branch, and merge happens only together
  with or after owner ratification of this ADR (see D8 "Merge sequencing").
  `planning/tasks/queue.yaml`'s TASK-0153 `exit_criteria_summary` records the
  same merge gate.

## Open questions for ratification

- **Licensor identity for a `Required Notice:` line — Resolved.** The
  licensor is Artem Hrechuk, a natural person (no legal entity). The root
  `NOTICE` file carries `Required Notice: Copyright 2026 Artem Hrechuk
  (https://github.com/fallowlone)` (see D1, References).
- **Exact commercial terms.** Price, contract form, and whether Pro
  Local/Subscription/Teams/Enterprise purchase terms are drafted as one
  commercial license or several, remain undecided; §3 of the monetization
  doc keeps its prices as hypotheses.
- **Government-institution use and the noncommercial-organization
  exclusion — OPEN, reopened.** D2 as drafted adopts the PolyForm
  Noncommercial text verbatim, including its unconditional "regardless of
  funding" language, which makes use by a government institution free. The
  owner now wants government-institution use to require payment (i.e. to be
  treated as requiring a commercial license), which is incompatible with a
  verbatim, unamended adoption of PolyForm Noncommercial 1.0.0: the license's
  Noncommercial Organizations clause grants government-institution use
  unconditionally, and this ADR does not have authority to silently narrow
  licensed text. Expressing the owner's intent would require either a custom
  license variant/amendment carving government institutions out of the
  Noncommercial Organizations clause, or selecting a different base license
  entirely — both are a bigger change than this task's scope, are not
  decided here, and require their own ADR amendment or replacement before
  ratification. Until resolved, D1/D2's verbatim-adoption text should be
  read as the current Proposed draft, not a settled position.
- **Freelancer/contractor working for a noncommercial organization.**
  PolyForm's text does not expressly resolve whether a paid contractor or
  freelancer building software for a government/educational/charitable
  institution is using the software "for" that institution (free, if the
  institution's use is itself a permitted purpose) or for the contractor's
  own commercial activity (requiring a commercial license). The owner's
  stated position: freelancers and contractors pay for commercial use,
  including when the work is performed for a government, educational or
  charitable institution — the contractor's own use is treated as
  commercial regardless of the institution's noncommercial status. This
  position is recorded here as the owner's intent for future ADR/LICENSING.md
  wording; it is not yet reflected in `LICENSING.md`, which does not state a
  position on this question.
- **Commercial evaluation/trial license.** No separate commercial
  evaluation or trial license exists today. Evaluating the product for an
  intended commercial adoption is commercial use, not a permitted
  noncommercial purpose, under a verbatim PolyForm Noncommercial adoption;
  whether the owner wants to offer a separate, explicitly scoped commercial
  evaluation/trial license (e.g. a time-boxed free trial for prospective
  commercial customers) is open and not decided by this ADR.

## References

- [LICENSE](../../../LICENSE) — PolyForm Noncommercial 1.0.0, verbatim
  (sha256: `c0ea4a896d2c8c394b29f9427589996db826cd501c512279ff0ed3ef48fabbe5`).
- [NOTICE](../../../NOTICE) — new root file; carries the license's required
  plain-text `Required Notice:` line naming the licensor (D1).
- [LICENSING.md](../../../LICENSING.md) — repository licensing boundaries.
- [README.md](../../../README.md) — License section.
- [AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md](../../../AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md)
  §2–§3.6 — product tier and licensing-strategy sections this ADR aligns.
- [ADR-0014](./ADR-0014-local-first-privacy.md) — local-first privacy; no
  phone-home license checks (D6).
- PolyForm Noncommercial 1.0.0 upstream text:
  <https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Noncommercial-1.0.0.md>
  and <https://polyformproject.org/licenses/noncommercial/1.0.0>.
- SPDX License List entry for `PolyForm-Noncommercial-1.0.0`:
  <https://spdx.org/licenses/PolyForm-Noncommercial-1.0.0.html> — listed on
  SPDX; not OSI-approved (SPDX listing and OSI approval are different
  things).
- PolyForm Shield 1.0.0 (current, being replaced):
  <https://polyformproject.org/licenses/shield/1.0.0>.
- Anthropic legal & compliance page (read 2026-09-25):
  <https://code.claude.com/docs/en/legal-and-compliance>.
- U.S. Copyright Office, *Copyright and Artificial Intelligence, Part 2:
  Copyrightability* (January 2025) — cited as a legal-review risk, not a
  legal conclusion.
- `planning/tasks/queue.yaml` — TASK-0153 (this task).
