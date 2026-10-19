//! Durable organization seam: identity, policy, sync log, and usage
//! ledger behind a port. Implementations must be append-only for
//! envelopes and usage entries (ADR-0047 §§3, 5).
use crate::model::{
    AccountId, AccountRecord, EnvelopeId, MembershipId, MembershipRecord, OrgError, OrgPolicy,
    OrganizationId, OrganizationRecord, SyncEnvelope, TeamId, TeamRecord, UsageEntry,
};

pub trait OrgStorePort {
    fn put_account(&mut self, account: &AccountRecord) -> Result<(), OrgError>;
    fn load_account(&self, id: &AccountId) -> Result<Option<AccountRecord>, OrgError>;
    fn put_organization(&mut self, org: &OrganizationRecord) -> Result<(), OrgError>;
    fn load_organization(
        &self,
        id: &OrganizationId,
    ) -> Result<Option<OrganizationRecord>, OrgError>;
    fn update_organization(&mut self, org: &OrganizationRecord) -> Result<(), OrgError>;
    fn put_team(&mut self, team: &TeamRecord) -> Result<(), OrgError>;
    fn load_team(&self, id: &TeamId) -> Result<Option<TeamRecord>, OrgError>;
    fn list_teams(&self, org_id: &OrganizationId) -> Result<Vec<TeamRecord>, OrgError>;
    fn put_membership(&mut self, membership: &MembershipRecord) -> Result<(), OrgError>;
    fn load_membership(&self, id: &MembershipId) -> Result<Option<MembershipRecord>, OrgError>;
    fn update_membership(&mut self, membership: &MembershipRecord) -> Result<(), OrgError>;
    fn list_memberships(&self, org_id: &OrganizationId) -> Result<Vec<MembershipRecord>, OrgError>;
    /// Stores one policy version; `current_policy` yields the highest.
    fn put_policy(&mut self, policy: &OrgPolicy) -> Result<(), OrgError>;
    fn current_policy(&self, org_id: &OrganizationId) -> Result<Option<OrgPolicy>, OrgError>;
    /// Records an accepted envelope and advances the stream head.
    /// A previously recorded envelope id refuses as duplicate.
    fn append_envelope(&mut self, envelope: &SyncEnvelope) -> Result<(), OrgError>;
    fn load_envelope(&self, id: &EnvelopeId) -> Result<Option<SyncEnvelope>, OrgError>;
    /// Current sync stream head for the org; 0 when nothing applied.
    fn sync_head(&self, org_id: &OrganizationId) -> Result<u64, OrgError>;
    /// Appends a usage entry; the same idempotency key with the same
    /// payload is a no-op, with a different payload a conflict.
    fn append_usage(&mut self, entry: &UsageEntry) -> Result<(), OrgError>;
    fn list_usage(&self, org_id: &OrganizationId) -> Result<Vec<UsageEntry>, OrgError>;
}
