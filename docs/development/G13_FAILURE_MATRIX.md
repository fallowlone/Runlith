# G13 Failure Matrix and Dogfood (TASK-0213)

> Failure-matrix verification for G13 (Cloud/Team), ADR-0047 threat
> model C-3, C-5..C-10. Tests only; no production code. Shipped-binary
> two-daemon dogfood was delivered by TASK-0212 and re-verified here at
> service level between two independent SQLite stores.

## Matrix

| Case (ADR-0047) | Test | Result |
|---|---|---|
| C-5 revoked member, durable | `revoked_member_refuses_after_store_reopen` | Mutations, authoring and usage refuse `RevokedMember` after store reopen; head stays 0 |
| C-6 tampered envelope | `tampered_envelope_variants_apply_nothing` | Flipped ciphertext → `BadSignature`; wrong org key → `TamperedEnvelope`; nothing stored, head 0; valid envelope still accepts |
| C-7 stale revision | `stale_revision_conflict_applies_nothing` | Out-of-order revision 2 → `Conflict`, head 0; in-order 1, 2 accepted |
| C-8 policy weakening | `policy_weakening_attempts_leave_policy_unchanged` | Capability, sync-sensitivity and quota weakening each refuse `PolicyWeakening`; policy stays v1; narrowing applies |
| C-3 secret in envelope | `secret_in_envelope_probe_refuses_before_encryption` | Every kind × `RESTRICTED`/`SECRET` refuses `SensitivityRefusal` pre-encryption; `PRIVATE` into `PROJECT`-capped kind refuses; head 0 |
| C-2 carrier blindness | same test | Sealed ciphertext contains no plaintext bytes |
| C-9 over-quota + retry evasion | `over_quota_and_idempotency_evasion_refuse` | Same-key retry errors and appends nothing; 900+200 → `OverQuota`, 900+100 admits, full ledger blocks 1; out-of-window spend exempt |
| C-10 replay / impersonation | `replay_and_impersonation_refuse` | Replay → `DuplicateEnvelope`; member id signed with attacker secret → `BadSignature`; head unchanged |

## Dogfood

`two_store_dogfood_round_trip_survives_receiver_reopen`: envelope
sealed on store A, accepted on independently seeded store B with exact
plaintext, receiver reopened — head 1 and the stored envelope are
durable. Shipped-binary form: TASK-0212
`org_sync_policy_quota_take_effect_between_two_daemon_stores`
(two `agentd` processes on loopback, restart durability, C-1 offline
execution unaffected).

## Run

```sh
cargo test --locked -p app-services --test org_failure_matrix --test org_service
# org_failure_matrix 8/8, org_service 8/8
```
