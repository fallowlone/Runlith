# ADR-0031 - G8 Side-Effect Execution Protocol

- Status: **PROPOSED** (TASK-0122; acceptance requires an owner decision and
  independent contract review before TASK-0126 implements against it)
- Date: 2026-09-21
- Gate: G8
- Independent of: ADR-0030 (claim verification). Its only touchpoint is
  downstream: reconciliation evidence later feeds `ObservedEffect`
  provenance.

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

Binding constraints (never weakened below): INV-001 (state survives
restart), INV-007 (destructive/privileged actions pass Policy), INV-008 /
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
  -> ESCALATE       reconciliation cannot prove either within its bounded
                    budget, or the target contradicts both
ESCALATE            typed durable blocker; enters the existing bounded
                    human-decision lane where one is in scope, else a
                    typed operator-visible blocker. Never auto-retried.
```

No other transition exists. In particular: there is no `AMBIGUOUS ->
FAILED`, no `EXECUTING -> PREPARED`, and no re-entry into `EXECUTING`
except from `SAFE_TO_RETRY` as a retry of the **same** intent (SE-4).
`FAILED` is terminal for the intent; a logically new attempt mints a new
intent. Terminal states are `CONFIRMED`, `FAILED`, and an `ESCALATE`d
intent resolved through its human/operator lane.


### 3. The durable record

```text
SideEffectIntent
  intent_id           // unique, product-minted
  operation_kind      // typed registry, §4 (e.g. GIT_BRANCH_CREATE,
                      // WORKTREE_MATERIALIZE, WORKSPACE_CLEANUP)
  target_ref          // bounded identity of the external target
  params_hash         // SHA-256 over canonical operation parameters
  idempotency_key     // product-minted per intent; re-presented on every
                      // retry of the SAME intent (SE-4)
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
(TASK-0126's dogfood surface): `GIT_BRANCH_CREATE`,
`WORKTREE_MATERIALIZE`, `WORKSPACE_CLEANUP`. Kinds are extended only by
ADR amendment; an unknown kind fails closed and can never enter
`EXECUTING`.

Each kind declares its reconciliation authority — the trusted component
allowed to read actual target state:

- filesystem and Git targets reconcile **locally** through the existing
  workspace/Git ports (read-only inspections, as ADR-0023/ADR-0025
  already practice);
- remote/provider targets reconcile through provider-specific read-back
  or receipt mechanisms **behind ports**, never through provider
  self-report parsed as authority (INV-012; provider output is evidence
  input, not proof);
- a target whose actual state cannot be read within the bounded
  reconciliation budget goes to `ESCALATE`, never to a guess (SE-6).

### 5. Protocol rules (executable-as-specified)

The trusted side-effect boundary applies every rule. Rule IDs are stable
and MUST appear verbatim in implementing test names (TASK-0126; the
ADR-0027 §3 pattern).

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
  `CONFIRMED` or `SAFE_TO_RETRY` from actual target state. Reconciliation
  evidence is durable and referenced from the transition
  (`outcome_evidence_refs`).
- **SE-4 IDEMPOTENCY IDENTITY** — every retry of a `SAFE_TO_RETRY` intent
  re-presents the exact `idempotency_key` and `params_hash`. A replay of
  the same accepted effect is distinguished from a conflicting attempt by
  durable provenance (the ADR-0028 materialization retry-equivalence
  pattern); a matching key with a different `params_hash` or owner is a
  conflict, never a silent reuse. Externally retryable mutations expose
  this idempotency explicitly; idempotency does not replace transactional
  invariants (SoT cross-cutting note).
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
  the three §4 kinds, mapping the existing ADR-0023/ADR-0025 workspace/Git
  reconciliations onto the protocol; every SE rule ID MUST appear in test
  names, including crash-injected AMBIGUOUS recovery tests that prove no
  duplicate external mutation and no silent retry.
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
local paths; they stay local-first (INV-011/INV-014) and follow the
existing redaction discipline for logs and audit payloads. Reconciliation
is read-only against targets; the only mutations it authorizes are the
protocol's own durable transitions.

## Migration / replacement impact

New durable storage only; no existing table, event, or journal fact
changes meaning. Existing reconciliation behavior keeps working while
TASK-0126 maps it onto the protocol seam by seam; mixed old/new states
must be distinguishable by provenance and fail closed, per the ADR-0028
migration posture. No G6/G7 artifact is migrated or reinterpreted.

## Open

- Provider read-back/receipt port shapes for remote kinds (G11/G12).
- Whether daemon restart may auto-enter `RECONCILE` for recorded
  `AMBIGUOUS` intents (read-only target inspection) without an explicit
  operator resume, or whether ADR-0025's surface-only rule covers it;
  decided with the G11 consumption contract, not assumed here.
- Reconciliation budgets per kind (policy-owned tunables, SE-8).

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
