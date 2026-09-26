# Runlith licensing

Runlith is **source-available**, not OSI Open Source, and must never be
described as "open source." The repository's default license is the
[Business Source License 1.1](LICENSE) (BSL, SPDX id `BUSL-1.1`). That
license text governs; this page explains scope and product direction and
does not add restrictions to or replace its grants.

This page is product/documentation guidance, not legal advice. It records
the decision in
[ADR-0038](docs/architecture/adr/ADR-0038-product-licensing-bsl.md)
(status: Accepted, owner-ratified 2026-09-26). The license text in `LICENSE`
governs once this change is merged into the repository default branch.

## Current scope

Unless a file or component explicitly states otherwise, Runlith-owned code and
documentation in this repository are offered under Business Source License
1.1. Third-party code, dependencies and assets retain their respective
licenses and notices; the root license does not relicense them.

## Non-production use is free for everyone

BSL's base grant — "the right to copy, modify, create derivative works,
redistribute, and make non-production use of the Licensed Work" — is free
for **everyone**, including every business and government body, and cannot
be removed: the Additional Use Grant below can only add rights on top of it,
never take rights away (see [LICENSE](LICENSE), Covenants of Licensor 2 and
4). In practical terms: evaluating Runlith, testing it, or developing or
debugging Runlith itself is free for anyone, with no commercial license
required. See [ADR-0038](docs/architecture/adr/ADR-0038-product-licensing-bsl.md)
D3a for the reasoning and for the Licensor's own (non-binding) interpretation
of "production use" versus "non-production use."

## Free production use is limited to the Additional Use Grant

Production use — using Runlith to do real work, such as developing, shipping
or operating software that you, your employer or a client relies on — is
free without a commercial license **only** if you are (this framing of
"production use" is the Licensor's interpretation, not license text; see
ADR-0038 D3a):

- **(a)** an individual using Runlith for your own personal, noncommercial
  purposes, without any anticipated commercial application; or
- **(b)** a charitable organization, or a privately operated,
  non-governmental, not-for-profit educational or research institution,
  using Runlith for its noncommercial purposes.

This Additional Use Grant does **not** extend production use to: any
business or other for-profit entity of any size; an individual acting in the
course of commercial activity, including work performed as a freelancer or
contractor for any client (including a client described in (b) above — the
contractor's own use is commercial regardless of the client); or any
government, government body or agency, or any institution owned or operated
by a government, **including public schools and public universities**. The
exact wording is reproduced in [LICENSE](LICENSE)'s Parameters block; this
page only summarizes it.

Everyone permitted to use Runlith under the License gets the full product:
every software feature — including team/organizational/governance
features — is available to anyone permitted to use Runlith, whether for
free under non-production use or the Additional Use Grant, or under a paid
commercial license; tiers do not gate functionality. What is paid is (a) the
commercial production-use license itself, required for any production use
outside the Additional Use Grant, and (b) separately contracted services
such as support, SLAs, rollout assistance and hosted/cloud add-ons, which
are not software features granted by the license.

## Commercial license required for production use outside the Additional Use Grant

Any production use of Runlith not covered by the Additional Use Grant above
— by an individual commercial user such as a freelancer, by a company of any
size for internal use or otherwise, or by any government body or
government-operated institution (including a public school or public
university) — requires a separately executed commercial license from the
Licensor. BSL does not grant commercial production-use rights beyond the
Additional Use Grant; using Runlith for production use outside that grant
without a separate commercial license is not permitted.

This repository does not yet contain an executed or counsel-approved
commercial license or EULA. This page is not one, and no price, term or legal
entity is fixed by it. To ask about a commercial license, contact the
Licensor (Artem Hrechuk, see [NOTICE](NOTICE)) via
<https://github.com/fallowlone>; no other contact channel is established.

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
Under the license direction recorded here, those tiers are the commercial
production-use licensing path for anyone outside the Additional Use Grant,
not a feature-gating mechanism. Their purchase terms and any additional
commercially licensed components must be explicitly identified when
offered. Buying a commercial license does not automatically grant OEM,
white-label, hosted-resale or rebranding rights beyond what is explicitly
purchased; those remain a separate negotiated agreement question.

The intended Pro Local offer is perpetual use of the last eligible version
after its update period ends; that entitlement must be specified in the
actual purchase agreement, which does not yet exist.

## Each version converts to Apache-2.0 after four years

Unlike a permanent source-available license, BSL converts: per the license's
own Terms, each version's Additional Use Grant terminates and that version's
rights convert to the Change License — Apache License, Version 2.0 — on its
own Change Date, four years after that specific version was first publicly
distributed under this License. The License "applies separately for each
version," so each release's Change Date must be tracked independently. The
repository is currently private, so **no Change Date clock has started for
any version yet.** This means an older version eventually becomes free for
everyone, including businesses and governments, once its own Change Date
passes; the currently shipping version does not become free earlier than
its own four-year clock. See
[ADR-0038](docs/architecture/adr/ADR-0038-product-licensing-bsl.md) D4.

## Open-source-program eligibility

Eligibility for AI-vendor open-source maintainer programs (for example
Claude for Open Source or Codex for Open Source) is decided by each program,
not by this document. BSL (`BUSL-1.1`) is not an OSI-approved license;
program pages reviewed for ADR-0038 describe adoption-based criteria, and a
secondary source claims an OSI-license requirement (not independently
verified). The Runlith core is, at best, unlikely to qualify as an
"open-source project" for such a program while under BSL — a judgement, not
a program ruling. Once a given version's Change Date passes, that specific
version does become genuinely Apache-2.0 (OSI-approved), but always four
years behind the current release. The owner's recorded direction is to
pursue that eligibility primarily through separately scoped, genuinely
independent components released under an OSI license — the Apache-2.0
direction below for a future Plugin SDK, public schemas and examples — each
of which must earn its own adoption. The Runlith core itself must not be
marketed as "open source" regardless of any component's or older version's
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
license's Additional Use Grant. Plugin/core licensing boundaries and any
necessary additional permissions must be settled before opening the
ecosystem.

## Contributions, trademarks and release readiness

Before accepting substantial external contributions, establish contributor
terms sufficient for the intended free/commercial licensing model, e.g. an
appropriate CLA. A Developer Certificate of Origin (DCO) alone is not an
additional grant of commercial relicensing rights. No CLA is established
here.

Source-code availability does not imply permission to claim official Runlith
endorsement. A separate trademark policy remains to be established; this page
does not assert a trademark registration or invent a legal owner.

Before public distribution, obtain qualified legal review of the Additional
Use Grant wording (the one piece of owner-authored legal text in this
decision), copyright ownership and contributor permissions, third-party
notices, commercial agreements, plugin/trademark boundaries, and the
copyright status of any AI-assisted authorship in this repository (see
ADR-0038's migration impact and open questions). Adding the standard license
text is not evidence that those release checks have been completed.

## Effective date

This LICENSE/NOTICE change takes effect only once it is merged into the
repository default branch, and merge happens only together with or after
owner ratification of ADR-0038 (ratified 2026-09-26; merge requires separate
owner authorization). Until merge, this page and NOTICE describe an adopted
decision, not the license terms already in effect on the default branch.

## License source

`LICENSE`'s license body (from the "Terms" heading through the Covenants of
Licensor) is an unmodified copy of the Business Source License 1.1 text, per
[SPDX license-list-data](https://raw.githubusercontent.com/spdx/license-list-data/main/text/BUSL-1.1.txt),
with only the license's own permitted Parameters (Licensor, Licensed Work,
Additional Use Grant, Change Date, Change License) filled in, laid out using
the same Parameters-block column format used by other real-world BSL
adopters (e.g. Terraform's `LICENSE`). The root [NOTICE](NOTICE) file carries
plain-text copyright and licensing-summary lines; BSL itself has no
required-notice mechanism like PolyForm's. The licensor is Artem Hrechuk, as
stated in NOTICE.
