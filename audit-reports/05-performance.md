# Performance and resource behavior

The findings below are supported by source paths, query counts and documented timeout constants. No release-mode benchmark, heap profile, RSS curve, startup measurement or throughput measurement was performed. Severity is calibrated to a local single-user daemon, not a high-volume web service.

## Architectural/performance improvement opportunities

### F10 - Synchronous service work blocks all daemon connections

- **Severity:** Medium. **Confidence:** High for the blocking path; no measured latency claim.
- **Locations:** `bins/agentd/src/main.rs:14-15`; `bins/agentd/src/lib.rs:486-491,530-558,567-584,607-648,690-701`; `adapters/storage-sqlite/src/task_store.rs:61-75`. Existing offload pattern: `agentd/src/lib.rs:591-602` and `agentd/src/vault.rs:54-59`.
- **Evidence:** production uses Tokio's `current_thread` runtime. Business, policy, Human Decision, planning, memory, routine, worker and organization dispatch perform synchronous service/database work directly inside async tasks. SQLite is configured to wait up to 30 seconds for locks. The `await` on the async mutex does not make subsequent rusqlite calls asynchronous.
- **Root cause:** async task concurrency is being used around blocking operations without moving those operations off the only scheduler thread. Finder and vault requests already recognize this distinction and use `spawn_blocking`.
- **Trigger / boundary:** an execution/background connection holds a database write lock while another client performs a synchronous mutation or service open. A busy wait on that dispatch thread prevents the runtime from polling unrelated ping, shutdown, connection or timer tasks.
- **Realistic impact:** the UI can report a missing/unresponsive daemon, replies can exceed the five-second client deadline, and idle-lock/expiry ticks can be delayed even though their own work is unrelated. Some mutations may commit after the caller's timeout; existing no-replay handling is valuable and must remain. The 30-second number is the configured upper busy wait, not an observed stall in this audit.
- **Verification status:** statically confirmed runtime/blocking combination. Live contention/latency reproduction was not possible because local socket binding is denied.
- **Safe reproduction:** in a disposable database/daemon fixture, hold a write transaction on a separate connection and issue one business mutation. Concurrently time a ping and expiry tick. After remediation, the control plane should remain schedulable while the mutation waits, within its documented budgets.
- **Recommended solution:** offload whole synchronous service operations using the existing blocking-dispatch pattern, maintaining service lock ordering and typed errors. Include the routine/expiry paths in the review. Do not simply increase response deadlines or convert the runtime flavor and assume all blocking behavior is solved.
- **Estimated effort:** 1-2 days for the affected dispatch/timer paths and a focused contention test.

### F11 - Desktop task listing hydrates unneeded domain data

- **Severity:** Low. **Confidence:** High for query structure; slowdown not benchmarked.
- **Locations:** `adapters/storage-sqlite/src/task_store.rs:649-778,784-845`; `crates/app-services/src/task_service.rs:252-255`; `bins/agentd/src/dispatch.rs:114-122,339-345`.
- **Evidence:** one query retrieves all task IDs, then `load_task_snapshot` performs at least seven queries per task: task row, project existence, strings, dependencies, prerequisite rows, evidence parameter rows and evidence requirement rows. It restores full domain objects and then discards almost everything except ID, project ID, objective and state for the wire DTO.
- **Root cause:** a full aggregate reconstruction path doubles as a list/read-model query.
- **Trigger / realistic impact:** a project with 1,000 tasks requires at least 7,001 SQL statements before response encoding, even without evidence rows. This is a source-derived count, not a timing benchmark. Combined with F10, repeated listing work occupies the daemon control thread. With small projects it may be acceptable; there is no remote-service-scale claim.
- **Verification status:** query/call-chain inspection; no SQL trace or release profile was captured.
- **Safe reproduction:** use fixture data and rusqlite statement tracing to count queries for 1, 100 and 1,000 listed tasks. Compare returned DTOs and error semantics, not just runtime.
- **Recommended solution:** implement a bounded list projection in the existing storage/application read boundary as part of F06 pagination. Preserve stored-data validation for fields actually returned and retain full reconstruction for lifecycle/evaluation operations. Avoid adding a separate mutable cache or second source of Task truth.
- **Estimated effort:** approximately 1 day when combined with F06; measure before broader optimization.

### F12 - One slow metric blocks cached metric reads

- **Severity:** Low. **Confidence:** High for serialization; latency unmeasured.
- **Locations:** `apps/desktop/src-tauri/src/live_metrics.rs:27,195-216,231-301,304-332`; `apps/desktop/src/components/widgets/MetricCard.tsx:29-57`.
- **Evidence:** `read` acquires one app-wide `Mutex<State>` at line 238 and holds it during the HTTP await at lines 262-269. A cached/due check for a different source cannot run until that request ends. Successful requests also serialize and synchronously write/fsync the entire cache while holding the same state lock. Each request has a ten-second timeout; network requests are deliberately serial for quota control.
- **Root cause:** state access and network admission share one lock with the full I/O lifetime.
- **Trigger / realistic impact:** a cold or unreachable source is ahead of another source with a usable cached value. The cached answer waits behind the slow network request; several due sources can compound initial loading latency. Disk synchronization also blocks the async worker. Actual latency depends on the network/disk and was not measured.
- **Verification status:** source-confirmed lock scope, not a live GitHub benchmark. HTTP fixture tests could not bind in this environment.
- **Safe reproduction:** with an isolated local HTTP fixture, hold source A's response until a test signal, then request a not-due cached source B. Assert that B can return its stored value promptly while preserving one admitted outbound fetch and the global rate-limit gate.
- **Recommended solution:** keep cache/state critical sections short; preserve serialization for actual outbound requests separately, and use the platform's blocking pool for file persistence. Return cached values without waiting on another source's request. Retain rate-limit, ETag, fixed-host and response-size controls. Add retention/size limits only after choosing explicit cache policy; historical cache growth was not load-tested here.
- **Estimated effort:** 0.5-1 day for lock-scope/persistence changes and fixture coverage.

## Confirmed resource/lifecycle defects covered elsewhere

- [F04](03-correctness-and-reliability.md#f04---completed-ipc-tasks-are-never-reaped-during-service): unbounded retained completed connection tasks at ordinary five-second polling cadence.
- [F05](03-correctness-and-reliability.md#f05---verification-timeout-does-not-bound-output-finalization): unbounded verifier capture finalization after direct-child exit.
- [F06](03-correctness-and-reliability.md#f06---valid-task-data-can-make-project-listing-unreadable): aggregate reply growth exceeds the bounded transport.
- [F08](03-correctness-and-reliability.md#f08---widget-provider-work-is-not-owned-by-daemon-shutdown): detached work outlives the lifetime that should own its resource/spend limit.

## Existing bounds that should be preserved

| Resource | Control inspected | Qualification |
|---|---|---|
| Active local clients | Eight permits; bounded accept channel | Does not bound the completed JoinSet history |
| IPC payload | 1 MiB; partial-frame and write deadlines | Does not solve aggregate list/pagination |
| Provider output/lifetime | Bounded retained streams, timeout/cancel, process-group kill, natural-exit capture grace | Widget daemon-shutdown ownership remains missing; pipe-holding descendants were not stress-tested |
| Git | 30-second command deadline and bounded stream capture | All repository/OS failure paths were not exhaustively exercised |
| Finder | 1 MiB file cap, bounded hit excerpts and continuation contracts; off-thread scans | Directory-count and large-repository end-to-end memory were not measured |
| Candidate data | File/path/entry/aggregate blob limits and descriptor-relative validation | No million-entry directory or malicious allocation stress test |
| Vault | 32 MiB file read cap; bounded entries and values; Argon2 resource ceiling | High valid KDF settings can still be expensive; no password-cost benchmark |
| Dashboard/widgets | 24-widget layout limit, description/ID/repository bounds, refresh minimum, job history bound | Limits apply to admitted layouts/jobs, not every historical cache key or sequential provider spend |

The production frontend build completed successfully and emitted approximately 371 kB of JavaScript before gzip (111 kB gzip). This is Vite build output, not a claim about native app size, WebView RAM or startup time. No clone/allocation micro-optimization is recommended without a profile.

Not checked: release performance, sustained daemon RSS, very large repositories, slow/network filesystems, disk-full behavior, sustained credential derivation load, and cache-retention workloads.
