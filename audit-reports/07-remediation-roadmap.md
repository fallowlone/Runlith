# Remediation roadmap

This is a proposed order for separate implementation work. The audit itself changes only reports. Estimates are engineering effort, not elapsed delivery promises; they include focused regression coverage but assume the existing architecture and an unrestricted macOS test environment.

## First: close authority and truthfulness gaps

| Order | Finding / smallest deliverable | Effort | Acceptance evidence |
|---|---|---|---|
| 1 | **F01:** authenticate the server principal in the shared Unix client connect path before protocol/secret traffic; retain private endpoint checks | 1-2 days | Same-user genuine server accepted; foreign server in a precreated shared-temp leaf rejected before marker-bearing business bytes; no cross-user confusion under replacement races |
| 2 | **F16:** stop producing successful Routine records without execution. Return an explicit blocked/unsupported result until a specified executor is connected | 0.5-1 day immediate safeguard | A due routine cannot become `COMPLETED` merely from a caller evidence label. Recording executor/receipt test proves either authorized work occurred or the run is explicitly blocked |
| 3 | **F02:** remove raw worker/account/org keys from CLI arguments, reusing protected-file/descriptor or hidden-input patterns | 1-2 days | All key-consuming commands work with synthetic material; captured argv/history-facing invocation contains selectors only; no raw keys in output/errors; noninteractive flow remains usable |
| 4 | **F03 + F13:** update root crypto primitive and Tokio resolution to patched compatible releases in an authorized dependency-only change | About 1 day combined | Both lockfile audits repeated; F03 vulnerability match gone; root Tokio patched; vault/worker/org/IPC contracts run on their real supported platform; no unrelated upgrades |

A full routine executor is a separate, bounded follow-up to the immediate F16 truthfulness fix. It needs a specified mapping from routine objective to Task/execution, policy and spend/resource budgets, and real observed evidence. Replacing no-op completion with an arbitrary unsandboxed shell would make the architecture less safe.

## Next: bound lifetimes and commit coherent state

| Order | Finding / smallest deliverable | Effort | Acceptance evidence |
|---|---|---|---|
| 5 | **F05:** bound capture after both normal child exit and timeout; own verifier descendants | 1-2 days | A short-lived wrapper with a bounded pipe-holding descendant returns within the total documented budget; timeout cleanup leaves no fixture-owned child and evidence records incomplete capture honestly |
| 6 | **F07:** one transaction for terminal routine status and budget/schedule/exhaustion; validate evidence before partial success | 1-2 days | Inject failure between proposed writes; reopen fixture store and observe both facts or neither. A one-run budget cannot authorize a second completion after crash/retry |
| 7 | **F08:** include widget work in daemon cancel/join ownership | About 1 day | In-flight synthetic widget call is cancelled and joined before shutdown returns; no paid/live provider required for the regression |
| 8 | **F04:** reap completed connection tasks in the live serve loop | 0.5 day | Sequential short connections do not grow retained JoinSet count after quiescence; active admission remains eight; task failures are observed |
| 9 | **F15:** atomically persist memory supersession state and graph link | 0.5-1 day | Forced link/storage failure rolls back the old state; successful result has both facts; retry does not get stuck on partial success |

Fix F07 before adding side-effecting routine execution. Its current bookkeeping inconsistency would become a duplicate-side-effect risk if an executor were added first. Keep existing Task state/journal atomicity, no-replay rules and per-execution cycle guards intact.

## Then: make read paths and verification coverage match the product

| Order | Finding / smallest deliverable | Effort | Acceptance evidence |
|---|---|---|---|
| 10 | **F06 + F11:** versioned, byte-aware Task-list pagination with a lightweight list projection and continuation-aware clients | 2-3 days plus up to 1 day query work | Two 600 kB objectives and a many-task fixture are fully retrievable through bounded frames; no silent truncation; old protocol clients receive explicit compatibility behavior; SQL query count no longer grows as seven queries per task |
| 11 | **F10:** offload synchronous dispatch/database work from the current-thread runtime, preserving lock order | 1-2 days | Holding a fixture database write lock does not stall independent ping/shutdown/expiry scheduling; mutation ambiguity/no-replay behavior stays correct |
| 12 | **F14:** add standalone Tauri Cargo checks/tests and binding consistency to CI | 1-2 days | CI actually enters `apps/desktop/src-tauri`; a host-only compilation/validation regression is caught; browser/demo tests remain identified as web-only evidence |
| 13 | **F09:** align desktop validation with canonical IDs | 0.5 day | Valid 128/129/256-byte project/task IDs round-trip through listing and dependent commands; invalid inputs still fail at the authoritative boundary |
| 14 | **F12:** shorten metric-state lock scope while preserving outbound serialization/rate limits | 0.5-1 day | Cached source B answers while source A's fixture fetch is stalled; one outbound fetch admitted at a time; fixed-host, ETag and rate-limit behavior unchanged |

The estimates overlap when work shares a boundary; do not mechanically sum every row or turn this list into one large refactor. State/protocol changes should be independently reviewable, and no remediation is authorized by this report alone.

## Required evidence before a clean audit claim

1. Re-run the existing full root suite on macOS with Unix/TCP fixture sockets and Seatbelt available. The current result is 1,858 passed, 118 failed, five ignored; resolve the 16 failed Cargo targets. Preserve the distinction between sandbox startup refusal and an independent state/assertion defect.
2. Run standalone host tests with local HTTP fixtures available; current result is 27 passed and nine fixture-bind failures. Keep binding exports intentional and review any generated-source diff.
3. Run Playwright and accessibility scenarios against the built frontend. Current tests did not start because the preview server could not bind. Separately test the native permission/origin boundary; demo mode is not native ACL evidence.
4. Obtain a real npm advisory result and a deliberate cargo-deny policy run when that tool is available. Do not install or update tooling as an incidental audit action. Keep both Cargo locks under advisory review.
5. Run each new fault/timeout/auth regression against its production owner rather than asserting that a manifest contains a string. A green test must demonstrate the failure scenario is prevented.
6. Before release, verify the actual signed/notarized macOS artifact, CSP/origin behavior and any future updater keys/endpoints. Do not infer release integrity from `bundle.active`, build success or source-only inspection.

## Retained design constraints

- Keep Tauri a thin adapter with a fixed command set; preserve the explicit `AppManifest::commands` registration and narrow main-window grants.
- Keep the 1 MiB transport cap, strict deserialization, path validation, private file admission, environment clearing and OS-isolation requirements.
- Keep Task/Execution/Attempt/Session identities separate. Provider text, Routine evidence strings, majority opinions and scanner exit codes do not grant Task completion.
- Keep private keys/passwords out of process arguments, ordinary logs, model context and durable Task data. Reuse the existing secret-input/broker patterns.
- Keep derived caches disposable and bounded by an explicit policy; do not add another mutable source of Task authority to optimize a read query.
- Retain source/build identity and failed/incomplete evidence. Do not replace today's uncertainty with a generic claim of "security verified".

## Report acceptance checklist

| Requested deliverable | Evidence |
|---|---|
| Repository, versions, crates/frameworks/plugins, manifests/locks/build scripts | [00](00-executive-summary.md), [04](04-architecture.md), [06](06-dependencies.md) |
| Every Tauri command and frontend invoke route; capabilities/scopes/CSP/network/storage | Complete command table and boundary review in [01](01-tauri-security.md) |
| Rust unsafe/process/error/concurrency/persistence review | [02](02-rust-security.md), [03](03-correctness-and-reliability.md), [05](05-performance.md); limits explicitly stated |
| Available static/dependency/frontend checks | Exact command/result ledger and all failed target names in [06](06-dependencies.md) |
| Per-finding severity/confidence/source/evidence/root cause/impact/status/fix/effort | Detailed entries F01-F16 in the relevant reports; global ranked register in [00](00-executive-summary.md) |
| Separate confirmed vulnerabilities, correctness defects, potential risks and opportunities | Four categories in [00](00-executive-summary.md), with cross-referenced canonical entries |
| Safe reproduction and no destructive tests/source fixes | Fixture-only procedures marked as proposed where not executed; application/source/lockfiles unchanged |
| Explicit incomplete coverage | Coverage table in [00](00-executive-summary.md) and limits in every topical report |

Not checked: the remediation implementations and future release/runtime validations above. All findings remain open in the source; the deliverable is the audit and its evidence, not fixes or a production security approval.
