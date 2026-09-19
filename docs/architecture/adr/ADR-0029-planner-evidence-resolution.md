# ADR-0029 — Planner evidence resolution

- Status: **ACCEPTED** (option A; owner decision, 2026-09-19)
- Date: 2026-09-19
- Gate: G7
- Supersedes: nothing. Amends the reading of ADR-0027 §1 `DecompositionRequest`.

## Problem

TASK-0106 ran the G7 planning path against the real `claude` CLI for the
first time. In the configuration shipped at the time, the provider refused
to answer and said why:

> there is no `repo:src` it can see, no `criterion-root` it has seen and no
> `evidence-planner-smoke` it can verify, so producing the JSON would mean
> inventing planning data with no basis

TASK-0109 established that the refusal itself was mechanical — the planner
role was sent as message-body text competing with the provider's own system
prompt, under a permission mode that made the model treat it as an override
attempt. Fixing the call shape made the smoke pass, 4 of 4.

**It did not make the objection false.** The provider complies now because
the role is legitimately its own, not because the evidence became
resolvable. The substantive question survives its mechanical half and is
recorded here rather than closed by a green test.

## What the planner actually receives today

`DecompositionRequest` (ADR-0027 §1) carries:

| field | what the planner can do with it |
|---|---|
| `parent_task_ref` | opaque string |
| `acceptance_criteria_refs` | opaque `CriterionId` strings |
| `evidence_refs` | opaque `EvidenceRef` strings |
| `scope_bound` | **real values** — `workspace:path` roots |
| `base_plan_revision`, `depth_remaining`, `budget_remaining`, `baseline_risk` | real bounds |

Everything describing *the work* is an opaque identifier. Only the bounds
are substantive.

Repository evidence for what `EvidenceRef` is for, as built:

- **Cycle identity.** `planning_evidence_key` (`crates/plan/src/hash.rs`)
  hashes the ref set into the key for the per-triggering-evidence-event
  call and token budget. The refs identify *which planning cycle this is*.
- **Provenance.** `PlanChangeSet.source_evidence_refs` echoes them, and the
  Claude adapter refuses an assessment that invents a ref it was not given.
- **Nothing else.** There is no resolver. No code path anywhere turns an
  `EvidenceRef` into content. In production they are identity strings such
  as `attempt:<id>` and `human-decision:<id>`.

So the contract already treats evidence as identity, not as material. The
planner is handed provenance keys and asked to decompose against them.

## The consequence, stated plainly

A planner given only identifiers produces **plausible structure, not
informed structure**. It cannot be otherwise: nothing it receives describes
the work. The decompositions the smoke accepts are well-formed, in-scope,
V5-clean and entirely invented.

This is not currently unsafe. INV-043 keeps the proposal a proposal, V1-V9
validate it trusted-side, INV-093 keeps the output untrusted, and
materialization (ADR-0027 TASK-0086/0096) refuses obligations the system
cannot honour. A bad decomposition is refused or produces bad child tasks
that later evidence corrects.

But it means G7's planning capability, as built, is **structurally
incapable of using evidence**, while its contract and its prompt both
speak as though it does. That gap should be an explicit decision, not an
accident nobody wrote down.

## Options

### A — Accept identity-only as the v1 semantic

Record that a v1 planner proposes from bounds and identifiers, that
validation is the safety net, and that evidence refs are cycle identity and
provenance only. Change nothing in the contract; change the prompt so it
stops implying the planner can read evidence.

- Cheapest, and honest if written down.
- Leaves a planner whose output quality is unbounded by anything real.
- Risk: the phrase "evidence_refs" keeps inviting the opposite reading, in
  this ADR's absence and in the prompt.

### B — Add a resolution boundary

The trusted planning service resolves refs to bounded, classified content
and passes that content on the request. The planner still never holds a
handle, a query or a store.

- Makes the planner's input real, which is the only way its output can be.
- Cost: a new contract surface, a size/redaction policy, and a privacy
  question — resolved content leaves the device with the provider call
  (`DATA_FLOW_MAP`, PRIV-010), which identifiers largely do not.
- Interacts with the token budget this session just measured: content is
  the one thing that would grow per-call input again.

### C — Narrow the port instead

Drop `evidence_refs` from `DecompositionRequest` and keep them only where
they are used — the guardrail key and the changeset provenance — so the
contract stops promising the planner something it never gets.

- Smallest contract that matches the implementation.
- Does not improve planning; makes the limitation structural and visible.
- Loses the ability to add B later without re-widening the port.

## Decision

**Option A, accepted.** B becomes its own gate-scoped work, never a silent
widening.

The measurement says the planner path works mechanically and is sound
under validation. That is enough for G7, whose objective is the planning
*machinery* — typed messages, versioned revisions, PlannerPort, guardrails
— not planner output quality.

What G7 should not do is close while the contract implies a capability the
code cannot perform. So: adopt A explicitly, fix the prompt to stop
implying evidence is readable, and record B as the named successor with its
privacy and budget questions attached.

C is rejected: removing `evidence_refs` would break `planning_evidence_key`
and the provenance echo, both of which use them for exactly what they are.

## Consequences

- ADR-0027 §1 gains a note that `evidence_refs` are cycle identity and
  provenance, not planner reading material.
- The adapter prompt stops naming evidence as something to reason from.
- `providers.md` §16 loses the D5 open item and gains a pointer here.
- G7 closure becomes defensible: the gate's capability is the machinery,
  and the machinery is proven end to end against a real provider.
- The quality question moves to whichever gate owns planner output quality,
  with B as its entry point.

## Security and privacy impact

A: none. No new data crosses the provider boundary; identifiers already do.

B, if later accepted, is privacy-sensitive: resolved evidence content would
leave the device with the provider call. It requires a classification and
redaction policy under `docs/privacy/TECHNICAL_PRIVACY_CONTRACT.md` and an
explicit `DATA_FLOW_MAP` entry before implementation, not after.

## Open

Whether planner output quality is a gate criterion anywhere. This ADR
asserts it is not G7's, and does not claim to know whose it is.
