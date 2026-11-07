-- ADR-0051: the acceptance statement a requirement was stamped with. It is
-- part of the requirement revision, so verdicts and reviews bind to the exact
-- text. NULL for every existing requirement (no backfill, no invented text).
ALTER TABLE task_evidence_requirements ADD COLUMN statement TEXT
    CHECK (statement IS NULL OR (length(trim(statement)) > 0 AND length(CAST(statement AS BLOB)) <= 1024));
