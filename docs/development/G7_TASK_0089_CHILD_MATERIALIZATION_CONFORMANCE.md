# TASK-0089 — child materialization conformance verification

**Verdict: CONFORMANCE_REMEDIATION_REQUIRED — TASK-0087 not accepted.**

2026-09-17. Worktree `~/dev/runlith-wt-0089`, branch
`task-0089-child-materialization-conformance`, base/HEAD
`f0eeb72` (TASK-0088 amendment accepted and merged). Read-only with respect to
production code: this pass adds only this report and the TASK-0089 queue entry.
No production source, accepted ADR, version or roadmap change; no merge.

## Scope

Verifies the merged TASK-0087 implementation against every clause of the
ADR-0027 amendment accepted by TASK-0088 (`ADR-0027`:497-570). It does not
re-litigate the owner decision itself, and it does not re-audit G7 closure.

## Independence limitation — read this first

The TASK-0088 amendment text was authored by this same orchestrator session
from the clause proposed in `G7_TASK_0087_R1_VERIFICATION.md`. A conformance
pass run by the author of the contract is not independent in the sense the
project's review workflow normally requires. The findings below are stated with
source lines so a genuinely independent reviewer can confirm or reject them, and
the acceptance decision should not rest on this document alone.

## Findings

### F-01 — exact retry does not enforce creation-before-readiness ordering (MEDIUM)

**Evidence.** `adapters/storage-sqlite/src/plan_store.rs`:1128-1177.
`is_idempotent_child_retry` selects `created[0]` and `ready[0]` from the
type-filtered lists and validates type, Task/project scope, from/to states,
provenance, causation and envelope. It never compares `JournalEvent.seq`, which
`adapters/storage-sqlite/src/event.rs`:391-392 defines as journal retrieval
order and `journal_store.rs`:44 returns via `ORDER BY seq`. The ordering data is
present and unused.

**Failure scenario.** A journal in which `task.marked_ready` carries a lower
`seq` than `task.created` for the same child — reachable through direct database
manipulation or a future writer that mints the pair out of order — is accepted
as a valid exact retry. The contract requires refusal.

**Broken clause.** ADR-0027 TASK-0088 amendment, "Exact retry equivalence":
"a valid unique `CREATED`/`READY` pair with creation ordered before readiness".

**Remediation.** Compare `created.seq < ready.seq` inside the pair validation and
add a `ready-before-created` row to the malformed-evidence matrix in
`adapters/storage-sqlite/tests/plan_persistence.rs`:1548.

**Acceptance blocker:** yes.

### F-02 — extra lifecycle facts conflict only when they duplicate the pair (MEDIUM)

**Evidence.** `adapters/storage-sqlite/src/plan_store.rs`:1146-1151 counts only
`EventType::TaskCreated` and `EventType::MarkedReady` and rejects a count other
than exactly one of each. Any other journal fact bound to the same `task_id` is
filtered out and never considered. `adapters/storage-sqlite/src/event.rs`:35-53
defines 18 event types; `execution.created`, `attempt.created`,
`workspace.created` and `workspace.removed` change no Task field.

**Failure scenario.** A materialized child accumulates a non-state-changing fact
(for example `execution.created`), then the same accepted proposal is
republished. `existing.snapshot() != child.task.snapshot()` does not fire,
because the snapshot (`crates/task/src/task.rs`:337-353) carries no journal
facts, so the retry is recognized as exact. The contract says this is a
conflict.

**Broken clause.** ADR-0027 TASK-0088 amendment: "Additional lifecycle facts
beyond the required pair are a conflict, not a match."

**Mitigation that limits severity.** Any fact that alters Task fields or state
does break snapshot equality and is caught, so this is narrower than it first
appears, and reuse writes no duplicate facts either way. The gap is contract
strictness, not observed corruption.

**Remediation.** Either require the child's journal to hold exactly the two
facts, or name in the ADR the fact types that may coexist with a retryable
child. Add a matrix row for a foreign-type fact on the child.

**Acceptance blocker:** yes — the clause as accepted is unambiguous.

### F-03 — parent proof obligations default to the weakest method instead of failing closed (MEDIUM)

**Evidence.** `crates/plan/src/task_authority.rs`:56-60 assigns every derived
parent criterion `VerificationMethod::AutomatedTest`, with a comment stating
that G7 v1 has no criterion-to-proof-method field in the Task contract.
`crates/plan/src/risk.rs`:50-55 orders the enum `AutomatedTest`,
`IndependentReview`, `SecurityCheck`, `HumanApproval`; automated test is the
weakest and no strength comparison function exists anywhere in the crate.
`crates/plan/src/validation.rs`:507-508 only flags `method_changed` when a child
method differs from the derived parent method, routing that to V9.

**Failure scenario.** A parent whose acceptance genuinely demands human approval
is derived as demanding an automated test. A child declaring
`automated-test` then produces `method_changed == false`, so V9 escalation never
triggers and the stronger obligation is silently lost.

**Broken clause.** ADR-0027 TASK-0088 amendment: "Obligations that cannot be
represented on the child block materialization. Defaulting to a weaker
obligation is forbidden (INV-046 non-weakening)."

**Mitigation that limits severity.** The default is applied symmetrically to
parent and child, and the Task contract v1 stores no proof method at all, so no
stronger obligation is representable anywhere in the system today. The loss is
of an obligation the durable model never carried, not of one it dropped.

**Remediation.** Choose one and record it: add a criterion proof-method field to
the Task contract and fail closed when it cannot be represented, or amend
ADR-0027 with an explicit, time-boxed v1 exception naming the default.

**Acceptance blocker:** yes — as written, the clause and the code disagree.

## Observations (non-blocking)

- **O-01.** The clause "pending children count only ... without incomplete Task
  dependencies" holds by construction, not by check.
  `crates/app-services/src/planning_service.rs`:1691-1701 builds children with
  `Task::draft` and never sets `depends_on`, and
  `crates/app-services/src/plan_graph.rs`:264-280 verifies only plan
  predecessors for pending ids. If children ever gain dependencies, L4 will
  silently count unrunnable work.
- **O-02.** Child objectives are the placeholder
  `format!("planned child task {}", id)`
  (`planning_service.rs`:1691-1694). Proposal-supplied intent is not carried
  onto the canonical Task.

## Clause acceptance table

| Clause | Source evidence | Result |
| --- | --- | --- |
| DRAFT -> READY only after V1-V9 validation | `planning_service.rs`:1565,1703-1732; `validation.rs`:221-320 | PASS |
| Canonical parent/project and new-ID checks | `planning_service.rs`:1676-1702,2057-2121; `plan_store.rs`:355-380,1197-1232 | PASS |
| Parent scope / risk / gates preserved | `validation.rs`:475-583; `task_authority.rs`:30-68 | PASS |
| Acceptance proof obligations preserved | `task_authority.rs`:56-60; `validation.rs`:507-508 | **FAIL (F-03)** |
| Planner output never authorizes readiness | `planning_service.rs`:1727-1732 — readiness is trusted-code derived, never proposal-carried | PASS |
| READY is not execution permission | Publication transaction writes only task rows, edges, strings and journal facts (`plan_store.rs`:373-379); no execution effect | PASS |
| Unrepresentable obligations block materialization | `task_authority.rs`:38-47 refuses forbidden-path constraints the ScopeBound cannot represent | PARTIAL — holds for scope, not for proof method (F-03) |
| Child never completes or satisfies parent | No `UPDATE tasks` and no parent mutation in the publication path (`plan_store.rs`:322-412) | PASS |
| Fences: snapshot, run/generation, plan head, parent/project, descendants, child IDs | `plan_store.rs`:330-336,392-410; `planning_control_store.rs`:471-602 | PASS |
| Atomic commit of children, pair, revision, lineage, journal | Single `TransactionBehavior::Immediate` (`plan_store.rs`:322-324,355-379) | PASS |
| Refusal rolls back approval consumption | `plan_store.rs`:337-353 inside the same transaction; test `risk_decision_consumption_rolls_back_when_plan_commit_fails_then_consumes_on_retry`:1069 | PASS |
| Exactly one CREATED + one MARKED_READY | `plan_store.rs`:1146-1151 | PASS |
| Extra lifecycle facts are a conflict | `plan_store.rs`:1146-1151 filters foreign types out | **FAIL (F-02)** |
| Creation ordered before readiness | Not implemented | **FAIL (F-01)** |
| Matching Task, project, states, provenance, proposal-hash causation | `plan_store.rs`:1159-1172 | PASS |
| Reuse appends no duplicate facts, no head/generation/V7 bypass | `plan_store.rs`:374-379 `continue` path; head check at 392-410 | PASS |
| L4 leaves are canonical Task work, not graph sinks | `plan_graph.rs`:245-280 | PASS |
| Pending children count only under READY guarantee | `plan_graph.rs`:260-272 | PASS |
| Pending predecessors never count as completed | `plan_graph.rs`:273-279 | PASS |
| No parent auto-completion on empty runnable set | No completion path in the planning service | PASS |

## Verification evidence

Binaries rebuilt first with `cargo build -p agentd -p agentctl` (exit 0), since
binary-level suites exec `target/debug/agentd` and `cargo test` does not rebuild
it.

| Command | Result |
| --- | --- |
| `cargo build -p agentd -p agentctl` | PASS |
| `cargo test -p storage-sqlite --test plan_persistence` | 24 passed |
| `cargo test -p app-services --lib plan_graph` | 3 passed (22 filtered) |
| `cargo test -p app-services --test planning_service` | 58 passed |
| `cargo test -p app-services --test planning_failure_matrix` | 3 passed |
| `cargo test -p app-services --test planning_task_authority` | 6 passed |
| `cargo test -p plan-domain` | 11 + 50 passed |

Every suite is green. Green tests are the reason the findings are stated as
contract-conformance gaps rather than observed failures: the implementation is
self-consistent and tested against its own assumptions, and the matrix at
`plan_persistence.rs`:1548 has no row for out-of-order pairs or foreign-type
facts, so no existing test can fail on F-01 or F-02.

No workspace-wide run, no authenticated provider smoke, no Windows compile and
no adversarial/security audit were performed in this pass.

## Canonical state

`completed_through` advances to TASK-0089 and product identity to
`0.1.0-dev.task0089`: the verification task itself completed and legitimately
changed audit/planning artifacts (VERSIONING.md task-completion rule). That
records the report, not acceptance of its subject. G7 remains IN_PROGRESS. TASK-0087 remains
merged and unaccepted pending F-01..F-03 remediation, which is separately
scoped work.

## Machine-readable summary

```json
{
  "task": "TASK-0089",
  "base_head": "f0eeb72",
  "verdict": "CONFORMANCE_REMEDIATION_REQUIRED",
  "subject": "TASK-0087 merged child materialization implementation",
  "contract": "ADR-0027 TASK-0088 child readiness eligibility amendment",
  "independent": false,
  "independence_note": "contract text and this pass share one author",
  "clauses_total": 19,
  "clauses_pass": 15,
  "clauses_partial": 1,
  "clauses_fail": 3,
  "findings": [
    {
      "id": "F-01",
      "severity": "MEDIUM",
      "title": "exact retry does not enforce creation-before-readiness ordering",
      "source": "adapters/storage-sqlite/src/plan_store.rs:1128-1177",
      "acceptance_blocker": true
    },
    {
      "id": "F-02",
      "severity": "MEDIUM",
      "title": "extra lifecycle facts conflict only when they duplicate the pair",
      "source": "adapters/storage-sqlite/src/plan_store.rs:1146-1151",
      "acceptance_blocker": true
    },
    {
      "id": "F-03",
      "severity": "MEDIUM",
      "title": "parent proof obligations default to the weakest method instead of failing closed",
      "source": "crates/plan/src/task_authority.rs:56-60",
      "acceptance_blocker": true
    }
  ],
  "observations": ["O-01", "O-02"],
  "suites_green": true,
  "suites": {
    "storage-sqlite/plan_persistence": 24,
    "app-services/plan_graph": 3,
    "app-services/planning_service": 58,
    "app-services/planning_failure_matrix": 3,
    "app-services/planning_task_authority": 6,
    "plan-domain": 61
  },
  "not_performed": [
    "workspace-wide run",
    "authenticated provider smoke",
    "Windows compile",
    "security audit"
  ],
  "canonical_state": {
    "completed_through": "TASK-0089",
    "version": "0.1.0-dev.task0089",
    "gate": "G7",
    "gate_status": "IN_PROGRESS",
    "task_0087": "merged, unaccepted"
  }
}
```
