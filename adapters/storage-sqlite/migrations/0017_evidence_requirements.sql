-- TASK-0123: explicit G8 evidence requirements on canonical Task criteria
-- (ADR-0030 §2/§6).
--
-- Before this migration, a Task's acceptance obligations were criterion
-- identities only and every derived planning criterion defaulted to
-- `automated-test` inside the derivation (ADR-0027 TASK-0093 amendment).
-- The default now exists as visible data, one row per acceptance criterion,
-- so no reader has to rediscover it and no plan-side method can be silently
-- downgraded (TASK-0096 defect class).
--
-- Legacy rows are backfilled with the explicit v1 default: requirement kind
-- `test-suite`, the kind's default subject binding, `required = 1` and NO
-- policy fingerprint (NULL = "not policy-derived"). Per ADR-0030 CV-7 such
-- requirements are stale input and must be re-derived or re-validated at the
-- verification boundary; they are never current policy authority.
--
-- The backfill doubles as the fail-closed legacy check: the UNIQUE constraint
-- on (task_id, criterion_id) rejects a stored Task whose acceptance list
-- already carries a duplicate, and the migration then aborts instead of
-- silently dropping or merging an obligation.

CREATE TABLE task_evidence_requirements (
    task_id            TEXT NOT NULL REFERENCES tasks (id),
    position           INTEGER NOT NULL CHECK (position >= 0),
    criterion_id       TEXT NOT NULL CHECK (length(trim(criterion_id)) > 0),
    requirement_kind   TEXT NOT NULL CHECK (requirement_kind IN (
        'process-exit', 'test-suite', 'artifact-hash', 'fs-state', 'git-state',
        'independent-review', 'security-check', 'human-approval'
    )),
    subject_binding    TEXT NOT NULL CHECK (subject_binding IN (
        'content', 'content+toolchain', 'content+toolchain+config'
    )),
    required           INTEGER NOT NULL CHECK (required IN (0, 1)),
    policy_fingerprint TEXT NULL CHECK (
        policy_fingerprint IS NULL
        OR (length(trim(policy_fingerprint)) > 0
            AND length(policy_fingerprint) <= 256)
    ),
    PRIMARY KEY (task_id, position),
    UNIQUE (task_id, criterion_id)
);

CREATE TABLE task_evidence_requirement_params (
    task_id      TEXT NOT NULL,
    criterion_id TEXT NOT NULL,
    position     INTEGER NOT NULL CHECK (position >= 0),
    param_key    TEXT NOT NULL CHECK (length(trim(param_key)) > 0),
    param_value  TEXT NOT NULL CHECK (length(trim(param_value)) > 0),
    PRIMARY KEY (task_id, criterion_id, position),
    UNIQUE (task_id, criterion_id, param_key),
    FOREIGN KEY (task_id, criterion_id)
        REFERENCES task_evidence_requirements (task_id, criterion_id)
);

INSERT INTO task_evidence_requirements
    (task_id, position, criterion_id, requirement_kind, subject_binding, required, policy_fingerprint)
SELECT
    ts.task_id,
    ts.position,
    ts.value,
    'test-suite',
    'content+toolchain+config',
    1,
    NULL
FROM task_strings AS ts
WHERE ts.slot = 'acceptance';
