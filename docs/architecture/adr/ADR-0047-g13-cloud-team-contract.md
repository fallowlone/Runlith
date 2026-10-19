# ADR-0047 — G13 Cloud/Team contract

- Status: **Accepted** (TASK-0207; G13 sequence registered by TASK-0206,
  ratified by owner `go`)
- Date: 2026-10-19
- Gate: G13
- Extends: source-of-truth §81 (future cloud: account, billing, encrypted
  sync, organizations, teams, org policies; local `agentd` stays
  authoritative for local execution), §82 device identity discipline

## Problem

G13 is the first gate where state may leave the device and where one
person's configuration can constrain another's execution. Without a
frozen contract, TASK-0208 through TASK-0213 would each invent their own
identity, policy-precedence, sync-conflict and quota semantics — and the
failure modes are not cosmetic: a synced secret, a silently weakened org
policy, or an auto-merged conflict each break a binding invariant
(INV-008, INV-011, INV-148). A name string is not an identity, a relay
that can read payloads is not encrypted sync, and "the cloud said so" is
never local execution authority.

## Decision

### 1. Identity: account, organization, team, membership

A **local account** is the operator identity on one device. It is not a
cloud account; no cloud account exists in G13 v1 (§8).

| field | contract |
|---|---|
| Account `id` | `acc_` + 26-char Crockford Base32 |
| `display_name` | bounded text |
| `signing_public_key` | 32-byte ed25519 public key (hex on the wire) |
| `fingerprint` | `sha256:<hex>` of the public key bytes |

The account signing keypair is generated on-device; the private key is
stored only through the existing vault/secrets layer and never crosses
the wire, logs, IPC payloads, or any sync envelope (same discipline as
ADR-0046 W-1).

| field | contract |
|---|---|
| Organization `id` | `org_` + 26-char Crockford Base32 |
| `name` | bounded text |
| `state` | `ACTIVE` or `ARCHIVED` (terminal for mutations) |
| Team `id` | `team_` + 26-char Crockford Base32; `org_id`; `name` bounded |
| Membership `id` | `mem_` + 26-char Crockford Base32 |
| Membership `org_id` / `account_id` | owning organization and account |
| Membership `team_id` | optional team within the same organization |
| Membership `role` | closed enum: `OWNER`, `ADMIN`, `MEMBER`, `VIEWER` |
| Membership `state` | `ACTIVE` or `REVOKED` (revocation is terminal) |

Role authority (checked in the trusted service, never by prompt or
display name):

| action | OWNER | ADMIN | MEMBER | VIEWER |
|---|---|---|---|---|
| create team | yes | yes | no | no |
| add member / change role / revoke | yes | yes* | no | no |
| write org policy | yes | yes | no | no |
| author sync envelope | yes | yes | yes | no |
| read org / team / usage | yes | yes | yes | yes |

\* An `ADMIN` cannot grant, change, or revoke the `OWNER` role. Nobody
changes their own role. Any other role attempt refuses typed (C-4).
Creating a membership is an authorized act by an existing `OWNER`/`ADMIN`;
creation starts `ACTIVE`. Revocation is durable and immediate: from the
revocation moment, the revoked member's mutations and envelopes refuse
(C-5).

### 2. Layered organization policy

`OrgPolicy` (normative fields):

| field | contract |
|---|---|
| `id` | `pol_` + 26-char Crockford Base32 |
| `org_id` | owning organization |
| `version` | u64, monotonic per organization |
| `hard_capability_ceiling` | ADR-0045 enum (`READ_ONLY`, `WORKSPACE_WRITE`, `EXECUTION`); nothing in this org may execute above it |
| `sync_max_sensitivity` | highest `DATA_CLASSIFICATION` level allowed in sync (§4) |
| `quota_max_cost_microusd` | optional u64 spend limit per quota window |
| `quota_window_seconds` | u64 window for the spend limit |
| `updated_by` / `updated_at` | account ref; Unix millis UTC |

Resolution composes organization > project > user exactly as
`POLICY_CONTRACT.md` composes restrictive layers
(`DENY > REQUIRE_APPROVAL > ALLOW`): the effective value of each knob is
the **most restrictive** across layers. A project or user layer that
would raise the org hard ceiling, raise the sync sensitivity ceiling, or
raise the spend limit is a weakening attempt: it refuses typed
(`PolicyWeakening`) and the effective policy is unchanged (C-8, INV-148).

Org policy is an input to the existing G6 Policy Engine as its strong
managed layer (the insertion `POLICY_CONTRACT.md` reserves). `OrgService`
never grants execution authority itself; canonical authorization remains
the Policy Engine decision (INV-146). Domain crates hold the resolution
predicate only — no adapter, provider, or IPC type enters it (INV-025).

### 3. Encrypted sync envelope (server-blind)

Sync is opt-in replication between local stores. The local store stays
canonical for every org fact (INV-070); any relay or peer store is an
untrusted carrier that sees ciphertext and bounded metadata only.

| field | contract |
|---|---|
| `id` | `env_` + 26-char Crockford Base32 |
| `org_id` | owning organization |
| `author_account_id` | signing account; its membership must be `ACTIVE` |
| `kind` | closed v1 enum: `ORG_PROFILE`, `TEAM`, `MEMBERSHIP`, `ORG_POLICY`, `USAGE_SNAPSHOT` |
| `revision` | u64; per-org linear stream, `revision == base_revision + 1` |
| `base_revision` | u64; sender's view of the current stream head |
| `max_sensitivity` | header classification of the plaintext (§4) |
| `cipher_suite` | closed v1 value: XChaCha20-Poly1305 AEAD + ed25519 signature |
| `nonce` / `ciphertext` | bounded bytes; ciphertext is the only payload a carrier sees |
| `signature` | ed25519 over `org_id \|\| revision \|\| base_revision \|\| kind \|\| sha256(ciphertext)` |
| `created_at` | Unix millis UTC |

The org AEAD key lives per member device in the vault and is provisioned
out-of-band in v1 (fingerprint-verified); a key-distribution protocol is
not G13 v1 scope (§8). Private keys and the org key never appear inside
an envelope.

Acceptance is one pure predicate in the org domain; every failure refuses
typed and applies nothing (no partial application):

- organization matches and `cipher_suite` is the v1 suite;
- author signature verifies and the author's membership is `ACTIVE` in
  the receiver's applied state;
- AEAD opens (tampered ciphertext refuses, C-6);
- `base_revision` equals the receiver's stream head, else `Conflict`
  (C-7) — v1 never auto-merges and never silently overwrites; rebasing
  is an explicit operator act producing a new envelope;
- envelope `id` is unseen, else `DuplicateEnvelope` (replay refuses, C-10);
- header `max_sensitivity` is at or below the org sync ceiling.

### 4. Data classification for sync

Classification follows `DATA_CLASSIFICATION.md`; the envelope kind caps
what may be constructed at all:

| kind | max plaintext sensitivity |
|---|---|
| `ORG_PROFILE`, `TEAM` | `PROJECT` |
| `MEMBERSHIP`, `ORG_POLICY`, `USAGE_SNAPSHOT` | `PRIVATE` |

`RESTRICTED` and `SECRET` content has no sync kind. Construction refuses
it with `SensitivityRefusal` **before** encryption, so a secret can never
exist inside a produced envelope (C-3, C-4 probe in TASK-0213). Secrets,
private keys, and provider credentials stay in the OS/vault layer exactly
as before (INV-008, INV-151); sync changes nothing about their storage.

### 5. Usage ledger and quota

`UsageEntry` (normative fields):

| field | contract |
|---|---|
| `id` | `use_` + 26-char Crockford Base32 |
| `org_id` / `account_id` | owning organization and account |
| `project_ref` | optional bounded text |
| `kind` | closed v1 enum: `EXECUTION`, `PLANNING`, `EVALUATION` |
| `units` | u64 provider-neutral units (tokens where applicable) |
| `cost_microusd` | u64 measured cost; estimates are never recorded as measured |
| `idempotency_key` | bounded text; a duplicate key returns the existing entry and appends nothing |
| `recorded_at` | Unix millis UTC |

The ledger is append-only and carries no content payloads (per
`DATA_CLASSIFICATION.md` category separation). Quota predicate: when
`quota_max_cost_microusd` is set, dispatch is refused **before** execution
iff `sum(cost in current window) + requested estimate > limit` (C-9).
Entries cannot be edited or deleted to evade quota; whole-data deletion
under INV-015 remains the only removal path. Billing in v1 is this local
ledger only (§8).

### 6. Local sovereignty

Local operation never requires a cloud account, connectivity, or sync
(INV-011, PRIV-009). Sync disabled, unreachable, or conflicted changes
nothing about local Task execution; there is no silent cloud fallback and
no degraded local mode (C-1). Offline work neither blocks on nor is
blocked by envelope state.

### 7. Threat model (C-1..C-10)

| ID | Threat | Normative rule | Verified by |
|---|---|---|---|
| C-1 | Cloud becomes a silent dependency or fallback | §6: local execution identical with sync off/absent | TASK-0212 offline check |
| C-2 | Relay/carrier reads synced content | §3: ciphertext + bounded metadata only; keys in vault | TASK-0212 round-trip inspection |
| C-3 | Secret or private key enters sync | §4: no kind admits `RESTRICTED`/`SECRET`; construction refuses pre-encryption | TASK-0213 secret-in-envelope probe |
| C-4 | Non-admin or self-escalation changes roles/policy | §1 authority matrix; no self role change; typed refusal | TASK-0210 refusal tests |
| C-5 | Revoked member keeps acting | §1: revocation durable, immediate; later mutations/envelopes refuse | TASK-0213 revoked-member case |
| C-6 | Tampered envelope is applied | §3: signature + AEAD verified before apply; nothing partial | TASK-0213 tamper case |
| C-7 | Stale writer overwrites newer state | §3: `base_revision` must equal head; `Conflict`, no auto-merge | TASK-0213 stale-revision case |
| C-8 | Lower layer weakens org hard policy | §2: most-restrictive resolution; weakening refuses, policy unchanged | TASK-0212/0213 weakening cases |
| C-9 | Spend evades quota via edit or retry | §5: append-only ledger, idempotent keys, pre-dispatch quota refusal | TASK-0213 over-quota case |
| C-10 | Envelope replay or impersonation | §3: signer bound to `ACTIVE` membership; unseen `id` required; unknown signer refuses | TASK-0213 matrix |

Residual risk: a fully compromised local device owns its vault keys and
is out of scope, as in THR-025. Compromise of one member never grants
another account's signing identity.

### 8. Deferred decisions and non-goals (explicit)

- **Real payment processor: deferred.** Requires its own ADR (processor
  selection, PCI scope, refund/dispute semantics). G13 v1 ships the local
  ledger and quota only; no payment API is designed here by implication.
- **Hosted control plane: deferred.** ADR-0002 (Go + PostgreSQL + object
  storage) remains the future direction, but no hosted service, account
  server, or enrollment endpoint is built in G13. It requires its own ADR
  and threat model before any implementation task.
- Also not G13 v1: key-distribution/recovery protocol, CRDT or automatic
  conflict merge, multi-organization federation, cloud accounts, remote
  fleet management (G12 workers stay as they are).

### 9. Storage and IPC

SQLite schema v37 (migration `0037`): `organizations`, `org_teams`,
`org_memberships`, `org_policies`, `org_sync_envelopes` (applied-envelope
log and stream head), `org_usage_entries`. New pure crates `org` and
`org-contract` (TASK-0208) hold the domain and shared conformance suite;
`adapters/storage-sqlite` implements the store port (TASK-0209);
`OrgService` in `crates/app-services` is the only mutation authority
(TASK-0210). Additive IPC V12 with `org_core = 15`; sessions below V12 get
a typed refusal. `agentctl org create|team|member|policy|sync|usage` is
the operator surface (TASK-0211); IPC stays a thin control surface and
implements no second copy of org semantics (INV-094).

### 10. Error taxonomy

`InvalidInput`, `UnknownKind`, `UnknownRole`, `ForbiddenRole`,
`SelfRoleChange`, `RevokedMember`, `UnknownSigner`, `BadSignature`,
`TamperedEnvelope`, `Conflict`, `DuplicateEnvelope`, `SensitivityRefusal`,
`PolicyWeakening`, `OverQuota`, `NotFound`, `BackendFailure`. Errors are
seam-local and typed; no panics on input.

## Alternatives considered

- Cloud-authoritative organizations (hosted control plane as source of
  truth): rejected; violates INV-011 and §6. The deferred hosted plane
  (§8) may carry envelopes later, never own local facts.
- CRDT / automatic conflict merge: rejected for v1; a linear revision
  chain with explicit rebase keeps conflicts operator-visible and
  auditable. Merge semantics need measured need and their own ADR.
- Shared-secret (HMAC) envelope authenticity: rejected for the same
  reason as ADR-0046 — a leaked secret impersonates forever; ed25519
  signing binds envelopes to enrolled account fingerprints.
- Syncing everything including audit and secrets "encrypted, therefore
  safe": rejected; classification is enforced at construction (§4), so
  forbidden content never becomes ciphertext in the first place.
- Org policy as a second authorization engine beside the Policy Engine:
  rejected; org policy is a managed input layer to the one canonical
  decision point (INV-146).

## Consequences

Two local stores can exchange organization state as verifiable encrypted
envelopes; an organization can hard-cap capability, sync sensitivity, and
spend for every member; revocation and quota act locally and immediately.
TASK-0208 can implement the domain without inventing identity, policy,
conflict, or quota semantics.

## Security impact

First off-device data flow in the product, and it is opt-in, encrypted,
signed, classification-gated, and locally enforceable. Canonical
authorization stays in the Policy Engine; org policy only narrows it.
Private keys never leave the device vault; a carrier compromise yields
ciphertext and bounded metadata. Policy weakening, replay, tampering, and
stale writes all fail closed with typed refusals.

## Migration impact

Additive: schema v37 adds six tables; IPC V12 adds one envelope; two new
pure crates. Installations that never create an organization behave
identically. Nothing in G12 worker identity changes; account signing
reuses its key discipline without sharing its keys.

## References

- [Roadmap G13](../../planning/roadmap.yaml), [PROJECT_CHARTER.md](../../PROJECT_CHARTER.md)
  (local-first, opt-in cloud), source-of-truth §§81–82;
  INV-008, INV-011, INV-014, INV-025, INV-058, INV-070, INV-094,
  INV-146, INV-148, INV-151.
- [ADR-0002](./ADR-0002-cloud-control-plane-go.md) (deferred hosted plane),
  [ADR-0014](./ADR-0014-local-first-privacy.md),
  [ADR-0045](./ADR-0045-g11-background-daemon-contract.md) (capability enum),
  [ADR-0046](./ADR-0046-g12-remote-worker-contract.md) (key discipline).
- [POLICY_CONTRACT.md](../security/POLICY_CONTRACT.md) (restrictive layer
  composition, managed-layer insertion),
  [DATA_CLASSIFICATION.md](../security/DATA_CLASSIFICATION.md),
  [THREAT_MODEL.md](../security/THREAT_MODEL.md).
