# ADR-0030 - G8 Observed Effect / Claim Verification Contract

- Status: **PROPOSED** (TASK-0121; acceptance requires an owner decision and
  independent contract review before TASK-0123..TASK-0125 implement against it)
- Date: 2026-09-21
- Gate: G8
- Carries forward: the v1 obligation boundary recorded by the ADR-0027
  TASK-0093 and TASK-0096 amendments (superseded only as §6 states).

> Contract-only ADR (TASK-0121). It freezes the G8 verification seam —
> `AgentClaim`, `EvidenceRequirement`, `ObservedEffect`, `ClaimVerification`,
> the verification rules CV-1..CV-9, and the migration path off the
> criterion-identity-only Task boundary — before any implementation.
> No domain code, no persistence, no provider calls are authorized here;
> TASK-0123..TASK-0126 implement against this contract.

## Context

ADR-0018 fixed the authority split: generation is not Definition of Done, an
agent has no `complete_task()`, and a Task becomes COMPLETED only after an
independent Evaluation/Verification path establishes that its acceptance
contract is satisfied. G3 proved the Submit -> Verify flow against a fake
evaluator. What neither ADR-0018 nor G7 decided is the data contract for what
a claim is checked *against* and what counts as having checked it. The Source
of Truth (§105.1) names exactly this gap:

```text
AgentClaim
  -> EvidenceRequirement
  -> ObservedEffect
  -> ClaimVerification
```

and assigns it to G8 Verification. Three pressures make it the first G8
contract:

1. **Self-report is cheap.** A model output claiming "tests passed" costs
   nothing to produce. ADR-0018 removed completion authority from that
   statement, but the system still lacks a typed separation between the
   statement and the independently observed runtime evidence for it.
2. **The v1 obligation boundary is implicit and lossy.** The ADR-0027
   TASK-0093 amendment recorded that a canonical Task carries acceptance
   obligations as criterion identities only, with every derived criterion
   defaulting to `VerificationMethod::AutomatedTest`; the TASK-0096 amendment
   makes materialization refuse any criterion whose declared method the Task
   cannot carry. The refusal is honest, but a strengthened proof obligation
   currently has no canonical representation at all.
3. **Receipts drift toward verdicts.** INV-072 already states that a runtime
   receipt proves execution provenance, not authorization or correctness.
   Without a distinct `ObservedEffect` type, "the run produced a receipt"
   keeps being read as "the claim is true".

Binding constraints (never weakened below): INV-009 (agents cannot mark their
own task completed), INV-072/INV-073 (receipts are provenance; durable audit
evidence is separate from model-visible receipts), INV-086 (human-approval
requirements survive any evaluation outcome), INV-092 (mandatory gates keep
runtime attestation), INV-107/108/109 (intent, requirements, execution and
verification are distinct traceable layers), INV-113 (verification is
mandatory, technique contextual), INV-114 (an implementation agent may not
weaken the evidence used to approve its own task), INV-131/INV-136
(incomplete/blocked verification is distinct from failure, and neither may
masquerade as a pass), INV-146..153 where security verification participates,
INV-152 (providers never author audit facts).

## Decision

### 1. Four types, four owners

G8 verification uses four durable concepts with separate trust levels and
separate authorship. They must not be collapsed into one "verification
state" row.

`AgentClaim` is an **untrusted statement** by an agent or provider about an
outcome: "criterion X is satisfied", "tests passed", "the file changed".
It is input to verification, never evidence of it. A claim carries no
authority and no default truth value; the absence of a claim never blocks
verification, and the presence of one never substitutes for it (CV-1).

`EvidenceRequirement` is a **trusted, policy-derivable requirement**
attached to one acceptance criterion: what observed evidence would satisfy
it, against which subject fingerprint, captured by whom. Requirements are
authored or strengthened only by trusted application services under Policy;
planner proposals may reference requirement kinds (the ADR-0027
`VerificationMethod` vocabulary, §6) but never define or weaken a
requirement (CV-7).

`ObservedEffect` is **trusted-captured runtime evidence**: a process exit
envelope, a test-suite result, an artifact hash, a filesystem or Git state
reading — captured by the trusted execution/evaluation harness, never by
the agent whose work is under evaluation (CV-2), and bound to the exact
subject it describes (CV-3). Receipts remain execution provenance
(INV-072); an `ObservedEffect` is what a receipt can support, not what it
is.

`ClaimVerification` is the **durable verdict record** binding one
criterion's `EvidenceRequirement` to the `ObservedEffect` set (or durable
human approval) that was actually checked, with a typed verdict, the
evaluator identity, and the policy fingerprint under which the check ran.
It is the only input completion authority may read (CV-4).

### 2. Field-level schema (normative names)

```text
AgentClaim
  claim_id              // unique, product-minted
  subject_ref           // Task / Execution / Attempt / artifact identity
  criterion_ref         // acceptance criterion identity the claim addresses
  claim_kind            // typed: SATISFIED | TESTS_PASSED | STATE_CHANGED | ...
  claim_payload_ref     // bounded content ref; payloads never inline secrets
  claimed_by            // agent/adapter identity (provenance, not authority)
  claimed_at

EvidenceRequirement
  criterion_id          // the acceptance criterion this requirement proves
  requirement_kind      // typed registry, §3
  subject_binding       // what the subject fingerprint covers (content,
                        // toolchain, config), per kind
  params                // kind-specific, schema-bounded (e.g. test selector,
                        // expected hash algorithm, path roots)
  policy_fingerprint    // policy under which the requirement was derived
  required              // hard-floor flag: Policy-derived minimums cannot be
                        // weakened by task/session/UX configuration (INV-148)

ObservedEffect
  effect_id             // unique, product-minted
  subject_ref           // what was observed
  subject_fingerprint   // SHA-256 over the canonical subject material the
                        // effect describes (content/config/toolchain per kind)
  effect_kind           // PROCESS_EXIT | TEST_SUITE_RESULT | ARTIFACT_HASH |
                        // FS_STATE | GIT_STATE (extensible registry, §3)
  captured_by           // trusted harness identity — never the claimant (CV-2)
  captured_at
  payload_ref           // bounded captured material (exit code, counts,
                        // hash, path set), redacted per privacy contract
  provenance            // run/attempt/evaluator lineage refs

ClaimVerification
  criterion_id
  requirement_ref       // the exact EvidenceRequirement checked
  claim_refs            // claims this verdict resolves (possibly empty:
                        // verification does not require a claim)
  effect_refs           // observed effects relied on (empty only for
                        // HUMAN_APPROVAL, which references the durable
                        // approval instead, CV-8)
  verdict               // VERIFIED | REFUTED | UNVERIFIED | BLOCKED (§4)
  verified_by           // evaluator identity (trusted service/adapter id)
  policy_fingerprint
  created_at
  superseded_by         // explicit supersession chain (CV-5), never deletion
```

### 3. Requirement and effect kind registries

v1 freezes two small registries; both are extended only by ADR amendment,
and unknown kinds fail closed (INV-149 analogue: an unverifiable
requirement kind is BLOCKED, never silently VERIFIED).

- Requirement kinds: `PROCESS_EXIT`, `TEST_SUITE`, `ARTIFACT_HASH`,
  `FS_STATE`, `GIT_STATE`, `INDEPENDENT_REVIEW`, `SECURITY_CHECK`,
  `HUMAN_APPROVAL`.
- Effect kinds: `PROCESS_EXIT`, `TEST_SUITE_RESULT`, `ARTIFACT_HASH`,
  `FS_STATE`, `GIT_STATE`. Non-capture requirement kinds
  (`INDEPENDENT_REVIEW`, `SECURITY_CHECK`, `HUMAN_APPROVAL`) resolve to
  their own durable evidence: review verdict records, G6 security-check
  results, and TASK-0062-shaped approvals respectively — they do not
  fabricate `ObservedEffect` rows.

The ADR-0027 `VerificationMethod` vocabulary (`automated-test`,
`independent-review`, `security-check`, `human-approval`) maps onto
requirement kinds as follows: `automated-test` -> `TEST_SUITE`;
`independent-review` -> `INDEPENDENT_REVIEW`; `security-check` ->
`SECURITY_CHECK`; `human-approval` -> `HUMAN_APPROVAL`. The mapping is
one-directional vocabulary reconciliation, not a strength ranking;
`VerificationMethod` deliberately derives no `Ord` and nothing here adds
one (ADR-0027 TASK-0093 amendment, carried).

### 4. Verdict taxonomy

`ClaimVerification.verdict` is a closed four-state vocabulary:

- **VERIFIED** — every element of the requirement was checked against
  in-window, fingerprint-matching evidence, and the evidence supports the
  criterion.
- **REFUTED** — the checks ran and the evidence contradicts the criterion.
- **UNVERIFIED** — no conclusive check exists: coverage missing, evidence
  stale (subject fingerprint moved), or evidence window expired. This is
  the default state of every criterion; it is reached by doing nothing.
- **BLOCKED** — the environment prevented the check from running (missing
  tool, sandbox unavailable, capture failure). Distinct from REFUTED
  (INV-131/INV-136): an analytical obligation whose check could not run is
  not a failed check, and neither may be reported as VERIFIED.

Verdicts are durable records, not computed-on-read caches. State changes
are supersessions with provenance (CV-5), so "why was this criterion
accepted?" is answerable from history, matching the ADR-0027 §4
explainability requirement for plan revisions.

### 5. Verification rules (executable-as-specified)

The trusted evaluation/verification boundary applies every rule. Rule IDs
are stable and MUST appear verbatim in implementing test names
(TASK-0123..TASK-0125 pattern from ADR-0027 §3).

- **CV-1 CLAIMS ARE NOT EVIDENCE** — an `AgentClaim` never satisfies an
  `EvidenceRequirement`, in whole or in part. Only `ObservedEffect`s
  captured by a trusted harness, review verdict records, G6 security-check
  results, or durable human approvals can. A claim with no verifying
  evidence leaves the criterion UNVERIFIED, not failed and never verified.
- **CV-2 INDEPENDENT CAPTURE AUTHORITY** — the agent or provider whose work
  is under evaluation cannot be the capture authority (`captured_by`) for
  the effects that verify its own claims, and cannot author, edit, or
  delete `EvidenceRequirement`, `ObservedEffect`, or `ClaimVerification`
  records. Provider-local gather-act-verify loops produce claims and raw
  receipts, never verdicts (INV-009, INV-114; SoT G8 hook note).
- **CV-3 SUBJECT FINGERPRINT BINDING** — verification is valid only while
  the effect's `subject_fingerprint` matches the current canonical subject
  (content/config/toolchain per the requirement's `subject_binding`). A
  moved subject makes prior evidence stale: the criterion returns to
  UNVERIFIED. Stale evidence is never silently reused (INV-065 analogue:
  similarity is not authority).
- **CV-4 REQUIREMENT COVERAGE AT COMPLETION** — a Task may reach COMPLETED
  only when every acceptance criterion holds a current VERIFIED
  `ClaimVerification` against its `EvidenceRequirement` (or the criterion
  carries no requirement because policy requires none — permitted only
  where policy says so, never by default for risk-elevated work). Missing
  coverage is UNVERIFIED, and UNVERIFIED is not a completion state. This
  binds, and does not replace, ADR-0018: the Evaluation Engine's verdict
  for completion is constructed from `ClaimVerification` records.
- **CV-5 CONTRADICTION AND SUPERSESSION** — REFUTED verdicts and
  contradictory effects remain durable and queryable. A later VERIFIED
  supersedes earlier verdicts explicitly (`superseded_by` chain with
  reason and provenance); nothing is deleted or rewritten. Uncertainty and
  contradictory evidence must remain visible (INV-131 analogue).
- **CV-6 INCOMPLETE IS NOT FAILURE, FAILURE IS NOT SUCCESS** — BLOCKED
  (check could not run) and UNVERIFIED (no conclusive check) are distinct
  from REFUTED (check ran, evidence contradicts). No roll-up, report, or
  gate may conflate them; a release/completion decision that treats
  BLOCKED or UNVERIFIED as pass — or REFUTED as pass — violates this
  contract (INV-136).
- **CV-7 POLICY FLOOR** — `EvidenceRequirement`s are derived by trusted
  services under the current Policy fingerprint. Policy and risk can only
  strengthen requirements at derivation time; task, session, UX-mode,
  provider, or workspace configuration can never weaken a requirement
  marked `required` (INV-146, INV-148). Requirements are re-derived or
  re-validated at the verification boundary; a requirement minted under a
  superseded policy fingerprint is stale input, not authority.
- **CV-8 HUMAN APPROVAL EXACT BINDING** — `HUMAN_APPROVAL` requirements are
  satisfied only by a durable approval bound to the exact subject, risk,
  and validity window, single-use and unexpired (TASK-0062 semantics,
  INV-150). Human-approval requirements survive any automated evaluation
  outcome (INV-086): an automated VERIFIED never discharges a
  `HUMAN_APPROVAL` requirement.
- **CV-9 EVIDENCE INTEGRITY** — if the implementation agent modified the
  tests, checks, or evaluators used to approve its own task beyond what
  the task's `TestMutationPolicy` allows, every `ClaimVerification`
  relying on the affected evidence is invalid and the affected criteria
  return to UNVERIFIED with the tamper recorded (ADR-0018 Test Integrity,
  INV-114).

### 6. Task/Plan integration and the v1 boundary migration

Until TASK-0123 lands, the v1 boundary stands exactly as ADR-0027's
TASK-0096 amendment states: canonical Tasks carry criterion identities,
materialization refuses any criterion whose declared method is not
`automated-test`, and no runner, scheduler, or policy code consumes
`verification_method`.

TASK-0123 replaces the implicit default with explicit data:

- A canonical Task acceptance criterion becomes `(criterion identity,
  EvidenceRequirement)`; the TASK-0093 implicit `AutomatedTest` default
  becomes an explicit `TEST_SUITE` requirement at authoring or migration
  time — visible data, not an assumption a reviewer must rediscover.
- The plan-side `CriterionDecl { id, method }` vocabulary is preserved as
  the proposal-level expression of requirement kinds (§3 mapping). V3's
  method-difference flag and V9's hash-bound escalation keep their G7
  semantics; what changes is that a strengthened obligation can now
  actually travel through materialization instead of being refused.
- `VerificationMethod` gains no ordering and no new variants here.
- Legacy rows migrate by materializing the explicit default; the migration
  must be restart-safe and must fail closed on a criterion it cannot map,
  never silently drop or weaken one (INV-046).

### 7. Relationship to adjacent contracts

- **ADR-0018** owns the completion authority split; this ADR owns the
  evidence vocabulary its Evaluation Engine consumes. Nothing here moves
  completion authority into the evaluation layer.
- **G6 security verification** (TASK-0066..0070 lane) keeps its own
  finding taxonomy; `SECURITY_CHECK` requirements reference its durable
  results rather than duplicating them. Security Audit remains
  product-authored and separate (INV-152): `ClaimVerification` records are
  verification evidence, not audit facts, and flow into audit only through
  the existing audit boundary.
- **Hooks and provider-local loops** may produce `AgentClaim`s and raw
  receipts; they never define completion transitions (SoT G8/G11 note).
- **ADR-0029 option B** (planner evidence resolution) is not absorbed
  here. G8's verification machinery is a prerequisite for measuring
  planner output quality at all, so B is re-evaluated after TASK-0125 —
  recorded here so the question has a named home instead of none.

## Alternatives considered

- **Keep criterion identities only, verify at the Task level**: rejected —
  it preserves the TASK-0093 lossy boundary and leaves proof obligations
  implicit; two G7 audits already found the failure modes of implied
  obligations (ADR-0027 TASK-0093/0096 amendments).
- **Extend runtime receipts into verdicts**: rejected — INV-072/INV-073
  keep receipts as execution provenance; mixing verdict authority into the
  model-visible receipt channel would let the evaluated party's
  environment author its own evidence (CV-2).
- **Score-based completion**: rejected — ADR-0018 already decided scores
  inform, verdicts decide; no numeric reliability gate is added here.
- **Freeze the final `EvaluationPort` Rust API here**: rejected — the G3
  port already serves the Submit -> Verify flow; TASK-0124/TASK-0125 may
  refine the port against this vocabulary, and freezing the Rust surface
  now would pre-empt implementation evidence (same reasoning as ADR-0018
  leaving the port open and ADR-0024 freezing only the seam).

## Consequences

- TASK-0123 transcribes §§2–6 into the verification domain model and the
  Task/Plan schema migration; every CV rule ID MUST appear in test names.
- TASK-0124 implements the `ObservedEffect` capture ports and the first
  local adapters behind them; capture authority is always the trusted
  harness, and the shared contract suite pattern follows ADR-0024.
- TASK-0125 binds `ClaimVerification` coverage into the ADR-0018
  completion boundary so a self-claim is structurally insufficient.
- TASK-0122 (Side-Effect Execution Protocol) is independent and does not
  depend on this ADR; AMBIGUOUS recovery evidence later feeds
  `ObservedEffect` provenance.
- G7 is untouched: no V-rule, guardrail, fence, or HumanDecision semantic
  changes until TASK-0123's explicitly scoped migration.

## Security and privacy impact

Verification evidence is privilege-adjacent: a forged `ObservedEffect` or
`ClaimVerification` is a completion-authority attack, closed by CV-2/CV-3
authorship and binding rules and by INV-146 (provider output, repository
content and frontend state can never grant authority). No secrets enter
claims, requirements, effects, or verdicts; payloads carry references, not
secret material (INV-008, INV-151). Effect payloads are bounded and
redacted per `docs/privacy/TECHNICAL_PRIVACY_CONTRACT.md`; all evidence is
local-first and requires no telemetry (INV-011, INV-014). Durable
verification evidence is separate from the Event Journal and the Security
Audit, and reaches the latter only through the existing audit boundary
(INV-152).

## Migration / replacement impact

TASK-0123 owns the only schema change: criteria gain carried
`EvidenceRequirement` data. Legacy criterion identities migrate to the
explicit `TEST_SUITE` default (§6). Historical verdict-free completions
remain valid history; nothing re-adjudicates them. Evaluator and capture
implementations stay replaceable behind ports without domain rewrites.
No G6 or G7 artifact is migrated or reinterpreted; no G9+ contract is
preempted.

## Open

- The final `EvaluationPort`/capture-port Rust shapes (TASK-0124).
- Freshness windows per requirement kind (policy-owned tunables, §5 CV-3).
- Whether planner output quality becomes a gate criterion anywhere
  (ADR-0029's open question; re-evaluated after TASK-0125, §7).
- GateAttestation runtime binding, carried from ADR-0018's open items.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §105.1 (this
  vocabulary), §44–46 (Evaluation Engine, Executable DoD), G8 gate notes
  (typed verification policy; subject/toolchain/config fingerprints;
  independent principals; hooks never redefine transitions).
- `docs/architecture/adr/ADR-0018-independent-evaluation.md` — authority
  split this contract feeds.
- `docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md`
  — TASK-0093/TASK-0096 v1 obligation boundary and the plan-side
  `VerificationMethod` vocabulary §3 reconciles.
- `docs/architecture/adr/ADR-0029-planner-evidence-resolution.md` — open
  quality question given a named home in §7.
- `docs/architecture/INVARIANTS.md` — INV-008, INV-009, INV-011, INV-014,
  INV-046, INV-065, INV-072, INV-073, INV-086, INV-092, INV-107..109,
  INV-113, INV-114, INV-131, INV-136, INV-146..153.
- `planning/roadmap.yaml` G8; `planning/tasks/queue.yaml`
  TASK-0121..TASK-0127.

