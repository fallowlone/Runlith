# Runlith: strategic executive summary

Research date: 17 September 2026. Based on the main implementation at 891c36f, the existing external-research repository, subsequent documentation-only main commit f063af9, and current public product research. This is strategic advice, not an accepted roadmap change. [Scope and evidence](EVIDENCE_REGISTER.md).

**Runlith should become a local-first system for accountable delegation of engineering changes: preserve intent, control execution, recover interrupted work, and make acceptance understandable through specific evidence.**

The strongest initial business is a narrow workflow for professional engineers who already use capable coding agents and repeatedly pay the human cost of supervising, recovering and reviewing their work.

## 1. What Runlith should become

The long-term ambition is credible: engineering work increasingly spans human intent, autonomous execution, verification and human judgment. The durable product object is the work and its acceptance, not an agent conversation.

Runlith should own the relationship among:

**Intended outcome → authorized work → exact candidate artifact → acceptance evidence → human decision → observed delivery.**

Keep code editing, repository hosting and organizational planning connected to the tools users already have. Initially, the existing Task can carry the work identity; a user-facing “change” does not require a second lifecycle.

The interface should feel like calm control over consequential work. Its home should explain what needs a decision and what is ready to review. The diff and evidence should be more prominent than the agent roster or a large task graph.

The current project already has important foundations: Task/Execution/Attempt separation, versioned planning, durable authority checks, policy and enforcement boundaries, workspace ownership and independent completion authority. Those deserve preservation. [Architecture assessment](CURRENT_ARCHITECTURE_ASSESSMENT.md).

The key limitation is product completeness. The production evaluator currently leaves work awaiting verification; only one real provider integration was found; continuity, memory and broader unattended operation remain partly future gates; a production desktop experience was not found. This is a promising foundation, not yet demonstrated autonomous verified delivery.

## 2. The strongest strategic direction

**First user:** a senior engineer or technical lead in a 3–15-person B2B SaaS engineering team with a maintained repository, working CI and regular agent use.

**First job:** a bounded maintenance change, such as a reproducible bug fix with a meaningful regression test or a limited compatibility update.

**Value proposition:** delegate using existing tools, leave within an agreed scope, and return to a reviewable result with clear evidence and a recoverable history.

**Main success measure:** reduced human supervision and rework per accepted useful change, including time spent on failed and abandoned work. Guard it with acceptance quality, total cost, support burden and repeated voluntary use.

This is a hypothesis, not proven PMF. Native agents and trackers are strong alternatives. Linear already runs coding sessions; Warp advertises factories; Codex and Cursor support parallel/background workflows. Windsurf is now Devin Desktop, and Continue's official homepage says it has joined Cursor. “Multiple agents in one place” is insufficient differentiation. [Linear sessions](https://linear.app/docs/coding-sessions), [Warp Factories](https://www.warp.dev/factories), [Codex app](https://openai.com/index/introducing-the-codex-app/), [Cursor cloud agents](https://cursor.com/docs/cloud-agent), [Devin Desktop](https://cognition.com/blog/introducing-devin-desktop), [Continue](https://continue.dev/).

The competitive proposition must be observable: less reconstruction, more reliable acceptance, and useful continuity in the customer's actual workflow.

## 3. Biggest risks

| Risk | Why it matters | What changes the decision |
|---|---|---|
| Incumbents absorb the workflow | They already own the editor, model, repository or issue tracker | Runlith demonstrates a meaningful advantage against their current combined workflow |
| Infrastructure precedes demand for too long | More correct subsystems do not establish a paying use case | Paid recurring pilots become an investment criterion |
| Review overhead consumes the productivity gain | More output can create more responsibility | Measure active human time and quality, not number of agent runs |
| Trust claims exceed implemented guarantees | “Finished,” “verified” and “delivered” are different facts | Bind evidence and decisions to exact revisions and show missing checks |
| Local environment support erodes margins | Local compute is not free support | Bound supported configurations and track support minutes |
| Provider neutrality has little incremental value | Many customers may prefer one primary harness | Make the first single-harness journey useful; validate portability separately |
| The product is liked but not purchased | Interest in autonomy can be aspirational | Actual payment, repeated use and reduced founder assistance |

The proposed moat is not the architecture alone. It is a trusted repeated workflow, validated repository knowledge and useful acceptance history that improve future work. None is yet demonstrated as a commercial moat.

## 4. Recommended priorities

1. **Observe 10–12 qualified users doing actual work.** Understand recent interrupted, rejected and accepted changes, existing subscriptions and purchase authority.
2. **Define the smallest complete acceptance journey.** Make intent, scope, candidate revision, checks and human decision coherent. Respect existing implementation gates.
3. **Validate review and recovery UX.** Test a compact Task/change surface and Attention queue with incomplete and stale evidence. Reuse the new UI research rather than restarting visual exploration.
4. **Run a bounded paid pilot.** Compare with the user's best current agent + Git/PR + CI workflow. Include failed tasks, setup and support in the results.
5. **Prove real portability selectively.** A second production harness should preserve work/evidence semantics, not merely connect.
6. **Expand to team ownership only after repeat use.** Shared responsibility and governance are a plausible paid expansion.

Provisional learning gates include around 30% lower human effort per accepted change, several actual $29/month purchases, repeated use across four weeks, and no observed unsupported verified state. These are management hypotheses, not statistical proof or a safety guarantee. [Product strategy and experiment design](PRODUCT_STRATEGY.md).

## 5. What not to build

Do not initially build a replacement IDE, issue tracker, company wiki, generic automation canvas, agent marketplace, universal council, large model catalog or general knowledge graph.

Do not use a single reliability score to conceal unresolved evidence. Do not promise cross-platform isolation, unlimited subscription-backed inference, automatic overnight delivery or enterprise compliance beyond demonstrated capability.

Defer distributed infrastructure and a cloud implementation-language decision until ownership, availability or scale requires them. Preserve the local modular foundation and explicit boundaries.

Reduce planning ceremony for simple work through an explicit future product/contract decision, never by bypassing accepted authority. More planning is valuable only if it improves the result or reduces downstream effort.

## 6. First monetization path

Test **$29/month for a professional local application**, with agent/model usage paid separately. The paid value is recurring workflow relief: repeatable maintenance, continuity, recovery and efficient acceptance.

Keep baseline safety, honest status, evidence access and export available. Do not make customers pay to retrieve their own work or understand whether it is safe.

A possible later team offer is **$59 per active human/month** for shared ownership, policies and evidence. Enterprise licensing and marketplaces remain conditional options.

A second long-term expansion path is a **capability/module ecosystem**. The recommended order is core product → first-party modules → paid first-party modules → bounded SDK → third-party ecosystem → marketplace. Candidate first-party modules include Prompt Intelligence, which proposes a stronger task instruction from raw intent and repository context, and Research, which returns structured claims, sources and uncertainty. Installation must never imply authority: modules remain subject to Runlith policy, capability, isolation, evidence and acceptance boundaries. [Capability/module strategy](CAPABILITY_MODULE_ECOSYSTEM.md).

The price is unvalidated. In an illustrative local-Pro scenario, $29 revenue minus $6 in payment/service/support costs leaves $23 contribution; twenty support minutes at $60/hour reduces that contribution to $7. Support and hidden compute can determine viability. [Monetization models, economics and price experiments](MONETIZATION_STRATEGY.md).

The reason to pay must be stronger than the availability of free software: the complete workflow saves recurring human effort that the existing tool bundle does not save.

## 7. Long-term vision

Over three to five years, Runlith could evolve from one engineer's delegated maintenance to shared procedures, team responsibility, remote workers and governed engineering operations.

If the core workflow earns recurring paid use, Runlith can also evolve into a trusted capability platform: optional modules participate in the same durable workflow and can eventually be distributed through a third-party ecosystem. That is a conditional expansion thesis, not a launch requirement.

The lasting architecture needs durable intent and acceptance identities, exact artifact/evidence binding, explicit ownership and recovery, replaceable harnesses, scoped knowledge and observable delivery.

An “AI engineering organization” should mean accountable responsibilities and workflows across people and agents. It should not mean a simulated company of agent personas.

The category-leading outcome would be a system people trust to preserve what they meant, what was authorized, what happened and why a result was accepted, even as models and tools change. The next decision is whether a narrow paid workflow earns that trust and creates enough recurring value to justify expansion.

## Decision confidence and remaining uncertainty

**High confidence:** preserve the core authority separations; distinguish implemented capability from design; make evidence and recovery central; compete against current incumbent workflows.

**Medium confidence:** the proposed professional maintenance niche, a focused separate control surface, and a local-first hybrid business.

**Unvalidated:** willingness to pay, retention, time saved, acquisition cost, support economics and a defensible data/workflow moat.

The final G7 closure is reported in existing working-tree metadata, but the corresponding final independent audit artifact was not located; the checked-in historical report predates later remediation. No runtime suite was rerun for this strategic study. These limits do not erase the implementation work, and they prevent overstating what this review independently established.

Continue with the [document index](INDEX.md), [market study](MARKET_ANALYSIS.md), [UI/UX principles](RUNLITH_UI_UX_PRINCIPLES.md), [business positioning](BUSINESS_POSITIONING.md) and [architecture roadmap](FUTURE_ARCHITECTURE_ROADMAP.md).
