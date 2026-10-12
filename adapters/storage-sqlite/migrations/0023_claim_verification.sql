-- TASK-0125: durable G8 claim-verification seam (ADR-0030 §2/§4/§5).
-- Agent claims, claim verifications, policy exemptions, human-approval
-- discharges, and completion-admission records. Append-only: supersession
-- is by reference (claim_verifications.superseded_by), single-use is a
-- guarded consume (approval_discharges.consumed_by); nothing is deleted.
-- Closed vocabularies are enforced fail-closed in the schema.
CREATE TABLE agent_claims (
    claim_id TEXT PRIMARY KEY CHECK (length(trim(claim_id)) > 0 AND length(claim_id) <= 1024),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    criterion_id TEXT NOT NULL CHECK (length(trim(criterion_id)) > 0 AND length(criterion_id) <= 1024),
    claimed_outcome TEXT NOT NULL CHECK (claimed_outcome IN ('SATISFIED','REFUTED','UNKNOWN')),
    subject_ref TEXT NOT NULL CHECK (length(trim(subject_ref)) > 0 AND length(subject_ref) <= 1024),
    subject_fingerprint TEXT NOT NULL CHECK (subject_fingerprint GLOB 'sha256:*' AND length(subject_fingerprint) = 71),
    receipt_refs TEXT NOT NULL CHECK (length(receipt_refs) <= 4096),
    effect_refs TEXT NOT NULL CHECK (length(effect_refs) <= 4096),
    session_refs TEXT NOT NULL CHECK (length(session_refs) <= 4096),
    asserted_by TEXT NOT NULL CHECK (length(trim(asserted_by)) > 0 AND length(asserted_by) <= 1024),
    asserted_at TEXT NOT NULL CHECK (length(trim(asserted_at)) > 0 AND length(asserted_at) <= 1024),
    CHECK (instr(criterion_id, char(10)) = 0 AND instr(asserted_by, char(10)) = 0)
);
CREATE INDEX agent_claims_task_idx ON agent_claims(task_id, criterion_id);

CREATE TABLE claim_verifications (
    verification_id TEXT PRIMARY KEY CHECK (length(trim(verification_id)) > 0 AND length(verification_id) <= 1024),
    claim_id TEXT NOT NULL REFERENCES agent_claims(claim_id),
    -- CV-2: only the trusted control-plane verifier identity may author a
    -- verdict row; agents/providers can never hold it.
    verifier TEXT NOT NULL CHECK (verifier = 'runlith.claim-verifier.v1'),
    requirement_kind TEXT NOT NULL CHECK (requirement_kind IN ('process-exit','test-suite','artifact-hash','fs-state','git-state','independent-review','security-check','human-approval')),
    subject_fingerprint TEXT NOT NULL CHECK (subject_fingerprint GLOB 'sha256:*' AND length(subject_fingerprint) = 71),
    -- ADR-0032 rule 4: the exact requirement revision and policy
    -- fingerprint this verdict binds. Revision is a content hash by
    -- definition; the policy value stays free-form bounded text (as in
    -- policy_exemptions) with shape judged at admission.
    requirement_revision TEXT NOT NULL CHECK (requirement_revision GLOB 'sha256:*' AND length(requirement_revision) = 71),
    policy_fingerprint TEXT NULL CHECK (policy_fingerprint IS NULL OR (length(trim(policy_fingerprint)) > 0 AND length(policy_fingerprint) <= 1024)),
    verdict TEXT NOT NULL CHECK (verdict IN ('VERIFIED','REFUTED','UNVERIFIED','BLOCKED','EXEMPT')),
    receipt_refs TEXT NOT NULL CHECK (length(receipt_refs) <= 4096),
    effect_refs TEXT NOT NULL CHECK (length(effect_refs) <= 4096),
    session_refs TEXT NOT NULL CHECK (length(session_refs) <= 4096),
    -- CV-5: supersession by reference, never deletion.
    superseded_by TEXT NULL REFERENCES claim_verifications(verification_id),
    decided_at TEXT NOT NULL CHECK (length(trim(decided_at)) > 0 AND length(decided_at) <= 1024),
    -- ADR-0032 rule 4: VERIFIED requires a nonempty effect basis.
    CHECK (verdict != 'VERIFIED' OR length(trim(effect_refs)) > 0)
);
CREATE INDEX claim_verifications_claim_idx ON claim_verifications(claim_id);

CREATE TABLE policy_exemptions (
    exemption_id TEXT PRIMARY KEY CHECK (length(trim(exemption_id)) > 0 AND length(exemption_id) <= 1024),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    criterion_id TEXT NULL CHECK (criterion_id IS NULL OR (length(trim(criterion_id)) > 0 AND length(criterion_id) <= 1024)),
    subject_fingerprint TEXT NOT NULL CHECK (subject_fingerprint GLOB 'sha256:*' AND length(subject_fingerprint) = 71),
    policy_fingerprint TEXT NOT NULL CHECK (length(trim(policy_fingerprint)) > 0 AND length(policy_fingerprint) <= 1024),
    -- CV-4: the typed RiskClass Low gate, repeated fail-closed in the
    -- schema; no other risk class can ever be stored.
    risk_class TEXT NOT NULL CHECK (risk_class = 'low'),
    created_at TEXT NOT NULL CHECK (length(trim(created_at)) > 0 AND length(created_at) <= 1024)
);
CREATE INDEX policy_exemptions_task_idx ON policy_exemptions(task_id);

CREATE TABLE approval_discharges (
    discharge_id TEXT PRIMARY KEY CHECK (length(trim(discharge_id)) > 0 AND length(discharge_id) <= 1024),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    criterion_id TEXT NOT NULL CHECK (length(trim(criterion_id)) > 0 AND length(criterion_id) <= 1024),
    subject_fingerprint TEXT NOT NULL CHECK (subject_fingerprint GLOB 'sha256:*' AND length(subject_fingerprint) = 71),
    risk_class TEXT NOT NULL CHECK (risk_class IN ('low','medium','high','critical')),
    approval_ref TEXT NOT NULL CHECK (length(trim(approval_ref)) > 0 AND length(approval_ref) <= 1024),
    not_valid_before TEXT NOT NULL CHECK (length(trim(not_valid_before)) > 0 AND length(not_valid_before) <= 1024),
    not_valid_after TEXT NOT NULL CHECK (length(trim(not_valid_after)) > 0 AND length(not_valid_after) <= 1024),
    -- CV-8: single-use per (criterion, subject fingerprint); set once by
    -- the completion commit that consumed the discharge.
    consumed_by TEXT NULL REFERENCES completion_admissions(admission_id),
    created_at TEXT NOT NULL CHECK (length(trim(created_at)) > 0 AND length(created_at) <= 1024)
);
CREATE INDEX approval_discharges_task_idx ON approval_discharges(task_id, criterion_id);

CREATE TABLE completion_admissions (
    admission_id TEXT PRIMARY KEY CHECK (length(trim(admission_id)) > 0 AND length(admission_id) <= 1024),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    -- Only admitted completions are recorded; refusals are derivable from
    -- the durable verdicts and never reach this table.
    decision TEXT NOT NULL CHECK (decision = 'ADMIT'),
    policy_fingerprint TEXT NULL CHECK (policy_fingerprint IS NULL OR length(policy_fingerprint) <= 1024),
    outcomes TEXT NOT NULL CHECK (length(outcomes) <= 16384),
    decided_at TEXT NOT NULL CHECK (length(trim(decided_at)) > 0 AND length(decided_at) <= 1024)
);
CREATE INDEX completion_admissions_task_idx ON completion_admissions(task_id);
