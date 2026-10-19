//! Trusted organization composition (TASK-0210, ADR-0047).
//! The only mutation authority for organization facts: account/org/team
//! creation, membership role changes and revocation, org policy writes
//! and resolution, sync envelope author/accept, usage and quota. Role
//! authority is checked here against durable memberships; a revoked
//! member's mutations and envelopes refuse immediately (C-4, C-5).
use std::sync::atomic::{AtomicU64, Ordering};

use org::mint::{
    mint_account_id, mint_envelope_id, mint_membership_id, mint_organization_id, mint_policy_id,
    mint_team_id, mint_usage_id,
};
use org::model::{
    resolve_effective, sum_cost_in_window, AcceptContext, AccountId, AccountRecord,
    CapabilityCeiling, EffectivePolicy, EnvelopeId, EnvelopeKind, LayeredPolicy, MembershipId,
    MembershipRecord, OrgError, OrgPolicy, OrganizationId, OrganizationRecord, OrganizationState,
    Role, SealRequest, Sensitivity, SyncEnvelope, TeamId, TeamRecord, UsageEntry, UsageKind,
};
use org::port::OrgStorePort;

#[derive(Debug)]
pub struct PolicyRequest {
    pub hard_capability_ceiling: CapabilityCeiling,
    pub sync_max_sensitivity: Sensitivity,
    pub quota_max_cost_microusd: Option<u64>,
    pub quota_window_seconds: u64,
}

#[derive(Debug)]
pub struct UsageRequest {
    pub account_id: AccountId,
    pub project_ref: Option<String>,
    pub kind: UsageKind,
    pub units: u64,
    pub cost_microusd: u64,
    pub idempotency_key: String,
}

pub struct OrgService<S> {
    store: S,
    id_seq: AtomicU64,
}

impl<S: OrgStorePort> OrgService<S> {
    pub fn new(store: S) -> Self {
        Self {
            store,
            id_seq: AtomicU64::new(0),
        }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    fn seq(&self) -> u128 {
        u128::from(self.id_seq.fetch_add(1, Ordering::SeqCst))
    }

    fn get_membership(&self, id: &MembershipId) -> Result<MembershipRecord, OrgError> {
        self.store
            .load_membership(id)?
            .ok_or_else(|| OrgError::NotFound(id.to_string()))
    }

    fn get_organization(&self, id: &OrganizationId) -> Result<OrganizationRecord, OrgError> {
        self.store
            .load_organization(id)?
            .ok_or_else(|| OrgError::NotFound(id.to_string()))
    }

    fn ensure_org_mutable(&self, org: &OrganizationRecord) -> Result<(), OrgError> {
        if org.state == OrganizationState::Archived {
            return Err(OrgError::InvalidInput(format!(
                "organization {} is archived",
                org.id
            )));
        }
        Ok(())
    }

    /// Loads the actor membership and requires it to administer the org:
    /// ACTIVE (revocation refuses as `RevokedMember`) with OWNER/ADMIN.
    fn actor_admin(
        &self,
        actor_id: &MembershipId,
        org_id: &OrganizationId,
    ) -> Result<MembershipRecord, OrgError> {
        let actor = self.get_membership(actor_id)?;
        if actor.org_id != *org_id {
            return Err(OrgError::ForbiddenRole(
                "actor belongs to another organization".to_owned(),
            ));
        }
        if !actor.is_active() {
            return Err(OrgError::RevokedMember(actor.id.to_string()));
        }
        if !actor.role.can_administer() {
            return Err(OrgError::ForbiddenRole(format!(
                "role {} cannot administer this organization",
                actor.role.as_str()
            )));
        }
        Ok(actor)
    }

    fn membership_of(
        &self,
        org_id: &OrganizationId,
        account_id: &AccountId,
    ) -> Result<Option<MembershipRecord>, OrgError> {
        Ok(self
            .store
            .list_memberships(org_id)?
            .into_iter()
            .find(|m| &m.account_id == account_id))
    }

    pub fn register_account(
        &mut self,
        display_name: &str,
        signing_public_key: &[u8; 32],
        now: u64,
    ) -> Result<AccountRecord, OrgError> {
        let account = AccountRecord::register(
            mint_account_id(now, self.seq()),
            display_name,
            signing_public_key,
            now,
        )?;
        self.store.put_account(&account)?;
        Ok(account)
    }

    pub fn get_account(&self, id: &AccountId) -> Result<AccountRecord, OrgError> {
        self.store
            .load_account(id)?
            .ok_or_else(|| OrgError::NotFound(id.to_string()))
    }

    pub fn organization(&self, id: &OrganizationId) -> Result<OrganizationRecord, OrgError> {
        self.get_organization(id)
    }

    /// Creates an organization; the creating account becomes its OWNER.
    pub fn create_organization(
        &mut self,
        creator_account_id: &AccountId,
        name: &str,
        now: u64,
    ) -> Result<(OrganizationRecord, MembershipRecord), OrgError> {
        self.get_account(creator_account_id)?;
        let org = OrganizationRecord::create(mint_organization_id(now, self.seq()), name, now)?;
        self.store.put_organization(&org)?;
        let owner = MembershipRecord::grant(
            mint_membership_id(now, self.seq()),
            org.id.clone(),
            creator_account_id.clone(),
            None,
            Role::Owner,
            now,
        )?;
        self.store.put_membership(&owner)?;
        Ok((org, owner))
    }

    pub fn create_team(
        &mut self,
        actor_id: &MembershipId,
        org_id: &OrganizationId,
        name: &str,
        now: u64,
    ) -> Result<TeamRecord, OrgError> {
        self.actor_admin(actor_id, org_id)?;
        let org = self.get_organization(org_id)?;
        self.ensure_org_mutable(&org)?;
        let team = TeamRecord::create(mint_team_id(now, self.seq()), org_id.clone(), name, now)?;
        self.store.put_team(&team)?;
        Ok(team)
    }

    pub fn list_teams(&self, org_id: &OrganizationId) -> Result<Vec<TeamRecord>, OrgError> {
        self.get_organization(org_id)?;
        self.store.list_teams(org_id)
    }

    pub fn membership(&self, id: &MembershipId) -> Result<MembershipRecord, OrgError> {
        self.get_membership(id)
    }

    pub fn list_memberships(
        &self,
        org_id: &OrganizationId,
    ) -> Result<Vec<MembershipRecord>, OrgError> {
        self.get_organization(org_id)?;
        self.store.list_memberships(org_id)
    }

    /// Adds an ACTIVE membership. Only OWNER may grant the OWNER role.
    pub fn add_member(
        &mut self,
        actor_id: &MembershipId,
        org_id: &OrganizationId,
        account_id: &AccountId,
        team_id: Option<TeamId>,
        role: Role,
        now: u64,
    ) -> Result<MembershipRecord, OrgError> {
        let actor = self.actor_admin(actor_id, org_id)?;
        let org = self.get_organization(org_id)?;
        self.ensure_org_mutable(&org)?;
        if role == Role::Owner && actor.role != Role::Owner {
            return Err(OrgError::ForbiddenRole(
                "only OWNER may grant the OWNER role".to_owned(),
            ));
        }
        self.get_account(account_id)?;
        if let Some(team_id) = &team_id {
            let team = self
                .store
                .load_team(team_id)?
                .ok_or_else(|| OrgError::NotFound(team_id.to_string()))?;
            if team.org_id != *org_id {
                return Err(OrgError::InvalidInput(
                    "team belongs to another organization".to_owned(),
                ));
            }
        }
        if let Some(existing) = self.membership_of(org_id, account_id)? {
            if existing.is_active() {
                return Err(OrgError::Conflict(format!(
                    "account {account_id} already a member of {org_id}"
                )));
            }
            return Err(OrgError::RevokedMember(existing.id.to_string()));
        }
        let membership = MembershipRecord::grant(
            mint_membership_id(now, self.seq()),
            org_id.clone(),
            account_id.clone(),
            team_id,
            role,
            now,
        )?;
        self.store.put_membership(&membership)?;
        Ok(membership)
    }

    pub fn change_role(
        &mut self,
        actor_id: &MembershipId,
        target_id: &MembershipId,
        new_role: Role,
    ) -> Result<MembershipRecord, OrgError> {
        let actor = self.get_membership(actor_id)?;
        let mut target = self.get_membership(target_id)?;
        target.change_role(&actor, new_role)?;
        self.store.update_membership(&target)?;
        Ok(target)
    }

    pub fn revoke_member(
        &mut self,
        actor_id: &MembershipId,
        target_id: &MembershipId,
    ) -> Result<MembershipRecord, OrgError> {
        let actor = self.get_membership(actor_id)?;
        let mut target = self.get_membership(target_id)?;
        target.revoke(&actor)?;
        self.store.update_membership(&target)?;
        Ok(target)
    }

    /// Writes the next org policy version (OWNER/ADMIN only).
    pub fn write_policy(
        &mut self,
        actor_id: &MembershipId,
        org_id: &OrganizationId,
        request: PolicyRequest,
        now: u64,
    ) -> Result<OrgPolicy, OrgError> {
        let actor = self.actor_admin(actor_id, org_id)?;
        let org = self.get_organization(org_id)?;
        self.ensure_org_mutable(&org)?;
        let policy = match self.store.current_policy(org_id)? {
            Some(current) => current.next_revision(
                request.hard_capability_ceiling,
                request.sync_max_sensitivity,
                request.quota_max_cost_microusd,
                request.quota_window_seconds,
                actor.account_id.clone(),
                now,
            )?,
            None => OrgPolicy::new(
                mint_policy_id(now, self.seq()),
                org_id.clone(),
                request.hard_capability_ceiling,
                request.sync_max_sensitivity,
                request.quota_max_cost_microusd,
                request.quota_window_seconds,
                actor.account_id.clone(),
                now,
            )?,
        };
        self.store.put_policy(&policy)?;
        Ok(policy)
    }

    pub fn current_policy(&self, org_id: &OrganizationId) -> Result<OrgPolicy, OrgError> {
        self.store
            .current_policy(org_id)?
            .ok_or_else(|| OrgError::NotFound(format!("policy for {org_id}")))
    }

    /// Restrictive org > project > user resolution; a lower layer above
    /// the org hard value refuses as `PolicyWeakening` (C-8).
    pub fn resolve_policy(
        &self,
        org_id: &OrganizationId,
        layers: &[LayeredPolicy],
    ) -> Result<EffectivePolicy, OrgError> {
        resolve_effective(&self.current_policy(org_id)?, layers)
    }

    /// Pre-dispatch quota gate (C-9). No org policy means no org quota.
    pub fn check_quota(
        &self,
        org_id: &OrganizationId,
        requested_estimate: u64,
        now: u64,
    ) -> Result<(), OrgError> {
        let Some(policy) = self.store.current_policy(org_id)? else {
            return Ok(());
        };
        let effective = resolve_effective(&policy, &[])?;
        let entries = self.store.list_usage(org_id)?;
        let spent = sum_cost_in_window(&entries, org_id, now, policy.quota_window_seconds);
        effective.ensure_quota(spent, requested_estimate)
    }

    pub fn record_usage(
        &mut self,
        org_id: &OrganizationId,
        request: UsageRequest,
        now: u64,
    ) -> Result<UsageEntry, OrgError> {
        self.get_organization(org_id)?;
        let membership = self
            .membership_of(org_id, &request.account_id)?
            .ok_or_else(|| OrgError::NotFound(format!("membership for {}", request.account_id)))?;
        if !membership.is_active() {
            return Err(OrgError::RevokedMember(membership.id.to_string()));
        }
        let entry = UsageEntry::record(
            mint_usage_id(now, self.seq()),
            org_id.clone(),
            request.account_id,
            request.project_ref.as_deref(),
            request.kind,
            request.units,
            request.cost_microusd,
            &request.idempotency_key,
            now,
        )?;
        self.store.append_usage(&entry)?;
        Ok(entry)
    }

    pub fn list_usage(&self, org_id: &OrganizationId) -> Result<Vec<UsageEntry>, OrgError> {
        self.get_organization(org_id)?;
        self.store.list_usage(org_id)
    }

    pub fn sync_head(&self, org_id: &OrganizationId) -> Result<u64, OrgError> {
        self.get_organization(org_id)?;
        self.store.sync_head(org_id)
    }

    pub fn envelope(&self, id: &EnvelopeId) -> Result<SyncEnvelope, OrgError> {
        self.store
            .load_envelope(id)?
            .ok_or_else(|| OrgError::NotFound(id.to_string()))
    }

    /// Seals and records an envelope authored by an ACTIVE member whose
    /// role may author (C-5). Secrets are borrowed for this call only.
    #[allow(clippy::too_many_arguments)]
    pub fn author_envelope(
        &mut self,
        actor_id: &MembershipId,
        kind: EnvelopeKind,
        max_sensitivity: Sensitivity,
        plaintext: &[u8],
        org_key: &[u8; 32],
        author_secret: &[u8; 32],
        nonce: [u8; 24],
        now: u64,
    ) -> Result<SyncEnvelope, OrgError> {
        let actor = self.get_membership(actor_id)?;
        actor.ensure_can_author_envelope()?;
        let org = self.get_organization(&actor.org_id)?;
        self.ensure_org_mutable(&org)?;
        let author = self.get_account(&actor.account_id)?;
        let head = self.store.sync_head(&actor.org_id)?;
        let envelope = SyncEnvelope::seal(SealRequest {
            id: mint_envelope_id(now, self.seq()),
            org_id: actor.org_id.clone(),
            author: &author,
            author_secret,
            org_key,
            nonce,
            kind,
            revision: head
                .checked_add(1)
                .ok_or_else(|| OrgError::InvalidInput("revision overflow".to_owned()))?,
            base_revision: head,
            max_sensitivity,
            plaintext,
            created_at: now,
        })?;
        self.store.append_envelope(&envelope)?;
        Ok(envelope)
    }

    /// Accepts an envelope into this store (ADR-0047 §3). Every refusal
    /// applies nothing; success records the envelope and returns the
    /// plaintext exactly once for the caller to apply.
    pub fn accept_envelope(
        &mut self,
        envelope: &SyncEnvelope,
        org_key: &[u8; 32],
    ) -> Result<Vec<u8>, OrgError> {
        let org = self.get_organization(&envelope.org_id)?;
        self.ensure_org_mutable(&org)?;
        if let Some(policy) = self.store.current_policy(&envelope.org_id)? {
            if envelope.max_sensitivity > policy.sync_max_sensitivity {
                return Err(OrgError::SensitivityRefusal(format!(
                    "envelope sensitivity {} exceeds org sync ceiling {}",
                    envelope.max_sensitivity.as_str(),
                    policy.sync_max_sensitivity.as_str()
                )));
            }
        }
        let author_account = self
            .store
            .load_account(&envelope.author_account_id)?
            .ok_or_else(|| OrgError::UnknownSigner(envelope.author_account_id.to_string()))?;
        let author_membership = self
            .membership_of(&envelope.org_id, &envelope.author_account_id)?
            .ok_or_else(|| OrgError::UnknownSigner(envelope.author_account_id.to_string()))?;
        let plaintext = envelope.accept(&AcceptContext {
            expected_org: &envelope.org_id,
            author_account: &author_account,
            author_membership: &author_membership,
            current_head: self.store.sync_head(&envelope.org_id)?,
            already_seen: self.store.load_envelope(&envelope.id)?.is_some(),
            org_key,
        })?;
        self.store.append_envelope(envelope)?;
        Ok(plaintext)
    }
}
