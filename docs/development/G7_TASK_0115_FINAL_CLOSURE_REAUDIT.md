# TASK-0115 — Final Independent G7 Closure Re-Audit

> Audit-only re-evaluation of G7 after TASK-0114. No production behavior was
> changed by this task.
>
> **Verdict: `G7_REMEDIATION_REQUIRED`.**

The four blockers from the original G7 closure audit are closed, the shipped
planning/continuation path is reachable, the V9 concurrent RESUME race is
fixed, the real trusted-host Claude planner produces a durable revision with
measured spend, and the production macOS Claude path now fails closed before
provider work when provider authentication cannot operate inside mandatory G6
confinement.

G7 still cannot close because accepted ADRs disagree about one public
`PlannerPort` operation. ADR-0027 says `assess_atomicity` is pure and performs
zero durable writes. ADR-0028 later requires durable reservation and settlement
for every `PlannerPort` invocation. The implementation follows ADR-0028 and
also publishes the assessment's `PlanningInputSnapshot` as the branch's current
input. This is fail-closed, but the contract conflict remains unresolved.

## 1. Audited identity

| Field | Value |
| --- | --- |
| Source main | `8054852735b6cb75b1f4db17728c9922f8b42132` |
| Audit branch | `task-0115-g7-final-closure-reaudit` |
| Audit worktree | `~/dev/runlith-task0115` |
| Host | macOS 27.0 (26A428), arm64 |
| Rust | `rustc 1.95.0` / `cargo 1.95.0` |
| Real provider | Claude Code 2.1.278 |

The technical audit was performed with a clean worktree. Audit/governance
files were written only after the technical verdict was fixed.

## 2. Authority and scope

The audit re-read the current G7 authority rather than accepting task outcomes
as proof:

- `docs/architecture/INVARIANTS.md`, especially INV-002, INV-009,
  INV-043..047, INV-050, INV-093 and INV-146..153;
- `docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md`;
- `docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md`;
- `docs/architecture/adr/ADR-0029-planner-evidence-resolution.md`;
- `planning/roadmap.yaml` G7;
- the original `docs/development/G7_CLOSURE_AUDIT.md`;
- actual implementation and executable tests on the audited tree.

This task is audit-only. A closure blocker discovered here is recorded for a
separate remediation task.

## 3. Original closure blockers

### G7-BLOCKER-01 — planner-authored node identity / cross-Task authority

**PASS.** Existing Task references are restricted to the product-derived
candidate set. The planner cannot capture an unrelated or foreign-project
Task, and a new child identity is validated/materialized at the trusted
storage boundary. Child identity availability, parent authority and project
ownership are rechecked in the publication transaction.

Fresh targeted evidence:

- `planner_cannot_capture_foreign_or_unrelated_canonical_tasks_before_publication` — PASS;
- full `planning_service` and `plan_persistence` suites — PASS;
- full workspace suite — PASS.

### G7-BLOCKER-02 — shipped planning/continuation path

**PASS.** Planning and continuation are reachable through the shipped
`agentctl -> agentd` control plane. Canonical Task authority is server-derived;
the client cannot mint planning authority by supplying policy/scope fields.
Execution admission is gated by a unique current accepted Plan and rechecks the
Plan head and current planning-input binding before Task/Execution mutation.

Fresh targeted evidence:

- `v5_planning_cli_publishes_an_accepted_plan_then_admits_execution` — PASS;
- `v6_authored_task_plans_and_executes_without_sqlite_authority_seeding` — PASS;
- `v5_planning_cli_applies_risk_resume_without_replaying_the_planner` — PASS;
- `agentd --test planning_integration` — 6/6 PASS.

### G7-BLOCKER-03 — HumanDecision stale/expired/failed recovery

**PASS.** Unusable PENDING/RESOLVED decisions are durably superseded rather
than erased. Supersession is audited, survives restart, and permits a later
canonical planning pass to create a replacement for the bounded owner scope.
ABORT remains owner-scoped; unrelated work is not globally wedged.

Fresh targeted evidence:

- `expired_risk_decision_is_superseded_once_and_replaced_after_restart` — PASS;
- `replacement_supersession_is_audited_for_pending_and_resolved_chain_and_survives_restart` — PASS in the full suite;
- `failed_replan_is_superseded_without_erasing_decision_evidence` — PASS in the full suite;
- `resume_is_restart_durable_once_only_and_reopens_only_owner_scope` — PASS in the full suite.

### G7-BLOCKER-04 — V9 RESUME satisfiability and exactly-once consumption

**PASS.** RESUME rehydrates the exact durable approved proposal and original
measured receipt; it accepts no `PlannerPort`, so a second provider call cannot
be required. Concurrent consumers join the identical approved replay run and
the publication boundary admits exactly one winner. Losing consumers receive
typed stale/consumed refusals. Stale input/head changes cannot consume or
publish the old grant.

Fresh targeted evidence:

- `risk_decision_survives_restart_and_resume_is_consumed_with_plan_commit` — PASS;
- `concurrent_v9_resume_publishes_and_consumes_exactly_once` — PASS;
- stale-snapshot, corruption and trailing-byte V9 cases — PASS in the full suite;
- `planning_service` — 59/59 PASS.

## 4. Remaining G7 contract and regression envelope

| Area | Result | Evidence |
| --- | --- | --- |
| Planner output remains proposal-only | PASS | `PlannerPort` has no mutation handle; malformed/adversarial output tests PASS |
| Plan revision durability/provenance | PASS | revision + graph + receipt + journal persistence tests PASS |
| Plan head/input/run publication fences | PASS | stale head, stale input, superseded generation and Task-drift tests PASS |
| Canonical Task authority | PASS | `planning_task_authority` 6/6 PASS |
| Child materialization | PASS | parent/project/identity/retry/rollback matrix PASS |
| L3/L4 guardrails | PASS | durable L3 and runnable-leaf L4 tests PASS |
| V3/V5/V9 non-weakening | PASS for modeled G7 obligations | plan validation suite 51/51 PASS; TASK-0096 lossy boundary refuses unsupported proof methods |
| RESUME / REPLAN / ABORT | PASS | restart, once-only, failed REPLAN and bounded ABORT tests PASS |
| Ambiguous current Plan | PASS | ambiguity is typed and produces zero execution mutation |
| Restart durability | PASS | planning, HumanDecision, execution and storage recovery suites PASS |
| G6 isolation regression | PASS | G6 integration, runner and Seatbelt suites PASS |
| Real trusted-host planner | PASS | live Claude call accepted a durable revision with measured spend |
| Production-isolated real Claude on macOS | PASS as fail-closed unsupported condition | explicit pre-provider `provider_authentication_unsupported_in_mandatory_confinement`; no sandbox widening |

ADR-0029's identity-only evidence semantics are an accepted G7 limitation,
not a hidden capability claim. Planner output quality is not treated as proof
of authority; trusted validation remains the safety boundary.

TASK-0112/TASK-0114 also close the previous ambiguity around provider auth.
The current Claude macOS credential mechanism requires login-Keychain access,
SecurityServer and exact `/usr/bin/security` execution. G6 deliberately does
not grant that authority. An authenticated real Claude CLI therefore refuses
before provider work with a stable typed cause. This is the intended
G6-preserving production result; trusted-host planner evidence is measured
separately.

## 5. Closure blocker — G7-BLOCKER-05 / F-07 contract drift

**Finding:** accepted architecture prescribes incompatible semantics for
`assess_atomicity`.

ADR-0027 currently says `AtomicityAssessment` is pure and “performs zero
durable writes.” ADR-0028 later requires durable reservation before every
`PlannerPort` invocation so planner spend and crash recovery cannot reset on
restart. `PlanningService::assess_atomicity` follows the latter rule:

1. it constructs a `PlanningInputSnapshot`;
2. `start_planning_run` persists the snapshot, publishes it as the branch's
   current planning input and advances the durable run generation;
3. `metered_call` reserves and settles durable call/token accounting;
4. only then is the assessment result returned.

The current behavior is fail-closed. Snapshot identity is deterministic, so an
assessment over identical authoritative inputs can reproduce the same current
snapshot. An assessment using different evidence can intentionally move the
branch's current planning input, making an older accepted Plan inadmissible
until the branch is reconciled. That matches ADR-0028's current-input fencing,
but it does not match ADR-0027's zero-durable-write statement.

This is not a replay, privilege-escalation or cross-Task authority hole. It is
still a closure blocker because the repository has two Accepted ADRs giving
incompatible requirements for the same public operation. Closing G7 would
force a future implementer to choose which accepted contract to violate.

**Required remediation:** one bounded architecture/remediation task must make
the contract coherent. It must explicitly choose and test one of these
semantics:

- permit durable assessment accounting/control state and define whether an
  assessment may advance the branch's current planning-input pointer; or
- retain a stronger pure-assessment semantic and separate assessment accounting
  from the publication input head so assessment cannot change Plan admission.

No implementation choice is made by TASK-0115.

## 6. Verification

Fresh verification on the audited tree:

- `cargo test --workspace --no-fail-fast` — GREEN, zero failures; only opt-in
  real-provider smokes ignored in the ordinary run;
- `cargo clippy --workspace --all-targets -- -D warnings` — GREEN;
- `cargo fmt --all --check` — GREEN;
- `git diff --check` — GREEN;
- queue YAML parse/uniqueness — 103 items / 103 unique before this audit record;
- targeted original-blocker tests — all GREEN;
- real trusted-host planner smoke — PASS, one live Claude call, durable plan
  revision, measured provider spend (`1845` input / `999` output tokens);
- production G6 real-Claude smoke — PASS by the expected explicit fail-closed
  `EXECUTION_FAILED` confinement-auth result before provider invocation.

An independent fresh-context read-only audit separately re-derived the four
original blocker closures and independently classified F-07 as a closure
blocker. Its verdict was also `G7_REMEDIATION_REQUIRED`.

## 7. Final verdict

`G7_REMEDIATION_REQUIRED`

TASK-0115 is complete as an audit task. G7 remains `IN_PROGRESS`. No G8 work is
authorized. The blocker is assigned to `TASK-0116`, a separate bounded
remediation task; another fresh-context closure re-audit follows that task.
