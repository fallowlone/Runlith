# ADR-0031 - G8 Side-Effect Execution Protocol

- Status: **PROPOSED** (TASK-0122; acceptance requires an owner decision and
  independent contract review before TASK-0126 implements against it)
- Date: 2026-09-21
- Gate: G8
- Independent of: ADR-0030 (claim verification — a parallel PROPOSED
  contract on branch `task-0121-claim-verification-contract`; the only
  touchpoint is downstream: reconciliation evidence may later feed
  `ObservedEffect` provenance). Neither depends on the other's
  acceptance; `adr/INDEX.md` in this branch registers ADR-0030 when that
  branch merges first (merge order: 0121, then this branch rebased).

> Contract-only ADR (TASK-0122). It freezes the durable side-effect
> intent/recovery protocol — the state machine, the `SideEffectIntent`
> record, idempotency identity, and the rules SE-1..SE-8 — before any
> implementation. No domain code, no persistence, no provider calls are
> authorized here; TASK-0126 implements the workspace/Git dogfood consumer
> against this contract.

## Context

Local transaction state cannot prove what happened in an external system
after a crash. Runlith already lives this: SQLite commits and
Git/filesystem mutations are separate failure domains (INV-145), never one
ACID transaction, and every existing seam that crosses the boundary
carries hand-built reconciliation: ADR-0023's workspace materialization
postconditions and idempotent cleanup, ADR-0025's explicit in-place
resume with its "concluding an open attempt without provider evidence
would fabricate a terminal fact" rule, and ADR-0028's
distinguish-replay-from-conflict materialization fencing. Each seam
re-derived the same logic locally, and the failure mode that matters —
*the effect may or may not have happened* — has no shared name.

The Source of Truth (§105.2) names the missing contract and assigns it to
G8 Verification/recovery, with G11 daemon reconciliation consuming the
same semantics for unattended execution:

```text
PREPARED
  -> INTENT_RECORDED
  -> EXECUTING
       -> CONFIRMED
       -> FAILED
       -> AMBIGUOUS
            -> RECONCILE
                 -> CONFIRMED | SAFE_TO_RETRY | ESCALATE
```

The SoT also fixes two non-negotiables: `AMBIGUOUS` means the external
outcome is unknown and is not equivalent to `FAILED` or `NOT_EXECUTED`;
and Runlith must reconcile the actual target state before retrying any
side effect that could already have completed. The roadmap's
EVENT_SOURCING guardrail stands: durable relational state plus
append-oriented journals and explicit reconciliation are sufficient; this
protocol is not event sourcing.

Binding constraints (never weakened below): INV-001 (Task state survives
every agent/session/process restart; §3 carries the analogous demand for
intent state), INV-007 (destructive/privileged actions pass Policy), INV-008 /
INV-151 (no secrets in durable state), INV-010 (persistent mutations are
auditable), INV-012 (provider neutrality), INV-050 / INV-091 (no silent
unbounded retry), INV-066 (no transparent side-effect replay across
failover), INV-076 (required isolation fails closed), INV-145 (separate
failure domains, reconciled).

## Decision

### 1. What is a side effect under this protocol

An operation is a **protocol side effect** when it mutates state outside
the committing SQLite transaction and a crash between the external
mutation and the local commit can leave the two disagreeing: Git branch
and worktree creation, workspace cleanup/deletion, provider calls with
external effect, and — in later gates — remote publication. Pure SQLite
transactions are already atomic and are out of scope. Read-only
observations are out of scope. Admission transactions that touch only
SQLite (e.g. ADR-0024 §6 execution entry) are out of scope by
construction; their *preparation* and *finalization* halves, which cross
the boundary, are the protocol's natural consumers.

The frozen v1 registry (§4) is a dogfood slice, not a completeness claim.
Named consumers deferred to later work, recorded here so the gap is
visible rather than accidental: attempt conclusion and post-attempt
crash/orphan recovery stay under ADR-0025's explicit G11 deferral (this
protocol is the vocabulary that deferral will consume), and remote
publication stays with the SoT publish-pending direction (G11/G12).

**Applicability is by registration.** The rules below bind operation
kinds registered for them at the time of the operation (§4). Side-effect
classes not yet registered — today, the G5 provider execution seam and
its attempt lifecycle (ADR-0024/ADR-0025, whose recovery semantics are
their own accepted contracts), and later remote publication and daemon
reconciliation — are **deferred consumers of this vocabulary, not
standing violations of it**. Registering a class maps it onto the §2
machine by amendment or a scoped consumer contract; until then its
existing recovery semantics govern.

### 2. The state machine (normative)

States and transitions are exactly:

```text
PREPARED            intent validated, nothing durable yet, no effect
  -> INTENT_RECORDED
INTENT_RECORDED     durable intent + idempotency identity committed
  -> EXECUTING
EXECUTING           external mutation dispatched, outcome unknown
  -> CONFIRMED      durable evidence the effect completed as intended
  -> FAILED         durable evidence the effect did not happen (or was
                    fully rejected) and the target is provably unchanged
  -> AMBIGUOUS      dispatch happened; outcome unknown (crash, timeout,
                    disconnect, unreadable target)
AMBIGUOUS
  -> RECONCILE
RECONCILE           trusted reconciliation inspects actual target state
  -> CONFIRMED      reconciliation proves the effect landed
  -> SAFE_TO_RETRY  reconciliation proves the effect did not land and the
                    target matches the pre-effect state
  -> SAFE_TO_CONTINUE  reconciliation proves the effect partially landed
                    in a state from which a named, deterministic,
                    previously-proven continuation operation completes
                    it without discretionary choices
  -> ESCALATE       reconciliation cannot prove any of the above within
                    its bounded budget, or the target contradicts every
                    known continuation
SAFE_TO_RETRY       explicit re-dispatch of the same operation (same
                    intent, same idempotency identity)
  -> EXECUTING
SAFE_TO_CONTINUE    explicit dispatch of the named continuation
                    operation (same intent, same idempotency identity)
  -> EXECUTING
ESCALATE            typed durable blocker; enters the existing bounded
                    human-decision lane where one is in scope, else a
                    typed operator-visible blocker. Terminal for
                    autonomous transitions (below). Never auto-retried.
```

Two re-entry edges exist, both explicit: `SAFE_TO_RETRY -> EXECUTING`
re-dispatches the original operation; `SAFE_TO_CONTINUE -> EXECUTING`
dispatches the intent kind's named continuation operation (for
`WORKTREE_MATERIALIZE`, the ADR-0025 TASK-0057 branch-only attachment —
the fused `worktree add -b` may have created the managed branch before
crashing, and the proven continuation attaches rather than re-creates,
which a blind retry would refuse with a branch collision). Both edges
are entered only on an explicit, auditable control-plane instruction;
automatic retry is not authorized by this contract (its authorization
question is deferred to the G11 consumption contract, Open).

`SAFE_TO_CONTINUE` is a deliberate G8 refinement of the SoT §105.2
outcome list (`CONFIRMED | SAFE_TO_RETRY | ESCALATE`), recorded here
with rationale: the SoT non-negotiable — reconcile the actual target
state before retrying any side effect that could already have completed
— is honored, because a continuation dispatch happens only after
reconciliation proves the exact partial state and names the proven
completion. Forcing the branch-only recovery shape through `ESCALATE`
would convert ADR-0025's proven automatic (post-explicit-resume)
recovery into a mandatory human escalation — changing externally proven
behavior this ADR commits to carrying.

`ESCALATE` is terminal for autonomous transitions: no rule may leave it
autonomously. Resolution is an explicit, durable, product-authored
**intent-resolution record** made by the operator/human lane,
referencing the intent and its reconciliation evidence. A resolution
either (i) closes the intent as resolved-discarded (the resolution
record is its terminal evidence), or (ii) authorizes a fresh intent
that carries the original `idempotency_key` forward (linkage
preserved), dispatched only on explicit operator instruction. The
resolution decision itself is audited (INV-010).


### 3. The durable record

```text
SideEffectIntent
  intent_id           // unique, product-minted
  operation_kind      // typed registry, §4
  target_ref          // bounded identity of the external target
  params_hash         // SHA-256 over the kind's canonical parameter set
                      // (defined per kind in §4)
  idempotency_key     // product-minted per intent lineage; re-presented on
                      // every re-dispatch of the SAME intent and carried
                      // forward by resolution-authorized successor
                      // intents (SE-4, §2 ESCALATE resolution)
  state               // the §2 machine state
  linkage             // owning Task/Execution/Attempt refs (provenance)
  pre_state_fingerprint   // canonical pre-effect target reading, when the
                      // kind supports one (else recorded as unsupported)
  outcome_evidence_refs   // durable evidence behind the terminal or
                      // RECONCILE transition (SE-5)
  policy_fingerprint
  created_at / updated_at
```

The intent row and its state transitions are durable, auditable mutations
(INV-010); transitions append journal/audit evidence per the existing
boundaries and never edit history. The record carries no secrets and no
provider session identity (INV-008, INV-151, INV-012).

### 4. Operation-kind registry and reconciliation authority

v1 freezes the kind registry to the workspace/Git failure domain
(TASK-0126's dogfood surface), with one kind per durable intent and the
kind defining the intent's operation **and** its named continuation:

| Kind | Port operation | Canonical parameters (`params_hash` input) | Retry / continuation behavior |
|---|---|---|---|
| `WORKTREE_MATERIALIZE` | fused `create_worktree` (`git worktree add -b`): one dispatch creates managed branch + worktree | (managed branch name, target path, exact base revision, owning lease/execution identity) | retry re-dispatches the same fused operation after `SAFE_TO_RETRY`; after `SAFE_TO_CONTINUE` dispatches `attach_worktree` (the TASK-0057 branch-only attachment), which a blind retry could never reach |
| `WORKSPACE_CLEANUP` | `remove_worktree` (with attached-branch deletion per the ADR-0023 cleanup contract) — one intent covering the whole cleanup; its named continuation is the idempotent catch-up of the remaining half (crash after worktree removal but before branch deletion → re-prove ownership, then `remove_branch`; crash before any removal → full re-dispatch) | (worktree path, owning lease/execution identity, branch deletion scope) | retry re-dispatches after `SAFE_TO_RETRY`; `SAFE_TO_CONTINUE` dispatches the named catch-up half (ADR-0023's "crash implies inspection, then the next safe action"); idempotent already-absent outcomes are `CONFIRMED` with the observed evidence, per ADR-0023's idempotent-cleanup semantics — a partial cleanup therefore continues automatically (post-explicit-resume), it does not escalate |

There is no standalone `GIT_BRANCH_CREATE` kind: in v1, managed-branch
creation is always fused into `WORKTREE_MATERIALIZE` (one Git-native
dispatch), and branch deletion is part of `WORKSPACE_CLEANUP`. A future
unfused branch operation requires an ADR amendment adding its kind,
operations and canonical parameters. Kinds are extended only by ADR
amendment; an unknown kind fails closed and can never enter
`EXECUTING`.

Each kind declares its reconciliation authority — the trusted component
allowed to read actual target state:

- filesystem and Git targets reconcile **locally** through the existing
  workspace/Git ports (read-only inspections, as ADR-0023/ADR-0025
  already practice);
- remote/provider targets reconcile through provider-specific read-back
  or receipt mechanisms **behind ports**, never through provider
  self-report parsed as authority; provider output is evidence input,
  never proof — a boundary owned by SoT §105.1 and ADR-0018, not by
  INV-012, which here governs provider neutrality of the record itself);
- a target whose actual state cannot be read within the bounded
  reconciliation budget goes to `ESCALATE`, never to a guess (SE-6).

### 5. Protocol rules (executable-as-specified)

The trusted side-effect boundary applies every rule **to operation kinds
registered for them at operation time (§4)**; unregistered side-effect
classes are deferred consumers of this vocabulary, not standing
violations of it (§1). Rule IDs are stable
and MUST appear verbatim in implementing test names (TASK-0126; the
ADR-0027 §3 pattern). Dispatch of every kind additionally obeys the
standing isolation rule: where a kind requires sandboxed or otherwise
restricted execution, dispatch fails closed when the required backend is
unavailable — the intent records `FAILED` or `ESCALATE`, never a silent
downgrade to an unconstrained execution (INV-076).

- **SE-1 INTENT BEFORE EFFECT** — no external mutation is dispatched
  before `INTENT_RECORDED` is durable. An intent that cannot be persisted
  is never executed; an effect without a prior durable intent is a
  contract violation, surfaced as such.
- **SE-2 AMBIGUOUS IS NOT FAILED** — a dispatched effect whose outcome is
  unknown (crash, timeout, disconnect, unreadable target) records
  `AMBIGUOUS`. It may not be recorded `FAILED` (that asserts the target
  is unchanged, which requires proof) nor treated as never-executed.
  Roll-ups and reports preserve the distinction (INV-131/INV-136
  analogue).
- **SE-3 RECONCILE BEFORE RETRY** — an `AMBIGUOUS` intent is never
  retried, re-dispatched, or "cleaned up" until `RECONCILE` proves
  `CONFIRMED`, `SAFE_TO_RETRY`, or `SAFE_TO_CONTINUE` from actual target
  state. Reconciliation evidence is durable and referenced from the
  transition (`outcome_evidence_refs`).
- **SE-4 IDEMPOTENCY IDENTITY** — every re-dispatch of a
  `SAFE_TO_RETRY` intent (of the original operation) or a
  `SAFE_TO_CONTINUE` intent (of its named continuation operation)
  re-presents the exact `idempotency_key` and `params_hash`. A replay of
  the same accepted effect is distinguished from a conflicting attempt by
  durable provenance (the ADR-0028 materialization retry-equivalence
  pattern); a matching key with a different `params_hash` or owner is a
  conflict, never a silent reuse. Externally retryable mutations expose
  this idempotency explicitly; idempotency does not replace transactional
  invariants (SoT cross-cutting note). A resolution-authorized successor
  intent (§2 ESCALATE resolution) carries the original `idempotency_key`
  forward with linkage to its intent-resolution record: identical key,
  `params_hash`, owner and resolution provenance remain a distinguishable
  replay, not a conflict.
- **SE-5 NO FABRICATED TERMINAL FACTS** — `CONFIRMED` and `FAILED` are
  recorded only against durable evidence of the actual outcome
  (generalizing ADR-0025's no-terminal-fact-without-evidence rule).
  Absence of evidence yields `AMBIGUOUS`, never an assumption in either
  direction.
- **SE-6 RECONCILIATION READS THE TARGET, NOT THE PLAN** — reconciliation
  decides from observed target state through the §4 authority for that
  kind, never from what the intent expected, what a log says happened, or
  what a provider claims. Local-first: filesystem/Git reconcile locally;
  unknown or unreadable target classes `ESCALATE`.
- **SE-7 NO CROSS-DOMAIN ACID PRETENSE** — SQLite state and
  Git/filesystem/provider effects remain separate failure domains
  (INV-145). This protocol is the reconciliation contract between them;
  nothing here claims, simulates, or fakes a cross-domain transaction.
- **SE-8 ESCALATION IS TYPED, BOUNDED, AND AUDITED** — `ESCALATE` produces
  a durable typed blocker (and a `HumanDecisionRequest` where the G7 lane
  is in scope) with the intent identity and reconciliation evidence
  attached. Reconciliation and retry loops are bounded by explicit
  budgets; exhaustion escalates, never loops silently (INV-050, INV-091).


### 6. Relationship to existing contracts

- **ADR-0023 / ADR-0025 semantics are carried, not replaced.** Workspace
  materialization postconditions, idempotent cleanup of owned paths, the
  explicit in-place resume, and the rule that startup surfaces strands
  read-only and never auto-dispatches effects all stand. This protocol
  gives their hand-built reconciliations one shared vocabulary and one
  durable record; TASK-0126 maps the existing seams onto it without
  changing their externally proven behavior.
- **G11 consumes, G8 defines.** Unattended daemon reconciliation (drains,
  restarts resuming effects without an operator) is G11 scope over this
  same contract; nothing here authorizes automatic post-restart effect
  dispatch (ADR-0025 §2 stands: explicit, auditable resume).
- **G7 planning is untouched.** Planning fences, HumanDecision lanes, and
  spend accounting are unaffected; `ESCALATE` merely reuses the existing
  bounded handoff machinery where it is already in scope.
- **ADR-0030 is independent.** Reconciliation evidence may later serve as
  `ObservedEffect` provenance; neither contract depends on the other.

## Alternatives considered

- **Full Event Sourcing**: rejected — the roadmap guardrail keeps it a
  post-v1 selective direction; durable relational intent rows plus
  append-oriented evidence are sufficient (SoT §105.2).
- **Cross-domain transaction managers / two-phase commit over Git+SQLite**:
  rejected — INV-145 forbids representing the two as one ACID domain, and
  Git offers no prepare phase; reconciliation is the honest primitive.
- **Retry with backoff and no intent record**: rejected — that is the
  unbounded, unauditable loop INV-050/INV-091 forbid, and it cannot
  distinguish "didn't happen" from "happened twice".
- **Freeze provider read-back port shapes here**: rejected — no v1
  consumer exists (the v1 registry is local-only); inventing the remote
  surface now would preempt G11/G12 evidence, the ADR-0024 pattern of
  freezing only proven seams applied in reverse.

## Consequences

- TASK-0126 implements the §3 record, the §2 machine, and the §5 rules for
  the §4 kinds, mapping the existing ADR-0023/ADR-0025 workspace/Git
  reconciliations onto the protocol; every SE rule ID MUST appear in test
  names, including crash-injected AMBIGUOUS recovery tests that prove no
  duplicate external mutation and no silent retry — including the
  branch-only shape, whose `SAFE_TO_CONTINUE` attachment must be proven
  reachable and exactly-once.
- Durable intent storage is owned by the side-effect boundary and creates
  no second authority for Task, Execution, Workspace, or provider state
  (the ADR-0027 storage-ownership pattern).
- Later gates extend the kind registry by amendment: remote publication
  (the SoT publish-pending note) and G11 daemon reconciliation are the
  named successors.

## Security and privacy impact

Destructive kinds pass Policy before `EXECUTING` as they do today
(INV-007); the protocol adds no new authority, only new evidence. Intent
rows carry no secrets, credentials, or provider session material
(INV-008, INV-151). `target_ref` and `pre_state_fingerprint` may contain
local paths; the protocol introduces no new egress — nothing here is
sent to a cloud service or telemetry channel (INV-011/INV-014 remain
satisfied by construction) — and captured material follows the existing
redaction discipline for logs and audit payloads. Reconciliation
is read-only against targets; the only mutations it authorizes are the
protocol's own durable transitions.

## Migration / replacement impact

New durable storage only; no existing table, event, or journal fact
changes meaning. Existing reconciliation behavior keeps working while
TASK-0126 maps it onto the protocol seam by seam; mixed old/new states
must be distinguishable by provenance and fail closed, per the ADR-0028
migration posture. No G6/G7 artifact is migrated or reinterpreted.

## Open

- Provider read-back/receipt port shapes for remote kinds (G11/G12);
  provider-output-as-evidence (never proof) follows SoT §105.1 and
  ADR-0018, which govern that boundary.
- Whether daemon restart may auto-enter `RECONCILE` for recorded
  `AMBIGUOUS` intents (read-only target inspection) without an explicit
  operator resume, or whether ADR-0025's surface-only rule covers it;
  decided with the G11 consumption contract, not assumed here. Automatic
  *retry* (as opposed to read-only reconciliation) is decided by the same
  contract and is not authorized by this ADR (§2).
- Reconciliation budgets per kind (policy-owned tunables, SE-8).
- Intent durability is this ADR's own normative requirement (§3): intent
  state must survive every restart; INV-001 names the analogous Task
  property.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §105.2 (this state
  machine and its non-negotiables), cross-cutting idempotency note,
  G8/G11 ownership split, EVENT_SOURCING guardrail.
- `docs/architecture/adr/ADR-0023-workspace-ownership-and-git-worktree-lifecycle.md`
  — workspace materialization/cleanup reconciliation this protocol names.
- `docs/architecture/adr/ADR-0025-g5-post-admission-recovery.md` —
  explicit-resume and no-fabricated-terminal-fact semantics carried.
- `docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md`
  — replay-vs-conflict provenance pattern SE-4 reuses.
- `docs/architecture/INVARIANTS.md` — INV-001, INV-007, INV-008, INV-010,
  INV-012, INV-050, INV-066, INV-076, INV-091, INV-145, INV-151.
- `planning/roadmap.yaml` G8; `planning/tasks/queue.yaml` TASK-0122,
  TASK-0126.
