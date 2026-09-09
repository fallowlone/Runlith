# ADR-0021 - G2 Business IPC Is Versioned and Composed in agentd

Status: Accepted
Date: 2026-09-09

## Context

G2 sequences durable Task work before any provider, scheduler, or UI
complexity:

```text
TASK-0022 → Task domain (done)
TASK-0023 → SQLite persistence (done)
TASK-0024 → atomic mutation + Event Journal (done)
TASK-0025 → peer-auth ADR (done)
TASK-0026 → peer-auth implementation (done)
TASK-0027 → this decision: business IPC contract + local data composition
TASK-0028 → implementation (agentd dispatch, agentctl commands)
```

G1 left a versioned but bootstrap-only channel: protocol `0` with
handshake, ping, and shutdown over a per-user endpoint, plus OS-user
peer authentication (ADR-0020) that runs before any handshake is
interpreted. `agentctl` must become the CLI path to the Task core
(ADR-0016), but no versioned business envelope, no Project/Task DTO
surface, no lifecycle command mapping, no business error taxonomy, and
no canonical database location exist yet. Exposing raw storage calls
over IPC would violate INV-004 (provider/client layers never mutate
domain state directly), INV-010 (persistent mutations are auditable),
INV-025/INV-026 (no cross-context storage reads; infrastructure only
through explicit ports), and the completion-authority split (INV-009,
ADR-0018).

This ADR materializes the exact contract for:

```text
authenticated connection
→ protocol handshake
→ G2 business request
→ agentd
→ application service
→ SQLite + Event Journal
```

It decides the contract only. It implements no dispatch, opens no
database, and adds no CLI commands — TASK-0028 implements this contract
atomically before any Project/Task command ships.

## Decision

### 1. Protocol evolution: version 0 stays, version 1 is G2 business

G1 protocol version `0` remains supported indefinitely. G2 introduces
protocol version `1` for business IPC:

```rust
IPC_PROTOCOL_VERSION_V0 = 0      // handshake, ping, shutdown only
IPC_PROTOCOL_VERSION_V1 = 1      // G2 business surface added
IPC_PROTOCOL_VERSION_CURRENT = 1 // highest version this build speaks
SUPPORTED_PROTOCOL_VERSIONS = [0, 1]
```

Negotiation keeps the accepted highest-common rule, so:

```text
old G1 client [0] × new daemon [0,1] → 0
  → handshake / ping / shutdown keep working unchanged

new client [0,1] × new daemon [0,1] → 1
  → business messages enabled
```

Version `0` is never removed by this decision; its removal (if ever)
belongs to a future versioned deprecation with its own ADR.

### 2. Version semantics: v0 is bootstrap-only

Version 0 allows only:

```text
Handshake
Ping
Shutdown
```

Version 1 allows:

```text
Handshake
Ping
Shutdown
TaskCoreRequest
```

A business request on a version-0 negotiated session fails as
unsupported for the negotiated protocol
(`ControlError::UnsupportedProtocol` / wire
`BusinessErrorCode::UNSUPPORTED_PROTOCOL`). It never executes because
Protobuf happened to decode it: unknown-field tolerance and successful
decoding are never authority — only the negotiated version governs
(`ipc-control::validate_business_allowed`; session-tested for both v0
rejection and the v1 gate).

### 3. Protocol version != build version

Preserved without change: protocol version governs wire compatibility;
`client_build_version` / `daemon_build_version` stay diagnostic
metadata. Build-version mismatch alone never rejects a compatible
protocol (tested on both the v0 and v1 profiles).

### 4. Proto organization: bounded `task_core.proto`

The business schema lives in a new bounded contract file:

```text
crates/ipc-contracts/proto/task_core.proto
```

imported by the existing control envelope, which gains exactly one
field per direction and nothing else:

```text
ControlRequest
├ HandshakeRequest  = 1  (frozen G1 history)
├ PingRequest       = 2  (frozen G1 history)
├ ShutdownRequest   = 3  (frozen G1 history)
└ TaskCoreRequest   = 4  (v1 only)

ControlResponse
├ HandshakeResponse = 1  (frozen)
├ PingResponse      = 2  (frozen)
├ ShutdownResponse  = 3  (frozen)
└ TaskCoreResponse  = 4  (v1 only)
```

G1 field numbers 1–3 are frozen and covered by byte-level contract
tests; `agentd_control.proto` never becomes a giant business schema —
later domains get their own contract files behind their own envelope
fields and protocol versions.

### 5. Project DTO: identity plus name only

```text
ProjectDto { id, name }
```

Requests: `CreateProject`, `GetProject`, `ListProjects`. The contract
carries current G2 Project semantics only — no organization, team,
cloud account, billing, or owner concepts.

### 6. Task DTO: G2 domain fields only

```text
TaskDto { id, project_id, objective, state }
```

`objective` is the TASK-0022 durable intent; `state` is the canonical
lifecycle name (`DRAFT`, `READY`, … exactly as accepted). Extended
domain metadata (parent, acceptance, scope paths, verification/review
references, risk, artifacts, kind) is storage-capable but NOT on the v1
wire surface; no provider/model/session/runtime state appears anywhere
on it.

Requests: `CreateTask`, `GetTask`, `ListTasksForProject`. No Execution,
Attempt, AgentSession, Scheduler, Provider, or Worktree concepts exist
in this contract.

### 7. Dependencies: IDs in, server-side validation

Requests: `AddDependency { task_id, depends_on_id }` and
`ListDependencies { task_id }`. Dependency removal is explicitly OUT of
the v1 surface even though `storage-sqlite::remove_dependency` exists:
removal has no journal coverage today (section 13) and no G2
requirement forces it now; adding it later requires its own
`task.dependency_removed` event type plus journaled implementation.

Dependency requests carry Task IDs only. All edge validation —
self-edge, duplicate, cross-project, cycle — stays in the
domain/application service. The client can never assert an edge is
valid.

### 8. Lifecycle IPC: semantic operations, 1:1 with TaskService

No `set_state(target_state)`. No generic `transition(from, to)`. The
wire carries one semantic enum whose variants map 1:1 onto the trusted
`TaskService` methods accepted today:

| Wire op | TaskService method | Edge |
|---|---|---|
| `MARK_READY` | `mark_ready` | `DRAFT → READY` |
| `CLAIM` | `claim` | `READY → CLAIMED` |
| `START` | `start` | `CLAIMED → RUNNING`, plus accepted resumes `BLOCKED → RUNNING` and `RETRY_READY → RUNNING` |
| `BLOCK` | `block` | `RUNNING → BLOCKED` |
| `SUBMIT_RESULT` | `submit_result` | `RUNNING → SUBMITTED` |
| `ROUTE_TO_REVIEW` | `route_to_review` | `SUBMITTED → AWAITING_REVIEW` |
| `BEGIN_VERIFICATION` | `begin_verification` | `AWAITING_REVIEW → VERIFYING` |
| `VERIFICATION_FAILED` | `verification_failed` | `VERIFYING → RETRY_READY` |
| `VERIFICATION_SUCCEEDED` | `verification_succeeded` | `VERIFYING → COMPLETED` |

There are no separate `RESUME` / `RETRY` / `BEGIN_REVIEW` wire ops:
`START` covers both accepted resume edges and `ROUTE_TO_REVIEW` is the
review-routing op. There are no `CANCEL` / `FAIL` ops: those edges are
unresolved and stay server-rejected. If the accepted service surface
grows, the wire grows through the same 1:1 rule — never through a
generic setter.

### 9. Completion authority stays split on the wire

`SUBMIT_RESULT` and `VERIFICATION_SUCCEEDED` are distinct enum values
with distinct edges, tested never to alias. No `COMPLETE_TASK` /
`SET_COMPLETED` request exists. `VERIFICATION_SUCCEEDED` is the
trusted/manual G2 verification operation; it is not agent
self-completion, and future G3/G8 evaluator routing owns automated
verification (ADR-0018 unchanged).

### 10. Mutation reason: bounded human text in, provenance never

Where the service accepts a bounded reason (block reason, submission
summary, verdict note), `LifecycleRequest.reason` may carry it:
bounded to `MAX_REASON_LEN = 4096` bytes, non-blank where the operation
requires it, ignored otherwise. The client never supplies authoritative
`event_id`, `event_type`, timestamp, correlation/causation ids,
provenance identity, or from/to state — the server/application layer
mints all event construction in TASK-0028.

### 11. Business errors: small, typed, sanitized

```text
INVALID_ARGUMENT
NOT_FOUND
ALREADY_EXISTS
INVALID_TRANSITION
CONFLICT
STALE_STATE
CORRUPT_STATE
UNSUPPORTED_PROTOCOL
INTERNAL
```

Justified by current errors, and the only mapping TASK-0028 may use:

| Current error | Wire code |
|---|---|
| `DomainError::InvalidInput`, relational constraint rejection | `INVALID_ARGUMENT` |
| `StorageError::NotFound` | `NOT_FOUND` |
| `StorageError::Duplicate` | `ALREADY_EXISTS` |
| `DomainError::InvalidTransition` | `INVALID_TRANSITION` |
| `DomainError::{SelfDependency, DuplicateDependency, DependencyCycle, ProjectMismatch}`, `StorageError::InvalidGraph` | `CONFLICT` |
| `StorageError::Stale` | `STALE_STATE` |
| `StorageError::{Corrupt, DomainReconstruction, UnsupportedSchemaVersion}` | `CORRUPT_STATE` |
| v0-session business attempt | `UNSUPPORTED_PROTOCOL` |
| `StorageError::{Sqlite, Migration}` and anything unclassified | `INTERNAL` |

Human-readable detail is bounded and sanitized: never raw SQLite
errors, filesystem paths, SQL strings, Rust debug dumps, or backtraces.

### 12. Sequential model preserved

G1's one-request-in-flight-per-connection model is unchanged. No
request IDs, no multiplexing, no streaming RPC, no gRPC, no HTTP. One
business request receives one corresponding business response.

### 13. Authentication boundary: three conditions, all required

Business authority requires all three:

```text
OS peer authentication (ADR-0020)
+ successful handshake
+ negotiated protocol version 1
```

An authenticated peer that negotiated version 0 may use G1 controls
only. `client_kind` remains descriptive metadata: a valid `agentctl`
kind without OS peer authentication never reaches business IPC, and no
business request is authorized merely because `client_kind = agentctl`.

### 14. Local database composition: agentd owns, agentctl never opens

```text
agentctl → IPC → agentd → app-services → storage-sqlite
```

The durable G2 database is owned and opened by the `agentd`
composition root only. `agentctl` never opens SQLite directly — not the
production database, not a test copy. This preserves the dependency
direction (domain crates never instantiate infrastructure; consumers
own their ports).

### 15. Canonical application-data location

Stable internal directory `ai-engineering-os`, independent of any
marketing brand. Canonical filename `state.sqlite3`:

```text
macOS:   ~/Library/Application Support/ai-engineering-os/state.sqlite3
Linux:   ${XDG_DATA_HOME}/ai-engineering-os/state.sqlite3
         fallback ~/.local/share/ai-engineering-os/state.sqlite3
Windows: %LOCALAPPDATA%\ai-engineering-os\state.sqlite3
```

Resolved with native platform APIs/environment resolution in Rust.
Explicitly rejected for durable data: the IPC runtime directory
(socket/pipe parent), `/tmp`, `$XDG_RUNTIME_DIR`, the current working
directory, any repository directory, and beside-the-executable paths.

### 16. Test path injection without env-var authority

The production default comes from the OS application-data resolver
(section 15). Tests use an explicit injected test-owned database path
through a private/config seam such as `AgentdConfig { database_path }`
(defined in TASK-0028). No public environment variable is the sole
production authority for the database location. This task defines the
contract only.

### 17. Data-directory security

On Unix the application-data parent created by the application is
application-owned/current-user-only, mode `0700`. An existing path is
never blindly replaced or destructively cleaned: symlink,
foreign-owned, or unexpected objects fail closed for operator
attention. The database is durable user application data — unlike the
recoverable runtime socket, it is never deleted as "stale cleanup". On
Windows the current-user LocalAppData boundary plus normal file ACL
inheritance applies; no custom cryptographic DB credential is invented
in G2.

### 18. Local-first boundary

The database stays local. No account, no sync, no telemetry
requirement. No source/project data leaves the device because of this
contract (ADR-0014 unchanged).

### 19. Auditability review (INV-010): classification

Inspected: INV-010, `docs/architecture/events.md`, the canonical event
envelope schema, and the current `EventType` implementation (9
lifecycle types: `task.marked_ready` … `task.verification_failed`).

| Persistent mutation | Journal coverage today | Disposition |
|---|---|---|
| Project created | NONE — `TaskService::create_project` appends no event | GAP → new canonical type `project.created` (section 20) |
| Task created | NONE — `TaskService::create_task` appends no event | GAP → new canonical type `task.created` (section 20) |
| Dependency added | NONE — `SqliteStore::add_dependency` writes the edge with no event | GAP → new canonical type `task.dependency_added` (section 20) |
| Dependency removed | NONE — and NOT exposed on the v1 wire (section 7) | Deferred; a future removal API requires `task.dependency_removed` first |
| Task lifecycle transition | FULL — 9 committed transitions each carry their typed event | No gap; contract tests pin the mapping |

`task_strings` is confirmed bounded Task persistence machinery (list
metadata slots: acceptance, allowed/forbidden paths, verification,
required review, artifacts). It is not a business IPC DTO surface and
is not exposed on the wire.

### 20. Event-contract gap: three typed events, no generic fallback

The gap in section 19 is resolved by three exact canonical event
types, defined here and in `docs/architecture/events.md` plus the
machine-readable envelope schema:

```text
project.created
task.created
task.dependency_added
```

No generic `EntityChanged` / `MutationOccurred` is introduced: typed
events stay the rule. Their SQLite/application handling is explicitly
NOT implemented in this task — TASK-0028 implements all three
atomically (state mutation + journal evidence in one transaction, per
ADR-0006) before exposing the corresponding CLI commands. Known
encoding note for TASK-0028: `project.created` is project-scoped with
no task subject, while the current journal table requires `task_id`;
its migration must permit project-scoped rows (exact encoding is
TASK-0028 implementation detail inside the atomicity invariant).
Dependency edges never mutate lifecycle state: `task.dependency_added`
records `from_state == to_state ==` current state.

This changes no invariant and contradicts no ADR: it fulfills INV-010
through the staged path ADR-0006 already requires.

### 21. Event provenance and audit separation

Client requests never mint authoritative event provenance. In TASK-0028
the authenticated local principal plus the trusted `agentd`
application service establishes request provenance;
`client_kind` stays descriptive metadata. Peer-auth success/failure is
never an Event Journal event, and business mutation events never enter
the Security Audit — journal and audit stay separate categories
(INV-073). Security Audit itself remains G6.

### 22. Backward compatibility

Old G1 clients keep the full G1 surface against the future v1 daemon:
connect, handshake v0, ping, shutdown. TASK-0028 must preserve this;
its test matrix keeps a `[0]`-only client green against the `[0,1]`
daemon.

### 23. What this task does NOT do

No `agentd` business dispatch, no `TaskService` calls from IPC, no
SQLite opening in `agentd`, no `agentctl` Project/Task commands, no
scheduler, no FakeAgent, no provider logic, no Policy Engine, no
Security Audit, no cloud/account/sync. The v1 session gate passes the
wire shape and stops with `BusinessNotImplemented` until TASK-0028.

## Alternatives considered

- Unversioned business messages on protocol 0: rejected; old clients
  and old daemons could not distinguish the new surface, and v0's
  bootstrap-only promise would silently break.
- Generic `set_state` / `transition(from, to)` wire op: rejected; it
  would let any client assert any edge, bypassing the semantic
  1:1-with-service rule and the completion-authority split.
- Separate business socket/pipe: rejected; one authenticated endpoint
  with version-gated authority is simpler and keeps the ADR-0020
  boundary single.
- Environment-variable database location: rejected as sole production
  authority; explicit resolver default plus an injected test seam keeps
  production stable and tests hermetic.
- Generic `EntityChanged` event: rejected; typed events preserve the
  staged Event Sourcing migration (ADR-0006).
- Exposing dependency removal now: rejected; no journal coverage and
  no G2 requirement — it would ship the first unaudited mutable IPC.

## Consequences

- TASK-0028 implements exactly this contract: journaled creation and
  dependency mutations (section 20), version-gated dispatch, typed
  error mapping (section 11), `agentd`-owned database composition
  (sections 14–17), and `agentctl` Project/Task commands — then the G2
  closure audit verifies it.
- `task_core.proto` is the bounded pattern for future domains: new
  bounded contract files, never growth of the control envelope beyond
  one field per direction per version.
- The three new event types require a TASK-0028 storage migration and
  `EventType` variants; until then they exist as contract only, which
  is why no CLI command ships in this task.

## Security impact

Defines the authenticated/versioned boundary for all future G2
business IPC without exposing any mutable business IPC itself: OS peer
authentication plus handshake plus v1 negotiation, `client_kind`
explicitly non-authoritative, no secrets on the wire, sanitized error
detail, fail-closed data-directory handling, and no unaudited mutation
path (`agentctl → agentd → raw storage` stays forbidden — every future
mutation travels through the trusted service with journal evidence).

## Migration / replacement impact

Additive wire evolution: v0 peers are unaffected, v1 is opt-in through
negotiation. The event taxonomy grows by three typed names with no
change to the envelope or to existing persisted rows; TASK-0028's
migration extends the journal's accepted type set. Future protocol
versions coexist the same way; v0 removal needs its own ADR.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §§9–14, 20–22
- `INVARIANTS.md`: INV-001, INV-002, INV-004, INV-009, INV-010,
  INV-025, INV-026, INV-044, INV-070, INV-073
- `docs/architecture/ipc.md`, `docs/architecture/task-engine.md` (§5–§7,
  §13), `docs/architecture/events.md`
- ADR-0003 (local IPC), ADR-0004 (Task entity), ADR-0005
  (Task != Execution != Attempt != Session), ADR-0006 (state + journal),
  ADR-0016 (`agentctl` first), ADR-0018 (independent evaluation),
  ADR-0019 (gate-scoped activation), ADR-0020 (peer authentication)
- `contracts/json-schema/task.schema.json`,
  `contracts/json-schema/event.schema.json`
- Code authority: `crates/task` (domain ops), `crates/app-services`
  (`TaskService` methods), `adapters/storage-sqlite` (`EventType`,
  journal atomicity), `crates/ipc-contracts/proto/task_core.proto`,
  `crates/ipc-control` (version gate)
