# G7 Planning Failure Matrix

> TASK-0080 verification evidence. Architecture authority remains with
> ADR-0027, ADR-0028, the repository invariants, and the Task/Policy/Runner
> contracts. This document maps the accepted G7 failure taxonomy to
> deterministic executable evidence; it introduces no new G7 semantics and
> does not close G7. TASK-0081 owns the independent closure audit.

## Canonical scope

TASK-0080 is verification-first. Its canonical queue objective is to exercise
cyclic proposals, scope escape, acceptance-criteria weakening,
policy/approval weakening, budget/depth/fan-out breaches, duplicate recursive
decomposition, planner failure mid-proposal, and adversarial proposal content
through FakePlanner plus recorded real-planner cases. ADR-0027 V1-V9 and L3/L4
define the planning failure semantics; ADR-0028 adds durable input/run fencing
and accepted-Plan execution admission.

No UI, Council, G8 verification architecture, G9 continuity, provider routing,
new planner authority, or broad refactor is part of this task.

## Gap analysis before TASK-0080

TASK-0076 already made V1-V9 executable in `plan-domain`. TASK-0077 already
proved atomic PlanRevision plus journal persistence and restart
reconstruction. TASK-0078 already covered production planning, durable spend,
stale planning state, accepted-Plan execution admission, L3/L4, and real
Claude-shaped provider failures. TASK-0079 already covered bounded durable
HumanDecisionRequest behavior and V9 continuation semantics.

The remaining verification gaps were narrow:

1. `FakePlannerScenario::GuardrailFanOut` and `FakePlannerScenario::StaleBase`
   existed but were not exercised through the trusted `PlanningService`
   publication boundary.
2. Pre-call depth exhaustion needed an explicit proof of zero planner
   invocation plus durable human escalation.
3. Adversarial free-form planner text needed an explicit proof that text is
   inert data and cannot grant Task/policy/approval/process authority.
4. Real-provider nonzero exit already proved conservative spend, but its
   planning-service test did not explicitly assert the absence of Plan state.
5. The new fan-out matrix exposed one implementation defect: a post-call V6
   limit refusal returned the correct typed `GuardrailTripped` outcome but
   bypassed durable L3 rejection accounting. ADR-0027 requires rejected
   proposals to count toward L3. TASK-0080 routes that V6 outcome through the
   existing durable rejection/Human Decision path while preserving the public
   guardrail outcome.

No other architecture defect was found by the matrix.

## Canonical TASK-0080 taxonomy

| Failure class | Expected fail-closed behavior | Deterministic evidence |
|---|---|---|
| Malformed planner output | no Plan publication; measured/unmeasured spend remains accounted | `planning_failure_matrix::fake_planner_matrix_refuses_malformed_fanout_and_stale_proposals_with_durable_evidence`; `planning_service::malformed_planner_output_after_measured_spend_keeps_receipt_and_charge`; Claude planner contract malformed-output test |
| Invalid/cyclic proposal | V1 `CYCLIC_PROPOSAL`; zero revision/event publication | `planning_service::malformed_and_invalid_proposals_refuse_before_persistence`; `validation_rules::v1_acyclicity_*` |
| Referential-integrity violation | V2 `DANGLING_REF`; no partial mutation | `planning_service::update_and_removal_of_same_node_is_typed_refusal_never_panic_or_persistence`; V2 validation-rule suite |
| Acceptance-criteria loss | V3 `CRITERION_DROPPED`; proof obligation preserved | `planning_service::removing_criterion_bearing_node_requires_derived_v3_coverage`; V3 validation-rule suite |
| Scope widening | V4 `SCOPE_ESCAPE`; no Plan mutation | `planning_service::malformed_and_invalid_proposals_refuse_before_persistence`; V4 validation-rule suite |
| Policy/approval weakening | V5/V9 refusal; raw/stale authorization cannot publish | `planning_service::policy_weakening_is_rejected_before_persistence`; `raw_risk_grant_without_durable_decision_binding_cannot_publish`; V5/V9 validation-rule suite |
| Per-call planner budget breach | typed guardrail; measured receipt retained; no revision | `planning_service::token_budget_exhaustion_refuses_and_never_persists`; `post_call_over_envelope_usage_is_charged_and_durably_receipted` |
| Cumulative planner budget breach | durable across service restart; provider not called after exhaustion | `planner_call_budget_is_hard_and_prevents_ninth_call`; `cumulative_token_budget_survives_restart_for_same_evidence`; `repeated_post_call_budget_excess_cannot_bypass_the_cumulative_limit` |
| Depth breach | checked before planner invocation; durable bounded HDR escalation | `planning_failure_matrix::depth_exhaustion_escalates_before_fake_planner_invocation_and_survives_restart`; Claude adapter `depth_and_budget_refusal_happen_before_provider_invocation` |
| Fan-out breach | V6 typed `fan-out` trip; no Plan mutation; rejection participates in durable L3 | `planning_failure_matrix::fake_planner_matrix_refuses_malformed_fanout_and_stale_proposals_with_durable_evidence`; V6 domain test |
| Branch-node / revisions-per-cycle breach | V6 typed limit, proposal rejected | `validation_rules::v6_budget_depth_fanout_limits_rejects_branch_node_breach`; `v6_budget_depth_fanout_limits_rejects_second_revision_in_cycle`; `planning_service::same_evidence_in_any_order_can_never_reset_guardrails` |
| Duplicate recursive decomposition | V7 lineage replay rejected after restart | `planning_service::v7_duplicate_decomposition_is_refused_from_durable_lineage_after_restart`; V7 validation-rule suite |
| Planner/provider crash/failure mid-proposal | explicit provider failure; conservative or measured spend retained; no Plan publication | `planning_service::provider_failure_consumes_conservative_budget_without_fabricating_usage_evidence`; `provider_failure_without_usage_is_charged_conservatively_without_receipt`; Claude planner contract provider-failure cases |
| Adversarial proposal content | untrusted free-form text stays inert; typed structural fields alone are validated; no Task authority granted | `planning_failure_matrix::adversarial_freeform_planner_text_is_inert_and_grants_no_authority` plus V3/V4/V5 structural rejection tests |
| L3 no progress | two same-branch/same-evidence rejections become durable `NoProgress` HDR; planner stops while gate is pending | existing `planning_service::l3_no_progress_survives_restart_for_same_evidence`; TASK-0080 fan-out regression additionally proves V6 rejections enter the same path |
| L4 no runnable leaf | no revision accepted; bounded PlannerBlocked HDR | `planning_service::structurally_valid_plan_without_runnable_task_trips_l4_before_commit` |

## Stale state, concurrency, and publication fencing

ADR-0027 V8 and ADR-0028 require publication to fail closed at the durable
write boundary, including races after provider invocation.

| Scenario | Evidence |
|---|---|
| stale proposal base | TASK-0080 FakePlanner stale scenario; `planning_service::stale_base_uses_canonical_stored_head_and_never_persists` |
| concurrent Plan head movement | `planning_service::concurrent_head_advance_refuses_stale_graph_even_if_proposal_claims_new_head`; storage `two_same_head_writers_have_exactly_one_winner` |
| superseded PlanningRun generation | storage `superseded_planning_run_generation_cannot_publish_a_revision` |
| stale planning input at execution admission | `planning_service::stale_planning_input_at_admission_with_unchanged_plan_head_rolls_back_everything` |
| stale Plan head at execution admission | `planning_service::stale_plan_head_at_admission_leaves_task_execution_and_attempt_unmodified` |
| persistence/journal half-commit | `planning_service::persistence_failure_leaves_no_partial_plan_state`; storage `journal_insert_failure_rolls_back_revision_head_and_graph` and `state_write_failure_appends_no_event` |
| restart durability | Plan persistence reopen tests, durable budget/L3 tests, and HDQ restart tests |
| V9 decision consumption + publication atomicity | storage `risk_decision_consumption_rolls_back_when_plan_commit_fails_then_consumes_on_retry`; application `risk_decision_survives_restart_and_resume_is_consumed_with_plan_commit` |

## Human Decision Queue failure surface

TASK-0079 evidence remains authoritative for the bounded continuation
lifecycle. TASK-0080 reuses it instead of duplicating HDQ implementation.

| Scenario | Evidence |
|---|---|
| missing decision / raw grant | `raw_risk_grant_without_durable_decision_binding_cannot_publish` |
| stale Task / Plan / snapshot | `human_decision_stale_fences_task_plan_snapshot_scope_and_risk_changes` |
| widened scope | same stale-decision test, `affected scope widened` case |
| increased risk | same stale-decision test, `risk increased` case |
| superseded / invalid option / unknown / duplicate consumed | `invalid_unknown_superseded_and_duplicate_human_decisions_fail_closed` |
| RESUME once-only across restart | `resume_is_restart_durable_once_only_and_reopens_only_owner_scope` |
| REPLAN through canonical G7 boundary | `replan_decision_uses_normal_g7_planning_boundary_and_is_once_only`; risk REPLAN test |
| ABORT bounded to owner | `pending_and_abort_block_only_owner_while_independent_task_enters` |
| independent Task continues while another waits | same bounded-blocking test |

## Execution admission and authority boundaries

Production execution remains an accepted-Plan consumer. Missing Plan, Task
outside the accepted Plan, stale Plan head, and stale PlanningInputSnapshot
all leave Task state, Execution, Attempt, and provider invocation untouched.
The evidence is in `production_entry_without_accepted_plan_*`,
`production_entry_for_task_outside_current_plan_*`, and the two stale-admission
tests. `schedule_from_current_plan` and `enter_from_current_plan` accept no
`PlannerPort`, which keeps the production execution admission path from
implicitly invoking planning.

Planner and provider remain proposal/execution adapters. The TASK-0080
adversarial-text test proves planner text cannot mutate Task lifecycle state or
execute host commands. The agentd production composition test proves a real
Claude-shaped planner crosses existing G6 Policy/Runner/Security Audit before
provider launch. TASK-0080 changes no Policy, Approval, Secrets, Runner,
Security Audit, or provider process authority contract.

## Remediation discovered by the matrix

The only production change is in `PlanningService` V6 refusal handling.
Previously, a proposal that had already consumed a planner call and then failed
V6 as fan-out/branch/depth/revision limit returned `GuardrailTripped` directly.
That skipped `record_plan_proposal_rejection`, so repeated invalid proposals
could avoid ADR-0027 L3 no-progress escalation.

The fix routes the same typed `GuardrailTripped` outcome through the existing
`record_rejection_with_human_intervention` path. First rejection behavior is
unchanged for callers; durable `consecutive_rejections` now advances, and the
second same-evidence rejection creates the existing bounded `NoProgress` HDR.
No new state, event type, migration, dependency, or authority was introduced.

## Verification record

Targeted verification completed during TASK-0080:

- `cargo test -p app-services --test planning_failure_matrix` — 3 passed.
- `cargo test -p app-services --test planning_service` — 39 passed.
- `cargo test -p plan-domain --test validation_rules` — 50 passed.
- `cargo test -p storage-sqlite --test plan_persistence` — 18 passed.
- `cargo test -p fake-planner --test planner_port_contract` — 1 passed.
- `cargo test -p provider-claude --test planner_port_contract` — 10 passed.
- `cargo test -p agentd --test planning_integration` — 5 passed.

On this host, invoking Cargo through the selected Xcode toolchain fails before
Rust linking because the Xcode license has not been accepted. All Rust
verification therefore uses the repository-authorized workaround
`DEVELOPER_DIR=/Library/Developer/CommandLineTools`.

Mandatory full-workspace verification completed green with the same
CommandLineTools selection:

- `cargo check --workspace --all-targets` — passed.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo test --workspace` — passed; the historical 50-round branch-only live
  ownership race passed, and the two authenticated real-Claude smoke tests
  remained intentionally ignored/opt-in.
- `cargo fmt --check` — passed.
- `git diff --check` — passed.

## Gate boundary

TASK-0080 supplies failure-matrix evidence only. A green matrix makes the
repository eligible for TASK-0081 independent G7 closure review; it does not
itself assert `G7_CLOSED`.
