# IPC — Local Control Contract

> Architecture semantics for versioned local IPC between local clients
> (Tauri host, `agentctl`) and `agentd`. The machine-readable message shape
> is canonical in
> [`../../crates/ipc-contracts/proto/agentd_control.proto`](../../crates/ipc-contracts/proto/agentd_control.proto);
> this document explains what Protobuf alone cannot express, and records the
> accepted G1 implementation profile. Documents accepted architecture;
> implements nothing.
>
> - Binding rules: [`INVARIANTS.md`](./INVARIANTS.md) (esp. INV-026)
> - Decision: [`adr/ADR-0003-local-ipc.md`](./adr/ADR-0003-local-ipc.md) (with G1 implementation profile)
> - Privilege boundary: [`../security/TRUST_BOUNDARIES.md`](../security/TRUST_BOUNDARIES.md) (TB2)
> - IPC threat: [`../security/THREAT_MODEL.md`](../security/THREAT_MODEL.md) (THR-003)
> - Locality: [`../privacy/PRIVACY_ARCHITECTURE.md`](../privacy/PRIVACY_ARCHITECTURE.md) (§1)
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §9

## 1. Purpose and topology

```text
Tauri Host / agentctl
 |
 | local typed IPC
 v
 agentd
```

Local clients need a versioned, typed, language-independent channel to the
local daemon (Rust now, Go tooling later). IPC here is the TB2 privilege
boundary, not a trusted backplane.

## 2. Transport

```text
macOS/Linux → Unix Domain Socket
Windows     → Named Pipe
```

Canonical message serialization is Protobuf. Protobuf does NOT imply gRPC:
no `service` definition exists, none is added, and G1 introduces no
`tonic`, gRPC, or HTTP/2 — G1 uses Protobuf messages over our local framed
byte transport. Core desktop IPC never uses localhost HTTP, WebSocket,
REST, gRPC, or TCP sockets.

Both transports are normalized as a full-duplex byte stream: UDS stream on
Unix, Named Pipe Byte mode on Windows. Windows message-mode framing is
never relied upon, so both platforms share the same application-level
framing contract.

## 3. Control-plane scope (G1 bootstrap)

The G1 flow is limited to start, handshake, ping, version negotiation,
reconnect, and shutdown — nothing here expands that surface. The Protobuf
contract covers only the IPC-visible subset: handshake, ping, shutdown.
`start` is process/lifecycle behavior (detect daemon unavailable → start
`agentd` → connect → handshake), never a Protobuf message to a daemon
that is not running — no `StartDaemonRequest` exists. `reconnect` is
connection lifecycle behavior (re-establish transport, then handshake
again), never an IPC message — no `ReconnectRequest` exists, and
reconnect must not assume old in-memory connection state survived. No
Task, project, scheduler, provider, runner, memory, context, Git,
worktree, event, security-runner, browser, document, cloud, or telemetry
messages exist at this stage.

G1 protocol behavior is sequential: one in-flight request per connection
(`request → response → next request`). G1 therefore needs no `request_id`,
`response_id`, multiplexing, or out-of-order responses; the proto is not
modified to add them. Future protocol versions may add multiplexing if
justified.

## 4. Handshake semantics

Every newly connected stream must begin with `HandshakeRequest` before any
`PingRequest` or `ShutdownRequest`: new connection → handshake → accepted
→ normal G1 control requests. No successful handshake means no normal
control operations. Within one connection exactly one initial handshake is
allowed — a second `HandshakeRequest` after successful negotiation is a
protocol violation. A reconnect is a new transport plus a new handshake,
never restoration of socket-local state.

The client advertises `supported_protocol_versions`, a descriptive
`client_kind`, a diagnostic `client_build_version`, and
`requested_capabilities`; the daemon selects one compatible version or
rejects. Rejection carries a diagnostic `reason` plus
`daemon_supported_protocol_versions` so a wrong-version peer is observable
and testable.

Initial G1 versions: the existing bootstrap protocol number `0` is
preserved — no version bump merely because negotiation is now
implemented. Initial daemon supported set is `{0}`; initial clients
support `{0}`. Later versions may coexist during migrations. When multiple
versions eventually exist, the daemon selects the highest numeric protocol
version present in both sets; with no intersection it sends
`HandshakeRejected` followed by connection termination.

G1 recognized client kinds are exactly `tauri-host` and `agentctl`. An
unknown client kind yields `HandshakeRejected` for bootstrap compatibility
control. `client_kind` is descriptive metadata and MUST NOT authorize
privileged operations — a caller claiming `tauri-host` does not prove it
is Tauri, and `client_kind` is never used as an authorization credential.

G1 has no mandatory optional capability identifiers, so
`requested_capabilities` may be empty. Unknown requested capabilities are
not accepted, but do not by themselves require handshake rejection;
`accepted_capabilities` is the recognized supported subset. No closed
capability registry is invented in G1. A claimed or negotiated capability
is neither permission nor authentication, and grants no Policy authority.

## 5. Protocol version vs build version

Protocol version (wire compatibility) is distinct from daemon/client build
versions (diagnostic metadata). Two builds may remain protocol compatible;
a build-number mismatch never automatically implies IPC incompatibility,
and daemon build version never implicitly gates protocol compatibility.
For G1, build-version strings use package/application semantic version
metadata — conceptually `env!("CARGO_PKG_VERSION")` for Rust
binaries/crates where appropriate. Build-version mismatch alone never
rejects a compatible protocol.

## 6. Ping and shutdown

Ping proves basic control-channel responsiveness only: empty request and
reply, no timestamps, latency figures, or server state. Client-side G1
ping response deadline is 2 seconds. A ping timeout is connectivity
evidence only — not Task failure, database corruption, or daemon
process proof-of-death; the client may reconnect/probe per lifecycle
logic.

Shutdown is an empty request with an acknowledgement reply; no force
flag, grace period, descendant handling, or reason code exists. During
G1, a client may send `ShutdownRequest` only after successful handshake
over the secured per-user IPC endpoint. Authorization basis for G1 is
OS/user-private endpoint admission plus successful protocol handshake —
not `client_kind`. This is a bootstrap local-user control policy.

On accepted `ShutdownRequest`, `agentd` enters internal `SHUTTING_DOWN`
lifecycle state (daemon process terminology, not a Task lifecycle state —
never added to the Task schema), stops admitting new control clients,
initiates graceful control-plane shutdown, sends `ShutdownResponse`,
closes remaining G1 control connections, exits cleanly, and its endpoint
disappears/is cleaned up. Shutdown is idempotency-aware: once shutdown
has begun, no new work/control session becomes authoritative, and
concurrent clients cannot restart or cancel shutdown by racing requests —
no distributed consensus is invented; the single daemon process owns
shutdown state. There is no `ForceKillRequest`: if graceful shutdown
times out, failure is surfaced, and launcher-side process-force-termination
stays a separate OS lifecycle concern, never a generic IPC command.

Timing: the client waits 5 seconds for `ShutdownResponse`; after
acknowledgement, launcher/client allows another 5 seconds for daemon
process/endpoint disappearance. If either deadline expires, a shutdown
timeout is reported — success is never silently claimed.

## 7. Security boundary

TB2 applies in full: local machine never equals authenticated trusted
caller. `HandshakeRequest.client_kind` is descriptive metadata and MUST
NOT authorize privileged operations; future OS peer credentials,
installation authentication, and capability policy remain separate. Exact
cryptographic/per-installation peer authentication remains unresolved —
it is NOT a G1 blocker (secure per-user endpoint access suffices for the
G1 surface) and is classified PRE-G2 SECURITY REQUIREMENT before any
general Task/business mutation IPC, since a same-user arbitrary process
must not inherit full control-plane authority merely by knowing the
endpoint. TASK-0014 implements no peer authentication. The G2 peer-auth
decision is now accepted in
[`adr/ADR-0020-local-ipc-peer-authentication.md`](./adr/ADR-0020-local-ipc-peer-authentication.md):
transport → OS peer authentication (same OS-user principal) →
handshake → READY, with the Windows first-frame staging exception
(one bounded frame read as opaque bytes before token authentication;
no protocol authority until authentication succeeds).

Even after handshake, a client request is not automatic authorization:
future business/privileged operations still require relevant
Policy/application authorization. Handshake establishes protocol, not
universal permission. Control messages carry no secrets, and
implementation must safely reject malformed Protobuf,
unknown/unsupported protocols, invalid handshake ordering, oversized
messages, and invalid semantic values. Unknown-field tolerance alone
never proves compatibility — negotiated protocol version governs.
Application messages stay independent of socket/pipe APIs, Tauri/Tokio
types, and file descriptors, and remain usable from Rust and future Go
tooling without Rust-specific representations. No Task/provider/domain
structs enter the bootstrap proto: IPC DTOs are boundary representations,
never the domain model.

Implementation must expose distinguishable local failures for at least:
daemon missing, wrong protocol version, permission denied, endpoint
security invalid, stale endpoint, already running, connect timeout,
handshake timeout, malformed frame, oversized frame, malformed protobuf,
shutdown timeout. No giant Protobuf error enum is added — most of these
are local transport/application errors, not new wire messages.
Security-relevant failures (insecure endpoint permissions, foreign-owned
socket, unexpected symlink, pipe security setup failure) must fail
closed: never degrade to a less secure endpoint, localhost HTTP, or a
world-accessible pipe.

## 8. Endpoints, singleton, and races

Linux preferred endpoint parent is `$XDG_RUNTIME_DIR/ai-engineering-os/`,
only when valid for the current user; otherwise
`${TMPDIR:-/tmp}/ai-engineering-os-$UID/`. macOS uses the current user's
temporary runtime location `$TMPDIR/ai-engineering-os/` (platform
`temp_dir()` equivalent as resolution fallback). The application-owned
child directory must be owned by the current effective UID, must not be a
symlink, and must have mode `0700` — violations fail closed, never
reused. Inside the secure runtime directory the socket node is
`agentd.sock` (`<private-runtime-dir>/ai-engineering-os/agentd.sock`),
restricted to `0600` where Unix socket permissions apply. Before
using/removing an existing endpoint, implementation verifies parent
directory owner and permissions, absence of endpoint symlink
substitution, and socket file type and owner; unexpected symlinks,
regular files, foreign-owned sockets, or insecure directories produce
visible failure — arbitrary filesystem objects are never unlinked. The
endpoint builder preflights platform UDS path length: a too-long path is
an explicit endpoint configuration error, never a localhost TCP
fallback, and path components are never silently truncated.

Windows canonical pipe name is conceptually
`\\.\pipe\ai-engineering-os-agentd-<CURRENT_USER_SID>`, using the
normalized current Windows user SID string — never username, email, or
display name, and the SID is not hashed for aesthetics. No default Named
Pipe security descriptor: an explicit DACL grants full required pipe
access only to the current user SID and LOCAL SYSTEM — never Everyone or
Anonymous — and remote clients are rejected. `PipeMode::Byte` is used so
the same explicit length framing applies on every platform, with
`reject_remote_clients(true)` set explicitly even where a library
defaults that way. Max pipe instances is 8, matching
`MAX_CONTROL_CLIENTS = 8`; the accept loop keeps a listening pipe
instance available while connected clients are handed off, never relying
on one permanent single-connected instance. The first listening pipe
instance uses the platform first-instance mechanism
(`first_pipe_instance(true)`): a second `agentd` attempting to own the
same user pipe fails `AlreadyRunning` instead of creating an alternate
endpoint. On Unix the canonical UDS endpoint is the singleton authority:
endpoint absent → attempt bind → bind winner becomes daemon; a bind loss
because the endpoint became owned means treating it as an existing or
racing daemon. No PID file serves as the sole singleton mechanism.

Stale Unix socket cleanup never blindly removes: `lstat` the existing
node; verify socket file type, current-user ownership, and secure parent
directory; probe the endpoint with a bounded connection attempt; a
compatible/live daemon response means `AlreadyRunning`; only a proven
stale/unbound endpoint is removed after re-checking filesystem identity
pre-change, then the new socket binds — any ownership/type/identity
change fails closed.

Client startup flow connects first; if the daemon is missing it attempts
to spawn `agentd`. Concurrent spawns may race — acceptable, because
endpoint ownership decides the winner: daemon A binds, daemon B loses
the singleton race and exits `AlreadyRunning`, and both clients retry
against daemon A. No unreliable shared PID file.

## 9. Transport profile, framing, and timing

G1 local IPC implementation uses Tokio (`tokio ~1.51.4` LTS family, held
through G1 absent a security/correctness reason for an explicit update).
Protobuf codegen uses the `prost = 0.14.4` / `prost-build = 0.14.4`
family with build-time generation, pinned via `protoc-bin-vendored =
3.2.0` so normal Cargo builds need no separately installed protoc and
never depend on whichever global `protoc` exists. Pinning holds until an
explicit dependency-update task changes it; Cargo.lock stays part of
reproducible builds. Exact Cargo changes belong to TASK-0017/0018, not
here. Canonical IPC source remains
`crates/ipc-contracts/proto/agentd_control.proto`; `prost-build`
generates into Cargo `OUT_DIR` and generated Rust is never committed and
never becomes a second manually edited contract (the crate may later
expose generated types through a stable Rust module — TASK-0017's work,
not this document's).

Every IPC frame is a 4-byte big-endian `payload_length: u32` (payload
bytes only, prefix excluded) followed by exactly one encoded
`ControlRequest` or `ControlResponse`. Maximum frame size is
`MAX_FRAME_BYTES = 1 MiB = 1_048_576 bytes`, deliberately below larger
library defaults: G1 control messages are tiny, hostile local clients
must not force huge allocations, and future large artifacts must never
travel as giant control messages (raising the bound later requires
explicit protocol evolution). Oversized declarations are invalid.
Zero-length application frames are invalid — a valid envelope always
carries an actual Protobuf message; a zero-length frame is a protocol
violation and the connection is closed without resynchronization
attempts. Framing implementation should use `tokio-util
LengthDelimitedCodec` or a behaviorally equivalent minimal codec; the
accepted contract is the wire format, not the library type — configure
u32, big-endian, 1 MiB max explicitly, never relying on undocumented
defaults. On oversized frames, invalid lengths, truncated frames,
malformed Protobuf, or unexpected message ordering, implementation
rejects the connection with no byte-stream resynchronization: the
connection is local and cheap to reconnect, and resynchronization only
grows parser complexity and attack surface.

G1 supports multiple simultaneous local control clients (e.g. Tauri host,
`agentctl`) up to `MAX_CONTROL_CLIENTS = 8`; a ninth concurrent client
may be rejected/closed. Each connection carries independent handshake
state and one request in flight, with no authority over another
connection's protocol state and no cross-client session state.

Budgets and timeouts: bounded total startup/reconnect budget of 10
seconds, with retry delays 50 ms, 100 ms, 200 ms, 400 ms, 800 ms, 1000
ms, 1000 ms, … until the budget expires (no random jitter required for
G1 local IPC); expiry yields an explicit connection/start failure.
Complete frame read and write timeouts are 5 seconds each — an
incomplete frame must never be held forever, while healthy idle
connections carry no timeout (G1 has no automatic idle disconnect;
liveness uses explicit `PingRequest` when needed). After transport
accept, `HandshakeRequest` must arrive within 5 seconds and the client
waits at most 5 seconds for `HandshakeResponse`; failure closes the
connection.

## 10. No network fallback

Mandatory: if UDS/Named Pipe setup fails, core desktop IPC must NOT
silently become `127.0.0.1` HTTP. Endpoint permission failures stay
explicit, as do stale-socket conditions. Connection loss never implies
Task state loss: connection-local state may disappear, but durable state
belongs to `agentd` persistence/domain layers — G1 has no Task
persistence yet, and the boundary is preserved regardless. The contract
supports crash recovery by making reconnection plus fresh handshake
possible; crash recovery itself is not implemented here.

## 11. G1 failure scenarios and contract tests

The six canonical scenarios keep their expected behavior: daemon missing
→ connect-first, bounded spawn attempt, explicit failure on budget
expiry; wrong protocol version → `HandshakeRejected` with daemon
versions, then termination; daemon crash → connection loss, fresh
reconnect with new handshake, no assumed state; stale socket → verified
stale-endpoint cleanup (§8) before bind; permission error → explicit
failure, fail closed, no fallback; reconnect → new transport plus new
handshake. None are implemented here. Later tests additionally cover:
oversized frame rejected; zero-length frame rejected; malformed protobuf
closes connection; non-handshake first message rejected; second
handshake rejected; unknown `client_kind` rejected; unsupported
capability omitted from the accepted set; second daemon loses the
singleton race; shutdown timeout visible; more than 8 concurrent clients
bounded.

## 12. Explicitly outside G1

Task/project/provider/runner IPC, event or log streaming, terminal/PTY
protocol, file/artifact transfer, browser/cloud/remote-worker/plugin
protocols, and authentication/authorization implementation are all out of
scope for this skeleton.

## 13. Accepted G1 implementation profile vs remaining open items

Sections 2–11 above are the accepted G1 implementation profile: Tokio
LTS direction, prost/prost-build family with vendored pinned protoc,
OUT_DIR-only codegen, no gRPC, byte-stream normalization, u32
big-endian framing, 1 MiB cap, zero-length invalid, sequential
one-in-flight model without correlation IDs, 8-client bound, handshake
ordering and single-handshake rule, protocol 0 with highest-common
selection, CARGO_PKG_VERSION-concept build versions, two-kind client
registry with `client_kind` never authentication, empty-allowed
capabilities without closed registry, Unix endpoint strategy and 0700 /
0600 enforcement, Windows SID naming with explicit user+SYSTEM DACL and
remote rejection, byte mode, 8-instance accept loop, first-instance
singleton on both platforms, verified stale-socket cleanup, connect-first
start races, 10-second reconnect budget with staged backoff, 5-second
frame/handshake/shutdown timings, 2-second ping deadline, no idle
disconnect, G1 shutdown authorization basis with idempotent semantics
and no force-kill command, PRE-G2 peer-auth classification, and the
distinguishable-failure set. These items are therefore no longer listed
as unresolved. The G2 OS-user peer-authentication decision
([`adr/ADR-0020-local-ipc-peer-authentication.md`](./adr/ADR-0020-local-ipc-peer-authentication.md):
transport → peer authentication → handshake → READY, Windows
first-frame staging) is likewise accepted; only its implementation
(TASK-0026) is pending before mutable business IPC.

Still open: cryptographic/per-installation IPC peer authentication
implementation (future remote/device identity, explicitly not the G2
local mechanism); future protocol deprecation window; future protocol
version coexistence duration; future request multiplexing; future
business IPC; future event streaming; future Task commands; future
remote worker transport.

## 14. Relation to existing bootstrap code

`crates/ipc-contracts/src/lib.rs` holds bootstrap-only protocol-version
primitives (`IPC_PROTOCOL_VERSION = 0`, exact-match helper), explicitly
deferring negotiation policy to a future IPC task. That code is left
unchanged: it does not contradict this message-shape contract, and G1 may
replace it. Any genuine future contradiction must escalate as
`ARCHITECTURE_BLOCKER`, never as opportunistic G1 implementation.

## 15. G2 protocol v1: versioned business IPC (contract only)

Accepted in
[`adr/ADR-0021-g2-business-ipc-and-local-data-composition.md`](./adr/ADR-0021-g2-business-ipc-and-local-data-composition.md);
contract definition only — no business dispatch, no database opening,
and no `agentctl` Project/Task commands are implemented here.

Protocol `0` (handshake, ping, shutdown) stays supported; protocol `1`
adds the bounded `task_core` envelope
(`crates/ipc-contracts/proto/task_core.proto`, carried as
`ControlRequest.task_core = 4` / `ControlResponse.task_core = 4`; G1
field numbers 1–3 are frozen). Highest-common negotiation is
unchanged: old `[0]` clients negotiate `0` and keep the full G1
surface, new `[0,1]` clients negotiate `1` against a `[0,1]` daemon.
Build versions stay diagnostic metadata and never gate compatibility.

A version-0 session is bootstrap-only: a business frame on it is
rejected as unsupported for the negotiated protocol and never
executes — successful Protobuf decoding is never authority. Business
authority additionally requires OS peer authentication (ADR-0020) plus
a successful handshake; `client_kind` stays descriptive metadata and
never authorizes anything. The sequential one-in-flight model is
preserved: no request IDs, multiplexing, streaming, gRPC, or HTTP.

The v1 surface is exactly: `ProjectDto { id, name }` with
Create/Get/List; `TaskDto { id, project_id, objective, state }` with
Create/Get/List-for-project; ID-only `AddDependency` /
`ListDependencies` (removal deferred); nine semantic lifecycle ops
mapping 1:1 onto the trusted `TaskService` methods with no generic
set-state and no complete-task request (`submit_result !=
verification_succeeded` on the wire); bounded human `reason` text
(`MAX_REASON_LEN = 4096`); and the typed business-error codes
`INVALID_ARGUMENT / NOT_FOUND / ALREADY_EXISTS / INVALID_TRANSITION /
CONFLICT / STALE_STATE / CORRUPT_STATE / UNSUPPORTED_PROTOCOL /
INTERNAL` with sanitized detail.

Composition stays
`agentctl → IPC → agentd → app-services → storage-sqlite`: the durable
database is owned by the `agentd` composition root at the canonical
`ai-engineering-os/state.sqlite3` application-data location (per-OS
paths in ADR-0021 §15; the IPC runtime directory is explicitly
rejected), and `agentctl` never opens SQLite. TASK-0028 implements
this contract atomically — including the three new journal types
(`project.created`, `task.created`, `task.dependency_added`) — before
any CLI command ships, and preserves old-G1-client compatibility.
