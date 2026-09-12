# Trust Boundaries

> Canonical trust-boundary definitions. IDs TB1–TB7 are stable and exactly as
> accepted in the Architecture Source of Truth §53.
>
> - Threat scenarios per boundary: [`THREAT_MODEL.md`](./THREAT_MODEL.md)
> - Data sensitivity rules: [`DATA_CLASSIFICATION.md`](./DATA_CLASSIFICATION.md)
> - Decisions: [`../architecture/adr/INDEX.md`](../architecture/adr/INDEX.md)

Core principle: prompt restrictions ≠ security boundary. Every privileged
action passes the Policy Engine (ALLOW / DENY / REQUIRE_APPROVAL) **and**
executes under real runtime/OS/container/VM isolation. Neither layer replaces
the other.

## G6 request / decision / enforcement chain

The TB IDs below remain stable. G6 additionally assigns authority roles across
the internal execution path:

```text
User/UI              request only; never authorization authority
  ↓ TB1
Tauri host           validate/forward; never widen authority
  ↓ TB2
agentd/core          canonical product trust boundary
  ↓
orchestration        constructs PolicyRequest from product state
  ↓
Policy               deterministic ALLOW | DENY | REQUIRE_APPROVAL
  ↓
workspace/runtime    consumes effective capabilities
  ↓ TB3
provider adapter     translates restrictions; cannot broaden them
  ↓
provider process     untrusted/partially trusted executor
  ↓ TB4/TB5/TB6
filesystem/process/Git/network/tool/secret operation
```

The final privileged operation owner enforces the decided scope. A UI dialog,
policy preflight or provider-native permission prompt does not itself enforce
the operation. Security Audit records canonical product decisions/results at
the trusted boundaries; provider text is contextual evidence only.

## TB1 — WebView → Tauri Host

- From: application WebView (React UI). To: thin Tauri host.
- Why a boundary: rendered UI processes untrusted content (Markdown, repo
  text, web-derived data); a compromised renderer or dependency must not
  become host compromise.
- Data crossing: validated DTO requests; rendered views, dialogs,
  notifications back.
- Primary threats: XSS, malicious rendered Markdown, compromised frontend
  dependency, remote content execution, IPC abuse, parameter tampering,
  privilege escalation.
- Required controls: strict Tauri capabilities; request validation; strict
  CSP; no remote JS in the main application UI; sanitized untrusted display
  content; no arbitrary frontend shell capability; browser WebView isolated
  from the privileged app WebView with a strictly smaller capability set.
- Forbidden assumptions: "the UI can be trusted because we wrote it";
  "sanitized Markdown is trusted instruction".
- Evidence/audit: rejected/validation-failure events observable; privileged
  UI-invoked actions audited at TB2/TB4 layers.
- Refs: ADR-0007; INV-003, INV-094.

## TB2 — Tauri Host → agentd

- From: thin Tauri host. To: `agentd` over local IPC.
- Why a boundary: IPC is a privilege boundary — `local == trusted` is
  explicitly rejected. Any local process that can reach the socket/pipe could
  invoke privileged operations.
- Data crossing: typed/versioned IPC requests and responses.
- Primary threats: unauthorized local client, message spoofing, stale
  socket/pipe, protocol confusion, version mismatch, malformed payload,
  request replay, privileged-operation invocation.
- Required controls: typed/versioned protocol; peer/access validation
  appropriate to the OS transport; message validation; protocol negotiation;
  least-privilege commands; audit correlation where needed. Socket/pipe lives
  in a user-private runtime directory.
- Forbidden assumptions: "localhost means trusted"; "a well-formed message
  means an authorized caller".
- Evidence/audit: malformed/unauthorized IPC attempts observable; privileged
  invocations correlated in the Security Audit.
- Refs: ADR-0003, ADR-0020; INV-026.
- Peer authentication (accepted, implementation in TASK-0026): the
  authenticated principal is the OS user running `agentd`; a client is
  accepted only when kernel/token-backed peer credentials report the same
  local user (Unix effective UID via `SO_PEERCRED`/`getpeereid`
  equivalents; Windows client access-token SID). `client_kind` stays
  descriptive, never authentication. Same-user malicious code is
  explicitly outside this boundary; cross-user requests fail closed with
  no handshake, no mutation, no journal event.

## TB3 — agentd → Agent CLI

- From: `agentd` / Agent Runtime. To: external provider CLIs (Claude Code,
  Codex, OpenCode, future providers).
- Why a boundary: CLIs are replaceable, versioned externally and potentially
  fallible or compromised; they are executors, never authorities.
- Data crossing: canonical execution requests; provider outputs (treated as
  untrusted data); process lifecycle signals.
- Primary threats: unexpected CLI behavior, provider update/change, malicious
  or compromised binary, unexpected filesystem access, credential leakage,
  incorrect output, hallucinated execution claims, protocol/version changes,
  process escape from the intended workspace.
- Required controls: provider adapters as anti-corruption layers (no Task
  mutation, no memory writes, no merges, no policy bypass); contract-tested
  ports; FakeAgent precedence over real providers in testing; execution
  receipts that prove runtime-observed execution only.
- Forbidden assumptions: "provider output is correct"; "tool success text
  proves execution"; "the CLI's login lets us read its credentials" —
  provider-native authentication stays provider-owned; the app never scrapes
  CLI credentials.
- Evidence/audit: execution receipts (provenance, not authorization);
  adapter contract-suite results.
- Refs: ADR-0009; INV-004, INV-012, INV-066, INV-072, INV-093.

## TB4 — Agent CLI → OS

- From: provider CLI process. To: operating system (filesystem, processes,
  network, credentials).
- Why a boundary: highest-risk boundary — model-directed code execution meets
  ambient host capabilities (arbitrary commands, exfiltration, persistence).
- Data crossing: authorized workspace file/process operations; tool results
  back (untrusted).
- Primary threats: arbitrary command execution, filesystem/symlink/path
  escape, secret discovery, SSH/cloud credential access, network
  exfiltration, sudo/privilege escalation, destructive commands, process
  spawning, persistence, access outside the worktree.
- Required controls: Runner/Sandbox boundary; Policy Engine decisions before
  execution; workspace restrictions (isolated worktrees, never main/master);
  network restrictions where required; approval gates; Security Audit.
  Required isolation fails closed: backend unavailable → BLOCKED / REQUIRE
  USER ACTION, never silent unsandboxed fallback.
- Forbidden assumptions: "NativeRunner is a sandbox" (it is Trusted Host,
  honestly labeled); "the task is low-risk so isolation can slide".
- Evidence/audit: policy decisions, approvals and results in the Security
  Audit; tool execution receipts for provenance.
- Refs: ADR-0008, ADR-0013; INV-005, INV-006, INV-007, INV-076, INV-077.

## TB5 — Agent → Repository content

- From: agent execution. To: repository files, build scripts, configs,
  dependency metadata, generated code.
- Why a boundary: repository text ≠ trusted instruction. READMEs, configs and
  code may carry prompt injection aimed at hijacking the agent.
- Data crossing: file reads (evidence/data); file writes (constrained to the
  isolated worktree).
- Primary threats: prompt injection, malicious AGENT-like instructions in
  ordinary files, malicious build scripts, poisoned tests, malicious configs,
  path tricks, generated code, dependency metadata attacks.
- Required controls: only authoritative project/task/invariant/ADR layers give
  instructions; repository content is evidence/data; Context Security Filter
  before the provider boundary; agent annotations stay untrusted until
  validated.
- Forbidden assumptions: "repository files are safe"; "tests/configs are
  just data so they cannot instruct".
- Evidence/audit: provenance retained on retrieved items; policy-relevant
  violations auditable.
- Refs: INV-029, INV-038, INV-055.

## TB6 — Agent → Internet/provider

- From: agent execution. To: provider inference endpoints and general
  network/web destinations. Two separate concerns: provider inference vs
  general network access.
- Why a boundary: every byte leaving the device crosses privacy, SSRF and
  spend authority at once; redirects and page content are attacker-controlled.
- Data crossing: context packages to providers; fetched pages/snapshots back
  (untrusted even after cleaning).
- Primary threats: source/secret leakage, unexpected provider transfer, SSRF,
  private-network and cloud-metadata access, malicious redirects, malicious
  webpage prompt injection, data-residency/privacy mismatch, unbounded
  spend/rate.
- Required controls: Context Security Filter; Network Policy with centralized
  SSRF evaluation of every hop; explicit provider data-flow observability;
  budget/quota policy; web content stays untrusted after cleaning.
- Forbidden assumptions: "cleaned Markdown is trusted"; "provider transfer
  implies our cloud received it" (local CLI transfer ≠ our servers).
- Evidence/audit: data-flow inspector records (what left the device, to
  whom); fetch snapshots with provenance/freshness.
- Refs: ADR-0014; INV-008, INV-097, INV-098, INV-101.
- Note: anti-bot/stealth is an optional adapter capability under policy,
  never a core security requirement (INV-102).

## TB7 — App → Plugins/cloud services

- From: application core. To: plugins, integrations and (opt-in) cloud
  services.
- Why a boundary: core works without any of them; each integration is an
  independent compromise and over-sharing risk.
- Data crossing: explicitly scoped capability invocations; explicitly
  user-approved sync payloads only.
- Primary threats: excessive scopes, data over-sharing, plugin compromise,
  supply-chain compromise, cloud account compromise, silent telemetry/data
  export, credential leakage, provider confusion.
- Required controls: opt-in cloud; explicit capabilities/scopes; local core
  functionality without cloud; auditable data flows; replaceable
  integrations; telemetry off by default.
- Forbidden assumptions: "cloud is required to secure local state";
  "an installed plugin shares our trust level".
- Evidence/audit: integration data flows auditable; sync payloads inspectable
  before leaving the device.
- Refs: ADR-0014; INV-011, INV-014, INV-058.

## Specialized execution zones (descriptive, no new TB IDs)

### Browser Worker zone

- Why separate: web content is hostile and browser processes blend remote code
  with local capability. Must not share: Tauri privileged IPC, arbitrary
  filesystem, repository secrets, SSH credentials, other project files, OS
  credential store, agentd internal APIs.
- Credentials scoped per session; cookies/auth never inherit host secrets.
- Failure looks like: bounded retry exhaustion → degraded/partial marked
  result; policy denial → stop, never silent bypass.

### Document Worker zone

- Why separate: imported documents are hostile input; parsers and native
  converters are exploit targets.
- Sandboxed converter worker with bounded CPU/RAM/disk/time; no ambient
  secrets/home/repository access; fixed executable + argument-array
  invocation (never interpolate user-controlled metadata into shell
  commands); application-owned temporary directories; URL fetching only
  through Web Acquisition / SSRF policy.
- Failure looks like: explicit failed/partial/degraded unit state, never
  silent omission.

### Security Runner zone

- Why separate: security testing is itself dangerous to the target and the host.
- Production exploitation denied by default; authorized target binding; typed
  Rules of Engagement; dedicated test credentials; rate limits; network
  scope; destructive-action policy; kill/stop capability; durable evidence.
  A scanner crash is INCOMPLETE, not CLEAN.

### Container Runner zone

- Why separate: primary isolation layer for autonomous work (rootless
  preferred). Mounts `/workspace` read-write; never mounts `~/.ssh`,
  `~/.aws`, `~/Documents` or `/`. Network is a separate capability from
  filesystem isolation — a file-restricting backend must not claim network
  enforcement.

### Future Remote Worker zone

- Why separate (future): device-identity-bound outbound execution outside the
  local trust perimeter. Enrollment, transport and evidence rules are not yet
  specified — see OPEN items in [`THREAT_MODEL.md`](./THREAT_MODEL.md).
