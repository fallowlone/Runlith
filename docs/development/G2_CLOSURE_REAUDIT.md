# G2 Closure Re-Audit

> TASK-0032 — verification-only re-audit after the TASK-0031 version-pin
> remediation. TASK-0030’s audit is evidence, not authority: every claim
> below was re-verified against current source. Production implementation
> was frozen; nothing was repaired in this task.

## Result

```text
G2_COMPLETE
```

Both TASK-0030 blockers are resolved, the full workspace is green at
`0.1.0-dev.task0032`, every substantive G2 matrix re-passes, and the
gate transition G2 → G3 was performed.

## Product Version

Audit phase:

```toml
semver = "0.1.0"
development = "0.1.0-dev.task0032"
task = "TASK-0032"
gate = "G2"
```

Final (after G2_COMPLETE transition):

```toml
semver = "0.1.0"
development = "0.1.0-dev.task0032"
task = "TASK-0032"
gate = "G3"
```

Public SemVer preserved at `0.1.0`. Protocol versions unchanged
(`0, 1`). Runtime `build_version` resolves dynamically to
`0.1.0-dev.task0032` (proven by the repaired tests, no literals).

## TASK-0030 Blocker Resolution

```text
crates/product-version canonical_file_is_self_consistent      PASS (4/4 crate)
crates/ipc-control build_versions_report_canonical_…         PASS (35/35 crate)
```

- No current canonical test hardcodes `task0029`, `task0030`, or
  `task0031` as expected identity (repo-wide search; remaining hits
  are deliberate parser-rejection fixtures, doc `e.g.`s, provenance
  comments, and historical records — all allowed categories).
- Bumping `task0031` → `task0032` required zero test edits: the
  blockers are structurally gone, not rolled forward.

## Version Rollover Safety

- TASK-0031 demonstrated `task0031` → `task0032` simulation green
  without test edits; this task executed that exact bump for real and
  the suites stayed green, confirming the proof.
- The repaired tests assert relationships (suffix shape, task-suffix
  match, derivation from the canonical file, gate-vs-queue agreement),
  so `task0033`, `task0100`, … need no test changes. PASS

## Workspace Baseline

Final state (after gate transition; planning/version agree):

```text
cargo fmt --all -- --check                                     PASS
cargo check --workspace                                        PASS
cargo test --workspace --no-fail-fast                          243 passed, 0 failed
cargo clippy --workspace --all-targets --all-features
  -- -D warnings                                               PASS
```

Per-suite: agentctl lib 10, business_no_replay 1, real_process_matrix
1, task_core_matrix 13, agentd lib 26, app-services lib 3, service 14,
ipc-contracts 16, ipc-control 35, ipc-transport 32, product-version 4,
storage-sqlite lib 3, durability 22, journal 21, task-domain 42 —
zero failures.

## G2 Matrix Reconfirmation

No production code changed since TASK-0030 (working-tree diff limited
to `product/version.toml`, two `#[cfg(test)]` hunks, queue, and docs —
verified by hunk inspection), so deterministic suites re-provide the
evidence, all re-run green above:

```text
Task domain (12 states, one 11-edge table, readiness split)      PASS
Dependency graph (self/dup/cycle/cross-project rejected)         PASS
Migrations (fresh/v1/v2→v3, reopen no-op, future rejected)       PASS
Event Journal (typed/versioned/append-only/ordered/scoped)       PASS
Atomic mutations (state-fail/event-fail/invalid → no half-commit)PASS
Corruption handling (fail-closed, no silent repair)              PASS
Data-directory security (0700 discipline, fail-closed)           PASS
Business IPC (bounded v1 surface, typed errors, no replay)       PASS
Peer authentication (UID gate, kind non-authoritative)           PASS
Protocol abuse (framing/ordering/capability discipline)          PASS
```

## Completion Authority

Hard gate re-verified on current source. Grep over `crates/` and
`bins/` for `complete_task|set_state|set_status|mark_completed`
finds only doc comments stating their absence and the negative CLI
test (`no_generic_state_setter_exists` proves `set-state` rejected).
`submit_result != verification_succeeded` holds in domain,
app-services, protobuf (`SUBMIT_RESULT` vs `VERIFICATION_SUCCEEDED`
enum values), agentd dispatch (distinct arms), and agentctl CLI
(distinct verbs). Only `VERIFYING → COMPLETED` via the trusted
verification path exists. PASS

## Mutation Escape Hatches

Re-inspected on current source: `bins/agentd` contains zero raw-store
calls (`remove_dependency`, `save_task`, and raw create/add paths are
absent from dispatch, service, and composition); `TaskService`
exposes journaled mutations only. TASK-0030’s classification stands:
`SqliteStore::remove_dependency` remains public/unjournaled but
unreachable from every authoritative application/IPC flow and
deferred by ADR-0021 §7 (no wire variant, no service method, no
`dependency_removed` type). No reachable path mutates persistent G2
state without transactional journal evidence. PASS

## Real Process Evidence

Explicitly re-run (not inherited):

```text
cargo test -p agentctl --test real_process_matrix      1/1 PASS
cargo test -p agentctl --test task_core_matrix        13/13 PASS
cargo test -p agentctl --test business_no_replay       1/1 PASS
```

Covers: daemon-absent start, project/task create/get/list,
dependency add/list (+invalid edges), full valid lifecycle, invalid
lifecycle atomic no-op, restart durability, kill-9 committed
durability, v0 control compatibility, v1 business flow, peer-auth
business protection, mutation no-blind-replay, shutdown. All data in
test-owned sandboxes.

## G1 Regression

`g1_real_process_matrix` plus all control/transport suites green in
the workspace run: start, handshake, ping, negotiation, reconnect,
shutdown, singleton, stale recovery, malformed framing, TOCTOU-safe
cleanup, 8-client bound. No G2 regression. PASS

## Persistence / Event Journal

- Schema version 3; `PRAGMA foreign_keys = ON` enforced + verified at
  runtime; relational tables authoritative (no replay reconstruction
  in production code); journal history-only. PASS
- Supported mutations still transactionally coupled to typed events
  (creation/dependency/lifecycle suites green). PASS
- No historical migration rewritten: git shows 0001/0002 created once
  (TASK-0027), 0003 once (TASK-0028), zero later modifications. PASS

## Peer Authentication

Suite evidence re-run green (agentd lib 26/26): OS-UID gate before
handshake, denied peers reach no READY/session/storage/journal
(empty-journal assertion), `client_kind`/PID non-authoritative,
reconnect re-authenticates. Business authority still requires
auth + handshake + negotiated v1. PASS

## Version 0 / Version 1

`[0]×[0,1]→0`, `[0,1]×[0,1]→1` (negotiation matrix green); v0 is
handshake/ping/shutdown only with business rejected as
`UnsupportedProtocol` (unit + live-daemon proof); v1 carries the
bounded G2 Task Core surface. Protocol independent of build version.
PASS

## agentctl Boundary

`cargo tree -p agentctl --edges normal` contains none of `rusqlite`,
`storage-sqlite`, `app-services`, `task-domain` (nor any Tauri/gRPC/
HTTP/provider deps). Pure IPC client; composition stays
`agentctl → IPC → agentd → app-services → storage-sqlite`. PASS

## Windows Verification

macOS host; no Windows runtime claimed:

```text
target check (6 pure-Rust crates)                              PASS
target clippy (lib, -D warnings)                               PASS
agentd target check                                            NOT_EXECUTED /
  ENVIRONMENT LIMITATION — sole failure is libsqlite3-sys
  bundled C compilation (no MSVC C toolchain; stdlib.h absent).
  No Rust/product error. Not a G2 blocker.
runtime execution                                              NOT_EXECUTED — CI debt
```

## Known Issues Reclassification

1. `remove_dependency` public/unjournaled, unreachable, ADR-0021
   deferred → **non-blocking** (unchanged; future removal API must
   define `task.dependency_removed` first).
2. `usage_project()` mentions unimplemented `events` verb →
   **non-blocking** cosmetic (unchanged).
3. `real_process_matrix` sandbox overrides → **non-blocking**
   (explicitly verified below, not inherited).
4. Windows runtime CI debt → **non-blocking** (unchanged).

### real_process_matrix production-data verification (§16)

Full 215-line harness re-read plus a live experiment. Capability
inventory vs the canonical production paths (no overrides in the
harness; production `agentd` resolves the canonical DB path):

- CAN touch: canonical socket lifecycle (TMPDIR), store-file
  open/create, lossless schema migration *if* a stale schema existed,
  canonical daemon lifecycle (Phase 0 graceful shutdown of a live
  daemon, kill-9 limited to workspace-binary-path pids).
- CANNOT: write/read/modify any durable user fact. Test verbs are
  exactly {start, ping, shutdown}; the daemon serves those without
  touching tables, and no business frame is ever sent — no code path
  exists from this suite to a project/task/edge/journal row.
- Experiment: production `state.sqlite3` hashed before and after
  running all three real-process suites — identical
  (`8ed0a145…de280094`), no stray files, no lingering daemon.
  Current schema is v3, so migration was a proven no-op.

The worst-case residual writes (empty-store creation on a fresh
machine; tested lossless migration of a stale schema) are
store-lifecycle effects identical to production’s own first-launch
behavior — they cannot alter, fabricate, or destroy user facts.
Follow-up recommended (add the two sandbox env overrides to the G1
harness), but the suite is **not genuinely capable of mutating real
durable user data** → non-blocking. No repair performed (audit-only).

## G3 ADR Activation Readiness

Architecture docs untouched since TASK-0030 (empty diff). Authority
reconfirmed via ADR index/coverage: Planning/Orchestration ACTIVE
PRE-G3 → ADR-0017 Accepted; Evaluation/Verification ACTIVE PRE-G3 →
ADR-0018 Accepted; provider seam → ADR-0009 Accepted; task/event
staging → ADR-0004/0005/0006 Accepted. Open port shapes remain Open
Design Items, not missing decisions. `g3AdrActivationReadiness =
true`. No ADR created here. PASS

## Gate Transition

Performed (all §18 requirements met):

```text
planning/roadmap.yaml:  current_gate G2 → G3, G2 completed, G3 in_progress
planning/tasks/queue.yaml: current_gate → G3, completed_through → TASK-0032
product/version.toml:   development 0.1.0-dev.task0032, task TASK-0032, gate G3
```

Post-transition validation re-run green (check + version suites +
full workspace; planning/version agree via
`gate_matches_planning_queue`). No G3 task created; no G3
implementation begun.

## Evidence

- Audit-phase version (`task0032`/`G2`): fmt/check/243-test/clippy
  green with zero test edits for the bump.
- Blocker suites: product-version 4/4, ipc-control 35/35 at
  `task0032`.
- Real-process suites: 1 + 13 + 1 green; prod-DB hash unchanged
  across the run.
- Windows: pure-Rust check + clippy PASS; agentd blocked only by the
  MSVC C toolchain (logged).
- Git: migrations created once, never rewritten; frozen paths
  (architecture/contracts/prod sources) diff-empty apart from the two
  authorized TASK-0031 test hunks.
