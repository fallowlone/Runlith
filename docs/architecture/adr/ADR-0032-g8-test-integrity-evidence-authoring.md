# ADR-0032 — G8 Test Integrity and Trusted Verification Evidence

Status: Accepted (TASK-0143; owner v1 decision, independent review complete)
Gate: G8

> This ADR defines the CV-9 integrity and evidence-production contract.
> It grants no completion authority and does not weaken ADR-0018 or ADR-0030.

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

## v1 decision

Use two policy states: `Forbidden` (the trusted default when no exception is
selected) and `ScopedReviewed` (an explicit Task authorization for exact
test/check/evaluator paths and check identities). A changed input outside
that scope is tampering. A change inside it is eligible only after an
independent review record binds the Task, immutable base, candidate and
changed-input manifest, affected checks, and policy fingerprint. The
reviewer cannot be the implementing agent. Human approval is additionally
required when the Policy Engine's risk/requirement decision calls for it;
changing an ordinary test does not create a universal human-approval gate.
A missing or changed selected policy invalidates the old assessment; it
never silently widens the scope.

1. **Canonical policy and baseline.** The Policy Engine owns the
   `TestMutationPolicy` definition, revision and authorization rules. The
   trusted Task/application layer persists the selected immutable policy
   revision/fingerprint and evaluated-base binding; Evaluation applies it.
   Provider output, repository instructions and a review fixture cannot
   create or weaken it. A trusted evaluator compares the
   candidate revision with that base to identify changed test, check and
   evaluator inputs, including newly added and untracked inputs used by
   candidate checks. The base is an immutable Git commit plus content
   digests; the candidate is a captured path/type/content manifest bound
   to the evaluated subject, including dirty-worktree inputs when present.
   A bare `git diff` or commit name cannot establish candidate contents.
   If the selected policy or required comparison cannot be
   established, affected criteria remain `UNVERIFIED` and completion
   refuses. Historical completed Tasks are not re-adjudicated.
2. **Integrity decision.** The evaluator records a durable assessment
   naming the base/candidate revisions, relevant changed paths or content
   digests, policy fingerprint, affected evidence/check identities, and a
   canonical digest of the candidate input manifest plus governed invocation
   descriptor (check identity, selector, resolved executable, configuration,
   toolchain and relevant evaluator inputs), plus any exact independent
   review or human approval required by the chosen policy. Unauthorized changes produce a durable tamper result; every
   dependent `ClaimVerification` is ineligible and the affected criterion
   returns to `UNVERIFIED` (CV-9). A later authorized assessment supersedes
   the old one; it does not delete it.
3. **Evidence producer.** The trusted Evaluation/Runner composition may
   author an `ObservedEffect` only from a structured result returned by an
   accepted governed execution path. `TEST_SUITE` requires
   `TEST_SUITE_RESULT` through a trusted test adapter at the governed
   Runner boundary, with executed selector, pass/fail/skip counts,
   result digest, subject/config/toolchain fingerprint, attempt lineage,
   capture authority and time. The result also carries the canonical digest
   of the exact candidate input manifest and governed invocation descriptor
   recorded by its integrity assessment, including dirty and untracked
   inputs. The adapter validates a structured harness result against the
   governed invocation and Runner outcome before it authors a durable
   effect; neither child nor provider writes an
   authoritative effect directly. Console prose, provider self-report and
   runtime receipts are provenance, not a test result. Missing or
   unsupported capture emits no trusted test-result effect; the later
   verification boundary maps that condition to `BLOCKED` or `UNVERIFIED`,
   never `VERIFIED`.
4. **Verdict binding.** A `ClaimVerification` binds one exact criterion
   requirement revision, policy fingerprint and subject fingerprint to
   typed durable evidence references of the required kind. `VERIFIED`
   requires a nonempty, resolvable, in-window basis and a current integrity
   assessment where CV-9 applies. The assessment and each relied-on test
   result must have the same candidate-manifest and invocation-descriptor
   digest; a missing or mismatched binding is `UNVERIFIED` (or `BLOCKED`
   when trusted capture could not run), never `VERIFIED`. Claims may be
   absent; `claim_refs` can be empty. A receipt or session reference cannot
   by itself satisfy a requirement. Review and security requirements reference their own
   durable verdicts, while human approval uses its exact CV-8 discharge.
5. **Completion snapshot.** The trusted completion transaction rechecks
   the Task state, current subject, requirement/policy version, selected
   evidence and approval availability before committing the state change,
   journal event, admission record and only the selected approval
   consumptions. A concurrent change fails closed; no stale admission
   report is committed.

These rules describe the v1 contract. TASK-0144 selects the first
concrete structured harness format and implements the accepted contract;
TASK-0125 consumes its durable evidence. If no harness satisfies the
contract, capture stays unsupported and completion stays blocked.

## Alternatives

- **Treat any test/check/evaluator edit as forbidden.** Smallest policy
  surface, but blocks legitimate test repairs and cannot represent the
  independent-review case ADR-0018 anticipates.
- **Allow edits whenever the final tests pass.** Rejected: the evaluated
  agent could weaken its own approval evidence, violating INV-114/CV-9.
- **Rely only on an independent model review.** Rejected as a universal
  substitute: review can be required by policy, but it cannot replace
  structured test execution or deterministic changed-input measurement.
- **Chosen v1:** two trusted policy states with a fail-closed default and
  exact independent-review references for permitted edits; add human
  approval only when the Policy Engine requires it.
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

## Implementation boundary

TASK-0144 must show a concrete structured harness adapter satisfying the
Runner boundary and fail closed if no such source is available. Acceptance
of this ADR does not make the unmerged TASK-0125 implementation acceptable.

## References

- ADR-0018 — independent evaluation and open TestMutationPolicy schema.
- ADR-0030 — CV-1..CV-9, exact evidence and policy binding.
- ADR-0024 — governed provider execution seam.
- INV-009, INV-072, INV-092, INV-114, INV-145, INV-146..153.
- planning/tasks/queue.yaml — TASK-0124, TASK-0125, TASK-0143, TASK-0144.
