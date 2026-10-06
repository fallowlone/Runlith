# ADR-0041 Ratification Review Pack (TASK-0125 phase c)

> Independent architecture/security review input. Status of ADR-0041 stays
> **Proposed** until the owner ratifies after a PASS on this pack. Exact
> commits under review: implementation `509f810`, ADR draft `768fa31`
> (branch `task-0125-resuscitation`). No new commits are in scope.

## 1. What is claimed

- The sole production minter of `AgentClaim` / `ClaimVerification` rows
  is `mint_candidate_verdicts`
  (`crates/app-services/src/candidate_verdict.rs:37`), called only from
  the `Verified` branch of the shared concluding boundary
  (`crates/app-services/src/orchestration_service.rs:754`), after
  freshness revalidation and before completion admission.
- The operator dispatch path (`bins/agentd/src/dispatch.rs:190-204`)
  only attempts admission and never mints.
- Admission authority is unchanged
  (`crates/app-services/src/task_service.rs:378-419`).

## 2. Reviewer checklist

- [ ] **Single minter.** No production caller of
      `append_claim_verification` / `append_agent_claim` except the
      mint module: `rg -n "append_claim_verification|append_agent_claim"
      crates bins adapters -g '*.rs'` shows only the module plus test
      fixtures.
- [ ] **No synthesized measurement.** The mint reads effects via
      `list_observed_effects_for_attempt`
      (`adapters/storage-sqlite/src/observed_effect_store.rs:92`) and
      returns `Ok(empty)` when none bind; capture owns all writes.
- [ ] **CV-2 structural.** Asserter `candidate:{execution}:{attempt}`
      can never equal `TRUSTED_VERIFIER_IDENTITY`
      (`crates/evaluation/src/claim_verification.rs:42`); trusted or
      empty asserter is rejected before any insert.
- [ ] **CV-8 preserved.** `HUMAN_APPROVAL` criteria are skipped by the
      mint; discharge path untouched.
- [ ] **INV-009 / INV-113.** Builder claims and provider loops still
      produce evidence, never transitions; completion still requires
      admission over durable rows.
- [ ] **No migration surface.** No schema change, no journal shape
      change, no IPC change; one read-only query added.
- [ ] **Retry convergence.** Already-`VERIFIED` criteria are skipped,
      so a retried cycle mints only the missing remainder.
- [ ] **Gates green on the exact commits:** `cargo fmt --all --check`;
      `cargo clippy --workspace --all-targets --locked -- -D warnings`;
      `cargo test --workspace --locked --no-fail-fast` EXIT=0, 127
      suites ok (`/tmp/task-0125c-full.log` is local-only evidence and
      is not committed).

## 3. Focused regression tests (committed)

- `crates/app-services/tests/completion_admission.rs`:
  `cv_candidate_mint_binds_current_effects_and_admits_completion`,
  `cv_candidate_mint_without_effects_mints_nothing_and_admission_refuses`.
- `crates/app-services/tests/orchestration.rs`:
  `required_task_without_capture_effects_cannot_complete_and_mints_nothing`.

## 4. For the owner

- A PASS here authorizes a follow-up commit flipping the ADR-0041
  status line to:
  `Accepted (TASK-0125; owner ratified <date> after independent
  architecture/security review PASS of exact commits 509f810/768fa31)`.
- Known limitation carried over from phases a–b (not in this ADR's
  scope): `admit_completion` does not re-read inputs inside the
  transaction; strict rule-5 remains a separate task.
