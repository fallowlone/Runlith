-- TASK-0123: trusted planning-input snapshots bind the full optional Task
-- EvidenceRequirement, not only its lossy four-value plan method.
ALTER TABLE planning_input_criteria ADD COLUMN requirement_kind TEXT NULL CHECK (requirement_kind IS NULL OR requirement_kind IN ('process-exit', 'test-suite', 'artifact-hash', 'fs-state', 'git-state', 'independent-review', 'security-check', 'human-approval'));
ALTER TABLE planning_input_criteria ADD COLUMN subject_binding TEXT NULL CHECK (subject_binding IS NULL OR subject_binding IN ('content', 'content+toolchain', 'content+toolchain+config'));
ALTER TABLE planning_input_criteria ADD COLUMN requirement_required INTEGER NULL CHECK (requirement_required IS NULL OR requirement_required IN (0, 1));
ALTER TABLE planning_input_criteria ADD COLUMN requirement_policy_fingerprint TEXT NULL CHECK (requirement_policy_fingerprint IS NULL OR (length(trim(requirement_policy_fingerprint)) > 0 AND length(requirement_policy_fingerprint) <= 256));

-- Legacy planning snapshots already carry the closed four-value verification
-- method vocabulary. Preserve that obligation explicitly when introducing the
-- typed requirement columns; migrated rows have no policy derivation identity,
-- so the fingerprint deliberately remains NULL (stale input until refreshed).
UPDATE planning_input_criteria
SET requirement_kind = CASE verification_method
        WHEN 'automated-test' THEN 'test-suite'
        WHEN 'independent-review' THEN 'independent-review'
        WHEN 'security-check' THEN 'security-check'
        WHEN 'human-approval' THEN 'human-approval'
        ELSE '__invalid_verification_method__'
    END,
    subject_binding = 'content+toolchain+config',
    requirement_required = 1,
    requirement_policy_fingerprint = NULL;

CREATE TABLE planning_input_criterion_requirement_params (
    snapshot_id TEXT NOT NULL,
    criterion_position INTEGER NOT NULL CHECK (criterion_position >= 0),
    position INTEGER NOT NULL CHECK (position >= 0),
    param_key TEXT NOT NULL CHECK (length(trim(param_key)) > 0),
    param_value TEXT NOT NULL CHECK (length(trim(param_value)) > 0),
    PRIMARY KEY (snapshot_id, criterion_position, position),
    UNIQUE (snapshot_id, criterion_position, param_key),
    FOREIGN KEY (snapshot_id, criterion_position) REFERENCES planning_input_criteria (snapshot_id, position)
);
