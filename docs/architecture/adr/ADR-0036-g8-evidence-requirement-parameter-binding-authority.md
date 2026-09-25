# ADR-0036 — G8 EvidenceRequirement Parameter Binding Authority

Status: **Proposed** (TASK-0150; owner review pending — this ADR is not
ratified and grants no authority until the owner accepts it)
Gate: G8

> This decision proposes who is authorized to bind exact kind-specific
> parameters (`selector` / `path` / `literal`) into a Task's
> `EvidenceRequirement` revision, when that binding may happen, and under
> what bounds — so that ADR-0034/ADR-0035's "authorized candidate-input
> selector on an exact EvidenceRequirement revision" has a lawful
> production source. It reuses the name ADR-0029 gave its deferred
> "Option B" because the owner scoped this task under that name, but this
> proposal is **narrower**: it does not resolve evidence *content* for the
> planner to read (ADR-0029's actual Option B, still unratified and
> unaddressed here). It grants no completion authority. It does not by
> itself amend any accepted ADR; if ratified, it would narrowly extend
> ADR-0032/0034/0035's binding contract with a named authority source.

## Context

ADR-0034 §2 requires that "the exact trusted `EvidenceRequirement`
revision for the criterion must explicitly authorize method
`candidate-input`, the exact catalogued selector, and a scope whose
inspected candidate inputs and asserted properties are sufficient for
that criterion." ADR-0035 repeats the same requirement for its narrower
Git-object source. Both ADRs assume this authorized, parameter-bearing
requirement revision already exists by the time TASK-0144's evaluator
runs. **No accepted contract or shipped code produces it.**

Evidence:

- `crates/task/src/requirement.rs:205-207` — `EvidenceRequirement.params`
  is documented as "Kind-specific parameters; empty means 'resolved by
  policy at check time' and is the documented v1 default." No code
  resolves that promise; there is no resolver.
- `crates/app-services/src/planning_service.rs:1850-1867` — the only
  production writer of `EvidenceRequirement`. It always constructs
  `EvidenceRequirement::new(..., Vec::new(), Some(policy_fingerprint),
  true)`: params are unconditionally empty. Every materialized child
  criterion ships an identity-only requirement.
- `crates/plan/src/risk.rs:139-152` — `CriterionDecl { id, method }` is
  the plan-side criterion declaration the planner proposes and validation
  admits. It carries no parameter field at all; there is no channel for
  kind-specific parameters to enter a `PlanChangeSet` in the first place.
- `crates/plan/src/validation.rs:478-503`
  (`check_criteria_preservation`) — when a plan changeset's children
  cover a parent's already-required criterion, a non-empty
  `requirement.params` on the *parent* side is one of the conditions that
  flips `requirement_changed = true` (V9 escalation signal). Today this
  is vacuous because params are always empty, but it means any future
  writer that starts binding real params onto a criterion sitting behind
  parent-coverage must be recognized by V9 as an authorized, non-escalating
  change, not accidentally treated as unauthorized requirement drift or
  silently ignored.
- `crates/policy/src/test_mutation.rs:1-40` — the accepted ADR-0032
  precedent for how a Runlith policy component owns a closed, versioned,
  fingerprinted enum with a fail-closed trusted default
  (`TestMutationPolicy::Forbidden`) rather than free-form strings. Section
  1 below proposes the same shape for a selector catalog.
- draft `crates/evaluation/src/candidate.rs` (unaccepted,
  `/private/tmp/runlith-task0144-adr0035-v3`, read for consumer shape
  only) — `CandidateSelector::from_requirement` demands exactly three
  params (`literal`, `path`, `selector = "candidate.file-contains.v1"`),
  a non-`None` `policy_fingerprint`, `required = true`, and
  `subject_binding = ContentToolchainAndConfig`, then derives a
  `requirement_revision` hash from those fields. That consumer shape is
  informative evidence of what the eventual authority must produce; this
  ADR does not adopt draft code as a contract, and the draft's exact enum
  values/field names remain implementation detail for TASK-0144, not
  ratified here.

So today the chain is broken at its origin: ADR-0034/0035 require an
authorized, parameter-bearing requirement revision to exist before their
evaluator can run at all, and the only production path that creates
`EvidenceRequirement` rows creates them with empty params, permanently.
Without this ADR, TASK-0144's candidate-input check has no requirement
revision it is ever allowed to accept, and the accepted contract is
unreachable in production — not merely unimplemented.

### Relation to ADR-0029 Option A, and why this is not Option B

ADR-0029 accepted Option A: the planner receives only opaque
`evidence_refs` (cycle identity and provenance), never resolved evidence
*content*. ADR-0030 §7 named Option A's deferred sibling — "the trusted
planning service resolves refs to bounded, classified content and passes
that content on the [planner] request" — as a **separate, later
question**, to be re-evaluated after TASK-0125, "so the question has a
named home instead of none." That deferred question is about what the
**planner** is allowed to read before proposing.

This ADR answers a different, narrower question: after a criterion is
already accepted (by planner proposal + V1-V9 validation, or by direct
Task authoring), who is allowed to **write** the exact catalogued
selector/path/literal bytes into its `EvidenceRequirement`, so a
downstream *evaluator* — never the planner — has something lawful to
check against. The planner in this proposal never gains read access to
evidence content and never gains authorship of `params`; ADR-0029 Option
A stands unchanged and unaffected. Calling this "Option B" (per the task
envelope's framing) is a naming convenience carried over from the
objective, not a claim that ADR-0029's actual deferred question is
resolved here. If the owner later ratifies planner content-resolution
under ADR-0029 Option B, its output would still only ever be a
**proposal**; this ADR's authorized binding boundary would still be the
sole component permitted to persist the resulting `params` durably,
identically to how it treats every other candidate value.

## Invariants to honor

- **INV-002** family (Task != Execution != Attempt != Provider Session):
  a parameter binding is a Task/Plan-graph fact; it is never represented
  as, or derived from, execution/attempt/session state.
- **INV-043** — planner output remains proposal-only; only validated
  application services may mutate the durable task graph. A proposed
  param (if ADR-0029 Option B is ever ratified) is not itself authority.
- **INV-093 / INV-146** — provider/model output, repository content, and
  frontend state can never grant authority; only a deterministic
  AI Engineering OS core decision can bind params.
- **INV-149 / INV-150** — malformed or unprovable binding fails closed;
  a binding is scoped to the exact operation, Task, requirement revision,
  policy fingerprint, provenance and validity window; a stale or widened
  replay is invalid.
- **INV-009 / INV-113 / INV-114** — an implementing agent cannot bind (or
  widen) its own acceptance criterion's params; verification technique is
  risk-dependent but never self-authored by the evaluated party.
- **ADR-0032 TestMutationPolicy** — `Forbidden` remains the trusted
  default wherever ADR-0032/0034 require it; this ADR does not touch that
  policy's own selection/authorization path (TASK-0145/0146).
- **INV-008 / INV-151** — secrets never enter `params`, prompts, the
  Event Journal, logs, or tracked config; a literal/path parameter is
  bounded, non-secret content only, never a credential or secret
  reference.
- **INV-010** — every binding is an auditable, durable state mutation.

## Decision questions

### 1. Authority / source of parameter values

Who may author the literal/path content, and how is it validated?

- **Option 1a — Product-owned catalog + operator-authored binding through
  a validated application boundary (planner may propose, never bind).**
  A new Policy Engine-owned `CandidateSelectorCatalog` (mirroring
  `TestMutationPolicy`'s closed, versioned, fingerprinted shape) lists
  accepted selector ids (initially only
  `candidate.file-contains.v1`, per ADR-0034 §2) together with the
  path-normalization/confinement rules and literal bounds each selector
  accepts. A trusted application-service boundary (parallel to
  `planning_service.rs`'s existing `EvidenceRequirement` writer) is the
  only code path permitted to construct a non-empty-params
  `EvidenceRequirement`. It accepts an operator-authored (human, via the
  trusted control surface — the same trust class ADR-0033 uses for B1
  review) or, if ADR-0029 Option B is separately ratified, a
  planner-*proposed* literal/path, but validates every value against the
  catalog before persisting: selector id must be catalogued; path must
  pass ADR-0035-style hostile-path validation (no `.`/`..`, no absolute
  paths, no NUL/separator injection, confined to the evaluated
  base/candidate tree, normalized case/traversal rules); literal must be
  bounded (draft evidence: 4096 bytes) and rejected if it matches a
  secret-classification heuristic (reuse `docs/privacy` classification,
  not a new detector). A provider, repository file, or planner proposal
  can never itself become the persisted value; only the boundary's own
  validated write can.
- **Option 1b — Planner proposes, validated application boundary is the
  sole binder (no operator authorship).** Same validated boundary and
  catalog as 1a, but the *only* proposer of `literal`/`path` is the
  planner (requires ADR-0029 Option B to exist first, since the planner
  needs resolved content to propose from meaningfully). No human operator
  path.
- **Option 1c — Policy-resolved at verification time, never bound onto
  the durable requirement at all.** Keep `EvidenceRequirement.params`
  permanently empty (today's shape); instead, `Policy` resolves the
  concrete selector/path/literal for a `candidate-input` check purely
  from the criterion id and current policy at verification time,
  outside the requirement revision. This avoids ever persisting params
  on the Task graph.

**Tradeoffs.** 1a is the most flexible and the most consistent with
ADR-0029 Option A remaining in force independently (an operator path
exists even if Option B is never ratified, so this ADR does not
implicitly require ADR-0029 Option B to ship first). 1b is smaller but
strictly depends on an unratified, deferred ADR-0029 decision and leaves
no path to bind a candidate-input requirement until that separate
question is resolved — an unacceptable ordering dependency for a
narrowly-scoped prerequisite ADR. 1c avoids durable params entirely, but
it directly contradicts ADR-0034 §2's already-accepted text ("the exact
trusted `EvidenceRequirement` revision ... must explicitly authorize
... the exact catalogued selector"): ADR-0034/0035 were written and
reviewed against an exact-revision binding, and `CandidateSelector::
from_requirement`'s draft consumer shape derives its
`requirement_revision` hash from the bound params — reopening that
already-accepted binding shape is out of this ADR's scope and would
require re-litigating ADR-0034/0035 rather than filling their gap.

**Proposed recommendation (owner to ratify): Option 1a.** It is the
smallest change that makes ADR-0034/0035 reachable without depending on
ADR-0029 Option B, and it reuses the accepted `TestMutationPolicy`
pattern instead of inventing a new authority shape.

### 2. Timing: when does binding happen?

- **Option 2a — Bind at plan application / Task materialization.** The
  application boundary in 1a runs inside the same trusted transaction
  that `planning_service.rs` already uses to write
  `EvidenceRequirement` for a materialized child (or an equivalent path
  for a directly-authored Task). Params are set once, at creation, from
  a catalog+operator/planner-proposed source available at that time.
- **Option 2b — Bind later, as an explicit requirement-revision update on
  an already-materialized Task.** A separate trusted operation (not plan
  materialization) creates a **new** `EvidenceRequirement` revision on an
  existing criterion, superseding the empty-params original, at any time
  before verification is attempted.
- **Option 2c — Resolve at verification time only (no durable params
  ever).** Rejected here as a duplicate of Option 1c; not repeated.

**Tradeoffs.** 2a keeps "exact requirement revision" meaning what
ADR-0030/0032 already assume — one revision, bound once, replayed
identically at verification and on restart — and requires no new
supersession/versioning machinery beyond what
`define_evidence_requirements` already has. Its cost is that
`crates/plan/src/validation.rs:478-503`'s V9 `check_criteria_preservation`
must explicitly recognize an authorized, catalog-validated non-empty-params
requirement as non-escalating when it covers a parent criterion — today's
`!requirement.params.is_empty()` check would otherwise flag every such
binding as `requirement_changed`. That recognition is a scoped
implementation change to an existing V9 comparison, not a new invariant.
2b avoids touching V9's coverage-preservation check (binding never
appears inside a `PlanChangeSet`) but introduces a second kind of
requirement-revision mutation outside plan materialization, needs its own
staleness/replay rules distinct from `PlanChangeSet` revisioning
(INV-044), and creates a window where a criterion is `required = true`
but has no bindable params yet — indistinguishable from today's
permanent state without an explicit "pending binding" signal.

**Proposed recommendation (owner to ratify): Option 2a**, with the
V9 comparison change called out explicitly as required follow-up
implementation work (§ Implementation follow-up below), not silently
assumed.

### 3. Revisioning and durability

- **Option 3a — New `EvidenceRequirement` revision on every binding,
  durable record of who/what bound it plus policy fingerprint, audit
  event; existing empty-params tasks keep their current
  `PendingEvaluator`/manual verification path unchanged.** The bound
  requirement is `params.is_empty() == false`; an unbound one stays
  exactly as it is today. No historical task is touched or
  re-adjudicated (mirrors ADR-0032's "Historical completed Tasks are not
  re-adjudicated").
- **Option 3b — Retrofit existing empty-params requirements in place
  (mutate, not revision).** Rejected outright: violates the coding-style
  immutability rule and, more importantly, ADR-0030's exact-revision
  binding — a `ClaimVerification` that already referenced the old
  revision would silently point at different content.

**Proposed recommendation (owner to ratify): Option 3a.** It is the only
option consistent with ADR-0030's exact-revision binding and with "no
regression" for tasks that never opt into candidate-input evidence.

### 4. Failure semantics

- **Option 4a — Missing/invalid/stale/unauthorized params → the
  criterion stays blocked/unverified; never `VERIFIED`.** Matches
  ADR-0034 §2's existing text verbatim ("Missing, stale, mismatched, or
  insufficient authorization is blocked/unverified").
- **Option 4b — Fall back to a default/best-effort selector when params
  are missing.** Rejected: would let an unauthorized default silently
  stand in for an owner-scoped catalog entry, contradicting INV-149's
  fail-closed requirement for unprovable mandatory restrictions.

**Proposed recommendation (owner to ratify): Option 4a** — no new text
needed; this ADR only supplies the missing authority, it does not relax
ADR-0034's existing failure semantics.

### 5. Scope limits

- **Option 5a — v1 supports only the catalogued selector(s) already
  accepted by ADR-0034/0035 (`candidate.file-contains.v1`); no arbitrary
  selectors, no generated/external inputs, no expansion of what a
  selector may claim to evaluate.** The catalog in §1 is closed and
  versioned; adding a selector id is itself a reviewed catalog-revision
  event, not a runtime parameter.
- **Option 5b — Allow free-form selector strings validated only by shape
  (e.g., a regex), deferring catalog membership to policy at
  verification time.** Rejected: reopens the "matching source text or a
  commit name alone is insufficient" problem ADR-0034/0035 already closed
  for selector authorization, and lets a binder invent selectors the
  evaluator was never reviewed against.

**Proposed recommendation (owner to ratify): Option 5a.**

## Consequences

- ADR-0034/0035's "authorized candidate-input selector on an exact
  `EvidenceRequirement` revision" gains a named, reviewable production
  source; TASK-0144's evaluator has a lawful requirement to check against
  instead of an unreachable precondition.
- `crates/app-services/src/planning_service.rs` gains a second,
  narrowly-scoped `EvidenceRequirement` writer (the binding boundary),
  distinct from the existing empty-params materialization writer; both
  remain the only production writers.
- `crates/plan/src/validation.rs`'s V9 `check_criteria_preservation` needs
  a scoped follow-up change (§ Implementation follow-up) to recognize an
  authorized bound-params requirement as non-escalating when it covers a
  preserved parent criterion.
- Existing empty-params tasks are unaffected: they remain
  `PendingEvaluator`/manual-verification exactly as today; this ADR adds
  a new lawful path, it does not require every task to adopt it.
- `crates/policy` gains a new closed catalog type, structurally parallel
  to `TestMutationPolicy`, with its own revision/fingerprint.

## Security considerations

A parameter binding is a completion-adjacent authority artifact: an
unauthorized or malformed binding could make an evaluator "PASS" a
criterion it never meaningfully checked. This ADR requires (per §1) that
only a trusted, validated application boundary — never provider output,
repository content, or an unvalidated planner proposal — persists
`params`; that every literal/path is bounded and hostile-input-checked
(reusing ADR-0035's path-validation posture rather than inventing a new
one); that literals are screened against secret classification before
persistence (INV-008/151); and that the catalog itself is a reviewed,
versioned, fingerprinted artifact (INV-149/150), not free text. No new
network, filesystem, or process capability is proposed; this ADR is
Task/Plan-domain data-shape and authority-boundary work only.

## Rejected alternatives (ADR-level)

- **Do nothing; leave ADR-0034/0035 permanently unreachable.** Rejected:
  the owner explicitly scoped this task because production reachability
  blocks TASK-0144's evaluator and TASK-0125's completion admission.
- **Let TASK-0144 invent an ad hoc binder inside its own implementation
  task instead of a dedicated ADR.** Rejected: binding authority is
  exactly the class of decision AGENT.md §11 requires an ADR for
  (architecture-sensitive authority/timing/durability change), and
  ADR-0032/0034/0035 were each split into their own ADR for the same
  reason.
- **Absorb this into ADR-0029 as its Option B.** Rejected: ADR-0029
  Option B is about planner *read* access to evidence content, a
  separately deferred and larger privacy-sensitive question (ADR-0029
  "Security and privacy impact": B "requires a classification and
  redaction policy ... before implementation"). Conflating the two would
  block this narrower, non-privacy-sensitive prerequisite on that larger
  unresolved question.

## Open questions for the owner

1. Does the trusted control surface for operator-authored binding (§1a)
   reuse the same "fresh interactive platform user-presence" authority
   ADR-0033 defines for B1 review, or is a lighter authority sufficient
   because binding selects among a closed catalog rather than rendering
   an independent verdict?
2. Should the catalog (§1a) live in `crates/policy` alongside
   `TestMutationPolicy`, or in `crates/task`/`crates/plan` next to
   `EvidenceRequirement`/`CriterionDecl`? This ADR recommends `policy`
   for authority-ownership symmetry with `TestMutationPolicy` but defers
   the exact crate placement to implementation review.
3. Is Option 2a's V9 change (§2) acceptable as a follow-up inside the
   implementation task, or does the owner want it ratified as its own
   ADR amendment first, given it touches an already-accepted validation
   rule?

## Implementation follow-up (not created in the queue; proposed here only)

- **IMPL-A — Catalog and binding boundary.** Add the closed, versioned,
  fingerprinted selector catalog (§1a) to `crates/policy`; add the
  trusted application-service binding boundary that validates and
  persists a new `EvidenceRequirement` revision with non-empty `params`.
  Depends on: this ADR's ratification, TASK-0146 (policy model
  precedent), TASK-0149 (ADR-0035, for the path-validation posture to
  reuse).
- **IMPL-B — V9 preservation-check update.** Update
  `crates/plan/src/validation.rs:478-503` so an authorized, catalog-valid
  bound-params requirement covering a preserved parent criterion is not
  flagged as `requirement_changed`. Depends on: IMPL-A.
- **IMPL-C — Operator control-surface binding UI/IPC path.** Expose §1a's
  binding operation through the trusted Runlith control surface (parallel
  to ADR-0033's B1 review surface). Depends on: IMPL-A; may depend on
  whichever task implements ADR-0033's trusted control-surface primitives
  if not already available.
- **IMPL-D — TASK-0144 integration.** Once IMPL-A lands, TASK-0144's
  evaluator consumes bound requirements through the accepted
  `CandidateSelector::from_requirement`-shaped contract (draft reference
  only; the concrete Rust shape is TASK-0144's own implementation
  decision, not ratified here).

None of IMPL-A..D are created as queue tasks by this ADR; the owner
decomposes and schedules them after ratification.

## References

- ADR-0029 — planner evidence resolution (Option A accepted; Option B
  named but not addressed here).
- ADR-0030 — CV-1..CV-9, exact evidence/policy/subject binding; §7
  records ADR-0029 Option B's deferred status.
- ADR-0032 — CV-9 test integrity, `TestMutationPolicy` precedent shape.
- ADR-0033 — independent review authority precedent (trusted control
  surface, fresh user-presence).
- ADR-0034 — candidate-bound governed test execution; §2 requires the
  authorized-selector binding this ADR supplies a source for.
- ADR-0035 — immutable Git candidate input; path-validation posture
  reused by §1a.
- `docs/architecture/INVARIANTS.md` — INV-002, INV-008..010, INV-043,
  INV-093, INV-113, INV-114, INV-146, INV-149, INV-150, INV-151.
- `crates/task/src/requirement.rs:200-210` — `EvidenceRequirement.params`
  v1-default documentation; no resolver exists.
- `crates/app-services/src/planning_service.rs:1850-1867` — sole
  production `EvidenceRequirement` writer; always empty params.
- `crates/plan/src/risk.rs:139-152` — `CriterionDecl` carries no params
  field.
- `crates/plan/src/validation.rs:478-503` — V9 `requirement_changed`
  signal on non-empty parent params.
- `crates/policy/src/test_mutation.rs:1-40` — closed policy-enum
  precedent this ADR's catalog shape follows.
- `planning/tasks/queue.yaml` — TASK-0125, TASK-0143, TASK-0144,
  TASK-0148, TASK-0149.
