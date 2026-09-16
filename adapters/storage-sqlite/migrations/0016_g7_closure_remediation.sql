-- TASK-0082: durable G7 closure remediation state.
--
-- The trusted candidate Task set is part of PlanningInputSnapshot identity and
-- must survive restart so V9 RESUME can prove that adding the exact human
-- grant did not widen Task authority.
CREATE TABLE planning_input_candidate_tasks (
    snapshot_id TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    position    INTEGER NOT NULL CHECK (position >= 0),
    task_ref    TEXT NOT NULL CHECK (length(trim(task_ref)) > 0),
    PRIMARY KEY (snapshot_id, position),
    UNIQUE (snapshot_id, task_ref)
);

-- Risk-escalation Human Decisions bind the exact immutable proposal that the
-- human reviewed. The BLOB is an adapter-owned deterministic codec, verified
-- against proposal_hash before use; the receipt ref resolves through the
-- existing plan_cost_receipts store. Both columns are nullable because other
-- Human Decision reasons do not approve a concrete proposal.
ALTER TABLE human_decision_requests ADD COLUMN approved_changeset BLOB NULL;
ALTER TABLE human_decision_requests ADD COLUMN approved_receipt_ref TEXT NULL;

-- TASK-0082 adds Human Decision lifecycle facts to the existing append-only
-- Security Audit. Migration 0010 intentionally used closed CHECK vocabularies,
-- so extend those vocabularies by rebuilding the audit parent and capability
-- child together. Existing rows, sequence numbers, hashes, head state and
-- capability positions are copied byte-for-byte; hashes are never recomputed.
--
-- Foreign-key enforcement is ON while migrations run. Build the replacement
-- parent/child pair first, copy both tables, then drop the old child before the
-- old parent. SQLite updates the replacement child's FK target when the new
-- parent is renamed to the canonical table name.
CREATE TABLE security_audit_v16 (
    seq INTEGER PRIMARY KEY,
    record_version INTEGER NOT NULL CHECK (record_version = 1),
    event_type TEXT NOT NULL CHECK (event_type IN (
        'policy.requested', 'policy.decided', 'authority.effective',
        'approval.requested', 'approval.decided',
        'human_decision.requested', 'human_decision.decided',
        'human_decision.applied', 'human_decision.superseded',
        'secret.access', 'privilege.elevation', 'operation.denied',
        'isolation.enforced', 'isolation.violation', 'provider.invoked',
        'tool.invoked'
    )),
    author TEXT NOT NULL CHECK (author IN (
        'policy-engine', 'approval-service', 'human-decision-service',
        'secrets-broker', 'runner', 'provider-environment', 'runtime',
        'tool-service'
    )),
    outcome TEXT NULL CHECK (outcome IS NULL OR outcome IN (
        'requested', 'allowed', 'denied', 'approval_required', 'approved',
        'rejected', 'resolved', 'superseded', 'consumed', 'succeeded',
        'failed', 'enforced', 'violated'
    )),
    occurred_at_ms INTEGER NOT NULL CHECK (occurred_at_ms >= 0),
    correlation_id TEXT NOT NULL CHECK (length(trim(correlation_id)) > 0),
    causation_id TEXT NULL CHECK (causation_id IS NULL OR length(trim(causation_id)) > 0),
    operation_id TEXT NULL CHECK (operation_id IS NULL OR length(trim(operation_id)) > 0),
    task_id TEXT NULL CHECK (task_id IS NULL OR length(trim(task_id)) > 0),
    execution_id TEXT NULL CHECK (execution_id IS NULL OR length(trim(execution_id)) > 0),
    attempt_id TEXT NULL CHECK (attempt_id IS NULL OR length(trim(attempt_id)) > 0),
    subject_kind TEXT NULL CHECK (subject_kind IS NULL OR subject_kind IN (
        'policy_rule', 'approval', 'human_decision', 'secret_ref',
        'runner_backend', 'provider', 'tool', 'operation'
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

CREATE TABLE security_audit_capabilities_v16 (
    audit_seq INTEGER NOT NULL REFERENCES security_audit_v16(seq),
    position INTEGER NOT NULL CHECK (position >= 0),
    capability_kind TEXT NOT NULL CHECK (capability_kind IN (
        'filesystem.read', 'filesystem.write', 'process.execute', 'network.access',
        'git.mutate', 'secret.access', 'tool.invoke', 'external.action'
    )),
    workspace TEXT NULL CHECK (workspace IS NULL OR length(trim(workspace)) > 0),
    scope TEXT NOT NULL CHECK (length(trim(scope)) > 0),
    PRIMARY KEY (audit_seq, position)
);

INSERT INTO security_audit_v16 (
    seq, record_version, event_type, author, outcome, occurred_at_ms,
    correlation_id, causation_id, operation_id, task_id, execution_id,
    attempt_id, subject_kind, subject_id, policy_fingerprint, rule_id,
    reason_code, approval_request_id, previous_hash, record_hash
)
SELECT
    seq, record_version, event_type, author, outcome, occurred_at_ms,
    correlation_id, causation_id, operation_id, task_id, execution_id,
    attempt_id, subject_kind, subject_id, policy_fingerprint, rule_id,
    reason_code, approval_request_id, previous_hash, record_hash
FROM security_audit
ORDER BY seq;

INSERT INTO security_audit_capabilities_v16 (
    audit_seq, position, capability_kind, workspace, scope
)
SELECT audit_seq, position, capability_kind, workspace, scope
FROM security_audit_capabilities
ORDER BY audit_seq, position;

DROP TRIGGER security_audit_capabilities_no_update;
DROP TRIGGER security_audit_capabilities_no_delete;
DROP TRIGGER security_audit_no_update;
DROP TRIGGER security_audit_no_delete;
DROP TABLE security_audit_capabilities;
DROP TABLE security_audit;

ALTER TABLE security_audit_v16 RENAME TO security_audit;
ALTER TABLE security_audit_capabilities_v16 RENAME TO security_audit_capabilities;

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
