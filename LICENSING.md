# Runlith licensing

Runlith is **source-available**, not OSI Open Source, and must never be
described as "open source." The repository's default license is
[PolyForm Noncommercial 1.0.0](LICENSE). That license text governs; this page
explains scope and product direction and does not add restrictions to or
replace its grants.

This page is product/documentation guidance, not legal advice. It records a
Proposed decision — see
[ADR-0038](docs/architecture/adr/ADR-0038-product-licensing-noncommercial.md)
(status: Proposed, pending owner ratification after independent review).
Nothing here is Accepted policy until that ADR is ratified.

## Current scope

Unless a file or component explicitly states otherwise, Runlith-owned code and
documentation in this repository are offered under PolyForm Noncommercial
1.0.0. Third-party code, dependencies and assets retain their respective
licenses and notices; the root license does not relicense them.

## Free use is noncommercial use only

PolyForm Noncommercial grants free use only for permitted purposes, exactly as
its text defines them:

- any noncommercial purpose;
- personal use for research, experiment, and testing for the benefit of public knowledge,
  personal study, private entertainment, hobby projects, amateur pursuits or
  religious observance, without any anticipated commercial application;
- use by a charitable organization, educational institution, public research
  organization, public safety or health organization, environmental
  protection organization, or government institution — regardless of the
  source of funding or obligations resulting from that funding. This means
  government and education use is free even when publicly funded or under
  grant obligations.

Noncommercial users get the full product: every software feature — including
team/organizational/governance features — is available to all noncommercial
users, including noncommercial organizations; tiers no longer gate features
for noncommercial use. What is paid is (a) the commercial-use license itself
for commercial users, and (b) separately contracted services such as support,
SLAs, rollout assistance and hosted/cloud add-ons, which are not software
features granted by the license. Read the license text in [LICENSE](LICENSE)
for the exact boundaries; this page only summarizes it.

## Commercial use requires a paid license

Any commercial use of Runlith and all of its features — by an individual
commercial user such as a freelancer, or by a company of any size, for
internal use or otherwise — requires a separately executed commercial license
from the licensor. PolyForm Noncommercial does not grant commercial rights;
using the software for a commercial purpose without that separate license is
not a permitted purpose under [LICENSE](LICENSE).

This repository does not yet contain an executed or counsel-approved
commercial license or EULA. This page is not one, and no price, term or legal
entity is fixed by it. To ask about a commercial license, contact the
licensor (Artem Hrechuk, see [NOTICE](NOTICE)); no contact address is
established yet — this is a placeholder pending the owner's decision on a
contact channel, not an invented one.

## No enforcement in the product

The current license grant does not by itself enforce paid feature
entitlements or a per-seat subscription, and no license-enforcement or
phone-home mechanism exists in this codebase. Any future entitlement check
must stay consistent with the product's local-first privacy commitment
(see `docs/architecture/adr/ADR-0014-local-first-privacy.md`) and would need
its own accepted ADR before implementation.

## Commercial products and OEM rights

Pro Local, Pro Subscription, Teams and Enterprise are the product direction
defined in the [monetization strategy](AI_ENGINEERING_OS_MONETIZATION_AND_PLUGIN_ECOSYSTEM.md).
Under the license direction recorded here, those tiers become the commercial
licensing path for any commercial user, not a feature-gating mechanism among
noncommercial users. Their purchase terms and any additional commercially
licensed components must be explicitly identified when offered. Buying a
commercial license does not automatically grant OEM, white-label, hosted-
resale or rebranding rights beyond what is explicitly purchased; those remain
a separate negotiated agreement question.

The intended Pro Local offer is perpetual use of the last eligible version
after its update period ends; that entitlement must be specified in the
actual purchase agreement, which does not yet exist.

## Open-source-program eligibility

Eligibility for AI-vendor open-source maintainer programs (for example
Claude for Open Source or Codex for Open Source) is decided by each program,
not by this document. PolyForm Noncommercial is not an OSI-approved license;
program pages reviewed for ADR-0038 describe adoption-based criteria, and a
secondary source claims an OSI-license requirement (not independently
verified). A noncommercial-only core is, at best, unlikely to qualify as an
"open-source project" for such a program — a judgement, not a program ruling.
The owner's recorded direction is to pursue that eligibility only through
separately scoped, genuinely independent components released under an OSI
license — the Apache-2.0 direction below for a future Plugin SDK, public
schemas and examples — each of which must earn its own adoption. The Runlith
core itself is not expected to become OSI open source under this decision,
and must not be marketed as "open source" regardless of any component's
eligibility.

## Plugin SDK and Apache-2.0

The selected direction for a future independently identified Plugin SDK,
associated public schemas and examples is
[Apache License 2.0](https://www.apache.org/licenses/LICENSE-2.0).
No component is designated Apache-2.0 by this page. There is currently no
separately licensed Plugin SDK in this workspace; internal IPC, execution and
planner contracts remain under the repository default license unless explicitly
marked otherwise.

An eventual SDK must carry its own license and explicit scope. Apache-2.0 for
an SDK will not provide an alternative license for the Runlith core, and does
not by itself make the core eligible for OSI-scoped programs. Supporting
independent paid plugins is product strategy, not a blanket exception to the
noncommercial-only core license. Plugin/core licensing boundaries and any
necessary additional permissions must be settled before opening the
ecosystem.

## Contributions, trademarks and release readiness

Before accepting substantial external contributions, establish contributor
terms sufficient for the intended noncommercial/commercial licensing model,
e.g. an appropriate CLA. A Developer Certificate of Origin (DCO) alone is not
an additional grant of commercial relicensing rights. No CLA is established
here.

Source-code availability does not imply permission to claim official Runlith
endorsement. A separate trademark policy remains to be established; this page
does not assert a trademark registration or invent a legal owner.

Before public distribution, obtain qualified legal review of copyright
ownership and contributor permissions, third-party notices, commercial
agreements, plugin/trademark boundaries, and the copyright status of any
AI-assisted authorship in this repository (see ADR-0038's migration impact
and open questions). Adding the standard license text is not evidence that
those release checks have been completed.

## Effective date

This LICENSE/NOTICE change takes effect only once it is merged into the
repository default branch, and merge happens only together with or after
owner ratification of ADR-0038. Until then, this page and NOTICE describe a
Proposed decision, not the license terms already in effect.

## License source

`LICENSE` is an unmodified copy of
[PolyForm Noncommercial 1.0.0 from the PolyForm project](https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Noncommercial-1.0.0.md).
Examples of notices inside that standard text are examples, not Runlith's
copyright notices. The root [NOTICE](NOTICE) file carries the plain-text line
beginning `Required Notice:` that the license's Notices clause requires
recipients to also receive; NOTICE must be redistributed together with the
software for that reason. The licensor is Artem Hrechuk, as stated in
NOTICE.
