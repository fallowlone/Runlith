# ADR-0025 - G5 Post-Admission Entry Recovery Path

Status: Accepted
Date: 2026-09-11

## Context

TASK-0052 proved an admitted execution can strand: the atomic admission
transaction (ADR-0024 §6) commits Task `RUNNING` + Execution + Attempt
reservation + journal triple, and a crash — or a deterministic background
failure such as a non-repository path or an unresolvable base revision —
before `create_attempt` leaves a Task that ordinary `run` honestly refuses
(`INVALID_TRANSITION`), `cancel` misreports (`ALREADY_CONCLUDED … no
attempts recorded`), and no production operation recovers (B-RS). The only
working path was an in-process library call nothing in production invokes
after a restart. Durable did not mean recoverable (INV-001).

No `RUNNING → READY` edge exists (`task-engine.md` §7/§16; `state.rs`
`ACCEPTED_TRANSITIONS`), and the open Task-domain items list stale-claim
recovery as undecided. Any recovery semantic must therefore be derived
from the existing lifecycle, not invented in implementation code
(TASK-0054 §3).

## Decision

### 1. Recovery resumes the admitted Execution in place — no Task transition

A stranded pre-Attempt entry (execution exists, zero attempts, exactly one
owned reservation, reservation task agrees with execution ownership, task
is `RUNNING`, lease absent or `Allocated`/`Ready`) recovers by continuing
the SAME execution with its reserved attempt identity through the normal
background half (lease → attempt → execute → conclude → submit → verify).

No Task lifecycle transition is performed or required, so the canonical
state machine is untouched: no `RUNNING → READY` edge, no new state, no
`ACCEPTED_TRANSITIONS` change. A `BLOCKED` task resumes first through the
accepted `BLOCKED → RUNNING` operator path, which re-arms recovery with no
new execution.

### 2. Recovery is explicit, not automatic

The production control plane gains one additive IPC operation,
`ResumeExecution` (`execution resume`, protocol v2 field 5 — the error
field stays 4), plus a read-only daemon-startup scan that surfaces strands
with the exact resume invocation. Startup never spawns provider work:
workspace inputs are operator-supplied operational data that is never
persisted, and surprise provider spawns after a restart are the G11 orphan
class. The architecturally correct startup action is to surface the strand
honestly and await the explicit, auditable resume.

`OrchestrationService::plan_pre_attempt_resume` is the read-only planning
boundary (detection + validation, zero mutation); the `agentd` composition
root owns input checks, cancel-flag registration, and the detached
background spawn — reusing `run_background` unchanged. Domain crates take
no Git dependency for this: pre-admission validation and resume input
checks live in the composition root over read-only `GitPort` observations.

### 3. Deterministic failures refuse before admission

Repository-path usability and base-revision resolvability are
synchronously detectable with read-only Git observations, so they refuse
with typed `INVALID_ARGUMENT` before the first mutation instead of
wedging `RUNNING` in the background. Preflight is diagnostic, not a
concurrency or security boundary; Git/filesystem/provider effects stay
outside the SQLite transaction (INV-145).

### 4. Fail-closed posture everywhere else

- An execution that already records attempts is out of scope:
  post-attempt crash/orphan recovery stays an explicit G11 deferral.
  Concluding an open attempt without provider evidence would fabricate a
  terminal fact, so resume refuses `INVALID_TRANSITION` instead.
- Missing/ambiguous reservations, reservation/execution task disagreement,
  a missing task row, or a terminal lease fail closed as `Corrupt` — never
  healed by guessing ownership.
- Concurrent resumes converge: lease allocation and reservation
  consumption keep their existing constraint guards, so exactly one
  Attempt is created and exactly one provider run proceeds; losers fail
  safely before executing.
- `cancel` on a strand reports `AWAITING_RESUME` with the resume
  invocation (HARD-3 fixed) instead of claiming conclusion; `status`
  output is unchanged and already honest (`RUNNING`, `live=false`, zero
  attempts).

### 5. No new durable vocabulary

Recovery reuses the existing saga facts (`workspace.created`,
`attempt.created`, …) with their existing atomic boundaries — no new
event types, no schema migration, no reservation lifecycle change. The
Attempt reservation is still consumed atomically with Attempt creation;
the managed-identity reservation still follows the admission/lease claim
discipline (ADR-0024 §6, TASK-0053).

### 6. Repository identity: no redesign

Different `repository_ref` spellings for one physical repository need no
canonicalization for correct recovery: admission holds a conservative
global managed-identity reservation, lease allocation additionally
enforces per-repository ownership, and lease-backed resume requires
exact-string agreement with the recorded lease (fail closed). Distinct
spellings allocate distinct branches and can never alias. Precise
physical-repository canonicalization stays deferred as before.

## Alternatives considered

- Abandon-to-rerunnable (release the entry back to `READY`): rejected —
  no `RUNNING → READY` (or `RUNNING → RETRY_READY`) edge exists, and
  inventing one rewrites undecided Task-domain semantics for a case
  resume already covers without any state-machine change.
- Automatic resume on daemon boot: rejected — workspace inputs are not
  persisted by design (`repository_path` is machine-local operational
  data), and boot-time provider spawns surprise the operator and widen
  the G11 orphan class. Explicit resume keeps a human intent behind every
  provider launch.
- Bounded conclusion of stranded OPEN attempts (`Failed` with a
  reconciliation note): rejected — a terminal outcome without provider
  evidence fabricates history; the open attempt may still have a live
  orphaned provider. Documented G11 deferral instead.
- Persisting workspace inputs at admission for fully automatic restart
  recovery: rejected — new durable machine-local state for marginal gain
  over explicit resume, plus the surprise-spawn objection above.

## Consequences

- `crates/ipc-contracts/proto/execution_core.proto` grows the additive
  `ResumeExecution{Request,Response}` messages (field 5, protocol still
  v2); `agentctl` gains `execution resume`; `ipc.md` §16 and
  `workspace.md` Crash/Reconciliation record the new operation.
- `ACCEPTED_TRANSITIONS`, `EventType`, and the SQLite schema are
  unchanged (still schema 7, still 17 event types).
- Entries admitted before migration 0006 whose reservations were never
  recorded are not resume-eligible (fail closed `Corrupt`): the attempt
  identity is unrecoverable by construction. Dev-era data only; no
  backfill is possible or attempted.

## Security impact

Resume triggers privileged execution (lease + provider spawn), so it
rides the same TB2/peer-auth/handshake authority as `run` — no new trust
boundary, no new IPC field carrying secrets, prompts, or credentials.
Error detail stays static (supplied paths are never echoed), provider
precheck and managed-root gates run before planning, and provider outputs
stay untrusted evidence under the existing conclusion authority
(ADR-0018 unchanged).

## Migration / replacement impact

None: no schema change, no protocol-version change, no `ExecutionPort`
change. A second provider reuses resume unchanged (composition-owned
executor binding, as with `run`).

## References

- `INVARIANTS.md`: INV-001, INV-002, INV-004, INV-009, INV-010, INV-145
- ADR-0005 (Task != Execution != Attempt != Session),
  ADR-0006 (journal atomicity), ADR-0018 (completion authority),
  ADR-0022 (runtime journal scope), ADR-0023 (workspace ownership),
  ADR-0024 §4/§6 (execution seam, atomic admission)
- `docs/architecture/task-engine.md` §§5–7, 16 (lifecycle, open items)
- `docs/development/G5_CLOSURE_REREAUDIT.md` §§6, 17–18 (B-RS, DOC-2)
- `docs/development/G5_POST_ADMISSION_RECOVERY_REMEDIATION.md` (evidence)

## Amendment (TASK-0056, G5 B-CR): exclusive process-local live ownership

§4's "losers fail safely" now names its mechanism instead of assuming it.
For one Execution inside one live daemon there is at most one live-run
owner: `run` and `resume` share one ownership domain in the composition
root (`CancelRegistry::try_claim`, insert-if-absent, returning an opaque
`LiveOwnerToken`). A request reports acceptance of the live continuation
only after winning the claim; a loser is refused with typed `CONFLICT`
("already has a live run"), spawns no background continuation, and touches
no registry entry. Release is owner-scoped
(`release(execution, token)` removes only the current token), so a stale
or losing path can never unregister the winner, replace its cancel
handle, or flip `status` to `live=false` / `cancel` to `NO_LIVE_RUN`
while the owner runs. Ownership is process-local runtime state only — no
new Task state, no persisted owner rows; a fresh daemon starts with an
empty registry and stranded entries recover under the §1–§6 rules above.
Evidence: `docs/development/G5_LIVE_OWNERSHIP_REMEDIATION.md`.

## Amendment (TASK-0057, G5 B-RS2): proven branch-only continuation

An `Allocated` v2 lease may be resumed from the narrow branch-only crash
shape left when `git worktree add -b` created the managed branch but the
daemon died before worktree registration/path creation and lease
finalization. Recovery is allowed only after proving the persisted lease
belongs to the requested Execution, the v2 branch is the exact branch
derived for that Execution, the durable managed-identity reservation is
owned by the same Execution, no competing lease owns the branch or target
path, the recorded base still resolves, the branch tip equals that exact
base, the target path is absent, and Git registers neither the target nor
the branch in another worktree.

The recovery mutation continues the existing branch with
`git worktree add <path> <existing-branch>`. It does not create, reset, or
delete a branch. After the normal postconditions pass, the existing
`Allocated -> Ready` transition emits the existing `workspace.created`
fact exactly once. A crash after attachment but before finalization is
therefore the already-recognized complete-Git/Allocated shape and is safe
to reconcile on the next restart.

`resume` performs the same branch-only proof read-only before claiming
TASK-0056 live ownership. A deterministically ambiguous state therefore
returns the existing typed error taxonomy synchronously and spawns no
background continuation; the background path repeats the authoritative
proof immediately before mutation, so this preflight is not a TOCTOU
correctness boundary. Pre-v2 lease classification is unchanged here;
B-OL remains TASK-0058 scope.

TASK-0058 subsequently replaced that prefix-only persisted-lease
classification. Current-v2 branch-only recovery is entered only after the
complete persisted branch/worktree pair and durable reservation prove
current v2. Supported historical leases use their persisted identity
through ordinary legacy reconciliation; mixed or corrupt evidence fails
closed. This does not widen the branch-only recovery class above.

Evidence: `docs/development/G5_BRANCH_ONLY_MATERIALIZATION_RECOVERY.md`.
