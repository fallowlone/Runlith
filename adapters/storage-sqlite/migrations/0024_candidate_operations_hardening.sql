-- TASK-0151 findings 1, 6, 14: candidate operation hardening.
--
-- Finding 14 (repair): migration 0022 was edited in place across review
-- rounds, gaining candidate_input_operations plus two triggers after some
-- development databases had already recorded user_version 22. Those
-- databases are missing the table (and its triggers) even though their
-- version claims 0022. The IF NOT EXISTS repair below recreates exactly
-- the 0022 shape for them; on healthy databases every statement is a
-- no-op. Historical migrations stay untouched per policy.
CREATE TABLE IF NOT EXISTS candidate_input_operations (
    execution_id TEXT NOT NULL REFERENCES executions(id),
    attempt_id TEXT NOT NULL REFERENCES execution_attempts(attempt_id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) BETWEEN 1 AND 1024),
    base_revision TEXT NOT NULL CHECK (length(base_revision) BETWEEN 1 AND 1024),
    selected_commit_oid TEXT NOT NULL CHECK (length(selected_commit_oid) IN (40,64)),
    requirement_revision TEXT NOT NULL CHECK (length(requirement_revision) <= 1024),
    policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
    policy_fingerprint TEXT NOT NULL CHECK (length(policy_fingerprint) <= 1024),
    criterion_id TEXT NOT NULL CHECK (length(criterion_id) <= 1024),
    selector TEXT NOT NULL CHECK (selector = 'candidate.file-contains.v1'),
    selector_path TEXT NOT NULL CHECK (length(selector_path) BETWEEN 1 AND 4096),
    selector_literal TEXT NOT NULL CHECK (length(selector_literal) BETWEEN 1 AND 4096),
    status TEXT NOT NULL CHECK (status IN ('PENDING','RESULT_ADMITTED','MISMATCH','BLOCKED','TAMPERED','COMPLETE')),
    detail_code TEXT CHECK (detail_code IS NULL OR length(detail_code) <= 128),
    status_history TEXT NOT NULL DEFAULT 'PENDING' CHECK (length(status_history) BETWEEN 1 AND 8192),
    invalidated_at_ms INTEGER CHECK (invalidated_at_ms IS NULL OR invalidated_at_ms >= 0),
    invalidation_code TEXT CHECK (invalidation_code IS NULL OR length(invalidation_code) BETWEEN 1 AND 256),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= started_at_ms),
    PRIMARY KEY(execution_id, attempt_id),
    CHECK ((invalidated_at_ms IS NULL AND invalidation_code IS NULL) OR (invalidated_at_ms IS NOT NULL AND invalidation_code IS NOT NULL))
);
CREATE TRIGGER IF NOT EXISTS candidate_input_operations_status_no_regression
BEFORE UPDATE OF status ON candidate_input_operations
WHEN OLD.status IN ('RESULT_ADMITTED', 'COMPLETE')
    AND NEW.status NOT IN ('RESULT_ADMITTED', 'COMPLETE')
BEGIN
    SELECT RAISE(ABORT, 'candidate operation status must not regress from RESULT_ADMITTED or COMPLETE');
END;
CREATE TRIGGER IF NOT EXISTS candidate_input_operations_invalidation_sticky
BEFORE UPDATE OF invalidated_at_ms, invalidation_code ON candidate_input_operations
WHEN OLD.invalidated_at_ms IS NOT NULL
    AND (NEW.invalidated_at_ms IS NOT OLD.invalidated_at_ms OR NEW.invalidation_code IS NOT OLD.invalidation_code)
BEGIN
    SELECT RAISE(ABORT, 'candidate operation invalidation is permanent');
END;

-- Finding 1: operation rows are permanent audit records (the sticky
-- comment above already assumes "until the row is deleted (which policy
-- forbids)"). Enforce the policy at the store level.
CREATE TRIGGER candidate_input_operations_no_delete
BEFORE DELETE ON candidate_input_operations
BEGIN
    SELECT RAISE(ABORT, 'candidate operation rows cannot be deleted');
END;

-- Finding 6: an assessment row cannot land without its selection
-- (operation) row. SQLite cannot ADD a foreign key to an existing
-- table, so the binding is a trigger: same keys, same guarantee.
CREATE TRIGGER candidate_input_assessments_require_selection
BEFORE INSERT ON candidate_input_assessments
WHEN NOT EXISTS (
    SELECT 1 FROM candidate_input_operations
    WHERE execution_id = NEW.execution_id AND attempt_id = NEW.attempt_id
)
BEGIN
    SELECT RAISE(ABORT, 'candidate assessment requires its selection row');
END;
