# ADR-0020 - Local IPC Uses OS-Authenticated User Identity

Status: Accepted
Date: 2026-09-09

## Context

G1 established a secure per-user endpoint (0700 runtime directory,
current-user ownership, no symlink traversal, 0600 socket, explicit
user+SYSTEM pipe DACL) plus a versioned handshake carrying a descriptive
`client_kind` (`tauri-host` / `agentctl`). But `client_kind` is
self-asserted protocol metadata, not authentication: any local process
that can reach the endpoint can claim any kind. G2 is about to expose
mutable Task/Project operations through `agentctl` (TASK-0027), so the
local peer principal and its authentication mechanism must be explicit
first. This ADR activates the peer-auth decision deferred since G1
(TB2 open item, THR-003 open risk); it decides, it does not implement
(TASK-0026 implements).

## Decision

For v1 local desktop IPC, the authenticated principal is the
operating-system user identity running `agentd`. A client is accepted
only when the OS reports that the connected peer belongs to the same
local user principal:

```text
Linux/macOS → effective UID
Windows     → user SID
```

This is a local-account security boundary, applied per accepted
transport connection, before protocol acceptance:

```text
transport connection
 ↓
OS peer authentication
 ↓
protocol handshake
 ↓
READY
```

The frame a Windows server must physically read before token
authentication stays opaque and gains no protocol authority until
authentication succeeds (section 8).

### 1. What this proves — and what it does not

The security claim is exactly:

```text
a process running as another OS user may not issue authenticated
local control/business requests
```

It is explicitly NOT claimed that this proves the genuine `agentctl`
or Tauri binary, an un-injected process, or a malware-free peer. Any
arbitrary malicious code already executing with the same user's
authority can generally reach that user's repositories and application
data; that is outside this IPC-auth boundary. Same-user peer
authentication must never be oversold.

### 2. `client_kind` remains descriptive

```text
client_kind != authentication
client_kind != authorization
```

`tauri-host` / `agentctl` stay as protocol metadata and capability
description only. `client_kind == "agentctl"` therefore never implies
trust.

### 3. Unix mechanisms

- Linux: kernel-provided credentials on the connected Unix Domain
  Socket — `SO_PEERCRED`, or the safe Rust/Tokio equivalent exposing
  the same kernel-authenticated credentials. Authenticate peer
  effective/user UID == `agentd` effective UID.
- macOS: `getpeereid()`, or a safe standard/Tokio abstraction providing
  equivalent kernel peer credentials. Authenticate peer effective UID
  == `agentd` effective UID.

Never authenticate on PID alone, `/proc` executable path, process
name, argv, environment variables, or any filesystem claim supplied by
the client. PID may be retained only as optional diagnostic metadata.

Identity is never derived from client-provided protocol data.

### 4. Existing Unix filesystem security remains

Peer authentication supplements, never replaces, the G1 endpoint
profile: 0700 runtime directory, current-user ownership, no symlink
traversal, 0600 socket, verified stale cleanup. Both layers stay as
defense in depth; endpoint permissions are not relaxed because peer
credentials now exist.

### 5. Windows mechanism

The connected client's OS access token decides. Required sequence:

```text
Named Pipe client
 ↓
server reads first bounded transport frame as untrusted bytes
 ↓
ImpersonateNamedPipeClient
 ↓
OpenThreadToken
 ↓
GetTokenInformation(TokenUser)
 ↓
client SID
 ↓
compare with agentd process-user SID
 ↓
RevertToSelf
```

The exact Win32 helper sequence may differ only if an equally strong
kernel/token-based mechanism is used. Authoritative property:
connected client's user SID == `agentd` user's SID. `GetNamedPipeClientProcessId`
and peer PID generally may serve diagnostics/testing only — never the
primary authentication authority.

### 6. Windows first-frame staging

Pipe impersonation attaches to data already read, so the server reads
exactly one bounded frame first under a strict staged boundary:

```text
ACCEPTED TRANSPORT
 ↓
read exactly one bounded frame
 ↓
frame remains OPAQUE / UNTRUSTED
 ↓
authenticate OS peer
 ↓
if authenticated: decode/interpret as HandshakeRequest
else: close
```

Before successful peer authentication the server must NOT interpret
business semantics, run application services, mutate state, process
Shutdown, or respond as an authenticated session. G1 bounded framing
rules still apply.

### 7. Failure behavior

Authentication failure fails closed: close the connection, perform no
business mutation, create no Task lifecycle event, grant no READY
session. No silent fallback to endpoint-permissions-only,
`client_kind`, PID checks, TCP/HTTP, or anonymous connection applies.
No rich protocol error to the unauthenticated peer is required —
closing the transport is preferred, since the peer is not a protocol
participant. An internal diagnostic error may be exposed locally.

### 8. Windows impersonation safety

`ImpersonateNamedPipeClient` return value is checked, and
`RevertToSelf` executes on every successful impersonation path
(RAII/guard-style cleanup in implementation). No unrelated
application work runs while impersonating; scope is minimal:
impersonate → read token identity → copy/compare SID → revert
immediately.

### 9. LOCAL SYSTEM

The G1 pipe DACL may keep allowing current user plus LOCAL SYSTEM as
transport hardening — not application authentication. A normal desktop
`agentd` runs as the logged-in user, so `SYSTEM SID != agentd user
SID` and a SYSTEM peer is not authenticated as that user. No special
SYSTEM authorization is created in G2.

### 10. No executable-identity or cryptographic schemes in G2

Binary path, process name, client-supplied code-signature strings,
argv, environment, and `client_kind` are not the G2 principal. No
shared-secret file, API key, JWT, password, OAuth, certificate,
device keypair, mTLS, or Keychain/Credential-Manager secret is
introduced for local desktop peer auth: those are a different
credential lifecycle, unnecessary for authenticating the current
OS-user principal. Remote Worker / Cloud will use a separate
authenticated device-identity design; local peer auth is not
conflated with future remote mTLS.

### 11. Client-side posture

Existing endpoint verification and ownership rules stand. The G2
minimum is authoritative server-side authentication of the connected
client before control/business requests gain authority. No speculative
cross-platform application-binary attestation is introduced; symmetric
same-user verification on Unix may later serve as defense in depth,
not as the core decision.

### 12. Layer ownership

```text
ipc-transport → OS transport mechanics; extraction of kernel/
                 token-backed peer identity
agentd          → local authentication policy: peer principal must
                 equal daemon principal
ipc-control     → handshake/version/ping semantics; decides NO
                 OS identity
app-services    → business mutations; receives requests only after
                 an authenticated READY connection
```

No OS-specific UID/SID logic enters the Task domain or app-services.

### 13. Authentication vs authorization

Authentication answers "who is this local OS peer"; authorization
answers "may this principal perform this operation". G2 authenticates
the peer; the full Policy Engine/approval framework stays G6 and is
not prematurely implemented. For the G2 bootstrap, an authenticated
same-user local client is eligible for the limited G2 control API —
not the final policy model for privileged agent actions.

### 14. Connection lifetime and non-portability

Authentication binds to one accepted transport connection. Reconnect
means new transport → new peer authentication → new handshake; state
is never inherited across reconnects. The result is never persisted:
no `AuthenticatedPeer` durable authorization, no UID/SID auth state
in the Task domain. Transient request provenance may later carry the
peer identity where architecture permits — separate from Task
authority.

### 15. Journal and audit boundaries

Peer-auth success/failure is NOT a Task lifecycle Event Journal
event; the journal is not a generic security log. Durable
security-policy/audit records belong to the future Security Audit —
which is not introduced in G2.

### 16. Protocol surface

No protobuf identity fields are added: no `uid`, `sid`, `username`,
`auth_token`, `process_id`, `binary_path`, or `signature`. OS peer
authentication happens below protocol semantics; the client never
asserts its own identity.

### 17. Implementation test matrix (TASK-0026 minimum)

Unix:

```text
same UID peer → accepted
different UID peer → rejected where safely testable
peer-auth failure → no handshake READY
reconnect → fresh authentication
existing endpoint ownership/permission tests stay green
```

Where a second real UID is impractical in CI, the
credential-comparison policy is isolated so mismatched-UID tests are
deterministic without root; actual kernel credential extraction is
still tested on the Unix host.

Windows:

```text
same SID → accepted
different SID → rejected at the policy boundary
impersonation failure → fail closed
token query failure → fail closed
RevertToSelf always executed after successful impersonation
first frame not decoded/acted upon before successful auth
no READY session on auth failure
```

Real Windows runtime verification runs when Windows CI/host exists;
cross-compilation stays required on non-Windows hosts where the
toolchain permits.

## Alternatives considered

- `client_kind` as trust: rejected; self-asserted metadata, not
  authentication (TB2 forbids "well-formed means authorized").
- PID / binary-path / signature-string identity: rejected; not the
  OS-user principal, spoofable or client-supplied.
- Local shared secret / JWT / mTLS for desktop IPC: rejected for G2;
  separate credential lifecycle, unnecessary for the OS-user
  principal, conflates local and future remote trust.
- Endpoint permissions alone: rejected as sufficient; kept as the
  complementary defense-in-depth layer.

## Consequences

- Mutable business IPC (TASK-0027) may only be exposed behind the
  TASK-0026 implementation of this decision.
- Same-user malicious code stays outside this boundary by explicit
  threat-model limitation, not by oversight.
- Future remote/device identity work starts from a clean,
  unconflated local baseline.

## Security impact

Activates the TB2 peer-auth open item and converts THR-003's open
"peer-auth mechanism unspecified" risk into a specified,
fail-closed OS-backed boundary before mutable G2 IPC. Same-user
threat limitation is explicit (section 1). No new cryptographic
material, no secret storage, no Security Audit storage introduced.

## Migration / replacement impact

Unix credential extraction and Windows token handling stay behind the
transport/policy ports (INV-026, INV-077 platform-abstraction
discipline); mechanism upgrades (e.g. symmetric Unix verification as
defense in depth) need no domain changes. Future remote mTLS is a
separate design, unaffected.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §9 (IPC), §53
  (trust boundaries incl. TB2)
- `INVARIANTS.md`: INV-007, INV-008, INV-013, INV-026, INV-073, INV-077
- [`../ipc.md`](../ipc.md) (§7, §13: PRE-G2 peer-auth classification)
- [`../../security/TRUST_BOUNDARIES.md`](../../security/TRUST_BOUNDARIES.md) (TB2)
- [`../../security/THREAT_MODEL.md`](../../security/THREAT_MODEL.md) (THR-003)
- ADR-0003 (local IPC), ADR-0013 (policy + isolation; G6 owns full
  authorization), ADR-0019 (gate-scoped activation: this deferred item
  activates now, before its dependent implementation)
- Implementability note: G1 code already centralizes the primitives
  this builds on — `current_uid()`/`geteuid` ownership checks
  (`crates/ipc-transport/src/platform/unix.rs`) and process-token SID
  handling (`platform/windows.rs`); no peer-credential extraction
  exists yet, which is exactly TASK-0026's work. G1 behavior is not
  redesigned here.
