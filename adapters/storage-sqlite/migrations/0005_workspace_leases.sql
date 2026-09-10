-- 0005_workspace_leases: durable WorkspaceLease ownership plus the
-- frozen workspace journal scope (ADR-0023 section 9).
--
-- New relational state (current-state authority for workspace reads; the
-- journal is never replayed for normal reads):
--
--   workspace_leases — one row per durable lease: its identity, the single
--                       owning Execution (UNIQUE: one Execution owns exactly
--                       one lease), the provider-neutral repository ref, the
--                       immutable base commit OID, the managed branch ref,
--                       the opaque worktree reference, and the lifecycle
--                       marker. No dirtiness columns exist and none may be
--                       added: Clean/DirtyTracked/DirtyUntracked stay live
--                       Git observations, never persisted authority.
--                       Lifecycle is stored as the canonical wire name
--                       (ALLOCATED, READY, CLEANUP_PENDING, REMOVED), never
--                       as an enum ordinal. Foreign keys are restrictive
--                       (default NO ACTION): nothing here designs Execution
--                       deletion, so no cascade semantics are invented.
--
-- The journal table is rebuilt (CHECK constraints cannot be altered in
-- place) with one nullable first-class scope column and the two frozen
-- canonical types. Historical rows are copied verbatim (seq included, new
-- column NULL) and the AUTOINCREMENT sequence is preserved: no
-- re-numbering, no recreated Event IDs, no G2/G3 weakening. Every
-- pre-existing family additionally requires workspace_id IS NULL, so old
-- and new scopes can never mix.
--
-- Workspace scope contract (ADR-0023: from_state/to_state carry the small
-- lease lifecycle as task-state-style semantics owned by the Workspace
-- context; no new Task lifecycle states):
--
--   workspace.created → project + task + execution + workspace required,
--                        from_state = 'ALLOCATED', to_state = 'READY'
--                        (lease allocated + verified consistent),
--                        attempt columns NULL
--   workspace.removed → project + task + execution + workspace required,
--                        from_state = 'CLEANUP_PENDING', to_state = 'REMOVED'
--                        (owned worktree + branch torn down after safety
--                        checks), attempt columns NULL

CREATE TABLE workspace_leases (
    workspace_id   TEXT PRIMARY KEY,
    execution_id   TEXT NOT NULL UNIQUE REFERENCES executions (id),
    repository_ref TEXT NOT NULL,
    base_revision  TEXT NOT NULL,
    branch_ref     TEXT NOT NULL,
    worktree_ref   TEXT NOT NULL,
    lifecycle      TEXT NOT NULL CHECK (lifecycle IN (
        'ALLOCATED', 'READY', 'CLEANUP_PENDING', 'REMOVED'
    ))
);

CREATE TABLE event_journal_v5 (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type     TEXT NOT NULL CHECK (event_type IN (
        'task.marked_ready', 'task.claimed', 'task.started',
        'task.blocked', 'task.submitted', 'task.routed_to_review',
        'task.verification_began', 'task.verification_succeeded',
        'task.verification_failed',
        'project.created', 'task.created', 'task.dependency_added',
        'execution.created', 'attempt.created', 'attempt.concluded',
        'workspace.created', 'workspace.removed'
    )),
    event_version  INTEGER NOT NULL CHECK (event_version = 1),
    task_id        TEXT NULL REFERENCES tasks (id),
    project_id     TEXT NOT NULL REFERENCES projects (id),
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
    CHECK (
        (event_type = 'project.created'
            AND task_id IS NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL
            AND workspace_id IS NULL)
        OR (event_type IN (
                'task.marked_ready', 'task.claimed', 'task.started',
                'task.blocked', 'task.submitted', 'task.routed_to_review',
                'task.verification_began', 'task.verification_succeeded',
                'task.verification_failed',
                'task.created', 'task.dependency_added'
            )
            AND task_id IS NOT NULL
            AND from_state IS NOT NULL
            AND to_state IS NOT NULL
            AND execution_id IS NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL
            AND workspace_id IS NULL)
        OR (event_type = 'execution.created'
            AND task_id IS NOT NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NOT NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL
            AND workspace_id IS NULL)
        OR (event_type = 'attempt.created'
            AND task_id IS NOT NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NOT NULL
            AND attempt_id IS NOT NULL
            AND attempt_outcome IS NULL
            AND workspace_id IS NULL)
        OR (event_type = 'attempt.concluded'
            AND task_id IS NOT NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NOT NULL
            AND attempt_id IS NOT NULL
            AND attempt_outcome IS NOT NULL
            AND workspace_id IS NULL)
        OR (event_type = 'workspace.created'
            AND task_id IS NOT NULL
            AND from_state = 'ALLOCATED'
            AND to_state = 'READY'
            AND execution_id IS NOT NULL
            AND workspace_id IS NOT NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL)
        OR (event_type = 'workspace.removed'
            AND task_id IS NOT NULL
            AND from_state = 'CLEANUP_PENDING'
            AND to_state = 'REMOVED'
            AND execution_id IS NOT NULL
            AND workspace_id IS NOT NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL)
    )
);

INSERT INTO event_journal_v5
    (seq, event_type, event_version, task_id, project_id, from_state,
     to_state, reason, correlation_id, causation_id, occurred_at,
     provenance, execution_id, attempt_id, attempt_outcome, workspace_id)
SELECT seq, event_type, event_version, task_id, project_id, from_state,
       to_state, reason, correlation_id, causation_id, occurred_at,
       provenance, execution_id, attempt_id, attempt_outcome, NULL
FROM event_journal;

DROP TABLE event_journal;

ALTER TABLE event_journal_v5 RENAME TO event_journal;

CREATE INDEX event_journal_task_seq ON event_journal (task_id, seq);
CREATE INDEX event_journal_project_seq ON event_journal (project_id, seq);
CREATE INDEX event_journal_execution_seq ON event_journal (execution_id, seq);
CREATE INDEX event_journal_workspace_seq ON event_journal (workspace_id, seq);

-- The copy above inserts explicit seq values; pin the AUTOINCREMENT
-- sequence to the preserved maximum so future `evt-<seq>` identities
-- continue without reuse or gaps beyond the copied history.
UPDATE sqlite_sequence SET seq = (SELECT COALESCE(MAX(seq), 0) FROM event_journal)
WHERE name = 'event_journal';
