-- 0003_event_scope: project-scoped journal events plus the G2 creation /
-- dependency taxonomy.
--
-- TASK-0027 gap: `project.created` is project-scoped, but `event_journal`
-- required `task_id NOT NULL`. No fake/sentinel/empty task id is used to
-- satisfy the old column: the table is rebuilt with `task_id NULL` for
-- legitimate project-scoped events, guarded by a scope CHECK so the
-- scoping stays typed (never a generic unscoped event model):
--
--   project.created      → project_id required, task_id NULL,
--                           from_state/to_state NULL (no task subject)
--   task.created         → project_id + task_id required,
--                           from_state/to_state = initial task state
--   task.dependency_added → project_id + task_id required,
--                           from_state/to_state = current task state
--                           (dependency edges never mutate lifecycle state)
--   task lifecycle events → unchanged task-scoped shape
--
-- Historical migrations are never modified; existing rows are copied
-- verbatim (seq included) and the AUTOINCREMENT sequence is preserved.
-- Current state stays authoritative in `tasks`/`projects`: this migration
-- changes journal scoping only, never state reconstruction.

CREATE TABLE event_journal_v3 (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type     TEXT NOT NULL CHECK (event_type IN (
        'task.marked_ready', 'task.claimed', 'task.started',
        'task.blocked', 'task.submitted', 'task.routed_to_review',
        'task.verification_began', 'task.verification_succeeded',
        'task.verification_failed',
        'project.created', 'task.created', 'task.dependency_added'
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
    CHECK (
        (event_type = 'project.created'
            AND task_id IS NULL
            AND from_state IS NULL
            AND to_state IS NULL)
        OR (event_type != 'project.created'
            AND task_id IS NOT NULL
            AND from_state IS NOT NULL
            AND to_state IS NOT NULL)
    )
);

INSERT INTO event_journal_v3
    (seq, event_type, event_version, task_id, project_id, from_state,
     to_state, reason, correlation_id, causation_id, occurred_at,
     provenance)
SELECT seq, event_type, event_version, task_id, project_id, from_state,
       to_state, reason, correlation_id, causation_id, occurred_at,
       provenance
FROM event_journal;

DROP TABLE event_journal;

ALTER TABLE event_journal_v3 RENAME TO event_journal;

CREATE INDEX event_journal_task_seq ON event_journal (task_id, seq);
CREATE INDEX event_journal_project_seq ON event_journal (project_id, seq);

-- The copy above inserts explicit seq values; pin the AUTOINCREMENT
-- sequence to the preserved maximum so future `evt-<seq>` identities
-- continue without reuse or gaps beyond the copied history.
UPDATE sqlite_sequence SET seq = (SELECT COALESCE(MAX(seq), 0) FROM event_journal)
WHERE name = 'event_journal';
