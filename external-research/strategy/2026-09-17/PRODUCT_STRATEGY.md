# Runlith: product strategy and validation plan

Research date: 17 September 2026. Status: proposed strategy. Customer selection, thresholds and pricing require validation; no customer interviews or pilot results were supplied. This document does not alter the implementation queue or accepted security/architecture gates.

**First product:** an accountable delegation workflow for bounded, recurring repository maintenance. Use the customer's existing coding agent, repository and CI. Return a reviewable change, specific acceptance evidence and a recoverable work record.

## 1. Product thesis

**Runlith exists because** delegated engineering work outlives the conversation that produced it, while people remain accountable for scope, quality, cost and delivery.

**The user problem is** reconstructing that work: what was intended, what changed, what was actually checked, what remains uncertain, and how to continue after interruption or rejection.

**Current solutions fail when** work crosses sessions, tools, execution locations or reviewers and the user must assemble a trustworthy acceptance picture manually. This is a bounded hypothesis; current products already solve parts of it and must be tested as the baseline.

**Runlith wins if** its durable work and independent evidence model measurably reduces supervision and rework without adding equivalent setup, ceremony or compute cost.

The thesis is not “agents need a better dashboard.” It must remain valuable even when the chosen coding agent improves.

## 2. Candidate customers

This is a qualitative prioritization based on expected pain and adoption friction, not a scored market survey.

| Segment | Likely motivation | Constraint | Initial decision |
|---|---|---|---|
| Hobby/learning developer | Explore agents and finish personal projects | Limited budget; irregular work; weak acceptance baseline | Useful feedback, not the primary commercial target |
| Indie hacker | Ship quickly with little staffing | Highly varied work; tolerance for manual intervention; budget pressure | Secondary segment after the workflow is simple |
| Professional solo consultant | Handle several client repositories and preserve context | Client permissions, heterogeneous environments, limited support time | Promising follow-on; validate separately |
| Small startup engineering team | Delegate recurring maintenance while preserving delivery accountability | Cannot adopt heavy process or maintain another platform | Strongest initial team context |
| Senior engineer/tech lead | Review and own agent-produced changes | Pays for additional tools only when the oversight burden falls | First user and likely purchase champion |
| Large enterprise | Govern many agents and protect organizational boundaries | Procurement, deployment, identity and support requirements | Later, unless a concrete paid design customer changes the evidence |

The recommended first user is a **senior engineer or technical lead in a 3–15-person engineering team at a B2B SaaS company with an existing maintained product**. They already use a capable coding agent, have repeatable checks, and remain responsible for reviewing changes.

Do not require multi-provider use to make the first workflow useful. Recruit some users who already alternate between harnesses to test whether portability has incremental value.

## 3. Ideal first paying customer: operational definition

Seek customers who:

- delegate repository work to AI at least weekly;
- can name recent interrupted, rejected or labor-intensive agent changes;
- have an existing repository, CI and human review practice;
- encounter bounded maintenance often enough for repeated use;
- can authorize the tool within their team's actual data/tool rules;
- can purchase a modest professional subscription without a long procurement cycle;
- will allow observation of workflow time and outcomes, with appropriate data consent.

Start in an explicitly supported execution environment. The current concrete confinement path is macOS; cross-platform IPC does not establish equal production isolation elsewhere. Select the initial repository ecosystem based on customer demand and verifiability, not simply because Runlith itself uses Rust.

Avoid initial pilots that require a new enterprise identity system, multi-repository migrations, critical production operations or an unknown test environment. These may become future customers, but would confound the first value experiment.

**Buyer:** initially the same accountable engineer, technical founder or team lead with a tools budget. **Beneficiary:** the engineer and reviewer. **Later economic buyer:** an engineering manager or platform owner paying for shared workflow and governance.

## 4. First job to be done

> When I have a bounded maintenance change in a repository I own, help me delegate it, leave it safely within an agreed scope, and return to a candidate I can accept or correct without reconstructing the entire session.

Suitable initial examples:

- fix a reproducible defect with a meaningful regression test;
- update a bounded API usage with existing checks and an explicit compatibility criterion;
- apply a limited dependency update where the acceptance path is already understood.

These are candidate task classes, not a promise that all such work is low risk. Select and review actual tasks. Exclude broad rewrites, poorly specified product design, automatic production deployment and changes whose acceptance cannot be established in the pilot.

The output is not necessarily an automatic merge. It can be a reviewable branch or PR with evidence and a clear human decision. Delivery should mean the specific destination agreed for that change.

## 5. The minimum complete product experience

| Moment | Customer outcome | Why it matters |
|---|---|---|
| Connect a repository | Existing source, checks and harness are usable | Adoption does not require migrating the work system |
| Define a change | Outcome, non-goals, scope and acceptance are understandable | Prevents a plausible but wrong result |
| Delegate | A bounded assignment starts with visible authority and budget | The user can leave without granting unlimited discretion |
| Interrupt | Partial work and state remain intelligible | Recovery is a core value claim |
| Review | Exact diff, requirement coverage and unresolved evidence are visible | Saves reconstruction and avoids false confidence |
| Correct or take over | The user can continue from the actual state | Failure remains useful rather than a dead end |
| Accept/deliver | Decision and observed destination link to the right revision | Closes the workflow and creates a useful record |
| Repeat | A later similar task requires less setup | Establishes recurring value |

This is a product completeness definition, not a request to implement every future engine. It can initially be supported by one real harness and a narrowly declared environment.

The current independent evaluator is a placeholder in production, so the verified acceptance claim remains a product-critical gap. Existing gate prerequisites remain authoritative. [Current assessment](CURRENT_ARCHITECTURE_ASSESSMENT.md).

## 6. What to preserve, change, simplify and remove

| Decision | Recommendation |
|---|---|
| Preserve | Durable work identity, independent completion authority, policy/enforcement separation, versioned planning, local control and provenance |
| Change | Frame the product around accepted engineering changes and human decisions |
| Simplify | The first journey, supported environment matrix, provider catalog, planning ceremony and memory scope |
| Remove from the launch promise | Universal autonomous engineering, broad enterprise readiness, cross-platform isolation parity, a full engineering memory platform and automatic overnight delivery |
| Keep as conditional options | Remote workers, team cloud, capability/module ecosystem and organization-wide learning |

“Remove from the promise” does not mean deleting already useful code or weakening safety. It means stop asking the customer to buy an unproven future platform.

## 7. Discovery: learn from actual behavior

Recruit 10–12 qualified participants across several companies. Record role, repository characteristics, existing tools, task frequency and buying authority. Do not treat people from one team as independent market validation.

Ask for a walkthrough of their most recent relevant change:

1. What was delegated, and what made it suitable?
2. Which tools and checks were used?
3. Where did they intervene, wait, restart or reconstruct context?
4. What evidence made them accept or reject the result?
5. What did the work cost in active time, compute and delay?
6. What have they already tried to improve this?
7. Who could pay, and which existing subscription would Runlith add to or replace?

Ask about completed and abandoned work. Interviewing only successful users of AI will hide failure costs; interviewing only frustrated users will exaggerate the market.

Avoid “Would you like more autonomy?” and “Would you use an AI Engineering OS?” These produce weak evidence.

## 8. Pilot design: compare against the real alternative

Use approximately two weeks of baseline observation, then two to four weeks of a supported pilot, followed by retention observation. This is a proposed research cadence, not an estimate that missing engineering can be finished within those dates.

The comparator is the participant's **best current harness/editor + Git/PR + CI workflow**, including native planning, checkpoints and automations they actually use. Do not disable useful incumbent features.

Aim for at least ten eligible changes per participant over the observation period where realistic. Stratify by task type, approximate size and repository familiarity. Use matched comparable tasks and vary order when feasible; repeating the identical task after solving it creates learning contamination.

For acceptance review, use a reviewer who did not implement the change where practical. Blind the reviewer to the workflow used when feasible, while preserving all required evidence. Keep acceptance criteria stable across conditions.

Record founder assistance separately. An operator silently repairing state or completing a task must not count as autonomous product success.

## 9. Measurement

**Primary value measure:** human supervision and rework minutes required per accepted useful change, with quality, cost and adoption guardrails.

Use a pooled measure that includes effort spent on failed and abandoned work:

**Total active supervision/rework minutes across all eligible delegated changes ÷ number of accepted useful changes.**

If no changes are accepted, report that failure; the ratio is undefined, not zero. Also show per-user and task-class results, distributions and acceptance rate. A median over successful tasks alone would hide expensive failures.

| Metric | Definition | Why it is needed |
|---|---|---|
| Accepted useful change | Human acceptance of the specified candidate against declared criteria, with required checks accounted for | Prevents “agent stopped” from becoming success |
| Acceptance rate | Accepted changes / all eligible started changes | Includes failed and abandoned work |
| Human effort | Active scoping, steering, review, recovery and rework time | Measures attention saved, not machine activity |
| Setup/admin burden | Initial setup plus recurring configuration/support time | Prevents hidden adoption costs |
| Time to reviewable candidate | Elapsed time, reported separately from active time | Captures delivery delay without assuming all waiting is lost labor |
| Rejection/reopen rate | Changes requiring correction at review or reopening within an agreed follow-up window, initially seven days | Adds a quality guardrail |
| Evidence integrity | Wrong-revision, skipped-required-check and unsupported-success incidents | Tests the core trust claim |
| Verified delivery cost | All measured planning/execution/retry/verification/worker costs across the cohort, divided by accepted delivered outcomes | Includes failure cost; unknown cost stays unknown |
| Recovery burden | Active minutes and success rate for declared interruption classes | Tests the proposed differentiator directly |
| Retention | Repeated voluntary use and continued payment | Separates curiosity from recurring value |
| Support burden | Staff minutes and interventions per active customer | Tests economic sustainability |

The delivery denominator must match the declared delivery target. A PR-ready result, merged change and production-validated change are different outcomes. Do not silently mix them.

Report small-cohort findings as directional. Preserve raw per-participant comparisons and explain selection, learning and instrumentation limitations.

## 10. Proposed decision gates

These thresholds are initial management choices, not statistically established market boundaries.

| Gate | Provisional positive signal | Failure implication |
|---|---|---|
| Recurring problem | Most qualified participants show repeated concrete supervision/recovery work | Narrow or replace the target job |
| Usability | Participants complete the core journey without founder explanation and understand incomplete evidence | Simplify before expanding capability |
| Value | At least about 30% lower human effort per accepted change in the matched cohort, with consistent individual benefit | Investigate ceremony, task fit and native alternatives |
| Quality | No observed false verified state; no material deterioration in rejection/reopen outcomes | Fix the evidence model or restrict scope; a small clean sample is not proof of zero risk |
| Cost | Added compute does not consume the customer's perceived benefit | Bound retries, change the workflow or reconsider the offer |
| Payment | At least 3 of the first 10 qualified purchasing participants pay the declared $29 monthly price | Investigate why before discounting |
| Retention | Engaged customers use the workflow in at least 3 of 4 follow-up weeks when eligible work exists | Determine whether value is episodic or insufficient |
| Operability | Useful results increasingly require no staff intervention; support cost fits the economic model | Restrict supported scope or stop scaling acquisition |

No single gate establishes PMF. Three friendly pilot payments are a reason for a larger test, not a reason to claim a category.

## 11. Product priorities

**Priority 1: establish the actual pain.** Observe current work, including interruption and rejected changes. Validate which user cares enough to pay.

**Priority 2: complete the smallest trustworthy acceptance loop.** The next visible product outcome should connect intent, candidate revision, required evidence and a human decision.

**Priority 3: make recovery and review understandable.** A simple return-to-work experience should show what survived and what needs judgment.

**Priority 4: demonstrate repeat use and paid value.** Instrument the pilot and reduce the assistance burden.

**Priority 5: prove a second real harness where customer need warrants it.** Test continuity and evidence fidelity, not only connection success.

**Priority 6: add team coordination after individual value is stable.** Shared ownership and access become a paid expansion, not initial setup overhead.

**Conditional expansion: test first-party modules after core retention.** Prompt Intelligence or structured Research can be used to test whether customers pay for bounded incremental capabilities. Do not build a public SDK or marketplace until repeated internal modules and external developer demand justify a stable extension contract.

This ordering is strategic. It does not authorize bypassing G8 or any other gate; a prototype can validate interaction before its full execution backend is available.

## 12. What not to build now

- A replacement IDE or general project tracker.
- An agent organization chart, universal council or model-voting product.
- A public plugin marketplace before repeated demand for first-party/shared capabilities and a proven extension contract.
- A broad memory/knowledge graph platform before a retrieval benefit is measured.
- Every provider and execution platform at once.
- A multi-region scheduler or mandatory cloud account for the local use case.
- A scoring system that compresses unresolved evidence into confidence.
- A generic browser/document automation platform.
- Self-modifying policy or autonomous weakening of acceptance criteria.
- A feature-rich enterprise edition before a qualified buyer and support model exist.

## 13. How the product can expand

The plausible primary expansion sequence is:

**One accountable engineer → repeated repository workflow → teammate handoff → shared procedures and policy → multiple repositories/remote execution → governed engineering operations.**

A parallel commercial expansion may become viable after retention:

**Core subscription → first-party modules → paid module experiments → bounded SDK → third-party ecosystem → marketplace.**

At each step, add a real responsibility the customer wants to delegate. Do not expand merely because the architecture has an extension point.

The long-term product can coordinate a mixture of people and agents. Its enduring value would be preserving intent, responsibility and acceptance as execution becomes cheaper and more capable.

See [Capability & module ecosystem](CAPABILITY_MODULE_ECOSYSTEM.md) for the proposed module classes, permission model and evidence gates.
