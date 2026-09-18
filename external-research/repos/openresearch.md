# alphaXiv/OpenResearch

## Source revision

- Repository URL: `https://github.com/alphaXiv/OpenResearch`
- Repository: `alphaXiv/OpenResearch`
- Default branch: `main`
- Reviewed HEAD: `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`
- Project version: `0.2.3`
- Exact tag at reviewed HEAD: none found during revision freeze
- Review date: `2026-09-16`
- Review mode: source-level architecture/security/reliability analysis; no destructive exploitation; external source treated as knowledge only

All findings below are bound to the reviewed commit above.

## Executive summary

OpenResearch is a local-first research/agent application that combines a Rust CLI and local dashboard, durable SQLite state, Git worktrees, multiple coding-agent harnesses, and several external compute backends. The most valuable part for Runlith is not a missing feature. It is independent evidence for several choices Runlith already made: distinguish known-safe retry from uncertain delivery, keep application-owned durable state outside provider-native sessions, reconcile after uncertain external effects, isolate parallel agent work in separate worktrees, and make restarted supervisors derive behavior from durable local state plus provider state rather than from an in-memory controller.

The reviewed revision also contains authority patterns Runlith should deliberately reject. The Claude integration can turn one approved blocked action into a resumed `bypassPermissions` session because the upstream CLI permission modes are coarse. Dashboard-managed environment variables are propagated into autonomous harness children, creating ambient credential/environment authority. The normal dashboard deliberately treats loopback plus Host/Origin checks as its local trust boundary and exposes broad user-authority operations, including absolute-path file reads, without authenticating an OS principal. Those decisions are understandable for a single-user local app, but Runlith's G6/IPC architecture is already stricter.

No convincing `NOVEL_GAP`, `PARTIALLY_EXISTS`, or `NEEDS_ADR` item specific to OpenResearch survived the mandatory overlap search. The useful ideas are already designed or implemented in Runlith, while the weaker authority patterns are superseded or conflict with Runlith architecture. Therefore this review adds no OpenResearch item to `IMPLEMENTATION_KNOWLEDGE_QUEUE.md` and proposes no new ADR or TASK candidate.

## Project purpose

The repository implements a local research environment around experiments, papers, agent-assisted coding/research sessions, and external compute. It has two related orchestration planes:

1. interactive local agent sessions, driven through coding-agent harnesses such as Claude Code, Codex, OpenCode, and Cursor; and
2. experiment/run execution on local or external compute backends, monitored by detached supervisors.

The UI is served by `orx up`, while `orx serve` exposes a smaller read-only loopback run/log surface intended for local or SSH-tunnel use.

## Architecture map

```text
OpenResearch
├── Interfaces
│   ├── Rust CLI / command dispatch                  src/main.rs
│   ├── Local dashboard HTTP/SSE/WebSocket API      src/commands/up.rs
│   ├── Minimal run/log HTTP/SSE daemon             src/commands/serve.rs
│   └── Embedded UI                                 ui/
├── Orchestration
│   ├── Chat/session lifecycle                      src/local/chat/mod.rs
│   ├── Harness normalization/dispatch              src/local/harness/mod.rs
│   └── External run supervisor                     src/commands/supervise.rs
├── Domain
│   ├── projects / experiments / runs
│   ├── chat sessions / messages / turns / spawns
│   └── compute backend descriptors                 src/jobs/mod.rs
├── State/Persistence
│   ├── SQLite local store                          src/store.rs
│   ├── Git repositories and per-session worktrees  src/local/git.rs
│   ├── run log files
│   └── provider-native session state as resumable runtime state
├── Providers
│   ├── Claude Code                                 src/local/harness/claude.rs
│   ├── Codex                                       src/local/harness/codex.rs
│   ├── OpenCode                                    src/local/harness/opencode*.rs
│   ├── Cursor                                      src/local/harness/cursor.rs
│   └── compute: HF/K8s/Modal/SSH/Slurm/Ray/etc.    src/jobs/*
├── Tools/Capabilities
│   ├── chat shell / project file operations
│   ├── skills / templates / local models
│   ├── permission bridge                           /api/internal/permissions
│   └── synchronized environment configuration
├── Execution
│   ├── provider CLI children
│   ├── detached Git worktrees
│   └── local/remote compute jobs + supervisor
├── Recovery
│   ├── durable turn leases and reconciliation
│   ├── delivery-state-aware retry/continue
│   ├── native-session transcript reconstruction
│   └── provider handle recovery / inspect-before-retry
├── Security
│   ├── loopback Host/Origin/Sec-Fetch checks
│   ├── bridge gate tokens and stale-turn revalidation
│   ├── provider-native permission modes
│   └── environment/credential propagation boundaries
└── Observability
    ├── SQLite status
    ├── log files
    ├── SSE state/log events
    ├── telemetry
    └── version-stamped health endpoints
```

Business logic is concentrated in command/application modules rather than in the UI. `src/local/chat/mod.rs` owns much of interactive-session lifecycle and recovery, while external run lifecycle is split between command code, `Store`, backend modules, and `supervise.rs`.

## Canonical state ownership

The reviewed code does not have one universal store for every physical fact. The canonical application state for runs, experiments, projects, chat sessions/messages/turns, queues, wakeups, spawns, and leases is the local SQLite `Store`. Git remains authoritative for repository/worktree content and refs. External providers remain authoritative for whether an external job actually exists or completed. The persisted `BackendDescriptor` is the local link between a run and that provider-side resource.

Provider-native chat/session IDs are resumable runtime state rather than sufficient durable workflow truth. When native state disappears, OpenResearch reconstructs bounded prior context from its local message store instead of treating the provider session as the only continuity source (`src/local/harness/mod.rs:107-155`). This is strongly aligned with Runlith's `Task != Execution != Attempt != Session` and provider-native-state-is-non-authoritative rules.

The strongest recovery ownership signal appears in `DeliveryState` (`src/local/chat/mod.rs:2258-2281`). `NotSent` and `Rejected` are retryable, whereas `Accepted` and `Unknown` continue/reconcile rather than replay. This means uncertainty itself changes the legal recovery action.

Atomic boundaries are therefore deliberately local: SQLite transactions can make local state atomic, but SQLite, Git, provider jobs, child process effects, and network effects are separate failure domains. The source does not pretend that external compute submission and local persistence form one ACID transaction.

## Strong engineering decisions

### OR-F001 — Delivery-state-aware recovery

**source:** implementation

**repository:** `alphaXiv/OpenResearch`

**revision:** `325eb509dc8e4ca7074568cf0ae1f0f98704eac0`

**file:** `src/local/chat/mod.rs`

**symbol:** `DeliveryState`, `DeliveryState::recovery_action`

**region:** `2258-2281`

**observation:** Delivery is represented as `NotSent`, `Rejected`, `Accepted`, or `Unknown`. Only known-unsent/rejected states map to retry; accepted or uncertain delivery maps to continuation/reconciliation.

**confidence:** `HIGH`

**Problem:** blindly replaying a user message/tool-driving turn after a transport failure can duplicate side effects when the provider actually received it.

**Mechanism:** persist delivery state around the send boundary; let recovery behavior depend on whether non-delivery is known or outcome is uncertain.

**Why it works:** uncertainty is modeled as durable semantic state instead of being collapsed into a generic error.

**Limitations:** `Unknown` still requires downstream reconciliation/continuation logic to decide what happened; it does not prove the external effect.

**Failure modes:** incorrect adapter classification can still mark a truly accepted submission as retryable or vice versa.

**Complexity:** moderate, because every provider adapter must maintain the state transition correctly.

**Runlith overlap:** `ALREADY_DESIGNED` / partially implemented. G8 already requires durable intent, `AMBIGUOUS`, `RECONCILE`, and forbids blind replay when the external result is uncertain. `INV-066` and roadmap G8 encode the stronger general rule.

**Decision:** `EXTERNAL_VALIDATION`.

**Differentiation:** Runlith applies this beyond model-message transport to general privileged/side-effect execution at the Attempt/Runner/Reconciler boundary, with Policy and evidence provenance.

### OR-F002 — Restart-idempotent external supervisor with inspect-before-retry semantics

**source:** implementation

**file:** `src/commands/supervise.rs`

**symbol:** `run`

**region:** `1-110`

**observation:** The supervisor is explicitly restart-idempotent. It reloads run state, takes a per-run supervisor lock, tries to recover a missing provider submission handle, and if the handle remains missing marks the run failed with instructions to inspect the provider before retrying (`59-77`) rather than resubmitting automatically.

**confidence:** `HIGH`

**Problem:** crash between remote submission and durable handle persistence creates an ambiguous external outcome.

**Mechanism:** reconstruct from local durable state plus provider/backend evidence; do not duplicate the external submission when ownership is uncertain.

**Why it works:** restart does not depend on the original process's memory, and ambiguous submission state becomes operator/reconciler work instead of duplicate execution.

**Limitations:** recovery quality depends on each backend's ability to rediscover a handle; missing provider-side idempotency/search metadata can still require manual inspection.

**Runlith overlap:** `ALREADY_DESIGNED` through durable Attempt state, Scheduler/Reconciler, G8 side-effect protocol, `AMBIGUOUS -> RECONCILE`, and the rule that external signals are evidence.

**Decision:** `EXTERNAL_VALIDATION`.

### OR-F003 — Provider-native session loss falls back to application-owned transcript

**source:** implementation

**file:** `src/local/harness/mod.rs`

**symbol:** `native_recovery_snapshot`, `native_recovery_context`

**region:** `107-155`

**observation:** If provider-native state cannot be resumed, the system reads locally stored chat messages, renders a bounded recovery snapshot, and instructs the new native session not to repeat completed tool actions. The snapshot is capped at 32 KiB (`src/local/harness/mod.rs:54-59,131-145`).

**confidence:** `HIGH`

**Problem:** opaque provider session loss should not erase application-owned conversation/work state.

**Mechanism:** provider-native state is an optimization; durable local history can bootstrap a new provider session.

**Limitations:** transcript reconstruction cannot reproduce hidden provider state exactly and therefore cannot prove semantic identity with native resume.

**Runlith overlap:** `ALREADY_DESIGNED`. Context Continuity is application-owned; provider sessions are evidence/derived execution state. Existing external research already identified a narrower future compatibility/invalidation contract in IKQ-002, so OpenResearch adds validation rather than a new queue item.

**Decision:** `EXTERNAL_VALIDATION`.

### OR-F004 — Per-session detached worktree isolation

**source:** implementation

**file:** `src/local/git.rs`

**symbol:** `ensure_session_worktree`, `ensure_worktree_from`

**region:** `1249-1329`

**observation:** Each chat session gets a private detached worktree from a baseline ref so parallel agents do not share one checkout. Existing unexpected/broken worktree state is preserved and surfaced rather than silently overwritten.

**confidence:** `HIGH`

**Problem:** concurrent agents operating in one checkout create accidental interference and unreviewable source state.

**Mechanism:** detached per-session worktree, shared Git object/ref store, later experiment branch creation.

**Limitations:** cleanup uses forced worktree removal and eventual `remove_dir_all` for session deletion (`1332-1349`), reflecting a product assumption that committed work is the durable artifact.

**Runlith overlap:** `ALREADY_IMPLEMENTED`, and Runlith's ownership model is stricter: autonomous execution owns a durable worktree lease/managed branch, dirty state cannot be silently destroyed, and SQLite/Git reconciliation is explicit (`INV-005`, `INV-145`, ADR-0023).

**Decision:** `EXTERNAL_VALIDATION`.

### OR-F005 — One harness abstraction with provider-specific semantics preserved

**source:** implementation

**file:** `src/local/harness/mod.rs`

**symbol:** `Harness`, `registry`

**region:** `246-330`, `481-490`

**observation:** Claude Code, Codex, OpenCode, and Cursor implement one harness interface for detection, turn execution, options, steering and one-shot operations, while the abstraction explicitly preserves provider-specific resume behavior rather than pretending all protocols are identical.

**confidence:** `HIGH`

**Problem:** a multi-provider product needs a stable application seam without discarding meaningful provider capabilities.

**Mechanism:** common trait and registry as extension point; provider adapters own protocol/event translation and prompt-resume semantics.

**Limitations:** a single broad trait can accumulate optional methods as provider capabilities diverge; application code must still understand some capability differences.

**Runlith overlap:** `ALREADY_DESIGNED` and substantially implemented through provider ports/adapters and the anti-corruption-layer model (ADR-0009). Runlith additionally keeps providers unable to mutate canonical Task state.

**Decision:** `EXTERNAL_VALIDATION`.

### OR-F006 — Immutable source snapshot metadata for delayed/restarted compute

**source:** implementation

**file:** `src/jobs/mod.rs`

**symbol:** `BackendDescriptor`

**region:** `46-95`

**observation:** External run descriptors can persist source archive digest, path, and size so a delayed or restarted supervisor is independent of the live mutable working tree.

**confidence:** `HIGH`

**Problem:** remote execution started later must not silently pick up a different local working tree than the one the run intended.

**Mechanism:** persist immutable snapshot identity/provenance next to the backend handle.

**Limitations:** the descriptor is a fixed union-like structure for all backends, creating coupling discussed below.

**Runlith overlap:** `ALREADY_DESIGNED`: Workspace/Attempt identity, source/worktree fingerprints, verification provenance, and immutable evidence binding already require exact revision/input identity.

**Decision:** `EXTERNAL_VALIDATION`.

### OR-F007 — Bounded retry/watchdog behavior

**source:** implementation

**file:** `src/local/harness/mod.rs`

**symbol:** `TURN_WATCHDOG`, `ORX_MAX_RETRIES`, `orx_retry_delay`

**region:** `48-59`, `157-175`

**observation:** turns have a 30 minute no-event watchdog, ORX retry count is capped at three, the retry time budget is capped, and jitter is deterministic per turn/retry number.

**confidence:** `HIGH`

**Runlith overlap:** `ALREADY_DESIGNED` and partly implemented via bounded retry/budget/liveness rules and typed failure/reconciliation policy.

**Decision:** `EXTERNAL_VALIDATION`.

## Implementation techniques

- **Persist uncertainty before transport.** Claude marks delivery `Unknown` before writing a message to the child (`src/local/harness/claude.rs:1841-1847`). Later evidence can move the state toward accepted/rejected; a crash in the gap does not silently become "not sent".
- **Use a per-run supervisor lock.** `supervise.rs:28-47` prevents multiple local supervisors from owning one run concurrently while still allowing restart.
- **Version health surfaces.** `orx serve` includes the package version in `/health` so a stale daemon is not mistaken for a compatible current process (`src/commands/serve.rs:89-99`).
- **Separate native protocol behavior from application state.** The harness layer explicitly models end-turn-and-resume vs inline-live-protocol behavior rather than forcing one fake interaction model (`src/local/harness/mod.rs:209+`).
- **Treat worktree creation failures as evidence.** Existing worktree state is checked and preserved on inconsistency instead of automatically deleting it (`src/local/git.rs:1291-1315`).
- **Use bounded recovery context.** Local transcript fallback selects newest entries up to a fixed byte ceiling rather than injecting unbounded history.

## AI / orchestration architecture

OpenResearch does not implement a single monolithic "agent loop" shared by all providers. It delegates the model/tool loop to provider-native coding agents and normalizes their observable lifecycle through the harness interface. This is a practical anti-corruption layer: the application owns sessions, messages, turn records, queues, prompts, delivery state and UI; the provider owns its native execution protocol.

**Who owns workflow state?** For interactive chat, `Store` plus `ChatHost` own durable session/message/turn/queue state. For experiments/runs, `Store` plus the detached supervisor own local lifecycle, with provider state treated as external evidence.

**Who owns completion authority?** Provider turn completion drives a chat turn outcome, but external run completion is derived from backend state and stored run transitions. This repository is not structured around Runlith's independent verification gate, so "provider completed a turn" is closer to product completion than Runlith would permit for an engineering Task.

**Can the model mutate canonical state?** Indirectly, yes: harness-driven tools and shell/file operations can alter project/worktree state and models can trigger product commands through their execution environment. The application still persists its own chat/run state, but model/provider actions have substantial operational authority.

**Can model output directly cause privileged action?** Provider-native tool calls can cause filesystem/shell/network actions according to provider permission mode and the application's bridge behavior. This is exactly where the Claude approval-widening finding matters.

**What happens after crash?** Chat has durable turn/lease reconciliation; native provider session loss can fall back to stored transcript; run supervisors are re-spawned for active local experiments during `orx up` startup and re-derive state from SQLite/provider evidence.

**What happens after provider timeout?** Harness code has bounded retries/watchdog behavior and typed delivery state. Uncertain delivery does not map to blind resend.

**How does execution resume?** Prefer provider-native resume when available; otherwise reconstruct bounded local context into a new native session. External compute resumes monitoring from persisted run/backend descriptor and backend inspection.

## Security analysis

### OR-SEC-001 — One blocked-action approval can widen to Claude `bypassPermissions`

**classification:** `CONFIRMED`

**severity:** `HIGH` as an authority-model weakness; practical impact is constrained by the provider CLI and the actions the resumed turn attempts.

**source evidence:** `src/local/harness/claude.rs:927-976`, especially `961-971`.

**observation:** when a `permission` prompt is approved, the default resumed Claude mode is `PermissionMode::Bypass`. The comments explicitly state that this is necessary because `acceptEdits` re-denies Bash/non-edit tools and `bypassPermissions` is the only upstream mode that lets the blocked tool through.

**failure scenario:** the user approves one specific blocked action, but the resumed provider session now runs under a coarse mode that can permit subsequent actions wider than the exact approved request until the mode changes/turn ends.

**Runlith overlap:** `CONFLICTS_WITH_ARCHITECTURE`. ADR-0026 / `INV-150` bind approval to the exact operation/request, capabilities, policy fingerprint, provenance and validity window. TASK-0062/0067 implement single-use consumption and reject widening/replay.

**adoption:** `REJECT`.

**Runlith lesson:** provider permission modes are compatibility hints/evidence. They cannot be the canonical authority representation. If an upstream provider only offers coarse bypass, Runlith must still enforce exact effective authority outside the provider.

### OR-SEC-002 — Autonomous harnesses inherit broad synced/shell environment

**classification:** `CONFIRMED`

**severity:** `HIGH` as a credential/ambient-authority boundary weakness.

**source evidence:** `src/config.rs:129-132,302-354`; `src/local/chat/mod.rs:7689-7706`.

**observation:** the code documents that `~/.openresearch/env` is fanned out to compute backends and agent environment. `prepare_env` exports imported shell environment and then dashboard-managed synced variables into provider children.

**failure scenario:** a token or credential placed in the synced/shell environment for convenience becomes available to an autonomous provider process or its spawned tools without an operation-scoped secret grant.

**Runlith overlap:** `SUPERSEDED` / `ALREADY_IMPLEMENTED` stronger protection. TASK-0065 clears inherited environment, applies an allowlist, isolates XDG/state/cache/scratch outside the worktree, and keeps explicit provider-owned auth anchors; Secrets Broker and Policy own broader secret authority.

**adoption:** `REJECT`.

### OR-SEC-003 — Normal dashboard uses loopback origin as the local trust boundary, not OS-principal authentication

**classification:** `CONFIRMED` trust-model property; exploitability beyond the intended local-user threat model is `PLAUSIBLE`, not proven here.

**severity:** `MEDIUM` architectural concern for a high-authority local control plane.

**source evidence:** `src/commands/up.rs:1-12,430-702`; `src/commands/up_remote.rs:1147-1245`; `src/commands/up.rs:3842-3975`.

**observation:** normal `orx up` binds `127.0.0.1`. Middleware validates loopback Host and, for unsafe methods/WebSockets, Origin/Sec-Fetch. The normal local mode has no session-bearer layer; bearer auth is added only for persistent remote-host mode. The local API includes project mutations, shell/chat actions, settings/secrets, compute actions, and `GET /api/files/abs` / `raw`, whose own documentation says loopback-only/no-auth and allows reading any file readable by the OpenResearch user.

**mitigations in source:** loopback bind, narrow Host validation (including protection against non-loopback Host/DNS-rebinding shapes), Origin/Sec-Fetch checks for unsafe methods/WebSockets, security response headers, separate token authentication for the internal permission bridge.

**failure scenario:** a local process or sandbox that can connect to loopback but does not otherwise have equivalent filesystem/application authority may attempt to use the dashboard as a confused deputy. The reviewed code proves the trust assumption and broad API authority; it does not prove that a specific lower-authority attacker context can exploit it on every supported platform.

**Runlith overlap:** `SUPERSEDED`. ADR-0020 authenticates local IPC peers by same-user OS identity (UID/SID) before protocol handshake and fails closed. Runlith still documents same-user malicious process limits, but loopback address alone is not its principal identity.

**adoption:** `REJECT` as a Runlith control-plane authority model.

### OR-SEC-004 — `orx serve` is an unauthenticated loopback read surface over run state/logs

**classification:** `CONFIRMED`

**severity:** `LOW` to `MEDIUM`, depending on sensitivity of local run metadata/logs and local process threat model.

**source evidence:** `src/commands/serve.rs:1-16,28-49,51-125`.

**observation:** the daemon binds `127.0.0.1`, parses GET requests directly, and serves run lists, run details, logs and SSE without an authentication mechanism in the reviewed handler.

**failure scenario:** another local process that can reach the port can read run metadata/log output while the daemon is active.

**Runlith overlap:** `SUPERSEDED` by authenticated local IPC and explicit data/security classification boundaries.

**adoption:** `REJECT` for privileged/sensitive Runlith state surfaces; a deliberately public low-sensitivity local diagnostics surface would need separate classification.

### OR-SEC-005 — Internal permission bridge does authenticate and revalidate stale requests

**classification:** `CONFIRMED` positive control

**source evidence:** `src/local/chat/mod.rs:2836-2911`; `src/commands/up.rs:7433-7458`.

**observation:** each child gets a gate token. Permission requests equality-check the current token, ensure the turn is still busy, serialize review per session, then revalidate token/turn state after waiting for the review lock.

**security value:** stale child requests cannot simply outlive a respawn/ended turn and still obtain an approval decision.

**limitation:** this authenticates the bridge request but does not cure the later coarse Claude `bypassPermissions` authority widening.

**Runlith overlap:** `ALREADY_IMPLEMENTED` in stronger form through exact approval request identity/binding, replay protection, durable single-use consumption and operation-boundary enforcement.

**decision:** `EXTERNAL_VALIDATION` for stale-request revalidation; not an adoption candidate.

### OR-SEC-006 — Shell/argument/path traversal vulnerabilities

**classification:** `NOT_SUPPORTED` as a general confirmed finding from the reviewed evidence.

Targeted inspection found multiple shell/Git/filesystem authority surfaces, but this review did not establish a concrete source-to-sink injection or traversal path meeting the requested confirmation bar. File-serving code uses canonicalization/containment checks in several project/artifact paths; absolute-file access is intentionally unrestricted by location and is therefore an authority design choice rather than path traversal. No destructive proof was attempted.

### OR-SEC-007 — Git trusted-host hardening weaker than Runlith

**classification:** `NOT_SUPPORTED` as an OpenResearch vulnerability claim.

The reviewed Git helper surface does not demonstrate Runlith's current G6/TASK-0071 defense set (cleared environment, external-diff/fsmonitor/credential-helper/hook suppression, metadata boundary validation), but absence of equivalent hardening in the inspected snippets is not enough to claim an exploitable OpenResearch Git vulnerability. For Runlith this is simply external contrast: its trusted-host Git boundary remains intentionally stricter.

## Reliability / recovery analysis

OpenResearch has several unusually good restart/recovery details for a product in this category.

- **Run supervisor restart:** active local experiment runs are rediscovered on dashboard startup and detached supervisors are spawned again. The supervisor itself uses local state + backend state and a per-run lock, so duplicate local watcher processes converge.
- **Ambiguous submission:** missing backend handle triggers recovery lookup; if still unknown, the run is failed with explicit provider-inspection guidance rather than auto-submitted again.
- **Chat delivery ambiguity:** the `DeliveryState` model separates safe resend from uncertain outcome.
- **Turn liveness:** durable turn leases are periodically reconciled, and provider waits have a watchdog.
- **Provider-session loss:** reconstruct from application-owned transcript rather than losing continuity.
- **Worktree consistency:** broken/mismatched worktree conditions are surfaced rather than always reconstructed destructively.

The main reliability concern in persistence is schema migration discipline.

### OR-REL-001 — Broadly ignored errors in best-effort `ALTER TABLE` migrations

**source evidence:** `src/store.rs:591-630`.

**observation:** a large list of `ALTER TABLE ... ADD COLUMN` migrations intentionally ignores every execution error because duplicate-column failures are expected on re-run. The same file later uses a transactional `PRAGMA user_version` migration for a transcript-tree transformation (`637-674`).

**impact:** ignoring all errors can mask failures other than "duplicate column", leaving schema drift that surfaces later in unrelated queries. The code itself demonstrates a more reliable pattern for nontrivial migration state immediately afterward.

**Runlith overlap:** `CONFLICTS_WITH_ARCHITECTURE` / `REJECT`. Runlith persistence changes should have explicit versioning, typed failure, deterministic migration evidence and verification rather than broad error swallowing.

## Performance / scalability analysis

The system is optimized primarily for a local single-user workflow, not horizontal orchestration scale.

Useful performance techniques include bounded recovery context, detached worktrees sharing one Git object store, concurrent log tailing with provider-state polling in the supervisor, and SSE diff streams rather than rebuilding every client view from scratch.

Scalability pressure appears in central modules and backend coupling. `src/local/chat/mod.rs` and `src/commands/up.rs` combine many lifecycle/UI/API concerns, which raises review and regression cost as provider/tool surfaces expand. External compute dispatch in `supervise.rs:79-99` is a string-kind switch, while `BackendDescriptor` is one fixed struct containing optional fields for every backend (`src/jobs/mod.rs:46-95`). Each new backend therefore changes shared persistence shape and central dispatch.

For Runlith this is contrast rather than a missing feature. Provider-neutral ports/adapters and domain-owned lifecycle are already intended to keep backend-specific fields/behavior at adapter boundaries.

## Anti-patterns

### OR-AP-001 — Exact user approval translated into coarse provider-wide bypass

**Evidence:** `src/local/harness/claude.rs:961-971`.

**Immediate impact:** one approved blocked action can resume under a mode capable of allowing additional actions.

**Long-term impact:** authorization semantics become constrained by the weakest/coarsest provider-native mode; audit cannot equate "user approved X" with the actual effective provider authority.

**Runlith lesson:** exact approval remains application/Policy state and must be enforced independently of provider-native permission vocabulary.

**Existing Runlith protection:** ADR-0026, TASK-0062, TASK-0067, `INV-148`, `INV-150`.

### OR-AP-002 — Synced user environment used as autonomous execution capability

**Evidence:** `src/config.rs:129-132,328-353`; `src/local/chat/mod.rs:7689-7706`.

**Immediate impact:** autonomous providers inherit environment variables unrelated to the exact operation.

**Long-term impact:** credentials become difficult to scope, rotate and audit; adding a new synced variable silently expands many child processes.

**Runlith lesson:** child environment is a capability surface. Start from empty/allowlisted state and deliver secrets explicitly.

**Existing Runlith protection:** TASK-0065, Secrets Broker, Runner/G6 authority contracts.

### OR-AP-003 — Loopback network location treated as sufficient identity for a broad local control plane

**Evidence:** `src/commands/up.rs:1-12,430-702`; `src/commands/up_remote.rs:1147-1245`; `src/commands/up.rs:3868-3975`.

**Immediate impact:** any local caller satisfying the loopback HTTP guard is inside a broad application authority boundary in normal local mode.

**Long-term impact:** as the local API gains more secrets, shell, compute and filesystem powers, the assumption "loopback caller == user-authorized principal" becomes increasingly consequential.

**Runlith lesson:** principal authentication and transport locality are separate properties.

**Existing Runlith protection:** ADR-0020 same-user OS peer authentication + endpoint permissions.

### OR-AP-004 — Fixed union-like backend descriptor plus central kind switch

**Evidence:** `src/jobs/mod.rs:46-95`; `src/commands/supervise.rs:79-99`.

**Immediate impact:** each backend adds optional fields to shared persisted state and branches to central orchestration.

**Long-term impact:** provider-specific persistence and lifecycle semantics leak into a common module, increasing migration/coupling cost and making replacement less local.

**Runlith lesson:** provider/backend-specific handles belong behind typed adapter-owned contracts; core state should store provider-neutral references/evidence required by lifecycle.

**Existing Runlith protection:** ADR-0009 provider anti-corruption layer, provider-neutral Task/Execution/Attempt state.

### OR-AP-005 — Best-effort migrations ignore every DDL error

**Evidence:** `src/store.rs:591-630`.

**Immediate impact:** non-duplicate migration failure can be silently accepted.

**Long-term impact:** installations can diverge in schema while continuing until a later path fails far away from migration startup.

**Runlith lesson:** migration idempotency should recognize expected prior-state conditions explicitly; unexpected migration failure is not a no-op.

**Existing Runlith protection:** persistence/versioning discipline and verification requirements; no OpenResearch-specific Runlith gap found.

### OR-AP-006 — Large central lifecycle/API modules

**Evidence:** responsibility spread visible in `src/local/chat/mod.rs`, `src/commands/up.rs`, `src/store.rs`, and `src/commands/supervise.rs`.

**Immediate impact:** unrelated features share compilation/review/change surfaces.

**Long-term impact:** provider/security/recovery changes become harder to reason about independently; architectural boundaries are more likely to become conventions inside one large module than enforced package seams.

**Runlith lesson:** keep durable domain authority in application/domain services and push provider/UI/storage mechanics through explicit ports. This is maintainability evidence, not a security vulnerability.

## Runlith overlap analysis

| External idea / finding | External evidence | Closest Runlith equivalent | Runlith evidence | Current status | Actual gap | Decision |
|---|---|---|---|---|---|---|
| Delivery-state-aware retry vs uncertain outcome | `chat/mod.rs:2258-2281` | G8 Side-Effect Execution Protocol | roadmap G8; Source of Truth `AMBIGUOUS -> RECONCILE`; `INV-066` | `ALREADY_DESIGNED` | None attributable to OpenResearch | `EXTERNAL_VALIDATION` |
| Restart-idempotent provider supervisor | `supervise.rs:1-110` | Attempt/Scheduler/Reconciler recovery | task-engine/events/workspace; G8 | `ALREADY_DESIGNED` | None | `EXTERNAL_VALIDATION` |
| Native provider session fallback to owned transcript | `harness/mod.rs:107-155` | G9 Context Continuity | `continuity.md`, provider-session non-authority, existing IKQ-002 | `ALREADY_DESIGNED` | OpenResearch adds no new delta beyond existing IKQ-002 | `EXTERNAL_VALIDATION` |
| Per-session detached Git worktree | `local/git.rs:1249-1329` | WorkspaceService worktree lease/managed branch | ADR-0008, ADR-0023, `INV-005`, `INV-145` | `ALREADY_IMPLEMENTED` | None; Runlith is stricter on ownership/cleanup | `EXTERNAL_VALIDATION` |
| Multi-provider harness registry | `harness/mod.rs:246-330,481-490` | Provider ports / anti-corruption layer | ADR-0009, providers.md | `ALREADY_DESIGNED` / implemented seam | None | `EXTERNAL_VALIDATION` |
| Immutable source snapshot provenance for delayed compute | `jobs/mod.rs:87-94` | Workspace/revision fingerprints + Attempt provenance | workspace/context/evidence architecture | `ALREADY_DESIGNED` | None | `EXTERNAL_VALIDATION` |
| Bounded retry/watchdog | `harness/mod.rs:48-59,157-175` | bounded retry/budget/liveness | `INV-050`, `INV-091`, planning/execution contracts | `ALREADY_DESIGNED` | None | `EXTERNAL_VALIDATION` |
| Gate-token stale request rejection | `chat/mod.rs:2836-2911` | exact approval request/binding/replay protection | ADR-0026, POLICY_CONTRACT, TASK-0062/0067 | `ALREADY_IMPLEMENTED` | None; Runlith stronger/durable | `EXTERNAL_VALIDATION` |
| Approval via `bypassPermissions` | `claude.rs:961-971` | exact scoped/single-use approval | ADR-0026, `INV-148`, `INV-150` | `CONFLICTS_WITH_ARCHITECTURE` | No gap; external mechanism is weaker | `REJECT` |
| Synced ambient env into provider children | `config.rs:129-132,328-353`; `chat/mod.rs:7689-7706` | provider environment isolation + Secrets Broker | TASK-0065; provider environment tests; G6 | `SUPERSEDED` | None | `REJECT` |
| Loopback-only unauthenticated broad local API | `up.rs`, `up_remote.rs`, absolute file routes | OS-authenticated local IPC | ADR-0020; THR-003; ipc.md | `SUPERSEDED` | None | `REJECT` |
| Fixed backend descriptor/switch | `jobs/mod.rs:46-95`; `supervise.rs:79-99` | provider-neutral core + adapters | ADR-0009; providers.md | `SUPERSEDED` | None | `INSPIRED_ONLY` as contrast |
| Broad ignored migration errors | `store.rs:591-630` | typed/versioned persistence evolution | storage architecture/verification discipline | `CONFLICTS_WITH_ARCHITECTURE` | None | `REJECT` |

## External validation of existing Runlith design

OpenResearch provides especially useful independent validation in five areas.

1. **Ambiguous effects must not be replayed as ordinary retry.** Its `DeliveryState::Unknown -> continue` and missing-handle supervisor behavior demonstrate the same failure class Runlith calls `AMBIGUOUS -> RECONCILE`.
2. **Provider-native continuity is replaceable.** The transcript recovery path shows a real product benefit from keeping durable conversation state outside provider-native sessions.
3. **Parallel agents need checkout isolation.** Per-session detached worktrees independently validate Runlith's stronger execution worktree ownership model.
4. **Provider abstraction must preserve real capability differences.** OpenResearch's harness contract does not pretend Claude's end-turn resume and OpenCode's inline protocol are the same. This supports Runlith's provider anti-corruption layer rather than lowest-common-denominator provider semantics.
5. **External execution needs immutable provenance.** Persisting source snapshot digest/path/size next to delayed compute reinforces Runlith's fingerprint/evidence binding rules.

The most useful negative validation is equally strong: OpenResearch's coarse provider permission workaround and ambient environment propagation demonstrate why Runlith keeps Policy, effective capability, Secrets Broker and Runner authority outside provider-native settings.

## Novel gaps

None found after mandatory Runlith overlap search.

Several ideas looked potentially useful before overlap analysis, but all resolved to `ALREADY_DESIGNED`, `ALREADY_IMPLEMENTED`, `SUPERSEDED`, or `CONFLICTS_WITH_ARCHITECTURE`. The existing IKQ-002 provider-native resume compatibility item is already narrower and more complete than anything OpenResearch newly implies, so this repository does not justify a duplicate queue item.

## Adoption matrix

| Idea | Status | Adoption | Why Runlith's version is preferable |
|---|---|---|---|
| Delivery-state-aware recovery | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | generalized to all side effects, durable Attempt/Reconciler state, evidence and Policy |
| Restart-aware external supervision | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | provider-neutral lifecycle and explicit reconciliation ownership |
| Native-session transcript fallback | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | provider-neutral Context Continuity with compatibility/invalidation work already identified |
| Detached worktree per agent session | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` | durable ownership, branch/worktree reconciliation, fail-safe cleanup |
| Harness abstraction | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | provider adapters cannot mutate Task truth; authority remains separate |
| Immutable source snapshot | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | revision/input/evidence fingerprints integrate with verification |
| Claude bypass after exact approval | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | exact scoped single-use approval remains enforceable outside provider mode |
| Ambient synced env | `SUPERSEDED` | `REJECT` | cleared/allowlisted environment and scoped Secrets Broker |
| Loopback as principal identity | `SUPERSEDED` | `REJECT` | OS-user peer authentication and explicit IPC authority |
| Fixed backend union/switch | `SUPERSEDED` | `INSPIRED_ONLY` | adapter-local backend details, provider-neutral core contracts |
| Best-effort migration error swallowing | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | explicit/versioned migrations with observable failure |

## Ideas rejected for Runlith

- Translating an exact human approval into a provider-wide permission bypass.
- Treating provider permission mode as the canonical security decision.
- Inheriting arbitrary dashboard/shell environment into autonomous provider processes.
- Treating loopback TCP location as sufficient principal authentication for privileged Runlith control-plane operations.
- Exposing arbitrary user-readable local files through an unauthenticated privileged control-plane surface.
- Growing a single persisted backend union and central dispatch switch as the primary provider extension mechanism.
- Ignoring all migration DDL errors in order to make duplicate-column reruns convenient.
- Letting a provider-native session become the only durable recovery state.

## Candidate ADRs

None.

No OpenResearch finding remained `NOVEL_GAP`, `PARTIALLY_EXISTS`, or `NEEDS_ADR` after overlap analysis. Existing Runlith IKQ/ADR work already covers the only nearby unresolved topic: provider-native resume compatibility/invalidation.

## Candidate implementation tasks

None.

No `TASK-CANDIDATE` is justified by this repository at the reviewed revision.

## Source evidence index

### OR-F001 — Delivery-state-aware recovery

- `src/local/chat/mod.rs:2258-2281` — delivery states and retry/continue mapping.
- `src/local/harness/claude.rs:1841-1847` — persist `Unknown` before writing to Claude child.
- Confidence: `HIGH`.

### OR-F002 — Restart-idempotent supervisor / ambiguous submission recovery

- `src/commands/supervise.rs:1-6` — explicit restart-idempotence contract.
- `src/commands/supervise.rs:37-77` — local state reload, supervisor lock, submission-handle recovery, inspect-before-retry failure path.
- Confidence: `HIGH`.

### OR-F003 — Provider-native recovery from owned transcript

- `src/local/harness/mod.rs:107-155` — bounded snapshot and recovery context.
- `src/local/harness/mod.rs:48-59` — watchdog/retry/snapshot bounds.
- Confidence: `HIGH`.

### OR-F004 — Per-session worktrees

- `src/local/git.rs:1249-1329` — detached session worktree creation and preservation on inconsistent state.
- `src/local/git.rs:1332-1349` — deletion cleanup policy.
- Confidence: `HIGH`.

### OR-F005 — Provider harness abstraction

- `src/local/harness/mod.rs:246-330` — `Harness` interface and explicit capability differences.
- `src/local/harness/mod.rs:481-490` — single registry containing Claude, Codex, OpenCode, Cursor.
- Confidence: `HIGH`.

### OR-F006 — Immutable source snapshot in backend descriptor

- `src/jobs/mod.rs:46-95` — fixed descriptor and source digest/path/size provenance.
- Confidence: `HIGH`.

### OR-F007 — Bounded retry/watchdog

- `src/local/harness/mod.rs:48-59` — 30m watchdog, three retries, 15s retry budget, 32KiB recovery snapshot.
- `src/local/harness/mod.rs:157-175` — bounded deterministic-jitter retry delay.
- Confidence: `HIGH`.

### OR-SEC-001 — Approval widening

- `src/local/harness/claude.rs:927-976`, especially `961-971` — approved blocked permission defaults to `PermissionMode::Bypass` because upstream modes are coarse.
- Confidence: `HIGH`.

### OR-SEC-002 — Ambient/synced environment propagation

- `src/config.rs:129-132` — comment explicitly states synced env is fanned out to compute backends and agent environment.
- `src/config.rs:302-354` — parsing of synced environment.
- `src/local/chat/mod.rs:7689-7706` — export shell and synced env into provider child.
- Confidence: `HIGH`.

### OR-SEC-003 — Loopback local trust boundary and absolute file authority

- `src/commands/up.rs:1-12` — normal dashboard described as loopback-only; remote persistent mode separately adds bearer.
- `src/commands/up_remote.rs:1147-1245` — Host/Origin/Sec-Fetch loopback guard.
- `src/commands/up.rs:430-702` — broad local API surface and middleware composition.
- `src/commands/up.rs:3842-3975` — arbitrary absolute user-readable file access explicitly documented as loopback-only/no-auth.
- Confidence: `HIGH` for the trust model and endpoint authority; `MEDIUM` for concrete attacker exploitability outside that intended threat model.

### OR-SEC-004 — Unauthenticated `orx serve` read surface

- `src/commands/serve.rs:1-16,28-49,51-125` — loopback bind and GET routes without auth logic in handler.
- Confidence: `HIGH`.

### OR-SEC-005 — Gate-token stale-request protection

- `src/local/chat/mod.rs:2836-2911` — token check, live-turn check, review lock, revalidation.
- `src/commands/up.rs:7433-7458` — permission endpoint delegates to token-checked handler.
- Confidence: `HIGH`.

### OR-REL-001 — Best-effort migrations

- `src/store.rs:591-630` — broad ignored `ALTER TABLE` errors.
- `src/store.rs:637-674` — contrasting transaction + `user_version` migration.
- Confidence: `HIGH`.

### OR-ARCH-001 — Backend coupling

- `src/jobs/mod.rs:46-95` — fixed union-like descriptor with backend-specific optional fields.
- `src/commands/supervise.rs:79-99` — central string-kind dispatch across backend implementations.
- Confidence: `HIGH`.

### Supplemental repository evidence

- `.github/workflows/ci.yml` — UI install/typecheck/tests, Rust fmt/clippy/build/test/release checks and cross-platform coverage were inspected as CI quality evidence.
- `src/store.rs` — SQLite schema includes durable chat/run lifecycle records, wakeups, spawns and leases; this supports the canonical-state conclusion alongside chat/supervisor code.

