use app_services::org_service::{OrgService, PolicyRequest, UsageRequest};
use org::model::{
    keypair_from_secret, CapabilityCeiling, EnvelopeKind, LayeredPolicy, OrgError, Role,
    Sensitivity, UsageKind,
};
use org::port::OrgStorePort;
use storage_sqlite::SqliteStore;

fn fresh() -> (OrgService<SqliteStore>, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static C: AtomicU64 = AtomicU64::new(0);
    let db = std::env::temp_dir().join(format!(
        "runlith-osvc-{}-{}.sqlite",
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

/// Org with owner + one MEMBER; returns (org, owner_membership, member, member_secret).
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

#[test]
fn create_org_team_membership_and_role_flow() {
    let (mut svc, db) = fresh();
    let (org, owner, member, _) = org_with_member(&mut svc);
    assert_eq!(owner.role, Role::Owner);

    let team = svc.create_team(&owner.id, &org.id, "core", 1_200).unwrap();
    assert_eq!(svc.list_teams(&org.id).unwrap(), vec![team]);

    let promoted = svc.change_role(&owner.id, &member.id, Role::Admin).unwrap();
    assert_eq!(promoted.role, Role::Admin);
    // The new ADMIN may administer teams.
    svc.create_team(&promoted.id, &org.id, "ops", 1_300)
        .unwrap();
    assert_eq!(svc.list_memberships(&org.id).unwrap().len(), 2);
    std::fs::remove_file(db).ok();
}

#[test]
fn non_admin_and_self_escalation_refuse() {
    let (mut svc, db) = fresh();
    let (org, owner, member, _) = org_with_member(&mut svc);
    let (other_acc, _) = account(&mut svc, 9);

    // MEMBER cannot create teams, add members, or write policy (C-4).
    assert!(matches!(
        svc.create_team(&member.id, &org.id, "nope", 1_200),
        Err(OrgError::ForbiddenRole(_))
    ));
    assert!(matches!(
        svc.add_member(
            &member.id,
            &org.id,
            &other_acc.id,
            None,
            Role::Viewer,
            1_200
        ),
        Err(OrgError::ForbiddenRole(_))
    ));
    assert!(matches!(
        svc.write_policy(&member.id, &org.id, policy_req(None), 1_200),
        Err(OrgError::ForbiddenRole(_))
    ));
    // Nobody changes their own role.
    assert!(matches!(
        svc.change_role(&owner.id, &owner.id, Role::Admin),
        Err(OrgError::SelfRoleChange(_))
    ));
    // ADMIN cannot grant the OWNER role.
    let admin = svc.change_role(&owner.id, &member.id, Role::Admin).unwrap();
    assert!(matches!(
        svc.add_member(&admin.id, &org.id, &other_acc.id, None, Role::Owner, 1_300),
        Err(OrgError::ForbiddenRole(_))
    ));
    assert!(matches!(
        svc.change_role(&admin.id, &admin.id, Role::Owner),
        Err(OrgError::SelfRoleChange(_))
    ));
    std::fs::remove_file(db).ok();
}

#[test]
fn revoked_member_refuses_mutations_and_authoring() {
    let (mut svc, db) = fresh();
    let (org, owner, member, secret) = org_with_member(&mut svc);
    svc.revoke_member(&owner.id, &member.id).unwrap();

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
            &[5u8; 32],
            &secret,
            [1u8; 24],
            1_200,
        ),
        Err(OrgError::RevokedMember(_))
    ));
    std::fs::remove_file(db).ok();
}

#[test]
fn policy_resolution_weakening_refuses_and_narrowing_applies() {
    let (mut svc, db) = fresh();
    let (org, owner, _, _) = org_with_member(&mut svc);
    let v1 = svc
        .write_policy(&owner.id, &org.id, policy_req(Some(1_000)), 1_200)
        .unwrap();
    assert_eq!(v1.version, 1);
    let v2 = svc
        .write_policy(&owner.id, &org.id, policy_req(Some(500)), 1_300)
        .unwrap();
    assert_eq!(v2.version, 2);
    assert_eq!(svc.current_policy(&org.id).unwrap().version, 2);

    // A lower layer may narrow but never weaken the org hard policy (C-8).
    let narrowed = svc
        .resolve_policy(
            &org.id,
            &[LayeredPolicy {
                capability_ceiling: Some(CapabilityCeiling::ReadOnly),
                sync_max_sensitivity: None,
                quota_max_cost_microusd: Some(250),
            }],
        )
        .unwrap();
    assert_eq!(narrowed.capability_ceiling, CapabilityCeiling::ReadOnly);
    assert_eq!(narrowed.quota_max_cost_microusd, Some(250));
    assert!(matches!(
        svc.resolve_policy(
            &org.id,
            &[LayeredPolicy {
                capability_ceiling: Some(CapabilityCeiling::Execution),
                sync_max_sensitivity: None,
                quota_max_cost_microusd: None,
            }],
        ),
        Err(OrgError::PolicyWeakening(_))
    ));
    std::fs::remove_file(db).ok();
}

#[test]
fn usage_quota_blocks_over_limit_dispatch() {
    let (mut svc, db) = fresh();
    let (org, owner, member, _) = org_with_member(&mut svc);
    svc.write_policy(&owner.id, &org.id, policy_req(Some(1_000)), 1_200)
        .unwrap();
    svc.record_usage(
        &org.id,
        UsageRequest {
            account_id: member.account_id.clone(),
            project_ref: None,
            kind: UsageKind::Execution,
            units: 10,
            cost_microusd: 900,
            idempotency_key: "k1".into(),
        },
        50_000,
    )
    .unwrap();
    // 900 spent + 200 estimate exceeds the 1000 limit before dispatch (C-9).
    assert!(matches!(
        svc.check_quota(&org.id, 200, 50_000),
        Err(OrgError::OverQuota(_))
    ));
    svc.check_quota(&org.id, 100, 50_000).unwrap();
    assert_eq!(svc.list_usage(&org.id).unwrap().len(), 1);
    std::fs::remove_file(db).ok();
}

/// Seeds a second store with the same org facts (two-store sync setup).
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

#[test]
fn envelope_round_trip_between_two_stores() {
    let (mut sender, db1) = fresh();
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();

    let envelope = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"org-profile-v1",
            &[5u8; 32],
            &secret,
            [1u8; 24],
            2_000,
        )
        .unwrap();
    assert_eq!(sender.sync_head(&org.id).unwrap(), 1);

    let (mut receiver, db2) = fresh();
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account, member_account],
        &[owner.clone(), member.clone()],
    );
    let plaintext = receiver.accept_envelope(&envelope, &[5u8; 32]).unwrap();
    assert_eq!(plaintext, b"org-profile-v1".to_vec());
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 1);
    // Replay refuses (C-10).
    assert!(matches!(
        receiver.accept_envelope(&envelope, &[5u8; 32]),
        Err(OrgError::DuplicateEnvelope(_))
    ));
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}

#[test]
fn envelope_tamper_stale_and_revoked_author_refuse() {
    let (mut sender, db1) = fresh();
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();
    let envelope = sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"org-profile-v1",
            &[5u8; 32],
            &secret,
            [1u8; 24],
            2_000,
        )
        .unwrap();

    let (mut receiver, db2) = fresh();
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account.clone(), member_account.clone()],
        &[owner.clone(), member.clone()],
    );
    // Tampered ciphertext refuses and applies nothing (C-6).
    let mut tampered = envelope.clone();
    tampered.ciphertext[0] ^= 1;
    assert!(matches!(
        receiver.accept_envelope(&tampered, &[5u8; 32]),
        Err(OrgError::BadSignature(_))
    ));
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 0);

    // Revoked author's envelope refuses (C-5).
    receiver.revoke_member(&owner.id, &member.id).unwrap();
    assert!(matches!(
        receiver.accept_envelope(&envelope, &[5u8; 32]),
        Err(OrgError::RevokedMember(_))
    ));
    assert_eq!(receiver.sync_head(&org.id).unwrap(), 0);
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}

#[test]
fn envelope_stale_revision_conflicts() {
    let (mut sender, db1) = fresh();
    let (org, owner, member, secret) = org_with_member(&mut sender);
    let owner_account = sender.get_account(&owner.account_id).unwrap();
    let member_account = sender.get_account(&member.account_id).unwrap();
    sender
        .author_envelope(
            &member.id,
            EnvelopeKind::OrgProfile,
            Sensitivity::Project,
            b"v1",
            &[5u8; 32],
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
            &[5u8; 32],
            &secret,
            [2u8; 24],
            2_100,
        )
        .unwrap();

    let (mut receiver, db2) = fresh();
    seed_receiver(
        &mut receiver,
        &org,
        &[owner_account, member_account],
        &[owner, member],
    );
    // base_revision 1 against head 0 is a stale writer (C-7).
    assert!(matches!(
        receiver.accept_envelope(&second, &[5u8; 32]),
        Err(OrgError::Conflict(_))
    ));
    std::fs::remove_file(db1).ok();
    std::fs::remove_file(db2).ok();
}
