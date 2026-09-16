# Tencent/BrowserSkill

## Source revision

- Repository URL: `https://github.com/Tencent/BrowserSkill`
- Repository: `Tencent/BrowserSkill`
- Default branch: `main`
- Reviewed HEAD: `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`
- Exact tag at reviewed HEAD: `ext-v0.3.0`
- Review date: `2026-09-16`
- Review mode: source-level architecture/security/reliability analysis; no destructive exploitation; external source treated as knowledge only

All findings below are bound to the reviewed commit above.

## Executive summary

BrowserSkill is a browser-agent execution stack rather than a thin browser CLI. The reviewed revision combines a Rust `bsk` CLI/daemon and typed protocol, a Chrome extension/controller, task-scoped Agent Windows, session-local observation references, a compact semantic observation layer (VOM), a DSH agent plugin, authenticated remote browser-device connections, cancellation/recovery semantics, and local operation audit.

The strongest ideas for Runlith are two browser-specific contracts that are not yet explicit in Runlith's deferred Web Acquisition architecture:

1. **Browser target ownership as an explicit temporary capability.** BrowserSkill distinguishes task-created tabs from user-owned tabs, requires an explicit borrow operation before a user tab becomes controllable, records its original location, returns it during cleanup, and refuses to treat mere window placement or opener relationships as authority.
2. **Actionable element references bound to fresh observation/document identity.** BrowserSkill's session-local ref store replaces the ref set on each observation, invalidates tab refs when the document changes, and refuses visual refs without verified document/node identity. This is a concrete stale-reference defense beyond Runlith's existing provenance/freshness rule for browser snapshots.

These survive the mandatory overlap search as `NEEDS_ADR` and `PARTIALLY_EXISTS`. They should inform the already-required dedicated Web Acquisition/Browser ADR rather than create a competing browser architecture.

Several other mechanisms independently validate Runlith's existing design: cancellation linearized against side-effect dispatch, typed unknown outcomes instead of blind retry, local audit that does not claim business correctness, bounded retries, provider/plugin session ownership, progressive disclosure of browser tools, scoped credentials, and browser actions that remain distinct from task completion authority.

The main architecture/security lessons are equally useful. BrowserSkill intentionally gives a paired remote server broad authority over the paired browser profile, including signed-in website sessions. Its navigation path validates session/window scope but does not perform a centralized scheme/IP/redirect SSRF/network-policy evaluation before navigation. It also exposes arbitrary page JavaScript evaluation on an authorized tab. These choices fit BrowserSkill's product boundary, but Runlith should retain its stricter Policy/Secret Broker/NetworkAccessPolicy model and reject broad browser-profile trust as canonical authority.

## Project purpose

BrowserSkill gives an AI agent a browser-facing tool protocol with task-isolated browser windows and compact observations. It can run locally or connect an extension to a remote `bsk` daemon/gateway. The extension owns Chrome-specific behavior; the Rust daemon/CLI owns transport, session routing, cancellation, remote authorization, audit and command-line UX; the DSH plugin exposes the browser capability to an agent runtime.

It is not a durable general workflow engine. The reviewed code has no Runlith-like durable Task/Execution/Attempt graph or independent completion authority. Browser sessions are operational runtime state, and remote reconnection explicitly starts new tasks rather than replaying old sessions (`docs/remote-extension-connection.md:168`).

## Architecture map

```text
BrowserSkill
├── Interfaces
│   ├── Rust CLI / daemon                         crates/bsk-cli/
│   ├── typed RPC/protocol contracts             crates/bsk-protocol/
│   ├── Chrome extension + popup/audit UI        apps/extension/
│   └── DSH agent plugin                         packages/dsh-plugin-browserskill/
├── Orchestration
│   ├── daemon IPC -> per-session queue -> WS
│   ├── extension tool dispatcher
│   └── DSH lazy tool/session coordinator
├── Domain
│   ├── browser session / Agent Window
│   ├── controlled / borrowed tab ownership
│   ├── session-local observation refs
│   └── browser RPC outcomes/effect state
├── State/Persistence
│   ├── daemon/browser/session runtime maps
│   ├── extension SessionManager + RefStore
│   ├── remote authorization snapshots
│   └── append-oriented local audit JSONL
├── Providers
│   └── browser driver: Chromium CDP + WebExtension APIs
├── Tools/Capabilities
│   ├── navigation/read/observe/click/type/scroll
│   ├── screenshot / recording
│   ├── arbitrary JS evaluate on authorized tabs
│   ├── tab create/borrow/return
│   └── DSH lazy browser tool registration
├── Execution
│   ├── task-scoped Agent Window
│   ├── per-session serialized RPC dispatch
│   └── browser-side CDP/WebExtension operations
├── Recovery
│   ├── cancel-vs-dispatch linearization
│   ├── explicit effect_state = committed/unknown/none
│   ├── borrow rollback / return compensation
│   └── bounded session-stop cleanup
├── Security
│   ├── tab/session ownership checks
│   ├── optional user confirmation for borrowing
│   ├── authenticated remote device pairing
│   ├── credential rotation/revocation
│   └── local operation audit/redaction
└── Observability
    ├── operation audit start/finish records
    ├── daemon/extension errors
    ├── typed RPC reasons/effect state
    └── VOM semantic observation/provenance
```

## Canonical state ownership

BrowserSkill does not have one durable canonical workflow state owner because it is an execution capability rather than a workflow engine.

For a live browser task, the extension `SessionManager` owns the session-to-Agent-Window mapping, controlled/borrowed-tab sets and borrow reservations (`apps/extension/src/session-manager/manager.ts:94-213`). The browser itself remains authoritative for actual tabs/windows/document state. The `RefStore` is explicitly session-local derived observation state, replaced/invalidateable as documents change (`apps/extension/src/session-manager/ref-store.ts:35-115`).

The Rust daemon separately owns IPC/WS inflight correlation, per-session dispatch/cancellation and remote browser-device routing. Remote device authorization is durable server state; browser/session commands are not replayed across a remote disconnect. The DSH plugin also keeps its own process-local set of sessions that it created and refuses to adopt foreign session IDs (`packages/dsh-plugin-browserskill/src/sessions.ts:132-193`).

This split is reasonable for BrowserSkill's scope, but it should not be imported as Runlith task ownership. In Runlith, browser/session/controller state must remain execution-side derived/runtime state below the durable Task -> Execution -> Attempt hierarchy (`INV-001`, `INV-002`, `INV-070`, `INV-105`).

## Strong engineering decisions

### BSK-F001 — Explicit browser target ownership and reversible user-tab borrowing

**finding_id:** `BSK-F001`

**source:** implementation + protocol documentation

**repository:** `Tencent/BrowserSkill`

**revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**file:** `apps/extension/src/session-manager/manager.ts`; `apps/extension/src/tools/tabs.ts`; `apps/extension/src/tools/session.ts`; `docs/remote-extension-connection.md`

**symbol:** `SessionManager::tryReserveBorrow`, `validateBorrowTarget`, `requestBorrowApproval`, `handleTabBorrow`, `handleSessionStop`

**region:** `manager.ts:159-213`; `tabs.ts:631-911`; `session.ts:173-193,225-358`; remote docs `162-168`

**observation:** A task controls tabs it created plus user tabs explicitly borrowed into its Agent Window. A user tab already placed in the Agent Window is not automatically authorized in remote mode. Cross-session double borrow is reserved/serialized before awaited browser work. Borrow records original window/index, can require browser-side human approval, rolls back on cancellation where possible, and records an unknown outcome if rollback cannot be confirmed. Session stop returns borrowed tabs before closing task-created tabs; failures keep the Agent Window open so user state is not destroyed. Opener/popup relationships do not automatically grant authority.

**confidence:** `HIGH`

**Problem:** Browser automation mixes application-created resources with pre-existing user state. Treating location, focus or opener relationships as ownership can let an agent operate on the wrong user tab or destroy user state during cleanup.

**Mechanism:** Explicit resource ownership, reservation before asynchronous mutation, exact borrow operation, optional human confirmation, recorded origin, compensating return, and fail-visible unknown outcomes.

**Why it works:** Authority is represented by an application-owned claim rather than inferred from where a browser object happens to be. Cleanup distinguishes task-created state from user-owned state.

**Limitations:** The claim is runtime state; BrowserSkill does not provide Runlith-style durable Attempt recovery for a machine/process restart. A failed physical move/rollback can still leave an ambiguous browser outcome that must be reconciled.

**Failure modes:** browser/tab disappearance during approval, move succeeds but bookkeeping commit fails, original window disappears before return, or cleanup fails after user state has moved.

**Complexity:** moderate. Correctness requires ownership metadata, reservations, rollback/compensation and cleanup ordering.

**Runlith overlap:** `NEEDS_ADR`. Runlith already states that a stateful browser session is a capability boundary and that cookies/auth are scoped credentials (`INV-099`), but no explicit contract was found for user-owned browser targets, temporary borrow grants, popup/opener inheritance, return/cleanup or ownership reconciliation. Web Acquisition is already marked as requiring a dedicated ADR before implementation.

**Decision:** `ADAPT`.

**Differentiation:** Runlith can keep the useful ownership model while making the grant durable/Attempt-correlated, Policy-bound, auditable, provider-neutral and recoverable across browser-worker/process restarts.

### BSK-F002 — Observation/document-bound actionable refs

**finding_id:** `BSK-F002`

**source:** implementation

**repository:** `Tencent/BrowserSkill`

**revision:** `75e2c64abaf4b7cc75682d94b0c1fd5db0cbd5e5`

**file:** `apps/extension/src/session-manager/ref-store.ts`; `apps/extension/src/tools/vom/capture-coordinator.ts`

**symbol:** `RefStore`, `invalidateTab`, VOM capture/document identity validation

**region:** `ref-store.ts:3-15,35-115,121-153`; `capture-coordinator.ts:237-338`

**observation:** BrowserSkill treats `@eN` references as session-local handles to the latest observation rather than durable element identity. A fresh observation replaces the ref mapping and increments generation. Navigation/document change invalidates refs for the tab. Visual refs are refused unless the system has verified a document attachment/frame/root/backend-node identity. Capture verifies document identity before producing actionable observation entries.

**confidence:** `HIGH`

**Problem:** DOM/CDP node identifiers can be reused or become stale after navigation, frame replacement or document mutation. Reusing an old agent-visible ref against a new document can target the wrong element.

**Mechanism:** Bind refs to observation generation + tab/document identity, invalidate on document transition, and fail when identity cannot be established.

**Why it works:** The action handle is treated as a capability derived from a specific observation rather than as a globally stable selector.

**Limitations:** The generation is internal and process-local; the public short ref itself does not carry durable provenance. Highly dynamic pages can invalidate useful refs frequently.

**Failure modes:** missed document transition, ref-store loss across restart, or an adapter action path that bypasses the validated ref resolver.

**Complexity:** moderate and localized to the observation/action boundary.

**Runlith overlap:** `PARTIALLY_EXISTS`. `INV-103` and the browser architecture already require provenance-bearing snapshots with freshness semantics and `BrowserActionReceipt` target/ref hashing, but no explicit action-ref validity contract tied to document/snapshot revision was found.

**Decision:** `ADAPT`.

**Differentiation:** Runlith can make actionable browser refs provider-neutral and bind them to immutable snapshot/document identity, policy/session scope and Attempt provenance so stale refs fail closed after restart as well as within one extension process.

### BSK-F003 — Cancellation linearized with side-effect dispatch

**finding_id:** `BSK-F003`

**source:** implementation

**file:** `crates/bsk-cli/src/daemon/inflight.rs`

**symbol:** `InflightInner`, `ToolInflightEntry::promote_to_forwarded_with`, `cancel_and_snapshot`

**region:** `91-169,211-306`

**observation:** Cancellation state and the `Queued -> Forwarded` transition share one mutex. If cancellation wins, the dispatch closure is not invoked and no WebSocket side-effect request leaves the daemon. If dispatch wins, browser/ws correlation is committed and the request is placed on the sink before a later cancel can snapshot the forwarded state, preserving request-before-cancel ordering.

**confidence:** `HIGH`

**Problem:** a cancel RPC racing with dispatch can otherwise report cancellation while a side-effecting browser command still escapes.

**Mechanism:** one atomic critical section for canonical cancellation state, promotion metadata and request dispatch.

**Why it works:** there is no state in which cancellation has concluded "queued/no effect" while another thread can still publish the request.

**Limitations:** once a side effect has been dispatched, cancellation cannot prove whether the browser committed it; BrowserSkill separately needs effect-state/unknown outcome handling.

**Runlith overlap:** `ALREADY_DESIGNED` through bounded cancellation, governed side-effect execution, ambiguous-outcome reconciliation and enforcement near the privileged operation. The exact mutex technique is adapter-specific.

**Decision:** `EXTERNAL_VALIDATION`.

### BSK-F004 — Compact semantic browser observation with bounded work

**finding_id:** `BSK-F004`

**source:** implementation

**file:** `packages/vom/src/types.ts`; `packages/vom/src/render.ts`; `apps/extension/src/tools/vom/record-safe-observation.ts`; `apps/extension/src/tools/vom/capture-coordinator.ts`

**symbol:** VOM observation model/render pipeline; bounded capture worker pool

**region:** `types.ts:14-131`; `render.ts:182-210,783-960,1181-1361`; `record-safe-observation.ts:31-49,87-103`; `capture-coordinator.ts:43-70,199-227`

**observation:** VOM joins accessibility semantics with DOM-derived geometry/identity into a compact model-facing representation rather than returning raw trees by default. Rendering enforces bounds and semantic filtering/redaction, while capture uses bounded concurrency and deterministic output ordering.

**confidence:** `HIGH`

**Problem:** raw DOM/AX trees are expensive context and include low-value/sensitive details; model-facing browser context needs semantic compression without losing action provenance.

**Mechanism:** derived semantic observation with stable short refs, explicit bounds, redaction and document/node provenance.

**Why it works:** the model receives a smaller interaction-oriented view while the controller retains richer identity needed for actions.

**Limitations:** semantic projection can omit detail needed for unusual pages, and DOM/AX fusion is Chromium-specific implementation machinery.

**Runlith overlap:** `ALREADY_DESIGNED` at the architectural boundary: Context is a derived view, browser-specific types stay behind `BrowserAutomationPort`, and web snapshots are provenance-bearing resources. The concrete VOM projection is useful implementation inspiration for the future browser adapter.

**Decision:** `INSPIRED_ONLY`.

### BSK-F005 — Progressive disclosure of browser tool schemas

**finding_id:** `BSK-F005`

**source:** implementation

**file:** `packages/dsh-plugin-browserskill/src/lazy-tools.ts`

**symbol:** lazy browser tool registration/recovery

**region:** `1-153`

**observation:** The DSH integration does not register all browser tools into the model's baseline prompt. Browser tools become available only after the browser skill is successfully invoked. Successful reveal can be reconstructed from durable session events, while a failed reveal does not leave partially registered state latched.

**confidence:** `HIGH`

**Problem:** large tool catalogs consume context, increase ambiguity and widen the model-visible attack/decision surface.

**Mechanism:** small discovery surface first; load concrete schemas only when the skill/capability is requested.

**Runlith overlap:** `ALREADY_DESIGNED`. Source of Truth §43 already says not to send hundreds of tool schemas to every agent and specifies `tool.search` followed by on-demand schema loading. `INV-028` requires lazy skill bodies. `docs/architecture/adr/COVERAGE.md` already reserves a dedicated Tools/Skills registry ADR before implementation.

**Decision:** `EXTERNAL_VALIDATION`.

### BSK-F006 — Remote device credential lifecycle with one-use pairing and rotation

**finding_id:** `BSK-F006`

**source:** implementation + protocol documentation

**file:** `crates/bsk-cli/src/daemon/remote/authorization.rs`; `crates/bsk-cli/src/daemon/remote/server.rs`; `crates/bsk-cli/src/daemon/remote/rate_limit.rs`; `docs/remote-extension-connection.md`

**symbol:** remote authorization store/pair/renew/revoke; authenticated browser connection

**region:** `authorization.rs:1-167,170-355`; `server.rs:259-424`; `rate_limit.rs:1-69`; remote docs `52-72,147-158`

**observation:** Pairing secrets are one-use; server-side device credentials are stored as hashes; renewal rotates credentials and has retry semantics for the pending replacement; revocation closes/rejects connections; grants expire; authorization persistence uses private permissions and atomic snapshots; pairing/renewal/connection work is rate/capacity bounded. Cross-device routing does not trust a browser's self-reported identity as the sole authorization fact.

**confidence:** `HIGH`

**Problem:** a remote browser bridge needs revocable, bounded, restart-safe device authentication without persisting reusable plaintext bearer tokens server-side.

**Mechanism:** one-use bootstrap secret, random device credential, hashed durable verifier, expiry/renew-after, atomic rotation state and explicit revocation.

**Runlith overlap:** `ALREADY_DESIGNED` through Secret Broker, opaque secret references, scoped bindings and remote-worker credential/identity requirements. BrowserSkill provides useful implementation evidence, not a new Runlith authority model.

**Decision:** `EXTERNAL_VALIDATION`.

### BSK-F007 — Audit records execution uncertainty without claiming correctness

**finding_id:** `BSK-F007`

**source:** implementation + documentation

**file:** `crates/bsk-cli/src/daemon/audit.rs`; `docs/operation-audit.md`

**symbol:** audit append/start/finish/restart handling

**region:** `audit.rs:1-194,362-460,617-689`; audit docs `17-30,44-60`

**observation:** BrowserSkill audit is opt-in/local, append-oriented and metadata-allowlisted/redacted. Operation start is persisted before dispatch; operation finish records outcome; incomplete running records become `interrupted` after restart; transport/timeout ambiguity is represented as `unknown`. Audit status explicitly does not mean the website/business goal completed. If required pre-dispatch audit persistence fails, dispatch is refused.

**confidence:** `HIGH`

**Problem:** tool execution history must preserve uncertainty and must not be confused with business success.

**Mechanism:** pre-dispatch append, terminal/unknown/interrupted outcomes, redacted metadata and restart repair.

**Runlith overlap:** `ALREADY_IMPLEMENTED` at a stronger boundary. Runlith separates canonical product-authored Security Audit from Event Journal, logs and model-visible receipts (`INV-072`, `INV-073`, `INV-152`).

**Decision:** `EXTERNAL_VALIDATION`.

## Implementation techniques

The following implementation details are useful even where they do not justify new Runlith architecture:

- **Borrow reservations before awaited browser work** close a check-then-move race between sessions (`SessionManager::tryReserveBorrow`).
- **Revalidate after human approval**: BrowserSkill re-fetches/revalidates the target tab before moving it, so approval delay does not preserve stale target assumptions (`tabs.ts:753-785`).
- **Compensation produces `unknown` rather than lying about rollback** when a tab move succeeded but rollback cannot be confirmed (`tabs.ts:710-745,871-895`).
- **Session cleanup is ownership-sensitive**: borrowed tabs return first; task-created tabs close; user-created tabs survive; failed user-tab return keeps the window open (`session.ts:173-193`).
- **Capture concurrency is bounded** to a small worker pool and preserves deterministic output order (`capture-coordinator.ts:43-70,199-227`).
- **Probe budgets account for the next settle window before starting work**, reducing deadline overshoot (`apps/extension/src/tools/vom/hover-perception.ts:39-60`).
- **Plugin-owned session tracking refuses foreign identifiers** and reserves capacity before async spawn, preventing accidental takeover/capacity races (`packages/dsh-plugin-browserskill/src/sessions.ts:1-194`).
- **Runner retry is narrow and typed**: the DSH runner has one special retry for a known `session_busy` settlement race; other errors stay visible (`packages/dsh-plugin-browserskill/src/runner.ts:71-97,210-238`).

## AI / orchestration architecture

BrowserSkill is an AI tool capability, not an autonomous planning/completion engine.

**Who owns workflow state?** The embedding agent runtime owns the higher-level workflow. BrowserSkill owns live browser execution/session state only. In the DSH plugin, the plugin tracks sessions it created, while the BrowserSkill daemon/extension owns the corresponding browser execution resources.

**Who owns completion authority?** BrowserSkill reports tool results/effect state. It does not determine that the parent engineering/research task is complete. Audit documentation explicitly distinguishes a completed tool call from a confirmed business outcome.

**Can the model mutate canonical state?** The model can request browser actions, which mutate browser/page state, but BrowserSkill has no Runlith-like canonical Task state for the model to mutate. The important Runlith lesson is to keep these browser effects below trusted application-service/Policy ownership.

**Can model output directly cause privileged action?** Through the exposed tool interface, yes: authorized tool calls can navigate, click/type and evaluate JavaScript in controlled/borrowed tabs. BrowserSkill relies on session/tab scope and, for user-tab borrow, optional browser-side confirmation. It does not implement Runlith's separate typed Policy Engine decision for every browser action.

**What happens after crash/restart?** Local audit repairs unfinished audit state to `interrupted`; remote credential state is persisted; browser task/session state is not a durable workflow that is automatically resumed. Remote disconnect cleanup returns borrowed tabs/closes task-created tabs, and reconnection starts new tasks (`docs/remote-extension-connection.md:168`).

**What happens after provider/browser timeout?** Several side-effect paths preserve an `unknown` effect state rather than blindly retrying. Per-session dispatch remains occupied during bounded cleanup/compensation for sensitive input/screenshot operations. Retry policy is operation-specific and bounded.

**How does execution resume?** BrowserSkill favors a new session/task after remote reconnect. Within one live process, refs and session ownership are runtime state; after a new observation the agent receives fresh refs rather than assuming old element handles are durable.

## Security analysis

### CONFIRMED — Navigation path has no centralized SSRF/network-policy boundary

**finding_id:** `BSK-S001`

**source:** implementation

**file:** `apps/extension/src/tools/navigation.ts`; `apps/extension/src/tools/browser-navigation.ts`

**symbol:** `handleNavigate`, browser-navigation fallback

**region:** `navigation.ts:568-674`; `browser-navigation.ts:16-40,47-109`

**observation:** `handleNavigate` checks only that `params.url` is a non-empty string, resolves the session/tab and enforces Agent Window authority, then sends the URL to `Page.navigate` or the browser navigation fallback. No centralized evaluation of URL scheme, resolved IP/private ranges, redirect destination or site/network policy is performed in this path.

**classification:** `CONFIRMED`

**impact:** A controller with navigation authority can ask the browser to navigate wherever the browser/network profile can reach. This is an architecture-level SSRF/network-authority gap relative to Runlith; this review does not claim a specific exploit against a deployment.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE`. `INV-097`, Source of Truth `NetworkAccessPolicy`, `AGENT.md` web rules and `THR-007` require centralized policy evaluation for every acquisition/redirect hop.

**decision:** `REJECT`.

### CONFIRMED — Pairing grants broad browser-profile authority by design

**finding_id:** `BSK-S002`

**source:** protocol documentation + implementation-supported product contract

**file:** `docs/remote-extension-connection.md`

**region:** `162-170`

**observation:** The project explicitly documents that a paired server can create Agent Windows and navigate using that browser profile, including signed-in website sessions. Pairing is device authorization rather than a website/account sandbox. Content reads/actions are limited to task-created or explicitly borrowed tabs, but the paired server is still trusted at the browser-profile level.

**classification:** `CONFIRMED`

**impact:** Compromise/misconfiguration of the paired server is intentionally within a large authority boundary. Existing website login state can become usable by an authorized browser task after target control is granted.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE` as a canonical trust model. Runlith requires scoped browser credentials/capabilities and Secret Broker/Policy boundaries (`INV-099`, `INV-106`, ADR-0026).

**decision:** `REJECT` for broad profile authority; `ADAPT` the narrower device-authentication mechanics.

### CONFIRMED — Arbitrary page JavaScript evaluation is an exposed browser capability

**finding_id:** `BSK-S003`

**source:** implementation

**file:** `apps/extension/src/tools/evaluate.ts`

**symbol:** `handleEvaluate`

**region:** `133-179`

**observation:** `tool.evaluate` accepts a non-empty arbitrary JavaScript expression and executes it with CDP `Runtime.evaluate` on a target tab after session resolution and Agent Window/borrow authority checks. `throwOnSideEffect` is explicitly false.

**classification:** `CONFIRMED`

**impact:** Once a tab is controlled, the capability is intentionally powerful and can perform page-context side effects available to JavaScript. The session/window check prevents use against arbitrary unborrowed user tabs, but it is not a fine-grained browser action policy.

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE` if exposed as an unrestricted generic browser tool. Runlith's source design names `execute_restricted_browser_action` and requires Policy/Network/credential boundaries.

**decision:** `REJECT` for unrestricted arbitrary evaluation as a canonical capability. A future restricted, Policy-bound scripted browser action would be a separate design decision.

### CONFIRMED — User-tab borrow has explicit approval and ownership controls

**finding_id:** `BSK-S004`

**source:** implementation

**file:** `apps/extension/src/tools/tabs.ts`; `apps/extension/src/lib/interaction-preferences.ts`; `apps/extension/src/tools/borrow-confirmation.ts`

**region:** `tabs.ts:631-911`; preferences `1-60`; borrow confirmation `383-650`

**observation:** BrowserSkill can require browser-side human confirmation before borrowing an existing user tab; the request's legacy `confirm` field does not override browser preference. Target/session ownership is revalidated, double-borrow is reserved/refused, and failed confirmation denies the operation.

**classification:** `CONFIRMED`

**impact:** This materially reduces accidental authority transfer from the user's existing browsing state.

### CONFIRMED — Remote device credentials are hashed, rotated, expiring and revocable

**finding_id:** `BSK-S005`

**source:** implementation

**file:** `crates/bsk-cli/src/daemon/remote/authorization.rs`; `server.rs`; `rate_limit.rs`

**region:** `authorization.rs:1-355`; `server.rs:259-424`; `rate_limit.rs:1-69`

**observation:** Remote authorization uses one-use pairing, hashed durable device credentials, expiry/renewal, revocation, private/atomic storage and bounded request/connection handling.

**classification:** `CONFIRMED`

### PLAUSIBLE / NOT SUPPORTED items

- **Shell/argument injection:** `NOT_SUPPORTED` as a material BrowserSkill browser-control finding from the reviewed paths. The primary execution surface is typed RPC/CDP/WebExtension APIs rather than concatenated shell commands.
- **Path traversal/symlink filesystem authority:** `NOT_SUPPORTED` as a primary browser-session finding. File-transfer features have their own boundaries, but no source evidence reviewed here supports a repository-wide confirmed traversal claim.
- **Provider/browser identity spoofing across remote devices:** `NOT_SUPPORTED` for the simple self-reported-ID attack because remote server routing/authentication binds authenticated device identity and does not rely solely on a browser-provided ID.
- **Durable task restart semantics:** `PLAUSIBLE limitation`, not a vulnerability. BrowserSkill intentionally does not provide durable workflow replay/resume across remote disconnect; reconnection starts new tasks.

## Reliability / recovery analysis

BrowserSkill's reliability model is strongest around local browser effects:

- `Queued -> Forwarded -> Done` inflight state and cancellation share one serialization point, preventing cancel-before-dispatch races.
- Several browser tools return structured `effect_state: unknown` when the caller cannot establish whether a side effect committed.
- Borrow uses rollback/compensation and does not claim successful rollback when it cannot verify the result.
- Session stop returns borrowed user state first and preserves the Agent Window if return fails, favoring recoverability over destructive cleanup.
- The DSH runner cancels gracefully first and hard-kills only after a grace interval; it retries only a narrowly recognized settlement race.
- Remote reconnection deliberately does not replay old commands/sessions, avoiding hidden duplicate side effects.
- Audit restart marks unfinished records `interrupted` rather than success/failure guessed from absence of a completion event.

The main limitation for Runlith adoption is durability scope. These mechanisms are excellent within a live browser/daemon execution, but session/ref ownership is not a durable Attempt-level contract that survives arbitrary process/browser-worker restart. Runlith would need to bind any resumable browser state to durable Attempt/Policy/evidence identity and reconcile physical browser resources after restart.

## Performance / scalability analysis

BrowserSkill contains several bounded-load decisions that are relevant to an agent browser runtime:

- VOM capture uses a small fixed worker pool and preserves result order.
- Hover/name enrichment tracks explicit wall-clock budgets and avoids starting work whose settle window cannot fit.
- Per-session queues prevent concurrent operations from racing within one browser session; same-session overload surfaces `session_busy` rather than silently parallelizing actions.
- Remote server code limits pairing/renewal and connection setup capacity; overload has explicit rejection/backoff behavior.
- VOM is intentionally compact and avoids returning raw DOM/AX trees as the default model context.

The tradeoff is serialized session throughput: a single browser session prioritizes deterministic state over parallel command throughput. This is appropriate for interactive browser state. Horizontal scale should come from multiple isolated sessions/workers rather than concurrent mutation of one tab/session.

## Anti-patterns

### BSK-AP001 — Browser navigation authority is not coupled to centralized network policy

**Evidence:** `apps/extension/src/tools/navigation.ts:568-674` accepts a string URL and navigates after session/window checks; no centralized scheme/IP/redirect policy is invoked.

**Immediate impact:** Browser network reachability is broader than the application-level security decision represented by the tool call.

**Long-term impact:** Adding remote/cloud browser workers can turn local network reachability into an SSRF/control-plane boundary unless every hop is checked at a trusted network layer.

**Runlith lesson:** keep `NetworkAccessPolicy` as the canonical per-hop gate and bind validation to the actual connection/redirect path.

**Existing Runlith protection:** `INV-097`, `THR-007`, AGENT web rules, Source of Truth `NetworkAccessPolicy`.

### BSK-AP002 — Device pairing doubles as browser-profile trust

**Evidence:** `docs/remote-extension-connection.md:162-168` explicitly states that a paired server can operate with the paired browser profile, including signed-in sessions.

**Immediate impact:** one device grant covers a broad collection of existing browser credentials/state.

**Long-term impact:** server compromise or over-broad automation intent has a large blast radius unless site/account credentials and target ownership are independently scoped.

**Runlith lesson:** remote device authentication should prove which worker/device is connected; it should not itself grant every browser/site capability available on that device.

**Existing Runlith protection:** Policy authority, scoped capabilities, Secret Broker, browser credentials as explicit secret references (`INV-099`, `INV-106`, `INV-146`-`INV-153`).

### BSK-AP003 — Process-local browser session state is insufficient as durable workflow truth

**Evidence:** extension/DSH session ownership is maintained in runtime maps; remote disconnect cleanup ends task work and reconnection starts new tasks (`docs/remote-extension-connection.md:168`).

**Immediate impact:** live BrowserSkill sessions/element refs cannot be treated as durable task identity across restart.

**Long-term impact:** an embedding workflow engine that mistakes these runtime handles for canonical state would lose recovery semantics and could replay browser effects incorrectly.

**Runlith lesson:** browser session/ref state remains below durable Task/Execution/Attempt ownership and must be reconciled after worker restart.

**Existing Runlith protection:** `INV-001`, `INV-002`, `INV-019`, `INV-070`, `INV-105`.

## Runlith overlap analysis

### External idea: Explicit browser target ownership / borrow-return lifecycle

**External evidence:** `SessionManager::tryReserveBorrow`; `handleTabBorrow`; `handleSessionStop`; remote docs `162-168`.

**Closest Runlith equivalent:** stateful browser session as capability boundary; scoped browser credentials; browser actions as auditable execution.

**Runlith evidence:** Source of Truth `BrowserAutomationPort`, browser isolation profile, `INV-099`, `INV-104`, `INV-105`; `docs/architecture/adr/COVERAGE.md` marks Web Acquisition as a deferred architecture target requiring a dedicated ADR.

**Current status:** `NEEDS_ADR`.

**Actual gap:** No explicit contract says who owns pre-existing user browser targets, how temporary control is granted/revoked, whether opener/window placement inherits authority, what cleanup/compensation means, or how ownership survives/reconciles across browser-worker restart.

**Adoption:** `ADAPT`.

### External idea: Observation/document-bound actionable refs

**External evidence:** `RefStore` generation/replacement/invalidation and capture document identity verification.

**Closest Runlith equivalent:** immutable provenance-bearing web snapshots with freshness plus browser action receipts containing target/ref hash.

**Runlith evidence:** Source of Truth browser §§12-13; `INV-103`, `INV-104`.

**Current status:** `PARTIALLY_EXISTS`.

**Actual gap:** Snapshot freshness is defined, but the validity rule for an element/action reference is not: no explicit binding to snapshot/document/frame identity, stale-ref refusal or restart-safe ref revalidation was found.

**Adoption:** `ADAPT`.

### External idea: Compact semantic VOM observation

**External evidence:** VOM types/render/capture pipeline.

**Closest Runlith equivalent:** Context as a derived view, browser-neutral automation port, provenance-bearing Resource snapshots.

**Runlith evidence:** `INV-016`-`INV-018`, `INV-033`, `INV-095`-`INV-104`.

**Current status:** `ALREADY_DESIGNED` at the boundary; concrete adapter technique deferred.

**Actual gap:** No architecture gap. VOM is implementation inspiration for a future browser adapter/evaluation spike.

**Adoption:** `INSPIRED_ONLY`.

### External idea: Progressive browser tool-schema disclosure

**External evidence:** `packages/dsh-plugin-browserskill/src/lazy-tools.ts:1-153`.

**Closest Runlith equivalent:** Tool Registry + lazy skills.

**Runlith evidence:** Source of Truth §43 explicitly says not to send hundreds of schemas and to load a concrete schema after `tool.search`; `INV-028`; ADR coverage already reserves a dedicated Tools/Skills registry ADR.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** implementation remains deferred but is already an owned architecture target, so BrowserSkill does not create a new queue item.

**Adoption:** `EXTERNAL_VALIDATION`.

### External idea: Cancel-vs-dispatch serialization and typed unknown outcome

**External evidence:** `crates/bsk-cli/src/daemon/inflight.rs:91-306`; borrow compensation paths; daemon per-session queue tests.

**Closest Runlith equivalent:** governed side-effect execution, bounded retry/cancel, `AMBIGUOUS -> RECONCILE`, enforcement coupled to the privileged operation.

**Runlith evidence:** `INV-066`, `INV-091`, `INV-145`, `INV-153`; G8 side-effect protocol in Source of Truth/roadmap.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none at architecture level. BrowserSkill offers a useful adapter-level concurrency implementation example.

**Adoption:** `EXTERNAL_VALIDATION`.

### External idea: Remote device credential rotation/revocation

**External evidence:** remote authorization/server/rate-limit implementation.

**Closest Runlith equivalent:** Secret Broker + remote worker/device identity/scoped credentials.

**Runlith evidence:** ADR-0026, `INV-151`, G12 remote-worker architecture.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** no BrowserSkill-specific architecture gap; concrete browser-device protocol can inform future adapter implementation.

**Adoption:** `EXTERNAL_VALIDATION`.

### External idea: Direct navigation without centralized SSRF policy

**External evidence:** `handleNavigate` / fallback navigation.

**Closest Runlith equivalent:** centralized `NetworkAccessPolicy`.

**Runlith evidence:** `INV-097`, `THR-007`, Source of Truth browser/network policy.

**Current status:** `CONFLICTS_WITH_ARCHITECTURE`.

**Actual gap:** none; Runlith is already stricter.

**Adoption:** `REJECT`.

### External idea: Browser-profile-wide authority after device pairing

**External evidence:** remote docs `162-168`.

**Closest Runlith equivalent:** scoped capability/secret binding, authenticated worker identity.

**Runlith evidence:** `INV-099`, `INV-106`, ADR-0026, Secret Broker.

**Current status:** `CONFLICTS_WITH_ARCHITECTURE` as an authorization model.

**Actual gap:** none; only the narrower authentication mechanics are useful.

**Adoption:** `REJECT` broad trust / `EXTERNAL_VALIDATION` credential mechanics.

## External validation of existing Runlith design

BrowserSkill independently supports several Runlith decisions:

- **Tools/skills should be progressively disclosed.** BrowserSkill lazily exposes browser tools; Runlith already specifies on-demand tool schema loading.
- **Browser/controller state is not Task state.** BrowserSkill browser/session internals stay in the browser execution layer; Runlith already requires browser runtime infrastructure not to own durable Task state.
- **Cancellation around side effects needs a real serialization boundary.** BrowserSkill fixed a concrete cancel/promote race by making the decision atomic with dispatch.
- **Unknown is a real outcome.** Borrow/input/navigation paths preserve unknown effect state when compensation/transport cannot prove what happened.
- **Audit evidence is not business correctness.** BrowserSkill explicitly says operation completion does not prove the website/business goal succeeded; Runlith already separates receipts/audit/evaluation.
- **User state needs ownership-aware cleanup.** BrowserSkill closes task-created state but preserves/returns user-owned tabs; this validates Runlith's general resource/authority ownership discipline.
- **Remote credentials need rotation/revocation and bounded authorization storage.** BrowserSkill demonstrates a concrete implementation compatible with Runlith's Secret Broker principles.
- **Model-facing browser context benefits from bounded semantic projection.** VOM supports Runlith's existing Context-as-derived-view model without making browser-specific structures canonical domain state.

## Novel gaps

### Gap 1 — Browser target ownership and temporary control lifecycle

**status:** `NEEDS_ADR`

Runlith has browser-session isolation and scoped credential rules, but it does not yet define the ownership/authority lifecycle for pre-existing user tabs/windows versus task-created targets. This becomes material if Runlith controls a user's normal browser profile or any shared browser worker.

The future contract should define at minimum:

- target owner (`task-created`, `user-owned`, possibly external/shared worker-owned);
- whether/when a user-owned target may be temporarily granted to an Attempt;
- exact grant scope, Policy/approval binding and expiry;
- whether opener/popup/window placement or navigation can inherit authority (default: no implicit widening);
- original-location/state provenance needed for return;
- cleanup ordering and compensation;
- ambiguous physical move/return outcome and reconciliation;
- behavior across browser-worker/agentd restart;
- browser adapter neutrality and no leakage of Chrome tab IDs into core Task contracts.

### Gap 2 — Freshness contract for actionable browser refs

**status:** `PARTIALLY_EXISTS`

Runlith already defines freshness/provenance for fetched snapshots, but no explicit action-ref contract was found. A future interactive browser implementation needs to prevent an element ref observed in document A from being applied after navigation/frame replacement to document B.

The future contract should bind every actionable ref to enough identity to prove its observation/session/document/frame scope, invalidate/re-resolve it after relevant transitions, and fail closed when identity/freshness cannot be proven.

## Adoption matrix

| External idea | Runlith status | Decision | Meaningful Runlith delta |
|---|---|---|---|
| Explicit task-created vs user-owned browser targets with borrow/return | `NEEDS_ADR` | `ADAPT` | durable/Policy-bound/provider-neutral ownership + restart reconciliation |
| Observation/document-bound actionable refs | `PARTIALLY_EXISTS` | `ADAPT` | immutable provenance + stale-ref fail-closed across adapters/restarts |
| VOM compact semantic observation | `ALREADY_DESIGNED` | `INSPIRED_ONLY` | keep projection replaceable/evaluated rather than canonical |
| Cancel/dispatch linearization | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | apply generic side-effect protocol; adapter chooses locking mechanism |
| Unknown effect state / no blind retry | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | durable Attempt/Reconciler ownership |
| Lazy browser tool schema reveal | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Runlith Tool Registry is provider-neutral and separately governed |
| Remote device token hash/rotate/revoke | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Secret Broker/scoped worker identity |
| Audit start-before-dispatch + unknown/interrupted states | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` | canonical product Security Audit + separate verification |
| Direct navigation without centralized per-hop SSRF policy | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Runlith retains `NetworkAccessPolicy` |
| Paired server receives browser-profile-wide trust | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | device identity separated from site/account capability |
| Unrestricted arbitrary page JS as normal browser action primitive | `CONFLICTS_WITH_ARCHITECTURE` if canonical | `ADAPT` | only restricted, Policy-bound scripted capability if justified |

## Ideas rejected for Runlith

1. **Treating device pairing as authorization for the whole browser profile.** Runlith should authenticate a browser worker/device independently from the site/account/target capabilities an Attempt receives.
2. **Allowing the browser adapter to navigate before centralized SSRF/network policy has authorized the actual destination/redirect path.** This directly conflicts with `INV-097`.
3. **Treating process-local browser session/ref state as durable workflow truth.** Browser state remains execution infrastructure and must reconcile into durable Attempt evidence.
4. **Making arbitrary JavaScript evaluation a default unrestricted capability.** If a future adapter needs scripted evaluation, it should be explicit, restricted, Policy-visible and auditable.
5. **Copying VOM/CDP/Chrome types into Runlith core.** The useful concept is compact semantic observation + action provenance, not BrowserSkill's browser-specific representation.

## Candidate ADRs

### ADR-CANDIDATE — Browser target ownership, temporary control and cleanup semantics

**Type:** `ADR`

**Problem:** Runlith's Web/Browser architecture defines session isolation and scoped credentials but not the ownership/authority transition for pre-existing user browser targets.

**Current Runlith state:** Web Acquisition is a deferred architecture target requiring a dedicated ADR. Browser sessions are capability boundaries; browser state does not own durable Task state.

**External inspiration:** BrowserSkill Agent Windows plus explicit `tab_borrow`/`tab_return`, reservation, confirmation and ownership-aware cleanup.

**Actual delta:** Define provider-neutral browser target identity/owner, temporary grant/revocation, popup/opener inheritance rule, Policy/approval binding, cleanup/compensation and restart reconciliation.

**Target subsystem:** future `BrowserAutomationPort` / Browser Runtime / Policy / Attempt recovery.

**Dependencies:** dedicated Web Acquisition/Browser ADR activation; target/secret capability model; browser worker identity.

**Architecture impact:** moderate; new browser execution contract, no change to Task identity.

**Security impact:** high positive; prevents implicit authority transfer from user browsing state.

**Acceptance direction:** authority cannot widen due to window placement/opener/focus; user-owned targets require explicit scoped grant; cleanup preserves user state; ambiguous return becomes typed/reconcilable; adapter-specific IDs stay below port boundary.

**Verification direction:** adversarial ownership tests across two sessions, popup/opener cases, approval delay, move/commit failure, worker restart, target disappearance and failed return.

**Suggested gate:** before first interactive browser automation implementation under deferred Web Acquisition architecture.

**Priority:** `MEDIUM`, becomes `HIGH` when shared/user-profile browser control enters scope.

### ADR-CANDIDATE — Actionable browser reference freshness and identity

**Type:** `ADR` section or contract within the Web Acquisition/Browser ADR

**Problem:** Browser snapshot freshness is defined, but actionable element references do not yet have a documented validity/invalidation contract.

**Current Runlith state:** provenance-bearing browser snapshots and BrowserActionReceipt ref hashes are designed.

**External inspiration:** BrowserSkill `RefStore` generation replacement, tab/document invalidation and verified visual ref identity.

**Actual delta:** Bind actionable ref to snapshot/document/frame/session identity and specify fail-closed stale-ref behavior plus optional re-observation/re-resolution.

**Target subsystem:** BrowserAutomationPort + browser observation/action adapter contracts.

**Dependencies:** browser snapshot/resource identity and future browser session contract.

**Architecture impact:** small-to-moderate; strengthens the browser port contract without changing Task domain semantics.

**Security impact:** positive; reduces confused-target actions after navigation or frame/document replacement.

**Acceptance direction:** stale/unprovable refs cannot trigger browser mutation; action receipts identify the observation/ref identity used.

**Verification direction:** navigation, same-tab document replacement, frame replacement, ref reuse, restart/reconnect and cross-session ref attempts must fail or require fresh observation.

**Suggested gate:** same pre-implementation browser/Web ADR gate.

**Priority:** `MEDIUM`.

## Candidate implementation tasks

### TASK-CANDIDATE — Browser target ownership/borrow contract spike

**Type:** `RESEARCH`

**Problem:** No executable Runlith contract currently proves ownership-safe operation of shared/user browser targets.

**Current Runlith state:** Browser automation implementation is deferred; architecture boundary exists.

**External inspiration:** BrowserSkill task-created/borrowed distinction and compensating return.

**Actual delta:** Prototype provider-neutral target ownership/grant types and an adversarial test matrix without selecting a permanent browser backend.

**Target subsystem:** BrowserAutomationPort contract / Policy capability model.

**Dependencies:** ADR-CANDIDATE above.

**Architecture impact:** contract-level only for the spike.

**Security impact:** validates no implicit authority widening.

**Acceptance direction:** represent ownership/grant/expiry/return outcome without Chrome-specific IDs in core contracts.

**Verification direction:** deterministic model tests for grant scope, revocation, cross-session conflict, ambiguous return/reconcile and restart identity.

**Suggested gate:** before browser adapter production implementation.

**Priority:** `MEDIUM`.

### TASK-CANDIDATE — Stale browser action-ref contract tests

**Type:** `RESEARCH`

**Problem:** Snapshot freshness does not yet mechanically prevent stale element actions.

**Current Runlith state:** snapshot provenance/freshness and browser receipts are architecture-only.

**External inspiration:** BrowserSkill ref generation/document invalidation.

**Actual delta:** define a minimal provider-neutral observation/ref identity model and executable invalidation cases for the future adapter contract.

**Target subsystem:** BrowserAutomationPort / browser observation resources.

**Dependencies:** Web/Browser ADR and snapshot identity model.

**Architecture impact:** localized contract/test addition after architecture acceptance.

**Security impact:** prevents confused-target mutation.

**Acceptance direction:** ref from observation/document A cannot mutate document B; missing identity fails closed; fresh re-observation yields a new actionable ref.

**Verification direction:** cross-document, cross-frame, cross-session, post-restart and ref-reuse cases.

**Suggested gate:** before click/type/evaluate-like capabilities are production-enabled.

**Priority:** `MEDIUM`.

## Source evidence index

| Finding | Repository / revision | File / symbol / region | Confidence |
|---|---|---|---|
| `BSK-F001` browser target ownership/borrow | `Tencent/BrowserSkill@75e2c64` | `apps/extension/src/session-manager/manager.ts` `tryReserveBorrow` `159-213`; `apps/extension/src/tools/tabs.ts` `631-911`; `apps/extension/src/tools/session.ts` `173-358`; `docs/remote-extension-connection.md:162-168` | `HIGH` |
| `BSK-F002` document-bound refs | same | `apps/extension/src/session-manager/ref-store.ts:3-153`; `apps/extension/src/tools/vom/capture-coordinator.ts:237-338` | `HIGH` |
| `BSK-F003` cancel/dispatch linearization | same | `crates/bsk-cli/src/daemon/inflight.rs:91-306` | `HIGH` |
| `BSK-F004` VOM compact observation | same | `packages/vom/src/types.ts:14-131`; `packages/vom/src/render.ts:182-210,783-960,1181-1361`; `record-safe-observation.ts:31-103`; `capture-coordinator.ts:43-70,199-227` | `HIGH` |
| `BSK-F005` lazy browser tools | same | `packages/dsh-plugin-browserskill/src/lazy-tools.ts:1-153` | `HIGH` |
| `BSK-F006` remote auth lifecycle | same | `crates/bsk-cli/src/daemon/remote/authorization.rs:1-355`; `server.rs:259-424`; `rate_limit.rs:1-69`; remote docs `147-158` | `HIGH` |
| `BSK-F007` audit uncertainty | same | `crates/bsk-cli/src/daemon/audit.rs:1-194,362-460,617-689`; `docs/operation-audit.md:17-60` | `HIGH` |
| `BSK-S001` missing centralized navigation SSRF policy | same | `apps/extension/src/tools/navigation.ts` `handleNavigate` `568-674`; `browser-navigation.ts:16-109` | `HIGH` |
| `BSK-S002` profile-wide pairing authority | same | `docs/remote-extension-connection.md:162-168` | `HIGH` |
| `BSK-S003` arbitrary JS evaluate | same | `apps/extension/src/tools/evaluate.ts` `handleEvaluate` `133-179` | `HIGH` |
| `BSK-S004` borrow approval | same | `apps/extension/src/tools/tabs.ts:631-911`; `borrow-confirmation.ts:383-650`; `interaction-preferences.ts:1-60` | `HIGH` |
| `BSK-S005` credential hardening | same | `authorization.rs:1-355`; `server.rs:259-424`; `rate_limit.rs:1-69` | `HIGH` |

Runlith comparison evidence used for the adoption decisions:

- `AGENT.md` web/browser rules (`~452-469`): untrusted web content, centralized per-hop SSRF/policy, no inherited host authority, Secret Broker credentials, bounded retries and receipts.
- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §43: tool schemas loaded on demand after `tool.search`.
- Source of Truth browser architecture (`~5927-6191`): `BrowserAutomationPort`, browser isolation, `NetworkAccessPolicy`, provenance/freshness snapshots and `BrowserActionReceipt`.
- `docs/architecture/INVARIANTS.md`: `INV-028`, `INV-033`, `INV-095`-`INV-106`, plus canonical authority/audit rules.
- `docs/security/THREAT_MODEL.md`: `THR-007` SSRF, `THR-008` web prompt injection, `THR-010` browser credential sharing.
- `docs/architecture/adr/COVERAGE.md`: Tools/Skills registry and Web Acquisition are deferred architecture targets requiring dedicated ADRs before implementation.
