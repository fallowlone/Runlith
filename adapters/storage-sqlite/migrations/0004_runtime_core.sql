-- 0004_runtime_core: durable provider-neutral Execution/Attempt state
-- plus ADR-0022 runtime scope in the single Event Journal.
--
-- New relational state (current-state authority for runtime reads; the
-- journal is never replayed for normal reads):
--
--   executions         — one row per durable Execution: its identity plus
--                         its single Task owner. No Task title/description/
--                         lifecycle copy, no provider columns (ProviderRef
--                         lives per Attempt in TASK-0033), no session
--                         columns, no scheduler columns.
--   execution_attempts — one row per Attempt: identity, owning Execution,
--                         explicit deterministic position (Vec order, NOT
--                         rowid), opaque provider reference, terminal
--                         outcome, opaque evidence note. Outcome uses the
--                         ADR-0022 canonical serialization, never an enum
--                         ordinal. Detail is diagnostic relational state,
--                         never journal taxonomy.
--
-- Foreign keys are restrictive (default NO ACTION): nothing here designs
-- Task deletion or runtime retention, so no cascade semantics are
-- invented. UNIQUE (execution_id, position) keeps the explicit order
-- fail-closed under interleaved writers.
--
-- The journal table is rebuilt (CHECK constraints cannot be altered in
-- place) with three nullable first-class scope columns and the extended
-- canonical type/scope contract. Historical rows are copied verbatim
-- (seq included, new columns NULL) and the AUTOINCREMENT sequence is
-- preserved: no re-numbering, no recreated Event IDs, no G2 weakening.

CREATE TABLE executions (
    id      TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks (id)
);

CREATE TABLE execution_attempts (
    attempt_id   TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL REFERENCES executions (id),
    position     INTEGER NOT NULL CHECK (position >= 0),
    provider_ref TEXT NOT NULL,
    outcome      TEXT NULL CHECK (outcome IN (
        'succeeded', 'failed', 'timed_out', 'cancelled'
    )),
    detail       TEXT NULL,
    UNIQUE (execution_id, position)
);

CREATE TABLE event_journal_v4 (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type     TEXT NOT NULL CHECK (event_type IN (
        'task.marked_ready', 'task.claimed', 'task.started',
        'task.blocked', 'task.submitted', 'task.routed_to_review',
        'task.verification_began', 'task.verification_succeeded',
        'task.verification_failed',
        'project.created', 'task.created', 'task.dependency_added',
        'execution.created', 'attempt.created', 'attempt.concluded'
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
    CHECK (
        (event_type = 'project.created'
            AND task_id IS NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL)
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
            AND attempt_outcome IS NULL)
        OR (event_type = 'execution.created'
            AND task_id IS NOT NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NOT NULL
            AND attempt_id IS NULL
            AND attempt_outcome IS NULL)
        OR (event_type = 'attempt.created'
            AND task_id IS NOT NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NOT NULL
            AND attempt_id IS NOT NULL
            AND attempt_outcome IS NULL)
        OR (event_type = 'attempt.concluded'
            AND task_id IS NOT NULL
            AND from_state IS NULL
            AND to_state IS NULL
            AND execution_id IS NOT NULL
            AND attempt_id IS NOT NULL
            AND attempt_outcome IS NOT NULL)
    )
);

INSERT INTO event_journal_v4
    (seq, event_type, event_version, task_id, project_id, from_state,
     to_state, reason, correlation_id, causation_id, occurred_at,
     provenance, execution_id, attempt_id, attempt_outcome)
SELECT seq, event_type, event_version, task_id, project_id, from_state,
       to_state, reason, correlation_id, causation_id, occurred_at,
       provenance, NULL, NULL, NULL
FROM event_journal;

DROP TABLE event_journal;

ALTER TABLE event_journal_v4 RENAME TO event_journal;

CREATE INDEX event_journal_task_seq ON event_journal (task_id, seq);
CREATE INDEX event_journal_project_seq ON event_journal (project_id, seq);
CREATE INDEX event_journal_execution_seq ON event_journal (execution_id, seq);

-- The copy above inserts explicit seq values; pin the AUTOINCREMENT
-- sequence to the preserved maximum so future `evt-<seq>` identities
-- continue without reuse or gaps beyond the copied history.
UPDATE sqlite_sequence SET seq = (SELECT COALESCE(MAX(seq), 0) FROM event_journal)
WHERE name = 'event_journal';
