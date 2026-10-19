//! ADR-0047 organization vocabulary: local account / organization /
//! team / membership identity with role authority, layered org policy,
//! server-blind sync envelopes, and the usage quota ledger. Pure;
//! private keys are inputs to signing only and never stored in records.
use std::fmt;

use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

pub const MAX_TEXT_CHARS: usize = 1024;
pub const MAX_CIPHERTEXT_BYTES: usize = 1_048_576;
pub const MAX_PLAINTEXT_BYTES: usize = 1_048_576;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn valid_prefixed(raw: &str, prefix: &str) -> bool {
    raw.strip_prefix(prefix)
        .is_some_and(|rest| rest.len() == 26 && rest.bytes().all(|b| CROCKFORD.contains(&b)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgError {
    InvalidInput(String),
    UnknownKind(String),
    ForbiddenRole(String),
    SelfRoleChange(String),
    RevokedMember(String),
    UnknownSigner(String),
    BadSignature(String),
    TamperedEnvelope(String),
    Conflict(String),
    DuplicateEnvelope(String),
    SensitivityRefusal(String),
    PolicyWeakening(String),
    OverQuota(String),
    NotFound(String),
    BackendFailure { detail: String },
}

impl fmt::Display for OrgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(d) => write!(f, "invalid input: {d}"),
            Self::UnknownKind(d) => write!(f, "unknown kind: {d}"),
            Self::ForbiddenRole(d) => write!(f, "forbidden role: {d}"),
            Self::SelfRoleChange(d) => write!(f, "self role change: {d}"),
            Self::RevokedMember(d) => write!(f, "revoked member: {d}"),
            Self::UnknownSigner(d) => write!(f, "unknown signer: {d}"),
            Self::BadSignature(d) => write!(f, "bad signature: {d}"),
            Self::TamperedEnvelope(d) => write!(f, "tampered envelope: {d}"),
            Self::Conflict(d) => write!(f, "conflict: {d}"),
            Self::DuplicateEnvelope(d) => write!(f, "duplicate envelope: {d}"),
            Self::SensitivityRefusal(d) => write!(f, "sensitivity refusal: {d}"),
            Self::PolicyWeakening(d) => write!(f, "policy weakening: {d}"),
            Self::OverQuota(d) => write!(f, "over quota: {d}"),
            Self::NotFound(d) => write!(f, "not found: {d}"),
            Self::BackendFailure { detail } => write!(f, "backend failure: {detail}"),
        }
    }
}

impl std::error::Error for OrgError {}

macro_rules! oid {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn new(raw: impl Into<String>) -> Result<Self, OrgError> {
                let raw = raw.into();
                if !valid_prefixed(&raw, $prefix) {
                    return Err(OrgError::InvalidInput(format!(
                        "id must be {} + 26 Crockford chars",
                        $prefix
                    )));
                }
                Ok(Self(raw))
            }
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

oid!(AccountId, "acc_");
oid!(OrganizationId, "org_");
oid!(TeamId, "team_");
oid!(MembershipId, "mem_");
oid!(PolicyId, "pol_");
oid!(EnvelopeId, "env_");
oid!(UsageId, "use_");

macro_rules! wire_enum {
    ($name:ident, [$($variant:ident => $wire:literal),+ $(,)?]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name { $($variant,)+ }
        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $wire,)+ }
            }
            pub fn parse(raw: &str) -> Result<Self, OrgError> {
                match raw {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(OrgError::UnknownKind(raw.to_owned())),
                }
            }
        }
    };
}

wire_enum!(Role, [
    Owner => "OWNER",
    Admin => "ADMIN",
    Member => "MEMBER",
    Viewer => "VIEWER",
]);
wire_enum!(MembershipState, [Active => "ACTIVE", Revoked => "REVOKED"]);
wire_enum!(OrganizationState, [Active => "ACTIVE", Archived => "ARCHIVED"]);
wire_enum!(UsageKind, [
    Execution => "EXECUTION",
    Planning => "PLANNING",
    Evaluation => "EVALUATION",
]);

impl Role {
    /// May add members, change roles, revoke, and write org policy.
    #[must_use]
    pub const fn can_administer(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
    /// May author sync envelopes (ADR-0047 §1).
    #[must_use]
    pub const fn can_author_envelope(self) -> bool {
        matches!(self, Self::Owner | Self::Admin | Self::Member)
    }
}

/// `DATA_CLASSIFICATION.md` levels, ordered least to most sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sensitivity {
    Public,
    Project,
    Private,
    Restricted,
    Secret,
}

impl Sensitivity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Public => "PUBLIC",
            Self::Project => "PROJECT",
            Self::Private => "PRIVATE",
            Self::Restricted => "RESTRICTED",
            Self::Secret => "SECRET",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, OrgError> {
        match raw {
            "PUBLIC" => Ok(Self::Public),
            "PROJECT" => Ok(Self::Project),
            "PRIVATE" => Ok(Self::Private),
            "RESTRICTED" => Ok(Self::Restricted),
            "SECRET" => Ok(Self::Secret),
            _ => Err(OrgError::UnknownKind(raw.to_owned())),
        }
    }
}

/// ADR-0045 capability enum reused as the org hard ceiling vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapabilityCeiling {
    ReadOnly,
    WorkspaceWrite,
    Execution,
}

impl CapabilityCeiling {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "READ_ONLY",
            Self::WorkspaceWrite => "WORKSPACE_WRITE",
            Self::Execution => "EXECUTION",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, OrgError> {
        match raw {
            "READ_ONLY" => Ok(Self::ReadOnly),
            "WORKSPACE_WRITE" => Ok(Self::WorkspaceWrite),
            "EXECUTION" => Ok(Self::Execution),
            _ => Err(OrgError::UnknownKind(raw.to_owned())),
        }
    }
}

/// Closed v1 envelope kinds; each caps its plaintext sensitivity (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EnvelopeKind {
    OrgProfile,
    Team,
    Membership,
    OrgPolicy,
    UsageSnapshot,
}

impl EnvelopeKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrgProfile => "ORG_PROFILE",
            Self::Team => "TEAM",
            Self::Membership => "MEMBERSHIP",
            Self::OrgPolicy => "ORG_POLICY",
            Self::UsageSnapshot => "USAGE_SNAPSHOT",
        }
    }
    pub fn parse(raw: &str) -> Result<Self, OrgError> {
        match raw {
            "ORG_PROFILE" => Ok(Self::OrgProfile),
            "TEAM" => Ok(Self::Team),
            "MEMBERSHIP" => Ok(Self::Membership),
            "ORG_POLICY" => Ok(Self::OrgPolicy),
            "USAGE_SNAPSHOT" => Ok(Self::UsageSnapshot),
            _ => Err(OrgError::UnknownKind(raw.to_owned())),
        }
    }
    /// Highest plaintext sensitivity this kind may ever carry.
    #[must_use]
    pub const fn max_sensitivity(self) -> Sensitivity {
        match self {
            Self::OrgProfile | Self::Team => Sensitivity::Project,
            Self::Membership | Self::OrgPolicy | Self::UsageSnapshot => Sensitivity::Private,
        }
    }
}

wire_enum!(CipherSuite, [XChaCha20Poly1305Ed25519 => "XCHACHA20_POLY1305_ED25519"]);

fn bounded(value: &str, field: &str, max: usize) -> Result<String, OrgError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max {
        return Err(OrgError::InvalidInput(format!(
            "{field} must be 1..={max} chars"
        )));
    }
    Ok(trimmed.to_owned())
}

/// sha256 fingerprint of an account signing public key (ADR-0047 §1).
#[must_use]
pub fn fingerprint(public_key: &[u8; 32]) -> String {
    format!("sha256:{:x}", Sha256::digest(public_key))
}

/// Derives (secret, public) from a 32-byte secret seed. The secret is
/// returned to the caller (vault) and never stored in domain records.
pub fn keypair_from_secret(secret: &[u8; 32]) -> Result<([u8; 32], [u8; 32]), OrgError> {
    let signing = SigningKey::from_bytes(secret);
    Ok((signing.to_bytes(), signing.verifying_key().to_bytes()))
}

/// Local operator identity on one device (ADR-0047 §1). Not a cloud
/// account; the signing private key lives only in the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    pub id: AccountId,
    pub display_name: String,
    pub signing_public_key: [u8; 32],
    pub fingerprint: String,
    pub created_at: u64,
}

impl AccountRecord {
    pub fn register(
        id: AccountId,
        display_name: impl Into<String>,
        signing_public_key: &[u8; 32],
        now: u64,
    ) -> Result<Self, OrgError> {
        VerifyingKey::from_bytes(signing_public_key)
            .map_err(|_| OrgError::InvalidInput("invalid ed25519 public key".to_owned()))?;
        Ok(Self {
            fingerprint: fingerprint(signing_public_key),
            id,
            display_name: bounded(&display_name.into(), "display_name", MAX_TEXT_CHARS)?,
            signing_public_key: *signing_public_key,
            created_at: now,
        })
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        if self.fingerprint != fingerprint(&self.signing_public_key) {
            return Err(OrgError::Conflict(format!(
                "fingerprint mismatch for {}",
                self.id
            )));
        }
        Ok(())
    }
}

/// One organization (ADR-0047 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationRecord {
    pub id: OrganizationId,
    pub name: String,
    pub state: OrganizationState,
    pub created_at: u64,
}

impl OrganizationRecord {
    pub fn create(id: OrganizationId, name: impl Into<String>, now: u64) -> Result<Self, OrgError> {
        Ok(Self {
            id,
            name: bounded(&name.into(), "name", MAX_TEXT_CHARS)?,
            state: OrganizationState::Active,
            created_at: now,
        })
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        bounded(&self.name, "name", MAX_TEXT_CHARS).map(|_| ())
    }

    pub fn archive(&mut self) -> Result<(), OrgError> {
        if self.state == OrganizationState::Archived {
            return Err(OrgError::InvalidInput(
                "organization already archived".to_owned(),
            ));
        }
        self.state = OrganizationState::Archived;
        Ok(())
    }
}

/// One team inside exactly one organization (ADR-0047 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRecord {
    pub id: TeamId,
    pub org_id: OrganizationId,
    pub name: String,
    pub created_at: u64,
}

impl TeamRecord {
    pub fn create(
        id: TeamId,
        org_id: OrganizationId,
        name: impl Into<String>,
        now: u64,
    ) -> Result<Self, OrgError> {
        Ok(Self {
            id,
            org_id,
            name: bounded(&name.into(), "name", MAX_TEXT_CHARS)?,
            created_at: now,
        })
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        bounded(&self.name, "name", MAX_TEXT_CHARS).map(|_| ())
    }
}

/// One account's membership in one organization (ADR-0047 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipRecord {
    pub id: MembershipId,
    pub org_id: OrganizationId,
    pub account_id: AccountId,
    pub team_id: Option<TeamId>,
    pub role: Role,
    pub state: MembershipState,
    pub created_at: u64,
}

impl MembershipRecord {
    pub fn grant(
        id: MembershipId,
        org_id: OrganizationId,
        account_id: AccountId,
        team_id: Option<TeamId>,
        role: Role,
        now: u64,
    ) -> Result<Self, OrgError> {
        Ok(Self {
            id,
            org_id,
            account_id,
            team_id,
            role,
            state: MembershipState::Active,
            created_at: now,
        })
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        Ok(())
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.state == MembershipState::Active
    }

    fn ensure_actor_admin(&self, actor: &MembershipRecord) -> Result<(), OrgError> {
        if actor.org_id != self.org_id {
            return Err(OrgError::ForbiddenRole(
                "actor belongs to another organization".to_owned(),
            ));
        }
        if !actor.is_active() {
            return Err(OrgError::RevokedMember(actor.id.to_string()));
        }
        if !actor.role.can_administer() {
            return Err(OrgError::ForbiddenRole(format!(
                "role {} cannot administer memberships",
                actor.role.as_str()
            )));
        }
        Ok(())
    }

    /// Role change under ADR-0047 §1 authority: no self change, only
    /// OWNER/ADMIN administer, only OWNER touches the OWNER role.
    pub fn change_role(
        &mut self,
        actor: &MembershipRecord,
        new_role: Role,
    ) -> Result<(), OrgError> {
        if actor.id == self.id {
            return Err(OrgError::SelfRoleChange(self.id.to_string()));
        }
        self.ensure_actor_admin(actor)?;
        if !self.is_active() {
            return Err(OrgError::RevokedMember(self.id.to_string()));
        }
        if (new_role == Role::Owner || self.role == Role::Owner) && actor.role != Role::Owner {
            return Err(OrgError::ForbiddenRole(
                "only OWNER may grant or change the OWNER role".to_owned(),
            ));
        }
        self.role = new_role;
        Ok(())
    }

    /// Revocation is durable, immediate, and terminal (C-5).
    pub fn revoke(&mut self, actor: &MembershipRecord) -> Result<(), OrgError> {
        if actor.id == self.id {
            return Err(OrgError::SelfRoleChange(self.id.to_string()));
        }
        self.ensure_actor_admin(actor)?;
        if !self.is_active() {
            return Err(OrgError::InvalidInput(
                "membership already revoked".to_owned(),
            ));
        }
        if self.role == Role::Owner && actor.role != Role::Owner {
            return Err(OrgError::ForbiddenRole(
                "only OWNER may revoke the OWNER membership".to_owned(),
            ));
        }
        self.state = MembershipState::Revoked;
        Ok(())
    }

    /// Envelope authoring requires an ACTIVE non-VIEWER membership.
    pub fn ensure_can_author_envelope(&self) -> Result<(), OrgError> {
        if !self.is_active() {
            return Err(OrgError::RevokedMember(self.id.to_string()));
        }
        if !self.role.can_author_envelope() {
            return Err(OrgError::ForbiddenRole(format!(
                "role {} cannot author envelopes",
                self.role.as_str()
            )));
        }
        Ok(())
    }
}

/// Organization hard policy (ADR-0047 §2). One record per version;
/// `version` is monotonic per organization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgPolicy {
    pub id: PolicyId,
    pub org_id: OrganizationId,
    pub version: u64,
    pub hard_capability_ceiling: CapabilityCeiling,
    pub sync_max_sensitivity: Sensitivity,
    pub quota_max_cost_microusd: Option<u64>,
    pub quota_window_seconds: u64,
    pub updated_by: AccountId,
    pub updated_at: u64,
}

impl OrgPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PolicyId,
        org_id: OrganizationId,
        hard_capability_ceiling: CapabilityCeiling,
        sync_max_sensitivity: Sensitivity,
        quota_max_cost_microusd: Option<u64>,
        quota_window_seconds: u64,
        updated_by: AccountId,
        now: u64,
    ) -> Result<Self, OrgError> {
        if quota_window_seconds == 0 {
            return Err(OrgError::InvalidInput(
                "quota_window_seconds must be >= 1".to_owned(),
            ));
        }
        if sync_max_sensitivity > Sensitivity::Private {
            return Err(OrgError::SensitivityRefusal(
                "org sync ceiling may not exceed PRIVATE".to_owned(),
            ));
        }
        Ok(Self {
            id,
            org_id,
            version: 1,
            hard_capability_ceiling,
            sync_max_sensitivity,
            quota_max_cost_microusd,
            quota_window_seconds,
            updated_by,
            updated_at: now,
        })
    }

    /// Next version of this policy with replaced knobs (same identity).
    #[allow(clippy::too_many_arguments)]
    pub fn next_revision(
        &self,
        hard_capability_ceiling: CapabilityCeiling,
        sync_max_sensitivity: Sensitivity,
        quota_max_cost_microusd: Option<u64>,
        quota_window_seconds: u64,
        updated_by: AccountId,
        now: u64,
    ) -> Result<Self, OrgError> {
        let mut next = Self::new(
            self.id.clone(),
            self.org_id.clone(),
            hard_capability_ceiling,
            sync_max_sensitivity,
            quota_max_cost_microusd,
            quota_window_seconds,
            updated_by,
            now,
        )?;
        next.version = self
            .version
            .checked_add(1)
            .ok_or_else(|| OrgError::InvalidInput("policy version overflow".to_owned()))?;
        Ok(next)
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        if self.version == 0 || self.quota_window_seconds == 0 {
            return Err(OrgError::InvalidInput(
                "policy version and quota window must be >= 1".to_owned(),
            ));
        }
        if self.sync_max_sensitivity > Sensitivity::Private {
            return Err(OrgError::SensitivityRefusal(
                "org sync ceiling may not exceed PRIVATE".to_owned(),
            ));
        }
        Ok(())
    }
}

/// One project/user layer input to policy resolution. `None` knobs add
/// no constraint; present knobs may only narrow the org hard policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayeredPolicy {
    pub capability_ceiling: Option<CapabilityCeiling>,
    pub sync_max_sensitivity: Option<Sensitivity>,
    pub quota_max_cost_microusd: Option<u64>,
}

/// Result of restrictive composition (ADR-0047 §2): the most
/// restrictive value of every knob across org and lower layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub capability_ceiling: CapabilityCeiling,
    pub sync_max_sensitivity: Sensitivity,
    pub quota_max_cost_microusd: Option<u64>,
    pub quota_window_seconds: u64,
}

impl EffectivePolicy {
    /// Pre-dispatch quota gate (C-9): refuses before execution when the
    /// window spend plus the requested estimate exceeds the limit.
    pub fn ensure_quota(
        &self,
        spent_in_window: u64,
        requested_estimate: u64,
    ) -> Result<(), OrgError> {
        if let Some(limit) = self.quota_max_cost_microusd {
            let projected = spent_in_window.saturating_add(requested_estimate);
            if projected > limit {
                return Err(OrgError::OverQuota(format!(
                    "projected {projected} exceeds limit {limit}"
                )));
            }
        }
        Ok(())
    }
}

/// Composes org hard policy with project/user layers. A layer knob above
/// the org hard value is a weakening attempt and refuses typed; the
/// effective policy is never silently capped (C-8, INV-148).
pub fn resolve_effective(
    org: &OrgPolicy,
    layers: &[LayeredPolicy],
) -> Result<EffectivePolicy, OrgError> {
    org.revalidate()?;
    let mut effective = EffectivePolicy {
        capability_ceiling: org.hard_capability_ceiling,
        sync_max_sensitivity: org.sync_max_sensitivity,
        quota_max_cost_microusd: org.quota_max_cost_microusd,
        quota_window_seconds: org.quota_window_seconds,
    };
    for layer in layers {
        if let Some(ceiling) = layer.capability_ceiling {
            if ceiling > org.hard_capability_ceiling {
                return Err(OrgError::PolicyWeakening(format!(
                    "capability ceiling {} exceeds org hard {}",
                    ceiling.as_str(),
                    org.hard_capability_ceiling.as_str()
                )));
            }
            effective.capability_ceiling = effective.capability_ceiling.min(ceiling);
        }
        if let Some(sensitivity) = layer.sync_max_sensitivity {
            if sensitivity > org.sync_max_sensitivity {
                return Err(OrgError::PolicyWeakening(format!(
                    "sync sensitivity {} exceeds org hard {}",
                    sensitivity.as_str(),
                    org.sync_max_sensitivity.as_str()
                )));
            }
            effective.sync_max_sensitivity = effective.sync_max_sensitivity.min(sensitivity);
        }
        if let Some(quota) = layer.quota_max_cost_microusd {
            if let Some(org_quota) = org.quota_max_cost_microusd {
                if quota > org_quota {
                    return Err(OrgError::PolicyWeakening(format!(
                        "quota {quota} exceeds org hard {org_quota}"
                    )));
                }
            }
            effective.quota_max_cost_microusd = Some(match effective.quota_max_cost_microusd {
                Some(current) => current.min(quota),
                None => quota,
            });
        }
    }
    Ok(effective)
}

/// One append-only usage ledger entry (ADR-0047 §5). No content payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEntry {
    pub id: UsageId,
    pub org_id: OrganizationId,
    pub account_id: AccountId,
    pub project_ref: Option<String>,
    pub kind: UsageKind,
    pub units: u64,
    pub cost_microusd: u64,
    pub idempotency_key: String,
    pub recorded_at: u64,
}

impl UsageEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        id: UsageId,
        org_id: OrganizationId,
        account_id: AccountId,
        project_ref: Option<&str>,
        kind: UsageKind,
        units: u64,
        cost_microusd: u64,
        idempotency_key: &str,
        now: u64,
    ) -> Result<Self, OrgError> {
        Ok(Self {
            id,
            org_id,
            account_id,
            project_ref: match project_ref {
                Some(raw) => Some(bounded(raw, "project_ref", MAX_TEXT_CHARS)?),
                None => None,
            },
            kind,
            units,
            cost_microusd,
            idempotency_key: bounded(idempotency_key, "idempotency_key", MAX_TEXT_CHARS)?,
            recorded_at: now,
        })
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        bounded(&self.idempotency_key, "idempotency_key", MAX_TEXT_CHARS).map(|_| ())
    }
}

/// Total measured cost for one org inside `[now - window, now]`.
/// Out-of-window and other-org entries never count toward quota.
#[must_use]
pub fn sum_cost_in_window(
    entries: &[UsageEntry],
    org_id: &OrganizationId,
    now: u64,
    window_seconds: u64,
) -> u64 {
    let start = now.saturating_sub(window_seconds);
    entries
        .iter()
        .filter(|e| &e.org_id == org_id && e.recorded_at >= start && e.recorded_at <= now)
        .fold(0u64, |acc, e| acc.saturating_add(e.cost_microusd))
}

/// Inputs for sealing one sync envelope. Secrets and the org key are
/// borrowed for this call only and never stored in the record.
pub struct SealRequest<'a> {
    pub id: EnvelopeId,
    pub org_id: OrganizationId,
    pub author: &'a AccountRecord,
    pub author_secret: &'a [u8; 32],
    pub org_key: &'a [u8; 32],
    pub nonce: [u8; 24],
    pub kind: EnvelopeKind,
    pub revision: u64,
    pub base_revision: u64,
    pub max_sensitivity: Sensitivity,
    pub plaintext: &'a [u8],
    pub created_at: u64,
}

/// One E2E-encrypted, server-blind sync envelope (ADR-0047 §3). A
/// carrier sees only this record: ciphertext plus bounded metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncEnvelope {
    pub id: EnvelopeId,
    pub org_id: OrganizationId,
    pub author_account_id: AccountId,
    pub kind: EnvelopeKind,
    pub revision: u64,
    pub base_revision: u64,
    pub max_sensitivity: Sensitivity,
    pub cipher_suite: CipherSuite,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
    pub signature: [u8; 64],
    pub created_at: u64,
}

/// Receiver-side facts for the pure acceptance predicate (§3).
#[derive(Debug, Clone, Copy)]
pub struct AcceptContext<'a> {
    pub expected_org: &'a OrganizationId,
    pub author_account: &'a AccountRecord,
    pub author_membership: &'a MembershipRecord,
    pub current_head: u64,
    pub already_seen: bool,
    pub org_key: &'a [u8; 32],
}

impl SyncEnvelope {
    /// Seals plaintext into a signed envelope. Classification is refused
    /// before any encryption happens (C-3): no envelope carrying
    /// `RESTRICTED`/`SECRET` content can ever be constructed.
    pub fn seal(req: SealRequest<'_>) -> Result<Self, OrgError> {
        if req.max_sensitivity > req.kind.max_sensitivity() {
            return Err(OrgError::SensitivityRefusal(format!(
                "sensitivity {} exceeds kind {} maximum {}",
                req.max_sensitivity.as_str(),
                req.kind.as_str(),
                req.kind.max_sensitivity().as_str()
            )));
        }
        if req.revision
            != req
                .base_revision
                .checked_add(1)
                .ok_or_else(|| OrgError::InvalidInput("revision overflow".to_owned()))?
        {
            return Err(OrgError::InvalidInput(
                "revision must equal base_revision + 1".to_owned(),
            ));
        }
        if req.plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err(OrgError::InvalidInput(
                "plaintext exceeds size bound".to_owned(),
            ));
        }
        let mut envelope = Self {
            id: req.id,
            org_id: req.org_id,
            author_account_id: req.author.id.clone(),
            kind: req.kind,
            revision: req.revision,
            base_revision: req.base_revision,
            max_sensitivity: req.max_sensitivity,
            cipher_suite: CipherSuite::XChaCha20Poly1305Ed25519,
            nonce: req.nonce,
            ciphertext: Vec::new(),
            signature: [0u8; 64],
            created_at: req.created_at,
        };
        let mut ciphertext = req.plaintext.to_vec();
        XChaCha20Poly1305::new(req.org_key.into())
            .encrypt_in_place(
                &XNonce::from(req.nonce),
                &envelope.header_bytes(),
                &mut ciphertext,
            )
            .map_err(|_| OrgError::TamperedEnvelope("seal failed".to_owned()))?;
        if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(OrgError::InvalidInput(
                "ciphertext exceeds size bound".to_owned(),
            ));
        }
        envelope.ciphertext = ciphertext;
        let signing = SigningKey::from_bytes(req.author_secret);
        envelope.signature = signing.sign(&envelope.signing_message()).to_bytes();
        Ok(envelope)
    }

    fn header_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        out.extend_from_slice(self.org_id.as_str().as_bytes());
        out.extend_from_slice(&self.revision.to_be_bytes());
        out.extend_from_slice(&self.base_revision.to_be_bytes());
        out.extend_from_slice(self.kind.as_str().as_bytes());
        out
    }

    /// Bytes covered by the author signature:
    /// header || sha256(ciphertext).
    #[must_use]
    pub fn signing_message(&self) -> Vec<u8> {
        let mut out = self.header_bytes();
        out.extend_from_slice(&Sha256::digest(&self.ciphertext));
        out
    }

    /// Verifies the author signature against the named account key.
    #[must_use]
    pub fn verify_signature(&self, author: &AccountRecord) -> bool {
        if author.id != self.author_account_id {
            return false;
        }
        let Ok(verifying) = VerifyingKey::from_bytes(&author.signing_public_key) else {
            return false;
        };
        verifying
            .verify(
                &self.signing_message(),
                &Signature::from_bytes(&self.signature),
            )
            .is_ok()
    }

    /// Opens the envelope with the org key. Failure is typed tampering;
    /// no partial plaintext ever escapes.
    pub fn open(&self, org_key: &[u8; 32]) -> Result<Vec<u8>, OrgError> {
        let mut buffer = self.ciphertext.clone();
        XChaCha20Poly1305::new(org_key.into())
            .decrypt_in_place(&XNonce::from(self.nonce), &self.header_bytes(), &mut buffer)
            .map_err(|_| OrgError::TamperedEnvelope(self.id.to_string()))?;
        Ok(buffer)
    }

    pub fn revalidate(&self) -> Result<(), OrgError> {
        if self.revision != self.base_revision.wrapping_add(1) || self.revision == 0 {
            return Err(OrgError::InvalidInput(
                "revision must equal base_revision + 1".to_owned(),
            ));
        }
        if self.ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(OrgError::InvalidInput(
                "ciphertext exceeds size bound".to_owned(),
            ));
        }
        if self.max_sensitivity > self.kind.max_sensitivity() {
            return Err(OrgError::SensitivityRefusal(
                "envelope sensitivity exceeds kind maximum".to_owned(),
            ));
        }
        Ok(())
    }

    /// The full acceptance predicate (ADR-0047 §3). Every failure refuses
    /// typed and applies nothing; success returns the plaintext exactly
    /// once for the caller to apply and record.
    pub fn accept(&self, ctx: &AcceptContext<'_>) -> Result<Vec<u8>, OrgError> {
        self.revalidate()?;
        if &self.org_id != ctx.expected_org {
            return Err(OrgError::Conflict(format!(
                "envelope org {} does not match expected {}",
                self.org_id, ctx.expected_org
            )));
        }
        if ctx.already_seen {
            return Err(OrgError::DuplicateEnvelope(self.id.to_string()));
        }
        if ctx.author_account.id != self.author_account_id
            || ctx.author_membership.account_id != self.author_account_id
            || ctx.author_membership.org_id != self.org_id
        {
            return Err(OrgError::UnknownSigner(self.author_account_id.to_string()));
        }
        if !ctx.author_membership.is_active() {
            return Err(OrgError::RevokedMember(
                ctx.author_membership.id.to_string(),
            ));
        }
        ctx.author_membership.ensure_can_author_envelope()?;
        if self.base_revision != ctx.current_head {
            return Err(OrgError::Conflict(format!(
                "base_revision {} != current head {}",
                self.base_revision, ctx.current_head
            )));
        }
        if !self.verify_signature(ctx.author_account) {
            return Err(OrgError::BadSignature(self.id.to_string()));
        }
        self.open(ctx.org_key)
    }
}
