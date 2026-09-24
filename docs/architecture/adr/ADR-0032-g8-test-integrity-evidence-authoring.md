# ADR-0032 — G8 Test Integrity and Trusted Verification Evidence

Status: Proposed (TASK-0143; owner decision and independent review pending)
Gate: G8

> This proposal has no completion authority. ADR-0018 and ADR-0030 remain
> binding until an owner accepts a reviewed decision.

## Context

ADR-0030 CV-9 invalidates evidence when an implementation agent changes the
tests, checks or evaluators used to approve its own Task beyond the Task's
`TestMutationPolicy`. ADR-0018 deliberately left that policy's serialization
open. The unmerged TASK-0125 review at `c33bbee..2b93942` exposed the
resulting gap: CV-9 is not enforced, `VERIFIED` verdicts may have no evidence
references, verdicts do not bind the exact requirement and policy, and the
G7 completion fixtures mint verdicts in test code without a trusted
`TEST_SUITE_RESULT` producer. The implementation also reads the current
subject before opening its completion transaction, leaving a freshness race.

These are contract and authority questions. A green test suite containing
synthetic `VERIFIED` rows is not evidence that the production completion
path satisfies CV-1..CV-9.

## Recommended decision for owner review

1. **Canonical policy and baseline.** A trusted Task/application authority
   owns a versioned `TestMutationPolicy` bound to the Task and its evaluated
   base revision. Provider output, repository instructions and a review
   fixture cannot create or weaken it. A trusted evaluator compares the
   candidate revision with that base to identify changed test, check and
   evaluator inputs. If the policy or required comparison cannot be
   established, affected criteria remain `UNVERIFIED` and completion
   refuses. Historical completed Tasks are not re-adjudicated.
2. **Integrity decision.** The evaluator records a durable assessment
   naming the base/candidate revisions, relevant changed paths or content
   digests, policy fingerprint, affected evidence/check identities, and
   any exact independent review or human approval that the chosen policy
   requires. Unauthorized changes produce a durable tamper result; every
   dependent `ClaimVerification` is ineligible and the affected criterion
   returns to `UNVERIFIED` (CV-9). A later authorized assessment supersedes
   the old one; it does not delete it.
3. **Evidence producer.** The trusted Evaluation/Runner composition may
   author an `ObservedEffect` only from a structured result returned by an
   accepted governed execution path. `TEST_SUITE` requires
   `TEST_SUITE_RESULT` with executed selector, pass/fail/skip counts,
   result digest, subject/config/toolchain fingerprint, attempt lineage,
   capture authority and time. Console prose, provider self-report and
   runtime receipts are provenance, not a test result. Missing or
   unsupported producers yield `BLOCKED` or `UNVERIFIED`, never `VERIFIED`.
4. **Verdict binding.** A `ClaimVerification` binds one exact criterion
   requirement revision, policy fingerprint and subject fingerprint to
   typed durable evidence references of the required kind. `VERIFIED`
   requires a nonempty, resolvable, in-window basis and a current integrity
   assessment where CV-9 applies. Claims may be absent; `claim_refs` can
   be empty. A receipt or session reference cannot by itself satisfy a
   requirement. Review and security requirements reference their own
   durable verdicts, while human approval uses its exact CV-8 discharge.
5. **Completion snapshot.** The trusted completion transaction rechecks
   the Task state, current subject, requirement/policy version, selected
   evidence and approval availability before committing the state change,
   journal event, admission record and only the selected approval
   consumptions. A concurrent change fails closed; no stale admission
   report is committed.

These rules describe the proposed minimum contract. The exact v1
`TestMutationPolicy` modes and the structured test-result adapter remain
subject to the owner decisions below; TASK-0144 implements the accepted
contract, and TASK-0125 consumes its durable evidence.

## Alternatives

- **Treat any test/check/evaluator edit as forbidden.** Smallest policy
  surface, but blocks legitimate test repairs and cannot represent the
  independent-review case ADR-0018 anticipates.
- **Allow edits whenever the final tests pass.** Rejected: the evaluated
  agent could weaken its own approval evidence, violating INV-114/CV-9.
- **Rely only on an independent model review.** Rejected as a universal
  substitute: review can be required by policy, but it cannot replace
  structured test execution or deterministic changed-input measurement.
- **Recommended:** a small trusted policy with a fail-closed default and
  exact independent-review/human-approval references for permitted edits.
  Avoid a general policy language until a concrete task needs it.

## Consequences and security impact

- TASK-0144 can add the first production evidence producer without giving
  the provider completion authority or adding a direct process probe.
- TASK-0125 must stop accepting empty or wrong-kind evidence, missing
  risk, stale subject/policy, and unrelated approval discharges. Its
  existing G7 fixtures must use producer-authored evidence.
- The integrity assessment is a product-authored durable fact, separate
  from model-visible receipts and the G6 Security Audit. It carries
  fingerprints and references, never secret payloads.
- Same-user direct SQLite tampering is not solved by this contract; the
  local OS principal remains the existing storage trust boundary.

## Migration and sequencing

Only schema version 20 is in `main` at TASK-0143 start. TASK-0125's
`0021_claim_verification.sql` exists solely in its unmerged branch.
TASK-0144 must allocate the next schema version against the accepted main
HEAD. If it uses 0021, TASK-0125 must reconcile its migration to the next
available version after TASK-0144 lands, then rerun migration and full
workspace checks. No hidden cherry-pick/rebase or reuse of a numbered
migration with different contents. Historical verdict-free completions
remain valid history under ADR-0030.

## Open owner decisions before acceptance

1. Select the minimal v1 `TestMutationPolicy` modes and the exact
   authorization for changed approval tests: strict forbid, scoped allow
   with independent review, and/or human approval. Define the absence
   default and how policy updates invalidate prior assessments.
2. Confirm whether the trusted changed-input baseline is a Git commit
   plus content digests, or another already-durable workspace revision.
3. Select the first structured `TEST_SUITE_RESULT` source supported by
   the current governed Runner. Do not infer a test result from stdout.

## References

- ADR-0018 — independent evaluation and open TestMutationPolicy schema.
- ADR-0030 — CV-1..CV-9, exact evidence and policy binding.
- ADR-0024 — governed provider execution seam.
- INV-009, INV-072, INV-092, INV-114, INV-145, INV-146..153.
- planning/tasks/queue.yaml — TASK-0124, TASK-0125, TASK-0143, TASK-0144.
