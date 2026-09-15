-- 0012_plan_persistence: durable G7 Plan revisions and plan-scoped journal facts.
--
-- Current Plan state is selected by plan_branches.head_revision. Each
-- committed revision owns one immutable normalized graph snapshot; there is
-- no second mutable "current graph" copy and the Event Journal never stores
-- node blobs. Historical revisions therefore remain addressable/diffable
-- while normal current-state reads do not require journal replay.

CREATE TABLE plan_branches (
    plan_branch_id TEXT PRIMARY KEY CHECK (length(plan_branch_id) > 0),
    head_revision  INTEGER NOT NULL DEFAULT 0 CHECK (head_revision >= 0)
);

CREATE TABLE plan_revisions (
    plan_branch_id             TEXT NOT NULL REFERENCES plan_branches (plan_branch_id),
    revision_seq               INTEGER NOT NULL CHECK (revision_seq >= 1),
    base_revision              INTEGER NOT NULL CHECK (base_revision >= 0),
    changeset_hash             TEXT NOT NULL CHECK (length(changeset_hash) > 0),
    reason                     TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    planner_adapter_id         TEXT NOT NULL CHECK (length(planner_adapter_id) > 0),
    planner_model_ref          TEXT NOT NULL CHECK (length(planner_model_ref) > 0),
    planning_cost_receipt_ref  TEXT NOT NULL CHECK (length(planning_cost_receipt_ref) > 0),
    validated_service_id       TEXT NOT NULL CHECK (length(validated_service_id) > 0),
    policy_fingerprint         TEXT NOT NULL CHECK (length(policy_fingerprint) > 0),
    created_at_ms              INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (plan_branch_id, revision_seq),
    UNIQUE (plan_branch_id, changeset_hash),
    CHECK (revision_seq = base_revision + 1)
);

CREATE TABLE plan_revision_causes (
    plan_branch_id TEXT NOT NULL,
    revision_seq   INTEGER NOT NULL,
    position       INTEGER NOT NULL CHECK (position >= 0),
    evidence_ref   TEXT NOT NULL CHECK (length(evidence_ref) > 0),
    PRIMARY KEY (plan_branch_id, revision_seq, position),
    FOREIGN KEY (plan_branch_id, revision_seq)
        REFERENCES plan_revisions (plan_branch_id, revision_seq)
);

CREATE TABLE plan_nodes (
    plan_branch_id TEXT NOT NULL,
    revision_seq   INTEGER NOT NULL,
    position       INTEGER NOT NULL CHECK (position >= 0),
    node_id        TEXT NOT NULL CHECK (length(node_id) > 0),
    risk_class     TEXT NOT NULL CHECK (risk_class IN ('low', 'medium', 'high', 'critical')),
    required_gate  TEXT NULL,
    PRIMARY KEY (plan_branch_id, revision_seq, node_id),
    UNIQUE (plan_branch_id, revision_seq, position),
    FOREIGN KEY (plan_branch_id, revision_seq)
        REFERENCES plan_revisions (plan_branch_id, revision_seq)
);

CREATE TABLE plan_node_scopes (
    plan_branch_id TEXT NOT NULL,
    revision_seq   INTEGER NOT NULL,
    node_id        TEXT NOT NULL,
    position       INTEGER NOT NULL CHECK (position >= 0),
    scope_root     TEXT NOT NULL CHECK (length(scope_root) > 0),
    PRIMARY KEY (plan_branch_id, revision_seq, node_id, position),
    FOREIGN KEY (plan_branch_id, revision_seq, node_id)
        REFERENCES plan_nodes (plan_branch_id, revision_seq, node_id)
);

CREATE TABLE plan_node_criteria (
    plan_branch_id      TEXT NOT NULL,
    revision_seq        INTEGER NOT NULL,
    node_id             TEXT NOT NULL,
    position            INTEGER NOT NULL CHECK (position >= 0),
    criterion_id        TEXT NOT NULL CHECK (length(criterion_id) > 0),
    verification_method TEXT NOT NULL CHECK (verification_method IN (
        'automated-test', 'independent-review', 'security-check', 'human-approval'
    )),
    PRIMARY KEY (plan_branch_id, revision_seq, node_id, position),
    UNIQUE (plan_branch_id, revision_seq, node_id, criterion_id),
    FOREIGN KEY (plan_branch_id, revision_seq, node_id)
        REFERENCES plan_nodes (plan_branch_id, revision_seq, node_id)
);

CREATE TABLE plan_edges (
    plan_branch_id TEXT NOT NULL,
    revision_seq   INTEGER NOT NULL,
    position       INTEGER NOT NULL CHECK (position >= 0),
    from_node_id   TEXT NOT NULL,
    to_node_id     TEXT NOT NULL,
    PRIMARY KEY (plan_branch_id, revision_seq, position),
    UNIQUE (plan_branch_id, revision_seq, from_node_id, to_node_id),
    CHECK (from_node_id != to_node_id),
    FOREIGN KEY (plan_branch_id, revision_seq, from_node_id)
        REFERENCES plan_nodes (plan_branch_id, revision_seq, node_id),
    FOREIGN KEY (plan_branch_id, revision_seq, to_node_id)
        REFERENCES plan_nodes (plan_branch_id, revision_seq, node_id)
);

CREATE INDEX plan_revisions_branch_seq
    ON plan_revisions (plan_branch_id, revision_seq);

-- The pre-G7 journal envelope required a project for every row. Plan branch
-- identity is independent of Task/Project identity, so a plan fact gets its
-- own typed scope instead of a fake project/task reference. Existing rows
-- retain their exact values and all historical type/scope rules remain.
CREATE TABLE event_journal_v12 (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type     TEXT NOT NULL CHECK (event_type IN (
        'task.marked_ready', 'task.claimed', 'task.started',
        'task.blocked', 'task.submitted', 'task.routed_to_review',
        'task.verification_began', 'task.verification_succeeded',
        'task.verification_failed',
        'project.created', 'task.created', 'task.dependency_added',
        'execution.created', 'attempt.created', 'attempt.concluded',
        'workspace.created', 'workspace.removed',
        'plan.revision_committed'
    )),
    event_version  INTEGER NOT NULL CHECK (event_version = 1),
    task_id        TEXT NULL REFERENCES tasks (id),
    project_id     TEXT NULL REFERENCES projects (id),
    from_state     TEXT NULL,
    to_state       TEXT NULL,
    reason         TEXT NULL,
    correlation_id TEXT NOT NULL CHECK (length(correlation_id) > 0),
    causation_id   TEXT NOT NULL CHECK (length(causation_id) > 0),
    occurred_at    TEXT NOT NULL,
    provenance     TEXT NOT NULL,
    execution_id   TEXT NULL REFERENCES executions (id),
    attempt_id     TEXT NULL REFERENCES execution_attempts (attempt_id),
    attempt_outcome TEXT NULL CHECK (attempt_outcome IN (
        'succeeded', 'failed', 'timed_out', 'cancelled'
    )),
    workspace_id   TEXT NULL REFERENCES workspace_leases (workspace_id),
    plan_branch_id TEXT NULL,
    plan_revision_seq INTEGER NULL,
    plan_changeset_hash TEXT NULL,
    plan_planner_adapter_id TEXT NULL,
    plan_model_ref TEXT NULL,
    plan_planning_cost_receipt_ref TEXT NULL,
    plan_validated_service_id TEXT NULL,
    plan_policy_fingerprint TEXT NULL,
    FOREIGN KEY (plan_branch_id, plan_revision_seq)
        REFERENCES plan_revisions (plan_branch_id, revision_seq),
    CHECK (
        (event_type = 'project.created'
            AND project_id IS NOT NULL
            AND task_id IS NULL
            AND from_state IS NULL AND to_state IS NULL
            AND execution_id IS NULL AND attempt_id IS NULL
            AND attempt_outcome IS NULL AND workspace_id IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type IN (
                'task.marked_ready', 'task.claimed', 'task.started',
                'task.blocked', 'task.submitted', 'task.routed_to_review',
                'task.verification_began', 'task.verification_succeeded',
                'task.verification_failed', 'task.created', 'task.dependency_added'
            )
            AND project_id IS NOT NULL AND task_id IS NOT NULL
            AND from_state IS NOT NULL AND to_state IS NOT NULL
            AND execution_id IS NULL AND attempt_id IS NULL
            AND attempt_outcome IS NULL AND workspace_id IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type = 'execution.created'
            AND project_id IS NOT NULL AND task_id IS NOT NULL
            AND from_state IS NULL AND to_state IS NULL
            AND execution_id IS NOT NULL AND attempt_id IS NULL
            AND attempt_outcome IS NULL AND workspace_id IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type = 'attempt.created'
            AND project_id IS NOT NULL AND task_id IS NOT NULL
            AND from_state IS NULL AND to_state IS NULL
            AND execution_id IS NOT NULL AND attempt_id IS NOT NULL
            AND attempt_outcome IS NULL AND workspace_id IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type = 'attempt.concluded'
            AND project_id IS NOT NULL AND task_id IS NOT NULL
            AND from_state IS NULL AND to_state IS NULL
            AND execution_id IS NOT NULL AND attempt_id IS NOT NULL
            AND attempt_outcome IS NOT NULL AND workspace_id IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type = 'workspace.created'
            AND project_id IS NOT NULL AND task_id IS NOT NULL
            AND from_state = 'ALLOCATED' AND to_state = 'READY'
            AND execution_id IS NOT NULL AND workspace_id IS NOT NULL
            AND attempt_id IS NULL AND attempt_outcome IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type = 'workspace.removed'
            AND project_id IS NOT NULL AND task_id IS NOT NULL
            AND from_state = 'CLEANUP_PENDING' AND to_state = 'REMOVED'
            AND execution_id IS NOT NULL AND workspace_id IS NOT NULL
            AND attempt_id IS NULL AND attempt_outcome IS NULL
            AND plan_branch_id IS NULL AND plan_revision_seq IS NULL
            AND plan_changeset_hash IS NULL AND plan_planner_adapter_id IS NULL
            AND plan_model_ref IS NULL AND plan_planning_cost_receipt_ref IS NULL
            AND plan_validated_service_id IS NULL AND plan_policy_fingerprint IS NULL)
        OR (event_type = 'plan.revision_committed'
            AND project_id IS NULL AND task_id IS NULL
            AND from_state IS NULL AND to_state IS NULL
            AND execution_id IS NULL AND attempt_id IS NULL
            AND attempt_outcome IS NULL AND workspace_id IS NULL
            AND reason IS NOT NULL AND length(trim(reason)) > 0
            AND plan_branch_id IS NOT NULL AND plan_revision_seq IS NOT NULL
            AND plan_changeset_hash IS NOT NULL
            AND plan_planner_adapter_id IS NOT NULL AND plan_model_ref IS NOT NULL
            AND plan_planning_cost_receipt_ref IS NOT NULL
            AND plan_validated_service_id IS NOT NULL
            AND plan_policy_fingerprint IS NOT NULL)
    )
);

INSERT INTO event_journal_v12
    (seq, event_type, event_version, task_id, project_id, from_state,
     to_state, reason, correlation_id, causation_id, occurred_at,
     provenance, execution_id, attempt_id, attempt_outcome, workspace_id)
SELECT seq, event_type, event_version, task_id, project_id, from_state,
       to_state, reason, correlation_id, causation_id, occurred_at,
       provenance, execution_id, attempt_id, attempt_outcome, workspace_id
FROM event_journal;

DROP TABLE event_journal;
ALTER TABLE event_journal_v12 RENAME TO event_journal;

CREATE INDEX event_journal_task_seq ON event_journal (task_id, seq);
CREATE INDEX event_journal_project_seq ON event_journal (project_id, seq);
CREATE INDEX event_journal_execution_seq ON event_journal (execution_id, seq);
CREATE INDEX event_journal_workspace_seq ON event_journal (workspace_id, seq);
CREATE INDEX event_journal_plan_branch_seq ON event_journal (plan_branch_id, seq);

UPDATE sqlite_sequence SET seq = (SELECT COALESCE(MAX(seq), 0) FROM event_journal)
WHERE name = 'event_journal';

CREATE TABLE event_plan_causes (
    event_seq    INTEGER NOT NULL REFERENCES event_journal (seq),
    position     INTEGER NOT NULL CHECK (position >= 0),
    evidence_ref TEXT NOT NULL CHECK (length(evidence_ref) > 0),
    PRIMARY KEY (event_seq, position)
);
