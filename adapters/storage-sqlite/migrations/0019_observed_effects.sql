CREATE TABLE observed_effects (
    effect_id TEXT PRIMARY KEY CHECK (length(trim(effect_id)) > 0 AND length(effect_id) <= 1024),
    subject_ref TEXT NOT NULL CHECK (length(trim(subject_ref)) > 0 AND length(subject_ref) <= 1024),
    subject_fingerprint TEXT NOT NULL CHECK (subject_fingerprint GLOB 'sha256:*' AND length(subject_fingerprint) = 71),
    effect_kind TEXT NOT NULL CHECK (effect_kind IN ('PROCESS_EXIT','TEST_SUITE_RESULT','ARTIFACT_HASH','FS_STATE','GIT_STATE')),
    captured_by TEXT NOT NULL CHECK (captured_by = 'runlith.local-harness.v1'),
    captured_at TEXT NOT NULL CHECK (length(trim(captured_at)) > 0 AND length(captured_at) <= 1024),
    payload_ref TEXT NOT NULL CHECK (length(payload_ref) > 0 AND length(payload_ref) <= 4096),
    execution_id TEXT NOT NULL REFERENCES executions(id),
    attempt_id TEXT NOT NULL REFERENCES execution_attempts(attempt_id),
    runtime_ref TEXT NOT NULL CHECK (length(trim(runtime_ref)) > 0 AND length(runtime_ref) <= 1024),
    evaluator_ref TEXT CHECK (evaluator_ref IS NULL OR length(evaluator_ref) <= 1024),
    receipt_ref TEXT CHECK (receipt_ref IS NULL OR length(receipt_ref) <= 1024),
    CHECK (instr(payload_ref, char(10)) = 0 AND instr(payload_ref, char(13)) = 0)
);
CREATE INDEX observed_effects_execution_idx ON observed_effects(execution_id, attempt_id);
