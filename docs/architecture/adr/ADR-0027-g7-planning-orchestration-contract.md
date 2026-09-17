# ADR-0027 - G7 Planning and Orchestration Contract

Status: Accepted
Date: 2026-09-14

> Contract-only ADR (TASK-0075). It freezes the G7 planning seam —
> `PlannerPort`, `PlanChangeSet`, validation rules, `PlanRevision`
> provenance, liveness guardrails, planner budget envelope, vocabulary
> reconcile, and G6 follow-up re-homing — before any implementation.
> No domain code, no persistence, no provider calls are authorized here;
> TASK-0076..0080 implement against this contract.

## Context

ADR-0017 decided that adaptive planning is proposal-driven and revisioned,
but explicitly left open the `PlannerPort` API, the `PlanChangeSet`
schema, the `PlanRevision` storage shape, exact decomposition limits, and
the planner selection mechanics. G7 (roadmap: multi-agent DAG + bounded
adaptive planning; queue TASK-0075..0081) must now close those openings
with an executable contract a TASK-0076 implementer can transcribe to
tests without guessing.

Two additional pressures force this ADR now rather than later:

1. **Stale placements.** The Source of Truth staleness note (2026-09-14,
   §105) and the roadmap G6 note both state that former "G6 follow-up"
   items (explicit execution modes + immutable `ActionProposal`,
   task-scoped `CapabilityLease`) predate G6 closure (TASK-0074,
   `G6_CLOSED`). G6 must not be reopened; those items need explicit G7
   homes or explicit deferral.
2. **Vocabulary overlap.** The §105 research vocabulary (`ActionProposal`,
   `HandoffPacket`/`ResumeToken`, `CapabilityLease`, `ContextItem`
   envelope) overlaps the already-accepted future vocabulary
   (`HumanDecisionRequest`, Council Engine, Playbooks/Procedures,
   `AgentMessage`). Implementing either side before the reconcile would
   create parallel competing concepts.

Binding constraints (never weakened below): INV-043 (proposal-only
planner), INV-044 (revision provenance), INV-045 (bounded recursion),
INV-046 (no weakening via decomposition), INV-047 (lifecycle vs
readiness), INV-050 (termination), INV-002/INV-009 (identity and
completion authority), INV-146…153 (G6 security authority, exact-bound
approval, audit separation), INV-074/INV-075/INV-083 (procedure
preference and boundaries), INV-091 (bounded retries).

## Decision

### 1. `PlannerPort` is the proposal-only planning seam

The planner lives behind one replaceable port. It proposes only;
validated application services mutate the graph (INV-043). Planner
output is untrusted data until validated (INV-093 analogue: model
output never carries authority).

```rust
trait PlannerPort {
    fn assess_atomicity(&self, input: AtomicityRequest)
        -> Result<AtomicityAssessment, PlannerFault>;
    fn propose_decomposition(&self, input: DecompositionRequest)
        -> Result<PlanChangeSet, PlannerFault>;
    fn propose_replan(&self, input: ReplanRequest)
        -> Result<PlanChangeSet, PlannerFault>;
}
```

Typed inputs and outputs (field-level schema owned by TASK-0076; the
names and semantics below are normative and stable):

- `AtomicityRequest { task_ref, acceptance_criteria_refs,
  scope_bound, evidence_refs, execution_policy_ref }` — everything the
  planner may read; no graph handles, no policy handles.
- `AtomicityAssessment { verdict, rationale, evidence_refs }` where
  `verdict ∈ { ATOMIC, DECOMPOSABLE, BLOCKED }`; `BLOCKED` carries a
  typed blocker. Assessment is pure: it performs zero durable writes
  (a test calling it twice must observe zero new journal rows).
- `DecompositionRequest { parent_task_ref, base_plan_revision,
  acceptance_criteria_refs, scope_bound, evidence_refs,
  depth_remaining, budget_remaining }` — the planner receives its
  remaining depth/budget headroom so it can refuse before an LLM call
  instead of proposing into a certain rejection.
- `ReplanRequest { branch_root_ref, base_plan_revision,
  triggering_evidence_refs, scope_bound, depth_remaining,
  budget_remaining }` — replan addresses one affected branch only;
  regenerating the whole project plan is forbidden input (ADR-0017:
  revise the affected branch).
- `PlanChangeSet` — §2 below. Returned changesets carry no authority.
- `PlannerFault ∈ { UNPLANNABLE { blocker }, BUDGET_EXHAUSTED,
  DEPTH_EXHAUSTED, PROVIDER_FAILURE { typed_cause } }` — a planner
  that cannot produce a valid smaller decomposition reports a fault;
  the orchestrator then BLOCKs or escalates. It never retries silently
  (INV-050, INV-091).

`assess_atomicity` / `propose_decomposition` / `propose_replan` map
1:1 onto the Source of Truth §21.1.1 seam; no fourth method is added
in v1. Routing, capability snapshots, and model selection stay outside
this port (future Router; INV-059).

Composition-root rule (ADR-0024 pattern): only the `agentd`
composition root instantiates concrete `PlannerPort` adapters. No
domain, scheduler, storage, workspace, or IPC crate depends on a
planner adapter crate. One injected planner per orchestration cycle;
no planner comparison, no silent fallback retry on the seam.

### 2. `PlanChangeSet` required fields

Every proposal MUST carry all of the following; a missing field is a
malformed proposal and is rejected before validation:

```text
base_plan_revision      // branch head the proposal was built against
reason                  // non-empty; why this change exists
source_evidence_refs    // typed refs: execution/attempt/test/human-decision ids
node_additions          // new nodes with scope + acceptance-criteria refs
node_updates            // (target id, field deltas)
node_removals           // target ids
edge_additions / edge_removals
expected_outcome        // what executable work this unlocks
risk_delta              // per-node risk-class transitions, possibly empty
budget_delta            // planning + expected execution cost movement
scope_delta             // must be ⊆ scope_bound (see V4)
coverage_map            // parent_criterion_id -> [child_criterion_ids] (see V3)
proposal_hash           // SHA-256 over canonical changeset bytes, computed by
                        // the proposer; any material mutation changes the hash
proposed_by             // planner identity (adapter id + model ref)
planning_cost_receipt   // tokens in/out, wall time, price class (see §6)
```

`proposal_hash` extends the TASK-0062 exact-binding principle into
planning: EXECUTE authorization binds to the exact hash (§7, F1), and
any material proposal mutation invalidates prior authorization.

### 3. Validation rules (executable-as-specified)

The trusted application service applies every rule, in order, to each
proposal. First failing rule refuses the proposal with its typed code;
no partial application ever persists. Rule IDs are stable and MUST
appear verbatim in TASK-0076 test names.

- **V1 ACYCLICITY** — merged graph = base-revision graph + deltas MUST
  be a DAG under a deterministic topological check. Violation →
  `CYCLIC_PROPOSAL`.
- **V2 REFERENTIAL INTEGRITY** — every edge endpoint MUST resolve to
  (base node ∪ added node) ∖ removed nodes; every update/remove
  target MUST exist in the base revision; every removed node MUST have
  no surviving inbound/outbound edge outside the removal set unless
  that edge is also removed in the same changeset. Violation →
  `DANGLING_REF`.
- **V3 ACCEPTANCE-CRITERIA PRESERVATION** — the `coverage_map` MUST
  cover every affected parent criterion with ≥1 child criterion; every
  covering child MUST declare `verification_method ∈
  { automated-test, independent-review, security-check,
  human-approval }`; a covering child whose method differs from the
  parent criterion's method flags risk escalation and routes to V9
  (no silent weakening of how a criterion is proven). Dropped or
  uncovered criterion → `CRITERION_DROPPED`. (INV-046; G8 owns the
  verification pipeline itself — this rule only preserves the proof
  obligation, it does not design G8.)
- **V4 SCOPE CONTAINMENT** — every added/updated node scope MUST be a
  subset of the branch authorized scope (`scope_delta ⊆ scope_bound`;
  exact scope syntax owned by TASK-0076, subset relation defined
  here). Violation → `SCOPE_ESCAPE`.
- **V5 NO POLICY/APPROVAL WEAKENING** — the changeset MUST NOT contain
  policy-rule, approval-binding, or gate-definition deltas (plan nodes
  may reference required gates, never redefine them); every node whose
  risk class exceeds the branch baseline MUST carry a
  `required_gate` annotation. Violation → `POLICY_WEAKENING`.
  (INV-046, INV-146, INV-148.)
- **V6 BUDGET/DEPTH/FAN-OUT LIMITS** — decomposition depth MUST be ≤
  `MAX_DECOMPOSITION_DEPTH`; new children per decomposition MUST be ≤
  `MAX_CHILDREN_PER_DECOMPOSITION`; total nodes in the affected
  branch after merge MUST be ≤ `MAX_NODES_PER_PLAN_BRANCH`; accepted
  revisions per reconcile cycle per branch MUST be ≤
  `MAX_REVISIONS_PER_CYCLE_PER_BRANCH`. Violation →
  `LIMIT_EXCEEDED { which, bound, observed }`. Values in §5.
- **V7 DUPLICATE-DECOMPOSITION REJECTION** — `fingerprint =
  SHA-256(canonical(parent_task_id, sorted child scope roots, sorted
  criterion ids, sorted evidence refs))` MUST NOT equal any
  fingerprint in the branch lineage. Violation →
  `DUPLICATE_DECOMPOSITION`. (WriteHERE failure mode:
  thinking-only expansions that never reach executable leaves.)
- **V8 STALE BASE** — `base_plan_revision` MUST equal the current
  branch head (optimistic concurrency). Violation → `STALE_BASE`;
  the planner re-reads and re-proposes. Silent auto-rebase is
  forbidden.
- **V9 RISK-ESCALATION APPROVAL GATE** — if `risk_delta` raises any
  node's risk class, or V3 flagged a verification-method change, the
  revision MUST NOT persist until a bound, unexpired, single-use
  `HumanDecisionRequest` grant referencing the exact `proposal_hash`
  exists (TASK-0079 lifecycle). Planner self-approval never satisfies
  this rule. Missing/expired/mismatched grant → `RISK_GATE_OPEN`.

Rejected proposals return typed errors to the orchestration loop,
count toward the no-progress guardrail (L3), and are surfaced as
guardrail interventions in planning observability (§21.1.7). They
append no Event Journal rows (the journal records durable change,
not refused attempts).

### 4. `PlanRevision` provenance + Event Journal coupling

Every accepted structural mutation creates exactly one versioned
`PlanRevision`:

```text
PlanRevision
  revision_seq        // u64, monotonic per plan branch
  plan_branch_id
  base_revision       // parent revision this supersedes
  changeset_hash      // SHA-256 over canonical PlanChangeSet bytes
  reason              // required, non-empty
  caused_by           // typed evidence refs (execution/attempt/test/human-decision)
  proposed_by         // planner identity + planning_cost_receipt ref
  validated_by        // reconciler/service identity + policy fingerprint
  created_at
```

Coupling rule: the revision row and its `plan.revision_committed`
journal event commit in ONE SQLite transaction (TASK-0024 atomicity
pattern). The event carries revision identity, `changeset_hash`,
`reason`, `caused_by`, `proposed_by`, `validated_by` — full node
blobs live in the plan store, never in the event (bounded event
size). No Git/filesystem mutation participates in that transaction
(INV-145: separate failure domains, reconciled — never one ACID
transaction). Scheduler/Reconciler coordinate against validated
heads only; a revision is addressable and diffable for "why was this
task created?" explanations.

### 5. Recursion/liveness guardrails (numeric bounds + rationale)

Bounds are v1 defaults owned by planning policy (mechanism/policy
split, INV-112): tunable without domain rewrites, never tunable by
the planner itself.

| Bound | Value | Rationale |
|---|---|---|
| `MAX_DECOMPOSITION_DEPTH` | 4 edges, branch root → deepest leaf | Each level must strictly narrow scope (V4); depth beyond 4 means atomicity assessment failed upstream — escalate, don't recurse. |
| `MAX_CHILDREN_PER_DECOMPOSITION` | 8 | One validated revision must stay human-reviewable (§21.1.7 observability) and fit one atomic revision transaction; more children means the parent bundled unrelated work — split the parent first. |
| `MAX_NODES_PER_PLAN_BRANCH` | 64 | Bounds the validation transaction, journal payload, and DAG rendering; larger programs split into reconciler-coordinated branches, not one recursive expansion. |
| `MAX_REVISIONS_PER_CYCLE_PER_BRANCH` | 1 accepted | Prevents multi-hop auto-rewrite loops inside one evidence cycle; each further mutation needs a fresh cycle with fresh evidence. |
| `L3` no-progress trip | 2 consecutive rejections, same branch + same triggering evidence → BLOCKED + `HumanDecisionRequest` (REPLAN/ABORT) | Structural termination per INV-050; matches the WriteHERE thinking-only-subtask failure mode. |
| `L4` revision progress | accepted revision MUST leave ≥1 RUNNABLE leaf (derived readiness, ADR-0017 §21.1.3) in the branch, or move it to BLOCKED with blocker + HDR | Guarantees every decomposition moves toward executable leaves; creating children never resolves the parent (parent completion still needs independent verification, ADR-0018). |

Enforcement points (both, always): the orchestrator verifies
depth/budget headroom BEFORE any planner call (no LLM spend into a
certain refusal), and the application service re-validates every
bound ON every proposal (planner output is untrusted). Guardrail
trips are typed (`GuardrailTrip { bound, observed }`), never silent.

### 6. Planner call budget envelope (token/time/cost)

Per `PlannerPort` call: wall-clock ≤ 120 s, input ≤ 32,000 tokens,
output ≤ 8,000 tokens. Per triggering-evidence-event per branch:
≤ 8 planner calls AND ≤ 256,000 cumulative input tokens; exhaustion
→ BLOCKED + `HumanDecisionRequest` (no silent spend).

Cost is a derived reporting field, not a third independent limit:
`planning_cost_receipt` records tokens in/out, wall time, and
provider price class per call; cumulative receipts feed the existing
Verified Delivery Cost model (roadmap guardrail: unchanged — token
count alone never proves delivery cost, since retries, verification,
and human intervention dominate). Rationale for the magnitudes:
minute-scale provider calls are already proven on the G5 production
path, so 120 s keeps the propose→validate loop interactive over
`agentctl`; ~8 full-context retries fit inside one human-attention
escalation, per the Human Attention Queue principle (escalate cost
is bounded and visible before it accrues further).

### 7. Vocabulary reconcile (zero undecided rows)

| Term | Verdict | Rationale |
|---|---|---|
| `ActionProposal` | MERGE into `PlanChangeSet` | One proposal object survives. EXPLAIN/PROPOSE/EXECUTE adopted as orchestration modes (§8); the immutable proposal IS the `PlanChangeSet` + `proposal_hash`; no second durable proposal entity. |
| `HandoffPacket` | MERGE into `HumanDecisionRequest` | HDR is the durable record; every packet field (goal/state/actions/evidence/blockers/uncertainty/question/options/recommendation) maps onto HDR fields; "HandoffPacket" survives only as the human-facing rendering name, never a second stored type. |
| `ResumeToken` | MERGE into `HumanDecisionRequest` continuation | Opaque handle bound to the HDR identity, carrying RESUME/REPLAN/TAKEOVER/ABORT + resulting revision ref; non-bearer, single-use, expiry-bound — same semantics as TASK-0062 approvals. |
| `CapabilityLease` | DEFER (post-G7) | No G7 task needs temporary delegation: Policy effective authority + Secrets Broker scoped delivery already enforce least privilege. Revisit for G12 remote workers. |
| `ContextItem` envelope | DEFER to G9 | Owner already assigned (roadmap G9). G7 planners exchange evidence refs, never envelope internals. |
| `HumanDecisionRequest` | ADOPT (G7 durable record) | TASK-0079 implements the bounded lifecycle; blocks minimal scope only; risk-escalation gate (V9) consumes it. |
| Council / CouncilEngine (quorum/veto) | DEFER (post-G7) | G7 uses one bounded planner + the existing independent-evaluation path; multi-model deliberation is unproven on our workloads (ADR-0017 evidence discipline: benchmark before default). INV-086…089 already constrain any future council. |
| Procedures / Playbooks / Routines ladder | ADOPT vocabulary, DEFER engine | Planner inputs/outputs may reference procedure identities by these canonical names; the Procedure Engine itself stays G10 per Scape placement. INV-074/INV-075/INV-083 govern. |
| `AgentMessage` | DEFER to G9 | Scape placement. G7 coordination uses typed agent-bus events (§22: TaskAssigned…TaskSubmitted) + Event Journal; no durable agent-to-agent chat state in G7. |
| `MissionSpec` / `RoleProfile` | ADOPT as opaque metadata, DEFER materialization | Accepted-future orchestration vocabulary; G7 planner I/O may carry role/mission labels as opaque strings with zero domain semantics. |
| `WorkClaim` | DEFER (post-G7) | Advisory hint only, never a lock and never a worktree-isolation substitute; no TASK-0076..0080 consumer requires it, so it waits for a proven scheduler coordination gap. |

Other §105-adjacent candidate names resolve without new concepts:
Codex `CapabilityDelta` ≡ a V9 risk-escalation request; Codex
`ApprovalGrant` ≡ the TASK-0062 grant semantics extended with
`proposal_hash` (§8); `ContinuationCheckpoint`/`ExecutionHandoff`
belong to G9 continuity, not G7 planning.

### 8. Re-homing of former "G6 follow-up" items (G6 stays closed)

| Former follow-up | Home | Notes |
|---|---|---|
| F1: EXPLAIN/PROPOSE/EXECUTE modes + immutable proposal + hash-bound authorization (roadmap G6 note; SoT §105.3) | CONTRACT here (§2 hash, §7 merge); IMPLEMENTATION in TASK-0078 (modes + propose→validate→persist→execute loop) + TASK-0079 (risk-escalation grants) | Both queue scopes already cover this; no queue edit needed or made. |
| F2a: task-scoped `CapabilityLease` (roadmap G6 note; SoT §105.4) | DEFERRED post-G7 (§7 row) | No G7 consumer; G6 Policy/Secrets cover v1 least privilege. |
| F2b: canonical DENY-attempt audit evidence (SoT §105.4) | ALREADY MATERIALIZED in G6 — no new task | TASK-0066 audit vocabulary includes denied operations; POLICY_CONTRACT §Security Audit. Nothing to re-home. |
| Handoff (SoT §105.5) | TASK-0079 (HDR lifecycle), G9 continuity support | Matches the §105.5 owner note. |

TASK-0075 makes no change to any G6 verdict, record, or artifact.

## Alternatives considered

- **Planner with direct graph write access**: rejected — violates
  INV-043/INV-004 and makes mutations unreviewable (ADR-0017).
- **Freezing the full conceptual planner surface** (capability
  routing, model selection, council deliberation) as one Rust seam:
  rejected — G7 needs the proposal/validation half only; freezing the
  rest would invent Router/Council contracts later gates own
  (same reasoning as ADR-0024 §Alternatives for `AgentProviderPort`).
- **Whole-plan regeneration on each trigger**: rejected — wasteful,
  discards unaffected-branch evidence; replan input is branch-scoped
  by contract (§1).
- **Separate durable `ActionProposal` entity alongside
  `PlanChangeSet`**: rejected — parallel proposal concepts diverge;
  the merge (§7) keeps one hash-bound proposal with TASK-0062-proven
  binding semantics.
- **Cost as a third independent enforcement limit**: rejected —
  dual token+cost accounting disputes; cost stays derived evidence
  for Verified Delivery Cost (§6).
- **Numeric bounds left to implementation tasks**: rejected — the
  TASK-0075 exit criterion requires transcribable rules; defaults
  are explicit here and tunable later as policy, not as planner
  self-configuration.

## Consequences

- TASK-0076 transcribes §§1–5 into the pure plan-domain model; every
  V-rule ID MUST appear in test names; planner output cannot mutate
  Task state by construction (port returns data, never a mutation
  handle).
- TASK-0077 persists `PlanRevision` (§4) with atomic
  revision-plus-event commits; provenance survives restarts.
- TASK-0078 builds the real planner adapter + `FakePlanner` behind
  this seam; both pass one shared contract suite whose assertion
  list is exactly §§3+5+6 plus the `PlannerFault` taxonomy (G5
  TASK-0049 pattern: suite owned by provider-side tests, run against
  both executors). Guardrails enforced before any LLM call and on
  every proposal.
- TASK-0079 implements the bounded `HumanDecisionRequest` lifecycle
  that V9 and L3/L4 escalate into; no graphical UI.
- TASK-0080 exercises the failure taxonomy (cyclic, scope escape,
  criterion weakening, policy weakening, bound breach, duplicate
  recursion, planner crash mid-proposal, adversarial content) — each
  maps to a V/L rule or `PlannerFault` above, so the matrix is
  writable directly from this ADR.
- Static planning remains legitimate until benchmarks say otherwise
  (ADR-0017 evidence boundary, unchanged).

## Security impact

Planning proposals are an untrusted privilege-adjacent input:
decomposition is a scope/policy/approval-escalation path, closed by
V3 (criterion preservation + proof-method pinning), V4 (scope
containment), V5 (no policy/approval deltas; mandatory
`required_gate` annotation above baseline risk), V9 (HDR grant bound
to the exact `proposal_hash` before persistence of any
risk-escalated revision), and the dual enforcement points (§5).
Planner identity and cost receipts are provenance, never authority;
provider/planner text cannot author audit facts (INV-152 —
planning evidence flows through the Event Journal, never directly
into the Security Audit). Approval binding gains a `proposal_hash`
field (§8/F1) under TASK-0062 exact-binding semantics: stale or
widened replay stays invalid, DENY still has no approval conversion
path (INV-148, INV-150). No secrets, credentials, or policy rules
enter planner inputs; evidence travels as refs, never payloads
(INV-008, INV-151).

## Migration / replacement impact

Planner, scheduler, and reconciler implementations stay replaceable
behind `PlannerPort` without domain rewrites; static and adaptive
strategies coexist behind the same proposal/validation boundary
(ADR-0017, unchanged). Adding a second planner adapter requires only
a new adapter crate + composition-root wiring + passing the shared
contract suite — no change to Task, Execution/Attempt, Storage,
Workspace, orchestration, completion, journal, or IPC semantics
(ADR-0024 §10 pattern). Numeric bounds (§5) and the call envelope
(§6) are policy-owned tunables: changing them needs no domain-code
restructure. No G6 artifact is migrated or reinterpreted; no
G8/G9 contract is preempted (deferral rows in §7 name their owning
gates). The normative planning contract lives in this ADR alone —
no `planning/` doc files were required, so none were added.

## References

- Source of Truth §§1, 20, 21–21.1.8 (planning/reconciler model),
  §22 (typed agent bus), §§105–105.10 + 2026-09-14 staleness note
  (research integration, follow-up placement, vocabulary overlap)
- Scape gate-placement block (RoleProfile/Mission/HDQ G7;
  AgentMessage/ContinuationCheckpoint G9; Procedures G10;
  Routine/Pulse G11); Codex gate placement (minimal human blocking
  G7); Tessra note (advisory `WorkClaim`, G7)
- ARCHITECTURE.md: Planning/Orchestration bounded context +
  canonical-state ownership (PlanRevision row)
- INVARIANTS.md: INV-002, INV-009, INV-043…050, INV-074/075/083,
  INV-086…092, INV-146…153
- ADR-0017 (proposal-driven policy refined here), ADR-0018
  (completion authority — untouched), ADR-0024 (seam + shared-suite
  pattern reused), ADR-0026 + POLICY_CONTRACT (approval/policy
  non-weakening extended, never duplicated)
- planning/roadmap.yaml G6 note + G7 objective/planned_capabilities;
  planning/tasks/queue.yaml TASK-0075..0081

## Amendment — TASK-0086 trusted child-Task materialization (2026-09-17)

### Problem

TASK-0085 verification confirmed a decomposition reachability gap. A
`PlanChangeSet.node_addition` can describe a desired node, but executable Plan
nodes ultimately require canonical Task identity. The shipped planning path
cannot pre-author every adaptive child because child authority is derived from
canonical Task relationships. Requiring pre-created children would make the
planner select from manually predicted work instead of allowing proposal-driven
decomposition.

Manual authoring of an already-known child Task may exist as a future control
operation. It is not required for planner-driven G7 decomposition.

### Decision: trusted materialization boundary

Planner output remains proposal-only. A trusted planning/reconciliation
boundary validates the complete `PlanChangeSet`, then materializes canonical
child Tasks for accepted `node_additions`.

The contract is:

- `node_addition.id` becomes the canonical Task identity after trusted
  materialization. No temporary child identity or hidden mapping exists.
- the planner/provider never creates or mutates Tasks.
- the parent Task is the immediate canonical Task being decomposed; nested
  decomposition follows the Task.parent chain.
- project identity is inherited from the owning parent and cannot be selected
  by planner output.
- creating a child never completes or satisfies the parent Task.

Canonical ownership remains:

| Fact | Canonical owner |
| --- | --- |
| Task identity | Task aggregate |
| project membership | Task/project relationship |
| parent relationship | Task.parent |
| lifecycle | Task state machine |
| accepted structure | PlanRevision |
| decomposition provenance | PlanRevision and journal evidence |
| scope, acceptance, baseline risk | Task-derived planning authority |

Plan edges are projections of accepted planning structure and do not create a
second mutable Task hierarchy.

### Materialization validation

Trusted validation derives child authority from the parent. Effective child
authority is a subset of parent authority.

The implementation must enforce:

- child scope is contained by parent scope;
- V3 acceptance coverage is preserved;
- verification obligations cannot be weakened;
- risk and required gates cannot be weakened;
- project, policy, and approval semantics are derived by trusted code.

Required failure behavior:

- missing parent: refuse;
- parent/project mismatch: refuse;
- child id equal to parent id: refuse;
- existing unrelated child id: refuse;
- duplicate proposed ids: refuse;
- unrelated existing Task capture: refuse.

Existing Tasks are never silently attached to satisfy a proposal. Retry of the
same accepted materialization is recognized through durable provenance;
different proposal hashes or ownership are conflicts.

### Durable state and recovery contract

The implementation must preserve the existing bounded-context storage model.
It must not claim cross-context ACID without evidence from storage design.

The required durable outcome is:

`task.created` evidence + canonical child Task → accepted PlanRevision →
`plan.revision_committed` evidence.

If Task and Plan facts cannot share one transaction boundary, the
implementation must introduce an explicit recoverable staged protocol. A Plan
revision is never executable while its canonical Task representation is
missing.

### L4

An accepted decomposition satisfies L4 only when at least one runnable leaf has
a canonical Task representation. A proposed but unmaterialized child cannot
satisfy runnable-leaf requirements.
