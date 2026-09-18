# Task Engine — Domain Contract

> Repository-local contract for the Task domain: ownership, the
> Task/Execution/Attempt/Session separation, the canonical lifecycle, accepted
> transitions, completion authority, and explicitly open questions. Documents
> accepted architecture; invents none; implements nothing.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md)
> - Decisions: [`adr/ADR-0004-task-central-entity.md`](./adr/ADR-0004-task-central-entity.md),
>   [`adr/ADR-0005-task-execution-attempt-session.md`](./adr/ADR-0005-task-execution-attempt-session.md),
>   [`adr/ADR-0006-state-event-journal-event-sourcing.md`](./adr/ADR-0006-state-event-journal-event-sourcing.md),
>   [`adr/ADR-0008-worktree-isolation.md`](./adr/ADR-0008-worktree-isolation.md),
>   [`adr/ADR-0009-provider-adapters-acl.md`](./adr/ADR-0009-provider-adapters-acl.md)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§11–14, 20–21.1, 44–45

## 1. Core principle

```text
Task
!=
Execution
!=
Attempt
!=
Session
```

These four concepts never collapse into one persistent object. Durable
business state (Task) stays independent of orchestration instances
(Execution), concrete tries (Attempt), and transient provider mechanics
(Session). Provider or model must never become Task domain semantics
(INV-002, INV-012, INV-059).

## 2. Canonical ownership

The Task bounded context owns the durable business concept of work. Its
authoritative state is task rows, dependency edges, and lifecycle status
(ARCHITECTURE.md §2, §5). Conceptually:

```text
Task
├── ID
├── Objective
├── Parent / Epic relationship
├── Dependencies
├── Acceptance Criteria
├── Scope
├── Risk
├── Required Verification
├── Required Review
├── Artifact references
└── Lifecycle Status
```

No persistence-internal fields (timestamps, revision counters,
optimistic-lock versions, tenant IDs, or similar) are canonical here — the
accepted sources do not establish them. Where such mechanics are clearly
necessary but unspecified, they are listed as open design items (§16).

## 3. Task does NOT own

Explicitly outside Task ownership:

```text
provider-specific session ID
Claude/Codex/Gemini-specific state
process PID
PTY state
provider transcript as canonical state
model context window
worktree implementation mechanics
SQLite row layout
verification self-approval
secret values
routing/model-selection internals
```

Provider references travel only as opaque `ProviderRef` /
`external_session_ref`, never as provider-specific fields on the Task
(ADR-0005, ADR-0009). A field such as `claude_session_id` on Task is an
explicit domain-model violation.

## 4. Concept definitions

### Task

Durable unit of intended work and acceptance. Survives agent restart,
session restart, provider migration, and agentd restart (INV-001). Task
lifecycle stays independent of provider behavior: scheduling may reassign a
Task across providers without changing Task semantics (ADR-0004).

### Execution

A durable orchestration instance attempting to deliver a Task under a
particular execution plan, scope, and environment. No provider-specific
semantics are defined here; exact Execution ownership details beyond this
are open (§16).

### Attempt

One concrete try inside an Execution. Provider, model, and runtime choices
may differ between Attempts without altering Task semantics. Stateful
failover creates or recovers an Attempt rather than transparently replaying
side effects across providers/models (INV-066).

### Session

Ephemeral provider/runtime conversation and process detail belonging to an
Attempt. Provider session state is not durable application truth
(INV-019); a chat-session store is never project state.

Accepted hierarchy:

```text
Task AUTH-42
 |
 +-- Execution E1
 | |
 | +-- Attempt 1 / provider A / failed
 | |
 | +-- Attempt 2 / provider A / rejected
 |
 +-- Execution E2
 |
 +-- Attempt 1 / provider B / passed
```

`Session` is a temporary provider-specific runtime entity inside an Attempt.

## 5. Canonical lifecycle

Accepted lifecycle, exactly as established (SoT §12). State names are
fixed — never renamed, added, removed, or reordered:

```text
DRAFT
 ↓
READY
 ↓
CLAIMED
 ↓
RUNNING
 ├──→ BLOCKED ───→ RUNNING
 ↓
SUBMITTED
 ↓
AWAITING_REVIEW
 ↓
VERIFYING
 ├── failure → RETRY_READY → RUNNING
 ↓
COMPLETED
```

Side states:

```text
FAILED
CANCELLED
```

Solid arrows above are accepted main-path transitions. Every transition
involving FAILED, CANCELLED, review rejection, or a bypass of
AWAITING_REVIEW is unresolved side-state semantics (§7, §16) — the diagram
must not be read as more complete than the architecture is.

## 6. State semantics

- **DRAFT** — work not yet eligible for execution. Establishes only
  `DRAFT -> READY`; the readiness validation checklist is open (§16).
  Planning metadata (scope, acceptance obligations, baseline risk) is
  authorable only here, so a DRAFT Task's own contract is not yet frozen.
  Consequently a DRAFT Task cannot publish a decomposition: the parent
  admission set for planning is READY, CLAIMED, RUNNING, BLOCKED and
  RETRY_READY (ADR-0027 TASK-0091 amendment).
- **READY** — persistent lifecycle state: the Task is eligible for
  orchestration. READY must never be equated with "scheduler says RUNNABLE
  right now" (§8).
- **CLAIMED** — orchestration has reserved/assigned the work for execution.
  Assignment identity (which provider/agent) belongs to
  execution/orchestration state, never to Task semantics: no
  `claimed_by_claude` / `claimed_by_codex` fields. Lease and stale-claim
  recovery semantics are open (§16).
- **RUNNING** — active work toward the Task acceptance contract. RUNNING
  implies none of: progress is correct, tests pass, the agent owns
  completion authority, or the current provider will remain assigned.
- **BLOCKED** — explicit, observable impediment; accepted loop
  `RUNNING -> BLOCKED -> RUNNING`. Supported cause families: missing
  dependency, policy restriction, missing user input, required isolation
  unavailable, planning unable to produce valid executable work,
  environment/runtime problem. No canonical blocker enum is established;
  a typed blocker taxonomy is an open design item (§16).
- **SUBMITTED** — an implementation attempt claims readiness for
  independent review/verification. Submission is a claim, never completion:
  the model/agent cannot grant itself success.
- **AWAITING_REVIEW** — review pending/routed before verification where
  required. Whether `SUBMITTED -> VERIFYING` may bypass AWAITING_REVIEW
  when `required_review = none` is unresolved (§16).
- **VERIFYING** — independent acceptance evaluation consuming the trusted
  outcome of the Evaluation context (tests, compilation/type checking,
  acceptance criteria, review evidence, security checks, artifact
  inspection). Task owns the requirement/reference; Evaluation owns
  execution and interpretation.
- **RETRY_READY** — another execution attempt is permitted/required after
  failed verification. Never means "repeat identical side effects
  blindly": the new/recovered execution/attempt must respect state,
  workspace, policy, provider, checkpoint, and side-effect semantics.
  Retry budgets and backoff belong to orchestration/reaction policy, not to
  the lifecycle state.
- **COMPLETED** — the Task acceptance contract has been independently
  satisfied via VERIFIED verdict. Not implied by: agent exit code, provider
  "done" claims, claimed test passes, existence of children, planner halt,
  or session end. Parent completion additionally requires independent
  verification of the parent's own acceptance contract — never inferred
  from child creation or child completion (§10).
- **FAILED** — accepted side state, conservatively: the Task cannot
  continue successfully under the current accepted execution/recovery
  policy. All incoming transitions are unresolved (§16). Mandatory:
  provider attempt failure != Task FAILED automatically — a Task may be
  retried or migrated to another provider (§12).
- **CANCELLED** — accepted side state. Authority, reversibility,
  cancellable states, and worktree/process consequences are unresolved
  (§16).

## 7. Transition registry

"Accepted" only where current architecture clearly establishes the
transition; everything else is "Unresolved" and must not be guessed.

| From | To | Trigger / semantic reason | Authority allowed to request | Required preconditions | Evidence / reason required | Accepted or unresolved |
|------|----|---------------------------|------------------------------|------------------------|----------------------------|------------------------|
| DRAFT | READY | authoring finished; work declared eligible | User / authorized client, Task Application Service, Planning Application Service (materialized children only, ADR-0027 TASK-0088 amendment) | readiness validation (checklist open, §16); for materialized children, full V1-V9 validation inside the publication transaction | reason for eligibility | Accepted (path; general preconditions open) |
| READY | CLAIMED | orchestration reserves work for execution | Scheduler / Reconciler via Task Application Service | Task READY; dependencies satisfiable per readiness | assignment reference | Accepted |
| CLAIMED | RUNNING | execution begins under a plan/scope/environment | Scheduler / Reconciler, Agent Runtime via Task Application Service | valid execution assignment | execution reference | Accepted |
| RUNNING | BLOCKED | explicit impediment encountered | Agent Runtime, Scheduler / Reconciler, Policy Engine, Planner | blocker is explicit and observable | blocker reason | Accepted |
| BLOCKED | RUNNING | blocker cleared or escalated past | Scheduler / Reconciler via Task Application Service | impediment resolved or superseded | resolution reference | Accepted |
| RUNNING | SUBMITTED | attempt claims readiness for review/verification | Agent Runtime (agent `submit_result()`) | work product exists for review | submission payload reference | Accepted |
| SUBMITTED | AWAITING_REVIEW | routed to required review | Scheduler / Reconciler, Orchestrator | required review pending | routing reference | Accepted |
| AWAITING_REVIEW | VERIFYING | review satisfied; admitted to evaluation | Review workflow / Evaluation Engine | review requirements met | review evidence | Accepted (path; bypass question open) |
| SUBMITTED | VERIFYING | direct admission when no review required | Scheduler / Reconciler | `required_review = none` semantics | routing reference | Unresolved |
| AWAITING_REVIEW | RETRY_READY | review rejection returns work | Review workflow | rejection semantics defined | rejection reason | Unresolved |
| AWAITING_REVIEW | RUNNING | review returns work to execution | Review workflow | return semantics defined | return reason | Unresolved |
| AWAITING_REVIEW | FAILED | review declares work unviable | Review workflow | failure semantics defined | failure reason | Unresolved |
| VERIFYING | COMPLETED | trusted VERIFIED verdict on the acceptance contract | Evaluation Engine only | all required verification satisfied | verdict + evidence refs | Accepted |
| VERIFYING | RETRY_READY | trusted REJECTED/failed verdict | Evaluation Engine | failure verdict with evidence | verdict + evidence refs | Accepted |
| RETRY_READY | RUNNING | new/recovered execution permitted | Scheduler / Reconciler via Task Application Service | retry policy permits; state/workspace/policy/checkpoint semantics respected | new execution reference | Accepted (path; budgets open) |
| * | FAILED | task cannot continue under accepted recovery policy | Task Application Service on behalf of authorized trigger | failure declared with reason | failure reason | Unresolved (all incoming) |
| * | CANCELLED | authorized cancellation | unresolved authority (§16) | unresolved | cancellation reason | Unresolved (all incoming) |

## 8. Lifecycle vs scheduling readiness

Persistent lifecycle (authority: Task context):

```text
DRAFT / READY / CLAIMED / RUNNING / BLOCKED / SUBMITTED /
AWAITING_REVIEW / VERIFYING / RETRY_READY / COMPLETED / FAILED / CANCELLED
```

Derived scheduling readiness (authority: Scheduler/Reconciler; examples,
not lifecycle):

```text
RUNNABLE / WAITING_ON_DEPENDENCY / WAITING_ON_POLICY / EXPANDED / TERMINAL
```

Invariant (INV-047): Task lifecycle state is distinct from derived
scheduling readiness. The two state machines must never merge; e.g.
`Task.status = READY` with an incomplete dependency yields
`readiness = WAITING_ON_DEPENDENCY` without rewriting Task lifecycle.

## 9. Dependencies

Dependencies are durable Task-graph relationships (dependency edges are
authoritative Task state). They contribute to derived scheduling readiness
only. A dependency change never automatically mutates a Task's lifecycle
state — no dependency-specific lifecycle states exist.

## 10. Parent / child and plan revisions

Adaptive Planning may create child Tasks, but Planner output is a proposal,
never authority (INV-043): only validated application services mutate the
durable Task graph, via validated `PlanChangeSet`s (acyclicity, valid node
references, preserved parent acceptance, in-scope children, unweakened
policy/approvals, budget/depth/fan-out limits). Plan graph revisions and
Task state transitions are different concepts — `plan_revision` never
enters the Task lifecycle enum; a planner may revise graph structure
without redefining lifecycle semantics (INV-044: every runtime graph
mutation creates a versioned plan revision with reason and provenance).
Creating children never satisfies parent acceptance.

## 11. Task kind, scope, acceptance, verification, review, risk, artifacts

- **Task kind** — extensible metadata guiding planning, routing, context
  preparation, and evaluator selection (illustrative classes: Research,
  Design, Implementation, Verification, Integration, Operations). Kinds
  never alter core lifecycle through provider-specific branching: no
  `ClaudeTask` / `CodexTask` domain variants. Registry values are open
  (§16); new kinds must not change the state machine.
- **Scope** — explicit contract (conceptually: allowed paths, forbidden
  paths, authorized boundaries referenced by the execution envelope).
  Scope cannot be silently expanded by an agent; out-of-scope discoveries
  resolve to block / propose separate task / plan revision. Final
  serialization is not defined here.
- **Acceptance Criteria** — durable Task intent. Tests are evidence for
  criteria, never the criteria themselves. Criteria cannot be weakened by
  Planner, Agent, Provider, retry path, or child decomposition without an
  authorized specification/plan revision (INV-046).
- **Required Verification** — Task stores the requirement/reference;
  Evaluation owns executing and interpreting verification. No evaluator
  implementation inside the Task domain. Required-verification taxonomy is
  open (§16).
- **Required Review** — Task owns the requirement/reference; the review
  workflow owns processing. No provider-specific reviewer semantics in
  Task. Required-review taxonomy is open (§16).
- **Risk** — metadata influencing routing, policy, required review,
  verification, and isolation. No provider-specific Task semantics follow
  from risk. Risk taxonomy is open (§16).
- **Artifact references** — Task refers to artifacts; content and storage
  ownership belong to the Artifact/Resource subsystem. Task never becomes
  an artifact blob store.

## 12. Provider failure semantics

```text
Attempt failure
!=
Task failure
```

Accepted behavior: provider A attempt fails → later provider B attempt may
succeed, without altering Task objective or acceptance semantics (Task may
migrate between providers without state loss, INV-023). Stateful failover
creates a new/recovered attempt; side effects are never transparently
replayed across models/providers (INV-066). The whole recovery algorithm is
not designed here.

## 13. Completion authority

The agent/runtime API is:

```text
submit_result()
```

and never:

```text
complete_task()
```

COMPLETED authority belongs solely to the trusted Evaluation /
Verification path (INV-009): no provider, agent, CLI, or session may mark
its own Task COMPLETED. Only the evaluation pipeline grants COMPLETED, on
a VERIFIED verdict with evidence (SoT §44–45: tests, lint, typecheck,
security, acceptance criteria, reviewer, benchmarks → VERIFIED / REJECTED).

## 14. Persistence and event boundary

Task state survives every agent/session/process restart (INV-001). In v1,
Task current state is relational authoritative state — explicitly not full
Event Sourcing (ADR-0006). Once persistence exists, every Task state
mutation produces an append-only Event Journal record transactionally with
the state change:

```text
BEGIN
UPDATE task current state
INSERT corresponding event-journal record
COMMIT
```

No SQLite schema and no event implementation are defined by this document.

## 15. Transition authority and integrity

Conceptual authority (no concrete APIs defined here):

| Actor / component | May request | May propose | May validate | May authoritatively commit |
|-------------------|-------------|-------------|--------------|----------------------------|
| User / authorized client | lifecycle actions within scope | — | — | — (via Task Application Service) |
| Task Application Service | — | — | transitions, graph mutations | Task state and graph |
| Scheduler / Reconciler | claim, run, block, submit-routing, retry | next actions | readiness, recovery | — (commits via Task Application Service) |
| Planner | — | decompositions, replans (`PlanChangeSet`) | — | never Task graph/state directly |
| Agent Runtime | submit, block-report | — | — | never durable Task state |
| Provider Adapter | — | — | — | never Task state (INV-004) |
| Evaluation Engine | verify, retry-ready, complete | — | verdicts | COMPLETED only via VERIFIED verdict path |
| Policy Engine | block, deny, require approval | — | authorization decisions | policy decisions, not Task lifecycle |

Critical rules: Provider Adapter cannot commit Task state. Planner cannot
commit Task graph/state directly. Agent cannot commit COMPLETED. An
Evaluation result is required before COMPLETED.

Every eventual durable Task state mutation must be validated, auditable,
reason-bearing, and correlatable (INV-010, INV-044). UI, provider adapter,
model, or planner may request or propose but must never mutate durable
Task state directly. Invalid lifecycle transitions must be explicitly
rejected: never silently coerced, never mutating durable state, always
observable and testable. Repeated lifecycle commands after crash/retry
should be designed with idempotency awareness; exact command-idempotency
design is open (§16).

## 16. Open Task Domain Design Items

Unresolved by accepted architecture; listed, not decided. Each item below
is included only because no accepted source resolves it:

- exact DRAFT → READY validation/preconditions for the general
  User / authorized client path. Partially resolved for one case only: a
  planner-materialized child has defined preconditions (full V1-V9 validation,
  canonical parent/project and new-ID checks, and parent admission) per the
  ADR-0027 TASK-0088 and TASK-0091 amendments. The general checklist remains
  undecided;
- whether tasks with no required review bypass AWAITING_REVIEW
  (`SUBMITTED -> VERIFYING`);
- review-rejection transition (`AWAITING_REVIEW -> RETRY_READY / RUNNING /
  FAILED`);
- all valid incoming transitions to FAILED;
- all valid incoming transitions to CANCELLED;
- whether CANCELLED is reversible;
- who has cancellation authority;
- typed blocker taxonomy;
- retry-budget ownership and exhaustion outcome;
- exact CLAIMED lease/ownership semantics;
- whether stale CLAIMED automatically returns to READY or uses a recovery
  flow;
- Task revision/versioning mechanism;
- concurrency/optimistic-lock semantics for Task mutations;
- Task-kind registry;
- risk taxonomy;
- required-review taxonomy;
- required-verification taxonomy.

## 17. Non-goals

This document does NOT define: SQLite schema; Event schema; IPC commands;
provider protocol; runner protocol; scheduler algorithm; retry policy;
planning algorithm; context compilation; worktree implementation;
verification implementation; UI state model.
