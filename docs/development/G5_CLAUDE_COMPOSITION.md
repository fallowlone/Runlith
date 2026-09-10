# G5 Claude Composition — Production Wiring Evidence (TASK-0047)

> The real Claude provider behind the real `agentd → application →
> execution → workspace → provider` production path. Authority:
> `docs/architecture/providers.md`, ADR-0009, ADR-0005, ADR-0021/0022/0023;
> contract: `crates/ipc-contracts/proto/execution_core.proto` (v2).
> Verdict: `G5_COMPOSITION_READY` (gate closure belongs to TASK-0048).

## 1. Production path

```text
agentctl execution run/status/cancel
  → ControlRequest.execution_core = 5 / ControlResponse.execution_core = 5
    (protocol v2 only; v0/v1 sessions reject as UNSUPPORTED_PROTOCOL)
  → agentd serve_connection (bins/agentd/src/execution.rs composition)
  → OrchestrationService::enter_execution (durable entry, synchronous)
  → detached background: WorkspaceService lease → ClaudeExecutor
    (configured default ExecutionPort) → attempt → submit → review →
    begin_verification → PendingEvaluator (no verdict → stays VERIFYING)
  → durable state observable via status, signallable via cancel
```

`run` commits claim/start/execution synchronously and returns the
identities immediately; the provider half never holds the IPC
connection. A client disconnect cannot redefine the outcome; `status`
re-observes it; `cancel` targets the durable execution identity.

## 2. Composition-root dependency graph

```text
agentd (composition root only)
 ├─ app-services (OrchestrationService, WorkspaceService, TaskService)
 ├─ provider-claude (ClaudeExecutor, LiveProcessTracker, CancelFlag)
 ├─ git-cli (GitCli::with_liveness(tracker))
 ├─ storage-sqlite (per-operation short-lived connections)
 └─ evaluation (PendingEvaluator lives HERE, not in app-services)

Forbidden directions verified (`cargo tree` + source search):
task ↛ provider-claude, workspace ↛ provider-claude,
storage ↛ provider-claude, ipc-contracts ↛ provider-claude,
app-services ↛ provider-claude, agentctl ↛ provider-claude.
No `provider == "claude"` branching in core (only the opaque
`claude-code` provider_ref identity, never branched on).
```

## 3. IPC operations (added vs reused)

Added (`execution_core.proto`, new bounded file per ADR-0021 §4):

```text
RunExecution      { task_id, execution_id?, attempt_id?, repository_ref,
                    repository_path, base_revision? }
RunExecutionResponse   { task_id, execution_id, attempt_id, task_state }
GetExecution      { execution_id }
GetExecutionResponse   { task_id, execution_id, task_state, attempts[],
                         provider_ref, workspace_path, execution_live }
AttemptView       { attempt_id, outcome, provider_ref, detail }
CancelExecution   { execution_id }
CancelExecutionResponse { execution_id, cancel_requested, run_state, detail }
```

Reused: `BusinessError` taxonomy + one additive code
`PROVIDER_UNAVAILABLE = 10` (execution surface only; old clients never
observe it on task_core paths). Protocol `V2 = 2`,
`SUPPORTED = [0, 1, 2]`; v2 sessions keep full task_core authority.

## 4. Provider selection semantics

No router, no scoring, no fallback (explicit non-goals, all deferred):

```text
composition root → configured default ExecutionPort = ClaudeProvider
```

`run` carries no provider field; the daemon runs what it composed. A
missing executable fails fast as PROVIDER_UNAVAILABLE *before* any
mutation (PATH scan, no spawn, no quota burn).

## 5. Workspace binding (fail-closed, proven)

Every background run resolves `<db-parent>/workspaces` (0700,
fail-closed like the data dir) and ensures the lease before building
the executor; the executor binds the lease path and refuses a missing
directory pre-spawn. Proven by `missing_workspace_fails_closed_…`:
entry commits (RUNNING + execution, honest intermediate), the lease
refuses, no attempt is created, `workspace_path` stays empty, and
nothing falls back to the daemon cwd / $HOME / another workspace.

## 6. Liveness wiring (§18, proven)

One `LiveProcessTracker` per daemon is shared between each
`ClaudeExecutor::with_tracker` and the `GitCli::with_liveness` backend,
through the provider-neutral `workspace::OwnerLiveness` seam. Proven by
`live_provider_run_protects_its_workspace_then_releases`: cleanup
during a live run is refused as `ActiveOwnerRejectsCleanup` with the
worktree intact; after the terminal outcome the normal cleanup rules
proceed. No Workspace → Claude dependency exists.

## 7. Cancellation path (§17, proven)

```text
agentctl cancel E → IPC → CancelRegistry::signal(E)
 → shared CancelFlag → wait-loop quantum → kill direct child + wait
 → unix process-group killpg → terminal Cancelled committed
 → Task stays RUNNING (never completed)
```

Proven by `cancel_reaches_only_the_target_execution`: only the target
tree dies, the unrelated execution still succeeds, the task is not
completed, repeat cancel is idempotent (`ALREADY_CONCLUDED`). Never
process-name killing; already-exited children are safe no-ops.

## 8. Failure propagation (§20, proven)

Adapter → runtime → service → IPC → agentctl preserves:
missing/incompatible/auth/malformed/timeout/cancelled as typed attempt
outcomes + bounded detail in `status`; synchronous `run` failures as
`NOT_FOUND / INVALID_TRANSITION / CONFLICT / PROVIDER_UNAVAILABLE`.
No `Internal error` flattening; no secrets/credential text on the wire
or in daemon logs (logs carry ids + outcome variants only).

## 9. Durable state ownership

Unchanged: Task (task-domain), Execution/Attempt (agent-runtime),
lease (workspace), journal (events). No Claude-specific tables or
fields; the CLI `session_id` travels only inside `evidence_ref`
(`claude:session:<id>`) and attempt detail. `run` backgrounds always
conclude durably; stranded opens (restart/shutdown) stay open and
reconcilable via `status` (`execution_live=false` + open attempt).

## 10. Sync vs async decision (§11)

Async `start → status → cancel` (not one blocking IPC): the daemon is a
single-threaded Tokio runtime with a 5 s client exchange bound, so a
synchronous multi-minute provider run would wedge control. Detached
background threads (not JoinSet-tracked) keep shutdown prompt; the
price — shutdown/restart orphans the provider child — is documented,
not hidden. No job persistence beyond Execution/Attempt rows (G11 owns
recovery).

## 11. agentctl surface (§12)

```text
agentctl execution run <task-id> <repo-path> [--rev R] [--ref N]
    [--execution E] [--attempt A] [--detach]
agentctl execution status <execution-id>
agentctl execution cancel <execution-id>
```

`run` mints client-side E/A ids (recoverable after an ambiguous reply
via `status`, never replayed), prints the accepted identities, then
polls `status` to a terminal attempt unless `--detach`. Exit 0 only on
attempt SUCCEEDED (Task VERIFYING — evidence submitted, never
completed). A local Ctrl-C never cancels the provider.

## 12. Real composition smoke (§24) — 2026-09-10, PASS

```text
agentctl → real agentd → real IPC → app service → WorkspaceLease
→ ClaudeProvider → Claude Code CLI 2.1.263 (subscription auth)
```

Task `T-SMOKE` ("Reply with exactly G5_COMPOSITION_SMOKE_OK…"), repo
`/tmp/g5smoke/repo`: 8.1 s wall, session
`0f78e158-709d-43d7-a94d-30df193db87a`, attempt SUCCEEDED, submission
carries the marker verbatim, Task VERIFYING (claim of completion did
not complete), full journal chain
(`project/task/claim/start/execution/workspace/attempt/submit/review/
verify-began`), worktree preserved, no leaked child, clean shutdown.
Normal deterministic CI never runs it (subscription-gated, manual).

## 13. Natural-exit grandchild pipe (§19) — RESOLVED, bounded

The adapter previously joined drain threads unboundedly after natural
exit: a double-forked daemon grandchild holding an inherited pipe
could stall finalization indefinitely. Now: shared capture buffers +
`capture_grace` (default 5 s, configurable) past the reap, then
finalize with bytes collected so far and detach. Proven by
`daemon_grandchild_holding_pipes_does_not_block_finalization`
(~1 s grace vs 5 s grandchild). Timeout/cancel paths still kill the
tree first, so their joins stay prompt without a grace.

## 14. Storage concurrency found by this task (both fixed)

Two production-feeding races, each with a keeper regression test:

1. **Lease-id collision** (`workspace_service.rs`): per-instance
   counters minted identical `ws-<millis>-1` for same-millisecond
   allocations from two service instances (the daemon opens one per
   background). Fixed with a process-wide counter + pid.
   Test: `concurrent_allocations_from_separate_instances_never_collide`.
2. **DEFERRED upgrade deadlock** (`storage-sqlite`): concurrent writers
   with DEFERRED transactions deadlocked on lock upgrade (both hold
   RESERVED, both want EXCLUSIVE); SQLite returns BUSY immediately to
   break the tie, bypassing the busy timeout. All write transactions
   (incl. migrations) now use `TransactionBehavior::Immediate`,
   serializing at BEGIN where the busy wait (30 s) applies, plus a
   re-check-inside migration race fix. Test: `concurrent_cycles.rs`
   (writer pairs + status readers).

## 15. Second-provider structural assessment (§27)

`provider-codex` next requires: a new adapter crate implementing
`scheduler::ExecutionPort` (+ own failure taxonomy/argv/process
mechanics, optionally `OwnerLiveness`), a factory closure in
`bins/agentd/src/execution.rs`, and tests. No changes to task-domain,
agent-runtime, scheduler, workspace, storage-sqlite,
OrchestrationService, completion semantics, or the journal. Verified:
`cargo tree -p provider-claude` shows only contract crates; no
`provider == "claude"` branching in core; binding tests prove
non-Claude executors bind identically. No blockers found.

## 16. Restart semantics (§26, documented limitation)

Daemon shutdown/restart orphans in-flight provider children (detached
backgrounds die with the process; children reparent and exit on
their own) and strands attempts as open rows: `status` shows
`execution_live=false` with the attempt OPEN — reconcilable, never
silently successful. No restart-resumable execution is claimed; G11
owns recovery.

## 17. Known limitations (not defects)

- No automated verification in G5: every provider success parks the
  Task in VERIFYING for explicit operator/G8 verification
  (`PendingEvaluator`).
- No live token/event streaming: `start/status/result` only; rich
  streaming is later work, decoupled from G5 closure.
- Pre-attempt background failures (lease refusal) leave detail in
  daemon logs only; durable state shows the honest intermediate.
- Context given to the provider is Task objective + acceptance only
  (bounded prompt); G9 owns retrieval/budgeting.
- Model selection is provider-native (no `--model` passed).

## 18. Verification

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --no-fail-fast (incl. 3× re-runs of composition suites)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo tree direction audit + `claude|Claude|provider ==` source search
```

Counts: provider-claude 53 lib + 12 fixture (+2 new), ipc-contracts 18
(+2), ipc-control 40 (+3), app-services full suite green (incl. new
`concurrent_*` + lease-collision tests), agentd 31 (+5 execution
unit), agentctl execution_composition 8 (new) + execution_binaries 2
(new). Real composition smoke: PASS (see §12).
