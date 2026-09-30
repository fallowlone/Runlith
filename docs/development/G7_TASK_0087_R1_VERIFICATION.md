# TASK-0087-R1 — contract and regression verification

**Verdict: BLOCKED — materialization contract decision required; R1 submitted, not accepted.**

2026-09-17. Worktree `~/dev/runlith-wt-0087-r1`, branch
`task-0087-r1-contract-verification`, base/HEAD
`6a02eaa4f696e673613f7ebd0511c2df7613b5db` (PR #2 merged).
Implementation is merged, not merely branch-local. This review changes only
this report, `planning/tasks/queue.yaml`, and
`adapters/storage-sqlite/tests/plan_persistence.rs`. No production, accepted ADR,
version, roadmap or licensing changes; no commit, push or merge.

## Authority and architecture STOP

Accepted [ADR-0027](../architecture/adr/ADR-0027-g7-planning-orchestration-contract.md)
V3–V9 and L4 require obligation preservation and runnable work, not automatic
lifecycle eligibility. Accepted
[ADR-0028](../architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md)
and its TASK-0084 amendment require trusted publication/execution fences.
[Task lifecycle](../architecture/task-engine.md):169–173,227,402 accepts the
`DRAFT -> READY` path but explicitly leaves its readiness checklist open.
The listed initiator is User / authorized client, Task Application Service.
[INV-043/046/047](../architecture/INVARIANTS.md) distinguish proposal authority,
non-weakening and lifecycle/readiness.

TASK-0086 comparison source is commit
`bc4168c5e1afda908465115327d3391d035294ad`, ADR-0027:403–495 and
ADR-0028:210–257, available in sibling worktree `runlith-wt-0086`.
These child-materialization amendments are absent from the reviewed base and
are not accepted authority here. They require canonical children, durable
creation and Task-backed L4, but do not specify automatic READY eligibility
or a mandatory CREATED + MARKED_READY retry pair.

**STOP:** `crates/app-services/src/planning_service.rs`:1727–1739 calls
`mark_ready()` automatically; `adapters/storage-sqlite/src/plan_store.rs`:1207–1208
requires READY; `crates/app-services/src/plan_graph.rs`:245–280 counts pending
children on that guarantee. The domain transition permits READY but does not
settle the authoring-eligibility policy. This is unresolved authority under the
approved task's stop condition, not a demonstrated atomicity defect or a claim
that the accepted transition is prohibited. Passing tests cannot accept that
policy. Obligation preservation and fence-field completeness below are parts
of the same required contract clarification, not proven product defects.

## Six-requirement trace

Paths and line numbers are relative to the final worktree unless explicitly
marked as sibling/base evidence. Named tests provide stable anchors.

| Requirement | Source and test evidence | Assessment / limit |
| --- | --- | --- |
| Identity, parent and project | Service:1676–1702,2057–2121 derives project from parent, rejects parent-as-child/foreign capture and restricts existing references. Store:355–380,1197–1232 checks parent and conflicts. Service tests at 2422,2567,2649; persistence `malformed_child_materialization_is_refused_before_any_write` at 2009. | Mechanisms present. New journal-project cases do not test child Task project versus parent; missing-parent, duplicate-ID and race coverage must not be inferred from happy paths. |
| Scope, criteria, risk and policy | `crates/plan/src/validation.rs`:221–320,475–583; service:1414,1703–1717. `validation_rules` passes all 50 tests including dropped criteria, scope escapes and escalation grants. | Task metadata stores criterion IDs, not proof methods/gates; `crates/plan/src/task_authority.rs`:52–60 defaults derived methods to automated-test. V5 addresses above-baseline escalation, not a universal inherited-risk floor. Clarify nested preservation rather than assert a new defect. |
| Atomic child, lifecycle pair and Plan | Store:322–380,432–503 uses one IMMEDIATE transaction. Service:1719–1745 mints both facts. Persistence exact-retry test at 1492 and late-failure rollback test at 1952; service lifecycle assertions at 2422–2463. | Atomicity tests pass. Automatic READY preconditions remain the architecture STOP. |
| Exact retry | Store:1127–1177 compares full Task snapshots, requires exactly one CREATED/READY pair and validates states, Task/project, provenance, causation and envelopes. Service:2090–2099 is an early filter only. Persistence tests at 1492,1546,1851; service tests at 2605,2686. | Expanded 18-case matrix passes. Reuse is not a Plan V7/V8 bypass: happy-path reuse uses another branch. Envelope validity is not equality of IDs/timestamps/reasons; ordering and additional lifecycle facts need explicit semantics. |
| Publication fences | `adapters/storage-sqlite/src/planning_control_store.rs`:471–602; store:330–380,392–410. All six `planning_task_authority` tests pass; persistence assertions at 733,919,978. | Covers scope/acceptance/risk/forbidden-path and descendant add/reparent drift. Derived authority is not a complete project/lifecycle snapshot. Explicit covered-field and concurrent child-ID semantics remain to be settled. |
| L4 runnable work | `plan_graph.rs`:198–280 requires completed canonical Plan predecessors, and derived RUNNABLE for existing Tasks. Service:1565 checks before publication. Three graph unit tests plus service tests at 2466,2500,2531 cover non-runnable and running predecessors versus completed predecessors. | Pending children depend on the atomic READY guarantee. Clarify “leaf” as executable Task work rather than necessarily a graph sink. No exhaustive lifecycle/race matrix or parent auto-completion is claimed. |

## Regression diff and coverage

The base matrix in `malformed_persisted_child_retry_evidence_fails_closed_without_partial_state`
contained nine cases, preserved unchanged:
`missing-ready`, `malformed-ready-type`, `wrong-ready-states`,
`wrong-ready-provenance`, `wrong-ready-causation`, `wrong-ready-task`,
`malformed-ready-envelope`, `malformed-created-states`,
`contradictory-created-count`.

R1 adds nine cases to the same test (18 rows, not 18 new Rust tests):

- `missing-created`;
- `malformed-created-type`;
- `wrong-created-provenance`;
- `wrong-created-causation`;
- `wrong-created-task`;
- `wrong-created-project`;
- `wrong-ready-project`;
- `malformed-created-envelope`;
- `contradictory-ready-count`.

The project cases change a journal fact to a separately seeded `P-FOREIGN`
project. They do **not** change the child's canonical project or its parent.
The existing harness still asserts rejection, unchanged existing Task and
journal facts, no retry branch/revision/lineage and rollback of a sibling
inserted earlier in the attempted transaction. No assertion was weakened.
These tests characterize the existing fail-closed implementation; they do not
accept automatic READY policy. No L4 tests were changed.

Remaining limits include broader envelope fields, complete snapshot drift
beyond the existing objective example, event ordering/additional unrelated
lifecycle facts, and explicit child/parent project and race cases. Missing
coverage is not evidence of a defect.

## Proposed amendment — NOT Accepted

Proposed for the normal ADR/task-lifecycle approval path, not retroactive
acceptance or a statement that all clauses already exist in code:

> For G7 materialization, the trusted Planning Application Service may declare
> authoring complete and perform DRAFT -> READY only after V1–V9 validation,
> canonical parent/project and new-ID checks, and explicit preservation of
> parent scope, acceptance proof obligations, baseline risk and required gates.
> Planner output never authorizes readiness. READY means orchestration
> eligibility, not RUNNABLE or execution permission. Unrepresentable obligations
> block materialization instead of defaulting to weaker obligations.
>
> The publication transaction must fence input snapshot, active run/generation,
> Plan head, parent/project authority, descendant membership and proposed child
> IDs. Name which Task/project lifecycle changes invalidate authority. Commit
> READY children, one CREATED (DRAFT -> DRAFT) and one MARKED_READY
> (DRAFT -> READY) fact per child, revision, lineage and plan journal fact
> atomically; refusal also rolls back approval consumption. Previously recorded
> spend is not erased.
>
> Exact child reuse requires identical canonical Task snapshots, a valid unique
> CREATED/READY pair with creation ordered before readiness, matching Task,
> project, states, trusted provenance and proposal-hash causation. New request
> timestamps or event IDs do not require byte-identical events. Explicitly define
> extra lifecycle facts and correlation equivalence. Reuse appends no duplicate
> facts and never bypasses Plan head, generation or V7 checks.
>
> L4 executable leaves are canonical Task work, not necessarily graph sinks.
> They count only with derived RUNNABLE and completed canonical Plan predecessors.
> Pending children count only under the same transaction's READY guarantee,
> without incomplete Task dependencies. Pending predecessors never count as
> completed. No runnable work yields the specified blocker/human decision,
> without automatically completing the parent.

Owner approval must settle these choices, including alternative explicit
readiness admission, before any separately scoped production remediation.

## Verification evidence

Commands ran sequentially in the isolated worktree. Shipped binaries were
rebuilt first with `cargo build -p agentd -p agentctl`. Tests use fixtures unless
explicitly stated; no authenticated real-provider smoke was run.

| Command / suite | Result |
| --- | --- |
| Binary build | PASS |
| `cargo test -p storage-sqlite --test plan_persistence` (baseline) | 24 passed |
| `cargo test -p app-services --lib plan_graph::tests` | 3 passed |
| `cargo test -p app-services --test planning_service` | 58 passed |
| `cargo test -p app-services --test planning_failure_matrix --test planning_task_authority` | 3 + 6 passed |
| `cargo test -p plan-domain --test validation_rules` | 50 passed |
| `cargo test -p fake-planner --test planner_port_contract` | 1 passed |
| `cargo test -p provider-claude --test planner_port_contract` | 10 passed |
| `cargo test -p agentd --test planning_integration` | 5 passed |
| `cargo test -p agentd --test execution_recovery --test live_ownership` | 14 + 5 passed; live ownership took 63.72s |
| `cargo test -p agentctl` | 47 passed (10 + 1 + 1 + 10 + 8 + 3 + 1 + 13); prior verbal total 48 was an arithmetic error |
| Expanded persisted-evidence test, `--exact --nocapture`, `RUST_BACKTRACE=1` | PASS, all 18 rows |
| `cargo test --workspace` (final) | PASS: 1074 passed, 0 failed, 2 ignored across 115 result summaries; includes all 24 persistence tests with expanded matrix |
| `cargo check --workspace --all-targets` | PASS |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo fmt --check` | PASS after fixture-only formatting correction |
| `git diff --check` | PASS |

Two ignored tests require `RUNLITH_CLAUDE_SMOKE=1` and an authenticated CLI:
`execution::tests::real_claude_runs_through_production_g6_isolation` and
`real_claude_smoke_replies_with_marker`. They remain unverified, not green.

Failure/aborted-run history:

- One workspace command started from the main checkout by mistake; stopped
  during compilation, with no test evidence or tracked main edits used.
- An intermediate isolated-worktree workspace run was stopped to verify the
  new matrix directly; it is not a green receipt.
- Two direct matrix runs failed during new fixture mutation: NULL project
  violated CHECK; non-existent foreign project violated FK. Seeding the foreign
  project via public store API fixed the fixture. Production code was unchanged.
- Final static chain initially stopped on rustfmt's requested formatting of the
  new project fixture. Formatting was corrected and fmt/diff checks passed.
- Early check/clippy commands used output-tail pipelines without pipefail; the
  final direct invocations supersede those ambiguous shell exit statuses.

Final workspace output: `/tmp/runlith-r1-final-workspace.log`, SHA-256
`964ab66101be0c05f005e84a24591a97874830645226f83b2c528a80f51558ac`.
Final test file SHA-256:
`8fec91bbef61f03cb67b96af0c29aa2d435e8c1056007168ad5d1c1a55008770`.
Workspace execution preceded a whitespace-only rustfmt repair; check/clippy
passed after semantic fixture correction, and final fmt validates the repair.
Baseline logs are session-local under
`~/.zcode/cli/exec/sess_5dacfebd-dde8-41a3-839f-0a5f01ca1930/`
(`084c3aac-e63d-47ff-ac6f-d2d54ff34452-stdout.log` and
`b034ea2e-89ab-4200-b74e-3b435940ac64-stdout.log`). These logs are operational
artifacts, not committed evidence or post-merge verification.

## Independent review and disposition

An independent read-only reviewer inspected the actual three-file scope and
base identity. It found no assertion weakening or production changes, but
requested accurate project-mismatch wording, separation of base nine-case
coverage from final eighteen-case coverage, and removal of “solely blocked”
while verification was unfinished. Those documentation findings are reconciled
above. Review was stopped at root request; it did not fully validate every
primary authority citation, review the final project-seeding correction or
independently rerun commands. Root verified the corrected fixture and final
execution evidence. This is a bounded independent review, not complete final
architecture certification or an exhaustive security audit.

Keep `completed_through: TASK-0085`, version `0.1.0-dev.task0087` /
`TASK-0087`, and G7 `IN_PROGRESS`. The required owner architecture decision and
subsequent full independent acceptance remain open. No G7_CLOSED claim,
invented acceptance history, production remediation or Git publication.
