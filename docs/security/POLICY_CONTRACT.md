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

TASK-0062 materializes this lifecycle as durable local approval state. The
stored record retains the exact request/origin metadata, policy fingerprint,
expiry, requested authority, effective-if-approved authority and a stable
exact-binding fingerprint. Human resolution can only approve or deny an
already-created canonical `REQUIRE_APPROVAL` request; the control plane cannot
create an approval requirement, alter its binding or consume authority.

An approved row is not a bearer grant. The operation owner must re-present the
current canonical `REQUIRE_APPROVAL` decision before authority becomes usable.
The trusted approval service recomputes the exact binding, rejects stale or
widened bindings, expiry, denial and prior consumption, then atomically consumes
the approval exactly once. Restart does not erase a decision or make replay
valid. The current local control plane records approver provenance inside the
authenticated daemon boundary rather than accepting arbitrary provenance text
from the CLI.

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

TASK-0061 makes this precedence executable with ordered `PolicyLayer` values:

```text
BuiltInHard
  -> UserManaged
  -> Workspace
  -> Repository
  -> RequestSession
```

`BuiltInHard` and `UserManaged` are the positive authorization roots in the
current local policy model. `Workspace`, `Repository` and `RequestSession`
rules are restriction-only: they may add `DENY` or `REQUIRE_APPROVAL`, and an
`ALLOW` in one of those layers is never sufficient to create authority. This
makes repository/session self-grant structurally impossible while retaining
their rule provenance for explanation/audit.

No matching positive authorization fails closed as `DENY` with stable
`policy.default_deny` / `no_authorizing_rule` provenance. Policy compilation
rejects duplicate rule identities; unknown capability names and malformed
typed scopes are rejected by the capability contract instead of being silently
ignored.

Rule matching is scope-aware. An `ALLOW` rule must contain the complete
requested capability. `DENY` and `REQUIRE_APPROVAL` apply on typed scope
overlap so a narrow restriction cannot be bypassed by requesting a broader
filesystem root. Filesystem overlap requires the same workspace and path
component containment; `src/api` therefore does not match `src/api2`. Initial
non-filesystem `ScopeId` resources remain exact-match only.

One `PolicyRequest` is authorization-atomic. Multiple requested capabilities
are evaluated individually and composed with `DENY > REQUIRE_APPROVAL >
ALLOW`, but a mixed result does not expose partial authority: any denial denies
the whole request; otherwise any approval requirement withholds the whole
request until approval. Callers that need independently executable partial
authority must issue separate policy requests.

Parent authority is checked before rule evaluation through the typed
`EffectiveCapabilities::narrow` contract. Therefore an engine result can never
make a child capability wider than the request or the parent authority.

## Policy compilation and fingerprint

`CompiledPolicy` is the canonical in-memory TASK-0061 policy form. Source
loading/parsing remains outside the engine. Compilation validates stable rule
identity, canonicalizes rule order independently of insertion order and
computes a versioned SHA-256 fingerprint over layer, rule identity, reason,
effect and typed capability scope. Secret capability entries contain only
opaque secret identity; no secret value enters the fingerprint.

The same canonical policy produces the same fingerprint. A relevant rule,
effect, layer or scope change changes it. `PolicyDecision` retains the
fingerprint plus structured matching-rule evidence for later Security Audit
persistence.

For `REQUIRE_APPROVAL`, TASK-0061 creates the existing exact-bound
`ApprovalRequirement`: the binding contains the exact `PolicyRequest`, current
policy fingerprint and caller-supplied expiry, while `effective` remains empty.
`effective_if_approved` is already bounded by both request and parent
authority. TASK-0062 persists that requirement, records one human resolution
and makes approval authorization single-use. Persistence and IPC never become
independent authorization roots: only an exact current `REQUIRE_APPROVAL`
decision can register or consume approval authority.

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

TASK-0060 selected none of these backends, and TASK-0064 deliberately still
does not choose one because the concrete platform matrix remains open. An
adapter must advertise only what it actually enforces; filesystem restriction
never implies network isolation.

TASK-0064 implements the bounded provider-neutral foundation in `crates/runner`:

```text
EffectiveCapabilities + exact process + IsolationRequirements
  -> deterministic IsolationPlan
  -> IsolationBackend
  -> exact-plan EnforcementReport
  -> verify required filesystem/network/process axes
  -> privileged process operation
```

The exact `ProcessExecute` capability is checked immediately before backend
enforcement. Filesystem capabilities must name the same managed workspace as
the execution. Backend identity and the full plan are rechecked before the
operation closure runs, so an enforcement report is not a reusable bearer
grant and a stale/widened report cannot authorize another operation.

`adapters/runner-native` declares `Trusted Host` semantics and reports no
filesystem/network/process sandbox enforcement. It can satisfy only explicit
trusted-host requirements. If any isolation axis is required, NativeRunner is
insufficient and execution fails closed. TASK-0064 does not claim that current
provider execution is OS-sandboxed and does not select a concrete platform
backend; the platform matrix remains open. TASK-0065 closes H7 provider
environment pollution at the provider launch boundary; final G6 production-
path integration remains TASK-0067.

## Secrets Broker boundary

```text
execution/tool
  -> secret capability request
  -> Policy
  -> EffectiveCapabilities
  -> Secrets Broker
  -> exact SecretRef + exact Execution|Tool binding
  -> local native credential backend
  -> trusted scoped delivery at execution/tool boundary
  -> Security Audit
```

Secret values live in OS Keychain/Credential Manager/Secret Service or another
approved secure backend. Application persistence stores opaque references and
bindings only. Values do not enter SQLite domain state, Event Journal, generic
domain events, ordinary logs, frontend state, prompts or model context. Broad
environment inheritance is not an accepted delivery mechanism.

TASK-0063 broker release consumes only already-computed effective authority.
It requires exact `SecretAccess(ref)` plus the exact bound delivery target
before any backend read. A stored approval decision is not a bearer credential;
the operation owner must first pass the TASK-0062 authorization boundary and
obtain current effective capabilities. Secret bytes are borrowed only through a
trusted delivery callback and the broker clears its temporary mutable buffer
after the delivery attempt. The native local backend has no file or cloud
fallback.

Provider-native login credentials remain provider owned; AI Engineering OS
does not scrape them. Provider-native login or permission state is never
canonical secret-access authority.

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
worktree. TASK-0065 closes that finding at the provider-process environment
boundary. The production Claude launch now clears inherited environment and
rebuilds a narrow allowlist; provider-writable XDG config/data/state/cache and
scratch/temp directories are private application-owned paths outside the
managed worktree. Unsafe/overlapping environment roots fail before spawn.

Provider-native authentication remains provider-owned and is not copied into
Runlith persistence. The current Claude CLI requires the existing `HOME`/
profile as an explicit authentication anchor; removing it causes the CLI to
report `Not logged in`. That anchor is therefore allowlisted while ambient API
keys, cloud credentials, Git/SSH overrides and provider/plugin variables are
not inherited. Provider `--safe-mode` is defense in depth only. None of these
environment mechanics can ALLOW, approve, widen `EffectiveCapabilities`,
release a SecretRef or satisfy a Runner isolation requirement.

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
