# Runlith strategy: evidence register and limitations

Research date: 17 September 2026. This register separates repository facts, documented external capabilities, historical/user reports and strategic hypotheses. It is not a certification, a new source of implementation authority or a representative market survey.

Product-owner direction captured during the strategy follow-up: preserve a future Runlith module/plugin ecosystem, with examples including prompt enhancement and research capabilities, and consider paid first-party modules before a later third-party marketplace. This is product intent/hypothesis, not external market evidence or an implementation commitment. The strategy package incorporates it in [Capability & module ecosystem](CAPABILITY_MODULE_ECOSYSTEM.md).

## 1. Repository provenance

| Repository | Role in the study | Baseline |
|---|---|---|
| Main Runlith checkout | Current implementation and canonical project contracts | /Users/artemmac/dev/runlith; main; 891c36f4b30a030ee581344fb718403712c8bbf1, 16 September 2026 |
| External-research checkout | Existing research and destination for this package | /Users/artemmac/dev/runlith-external-research; research/external-architecture-intelligence; d3a656b47b34e800935b182e93b6eefe5431d87b, 16 September 2026 |
| Main documentation update during the study | Additional UI/UX foundation, reconciled before completion | f063af90e75e3bd7f8ba0ece78943d7808ff7431, 17 September 2026; adds only docs/ui/research/2026-09-17-runlith-ui-ux-research.md |

At initial inspection, main already had modified planning/roadmap.yaml and planning/tasks/queue.yaml, plus an untracked image. Those files were not edited by this study. The two planning files were read as existing working-tree evidence, not silently treated as committed facts. The research checkout was initially clean.

Main's code is newer than the research checkout's copy of the project. Absence in the latter was not used to infer absence in current main. The final main delta was inspected and contained no code changes.

Sibling-repository links assume these two checkouts remain side by side. They resolve to the working files, not immutable web permalinks; use the recorded commit identities to reconstruct the implementation baseline. The UI research addition belongs to the later documented commit.

## 2. Inspection scope

Repository inspection preceded external comparison and strategic conclusions. It included:

- canonical AGENT.md instructions in both checkouts;
- project README, charter, architecture navigation and relevant accepted ADRs;
- the large architecture source of truth, with focused reading of domain, planning, verification, memory, security, UX, monetization and future-platform sections;
- roadmap, queue and G0–G7 historical audit/remediation evidence;
- Cargo workspace/dependency structure and targeted implementation/call-path inspection;
- the existing external-research report inventory, pattern/anti-pattern libraries and implementation knowledge queue;
- late-arriving UI research: summary, contract gaps, IA, recommendations and open questions, with relevant wire contracts independently checked.

This is not a line-by-line audit of every repository file, nor an independent reproduction of each historical security finding. Prior research reports were inventoried and selectively traced; their claims were not automatically adopted as current product facts.

## 3. Local source map

| Question | Primary local evidence |
|---|---|
| Product intent and accepted architecture | [Charter](../../../../runlith/PROJECT_CHARTER.md), [architecture map](../../../../runlith/ARCHITECTURE.md), [architecture source of truth](../../../../runlith/AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md), [architecture index](../../../../runlith/docs/architecture/INDEX.md) |
| Actual implementation boundaries | [Cargo workspace](../../../../runlith/Cargo.toml), [application services dependencies](../../../../runlith/crates/app-services/Cargo.toml), [TaskService](../../../../runlith/crates/app-services/src/task_service.rs) |
| Task identity, scope and lifecycle | [Task](../../../../runlith/crates/task/src/task.rs), [state transitions](../../../../runlith/crates/task/src/state.rs), [ADR-0005](../../../../runlith/docs/architecture/adr/ADR-0005-task-execution-attempt-session.md) |
| Execution, scheduling and provider seam | [Execution](../../../../runlith/crates/agent-runtime/src/execution.rs), [Attempt](../../../../runlith/crates/agent-runtime/src/attempt.rs), [readiness](../../../../runlith/crates/scheduler/src/readiness.rs), [execution port](../../../../runlith/crates/scheduler/src/execution_port.rs), [provider contract](../../../../runlith/docs/architecture/providers.md) |
| Planning and authority | [ADR-0027](../../../../runlith/docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md), [ADR-0028](../../../../runlith/docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md), [Task authority derivation](../../../../runlith/crates/plan/src/task_authority.rs), [planning publication](../../../../runlith/adapters/storage-sqlite/src/plan_store.rs), [publication fencing](../../../../runlith/adapters/storage-sqlite/src/planning_control_store.rs) |
| Human decisions | [HumanDecision model](../../../../runlith/crates/plan/src/human_decision.rs), [human decision service](../../../../runlith/crates/app-services/src/human_decision_service.rs) |
| Verification | [ADR-0018](../../../../runlith/docs/architecture/adr/ADR-0018-independent-evaluation.md), [request](../../../../runlith/crates/evaluation/src/request.rs), [verdict](../../../../runlith/crates/evaluation/src/verdict.rs), [production execution and PendingEvaluator](../../../../runlith/bins/agentd/src/execution.rs) |
| Isolation, privacy and historical limits | [G6 final re-audit](../../../../runlith/docs/development/G6_TASK_0074_G6_CLOSURE_REAUDIT.md), [runner contract](../../../../runlith/docs/architecture/runners.md), [privacy architecture](../../../../runlith/docs/privacy/PRIVACY_ARCHITECTURE.md) |
| Continuity and memory | [continuity contract](../../../../runlith/docs/architecture/continuity.md), [memory ADR](../../../../runlith/docs/architecture/adr/ADR-0012-structured-memory-provenance.md) |
| Gate and product maturity | [roadmap](../../../../runlith/planning/roadmap.yaml), [queue](../../../../runlith/planning/tasks/queue.yaml), [G7 historical audit](../../../../runlith/docs/development/G7_CLOSURE_AUDIT.md) |
| UI capability and read gaps | [new UI research](../../../../runlith/docs/ui/research/2026-09-17-runlith-ui-ux-research.md), [control proto](../../../../runlith/crates/ipc-contracts/proto/agentd_control.proto), [Task proto](../../../../runlith/crates/ipc-contracts/proto/task_core.proto), [Execution proto](../../../../runlith/crates/ipc-contracts/proto/execution_core.proto), [Planning proto](../../../../runlith/crates/ipc-contracts/proto/planning_core.proto), [IPC](../../../../runlith/docs/architecture/ipc.md) |

The source trace included relevant planning service, authority, failure-matrix, cost-receipt, persistence and shipped-binary integration test files. They establish what is tested in source, not that those tests passed during this study.

Historical gate evidence was read with chronology: later G1 Windows remediation, G2 re-audit, G3/G4 closure, G5 final closure and G6 TASK-0074 supersede earlier partial/failed audit snapshots where applicable.

## 4. Material reconciliation: G7

Existing working-tree queue metadata says G7 is CLOSED following an independent final audit of TASK-0084 / 891c36f. The checked-in G7_CLOSURE_AUDIT.md is the TASK-0081 report requiring remediation.

TASK-0082, TASK-0083 and TASK-0084 subsequently changed planning authority, continuation and control-plane behavior. The latest source and ADR amendment were checked. The earlier audit's findings are therefore not repeated as current unresolved bugs.

The final independent closure artifact cited by the metadata was not located in the inspected documentation. The calibrated statement is:

**Implementation through TASK-0084 is present; final G7 closure is metadata-reported and was not independently re-established by this strategic study.**

This discrepancy is a provenance issue for the report, not a decision to reopen or close a gate.

## 5. Existing external research and its relevance

The research repository already contains twelve reports: 9router, awesome-harness-engineering, browserskill, claude-red, deepseek-harness, hermes-agent, mercury-agent, openresearch, overflow, recurrent-looped-tranformer, simple-attention-sparsification and ultra-scale-playbook.

The [research index](../../README.md), [pattern library](../../PATTERN_LIBRARY.md), [anti-pattern library](../../ANTI_PATTERN_LIBRARY.md) and [implementation knowledge queue](../../IMPLEMENTATION_KNOWLEDGE_QUEUE.md) were used to avoid relabeling existing design work as a new discovery.

| Existing research area | Strategic treatment |
|---|---|
| IKQ-001 ownership/fencing and IKQ-004 repeated blockers | Important future generic recovery work; do not confuse it with already implemented G7 planning fencing |
| IKQ-002 provider resume, IKQ-003 failure scope, IKQ-009 protocol conformance | Relevant to a second real harness and trustworthy recovery |
| IKQ-005 architecture fitness | Useful for keeping declared capabilities and actual source aligned |
| IKQ-006 hardware topology | A later optimization unless the selected customer workflow requires it |
| IKQ-007/008 browser ownership and observation identity | Preserve as specialist options; not a generic browser platform launch requirement |
| IKQ-010 evaluator environment/contamination | Directly relevant to acceptance evidence and G8 strategy |
| Model-training/attention-scaling reports | Interesting long-horizon context; not a reason for Runlith to train a foundation model |

Research is supporting evidence. No pattern was promoted into an accepted ADR or implementation task by this package.

## 6. External evidence standards

| Evidence class | What it can support | What it cannot support |
|---|---|---|
| Official documentation | Documented interfaces, behavior and limitations | Independently measured reliability or broad user satisfaction |
| Official pricing | Displayed price, currency and billing conditions at observation | Future price, actual negotiated enterprise cost or Runlith willingness to pay |
| Official release/company statement | Announced change and date where specified | Product-market fit or general availability beyond the announcement |
| Maintainer repository/paper | Maintainer intent, public design and reported experiment | Transfer of a benchmark to Runlith's customers |
| Survey/empirical study | Findings within its population and method | Universal causality or current market size |
| Public issue/forum/user post | A reported experience and candidate interview question | Complaint prevalence, current unresolved status or an exploit finding |
| Secondary reporting | A qualified historical statement where primary material is unavailable | A silently promoted primary technical source |
| Strategic synthesis | A reasoned recommendation and testable hypothesis | An observed customer fact |

External pages were consulted on 17 September 2026. Publication/event dates are separate from retrieval dates. Undated pricing pages are snapshots. Prices may be localized, annual-billing equivalents may differ from monthly billing, and usage allowances are not interchangeable.

## 7. Freshness corrections with high decision impact

| Claim used in this package | Evidence | Qualification |
|---|---|---|
| Windsurf transitioned to Devin Desktop | [Cognition announcement](https://cognition.com/blog/introducing-devin-desktop) | Announcement dated 2 June 2026; not two independent current companies |
| Continue has joined Cursor | [Official homepage](https://continue.dev/) | Acquisition date was not established; historical independent prices not reused |
| Copilot Workspace preview ended | [GitHub Next](https://githubnext.com/projects/copilot-workspace/) | 30 May 2025; distinct from current Copilot |
| SWE-agent maintainers prefer mini-SWE-agent | [Maintainer documentation](https://github.com/SWE-agent/SWE-agent/blob/main/docs/index.md) | Do not infer that all more complex harnesses are worse |
| Height shut down | [Contemporaneous report](https://www.creativerly.com/height-app-is-shutting-down/) | 24 September 2025 according to secondary reporting; primary closure page unavailable; reason not established |
| Arc is a historical design reference with a changed product trajectory | [Current homepage](https://arc.net/) | Maintenance messaging and Dia referral; no inferred revenue story |
| Linear participates directly in coding execution | [Coding sessions](https://linear.app/docs/coding-sessions) | Documented capability, not independent performance proof |
| Warp targets evaluation and improving workflows | [Factories](https://www.warp.dev/factories) | Early-access offering on the inspected page |
| Raycast pricing changed recently | [10 September 2026 announcement](https://www.raycast.com/blog/changing-how-raycast-ai-is-priced) | Do not reuse older unlimited-AI assumptions |

## 8. Decision-to-evidence chain

| Recommendation | Supporting evidence | Counterevidence or uncertainty | Validation needed |
|---|---|---|---|
| Preserve durable authority boundaries | Current domain/source and planning publication semantics | Does not establish customer value by itself | Complete supported workflow and interruption cases |
| Make acceptance central | Production evaluation gap; trust/quality research | Competitors already provide tests, artifacts and review | Compare human acceptance effort and errors |
| Narrow to professional maintenance | Existing architecture fits bounded repository work | No customer interviews establish best niche | Qualified discovery and actual paid pilot |
| Use a local hybrid commercial model | Current local architecture; available BYOK market patterns | Heterogeneous support and additional-subscription friction | Payment, retention and support economics |
| Defer broad multi-agent platform | Incumbent capability and coordination costs | Some customers may need parallel specialized work | Task-specific improvement over one strong harness |
| Prefer work/evidence UI over agent dashboard | Domain semantics and professional-app design patterns | Specific navigation/layout is untested | Prototype tasks including stale evidence and recovery |
| Treat accumulated procedures as a possible moat | Repeated workflow/knowledge can compound | No dataset, distribution or network effect demonstrated | Longitudinal outcome improvement and retention |

## 9. What was not established

- No customer interviews, purchase commitments, revenue, retention or willingness-to-pay data.
- No head-to-head execution benchmark of proprietary competitors.
- No fresh cargo build/test run, security validation or runtime performance measurement.
- No successful unattended production delivery demonstrated in this study.
- No current independent final G7 closure artifact located.
- No complete cross-platform isolation or enterprise/compliance readiness claim.
- No formal accessibility audit or instrumented comparison of the referenced desktop applications.
- No defensible TAM estimate; business scale figures are explicitly arithmetic scenarios.
- No exhaustive current legal review of provider entitlements or an approved project license strategy.

The report's uncertainty is concentrated in commercial value and production completeness, not hidden by confidence scores.

## 10. How to keep this research useful

Refresh competitor capability, pricing and entitlement facts before a purchasing or integration decision. Re-check source/capability claims when main changes.

Replace customer hypotheses with observed data as pilots happen. Preserve negative results, abandoned tasks, support interventions and competing explanations.

Keep accepted architecture decisions in the existing project process. These documents should inform that process without becoming another oversized canonical specification.

## 11. External source catalog

The catalog below indexes unique external URLs cited in the reports. Claims and qualifications remain beside their citations in the relevant report; the catalog is a navigation aid rather than a second set of endorsements.

| Source | Evidence class | Used in |
|---|---|---|
| [agentclientprotocol.com: ACP introduction](https://agentclientprotocol.com/get-started/introduction) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [agpt.co: platform licensing explanation](https://agpt.co/blog/introducing-the-autogpt-platform/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [agpt.co: agent product](https://agpt.co/product/agents/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [aider.chat: product and workflow](https://aider.chat/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [aider.chat: repository map](https://aider.chat/docs/repomap.html) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [arc.net: Current homepage](https://arc.net/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [arxiv.org: SDK paper](https://arxiv.org/abs/2511.03690) | Research / survey; method-specific | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [blog.superhuman.com: command-palette design](https://blog.superhuman.com/how-to-build-a-remarkable-command-palette/) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [claude.com: pricing](https://claude.com/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [code.claude.com: Agents](https://code.claude.com/docs/en/agents) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [FUTURE_ARCHITECTURE_ROADMAP](FUTURE_ARCHITECTURE_ROADMAP.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [code.claude.com: checkpoints](https://code.claude.com/docs/en/checkpointing) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [code.visualstudio.com: Profiles](https://code.visualstudio.com/docs/configure/profiles) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [code.visualstudio.com: user interface](https://code.visualstudio.com/docs/editing/getting-started/userinterface) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [cognition.com: Cognition announcement](https://cognition.com/blog/introducing-devin-desktop) | Official product / project source | [EXECUTIVE_SUMMARY](EXECUTIVE_SUMMARY.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [community.atlassian.com: action-discoverability discussion](https://community.atlassian.com/forums/Jira-questions/Ticket-content-options-grouped-into-quot-Add-quot-button/qaq-p/2762609) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [continue.dev: Official homepage](https://continue.dev/) | Official product / project source | [EXECUTIVE_SUMMARY](EXECUTIVE_SUMMARY.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [culturedcode.com: Things support reference](https://culturedcode.com/things/support/articles/2803579/) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [culturedcode.com: Things scheduling guidance](https://culturedcode.com/things/support/articles/4001304/) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [cursor.com: computer use and artifacts](https://cursor.com/blog/agent-computer-use) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [cursor.com: automations](https://cursor.com/changelog/03-05-26) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [cursor.com: Cursor cloud agents](https://cursor.com/docs/cloud-agent) | Official product / project source | [BUSINESS_POSITIONING](BUSINESS_POSITIONING.md), [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [EXECUTIVE_SUMMARY](EXECUTIVE_SUMMARY.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [cursor.com: pricing](https://cursor.com/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [devin.ai: Devin Desktop](https://devin.ai/desktop) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [devin.ai: Devin pricing](https://devin.ai/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [docs.continue.dev: documentation](https://docs.continue.dev/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [docs.continue.dev: CLI](https://docs.continue.dev/cli/quickstart) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [docs.continue.dev: FAQ](https://docs.continue.dev/faqs) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [docs.devin.ai: billing distinctions](https://docs.devin.ai/admin/billing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [docs.openhands.dev: SDK](https://docs.openhands.dev/sdk) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [docs.warp.dev: terminal blocks](https://docs.warp.dev/terminal/blocks) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [dora.dev: DORA analysis](https://dora.dev/insights/balancing-ai-tensions/) | Research / survey; method-specific | [BUSINESS_POSITIONING](BUSINESS_POSITIONING.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [forum.cursor.com: new-user discussion](https://forum.cursor.com/t/questions-from-a-new/148900) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [forum.cursor.com: cloud-cost discussion](https://forum.cursor.com/t/what-is-the-pricing-structure-for-using-cloud-agents/156843) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: retry request](https://github.com/Aider-AI/aider/issues/2385) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: edit-format report](https://github.com/Aider-AI/aider/issues/3651) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: historical startup issue](https://github.com/OpenHands/OpenHands/issues/12684) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: intranet issue](https://github.com/OpenHands/OpenHands/issues/14862) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: Maintainer guidance](https://github.com/SWE-agent/SWE-agent/blob/main/docs/index.md) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: historical compaction issue](https://github.com/anthropics/claude-code/issues/27242) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: historical tool-performance issue](https://github.com/continuedev/continue/issues/5940) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: Copilot](https://github.com/features/copilot) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: plans](https://github.com/features/copilot/plans) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [github.com: historical authentication issue](https://github.com/warpdotdev/warp/issues/6929) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [github.com: historical issue](https://github.com/zed-industries/zed/issues/53189) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [githubnext.com: Historical product page](https://githubnext.com/projects/copilot-workspace/) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [help.superhuman.com: default Split Inbox](https://help.superhuman.com/hc/en-us/articles/46005619081101-Default-Split-Inbox) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [help.superhuman.com: custom Split Inbox](https://help.superhuman.com/hc/en-us/articles/46005636204941-Custom-Split-Inbox) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [linear.app: June 2026 launch](https://linear.app/changelog/2026-06-11-coding-sessions) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [linear.app: Linear coding sessions](https://linear.app/docs/coding-sessions) | Official product / project source | [BUSINESS_POSITIONING](BUSINESS_POSITIONING.md), [EXECUTIVE_SUMMARY](EXECUTIVE_SUMMARY.md), [FUTURE_ARCHITECTURE_ROADMAP](FUTURE_ARCHITECTURE_ROADMAP.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [linear.app: design reasoning](https://linear.app/now/behind-the-latest-design-refresh) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [linear.app: earlier redesign](https://linear.app/now/how-we-redesigned-the-linear-ui) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [linear.app: pricing](https://linear.app/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [manual.raycast.com: action panel](https://manual.raycast.com/action-panel) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [metr.org: Original study](https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/) | Research / survey; method-specific | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [metr.org: 2026 qualification](https://metr.org/blog/2026-02-24-uplift-update/) | Research / survey; method-specific | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [mini-swe-agent.com: mini-SWE-agent](https://mini-swe-agent.com/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [mini-swe-agent.com: environment options](https://mini-swe-agent.com/latest/advanced/environments/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [openai.com: Codex app](https://openai.com/index/introducing-the-codex-app/) | Official product / project source | [BUSINESS_POSITIONING](BUSINESS_POSITIONING.md), [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [EXECUTIVE_SUMMARY](EXECUTIVE_SUMMARY.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [openai.com: cross-device work](https://openai.com/index/work-with-codex-from-anywhere/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [resources.arc.net: Profiles](https://resources.arc.net/hc/en-us/articles/19227964556183-Profiles-Separate-Work-Personal-Browsing) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [resources.arc.net: Spaces](https://resources.arc.net/hc/en-us/articles/19228064149143-Spaces-Distinct-Browsing-Areas) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [support.claude.com: SDK plan guidance](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [survey.stackoverflow.co: 2025 developer survey](https://survey.stackoverflow.co/2025/ai) | Research / survey; method-specific | [BUSINESS_POSITIONING](BUSINESS_POSITIONING.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [agpt.co: current pricing](https://www.agpt.co/pricing/) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [atlassian.com: pricing and plan structure](https://www.atlassian.com/en/software/jira/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [creativerly.com: Contemporaneous report](https://www.creativerly.com/height-app-is-shutting-down/) | Secondary historical reporting | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [figma.com: Figma's UI3 design account](https://www.figma.com/blog/our-approach-to-designing-ui3/) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [jetbrains.com: New UI](https://www.jetbrains.com/help/idea/new-ui.html) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [jetbrains.com: Search Everywhere](https://www.jetbrains.com/help/idea/searching-everywhere.html?keymap=primary_default_for_windows) | Official product / project source | [RUNLITH_UI_UX_PRINCIPLES](RUNLITH_UI_UX_PRINCIPLES.md) |
| [linkedin.com: company's autonomous-product positioning](https://www.linkedin.com/posts/heightapp_unveiling-heightthe-first-ever-autonomous-activity-7241824769879277570-UlNn) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [notion.com: offline behavior](https://www.notion.com/help/use-pages-offline) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [notion.com: pricing](https://www.notion.com/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [openhands.dev: pricing](https://www.openhands.dev/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [raycast.com: 10 September 2026 announcement](https://www.raycast.com/blog/changing-how-raycast-ai-is-priced) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |
| [raycast.com: current pricing](https://www.raycast.com/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [reddit.com: custom-field discussion](https://www.reddit.com/r/Linear/comments/1tm85kg/add_customized_fields_into_linear/) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [reddit.com: database-performance discussion](https://www.reddit.com/r/Notion/comments/1srbq1w/notion_database_is_sluggish/) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [reddit.com: historical quota discussion](https://www.reddit.com/r/windsurf/comments/1s0mtx5/the_new_windsurf_quota_system_is_ridiculous_im/) | Community / reported experience | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [warp.dev: BYOK and pricing explanation](https://www.warp.dev/blog/warp-new-pricing-flexibility-byok) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [warp.dev: Warp Factories, early-access positioning](https://www.warp.dev/factories) | Official product / project source | [BUSINESS_POSITIONING](BUSINESS_POSITIONING.md), [EXECUTIVE_SUMMARY](EXECUTIVE_SUMMARY.md), [FUTURE_ARCHITECTURE_ROADMAP](FUTURE_ARCHITECTURE_ROADMAP.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [warp.dev: pricing](https://www.warp.dev/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [zed.dev: Zed external-agent ownership](https://zed.dev/docs/ai/external-agents) | Official product / project source | [CURRENT_ARCHITECTURE_ASSESSMENT](CURRENT_ARCHITECTURE_ASSESSMENT.md), [MARKET_ANALYSIS](MARKET_ANALYSIS.md) |
| [zed.dev: pricing](https://zed.dev/pricing) | Official product / project source | [MARKET_ANALYSIS](MARKET_ANALYSIS.md), [MONETIZATION_STRATEGY](MONETIZATION_STRATEGY.md) |

## 12. Package verification

The final package contains ten Markdown documents, including all seven explicitly named deliverables, a separate product strategy, this register and an index. Coverage checks confirmed seventeen requested market profiles and ten requested desktop UX references, with additional discussion of Codex and historical Copilot Workspace.

Verification included local link resolution, table-column consistency, heading/fence formatting, absence of internal browser citation tokens, whitespace/diff checks for every newly added file, and arithmetic checks for the business scenarios. The source catalog contains 83 unique external URLs; its entries are classified by evidence type.

The final scope review found only the ten new research documents in this checkout. Main was read, including its concurrent documentation addition, without edits by this study. No implementation, dependency, ADR, roadmap or queue changes were made. Runtime tests were not run because this deliverable is strategic documentation; historical test claims remain explicitly historical.
