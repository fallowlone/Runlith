# Architecture Invariants

> Repository-local canonical invariant registry, materialized from
> `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` (2026-09-09 baseline).
>
> **Changing an accepted invariant requires an ADR and explicit architecture review.**
>
> IDs are stable and never renumbered. Categories are navigation aids only.

## Task, execution lifecycle and durability

- **INV-001** — Task state survives every agent/session/process restart.
- **INV-002** — Task != Execution != Attempt != Agent Session.
- **INV-009** — Agent cannot mark its own task completed.
- **INV-010** — Persistent state mutations are auditable.
- **INV-020** — No context epoch is discarded before durable checkpoint.
- **INV-021** — Historical evidence remains retrievable after rollover.
- **INV-023** — Task may migrate between providers without state loss.
- **INV-043** — Planner output is a proposal; only validated application services may mutate the durable task graph.
- **INV-044** — Every runtime graph mutation creates a versioned plan revision with reason and provenance.
- **INV-045** — Recursive decomposition is bounded by depth, fan-out, node, time/token and no-progress limits.
- **INV-046** — Decomposition may not weaken parent acceptance criteria, security policy, scope or human-approval requirements.
- **INV-047** — Task lifecycle state is distinct from derived scheduling readiness.
- **INV-050** — Planning loops must terminate in executable work, an explicit blocker, or escalation; silent infinite replanning is a correctness failure.
- **INV-078** — Long-running agentd state is persisted incrementally; graceful shutdown is never the sole durability mechanism.
- **INV-140** — Security workflow state is durable and resumable at stage/attempt granularity; provider/process failure may not require repeating already verified stages when fingerprints still match.
- **INV-145** — Git/filesystem mutation and SQLite state are separate failure domains and require reconciliation; they are never represented as one ACID transaction.

## Provider neutrality and model routing

- **INV-004** — Provider adapters never mutate domain state directly.
- **INV-012** — Model/provider is not part of Task domain semantics.
- **INV-035** — Agent personality/role metadata never grants security or completion authority.
- **INV-048** — Task kinds are extensible routing/planning metadata and must not couple domain logic to specific providers.
- **INV-059** — Model/provider selection is a replaceable policy subsystem and never part of core Task domain identity.
- **INV-060** — Routing applies hard capability, security, privacy and resource eligibility gates before preference scoring.
- **INV-061** — Structured task metadata outranks prompt-text classification for routing decisions.
- **INV-062** — Global benchmark scores are evidence with provenance, not authoritative universal model quality.
- **INV-063** — Local/runtime measurements and estimated values must be distinguishable and retain hardware/backend/time provenance.
- **INV-064** — Device/VRAM/unified-memory management is isolated behind resource/residency ports and may not leak platform-specific logic into the router domain.
- **INV-065** — Semantic prompt similarity alone may never authorize cached response reuse for stateful agent/coding work.
- **INV-066** — Stateful failover creates a new/recovered execution attempt; side effects may not be transparently replayed across models/providers.
- **INV-067** — Routing decisions are explainable/replayable through versioned RoutePlan inputs, policy and profile references.
- **INV-068** — Verified repository-specific outcomes may inform future routing, but model self-report or self-judging never grants completion authority.
- **INV-084** — Every costly/multi-provider/privileged dispatch can produce an inspectable preflight with capabilities, context, privacy, capacity and cost estimates.
- **INV-085** — Provider availability/degradation is explicit runtime state; graceful degradation is allowed only when the workflow policy permits it.
- **INV-093** — Provider/model outputs are untrusted data even when produced by an authenticated local/external CLI.

## Planning, procedures, councils and multi-provider workflows

- **INV-074** — Known repeatable workflows prefer a deterministic Procedure Engine over repeated LLM planning.
- **INV-075** — Procedure executions are versioned, checkpointed, resumable and idempotency-aware, with Policy/approval gates.
- **INV-083** — Workflow templates/procedures may coordinate Tasks but never replace the durable Task lifecycle or acceptance authority.
- **INV-086** — Multi-model consensus never overrides deterministic failing evidence, mandatory security gates or human-approval requirements.
- **INV-087** — Material minority opinions and critical objections survive council synthesis with provenance.
- **INV-088** — Critical veto authority is typed and evidence-backed; no universal percentage threshold determines safety/completion.
- **INV-089** — Multi-model verification records actual provider/model/role contributions and may enforce provider/model diversity policies.
- **INV-090** — Role/persona/skill catalogs are versioned data/content packs and cannot introduce provider-specific branches into core Task/Context/Policy domain logic.
- **INV-091** — Reaction/retry policies are bounded, checkpoint-aware and observable through durable events; unbounded autonomous retry loops are forbidden.
- **INV-092** — A configured gate is not completion evidence; mandatory gates require runtime attestation that the expected version actually executed.

## Ports, adapters, modularity and replaceability

- **INV-025** — Bounded contexts do not read each other's storage directly.
- **INV-026** — Every infrastructure dependency is accessed through an explicit port.
- **INV-027** — Replacing one adapter must not require changing unrelated domain crates.
- **INV-069** — Stable extension contracts live in a deliberately small provider-neutral API/contracts layer.
- **INV-070** — Every persistent fact has one canonical owner; caches, snapshots and projections are explicitly derived.
- **INV-077** — Platform-specific sandbox implementations stay behind replaceable Runner/Sandbox contracts and declare the capabilities they actually enforce.
- **INV-082** — Adding/replacing a provider, tool, runner, memory backend or channel/integration must not require modifying unrelated domain business logic.
- **INV-094** — UI/MCP/IDE adapters remain thin control surfaces and cannot become alternative orchestration authorities.
- **INV-112** — Mechanism/engine code exposes capabilities; mutable business/security/routing/workflow policy is owned by explicit policy components and cannot be hidden in adapters/UI.
- **INV-141** — Security workflow/business services may not depend on a specific orchestration runtime such as Temporal; workflow engines are replaceable adapters/infrastructure.

## Context and continuity

- **INV-016** — Model context is a disposable materialized view.
- **INV-017** — Model-facing serialization format is not part of domain model.
- **INV-018** — Context selection is observable and reproducible.
- **INV-019** — Provider context is not durable application state.
- **INV-022** — Provider-native compaction is optimization, not sole persistence.
- **INV-030** — Context abstracts/summaries are derived; source evidence stays authoritative.
- **INV-031** — Retrieval preserves an observable selection trajectory.
- **INV-032** — Large tool outputs are content-addressed artifacts, not repeated prompt history.
- **INV-037** — Fixed-size token chunks are derived retrieval units, not authoritative semantic structure.
- **INV-039** — Original/source evidence remains authoritative over display/model/summary representations.
- **INV-049** — Durable task/work graphs are retrieval sources, not prompts; Context Engine selects graph-local evidence just in time.
- **INV-071** — Context pruning/compaction preserves provider-valid tool-call/tool-result correlation and provenance.
- **INV-079** — AGENT.md remains a compact bootstrap contract; an Architecture Map provides progressive-disclosure routing to detailed source-of-truth docs.
- **INV-115** — Whole-repository prompt packing is an explicit bounded fallback; default context construction uses structural just-in-time retrieval.

## Skills, memory and learned annotations

- **INV-028** — Skills are lazy-loaded; full skill bodies are not baseline context.
- **INV-029** — Agent-generated annotations/learned notes are untrusted until validated.

## Resources and document processing

- **INV-038** — Imported/user/external resource content is untrusted data and never gains instruction authority automatically.
- **INV-040** — Resource parsing/normalization is isolated from Context selection behind replaceable contracts.
- **INV-041** — Persistent user-controlled resources must not use deserialization mechanisms capable of arbitrary code execution.
- **INV-042** — UI/display sanitization and model-context normalization are separate pipelines.
- **INV-117** — Vision OCR is a document-processing strategy/fallback, not the canonical ingestion path for every structured document.
- **INV-118** — Document parsing/conversion/inference implementations are adapters behind document-processing ports; their library/provider types may not leak into Resource/Task/Context domain models.
- **INV-119** — Untrusted document conversion/rendering executes in a restricted document worker with explicit resource and filesystem/network boundaries.
- **INV-120** — User-controlled filenames, URLs, MIME declarations and extensions must never be interpolated into shell commands; native converters are invoked with fixed executable + argument arrays.
- **INV-121** — Remote document URLs are acquired through the centralized Web Acquisition/SSRF policy before conversion; document parsers may not perform arbitrary network fetches themselves.
- **INV-122** — Temporary document workspaces are unique and application-owned; cleanup may never recursively delete arbitrary user-selected paths.
- **INV-123** — Large document processing is page/unit durable and resumable; one failed provider call must not require repeating already verified units.
- **INV-124** — Cross-page continuity uses bounded derived carry-state rather than unbounded prior-page transcript accumulation.
- **INV-125** — Markdown/OCR text is a derived representation with provenance and quality, never the sole canonical representation of the document.
- **INV-126** — Schema-constrained extraction must be structurally validated and remain evidence-bearing derived data; schema validity alone is not factual verification.
- **INV-127** — Failed/partial/degraded document units remain explicit in aggregate state and may not be silently omitted.
- **INV-128** — Document model/provider selection is capability/policy/evidence driven and independent from document domain semantics.
- **INV-129** — Document jobs enforce bounded concurrency, retries, token/cost/time/disk budgets and cancellation/backpressure semantics.
- **INV-130** — External document inference must be privacy-observable: execution plans identify what representations/pages leave the device and which provider receives them.

## Web acquisition and browser automation

- **INV-033** — Browser automation is isolated behind a controller/port and browser-neutral.
- **INV-095** — Web acquisition, browser automation, resource normalization and context selection are separate bounded responsibilities.
- **INV-096** — Browser/CDP/Playwright-specific types never enter core Task/Context/Memory domain contracts.
- **INV-097** — Every web acquisition and redirect hop is subject to centralized network/SSRF/policy evaluation before access.
- **INV-098** — Cleaned Markdown, extracted text and browser DOM remain untrusted external evidence; cleaning never elevates instruction authority.
- **INV-099** — Browser sessions/cookies/auth storage are scoped credentials/capabilities and may not inherit unrestricted host secrets or filesystem access.
- **INV-100** — Extraction fallbacks are explicitly marked degraded/partial and never silently treated as equivalent structured representations.
- **INV-101** — Web retries/crawls are bounded by typed retry policy, time/page/byte/concurrency budgets and idempotency/session semantics.
- **INV-102** — Anti-detection/stealth is an optional adapter capability governed by policy, never a core requirement or authorization bypass mechanism.
- **INV-103** — Fetched web content is stored/referenced as provenance-bearing snapshots with explicit freshness semantics when reproducibility or reuse matters.
- **INV-104** — Browser actions that materially affect execution can emit correlated receipts/artifacts and remain auditable independently of model claims.
- **INV-105** — Browser process pooling/health/recycling is runtime adapter infrastructure and cannot become an alternative owner of durable Task state.
- **INV-106** — Web/proxy/site credentials are represented by secret references and never embedded in ordinary task/config/event/log payloads.

## Security, isolation, secrets and execution evidence

- **INV-003** — Frontend never executes system commands directly.
- **INV-005** — Autonomous executions use isolated workspaces/worktrees.
- **INV-006** — Agents never write directly to main/master.
- **INV-007** — Destructive/privileged actions pass Policy Engine.
- **INV-008** — Secrets never enter model context, logs or event payloads.
- **INV-013** — Prompt instructions are never the only security control.
- **INV-036** — Copyleft/AGPL implementations require explicit license review before bundling/linking.
- **INV-072** — Successful privileged/tool executions can emit runtime-authenticated receipts; a receipt proves execution provenance, not authorization or correctness.
- **INV-073** — Durable security/audit evidence is separate from model-visible session receipt mechanics.
- **INV-076** — Required isolation fails closed; sandbox backend unavailability may not silently downgrade to unsandboxed execution.
- **INV-131** — Security hypotheses, analytical risks and live-proven exploits are distinct states; inability to reproduce an exploit may not silently erase the underlying analytical risk.
- **INV-132** — Dynamic exploit verification executes only against an explicitly authorized, target-bound environment and defaults to fail-closed for production targets.
- **INV-133** — Rules of Engagement that must be enforced are represented as typed Policy Engine constraints; free-form prose alone is never an enforcement boundary.
- **INV-134** — Security run/checkpoint state is bound to target identity and relevant repository/build scope; incompatible resumes fail closed.
- **INV-135** — A ProvenExploit requires runtime evidence/receipts/artifacts tied to the exact target/build and may not be created from an LLM narrative alone.
- **INV-136** — Security CI/release policy distinguishes incomplete, blocked, analytical-risk and proven-exploit states; hypotheses alone may not masquerade as proven findings.
- **INV-137** — Static/code-analysis and runtime/recon findings are reconciled/deduplicated with provenance before expensive exploit scheduling.
- **INV-138** — Security testing that may mutate target state runs through a dedicated restricted Security Runner/worker boundary with bounded credentials, network, rate, cost and destructive-action policy.
- **INV-139** — Canonical security findings remain provider/runtime/CI-format independent; SARIF/JSON/PDF/Markdown are output representations/adapters.
- **INV-142** — Parallel security work must respect target-state interference, rate limits, account/session conflicts and policy; availability of multiple agents never authorizes unsafe concurrency.
- **INV-143** — Security source/web/payload content remains untrusted evidence and cannot acquire system/policy authority through inclusion in a pentest context.
- **INV-144** — High-impact security findings preserve human-review capability and explicit uncertainty; autonomous exploitation does not imply infallible severity or remediation.

## Specifications, traceability and verification

- **INV-107** — Product intent, requirements, architecture constraints, task execution and verification are distinct layers; a single PRD may not silently own all of them.
- **INV-108** — Every material requirement/change must be traceable to the tasks and verification evidence claiming to satisfy it.
- **INV-109** — Tasks reference a concrete versioned ChangeSpec/requirement revision; material spec changes require explicit reconciliation.
- **INV-110** — Raw prompt/chat history is optional historical evidence, not authoritative project state and not automatically version-controlled.
- **INV-111** — Important design intent from conversations must be promoted into typed decisions/specifications/ADRs with provenance rather than depending on transcript recall.
- **INV-113** — Verification is mandatory, but its technique is risk/context dependent; TDD is a method, not the source of truth for completion.
- **INV-114** — An implementation agent may not weaken tests/evaluators/acceptance criteria used to approve its own task unless the task explicitly authorizes that change and independent review validates it.

## Local-first, privacy, data control, UI and documentation

- **INV-011** — Cloud is optional for core functionality.
- **INV-014** — Core product works without telemetry.
- **INV-015** — User can delete all app-owned local project data.
- **INV-024** — UI primitives do not define the product design system.
- **INV-034** — PRODUCT.md and DESIGN.md are separate sources of product vs visual truth.
- **INV-051** — Generated documentation is derived state and never outranks its authoritative source evidence by default.
- **INV-052** — Human-curated documentation must not be silently overwritten by AI regeneration.
- **INV-053** — Every generated documentation artifact must retain sufficient provenance to explain what evidence produced it.
- **INV-054** — Documentation refresh is incrementally freshness-aware; full repository regeneration is not the default update strategy.
- **INV-055** — Repository/resource content may not be promoted into system instruction authority during documentation generation.
- **INV-056** — Diagram/editor libraries are replaceable renderers; visual representations are not domain state.
- **INV-057** — Generated documentation used as agent context remains secondary evidence and must not create recursive summary-as-truth feedback loops.
- **INV-058** — Documentation cloud synchronization is opt-in and independent from the local documentation engine.
- **INV-080** — Runtime performance claims require reproducible benchmark methodology and hardware/build provenance.
- **INV-081** — Rust crate/module growth is monitored by architecture fitness checks; oversized components require decomposition or an explicit reviewed exception.
- **INV-116** — Provider-specific agent/rules files are derived compatibility views of canonical project policy and must support provenance/drift detection.
