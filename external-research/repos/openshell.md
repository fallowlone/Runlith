# Repository

`NVIDIA/OpenShell`

- repository: `OpenShell`
- url: https://github.com/NVIDIA/OpenShell
- default_branch: `main`
- reviewed_revision: `main@eeba0e7954c0fb4d8e9e2e29d1bfa68e290eb8b3`
- commit_sha: `eeba0e7954c0fb4d8e9e2e29d1bfa68e290eb8b3`
- review_date: `2026-10-10`
- resource_type: GitHub repository + hosted docs (`https://docs.nvidia.com/openshell/latest/`)
- version_observed: docs `Latest (v0.1.3)`; workspace `Cargo.toml` version `0.0.0`, edition 2024, Rust 1.94

# Reviewed revision

Evidence collected 2026-10-10 without a local clone (shell unavailable in this
environment). Sources actually inspected:

- GitHub landing page + `README.md` via raw fetch.
- `Cargo.toml` workspace manifest via raw fetch.
- `crates/` directory listing via GitHub Contents API (38 crates).
- `docs/about/architecture.mdx` via raw fetch.
- Hosted docs pages: sandbox policies overview, policy prover, providers
  overview, sandbox management lifecycle.
- `SECURITY.md` via raw fetch.
- Latest commit SHA/date/message via GitHub Commits API.

Not inspected at source level: Rust implementation of supervisor, sandbox
backend, Landlock/seccomp code, gateway auth, prover SMT encoding, middleware
chain, Kubernetes driver. Claims about kernel behavior below are therefore
marked as documentation-verified, not source-verified. No external code was
copied into Runlith, per `AGENT.md` §16.1.

# Research target

Evaluate OpenShell's sandbox/policy/credential architecture as a secure
runtime for autonomous coding agents against Runlith's G5 provider execution,
G6 Policy/Secrets/Runner/Audit, G8 verification, and deferred container /
Linux / Windows isolation work. Identify missing capabilities, improvements,
redesigns, replacements, inspirations, validations, and rejections.

# Executive summary

OpenShell is the safe, private runtime for fleets of autonomous AI agents. It
is a gateway + supervisor + sandbox system: a control plane (gateway) manages
sandbox lifecycle, policy, providers, and audit; a trusted supervisor enforces
policy and injects credentials; an untrusted sandbox runs the agent under
kernel controls (Landlock, seccomp user-notify) and an outer network fence
whose only egress is the mediated supervisor channel. Policy changes are
checked by an SMT-based prover (`z3`, standalone `openshell-prover`) for
boundary containment and proposal risk before approval.

Relevance to Runlith is high but narrow: OpenShell owns the **execution
isolation + network/credential enforcement** layer that Runlith's G6
deliberately left open (per-OS backend matrix, Linux container backend,
network isolation beyond macOS Seatbelt TCP/443, provider credential
confinement). It does not own durable Task graphs, planning, verification, or
memory; those Runlith differentiators are absent in OpenShell.

Most valuable, in priority order:

1. SMT-based policy boundary/proposal checks as a pre-approval review tool
   (ADAPT, not a replacement for Runlith's deterministic Policy Engine).
2. Endpoint-bound credential placeholder injection with dual authorization
   (network policy + credential binding must both pass) and fail-closed
   mismatch handling.
3. Supervisor/sandbox split with explicit non-decision rule for the untrusted
   side, per-generation JWTs, freeze-on-disconnect, and honest capability
   advertisement per runtime.
4. Outer network fence per runtime + provider-contributed network rules with
   global-policy override semantics as negative/positive lessons for Runlith's
   layered policy.

No wholesale adoption is justified. OpenShell's operational cost (Docker /
Podman / Kubernetes / MicroVM drivers, gateway service, Helm, CNI
`NetworkPolicy` dependence, telemetry default) conflicts with Runlith's
local-first single-daemon modular monolith.

# Resource information

- Original URL: https://github.com/NVIDIA/OpenShell
- Docs: https://docs.nvidia.com/openshell/latest/
- License: Apache-2.0 (`LICENSE`), per landing page and raw `README.md`.
- Scale signals: ~15.7k stars, ~1.8k forks, 1,687 commits on `main`,
  392 issues, 198 PRs (landing-page snapshot; not a quality claim).
- Language/stack: Rust workspace (`crates/*`), providers directory,
  Python/TypeScript/Go/Rust SDKs, `proto/`, `deploy/` (Helm), `nix/`,
  `e2e/`, `skills/`, `rfc/`, `telemetry/`.
- Research date: 2026-10-10.

# Core concepts

## 1. Gateway / supervisor / sandbox separation

The gateway is the control plane (identity, sandbox records, policy
distribution, provider records, session relay). The supervisor lives on the
trusted side of the boundary, decides policy, resolves DNS, opens approved
connections, supplies credentials, and relays to the gateway. The sandbox
lives with the untrusted agent, owns agent processes, identifies the calling
program from trusted `/proc` observations, applies process controls, and
forwards TCP/DNS to the supervisor. The sandbox never makes policy decisions.

## 2. Kernel-level enforcement + outer fence

Per docs: Landlock LSM confines filesystem access; seccomp user notification
stages network operations; workload runs as one non-root identity with no
Linux capabilities (current Linux backend). The outer fence denies all direct
egress except the protected supervisor channel (container networking off for
Docker/Podman, `NetworkPolicy` for Kubernetes, no network device for
MicroVM). Filesystem, process, and network enforcement are distinct axes.

## 3. Deny-by-default policy with static + dynamic parts

Policy YAML `version: 1` has `filesystem_policy`, `landlock`,
`process` (fixed at startup/creation) and `network_policies`,
`network_middlewares` (changeable at runtime). Network defaults to deny; each
rule names destinations, authorized binaries, and optionally request-level
restrictions (e.g. read but not write). Providers contribute additional
network rules; effective policy = base + provider rules.

## 4. Formally verified policy changes

Two prover checks: (a) boundary check — candidate policy allows nothing
beyond a boundary policy in modeled domains; (b) proposal risk check —
agent-proposed network rule adds no risky access (new credentialed reach, new
HTTP methods, cloud metadata) compared with current policy. Findings block
auto-approval and route to human review. Standalone `openshell-prover check
candidate.yaml --boundary boundary.yaml` with `within_boundary /
exceeds_boundary / error / unsupported / inconclusive` and JSON output for
agents. SMT solver is `z3` (workspace dependency).

## 5. Credential providers with placeholder injection

Providers bind a service name to stored credentials + profile metadata
(endpoints, binaries, refresh). The agent sees only opaque placeholders in
env; the supervisor proxy resolves them just before forwarding, only when
(a) network policy allows binary+destination and (b) credential binding
includes host/port/path. Mismatch, unknown, malformed, or expired
placeholders fail closed (HTTP 403 `credential_endpoint_mismatch`) without
logging secrets. Refresh strategies include OAuth2 refresh/client-credentials,
Google service-account JWT, AWS STS `AssumeRole` (atomic triple-write), plus
external push via `provider update`.

## 6. Runtime-agnostic isolation backend

One isolation-backend interface shared by all runtimes; drivers translate
core behavior to platform tools and advertise capabilities. Same policy
behaves the same everywhere; readiness/feature differences are explicit, not
silent downgrades.

## 7. Agent-first extensibility

Middleware/interceptors, compute drivers, supervisor middleware builtins,
provider profiles, skills (`skills/` + `npx skills add NVIDIA/OpenShell`),
and four SDKs (Python/TS/Go/Rust). Gateway and supervisor define behavior;
drivers are thin translators.

# Architecture and implementation

## Component map (docs-verified)

```text
operator / SDK / CLI
  -> gateway (auth, sandbox records, policy, providers, logs, sessions)
    -> compute driver (docker | podman | kubernetes | vm)
      -> supervisor (trusted: policy check, DNS, egress, credential inject)
      <-> Sandbox Protocol (mut-auth HTTP/2, per-TCP-connection streams)
      -> sandbox workload (untrusted agent + sandbox shim)
      -> outer fence (net off / NetworkPolicy / no net device)
```

Source paths (names only, not copied): `crates/openshell-gateway`,
`crates/openshell-supervisor`, `crates/openshell-supervisor-network`,
`crates/openshell-supervisor-process`, `crates/openshell-sandbox`,
`crates/openshell-sandbox-backend`, `crates/openshell-isolation-interface`,
`crates/openshell-policy`, `crates/openshell-policy-schema`,
`crates/openshell-prover`, `crates/openshell-prover-cli`,
`crates/openshell-providers`, `crates/openshell-driver-docker`,
`crates/openshell-driver-podman`, `crates/openshell-driver-kubernetes`,
`crates/openshell-driver-vm`, `crates/openshell-driver-mxc`,
`crates/openshell-gateway-interceptors`,
`crates/openshell-supervisor-middleware(-builtins)`,
`crates/openshell-sdk`, `crates/openshell-cli`, `crates/openshell-tui`,
`crates/openshell-core`, `crates/openshell-ocsf`, `crates/openshell-otel`,
`providers/` (example profiles), `proto/`, `deploy/` (Helm).

## Execution flow

1. `openshell sandbox create --from <image> [--provider X] [--policy Y] -- <agent-cmd>`.
2. Gateway validates effective policy + provider config before workload start;
   `ConfigurationInvalid` keeps workload unstarted with repairable
   `Provisioning` state.
3. Driver provisions workload + supervisor, establishes protected channel,
   builds fence; supervisor confirms boundary before agent start.
4. Sandbox launches agent as owned child, provides exec/terminal/signals,
   identifies binaries from trusted observations.
5. Agent TCP/DNS intercepted (seccomp) and forwarded over Sandbox Protocol to
   supervisor; supervisor checks policy, injects credentials if bound, opens
   real connection, relays traffic with per-stream backpressure.
6. Network policy/middleware can change at runtime; filesystem/process
   identity fixed at startup.
7. Disconnect freezes agent; only the same supervisor process can resume; new
   supervisor cannot take over even with valid credentials. Generation-scoped
   JWTs + fresh TLS certs per restart; old credentials stop working.

## Authentication

Gateway is the sole credential signer; every credential names exactly one
sandbox+generation. Supervisor-to-gateway uses gateway JWT over TLS;
supervisor-to-sandbox uses mTLS + sandbox JWT (sandbox holds only gateway
public key, can verify but not mint). Bootstrap via driver-injected files
(Docker/Podman/MicroVM) or ServiceAccount-token verification (Kubernetes).
Supervisor keeps tokens in memory, renews together; deletion cuts off
supervisor. Agent/sandbox holds no gateway JWT, no signing key, no provider
secrets.

## Policy lifecycle

Selection order: global policy (if set, replaces per-sandbox policy, blocks
edits/approvals, suppresses provider rules) → sandbox saved policy
(`--policy` over `OPENSHELL_SANDBOX_POLICY`) → image policy (invalid blocks
start) → restrictive default. Base + provider rules = effective. Agent can
propose narrow network rules via advisor; prover risk check gates
auto-approval; human approves. Prover also usable in CI.

## Sandbox lifecycle

Phases `Provisioning → Ready → Stopping/Stopped/Starting`, plus
`Completed/Error`, restart policies (`never/on-failure/always` with
backoff), ephemeral `--no-keep`, CPU/memory/GPU flags (K8s request=limit,
Docker/Podman CDI round-robin, VM single-GPU), templates, labels/selectors,
port forwarding, SSH config, upload/download with symlink preservation and
Git-ignore filtering, service exposure with bearer-passthrough option, VS
Code/Cursor integration.

## Observability

`openshell logs`, `openshell term` dashboard (gateways/providers/sandboxes),
endpoint statuses for MCP-over-HTTP tool servers (`NoObservedExchange /
HttpResponseReceived / PolicyDenied / CredentialUnavailable / TlsFailed /
TransportFailed / UpstreamRejected`), Prometheus metrics, OpenTelemetry OTLP,
OCSF crate. Telemetry collected by default (operational counts only),
opt-out via `OPENSHELL_TELEMETRY_ENABLED=false` or compile-out; community
reports published.

# Security analysis

## Enforced technically (per docs)

- Untrusted-side non-decision rule: sandbox reports, supervisor decides.
- Kernel confinement (Landlock FS, seccomp-staged net, non-root, no caps on
  current Linux backend) + outer egress fence per runtime.
- Deny-by-default network; binary-identity-aware rules; request-level
  REST/method/path restrictions.
- Dual authorization for secrets (policy + binding) with fail-closed
  mismatch/unknown/expired handling and no secret logging.
- Generation-bound, sandbox-bound credentials; sandbox holds no minting key;
  freeze-on-disconnect; no takeover by new supervisor.
- Pre-start configuration admission; invalid policy blocks workload.
- Prover blocks auto-approval on risky/boundary-exceeding changes; explicit
  `unsupported`/`inconclusive` treated as failure, not silent pass.
- Program identity from trusted `/proc`, not agent-claimed paths.
- SSH host keys per sandbox, pinned via gateway-authenticated fingerprint.

## Documented but not source-verified here

Kernel code paths, seccomp filter completeness, Landlock rule compilation,
TOCTOU handling on FS paths, symlink behavior in enforcement (prover
explicitly returns `unsupported` for nested-path comparisons because image
symlinks could escape — enforcement behavior unconfirmed), DNS mediation
robustness, proxy body-rewrite scope, side-channel resistance, audit
tamper-evidence, gateway credential-store hardening, Kubernetes
`NetworkPolicy` reliance on cluster CNI actually enforcing it.

## Assumptions and limitations

- Prover guarantees apply only to modeled domains
  (`filesystem,network_l4,network_rest,process,landlock`); GraphQL/MCP/
  WebSocket/JSON-RPC, audit-mode endpoints, mixed REST+raw endpoints on same
  host/port, query-param rules, `?`/bracket paths, IPv6 hosts, non-ASCII,
  glob-vs-exact binary mappings, UID changes, and large policies return
  `unsupported`/`inconclusive`. Passing boundary ≠ minimal/safe; passing
  proposal-risk ≠ within boundary.
- Filesystem/process policy fixed at startup; only network is dynamic.
- Global policy replaces (not intersects) per-sandbox policy — coarse.
- Telemetry on by default (opt-out), conflicting with Runlith's no-telemetry
  core (INV-014).
- Kubernetes isolation depends on CNI enforcing `NetworkPolicy`; misconfigured
  CNI silently weakens the fence unless separately verified.
- `BYPASS` risks typical of proxy injection: raw `tls: skip`/non-HTTP tunnels
  opaque (no rewrite, correctly fail closed for credential use, but agent can
  still exfiltrate over allowed raw channels if policy permits them); cookies/
  responses/binary frames not rewritten; body rewrite opt-in per endpoint.
- Supply chain: OCI images, provider profiles, middleware, drivers, and agent
  CLIs are trusted inputs; malicious image/profile is outside the enforcement
  model described.

## Attack surfaces

Malicious repo/prompt injection inside sandbox (contained by, not solved by,
isolation); malicious tool-server responses; compromised provider CLI/binary
with valid interface; malicious OCI image or provider profile widening
effective policy; overly broad network rules allowing raw tunnels; stolen
gateway JWT within generation window; misconfigured global policy; CNI not
enforcing policy; operator scope misconfiguration; credential-word env vars
warned but not blocked (`--env` secrets visible to agent — documented
warning only).

Unverified: receipt/attestation model for started/running/exited isolation
states; whether denials/audit are hash-chained or externally signed; whether
policy evaluation is deterministic and fingerprinted like Runlith's
`CompiledPolicy`.

# Comparison with our project

## OSH-01 — Policy decision vs policy verification

- External: OpenShell gateway/supervisor decides at runtime; `openshell-prover`
  (Z3 SMT) verifies candidate-vs-boundary and proposal-vs-current before
  approval. Decision and verification are separate tools.
- Ours: `crates/policy` deterministic layered evaluation
  (`BuiltInHard → UserManaged → Workspace → Repository → RequestSession`,
  `DENY > REQUIRE_APPROVAL > ALLOW`, exact scope matching, parent narrowing,
  SHA-256 fingerprint) is the canonical authority; no SMT pre-check.
  `docs/security/POLICY_CONTRACT.md`, ADR-0026, INV-146..153.
- Difference: OpenShell adds a pre-decision formal review; Runlith adds
  deterministic precedence, exact-bound single-use approvals, and audit
  coupling that OpenShell docs do not describe.
- Implication: complementary, not competing. Prover-style checks could sit
  before Runlith approval without changing decision authority.

## OSH-02 — Enforcement coupling

- External: supervisor enforces adjacent to operation (DNS/TCP proxy, Landlock
  at startup, fence per runtime); sandbox holds launch until supervisor
  confirms; freeze on disconnect.
- Ours: `IsolationPlan → IsolationBackend → EnforcementReport` with exact-plan
  recheck immediately around provider launch (TASK-0064/0067/0069); preflight
  explicitly not authority (INV-153); macOS Seatbelt enforces FS/net/process
  for Claude, but Trusted Host Git path remains open (TASK-0070).
- Difference: OpenShell has a complete Linux container story; Runlith has a
  stronger exact-binding/approval/audit story but an incomplete backend
  matrix (no Linux/Windows concrete backend, no container choice).
- Implication: largest concrete gap is Runlith's missing Linux container
  backend, not its policy algebra.

## OSH-03 — Secrets handling

- External: provider profiles declare endpoints/binaries/credentials; agent
  gets placeholders; supervisor proxy injects at approved endpoints only;
  dual check (policy + binding); refresh built in (OAuth2/AWS STS/Google);
  static values never in list output (keys only).
- Ours: Secrets Broker with opaque `SecretRef`/bindings, OS
  Keychain/Credential Manager/Secret Service backends, exact
  `SecretAccess(ref)` + exact delivery-target check after current effective
  capabilities, borrowed-buffer clearing, no plaintext in SQLite/journal/logs/
  context (TASK-0063, INV-008/151).
- Difference: OpenShell optimizes for network-credential injection with
  endpoint binding and refresh; Runlith optimizes for local secret hygiene
  and approval coupling. OpenShell's endpoint binding is stricter than
  Runlith's current delivery-target model for network exfiltration; Runlith's
  OS-store + approval atomicity is stricter for local handling.
- Implication: adapt endpoint-binding as a second authorization axis for
  network-capable secret use.

## OSH-04 — Isolation scope and honesty

- External: filesystem/network/process as distinct capabilities; each runtime
  advertises what it enforces; same policy, different readiness reporting.
- Ours: identical principle already (`runners.md` §6-7, INV-076/077,
  TASK-0064 honest `Trusted Host` NativeRunner, TASK-0117 drift guard).
- Difference: vocabulary differs, principle converges.
- Implication: validation, plus a concrete reference for Linux capability
  advertisement shape.

## OSH-05 — Credentialed network reach as first-class risk

- External: prover explicitly flags new credentialed reach, new methods,
  cloud-metadata access as auto-approval blockers.
- Ours: network is an exact host/destination capability; SSRF per-hop policy
  is accepted but deferred (Web Acquisition); THR-007/024 open; no
  metadata-endpoint-specific rule evidenced.
- Difference: OpenShell names the highest-risk network deltas; Runlith models
  them generically.
- Implication: narrow, high-value rule set to adapt.

## OSH-06 — Global policy vs layered policy

- External: one global policy replaces per-sandbox policy when active.
- Ours: layered precedence where weaker layers narrow only, never widen;
  repo/session `ALLOW` insufficient for authority.
- Difference: OpenShell trades precision for operational simplicity; Runlith
  preserves least-privilege composition.
- Implication: Runlith's model is stronger; global kill-switch is an
  operational inspiration only, not a semantic replacement.

## OSH-07 — Durability/orchestration (non-overlap)

- External: sandbox lifecycle (restart policies, generations, templates) but
  no durable Task graph, no PlannerPort/PlanChangeSet, no independent
  evaluation/verification, no context/memory engines evidenced.
- Ours: Task/Execution/Attempt/Session, planning revisions, G8 claim/effect/
  side-effect verification, context/memory roadmap (G9/G10).
- Implication: no lesson to import; confirms Runlith differentiation
  (coordination/reliability/continuity/verification across providers/time).

# Identified opportunities

- OPP-01 — Pre-approval policy-diff review with formal boundary semantics.
  Problem: human approval of policy/network changes lacks a mechanical
  second opinion beyond eyeballing YAML.
- OPP-02 — Endpoint-bound secret use for network egress. Problem: a secret
  authorized for one host could be exfiltrated to another allowed host
  without a second binding check.
- OPP-03 — Linux container isolation backend behind existing
  `IsolationBackend` contract. Problem: production Linux has no concrete
  backend; NativeRunner fails closed and blocks autonomous work.
- OPP-04 — Supervisor/untrusted-side split as an explicit design guard.
  Problem: future web/document/tool runners may blur decision vs reporting
  responsibilities.
- OPP-05 — Risky-destination blocklist semantics (cloud metadata, new
  credentialed hosts, method widening) as policy-review rules. Problem:
  generic network capability does not name the riskiest deltas.
- OPP-06 — Provider profile shape (endpoints/binaries/credential metadata/
  refresh) as inspiration for Runlith provider capability declarations.
  Problem: provider capability negotiation is implicit.
- OPP-07 — Capability advertisement + drift guard already exists
  (TASK-0117); OpenShell's per-runtime advertisement is validation, not a
  new gap.

# Recommendations

## R-01 — Adapt prover-style boundary + proposal-risk checks as review-only gates (P1)

- Proposed change: read-only policy-diff reviewer that, given current
  `CompiledPolicy`, proposed delta, and an optional boundary policy,
  reports `within_boundary / exceeds_boundary / unsupported / inconclusive`
  with counterexample, without granting authority.
- Problem: OPP-01.
- Approach: independent crate behind a port; reuse existing typed scopes;
  return `unsupported` on unmodeled shapes (nested-path containment, globs,
  UID changes, non-REST protocols) exactly like OpenShell; never auto-approve
  on `unsupported`/`inconclusive`. Wire into approval UX and CI as advisory;
  Policy Engine remains sole decider (INV-146).
- Benefits: catches credentialed-reach widening and method/scope creep before
  human approval; CI-checkable boundaries for subagent policies.
- Security: strictly increases review; must not become a bypass (advisory
  only), must not leak policy content into model context.
- Complexity: Medium. SMT dependency (`z3` system lib) is heavy; start with
  deterministic subset checker, add SMT only if deterministic subset proves
  insufficient. Evaluate license/maintenance/supply-chain separately per
  AGENT.md §16.1.
- Risks: false confidence if coverage line ignored; symlink/alias soundness
  gaps; perf on large policies (cap sizes, timeout, e.g. 10s default).
- Acceptance: given a boundary allowing `/usr,/etc` read-only, candidate
  adding `/tmp` write reports `exceeds_boundary` with counterexample;
  nested-path containment returns `unsupported`; mixed REST+raw on same
  host/port returns `unsupported`; advisory never changes a `DENY`.
- Priority: P1 (High). Novel review value, bounded scope.
- Classification: `PARTIALLY_EXISTS` (deterministic engine exists, formal
  pre-review does not) → `ADAPT`.

## R-02 — Add credential-endpoint binding as a second authorization axis for network secret use (P1)

- Proposed change: when a secret release is destined for network egress,
  require both (a) current effective network capability and (b) an explicit
  credential-binding (host/port/path or profile endpoint set) to pass;
  mismatch fails closed with typed denial + audit without recording secrets.
- Problem: OPP-02, THR-024.
- Approach: extend Secrets Broker delivery check + Policy evaluation evidence
  with endpoint binding refs; provider profiles contribute allowed endpoint
  sets; audit records binding identity, not values. Keep OS-store and
  approval atomicity unchanged.
- Benefits: least-privilege credential confinement; blocks
  allowed-host-to-allowed-host secret pivoting.
- Security: reduces exfiltration path; requires careful path/method matching
  semantics and opt-in body-rewrite scope to avoid overreach.
- Complexity: Medium (contract + broker + audit + tests).
- Risks: breakage of legitimate multi-host credentials if profiles
  incomplete; needs migration path.
- Acceptance: placeholder sent to bound host succeeds; same placeholder to
  another policy-allowed host fails closed with `credential_endpoint_mismatch`
  analogue + audit; expired/unknown placeholder never forwarded.
- Priority: P1. Directly strengthens Agent Firewall.
- Classification: `PARTIALLY_EXISTS` → `ADAPT`.

## R-03 — Prototype a Linux container backend behind `IsolationBackend` using OpenShell's fence+Landlock+seccomp shape (P2, spike first)

- Proposed change: bounded spike, then a concrete Linux backend (namespaces /
  Landlock / seccomp / cgroup + outer network fence) implementing the three
  FS/net/process axes honestly.
- Problem: OPP-03; open backend matrix in `runners.md` §13; G6 follow-up.
- Approach: driver-style adapter consuming `EffectiveCapabilities +
  IsolationRequirements`; deny-by-default egress with supervisor-equivalent
  enforcement point in Runlith's operation boundary; advertise only enforced
  axes; fail closed when unavailable. Learn fence placement and
  freeze-on-disconnect from OpenShell; reimplement inside Runlith contracts,
  do not import code.
- Benefits: unblocks autonomous Linux execution without silent Trusted Host
  downgrade.
- Security: must prove FS containment (canonicalize + symlink/TOCTOU
  handling), no ambient `~/.ssh`/`~/.aws` mounts, no network-implies-FS
  confusion.
- Complexity: High (spike Medium). Requires ADR (backend choice,
  Docker/Podman selection, rootless, mount policy, GPU passthrough out of
  scope initially).
- Risks: container ≠ perfect security; CNI/driver misconfiguration;
  maintenance burden; platform-specific bugs.
- Acceptance: mandatory three-axis requirement passes on Linux with
  escape/regression keepers; unsupported scope fails before spawn; honest
  capability report matches enforcement.
- Priority: P2 until ADR + spike evidence; then re-evaluate as P1 if Linux is
  a release target.
- Classification: `NOVEL_GAP` (for Linux) → `ADAPT` with 1% improvement:
  exact-bound approval/audit coupling that OpenShell docs do not evidence.

## R-04 — Harden runner/operation-owner split with an explicit non-decision rule for untrusted reporters (P2)

- Proposed change: document and test that okol execution reporters (tool
  shims, browser/document workers, future Policies) report observations and
  never decide; add architecture-fitness check.
- Problem: OPP-04.
- Approach: small ADR/contract note + contract tests asserting decision
  authority lives only in Policy Engine; reporters cannot mint capabilities
  or approvals.
- Benefits: prevents confused-deputy drift as new runners are added.
- Complexity: Low.
- Acceptance: new runner test fails if reporter path can produce
  `EffectiveCapabilities` without Policy decision.
- Priority: P2.
- Classification: `ALREADY_DESIGNED` in spirit (INV-146, mechanism/policy
  split INV-112) → `EXTERNAL_VALIDATION` + small `ADAPT`.

## R-05 — Adopt risky-destination review rules for network deltas (P2)

- Proposed change: proposal-risk checklist blocking auto-approval on new
  credentialed hosts, new methods/paths, cloud-metadata/link-local/LAN
  destinations, and raw-tunnel endpoints; human review required.
- Problem: OPP-05.
- Approach: policy-review rules in approval UX/CI, independent of SMT
  backend; start deterministic, promote to R-01 prover later.
- Benefits: cheap, immediate reduction in silent egress widening.
- Complexity: Low-Medium.
- Acceptance: proposal adding metadata-IP reach is flagged and blocked from
  auto-approval; evidence cites rule + diff.
- Priority: P2.
- Classification: `PARTIALLY_EXISTS` (generic network capability exists,
  named risk rules do not) → `ADAPT`.

## R-06 — Use provider-profile shape as inspiration for capability declarations (P3)

- Proposed change: versioned provider capability/profile data (endpoints,
  binaries, credential metadata, refresh) consumed by routing/policy as
  evidence, never as authority.
- Problem: OPP-06.
- Approach: data-driven catalog behind provider ports; no provider branches
  in domain logic (INV-090); provenance/version/drift detection.
- Benefits: explainable routing, tighter policy authoring, refresh story.
- Complexity: Medium; defer until G14 router shadow evidence needs it.
- Priority: P3 (optional, post-G8/G9).
- Classification: `ALREADY_DESIGNED` (Tools/Skills registry, Router profiles)
  → `INSPIRED_ONLY`.

# Rejected or inapplicable ideas

- **Replace Runlith Policy Engine with OpenShell policy YAML/prover.**
  Reason: OpenShell decides per-sandbox with YAML + advisory proofs; Runlith
  requires deterministic layered precedence, exact-bound single-use
  approvals, parent narrowing, fingerprinting, and audit atomicity with
  security impact beyond what OpenShell evidences. Replacement would weaken
  guarantees. (REJECT / CONFLICTS_WITH_ARCHITECTURE.)
- **Adopt gateway + multi-runtime fleet ops (Helm, K8s, MicroVM, remote
  gateways) as Runlith's runtime.** Reason: operational and dependency weight
  contradicts local-first single-daemon modular monolith, offline-first core,
  and Pareto focus on durable Task/verification differentiation. Revisit only
  for G12 Remote Worker with its own ADR. (REJECT for v1.)
- **Global policy replacing per-task policy.** Reason: weaker than Runlith's
  narrowing-only layers; repo/session self-grant becomes structurally
  possible under replacement semantics. Keep as operational kill-switch
  inspiration only. (REJECT as semantics; INSPIRED_ONLY as ops control.)
- **Telemetry default-on.** Reason: conflicts with INV-014 (core works
  without telemetry) and local-first privacy. (REJECT.)
- **CNI-dependent network isolation as sole enforcement.** Reason: silently
  weakens when CNI misconfigured; Runlith requires honest capability
  advertisement + fail-closed + drift guard. (REJECT as sole mechanism.)
- **Importing OpenShell crates as dependencies or copying sandbox/prover
  code.** Reason: prohibited by AGENT.md §16.1; license/maintenance/
  architecture mismatch. Reimplement adapted concepts inside Runlith
  contracts. (REJECT.)
- **Parity SDK matrix (Python/TS/Go/Rust) now.** Reason: Runlith's sequencing
  is `agentctl`-first (ADR-0016); SDK breadth before G8/G9 is scope creep.
  (REJECT for now.)

# Proposed implementation plan

Independent, ordered by leverage/cost (no implementation authorized by this
research file):

1. R-05 risky-destination review rules (small, deterministic, immediate).
2. R-04 non-decision rule + contract tests (small, prevents drift).
3. R-02 credential-endpoint binding (medium, needs contract + broker + audit
   changes + migration; requires ADR amendment to ADR-0026/POLICY_CONTRACT).
4. R-01 prover-style advisory checker (medium; deterministic subset first,
   SMT only on evidence; requires ADR for dependency/model/coverage
   semantics).
5. R-03 Linux container backend spike → ADR → implementation (high; after
   G8, coordinated with runner roadmap).
6. R-06 provider profiles (defer to G14 router work).

Each step needs acceptance tests, adversarial keepers for enforcement paths,
audit evidence assertions, and docs updates. No step changes public
versioning or cross-context storage reads.

# Evidence and references

Directly verified:

- https://github.com/NVIDIA/OpenShell (landing: description, stars/forks/
  issues/PRs snapshot, Apache-2.0, docs link).
- https://raw.githubusercontent.com/NVIDIA/OpenShell/main/README.md (How It
  Works, Quickstart, Sandboxes/Policies/Providers/Gateways/K8s/Extensibility
  links, skills, SDK table, telemetry opt-out).
- https://raw.githubusercontent.com/NVIDIA/OpenShell/main/Cargo.toml
  (workspace `crates/*`, tokio/tonic/axum/rustls/clap/sqlx/kube/z3 deps,
  Rust 1.94, edition 2024).
- GitHub Contents API `crates` (38 crate names listed in §Architecture).
- GitHub Commits API `main` → `eeba0e7954c0fb4d8e9e2e29d1bfa68e290eb8b3`,
  2026-10-10, `refactor(runtime): confine and reduce unsafe code (#4130)`.
- https://raw.githubusercontent.com/NVIDIA/OpenShell/main/docs/about/architecture.mdx
  (gateway/supervisor/sandbox roles, Sandbox Protocol, per-runtime fence
  table, JWT/generation auth, driver/auth details).
- https://docs.nvidia.com/openshell/latest/how-it-works/policies/overview
  (deny-by-default, five policy sections, base vs effective, global override).
- https://docs.nvidia.com/openshell/latest/how-it-works/policies/prover
  (boundary vs proposal-risk checks, CLI examples, result codes, coverage
  domains, unsupported/inconclusive semantics, limits).
- https://docs.nvidia.com/openshell/latest/how-it-works/providers/overview
  (creation/update/refresh flows, placeholder injection table, dual
  authorization, fail-closed mismatch, AWS STS/Google/OIDC details).
- https://docs.nvidia.com/openshell/latest/how-it-works/sandboxes/overview
  (lifecycle, restart policies, CPU/memory/GPU, images, SSH, forwarding,
  service exposure, endpoint statuses).
- https://raw.githubusercontent.com/NVIDIA/OpenShell/main/SECURITY.md
  (private disclosure via NVIDIA PSIRT, no GitHub issues).

Inferred (not source-verified): kernel-enforcement completeness, TOCTOU/
symlink handling in enforcement, audit tamper-evidence, gateway store
hardening, CNI enforcement in practice. Marked as such in §Security.

Runlith references: `AGENT.md` §§4/7/16.1/16.2, `ARCHITECTURE.md` (Runner,
Policy, Secrets, Audit contexts), `docs/architecture/INVARIANTS.md`
(INV-076/077/146..153, INV-008/151, INV-112, INV-014, INV-028/090),
`docs/security/POLICY_CONTRACT.md`, `docs/security/THREAT_MODEL.md`
(THR-007/024), `docs/architecture/runners.md`, ADR-0013/0026, TASK-0061..0069
and TASK-0070 re-audit, `planning/roadmap.yaml` (G8/G14).

# Final assessment

OpenShell is the strongest external reference yet for Runlith's missing
**concrete isolation + credentialed-egress enforcement** layer. Its
supervisor/sandbox split, outer fence, dual-authorized placeholder
injection, and SMT-assisted policy review are genuinely valuable and map
cleanly onto Runlith's Policy/Runner/Secrets boundaries as adaptations —
not replacements. Its fleet/gateway/Kubernetes machinery, global-replace
policy semantics, default-on telemetry, and SDK breadth are inapplicable to
Runlith v1's local-first monolith.

Recommended next steps: keep this file as evidence; consider narrow follow-up
tasks for R-05 (cheap), R-04 (guardrail), R-02 (endpoint binding), and an
R-01 deterministic subset checker; require a spike + ADR before any Linux
container backend (R-03). Do not import OpenShell code or add it as a
dependency.

## Novelty / adoption summary

| Finding | Novelty | Adoption |
|---|---|---|
| SMT boundary + proposal-risk pre-review | `PARTIALLY_EXISTS` | `ADAPT` (R-01) |
| Endpoint-bound credential injection | `PARTIALLY_EXISTS` | `ADAPT` (R-02) |
| Linux fence + Landlock/seccomp backend shape | `NOVEL_GAP` (Linux) | `ADAPT` via spike (R-03) |
| Untrusted-side non-decision rule | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` + small adapt (R-04) |
| Risky-destination review rules | `PARTIALLY_EXISTS` | `ADAPT` (R-05) |
| Provider profile catalog | `ALREADY_DESIGNED` | `INSPIRED_ONLY` (R-06) |
| Honest per-runtime capability advertisement | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` |
| Gateway fleet / Helm / global-replace / telemetry-on / SDK parity | `CONFLICTS_WITH_ARCHITECTURE` or out-of-scope | `REJECT` |
