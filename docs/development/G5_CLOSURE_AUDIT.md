# G5 Closure Audit (TASK-0048)

> Independent closure audit for **G5 — First Real Provider**. Authority:
> `AGENT.md`, `INVARIANTS.md`, ADR-0005/0009/0019/0021/0022/0023,
> `docs/architecture/providers.md`. TASK-0046/0047 reports
> (`G5_CLAUDE_PROVIDER.md`, `G5_CLAUDE_COMPOSITION.md`) are evidence under
> test, not authority. Production source was traced first, verification
> re-run independently, and 13 throwaway probes were executed (removed
> after execution — no probe files remain in the tree).
>
> Verdict: **`G5_REMEDIATION_REQUIRED`** (rationale §15). G5 stays
> `in_progress`; TASK-0049 (remediation) and TASK-0050 (re-audit) are next.

## 1. Scope and declared exit criteria

Roadmap G5 objective: "One provider passes the AgentProvider contract; the
second must add cleanly without core changes." The AgentProvider contract
is still conceptual (`providers.md` §2 "No Rust trait signature is
frozen", §16 open items); TASK-0046 adopted `scheduler::ExecutionPort` as
the de-facto seam. This audit judges the implemented seam plus the TASK-0048
exit list (production control plane, provider-neutral IPC, lifecycle
boundaries, isolation, completion authority, bounded supervision, honest
durable state, deterministic verification, provable second-provider
addability).

Metadata conflict found: `planning/roadmap.yaml` `current_gate: G4` while
G4 is `completed`, G5 `in_progress`, and `queue.yaml` `current_gate: G5`
(stale since TASK-0045; corrected by this task).

## 2. Production path (traced from source)

```text
agentctl execution run (main.rs:575 mints E-<ms>-<pid>/A-<ms>-<pid>)
 → agentctl::execution_request → v2 handshake → ControlRequest.execution_core=5
 → ipc-control ServerSession::next_request (session.rs:563; v0/v1 → UnsupportedProtocol, connection ends)
 → agentd serve_connection (lib.rs:337, current_thread Tokio, per-connection task)
 → execution::dispatch_execution → start_run (execution.rs:406)
     validate ids/paths → provider_precheck (PATH presence only, :537)
     → ensure_managed_root (<db-parent>/workspaces, 0700)
     → OrchestrationService::enter_execution (claim → start → create_execution; orchestration_service.rs:320-327)
     → CancelRegistry::register → RunExecutionResponse (RUNNING)
 → detached std::thread run_background (lib.rs:351)
     → continue_workspace_cycle → WorkspaceService::ensure_workspace_ready (allocate → git worktree add → finalize_ready)
     → make_executor: build_task_prompt + ClaudeExecutor::bind(lease.worktree_ref) + tracker + cancel flag
     → run_workspace_attempt: create_attempt → ExecutionPort::execute (outside SQLite)
         → spawn_and_supervise (argv-only, cwd=lease, process_group(0), 5 ms poll, timeout 300 s)
         → parse_provider_output → WorkEvidence{summary≤1024, evidence_ref=claude:session:<id>}
     → finish: interpret → conclude_attempt → submit_result → route_to_review → begin_verification
     → PendingEvaluator (no verdict) → Task stays VERIFYING
 → status: handle_get reads durable rows + in-memory execution_live (cancel registry)
 → cancel: CancelRegistry::signal → CancelFlag → wait-loop kill + killpg
```

Confirmed end to end by the independent real-provider smoke (§12).

## 3. Provider boundary and second-provider analysis

Mechanical evidence: `cargo tree -i provider-claude -e normal` → only
`agentd`; `-e dev` → only `app-services` (tests). `task-domain`,
`agent-runtime`, `scheduler`, `workspace`, `storage-sqlite`,
`ipc-contracts`, `ipc-control`, `app-services`, `agentctl`, `evaluation`,
`git-cli`: zero `provider-claude` edges. `provider-claude` depends only on
`agent-runtime`, `scheduler`, `task-domain`, `workspace`.

`claude` occurrences outside the adapter, classified: `agentd/src/execution.rs`
(composition root — legitimate); opaque `claude-code` provider_ref in test
assertions and a proto comment (identity only, never branched); storage
journal tests asserting provider tokens are *absent*; doc comments. No
`provider == "claude"` branch, no Claude-specific table/column/IPC field/Task
state. Provider session id appears only inside opaque evidence text
(`execution_attempts.detail`, `AttemptView.detail`), never as a schema field.

Second provider (`provider-codex`) would change:

| File | Category |
|---|---|
| `Cargo.toml` (member) | EXPECTED |
| `adapters/provider-codex/**` (new `ExecutionPort` impl, failure taxonomy, argv/process) | EXPECTED |
| `bins/agentd/Cargo.toml`, `bins/agentd/src/execution.rs` (factory, precheck executable name) | EXPECTED (composition root) |
| provider tests | EXPECTED |
| `CancelFlag` / `LiveProcessTracker` usage in `execution.rs:86-175` | SMELL: provider-neutral process primitives live in `provider-claude`; cancellation is out-of-band, not on the seam (H6) |
| Task / Workspace / Storage / IPC / orchestration / completion | none required |

`structurallyAddable = true`; core semantic changes required: none. What is
missing is the *proof mechanism*: no shared `ExecutionPort` contract suite
exists (ADR-0009 "every adapter passes the shared port contract suite";
AGENT.md §6), so addability is asserted by evidence docs, not by a contract a
new adapter must pass (B3).

## 4. IPC v2 audit

- Numbering: `ControlRequest/Response` 1–3 frozen, `task_core=4`,
  `execution_core=5`; `ExecutionCoreRequest` 1–3, response 1–3 + `error=4`;
  `PROVIDER_UNAVAILABLE=10` appended. No collisions.
- Gate: only negotiated v2 surfaces `Execution`; v0/v1 fail with
  `UnsupportedProtocol` and the connection ends (same discipline as G2
  task_core). Old `[0]`/`[0,1]` clients keep their exact surface
  (negotiation tests green).
- Unknown oneof in envelope → `UnexpectedMessage` (session terminated);
  empty execution payload → typed `INTERNAL`.
- Operations are provider-neutral (task/execution/attempt ids, repository
  inputs, opaque provider_ref, bounded detail). No provider name, model,
  flag, credential, or session field in the schema.
- Correlation: sequential one-in-flight per connection (ADR-0021 §12).
- Fail-safe gap: `execution.rs:843-850 truncate_id` byte-slices at 64; a
  validated multibyte execution id panics the `get`/`cancel` NOT_FOUND
  paths (probe P1, panic at `execution.rs:846`). The per-connection task
  dies; the daemon survives. Same class in `dispatch.rs:354-370`
  (`String::truncate`, `truncate_id`) (H1).
- Documentation: `ipc.md` has no v2/execution_core section;
  `VERSIONING.md` still states "supported IPC 0, 1" (B3).

## 5. Task / Execution / Attempt / Session

Distinct durable concepts hold (`executions`, `execution_attempts`, Task
rows; session only inside evidence). One Execution → one lease → one attempt
per `run`. Defects:

- **Entry is not identity-safe (B1, probe P2).** `enter` commits
  `task.claimed` + `task.started` before `create_execution` checks
  uniqueness (`runtime_store.rs:90`). `run T-2 --execution <E owned by T-1>`
  returns `ALREADY_EXISTS` while T-2 is left `RUNNING` with no Execution;
  re-run with fresh ids → `INVALID_TRANSITION` (not runnable from RUNNING).
  The error reply contradicts a committed mutation and the Task is
  permanently non-executable through the control plane.
- **Duplicate attempt id is accepted then stranded (B1, probe P3).**
  `run` → `RUNNING` accepted; background creates Execution + workspace
  (real worktree), then `create_attempt` hits the PK; status shows
  `live=false attempts=[]`, reason only in daemon stderr.
- Managed-branch slug is not injective (`workspace_service.rs:91-105`:
  `-` passes through while other bytes encode as `-XX`; ≤64-byte ids can
  equal the `h<hash>` form). `E/S` and `E-2FS` both derive
  `refs/heads/agent/E-2FS` (probe P4): second lease stays `Allocated`,
  first lease/worktree intact — fail-closed, Task stranded (H4).

## 6. Workspace isolation

- cwd is always the lease worktree: fixture marker proof
  (`provider_binding.rs`), real smoke (Claude reported the managed
  worktree as cwd). Missing workdir → `WorkspaceBinding` pre-spawn; no
  fallback path exists in `spawn_and_supervise`.
- Managed root `<db-parent>/workspaces` (0700); lease path derived from
  lease id, re-validated before destruction.
- G4-F1 under real wiring: no production caller of `cleanup_workspace`,
  `request_cleanup`, or `compute_diff` exists in `bins/` (only an agentd
  unit test). `run.repository_path` feeds allocation only;
  `repository_ref` is an unverified client label (audit label only —
  cleanup still requires registration/history proof). F1 fix intact.
- BranchRef constructors: `branch_for_execution` and store restore only;
  no IPC/provider/CLI route constructs one; deletion gated by
  `is_managed`.
- Real Claude run left `?? data/sessions.db` in the worktree (provider-side
  tooling write) → ordinary cleanup will refuse `WorktreeDirty`; leases
  accumulate (H7). The untracked `data/sessions.db` at this repository's
  root is the same tooling artifact, not product output.

## 7. Process supervision and liveness

| Path | Evidence | Result |
|---|---|---|
| natural exit, no grandchild | P7 baseline | 0.30 s |
| natural exit, grandchild holds pipes, grace 1 s / 2 s | P7 | 2.29 s / 4.01 s — effective bound ≈ 2×grace (stdout then stderr, `process.rs:167-168`) + spawn latency; production 5 s grace ⇒ up to ~10 s |
| grandchild after finalize | P7 | still alive; tracker already released (H3) |
| timeout / cancel | fixtures | tree killed (direct kill + reap + `killpg`), joins prompt |
| daemon SIGKILL / graceful shutdown | P11 | provider child survives unsupervised (no timeout, not cancellable after restart) |

`LiveProcessTracker` refcounts per execution; `mark_live`/`mark_finished`
bracket `run()` (`executor.rs:126-135`, not RAII — a panic inside `run()`
would leave the owner live; the parser/process code paths are
non-panicking, so unreachable today). Liveness is registered only when
`execute` starts, after lease Ready (window unreachable without a
production cleanup caller). Reverse case: provider dead ⇒ tracker cleared
on every terminal path (P7, P8, P13 `live_after=false`).

## 8. Cancellation race matrix

| Scenario | Evidence | Outcome |
|---|---|---|
| before background starts | P5 | ack `LIVE_SIGNALLED`; lease + attempt created; child spawned and killed on first quantum; `CANCELLED` in 0.48 s; Task RUNNING |
| during/just after spawn, while running | P8 i=0–8, fixtures | `CANCELLED`, exactly one `attempt.concluded` |
| concurrent with natural exit | P8 i=9 | ack `LIVE_SIGNALLED`, exit won → `SUCCEEDED`/VERIFYING (ack ≠ guarantee) |
| after exit / after terminal persistence | P8 i=10–15 | `ALREADY_CONCLUDED` |
| two clients at once | P13 | both `LIVE_SIGNALLED`; one `CANCELLED`; 1 event |
| repeat after terminal | existing tests | `ALREADY_CONCLUDED` |
| unknown execution | code | `NOT_FOUND` (multibyte >64 B id panics handler, H1) |
| after daemon restart | P11 | `NO_LIVE_RUN`; orphan untouched |
| wrong attempt | n/a | cancel addresses the execution; one attempt per execution |

16/16 race iterations consistent (0 anomalies); only the target tree is
affected (`cancel_reaches_only_the_target_execution`).

## 9. SQLite concurrency and migrations

- Every explicit write transaction is `TransactionBehavior::Immediate`
  (`task_store.rs` ×6, `runtime_store.rs` ×3, `workspace_store.rs` ×2,
  `migration.rs` ×1). Remaining writes are single-statement autocommit
  (e.g. `mark_lease_cleanup_pending`), which take write locks from an idle
  connection where the busy handler applies. No bare `transaction()`,
  `unchecked_transaction`, or raw `BEGIN`. Rollback-journal mode, busy
  timeout 30 s.
- Probe P10: fresh DB 10 rounds × 8 concurrent openers → 0 errors;
  partially migrated v1/v2/v3/v4 × 5 rounds × (5 openers + 3 readers) →
  0 errors, schema 5, identical table set; current DB 6 openers + 4
  readers → 0 errors. `concurrent_cycles.rs` green.
- Lease id `ws-<ms>-<pid>-<process counter>` + PK + target-path collision
  guard: collision needs equal ms, pid, and counter (clock rollback plus
  PID reuse after restart) and fails closed. Sufficient. Server-minted
  execution ids `E-<ms>-<counter>` omit pid; a clock-rollback collision
  would land in B1.

## 10. Provider output, parser, auth, prompt, CLI policy

- Capture: 1 MiB per stream (`take(bound+1)`); over-bound output is
  truncated then fails JSON parsing (no explicit refusal despite the
  comment at `process.rs:144-146`).
- Parser totality: malformed/truncated/bad-escape/deep(10k) inputs return
  typed errors, no panic; duplicate keys → first wins; unknown fields
  ignored.
- **Parser cost is quadratic (H2).** `json.rs:286-297` runs
  `std::str::from_utf8` over the whole remaining input for every string
  character. Probe P6: 256 KiB ASCII 1.37 s; 1 MiB ASCII 21.8 s;
  512 KiB two-byte 45.3 s (≈2 min extrapolated for 1 MiB three-byte). The
  stall happens after the child exits — outside the 300 s timeout and the
  cancel poll; status shows `live=true`. Bounded by the capture limit.
- Auth: no credential reads/persistence/forwarding (source sweep); env
  inherited unchanged; auth failure → `AuthRequired` via marker table with
  a ≤500-char stderr excerpt that can reach attempt detail/IPC (bounded;
  redaction is G6). Real smoke: 0 secret markers in DB and daemon log;
  daemon log carries ids + outcome variant only.
- Permission bypass: `with_permission_mode` refuses any `bypass`
  spelling; composition never sets a mode or `--resume`; argv builder
  emits only `-p --output-format json [mode] [resume] <prompt>`; no
  `dangerously*` anywhere.
- Prompt: objective + acceptance, trimmed, ≤8192 chars with explicit
  marker, argv-only (hostile-prompt fixture proves no substitution).
  Empty objective is impossible (domain validation).
- CLI policy (`detect.rs:87`): `2.x` with minor ≥ 1 → compatible
  (2.99 passes, 3.0 fails, patch ignored, malformed → `UNKNOWN`). Help
  probing is substring-based and fails closed. **Neither detection,
  version gate, nor capability probe runs on the production path** —
  `provider_precheck` checks PATH presence only; an incompatible CLI is
  spawned and fails closed at parse (`G5_CLAUDE_PROVIDER.md` §10 "never
  spawned blindly" is inaccurate). Usage is parsed but dropped at the
  neutral seam (H5).

## 11. Completion authority, journal, lifetime semantics

- No provider signal completes a Task: fixture claiming completion →
  VERIFYING; real smoke → VERIFYING; `PendingEvaluator` returns no verdict;
  v2 has no completion op; COMPLETED only via evaluator verdict
  (`provider_binding.rs`).
- Journal (real smoke): `project.created … execution.created,
  workspace.created, attempt.created, attempt.concluded(succeeded),
  task.submitted, task.routed_to_review, task.verification_began` — typed,
  scoped, one conclusion per attempt (P8/P13). Raw provider JSON never
  journaled; `task.submitted.reason` carries the bounded provider result
  text (untrusted model output in an event payload; redaction is G6).
- Client disconnect: killing wait-mode `agentctl` mid-run does not cancel;
  run concludes `SUCCEEDED`, Task VERIFYING (P11). Background ownership is
  daemon-side.
- Restart: after SIGKILL and after graceful shutdown, status reports
  `RUNNING live=false attempt OPEN`, cancel `NO_LIVE_RUN`, re-run
  `INVALID_TRANSITION`; never success, never corrupt (P11).
- No re-execution path exists through the control plane: any non-success
  leaves the Task RUNNING (or RETRY_READY after verify-fail) and `run`
  requires READY — inherited G3 `DEFERRED_BY_DESIGN`, undocumented in G5.

## 12. Real-provider reproduction (2026-09-11)

Isolated `AI_ENGINEERING_OS_RUNTIME_DIR`/`DATA_DIR`, built `agentd` +
`agentctl`, real `claude` 2.1.268 (subscription auth, provider-owned).
Tool-free task asking for `G5_AUDIT_SMOKE_OK <cwd>`:

```text
run exit 0, 14 s wall; attempt A-AUD SUCCEEDED claude-code
evidence claude:session:af787d25-bbc7-4c3c-94e9-ba3218192b0e
submitted text: "G5_AUDIT_SMOKE_OK <managed worktree path>"   (cwd proven)
task T-AUD VERIFYING; cancel → ALREADY_CONCLUDED
primary HEAD unchanged, primary clean; no process under the worktree
worktree porcelain: "?? data/sessions.db"   (H7)
data dir mode 700; secret markers DB=0 log=0; clean shutdown
```

No failure real-smoke was run (optional; deterministic fixtures cover the
failure taxonomy).

## 13. Test integrity

- Production seams are genuinely exercised: in-process `agentd` over real
  UDS IPC with the real `agentctl` client; shipped binaries with a PATH
  `claude` fixture; `ClaudeExecutor` through `schedule_in_workspace`.
- CI independence: the real smoke is `#[ignore]` + env-gated; no global
  env/PATH mutation (env passed per child); polling with deadlines.
- **Non-deterministic gate test (B2):**
  `process_fixtures::daemon_grandchild_holding_pipes_does_not_block_finalization`
  failed in the first independent full run (4.40 s vs `< 4 s`,
  `process_fixtures.rs:339`); full runs 2 and 3 passed (1 of 3 red);
  passes 6/6 in isolation. P7 shows the
  measured quantity is ≈2×grace + spawn latency (≈2.3 s with grace 1 s),
  leaving 1.7 s of headroom that full-suite load consumes. The TASK-0047
  "624 passed / 0 failed" census was not reproduced on that run.
- Missing coverage: no shared `ExecutionPort` contract suite (B3); no test
  for identity conflicts before mutation (B1); no multibyte-id or
  large-output parser tests.

## 14. Known limitations classification

| Limitation | Classification | Reason |
|---|---|---|
| Daemon restart/shutdown orphans provider child | ACCEPTABLE_DEFERRED (G11) | State stays honest (P11); orphan is unsupervised — record the risk |
| Pre-attempt background failure detail only in logs | HARDENING_FOLLOWUP | Honest intermediate for environment failures (lease refusal); synchronous-detectable identity conflicts are B1 |
| Verification manual / deferred to G8 | ACCEPTABLE_DEFERRED | Completion authority correct (VERIFYING) |
| No live token/event streaming | ACCEPTABLE_DEFERRED | Not required for G5 correctness |
| Provider context = objective + acceptance until G9 | ACCEPTABLE_DEFERRED | Bounded, argv-safe |
| Scratch dirs may remain under OS temp | HARDENING_FOLLOWUP | Confirmed test-hygiene leak: `agentd-dispatch-test-*` never removed (404 accumulated), 9 `runlith-workspace-service-test-*` per run; no product impact |
| No re-execution/retry through control plane (added) | ACCEPTABLE_DEFERRED | G3 deferred-by-design; must be documented |

## 15. Findings and verdict

**Blockers (G5 remediation required):**

- **B1 — `run` is not identity-safe before mutation.** Scenario: `run`
  with an execution id owned by another Task, or an attempt id already
  used. Expected: typed refusal with no mutation (the `start_run`
  fail-fast contract). Actual: claim+start committed then
  `ALREADY_EXISTS` (Task wedged RUNNING, no Execution); duplicate attempt
  id accepted, workspace created, stranded with no attempt. Invariants:
  INV-001/INV-010 (honest durable, auditable state), TASK-0048 §28,
  G5_CLAUDE_COMPOSITION §8. Evidence: P2, P3; `orchestration_service.rs:320-327`,
  `runtime_store.rs:90`, `execution.rs:504`. Scope: refuse
  execution/attempt identity conflicts before claim (pre-entry check plus
  store backstop), regression tests through `dispatch_execution`.
- **B2 — required verification is not deterministic.** Scenario: full
  `cargo test --workspace`. Expected: green. Actual: grandchild-grace test
  fails intermittently under load (1 of 3 full runs; 4.40 s > 4 s). Contract: G5 exit "full deterministic
  verification is green"; AGENT.md §5. Evidence: §13, P7. Scope: make the
  assertion load-robust and grace-relative (per-stream grace is
  sequential) without weakening what it proves; align docs.
- **B3 — the provider contract is not materialized as authority.** No
  shared `ExecutionPort` contract suite (ADR-0009 consequence, AGENT.md
  §6); no ADR for G5 active decisions (provider seam = `ExecutionPort` with
  out-of-band cancel/liveness, IPC v2 `execution_core` async detached
  semantics + `PROVIDER_UNAVAILABLE`, CLI version policy) as ADR-0019
  requires; `providers.md` unchanged since TASK-0027, `ipc.md` lacks v2,
  `COVERAGE.md` lacks a G5 row, `VERSIONING.md` stale. Second-provider
  addability is therefore asserted, not provable. Scope: one ADR +
  providers/ipc/coverage/versioning doc updates + a reusable contract
  suite run against the FakeAgent executor and `ClaudeExecutor` fixtures.
  No new provider.

**Hardening follow-ups (not blocking):**

- H1 byte-slice truncation panics on multibyte ids (`execution.rs:846`,
  `dispatch.rs:354-370`) — cheap; recommended inside TASK-0049.
- H2 quadratic JSON string scan (P6).
- H3 capture grace sequential per stream; natural-exit grandchild survives
  with liveness released.
- H4 non-injective managed-branch slug (P4).
- H5 detection/version gate/capabilities not on production path; usage
  dropped at seam; doc inaccuracy.
- H6 neutral cancel/liveness primitives live in `provider-claude`;
  cancellation off-seam.
- H7 real provider tooling dirties worktrees → cleanup refusal/lease
  accumulation; provider inherits user CLI plugins/hooks (G6 isolation).
- H8 cancel-before-spawn spawns then kills (check flag pre-spawn).
- H9 INV-081 review signals: `workspace_service.rs` 1398, `task_store.rs`
  1409, `agentd/src/lib.rs` 1366, `ipc-control/src/session.rs` 1371,
  `agentd/src/execution.rs` 1186 LOC.

Not falsified (hold independently): real provider through the production
control plane; provider-neutral IPC schema; no core→provider dependency;
auth provider-owned; cwd = managed worktree; provider success never
completes; cancellation attempt-specific and single-terminal; bounded
supervision on timeout/cancel; honest state across client loss and daemon
restart; SQLite write/migration concurrency fixes; parser non-panicking;
FakeAgent suites green; core requires no change for a second provider.

**Verdict: `G5_REMEDIATION_REQUIRED`.** B1 is an incorrect durable state
reachable from validated client input; B2 leaves the gate's own
verification non-deterministic; B3 leaves the gate's second clause
unprovable from the repository. G6 must not start until TASK-0049 lands
and TASK-0050 re-audits. ADR-0019 forward check for G6: Policy Engine
design ADR, Secrets Broker ADR, and Security Audit ADR are required
before G6 implementation; none exists yet.

## 16. Commands executed and census

```text
cargo fmt --all -- --check                     clean (3 runs)
cargo check --workspace                        clean (2 runs)
cargo clippy --workspace --all-targets --all-features -- -D warnings   clean (3 runs)
cargo test --workspace --no-fail-fast
  run 1: 623 passed, 1 FAILED (grandchild-grace), 1 ignored — 67 binaries
  run 2: 624 passed, 0 failed, 1 ignored
  run 3: 624 passed, 0 failed, 1 ignored
cargo test -p provider-claude --test process_fixtures    6× isolated → 12/12 each
repeats: concurrent_cycles 3×, execution_composition 3×, agentd execution
  unit 3×, liveness_binding 3×, execution_binaries 2×, storage-sqlite 2×,
  provider-claude 2× → all green
cargo tree -i provider-claude --workspace -e normal / -e dev;
cargo tree -p <11 core crates> -e normal (provider-claude edges: 0)
rg sweeps: provider identity, credentials/tokens, bypass/permission flags,
  transaction patterns, cleanup callers, BranchRef constructors,
  contract suites, ADR/doc coverage
probes (removed after execution): P1 multibyte id truncation; P2
  execution-id collision; P3 attempt-id collision; P4 slug collision; P5
  cancel before spawn; P6 parser bounds/timing; P7 capture grace +
  grandchild survival; P8 cancel vs natural exit ×16; P10 migration
  concurrency (fresh, partial v1–v4, current, with readers); P11 daemon
  SIGKILL / graceful shutdown / client kill via real binaries; P13
  concurrent duplicate cancels
real provider smoke (§12): built binaries + claude 2.1.268
```

Census (runs 2/3): 624 passed + 1 ignored (`provider-claude/tests/smoke.rs`,
env-gated real provider). G5-owned: provider-claude 53 unit + 12 process
fixtures + 1 ignored smoke; agentd 31 unit (execution composition
included); agentctl `execution_composition` 8 + `execution_binaries` 2;
app-services `provider_binding` 6 + `concurrent_cycles` 2; ipc-contracts
18; ipc-control 40; git-cli `liveness_binding` 2. FakeAgent suites
(`fake_agent` 10, `scenario_matrix` 14, `fake_agent_seam` 3,
`failure_matrix` 27) green — FakeAgent remains valid on the same seam.

Hygiene sweep after all runs: no stray fixture/provider processes, no
leftover sockets, no probe directories. Pre-existing debris: OS-temp test
dirs (§14) and `/tmp/g5smoke` (TASK-0047 smoke, 2026-09-10). An earlier
interrupted attempt of this audit had left an untracked probe
(`bins/agentd/tests/zz_audit_truncate.rs`) and its temp dir; the probe was
moved out of the tree before verification and the dir removed. The
untracked repository-root `data/sessions.db` is a provider-tooling
artifact (§6) and was left untouched.

Planning/version: `product/version.toml` → `0.1.0-dev.task0048` (gate
G5); `queue.yaml` completed through TASK-0048 with TASK-0049
(remediation) and TASK-0050 (re-audit) queued; `roadmap.yaml`
`current_gate` corrected G4 → G5; G5 remains `in_progress`.
