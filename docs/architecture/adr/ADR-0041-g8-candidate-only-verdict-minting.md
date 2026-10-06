# ADR-0041 — G8 Candidate-Only Verdict Minting Authority

Status: **Proposed** (TASK-0125 phase c; implementation `509f810`, workspace verification green, owner ratification pending)
Gate: G8

> This decision states who is authorized to mint durable `AgentClaim` /
> `ClaimVerification` rows, when that minting may happen, and what it may
> bind — so that the TASK-0125 completion gate has a lawful production
> issuance path. It grants no new completion authority: admission stays
> solely with `verification_succeeded_under_policy` (ADR-0030 CV-1..CV-9
> as implemented). It does not amend the text of any accepted ADR; it
> extends ADR-0018/ADR-0030's completion boundary with a named minting
> authority.

## Context

After TASK-0125 phases a–b, completion admission reads durable state only
and the operator-supplied verdict string is journal context, never
authority. That closed the gate but left issuance unowned: no shipped
control-plane path minted `ClaimVerification` rows.

Evidence (worktree `task-0125-resuscitation`, commit `4e015f1`):

- `adapters/storage-sqlite/src/claim_verification_store.rs:200` —
  `append_claim_verification` and `append_agent_claim` (`:103`),
  `append_approval_discharge` (`:472`) existed with zero production
  callers; every call site was a test fixture.
- `bins/agentd/src/dispatch.rs:190-204` — the operator
  `VerificationSucceeded` path calls
  `verification_succeeded_under_policy` with the request reason as
  journal text. It mints nothing.
- `crates/app-services/src/orchestration_service.rs:753-782` — the
  candidate path called the same admission entry with a fixed
  `"VERIFIED via independent evaluation"` string, but the evaluator's
  in-memory `EvaluationVerdict::Verified` was never persisted. With
  empty durable tables admission refused, so even the lawful candidate
  path could never complete a task carrying evidence requirements.
- `crates/evaluation/src/claim_verification.rs:995-1002` — a `VERIFIED`
  verdict counts only with a nonempty basis resolving to current-subject
  effects, so any mint must bind capture measurement, never invent it.

So the chain was broken at its origin: the gate demanded durable
verdicts no shipped path was allowed to produce.

## Decision

1. **Authority.** The sole production minter of `AgentClaim` /
   `ClaimVerification` rows is `mint_candidate_verdicts`
   (`crates/app-services/src/candidate_verdict.rs:37`), called only
   from the shared orchestration concluding boundary
   (`orchestration_service.rs:754`, the `Verified` branch of `finish()`),
   after freshness revalidation and before admission. `finish()` is the
   funnel for `schedule`, `run_workspace_attempt`, and (through it)
   `continue_workspace_cycle`, so one hook covers the daemon background
   half without touching `bins/agentd/src/execution.rs`.
2. **Binding.** The mint never synthesizes measurement. It binds
   verdicts exclusively to observed effects the capture pipeline already
   recorded for the evaluated attempt
   (`list_observed_effects_for_attempt`,
   `observed_effect_store.rs:92`), under the current trusted subject
   fingerprint, with the canonical `requirement_revision` and the
   requirement's own policy echo. Claim asserter is
   `candidate:{execution_id}:{attempt_id}`, never the trusted verifier
   identity (`TRUSTED_VERIFIER_IDENTITY`,
   `claim_verification.rs:42`); CV-2 holds structurally.
3. **Fail-closed vacuum.** With no bound evidence the mint succeeds
   vacuously with zero rows. Missing measurement is never an error
   here; the subsequent admission refusal is the loud signal, owned by
   admission alone. Storage failures and corrupt lineage propagate as
   errors and the task stays `VERIFYING`.
4. **Scope limits.** `HUMAN_APPROVAL` criteria are never minted; they
   discharge only via durable approval (CV-8). Criteria already holding
   a current `VERIFIED` verdict are skipped, so a retried cycle
   converges instead of stacking duplicates. The operator dispatch path
   never calls the mint — it only attempts admission.

## Consequences

- `schedule` without capture-pipeline effects still refuses required
  tasks and mints nothing (pinned by
  `required_task_without_capture_effects_cannot_complete_and_mints_nothing`);
  zero-requirement tasks complete exactly as before (mint returns
  empty, admission sees empty criteria).
- The daemon path with capture measurement can now complete required
  tasks through the lawful chain: capture → independent evaluation →
  mint → admission.
- Every minted row is audit data with full provenance (asserter,
  basis refs, revision, policy echo); supersession history is
  untouched (mint never supersedes; CV-5 chains remain append-only).

## Migration impact

None. No schema change (one read-only query added); no journal shape
change; no IPC change. Existing flows without attempt-bound effects
behave exactly as before (admission refuses as it did). Discharge and
exemption paths are untouched.

## Security considerations

- INV-009 / INV-113 preserved: builder self-claims and provider-local
  loops still produce evidence, never transitions — the mint runs in
  the trusted control plane after independent evaluation, keyed to a
  candidate identity distinct from the verifier.
- No operator-supplied string reaches any minted field: asserter is
  composed from durable execution/attempt identities, basis from
  store-read effect ids, revision/policy from the task's own
  requirements.
- A trusted-identity or empty asserter is rejected loudly at the mint
  boundary (fail closed, no partial rows: all preconditions validate
  before the first insert).

## Rejected alternatives (ADR-level)

- **Mint in dispatch on operator word.** Rejected: operator prose is
  not measurement; it would make completion depend on an untrusted
  string and break INV-009/INV-113.
- **Mint synthesizes its own effect rows.** Rejected: fabricated
  measurement bypasses the capture boundary (TASK-0124/0144
  ownership); a verdict minted on a self-made effect is authority
  without observation.
- **Manual verdict seeding as the shipped path.** Rejected: direct
  store writes are not a control-plane path and cannot run in
  production.

## References

- ADR-0018 (independent evaluation), ADR-0030 (CV-1..CV-9 contract),
  ADR-0032 (rules 4–5: revision/policy/basis binding)
- `crates/app-services/tests/completion_admission.rs`:
  `cv_candidate_mint_binds_current_effects_and_admits_completion`,
  `cv_candidate_mint_without_effects_mints_nothing_and_admission_refuses`
- TASK-0125 (queue `planning/tasks/queue.yaml`), TASK-0144 (governed
  evidence producer, owns candidate integrity assessments this mint
  does not fabricate)
