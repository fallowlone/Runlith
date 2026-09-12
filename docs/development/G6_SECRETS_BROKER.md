# G6 Secrets Broker (TASK-0063)

> Implementation/evidence record for TASK-0063.
>
> Development version: `0.1.0-dev.task0063`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`.

## Scope

TASK-0063 implements the provider-neutral Secrets Broker boundary defined by
ADR-0026 and `POLICY_CONTRACT.md`:

- opaque `SecretRef` identities and exact `SecretBinding` metadata;
- `SecretDeliveryTarget::{Execution, Tool}`;
- policy-authorized scoped release using already-computed
  `EffectiveCapabilities`;
- local OS credential backend adapter through `keyring 4.2.0`;
- SQLite metadata persistence for refs/bindings only;
- redacted errors/debug plus trusted borrowed delivery semantics.

Out of scope and unchanged: cloud secret manager/file fallback, TASK-0064
Runner/OS isolation, TASK-0065 provider-environment isolation/H7, TASK-0066
Security Audit persistence, TASK-0067/0068 integration/closure, secret
IPC/CLI/protobuf/frontend/model-context surfaces and provider credential
scraping.

## Authority and trust boundary

Canonical flow:

```text
Policy / TASK-0062 approval
  -> EffectiveCapabilities
  -> SecretsBroker
  -> exact SecretAccess(secret_ref)
  -> exact SecretBinding target (Execution | Tool)
  -> local OS credential backend
  -> trusted SecretDelivery
```

The broker has no approval mechanism and provider-native permission state
cannot grant authority. `release()` first resolves the exact binding, rejects a
target mismatch, checks exact secret capability, and only then reads the
backend.

The TASK-0062 integration test proves a durable `Approved` row is not bearer
authority: broker release with empty effective capabilities returns
`Unauthorized`; release succeeds only after the canonical
`ApprovalService::authorize()` path yields the bound effective capability.

## Persistence and leakage boundary

SQLite schema advances from version 8 to version 9 with
`0009_secret_refs.sql`.

`secret_refs` stores only:

```text
ref_id
```

`secret_bindings` stores only:

```text
binding_id | secret_ref_id | target_kind | target_id
```

No secret value/payload column exists. Secret-ref/binding registration and
authorized release append no Event Journal facts. Leakage tests read the raw
SQLite file bytes after release and verify the runtime-generated secret
material is absent.

Secret values remain in the OS credential store. The application owns only
opaque refs/bindings and a short-lived mutable buffer during release.

## Delivery and redaction

`SecretDelivery` receives borrowed bytes only at the trusted execution/tool
boundary. The broker clears its temporary mutable `Vec<u8>` after the delivery
attempt, including delivery failure.

Broker/backend error text is categorical and contains no secret payload.
`NativeKeychain` debug output exposes only the fixed service identity and never
credential bytes. Tests use runtime-generated binary material rather than
source literals that resemble credentials.

## Platform backend

`adapters/keychain-native` uses `keyring 4.2.0` and the platform-native store:

- macOS: Apple Keychain;
- Windows: Windows Credential Manager;
- Linux/*nix: Secret Service backend selected by `keyring`.

There is no file fallback and no cloud backend. Provider-owned CLI login
credentials are not extracted or copied into the broker. Windows cross-check
for the adapter passed during implementation.

## IPC and CLI

TASK-0063 adds no secret IPC, CLI, protobuf, frontend or model-context surface.
Secret refs/bindings remain internal trusted application contracts. A future
surface must preserve the same policy and delivery boundary rather than expose
secret values.

## Targeted acceptance evidence

Acceptance-level implementation verification before the canonical completion
patch recorded:

```text
crates/secrets
4 passed; 0 failed; 2 suites

adapters/keychain-native
3 passed; 0 failed; 2 suites

storage-sqlite
101 passed; 0 failed; 8 suites

approval boundary targeted suite
4 passed; 0 failed; 1 suite

app-services
199 passed; 0 failed; 18 suites

policy
44 passed; 0 failed; 3 suites

targeted cargo check/clippy
PASS
```

These suites cover exact target/capability refusal before backend read,
metadata conflict/corruption handling, raw SQLite leakage, empty Event Journal,
approval-not-bearer semantics, redacted error/debug behavior and native backend
contract behavior.

## Full workspace verification history

The implementation tree first reproduced the existing TEST-H/process
hermeticity family under overlapping test-owned daemons. This RED result is
retained as evidence and was not classified as product success. After the
test-owned overlap cleared, the exact workspace command was rerun on a quiet
state and passed:

```text
cargo test --workspace --no-fail-fast
788 passed; 0 failed; 1 ignored; 86 suites; 94.70s
```

The known TEST-H mechanism is documented by the G5 closure evidence:
machine-global/same-binary process assertions can observe unrelated test
daemons. TASK-0063 does not change process-hermeticity code and does not hide
the first RED run.

The final pre-merge verification after this canonical completion patch is
recorded below once run on the completed tree.

## Final pre-merge verification

Fresh verification after the canonical architecture/evidence/queue/version
completion patch:

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
788 passed; 0 failed; 1 ignored; 86 suites; 97.06s
```

The targeted acceptance suites were also rerun after the completion patch and
retained the counts recorded above: secrets 4, keychain-native 3,
storage-sqlite 101, approval boundary 4, app-services 199 and policy 44, all
with zero failures.

## Post-merge verification

Pending merge and mandatory verification on `main`.

## G5 regression status

G5 remains `CLOSED`. TASK-0063 adds the G6 secret boundary without changing
the accepted G5 provider/workspace execution authority model. No evidence in
TASK-0063 reopens G5.

## Deferred findings / remaining G6

Existing deferred findings remain outside TASK-0063, including N-1, N-2,
F-SCAN, TEST-H, TEST-H2, Git `commondir`, H9 and H7.

Remaining G6 sequence starts with TASK-0064 Runner and OS Isolation Foundation,
then TASK-0065 H7/provider environment isolation, TASK-0066 Security Audit
persistence, TASK-0067 integration/adversarial verification and TASK-0068
independent closure audit.

TASK-0064 is not started by this task.

## Result

TASK-0063 is complete against its implementation acceptance criteria on the
task branch once the final completed-tree verification below is green. Merge
acceptance remains contingent on mandatory post-merge verification.

- Public semver: `0.1.0`.
- Development version: `0.1.0-dev.task0063`.
- Queue: `completed_through: TASK-0063`, `current_gate: G6`,
  `gate_status: IN_PROGRESS`.
- G5: `CLOSED`.
- G6: `IN_PROGRESS`.
- Next canonical recommendation after accepted merge: TASK-0064 Runner and OS
  Isolation Foundation, target `0.1.0-dev.task0064`, GPT-5.6 Sol / High. It is
  explicitly not started here.
