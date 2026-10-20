//! TASK-0213 failure matrix (ADR-0047 C-3, C-5..C-10): revoked member,
//! tampered envelope, stale-revision conflict, policy-weakening attempt,
//! secret-in-envelope probe, over-quota dispatch. Every refusal must
//! apply nothing: heads, policies and ledgers stay byte-identical.
//! Dogfood: two independent SQLite stores exchange one envelope and the
//! receiver reopens before reading it back.
use app_services::org_service::{OrgService, PolicyRequest, UsageRequest};
use org::model::{
    keypair_from_secret, CapabilityCeiling, EnvelopeKind, LayeredPolicy, OrgError, Role,
    Sensitivity, UsageKind,
};
use org::port::OrgStorePort;
use storage_sqlite::SqliteStore;

fn fresh(tag: &str) -> (OrgService<SqliteStore>, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    let db = std::env::temp_dir().join(format!(
        "runlith-orgfail-{tag}-{}-{}.sqlite",
        std::process::id(),
        C.fetch_add(1, Ordering::SeqCst)
    ));
    (OrgService::new(SqliteStore::open(&db).unwrap()), db)
}

fn account(svc: &mut OrgService<SqliteStore>, seed: u8) -> (org::model::AccountRecord, [u8; 32]) {
    let (secret, public) = keypair_from_secret(&[seed; 32]).unwrap();
    (
        svc.register_account("operator", &public, 1_000).unwrap(),
        secret,
    )
}

fn policy_req(quota: Option<u64>) -> PolicyRequest {
    PolicyRequest {
        hard_capability_ceiling: CapabilityCeiling::WorkspaceWrite,
        sync_max_sensitivity: Sensitivity::Private,
        quota_max_cost_microusd: quota,
        quota_window_seconds: 86_400,
    }
}

fn org_with_member(
    svc: &mut OrgService<SqliteStore>,
) -> (
    org::model::OrganizationRecord,
    org::model::MembershipRecord,
    org::model::MembershipRecord,
    [u8; 32],
) {
    let (owner_acc, _) = account(svc, 7);
    let (member_acc, member_secret) = account(svc, 8);
    let (org, owner) = svc
        .create_organization(&owner_acc.id, "acme", 1_000)
        .unwrap();
    let member = svc
        .add_member(
            &owner.id,
            &org.id,
            &member_acc.id,
            None,
            Role::Member,
            1_100,
        )
        .unwrap();
    (org, owner, member, member_secret)
}

fn seed_receiver(
    receiver: &mut OrgService<SqliteStore>,
    org: &org::model::OrganizationRecord,
    accounts: &[org::model::AccountRecord],
    memberships: &[org::model::MembershipRecord],
) {
    let store = receiver.store_mut();
    for account in accounts {
        store.put_account(account).unwrap();
    }
    store.put_organization(org).unwrap();
    for membership in memberships {
        store.put_membership(membership).unwrap();
    }
}

const ORG_KEY: [u8; 32] = [5u8; 32];

#[test]
fn revoked_member_refuses_after_store_reopen() {
    let (mut svc, db) = fresh("revoked");
    let (org, owner, member, secret) = org_with_member(&mut svc);
    svc.revoke_member(&owner.id, &member.id).unwrap();
    drop(svc);

    // Revocation is durable: a reopened store still refuses (C-5).
    let mut svc = OrgService::new(SqliteStore::open(&db).unwrap());
    assert!(matches!(
        svc.create_team(&member.id, &org.id, "nope", 1_200),
        Err(OrgError::RevokedMember(_))
    ));
    assert!(matches!(
        svc.author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"profile",
            &ORG_KEY,
            &secret,
            [1u8; 24],
            1_200,
        ),
        Err(OrgError::RevokedMember(_))
    ));
    assert!(matches!(
        svc.record_usage(
            &org.id,
            UsageRequest {
                account_id: member.account_id.clone(),
                project_ref: None,
                kind: UsageKind::Execution,
                units: 1,
                cost_microusd: 1,
                idempotency_key: "revoked-k".into(),
            },
            1_200,
        ),
        Err(OrgError::RevokedMember(_))
    ));
    assert_eq!(svc.sync_head(&org.id).unwrap(), 0);
    std::fs::remove_file(db).ok();
}

#[test]
fn tampered_envelope_variants_apply_nothing() {
    let (mut sender, db1) = fresh("tamper-a");
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();
    let envelope = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"org-profile-v1",
            &ORG_KEY,
            &secret,
            [1u8; 24],
            2_000,
        )
        .unwrap();

    let (mut receiver, db2) = fresh("tamper-b");
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account, member_account],
        &[owner, member],
    );

    // Flipped ciphertext byte: signature no longer verifies (C-6).
    let mut flipped = envelope.clone();
    flipped.ciphertext[0] ^= 1;
    assert!(matches!(
        receiver.accept_envelope(&flipped, &ORG_KEY),
        Err(OrgError::BadSignature(_))
    ));
    // Wrong org key: AEAD open fails as tampering, nothing partial.
    assert!(matches!(
        receiver.accept_envelope(&envelope, &[9u8; 32]),
        Err(OrgError::TamperedEnvelope(_))
    ));
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 0);
    assert!(matches!(
        receiver.envelope(&envelope.id),
        Err(OrgError::NotFound(_))
    ));
    // The untampered envelope still accepts afterwards.
    assert_eq!(
        receiver.accept_envelope(&envelope, &ORG_KEY).unwrap(),
        b"org-profile-v1".to_vec()
    );
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 1);
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}

#[test]
fn stale_revision_conflict_applies_nothing() {
    let (mut sender, db1) = fresh("stale-a");
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();
    let first = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"v1",
            &ORG_KEY,
            &secret,
            [1u8; 24],
            2_000,
        )
        .unwrap();
    let second = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"v2",
            &ORG_KEY,
            &secret,
            [2u8; 24],
            2_100,
        )
        .unwrap();

    let (mut receiver, db2) = fresh("stale-b");
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account, member_account],
        &[owner, member],
    );
    // Out-of-order delivery: revision 2 against head 0 refuses (C-7).
    assert!(matches!(
        receiver.accept_envelope(&second, &ORG_KEY),
        Err(OrgError::Conflict(_))
    ));
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 0);
    // In-order delivery succeeds; the stale writer cannot overwrite.
    receiver.accept_envelope(&first, &ORG_KEY).unwrap();
    receiver.accept_envelope(&second, &ORG_KEY).unwrap();
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 2);
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}

#[test]
fn policy_weakening_attempts_leave_policy_unchanged() {
    let (mut svc, db) = fresh("weaken");
    let (org, owner, _, _) = org_with_member(&mut svc);
    let v1 = svc
        .write_policy(&owner.id, &org.id, policy_req(Some(1_000)), 1_200)
        .unwrap();

    // Each knob weakened in isolation refuses; policy stays at v1 (C-8).
    for layers in [
        LayeredPolicy {
            capability_ceiling: Some(CapabilityCeiling::Execution),
            sync_max_sensitivity: None,
            quota_max_cost_microusd: None,
        },
        LayeredPolicy {
            capability_ceiling: None,
            sync_max_sensitivity: Some(Sensitivity::Restricted),
            quota_max_cost_microusd: None,
        },
        LayeredPolicy {
            capability_ceiling: None,
            sync_max_sensitivity: None,
            quota_max_cost_microusd: Some(2_000),
        },
    ] {
        assert!(matches!(
            svc.resolve_policy(&org.id, &[layers]),
            Err(OrgError::PolicyWeakening(_))
        ));
    }
    let current = svc.current_policy(&org.id).unwrap();
    assert_eq!(current.version, v1.version);
    assert_eq!(current.quota_max_cost_microusd, Some(1_000));
    // Narrowing on every knob still applies.
    let narrowed = svc
        .resolve_policy(
            &org.id,
            &[LayeredPolicy {
                capability_ceiling: Some(CapabilityCeiling::ReadOnly),
                sync_max_sensitivity: Some(Sensitivity::Project),
                quota_max_cost_microusd: Some(250),
            }],
        )
        .unwrap();
    assert_eq!(narrowed.capability_ceiling, CapabilityCeiling::ReadOnly);
    assert_eq!(narrowed.sync_max_sensitivity, Sensitivity::Project);
    std::fs::remove_file(db).ok();
}

#[test]
fn secret_in_envelope_probe_refuses_before_encryption() {
    let (mut svc, db) = fresh("secret");
    let (org, _owner, member, secret) = org_with_member(&mut svc);

    // No kind admits RESTRICTED or SECRET (C-3): construction refuses
    // before encryption, so no envelope can ever carry a secret.
    for kind in [
        EnvelopeKind::OrgProfile,
        EnvelopeKind::Team,
        EnvelopeKind::Membership,
        EnvelopeKind::OrgPolicy,
        EnvelopeKind::UsageSnapshot,
    ] {
        for sensitivity in [Sensitivity::Restricted, Sensitivity::Secret] {
            assert!(
                matches!(
                    svc.author_envelope(
                        &member.id,
                        kind,
                        sensitivity,
                        b"vault-material",
                        &ORG_KEY,
                        &secret,
                        [1u8; 24],
                        2_000,
                    ),
                    Err(OrgError::SensitivityRefusal(_))
                ),
                "{kind:?} at {sensitivity:?} must refuse"
            );
        }
    }
    // PRIVATE into a PROJECT-capped kind also refuses pre-encryption.
    assert!(matches!(
        svc.author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Private,
            b"private-profile",
            &ORG_KEY,
            &secret,
            [1u8; 24],
            2_000,
        ),
        Err(OrgError::SensitivityRefusal(_))
    ));
    assert_eq!(svc.sync_head(&org.id).unwrap(), 0);

    // Carrier blindness (C-2): a valid envelope holds no plaintext bytes.
    let envelope = svc
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"org-profile-v1",
            &ORG_KEY,
            &secret,
            [3u8; 24],
            2_100,
        )
        .unwrap();
    assert!(!envelope
        .ciphertext
        .windows(b"org-profile-v1".len())
        .any(|w| w == b"org-profile-v1"));
    std::fs::remove_file(db).ok();
}

#[test]
fn over_quota_and_idempotency_evasion_refuse() {
    let (mut svc, db) = fresh("quota");
    let (org, owner, member, _) = org_with_member(&mut svc);
    svc.write_policy(&owner.id, &org.id, policy_req(Some(1_000)), 1_200)
        .unwrap();
    let usage = |key: &str, cost: u64| UsageRequest {
        account_id: member.account_id.clone(),
        project_ref: None,
        kind: UsageKind::Execution,
        units: 10,
        cost_microusd: cost,
        idempotency_key: key.into(),
    };
    svc.record_usage(&org.id, usage("k1", 900), 50_000).unwrap();
    // Retry with the same key cannot double-count or evade (C-9).
    assert!(svc.record_usage(&org.id, usage("k1", 900), 50_000).is_err());
    assert_eq!(svc.list_usage(&org.id).unwrap().len(), 1);
    // Over-limit estimate refuses before dispatch; exact-fit admits.
    assert!(matches!(
        svc.check_quota(&org.id, 200, 50_000),
        Err(OrgError::OverQuota(_))
    ));
    svc.check_quota(&org.id, 100, 50_000).unwrap();
    svc.record_usage(&org.id, usage("k2", 100), 50_100).unwrap();
    assert!(matches!(
        svc.check_quota(&org.id, 1, 50_100),
        Err(OrgError::OverQuota(_))
    ));
    // Outside the quota window the spend no longer counts.
    svc.check_quota(&org.id, 1_000, 50_000 + 86_400_000)
        .unwrap();
    std::fs::remove_file(db).ok();
}

#[test]
fn replay_and_impersonation_refuse() {
    let (mut sender, db1) = fresh("replay-a");
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();
    let envelope = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"org-profile-v1",
            &ORG_KEY,
            &secret,
            [1u8; 24],
            2_000,
        )
        .unwrap();

    let (mut receiver, db2) = fresh("replay-b");
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account.clone(), member_account.clone()],
        &[owner.clone(), member.clone()],
    );
    receiver.accept_envelope(&envelope, &ORG_KEY).unwrap();
    // Replay of the same envelope id refuses (C-10).
    assert!(matches!(
        receiver.accept_envelope(&envelope, &ORG_KEY),
        Err(OrgError::DuplicateEnvelope(_))
    ));

    // Impersonation: member's id signed with an attacker's secret.
    let (attacker_secret, _) = keypair_from_secret(&[99u8; 32]).unwrap();
    let forged = org::model::SyncEnvelope::seal(org::model::SealRequest {
        id: org::mint::mint_envelope_id(3_000, 7),
        org_id: org.id.clone(),
        author: &member_account,
        author_secret: &attacker_secret,
        org_key: &ORG_KEY,
        nonce: [7u8; 24],
        kind: EnvelopeKind::OrgProfile,
        revision: 2,
        base_revision: 1,
        max_sensitivity: Sensitivity::Project,
        plaintext: b"forged",
        created_at: 3_000,
    })
    .unwrap();
    assert!(matches!(
        receiver.accept_envelope(&forged, &ORG_KEY),
        Err(OrgError::BadSignature(_))
    ));
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 1);
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}

#[test]
fn two_store_dogfood_round_trip_survives_receiver_reopen() {
    let (mut sender, db1) = fresh("dogfood-a");
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();
    let envelope = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"dogfood-profile",
            &ORG_KEY,
            &secret,
            [4u8; 24],
            2_000,
        )
        .unwrap();

    let (mut receiver, db2) = fresh("dogfood-b");
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account, member_account],
        &[owner, member],
    );
    // Local work on the receiver is unaffected before any sync (C-1).
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 0);
    assert_eq!(
        receiver.accept_envelope(&envelope, &ORG_KEY).unwrap(),
        b"dogfood-profile".to_vec()
    );
    drop(receiver);

    let receiver = OrgService::new(SqliteStore::open(&db2).unwrap());
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 1);
    assert_eq!(receiver.envelope(&envelope.id).unwrap(), envelope);
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}
