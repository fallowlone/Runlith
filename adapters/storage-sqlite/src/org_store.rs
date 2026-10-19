//! SQLite organization seam (TASK-0209, ADR-0047). Policy, role, and
//! envelope decisions stay in the pure `org` domain; decode revalidates
//! every row so stored content can never bypass domain validation.
use org::model::{
    AccountId, AccountRecord, CapabilityCeiling, CipherSuite, EnvelopeId, EnvelopeKind,
    MembershipId, MembershipRecord, MembershipState, OrgError, OrgPolicy, OrganizationId,
    OrganizationRecord, OrganizationState, Role, Sensitivity, SyncEnvelope, TeamId, TeamRecord,
    UsageEntry, UsageId, UsageKind,
};
use org::port::OrgStorePort;
use rusqlite::{params, Connection, OptionalExtension};

use crate::task_store::SqliteStore;

fn backend(error: rusqlite::Error) -> OrgError {
    OrgError::BackendFailure {
        detail: error.to_string(),
    }
}

fn stored<T>(parsed: Result<T, OrgError>, what: &str) -> Result<T, OrgError> {
    parsed.map_err(|err| OrgError::BackendFailure {
        detail: format!("stored {what} is malformed: {err}"),
    })
}

fn u64_to_i64(v: u64, f: &str) -> Result<i64, OrgError> {
    i64::try_from(v).map_err(|_| OrgError::InvalidInput(format!("{f} overflows i64")))
}

fn i64_to_u64(v: i64, f: &str) -> Result<u64, OrgError> {
    u64::try_from(v).map_err(|_| OrgError::BackendFailure {
        detail: format!("stored {f} is negative"),
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode<const N: usize>(raw: &str, what: &str) -> Result<[u8; N], OrgError> {
    if raw.len() != N * 2 {
        return Err(OrgError::BackendFailure {
            detail: format!("stored {what} has wrong length"),
        });
    }
    let bytes: Vec<u8> = (0..N)
        .map(|i| {
            u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).map_err(|_| OrgError::BackendFailure {
                detail: format!("stored {what} is not hex"),
            })
        })
        .collect::<Result<Vec<u8>, OrgError>>()?;
    bytes.try_into().map_err(|_| OrgError::BackendFailure {
        detail: format!("stored {what} has wrong length"),
    })
}

fn blob<const N: usize>(raw: Vec<u8>, what: &str) -> Result<[u8; N], OrgError> {
    raw.try_into().map_err(|_| OrgError::BackendFailure {
        detail: format!("stored {what} has wrong length"),
    })
}

type AccountRow = (String, String, String, String, i64);
type OrganizationRow = (String, String, String, i64);
type TeamRow = (String, String, String, i64);
type MembershipRow = (String, String, String, Option<String>, String, String, i64);
type PolicyRow = (
    String,
    String,
    i64,
    String,
    String,
    Option<i64>,
    i64,
    String,
    i64,
);
type EnvelopeRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    i64,
);
type UsageRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    i64,
    i64,
    String,
    i64,
);

fn account_from(t: AccountRow) -> Result<AccountRecord, OrgError> {
    let a = AccountRecord {
        id: stored(AccountId::new(t.0), "account id")?,
        display_name: t.1,
        signing_public_key: hex_decode::<32>(&t.2, "signing public key")?,
        fingerprint: t.3,
        created_at: i64_to_u64(t.4, "created_at")?,
    };
    a.revalidate()?;
    Ok(a)
}

fn organization_from(t: OrganizationRow) -> Result<OrganizationRecord, OrgError> {
    let o = OrganizationRecord {
        id: stored(OrganizationId::new(t.0), "organization id")?,
        name: t.1,
        state: stored(OrganizationState::parse(&t.2), "organization state")?,
        created_at: i64_to_u64(t.3, "created_at")?,
    };
    o.revalidate()?;
    Ok(o)
}

fn team_from(t: TeamRow) -> Result<TeamRecord, OrgError> {
    let team = TeamRecord {
        id: stored(TeamId::new(t.0), "team id")?,
        org_id: stored(OrganizationId::new(t.1), "organization id")?,
        name: t.2,
        created_at: i64_to_u64(t.3, "created_at")?,
    };
    team.revalidate()?;
    Ok(team)
}

fn membership_from(t: MembershipRow) -> Result<MembershipRecord, OrgError> {
    let m = MembershipRecord {
        id: stored(MembershipId::new(t.0), "membership id")?,
        org_id: stored(OrganizationId::new(t.1), "organization id")?,
        account_id: stored(AccountId::new(t.2), "account id")?,
        team_id: t
            .3
            .map(|raw| stored(TeamId::new(raw), "team id"))
            .transpose()?,
        role: stored(Role::parse(&t.4), "role")?,
        state: stored(MembershipState::parse(&t.5), "membership state")?,
        created_at: i64_to_u64(t.6, "created_at")?,
    };
    m.revalidate()?;
    Ok(m)
}

fn policy_from(t: PolicyRow) -> Result<OrgPolicy, OrgError> {
    let p = OrgPolicy {
        id: stored(org::model::PolicyId::new(t.0), "policy id")?,
        org_id: stored(OrganizationId::new(t.1), "organization id")?,
        version: i64_to_u64(t.2, "version")?,
        hard_capability_ceiling: stored(CapabilityCeiling::parse(&t.3), "capability ceiling")?,
        sync_max_sensitivity: stored(Sensitivity::parse(&t.4), "sync sensitivity")?,
        quota_max_cost_microusd: t
            .5
            .map(|v| i64_to_u64(v, "quota_max_cost_microusd"))
            .transpose()?,
        quota_window_seconds: i64_to_u64(t.6, "quota_window_seconds")?,
        updated_by: stored(AccountId::new(t.7), "updated_by")?,
        updated_at: i64_to_u64(t.8, "updated_at")?,
    };
    p.revalidate()?;
    Ok(p)
}

fn envelope_from(t: EnvelopeRow) -> Result<SyncEnvelope, OrgError> {
    let e = SyncEnvelope {
        id: stored(EnvelopeId::new(t.0), "envelope id")?,
        org_id: stored(OrganizationId::new(t.1), "organization id")?,
        author_account_id: stored(AccountId::new(t.2), "author account id")?,
        kind: stored(EnvelopeKind::parse(&t.3), "envelope kind")?,
        revision: i64_to_u64(t.4, "revision")?,
        base_revision: i64_to_u64(t.5, "base_revision")?,
        max_sensitivity: stored(Sensitivity::parse(&t.6), "max sensitivity")?,
        cipher_suite: stored(CipherSuite::parse(&t.7), "cipher suite")?,
        nonce: blob::<24>(t.8, "nonce")?,
        ciphertext: t.9,
        signature: blob::<64>(t.10, "signature")?,
        created_at: i64_to_u64(t.11, "created_at")?,
    };
    e.revalidate()?;
    Ok(e)
}

fn usage_from(t: UsageRow) -> Result<UsageEntry, OrgError> {
    let u = UsageEntry {
        id: stored(UsageId::new(t.0), "usage id")?,
        org_id: stored(OrganizationId::new(t.1), "organization id")?,
        account_id: stored(AccountId::new(t.2), "account id")?,
        project_ref: t.3,
        kind: stored(UsageKind::parse(&t.4), "usage kind")?,
        units: i64_to_u64(t.5, "units")?,
        cost_microusd: i64_to_u64(t.6, "cost_microusd")?,
        idempotency_key: t.7,
        recorded_at: i64_to_u64(t.8, "recorded_at")?,
    };
    u.revalidate()?;
    Ok(u)
}

const ACCOUNT_COLUMNS: &str =
    "account_id, display_name, signing_public_key, fingerprint, created_at";
const ORGANIZATION_COLUMNS: &str = "organization_id, name, state, created_at";
const TEAM_COLUMNS: &str = "team_id, org_id, name, created_at";
const MEMBERSHIP_COLUMNS: &str =
    "membership_id, org_id, account_id, team_id, role, state, created_at";
const POLICY_COLUMNS: &str = "policy_id, org_id, version, capability_ceiling, \
    sync_max_sensitivity, quota_max_cost_microusd, quota_window_seconds, updated_by, updated_at";
const ENVELOPE_COLUMNS: &str = "envelope_id, org_id, author_account_id, kind, revision, \
    base_revision, max_sensitivity, cipher_suite, nonce, ciphertext, signature, created_at";
const USAGE_COLUMNS: &str = "usage_id, org_id, account_id, project_ref, kind, units, \
    cost_microusd, idempotency_key, recorded_at";

fn load_account(conn: &Connection, id: &AccountId) -> Result<Option<AccountRecord>, OrgError> {
    let row = conn
        .query_row(
            &format!("SELECT {ACCOUNT_COLUMNS} FROM org_accounts WHERE account_id = ?1"),
            params![id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(backend)?;
    row.map(account_from).transpose()
}

fn load_organization(
    conn: &Connection,
    id: &OrganizationId,
) -> Result<Option<OrganizationRecord>, OrgError> {
    let row = conn
        .query_row(
            &format!("SELECT {ORGANIZATION_COLUMNS} FROM organizations WHERE organization_id = ?1"),
            params![id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(backend)?;
    row.map(organization_from).transpose()
}

fn load_membership(
    conn: &Connection,
    id: &MembershipId,
) -> Result<Option<MembershipRecord>, OrgError> {
    let row = conn
        .query_row(
            &format!("SELECT {MEMBERSHIP_COLUMNS} FROM org_memberships WHERE membership_id = ?1"),
            params![id.as_str()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    row.map(membership_from).transpose()
}

fn load_envelope(conn: &Connection, id: &EnvelopeId) -> Result<Option<SyncEnvelope>, OrgError> {
    let row = conn
        .query_row(
            &format!("SELECT {ENVELOPE_COLUMNS} FROM org_sync_envelopes WHERE envelope_id = ?1"),
            params![id.as_str()],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    row.map(envelope_from).transpose()
}

impl OrgStorePort for SqliteStore {
    fn put_account(&mut self, a: &AccountRecord) -> Result<(), OrgError> {
        a.revalidate()?;
        if let Some(existing) = load_account(self.connection(), &a.id)? {
            if existing == *a {
                return Ok(());
            }
            return Err(OrgError::Conflict(a.id.to_string()));
        }
        self.connection()
            .execute(
                &format!("INSERT INTO org_accounts ({ACCOUNT_COLUMNS}) VALUES (?1,?2,?3,?4,?5)"),
                params![
                    a.id.as_str(),
                    a.display_name,
                    hex_encode(&a.signing_public_key),
                    a.fingerprint,
                    u64_to_i64(a.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_account(&self, id: &AccountId) -> Result<Option<AccountRecord>, OrgError> {
        load_account(self.connection(), id)
    }

    fn put_organization(&mut self, o: &OrganizationRecord) -> Result<(), OrgError> {
        o.revalidate()?;
        if let Some(existing) = load_organization(self.connection(), &o.id)? {
            if existing == *o {
                return Ok(());
            }
            return Err(OrgError::Conflict(o.id.to_string()));
        }
        self.connection()
            .execute(
                &format!("INSERT INTO organizations ({ORGANIZATION_COLUMNS}) VALUES (?1,?2,?3,?4)"),
                params![
                    o.id.as_str(),
                    o.name,
                    o.state.as_str(),
                    u64_to_i64(o.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_organization(
        &self,
        id: &OrganizationId,
    ) -> Result<Option<OrganizationRecord>, OrgError> {
        load_organization(self.connection(), id)
    }

    fn update_organization(&mut self, o: &OrganizationRecord) -> Result<(), OrgError> {
        o.revalidate()?;
        let n = self
            .connection()
            .execute(
                "UPDATE organizations SET name=?2, state=?3, created_at=?4 WHERE organization_id=?1",
                params![
                    o.id.as_str(),
                    o.name,
                    o.state.as_str(),
                    u64_to_i64(o.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(OrgError::NotFound(o.id.to_string()));
        }
        Ok(())
    }

    fn put_team(&mut self, t: &TeamRecord) -> Result<(), OrgError> {
        t.revalidate()?;
        if let Some(existing) = self.load_team(&t.id)? {
            if existing == *t {
                return Ok(());
            }
            return Err(OrgError::Conflict(t.id.to_string()));
        }
        self.connection()
            .execute(
                &format!("INSERT INTO org_teams ({TEAM_COLUMNS}) VALUES (?1,?2,?3,?4)"),
                params![
                    t.id.as_str(),
                    t.org_id.as_str(),
                    t.name,
                    u64_to_i64(t.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_team(&self, id: &TeamId) -> Result<Option<TeamRecord>, OrgError> {
        let row = self
            .connection()
            .query_row(
                &format!("SELECT {TEAM_COLUMNS} FROM org_teams WHERE team_id = ?1"),
                params![id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        row.map(team_from).transpose()
    }

    fn list_teams(&self, org_id: &OrganizationId) -> Result<Vec<TeamRecord>, OrgError> {
        let mut stmt = self
            .connection()
            .prepare(&format!(
                "SELECT {TEAM_COLUMNS} FROM org_teams WHERE org_id = ?1 ORDER BY team_id"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![org_id.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(team_from(r.map_err(backend)?)?);
        }
        Ok(out)
    }

    fn put_membership(&mut self, m: &MembershipRecord) -> Result<(), OrgError> {
        m.revalidate()?;
        if let Some(existing) = load_membership(self.connection(), &m.id)? {
            if existing == *m {
                return Ok(());
            }
            return Err(OrgError::Conflict(m.id.to_string()));
        }
        self.connection()
            .execute(
                &format!(
                    "INSERT INTO org_memberships ({MEMBERSHIP_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7)"
                ),
                params![
                    m.id.as_str(),
                    m.org_id.as_str(),
                    m.account_id.as_str(),
                    m.team_id.as_ref().map(|t| t.as_str()),
                    m.role.as_str(),
                    m.state.as_str(),
                    u64_to_i64(m.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_membership(&self, id: &MembershipId) -> Result<Option<MembershipRecord>, OrgError> {
        load_membership(self.connection(), id)
    }

    fn update_membership(&mut self, m: &MembershipRecord) -> Result<(), OrgError> {
        m.revalidate()?;
        let n = self
            .connection()
            .execute(
                "UPDATE org_memberships SET org_id=?2, account_id=?3, team_id=?4, role=?5, state=?6, created_at=?7 WHERE membership_id=?1",
                params![
                    m.id.as_str(),
                    m.org_id.as_str(),
                    m.account_id.as_str(),
                    m.team_id.as_ref().map(|t| t.as_str()),
                    m.role.as_str(),
                    m.state.as_str(),
                    u64_to_i64(m.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        if n == 0 {
            return Err(OrgError::NotFound(m.id.to_string()));
        }
        Ok(())
    }

    fn list_memberships(&self, org_id: &OrganizationId) -> Result<Vec<MembershipRecord>, OrgError> {
        let mut stmt = self
            .connection()
            .prepare(&format!(
                "SELECT {MEMBERSHIP_COLUMNS} FROM org_memberships WHERE org_id = ?1 ORDER BY membership_id"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![org_id.as_str()], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(membership_from(r.map_err(backend)?)?);
        }
        Ok(out)
    }

    fn put_policy(&mut self, p: &OrgPolicy) -> Result<(), OrgError> {
        p.revalidate()?;
        let existing = self
            .connection()
            .query_row(
                &format!(
                    "SELECT {POLICY_COLUMNS} FROM org_policies WHERE policy_id = ?1 AND version = ?2"
                ),
                params![p.id.as_str(), u64_to_i64(p.version, "version")?],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if let Some(row) = existing {
            if policy_from(row)? == *p {
                return Ok(());
            }
            return Err(OrgError::Conflict(p.id.to_string()));
        }
        self.connection()
            .execute(
                &format!(
                    "INSERT INTO org_policies ({POLICY_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
                ),
                params![
                    p.id.as_str(),
                    p.org_id.as_str(),
                    u64_to_i64(p.version, "version")?,
                    p.hard_capability_ceiling.as_str(),
                    p.sync_max_sensitivity.as_str(),
                    p.quota_max_cost_microusd
                        .map(|v| u64_to_i64(v, "quota_max_cost_microusd"))
                        .transpose()?,
                    u64_to_i64(p.quota_window_seconds, "quota_window_seconds")?,
                    p.updated_by.as_str(),
                    u64_to_i64(p.updated_at, "updated_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn current_policy(&self, org_id: &OrganizationId) -> Result<Option<OrgPolicy>, OrgError> {
        let row = self
            .connection()
            .query_row(
                &format!(
                    "SELECT {POLICY_COLUMNS} FROM org_policies WHERE org_id = ?1 ORDER BY version DESC LIMIT 1"
                ),
                params![org_id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        row.map(policy_from).transpose()
    }

    fn append_envelope(&mut self, e: &SyncEnvelope) -> Result<(), OrgError> {
        e.revalidate()?;
        if load_envelope(self.connection(), &e.id)?.is_some() {
            return Err(OrgError::DuplicateEnvelope(e.id.to_string()));
        }
        if e.revision <= self.sync_head(&e.org_id)? {
            return Err(OrgError::Conflict(format!(
                "revision {} is not above the current head",
                e.revision
            )));
        }
        self.connection()
            .execute(
                &format!(
                    "INSERT INTO org_sync_envelopes ({ENVELOPE_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
                ),
                params![
                    e.id.as_str(),
                    e.org_id.as_str(),
                    e.author_account_id.as_str(),
                    e.kind.as_str(),
                    u64_to_i64(e.revision, "revision")?,
                    u64_to_i64(e.base_revision, "base_revision")?,
                    e.max_sensitivity.as_str(),
                    e.cipher_suite.as_str(),
                    e.nonce.as_slice(),
                    e.ciphertext,
                    e.signature.as_slice(),
                    u64_to_i64(e.created_at, "created_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn load_envelope(&self, id: &EnvelopeId) -> Result<Option<SyncEnvelope>, OrgError> {
        load_envelope(self.connection(), id)
    }

    fn sync_head(&self, org_id: &OrganizationId) -> Result<u64, OrgError> {
        let head: Option<i64> = self
            .connection()
            .query_row(
                "SELECT MAX(revision) FROM org_sync_envelopes WHERE org_id = ?1",
                params![org_id.as_str()],
                |r| r.get(0),
            )
            .map_err(backend)?;
        match head {
            Some(v) => i64_to_u64(v, "revision"),
            None => Ok(0),
        }
    }

    fn append_usage(&mut self, e: &UsageEntry) -> Result<(), OrgError> {
        e.revalidate()?;
        let by_key = self
            .connection()
            .query_row(
                &format!(
                    "SELECT {USAGE_COLUMNS} FROM org_usage_entries WHERE org_id = ?1 AND idempotency_key = ?2"
                ),
                params![e.org_id.as_str(), e.idempotency_key],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if let Some(row) = by_key {
            if usage_from(row)? == *e {
                return Ok(());
            }
            return Err(OrgError::Conflict(e.idempotency_key.clone()));
        }
        let by_id: Option<UsageRow> = self
            .connection()
            .query_row(
                &format!("SELECT {USAGE_COLUMNS} FROM org_usage_entries WHERE usage_id = ?1"),
                params![e.id.as_str()],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                        r.get(8)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        if by_id.is_some() {
            return Err(OrgError::Conflict(e.id.to_string()));
        }
        self.connection()
            .execute(
                &format!(
                    "INSERT INTO org_usage_entries ({USAGE_COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
                ),
                params![
                    e.id.as_str(),
                    e.org_id.as_str(),
                    e.account_id.as_str(),
                    e.project_ref,
                    e.kind.as_str(),
                    u64_to_i64(e.units, "units")?,
                    u64_to_i64(e.cost_microusd, "cost_microusd")?,
                    e.idempotency_key,
                    u64_to_i64(e.recorded_at, "recorded_at")?,
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    fn list_usage(&self, org_id: &OrganizationId) -> Result<Vec<UsageEntry>, OrgError> {
        let mut stmt = self
            .connection()
            .prepare(&format!(
                "SELECT {USAGE_COLUMNS} FROM org_usage_entries WHERE org_id = ?1 ORDER BY recorded_at, usage_id"
            ))
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![org_id.as_str()], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(usage_from(r.map_err(backend)?)?);
        }
        Ok(out)
    }
}
