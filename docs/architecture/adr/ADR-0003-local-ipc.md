# ADR-0003 - Local IPC over UDS / Named Pipe with versioned typed contracts

Status: Accepted
Date: 2026-09-09

## Context

The thin Tauri host, `agentd` and `agentctl` need a local IPC channel with
versioning, typed schemas and independence from any single language toolchain
(Rust now, Go tooling later).

## Decision

- Transport: Unix Domain Socket on macOS/Linux, Named Pipe on Windows.
- Contracts: versioned Protobuf as the canonical local process contract format.
- Localhost HTTP is not used for core desktop IPC.
- Startup supports protocol version negotiation, daemon build version, client
  identity, capability negotiation and reconnect; the socket/pipe lives in a
  user-private runtime directory.

## Alternatives considered

- Localhost HTTP: rejected for core desktop IPC.
- Unversioned/ad-hoc JSON over pipes: rejected; versioning and typed schema are
  required for Rust/Go interop and transport independence.

## Consequences

- All local clients speak the same versioned contracts; contract changes follow
  schema-version discipline with correlation/causation metadata where events apply.
- Per-installation keypair/challenge-response for local-client authentication may
  be added later to strengthen the trust boundary.

## Security impact

The socket/pipe must live in a user-private runtime directory. Trust boundaries
TB1 (WebView → Tauri host) and TB2 (Tauri host → agentd) each require a dedicated
threat-model section as the IPC surface is implemented.

## Migration / replacement impact

Protobuf keeps contracts transport-independent, so the framing can evolve without
redefining message semantics.

## G1 implementation profile (TASK-0014 refinement)

This section refines — never replaces — the decision above with the accepted
G1 implementation direction. Full operational semantics live in
[`../ipc.md`](../ipc.md); the summary here is binding:

- Async runtime: Tokio (`tokio ~1.51.4` LTS family, held through G1).
- Protobuf codegen: `prost` / `prost-build` 0.14.4 family, build-time
  generation, `protoc-bin-vendored =3.2.0` pinned; generated Rust goes to
  Cargo `OUT_DIR` and is never committed.
- No gRPC: Protobuf messages over our local framed byte transport
  (UDS stream on Unix, Named Pipe Byte mode on Windows).
- Framing: 4-byte big-endian `payload_length: u32` + payload;
  `MAX_FRAME_BYTES = 1_048_576`; zero-length frames invalid; malformed
  frames close the connection without resynchronization.
- Sequential model: one in-flight request per connection, no correlation
  IDs in G1; up to 8 simultaneous clients (`MAX_CONTROL_CLIENTS = 8`).
- Handshake: first message, exactly once per connection; protocol `0`
  preserved with highest-common selection; client kinds exactly
  `tauri-host` and `agentctl` (descriptive only, never authentication);
  capabilities may be empty with no closed registry.
- Endpoints: per-user private directories (Linux XDG with `0700` fallback,
  macOS `$TMPDIR`, `agentd.sock` at `0600`); Windows SID-suffixed pipe name
  with explicit user+SYSTEM DACL, byte mode, remote rejection;
  first-instance singleton on both platforms; verified stale-socket cleanup;
  connect-first start races.
- Budgets: 10 s reconnect budget with staged backoff; 5 s frame/handshake/
  shutdown timings; 2 s ping deadline; no idle disconnect.
- G1 shutdown authorization basis is OS/user-private endpoint admission
  plus successful handshake (not `client_kind`); no force-kill command.
- Exact cryptographic/per-installation peer authentication stays open and
  is classified PRE-G2, not a G1 blocker.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §1 (decision table), §9 (IPC)
- `INVARIANTS.md`: INV-026
