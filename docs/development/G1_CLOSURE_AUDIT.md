# G1 Closure Audit

> Independent gate verification for G1 (Tauri ↔ agentd), performed by
> TASK-0021. Production sources were frozen during this audit: no file under
> `bins/**/src/**`, `crates/**/src/**`, or `crates/ipc-contracts/proto/**`
> was modified. New verification material lives only in
> `bins/agentctl/tests/real_process_matrix.rs` (allowed `**/tests/**`
> path) and this report. Prior task reports were not assumed correct;
> everything below was re-executed against current code.

## Result

```text
G1_INCOMPLETE
```

One hard gate requirement fails: the Windows implementation does not
compile for its target (`E0432`, missing `windows-sys` feature). The defect
is small, precisely characterized, and verified minimal via a scratch-copy
experiment (see § Windows Verification); per the verification rule it is
reported here, not repaired in the repository.

No gate transition is performed: `current_gate` remains `G1`, G1 remains
`in_progress`, `completed_through` remains `TASK-0020`.

## Implementation Surface

G1 is exactly the accepted surface, confirmed by code inspection:

- Control operations: `start`, `handshake`, `ping`, version negotiation,
  `reconnect`, `shutdown` — no other behavior exists in the bins/crates.
- Proto messages (`crates/ipc-contracts/proto/agentd_control.proto`,
  sha256 `e198fa35…`, byte-identical): `ControlRequest`,
  `ControlResponse`, `HandshakeRequest/Response/Accepted/Rejected`,
  `PingRequest/Response`, `ShutdownRequest/Response`. No `service`/`rpc`,
  no `StartDaemonRequest`, `ReconnectRequest`, `ForceKillRequest`,
  `TaskRequest`, or generic command.
- Layers preserved: contracts (DTOs) → transport (bytes/framing/OS
  primitives) → control (protocol semantics) → agentd/agentctl
  (lifecycle integration).

## Canonical Failure Matrix

| # | Scenario | Result | Evidence |
|---|----------|--------|----------|
| 1 | Daemon missing: `start` launches + handshakes; `ping` may start; `shutdown` never starts | PASS | `real_process_matrix` phases 1–2, 7; `agentctl` unit tests `connect_first_*`, `missing_daemon_is_spawned_once_*`, `shutdown_without_daemon_never_spawns` |
| 2 | Wrong protocol `[99]` → `HandshakeRejected` + versions `[0]`; build mismatch accepted | PASS | `ipc-control` tests `wrong_protocol_version_is_rejected_with_daemon_versions`, `build_version_mismatch_is_allowed`; negotiation matrix tests |
| 3 | Daemon crash: loss observed, stale node sane, next start converges with fresh handshake | PASS | `real_process_matrix` phase 8 (kill -9 → stale node → start → ping → shutdown); no session reuse by construction (sessions own no persistent state; reconnect always builds a new transport + handshake) |
| 4 | Stale Unix socket: inspect → probe → snapshot → re-check → remove-if-unchanged → bind → handshake → ping | PASS | `agentd` test `stale_socket_is_recovered`; `real_process_matrix` phase 8; transport snapshot/remove unit tests |
| 5 | Permission/security: insecure dir, symlink, regular file, foreign ownership rejected; nothing deleted/chmodded; no alternate endpoint; no TCP/HTTP fallback | PASS | `ipc-transport` unix tests (0700 enforcement, symlink/file preservation); `agentd` test `unexpected_objects_fail_closed_and_preserved`; `agentctl` retry-classification test (security never retried); grep audit confirms zero TCP/HTTP/fallback code |
| 6 | Reconnect: new connection + new handshake; backoff 50/100/200/400/800/1000… within 10s monotonic budget | PASS | `planned_delays_fit_budget_without_weakening_constants`, `backoff_sequence_matches_accepted_profile`, `concurrent_racers_converge_on_one_owner`; `shutdown_session` takes a fresh session per call |

## Protocol Abuse Matrix

| Check | Result | Evidence |
|-------|--------|----------|
| Zero-length frame rejected | PASS | `framing` encode + decode tests |
| >1 MiB frame rejected (encode) | PASS | `oversized_payload_encode_is_rejected` |
| Oversized declared length rejected without payload allocation | PASS | `declared_oversized_incoming_frame_is_rejected_without_payload` |
| Exact u32 big-endian prefix | PASS | `length_prefix_is_exact_u32_big_endian` |
| 1 MiB accepted | PASS | `one_mebibyte_payload_is_accepted` |
| Malformed protobuf terminates session (then unusable) | PASS | `malformed_protobuf_terminates_session`, `malformed_client_does_not_kill_daemon` |
| Ping before handshake rejected | PASS | `ping_before_handshake_is_violation`, `client_ping_before_handshake_is_rejected_locally` |
| Shutdown before handshake rejected | PASS | `shutdown_before_handshake_is_violation` |
| Second handshake rejected | PASS | `duplicate_handshake_is_rejected_on_both_sides` |
| Unknown `client_kind` rejected (descriptive, never auth) | PASS | `unknown_client_kind_is_rejected`, registry exactness test |
| Unknown capability does not reject; accepted set empty | PASS | `unknown_capabilities_are_accepted_but_empty`, intersection test |
| Unexpected response variant rejected | PASS | `unexpected_response_variant_is_protocol_error` |
| No resynchronization after fatal errors | PASS | By construction: every fatal path sets `Terminated`; subsequent ops return `SessionTerminated` (tested) |

## Endpoint Security

PASS. Runtime dir admitted only with owner == euid, non-symlink, mode
exactly `0700` (created with explicit chmod against umask); socket bound
at `0600`; existing symlinks/files/foreign nodes fail closed and are
preserved (never unlinked, renamed, chmodded); no alternate endpoint; no
network fallback. Verified by `ipc-transport` unix tests (28 pass) and
`agentd` unexpected-object tests.

## Singleton / Start Races

- In-process: PASS (`second_daemon_reports_already_running`: live probe →
  `AlreadyRunning`, winner serves).
- Real multi-process, same canonical endpoint: PASS (new
  `real_process_matrix`: second real `agentd` exits code 2, winner pingable,
  exactly one daemon PID throughout).
- Concurrent real launchers: PASS (3× `agentctl start` converge, one PID).
- No PID files, no alternate names (code inspection + tests).

## Reconnect

PASS (see matrix #6). Fresh handshake per reconnect is structural:
`ClientSession` holds only connection-local state; `connect_or_start`
always builds a new transport + session. `ShutdownRequest` is never
replayed after ambiguity (`ambiguous_shutdown_is_never_replayed` proves
exactly one send).

## Shutdown

PASS. Post-handshake `ShutdownRequest` → `ShutdownResponse` (protocol-only
`respond_shutdown`) → admission stops → control plane winds down
(`abort_all` + reap) → owned endpoint removed via identity-verified removal
→ process exits 0. Absent daemon → `NotRunning`, no spawn. Ambiguous loss →
explicit non-replayed error. Deadlines `SHUTDOWN_RESPONSE_TIMEOUT = 5s`
and `DISAPPEAR_TIMEOUT = 5s` are unweakened constants; `PING_TIMEOUT = 2s`
proven by a real 2-second timeout test; `HANDSHAKE_TIMEOUT = 5s` equals the
transport frame deadline by assertion.

## Windows Verification

| Item | Result |
|------|--------|
| Toolchain: `rustup target add x86_64-pc-windows-msvc` (environment-only) | PASS |
| `cargo check --workspace --target x86_64-pc-windows-msvc` | **FAIL** — `E0432`: `windows_sys::Win32::Security::Authorization` import in `crates/ipc-transport/src/platform/windows.rs` requires the `Win32_Security_Authorization` feature, which is not enabled in `crates/ipc-transport/Cargo.toml`. Full error captured in evidence log below. |
| `cargo clippy … --target … -D warnings` | NOT_EXECUTED (blocked behind the check failure; evaluated in scratch, see below) |
| Windows runtime execution | NOT_EXECUTED — no Windows host available. Recorded honestly; the hard minimum for this gate is compilation, which already fails. |
| Static profile audit of the Windows code | PASS (by inspection, all present): SID-suffixed pipe name (`PIPE_NAME_PREFIX` + `current_user_sid()`; never username/email); explicit user+SYSTEM DACL via `create_with_security_attributes_raw` (no Everyone/Anonymous strings anywhere in the tree); `reject_remote_clients(true)`; `PipeMode::Byte` on server and client; `max_instances(MAX_CONTROL_CLIENTS = 8)`; `first_pipe_instance(first)` with `bind_first()` using `true`; second owner maps to `EndpointOwned`/`AlreadyRunning`; no alternate names. |

### Scratch-copy remediation experiment (not applied to the repository)

To prove minimality, the repo (minus `target/`) was copied to a scratch
directory and the following candidate fix applied there only:

1. `crates/ipc-transport/Cargo.toml`: add `"Win32_Security_Authorization"`
   to the `windows-sys` features.
2. `platform/windows.rs`: `#[cfg(test)]`-gate the test-only imports
   (`BOOL`, `GetSecurityDescriptorDacl`, `GetAclInformation`, `GetAce`,
   `ACL_SIZE_INFORMATION`, `AclSizeInformation`, `PSECURITY_DESCRIPTOR`,
   `ACCESS_ALLOWED_ACE_TYPE_VALUE`) and document `#[allow(dead_code)]` on
   the `acl` buffer field (owned for descriptor lifetime; read by the OS,
   not by Rust code).
3. `bins/agentd/src/lib.rs`: `#[cfg(unix)]`-gate the `ClientConfig` /
   `ClientSession` import (used only by the Unix liveness probe).

Scratch results: Windows `cargo check --workspace` 0 errors; Windows
`cargo clippy --workspace --all-targets --all-features -D warnings`
passes; host `cargo test --workspace` passes; real-process matrix passes
with a clean host afterward. The defect is therefore a small,
well-understood gap — but under the TASK-0021 production freeze it is
reported, not repaired. Minimal remediation for the follow-up: apply the
three items above (plus `cargo fmt`), then re-run this gate's Windows
commands in the repository.

### Exact gate failure (repository state)

```text
error[E0432]: unresolved import `windows_sys::Win32::Security::Authorization`
  --> crates/ipc-transport/src/platform/windows.rs
  (module gated behind the `Win32_Security_Authorization` feature,
   not enabled in crates/ipc-transport/Cargo.toml)
error: could not compile `ipc-transport` (lib) due to 1 previous error
```

Plus two `unused-imports` warnings on the same file for test-only items
(harmless on host builds where the module is compiled out; addressed by
item 2 above for `-D warnings` cleanliness on the Windows target).

## Dependency Boundaries

PASS. `ipc-transport` has no protocol/crate deps beyond bytes/futures/
tokio-util/libc; `ipc-control → {ipc-contracts, ipc-transport}` with
prost 0.14.4 + tokio 1.51.4 (time-minimal) only; `agentd`/`agentctl → G1
layers` only. No Tauri, SQLite, provider SDK, tonic, HTTP/TCP, serde,
clap. Tokio features stay minimal (no `full`).

## G2 Leakage Audit

PASS. Grep over all bin/crate sources for Task persistence, dependency
graphs, SQLite, Event Journal, provider/memory/context machinery returns
zero hits. The only `tauri-host` strings are the accepted G1 client-kind
registry values. G1 remains control-plane bootstrap only.

## Remaining Known Issues

1. **Gate defect (this audit's verdict):** Windows target compilation
   fails (E0432, details above). Remediation is characterized and proven
   minimal in scratch; it must land as a separate change with a fresh
   Windows check/clippy run in the repository.
2. Windows runtime execution not exercised (no Windows host). After the
   fix, `cargo test` for the Windows target (or execution on Windows)
   should be recorded before any claim beyond compilation.
3. Full multi-process kill-race (two real `agentd` binaries racing from
   absence) is covered at the in-process level plus real singleton/race
   tests; a dedicated two-process race-from-absence run can join the
   post-fix verification pass.

## Gate Transition

None performed. `planning/roadmap.yaml` (`current_gate: G1`,
G1 `in_progress`) and `planning/tasks/queue.yaml`
(`completed_through: TASK-0020`) are intentionally unchanged.

## Verification Evidence

- `cargo fmt --all -- --check` — clean.
- `cargo check --workspace` (host) — clean.
- `cargo test --workspace` (host) — 77 passed, 0 failed:
  agentctl 10, agentd 6, ipc-contracts 7, ipc-control 25, ipc-transport
  28, real-process matrix 1 (9 ordered phases, host left clean).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  (host) — clean.
- `cargo tree` for all four G1 packages — accepted families only.
- Real-process matrix output: start→ready, ping→pong, idempotent
  restarts (one PID), second agentd→exit 2, 3× concurrent launchers
  converge, shutdown→complete with endpoint gone, shutdown-absent→
  not-running without spawn, kill-9→stale node→start recovery→ping→
  clean shutdown.
- Windows: `rustup target add x86_64-pc-windows-msvc` succeeded;
  `cargo check --workspace --target x86_64-pc-windows-msvc` fails with
  the E0432 above; scratch-copy experiment proves the minimal fix.
- Audits: proto surface (10 messages, no service/rpc), dependency
  direction, G2-leakage grep (zero hits), unsafe inventory unchanged
  from TASK-0018/0020 (Windows SID/security interop + `geteuid` +
  test-only env mutation).
