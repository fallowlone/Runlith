-- 0029: G9 execution continuity (TASK-0171, ADR-0043).
--
-- Epochs, checkpoints, working notes, history refs, session/migration
-- links, budget events, receipt refs. Staging is a checkpoint state,
-- never a separate store: only COMMITTED rows are readable, and only
-- the commit pointer swap (state flip) makes a checkpoint readable.
-- Crash windows (ADR-0043 section 9) resolve through the activation
-- record, never through staged leftovers.
--
-- Checkpoint source refs reuse the ADR-0042 SourceRef mapping from
-- 0028 (TASK/REPO_PATH/PLAN/ATTEMPT/DECISION/EVENT/MEMORY/OBSERVATION).
-- Task/execution/attempt/provider refs cap at 256 chars per the
-- domain id bounds.

CREATE TABLE continuity_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL CHECK (value >= 0)
) STRICT;

INSERT INTO continuity_meta (key, value) VALUES ('checkpoint_seq', 0);

CREATE TABLE continuity_epochs (
    epoch_id TEXT PRIMARY KEY CHECK (
        epoch_id GLOB 'ctxepoch_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(epoch_id) = 35
    ),
    task_ref TEXT NOT NULL CHECK (length(task_ref) BETWEEN 1 AND 256),
    execution_ref TEXT NOT NULL CHECK (length(execution_ref) BETWEEN 1 AND 256),
    epoch_seq INTEGER NOT NULL CHECK (epoch_seq >= 0),
    state TEXT NOT NULL CHECK (state IN ('ACTIVE', 'INDEXED')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    checkpointed_by TEXT CHECK (
        checkpointed_by IS NULL
        OR (
            checkpointed_by GLOB 'ctxckpt_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
            AND length(checkpointed_by) = 34
        )
    ),
    UNIQUE (task_ref, execution_ref, epoch_seq)
) STRICT;

CREATE UNIQUE INDEX continuity_one_active_per_chain
    ON continuity_epochs(task_ref, execution_ref)
    WHERE state = 'ACTIVE';

CREATE INDEX continuity_epochs_by_chain
    ON continuity_epochs(task_ref, execution_ref, epoch_seq);

CREATE TABLE continuity_checkpoints (
    checkpoint_id TEXT PRIMARY KEY CHECK (
        checkpoint_id GLOB 'ctxckpt_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(checkpoint_id) = 34
    ),
    epoch_ref TEXT NOT NULL REFERENCES continuity_epochs(epoch_id),
    task_ref TEXT NOT NULL CHECK (length(task_ref) BETWEEN 1 AND 256),
    execution_ref TEXT NOT NULL CHECK (length(execution_ref) BETWEEN 1 AND 256),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    format_version INTEGER NOT NULL CHECK (format_version >= 0),
    parent_checkpoint TEXT CHECK (
        parent_checkpoint IS NULL
        OR (
            parent_checkpoint GLOB 'ctxckpt_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
            AND length(parent_checkpoint) = 34
        )
    ),
    continuation TEXT NOT NULL CHECK (length(continuation) <= 4096),
    context_plan_ref TEXT NOT NULL CHECK (
        context_plan_ref GLOB 'ctxplan_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(context_plan_ref) = 34
    ),
    sensitivity_ceiling TEXT NOT NULL CHECK (
        sensitivity_ceiling IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')
    ),
    state TEXT NOT NULL CHECK (state IN ('STAGED', 'COMMITTED')),
    staged_hash TEXT NOT NULL CHECK (
        staged_hash GLOB 'sha256:[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'
    ),
    committed_at INTEGER CHECK (committed_at IS NULL OR committed_at >= 0),
    store_revision INTEGER CHECK (store_revision IS NULL OR store_revision >= 0),
    CHECK ((state = 'COMMITTED') = (committed_at IS NOT NULL)),
    CHECK ((state = 'COMMITTED') = (store_revision IS NOT NULL))
) STRICT;

CREATE INDEX continuity_checkpoints_by_epoch ON continuity_checkpoints(epoch_ref);
CREATE INDEX continuity_checkpoints_by_chain
    ON continuity_checkpoints(task_ref, execution_ref, created_at);

CREATE TABLE continuity_checkpoint_sources (
    checkpoint_id TEXT NOT NULL REFERENCES continuity_checkpoints(checkpoint_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    source_kind TEXT NOT NULL CHECK (
        source_kind IN (
            'TASK', 'REPO_PATH', 'PLAN', 'ATTEMPT',
            'DECISION', 'EVENT', 'MEMORY', 'OBSERVATION'
        )
    ),
    source_p1 TEXT NOT NULL DEFAULT '' CHECK (length(source_p1) <= 4096),
    source_p2 TEXT NOT NULL DEFAULT '' CHECK (length(source_p2) <= 4096),
    source_n1 INTEGER NOT NULL CHECK (source_n1 >= 0),
    source_span_start INTEGER CHECK (source_span_start IS NULL OR source_span_start >= 0),
    source_span_end INTEGER CHECK (source_span_end IS NULL OR source_span_end >= 0),
    PRIMARY KEY (checkpoint_id, position),
    CHECK ((source_span_start IS NULL) = (source_span_end IS NULL)),
    CHECK (source_span_start IS NULL OR source_span_start <= source_span_end),
    CHECK ((source_kind = 'EVENT') = (source_p1 = '')),
    CHECK ((source_kind <> 'REPO_PATH') OR (length(source_p2) >= 1))
) STRICT;

CREATE TABLE continuity_checkpoint_evidence (
    checkpoint_id TEXT NOT NULL REFERENCES continuity_checkpoints(checkpoint_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    evidence_ref TEXT NOT NULL CHECK (length(evidence_ref) BETWEEN 1 AND 4096),
    PRIMARY KEY (checkpoint_id, position)
) STRICT;

CREATE TABLE continuity_checkpoint_notes (
    checkpoint_id TEXT NOT NULL REFERENCES continuity_checkpoints(checkpoint_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    note_id TEXT NOT NULL REFERENCES continuity_notes(note_id),
    PRIMARY KEY (checkpoint_id, position)
) STRICT;

CREATE TABLE continuity_notes (
    note_id TEXT PRIMARY KEY CHECK (
        note_id GLOB 'cknote_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(note_id) = 33
    ),
    epoch_ref TEXT NOT NULL REFERENCES continuity_epochs(epoch_id),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    author TEXT NOT NULL CHECK (author IN ('HUMAN', 'AGENT', 'SYSTEM')),
    body_kind TEXT NOT NULL CHECK (body_kind IN ('INLINE', 'REF')),
    body TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 4096),
    sensitivity TEXT NOT NULL CHECK (
        sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')
    )
) STRICT;

CREATE INDEX continuity_notes_by_epoch ON continuity_notes(epoch_ref);

CREATE TABLE continuity_activations (
    checkpoint_id TEXT PRIMARY KEY REFERENCES continuity_checkpoints(checkpoint_id),
    successor TEXT CHECK (
        successor IS NULL
        OR (
            successor GLOB 'ctxepoch_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
            AND length(successor) = 35
        )
    ),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    completed_at INTEGER CHECK (completed_at IS NULL OR completed_at >= started_at)
) STRICT;

CREATE TABLE continuity_history_refs (
    history_ref TEXT PRIMARY KEY CHECK (
        history_ref GLOB 'histref_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(history_ref) = 34
    ),
    epoch_id TEXT NOT NULL REFERENCES continuity_epochs(epoch_id),
    kind TEXT NOT NULL CHECK (kind IN ('CHECKPOINT', 'NOTE', 'RECEIPT')),
    target_id TEXT NOT NULL CHECK (length(target_id) BETWEEN 1 AND 64),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 4096),
    body TEXT NOT NULL CHECK (length(body) <= 4096)
) STRICT;

CREATE INDEX continuity_history_by_epoch ON continuity_history_refs(epoch_id);

CREATE TABLE continuity_session_links (
    session_ref TEXT NOT NULL CHECK (length(session_ref) BETWEEN 1 AND 4096),
    epoch_id TEXT NOT NULL REFERENCES continuity_epochs(epoch_id),
    attempt_ref TEXT NOT NULL CHECK (length(attempt_ref) BETWEEN 1 AND 256),
    provider_id TEXT NOT NULL CHECK (length(provider_id) BETWEEN 1 AND 256),
    linked_at INTEGER NOT NULL CHECK (linked_at >= 0),
    PRIMARY KEY (session_ref, epoch_id)
) STRICT;

CREATE TABLE continuity_migration_links (
    from_checkpoint TEXT NOT NULL REFERENCES continuity_checkpoints(checkpoint_id),
    to_epoch TEXT NOT NULL REFERENCES continuity_epochs(epoch_id),
    migrated_at INTEGER NOT NULL CHECK (migrated_at >= 0),
    PRIMARY KEY (from_checkpoint, to_epoch)
) STRICT;

CREATE TABLE continuity_budget_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_ref TEXT NOT NULL CHECK (
        plan_ref GLOB 'ctxplan_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(plan_ref) = 34
    ),
    hard INTEGER NOT NULL CHECK (hard IN (0, 1)),
    recorded_at INTEGER NOT NULL CHECK (recorded_at >= 0)
) STRICT;

CREATE TABLE continuity_receipt_refs (
    history_ref TEXT PRIMARY KEY REFERENCES continuity_history_refs(history_ref),
    epoch_id TEXT NOT NULL REFERENCES continuity_epochs(epoch_id),
    receipt_item_id TEXT NOT NULL CHECK (
        receipt_item_id GLOB 'ctxitem_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(receipt_item_id) = 34
    )
) STRICT;
