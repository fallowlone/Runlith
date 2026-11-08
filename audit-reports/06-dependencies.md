# Dependencies and verification evidence

Both independently resolved Cargo lockfiles were inspected and scanned. The root lock contains an advisory-affected cryptographic dependency; the desktop Cargo advisory command exits successfully but still reports informational warnings. npm advisory coverage is incomplete because registry DNS access failed. No tools were installed and no dependency or lockfile was updated.

## Environment and resolution boundaries

| Item | Observed value |
|---|---|
| Audit platform | macOS 27.2, ARM64 / `aarch64-apple-darwin` |
| Rust toolchain | `rustc 1.95.0 (59807616e 2026-04-14)`, LLVM 22.1.2 |
| Root workspace | 49 Rust packages, edition 2021; `Cargo.lock` has 266 package entries, including local packages |
| Desktop workspace | Separate `apps/desktop/src-tauri/Cargo.toml` / lock, edition 2021; 456 lock entries |
| Tauri | `tauri` 2.12.5; build/codegen/macros 2.7.4; runtime 2.12.2, runtime-wry 2.12.3, utils 2.10.2 |
| Frontend | React/React DOM 19.3.1; Vite 8.3.5; TypeScript 7.0.2; Tauri JS API/CLI 2.12.4 |
| npm lock | Lockfile v3; 91 package entries including root and platform-optional packages; every non-root entry has integrity and a registry.npmjs.org resolved URL |
| Available audit tools | `cargo-audit` installed; `cargo-deny` absent; npm installed |
| Local advisory database | 1,403 advisories, commit `6c5fedf8ff03c485ed674d8f941e0180f3f72d70`, timestamp `2026-11-03T10:44:19+01:00` |

The dependency/database timestamp is provenance for the local scan, not a claim that a live fetch succeeded. Cargo audit warned that it could not open the crates.io index because `~/.cargo/.package-cache` could not be locked in this environment. Advisory matching still completed against the local database; index-dependent/yanked-package checks are incomplete.

## Confirmed dependency vulnerability

### F03 - Affected ctutils is in production cryptography

- **Severity:** Medium for this application assessment; no upstream CVSS score is invented. **Confidence:** High that the affected package is locked and in the production graph; exploitation is unproven.
- **Locations:** `Cargo.lock:614-620` pins `ctutils` 0.4.2; `adapters/secrets-vault/Cargo.toml:17-21`; `adapters/secrets-vault/src/envelope.rs:16-18,78-83,372-400`; cryptographic dependencies in `crates/org/Cargo.toml` and `crates/worker/Cargo.toml`.
- **Evidence:** root `cargo audit --no-fetch` exits 1 and reports **RUSTSEC-2026-0385**. `cargo tree --locked --offline -i ctutils --target aarch64-apple-darwin -e normal` confirms production paths through `digest` -> Argon2/Curve25519/Ed25519 and through `universal-hash` -> Poly1305 -> ChaCha20-Poly1305 -> vault/organization encryption.
- **Root cause / upstream issue:** the locked package predates the corrected constant-time mask/comparison implementation. The official advisory identifies version 0.4.4 as the minimum patched version. [RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2026-0385.html).
- **Boundary / realistic impact:** secret-dependent cryptographic operations rely on timing properties of a vulnerable transitive primitive. Key leakage would require an applicable affected operation and a useful timing/covert observation channel; neither was established for Runlith on this CPU. The 32-bit-specific portion is not automatically an ARM64 exploit.
- **Verification status:** affected version and normal macOS dependency graph confirmed; no assembly analysis, timing experiment or secret-recovery attempt. A dependency finding is not a proven application exploit.
- **Safe reproduction:** rerun the lockfile audit and inverse dependency tree. These demonstrate exposure without using a real vault, signing key or provider token. After a separately authorized update, repeat audits and vault/organization/worker contract tests.
- **Recommended solution:** resolve the transitive package to an upstream patched release, at least 0.4.4, through a dedicated dependency change. Keep cryptographic regression/format tests; do not suppress the advisory or replace the crypto design to avoid a small update.
- **Estimated effort:** 0.5 day plus normal cryptographic contract checks; no migration is implied by this patch recommendation.

## Potential risk requiring verification

### F13 - Root Tokio has an unsound API advisory

- **Severity:** Potential Medium if invalid-descriptor use is reachable. **Confidence:** High for affected version; low for an exploitable application path.
- **Locations:** `Cargo.lock:2133-2145` pins Tokio 1.51.4; root Tokio constraints in `bins/agentd/Cargo.toml` and `crates/ipc-transport/Cargo.toml`. The desktop lock separately resolves **1.51.6**.
- **Evidence / root cause:** root cargo audit reports informational **RUSTSEC-2026-0386**, concerning the safe AsyncFd constructors' descriptor-lifetime contract. The official advisory lists patched 1.51.6 within the 1.51 series and newer patched lines. [RustSec advisory](https://rustsec.org/advisories/RUSTSEC-2026-0386.html).
- **Trust boundary / impact:** invalid, replaced or prematurely closed descriptors passed through the affected API can violate I/O safety. Searching application Rust found no direct `AsyncFd`/`tokio::io::unix` usage. This does not rule out transitive dependency use or prove misuse.
- **Verification status:** version match confirmed; no direct call found; no use-after-free was demonstrated. This remains a potential dependency/API risk, not a confirmed Runlith memory-corruption vulnerability.
- **Safe verification / reproduction limits:** trace transitive AsyncFd owners on each supported target and validate descriptor lifetimes. An application exploit reproduction cannot be specified honestly until such a call path is found. Independently update to a patched compatible release and rerun IPC/peer tests on an environment with real sockets. Do not manufacture invalid OS handles in the user's daemon.
- **Recommended solution:** update the root lock to a patched compatible Tokio version and maintain both lockfiles' advisory coverage. The desktop's patched resolution does not protect the separate daemon.
- **Estimated effort:** 0.5 day for resolution/checks; additional time only if an actual transitive misuse is found.

## Other advisory output, without inflated severity

| Lockfile | Package / version | Advisory / type | Assessment |
|---|---|---|---|
| Root | `zbus_names` 4.3.4 | RUSTSEC-2026-0371, unmaintained | Maintenance warning; inverse normal-dependency tree for current macOS target is empty |
| Root | `zvariant` 5.15.0 | RUSTSEC-2026-0370, unmaintained | Target-specific dependency maintenance risk; no exploit claimed |
| Root | `zvariant_derive` 5.15.0 | RUSTSEC-2026-0372, unmaintained | Macro/dependency maintenance risk; no exploit claimed |
| Desktop | `proc-macro-error` 1.0.4 | RUSTSEC-2024-0370, unmaintained | Build-time maintenance warning, not a known runtime exploit |
| Desktop | `glib` 0.18.5 | RUSTSEC-2024-0429, unsound `VariantStrIter` iterator | Current macOS inverse normal-dependency tree is empty; Linux/other target applicability was not dynamically tested |

The corresponding local advisory files were read for ctutils/Tokio; the remaining rows reproduce the scan's package/type/ID output. No direct application `VariantStrIter` call was found. Unmaintained and informational-unsound advisories do not automatically mean the shipped macOS app is exploitable. Successful desktop audit exit does not mean zero warnings or comprehensive application safety.

## Verification command ledger

Commands below show the underlying programs; execution used the repository's `rtk` wrapper. Root commands ran from `<repo>`; host commands from `apps/desktop/src-tauri`; frontend commands from `apps/desktop`. `--locked --offline` prevented dependency resolution changes and used already available dependencies. Test/build output and temporary fixtures are verification artifacts; only the eight Markdown reports were added to the working tree.

| Scope | Command | Result |
|---|---|---|
| Root Rust | `cargo check --workspace --all-targets --locked --offline` | PASS, exit 0 |
| Root Rust | `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` | PASS, exit 0; proc-macro/ts-rs attribute notices remain, not promoted Clippy lints |
| Root Rust | `cargo fmt --all --check` | PASS, exit 0 |
| Root binaries | `cargo build --locked --offline --bin agentd --bin agentctl` | PASS; performed before process tests so sibling executables were current |
| Root Rust | `cargo test --workspace --locked --offline --no-fail-fast` | FAILED, exit 101: **1,858 passed, 118 failed, 5 ignored**, aggregated over 215 test-summary groups; **16 Cargo targets failed** |
| Host Rust | `cargo check --all-targets --locked --offline` | PASS, exit 0; ts-rs notices about Serde attributes |
| Host Rust | `cargo clippy --all-targets --all-features --locked --offline -- -D warnings` | PASS, exit 0; same attribute notices |
| Host Rust | `cargo fmt --all --check` | PASS, exit 0 |
| Host Rust | `env TS_RS_EXPORT_DIR=/private/tmp/runlith-audit-bindings-75b176d cargo test --lib --locked --offline` | FAILED, exit 101: **27 passed, 9 failed**; all nine fail at HTTP-fixture socket bind. All tests, including export tests, were attempted; no test was skipped to hide a failure |
| Frontend | `npm exec --no -- tsc --noEmit` | PASS, exit 0 |
| Frontend | `npm run build` | PASS, exit 0; type checking and Vite production build |
| Frontend | `npm run test:data` | PASS, exit 0; **17/17** tests |
| Frontend | `npm run test:e2e -- --output=/private/tmp/runlith-audit-playwright-75b176d` | BLOCKED/failed, exit 1 before tests: preview server `listen EPERM ::1:1420` |
| Root advisories | `cargo audit --no-fetch` | FAILED, exit 1: one vulnerability (F03), four informational warnings, index-lock limitation |
| Host advisories | `cargo audit --no-fetch` | Exit 0: two informational warnings, index-lock limitation; no vulnerability-class match reported |
| Root policy scanner | `cargo deny check` | UNAVAILABLE, exit 101: `no such command: deny`; tool not installed |
| Frontend advisories | `npm audit --json --fetch-retries=0 --fetch-timeout=20000` | BLOCKED/failed, exit 1: `getaddrinfo ENOTFOUND registry.npmjs.org`; npm also could not write its normal user log directory |
| Dependency reachability | `cargo tree --locked --offline -i ctutils --target aarch64-apple-darwin -e normal` | PASS; production graph confirmed |
| Target filtering | Same tree form for desktop `glib` and root `zbus_names` | Exit 0 with `nothing to print` for current macOS normal graph |
| Tauri inventory | Read-only assertion over command declarations, build manifest, frontend call names and capability grants | PASS: all 12 matched; generated ACL and handler registration also inspected |

### Root test failure accounting

The complete terminal output was recovered through RTK recall and the totals above were calculated from its 215 `test result` lines; the suite reached its terminal exit. It was not stopped early. Failures occurred in these targets:

| Package | Failing target |
|---|---|
| `agentctl` | `--lib` |
| `agentctl` | `--test approval_control` |
| `agentctl` | `--test business_no_replay` |
| `agentctl` | `--test execution_binaries` |
| `agentctl` | `--test execution_composition` |
| `agentctl` | `--test execution_recovery` |
| `agentctl` | `--test finder_composition` |
| `agentctl` | `--test real_process_matrix` |
| `agentctl` | `--test task_core_matrix` |
| `agentctl` | `--test widget_control` |
| `agentd` | `--lib` |
| `agentd` | `--test execution_recovery` |
| `agentd` | `--test live_ownership` |
| `agentd` | `--test planning_integration` |
| `ipc-transport` | `--lib` |
| `runner-macos-seatbelt` | `--lib` |

Direct environmental evidence includes `EndpointSecurity("cannot bind ... permission denied")`, `TcpListener::bind: Operation not permitted`, and `sandbox-exec: sandbox_apply: Operation not permitted`. Many process tests subsequently fail "daemon never became ready"/`StartupTimeout`. Candidate processes also return exit 71 with no stdout, consistent with sandbox startup refusal. Some execution/recovery/credential tests assert `SUCCEEDED`/`VERIFYING` or a later credential classification but see `FAILED`/`RUNNING` or an earlier refusal instead. Their failure output alone does not prove an independent product defect, and **they remain unresolved until rerun with sockets/Seatbelt available**; they are not silently counted as passing or all individually exonerated.

All root failures were confined to the 16 targets above; the other target summaries passed or were ignored. In particular, passing domain/storage/service contract tests do not prove absent integration defects: F07/F15 concern fault windows the current tests do not cover, and F16's tests currently verify bookkeeping behavior. Five pre-existing ignored tests were not enabled; live-provider smokes were not run.

### Diagnostic notices and tools

Host commands emitted a pre-existing shell initialization notice from RVM (`operation not permitted: ps`). Successful command exits were retained; that shell notice is not a Rust compilation failure. ts-rs emitted unsupported-Serde-attribute notices; actual Rust validation was inspected separately and remained present. Cargo-deny absence, registry DNS failure, kernel denial and actual code findings are distinct outcomes.

The requested Agent Reach search route could not run because its `mcporter` executable is absent. Official Tauri/Tokio/RustSec references were retrieved with the available web tool instead. No CLI, browser, advisory database or package installation was performed to bypass an environmental restriction.

## Supply-chain limits

Lockfile checksums and npm integrity fields improve reproducibility but do not establish that source/build scripts are benign. No full transitive dependency-source audit, license-policy certification, provenance/signature verification of artifacts, malicious-package scan, complete yanked-package check, or remote npm advisory result was obtained. Native macOS/Windows update signatures and installer integrity require real release artifacts and are outside the available evidence.

Not checked: current npm vulnerability status, cargo-deny policy results, transitive AsyncFd misuse, crypto timing, production packaging integrity and platform-specific dependency behavior outside macOS. Do not interpret unavailable scanners as a clean dependency bill.
