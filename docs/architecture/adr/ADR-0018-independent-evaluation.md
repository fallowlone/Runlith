# ADR-0018 - Task Completion Requires Independent Evaluation

Status: Accepted
Date: 2026-09-09

## Context

An implementation agent has a structural incentive to report success: model
output claiming "done" costs nothing to produce. Completion granted on
self-report would collapse the product's core promise — tracked, verified,
recoverable work — into chat optimism. The FakeAgent pipeline (G3) already
ends in Submit → Verify semantics, and the full Verification pipeline (G8)
must rest on the same authority split from the start.

## Decision

Generation is not Definition of Done. The completion flow is:

```text
Implementation Agent
 ↓
submit_result
 ↓
Evaluation / Verification
 ↓
VERIFIED / REJECTED
 ↓
trusted Task application transition
```

The agent has no `complete_task()`; it may only submit a result with
evidence. Provider/model claims ("done", "tests passed", "works") grant no
completion authority.

The Evaluation Engine owns independent assessment of whether submitted work
satisfies required evidence and acceptance. Potential inputs include tests,
lint, typecheck, security checks, acceptance criteria, review, and
benchmarks — the exact combination is Task/risk/policy dependent, and no
evaluator is required to run every category.

Evaluation produces a trusted verdict plus evidence; it never reaches
directly into Task private persistence. The trusted Task/application layer
commits the authoritative Task transition on valid Evaluation evidence
(no cross-bounded-context storage reads/writes).

A Task becomes COMPLETED only after the required Evaluation/Verification
path establishes its acceptance contract is satisfied. None of exit code 0,
test claims, provider self-report, session end, or child completion equals
Task completion.

Verification is mandatory; technique is contextual. Strategies may include
unit tests, property-based tests, contract tests, integration tests,
state/model-based tests, fuzzing, security/static analysis,
performance/regression benchmarks, independent model review, and human
approval. TDD is not mandated as the universal implementation process.

Test Integrity: an implementation agent may not make its own implementation
pass by silently weakening the evidence used to approve it. Tasks may carry
a `TestMutationPolicy` with concepts such as Forbidden,
AllowedInScopedFiles, RequiresIndependentReview, RequiresHumanApproval
(schema not frozen here). Evaluation records preserve what was evaluated,
which criteria applied, which checks actually executed, what passed/failed,
and whether the implementation agent modified approval evidence.

A configured verification/review gate is not proof the expected gate
actually executed — completion requires runtime/evidence-backed attestation
(implementation of GateAttestation itself is not decided here).

Evaluation semantics stay provider-neutral: no `ClaudeReviewPassed` /
`CodexScore` / `GeminiApproval` core types; provider/model contribution may
later appear as evidence metadata only. A future evidence-backed
Reliability Score stays optional product functionality, never completion
authority — no numeric score is required for G3/G8.

Evaluation answers whether submitted work satisfied required evidence;
Policy answers what verification/review/approval is required. Mechanism and
policy stay separate.

Explicitly left open (not decided here): final EvaluationPort Rust API,
EvaluationResult schema, review taxonomy, verification taxonomy, review
score threshold, universal reliability formula, TestMutationPolicy
serialization, human approval UI, security verifier implementation.

## Alternatives considered

- Agent self-completion (`complete_task()`): rejected; it makes completion
  claims unfalsifiable (INV-009).
- TDD mandated universally: rejected; verification is mandatory but
  technique is risk/context dependent (INV-113).
- Numeric-score-gated completion: rejected; scores inform, verdicts decide.

## Consequences

- G3 FakeAgent Submit → Verify flow and G8 full Verification share one
  authority model from the start.
- Every COMPLETED Task carries verdict + evidence refs an auditor can replay.
- Evaluators stay replaceable without touching completion authority.

## Security impact

False completion is a trust vulnerability: silent test/evaluator weakening
is governed by TestMutationPolicy discipline plus independent review
(INV-114), mandatory gates keep runtime attestation (INV-092), and
human-approval requirements survive any evaluation outcome (INV-086).
Evaluation evidence joins the audit trail; receipts prove execution only,
never correctness.

## Migration / replacement impact

Evaluator implementations (tests, static analysis, model review, human
gates) are swappable adapters behind the Evaluation boundary; tightening
or loosening required evidence per Task/risk/policy needs no domain
rewrite.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table),
  §§44–46 (Evaluation Engine, Executable Definition of Done, Reliability
  Score), §104 research-adoption notes (Test Integrity; Gate attestation)
- `INVARIANTS.md`: INV-009, INV-068, INV-086, INV-092, INV-107, INV-108,
  INV-113, INV-114
