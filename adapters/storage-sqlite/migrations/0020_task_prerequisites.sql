-- TASK-0135 wiring: durable declared activation prerequisites on the
-- canonical Task record (ADR-0030, G8 prerequisite preflight).
--
-- Before this migration, `Prerequisite` existed only as in-memory
-- vocabulary: nothing could declare prerequisites durably, so the
-- preflight checker had no production input. One row per declared
-- prerequisite, in declaration order; the kind registry is the closed v1
-- vocabulary from `task-domain` (`git-ancestor-ref`, `repo-file-exists`,
-- `command-on-path`) and unknown kinds fail closed both here (CHECK) and
-- on read (domain reconstruction maps them to corruption, never silent
-- drops).
--
-- No backfill: legacy tasks simply have no declared prerequisites, which
-- the preflight gate reads as "nothing to check" — the pre-TASK-0135
-- behavior, never a fabricated obligation.

CREATE TABLE task_prerequisites (
    task_id           TEXT NOT NULL REFERENCES tasks (id),
    position          INTEGER NOT NULL CHECK (position >= 0),
    prerequisite_kind TEXT NOT NULL CHECK (prerequisite_kind IN (
        'git-ancestor-ref', 'repo-file-exists', 'command-on-path'
    )),
    value             TEXT NOT NULL CHECK (
        length(trim(value)) > 0 AND length(value) <= 256
    ),
    PRIMARY KEY (task_id, position),
    UNIQUE (task_id, prerequisite_kind, value)
);
