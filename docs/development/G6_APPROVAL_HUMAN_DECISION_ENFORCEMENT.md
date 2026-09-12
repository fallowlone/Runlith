# G6 Approval and Human Decision Enforcement (TASK-0062)

> Implementation/evidence record for TASK-0062.
>
> Development version: `0.1.0-dev.task0062`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`.

## Scope

TASK-0062 materializes the durable bounded lifecycle required by a canonical
`REQUIRE_APPROVAL` decision:

- durable approval state across daemon/process restart;
- exact binding to request/origin, requested scope, policy fingerprint, expiry
  and `effective_if_approved` authority;
- human approve/deny resolution through a minimal authenticated local control
  plane;
- expiry, mismatch, denial and replay refusal;
- single-use authorization after the operation owner re-presents the current
  exact `REQUIRE_APPROVAL` decision.

Out of scope and unchanged: production graphical approval UI, Secrets Broker
(TASK-0063), Runner/OS isolation (TASK-0064), H7 provider-environment isolation
(TASK-0065), Security Audit persistence (TASK-0066), G6 integration/closure
(TASK-0067/0068), remote/cloud approval authority and G7+.

## Authority and lifecycle

`ApprovalService` is the trusted application-service owner of the lifecycle.
Only a canonical `PolicyDecision::RequireApproval` can register or consume an
approval. `ALLOW` cannot manufacture approval state and `DENY` remains final.

The durable record is not a bearer token. The control plane can observe a
record and resolve it as approve/deny, but it cannot create an approval
requirement, change request/scope/policy/expiry, or consume authority. To use an
approved decision, the operation owner must re-present the current canonical
requirement. The service recomputes the exact binding fingerprint and only then
atomically consumes the matching unexpired approved row.

This preserves ADR-0026 / INV-148 / INV-150 / INV-153 authority order:

```text
Policy REQUIRE_APPROVAL
  -> durable pending approval
  -> authenticated human approve|deny
  -> operation owner re-presents current exact requirement
  -> exact binding + expiry + state check
  -> atomic single-use consume
  -> effective_if_approved authority
```

## Persistence

SQLite schema advances from version 7 to version 8 with
`0008_approvals.sql`:

- `approvals` stores request/origin metadata, policy fingerprint, exact binding
  fingerprint, expiry, lifecycle state, approver provenance and decision/
  consumption timestamps;
- `approval_capabilities` stores requested and effective-if-approved capability
  descriptions in deterministic position order;
- state is constrained to `pending | approved | denied | consumed`;
- approved/denied rows require decision provenance/time; consumed rows also
  require consumption time.

Approval state is current Policy/Security state. It is deliberately separate
from Event Journal and from the future append-oriented Security Audit owned by
TASK-0066. Historical audit documents were not rewritten for schema v8.

## Policy binding

`crates/policy` exposes a stable `approval-binding-v1` SHA-256 fingerprint over
the exact approval request identity and canonical request, policy fingerprint,
expiry and effective-if-approved capabilities. Secret capabilities contribute
only opaque secret identity already present in the capability contract.

Changing operation/request origin, capability scope, policy fingerprint,
expiry or effective authority changes the durable binding and cannot reuse an
existing approval id with different authority.

## IPC and CLI

Protocol v3 is additive. Historical protocol v0/v1/v2 remain supported; the
frozen G2 profile still advertises `[0,1]` and the frozen G5 profile advertises
`[0,1,2]`. G6 production composition advertises `[0,1,2,3]`; v3 adds only the
bounded `policy_core` surface while retaining cumulative business/execution
capability.

`policy_core.proto` exposes:

- `get_approval(request_id)`;
- `decide_approval(request_id, approve|deny)`.

There is no wire operation for register/mint/alter/consume. Human-decision
provenance is supplied inside the already OS-authenticated daemon boundary; the
CLI does not provide arbitrary approver text.

`agentctl` exposes:

```text
agentctl approval get <approval-request-id>
agentctl approval decide <approval-request-id> <approve|deny>
```

Decision exchanges are never automatically replayed after an ambiguous local
IPC result.

## Required/adversarial evidence

The TASK-0062 suites cover:

- restart durability of pending/approved state;
- exact-scope/policy/origin binding and widened/rebound mismatch refusal;
- expiry refusal;
- explicit human denial refusal;
- hard `DENY` finality and `ALLOW` inability to mint approvals;
- duplicate approval id with different binding refusal;
- one decision only and one authorization consume only;
- protocol-v3 negotiation while G2/G5 profiles remain frozen;
- real local IPC pending-read + human approve + daemon shutdown + durable
  post-restart authorization + second-consume replay refusal.

Fresh targeted verification on the final implementation tree:

```text
cargo test -p policy
44 passed; 0 failed; 0 ignored; 3 suites

cargo test -p storage-sqlite
98 passed; 0 failed; 0 ignored; 7 suites

cargo test -p app-services
198 passed; 0 failed; 0 ignored; 18 suites

cargo test -p ipc-contracts
20 passed; 0 failed; 0 ignored; 2 suites

cargo test -p ipc-control
41 passed; 0 failed; 0 ignored; 2 suites

cargo test -p agentd
58 passed; 0 failed; 0 ignored; 5 suites

cargo test -p agentctl
41 passed; 0 failed; 0 ignored; 10 suites
```

During implementation, storage verification first exposed current-schema tests
still asserting schema version 7; only assertions representing current schema
were advanced to 8. Historical audit claims were left immutable. IPC-control
verification also exposed two tests that equated the global current protocol
with G2/G5 profiles; those tests now assert the frozen profile version sets and
v3 has its own G6 capability gate.

## Full verification

Final pre-merge tree:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS (4 crates compiled in the final cached run)

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS (no issues)

cargo test --workspace --no-fail-fast
777 passed; 0 failed; 1 ignored; 81 suites; 76.27s
```

## G5 regression status

`G5_CLOSED` remains valid. The full workspace suite, including G5 provider,
workspace, recovery and process tests, is green. TASK-0062 adds protocol v3
without changing the frozen G5 v2 profile or provider authority model.

No evidence was found to reopen G5.

## Deferred findings / remaining G6

Existing deferred findings remain outside TASK-0062, including N-1, N-2,
F-SCAN, TEST-H, TEST-H2, Git `commondir`, H9 and H7.

G6 work still remaining:

1. TASK-0063 — Secrets Broker.
2. TASK-0064 — Runner / OS isolation foundation.
3. TASK-0065 — provider environment isolation and H7 closure.
4. TASK-0066 — Security Audit persistence.
5. TASK-0067 — integrated adversarial G6 verification.
6. TASK-0068 — independent G6 closure audit.

TASK-0063 was not started.

## Result

TASK-0062 is complete against its repository acceptance criteria on the task
branch.

- Development version: `0.1.0-dev.task0062`.
- G5: `CLOSED`.
- G6: `IN_PROGRESS`.
- Next task after merge/post-merge verification: TASK-0063, target
  `0.1.0-dev.task0063`; do not start it as part of TASK-0062.
