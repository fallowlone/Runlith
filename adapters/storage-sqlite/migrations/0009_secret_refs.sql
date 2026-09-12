-- TASK-0063: opaque secret metadata only. Secret values are forbidden here.

CREATE TABLE secret_refs (
    ref_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(trim(ref_id)) > 0)
);

CREATE TABLE secret_bindings (
    binding_id TEXT PRIMARY KEY NOT NULL
        CHECK (length(trim(binding_id)) > 0),
    secret_ref_id TEXT NOT NULL
        REFERENCES secret_refs(ref_id) ON DELETE CASCADE,
    target_kind TEXT NOT NULL
        CHECK (target_kind IN ('execution', 'tool')),
    target_id TEXT NOT NULL
        CHECK (length(trim(target_id)) > 0)
);

CREATE INDEX idx_secret_bindings_ref
    ON secret_bindings(secret_ref_id);
