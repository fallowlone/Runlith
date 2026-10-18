-- 0035: G11 Background daemon (TASK-0191, ADR-0045).
CREATE TABLE routines (
    routine_id TEXT PRIMARY KEY CHECK (routine_id GLOB 'rou_*' AND length(routine_id) = 30),
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 1024),
    objective TEXT NOT NULL CHECK (length(objective) BETWEEN 1 AND 16384),
    interval_seconds INTEGER NOT NULL CHECK (interval_seconds BETWEEN 60 AND 2592000),
    capability_scope TEXT NOT NULL CHECK (capability_scope IN ('READ_ONLY', 'WORKSPACE_WRITE', 'EXECUTION')),
    budget_max_runs INTEGER NOT NULL CHECK (budget_max_runs BETWEEN 1 AND 10000),
    author_ref TEXT NOT NULL CHECK (length(author_ref) BETWEEN 1 AND 1024),
    authorized_by TEXT CHECK (authorized_by IS NULL OR length(authorized_by) BETWEEN 1 AND 1024),
    state TEXT NOT NULL CHECK (state IN ('PROPOSED', 'AUTHORIZED', 'DISABLED', 'EXHAUSTED')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    next_run_at INTEGER NOT NULL CHECK (next_run_at >= created_at),
    runs_completed INTEGER NOT NULL CHECK (runs_completed BETWEEN 0 AND budget_max_runs)
) STRICT;

CREATE TABLE routine_runs (
    run_id TEXT PRIMARY KEY CHECK (run_id GLOB 'run_*' AND length(run_id) = 30),
    routine_id TEXT NOT NULL REFERENCES routines(routine_id),
    status TEXT NOT NULL CHECK (status IN ('RUNNING', 'COMPLETED', 'FAILED', 'BLOCKED', 'INTERRUPTED')),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    finished_at INTEGER CHECK (finished_at IS NULL OR finished_at >= started_at),
    evidence_ref TEXT CHECK (evidence_ref IS NULL OR length(evidence_ref) BETWEEN 1 AND 1024)
) STRICT;

CREATE INDEX routines_due_idx ON routines (state, next_run_at, routine_id);
CREATE INDEX routine_runs_routine_idx ON routine_runs (routine_id, run_id);
