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
- `AtomicityAssessment { verdict, rationale, evidence_refs,
  planning_cost_receipt }` where
  `verdict ∈ { ATOMIC, DECOMPOSABLE, BLOCKED }`; `BLOCKED` carries a
  typed blocker. Assessment is pure: it performs zero durable writes
  (a test calling it twice must observe zero new journal rows). The
  receipt is provider-usage evidence for this `PlannerPort` call, not
  authority and not planner-authored business state.

  > **Corrected by the TASK-0116 assessment durability amendment
  > (2026-09-20).** Assessment is not free of durable writes, and this
  > clause contradicted ADR-0028 §4's requirement to reserve spend before
  > every `PlannerPort` call. What holds is narrower: assessment writes no
  > `Plan` — no `PlanRevision`, no `PlanGraph`, no accepted-revision
  > journal fact — and does not republish the branch's current planning
  > input. It does persist its own snapshot, advance the planning-run
  > generation and reserve its own spend. One exception: when assessment
  > opens a human gate, that gate publishes current input, because a
  > durable `HumanDecision` can only be created and resumed while its
  > bound snapshot is current.

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
- `evidence_refs` / `triggering_evidence_refs` on the two proposal requests
  are **cycle identity and provenance, not planner reading material**. They
  key the per-triggering-evidence budget (`planning_evidence_key`) and are
  echoed into `PlanChangeSet.source_evidence_refs`; nothing resolves them to
  content. A v1 planner proposes from its bounds, and validation is the
  safety net. ADR-0029 records this as an accepted limitation rather than an
  accident, and owns the successor question.
- `PlanChangeSet` — §2 below. Returned changesets carry no authority.
- `PlannerCallSpend ∈ { NOT_STARTED, MEASURED { receipt }, UNMEASURED }`
  is adapter-authored control-flow evidence. `NOT_STARTED` means no
  provider process/call crossed its execution boundary; `MEASURED`
  carries the provider-reported `planning_cost_receipt`; `UNMEASURED`
  means a provider call may have started but exact usage cannot be
  recovered. Model/provider text cannot choose this value.
- `PlannerFault ∈ { UNPLANNABLE { blocker, spend },
  BUDGET_EXHAUSTED { spend }, DEPTH_EXHAUSTED,
  PROVIDER_FAILURE { typed_cause, spend } }` — a planner that cannot
  produce a valid smaller decomposition reports a fault; the
  orchestrator then BLOCKs or escalates. `DEPTH_EXHAUSTED` is always a
  pre-call `NOT_STARTED` refusal. It never retries silently (INV-050,
  INV-091).

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

TASK-0078 accounting amendment: call/token guardrails are durable
application state, not caller-memory state. Before invoking
`PlannerPort`, the trusted planning service reserves one call slot and
the maximum per-call input charge (32,000 tokens) for the exact
`(plan_branch, triggering_evidence)` cycle. The call slot remains
consumed for every reserved `PlannerPort` invocation, including a
`NOT_STARTED` refusal, so repeated policy/runtime refusals cannot form
an unbounded loop. Settlement then replaces only the token reservation:

- `NOT_STARTED` → zero input-token charge and no receipt;
- `MEASURED { receipt }` → charge the reported input tokens and persist
  that exact receipt under a product-owned call identity;
- `UNMEASURED` → retain the conservative 32,000-token charge and do not
  fabricate a receipt.

If the process crashes after reservation but before settlement, the
conservative reservation remains durable. Plan revisions may reference
only a measured receipt that already resolves in durable planning-cost
storage; missing or mismatched receipt provenance fails closed. Accepted
revision lineage and per-evidence call/token/L3 counters must survive
restart so V7, revision-per-cycle and budget checks cannot be reset by
recreating an application-service object.

### 7. Vocabulary reconcile (zero undecided rows)

| Term | Verdict | Rationale |
|---|---|---|
| `ActionProposal` | MERGE into `PlanChangeSet` | One proposal object survives. EXPLAIN/PROPOSE/EXECUTE adopted as orchestration modes (§8); the immutable proposal IS the `PlanChangeSet` + `proposal_hash`; no second durable proposal entity. |
| `HandoffPacket` | MERGE into `HumanDecisionRequest` | HDR is the durable record; "HandoffPacket" survives only as the human-facing rendering name, never a second stored type. **Narrowed by the TASK-0097 amendment:** the claim that *every* packet field maps onto HDR was never true — `actions`, `uncertainty` and `question` have no HDR field. G7 carries scope/risk/evidence/options/recommendation/precondition; full packet rendering moves to G9. |
| `ResumeToken` | MERGE into `HumanDecisionRequest` continuation | Opaque handle bound to the HDR identity, non-bearer, single-use, expiry-bound — same semantics as TASK-0062 approvals. **Narrowed by the TASK-0097 amendment:** G7 carries RESUME/REPLAN/ABORT; TAKEOVER and the resulting-revision reference move to G9. |
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
`PlannerCallSpend` and receipt identity are derived by trusted adapter /
application control flow, never parsed from planner output. Provider/
planner text cannot author audit facts (INV-152 —
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
restructure. Implementations may add durable storage for the §6
reservation/settlement and revision-lineage state, but that storage is
owned by the planning boundary and does not create a second authority
for Task, Policy, Approval, Execution, or provider state. No G6
artifact is migrated or reinterpreted; no
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

## Amendment — TASK-0088 child readiness eligibility (2026-09-17)

> Accepted 2026-09-17. Owner decision of 2026-09-17 selected direct READY
> materialization over a separate readiness-admission operation; this amendment
> writes that decision down. Source of the clause text: the TASK-0087-R1 verification
> report (`docs/development/G7_TASK_0087_R1_VERIFICATION.md`), whose BLOCKED
> verdict identified readiness eligibility as unsettled authority rather than
> a demonstrated defect.

### Problem

The TASK-0086 amendment defines trusted materialization of canonical child
Tasks but does not say whether a materialized child may be authored directly
into READY. `docs/architecture/task-engine.md` permits the `DRAFT -> READY`
transition and deliberately leaves its readiness checklist open, naming User /
authorized client and the Task Application Service as initiators. The shipped
implementation marks validated children READY inside the materialization
transaction and fences on it, so the behavior needs accepted authority.

### Decision: trusted service may author children READY

For G7 materialization, the trusted Planning Application Service may declare
authoring complete and perform `DRAFT -> READY` only after V1–V9 validation,
canonical parent/project and new-ID checks, and explicit preservation of parent
scope, acceptance proof obligations, baseline risk and required gates.

- Planner output never authorizes readiness. Readiness is a trusted-code
  decision derived from validation, never from proposal content.
- READY means orchestration eligibility, not RUNNABLE and not execution
  permission. Execution authority stays with the accepted Runner/G5/G6
  boundary.
- An obligation the v1 Task contract models, whose value the child cannot
  carry, blocks materialization; silently substituting a weaker value is
  forbidden (INV-046 non-weakening). Scope is the worked example: a parent
  carrying forbidden-path constraints the G7 `ScopeBound` cannot express is
  refused outright rather than materialized without them. Obligation *kinds*
  the v1 contract does not model at all are governed by the TASK-0093
  amendment, not by this rule.
- Creating or readying a child never completes or satisfies the parent.

### Publication and fencing

The publication transaction fences input snapshot, active run/generation, Plan
head, parent/project authority, descendant membership and proposed child IDs.
The ADR-0028 amendment names which Task/project lifecycle changes invalidate
derived authority.

Committed atomically: READY children, exactly one `CREATED` (`DRAFT -> DRAFT`)
and one `MARKED_READY` (`DRAFT -> READY`) fact per child, the revision, lineage
and the plan journal fact. A refusal also rolls back approval consumption.
Previously recorded spend is not erased.

### Exact retry equivalence

Reuse of an existing child requires identical canonical Task snapshots, a valid
unique `CREATED`/`READY` pair with creation ordered before readiness, matching
Task, project and states, trusted provenance and proposal-hash causation. New
request timestamps or event IDs do not require byte-identical events.
Additional lifecycle facts beyond the required pair are a conflict, not a
match. Reuse appends no duplicate facts and never bypasses Plan head,
generation or V7 checks.

### L4 interaction

L4 executable leaves are canonical Task work, not necessarily graph sinks. They
count only with derived RUNNABLE and completed canonical Plan predecessors.
Pending children count only under this transaction's READY guarantee and only
without incomplete Task dependencies. Pending predecessors never count as
completed. No runnable work yields the specified blocker or human decision,
without automatically completing the parent.

### Acceptance envelope

This amendment states the contract; it does not certify conformance. TASK-0087
is accepted only after an independent pass verifies the merged implementation
against every clause above and reports remaining gaps as separately scoped
remediation.

## Amendment — TASK-0091 parent admission for decomposition (2026-09-18)

> Accepted 2026-09-18. Owner decision of 2026-09-18 selected the admission set
> below. Origin: an independent review of the TASK-0087..TASK-0090 chain found
> that publication never consults the owning parent's lifecycle state or
> project, and that the TASK-0089 conformance pass graded the fencing clause
> PASS while two of the fences ADR-0028 names were absent.

### Problem

ADR-0028 "Publication fencing additions" item 4 requires fencing the
"project/task state used for derived authority". The shipped publication
transaction did not read parent state at all, did not compare a materialized
child's project to its parent's, and did not require the parent Task to exist.
A decomposition therefore published under a COMPLETED parent, under a parent
that had never been created, or with children claiming a foreign project.

### Decision: admission is an invariant, not a staleness comparison

Equality against the planning-input snapshot is the wrong instrument for
lifecycle state. `Task::define_planning_metadata` is DRAFT-only, so the scope,
acceptance obligations and baseline risk that authority is derived from are
frozen once the Task leaves DRAFT; current state does not change them.
`READY -> CLAIMED -> RUNNING` are legitimate moves that must not invalidate a
planner call already in flight. What lifecycle state governs is whether
decomposition may be published at all.

Publication therefore proves three things about the owning parent, before any
other check and unconditionally:

1. **The parent exists.** A lineage reference is not evidence of a Task.
2. **The parent's state authorizes decomposition** — it is one of
   `READY`, `CLAIMED`, `RUNNING`, `BLOCKED`, `RETRY_READY`.
3. **Every materialized child carries the parent's project.**

These are invariants of publication, so they do not sit behind the
`canonical_task_authority` flag, which a caller may legitimately clear when its
snapshot was not derived from canonical Task state. The existing snapshot fence
keeps its current meaning and remains conditional.

### The admission set, and why the rest are excluded

`READY` through `RETRY_READY` is the parent's working life. `BLOCKED` is
included deliberately: task-engine lists "planning unable to produce valid
executable work" as a supported blocker cause, so decomposition is a normal
exit from BLOCKED rather than an anomaly. `RETRY_READY` is included because
another attempt is permitted there.

- **DRAFT** is excluded because planning metadata is still authorable, so the
  parent's own contract is not yet frozen while its children would already be
  orchestration-eligible.
- **SUBMITTED, AWAITING_REVIEW, VERIFYING** are excluded because the parent has
  claimed its work is finished and is under independent evaluation; adding
  READY children there silently changes what is being evaluated.
- **COMPLETED, FAILED, CANCELLED** are excluded because the work is over.
  task-engine §10 requires parent completion to rest on independent
  verification of the parent's own acceptance contract, never on children.

### Refusal semantics

A missing parent, a parent outside the admission set and a child claiming a
foreign project are all constraint violations, not stale input. The
distinction is behavioural, not cosmetic: `PlanningInputStale` is mapped by
`planned_orchestration` to the `PlanningInputMoved` **success** outcome and
reported to clients as `StaleState`, both of which promise that a fresh
planning cycle can proceed. None of these three conditions is repaired by
replanning, so reporting them as concurrent movement would hide them and
invite an unbounded replan loop.

### Consequence for the production flow

Decomposition now requires the parent to have been declared READY. Authoring a
Task and planning against it while still DRAFT is no longer a valid sequence;
the author declares eligibility first (`TaskLifecycleOp::MarkReady`), then
planning may decompose. Every fixture that planned against a DRAFT parent was
corrected to this order rather than the rule being relaxed.

### Open

The three refusals share one error type. If a caller ever needs to distinguish
them — for example to surface "parent not eligible" differently from "parent
missing" — that requires a typed refusal and an IPC mapping, and is not
established here.

## Amendment — TASK-0093 v1 obligation boundary (2026-09-18)

> Accepted 2026-09-18. Owner decision of 2026-09-18: state the boundary
> honestly in G7 and let G8 model obligations properly, rather than inventing a
> typed obligation contract here. Origin: TASK-0089 finding F-03 and the
> independent review's R-03, which disagreed with each other about what the
> durable model actually carries. Neither was right, and this amendment records
> what is.

### What the v1 contract actually models

A canonical Task carries acceptance obligations as **criterion identities**
and nothing else. There is no per-criterion proof method in the Task contract,
and `TaskPlanningAuthority::derive` therefore assigns every derived criterion
`VerificationMethod::AutomatedTest`. That default is product-owned and lives in
exactly one place.

`Task::verification` and `Task::required_review` exist as fields and are
persisted and round-tripped, but they are **not authorable and not read**:

- the domain exposes no setter for either (unlike `add_acceptance`);
- protocol v6 `CreatePlanningTaskRequest` carries `allowed_paths`,
  `acceptance_criteria` and `baseline_risk`, and nothing else, and the contract
  states there is no later edit operation;
- no planning, scheduling, policy or runner code reads them.

Through the supported control plane they are therefore always empty. They are
reserved for the typed model, not a live obligation channel.

### Consequences, stated plainly rather than implied

1. **The plan model carries all four methods; the Task contract carries none.**
   The `PlanChangeSet` expresses `CriterionDecl { id, method }`, the shipped
   planner prompt offers all four, the parser accepts all four, and
   `plan_node_criteria` and `planning_input_criteria` store them durably. What
   cannot carry a method is the canonical Task, whose acceptance is a list of
   criterion identities. `VerificationMethod` deliberately derives no `Ord`:
   the variants are a closed vocabulary, not a strength ranking, and no code
   may treat their declaration order as one.
2. **V9 method escalation does fire on a proof-method difference.**
   `check_criteria_preservation` raises `method_changed` whenever a child's
   declared method differs from the parent's, in either direction, and routes
   that to V9's hash-bound single-use grant. The parent side is not always the
   default either: for `node_removals` the base graph node's stored methods
   become the affected parent criteria.

   > **Corrected 2026-09-18 by the TASK-0096 amendment.** Both clauses above
   > previously asserted that non-`AutomatedTest` methods were unreachable in
   > v1 and that V9 could not fire on a method difference. Both were false, by
   > two independent routes, and two independent audits found it. The original
   > reasoning inspected only the Task side of the seam and never the plan and
   > provider side.
3. **INV-046 is not weakened by this, and is not fully enforced by it either.**
   INV-046 forbids weakening parent acceptance criteria, security policy, scope
   and human-approval requirements. Criteria coverage, scope containment and
   baseline risk are enforced (V3, scope containment, V5). Human-approval
   requirements are not enforced as *proof obligations*, because the v1
   contract cannot express one. This is a stated limit of G7, not a claim of
   coverage.

### Why this is not solved in G7

The roadmap assigns G8 the Verification gate, whose planned capabilities name
the Observed Effect / Claim Verification contract separating `AgentClaim`,
`EvidenceRequirement`, `ObservedEffect` and `ClaimVerification`. A typed
requirement-to-evidence relationship is that contract's subject. Adding a
per-criterion proof method to the Task contract in G7 would mean designing a
narrower version of `EvidenceRequirement` before G8 has decided its shape, then
migrating twice — a new IPC version and a schema migration for a model that is
already scheduled to be replaced.

Until G8 lands, a Runlith Task's acceptance obligations are criterion
identities proved by automated test. That is the honest description of the
system, and it belongs in the contract rather than in a reviewer's finding.

### Not a licence to widen the gap

This amendment describes v1; it does not authorize new code to depend on the
default. Specifically: no path may write `Task::verification` or
`Task::required_review` without a contract that also says who reads them, and
no path may infer obligation strength from `VerificationMethod` ordering.

## Amendment — TASK-0096 materialization refuses unrepresentable obligations (2026-09-18)

> Accepted 2026-09-18. Two independent audits found that the TASK-0093
> amendment rested on a false premise. This amendment corrects the record and
> makes the code enforce what the contract claims.

### What was wrong

TASK-0093 asserted that `VerificationMethod` beyond `AutomatedTest` was
unreachable in v1, and that V9 method escalation therefore had no differing
input. Both statements were false:

- the shipped Claude planner prompt offered all four methods and the parser
  accepted all four;
- `CriterionDecl` carries the method, and `plan_node_criteria` /
  `planning_input_criteria` store it durably — the repository already contained
  a passing test round-tripping `IndependentReview` through SQLite;
- V3 flags any method difference in either direction and routes it to V9's
  hash-bound, single-use grant;
- the parent side is not always the default: `node_removals` feed the base
  graph node's stored methods into the affected parent criteria.

The reasoning that produced the false premise examined only the Task side of
the seam. The consequence was worse than an inaccurate document: a human could
be asked for, and give, a V9 grant for a strengthened proof obligation, after
which materialization dropped the method and the child Task carried criterion
identities alone. Decomposing that child re-derived `AutomatedTest`, so a
grandchild declaring `automated-test` raised no difference and needed no
further grant. The approved obligation disappeared one level down and the
consumed grant bought nothing.

### Decision: refuse, do not downgrade

The canonical Task contract still models acceptance obligations as criterion
identities without a proof method — that part of TASK-0093 stands. What changes
is what happens when a proposal carries an obligation the Task cannot hold:

**Materialization refuses the child.** `materialize_child_tasks` rejects any
`node_addition` declaring a criterion whose method is not `AutomatedTest`,
before any Task is minted. The refusal is a bounded, visible availability
regression and a correct one: it surfaces exactly the case an owner must see,
instead of consuming a human approval and discarding what it approved.

This is the accepted non-weakening clause applied literally: an obligation the
child cannot carry blocks materialization, and substituting a weaker one is
forbidden.

### What deliberately does not change

The plan model, `VerificationMethod`, V3's comparison, V9's grant machinery and
the durable plan-side storage all keep the full four-method vocabulary. They are
the foundation G8's `EvidenceRequirement` will build on, and the executable
proof that V9's method path works stays in the suite. Only the lossy boundary
refuses.

The planner prompt is narrowed to `automated-test` so the provider is not
invited to produce what the system will reject. The prompt is guidance, not a
control: the guard stands on its own and a provider ignoring the prompt is
refused.

### Open

Enforcement of proof obligations remains absent in v1: no runner, policy or
scheduler consumes `verification_method`. Until G8 models
requirement-to-evidence, Runlith Task acceptance obligations are criterion
identities proved by automated test — now by refusal rather than by assumption.

## Amendment — TASK-0097 handoff scope reconciled with G9 (2026-09-18)

> Accepted 2026-09-18. Owner decision of 2026-09-18. Origin: two independent
> audits found that G7's accepted handoff contract is wider than anything the
> system implements, and that a queue entry — not an amendment — is what
> narrowed it.

### The drift, and how it happened

The roadmap's G7 exit criterion demanded `RESUME / REPLAN / TAKEOVER / ABORT`,
and this ADR's vocabulary table asserted that *every* HandoffPacket field maps
onto `HumanDecisionRequest`. Neither is implemented:

- `HumanContinuation` and the IPC contract carry three variants;
  `TAKEOVER` appears nowhere in shipped code.
- `HumanDecisionRequest` carries scope, risk, options, recommendation, evidence
  and precondition. `actions`, `uncertainty` and `question` have no field at
  all, so the total-mapping claim fails independently of TAKEOVER.

The narrowing was already made in practice: TASK-0079's objective in
`planning/tasks/queue.yaml` scopes the work to RESUME/REPLAN/ABORT. That is the
defect worth naming. Under the project's authority order a queue entry ranks
below an accepted ADR, so a task cannot quietly shrink a gate criterion. The
contract stayed wide, the implementation went narrow, and nothing recorded the
gap for four tasks.

### Decision

G7 owns a **bounded** human handoff: a durable `HumanDecisionRequest` carrying
scope, risk, evidence, options, recommendation and precondition, resolved by
`RESUME`, `REPLAN` or `ABORT`.

Deferred to G9, which already owns continuity: `TAKEOVER`, the
resulting-revision reference, and rendering of the remaining packet fields
(`goal`, `state`, `actions`, `blockers`, `uncertainty`, `question`).

This is a narrowing of scope, not of authority. Nothing about proposal-only
planning, fencing, approval binding or non-weakening changes. The deferred
items are user-facing richness and one additional continuation verb, none of
which is load-bearing for G7's guarantees.

### Why defer rather than implement

`TAKEOVER` means a human assumes execution authority mid-flight. That needs
semantics G7 has not settled: what happens to the active planning run, the
claimed Task, the workspace lease and the in-flight execution, and how the
Task returns to machine execution afterwards. Implementing a fourth enum
variant without those answers would produce a verb the system accepts and
cannot honour — the same shape of defect as a `required_gate` nothing enforces
or a proof method that disappears at materialization. Two audits found both of
those in this gate already.

### Consequence

`roadmap.yaml` G7 and G9 are updated to match, so the gate criterion and the
implementation now agree. G7 closure no longer requires TAKEOVER. Any future
work that reintroduces the wider contract must amend this ADR rather than
restate the wide form in a queue entry.

## Amendment — TASK-0107 planner baseline risk (2026-09-19)

### Problem

§1 fixes `DecompositionRequest` and `ReplanRequest` as normative and stable,
and states the rationale for what they carry: the planner receives its
remaining depth and budget headroom "so it can refuse before an LLM call
instead of proposing into a certain rejection."

Neither request carried the baseline risk the proposal is judged against.
V5 (§3) refuses any added node whose `risk_class` exceeds the branch baseline
without a `required_gate`. The planner could not know the threshold, so it
could not satisfy the rule except by accident.

This was not a theoretical gap. TASK-0106's real-provider smoke asked the
live Claude CLI for a decomposition against a `Low` baseline and received a
well-formed proposal containing a `medium` node with `required_gate: null` —
a certain V5 refusal, produced by a planner that had no way to know better.

### Decision: the baseline travels with the request

`DecompositionRequest` and `ReplanRequest` each gain one field:

```text
baseline_risk: RiskClass
```

It is the branch baseline the proposal will be validated against, and the
trusted planning service overwrites any caller-supplied value from
`PlanningPolicyContext.baseline_risk`, exactly as it already does for
`scope_bound`. A planner never chooses the baseline it is judged by; it is
told, for the same reason it is told its depth and budget headroom.

Adapters must state the baseline and the gate rule in provider input. The
Claude adapter carries `"baseline_risk"` in the request object and the
explicit rule that a node above the baseline must name a `required_gate`.

### Why this is not a weakening

The baseline is an input bound, not an authority. It tells the planner what
will be refused; it does not let the planner change what is refused. V5 keeps
running against `PlanningPolicyContext`, on the trusted side, over a proposal
that remains untrusted data (§1, INV-093). A planner that lies about, ignores
or echoes back a different baseline still has its nodes validated against the
policy value, and still loses.

What changes is only that an honest planner can now succeed. Before this
amendment the contract asked for something it withheld the means to produce.

### Scope

No change to `PlanChangeSet`, the validation rules, the guardrail bounds, the
budget envelope, `PlannerCallSpend`, `PlannerFault` or the composition-root
rule. `AtomicityRequest` is unchanged: assessment proposes no nodes, so it is
never judged by V5.

## Amendment — TASK-0116 assessment durability (2026-09-20)

### Problem

§1 states that assessment is pure: "it performs zero durable writes (a test
calling it twice must observe zero new journal rows)."

ADR-0028 §4 then required that "before each `PlannerPort` invocation, the
trusted planning service durably reserves one call slot and the maximum
per-call input charge." `assess_atomicity` is a `PlannerPort` invocation, so
the two accepted ADRs prescribed incompatible semantics for the same
operation: one forbade the durable write the other mandated.

The implementation followed ADR-0028 and went further. Assessment also
republished the branch's current `PlanningInputSnapshot`. Because publication
fences an accepted `PlanRevision` on its bound snapshot still being the
branch's current planning input, an assessment carrying a different evidence
set made the already-accepted revision fail that fence until something
reconciled it. A question that changed no plan could de-admit one.

This was recorded as F-07 / G7-BLOCKER-05 by the TASK-0115 closure re-audit
and blocked G7 closure.

### Decision: separate crash-safe spend from current-input publication

§1's purity clause is corrected in place by a blockquote beneath it, following
the precedent at ADR-0028 §"TASK-0086 child materialization": the accepted
sentence stays readable and its correction sits directly under it. Assessment
is **not** a zero-write operation, and pretending otherwise is what created
the conflict. It is instead bounded by what it may write:

- Assessment **may** persist its own immutable `PlanningInputSnapshot`,
  advance the branch's planning-run generation, and durably reserve and settle
  its own `PlannerPort` spend. ADR-0028 §3 and §4 apply to it unchanged: its
  call is fenced and a crash after the call cannot erase possible spend.
- Assessment **must not** republish the branch's current planning input,
  except through a human gate (carve-out below). It proposes nothing, so it
  must not otherwise affect whether any accepted revision is admissible.
- Assessment still writes no `Plan`: no `PlanRevision`, no `PlanGraph`, no
  accepted-revision journal fact. The testable form of the old clause becomes:
  calling assessment twice, without opening a gate, observes no new accepted
  revision and no change to the branch's current planning input — not zero
  durable rows.

One carve-out is explicit rather than silent, and it covers **both** of
assessment's human gates:

- the **pre-call** gate, when a guardrail trips before the planner is invoked;
- the **post-call** gate, when the planner call itself trips a bound or faults
  with `DepthExhausted`, `Unplannable` or `BudgetExhausted`.

Both publish current input, because a durable `HumanDecision` can only be
created, and later resumed, while its bound snapshot is the branch's current
planning input. A gate exists to lead to publication; it is not an assessment
result.

Enumerating only the pre-call gate is not a documentation nicety. The first
implementation of this amendment carved out the pre-call gate alone, and the
post-call gate consequently returned a `PlanningInputStale` storage error
instead of the designed `WaitingForHuman` outcome — reachable in production
through the shipped Claude planner adapter, which raises `BudgetExhausted`
from assessment. Independent review caught it. Executable coverage for this
amendment, so none of it can regress silently:

- `assessment_planner_fault_opens_a_human_gate_rather_than_failing`
  (`crates/app-services/tests/planning_service.rs`) — a faulting assessment
  reaches `WaitingForHuman`, and the decision's bound snapshot is current
  input, which is what makes it resumable;
- `atomicity_assessment_does_not_de_admit_the_accepted_revision` (same file) —
  an ungated assessment leaves an accepted revision admissible;
- `assessment_run_reserves_a_generation_without_replacing_current_input`
  (`adapters/storage-sqlite/tests/plan_persistence.rs`) — the claim still
  advances the generation and takes over the run head per ADR-0028 §3;
- `publishing_planning_input_from_a_superseded_run_is_refused` (same file) —
  the gate's publication is fenced by its own run generation, since it is the
  one writer of `planning_input_heads` not fused into a run claim.

Spend reservation and settlement stay pinned by the pre-existing
`atomicity_assessment_shares_the_bounded_recorded_call_budget`.

### Why this is not a weakening

The reservation ADR-0028 §4 requires is untouched, so the budget guardrail
keeps its crash-safety: the alternative reading — making assessment truly pure
— would have let a crash after an assessment provider call erase spend that
really happened.

What is removed is an authority assessment never needed. Publishing current
input is how an operation claims the right to have its revision accepted.
Assessment produces no revision, so the claim was meaningless to it and
harmful to everything else on the branch.

### Scope

No change to `AtomicityRequest`, `AtomicityAssessment`, the validation rules
V1–V9, the guardrail bounds, `PlannerCallSpend`, `PlannerFault`, the
publication fence itself, or ADR-0028 as written — this amendment resolves the
conflict entirely on ADR-0027's side.
