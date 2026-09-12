# G6 Security Audit Persistence (TASK-0066)

> Implementation/evidence record for TASK-0066.
>
> Target development version: `0.1.0-dev.task0066`.
> G5 remains `CLOSED`. G6 remains `IN_PROGRESS`.

## Canonical scope and acceptance

TASK-0066 materializes the Security Audit persistence boundary already defined
by ADR-0006, ADR-0013, ADR-0026, INV-073 and INV-152:

- persist canonical product-authored security evidence in a store separate
  from Event Journal, operational logs, transcripts and model-visible receipts;
- cover the canonical minimum vocabulary: policy request/decision/effective
  authority, approval requested/decided, secret access, privilege elevation,
  denied operation, isolation enforcement/violation and security-relevant
  provider/tool invocation;
- retain exact structured correlation/provenance references without secret
  values, provider/model narrative, environment dumps or generic payloads;
- make normal persistence append-oriented and locally tamper-evident;
- survive restart with stable identities and deterministic ordered reads;
- expose a bounded correlation read surface for forensic reconstruction;
- keep Security Audit evidence non-authorizing.

Out of scope: TASK-0067 production-path integration/adversarial matrix,
TASK-0068 independent G6 closure, enterprise SIEM/compliance export, remote or
cloud audit backends, external signing/key management, telemetry/analytics,
per-record retention controls and G7+ work.

## Architecture and trust boundary

`crates/security-audit` owns the provider-neutral typed contract. It accepts
only bounded structured facts whose authoritative author is one of the
product-controlled security/runtime boundaries: Policy Engine, Approval
Service, Secrets Broker, Runner, Provider Environment, Runtime or Tool Service.
The crate deliberately has no provider/model text field. Provider adapters do
not depend on this crate in TASK-0066 and cannot write audit state through the
provider execution contract.

Security Audit records evidence. They never ALLOW, approve, widen
`EffectiveCapabilities`, release a secret, satisfy Runner enforcement or mutate
Task/Event Journal state.

## Storage, ordering and tamper evidence

SQLite advances from schema v9 to v10 via additive migration
`0010_security_audit.sql`. Historical migrations are unchanged.

The new schema owns:

- `security_audit`: one canonical record per monotonic local sequence;
- `security_audit_capabilities`: deterministic typed capability scopes;
- `security_audit_head`: persisted sequence/hash head for tail-truncation
  detection when the head itself remains intact.

Identity is `aud-<seq>`. Each row stores `previous_hash` and `record_hash`; the
hash is SHA-256 over a versioned, length-delimited serialization of every
canonical field and every sorted/deduplicated capability. Reads recompute the
chain before returning records. SQL triggers reject normal UPDATE/DELETE on the
record and capability tables.

This is tamper-evidence for the accepted local application threat model. It is
not a signature anchored outside the database: a fully compromised local host
that can replace the database/application can recompute or replace the chain.
THR-025 already keeps local host compromise as residual risk.

## Record taxonomy and provenance

The v1 event taxonomy is:

```text
policy.requested
policy.decided
authority.effective
approval.requested
approval.decided
secret.access
privilege.elevation
operation.denied
isolation.enforced
isolation.violation
provider.invoked
tool.invoked
```

Records may carry correlation/causation, operation, Task/Execution/Attempt,
typed subject, policy fingerprint, rule/reason, approval request and capability
references. Event-specific validation requires the canonical fields needed to
interpret policy decisions, approval outcomes, secret references and Runner
evidence. Capabilities are reconstructed through the existing `policy` typed
scope validation; malformed filesystem or exact-ID scope fails closed.

## Secret safety and data minimization

There is no `payload`, `secret_value`, provider text, transcript, context or
environment column. Secret audit evidence carries only an opaque `SecretRef`
subject/capability identity. The schema cannot persist secret bytes through its
typed API, and Security Audit appends never write Event Journal rows.

The store intentionally does not accept arbitrary JSON. This prevents a
convenient escape hatch for credentials, prompts, tool output or provider
narrative to become canonical security evidence.

## Durability, retention and queries

Audit records live in the same app-owned SQLite database but in independent
tables. Reopen/restart preserves sequence, identities, chain and structured
facts. Reads are:

- full ordered history, with chain verification;
- correlation-scoped history, also after full chain verification.

No automatic TTL, compaction or per-record delete API exists in v1. Audit
history persists with app-owned local data until explicit whole-data deletion
or a future privacy/retention migration introduces narrower semantics. The
store has no IPC/CLI/frontend query surface in TASK-0066; those surfaces are
not required by the canonical task and would expand the security exposure.

## Transaction boundary

One audit append, all capability rows and the persisted head advance commit in
one SQLite IMMEDIATE transaction. A partial record cannot commit.

TASK-0066 does not retrofit audit appends into Policy/Approval/Secrets/Runner/
Provider production operations. The canonical queue assigns that integrated
production path and adversarial failure matrix to TASK-0067. Therefore this
task does not claim atomic business-action + audit commit across boundaries it
does not yet own.

## Interaction with existing G6 boundaries

- Policy remains the only canonical authorization authority; policy
  fingerprints/rule/reason are evidence only.
- Approval state remains exact-bound durable current security state; an audit
  approval record cannot be replayed as a grant.
- Secrets Broker remains the only secret release boundary; audit stores opaque
  reference/target metadata only, never bytes.
- Runner remains the enforcement boundary; isolation audit records prove what
  the trusted product recorded, not permission to execute.
- Provider Environment/H7 isolation is unchanged; provider-owned auth/runtime
  state is never copied into audit storage.
- Event Journal remains the domain/runtime history store and receives no
  Security Audit append.

## Verification evidence

Targeted RED/GREEN work found one expected schema-bump regression: the existing
future-schema test still expected supported version 9 after migration v10 was
introduced. The observed failure was `(99, 10) != (99, 9)`; updating that stale
test expectation restored the full storage suite. No product defect was found.

Targeted verification after the final audit-storage shape:

```text
cargo test -p security-audit
2 passed; 0 failed

cargo test -p storage-sqlite --test security_audit
9 passed; 0 failed

cargo test -p storage-sqlite
110 passed; 0 failed; 9 suites

cargo test -p policy
44 passed; 0 failed; 3 suites

cargo test -p app-services --test approval_service
4 passed; 0 failed; 1 suite

cargo test -p secrets
4 passed; 0 failed; 2 suites

cargo test -p runner
7 passed; 0 failed; 3 suites

cargo test -p provider-claude
72 passed; 0 failed; 1 ignored; 5 suites

cargo check -p security-audit --target x86_64-pc-windows-msvc
PASS

cargo clippy -p security-audit -p storage-sqlite --all-targets --all-features -- -D warnings
PASS - No issues found
```

The audit-specific suite proves restart/durability, stable identity/order,
correlation reads, append-only SQL guards, mutation/hash detection, tail
truncation detection, complete minimum taxonomy round-trip, Event Journal
separation, lack of generic/secret payload columns and fail-closed rejection of
malformed canonical facts.

The first full pre-merge candidate pass after code/docs/version changes was:

```text
git diff --check
PASS

cargo fmt --all -- --check
PASS

cargo check --workspace
PASS - 84 crates compiled

cargo clippy --workspace --all-targets --all-features -- -D warnings
PASS - No issues found

cargo test --workspace --no-fail-fast
813 passed; 0 failed; 1 ignored; 95 suites
```

The same mandatory suite is rerun after this evidence record is finalized,
before commit/merge. Post-merge verification is reported from merged `main`;
it is intentionally not back-written into the already-merged implementation
commit.

## Gate status and next task

TASK-0066 completes the persistence substrate only. G5 remains `CLOSED`. G6
remains `IN_PROGRESS`.

After TASK-0066 is accepted in `main`, the next canonical task is TASK-0067 —
G6 Integration and Adversarial Security Verification, target
`0.1.0-dev.task0067`, recommended GPT-5.6 Sol / High unless canonical docs
change. TASK-0067 is not started here.
