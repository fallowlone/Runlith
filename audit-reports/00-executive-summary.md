# Runlith security and code quality audit

What this repo does: Runlith is a local Rust control plane for durable engineering tasks, provider execution, policy, isolated Git workspaces, and verification. A React/Tauri desktop application reads daemon state and authors bounded dashboard widgets. This audit assumes one OS user, one daemon, a desktop window plus occasional CLI clients, and untrusted repository/provider content.

**Verdict: remediate the client-side IPC authentication gap and false routine completion first. Compilation and static lint checks pass, but runtime security assurance is incomplete in this restricted environment.** No application source, tests, dependencies, or product versions were changed.

## Snapshot and scope

- Audit date: 2026-11-03.
- Repository: `<repo>`.
- HEAD: `75b176dba3b450c2c573ac8c7763c3e654ba896e`, branch `fix/planned-child-objective-and-attempt-verdict`.
- Working tree: no tracked changes at entry; pre-existing untracked pixel-world prototype files and a historical probe checkpoint were left alone.
- Current root workspace: **49 packages**, comprising 35 shared crates, 12 adapters, and two binaries. The desktop host is a separate, 50th Rust package/workspace. All declare edition 2021. Older generated repository maps reporting 40 packages are stale.
- Desktop: Tauri **2.12.5**, tauri-build 2.7.4, React/React DOM 19.3.1, TypeScript 7.0.2, Vite 8.3.5, `@tauri-apps/api`/CLI 2.12.4. No application-installed Tauri plugin was found.
- Tracked inventory: 883 files, including 377 Rust files, 38 SQL migrations, 14 Protobuf files, 44 TypeScript files, and 20 TSX files. Inventory is broader than detailed line-by-line inspection.
- Task contract: the attached audit request supplies the scope and eight required reports; no TASK ID or Execution Envelope was provided. Its Markdown-only instruction takes precedence over implementation-result/version-bump conventions.

## Ranked finding register

Severity reflects the stated scenario, not a CVSS score. Confidence distinguishes proof of a code path from proof of an exploit. Detailed entries contain source locations, root cause, impact, reproduction, remediation, and effort. Cross-references reuse IDs instead of counting the same defect twice.

### Confirmed vulnerabilities

| Rank / ID | Severity | Confidence | Finding | Evidence level / detail |
|---|---|---|---|---|
| 1 / F01 | High | High | Unix clients do not authenticate the daemon before sending secrets; a foreign endpoint in a shared temp parent can impersonate it | Confirmed source path; cross-user exploit not run. [Rust security](02-rust-security.md#f01---unix-clients-trust-an-unauthenticated-server) |
| 3 / F02 | Medium | High | Worker/account private keys and organization keys enter CLI process arguments | Confirmed argument parsing; no real secret/process inspection. [Rust security](02-rust-security.md#f02---private-keys-are-command-line-arguments) |
| 4 / F03 | Medium | High for affected dependency; unproven exploitability | `ctutils` 0.4.2 is in the production cryptographic dependency graph and is covered by RUSTSEC-2026-0385 | Audit and dependency tree confirmed; timing/key recovery not tested. [Dependencies](06-dependencies.md#f03---affected-ctutils-is-in-production-cryptography) |

### Confirmed correctness defects

| Rank / ID | Severity | Confidence | Finding | Detail |
|---|---|---|---|---|
| 2 / F16 | High | High | Background routines report completion without executing their objective | [Correctness](03-correctness-and-reliability.md#f16---routines-report-completion-without-executing-work) |
| 5 / F05 | Medium | High | A naturally exiting verifier can leave pipe readers blocking beyond its timeout | [Correctness](03-correctness-and-reliability.md#f05---verification-timeout-does-not-bound-output-finalization) |
| 6 / F07 | Medium | High | Routine completion and budget/schedule changes are separate commits | [Correctness](03-correctness-and-reliability.md#f07---routine-completion-and-budget-accounting-are-not-atomic) |
| 7 / F08 | Medium | High for ownership defect | Widget provider threads are outside daemon shutdown cancellation/join ownership | [Correctness](03-correctness-and-reliability.md#f08---widget-provider-work-is-not-owned-by-daemon-shutdown) |
| 8 / F06 | Medium | High | Unpaged list replies can exceed the fixed 1 MiB transport cap | [Correctness](03-correctness-and-reliability.md#f06---valid-task-data-can-make-project-listing-unreadable) |
| 9 / F04 | Medium | High | Completed connection tasks accumulate until daemon shutdown | [Correctness](03-correctness-and-reliability.md#f04---completed-ipc-tasks-are-never-reaped-during-service) |
| 13 / F09 | Low | High | Desktop IDs are limited to 128 bytes while the domain accepts 256 | [Tauri security](01-tauri-security.md#f09---the-desktop-rejects-daemon-valid-identifiers) |
| 14 / F15 | Low | High | Memory supersession can persist state without its provenance link, then reject retry | [Correctness](03-correctness-and-reliability.md#f15---memory-supersession-is-not-one-transaction) |

These are confirmed by source analysis. They are not presented as dynamically reproduced exploits. The existing test suite is useful supporting evidence, not a regression test for every new finding.

### Potential risks requiring verification

| Rank / ID | Severity | Confidence | Finding | Detail |
|---|---|---|---|---|
| 10 / F13 | Medium if affected API misuse is reachable | High for version match; low for application exploitability | Root Tokio 1.51.4 has the AsyncFd I/O-safety advisory; direct application use was not found | [Dependencies](06-dependencies.md#f13---root-tokio-has-an-unsound-api-advisory) |

### Architectural and performance improvement opportunities

| Rank / ID | Severity | Confidence | Finding | Detail |
|---|---|---|---|---|
| 11 / F10 | Medium | High for blocking path; latency unmeasured | Synchronous database/dispatch work blocks the single Tokio runtime thread | [Performance](05-performance.md#f10---synchronous-service-work-blocks-all-daemon-connections) |
| 12 / F14 | Medium assurance gap | High | CI checks the web layer and core workspace but omits the standalone Rust/Tauri host | [Architecture](04-architecture.md#f14---ci-does-not-verify-the-standalone-tauri-host) |
| 15 / F11 | Low | High for query shape; latency unmeasured | Task lists hydrate full domain records through at least seven queries per task | [Performance](05-performance.md#f11---desktop-task-listing-hydrates-unneeded-domain-data) |
| 16 / F12 | Low | High for serialization; latency unmeasured | The live-metric state mutex is held across network waits and disk synchronization | [Performance](05-performance.md#f12---one-slow-metric-blocks-cached-metric-reads) |

## Verification status

Successful checks: both Rust workspaces passed all-target compilation, all-target/all-feature Clippy with `-D warnings`, and format checks. The frontend passed its type check, production build, and all 17 data tests. A deterministic command inventory matched all 12 frontend command names to Rust declarations, build-time ACL command registration, and explicit capability grants. Generated ACL output was also inspected.

The desktop host test run completed with **27 passed and 9 failed**. Every failure was at the local HTTP fixture's `TcpListener::bind`, with `Operation not permitted`; no assertion about an HTTP response was reached. Its binding exports were redirected to `/private/tmp/runlith-audit-bindings-75b176d` to preserve checked-in TypeScript files.

The root full test run finished with **1,858 passed, 118 failed, and five ignored**, with **16 Cargo targets failing**. Direct failures include denied socket binding and Seatbelt startup; downstream execution/recovery assertions remain unresolved until rerun on an unrestricted test machine. The full ledger and target list are in [06-dependencies.md](06-dependencies.md#verification-command-ledger). These failures must not be interpreted as verified application vulnerabilities or as a clean test run. Playwright could not start its preview server (`listen EPERM ::1:1420`). npm audit could not resolve the npm registry. cargo-deny is absent and was not installed. Cargo advisory scans used the available local database; they are not a complete supply-chain certification.

## Coverage and limits

| Area | Work performed | Limit |
|---|---|---|
| Repository/build graph | All 50 package manifests inventoried; both Cargo locks, npm lock, build scripts, CI, entry points, and key architecture/security contracts examined | Not every historical planning/audit document was read |
| Tauri | All 12 commands, all frontend invoke routes, complete host modules' production paths, config, capability, generated permission declarations/ACL, and widget schemas reviewed | No packaged WebView/origin-denial/signing/notarization test |
| IPC | Unix resolution, endpoint checks, peer identity, framing, admission, handshake/response paths, dispatch and shutdown traced | Live cross-user impersonation was not attempted; Windows token/pipe code was inspected selectively, not run |
| Rust runtime | Process supervision, Runner/Seatbelt construction, provider environment, candidate descriptor/FFI paths, vault envelope/storage, SQLite admission/migration and selected service transactions reviewed | No Miri, fuzzing campaign, model checking, side-channel test, or OS sandbox escape test |
| Domains | Task lifecycle/IDs, policy and completion ownership, routine, memory, worker/org crypto and persistence seams, context/continuity interfaces and contract tests sampled | Large planning/workspace/evaluation implementations received targeted review, not exhaustive line-by-line proof |
| Performance | Query counts, bounds, synchronization and blocking traced from source | No release benchmark, RSS measurement, workload replay, startup/throughput/binary-size claim |
| Other content | Prototypes, docs, scripts, and fixture inventories identified | Untracked pixel-world experiment and historical probe artifacts are not shipped runtime surfaces; no claim of a complete audit of their JavaScript/art/render tooling |

The reports prioritize reachable issues over the number of findings. No remote code execution, Tauri shell-command escape, or direct application memory-corruption exploit was demonstrated. No production database, vault, credentials, or real provider inference was used for exploit testing.

## Report navigation

1. [Tauri security and command inventory](01-tauri-security.md)
2. [Rust security and trust boundaries](02-rust-security.md)
3. [Correctness and reliability](03-correctness-and-reliability.md)
4. [Architecture and coverage](04-architecture.md)
5. [Performance](05-performance.md)
6. [Dependencies and verification evidence](06-dependencies.md)
7. [Prioritized remediation roadmap](07-remediation-roadmap.md)

Not checked: production deployment integrity, live providers, other operating systems, and the dynamic scenarios explicitly marked unverified above. Unfixed findings remain in the audited source because this task authorizes reports only.
