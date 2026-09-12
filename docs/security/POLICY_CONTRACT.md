# G6 Policy and Capability Contract

> Canonical G6 authorization contract. Threats live in
> [`THREAT_MODEL.md`](./THREAT_MODEL.md); trust-zone detail lives in
> [`TRUST_BOUNDARIES.md`](./TRUST_BOUNDARIES.md). Decision authority is
> ADR-0026.

## Authority

AI Engineering OS core is the canonical authorization authority. The model,
provider CLI, provider adapter, repository, plugin, shell script, frontend and
provider-native permission configuration cannot grant authority.

Canonical flow:

```text
operation request
  -> typed requested capabilities + exact scope
  -> deterministic policy evaluation
  -> ALLOW | DENY | REQUIRE_APPROVAL
  -> local enforcement point
  -> operation
  -> Security Audit evidence
```

Policy decides. Operation owners enforce. Security Audit records. A preflight
alone is diagnostic and cannot replace the enforcement check adjacent to the
privileged operation.

## Capability model

G6 starts with eight provider-neutral kinds:

| Capability | Initial scope |
| --- | --- |
| filesystem read | managed workspace + relative root |
| filesystem write | managed workspace + relative root |
| process execute | exact executable/tool identity |
| network access | exact host/destination identity |
| Git mutate | exact repository identity |
| secret access | opaque secret identity |
| tool invoke | exact tool/plugin identity |
| external action | exact action identity |

This is the minimum authority vocabulary, not the provider capability taxonomy
from `providers.md`. Future scope forms require an explicit contract change;
arbitrary string expressions/wildcards are not accepted policy syntax.

Filesystem paths are logical relative roots inside a managed workspace. Lexical
validation rejects absolute paths and `..`. Runtime enforcement must still
resolve/canonicalize against the owned workspace and defend symlink/race
escapes; the typed contract does not claim filesystem atomicity.

## Requested and effective authority

Requested authority describes what an agent/provider/tool asks to do.
Effective authority is what the product permits after parent authority and
policy constraints are applied.

```text
requested != effective

effective ⊆ requested
child effective ⊆ parent effective
```

Delegation that introduces a new capability kind, another resource, another
workspace or a wider filesystem root is refused. Providers consume effective
authority; they never turn requested authority into effective authority.

## Decision semantics and precedence

`PolicyDecision` has three outcomes:

```text
DENY > REQUIRE_APPROVAL > ALLOW
```

The ordering is restrictive composition, not user-interface priority. Any hard
`DENY` remains `DENY` regardless of a weaker layer that says `ALLOW` or an
approval object. Repository/workspace policy can add restrictions but cannot
weaken stronger built-in/user-managed hard policy.

Unknown capability, malformed scope, unsupported mandatory restriction,
unprovable child scope or missing secret authorization fails closed. Internal
system failures use a separate error path and are not mislabeled as policy
denials.

For identical canonical request + policy state, authorization is deterministic.
An LLM may explain or recommend; it never decides hard authorization.

## Approval boundary

An approval request binds:

- approval request identity;
- exact `PolicyRequest` (operation ID, requester, Task/Execution/Attempt
  provenance, requested scoped capabilities);
- policy fingerprint;
- expiry;
- effective authority that would become usable if approved.

An approval resolution records decision, approver provenance and decision time.
Approval applies only when request ID and the complete binding match and the
binding is unexpired. A grant for `workspace/src/api` cannot authorize
`workspace/src`; changing operation identity, capability scope, policy
fingerprint or origin requires re-evaluation/new approval. `DENY` has no
approval conversion path.

Cryptographic approval tokens and persistent approval storage are deferred to
TASK-0062.

## Policy sources

Initial precedence principle:

```text
built-in hard policy
  -> user/local managed policy
  -> workspace policy
  -> repository policy
  -> request/session preferences
```

Each weaker layer may narrow or require approval. None may widen authority
forbidden by a stronger layer. Future organization policy can be inserted as a
strong managed layer without changing the capability algebra. Repository
content is untrusted and can never grant itself authority.

## Enforcement points

| Operation | Request | Decide | Enforce | Audit |
| --- | --- | --- | --- | --- |
| provider process launch | Agent Runtime | Policy | Runner/process launcher | Security Audit |
| child/process execution | Runtime/tool | Policy | Runner/process backend | Security Audit |
| filesystem access/mutation | runtime/tool | Policy | sandbox/fs operation boundary | Security Audit when security-relevant |
| Git mutation | Workspace/orchestration | Policy | Git adapter + runner boundary | Security Audit |
| network access | web/tool/runtime | Policy | network-capable runner/proxy boundary | Security Audit |
| secret access | execution/tool | Policy | Secrets Broker + delivery boundary | Security Audit |
| plugin/tool invocation | runtime | Policy | tool/plugin dispatcher | Security Audit |
| future remote dispatch | orchestration | Policy | remote-runner transport/worker | Security Audit |

Policy Engine must not absorb these implementation responsibilities.

TOCTOU rule: a read-only decision made before mutable state changes is not
sufficient proof at execution time. Enforcement must revalidate the facts it
depends on or use a mechanism that couples restriction to the operation. Exact
atomic mechanisms are deferred to their implementation tasks.

## Provider boundary

Provider adapters may map effective authority to provider-native flags/modes,
launch under restrictions already chosen by core policy, and report
provider-specific failures. They cannot decide canonical allow/deny, grant or
broaden capabilities, approve actions or bypass hard policy.

Provider-native sandbox/permission modes are defense in depth. If they cannot
represent a mandatory restriction, the product must enforce the restriction at
another trusted boundary or fail closed.

## Runner / OS isolation

Canonical authority is capability based. Named runner/security profiles may be
added as user-facing presets only if they compile to capability and isolation
requirements.

Platform adapters remain replaceable:

- macOS: product-compatible sandbox/Seatbelt-equivalent mechanisms;
- Linux: namespace/seccomp/cgroup/filesystem/network mechanisms as appropriate;
- Windows: restricted token/Job Object/AppContainer-equivalent mechanisms as
  appropriate.

TASK-0060 selects none of these backends. An adapter must advertise only what
it actually enforces; filesystem restriction never implies network isolation.

## Secrets Broker boundary

```text
execution/tool
  -> secret capability request
  -> Policy
  -> Secrets Broker
  -> scoped delivery at execution boundary
  -> Security Audit
```

Secret values live in OS Keychain/Credential Manager/Secret Service or another
approved secure backend. Application persistence stores opaque references and
bindings only. Values do not enter SQLite domain state, Event Journal, generic
domain events, ordinary logs, frontend state, prompts or model context. Broad
environment inheritance is not an accepted delivery mechanism.

Provider-native login credentials remain provider owned; AI Engineering OS
does not scrape them.

## Security Audit boundary

Security Audit is distinct from Event Journal, operational logs, transcript and
Tool Execution Receipts. The audit vocabulary covers at minimum policy request,
policy decision/rule, effective authority, approval requested/decided, secret
access, privilege elevation, denied operation, isolation violation and
security-relevant provider/tool invocation.

Product-controlled code creates canonical audit records. Provider/model/tool
text may be attached as untrusted context but cannot author the authoritative
decision/result. Persistence should be append-oriented and tamper-evident for
the local threat model; exact storage/signing/retention is TASK-0066.

## H7 provider environment isolation

TASK-0059 reproduced real Claude writing `data/sessions.db` inside a managed
worktree. G6 assigns H7 to TASK-0065. Its architectural owner is the
Runner/provider-process environment boundary: controlled provider HOME/XDG/data
directories outside the managed worktree, sanitized inherited environment,
explicit writable scratch/state areas and filesystem policy. TASK-0060 records
the owner and dependency only; it does not implement the remediation.

## Local-first constraints

Local policy evaluation, approval enforcement, secret access and runner
isolation must work without cloud, account or mandatory telemetry. Secrets are
local by default. Future cloud/team policy may add stricter managed constraints
but cannot become a dependency for securing local execution.

## G6 task sequence

1. TASK-0061 — Policy Engine Core: deterministic rule composition/evaluation
   over the contracts in `crates/policy`.
2. TASK-0062 — Approval and Human Decision Enforcement: durable bounded
   approval lifecycle, replay/expiry enforcement, control-plane surface.
3. TASK-0063 — Secrets Broker: OS credential-store port, opaque refs, scoped
   release and redaction boundaries.
4. TASK-0064 — Runner / OS Isolation Foundation: capability-to-runner policy,
   fail-closed backend boundary and platform adapter contracts.
5. TASK-0065 — Provider Environment Isolation: close H7 with controlled
   provider state/home/scratch/environment outside managed worktrees.
6. TASK-0066 — Security Audit Persistence: append-oriented canonical audit
   storage and correlation, separate from Event Journal.
7. TASK-0067 — G6 Integration and Adversarial Security Verification: wire the
   G6 boundaries and run the security failure matrix without claiming closure.
8. TASK-0068 — Independent G6 Closure Audit.

G6 remains in progress until the independent closure audit succeeds.
