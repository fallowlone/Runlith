-- 0028: G9 context selection (TASK-0170, ADR-0042).
--
-- Durable (plan, items, trace) triples for admitted context selections.
-- Plans and traces are immutable once admitted; items carry their full
-- ADR-0042 section 1 fields plus the 9-field provenance envelope.
-- Selection decisions live in context_trace_decisions; plan sections
-- are recorded per item so a plan is explainable without re-running
-- selection. Source payload mapping per source_kind:
--   TASK:        p1 = task_id, n1 = event_seq
--   REPO_PATH:   p1 = rev, p2 = path, span_start/span_end (NULL = whole file)
--   PLAN:        p1 = branch_id, n1 = revision_seq
--   ATTEMPT:     p1 = execution_id, n1 = attempt_seq
--   DECISION:    p1 = request_id
--   EVENT:       n1 = seq
--   MEMORY:      p1 = opaque (owning gate decides the shape)
--   OBSERVATION: p1 = handle
-- The item source (source_*) and the envelope source (env_source_*)
-- are independent SourceRefs with the same mapping; decode must
-- never alias them.
--
-- Identifier columns pin prefix + exact 26-char Crockford suffix
-- (domain decode stays fail-closed as a second layer).
-- Budgets are token units while plan sections count items; no
-- cross-unit CAP is expressible here (items carry no cost).

CREATE TABLE context_plans (
    plan_id TEXT PRIMARY KEY CHECK (
        plan_id GLOB 'ctxplan_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(plan_id) = 34
    ),
    revision_seq INTEGER NOT NULL CHECK (revision_seq >= 0),
    budget_soft INTEGER NOT NULL CHECK (budget_soft >= 0),
    budget_hard INTEGER NOT NULL CHECK (budget_hard >= 0),
    counter_method TEXT NOT NULL CHECK (
        counter_method IN ('EXACT_TOKENIZER', 'CHAR_HEURISTIC')
    ),
    renderer TEXT NOT NULL CHECK (
        renderer GLOB 'renderer:*' AND length(renderer) BETWEEN 10 AND 1025
    ),
    -- Opaque policy fingerprint: the owning gate defines the shape
    -- (domain carries a free-form String); storage only caps length.
    policy_fingerprint TEXT NOT NULL CHECK (
        length(policy_fingerprint) BETWEEN 1 AND 4096
    ),
    CHECK (budget_soft <= budget_hard)
) STRICT;

CREATE TABLE context_items (
    item_id TEXT PRIMARY KEY CHECK (
        item_id GLOB 'ctxitem_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
        AND length(item_id) = 34
    ),
    plan_id TEXT NOT NULL REFERENCES context_plans(plan_id),
    section TEXT NOT NULL CHECK (
        section IN ('REQUIRED', 'SELECTED', 'DEFERRED', 'REJECTED')
    ),
    reason_code TEXT CHECK (
        reason_code IS NULL
        OR reason_code IN (
            'BUDGET_EXCEEDED', 'SUPERSEDED', 'SENSITIVITY_REFUSAL',
            'STALE_SOURCE', 'DUPLICATE_OF', 'TRUST_BELOW_FLOOR',
            'RENDER_LIMIT', 'DEFERRED_BY_POLICY'
        )
    ),
    kind TEXT NOT NULL CHECK (
        kind IN (
            'TASK_SNAPSHOT', 'FILE_EXCERPT', 'SYMBOL_SPAN', 'DIFF_HUNK',
            'PLAN_REVISION', 'ATTEMPT_RECORD', 'HUMAN_DECISION',
            'EVENT_FACT', 'MEMORY_NOTE', 'REVIEW_PACKET',
            'TOOL_OBSERVATION', 'COMPACTION_RECEIPT'
        )
    ),
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
    content_hash TEXT NOT NULL CHECK (
        content_hash GLOB 'sha256:[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'
    ),
    priority INTEGER NOT NULL CHECK (priority BETWEEN 0 AND 65535),
    required INTEGER NOT NULL CHECK (required IN (0, 1)),
    cache_class TEXT NOT NULL CHECK (
        cache_class IN ('STABLE_PREFIX', 'SESSION_EPHEMERAL', 'NO_CACHE')
    ),
    freshness_observed_at INTEGER NOT NULL CHECK (freshness_observed_at >= 0),
    freshness_rev TEXT NOT NULL CHECK (
        length(freshness_rev) BETWEEN 1 AND 4096
    ),
    freshness_stale_after INTEGER CHECK (
        freshness_stale_after IS NULL OR freshness_stale_after >= 0
    ),
    trust_level TEXT NOT NULL CHECK (
        trust_level IN ('VERIFIED', 'UNVERIFIED', 'UNTRUSTED')
    ),
    trust_reason TEXT NOT NULL CHECK (
        length(trust_reason) BETWEEN 1 AND 4096
    ),
    sensitivity TEXT NOT NULL CHECK (
        sensitivity IN ('PUBLIC', 'PROJECT', 'PRIVATE', 'RESTRICTED', 'SECRET')
    ),
    env_source_rev TEXT NOT NULL CHECK (
        length(env_source_rev) BETWEEN 1 AND 4096
    ),
    env_retrieved_at INTEGER NOT NULL CHECK (env_retrieved_at >= 0),
    env_selector TEXT NOT NULL CHECK (
        length(env_selector) BETWEEN 1 AND 4096
    ),
    env_reopen_handle TEXT CHECK (
        env_reopen_handle IS NULL OR length(env_reopen_handle) BETWEEN 1 AND 4096
    ),
    env_reopen_unavailable_reason TEXT CHECK (
        env_reopen_unavailable_reason IS NULL
        OR length(env_reopen_unavailable_reason) BETWEEN 1 AND 4096
    ),
    env_permission_scope TEXT NOT NULL CHECK (
        length(env_permission_scope) BETWEEN 1 AND 4096
    ),
    env_confidence INTEGER NOT NULL CHECK (
        env_confidence BETWEEN 0 AND 1000
    ),
    env_supersedes TEXT CHECK (
        env_supersedes IS NULL
        OR (
            env_supersedes GLOB 'ctxitem_[0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z][0-9A-HJ-KM-NP-TV-Z]'
            AND length(env_supersedes) = 34
        )
    ),
    env_supersession_reason TEXT CHECK (
        env_supersession_reason IS NULL
        OR length(env_supersession_reason) BETWEEN 1 AND 4096
    ),
    env_source_kind TEXT NOT NULL CHECK (
        env_source_kind IN (
            'TASK', 'REPO_PATH', 'PLAN', 'ATTEMPT',
            'DECISION', 'EVENT', 'MEMORY', 'OBSERVATION'
        )
    ),
    env_source_p1 TEXT NOT NULL DEFAULT '' CHECK (length(env_source_p1) <= 4096),
    env_source_p2 TEXT NOT NULL DEFAULT '' CHECK (length(env_source_p2) <= 4096),
    env_source_n1 INTEGER NOT NULL CHECK (env_source_n1 >= 0),
    env_source_span_start INTEGER CHECK (
        env_source_span_start IS NULL OR env_source_span_start >= 0
    ),
    env_source_span_end INTEGER CHECK (
        env_source_span_end IS NULL OR env_source_span_end >= 0
    ),
    CHECK (
        (env_reopen_handle IS NULL) <> (env_reopen_unavailable_reason IS NULL)
    ),
    CHECK (
        (env_supersedes IS NULL) = (env_supersession_reason IS NULL)
    ),
    CHECK (
        (section IN ('DEFERRED', 'REJECTED')) = (reason_code IS NOT NULL)
    ),
    CHECK (
        (source_span_start IS NULL) = (source_span_end IS NULL)
    ),
    CHECK (
        source_span_start IS NULL OR source_span_start <= source_span_end
    ),
    CHECK ((source_kind = 'EVENT') = (source_p1 = '')),
    CHECK ((source_kind <> 'REPO_PATH') OR (length(source_p2) >= 1)),
    CHECK (
        (env_source_span_start IS NULL) = (env_source_span_end IS NULL)
    ),
    CHECK (
        env_source_span_start IS NULL
        OR env_source_span_start <= env_source_span_end
    ),
    CHECK ((env_source_kind = 'EVENT') = (env_source_p1 = '')),
    CHECK ((env_source_kind <> 'REPO_PATH') OR (length(env_source_p2) >= 1))
) STRICT;

CREATE INDEX context_items_by_plan ON context_items(plan_id);

CREATE TABLE context_item_transforms (
    item_id TEXT NOT NULL REFERENCES context_items(item_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    transform TEXT NOT NULL CHECK (length(transform) BETWEEN 1 AND 4096),
    PRIMARY KEY (item_id, position)
) STRICT;

CREATE TABLE context_plan_hashes (
    plan_id TEXT NOT NULL REFERENCES context_plans(plan_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    content_hash TEXT NOT NULL CHECK (
        content_hash GLOB 'sha256:[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]'
    ),
    PRIMARY KEY (plan_id, position)
) STRICT;

CREATE TABLE context_cache_boundaries (
    plan_id TEXT NOT NULL REFERENCES context_plans(plan_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    cache_class TEXT NOT NULL CHECK (
        cache_class IN ('STABLE_PREFIX', 'SESSION_EPHEMERAL', 'NO_CACHE')
    ),
    PRIMARY KEY (plan_id, position)
) STRICT;

CREATE TABLE context_cache_boundary_items (
    plan_id TEXT NOT NULL,
    boundary_position INTEGER NOT NULL,
    item_position INTEGER NOT NULL CHECK (item_position >= 0),
    item_id TEXT NOT NULL REFERENCES context_items(item_id),
    PRIMARY KEY (plan_id, boundary_position, item_position),
    FOREIGN KEY (plan_id, boundary_position)
        REFERENCES context_cache_boundaries(plan_id, position)
) STRICT;

CREATE TABLE context_traces (
    plan_id TEXT PRIMARY KEY REFERENCES context_plans(plan_id)
) STRICT;

CREATE TABLE context_trace_decisions (
    plan_id TEXT NOT NULL REFERENCES context_traces(plan_id),
    item_id TEXT NOT NULL REFERENCES context_items(item_id),
    outcome TEXT NOT NULL CHECK (
        outcome IN ('SELECTED', 'DEFERRED', 'REJECTED')
    ),
    reason_code TEXT CHECK (
        reason_code IS NULL
        OR reason_code IN (
            'BUDGET_EXCEEDED', 'SUPERSEDED', 'SENSITIVITY_REFUSAL',
            'STALE_SOURCE', 'DUPLICATE_OF', 'TRUST_BELOW_FLOOR',
            'RENDER_LIMIT', 'DEFERRED_BY_POLICY'
        )
    ),
    rule TEXT NOT NULL CHECK (length(rule) BETWEEN 1 AND 4096),
    PRIMARY KEY (plan_id, item_id)
) STRICT;

CREATE INDEX context_trace_decisions_by_plan ON context_trace_decisions(plan_id);

CREATE TABLE context_trace_revs (
    plan_id TEXT NOT NULL REFERENCES context_traces(plan_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    source_rev TEXT NOT NULL CHECK (length(source_rev) BETWEEN 1 AND 4096),
    PRIMARY KEY (plan_id, position)
) STRICT;
