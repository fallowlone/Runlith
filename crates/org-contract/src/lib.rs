//! Shared `org` store contract suite (TASK-0208, G13).
//! Every store implementing `OrgStorePort` must satisfy
//! [`run_org_store_contract`].
use org::mint::{
    mint_account_id, mint_envelope_id, mint_membership_id, mint_organization_id, mint_policy_id,
    mint_team_id, mint_usage_id,
};
use org::model::{
    keypair_from_secret, AcceptContext, AccountRecord, CapabilityCeiling, EnvelopeKind,
    MembershipRecord, OrgPolicy, OrganizationRecord, Role, SealRequest, Sensitivity, SyncEnvelope,
    TeamRecord, UsageEntry, UsageKind,
};
use org::port::OrgStorePort;

pub mod fixtures {
    use super::*;

    pub const ORG_KEY: [u8; 32] = [77u8; 32];

    pub fn account(seq: u128, seed: u8) -> (AccountRecord, [u8; 32]) {
        let (secret, public) = keypair_from_secret(&[seed; 32]).unwrap();
        (
            AccountRecord::register(mint_account_id(1_000, seq), "operator", &public, 1_000)
                .unwrap(),
            secret,
        )
    }

    pub fn organization() -> OrganizationRecord {
        OrganizationRecord::create(mint_organization_id(1_000, 1), "acme", 1_000).unwrap()
    }

    pub fn team(org: &OrganizationRecord) -> TeamRecord {
        TeamRecord::create(mint_team_id(1_000, 1), org.id.clone(), "core", 1_000).unwrap()
    }

    pub fn owner(org: &OrganizationRecord, account: &AccountRecord) -> MembershipRecord {
        MembershipRecord::grant(
            mint_membership_id(1_000, 1),
            org.id.clone(),
            account.id.clone(),
            None,
            Role::Owner,
            1_000,
        )
        .unwrap()
    }

    pub fn policy(org: &OrganizationRecord, by: &AccountRecord) -> OrgPolicy {
        OrgPolicy::new(
            mint_policy_id(1_000, 1),
            org.id.clone(),
            CapabilityCeiling::WorkspaceWrite,
            Sensitivity::Private,
            Some(1_000_000),
            86_400,
            by.id.clone(),
            1_000,
        )
        .unwrap()
    }

    pub fn envelope(
        seq: u128,
        org: &OrganizationRecord,
        author: &AccountRecord,
        author_secret: &[u8; 32],
        base_revision: u64,
    ) -> SyncEnvelope {
        SyncEnvelope::seal(SealRequest {
            id: mint_envelope_id(1_000, seq),
            org_id: org.id.clone(),
            author,
            author_secret,
            org_key: &ORG_KEY,
            nonce: [9u8; 24],
            kind: EnvelopeKind::OrgProfile,
            revision: base_revision + 1,
            base_revision,
            max_sensitivity: Sensitivity::Project,
            plaintext: b"org-profile-v1",
            created_at: 2_000,
        })
        .unwrap()
    }

    pub fn usage(
        org: &OrganizationRecord,
        account: &AccountRecord,
        key: &str,
        cost: u64,
    ) -> UsageEntry {
        UsageEntry::record(
            mint_usage_id(1_000, 1),
            org.id.clone(),
            account.id.clone(),
            None,
            UsageKind::Execution,
            10,
            cost,
            key,
            50_000,
        )
        .unwrap()
    }
}

fn assert_identity_roundtrip<S: OrgStorePort>(store: &mut S) {
    let (account, _) = fixtures::account(1, 7);
    let org = fixtures::organization();
    let team = fixtures::team(&org);
    let owner = fixtures::owner(&org, &account);

    store.put_account(&account).unwrap();
    store.put_account(&account).unwrap();
    let mut changed = account.clone();
    changed.display_name = "changed".to_owned();
    assert!(
        store.put_account(&changed).is_err(),
        "differing payload conflicts"
    );
    assert_eq!(
        store.load_account(&account.id).unwrap(),
        Some(account.clone())
    );

    store.put_organization(&org).unwrap();
    store.put_team(&team).unwrap();
    store.put_membership(&owner).unwrap();
    assert_eq!(store.load_organization(&org.id).unwrap(), Some(org.clone()));
    assert_eq!(store.list_teams(&org.id).unwrap(), vec![team]);
    assert_eq!(store.list_memberships(&org.id).unwrap(), vec![owner]);

    let mut archived = org.clone();
    archived.archive().unwrap();
    store.update_organization(&archived).unwrap();
    assert_eq!(
        store.load_organization(&org.id).unwrap().unwrap().state,
        org::model::OrganizationState::Archived
    );
    assert!(store
        .load_account(&mint_account_id(9, 9))
        .unwrap()
        .is_none());
}

fn assert_policy_currents<S: OrgStorePort>(store: &mut S) {
    let (account, _) = fixtures::account(1, 7);
    let org = fixtures::organization();
    let v1 = fixtures::policy(&org, &account);
    store.put_policy(&v1).unwrap();
    let v2 = v1
        .next_revision(
            CapabilityCeiling::ReadOnly,
            Sensitivity::Private,
            Some(500_000),
            86_400,
            account.id.clone(),
            2_000,
        )
        .unwrap();
    store.put_policy(&v2).unwrap();
    let current = store.current_policy(&org.id).unwrap().unwrap();
    assert_eq!(current.version, 2);
    assert_eq!(current.hard_capability_ceiling, CapabilityCeiling::ReadOnly);
}

fn assert_envelope_flow<S: OrgStorePort>(store: &mut S) {
    let (account, secret) = fixtures::account(1, 7);
    let org = fixtures::organization();
    let owner = fixtures::owner(&org, &account);
    assert_eq!(store.sync_head(&org.id).unwrap(), 0);

    let env = fixtures::envelope(1, &org, &account, &secret, 0);
    let plaintext = env
        .accept(&AcceptContext {
            expected_org: &org.id,
            author_account: &account,
            author_membership: &owner,
            current_head: store.sync_head(&org.id).unwrap(),
            already_seen: store.load_envelope(&env.id).unwrap().is_some(),
            org_key: &fixtures::ORG_KEY,
        })
        .unwrap();
    assert_eq!(plaintext, b"org-profile-v1".to_vec());
    store.append_envelope(&env).unwrap();
    assert_eq!(store.sync_head(&org.id).unwrap(), 1);
    assert!(
        store.append_envelope(&env).is_err(),
        "replayed envelope refuses"
    );

    let env2 = fixtures::envelope(2, &org, &account, &secret, 1);
    store.append_envelope(&env2).unwrap();
    assert_eq!(store.sync_head(&org.id).unwrap(), 2);
}

fn assert_usage_idempotent<S: OrgStorePort>(store: &mut S) {
    let (account, _) = fixtures::account(1, 7);
    let org = fixtures::organization();
    let entry = fixtures::usage(&org, &account, "k1", 900_000);
    store.append_usage(&entry).unwrap();
    store.append_usage(&entry).unwrap();
    assert_eq!(store.list_usage(&org.id).unwrap().len(), 1);
    let mut changed = entry.clone();
    changed.cost_microusd = 1;
    assert!(
        store.append_usage(&changed).is_err(),
        "same idempotency key with different payload conflicts"
    );
    let entries = store.list_usage(&org.id).unwrap();
    let spent = org::model::sum_cost_in_window(&entries, &org.id, 50_000, 86_400);
    assert_eq!(spent, 900_000);
}

fn assert_reopen_holds<S: OrgStorePort>(store: &S) {
    let org = fixtures::organization();
    assert!(store.load_organization(&org.id).unwrap().is_some());
    assert!(store.current_policy(&org.id).unwrap().is_some());
    assert_eq!(store.sync_head(&org.id).unwrap(), 2);
    assert_eq!(store.list_usage(&org.id).unwrap().len(), 1);
}

/// Runs the full org store contract against a fresh store.
/// `reopen` must yield the same durable state (restart durability).
pub fn run_org_store_contract<S: OrgStorePort>(store: &mut S, reopen: impl Fn() -> S) {
    assert_identity_roundtrip(store);
    assert_policy_currents(store);
    assert_envelope_flow(store);
    assert_usage_idempotent(store);
    let fresh = reopen();
    assert_reopen_holds(&fresh);
}
