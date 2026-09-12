# ADR-0026 - G6 security authority and capability contracts

Status: Accepted
Date: 2026-09-12

## Context

ADR-0013 established that security requires both Policy Engine authorization
and runtime/OS isolation. G6 now needs a concrete provider-neutral authority
model before Policy Engine, approvals, Secrets Broker, runner isolation and
Security Audit implementations can be built.

The system executes untrusted model output, repository content, generated
code, tools, plugins, build scripts and provider processes. Provider-native
permission modes, prompts and frontend state therefore cannot be canonical
authorization.

## Decision

AI Engineering OS core owns canonical authorization. A privileged operation is
represented as a typed `PolicyRequest` carrying requested scoped
capabilities. Deterministic policy evaluation produces exactly one of:

```text
ALLOW | DENY | REQUIRE_APPROVAL
```

`DENY` is non-approvable. `REQUIRE_APPROVAL` is approvable only for the exact
bound request, capability scope, policy fingerprint and validity window.
Approval never overrides a hard denial.

Requested and effective authority are separate types. Delegation is monotonic:

```text
child effective authority ⊆ parent effective authority
```

The initial capability vocabulary is deliberately small: filesystem read and
write, process execution, network access, Git mutation, secret access, tool
invocation and external action. Filesystem scope is a managed workspace plus a
validated relative root; other initial scopes are exact typed resource IDs.
No wildcard policy language is introduced by this ADR.

Policy layers may only narrow authority established by stronger layers.
Built-in hard policy cannot be weakened by repository, workspace, user/session,
provider or UI configuration. Unknown capabilities, malformed scopes and
unprovable security scope fail closed as policy-contract errors; ordinary
runtime/internal failures remain separate from policy denial.

TASK-0061 implementation clarification: the executable local layer order is
`BuiltInHard -> UserManaged -> Workspace -> Repository -> RequestSession`.
`BuiltInHard` and `UserManaged` are the current positive authorization roots;
the weaker workspace/repository/request-session layers are restriction-only and
cannot self-grant authority. Restrictive filesystem rules match typed scope
overlap, so requesting a broader root cannot bypass a narrower deny/approval
rule. Non-filesystem `ScopeId` resources remain exact-match.

One `PolicyRequest` is authorization-atomic. Per-capability results compose as
`DENY > REQUIRE_APPROVAL > ALLOW`; mixed requests expose no partial current
authority. If independent partial execution is desired, the operation owner
issues separate requests. Policy compilation is insertion-order independent and
binds decisions/approvals to a deterministic versioned SHA-256 policy
fingerprint plus stable rule/reason provenance.

TASK-0062 implementation clarification: approval state is durable current
security state, separate from Event Journal and the future Security Audit. An
approval record is created only from a canonical `REQUIRE_APPROVAL` decision.
The authenticated local control plane may read that record and resolve it as
approve/deny, but it cannot mint or alter the bound authority. Approved state
is not a bearer token: the operation owner must re-present the current exact
approval requirement, after which the trusted service atomically consumes the
matching unexpired grant once. Binding mismatch, denial, expiry and replay fail
closed.

Policy evaluation and enforcement are separate responsibilities. An
authorization preflight is not an enforcement boundary. The component that
performs a privileged filesystem/process/Git/network/secret/tool operation
must enforce the already-decided effective authority close enough to the
operation to avoid treating stale preflight state as authorization.

Provider adapters may translate effective authority into provider-native
controls and report metadata. They may not grant capabilities, broaden scope,
approve operations or bypass hard policy. Provider-native security is defense
in depth. If a required restriction cannot be represented or enforced, the
execution fails closed or selects a runner/isolation backend that can enforce
it.

Runner security profiles, if introduced, are convenience inputs that compile
to canonical capability/policy requirements. They never form a second
authorization model.

Secrets are referenced by opaque identity in application state. Secret values
live in an approved OS credential backend and are released only at an
authorized execution/tool boundary. Secret material never enters ordinary
SQLite domain state, Event Journal payloads, logs, model context or approval
explanations.

TASK-0063 implementation clarification: `crates/secrets` owns the
provider-neutral broker contracts. Durable application state stores only
`SecretRef` plus exact `SecretBinding` metadata to an execution or tool target;
SQLite schema v9 contains no secret-value/payload column and secret metadata
operations append no Event Journal facts. The broker accepts only
already-computed `EffectiveCapabilities`, verifies exact `SecretAccess(ref)`
and the exact delivery target before reading the backend, then lends secret
bytes only to a trusted `SecretDelivery` callback and clears its temporary
mutable buffer after the delivery attempt. The local backend adapter uses the
platform-native OS credential store (Apple Keychain, Windows Credential
Manager, Linux Secret Service) with no file or cloud fallback. A durable
TASK-0062 approval row is not bearer authority: release is possible only after
the canonical approval service has produced effective authority. Provider
native login state remains provider owned and cannot grant or widen canonical
authority.

Security Audit remains separate from Event Journal, operational logging and
model-visible execution receipts. Product-controlled code records canonical
security decisions and privileged-action evidence; provider/model metadata is
context only and cannot author authoritative audit facts.

## Consequences

- `crates/policy` is the small provider-neutral G6 contract owner.
- Policy Engine implementation can be added without changing provider, UI or
  persistence-domain contracts.
- Child/delegated execution cannot acquire authority through prompts or
  provider behavior.
- Approval replay outside the exact request/scope/policy binding is rejected.
- OS-specific isolation remains behind Runner/Sandbox adapters consuming
  canonical effective capabilities.
- Local execution requires no cloud control plane, account or telemetry.
- Secret release has no separate approval or provider-authority mechanism; it
  consumes canonical effective capability already decided by Policy/approval.
- H7 (`data/sessions.db` written into managed worktrees by real Claude) is
  owned by TASK-0065 provider-environment isolation after the Policy and
  Runner authority foundations exist.

## Rejected alternatives

- Boolean `allowed` APIs: rejected because they cannot distinguish hard denial
  from human approval.
- Provider-native permissions as authority: rejected because providers are
  untrusted executors and controls differ by provider/version.
- Prompt instructions as enforcement: rejected by ADR-0013/INV-013.
- Runner security-level enum as canonical authority: rejected because it
  duplicates the capability model and hides scope.
- One global Policy Engine object that performs every privileged operation:
  rejected; policy decides, operation owners enforce.

## Security impact

This ADR activates G6 invariants INV-146 through INV-153 and concretizes
ADR-0013 without changing its two-layer security model.

## References

- `docs/security/POLICY_CONTRACT.md`
- `docs/security/THREAT_MODEL.md`
- `docs/security/TRUST_BOUNDARIES.md`
- `docs/architecture/runners.md`
- `docs/architecture/providers.md`
- `crates/policy`
- `crates/secrets`
- `adapters/keychain-native`
- `adapters/storage-sqlite/migrations/0009_secret_refs.sql`
- `docs/development/G6_SECRETS_BROKER.md`
- `docs/development/G5_FINAL_CLOSURE_AUDIT.md` (H7)
