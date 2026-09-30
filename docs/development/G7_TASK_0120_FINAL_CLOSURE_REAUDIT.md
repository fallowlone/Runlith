# TASK-0120 — Final Independent G7 Closure Re-Audit

> Audit-only re-evaluation of G7 after TASK-0119. No production behavior was
> changed by this task.
>
> **Verdict: `G7_CLOSED`.**

The four original blockers from TASK-0081 remain closed under fresh targeted
testing, the G7-BLOCKER-05 / F-07 contract conflict is resolved coherently by
the TASK-0116 amendment, and the TASK-0119 human-gate publication atomicity
holds under inspection. The proposal-path de-admission residue TASK-0119
disclosed is adjudicated here as contract-mandated ADR-0028 fencing with a
reachable, designed recovery — not a closure blocker. Build, lint, format and
the full workspace suite are green; the production G6 confinement smoke
re-passed fresh; the trusted-host live planner smoke was environmentally
blocked (external account limit, reproduced outside the product) and its
evidence is inherited from TASK-0115 against a byte-identical provider path,
as detailed in §7.

## 1. Audited identity

| Field | Value |
| --- | --- |
| Source main | `85bc607adc510f07dfc7e13f3fa1827cf3ac259f` (post TASK-0119 merge; TASK-0120 queueing commit) |
| Audit branch | `task-0120-g7-closure-reaudit` |
| Audit worktree | `~/dev/runlith/.claude/worktrees/task-0120-g7-closure-reaudit` (clean tree; `git diff --check` green) |
| Host | macOS 27.0 (26A428), arm64 |
| Rust | `rustc 1.95.0` / `cargo 1.95.0` |
| Toolchain workaround | `DEVELOPER_DIR=/Library/Developer/CommandLineTools` (Xcode license not accepted on host) |
| Real provider | Claude Code 2.1.278 |

The technical audit was performed on the clean worktree above. Governance
records (this report, queue outcome, version identity) were written only
after the technical verdict was fixed.

## 2. Authority and scope

The audit re-read the current G7 authority rather than accepting task
outcomes as proof:

- `AGENT.md` authority order and hard rules;
- `docs/architecture/INVARIANTS.md` (INV-002, INV-009, INV-043..047,
  INV-050, INV-093, INV-146..153);
- `docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md`
  including all amendments through TASK-0116;
- `docs/architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md`;
- `docs/architecture/adr/ADR-0029-planner-evidence-resolution.md`;
- `planning/roadmap.yaml` G7;
- the original `docs/development/G7_CLOSURE_AUDIT.md` (TASK-0081) and
  `docs/development/G7_TASK_0115_FINAL_CLOSURE_REAUDIT.md` (TASK-0115);
- actual implementation and executable tests on the audited tree.

This task is audit-only. TASK-0119 explicitly deferred one adjudication to
this audit: whether the disclosed proposal-path de-admission residue blocks
G7 closure (§5.2). The roadmap's G7 planned-capability prose
(`ActionProposal` lifecycle, multi-perspective topology, TAKEOVER) is not a
hidden gap: ADR-0027 §7's accepted vocabulary reconcile merged
`ActionProposal` into `PlanChangeSet` + `proposal_hash`, deferred
Council/multi-model deliberation post-G7, and the TASK-0097 amendment re-homed
TAKEOVER and full HandoffPacket rendering to G9 with roadmap updated to match.


## 3. Original closure blockers — re-verified, not inherited

Each blocker was re-tested on the audited tree by name, and the named tests
were individually confirmed green in the full workspace run (§7).

### G7-BLOCKER-01 — planner-authored node identity / cross-Task authority

**PASS.** Planner proposals may reference only the product-derived candidate
Task set (`trusted_candidate_task_refs`,
`crates/app-services/src/planning_service.rs:1074`); child identity,
parent authority and project ownership are revalidated at the trusted
publication boundary.

- `planner_cannot_capture_foreign_or_unrelated_canonical_tasks_before_publication` — PASS;
- `task_not_in_candidate_plan_is_refused_before_commit`,
  `plan_predecessor_without_owned_canonical_task_is_refused_before_commit` — PASS;
- `planning_task_authority` suite — PASS (full workspace run);
- `planning_service` suite — 62/62 PASS.

### G7-BLOCKER-02 — shipped planning/continuation path

**PASS.** Planning, continuation and accepted-Plan execution admission are
reachable through the shipped `agentctl -> agentd` control plane with
server-derived canonical Task authority.

- `v5_planning_cli_publishes_an_accepted_plan_then_admits_execution` — PASS;
- `v6_authored_task_plans_and_executes_without_sqlite_authority_seeding` — PASS;
- `v5_planning_cli_applies_risk_resume_without_replaying_the_planner` — PASS;
- `agentd --test planning_integration` — 6/6 PASS;
- `agentctl --test execution_binaries` — 10/10 PASS. Honest process note: the
  first standalone run of this suite in the fresh audit worktree failed 9/10
  in 0.12s because the suite drives the compiled `agentd`/`agentctl` binaries
  and they had not been built in that target directory yet; after a normal
  build the suite passed 10/10, and it had already passed inside the full
  workspace run. Not a product defect — recorded so the red run is not
  mistaken for one.

### G7-BLOCKER-03 — HumanDecision stale/expired/failed recovery

**PASS.** Unusable PENDING/RESOLVED decisions are durably superseded with
matching audit facts, supersession survives restart, and a later canonical
planning pass may create a replacement for the bounded owner scope; a failed
REPLAN application supersedes rather than erases.

- `expired_risk_decision_is_superseded_once_and_replaced_after_restart` — PASS;
- `replacement_supersession_is_audited_for_pending_and_resolved_chain_and_survives_restart` — PASS;
- `failed_replan_is_superseded_without_erasing_decision_evidence` — PASS;
- `replacement_audit_failure_rolls_back_supersession_and_new_request` — PASS;
- `resume_is_restart_durable_once_only_and_reopens_only_owner_scope` — PASS.

### G7-BLOCKER-04 — V9 RESUME satisfiability and exactly-once consumption

**PASS.** RESUME rehydrates the exact durable approved proposal with hash and
receipt re-verified, accepts no `PlannerPort` (no second provider call can be
required), joins rather than supersedes an identical in-flight replay run,
and consumes the grant atomically with the revision commit.

- `risk_decision_survives_restart_and_resume_is_consumed_with_plan_commit` — PASS;
- `concurrent_v9_resume_publishes_and_consumes_exactly_once` — PASS;
- `planning_service` — 62/62 PASS, including the stale/corrupt V9 cases.

## 4. G7-BLOCKER-05 / F-07 — assessment durability contract conflict

**PASS.** The TASK-0115 blocker was that ADR-0027 §1 said `assess_atomicity`
"performs zero durable writes" while ADR-0028 §4 mandates durable spend
reservation before every `PlannerPort` invocation, and the implementation
additionally republished the branch's current planning input.

Contract coherence, verified in the text: ADR-0027 §1's clause now carries an
in-place "Corrected by the TASK-0116 assessment durability amendment"
blockquote (`ADR-0027:80-90`), and the amendment (`ADR-0027:983-1080`)
prescribes exactly one model: assessment may persist its own immutable
snapshot, advance the run generation and reserve/settle its own spend
(ADR-0028 §3/§4 apply unchanged); it must not republish the branch's current
planning input except through a human gate; it still writes no `Plan`. Nothing
in ADR-0028 contradicts any of that. A future implementer no longer has to
choose which accepted contract to violate.

Implementation coherence, verified in code:

- `ControlRunMode` is an explicit four-state value
  (`planning_service.rs:101-112`): `Proposal` publishes current input at
  claim; `ApprovedReplay` is fenced against its decision's plan head;
  `Assessment` and `HumanGate` publish nothing at claim.
- The `planning_input_heads` write in `start_planning_run_inner` is gated by
  `claim.publishes_current_input()` (`planning_control_store.rs:430-443`), so
  an assessment claim cannot move the branch pointer.
- Assessment spend is still reserved before the planner call and settled
  after it (`metered_call`, `planning_service.rs:1295+`), so the ADR-0028 §4
  crash-safety property that motivated the conflict is intact.

The amendment's named coverage exists, targets the claimed seams, and is
non-vacuous (each test fails against the pre-remediation behavior described
in the amendment):

- `assessment_planner_fault_opens_a_human_gate_rather_than_failing` — PASS;
- `atomicity_assessment_does_not_de_admit_the_accepted_revision` — PASS;
- `assessment_run_reserves_a_generation_without_replacing_current_input` — PASS;
- `publishing_planning_input_from_a_superseded_run_is_refused` — PASS,
  asserting specifically `StorageError::PlanningRunStale` from the generation
  fence rather than a bare error;
- `atomicity_assessment_shares_the_bounded_recorded_call_budget` — PASS
  (pre-existing spend pin).


## 5. TASK-0119 — human-gate publication atomicity

### 5.1 The fix holds

`SqliteStore::publish_planning_input_and_create_human_decision`
(`planning_control_store.rs:337-352`) opens one `BEGIN IMMEDIATE` transaction
covering the run-generation fence, the `planning_input_heads` upsert and the
decision insert; both writes commit together or neither happens. The pre-call
gate claims its generation as `PlanningRunClaim::HumanGate`, which publishes
nothing (`planning_service.rs:1190-1198`), then publishes with its decision
through the fused writer (`open_human_gate`, `:1218-1237`); the post-call
assessment gate delegates to the same helper (`:1012-1028`). A failed gate
creation still finishes the run rather than leaving it ACTIVE
(`:1201-1206`). The proposal path's separate `create_planning_human_decision`
(`:1245-1258`) deliberately does not republish, because its run claim already
published the snapshot — that is the one shape where a separate decision
write is correct, since `require_plan_precondition`
(`human_decision_store.rs`) refuses unless the decision's bound snapshot is
the current published head. The standalone unfused `publish_planning_input`
is deleted; the in-transaction writer is private with one caller, so the
fused writer is the only way to publish outside a run claim.

- `a_failed_gate_decision_leaves_the_accepted_revision_admissible` — PASS
  (storage-level atomicity: no orphaned publication);
- `the_pre_call_gate_publishes_the_snapshot_its_decision_is_bound_to` — PASS;
- `publishing_planning_input_from_a_superseded_run_is_refused` — PASS on the
  retargeted fused writer.

### 5.2 Adjudication of the disclosed proposal-path residue

TASK-0119 disclosed, without fixing, that a proposal run publishes its
snapshot when it claims its generation, so an ordinary refusal — a validation
refusal (`record_rejection_with_human_intervention`,
`planning_service.rs:1933-1956`) or a guardrail trip whose bound maps to no
human reason (`human_reason_for_guardrail`, `:2267-2276`) — leaves the
branch's current input moved, the previously accepted revision de-admitted,
and no decision recorded. The closure audit owns the verdict on whether this
blocks G7. It does not, for three independently verified reasons:

1. **The de-admission is the contract, not a violation of it.** ADR-0028 §2
   makes snapshot freshness explicit and fail-closed: publishing a newer
   product-owned snapshot supersedes the previous pointer, and §5 requires
   the accepted revision to be bound to the branch's *current* planning-input
   snapshot before any Task mutation. A revision bound to superseded input
   *must* stop admitting execution — that is the fence doing its designed
   work, identical in kind to the stale-head/stale-input admission refusals
   the gate was audited for at TASK-0081. No accepted invariant or ADR
   sentence requires an accepted Plan to remain admissible after the branch's
   planning input legitimately moved.
2. **The state is not a wedge.** Unlike G7-BLOCKER-03 (no return path),
   nothing blocks recovery: `start_planning_run_inner`
   (`planning_control_store.rs:386+`) imposes no precondition that a prior
   refused run or a moved input violates — a new run supersedes the old
   generation and may publish a new revision bound to the then-current input,
   and the whole path is reachable through the shipped control plane (§3
   G7-BLOCKER-02 evidence). Repetition is bounded by design: durable L3
   no-progress counting (`record_rejection`, `planning_service.rs:1916-1931`;
   `l3_no_progress_survives_restart_for_same_evidence` — PASS) escalates
   repeated same-evidence refusals into a bounded NoProgress human gate with
   REPLAN/ABORT options.
3. **The window the residue resembles is closed where it mattered.** The
   TASK-0119 defect was a crash window that stranded the pointer with no
   record of *why*; the residue is a recorded, typed refusal outcome
   (run finished `Refused`, rejection durably counted), so the branch's state
   is explainable from the journal and the recovery operation is the ordinary

## 6. Remaining G7 contract and regression envelope

| Area | Result | Evidence |
| --- | --- | --- |
| Task != Execution != Attempt != Session | PASS | no planner/provider field on Task; G7 history re-checked |
| Planner output remains proposal-only | PASS | `PlannerPort` returns owned data; malformed/adversarial suites PASS |
| V1-V9 order, first refusal wins | PASS | `plan-domain` validation suites in the full run |
| Plan revision durability/provenance, journal atomicity | PASS | `plan_persistence` 32/32 |
| Plan head/input/run publication fences | PASS | stale head, stale input, superseded generation and Task-drift tests PASS |
| Canonical Task authority | PASS | `planning_task_authority` suite PASS |
| Child materialization | PASS | parent/project/identity/retry/rollback matrix PASS |
| L3/L4 guardrails, durable across restart | PASS | `l3_no_progress_survives_restart_for_same_evidence`, L4 runnable-leaf tests |
| V9 grant binding, once-only consumption | PASS | §3 G7-BLOCKER-04 evidence |
| RESUME / REPLAN / ABORT lifecycle | PASS | restart, once-only, failed-REPLAN and bounded-ABORT suites PASS |
| Ambiguous current Plan | PASS | `production_entry_with_multiple_legitimate_current_plans_is_ambiguous_and_non_mutating` |
| Production admission never plans | PASS | `schedule_from_current_plan` / `enter_from_current_plan` accept no `PlannerPort`; missing/stale/ambiguous admission tests PASS |
| Restart durability | PASS | planning, HumanDecision, execution and storage recovery suites PASS |
| G6 isolation regression | PASS | `g6_security_integration`, runner and policy suites PASS in the full run; §7 confinement smoke fresh PASS |
| IPC/agentctl reachability | PASS | §3 G7-BLOCKER-02 evidence |
| Malformed/adversarial planner output | PASS | `planning_failure_matrix` suite, adversarial-text inertness test |
| Real trusted-host planner | PASS (inherited) | §7.2 — path byte-identical to TASK-0115's live PASS; fresh re-run blocked by external account limit |
| Production-isolated real Claude on macOS | PASS (fresh) | §7.2 — fail-closed confinement-auth refusal before provider work |

ADR-0029's identity-only evidence semantics remain an accepted G7
limitation; planner output quality is not treated as proof of authority, and
trusted validation remains the safety boundary.

   one.

Recorded as an accepted residual limitation, not hidden: execution admission
for the previously accepted revision is unavailable from a refused proposal
until a new revision is accepted, and no single test today pins the full
refusal-to-re-acceptance flow end to end (its components are each pinned).
Any future change to this semantics must amend ADR-0028 rather than restate
it in prose.

### 5.3 Convention-enforcement caveat, carried forward

`PlanningRunClaim::HumanGate` prevents a future gate from reopening the
two-transaction window by convention, not by type-level enforcement: a gate
written in main's old shape (claim via the still-public `start_planning_run`,
decision recorded separately) would reopen it, and no current test would
catch that exact shape. This was disclosed by TASK-0119 itself; the partial
revert (HumanGate claim + separate decision write) fails closed on
`require_plan_precondition`. Accepted as documented residual risk; a
follow-up may make the claim mode unrepresentable for new gates.


## 7. Verification

### 7.1 Deterministic verification (fresh, on the audited tree)

- `cargo fmt --all --check` — GREEN;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — GREEN,
  zero diagnostics;
- `cargo test --workspace --locked --no-fail-fast` — GREEN, **1104 passed /
  0 failed / 3 ignored**; the 3 ignored are the opt-in real-provider smokes;
- `git diff --check` — GREEN (clean tree);
- queue integrity — 109 items, 109 unique ids; the `product-version` gate
  guard (`gate_matches_planning_queue`, unquoted-mapping-colon scan) runs
  inside the workspace suite and is green;
- targeted suites re-run standalone on the audited tree:
  `app-services --test planning_service` 62/62,
  `storage-sqlite --test plan_persistence` 32/32,
  `agentd --test planning_integration` 6/6,
  `agentctl --test execution_binaries` 10/10 (see the process note in §3);
- every test named in §§3-5 was individually confirmed green by name in the
  full-run log, not inferred from suite totals.

### 7.2 Real-provider evidence

- **Production G6 confinement path — fresh PASS.**
  `RUNLITH_CLAUDE_SMOKE=1 cargo test -p agentd --lib
  real_claude_refuses_before_provider_work_under_production_g6_confinement --
  --ignored`: the production macOS Claude path refuses with the typed
  `EXECUTION_FAILED` confinement-auth result before any provider invocation.
  Notably this passed *because* the refusal precedes provider work, which is
  exactly the G6-preserving property under test.
- **Trusted-host live planner — BLOCKED by environment, evidence inherited.**
  `RUNLITH_CLAUDE_SMOKE=1 cargo test -p app-services --test
  planner_real_provider_smoke -- --ignored` failed with
  `claude exited with code Some(1)` and empty stderr, zero tokens, ~2.7s.
  Root cause is external account state, not product behavior: a bare
  `claude -p "Reply with exactly: OK"` outside any product code prints
  "You've hit your weekly limit · resets Sep 24 at 4pm (Europe/Berlin)" on
  this host. Per the audit rule of exposing blocked verification rather than
  converting it into success: no fresh live-planner receipt exists for this
  run. The evidence stands from TASK-0115 (one live Claude call, durable plan
  revision, measured spend 1845 input / 999 output tokens) because the
  provider path is byte-identical: `git log 8054852..85bc607 --
  adapters/provider-claude bins/agentd/src/planning.rs` is empty. The
  deterministic planner contract suites (`fake-planner` and `provider-claude`
  `planner_port_contract`, process fixtures) are green in the full run, and
  the failed call's spend behaved per contract: the CLI emitted a
  provider-reported zero-usage envelope, so the adapter returned
  `Measured { 0, 0 }` rather than fabricating or dropping usage
  (`adapters/provider-claude/src/planner.rs:179-198`, `:344-354`).
- **Non-blocking observation (test tooling, not product):** the smoke's
  `ENVIRONMENT_MARKERS` classifier did not recognize the account-limit shape
  because the CLI exits 1 with empty stderr in the adapter's invocation
  shape, so the failure reported as "non-environment" when the cause was
  environmental. A future tooling task may add this signature; no production
  code is involved. The trusted-host smoke MUST be re-run green before any
  release decision that relies on fresh live-planner evidence.

## 8. Independent second pass

An independent fresh-context read-only re-derivation (no access to this
report's conclusions, instructed to distrust completion records) re-read the
authority set and the implementation and returned **`G7_CLOSED`**
independently: it re-derived all five blocker closures with file:line
evidence, confirmed ADR-0027/ADR-0028 coherence post-amendment, confirmed the
named coverage is non-vacuous, and judged the §5.2 residue contract-mandated
fencing with designed recovery. Its adversarial sweep of
`planning_service.rs`, `planning_control_store.rs`, `human_decision_store.rs`,
`plan_store.rs`, `entry_store.rs` admission and the `agentd` seams found no
new closure-blocking defect. Its two non-blocking observations are adopted
here: (a) a CONSUMED+ABORT decision is terminal for its scope by deliberate
design — no accepted contract requires post-ABORT replanning of the same
scope; (b) the §5.3 convention-enforcement caveat.

## 9. Final verdict

`G7_CLOSED`.

All five G7 blockers are remediated and re-proven on the audited tree; the
accepted contract set is internally coherent; the disclosed residual risks
(§5.2, §5.3, §7.2 tooling observation) are recorded rather than hidden and
none violates an accepted ADR or invariant. This task changes no production
behavior, starts no G8 work, and could not re-prove the trusted-host live
planner for the environmental reason stated in §7.2; that evidence is
inherited from TASK-0115 against a byte-identical provider path. Gate
bookkeeping: G7 is marked completed in `planning/roadmap.yaml`,
`gate_status: CLOSED` in `planning/tasks/queue.yaml`, and product identity
advances to TASK-0120 with the gate field unchanged at G7 until the G8
breakdown is materialized.

