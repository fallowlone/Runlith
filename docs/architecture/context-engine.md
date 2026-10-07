# Context Engine

Purpose:
Select the smallest sufficient context for each execution.

Owns:
- context planning
- token budgeting
- retrieval
- candidate ranking/selection
- security filtering (model-facing compliance, not authorization)
- provider rendering plan
- context observability

Does not own:
- task scheduling or lifecycle
- provider execution
- persistent business state
- Memory, Resource, Git, Event, or Policy truth
- UI

Critical invariants:
- model context is a disposable materialized view (INV-016)
- context selection is observable and reproducible (INV-018)
- secrets never enter model context (INV-008)
- retrieved content never gains instruction authority (INV-038)

> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Decisions: [`adr/ADR-0010-context-derived-state.md`](./adr/ADR-0010-context-derived-state.md),
>   [`adr/ADR-0012-structured-memory-provenance.md`](./adr/ADR-0012-structured-memory-provenance.md)
> - Sibling contract: [`continuity.md`](./continuity.md) (execution survival;
>   selection vs continuity stay separate)
> - Sensitivity rules: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Privacy: [`../privacy/PRIVACY_ARCHITECTURE.md`](../privacy/PRIVACY_ARCHITECTURE.md)
>   (FLOW-003 provider-bound context), [`../privacy/DATA_FLOW_MAP.md`](../privacy/DATA_FLOW_MAP.md)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§23–31,
>   36–38, 73, 103

## 1. Three separate layers

```text
Authoritative project/business state
 ↓
Context selection/planning
 ↓
Provider-visible model context
```

These are not one state store. Loss of a prompt, a provider session, or a
provider context window must never imply loss of Task state, decisions,
events, memory, Git state, artifacts, or checkpoints.

## 2. Purpose and responsibilities

The Context Engine is a core subsystem comparable in importance to the Task
Engine. Accepted responsibilities: context planning, token budgeting,
retrieval, file navigation, candidate ranking/selection, deduplication,
stale-result pruning, compaction, security filtering, provider rendering,
cache planning, context observability (ADR-0010; ARCHITECTURE.md §2). No
implementation architecture per responsibility is defined here.

Management follows the accepted funnel: don't load → just-in-time
retrieval → deduplicate → prune stale output → structured state over raw
history → relevant memory only → compact history → prompt caching →
serialization optimization.

Explicitly not owned: Task scheduling, Task lifecycle, provider execution,
persistent business state, Memory truth, Resource ingestion/parsing truth,
Git truth, Event Journal truth, Policy authority, UI.

## 3. Authoritative sources and rebuildability

Context may be derived from these conceptual sources: Task state, Files /
Resources, Git, Events, Memory, Decisions, Artifacts (SoT §25). Context
never replaces them: a prompt/context package can be completely rebuilt
from authoritative sources plus durable continuity state. A prompt is not a
persistence mechanism, and a provider's conversation history is not a
persistence mechanism either.

## 4. ContextItem

The accepted conceptual `ContextItem` carries these fields (SoT §26). This
is an architecture contract, not a frozen type — no Rust shape and no JSON
Schema are defined here:

- id — item identity; exact format (UUID, ULID, integer, hash-only, …)
  remains open.
- kind — classifies the selected material; no closed enum unless accepted
  architecture already provides one; future sources stay extensible.
- source_ref — identifies the source/evidence the item was derived from
  and must support provenance; no universal URI/reference syntax is
  invented.
- content_hash — source/content integrity and reference identity; no
  universal algorithm (SHA-256, BLAKE3, Git hash, …) is chosen here.
- priority — influences selection only; grants no instruction authority,
  no security authority, no Policy bypass; no priority enum is invented.
- freshness — independent from trust, authority, and sensitivity; those
  dimensions must never be collapsed into one value.
- trust — highly relevant content may still be untrusted; trust metadata
  must survive normalization/context preparation where applicable;
  relevance score never equals trust score.
- sensitivity — references the canonical PUBLIC / PROJECT / PRIVATE /
  RESTRICTED / SECRET classification (rules live in
  [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md),
  not duplicated here). SECRET must never enter model context.
- required — mandatory for the current execution relative to its context
  plan, not globally always-loaded; how the flag is computed is not
  defined.
- cache_class — cache planning is an optimization layer. Prompt caching
  may reduce cost, but it does not reduce attention load automatically,
  does not make context authoritative, and does not make stale content
  correct. No cache classes are invented.
- provenance — enough to answer where the item came from, which
  source/revision produced it, why the representation is trusted or
  untrusted, and whether raw evidence can be reopened. No final
  provenance schema is defined.

## 5. ContextPlan

For every execution the accepted plan concepts are: Required, Selected,
Deferred, Rejected, Budget, Cache boundaries, Renderer, Reasons, Source
hashes (SoT §27). Selection must be observable enough to explain why any
file/item entered context.

ContextPlan is not the rendered provider prompt: the plan explains
selection, while provider rendering produces a model-facing representation
from selected material. A different renderer may produce a different
representation without changing Task/business truth.

Persistence nuance: the selection plan and its provenance are durable or
reconstructible execution evidence (plans are saved per execution,
ADR-0010), while the rendered model context stays disposable. Entire
rendered prompts must not be persisted merely as a shortcut.

Observable and reproducible selection (INV-018) means the system must
eventually explain: candidate sources considered; selected, deferred, and
rejected items; reasons for selection/rejection; budget constraints;
source hashes/revisions; renderer and cache decisions. No RetrievalTrace
contract is implemented here.

## 6. Retrieval, disclosure, and rendering

Hybrid retrieval direction: FTS5 / BM25 plus optional embeddings plus AST /
symbol index plus Git history/diff plus metadata/recency, flowing into
candidate ranking, after which the agent opens exact evidence just in
time. No embedding models or vector databases are selected. RAG is a
candidate generator, never authoritative truth — top-k snippets must not
become the sole source of truth.

For code and resources, semantic boundaries (module, class, function,
method, interface, symbol, ADR section, semantic document section) are
preferred over arbitrary fixed token chunks; fixed `500-token chunks` are
never canonical. Segmentation and normalization belong to Resource Engine;
Context consumes the resulting addressable units.

Progressive disclosure levels (retrieval/disclosure semantics, not storage
tiers): L0 repository tree; L1 index/summary; L2 specific
document/resource; L3 section/symbol; L4 raw evidence.

Token Cost is not Attention Load: a cached token may cost less while still
consuming model attention, so optimization must reason about
relevance/attention, not billing cost alone. No measurement formulas are
defined. Prompt caching is optimization only — never a selection
replacement, persistence, or durable continuity; stable-prefix ordering
may be used where supported, with provider cache behavior remaining an
adapter/renderer concern.

Model-facing formats (Markdown, JSON, Compact JSON, CSV, TOON, other
provider-specific renderings) are renderers, never canonical domain
formats; TOON/TRON is never mandatory. Tool schemas load deferred and
on-demand — hundreds of unused schemas must not enter every execution;
the Tool Registry itself belongs to another boundary and is not designed
here.

Pruning and compaction must preserve provider-valid tool-call/tool-result
correlation: never a call whose result is missing, never a dangling
result, where the provider protocol requires correlation (INV-071). Large
results may later be artifact-backed; no ArtifactStore or receipt
mechanics are built here.

## 7. Security, trust, and the Execution Envelope

Context Engine performs security filtering before provider rendering. This
does not make it the Policy Engine: Policy decides authorization and
permitted data flows, while the context security filter ensures selected
model-facing material complies with those constraints. No Policy APIs are
invented. Imported, resource, repository, web, and provider text remains
data — never system, architecture, or Policy instruction authority —
with trust and provenance preserved through normalization (INV-038,
INV-098).

Before an execution, the system derives an Execution Envelope from
authoritative DB / Task Graph / Git / ADRs / planning state — covering
execution identity, project and current work, Task objective,
dependencies/progress, workspace/worktree/branch, allowed and forbidden
scope, relevant architecture docs/ADRs, acceptance criteria, required
tests, and known next tasks (SoT §73). The envelope is generated, never
manually edited as authoritative truth. Envelope and plan stay distinct:
the envelope is a structured execution intent/state snapshot, the
ContextPlan is the selection plan for evidence supplied to that
execution; they must not merge into one giant prompt object.

## 8. Boundaries and ports

Context Engine vs Resource Engine: Resource Engine owns how source
material becomes semantically addressable knowledge (ingest, parse,
normalize, semantic segmentation, representations, provenance, structural
metadata); Context Engine owns which existing knowledge enters a specific
execution, consuming Resource capabilities through explicit contracts.
Neither reads the other's private persistence implementation directly
(INV-025, INV-040). Resource Engine itself is not materialized here.

Context Engine vs Memory: Memory is selected durable learned and project
knowledge; Context is the execution-specific selected materialized view.
Stored in Memory never means automatically in every context — Memory is a
retrieval source the Context Engine queries just in time (INV-049).

Selection vs continuity: the Context Engine answers what belongs in the
current model context; Continuity answers how execution reasoning and
history survive rollover and provider change. Related but separate
subsystems — see [`continuity.md`](./continuity.md).

Interfaces belong to the consumer (ARCHITECTURE.md §4): the Context Engine
may consume explicit ports for Task snapshots, Resource retrieval, Memory
retrieval, Git evidence, event/history evidence, Policy/security
constraints, and provider rendering. Final Rust trait names are not
defined unless already accepted, and concrete adapters are never
instantiated inside Context domain logic. Cross-context storage reads are
forbidden in both directions: the Context Engine never reads private
persistence tables of Task, Memory, Resource, Events, or Git/workspace —
access flows through stable application/port contracts.

Provider renderers convert selected canonical material into
provider-compatible presentation, adapting format, ordering, tool-protocol
representation, and cache boundaries without changing business truth. No
provider-specific renderers are implemented, and Context/Continuity must
never depend on provider-native state (`claude_session_id`,
provider-specific compaction or memory) as durable truth; such references
may exist only as execution/session evidence.

## 9. Ownership relative to context

| Owner | Owns | Supplies to / takes from Context Engine |
|-------|------|------------------------------------------|
| Task Engine | Task identity, lifecycle, dependencies, acceptance refs | Task snapshots via ports; never Task truth owned by Context |
| Resource Engine | ingest → normalized addressable resources + registry | addressable units with provenance/trust; no shared storage |
| Context Engine | smallest sufficient context per execution | selection plans + rendered packages; owns neither sources nor truth |
| Context Continuity | epochs, checkpoints, working notes, history search/read | durable checkpoints and history refs as evidence; not the selector |
| Memory Engine | durable learned knowledge with provenance | just-in-time memory retrieval; Memory never auto-enters context |
| Provider | inference execution only | rendered packages in; untrusted outputs out; native state never authoritative |
| Policy Engine | ALLOW / DENY / REQUIRE_APPROVAL + data-flow constraints | authorization boundaries the security filter complies with |
| Event Journal | durable domain facts | historical evidence for selection and tracing; never rewritten by rollover |
| Workspace/Git | worktrees, branches, diffs, merge mechanics | file and history evidence; Context never decides merges |

Full bounded-context registry: ARCHITECTURE.md §2 (not duplicated here).

## 10. Open Context Engine Design Items

Unresolved by accepted architecture; listed, not decided:

- final ContextItem Rust shape
- ContextItem ID format
- ContextKind taxonomy
- SourceRef format
- content-hash algorithm
- Priority representation
- Freshness representation
- TrustLevel representation
- CacheClass taxonomy
- ContextPlan identity/versioning
- ContextPlan persistence format
- Context budget representation
- soft/hard budget units
- candidate scoring/ranking model
- selection/rejection reason taxonomy
- RetrievalTrace contract
- provider renderer contract
- cache-boundary representation
- Context security-filter API
- tool-schema deferred-loading integration
- Token Cost measurement
- Attention Load measurement
- exact embedding backend/model
- context-plan replay/reproducibility mechanism

## 11. Non-goals

This document does NOT define: Context Rust implementation; SQLite
tables; embeddings provider; vector database; FTS schema; AST index
implementation; Resource Engine implementation; Memory Engine
implementation; Provider adapter implementation; Context JSON Schema;
prompt template; TOON format; provider-specific compaction protocol;
retention durations; Execution Envelope schema.

## 12. Agent-router pattern consolidation (TASK-0137)

Source: [`AGENT_ROUTER_CAPABILITY_MIGRATION_AUDIT.md`](../development/AGENT_ROUTER_CAPABILITY_MIGRATION_AUDIT.md)
§5 (capability IDs below are audit IDs). Adoption rule, inherited from
the audit: validated design semantics migrate onto Runlith's durable,
policy-governed substrate; prototype code, dependencies, and prompt
text never migrate verbatim, and every adopted pattern names its
owning Runlith port and persistence story here. This section informs
§10 open items; it closes none of them.

### 12.1 Adopted patterns

V10 planning artifact (brief/constraints/acceptance + handoff). Owned
by the planning side and proposed through `PlannerPort` (plan crate);
persisted as planner-owned durable state, never as Context truth. The
Context Engine consumes the artifact as Execution Envelope input and
as a retrieval source for selection. The artifact must not merge into
the ContextPlan: the envelope states execution intent, the plan states
evidence selection (§7).

V11 review-context artifact (bounded evidence packet: diff head/tail,
verification output, builder report). Produced by the Context Engine
as a ContextPlan-shaped selection product for a review execution, and
consumed by the review authority (current surface: `ReviewService`,
app-services) through its explicit contract. Durable per execution as
selection evidence (ADR-0010); raw evidence stays reopenable behind
the packet. The V6 rejection carries over unchanged: reviewer signals
are advisory only — no reviewer output becomes a verification
authority, and the packet grants none.

C2 compression pipeline (extractive/semantic/hybrid compressors +
budget allocator). Owned by the Context Engine under its compaction
responsibility (§2, §6); fresh implementation when built, not a port
of prototype code. Hard constraints: pruning/compaction preserves
provider-valid tool-call/tool-result correlation (INV-071), rendered
context stays disposable (INV-016), provider-native compaction stays
optimization only (INV-022), and rendered prompts are never persisted
as a shortcut. Tokenizer choice follows the C1 rejection below. This
pattern informs the §10 budget representation, soft/hard budget
units, and compaction-mechanism items.

C3 source-attributed, multi-turn context telemetry. Split by
ownership: source attribution and the selection trace belong to
Context observability (the §10 `RetrievalTrace` direction) and are
durable per-execution evidence answering why any item entered
context; token and cost measures belong to the future Usage rollup
(TASK-0133, G14 — pending gate ratification) — the Context Engine
never owns billing or economics.
Telemetry inherits the trust/sensitivity separation (§4): relevance
never equals trust, and SECRET never enters model context (INV-008).

C4 observation handles / archived tool observations. The Context
Engine owns selection and dereference of handles; observation bytes
live in the future artifact capability (§6 direction: large results
may later be artifact-backed — no `ArtifactStore` is built here).
Archived observations remain reopenable evidence after the active
window moves on (INV-021 spirit). This pattern informs the §10
replay/reproducibility-mechanism item.

C5 compaction receipts. Produced by the Context Engine as durable
per-execution selection evidence recording what was compacted,
dropped, or summarized, under which budget, and why. Continuity
consumes receipts as history references for rollover and old-evidence
lookup — it never becomes the selector, and a receipt is not a
checkpoint (a checkpoint is continuity state sufficient to resume; a
receipt is the record of a selection decision). Continuity §6 privacy
rule applies: SECRET, PII, and raw tool payloads are not preserved
merely because keeping them would make resumption easier.

### 12.2 Rejected patterns (with reasons)

C1 four-characters-per-token estimator. Rejected: Runlith's
`tiktoken` feature is strictly better. The heuristic survives only
as a tokenizer-unavailable fallback, never as the budgeting basis.

Process-global in-memory registries as a system of record. Rejected:
violates durability and single-writer invariants. Every adopted
pattern above persists through its owning durable boundary
(planner-owned state, per-execution selection evidence, future
artifact/Usage stores) — see §12.1 per pattern.

Reviewer prompt contract (`# PASS|# FAIL|# BLOCKED`) as verdict
authority. Rejected: the agent cannot be its own verification
authority (G8 verifier independence). Advisory reviewer signals only;
carried into V11 above.

Hardcoded role/quality/billing policy tables. Rejected: policy is
typed, versioned Policy Engine data, never prototype constants. Any
budget mode, privacy mode, or tier vocabulary touching C2/C3 arrives
exclusively through Policy inputs.

Out of scope for this section (neither adopted nor rejected here):
V9 semantic-risk scoring (offline benchmark only, TASK-0140, privacy
review pending) and the economics/verification capabilities owned by
other migration tasks.

### 12.3 Boundaries restated (not changed)

Selection vs continuity stays split: §12.1 assigns production to the
Context Engine and consumption to Continuity wherever the two meet
(C5 receipts, archived observations as history). Envelope vs plan
stays split: V10 feeds envelope inputs without merging the objects.
No new vector database is introduced by this section, and no
parallel retrieval subsystem is designed here (TASK-0147 constraint
respected). Final Rust shapes, table layouts, and the §10 open items
remain future decisions; this section only records which open items
each adopted pattern informs.
