# G1 Windows Remediation

> Follow-up to `G1_CLOSURE_AUDIT.md` (TASK-0021, verdict `G1_INCOMPLETE`).
> That file is historical evidence of the failed first closure attempt and is
> left unchanged. This report records the minimal production remediation
> (TASK-0021R1), its verification, and the conditional G1 closure.

## Original Blocker

TASK-0021 audited gate G1 (Tauri ↔ agentd) and returned `G1_INCOMPLETE` on
exactly one hard blocker: the Windows implementation did not compile for its
target.

```text
error[E0432]: unresolved import `windows_sys::Win32::Security::Authorization`
  --> crates/ipc-transport/src/platform/windows.rs
  (module gated behind the `Win32_Security_Authorization` feature,
   not enabled in crates/ipc-transport/Cargo.toml)
error: could not compile `ipc-transport` (lib) due to 1 previous error
```

Plus two `unused-imports` warnings on the same file for test-only items, and
Unix-only probe imports in `bins/agentd/src/lib.rs` that would break
warning-free Windows clippy (`-D warnings`).

Reproduced in-repo before editing via
`cargo check --workspace --target x86_64-pc-windows-msvc` (1 error E0432 +
2 unused-import warnings, same as the audit).

## Root Cause

`crates/ipc-transport/src/platform/windows.rs` uses
`windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW` (current-
user SID string for the canonical pipe name), but
`crates/ipc-transport/Cargo.toml` enabled only `Win32_Foundation`,
`Win32_Security`, `Win32_System_Threading` — missing the
`Win32_Security_Authorization` feature that gates the `Authorization` module
in `windows-sys` 0.61. Secondary: test-only Win32 read-back imports and the
test-only `ACCESS_ALLOWED_ACE_TYPE_VALUE` constant were unconditional (warn
under `-D warnings` on the Windows target), the OS-read ACL backing buffer
looked dead to the compiler, and `ClientConfig`/`ClientSession` (Unix
liveness-probe only) were imported unconditionally in `agentd`.

## Minimal Fix

Three production files, no behavior change, no new crates, no proto change:

1. `crates/ipc-transport/Cargo.toml` — add exactly one feature to the
   existing Windows-only `windows-sys` dependency:
   `Win32_Security_Authorization`. No feature-set broadening, still
   Windows-only, no second Windows crate.
2. `crates/ipc-transport/src/platform/windows.rs` —
   `#[cfg(test)]`-gate the genuinely test-only imports (`BOOL`,
   `AclSizeInformation`, `GetAce`, `GetAclInformation`,
   `GetSecurityDescriptorDacl`, `ACL_SIZE_INFORMATION`,
   `PSECURITY_DESCRIPTOR`) and the test-only
   `ACCESS_ALLOWED_ACE_TYPE_VALUE` constant; keep all production imports
   (`ConvertSidToStringSidW`, `AddAccessAllowedAceEx`, `CreateWellKnownSid`,
   `GetLengthSid`, `GetTokenInformation`, `InitializeAcl`,
   `InitializeSecurityDescriptor`, `SetSecurityDescriptorDacl`, etc.)
   unconditional. Add narrowly scoped `#[allow(dead_code)]` on the
   `PipeSecurityAttributes::acl` field with a safety/lifetime comment: it is
   the owned DACL backing store referenced by `descriptor` via
   `SetSecurityDescriptorDacl`, read by the OS during pipe creation — Rust
   never reads it again, but it must stay owned to keep the
   descriptor/ACL pointers valid. No buffer removal, leak, global, or
   crate-level allow.
3. `bins/agentd/src/lib.rs` — `#[cfg(unix)]`-gate `ClientConfig` /
   `ClientSession` (used only by the Unix stale-endpoint liveness probe and
   Unix tests). No fake Windows implementations.

`cargo fmt` applied. `Cargo.lock` untouched (feature-only change, no new
crate/version; `windows-sys` stays 0.61.2). Canonical proto byte-identical
(sha256 `e198fa356453fb646f8b74fb42d8b6fb8bffe61a730fdff343482ebd6aa9ad3b`).

Windows security profile unchanged: current-user-SID pipe identity, explicit
user+SYSTEM DACL, `reject_remote_clients(true)`, byte mode both sides,
`max_instances = 8`, `first_pipe_instance` singleton. Unix behavior, framing,
handshake, version negotiation, ping, shutdown, reconnect, stale-socket
policy, TOCTOU protections untouched.

## Windows Compilation Evidence

```text
Windows target compile: PASS
Windows target clippy: PASS
Windows runtime execution: NOT EXECUTED
```

- `cargo check --workspace --target x86_64-pc-windows-msvc` — PASS, zero
  errors (previously 1 × E0432).
- `cargo clippy --workspace --all-targets --all-features --target
  x86_64-pc-windows-msvc -- -D warnings` — PASS, zero warnings/errors.
- Windows-target test surfaces compile through the same all-targets clippy
  invocation (Windows `#[cfg(test)]` module included in the check).
- Windows runtime execution: NOT EXECUTED — no Windows host on this machine.
  No claim of Windows runtime testing is made; post-fix Windows execution
  remains a future CI/platform-validation item.

## Host Regression Evidence

- `cargo fmt --all -- --check` — clean.
- `cargo check --workspace` (host) — clean.
- `cargo test --workspace` (host) — 77 passed, 0 failed (agentctl 10,
  agentd 6, ipc-contracts 7, ipc-control 25, ipc-transport 28, real-process
  matrix 1), matching the TASK-0021 baseline.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  (host) — clean.
- `cargo tree -p ipc-transport --target x86_64-pc-windows-msvc` — no new
  crates; direct deps unchanged (bytes, futures-core, futures-sink, tokio,
  tokio-util + `windows-sys` 0.61.2 on Windows, `libc` on Unix). New direct
  dependency count: 0.

## Process-Level Verification

- `cargo test -p agentctl --test real_process_matrix` — PASS (all 9 ordered
  phases: start, ping, idempotent start, second daemon loses with exit 2,
  concurrent launchers converge, shutdown, shutdown-absent/no-spawn,
  kill -9 → stale recovery → ping → clean shutdown). Test not weakened.
- Focused two-real-daemon race from absence (ad-hoc host verification, not a
  new committed test): two real `agentd` processes spawned simultaneously
  against an initially absent canonical endpoint — exactly one canonical
  endpoint owner survived, the loser exited `AlreadyRunning` / code 2
  (`agentd: already running (endpoint owned)`), the winner stayed usable
  through handshake + ping (`agentctl: pong`), and clean shutdown succeeded
  with the host left clean (no stray daemon, endpoint gone). Verdict: PASS.
  (Existing matrix phase 4 covers singleton-from-presence and phase 5 covers
  concurrent launchers-from-presence; this run covers the race-from-absence
  gap noted in the audit § Remaining Known Issues #3.)
- Regression security checks re-confirmed: Unix stale cleanup still
  inspect → probe → snapshot → re-check → remove-if-unchanged → bind
  (`probe_live_daemon`, `recover_stale`, `remove_endpoint_if_unchanged`
  intact); unexpected endpoint objects fail closed and are preserved
  (`unexpected_objects_fail_closed_and_preserved` passes); shutdown-absent
  never spawns (`shutdown_without_daemon_never_spawns` passes); reconnect
  builds a fresh transport + handshake per attempt (no session reuse);
  shutdown ambiguity never replayed (`ambiguous_shutdown_is_never_replayed`
  passes); grep audit: no TCP/HTTP fallback (one `tonic` substring hit is
  `monotonic` in a timing comment — false positive), no `service`/`rpc` in
  proto, no SQLite/Task/Dependency/Event-Journal/provider/memory/context
  code beyond explicit non-goal doc comments, no Everyone/Anonymous grant
  (only a `Never Everyone/Anonymous` comment), `client_kind` never used as
  auth (only a `never client_kind` comment in the shutdown path).

## Remaining Known Limitations

1. Windows runtime execution not exercised (no Windows host). Compilation +
   warning-free clippy are proven; on-Windows execution (or Windows CI)
   should be recorded before any claim beyond compilation.
2. No new hard G1 blocker discovered during remediation.

## Final G1 Verdict

```text
G1_COMPLETE
```

All closure conditions hold: Windows workspace check passes, Windows clippy
passes, host workspace passes (fmt/check/test/clippy), real-process matrix
passes, singleton/start race remains correct (including the dedicated
race-from-absence run), no architecture/security behavior changed, no new
blocker discovered.

## Gate Transition

Per TASK-0021R1 §17, with `G1_COMPLETE`:

- `planning/roadmap.yaml`: `current_gate: G1 → G2`, G1 `in_progress →
  completed`, G2 `planned → in_progress`.
- `planning/tasks/queue.yaml`: `completed_through: TASK-0020 → TASK-0021`
  (canonical sequential ID; TASK-0021R1 is the remediation adjunct, so the
  pair TASK-0021 audit + TASK-0021R1 remediation = G1 closure satisfied),
  `current_gate: G0 → G2` (the G0 value was stale — G1 tasks executed while
  it still read G0; now aligned with the roadmap).
- Next G2 Task Core task left unchanged/eligible; G2 implementation not
  started here.
