# Engineering Orchestrator Runbook

> Detailed development workflow for the stable bootstrap in
> [`../../ORCHESTRATOR.md`](../../ORCHESTRATOR.md).  This runbook coordinates
> repository work; it is subordinate to `AGENT.md`, architecture invariants,
> accepted ADRs and active task envelopes.

## 1. Why this exists

Long-running AI-assisted development fails when project state lives mainly in a
chat window.  Chats roll over, providers change, reviewers see different
context, and remembered "current state" becomes stale.

The orchestrator therefore keeps **process stable but state reconstructable**:

```text
stable role/runbook
        +
fresh repository evidence on every session
        =
resumable orchestration without transcript dependence
```

This deliberately mirrors Runlith's own product principle: durable state is not
the provider session.

## 2. State model

### 2.1 Canonical development state

Canonical facts are owned by the repository source that defines them:

| Fact | Canonical source |
|---|---|
| Product purpose / non-goals | `PROJECT_CHARTER.md` |
| Coding-agent process / authority | `AGENT.md` |
| Non-negotiable architecture | `docs/architecture/INVARIANTS.md` |
| Accepted architecture decisions | `docs/architecture/adr/*.md` + index/coverage |
| Gate progression | `planning/roadmap.yaml` |
| Near-term sequencing / task outcomes | `planning/tasks/queue.yaml` |
| Development identity | `product/version.toml` |
| Implemented behavior | committed code + tests |
| Integration truth | Git commits/merges + post-merge verification |

The orchestrator must not create a second manually maintained "current state"
file that duplicates these facts.

### 2.2 Operational state

Operational state is measured at runtime:

- active Git worktrees;
- clean/dirty branches;
- current HEADs and merge-bases;
- submitted but unmerged commits;
- running/recent test evidence;
- externally produced review reports.

It is valid to summarize this in chat, but on a new session it must be measured
again.

### 2.3 Ephemeral context

The following can help navigation but are never authoritative on their own:

- chat transcript;
- assistant/model memory;
- unverified agent report;
- copied plan from another tool;
- "I think the branch was..." recollection.

## 3. Fresh-session procedure

### Step A — establish repository identity

Determine the exact repository and `main` state.  Do not assume the shell opened
in the right worktree.

Recommended commands:

```bash
git rev-parse --show-toplevel
git status --short --branch
git rev-parse HEAD
git rev-parse main
git log -12 --oneline --decorate
git worktree list
```

Do not clean, reset, stash or remove anything during discovery.

### Step B — load L0 authority

Read the compact policy and state sources only:

1. `AGENT.md`
2. `PROJECT_CHARTER.md`
3. `product/version.toml`
4. `planning/roadmap.yaml` current gate/status
5. `planning/tasks/queue.yaml` header and current gate tasks
6. `docs/architecture/INDEX.md`
7. relevant invariant categories

Never use `find ... -exec cat` over arbitrary repository trees.  File discovery
may enumerate paths, but context loading must be intentional and avoid
credential-bearing files.

### Step C — reconcile obvious state drift

Cross-check:

```text
version.task
queue outcomes
roadmap.current_gate
merged main history
active worktrees
```

(TASK-0166: `queue.completed_through` removed as legacy-stale; per-task
outcomes plus merged main history are the completion record.)

A mismatch does not automatically mean one value should overwrite another.
Establish what actually completed and what each field is defined to represent.

### Step D — identify the current action

The priority order is normally:

1. resolve a red post-merge or security/correctness blocker;
2. finish/verify an already-active canonical task;
3. reconcile state that would make the next task ambiguous;
4. perform required independent closure/review;
5. start the next dependency-ready queue task;
6. discretionary backlog/research only when the critical path is unblocked.

### Step E — load task-specific L1/L2 context

Read only the current task's dependencies, references, ADRs, source and tests.

## 4. Selecting work without hallucinating a roadmap

The queue sequences work; architecture constrains it.  To choose a new task:

1. find the first not-completed task relevant to `current_gate`;
2. verify every `depends_on` task is actually integrated;
3. verify no `blocked_by` condition is active;
4. verify the task does not conflict with a closure/remediation result merged
   after the queue entry was written;
5. read its references before writing an execution prompt;
6. if the task is too broad to be executable under the accepted contract,
   propose bounded decomposition rather than silently expanding it.

Do not use a model's enthusiasm as a prioritization signal.  Product leverage is
important, but correctness, security, durable-state integrity and explicit gate
dependencies are hard constraints.

## 5. Worktree ownership

The orchestrator should normally keep `main` as integration-only state.

For a canonical task:

```text
main
  └─ task branch
       └─ dedicated worktree
```

Rules:

- implementation agents write only to their supplied task worktree;
- read-only reviewers may inspect the task worktree/commit but do not modify it;
- if a reviewer must test patches, create a separate review/remediation branch;
- never give two write-capable agents the same worktree;
- leave dirty unrelated worktrees alone unless their owner/task explicitly
  authorizes cleanup.

## 6. Delegation workflow

### 6.1 Prepare

Before delegating, the orchestrator records/knows:

- task ID or bounded reconciliation identity;
- objective;
- base `main` commit;
- branch and worktree;
- task dependencies;
- authority references;
- allowed/forbidden paths;
- acceptance criteria;
- verification suite;
- merge rights;
- stopping conditions.

Use `ORCHESTRATOR_EXECUTION_ENVELOPE_TEMPLATE.md`.

### 6.2 Delegate

The agent receives enough context to execute but not the whole repository in its
prompt.  The prompt tells it where to retrieve exact contracts.

For implementation, require a machine-readable final result consistent with
`AGENT.md` §14.

For independent review, require:

- immutable comparison base and reviewed HEAD;
- read-only behavior;
- finding-by-finding evidence;
- explicit merge-blocking classification;
- verification results;
- no repair work in the same review.

### 6.3 Observe

Do not micromanage a healthy agent.  Intervene when:

- it expands scope;
- it edits the wrong worktree;
- a contract is missing;
- repeated failures show a wrong hypothesis;
- security/architecture uncertainty requires escalation.

## 7. Verifying an implementation result

When an implementer reports completion:

### Repository checks

```bash
git status --short --branch
git rev-parse HEAD
git merge-base main HEAD
git log --oneline --decorate main..HEAD
git diff --stat main...HEAD
git diff --check main...HEAD
```

Then inspect the actual diff and the files relevant to the acceptance criteria.

### Claim checks

For every material claim ask:

- Is this visible in production code, or only in a test/mock?
- Does the production composition path use the claimed behavior?
- Is failure fail-closed where required?
- Did the agent change its own approval criteria/tests in a way requiring
  independent validation?
- Are version/queue/roadmap changes appropriate for this task?

### Verification checks

Risk determines whether to rerun the whole suite, targeted suites, or simply
validate deterministic artifacts.  Never rerun expensive work for ceremony,
but never substitute a prior branch run for required post-merge evidence.

## 8. Independent review patterns

### Single independent reviewer

Good default for persistence, public contracts and significant cross-module
implementation.

### Two independent reviewers

Useful for security boundaries, closure claims, destructive behavior, or a
high-impact change where model-specific blind spots matter.

To preserve independence:

- give both the same immutable branch/commit;
- do not show reviewer A's verdict to reviewer B;
- distinguish a formal reviewer from an adversarial reviewer when useful;
- reconcile only after both reports are complete.

### Reconciliation

Create one final finding matrix.  Resolve differences using primary source and
reproduction, not majority vote.

Two models agreeing is stronger evidence, not authority by itself.

## 9. Merge and post-merge

When authorized to merge:

1. verify `main` has not drifted unexpectedly;
2. merge the exact reviewed/submitted commit;
3. record the resulting merge commit and parents;
4. run the required post-merge suite on `main`;
5. verify tracked/index cleanliness;
6. check canonical planning/version state;
7. only then report the task integrated.

Do not silently delete the task branch/worktree immediately after merge.  Cleanup
can be a separate deterministic step after acceptance, especially when audit
evidence may still need the exact worktree/branch.

## 10. Handling test interference

Parallel agents can create environmental interference, especially tests that use
global process discovery, ports, sockets or user-level daemon state.

When a test fails during concurrent activity:

1. preserve the failure output;
2. identify whether the test shares system-global state;
3. inspect active processes/worktrees rather than guessing;
4. rerun the smallest reproducer in isolation if justified;
5. classify the result as product defect, test-isolation defect, environment
   issue, or unresolved failure;
6. never convert an unexplained red test into green by narrative.

## 11. Architecture decisions and backlog

The orchestrator keeps three categories distinct:

- **implementation blocker** — must be resolved before task acceptance;
- **architecture decision** — requires ADR/contract work, not an improvised patch;
- **accepted backlog/residual** — explicit non-blocking follow-up with rationale.

Do not let backlog items silently disappear after a successful merge.  Equally,
do not repeatedly reopen a closed gate for a consciously deferred architecture
decision unless its threat model/assumptions changed.

## 12. Durable conversational intent

If the user makes a durable decision in chat, such as:

- a new architecture rule;
- a changed product priority;
- a security/trust-model decision;
- a gate/task sequencing decision;

the orchestrator should identify the correct repository owner (ADR, charter,
roadmap, queue, design contract, etc.) and propose promotion there.

Do not rely on "the assistant will remember" as project governance.

## 13. Recommended user workflow

### Start a fresh orchestrator

Open the repository root and send:

```text
Read AGENT.md and ORCHESTRATOR.md. Act as the Runlith Engineering Orchestrator.
Rehydrate the current project state from repository evidence, show me the
startup snapshot, and continue from the highest-priority canonical next action.
```

### Give it a finished agent report

Paste the report and say:

```text
Reconcile this report against the actual branch/worktree and canonical task.
Do not accept its claims without verification. Tell me whether it is ready for
independent review, remediation, or merge.
```

### Authorize merge

Use explicit wording:

```text
You have merge authority for this verified task. Merge the exact reviewed HEAD
into main, run the required post-merge verification, reconcile canonical state,
and report what should happen next.
```

This keeps integration authority intentional and auditable.
