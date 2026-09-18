# Independent review — TASK-0087 / 0088 / 0089 / 0090 chain

**Verdict as submitted: CONDITIONAL_REJECT.**

2026-09-17/18. Produced in a separate session against base `b795519` with the
fix branch `task-0090-retry-pair-ordering` @ `96a77ca`, read-only, with repros
executed in a scratch clone outside the repository.

## Why this document exists

The TASK-0088 amendment text, the TASK-0089 conformance verification and the
TASK-0090 fix were all produced by one orchestrator session. That chain had no
independent reader: the author of a contract graded the implementation against
it and then repaired what they had graded. This review was commissioned to
break that loop, and it is recorded here because it corrects statements in
`G7_TASK_0089_CHILD_MATERIALIZATION_CONFORMANCE.md` that would otherwise stand
as accepted project history.

## Provenance and trust

This is an external artifact, relayed into the repository by the project owner.
It is evidence, not authority, and it was not re-executed wholesale. What the
orchestrator verified independently against the tree before accepting it:

| Claim | Verified how | Result |
| --- | --- | --- |
| R-01 parent lifecycle never fenced | read `planning_control_store.rs:533-603`, `task_authority.rs:30-70`; confirmed no `root.state()` read anywhere in the fence | CONFIRMED, and **worse than reported**: `planning_service.rs` contains no parent-state check either, so the trusted service path is equally unfenced, not only the public storage API |
| R-02 child project never compared to parent | read `plan_store.rs:1185-1234`; every project check is against `task.project_id()`, the child's own | CONFIRMED |
| R-03 `verification` / `required_review` unread by planning | repo-wide grep: readers only in `task_store` round-trip and the migration; zero `.verification()` call sites | CONFIRMED — the TASK-0089 mitigation text was wrong |
| `VerificationMethod` has no ordering | read `risk.rs:49` — `derive(Clone, Copy, Debug, Eq, Hash, PartialEq)`, no `Ord` | CONFIRMED — "weakest of four" was declaration order, not an ordering |
| R-01/R-02 reproducible | independent tests written in `plan_persistence.rs` failed at the `expect_err` boundary before the fix | CONFIRMED by separate repro |

Not independently verified: R-04 (`required_gate` has no consumer), R-05
(`task-engine.md` staleness), R-06 (authority flag shape), R-07 (queue
bookkeeping), and the reviewer's own execution counts.

## Findings as submitted

### R-01 — Parent Task lifecycle is never fenced (HIGH)

`planning_control_store.rs:533-603` re-derives authority from `allowed_paths`,
`forbidden_paths`, `acceptance` and `risk` and never reads `root.state()`.
`define_planning_metadata` is DRAFT-only, so the compared facts are frozen at
DRAFT while the live lifecycle moves underneath. Executable proof: with the
authority fence explicitly enabled, publication under CANCELLED, COMPLETED and
FAILED parents all succeeded with children landing READY and the plan head
advancing. Breaks ADR-0028 publication fencing item 4.

### R-02 — Child project never compared to the parent project (HIGH)

`plan_store.rs:1185-1234` validates node-addition membership, parent ref, READY
state and event scope — all against the child's own declared project. Proof: a
child in `P-FOREIGN` committed under a parent in another project. Mitigated in
practice because `planning_service.rs:1678-1690` derives the project from the
parent, but ADR-0027 places the refusal at this boundary, not in the caller.

### R-03 — `Task.verification` and `Task.required_review` never reach planning (MEDIUM)

Both fields are persisted and round-tripped but have no readers in planning.
This makes the TASK-0089 F-03 mitigation ("no stronger obligation is
representable anywhere in the system today") false: the durable model carries
two obligation lists that planning ignores.

### F-02 — CONFIRMED, with corrections (MEDIUM)

Foreign-type facts are filtered out rather than treated as conflicts. Two
corrections to the original report: an undecodable journal row does already
fail closed via `StorageError::Corrupt`; and the realistic vector needs a fact
that carries `task_id` without touching the Task row, which the reviewer could
not inject synthetically because `execution.created` is FK-constrained to a
real `executions` row. That sub-scenario is reasoned, not executed.

### R-04 — `required_gate` is never written to the child Task and has no consumer (MEDIUM, latent)

`validation.rs:561-570` accepts an above-baseline child risk when a
`required_gate` is present, but `planning_service.rs:1703-1718` does not carry
the gate onto the Task; it lives only on `plan_nodes`, and nothing reads it.
Latent risk ratchet across decomposition levels; not demonstrated.

### F-01 — CONFIRMED, severity overstated (LOW/MEDIUM)

Reachable only by rewriting `event_journal.seq` with raw SQL; no in-process
path produces an out-of-order pair. Defense-in-depth against a corrupted or
hand-restored journal rather than a reachable production defect.

### R-05 — `task-engine.md` not reconciled with the readiness amendment (MEDIUM, docs)

Last touched by `154fb7c`; line 227 still lists only the Task Application
Service as the DRAFT→READY initiator, while ADR-0027 now adds the Planning
Application Service. No authority contradiction, but the lifecycle document is
stale on the transition this chain turns on.

### R-06 / R-07 (LOW)

The Task-authority fence is switched by a caller-supplied public bool
(production always sets `true`); `queue.yaml` header and the TASK-0089 outcome
text disagreed on `completed_through`.

## Assessment of the TASK-0088 amendment

Does not weaken INV-043, INV-046 or INV-047 as written. One clause —
"obligations that cannot be represented on the child block materialization" —
is unfalsifiable in v1: read literally it forbids all materialization, read as
implemented it forbids nothing. One clause leans on the ADR-0028 lifecycle
fence that R-01 shows is unimplemented.

## Assessment of the TASK-0090 fix

Correct and does not reject legitimate retries: `created.seq < ready.seq` holds
for everything the system writes. It narrows rather than closes, because the
filtered-view flaw underlying F-01 is the same one F-02 names; one unfiltered
check would have closed both.

## Disposition

Accepted by the orchestrator. TASK-0087 stays unaccepted. Remediation order was
changed on the strength of this review: R-01 and R-02 move ahead of F-02 and
are addressed by TASK-0091. R-03/F-03, F-02, and R-04..R-07 remain open.
