# ADR-0038 — Product licensing: Business Source License 1.1 with noncommercial Additional Use Grant

Status: **Proposed** (TASK-0153; awaiting owner ratification after independent review)
Gate: cross-gate (governance)

ADR-0037 is reserved for TASK-0152 (confined Claude provider authentication),
being drafted on branch `claude/adr-0037-provider-confinement-7qvn2z`; it is
unrelated to this ADR's licensing decision.

> Changes the repository default license from PolyForm Shield 1.0.0 to
> Business Source License 1.1 (BSL), with an Additional Use Grant that makes
> production use free only for (a) individuals for personal, noncommercial
> purposes without anticipated commercial application, and (b) charitable
> organizations and privately operated, non-governmental, not-for-profit
> educational or research institutions, for their noncommercial purposes.
> Every other production use — including by any business or government body,
> and by freelancers/contractors regardless of client — requires a paid
> commercial license. Each version converts to Apache License 2.0 four years
> after its first public distribution under this License. This supersedes
> this same ADR's earlier Proposed draft, which selected PolyForm
> Noncommercial 1.0.0; that draft never merged and is superseded by this
> text. This is not legal advice; it is a product/governance decision pending
> owner ratification, recorded here so the review and ratification have a
> fixed text to evaluate.

## Context

The repository's current default license, verified byte-identical to
upstream, is [PolyForm Shield 1.0.0](../../../LICENSE) as distributed on the
repository's default branch. Shield permits broad use — including ordinary
internal business use — subject only to a noncompete restriction on
providing a *competing* product. It does not require payment for internal
commercial use that does not compete with the licensor's own offering. That
does not meet the owner's goal: the owner wants **all** commercial and
government-institution use of the product, by any individual, company or
government body of any size, to require a paid commercial license, while
free production use is limited to individuals' personal noncommercial use
and to charitable/educational/research noncommercial organizations.

This ADR's earlier Proposed draft (TASK-0153, same branch history) selected
**PolyForm Noncommercial 1.0.0** to replace Shield. That draft also did not
meet the owner's goal: PolyForm Noncommercial's Noncommercial Organizations
clause grants free use to "any ... government institution ... regardless of
the source of funding," verbatim and unconditionally. The owner explicitly
wants government-institution use, including public schools and public
universities, to require a paid commercial license — the opposite of what a
verbatim PolyForm Noncommercial adoption grants. Narrowing that clause would
require a custom, modified license text, which that draft's own Open
questions flagged as needing qualified legal counsel and carrying
modification/renaming uncertainty under PolyForm's own terms. The owner
changed the license decision (2026-09-26) to Business Source License 1.1
(BSL) instead: BSL's free-use scope is defined entirely by a Licensor-authored
Additional Use Grant, so the owner's exact intended scope — free for
qualifying individuals and noncommercial organizations only, paid for
everyone else including government — can be expressed directly in Parameters
text, using the BSL license body verbatim and unmodified. This ADR reworks
the PolyForm Noncommercial draft into a BSL 1.1 draft; the PolyForm
Noncommercial text is no longer proposed.

Today's monetization strategy (`AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`
§3) also gates *features* by tier (Community vs. Pro/Teams/Enterprise)
independently of the base license. Under this decision, tiers stop gating
**software features** for everyone permitted to use the product for free:
every software feature in the licensed product — including
team/organizational/governance features — is available to anyone permitted
by the Additional Use Grant to make production use of it, and to anyone
making non-production use of it under the License's base grant (see D3a).
What the paid tiers sell is (a) the commercial production-use license
itself, and (b) separately contracted services (support, SLAs, rollout
assistance) and hosted/cloud add-ons, neither of which are software features
granted by the license.

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
BSL 1.1's SPDX identifier is `BUSL-1.1`; BSL is **not** an OSI-approved
license (SPDX listing and OSI approval are different things — see
References), so the Runlith core is, at best, unlikely to qualify as an
"open-source project" for such a program while under BSL — a judgement, not
a verified program ruling. Each individual released version does become
Apache-2.0 (OSI-approved) at its own Change Date, four years after that
version's first public distribution (D4), which is a structural difference
from the PolyForm Noncommercial draft, which promised no such conversion.

## Decision

**D1 — Default license and Parameters.** The repository default license
becomes [Business Source License 1.1](https://mariadb.com/bsl11/) (BSL,
SPDX id `BUSL-1.1`), replacing PolyForm Shield 1.0.0. `LICENSE` reproduces
the BSL 1.1 license body verbatim (byte-identical from the "Terms" heading
through the Covenants of Licensor, verified by `sha256sum`, see Verification
below) with only the license's own permitted Parameters filled in, laid out
in the same Parameters-block column format used by other real-world BSL
adopters (e.g. Terraform's `LICENSE`, see References):

- **Licensor:** Artem Hrechuk (<https://github.com/fallowlone>) — a natural
  person, not a legal entity.
- **Licensed Work:** Runlith. The Licensed Work is (c) 2026 Artem Hrechuk.
- **Additional Use Grant:** see D2 (full text in LICENSE and Consequences).
- **Change Date:** four years from the date on which a version of the
  Licensed Work is first publicly distributed under this License (applies
  per version — see D4).
- **Change License:** Apache License, Version 2.0.

The root `NOTICE` file (new, plain text) carries the plain-text copyright
and licensing-summary lines described in Migration impact; it is not a BSL
required-notice mechanism (BSL has none) but standard repository practice.
Per Covenant 4 of the license text, the Licensor may not modify the License
body in any other way; every word of `LICENSE` outside the Parameters block,
the contact line and the Notice paragraph is the unmodified BSL 1.1 text.

**D2 — Free production use, exactly per the Additional Use Grant.** The
Additional Use Grant, per Covenant 2 of the License, may only *add* rights on
top of the base grant — it cannot narrow the base grant's copy/modify/
non-production rights (see D3a). As drafted, production use is free without
a commercial license only for:

- (a) an individual using the Licensed Work for their own personal,
  noncommercial purposes, without any anticipated commercial application; or
- (b) a charitable organization, or a privately operated, non-governmental,
  not-for-profit educational or research institution, using the Licensed
  Work for its noncommercial purposes.

This resolves the government-institution problem that blocked the earlier
PolyForm Noncommercial draft: government bodies are not covered by (a) or
(b), so government production use is not free under this Additional Use
Grant (see D3).

**D3 — Commercial license required for everyone outside the Additional Use
Grant; government institutions, including public schools and public
universities, are not exempt.** Production use requires a paid commercial
license from the Licensor whenever it is not covered by D2, specifically:

- any business or other for-profit entity of any size, for any production
  use, including ordinary internal use;
- any individual acting in the course of commercial activity, including
  work performed as a freelancer or contractor for any client — **including
  a client that is itself a D2(b) charitable/educational/research
  institution.** A contractor's or freelancer's own production use is
  commercial regardless of who the client is or what the client's use would
  qualify as if the client did the work itself;
- any government, government body or agency, or any institution owned or
  operated by a government. **This explicitly includes public schools and
  public universities:** the owner's decision is that a publicly owned or
  operated educational institution is a government institution for this
  License, not a D2(b) "privately operated, non-governmental" institution,
  regardless of whether its subject matter is educational or research in
  nature. Only a *privately* operated, non-governmental not-for-profit
  educational or research institution qualifies under D2(b).

No commercial license or EULA is executed as of this ADR.

**D3a — Non-production use (evaluation, testing, development) is free for
everyone, by the base License grant, and this ADR cannot remove that.** BSL's
first paragraph of Terms grants "the right to copy, modify, create
derivative works, redistribute, and make non-production use of the Licensed
Work" to everyone, unconditionally; the Additional Use Grant in D2 only adds
production-use rights on top of that base grant (Covenant 2) and the
Licensor covenants not to modify the License body in any other way
(Covenant 4). Consequently, **evaluating, testing, or developing against
Runlith is free for any individual, business or government, with no
commercial license required, and no Additional Use Grant wording can take
that away.** This ADR was drafted after that fact was verified
(orchestrator-verified) and the owner was told of it; it is stated plainly
here and must be stated plainly in `LICENSING.md`/README, not left implicit.

**Licensor's interpretation of "production use" (interpretation, not
license text).** BSL does not itself define "production use." The following
is the Licensor's own interpretation, recorded for guidance and not a term
of the License itself — see also Open questions on whether to publish a
fuller FAQ:

- **Production use** (requires D2 coverage or a commercial license): using
  Runlith to perform real work — for example, using it to develop, review,
  ship or operate software that you, your employer, or a client relies on,
  uses, or distributes.
- **Non-production use** (free for everyone under the base grant): evaluating
  Runlith, testing it, or developing/debugging *Runlith itself* (e.g.
  running it against a scratch/throwaway project purely to assess or
  improve the tool, not to produce work product you keep or ship).

**D4 — Source-available now; each version becomes Apache-2.0 at its own
Change Date.** The core must always be described as "source-available,"
never as "open source," "OSS," or similar, in documentation, marketing or
code comments while under BSL — BSL (`BUSL-1.1`) is not an OSI-approved
license. Unlike the PolyForm Noncommercial draft, BSL is not permanent: per
the License's own Terms, each version's rights under the Additional Use
Grant automatically end and convert to the Change License (Apache License,
Version 2.0) on that version's Change Date — four years after that specific
version was first publicly distributed under this License. This License
"applies separately for each version," so each release's Change Date is
tracked independently (see Migration impact). The repository is currently
private and no version has been publicly distributed under this License, so
**no Change Date clock has started for any version** as of this ADR.

**D5 — OSS-program strategy: converted versions are Apache-2.0 but lag four
years; the primary path remains separately scoped Apache-2.0 components.**
The owner's interest in AI-vendor open-source maintainer-program eligibility
is pursued primarily through genuinely independent, separately scoped
components released under an OSI-approved license — the already-selected
Apache-2.0 direction for a future Plugin SDK, public schemas and examples.
Each such component must earn its own adoption independently; none of it
changes the Runlith core's BSL status. Secondarily, once versions begin
converting to Apache-2.0 at their Change Dates, those specific converted
versions become genuinely OSI-licensed, but always four years behind the
current release, so a program requiring the *current* version to be
OSI-licensed would not be satisfied by an old converted version. Program
eligibility is decided by each program, not asserted here — this is
strategy and judgement, not a guarantee of qualification either now or after
any conversion.

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
free/commercial licensing model, e.g. an appropriate CLA; a DCO alone is not
an additional grant of commercial relicensing rights. No CLA is established
by this ADR; drafting one is a follow-up (see Migration impact).

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
copy's Shield grant is unaffected by this decision; the same holds for the
earlier, never-merged PolyForm Noncommercial draft of this same ADR, which
was never distributed as an in-effect license (it existed only as a Proposed
file on this task branch, pending ratification, and is now superseded by
this text before it ever took effect). The license change applies going
forward to the repository default as of ratification.

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

**D10 — All software features available to everyone permitted to use them;
paid means the commercial license plus services.** Every software feature in
the licensed product — including team/organizational/governance features —
is available at no additional feature-gate cost to anyone whose use is
permitted for free under D2/D3a, and equally available (not a reduced
feature set) to anyone who has purchased the required commercial license
under D3. Tiers (Community/Pro Local/Pro Subscription/Teams/Enterprise) stop
gating software functionality; what is paid is (a) the commercial
production-use license itself, required by D3, and (b) separately contracted
services such as support, SLAs, rollout assistance and hosted/cloud add-ons,
which are not software features granted or withheld by the license.

## Alternatives considered

- **Keep PolyForm Shield 1.0.0.** Rejected: Shield does not require payment
  for ordinary internal business use that does not compete with the
  licensor's product; it fails the owner's "all commercial and government
  use pays, except qualifying individuals/nonprofits" goal.
- **PolyForm Noncommercial 1.0.0 (this ADR's previous Proposed draft).**
  Rejected: its Noncommercial Organizations clause grants free,
  unconditional use to government institutions "regardless of the source of
  funding," which the owner rejected; it also has no timed conversion to an
  open license for old versions, unlike BSL. See Context.
- **PolyForm Small Business 1.0.0.** Free for organizations under 100 people
  and under $1,000,000 (2019 USD) revenue. Rejected: the owner wants all
  commercial use, including small commercial users, to require payment; a
  revenue/headcount carve-out contradicts that.
- **FSL-1.1-ALv2 (Fair Source, converts to Apache-2.0 after 2 years).**
  Competing-use ban only; ordinary internal business use is free. Rejected:
  same free-internal-use gap as Shield for the owner's goal.
- **Elastic License 2.0.** Prohibits offering the software as a hosted
  service and provides a license-key/feature-restriction clause, but does
  not require payment for ordinary internal business use. Rejected for the
  same reason as Shield and FSL.
- **AGPL-3.0 core + commercial dual license.** OSI-approved, and would make
  the core eligible for OSS maintainer programs immediately. Rejected: AGPL
  cannot oblige ordinary internal business use to pay (a company may run
  AGPL software internally, without distributing it, and owe nothing) — it
  fails the "all commercial/government use pays" requirement at its
  foundation, not just at the margins.
- **Apache-2.0/MIT open core + proprietary `ee/` directory (open-core
  model).** The open core itself would be free for commercial use too
  (Apache-2.0 grants that); only the separately walled-off proprietary
  features/services would be paid. Rejected: contradicts "commercial and
  government use of the product and **all its features** requires a paid
  commercial license" — an open-core split makes the core itself free for
  commercial use, which is exactly what the owner does not want.
- **A custom, modified PolyForm Noncommercial variant carving out
  government institutions.** Considered as an alternative to switching base
  licenses entirely. Rejected for this ADR: PolyForm's own terms make
  modifying and still calling the result "PolyForm Noncommercial" legally
  uncertain (naming/derivative-license conventions), and drafting a
  materially modified license text needs qualified legal counsel before it
  can be proposed as a fixed text — a bigger, slower change than adopting
  BSL, whose Additional Use Grant mechanism is purpose-built for exactly
  this kind of Licensor-defined carve-out without modifying the License body
  itself (Covenant 4 forbids modifying BSL; Covenant 2 permits only an
  additive Additional Use Grant).

## Consequences

- Free production use is narrower than the previous PolyForm Noncommercial
  draft in one respect (no unconditional government carve-out) and about the
  same in another (personal noncommercial use and qualifying private
  nonprofits remain free); it now excludes all government use and all
  freelancer/contractor use regardless of client, matching the owner's
  stated goal directly instead of requiring an unratified license
  modification.
- **Non-production use (evaluation, testing, development) is free for
  everyone, including every business and government body, and cannot be
  restricted by the Additional Use Grant** — this is a structural property
  of BSL's base grant (Covenants 2 and 4), not a discretionary Licensor
  choice, and must be stated plainly wherever this ADR, `LICENSING.md` or
  `README.md` describe the license (see D3a).
- **Each version converts to Apache-2.0 four years after its own first
  public distribution.** This means an old version of Runlith eventually
  becomes free for business and government production use too, once its
  Change Date passes — a real trade-off the owner accepted in exchange for
  BSL's Licensor-defined Additional Use Grant mechanism and the resulting
  OSS-program story (D5). This did not exist under the previous PolyForm
  Noncommercial draft, which promised no conversion at all.
- **Per-version Change Date bookkeeping becomes a real, ongoing obligation.**
  Because the License applies separately per version and each version's
  Change Date is measured from *that version's* first public distribution,
  the project must record, for every publicly released version, the date it
  was first publicly distributed under this License (see Migration impact).
  No version has started that clock yet (repository is private).
- **The Additional Use Grant text is owner-authored legal text, not
  boilerplate**, unlike adopting a pre-existing license verbatim (as the
  PolyForm Noncommercial draft did). It is the single piece of custom legal
  language in this decision and should be the top priority for qualified
  legal review before any public distribution (see Migration impact and Open
  questions).
- Eligibility for OSI-scoped/open-source-only program benefits (e.g.
  AI-vendor open-source maintainer programs) is decided by each program. BSL
  (`BUSL-1.1`) is not an OSI-approved license, so the *current* release is,
  at best, unlikely to qualify while under BSL; only separately scoped
  Apache-2.0 components (Plugin SDK, schemas, examples), or a version old
  enough to have already converted to Apache-2.0, can pursue that path (D5).
- The monetization doc's tier semantics change: Community stops being a
  feature-limited free trial and becomes the free-production-use tier
  exactly matching the Additional Use Grant (individuals' personal
  noncommercial use; qualifying private nonprofits), with non-production use
  free for everyone regardless of tier; Pro/Teams/Enterprise become the
  commercial production-use licensing vehicle, not merely a feature unlock.
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

This task changes only documentation/governance files: `LICENSE`, `NOTICE`,
`LICENSING.md`, `README.md`'s License section, targeted sections of
`AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`, this ADR (renamed
from its earlier PolyForm-Noncommercial-titled filename, see D8/Context, to
`ADR-0038-product-licensing-bsl.md`), the ADR index, and
`planning/tasks/queue.yaml`. No code, `product/version.toml`, or Cargo
manifest changes are made here. Follow-up work required before this decision
is fully realized, none of it authorized to happen automatically by this
ADR:

- Add `license = "BUSL-1.1"` — this identifier exists on the SPDX License
  List (SPDX is not the same as OSI approval; the license is not
  OSI-approved) — to workspace Cargo manifests; no workspace crate currently
  declares a `license` field, verified 2026-09-26.
- Draft an actual commercial license/EULA with qualified legal counsel; none
  exists today, and this ADR does not create one.
- Draft and adopt a CLA before accepting the first external contribution
  (D7); none exists today.
- Establish a trademark policy for the "Runlith" name; none exists today,
  and this ADR does not assert a trademark registration.
- Obtain qualified legal review before public distribution, prioritizing the
  owner-authored Additional Use Grant text (D2, the one piece of custom
  legal language in this decision), plus the final license text as a whole,
  the commercial-license/EULA draft, and the copyright status of AI-assisted
  authorship in this repository. The U.S. Copyright Office's *Copyright and
  Artificial Intelligence* report, Part 2 (January 2025), states that
  material generated solely by AI is not copyrightable while human-authored
  selection, arrangement or modification can be; with 66 of 181 commits in
  this repository carrying an AI co-author trailer (verified 2026-09-26),
  this repository's copyright ownership over AI-assisted portions is a
  genuine legal-review question, stated here as a risk requiring legal
  review, not resolved as a legal conclusion.
- Record, for each publicly released version going forward, the date that
  version was first publicly distributed under this License, so that
  version's Change Date (four years later) can be tracked; no version has
  started that clock yet because the repository is currently private.
- Check `PROJECT_CHARTER.md` and
  `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` for license-related
  wording that may need to follow this decision once ratified; both files
  are outside this task's allowed file list and are not edited here.
- Reconcile sections of `AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md`
  that still reflect the pre-ADR-0038 (or the pre-BSL-rework) model and were
  not in this task's allowed edit scope, including its appendix and §5
  "Community rights concept" (~line 1205, lists "evaluation" as a potential
  free/community right — now redundant, since D3a makes non-production use
  free for everyone regardless of tier), §6 "Commercial use of the product
  should remain allowed" (~line 1228), §7/§12/§33 (PolyForm Shield
  recommendations/ranking, ~lines 1251, 1322, 1795), and §34 "Critical
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

- **Additional Use Grant wording — needs legal review.** D2's text is
  owner-authored (via this task's drafting process), not a pre-existing
  license clause reused verbatim; it is the piece of this decision most in
  need of qualified legal review before public distribution (see Migration
  impact).
- **Whether to publish a Licensor FAQ on "production use."** D3a records the
  Licensor's own interpretation of production vs. non-production use as
  interpretation, not License text. Whether to publish a fuller,
  citable FAQ (in the style of the MariaDB BSL FAQ or the HashiCorp/IBM
  Terraform BSL FAQ referenced below) to reduce ambiguity for users is open
  and not decided by this ADR.
- **Exact commercial terms.** Price, contract form, and whether Pro
  Local/Subscription/Teams/Enterprise purchase terms are drafted as one
  commercial license or several, remain undecided; §3 of the monetization
  doc keeps its prices as hypotheses (unchanged by this task).
- **Whether a government-owned but independently chartered institution
  counts as "government" for D3 — the owner's decision is recorded, but the
  boundary for edge cases is not fully drafted.** The owner has decided that
  public schools and public universities are government institutions for
  this License (D3), not D2(b) private nonprofits, settling that specific
  case. Harder edge cases — e.g. a government-chartered but operationally
  independent research foundation, or a public-private partnership — are not
  resolved by this ADR and remain open for a future FAQ or Licensor
  determination if they arise.

## References

- [LICENSE](../../../LICENSE) — Business Source License 1.1, license body
  (from "Terms" onward) verbatim (sha256 of that tail:
  `418989f0e58e9be45720f11d0bba6bf7ff7b61da68753918b784754dc22b011b`), with
  the license's own permitted Parameters filled in.
- [NOTICE](../../../NOTICE) — new root file; plain-text copyright and
  licensing-summary lines (D1).
- [LICENSING.md](../../../LICENSING.md) — repository licensing boundaries.
- [README.md](../../../README.md) — License section.
- [AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md](../../../AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md)
  §2.1, §2.2, §3–§3.5 — product tier and licensing-strategy sections this ADR
  aligns.
- [ADR-0014](./ADR-0014-local-first-privacy.md) — local-first privacy; no
  phone-home license checks (D6).
- Business Source License 1.1, SPDX License List entry for `BUSL-1.1`:
  <https://spdx.org/licenses/BUSL-1.1.html> — listed on SPDX; not
  OSI-approved (SPDX listing and OSI approval are different things). Text
  fetched from
  <https://raw.githubusercontent.com/spdx/license-list-data/main/text/BUSL-1.1.txt>.
- MariaDB, "Adopting and Using the Business Source License FAQ":
  <https://mariadb.com/bsl-faq-adopting/> — **cited for background,
  not independently fetched/verified by this task**; treat as a pointer, not
  a verified source.
- Terraform (IBM/HashiCorp) `LICENSE` — used only as a real-world reference
  for the Parameters-block column layout convention, not as legal precedent
  for this ADR's terms.
- PolyForm Shield 1.0.0 (current production default, being replaced):
  <https://polyformproject.org/licenses/shield/1.0.0>.
- PolyForm Noncommercial 1.0.0 (this ADR's previous, now-superseded Proposed
  draft, never merged):
  <https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Noncommercial-1.0.0.md>.
- Anthropic legal & compliance page (read 2026-09-25):
  <https://code.claude.com/docs/en/legal-and-compliance>.
- U.S. Copyright Office, *Copyright and Artificial Intelligence, Part 2:
  Copyrightability* (January 2025) — cited as a legal-review risk, not a
  legal conclusion.
- `planning/tasks/queue.yaml` — TASK-0153 (this task).
