# G2 Business IPC Implementation (TASK-0028)

> How the ADR-0021 contract was implemented as the complete G2 vertical
> slice. ADR-0021 itself is unchanged and remains authoritative; this
> document records implementation status, evidence, and limitations.

## Implemented Surface

`agentctl project create|get|list`, `agentctl task create|get|list`,
`agentctl task dependency add|list`, and the nine semantic lifecycle
commands `ready|claim|start|resume|block|submit|review|verify-begin|
verify-fail|verify-succeed` (plus G1 `start|ping|shutdown`, unchanged).
`resume` spells the accepted `START` resume edges; there is no
`set-state`, `transition`, `complete`, or `done` command — unknown verbs
are usage failures. No history-query command exists: the frozen v1
contract carries no history request, so journal audit is verified
through the storage adapter in test harnesses only.

Wire mapping is 1:1 with `TaskService` (`bins/agentd/src/dispatch.rs`):
`CreateProject/GetProject/ListProjects`, `CreateTask/GetTask/
ListTasksForProject`, `AddDependency/ListDependencies`, and the nine
`TaskLifecycleOp` values. `submit-result != verification-succeeded`
holds on the wire, in dispatch, and in the CLI; `COMPLETED` is reachable
only through the trusted verification-success operation.

## Data Composition

```text
agentctl → IPC → agentd → app-services → storage-sqlite
```

`agentd` is the composition root: it resolves the database path,
validates (creating, 0700) the data directory, opens and migrates the
store, wraps it in `TaskService`, and serves one bounded typed
dispatcher per v1 session (`BusinessHandle =
Arc<tokio::sync::Mutex<TaskService>>`; one request in flight per
connection, sessions serialize through the single SQLite connection).
`agentctl` links only `ipc-contracts`/`ipc-control`/`ipc-transport`
plus `prost`/`bytes`/`tokio` — `cargo tree -p agentctl --edges normal`
shows no `rusqlite` and no `storage-sqlite` (both appear only under
dev-dependencies for the test harness, alongside `agentd` itself).

Canonical location `ai-engineering-os/state.sqlite3` is implemented in
`bins/agentd/src/data_dir.rs` (macOS Application Support, Linux
XDG_DATA_HOME with `~/.local/share` fallback, Windows LocalAppData;
`AI_ENGINEERING_OS_DATA_DIR` overrides the parent for tests).
Production `agentd` always opens the database at startup and fails
clearly (`DataDir`/`Storage` errors, no fallback, no alternate path) —
v0 negotiation semantics never depend on storage availability, so the G1
surface is unaffected by business-storage state.

## Migration 0003

`adapters/storage-sqlite/migrations/0003_event_scope.sql`, schema
`2 → 3`. The journal table is rebuilt with `task_id NULL` for
legitimate project-scoped events plus a scope `CHECK`
(`project.created` ⇔ no task scope; every other type ⇔ full task
scope), the 12-type event CHECK, preserved indexes, verbatim row copy
(`seq` included), and a pinned `sqlite_sequence` continuation.
Historical migrations untouched. Fresh → v3, v1 → v3 (chained), v2 →
v3 (preserved), v3 reopen no-op, and future-version fail-closed are all
tested.

## Business Event Coverage

`project.created`, `task.created`, `task.dependency_added` join the
nine lifecycle types (`EventType::all()` is now 12; no generic event
exists). `NewJournalEvent.task_id/from_state/to_state` are `Option`:
`project.created` carries no task scope at all (never a fake or
sentinel id); `task.created` records the initial state;
`task.dependency_added` records the unchanged current state. Storage
re-validates scope on write and on read (mis-scoped rows are
corruption). Creation/dependency commits are single transactions:
project row + event, task image + event, edge + event. Dependency
removal is not exposed on the wire (contract excludes it), so no
`dependency.removed` type exists. Reads (`get/list/dependencies`)
emit no events. Relational tables stay authoritative; no replay-based
reconstruction was introduced.

## Authentication / Version Boundary

Every business byte stays behind OS peer authentication → handshake →
negotiated v1. A v0 session receiving `TaskCoreRequest` gets
`UnsupportedProtocol` with zero execution (session-terminating, proven
live against a real daemon in `task_core_matrix`). A denied peer never
reaches handshake, dispatch, or storage (in-process daemon test asserts
an empty database afterwards). `client_kind` remains descriptive only.

## Real Process Evidence

`bins/agentctl/tests/task_core_matrix.rs` (Unix, per-test sandbox
endpoint + database): project/task E2E with restart, dependency E2E
with self/duplicate/cross-project/cycle/unknown-target rejections,
full DRAFT → COMPLETED lifecycle with ordered journal assertion,
invalid-lifecycle atomic no-op, submit-not-completion, CLI surface
discipline, read-only-no-events, complete ordered audit trail,
combined restart durability, kill-9 crash durability, and v0
compatibility (live `[0]`-only handshake negotiates 0; smuggled
business frame unanswered; daemon unharmed). Peer-auth denial and the
dispatch round trip are additionally covered in-process in `agentd`.
The G1 `real_process_matrix` is untouched and green.

## Restart / Crash Durability

Graceful shutdown + restart preserves projects, tasks, states, edges,
and the verbatim journal (order included); the reopened daemon keeps
committing with deterministic `evt-<seq>` continuation. `kill -9`
mid-flow preserves everything committed; the stale socket is recovered
on restart. In-flight (uncommitted) operations fail rather than
half-commit, per SQLite semantics.

## Backward Compatibility

Old `[0]`-only clients negotiate 0 against the `[0,1]` daemon and keep
`ping`/`shutdown`; `agentctl start|ping|shutdown` still handshake at
v0. All G1 suites (transport, control, agentd, agentctl) pass
unchanged. No ambiguous business mutation is ever replayed
(`BusinessAmbiguous`, non-retryable by unit test); reconnects always
re-authenticate and re-handshake with no inherited state.

## Windows Verification

Pure-Rust IPC layers (`ipc-contracts`, `ipc-control`, `ipc-transport`)
were checked for the Windows target where the toolchain permits; the
Windows pipe-name test override (`AI_ENGINEERING_OS_PIPE_NAME`) and
SID policy tests are compile-gated for Windows CI. Full `agentd`
links `storage-sqlite` (`rusqlite` `bundled`), which needs an MSVC C
toolchain absent from this macOS host — see Known Limitations.
Windows runtime execution remains future CI debt (as in G1).

## Known Limitations

- Full Windows-target compilation of `agentd`/`agentctl` (and therefore
  Windows runtime tests) is blocked by the missing MSVC C toolchain for
  `rusqlite` `bundled` on this host — an environment limitation, not a
  Rust defect. Recorded per task §45; no `bundled` change was made.
- No `task/ project events` CLI: the frozen v1 contract has no history
  request. Journal audit over IPC would need a contracted history
  query in a later version.
- `storage-sqlite::remove_dependency` (unjournaled, pre-existing) is
  not exposed over IPC and gains no event; a future removal API must
  define `task.dependency_removed` first.
- Concurrent sessions serialize through one SQLite connection behind
  an async mutex — correct for G2 scale, not a throughput design.
