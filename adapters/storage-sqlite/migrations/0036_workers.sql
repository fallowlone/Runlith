-- 0036: G12 Remote Worker (TASK-0200, ADR-0046).
CREATE TABLE worker_devices (
    device_id TEXT PRIMARY KEY CHECK (device_id GLOB 'dev_*' AND length(device_id) = 30),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 1024),
    public_key TEXT NOT NULL CHECK (length(public_key) = 64),
    fingerprint TEXT NOT NULL CHECK (fingerprint GLOB 'sha256:*' AND length(fingerprint) = 71),
    state TEXT NOT NULL CHECK (state IN ('ENROLLED', 'REVOKED')),
    enrolled_by TEXT NOT NULL CHECK (length(enrolled_by) BETWEEN 1 AND 1024),
    enrolled_at INTEGER NOT NULL CHECK (enrolled_at >= 0)
) STRICT;

CREATE TABLE worker_jobs (
    job_id TEXT PRIMARY KEY CHECK (job_id GLOB 'job_*' AND length(job_id) = 30),
    objective TEXT NOT NULL CHECK (length(objective) BETWEEN 1 AND 16384),
    capability_scope TEXT NOT NULL CHECK (capability_scope IN ('READ_ONLY', 'WORKSPACE_WRITE', 'EXECUTION')),
    authorized_by TEXT NOT NULL CHECK (length(authorized_by) BETWEEN 1 AND 1024),
    status TEXT NOT NULL CHECK (status IN ('OFFERED', 'CLAIMED', 'COMPLETED', 'FAILED', 'BLOCKED')),
    claimed_by TEXT REFERENCES worker_devices(device_id),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= created_at),
    evidence_ref TEXT CHECK (evidence_ref IS NULL OR length(evidence_ref) BETWEEN 1 AND 1024)
) STRICT;

CREATE TABLE worker_challenges (
    challenge_id TEXT PRIMARY KEY CHECK (challenge_id GLOB 'chl_*' AND length(challenge_id) = 30),
    device_id TEXT NOT NULL REFERENCES worker_devices(device_id),
    job_id TEXT NOT NULL REFERENCES worker_jobs(job_id),
    nonce TEXT NOT NULL CHECK (length(nonce) = 64),
    used INTEGER NOT NULL CHECK (used IN (0, 1)),
    created_at INTEGER NOT NULL CHECK (created_at >= 0)
) STRICT;

CREATE INDEX worker_jobs_status_idx ON worker_jobs (status, job_id);
