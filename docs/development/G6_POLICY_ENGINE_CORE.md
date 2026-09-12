# G6 Policy Engine Core (TASK-0061)

> Implementation/evidence record for TASK-0061.
>
> Development version: `0.1.0-dev.task0061`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`.

## Scope

TASK-0061 materializes deterministic provider-neutral policy evaluation over
the capability contracts established by TASK-0060. It does not implement
approval persistence/UI, Secrets Broker, OS sandboxing, H7 provider-environment
isolation, Security Audit persistence, remote workers/cloud RBAC or G7.

## Implementation

`crates/policy/src/engine.rs` adds:

- explicit ordered `PolicyLayer` values: `BuiltInHard`, `UserManaged`,
  `Workspace`, `Repository`, `RequestSession`;
- typed `PolicyRule` values over the existing `Capability` algebra;
- deterministic `CompiledPolicy` validation/order/fingerprint;
- `PolicyEngine::evaluate` using the existing `PolicyRequest`,
  `EffectiveCapabilities`, `PolicyDecision` and approval contracts;
- structured `PolicyRuleEvidence` retained on engine decisions.

No provider-specific rule type or branch exists.

### Layer semantics

The current local positive authorization roots are `BuiltInHard` and
`UserManaged`. `Workspace`, `Repository` and `RequestSession` are
restriction-only: they may narrow with `DENY` or `REQUIRE_APPROVAL`, while
their `ALLOW` rules cannot create authority.

Restrictive composition is explicit:

```text
DENY > REQUIRE_APPROVAL > ALLOW
```

No matching positive authorization returns a stable default deny:

```text
rule_id     = policy.default_deny
reason_code = no_authorizing_rule
```

### Requested/effective/parent authority

Parent containment is checked before policy rules through the existing
`EffectiveCapabilities::narrow` contract. Therefore an accepted effective
capability is always within both the exact request and parent authority.
Capability kind, workspace or scope widening returns the existing typed
`AuthorityExpansion` failure.

One `PolicyRequest` is authorization-atomic. Per-capability outcomes are
evaluated and then composed restrictively. Any deny returns no effective
authority. Otherwise any approval requirement withholds all current effective
authority. Callers needing independent partial execution must split the
operation into separate requests.

### Scope matching

`ALLOW` requires the entire requested capability to fit inside the typed rule
scope. `DENY` and `REQUIRE_APPROVAL` use typed overlap, preventing a narrow
restriction from being bypassed by asking for a broader filesystem root.

Filesystem matching requires the same workspace plus `Path` component
containment. `src/api` and `src/api2` therefore do not overlap by string prefix.
Initial non-filesystem `ScopeId` capabilities remain exact resource matches.

### Approval boundary

`REQUIRE_APPROVAL` reuses TASK-0060 `ApprovalBinding` and
`ApprovalRequirement`. The decision exposes zero current effective authority.
`effective_if_approved` is already request/parent bounded. The binding carries
the exact request, current policy fingerprint and explicit expiry input.

TASK-0061 generates only the exact-bound requirement. Durable approval state,
replay/expiry enforcement and human-decision lifecycle remain TASK-0062.

### Policy compilation/fingerprint

Compilation rejects duplicate rule IDs and canonicalizes rule order independent
of insertion order. Unknown capability strings and malformed scopes continue to
fail through TASK-0060 typed constructors/parsing rather than being ignored.

The policy fingerprint is versioned SHA-256 over canonical layer, rule ID,
reason code, effect and typed capability scope. It includes opaque secret
identity only, never secret material. Rule insertion order does not affect the
fingerprint; relevant policy changes do.

`sha2` was already present in the workspace dependency graph and is now a
direct `crates/policy` dependency because approval binding needs a stable,
collision-resistant fingerprint rather than an implementation-defined standard
library hasher.

### Explanation and future Security Audit

Engine decisions retain:

- primary stable `rule_id` and `reason_code`;
- policy fingerprint;
- structured matching-rule evidence including layer, effect, requested
  capability and rule capability.

This is sufficient input for TASK-0066 Security Audit persistence without
writing audit state during TASK-0061.

### TOCTOU boundary

TASK-0061 decides authority only. It does not make policy preflight an
enforcement boundary. Filesystem/process/Git/network/secret/tool operation
owners must still enforce or revalidate the decided authority adjacent to the
privileged operation per INV-153.

## Required/adversarial test evidence

`crates/policy/tests/policy_engine.rs` plus the existing TASK-0060 contract
tests cover the required matrix, including:

- no-match default deny;
- built-in allow/deny/require-approval;
- stronger deny/approval over weaker allow;
- weaker deny/approval narrowing;
- repository/session non-widening and repository self-grant refusal;
- request and parent containment, equal/subset child authority, cross-workspace
  and unrelated-capability refusal;
- unknown capability and malformed scope failure;
- filesystem sub-scope and `src/api` vs `src/api2` prefix adversary;
- exact `ScopeId` matching;
- no effective authority before approval;
- multi-capability deny/approval atomicity;
- deterministic identical evaluation;
- insertion-order-independent fingerprint and fingerprint change on policy
  semantics change;
- stable rule/reason evidence;
- provider authority broadening refusal;
- duplicate-rule compilation failure;
- deterministic corpus proving effective authority remains within request and
  parent;
- monotonicity check showing that adding a stronger deny cannot increase
  effective authority.

Targeted final policy suite:

```text
cargo test -p policy
43 passed; 0 failed; 0 ignored; 3 suites
```

## Full verification

Final-tree verification after moving engine tests out of the implementation
module:

```text
cargo fmt --all -- --check
PASS

cargo check --workspace
PASS (1 crate recompiled in the final cached run)

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS (no issues)

cargo test --workspace --no-fail-fast
765 passed; 0 failed; 1 ignored; 78 suites; 92.07s
```

Fresh pre-merge re-verification on 2026-09-12 first reproduced existing
process-test hermeticity findings: `binary_concurrent_resumes_elect_a_single_owner`
and `g1_real_process_matrix` failed while unrelated test daemons overlapped.
No matching `agentd` process remained after that run. Re-running the exact
mandatory command on a quiet host passed `765 passed; 0 failed; 1 ignored; 78
suites; 91.39s`. This successful exact-command rerun is the authoritative
pre-merge workspace evidence; the failed run is retained here as TEST-H-family
flakiness evidence rather than hidden.

## G5 regression status

`G5_CLOSED` remains valid. TASK-0061 does not modify G5 workspace/provider/
execution recovery paths. The complete workspace suite, including existing G5
regression tests, is green in the final run: `765 passed`, `0 failed`, `1
ignored`.

No evidence was found to reopen G5.

## Deferred findings / remaining G6

Existing deferred findings remain unchanged and were not bundled into this
task: N-1, N-2, F-SCAN, TEST-H, TEST-H2, Git `commondir`, H9 and H7.

G6 work still remaining:

1. TASK-0062 — durable approval and human decision enforcement.
2. TASK-0063 — Secrets Broker.
3. TASK-0064 — Runner / OS isolation foundation.
4. TASK-0065 — provider environment isolation and H7 closure.
5. TASK-0066 — Security Audit persistence.
6. TASK-0067 — integrated adversarial G6 verification.
7. TASK-0068 — independent G6 closure audit.

TASK-0062 was not started.

## Result

TASK-0061 is complete against its acceptance criteria.

- Development version: `0.1.0-dev.task0061`.
- G5: `CLOSED`.
- G6: `IN_PROGRESS`.
- Next task: TASK-0062, target `0.1.0-dev.task0062`, GPT-5.6 Sol, High
  reasoning.
