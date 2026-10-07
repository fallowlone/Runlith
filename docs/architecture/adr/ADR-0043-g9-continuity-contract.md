# ADR-0043 — G9 Continuity contract

- Status: **Proposed** (TASK-0168; ratification pending)
- Date: 2026-10-07
- Gate: G9

## Problem

G9 owns epochs, checkpoints and history search, but `continuity.md`
section 8 lists 21 open design items and resolves none of them. No
epoch or checkpoint identity exists, crash windows are unnamed, and
concurrent-execution semantics are undecided. TASK-0171 (epochs,
checkpoints, history) and TASK-0172 (TAKEOVER, which must state what
happens to in-flight execution) cannot be implemented without frozen
durability, recovery and privacy semantics. Selection vocabulary is
settled by ADR-0042; this ADR settles survival. The two stay separate:
the Context Engine answers what belongs in the current window,
Continuity answers how execution survives rollover and provider change.

## Decision

### 1. Epoch identity and chain scope

Epoch identity is `ctxepoch_<ulid26>` with a monotonic `epoch_seq`
(`u64`) within its chain. The ULID is the identity (globally
unique); the sequence is per-chain ordering only. A chain is scoped
to (task, execution): concurrent executions never share an epoch
chain (rule CR-9), and cross-chain reads happen only through history
references. Exactly one epoch is ACTIVE per chain; older epochs are
INDEXED and read-only. No provider-specific epoch types exist.
Selection-side epoch bindings stay opaque strings per ADR-0042
section 3 until a future amendment needs a typed variant.

### 2. Checkpoint identity and schema

Checkpoint identity is `ctxckpt_<ulid26>`. Normative fields:

| field | contract |
|---|---|
| `epoch_ref` | the ACTIVE epoch this checkpoint closes (mandatory). |
| `task_ref`, `execution_ref` | owning chain (mandatory). |
| `created_at` | Unix millis UTC (mandatory). |
| `format_version` | `u32`, starting at 1 (mandatory). |
| `parent_checkpoint` | previous checkpoint id, or null for genesis (supersession chain). |
| `working_state` | bounded structured notes references (never raw transcripts). |
| `source_refs` | ADR-0042 `SourceRef` list, possibly empty but always present. |
| `evidence_refs` | opaque evidence identity strings (ADR-0029 semantics). |
| `continuation` | next actions, open questions, blockers (bounded text). |
| `context_plan_ref` | the ACTIVE epoch's `ContextPlan` id (ADR-0042 §5). |
| `sensitivity_ceiling` | max DATA_CLASSIFICATION level carried (see 13). |

A checkpoint references raw evidence; it never embeds transcripts, tool
payloads or files. Summaries point to reopenable refs — a
summary-of-summary chain must never become truth.

### 3. Checkpoint transaction and verification

Commit is atomic over (checkpoint row, epoch index entry, history
references) and returns a `CheckpointReceipt` (`checkpoint_id`,
`epoch_id`, `committed_at`, store revision). Exact Rust shapes for
commit, `history.search` and `history.read` are TASK-0171's; this
section pins atomicity, receipt fields and staging semantics only.
Writes land in staging
first; only the commit pointer swap makes a checkpoint readable —
staging is never readable as committed (rule CR-2). Verification
criteria: read-back equals written bytes, content hashes compare, the
parent chain is intact. If verification fails, the checkpoint does not
exist and the old epoch stays ACTIVE.

### 4. Rollover thresholds and Budget Monitor ownership

Soft and hard token limits are the ADR-0042 `ContextPlan` budget
values — no second budget vocabulary is invented. The Budget Monitor is
a Continuity-owned observer over plan budgets plus provider usage
signals (adapter-provided, untrusted hints). It may trigger checkpoint
and rollover; it never selects content. Rollover checkpointing begins
at the soft limit; past the hard limit, new execution cycles block
until rollover completes (fail-closed), recorded as a
budget-snapshot/rejection entry per ADR-0042 section 7 and carrying
no payload bytes.

### 5. Current working set

The current working set is the ACTIVE epoch's selected `ContextPlan`
plus open working notes plus in-flight tool-correlation pairs. It is
bounded and disposable: on resume it is rebuilt from the committed
checkpoint plus the plan, never trusted from a crashed process image.

### 6. Working notes representation

Timestamped entries: `note_id`, `created_at`, `author`
(`Human` / `Agent` / `System`), body as authored observation text
inline (at most 4 KiB) or reference for anything larger, and a
sensitivity label. The inline bound is authored text only: raw
evidence — transcripts, tool payloads, files — always travels by
reference (`reopen_ref`, `SourceRef`, artifact handle), never
embedded. Notes are execution evidence. They are never automatically
promoted to Memory, Decision, ADR or Task state — promotion needs
the owning subsystem and its process.

### 7. History search and read contract

Two stages, no bulk dump by default (rule CR-4):
`history.search(query, filters)` returns `HistoryHit` values
(`history_ref`, `epoch_id`, disclosed rank signals, snippet method);
`history.read(exact_ref)` returns bounded content for one ref.
Reference format is `histref_<ulid26>`, opaque, epoch-bound inside.
Ranking discloses its signals (recency plus full-text rank where an
FTS index exists); exact weight tuning is TASK-0171's. Embeddings
stay optional. Accepted filters: metadata, task, epoch, event type.

### 8. Provider Session to Epoch linkage

Provider sessions attach as append-only evidence: `SessionLink`
(`session_ref` as an opaque provider string, `epoch_id`,
`attempt_ref`, `provider_id`). Provider-native session state and
compaction stay optimization only (INV-022). Session references are
never authoritative for continuation truth (INV-019, rule CR-6):
correctness-critical continuation must survive independently of raw
chat transcript retention (INV-110).

### 9. Crash semantics

Three windows, all fail-closed:

- Crash during checkpoint: the checkpoint was never committed, so it
  does not exist. The old epoch stays ACTIVE; rollover never started;
  the next trigger re-attempts from scratch. Staging leftovers are
  ignored, never resumed as partial truth. Historical evidence stays
  retrievable throughout (INV-021).
- Crash between checkpoint commit and new-epoch activation: restart
  recovery completes activation from the committed checkpoint.
  Activation is idempotent — one checkpoint activates at most one
  successor epoch (rule CR-3) — so a repeated recovery cannot fork
  the chain.
- Crash during activation: recovery reads the activation record
  (`checkpoint_id`, optional `successor_epoch_id`, `started_at`,
  optional `completed_at`). If a successor epoch already completed
  activation, recovery is a no-op (CR-3 forbids a second); otherwise
  it completes the pending activation. Unlike the previous window,
  where activation never began, this window disambiguates through
  the record.

### 10. Concurrent execution and epoch semantics

One ACTIVE epoch per (task, execution) chain; concurrent executions
never share a chain (rule CR-9). An execution reads another chain
only through `history.search` / `history.read`. No cross-chain
writes, no shared mutable epoch state.

### 11. Provider migration resume protocol

Migration (Claude to Codex or any other pair) resumes on durable
state only, without losing authoritative state (INV-023): a new
attempt, a new provider session, and a new epoch chain continuing
from the source chain's last committed checkpoint, linked by
`MigrationLink` (`from_checkpoint`, `to_epoch`). Side
effects are never replayed across providers and authorization is
never inherited (INV-066, rule CR-7): continuity restores context
and evidence, never permission for blind repetition.

### 12. Version compatibility and supersession

Readers refuse an unknown checkpoint major version (fail-closed,
rule CR-10); minor versions must stay backward compatible.
Supersession runs through `parent_checkpoint`: superseded
checkpoints stay readable — supersession never deletes. Retention
and cleanup durations are owned by privacy/storage policy (values
open); enforcement mechanics are TASK-0171's.

### 13. Privacy bounds

SECRET material is never persisted in checkpoints, notes or history:
durable continuity state stores only opaque secret references and
scoped bindings (INV-151, INV-008). PII and raw tool payloads travel
by reference only and are never embedded (continuity.md section 6,
Privacy Architecture). Every checkpoint carries its sensitivity
ceiling as a label describing the highest level referenced — never
as authority to carry SECRET values; resumption re-applies the
ADR-0042 security filter before anything re-enters model context.
Retention durations stay open.

### Executable rules

- **CR-1** No ACTIVE epoch is discarded without a committed
  checkpoint (INV-020).
- **CR-2** Rollover reads only committed checkpoints; staging is
  never readable as committed.
- **CR-3** Activation is idempotent: one checkpoint activates at
  most one successor epoch.
- **CR-4** `history.read` requires an exact ref; no bulk session
  dump by default.
- **CR-5** Checkpoints embed no raw evidence; the section 6 inline
  bound is authored observation text only. Summaries point to
  reopenable refs.
- **CR-6** Provider session refs are never authoritative for
  continuation truth (INV-019).
- **CR-7** Migration never replays side effects and never inherits
  authorization (INV-066).
- **CR-8** Checkpoints, notes and history carry no SECRET values
  (opaque refs and scoped bindings only, INV-151, INV-008); PII and
  raw payloads travel by reference only (continuity.md section 6,
  Privacy Architecture); the ceiling is enforced as a label.
- **CR-9** Concurrent executions never share an epoch chain.
- **CR-10** Unknown checkpoint major versions refuse (fail-closed).

### C5 receipt consumption

ADR-0042 `CompactionReceipt` items are consumed by Continuity as
history references for rollover and old-evidence lookup. A receipt
is the record of a selection decision, never a checkpoint: it
cannot satisfy CR-1 and cannot seed activation under CR-3.

### Dogfood detachment per capability

- Epochs + checkpoints + rollover gating: testable on one execution
  without history search, migration or TAKEOVER.
- History search/read: testable against indexed epochs without
  migration.
- Migration resume: testable as checkpoint handoff without a second
  real provider (fixture sessions).
- Nothing here requires the entity graph, Memory truth or TAKEOVER.

### Resolved vs still open

Resolved from section 8: epoch ID format and chain scope, checkpoint
identity and schema, checkpoint transaction and verification,
threshold source and Budget Monitor ownership with trigger/gating
semantics, working-set definition, working-notes representation,
history reference format with search/read contract and ranking
disclosure, session linkage, all three crash windows, concurrent
semantics, migration protocol, version compatibility and
supersession, privacy bounds.

Still open (explicitly not decided here): checkpoint persistence
format (exact tables are TASK-0171's); history index/storage
implementation; ranking weight tuning; retention and cleanup
durations (values owned by privacy/storage policy; enforcement
mechanics are TASK-0171's); Budget Monitor, commit and
history.search/history.read call mechanics (exact function shapes
are TASK-0171's); Memory truth shapes (owning gate).

## Alternatives considered

- Epoch chains under the bare task: rejected; concurrent executions
  would share mutable epoch state, against CR-9.
- Checkpoint as full transcript: rejected; it makes chat history
  authoritative (INV-110), explodes size, and drags SECRET/PII into
  durable state against CR-8.
- Provider session as continuity root: rejected; provider-native
  state is optimization, never durability (INV-019, INV-022).
- Automatic promotion of working notes to Memory: rejected; the
  section 6 boundary requires the owning subsystem and its process.
- Silent overflow past the hard limit: rejected; new cycles block
  until rollover completes, so an unbounded window can never pretend
  it was managed.

## Consequences

- TASK-0171 transcribes this ADR for epochs, checkpoints, history
  and migration. The activation and crash semantics may inform the
  TASK-0169 amendment consumed by TASK-0172; TAKEOVER authority stays
  TASK-0169's, and no dependency change follows. Any deviation needs
  an ADR amendment, not a queue-entry restatement (the TASK-0097
  lesson).
- `continuity.md` sections 2, 4, 5, 6 and 8 gain "Proposed by
  ADR-0043 (pending ratification)" annotations, flipped to "Decided
  by" on acceptance together with a rewrite of the stale
  pre-ratification bodies in sections 2, 5 and 6; the section 8
  list stays as the record with proposed items marked.
- No schema, IPC or dependency change in this task: this ADR is
  contract only. New tables are expected in TASK-0171.

## Security impact

The sensitivity ceiling is enforced at resumption through the
ADR-0042 filter, so a checkpoint cannot smuggle SECRET material
into a new window. Migration inherits no authorization and replays
no side effects (CR-7). Session references stay untrusted evidence
(CR-6). Crash windows fail closed toward the old ACTIVE epoch or an
idempotent completion — never toward a forked chain or a half-read
staging area.

## Migration and replacement impact

History index engines (FTS/BM25, optional embeddings) stay
replaceable behind ports; this ADR pins vocabulary and recovery
semantics, not implementations. Existing evidence (finder spans,
plan revisions, journal facts) is referenced, never copied into
checkpoints. No backfill: no durable continuity state exists yet.

## References

- `docs/architecture/continuity.md` sections 2, 4, 5, 6, 8
- `docs/architecture/context-engine.md` (sibling boundary; selection
  vs survival stay separate)
- ADR-0011 (provider-independent continuity), ADR-0042 (selection
  vocabulary, budgets, trace, security filter), ADR-0029
  (identity-only evidence refs)
- `docs/architecture/INVARIANTS.md`: INV-008, INV-019, INV-020,
  INV-021, INV-022, INV-023, INV-066, INV-110, INV-151 (each cited
  invariant is invoked in the body above)
- `docs/security/DATA_CLASSIFICATION.md` (sensitivity levels)
- `planning/roadmap.yaml` G9 (epochs, checkpoints, history search)