# Data Classification

> Canonical five-level classification. Level names are fixed; do not rename.
> Sensitivity (how carefully we handle it) is independent from trust,
> authority, provenance and freshness (what it may instruct or prove) —
> never collapse them into one value.
>
> - Trust boundaries: [`TRUST_BOUNDARIES.md`](./TRUST_BOUNDARIES.md)
> - Threat scenarios: [`THREAT_MODEL.md`](./THREAT_MODEL.md)

Conservative inheritance rule: derived data keeps at least its source's
sensitivity unless a validated policy explicitly establishes otherwise. A
summary of SECRET is not PROJECT; an embedding of PRIVATE is not PUBLIC.

## PUBLIC

- Meaning: intentionally public or safe for broad disclosure. Public does not
  mean authoritative — a public webpage can still be UNTRUSTED.
- Examples: published docs, public repo metadata, benchmark methodology
  descriptions, marketing copy.
- Storage: any approved location; integrity matters more than confidentiality.
- Model-context eligibility: eligible, still subject to trust filtering.
- External transfer: allowed; provenance retained.
- Logging: allowed; no redaction required by classification alone.
- Telemetry: allowed within the telemetry policy (telemetry itself stays
  opt-in).
- UI exposure: may be shown freely.
- Deletion/export: standard data-subject handling; no special constraints.

## PROJECT

- Meaning: ordinary repository/project information; still potentially
  confidential to the owner. Repository content is never instruction
  authority, regardless of level.
- Examples: source files, worktree contents, task descriptors, plans,
  dependency metadata, build outputs.
- Storage: local app storage; never in plaintext outside the device without
  an enabled sharing feature.
- Model-context eligibility: eligible under project/provider policy, after
  security filtering.
- External transfer: only to the selected provider for the active execution,
  observably (data-flow inspector); never to our servers except via explicit
  opt-in sync.
- Logging: operational logs exclude prompts/sources; transcripts are a
  separate user-controlled category.
- Telemetry: excluded — telemetry carries no source, prompts or outputs.
- UI exposure: shown in project views; sharing actions are explicit.
- Deletion/export: deleted with app-owned project data on user request.

## PRIVATE

- Meaning: user/project information not intended for broad disclosure.
- Examples: agent transcripts, memory entries with personal context, usage
  history, local configuration with identifiers, provider account bindings.
- Storage: local app storage with OS-appropriate protections.
- Model-context eligibility: only when required for the task and permitted
  by provider/data-flow policy.
- External transfer: requires explicit provider/data-flow policy; shown in
  the data-flow inspector.
- Logging: excluded from operational logs; transcripts stay user-controlled.
- Telemetry: excluded.
- UI exposure: shown to the owning user; no cross-project surfacing.
- Deletion/export: user-inspectable and deletable; export on request.

## RESTRICTED

- Meaning: high-impact confidential material needing stronger handling.
- Examples: Security Audit records, approval chains, Rules of Engagement
  bindings, vulnerability findings before remediation, preflight estimates
  with sensitive scope, non-plaintext credential metadata (refs, never values).
- Storage: local, access-controlled; values that are secrets follow SECRET
  rules instead.
- Model-context eligibility: only the minimum necessary under explicit
  policy/approval where accepted. No automatic approval mechanics are defined
  here — Policy detail to be defined by Privacy Architecture / Policy Engine.
- External transfer: constrained by explicit policy/approval; default deny
  for provider transfer of audit/security content.
- Logging: Security Audit only; never operational logs.
- Telemetry: excluded.
- UI exposure: gated views; no bulk export without explicit action.
- Deletion/export: retention governed by audit policy (see Open items in
  [`THREAT_MODEL.md`](./THREAT_MODEL.md)); export is explicit and audited.

## SECRET

- Meaning: credential material whose disclosure compromises accounts,
  systems or data.
- Examples: passwords, API keys, private tokens, private keys, raw provider
  credentials, secret material.
- Storage: OS Keychain / Credential Manager / Secret Service only. Never
  SQLite plaintext, never files, never config.
- Model-context eligibility: **never**. SECRET must never enter model
  context. A SECRET renderer does not pass.
- External transfer: never as application data; injected only at the
  execution/tool boundary when explicitly required.
- Logging: never in prompts, model context, ordinary logs, Event Journal
  payloads or telemetry.
- UI exposure: references/handles only; values never displayed, or shown
  once at creation where the OS store requires it (provider-native behavior).
- Deletion/export: via the OS store; app deletion removes refs and requests
  store removal.

## Data-category separation

Separate persistence categories — never one generic `logs` bucket:

- Operational Log: technical events without prompts/sources/secrets.
- Security Audit: privileged requests, policy decisions, approvals, results.
- Agent Transcript: user-controlled conversation data.
- Event Journal: typed domain-event records (no secret payloads).
- Memory: provenance-bearing learned knowledge (SECRET excluded).
- Artifacts / Files: content-addressed outputs with provenance.
- Usage Ledger: tokens/cost/quota/runtime measurements (no content payloads).

Each category carries the highest classification of what it may contain and
inherits the corresponding handling rules. Storage schemas are not defined
here.
