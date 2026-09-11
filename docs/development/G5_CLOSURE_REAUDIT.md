# G5 Closure Re-Audit (TASK-0050)

> Independent adversarial re-audit of G5 after the TASK-0049 remediation.
> Authority: `AGENT.md`, `INVARIANTS.md`, ADR-0005/0009/0018/0019/0021/0022/0023/0024,
> `docs/architecture/providers.md`, `docs/architecture/ipc.md`.
> `G5_CLOSURE_AUDIT.md` (TASK-0048) is the historical record of failure;
> `G5_CLOSURE_REMEDIATION.md` (TASK-0049) is evidence under test, not authority.
> Production source was traced first; three throwaway probe files were executed
> and removed afterwards (`crates/app-services/tests/zz_audit_race.rs`,
> `adapters/fake-agent/tests/zz_neg_control.rs`,
> `adapters/provider-claude/tests/zz_parser_scale.rs`) — no probe files remain.
>
> Verdict: **`G5_REMEDIATION_REQUIRED`** (rationale §16). G5 stays
> `in_progress`; TASK-0051 (remediation) is next. G6 must not start.

## 1. Production paths inspected

```text
crates/app-services/src/orchestration_service.rs  (enter/enter_execution/continue_workspace_cycle/finish)
crates/app-services/src/runtime_service.rs        (create_execution/create_attempt/conclude_attempt)
crates/app-services/src/workspace_service.rs      (slug_body/branch_for_execution/ensure/allocate/materialize/cleanup)
adapters/storage-sqlite/src/runtime_store.rs      (create_execution_with_event/create_attempt_with_event + Immediate txns)
bins/agentd/src/execution.rs                      (start_run/handle_get/handle_cancel/run_background/provider_precheck)
bins/agentd/src/dispatch.rs                       (map_error/floor_char_boundary/truncate)
adapters/provider-claude/src/process.rs           (spawn_and_supervise/join_drains_graceful/kill_tree_and_reap)
adapters/provider-claude/src/json.rs              (string/decode_first_char)
crates/execution-contract/src/lib.rs              (shared suite)
adapters/fake-agent/tests/execution_port_contract.rs
adapters/provider-claude/tests/execution_port_contract.rs
bins/agentctl/tests/real_process_matrix.rs        (hermeticity)
crates/agent-runtime/src/id.rs                    (identifier validation)
```

## 2. TASK-0048 blocker revalidation (sequential)

- **B1 sequential: RESOLVED.** `enter()` preflights execution- and attempt-ID
  existence before claim/start; keepers
  `run_execution_id_collision_refuses_without_mutation` and
  `run_attempt_id_collision_refuses_without_mutation` green; rerunnability
  with fresh IDs confirmed by the keeper assertions. No other deterministic
  synchronous validation was found after lifecycle mutation on the start path
  (id shapes, repository inputs, provider precheck, managed-root ensure all
  precede `enter_execution`; post-entry failures are genuinely async).
  Refused starts commit no journal lifecycle facts for the rejected task
  (keeper asserts journal cleanliness).
- **B2: RESOLVED.** `join_drains_graceful` implements one shared deadline for
  both drain threads (`process.rs:309-330`); per-stream sequential grace is
  gone. Assertion is grace-relative (`grace×2 + 3 s` margin, `<<` 10 s
  grandchild lifetime). Evidence: grandchild test 10/10 isolated (~1.23 s
  each), `process_fixtures` 5/5 under normal load, full workspace suite 3/3
  green. Finalization bounded, never waits grandchild lifetime.
- **B3: RESOLVED.** `crates/execution-contract` defines only seam-promised
  behavior (success→submittable bounded evidence, failure→canonical Failed,
  malformed→never success, timeout→TimedOut, opaque stable provider_ref).
  Exclusions (cancel/cwd/partial/Task-mutation/streaming/usage) are documented
  with where-each-is-proven-instead. FakeAgent and Claude fixtures invoke the
  SAME `run_execution_port_contract` from provider-owned test targets
  (no copied suite). Dep direction: `execution-contract → {agent-runtime,
  task-domain, scheduler}` only; providers consume via dev-deps;
  `cargo tree -i provider-claude -e normal` reaches only `agentd`;
  `-e dev` only `app-services` (tests). No `provider == "claude"` branch in
  core crates (rg clean). Negative control independently re-performed: a
  deliberately broken executor (blank summary; malformed-reported-as-success)
  fails the suite (2/2 temp probes, removed afterwards).

## 3. Concurrent B1 TOCTOU — NEW G5 BLOCKER

TASK-0049's preflight reads existence, then commits claim → start →
create_execution as separate transactions on separate connections. Two
concurrent `enter_execution` calls with the same new execution ID (barrier-
synchronized, separate service handles = separate SQLite connections, exactly
like two concurrent `run` IPCs) reproduce the original B1:

- Probe `concurrent_shared_execution_id_leaves_loser_honest`: **6/6 FAIL.**
  Loser receives `Duplicate` or `UNIQUE constraint failed: executions.id`
  (i.e. sometimes even the "backstop" fires, sometimes the winner commits
  between loser-preflight and loser-claim) — but in ALL cases the loser Task
  is left `RUNNING` with committed `Claimed` + `Started` journal events and
  no Execution, and is non-rerunnable (`INVALID_TRANSITION`/`NotRunnable`).
  The documented "store-level uniqueness backstop" (ADR-0024 §6, remediation
  §1) refuses the request but does NOT preserve honest durable state — it is
  the old bug, not a backstop.
- Probe `concurrent_shared_attempt_id_leaves_loser_honest`: both entries
  succeed (attempt does not exist yet for either), committing claim + start +
  execution on BOTH tasks with the same pending attempt ID. The collision
  then detonates later at background `create_attempt` (PK), stranding the
  loser RUNNING + Execution + lease/worktree with no attempt — the old P3
  shape, moved later in the pipeline.

Expected per the task brief: loser preserves honest durable state
(pre-run Task, zero unrelated facts). Actual: poisoned Task + misleading
journal. This is not a rare race — barrier rendezvous hits it every run, and
any same-millisecond duplicate-`run` pair (operator retry, double-click,
two controllers) can hit it without a barrier.

**Classification: G5_BLOCKER (B1-concurrent).** Remediation scope: make the
entry boundary atomic — preflight + claim + start + create_execution in one
`Immediate` transaction (or a serialized entry mutex + single transaction),
so a conflict refuses before the first mutation under concurrency too; add
concurrent keeper(s) through `enter_execution`/`dispatch_execution`.

## 4. H4 slug collision — PROMOTED to G5 BLOCKER

Re-audited after B1 remediation. `slug_body` is unchanged and still
non-injective (`-` passthrough vs `-XX` encoding; >64 B digest form
collidable with short form in principle). `ExecutionId` validation is
trim/non-blank/≤256 B with NO charset restriction — so the collision is
reachable through production IPC with ordinary valid IDs. Probe
`slug_collision_wedges_second_task` (service level, valid IDs `E/S`,
`E-2FS`):

```text
ensure(E/S)   → CreatedFresh, branch refs/heads/agent/E-2FS
ensure(E-2FS) → Err(BranchCollision) — fail-closed, no reuse, no reset
T-B state RUNNING; re-entry with fresh IDs → NotRunnable (RUNNING)
```

T-B is permanently non-rerunnable through the control plane (no re-execution
path; G3 deferred-by-design) with a committed execution whose workspace can
never materialize — incorrect durable state from a synchronously detectable
conflict (both slugs derivable at entry), i.e. B1-shaped. Per the brief's
§21 promotion criterion ("orphan WorkspaceLease or permanent non-rerunnable
Task → promote"), **H4 is a G5_BLOCKER**. Remediation scope (same TASK-0051):
entry-time slug/branch-conflict preflight before mutation, or injective slug
redesign; plus orphan-`Allocated`-lease disposition. (P4's "second lease
stays Allocated" disposition was not re-probed at row level; the wedge was.)

## 5. Shared-contract strength (B3 follow-through)

Coverage assessed against §10: success→evidence, failure→Failed,
malformed→never-success, timeout→TimedOut, ref opacity — all meaningfully
asserted with bounds (4096/1024/8192 guards catch transcript dumps, not exact
limits). Cancellation correctly excluded (no `cancel` on the seam; proven by
`preset_cancel_flag_spawns_no_child` marker test + cancel-race suites).
Second-provider thought experiment (`provider-codex`): needs workspace
registration, new adapter crate, composition-root factory/precheck name,
provider tests + contract invocation — all EXPECTED; no Task/Workspace/
Storage/IPC/completion changes required. `structurallyAddable = true`, now
with a passing contract mechanism rather than prose.

## 6. ADR-0024 / document consistency

ADR-0024 accurately describes current production behavior in all §14 points
(seam, ACLs, composition ownership, IPC v2 run/get/cancel, identity split,
completion authority, provider-owned auth, Claude-first, addability) with one
required precision: §6's "store-level uniqueness guards remain the backstop
for concurrent races" is now proven false as a state-honesty backstop (§3) —
the ADR must be amended by the remediation task. Normative docs
(`providers.md` §2/§2a/§13a/§15/§16, `ipc.md` §16, `VERSIONING.md` IPC 0,1,2)
are accurate, including the H5 deferral (adapter-local version policy;
production precheck is PATH-presence only; 5 s IPC bound vs 10 s probe
rationale recorded at the `provider_precheck` callsite). **Stale:** evidence
doc `G5_CLAUDE_PROVIDER.md:139` still claims "never spawned blindly" — the
exact inaccuracy TASK-0048 flagged; TASK-0049 did not correct it. Normative
layer is clean, so this is a HARDENING_FOLLOWUP doc fix, not a blocker.

## 7. Hardening revalidation

- **H1 RESOLVED.** Single shared `floor_char_boundary` helper; all three
  former byte-cut sites route through it; sweep shows remaining `[..]` cuts
  are byte frames/SIDs/`Vec`s. Targeted suites green (agentd execution 9,
  dispatch 10).
- **H2 RESOLVED.** `decode_first_char` validates only the char's own bytes
  (O(1)); independent scaling probe via public `parse_provider_output` on
  two-byte text: 128 KiB → 4.9 ms, 256 KiB → 9.7 ms, 512 KiB → 19.4 ms,
  1 MiB → 38.7 ms — exact linear doubling (old: ~45 s / ~22 s). Bounds,
  malformed-totality, and Unicode battery green.
- **H3 RESOLVED.** Joint deadline verified in code; no data race (drain
  buffers behind `Mutex`, finished threads joined, still-blocked threads
  detached with bytes snapshotted — documented truncation, not loss);
  timeout/cancel paths kill the tree before joining so no grace applies.
- **H6 RESOLVED as documented decision.** `CancelFlag`/`LiveProcessTracker`
  remain in `provider-claude` per ADR-0024 §9 (no speculative extraction);
  liveness crosses to Workspace only via neutral `OwnerLiveness`; no core
  crate depends on a provider (normal or dev, modulo test-only edges). A
  second provider can carry its own primitives without core changes.
- **H8 RESOLVED.** Pre-spawn cancel check in `spawn_and_supervise`
  (`process.rs:113-118`); fixture marker proves no child starts;
  `preset_cancel_flag_spawns_no_child` green.
- **H5 ACCEPTABLE_DEFERRED** (+ doc fix above). Incompatible CLI fails closed
  at parse (`MalformedOutput`, honest state, no authority) — safe to defer
  pending a bounded-fast probe design.
- **H7 ACCEPTABLE_DEFERRED.** Reproduced again in this audit's real smoke:
  `?? data/sessions.db` in the managed worktree; cleanup refuses dirty
  worktrees at three gates (`WorktreeDirty`, fail-closed, work preserved).
  Lease accumulation is G6-isolation scope; no force-clean hacks exist.
- **H9 ACCEPTABLE_DEFERRED.** INV-081 ("monitored by architecture fitness
  checks … or an explicit reviewed exception") has no automated enforcement
  in the repo — the review-signal reading is honest, not convenient. No
  normative maximum is violated.

## 8. Environment / test-hermeticity audit

- Preflight hygiene recorded before full runs: no test-path `agentd`
  processes; no canonical socket dir; no `/tmp/runlith-*` debris; only Apple
  system daemons match bare `pgrep agentd` (`talagentd`,
  `gamecontrolleragentd` — unrelated). No pre-cleaning was performed.
- `g1_real_process_matrix` couples to **same-binary-path** processes only
  (`pgrep -f <target/.../agentd>`), NOT to all machine-wide agentd processes:
  a legitimate separately installed daemon at another path does not match.
  The TASK-0049 incident (two stale processes) was stale same-path daemons
  from prior test runs, which the Phase-0 endpoint-ping shutdown does not
  cover (they own no endpoint). So the "environmental" label was essentially
  correct but imprecise. Classification: **HARDENING_FOLLOWUP** (test defect,
  not G5 semantics) — scope the count to the endpoint-owned PID or document
  the exclusive-run requirement. Not a gate blocker. This audit's own smoke
  daemon was shut down via the product flow and verified absent before full
  runs.
- SQLite regression: `concurrent_cycles` (2 tests) green in all 3 full runs;
  no `SQLITE_BUSY`/migration issues; entry-atomicity remediation must not
  regress this (Immediate-transaction discipline must extend to the unified
  entry transaction).

## 9. Completion authority / lifecycle

Re-proven: real smoke (Claude 2.1.268, subscription auth, tool-free prompt)
`agentctl → IPC → agentd → WorkspaceLease → ClaudeProvider` → attempt
`A-SMOKE SUCCEEDED` (`claude:session:5d4c99ff-…`), Task `VERIFYING`, cancel →
`ALREADY_CONCLUDED`; submitted text contains `G5_REAUDIT_SMOKE_OK` + the
managed worktree path (cwd proven); no leaked supervised processes; no
credentials in status/task output. `provider_binding` (6) green: fixture
claiming completion → VERIFYING. Client-disconnect/daemon-death semantics
unchanged since TASK-0048 (not re-probed; no code touched on those paths).

## 10. Journal honesty

Sequential B1 refusals commit zero lifecycle facts (keepers). Concurrent
losers commit `Claimed` + `Started` (+ `ExecutionCreated` for attempt-races)
for runs that were refused — misleading history, part of the §3 blocker. No
other journal dishonesty found; provider JSON never journaled; submit reason
carries bounded provider text (redaction is G6).

## 11. Commands and census

```text
cargo fmt --all -- --check                        clean
cargo check --workspace                           clean
cargo test --workspace --no-fail-fast             3×: 636 passed, 0 failed, 1 ignored each
cargo clippy --workspace --all-targets --all-features -- -D warnings   clean
cargo test -p provider-claude --test process_fixtures                5× green (13 each)
daemon_grandchild_*                               10× isolated green
B1 keepers / H1 suites / provider_binding / contract suites           green
cargo tree -i provider-claude -e normal → agentd only; -e dev → app-services(tests) only
rg: provider=="claude" in core crates → none; unsafe slicing sweep → byte-frames/SIDs only
probes (removed): zz_audit_race (exec-race 6/6 blocker, attempt-race structural, H4 wedge),
  zz_neg_control (2/2 contract rejects violation), zz_parser_scale (linear doubling)
real smoke: claude 2.1.268, ~1 min wall, Task VERIFYING, sessions.db observed, daemon shut down after
```

Census delta vs TASK-0048 baseline (624 + 1 ignored): +12 (B1 keepers, H1
units, H2 decoder/linear, H8 no-spawn, 2 contract invocations, assertion
reworks). Ignored: `provider-claude/tests/smoke.rs` (env-gated live test).

## 12. Final classification table

| Item | Verdict | Evidence |
|---|---|---|
| B1 sequential | RESOLVED | §2 keepers |
| B1 concurrent | G5_BLOCKER | §3, 6/6 + structural |
| B2 | RESOLVED | §2, joint drain + repeats |
| B3 | RESOLVED | §2, suite + neg. control + deps |
| H1 | RESOLVED | §7 |
| H2 | RESOLVED | §7, linear timings |
| H3 | RESOLVED | §7 |
| H4 | G5_BLOCKER (promoted) | §4 |
| H5 | ACCEPTABLE_DEFERRED (+doc fix) | §6/§7 |
| H6 | RESOLVED (documented) | §7 |
| H7 | ACCEPTABLE_DEFERRED | §7 + smoke |
| H8 | RESOLVED | §7 |
| H9 | ACCEPTABLE_DEFERRED | §7 |
| process-count test | HARDENING_FOLLOWUP | §8 |

## 13. Verdict

**`G5_REMEDIATION_REQUIRED`.** TASK-0049 fixed the sequential cases it
targeted, but the entry boundary is not atomic: concurrent identity races
reproduce the exact B1 durable-state poisoning (§3), and the deferred H4
slug collision is deterministically reachable through production IPC and
wedges tasks the same way (§4). G5 stays `in_progress`. Next is TASK-0051
(entry atomicity + slug-conflict preflight + ADR-0024 §6 amendment + keepers),
followed by another re-audit. G6 must not start; the TASK-0048 forward check
(Policy Engine / Secrets Broker / Security Audit ADRs before G6
implementation) still stands.

## 14. Planning/version

`product/version.toml` → `0.1.0-dev.task0050` (gate G5). Queue:
TASK-0050 completed; TASK-0051 (G5 entry-atomicity remediation) queued next.
