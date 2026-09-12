# Threat Model

> Architecture-level threat model for AI Engineering OS. Documents accepted
> risk and mitigation; designs no mechanisms and gives no exploit instructions.
>
> - Boundary definitions: [`TRUST_BOUNDARIES.md`](./TRUST_BOUNDARIES.md)
> - Sensitivity rules: [`DATA_CLASSIFICATION.md`](./DATA_CLASSIFICATION.md)
> - Decisions: [`../architecture/adr/INDEX.md`](../architecture/adr/INDEX.md)

Core principle: prompt restrictions ≠ security boundary. Protection comes
from Policy Engine decisions (ALLOW / DENY / REQUIRE_APPROVAL) **plus** real
runtime/OS/container/VM isolation. AI/model output is untrusted data and never
gains policy or architecture authority, however capable the model.

## 1. What we protect

Protected assets (sensitivity per [`DATA_CLASSIFICATION.md`](./DATA_CLASSIFICATION.md)):

- source repositories, uncommitted code and isolated worktrees;
- Task and Execution state, acceptance criteria, architecture/ADR/policy state;
- model context, Memory, resource data, imported documents, agent transcripts;
- tool outputs and artifacts, Event Journal, Security Audit evidence, tool
  execution receipts;
- Git credentials, provider credentials/auth sessions, API keys, OS
  credentials, secret references;
- user configuration, provider usage/quota information;
- private network resources (link-local, LAN, cloud metadata endpoints);
- the local filesystem outside the authorized workspace.

## 2. Threat actors and untrusted sources

No single malicious-human model. Relevant hostile or fallible sources:

- malicious repository content; prompt injection in source/README/docs;
- malicious or compromised dependencies (code, build scripts, metadata);
- malicious webpages; malicious PDF/DOCX/EPUB/imported resources;
- malicious or unexpected tool output; hallucinating or compromised
  provider/model; compromised provider CLI or binary update;
- malicious plugins/integrations; supply-chain compromise of any adapter;
- malicious remote worker (future); local unprivileged process probing IPC;
- network attacker on paths the app legitimately uses;
- user mistake — accidental destructive action, over-broad approval.

## 3. Threat register

Qualitative likelihood/impact labels are triage aids only, not policy. Status
is one of: `Mitigated by architecture` (control exists in accepted design),
`Partially mitigated`, `Accepted` (known, deferred by decision), `Open`
(requires a future design item).

| ID | Boundary / asset | Threat | Accepted architectural mitigation | Residual / open risk | Status | Refs |
|----|------------------|--------|-----------------------------------|----------------------|--------|------|
| THR-001 | TB5 / repo content | Malicious repository prompt injection hijacks agent behavior | Repo text never instruction; only task/invariant/ADR layers instruct; security filter pre-provider | Filter bypass via novel encoding | Partially mitigated | INV-038, INV-055 |
| THR-002 | TB1 / host | Frontend/WebView privilege escalation to host commands | No frontend shell; strict capabilities/CSP; isolated browser WebView | Compromised dependency below review | Partially mitigated | ADR-0007; INV-003 |
| THR-003 | TB2 / IPC | Unauthorized local IPC caller invokes privileged ops | User-private socket dir; versioned typed protocol; least-privilege commands; OS-user peer authentication per ADR-0020 (same UID/SID, fail closed) | Mechanism implemented (TASK-0026): cross-user local IPC mitigated by OS peer authentication + endpoint ACL/permissions; same-user malicious process explicitly outside this peer-auth guarantee | Mitigated by architecture | ADR-0020; INV-026 |
| THR-004 | TB3 / CLI | Provider CLI compromise or misbehavior | Anti-corruption adapters; contract suites; FakeAgent-first testing | Malicious binary with valid interface | Partially mitigated | ADR-0009; INV-093 |
| THR-005 | TB4 / filesystem | Agent filesystem escape from worktree | Isolated worktrees; runner sandbox; policy paths; fail-closed isolation | Backend-specific escape bugs | Partially mitigated | ADR-0008/0013; INV-005, INV-076 |
| THR-006 | TB4+TB6 / secrets | Secret exfiltration via tool output or transfer | Secrets never in context/logs/events; injection at execution boundary only; OS stores | Side-channel via crafted outputs | Partially mitigated | INV-008 |
| THR-007 | TB6 / network | SSRF and private-network access | Centralized per-hop SSRF/policy evaluation; scoped network capability | New bypass primitives | Partially mitigated | INV-097, INV-101 |
| THR-008 | TB6 / context | Malicious web prompt injection via fetched content | Fetched content stays untrusted after cleaning; provenance retained | Cleaning never confers trust — handled as untrusted always | Mitigated by architecture | INV-098 |
| THR-009 | Document zone / host | Malicious document or converter exploit | Sandboxed worker; bounded resources; argv-array invocation; owned temp dirs | Zero-day in native converter | Partially mitigated | INV-119…122 |
| THR-010 | Browser zone / creds | Unsafe browser capability sharing | No shared privileged IPC/fs/secrets; scoped session credentials | Session-cookie theft within zone | Partially mitigated | INV-099 |
| THR-011 | Security zone / target | Security-runner target escape or overreach | Target binding; typed Rules of Engagement; bounded creds/net/rate; kill switch | Scope misconfiguration by operator | Partially mitigated | INV-132…134, INV-138 |
| THR-012 | TB7 / data | Plugin/cloud overreach and silent export | Opt-in; explicit scopes; local-first core; telemetry off; auditable flows | Malicious update widening scope | Partially mitigated | ADR-0014; INV-011 |
| THR-013 | Supply chain / build | Dependency compromise (npm/cargo/go, skills, adapters) | Scanning/SAST/monitoring; AGPL review gate; adapter replaceability | Detection lag | Accepted | INV-036 |
| THR-014 | TB3–TB4 / evidence | Fabricated agent/tool success claim | Runtime-authenticated receipts prove execution only; evaluation requires evidence | Receipt-key compromise (see open items) | Partially mitigated | INV-072, INV-092 |
| THR-015 | Logs / transcripts | Accidental secret retention in logs or transcripts | Category separation (ops/audit/transcript); secrets excluded by construction | Human pastes secret into chat | Partially mitigated | INV-008 |
| THR-016 | TB4 / availability | Sandbox unavailable while task requires isolation | Fail closed: BLOCKED / REQUIRE USER ACTION, never silent fallback | User overrides block under pressure | Mitigated by architecture | INV-076 |
| THR-017 | Routing / execution | Stateful provider failover replays side effects | New/recovered attempt per failover; no transparent replay | Adapter bug replaying effects | Partially mitigated | INV-066 |
| THR-018 | Policy / delegation | Confused deputy uses a more-privileged parent/runtime to perform an operation outside the requester's authority | Typed requested/effective capabilities; child authority subset; operation-bound enforcement | Incorrect scope modeling | Partially mitigated | ADR-0026; INV-147, INV-153 |
| THR-019 | Policy / child execution | Child agent/process gains a new capability or widens filesystem/network/secret scope | Monotonic capability narrowing; widening is a typed refusal | Future runner implementation defect | Partially mitigated | ADR-0026; INV-147 |
| THR-020 | Approval | Stale approval is replayed for another operation, policy revision or wider path/resource | Approval binds exact request/scope/provenance/policy fingerprint/expiry | Persistent approval-store bugs | Partially mitigated | ADR-0026; INV-150 |
| THR-021 | Filesystem | Path traversal or symlink/race escapes a managed workspace | Typed relative scope rejects traversal; future enforcement canonicalizes/revalidates adjacent to operation | OS/filesystem TOCTOU | Open | ADR-0026; INV-149, INV-153 |
| THR-022 | Repository/process | Malicious Git hook, test/build script or project executable escalates from repository code to ambient host authority | Repository remains untrusted; process capability + runner isolation + controlled environment | Backend escape or over-broad process scope | Open | ADR-0013, ADR-0026; INV-013, INV-146 |
| THR-023 | Provider process / credentials | Provider subprocess inherits HOME/XDG/environment secrets or writes provider state into the managed worktree (H7) | Sanitized provider environment; scoped HOME/XDG/state/scratch outside worktree; filesystem policy | Provider behavior/version changes | Open | ADR-0026; G5 final audit H7; TASK-0065 |
| THR-024 | Secrets / network | Environment-variable or credential leakage enables network exfiltration | Secrets Broker releases scoped material only after policy; broad inheritance forbidden; network separate capability | Side channels inside approved tool | Open | ADR-0026; INV-151 |
| THR-025 | Audit | Provider/model tampers with or fabricates security logs to hide a denied/elevated action | Product-authored Security Audit separate from provider output/Event Journal; append-oriented persistence planned | Local host compromise | Open | ADR-0026; INV-152; TASK-0066 |
| THR-026 | Policy/enforcement | State changes between policy check and operation (TOCTOU), invalidating approved scope | Preflight is not authority; enforcement revalidates/couples restriction at operation boundary | Backend-specific atomicity limits | Open | ADR-0026; INV-153 |
| THR-027 | Plugins/tools/external services | Tool/plugin request is treated as self-authorizing or provider-native permission silently widens product policy | Canonical PolicyDecision before invocation; provider-native controls defense in depth only | New extension point bypass | Partially mitigated | ADR-0026; INV-146, INV-148 |

## 4. Tool execution evidence

A Tool Execution Receipt proves the trusted runtime observed an
execution/result. It does **not** prove authorization, correctness, safety or
task completion. Durable security/audit evidence is a separate record from
model-visible session receipt mechanics. No cryptographic specifics beyond
this accepted split are defined here.

## 5. Security assumptions we refuse to make

- "the model will follow the prompt" (INV-013);
- "localhost means trusted" (TB2);
- "repository files are safe" (INV-038);
- "provider output is correct" (INV-093);
- "tool success text proves execution" (INV-072, INV-092);
- "sanitized Markdown is trusted" (INV-098);
- "native runner is a sandbox" (Trusted Host, not isolation);
- "the browser can share app privileges safely" (INV-099);
- "failed exploit attempt means no vulnerability" (INV-131);
- "schema-valid extraction means factual truth" (INV-126);
- "cloud is required to secure local state" (INV-011).

## 6. Open Security Design Items

Unresolved by accepted architecture; listed, not decided:

- device signing-key lifecycle for receipts and durable audit evidence;
- sandbox backend matrix per OS (which backends satisfy which profiles);
- secure remote-worker enrollment and transport;
- plugin permission-model details;
- data-at-rest encryption policy;
- audit retention policy.

G6 authorization semantics, capability monotonicity, approval binding and
security-audit authority are no longer open design items; ADR-0026 and
`POLICY_CONTRACT.md` define their foundation. Their persistence/enforcement
implementations remain open by the G6 task sequence.

## 7. Accepted and deferred risks

Accepted: detection lag on supply-chain compromise (mitigated by scanning
discipline, not eliminated). Deferred to later tasks by explicit scope:
privacy retention/legal detail (TASK-0005 Privacy Architecture), full
data-flow matrix, telemetry schema, security test plan.
