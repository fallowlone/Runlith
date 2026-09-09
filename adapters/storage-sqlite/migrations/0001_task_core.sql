-- 0001_task_core: authoritative persisted current state for G2.
--
-- Tables: projects, tasks, task_dependencies, task_strings (list-valued
-- Task metadata), nothing else. No event_journal, no execution/attempt/
-- session/provider tables — those belong to later tasks.
--
-- Lifecycle state is stored as the explicit canonical name (DRAFT, READY,
-- ...), never as an enum ordinal. The CHECK below mirrors the accepted
-- 12-state census; readers still map unknown strings to a corruption
-- error rather than trusting the constraint (defense in depth, and the
-- constraint can be bypassed in corruption fixtures via
-- PRAGMA ignore_check_constraints).

CREATE TABLE projects (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE tasks (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects (id),
    objective   TEXT NOT NULL,
    parent_id   TEXT NULL REFERENCES tasks (id),
    state       TEXT NOT NULL CHECK (state IN (
        'DRAFT', 'READY', 'CLAIMED', 'RUNNING', 'BLOCKED', 'SUBMITTED',
        'AWAITING_REVIEW', 'VERIFYING', 'RETRY_READY', 'COMPLETED',
        'FAILED', 'CANCELLED'
    )),
    kind        TEXT NULL,
    risk        TEXT NULL
);

CREATE TABLE task_dependencies (
    task_id       TEXT NOT NULL REFERENCES tasks (id),
    depends_on_id TEXT NOT NULL REFERENCES tasks (id),
    PRIMARY KEY (task_id, depends_on_id)
);

CREATE TABLE task_strings (
    task_id TEXT NOT NULL REFERENCES tasks (id),
    slot    TEXT NOT NULL CHECK (slot IN (
        'acceptance', 'allowed_path', 'forbidden_path',
        'verification', 'required_review', 'artifact'
    )),
    position INTEGER NOT NULL CHECK (position >= 0),
    value   TEXT NOT NULL,
    PRIMARY KEY (task_id, slot, position)
);
