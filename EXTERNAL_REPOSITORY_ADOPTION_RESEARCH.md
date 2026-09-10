# External Repository Research — Adoption Decisions for AI Engineering OS

> **Snapshot:** 2026-09-10  
> **Purpose:** evaluate external repositories/papers supplied during the project and decide what should influence AI Engineering OS.  
> **Rule:** default mode is independent adoption/transformation of ideas and failure lessons, not copying code or adding dependencies. License/privacy details are secondary unless a concrete implementation, dependency, protocol/format copy, or redistribution decision makes them material. Popularity is not an architecture argument.

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
\n\n---\n\n## ZeroClaw (`zeroclaw-labs/zeroclaw`)\n\n**Reviewed:** 2026-09-09  \n**Verdict:** **ADAPT / STUDY DEEPLY**  \n**Direct runtime dependency:** **No**  \n**License:** dual MIT OR Apache-2.0 in the current upstream repository.\n\n### Current project reality\n\nThe social post understates the current popularity: public GitHub analytics in September 2026 report roughly **32.6–32.7k stars** and ~4.9k forks. The current upstream is no longer a tiny weekend codebase: GitHub shows thousands of commits and a large Rust workspace with many crates.\n\nThe famous footprint/startup comparison is useful as a direction but should not be treated as independent scientific evidence. The published benchmark is project-reported, some comparison values are normalized for edge hardware, and current builds/feature sets may differ. Reproduce measurements locally before using them for architectural or marketing decisions.\n\n### What is genuinely valuable for our project\n\n#### 1. Trait-driven extension points\n\nZeroClaw keeps providers, channels, tools, memory and runtime-adjacent capabilities behind public Rust traits. This strongly validates our Ports & Adapters decision.\n\n**Adopt principle:** keep a small provider-neutral contracts/API crate and wire concrete implementations through factories/registries outside the domain.\n\n#### 2. Compact AGENTS bootstrap + architecture routing map\n\nZeroClaw's root `AGENTS.md` deliberately stays compact and points agents to an architecture map for task-specific reading.\n\n**Adopt:** our `AGENT.md` remains the always-loaded contract; add `docs/architecture/MAP.md` for progressive disclosure.\n\n#### 3. Explicit Single Source of Truth rule\n\nTheir contributor guidance requires naming the canonical source before adding state/caches/config duplication.\n\n**Adopt strongly:** every persistent fact in our architecture must have one bounded-context owner. Other forms are derived views/snapshots/caches with invalidation/provenance.\n\n#### 4. History trimming keeps tool protocol valid\n\nZeroClaw explicitly documents that trimming is lossy but must preserve tool-call/tool-result pairing and payload provenance.\n\n**Adopt:** our Context Engine cannot prune individual sides of a correlated tool exchange. Large results become bounded summaries + artifact references, not dangling protocol objects.\n\n#### 5. Tool execution receipts\n\nZeroClaw implements HMAC-SHA256 receipts for successful tool results so the model cannot forge a new valid claim that a tool executed. This addresses an important distinction between model narration and runtime evidence.\n\n**Adopt and extend:**\n\n- live per-session authenticated receipt for provider-visible tool results;\n- durable audit evidence in our Event Journal/Security Audit for security-relevant actions;\n- never treat a receipt as authorization, correctness or completion evidence by itself.\n\nThe current ZeroClaw history also shows why implementation truth must outrank documentation: an upstream issue documented a period where receipt docs described wiring as shipped while production call sites still passed `None`. Our Architecture/Documentation Engine must therefore tie generated claims to executable/testable evidence where possible.\n\n#### 6. Risk profiles and sandbox backend separation\n\nZeroClaw separates agent risk profiles from OS sandbox implementations and documents backend-specific capability differences (filesystem vs network, etc.).\n\n**Adopt:** policy/risk semantics are domain-level; Landlock/Seatbelt/container/VM details live behind sandbox adapters. Required isolation fails closed.\n\n#### 7. Deterministic SOP engine\n\nZeroClaw has deterministic Standard Operating Procedures with triggers, approval gates, durable run state and resumability.\n\n**Adopt concept:** our adaptive Planner is not the answer for every workflow. Introduce a future deterministic `ProcedureEngine` for known repeatable sequences.\n\n#### 8. Separation of history, memory, files, logs and usage\n\nZeroClaw's documentation is unusually explicit that file content is not automatically memory, logs are diagnostics rather than source-of-truth memory, and tool results require intentional promotion if they should persist.\n\n**Adopt strongly:** preserve separate ownership/lifecycles for session history, long-term memory, artifacts/files, Event Journal, Security Audit, operational logs and usage ledgers.\n\n#### 9. Incremental daemon persistence\n\nThe daemon writes conversation/memory state incrementally rather than relying on a shutdown flush.\n\n**Adopt:** hard-kill recovery is a first-class test case for `agentd`.\n\n### Important warning: runtime efficiency != maintainable architecture\n\nA closed RFC in the ZeroClaw repository criticized the project's growth to a very large Rust codebase, citing roughly 1,076 Rust files / ~776k lines at that point and several extremely large modules. The maintainers did not rewrite it in Python, but the criticism is architecturally useful.\n\n**Our response:**\n\n- CI dependency-boundary tests;\n- narrow crates;\n- generated code separated from hand-written domain code;\n- soft/hard module size review budgets;\n- no speculative abstractions or giant central configuration modules;\n- every cross-context integration through an explicit contract.\n\n### Reject / do not copy blindly\n\n1. **Self-reported marketing benchmark as fact.** Reproduce performance on our own build/hardware.\n2. **Silent sandbox fallback to `none`.** For our required security profiles, missing isolation must block execution instead of weakening it.\n3. **Putting all personal-assistant/channel/hardware capabilities into the coding-core.** Our product remains focused; channels/peripherals are optional integrations, not core domain.\n4. **A giant Rust workspace as an acceptable cost of extensibility.** Replaceability is useful only if the implementation remains navigable.\n5. **In-band receipts as the whole audit system.** We additionally need durable Event Journal/Security Audit evidence.\n\n### Architectural changes accepted\n\n- add Tool Execution Receipt design;\n- add future deterministic Procedure Engine;\n- add `docs/architecture/MAP.md`;\n- strengthen canonical state ownership;\n- strengthen tool-call/tool-result pruning rules;\n- require fail-closed isolation;\n- add Rust architecture fitness and resource benchmarks;\n- add invariants INV-069 through INV-082.\n\n### Priority\n\n**High.** ZeroClaw is one of the most relevant repositories reviewed so far because its implementation domain overlaps directly with our Rust local agent runtime. We should study specific subsystems during implementation, but reproduce the ideas behind our own contracts rather than importing the runtime wholesale.\n
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
- Licensing overview: https://github.com/JuliusBrussee/caveman/blob/main/LICENSING.md
- Engine BSL text: https://github.com/JuliusBrussee/caveman/blob/main/LICENSE.BSL

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

