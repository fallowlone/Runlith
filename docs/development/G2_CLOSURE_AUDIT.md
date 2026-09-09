# G2 Closure Audit

> TASK-0030 — final G2 verification (audit only, no production repairs).
> Independent source inspection; TASK-0022…0029 summaries were not accepted
> as proof. Production sources were frozen for this task; the only changes
> are `product/version.toml` (mandated bump), this report, and one new
> audit test under `bins/agentctl/tests/`.

## Result

```text
G2_INCOMPLETE
```

G2 **substance is fully verified**: every architectural, behavioral,
persistence, IPC, security, durability and CLI check below passes on
independent evidence. Closure is blocked by exactly one defect class,
in versioning-test scaffolding (not in Task Core substance): the
task-mandated advancement of `product/version.toml` to `task0030`
breaks two hardcoded `task0029` literals in frozen `#[cfg(test)]`
modules, so the workspace is not green in its final state (§44
requires green for `G2_COMPLETE`, and the freeze forbids repairing
frozen sources in-audit). Details in “Workspace Baseline” and
“Gate Transition”.

Blockers (both the same root cause):

```text
1. crates/product-version/src/lib.rs:184
   tests::canonical_file_parses_to_task0029 — asserts the literal
   "0.1.0-dev.task0029"/"TASK-0029"; derived value is now task0030.
2. crates/ipc-control/src/negotiation.rs:297-298
   build_versions_report_canonical_development_identity — asserts the
   literal "0.1.0-dev.task0029" twice; derived value is now task0030.
```

The derivation mechanism itself is proven correct (it produced
`task0030`), and the dynamic assertions sitting beside the literals
(`gate_matches_planning_queue`, the `development_version()` equality
at negotiation.rs:299-302) PASS with the new version. This is a stale
test-pin defect, not a product defect.

## Product Version

```toml
semver = "0.1.0"
development = "0.1.0-dev.task0030"
task = "TASK-0030"
gate = "G2"
```

- `semver` preserved at `0.1.0` (no release bump). PASS
- `development`/`task` advanced per VERSIONING.md. PASS
- `gate = "G2"` because the gate does not transition on INCOMPLETE
  (§46). PASS
- Product version != Cargo package versions (`0.1.0` workspace-wide,
  untouched). PASS
- Product version != IPC protocol versions (`0, 1`, untouched). PASS
- Runtime handshake `build_version` derives from the canonical file via
  the zero-dependency `product-version` crate
  (`default_daemon_build_version` / `default_client_build_version`);
  no second editable version string exists (repo-wide grep finds only
  the derivation sites plus the two stale test literals). PASS
- Validated through the existing mechanism (`cargo test -p
  product-version`): consistency/shape/gate pins pass; only the stale
  task literal fails (blocker above).

## Scope Audited

```text
Project, Task, Dependency, Task state machine, SQLite, Event Journal,
agentctl — and NO AI.
```

All G2 production paths were inspected at source: `crates/task`,
`crates/app-services`, `adapters/storage-sqlite` (code + migrations),
`crates/ipc-contracts` (incl. both protos), `crates/ipc-control`,
`crates/ipc-transport`, `bins/agentd`, `bins/agentctl`, both JSON
schemas, and the full G1/G2 test harnesses.

## Workspace Baseline

Pre-bump baseline (tree at `task0029`, before the mandated bump):

```text
cargo fmt --all -- --check          PASS (exit 0)
cargo check --workspace             PASS
cargo test --workspace              PASS — 242 passed, 0 failed
cargo clippy --workspace --all-targets --all-features -- -D warnings
                                    PASS
```

Final state (after mandated bump + one new audit test, 243 total):

```text
cargo fmt --all -- --check          PASS
cargo check --workspace             PASS
cargo test --workspace --no-fail-fast
                                    241 passed, 2 failed —
                                    ONLY the two stale-pin tests above
cargo clippy --workspace --all-targets --all-features -- -D warnings
                                    PASS (includes the new test target)
```

Per-suite final counts (passed/failed): agentctl lib 10/0,
business_no_replay 1/0, real_process_matrix 1/0, task_core_matrix
13/0, agentd lib 26/0, app-services lib 3/0, service.rs 14/0,
ipc-contracts 16/0, ipc-control 34/1 (pin), ipc-transport 32/0,
product-version 3/1 (pin), storage-sqlite lib 3/0, durability.rs 22/0,
journal.rs 21/0, task-domain 42/0.

## Task Domain

- Census is exactly **12** persistent lifecycle states
  (`TaskState::all()`, `state.rs:32`), in canonical contract order;
  the historical erroneous “14” does not appear anywhere. PASS
- Rust state names match the canonical schema byte-for-byte
  (`as_str()` + `schema_names_stay_aligned_with_contract` test which
  includes the schema file at compile time). PASS
- One authoritative transition table exists
  (`ACCEPTED_TRANSITIONS`, 11 edges, `state.rs:86`); `can_transition`
  is a pure membership check; the exhaustive 12×12 matrix test pins
  the table. PASS
- Lifecycle readiness stays distinct from scheduling readiness: no
  `RUNNABLE`/`WAITING_ON_*` concepts exist in the domain crate; the
  dependency module contributes only to graph validation, and
  task-engine.md §8 separation holds in code. PASS

## Completion Authority

Hard gate. Full-workspace grep for `complete_task`, `set_state`,
`set_status`, `mark_completed`, `task done`, `task complete`,
`arbitrary transition` finds only doc comments stating their absence;
no such API exists in domain, service, storage, dispatch, or CLI.
`Task::submit_result` (`RUNNING → SUBMITTED`) and
`Task::verification_succeeded` (`VERIFYING → COMPLETED`) are distinct
methods/commands/wire ops, tested never to alias
(`submit_is_not_completion`, `verification_succeeded_fails_outside_verifying`,
`direct_paths_to_completed_are_rejected`). Only the trusted
verification-success path reaches `COMPLETED`, and only from
`VERIFYING` (`only_verifying_reaches_completed`). CLI/IPC surface
checked separately below — no generic path to `COMPLETED` exists
there either. PASS

## Dependency Graph

Proven through domain (`task.rs`, `dependency.rs`), service
(`add_dependency` pre-validates + `validate_acyclic`), storage
(re-validates before write), and the live IPC matrix
(`dependency_e2e_with_invalid_edges`):

```text
A depends on B → valid                              PASS
A depends on A → rejected (domain + storage)        PASS
duplicate A→B → rejected (domain + storage)         PASS
A→B, B→C, C→A cycle → rejected (validate_acyclic)   PASS
cross-project edge → rejected (domain + storage)    PASS
```

No invalid persistent graph is committable through any supported
public API (invalid attempts write nothing:
`dependency_cycle_is_rejected_without_mutation`,
`cross_project_dependency_is_rejected_without_mutation`,
`dependency_add_commits_typed_event_and_invalid_edges_write_nothing`).
`FAILED`/`CANCELLED` have no incoming edges (all such transitions
unresolved per architecture, fail closed). PASS

## Persistence & Migrations

- `task-domain` has **zero** dependencies
  (`cargo tree -p task-domain` shows the crate alone); no
  SQL/rusqlite types in domain APIs (boundary type is the plain-data
  `TaskSnapshot`, rebuilt exclusively via validated `Task::restore`).
  PASS
- Migration chain independently verified by suite + source:
  fresh → 0001 → 0002 → 0003 → version 3
  (`fresh_database_reaches_v3`, `fresh_database_migrates_to_current`);
  v1 → v3 with data preserved
  (`v1_database_with_data_migrates_preserving_state`);
  v2 → v3 with rows preserved
  (`v2_journal_rows_survive_migration_to_v3`); v3 reopen is a no-op
  (`v3_reopen_is_a_noop`,
  `migration_is_idempotent_and_preserves_data`); future version
  rejected (`future_schema_version_fails_closed`,
  `future_version_rejected_after_v3`). PASS
- Historical migrations never rewritten: git history shows 0001/0002
  created once (TASK-0027) and 0003 created once (TASK-0028), with no
  later modifications (`git log --follow` per file; TASK-0028 diff
  touches neither 0001 nor 0002). PASS
- Foreign keys: every production connection runs
  `PRAGMA foreign_keys = ON` plus `verify_foreign_keys` (fail-closed
  when inactive) in `SqliteStore::open`; `foreign_keys_are_actually_enabled`
  and `foreign_keys_enabled()` prove it is active at runtime, not
  merely declared. PASS

## Current-State Authority

Relational SQLite tables (`projects`, `tasks`, …) are the v1
current-state authority. Source grep finds no replay/reducer-based
current-state reconstruction anywhere in production code; journal
reads decode rows for history/projection only, and
`current_state_survives_journal_loss` proves state does not depend on
the journal. Migration 0003 header and `task_store.rs` header state
the authority explicitly. PASS

## Event Journal

- Append-only through production APIs: the only journal write path is
  `insert_journal_event` inside a caller-owned transaction; no
  `update_event`/`delete_event`/`replace_event` API exists (grep
  confirms). PASS
- Typed (closed 12-variant `EventType`, stable canonical names) and
  versioned (`EVENT_SCHEMA_VERSION = 1`, explicit on write, strict on
  read; unknown type/version fail as corruption, never defaults).
  PASS
- Ordered deterministically (`seq` AUTOINCREMENT, `ORDER BY seq`;
  `journal_order_follows_commit_order`). PASS
- Distinct from Security Audit: separate concepts in code and docs;
  peer-auth outcomes never enter the journal (proven by
  `denied_peer_cannot_reach_business_dispatch_or_storage` asserting an
  empty journal after denied attempts). PASS
- Scope (migration 0003): `project.created` rows carry NULL
  `task_id`/`from_state`/`to_state` with a CHECK constraint (no
  fake/sentinel task ids); every other type carries full task scope;
  scope re-validated on write (`require_scope`) and on read (mis-scoped
  rows are corruption: `scope_check_rejects_mis_scoped_rows`). Pre-0003
  Task rows decode unchanged (verbatim `seq`-preserving copy +
  `sqlite_sequence` pinning, tested). PASS
- Coverage: all four supported persistent mutations carry typed
  evidence — `project.created`, `task.created`,
  `task.dependency_added`, plus the nine lifecycle types
  (`project_creation_commits_project_created_event`,
  `task_creation_commits_task_created_event`,
  `dependency_add_commits_typed_event…`,
  `full_lifecycle_commits_matching_ordered_history`,
  `business_audit_trail_is_complete_and_ordered`). Read-only
  operations emit nothing (`read_only_operations_emit_no_events`,
  `reads_emit_no_mutation_events`). Dependency removal is not exposed
  on the wire, so no `dependency_removed` type exists — exactly per
  ADR-0021 §7/§19-20 (see Escape-Hatch Audit). PASS
- No secrets in journal schema/construction: columns are
  type/version/refs/states/reason/correlation/causation/timestamp/provenance
  only; `journal_columns_carry_no_secret_or_transcript_fields` and
  `journal_event_facts_have_no_secret_fields` pin this. PASS

## Mutation Atomicity

Hard gate. Every exposed persistent mutation commits state + event in
one SQLite transaction (`create_project_with_event`,
`create_task_with_event`, `add_dependency_with_event`,
`apply_lifecycle_transition` with guarded `UPDATE … WHERE state =
expected`). Failure injection proven:

```text
state write fails → event absent
  (state_write_failure_commits_no_event)                    PASS
event append fails → state rollback
  (event_append_failure_rolls_back_state)                   PASS
invalid domain mutation → state unchanged + event absent
  (invalid_transition_changes_nothing_and_appends_nothing,
   mismatched_event_references_write_nothing,
   stale_expected_state_rolls_back_with_no_event,
   invalid_lifecycle_is_an_atomic_noop over live IPC)       PASS
creation/dependency atomicity
  (creation_mutations_are_atomic_with_their_events,
   dependency_mutation_is_atomic_with_its_event)            PASS
```

## Mutation Escape-Hatch Audit

All public production storage/application mutation APIs inspected:
`SqliteStore::{create_project, create_task, add_dependency,
remove_dependency, save_task}` (raw, unjournaled primitives) and the
`*_with_event` / `apply_lifecycle_transition` (journaled) variants,
plus `TaskService` (journaled-only) and `agentd` dispatch/composition.

- Every IPC-reachable authoritative mutation travels
  `agentctl → IPC → agentd dispatch → TaskService journaled method →
  single transaction`. Dispatch calls only `service.create_project`,
  `create_task`, `add_dependency`, and the nine lifecycle methods
  (verified by source grep: zero raw-store calls in `bins/agentd`).
  PASS
- Raw store primitives are unreachable as a mutation authority in any
  production binary path: the only in-workspace callers are
  `storage-sqlite`’s own tests on test-owned scratch databases;
  `TaskService::store()` exposes only a shared (`&`) reference, so no
  caller can mutate through it; `TaskService::attach` in production
  (`open_business_service`) wraps a freshly opened store with no
  intervening raw writes. PASS (by reachability evidence above)
- Known `remove_dependency` concern (TASK-0028): public, unjournaled,
  idempotent edge delete; NOT exposed over IPC (no wire variant, no
  service method, no CLI verb) and intentionally without a journal type
  per ADR-0021 §7 / events.md §4.1 (“a future removal API must define
  `task.dependency_removed` first”). It cannot be reached through any
  supported production path, so it does not constitute an
  authoritative unaudited mutation path today; carried as a Known
  Issue, not a blocker. PASS (with that recorded constraint)

## Corruption Matrix

Fail-closed behavior proven with test-owned databases (no silent
repair/default mapping):

```text
unknown Task state
  (unknown_persisted_state_is_corruption)                   PASS
invalid TaskId/ProjectId
  (invalid_persisted_identifiers_fail_reconstruction)       PASS
unknown Event type / unknown Event version
  (unknown_event_type_fails_explicitly,
   future_event_version_fails_explicitly)                   PASS
invalid event scope (mis-scoped rows)
  (scope_check_rejects_mis_scoped_rows)                     PASS
future schema version
  (future_schema_version_fails_closed)                      PASS
broken relation bypassing FK
  (broken_reference_without_fk_fails_explicitly)            PASS
unknown metadata slot in task_strings → Corrupt             PASS
```

## Data Directory Security

Unix/macOS host, `ensure_data_dir` + `resolve_data_dir`
(`bins/agentd/src/data_dir.rs`):

```text
missing dir → created 0700 (umask-hardened, re-validated)   PASS
safe owned 0700 dir → accepted                               PASS
symlink → rejected, never followed                          PASS
regular file at dir path → rejected, never replaced         PASS
insecure mode (0755) → rejected, never repaired             PASS
foreign ownership → uid check in code; no destructive repair.
  NOT_EXECUTED as a live test (requires root/chown; not safely
  testable here). Non-blocking: same current_uid() mechanism as the
  proven peer-auth check. Code path reviewed, fail-closed.
```

No fallback to `/tmp`, cwd, repository, or socket dir: resolver fails
closed (missing HOME / relative override / non-absolute XDG all
error); daemon startup fails rather than serving without durable
state (`database_failure_fails_startup_clearly`). PASS

## Business IPC v0/v1

- Versions `0` (bootstrap-only) and `1` (G2 business) coexist;
  highest-common selection verified both ways
  (`g2_daemon_serves_both_versions_with_v1_current`,
  `version_selection_matrix`):
  `[0]×[0,1]→0`, `[0,1]×[0,1]→1`. PASS
- v0 executes no business: `validate_business_allowed(0)` errors
  `UnsupportedProtocol`; live-proven against a real daemon
  (`v0_client_keeps_g1_surface_and_cannot_run_business`: `[0]`-only
  handshake negotiates 0, smuggled business frame unanswered, daemon
  unharmed). PASS
- v1 surface is exactly the accepted bounded contract
  (`task_core.proto`): Project create/get/list, Task
  create/get/list-for-project, Add/ListDependencies, nine semantic
  lifecycle ops 1:1 with `TaskService`; bounded reason (4096,
  enforced at dispatch + service); typed 9-code business errors;
  sequential one-in-flight model preserved; no generic RPC. PASS
- No ambiguous mutation replay: after the request bytes are written,
  any failure (lost/undecodable/wrong-variant/timed-out reply) is
  `BusinessAmbiguous`, which the retry classifier pins non-retryable
  (`retry_classification_fails_closed`); new live audit test
  `business_no_replay.rs` proves exactly one frame is sent and none
  follows (`business_mutation_is_sent_once_then_reported_ambiguous`).
  PASS

## Peer Authentication

Hard gate (ADR-0020, Unix path live on this host):

```text
OS peer auth + handshake + negotiated v1 required for business  PASS
valid-looking business frame with denied peer → no READY, no
  SQLite mutation, no journal mutation
  (mismatched_peer_is_rejected_before_handshake,
   denied_peer_cannot_reach_business_dispatch_or_storage
   asserting an empty journal afterwards)                       PASS
client_kind non-authoritative (both kinds rejected for a
  mismatched peer)                                              PASS
PID/path/process-name non-authoritative (kernel UID only)       PASS
reconnect → fresh transport + fresh auth + fresh handshake, no
  inherited state (reconnect_reauthenticates_every_connection)  PASS
```

Windows staged first-frame/SID path is implemented per ADR-0020 §5-8
and compile-verified; runtime execution is Windows-CI debt (see
Windows Verification). PASS (Unix live; Windows staged + compiled)

## agentctl Surface

- `cargo tree -p agentctl --edges normal`: only
  bytes/ipc-contracts/ipc-control/ipc-transport/prost/tokio. NO
  rusqlite, NO storage-sqlite, NO app-services, NO task-domain in
  normal deps (storage appears under dev-dependencies for the test
  harness only). Business state reachable only through `agentd` over
  IPC. PASS
- CLI census (`main.rs`): `start|ping|shutdown`,
  `project create|get|list`, `task create|get|list`,
  `task dependency add|list`,
  `task ready|claim|start|resume|block|submit|review|verify-begin|verify-fail|verify-succeed`.
  Grep for `set-state|set-status|complete|done|raw-sql|exec-sql`
  finds nothing; unknown verbs are usage failures (proven live in the
  manual matrix and by `no_generic_state_setter_exists`). No
  `cancel`/`fail` verbs (edges unresolved). Minor observation: the
  `usage_project()` string mentions an `events` verb that is not
  implemented (rejected as usage) — cosmetic only. PASS
- Composition ownership: `agentd` is the composition root
  (`open_business_service` → `SqliteStore::open` → `TaskService` →
  `dispatch`). Dispatch contains no lifecycle rules, no cycle
  detection, no event construction, no verification authority — all
  delegated to `TaskService`/domain/storage. PASS
- Error sanitization: wire errors carry only static text plus
  validated identifiers/state names, truncated (`map_error`,
  `truncate`); raw SQLite errors, paths, SQL, Debug dumps and
  backtraces map to static strings — proven by
  `error_messages_carry_no_paths_or_sql` and
  `unknown_operation_and_empty_request_are_invalid_argument`.
  Transport-level diagnostics may name the caller’s own socket
  endpoint (usability, not a DB path or secret). PASS

## Restart & Crash Durability

Real file, real daemon, test-owned sandbox (`task_core_matrix.rs`
plus an independent manual binary run by this audit):

```text
create Project → Tasks → Dependency → lifecycle mutations →
  graceful shutdown → restart → all state + journal + order survive
  (restart_durability_e2e, project/task_e2e_with_restart,
   manual run: T-1 COMPLETED survived restart)                  PASS
commit → kill -9 agentd → restart →
  all committed Project/Task/Dependency/Events survive
  (kill9_crash_preserves_committed_state,
   manual run: T-2 CLAIMED survived kill -9)                    PASS
```

Uncommitted (in-flight) operations are not claimed durable (SQLite
semantics; stale socket recovered on restart). Manual run also
verified terminal-state rejection (`COMPLETED` rejects everything),
`INVALID_TRANSITION` atomicity over live IPC, forbidden verbs
(`complete`, `set-state`) rejected as usage, `shutdown` /
`shutdown-absent` behavior, `state.sqlite3` distinct from
`agentd.sock`, data dir `0700`, socket `0600`. PASS

## G1 Regression

Full existing G1 matrix re-run green, untouched and unweakened:
`g1_real_process_matrix` (start/handshake/ping/negotiation/reconnect/
shutdown/daemon-missing/stale-socket/crash-recovery/singleton/
TOCTOU-safe cleanup) plus in-process suites for malformed framing
(zero/oversized/malformed-protobuf/ordering/second-handshake/unknown-kind/
capabilities/unexpected-variant), 8-client bound with per-connection
isolation (`ninth_concurrent_client_is_closed_not_fatal`,
`malformed_client_does_not_kill_daemon`), and no-resync behavior.
G2 broke nothing in G1. PASS

## Windows Verification

Host is macOS (no Windows runtime claimed):

```text
cargo check --target x86_64-pc-windows-msvc
  -p ipc-contracts -p ipc-control -p ipc-transport
  -p task-domain -p product-version -p agentctl               PASS
corresponding clippy (lib, -D warnings)                       PASS
  (--all-targets clippy pulls dev-dep rusqlite → same C-toolchain
   limitation below; lib clippy is clean)
cargo check -p agentd --target x86_64-pc-windows-msvc
  NOT_EXECUTED / ENVIRONMENT LIMITATION — sole failure is
  libsqlite3-sys C compilation (no MSVC C toolchain on this macOS
  host: cc cannot target msvc, stdlib.h absent). No Rust or product
  error appears. NOT a G2 product blocker per the task’s known
  limitation clause.
runtime execution (business/SQLite/peer-auth)                 NOT_EXECUTED —
  no Windows host. Explicit future CI debt (as in G1).
```

## Dependency Boundaries

`cargo tree` per crate (normal edges):

```text
task-domain independent of adapters (zero deps)               PASS
agentctl independent of SQLite/domain/app-services            PASS
agentd composition root (app-services + storage-sqlite)       PASS
no Tauri / provider SDK / tonic-gRPC / HTTP-web-framework /
  cloud-account deps anywhere in manifests                    PASS
```

## No-G3-Leakage

Production grep for `FakeAgent`, `AgentProvider` implementations,
`Scheduler`, execution/attempt runtimes, provider SDKs, LLM calls,
prompt execution, worktree execution: only architecture-doc mentions
and process-lifecycle vocabulary (e.g. “attempts to bind”) — no G3
runtime behavior implemented. `task_core.proto` carries no
Execution/Attempt/Provider/Agent/Runner/Worktree/Context/Memory/Scheduler
messages and no dynamic method strings. PASS

## G3 ADR Activation Readiness

Per ADR-0019, decisions ACTIVE at G3 (FakeAgent runtime:
Task → orchestration → FakeAgent → submit_result → verification):

```text
Planning/Orchestration  ACTIVE PRE-G3 → ADR-0017 Accepted       PASS
Evaluation/Verification ACTIVE PRE-G3 → ADR-0018 Accepted       PASS
Provider ACL (FakeAgent seam) → ADR-0009 Accepted               PASS
Task model / event staging → ADR-0004/0005/0006 Accepted        PASS
Coverage register (COVERAGE.md) classifies PRE-G3 rows covered  PASS
```

Open provider/runner port shapes (providers.md §16, runners.md §13)
are listed Open Design Items, not missing decisions: no new ADR is
required to *begin* G3, and none was created in this audit. Research /
deferred targets (model router, residency, tools registry, artifact
store, security runner, web acquisition, …) are not G3 implementation
dependencies and do not block. PASS — G3 may open once the test-pin
remediation below lands and this audit is re-run green.

## Known Issues

1. (Blocker) Stale version-pin literals fail the workspace after the
   mandated bump (see Result). Remediation is a 3-line dynamic-pin
   task, not a G2 substance repair.
2. `SqliteStore::remove_dependency` (public, unjournaled, pre-existing
   from TASK-0023) stays reachable only as a library primitive, never
   through IPC/service/CLI. A future removal API must define
   `task.dependency_removed` and a journaled path first (ADR-0021 §7).
3. `usage_project()` help string mentions an unimplemented `events`
   verb (rejected as usage today). Cosmetic.
4. `real_process_matrix.rs` (G1, predates the database) runs
   production binaries without the `AI_ENGINEERING_OS_*` sandbox
   overrides, so the spawned daemon opens the user’s canonical
   `state.sqlite3`. Proven read-only in practice (G1 verbs only;
   production DB mtime untouched by this audit’s runs), but the suite
   should gain the sandbox overrides in a follow-up. No production
   state was written by any audit test or manual run (all business
   flows used test-owned sandboxes; pre-existing production DB left
   byte-identical).
5. Windows runtime execution (business/SQLite/peer-auth) remains
   future CI debt; full Windows `agentd` link needs an MSVC C
   toolchain for `rusqlite` bundled.

## Gate Transition

Not performed (§46 incomplete behavior):

```text
G2.status:        in_progress (unchanged)
G3.status:        planned (unchanged)
current_gate:     G2 (unchanged)
completed_through: TASK-0029 (unchanged)
product/version.toml → task TASK-0030, gate G2 (advanced; audit artifact)
```

Smallest recommended next task: **TASK-0031 — Version-pin rollover**:
replace the hardcoded `task0029` literals with dynamic derivation
checks in exactly two `#[cfg(test)]` modules
(`crates/product-version/src/lib.rs`
`canonical_file_parses_to_task0029`;
`crates/ipc-control/src/negotiation.rs`
`build_versions_report_canonical_development_identity`), explicitly
authorizing those modules; then re-run the G2 closure audit. No
production logic changes; no G3 work begins before the re-audit.

## Evidence

- Pre-bump: fmt/check/test(242/242)/clippy green (this audit).
- Post-bump: fmt/check/clippy green; test 241 passed + 2 failed
  (both stale-pin literals, quoted above).
- Manual real-binary matrix (§43): sandboxed start → project/task/
  dependency E2E → full DRAFT→COMPLETED lifecycle (incl.
  block/resume) → invalid-transition + forbidden-verb rejections →
  restart durability → kill-9 durability → shutdown/shutdown-absent;
  data dir `0700`, socket `0600`, `state.sqlite3` distinct from socket.
- Suites: `task_core_matrix` (13), `real_process_matrix` (1),
  `business_no_replay` (1, new), `service` (14), `journal` (21),
  `durability` (22), domain (42), transport (32), control (35).
- Windows: pure-Rust target check + clippy PASS (6 crates);
  `agentd` target check fails solely on the MSVC C toolchain
  (logged output retained in audit working notes).
- Git: migrations 0001/0002/0003 each created once, never rewritten.
