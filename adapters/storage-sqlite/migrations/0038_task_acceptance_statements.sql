-- ADR-0051: per-criterion acceptance statements on the canonical Task.
-- Descriptive text only; completion authority stays with the explicit
-- evidence requirements. No backfill: legacy tasks have no statements.
CREATE TABLE task_acceptance_statements (
    task_id      TEXT NOT NULL REFERENCES tasks (id),
    criterion_id TEXT NOT NULL,
    statement    TEXT NOT NULL CHECK (
        length(trim(statement)) > 0 AND length(CAST(statement AS BLOB)) <= 1024
    ),
    PRIMARY KEY (task_id, criterion_id)
);
