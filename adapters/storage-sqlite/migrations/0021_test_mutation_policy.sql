CREATE TABLE test_mutation_policy_revisions (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    revision INTEGER NOT NULL CHECK (revision > 0),
    serialized TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (task_id, revision)
);

INSERT INTO test_mutation_policy_revisions(task_id, revision, serialized, fingerprint)
SELECT id, 1, 'forbidden/v1',
       'sha256:428d8e5c6468e49d057ec6131535f346b4ef2d76e312e027a6a9f2b916d656c8'
FROM tasks;

CREATE TRIGGER test_mutation_policy_revision_no_update
BEFORE UPDATE ON test_mutation_policy_revisions
BEGIN SELECT RAISE(ABORT, 'test mutation policy revisions are immutable'); END;

CREATE TRIGGER test_mutation_policy_revision_no_delete
BEFORE DELETE ON test_mutation_policy_revisions
BEGIN SELECT RAISE(ABORT, 'test mutation policy revisions are immutable'); END;

CREATE TABLE evaluated_policy_bases (
    execution_id TEXT PRIMARY KEY REFERENCES executions(id),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    workspace_id TEXT NOT NULL REFERENCES workspace_leases(workspace_id),
    policy_revision INTEGER NOT NULL,
    policy_fingerprint TEXT NOT NULL,
    base_revision TEXT NOT NULL,
    FOREIGN KEY (task_id, policy_revision)
        REFERENCES test_mutation_policy_revisions(task_id, revision)
);

CREATE TRIGGER evaluated_policy_base_no_update
BEFORE UPDATE ON evaluated_policy_bases
BEGIN SELECT RAISE(ABORT, 'evaluated policy base bindings are immutable'); END;

CREATE TRIGGER evaluated_policy_base_no_delete
BEFORE DELETE ON evaluated_policy_bases
BEGIN SELECT RAISE(ABORT, 'evaluated policy base bindings are immutable'); END;
