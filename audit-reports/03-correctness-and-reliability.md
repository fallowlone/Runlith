# Correctness and reliability

These findings concern reachable source paths at HEAD `75b176d`. Confirmation is by source/control-flow analysis unless an executed check is explicitly named. A proposed fixture procedure is not reported as an executed reproduction. This audit changed no tests or production code.

## Confirmed correctness defects

### F16 - Routines report completion without executing work

- **Severity:** High. **Confidence:** High.
- **Locations:** `crates/app-services/src/background_service.rs:119-167`; `bins/agentd/src/lib.rs:679-705`; `bins/agentd/src/routine.rs:147-168`; contract `docs/architecture/adr/ADR-0045-g11-background-daemon-contract.md:26-29,66-85,108-114`.
- **Evidence:** both `tick` and `run_now` construct a `RoutineRun`, persist it, immediately call `run.complete(evidence, now)`, and increment the definition's completion count. Neither calls an execution port, a task runner, a verifier, or any function that interprets/executes the objective. The daemon's automatic tick supplies the constant `evidence:daemon-tick`; manual IPC supplies the caller's evidence string. The routine's objective and capability scope never become an execution request in these paths.
- **Root cause:** lifecycle bookkeeping has been wired as if it were an execution implementation. The accepted contract says routines execute in daemon ticks and execution passes through existing Policy/execution ports; the production composition stops at recording a successful row.
- **Input / boundary:** an operator-authored, separately authorized routine reaches the trusted service through IPC or the one-second daemon tick. The trusted service grants `COMPLETED` without observed work/evidence. This does not mark a Task completed or create an arbitrary-process vulnerability; it creates false Routine success.
- **Realistic impact:** a routine such as "run the authorized checks" consumes its run budget and presents completed work while performing no checks. An operator relying on scheduled maintenance/verification receives a false success record.
- **Verification status:** complete source tracing of both execution entry paths; existing `background_service`/`background_integration` tests exercise bookkeeping with caller-provided evidence, not execution of an objective. No real maintenance routine or provider was launched for this audit.
- **Safe reproduction:** in a fixture store, define and authorize a one-run routine with an objective that would require an observable action. Use a recording fake execution port in a future regression test. Invoke `tick`; the present code returns a completed run without any execution dependency to call. Assert that unsupported objectives cannot produce `COMPLETED`, or that a real authorized execution receipt is required.
- **Recommended solution:** immediately fail closed as `BLOCKED`/unsupported when no routine executor is available. Implement the intended routine-to-execution integration through the existing policy/runner contracts before restoring automatic completion. A caller-supplied evidence label must not substitute for observed execution.
- **Estimated effort:** 0.5-1 day for honest unsupported/blocked behavior and tests; approximately 3-5 days for a bounded, specified execution integration, depending on objective mapping requirements.

### F05 - Verification timeout does not bound output finalization

- **Severity:** Medium. **Confidence:** High.
- **Locations:** `crates/app-services/src/verification_execution.rs:208-227,418-487`, especially `:452-462`; caller `bins/agentd/src/execution.rs:3772-3788,3910-3915`. A more careful existing implementation is `adapters/provider-claude/src/process.rs:298-307,475-509`.
- **Evidence:** `wait_bounded` stops its deadline loop when the direct child exits. It then uses unbounded `stdout_rx.recv()` and `stderr_rx.recv()` for `ChildStatus::Exited`. Reader threads send only after EOF. A descendant inheriting either pipe can remain alive after the direct child exits, so EOF need not arrive. The child timeout no longer applies. Only the timeout branch uses `recv_timeout`.
- **Root cause:** the code assumes that direct-child exit closes every writer of its pipes. Descendant processes invalidate that assumption. Verification also does not install the provider supervisor's process-group lifecycle.
- **Input / boundary:** an admitted verification executable or repository test/build process forks a descendant and exits. Process output crosses into a supposedly bounded trusted verification operation.
- **Realistic impact:** advisory verification, execution finalization, workspace-cycle ownership and daemon shutdown joins can remain blocked for the descendant's lifetime. The sandbox does not by itself establish EOF or a time bound. This is not a claimed filesystem/network sandbox escape.
- **Reachability qualification:** `VerificationRunner` supports injected backends and is wired into the production workspace cycle with strict confinement. Actual toolchain launch may fail closed before reaching this path on a given machine; the defect applies whenever an admitted child can leave a pipe-holding descendant. No claim of successful production toolchain execution in this environment is made.
- **Verification status:** confirmed static blocking path; no hang scenario was executed. Existing verifier tests were inspected for coverage, and the provider adapter already recognizes the same natural-exit pipe issue.
- **Safe reproduction:** in an isolated verifier integration fixture, use a short-lived wrapper that starts a bounded two-second descendant inheriting stdout/stderr and exits immediately. Set the verifier timeout below that interval. Assert that the entire invocation, including capture, respects the promised bound. Clean up fixture-owned descendants through a test-owned process group; do not launch an unbounded child.
- **Recommended solution:** apply one bounded capture deadline to natural exit as well as timeout and retain any captured prefix honestly. Reuse the provider supervisor's bounded-finalization pattern. Own/cancel the verification process tree so a timed-out root does not leave work running, and preserve typed timeout/incomplete-capture evidence.
- **Estimated effort:** 1-2 days, including natural-exit and timeout descendant tests.

### F07 - Routine completion and budget accounting are not atomic

- **Severity:** Medium. **Confidence:** High.
- **Locations:** `crates/app-services/src/background_service.rs:108-135,149-167`; `adapters/storage-sqlite/src/routine_store.rs:199-219,235-255,271-307`; `crates/background/src/model.rs:249-266`; `adapters/storage-sqlite/migrations/0035_routines.sql:1-31`.
- **Evidence:** `start_run`, `update_run(COMPLETED)`, and `update(definition)` are separate autocommitted SQL statements. No shared transaction is exposed by the current routine port. The next-run time, run count and exhausted state are written only in the last call. `reconcile` loads only `RUNNING` runs, so it cannot repair a completed run whose definition update never committed.
- **Root cause:** an invariant spanning `routine_runs` and `routines` is committed through independent calls. Per-row SQL constraints cannot enforce consistency between completion history and the budget counter.
- **Input / boundary:** a normal authorized tick/run crosses the persistence boundary; a process crash, disk error, or failed definition update occurs after completion is stored.
- **Realistic impact:** after restart, the history contains a completed run but its budget/schedule remain unchanged. A one-run routine can be admitted again. Separately, invalid evidence can return an error after `start_run` has already inserted a live `RUNNING` record. Current routines are bookkeeping-only (F16), so this does not prove real duplicated external side effects today.
- **Verification status:** confirmed transaction/control-flow gap, not a crash-injection reproduction. Existing restart tests cover stranded `RUNNING` rows, not the completed-before-counter commit window.
- **Safe reproduction:** wrap a fixture `RoutineStorePort` so the definition update fails after successful run completion. Inspect the fixture's history/counter and reconstruct the service; a completed run and still-due definition disagree. A SQLite regression should inject a failure inside the proposed combined transaction and prove rollback of both facts.
- **Recommended solution:** validate finish data before persistent start where possible, and commit terminal run status plus definition count/schedule/exhaustion in one adapter transaction through one port operation. Preserve an intentional durable in-progress record for actual execution, with explicit failure/interruption reconciliation.
- **Estimated effort:** 1-2 days across the port, SQLite adapter, contract suite and service tests.

### F08 - Widget provider work is not owned by daemon shutdown

- **Severity:** Medium. **Confidence:** High for missing ownership; runtime orphan duration unmeasured.
- **Locations:** `bins/agentd/src/widget.rs:60-79,96-128,147-158,266-377`; `bins/agentd/src/lib.rs:813-848`; contrast `bins/agentd/src/execution.rs:575-615`.
- **Evidence:** widget start drops the successfully created thread handle. The cancel flag is stored only in `Jobs.running`. `WidgetAuthoring` has no shutdown/join hook, and daemon wind-down only cancels/joins `ExecutionComposition`'s separately registered `BackgroundJob` threads. The widget's `prepare_provider_launch` does not register its flag or thread there. Frontend cancellation on component cleanup is best-effort and is not daemon shutdown ownership.
- **Root cause:** the new widget job path has a separate thread/cancellation lifetime from the existing daemon execution registry.
- **Input / boundary:** a normal `widget_draft_start` followed by daemon shutdown while its provider request is still running.
- **Realistic impact:** shutdown can return while a widget request is executing. On process exit its supervising Rust thread disappears; a spawned external process is not automatically killed by dropping `std::process::Child`. Provider work/spend may outlive the daemon, and final trusted audit/result bookkeeping can be lost. In a library-hosted daemon the detached thread can continue after the server has shut down. Existing explicit widget cancellation works only when invoked while the daemon is serving.
- **Verification status:** confirmed source ownership gap. No live provider or orphan-process test was run; sandbox restrictions prevent verifying the external-child case here.
- **Safe reproduction:** use a fixture widget runner that waits on its cancel flag and records completion in memory. Start a job and invoke daemon shutdown in an isolated harness. Assert that shutdown signals cancellation and joins the runner before returning. For a later subprocess test, use a fixture-owned bounded child, never a real paid provider.
- **Recommended solution:** give widget jobs a shutdown cancellation/join method and call it during wind-down, or register them with the existing owned background-work mechanism. Ensure cancellation is signalled before waiting, without holding the job mutex during join.
- **Estimated effort:** 1 day including a shutdown-in-flight fixture test.

### F06 - Valid task data can make project listing unreadable

- **Severity:** Medium. **Confidence:** High.
- **Locations:** `bins/agentd/src/dispatch.rs:87-123,324-345`; `crates/task/src/task.rs:120-149`; `crates/ipc-control/src/session.rs:574-588` (`respond_business`); `crates/ipc-transport/src/framing.rs:176-194`; `crates/ipc-transport/src/lib.rs:26-29`; desktop caller `apps/desktop/src-tauri/src/agentd.rs:76-85`.
- **Evidence:** task creation accepts a nonblank objective with no aggregate project-list budget. `ListTasksForProject` reads every task, includes each full objective and encodes one response. The transport refuses payloads larger than 1,048,576 bytes. The daemon ends the connection on response failure rather than providing a cursor or a recoverable bounded page.
- **Root cause:** unbounded result cardinality/aggregate bytes conflict with the deliberately bounded transport contract.
- **Input / boundary:** two individually valid tasks with 600,000-byte ASCII objectives each fit comfortably into separate creation frames; their combined list response is greater than 1 MiB even before protobuf overhead. Normal accumulation of many smaller tasks can reach the same condition.
- **Realistic impact:** the project Task list becomes unavailable to the desktop and CLI clients. This is not an IPC-cap bypass; the cap works, but ordinary stored data becomes unreadable via the listing API. Other unpaged list surfaces deserve the same follow-up review; this finding proves the Task path specifically.
- **Verification status:** confirmed static size/control-flow proof. No large records were inserted into a user database or a running daemon.
- **Safe reproduction:** create the two tasks above in a disposable daemon fixture; encode the list response with the actual protobuf type and assert its `encoded_len()` exceeds `MAX_FRAME_BYTES`. A post-fix test should retrieve every task through multiple bounded pages and retain the frame cap.
- **Recommended solution:** add protocol-versioned pagination with a byte-aware response budget and a bounded list projection. The frontend/CLI must follow continuation state. Do not fix this by raising/removing the frame limit or silently truncating results.
- **Estimated effort:** 2-3 days across protobuf negotiation, dispatch/storage queries, CLI/Tauri readers and boundary tests.

### F04 - Completed IPC tasks are never reaped during service

- **Severity:** Medium. **Confidence:** High.
- **Locations:** `bins/agentd/src/lib.rs:779-810,821-824`; `apps/desktop/src/App.tsx:18-44`; `apps/desktop/src-tauri/src/agentd.rs:181-201`.
- **Evidence:** the live serve loop spawns every admitted connection into one `JoinSet` and never polls/joins that set. `join_next` is used only after shutdown. Completed tasks release their semaphore permits, allowing more connections, but remain in the set. The desktop opens a fresh handshake connection every five seconds, so the issue occurs during normal idle use.
- **Root cause:** the active-client semaphore was mistaken for total task-retention control. Tokio removes a tracked task when its result is joined; aborting/spawning does not reap finished results. The dependency's source was inspected, and the behavior is documented in [Tokio's JoinSet API](https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html).
- **Realistic impact:** retained task records/results grow with cumulative connections until restart. One window's connection probe alone creates about 17,280 connection tasks per day; this is an arithmetic request count, **not** an RSS measurement. Task panics also remain unobserved until shutdown.
- **Verification status:** source-confirmed retention path; no daemon RSS/load benchmark was run.
- **Safe reproduction:** in an isolated test harness, admit and close sequential connections beyond the active-client bound and observe an instrumented retained-task count. After quiescence it should return near zero rather than equal cumulative completed connections. Kernel socket restrictions prevented that live harness here.
- **Recommended solution:** poll `joins.join_next()` in a guarded select branch while the set is nonempty, record unexpected task errors, and keep the existing active-client semaphore and shutdown abort/join handling.
- **Estimated effort:** 0.5 day including a sequential-connection retention regression.

### F15 - Memory supersession is not one transaction

- **Severity:** Low. **Confidence:** High.
- **Locations:** `crates/app-services/src/memory_service.rs:129-146`; `adapters/storage-sqlite/src/memory_store.rs:297-362`; `crates/memory/src/model.rs:465-486`.
- **Evidence:** the service commits the old record as superseded through `update`, then separately inserts the `SUPERSEDES` link through `link`. Each adapter call commits its own transaction. The service comment describes this as atomic, but two individually atomic commits are not one atomic operation. If the second call fails, the old record has changed and retry fails `AlreadySuperseded` before inserting the missing link.
- **Root cause:** state and its graph/provenance fact have no shared commit boundary.
- **Input / boundary:** a valid confirmed successor and a normal supersede request cross persistent memory storage; a crash or storage error occurs between calls.
- **Realistic impact:** incomplete supersession provenance and an error response after partial durable success. The successor's `supersedes` field still names the old record, so this is not claimed to delete memory content or reactivate stale context.
- **Verification status:** source-confirmed partial-commit/retry path; no fault was injected into a real store.
- **Safe reproduction:** in a fixture port wrapper, make `link` fail after `update` succeeds. Confirm that the old record is already superseded, the link is absent and retry is refused. Correct behavior must commit both or neither.
- **Recommended solution:** add one store operation/transaction for supersession state plus link, preserving domain validation and unique-link semantics.
- **Estimated effort:** 0.5-1 day with a port/SQLite rollback regression.

## Related defects and limits

[F09](01-tauri-security.md#f09---the-desktop-rejects-daemon-valid-identifiers) covers the desktop/domain ID mismatch. [F10](05-performance.md#f10---synchronous-service-work-blocks-all-daemon-connections) covers long synchronous work on the current-thread runtime, which also makes request deadlines and heartbeats unreliable under database contention.

The core Task transition path uses a shared transaction for state/journal evidence (`task_store.rs:513-578`), and database migrations run transactionally. Those controls should be preserved; F07/F15 do not imply all repository persistence is non-atomic. The per-execution OS lock added in `execution_cycle_lock.rs` gives cross-composition exclusion on the verified platform; no lock-file deletion was performed.

Not checked: crash/fault injection, long-running RSS, real provider subprocess shutdown, every concurrency interleaving, and recovery under disk exhaustion. The full-suite failures in [06-dependencies.md](06-dependencies.md#verification-command-ledger) are not newly authored regression tests for these findings.
