# 9Router

## Source revision

- **Repository:** `https://github.com/decolua/9router`
- **Repository name:** `decolua/9router`
- **Default branch:** `master`
- **Revision reviewed:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`
- **Exact tag at revision:** none found
- **Package version at revision:** `0.5.75` (`package.json`)
- **Review date:** `2026-09-16`
- **Research mode:** source-level architecture/security/reliability review. External source was treated as evidence only; no source code was copied into Runlith and no dependency was added.

All findings below are scoped to the frozen revision above.

## Executive summary

9Router is a local/self-hostable HTTP AI routing proxy and compatibility layer. Its strongest engineering work is concentrated around provider protocol translation, model/provider capability metadata, account-level fallback/cooldown, credential refresh, and request compatibility across heterogeneous provider APIs. It is not a durable agent orchestrator: it has no Runlith-style Task → Execution → Attempt → Provider Session state machine, no product-owned completion authority, no durable recovery/reconciliation workflow, and no independent verification boundary.

The highest-value positive result for Runlith is a narrow provider-adapter testing technique: 9Router derives a provider/model compatibility matrix from the same registry used by runtime routing and couples it with golden/round-trip translation tests. Runlith already has a reusable provider-neutral `ExecutionPort` contract suite, but the broader provider contract (`detect/capabilities/send`, streaming, tool-call normalization) is explicitly still open. This is therefore `PARTIALLY_EXISTS`, decision `ADAPT`, and is the only new implementation-knowledge candidate created by this review.

Most other strong ideas are external validation of Runlith decisions already present: capability-aware routing, provider-neutral adaptation, failure-scope-aware fallback, bounded council/fusion, native-format preference to reduce lossy transformations, secret/process isolation, SSRF connection binding, and the rule that provider output cannot own workflow completion.

The review also found several source-confirmed negative lessons. Provider access/refresh/API credentials are persisted in ordinary application SQLite data; optional request-file logging deliberately disables header redaction; the local MCP bridge shares one child process across sessions while its POST path ignores the issued `sessionId` and broadcasts child output to every session; the Browser MCP preset executes an unpinned `@latest` npm package while inheriting the full parent environment; and the `sql.js` fallback acknowledges in-memory writes before a debounced whole-database file replacement. SSRF handling is stronger than simple hostname blocklists but still has a DNS check/connect TOCTOU window, while custom-provider validation uses only literal-host blocking before a normal fetch.

No evidence supports an unauthenticated remote MCP execution finding at this revision. MCP routes are protected by deny-by-default API middleware and an additional local-only/CLI-token gate. That negative result is important because the route handlers alone would otherwise look more exposed than the composed control plane actually is.

## Project purpose

9Router exposes OpenAI-/Claude-/Gemini-compatible HTTP surfaces to coding tools and routes requests to many providers/models. The runtime adds format translation, provider/account selection, fallback, quota/cooldown tracking, token refresh, token/context transformations, usage/request observability, and optional multi-model fusion. The dashboard manages providers, credentials, combos, API keys and runtime settings.

Supplemental project evidence: `README.md:4-8,46-69,74-120`; `package.json:1-17,19-57`.

## Architecture map

```text
Repository
├── Interfaces
│   ├── Next.js dashboard and /api/* management routes
│   ├── /v1, /v1beta, /responses and provider-compatible LLM HTTP surfaces
│   └── local MCP SSE/message bridge
├── Orchestration
│   ├── request-scoped chat/fetch/search handlers
│   ├── combo fallback / sticky round-robin
│   └── optional fusion panel + judge synthesis
├── Domain
│   ├── provider/model registries
│   ├── capability metadata
│   ├── connection/account health and model locks
│   └── settings, combos, provider nodes and API keys
├── State/Persistence
│   ├── SQLite-family adapters with sql.js fallback
│   ├── versioned migrations + additive schema synchronization
│   ├── providerConnections/providerNodes/settings/usage/request details
│   └── process-local Maps for refresh, rotation, sessions and transient state
├── Providers
│   ├── provider registry/configuration
│   ├── specialized executors + default executor
│   ├── OAuth/token-refresh services
│   └── source/target protocol translation registry
├── Tools/Capabilities
│   ├── centralized model/provider capability metadata
│   ├── input-modality detection and capability-aware ordering
│   └── local/remote MCP integrations
├── Execution
│   ├── executor.execute() HTTP/provider call boundary
│   ├── native-format path when transport supports source format
│   └── translated path when protocol adaptation is required
├── Recovery
│   ├── account/model cooldown + fallback
│   ├── bounded token-refresh retry
│   ├── provider-combo fallback
│   └── no durable long-running workflow reconciler
├── Security
│   ├── dashboard/API guard + local-only route gate
│   ├── trusted TCP-peer stamping in custom server
│   ├── URL/SSRF helpers
│   └── credential/API-key persistence in application DB
└── Observability
    ├── usage/request-details persistence
    ├── structured application logs
    └── optional per-request filesystem trace logging
```

## Canonical state ownership

9Router has no single canonical workflow-state aggregate because it is primarily a router, not a durable workflow engine.

Durable configuration/operational state is owned by the local database repositories. `providerConnections`, `providerNodes`, settings, combos, API keys, usage history and request details are persisted through `src/lib/db/**`. Provider connection health/cooldown fields are written back into connection records, so account-selection state partly survives restart.

Request lifecycle is process/request owned. `chatCore` and capability-specific handlers construct a request, choose an executor/translation path, invoke a provider, process errors/refresh/fallback, normalize output and return content. There is no durable Task/Execution/Attempt object around this lifecycle.

Several coordination states are explicitly process-local: provider-selection mutex state, OAuth refresh single-flight locks, combo rotation state, MCP child/session maps and other runtime caches. Restart resets them. That is acceptable where they are optimizations, but they must not be interpreted as durable ownership/recovery state.

For Runlith this split is a key boundary lesson: 9Router is useful below the orchestration layer as provider-routing/adaptation evidence, not as a template for canonical Task lifecycle ownership.

## Strong engineering decisions

### 9R-F001 — Registry-based provider translation with direct loss-reduction paths

**finding_id:** `9R-F001`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `open-sse/translator/index.js`

**symbol:** `register`, `translateRequest`, `translateResponse`

**region:** `18-29,51-159,161-219`

**observation:** Request/response translators register by source/target protocol pair. Exact translators are preferred where present; otherwise conversion can pivot through the OpenAI representation. Streaming conversion keeps explicit state. This limits provider-specific protocol logic to an adaptation layer and allows selected pairs to avoid a lossy double translation.

**confidence:** `HIGH`

**Problem:** Heterogeneous provider wire protocols drift in message, tool, image, reasoning and streaming semantics.

**Mechanism:** Explicit translator registry plus direct pair-specific paths where fidelity requires them.

**Why it works:** Provider-specific quirks do not need to leak into the caller's request model, and the direct path can preserve semantics that an intermediate canonical form would lose.

**Limitations/failure modes:** A canonical pivot can still be lossy; translation correctness grows combinatorially as protocols evolve; direct translators can become special-case islands without contract testing.

**Complexity:** medium/high, dominated by protocol evolution and compatibility testing.

**Runlith status:** `ALREADY_DESIGNED` at the provider anti-corruption boundary; broader send/stream/tool normalization remains open.

**Decision:** `EXTERNAL_VALIDATION` for the boundary; `ADAPT` for the testing technique in `9R-F002`.

### 9R-F002 — Provider/model registry drives translation conformance coverage

**finding_id:** `9R-F002`

**source:** external tests

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `tests/translator/matrix.js`; `tests/translator/golden-request.test.js`; translator round-trip tests

**symbol:** `buildModelMatrix`, golden request suites, round-trip suites

**region:** `matrix.js:1-67`; `golden-request.test.js:1-58` plus related translator tests

**observation:** The test matrix is derived from the same `PROVIDER_MODELS` registry used by runtime configuration, so adding a provider/model expands compatibility coverage automatically. Golden requests lock behavior for system/image/tool/tool-result/reasoning concerns, while round-trip coverage checks semantic preservation including distinct tool identities.

**confidence:** `HIGH`

**Problem:** Provider support can appear in configuration while translation coverage silently lags.

**Mechanism:** One registry feeds both runtime compatibility metadata and generated test cases; golden and round-trip invariants exercise semantic concerns.

**Why it works:** Registry drift becomes a test-surface change rather than a manually maintained checklist.

**Limitations/failure modes:** Snapshot/golden equality can bless a bad shape if reviewed mechanically; a matrix proves configured transformations, not live upstream behavior; capability metadata itself can be wrong.

**Complexity:** moderate and scales better than handwritten per-model suites.

**Runlith status:** `PARTIALLY_EXISTS`. `crates/execution-contract` already supplies one reusable provider-neutral `ExecutionPort` contract suite, but `docs/architecture/providers.md:382-406` explicitly leaves full AgentProvider send/capability/stream/tool-call contracts open.

**Decision:** `ADAPT`.

### 9R-F003 — Native-format preference reduces avoidable translation loss

**finding_id:** `9R-F003`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `open-sse/handlers/chatCore.js`; `open-sse/providers/capabilities.js`

**symbol:** request-format selection; capability registry

**region:** `chatCore.js:~80-190`; `capabilities.js:1-300`

**observation:** Runtime routing can keep the incoming wire format when the selected model/provider transport supports it, translating only when required. Capabilities are centralized with exact provider/model overrides and ordered fallback patterns.

**confidence:** `HIGH`

**Runlith status:** `ALREADY_DESIGNED` conceptually by Provider capabilities and anti-corruption boundaries.

**Decision:** `EXTERNAL_VALIDATION`.

### 9R-F004 — Account selection serializes mutation and persists scoped cooldown evidence

**finding_id:** `9R-F004`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/sse/services/auth.js`; `open-sse/config/errorConfig.js`

**symbol:** `getProviderCredentials`, `markAccountUnavailable`, fallback classification

**region:** `auth.js:9-10,28-40,84-139,141-193,229-280`

**observation:** Selection is serialized with a Promise mutex, supports preferred/fill-first/sticky round-robin behavior, filters model-scoped locks, and persists error/cooldown/backoff state to the connection record. Exact provider reset timestamps can override generic exponential backoff.

**confidence:** `HIGH`

**Runlith status:** `PARTIALLY_EXISTS` only at future Adaptive Model Router detail level, already tracked by `IKQ-003 — Provider failure-scope identity for fallback routing`.

**Decision:** `EXTERNAL_VALIDATION`; 9Router adds evidence to IKQ-003, not a duplicate queue item.

### 9R-F005 — Credential refresh uses stable-identity single-flight and rotation-aware retry

**finding_id:** `9R-F005`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `open-sse/services/oauthCredentialManager.js`; `src/sse/services/tokenRefresh.js`; `open-sse/handlers/chatCore.js`

**symbol:** `withCredentialRefreshLock`, `mergeRefreshedCredentials`, `checkAndRefreshToken`, 401/403 refresh path

**region:** `oauthCredentialManager.js:11,70-120,123-155`; `tokenRefresh.js:153-210,224-268`; `chatCore.js:405-450`

**observation:** Concurrent refreshes for one provider/credential identity collapse onto one promise. Refresh-token rotation is merged into current credentials, and the 401/403 retry path mutates the active credential object after each successful refresh so subsequent attempts do not reuse a consumed rotating refresh token.

**confidence:** `HIGH`

**Runlith status:** `INSPIRED_ONLY` for future providers whose authentication is application-managed. Runlith deliberately keeps CLI-native authentication provider-owned and routes explicit secrets through Secrets Broker.

**Decision:** `INSPIRED_ONLY`.

### 9R-F006 — Transport-derived peer evidence is stronger than caller-supplied forwarding headers

**finding_id:** `9R-F006`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `custom-server.js`; `src/dashboardGuard.js`

**symbol:** wrapped `http.createServer`, `isLoopbackPeer`, `isLocalRequest`

**region:** `custom-server.js:9-14,49-75`; `dashboardGuard.js:109-133`

**observation:** The custom server derives the peer from the TCP socket, strips caller-supplied forwarding/internal headers, trusts a forwarded address only from a loopback reverse proxy, and stamps a per-process token consumed by the dashboard guard.

**confidence:** `HIGH`

**Runlith status:** `SUPERSEDED` by ADR-0020, which authenticates the local OS principal rather than relying on a process-secret HTTP peer scheme.

**Decision:** `EXTERNAL_VALIDATION` of the principle that caller headers are not identity; do not adopt the weaker principal model.

## Implementation techniques

### 9R-F007 — Capability-aware combo ordering preserves fallback while preferring semantic fit

**finding_id:** `9R-F007`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `open-sse/services/combo.js`

**symbol:** `reorderByCapabilities`, `detectRequiredCapabilities`

**region:** `10-12,61-82,102+`

**observation:** Input modality requirements are detected and combo members are stably reordered into hard-capability/full-fit tiers without deleting fallback candidates.

**confidence:** `HIGH`

**Runlith status:** `ALREADY_DESIGNED` by `INV-060` hard eligibility before preference scoring. Runlith should remain stricter: a candidate missing a mandatory capability/security/privacy condition is ineligible, not merely lower-ranked.

**Decision:** `EXTERNAL_VALIDATION` with that stricter Runlith distinction.

### 9R-F008 — Fusion uses quorum plus straggler grace instead of full-panel latency

**finding_id:** `9R-F008`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `open-sse/services/combo.js`

**symbol:** `collectPanel`, `handleFusionChat`, judge prompt construction

**region:** `443-475,489-535,547-624`

**observation:** Panel calls run in parallel with minimum-success quorum, short straggler grace and a hard timeout. Sources are anonymized before a judge model synthesizes the result. Zero successful answers fail; one successful answer bypasses synthesis.

**confidence:** `HIGH`

**Runlith status:** `ALREADY_DESIGNED` at council level, with stronger authority invariants `INV-086`/`INV-087` requiring deterministic evidence/security gates and minority objections to survive synthesis.

**Decision:** `INSPIRED_ONLY` for latency tuning; `REJECT` as verification/completion authority.

## AI / orchestration architecture

9Router is AI infrastructure but not an AI engineering workflow orchestrator in the Runlith sense.

| Question | 9Router at reviewed revision |
|---|---|
| Who owns workflow state? | No durable workflow aggregate exists; the HTTP request handler owns request progression and DB repositories own durable configuration/usage state. |
| Who owns completion authority? | The router returns the normalized provider result. Fusion may return a judge model's synthesis. There is no separate task-verification authority. |
| Can the model mutate canonical Task state? | No Runlith-like Task state exists. Model output is returned to the caller; external clients may subsequently act on it. |
| Can model output directly cause privileged action inside 9Router? | Reviewed chat routing does not expose a generic model-authored shell execution loop. MCP/plugin execution is managed by API routes and preset plugins. |
| What happens after process crash? | Durable DB configuration remains subject to backend persistence semantics; request-local work, refresh locks, combo rotation and MCP sessions are lost. |
| What happens after provider timeout/failure? | Request-level account/model/combo fallback and bounded refresh logic may try another route depending on classified failure. |
| How does execution resume? | New HTTP request/retry/fallback; there is no durable Attempt checkpoint/reconciler for an interrupted long-running workflow. |

Planner, durable subagent graph, skills-as-orchestration-state, durable context continuity, independent verification and completion authorization are outside the reviewed product's core runtime. This is a product-scope difference, not by itself a defect.

## Security analysis

### 9R-S001 — Provider credentials and API keys are persisted in ordinary application SQLite

**classification:** `CONFIRMED`

**finding_id:** `9R-S001`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/lib/db/repos/connectionsRepo.js`; `src/lib/db/schema.js`; `src/sse/services/tokenRefresh.js`

**symbol:** `OPTIONAL_FIELDS`, `connToRow`, `upsert`, `providerConnections`, `apiKeys`, `updateProviderCredentials`

**region:** `connectionsRepo.js:5-11,52-78`; `schema.js:34-52,78-87`; `tokenRefresh.js:153-210`

**observation:** `accessToken`, `refreshToken`, `apiKey` and `idToken` are ordinary connection properties serialized into the `providerConnections.data` JSON text column. Application API keys are stored directly in an indexed `key` column. Token refresh writes refreshed credential values back through this path. These persistence paths invoke no application-level encryption/decryption step before serialization or database write.

**confidence:** `HIGH`

**Impact:** A copied/readable application database exposes reusable credential material subject to the provider/token lifetime. OS-level disk/file protections may reduce exposure but are outside this application contract.

**Runlith overlap:** `SUPERSEDED`. ADR-0026 and `DATA_CLASSIFICATION.md` explicitly forbid SECRET values in SQLite/files/logs and use platform credential stores with opaque `SecretRef` application state.

**Decision:** `REJECT`.

### 9R-S002 — Optional request trace logging deliberately preserves full secrets

**classification:** `CONFIRMED`

**finding_id:** `9R-S002`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `open-sse/utils/requestLogger.js`; `open-sse/handlers/chatCore.js`

**symbol:** `maskSensitiveHeaders`, `createRequestLogger`, `logTargetRequest`

**region:** `requestLogger.js:4-5,60-90,117-175`; `chatCore.js:357-380`

**observation:** When `ENABLE_REQUEST_LOGS=true`, masking is explicitly disabled with a comment stating that the full token is kept for testing. Client and provider-target headers and bodies are written into files beneath `logs/`; `chatCore` invokes target-request logging after executor request construction.

**confidence:** `HIGH`

**Impact:** Authorization/API-key/cookie material and potentially sensitive prompts/provider payloads can be written to plaintext trace files. A separate DB request-details path performs header sanitation; that does not protect this filesystem logger.

**Runlith overlap:** `SUPERSEDED` by SECRET logging rules, Secrets Broker and security-audit separation.

**Decision:** `REJECT`.

### 9R-S003 — DNS policy check is not bound to the socket connection

**classification:** `PLAUSIBLE`

**finding_id:** `9R-S003`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/shared/utils/ssrfGuard.js`

**symbol:** `assertPublicUrlResolved`, `fetchPublic`

**region:** `160-214`

**observation:** The guard performs strong literal/range checks, resolves DNS, rejects resolved private/metadata addresses and revalidates every redirect. The subsequent connection is still made by an ordinary `fetch(currentUrl)`, which can resolve the hostname again; the previously validated addresses are not pinned to the actual connection.

**confidence:** `HIGH` for the TOCTOU structure; exploitability depends on DNS/network conditions.

**Impact:** An attacker-controlled hostname can potentially change resolution between policy evaluation and connect (DNS rebinding/time-of-check-to-time-of-use).

**Runlith overlap:** `ALREADY_DESIGNED`; Pattern Library P-017 and `INV-097` already require connection-bound SSRF enforcement.

**Decision:** `EXTERNAL_VALIDATION` of the existing Runlith requirement.

### 9R-S004 — Remote custom-provider validation uses literal-only SSRF filtering before network access

**classification:** `PLAUSIBLE`

**finding_id:** `9R-S004`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/app/api/provider-nodes/validate/route.js`; `src/shared/utils/ssrfGuard.js`

**symbol:** `POST`, `assertPublicUrl`

**region:** `validate/route.js:54-76,78+`; `ssrfGuard.js:160-166`

**observation:** For a non-local caller, custom provider validation calls the literal-host-only `assertPublicUrl`, then performs provider validation using normal fetch helpers. A public hostname that resolves to an internal address is not rejected by that preliminary check.

**confidence:** `HIGH` for the missing resolved-address enforcement; practical exploitation requires an authenticated/authorized caller because provider-node routes are protected by the dashboard guard.

**Impact:** An authenticated management request that reaches this route can potentially make the server connect to network destinations not intended by the public-host policy.

**Runlith overlap:** `ALREADY_DESIGNED` by centralized network policy/redirect/range checks; P-017 is stronger.

**Decision:** `REJECT` as a network enforcement pattern.

### 9R-S005 — MCP child receives ambient environment and runtime-resolves a mutable package

**classification:** `CONFIRMED`

**finding_id:** `9R-S005`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/lib/mcp/stdioSseBridge.js`; `src/shared/constants/coworkPlugins.js`

**symbol:** `getOrSpawn`, `LOCAL_STDIO_PLUGINS`

**region:** `stdioSseBridge.js:104-118`; `coworkPlugins.js:23-34`

**observation:** Only preset local plugins may spawn, which usefully blocks arbitrary user-supplied commands. The spawned process nevertheless receives `env: process.env`. The Browser MCP preset is launched through `npx -y @browsermcp/mcp@latest`, so runtime dependency resolution is mutable rather than bound to an audited package revision.

**confidence:** `HIGH`

**Impact:** The child inherits every credential/config variable available to the 9Router process, and a future package release can change executable code without a 9Router source revision change. This is supply-chain/ambient-authority risk, not evidence that the current package is malicious.

**Runlith overlap:** ambient env is already covered by AP-020 and TASK-0065; execution pinning/explicit authority is consistent with G6 Runner/Secrets boundaries.

**Decision:** `REJECT`.

### 9R-S006 — MCP session identifier is issued but not enforced; child responses broadcast across sessions

**classification:** `CONFIRMED`

**finding_id:** `9R-S006`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/app/api/mcp/[plugin]/sse/route.js`; `src/app/api/mcp/[plugin]/message/route.js`; `src/lib/mcp/stdioSseBridge.js`

**symbol:** `GET`, `POST`, `registerSession`, `sendToChild`

**region:** `sse/route.js:6-24`; `message/route.js:7-18`; `stdioSseBridge.js:109-175`

**observation:** SSE registration returns a random `sessionId` and advertises a POST endpoint carrying it. The POST handler does not read or validate that query parameter; it forwards the JSON-RPC body to a plugin-wide child. The child is shared per plugin and each stdout JSON-RPC frame is broadcast to every registered session.

**confidence:** `HIGH`

**Impact:** Concurrent authorized local sessions are not isolated at the bridge boundary. One session can inject requests into the shared child while responses/events are visible to all sessions, creating cross-session confusion and potential data disclosure. Exposure is limited by the composed local-only/auth gate.

**Runlith overlap:** `ALREADY_DESIGNED` principle: explicit execution/session identity and authority must be bound at the trusted operation boundary; opaque IDs without validation are not authorization.

**Decision:** `REJECT`.

### 9R-S007 — Unauthenticated remote MCP execution is not supported by composed-route evidence

**classification:** `NOT_SUPPORTED`

**finding_id:** `9R-S007`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/dashboardGuard.js`; `custom-server.js`

**symbol:** `LOCAL_ONLY_PATHS`, `canAccessLocalOnlyRoute`, `proxy`, peer stamping

**region:** `dashboardGuard.js:50-88,109-166,202-230`; `custom-server.js:49-75`

**observation:** `/api/mcp/*` is covered by the protected API set and the stronger local-only route gate. Access requires a valid local CLI token or a local peer plus dashboard authentication; the custom server strips spoofable internal peer headers before stamping its own evidence.

**confidence:** `HIGH`

**Conclusion:** Do not report the MCP route handlers as an unauthenticated remote RCE/auth-bypass path at this revision.

## Reliability / recovery analysis

### 9R-R001 — sql.js fallback acknowledges logical writes before durable disk persistence

**finding_id:** `9R-R001`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/lib/db/adapters/sqlJsAdapter.js`; `src/lib/db/driver.js`

**symbol:** `createSqlJsAdapter`, `persist`, `scheduleSave`, `transaction`

**region:** `sqlJsAdapter.js:13-39,46-58,83-114`; driver fallback selection

**observation:** The sql.js backend commits mutations to an in-memory database and schedules a whole-database export 100 ms later. `transaction()` releases its savepoint and returns before the scheduled file write. Shutdown handlers flush best effort, but abrupt process/power loss can discard recently acknowledged writes. The file is replaced directly with `writeFileSync`; no temp-file + atomic rename protocol is visible.

**confidence:** `HIGH`

**Impact:** In environments that fall back to sql.js, application-level commit semantics are stronger than crash durability. This matters for credentials/settings and would be unacceptable for Runlith canonical orchestration state.

**Runlith overlap:** `SUPERSEDED` by durable SQLite-backed Task/Execution/Attempt/journal semantics and crash/restart verification.

**Decision:** `REJECT` for canonical durable state.

### 9R-R002 — Versioned migrations are transactional, but additive repair can continue after unexpected schema failures

**finding_id:** `9R-R002`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/lib/db/migrate.js`; `src/lib/db/backup.js`

**symbol:** `runVersionedMigrations`, `syncSchemaFromTables`, `importWithAssertion`, `runMigrationOnce`, `backupDbLite`

**region:** `migrate.js:17-39,56-109,231-250,263-289`; `backup.js:1-17,36-74`

**observation:** Versioned migrations run transactionally and legacy import checks row counts, aborting the transaction if records were dropped. In contrast, additive column/index synchronization logs/ignores some DDL failures and startup continues. Pre-schema backup is best effort and migration continues if backup fails. Backups intentionally omit request details and have manual restore semantics.

**confidence:** `HIGH`

**Runlith lesson:** Keep the transactional/versioned migration and explicit import assertions; fail visibly on unexpected canonical-schema evolution errors rather than converting them into partial schema drift.

**Decision:** `INSPIRED_ONLY` for the assertions; `REJECT` for fail-open additive schema repair of canonical state.

### 9R-R003 — Restart semantics are request-level, not workflow-level

**finding_id:** `9R-R003`

**source:** external implementation

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `src/sse/services/auth.js`; `open-sse/services/oauthCredentialManager.js`; `open-sse/services/combo.js`; `src/lib/mcp/stdioSseBridge.js`

**symbol:** process-local mutex/Maps/session store

**region:** `auth.js:9-10`; `oauthCredentialManager.js:11`; `combo.js:84-88`; `stdioSseBridge.js:100-102`

**observation:** Several concurrency/rotation/session mechanisms are process-local. Restart safely loses them only insofar as they are optimizations/transport state; there is no durable workflow reconciliation contract to resume an interrupted multi-step job.

**confidence:** `HIGH`

**Runlith status:** `CONFLICTS_WITH_ARCHITECTURE` if copied as orchestration truth; acceptable inside adapter-local transient state.

**Decision:** `REJECT` as a Task/recovery design.

### 9R-R004 — Rich compatibility tests are not wired into repository CI workflows

**finding_id:** `9R-R004`

**source:** external repository workflow configuration

**repository:** `decolua/9router`

**revision:** `17c4cc76877bd1755030a8414f8d0083f48dcccf`

**file:** `.github/workflows/docker-publish.yml`; `.github/workflows/gitbook-pages.yml`; `tests/translator/**`

**symbol:** GitHub Actions jobs

**region:** `docker-publish.yml:1-60`; full `gitbook-pages.yml`

**observation:** The reviewed workflow directory contains only image publishing and GitBook deployment workflows. No test/lint/translator-conformance job runs the substantial test suite before those repository workflows. Docker publishing also explicitly disables provenance and SBOM output.

**confidence:** `HIGH`

**Impact:** High-value protocol compatibility tests exist but repository CI does not make them a merge/release gate at this revision.

**Runlith status:** `ALREADY_IMPLEMENTED` at the general verification discipline level; no new Runlith architecture gap.

**Decision:** `EXTERNAL_VALIDATION` of why provider contract suites need CI enforcement.

## Performance / scalability analysis

9Router contains several useful request-path optimizations:

- native protocol passthrough can avoid unnecessary translation work and semantic loss;
- account/model cooldown avoids repeatedly hitting known-unavailable routes;
- single-flight credential refresh reduces duplicate refresh bursts;
- capability-aware combo ordering improves the chance that early routes satisfy modality/tool needs;
- fusion panel collection uses quorum + grace + hard timeout to bound tail latency;
- token/context transformations include explicit checks for claimed savings versus actual outbound-size change.

The main scaling limits are equally clear. The architecture is process-centric: selection locks, combo rotation, refresh locks and MCP session state are local memory; multi-process deployment would require explicit coordination if those mechanisms were promoted from optimization to correctness. `sql.js` persistence rewrites the whole database and is therefore a compatibility fallback rather than a scalable high-write storage path. Fusion multiplies provider cost/latency by panel size and introduces an additional judge request. Central provider/model capability tables also require disciplined freshness and test coverage as model catalogs churn.

## Anti-patterns

### AP — Debug observability captures raw credentials

**Evidence:** `open-sse/utils/requestLogger.js:72-90,129-175`.

**Immediate impact:** enabling a debugging feature creates plaintext credential-bearing files.

**Long-term impact:** operators can accumulate a secondary secret store with unclear retention and wider filesystem exposure.

**Runlith lesson:** redact at the capture boundary; a debug flag must not widen SECRET disclosure rules.

**Existing Runlith protection:** ADR-0026 + SECRET data classification + Secrets Broker.

### AP — Application DB is also a credential vault

**Evidence:** `src/lib/db/repos/connectionsRepo.js:5-11,52-78`; `src/lib/db/schema.js:34-52,78-87`.

**Immediate impact:** database compromise becomes credential compromise.

**Long-term impact:** rotation, deletion, backup and export semantics become coupled to ordinary application persistence.

**Runlith lesson:** canonical app state stores opaque references/bindings, not secret values.

**Existing Runlith protection:** TASK-0063 / ADR-0026.

### AP — Session token exists in the protocol but not in authorization/isolation enforcement

**Evidence:** MCP SSE route issues `sessionId`; message route ignores it; bridge shares one child and broadcasts output to all sessions.

**Immediate impact:** concurrent sessions can interfere and observe each other's child output.

**Long-term impact:** adding more privileged MCP tools makes an ornamental session identifier increasingly misleading as an isolation boundary.

**Runlith lesson:** IDs/provenance become security-relevant only when checked at the trusted operation boundary.

**Existing Runlith protection:** exact Task/Execution/Attempt provenance and capability binding.

### AP — Runtime executable dependency floats at `latest`

**Evidence:** `src/shared/constants/coworkPlugins.js:23-34` plus child spawn in `stdioSseBridge.js:109-118`.

**Immediate impact:** behavior can change without the parent repository revision changing.

**Long-term impact:** source revision no longer fully identifies executable supply-chain inputs; incident reproduction/audit weakens.

**Runlith lesson:** executable adapters/tools need version/provenance evidence, and runtime authority must remain independently scoped.

**Existing Runlith protection:** governed Runner, explicit provider/tool boundaries, restricted environment and execution receipts.

### AP — In-memory transaction success is stronger than physical durability

**Evidence:** `src/lib/db/adapters/sqlJsAdapter.js:20-39,88-114`.

**Immediate impact:** abrupt failure inside the debounce window can lose an already-returned write.

**Long-term impact:** callers cannot infer restart-safe state from successful mutation return values.

**Runlith lesson:** durable lifecycle transitions require a persistence barrier whose success means recoverable state is on the durable side of the boundary.

**Existing Runlith protection:** durable state/journal contracts and restart/crash verification.

## Runlith overlap analysis

| External idea | External evidence | Closest Runlith equivalent | Runlith evidence | Current status | Actual gap | Decision |
|---|---|---|---|---|---|---|
| Registry-based provider ACL/translation | `translator/index.js` | Provider Adapter anti-corruption layer | `docs/architecture/providers.md:41-43,90-128`; ADR-0009 | `ALREADY_DESIGNED` | none at boundary level | `EXTERNAL_VALIDATION` |
| Registry-derived translation matrix + golden/round-trip cases | `tests/translator/matrix.js`, golden/roundtrip suites | `crates/execution-contract` shared provider seam tests | `providers.md:282-303,382-406` | `PARTIALLY_EXISTS` | future send/stream/tool-call/capability protocol conformance matrix | `ADAPT` |
| Native-format preference | `chatCore.js` + provider formats/capabilities | adapter-local protocol translation/capabilities | `providers.md:90-100,171-192` | `ALREADY_DESIGNED` | implementation depends on future provider protocols | `EXTERNAL_VALIDATION` |
| Failure-scoped persistent cooldown | `auth.js:229-280` | Adaptive Model Router failure-scope contract | IKQ-003; `INV-066/067/085` | `PARTIALLY_EXISTS` | already tracked by IKQ-003 | `EXTERNAL_VALIDATION` |
| OAuth refresh single-flight | `oauthCredentialManager.js:123-155` | provider auth + Secrets Broker boundary | `providers.md:160-169`; ADR-0026 | `ALREADY_DESIGNED` boundary, provider-specific implementation open | only relevant for app-managed OAuth providers | `INSPIRED_ONLY` |
| Quorum/grace fusion | `combo.js:473-624` | Multi-provider Council | `INV-086`, `INV-087`, Source of Truth council sections | `ALREADY_DESIGNED` | latency tuning only | `INSPIRED_ONLY` |
| Connection-bound SSRF | negative evidence in `ssrfGuard.js:202-214` | `NetworkAccessPolicy` | P-017; `INV-097` | `ALREADY_DESIGNED` | future concrete network adapter implementation | `EXTERNAL_VALIDATION` |
| Transport-derived peer identity | `custom-server.js:49-75` | local IPC peer authentication | ADR-0020 | `SUPERSEDED` | none | `EXTERNAL_VALIDATION` |
| Ambient child environment | `stdioSseBridge.js:117` | provider/runtime env isolation | TASK-0065, runners architecture, AP-020 | `ALREADY_IMPLEMENTED` protection | none | `REJECT` |
| Plaintext app-DB credentials | connection/schema repos | Secret Broker | ADR-0026; DATA_CLASSIFICATION SECRET | `SUPERSEDED` | none | `REJECT` |
| Request logging without secret masking | `requestLogger.js:72-90` | SECRET/logging boundary | ADR-0026; DATA_CLASSIFICATION | `SUPERSEDED` | none | `REJECT` |
| Debounced sql.js persistence for canonical state | `sqlJsAdapter.js` | durable SQLite Task/Event storage | G2-G7 persistence/restart contracts | `CONFLICTS_WITH_ARCHITECTURE` | none | `REJECT` |
| MCP plugin-wide shared session | MCP SSE/message/bridge files | Task/Attempt/session provenance + Policy authority | `INV-146`, exact provenance/approval contracts | `CONFLICTS_WITH_ARCHITECTURE` if used as trusted multi-session execution | none | `REJECT` |
| Context/tool-output compression | chat transformations | Execution Efficiency Architecture | Source of Truth recoverable compression; prior Caveman/Ponytail research | `ALREADY_DESIGNED` | none from 9Router | `EXTERNAL_VALIDATION` |

## External validation of existing Runlith design

9Router provides independent implementation evidence for several decisions Runlith already made:

1. **Provider-specific behavior belongs behind adapters.** The amount of provider-specific request, response, thinking, tool and transport logic in 9Router demonstrates why Runlith keeps provider semantics out of Task identity.
2. **Capability metadata must be explicit.** Model families differ on vision, reasoning, search, context/output bounds and thinking controls; hard eligibility cannot be inferred from a provider name alone.
3. **Fallback requires scoped failure identity.** Per-model/account cooldown behavior validates IKQ-003's need to distinguish credential/model/endpoint failure scope rather than globally poisoning a provider.
4. **Native protocol paths can preserve fidelity.** A canonical representation is useful but should not force unnecessary lossy conversion when the selected provider can safely consume the original shape.
5. **Council latency needs bounded collection.** Quorum/grace/hard timeout is good performance evidence, while Runlith's stronger Verification/Policy invariants remain necessary for authority.
6. **SSRF hostname checks are insufficient unless the connection is bound to the validated destination.** 9Router's otherwise careful guard still leaves the classic DNS check/connect gap already captured by P-017.
7. **Secret isolation and environment minimization are architecture concerns, not logging conventions.** Plaintext credential persistence, raw debug logging and `process.env` inheritance show why Runlith's Secrets Broker and TASK-0065 boundaries are valuable.

## Novel gaps

Exactly one evidence-backed Runlith delta was found.

### 9R-GAP-001 — Provider protocol translation conformance matrix

**External idea:** Generate provider/model protocol compatibility cases from the runtime provider/capability registry and require semantic golden/round-trip invariants for canonical concerns.

**External evidence:** `tests/translator/matrix.js:1-67`; `tests/translator/golden-request.test.js:1-58`; related round-trip translator tests; `open-sse/translator/index.js:51-219`.

**Closest Runlith equivalent:** reusable `crates/execution-contract` suite against `ExecutionPort`.

**Runlith evidence:** `docs/architecture/providers.md:282-303` proves the current execution seam; `382-406` explicitly leaves full `AgentProviderPort`, capability taxonomy, streaming output and tool-call normalization open.

**Current status:** `PARTIALLY_EXISTS`.

**Actual gap:** When Runlith activates a broader multi-provider protocol/session layer, there is no accepted executable matrix yet that derives cases from declared provider/model capabilities/formats and proves canonical request/response/tool/stream semantics remain stable across adapters.

**Differentiation:** Runlith's version should bind the matrix to provider-neutral contracts and capability declarations, keep provider output untrusted, preserve Task/Execution authority outside the adapter, and test policy/security failure behavior independently. It should not reproduce 9Router's application-owned credential storage or request-level lifecycle as part of the matrix.

**Decision:** `ADAPT`.

No other `NOVEL_GAP`, `PARTIALLY_EXISTS` or `NEEDS_ADR` item attributable to 9Router survived overlap analysis.

## Adoption matrix

| Finding | Runlith status | Adoption |
|---|---|---|
| Registry/provider translation ACL | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Registry-derived protocol conformance matrix | `PARTIALLY_EXISTS` | `ADAPT` |
| Native-format preference | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Failure-scoped cooldown/fallback | `PARTIALLY_EXISTS`, already IKQ-003 | `EXTERNAL_VALIDATION` |
| Credential refresh single-flight | provider-specific future concern | `INSPIRED_ONLY` |
| Fusion quorum/grace | `ALREADY_DESIGNED` council | `INSPIRED_ONLY` |
| Judge synthesis as completion/verification | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |
| TCP/process-secret peer authentication | `SUPERSEDED` by OS peer auth | `EXTERNAL_VALIDATION` |
| Plaintext app-DB credentials | `SUPERSEDED` by Secrets Broker | `REJECT` |
| Raw secret-bearing debug traces | `SUPERSEDED` by SECRET rules | `REJECT` |
| Ambient child env | `ALREADY_IMPLEMENTED` protection | `REJECT` |
| Mutable `@latest` executable runtime dependency | conflicts with reproducible governed execution | `REJECT` |
| Shared MCP child without session enforcement | conflicts with provenance/authority isolation | `REJECT` |
| sql.js debounced persistence as durable truth | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |
| SSRF check separate from actual connection | `ALREADY_DESIGNED` stronger Runlith rule | `EXTERNAL_VALIDATION` |
| Token/context compression | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |

## Ideas rejected for Runlith

- Persisting provider secrets/API keys in ordinary application SQLite.
- Disabling redaction when verbose request logging is enabled.
- Treating a successful provider/fusion judge response as workflow completion evidence.
- Making a process-local provider/session Map authoritative workflow state.
- Replaying stateful work merely because an HTTP/provider request failed.
- Giving provider/MCP subprocesses the parent's entire environment.
- Executing mutable `@latest` tool dependencies in an autonomous execution path.
- Using a session identifier decoratively while routing writes and responses through a shared plugin-wide channel.
- Using a debounced in-memory sql.js export as the durability contract for Task/Execution/Approval/Audit state.
- Treating literal URL validation or pre-connect DNS validation as sufficient SSRF enforcement.
- Replacing ADR-0020 OS-principal IPC authentication with the weaker HTTP process-token scheme.

## Candidate ADRs

None created from 9Router.

The one real gap is an executable conformance technique inside an already accepted provider anti-corruption direction. The exact broader AgentProvider contract remains an existing open Runlith design item and should be decided by its owning gate rather than by this external repository.

## Candidate implementation tasks

### TASK-CANDIDATE — Provider protocol translation conformance matrix

**Type:** `TASK`

**Problem:** The current shared `ExecutionPort` suite proves execution-result semantics but does not yet cover future provider protocol translation/capability/stream/tool-call contracts because those halves remain intentionally open.

**Current Runlith state:** Provider ACL and replacement semantics are designed; one real Claude executor plus FakeAgent satisfy the shared G5 execution contract; full AgentProvider capability/send/stream/tool normalization remains open.

**External inspiration:** 9Router derives compatibility cases from its provider/model registry and couples them to golden/round-trip translation checks.

**Actual delta:** When the broader provider contract becomes active, derive conformance cases from Runlith's canonical provider capability/format declarations so declaring support creates required tests automatically. Cover canonical request/response, structured tool call/result identity, streaming assembly, reasoning metadata, supported media, malformed/unsupported cases, and provider-native passthrough equivalence where applicable.

**Target subsystem:** Provider Adapter contract/test infrastructure.

**Dependencies:** activation of the broader AgentProvider send/capability/streaming/tool-call contract; at least two materially different provider protocol implementations or one canonical protocol plus deterministic fixtures.

**Architecture impact:** low/positive if kept test-only and derived from accepted provider contracts; no new domain authority.

**Security impact:** malformed/provider output remains untrusted; capability declarations never grant Policy authority; tests should include refusal when mandatory capability/security semantics cannot be represented.

**Acceptance direction:** adding/changing a declared provider/model protocol surface automatically expands the matrix; all adapters pass shared semantic invariants; direct/native paths and translated paths produce equivalent canonical meaning for supported concerns; unsupported/lossy cases fail explicitly.

**Verification direction:** deterministic fixtures, no live provider dependency; golden tests for stable outbound protocol shapes where necessary; semantic round-trip tests for tool IDs/results/streaming/reasoning/media; mutation/negative fixtures for missing capabilities and malformed provider responses; CI gate.

**Suggested gate:** the first gate that activates the full AgentProvider send/capability/stream/tool-call contract or introduces a second materially distinct production provider protocol.

**Priority:** `MEDIUM`, becoming `HIGH` when that gate activates.

## Source evidence index

| ID | External source evidence | Confidence |
|---|---|---|
| `9R-F001` | `open-sse/translator/index.js:18-29,51-219` | `HIGH` |
| `9R-F002` | `tests/translator/matrix.js:1-67`; `tests/translator/golden-request.test.js:1-58`; related round-trip tests | `HIGH` |
| `9R-F003` | `open-sse/handlers/chatCore.js:~80-190`; `open-sse/providers/capabilities.js` | `HIGH` |
| `9R-F004` | `src/sse/services/auth.js:9-10,28-40,84-139,141-193,229-280` | `HIGH` |
| `9R-F005` | `open-sse/services/oauthCredentialManager.js:11,70-155`; `src/sse/services/tokenRefresh.js:153-268`; `open-sse/handlers/chatCore.js:405-450` | `HIGH` |
| `9R-F006` | `custom-server.js:9-14,49-75`; `src/dashboardGuard.js:109-133` | `HIGH` |
| `9R-F007` | `open-sse/services/combo.js:10-12,61-82,102+` | `HIGH` |
| `9R-F008` | `open-sse/services/combo.js:443-475,489-624` | `HIGH` |
| `9R-S001` | `src/lib/db/repos/connectionsRepo.js:5-11,52-78`; `src/lib/db/schema.js:34-52,78-87` | `HIGH` |
| `9R-S002` | `open-sse/utils/requestLogger.js:4-5,60-90,117-175`; `chatCore.js:357-380` | `HIGH` |
| `9R-S003` | `src/shared/utils/ssrfGuard.js:160-214` | `HIGH` structure / `PLAUSIBLE` exploitability |
| `9R-S004` | `src/app/api/provider-nodes/validate/route.js:54-76,78+`; `ssrfGuard.js:160-166` | `HIGH` structure / `PLAUSIBLE` exploitability |
| `9R-S005` | `src/lib/mcp/stdioSseBridge.js:104-118`; `src/shared/constants/coworkPlugins.js:23-34` | `HIGH` |
| `9R-S006` | MCP SSE/message routes + `stdioSseBridge.js:109-175` | `HIGH` |
| `9R-S007` | `src/dashboardGuard.js:50-88,109-166,202-230`; `custom-server.js:49-75` | `HIGH` |
| `9R-R001` | `src/lib/db/adapters/sqlJsAdapter.js:13-39,46-58,83-114` | `HIGH` |
| `9R-R002` | `src/lib/db/migrate.js:17-39,56-109,231-289`; `src/lib/db/backup.js:1-17,36-74` | `HIGH` |
| `9R-R003` | process-local state in auth/OAuth/combo/MCP modules | `HIGH` |
| `9R-R004` | `.github/workflows/docker-publish.yml`; `.github/workflows/gitbook-pages.yml`; translator tests | `HIGH` |

Runlith overlap evidence used for decisions: `docs/architecture/providers.md:41-43,90-128,160-192,282-303,382-406`; `docs/architecture/adr/ADR-0020-local-ipc-peer-authentication.md`; `docs/architecture/adr/ADR-0026-g6-security-authority-and-capability-contracts.md:92-112`; `docs/architecture/runners.md:132-153,174-207`; `docs/security/DATA_CLASSIFICATION.md:86-103`; `docs/architecture/INVARIANTS.md` (`INV-059`, `INV-060`, `INV-066`, `INV-067`, `INV-072`, `INV-085`, `INV-086`, `INV-087`, `INV-093`, `INV-097`, `INV-146`, `INV-150`); `external-research/PATTERN_LIBRARY.md` P-011/P-017; `external-research/IMPLEMENTATION_KNOWLEDGE_QUEUE.md` IKQ-003.
