# ADR-0042 — G9 Context Engine contract

- Status: **Accepted** (TASK-0167; ratified 2026-10-07 by owner decision
  after three independent review rounds: R1 15 findings, R2 3 new, R3
  PASS; pre-acceptance commit 6d50845)
- Date: 2026-10-07
- Gate: G9

## Problem

G9 owns retrieval, token budgets, context plans, epochs, checkpoints and
history search, but `context-engine.md` section 10 lists 24 open design
items and resolves none of them. Section 12 (TASK-0137) adopts six
agent-router patterns and explicitly closes nothing. Meanwhile three
shipped boundaries already touch context selection without a shared
contract: the repo finder (`FinderService` with a `PassthroughFilter`
that redacts nothing), the planner port (`evidence_refs` accepted as
identity-only by ADR-0029), and review admission (`ReviewService`).
Without a frozen vocabulary, TASK-0170 through TASK-0175 would each
invent their own item shape, budget unit and trace format — the same
class of drift that produced the G7 handoff gap (ADR-0027 TASK-0097
amendment).

This ADR freezes the context-selection vocabulary. Continuity
(epochs/checkpoints/history) is TASK-0168; TAKEOVER is TASK-0169. This
ADR covers selection only, plus the provenance envelope both later
contracts consume.

## Decision

### 1. ContextItem (normative fields)

The conceptual `ContextItem` of `context-engine.md` section 4 gains
normative field types. Field *names* are fixed; Rust syntax is the
implementer's, but every field below must exist with the stated
semantics:

| field | type contract |
|---|---|
| `id` | opaque string, `ctxitem_` + 26-char Crockford Base32 (ULID shape, time-sortable). Sortability is required; the ULID crate is not mandated. |
| `kind` | closed `ContextKind` v1 enum (see 2). Unknown kinds fail closed. |
| `source_ref` | typed `SourceRef` (see 3). Bare strings are rejected. |
| `content_hash` | `sha256:<64 lowercase hex>` over canonical source bytes (SHA-256 matches the plan-hash precedent in `crates/plan/src/hash.rs`). Namespaces are disjoint from the finder seam: `ctxitem_` ids never collide with finder `EvidenceId` (`rf1.` namespace, `crates/repo-finder/src/model.rs`), and finder hashes are bare 64-hex — the translation rule at the seam is to strip or add the `sha256:` prefix, never to reinterpret the bytes. |
| `priority` | `u16` rank, selection-only. Grants no instruction, security or Policy authority. |
| `freshness` | `Freshness` struct: `observed_at` (Unix millis UTC), `source_rev` binding, optional `stale_after`. Never merged with trust or sensitivity. |
| `trust` | `Trust` struct: level `Verified` / `Unverified` / `Untrusted` plus a machine-readable reason. Relevance never equals trust. |
| `sensitivity` | `DATA_CLASSIFICATION.md` level (`PUBLIC` / `PROJECT` / `PRIVATE` / `RESTRICTED` / `SECRET`). SECRET never enters model context (INV-008). |
| `required` | `bool`, plan-relative: mandatory for this execution's plan, never globally always-loaded. |
| `cache_class` | `CacheClass` v1: `StablePrefix` / `SessionEphemeral` / `NoCache`. Optimization hint only; never selection, persistence or correctness. |
| `provenance` | `ProvenanceEnvelope` (see 4). Mandatory on every selected item. |

### 2. ContextKind v1 (closed)

`TaskSnapshot`, `FileExcerpt`, `SymbolSpan`, `DiffHunk`, `PlanRevision`,
`AttemptRecord`, `HumanDecision`, `EventFact`, `MemoryNote`,
`ReviewPacket`, `ToolObservation`, `CompactionReceipt`. New kinds arrive
only through an ADR amendment; an implementation that meets an unknown
kind refuses plan admission (rule CX-1) instead of guessing.

### 3. SourceRef (typed, revision-bound)

One of `TaskRef`, `RepoPath`, `PlanRef`, `AttemptRef`, `DecisionRef`,
`EventSeq`, `MemoryRef`, `ObservationHandle`. Every variant carries the
revision binding of its source (commit/tree, revision_seq, event seq).
Epoch and memory bindings stay opaque strings: no Epoch ID format is
selected (`continuity.md` leaves it open for TASK-0168) and Memory
truth shapes belong to their owning gate. A ref without a revision
binding is malformed and refused (rule CX-2).

### 4. ProvenanceEnvelope

The roadmap's Context Provenance Envelope, normative fields: `source`
(SourceRef), `source_rev`, `retrieved_at`, `selector` (which rule or
query produced the item), `transform_chain` (ordered normalizations
applied, possibly empty), `reopen_ref` (handle sufficient to reopen raw
evidence, or explicit `NotReopenable` with reason), `permission_scope`
(the authorization scope the item was selected under, as an opaque
string referencing the Policy decision), `confidence` (selector
confidence as permille `0..=1000`; confidence never overrides trust or
sensitivity), `supersession` (optional: which item id this envelope
supersedes, with reason; a superseded item is Deferred with reason
`Superseded`). The envelope answers where the item came from, which
revision produced it, why its representation is trusted or untrusted,
and whether raw evidence can be reopened.

### 5. ContextPlan

Identity `ctxplan_<ulid26>`, `revision_seq` per execution, sections
`Required` / `Selected` / `Deferred` / `Rejected`, `Budget`, cache
boundaries, renderer id, reasons, source hashes. Cache boundaries are
recorded as an ordered list of (`CacheClass`, item-id range) segments.
The renderer id is an opaque string `renderer:<name>`; no renderer
registry is built here. Every deferred or rejected item carries a
closed `ReasonCode` v1: `BudgetExceeded`, `Superseded`,
`SensitivityRefusal`, `StaleSource`, `DuplicateOf`, `TrustBelowFloor`,
`RendererLimit`, `DeferredByPolicy`. Rendered prompts are never plan
evidence (rule CX-4).

### 6. Budget representation

Budgets are counted in tokens through a `TokenCounter` port with two
shapes: an exact tokenizer when one exists in the deployment, and the
`chars/4` estimator explicitly labeled as an estimate. No exact
tokenizer ships in the repo today, so the heuristic is the only
available counter — but it is never a silent basis: every budget
records its counter method, and the trace carries it (section 7). A
plan carries `soft_tokens` and `hard_tokens`: breaching soft triggers
prune-or-compact, breaching hard refuses admission before render (rule
CX-6). Attention Load is tracked separately as item count plus total
tokens; no fancier formula is adopted. Token *cost* (money) is never
computed here: counts are reported, economics belong to the G14 usage
rollup (TASK-0133).

### 7. RetrievalTrace contract

Per-execution durable trace, persisted as execution evidence per
ADR-0010: candidates considered; per-candidate decision (selected /
deferred / rejected + ReasonCode + rule); budget snapshot including
the counter method; source revisions; renderer and cache decisions.
The trace carries hashes and
references only — never secret payloads, never rendered prompt bytes.
It must answer "why did this file enter context?" without re-running
selection.

### 8. Security-filter API

Two stages: Policy supplies data-flow constraints (the Context Engine
never invents authorization); the filter enforces them over selected
items before render. Filter functions are total: they never fail and
never panic — filtering degrades to redaction, never to errors (the
`ContentFilter` totality precedent in
`crates/app-services/src/finder_service.rs`, whose trait returns display
bytes and has no exclusion signal). Exclusion is expressed by the
selector, never by the filter: an excluded item moves to Rejected with
its ReasonCode. SECRET exclusion is fail-closed. Filtering applies to
display bytes; integrity hashes bind pre-filter bytes (the
`FinderSpan.content_hash` precedent).

### 9. Replay and reproducibility

Selection is deterministic given (plan inputs, source revisions, Policy
fingerprint): the same `ContextPlan` against the same revisions
reproduces the recorded decision, and the freshness re-check is itself
a traced step with its own inputs recorded (rule CX-5, INV-018).

### 10. Renderer contract

A renderer is a pure function over (selected items, cache boundaries)
producing model-facing bytes; the choice of renderer is recorded in the
plan. Renderers never change business truth, are never canonical
formats, and never persist as evidence. Tool schemas load deferred and
on demand. Pruning and compaction preserve provider-valid
tool-call/tool-result correlation (INV-071).

### Executable rules

- **CX-1** Unknown `ContextKind` refuses plan admission; no guessing.
- **CX-2** `SourceRef` without a revision binding is malformed.
- **CX-3** SECRET items are never selected for model context; the
  refusal is traced as `SensitivityRefusal` without content.
- **CX-4** Rendered model context is never persisted as plan evidence.
  Only the plan, the trace and hashes persist.
- **CX-5** Selection replays the recorded decision from recorded
  revisions; the freshness re-check is a traced step, not an untraced
  hedge.
- **CX-6** The hard budget is enforced before render; soft breach
  prunes or compacts, never silently overflows.
- **CX-7** Trust, sensitivity and freshness stay separate fields with
  separate tests; no implementation may collapse them into one score.
- **CX-8** Whole-repository packing is allowed only as an explicit
  bounded fallback recorded in the trace with its reason (INV-115).
- **CX-9** A selected item without a provenance envelope is a malformed
  plan and refuses admission.
- **CX-10** Selection reads authoritative state only through explicit
  ports; cross-context storage reads fail the design (INV-025, INV-040).

### Section 12 pattern grounding

Each adopted pattern names its contract surface (no new code here):

| pattern | surface |
|---|---|
| V10 planning artifact | Future contract: planner-owned artifact feeds Execution Envelope inputs; `PlannerPort` (`crates/plan/src/model.rs`) stays the proposal-only seam and carries no envelope field today. Never merged into `ContextPlan`. |
| V11 review-context artifact | Future contract: `ContextPlan`-shaped product consumed through an extension of the `ReviewService` admission boundary (today `B1ReviewInput` / `ReviewAdmissionQuery` in `crates/app-services/src/review_service.rs`); advisory only, grants no verification authority. |
| C2 compression pipeline | Compaction under section 6 budgets and rule CX-6; correlation preserved per INV-071. |
| C3 telemetry | Source attribution and selection trace live in the section 7 trace; token/cost measures belong to TASK-0133 (G14). |
| C4 observation handles | `ObservationHandle` source refs with `reopen_ref`; bytes live in a future artifact capability, not built here. |
| C5 compaction receipts | `CompactionReceipt` items consumed by Continuity as history references; a receipt is never a checkpoint. |

### Dogfood detachment per capability

- Provenance envelope + trace: independently useful and testable
  without Continuity (select, explain, replay on one execution).
- Finder seam: already standalone; TASK-0173/0174 keep it so.
- Budgets and renderer choice: unit-testable without persistence.
- Nothing here requires epochs, checkpoints or the entity graph.

### Resolved vs still open

Resolved from section 10: ContextItem shape, ID format, ContextKind
taxonomy, SourceRef format, content-hash algorithm, priority/freshness/
trust/cache-class representation, plan identity and versioning, budget
representation and units, selection/rejection reason taxonomy,
RetrievalTrace contract, renderer contract, cache-boundary
representation, security-filter API, Token Cost and Attention Load
measurement, replay mechanism.

Still open (explicitly not decided here): `ContextPlan` persistence
format (durable evidence is required; exact tables are TASK-0170's);
the candidate scoring/ranking model (`priority` is a rank input, not
a scoring model); exact embedding backend and model (embeddings stay
optional behind a port); tool-schema deferred-loading integration
mechanics; retention durations; Execution Envelope schema (a section
11 non-goal of `context-engine.md`); Memory/Resource truth shapes
(owning gates).

## Alternatives considered

- One merged context object (envelope + plan + prompt): rejected; it
  collapses the envelope/plan split of section 7 and makes the prompt
  look authoritative, against INV-016.
- UUIDs instead of ULID-shaped ids: rejected; time-sortability without
  coordination matters for traces and epochs, and the shape costs
  nothing.
- BLAKE3 instead of SHA-256: rejected; speed is irrelevant at
  selection cardinality, and SHA-256 matches the plan-hash precedent
  (`crates/plan/src/hash.rs`: SHA-256 over deterministic canonical
  bytes).
- Open-string kinds with best-effort handling: rejected; fail-closed
  admission (CX-1) is the only behavior that keeps a stale selector
  from silently misreading new material.
- Money-cost accounting in the Context Engine: rejected; economics
  belong to the G14 usage rollup, and mixing them into selection
  would make budgets untestable without pricing data.

## Consequences

- TASK-0170, TASK-0174 and TASK-0175 transcribe this ADR's selection
  vocabulary and provenance envelope instead of inventing their own;
  TASK-0171 and TASK-0172 additionally transcribe the TASK-0168
  continuity and TASK-0169 TAKEOVER contracts. TASK-0173 transcribes
  nothing here (it stays inside the finder seam on its TASK-0147
  dependency) but is constrained by the section 1 namespace rule: the
  hash16 direction applies to finder evidence ids, never to
  `ctxitem_` ids. Any deviation needs an ADR amendment, not a
  queue-entry restatement (the TASK-0097 lesson).
- `context-engine.md` sections 4, 5 and 10 gain "Proposed by ADR-0042
  (pending ratification)" annotations, flipped to "Decided by" on
  acceptance together with a rewrite of the stale "remains open"
  bodies; the section 10 list stays as the record with proposed items
  marked.
- The finder `ContentFilter` seam is unchanged; TASK-0174 replaces
  `PassthroughFilter` under section 8 totality and hash-binding rules.
- No schema, IPC or dependency change in this task: this ADR is
  contract only. New tables are expected in TASK-0170/0171.

## Security impact

SECRET exclusion is fail-closed at selection (CX-3), not at render;
refusals are traced without content, so the trace itself cannot leak
what it refused. All selected material stays untrusted data through
normalization (INV-038, INV-098); priority and trust metadata never
grant instruction authority. Filter totality removes the
fail-open-on-error shape. Resolved content crossing the provider
boundary stays gated on the TASK-0175 privacy policy; nothing here
authorizes it.

## Migration and replacement impact

Retrieval engines (FTS5/BM25, optional embeddings, AST/symbol index)
and renderers stay replaceable behind ports; this ADR pins vocabulary,
not implementations. Existing finder evidence (`RepoProvenance`,
`EvidenceParts`, continuation tokens) is carried forward, not
replaced; TASK-0173/0174 extend the seam within these rules. No
backfill: no durable context-selection state exists yet.

## References

- `docs/architecture/context-engine.md` sections 4, 5, 7, 8, 10, 12
- `docs/architecture/continuity.md` (sibling boundary; selection vs
  survival stay separate)
- ADR-0010 (derived disposable state), ADR-0011 (continuity),
  ADR-0029 (identity-only evidence refs; option B is TASK-0175)
- `docs/architecture/INVARIANTS.md`: INV-008, INV-016, INV-018,
  INV-025, INV-038, INV-040, INV-071, INV-098, INV-115 (each cited
  invariant is invoked in the body above)
- `docs/security/DATA_CLASSIFICATION.md` (sensitivity levels)
- `planning/roadmap.yaml` G9 (Provenance Envelope, finder, TAKEOVER,
  conditional entity graph)
