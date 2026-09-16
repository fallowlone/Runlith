# G7 Closure Audit (TASK-0081)

> Independent closure audit of Gate G7 (bounded adaptive planning, durable
> planning state, Human Decision Queue, accepted-Plan execution admission).
> Audit only: no repairs, no refactors, no later-gate work. Auditor-owned
> probes ran from an external scratch crate that depends on the workspace
> crates by path; they are not part of this change. Implementation reports,
> the TASK-0080 matrix and queue outcomes were treated as claims to verify.

## 0. Verdict

**`G7_REMEDIATION_REQUIRED`.**

Build, lint and format are green. The full workspace suite passed once in
three runs; both red runs are the known daemon-orphan test-harness race
(F-09), not product behavior. Most of the ADR-0027/ADR-0028 mechanics hold
under independent inspection: V1-V9 run in
ADR order, publication is fenced by head CAS, input snapshot and PlanningRun
generation inside one `BEGIN IMMEDIATE` transaction, spend is reserved before
the provider call and settled durably, and G6 Policy/Runner/Security Audit
still wrap every provider launch. G7 does not close because four material
defects were reproduced (§5):

| ID | Severity | Summary |
|---|---|---|
| G7-BLOCKER-01 | High | Planner-chosen node ids bind to any canonical Task, including other projects' Tasks: an untrusted proposal can block unrelated execution or make an unrelated Task admissible. |
| G7-BLOCKER-02 | High | The shipped control plane cannot plan or apply human continuations, so shipped `RunExecution` always refuses (G5 execution regression). |
| G7-BLOCKER-03 | High | A stale, expired or failed-continuation HumanDecisionRequest has no return path; its Task/Plan scope stays wedged with no way out. |
| G7-BLOCKER-04 | High | V9 RESUME can never publish with a planner that measures cost per call, and one failed RESUME attempt wedges the scope. |

## 1. Repository identity and audit environment

| Field | Value |
|---|---|
| Worktree | `/Users/artemmac/dev/runlith-wt-0081` |
| Branch | `task-0081-g7-closure-audit` |
| Audited base | `655ab03b07bef59e55b3ef31b939b13596d0897d` (main, TASK-0080; clean tree) |
| Host | macOS 27.0 arm64, rustc/cargo 1.95.0 |
| Toolchain workaround | `DEVELOPER_DIR=/Library/Developer/CommandLineTools` (Xcode license not accepted on host) |
| Main worktree user changes | `planning/roadmap.yaml` (uncommitted UI spike note) and one untracked PNG: not read into scope, not touched |

## 2. Canonical G7 contract (rehydrated)

Sources: `AGENT.md`, `docs/architecture/INVARIANTS.md` (INV-002, 009, 043-047,
050, 093, 146-153), ADR-0016, ADR-0017, ADR-0027, ADR-0028, SoT §21.1 and
§105.3/105.5, `docs/architecture/task-engine.md` §10/§15,
`docs/security/POLICY_CONTRACT.md`, roadmap G7 + `DOGFOOD_DETACHMENT`, queue
TASK-0075..0081.

TASK-0081 itself: audit only, no repairs, no G8; exit criterion is a closure
report with an explicit overall status.

Required properties used as the audit baseline:

1. Planner is a proposal engine behind `PlannerPort`. Its output is untrusted,
   and only validated services mutate state (INV-043, INV-093). Decomposition
   cannot widen project/task scope or weaken approvals (INV-046, SoT §21.1.1
   "child scopes stay inside authorized project/task scope").
2. V1-V9 run in order, first refusal wins, nothing is partially applied, and
   rejections count toward L3 (ADR-0027 §3). Bounds: depth 4, fan-out 8,
   64 nodes/branch, 1 revision per evidence cycle, L3 at 2, L4 runnable leaf,
   §6 call envelope plus 8 calls / 256k tokens per evidence event (ADR-0027 §5-6).
3. A PlanRevision commits atomically with its journal fact, with provenance
   (ADR-0027 §4). PlanningInputSnapshot, PlanningRun and PlanRevision stay
   distinct. Publication is fenced by head, input snapshot and run generation
   inside the write transaction, and spend is reserved, then settled
   (ADR-0028 §1-4).
4. Production `RunExecution` never calls a planner and requires an accepted,
   current, unambiguous Plan that represents the Task, with head and input
   rechecked inside admission (ADR-0028 §5-6). "Initial planning is a
   separate control-plane operation."
5. The HumanDecisionRequest is durable, bounded, provider-neutral, bound to
   exact Task/Plan/snapshot/policy state, and once-only. RESUME/REPLAN/ABORT
   are applied with control-plane enforcement (TASK-0079). Blocking stays
   minimal (SoT: "unrelated Tasks/Executions continue"), and there is a
   "durable return path into the same workflow" (SoT §105.5).
6. V9: a risk-escalated revision persists only with a bound, unexpired,
   single-use grant over the exact `proposal_hash`, consumed atomically with
   publication (ADR-0027 V9, ADR-0027 security impact).
7. Every core capability has a CLI-invocable, contract-tested path (ADR-0016).
   Capabilities provide dogfood value (roadmap `DOGFOOD_DETACHMENT`).
8. G5/G6 boundaries (Policy, approvals, Secrets, Runner/Seatbelt, H7
   environment, Security Audit, trusted-host Git) are not weakened.

## 3. Evidence reviewed

- **Contracts:** the documents listed in §2, plus `ARCHITECTURE.md` (Planning /
  Orchestration context), `ORCHESTRATOR.md`, `docs/development/VERSIONING.md`,
  and prior closure audits for report conventions.
- **Production code, read line by line:** `crates/plan` (all modules),
  `crates/planner-contract`, `adapters/fake-planner`,
  `adapters/provider-claude/src/planner.rs`, `adapters/storage-sqlite`
  (`plan_store`, `planning_control_store`, `plan_guardrail_store`,
  `plan_cost_store`, `human_decision_store`, `entry_store` admission,
  migrations 0012-0015), `crates/app-services` (`planning_service`,
  `planned_orchestration`, `plan_graph`, `human_decision_service`),
  `bins/agentd` (`planning.rs`, `human_decision.rs`, `execution.rs` launch and
  admission paths), the IPC protos and negotiation, and `bins/agentctl` usage.
- **Tests:** `validation_rules`, `plan_persistence`, `plan_guardrails`,
  `plan_cost_receipts`, `planning_service`, `planning_failure_matrix`, both
  `planner_port_contract` suites, `planning_integration`, and the binary suites
  that seed plans.
- **History:** every G7 commit (`db6b312`, `7694215`, `328fd90`, `265cf26`,
  `d6c09b4`, `7e9273f`, `87917ac`, `030b8d0`, `655ab03`). Test names present
  during G7 history but absent at HEAD are all renames (schema/protocol
  version bumps). No coverage was deleted. G5/G6-sensitive paths touched by
  G7 are limited to the `agentd` provider-launch refactor and the
  `ClaudeExecutor` observed-usage path (§7).

## 4. Executive matrix

| Area | Result | Evidence |
|---|---|---|
| Task != Execution != Attempt != Session | PASS | No planner/provider field on Task. Planner launches audit with no Execution/Attempt ids (`planning_integration.rs:548-562`). |
| Planner proposal-only (no direct writes) | PASS | `PlannerPort` returns owned data. All writes go through `PlanningService` and the storage transactions. |
| Planner authority over Task identity/scope | **FAIL** | G7-BLOCKER-01 |
| V1-V9 order and typed refusal | PASS (with observations F-03, F-05, F-06) | `validation.rs:221-330`; 50 domain tests |
| PlanRevision + journal atomicity, provenance | PASS | `plan_store.rs:223-402`; persistence and rollback tests |
| Input/run fencing, stale publication refusal | PASS | `planning_control_store.rs:433-482` inside the commit transaction; `superseded_planning_run_generation_cannot_publish_a_revision` |
| Durable spend, per-call and cumulative budgets, restart | PASS | `planning_service.rs:898-965`, `plan_guardrail_store.rs:55-130`; budget/L3 restart tests |
| Repeated refusals/failures bypassing L3/budget | PASS | Provider failures consume call slots (9th call refused). V6 refusals enter L3 (TASK-0080 fix). Evidence order cannot reset counters. |
| Production RunExecution accepted-Plan gated, no planner | PASS | `planned_orchestration.rs:290-416`; head/input rechecked in `entry_store.rs:149-197` |
| Planning and continuations reachable from the shipped control plane | **FAIL** | G7-BLOCKER-02 |
| HDQ durability, binding, once-only, stale refusal | PASS | `human_decision_store.rs`; TASK-0079 tests |
| HDQ bounded wait with a return path | **FAIL** | G7-BLOCKER-03 |
| V9 grant binding and atomic consumption | PASS (binding) / **FAIL** (satisfiability) | `human_decision_store.rs:376-525`; G7-BLOCKER-04 |
| REPLAN via the normal G7 boundary, no replay | PASS (no replay) / **FAIL** (failure wedges) | `human_decision_service.rs:175-248`; probe G |
| ABORT bounded to the owner | PASS | `pending_and_abort_block_only_owner_while_independent_task_enters` |
| Independent Task progress while another waits | PASS in isolation; see BLOCKER-01/03 for cross-scope effects | same test; probes D, E |
| G5/G6 boundaries not weakened | PASS | §7 |
| Failure-taxonomy coverage | PARTIAL | §8 |
| Full verification | PARTIAL: static gates green; workspace tests green 1/3 (harness race F-09) | §9 |

## 5. Blockers

### G7-BLOCKER-01 — Planner-authored node identity captures Tasks outside the parent scope

- **Severity:** High (authority/scope).
- **Violated:** INV-043, INV-046, INV-093; SoT §21.1.1 ("child scopes stay
  inside authorized project/task scope"); ADR-0028 §5-6 (the admitting Plan
  must *represent* the Task); ADR-0027 V4 intent; minimal blocking.
- **Code path:** Plan node ids are canonical Task ids
  (`crates/app-services/src/plan_graph.rs:6-7`), chosen by the planner
  (`adapters/provider-claude/src/planner.rs:474-487`). Validation constrains
  node identity only against the base graph of the *same branch*
  (`crates/plan/src/validation.rs:400-409`). V4 checks path scopes only
  (`validation.rs:517-549`). L4 and applicability accept any existing Task id
  (`plan_graph.rs:211-265`). Admission resolves membership by node id across
  every branch and project
  (`adapters/storage-sqlite/src/planning_control_store.rs:381-407`,
  `planning_service.rs:376-400`).
- **Reproduction (probe E):**
  1. Projects P-A, P-B and P-C each hold one READY Task (TASK-A, TASK-B, TASK-C).
  2. TASK-B gets a legitimate single-leaf plan on `branch-b`.
  3. `enter_from_current_plan(TASK-C)` returns `NoAcceptedPlan`.
  4. With policy parent TASK-A, a planner proposes node `TASK-B` on
     `branch-a1` and node `TASK-C` on `branch-a2`. Both are `Accepted rev 1`.
- **Expected:** refuse node ids outside the parent's authorized Task scope, or
  already represented by another current Plan, before commit.
- **Actual:** `enter_from_current_plan(TASK-B)` returns
  `AmbiguousAcceptedPlan { branch-a1, branch-b }`: TASK-B's execution is
  blocked by another project's planner. `enter_from_current_plan(TASK-C)`
  returns `Entered` via `branch-a2`, and TASK-C moves READY→RUNNING under a Plan
  validated only against TASK-A's scope and criteria.
- **Security/authority impact:** untrusted planner output decides execution
  admission for Tasks it has no authority over (fail-open for the G7 gate), and
  can deny service to independent Tasks. It needs no provider bug; a
  hallucinated common id is enough. No test covers `AmbiguousAcceptedPlan`.
- **Related gap:** planning never materializes child Tasks (no `create_task`
  caller in planning code). The planner prompt carries no existing Task ids
  (`planner.rs:673-705`). New decomposed work is therefore L4-refused
  (probe C), unless the planner happens to name a pre-existing READY Task.
  That coincidence is exactly the capture path above.
- **Minimal remediation:** bind node identity to trusted Task scope at the
  validation/commit boundary. Either materialize accepted child nodes as
  Tasks through `TaskService` under the parent's project, or give the planner
  a trusted candidate set and enforce it. Refuse ids owned by other projects,
  outside the parent subtree, or already represented in another current
  accepted Plan, with a typed refusal. Add deterministic capture, ambiguity and
  admission tests.
- **Why it blocks closure:** the proposal-only boundary and minimal-blocking
  requirement are canonical G7 invariants. Here they fail under untrusted input.

### G7-BLOCKER-02 — G7 is unreachable from the shipped control plane; shipped execution always refuses

- **Severity:** High (canonical exit criteria / production regression).
- **Violated:** ADR-0016 ("Every core capability needs a CLI-invocable,
  contract-tested path"); ADR-0028 §5 ("Initial planning is a separate
  control-plane operation", and none exists); TASK-0078 exit ("Real planner
  decomposes a fixture project through the production path"); TASK-0079
  objective (RESUME/REPLAN/ABORT "with control-plane enforcement"); roadmap
  `DOGFOOD_DETACHMENT` (P1).
- **Code path:** `ControlRequest` has no planning or continuation message
  (`crates/ipc-contracts/proto/agentd_control.proto:20-35`). The HDQ wire can
  only list/get/decide (`bins/agentd/src/human_decision.rs:1-3,22-71`).
  `agentctl` has no planning verb (`bins/agentctl/src/main.rs:40-58`).
  `ExecutionComposition::planning_service` / `secured_claude_planner`
  (`bins/agentd/src/planning.rs:129-184`) and `HumanDecisionService::apply_local`
  / `risk_grant` / `apply_replan` have no production caller. `fake-planner` is
  a dev-dependency only. `start_run` refuses without an accepted Plan
  (`bins/agentd/src/execution.rs:1047-1113`). Every binary-level and in-crate
  execution test seeds plans out-of-band by opening the daemon's SQLite file
  with the library and `FakePlanner`
  (`bins/agentctl/tests/execution_binaries.rs:171`,
  `bins/agentctl/tests/execution_recovery.rs:284`,
  `bins/agentd/tests/live_ownership.rs:134`,
  `bins/agentd/tests/execution_recovery.rs:137`,
  `bins/agentd/src/execution.rs:2039`).
- **Reproduction (shipped binaries):** start `agentd` with fixture `claude` on
  PATH, then run `agentctl project create P1 …`, `task create T1 P1 …`,
  `task ready T1`, `execution run T1 <git repo> --detach`. The result is
  `error STALE_STATE: task has no current accepted plan; complete planning
  before execution` (exit 1). The Task stays READY, `human-decision list` is
  empty, and no command can plan.
- **Expected:** a contract-tested daemon/CLI path that creates accepted Plans
  (`PlanningService` + daemon-composed `SecuredClaudePlanner`, product-derived
  policy context), plus a path that applies human continuations.
- **Actual:** G7 planning, guardrail observability and continuation
  application exist only as library calls. The G5 execution capability is
  unusable in shipped binaries.
- **Security/authority impact:** none weakened (fail closed). Availability of
  the product's core flow is lost.
- **Minimal remediation:** add a minimal protocol-gated IPC plus `agentctl`
  surface for (a) planning a Task through `PlanningService` with daemon-owned
  policy context and the secured planner, and (b) applying RESUME/REPLAN/ABORT
  through the existing services. Add a binary test running
  `task → plan → decide/apply → execution run` without DB seeding.
- **Why it blocks closure:** closing G7 would hand G8 and the UI milestone a
  planning capability with no control-plane contract to consume, and a
  regressed execution path.

### G7-BLOCKER-03 — A stale, expired or failed HumanDecisionRequest wedges its scope permanently

- **Severity:** High (durable correctness, Human Decision semantics).
- **Violated:** SoT §105.5 ("durable return path into the same workflow");
  INV-050 (the escalation becomes unanswerable); TASK-0079 exit ("Blocked scope
  *waits* while unrelated work continues").
- **Code path:** `latest_human_decision_for_scope` keeps returning the newest
  PENDING/RESOLVED row forever (`human_decision_store.rs:164-187`).
  `planning_human_gate` (`planning_service.rs:775-817`) and admission
  `human_gate` (`planned_orchestration.rs:521-538`) propagate
  `validate_human_decision_current` errors instead of waiting or superseding.
  `resolve_human_decision` and `consume_human_decision` validate currency first
  (`human_decision_store.rs:204,273`). The only supersession is inside
  `create_human_decision_request` (`human_decision_store.rs:53-60`), which is
  reached only after the failing gate, or through
  `HumanDecisionService::create`, which has no production caller. Every
  planning attempt republishes the branch input snapshot
  (`planning_control_store.rs:204-231`). The TTL is 30 minutes
  (`planning_service.rs:57`). Decision creation in tests uses
  `expires_at_ms: None` (`crates/app-services/tests/planning_service.rs:598`).
- **Reproduction:**
  - **Probe C (expiry):** a planning L4 refusal creates an HDR expiring at
    T0+30 min. At T0+30 min+1 ms, `decide` returns `HumanDecisionStale
    "decision expired"`, `list_pending` still lists it, and planning with
    fresh evidence errors before any planner call. Seven days later it is
    unchanged.
  - **Probe D (unrelated revision):** TASK-X has a pending HDR on a shared
    branch, and parent TASK-Y then commits a revision there.
    `enter_from_current_plan(TASK-X)` returns `Err(HumanDecisionStale "Plan
    head moved")` permanently, and decide and planning for TASK-X also error.
    TASK-Y still enters, so the bound holds for others.
  - **Probe G (REPLAN failure):** REPLAN is decided, then `apply_replan` hits a
    provider failure. A retry with a healthy planner returns
    `HumanDecisionStale "PlanningInputSnapshot changed"`, fresh planning
    errors, and the decision stays RESOLVED forever. A crash between
    publication and `mark_human_decision_consumed_after_application` leaves
    the same state.
  - **Probe B:** also shows the wedge after a failed RESUME attempt
    (BLOCKER-04).
- **Expected:** bounded waiting that ends in an answerable or re-creatable
  decision, or an explicit terminal state that stops gating and lets trusted
  planning escalate again.
- **Actual:** the owner scope is refused with a storage error indefinitely,
  and nothing short of direct DB edits recovers it.
- **Security/authority impact:** fail-closed. There is no bypass, but a
  durable denial of service follows ordinary operator latency (over 30 min)
  or a transient provider failure.
- **Minimal remediation:** define HDQ exits for EXPIRED and STALE. Either
  transition to a terminal state with a timestamp that no longer gates, or
  let trusted planning supersede a non-current decision by creating a fresh
  bounded request. Make continuation failures leave a re-creatable decision.
  Test with the production TTL, head/snapshot movement and failure injection.
- **Why it blocks closure:** the HDQ exists to make blocked planning
  recoverable. Here its escalation is a one-way trap.

### G7-BLOCKER-04 — V9 RESUME is unsatisfiable with a measured-receipt planner

- **Severity:** High (V9 continuation semantics).
- **Violated:** ADR-0027 V9 (a granted escalation must be publishable); the
  TASK-0079 RESUME continuation; the audit binding Human Decision → exact
  proposal → Plan publication.
- **Code path:** `proposal_hash` covers `planning_cost_receipt` input/output
  tokens, `wall_time_ms` and price class (`crates/plan/src/hash.rs:223-234`).
  `ClaudePlanner` measures wall time and provider usage per call
  (`adapters/provider-claude/src/planner.rs:178-196`). The RiskEscalation HDR
  stores only the hash (`planning_service.rs:1180-1211`), not the approved
  changeset. RESUME therefore re-invokes the planner through
  `propose_decomposition` / `propose_replan`, and V9 compares the new hash
  (`validation.rs:653-667`). The commit re-checks the same hash
  (`human_decision_store.rs:431`). The repository RESUME test uses a fixed
  receipt, `small_receipt()` 100/50/5 ms
  (`crates/app-services/tests/planning_service.rs:276,2146-2227`), so the test
  shares the defect.
- **Reproduction:**
  - **Probe A:** a structurally identical risk proposal whose second call
    measures 1001 ms instead of 1000 ms. The HDR hash is `sha256:72e2…f0b2`, the
    RESUME re-proposal hash is `sha256:8171…808f`, and the outcome is
    `Refused(RiskGateOpen "risk grant does not reference the exact
    proposal_hash")` with no revision. The control run (fixed receipt) is
    Accepted and consumes the decision.
  - **Probe B:** a transient provider failure during the RESUME attempt,
    then a retry with the byte-identical approved proposal, returns
    `HumanDecisionStale "PlanningInputSnapshot changed"` before any planner
    call. `risk_grant` and re-decide also error, so the approval can never be
    used (BLOCKER-03).
- **Expected:** after human RESUME, the exact approved proposal publishes
  once and atomically.
- **Actual:** with the production planner, a risk-escalated proposal can
  never be published. Each attempt either refuses or wedges the scope.
- **Security/authority impact:** fail-closed. No bypass: raw grants and
  mismatched hashes are refused (`raw_risk_grant_without_durable_decision_binding_cannot_publish`,
  consumption binding at `human_decision_store.rs:376-525`). The defect is
  functional.
- **Minimal remediation:** persist the approved canonical changeset (or its
  exact canonical bytes) with the RiskEscalation decision, and publish it on
  RESUME without re-invoking the planner, under the existing fences. Or, by
  ADR amendment, bind V9 to material proposal content while keeping receipt
  provenance durable. Test with per-call varying receipts and a failed
  attempt.
- **Why it blocks closure:** V9 is a mandatory ADR-0027 rule, and its human
  continuation must work with the production planner.

## 6. Findings (non-blocking unless noted)

- **F-01 (Medium) — HDR decisions leave no audit evidence.** Creation,
  resolution, consumption and supersession of HumanDecisionRequests append no
  Security Audit and no Event Journal record (probe H: audit 0→0, journal
  5→5 across a RESUME decision). `decided_by` is a constant
  (`bins/agentd/src/human_decision.rs:16`), and supersession carries no
  timestamp. G6 approvals, by contrast, audit atomically
  (`decide_approval_with_audit`). V9 risk approval is "approval decided" and
  "privilege elevation" in the audit vocabulary (`POLICY_CONTRACT.md:325-329`).
- **F-02 (Medium) — Governance drift.** The queue has no `outcome` for
  TASK-0078 (removed in `87917ac`) or TASK-0079 (never added). The product
  version skipped `task0079`. No committed acceptance review exists for
  TASK-0078 round 3 or TASK-0079. `VERSIONING.md` still lists IPC `0, 1, 2`
  (v3 and v4 exist). `G7_FAILURE_MATRIX.md` claims "No other architecture
  defect was found", which is contradicted by §5.
- **F-03 (Low) — V5 allows downgrade plus gate removal.** V5 accepts lowering
  an existing gated node's risk and clearing its `required_gate` (probe F:
  `Accepted { risk_escalated: false }`). `risk_class` and `required_gate` have
  no consumer outside planning validation and persistence, and `GateRef` is
  free text, so the annotations are inert in G7.
- **F-04 (Low) — Misleading dependency test.** `plan_domain_depends_only_on_sha2`
  (`crates/plan/tests/validation_rules.rs:1083`) asserts nothing about
  dependencies. `plan-domain` now also depends on `task-domain` (TASK-0079),
  and the TASK-0076 outcome's claim that a test pins the dependency set is
  false. `task-domain` itself has no dependencies.
- **F-05 (Low) — Unvalidated planner evidence refs.** Planner-authored
  `source_evidence_refs` flow unvalidated into `PlanRevision.caused_by`, the
  journal `causation_id`, V7 fingerprints and HDR evidence. `ClaudePlanner`
  injects trusted request refs (`planner.rs:302,317`), so production is safe
  today, but neither the `PlannerPort` contract nor the service enforces it.
- **F-06 (Low) — Depth measures dependency chains.** `MAX_DECOMPOSITION_DEPTH`
  counts the longest dependency-edge chain (`plan_graph.rs:81-124`), not
  hierarchical decomposition depth. Sequential plans longer than 5 steps are
  refused, while nesting is not represented. Recursion is bounded instead by
  the 64-node branch limit and 1 revision per evidence event.
- **F-07 (Low) — "Pure" assessment has admission side effects.** A successful
  `assess_atomicity` (zero journal rows) republishes the branch input snapshot.
  The branch's accepted Plan then becomes non-admissible
  (`Err(PlanningInputStale)`), and re-planning with the original evidence trips
  `revisions-per-cycle` (probe I). This fails closed, but it contradicts
  ADR-0027 "assessment is pure". `PlannedExecutionEntryOutcome::PlanningInputMoved`
  is unreachable on this path because the resolver errors first
  (`planning_service.rs:353-363`).
- **F-08 (Low) — Test integrity.** No test covers `AmbiguousAcceptedPlan`,
  HDR expiry, stale-decision recovery or measured-receipt RESUME. The
  "recorded real-planner cases" are hand-written shell fixtures, and no
  real-planner smoke exists, not even opt-in. The storage test
  `risk_decision_consumption_rolls_back_when_plan_commit_fails_then_consumes_on_retry`
  retries with the same run/snapshot, which the application flow cannot do
  (probe B). The adversarial-text marker `$(touch …)` has no shell sink, so
  that assertion is tautological.
- **F-09 (Medium, closure prerequisite) — Test harness leaks daemons;
  hermeticity fix unmerged.** The TEST-H hermeticity fix
  (`hardening/test-h-agentd-hermeticity`, `c4e0eb6`) is not merged, and the
  race reproduced during this audit, so the workspace gate is not reliably
  green.
  - Mechanism: binary tests call `spawn_agentd()` and then `wait_ready()`,
    which pings in a loop (`agentctl ping`). `ping` goes through
    `connect_or_start_with` (`bins/agentctl/src/lib.rs:844-877`), which
    spawns a detached sibling `agentd` when the child is not yet listening.
    `stop_daemon` reaps only the child, so the sibling is orphaned (PPID 1).
    `g1_real_process_matrix` then counts every process with that binary path
    (`bins/agentctl/tests/real_process_matrix.rs:143`), so an orphan fails it.
  - Occurrence, run 2 (audit commit): `g1_real_process_matrix` failed with
    `left: 4, right: 1`. The orphan sandboxes were `human-decision-cli` (a
    TASK-0079 test), `wait` and `liveown-runresume`, all started in the same
    second, plus the matrix's own daemon.
  - Occurrence, run 3: an orphan from `liveown-race-3` kept a killed
    daemon's execution live, which failed
    `binary_concurrent_resumes_elect_a_single_owner` ("strand is honest …
    live=true", `execution_binaries.rs:389`) and then
    `g1_real_process_matrix` (`left: 2`).
  - Isolation: the `agentctl` package passed 7/7 with zero orphans,
    `live_ownership` passed 2/2, and the first full run passed. Orphans were
    killed with a path-scoped `pkill` after each run.
  - Impact: this is a harness defect, not product behavior. The TASK-0079
    test adds one more instance of the racy pattern.

## 7. Security boundary audit (G5/G6 regression)

- The provider-launch refactor (`d6c09b4`) extracted
  `build_provider_launch_security_for_context` and `prepare_provider_launch`
  (`bins/agentd/src/execution.rs:378-616`). The executor keeps ReadWrite
  workspace authority. The planner gets ReadOnly plus the process and
  `tcp:443` network scopes. Both still run G6 `authorize` (ApprovalRequired
  and Denied fail closed), then Runner `prepare_process_launch` and
  `enforce_process_operation` with H7 `ChildEnvironment::from_explicit` on
  every call.
- `SecuredClaudePlanner` refuses a request whose task differs from the
  composition-bound Task (`planning.rs:44-48`). It charges boundary refusals as
  `NotStarted` and post-invocation boundary errors as `Unmeasured`.
- The Seatbelt profile stays `(deny default)`, allowing outbound only to
  `tcp *:443` and mDNSResponder, with exec restricted to listed paths. A
  sandboxed provider cannot reach the agentd socket or run `agentctl`, so
  there is no self-approval path for HDR or approvals.
- HDQ IPC is protocol-v4 gated (`ipc-control/src/negotiation.rs`).
  Policy/Secrets/Security Audit/git-cli code is unchanged by G7 commits.
- Human Decisions cannot authorize provider launch. Policy is re-evaluated at
  the launch boundary regardless of Plan or HDR state.

Result: no G5/G6 weakening found. Security-relevant G7 gaps are BLOCKER-01
(admission authority) and F-01 (audit evidence).

## 8. Failure-matrix audit (TASK-0080 claims vs. evidence)

| Class | Contract | Production path | Deterministic test | Forbidden-mutation assertion | Assessment |
|---|---|---|---|---|---|
| Malformed output | ADR-0027 §2 | `validate_proposal` pre-check; Claude parse → `ProviderFailure(Measured)` | matrix + service + Claude contract | head/lineage/journal absent | Adequate |
| Cyclic / dangling / criterion / scope | V1-V4 | `validation.rs` | domain + service | no persistence | Adequate at domain level; scope does not cover Task identity (BLOCKER-01) |
| Policy weakening | V5/V9 | `validation.rs` | domain + service | no persistence | Presence-only check (F-03) |
| Budget per call / cumulative / calls | §6, ADR-0028 §4 | `metered_call`, guardrail store | restart, 9th-call and post-call tests | receipt + no revision | Adequate |
| Depth / fan-out / nodes / revisions | V6, §5 | `prepare_proposal`, `check_limits` | domain + matrix | no mutation, L3 counted | Adequate (F-06 semantics) |
| Duplicate decomposition | V7 | durable lineage | restart test | refusal | Adequate (F-05) |
| Provider crash mid-proposal | §1 fault, ADR-0028 §4 | reservation → settlement | conservative-charge tests | no plan | Adequate; crash *inside* a continuation is not covered (BLOCKER-03/04) |
| Adversarial content | INV-093 | typed fields only | free-text test | Task unchanged | **Insufficient:** structural identity capture untested and exploitable (BLOCKER-01) |
| L3 / L4 | §5 | durable counters; `has_runnable_node` | restart + L4 tests | no revision | Adequate for triggering; HDR escalation not answerable after expiry (BLOCKER-03) |
| Stale head / input / run / concurrent writers | V8, ADR-0028 §2-3 | commit fence + CAS | storage + service + admission tests | Task/Execution/Attempt unchanged | Adequate |
| HDQ stale / widened / risk / superseded / duplicate | TASK-0079 | `validate_record_current` | lifecycle tests | refusal | Refusal adequate; post-refusal recovery missing (BLOCKER-03) |
| RESUME / REPLAN / ABORT | TASK-0079 | library services only | service tests | owner-bounded | Deterministic-receipt RESUME only (BLOCKER-04); not control-plane reachable (BLOCKER-02) |
| Ambiguous accepted Plan | ADR-0028 §6 | resolver | **none** | — | Missing |

## 9. Adversarial probes and verification

Auditor probes ran from an external crate
(`g7probe/tests/g7_probes.rs`, SHA-256 `bc9aeb89…66c8`) against the workspace
crates at `655ab03`. It covers nine tests (A-I), all green, meaning every
asserted defect or behavior reproduced. A separate shipped-binary probe covered
BLOCKER-02. Probes on the adversarial checklist that found no defect:

- stale snapshot with unchanged head
- concurrent publication
- stale decision after head, risk or scope movement
- proposal mutation after approval (hash mismatch refused)
- decision replay (once-only)
- reordered duplicate evidence
- planner authority text
- provider environment
- malformed or missing usage
- over-budget usage after invocation
- partial persistence failure
- journal/state disagreement
- admission racing planning movement (head and input rechecked in the
  admission transaction)

Workspace verification (in this worktree, with `DEVELOPER_DIR` set):

| Command | Result |
|---|---|
| `cargo check --workspace --all-targets` | pass, 0 warnings (base and audit commit) |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass (base and audit commit) |
| `cargo fmt --check` | pass (base and audit commit) |
| `git diff --check` | pass |
| `cargo test --workspace --no-fail-fast`, run 1 (base `655ab03`) | pass: 118 suites, 1022 passed, 0 failed, 2 ignored |
| same, run 2 (audit commit; docs/queue/version only) | 1021 passed, **1 failed** (`g1_real_process_matrix`, orphan daemons, F-09) |
| same, run 3 (audit commit) | 1020 passed, **2 failed** (`binary_concurrent_resumes_elect_a_single_owner`, `g1_real_process_matrix`, orphan daemons, F-09) |
| `cargo test -p agentctl`, isolated ×7 | 42 passed each, 0 orphans |
| `cargo test -p agentd --test live_ownership` ×2 | 5 passed each (50-round race included) |
| `cargo test -p product-version` (after version bump) | 4 passed |

The two ignored tests are the opt-in real-Claude executor smokes
(`RUNLITH_CLAUDE_SMOKE=1`, canonical opt-in per TASK-0046). They were not run.
No real-planner smoke exists.

## 10. Minimal remediation scope (not implemented)

One bounded remediation task, then an independent G7 closure re-audit:

1. **BLOCKER-01:** trusted binding of plan-node identity to the parent's Task
   scope, and child Task materialization or a trusted candidate set, with
   typed refusals and capture/ambiguity/admission tests.
2. **BLOCKER-02:** a minimal protocol-gated IPC + `agentctl` planning
   operation and continuation application through the existing services,
   plus a binary end-to-end test with no out-of-band seeding.
3. **BLOCKER-03:** HDQ EXPIRED/STALE exits and supersession by trusted
   planning, with continuation-failure recovery; tests use the production TTL.
4. **BLOCKER-04:** a durable approved changeset published on RESUME without
   re-invoking the planner (or an ADR amendment of the V9 binding), with
   measured-receipt tests.
5. **Verification gate:** land or port the TEST-H hermeticity fix, so no
   binary test can leave an orphan sibling daemon or count host-global
   processes (F-09). Demonstrate repeated green workspace runs.
6. **Fold-ins:** HDR Security Audit evidence (F-01), queue/version
   reconciliation for TASK-0078/0079 (F-02), and the F-08 test gaps.

No G8, G9, UI, Council or provider work is required.

## 11. Remaining uncertainty

- No authenticated real planner was run. Real-LLM node-id and L4 behavior is
  inferred from the prompt and validation code, not observed.
- Interpretations used: ADR-0016 applies to G7 planning and continuations;
  L3/L4 "BLOCKED" means the HDR planning gate rather than Task lifecycle
  BLOCKED (INV-047); the depth metric follows the literal "root → deepest
  leaf" reading.
- Windows was not compiled or tested in this audit.
- The admission transaction rechecks head and input but not the HDR gate.
  Planning-created HDRs are still fenced, because each one republishes the
  input snapshot. `HumanDecisionService::create`, which has no production
  caller, is not fenced.
- The daemon-orphan harness race (F-09) is timing dependent. Its observed
  rate here is based on a small sample (§9).

## 12. Canonical state

The queue records the TASK-0081 outcome (`completed_through: TASK-0081`), and
G7 stays `IN_PROGRESS`. `product/version.toml` advances to
`0.1.0-dev.task0081` per AGENT.md §26. The roadmap is unchanged. Nothing is
merged to `main`.
