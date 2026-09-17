# Runlith strategic research — 17 September 2026

This package answers: **If building the next generation AI Engineering OS in 2026, what should Runlith become?**

The recommended direction is a local-first system for accountable delegation and acceptance of engineering changes, beginning with a narrow paid maintenance workflow for professional engineers.

This is a strategic research package. It does not change source code, accepted ADRs, gate closure, the implementation queue or product commitments.

## Reading order

| Document | Decision it supports |
|---|---|
| [Executive summary](EXECUTIVE_SUMMARY.md) | Direction, priorities, risks and first monetization |
| [Product strategy](PRODUCT_STRATEGY.md) | First paying customer, initial workflow, discovery and falsification |
| [Current architecture assessment](CURRENT_ARCHITECTURE_ASSESSMENT.md) | What is implemented, strong, limiting and still aspirational |
| [Future architecture roadmap](FUTURE_ARCHITECTURE_ROADMAP.md) | Core semantics and conditional 3–5 year evolution |
| [Market analysis](MARKET_ANALYSIS.md) | Requested competitor profiles, adoption mechanisms and current convergence |
| [UI/UX principles](RUNLITH_UI_UX_PRINCIPLES.md) | Information hierarchy, attention, review, recovery and professional-app lessons |
| [Monetization strategy](MONETIZATION_STRATEGY.md) | Pricing alternatives, proposed first offer, economics and payment experiments |
| [Business positioning](BUSINESS_POSITIONING.md) | Timing, category, moat, distribution, risks and scale scenarios |
| [Capability & module ecosystem](CAPABILITY_MODULE_ECOSYSTEM.md) | First-party paid modules, capability boundaries, SDK/marketplace sequencing and ecosystem evidence gates |
| [Evidence register](EVIDENCE_REGISTER.md) | Repository provenance, source map, freshness corrections and limitations |

For a short decision meeting, read the executive summary and product strategy, then use the other documents to challenge the assumptions.

## What the package changes conceptually

- Preserve durable work, independent completion authority, local control and explicit security boundaries.
- Make the customer outcome an accepted useful change, with measurable human effort and cost.
- Treat provider breadth, memory and team cloud as conditional expansions.
- Test a $29/month professional local product with separate compute charges.
- Preserve a future capability-module seam, but validate first-party paid modules before building an SDK or marketplace.
- Use actual paid recurring behavior to decide whether the broader platform thesis deserves investment.

All Runlith prices and experiment thresholds are proposals, not validated facts.

## Relationship to existing work

The main implementation baseline is 891c36f; the research checkout baseline is d3a656b. Main advanced during the study to f063af9 with a documentation-only addition, which was reconciled. Detailed provenance and the G7 closure-evidence discrepancy are in the evidence register.

The existing [external-research libraries](../../README.md) remain supporting material. The new main [desktop UI/UX research foundation](../../../../runlith/docs/ui/research/2026-09-17-runlith-ui-ux-research.md) supplies detailed Task-first UI alternatives; this package adds business and customer strategy and preserves its domain constraints.

The documents are written in English for reuse in product and architecture discussions. They separate current facts, historical observations and strategic hypotheses. No interviews, fresh runtime tests or competitor performance benchmarks were conducted.
