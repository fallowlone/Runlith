CREATE TABLE candidate_input_assessments (
    assessment_id TEXT PRIMARY KEY CHECK (length(trim(assessment_id)) > 0 AND length(assessment_id) <= 1024),
    effect_id TEXT NOT NULL UNIQUE REFERENCES observed_effects(effect_id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    execution_id TEXT NOT NULL REFERENCES executions(id),
    attempt_id TEXT NOT NULL REFERENCES execution_attempts(attempt_id),
    subject_ref TEXT NOT NULL CHECK (length(trim(subject_ref)) > 0 AND length(subject_ref) <= 1024),
    requirement_revision TEXT NOT NULL CHECK (length(requirement_revision) <= 1024),
    policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
    policy_fingerprint TEXT NOT NULL CHECK (length(policy_fingerprint) <= 1024),
    criterion_id TEXT NOT NULL CHECK (length(criterion_id) <= 1024),
    selector TEXT NOT NULL CHECK (selector = 'candidate.file-contains.v1'),
    selector_path TEXT NOT NULL CHECK (length(selector_path) BETWEEN 1 AND 4096),
    selector_literal TEXT NOT NULL CHECK (length(selector_literal) BETWEEN 1 AND 4096),
    invocation_id TEXT NOT NULL CHECK (length(invocation_id) <= 1024),
    evaluator_ref TEXT NOT NULL CHECK (length(evaluator_ref) <= 1024),
    runtime_ref TEXT NOT NULL CHECK (length(runtime_ref) <= 1024),
    enforcement_ref TEXT NOT NULL CHECK (length(enforcement_ref) <= 1024),
    base_commit_oid TEXT NOT NULL CHECK (length(base_commit_oid) IN (40,64)),
    base_tree_oid TEXT NOT NULL CHECK (length(base_tree_oid) IN (40,64)),
    base_manifest_sha256 TEXT NOT NULL CHECK (base_manifest_sha256 GLOB 'sha256:*' AND length(base_manifest_sha256) = 71),
    candidate_commit_oid TEXT NOT NULL CHECK (length(candidate_commit_oid) IN (40,64)),
    candidate_tree_oid TEXT NOT NULL CHECK (length(candidate_tree_oid) IN (40,64)),
    candidate_manifest_sha256 TEXT NOT NULL CHECK (candidate_manifest_sha256 GLOB 'sha256:*' AND length(candidate_manifest_sha256) = 71),
    checkpoints TEXT NOT NULL CHECK (length(checkpoints) > 0 AND length(checkpoints) <= 65536),
    integrity_outcome TEXT NOT NULL CHECK (integrity_outcome IN ('INTACT','MISMATCH')),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    completed_at_ms INTEGER NOT NULL CHECK (completed_at_ms >= started_at_ms),
    deletions TEXT NOT NULL CHECK (length(deletions) <= 4194304),
    invocation_descriptor_version INTEGER NOT NULL CHECK (invocation_descriptor_version = 1),
    invocation_descriptor_sha256 TEXT NOT NULL CHECK (invocation_descriptor_sha256 GLOB 'sha256:*' AND length(invocation_descriptor_sha256) = 71),
    evaluator_executable TEXT NOT NULL CHECK (length(evaluator_executable) BETWEEN 1 AND 4096),
    toolchain_ref TEXT NOT NULL CHECK (length(toolchain_ref) BETWEEN 1 AND 1024),
    runner_backend TEXT NOT NULL CHECK (length(runner_backend) BETWEEN 1 AND 256),
    runner_plan_sha256 TEXT NOT NULL CHECK (runner_plan_sha256 GLOB 'sha256:*' AND length(runner_plan_sha256) = 71),
    runner_enforced_axes INTEGER NOT NULL CHECK (runner_enforced_axes BETWEEN 3 AND 7),
    runner_env_isolated INTEGER NOT NULL CHECK (runner_env_isolated = 1)
);
CREATE INDEX candidate_input_assessments_task_idx ON candidate_input_assessments(task_id, execution_id, attempt_id, completed_at_ms);

CREATE TABLE candidate_input_operations (
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
-- R2 (owner review 2026-09-25, MAJOR): DB-level backstop for the
-- application-layer compare-and-swap guards in candidate_assessment_store.rs
-- (update_candidate_operation_status, record_candidate_operation_failure).
-- No writer — present or future — may regress status away from
-- RESULT_ADMITTED or COMPLETE once reached; a crash-window failure must be
-- journaled (status_history) without state regression, never silently
-- overwrite an already-admitted or already-completed row.
CREATE TRIGGER candidate_input_operations_status_no_regression
BEFORE UPDATE OF status ON candidate_input_operations
WHEN OLD.status IN ('RESULT_ADMITTED', 'COMPLETE')
    AND NEW.status NOT IN ('RESULT_ADMITTED', 'COMPLETE')
BEGIN
    SELECT RAISE(ABORT, 'candidate operation status must not regress from RESULT_ADMITTED or COMPLETE');
END;

-- Sticky invalidation (TASK-0144, owner decision 2026-09-25): once
-- invalidated_at_ms and invalidation_code are set (indicating a detected
-- tamper or mismatch after admission), they cannot be changed or cleared.
-- A permanent record of the invalidation fact is guaranteed to exist until
-- the row is deleted (which policy forbids). Combined with evidence-getter
-- checks, this makes the evidence stale/blocked and prevents reuse or
-- completion. Invalidation is independent of status: status remains
-- monotonic (R2), while invalidation is the sticky barrier.
CREATE TRIGGER candidate_input_operations_invalidation_sticky
BEFORE UPDATE OF invalidated_at_ms, invalidation_code ON candidate_input_operations
WHEN OLD.invalidated_at_ms IS NOT NULL
    AND (NEW.invalidated_at_ms IS NOT OLD.invalidated_at_ms OR NEW.invalidation_code IS NOT OLD.invalidation_code)
BEGIN
    SELECT RAISE(ABORT, 'candidate operation invalidation is permanent');
END;
