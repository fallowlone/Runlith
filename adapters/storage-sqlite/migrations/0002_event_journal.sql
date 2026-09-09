-- 0002_event_journal: append-only Event Journal for committed Task facts.
--
-- One row per committed Task lifecycle mutation, written in the same SQLite
-- transaction as the current-state update (events.md section 6). The journal
-- is history/evidence only: current state stays authoritative in `tasks`
-- and is never rebuilt by replaying this table.
--
-- Ordering: `seq` (AUTOINCREMENT) is the journal's deterministic retrieval
-- order. It is storage-local sequencing mechanics, not a domain contract:
-- no domain code may treat row order as lifecycle semantics.
--
-- Envelope fields mirror contracts/json-schema/event.schema.json
-- (event_type, schema_version/event_version, occurred_at, correlation_id,
-- causation_id) plus the references events.md requires in practice
-- (identity, task/project refs, from/to states, bounded reason metadata,
-- provenance). No prompt/secret/transcript columns exist and none may be
-- added without an explicit model/migration change.

CREATE TABLE event_journal (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type     TEXT NOT NULL CHECK (event_type IN (
        'task.marked_ready', 'task.claimed', 'task.started',
        'task.blocked', 'task.submitted', 'task.routed_to_review',
        'task.verification_began', 'task.verification_succeeded',
        'task.verification_failed'
    )),
    event_version  INTEGER NOT NULL CHECK (event_version = 1),
    task_id        TEXT NOT NULL REFERENCES tasks (id),
    project_id     TEXT NOT NULL REFERENCES projects (id),
    from_state     TEXT NOT NULL,
    to_state       TEXT NOT NULL,
    reason         TEXT NULL,
    correlation_id TEXT NOT NULL CHECK (length(correlation_id) > 0),
    causation_id   TEXT NOT NULL CHECK (length(causation_id) > 0),
    occurred_at    TEXT NOT NULL,
    provenance     TEXT NOT NULL
);

CREATE INDEX event_journal_task_seq ON event_journal (task_id, seq);
CREATE INDEX event_journal_project_seq ON event_journal (project_id, seq);
