-- TASK-0066: append-oriented canonical Security Audit, separate from Event Journal.
-- No secret payload, provider text, transcript, environment dump, or generic JSON column exists.

CREATE TABLE security_audit (
    seq INTEGER PRIMARY KEY,
    record_version INTEGER NOT NULL CHECK (record_version = 1),
    event_type TEXT NOT NULL CHECK (event_type IN (
        'policy.requested', 'policy.decided', 'authority.effective',
        'approval.requested', 'approval.decided', 'secret.access',
        'privilege.elevation', 'operation.denied', 'isolation.enforced',
        'isolation.violation', 'provider.invoked', 'tool.invoked'
    )),
    author TEXT NOT NULL CHECK (author IN (
        'policy-engine', 'approval-service', 'secrets-broker', 'runner',
        'provider-environment', 'runtime', 'tool-service'
    )),
    outcome TEXT NULL CHECK (outcome IS NULL OR outcome IN (
        'requested', 'allowed', 'denied', 'approval_required', 'approved',
        'rejected', 'consumed', 'succeeded', 'failed', 'enforced', 'violated'
    )),
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    correlation_id TEXT NOT NULL CHECK (length(trim(correlation_id)) > 0),
    causation_id TEXT NULL CHECK (causation_id IS NULL OR length(trim(causation_id)) > 0),
    operation_id TEXT NULL CHECK (operation_id IS NULL OR length(trim(operation_id)) > 0),
    task_id TEXT NULL CHECK (task_id IS NULL OR length(trim(task_id)) > 0),
    execution_id TEXT NULL CHECK (execution_id IS NULL OR length(trim(execution_id)) > 0),
    attempt_id TEXT NULL CHECK (attempt_id IS NULL OR length(trim(attempt_id)) > 0),
    subject_kind TEXT NULL CHECK (subject_kind IS NULL OR subject_kind IN (
        'policy_rule', 'approval', 'secret_ref', 'runner_backend', 'provider', 'tool', 'operation'
    )),
    subject_id TEXT NULL CHECK (subject_id IS NULL OR length(trim(subject_id)) > 0),
    policy_fingerprint TEXT NULL CHECK (policy_fingerprint IS NULL OR length(trim(policy_fingerprint)) > 0),
    rule_id TEXT NULL CHECK (rule_id IS NULL OR length(trim(rule_id)) > 0),
    reason_code TEXT NULL CHECK (reason_code IS NULL OR length(trim(reason_code)) > 0),
    approval_request_id TEXT NULL CHECK (approval_request_id IS NULL OR length(trim(approval_request_id)) > 0),
    previous_hash TEXT NOT NULL CHECK (length(previous_hash) = 71),
    record_hash TEXT NOT NULL UNIQUE CHECK (length(record_hash) = 71),
    CHECK ((subject_kind IS NULL) = (subject_id IS NULL))
);

CREATE TABLE security_audit_capabilities (
    audit_seq INTEGER NOT NULL REFERENCES security_audit(seq),
    position INTEGER NOT NULL CHECK (position >= 0),
    capability_kind TEXT NOT NULL CHECK (capability_kind IN (
        'filesystem.read', 'filesystem.write', 'process.execute', 'network.access',
        'git.mutate', 'secret.access', 'tool.invoke', 'external.action'
    )),
    workspace TEXT NULL CHECK (workspace IS NULL OR length(trim(workspace)) > 0),
    scope TEXT NOT NULL CHECK (length(trim(scope)) > 0),
    PRIMARY KEY (audit_seq, position)
);

CREATE TABLE security_audit_head (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    seq INTEGER NOT NULL CHECK (seq >= 0),
    record_hash TEXT NOT NULL CHECK (length(record_hash) = 71)
);

INSERT INTO security_audit_head (singleton, seq, record_hash)
VALUES (
    1,
    0,
    'sha256:0000000000000000000000000000000000000000000000000000000000000000'
);

CREATE INDEX idx_security_audit_correlation_seq
    ON security_audit(correlation_id, seq);

CREATE TRIGGER security_audit_no_update
BEFORE UPDATE ON security_audit
BEGIN
    SELECT RAISE(ABORT, 'security audit is append-only');
END;

CREATE TRIGGER security_audit_no_delete
BEFORE DELETE ON security_audit
BEGIN
    SELECT RAISE(ABORT, 'security audit is append-only');
END;

CREATE TRIGGER security_audit_capabilities_no_update
BEFORE UPDATE ON security_audit_capabilities
BEGIN
    SELECT RAISE(ABORT, 'security audit capabilities are append-only');
END;

CREATE TRIGGER security_audit_capabilities_no_delete
BEFORE DELETE ON security_audit_capabilities
BEGIN
    SELECT RAISE(ABORT, 'security audit capabilities are append-only');
END;
