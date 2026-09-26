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
> review. The owner's own real-credential gateway probe has now run
> successfully for both credential classes and is recorded in "Owner
> real-credential receipts", alongside an operator-error exposure of the
> OAuth token used, the owner's scheduled revocation of it, and the round-4
> hardening that exposure triggered — carried into the hardening rules
> below (D15-D22, D25-D26). This revision also remediates two independent
> ADR_REVIEW_FAIL findings sets (B1-B3 blocking, N1-N10 non-blocking; see
> each D-rule and the Security impact section for the finding it fixes). No
> real credential value or the owner's own Keychain item was ever read;
> `security` and this ADR's own probe scripts were invoked, but only
> against throwaway temp keychains holding dummy/fake values, never the
> owner's Keychain or a real credential (`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md`,
> "Option B probe" preamble).

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

Four currently accepted documents state the provider-credential/
authentication boundary in a way that this ADR must reconcile, not
contradict:

- `docs/architecture/providers.md` §7 ("Authentication boundary") states
  that the adapter "never becomes owner of the provider credential" and
  that secrets follow "the Secrets Broker path (reference → broker →
  trusted execution boundary) ... except where explicitly required at a
  trusted execution boundary" (`providers.md:162-171`).
- `docs/architecture/providers.md` §9a ("Authentication state") binds
  "Authentication is a **precondition verified before launch**" to the
  host's own provider-native `claude auth status` state — `UNAUTHENTICATED`
  or `UNKNOWN` refuses closed, unconditionally, for every launch, before
  `AUTH_CONFINEMENT_VERDICT` is even consulted (`providers.md`, §9a; code
  path: `bins/agentd/src/execution.rs:816-826`, which calls
  `provider_claude::probe_authentication` first and only reaches the
  confinement-verdict check afterward). §9a's own text already
  acknowledges D14's refusal as a *second, later* gate ("A launch may
  satisfy §9a and still fail when the profile denies the mechanism the
  provider authenticates through") — but it does not contemplate a launch
  shape where the host's own login state is never the relevant fact at
  all. D14 alone, which only flips `AUTH_CONFINEMENT_VERDICT`, cannot fix
  this: the earlier §9a gate would still run first and still consult the
  wrong precondition for a gateway-backed launch.
- The TASK-0065 H7 closure record states "Provider-native subscription
  login stays provider-owned and is never treated as `SecretAccess`
  authority" (`docs/development/G6_PROVIDER_ENVIRONMENT_ISOLATION_H7_CLOSURE.md:120-121`).
- `docs/privacy/PRIVACY_ARCHITECTURE.md` §5 states Runlith "does not scrape
  or extract provider credentials to reuse the session" and that
  "provider-native login state cannot grant access" to the Secrets Broker
  path (`PRIVACY_ARCHITECTURE.md:114-134`).

None of these four texts names a *gateway* process as a legitimate
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
- **INV-069 / INV-082** — stable extension contracts live in a small
  provider-neutral layer; adding/replacing a provider must not require
  modifying unrelated domain business logic (this ADR's gateway is
  provider-neutral core with a Claude-specific adapter layer — see
  "Provider neutrality"). (INV-023, "Task may migrate between providers
  without state loss," does not itself apply here and is not cited for
  this point.)

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

**Mechanism.** `agentd` hosts a per-attempt HTTP(S) listener component
(D-rule N7 below: in-process within `agentd`, not a separate helper
process) bound to loopback addresses only (D11's true destination set, not
"127.0.0.1 only" — see D11), on an ephemeral port (the "gateway"). The
confined child receives, in the mode-appropriate variable, a random
per-attempt placeholder value plus `ANTHROPIC_BASE_URL` pointed at that
loopback listener, and a Seatbelt network scope permitting outbound
connections to nothing but that one port on any local address the rule
admits (D11). This confines the *transport*: it does not, by itself, limit
what the real Anthropic API is asked to do with a forwarded request's JSON
body — see D24 and "Security impact" for the owner-accepted residual risk
this leaves open (server-side tools, the MCP connector). The gateway holds
the real credential — released from the Secrets Broker under
policy-authorized `SecretAccess` — validates the placeholder on every
request, rewrites the credential header before forwarding to the pinned
real Anthropic host, and streams the response back unmodified. This
is the exact mechanism the evidence doc's "Option B probe" section
(`docs/development/task0152-probes/owner_proxy_probe.py`) designed and the
owner has now run end-to-end with a real credential for both modes: the
negative self-check (wrong placeholder → local `401`, zero forwarded
requests) and one legitimate forwarded request reaching the real Anthropic
API with a genuine `200` (see "Owner real-credential receipts").

**Threat model.** The real credential never enters the confined child's
address space, environment, or filesystem — only a random, non-secret-
classified, per-attempt placeholder does (rule D2 below). A compromised
confined child can, at most, replay its own already-possessed placeholder
back to the gateway (no incremental authority — the gateway already
permits exactly this for this attempt) or attempt a connection outside the
allowed destination set (denied at the OS boundary — but see D11: that
set is broader than "loopback," admitting every local address the host
happens to have, not only `127.0.0.1`/`::1`). Exfiltrating the placeholder
off-host is useless without also reaching one of the addresses the gateway
binds; a same-host, same-user process is a different question (below).
The gateway itself becomes a new trusted-host component: the owner-run
probe's hardening already closes several gaps empirically — the forwarding
opener never follows a redirect (a 3xx from upstream is relayed to the
child exactly as received rather than triggering a second request — this
no-redirect *design property* was verified against a throwaway local
redirecting stub in round 3, and was **not** separately re-exercised by the
real-credential e2e test, whose 9 responses were all `401`, never `3xx`;
see "What this proves" in the evidence doc's e2e section) and it ignores
ambient proxy environment variables (`ProxyHandler({})`), so
`http_proxy`/`https_proxy` cannot reroute the credential-bearing request.
What the probe does **not** close — stated as a documented limitation, not
glossed over — is a same-host, same-user concurrent process guessing or
racing the placeholder before the legitimate child does (no TOCTOU/
concurrency analysis was performed), nor any **other-uid local attacker**
on the same host: this ADR's threat model assumes the confined child and
any co-resident attacker share the launching user's uid (matching G6's
existing trust model elsewhere); a different-uid local process is out of
scope, same as for every other Runner-enforced boundary. This is carried
into "Security impact" below as a residual risk, not silently assumed
away.

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
value), and every locally-rejected request (`401`/`403`) by method plus a
fixed rejection-reason category and a truncated digest of the offending
path/query — never the raw child-supplied value (D4) — consistent with the
Security Audit schema's constraint that it carries "no generic payload,
secret value, provider text, environment dump or transcript field"
(`ADR-0026:150-152`).

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
(`adapters/runner-macos-seatbelt/src/lib.rs:229-245`); the needed scope
does not exist as a Runner capability at all yet — confirmed empirically
by R6/R7 (hand-modified profile) and, more precisely, by round-6's
credential-free "L1 loopback admission" measurement, which shows the
underlying Seatbelt primitive a future scope would compile to admits
every local address the host has configured, not only `127.0.0.1`
(`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:284-333`). This
ADR's Decision (rule D11) proposes the missing capability, with that true
destination set stated explicitly rather than assumed narrower; until it
ships, Option B is not reachable through the current codebase, exactly as
Option A is blocked on its own environment-allowlist gap.

**Implied implementation tasks.** Enumerated in "Implementation task list"
below.

## Decision: Option B — confined Claude provider authentication through a trusted-host credential gateway

The owner selected **Option B** on 2026-09-25, in a structured decision
prompt, as a proposed decision pending ratification. The following
normative rules define the gateway's required shape. They formalize the
design already probed by `docs/development/task0152-probes/owner_proxy_probe.py`
and now run end-to-end with a real credential for both modes (see "Owner
real-credential receipts") rather than inventing a new, unprobed design.

**Process boundary (orchestrator decision, flagged for owner ratification
— N7).** The gateway is a component *inside* the existing `agentd`
process, not a separate helper process. The Secrets Broker releases the
real credential through its existing `SecretDeliveryTarget::Execution`
target to a trusted in-process delivery callback (`crates/secrets`,
ADR-0026) — no new IPC or process-spawn surface is added to carry the
secret. **Considered alternative: a separate helper process.** Trade-offs:
a separate process could hold its own distinct code-signing identity for
D22's Keychain ACL (narrower than trusting all of `agentd`) and would
scope D19's core-dump disabling to that helper alone rather than to
`agentd`'s whole process — but it requires a new local IPC channel to
carry the released secret from `agentd` to the helper (a channel this ADR
would then have to secure), a second process lifecycle to manage per
attempt, and a second binary's code-signing identity to provision and
verify. This ADR selects the in-process shape as the smaller change
against the existing Secrets Broker contract; the separate-process
alternative is recorded here, not ratified, in case a future security
review reaches a different conclusion.

- **D1.** The gateway is owned by `agentd`, created fresh per provider
  attempt, on an ephemeral (OS-assigned) port. It binds every local
  address D11's compiled network rule admits (at minimum `127.0.0.1` and
  `[::1]`; see D11 for the full set and the residuals this does not
  cover), and never binds a non-loopback interface address itself, even
  though the Seatbelt rule may separately admit connections toward one
  (D11).
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
  child: `HEAD`/`GET /api/hello`, answered locally without forwarding
  (unauthenticated — see N4 in "Security impact": this reveals only that a
  Runlith gateway is listening, never a credential or attempt detail); and
  `POST /v1/messages` whose query string is exactly empty or exactly
  `beta=true`, forwarded only on a placeholder match per D3. Every query
  string R6/R7/R8 actually **observed** the real CLI send was `beta=true`
  — never bare/empty in any measured row
  (`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:196,206,236`);
  the empty-string branch is allowed defensively, by design, not because it
  was itself observed (reference implementation: `ALLOWED_QUERY_STRINGS =
  ("", "beta=true")`, `docs/development/task0152-probes/owner_proxy_probe.py:269`).
  Any other query string, method, or path is rejected locally with `403`
  and recorded to Security Audit as a fixed category plus a truncated,
  non-reversible digest of the raw value — **never the raw child-supplied
  path, query, or header value itself** (round-6 finding T2d: recording a
  child-controlled string verbatim turns the audit trail into an
  attacker-steered logging channel;
  `docs/development/task0152-probes/owner_proxy_probe.py:505-514`,
  `_hash_path_for_logging`). This resolves the tension a raw-path audit
  record would otherwise create against D21's stricter "never in any form"
  posture for the placeholder: nothing child-controlled or secret is ever
  recorded raw, only a category and a digest.
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
- **D11 (corrected — B3).** `crates/runner` gains a new provider-neutral
  network scope (structurally parallel to `NETWORK_SCOPE_OUTBOUND_TCP_443`,
  `crates/runner/src/lib.rs:27`) compiled by the macOS Seatbelt backend
  (`adapters/runner-macos-seatbelt/src/lib.rs:103-248`) to an exact
  `(remote tcp "localhost:<port>")` rule bound to the gateway's actual
  listening port — replacing `tcp:443` for a gateway-authenticated
  provider launch, never adding to it. **The scope's true destination set
  is broader than "loopback" and must be named accurately, not assumed:**
  round-6's credential-free "L1 loopback admission" measurement
  (`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:284-333`) proved
  that Seatbelt's `(remote tcp "localhost:P")` admits `127.0.0.1:P`,
  `[::1]:P`, a second distinct `127.0.0.0/8` alias configured on the host,
  and the host's own non-loopback LAN interface address — while correctly
  denying a different port and a genuinely remote host. Consequently:
  - **The gateway must hold the port on every local address the rule
    admits, not just `127.0.0.1`.** At gateway start it binds
    `127.0.0.1:<port>` and `[::1]:<port>` at minimum (both addresses, same
    port, same handler); if either bind fails, gateway startup fails
    closed (same shape as D14's other startup-failure paths). Every
    accepted connection is checked against a loopback-peer allowlist
    (`127.0.0.0/8` or `::1`) and any other peer is dropped immediately,
    unauthenticated by anything but that source-address check (probe
    reference: dual-stack bind and peer filter,
    `docs/development/task0152-probes/owner_proxy_probe.py:1257-1270`,
    `_is_loopback_peer` at `:558-565`).
  - **Residual, stated explicitly, not fixed by this rule:** (a) a local,
    non-loopback interface address or an additional `127/8` alias that
    Seatbelt's rule also admits, but which the gateway does not itself
    bind at attempt start, remains something the *rule* would admit a
    connection attempt toward — there is simply nothing listening there
    for *this* gateway to accept, so no functional gap exists for this
    design today, but a future consumer reusing this same network-rule
    pattern must account for that admission itself, not assume "loopback
    only"; (b) an address added to the host mid-attempt (DHCP renewal, a
    VPN interface coming up) after the gateway has already bound its
    fixed set is not retroactively covered — the gateway does not
    re-enumerate interfaces during an attempt; and (c) binding a
    non-loopback address specifically (if a future implementation ever
    chose to, which this rule does not require) would itself expose a
    listener on that address, relying entirely on the peer-drop check
    above rather than on the OS not routing a connection there at all —
    the peer filter is a real, load-bearing check under this design, not
    defense in depth.
  - Any Runner backend that cannot express this true destination set
    (bind-everywhere-admitted-plus-peer-filter) fails closed
    (`RunnerError::BackendUnavailable`), exactly as `profile_for` already
    fails closed on every unsupported destination set today
    (`adapters/runner-macos-seatbelt/src/lib.rs:229-245`). This is new
    capability surface, not a reinterpretation of the existing `tcp:443`
    rule; a non-gateway provider launch keeps its current `tcp:443` scope
    unchanged.
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
- **D23 (§9a authentication precondition — B1).** For a gateway-backed
  launch, the pre-launch authentication precondition §9a currently binds
  to the host's own `claude auth status` (`AUTHENTICATED`/
  `UNAUTHENTICATED`/`UNKNOWN`) is **replaced** by: a policy-bound
  credential `SecretRef` exists for the attempt, its shape validates
  (D20), and the gateway started (D1, D11). The host provider-native login
  state is neither required nor consulted for a gateway-backed launch —
  `bins/agentd/src/execution.rs:816-826`'s call to
  `provider_claude::probe_authentication` is skipped entirely for this
  launch shape, not merely overridden after running. A non-gateway launch
  keeps §9a exactly as accepted today: unchanged precondition, unchanged
  refusal text, unchanged enforcement point. This does not reopen §7's
  "the adapter still never reads, persists, logs, forwards, or repairs a
  credential": the `provider-claude` adapter is unchanged by this rule and
  still never touches the credential in either launch shape; the new
  trusted boundary that attaches it is the `agentd`-owned gateway (D12),
  entirely outside the adapter. See "Proposed amendments" for the exact
  §9a superseding text and "Implementation task list" for the matching
  code change.
- **D24 (body-scoped enforcement — B2, owner-accepted residual risk).** The
  gateway parses the forwarded `POST /v1/messages` JSON body only to
  enforce two owner-configured limits: (a) the `model` field must match an
  owner-configured model allowlist — an unknown or missing `model` is a
  local rejection, never forwarded; (b) `max_tokens` must not exceed an
  owner-configured per-request cap, and the gateway tracks a per-attempt
  request-count and cumulative-token budget, rejecting locally once either
  is exceeded. If no allowlist/cap configuration is present for an
  attempt, the gateway fails closed (refuses to forward at all) rather
  than defaulting to unrestricted. This is a narrow, owner-scoped guard,
  not a general body-content policy: it does not inspect, filter, or
  reject `tools`, `mcp_servers`, or any other body field — see "Security
  impact" for the residual risk this leaves open, which the owner has
  reviewed and accepted, and "Implementation task list" for an optional,
  not-yet-decided future hardening follow-up that would go further.

### Additional hardening rules (D15-D22, D25-D26)

Triggered by an owner operator error during the withdrawn round-2/3
*manual* Keychain-setup flow (not the round-4 probe itself): on 2026-09-25
the owner typed a real OAuth token into a Keychain-setup command's
service-name argument instead of letting `-w` prompt for it interactively,
exposing it in shell argv, history, and chat (the owner stated intent to
revoke it on 2026-09-26; no `setup-token` revocation mechanism is
established in primary docs, so this ADR treats the token as exposed
until revocation is confirmed, not as already revoked — see "Owner
real-credential receipts"). The probe's own round-4/round-6 hardening
(C1-C10, T1-T2, `docs/development/task0152-probes/`) fixes the class of
mistake, not only that symptom; D15-D26 carry the same posture into the
production Rust gateway as normative rules, not optional polish.

- **D15.** Only an explicit header-**name** allowlist is forwarded
  upstream: `content-type`, `accept`, `anthropic-version`, `anthropic-beta`,
  `user-agent`, the R8-observed non-identifying Stainless SDK metadata
  names, and `x-app` (`FORWARD_HEADER_ALLOWLIST`, `owner_proxy_probe.py:318-332`).
  Everything else the child sends — including `x-claude-code-session-id`
  and `anthropic-dangerous-direct-browser-access` — is dropped; only the
  dropped header **name** may be audited, never a value.
- **D16.** Request-smuggling/parser hardening, checked before any body is
  read: reject `Transfer-Encoding` from the child, more than one
  `Content-Length` header or a non-numeric one, and a declared body over a
  fixed size limit (`owner_proxy_probe.py:272`, `MAX_BODY_BYTES`); enforce
  a bounded header count/size and a per-connection read timeout
  (`CLIENT_SOCKET_TIMEOUT`, `:274`, used at `:602`); run with no
  address reuse, bound only to the addresses D11 requires. **Bounded
  concurrency and a whole-request deadline (round-6, T2b — closes an
  other-uid-irrelevant but same-attempt DoS gap a slow client can otherwise
  open):** a client trickling a request body a byte at a time can hold a
  single per-operation-timeout-only server the *entire* time, since each
  individual read completes within its own timeout even though the whole
  request never does. The gateway bounds concurrent in-flight requests to
  a small fixed number (the reference implementation is single-threaded,
  i.e. exactly one) and enforces a separate, shorter wall-clock deadline
  from connection accept through headers-and-body-fully-read (never
  through response writing) — reference: a 10s `signal.alarm`-based
  deadline layered on top of the unchanged per-operation socket timeout
  (`REQUEST_DEADLINE_SECONDS = 10`, `owner_proxy_probe.py:283`; enforcement
  at `:607-630`).
- **D17.** Upstream TLS uses an explicit, code-configured trust
  configuration — **an explicitly loaded/pinned root store**, never
  whatever the ambient process environment happens to select — ignoring
  `SSL_CERT_FILE`, `SSL_CERT_DIR`, any `*_CA_BUNDLE` name, any `OPENSSL_*`
  name (including `OPENSSL_CONF` specifically), `SSLKEYLOGFILE`, and
  ambient proxy variables. **`OPENSSL_CONF` is checked and refused before
  any TLS-capable module is even imported** (round-6, T2a): a review
  measured that OpenSSL can read `OPENSSL_CONF` at library-initialization
  time, which can happen as early as import, so scrubbing the process
  environment *after* that point is already too late — the reference
  implementation's check runs as the very first code in the process
  (`_c3_refuse_if_dangerous_tls_env`,
  `docs/development/task0152-probes/owner_proxy_probe.py:207-224`), and
  refuses outright rather than stripping-and-continuing. TLS is pinned to
  a minimum of 1.2, with full certificate and hostname verification, and a
  pinned upstream host (reference: `_build_opener`, `:363-372`). **This
  reconciles with D6:** D6's "default TLS certificate verification" means
  the verification *algorithm* is the platform TLS library's own default
  (full chain + hostname checks, nothing loosened) — it does not mean
  "whatever root store or config the ambient environment happens to
  supply," which D17 explicitly pins instead; no redirect is ever
  followed, per D6.
- **D18.** On the first upstream authentication failure (`401`/`403` from
  the real Anthropic API), the gateway stops forwarding for that attempt
  and fails with a typed provider-authentication condition — never a retry
  of the same request, never a retry with a different credential or class,
  never a fallback around D14's refusal. This is deliberately stricter
  than the child's own observed behavior: an end-to-end test against a
  real-API-key-shaped but invalid dummy credential (not owner-run), run by
  the implementer agent twice (2026-09-25 and 2026-09-26) plus once more
  by the orchestrator with a fresh dummy value and a fresh temp keychain,
  all at script sha256
  `ae3b4208e586b1a529b4df7055d27943b3522cda6cdb91e5a8efe0b7519ec37b`,
  recorded the CLI child issuing exactly 9 forwarded requests and receiving
  9 real `401` responses before the harness's own 120s timeout cut it off
  without the child ever exiting
  (`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:785-909`,
  "Agent-run end-to-end invalid-credential test (round 4)" — the two
  implementer-agent runs are recorded there in full; the orchestrator's
  additional confirmation reproduced the same shape). **Why the CLI
  retried 9 times is not established** — that section explicitly states no
  header value (including `x-stainless-retry-count`) was ever captured,
  only header names, so the retry's cause is an open question, not a
  claimed mechanism; what is observed is only the count and outcome. The
  gateway must not wait out whatever the child's own retry budget turns
  out to be. Total forwards per attempt remain bounded regardless (D9;
  `MAX_FORWARDED_REQUESTS = 32`, `owner_proxy_probe.py:273`).
- **D19.** No credential value ever appears in an error, log line, panic/
  crash message, `Debug`/`Display` output, crash report, or Security Audit
  record — only classified failure reasons and counts, matching the
  probe's own posture of printing an exception's class name only, never
  its message (C6 hardening). Credential buffers are cleared/zeroized
  after use to the extent the implementation language allows. **Because
  the gateway is an in-process `agentd` component, not a separate process
  (see "Process boundary" above, N7), core-dump disabling is a property of
  the `agentd` process itself** — reference:
  `_c9_disable_core_dumps`/`RLIMIT_CORE`,
  `owner_proxy_probe.py:474-480`, which this rule generalizes from a
  standalone probe process to `agentd` as a whole. Stated as a trade-off,
  not glossed over: this disables core dumps for all of `agentd`'s other
  responsibilities too, for as long as any credential may be resident in
  its address space, not only for a narrowly-scoped helper — the separate-
  helper-process alternative above would have scoped this more tightly, at
  the cost of the IPC/ACL trade-offs already described there.
- **D20.** Credential shape is strictly validated per class — mode-specific
  prefix and charset — at both provisioning and release; a mismatch fails
  closed without logging the rejected value
  (`_validate_credential_shape`, `owner_proxy_probe.py:483-491`; same
  pattern in `provision_keychain_item.py`'s `TOKEN_PREFIX` check).
- **D21.** The per-attempt placeholder is at least 256 bits from the OS
  CSPRNG (`secrets.token_urlsafe(32)` = 32 bytes, `owner_proxy_probe.py:1253`),
  compared only with a constant-time comparison (D3;
  `hmac.compare_digest`, `:652-654`), and is never written to a log, Event
  Journal payload, or Security Audit record in any form.
- **D22 (provisioning UX, implementation follow-up).** The owner-facing
  provisioning tool never accepts a credential as a command argument,
  environment variable, or clipboard-persisted value — only a masked,
  hidden prompt entered twice and compared, matching
  `provision_keychain_item.py`'s `getpass.getpass` pattern (`:149-151`)
  and its refusal to run on any credential-shaped argv element. Service
  names are fixed by credential class, never typed by the owner
  (`SERVICE_NAME`, `:38-41`). **Round-6 additions (N8):** the tool refuses
  to provision when an item for the service/account already exists — it
  never re-adds over one (no `-U`), because re-adding does not reset an
  existing item's ACL, so an item created some other, looser way would
  silently keep that looser ACL forever; it performs a metadata-only
  existence check first (`find-generic-password` without `-w`, never
  touching the confidential data) and instructs the owner to delete the
  old item explicitly before re-provisioning
  (`_item_exists`, `provision_keychain_item.py:192-208`; refusal logic,
  `:358-372`). The account name comes from the OS user database
  (`pwd.getpwuid(os.getuid()).pw_name`, `:353-356`), never an ambient
  `$USER`/`$LOGNAME` an unusual environment could set to something else.
  Any value destined for the `security -i` command line is rejected if it
  contains a control character, quote, or backslash — a raw newline could
  otherwise terminate one intended command and start a second,
  attacker-chosen one, since `security -i` reads one command per line
  (`_reject_unsafe_for_security_line`, `:118-132`). **When macOS prompts
  for approval to use the item, the owner clicks "Allow," never "Always
  Allow"** — "Always Allow" permanently adds the requesting application to
  the item's trusted-application list, silently widening exactly what
  `-T ""` exists to prevent, one click at a time, every time the gateway
  runs. Verified ACL shape (round-6, T1c), a sanitized
  `security dump-keychain -a` excerpt against a `-T ""` item created by
  this tool: the confidentiality-relevant ACL entry (the one whose
  `authorizations` list includes `decrypt`/`export_clear`) shows
  `applications (0):` — zero trusted applications, so every read prompts
  the owner, exactly as designed
  (`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:663-712`,
  "ACL excerpt (T1c)").

  **ACL trade-off, stated explicitly rather than as a simple "stricter."**
  `provision_keychain_item.py` stores the owner's real item with `-T ""`
  (`:212-221`) — no application is trusted, so *every* read, including
  `agentd`'s own, prompts the owner for approval. Production instead names
  exactly one trusted application: the code-signed `agentd` binary. That
  is **looser** than `-T ""` by precisely that one binary — `agentd` reads
  silently where the probe's item would still prompt — and **tighter**
  than the ACL Keychain assigns by default when no `-T` is given at all
  (which trusts whichever tool created the item, not necessarily the one
  that should read it later). D22 is therefore a deliberate three-way
  choice (default-trusts-creator ⊃ agentd-only ⊃ `-T ""`), not a strictly
  tighter version of the probe's own posture.

  **Code-signing identity caveat.** macOS binds a named-application trust
  entry to the binary's code-signing identity/designated requirement, not
  to a file path. An ad-hoc-signed or unsigned development build changes
  identity on every rebuild, so a production implementation must either
  fail closed with a typed error when the running binary's identity does
  not match the ACL entry, or re-prompt the owner — it must never silently
  widen the ACL (e.g., to a wildcard, or by re-trusting whatever binary
  happens to be running) to work around a development-build identity
  mismatch. See "Open questions" for the case this ADR does not itself
  settle.
- **D25 (fresh config anchor — N2).** A gateway-backed provider launch
  uses a fresh, empty `HOME`/`CLAUDE_CONFIG_DIR` under the attempt's own
  isolated runtime root and grants **no** host `~/.claude` or
  `~/.claude.json` support-read — the same pattern R5 and the owner's own
  probe already use (`launch_confined_child`,
  `docs/development/task0152-probes/owner_proxy_probe.py:990-1060`;
  current production support-read roots that this rule *excludes* for a
  gateway-backed launch:
  `adapters/provider-claude/src/environment.rs:219-230`). A gateway-backed
  launch's authentication does not depend on, and must not read, any
  pre-existing host provider-session state.
- **D26 (plan-scoped typed environment admission — N1).** `CHILD_ENV_ALLOWLIST`
  (`crates/runner/src/lib.rs:34-64`) is a static, process-wide global name
  list; it has no mechanism of its own for "these names are admitted only
  for a gateway-backed launch." The Runner gains a typed, plan-scoped
  admission: the D11 loopback-scope capability, when present on an
  `IsolationPlan`, is what actually authorizes the gateway's placeholder/
  `ANTHROPIC_BASE_URL` names for that specific plan — a plan without the
  D11 capability cannot admit them even if the static allowlist is later
  extended to list their names for other reasons. Naming stays
  provider-neutral (no `claude`/`anthropic`-specific identifier in the
  Runner's own admission type). Independent of that binding, the Runner
  refuses — as a defense-in-depth backstop, not the primary control — any
  admitted value shaped like a real credential (reusing D20's shape-
  validation posture): a plan-admitted "placeholder" that happens to match
  a real credential's prefix/charset is refused rather than passed
  through, since D2 already guarantees a genuine placeholder never has
  that shape.

## Provider neutrality

Per AGENT.md §13, the gateway's core (D1, D3, D4, D6-D11, D14, D16, D17,
D19, D21, D26) is provider-neutral request-forwarding/placeholder-
validation/hardening machinery that
could host any HTTP-based provider credential, not Claude-specific logic
baked into `agentd`. The Claude-specific pieces — which header carries the
credential per mode (D5), which variable names to deliver (D2), and the
pinned upstream host — live behind the `provider-claude` adapter, which
supplies them to the shared gateway core rather than the core branching on
`provider == "claude"`. A second HTTP-based provider would supply its own
header/variable/host mapping to the same gateway core; this ADR does not
generalize that contract beyond Claude, since no second provider's shape
is evidenced here (INV-069, INV-082).

## Compliance

Anthropic's Agent SDK overview states that it "does not allow third party
developers to offer claude.ai login or rate limits" for their own products
absent prior approval, and directs integrators toward API-key
authentication instead (<https://docs.claude.com/en/api/agent-sdk/overview>,
retrieved 2026-09-25).

Anthropic's Claude Code authentication documentation (retrieved
2026-09-26, <https://code.claude.com/docs/en/authentication>,
"Authentication precedence") documents **seven** credential sources in
precedence order, corrected here from an earlier draft's inaccurate
three-item summary: (1) cloud provider credentials
(`CLAUDE_CODE_USE_BEDROCK`/`_VERTEX`/`_FOUNDRY`); (2) `ANTHROPIC_AUTH_TOKEN`,
sent as an `Authorization: Bearer` header; (3) `ANTHROPIC_API_KEY`, sent as
an `X-Api-Key` header — matching R8's directly observed header shapes
exactly; (4) `apiKeyHelper` script output; (5) `CLAUDE_CODE_OAUTH_TOKEN`
(the `setup-token` variable); (6) a named/federation profile (a
`user_oauth`/WIF-derived profile); (7) a saved claude.ai or Claude Console
`/login` credential, the default fallback. **`apiKeyHelper` and
`CLAUDE_CODE_OAUTH_TOKEN` rank below `ANTHROPIC_API_KEY`, not above or
absent, contrary to an earlier draft's summary.** A signed-in Claude apps
gateway session sits outside this list entirely and outranks every item
in it. The same page documents `claude setup-token` as generating a
one-year OAuth token that the CLI does not save anywhere itself, tied to a
Pro/Max/Team/Enterprise subscription, and scoped to making model requests
only. It does not document a revocation procedure for that token (carried
into "Open questions" below, unresolved by this ADR).

Anthropic's LLM-gateway documentation (retrieved 2026-09-25,
<https://docs.claude.com/en/docs/claude-code/llm-gateway>) describes a
gateway-supplied credential or an `apiKeyHelper` as replacing the
subscription login for that session, notes that setting only a base URL
does not by itself replace the subscription, and states that a gateway
relaying this traffic must forward the OAuth capability the CLI advertises
in its `anthropic-beta` header — consistent with D5/R8 above: the gateway
never invents this flag set, it only relays what the CLI already sent.

Anthropic's Commercial Terms of Service (`anthropic.com/legal/commercial-terms`)
govern any Anthropic API key and the Console. Anthropic's Consumer Terms
of Service (`anthropic.com/legal/consumer-terms`, retrieved 2026-09-26)
govern an individual consumer's claude.ai account and state that
Commercial Terms instead govern API-key/Console use. Since `setup-token`
requires a Pro, Max, Team, or Enterprise plan (see above), this ADR does
**not** generalize which terms apply to every `setup-token` case: a
Pro/Max subscription is an individual consumer account (Consumer Terms),
while Team and Enterprise are organizational/commercial plans (Commercial
Terms) — the owner's own plan type determines which applies, and this ADR
draws no independent legal conclusion beyond stating that distinction; it
is not a substitute for the owner's own review of Anthropic's primary
terms.

**Compliance risk, owner-accepted (2026-09-25).** On 2026-09-25 the owner
chose, in a structured decision prompt, the option "API key + setup-token."
That option's stated consequence — presented to the owner before the
choice was made — was amending `providers.md` §7, the H7 closure record,
and `PRIVACY_ARCHITECTURE.md` §5 as this ADR proposes, and recording the
Anthropic third-party-login compliance risk discussed above as a risk the
owner accepts. This is the owner's own recorded decision, not an
independent legal conclusion by this ADR, and it is not a substitute for
the owner's own review of Anthropic's primary terms.

**Proposed for ratification, not yet owner-ratified:** the `setup-token`
credential class is constrained to the owner's own account, used only on
the owner's own machine, exclusively through this ADR's gateway (D1-D26) —
never a raw HTTP adapter that reuses the token outside the gateway's
single-host, single-attempt design — and must not be offered to any other
user as a product sign-in or authentication feature without separate,
explicit Anthropic approval. The API-key credential class remains the
default and is the *only* class available in any distributed or
multi-user Runlith configuration; the `setup-token` class is never enabled
by default and never exposed as a general product feature.

**Anthropic-server-mediated egress risk, owner-accepted (2026-09-26).**
The gateway forwards the `/v1/messages` body and `anthropic-beta`
unchanged (D5). Two Anthropic-side features let the confined child cause
Anthropic's own servers — not the confined child directly — to contact
arbitrary third-party hosts: the MCP connector accepts an `mcp_servers`
array naming any publicly-exposed HTTP(S) server URL, which Anthropic's
servers then connect to on the caller's behalf
(<https://platform.claude.com/docs/en/agents-and-tools/mcp-connector>,
retrieved 2026-09-26); and the web fetch server tool has Anthropic's API
retrieve the content of a URL the model selects, inserted server-side into
the conversation (<https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-fetch-tool>,
retrieved 2026-09-26; that page's own warning names this an exfiltration
risk and documents `allowed_domains`/`max_uses`/disabling the tool
entirely as mitigations available at the API-request level, not something
this gateway currently sets). On 2026-09-26, in a structured decision
prompt, the owner chose "accept as risk": D24's body parsing enforces
only a model allowlist and token/request budget (see above); the body is
**not** filtered to strip or block `tools`, `mcp_servers`, or any other
field. This channel — Anthropic-server-mediated egress via server-side
tools and the MCP connector — is recorded here explicitly as an
owner-accepted residual risk, not silently narrowed by D24's wording (see
"Security impact" for the full statement, and "Implementation task list"
for an optional, not-yet-decided future hardening follow-up).

Measured by the **owner** (not this ADR's drafter, per its no-credential-
read constraint) on 2026-09-25, CLI `2.1.281`, running
`owner_proxy_probe.py` at commit `9787690` — the pre-round-4 script; C1-C10
had not yet landed at the time these two runs were made
(`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md:735-783`, "Owner
real-credential receipts"). Both modes (api-key, OAuth)
produced this identical result shape:

| field | value |
| --- | --- |
| `self_check_pass` | `True` |
| `self_check_status` | `401` |
| `child_exit` | `0` |
| `marker_present` | `True` |
| `failure_class` | `success` |
| `local_401` | `1` (pre-flight self-check) |
| `forwarded` | `1` |
| `upstream_status_codes` | `[200]` |

**What this proves.** The negative self-check correctly rejected a wrong
placeholder locally, with zero forwarded requests, before either mode ever
read the real credential; the one legitimate forwarded request reached the
real Anthropic API and returned a real `200`; the confined child completed
its non-interactive round trip while holding only the placeholder. This
is direct evidence for Option B's core claim (a real, valid credential
completes a session through the gateway without the confined child ever
holding it), for both credential classes.

**What this does not prove.** One short, non-interactive, single-turn run
per mode — no long-lived session near real token expiry, and no OAuth
refresh behavior, was exercised (same open question as "Security impact"
below). The result was measured against the pre-round-4 script, so it does
not by itself prove the round-4 hardened gateway (D15-D26, C1-C10, T1-T2) behaves
identically against a real credential; round 4's own verification
exercises the hardening's local-only rejection paths via
`--dry-self-check`, which never touches Keychain or upstream.

**Operator-error exposure and revocation (unchanged by this receipt's
validity).** The OAuth token used for the OAuth-mode run above was exposed
during round-2/3 manual Keychain setup: the owner typed a real token into
a command's service-name argument instead of letting `-w` prompt for it,
landing it in shell argv/history/chat. No token value, account identifier,
or org identifier is recorded here or anywhere in this task's artifacts.
The owner stated intent to revoke that token on 2026-09-26; no
`setup-token` revocation mechanism is established in any primary Anthropic
documentation read for this ADR (see "Compliance"), so this ADR treats the
token as **exposed until revocation is confirmed**, not as already
revoked on the stated date. This receipt stands as evidence of a real,
working round trip because it was measured **before** any revocation
attempt; it does not imply the exposed token remains valid or usable
afterward, confirmed or not. This exposure is the direct trigger for the
round-4/round-6 hardening cited throughout D15-D26 above, and for
withdrawing the manual `security add-generic-password ... -w` instructions
in favor of `provision_keychain_item.py`'s hidden-prompt provisioning (D22).

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
> loopback-authenticating credential gateway (D11's true destination set
> is broader than "loopback only" — see ADR-0037 D11 — but the gateway
> accepts only loopback-sourced connections regardless of which local
> address it is reached on, per D11's peer filter). The gateway — not the
> provider CLI, and not the host's own provider-native login session — is
> the sole recipient of a Secrets-Broker-released Claude credential (an
> Anthropic Console API key or the owner's own `claude setup-token` OAuth
> token); the confined child receives only a non-secret, attempt-scoped
> placeholder.

**`docs/architecture/providers.md` §9a ("Authentication state")** — append
after "A launch may satisfy §9a and still fail when the profile denies the
mechanism the provider authenticates through" (text unchanged above this
point):

> Per ADR-0037, this precondition applies to a launch that is **not**
> gateway-backed. For a gateway-backed launch (ADR-0037), the relevant
> precondition is different in kind, not merely in outcome: a policy-bound
> credential `SecretRef` exists for the attempt, its shape validates, and
> the gateway started. The host's own provider-native login state
> (`claude auth status`) is neither required nor consulted for that launch
> shape. This does not relax §7's "the adapter still never reads, persists,
> logs, forwards, or repairs a credential": the `provider-claude` adapter
> is unchanged and still never touches the credential in either launch
> shape; the trusted boundary that attaches it is the ADR-0037 gateway,
> entirely outside the adapter.

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
  than `tcp:443` in the sense that it names one fixed port rather than a
  broad remote host class — but, per D11's corrected true destination set,
  that one port is reachable on *every* local address the host has
  configured (loopback and non-loopback alike), not "one loopback port";
  it does not widen what any *other* launch purpose may reach.
- `ProviderEnvironment` and `CHILD_ENV_ALLOWLIST` each gain a small,
  reviewed set of new allowlisted names, all non-secret-classified
  (placeholder + `ANTHROPIC_BASE_URL`), admitted only for a plan carrying
  D11's capability (D26).
- `agentd` gains a new in-process component (the gateway, N7) with its own
  bounded lifetime, request allowlist, and audit surface — a new piece of
  reviewable code, not a new authority model (it composes existing Secrets
  Broker/Policy primitives, per D12).
- `providers.md` §7, §9a, the H7 closure record, and
  `PRIVACY_ARCHITECTURE.md` §5 each require the amendment text above once
  this ADR is ratified (D23 for §9a).
- Option A and Option C remain explicitly rejected; adopting either later
  would require a superseding ADR, not a silent implementation choice.

## Security impact

Residual risks, not resolved by this ADR and not glossed over:

- **Confused-deputy spend through the gateway during an attempt.** Any
  process able to reach the gateway's bound addresses (D11: broader than
  `127.0.0.1` alone) with the correct placeholder during the attempt's
  lifetime can spend against the real credential through the gateway's
  allowlisted `/v1/messages` path, bounded by D24's model/token limits. The
  intended caller is the one confined child the attempt launched; nothing
  in D1-D26 cryptographically binds the placeholder to that specific
  process beyond "whoever currently holds the placeholder bytes."
- **Same-user local processes that learn the placeholder during an
  attempt.** The probe's own limitation statement is carried forward
  unresolved: no concurrency/TOCTOU analysis exists for a hostile,
  same-user, same-host process racing the legitimate child to use a
  captured placeholder before attempt end. D10's short attempt-bound
  lifetime narrows the exposure window; it does not eliminate it. **A
  different-uid local attacker is out of this threat model entirely** —
  same as every other Runner-enforced boundary — and D4's unauthenticated
  `/api/hello` endpoint means any same-host process, of any uid that can
  reach the bound port, can discover that a Runlith gateway is listening
  (nothing more: no attempt detail, credential, or placeholder is
  revealed by that endpoint).
- **Data sent to Anthropic via prompts is inherent to using the provider
  at all**, unaffected by which of these options is chosen — out of this
  ADR's scope.
- **Anthropic-server-mediated egress via server-side tools and the MCP
  connector — owner-accepted residual risk (B2).** D24 enforces only a
  model allowlist and a token/request budget on the forwarded body; it
  does **not** filter, strip, or block `tools`, `mcp_servers`, or any other
  body field. If the CLI's request includes Anthropic's web-fetch/
  web-search/code-execution server tools or an `mcp_servers` entry naming
  an arbitrary HTTP(S) server, **Anthropic's own servers** — not the
  confined child — will contact that third-party host or service on the
  API caller's behalf
  (<https://platform.claude.com/docs/en/agents-and-tools/mcp-connector>,
  <https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-fetch-tool>,
  both retrieved 2026-09-26). This is a real egress channel this design
  does not close: it exists because the gateway forwards the body
  unchanged (D5), not because of any gap in the loopback network
  confinement (D11) — the confined child's own network access is still
  exactly the gateway's port, nothing more. The owner reviewed this and
  chose, on 2026-09-26 in a structured decision prompt, to accept it as
  risk (see "Compliance"); it is not silently narrowed by D24's more
  limited scope. A stricter, optional body policy (rejecting `mcp_servers`/
  server tools/`container` entirely, or an `anthropic-beta` allowlist) is
  listed in "Implementation task list" as a not-yet-decided future
  hardening follow-up.
- **`/api/hello`, `/v1/messages` beyond D24's own scope, and any CLI
  feature needing a different outbound destination than the pinned
  upstream** — D4's allowlist is deliberately narrow; anything else is
  refused locally (`403`), a capability regression relative to an
  unconfined host session (independent of the residual risk above, which
  is about what the *allowed* path can still reach server-side).
- **OAuth refresh behavior over a long-lived session is untested.** R8's
  "no other outbound destination observed" finding is scoped to one short,
  non-interactive, marker-prompt invocation; a longer-lived attempt
  approaching real token expiry was never exercised and could need an
  outbound call this design's allowlist does not yet account for (same
  caveat the evidence doc records verbatim).

**What this ADR explicitly cannot prevent:**

- **Root or kernel compromise of the host** — every guarantee above is an
  OS-process-boundary guarantee; regardless of D1-D22.
- **Malware running as the same user as `agentd`** — can read the confined
  child's environment (the attempt-scoped placeholder, per the
  confused-deputy risk above) or attempt to trigger/automate D22's OS
  approval prompt; D22 raises the bar (only code-signed `agentd` is
  silently trusted) but does not make a same-user compromise safe.
- **Compromise of the upstream provider itself** is outside this ADR's
  threat model; the gateway pins and authenticates the connection, it does
  not attest upstream behavior.
- **Operator error outside the tool** — D15-D26 narrow, but do not
  eliminate, the class of mistake behind this ADR's own receipts exposure
  (a secret typed somewhere other than the intended hidden prompt); no
  tool-level control substitutes for owner care.

**Option A's probe was withdrawn, never run.** `owner_positive_probe.sh`
was deleted in round 4 without being run — the `*:443` blast-radius
argument above already rejected Option A on its own
(`docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md`, "Owner-run
positive probe (Option A) — WITHDRAWN, never run").

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

- **Runner loopback network scope (D11 — B3).** Add the D11 capability to
  `crates/runner` and compile it in `adapters/runner-macos-seatbelt`,
  binding every local address the Seatbelt rule admits (D11) and filtering
  to loopback-sourced peers only. Adversarial tests must specifically
  include: a connection attempt from `::1`, from a second/alias
  `127.0.0.0/8` address, and from a non-loopback interface address —
  proving each is either reachable-but-peer-rejected or unreachable as
  D11 requires, not merely proving `127.0.0.1` alone works; plus a test
  that every backend without this capability fails closed. Depends on:
  this ADR's ratification.
- **§9a authentication-precondition code change (D23 — B1).** Update
  `bins/agentd/src/execution.rs:816-826`'s pre-launch flow so a
  gateway-backed launch skips `provider_claude::probe_authentication`
  entirely and instead checks D23's credential/shape/gateway-started
  precondition; a non-gateway launch is untouched. Depends on: gateway
  component, doc amendments (§9a text is fixed by this ADR).
- **Credential gateway component.** Implement D1-D10 and the D15-D21,
  D24-D26 hardening/scope rules as a new component *inside* `agentd`
  (N7 — not a separate process) with its own targeted test suite:
  self-check, method/path/query allowlist (D4, with audit digest not raw
  values, N3), header-name allowlist (D15), request-smuggling/size/
  timeout/single-listener/bounded-concurrency/whole-request-deadline rules
  (D16, N4), pinned-TLS/no-redirect/no-ambient-proxy/no-`OPENSSL_CONF`
  bypass (D6, D7, D17), stop-on-first-upstream-401/403 with a typed
  condition and bounded total forwards (D18), no-credential-in-error/log/
  panic/audit plus zeroization and `agentd`-scoped core-dump disabling
  (D19), credential shape validation at release (D20, shared with the
  provisioning path below), CSPRNG placeholder length/constant-time
  comparison (D21), the fresh config anchor with no host `~/.claude` read
  (D25), the D26 plan-scoped typed environment admission, and D24's
  model-allowlist/token-budget body enforcement — each rule gets its own
  adversarial test (e.g., a crafted `Transfer-Encoding`/duplicate-
  `Content-Length` request for D16, a forced upstream `401` for D18, a
  deliberately malformed credential for D20, an over-cap `max_tokens` or
  unlisted `model` for D24). Depends on: this ADR's ratification.
- **Secrets Broker binding/provisioning path for both credential classes
  (D22 — N8).** An owner-run CLI/setup path that creates the `SecretRef`
  for an API key or a `setup-token` through a masked, hidden interactive
  prompt only — never argv, environment, shell history, or clipboard
  persistence — refusing any argument that looks credential-shaped,
  mirroring `provision_keychain_item.py`'s pattern including its
  existence-check refusal (no re-add over an existing item), account
  derived from the OS user database, and control-character rejection
  before assembling any `security -i` command line; the Keychain item's
  ACL trusts only the code-signed `agentd` binary and fails closed
  (typed error) or re-prompts, never silently widens, when the running
  binary's code-signing identity does not match (development-build case
  left open — see "Open questions"); service names are fixed by
  credential class, never typed. Depends on: this ADR's ratification.
- **`provider-claude` environment + `CHILD_ENV_ALLOWLIST` changes (D25,
  D26 — N1, N2).** Add the placeholder variable(s) and
  `ANTHROPIC_BASE_URL` to `ProviderEnvironment` (with D25's fresh config
  anchor) and the D26 plan-scoped typed admission to the Runner, bound to
  the D11 capability rather than a bare static-global name extension;
  refuse any admitted value shaped like a real credential. Depends on:
  gateway component.
- **Security Audit facts (N3).** Extend `crates/security-audit`'s typed
  vocabulary to cover gateway bind/lifetime and locally-rejected requests
  as a fixed category plus a truncated digest — never a raw child-supplied
  path/query/header value. Depends on: gateway component.
- **Doc amendments.** Apply the exact superseding text in "Proposed
  amendments" to `providers.md` §7, §9a, the H7 closure record, and
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
- **Optional, not-yet-decided future hardening follow-up (B2).** A
  stricter body policy that rejects `mcp_servers`, Anthropic server-side
  tools (web fetch/web search/code execution), and `container` entirely,
  or pins an explicit `anthropic-beta` allowlist, closing the
  Anthropic-server-mediated egress channel "Security impact" records as
  owner-accepted residual risk. Not authorized or scoped by this ADR;
  listed so the option is not lost, not committed to.

## Open questions

- **`setup-token` revocation path.** Not established by any primary
  source read for this ADR (see "Compliance"). Blocks a confident
  incident-response story for a leaked OAuth token until resolved.
- **OAuth refresh over long-lived sessions.** Untested by any probe in
  `docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md` (see "Security
  impact"); a longer attempt may need an outbound call this ADR's
  allowlist (D4) does not yet account for.
- **Linux/Windows Runner backends.** Out of scope. D11's network scope is
  specified provider-neutrally in `crates/runner`, but only the macOS
  Seatbelt compilation (with its broader-than-loopback true destination
  set) is evidenced here; any other backend must implement its own
  equivalent — including its own true destination-set discovery, which may
  differ from macOS's — or fail closed on the new scope, per D11, before a
  gateway-backed launch is reachable on that platform.
- **Keychain ACL identity for development builds (D22).** An unsigned or
  ad-hoc-signed development build of `agentd` does not have the stable
  code-signing identity a named-application ACL entry binds to, so D22's
  fail-closed-or-re-prompt rule has no ratified answer here for local
  development workflows specifically — only the production constraint
  (never silently widen the ACL) is settled.

## References

- `docs/development/G7_TASK_0112_PROVIDER_AUTH_DIAGNOSIS.md` — root cause,
  capability ablation, rejected remediation (Option C).
- `docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md` — R0-R8, Option B
  probe design, round-4 hardening (C1-C10), "Owner commands", "Owner
  real-credential receipts" (lines 520-565), "Agent-run end-to-end
  invalid-credential test (round 4)" (lines 567-687), "Owner-run positive
  probe (Option A) — WITHDRAWN, never run" (lines 1025-1042), "What this does
  NOT prove".
- `docs/development/task0152-probes/owner_proxy_probe.py` — probed
  mechanics D1-D21, D25 formalize; `:207-224` (`_c3_refuse_if_dangerous_tls_env`,
  `_DANGEROUS_ENV_EXACT`), `:269` (`ALLOWED_QUERY_STRINGS`), `:272-274`
  (body/count/timeout limits), `:283` (`REQUEST_DEADLINE_SECONDS`),
  `:318-332` (`FORWARD_HEADER_ALLOWLIST`), `:363-372` (`_build_opener`, TLS
  pinning), `:990-1060` (`launch_confined_child`, fresh HOME/
  `CLAUDE_CONFIG_DIR` anchor), `:474-480`
  (`_c9_disable_core_dumps`), `:483-491` (`_validate_credential_shape`),
  `:505-514` (`_hash_path_for_logging`), `:558-565` (`_is_loopback_peer`),
  `:569-573` (`ProxyServer`, single listener, no address reuse), `:602`
  (per-connection socket timeout), `:607-630` (whole-request deadline
  enforcement), `:652-654` (`hmac.compare_digest`), `:1253`
  (`secrets.token_urlsafe(32)`), `:1257-1270` (dual-stack `127.0.0.1`/
  `[::1]` bind).
- `docs/development/task0152-probes/provision_keychain_item.py` — D22
  provisioning UX; `:38-41` (`SERVICE_NAME`), `:118-132`
  (`_reject_unsafe_for_security_line`), `:149-151`
  (`_read_secret_getpass`), `:192-208` (`_item_exists`), `:212-221`
  (`_store_via_security_interactive`, `-T ""` Keychain-trust precedent),
  `:353-372` (OS-user-database account, existence-refusal logic).
  `owner_positive_probe.sh` (Option A's probe) was deleted in round 4,
  withdrawn unrun — not cited as a live source.
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
- `docs/architecture/INVARIANTS.md` — INV-007, INV-008, INV-013, INV-069,
  INV-076, INV-082, INV-093, INV-099, INV-106, INV-146, INV-147, INV-148,
  INV-149, INV-151, INV-152.
- `planning/tasks/queue.yaml` — TASK-0152 (this task), TASK-0112,
  TASK-0114 (`depends_on`).
