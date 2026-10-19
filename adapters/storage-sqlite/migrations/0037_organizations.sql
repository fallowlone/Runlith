-- 0037: G13 Cloud/Team organizations (TASK-0209, ADR-0047 §9).
CREATE TABLE org_accounts (
    account_id TEXT PRIMARY KEY CHECK (account_id GLOB 'acc_*' AND length(account_id) = 30),
    display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 1024),
    signing_public_key TEXT NOT NULL CHECK (length(signing_public_key) = 64),
    fingerprint TEXT NOT NULL CHECK (fingerprint GLOB 'sha256:*' AND length(fingerprint) = 71),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT;

CREATE TABLE organizations (
    organization_id TEXT PRIMARY KEY CHECK (organization_id GLOB 'org_*' AND length(organization_id) = 30),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 1024),
    state TEXT NOT NULL CHECK (state IN ('ACTIVE', 'ARCHIVED')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT;

CREATE TABLE org_teams (
    team_id TEXT PRIMARY KEY CHECK (team_id GLOB 'team_*' AND length(team_id) = 31),
    org_id TEXT NOT NULL REFERENCES organizations(organization_id),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 1024),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT;

CREATE TABLE org_memberships (
    membership_id TEXT PRIMARY KEY CHECK (membership_id GLOB 'mem_*' AND length(membership_id) = 30),
    org_id TEXT NOT NULL REFERENCES organizations(organization_id),
    account_id TEXT NOT NULL REFERENCES org_accounts(account_id),
    team_id TEXT REFERENCES org_teams(team_id),
    role TEXT NOT NULL CHECK (role IN ('OWNER', 'ADMIN', 'MEMBER', 'VIEWER')),
    state TEXT NOT NULL CHECK (state IN ('ACTIVE', 'REVOKED')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT;

CREATE TABLE org_policies (
    policy_id TEXT NOT NULL CHECK (policy_id GLOB 'pol_*' AND length(policy_id) = 30),
    org_id TEXT NOT NULL REFERENCES organizations(organization_id),
    version INTEGER NOT NULL CHECK (version >= 1),
    capability_ceiling TEXT NOT NULL CHECK (capability_ceiling IN ('READ_ONLY', 'WORKSPACE_WRITE', 'EXECUTION')),
    sync_max_sensitivity TEXT NOT NULL CHECK (sync_max_sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')),
    quota_max_cost_microusd INTEGER CHECK (quota_max_cost_microusd IS NULL OR quota_max_cost_microusd >= 0),
    quota_window_seconds INTEGER NOT NULL CHECK (quota_window_seconds >= 1),
    updated_by TEXT NOT NULL REFERENCES org_accounts(account_id),
    updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
    PRIMARY KEY (policy_id, version),
    UNIQUE (org_id, version)
) STRICT;

CREATE TABLE org_sync_envelopes (
    envelope_id TEXT PRIMARY KEY CHECK (envelope_id GLOB 'env_*' AND length(envelope_id) = 30),
    org_id TEXT NOT NULL REFERENCES organizations(organization_id),
    author_account_id TEXT NOT NULL REFERENCES org_accounts(account_id),
    kind TEXT NOT NULL CHECK (kind IN ('ORG_PROFILE', 'TEAM', 'MEMBERSHIP', 'ORG_POLICY', 'USAGE_SNAPSHOT')),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    base_revision INTEGER NOT NULL CHECK (base_revision >= 0),
    max_sensitivity TEXT NOT NULL CHECK (max_sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')),
    cipher_suite TEXT NOT NULL CHECK (cipher_suite IN ('XCHACHA20_POLY1305_ED25519')),
    nonce BLOB NOT NULL CHECK (length(nonce) = 24),
    ciphertext BLOB NOT NULL CHECK (length(ciphertext) BETWEEN 16 AND 1048592),
    signature BLOB NOT NULL CHECK (length(signature) = 64),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    UNIQUE (org_id, revision)
) STRICT;

CREATE TABLE org_usage_entries (
    usage_id TEXT PRIMARY KEY CHECK (usage_id GLOB 'use_*' AND length(usage_id) = 30),
    org_id TEXT NOT NULL REFERENCES organizations(organization_id),
    account_id TEXT NOT NULL REFERENCES org_accounts(account_id),
    project_ref TEXT CHECK (project_ref IS NULL OR length(project_ref) BETWEEN 1 AND 1024),
    kind TEXT NOT NULL CHECK (kind IN ('EXECUTION', 'PLANNING', 'EVALUATION')),
    units INTEGER NOT NULL CHECK (units >= 0),
    cost_microusd INTEGER NOT NULL CHECK (cost_microusd >= 0),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 1024),
    recorded_at INTEGER NOT NULL CHECK (recorded_at >= 0),
    UNIQUE (org_id, idempotency_key)
) STRICT;

CREATE INDEX org_teams_org_idx ON org_teams (org_id, team_id);
CREATE INDEX org_memberships_org_idx ON org_memberships (org_id, membership_id);
CREATE INDEX org_policies_org_idx ON org_policies (org_id, version);
CREATE INDEX org_usage_org_idx ON org_usage_entries (org_id, recorded_at, usage_id);
