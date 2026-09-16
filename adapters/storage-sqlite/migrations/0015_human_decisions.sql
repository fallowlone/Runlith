-- TASK-0079: durable bounded Human Decision Queue.
--
-- Human decisions are planning/control-plane state. They do not mutate Task
-- lifecycle, do not duplicate Security Approval authority, and do not grant
-- provider capabilities. Preconditions bind every request to the exact Task,
-- Plan head and PlanningInputSnapshot the human saw.

CREATE TABLE human_decision_requests (
    seq                         INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id                  TEXT NOT NULL UNIQUE CHECK (length(trim(request_id)) > 0),
    request_version             INTEGER NOT NULL CHECK (request_version = 1),
    task_id                     TEXT NOT NULL REFERENCES tasks(id),
    plan_branch_id              TEXT NOT NULL CHECK (length(trim(plan_branch_id)) > 0),
    reason                      TEXT NOT NULL CHECK (reason IN (
        'RISK_ESCALATION', 'NO_PROGRESS', 'PLANNER_BUDGET', 'PLANNER_BLOCKED'
    )),
    risk_class                  TEXT NOT NULL CHECK (risk_class IN ('low', 'medium', 'high', 'critical')),
    recommendation              TEXT NULL CHECK (recommendation IS NULL OR recommendation IN ('RESUME', 'REPLAN', 'ABORT')),
    proposal_hash               TEXT NULL CHECK (
        proposal_hash IS NULL OR (
            length(proposal_hash) = 71 AND substr(proposal_hash, 1, 7) = 'sha256:'
        )
    ),
    status                      TEXT NOT NULL CHECK (status IN ('PENDING', 'RESOLVED', 'CONSUMED', 'SUPERSEDED')),
    selected_continuation       TEXT NULL CHECK (selected_continuation IS NULL OR selected_continuation IN ('RESUME', 'REPLAN', 'ABORT')),
    decided_by                  TEXT NULL,
    created_at_ms               INTEGER NOT NULL CHECK (created_at_ms >= 0),
    expires_at_ms               INTEGER NULL CHECK (expires_at_ms IS NULL OR expires_at_ms >= 0),
    decided_at_ms               INTEGER NULL CHECK (decided_at_ms IS NULL OR decided_at_ms >= 0),
    consumed_at_ms              INTEGER NULL CHECK (consumed_at_ms IS NULL OR consumed_at_ms >= 0),
    task_state                  TEXT NOT NULL CHECK (task_state IN (
        'DRAFT', 'READY', 'CLAIMED', 'RUNNING', 'BLOCKED', 'SUBMITTED',
        'AWAITING_REVIEW', 'VERIFYING', 'RETRY_READY', 'COMPLETED', 'FAILED', 'CANCELLED'
    )),
    task_event_seq              INTEGER NOT NULL CHECK (task_event_seq >= 0),
    plan_head                   INTEGER NOT NULL CHECK (plan_head >= 0),
    planning_input_snapshot_id  TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    planning_input_fingerprint  TEXT NOT NULL CHECK (
        length(planning_input_fingerprint) = 71 AND substr(planning_input_fingerprint, 1, 7) = 'sha256:'
    ),
    policy_fingerprint          TEXT NOT NULL CHECK (length(trim(policy_fingerprint)) > 0),
    CHECK (
        (status = 'PENDING' AND selected_continuation IS NULL AND decided_by IS NULL AND decided_at_ms IS NULL AND consumed_at_ms IS NULL)
        OR (status = 'RESOLVED' AND selected_continuation IS NOT NULL AND decided_by IS NOT NULL AND decided_at_ms IS NOT NULL AND consumed_at_ms IS NULL)
        OR (status = 'CONSUMED' AND selected_continuation IS NOT NULL AND decided_by IS NOT NULL AND decided_at_ms IS NOT NULL AND consumed_at_ms IS NOT NULL)
        OR (status = 'SUPERSEDED' AND consumed_at_ms IS NULL)
    )
);

CREATE TABLE human_decision_scope_roots (
    request_id TEXT NOT NULL REFERENCES human_decision_requests(request_id),
    position   INTEGER NOT NULL CHECK (position >= 0),
    scope_root TEXT NOT NULL CHECK (length(trim(scope_root)) > 0),
    PRIMARY KEY (request_id, position),
    UNIQUE (request_id, scope_root)
);

CREATE TABLE human_decision_options (
    request_id   TEXT NOT NULL REFERENCES human_decision_requests(request_id),
    position     INTEGER NOT NULL CHECK (position >= 0),
    continuation TEXT NOT NULL CHECK (continuation IN ('RESUME', 'REPLAN', 'ABORT')),
    PRIMARY KEY (request_id, position),
    UNIQUE (request_id, continuation)
);

CREATE TABLE human_decision_evidence (
    request_id  TEXT NOT NULL REFERENCES human_decision_requests(request_id),
    position    INTEGER NOT NULL CHECK (position >= 0),
    evidence_ref TEXT NOT NULL CHECK (length(trim(evidence_ref)) > 0),
    PRIMARY KEY (request_id, position),
    UNIQUE (request_id, evidence_ref)
);

CREATE INDEX human_decision_owner_state
    ON human_decision_requests(task_id, plan_branch_id, status, seq);

CREATE INDEX human_decision_pending
    ON human_decision_requests(status, seq);
