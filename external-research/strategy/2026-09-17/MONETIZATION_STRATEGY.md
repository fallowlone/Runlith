# Runlith: monetization strategy

Research date: 17 September 2026. All proposed Runlith prices, margins, conversion gates and customer counts are hypotheses or arithmetic scenarios. None is validated willingness to pay, a forecast, or an approved commercial offer. Public competitor prices are dated observations. [Evidence register](EVIDENCE_REGISTER.md).

**Recommendation:** test a paid local application subscription with customer-supplied agent/model access. Sell recurring relief from supervision, recovery and acceptance work. Add an optional team coordination service only after teams pay for a shared workflow. Do not begin by subsidizing inference or building an enterprise sales organization.

## 1. The strongest reason to pay

**“I already have good coding agents. Runlith lets me repeatedly delegate useful maintenance work and return to a reviewable result, with clear evidence and recovery, instead of reconstructing everything myself.”**

This is stronger than “use multiple models,” “manage agents,” or “store memory.” Those are capabilities; the customer buys reduced effort and dependable outcomes.

The incremental value must be measured against a capable existing harness plus Git/PR review and CI. If Runlith merely wraps the same work in another dashboard, the rational price is close to zero.

A payment decision can be expressed as:

**Value of supervision and rework avoided + value of additional accepted work − additional compute − adoption/administration cost > subscription price.**

For illustration, 20 minutes saved each week at an assumed $75/hour is about $108/month using 4.33 weeks. A $29 subscription plus $10 of additional compute leaves about $69 of modeled value before setup and administration. This is opportunity-cost arithmetic, not a promise of cash savings or evidence that such time savings occur.

## 2. Who pays, for what, and why value recurs

| Customer | Recurring job | Potential paid value | Main barrier |
|---|---|---|---|
| Individual developer | Finish bounded changes without losing work context | Continuity, review organization, recurring procedures and recovery | Another subscription; existing tools may be sufficient |
| Professional engineer or tech lead | Delegate maintenance while remaining accountable | Less supervision, clear acceptance evidence, budget control and handoff | Must fit repository conventions and existing review tools |
| Small team | Share ownership and repeat proven procedures | Shared decision queues, access control, policy, evidence and responsibility | Team onboarding and coordination can add more work than they remove |
| Enterprise | Operate agents under organizational constraints | Deployment choice, identity lifecycle, audit export, policy management and support | Long sales cycles, procurement and costly support obligations |

Professional users are the strongest initial segment because they experience both the upside of agent output and the cost of being accountable for it. Hobby users can provide feedback, but may have low purchase urgency. Enterprises can pay more, but can also force a premature infrastructure roadmap.

## 3. Competitive price anchors

These are not like-for-like bundles. Some include inference credits, some sell an editor, and some charge separately for consumption. Monthly figures below mean monthly billing unless marked otherwise.

| Product | Public anchor observed | Implication |
|---|---|---|
| Cursor | Pro $20, Pro+ $60, Ultra $200/month | There is spending room around professional AI work, but substantial capability is already bundled. [Pricing](https://cursor.com/pricing) |
| Claude | Pro $20/month; other subscription/API modes | A Runlith fee may be additional to an existing model subscription. [Pricing](https://claude.com/pricing) |
| GitHub Copilot | Pro $10, Pro+ $39, Max $100/month | Distribution and low entry prices make generic assistance difficult to monetize separately. [Plans](https://github.com/features/copilot/plans) |
| Devin | Pro $20, Max $200/month; separate team structure | Delegation is no longer available only at a high enterprise-like entry price. [Pricing](https://devin.ai/pricing) |
| Zed | Pro $10/month, including $5 in model usage; external-agent/BYOK paths | Interoperability and BYOK alone command limited differentiation. [Pricing](https://zed.dev/pricing) |
| Linear | Basic $10 and Business $16/user/month, annually billed | A separate workflow layer must justify stacking onto an existing coordination bill. [Pricing](https://linear.app/pricing) |
| Raycast AI | September announcement: $10/$20/$50 monthly tiers with credits | Inference allowances change; unrestricted AI promises are economically fragile. [September announcement](https://www.raycast.com/blog/changing-how-raycast-ai-is-priced) |
| OpenHands | Free local and free individual cloud platform; compute separate; enterprise custom | Free infrastructure alternatives put pressure on an undifferentiated control-plane fee. [Pricing](https://www.openhands.dev/pricing) |

Do not infer willingness to pay for Runlith from these prices. They anchor the customer's alternatives and total bill, not Runlith's value.

## 4. Comparison of monetization models

All amounts in this table are proposed experiments, not competitor facts. Models overlap; they are strategic alternatives, not six simultaneous launch plans.

| Model | Target customer and test price | Value proposition | Advantages | Risks | Required capabilities |
|---|---|---|---|---|---|
| SaaS subscription | Individuals/teams; $29–59 per active human/month, compute separate | Managed coordination and availability | Recurring revenue; centralized operation | Weakens local-control differentiation; infrastructure and data obligations | Reliable hosted service, identity, billing, data controls and support |
| Paid local application | Professional individuals; $149 license with 12 months of updates, then optional $89/year renewal | Own a useful local workflow with optional updates | Clear local ownership; simpler inference economics | Revenue seasonality; customers may expect indefinite compatibility/support | Durable local operation, export, update policy and compatible licensing |
| Hybrid subscription — preferred | Pro $29/month; possible $290/year after validation; optional team service later | Continuous improvements to recurring work, with local authority and BYO compute | Aligns with current architecture; recurring revenue without inference subsidy | Must demonstrate continuing value; offline licensing and cancellation need fair semantics | Reliable local journey, continuity/routines, upgrade and entitlement behavior |
| Open-core | Free core; paid application around $29/month and commercial team features | Inspectable foundations plus product/support value | Trust, adoption, integration contributions | Free alternatives may satisfy buyers; dual maintenance and license confusion | Explicit licensing, clear boundary, contribution policy and commercial differentiation |
| Enterprise licensing | First qualifying design customers; hypothetical $25k–100k/year depending on deployment/support scope | Governance, customer-controlled deployment and operational accountability | Larger contracts, strong recurring needs | Custom work and support can consume the company; certification promises are expensive | SSO/access lifecycle, audit, deployment lifecycle, support capability and contractual clarity |
| First-party capability modules | Existing paid users; experiment with small add-ons such as Prompt Intelligence or Research Pro | Incremental workflow value without bundling every specialist capability into the core | Additional ARPU; tests ecosystem demand before a public SDK | Nickel-and-diming; weak differentiation if the module should be core; extra support surface | Stable module seam, capability policy, provenance, metering and measurable incremental outcome |
| Marketplace | Later ecosystem; test 10–20% transaction commission | Discover and operate trusted procedures/integrations | Potential distribution and complementary supply | No value without demand; quality and security burden; weak early liquidity | Installed base, trusted extension model, versioning, discovery, payments and support ownership |

The repository inspection did not locate a LICENSE/COPYING file in main's tracked file inventory. Do not describe the current project as open source or assume redistribution rights from repository availability. Open-core is a future licensing decision, not an existing fact.

A perpetual application and a subscription can both work in principle. Offering both at launch would obscure the willingness-to-pay experiment and increase entitlement complexity. Start with one coherent offer.

## 5. Recommended first offer

**Pro pilot: $29/month per accountable human, with existing provider/harness access paid separately.**

The value proposition should be a repeatable maintenance workflow that returns a candidate diff, acceptance evidence, useful interruption recovery and a clear next decision. Only advertise capabilities that work in the pilot's declared environment.

Do not initially charge by:

- agent seat, because creating more agents is not customer value;
- generated token, because this is already a model-provider meter;
- number of tasks created, which discourages honest decomposition;
- a self-declared “successful task,” which creates pressure to label work successful;
- storage of the user's essential history, which creates distrust and weak differentiation.

An annual option could later improve cash flow and retention economics, but should follow evidence of recurring use. A discount does not fix an unneeded product.

Use a time-limited trial with the full review experience. Existing artifacts, export and access to the user's work should survive trial expiration. Stopping payment should not terminate a running process without warning or make its safety state unknowable.

### What stays free

The free experience should prove one complete bounded delegation and review journey, at a sensible usage or active-work limit. Preserve:

- accurate status, required evidence visibility and honest failure reporting;
- baseline policy enforcement and supported isolation;
- stop/recovery visibility and the ability to retrieve one's work;
- local inspection and export of one's records;
- access to artifacts after cancellation.

Free does not have to mean unlimited recurring automation, every workflow, unlimited concurrency or commercial support.

### What can justify payment

Charge for recurring convenience and collaboration that users actually return for:

- repeatable approved maintenance procedures;
- continuity and recovery across a series of work episodes;
- efficient review and decision batching;
- useful history that prevents repeated investigation;
- selective cross-harness handoff once demonstrated;
- later, shared ownership, policy distribution and organizational access controls.

Basic safety belongs in the foundation. Advanced organization policy administration can be paid without making a free user's work unsafe.

### Optional first-party modules

After the core Pro offer demonstrates retention, test whether bounded optional capabilities create incremental willingness to pay. Two plausible experiments are **Prompt Intelligence** (for example +$5–10/month) and **Research Pro** (for example +$8–15/month). These ranges are pricing hypotheses, not recommendations to ship separate SKUs immediately.

The test is whether the module measurably improves the customer's workflow beyond the core product: fewer clarification/retry cycles, lower supervision effort, better evidence or a repeated research job the user would otherwise perform manually. Do not split baseline safety, truthful status, evidence access or essential acceptance behavior into paid modules.

Use first-party modules to learn the extension contract before exposing an SDK. Third-party marketplace economics should wait until there is both buyer demand and credible external supply. [Capability/module strategy](CAPABILITY_MODULE_ECOSYSTEM.md).

## 6. Revisiting the existing price/feature concept

The architecture baseline proposes a generous free local product, Pro around €15–20, Power around €35–40, Team around €25–40 per seat, and later enterprise/marketplace revenue. These are project intentions, not validated prices. [Existing monetization sections](../../../../runlith/AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md).

Three changes are advisable:

1. **Test a single professional offer first.** Pro/Power segmentation before behavior is known creates packaging work rather than learning.
2. **Do not make stronger baseline enforcement a premium identity.** A user should not interpret “advanced firewall” as paying to prevent an otherwise avoidable unsafe action.
3. **Define recurring value before assigning features to tiers.** Memory, routers and context modes are implementation concepts; the purchase proposition is less repeated work and more dependable completion.

The suggested $29 USD pilot is a deliberate hypothesis, not a currency conversion of the existing euro proposal. Local taxes and regional presentation would require a separate commercial decision.

## 7. Teams and enterprise

After individual value is demonstrated, test **$59 per active human/month**, with a small team minimum only if support economics require it. For a five-person team, that is $295/month before compute.

The team upgrade must add a shared outcome: a colleague can take responsibility without rebuilding context; reviewers see the same current evidence; policy and permissions reflect the organization; departures do not orphan work.

Read-only reviewers should have a low-friction participation path. Charging every occasional approver can suppress the collaboration that makes the product valuable. Do not price autonomous worker processes as employees.

Enterprise should be sold when a buyer has a concrete deployment, identity, audit or support requirement and an identified budget owner. Avoid leading with speculative compliance promises. Self-hosting is not a low-effort checkbox: upgrades, migrations, diagnostics, support and compatibility remain obligations.

Qualify enterprise demand through a bounded paid design engagement with a supported environment and agreed outcome. Do not exchange an indefinite bespoke roadmap for an unsigned letter of interest.

## 8. Unit economics and support risk

These are arithmetic scenarios using assumed variable costs. They exclude fixed R&D, salaries, acquisition, refunds, taxes and financing. Contribution margin is not operating profit.

| Scenario | Revenue/month | Assumed variable costs/month | Contribution | Interpretation |
|---|---:|---:|---:|---|
| Local Pro, manageable support | $29 | Payment $1.20 + coordination $0.80 + support $4 | $23; 79% | Attractive only if integration/support burden stays small |
| Local Pro, 20 support minutes at $60/hour | $29 | Payment $1.20 + coordination $0.80 + support $20 | $7; 24% | A few difficult environment issues can erase the benefit of local compute |
| Five-person team | $295 | Payment $9 + service $20 + support $45 | $221; 75% | Plausible only after validating actual organization support needs |
| Pro with $15 of subsidized inference added to first scenario | $29 | $6 baseline + $15 inference | $8; 28% | “Unlimited AI” or hidden retry cost can break the model |

At $23 contribution per subscriber, covering $10,000/month of fixed cost requires approximately 435 subscribers; $30,000 requires approximately 1,305. These are break-even arithmetic, not acquisition or profitability forecasts.

A local-first application avoids paying for every user's execution, but creates heterogeneous-environment support. Restrict supported configurations initially, record failure categories and measure support minutes per active customer.

Track gross retention, actual recurring use and contribution after support. Annual prepayment can conceal disengagement if the team looks only at cash receipts.

## 9. Compute and entitlement boundaries

Keep application price, provider charge and optional remote infrastructure charge separate and understandable. A Runlith budget must include planning, retries and verification, not only implementation tokens.

Where providers support it, use official authentication and metering paths. The Claude SDK guidance, for example, distinguishes applicable subscription/credit behavior; do not assume a consumer subscription is an unlimited automation backend. Provider terms and integration details require a current check before commercial launch. [Official SDK guidance](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan).

“Subscription optimizer” should mean explainable choice among supported entitlements and costs. It should not mean scraping credentials, evading limits or promising pooled unlimited access.

If a run's cost cannot be measured accurately, disclose the uncertainty and bound exposure where possible. Hidden retries and silent model upgrades undermine both customer trust and margins.

## 10. Willingness-to-pay experiments

Run sequential experiments rather than presenting respondents with six hypothetical business models.

| Stage | Experiment | Evidence that matters | Evidence that does not suffice |
|---|---|---|---|
| Problem | Interview 10–12 qualified engineers about recent delegated changes | Repeated concrete supervision/recovery pain and existing workaround | Enthusiasm for the phrase “AI Engineering OS” |
| Value | Observe matched maintenance work with existing tools and a bounded Runlith pilot | Measured human effort, acceptance quality and total cost | A polished happy-path demo |
| Price | Offer the declared $29 monthly pilot after value is experienced | Actual payment by at least 3 of the first 10 qualified participants, as an initial learning gate | A survey saying they might pay |
| Retention | Follow the paying cohort for at least four weeks | Repeated unprompted use and a second paid period when due | A one-time assisted success |
| Team | Offer the shared workflow to existing engaged teams | A budget owner pays for a team-level benefit | Individual users asking for a free shared dashboard |

The sample is too small for statistical market estimation. Use it to expose contradictions and decide whether to invest in a larger test.

Record why people decline: no recurring need, incumbent feature sufficient, setup burden, trust, missing integration, unclear billing or price. A discount is an appropriate response only if price is actually the constraint.

## 11. Commercial stop conditions

Reconsider the wedge if qualified users like the idea but repeatedly choose native tools, if review time does not improve, or if each useful outcome needs founder intervention.

Do not respond to weak conversion by adding more providers, memory subsystems or enterprise settings. Investigate whether the product removes a recurring cost that customers recognize.

If the strongest demand is only for enterprise governance, treat that as a distinct customer and product decision with different economics. If demand is only for a pleasant personal utility, a smaller paid application business may be more appropriate than a venture-scale platform.
