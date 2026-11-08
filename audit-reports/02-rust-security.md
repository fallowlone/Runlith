# Rust security audit

The most significant application security issue is asymmetric local IPC authentication: the daemon authenticates clients, but Unix clients do not authenticate the daemon. Crypto dependency findings and informational advisories are detailed separately in [06-dependencies.md](06-dependencies.md).

## Confirmed vulnerabilities

### F01 - Unix clients trust an unauthenticated server

- **Severity:** High in the shared-temp-parent scenario below. **Confidence:** High for the source/control gap; no dynamic cross-user exploit was attempted.
- **Locations:** `crates/ipc-transport/src/platform/unix.rs:59-76,82-92,344-359,408-445`; `bins/agentctl/src/lib.rs:964-1015`; `apps/desktop/src-tauri/src/agentd.rs:181-201`; existing reusable peer-identity primitives at `crates/ipc-transport/src/platform/peer.rs:47-108,258-278`.
- **Evidence:** server binding validates a current-user-owned 0700 runtime directory and creates a 0600 socket (`unix.rs:109-166,308-339`). By contrast, shared `connect()` only calls `UnixStream::connect` and maps errors. It neither validates the directory/socket owner nor calls `getpeereid`/`SO_PEERCRED` on the connected server. The CLI completes a protocol handshake and then encodes/writes `VaultCoreRequest`, including password/token/recovery-bearing request variants. The handshake establishes protocol compatibility, not OS identity or a cryptographic server identity.
- **Root cause:** transport admission checks protect daemon listeners only. Client callers assume that a successful connection to a predictable path establishes the same trust boundary.
- **Reachability / realistic preconditions:** macOS normally has a private per-user `TMPDIR`, which prevents a different user planting an endpoint there. However, macOS shared `TMPDIR=/tmp`, the Unix fallback temp parent, or an explicitly shared runtime override produce a predictable leaf such as `/tmp/ai-engineering-os-<victim UID>`. Before the genuine daemon creates it, a different local user can create that leaf as an accessible directory and place an accessible fake socket inside. Adding the victim's UID to a filename does not make its owner the victim. The listener's fail-closed ownership checks never run on a successful connect-first client path. This is **not** a claim that a foreign user can replace a socket already protected by the genuine private directory.
- **Input -> operation -> boundary:** foreign process socket -> successful client connect -> attacker-compatible handshake reply -> CLI sends a vault request across an unauthenticated cross-user IPC boundary. The desktop also accepts counterfeit project/decision data through this shared connect path, although its custom commands do not expose vault operations.
- **Realistic impact:** a planted endpoint can capture the next secret-bearing CLI request and forge ordinary responses. The attacker does not need to defeat the real daemon's client authentication. A same-UID malicious process is outside the documented peer-auth guarantee and is not used to justify this finding.
- **Verification status:** confirmed analytical vulnerability from complete connect/handshake/send tracing. Cross-user runtime proof is missing because this environment denies socket binding and no second-user environment was authorized/provisioned. No real secret was read or transmitted.
- **Safe reproduction procedure:** on a disposable two-user VM, select a dedicated shared test parent and use separate test users. Have the first user create an accessible fake endpoint at the second user's expected leaf. Run a minimal protocol-compatible fixture server that records only whether a synthetic marker request arrived. Invoke the second user's client against that isolated endpoint with dummy vault bytes. Current behavior should send the marker; corrected behavior must reject the foreign server before handshake/business bytes. Do not plant anything at a real user's endpoint, use real vault inputs, or change global `TMPDIR`.
- **Recommended solution:** in the shared Unix client connect path, authenticate the connected server's kernel-reported principal against the client's effective UID before any handshake or secret bytes. Also non-mutatively validate runtime/socket admission where appropriate. Reuse the existing peer primitives; do not treat path spelling or the `tauri-host`/`agentctl` claim as authentication. Keep listener-side checks.
- **Estimated effort:** 1-2 days, including same-user success, foreign-server refusal, shared-temp precreation and replacement/race tests.

### F02 - Private keys are command-line arguments

- **Severity:** Medium. **Confidence:** High.
- **Locations:** `bins/agentctl/src/main.rs:1140-1155,1266-1293,1342-1353`; `bins/agentctl/src/lib.rs:2462,2534,2782`; the last location reads `--org-key` in organization accept parsing. Existing safer input infrastructure: `bins/agentctl/src/vault.rs:315-353`.
- **Evidence / root cause:** `worker sign` requires `--secret <hex64>`; `org sync seal` accepts both `--secret` and `--org-key`, and `org sync accept` accepts `--org-key`. These raw values originate in `std::env::args` and stay in process arguments. The org-seal path retains them while awaiting two daemon requests, increasing exposure time. Generating the original key into a 0600 file does not protect the later argument-based consumption path.
- **Trust boundary / impact:** operator key input crosses into OS process metadata, terminal history when typed literally, and command-capture tooling. An observer permitted to inspect arguments or retained shell/automation history may recover a worker/account signing key or organization decryption key. Process-list visibility varies by OS policy; no claim that every other user can read it on every system is made. This is independent of debug-message redaction.
- **Reachability:** these are shipped local CLI command paths and documented usage, not test-only helpers. Vault password prompts already avoid this issue; they do not protect the worker/org commands.
- **Verification status:** confirmed source-level exposure. Real process arguments, terminal history and secret values were not inspected.
- **Safe reproduction:** run only an isolated fixture command with a public dummy 64-hex marker and capture its argv using a test process launcher, or test parsing via a fixture. Confirm that the marker is an argument; do not inspect unrelated running processes. After remediation, assert that only a file/descriptor selector appears in argv and that raw key bytes do not.
- **Recommended solution:** read keys through a protected file descriptor/file or the existing echo-disabled terminal input pattern. Keep raw secrets out of argv and diagnostics, retain zeroizing storage, and support noninteractive use without introducing a raw-key environment-variable replacement.
- **Estimated effort:** 1-2 days across worker sign, org seal/accept, usage text, and CLI tests.

## Dependency vulnerability and potential risk

- **F03, Medium:** production cryptography includes the advisory-affected `ctutils` 0.4.2. Confirmed dependency exposure, not demonstrated key recovery. [Full entry](06-dependencies.md#f03---affected-ctutils-is-in-production-cryptography).
- **F13, potential Medium:** root Tokio 1.51.4 includes an unsound AsyncFd API. No direct use of those constructors was found in application code; transitive use and invalid descriptor lifetime remain unproven. The desktop independently resolves Tokio 1.51.6. [Full entry](06-dependencies.md#f13---root-tokio-has-an-unsound-api-advisory).

## Controls verified in source

| Boundary | Evidence | Assessment and qualification |
|---|---|---|
| Inbound daemon IPC | `bins/agentd/src/lib.rs:434-442`; Unix accept pump; `peer.rs:60-108` | Kernel UID equality before request dispatch. `client_kind` is descriptive, not authority. F01 concerns the opposite direction |
| Framing/admission | `crates/ipc-transport/src/lib.rs:26-38`; `framing.rs:69-113,176-194`; `agentd/src/lib.rs:779-807` | 1 MiB frame cap, zero-frame refusal, partial-frame/write timeouts, eight active control clients. F04 shows this does not cap retained finished tasks |
| Tauri -> native | `apps/desktop/src-tauri/build.rs`; capabilities and all command bodies | Closed custom-command set, no arbitrary shell/path command; details in [01-tauri-security.md](01-tauri-security.md) |
| Policy -> process | `crates/runner/src/lib.rs:725-828`; `bins/agentd/src/execution.rs:934-995,3509-3596` | Exact process authority, explicit environment, runner-owned spawn; production requires confinement rather than silently using NativeRunner |
| Seatbelt | `adapters/runner-macos-seatbelt/src/lib.rs:103-247,273-333` | Deny-default profile, explicit paths/executables; literal escaping checks; limited network contract. Outbound TCP/443 is a port scope, not a destination-host/SSRF guarantee |
| Provider environment | `adapters/provider-claude/src/environment.rs`; `process.rs:239-276` | Private HOME/config/runtime paths; governed child environment cleared/rebuilt. Trusted-host test feature is explicit, not production fallback |
| Git execution | `adapters/git-cli/src/runner.rs:364-447,591-719`; metadata back-reference checks | Fixed argv, cleared environment, hooks/fsmonitor/helpers disabled, named executable config rejected, bounded capture/deadlines. Path races and every Git extension/version were not exhaustively tested |
| Candidate filesystem | `adapters/effect-capture-local/src/candidate_snapshot.rs:71-194,249-299,449-474,635-691,719-811` | Descriptor-relative opens, no-follow flags, regular-file checks, path/byte validation. No exploit or formal memory-safety proof claimed |
| SQLite | `adapters/storage-sqlite/src/path_security.rs`; `task_store.rs:53-79` | Private regular file/sidecar checks, inode revalidation, NOFOLLOW, foreign keys, bounded busy timeout. Service-level multi-commit defects remain F07/F15 |
| Vault | `adapters/secrets-vault/src/envelope.rs:52-83,181-275,366-434`; `backend.rs:253-324` | Argon2id, random salts/nonces, AEAD labels, bounds before derivation, zeroizing key/plaintext containers, private create-new/atomic replacement. No independent cryptanalysis/side-channel validation |
| Native keychain | `adapters/keychain-native/src/lib.rs:22-45,109-122` | Uses native `keyring` API, payload-free error mapping; no plaintext CLI argument transport in this adapter. Actual OS credentials were not accessed |
| Secret diagnostics | `crates/ipc-contracts/build.rs:9-22,45-51`; redacting Debug implementations; vault/credential error mapping | Secret-bearing protobuf Debug output is replaced, not indiscriminately logged. This is source evidence, not a scan of user logs |

## Unsafe, FFI, panics, and resource review

The unsafe scan identified platform-local peer/token queries, Windows pipe security helpers, Unix/Windows cycle locks, process-group signals, candidate descriptor I/O, and build-script environment setup. No handwritten `unsafe impl Send/Sync` or application `transmute` use was found in the searched runtime trees. Candidate FFI uses a manually declared Darwin `dirent` layout and OS constants; it merits platform-specific testing when changing SDK/platform support. Windows SID/token/impersonation code was only selectively read and not compiled or run on Windows.

Unwrap/expect candidates were evaluated by context. Examples such as `CString::new(".git").expect(...)`, UTF-8 conversion immediately after a checked UTF-8 path, and constant provider configuration are not reported as vulnerabilities. Many matches were test fixtures or fake adapters. No claim is made that every production panic candidate was proven unreachable; large planning/evaluation branches were sampled.

Provider cancellation normally owns process groups and bounds output collection (`adapters/provider-claude/src/process.rs:280-335,355-407,475-509`). That protection does not automatically extend to every other process owner: verification output finalization is F05, and widget shutdown ownership is F08. Existing environment/set-var test seams and build-script setup are not counted as exposed memory-corruption bugs.

Not checked: live sandbox escape attempts, Miri/ASan/fuzzing, Windows/Linux runtime enforcement, cross-user server impersonation, actual credential-store behavior, and cryptographic timing. No destructive exploit or production-state mutation was performed.
