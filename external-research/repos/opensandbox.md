# Repository

`opensandbox-group/OpenSandbox`

- repository: `OpenSandbox`
- url: https://github.com/opensandbox-group/OpenSandbox
- default_branch: `main`
- reviewed_revision: `main@fdb06ac7d392a1d8570e563857d796bacd7250c4`
- commit_sha: `fdb06ac7d392a1d8570e563857d796bacd7250c4`
- review_date: `2026-10-10`
- resource_type: GitHub repository (FastAPI lifecycle server + Go execd + egress sidecar + K8s operator + Firecracker microVMs)
- version_observed: server ≥0.2.0 / egress ≥1.1.1 era; 15.8K stars / 1.5K forks snapshot; Apache-2.0

# Reviewed revision

Evidence collected 2026-10-10 without a local clone. Sources actually
inspected at source level via raw fetch at the pinned SHA:

- Landing `README.md` (features, quick start, SDK/CLI/MCP, registries +
  Cosign signing, docs map).
- Latest commit via GitHub Commits API (`fdb06ac7`, 2026-10-10,
  `fix(egress): verify quarantine across older nft readback formats`).
- `docs/architecture/index.md` (full: 3 paths, 6 surfaces, control/data/
  network planes, Docker/K8s/FastSandbox backends, 10 reference links).
- `docs/guides/credential-vault.md` (full: MITM injection, 5 auth types,
  substitutions, ETag snapshot, in-memory warning, binding guidance).
- `docs/architecture/network/egress.md` (full: DNS+nft layers, precedence,
  fail-closed, TLS intercept scope, OSEP-0023 gate, quarantine fence).
- `docs/architecture/fast-sandbox/performance.md` (full: ACK 2026-10-08
  numbers with saturation + resume-gap warnings).
- `SECURITY.md` (reporting, supported versions, TLS key-length policy).
- `specs/` inventory via Contents API (lifecycle 89 KB, execd 117 KB,
  egress 22 KB, diagnostic 13 KB + tests).

Not inspected at source level: `server/` Python, `components/execd` Go,
`components/egress` proxy code, `kubernetes/` operator, `cli/`, SDKs,
`specs/*.yml` bodies, OSEPs, `tests/`. Claims about handler internals are
therefore doc-inferred. No external code copied into Runlith (§16.1).

# Research target

Evaluate OpenSandbox's sandbox runtime for AI agents against Runlith's G6
Policy/Secrets/Runner/Audit + agentd/agentctl isolation. Identify missing
capabilities, improvements, redesigns, replacements, inspirations,
validations, rejections.

# Executive summary

OpenSandbox runs untrusted agent code in isolated sandboxes on own infra:
Docker locally, Kubernetes (BatchSandbox/agent-sandbox providers, Pool
pre-warm, task orchestration) + external FastSandbox Firecracker microVMs
(64 ms p50 serial warm create, snapshot restore not kernel boot) in
cluster. Contract-first: `specs/` OpenAPI (lifecycle `/v1`, execd, egress,
diagnostic) is source of truth, SDKs generated + handwritten. Control plane
(FastAPI: auth, validation, persistence of snapshots/template catalog in
SQLite default or Postgres, endpoint resolution, proxy) vs data plane (Go
`execd`: SSE commands, sessions, PTY WS, files, Jupyter code contexts,
metrics, optional access token) vs network plane (ingress gateway modes,
per-sandbox egress sidecar sharing netns, `NET_ADMIN` held only by
sidecar, platform deny.always>allow.always overlays, fail-closed startup).

Egress: DNS proxy (denied→NXDOMAIN, allowed→resolve+record) + `dns+nft`
(default-deny, DNS-learned bounded leases, TCP grace) + transparent MITM
(port 80/443 always, extras decrypt-only, IPv4+IPv6, 503-not-ready, CA
delivered to system/NSS/JDK). Credential Vault: real secrets in sidecar
memory only, fake env in workload, exact-match (scheme/host/port/method/
path) header injection (bearer/basic/apiKey/customHeaders/passthrough) +
scoped placeholder substitutions (path/query/header/body, one-pass, no
re-scan, redaction set), single-binding-wins else 403/drop, ETag snapshot
(304 reuse / 200 validated replace / 404 clears), fail-closed on socket/
snapshot errors (buffered→503, streaming→terminate). Requires
`dns+nft` + default-deny + CredentialProxy enabled + no mesh sidecar;
vault is process-local (K8s pause wipes it — re-inject after resume);
`match.ports` deprecated (scheme-derived 80/443).

Relevance to Runlith is direct on G6 Runner/Secrets/Audit: OpenSandbox
ships the network-isolated, credential-blind execution substrate Runlith's
Runner port abstracts but Seatbelt-only macOS cannot provide on Linux, plus
a fail-closed egress + placeholder-injection reference for the Secrets
broker/delivery work. It does not own durable Task graphs, capability
approvals, or independent action verification — those Runlith
differentiators are absent (no Task/Policy/Evaluation here).

Most valuable, in priority order:

1. Egress fail-closed + DNS/nft dual-layer + platform overlays as Runner
   network requirements (ADAPT).
2. Credential Vault placeholder-injection + exact-match + redaction +
   fail-closed-vault as Secrets delivery reference (ADAPT).
3. Protocol-first specs + control/data-plane split as service-boundary
   validation (EXTERNAL_VALIDATION).
4. Pause/resume/snapshot semantics (rootfs-commit vs VM-checkpoint vs
   public snapshot API) as lifecycle vocabulary (INSPIRED_ONLY).
5. Perf honesty (saturation + resume-gap warnings, raw report published)
   as measurement pattern (INSPIRED_ONLY).
6. Release verification (Cosign keyless + attestations + digest pinning)
   as supply-chain checklist (EXTERNAL_VALIDATION).

# Resource information

- Type: FastAPI server + Go execd/egress/ingress + K8s controller +
  FastSandbox gRPC FastPath v2 (:9090) to external microVM platform;
  5 language SDKs + `osb` CLI + MCP server (lifecycle/commands/text-files).
- Runtime: Docker (local) → Kubernetes (cluster); images in 3 registries,
  signed keylessly, pin-by-digest; templates catalog in DB, golden builds
  via SandboxTemplate CRs.
- License: Apache-2.0; OSEPs for design direction; Discord/DingTalk;
  OpenSSF Best Practices + CNCF Landscape badges.
- Distribution: `uvx opensandbox-server init-config` + `uvx
  opensandbox-server`; `uv pip install opensandbox`; per-request registry
  auth, image pull secrets, RuntimeClass/secure runtimes (Kata supported,
  gVisor not — netstack lacks redirect).
- Status: active main (3,401 commits); issues 66 / PRs 59 snapshot;
  FastSandbox adapter rejects `credentialProxy.enabled` + volumes on that
  path; MCP exposes no SQL; pooled sandboxes can't gain per-request
  sidecars (controls go in Pool template).

# Core concepts

## 1. Three paths, six surfaces

Lifecycle (orange: SDK→server→runtime), execution (blue: client→execd/
proxy/ingress direct), outbound (green: sandbox→egress→external). Client /
protocol / control-plane / backends / data-plane / network-security split
is intentional: SDKs depend on specs, server orchestrates, providers own
platform resources, execd/egress own in-namespace ops.

## 2. Egress deny-by-default with teeth

Create-time `networkPolicy` + runtime `PATCH /policy`; first-match
platform-deny > platform-allow > user-policy; overlays hot-reload/min from
sidecar image files (publish only after nft accept). No `NET_ADMIN` in
workload; DoT:853 always dropped, DoH-over-443 one-switch; supervisor +
backoff + crash-loop breaker; shutdown keeps rules during bounded drain.
Experimental OSEP-0023 revision runtime + quarantine fence (`inet
opensandbox_quarantine`, input/output/forward incl. loopback, confirmed by
exact kernel readback) documented with explicit non-guarantees (no zero-
leak before detect, no cross-restart containment, destroy-not-restart).

## 3. Credential Vault: workload never holds secret

Host SDK writes creds+bindings to sidecar; workload env fake/empty;
MITM matches exactly one binding → injects; ambiguous/overlap rejected;
path-confusion (dot-segments/backslash/non-convergent decoding) → 403;
encoded-slash allowed only if every decode depth matches same binding.
Plaintext write-only (get/list/patch sanitized); active vault over Unix
socket, ETag-tagged (public revision may reset to 1 — tag differs, no
plaintext reuse). Vault-socket failure fails closed (never forwards
unexamined). Env knobs: require-TLS, trusted-proxy CIDRs, require-scoped-
match (fail-closed guard: non-empty methods+paths, no host-wide `/*`).

## 4. Execution plane with streaming + tokens

execd: `/ping`, `/code[/contexts|/context]`, `/session`, `/command` +
status/background logs, `/files/*`, `/directories`, `/metrics[/watch]`,
`/pty` WS; SSE streaming; `X-EXECD-ACCESS-TOKEN` optional; Jupyter bridge
(HTTP/WS → streaming events); default 200 ms stream-close grace (explains
~203 ms echo p50 — not shell startup). Code-interpreter image versioned via
env (`PYTHON_VERSION` etc.), maintained in `sandbox-images` repo.

## 5. Lifecycle with identity-preserving resume (scoped)

Docker pause = container freeze (sidecar-first ordering); BatchSandbox =
rootfs-commit/recreate or QEMU VM-state path (same sandbox ID); FastSandbox
= FastPath checkpoint/restore (re-resolve endpoints after). Public snapshot
API (Docker local-image commit; BatchSandbox SandboxSnapshot CR → OCI push;
create-from-snapshot resolves image) distinct from internal pause/resume.
Measured: pause→durable ~61 s (11 s frozen, 46 s publish 4 GiB); same-node
resume SDK ~63 s vs runtime-restore ~35 ms (gap unresolved, endpoint/guest
availability suspected — stated, not hidden).

## 6. Access + persistence + observability

API-key auth + startup guardrails for unauthenticated mode; resource limits
(CPU/mem/GPU), capabilities/AppArmor/seccomp/PID settings; `secureAccess`
endpoint creds + signed route tokens (K8s ingress); renew-on-access TTL
(via proxy or Redis ingress events, per-sandbox opt-in). Store: SQLite
default (`~/.opensandbox/opensandbox.db`) or Postgres; snapshots + template
catalog persisted. State carries state/reason/message/transition-time;
request IDs propagated; execd metrics + OTLP sidecar metrics (bounded
cardinality, no domain labels); denied vs resolver-failure counters kept
separate deliberately.

# Architecture and implementation

## Component map (doc-verified structure, code-unread)

- `server/opensandbox_server/` — main/middleware (API-key, request-ID) /
  api (lifecycle/proxy/pool/diagnostics/schemas) / services (Docker,
  K8s providers, Composite router by `templateId` vs image/snapshot and
  `fsb-` prefix, FastSandbox adapter) / repositories (snapshots,
  templates) / renew_intent / k8s helpers+informers.
- `components/execd/` (Go/Gin), `components/egress/` (DNS/nft/MITM/vault),
  `components/ingress/` (header/URI/wildcard-host, BatchSandbox annotation
  vs agent-sandbox FQDN vs FastPath resolve).
- `kubernetes/` (BatchSandbox/Pool/SandboxSnapshot CRDs), `cli/` (`osb`
  sandbox/command/file/egress/devops/skills), `sdks/` ×5 + `sdks/mcp`,
  `specs/` ×4 + tests, `oseps/`, `manifests/`, `tests/`, `examples/`
  (Claude Code/Codex/DeerFlow/Playwright/Chrome/Desktop/Harbor-eval).

## Execution flow

Create (validate→route→provision→stage execd/bootstrap→Running/Failed,
async — poll or SDK readiness) → resolve endpoint (host-mapped / ingress /
server-proxy with header rebasing rules) → exec via execd (SSE/WS, token
when required) → outbound via egress (DNS→nft→MITM→vault-inject) → pause/
resume/snapshot per backend semantics → destroy (9 s sidecar stop budget
on Docker; best-effort telemetry flush).

## Authentication

Server API-key; execd shared token (`X-EXECD-ACCESS-TOKEN`); sidecar auth
optional (forwarded headers); K8s ingress `secureAccess` creds + signed
tokens; proxy strips hop-by-hop + backend Server, preserves origin Date,
rebases root-relative Location only. Unauthenticated mode exists behind
startup guardrails (local-dev posture, not prod).

## Policy lifecycle

`networkPolicy` at create + runtime PATCH; platform overlays as image files;
vault bindings via sidecar API with sanitized readback; scoped-match guard
optional; default-allow deprecated with warning toward default-deny
enforcement. No human-approval chain for actions (policy is network/
credential scope, not capability approvals).

## Storage lifecycle

Volumes `host`/`pvc`/`ossfs` (runtime-neutral shape, provider materializes;
FastSandbox rejects mounts). Snapshots: Docker→local images; K8s→OCI via
CR; template catalog in DB (source of truth) + CRs for builds/status.
Backup/restore commands are roadmap (cf. Utopia same gap).

## Observability

Diagnostics API + plain-text DevOps routes; sandbox state/reason/message;
execd `/metrics`; OTLP; `OPEN-SANDBOX-ORIGIN` header; raw perf report
published (`benchmarks/fast-sandbox-ack-2026-10-08.json`).

# Security analysis

## Enforced technically (doc-verified, code-unread)

- Workload holds no `NET_ADMIN`; only sidecar mutates netns rules; every
  packet passes policy engine (no app cooperation, no bypass from inside).
- Default-deny packet layer with bounded DNS leases + TCP grace (allowed
  never forever); DoT dropped; fail-closed startup (exit, never no-op).
- Vault exact-single-match + path-ambiguity reject + redaction set +
  one-pass substitution (no secret-rewrites-secret) + fail-closed vault
  dependency + in-memory-only (pause wipes — documented, re-inject
  required) + write-only plaintext + TLS/transport guards.
- Trust delivered: sidecar CA bootstrapped into system/NSS/JDK (verify
  stays on, `curl` without `-k`); chained upstream proxy fail-closed (no-
  SNI/ignore/tcp/udp refused, never direct; proxy endpoint UID/IP/port-
  scoped, infra-DNS path bypassing sandbox policy).
- Release signing (Cosign keyless + attestations, digest pinning guide);
  TLS key-length policy aligned to OpenSSF/NIST-2030 with explicit legacy
  opt-ins (Go SDK flag, controller flag).
- Capability/AppArmor/seccomp/PID/secure-runtime knobs; resource limits;
  per-request registry auth; platform overlays user-cannot-override.

## Documented but not source-verified here

- nft/DNS/TLS handler code, FastPath gRPC, operator reconcilers, SDK
  streaming/error mapping, proxy header edge cases, informer reads,
  renew-intent Redis path, OTLP exporters, snapshot commit/push code.

## Assumptions and limitations

- Docs are the evidence; handler code unread — numbers/semantics are
  doc-stated (though unusually honest: saturation + resume-gap + recall
  caveats published with raw data).
- FastSandbox is external platform (adapter in repo, runtime outside);
  `credentialProxy.enabled` + volumes rejected there; no SQL-over-MCP;
  no streaming server events beyond SSE/WS execd; pooled sidecars
  impossible per-request; mesh sidecars + gVisor unsupported by
  construction; OSEP-0023 gate dev-only with 503-on-mutate while on.
- Vault pause-wipe + re-inject discipline is operator burden; default-
  allow compat window still open (warning only); extra MITM ports
  decrypt-but-not-credentialed; SSE >~1 MB RST truncation known.
- No Task/approval/evaluation layer; policy ≠ capability governance.

## Attack surfaces

- Egress MITM CA + custom addons (`OPENSANDBOX_EGRESS_MITMPROXY_SCRIPT`):
  powerful L7 extension, but addon supply chain = policy bypass if
  malicious (system addon first mitigates order, not content).
- Upstream-proxy chaining + extra CA bundle: trust expansion point (scoped
  nft + infra-DNS separation mitigate, rotation ops burden remains).
- Vault bindings mis-scoped (wide paths, overlapping, default-allow):
  credential-destination bypass; scoped-match guard + guidance mitigate,
  not enforce by default.
- Snapshot/OCI images + volumes/OSSFS mounts: exfil/persistence via image
  layers or mounts if registry/volume grants loose (per-request auth
  exists; operator must scope).
- Server proxy + ingress: SSRF/routing risk if endpoint headers mis-
  configured (secureAccess + header rebasing rules mitigate).
- MCP/CLI/API-key theft = sandbox fleet access (revocation/rotation story
  doc-thin here).

# Comparison with our project

## OS-01 — Isolation model

OpenSandbox: netns-sharing sidecar + nft + MITM + microVM options (Kata/
Firecracker) per-sandbox network identity, K8s at fleet scale. Runlith:
worktree isolation + Seatbelt (macOS) + RunnerPort abstraction; Linux
network containment unbuilt, Trusted Host Git path open (G6
REMEDIATION_REQUIRED). Overlap is least-privilege instinct; novelty is
per-sandbox network enforcement Runlith never implemented.

## OS-02 — Secrets delivery

OpenSandbox: fake-in-workload + exact-match inject + substitutions +
redaction + fail-closed, memory-only. Runlith: broker (metadata/backend/
delivery, OS keychain, bindings) designed; delivery into untrusted
execution + prompt-injection exfil story thinner. Closest delivery
reference alongside OpenShell R-02.

## OS-03 — Contracts

OpenSandbox: specs-first, generated+handwritten SDKs, control/data split,
Composite routing by ID prefix. Runlith: Protobuf IPC + ports/adapters
monolith; specs discipline comparable, ID-prefix routing an anti-lesson
(magic prefix vs explicit type).

## OS-04 — Lifecycle

OpenSandbox: create/pause/resume/snapshot/template/pool/task vocabulary
with backend-divergent semantics + published timings. Runlith: Task/
Execution/Attempt/Session durable graph with provider churn survival.
Execution-state durability is Runlith's edge; sandbox-lifecycle vocabulary
is OpenSandbox's.

## OS-05 — Non-overlap (approvals/verification/memory)

No capability approvals, no independent verifier, no bitemporal memory —
Runlith G6-policy/G8/G10 differentiators untouched.

# Bilateral comparison (as in mercury-agent)

## What is better in Runlith

- Durable Task graph + Execution/Attempt/Session vs sandbox lifecycle
  (create/pause/snapshot). Tasks survive provider churn; sandboxes are
  ephemeral compute with snapshots, not commitments.
- Policy Engine (DENY>REQUIRE_APPROVAL>ALLOW, exact-bound single-use,
  fail-closed, child-subset) vs networkPolicy + vault scopes (network/
  credential scope, action-blind). Runlith gates *doings*; OpenSandbox
  gates *packets/requests*.
- Independent Evaluation Port vs SSE stream + exit codes (execution
  evidence, not verification). Runlith separates propose from verdict;
  OpenSandbox streams output and stops.
- Local-first SQLite + journal + worktrees vs server + Docker/K8s +
  external FastPath (fleet-scale, operator burden, backup=DB+volumes+
  registries). Runlith runs on laptop; OpenSandbox needs infra.
- Secret Broker with OS keychain + binding model vs sidecar memory vault
  (correct delivery, pause-wipes, operator re-injects, no stated keychain
  story).
- Hash-chained audit vs state/reason/message + Activity-per-tool (good
  ops trail, no tamper-evidence chain evidenced).

## What is worse in Runlith (gaps OpenSandbox exposes)

- No per-sandbox network enforcement: no netns sidecar, no DNS/nft dual
  layer, no platform overlays, no fail-closed startup. G6 Runner network
  requirements unbuilt.
- No credential-blind delivery: no fake-env + exact-match inject +
  substitution + redaction + fail-closed-vault. Secrets reach execution
  environment more directly.
- No TLS-intercept visibility + chained-proxy containment vocabulary
  (scoped nft, infra-DNS separation, UID-scoped dial).
- No pause/resume/snapshot lifecycle vocabulary with honest timings
  (61 s durable pause, ~63 s SDK resume vs 35 ms runtime-restore gap
  published — Runlith has no equivalent measurement).
- No release-verification checklist (Cosign keyless + attestations +
  digest pinning) cited in runner/image reviews.
- No pool/pre-warm + task-orchestration vocabulary for eval/RL scale
  (Runlith batch story thinner).

## What is better in OpenSandbox

- Fail-closed egress done completely: shared-netns inevitability, dual
  layer with bounded leases, DoT/DoH handling, platform precedence,
  supervisor+breaker, shutdown-drain, quarantine fence with stated
  non-guarantees.
- Vault delivery done precisely: 5 auth types, scoped substitutions with
  per-surface encoding + traversal guards, single-match-wins, write-only
  plaintext, ETag snapshots, transport guards, pause-wipe honesty.
- Contract-first with generated clients + handwritten ergonomics +
  control/data split + Composite routing (prefix lesson noted).
- Perf honesty as documentation genre: saturation 48×429 + projection
  conflict + resume gap + raw JSON + fixed-comparison discipline.
- Secure-defaults-with-escape-hatches posture stated once and applied
  everywhere (auth guardrails, resource limits, Kata-over-gVisor,
  mesh-exclusion, default-deny migration warning).

## What is worse in OpenSandbox

- Scale/ops: fleet K8s + external microVM platform, forward-moving
  snapshots, operator-owned vault re-injection, backup/restore thin.
- Trust substrate: image/registry/volume grants are the real boundary;
  docs say so (good) but enforcement is operator config, not code proof
  here.
- Completeness: FastSandbox path rejects vault-proxy flag + volumes;
  pooled per-request networkPolicy rejected; no SQL-over-MCP; extra
  ports decrypt-only; SSE-RST truncation known.
- Action blindness: no approvals, no verifier, no memory — execution
  substrate, not control plane.
- Doc-code gap: evidence here is docs + specs inventory; handlers unread,
  so every mechanism claim carries doc-stated risk.

# Identified opportunities

- OPP-01 — Fail-closed egress requirements (dual-layer, bounded leases,
  overlays, DoT/DoH, startup exit, drain) for G6 Runner on Linux.
- OPP-02 — Credential-blind delivery (fake-env, exact-match, substitutions
  with encoding guards, redaction, fail-closed vault, pause-wipe discipline)
  for Secrets delivery hardening.
- OPP-03 — Specs-first + control/data split as boundary checklist for
  agentd/agentctl evolution.
- OPP-04 — Lifecycle vocabulary (pause/resume/snapshot/template/pool with
  divergent semantics + honest timings) for Runner UX.
- OPP-05 — Release verification (Cosign + attestations + digest pin) for
  runner image reviews.
- OPP-06 — Measurement honesty (saturation + gap warnings + raw reports)
  for eval/bench pattern.

# Recommendations

## R-01 — Adapt fail-closed egress as Runner network requirements (P1)

- Proposed change: Linux runner work items carry default-deny outbound,
  DNS+nft-equivalent enforcement, bounded learned-address leases,
  platform overlays, fail-closed startup, drain-on-shutdown; no-NET_ADMIN
  in workload.
- Problem: OPP-01; G6 REMEDIATION_REQUIRED (network path).
- Approach: requirements first (no implementation in this file); spike
  behind Runner port on Linux; document DoT/DoH + mesh/gVisor-equivalent
  boundaries for macOS/Runner backends.
- Benefits: exfil-resistant execution; "no enforcement never looks like
  enforcement".
- Security: filters (INV-008) extend to network + DNS; overlays preserve
  operator floor.
- Complexity: High (owned by G6 Runner).
- Risks: Linux-only mechanisms don't transfer to Seatbelt; perf/caps cost;
  supervisor/restart semantics new to agentd.
- Acceptance: enforcement-absent startup test exits (never pass-through);
  bypass-without-NET_ADMIN attempt fails; overlay-deny beats user-allow.
- Priority: P1 within G6 Runner network scope.
- Classification: `NOVEL_GAP` → `ADAPT`.

## R-02 — Adapt credential-blind delivery as Secrets hardening (P1)

- Proposed change: secrets reach untrusted execution only as placeholders;
  exact single-binding match injects at boundary; substitutions with
  per-surface encoding + traversal guards; redaction set; vault failure
  fails closed; memory-only with explicit re-inject-after-restore.
- Problem: OPP-02; complements OpenShell R-02 (binding) with delivery.
- Approach: requirements + spike behind Secrets delivery; scoped-match
  guard default-on (methods+paths, no host-wide `/*`); write-only
  plaintext reads; transport guards (TLS/loopback/proxy-CIDR).
- Benefits: prompt-injection/untrusted-code exfil surface shrinks;
  existing tool CLIs keep workflows (fake env pattern).
- Complexity: Medium-High.
- Risks: pause/resume wipe burden; streaming-body 403-vs-drop edge;
  extra-port decrypt-only limits.
- Acceptance: workload-env-dump shows no secret; mismatched-path request
  gets no injection; vault-outage request fails closed; re-inject-after-
  restore test.
- Priority: P1 within Secrets delivery.
- Classification: `NEEDS_ADR` → `ADAPT`.

## R-03 — Cite specs-first + control/data split as boundary checklist (P2)

- Proposed change: treat specs-as-truth + generated/handwritten split +
  thin-routes/thick-services + provider-behind-interface as the checklist
  for agentd/agentctl changes; note ID-prefix routing as anti-pattern
  (prefer explicit type field).
- Problem: OPP-03.
- Approach: notes only; cite in reviews.
- Benefits: stable contracts, swappable runtimes.
- Complexity: Low.
- Acceptance: review checklist references.
- Priority: P2.
- Classification: `ALREADY_DESIGNED` → `EXTERNAL_VALIDATION`.

## R-04 — Borrow lifecycle vocabulary + honest-measurement pattern (P3)

- Proposed change: pause/resume/snapshot/template/pool naming with
  divergent-semantics footnotes; perf reports carry saturation + gap
  warnings + raw data + fixed-comparison discipline.
- Problem: OPP-04, OPP-06.
- Approach: notes only.
- Benefits: operator-honest UX; non-benchmarketing numbers.
- Complexity: Low.
- Acceptance: notes cited in Runner UX/eval docs.
- Priority: P3.
- Classification: `ALREADY_DESIGNED` → `INSPIRED_ONLY`.

## R-05 — Cite release verification for runner images (P2)

- Proposed change: production runner images pinned by digest + Cosign
  keyless + attestations verified against Actions identity before deploy.
- Problem: OPP-05.
- Approach: checklist in runner/image reviews.
- Benefits: supply-chain posture without new code.
- Complexity: Low.
- Acceptance: review cites verification guide triple.
- Priority: P2.
- Classification: `ALREADY_IMPLEMENTED` (image discipline) →
  `EXTERNAL_VALIDATION`.

# Rejected or inapplicable ideas

- **Adopt OpenSandbox server/sidecar/operator as Runlith runner.**
  Reason: fleet Python/FastAPI + Go + K8s + external microVM platform
  conflicts with local-first Rust monolith + SQLite + offline posture;
  supply-chain + authority split unjustified for v1. (REJECT as stack.)
- **Copy specs/handlers into Runlith.** Reason: §16.1 prohibits external
  code import; language + ops mismatch stands alone. Requirements only.
  (REJECT.)
- **Add OpenSandbox as dependency or sidecar.** Reason: reviewed
  competitor as implementation shortcut prohibited (§16.1). (REJECT.)
- **Treat networkPolicy/vault as capability approvals.** Reason: category
  error — network/credential scope ≠ action authority; G6 Policy Engine
  semantics (single-use, child-subset, DENY-first) have no counterpart
  here. (REJECT.)
- **Serve production DBs / broad tool APIs over agent MCP.** Reason: even
  OpenSandbox withholds SQL-over-MCP and scoped vault writes; Runlith has
  no production-data-via-agent story either. (REJECT for v1.)
- **Pool/pre-warm + RL orchestration as Runlith scheduler.** Reason: fleet
  eval scale ≠ local durable scheduling; no parity merely for existence
  (§16.2). (REJECT for v1.)

# Proposed implementation plan

Independent, ordered by leverage/cost (no implementation authorized by this
research file):

1. R-01 egress requirements (with G6 Runner network scope).
2. R-02 credential-blind delivery (with Secrets delivery).
3. R-03 boundary checklist (cite in agentd/agentctl reviews).
4. R-05 release verification (cite in image reviews).
5. R-04 vocabulary + measurement notes (Runner UX/eval docs).

Each step needs network-bypass tests, secret-exfil keepers, provenance/
audit assertions, and docs updates. No step changes public versioning or
cross-context storage reads.

# Evidence and references

Directly verified (raw fetch at pinned SHA
`fdb06ac7d392a1d8570e563857d796bacd7250c4`):

- https://github.com/opensandbox-group/OpenSandbox (landing: 15.8K stars /
  1.5K forks / 60 watchers snapshot, Apache-2.0, 5 topics).
- GitHub Commits API `main` →
  `fdb06ac7d392a1d8570e563857d796bacd7250c4`, 2026-10-10T09:37:39Z,
  `fix(egress): verify quarantine across older nft readback formats`.
- https://raw.githubusercontent.com/opensandbox-group/OpenSandbox/fdb06ac7d392a1d8570e563857d796bacd7250c4/docs/architecture/index.md
  (3 paths, 6 surfaces, Docker/K8s/FastSandbox, execd/egress/ingress,
  Composite `fsb-` routing, store SQLite/Postgres).
- https://raw.githubusercontent.com/opensandbox-group/OpenSandbox/fdb06ac7d392a1d8570e563857d796bacd7250c4/docs/guides/credential-vault.md
  (5 auth types, substitutions matrix, ETag/304/404 socket discipline,
  in-memory warning, Claude/Git/curl examples, binding guidance,
  ports-deprecation table).
- https://raw.githubusercontent.com/opensandbox-group/OpenSandbox/fdb06ac7d392a1d8570e563857d796bacd7250c4/docs/architecture/network/egress.md
  (DNS+nft, precedence, fail-closed, MITM scope, OSEP-0023 gate,
  quarantine fence, mesh/gVisor exclusions, shutdown, OTLP).
- https://raw.githubusercontent.com/opensandbox-group/OpenSandbox/fdb06ac7d392a1d8570e563857d796bacd7250c4/docs/architecture/fast-sandbox/performance.md
  (ACK 2026-10-08: 64.0 ms p50 serial / 125.4 ms p99 c10 / c20 51/100 +
  48×429, echo ~203 ms = 200 ms grace, snapshot 60.6 s, resume SDK ~63 s
  vs runtime ~35 ms gap, raw JSON report).
- https://raw.githubusercontent.com/opensandbox-group/OpenSandbox/fdb06ac7d392a1d8570e563857d796bacd7250c4/SECURITY.md
  (private advisories, 48h ack, latest+main support, Cosign/Sigstore/
  Maven signatures, TLS key-length policy with legacy opt-ins).
- GitHub Contents API `specs/` at pinned SHA (lifecycle 89 KB, execd
  117 KB, egress 22 KB, diagnostic 13 KB + tests).

Inferred (not source-verified): handler internals, FastPath gRPC, operator
reconcilers, proxy edge cases, informer/renew paths, snapshot commit code.
Marked as such in §Security.

Runlith references: `AGENT.md` §§4/7/16.1/16.2/20, `ARCHITECTURE.md`,
`docs/architecture/INVARIANTS.md` (INV-008/022/029/115),
`docs/architecture/adr/ADR-0010-context-derived-state.md`,
`docs/architecture/adr/ADR-0011-provider-independent-context-continuity.md`,
`docs/architecture/adr/ADR-0012-structured-memory-provenance.md`,
`planning/roadmap.yaml` (G6 REMEDIATION_REQUIRED, G9/G10 deferred),
`planning/tasks/queue.yaml`, `crates/AGENTS.md`, OpenShell/Utopia reports
(R-02 binding, R-01/R-02 time/proposal references).

# Final assessment

OpenSandbox is the strongest G6-Runner/Secrets reference reviewed so far:
fail-closed per-sandbox egress with dual-layer enforcement and platform
overlays, credential-blind delivery with exact-match injection and honest
pause-wipe/fail-closed semantics, contract-first specs with generated
clients, and perf documentation that publishes saturation and resume gaps
instead of hiding them. Its value to Runlith is requirements + checklists
(R-01/R-02) plus boundary/vocabulary/measurement notes (R-03…R-05), not
stack or code. Its fleet model (FastAPI + K8s + external microVMs),
operator-owned vault discipline, and action blindness (no approvals,
verifier, or memory) are inapplicable to Runlith v1's local-first,
policy-gated, provider-neutral monolith.

Recommended next steps: keep this file as evidence; fold R-01/R-02 into G6
Runner/Secrets scope (requirements, then Linux spike); cite R-03/R-05 in
agentd/agentctl and image reviews; keep R-04 as Runner UX/eval notes. Do
not import OpenSandbox code or add it as a dependency.

## Novelty / adoption summary

| Finding | Novelty | Adoption |
|---|---|---|
| Fail-closed egress (DNS/nft, leases, overlays, DoT/DoH) | `NOVEL_GAP` (no net enforcement) | `ADAPT` (R-01) |
| Credential-blind delivery (placeholders, exact-match, redaction) | `NEEDS_ADR` | `ADAPT` (R-02) |
| Specs-first + control/data split | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` (R-03) |
| Lifecycle vocabulary + honest measurement | `ALREADY_DESIGNED` | `INSPIRED_ONLY` (R-04) |
| Release verification (Cosign + attestations + digest) | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` (R-05) |
| Server/sidecar/operator stack as runner | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |
| Specs/handlers as code/dependency, policy-as-approvals | `CONFLICTS_WITH_ARCHITECTURE` or out-of-scope | `REJECT` |
