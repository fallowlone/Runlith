# Runlith capability and module ecosystem

Research date: 17 September 2026. Status: strategic proposal; this document does not authorize SDK, marketplace, billing, protocol, security-boundary or implementation work.

**Thesis:** Runlith can eventually become a platform for trusted engineering capabilities, not merely a host for agent providers. The sequencing should be **core workflow → first-party modules → paid first-party modules → bounded SDK → third-party ecosystem → marketplace**. Every later step is conditional on the core Runlith workflow first proving recurring paid value.

## 1. Why modules fit the product

The durable product is the engineering workflow: intent, authorized work, execution, evidence, acceptance and delivery. A module should add one bounded capability to that workflow without becoming a second orchestration system.

Examples include:

- **Intent / Prompt Intelligence** — turn a weak or underspecified request into a proposed stronger instruction using repository context, user preferences, scope, acceptance criteria and constraints.
- **Research** — collect external or repository research, sources, claims, uncertainty and artifacts that planning or review can consume.
- **Execution** — add a specialized harness, browser-capable worker or domain-specific execution adapter.
- **Verification** — security review, performance checks, accessibility, architecture review or specialized test/evaluation capabilities.
- **Knowledge** — validated repository memory, organization rules, retrieval and codebase understanding.
- **Delivery** — pull requests, issue updates, release/deployment actions and delivery reconciliation.
- **Business / Governance** — cost analysis, audit exports, policy packs, reporting and organization controls.

The commercial value is not that Runlith has "plugins." It is that useful capabilities can participate in the same durable, permissioned and evidence-producing workflow.

## 2. Module contract

A module should not be an arbitrary prompt file with ambient access. A future module contract should conceptually resemble:

**typed input → declared capabilities → bounded execution → structured artifacts/evidence → typed result**

Illustrative capability declarations:

- `network.read`
- `repository.read`
- `repository.write`
- `browser.use`
- `secrets.request`
- `llm.call`
- `artifact.create`

The names above are examples, not a proposed protocol schema. The important architectural rule is that installation does not grant authority. Runlith remains responsible for deciding whether a requested capability is allowed for the current Task, policy, workspace, user and environment.

Modules must not silently bypass Policy Engine, runtime isolation, approval, evidence or acceptance semantics. A module may propose or produce work; it does not acquire completion authority merely because it is installed or paid.

## 3. First-party modules before a marketplace

The first ecosystem test should be first-party. That avoids the large operational surface of third-party discovery, signing, review, version compatibility, moderation, support ownership, payouts and fraud before there is demand.

Two particularly useful candidates are:

### Prompt Intelligence

Input:

**raw intent + repository context + user preferences + current constraints**

Output:

**proposed enriched intent + explanation/diff of what was added**

The user should be able to see the transformation before it becomes authoritative. Useful additions may include scope, non-goals, acceptance criteria, verification expectations and relevant repository conventions. The module must not silently broaden scope or invent authority.

Its value can later be tested against observable outcomes such as fewer clarification cycles, fewer retries, lower supervision time and a higher accepted-change rate. Those metrics require controlled comparison; they should not be presented as causal proof merely because usage correlates with better outcomes.

### Research

Input:

**question + allowed sources/scope + freshness requirements**

Output:

**source set + claims + timestamps + uncertainty + reusable research artifact/evidence**

Research should produce structured provenance rather than an opaque prose answer. The planner or human reviewer may consume that artifact, but the research module does not automatically convert external claims into trusted policy or repository truth.

## 4. Commercial model

The core subscription should remain understandable on its own. Modules can create a second expansion path once recurring core value exists.

Possible experiments, not validated prices:

| Offer | Example pricing hypothesis | Purpose |
|---|---:|---|
| Runlith Pro | $29/month | Core delegated-work and acceptance workflow |
| Prompt Intelligence | +$5–10/month | Higher-quality intent preparation and reusable task framing |
| Research Pro | +$8–15/month | Structured external/repository research with provenance |
| Security / Verification pack | +$10–20/month | Additional evidence-producing checks |
| Team module packs | bundled or per-seat | Shared procedures, governance and organization-specific capabilities |
| Future marketplace | 10–20% transaction commission hypothesis | Third-party distribution after installed-base demand exists |

Packaging should follow observed willingness to pay. If modules merely split expected core functionality into nuisance add-ons, they reduce trust. Baseline safety, honest status, evidence visibility, export and core acceptance semantics should not be paywalled as premium modules.

## 5. A measurable ecosystem, not a prompt catalog

Runlith has a potential long-term advantage if it can evaluate modules by real engineering outcomes rather than descriptions or star ratings.

For example, a module could eventually have an evidence-backed operational profile containing measurements such as:

- eligible runs;
- accepted useful change rate;
- supervision/rework minutes per accepted change;
- verification cost;
- retry and failure frequency;
- support or intervention burden;
- environments in which the measurements were obtained.

These should be cohort- and context-aware. A single universal "quality score" would hide task mix, repository difficulty, provider differences and missing evidence. Runlith should preserve the underlying measurements and uncertainty.

## 6. B2B implications

The same module model becomes more valuable for teams because organizations can standardize approved capability packs instead of allowing every developer to assemble an opaque agent stack.

Examples:

- an approved dependency-update procedure;
- an organization research module restricted to allowed sources;
- a security-review pack required for sensitive repositories;
- a provider-access module with budget and model restrictions;
- a release module that requires specific approvals and evidence before external publication.

For Business customers, useful controls may include module allowlists, version pinning, capability policy, repository scope, audit history, spending limits and approved distribution channels. These remain future capabilities, not present claims.

## 7. Ecosystem moat and limits

An ecosystem becomes defensible only if it contains capabilities and procedures customers repeatedly rely on and if Runlith provides a better trust, measurement and distribution layer than direct scripts or provider-native extensions.

Possible compounding assets include:

- accumulated organization-specific procedures and policy packs;
- compatibility knowledge across providers and environments;
- provenance-rich outcome measurements;
- developer distribution for useful third-party capabilities;
- switching cost created by valuable workflows, not by trapping customer data.

The negative case must remain explicit: if strong providers make capability installation, permissions, evaluation and workflow integration sufficiently good inside their own products, Runlith's marketplace layer may add little value. The ecosystem should therefore be earned through customer pull, not assumed as a moat.

## 8. Evidence gates before each stage

| Stage | Build only when | Do not infer |
|---|---|---|
| Core Runlith | Users repeatedly pay for the accountable delegation/acceptance workflow | That they want add-ons |
| First-party modules | Repeated workflow pain maps to a bounded optional capability | That a marketplace is needed |
| Paid modules | Users voluntarily pay for measurable incremental value | That third-party supply exists |
| SDK | Multiple internal modules need a stable reusable contract and external developers request access | That open extension points create a network effect |
| Third-party ecosystem | External developers can produce useful modules under enforceable capability boundaries | That discovery and trust are solved |
| Marketplace | Buyers and sellers transact repeatedly enough to justify payments, review, moderation, versioning and support | That commission revenue will be material |

## 9. Near-term architecture rule

Do **not** build the universal plugin SDK or marketplace now. Preserve seams that make future modules possible:

- provider-neutral typed inputs and outputs;
- explicit capability requests;
- policy-mediated authority;
- sandbox/runtime enforcement where required;
- artifacts and evidence with provenance;
- versioned contracts;
- no direct database or UI-owned domain mutation;
- observable cost and result boundaries.

The immediate product priority remains a complete, paid, trustworthy engineering-change workflow. The module ecosystem is an expansion strategy that should become easier because the core architecture is disciplined, not a reason to postpone product validation.
