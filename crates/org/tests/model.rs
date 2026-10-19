use org::mint::{
    mint_account_id, mint_envelope_id, mint_membership_id, mint_organization_id, mint_policy_id,
    mint_team_id, mint_usage_id,
};
use org::model::{
    keypair_from_secret, AcceptContext, AccountRecord, CapabilityCeiling, EnvelopeKind,
    LayeredPolicy, MembershipRecord, OrgError, OrgPolicy, OrganizationRecord, Role, SealRequest,
    Sensitivity, SyncEnvelope, TeamRecord, UsageEntry, UsageKind,
};

const ORG_KEY: [u8; 32] = [77u8; 32];

fn account(seq: u128, seed: u8) -> (AccountRecord, [u8; 32]) {
    let (secret, public) = keypair_from_secret(&[seed; 32]).unwrap();
    (
        AccountRecord::register(mint_account_id(1_000, seq), "operator", &public, 1_000).unwrap(),
        secret,
    )
}

fn org() -> OrganizationRecord {
    OrganizationRecord::create(mint_organization_id(1_000, 1), "acme", 1_000).unwrap()
}

fn membership(
    seq: u128,
    org_id: &org::model::OrganizationId,
    account_id: &org::model::AccountId,
    role: Role,
) -> MembershipRecord {
    MembershipRecord::grant(
        mint_membership_id(1_000, seq),
        org_id.clone(),
        account_id.clone(),
        None,
        role,
        1_000,
    )
    .unwrap()
}

fn policy(org_id: &org::model::OrganizationId, by: &org::model::AccountId) -> OrgPolicy {
    OrgPolicy::new(
        mint_policy_id(1_000, 1),
        org_id.clone(),
        CapabilityCeiling::WorkspaceWrite,
        Sensitivity::Private,
        Some(1_000_000),
        86_400,
        by.clone(),
        1_000,
    )
    .unwrap()
}

#[test]
fn identity_records_validate_and_fingerprint_binds() {
    let (acct, _) = account(1, 7);
    let (_, public) = keypair_from_secret(&[7u8; 32]).unwrap();
    assert_eq!(acct.fingerprint, org::model::fingerprint(&public));
    let o = org();
    let team = TeamRecord::create(mint_team_id(1_000, 1), o.id.clone(), "core", 1_000).unwrap();
    assert_eq!(team.org_id, o.id);
    assert!(AccountRecord::register(mint_account_id(1_000, 9), "", &public, 1_000).is_err());
    assert!(OrganizationRecord::create(mint_organization_id(1_000, 2), "", 1_000).is_err());
}

#[test]
fn role_authority_refuses_non_admin_self_and_owner_escalation() {
    let o = org();
    let (owner_acct, _) = account(1, 1);
    let (admin_acct, _) = account(2, 2);
    let (member_acct, _) = account(3, 3);
    let owner = membership(1, &o.id, &owner_acct.id, Role::Owner);
    let admin = membership(2, &o.id, &admin_acct.id, Role::Admin);
    let mut member = membership(3, &o.id, &member_acct.id, Role::Member);

    // MEMBER cannot change another member's role (C-4).
    let (other_acct, _) = account(4, 4);
    let other_member = membership(4, &o.id, &other_acct.id, Role::Member);
    let mut target = member.clone();
    assert!(matches!(
        target.change_role(&other_member, Role::Viewer),
        Err(OrgError::ForbiddenRole(_))
    ));
    // Nobody changes their own role.
    assert!(matches!(
        member.change_role(&member.clone(), Role::Admin),
        Err(OrgError::SelfRoleChange(_))
    ));
    // VIEWER cannot author envelopes; MEMBER can.
    let viewer = membership(5, &o.id, &member_acct.id, Role::Viewer);
    assert!(viewer.ensure_can_author_envelope().is_err());
    assert!(member.ensure_can_author_envelope().is_ok());
    // ADMIN cannot grant OWNER; OWNER can.
    assert!(member.change_role(&admin, Role::Viewer).is_ok());
    let mut target2 = member.clone();
    assert!(target2.change_role(&admin, Role::Owner).is_err());
    assert!(target2.change_role(&owner, Role::Owner).is_ok());
}

#[test]
fn revocation_is_terminal_and_blocks_authoring() {
    let o = org();
    let (owner_acct, _) = account(1, 1);
    let (member_acct, _) = account(3, 3);
    let owner = membership(1, &o.id, &owner_acct.id, Role::Owner);
    let mut member = membership(3, &o.id, &member_acct.id, Role::Member);
    member.revoke(&owner).unwrap();
    assert!(!member.is_active());
    assert!(matches!(
        member.ensure_can_author_envelope(),
        Err(OrgError::RevokedMember(_))
    ));
    assert!(member.revoke(&owner).is_err(), "revocation is terminal");
}

#[test]
fn policy_resolution_is_most_restrictive_and_weakening_refuses() {
    let o = org();
    let (owner_acct, _) = account(1, 1);
    let pol = policy(&o.id, &owner_acct.id);

    // A narrower project layer composes.
    let narrow = LayeredPolicy {
        capability_ceiling: Some(CapabilityCeiling::ReadOnly),
        sync_max_sensitivity: None,
        quota_max_cost_microusd: Some(500_000),
    };
    let eff = org::model::resolve_effective(&pol, &[narrow]).unwrap();
    assert_eq!(eff.capability_ceiling, CapabilityCeiling::ReadOnly);
    assert_eq!(eff.sync_max_sensitivity, Sensitivity::Private);
    assert_eq!(eff.quota_max_cost_microusd, Some(500_000));

    // Any layer above the org hard ceiling refuses; nothing is capped silently (C-8).
    let widen = LayeredPolicy {
        capability_ceiling: Some(CapabilityCeiling::Execution),
        sync_max_sensitivity: None,
        quota_max_cost_microusd: None,
    };
    assert!(matches!(
        org::model::resolve_effective(&pol, &[widen]),
        Err(OrgError::PolicyWeakening(_))
    ));
    let widen_sync = LayeredPolicy {
        capability_ceiling: None,
        sync_max_sensitivity: Some(Sensitivity::Restricted),
        quota_max_cost_microusd: None,
    };
    assert!(matches!(
        org::model::resolve_effective(&pol, &[widen_sync]),
        Err(OrgError::PolicyWeakening(_))
    ));
}

#[test]
fn quota_blocks_over_limit_and_window_sum() {
    let o = org();
    let (owner_acct, _) = account(1, 1);
    let pol = policy(&o.id, &owner_acct.id);
    let eff = org::model::resolve_effective(&pol, &[]).unwrap();
    let e1 = UsageEntry::record(
        mint_usage_id(1_000, 1),
        o.id.clone(),
        owner_acct.id.clone(),
        None,
        UsageKind::Execution,
        10,
        900_000,
        "k1",
        50_000,
    )
    .unwrap();
    let spent = org::model::sum_cost_in_window(std::slice::from_ref(&e1), &o.id, 50_000, 86_400);
    assert_eq!(spent, 900_000);
    assert!(eff.ensure_quota(spent, 50_000).is_ok());
    assert!(matches!(
        eff.ensure_quota(spent, 200_000),
        Err(OrgError::OverQuota(_))
    ));
    // Entries outside the window do not count.
    let old = UsageEntry::record(
        mint_usage_id(1_000, 2),
        o.id.clone(),
        owner_acct.id.clone(),
        None,
        UsageKind::Execution,
        10,
        900_000,
        "k0",
        1,
    )
    .unwrap();
    assert_eq!(
        org::model::sum_cost_in_window(&[old], &o.id, 50_000, 1_000),
        0
    );
}

#[test]
fn envelope_seal_accept_roundtrip_and_all_refusals() {
    let o = org();
    let (owner_acct, owner_secret) = account(1, 1);
    let owner = membership(1, &o.id, &owner_acct.id, Role::Owner);
    let seal =
        |id_seq: u128, base: u64, rev: u64, kind: EnvelopeKind, sens: Sensitivity, pt: &[u8]| {
            SyncEnvelope::seal(SealRequest {
                id: mint_envelope_id(1_000, id_seq),
                org_id: o.id.clone(),
                author: &owner_acct,
                author_secret: &owner_secret,
                org_key: &ORG_KEY,
                nonce: [9u8; 24],
                kind,
                revision: rev,
                base_revision: base,
                max_sensitivity: sens,
                plaintext: pt,
                created_at: 2_000,
            })
        };
    let env = seal(
        1,
        0,
        1,
        EnvelopeKind::OrgProfile,
        Sensitivity::Project,
        b"hello",
    )
    .unwrap();
    let ctx = AcceptContext {
        expected_org: &o.id,
        author_account: &owner_acct,
        author_membership: &owner,
        current_head: 0,
        already_seen: false,
        org_key: &ORG_KEY,
    };
    assert_eq!(env.accept(&ctx).unwrap(), b"hello".to_vec());

    // SECRET plaintext refuses at construction, before encryption (C-3).
    assert!(matches!(
        seal(2, 0, 1, EnvelopeKind::OrgProfile, Sensitivity::Secret, b"x"),
        Err(OrgError::SensitivityRefusal(_))
    ));
    // Stale base refuses as Conflict (C-7); revision must be base+1.
    assert!(matches!(
        env.accept(&AcceptContext {
            current_head: 5,
            ..ctx
        }),
        Err(OrgError::Conflict(_))
    ));
    // Replay refuses (C-10).
    assert!(matches!(
        env.accept(&AcceptContext {
            already_seen: true,
            ..ctx
        }),
        Err(OrgError::DuplicateEnvelope(_))
    ));
    // Tampered ciphertext refuses; nothing is returned (C-6).
    let mut bad = env.clone();
    bad.ciphertext[0] ^= 0xFF;
    assert!(bad.accept(&ctx).is_err());
    assert!(bad.open(&ORG_KEY).is_err());
    // Wrong org key refuses.
    assert!(env.open(&[1u8; 32]).is_err());
    // Revoked author's envelope refuses (C-5).
    let mut revoked = owner.clone();
    let (oa, _) = account(9, 9);
    let revoker = membership(9, &o.id, &oa.id, Role::Owner);
    revoked.revoke(&revoker).unwrap();
    assert!(matches!(
        env.accept(&AcceptContext {
            author_membership: &revoked,
            ..ctx
        }),
        Err(OrgError::RevokedMember(_))
    ));
    // Signature from another account refuses.
    let (other_acct, _) = account(5, 5);
    assert!(!env.verify_signature(&other_acct));
    assert!(env.verify_signature(&owner_acct));
}

#[test]
fn wire_enums_roundtrip_and_unknown_refuses() {
    assert_eq!(Role::parse("OWNER").unwrap(), Role::Owner);
    assert!(Role::parse("SUPERUSER").is_err());
    assert_eq!(
        EnvelopeKind::parse("ORG_POLICY").unwrap(),
        EnvelopeKind::OrgPolicy
    );
    assert_eq!(
        EnvelopeKind::OrgProfile.max_sensitivity(),
        Sensitivity::Project
    );
    assert_eq!(
        EnvelopeKind::Membership.max_sensitivity(),
        Sensitivity::Private
    );
    assert!(Sensitivity::parse("SECRET").unwrap() > Sensitivity::Private);
}
