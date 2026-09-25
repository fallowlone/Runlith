# ADR-0037 — Confined Claude Provider Authentication Through a Trusted-Host Credential Gateway

Status: **Proposed** (TASK-0152; owner review pending — this ADR is not
ratified and grants no authority until the owner accepts it)
Gate: G8

> This decision selects a mechanism that lets the confined Claude provider
> authenticate under mandatory G6 macOS Seatbelt confinement without
> granting the provider process Keychain, SecurityServer, or
> `/usr/bin/security` authority — the capability set TASK-0112 measured as
> jointly required by the provider-owned macOS authentication path, and
> that TASK-0114 refuses before any provider work
> (`provider_authentication_unsupported_in_mandatory_confinement`,
> `bins/agentd/src/execution.rs:88-89`). It evaluates Option A (a
> trusted-host credential broker that injects the real credential into the
> child's environment), Option B (a trusted-host authenticated loopback
> proxy — the "credential gateway"), and Option C (a Keychain/
> SecurityServer/`security` grant inside the sandbox, already rejected by
> TASK-0112). It selects **Option B**. It does not itself edit
> `providers.md` §7, the TASK-0065 H7 closure text, or
> `PRIVACY_ARCHITECTURE.md` §5 — it states the exact superseding text for a
> separate, listed documentation follow-up (see "Proposed amendments"). It
> authorizes no production implementation on its own; it stays Proposed
> until the owner ratifies it after independent architecture/security
> review, and the owner's own real-credential gateway probe is recorded
> here as **PENDING** (see "Owner real-credential receipts"). No credential
> value, Keychain item, or `security`/`claude` invocation was read, run, or
> reproduced while drafting this ADR.

## Context

### TASK-0112: why the host session cannot be reused as-is

TASK-0112 diagnosed why an authenticated Claude CLI works on the host but
fails under Runlith's production-shaped macOS Seatbelt profile. Differential
ablation isolated the exact capability set the CLI's Keychain-backed
authentication path needs, all three together and none alone:

```text
mach-lookup com.apple.SecurityServer
file-read-data ~/Library/Keychains/login.keychain-db
process-exec /usr/bin/security
```

(`docs/development/G7_TASK_0112_PROVIDER_AUTH_DIAGNOSIS.md`, "Capability
isolation"). TASK-0112's own architecture decision rejected adding this set
to the production sandbox: Seatbelt can scope the executable path and the
Keychain file path, but it cannot bind `/usr/bin/security` argv to one
Claude credential item, so granting it would expose the user's *general*
login-Keychain access mechanism to the provider process — a strictly larger
grant than "let Claude authenticate," and a direct conflict with G6
least-authority (`docs/development/G7_TASK_0112_PROVIDER_AUTH_DIAGNOSIS.md`,
"Root cause" and "Architecture decision for remediation").

### TASK-0114: the current fail-closed refusal

TASK-0114 implemented the successor: an explicit pre-launch refusal when an
authenticated-but-confinement-incompatible provider state is detected,
before any provider work runs. The refusal constant and its macOS reason
are:

- `bins/agentd/src/execution.rs:88-89` —
  `const PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED: &str =
  "provider_authentication_unsupported_in_mandatory_confinement";`
- `bins/agentd/src/execution.rs:98-111` — `enum ConfinementAuthVerdict {
  NoMandatoryConfinement, Compatible, Unsupported(&'static str) }`. The
  `Compatible` variant already exists in the type precisely so a future
  confining platform (this ADR's implementation follow-up) must choose it
  deliberately rather than inherit silence.
- `bins/agentd/src/execution.rs:159-165` (the `#[cfg(target_os = "macos")]`
  arm) currently sets `AUTH_CONFINEMENT_VERDICT` to `Unsupported(...)`,
  citing TASK-0112's measurement verbatim in the reason string.
- `bins/agentd/src/execution.rs:816-826`, inside `prepare_provider_launch`
  — after the provider's own `auth status` probe confirms a host-valid
  authenticated session (`provider_claude::probe_authentication`,
  `adapters/provider-claude/src/authentication.rs:94`,
  `permits_launch()` at `authentication.rs:58-60`), the function reads
  `AUTH_CONFINEMENT_VERDICT` and, if `Unsupported`, refuses with
  `format!("{PROVIDER_AUTH_CONFINEMENT_UNSUPPORTED}: {reason}")` before any
  provider process is spawned.

Today this refusal fires unconditionally under mandatory macOS confinement:
production cannot run the Claude provider under G6 confinement at all. This
ADR's exit criterion is a mechanism that lets `AUTH_CONFINEMENT_VERDICT`
become `Compatible` for a specific, narrow launch shape, without touching
the `Unsupported` reasoning TASK-0112 established for the general case.

### TASK-0152: current CLI (2.1.281) empirical evidence

`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md` (repository base
`78079e3`, Claude Code `2.1.281`) probed which non-Keychain authentication
inputs the current CLI honours under the exact production-shaped Seatbelt
profile, using only dummy credentials or no credential. Load-bearing rows:

- **R1** — unauthenticated refusal reproduces the TASK-0112 pattern under
  the current CLI, validating the harness (`"Not logged in · Please run
  /login"`).
- **R2/R3/R4** — `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`, and
  `ANTHROPIC_AUTH_TOKEN` are each **honoured**: the CLI reaches the real
  Anthropic API over the profile's existing `*:443` scope and receives a
  genuine `401` for the dummy value — proof the variable is read and
  transmitted, not proof a valid credential completes a session.
- **R5** — the honoured mechanism needs no pre-existing host `~/.claude`
  state at all; a completely fresh identity anchor reproduces the same
  result.
- **R6** — the load-bearing finding: with `ANTHROPIC_BASE_URL` pointed at a
  trusted local HTTP stub and the network scope reduced to that one local
  port (no `*:443`, no mDNSResponder), the CLI **completes a full
  non-interactive run** (marker present, exit `0`) with internet access
  removed from the sandbox profile entirely.
- **R6 no-credential variant** — the CLI still reaches the stub (one `HEAD
  /api/hello`) but refuses before `POST /v1/messages` when no credential
  variable is set — the credential-less refusal survives the redirection,
  so pointing the CLI at a local endpoint does not itself bypass
  authentication.
- **R7** — `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` produced no
  observed behavioral change against R6 in this exact probe shape.
- **R8** — against the local stub, per credential mode: `ANTHROPIC_API_KEY`
  → `x-api-key` header; `CLAUDE_CODE_OAUTH_TOKEN` and `ANTHROPIC_AUTH_TOKEN`
  → `authorization: Bearer <value>`. The `anthropic-beta`/`anthropic-version`
  flag set is entirely CLI-determined per mode (OAuth mode adds
  `oauth-2025-04-20` and omits several flags the other two modes send) — a
  forwarding gateway never has to add or guess a flag, only relay what the
  CLI already sent.

The evidence doc's own "Capability and compatibility notes" section
establishes two gaps this ADR must resolve, both re-verified against
current `HEAD` for this ADR:

- **`ProviderEnvironment` passes none of the honoured variables today.**
  `resolved_pairs()` (`adapters/provider-claude/src/environment.rs:142-199`)
  builds an explicit, closed list — `PATH, HOME, USER, PWD,
  XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_STATE_HOME, XDG_CACHE_HOME,
  CLAUDE_CODE_TMPDIR, TMPDIR/TMP/TEMP, LANG/LC_ALL/LC_CTYPE` (plus
  Windows-only names) — from scratch; no `ANTHROPIC_*` name is constructed
  anywhere in the module. The adapter's argv builder
  (`adapters/provider-claude/src/argv.rs:33-59`, `build_argv`) does not
  emit `--bare` either, so the `--help`-documented Keychain-free guarantee
  conditioned on that flag is not in play for production's actual argv
  shape.
- **`CHILD_ENV_ALLOWLIST` is a second, independent gate that would also
  refuse them.** `ChildEnvironment::from_explicit`
  (`crates/runner/src/lib.rs:91-111`) fails closed
  (`RunnerError::EnvironmentNameNotAllowed`) on any name outside
  `CHILD_ENV_ALLOWLIST` (`crates/runner/src/lib.rs:34-64`), which today
  contains none of `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN`,
  `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, or `CLAUDE_CONFIG_DIR`.
- **A localhost-only network scope does not exist as a Runner capability
  today.** `profile_for` (`adapters/runner-macos-seatbelt/src/lib.rs:103-248`)
  accepts only `plan.network_destinations()` equal to `[]` or exactly
  `NETWORK_SCOPE_OUTBOUND_TCP_443` (`crates/runner/src/lib.rs:27`, value
  `"tcp:443"`); any other destination set returns
  `RunnerError::BackendUnavailable` (`adapters/runner-macos-seatbelt/src/lib.rs:229-245`).
  R6/R7's local-endpoint network rule was a hand-modified profile string,
  not a reachable production code path — the evidence doc notes Seatbelt's
  own `remote tcp` primitive additionally rejects a literal IP host
  (`"host must be * or localhost in network address"`), so even a future
  loopback capability must resolve through the `localhost` keyword rather
  than a literal `127.0.0.1`.

### Conflict with currently accepted text

Three currently accepted documents state the provider-credential boundary
in a way that this ADR must reconcile, not contradict:

- `docs/architecture/providers.md` §7 ("Authentication boundary") states
  that the adapter "never becomes owner of the provider credential" and
  that secrets follow "the Secrets Broker path (reference → broker →
  trusted execution boundary) ... except where explicitly required at a
  trusted execution boundary" (`providers.md:162-171`).
- The TASK-0065 H7 closure record states "Provider-native subscription
  login stays provider-owned and is never treated as `SecretAccess`
  authority" (`docs/development/G6_PROVIDER_ENVIRONMENT_ISOLATION_H7_CLOSURE.md:120-121`).
- `docs/privacy/PRIVACY_ARCHITECTURE.md` §5 states Runlith "does not scrape
  or extract provider credentials to reuse the session" and that
  "provider-native login state cannot grant access" to the Secrets Broker
  path (`PRIVACY_ARCHITECTURE.md:114-134`).

None of these three texts names a *gateway* process as a legitimate
"trusted execution boundary" distinct from the confined provider process
itself, and none of them distinguishes "the provider CLI's own native
login session" (which Runlith must never scrape, extract, or treat as
`SecretAccess` authority — unchanged by this ADR) from "an owner-created
API key or `setup-token`, provisioned by the owner as an ordinary
application secret and never derived from the CLI's own login state"
(which this ADR proposes routing through the existing Secrets Broker path
exactly as any other application secret would be). This ADR resolves that
ambiguity by proposed amendment (see "Proposed amendments"), not by
reinterpreting the existing text silently.

## Invariants to honor

- **INV-007 / INV-008 / INV-013** — privileged operations route through
  Policy; secrets never enter model context, logs, or event payloads;
  prompt instructions are never the sole security boundary.
- **INV-076 / INV-149** — required isolation fails closed; unprovable
  mandatory restrictions fail closed rather than silently downgrading.
- **INV-093 / INV-146** — provider/model output and provider-native
  permission configuration can never grant authority; only a deterministic
  AI Engineering OS core decision can.
- **INV-099 / INV-106** — scoped credentials/capabilities never inherit
  unrestricted host secrets; web/proxy/site credentials are secret
  references, never embedded in ordinary payloads.
- **INV-147 / INV-148** — delegated capabilities are a strict subset of
  parent authority; a hard denial cannot be weakened by provider or UX
  mode.
- **INV-151** — secret material never becomes ordinary application/SQLite/
  Event Journal state; durable state stores only opaque refs and bindings.
- **INV-152** — canonical Security Audit evidence is product-authored and
  separate from operational logs and provider-visible receipts.
- **INV-023 / INV-082** — a provider remains replaceable without domain
  coupling; adding/replacing a provider must not require modifying
  unrelated domain business logic (this ADR's gateway is provider-neutral
  core with a Claude-specific adapter layer — see "Provider neutrality").

## Options evaluated

### Option A — Trusted-host credential broker injects the real credential into the child's environment

**Mechanism.** Secrets Broker releases the real API key or OAuth token to
a trusted host-side launcher, which adds it directly as
`ANTHROPIC_API_KEY` (or `CLAUDE_CODE_OAUTH_TOKEN`) to the confined child's
environment, and the provider process makes its own outbound HTTPS calls
under the existing `tcp:443` network scope — no network-scope change is
needed, only environment-layer changes.

**Threat model.** R2-R4 prove the CLI reads and transmits these variables
over a real network connection. Once the real credential is inside the
child's address space, anything that executes with the confined process's
authority — the CLI itself, or, architecturally, any future tool-use/
agentic path this ADR's marker-prompt evidence did not exercise — can
attach it to *any* outbound HTTPS request the network scope allows. This
is the decisive defect: `profile_for`'s compiled Seatbelt text is `(allow
network-outbound (literal "/private/var/run/mDNSResponder") (remote tcp
"*:443"))` (`adapters/runner-macos-seatbelt/src/lib.rs:236-238`). Seatbelt's
`remote tcp` primitive expresses a **port**, not a hostname allowlist —
there is no way to restrict `*:443` to `api.anthropic.com` specifically
within the current Runner/Seatbelt capability model. Under Option A, a
credential placed in the child's environment can be exfiltrated, via one
crafted HTTPS request, to any host reachable on port 443, not only the
intended upstream.

**Credential lifetime/scope/rotation/revocation.** Unchanged by the
mechanism itself: API-key lifetime/rotation/revocation is Console-owned;
`setup-token` mints a one-year OAuth token per Anthropic's own
documentation (see "Compliance"), with no documented revocation path.

**Secret at rest.** Same as every option: host OS credential store via
Secrets Broker/`adapters/keychain-native` (ADR-0026, ADR-0013); never
SQLite (INV-151).

**Audit.** The Secrets Broker release event is auditable (ADR-0026), but
audit facts carry "no generic payload, secret value, provider text,
environment dump" (`docs/architecture/adr/ADR-0026-g6-security-authority-and-capability-contracts.md:150-152`)
— so once the value is inside the child's environment, nothing closes the
loop on what the child process actually did with it.

**Failure semantics.** Fail closed on missing `SecretRef`/release failure,
same shape as every option.

**Owner UX.** Same one-time credential provisioning as Option B; simpler
host-side wiring (env injection vs. a running proxy), but that simplicity
is exactly what removes the host-confinement property below.

**Blast radius if leaked.** Same credential-class blast radius as Option B
(token-billed spend, or one-year subscription access), but with a
materially larger and less predictable exfiltration surface: any `*:443`
host, not one pinned upstream.

**Compatibility with `ProviderEnvironment`/Seatbelt.** Requires extending
both `ProviderEnvironment.resolved_pairs()`
(`adapters/provider-claude/src/environment.rs:142-199`) and
`CHILD_ENV_ALLOWLIST` (`crates/runner/src/lib.rs:34-64`) to admit the
secret-carrying variable names; no network-scope change. This is the
option's only implementation-simplicity advantage, and it is exactly why
its threat model is worse — the missing host-level confinement is not a
gap that a Runner/environment change can close, because Seatbelt's own
primitive has no per-host literal form.

**Implied implementation tasks.** Extend `ProviderEnvironment` to carry a
secret-classified credential pair; extend `CHILD_ENV_ALLOWLIST`; a Secrets
Broker delivery path that treats the confined child process itself as the
`SecretDelivery` target.

**Disposition: rejected.** The `*:443` host-confinement gap is
architectural, not an implementation bug this option can separately fix
while the real credential sits inside the child's address space. Closing
it would require a materially different capability (e.g., a TLS-inspecting
egress filter with SNI/host matching) that is out of scope for this ADR
and has no supporting evidence.

### Option C — Keychain/SecurityServer/`security` grant inside the sandbox

**Mechanism.** Grant the confined provider process the exact three
Seatbelt capabilities TASK-0112 measured as jointly sufficient (`mach-lookup
com.apple.SecurityServer`, `file-read-data` on the login Keychain database,
`process-exec /usr/bin/security`), letting the CLI's existing macOS
Keychain-backed authentication path work unmodified inside the sandbox.

**Threat model.** Already measured and rejected by TASK-0112: this is not
"Claude's one credential item," it is the user's *general* login-Keychain
access mechanism. A compromised confined process gains the same Keychain
authority as an interactive user session — every other Keychain item the
login keychain holds, not only the Claude credential — because Seatbelt
can scope the `security` executable's path but cannot scope which
Keychain item its argv is allowed to request.

**Credential lifetime/scope/rotation/revocation.** Whatever the host
session's own lifetime is; no attempt-scoping, no gateway-mediated
rotation, no isolation from the rest of the user's Keychain.

**Secret at rest.** The user's general login Keychain — host-wide, not a
Runlith-owned Secrets Broker item.

**Audit.** None: this is exactly the ambient, non-Policy-mediated
credential authority G6 is built to forbid (ADR-0026, ADR-0013).

**Failure semantics.** N/A — rejected outright, not a fail-closed shape
under consideration.

**Owner UX.** None required beyond the host's existing `claude` login —
superficially the best UX of the three, which is exactly the property that
makes it the worst security boundary: it reuses ambient authority instead
of an explicit, scoped grant.

**Blast radius if leaked.** Worst of the three options: a compromised
confined process gains ambient Keychain-adjacent authority, not a bounded
credential.

**Compatibility with `ProviderEnvironment`/Seatbelt.** Directly
contradicts G6 least-authority: ADR-0026 states "Runner security profiles,
if introduced, are convenience inputs that compile to canonical
capability/policy requirements. They never form a second authorization
model" (`ADR-0026:88-90`), and TASK-0112's own architecture decision is
explicit: "Do not add this capability set to the production worker
sandbox" (`G7_TASK_0112_PROVIDER_AUTH_DIAGNOSIS.md`, "Architecture decision
for remediation").

**Implied implementation tasks.** None proposed; this option is not
carried forward.

**Disposition: rejected**, per TASK-0112, unchanged by this ADR. Retained
here only because the task envelope requires evaluating it explicitly
alongside A and B.

### Option B — Trusted-host authenticated loopback proxy (the "credential gateway") — SELECTED

**Mechanism.** `agentd` hosts a per-attempt HTTP(S) listener bound only to
`127.0.0.1` on an ephemeral port (the "gateway"). The confined child
receives, in the mode-appropriate variable, a random per-attempt
placeholder value plus `ANTHROPIC_BASE_URL` pointed at that loopback
listener, and a network scope permitting outbound connections to nothing
else. The gateway holds the real credential — released from the Secrets
Broker under policy-authorized `SecretAccess` — validates the placeholder
on every request, rewrites the credential header before forwarding to the
pinned real Anthropic host, and streams the response back unmodified. This
is the exact mechanism the evidence doc's "Option B probe" section
(`docs/development/task0152-probes/owner_proxy_probe.py`) already designed
and partially exercised — its negative self-check (wrong placeholder →
local `401`, zero forwarded requests) was run; a real-credential forward
was not (see "Owner real-credential receipts").

**Threat model.** The real credential never enters the confined child's
address space, environment, or filesystem — only a random, non-secret-
classified, per-attempt placeholder does (rule D2 below). A compromised
confined child can, at most, replay its own already-possessed placeholder
back to the gateway (no incremental authority — the gateway already
permits exactly this for this attempt) or attempt a connection outside the
allowed loopback port (denied at the OS boundary, same enforcement class
as today's `*:443` rule, just narrower). Exfiltrating the placeholder
off-host is useless without also reaching the gateway's own loopback
address, unreachable from outside the host. The gateway itself becomes a
new trusted-host component: the owner-run probe's round-3 hardening
already closes the two sharpest gaps empirically — the forwarding opener
never follows a redirect (a 3xx from upstream is relayed to the child
exactly as received rather than triggering a second request, verified
against a throwaway local redirecting server) and it ignores ambient proxy
environment variables (`ProxyHandler({})`), so `http_proxy`/`https_proxy`
cannot reroute the credential-bearing request. What the probe does **not**
close — stated as a documented limitation, not glossed over — is a
same-host, same-user concurrent process guessing or racing the placeholder
before the legitimate child does (no TOCTOU/concurrency analysis was
performed); this is carried into "Security impact" below as a residual
risk, not silently assumed away.

**Credential lifetime/scope/rotation/revocation.**

- *API key*: Console-scoped, Console-rotated/revoked; the gateway does not
  change the credential's own lifetime, only bounds its *exposure* to one
  process that never crosses a trust boundary except as one HTTPS header
  it constructs itself.
- *OAuth `setup-token`*: a one-year OAuth token per Anthropic's own
  documentation (see "Compliance"); no documented revocation path (open
  question, unchanged by which option is selected).
- *Placeholder* (distinct from both credential classes above): lives
  exactly one attempt — minted at gateway start, invalid at gateway
  shutdown, never persisted, never a `SecretRef` (rule D2).

**Secret at rest.** Identical to every option: host OS credential store
via Secrets Broker/`adapters/keychain-native` (ADR-0026, ADR-0013); never
SQLite (INV-151). The placeholder is not secret-classified at all — it
authorizes nothing once the attempt ends, so it is not subject to this
requirement.

**Audit (ADR-0026 Security Audit facts).** The gateway is the natural
place to record, per attempt: the `SecretAccess` release event (already
the existing Secrets Broker/Policy audit surface), gateway bind/lifetime
(port, start/stop timestamps — never the placeholder or credential
value), and every locally-rejected request (`401`/`403`) by method and
path only — consistent with the Security Audit schema's constraint that it
carries "no generic payload, secret value, provider text, environment
dump or transcript field" (`ADR-0026:150-152`).

**Failure semantics.** Fail closed: no fallback to the host's own
authenticated provider session, no fallback to `*:443`, no retry with a
different credential or credential class. If the gateway cannot bind its
port, or no credential `SecretRef` is bound for the attempt, the existing
TASK-0114 refusal fires exactly as it does today, before any provider work
(rule D14).

**Owner UX.** One-time setup per credential class, unchanged from Option
A: the owner creates a Keychain item for an API key, or generates a
`claude setup-token` themselves (Runlith never runs that command and never
sees the token) and stores the resulting value the same way, mirroring the
evidence doc's "Owner commands" pattern (`-w` as the last `security`
argument so the value is typed only at an interactive prompt, never in
argv or shell history).

**Blast radius if leaked.** Bounded to the credential class described
above; critically, "leaked" here means the *real* credential being
exfiltrated from the gateway process itself — a trusted-host component
under the same authority `agentd` already has, not a new exposure class —
never from the confined child, which never holds a value whose leak has
this blast radius. A leaked *placeholder* grants no authority beyond the
current attempt on the current host.

**Compatibility with `ProviderEnvironment` isolation and the Seatbelt
profile.** `ProviderEnvironment` needs the mode-appropriate placeholder
variable name plus `ANTHROPIC_BASE_URL` added to its closed pair list
(`adapters/provider-claude/src/environment.rs:142-199`) and to
`CHILD_ENV_ALLOWLIST` (`crates/runner/src/lib.rs:34-64`) — smaller in
security weight than Option A's equivalent change because the values
carried are never secret-classified, but the same class of reviewed,
explicit allowlist change either way. The network side is the harder gap:
`profile_for` (`adapters/runner-macos-seatbelt/src/lib.rs:103-248`) accepts
only `[]` or exactly `NETWORK_SCOPE_OUTBOUND_TCP_443`
(`crates/runner/src/lib.rs:27`) today, failing closed
(`RunnerError::BackendUnavailable`) on anything else
(`adapters/runner-macos-seatbelt/src/lib.rs:229-245`); a loopback-port
scope does not exist as a Runner capability at all yet — confirmed
empirically by R6/R7, which needed a hand-modified profile outside any
reachable production code path. This ADR's Decision (rule D11) proposes
the missing capability; until it ships, Option B is not reachable through
the current codebase, exactly as Option A is blocked on its own
environment-allowlist gap.

**Implied implementation tasks.** Enumerated in "Implementation task list"
below.

## Decision: Option B — confined Claude provider authentication through a trusted-host credential gateway

The owner selects **Option B**, as a proposed decision pending ratification.
The following normative rules define the gateway's required shape. They
formalize the design already probed by
`docs/development/task0152-probes/owner_proxy_probe.py` (self-check
exercised; real-credential forward pending — see "Owner real-credential
receipts") rather than inventing a new, unprobed design.

- **D1.** The gateway is owned by `agentd`, created fresh per provider
  attempt, and binds a TCP listener to `127.0.0.1` only, on an ephemeral
  (OS-assigned) port. It never binds any non-loopback interface.
- **D2.** At gateway start, `agentd` mints a random per-attempt placeholder
  credential value and delivers it to the confined child in the
  mode-appropriate variable — `ANTHROPIC_API_KEY` for API-key mode,
  `CLAUDE_CODE_OAUTH_TOKEN` for OAuth mode — plus `ANTHROPIC_BASE_URL`
  pointed at the gateway's bound loopback address and port. The placeholder
  is an attempt-scoped capability, never a `SecretRef`; it is never
  persisted beyond the attempt and is redacted from logs and Event Journal
  payloads like any other execution-scoped value, not because it is
  secret-classified but because it is a bearer capability for the
  attempt's own gateway.
- **D3.** The gateway compares an incoming request's credential header
  against the current placeholder using a constant-time comparison (the
  probe's `hmac.compare_digest` on bytes is the reference shape). A
  mismatch is rejected locally with `401`, never forwarded.
- **D4.** The gateway allowlists exactly two request shapes from the
  child: `HEAD`/`GET /api/hello`, answered locally without forwarding; and
  `POST /v1/messages` (any query string), forwarded only on a placeholder
  match per D3. Every other method or path is rejected locally with `403`
  and recorded to Security Audit (method + path only, per the Audit
  requirement above).
- **D5.** A forwarded request has its credential header rewritten before
  leaving the gateway: API-key mode sets `x-api-key` to the real key and
  drops any `authorization` header; OAuth mode sets `authorization: Bearer
  <real>` and drops any `x-api-key`. `anthropic-beta` and
  `anthropic-version` are forwarded exactly as the child sent them — R8
  already proved the CLI itself decides the correct per-mode flag set, so
  the gateway never adds or guesses one.
- **D6.** The upstream host is pinned (the real Anthropic API host); the
  gateway uses default TLS certificate verification (no custom trust
  override) and never follows a redirect from upstream — a `3xx` response
  is relayed to the child exactly as received rather than triggering a
  second outbound request.
- **D7.** The gateway ignores ambient proxy environment variables
  (`http_proxy`/`https_proxy` and equivalents) for its own outbound
  connection to upstream, so a host-level proxy setting cannot silently
  reroute the credential-bearing request.
- **D8.** A streaming (`text/event-stream`) upstream response is relayed
  to the child as a real byte-at-a-time stream, not buffered and re-sent; a
  non-streaming response is buffered once and relayed with a recomputed
  `Content-Length`. Hop-by-hop headers are stripped in both directions.
- **D9.** The gateway enforces request/response body size limits and a
  bounded request count per attempt, sufficient for one non-interactive
  provider turn; exceeding either is a local rejection, never an
  unbounded relay.
- **D10.** The gateway's lifetime is bound to the attempt: its listener
  closes and its placeholder becomes invalid at attempt end (success,
  failure, cancellation, or timeout alike). No gateway outlives the
  attempt that created it.
- **D11.** `crates/runner` gains a new provider-neutral network scope
  (structurally parallel to `NETWORK_SCOPE_OUTBOUND_TCP_443`,
  `crates/runner/src/lib.rs:27`) representing "loopback TCP, exact port,"
  compiled by the macOS Seatbelt backend
  (`adapters/runner-macos-seatbelt/src/lib.rs:103-248`) to an exact
  `(remote tcp "localhost:<port>")` rule bound to the gateway's actual
  listening port — replacing `tcp:443` for a gateway-authenticated
  provider launch, never adding to it. Any Runner backend that cannot
  express this scope fails closed (`RunnerError::BackendUnavailable`),
  exactly as `profile_for` already fails closed on every unsupported
  destination set today (`adapters/runner-macos-seatbelt/src/lib.rs:229-245`).
  This is new capability surface, not a reinterpretation of the existing
  `tcp:443` rule; a non-gateway provider launch keeps its current `tcp:443`
  scope unchanged.
- **D12.** The real credential is released by the Secrets Broker
  (`crates/secrets`, policy-authorized `Capability::SecretAccess`, per
  ADR-0026) only to the gateway process — never to the confined child, and
  never through `ProviderEnvironment`/`CHILD_ENV_ALLOWLIST`, both of which
  carry only the non-secret placeholder and `ANTHROPIC_BASE_URL`.
- **D13.** `ProviderEnvironment` stays secret-free: the placeholder added
  to its resolved pairs (`adapters/provider-claude/src/environment.rs:142-199`)
  is not a secret value, does not require Secrets Broker mediation to
  construct, and is not subject to INV-008/INV-151's secret-handling
  obligations — only D2's attempt-scoping and redaction obligations.
- **D14.** The existing TASK-0114 refusal
  (`provider_authentication_unsupported_in_mandatory_confinement`) remains
  the exact behavior whenever no credential `SecretRef` is bound for the
  attempt, or the gateway cannot start (port bind failure, Secrets Broker
  release failure, or any other gateway-startup error). `AUTH_CONFINEMENT_VERDICT`
  (`bins/agentd/src/execution.rs:159-165`) moves from `Unsupported` to
  `Compatible` only for a launch that actually has a bound gateway and a
  released credential — never unconditionally for the macOS platform as a
  whole.

## Provider neutrality

Per AGENT.md §13, the gateway's core (D1, D3, D4, D6-D11, D14) is
provider-neutral request-forwarding/placeholder-validation machinery that
could host any HTTP-based provider credential, not Claude-specific logic
baked into `agentd`. The Claude-specific pieces — which header carries the
credential per mode (D5), which variable names to deliver (D2), and the
pinned upstream host — live behind the `provider-claude` adapter, which
supplies them to the shared gateway core rather than the core branching on
`provider == "claude"`. A second HTTP-based provider would supply its own
header/variable/host mapping to the same gateway core; this ADR does not
generalize that contract beyond Claude, since no second provider's shape
is evidenced here (INV-023, INV-082).

## Compliance

Anthropic's Agent SDK overview states that it "does not allow third party
developers to offer claude.ai login or rate limits" for their own products
absent prior approval, and directs integrators toward API-key
authentication instead (<https://docs.claude.com/en/api/agent-sdk/overview>,
retrieved 2026-09-25).

Anthropic's Claude Code IAM documentation (retrieved 2026-09-25,
<https://docs.claude.com/en/docs/claude-code/iam>) documents three
credential inputs in precedence order: a cloud provider's own variables
highest, then `ANTHROPIC_AUTH_TOKEN` sent as an `Authorization: Bearer`
header, then `ANTHROPIC_API_KEY` sent as an `X-Api-Key` header — matching
R8's directly observed header shapes exactly. The same page documents
`claude setup-token` as generating a one-year OAuth token that the CLI
does not save anywhere itself, tied to a Pro/Max/Team/Enterprise
subscription, and scoped to making model requests only. It does not
document a revocation procedure for that token (carried into "Open
questions" below, unresolved by this ADR).

Anthropic's LLM-gateway documentation (retrieved 2026-09-25,
<https://docs.claude.com/en/docs/claude-code/llm-gateway>) describes a
gateway-supplied credential or an `apiKeyHelper` as replacing the
subscription login for that session, notes that setting only a base URL
does not by itself replace the subscription, and states that a gateway
relaying this traffic must forward the OAuth capability the CLI advertises
in its `anthropic-beta` header — consistent with D5/R8 above: the gateway
never invents this flag set, it only relays what the CLI already sent.

Anthropic's Commercial Terms govern API-key use; the `claude.ai`
subscription used by `setup-token` falls under the Consumer Terms
(`anthropic.com/legal/commercial-terms`). This ADR states this as context
only; it draws no independent legal conclusion beyond it, and is not a
substitute for the owner's own review of Anthropic's primary terms.

**Compliance risk, owner-accepted (2026-09-25).** The owner has read the
above and explicitly accepts the compliance risk of running the
`setup-token` (OAuth subscription) credential class for their own personal
use of Runlith under their own Claude subscription. This acceptance is the
owner's own risk decision; it is recorded here, not concluded by this ADR,
and this ADR is not a substitute for the owner's own legal review.

**Proposed for ratification, not yet owner-ratified:** the `setup-token`
credential class is constrained to the owner's own account, used only on
the owner's own machine, exclusively through this ADR's gateway (D1-D14) —
never a raw HTTP adapter that reuses the token outside the gateway's
single-host, single-attempt design — and must not be offered to any other
user as a product sign-in or authentication feature without separate,
explicit Anthropic approval. The API-key credential class remains the
default and is the *only* class available in any distributed or
multi-user Runlith configuration; the `setup-token` class is never enabled
by default and never exposed as a general product feature.

## Owner real-credential receipts: PENDING

Every automated probe in
`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md` used a dummy or no
credential. No row in that document, and nothing in this ADR, is evidence
that a *real* Claude credential completes an authenticated round trip
through the Option B gateway. That gap is closed only by the owner running
the two commands the evidence doc already documents, on their own host,
with their own credentials; this task does not run them and does not
fabricate a result:

```bash
python3 task0152-probes/owner_proxy_probe.py api-key --service runlith-task0152-owner-anthropic-api-key
python3 task0152-probes/owner_proxy_probe.py oauth   --service runlith-task0152-owner-oauth-token
```

(One-time Keychain setup for both credential classes, and the exact
command shapes, are recorded in
`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md`, "Option B probe",
"Owner commands" — owner-run, not run by this task.)

**Status: PENDING.** Ratification of this ADR requires the owner to record
the result of both invocations (pass/fail, `forwarded` count,
`upstream_status_codes`, and confirmation that no credential value was
printed) in this section or a linked follow-up record before the owner
accepts this decision. Ratifying without that evidence would violate
AGENT.md §5 (evidence over claims).

## Proposed amendments (pending ratification)

This ADR does not edit the files below; editing them is a listed
implementation follow-up (see "Implementation task list", "Doc
amendments"). It states the exact superseding text now so the follow-up
has no interpretive latitude.

**`docs/architecture/providers.md` §7 ("Authentication boundary")** — append
after the existing paragraph (`providers.md:162-171`, text unchanged above
this point):

> Under mandatory G6 confinement, the confined provider process itself is
> never the trusted execution boundary referenced above: per ADR-0037, the
> trusted execution boundary is an `agentd`-owned, per-attempt,
> loopback-only credential gateway. The gateway — not the provider CLI,
> and not the host's own provider-native login session — is the sole
> recipient of a Secrets-Broker-released Claude credential (an Anthropic
> Console API key or the owner's own `claude setup-token` OAuth token);
> the confined child receives only a non-secret, attempt-scoped
> placeholder.

**TASK-0065 H7 closure
(`docs/development/G6_PROVIDER_ENVIRONMENT_ISOLATION_H7_CLOSURE.md`,
"Sanitized environment and secret handling")** — append after "Provider-
native subscription login stays provider-owned and is never treated as
`SecretAccess` authority" (`:120-121`, text unchanged above this point):

> Per ADR-0037, when the owner has separately provisioned their own Claude
> API key or `setup-token` as an explicit, owner-created `SecretRef`
> (never scraped or copied from a provider-native login session), that
> owner-provisioned credential may be released by the Secrets Broker under
> policy-authorized `SecretAccess` — but only to the ADR-0037 credential
> gateway, never to the confined provider process, and never derived from
> or substituting for the provider's own native login state.

**`docs/privacy/PRIVACY_ARCHITECTURE.md` §5 ("Provider-owned
credentials")** — append after "...outside our application's control and
never presented as ours (PRIV-004)." (`PRIVACY_ARCHITECTURE.md:141-144`,
text unchanged above this point):

> Per ADR-0037, this section's provider-owned-credential principle governs
> the provider CLI's own native login session only. It does not forbid the
> owner from separately creating their own Claude API key or `setup-token`
> as an explicit, owner-authored credential stored under Runlith's own
> Keychain service identity — never copied or derived from the provider
> CLI's own login state — and registered as an ordinary `SecretRef`. Such
> a credential follows the same secret reference → Policy → Secrets Broker
> → trusted execution boundary path as any other application secret; per
> ADR-0037 the only trusted execution boundary that may receive it is the
> per-attempt credential gateway, and it reaches the confined provider
> process only as a non-secret, attempt-scoped placeholder.

## Consequences

- Production can run the Claude provider under mandatory G6 macOS
  confinement without granting Keychain, SecurityServer, or
  `/usr/bin/security` authority, once the implementation follow-ups below
  land — resolving the blocker TASK-0112/TASK-0114 left open.
- `crates/runner` gains a second network-scope capability (D11), narrower
  than `tcp:443` in the specific sense that it names one loopback port, not
  a broad remote host class; it does not widen what any *other* launch
  purpose may reach.
- `ProviderEnvironment` and `CHILD_ENV_ALLOWLIST` each gain a small,
  reviewed set of new allowlisted names, all non-secret-classified
  (placeholder + `ANTHROPIC_BASE_URL`).
- `agentd` gains a new trusted-host component (the gateway) with its own
  bounded lifetime, request allowlist, and audit surface — a new piece of
  reviewable code, not a new authority model (it composes existing Secrets
  Broker/Policy primitives, per D12).
- `providers.md` §7, the H7 closure record, and `PRIVACY_ARCHITECTURE.md`
  §5 each require the amendment text above once this ADR is ratified.
- Option A and Option C remain explicitly rejected; adopting either later
  would require a superseding ADR, not a silent implementation choice.

## Security impact

Residual risks, not resolved by this ADR and not glossed over:

- **Confused-deputy spend through the gateway during an attempt.** Any
  process able to reach `127.0.0.1:<port>` with the correct placeholder
  during the attempt's lifetime can spend against the real credential
  through the gateway's allowlisted `/v1/messages` path. The intended
  caller is the one confined child the attempt launched; nothing in D1-D14
  cryptographically binds the placeholder to that specific process beyond
  "whoever currently holds the placeholder bytes."
- **Same-user local processes that learn the placeholder during an
  attempt.** The probe's own limitation statement is carried forward
  unresolved: no concurrency/TOCTOU analysis exists for a hostile,
  same-user, same-host process racing the legitimate child to use a
  captured placeholder before attempt end. D10's short attempt-bound
  lifetime narrows the exposure window; it does not eliminate it.
- **Data sent to Anthropic via prompts is inherent to using the provider
  at all**, unaffected by which of these options is chosen — out of this
  ADR's scope.
- **Provider `WebFetch`/`WebSearch` and any other non-`/v1/messages`
  outbound network use are unavailable in confined mode.** D4's allowlist
  is deliberately narrow (`/api/hello`, `/v1/messages`); any CLI feature
  that needs a different outbound destination is refused locally (`403`)
  under this design, a capability regression relative to an unconfined
  host session.
- **OAuth refresh behavior over a long-lived session is untested.** R8's
  "no other outbound destination observed" finding is scoped to one short,
  non-interactive, marker-prompt invocation; a longer-lived attempt
  approaching real token expiry was never exercised and could need an
  outbound call this design's allowlist does not yet account for (same
  caveat the evidence doc records verbatim).

## Migration impact

This is a docs-only ADR: no schema, storage, or `completed_through`/
`version.toml` change is made or authorized here. No existing provider
launch path changes today. Once ratified, the implementation follow-ups
below are additive: a non-gateway-authenticated provider launch keeps its
current `tcp:443` scope and its current `Unsupported` refusal on macOS
confinement unless and until a gateway-backed launch path is separately
wired in and the platform's `AUTH_CONFINEMENT_VERDICT` is updated for that
specific launch shape (D14). Rollback is trivial at the ADR level: if the
implementation follow-ups are never built, behavior is unchanged from
today — production continues to refuse mandatory-confinement Claude
launches exactly as TASK-0114 already does.

## Implementation task list (proposed follow-ups)

Not created as queue tasks by this ADR; the orchestrator registers them
separately after ratification. Each depends on this ADR's ratification
(and the owner's real-credential receipts, above).

- **Runner loopback network scope.** Add the D11 capability to
  `crates/runner` and compile it in `adapters/runner-macos-seatbelt`;
  adversarial tests proving no destination outside the exact bound port is
  reachable, and that every other backend fails closed on the new scope
  until it implements it. Depends on: this ADR's ratification.
- **Credential gateway component.** Implement D1-D10 as a new `agentd`-
  owned component with its own targeted test suite (self-check,
  method/path allowlist, header rewrite per mode, redirect/proxy-env
  hardening, streaming passthrough, size/count limits, attempt-bound
  lifetime). Depends on: this ADR's ratification.
- **Secrets Broker binding/provisioning path for both credential
  classes.** An owner-run CLI/setup path that creates the `SecretRef` for
  an API key or a `setup-token` without ever placing the value in argv or
  shell history (mirroring the evidence doc's `-w`-last pattern). Depends
  on: this ADR's ratification.
- **`provider-claude` environment + `CHILD_ENV_ALLOWLIST` changes.** Add
  the placeholder variable(s) and `ANTHROPIC_BASE_URL` to
  `ProviderEnvironment` and to `CHILD_ENV_ALLOWLIST`, gated on a
  gateway-backed launch shape only (D13). Depends on: gateway component.
- **Security Audit facts.** Extend `crates/security-audit`'s typed
  vocabulary to cover gateway bind/lifetime and locally-rejected requests,
  per the Audit requirement above. Depends on: gateway component.
- **Doc amendments.** Apply the exact superseding text in "Proposed
  amendments" to `providers.md` §7, the H7 closure record, and
  `PRIVACY_ARCHITECTURE.md` §5. Depends on: this ADR's ratification (text
  is already fixed by this ADR; the follow-up is mechanical).
- **Real-provider production G6 smoke, both credential classes.** A
  production-shaped smoke test exercising the gateway with a real
  (owner-provisioned, non-committed) credential for each class, verifying
  a genuine `2xx` round trip and that `AUTH_CONFINEMENT_VERDICT` correctly
  reports `Compatible` only for the gateway-backed launch shape. Depends
  on: all of the above.
- **Removal of the unconditional refusal.** Update
  `bins/agentd/src/execution.rs:159-165`'s macOS
  `AUTH_CONFINEMENT_VERDICT` to `Compatible` for a gateway-backed launch
  only once the smoke task above is green; the `Unsupported` path (and its
  TASK-0112 reasoning) remains unchanged for any launch that is not
  gateway-backed. Depends on: real-provider production smoke.

## Open questions

- **`setup-token` revocation path.** Not established by any primary
  source read for this ADR (see "Compliance"). Blocks a confident
  incident-response story for a leaked OAuth token until resolved.
- **OAuth refresh over long-lived sessions.** Untested by any probe in
  `docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md` (see "Security
  impact"); a longer attempt may need an outbound call this ADR's
  allowlist (D4) does not yet account for.
- **Linux/Windows Runner backends.** Out of scope. D11's loopback network
  scope is specified provider-neutrally in `crates/runner`, but only the
  macOS Seatbelt compilation is evidenced here; any other backend must
  implement or fail closed on the new scope, per D11, before a
  gateway-backed launch is reachable on that platform.

## References

- `docs/development/G7_TASK_0112_PROVIDER_AUTH_DIAGNOSIS.md` — root cause,
  capability ablation, rejected remediation (Option C).
- `docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md` — R0-R8, Option B
  probe design and hardening, "Owner commands", "What this does NOT
  prove".
- `docs/development/task0152-probes/owner_proxy_probe.py`,
  `owner_positive_probe.sh` — probed mechanics this Decision formalizes.
- `docs/development/G6_SECRETS_BROKER.md` — Secrets Broker authority/trust
  boundary (`SecretRef`, `SecretBinding`, `SecretDelivery`).
- `docs/development/G6_PROVIDER_ENVIRONMENT_ISOLATION_H7_CLOSURE.md` —
  amended text target; `ProviderEnvironment` architecture.
- `docs/architecture/providers.md` §7-9 — amended text target;
  capability/availability vocabulary.
- `docs/privacy/PRIVACY_ARCHITECTURE.md` §5 — amended text target.
- `docs/architecture/adr/ADR-0013-policy-engine-os-isolation.md` — secrets
  live in OS credential store, never SQLite.
- `docs/architecture/adr/ADR-0020-local-ipc-peer-authentication.md` —
  local trusted-boundary authentication precedent.
- `docs/architecture/adr/ADR-0024-g5-provider-execution-seam.md` —
  `agentd` as composition root for concrete provider adapters.
- `docs/architecture/adr/ADR-0026-g6-security-authority-and-capability-contracts.md` —
  canonical Policy/Secrets Broker/Security Audit authority model this
  ADR's gateway composes rather than replaces.
- `external-research/studies/2026-09-19-provider-authentication-boundary.md`
  §F-5 — non-authoritative compliance caution; informed the "Compliance"
  section's owner-risk framing, not adopted as a decision input on its
  own.
- `adapters/runner-macos-seatbelt/src/lib.rs:103-248` — `profile_for`,
  network-scope fail-closed match (`:229-245`).
- `crates/runner/src/lib.rs:27` (`NETWORK_SCOPE_OUTBOUND_TCP_443`),
  `:34-64` (`CHILD_ENV_ALLOWLIST`), `:91-111` (`from_explicit`).
- `adapters/provider-claude/src/environment.rs:142-199`
  (`resolved_pairs`), `:219-237` (isolation support roots).
- `adapters/provider-claude/src/argv.rs:33-59` (`build_argv`, no `--bare`
  in production argv).
- `adapters/provider-claude/src/authentication.rs:58-60`
  (`permits_launch`), `:94` (`probe_authentication`).
- `bins/agentd/src/execution.rs:88-89` (refusal constant), `:98-111`
  (`ConfinementAuthVerdict`), `:159-165` (macOS verdict), `:816-826`
  (refusal call site in `prepare_provider_launch`), `:3259-3301`
  (`SecuredClaudeExecutor::execute`), `:3343-3413` (provider launch
  capability composition, including the current hardcoded `tcp:443`
  request).
- `docs/architecture/INVARIANTS.md` — INV-007, INV-008, INV-013, INV-023,
  INV-076, INV-082, INV-093, INV-099, INV-106, INV-146, INV-147, INV-148,
  INV-149, INV-151, INV-152.
- `planning/tasks/queue.yaml` — TASK-0152 (this task), TASK-0112,
  TASK-0114 (`depends_on`).
