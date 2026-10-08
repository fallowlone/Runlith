-- 0030: TAKEOVER continuation (TASK-0172, TASK-0169 amendment).
--
-- Widens the HumanDecisionRequest continuation vocabularies with
-- 'TAKEOVER' by rebuilding the request parent and the options child
-- together (the 0016 technique: build the replacement pair, copy both,
-- drop the old child before the old parent, rename; SQLite retargets
-- the replacement child's FK when the new parent takes the canonical
-- name). Existing rows carry no TAKEOVER value, so the copy is
-- CHECK-clean. Scope-root and evidence children dangle by name while
-- the parent is dropped and resolve again after the rename; no DML
-- touches them in between.
--
-- Plus the TakeoverAdmission evidence record with its per-execution
-- workspace-anchor snapshots. The record status (ACTIVE/COMPLETED/
-- EXPIRED) is the durable marker distinguishing takeover-Blocked from
-- other Blocked causes — never journal text.

CREATE TABLE human_decision_requests_v30 (
    seq                         INTEGER PRIMARY KEY AUTOINCREMENT,
    request_id                  TEXT NOT NULL UNIQUE CHECK (length(trim(request_id)) > 0),
    request_version             INTEGER NOT NULL CHECK (request_version = 1),
    task_id                     TEXT NOT NULL REFERENCES tasks(id),
    plan_branch_id              TEXT NOT NULL CHECK (length(trim(plan_branch_id)) > 0),
    reason                      TEXT NOT NULL CHECK (reason IN (
        'RISK_ESCALATION', 'NO_PROGRESS', 'PLANNER_BUDGET', 'PLANNER_BLOCKED'
    )),
    risk_class                  TEXT NOT NULL CHECK (risk_class IN ('low', 'medium', 'high', 'critical')),
    recommendation              TEXT NULL CHECK (recommendation IS NULL OR recommendation IN ('RESUME', 'REPLAN', 'ABORT', 'TAKEOVER')),
    proposal_hash               TEXT NULL CHECK (
        proposal_hash IS NULL OR (
            length(proposal_hash) = 71 AND substr(proposal_hash, 1, 7) = 'sha256:'
        )
    ),
    status                      TEXT NOT NULL CHECK (status IN ('PENDING', 'RESOLVED', 'CONSUMED', 'SUPERSEDED')),
    selected_continuation       TEXT NULL CHECK (selected_continuation IS NULL OR selected_continuation IN ('RESUME', 'REPLAN', 'ABORT', 'TAKEOVER')),
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
    approved_changeset          BLOB NULL,
    approved_receipt_ref        TEXT NULL,
    CHECK (
        (status = 'PENDING' AND selected_continuation IS NULL AND decided_by IS NULL AND decided_at_ms IS NULL AND consumed_at_ms IS NULL)
        OR (status = 'RESOLVED' AND selected_continuation IS NOT NULL AND decided_by IS NOT NULL AND decided_at_ms IS NOT NULL AND consumed_at_ms IS NULL)
        OR (status = 'CONSUMED' AND selected_continuation IS NOT NULL AND decided_by IS NOT NULL AND decided_at_ms IS NOT NULL AND consumed_at_ms IS NOT NULL)
        OR (status = 'SUPERSEDED' AND consumed_at_ms IS NULL)
    )
);

CREATE TABLE human_decision_options_v30 (
    request_id   TEXT NOT NULL REFERENCES human_decision_requests_v30(request_id),
    position     INTEGER NOT NULL CHECK (position >= 0),
    continuation TEXT NOT NULL CHECK (continuation IN ('RESUME', 'REPLAN', 'ABORT', 'TAKEOVER')),
    PRIMARY KEY (request_id, position),
    UNIQUE (request_id, continuation)
);

INSERT INTO human_decision_requests_v30 (
    seq, request_id, request_version, task_id, plan_branch_id, reason,
    risk_class, recommendation, proposal_hash, status,
    selected_continuation, decided_by, created_at_ms, expires_at_ms,
    decided_at_ms, consumed_at_ms, task_state, task_event_seq, plan_head,
    planning_input_snapshot_id, planning_input_fingerprint,
    policy_fingerprint, approved_changeset, approved_receipt_ref
)
SELECT
    seq, request_id, request_version, task_id, plan_branch_id, reason,
    risk_class, recommendation, proposal_hash, status,
    selected_continuation, decided_by, created_at_ms, expires_at_ms,
    decided_at_ms, consumed_at_ms, task_state, task_event_seq, plan_head,
    planning_input_snapshot_id, planning_input_fingerprint,
    policy_fingerprint, approved_changeset, approved_receipt_ref
FROM human_decision_requests
ORDER BY seq;

INSERT INTO human_decision_options_v30 (request_id, position, continuation)
SELECT request_id, position, continuation
FROM human_decision_options
ORDER BY request_id, position;

DROP TABLE human_decision_options;
DROP TABLE human_decision_requests;

ALTER TABLE human_decision_requests_v30 RENAME TO human_decision_requests;
ALTER TABLE human_decision_options_v30 RENAME TO human_decision_options;

CREATE INDEX human_decision_owner_state
    ON human_decision_requests(task_id, plan_branch_id, status, seq);

CREATE INDEX human_decision_pending
    ON human_decision_requests(status, seq);

-- Takeover admission evidence. One row per consumed TAKEOVER; authored
-- fields arrive through the append path as NULL-able column trios
-- (kind/body/sensitivity, all NULL or all set). resulting_revision is
-- an optional FK into the accepted plan revisions of the same branch.
CREATE TABLE takeover_admissions (
    seq                         INTEGER PRIMARY KEY AUTOINCREMENT,
    record_id                   TEXT NOT NULL UNIQUE CHECK (length(trim(record_id)) > 0),
    hdr_id                      TEXT NOT NULL REFERENCES human_decision_requests(request_id),
    task_id                     TEXT NOT NULL REFERENCES tasks(id),
    plan_branch_id              TEXT NOT NULL CHECK (length(trim(plan_branch_id)) > 0),
    basis_plan_revision         INTEGER NOT NULL CHECK (basis_plan_revision >= 0),
    expires_at_ms               INTEGER NOT NULL CHECK (expires_at_ms >= 0),
    status                      TEXT NOT NULL CHECK (status IN ('ACTIVE', 'COMPLETED', 'EXPIRED')),
    actions_body_kind           TEXT NULL CHECK (actions_body_kind IS NULL OR actions_body_kind IN ('INLINE', 'REF')),
    actions_body                TEXT NULL CHECK (actions_body IS NULL OR length(actions_body) <= 4096),
    actions_sensitivity         TEXT NULL CHECK (actions_sensitivity IS NULL OR actions_sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')),
    uncertainty_body_kind       TEXT NULL CHECK (uncertainty_body_kind IS NULL OR uncertainty_body_kind IN ('INLINE', 'REF')),
    uncertainty_body            TEXT NULL CHECK (uncertainty_body IS NULL OR length(uncertainty_body) <= 4096),
    uncertainty_sensitivity     TEXT NULL CHECK (uncertainty_sensitivity IS NULL OR uncertainty_sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')),
    question_body_kind          TEXT NULL CHECK (question_body_kind IS NULL OR question_body_kind IN ('INLINE', 'REF')),
    question_body               TEXT NULL CHECK (question_body IS NULL OR length(question_body) <= 4096),
    question_sensitivity        TEXT NULL CHECK (question_sensitivity IS NULL OR question_sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')),
    resulting_revision          INTEGER NULL CHECK (resulting_revision IS NULL OR resulting_revision >= 1),
    created_at_ms               INTEGER NOT NULL CHECK (created_at_ms >= 0),
    resolved_at_ms              INTEGER NULL CHECK (resolved_at_ms IS NULL OR resolved_at_ms >= 0),
    CHECK ((actions_body_kind IS NULL) = (actions_body IS NULL)),
    CHECK ((actions_body IS NULL) = (actions_sensitivity IS NULL)),
    CHECK ((uncertainty_body_kind IS NULL) = (uncertainty_body IS NULL)),
    CHECK ((uncertainty_body IS NULL) = (uncertainty_sensitivity IS NULL)),
    CHECK ((question_body_kind IS NULL) = (question_body IS NULL)),
    CHECK ((question_body IS NULL) = (question_sensitivity IS NULL)),
    CHECK ((status = 'ACTIVE') = (resolved_at_ms IS NULL)),
    FOREIGN KEY (plan_branch_id, resulting_revision)
        REFERENCES plan_revisions(plan_branch_id, revision_seq)
) STRICT;

CREATE INDEX takeover_active_scope
    ON takeover_admissions(task_id, plan_branch_id, status, seq);

-- Workspace-anchor snapshots captured at admission: one row per
-- execution of the task that holds a lease (leaseless executions
-- leave no row; derivation is symmetric at return). Return refuses
-- with GitBackendFailure when the live bindings differ.
CREATE TABLE takeover_admission_anchors (
    record_id     TEXT NOT NULL REFERENCES takeover_admissions(record_id),
    execution_id  TEXT NOT NULL CHECK (length(trim(execution_id)) > 0),
    lease_id      TEXT NOT NULL CHECK (length(trim(lease_id)) > 0),
    base_revision TEXT NOT NULL CHECK (length(trim(base_revision)) > 0),
    worktree_ref  TEXT NOT NULL CHECK (length(trim(worktree_ref)) > 0),
    lifecycle     TEXT NOT NULL CHECK (length(trim(lifecycle)) > 0),
    anchor        TEXT NULL CHECK (anchor IS NULL OR length(trim(anchor)) > 0),
    PRIMARY KEY (record_id, execution_id)
) STRICT;

CREATE INDEX takeover_anchors_record
    ON takeover_admission_anchors(record_id);
