# External Repository Research — Adoption Decisions for AI Engineering OS

> **Snapshot:** 2026-09-10  
> **Purpose:** evaluate external repositories/papers supplied during the project and decide what should influence AI Engineering OS.  
> **Rule:** external repositories are research material only. We adopt independently evaluated **architecture, concepts, patterns, and lessons**; we use mistakes, failures, insecure designs, and bad ideas only as negative evidence. We **never copy source code** from reviewed repositories and **never add a reviewed repository/codebase as a dependency to reuse its implementation**. Any useful concept is reimplemented independently under AI Engineering OS contracts, invariants, tests, security boundaries, and coding conventions. A genuinely necessary generic third-party library must be evaluated separately for license, maintenance, security, supply-chain risk, necessity, and replaceability. Popularity is not an architecture argument.

## External-research doctrine

For every external repository review:

1. Extract architecture, interfaces, invariants, product ideas, operational lessons, and failure modes.
2. Keep only ideas that improve AI Engineering OS after independent evaluation.
3. Record weak designs and failures as things to avoid, not things to reproduce.
4. Do not copy code, patches, implementation fragments, or project-specific internals.
5. Do not vendor, fork, or depend on the reviewed project as an implementation shortcut.
6. Reimplement selected ideas independently so the resulting design is native to AI Engineering OS.
7. Evaluate any unrelated generic third-party dependency separately; external-repo research is never itself justification to add that dependency.

This rule exists both to preserve architectural independence and to minimize license/IP/supply-chain exposure.

## Executive decision

The list is genuinely useful, but not all projects are equally relevant.

### Adopt into the architecture now

1. **OpenViking** — hierarchical L0/L1/L2 context resources, observable retrieval trajectories, asynchronous session-to-memory consolidation.
2. **Superpowers** — atomic implementation plans, test-first behavior where meaningful, spec-compliance review before code-quality review, worktree discipline, lazy skills.
3. **Context Hub** — versioned curated docs, incremental fetch, local annotations treated as untrusted by default.
4. **DeepAgents** — isolated subagent contexts, large tool-output offload, pluggable backends, skills loaded on demand, human approval at tool boundary.
5. **Impeccable** — separate durable product truth from visual design truth and use deterministic UI/design quality detectors.
6. **Agency Agents / Agency Agents App** — explicit role contracts, measurable deliverables/success metrics, deterministic provider-specific rendering, local install ledger/drift detection.
7. **Page Agent** — separate browser/controller operations from LLM reasoning; prefer compact DOM/text representation and indexed deterministic actions when visual reasoning is unnecessary.
8. **WriteHERE** — bounded just-in-time recursive decomposition, versioned adaptive plan revisions, heterogeneous task classes, graph-scoped context and explicit anti-recursion/liveness guardrails.

### Defer, but design an extension point now

9. **Lightpanda** — future browser backend via CDP/BiDi adapter; excellent performance characteristics, but compatibility gaps and AGPL considerations mean it should not become a hard core dependency.
10. **BitNet** — future local utility-model backend for privacy/cheap classification, reranking, summarization or embeddings; not a primary coding agent for v1.

### Research only / do not integrate into v1

11. **MiroFish** — useful inspiration for simulation, population-level testing and temporal/collective memory, but its prediction-swarm architecture is not the product's execution model. AGPL and heavy simulation dependencies are additional reasons not to integrate it.

### Later high-value reviews incorporated into the architecture

- **microsoft/tgrep** — indexed lexical retrieval patterns, snapshot + live overlay, reconciliation and bounded indexes; primary placement G9.
- **Graphify-Labs/graphify** — revision-bound structural graph retrieval, provenance, blast-radius analysis and incremental-index integrity; primary placement G9.
- **juliusbrussee/caveman** — recoverable shape-aware context/tool-output compression, cache-aware optimization and measurement provenance; primary placement G9/Usage Ledger.
- **dietrichgebert/ponytail** — work avoidance, reuse-first solution minimality, complexity review and deliberate simplification with upgrade triggers; placement across Planner/ChangeSpec/Review Packet and later efficiency learning.

These reviews reinforce one combined production principle: optimize **verified delivery efficiency**, not a single token/LOC metric.

---

# 1. Validating the supplied “trending” text

The supplied weekly-growth numbers cannot be verified from a repository README alone. GitHub exposes current totals, while a claim such as `+23.2K this week` needs a historical star series from a third-party tracker or captured snapshots.

For architecture decisions, this research therefore uses:

- current repository state;
- documented architecture/features;
- licenses;
- security/privacy posture;
- direct relevance to our product.

Approximate GitHub totals visible during this review:

| Repository | Approx. stars | License | Relevance |
|---|---:|---|---|
| `obra/superpowers` | 283k | MIT | Very high |
| `msitarzewski/agency-agents` | 151k | MIT | High |
| `666ghj/MiroFish` | 72k | AGPL-3.0 | Medium/Research |
| `pbakaus/impeccable` | 66k | Apache-2.0 | Very high for UI workflow |
| `microsoft/BitNet` | 39k | MIT | Future/local inference |
| `volcengine/OpenViking` | 36k | Mixed; main AGPL-3.0 | Very high conceptually |
| `lightpanda-io/browser` | 35k | AGPL-3.0 | High future browser backend |
| `langchain-ai/deepagents` | 29k | MIT | Very high conceptually |
| `alibaba/page-agent` | 28k | MIT | High future browser/UI agent |
| `andrewyng/context-hub` | 14k | MIT | Very high context/docs |

Star counts are a snapshot and will change.

---

# 2. `msitarzewski/agency-agents`

Repository: https://github.com/msitarzewski/agency-agents  
Related native app: https://github.com/msitarzewski/agency-agents-app  
License: MIT.

## What is valuable

The core repo is not really an orchestration engine. Its main strength is **role specification**: specialized agents are described with a mission, workflow, concrete deliverables, success metrics, memory/learning expectations and communication conventions.

The companion Tauri app is more architecturally relevant to us. It uses deterministic per-tool renderers, tracks what it wrote in a local install ledger, detects drift when files are modified outside the app, stores tokens in the platform keychain, avoids arbitrary frontend shell execution, and keeps a tool registry as a single source of truth.

## Adopt

### A. Agent Definition Contract

Our future agent profiles should not be loose persona prompts. Use a typed definition such as:

```text
AgentDefinition
├── id / version
├── role
├── mission
├── required_capabilities
├── allowed_tool_profile
├── input_contract
├── output_contract
├── workflow_refs
├── success_metrics
├── escalation_rules
├── review_requirements
├── memory_policy
└── presentation metadata (optional voice/name/icon)
```

The important transfer from Agency Agents is **measurable work contracts**, not fictional personality.

Personality may affect UX/tone, but must never influence security, policy, architecture authority or Definition of Done.

### B. Deterministic provider rendering

If our product later installs skills/agent definitions into Claude/Codex/OpenCode-native locations, create:

```text
Canonical Agent/Skill Definition
        ↓
ProviderRendererPort
        ├── ClaudeRenderer
        ├── CodexRenderer
        └── OpenCodeRenderer
```

Renderers must be deterministic and contract-tested.

### C. Install ledger + drift reconciliation

If we write provider-native files outside our own state directory, keep a ledger:

```text
managed artifact
├── destination
├── content hash
├── renderer version
├── source definition version
└── last reconciliation
```

Never overwrite a user-modified file blindly.

## Do not adopt

- Huge persona prose in every execution context.
- “Personality” as a substitute for permissions/capabilities.
- 200+ agents preloaded into context.

## Priority

**P1 for Agent Profile schema. P2 for provider-native install/rendering.**

---

# 3. `obra/superpowers`

Repository: https://github.com/obra/superpowers  
License: MIT.

## What is valuable

Superpowers formalizes a repeatable software-development workflow for coding agents: small plans with exact file paths and verification steps, worktrees, subagent-driven development, test-driven implementation, systematic debugging, review between tasks, and verification before completion.

Its strongest principle for us is **evidence over claims**.

## Adopt

### A. Skills as lazy reusable workflows

Add a future `SkillRegistry`/`SkillResolver` behind ports. Base context contains only skill metadata/descriptions. Full skill content is loaded only when matched to the task.

A skill is subordinate to:

```text
Task Scope
Architecture Invariants
Policy Engine
Accepted ADRs
```

### B. Two-stage review

Every meaningful implementation should support two independent review stages:

1. **Spec compliance** — did the change implement exactly the task and acceptance criteria without scope expansion?
2. **Code quality/security** — is the implementation maintainable, safe and idiomatic?

This is better than one broad “review this PR” prompt.

### C. Atomic plans

Our task generator should prefer small tasks with:

- exact allowed paths;
- explicit forbidden paths;
- prerequisites;
- acceptance criteria;
- exact verification;
- output contract.

### D. TDD — with one modification

Use RED → GREEN → REFACTOR by default for behavior and bugs.

Do **not** make TDD dogmatic for pure scaffolding/configuration where a fabricated test adds no evidence. There, deterministic build/contract verification is better.

## Do not adopt

- Any skill being able to override system policy.
- Blindly spawning fresh subagents for every trivial operation.
- Methodology as runtime business logic; it should remain composable workflow policy.

## Priority

**P0 — use these rules during development immediately.**

---

# 4. `666ghj/MiroFish`

Repository: https://github.com/666ghj/MiroFish  
License: AGPL-3.0.

## What is valuable

MiroFish builds a graph from seed information, generates large populations of agents/personas, injects individual/collective memory, runs simulations with temporal memory updates, then produces reports from the simulated environment.

This is a different problem from our execution engine, but two ideas are useful.

## Adapt later

### A. Orchestration Simulation Lab

Instead of testing only three real LLM agents, create a future synthetic simulation harness using `FakeAgent` populations:

```text
1,000 synthetic agents
    ↓
controlled latency/failure/quota patterns
    ↓
Scheduler/Reconciler
    ↓
metrics: deadlocks, retries, fairness, recovery, queue pressure
```

This gives us swarm-scale testing without spending LLM tokens.

### B. Temporal/collective state experiments

For future team memory research, distinguish:

- individual agent experience;
- task/team shared knowledge;
- project-wide durable decisions.

This maps better to our existing memory channels than a single global memory store.

## Do not adopt

- MiroFish as an internal dependency.
- GraphRAG as default memory architecture.
- Thousands of LLM agents as a product primitive.
- Prediction/simulation ontology in our Task domain.

## Licensing

Because the main project is AGPL, do not copy/link its implementation into a closed commercial core without an explicit licensing strategy. Concepts can be independently implemented.

## Priority

**P3 research / test-lab inspiration only.**

---

# 5. `volcengine/OpenViking`

Repository: https://github.com/volcengine/OpenViking  
License: main project AGPL-3.0; selected CLI/examples components use Apache-2.0.

## Why this is the most important repo in the list for our Context Engine

OpenViking treats memory/resources/skills like a browsable context filesystem and preprocesses entries into three levels:

- **L0** — tiny abstract for relevance decisions;
- **L1** — overview for planning;
- **L2** — full source/details loaded only when needed.

It also records the retrieval trajectory and turns committed sessions into long-term memory asynchronously.

This strongly validates our existing progressive-disclosure design.

## Adopt now

### A. `ContextResource` with layered representations

Add a model like:

```text
ContextResource
├── stable resource URI
├── kind
├── provenance
├── content hash/version
├── L0 abstract
├── L1 overview
└── L2 source reference/full content
```

L0/L1 are derived and rebuildable. L2/source remains authoritative.

### B. Stable internal context URIs

Use a provider-neutral internal namespace, e.g.:

```text
ctx://project/{project_id}/resources/...
ctx://project/{project_id}/memory/...
ctx://project/{project_id}/skills/...
ctx://task/{task_id}/artifacts/...
```

This is a reference scheme, not necessarily a mounted OS filesystem.

### C. Observable retrieval trajectory

Every Context Plan should record:

```text
query
→ candidate directories/resources
→ ranking signals
→ L0 inspected
→ L1 inspected
→ L2 selected
→ final context item
```

This enables debugging, privacy inspection and later policy learning.

### D. Async memory consolidation

After a task/session is durably committed:

```text
raw events/evidence
   ↓
MemoryConsolidator
   ↓
proposed memory
   ↓
validation/dedup/provenance
   ↓
durable memory
```

Never let session summarization mutate authoritative task state.

## Do not adopt blindly

- Do not make a vector database the truth layer.
- Do not depend on OpenViking main AGPL code in core.
- Do not expose its exact `viking://` namespace; use our own neutral scheme.

## Priority

**P0/P1 — incorporate into Context Engine specification before implementation.**

---

# 6. `lightpanda-io/browser`

Repository: https://github.com/lightpanda-io/browser  
License: AGPL-3.0.

## What is valuable

Lightpanda is a browser built for machine/headless use rather than a Chromium fork. It exposes CDP/BiDi compatibility, can dump pages to Markdown, and its published benchmark shows substantially lower memory and faster execution than headless Chrome. Its native agent can also produce deterministic replayable JavaScript automation scripts.

## Adopt architecturally

### A. Browser backend as a replaceable port

Future design:

```text
BrowserAutomationPort
├── Chromium/Playwright adapter
├── Lightpanda adapter
└── future native/browser adapters
```

No browser engine becomes domain logic.

### B. Prefer semantic/text extraction when possible

For many web tasks:

```text
DOM / accessibility state
    ↓
compact text/markdown representation
    ↓
LLM
```

is cheaper and more deterministic than screenshots.

Use screenshots/vision only when visual layout actually matters.

### C. Compile successful AI browsing into deterministic replay

Future strategic feature:

```text
LLM explores once
   ↓
validated browser action script
   ↓
replay without LLM tokens
```

This is a powerful general pattern, not limited to Lightpanda.

## Risks / why not core-v1

- Web API/CDP coverage remains incomplete compared with Chromium.
- Native Windows support is not equivalent to Chrome today.
- AGPL requires a deliberate distribution/integration strategy.

Do not bundle it by default until license and compatibility policy are settled. A user-installed sidecar adapter is the safer initial path.

## Priority

**P2 future Browser Tool.**

---

# 7. `pbakaus/impeccable`

Repository: https://github.com/pbakaus/impeccable  
License: Apache-2.0.

## What is valuable

Impeccable separates durable product truth (`PRODUCT.md`) from visual design truth (`DESIGN.md`), provides an explicit design workflow, and includes deterministic detectors for common AI-generated UI problems in addition to subjective LLM critique.

This directly supports our belief that design is part of product/marketing quality.

## Adopt now

### A. Add `PRODUCT.md`

Contains durable product facts:

- audience;
- core job-to-be-done;
- trust/privacy positioning;
- user skill level;
- main workflows;
- non-goals;
- product language/voice constraints.

Do not put pixel-level visual decisions here.

### B. Add `DESIGN.md`

Contains:

- visual principles;
- density/hierarchy;
- typography;
- spacing;
- motion;
- color semantics;
- component patterns;
- interaction rules;
- accessibility baseline;
- screenshot/marketing quality requirements.

### C. Deterministic design gate

Add a replaceable design-quality check to frontend CI/development:

```text
rendered UI
  ↓
static/a11y/design detectors
  ↓
visual regression
  ↓
optional LLM critique
```

We may use Impeccable directly as a development tool initially, but our product architecture must not depend on it.

## Priority

**P0 for project development workflow.**

---

# 8. `alibaba/page-agent`

Repository: https://github.com/alibaba/page-agent  
License: MIT.

## What is valuable

Its internal separation is especially relevant:

```text
LLM/Core Agent
      ↓
Page Controller interface
      ↓
DOM extraction / indexed actions
```

The controller does not depend on the LLM, while the LLM reasons over a simplified representation and invokes deterministic page actions.

The repo also explicitly favors traceability/predictability and keeps client-side BYOK privacy clear.

## Adopt for future browser/UI automation

### A. `BrowserController` independent of LLM

The model should never directly manipulate a webview object.

```text
Agent Runtime
   ↓ typed tool request
Browser Tool Service
   ↓
BrowserControllerPort
   ↓
Browser backend
```

### B. Indexed/symbolic actions

Use stable element/action references within one page snapshot rather than asking the model to repeatedly reconstruct selectors.

### C. Compact DOM representation

Introduce browser-specific Context Renderer capable of:

- accessibility tree / semantic DOM;
- interactive elements;
- current URL/title/state;
- bounded text content;
- explicit page version/snapshot ID.

### D. Permission token for browser control

Remote/extension control must have its own scoped authorization; never equate “can talk to the app” with “can control all browser tabs”.

## Do not adopt

- Client-side API keys embedded into production web scripts.
- Browser controller state inside the general Task domain.

## Priority

**P2 future browser subsystem.**

---

# 9. `andrewyng/context-hub`

Repository: https://github.com/andrewyng/context-hub  
License: MIT.

## Why it matters

Context Hub solves a common coding-agent failure: using stale or hallucinated API knowledge. It provides curated, versioned, language-specific Markdown docs, incremental fetch, and persistent local annotations.

Crucially, annotations are treated as **untrusted input and excluded by default**. This is an excellent security pattern for agent memory.

## Adopt now

### A. `DocumentationSourcePort`

Context Engine should be able to ask a source for documentation matching:

```text
package/framework
version
language
requested topic/file
```

Possible adapters later:

- repository local docs;
- Context Hub CLI/API;
- provider official docs;
- cached verified docs.

### B. Dependency-aware documentation resolution

When researching an API, derive the desired version from lockfiles/config where possible rather than retrieving generic latest docs.

### C. Incremental documentation fetch

Fetch index/overview first; specific reference files next; full documentation only as a last resort.

### D. Untrusted annotations

Any agent/user annotations to external docs must carry:

```text
trust = untrusted
source
created_by
created_at
validation status
```

They are never silently injected into high-priority instructions.

### E. Feedback loop

Track whether a context resource helped or hurt successful execution. This can later train our Context Policy without retraining the model.

## Priority

**P0/P1 for Context Engine and memory trust model.**

---

# 10. `langchain-ai/deepagents`

Repository: https://github.com/langchain-ai/deepagents  
License: MIT.

## What is valuable

DeepAgents combines long-horizon planning, isolated subagents, pluggable filesystems/backends, persistent memory, context summarization, disk offload of large tool results, human approval, and lazily loaded skills. It explicitly recommends enforcing security at tool/sandbox boundaries rather than trusting the model.

That is very close to our target principles.

## Adopt now

### A. Large tool-result offload

Do not keep massive command/browser/test output in conversation history.

Introduce a generic result form:

```text
ToolResult
├── short inline summary
├── artifact_ref
├── content_hash
├── byte_size
├── token_estimate
├── mime/type
└── provenance
```

Full data goes to Artifact Store; context gets only the summary + handle until exact data is requested.

### B. Isolated subagent context

Each delegated task gets a new Context Plan derived from its own scope. Do not clone the supervisor's full conversation into every worker.

### C. Pluggable execution/file backends

This validates our Runner/Workspace ports. Keep local/container/remote storage and execution replaceable.

### D. Human-in-the-loop belongs at action boundary

Approvals apply to concrete actions/capabilities, not vague “are you sure?” prompts.

## Do not adopt

- LangGraph as our core runtime; we intentionally use Rust and our own domain model.
- Python/JS framework objects in domain contracts.

## Priority

**P0/P1 concepts.**

---

# 11. `microsoft/BitNet`

Repository: https://github.com/microsoft/BitNet  
License: MIT.

## What is valuable

`bitnet.cpp` is an efficient inference runtime for ternary/1-bit models. Microsoft currently ships an official 2.4B model and has also released small BitNet embedding models. This makes the project relevant to local-first, privacy-sensitive helper workloads.

## Future use in AI Engineering OS

Do not make BitNet a first-class requirement. Instead keep a generic:

```text
LocalInferencePort
```

Potential low-risk utility tasks:

- context relevance classification;
- cheap summarization/abstraction generation (L0/L1) after evaluation;
- log/event categorization;
- offline metadata extraction;
- embeddings/reranking where platform support is adequate;
- privacy mode for non-critical helpers.

Never give a small local utility model independent authority over:

- security approvals;
- destructive commands;
- architecture changes;
- final Definition of Done;
- critical code review.

## Current limitation for our Mac-first product

The BitNet ecosystem is promising, but model/platform coverage is still less mature than mainstream llama.cpp/Ollama ecosystems. It should be one optional adapter, not “the local AI architecture”.

## Priority

**P3; revisit after Context/Memory Engine exists.**

---

# 12. Architecture changes that should be made to our Source of Truth

## 12.1 New/strengthened concepts

### `ContextResource`

```text
id
uri
kind
source_ref
version/content_hash
trust
sensitivity
L0 abstract
L1 overview
L2 source/detail ref
provenance
```

### `RetrievalTrace`

Records why each resource was traversed/selected/rejected.

### `DocumentationSourcePort`

Provider-neutral access to versioned external technical docs.

### `ArtifactStorePort`

Stores large tool results once and returns stable content-addressed references.

### `SkillRegistryPort`

Provides on-demand reusable workflows; only metadata is visible until a skill is loaded.

### `AgentDefinition`

Typed role/deliverable/success/escalation contract rather than a giant persona prompt.

### `BrowserAutomationPort` — future

Provider-neutral browser automation. Chromium is compatibility baseline; Lightpanda/Page-Agent-like adapters are optional.

### `LocalInferencePort` — future

Optional local models for utility workloads.

---

# 13. New architecture invariants recommended

```text
INV-028 Skills are lazy-loaded; full skill bodies are not baseline context.

INV-029 Agent-generated annotations and learned notes are untrusted until
        provenance/validation policy promotes them.

INV-030 Context summaries/abstracts are derived representations; original
        source/evidence remains authoritative.

INV-031 Context retrieval must preserve an observable selection trajectory.

INV-032 Large tool outputs are content-addressed artifacts, not repeatedly
        embedded in model conversation history.

INV-033 Browser automation logic is isolated behind a controller/port and is
        not coupled to a specific LLM or browser engine.

INV-034 Durable product truth and visual design truth are separate documents
        (`PRODUCT.md` and `DESIGN.md`).

INV-035 Agent role/personality metadata never grants capabilities, permissions,
        architecture authority, or completion authority.

INV-036 AGPL/copyleft external implementations are not copied, linked or bundled
        into proprietary core without an explicit license architecture review.
```

---

# 14. Repository additions recommended

```text
/
├── PRODUCT.md
├── DESIGN.md
├── AGENT.md
├── docs/
│   └── research/
│       └── external-projects-adoption.md
├── skills/                       # later; lazy-loadable workflows
│   └── INDEX.md
└── crates/
    ├── context/                  # ContextResource + planning ports
    ├── tools/                    # Tool registry/result contracts
    └── ...
```

Do not create all future modules before a task actually needs them.

---

# 15. What should affect the first implementation tasks

Immediately:

1. Use the `AGENT.md` development protocol.
2. Keep tasks atomic and exact-path scoped.
3. Require evidence/verification rather than self-reported completion.
4. Plan for `ContextResource` L0/L1/L2 before implementing Context Engine.
5. Plan Artifact Store handles before allowing huge tool outputs into history.
6. Treat all stored annotations/memory as data with explicit trust/provenance.
7. Create `PRODUCT.md` and `DESIGN.md` before substantial UI work.

Do **not** delay G1 for browser automation, swarm simulation, BitNet, marketplace or remote agents.

---

# 16. Source index

Primary sources reviewed:

- Agency Agents: https://github.com/msitarzewski/agency-agents
- Agency Agents App: https://github.com/msitarzewski/agency-agents-app
- Superpowers: https://github.com/obra/superpowers
- MiroFish: https://github.com/666ghj/MiroFish
- OpenViking: https://github.com/volcengine/OpenViking
- Lightpanda: https://github.com/lightpanda-io/browser
- Impeccable: https://github.com/pbakaus/impeccable
- Page Agent: https://github.com/alibaba/page-agent
- Context Hub: https://github.com/andrewyng/context-hub
- DeepAgents: https://github.com/langchain-ai/deepagents
- BitNet: https://github.com/microsoft/BitNet

License-specific note: this document is an architectural research summary, not legal advice. Before bundling or linking third-party code into a commercial distribution, verify the exact component/version license and distribution model again.

---

# 17. reader3 (karpathy/reader3)

Decision: **ADAPT — high conceptual value, low direct-code value.**

`reader3` is useful as a compact demonstration of AI-friendly resource handling rather than as a production architecture to reuse. The project separates an EPUB's logical reading structure from its physical files/spine, exposes content one meaningful unit at a time, and derives both display-oriented and plain-text representations from the same source. Those ideas reinforce the project's Context/Resource architecture.

## 17.1 Adopt / adapt

### Semantic Resource Segmentation

Do not make fixed token chunks the primary knowledge unit. Parse the strongest available semantic structure first and fall back only when necessary.

Examples:

```text
Code       Repository → Package → File → Type → Function/Method
Markdown   Document → Heading → Section
EPUB       Book → TOC/Spine → Chapter → Section
PDF        Document → Chapter/Section → Page/Table/Figure fallback
```

A chunk is a derived transport/retrieval representation, not authoritative structure.

### Logical structure != physical storage

Resources may have both physical and logical graphs. Parsers must preserve the distinction rather than flattening everything into file order.

### Multiple representations per resource

A resource may expose several derived representations:

```text
Resource
├── Original
├── Display
├── PlainText
├── Markdown
├── ModelOptimized
├── SummaryL0
└── SummaryL1
```

The original/source representation remains authoritative. Derived representations carry source references, parser/renderer version and content hash.

### Preprocess once, retrieve many

Parsing, normalization, structural extraction and derived-representation generation happen during ingestion or invalidation, not repeatedly for every model request.

### Progressive structural disclosure

Retrieve the smallest meaningful semantic unit first; expand only when the task requires more context.

### Parser fallback with confidence

Parsers may fall back from authoritative structure to inferred/heuristic structure, but the system must expose the confidence/source of that structure.

Target concept:

```rust
enum StructuralConfidence {
    Authoritative,
    Parsed,
    Inferred,
    Fallback,
}
```

### Imported content is untrusted

EPUB, Markdown, PDF, HTML, repository text and any other user/external resource are data, not instructions. They may contain prompt-injection strings and must never gain authority merely because they entered model context.

### Separate UI and model sanitization pipelines

Display HTML and model-readable text are different representations and require independent processing. A UI sanitizer must use an allowlist, remove executable/event-handler content and validate URL schemes. Model normalization must preserve evidence/provenance without converting resource text into system instructions.

## 17.2 New bounded context: Resource Engine

Resource ingestion/normalization should be separated from Context selection.

```text
Input Sources
    ↓
Resource Engine
    ├── ingest
    ├── parse
    ├── normalize
    ├── semantic segmentation
    ├── representation generation
    ├── provenance
    └── indexing metadata
    ↓
Resource Store
    ↓
Context Engine
    ├── search/retrieve
    ├── rank
    ├── budget
    ├── select
    └── provider render
    ↓
Agent
```

The Context Engine must not own EPUB/PDF/code parsing. Parsers are adapters behind a resource-parser contract so they can be replaced independently.

Candidate future port:

```text
ResourceParserPort
```

Potential adapters later include code/AST, Markdown, HTML, PDF, EPUB and DOCX parsers. Do not create empty implementations before a real task requires them.

## 17.3 Resource Registry instead of folder-as-database

Do not use filesystem presence as authoritative application state. A resource registry tracks at least:

```text
resource_id
source_ref / source_uri
content_hash
parser_id + parser_version
processing_status
trust_level
representations
structural_confidence
index/version metadata
```

A missing source becomes an explicit state such as `SourceMissing`; it does not silently erase historical knowledge or references.

## 17.4 Explicit rejects

Do not carry over:

- Python server architecture;
- unsafe general object deserialization such as `pickle` for user-controlled/persistent resources;
- folder presence as the authoritative database;
- simplistic HTML sanitization based only on deleting a handful of tags;
- manual copy/paste as the long-term AI integration model;
- direct dependency on reader3 itself.

## 17.5 Additional invariants

```text
INV-037 Fixed-size token chunks are derived retrieval units, not authoritative semantic structure.
INV-038 Imported/user/external resource content is untrusted data and never gains instruction authority automatically.
INV-039 Original/source evidence remains authoritative over display/model/summary representations.
INV-040 Resource parsing/normalization is isolated from Context selection behind replaceable contracts.
INV-041 Persistent user-controlled resources must not use deserialization mechanisms capable of arbitrary code execution.
INV-042 UI/display sanitization and model-context normalization are separate pipelines.
```

Primary source reviewed: https://github.com/karpathy/reader3


# 18. `principia-ai/WriteHERE`

Repository: https://github.com/principia-ai/WriteHERE  
Paper: https://aclanthology.org/2025.emnlp-main.1254/  
Paper: **Beyond Outlining: Heterogeneous Recursive Planning for Adaptive Long-form Writing with Language Models**, EMNLP 2025.  
License: repository README declares MIT.  
Snapshot during review: roughly 970+ stars; no formal GitHub releases were published.

## Verdict

**ADAPT — high architectural value for Planner/Reconciler/Task Graph, low value as a direct runtime dependency.**

WriteHERE is not primarily interesting because it writes long documents. Its useful idea for us is that planning and execution do not need to be two rigid phases. The system recursively decomposes only when needed, executes primitive leaves immediately, then changes the work graph as new results become available.

The published paper models the process as a DAG and uses a state-based hierarchical task scheduler. Tasks are heterogeneous (`retrieval`, `reasoning`, `composition`), and planning is interleaved with execution instead of generating one immutable outline upfront. The paper reports improvements on long-form writing benchmarks and ablations show degradation when recursive planning is removed. This is useful evidence for the planning pattern, but **it is not a software-engineering benchmark**, so we must validate the strategy ourselves before making it the default coding planner.

## What to adopt

### A. Interleave decomposition and execution

Do not require the Planner to predict the entire project perfectly before any work starts.

Target concept:

```text
coarse goal / current plan
        ↓
select actionable node
        ↓
atomic enough?
   ├─ yes → execute → evidence → reconcile
   └─ no  → propose bounded children/dependencies
                    ↓
              validate + persist
```

Only the affected branch of the DAG should need replanning when new evidence invalidates an assumption.

### B. `PlannerPort` returns `PlanChangeSet`, never direct mutation

The LLM/planner is not authoritative application state. It proposes a typed, versioned graph delta containing:

```text
base revision
reason
source evidence
nodes/edges to add/update/remove
risk delta
budget delta
scope delta
```

The Task/Plan application layer validates DAG integrity, policies, scope, acceptance preservation and budgets before persisting the next plan revision.

### C. First-class plan revisions

Every dynamic structural change should be explainable later:

```text
revision 17
reason: test exposed a missing migration prerequisite
caused_by: TEST-8831
added: TASK-214
changed dependency path: 210 → 214 → 211
```

This supports audit, replay, debugging and a strong Mission-Control UX.

### D. Heterogeneous task classes

Adopt the principle that different work needs different decomposition/execution policies, but do not hardcode WriteHERE's writing-specific three classes.

Candidate extensible classes for our system:

```text
Research
Design
Implementation
Verification
Integration
Operations
```

A task kind may influence planner strategy, capability routing, context construction and evaluator choice. It must remain provider-neutral metadata rather than domain branching on Claude/Codex/etc.

### E. Graph-scoped context

The paper's memory is task graph + workspace, while only task-relevant knowledge is loaded into the actual execution context. This aligns strongly with our Context Engine.

Prefer context from:

```text
current task
+ relevant ancestors/goal constraints
+ completed prerequisite outputs
+ selected sibling/dependent evidence
+ needed workspace resources
```

Do not inject the entire task DAG/history into every agent.

### F. Planning observability

Expose durable operational facts in the UI:

- current DAG;
- task types;
- dependencies/readiness;
- plan revision history;
- why a task appeared or was replanned;
- which evidence triggered a change;
- budget/guardrail interventions.

Do **not** market or expose private chain-of-thought. The useful transparency is structured state and provenance.

## Critical lessons from repository failure modes

The repository's open issues are as valuable as the paper. Current reports include:

- infinite loop in summarization;
- Python maximum recursion-depth failure;
- a decomposition failure where a writing task becomes only thinking-type subtasks and stops making writing progress;
- key-handling/security concerns;
- historical bugs around task dependency data.

These show why adaptive recursive planning must never be an unconstrained `while LLM says decompose` loop.

### Required anti-recursion/liveness controls

```text
max_decomposition_depth
max_children_per_decomposition
max_total_plan_nodes
max_plan_mutations_per_reconcile_cycle
planning time/token budget
no-progress timeout
cycle detection
duplicate/self-similar decomposition fingerprint detection
scope/risk escalation gates
```

A decomposition must move toward executable work. If it cannot produce a valid smaller plan inside limits, the system explicitly blocks/escalates instead of planning forever.

## What not to copy

- the Python/Flask/React implementation architecture;
- direct API-key file handling as our credential model;
- `ACTIVE/SUSPENDED/SILENT` as our complete persistent Task lifecycle;
- fixed `retrieval/reasoning/composition` task ontology;
- unbounded recursive LLM decomposition;
- the repository itself as a runtime dependency.

WriteHERE's simple scheduling states are useful conceptually, but our richer lifecycle remains authoritative. `RUNNABLE/WAITING/...` should be a **derived scheduling readiness**, distinct from persistent business status.

## Architecture decisions accepted

The Source of Truth has been updated with:

- bounded adaptive hierarchical planning;
- `PlannerPort` and declarative `PlanChangeSet`;
- versioned plan revisions with provenance;
- separation of persistent lifecycle vs derived scheduling readiness;
- extensible task kinds;
- graph-scoped context retrieval;
- hard decomposition/liveness budgets;
- requirement to benchmark adaptive planning against static planning on coding tasks before default enablement;
- invariants `INV-043` through `INV-050`.

## Priority

**P1 architecture / G7 runtime feature.**  
Do not implement adaptive recursive planning before durable Task Core, FakeAgent, Git/worktree isolation, Policy Engine and basic multi-agent DAG are stable.


# Davia — davialabs/davia

**Decision: ADAPT (high product/UX value, medium architecture value, low value as a runtime dependency).**

Repository: `davialabs/davia`  
License: MIT.  
Status at review time: **archived/read-only since 2026-08-10**. GitHub showed roughly **1.7k stars**, so the social post's ~693-star figure is stale.

## What the project actually does

Davia is an interactive internal-documentation environment aimed at coding agents. Its CLI initializes a `.davia` workspace and agent-specific instructions. Documentation is generated locally, then viewed through a rich web workspace. The project also offered an opt-in push flow to a hosted collaborative workspace.

The repository is a TypeScript pnpm/Turborepo monorepo with:

- CLI application;
- web documentation workspace;
- a separate AI-agent package;
- rich editor/visualization dependencies including Tiptap, React Flow, Excalidraw, Mermaid-to-Excalidraw and CodeMirror.

The agent implementation uses LangChain/LangGraph and direct Anthropic/OpenAI/Google API integrations.

## Strong ideas to adopt/adapt

### 1. Documentation as a first-class AI/human artifact

The strongest idea is product-level: documentation should not only be static Markdown generated once. It can be an interactive project knowledge surface shared by humans and agents.

For our product this becomes a **Documentation Engine + Architecture Explorer**, not a clone of Davia.

### 2. Editable visual documentation

Interactive graphs and diagrams make repository/business logic much easier to inspect. We adopt visual architecture/documentation projections, but keep a structured evidence-grounded graph or declarative model underneath so that a renderer can be replaced.

### 3. Local-first generation with optional collaboration

Davia generates files locally and only uploads through an explicit push flow. This supports our local-first principle. We go further: cloud sync remains fully optional, and the user must be able to inspect what data will leave the device.

### 4. Separate generated and curated knowledge

Davia's editable documentation highlights an important problem: if both the agent and the human edit the same generated page, regeneration can destroy human intent. Our design therefore explicitly separates generated/rebuildable projections from human-curated overlays and composes them deterministically.

### 5. Visual project map as agent-debugging UI

We extend the concept beyond docs. The Architecture Explorer should expose module/symbol/task/ADR/security relationships with provenance and actions such as attaching a node to task context or opening source evidence.

## Important weaknesses / anti-patterns

### 1. Whole-repository context injection

Davia's initialization middleware reads repository files, formats them as Markdown and injects the result into a `SystemMessage`. Config limits this to 1,000 files and approximately 130k tokens, where token count is estimated using a rough 4-characters-per-token rule.

This is incompatible with our architecture for two reasons:

1. it treats context capacity as a bulk-loading problem rather than semantic just-in-time retrieval;
2. more seriously, repository content is untrusted data and must not be elevated into system-instruction authority.

Our Documentation Engine must retrieve repository evidence through Resource Engine + Context Engine and keep resource data in a lower-authority context channel.

### 2. Coarse token/file budgeting

`MAX_FILES = 1000`, `MAX_TOKENS = 130000` and a universal 4 chars/token heuristic are acceptable prototype safety caps but not a context strategy. We retain hard budgets but use model/provider-aware token estimation and semantic retrieval.

### 3. Very high LangGraph recursion limit

The agent raises graph recursion to 1000. This is another reminder that planning/agent loops need explicit liveness and no-progress controls. Our existing bounded-planning invariants remain stronger.

### 4. Excluding broad resource classes

The repository excludes JSON, XML, CSV, PDFs, office documents, databases and many other extensions from the bulk reader. For repository understanding this can discard important configuration/schema/data evidence. Our Resource Engine handles file/resource types by parser capability and trust policy rather than a single broad exclusion list.

### 5. Credentials handling

The CLI searches `.davia/.env` and project `.env`, copies the selected provider key into process environment, and the sync code stores an access token in a local JSON config. This is not acceptable for our security model. We keep provider credentials behind OS credential stores/Secret Broker and prefer native CLI authentication where possible.

### 6. Diagram conversion coupled to Puppeteer

Davia converts Mermaid to Excalidraw through a Puppeteer-based helper. Useful as a prototype, but a headless browser must not become a core diagram/domain dependency. Our `DiagramRendererPort` makes Mermaid/React Flow/Excalidraw/headless conversion replaceable adapters.

### 7. Hosted workspace maturity

The archived README says a push creates/uploads a workspace but updating an already-pushed workspace was not supported at that point. This reinforces keeping synchronization as a separate later subsystem rather than making cloud state authoritative.

## Adopt

- interactive documentation as a first-class product surface;
- local generation + optional collaboration concept;
- architecture/project visual explorer;
- rich text + code + diagram presentation primitives;
- agent-facing documentation generation workflow.

## Adapt

- generated docs -> provenance-backed derived projections;
- editable docs -> Generated + Curated layers with merge/diff semantics;
- diagrams -> canonical structured graph/declarative source + replaceable renderer;
- full regeneration -> dependency-driven stale detection and incremental rebuild;
- local files -> Documentation Engine backed by Resource/Event/Task evidence;
- AI documentation context -> normal Context Engine retrieval, not whole-repo injection.

## Reject

- whole-repository source concatenated into system messages;
- a universal 4 chars/token estimate as the budgeting mechanism;
- generated documentation as authoritative truth;
- broad file-extension exclusion as repository understanding strategy;
- plaintext/local JSON credential storage;
- direct provider SDKs/API-key handling leaking into documentation domain;
- Puppeteer/Excalidraw as canonical diagram architecture;
- archived Davia codebase as a runtime dependency.

## Resulting architecture updates

The Source of Truth now defines:

- `DocumentationEnginePort` / `DocumentationRendererPort` / `DiagramRendererPort`;
- evidence/generated/curated documentation layers;
- provenance and freshness requirements;
- incremental staleness-aware regeneration;
- Architecture Explorer / Project Map;
- anti-feedback-loop rules for generated docs used as context;
- strict repository-content authority boundaries;
- new invariants `INV-051` through `INV-058`.

## Priority

**P2.** Build after durable project state, Resource Engine, Context Engine and repository/code graph foundations. A read-only Architecture Explorer may precede the full editor.

---

# SmarterRouter — Intelligent LLM Routing / VRAM-Aware Local Gateway

Repository: `peva3/SmarterRouter`

Decision: **ADAPT — HIGH VALUE for Model Routing, Local Inference Scheduling, Profiling and Failover architecture. Do not embed the Python project as a runtime dependency.**

License: MIT.

## What the project actually does

SmarterRouter is an OpenAI-compatible routing gateway over local and external model backends. Its architecture separates backend adapters from routing logic, profiles models on the user's actual hardware, combines local measurements with external benchmark data, tracks GPU/VRAM pressure, supports model discovery and hot-swap, and can fall back to alternative models when a preferred route fails.

Important observed mechanisms:

- backend abstraction for Ollama, llama.cpp and OpenAI-compatible services;
- model capability probing for features such as vision and tool use;
- standardized local profiling prompts;
- optional LLM-as-judge evaluation;
- external benchmark database aggregated from many public sources;
- hardware-specific latency/VRAM measurements;
- LRU / largest-first model eviction under VRAM pressure;
- exact and semantic routing-decision cache;
- response cache;
- local SQLite persistence for profiles, benchmarks and feedback;
- hot discovery/reprofiling of newly available models;
- cascading fallback when a selected model fails.

## Adopt

### 1. Hardware-specific model profiling

A model's usefulness is not an abstract global property. Runtime performance depends on the user's hardware, quantization, backend, context size and concurrency.

Our system should maintain a versioned `ModelRuntimeProfile` containing measured properties such as:

- cold-start latency;
- warm latency;
- tokens/sec;
- peak memory / estimated resident memory;
- context-window capability;
- tool-call capability;
- structured-output capability;
- vision/audio capability where relevant;
- reliability/error rate;
- measured-at timestamp;
- hardware/backend fingerprint.

External benchmark data may supplement missing information, but local measured data outranks generic estimates for local execution characteristics.

### 2. Separate model capability, quality and resource profiles

Do not collapse all model information into one scalar score.

Conceptually separate:

- `ModelCapabilities` — what the model/runtime can do;
- `ModelQualityProfile` — how well it performs for task classes;
- `ModelRuntimeProfile` — performance on this device/backend;
- `ProviderCapacityProfile` — quota, concurrency, rate-limit and availability state;
- `PrivacyProfile` — where inference runs and what data may leave the device.

Routing combines these dimensions at decision time.

### 3. Local resource-aware scheduling

Adopt a dedicated `DeviceResourceManagerPort` / `ModelResidencyManagerPort` rather than placing VRAM logic in the Model Router.

Responsibilities include:

- observing CPU/GPU/unified memory pressure;
- model residency tracking;
- estimated load/unload cost;
- pinned models;
- model load/unload requests;
- device-specific capacity;
- resource-pressure events.

The Router asks whether a candidate is feasible; it does not directly call `nvidia-smi`, ROCm, sysfs or OS APIs.

### 4. Model residency as a scheduling cost

Routing should consider not only quality and nominal speed but also whether a model is already resident.

A slightly weaker warm local model may be preferable to loading a much larger cold model for a low-risk task. Conversely, security-sensitive or architecture-critical tasks may explicitly override residency/cost preferences.

### 5. Dynamic model discovery

Backends are dynamic resources. The system should be able to discover models without restart and retain historical profile data when a model temporarily disappears.

Use states such as:

- Available;
- Unavailable;
- Degraded;
- Profiling;
- Unsupported;
- Disabled.

A missing model should not erase its historical performance profile.

### 6. Observed performance feedback loop

Our Adaptive Model Router should learn from completed project executions, not only synthetic benchmarks.

Useful dimensions include:

- task kind;
- repository;
- language/framework;
- first-pass verification rate;
- reviewer rejection rate;
- regression rate;
- execution duration;
- token/monetary cost;
- context size;
- number of retries;
- human override/approval outcome.

This repository-specific empirical profile is a stronger long-term moat than generic benchmark tables.

### 7. Multi-candidate failover

The router should produce a ranked route plan rather than a single irreversible choice:

`primary -> fallback candidates -> escalation policy`.

However failover semantics must depend on execution purity. Read-only inference may transparently retry. Stateful coding-agent executions create a new durable attempt and may not blindly replay side effects.

## Adapt

### SmarterRouter prompt classification -> structured task metadata first

SmarterRouter infers categories/difficulty from prompt content and heuristics. Our system usually has richer authoritative metadata already:

- TaskKind;
- risk;
- required capabilities;
- acceptance criteria;
- repository/language;
- tool requirements;
- context budget;
- privacy policy.

Therefore classification from raw prompt is only a fallback for ad-hoc/unstructured interactions, never the primary source when structured task metadata exists.

### Static benchmark database -> provenance-aware benchmark evidence

External benchmark data can be useful, but every score must retain:

- benchmark/source name;
- version/date;
- measured vs estimated status;
- evaluator methodology;
- model/runtime variant;
- confidence;
- applicability tags.

Estimated benchmark values must never silently masquerade as measurements.

### LLM-as-judge -> secondary evaluator signal

Use deterministic task-specific evaluation whenever possible. LLM judging is useful for qualitative tasks but is not authoritative by itself.

Avoid:

- model judging itself;
- one judge deciding all quality;
- routing loops where benchmark scores are generated by the candidate models themselves without independent evidence.

### GPU/VRAM support -> generic device-resource model

Do not encode NVIDIA/AMD/Intel/Apple assumptions in business logic. Platform-specific measurement lives in replaceable adapters.

Apple unified memory, container memory, CPU-only inference and future accelerators should fit the same resource model.

Measurements may have confidence/error bounds; heuristic values must be marked as estimates.

### OpenAI-compatible gateway -> optional compatibility adapter

An OpenAI-compatible local endpoint can be useful for integrations, but it is an adapter around our routing service, not the internal domain API.

Internal orchestration must retain richer semantics than `/v1/chat/completions`, including task identity, capability requirements, policy, attempt provenance and routing explanation.

## Reject / Do not copy blindly

### 1. Single weighted routing formula

A fixed scalar formula is too weak for our product. Routing is constrained optimization with hard eligibility gates first, then preference scoring.

Examples of hard gates:

- required tools unsupported;
- context window too small;
- provider forbidden by privacy policy;
- model unavailable/quota exhausted;
- resource budget impossible;
- task risk requires a trusted verifier/provider.

Only eligible candidates are scored afterward.

### 2. Model-name heuristics as authority

Names such as `*coder` may be weak hints but are not reliable capability evidence.

### 3. Semantic response cache keyed mainly by prompt similarity

This is unsafe for coding and stateful agents.

Two semantically similar prompts may correspond to different:

- commits;
- worktrees;
- task revisions;
- permissions;
- tool states;
- project constitutions;
- provider/system instructions.

Response reuse is allowed only when the operation is explicitly cacheable and the key contains a complete immutable state/context fingerprint. Otherwise cache routing metadata/embeddings/artifacts, not generated answers.

### 4. Transparent fallback after side effects

Never silently rerun a stateful agent on another model after files/commands/tools may already have changed the world. Create a new `ExecutionAttempt`, recover from durable task/worktree state and apply idempotency/policy rules.

### 5. Raw environment variables as our primary secret UX

Our product keeps credentials behind the Secret Broker / OS credential stores. Provider-native subscription authentication should remain provider-owned whenever possible.

## Resulting architecture updates

The Source of Truth now adds a provider-neutral **Adaptive Model Routing & Local Resource Scheduling** subsystem with explicit ports for:

- `ModelRouterPort`;
- `ModelCatalogPort`;
- `ModelProfilerPort`;
- `BenchmarkEvidencePort`;
- `DeviceResourceManagerPort`;
- `ModelResidencyManagerPort`;
- `ProviderCapacityPort`;
- `RoutingFeedbackPort`.

Routing sequence:

1. derive authoritative task requirements;
2. discover eligible provider/model/runtime candidates;
3. apply hard capability/security/privacy/resource gates;
4. score remaining candidates using repository-specific historical quality, global benchmark evidence, runtime performance, cost/quota, latency, context compatibility and residency cost;
5. return a ranked, explainable `RoutePlan` with fallback semantics;
6. record actual outcome and update empirical profiles after independent verification.

## Priority

**P1/P2 boundary.** The abstract routing contracts should exist early so provider selection never leaks into domain logic. Advanced learning/profiling/VRAM optimization can be implemented after the basic provider/runtime pipeline works reliably.


---

## ZeroClaw (`zeroclaw-labs/zeroclaw`)

**Reviewed:** 2026-09-09  
**Verdict:** **ADAPT / STUDY DEEPLY**  
**Direct runtime dependency:** **No**  
**License:** dual MIT OR Apache-2.0 in the current upstream repository.

### Current project reality

The social post understates the current popularity: public GitHub analytics in September 2026 report roughly **32.6–32.7k stars** and ~4.9k forks. The current upstream is no longer a tiny weekend codebase: GitHub shows thousands of commits and a large Rust workspace with many crates.

The famous footprint/startup comparison is useful as a direction but should not be treated as independent scientific evidence. The published benchmark is project-reported, some comparison values are normalized for edge hardware, and current builds/feature sets may differ. Reproduce measurements locally before using them for architectural or marketing decisions.

### What is genuinely valuable for our project

#### 1. Trait-driven extension points

ZeroClaw keeps providers, channels, tools, memory and runtime-adjacent capabilities behind public Rust traits. This strongly validates our Ports & Adapters decision.

**Adopt principle:** keep a small provider-neutral contracts/API crate and wire concrete implementations through factories/registries outside the domain.

#### 2. Compact AGENTS bootstrap + architecture routing map

ZeroClaw's root `AGENTS.md` deliberately stays compact and points agents to an architecture map for task-specific reading.

**Adopt:** our `AGENT.md` remains the always-loaded contract; add `docs/architecture/MAP.md` for progressive disclosure.

#### 3. Explicit Single Source of Truth rule

Their contributor guidance requires naming the canonical source before adding state/caches/config duplication.

**Adopt strongly:** every persistent fact in our architecture must have one bounded-context owner. Other forms are derived views/snapshots/caches with invalidation/provenance.

#### 4. History trimming keeps tool protocol valid

ZeroClaw explicitly documents that trimming is lossy but must preserve tool-call/tool-result pairing and payload provenance.

**Adopt:** our Context Engine cannot prune individual sides of a correlated tool exchange. Large results become bounded summaries + artifact references, not dangling protocol objects.

#### 5. Tool execution receipts

ZeroClaw implements HMAC-SHA256 receipts for successful tool results so the model cannot forge a new valid claim that a tool executed. This addresses an important distinction between model narration and runtime evidence.

**Adopt and extend:**

- live per-session authenticated receipt for provider-visible tool results;
- durable audit evidence in our Event Journal/Security Audit for security-relevant actions;
- never treat a receipt as authorization, correctness or completion evidence by itself.

The current ZeroClaw history also shows why implementation truth must outrank documentation: an upstream issue documented a period where receipt docs described wiring as shipped while production call sites still passed `None`. Our Architecture/Documentation Engine must therefore tie generated claims to executable/testable evidence where possible.

#### 6. Risk profiles and sandbox backend separation

ZeroClaw separates agent risk profiles from OS sandbox implementations and documents backend-specific capability differences (filesystem vs network, etc.).

**Adopt:** policy/risk semantics are domain-level; Landlock/Seatbelt/container/VM details live behind sandbox adapters. Required isolation fails closed.

#### 7. Deterministic SOP engine

ZeroClaw has deterministic Standard Operating Procedures with triggers, approval gates, durable run state and resumability.

**Adopt concept:** our adaptive Planner is not the answer for every workflow. Introduce a future deterministic `ProcedureEngine` for known repeatable sequences.

#### 8. Separation of history, memory, files, logs and usage

ZeroClaw's documentation is unusually explicit that file content is not automatically memory, logs are diagnostics rather than source-of-truth memory, and tool results require intentional promotion if they should persist.

**Adopt strongly:** preserve separate ownership/lifecycles for session history, long-term memory, artifacts/files, Event Journal, Security Audit, operational logs and usage ledgers.

#### 9. Incremental daemon persistence

The daemon writes conversation/memory state incrementally rather than relying on a shutdown flush.

**Adopt:** hard-kill recovery is a first-class test case for `agentd`.

### Important warning: runtime efficiency != maintainable architecture

A closed RFC in the ZeroClaw repository criticized the project's growth to a very large Rust codebase, citing roughly 1,076 Rust files / ~776k lines at that point and several extremely large modules. The maintainers did not rewrite it in Python, but the criticism is architecturally useful.

**Our response:**

- CI dependency-boundary tests;
- narrow crates;
- generated code separated from hand-written domain code;
- soft/hard module size review budgets;
- no speculative abstractions or giant central configuration modules;
- every cross-context integration through an explicit contract.

### Reject / do not copy blindly

1. **Self-reported marketing benchmark as fact.** Reproduce performance on our own build/hardware.
2. **Silent sandbox fallback to `none`.** For our required security profiles, missing isolation must block execution instead of weakening it.
3. **Putting all personal-assistant/channel/hardware capabilities into the coding-core.** Our product remains focused; channels/peripherals are optional integrations, not core domain.
4. **A giant Rust workspace as an acceptable cost of extensibility.** Replaceability is useful only if the implementation remains navigable.
5. **In-band receipts as the whole audit system.** We additionally need durable Event Journal/Security Audit evidence.

### Architectural changes accepted

- add Tool Execution Receipt design;
- add future deterministic Procedure Engine;
- add `docs/architecture/MAP.md`;
- strengthen canonical state ownership;
- strengthen tool-call/tool-result pruning rules;
- require fail-closed isolation;
- add Rust architecture fitness and resource benchmarks;
- add invariants INV-069 through INV-082.

### Priority

**High.** ZeroClaw is one of the most relevant repositories reviewed so far because its implementation domain overlaps directly with our Rust local agent runtime. We should study specific subsystems during implementation, but reproduce the ideas behind our own contracts rather than importing the runtime wholesale.

---

## Claude Octopus (`nyldn/claude-octopus`)

**Reviewed:** 2026-09-09  
**Verdict:** **ADAPT / HIGH VALUE for workflow orchestration, provider diversity and quality gates**  
**Direct runtime dependency:** **No**  
**License:** MIT.

### Current project reality

The social post is already behind the upstream project. The current repository exposes a much broader surface than the quoted 29 roles / 43 skills: the README currently advertises roughly **31 specialist personas, 53 commands and 63 skills**, with support for a dozen external provider integrations alongside the Claude host. The repository has also evolved far beyond a small shell plugin into a large workflow/orchestration project with MCP integration, worktree-based parallelism, provider health checks, reaction handling, routines, cost accounting and council workflows.

The project remains **Claude-centric by product design**: Claude Code is the host/orchestrator and external models are additional seats or escalation paths. That is useful as an implementation study, but it is explicitly not the architecture we want for our provider-neutral Agent OS.

### What is genuinely valuable for our project

#### 1. Workflow templates with explicit phase gates

Claude Octopus uses a Double Diamond-inspired lifecycle:

```text
Discover -> Define -> Develop -> Deliver
```

Each phase has a distinct purpose and can have its own providers, tools and quality gates.

**Adopt as a workflow-template concept, not as the universal Task state machine.**

Our Task lifecycle remains provider/workflow neutral. A higher-level `WorkflowTemplate` / future `ProcedureEngine` can encode macro flows such as:

```text
Research -> Scope -> Implement -> Verify -> Deliver
```

or:

```text
Threat Model -> Implement -> Security Verify -> Human Gate
```

The workflow template may create/coordinate durable Tasks but must never replace the Task state machine or authoritative acceptance criteria.

#### 2. Provider capability discovery and graceful degradation

Octopus detects whether providers are available and can continue a workflow with the remaining seats when an optional provider is missing or times out.

**Adopt strongly:** provider availability/capability is runtime state, not compile-time configuration.

The Model/Provider layer should expose states such as:

```text
available
missing
degraded
quota_exhausted
policy_blocked
incompatible
```

A workflow declares minimum required capabilities/quorum separately from preferred provider roster.

Graceful degradation is allowed only when the workflow contract permits it. Security- or verification-critical seats may fail closed.

#### 3. Provider-aware preflight before expensive dispatch

Octopus performs prompt-size/provider checks and exposes projected run cost before large workflows. It also records per-provider/per-skill usage.

**Adopt and generalize:** before dispatch, our system should calculate a `DispatchPreflight` containing:

- eligible providers/models;
- estimated context size;
- provider context limits;
- expected token/cost range where observable;
- subscription/quota status where observable;
- expected number of seats/attempts;
- required tool/sandbox capabilities;
- data/privacy destinations;
- risk of degraded/fallback execution.

Preflight estimates are evidence/estimates, never guarantees.

#### 4. Multi-model council as a bounded decision primitive

Octopus supports structured councils with multiple seats, styles such as adversarial/red-team, quorum, critical-veto gates, budget caps and optional implementation handoff.

**Adopt as a bounded high-risk decision mechanism.**

Our future `CouncilEngine` should be separate from ordinary agent chat and support:

```text
CouncilRequest
- goal: advice | decision | plan | review | risk_assessment
- seat requirements
- diversity requirements
- evidence set
- budget
- quorum policy
- veto policy
- synthesis policy
```

Councils are appropriate for architecture decisions, security review, ambiguous trade-offs and high-risk verification. They are not the default for every implementation task.

#### 5. Minority-opinion preservation

A valuable Octopus behavior is surfacing dissent rather than only returning a flattened majority synthesis.

**Adopt strongly:** a council/synthesis result must preserve unresolved material disagreement.

The result should contain:

```text
consensus findings
minority findings
critical objections
uncertain claims
evidence references
```

A synthesizer may summarize disagreement but may not silently erase it.

#### 6. Critical veto beats naive majority voting

Octopus has moved beyond only a scalar quality threshold by adding quorum and critical-veto concepts to council workflows.

**Adopt, but make risk-driven.**

Example:

```text
5 seats agree implementation is fine
SecurityVerifier reports confirmed auth bypass
=> FAIL / BLOCK
```

A safety/security/data-loss invariant can veto a majority when the veto is backed by the required evidence class.

#### 7. Provider/vendor diversity is a first-class signal

Octopus explicitly distinguishes model diversity from vendor diversity; two models from the same provider are not equivalent to independent cross-provider review.

**Adopt:** `DiversityPolicy` should be able to require:

- different provider families;
- different model families;
- different role prompts;
- independent context construction where appropriate;
- blinded review for selected evaluations.

This prevents false confidence from several highly correlated seats.

#### 8. Provider attribution in every aggregate result

Octopus exposes which providers actually contributed and records provider-aware usage/cost.

**Adopt strongly:** every synthesized/council/review artifact must retain seat attribution and provenance.

Never present a result as “multi-model verified” if only one provider successfully contributed.

#### 9. Worktree isolation for parallel implementation

Octopus runs independent work packages in isolated git worktrees and gates handoff/merge.

This confirms our existing worktree-per-execution direction. We keep Git/Workspace isolation in our own `WorkspaceEngine`, not inside workflow/persona code.

#### 10. Reaction engine for external lifecycle events

Octopus models reactions to CI failures, requested changes, stuck agents and PR lifecycle state with retry/escalation limits.

**Adapt into our Event -> Procedure/Reconciler model.**

Examples:

```text
CI Failed
  -> attach evidence to Task
  -> reopen/retry according to policy
  -> escalate after bounded attempts

Review Changes Requested
  -> create revision attempt
  -> preserve original review evidence

No Progress Timeout
  -> mark attempt stalled
  -> checkpoint
  -> reroute/escalate
```

Retries are bounded and visible. External event handling never creates an unbounded autonomous loop.

#### 11. Explicit activation instead of keyword-triggered hidden delegation

Current Octopus personas are selected by explicit workflows rather than automatically activating solely because a prompt contains a keyword.

**Adopt principle:** important role/workflow activation must be explainable and policy-driven. Keyword matching can be a suggestion signal, not hidden authority to launch expensive or privileged agents.

#### 12. MCP/IDE seam is thin

Octopus' IDE integration uses an MCP server over stdio and calls the existing orchestration core using argument-safe process execution rather than duplicating workflow behavior in the IDE adapter.

**Adopt the seam principle:** UI/IDE/MCP adapters are control surfaces. They do not own orchestration semantics.

### Important weaknesses / things we should not copy

#### 1. Claude as the architectural center

The host Claude is ultimately the coordinator/synthesizer in the primary product model.

**Reject for our core.** Our durable Task/Planner/Reconciler/Context state is authoritative; Claude, Codex, Muse, OpenCode, Gemini or future agents are replaceable execution seats.

#### 2. Fixed 75% consensus / quality threshold as a universal rule

A scalar threshold is easy to understand but too crude for security, architecture and deterministic software verification.

**Reject as a universal completion rule.**

Use typed gates:

- deterministic tests;
- required verifier classes;
- quorum where appropriate;
- evidence-backed critical veto;
- risk policy;
- human approval where required.

#### 3. Large persona/skill catalog as core complexity

31+ personas and 60+ skills are useful UX/workflow assets but can become prompt/configuration sprawl.

**Do not hard-code dozens of personas into the domain.**

Our role model stays data-driven and versioned:

```text
RoleProfile
- capabilities
- responsibilities
- deliverables
- success metrics
- allowed tools
- risk constraints
- handoff/escalation rules
```

Role packs/skills belong above the core contracts and can be installed/versioned independently.

#### 4. Shell-heavy orchestration as our runtime architecture

Octopus retains a Bash orchestration core with TypeScript/MCP wrappers. It uses safer process invocation patterns in the IDE seam, but this is not suitable as our long-lived system architecture.

**Reject implementation approach:** our orchestration/runtime stays Rust with typed contracts, durable state, Policy Engine and explicit process adapters.

#### 5. External CLI responses classified as low-risk by default

The published Octopus threat model labels external CLI responses as relatively low risk. For our product, any provider output, repository content, web result or tool-produced content is untrusted data until validated for its use.

**Reject the low-trust assumption.** Provider output can contain prompt injection, unsafe commands, hallucinated claims or malicious content derived from repository/web inputs.

#### 6. Gate delivery must be verifiably installed/enforced

A historical Octopus issue showed Marketplace packaging could strip a hooks manifest and silently disable many quality gates. This is a critical architectural lesson.

**Adopt the lesson:** security/quality controls must expose runtime attestation/health, not exist only as configuration files.

The system should be able to answer:

```text
Is policy gate X installed?
Was it executed for Attempt Y?
Which version ran?
What evidence/result did it produce?
```

A configured-but-not-executed gate is not evidence.

### Resulting architecture updates

Add or formalize these provider-neutral concepts:

- `WorkflowTemplatePort` / future `ProcedureEngine` macro workflows;
- `DispatchPreflight`;
- `CouncilEnginePort`;
- `CouncilPolicy` with quorum/diversity/critical-veto rules;
- `DiversityPolicy`;
- `ProviderContribution` / seat attribution;
- `GateAttestation` / enforcement health;
- bounded `ReactionPolicy` integrated through Event Journal/Reconciler;
- data-driven `RoleProfile` and independently versioned skill/workflow packs.

### Priority

- **P0/P1:** provider capability state, preflight, contribution provenance, gate attestation, worktree/reaction semantics.
- **P1/P2:** workflow templates, role profiles, council/quorum/diversity infrastructure.
- **P2:** marketplace-grade role/skill packs and sophisticated multi-model deliberation UX.

---

## vakra-dev/reader — web acquisition and browser infrastructure for LLMs

**Repository:** https://github.com/vakra-dev/reader  
**Reviewed:** 2026-09-09  
**Language:** TypeScript; includes a Rust-based HTML-to-Markdown dependency (`@vakra-dev/supermarkdown`)  
**License:** Apache-2.0  
**Decision:** **ADAPT / HIGH VALUE for web acquisition architecture; do not use as a core dependency.**

### What the current repository actually is

The current Reader release (`0.3.x` at review time) is a Playwright-based web acquisition stack exposing three main primitives:

1. scrape known URLs into Markdown/HTML;
2. crawl/discover pages with bounded depth/page count;
3. create browser sessions reachable through CDP for Playwright/Puppeteer control.

Its current `package.json` depends on `playwright-core`, `fingerprint-generator`, `fingerprint-injector`, `proxy-chain`, `undici` and `@vakra-dev/supermarkdown`. It does **not** depend on Ulixee Hero. The social post saying Reader "is part of Ulixee Hero" is therefore not accurate for the current repository; it may reflect an earlier implementation or confusion with related anti-detection projects.

Reader's own README describes browser pooling, anti-detection/fingerprint behavior, main-content cleaning, proxy tiers, retries/timeouts, crawl limits and daemon mode. The repository currently exposes typed errors, `robots.txt` checks, proxy health/gating and browser health/recycling.

### ADOPT / ADAPT

#### A. Separate acquisition from interpretation

Strongly adopt the architectural separation:

```text
web acquisition -> immutable raw artifact -> Resource Engine -> Context Engine
```

The browser/fetcher must never become the Context Engine.

#### B. Three different capabilities, not one giant browser API

Adapt Reader's product primitives into provider-neutral ports:

- `WebFetchPort` — fetch a known resource;
- `CrawlPort` — bounded discovery;
- `BrowserAutomationPort` — stateful rendered interaction.

This makes simple HTTP fetching possible without booting a browser and keeps interactive browser semantics out of resource/context code.

#### C. Warm browser runtime with health and recycling

Reader's browser pool tracks active/pending tabs, browser health, page counts and idle time, then proactively recycles unhealthy/old processes. This is useful for a future local/remote browser worker.

Adopt the lifecycle concept, not the TypeScript implementation.

#### D. Explicit extraction representations and graceful fallback

Reader keeps raw HTML and produces cleaned HTML/Markdown; its Markdown converter has a fallback to plain text when conversion fails. Adopt resilience but record the representation quality (`Structured`, `Degraded`, etc.). A fallback is not equivalent to a successful structural extraction.

#### E. Typed retryability and health-aware execution

Reader classifies errors and treats some as non-retryable; it also separates proxy selection/health from the scrape operation. Generalize this into a typed retry policy governed by Policy/Procedure/Reconciler rather than burying retries inside browser adapters.

#### F. Provenance-rich web snapshots

Store exact fetched snapshots/resources with URL/final URL/timestamp/content hash/backend/profile/freshness. This supports context replay and prevents "the web changed" from making historical agent runs irreproducible.

### SECURITY / ARCHITECTURE IMPROVEMENTS OVER READER

#### 1. Central SSRF/network policy must be stronger than URL validation

Reader validates URL syntax and checks `robots.txt`, but our future product may run inside user networks, cloud workers and authenticated sessions. We therefore require policy checks on DNS resolution and every redirect hop, including private/loopback/link-local ranges and organization allow/deny rules.

#### 2. Clean Markdown is still prompt-injection-capable

Reader's "LLM-ready Markdown" is an output format, not a trust boundary. Our Resource/Context engines retain `UNTRUSTED` provenance for HTML, Markdown, DOM, accessibility trees and page text.

#### 3. Browser needs stronger isolation than ordinary resource parsing

A browser session can execute hostile JavaScript, persist cookies and reach networks. It should run in a scoped browser worker/container/VM where policy requires, without host credentials or unrelated filesystem access.

#### 4. Stealth/anti-detection is optional

Do not make bypass/stealth a product invariant. Model it as an optional adapter capability controlled by user/org policy. Retrying through alternate fingerprint/proxy profiles must remain bounded and cannot override explicit policy/access restrictions.

#### 5. Credentials never live in task/config text

Proxy passwords, browser cookies and authenticated-site credentials are `secret_ref`s resolved by Secret Broker. Logs/audit use redacted identifiers.

### REJECT

- hard dependency on Playwright/Chromium in core `agentd` domain crates;
- exposing CDP/Playwright objects through domain interfaces;
- treating cleaned Markdown as trusted/model instructions;
- silent extraction fallback with no quality signal;
- blind retry/proxy escalation after policy or explicit access denial;
- putting proxy/site credentials in normal task/config/event payloads;
- making anti-bot bypass a core product promise.

### License / dependency decision

Reader itself is Apache-2.0, which is permissive for commercial use with its conditions. We still do not need to copy the implementation: the important value is its runtime patterns. Any future browser/formatter dependency must receive its own license/security/supply-chain review.

### Priority

- **P0:** network/SSRF policy and untrusted-web-content architecture.
- **P1:** acquisition ports + web snapshot/result contracts + Resource Engine integration.
- **P2:** browser pool/worker, crawl capability, remote browser, proxy/stealth adapters.


---

## automata/aicodeguide — AI Code Guide

**Verdict:** ADAPT / HIGH METHODOLOGY VALUE; no runtime dependency.

**Repository type:** documentation/roadmap rather than an agent runtime. MIT licensed. At review time GitHub showed ~2.7k stars.

### What is genuinely useful

1. **Plan/spec before implementation.** The guide recommends turning a product idea into a PRD/specification, then a dependency-aware task plan, and only then coding. This strongly matches our durable Task/Planner architecture.
2. **Task-level rather than project-level execution.** The recommendation to give an AI a bounded feature/ticket instead of "build the whole app" directly supports our atomic execution model.
3. **AGENTS.md as agent bootstrap.** This matches our compact agent instructions + progressive architecture map approach.
4. **Engine/mechanism vs policy separation.** The guide explicitly recommends keeping engine code apart from policy code. This is highly aligned with our Ports & Adapters and replaceability goal.
5. **Testing/TDD/property-based testing.** Particularly useful for state machines, policies, parsers, context/resource handling and other edge-case-heavy subsystems.
6. **Multi-model validation.** Already present in our Council/Adaptive Router design; the guide reinforces it as a practical development workflow.
7. **Design-intent history.** Keeping a record of why decisions were made is useful, but we adapt this into structured Decisions/ADRs/Event Journal rather than raw Git-tracked transcripts.

### ADOPT

- specification before implementation;
- small dependency-aware tasks;
- explicit project-wide agent rules/bootstrap docs;
- engine/mechanism vs policy separation;
- independent verification for AI-generated code;
- property-based tests as a first-class option;
- feature/task-level context rather than project-level free-form prompts;
- multi-model review when it improves evidence diversity.

### ADAPT

#### PRD → layered specifications

Do not use one monolithic PRD as product requirements + architecture + implementation status. Split product intent, requirements, architecture constraints/ADRs, versioned ChangeSpecs, Task DAG and verification evidence.

#### Prompt log → structured design-intent journal

Raw conversations are useful debugging/history but unsafe and noisy as authoritative version-controlled artifacts. Persist material outcomes as Decisions/ADRs/spec revisions with provenance. Keep raw transcript retention local/configurable.

#### TDD → risk-based verification

Use TDD where interfaces/behavior are clear, but do not force every work type into test-first ceremony. Always require verification appropriate to task risk. Add property/model-based/fuzz/contract/security testing where stronger.

#### Project rules → canonical policy + provider renderers

`AGENTS.md`, `CLAUDE.md`, Cursor rules, etc. should be generated/rendered compatibility surfaces from canonical rules, with hashes/drift detection, rather than multiple independently maintained authorities.

### REJECT / DO NOT GENERALIZE

- blindly pack an entire large repository into context via repomix/files-to-prompt as the default; use our Resource/Context Engine JIT retrieval instead;
- put raw complete LLM/prompt histories in Git by default;
- treat a generated PRD or TODO Markdown checklist as durable execution state;
- allow implementation agents to modify tests/acceptance criteria until their code "passes" without independent test-integrity review;
- provider-specific rules as independent sources of truth;
- YOLO/dangerously-skip-permissions workflows as a product default;
- assume "more prompt detail" can replace runtime policy, sandboxing, verification or typed contracts.

### Architectural additions derived from this review

- versioned `ChangeSpec` between requirements and tasks;
- requirement → task → acceptance → evaluation/evidence traceability;
- structured Design Intent / Decision Journal;
- explicit Test Mutation Policy + Test Integrity checks;
- stronger mechanism/policy separation contracts;
- canonical project-rule source rendered to provider-specific agent files;
- full-repository packing demoted to explicit bounded fallback.

### License / dependency decision

MIT is permissive, but there is little reason to copy code: the value is methodological. Cite/inspire from the workflow ideas, implement our own contracts and execution semantics.

### Priority

- **P0:** spec/task/evidence traceability + Test Integrity + mechanism/policy boundaries.
- **P1:** ChangeSpec and structured design-intent/decision trail + provider rule renderers.
- **P2:** development-process analytics comparing planning/verification strategies across agents/models.

---

## getomni-ai/zerox — multimodal OCR and document extraction

**Verdict:** ADAPT / HIGH VALUE for Document Ingestion and Resource Engine; do not use as a privileged in-process dependency.

**Repository status at review:** ~12.3k GitHub stars, ~450 commits, MIT license. Node and Python implementations exist, but feature parity differs.

### What the project actually does

Zerox converts documents/pages to images, sends page images to a vision model, asks for Markdown, then aggregates page results. It supports multiple document families through PDF/image conversion and supports structured extraction using JSON Schema in the Node implementation. It exposes concurrency, page selection, retries, orientation/edge processing and an optional `maintainFormat` mode that serializes page processing by passing previous-page output into the next page context.

### ADOPT

- page/unit-level document processing rather than a single monolithic prompt;
- multiple provider/model capability for vision extraction;
- per-page metadata and aggregation;
- separate structured extraction contract using JSON Schema;
- ability to use a different model for OCR vs structured extraction;
- page selection and bounded concurrency;
- explicit failure accounting instead of pretending all pages succeeded;
- cross-page continuity as a real concern for tables/layout;
- benchmark-driven comparison of OCR/extraction strategies.

### ADAPT

#### Vision-first OCR → strategy-based document ingestion

Do not rasterize everything. Prefer native structural extraction for born-digital Office/HTML/XML/spreadsheet sources when it preserves more semantics. Use PDF text/layout extraction where reliable, targeted visual extraction for figures/tables, and vision OCR for scanned/visual/degraded cases.

#### `maintainFormat` → bounded carry-state

Zerox passes the previous page Markdown into the next request to improve continuity. Our version creates a small explicit `CarryState` for open tables/sections/continuations so context size does not grow and continuity can be inspected/rebuilt.

#### structured extraction → evidence-bearing extraction

JSON Schema is retained at the model/tool boundary, but extracted fields carry page/region/source provenance and confidence. High-risk fields can require independent validation/review.

#### temp files → owned job workspaces

Zerox exposes configurable temporary directories and documents behavior that can clear existing contents. Our implementation only deletes unique application-owned job directories under a controlled runtime root.

#### long documents → durable jobs

An open issue reports timeouts while processing a 500+ page PDF. Our document pipeline persists page/unit results and resumes incomplete work instead of repeating an entire document after a late failure.

### Security finding that materially affects our design

A 2026 open issue reports OS command injection through a document URL-derived file extension reaching Poppler shell execution; CVE-2026-85672 describes affected Zerox 1.1.20 and rates it critical. Whether/when upstream remediation lands does not change our architectural lesson:

- converters must run in an isolated worker;
- URL acquisition happens through centralized SSRF/network policy;
- filenames/extensions/MIME claims are untrusted metadata;
- never construct shell strings from resource metadata;
- use fixed executable + argument arrays;
- sandbox and resource-limit native document parsers/renderers.

The repository issue list also includes a SheetJS dependency vulnerability report, reinforcing that document-format dependencies require normal supply-chain/security scanning.

### REJECT

- vision rasterization as the universal canonical parser;
- embedding LibreOffice/Poppler/Ghostscript invocation directly into Resource/Context business logic;
- arbitrary remote URL fetching from a document parser;
- shell-based converter commands containing user-controlled path/extension data;
- deleting arbitrary caller-provided temp-directory contents;
- treating Markdown returned by a vision model as ground truth;
- accepting schema-valid extraction as factual verification;
- all-or-nothing processing for large documents;
- silently dropping failed pages under best-effort mode;
- hard-coding current provider/model lists into document domain types.

### Product opportunity

A future Document Inspector can make processing observable:

```text
contract.pdf
├── Native structure      available
├── Page images           12
├── Markdown              VisionExtracted / 94% quality
├── Tables                3 extracted
├── Structured fields     18/20 verified
├── External transfer     Gemini: pages 3,7,8
└── Provenance            inspectable per field/page
```

This aligns strongly with our privacy/trust positioning: users can see not just the answer, but **how a document became model context and what left the device**.

### Priority

- **P0:** sandboxed document worker + command/path/temp/SSRF safety.
- **P1:** native-first ingestion strategy, page-level durable jobs, representation/provenance contracts, schema extraction.
- **P2:** adaptive strategy benchmarks, local VLM route, region/table/figure-specific extraction and high-quality document inspector UI.


## Shannon — KeygraphHQ/shannon

**Repository:** `KeygraphHQ/shannon`  
**Verdict:** **ADAPT / VERY HIGH VALUE for Security Verification, evidence-backed gates and durable security workflows**  
**Direct dependency:** **REJECT for core**  
**License note:** current open-source repository is **AGPL-3.0**; do not copy/link its implementation into our proprietary core without deliberate license review/commercial terms.

### Current reality vs the shared post

The post is materially outdated in a few places:

- Shannon 3.0 launched on 2026-09-02 and the project reports more than ~47K GitHub stars, not 15.7K.
- The current implementation is provider-agnostic across Anthropic/OpenAI/xAI/AWS Bedrock plus compatible gateways/local endpoints; it is no longer fundamentally Claude-only.
- Current latest Shannon supports ChatGPT Plus/Pro via Codex subscription integration; current latest does **not** support Claude Code subscription, while v1.9.0 was the final Claude Agent SDK-based release that did.
- Shannon 3.0 adds a multi-stage code-analysis pipeline, resumable workspaces, CI/CD integrations, SARIF, reconciliation/deduplication and stronger reporting/reliability.

### Architecture worth studying

Current Shannon runs a source-aware security pipeline roughly as:

```text
Source code ──► Recon + vulnerability analysis ─┐
                                                ├─► Reconcile/deduplicate ─► Exploit queue
Source code ──► Security code analysis ─────────┘
Running app ───────────────────────────────────────────────► Exploitation
                                                             │
                                                exploit proven?
                                                    │ yes         │ no
                                                    ▼             ▼
                                                report/evidence  discard as pentest finding
```

Its implementation also demonstrates several good production patterns:

- ephemeral Docker worker per scan;
- repository mounted read-only;
- per-run workspace/state/logs/artifacts;
- resumability/checkpointing;
- URL/target binding on resume to prevent cross-target contamination;
- Temporal orchestration with thin activities and Temporal-agnostic business services;
- parallel category-specific analysis/exploitation with bounded concurrency;
- configuration validated by schema;
- rules of engagement / scope controls;
- evidence-backed findings and reproducible PoCs;
- final reconciliation/deduplication;
- SARIF/JSON/PDF/Markdown outputs;
- CI gates that distinguish exploit-confirmed findings from analysis hypotheses.

### ADOPT

#### 1. Proof/evidence-backed security findings

A security conclusion should carry runtime/source evidence, not only LLM prose. Proven dynamic vulnerabilities link to exact target/build/repository state, request/response/browser/tool receipts and reproduction artifacts.

#### 2. Separate analysis from exploitation

Use static/structural analysis and runtime recon to generate/prioritize hypotheses, then use a distinct exploit-verification stage where policy permits.

#### 3. Reconciliation/deduplication before expensive exploitation

Merge candidates from independent analysis paths, preserve provenance/conflicts, and avoid duplicate destructive testing.

#### 4. Durable, resumable security workflow

Security scans are long-running. Checkpoint stage/agent progress and resume only when target/input fingerprints still match.

#### 5. Read-only repository + ephemeral security worker

A pentest worker should not need to mutate the scanned repository. Dynamic security activity belongs in an isolated worker against an authorized disposable/local/staging target.

#### 6. Target binding on resume

The workspace/checkpoint is bound to normalized target identity and relevant repository/build revision. Target mismatch fails closed rather than reusing stale attack state.

#### 7. Typed scope / Rules of Engagement

Shannon supports configuration such as focus/avoid rules, login flow and request limits. Our version promotes enforceable parts into typed Policy Engine rules instead of leaving them only in free-form prompt text.

#### 8. Standardized downstream security outputs

Support SARIF as an adapter so evidence-backed findings integrate with GitHub/GitLab/security tooling without making SARIF our domain model.

#### 9. Orchestration/business-logic boundary

Shannon's Temporal activities delegate to Temporal-agnostic services. We adopt this boundary so our security procedures can later run on our Rust procedure engine, Temporal/cloud or another durable workflow engine without rewriting security business logic.

### ADAPT

#### “No exploit, no report” → two-channel finding model

For pentest reports Shannon discards candidates it cannot exploit. That is excellent for reducing speculative noise but too lossy as our universal security model.

Our design keeps:

- `AnalyticalRisk` / unproven static finding;
- `ProvenExploit` / live validated finding.

Default CI gates can treat them differently. This preserves potentially serious but environment-dependent/static risks while maintaining a high-confidence exploit channel.

#### Free-form Rules of Engagement → executable policy

Human prose remains useful for intent, but rate limits, origins, paths, forbidden destructive operations, credentials and external callbacks become machine-enforceable policy.

#### One model per run → Adaptive Model Router

Shannon 2.x/3.x uses a unified selected model setting through a scan. Our architecture keeps security roles/model needs independent so different providers/models can be chosen per stage or for independent review.

#### Temporal → replaceable workflow adapter

Temporal is a proven option for durable security workflows, but our local v1 does not need to adopt it. The domain depends on workflow ports/contracts only.

#### Parallel vuln/exploit agents → interference-aware scheduling

Parallelism is allowed only when sessions/data/rate limits/destructive actions cannot interfere with each other.

### REJECT

- copying AGPL Shannon implementation into proprietary core without an explicit licensing decision;
- treating inability to exploit as proof of absence of risk;
- allowing exploit mode against production by default;
- storing Rules of Engagement only as free-form prompt text when constraints must be enforced;
- binding Security Verification domain semantics to Temporal, Docker, Playwright, Pi harness, Claude or any current provider;
- treating an LLM-generated report as sufficient evidence;
- resuming a security workflow against a different target/environment without explicit re-binding/new run;
- conflating `scan incomplete`, `scan clean`, `risk found`, and `exploit proven` into one success/failure bit;
- automatically running destructive security actions simply because an AI categorized a hypothesis as high severity.

### Product opportunity

This repository strengthens a potentially major differentiator for our product: **Verified Delivery can include exploit-backed security verification before merge/release**.

Possible UI:

```text
SECURITY GATE — release/1.8

Target            staging-42
Repository        commit a73f...
Status            COMPLETE

Proven exploits   0
Analytical risks  3
Blocked checks    0

Injection         PASS — exploit attempted
XSS               PASS — exploit attempted
Authorization     2 analytical risks, not reproduced
SSRF              PASS — constrained probe

Evidence          27 artifacts / receipts
SARIF             available
Human review      required for 1 high analytical risk
```

This fits our broader product thesis: agents do not decide that work is “done”; independent deterministic/agentic evaluators produce inspectable evidence.

### Priority

- **P0:** finding/evidence lifecycle, security runner authorization/isolation, typed Rules of Engagement, target fingerprint binding.
- **P1:** reconciliation, resumable exploit procedures, SARIF, CI/release gate integration, evidence browser.
- **P2:** source-informed attack graph, continuous security routines, automated fix→retest loop, enterprise governance/integrations.

---

## microsoft/tgrep — trigram-indexed repository search

**Reviewed:** 2026-09-10  
**Repository:** `microsoft/tgrep`  
**Version reviewed:** `1.0.5`  
**License:** MIT  
**Decision:** **ADAPT — HIGH VALUE for future repository lexical retrieval, freshness/reconciliation, and bounded indexing architecture. DEFER direct runtime dependency until G9 benchmark/security review. REJECT its standalone TCP server as part of our production control plane.**

### What the project actually does

`tgrep` is a Rust implementation of grep-style repository search accelerated by a pre-built trigram index. Its important architectural idea is not merely “grep faster”; it separates **candidate discovery** from **exact verification**:

```text
regex / literal query
        ↓
query decomposition
        ↓
trigram posting-list lookup
        ↓
small candidate file set
        ↓
full regex/literal verification
        ↓
exact matches
```

If a query cannot be safely reduced to useful literal/trigram constraints, correctness must win over index use and the system can fall back to a direct file scan.

The current project is split into a reusable `tgrep-core` library and a `tgrep-cli`. The server mode uses a `HybridIndex` consisting of a memory-mapped immutable on-disk snapshot plus an in-memory live overlay for changed files. Microsoft states that `tgrep` is integrated into GitHub Copilot CLI for fast searches across large repositories.

### Why this matters for AI Engineering OS

Our Context Engine already rejects “embed everything, retrieve top-k, ask an LLM” as the universal repository retrieval strategy. Exact symbol names, error strings, function names, config keys and regex searches are often better served by deterministic lexical retrieval.

`tgrep` reinforces a multi-strategy model:

```text
Repository query
       ↓
RepositoryTextSearchPort / retrieval planner
       ├── exact/direct scan
       ├── indexed lexical/trigram search
       ├── FTS5 / BM25
       ├── AST / symbol graph
       ├── Git/history
       └── optional embeddings
       ↓
verified candidate evidence
       ↓
Context Planner
```

The trigram index is therefore a **replaceable retrieval strategy**, not the Context Engine itself and not authoritative repository state.

### ADOPT

#### 1. Candidate-index first, exact verification second

Use indexes to reduce the search space, but verify results against the actual source content before treating them as evidence.

This yields a useful general rule:

```text
index hit != source evidence
index hit → candidate → exact source verification
```

For correctness-critical agent work, an index should never become a substitute for the source file it indexes.

#### 2. Immutable base snapshot + live overlay

`tgrep serve` maintains:

```text
HybridIndex
├── IndexReader     # mmap'd published snapshot
└── LiveIndex       # changed/new/deleted-file overlay
```

The live layer takes precedence over the base snapshot. This is a strong model for our future repository indexes because rebuilding a whole large repository index after every edit is wasteful.

Adapt into:

```text
RepositoryIndex
├── PublishedSnapshot(revision=N)
└── LiveDelta
      ├── changed
      ├── created
      └── deleted
```

Published indexes remain immutable/rebuildable; mutable recent changes are isolated in a bounded overlay.

#### 3. Watcher events are hints, not authority

`tgrep` explicitly handles the fact that file-system notifications can be missed because of queue overflow, filesystem behavior, branch/tree replacement or watch-capacity limits. Its server therefore combines watcher-driven incremental updates with periodic full reconciliation and can fall back to polling when native watch capacity is exhausted.

Adopt the principle broadly:

```text
filesystem watcher
      ↓
low-latency change hint

periodic reconciler
      ↓
compare index/resource state with filesystem truth
      ↓
repair missed / stale / deleted entries
```

A file watcher must never be the sole correctness mechanism for repository state.

#### 4. Explicit retrieval freshness / consistency

`tgrep` documents that an on-disk index can be stale and that even a running server may temporarily lag recent edits or still be building/reconciling. It provides a direct-scan path (`--no-index`) when current filesystem contents are required.

Our architecture should expose this intentionally rather than hide it:

```text
SearchConsistency

FAST
  derived index may be used within accepted freshness bounds

FRESH
  live overlay/reconciliation state must cover known recent mutations

AUTHORITATIVE
  exact source is read/verified directly before result is trusted
```

Names are provisional until the G9 contract task, but the semantic distinction should remain.

A retrieval result should be able to report, where useful:

```text
strategy
index_revision
freshness / consistency level
candidate_count
verified_source_count
fallback_reason
```

This integrates naturally with observable/reproducible `ContextPlan` reasoning.

#### 5. Correct fallback when an index cannot safely narrow a query

The lexical query planner must never produce false negatives merely to use the index.

Required principle:

```text
safe candidate reduction available
→ index

safe reduction unavailable / index unavailable / index suspect
→ exact scan or another correct strategy
```

Optimization failure must degrade performance, not correctness.

#### 6. Derived index corruption must not become project corruption

`tgrep` has had real bugs around malformed/corrupt on-disk index input, including an August 2026 issue where untrusted `lookup.bin` offset/length values could cause panic or excessive work. The project subsequently bounded posting decoding and also has recovery behavior for corrupt indexes.

For AI Engineering OS:

```text
corrupt repository index
!=
corrupt project
```

Expected behavior:

```text
index validation failure
→ mark derived index DEGRADED/INVALID
→ direct-scan fallback where feasible
→ rebuild derived index
```

Do not silently return incomplete search results from a known-corrupt index.

#### 7. Byte-bounded caches, not only entry-count bounds

`tgrep` documents a concrete failure mode where entry-count-only caching allowed a huge file to retain enormous memory. Its current server bounds cache size by:

```text
max entries
max total bytes
max single-entry bytes
```

Adopt the generic resource-budget pattern for `agentd` caches:

```text
CacheBudget
├── max_entries
├── max_total_bytes
└── max_single_entry_bytes
```

This should later apply to repository text, parsed resources, AST/index data and other large derived artifacts where appropriate.

#### 8. Memory-bounded index construction

`tgrep` moved its default full index build to an external merge-sort design: bounded arenas spill sorted segments to disk and k-way merge them into the final index rather than retaining the whole inverted index in memory.

This is highly relevant to a long-running multi-agent daemon. Index construction must coexist with provider processes, context compilation, SQLite and multiple executions without causing avoidable memory spikes.

Future index builders should have explicit resource budgets such as:

```text
IndexBuildBudget
├── max_memory_bytes
├── concurrency
├── io_priority/budget
└── background_priority
```

The exact implementation remains an adapter concern.

#### 9. Progressive-disclosure search for coding agents

`tgrep`'s agent guide recommends broad searches returning filenames first, then narrowing to specific files and using only small context windows. This strongly aligns with our L0→L4 context model.

Adopt the interaction pattern:

```text
query
  ↓
matching-file/symbol census
  ↓
selected locations
  ↓
bounded snippets
  ↓
exact source read only where needed
```

This reduces tool-output volume and model attention cost without reducing source availability.

### ADAPT

#### `tgrep-core` → future `RepositoryTextSearchPort` adapter

`tgrep-core` is already separated as a library crate and contains the reusable pieces we care about: trigram extraction, repository walking, binary index building/reading, regex query decomposition, live index and hybrid index.

However, it should not become a domain dependency.

Target boundary:

```text
Context / Resource application layer
          ↓
RepositoryTextSearchPort
          ↓
TgrepLexicalIndexAdapter       # optional future adapter
          ↓
tgrep-core or independently implemented equivalent
```

`RepositoryTextSearchPort` is the stable architecture concept. `tgrep-core` is only one possible implementation.

Before adopting the crate directly, re-review its exact release, dependency tree, binary/index format behavior, ignore/file-size semantics, corruption handling and supply-chain/security profile.

Current `tgrep-core` dependencies include `serde`, `serde_json`, `anyhow`, `blake3`, `memmap2`, `memchr`, `regex`, `regex-syntax`, `rayon`, `ignore`, `globset` and `encoding_rs`. This is acceptable for an infrastructure adapter in principle, but far too concrete to leak into Context or Resource domain contracts.

#### Indexed lexical search + Code Intelligence are complementary

Do not confuse text search with semantic code intelligence.

```text
RepositoryTextSearchPort
→ bytes/text/regex/literals/files

CodeIntelligencePort
→ symbols/references/definitions/types/semantic relationships
```

The retrieval planner may combine both. Neither should absorb the other merely because both can answer some symbol queries.

#### Ignore/filter semantics must be observable

`tgrep` deliberately applies ignore rules, binary filtering and default file-size limits. Indexed and direct-scan modes can also differ for some options.

Our search contract must make material exclusions visible enough that an agent can distinguish:

```text
no match found
```

from:

```text
source not searched because of scope / ignore / size / capability policy
```

This is especially important when retrieval evidence feeds verification.

### DEFER

#### 1. Direct `tgrep-core` dependency until G9

Do not add it during G3/G4/G5 merely because it is useful.

Before adoption, run our own representative benchmark against at least:

```text
ripgrep/direct scan
trigram/indexed lexical candidate search
FTS5/BM25 where applicable
AST/symbol lookup
```

across:

```text
small repository
medium repository
large monorepo
cold index
warm index
recently edited files
branch/tree replacement
literal symbol
regex with strong literal
broad regex
high-result-count query
```

Measure:

```text
query latency
index-build latency
peak/steady RSS
index disk size
freshness lag
candidate reduction
fallback frequency
correctness parity
context/tool-output volume
```

`tgrep` publishes strong project-reported benchmarks and wins most of its current measured repo/platform cells, but performance varies materially by repository/platform/query. Its own current results include a near-tie/loss against ripgrep for Kubernetes on Linux, so “indexed is always faster” must not become an architecture assumption.

#### 2. Persistent index format

Do not adopt `.tgrep/` as a canonical product format.

Any repository-search index in AI Engineering OS is:

```text
derived
versioned
rebuildable
discardable
```

The source repository remains authority.

### REJECT

#### Standalone `tgrep serve` TCP daemon in the production architecture

`tgrep serve` uses a localhost TCP JSON-RPC server with a discovered PID/port file. That architecture is reasonable for its CLI use case, but does not fit our local control-plane trust model.

We already deliberately use:

```text
agentd
+
OS-authenticated UDS / Named Pipe
+
versioned Protobuf
```

Do not introduce:

```text
agentd
  ↓
localhost TCP / JSON-RPC
  ↓
tgrep serve
```

as a required internal service.

If tgrep technology is adopted, prefer an in-process infrastructure adapter around `tgrep-core`, or an independently implemented equivalent behind our own port and security boundary.

Also reject:

- treating the search index as authoritative repository state;
- assuming watcher notifications provide complete change history;
- returning stale/partial indexed results without freshness/status signals when correctness matters;
- making embeddings the fallback for every lexical query;
- loading unrestricted search output directly into model context;
- copying tgrep's CLI/server topology into the product simply because the search algorithm is useful.

### Benchmark interpretation

The repository reports large improvements over ripgrep on many large repositories/platforms and explicitly notes that the margin depends on repository size, platform, query selectivity and output volume. Benchmarks measure search with an index already built, and the project separately measures index-build behavior.

Treat these results as **useful engineering evidence, not universal product truth**. Our own benchmark must include the indexing/freshness/resource costs relevant to a continuously running `agentd`.

### Security / reliability notes

Positive signals:

- MIT license;
- dedicated `tgrep-core` boundary rather than CLI-only code;
- CI, fuzzing and compatibility testing exist in the repository;
- recent releases addressed corrupted-index bounds handling, watcher/reconciliation behavior, memory bounding and polling fallback;
- project documentation explicitly explains stale/partial-index behavior to coding agents.

Risks/constraints to retain in our review:

- on-disk indexes are untrusted/derived input and require strict bounds/version validation;
- mmap/binary-index parsing must fail cleanly on corruption;
- watcher/index freshness is eventually reconciled rather than magically instantaneous;
- filesystem ignore/max-size rules can affect completeness;
- memory budgets need both total and single-object bounds;
- a local TCP server is not compatible with our accepted IPC security boundary as a core internal control service.

### Gate placement

**Do not modify the current G3 sequence.**

Primary placement:

```text
G9 — Context Engine + Continuity
```

Candidate G9 work:

```text
RepositoryTextSearchPort contract
→ direct-scan reference adapter
→ benchmark harness
→ optional trigram indexed adapter
→ freshness/reconciliation model
→ ContextPlan observability integration
```

`CodeIntelligencePort` integration may follow when structural/symbol intelligence is activated.

### Decision summary

```text
ADOPT
- indexed lexical candidate reduction + exact verification
- immutable snapshot + live overlay
- watcher as low-latency hint, reconciliation as correctness backstop
- explicit retrieval freshness/consistency
- correctness-preserving direct-scan fallback
- byte-bounded caches
- memory-bounded external index builds
- progressive-disclosure search
- corrupt derived index → degrade/rebuild, never corrupt project truth

ADAPT
- tgrep-core ideas behind RepositoryTextSearchPort
- combine lexical retrieval with BM25/AST/Git/embeddings through a planner
- expose search scope/freshness/fallback provenance

DEFER
- direct tgrep-core dependency until G9 benchmark/security review
- persistent index-format choice until retrieval contracts stabilize

REJECT
- standalone tgrep serve TCP/JSON-RPC daemon inside our production control plane
- .tgrep as canonical product data
- watcher/index as repository authority
```

### Primary sources reviewed

- Repository / README: https://github.com/microsoft/tgrep
- Agent usage/freshness guidance: https://github.com/microsoft/tgrep/blob/main/AGENTS.md
- Benchmarks: https://github.com/microsoft/tgrep/blob/main/BENCHMARKS.md
- Core dependency manifest: https://github.com/microsoft/tgrep/blob/main/tgrep-core/Cargo.toml
- Server/watch/reconciliation/cache implementation: https://github.com/microsoft/tgrep/blob/main/tgrep-cli/src/serve.rs
- Releases: https://github.com/microsoft/tgrep/releases
- Corrupt index issue #98: https://github.com/microsoft/tgrep/issues/98
- MIT license: https://github.com/microsoft/tgrep/blob/main/LICENSE

---

## Graphify-Labs/graphify — deterministic code/document knowledge graph and graph-assisted retrieval

**Reviewed:** 2026-09-10  
**Reference:** `Graphify-Labs/graphify` (`v8` development branch)  
**Observed package version:** `graphifyy 0.9.57` in current `pyproject.toml`  
**Current declared license:** Apache-2.0; `NOTICE` states portions contributed before relicensing remain available under MIT and `LICENSE-MIT` is retained.  
**Decision:** **ADAPT — HIGH VALUE for G9 Code Intelligence / Context, later Review Packet and architecture explorer; do not adopt Graphify as a core runtime dependency.**

### Why it matters to AI Engineering OS

Graphify attacks a problem that is directly relevant to this project: an agent should not repeatedly rediscover repository structure by reading arbitrary files or stuffing a whole repository into context. It builds a deterministic AST-derived code graph, augments it with document/rationale relations, and exposes graph queries such as node explanation, neighborhood traversal, shortest paths and affected/blast-radius traversal.

The useful architectural idea is **not** “replace search/RAG with one graph.” The useful idea is a separate structural evidence layer that can cooperate with exact lexical search, symbol lookup, Git evidence, requirements/ADRs and optional semantic retrieval.

Target adaptation:

```text
Repository sources / docs / ADRs / requirements
          │
          ├── exact lexical retrieval
          ├── AST / symbol extraction
          ├── Git revision + diff evidence
          └── typed resource/document extraction
                    │
                    ▼
          Project Structural Evidence Graph
                    │
          ┌─────────┼─────────────┐
          ▼         ▼             ▼
     neighborhood  path       impact/blast radius
          │         │             │
          └─────────┼─────────────┘
                    ▼
             Retrieval Planner
                    ▼
          exact source verification
                    ▼
               ContextPlan
```

The graph remains derived/rebuildable evidence. It does **not** become canonical Task, Git, requirement, architecture or source-code truth.

### What Graphify currently demonstrates

The current README describes local tree-sitter AST extraction for code, cross-file `calls` / `imports` / `inherits` and related edges, community detection, `query`, `path`, `explain`, `affected`, document/rationale nodes, global/cross-project graph composition, PR/worktree impact views, and optional MCP exposure. Code parsing is local/no-LLM; semantic extraction for docs/media may use an agent/provider. The architecture document separates detection, extraction, build, clustering, analysis, report/export, cache, security, validation, serving and watch stages.

The current package manifest is Python 3.10+ and depends directly on NetworkX, NumPy, RapidFuzz and a broad set of tree-sitter language packages. Optional extras add MCP/Starlette, Neo4j, FalkorDB, document/media tooling and several model/provider SDKs. This is useful as a reference implementation, but a poor fit as a foundational dependency for our Rust `agentd` and strict replaceable-port architecture.

### ADOPT — architectural ideas

#### 1. Structural graph is a first-class retrieval strategy

Add structural graph traversal as a peer of lexical/BM25/embedding retrieval rather than burying it inside RAG.

Relevant operations:

```text
get_node / resolve_symbol
neighbors
shortest_path
incoming/outgoing relationships
import/call cycles
affected / reverse-dependency traversal
subsystem/community candidates
```

This fits our existing `CodeIntelligencePort`; it does not replace `RepositoryTextSearchPort`.

#### 2. Evidence class and confidence are separate

Graphify distinguishes `EXTRACTED`, `INFERRED`, and `AMBIGUOUS` edges and stores an explicit confidence score. We adopt the separation between **evidence origin** and **confidence**, but strengthen it:

```text
StructuralRelationEvidence
- relation
- source_ref
- source_location
- source_revision
- extraction_method / extractor_version
- evidence_class: EXACT | DERIVED | SEMANTIC_INFERENCE | AMBIGUOUS
- confidence (optional calibrated estimate)
- provenance
```

A high confidence value never creates authority. Exact source remains verifiable and wins when graph evidence conflicts with source.

#### 3. Code → rationale / ADR / requirement traceability

Graphify promotes `NOTE` / `WHY` / ADR/RFC references into nodes connected to code. This is especially valuable for our existing `ChangeSpec` and requirement-traceability architecture.

Future graph view should support paths such as:

```text
Requirement
→ ChangeSpec revision
→ Task
→ changed symbol/file
→ verification evidence
→ ADR / rationale
```

This is more valuable to our product than generic “knowledge graph” visualization because it can answer *why does this code exist, what requirement owns it, and what evidence would be affected by changing it?*

#### 4. Blast-radius / affected traversal

Graphify's `affected` traversal follows selected directed relations such as calls/imports/references/inheritance. Adapt this into a future `ImpactAnalysis` capability.

Candidate uses:

- context planning before an execution;
- test-selection candidates;
- review packet construction;
- identifying dependent symbols/modules;
- security change scope;
- Delivery Reconciler / merge-risk hints.

Impact output must retain the path/relation/location that caused each hit. A raw “affected=true” label without evidence is insufficient.

#### 5. Progressive graph disclosure

Prefer:

```text
resolve seed
→ bounded neighborhood/path/impact summary
→ selected symbols/files
→ exact source reads
```

rather than dumping an entire graph/report into model context.

Graphify's own code benchmark is a useful signal that graph tooling can improve repository QA, but its reported code suite currently uses only six graded questions and around 140K tokens/query. That is nowhere near enough evidence to make graph-first retrieval mandatory for our product. Our G9 benchmark must measure answer quality **and** token/latency cost.

#### 6. Revision-bound structural indexes

A graph must carry the repository/source revision it represents. Queries need freshness evidence just like the tgrep-derived lexical index design.

Conceptually:

```text
StructuralGraphSnapshot
- repository_ref
- revision / source manifest hash
- extractor version
- corpus definition
- build completeness
- created_at
```

A stale graph can still be useful candidate evidence, but must never masquerade as current source truth.

#### 7. Incremental rebuild must preserve corpus integrity

Graphify's issue history is particularly useful here. It has encountered manifest shrinkage, differing corpora between rebuild paths, absolute/relative path drift, document-only deletion gaps and graph overwrite/shrink failures. These are exactly the failure modes a durable background indexer will meet.

Adopt stronger invariants:

```text
incremental update
!= changed-files-only new corpus

unchanged source entries remain represented unless source deletion is proven

failed / hollow / incomplete extraction
!= successful complete rebuild

partial derived graph
must not replace a known-good graph without explicit degraded state
```

Each build/reconcile path must use one canonical corpus-definition function. Hook/CLI/background-worker implementations may not independently decide which files belong to the graph.

#### 8. Build completeness is explicit

Introduce a future derived-index health record:

```text
IndexBuildEvidence
- expected source count/hash
- processed source count/hash
- failed sources
- omitted sources
- extractor failures
- partial flag
- prior graph revision
- produced graph revision
```

The publish step should refuse an unexpectedly catastrophic shrink or incomplete corpus unless policy explicitly authorizes replacement.

This is stronger than a node-count-only shrink guard: semantic changes can legitimately alter node count, while source coverage is a more direct integrity signal.

#### 9. Graph artifacts are untrusted and rebuildable

Graph JSON/index data can be malformed, stale or produced by an older extractor. Treat it the same way as the tgrep-derived index principle:

```text
corrupt graph
→ INVALID / DEGRADED
→ rebuild / fallback
!= corrupt canonical project state
```

Readers must perform schema/version/bounds validation before traversal.

#### 10. Community detection is a heuristic view, not architecture truth

Leiden/community clustering and “god nodes” are useful for:

- architecture explorer UI;
- context candidate grouping;
- repo onboarding;
- merge/review-risk hints.

But detected communities must never silently become bounded-context ownership, module policy or Task authority. They are derived hypotheses and should expose algorithm/revision/settings provenance.

#### 11. Cross-project graph as a namespaced overlay

Graphify prefixes project node IDs and can merge project graphs into a global graph. Adapt the idea for future multi-repository/team views, but not as one mutable `global-graph.json` authority.

Prefer:

```text
ProjectGraph A ─┐
ProjectGraph B ─┼─→ CrossProjectGraphView / federation
ProjectGraph C ─┘
```

Project identity stays explicit. Cross-project dedup/edges are derived and reversible. An external-library node matched by label alone is not enough evidence for canonical identity.

#### 12. Temporal graph / graph diff

Graphify's benchmark harness includes hundreds of historical ERPNext AST checkpoints. The lesson is useful: code intelligence should eventually support revision-to-revision structural comparison.

Potential product uses:

```text
architecture drift
new/removed dependency edges
blast-radius change
review packet before/after graph
risk hot-spots
```

Do not persist full snapshots forever by default. A content-addressed/rebuildable representation or checkpoint+delta strategy should be benchmarked.

#### 13. PR/worktree impact views

Graphify exposes PR dashboard ideas and “PRs sharing graph communities” as merge-risk hints. Adapt this later into our existing Review Packet + Delivery Reconciler architecture:

```text
Worktree diff
→ changed symbols
→ graph impact paths
→ overlapping impacted regions across executions/PRs
→ merge/review risk evidence
```

Community overlap alone must not determine merge order or block delivery. It is a candidate risk signal.

#### 14. Query observability — opt-in and bounded

Graphify includes local query logging controls. We already require telemetry off by default. If our Context/Code Intelligence layer logs retrieval decisions, retain only bounded metadata/provenance by default and keep full retrieved payload logging separately opt-in/sensitive.

Useful fields:

```text
query kind
repository revision
tool/strategy
candidate/returned counts
latency
context budget
freshness/fallback
```

Do not turn source snippets or private repository content into routine diagnostic logs.

#### 15. Architecture documentation parity tests

Graphify's architecture document has tests that import symbols named in its module-responsibility table so docs cannot drift silently. Adapt the principle for generated/checkable architecture facts:

- crate dependency maps;
- contract/ADR references;
- supported event/schema versions;
- registered provider/tool capabilities.

Do **not** make free-form prose mechanically generated just to satisfy tests; validate factual architecture claims that can actually be derived from code.

### ADAPT — security/privacy lessons

Positive patterns worth retaining:

- AST parsing instead of executing repository source;
- no `shell=True` for subprocesses;
- explicit URL SSRF checks and redirect revalidation for ingestion;
- output/download size caps;
- path-containment validation;
- HTML/text sanitization for graph labels;
- source content treated as untrusted when passed to an LLM;
- code-only mode can stay entirely local;
- credentials are not stored in graph artifacts.

For our system, prompt-injection delimiters/sentinel neutralization are only defense-in-depth. They are **not** a security boundary. Resource/semantic extraction still passes through Policy/Runner/provider isolation and provenance rules.

### DEFER

- a concrete graph storage engine decision (SQLite tables vs embedded graph index vs another representation) until G9 benchmarks;
- embeddings/semantic graph edges as an optional augmentation, not base truth;
- community detection engine choice;
- cross-project federation;
- temporal graph storage;
- PR/worktree graph-impact UI;
- optional Graphify import/interoperability adapter;
- hyperedges/n-ary relation storage until a real use case beats explicit typed relations;
- direct dependency on Graphify itself.

### REJECT

#### Graphify as a core runtime dependency

Do not embed the Python package into `agentd` as a foundational service. Reasons:

- Python runtime/lifecycle would become part of core availability;
- broad NetworkX + many tree-sitter dependencies conflict with our intentionally small Rust runtime boundary;
- optional integrations add substantial provider/MCP/database/media surface;
- its graph schema and lifecycle are Graphify product choices rather than our canonical contracts;
- our `CodeIntelligencePort` must remain replaceable.

A future benchmark may still compare Graphify as an external reference implementation.

#### MCP/HTTP as internal authority

Graphify can expose stdio MCP and optional HTTP. Do not make its MCP/HTTP server an internal control plane or authoritative code-intelligence service inside `agentd`.

If MCP integration is later useful, it should be an external/plugin adapter behind our own capability/security boundary. Existing OS-authenticated local IPC remains the product control plane.

#### “Graph first” as a mandatory retrieval rule

Do not copy Graphify's strict-hook approach that redirects raw source reads to the graph as a universal policy. Our Retrieval Planner chooses between exact lexical, structural graph, symbols, Git, documents and optional semantic retrieval according to query/risk/freshness. For correctness-sensitive evidence, exact source verification is still required.

#### Graph community as domain truth

Do not let clustering determine architectural boundaries, ownership, task decomposition, security policy or merge authority.

#### One giant JSON graph as canonical storage

Do not use `graph.json`/`global-graph.json` as canonical project state. Large monolithic JSON is acceptable as an export/interchange artifact, not as durable authority for a long-running multi-agent control plane.

### Reliability / maintenance findings from current repository

The project is active and fast-moving. The current `v8` branch reports package version `0.9.57`, while the GitHub Releases page indexed by search lagged behind at `0.9.47`; source manifest is the better current-version signal here.

Current/recent issue history reveals several lessons relevant to our design:

- an open issue reports divergent corpora between hook rebuild and `/graphify` skill paths when Markdown extractors are included by one path but not the other; this shows why corpus membership must have one canonical owner;
- a recent issue documented a graph being silently replaced by a drastically smaller graph after hollow/invalid LLM extraction responses were not classified as incomplete; even though fixes evolve, the broader lesson is to gate publication on explicit source-coverage/completeness evidence;
- historical incremental-manifest bugs caused unchanged files to be rediscovered as new after the manifest was rewritten from only the changed subset;
- path-root/absolute-vs-relative bugs have appeared in incremental/update paths, reinforcing that source addressing must be revision/root aware and portable;
- the security document currently lists “0.3.x” as supported even though the package manifest is at 0.9.x; it also says graph analysis makes no network calls except explicit ingest, while the current README documents provider-backed semantic extraction for docs/media. These are concrete examples of security/prose drift despite otherwise good architecture-doc parity practices;
- the code-intelligence benchmark is promising but currently only `n=6`, so it is far too small to justify adopting Graphify's retrieval policy wholesale.

These are not arguments that Graphify is poor quality; they are valuable evidence of the edge cases that emerge once a derived structural index becomes continuously updated and agent-facing.

### Benchmark interpretation

Project-reported results include:

- LOCOMO recall@10 `0.497` for graph expansion;
- LOCOMO QA `45.3%`;
- LongMemEval-S QA `76%`, tied with dense RAG in its harness;
- ERPNext code QA key-fact coverage `70.8% → 82.0%` when a graphify tool is added to a fixed agent, but only `n=6` questions and about `140K` tokens per query.

The memory harness has useful fairness controls (same model/budgets and published judge-validation details), but it remains a project-owned harness. The code result is especially underpowered statistically. Treat all numbers as **candidate evidence**, not product truth.

Our G9 benchmark should compare at least:

```text
exact lexical only
indexed lexical/trigram
symbol/AST lookup
structural graph
lexical + graph
BM25/FTS
optional embeddings/hybrid
```

Measure:

```text
repository QA correctness
symbol/path accuracy
impact-analysis precision/recall
freshness after edit/delete/branch switch
tokens delivered to model
first-use/cold latency
warm latency
index build/update latency
RSS/disk size
false positive structural edges
failure/corruption recovery
```

### License / dependency decision

The current project declares Apache-2.0 and retains an MIT license for portions predating relicensing. Apache-2.0 is commercially usable, but direct copying/redistribution carries notice/license obligations and would unnecessarily entangle our proprietary product with attribution tracking.

Preferred policy for this review:

```text
study architecture / behavior / tests
→ reimplement independently behind our own Rust ports
→ do not copy source code
```

If we ever import or vendor Graphify code, perform a file-level provenance/license review and preserve required Apache NOTICE/license terms.

### Gate placement

Do not change current G4 implementation sequence.

Primary placement:

```text
G9 — Context Engine + Continuity
```

Secondary later consumers:

```text
G8  Verification        → impacted-test/review evidence candidates
G11 Background          → incremental graph maintenance
later Delivery          → PR/worktree conflict/impact hints
UI Architecture Explorer → structural visualization
```

Do not implement this during current G4 Workspace/Git work.

### Decision summary

```text
ADOPT
- structural graph as a first-class retrieval/evidence strategy
- provenance/evidence class on every relation
- code ↔ ADR/rationale/requirement traceability
- affected/blast-radius traversal with causal paths
- revision-bound graph snapshots
- explicit build completeness and source coverage
- canonical corpus membership shared by every rebuild path
- incomplete derived graph cannot silently replace known-good evidence
- graph corruption degrades/rebuilds; source truth remains intact
- architecture-doc factual parity checks

ADAPT
- Graphify graph operations behind CodeIntelligencePort
- graph + lexical/symbol/Git/document retrieval through Retrieval Planner
- community/god-node analysis as heuristic UI/context signals
- cross-project graph as namespaced federation, not global authority
- temporal graph diff for architecture drift/review evidence
- PR/worktree graph overlap for Review Packet / Delivery Reconciler hints
- bounded opt-in retrieval observability

DEFER
- graph storage engine
- community algorithm
- semantic/LLM edges
- embeddings
- temporal persistence strategy
- cross-project federation
- hyperedges
- Graphify interoperability adapter
- direct Graphify dependency

REJECT
- Graphify Python package as core agentd dependency
- MCP/HTTP Graphify server as internal product control plane
- mandatory graph-first/raw-source-blocking retrieval policy
- graph communities as architecture/task/security authority
- one monolithic graph JSON as canonical product state
- inferred relation/confidence as authorization or completion evidence
```

### Primary sources reviewed

- Repository / README: https://github.com/Graphify-Labs/graphify
- Architecture: https://github.com/Graphify-Labs/graphify/blob/v8/ARCHITECTURE.md
- Benchmarks: https://github.com/Graphify-Labs/graphify/blob/v8/BENCHMARKS.md
- Package/dependencies: https://github.com/Graphify-Labs/graphify/blob/v8/pyproject.toml
- Security model: https://github.com/Graphify-Labs/graphify/blob/v8/SECURITY.md
- License / notice: https://github.com/Graphify-Labs/graphify/blob/v8/LICENSE ; https://github.com/Graphify-Labs/graphify/blob/v8/NOTICE
- Affected/blast-radius implementation: https://github.com/Graphify-Labs/graphify/blob/v8/graphify/affected.py
- Global graph implementation: https://github.com/Graphify-Labs/graphify/blob/v8/graphify/global_graph.py
- Corpus divergence issue #2089: https://github.com/Graphify-Labs/graphify/issues/2089
- Incomplete extraction / graph shrink issue #3105: https://github.com/Graphify-Labs/graphify/issues/3105
- Incremental manifest issue #837: https://github.com/Graphify-Labs/graphify/issues/837
- Update path portability issue #433: https://github.com/Graphify-Labs/graphify/issues/433
- Skill/provider mismatch issue #2513: https://github.com/Graphify-Labs/graphify/issues/2513


---

## `juliusbrussee/caveman` — recoverable context compression and token-efficiency measurement

**Reviewed:** 2026-09-10  
**Decision:** **ADAPT — VERY HIGH VALUE for future production Context Engine / token efficiency.**  
**Integration policy:** study and independently reimplement ideas behind our own ports; do not make Caveman Engine a core dependency.

Repository: https://github.com/juliusbrussee/caveman

### Why this matters to AI Engineering OS

Caveman began as a terse-output skill but the current repository also contains a much more relevant architecture for agent systems: local, type-aware compression of large upstream context/tool payloads, exact recovery handles, cache-aware transformation, token accounting and benchmark methodology.

The useful lesson is not “make the model speak like a caveman.” The useful lesson is:

```text
retain exact evidence
→ expose a smaller derived representation
→ recover exact source on demand
→ pass through unchanged when the optimization is unsafe or not beneficial
```

This aligns strongly with our existing rule that Context is a disposable/materialized view and source evidence remains authoritative.

### Two different Caveman mechanisms must not be confused

The repository documents two materially different layers:

1. **Caveman skill / terse output** — primarily changes model output style. The project's `HONEST-NUMBERS.md` explicitly says this layer does not compress input/context/reasoning and adds roughly 1–1.5k input tokens per turn through the skill instructions.
2. **Caveman Engine/Proxy family** — compresses upstream payloads/tool outputs locally and supports recovery of exact originals.

For AI Engineering OS, the second category is much more important.

### Benchmark evidence and limits

The project's `WRAP-BENCHMARK.md` reports a controlled tool-heavy benchmark:

```text
Direct Claude Code input:     885,793 provider-reported tokens
Caveman wrap + skill:          591,673
Reduction:                     33.2%
Exact-answer checks:           18/18
Case-clustered 95% interval:   14.6%–48.5%
```

This is useful controlled evidence, but it is explicitly `benchmark_counterfactual`, not production billing/customer savings.

The per-shape results make the workload dependence concrete:

```text
log needle          -50.2% input
deployment JSON     -26.4%
CSV outlier         -55.1%
test output         -27.8%
YAML/config         -46.2%
HTML dashboard       +9.9%  (regression)
```

The HTML workload used 140,687 direct input tokens versus 154,641 through Caveman because no useful transform applied while the wrapper/skill overhead was still paid. This is a critical negative result: an optimizer must be allowed to decide **not to optimize**.

The project's own documentation also reports workloads where optimization can lose. This is a feature, not an embarrassment, for our research: it proves the correct architecture must be **adaptive and measurable**, not always-on.

The terse-output skill separately reports large output-token reductions, but its own honesty note warns that input/reasoning are unchanged and per-turn instruction overhead can make already-terse workloads net-negative.

### Current Engine mechanics worth retaining as evidence

The current Engine documentation exposes a registry of **15 specialized compressors** covering JSON, logs, code, diffs, search results, text, HTML, tables, config, tool schemas, tool-schema annotations, TOON, accessibility trees, repetition and terminal output. We should not copy this taxonomy blindly, but it is strong evidence that payload-shape specialization is a practical architecture rather than a theoretical idea.

Caveman's native Context Content Recovery storage is bounded to **512 MiB by default**. When capacity is exhausted, new lossy transforms fail closed to original-byte pass-through rather than evicting existing recovery handles. This specific size is not our target; the **bounded recovery + preserve existing recoverability + pass-through on exhaustion** behavior is the valuable pattern.

The repository's memory-file compressor reports five real fixtures averaging roughly **46% reduction** for repeatedly loaded project instruction/memory files. We do not copy the mechanism as canonical-file rewriting; we adapt the underlying idea to derived, revisioned AgentPolicyView / Execution Envelope rendering.

Caveman Browse demonstrates the same “optimize representation only when it wins” lesson. In its large 200-row operations-table fixture, Playwright ARIA was 15,704 inferred tokens while a focused Caveman query was about **121 tokens (~129.8× smaller)**. On a small checkout form, Playwright ARIA was 67 tokens while the focused Caveman result was **111 tokens (1.66× larger)** because action IDs/recovery/accounting overhead dominated. The useful product rule is progressive disclosure and workload-aware representation, not “our representation is always smaller.”

### ADOPT / ADAPT

#### 1. Recoverable lossy compression — P0

Target:

```text
Exact Resource / Tool Evidence
        ↓ retain exact source
Compressed Context View
        ↓
RecoveryRef
        ↓ on demand
Exact original / exact bounded range
```

The compressed representation is derived evidence only.

Required semantics:

```text
COMPRESSED != AUTHORITATIVE
COMPRESSED != VERIFIED
COMPRESSED != permission
```

If the exact original cannot be safely recovered, do not apply lossy compression.

Permanent candidate rule:

```text
NO RECOVERY → NO LOSSY COMPRESSION
```

#### 2. Compression artifact provenance

Our implementation should go beyond a bare recovery handle:

```text
CompressionArtifact
- source_ref
- source_hash
- source_revision
- compressor
- compressor_version
- original bytes/tokens
- compressed bytes/tokens
- lossiness class
- sensitivity
- recovery_ref
- provenance
```

This supports invalidation, audit, cache decisions and exact re-read.

#### 3. Shape-aware compression

Caveman's strongest architectural idea is that JSON/logs/diffs/code/search/HTML/tables/tool schemas/terminal output should not all be summarized with one generic algorithm.

Future product boundary:

```text
ContextCompressorPort
├── TestOutputCompressor
├── LogCompressor
├── JsonCompressor
├── DiffCompressor
├── SearchResultCompressor
├── CodeStructureCompressor
├── TerminalOutputCompressor
└── DocumentCompressor
```

Examples:

- tests: failures/panics/exact failed names preserved; repetitive passes collapsed;
- logs: errors/stack traces/diagnostic boundaries retained;
- JSON: structure and relevant subtrees retained;
- diff: file/hunk/change structure retained with exact source recoverability;
- search results: diagnostic/security/high-value hits retained with exact references;
- code: signatures/symbols/structure before full bodies when appropriate.

#### 4. Fail-through instead of corrupting context

If parsing/compression fails, recovery storage is unavailable, or compressed output is not meaningfully smaller/safer, return the original/bounded exact representation.

Optimization failure may reduce efficiency; it must not silently reduce correctness.

#### 5. Tool-output compression as a high-value sink

Coding agents repeatedly feed terminal/test/search output back into later turns. The same noisy output can therefore consume context multiple times.

We should retain the trusted exact tool artifact while sending the model a compact, structured view. Recovery remains exact and tool-call/tool-result correlation remains valid.

#### 6. Observe → Shadow → Active rollout

Adopt a staged rollout model:

```text
OFF → OBSERVE → SHADOW → ACTIVE
```

`OBSERVE`: identify token sinks without changing context.

`SHADOW`: generate optimized context but keep baseline model-visible context; compare measurements/counterfactual where possible.

`ACTIVE`: enable optimization only after quality/security/robustness gates pass.

#### 7. Cache-aware optimization

A smaller prompt is not automatically cheaper/faster if it destroys provider prompt-cache reuse.

Conceptual comparison:

```text
40k input, 35k cached, 5k uncached
vs
25k input, 0 cached, 25k uncached
```

Raw tokens favor the second; effective cost/latency may favor the first.

Our Context Planner should therefore account for stable prefixes, cache epochs/invalidation and transformation stability.

#### 8. `EffectiveContextCost`

Do not optimize only `raw_token_count`. Evaluate something closer to:

```text
uncached input cost
+ cached input cost
+ transformation overhead
+ latency
+ expected context reuse
+ recovery/retry risk
```

The final production metric should be tied to verified delivery rather than prompt size alone.

#### 9. Provider-reported measurement provenance

Caveman's distinction between estimated, counterfactual and provider-reported numbers is worth adopting.

Future usage evidence:

```text
TokenMeasurement
- provider/model
- value/unit
- evidence source:
    ESTIMATED
    PROVIDER_REPORTED
    BENCHMARK_COUNTERFACTUAL
    MEASURED_LOCAL
- cache class
- execution_id
- timestamp
```

Savings claims should be similarly classified as inferred/measured/controlled-A-B/verified.

Never present tokenizer estimates as invoice-level savings.

#### 10. Context budget packing signals

Caveman experiments with relevance/recency/error-aware packing. Our richer ContextPlan should consider the same useful signal: error/diagnostic evidence often deserves priority over long successful output.

The concept adapts to our existing scoring model:

```text
relevance
freshness
trust
required
sensitivity
error/diagnostic signal
task phase
structural relation
token cost
attention cost
```

#### 11. Derived compact agent-policy views

Compression of project memory/instruction files is conceptually useful only if the canonical source stays untouched.

For us:

```text
canonical ADR/rules/ChangeSpec
→ validated renderer
→ compact Execution Envelope / provider-specific AgentPolicyView
```

The compact view is derived, versioned and recoverable/rebuildable. Do not let a summarizer overwrite authoritative architecture.

#### 12. Context Efficiency Observatory + automatic promotion/rollback

Long term, optimization needs real execution evidence:

```text
provider tokens
cached/uncached tokens
cost
latency
compression ratio
recovery calls
tool calls
retry count
verification result
human intervention
task type/risk
```

A candidate compressor/policy should be promoted only when A/B evidence shows efficiency gain without quality regression; otherwise reject/rollback it.

### REJECT / DEFER

#### Global terse-output skill as the core mechanism — REJECT

A large repeated “be terse” instruction can itself consume context every turn and does not reduce input/reasoning tokens. Our agents already use structured result contracts; concise output should come from bounded schemas and role-specific instructions, not a large always-on persona prompt.

#### Fixed line caps as correctness strategy — REJECT

Rules such as “keep first N lines” may be useful emergency bounds but are too crude as an authority. A critical compiler/test/security error can appear after the cutoff. Prefer semantic extraction + budget + exact recovery.

#### Tool-schema stripping — DEFER

Tool descriptions affect action selection and argument correctness. Compression of schemas is high risk and should remain experimental until controlled benchmarks prove tool-selection, argument, security and cache non-regression.

#### Text-as-image / pixel context — REJECT for core

Potential token-accounting gains are provider/model dependent and introduce worse exact retrieval, accessibility, provenance and visual/OCR failure modes. At most this belongs in an isolated future experiment/plugin.

#### Caveman Engine dependency — REJECT

Our goal is idea adoption and independent reimplementation inside Rust/local-first boundaries. The runtime dependency would also bring a foreign architecture and split-license considerations with no need when we are reimplementing the ideas.

### Implementation-policy note (not an adoption blocker)

Because our default research mode is independent reimplementation of ideas, Caveman's own privacy/license choices do not determine our architecture. They are useful only as failure/operations evidence. One surfaced current security page describes content-free anonymous CLI telemetry enabled after disclosure with opt-out controls, while another crawled `SECURITY.md` snapshot still states “no telemetry,” illustrating documentation drift. Our canonical product rule remains telemetry off by default / explicit opt-in.

### Recovery-store security lesson

Exact recovery can accidentally become a durable secret sink because terminal/log/tool payloads may contain credentials or sensitive data.

Future recovery retention must pass through sensitivity/Secret policy and have explicit local ownership, encryption/TTL/deletion semantics where required.

Do not durably retain a secret merely because compression wants reversibility.

### Browser / DOM representation lesson

Caveman's browser work illustrates a broader principle: focused structural extraction can make huge DOM/accessibility representations dramatically smaller, but on small pages UID/recovery metadata can make the optimized representation larger.

Conclusion:

```text
progressive disclosure + workload-aware selection
> universal compressed representation
```

### Gate placement

Primary:

```text
G9 — Context Engine + Continuity
```

Later:

```text
Usage Ledger / Model Router → cache/token/cost evidence
G11 Background → observe/shadow benchmarking and optimization learning
Review/Verification → quality non-regression gate
```

Do not interrupt current G4 Workspace/Git work.

### Caveman decision summary

```text
ADOPT / ADAPT
- exact recovery for lossy derived context (P0)
- no-recovery/no-lossy-compression rule (P0)
- shape-aware tool/context compression
- pass-through when unsafe/unprofitable
- tool-output semantic compression
- observe/shadow/active rollout
- provider/cache-aware context optimization
- explicit token-measurement provenance
- error/diagnostic signal in context packing
- compact derived policy/context views, never compressed authority
- execution-level efficiency observatory
- controlled A/B promotion and rollback

DEFER
- tool-schema compression
- exact compressor algorithms and storage backend
- provider-specific cache tuning until provider integrations exist

REJECT
- global terse persona/skill as core token strategy
- fixed line truncation as correctness authority
- text-as-image/pixel compression as core
- Caveman Engine as a core dependency
```

### Primary sources reviewed

- Repository / README: https://github.com/juliusbrussee/caveman
- Honest numbers: https://github.com/JuliusBrussee/caveman/blob/main/docs/HONEST-NUMBERS.md
- Wrap benchmark: https://github.com/JuliusBrussee/caveman/blob/main/docs/WRAP-BENCHMARK.md
- Licensing overview: https://github.com/JuliusBrussee/caveman/blob/main/LICENSE
- Engine Apache-2.0 text: https://github.com/JuliusBrussee/caveman/blob/main/LICENSE.Apache-2.0

---

## `dietrichgebert/ponytail` — solution minimality, reuse-first engineering and complexity control

**Reviewed:** 2026-09-10  
**Decision:** **ADAPT — VERY HIGH VALUE for planning/review efficiency.**  
**Integration policy:** transform the engineering ideas into typed planning/review mechanisms; do not copy an always-on prompt/skill architecture.

Repository: https://github.com/dietrichgebert/ponytail

### Why this matters to AI Engineering OS

Ponytail attacks an earlier layer of cost than context compression. Its central idea is that avoiding unnecessary code/abstractions/dependencies reduces not only LOC today but every future cost of indexing, reading, reviewing, testing, maintaining and re-sending that complexity to agents.

Conceptual distinction:

```text
Ponytail → Structural / Work Efficiency
Caveman  → Context Efficiency
```

The strongest combined principle is:

```text
Do less work.
Retrieve less.
Send less.
Reuse more.
Verify everything.
```

This should become architecture, not a slogan injected into every prompt.

### Core ladder worth adapting

The repository's current ladder is essentially:

```text
1. Does this need to exist? → YAGNI
2. Already in this codebase? → reuse it
3. Standard library?
4. Native platform feature?
5. Already-installed dependency?
6. Can the solution be much smaller?
7. Only then create the minimum new implementation that works.
```

The ladder is explicitly supposed to run after understanding the real code path/root cause. This distinction is important: minimal solution does not mean shallow diagnosis.

### Strongest idea: reuse before write

AI coding agents frequently create new helpers/managers/factories/wrappers that duplicate an existing capability elsewhere in the repository.

We can make reuse evidence-backed through existing/future AI Engineering OS capabilities:

```text
Task needs capability
      ↓
RepositoryTextSearchPort + CodeIntelligencePort + Graph
      ↓
existing symbols/services/ports/patterns/dependencies
      ↓
ReuseCandidateSet
      ↓
Planner / Execution Envelope
```

For example, the Execution Envelope may explicitly surface existing `WorkspaceService`, `GitPort` or `RuntimeService` rather than relying on a generic instruction to “search first.”

### Official agentic benchmark evidence

The repository's 2026-06-18 real Claude Code benchmark reports, for its feature-task aggregate, approximately:

```text
Ponytail vs baseline:
LOC     -54%
tokens  -22%
cost    -20%
time    -27%
```

The reduction varies dramatically by task: large on features with an over-build trap and close to zero on already-minimal CRUD-style work. The published examples include a date-picker task shrinking from roughly **404 added lines to 23** and a color-picker task from roughly **287 to 23** when native/platform capabilities replaced custom implementation. This is exactly the class of waste our reuse/native-capability preflight should detect.

The benchmark also reports that a simple YAGNI one-liner prompt reduced code but once dropped a path-traversal guard in the safety tasks; Ponytail's fuller rules retained the tested safety behavior. This is useful evidence that “be shorter” is weaker than a policy that explicitly preserves safety boundaries.

These are project-reported measurements and should be treated as directional evidence, not guaranteed savings for our workloads.

### Independent 480-build benchmark: the critical robustness warning

An independent repository (`Deepusleepy/ponytail-benchmark`) ran:

```text
24 tasks
× 4 modes (baseline/lite/full/ultra)
× 5 repetitions
= 480 builds
Claude Opus 4.8
```

Reported result at Full was roughly 44% less code / 53% fewer logical statements, while correctness remained about 99% and the tested security attacks did not regress.

However, on **5 of 24 tasks** where edge-case robustness was not explicitly specified, aggressive minimality removed defensive bad-input behavior and robustness dropped sharply. Stronger minimality levels made this effect worse.

This is the most important negative finding for us:

```text
MINIMUM CODE
!=
MINIMUM VALID ROBUST SOLUTION
```

Therefore robustness/security/data-safety floors must survive even when the task specification omits an obvious malformed-input case.

### Independent process-cost warning

A separate Cursor SDK A/B report in Ponytail issue #121 found that leaner output can increase tool calls/tokens/cost on large completion-forced tasks. Representative estimated-cost deltas in that small matrix were approximately **+55% Composer 2.5 Fast, +6% Gemini 3.5 Flash, +5% Claude Sonnet 4.6, -2% Claude Opus 4.8, and -13% GPT-5.5 High**. The benchmark authors explicitly label those costs as SDK usage × public list pricing, not invoice truth.

The pattern is important: for several models Ponytail caused more exploration/tool calls and more tokens while producing a leaner final artifact. That is consistent with a mechanism that spends more agent effort reading/tracing/reusing before writing less code. Model/task interaction matters; there is no universal “minimality saves tokens” rule.

Conclusion:

```text
LOC reduction != token optimization
```

Sometimes spending many additional tool reads to remove a small amount of code is not a good total efficiency trade.

This reinforces our need for a multi-objective Verified Delivery Cost rather than a one-dimensional LOC/token target.

### ADOPT / ADAPT

#### 1. Solution Minimality / Work Avoidance

Introduce a planning/review capability that asks whether new work is necessary before creating it.

It is not a new heavyweight Engine; it is a policy/evaluation concern spanning Planner, Execution Envelope and Review Packet.

#### 2. Reuse Candidate Set backed by retrieval

Turn “already exists in this codebase?” into structured evidence from lexical/symbol/graph retrieval.

Potential Execution Envelope fields later:

```text
existing_capabilities:
  - WorkspaceService
  - GitPort
  - RuntimeService

reuse_required:
  - GitPort for Git operations

forbidden_duplicate_mechanisms:
  - second Git service
  - raw shell abstraction
```

Exact schema is deferred.

#### 3. Complexity Delta in Review Packet

Future reviewer can explicitly identify:

```text
new dependency
new service/abstraction
new persistent state
new public API
new configuration
new background process
duplicated capability
stdlib/native capability ignored
unnecessary wrapper/factory/interface
```

The signal helps reviewers find overengineering; it does not substitute for correctness/security review.

#### 4. Optional Complexity Budget in ChangeSpec

Example:

```text
complexity_budget:
  new_dependencies: 0
  new_services: 0
  new_persistence: false
  new_public_api: minimal
```

Crossing the budget requires explicit rationale rather than silent scope expansion.

#### 5. Typed deliberate simplification + upgrade trigger

Ponytail's debt comments contain a useful idea: every deliberate shortcut should name its ceiling and trigger for replacement.

Use a typed project decision instead of a magic source comment:

```text
SimplificationDecision
- scope
- chosen_solution
- alternatives rejected
- rationale
- known ceiling
- upgrade trigger
- evidence
- ChangeSpec revision
- provenance
```

This can later feed Reconciler / Learning Pipeline so a deliberate simplification is revisited when its measured trigger becomes true.

#### 6. Minimality review as a separate Review Packet dimension

Ponytail's `review` and repository-wide `audit` modes are useful conceptually:

```text
Verification / Review Packet
├── acceptance correctness
├── architecture
├── security
├── test integrity
└── complexity/minimality review
```

Do not mix “can be simplified” with security/correctness severity; each finding retains its category.

#### 7. Root-cause before minimal patch

A tiny symptom patch can be worse than one slightly larger shared fix. The system should trace callers/ownership enough to identify the real shared root cause before applying minimality.

The objective is **smallest correct root-cause fix**, not smallest visible diff at any cost.

#### 8. Task-/role-/risk-aware policy selection

Different roles need different complexity pressure:

- bounded coding worker → strong reuse/minimal-change signal;
- architecture agent → prefer simplest architecture satisfying all invariants, but do not collapse necessary boundaries;
- security reviewer → security evidence dominates minimality;
- research agent → minimal-code rules mostly irrelevant.

Select policy from `RoleProfile + TaskKind + ExecutionPhase + RiskClass`, not a global always-loaded rule.

### Always-on prompt overhead: REJECT as our implementation

Ponytail itself has issues discussing context overhead from many always-loaded skill descriptions (#219). Another issue (#664) showed a version where `lite/full/ultra` rendered instructions were 96/99 lines identical and roughly 5.2k characters each, meaning mode switching changed stance much more than context footprint.

Our architecture should not repeatedly inject a large policy document on every agent turn. The context-overhead issue #219 explicitly proposes a conductor/lazy-loading pattern because every top-level skill description consumes session context. This reinforces our existing progressive-disclosure rule for skills/policies.

Issue #664 measured one release where the rendered `lite`, `full` and `ultra` instructions were about **5.2k characters each and 96 of 99 lines identical**. The product lesson is not that these exact modes are bad; it is that a mode selector is meaningless for efficiency if all modes carry almost the same context payload.

Instead:

```text
canonical structured rules/policy
        ↓
task classification
        ↓
only relevant derived constraints
        ↓
small Execution Envelope / RoleProfile view
```

If modes exist, differences must be semantically real/machine-observable rather than mostly labels/examples over the same prompt body.

### Do not copy `lite/full/ultra` directly

If we expose policy levels at all, they should preserve invariant floors and differ in allowed optimization pressure. A conceptual version might be:

```text
Conservative
Balanced
Aggressive
```

but automatic risk/task selection is preferred over a user-facing mode knob for core orchestration.

Regardless of level, never relax:

```text
security floor
data-loss floor
correctness floor
architecture-invariant floor
explicit acceptance floor
```

### Engineering Efficiency / Verified Delivery Cost

The combined Caveman/Ponytail evidence shows why AI Engineering OS must measure more than tokens or LOC:

```text
EngineeringEfficiency
- verified success
- robustness
- security
- wall-clock
- provider cost
- input/output/reasoning tokens
- cache efficiency
- tool calls
- retries
- human attention
- changed LOC
- dependency/abstraction growth
- future maintenance surface
```

A useful top-level business/engineering KPI later is conceptually:

```text
Verified Delivery Cost
=
total provider/runtime cost
+ retries/recovery
+ verification cost
+ human intervention
per independently verified task/change
```

Example intuition:

```text
Strategy A: 300k tokens, 90% first-pass verified
Strategy B: 180k tokens, 50% first-pass verified
```

B looks better on a token dashboard and may be substantially worse as a production system.

### P0 promotion rule

No minimality/reuse/compression/model/reasoning optimization becomes default merely because it lowers tokens/LOC/latency.

Required:

```text
controlled/project-local evidence
→ correctness/verification non-regression
→ security/robustness non-regression
→ measured total efficiency benefit
→ promotion
```

Otherwise keep experimental/reject.

### Relationship with Graphify/tgrep

Ponytail's strongest “reuse before write” idea becomes much more reliable when combined with our previously adopted retrieval architecture:

```text
Ponytail philosophy
        +
Graphify-style structural retrieval
        +
tgrep/indexed lexical exact discovery
        +
exact source verification
        ↓
evidence-backed reuse decision
```

This is an important synthesis: minimality policy should be powered by repository evidence rather than prompt persuasion.

### Relationship with Caveman

The two repositories operate at different points:

```text
Ponytail
→ prevent unnecessary work/code/complexity from being created

Caveman
→ reduce the context cost of the work/evidence that legitimately exists
```

Work avoidance precedes context compression in the architecture.

### Gate placement

Primary future placement:

```text
Planner / ChangeSpec / Execution Envelope → reuse/minimality constraints
Review Packet                         → ComplexityDelta / overengineering review
G9 Context Engine                     → context efficiency and reuse evidence
Learning/Promotion Pipeline           → measure outcomes and promote policies
Model Router                          → model/reasoning efficiency
```

No change to current G4 Workspace/Git sequence.

### Ponytail decision summary

```text
ADOPT / ADAPT
- work avoidance / solution minimality as a structured concern
- reuse-before-write backed by repository evidence
- stdlib/native/existing-dependency preference where appropriate
- smallest correct root-cause fix
- ComplexityDelta in Review Packet
- optional ComplexityBudget in ChangeSpec
- typed SimplificationDecision with known ceiling + upgrade trigger
- task/role/risk-aware minimality policy
- project-local efficiency benchmarking
- quality/robustness/security non-regression before promotion

DEFER
- exact minimality scoring formula
- user-visible policy levels
- automated repository-wide deletion suggestions until verification/ownership is mature

REJECT
- global large always-on Ponytail prompt
- minimum LOC as an optimization target
- aggressive YAGNI that strips unstated robustness/security behavior
- treating fewer lines as proof of better engineering
- copying lite/full/ultra prompt modes as product architecture
```

### Primary sources reviewed

- Repository / current ladder: https://github.com/DietrichGebert/ponytail
- Agentic benchmark: https://github.com/DietrichGebert/ponytail/blob/main/benchmarks/results/2026-06-18-agentic.md
- Benchmark overview / independent reports: https://github.com/DietrichGebert/ponytail/blob/main/benchmarks/README.md
- Independent 480-build benchmark: https://github.com/Deepusleepy/ponytail-benchmark
- Context-overhead issue #219: https://github.com/DietrichGebert/ponytail/issues/219
- Process-cost A/B issue #121: https://github.com/DietrichGebert/ponytail/issues/121
- Mode-context similarity issue #664: https://github.com/DietrichGebert/ponytail/issues/664
- Minimality review skill: https://github.com/DietrichGebert/ponytail/blob/main/skills/ponytail-review/SKILL.md
- Debt / ceiling + upgrade-trigger convention: https://github.com/DietrichGebert/ponytail/blob/main/skills/ponytail-debt/SKILL.md

---

## Caveman + Ponytail synthesis — Production Efficiency Architecture

The two repositories should not become two plugins/subsystems in AI Engineering OS. Their strongest ideas combine into one coherent optimization architecture.

### Ordered efficiency pipeline

```text
Task Intent
   ↓
1. Work Avoidance / Reuse
   ↓
2. Exact Retrieval / Progressive Disclosure
   ↓
3. Recoverable Context Compression
   ↓
4. Provider Cache Optimization
   ↓
5. Model + Reasoning Routing
   ↓
6. Execution
   ↓
7. Independent Verification
   ↓
8. Efficiency Observatory
   ↓
9. Learning / Promotion / Rollback
```

This ordering prevents a common mistake: spending sophisticated compression/model-routing effort on complexity that should never have been built or retrieved.

### Combined decision table

| Idea | Caveman | Ponytail | AI Engineering OS decision |
|---|:---:|:---:|---|
| Avoid unnecessary work | — | yes | **ADOPT** |
| Reuse before new implementation | — | yes | **ADOPT + CodeIntelligence** |
| Stdlib/native before new dependency | — | yes | **ADAPT** |
| Complexity/overengineering review | — | yes | **ADOPT in Review Packet** |
| Deliberate simplification + upgrade trigger | — | yes | **ADOPT as typed decision** |
| Shape-aware tool-output compression | yes | — | **ADOPT/ADAPT** |
| Lossy compression + exact recovery | yes | — | **ADOPT P0** |
| Context-budget packing | yes | — | **ADAPT** |
| Cache-aware optimization | yes | — | **ADOPT P0** |
| Observe/shadow/A-B rollout | yes | benchmark culture | **ADOPT P0** |
| Large always-on prompt | costly in terse-skill form | yes | **REJECT** |
| Shorter at any cost | unsafe/unprofitable cases exist | robustness trade-off exists | **REJECT** |
| Fixed intensity for every task | — | lite/full/ultra | **ADAPT to risk/task routing** |
| Measurement only by tokens/LOC | insufficient | insufficient | **REJECT** |

### Product differentiator

The potential differentiator is not “token compression.” It is a control plane that can answer, per verified engineering outcome:

```text
What work was avoided through reuse?
What evidence was retrieved and why?
What was compressed and recoverable?
What did caching save?
Why was this model/reasoning level selected?
How many retries/human interventions occurred?
Did the optimization preserve correctness/security/robustness?
What policy should be promoted for similar future tasks?
```

This is materially stronger than reporting a compression percentage.

### Final synthesis rule

```text
Do less work.
Retrieve less.
Send less.
Reuse more.
Verify everything.
Measure the verified outcome, not the vanity metric.
```

The words are explanatory; production behavior must be represented by typed contracts, evidence, routing and measurement rather than one giant system prompt.



---

# External review — `NitrusAphalion/tessra` (2026-09-11)

Repository: https://github.com/NitrusAphalion/tessra  
Related Reddit discussion: “Tessra - Git for Agents” (`r/coolgithubprojects`)  
License: MIT.

## Verdict

**Very high conceptual relevance; no direct dependency/adoption.** Tessra tackles agent concurrency one layer below AI Engineering OS by redesigning version control around semantic code units, proof-bearing landings, scoped capabilities, memory and an operation log. We keep Git/worktree isolation as the v1 substrate and independently adapt the best control-plane ideas.

## Adopt / adapt

1. **Verification standards as data** — typed clauses over deterministic evidence, independent reviews, human approvals and risk.
2. **No self-verification authority** — an author's/agent's own claim can be recorded but never satisfies its delivery gate.
3. **Evidence fingerprints + safe reuse** — cache verification by exact subject/toolchain/config/environment-relevant fingerprints.
4. **Budgeted context packs** — target scope + code graph + tests + memories + active claims + provenance under explicit budget.
5. **Typed memory lifecycle** — provenance, anchors, trust, confidence, staleness, supersession, expiry; auto-captured memories start proposed/untrusted.
6. **Advisory work claims** — expose overlapping scope/intent early without hard locking.
7. **Dependency-aware partitioning** — use structural graph information as a scheduler/planner hint, never Task authority.
8. **Mutation idempotency** — explicit replay keys/contracts for retryable commands.
9. **Gates vs hooks** — gates block; hooks react. Hooks can create evidence/tasks/notifications/agent runs but do not become hidden policy.
10. **Capability delegation monotonicity** — delegated child authority cannot exceed the parent's scope/budget.
11. **Principal-wide revocation** — future revoke semantics cover sessions, processes, capabilities, claims, workspaces, unlanded work and unconfirmed memories.
12. **Multi-altitude recovery** — distinguish op undo, delivered-change revert, and agent/principal revoke.
13. **Delivery reconciliation** — Git/local landing and remote push are a resumable saga, not falsely atomic. Reddit author explicitly describes export/push as idempotent rather than atomic, with remote failure leaving local progress durable and retryable.
14. **Derived semantic indexes** — canonical code/repository state remains authority; Tree-sitter/code graph is rebuildable projection.

## Research only / do not adopt into v1

- replacing Git with a custom VCS;
- unit-level semantic merge as canonical storage;
- signed/chained op DAG as our primary persistence model;
- Tessra's exact standards DSL;
- Tessra verifier isolation model (our target should be stronger);
- Tessra as a runtime dependency.

## Important implementation caveats in Tessra itself

The repository is explicitly pre-release. Documented limitations include single-machine operation, unbuilt hub/sync, verifier processes without container isolation, incomplete Git round-trip, semantic grammars limited to Rust/Python/JS/TS, and macOS/Linux paths not yet exercised. Performance/concurrency figures are project-reported and should not be treated as independent benchmarks.

## Useful failure lesson from Reddit

The author states that `export --format git --branch main --push origin` is **idempotent, not atomic**: local commits/mapping/ref updates can complete before a push fails; retry reuses persisted revision→commit mappings and retries the remote step. This is a good reference for our future Delivery Reconciler: persist each completed local fact, represent publish failure honestly, and retry the external side effect rather than attempting unsafe pseudo-rollback.

## License note

Tessra uses MIT. That is compatible with its adoption goal, but conflicts with AI Engineering OS's current anti-clone/anti-resale requirement for the commercial core. Study ideas independently; do not copy its license strategy.

## Placement

- G6: capability delegation, principal revocation, verifier isolation
- G7: WorkClaim + dependency-informed partitioning
- G8: standards/evidence/approval gates
- G9: budgeted context packs + derived code intelligence
- G10: typed memory lifecycle/staleness/trust
- G11: hooks/routines/watchdogs
- Future Delivery Reconciler: idempotent landing/publish saga
- Research: semantic unit merge

Detailed analysis is preserved in the merged research appendices below.

**Current G5/TASK-0051 scope unchanged.**

---

# External review — `anthropics/claude-code` (2026-09-11)

Repository: https://github.com/anthropics/claude-code  
Official docs: https://code.claude.com/docs/en/overview  
License posture: Anthropic copyright / all rights reserved; use subject to Anthropic commercial terms.

## Verdict

**Very high architectural/product relevance; zero code/dependency adoption.** Claude Code is both a real provider used by AI Engineering OS and a mature reference for agent harness design. The public repo is not an open-source core implementation, so conclusions are based on public repository material and official product documentation. We independently reimplement concepts only.

## Adopt / adapt

1. **Provider harness loop** — gather context → act → verify, while keeping Task completion authority outside the provider.
2. **One engine, multiple thin surfaces** — terminal/desktop/IDE/web/mobile/CI/MCP are adapters/projections rather than separate workflow implementations.
3. **Progressive disclosure** — metadata-first skills/tools, full schemas/docs only on demand; use explicit context/privacy budgets.
4. **Subagent context isolation** — bounded delegated objective, isolated tool trace, bounded summary + evidence refs back to parent.
5. **Dynamic fan-out** — many bounded workers + independent cross-checking, but generated workflows operate through canonical Task/Policy APIs.
6. **Agent-team coordination** — manager/worker roles, durable shared work state and messaging; map to RoleProfile + Task Graph + typed AgentMessage rather than provider-specific session state.
7. **Cross-session messaging** — useful as information transport; messages never directly mutate canonical Task state.
8. **Cross-surface continuity** — useful UX, but canonical continuity is repository/evidence/domain-state based and provider-neutral.
9. **Multi-layer recovery** — provider/file checkpoint, Git revert, Task recovery and remote side-effect reconciliation are distinct mechanisms.
10. **Interaction mode vs hard policy** — Plan/Manual/Auto-like UX modes are separate from enforced allow/deny/capability rules.
11. **Managed policy precedence** — organization restrictions cannot be weakened by project/user/session preferences.
12. **Sandbox scope** — filesystem + network + process + credentials; security does not rely on prompt instructions.
13. **Advisor/escalation role** — invoke a stronger/independent model for difficult decisions through risk/capability/cost/capacity routing.
14. **Compaction is optimization only** — never persistence or handoff authority.
15. **Hooks vs gates** — hooks react/validate/produce evidence; policy/verification gates decide transitions.
16. **Durable routines** — scheduled/event-triggered work should have explicit triggers, capabilities, budgets, evidence and audit history.
17. **Plugin package + marketplace** — manifest, capability declaration, dependency constraints, provenance, organization allow/deny, resolved install ledger/rollback.
18. **Deferred tool contracts** — cheap discovery descriptors first; full schemas only when selected.
19. **Enterprise provider gateway** — central credentials, model allowlists, spend/quota policy and audit, while preserving local/BYO-provider paths where possible.
20. **Remote-worker identity** — explicit session/worker identity, scoped credentials, controlled egress and lifecycle/health metadata.

## Already present / reinforced

- worktree isolation;
- provider adapters;
- thin Tauri UI;
- progressive/lazy context loading;
- provider compaction not persistence;
- Task != Execution != Attempt != Session;
- provider-specific compatibility files are not canonical project truth;
- Policy Engine + OS isolation direction;
- future plugin marketplace;
- future background routines and Remote Worker.

## Do not adopt

- Claude Code repository code or plugin/example implementation code;
- provider transcript/session JSONL as project authority;
- Claude-native memory as canonical Memory Engine;
- provider-specific permission-mode strings in domain semantics;
- raw cross-session text as authoritative command/state mutation;
- generated workflow scripts as independent orchestration authority;
- session-centric UI as the primary product object;
- Claude-specific agent-team/task/message types in provider-neutral core;
- any assumption that provider checkpointing can undo external API/database/deployment effects.

## License / IP lesson

The repository's all-rights-reserved posture is a useful reminder that public GitHub visibility does not imply open-source reuse rights. It strongly reinforces AI Engineering OS policy: analyze behavior/architecture, independently implement concepts, and never copy external repository code or make research repositories runtime dependencies merely because source/assets are visible.

## Placement

- G6: policy hierarchy, capability profiles, sandbox boundaries
- G7: manager/worker orchestration, bounded dynamic workflows, advisor role
- G8: independent verification / hook-vs-gate separation
- G9: progressive disclosure, deferred tools, isolated child contexts, continuation
- G10: canonical typed memory remains independent from provider-native memory
- G11: durable routines/background automation
- G12: remote worker identity/scoped credentials
- G13: managed policy, gateways, spend/audit/team governance
- Marketplace: plugin manifests, permissions, dependencies, provenance/install ledger

Detailed analysis is preserved in the merged research appendices below.

**Current G5/TASK-0051 scope unchanged.**

---

# External review — `openai/codex` (2026-09-11)

Repository: https://github.com/openai/codex  
License: Apache-2.0.  
Decision: **very high conceptual relevance; no code/dependency adoption from this review.**

## Strategic verdict

Codex should be treated as a first-class provider harness/executor and a high-quality architectural reference, not as the product model AI Engineering OS should clone. AI Engineering OS exists one layer above it: durable provider-neutral orchestration, policy, context, verification, continuity, memory, recovery, delivery reconciliation and human-attention management.

## Adopt / adapt

1. **Control plane vs execution plane** — app/server-like orchestration and exec-server-like process/filesystem execution are separate responsibilities.
2. **Async command/event protocol** — accepted command identity + streaming events + terminal outcome; our durable domain/Event Journal remains authority.
3. **Capability negotiation** — clients/workers/providers explicitly declare supported protocol/features; unsupported required capabilities fail closed.
4. **Generated protocol schemas** — canonical Rust contracts generate TS/JSON schemas with compatibility tests and versioned snapshots.
5. **Deterministic command path** — Git/test/lint/filesystem/verification utilities should not require an LLM turn when deterministic execution is sufficient.
6. **Fine-grained permission elevation** — request minimal filesystem/network/secret/process capability deltas instead of broad sandbox escape.
7. **Context-bound approvals** — approval grants are bound to exact action/intent/policy fingerprints and invalidated when material context changes.
8. **Auto-review as advisor** — an independent reviewer can reduce human interruptions, but hard policy remains authoritative and policy may require a human reviewer for sensitive classes.
9. **Testable policy-as-data** — versioned rules, explanations, load-time examples/tests, deterministic conflict semantics; expand beyond command-prefix matching to structured action semantics.
10. **Layered config provenance** — effective values expose which layer/policy produced them and use stable versions for conflict handling; hard policy remains distinct from preference merging.
11. **Model capability preflight** — provider/model/reasoning/modality/tool/quota compatibility is validated before launch when knowable.
12. **Remote execution protocol** — future workers use authenticated identities, leases, URI/resource references, scoped credentials, heartbeat and reconciliation.
13. **Fail-closed security controls** — required sandbox/network/secret/worker controls cannot silently degrade if enforcement metadata is missing.
14. **Serialized idempotent daemon lifecycle** — start/stop/restart/update commands are race-safe; our improvement adds drain/checkpoint/reconcile before disruptive upgrades.
15. **Provider session capabilities** — resume/fork/ephemeral/background process are adapter capabilities, never Task semantics.
16. **Structured output schema** — useful for AgentOutput, but schema validity is not evidence; correctness-changing claims require evidence refs.
17. **Tool/app capability metadata** — useful for approval defaults but treated as publisher claims subject to organization policy and runtime enforcement.
18. **Scoped human blocking** — a decision blocks only the minimal Task/Execution/Attempt scope that depends on it; unrelated work continues.
19. **Hierarchical project guidance** — useful with deterministic scope/precedence, but remains subordinate to system/org policy, Execution Envelope, invariants and ADRs.

## Negative/failure lessons

Public issues provide useful design warnings:

- a model/reasoning combination may be accepted despite catalog incompatibility and then fail/hang → preflight must fail fast;
- approval auto-review routing can obscure whether a human is actually required → policy needs explicit minimum reviewer and UIs need effective routing state;
- auto-review can be blocked by unrelated persistence bottlenecks → security/approval control paths need bounded persistence and watchdogs;
- a successful turn event is not sufficient if durable session materialization failed → canonical durability acknowledgement must precede authoritative completion;
- long-lived app-server processes can die mid-turn → worker/daemon supervision, heartbeats and orphan reconciliation are required.

## 1% improvement requirement

Every adopted Codex idea must be improved in at least one meaningful dimension:

- provider neutrality;
- durability/recovery;
- security/least privilege;
- verification/evidence;
- human-attention efficiency;
- observability/explainability;
- cross-provider continuity.

Examples: worktrees become durable provider-neutral workspace leases; auto-review becomes policy-bounded reviewer routing; thread resume becomes cross-provider ContinuationCheckpoint; daemon update becomes work-aware drain/checkpoint/reconcile.

## Pareto priority

Highest-leverage 20% from this review:

1. control/execution-plane split;
2. fine-grained capability/policy architecture;
3. typed/versioned protocol + generated schemas;
4. provider capability preflight;
5. remote worker/daemon supervision;
6. context-bound approvals/reviewer routing;
7. deterministic non-LLM execution path.

Do not spend equivalent effort cloning Codex TUI/editor/model-picker conveniences; they do not define our product differentiation.

Detailed analysis is preserved in the merged research appendices below.

**Current G5/TASK-0051 scope unchanged.**


---

# Merged detailed research appendices — 2026-09-11

> These appendices preserve the detailed notes that previously lived in
> standalone staging/review files. They are **research evidence, not independent
> architecture authority**. Current decisions are the adoption summaries above,
> `PROJECT_CHARTER.md`, accepted ADRs, invariants and the Architecture Source of
> Truth. Historical gate/task references inside an appendix are informational
> snapshots and do not override current project status.


## Appendix A — Scape / Senior Engineer Loop detailed research

> Status: staging document for later merge into the project Architecture Source of Truth, roadmap, ADRs, and external-research notes.
>
> Purpose: capture all useful product, orchestration, UX, continuity, security, and workflow ideas extracted from the Reddit post “Senior Engineer Loop Orchestrator — Sample Setup” and the accompanying screenshots.
>
> Important: this document does **not** change the current gate or TASK-0051 scope. G5 must still be completed first.

## 1. High-level takeaway

The referenced setup is useful because it demonstrates a real-world multi-agent engineering workflow that already contains several elements close to the direction of AI Engineering OS:

- persistent manager/orchestrator sessions;
- durable internal tickets;
- child coding/review sessions;
- Git worktrees;
- periodic manager wakeups / heartbeat;
- reusable playbooks;
- structured human questions;
- model selection by task complexity;
- automated plan/code review gates;
- secrets stored outside normal prompt text;
- session status, model, cost, and progress projections;
- read-only investigation agents;
- explicit “laws & permissions”;
- reusable engineering toolkit operations.

The important design conclusion for AI Engineering OS is not to copy this setup directly, but to convert the useful ideas into **provider-neutral, durable, enforceable system primitives**.

AI Engineering OS should remain:

```text
Human Intent
    ↓
Durable Task Graph
    ↓
Policy / Constraints
    ↓
Planner / Orchestrator
    ↓
Provider-neutral Executors
    ↓
Isolated Workspaces
    ↓
Evidence
    ↓
Independent Verification
    ↓
Human Attention Queue
```

The product should optimize for managing **work, outcomes, risk, and attention**, not terminal windows or provider conversations.

---

# 2. Mission model

The screenshots show a manager object such as “Lloyd's Mission” with:

- heartbeat;
- maximum children;
- expertise;
- current mission;
- recurring actions;
- settings.

This should become a first-class machine-readable concept rather than a giant prompt.

Proposed concept:

```text
MissionSpec
├── role_profile
├── objective
├── responsibilities
├── cadence
├── max_parallel_children
├── routines[]
├── escalation_policy
├── review_policy
└── resource_policy
```

Important distinction:

```text
RoleProfile
───────────
WHO

Mission
───────
WHAT

Procedure / Routine
───────────────────
HOW

Policy
──────
MUST / MUST NOT
```

Example:

```text
RoleProfile:
  Senior Backend Engineer

Mission:
  Implement TASK-0192

Procedure:
  TDD workflow

Policy:
  cannot merge
  cannot read secrets
  network disabled
  requires independent review
```

This separation prevents persona, workflow, and security rules from being mixed into one opaque prompt.

Likely gate:
- G7 for RoleProfile / Mission / delegation
- G10 for learned or promoted procedural knowledge

---

# 3. Routine / Pulse Engine

The manager in the reference periodically wakes up and performs recurring checks such as:

- inspect bug reports;
- inspect backlog;
- inspect recent PRs;
- inspect documentation drift;
- inspect logs;
- dispatch investigation agents;
- record the result of each pulse.

AI Engineering OS should generalize this beyond a simple heartbeat.

Proposed trigger model:

```text
Routine Engine
    │
    ├── Time trigger
    │     cron / interval
    │
    ├── Event trigger
    │     Task completed
    │     PR created
    │     test failed
    │     provider quota exhausted
    │     new issue
    │
    ├── External trigger
    │     GitHub
    │     Jira
    │     Linear
    │     Slack
    │
    └── Condition trigger
          error rate
          stale task
          blocked agent
          quota threshold
```

Example:

```text
Routine: Repository Health

Trigger:
  every 2h
  OR after merge

Steps:
  inspect failed tests
  inspect warnings
  inspect TODO/FIXME growth
  check documentation drift
  check dependencies
  create findings

On finding:
  create Task
  OR request investigation
  OR ask human
```

A heartbeat should therefore be only one trigger type.

Likely gate:
- G11 Background Daemon / Routines / Watchdogs

---

# 4. Playbook Engine

The screenshots show a “Playbook” with:

- named inputs;
- defaults;
- descriptions;
- secrets;
- variable syntax;
- executable steps;
- outputs;
- next-step flow;
- locked/read-only mode.

This is a strong candidate for a first-class AI Engineering OS primitive.

Proposed:

```text
PlaybookDefinition
──────────────────
id
version

inputs schema
secret references
required capabilities

steps[]
outputs schema

retry policy
timeout policy

allowed filesystem scope
allowed network scope

verification requirements

immutable / locked version
```

Conceptual hierarchy:

```text
Tool
→ atomic primitive

Playbook
→ sequence of tools

Procedure
→ conditional workflow

Routine
→ scheduled/event-driven procedure
```

Examples:

```text
Tool:
  git.create_worktree

Tool:
  tests.run

Playbook:
  prepare_release

Routine:
  weekly_release_health
```

Playbooks should eventually support the marketplace/plugin strategy.

Likely gates:
- G6 for permissions, secrets, execution safety
- G10 for learning/promotion of reusable procedures
- G11 for scheduled/event execution

---

# 5. Immutable / approved playbooks

A particularly useful idea from the screenshots is a locked playbook that can still be executed but cannot be edited by the current agent.

AI Engineering OS should support versioned immutable procedures:

```text
Playbook v7
SIGNED / APPROVED
LOCKED

Agent:
✓ execute
✗ edit
```

If an agent wants to improve it:

```text
Playbook v7
    ↓
proposed v8
    ↓
review
    ↓
approval
    ↓
v8 becomes active
```

This prevents a single agent from simultaneously:

1. changing the rules;
2. executing the changed rules;
3. declaring the result valid.

This aligns with separation-of-authority principles.

---

# 6. Capability profiles and read-only investigators

The reference workflow uses read-only investigation agents before assigning implementation work.

AI Engineering OS should support capability-scoped execution profiles.

Example:

```text
INVESTIGATOR
  repo.read        ✓
  logs.read        ✓
  tests.read       ✓
  git.diff         ✓

  files.write      ✗
  git.commit       ✗
  process.kill     ✗
  network.write    ✗
  secrets.read     ✗
```

Preferred flow:

```text
Investigation
    ↓
Evidence
    ↓
Root-cause hypothesis
    ↓
Task created
    ↓
Implementation Agent
```

Important principle:

> “Do not modify anything” should not exist only as a prompt instruction when the system can enforce the restriction technically.

Likely gate:
- G6 Security / Policy Engine / OS isolation

---

# 7. Soft rules vs hard policy

The reference system contains many “Laws & Permissions” as text instructions.

Some of those are useful as soft procedural guidance, but security-critical and authority-critical rules in AI Engineering OS should be enforced by software.

Example soft rule:

```text
Before creating new infrastructure,
search the repository for an existing solution.
```

Example hard policy:

```text
MERGE:
  requires:
    verification == PASSED
    independent_review == PASSED

SECRET_READ:
  requires:
    explicit capability

DELETE_WORKTREE:
  requires:
    owner_is_live == false
```

The system should distinguish:

```text
Instruction / Procedure
→ agent should follow

Policy
→ system enforces
```

Likely gate:
- G6 Security

---

# 8. Human Decision Queue

The reference workflow batches structured questions to the human and includes a recommended option.

AI Engineering OS should make human attention a first-class system object.

Proposed:

```text
HumanDecisionRequest
────────────────────
task
reason
risk

question
options[]
recommended_option
recommendation_reason

blocking: true / false

related_decisions[]

requested_by
created_at
```

Primary UI should expose something like:

```text
Needs your attention — 3

TASK-164
Architecture choice
Recommended: Option B

TASK-171
Allow network access?
Recommended: Deny

TASK-180
Merge verified change?
Recommended: Approve
```

This should integrate with:

- Human Steering / Takeover;
- Human Attention Metrics;
- approval flows;
- review gates;
- escalation.

Likely gates:
- G7 / G8

---

# 9. Verification and Review Gates

The reference workflow follows a chain similar to:

```text
Brainstorm
    ↓
Plan
    ↓
Independent Plan Review
    ↓
Implementation
    ↓
Independent Code Review
    ↓
Additional Review
    ↓
Human QA
```

AI Engineering OS should make this provider-neutral and risk-aware.

Proposed:

```text
Implementation
    ↓
Verification Policy
    │
    ├── tests
    ├── lint
    ├── architecture review
    ├── security review
    ├── adversarial model review
    └── human review
```

Risk-aware example:

```text
Risk LOW
→ tests only

Risk MEDIUM
→ tests + independent review

Risk HIGH
→ tests
  + independent-provider review
  + human approval
```

A specific provider such as Codex must not be hardcoded as “the reviewer”.

Likely gate:
- G8 Verification

---

# 10. Typed durable agent messaging

The reference setup supports inter-agent communication. AI Engineering OS should implement a stricter, typed, durable form.

Proposed:

```text
AgentMessage
──────────────────────────
message_id
task_id
execution_id
attempt_id

sender
recipient / role

kind:
  INFORMATION
  REQUEST
  REVIEW_REQUEST
  BLOCKER
  RESULT
  HANDOFF
  DECISION_REQUEST

trust:
  UNTRUSTED_AGENT
  SYSTEM_GENERATED
  HUMAN
  VERIFIED_ARTIFACT

artifact_refs[]
workspace_ref
evidence_refs[]

body

requires_reply
reply_to
created_at
```

Critical rule:

```text
Agent message
    ↓
information only

Task mutation
    ↓
canonical command
    ↓
Policy / State Machine
```

An agent message must never directly mutate authoritative Task state.

Agent-originated text should default to an untrusted trust level to reduce prompt-injection risk between agents.

Likely gate:
- G9 Context / Continuity

---

# 11. Cross-Provider Execution Continuity / Model Handoff Protocol

This should become a central future feature.

Goal:

```text
Claude / Provider A
       ↓
quota exhausted / failure / manual switch
       ↓
safe checkpoint
       ↓
durable Handoff Packet
       ↓
Codex / Gemini / Provider B
       ↓
continue same engineering work
```

The system must not depend on the previous provider's chat history.

Source of truth should be durable system state.

Suggested handoff contents:

```text
Task
Execution
Attempt

WorkspaceLease
repository path
worktree path

base commit
current HEAD

git status
git diff
staged diff

changed / added / deleted files

commands already executed
tests already executed
test results

completed acceptance criteria
remaining acceptance criteria

known failures
open problems
blocked actions

relevant ADRs / invariants

generated artifacts

previous provider/model
handoff reason
```

Optional human-readable summary:

```text
What I was trying to accomplish
What I changed
What currently works
What is still broken
What I would do next
Things the next agent must not redo
```

Potential entity names:

```text
ContinuationCheckpoint
ExecutionHandoff
```

Possible relationship:

```text
Task
 └─ Execution
      ├─ Attempt #1 — Claude
      ├─ Handoff checkpoint
      └─ Attempt #2 — Codex
```

Exact Execution-vs-Attempt semantics should be decided later against the final domain model.

---

# 12. Safe checkpoint procedure

Before provider handoff, the system should attempt:

```text
1. stop assigning new actions to the current agent
2. wait for or safely cancel the current command
3. save stdout/stderr/result
4. capture git status
5. capture diff
6. capture current HEAD
7. persist test evidence
8. persist unfinished actions/blockers
9. create Handoff Packet
10. launch replacement provider
```

If the previous model is already unavailable due to quota/failure, the system should still reconstruct most state from:

```text
repository + worktree
Task database
Execution / Attempt state
Event Journal
Git state
test evidence
tool history
artifacts
```

Previous-model prose should be supplemental, not authoritative.

---

# 13. Handoff modes

Suggested modes:

```text
MANUAL_HANDOFF
user explicitly selects another provider

LIMIT_HANDOFF
quota / rate limit exhausted

FAILURE_HANDOFF
provider unavailable or current execution cannot continue
```

Potential later behavior:

```text
Claude quota exhausted
    ↓
ContinuationCheckpoint
    ↓
Model Router
    ↓
Codex capacity available
    ↓
continue work
```

Likely gate:
- G9 Continuity
- later Model Router / Subscription Capacity Manager

---

# 14. Provider Capacity state

The UI screenshots show model/session usage indicators.

AI Engineering OS should eventually model provider availability/capacity explicitly, without making it part of Task semantics.

Possible state:

```text
ProviderCapacity
────────────────
provider
model
availability

quota_remaining
quota_confidence
rate_limit_state

degraded
unavailable
handoff_recommended

observed_at
provenance
```

This can feed the future router.

Important:

```text
Task semantics
X→ provider quota

Router / orchestration
→ observes provider capacity
```

---

# 15. Model Router

The reference hardcodes model choice by complexity. AI Engineering OS should generalize it.

Task profile:

```text
TaskProfile
────────────
complexity: HIGH
architecture_risk: HIGH
context_size: LARGE
tool_use: HEAVY
latency_priority: LOW
cost_priority: MEDIUM
```

Router considers:

```text
available providers
current quota
model capabilities
historical performance
task requirements
subscription availability
cost / latency constraints
```

Then selects:

```text
Claude
Codex
Gemini
Kimi
...
```

Do not hardcode:

```text
if complex → Claude X
```

The router should eventually integrate with handoff and capacity state.

---

# 16. Session Projection, not Session Authority

The reference UI is session-centric and shows:

- active/idle/generating;
- model;
- cost;
- branch;
- context/usage;
- child sessions.

These are useful projections, but AI Engineering OS should keep **Task / Outcome / Risk / Attention / Verification** as the main product objects.

Preferred UI:

```text
TASK-0182
Fix authentication race

Status: VERIFYING
Progress: 82%

Executions:
  Attempt 1 — Claude — failed
  Attempt 2 — Codex — succeeded

Verification:
  tests ✓
  architecture review ✓
  security review waiting

Capacity:
  Claude 18%
  Codex 72%
```

Sessions should be a drill-down technical projection.

Proposed concept:

```text
SessionProjection
```

It must not become authoritative state.

---

# 17. Main attention-oriented UI

Possible future dashboard:

```text
AI Engineering OS
────────────────────────────────────────────

NEEDS ATTENTION                         3

TASK-0138
Authentication refactor
Claude / Attempt 3
⚠ Review found 2 blockers

TASK-0141
SQLite migration
Codex
⚠ Waiting for decision

TASK-0147
Frontend forms
Gemini
⚠ Provider quota exhausted
   Handoff ready → Codex


ACTIVE                                 7

TASK-0142   Claude   coding       68%
TASK-0143   Codex    testing      82%
TASK-0144   Gemini   reviewing    34%

BACKGROUND ROUTINES

Repo health         ✓  8 min ago
Dependency audit    ✓  2 h ago
Documentation drift ⚠ 3 findings
```

Design principle:

> The user manages outcomes and attention, not agent chat windows.

---

# 18. Internal Task Graph remains authority

The reference setup has an Internal Tickets database with multiple projections:

```text
Table
Kanban
Gantt
List
```

AI Engineering OS should preserve its existing stronger architectural direction:

```text
External ingress
GitHub / Linear / Jira / Slack
        ↓
Internal Task Graph
        ↓
single authority
```

External systems are Work Ingress Adapters.

Kanban/Table/Gantt/List are projections over internal authoritative state.

Avoid parallel mutable authorities such as:

```text
GitHub Issue state
+ Linear state
+ internal ticket state
+ chat state
```

without reconciliation.

---

# 19. Toolkit as reusable operations

The reference UI exposes reusable operations such as:

- Worktree;
- bump version;
- release notes;
- release RC;
- build package;
- open project;
- conversion report.

AI Engineering OS should represent reusable operations through the Tool / Playbook / Procedure hierarchy.

This is also a natural foundation for a future plugin marketplace.

Marketplace candidates already envisioned:

```text
prompt improvement
context optimization
review
security checks
documentation
commit / PR preparation
requirements refinement
tests
repository-readiness rules
integrations
release procedures
```

---

# 20. Learning / Promotion Pipeline integration

Repeated successful execution patterns should eventually be promotable:

```text
successful ad-hoc action
    ↓
candidate procedure
    ↓
review/evidence
    ↓
approved Playbook
    ↓
Routine / Skill / Rule
```

Potential promotion destinations:

```text
Memory
Skills
Procedures
Rules
Docs
Playbooks
```

The agent must not autonomously promote a one-off behavior into policy without review/evidence.

---

# 21. Watchdog / stuck-agent detection

A long-running orchestrator needs explicit health monitoring.

Future watchdog signals:

```text
Task stale
Attempt no progress
agent repeatedly executes same action
agent silent
context drift
provider quota near exhaustion
child session orphaned
workspace owner inconsistent
test loop without progress
```

Potential outcomes:

```text
notify manager
request human attention
cancel attempt
create diagnostic Task
create handoff checkpoint
switch provider
```

Likely gate:
- G11

---

# 22. Routine audit trail

Every pulse/routine should produce durable evidence.

Proposed:

```text
RoutineRun
──────────
routine_id
version
trigger
started_at
finished_at

inputs
steps_executed
findings[]
actions_requested[]
tasks_created[]
errors[]

evidence_refs[]
```

This prevents background agents from becoming invisible autonomous behavior.

---

# 23. Resource policies

The screenshots show limits such as “max children”.

AI Engineering OS should generalize this into resource/workspace policies.

Possible:

```text
max_parallel_children
max_active_attempts
max_expensive_models
max_worktrees
max_disk_usage
max_runtime
max_provider_spend
max_networked_tasks
```

These belong to orchestration/resource management, not prompt prose.

---

# 24. Explicit review-before-build rule

One useful operational pattern is:

```text
non-trivial feature
    ↓
implementation plan
    ↓
independent review
    ↓
plan revision
    ↓
implementation
```

This should be configurable per risk class rather than universal.

Possible policy:

```text
trivial:
  no plan review

medium:
  plan required

high-risk:
  plan + independent plan review

security/concurrency/storage:
  plan + independent review + explicit verification strategy
```

---

# 25. Search-before-build rule

The screenshot includes a useful procedural law:

> Before building new infrastructure, search the repository for an existing solution and re-read relevant project instructions when reaching for a tool.

This is a good **soft procedure**, not a security policy.

Possible reusable procedure:

```text
RepositoryReuseCheck
────────────────────
search existing implementation
search relevant ADRs
search current tools/playbooks
search existing test harness
record why reuse is or is not possible
```

This helps avoid duplicate infrastructure.

---

# 26. Human QA should be late-stage

The reference workflow keeps automated review before human QA.

This aligns with Human Attention Metrics.

Preferred:

```text
implementation
    ↓
automated tests
    ↓
independent review
    ↓
security/architecture checks
    ↓
only unresolved or final decision
    ↓
human
```

Humans should receive distilled decisions/findings rather than raw agent chatter whenever possible.

---

# 27. Provider-neutral review roles

Do not encode:

```text
Codex = reviewer
Claude = builder
```

Instead define:

```text
Role:
  BUILDER
  REVIEWER
  INVESTIGATOR
  VERIFIER
  MANAGER
```

Then assign providers/models dynamically.

Example:

```text
RoleProfile: REVIEWER

requirements:
  independent provider from builder
  high reasoning capability
  repo read
  no merge authority
```

---

# 28. Trust labels for inter-agent content

Any agent-originated artifact or message should carry provenance/trust.

Possible:

```text
UNTRUSTED_AGENT_TEXT
PROVIDER_REPORTED
SYSTEM_OBSERVED
HUMAN_PROVIDED
VERIFIED_EVIDENCE
```

This can later influence:

- whether content may trigger actions;
- whether it can be promoted to memory;
- whether it requires verification;
- whether it may be shown as authoritative.

---

# 29. Provenance for learned information

The future Memory Engine and Procedure promotion system should preserve provenance:

```text
learned claim
source Task
source Attempt
provider
evidence
review status
promotion date
superseded_by
```

This prevents agent-generated guesses from silently becoming permanent rules.

---

# 30. Architectural placement by gate

Suggested mapping:

```text
G6 — Security
  Policy Engine
  Capability Profiles
  Read-only Investigator enforcement
  Secrets Broker
  Tool / Playbook permission enforcement
  hard policy vs soft instruction

G7 — Multi-agent DAG / Delegation
  RoleProfile
  MissionSpec
  delegation
  HumanDecisionRequest
  resource limits
  manager / child relationships

G8 — Verification
  ReviewGate
  plan review
  code review
  adversarial review
  risk-aware verification policy
  human QA gate

G9 — Context + Continuity
  AgentMessage
  ContinuationCheckpoint
  ExecutionHandoff
  Cross-Provider Handoff
  bounded continuation context
  context reconstruction

G10 — Memory / Learning
  Learning / Promotion Pipeline
  Procedure promotion
  Playbook promotion
  memory provenance

G11 — Background Daemon
  Routine Engine
  Pulse Engine
  Watchdog
  RoutineRun
  scheduled/event-driven background work

Future Router
  ProviderCapacity
  model selection
  quota-aware handoff
  provider failover
```

---

# 31. Explicit non-goal for current G5 work

None of the ideas in this document should modify:

```text
TASK-0051
G5 atomic entry
managed workspace identity remediation
```

Current sequence remains:

```text
TASK-0051
→ fix G5 entry atomicity / branch identity

TASK-0052
→ independent G5 closure re-audit

only after G5_CLOSED
→ begin G6 architecture prerequisites
```

Do not let future product ideas destabilize the current gate.

---

# 32. Product differentiation

A useful framing after reviewing the reference system:

```text
Other orchestrator:
Agent manages agents.

AI Engineering OS:
Deterministic control plane manages nondeterministic agents.
```

Potential differentiators:

- provider neutrality;
- durable Task / Execution / Attempt semantics;
- strict separation of Task state from provider conversations;
- isolated workspaces;
- independent completion authority;
- typed agent messaging;
- cross-provider continuation;
- policy-enforced capabilities;
- immutable reviewed playbooks;
- evidence-driven verification;
- human attention queue;
- internal Task Graph authority;
- provider capacity / quota awareness;
- future multi-provider handoff;
- local-first privacy.

---

# 33. Future first-class concepts to preserve

The following should be retained as named architecture candidates:

```text
MissionSpec
RoleProfile
RoutineDefinition
RoutineRun
PlaybookDefinition
CapabilityProfile
PolicyRule
HumanDecisionRequest
ReviewGate
AgentMessage
ContinuationCheckpoint
ExecutionHandoff
ProviderCapacity
SessionProjection
```

These names are provisional and may change during their gate-level architecture work.

The concepts are more important than the exact names.

---

# 34. Priority of ideas

## High priority

```text
Cross-Provider Handoff Protocol
Typed Agent Messaging
Policy / Capability separation
Playbook Engine
Human Decision Queue
Review Gates
Routine / Pulse Engine
```

## Medium priority

```text
MissionSpec
Provider Capacity
Session Projection
Watchdog
Resource Policies
Toolkit abstraction
```

## Later / marketplace-oriented

```text
playbook marketplace
community procedures
specialized review plugins
release workflows
external integration packs
```

---

# 35. Core principles extracted from the reference

1. Persistent engineering work must live outside model conversations.
2. A manager agent needs durable tasks, not just child chats.
3. Background routines need explicit triggers and audit trails.
4. Repeated workflows should become versioned procedures/playbooks.
5. Security-critical rules must be enforced by the system, not merely written in prompts.
6. Agent-to-agent messages are information, not authority.
7. Human attention should be batched, structured, and risk-driven.
8. Review should be independent from implementation where risk justifies it.
9. Provider/model choice should be dynamic and quota-aware.
10. A provider limit must not destroy engineering continuity.
11. Session UI is useful, but Task / Outcome / Verification remain primary.
12. External ticket systems should feed the internal Task Graph, not replace it.
13. Durable evidence is more important than conversational memory.
14. The system should support changing the model without changing the Task.
15. AI Engineering OS should orchestrate providers through enforceable contracts rather than prompt conventions.

---

# 36. Merge plan later

When the project is ready to merge these ideas into canonical documentation, review this staging file and distribute content into:

```text
AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md
EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md
docs/architecture/*
docs/architecture/adr/*
planning/roadmap.yaml
```

Only update `AGENT.md` if these ideas eventually become current agent-operating rules.

Do not move future architecture into `AGENT.md` prematurely.


## Appendix B — Observer detailed research

> Source context: Reddit post about Observer AI reaching ~2k users, its move from MIT to AGPLv3, local-first product strategy, possible VC/enterprise path, and the Observer repository architecture.
>
> Status: staging notes for later merge. This file does not change the current G5 scope or roadmap ordering.

## 1. Why this article matters

The Observer article is less about coding-agent orchestration and more about three strategic areas that are highly relevant to AI Engineering OS:

1. local-first product architecture;
2. sustainable open-source business model;
3. separating cheap/local continuous agents from more capable higher-cost orchestration models.

Observer is a privacy-first local AI product that monitors screen/camera/audio inputs and triggers actions. Its current public architecture is summarized as:

```text
Sensors → Models → Tools
```

The important transferable lesson is to keep the model as one replaceable reasoning component between typed inputs and typed capabilities.

## 2. Local-first as product strategy, not just infrastructure

Observer emphasizes that local inference can remain fully local while optional convenience services can be paid or hosted.

For AI Engineering OS this supports:

```text
Core local-first control plane
  - Task Graph
  - agentd
  - workspaces
  - local execution
  - provider adapters
  - local state
  - local verification

Optional commercial services
  - remote workers
  - team synchronization
  - hosted dashboards
  - enterprise governance
  - managed integrations
  - cloud artifact storage
  - organization policy packs
```

Principle:

> The open/local core should remain useful by itself; monetization should come from convenience, collaboration, governance, hosting, scale, and enterprise functionality rather than deliberately crippling local execution.

## 3. Potential business-model principle

Observer's author describes a line similar to:

```text
If it costs me nothing to provide, keep the equivalent local/self-hosted path free.
```

For AI Engineering OS this could become a product philosophy rather than a contractual promise:

```text
Self-hosted/local capabilities
→ generous/open core

Capabilities with ongoing vendor cost
→ paid service is reasonable

Enterprise capabilities
→ paid governance/operations layer
```

Do not encode this as a permanent pricing commitment until product economics are known.

## 4. Open-source core + paid convenience

Possible future commercial split:

```text
Free / Open Core
----------------
local agentd
agentctl
Task Graph
local provider adapters
worktree isolation
local SQLite state
local verification primitives
manual multi-provider operation
local Playbooks
basic local UI

Paid / Commercial
-----------------
team collaboration
hosted synchronization
remote workers
managed cloud execution
enterprise SSO
RBAC / organization policy
audit retention
central fleet management
managed integrations
enterprise support
hosted observability
compliance features
```

This should be evaluated later against the final license.

## 5. Licensing lesson

Observer moved from MIT to AGPLv3 due to concern about proprietary/parasitic forks.

This is strategically relevant to AI Engineering OS, but license selection should be treated as a separate product/legal decision rather than copied automatically.

Candidate directions to evaluate later:

```text
Apache-2.0 / MIT
→ maximum adoption and commercial friendliness
→ weaker protection against closed hosted forks

AGPLv3
→ strong copyleft including modified network-interactive deployments
→ may discourage some enterprise adopters/integrators

Dual licensing
→ open-source license + commercial license
→ requires clean copyright ownership/contributor agreements

Open-core
→ permissive or copyleft core
→ separate proprietary enterprise modules/services
```

Important legal/operational consideration:
if external contributors own copyright in their contributions, later relicensing can become difficult without appropriate rights or contributor agreements.

Before opening the project broadly to contributors, decide whether the project needs:
- Developer Certificate of Origin (DCO);
- Contributor License Agreement (CLA);
- assignment of copyright;
- or no relicensing strategy at all.

This requires actual legal review before implementation.

## 6. Separate code license from product moat

AI Engineering OS should not assume that source-code secrecy is the primary moat.

Possible moat:

```text
architecture quality
provider-neutral execution contracts
safe orchestration
verification quality
integrations
plugin/playbook ecosystem
enterprise governance
user experience
reliability
historical execution intelligence
team workflows
support/trust
```

Open-sourcing a large portion of the system can still be compatible with a commercial company.

## 7. Two-tier model architecture

Observer later describes a useful pattern:

```text
Large capable model
→ configures / builds / orchestrates

Small cheap/local model
→ runs repetitive monitoring loop
```

AI Engineering OS can adapt this concept:

```text
High-capability model
---------------------
architecture
planning
debugging
review
difficult implementation
handoff synthesis

Small/local/cheap model
-----------------------
classification
log triage
heartbeat checks
repo-health scanning
simple routing
summarization
routine monitoring
watchdog classification
```

This can substantially reduce provider usage.

Do not use a frontier model for every background pulse.

## 8. Model cost should be matched to cognitive difficulty

Future Router should consider:

```text
task complexity
risk
context requirement
latency requirement
tool-use requirement
historical success
provider quota
local hardware
cost
```

Then choose the cheapest model that satisfies the quality/risk requirement.

This aligns with the project's future Verified Delivery Cost principle:
optimize total cost of obtaining a verified correct result, not raw token cost.

## 9. Sensors → Models → Tools as an architecture pattern

Observer exposes a simple architecture:

```text
Sensors
→ Model
→ Tools
```

For AI Engineering OS the analogous pattern is:

```text
Evidence Sources
→ Context Materialization
→ Model / Agent
→ Typed Tool Contracts
→ Policy
→ Runtime
```

Possible evidence sources:

```text
repository
Task state
git diff
tests
logs
build output
issues
PRs
documentation
runtime events
user input
```

The model should never access all host capabilities directly.

## 10. Typed capability registry

Observer makes sensors and tools explicit.

AI Engineering OS should similarly maintain explicit capabilities:

```text
repo.read
repo.search
file.read
file.write
git.diff
git.commit
tests.run
build.run
network.read
network.write
secret.use
process.spawn
workspace.create
review.submit
```

This feeds:
- Policy Engine;
- CapabilityProfile;
- Playbooks;
- auditing;
- future marketplace metadata.

## 11. Local micro-agents

Observer's lightweight repeated agents suggest a useful future concept:

```text
MicroAgent
```

A MicroAgent is not a full autonomous coding agent.

It performs a narrow repeated task with:
- bounded context;
- bounded capabilities;
- small/local model;
- low execution cost;
- explicit trigger;
- typed result.

Examples for AI Engineering OS:

```text
test-failure classifier
log anomaly detector
dependency-change watcher
documentation drift checker
stale-task detector
provider-quota watcher
workspace-health checker
review finding deduplicator
```

Likely G11 / future optimization layer.

## 12. MCP as configuration/control surface

Observer later uses MCP to let a capable model configure micro-agents from natural language.

Potential lesson for AI Engineering OS:

```text
Natural-language intent
→ orchestration/control protocol
→ typed durable configuration
```

But do not let MCP become internal authority.

Preferred:

```text
MCP / UI / CLI / API
→ ingress adapter
→ canonical commands
→ Task/Policy/runtime state
```

MCP should be one control interface, not the system of record.

## 13. OpenAI-compatible endpoint lesson

Observer supports OpenAI-compatible local endpoints, allowing Ollama/llama.cpp/vLLM-style backends.

AI Engineering OS should preserve provider adapters rather than standardize core semantics around one API.

However, a generic OpenAI-compatible inference adapter may eventually be useful for local supporting models:

```text
LocalInferenceProvider
→ OpenAI-compatible endpoint
```

This is different from coding-agent providers such as Claude Code/Codex CLI, which have richer runtime/session/tool semantics.

Do not collapse both into one abstraction unless their behavioral contracts genuinely match.

## 14. Privacy boundary

Observer's strongest product story is local privacy.

AI Engineering OS can make privacy more explicit:

```text
default:
  source code stays local
  Task state stays local
  Event Journal stays local
  secrets stay local
  telemetry off

provider-required transmission:
  only context explicitly materialized for that provider
```

Future Context Engine should be able to explain:

```text
Which files/data will leave the machine?
Why?
To which provider?
For which Task?
```

Potential future UI:

```text
Context Disclosure Preview
```

before high-risk provider execution.

## 15. Hosted convenience must be explicit

If AI Engineering OS later offers cloud/team features, local and hosted paths should be clearly separated.

Example:

```text
Local mode
→ no account required

Team/Cloud mode
→ explicit sign-in
→ explicit sync
→ explicit remote execution
```

Avoid silently converting a local-first product into cloud-dependent software.

## 16. FOSS foundation / code signing lesson

Observer later became a SignPath Foundation project.

Possible future lesson for AI Engineering OS:
distribution trust matters for desktop software.

Eventually consider:

```text
reproducible builds
signed releases
SBOM
artifact provenance
release attestations
code signing
automated release verification
```

This is relevant closer to packaging/distribution, not current G5.

## 17. Community as product research

Observer's development appears strongly driven by community feedback.

AI Engineering OS could use:

```text
public roadmap
structured feature requests
anonymous/opt-in feedback
plugin requests
workflow sharing
playbook sharing
```

But product architecture must not be driven only by popularity.
Security and durable-state invariants remain non-negotiable.

## 18. AGPL caution

AGPL is not simply “MIT but safer.”

If AI Engineering OS chose AGPL:
- downstream modified network-interactive versions may have source-offer obligations;
- some companies may reject AGPL dependencies/projects;
- proprietary hosted forks become harder;
- dual licensing may be possible if copyright ownership remains controlled.

This is a business/legal strategy decision.

Do not choose AGPL merely out of fear of forks.

Evaluate:
- desired ecosystem;
- enterprise procurement friction;
- marketplace/plugin compatibility;
- contributor strategy;
- VC/business model;
- possible dual licensing.

## 19. Contribution-rights strategy

Before many external contributions arrive, explicitly decide whether future relicensing is important.

If yes, establish contribution policy early.

Possible options:

```text
DCO
CLA
copyright assignment
project-owned copyright
no planned relicensing
```

This should be reviewed by qualified counsel if the project becomes commercial.

## 20. Product moat vs fork risk

Fork protection alone should not drive architecture.

A strong fork can copy code but cannot trivially copy:

```text
brand/trademark
community
release trust
hosted infrastructure
enterprise integrations
support
plugin ecosystem
distribution channels
operational expertise
verified workflow data
customer relationships
```

Therefore licensing should balance adoption and protection rather than trying to prevent all competition.

## 21. Enterprise monetization candidates

If AI Engineering OS develops commercially, plausible enterprise value is:

```text
organization policy engine
central audit / compliance
SSO / SCIM
RBAC
team Task Graph
remote worker fleet
managed secrets integration
central provider controls
cost/quota governance
verified artifact retention
approval workflows
GitHub/Jira/Linear/Slack integrations
support / SLA
on-prem deployment management
```

These are more defensible than charging merely for running an agent.

## 22. Potential free/community edition

Possible future positioning:

```text
AI Engineering OS Community
----------------------------
local-first
single developer
self-hosted
bring-your-own provider subscriptions
local adapters
local task state
local worktrees
local verification
community playbooks/plugins
```

This aligns well with the user's preference for subscription-based provider access instead of requiring API billing.

## 23. Potential commercial edition/service

Possible:

```text
AI Engineering OS Team / Enterprise
-----------------------------------
multi-user
shared project control plane
organization policies
remote workers
central auditing
central provider governance
team approvals
managed connectors
cloud sync
support
```

Do not finalize names/pricing yet.

## 24. Architecture idea: cheap watcher + expensive reasoner

Useful concrete pattern:

```text
Watchdog
  small/local model
  checks cheap signals frequently
        ↓ interesting event
Escalation
        ↓
Reasoning Agent
  strong provider/model
```

Examples:

```text
small model:
  classify logs every 5 minutes

strong model:
  only invoked when anomaly confidence > threshold
```

This can dramatically reduce expensive provider usage.

## 25. Evidence-before-escalation

Micro-agents should not create expensive tasks from vague suspicion alone.

Preferred:

```text
cheap monitor
→ finding
→ evidence packet
→ threshold / deduplication
→ Task or investigation
```

This integrates with Repository Readiness Engine, Routine Engine, and Human Attention Metrics.

## 26. Local inference is supporting infrastructure, not mandatory core

AI Engineering OS should support local models where useful without requiring users to own large GPUs.

The core product should still work with subscription-backed providers.

Possible hierarchy:

```text
subscription coding providers
→ primary high-capability workers

small local models
→ optional background/supporting workers
```

This matches the current project direction.

## 27. Useful items to retain

Named future concepts strengthened by this article:

```text
MicroAgent
LocalInferenceProvider
ContextDisclosurePreview
ProviderCapacity
Routine/Watchdog Engine
Evidence Packet
Capability Registry
Community Edition
Team/Enterprise Layer
Contribution Rights Strategy
Release Provenance
```

## 28. What NOT to copy

Do not directly adopt:
- screen monitoring as a project goal;
- arbitrary model output string matching as core authority;
- untyped JavaScript agent actions;
- one generic inference API for all coding-agent providers;
- licensing based purely on fear of forks;
- cloud service dependence for basic use;
- unrestricted cross-agent memory mutation.

## 29. Relevance score

For AI Engineering OS:

```text
Technical orchestration ideas: 7/10
Local-first/privacy strategy: 9/10
Business/open-source strategy: 9/10
Direct code/architecture reuse: low
Conceptual reuse: high
```

The strongest value is strategic rather than implementation-level.

## 30. Current-gate impact

No change to TASK-0051 or G5.

Keep current sequence:

```text
TASK-0051
→ G5 entry-atomicity remediation

TASK-0052
→ G5 closure re-re-audit

after G5_CLOSED
→ G6 architecture/security work
```

Observer-derived ideas should remain staged for later architecture/product work.


## Appendix C — Tessra detailed research

> Reviewed: 2026-09-11
> Repository: `NitrusAphalion/tessra`
> Reddit discussion: “Tessra - Git for Agents” in r/coolgithubprojects
> Decision mode: adopt/adapt concepts independently; do not add Tessra as a dependency and do not replace Git/worktree architecture in v1.
> Current gate impact: none. G5 / TASK-0051 remains unchanged.

## Executive decision

Tessra is one of the most relevant external projects reviewed for AI Engineering OS because it attacks the same structural problem from a different layer: instead of orchestrating agents around Git, it redesigns version control around concurrent agents, proof-bearing changes, scoped capabilities, memory, and semantic units.

The strongest value is conceptual. We should **not** replace Git with Tessra, embed its VCS, or adopt its signed-op DAG wholesale. We should adopt/adapt several design principles into our existing control-plane architecture:

1. machine-readable verification standards and signed evidence;
2. independent evidence authority — an agent's own claim cannot satisfy its delivery gate;
3. typed, scoped memory with provenance, trust, staleness, supersession, and expiry;
4. budgeted context packs assembled from code + dependencies + tests + memory + active claims;
5. advisory work claims and dependency-aware task partitioning;
6. idempotency keys / replay-safe mutation contracts;
7. separation of blocking gates from reactive hooks/routines;
8. revocation as a whole-principal containment operation;
9. multi-altitude recovery: undo operation, revert delivered change, revoke agent;
10. local landing and remote publication as separate resumable reconciliation steps;
11. semantic code-unit identity and merge as a future research track, not a v1 VCS replacement;
12. semantic/structural indexes remain derived; canonical repository/blob state remains authoritative.

## What Tessra actually does

Tessra positions itself as “version control for the era of AI.” It can run beside Git, but maintains its own object store, operation log, workspaces, semantic units, standards, attestations, memories, capabilities, and daemon. It exposes a compact agent surface over CLI/MCP.

Important documented concepts:

- semantic merge at function/type/import-unit granularity for supported languages;
- stable unit identity across edits/renames;
- agent claims over paths/units as advisory coordination signals;
- planner partitions work by dependency edges;
- verification in a scratch copy with stripped environment;
- attestations cached by content/toolchain;
- standards expressed as data, including risk-gated human approval and distinct-model judges;
- bounded context reads and explicit token budgets;
- typed repository memory with provenance/trust/staleness;
- hooks reacting to events, distinct from standards that block landing;
- scoped/delegable capabilities;
- idempotency key on mutations;
- revocation that contains an agent across sessions/workspaces/claims/unlanded work;
- Git import/export bridge;
- local daemon and no mandatory server/account.

The repository is pre-release. Its documentation states that multi-machine sync is not built, verifiers are not container-isolated, only several languages get semantic merge, Git round-tripping is incomplete, and macOS/Linux paths are not yet exercised. Treat performance and concurrency claims as project claims, not independent benchmarks.

## A. Verification standards as data — ADOPT

Tessra's strongest idea is that “what counts as acceptable” is durable data, not prompt prose.

Conceptual AI Engineering OS form:

```text
VerificationPolicy
  clauses[]
    require TestEvidence(pass)
    require ChangedCodeCovered
    forbid TestWeakening
    require IndependentReview when risk >= HIGH
    require HumanApproval when risk >= CRITICAL
    require DistinctProviderJudges(2) when policy says so
```

Do not copy Tessra's exact DSL prematurely. First define typed domain contracts; a textual DSL can be a later projection/editor format.

Placement: G8 Verification, supported by G6 Policy.

## B. Agent self-claims never satisfy their own delivery gate — ADOPT STRONGLY

Tessra explicitly stores an agent's own attestation but does not let it satisfy the standard.

This reinforces an existing AI Engineering OS principle:

```text
Builder output != verification evidence
Agent says “tests pass” != tests passed
Agent says “done” != Task completed
```

Evidence that can satisfy a gate must come from an authorized evidence producer such as:

- deterministic verifier runner;
- CI principal;
- independent reviewer/provider;
- human approval;
- trusted observer with provenance.

Placement: G8.

## C. Evidence attestations + content/toolchain caching — ADOPT/ADAPT

Useful concept:

```text
VerificationEvidence
  subject_fingerprint
  verifier_kind
  toolchain_fingerprint
  command/config fingerprint
  result
  producer principal
  provenance
  timestamp
  signature/hash/receipt
```

A cache hit is allowed only when the relevant fingerprints match. Do not blindly reuse “tests passed” across changed code/toolchain/environment.

Cryptographic signing can be staged: provenance + immutable evidence first; signing where it adds security value later.

## D. Advisory WorkClaim — ADOPT FOR G7

Tessra does not use claims as hard locks. If another agent overlaps, it reports who, why, and that a merge/conflict may follow.

AI Engineering OS should consider a future `WorkClaim`:

```text
WorkClaim
  task_id
  execution_id
  attempt_id
  scope: path | code_unit | resource
  intent
  owner
  created_at
  expiry
```

Use it to improve planning and context, not as an authority that can deadlock development.

Worktree isolation remains the execution safety mechanism. Claims solve coordination/awareness, not filesystem isolation.

## E. Dependency-aware partitioning — ADOPT/ADAPT

Tessra's planner groups code units by dependency edges before handing work to parallel agents.

This aligns with G7 durable DAG + bounded JIT decomposition. Add a future partitioning heuristic:

```text
Task Graph
+ Code Dependency Graph
+ Active Claims
+ Risk/ownership boundaries
→ candidate parallel partitions
```

The Task Graph remains authority. Code-graph partitioning is advice/optimization.

## F. Semantic code-unit identity / unit-level merge — RESEARCH, DO NOT PUT IN V1

This is Tessra's most novel and most invasive idea.

Potential benefits:

- two agents modifying different functions in one file need not conflict;
- rename-aware reconciliation;
- code-unit blame/history;
- dependency-aware merge and context;
- conflict objects can carry both intents instead of presenting raw line conflicts.

But replacing Git's line/file merge semantics inside our product would add a second VCS substrate, language parser coverage, identity stability problems, formatting/regeneration questions, and long-tail language risk.

Decision:

```text
v1: Git + worktree isolation remains canonical
future: Semantic Change Graph / Merge Assist research track
```

Possible later use without replacing Git:

- pre-merge conflict prediction;
- semantic overlap detection;
- suggested three-way resolution;
- function/type-level ownership/claims;
- review grouping by logical unit;
- impact-aware landing order.

## G. Context packs with explicit budgets — ADOPT STRONGLY FOR G9

Tessra's context request asks: “what do I need to know to change X, in N tokens?” and assembles code, dependencies, dependents, tests, memories, claims, and recent authorship.

AI Engineering OS version:

```text
ContextRequest
  task/execution
  target scope
  token/byte budget
  provider/model constraints
  purpose

ContextPack
  exact code
  structural neighbors
  dependencies/dependents
  relevant tests
  architecture constraints
  scoped memories
  active claims/conflicts
  prior evidence
  provenance for every section
  omitted/recovery handles
  budget accounting
```

This directly strengthens G9 Context Compiler.

## H. Typed memory with trust + staleness anchors — ADOPT STRONGLY FOR G10

Tessra memory objects include kind, scope, provenance, content anchor, confidence, trust, supersession and expiry. When anchored code changes, memory becomes stale rather than silently remaining “true.”

AI Engineering OS should preserve its semantic/episodic/procedural/decision model but add these lifecycle properties:

```text
MemoryRecord
  type
  scope
  body / structured payload
  provenance
  anchor_fingerprint
  confidence
  trust_state
  lifecycle: proposed | confirmed | contradicted | stale | superseded | retired
  supersedes
  expiry
```

Important adoption:

- auto-extracted memory enters `proposed`, not authoritative;
- global/convention memory needs stronger capability/approval than task-local memory;
- revoking an agent should quarantine/retire its unconfirmed learned memories;
- stale memory remains visible with warning until confirmed/superseded/retired;
- generated `AGENT.md`/provider instructions can be projections, not the source of truth.

This strongly complements the Learning/Promotion Pipeline.

## I. State-rich, agent-friendly responses — ADOPT/ADAPT

Tessra responses include result + current state + suggested next actions + budget usage, while errors expose a typed code and remediation.

Future AI Engineering OS action response pattern:

```text
ActionResult
  outcome
  durable_state_ref / state summary
  evidence refs
  conflicts / unmet gates
  next_admissible_actions[]
  budget/resource usage
  retryability
  typed error/remediation if failed
```

Do not make the LLM reconstruct state from prose after every tool call.

## J. Idempotency keys on mutations — ADOPT

Every externally retryable mutation should eventually have an explicit replay/idempotency contract.

Examples:

- create execution;
- create attempt;
- request verification;
- create decision request;
- publish/land result;
- dispatch hook/routine action;
- remote synchronization.

This does not replace transactional invariants. Idempotency handles retry/replay; atomic transactions handle admission/state consistency.

## K. Gates block; hooks react — ADOPT STRONGLY

Excellent separation:

```text
Verification/Policy Gate
→ determines whether transition is allowed

Event Hook / Routine
→ reacts after/beside an event
→ cannot silently redefine the gate
```

A hook may produce evidence that a gate later consumes, but a reactive automation should not become hidden policy authority.

This should connect G8 Verification with the future G11 Routine/Pulse Engine.

## L. Human approvals as typed evidence — ADOPT

Human approval should be a durable `HumanDecision`/approval receipt tied to exact subject/risk/revision rather than “user said yes somewhere in chat.”

Important fields:

```text
subject
policy/gate
risk factors
question/options
human principal
answer
note
provenance
signature/session authentication as appropriate
```

## M. Scoped/delegable capabilities — ADOPT CONCEPT, IMPLEMENT THROUGH OUR G6 MODEL

Tessra allows a planner to receive a delegable capability and issue narrower capabilities to children; every child capability must remain contained within the parent's authority.

AI Engineering OS should adopt capability monotonicity:

```text
child authority ⊆ parent delegated authority
```

No agent may expand its own path/tool/network/secret/action scope through delegation.

This belongs in G6/G7.

## N. Whole-agent revocation — ADOPT FOR G6/G11

`cancel process` is not enough once agents can own workspaces, claims, pending approvals, memory proposals, and background routines.

Future `RevokePrincipal/Agent` should reconcile:

- active provider sessions;
- processes;
- pending mutations;
- work claims;
- leases/workspaces;
- unlanded proposals;
- delegated capabilities;
- scheduled child work;
- unconfirmed memory proposed by that principal.

Revocation should be auditable and idempotent.

## O. Multi-altitude recovery — ADOPT

Useful distinction:

```text
undo operation
revert delivered change
revoke agent/principal
```

AI Engineering OS already has recovery/reconciliation goals; this gives a clean UX taxonomy.

Do not promise perfect rollback for irreversible external effects. The recovery layer must distinguish reversible internal state from compensating actions.

## P. Git publish is resumable reconciliation, not an atomic illusion — ADOPT FAILURE LESSON

The Reddit discussion contains a useful author explanation: Tessra's Git export/push is idempotent rather than atomic. Local commits/ref updates can succeed while remote push fails; retry uses persisted revision→commit mappings and retries publication.

AI Engineering OS should explicitly model future landing/publishing as a saga:

```text
Verified change
→ local landing prepared
→ durable mapping recorded
→ branch/ref reconciled
→ remote publish attempted
→ publish receipt recorded
```

If remote publish fails, do not roll back already-canonical local facts merely to simulate atomicity. Persist a `PUBLISH_PENDING/FAILED` state and retry safely.

This belongs in the future Delivery Reconciler / Landing Manager.

## Q. Structural/semantic index is derived — ADOPT

Tessra states that blobs are source of truth while semantic index is derived/cached.

AI Engineering OS should preserve the same principle for G9 Code Intelligence:

```text
Repository/Git state = authority
Code graph / semantic index = rebuildable derived projection
```

A broken/stale index must degrade retrieval quality, not corrupt repository truth.

## R. Continuous anomaly score/throttle/revoke — ADAPT LATER

Interesting but not v1-critical. Signals could include:

- repeated policy refusals;
- repeated destructive attempts;
- scope violations;
- looping actions;
- unexplained workspace churn;
- suspicious secret/network requests.

Escalation:

```text
observe
→ warn
→ throttle
→ require approval
→ revoke
```

Do not let an opaque model-based anomaly score become authority. Security-critical escalation needs explainable signals/policy.

## S. Verification scratch environment — ADOPT, BUT STRONGER THAN TESSRA

Tessra runs verification in a scratch copy with stripped environment, but its own docs say it is not container-isolated.

AI Engineering OS should retain stronger G6/G8 direction:

- clean materialized verification workspace;
- minimal environment;
- no secrets by default;
- bounded network/process access;
- stronger OS/container/sandbox isolation where policy requires it.

## T. Do not adopt Tessra as our VCS/dependency

Reasons:

- AI Engineering OS already chose Git worktree isolation;
- Tessra is pre-release;
- semantic merge supports only a subset of languages;
- multi-machine sync is unbuilt;
- Git round-trip is incomplete;
- verification isolation is weaker than our intended security boundary;
- adopting a second VCS would dramatically increase core complexity;
- our product's primary value is the agent control plane, not replacing Git.

Use Tessra as a research/reference implementation only.

## U. License note

Tessra is MIT licensed. That makes it easy to study/reuse legally subject to MIT terms, but our project's current business goal is specifically to prevent commercial cloning/repackaging of the core. Therefore Tessra's MIT licensing choice should **not** be copied for AI Engineering OS core.

## V. Priority mapping

### P0 / strong future adoption

- verification standards as typed data;
- independent evidence authority;
- content/toolchain-bound verification evidence;
- context packs with explicit budgets/provenance;
- memory anchors/trust/staleness/supersession;
- gate vs hook separation;
- idempotent retryable mutations;
- capability delegation monotonicity;
- principal-wide revocation semantics.

### P1

- advisory WorkClaim;
- dependency-aware code partitioning;
- state-rich action responses;
- human approvals as evidence;
- Delivery Reconciler publish saga;
- multi-altitude recovery UX.

### Research / later

- semantic unit identity;
- unit-level merge;
- semantic conflict tasks;
- continuous anomaly scoring;
- signed/chained operation DAG as a replication mechanism.

## W. Gate placement

```text
G6 Security
  scoped/delegable capabilities
  principal revocation
  verifier isolation
  trusted evidence producers
  anomaly containment (later)

G7 Multi-agent orchestration
  WorkClaim
  dependency-aware partitioning
  budgeted child assignments

G8 Verification
  VerificationPolicy / clauses
  evidence attestations
  human approvals
  independent provider judges
  content/toolchain cache rules

G9 Context + Continuity
  budgeted ContextPack
  code-unit/dependency retrieval
  active-claim/conflict context
  derived semantic index

G10 Memory
  typed scoped memory
  provenance/trust/confidence
  anchor-based staleness
  proposed→confirmed promotion

G11 Background daemon
  reactive hooks/routines
  anomaly/watchdog reactions
  durable hook outcomes

Future Delivery Reconciler / Landing Manager
  idempotent local landing
  remote publish reconciliation
  retry receipts

Future research
  semantic merge / code-unit identity
```

## X. Effect on current work

None.

```text
G5 remains IN PROGRESS
TASK-0051 remains the current task
TASK-0052 remains the closure re-re-audit
```

Do not pull semantic merge, Tessra VCS objects, standards DSL, or memory implementation into G5.


## Appendix D — Anthropic Claude Code detailed research

> Date: 2026-09-11
> Source: `anthropics/claude-code` public repository plus Anthropic's official Claude Code documentation.
> Review mode: architecture/ideas/failure lessons only. No code copying and no dependency adoption.
> Current gate impact: none. G5 / TASK-0051 remains unchanged.

## 1. Source boundary and license

The public GitHub repository is not a normal open-source dump of Claude Code's core implementation. Its `LICENSE.md` states that Anthropic retains all rights and use is subject to Anthropic's Commercial Terms. The repository mainly exposes documentation-adjacent assets, examples, plugin material, settings samples and project metadata; therefore this review treats the public repository and official docs as a product/architecture reference, not as reusable implementation source.

Decision:

```text
Study behavior, contracts, UX, extension boundaries and failure lessons.
Do not copy code.
Do not vendor code.
Do not add claude-code repository code as a dependency.
Do not infer hidden internal architecture beyond public evidence.
```

## 2. Overall relevance

Claude Code is unusually relevant because it is both:

1. a real provider AI Engineering OS already integrates through the Claude Code CLI; and
2. a mature example of an agent harness with sessions, tools, permissions, sandboxing, worktrees, plugins, skills, hooks, subagents, teams, routines, remote execution and enterprise policy.

The useful lesson is not to clone Claude Code. AI Engineering OS operates one layer above provider-specific harnesses. The main design target is to preserve provider-neutral Task/Execution/Attempt semantics while adopting mature control-plane patterns that remain valid across Claude, Codex, Gemini and future executors.

## 3. Agentic loop: gather → act → verify

Claude Code documents its loop as repeated context gathering, action and verification. This is a useful baseline, but AI Engineering OS should separate these concerns into durable orchestration/evidence semantics rather than leaving completion to a single provider session.

Adopt as a planning/execution mental model:

```text
Gather evidence
    ↓
Act
    ↓
Verify
    ↓
Re-plan or finish
```

AI Engineering OS difference:

```text
provider agent loop
    !=
Task completion authority
```

The provider may perform local verification; G8 remains the authoritative independent delivery verification layer.

## 4. Stable engine, multiple surfaces

Claude Code exposes a similar agent engine through terminal, IDE, desktop, web, mobile/remote control and CI integrations.

Adopt the architectural principle:

```text
Core orchestration/domain semantics
            ↓
CLI | Tauri | IDE | Web | Mobile | CI | MCP
```

Interfaces are projections/adapters. They may not implement a second copy of Task, policy, retry, verification or provider lifecycle semantics.

This strongly reinforces the existing AI Engineering OS thin-Tauri / agentd design.

## 5. Context extensions as separate mechanisms

Claude Code clearly separates persistent instructions, skills, subagents, hooks, MCP and plugins. Each enters the agent loop at a different point.

Useful conceptual separation for AI Engineering OS:

```text
Policy / project instructions
Reusable Skill / Procedure
Isolated Subagent / worker
External Tool / Connector
Lifecycle Hook / Routine
Packaging / Marketplace unit
```

Do not collapse these into one generic "prompt extension" abstraction.

## 6. Progressive disclosure

Claude Code skills expose concise metadata first and load full content only when relevant. MCP tool definitions are also deferred/on-demand so inactive tools do not consume full context.

Adopt/reinforce for G9:

```text
L0 discovery metadata
    ↓ relevant
L1 compact contract/summary
    ↓ needed
L2 full procedure/docs/examples/evidence
```

The Context Compiler should resolve context lazily and under explicit token/privacy budgets. Tool registries should expose cheap discovery metadata without injecting every tool schema/instruction into every prompt.

This reinforces existing OpenViking/Context Hub decisions.

## 7. Subagent context isolation

Claude Code subagents run in their own context; their detailed tool traces stay out of the parent context and the parent receives a result summary.

Adopt the isolation principle, with a stronger trust model:

```text
Parent Task context
     ↓ delegated bounded objective
Child isolated context
     ↓ result + evidence refs
Parent receives bounded summary
```

But child summaries remain untrusted derived data. Correctness-critical claims must resolve to underlying evidence.

## 8. Dynamic workflows / fan-out + verification

Claude Code supports dynamic workflows where many subagents can be launched from a generated script, including codebase-wide audits and cross-checking findings.

Useful future G7/G8 pattern:

```text
Planner
  ↓ bounded decomposition
workers[]
  ↓ findings
independent verifier[]
  ↓ reconciler
verified result
```

Important adaptation: generated workflow code is never the authoritative workflow definition. The AI Engineering OS Task Graph, policy, budgets and evidence contracts remain authoritative. Generated orchestration may propose/instantiate bounded child work through canonical APIs.

## 9. Agent teams and shared coordination

Claude Code's agent-team model provides a lead, multiple agent sessions, shared work/task coordination and inter-agent messaging.

Adopt concepts, not provider semantics:

- manager/lead as a role, not Task authority;
- shared durable Task Graph rather than chat-owned task state;
- typed agent messages;
- explicit delegation scope;
- attention/escalation status visible centrally;
- provider sessions remain execution details.

This reinforces the planned RoleProfile, WorkClaim, AgentMessage and HumanDecisionRequest concepts.

## 10. Cross-session messaging

Claude Code supports sending messages between sessions, including sessions on other machines/web surfaces.

For AI Engineering OS this supports the future typed messaging design, but raw text must not become authority.

Preferred contract:

```text
AgentMessage
  sender
  recipient/role
  kind
  trust/provenance
  task/execution/attempt refs
  evidence/artifact refs
  body
  reply_to
```

A message can inform decisions; it cannot mutate canonical Task state directly.

## 11. Cross-surface/session continuity

Claude Code distinguishes resume, fork, moving sessions between local/web/desktop, and remote control.

Useful idea for G9/G12:

```text
Execution continuity
    independent of UI surface
```

However AI Engineering OS should go further: continuity must be provider-neutral and repository/evidence-based, not dependent on one provider transcript. `ContinuationCheckpoint` remains the stronger target abstraction.

## 12. Checkpoints and Git are different recovery layers

Claude Code checkpoints snapshot file edits independently of Git and can rewind session-local file changes, while remote/API effects cannot be rewound the same way.

Adopt the multi-layer recovery principle:

```text
conversation/action undo
workspace/file checkpoint
Git commit/revert
Task/Execution recovery
external side-effect reconciliation
```

Do not pretend these layers provide one universal rollback transaction.

This reinforces the Delivery Reconciler and saga approach learned from Tessra.

## 13. Permission mode vs permission rules

Claude Code distinguishes user-facing modes (manual/plan/accept-edits/auto) from specific allow/deny policy rules and organization-managed settings.

Adopt the separation:

```text
InteractionMode
  UX/default approval posture

PolicyRule
  authoritative allow/deny constraints

CapabilityGrant
  scoped runtime authority
```

A convenient UX mode must never weaken managed/organization policy.

## 14. Managed policy precedence

Claude Code supports centrally managed settings/policies as a higher-precedence administrative layer.

Strong future enterprise pattern:

```text
Organization hard policy
       ↓ cannot be weakened by
Workspace policy
       ↓
Project defaults
       ↓
User preferences
       ↓
Session choice
```

Exact precedence should be specified by our own ADR; the key invariant is that lower-trust/lower-authority layers cannot expand beyond higher-level restrictions.

## 15. Sandboxing as filesystem + network control

Claude Code publicly documents sandboxed command execution with filesystem and network restrictions, plus organization controls for allowed domains/sockets and unsandboxed commands.

Adopt/reinforce G6 target:

```text
ToolRequest
   ↓
Policy Engine
   ↓
Capability binding
   ↓
OS sandbox / worker isolation
   ├── filesystem scope
   ├── network scope
   ├── process scope
   └── credential scope
```

Prompt instructions are not a sandbox.

## 16. Plan/read-only mode as capability profile

Claude Code Plan mode lets the model explore and propose without editing source files.

AI Engineering OS should represent this as capabilities, not provider-specific mode strings:

```text
INVESTIGATOR / PLANNER
  repo.read      ✓
  search         ✓
  diagnostics    ✓
  file.write     ✗
  git.commit     ✗
  network.write  ✗ unless granted
```

This reinforces the read-only investigator concept from the Scape review.

## 17. Advisor / stronger-model escalation

Claude Code has an advisor mechanism where the main model can consult a stronger model at difficult decision points.

Useful adaptation for future Model Router:

```text
Primary executor
    ↓ hard/high-risk decision
Advisor request
    ↓ independent bounded answer
Primary/reconciler continues
```

Do not hard-code one model pair. Escalation should be capability/risk/cost/capacity driven and preserve provenance.

Potential uses:
- architecture decision;
- concurrency/storage change;
- security-sensitive change;
- ambiguous failure diagnosis;
- review disagreement.

## 18. Memory: useful UX, insufficient as authority

Claude Code supports project instructions and auto memory across sessions. The docs also explicitly acknowledge context compaction and the need to put durable rules outside transient conversation history.

Adopt the lesson:

```text
conversation != durable project truth
```

But AI Engineering OS should keep its stronger typed-memory target from Tessra/OpenViking:

- provenance;
- trust;
- lifecycle;
- code anchors;
- stale/superseded status;
- promotion/review policy;
- bounded retrieval.

Provider-native memory is an optimization/compatibility source, never canonical Memory Engine state.

## 19. Compaction is an optimization, not continuity

Claude Code automatically clears older tool outputs and summarizes conversation as context fills. It warns that earlier detailed instructions can be lost.

This directly reinforces existing AI Engineering OS invariants:

```text
provider compaction
= context optimization
≠ persistence
≠ Task state
≠ handoff protocol
```

Cross-provider continuity must reconstruct from durable domain state/evidence.

## 20. Worktrees for parallel isolation

Claude Code uses worktrees for parallel sessions and exposes worktree-aware session management.

Already adopted in AI Engineering OS. No architectural change.

Important distinction remains:

```text
worktree = write isolation
WorkClaim = coordination awareness
Task Graph = work authority
```

## 21. Hooks as lifecycle reactions

Claude Code hooks react to events such as tool use, session lifecycle and other events; plugins may register them automatically.

Adopt/reinforce:

```text
Hook/Event reaction
    can notify, validate, produce evidence, enqueue work

Hook
    != authoritative policy by itself
```

This aligns with the Tessra-derived rule: policy gates transitions; hooks react to transitions.

## 22. Routines / scheduled automation

Claude Code supports scheduled/event-triggered routines for repetitive work.

Adopt/reinforce G11 Routine Engine:

```text
RoutineDefinition
  trigger
  inputs
  procedure/playbook
  capability profile
  budgets
  output/evidence contract
  escalation policy
```

Routine execution should be durable/auditable and not merely a prompt scheduled at a timestamp.

## 23. Plugin packaging and marketplace

Claude Code plugins package skills, agents, hooks and MCP servers; marketplaces distribute them.

Very relevant to the future AI Engineering OS marketplace.

Suggested AI Engineering OS package boundary:

```text
PluginPackage
  manifest
  capabilities requested
  skills/procedures
  hooks/routines
  connector/tool adapters
  schemas
  version/dependencies
  publisher identity
  signatures/provenance
```

Security improvement over a simple folder bundle:
- explicit requested permissions;
- install-time review;
- runtime policy enforcement;
- dependency pinning;
- provenance/signatures;
- organization allow/deny controls.

## 24. Marketplace dependency constraints

Claude Code supports plugin dependency/version constraints and organization recommendation/marketplace controls.

Adopt the need for:

- semver/version constraints;
- immutable resolved install graph;
- lock/ledger of installed plugin versions;
- reproducible organization plugin sets;
- deny/allow policy by publisher/plugin/capability;
- rollback to prior plugin version.

This connects to Agency Agents' install-ledger lesson.

## 25. MCP as connector surface, not domain authority

Claude Code uses MCP to connect external tools/services.

AI Engineering OS should continue to treat MCP as one adapter protocol:

```text
MCP
UI
CLI
Webhook
Native connector
   ↓
canonical tool/ingress contracts
```

MCP types must not leak into Task, Memory, Policy or Verification domain semantics.

## 26. Tool discovery / deferred schemas

Claude Code defers MCP tool definitions and loads them on demand.

This suggests a useful future `ToolCatalog` split:

```text
ToolDescriptor
  cheap metadata for routing/discovery

ToolContract
  full schema/security/cost metadata loaded when selected
```

Benefits:
- lower context cost;
- faster discovery;
- easier marketplace scaling;
- smaller prompt-injection surface from unused tools.

## 27. Agent view / attention-first operations

Claude Code has an agent view for managing multiple sessions and highlighting which sessions need input.

This supports our UI principle but should be elevated from session-centric to Task-centric:

```text
Needs Attention
Blocked
Running
Verifying
Ready to Land
Routines
Provider Capacity
```

Session details remain drill-down projections.

## 28. Enterprise governance as a real monetizable layer

Claude Code's public docs include managed settings, centralized MCP allow/deny, gateways, spend controls, analytics, SSO-adjacent enterprise deployment, self-hosted environments and network configuration.

This validates the AI Engineering OS monetization hypothesis:

```text
Local individual core
    +
Team shared control plane
    +
Enterprise governance / policy / audit / remote worker management
```

The strongest enterprise value is governance and operations, not simply charging for another agent invocation.

## 29. Gateway pattern

Claude Code documents gateway deployment for centralized credentials, model access, spend limits and telemetry.

Future AI Engineering OS enterprise pattern:

```text
Developer / Remote Worker
        ↓
Organization Provider Gateway
        ├── identity
        ├── credential brokering
        ├── model allowlist
        ├── spend/quota policy
        ├── audit/telemetry
        └── upstream providers
```

But BYO subscription/local CLI paths must remain possible for the individual/local-first product where supported.

## 30. Self-hosted remote workers

Claude Code supports self-hosted execution environments and per-session identity verification.

Relevant to G12 Remote Worker:

- explicit worker identity;
- short-lived/session-scoped credentials;
- controlled egress;
- lifecycle hooks;
- health/metrics;
- workload routing;
- no ambient organization secrets.

Do not copy their transport or JWT format; adopt the identity/lifecycle principles.

## 31. What Claude Code does not solve for us

Claude Code remains a provider-specific harness. AI Engineering OS still needs higher-level semantics for:

- provider-neutral Task/Execution/Attempt;
- cross-provider handoff;
- durable Task Graph;
- independent completion authority;
- multi-provider routing/capacity;
- provider-neutral evidence;
- cross-provider memory;
- Delivery Reconciler;
- business/team governance independent of Anthropic.

Therefore Claude Code should remain an executor/provider adapter, not the orchestration source of truth.

## 32. Failure lessons / things not to adopt as authority

1. Provider transcript/session files are not canonical project state.
2. Provider-native memory is not canonical durable memory.
3. Provider compaction is not persistence.
4. Provider checkpoints cannot roll back external side effects.
5. Plain-text inter-agent messages are not Task commands.
6. Hooks are not policy unless explicitly validated through policy contracts.
7. A provider-specific permission mode must not become domain semantics.
8. Generated dynamic workflow scripts must remain bounded proposals/executors under the Task Graph.
9. Session-centric UI should not displace Task/outcome/verification UX.
10. A single provider's team/agent model must not leak into provider-neutral orchestration types.

## 33. Placement by gate

```text
G6 Security
  Policy hierarchy
  capability profiles
  interaction mode vs hard policy
  sandbox filesystem/network/process/credential controls
  organization-managed policy

G7 Multi-agent orchestration
  manager/worker role patterns
  bounded dynamic fan-out
  shared durable coordination
  advisor/escalation role

G8 Verification
  verify phase as independent gate
  cross-checking / independent review
  hook-generated evidence without hook-as-policy

G9 Context + Continuity
  progressive disclosure
  deferred tool contracts
  isolated child contexts
  cross-session/provider-neutral continuation packs
  compaction as optimization only

G10 Memory
  provider memory as compatibility input only
  typed canonical Memory Engine remains ours

G11 Background daemon
  routines
  event/schedule triggers
  durable/auditable background automation

G12 Remote Worker
  self-hosted worker identity/lifecycle
  scoped session credentials
  controlled egress

G13 Team/Enterprise
  managed policy
  gateways
  spend controls
  centralized connectors
  analytics/audit

Marketplace
  plugin package manifest
  dependency constraints
  publisher/provenance/security policy
```

## 34. Priority adoption list

### P0 — reinforce/encode in future architecture

- provider sessions remain execution detail, never Task truth;
- progressive context/tool disclosure;
- interaction mode separate from enforced policy;
- hard organization policy cannot be weakened downstream;
- sandbox scopes filesystem + network + process + credentials;
- provider compaction/checkpoints are not persistence/global rollback;
- hooks react, policy gates;
- plugin manifests declare capabilities and provenance;
- remote workers require explicit identity and scoped credentials.

### P1 — future high-value capabilities

- advisor/escalation role;
- agent-team coordination mapped to typed Task Graph + AgentMessage;
- ToolCatalog metadata/full-contract split;
- marketplace dependency lock/ledger;
- provider gateway controls;
- session/agent attention projection.

### P2 — product/UX refinements

- cross-surface continuity UX;
- status-line style context/cost/capacity telemetry;
- routine templates;
- organization recommended plugin sets.

## 35. Final decision

Claude Code is one of the most useful product references for AI Engineering OS, but it is not an implementation source and should not define our domain model.

Best synthesis:

```text
Claude Code teaches us how to build a strong provider harness.
AI Engineering OS must remain the durable, provider-neutral control plane above it.
```

Current G5/TASK-0051 scope is unchanged.


## Appendix E — OpenAI Codex detailed research

> Repository: https://github.com/openai/codex
> Review date: 2026-09-11
> Upstream license: Apache-2.0
> Adoption policy: architecture/ideas/failure lessons only. **No source code is copied, vendored, or adopted as a dependency because of this review.** Any useful concept is independently reimplemented against AI Engineering OS contracts, invariants, tests, and security boundaries.
> Current gate impact: none. G5 / TASK-0051 remains unchanged.

## 1. Executive verdict

OpenAI Codex is one of the highest-value references for AI Engineering OS because it exposes a mature local coding-agent harness, machine-readable app-server protocol, remote execution path, sandbox/approval architecture, daemon lifecycle, worktree support, model capability catalog, plugins/apps/skills, and multi-agent feature work.

The strategic conclusion is deliberately **not** “build a better Codex.”

```text
Codex / Claude Code / OpenCode / Gemini CLI
                ↓
        provider harnesses
                ↓
        AI Engineering OS
                ↓
 durable Task Graph + Policy + Context + Verification + Recovery
```

The clearest product positioning is:

> **Codex is a worker. AI Engineering OS is the durable provider-neutral control plane that decides what work exists, who should do it, with what authority/context, how the result is verified, how failures recover, and when a human actually needs to intervene.**

A user should choose AI Engineering OS even when they already like Codex because the product adds multi-provider continuity, durable task/execution state, policy, isolated workspaces, independent verification, cross-provider handoff, memory, human-attention management, delivery reconciliation, and organization governance.

## 2. What Codex currently demonstrates

Public repository/docs expose several mature subsystems:

- Rust-based local CLI and application server;
- machine-readable `app-server` with versioned request/response/notification protocol;
- explicit initialize/capability handshake;
- thread/turn/item lifecycle with asynchronous streaming notifications;
- standalone sandboxed command execution outside an LLM turn;
- separate `exec-server` for process/filesystem execution and remote environments;
- daemon lifecycle for background app-server instances;
- sandbox + approval architecture;
- preview `execpolicy` policy engine;
- layered configuration with source/provenance metadata and per-layer fingerprints;
- model catalog including supported capabilities/reasoning options;
- worktree-backed isolated sessions;
- apps/plugins/skills/MCP surfaces;
- automated approval review by a separate reviewer/subagent;
- remote control and mobile/desktop-oriented server flows.

Latest public release information also shows experimental worktree creation for new/forked sessions, shared background app-server behavior on Windows, inline answers while work continues, and tighter preservation/invalidation of authorization context around automatic approval review.

## 3. Highest-value idea: separate control protocol from execution protocol

Codex has a high-level app-server for threads/turns/events and a separate exec-server for subprocess/filesystem operations, including remote execution.

AI Engineering OS should preserve and sharpen this separation:

```text
Control Plane
-------------
Task
Execution
Attempt
Planner
Scheduler
Policy
Verification
Human decisions

      ↓ typed commands / leases

Execution Plane
---------------
process spawn
PTY
filesystem
sandbox
network
workspace
remote worker
```

### Our ≥1% improvement

Codex's split is primarily harness-oriented. Ours should be **domain- and provider-neutral**:

- execution plane never owns Task lifecycle;
- provider adapters never mutate Task state;
- all long-lived work has Task/Execution/Attempt identity;
- remote execution uses leases and durable reconciliation;
- execution capabilities are scoped and auditable;
- every command/result can be correlated back to durable work/evidence.

Likely placement: G6, G11, G12.

## 4. Async Submission/Event queues → durable event envelopes

Codex protocol uses asynchronous submission/event communication and streams started/completed/delta notifications for work items.

Useful principle:

```text
Command submission
      ↓
accepted identity
      ↓
async progress/events
      ↓
terminal outcome
```

### Our ≥1% improvement

For AI Engineering OS, event transport must not be the source of truth by itself. Use explicit envelopes such as:

```text
EventEnvelope
  event_id
  task_id
  execution_id
  attempt_id
  correlation_id
  causation_id
  sequence
  producer
  occurred_at
  payload
```

and maintain durable facts in canonical storage/Event Journal. Reconnect/replay must reconstruct projections without trusting a live provider stream.

Likely placement: cross-cutting / G11-G12.

## 5. Protocol capability negotiation

Codex clients initialize with client metadata and negotiated capabilities. Experimental APIs can be opt-in and notifications can vary by client capability.

This is valuable for our Tauri/CLI/IDE/remote-worker ecosystem.

Proposed concept:

```text
ProtocolHello
  protocol_version
  client_type
  supported_features[]
  supported_event_versions[]
  security_capabilities[]
```

### Our ≥1% improvement

- unsupported required capabilities fail closed before work starts;
- feature negotiation is separate from product entitlement;
- protocol compatibility has contract tests and a migration matrix;
- no silent semantic fallback for security-sensitive features.

## 6. Generated schemas from canonical protocol types

Codex generates JSON Schema / TypeScript protocol surfaces from canonical Rust types.

This is a strong pattern for AI Engineering OS's Rust backend + TypeScript UI.

### Adopt independently

```text
canonical protocol types
        ↓
 schema generation
        ↓
 Rust / TS contract artifacts
        ↓
 compatibility tests
```

### Our ≥1% improvement

Add:

- checked-in protocol snapshots;
- breaking-change detection;
- golden compatibility tests;
- versioned deprecation windows;
- provider-neutral/domain-neutral boundaries;
- fuzz/property tests for serialization where risk warrants it.

This can reduce frontend/backend drift substantially.

## 7. Deterministic commands should not require an LLM turn

Codex app-server can run a standalone sandboxed command without creating a conversational thread/turn.

This is a very high-leverage pattern for us.

AI Engineering OS should not invoke a model for operations that are already deterministic:

```text
git status
cargo test
formatting
lint
file metadata
workspace cleanup
verification commands
repository probes
```

should run through typed deterministic runners whenever reasoning is not needed.

### Why this matters

It directly improves:

- latency;
- cost;
- reproducibility;
- testability;
- security;
- reliability.

This is a Pareto/high-leverage feature: model avoidance on deterministic work can generate more value than many UI features.

## 8. Fine-grained permission elevation instead of full sandbox escape

Codex explicitly prefers requesting additional filesystem/network permissions while staying inside a sandbox rather than escalating directly to unrestricted execution.

This should become a major G6 principle.

```text
Current CapabilityGrant
        ↓
CapabilityDelta request
        ↓
Policy evaluation
        ↓
approve once / approve scoped / deny
        ↓
short-lived expanded sandbox
```

Potential capability deltas:

```text
filesystem.read += /path
filesystem.write += /path
network.connect += domain
secret.use += credential_ref
process.exec += executable class
```

### Our ≥1% improvement

Capability elevation should include:

- exact action/scope;
- reason;
- risk class;
- expiry/duration;
- one-action/session/workspace applicability;
- approval principal;
- context/intent fingerprint;
- durable audit evidence;
- automatic invalidation when the relevant Task intent or requested action changes.

Never equate `Auto Mode` with greater authority.

## 9. Authorization must be bound to context and invalidated when context changes

Recent Codex release notes explicitly improve automatic approval reviews by preserving authorization context across compaction and rejecting approvals invalidated by new user instructions/answers.

This is a highly valuable security idea.

AI Engineering OS should model approval evidence roughly as:

```text
ApprovalGrant
  subject
  action_fingerprint
  task_intent_fingerprint
  policy_version
  risk
  approver
  scope
  expires_at
```

If material context changes, the grant must be re-evaluated.

### Our ≥1% improvement

Do this as a durable, provider-neutral policy primitive instead of relying on provider conversation context.

This fits both G6 permissions and G8 human/verification approval.

## 10. Auto-review of approval requests — useful, but only as an advisor

Codex can route approval decisions to an automatic reviewer/subagent. This reduces unnecessary human interruptions.

We should adopt the *concept* as:

```text
Permission request
      ↓
hard Policy Engine
      ↓
if policy allows delegation:
      ApprovalAdvisor
      ↓
allow / deny / escalate-to-human recommendation
```

### Critical improvement learned from Codex issues

Public issues show ambiguity where a rule can require `prompt`, yet routing may globally send prompts to auto-review rather than force a human reviewer. For AI Engineering OS, **the policy rule itself must be able to constrain the minimum reviewer class**.

Example:

```text
PolicyRule:
  action: git.push
  decision: REQUIRE_APPROVAL
  minimum_reviewer: HUMAN
```

while a lower-risk rule may allow:

```text
minimum_reviewer: AUTO_REVIEWER
```

Hard policy always wins; an LLM reviewer never overrides an absolute deny.

## 11. Testable policy-as-data

Codex `execpolicy` has declarative allow/prompt/forbidden rules, human-readable justification, example `match`/`not_match` cases validated at load time, and strictest-matching decision semantics.

This is a strong reference for G6.

### Adopt independently

AI Engineering OS policy objects should be:

- typed;
- versioned;
- testable before activation;
- explainable;
- provenance-bearing;
- deterministic when hard policy is evaluated.

### Our ≥1% improvement

Do not stop at command-prefix matching. Evaluate structured action semantics:

```text
Action
  type
  executable
  argv
  filesystem_scope
  network_targets
  secret_refs
  workspace
  task
  side_effect_class
```

and allow policy tests to cover expected allow/deny/reviewer outcomes.

Potential invariant:

> Conflicting hard policies resolve monotonically toward the more restrictive result unless an explicit higher-authority policy version states otherwise.

## 12. Layered config with per-key origin metadata

Codex has a canonical layered config loader that can expose effective config, the source/origin of individual keys, and per-layer versions/fingerprints used for concurrency/conflict handling.

This is excellent for future enterprise UX.

AI Engineering OS should expose not merely:

```text
network = false
```

but:

```text
network = false
source = Organization Security Policy
policy_id = ORG-POLICY-17
version = 8
cannot_override = true
```

### Our ≥1% improvement

Strictly separate:

```text
Hard Policy
Soft Policy / defaults
User preference
Session preference
```

so ordinary config merging can never accidentally weaken security authority.

## 13. Model catalog → fail-fast router preflight

Codex exposes available models and capability metadata such as reasoning options/modalities. A public issue shows a failure mode where app-server could accept a model/reasoning combination cataloged as unsupported and then hang without useful events.

This is an important negative lesson.

AI Engineering OS router must preflight:

```text
provider available?
model available?
requested reasoning supported?
required modalities supported?
tools supported?
context capacity sufficient?
quota/capacity sufficient?
policy allows provider?
```

before Task entry/provider launch when these facts are knowable.

### Our ≥1% improvement

Use a typed `ProviderCapabilitySnapshot` with freshness/provenance and reject impossible combinations before mutating unrelated Task lifecycle state.

This directly reinforces the atomic-entry mindset of TASK-0051 without changing its scope.

## 14. Remote exec-server → future Worker Protocol

Codex exec-server supports remote registration/rendezvous and remote filesystem/process RPCs. It also avoids sending machine-local absolute paths in remote mode by requiring URI-style references.

Good lesson for G12:

```text
Control Plane
   ↓
RemoteWorker lease
   ↓
Environment identity
   ↓
URI/resource references
   ↓
process/filesystem RPC
```

### Our ≥1% improvement

Future remote workers should additionally have:

- authenticated worker identity;
- explicit repository/workspace lease;
- heartbeats/health;
- capability attestation;
- version compatibility;
- scoped secrets;
- network policy;
- durable attempt ownership;
- reconnect/reconciliation;
- no ambient organization credentials;
- provider-neutral execution contracts.

## 15. Fail-closed managed networking

Codex exec protocol includes explicit managed-network enforcement state and indicates the executor should fail closed if enforcement is required but necessary details are absent.

Adopt this exact principle conceptually:

> When a security control is required but cannot be proved active, the execution does not start in a weaker mode.

This should apply to:

- network proxy/policy;
- sandboxing;
- secret broker bindings;
- workspace scope;
- remote worker identity;
- required verification isolation.

## 16. Daemon lifecycle: idempotent + serialized

Codex daemon lifecycle includes idempotent start and serialization of mutating lifecycle commands.

Useful for our G11 daemon.

### Our ≥1% improvement

Codex documentation notes an updater-triggered restart may interrupt active or queued work. AI Engineering OS should prefer a work-aware lifecycle:

```text
UPDATE_AVAILABLE
    ↓
DRAINING
    ↓
checkpoint/cancel safely
    ↓
no owned active Attempt
    ↓
update
    ↓
restart
    ↓
reconcile/resume
```

Emergency security updates can use a stricter policy, but interruption must remain visible and recoverable.

## 17. Thread/fork/resume/ephemeral patterns are useful — but not canonical

Codex supports fresh, resumed, forked and ephemeral threads.

Provider session branching is useful, but for us:

```text
provider thread
≠ Task
≠ Execution
≠ Attempt
```

Potential adapter capabilities:

```text
supports_resume
supports_fork
supports_ephemeral_session
supports_background_process
```

The scheduler may exploit them, but durable project state remains provider-neutral.

## 18. Structured output schema

Codex app-server lets a caller constrain a turn's final assistant message with JSON Schema.

Strong idea for provider adapters:

```text
AgentOutputSchema
```

### Our ≥1% improvement

A schema-valid result is still only an assertion. It must carry evidence references for claims that affect verification or Task transitions.

```text
{
  "status": "implemented",
  "evidenceRefs": [...],
  "testEvidenceRefs": [...]
}
```

No JSON schema can replace verification authority.

## 19. Read-only hints and tool approval metadata

Codex connected apps can use tool metadata such as read-only hints to influence default approval behavior.

Useful marketplace concept, but the metadata must be treated as publisher claims.

### Our ≥1% improvement

Plugin/tool manifest metadata is not automatically trusted. Installation/organization policy can verify/override declared capabilities, and observed behavior can generate security findings if a supposedly read-only tool attempts mutation.

## 20. Worktrees validate our G4 direction

Codex latest release adds experimental isolated worktrees for new/forked sessions.

This is strong market validation for our existing decision, but **not** a new implementation requirement: AI Engineering OS already treats worktree isolation as a core durable invariant rather than an optional experimental session feature.

This is one of the clearest “1%+ better” differentiators:

```text
Codex:
worktree = provider session feature

AI Engineering OS:
workspace/worktree = provider-neutral execution isolation + lease + cleanup + recovery contract
```

## 21. Human questions need not freeze unrelated work

Latest Codex release supports answering questions inline while Codex continues working.

This strengthens our Human Decision Queue design:

```text
HumanDecisionRequest
  blocking_scope:
    TASK
    EXECUTION
    ATTEMPT
    GLOBAL
```

A question should block only the smallest work scope that genuinely depends on it. Unrelated work continues.

### Our ≥1% improvement

Batch compatible decisions, include recommendation/evidence, and track attention latency while preserving precise blocking scope.

## 22. Hierarchical repository instructions are useful, but must stay subordinate

Codex supports scoped AGENTS.md rules with deterministic directory precedence.

AI Engineering OS can support scoped project guidance, but repository text cannot become the ultimate security/architecture authority.

Possible future hierarchy:

```text
System / Organization hard policy
Task / Execution Envelope
Architecture invariants + accepted ADRs
Project policy
Scoped repository guidance
Provider compatibility instructions
```

External/untrusted repository content never gains authority simply because it looks like an instruction file.

## 23. What NOT to adopt

Do not copy/adopt as architecture authority:

- Codex thread/turn state as our Task model;
- provider conversation history as durable project truth;
- provider-specific approval enums in domain logic;
- prefix-only command policy as the final policy model;
- automatic reviewer as a substitute for hard policy or mandatory human approval;
- vendor-specific model catalog semantics in core Task state;
- provider-native worktree state as the workspace authority;
- updater behavior that can silently interrupt work without durable reconciliation;
- raw repository code or the repository as a dependency merely because it is Apache-2.0.

## 24. Negative lessons from public issues

Several public issues are useful specifically as things to design against:

1. **Unsupported model+effort accepted then no useful progress** → capability preflight must be authoritative and fail fast.
2. **Approval routing ambiguity with auto-review** → policy must specify minimum/required reviewer and expose effective routing to UI/hooks.
3. **Auto-review blocked behind unrelated/unbounded persistence flush** → approval/control-path persistence must be bounded and avoid deadlocking security decisions behind noncritical storage.
4. **Completed turn without durable rollout materialization in reported cases** → completion event cannot be considered durable until required canonical persistence commit is acknowledged.
5. **Silent app-server exit mid-turn reports** → daemon/worker supervision requires heartbeat, terminal reconciliation, and explicit orphaned-attempt recovery.

These are architecture lessons, not evidence that Codex is broadly unreliable.

## 25. Pareto prioritization — the 20% from Codex that matters most

Do **not** spend equal effort cloning every Codex feature.

Highest-leverage concepts for AI Engineering OS:

```text
P0 / top 20%
1. Control-plane vs execution-plane separation
2. Fine-grained capability elevation + hard policy
3. Typed/versioned async protocol with generated schemas
4. Provider capability preflight / fail-fast routing
5. Durable remote worker/daemon supervision and reconciliation
6. Context-bound approvals + reviewer routing
7. Deterministic command execution without unnecessary LLM calls
```

Lower-priority parity work:

```text
terminal editing modes
provider-specific TUI polish
model picker parity
vendor-specific thread UX
CLI convenience commands
```

Those may matter eventually, but they do not define why AI Engineering OS exists.

## 26. 1% Improvement matrix

| Codex concept | AI Engineering OS improvement target |
|---|---|
| app-server + exec-server | provider-neutral durable control/execution planes tied to Task/Execution/Attempt |
| approval prompts | typed capability deltas + expiry + context fingerprint + reviewer class |
| auto-review | hard policy first; policy can require human review for sensitive actions |
| execpolicy | structured semantic action policy, versioned/testable/provenance-bearing |
| model catalog | preflight router that rejects unsupported combinations before lifecycle mutation |
| worktrees | durable workspace lease/isolation/recovery contract across all providers |
| daemon lifecycle | work-aware drain/checkpoint/reconcile upgrades |
| thread resume/fork | provider-neutral ContinuationCheckpoint + cross-provider handoff |
| output schema | typed result + evidence links + independent verification |
| config layers | hard-policy vs preference separation + visible provenance for every effective control |
| connected tool metadata | declared capabilities are verified/overridable, not trusted blindly |
| inline human questions | blocking scope minimized + batch decisions + attention metrics |
| remote exec | authenticated leased workers + scoped credentials + durable reconciliation |

## 27. Product differentiation: why AI Engineering OS exists even if Codex is excellent

### Codex answers

> “How can this OpenAI coding agent work effectively on my code?”

### AI Engineering OS answers

> “How do I reliably run an AI engineering organization across Codex, Claude, Gemini, OpenCode and future agents, preserve state when any one provider fails, enforce policy, verify results independently, and only involve humans where their attention has highest value?”

The product should make these advantages obvious in the first-run experience:

```text
One durable task
  → can use multiple providers
  → survives provider/session restart
  → isolated workspace
  → explicit policy
  → evidence-backed verification
  → safe handoff
  → human attention only when necessary
  → delivery/landing reconciliation
```

We should not compete with Codex on “better terminal chat.” We should make Codex more valuable by managing it as one high-quality executor inside a broader system.

## 28. Gate placement

- **G6 Security:** capability deltas, policy-as-data, reviewer routing, context-bound approvals, fail-closed controls.
- **G7 Orchestration:** provider session capability model, scoped human-decision blocking, deterministic vs reasoning work classification.
- **G8 Verification:** structured output + evidence refs, independent approval/review semantics.
- **G9 Context/Continuity:** provider session resume/fork as adapter capability; provider-neutral checkpoints remain authority.
- **G11 Background daemon:** serialized lifecycle, supervision, drain/update/reconcile.
- **G12 Remote Worker:** control/execution protocol split, worker identity, URI resources, sandbox/network contracts.
- **G13 Enterprise:** layered managed policy/config provenance, organization-enforced reviewer/plugin/tool policy.
- **Cross-cutting:** generated schema contracts, protocol versioning, capability negotiation, deterministic command execution.

## 29. Final decision

**Adopt concepts aggressively, implementation independently.** Codex validates several of our existing decisions and provides high-value refinements around protocol design, permission escalation, policy testing, remote execution, daemon lifecycle, and capability negotiation.

The most important strategic lesson is:

```text
Do not build “another Codex”.
Build the control plane that makes Codex + every other strong coding agent
safer, more continuous, more verifiable, and easier to manage together.
```

Current G5/TASK-0051 remains unchanged.
