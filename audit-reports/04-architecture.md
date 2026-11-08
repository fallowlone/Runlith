# Architecture and assurance

Runlith currently has two separately resolved Rust workspaces: the 49-package local runtime workspace and one standalone Tauri host. That separation limits desktop coupling but creates an independent verification/dependency boundary. Persistent Task authority belongs to trusted application services and SQLite; frontend and provider responses are not equivalent to completion evidence.

## Current package inventory

All names below were read from current Cargo manifests, not inferred from the older generated AGENTS map. All 50 packages declare Rust edition 2021. A package appearing here does not mean every implementation line received detailed inspection.

| Area | Cargo packages |
|---|---|
| Binaries (2) | `agentd`, `agentctl` |
| Core lifecycle/planning/policy (9) | `task-domain`, `agent-runtime`, `scheduler`, `plan-domain`, `evaluation`, `workspace`, `policy`, `runner`, `app-services` |
| IPC (3) | `ipc-contracts`, `ipc-control`, `ipc-transport` |
| Local state/features (11) | `secrets`, `security-audit`, `repo-finder`, `context-engine`, `continuity`, `takeover`, `memory`, `background`, `worker`, `org`, `widget-spec` |
| Shared conformance libraries (11) | `execution-contract`, `effect-capture-contract`, `planner-contract`, `secret-backend-contract`, `repo-finder-contract`, `context-engine-contract`, `continuity-contract`, `memory-contract`, `background-contract`, `worker-contract`, `org-contract` |
| Product identity (1) | `product-version` |
| Adapters (12) | `storage-sqlite`, `fake-agent`, `fake-planner`, `provider-claude`, `fake-evaluator`, `git-cli`, `effect-capture-local`, `keychain-native`, `secrets-vault`, `runner-native`, `runner-macos-seatbelt`, `finder-local` |
| Separate desktop workspace (1) | `runlith-desktop` |

The 35 shared packages comprise the five shared-package rows above. React/Vite is the implemented desktop frontend; planned cloud/UI concepts in older documents should not be counted as extra shipped services. The untracked pixel-world prototype is an experiment, not the Tauri application.

## Responsibility, state, and I/O map

| Component / source | Input and responsibility | Filesystem / process / network / persistent state |
|---|---|---|
| React app, `apps/desktop/src/lib/api.ts` | User navigation, rendering, widget input; 12 fixed custom invoke routes | Preferences/demo layout in localStorage; no direct shell/Git/DB execution |
| Tauri bridge, `src-tauri/src/agentd.rs` | Typed command validation, negotiated daemon reads and widget jobs | Unix socket; opens a fresh session per call, never starts agentd |
| Tauri dashboard/metrics, `dashboard.rs`, `live_metrics.rs` | Desktop-owned layout and disposable widget values | App-data `dashboard.json`, corrupt backup, `metric-cache.json`; fixed GitHub HTTPS API; no Task writes |
| CLI, `bins/agentctl/src/main.rs`, `lib.rs`, `vault.rs` | Local operator control, connect-or-start lifecycle, typed requests, key/sealing helpers | Sibling daemon spawn, local socket, protected key files, terminal secret input; worker/org raw argv keys are F02 |
| Daemon, `bins/agentd/src/lib.rs` | Endpoint ownership, peer admission, protocol routing, service composition, supervision | Per-user Unix endpoint; current-thread Tokio runtime; eight active control clients; shared SQLite handles |
| Application services, `crates/app-services` | Trusted Task/runtime/workspace/policy, continuity, memory and workflow integration | Domain ports plus explicit concrete SQLite integration. `app-services/Cargo.toml` intentionally depends on `storage-sqlite`; it is not a pure domain crate |
| Policy / Runner | Decide authority, narrow capabilities, bind exact process/workspace/environment, enforce required axes | Seatbelt deny-default on macOS; NativeRunner explicitly has no OS confinement and cannot satisfy strict production requirements |
| Provider adapter, `adapters/provider-claude` | Convert neutral execution/planning/structured requests to CLI calls, bounded capture/cancellation | Explicit argv and child environment; private provider runtime directories; brokered credentials; provider owns its external inference connection |
| Git / capture adapters | Manage isolated worktree identity and derive immutable candidate/effect evidence | Trusted executable, metadata-anchor/config checks; descriptor-relative candidate I/O and sandboxed candidate helper re-execs |
| SQLite adapter | Durable state, migrations, journal/audit/effects and feature stores | Private database/sidecars, 38 checked-in migration files, foreign keys, transactions, hash-chain evidence, per-execution OS advisory locks |
| Secrets backends | Store/release scoped material rather than putting plaintext into Task state | Native credential store and encrypted file vault; inspected without opening user credentials |
| Worker / organization modules | Local contracts for devices, signed challenges, jobs, membership/policy, encrypted signed envelopes and usage | SQLite-backed local models/IPC. No deployed cloud transport or public remote-worker listener was found |

No blanket process sandbox is inferred for the daemon itself. Trusted host-side Git and filesystem operations still require careful input/metadata validation. A real security filter, network host policy, or remote identity service must not be assumed merely because an architecture document names one.

## Background ownership map

| Work | Current owner | Shutdown/recovery behavior |
|---|---|---|
| IPC sessions | `JoinSet` + semaphore in `agentd/src/lib.rs` | Aborted/joined on shutdown; completed tasks are not reaped while serving (F04) |
| Ordinary provider runs | `ExecutionComposition.background_runs`, cancel registry and process tracker | Cancel all, then join; finished handles pruned on spawn |
| Widget authoring | Separate detached thread and `WidgetAuthoring::Jobs` | Explicit request cancellation only; missing daemon wind-down ownership (F08) |
| Vault idle lock | Tokio task, `try_lock` + timer | Task aborted, vault explicitly locked during wind-down |
| Takeover expiry | Tokio task over Human Decision service | Watch-channel exit/abort |
| Routine scheduling | One-second Tokio task over `BackgroundService` | Watches shutdown; bookkeeping is immediate, with no executor invocation (F16) and non-atomic completion accounting (F07) |
| Finder scan | Blocking-pool request task | Offloaded from Tokio; aborting a waiting async task does not inherently cancel already-started blocking work |
| Advisory verification | Synchronous runner inside owned execution thread | Strict production sandbox; output-finalization time is not fully bounded (F05) |

## Trust boundaries and invariants

The reviewed code generally preserves `Task != Execution != Attempt != Session`, keeps normal UI reads out of Task mutation logic, and isolates provider-specific details in the Claude adapter. Domain Cargo manifests do not generally import infrastructure adapters; the explicit application-service/SQLite composition is consistent with the repository's local navigation policy.

Important boundaries are WebView -> Tauri custom command; local process -> daemon; daemon -> untrusted repository/CLI; capability decision -> physical process launch; runtime observation -> durable verification evidence; plaintext secret -> brokered child environment; and local signed/encrypted domain data -> any future remote transport. [02-rust-security.md](02-rust-security.md) lists the evidence for existing controls.

The accepted threat model explicitly excludes a same-user malicious process from the cross-user peer-auth guarantee. Consequently, self-reported authorizer names or local `client_kind` strings are not reported here as remote authentication vulnerabilities. These local claims must be revisited before enabling a remote multi-user transport.

Architecture documents include future browser/document/security-worker/cloud concepts. This audit did not find deployed implementations for all of them and does not certify their future security. Historical audit findings were treated as leads; current code was checked before repeating any claim. In particular, current production verifier construction requires strict sandbox axes (`execution.rs:3778-3787`), so NativeRunner test fixtures were not misreported as a production fallback.

## Architectural improvement opportunity

### F14 - CI does not verify the standalone Tauri host

- **Severity:** Medium assurance gap. **Confidence:** High.
- **Locations:** `apps/desktop/src-tauri/Cargo.toml:9-13`; root `Cargo.toml:1-54`; `.github/workflows/verification.yml` jobs `verify`, `verify-windows`, and `verify-desktop`, especially the desktop job's npm-only steps; `apps/desktop/playwright.config.ts:3-12`.
- **Evidence:** the Tauri package declares its own workspace and is absent from root members. Core Cargo check/test/Clippy therefore do not build it. Desktop CI runs npm build, data tests, and Playwright against browser demo data; it does not run Cargo in `apps/desktop/src-tauri`. Its tests cannot prove native ACL, host deserialization, Rust HTTP-client or dashboard persistence behavior.
- **Root cause:** verification composition mirrors only one half of the intentionally split desktop architecture.
- **Realistic impact:** a native command can stop compiling or lose security validation while the current core and frontend jobs remain green. Generated bindings can also drift from host types without a native/binding check. This is a coverage weakness, not proof of a current ACL exploit or CI compromise.
- **Verification status:** current CI and workspace manifests inspected; this audit independently compiled/linted the missing host. No GitHub Actions run was triggered or claimed.
- **Safe reproduction:** examine a fixture branch with a type error confined to the standalone host and enumerate current CI commands; none enters that Cargo workspace. Do not introduce such an error into this working tree.
- **Recommended solution:** add a macOS host job for locked Cargo check/Clippy/test and binding consistency, with ordinary local sockets available. Add a focused negative command-permission/origin test on the native surface where feasible. Keep browser/demo tests, but describe their scope accurately. Isolate export output or check generated bindings intentionally rather than silently rewriting source during an audit.
- **Estimated effort:** 1-2 days; packaged-origin automation may need an additional focused follow-up depending on available Tauri test support.

## Other architectural observations

`bins/agentd/src/execution.rs` is 9,132 lines including tests, with its first test module starting at line 5,107. It owns candidate Git plumbing, sandbox preparation, provider execution, evidence parsing, recovery and request dispatch. This is a review/ownership concentration, not by itself a vulnerability. The concrete lifecycle/transaction defects above are a better remediation target than an immediate broad refactor. When those fixes touch this file, extracting an existing responsibility behind its current contract would reduce review scope; no new abstraction is justified merely to lower a line count.

The root and desktop lockfiles resolve different Tokio versions, which demonstrates why both need independent advisory scans. The storage adapter's unsupported non-Unix admission fails closed, and the host explicitly rejects non-Unix compilation. Windows cycle-lock CI is scoped to that primitive, not a claim that the entire product works on Windows. Platform support should be reported at that same precision.

Not checked: all architectural branches line by line, deployed cloud/worker protocols, OS service registration, full Windows/Linux product behavior, plugin sandboxing not implemented in this tree, and independent multi-model review. No subagents or external model reviewers contributed to this audit.
