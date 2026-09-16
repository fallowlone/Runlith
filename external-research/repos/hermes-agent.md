# NousResearch/hermes-agent

## Source revision

- repository: `NousResearch/hermes-agent`
- repository_url: `https://github.com/NousResearch/hermes-agent`
- default_branch: `main`
- reviewed_revision: `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`
- exact_git_tag: none at the reviewed commit
- project_version: `0.21.3` (`pyproject.toml`)
- review_date: `2026-09-15`
- review_scope: architecture, durable state, orchestration, provider fallback, recovery, tool authority, host execution, memory/context, scheduled work, security boundaries, and overlap with current Runlith architecture

All repository-specific claims in this report refer to the frozen revision above. README-level descriptions are supplemental; material findings below are tied to implementation evidence.

## Executive summary

Hermes Agent is useful to Runlith primarily as implementation evidence for mature local-agent operational mechanics. Its strongest areas are durable Kanban task/run bookkeeping, stale-worker recovery, scheduled-job fencing, provider fallback identity, durable conversational recovery, and several practical security guards around MCP tools, paths, webhooks, browser access, and child environments.

The largest architectural lesson is the separation between Hermes' durable Kanban lifecycle and its process-local subagent lifecycle. Kanban tasks and runs are persisted, fenced and recovered. Subagent handles explicitly cannot reconnect after process restart. This independently validates Runlith's `Task != Execution != Attempt != Session` hierarchy and its rule that provider/delegation state is never durable workflow truth.

Two Hermes mechanisms expose real unresolved Runlith design gaps rather than duplicate existing architecture:

1. **Failure-scope-aware provider fallback.** Hermes distinguishes failures that invalidate a model/deployment, a credential surface, or an endpoint and uses backend identity to avoid retrying the same failed surface. Runlith already designs an explainable `RoutePlan`, provider neutrality and failover attempts, but the Adaptive Model Router has no accepted ADR and no typed failure-scope invalidation contract yet.
2. **Repeated blocker recurrence escalation.** Hermes counts recurrence of the same blocker and routes repeated cycles to triage instead of allowing an unblock/reblock loop forever. Runlith already forbids unbounded planning/retry loops, but the Task contract still explicitly leaves typed blocker taxonomy and retry-budget ownership/exhaustion unresolved.

Several other Hermes ideas are valuable external validation rather than new scope: durable Task/Run separation, expected-run fencing, heartbeat/stale claim recovery, durable scheduled routines, session restart markers, least-trust MCP write classification, environment credential scrubbing, stable prompt-prefix/cache awareness, and read-before-write/provenance controls for autonomous skill maintenance.

Several Hermes semantics should not be adopted by Runlith. In particular, generated memory is inserted into prompt context with language calling it "authoritative reference data"; goal-completion judging allows lifecycle handoff when the judge itself errors; memory/skill write-approval wrappers explicitly fail open when the gate module cannot be loaded; and the optional Tirith scanner defaults to fail open. These are source-confirmed policy choices or failure behaviors, not claims of practical exploitability. They conflict with Runlith's stronger authority and verification invariants.

## Project purpose

Hermes Agent is a local AI-agent runtime with multiple interaction surfaces, provider integrations, tool execution, delegation/subagents, a persistent Kanban work system, scheduled jobs, memory/skills, gateway sessions and background learning/review. The reviewed codebase mixes durable product state with deliberately process-local convenience state depending on subsystem.

For Runlith research, the relevant value is not the feature list. It is the concrete behavior around ownership, failure recovery, retry scope, fencing, approval gates, restart semantics and the boundaries between model output and durable operational state.

### Repository reconnaissance coverage

- **Primary runtime/manifests:** Python project in `pyproject.toml` / `uv.lock` (`requires-python >=3.11,<3.14`) with exact-pinned core dependencies and provider-specific optional extras; root Node workspace in `package.json` spans desktop/web/TUI/test packages.
- **Runtime/entry surfaces:** `run_agent.py`, `cli.py`, `gateway/`, `hermes_cli/`, `agent/`, `tools/`, `cron/`, `tui_gateway/`, `apps/desktop/`, `web/` and provider/plugin packages were mapped before selecting source-level paths for deeper review.
- **Persistence/state:** Kanban SQLite code, the split `hermes_state*` session/state modules, cron state, gateway recovery and in-memory subagent lifecycle were examined as separate ownership domains rather than inferred from README descriptions.
- **Tests/CI/deployment/docs:** the repository contains a large Python/JS test tree, Docker/Nix deployment paths, and 35 GitHub workflow files at the frozen revision, including OS test matrices, desktop E2E, lockfile checks, OSV/supply-chain scanning, Rust/JS tests and install E2E. Developer docs cover the agent loop, sessions, cron, provider runtime, plugins, tools and gateway internals; implementation evidence remains primary in this report.

## Architecture map

```text
Repository: NousResearch/hermes-agent
├── Interfaces
│   ├── CLI / TUI / desktop-facing commands
│   ├── Gateway platform adapters
│   └── Webhook and messaging integrations
├── Orchestration
│   ├── Main agent turn / tool loop
│   ├── Durable Kanban worker lifecycle
│   ├── Process-local subagent lifecycle
│   ├── Cron / scheduled jobs
│   └── Background memory / skill review
├── Domain
│   ├── Kanban Task
│   ├── Task Run / claim / blocker / review state
│   └── Goal-mode completion handoff
├── State/Persistence
│   ├── Kanban SQLite state + task_runs + events
│   ├── Session SQLite transcript / routing state
│   ├── File-backed cron jobs state
│   └── In-memory subagent registry
├── Providers
│   ├── Provider/model registry
│   ├── Chat-completion transports
│   ├── Credential pools / cooldowns
│   └── Backend identity + fallback selection
├── Tools/Capabilities
│   ├── Tool registry
│   ├── MCP tool discovery / trust gating
│   ├── Memory / skill management
│   ├── Browser / file tools
│   └── Delegation tools
├── Execution
│   ├── Local trusted-host shell environment
│   ├── Tool dispatch
│   └── Delegated child agents
├── Recovery
│   ├── Stale Kanban claim reclamation
│   ├── Session active-turn / resume recovery
│   ├── Scheduled occurrence claims / stale recovery
│   └── Provider retry / credential rotation / fallback
├── Security
│   ├── Approval system + optional Tirith scan
│   ├── MCP trust/write classification
│   ├── Path containment / browser SSRF checks
│   ├── Webhook authentication
│   └── Environment / credential scrubbing
└── Observability
    ├── task_runs and Kanban events
    ├── heartbeat / claim timestamps
    ├── session lifecycle markers
    ├── provider retry/fallback diagnostics
    └── logs / usage / background-review summaries
```

Hermes does **not** have one global canonical state owner. Canonical ownership is subsystem-specific:

- durable board/workflow state: Kanban SQLite (`tasks`, `task_runs`, links/comments/events and claim metadata);
- durable conversational transcript/routing state: session SQLite;
- durable scheduled-job state: file-backed cron job records plus occurrence claims;
- process-local delegation state: `SubagentLifecycleService` in-memory registry.

This split is coherent inside Hermes, but it means cross-subsystem semantics depend on explicit bridges rather than one universal transaction boundary.

## Canonical state ownership

### Kanban work

The Kanban database is the durable owner of board task status, claim/run identity, blockers, retries/failure counters, heartbeats and run history. A `Run` is historical execution-attempt state rather than the Task itself. Claiming work opens a run record and pins `current_run_id`; completion/block/review paths can require the expected run id so a stale worker cannot casually conclude a newer owner's work.

### Conversational sessions

Session persistence is separate from the Kanban board. Transcript appends are batched transactionally; active-turn markers and `resume_pending` are persisted for crash recovery. Routing recovery additionally checks profile and source/workspace scope before rebuilding in-memory routing entries.

### Scheduled jobs

Cron state is a third ownership domain. Recurring occurrences have explicit scheduling and claim semantics. For recurring jobs, `next_run_at` is advanced before execution, intentionally preferring a possible missed run after a crash over a duplicate crash-loop burst.

### Delegated subagents

The subagent registry is intentionally process-local. The service documentation states that children run in process and a serialized handle cannot reconnect after restart. This state is therefore not a durable workflow authority.

## Strong engineering decisions

### HERM-F01 — Durable Task / Run separation with transactional ownership fencing

**finding_id:** `HERM-F01`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `hermes_cli/kanban_db.py` — `Task`, region around `669+`; `Run`, region around `753+`; schema around `849+`.
- `hermes_cli/kanban_db.py` — claim/run creation, region around `2160+`.
- `hermes_cli/kanban_db.py` — `complete_task`, region around `2602+`; `request_review`, region around `3090+`.
- `hermes_cli/kanban_db_connect.py` — `write_txn`, region around `1157+`.

**observation:** Task state and run history are distinct. Claiming work creates a run and records the current run. Lifecycle paths re-check mutable prerequisites inside the write transaction, can fence against `expected_run_id`, close the run and append lifecycle evidence in the same bounded transaction. The transaction helper uses `BEGIN IMMEDIATE`, bounded busy retries, rollback discipline and rejects accidental nested writes unless an explicit savepoint is used.

**confidence:** `HIGH`

**Problem:** stale workers, concurrent claims and multi-step lifecycle transitions can corrupt durable task truth if ownership and state checks are not part of the authoritative write boundary.

**Mechanism:** durable run identity + expected-run fencing + write-time precondition recheck + bounded SQLite transaction.

**Why it works:** a process-local belief that it owns work is insufficient; the current durable run id participates in the mutation decision.

**Limitations:** SQLite gives strong local serialization but is not by itself a multi-machine consensus model. Hermes' wider state remains split across other persistence domains.

**Runlith overlap:** `ALREADY_IMPLEMENTED` / `EXTERNAL_VALIDATION`. Runlith already separates Task/Execution/Attempt/Session and has atomic execution entry, durable attempt identity, workspace ownership and journal coupling. Hermes is independent evidence for the same hierarchy and fencing principle.

### HERM-F02 — Real runtime activity refreshes durable worker liveness

**finding_id:** `HERM-F02`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `tools/kanban_tools.py` — runtime-activity heartbeat bridge, `415-463`; `heartbeat_current_worker_from_env`, `439+`.
- `hermes_cli/kanban_db.py` — stale-claim release/recovery, region around `2329+`.

**observation:** normal runtime/tool activity is mirrored into the board heartbeat so the model does not need to remember to call a heartbeat tool. `HERMES_KANBAN_RUN_ID` pins the update to the current run. Heartbeat writes are rate-limited and best effort. Stale claim recovery considers claim/heartbeat freshness and worker identity evidence; reclaim contributes to failure accounting.

**confidence:** `HIGH`

**Problem:** a healthy worker can be falsely reclaimed if durable liveness depends on a model deciding to emit a special heartbeat action.

**Mechanism:** infrastructure observes real work and refreshes the durable claim using the current run fence.

**Why it works:** liveness comes from the runtime that observes execution rather than from generated text.

**Limitations:** the bridge is best effort, so lack of heartbeat is still evidence requiring recovery policy rather than proof of death.

**Runlith overlap:** `PARTIALLY_EXISTS`. Runlith already has a real `LiveProcessTracker` behind provider-neutral `OwnerLiveness`, but that tracker is in-memory and currently protects workspace cleanup. Post-attempt crash/orphan recovery is explicitly deferred to G11. Hermes is useful G11/recovery design input, not a reason to create a second liveness subsystem now.

**Decision:** `ADAPT` as a future recovery mechanism; no standalone immediate task.

### HERM-F03 — Repeated same-blocker recurrence escalates to triage

**finding_id:** `HERM-F03`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**file:** `hermes_cli/kanban_db.py`

**symbol:** `_route_block`

**region:** `3048-3072`

**observation:** Hermes preserves blocker kind/recurrence across unblock cycles. Repeating the same blocker increments recurrence; reaching the configured limit routes the task to `triage` with `block_loop_detected` instead of returning to the ordinary blocked state indefinitely.

**confidence:** `HIGH`

**Problem:** unblock/retry/reblock can become an infinite autonomous loop even when every individual transition is legal.

**Mechanism:** stable blocker identity + recurrence counter + explicit escalation state/reason.

**Why it works:** loop detection is based on durable lifecycle evidence rather than the agent's self-assessment.

**Limitations:** choosing the blocker taxonomy, reset rules, escalation target and budget is product policy. A raw recurrence number should not be copied into Runlith without deciding those semantics.

**Runlith overlap:** `NEEDS_ADR`. Runlith already forbids unbounded planning/retry loops (`INV-050`, `INV-091`), and G7 has planning-specific numeric liveness guards. However, `docs/architecture/task-engine.md` explicitly leaves typed blocker taxonomy and retry-budget ownership/exhaustion open. Runtime blocker recurrence is therefore a real unresolved gap.

**Decision:** `ADAPT`.

### HERM-F04 — Scheduled occurrence claim/fence with explicit at-most-once tradeoff

**finding_id:** `HERM-F04`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**file:** `cron/jobs.py`

**symbols / regions:** `_jobs_lock` around `272+`; `_fire_job_lock` around `326+`; `_save_jobs_unlocked` around `1467+`; `advance_next_run` `2682-2687`; `claim_job_for_fire` `2704+`.

**observation:** scheduler state uses process and cross-process locking, a separate per-job fire fence, atomic file replacement and occurrence-aware claims. Recurring jobs advance `next_run_at` before execution; the source explicitly describes the choice as at-most-once for recurring jobs where one missed run is preferred to a duplicate crash-loop burst. Claim state distinguishes manual execution from a scheduled occurrence.

**confidence:** `HIGH`

**Problem:** restarts and concurrent scheduler instances can double-fire work or repeatedly replay one occurrence.

**Mechanism:** durable occurrence identity + claim TTL/token + fire fence + pre-execution schedule advancement.

**Why it works:** the scheduler chooses and documents a delivery semantic rather than accidentally inheriting one from timing.

**Limitations:** pre-advancing the schedule permits missed recurring work after a crash. File-backed scheduler state is appropriate locally but eventually becomes a scalability boundary for larger distributed deployments.

**Runlith overlap:** `ALREADY_DESIGNED` at the architectural level. G11 already defines scheduled/event-driven routines as durable `RoutineDefinition` / `RoutineRun` workflows with capability, budgets, evidence and escalation. Hermes supplies concrete implementation evidence for a future G11 ADR; it does not justify a duplicate current task.

**Decision:** `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` for the exact delivery mechanics.

### HERM-F05 — Provider fallback invalidates the failed identity surface, not every candidate

**finding_id:** `HERM-F05`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `agent/backend_identity.py` — `FailureScope`, `23+`; `BackendIdentity`, `40+`; `should_skip_candidate`, `127+`; `classify_failure_scope`, `151+`.
- `agent/chat_completion_helpers.py` — backend identity filtering around `1750+`.
- `agent/auxiliary_client.py` — failure-scope-aware candidate filtering around `3927+` and endpoint classification around `7138+`.
- `agent/agent_runtime_helpers.py` — primary transport rebuild/recovery around `917-975`, turn-scoped primary restore around `1121-1166`.
- `agent/anthropic_adapter.py` — SDK retry disabled so outer policy owns retry, regions around `314-318` and `433+`.

**observation:** Hermes models backend identity using provider/model/base-url information and classifies a failure by the identity dimension it invalidates. Candidate fallback filtering can therefore avoid repeating the failed deployment, credential surface or endpoint without automatically discarding unrelated alternatives. Provider SDK retry is disabled on reviewed adapter paths so the outer orchestration owns retry/fallback behavior and cooldown accounting.

**confidence:** `HIGH`

**Problem:** naive fallback either retries the same broken backend under a different alias or overreacts by excluding healthy alternatives that merely share one non-failed attribute.

**Mechanism:** typed failure scope + normalized backend identity + scope-specific equivalence predicates + centralized retry ownership.

**Why it works:** failure evidence invalidates only the part of routing state supported by that evidence.

**Limitations:** Hermes' exact taxonomy is implementation-specific. Runlith also needs policy/capability/privacy eligibility, durable Attempt boundaries, side-effect ambiguity handling and explainable RoutePlan provenance.

**Runlith overlap:** `NEEDS_ADR`. Runlith already defines provider neutrality, `RoutePlan`, hard eligibility gates, provider degradation as explicit runtime state and the rule that stateful failover creates a new/recovered Attempt (`INV-059..068`, `INV-085`). The Adaptive Model Router remains a deferred architecture target without an ADR, and no typed failure-scope invalidation contract was found.

**Decision:** `ADAPT`.

### HERM-F06 — Durable conversational recovery is scoped by identity and origin

**finding_id:** `HERM-F06`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `agent/session_persistence.py` — transactional batch flush, `216+`; compression-tip adoption, `228+`; background-review persistence isolation, `360-362`.
- `gateway/session_lifecycle.py` — persisted active-turn markers, `115+`; `recover_interrupted_turns`, `152+`; freshness window, `43+`; `suspend_recently_active`, `245+`.
- `gateway/session_recovery.py` — active-profile gate, `67+`; source/workspace scope check, `118+`; recoverable-row gating, `210-236`.

**observation:** transcript writes for one turn are batched transactionally; ephemeral recovery scaffolding is excluded from durable history. Gateway state persists an active-turn token before publishing it in memory. On unclean startup, sufficiently fresh interrupted turns can become `resume_pending`; suspended sessions are not re-armed. Recovered routing rows are rejected when profile/source scope is incompatible.

**confidence:** `HIGH`

**Problem:** restart recovery can accidentally attach stale or foreign conversation state to the wrong profile/workspace, or persist synthetic recovery scaffolding as if it were user history.

**Mechanism:** durable turn markers + freshness policy + scope checks + transactional transcript append.

**Runlith overlap:** largely `ALREADY_DESIGNED` via provider-independent Context Continuity and provider Session/Epoch separation. Hermes is implementation evidence for recovery fencing and scope checks. It does not replace Runlith's stronger requirement that provider/session state remains non-authoritative.

**Decision:** `EXTERNAL_VALIDATION`.

### HERM-F07 — MCP write capability defaults conservatively before transport

**finding_id:** `HERM-F07`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `tools/mcp_tool.py` — trust-tier/write-capability gate, region around `466+`; transport/lazy-spawn gate ordering, region around `516+`.
- `tools/registry.py` — schema validation and tool-shadow/override ownership checks, region around `623-680`.

**observation:** for an untrusted MCP server, a tool is considered read-only only when `readOnlyHint` is explicitly `True`; missing/malformed metadata is treated as write-capable. Write-capable untrusted calls require approval before transport/lazy spawn. Tool registry code rejects malformed schemas and requires explicit override/ownership rules for shadowing and deregistration.

**confidence:** `HIGH`

**Problem:** optimistic interpretation of capability metadata can turn malformed or omitted metadata into authority.

**Mechanism:** least-trust default + approval before external execution + registry ownership rules.

**Runlith overlap:** `ALREADY_IMPLEMENTED` conceptually in G6 typed Policy/approval authority and `ALREADY_DESIGNED` for future Tool/Skill registry details.

**Decision:** `EXTERNAL_VALIDATION`.

### HERM-F08 — Child environment construction actively strips ambient authority

**finding_id:** `HERM-F08`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `tools/environments/local.py` — trusted-host shell execution and subprocess environment construction.
- `tools/environments/local_env_policy.py` — always-strip and dynamic secret patterns.
- `tools/environments/bash_snapshot.py` — session-variable exclusion and atomic restricted-permission snapshot publication.

**observation:** local execution is intentionally a trusted-host shell capability. Before child/delegated execution, Hermes strips internal/provider credentials and high-value secret classes; session-scoped environment values override stale process state. Shell snapshots avoid persisting per-session identity and are published atomically with restrictive permissions.

**confidence:** `HIGH`

**Problem:** trusted-host tools can inherit unrelated provider credentials, stale session identity and internal control variables through ambient environment state.

**Mechanism:** explicit environment allow/strip policy + session overlay + careful shell snapshot hygiene.

**Runlith overlap:** provider-environment isolation and Secrets Broker/Runner boundaries are already implemented more strongly in G6. Hermes validates ambient-environment stripping as defense in depth, while Runlith still requires governed Runner isolation for autonomous high-risk execution.

**Decision:** `EXTERNAL_VALIDATION` for env hygiene; `REJECT` trusted-host shell as sufficient isolation for Runlith's high-risk execution boundary.

### HERM-F09 — Autonomous skill curation has ownership provenance and read-before-write guards

**finding_id:** `HERM-F09`

**source:** implementation

**repository:** `NousResearch/hermes-agent`

**revision:** `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`

**files / symbols / regions:**

- `agent/background_review.py` — background fork purpose and cache isolation, `1-6`; protected-skill/read-before-write contract around `391-446`; persistence isolation around `932-944`; dispatch whitelist around `1025-1113`.
- `tools/skill_manager_guards.py` — background-review ownership/provenance and exact-target read-before-write guards, region around `164-230`.

**observation:** background review runs in an isolated fork, with persistence to the user's main transcript disabled and a dispatch-side tool whitelist. Autonomous curation is restricted to curator-managed skills; bundled, externally owned, hub-installed, pinned and user-owned skills are protected. Existing targets must be read during the review before mutation, and provenance lookup failures are handled conservatively in the guard layer.

**confidence:** `HIGH`

**Problem:** an unattended learning process can silently rewrite user-owned instructions or mutate stale content it never inspected.

**Mechanism:** provenance ownership + protected classes + exact-target read-before-write + bounded/isolated background fork.

**Runlith overlap:** `PARTIALLY_EXISTS` at architecture level. Runlith already says learned notes are untrusted until validated and skills cannot grant authority (`INV-029`, `INV-090`), but the future Tool/Skill Registry and G10 promotion flows do not yet have an accepted autonomous-curator write contract.

**Decision:** `INSPIRED_ONLY`. Preserve the ownership/read-before-write principle, but Runlith learning must remain provenance-bearing and untrusted until explicit validation/promotion policy permits use.

## Implementation techniques

### Bounded SQLite write transactions

`write_txn` centralizes `BEGIN IMMEDIATE`, bounded lock contention retries, rollback and nested-transaction discipline. The useful principle for Runlith is already familiar from its atomic execution-entry and journal writes: preflight reads are diagnostics; the correctness check belongs inside the write boundary.

### Expected-run fencing

Lifecycle functions that may be called by a worker can compare the persisted current run id with the caller's expected run. This avoids granting a stale process mutation authority simply because it still knows the task id.

### Automatic activity-to-heartbeat bridge

Tool/runtime activity refreshes worker liveness without prompt instructions. This removes a class of model compliance failure from recovery correctness.

### Explicit delivery semantics for cron

The recurring scheduler chooses at-most-once behavior deliberately and documents the consequence: a crash can lose an occurrence, but it will not create a duplicate burst. This is a strong design habit even where Runlith eventually chooses a different semantic for a specific routine class.

### Central retry ownership

Reviewed provider adapters disable SDK-level retries when the outer agent loop owns retry/fallback/cooldown. This avoids nested invisible retry layers that make cost, latency and failure accounting hard to explain.

### Durable transcript markers instead of inference from recent text

The gateway persists active-turn state and explicit resume markers. Recovery does not infer an interrupted turn merely because a transcript ends in an odd shape.

### Internal-context streaming scrubber

`agent/memory_manager.py` contains a stateful scrubber for `<memory-context>` spans that can straddle streamed chunks and discards unterminated internal spans. The scrubbing technique is useful for keeping internal context markers out of user output, but it does not change the trust problem described below.

## AI / orchestration architecture

### Who owns workflow state?

For Kanban work, SQLite-backed Task/Run records own durable lifecycle. The model can invoke lifecycle tools, but durable state is mediated by database operations and worker/run guards. Conversational session state and scheduled-job state live in separate durable subsystems. Process-local subagent state is explicitly non-durable.

### Who owns completion authority?

For ordinary Kanban operations, lifecycle functions own the database transition. In goal mode, an auxiliary goal judge can gate completion/review handoff. However, the reviewed `_goal_gate` catches judge exceptions and allows the lifecycle handoff. Thus completion authority is not consistently fail-closed against verifier unavailability.

### Can the model mutate canonical state?

Yes, through exposed Kanban/memory/skill tools when its worker identity and tool guards allow the operation. This is not equivalent to direct database access, but generated tool calls can request durable mutations. Hermes compensates with worker ownership guards, expected run ids and subsystem-specific approval/curation checks.

Runlith's authority model remains stricter: providers/models propose or produce evidence; trusted application services, Policy and Verification own authoritative state changes.

### Can model output directly cause privileged action?

Tool calls can cause host shell, file, browser, MCP and persistent-memory/skill actions subject to each tool's approval/trust policy. The local shell environment is intentionally a trusted-host capability rather than a strong OS-isolation boundary.

### What happens after crash?

- Kanban stale claims can be reclaimed based on durable lease/heartbeat and worker identity evidence.
- Gateway sessions persist active-turn markers and can mark recent interrupted sessions `resume_pending`.
- Cron records can recover stale claims/occurrences according to scheduler rules.
- Process-local subagent handles cannot reconnect after process restart.

### What happens after provider timeout/failure?

The outer retry/fallback layer owns bounded retry behavior. Failure identity can influence which fallback candidates are skipped. Provider/credential cooldowns and transport rebuild logic avoid immediately repeating some failed surfaces.

### How does execution resume?

Resume behavior is subsystem-specific: Kanban claims/runs, session resume markers and cron claims each carry their own state. There is no single provider-neutral `ContinuationCheckpoint` equivalent spanning all Hermes execution. Runlith already chooses a stronger unified continuity concept for provider-independent execution history.

## Security analysis

### HERM-S01 — Goal judge failure permits lifecycle handoff

**classification:** `CONFIRMED`

**source evidence:** `tools/kanban_tools.py`, `_goal_gate`, region `395-408`; goal-mode block restrictions around `629-640`.

**observation:** when the goal judge is available and returns not-done, lifecycle handoff is rejected. If the judge raises, Hermes logs the failure and explicitly allows the handoff. Goal-mode block kinds are separately restricted to avoid using arbitrary blocking as an escape hatch.

**security/reliability meaning:** verifier availability can change whether an agent is allowed to hand off completion/review state. This is a fail-open completion-gate behavior. No exploitability claim is made; it is a confirmed control-flow policy.

**Runlith decision:** `REJECT`. Independent Verification authority may return blocked/unknown/unavailable, but verifier failure cannot silently become completion acceptance.

### HERM-S02 — Memory/skill write-approval module/config failure can disable the gate

**classification:** `CONFIRMED`

**source evidence:**

- `tools/memory_tool.py:66-73` — `_gate_or_stage` documentation says it fails open if the gate module cannot load; import failure returns `None` and proceeds.
- `tools/skill_manager_tool.py:583-591` — write gate explicitly fails open if `write_approval` cannot be imported.
- `tools/write_approval.py:43-49` — unset/invalid approval configuration means gate off; load failure resolves to disabled behavior.

**observation:** write approval is optional policy for these stores and is not a mandatory fail-closed security boundary under module/config failure.

**security/reliability meaning:** if an operator expects the approval gate to protect durable learned-state writes, enforcement availability/configuration becomes a hidden prerequisite. Exploitability through a specific import/config manipulation path was not established.

**Runlith decision:** `REJECT` for any security-sensitive durable learned-state mutation. Policy/approval unavailability must not become implicit allow.

### HERM-S03 — Optional Tirith scanner defaults fail open

**classification:** `CONFIRMED`

**source evidence:** `tools/approval_context.py:308-315`; `tools/approval.py` regions around `617-646` and `1092-1108`.

**observation:** `security.tirith_fail_open` defaults to true and unreadable configuration resolves to fail-open behavior. Both unattended and interactive Tirith integration paths honor that switch.

**important nuance:** Tirith is an optional scanner layer. Hermes' broader dangerous-command approval path has separate controls, so this finding must not be described as a universal approval bypass.

**Runlith decision:** `REJECT` if such a scanner becomes part of a canonical mandatory security gate. An optional advisory scanner may degrade explicitly, but the UI/audit must represent degradation honestly.

### HERM-S04 — Generated memory is promoted textually to authoritative reference context

**classification:** `CONFIRMED` architecture behavior

**source evidence:** `agent/memory_manager.py:163-187` (internal memory-context fencing/scrubbing) and `274-279` (prefetched memory injected with instruction text calling it "authoritative reference data").

**observation:** the implementation carefully prevents internal memory-context tags from leaking to streamed user output, yet the prompt wrapper explicitly promotes recalled memory text epistemically.

**security/reliability meaning:** poisoning/staleness in durable memory can receive stronger downstream influence than its provenance warrants. The streaming fence solves output leakage, not trust promotion.

**Runlith decision:** `REJECT` the authoritative-memory semantics; `ADAPT` the streaming scrubber technique if needed. Runlith `INV-029` and `INV-038` require learned/external content to remain untrusted until validated and never automatically gain instruction authority.

### HERM-S05 — Legacy generic V1 webhook authentication has replay weakness

**classification:** `CONFIRMED` documented weakness

**source evidence:** `gateway/platforms/webhook.py`, authentication/startup regions around `197-206`, signature checks around `445-467`, HMAC/version/timestamp verification around `703-753`.

**observation:** webhook secret is required except an explicitly loopback-only insecure mode. Modern timestamped variants enforce freshness and V2 cannot downgrade to V1. The legacy generic V1 body-only HMAC path is explicitly acknowledged as replay-vulnerable because it lacks timestamp freshness.

**Runlith lesson:** webhook/event authenticity and freshness are separate properties. A valid signature proves origin/integrity for the signed material; it does not by itself prove that the event is fresh or may mutate canonical state.

### NOT_SUPPORTED findings from inspected paths

No new broad source-confirmed vulnerability was established for the following classes in the reviewed paths:

- generic path traversal: `tools/path_security.py` resolves symlinks/`..` and checks containment;
- generic browser SSRF: browser policy detects private/internal classes, blocks metadata endpoints, and performs a post-redirect SSRF check before returning content;
- webhook missing-secret/auth bypass on non-loopback: startup/verification paths fail closed;
- arbitrary tool-registry shadowing by default: registry override/ownership checks exist;
- shell injection merely because `LocalEnvironment` executes shell: arbitrary shell is an intentional capability. The material security question is the authority/isolation granted to that capability, not whether shell syntax is interpreted.

## Reliability / recovery analysis

### Kanban stale claims

Hermes combines durable claim/run identity, heartbeat freshness and worker/process evidence. Stale reclamation is not treated as a clean success; it contributes to failure accounting. This is useful because repeated worker disappearance should influence recovery/escalation policy rather than look like harmless scheduler churn.

### Block recurrence

Block-loop recurrence is tracked separately from ordinary dispatcher failures. This distinction matters: "the same external blocker keeps returning" and "the provider process repeatedly crashes" are different evidence classes and should not consume the same budget blindly.

### Cron crash behavior

Recurring jobs intentionally accept possible omission to prevent duplicate crash-loop re-fire. This is a good explicit tradeoff, but Runlith will need per-routine delivery policy rather than one universal semantic because some future routines may require reconciliation or at-least-once processing.

### Session recovery

Active-turn markers, freshness and suspension semantics make restart behavior inspectable. Recovery also validates source/profile scope, reducing the risk that durable state is resumed in an incompatible routing context.

### Subagent lifecycle

`SubagentLifecycleService` explicitly documents that children run in process and serialized handles cannot reconnect after restart. This is honest behavior, but it means subagent state cannot serve as product-level durable continuity. Runlith should keep delegated provider sessions subordinate to durable Task/Execution/Attempt state.

### Provider fallback

Failure-scope identity is the strongest reliability mechanism that appears genuinely missing from Runlith's accepted contracts. It provides a principled bridge between observed failure and next-candidate selection without collapsing all failures into one generic retryable/nonretryable bit.

## Performance / scalability analysis

### Prompt cache stability

Hermes deliberately keeps some memory/session mutations out of the active prompt and uses isolated background-review forks so the foreground prefix can remain stable. Runlith's Source of Truth already recognizes provider prompt-cache economics and calls for stable prefixes/cache epochs. Hermes is implementation validation, not a new architectural requirement.

### Retry cost visibility

Disabling SDK retry when the outer loop owns retry avoids multiplicative retries whose latency/cost are invisible to product policy. Runlith should preserve one observable owner for retry/fallback decisions at each layer.

### SQLite/local-first strengths and limits

Kanban SQLite transactions are a good fit for local coordination and give simple durable serialization. Cross-machine scheduling/claims would eventually require stronger distributed ownership semantics than local SQLite/file locks.

### File-backed cron state

Hermes adds careful locking, merge-on-save and atomic replace safeguards, which make a local JSON scheduler much safer than a naive file. Nevertheless, whole-file state and OS locks become a scaling boundary as job count, write frequency or multi-host coordination grows. Runlith's G11 routine persistence should remain behind a domain contract rather than expose file semantics as architecture.

### Background review cost

Autonomous post-turn review consumes model/tool budget beyond the foreground task. Hermes bounds/isolates that work and gives foreground activity cancellation priority. Runlith should require measured benefit before introducing automatic learning/curation and should account for it separately from verified delivery cost.

## Anti-patterns

### HERM-A01 — Verification unavailability becomes permission to hand off completion

**Evidence:** `tools/kanban_tools.py:395-408`.

**Immediate impact:** a judge outage can convert a required check into allowed lifecycle progress.

**Long-term impact:** completion semantics depend on verifier availability rather than evidence.

**Runlith lesson:** verification may be `unknown`, `blocked` or retryable, but cannot fail open into success/handoff.

**Existing Runlith protection:** ADR-0018 independent evaluation; provider/model cannot grant Task completion.

### HERM-A02 — Durable learned-state approval silently disappears on enforcement failure

**Evidence:** `tools/memory_tool.py:66-73`; `tools/skill_manager_tool.py:583-591`; `tools/write_approval.py:43-49`.

**Immediate impact:** a configured operator expectation can diverge from effective behavior if the approval module/config cannot load.

**Long-term impact:** policy enforcement becomes optional implementation plumbing rather than an explicit authority boundary.

**Runlith lesson:** required Policy/approval evaluation is a typed prerequisite and fails closed.

**Existing Runlith protection:** G6 Policy/Approval exact-binding contracts and operation-boundary enforcement.

### HERM-A03 — Generated memory text is promoted to authoritative prompt input

**Evidence:** `agent/memory_manager.py:274-279`.

**Immediate impact:** stale or poisoned learned content can receive more prompt authority than provenance supports.

**Long-term impact:** memory becomes a shadow policy/instruction source.

**Runlith lesson:** memory is selected evidence/knowledge with trust/provenance state; it never automatically becomes instruction authority.

**Existing Runlith protection:** `INV-029`, `INV-030`, `INV-038`, ADR-0012 and Context Engine trust separation.

### HERM-A04 — Process-local delegation lifecycle treated as durable workflow continuity

**Evidence:** `agent/subagent_lifecycle.py`, `SubagentLifecycleService` class documentation around `237+`, explicitly stating in-process-only children and no reconnect after restart.

**Immediate impact if misused:** restart loses the live delegation handle and cannot reconnect.

**Long-term impact if promoted to workflow truth:** recovery becomes process-dependent and provider/runtime-specific.

**Runlith lesson:** process-local delegation may be a convenience layer, but durable continuity lives in Task/Execution/Attempt/checkpoint state.

**Existing Runlith protection:** `Task != Execution != Attempt != Session`; provider-native/session state is non-authoritative.

### HERM-A05 — Optional security scanner fail-open semantics used as if it were a mandatory boundary

**Evidence:** `tools/approval_context.py:308-315`; `tools/approval.py:617-646,1092-1108`.

**Immediate impact if misclassified:** scanner outage is indistinguishable from a clean result at the consuming layer.

**Long-term impact:** operational degradation can silently reduce security assurance.

**Runlith lesson:** optional defense-in-depth may degrade only with an explicit degraded result; mandatory Policy/Runner/Verification boundaries fail closed.

**Existing Runlith protection:** typed security outcomes, audit records and fail-closed mandatory capability/isolation enforcement.

## Runlith overlap analysis

| External idea | External evidence | Closest Runlith equivalent | Current status | Actual gap | Decision |
|---|---|---|---|---|---|
| Durable Task vs Run/attempt separation | `kanban_db.py` Task/Run/schema/claim/complete regions | Task → Execution → Attempt → Provider Session; durable stores | `ALREADY_IMPLEMENTED` | none | `EXTERNAL_VALIDATION` |
| Expected run ownership fencing | `kanban_db.py` completion/review expected-run checks | durable Execution/Attempt identity, atomic entry, workspace ownership/fencing | `ALREADY_IMPLEMENTED` / subsystem-specific | no generic new task | `EXTERNAL_VALIDATION` |
| Runtime activity → durable heartbeat | `kanban_tools.py:415-463`; stale reclaim | `LiveProcessTracker` + `OwnerLiveness`; G11 orphan recovery deferred | `PARTIALLY_EXISTS` | durable post-attempt recovery/liveness policy remains G11 work | `ADAPT` |
| Same-blocker recurrence → triage | `kanban_db.py:3048-3072` | `INV-050`, `INV-091`; Task blocker/retry budget open | `NEEDS_ADR` | blocker identity, recurrence reset/budget/escalation semantics | `ADAPT` |
| Failure-scope-aware provider fallback | `backend_identity.py:23-153` + outer retry/fallback paths | Adaptive Model Router / RoutePlan / `INV-059..068` | `NEEDS_ADR` | typed failure scope + backend invalidation/cooldown evidence | `ADAPT` |
| Durable scheduled occurrence claim/fence | `cron/jobs.py:2682-2716` + locks/save | G11 `RoutineDefinition` / `RoutineRun` | `ALREADY_DESIGNED` | exact delivery/claim policy intentionally future | `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` |
| Session crash markers + scoped recovery | `session_persistence.py`; `gateway/session_lifecycle.py`; `gateway/session_recovery.py` | G9 Context Continuity / checkpoints | `ALREADY_DESIGNED` | implementation mechanics future G9 | `EXTERNAL_VALIDATION` |
| Autonomous skill curation provenance/read-before-write | `background_review.py`; `skill_manager_guards.py` | `INV-029`, `INV-090`; future Tool/Skill registry | `PARTIALLY_EXISTS` / future ADR | unattended curator ownership contract if product later needs it | `INSPIRED_ONLY` |
| Stable prompt prefix / frozen active context | memory/session/background-review code | SoT cache-aware context ordering/epochs | `ALREADY_DESIGNED` | no semantic gap | `EXTERNAL_VALIDATION` |
| Untrusted MCP write default | `mcp_tool.py`, `registry.py` | G6 Policy/Approval + future registry | `ALREADY_IMPLEMENTED` / `ALREADY_DESIGNED` | no semantic gap | `EXTERNAL_VALIDATION` |
| Memory labeled authoritative | `memory_manager.py:274-279` | Memory provenance/trust invariants | `CONFLICTS_WITH_ARCHITECTURE` | none; semantics intentionally stronger in Runlith | `REJECT` |
| Goal judge failure allows handoff | `kanban_tools.py:395-408` | independent Verification authority | `CONFLICTS_WITH_ARCHITECTURE` | none | `REJECT` |
| Write-approval module/config fails open | memory/skill/write-approval tools | typed Policy/Approval enforcement | `CONFLICTS_WITH_ARCHITECTURE` | none | `REJECT` |
| Trusted-host shell as primary autonomous execution boundary | `tools/environments/local.py` | Policy → IsolationPlan → Runner | `SUPERSEDED` for high-risk autonomous execution | env scrubbing remains useful defense-in-depth | `REJECT` as isolation model |
| Process-local subagent handles as durable workflow state | `subagent_lifecycle.py` | durable Task/Execution/Attempt/continuity | `CONFLICTS_WITH_ARCHITECTURE` if promoted | none | `REJECT` as canonical state |

## External validation of existing Runlith design

Hermes independently validates several Runlith decisions:

- durable business/task state should be distinct from transient provider/delegation state;
- stale worker ownership must be fenced by durable identity, not only process-local locks;
- liveness should come from observed runtime activity, not prompt compliance;
- retry and failure accounting should be explicit and bounded;
- scheduled autonomy needs durable occurrence identity and a documented delivery semantic;
- provider SDK retry should not hide retries from the product-level policy owner;
- session recovery needs explicit markers and scope compatibility checks;
- untrusted tool metadata should default to less authority;
- child processes should not inherit unrelated host/provider credentials;
- generated durable memory and provider output need provenance/trust treatment rather than implicit authority;
- process-local subagents are useful execution machinery but not sufficient product-level continuity.

## Novel gaps

### Gap 1 — Provider failure-scope identity in Adaptive Model Router

Runlith has the surrounding architecture but no accepted failure invalidation contract. A future router needs to answer: after one provider/model call fails, which dimensions of the current route are now invalid evidence-wise? A credential error, endpoint outage, model/deployment incompatibility and provider capacity failure should not all invalidate the same candidate set.

**status:** `NEEDS_ADR`

**decision:** `ADAPT`

**meaningful Runlith delta:** make the failure model provider-neutral, Attempt-aware, Policy-aware and replayable through versioned RoutePlan evidence. Fallback never reuses side-effect authority and never silently replays stateful effects.

### Gap 2 — Durable runtime blocker recurrence/retry exhaustion semantics

Runlith already bounds planning loops but explicitly leaves runtime Task blocker taxonomy and retry-budget ownership open. Hermes demonstrates one simple and effective mechanism: preserve blocker identity across unblock, count same-kind recurrence separately from other failures, and escalate instead of looping.

**status:** `NEEDS_ADR`

**decision:** `ADAPT`

**meaningful Runlith delta:** escalation should be owned by Task/Scheduler/Reconciler policy, produce durable evidence, integrate HumanDecisionRequest/Verification/Recovery, and distinguish external blocker recurrence from provider/runner failure and ambiguous side effects.

## Adoption matrix

| Finding | Decision | Runlith status | Why Runlith's version is preferable |
|---|---|---|---|
| Task/Run durability + expected-run fencing | `EXTERNAL_VALIDATION` | `ALREADY_IMPLEMENTED` | provider-neutral Task/Execution/Attempt authority plus independent verification |
| Activity-derived heartbeat | `ADAPT` | `PARTIALLY_EXISTS` | liveness can feed durable G11 recovery without becoming authority or relying on one provider tracker |
| Block recurrence escalation | `ADAPT` | `NEEDS_ADR` | typed blocker evidence, explicit budgets and product-level escalation policy |
| Failure-scope-aware fallback | `ADAPT` | `NEEDS_ADR` | explainable RoutePlan, hard eligibility gates, durable Attempt boundary and side-effect reconciliation |
| Cron occurrence fencing | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | future RoutineRun can choose delivery semantics per routine and retain Policy/evidence/audit contracts |
| Session crash recovery | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | provider-independent checkpoint/epoch continuity rather than a session DB becoming workflow truth |
| MCP least-trust metadata | `EXTERNAL_VALIDATION` | `ALREADY_IMPLEMENTED` / `ALREADY_DESIGNED` | typed Policy/capability decisions and enforced Runner boundary |
| Background skill curator guards | `INSPIRED_ONLY` | `PARTIALLY_EXISTS` | provenance validation and explicit promotion prevent learned content becoming authority |
| Internal memory streaming scrubber | `ADAPT` | future implementation detail | preserves output hygiene while trust remains independently enforced |
| Memory as authoritative reference | `REJECT` | `CONFLICTS_WITH_ARCHITECTURE` | Runlith retains provenance/trust and prevents memory from becoming hidden instructions |
| Goal judge fail-open | `REJECT` | `CONFLICTS_WITH_ARCHITECTURE` | verification outage produces blocked/unknown evidence, never success |
| Learned-state approval fail-open | `REJECT` | `CONFLICTS_WITH_ARCHITECTURE` | mandatory Policy/Approval failures fail closed and remain auditable |
| Trusted-host shell as isolation | `REJECT` | `SUPERSEDED` | governed Runner/OS isolation plus env scrubbing limits effective authority |

## Ideas rejected for Runlith

- Treating generated/persistent memory as authoritative prompt reference data merely because it was saved previously.
- Letting a verifier/judge exception permit a completion/review lifecycle handoff.
- Treating missing/unloadable write-approval policy as equivalent to approval disabled/allow for security-sensitive learned state.
- Using an optional scanner's fail-open result as if the mandatory security boundary succeeded.
- Treating process-local subagent handles as durable orchestration state.
- Treating a trusted-host shell plus environment scrubbing as sufficient containment for high-risk autonomous execution.
- Copying Hermes' exact cron delivery policy globally. Runlith should choose delivery/reconciliation semantics per routine class.
- Copying exact failure-scope enum values into the Task domain. Provider routing failure evidence belongs to the routing/recovery subsystem, not Task identity.

## Candidate ADRs

### ADR-CANDIDATE — Adaptive Model Router failure-scope and fallback invalidation contract

**Problem:** Runlith has a RoutePlan architecture target but no accepted contract mapping provider/runtime failure evidence to candidate invalidation, cooldown and fallback selection.

**Current Runlith state:** provider execution seam implemented; provider-specific failure normalization exists in adapters; Model Router is deferred and requires an ADR before implementation; stateful failover must create a new/recovered Attempt and may not blindly replay side effects.

**External inspiration:** `FailureScope`, `BackendIdentity`, scope-specific candidate comparison and outer retry ownership in Hermes.

**Actual delta:** define provider-neutral failure dimensions and evidence, identity comparison, invalidation/cooldown scope, route-plan provenance, and interaction with retry/recovery/ambiguous side effects.

**Target subsystem:** Adaptive Model Router + Scheduler/Recovery + Provider Adapter normalized failure evidence.

**Security impact:** fallback must re-evaluate Policy/capability/privacy eligibility and never inherit stale authority from a failed provider/session.

**Suggested gate:** `PRE_RELEVANT_GATE` / assign when Adaptive Model Router becomes active.

**Priority:** `HIGH when routing/fallback implementation is scheduled`.

### ADR-CANDIDATE — Runtime blocker taxonomy, recurrence budget and escalation semantics

**Problem:** Task lifecycle allows `RUNNING -> BLOCKED -> RUNNING`, while typed blocker taxonomy and retry-budget ownership/exhaustion remain explicitly unresolved.

**Current Runlith state:** planning loops are bounded; G8 owns verification/recovery contracts; HumanDecisionRequest is being introduced in G7; runtime blocker recurrence has no accepted contract.

**External inspiration:** Hermes preserves blocker kind/recurrence and escalates repeated same-blocker loops to triage.

**Actual delta:** define blocker identity, durable recurrence evidence, reset rules, separate budgets for blocker recurrence vs execution/provider failure, escalation destinations and recovery authority.

**Target subsystem:** Task Engine + Scheduler/Reconciler + G8 Recovery/Human Decision integration.

**Security impact:** repeated failures cannot be converted into wider scope, weaker Policy or silent success; escalation carries no execution authority by itself.

**Suggested gate:** `G8` (with Task-domain ADR ownership before implementation).

**Priority:** `MEDIUM-HIGH before autonomous retry/recovery expands`.

## Candidate implementation tasks

### TASK-CANDIDATE — Failure-scope-aware RoutePlan recovery/fallback

**Type:** `TASK`

**Problem:** future multi-provider routing needs deterministic, explainable reaction to credential/model/endpoint/capacity failures without retrying the same failed surface or over-skipping healthy alternatives.

**Current Runlith state:** no Adaptive Model Router implementation task or accepted ADR yet.

**External inspiration:** Hermes backend identity/failure scope.

**Actual delta:** implement the accepted ADR's normalized failure evidence and route invalidation logic, with durable Attempt/fallback evidence and no side-effect replay.

**Dependencies:** accepted Adaptive Model Router failure-scope ADR; active routing gate; normalized provider failure evidence; existing Policy/Attempt contracts.

**Architecture impact:** moderate; must remain outside Task identity and provider adapters' business semantics.

**Security impact:** re-evaluate eligibility/authority on every route; stale failed session does not carry permissions.

**Acceptance direction:** scope-specific failure invalidates exactly the documented backend identity surface; RoutePlan reasons are replayable; unrelated eligible candidates remain available.

**Verification direction:** deterministic fixtures for model/deployment, credential and endpoint failures; alias/base-url collisions; cooldown expiry; provider switch; policy exclusion; ambiguous side-effect failure must reconcile before retry.

**Suggested gate:** `PRE_RELEVANT_GATE` after ADR activation.

**Priority:** `HIGH when Adaptive Model Router work starts`.

### TASK-CANDIDATE — Durable blocker recurrence accounting and escalation

**Type:** `TASK`

**Problem:** repeated `BLOCKED -> RUNNING -> BLOCKED` cycles can become an autonomous liveness failure once recovery/retry expands.

**Current Runlith state:** Task blocker taxonomy and retry-budget exhaustion are open design items; no implementation should invent them before an ADR.

**External inspiration:** Hermes same-blocker recurrence counter and `block_loop_detected` triage route.

**Actual delta:** after the ADR, persist blocker identity/recurrence under application-service authority and route exhausted recurrence to the decided escalation path with durable evidence.

**Dependencies:** blocker/retry ADR; G8 recovery contract; HumanDecisionRequest integration if selected as escalation target.

**Architecture impact:** Task/Scheduler/Reconciler lifecycle semantics; no provider-specific branches.

**Security impact:** escalation cannot weaken Policy, approval, scope or verification requirements.

**Acceptance direction:** same blocker increments deterministically; distinct blocker/failure classes do not incorrectly share the counter; reset/supersession is explicit; exhaustion never loops silently.

**Verification direction:** crash/restart persistence, concurrent stale worker attempts, repeated same blocker, blocker-kind change, manual resolution, policy blocker, external dependency blocker and provider failure are tested independently.

**Suggested gate:** `G8` after ADR acceptance.

**Priority:** `MEDIUM-HIGH`.

## Source evidence index

All source locations below refer to revision `5341f135a527f2e1ca42a2d011aab5ace3dc49ea`.

- Durable Kanban Task/Run state: `hermes_cli/kanban_db.py` — Task region ~`669+`, Run region ~`753+`, schema ~`849+`, claim/run creation ~`2160+`, stale claims ~`2329+`, completion ~`2602+`, blocker routing `3048-3072`, review request ~`3090+`.
- SQLite transaction discipline: `hermes_cli/kanban_db_connect.py` — `write_txn`, ~`1157+`.
- Runtime activity heartbeat and operator steering: `tools/kanban_tools.py:415-508`; worker lifecycle/goal gate around `190-216`, `395-412`, completion/block around `562+`, `618-646`.
- Provider backend identity/failure scope: `agent/backend_identity.py:23-153`.
- Failure-aware fallback consumers: `agent/chat_completion_helpers.py` around `1750+`; `agent/auxiliary_client.py` around `3927+` and `7138+`; `agent/agent_runtime_helpers.py` around `917-975`, `1121-1166`.
- Central retry ownership / SDK retry disabling: `agent/anthropic_adapter.py` around `314-318`, `433+`; `agent/auxiliary_client.py:176-184`; `agent/client_lifecycle.py:435`; `agent/agent_runtime_helpers.py:1769-1774`.
- Process-local subagent lifecycle and permission narrowing: `agent/subagent_lifecycle.py`, state enum around `36-45`, service documentation around `237+`, allowed-toolset narrowing around `408-416`.
- Durable transcript persistence: `agent/session_persistence.py`, ephemeral scaffolding `28-29`, transaction batch `216+`, compression-tip adoption `228+`, persistence-isolated review `360-362`.
- Gateway session crash recovery: `gateway/session_lifecycle.py:43-60,115-215,245-260`; source/profile-gated recovery: `gateway/session_recovery.py:67-88,118-134,181-236`.
- Background memory/skill review: `agent/background_review.py:1-6`, read-before-write/protected skill contract around `391-446`, persistence isolation `932-944`, dispatch whitelist `1025-1113`; enforcement helpers in `tools/skill_manager_guards.py` around `164-230`.
- Memory internal-context fence and trust wording: `agent/memory_manager.py:163-187,274-279`.
- Learned-state write approval behavior: `tools/memory_tool.py:66-73`; `tools/skill_manager_tool.py:583-591`; `tools/write_approval.py:43-59,73-88`.
- Tirith failure mode: `tools/approval_context.py:308-315`; `tools/approval.py:617-646,1092-1108`.
- MCP least-trust gating: `tools/mcp_tool.py` region around `466-516+`; registry schema/override ownership: `tools/registry.py:623-680`.
- Trusted-host execution/environment isolation: `tools/environments/local.py`; `tools/environments/local_env_policy.py`; `tools/environments/bash_snapshot.py`.
- Path containment: `tools/path_security.py:7-18`; file path anchoring/guards: `tools/file_tools_paths.py`, `tools/file_tools_write_guards.py`.
- Browser SSRF controls: `tools/browser_tool.py:261-291,606-686`.
- Webhook authentication/freshness/idempotency: `gateway/platforms/webhook.py:197-206,445-467,617-622,703-753`.
- Scheduled-job locking, atomic saves and claims: `cron/jobs.py`, `_jobs_lock` ~`272+`, `_fire_job_lock` ~`326+`, `_save_jobs_unlocked` ~`1467+`, `advance_next_run` `2682-2687`, `claim_job_for_fire` `2704+`.
