-- 0025: ADR-0033 review authority durability (TASK-0145).
--
-- Owner presence proofs bind one exact operation each; review-path
-- settings are per-task revisioned history consuming proofs; reviewer
-- profiles are product-managed; review records bind every ADR-0033
-- field with proof-checked nonces, linear supersession, and sticky
-- invalidation. All history is append-only (no UPDATE/DELETE except
-- the single invalidation/supersession transitions).

CREATE TABLE owner_presence_proofs (
    nonce TEXT PRIMARY KEY CHECK (length(nonce) BETWEEN 1 AND 256),
    operation TEXT NOT NULL CHECK (length(operation) BETWEEN 1 AND 1024),
    input_digest TEXT NOT NULL CHECK (input_digest GLOB 'sha256:*' AND length(input_digest) = 71),
    proven_at_ms INTEGER NOT NULL CHECK (proven_at_ms >= 0)
) STRICT;

CREATE TRIGGER owner_presence_proof_no_update
BEFORE UPDATE ON owner_presence_proofs
BEGIN
    SELECT RAISE(ABORT, 'owner presence proofs are immutable');
END;

CREATE TRIGGER owner_presence_proof_no_delete
BEFORE DELETE ON owner_presence_proofs
BEGIN
    SELECT RAISE(ABORT, 'owner presence proofs are immutable');
END;

CREATE TABLE review_path_settings (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    setting_revision INTEGER NOT NULL CHECK (setting_revision > 0),
    path TEXT NOT NULL CHECK (path IN ('review-path/v1:b1', 'review-path/v1:b2')),
    fingerprint TEXT NOT NULL CHECK (fingerprint GLOB 'sha256:*' AND length(fingerprint) = 71),
    changed_at_ms INTEGER NOT NULL CHECK (changed_at_ms >= 0),
    presence_nonce TEXT NOT NULL UNIQUE REFERENCES owner_presence_proofs(nonce),
    PRIMARY KEY (task_id, setting_revision)
) STRICT;

CREATE TRIGGER review_path_setting_no_update
BEFORE UPDATE ON review_path_settings
BEGIN
    SELECT RAISE(ABORT, 'review path settings are immutable');
END;

CREATE TRIGGER review_path_setting_no_delete
BEFORE DELETE ON review_path_settings
BEGIN
    SELECT RAISE(ABORT, 'review path settings are immutable');
END;

-- The consumed proof must bind this exact setting change.
CREATE TRIGGER review_path_setting_proof_operation
BEFORE INSERT ON review_path_settings
WHEN (SELECT operation FROM owner_presence_proofs WHERE nonce = NEW.presence_nonce)
    IS NOT ('review-setting-change:rev=' || NEW.setting_revision || ':path=' || NEW.path)
BEGIN
    SELECT RAISE(ABORT, 'setting proof binds a different operation');
END;

CREATE TABLE reviewer_profiles (
    profile_id TEXT PRIMARY KEY CHECK (length(profile_id) BETWEEN 1 AND 256),
    profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
    key_ref TEXT NOT NULL CHECK (length(key_ref) BETWEEN 1 AND 256),
    fingerprint TEXT NOT NULL CHECK (fingerprint GLOB 'sha256:*' AND length(fingerprint) = 71)
) STRICT;

CREATE TRIGGER reviewer_profile_no_update
BEFORE UPDATE ON reviewer_profiles
BEGIN
    SELECT RAISE(ABORT, 'reviewer profiles are immutable');
END;

CREATE TRIGGER reviewer_profile_no_delete
BEFORE DELETE ON reviewer_profiles
BEGIN
    SELECT RAISE(ABORT, 'reviewer profiles are immutable');
END;

CREATE TABLE review_records (
    record_id TEXT PRIMARY KEY CHECK (length(record_id) BETWEEN 1 AND 256),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    requirement_revision TEXT NOT NULL CHECK (length(requirement_revision) BETWEEN 1 AND 1024),
    base_manifest TEXT NOT NULL CHECK (base_manifest GLOB 'sha256:*' AND length(base_manifest) = 71),
    candidate_manifest TEXT NOT NULL CHECK (candidate_manifest GLOB 'sha256:*' AND length(candidate_manifest) = 71),
    changed_inputs TEXT NOT NULL CHECK (length(changed_inputs) BETWEEN 1 AND 65536),
    affected_checks TEXT NOT NULL CHECK (length(affected_checks) BETWEEN 1 AND 65536),
    policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
    policy_fingerprint TEXT NOT NULL CHECK (policy_fingerprint GLOB 'sha256:*' AND length(policy_fingerprint) = 71),
    profile_id TEXT NOT NULL REFERENCES reviewer_profiles(profile_id),
    profile_revision INTEGER NOT NULL CHECK (profile_revision > 0),
    setting_revision INTEGER NOT NULL CHECK (setting_revision > 0),
    setting_path TEXT NOT NULL CHECK (setting_path IN ('review-path/v1:b1', 'review-path/v1:b2')),
    reviewer_namespace TEXT NOT NULL CHECK (reviewer_namespace = 'human/v1'),
    reviewer_nonce TEXT NOT NULL UNIQUE REFERENCES owner_presence_proofs(nonce),
    reviewer_input_digest TEXT NOT NULL CHECK (reviewer_input_digest GLOB 'sha256:*' AND length(reviewer_input_digest) = 71),
    reviewer_verdict TEXT NOT NULL CHECK (reviewer_verdict IN ('approved', 'rejected')),
    implementer_execution_id TEXT NOT NULL REFERENCES executions(id),
    implementer_attempt_id TEXT NOT NULL REFERENCES execution_attempts(attempt_id),
    implementer_admission_ref TEXT NOT NULL CHECK (length(implementer_admission_ref) BETWEEN 1 AND 256),
    rationale TEXT NOT NULL CHECK (length(rationale) <= 4096),
    evidence_refs TEXT NOT NULL CHECK (length(evidence_refs) <= 32768),
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    supersedes TEXT REFERENCES review_records(record_id),
    fingerprint TEXT NOT NULL CHECK (fingerprint GLOB 'sha256:*' AND length(fingerprint) = 71),
    invalidated_at_ms INTEGER CHECK (invalidated_at_ms IS NULL OR invalidated_at_ms >= 0),
    invalidation_code TEXT CHECK (invalidation_code IS NULL OR length(invalidation_code) BETWEEN 1 AND 256),
    superseded_by TEXT,
    FOREIGN KEY (task_id, setting_revision) REFERENCES review_path_settings(task_id, setting_revision)
) STRICT;

CREATE INDEX review_records_by_task ON review_records(task_id);

CREATE TRIGGER review_record_no_delete
BEFORE DELETE ON review_records
BEGIN
    SELECT RAISE(ABORT, 'review records are immutable');
END;

-- Record bodies are immutable: only the single invalidation
-- transition and the single supersession link may land, each at most
-- once (sticky triggers below, mirroring the candidate operations
-- pattern from 0022/0024).
CREATE TRIGGER review_record_body_immutable
BEFORE UPDATE OF record_id, task_id, requirement_revision, base_manifest,
    candidate_manifest, changed_inputs, affected_checks, policy_revision,
    policy_fingerprint, profile_id, profile_revision, setting_revision,
    setting_path, reviewer_namespace, reviewer_nonce, reviewer_input_digest,
    reviewer_verdict, implementer_execution_id, implementer_attempt_id,
    implementer_admission_ref, rationale, evidence_refs, captured_at_ms,
    supersedes, fingerprint ON review_records
BEGIN
    SELECT RAISE(ABORT, 'review records are immutable');
END;

CREATE TRIGGER review_record_invalidation_sticky
BEFORE UPDATE OF invalidated_at_ms, invalidation_code ON review_records
WHEN OLD.invalidated_at_ms IS NOT NULL
    AND (NEW.invalidated_at_ms IS NOT OLD.invalidated_at_ms OR NEW.invalidation_code IS NOT OLD.invalidation_code)
BEGIN
    SELECT RAISE(ABORT, 'review record invalidation is permanent');
END;

CREATE TRIGGER review_record_supersede_once
BEFORE UPDATE OF superseded_by ON review_records
WHEN OLD.superseded_by IS NOT NULL AND NEW.superseded_by IS NOT OLD.superseded_by
BEGIN
    SELECT RAISE(ABORT, 'review record supersession is single');
END;

-- The consumed proof must bind this exact review record.
CREATE TRIGGER review_record_proof_operation
BEFORE INSERT ON review_records
WHEN (SELECT operation FROM owner_presence_proofs WHERE nonce = NEW.reviewer_nonce)
    IS NOT ('b1-review:record=' || NEW.record_id)
BEGIN
    SELECT RAISE(ABORT, 'review proof binds a different operation');
END;

-- A superseding record links its parent exactly once: double
-- supersession of the same parent refuses (linear history).
CREATE TRIGGER review_record_supersede_parent
AFTER INSERT ON review_records
WHEN NEW.supersedes IS NOT NULL
BEGIN
    UPDATE review_records SET superseded_by = NEW.record_id WHERE record_id = NEW.supersedes;
END;
