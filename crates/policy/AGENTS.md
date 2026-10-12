# Policy - derived navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived guidance only; canonical policy is [AGENT.md](../../AGENT.md).
Package: [policy](Cargo.toml). Authority contract:
[ADR-0026](../../docs/architecture/adr/ADR-0026-g6-security-authority-and-capability-contracts.md).

## Source map and consumers

| Concern | Primary source | Role |
| --- | --- | --- |
| Typed authority | [lib](src/lib.rs) | Capability scopes, requested/effective sets, requests, decisions and approval bindings. |
| Deterministic rules | [engine](src/engine.rs) | PolicyLayer, PolicyRule, CompiledPolicy, PolicyEngine and decision evidence. |
| Test mutation governance | [test_mutation](src/test_mutation.rs) | Forbidden/ScopedReviewed values, exact scopes, revision and fingerprint validation. |
| Rule behavior tests | [policy_engine](tests/policy_engine.rs) | Layer precedence, narrowing, atomic decisions and fingerprints. |

[Runner](../runner/src/lib.rs) compiles `IsolationPlan` from effective capabilities
and enforces them at process launch. Policy rules remain here, not in concrete
runner/provider adapters; adapters implement the already-decided restrictions.
[ApprovalService](../app-services/src/approval_service.rs) owns durable grant
registration and consumption; this crate owns deterministic authority values.

## Local traps

- [Layer order](src/engine.rs) is `BuiltInHard`, `UserManaged`, `Workspace`,
  `Repository`, `RequestSession`. Only the first two can grant; weaker layers
  restrict existing authority. No authorizing rule means default deny.
- [Rule matching](src/engine.rs) uses subset checks for allows and overlap for
  denies/approval requirements. A broad filesystem request cannot escape a
  narrower restriction. Compilation rejects duplicate rule IDs and sorts rules
  before hashing, so insertion order does not change the policy fingerprint.
- [Evaluation](src/engine.rs) first narrows against parent effective authority.
  Mixed requests compose as `Deny > RequireApproval > Allow`; denial and pending
  approval expose no partial effective capabilities.
- [Approval application](src/lib.rs) binds request ID, exact request, policy
  fingerprint and validity window. Hard denial cannot be approved. Durable
  single-use consumption belongs to [ApprovalService](../app-services/src/approval_service.rs),
  not a reusable capability token created by this value layer.
- [ScopedReviewed](src/test_mutation.rs) requires nonempty exact path and check
  sets; both must match. It is not a wildcard grant. Deserialization rejects
  noncanonical encoding; restored revisions verify the policy fingerprint.

## Targeted verification from repository root

```sh
rtk cargo test --locked -p policy --lib
rtk cargo test --locked -p policy --test policy_engine
rtk cargo test --locked -p app-services --test approval_service
```

Inline tests in [lib](src/lib.rs) and [test_mutation](src/test_mutation.rs) cover
contract validation; [engine tests](tests/policy_engine.rs) cover rule composition.
[Approval service tests](../app-services/tests/approval_service.rs) cover durable
binding, expiry and replay. Commands are entry points, not execution evidence.
