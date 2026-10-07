# Context Continuity

Purpose:
Let long executions survive finite context windows, rollover, and provider
switches on durable application state — never on provider-native state.

Owns:
- execution epochs
- durable checkpoints
- working notes
- history continuity and old-evidence lookup
- rollover gating (NO CHECKPOINT → NO ROLLOVER)

Does not own:
- current model-context selection (Context Engine)
- durable project knowledge (Memory)
- domain history (Event Journal)
- provider-native compaction or memory

Critical invariants:
- no context epoch is discarded before a durable checkpoint (INV-020)
- historical evidence remains retrievable after rollover (INV-021)
- provider-native compaction is optimization, not persistence (INV-022)
- tasks migrate between providers without state loss (INV-023)

> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Decisions: [`adr/ADR-0011-provider-independent-context-continuity.md`](./adr/ADR-0011-provider-independent-context-continuity.md),
>   [`adr/ADR-0012-structured-memory-provenance.md`](./adr/ADR-0012-structured-memory-provenance.md)
> - Sibling contract: [`context-engine.md`](./context-engine.md) (current
>   selection; continuity is survival, not selection)
> - Journal boundary: [`events.md`](./events.md) (rollover never rewrites
>   domain history)
> - Sensitivity rules: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Privacy: [`../privacy/PRIVACY_ARCHITECTURE.md`](../privacy/PRIVACY_ARCHITECTURE.md)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§32–36,
>   ZeroClaw-derived §§4, 6, 9

## 1. Provider-independent continuity

```text
provider context window
!=
durable execution continuity
```

Continuity works across Claude, Codex, OpenCode, and future providers
without making any provider-native state authoritative.

## 2. Context Epoch

> Epoch identity and chain scope proposed by
> [`ADR-0043`](./adr/ADR-0043-g9-continuity-contract.md) (TASK-0168,
> pending ratification).

Work is organized as bounded active context windows:

```text
Task
 ├── Context Epoch 001
 ├── Context Epoch 002
 ├── Context Epoch 003
 └── ...
```

An epoch is a bounded active context window. No final Epoch ID format is
selected prior to ADR-0043 ratification, and no provider-specific Epoch
types exist.

## 3. Three continuity levels

Level 1 — Provider Context: provider-native context, provider-native
compaction, provider-native memory and continuation behavior. Provider
optimization only, never durable application truth.

Level 2 — Execution Continuity: application-owned epochs, checkpoints,
working notes, and searchable task history. This is the core
provider-independent continuity layer, and the layer this document
formalizes.

Level 3 — Durable Project/Task Memory: decisions, architecture, failures,
skills, repository knowledge. Owned by Memory and other durable project
knowledge boundaries. Continuity may retrieve Memory; Continuity is not
Memory itself.

## 4. Rollover algorithm

> Transaction, verification, threshold and crash semantics proposed by
> [`ADR-0043`](./adr/ADR-0043-g9-continuity-contract.md) (TASK-0168,
> pending ratification).

The accepted sequence (SoT §33):

```text
1. Budget Monitor observes soft/hard limits.

2. Before soft limit, create structured checkpoint.

3. Persist checkpoint + notes + source refs durably.

4. Index old epoch.

5. Verify persistence.

6. Only after successful checkpoint create new epoch.

7. Build new context from:
   stable instructions
   + Task/Execution Envelope
   + checkpoint
   + current working set

8. Old epochs remain available via history search/read.

9. Raw evidence remains source of truth.
```

The critical rule:

```text
NO CHECKPOINT
→
NO ROLLOVER
```

No context epoch is discarded before a durable checkpoint (INV-020). The
checkpoint transaction must complete successfully before the previous
epoch is discarded or rolled over; SQL mechanics and storage tables are
not defined here. If checkpoint persistence fails, rollover must not
proceed as though continuity were safely persisted. No final Task
lifecycle outcome for a failed checkpoint is invented — resulting
execution behavior belongs to later implementation and recovery policy.

## 5. Checkpoints, history, and provider migration

> Checkpoint schema, history contract and migration protocol proposed
> by [`ADR-0043`](./adr/ADR-0043-g9-continuity-contract.md) (TASK-0168,
> pending ratification).

A checkpoint is structured continuity state sufficient to continue work
without treating the full previous transcript as authoritative state. It
may conceptually carry working state, important notes, source references,
evidence references, and continuation information — no exact schema and
no mandatory fields are defined prior to ADR-0043 ratification. A checkpoint helps resume but replaces
nothing: not the source file, Task, decision, event, artifact, or Git
state where exact evidence is needed. Raw evidence stays authoritative;
summary-of-summary chains must never become truth.

History follows the two-stage pattern: `history.search(...)` first, then
`history.read(exact_ref)` — never bulk-loading entire old sessions by
default. Accepted search capabilities: FTS/BM25, metadata filters, Task
filters, Epoch filters, event-type filters, optional embeddings. No
implementation libraries are chosen. Historical evidence remains
retrievable after rollover (INV-021): old windows may leave the active
context but must not become irretrievable merely because rollover
occurred.

A Task may migrate Claude → Codex (or between any providers) without
losing authoritative state (INV-023). Migration may produce a
new/recovered Attempt, a new provider Session, and a new rendered
context — while Task and continuity truth survive independently.
Provider-native compaction stays optimization only: never the sole
persistence mechanism, checkpoint replacement, Memory, Event Journal, or
Task state (INV-022). Provider and session transcript history is useful
historical evidence, but Session History is not Continuity authoritative
state — correctness-critical continuation must survive independently of
raw chat transcript retention (INV-110).

## 6. Boundaries

> Working-notes representation per ADR-0043 §6 and privacy bounds per
> ADR-0043 §13 proposed by
> [`ADR-0043`](./adr/ADR-0043-g9-continuity-contract.md) (TASK-0168,
> pending ratification).

Working notes belong to Level 2 execution continuity. They are never
automatically promoted to Memory, Decision, ADR, or Task state —
promotion to durable project knowledge requires the owning subsystem and
its process.

Continuity stores what an execution needs in order to continue; Memory
stores selected knowledge intended to survive and be reused across
execution contexts under Memory policy. The two stores stay conceptually
separate. Context checkpoints and history may reference or consume Event
Journal evidence, but they never replace the journal, rollover never
rewrites domain history, and no continuity event taxonomy is created.

Context, history, and checkpoints remain subject to Data Classification
and Privacy Architecture: SECRET, PII, and raw tool payloads must not be
preserved merely because keeping them would make resumption easier
(prior to ADR-0043 ratification; on acceptance SECRET is never
persisted — opaque refs only — per ADR-0043 §13).
Exact retention and cleanup policies are separate privacy/storage work.

Rendered context packages are disposable; Context Plans, checkpoints, and
continuity references are durable or reconstructible execution evidence
as architecture requires. Giant prompts must not be persisted merely as
a shortcut.

Continuity is one recovery input alongside Task state, Execution/Attempt
state, worktree state, the Event Journal, Policy state, and idempotency
evidence. Checkpoint replay alone is never claimed to recover all
execution side effects. On stateful failover, side effects are never
transparently replayed across providers or models (INV-066): continuity
restores context and evidence, never authorization for blind repetition
of filesystem writes, tool calls, network actions, or privileged
operations.

## 7. Ownership relative to continuity

| Owner | Owns | Relation to Continuity |
|-------|------|------------------------|
| Task Engine | Task identity, lifecycle, dependencies, acceptance refs | migrated Tasks keep authoritative state; continuity never owns lifecycle |
| Resource Engine | normalized addressable resources + registry | raw evidence stays authoritative over checkpoint summaries |
| Context Engine | smallest sufficient context per execution | selects the current window; continuity preserves survival across windows |
| Context Continuity | epochs, checkpoints, working notes, history search/read | this contract; gates rollover on durable checkpoints |
| Memory Engine | durable learned knowledge with provenance | Level 3 knowledge; continuity retrieves, never absorbs |
| Provider | inference execution only | native context/compaction is optimization; migration-safe by design |
| Policy Engine | ALLOW / DENY / REQUIRE_APPROVAL + constraints | failover replay stays denied without authorization |
| Event Journal | durable domain facts | rollover references evidence; history is never rewritten |
| Workspace/Git | worktrees, branches, diffs, merge mechanics | worktree state is a sibling recovery input |

Full bounded-context registry: ARCHITECTURE.md §2 (not duplicated here).

## 8. Open Context Continuity Design Items

Unresolved by accepted architecture; listed, not decided. Items marked
[ADR-0043] are proposed by
[`ADR-0043`](./adr/ADR-0043-g9-continuity-contract.md) (TASK-0168,
pending ratification); the rest stay open:

- Context Epoch ID format [ADR-0043 §1, with chain scope]
- checkpoint identity format [ADR-0043 §2]
- checkpoint schema [ADR-0043 §2]
- checkpoint persistence format [open; exact tables are TASK-0171's]
- checkpoint transaction API [ADR-0043 §3 semantics; exact Rust
  shapes are TASK-0171's]
- checkpoint verification criteria [ADR-0043 §3]
- soft/hard rollover thresholds [ADR-0042 budgets as source, ADR-0043
  §4 gating semantics]
- Budget Monitor ownership/API [ADR-0043 §4 ownership and
  trigger/gating semantics; call mechanics are TASK-0171's]
- current-working-set definition [ADR-0043 §5]
- working-notes representation [ADR-0043 §6]
- history reference format [ADR-0043 §7; search/read Rust shapes
  are TASK-0171's]
- history index/storage implementation [open]
- history result ranking [ADR-0043 §7 signal disclosure; weight
  tuning is TASK-0171's]
- history retention/cleanup [durations open, owned by privacy/storage
  policy; enforcement mechanics are TASK-0171's]
- provider Session ↔ Epoch linkage [ADR-0043 §8]
- crash during checkpoint semantics [ADR-0043 §9]
- crash between checkpoint commit and new-epoch activation [ADR-0043
  §9]
- concurrent execution/epoch semantics [ADR-0043 §10]
- provider migration resume protocol [ADR-0043 §11]
- checkpoint version compatibility [ADR-0043 §12]
- checkpoint supersession semantics [ADR-0043 §12]

## 9. Non-goals

This document does NOT define: Continuity Rust implementation; SQLite
tables; checkpoint schema; history index implementation; Memory Engine
implementation; Provider adapter implementation; Checkpoint JSON Schema;
provider-specific compaction protocol; retention durations; migration
resume protocol implementation. This list scopes this document only —
decisions themselves may live in accepted ADRs (e.g. ADR-0043 once
accepted).
