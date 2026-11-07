# Local control authorization options

Status: the owner selected Option B on 2026-11-04. The accepted contract
is [ADR-0050](../architecture/adr/ADR-0050-os-user-presence-for-local-control.md);
implementation and verification are in progress. This note preserves the
alternatives and the separate internal-helper requirement.

## Existing contract and problem

[ADR-0020](../architecture/adr/ADR-0020-local-ipc-peer-authentication.md)
authenticates the OS user. Sections 1, 3, 10 and 16 explicitly exclude
binary attestation and shared-secret peer identity. `client_kind`, argv,
environment, PID and a claimed executable path are not credentials.
Shutdown currently needs an authenticated READY connection. Execution also
passes the trusted execution, policy, approval and Runner checks.

A 0600 file is readable by the same UID. A file token can add an explicit
operation capability and invalidate stale clients at daemon restart; it
cannot establish human presence or exclude arbitrary same-user code.
The old ADR's principal must not silently acquire either stronger claim.

## Option A: explicit operation capability

Keep the OS user as the principal and introduce a separate, random
per-daemon capability for Shutdown and mutating execution operations.
Create its file atomically at mode 0600 in the admitted 0700 runtime
directory. Require exact owner, type, mode and single-link admission when
reading; refuse links and oversized or malformed input. Never put it in
argv, logs, plan hashes or durable domain data. Rotate at every daemon
start and remove only the inode that daemon created.

Negotiate a new capability/version, update CLI and native desktop clients,
and fail old or missing-capability mutations closed. Leave diagnostics
and read-only queries available under the existing peer contract. Compare
fixed-length capability bytes in constant time. Define response-loss and
restart behavior without replaying a mutation. Tests must cover missing,
wrong, old and linked tokens plus valid shipped clients.

This implements possession checking within the existing same-user trust
boundary. It does not remediate malicious same-UID access.

## Option B: human-authorized operations

Require OS-protected user presence before issuing an operation-bound,
short-lived authorization. Bind it to daemon instance, action and request
digest; use a one-time challenge and consume it before mutation. Keep the
proof outside the repository, provider environment and web storage. A
macOS implementation needs an OS-managed key with access control and a
defined interaction with headless CLI/routines; a plain passwordless
file or a self-asserted `user_confirmed` bit is insufficient.

This changes the trust and unattended-operation model and needs a new
accepted ADR, including recovery and non-macOS behavior. No unattended
fallback may silently grant the interactive operation.

## Internal candidate helper authority

`__candidate-*` now requires a daemon-issued one-shot permit before reading
or mutating candidate files. The verified Runner invocation creates a
private socketpair and a random 256-bit nonce bound to the complete helper
argv and input digest. The child receives the nonce and bounded argv header
on stdin, consumes the permit over the inherited socket on fd 2, then does
the file operation. The parent answers once and closes; the 30-second
deadline and process lifetime bound the permit. No token file, public mint
route or extra listener survives shutdown. Cleanup uses the same governed
launch path and receives its own permit.

The child authenticates the socket's kernel peer UID, parent PID and audit
token. macOS `csops_audittoken` must return a valid, non-debugged parent with
the same code-directory hash as the child image. A caller-created token or
socket from another executable cannot substitute. Unsupported platforms
and unavailable kernel identity checks refuse. Tests cover direct calls,
another-image issuer, nonce/argv/input mismatch, expiry, replay and an
authenticated write/evaluate/remove cycle. Full Runner containment remains
subject to the environment-dependent checks in the remediation ledger.

This checks the running parent image, not its publisher or installation.
It does not prove injection resistance or a hardened runtime. Same-user
filesystem control remains a separate limitation.
