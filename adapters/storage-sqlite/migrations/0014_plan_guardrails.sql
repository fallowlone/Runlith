-- TASK-0078: durable G7 planning control state (ADR-0027 + ADR-0028).
--
-- Guardrails are defined against branch lineage and triggering evidence, not
-- against the lifetime of an in-memory planning object. Two narrow tables:
--
-- plan_revision_lineage: one row per committed revision, written in the same
--   transaction as the revision. It records the V7 decomposition fingerprint
--   and the trusted triggering-evidence key that produced the revision, so V7
--   lineage and accepted-revisions-per-evidence-cycle survive restarts.
--
-- plan_guardrail_events: per (branch, triggering-evidence key) planner-call
--   count, charged input tokens (measured usage, or a conservative charge
--   when a started call reported none) and the L3 consecutive-rejection
--   counter. Measured usage itself stays in plan_cost_receipts.
--
-- Neither table stores prompts, transcripts, provider output, or secrets.

CREATE TABLE plan_revision_lineage (
    plan_branch_id            TEXT NOT NULL,
    revision_seq              INTEGER NOT NULL CHECK (revision_seq >= 1),
    parent_task_ref           TEXT NOT NULL CHECK (length(trim(parent_task_ref)) > 0),
    decomposition_fingerprint TEXT NOT NULL CHECK (
        length(decomposition_fingerprint) = 71
        AND substr(decomposition_fingerprint, 1, 7) = 'sha256:'
    ),
    trigger_evidence_key      TEXT NOT NULL CHECK (
        length(trigger_evidence_key) = 71
        AND substr(trigger_evidence_key, 1, 7) = 'sha256:'
    ),
    planning_input_snapshot_id TEXT NOT NULL CHECK (length(trim(planning_input_snapshot_id)) > 0),
    planning_input_fingerprint TEXT NOT NULL CHECK (
        length(planning_input_fingerprint) = 71
        AND substr(planning_input_fingerprint, 1, 7) = 'sha256:'
    ),
    planning_run_id            TEXT NOT NULL CHECK (length(trim(planning_run_id)) > 0),
    planning_run_generation    INTEGER NOT NULL CHECK (planning_run_generation >= 1),
    PRIMARY KEY (plan_branch_id, revision_seq),
    FOREIGN KEY (plan_branch_id, revision_seq)
        REFERENCES plan_revisions (plan_branch_id, revision_seq)
);

CREATE INDEX plan_revision_lineage_trigger
    ON plan_revision_lineage (plan_branch_id, trigger_evidence_key);

CREATE TABLE plan_guardrail_events (
    plan_branch_id         TEXT NOT NULL CHECK (length(plan_branch_id) > 0),
    trigger_evidence_key   TEXT NOT NULL CHECK (
        length(trigger_evidence_key) = 71
        AND substr(trigger_evidence_key, 1, 7) = 'sha256:'
    ),
    planner_calls          INTEGER NOT NULL DEFAULT 0 CHECK (planner_calls >= 0),
    charged_input_tokens   INTEGER NOT NULL DEFAULT 0 CHECK (charged_input_tokens >= 0),
    consecutive_rejections INTEGER NOT NULL DEFAULT 0 CHECK (consecutive_rejections >= 0),
    PRIMARY KEY (plan_branch_id, trigger_evidence_key)
);

-- Immutable product-authored planning inputs. The branch may be genesis
-- (there is no plan_branches row yet), so branch identity is deliberately not
-- a foreign key here.
CREATE TABLE planning_input_snapshots (
    snapshot_id          TEXT PRIMARY KEY CHECK (length(trim(snapshot_id)) > 0),
    plan_branch_id       TEXT NOT NULL CHECK (length(trim(plan_branch_id)) > 0),
    task_ref             TEXT NOT NULL CHECK (length(trim(task_ref)) > 0),
    expected_plan_head   INTEGER NOT NULL CHECK (expected_plan_head >= 0),
    baseline_risk        TEXT NOT NULL CHECK (baseline_risk IN ('low', 'medium', 'high', 'critical')),
    policy_fingerprint   TEXT NOT NULL CHECK (length(trim(policy_fingerprint)) > 0),
    input_fingerprint    TEXT NOT NULL UNIQUE CHECK (
        length(input_fingerprint) = 71
        AND substr(input_fingerprint, 1, 7) = 'sha256:'
    ),
    risk_grant_proposal_hash TEXT NULL,
    risk_grant_expires_at_ms INTEGER NULL CHECK (risk_grant_expires_at_ms IS NULL OR risk_grant_expires_at_ms >= 0),
    risk_grant_consumed      INTEGER NULL CHECK (risk_grant_consumed IS NULL OR risk_grant_consumed IN (0, 1)),
    created_at_ms        INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CHECK (
        (risk_grant_proposal_hash IS NULL AND risk_grant_expires_at_ms IS NULL AND risk_grant_consumed IS NULL)
        OR
        (risk_grant_proposal_hash IS NOT NULL AND risk_grant_expires_at_ms IS NOT NULL AND risk_grant_consumed IS NOT NULL)
    )
);

CREATE TABLE planning_input_scope_roots (
    snapshot_id TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    position    INTEGER NOT NULL CHECK (position >= 0),
    scope_root  TEXT NOT NULL CHECK (length(trim(scope_root)) > 0),
    PRIMARY KEY (snapshot_id, position),
    UNIQUE (snapshot_id, scope_root)
);

CREATE TABLE planning_input_criteria (
    snapshot_id         TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    position            INTEGER NOT NULL CHECK (position >= 0),
    criterion_id        TEXT NOT NULL CHECK (length(trim(criterion_id)) > 0),
    verification_method TEXT NOT NULL CHECK (verification_method IN (
        'automated-test', 'independent-review', 'security-check', 'human-approval'
    )),
    PRIMARY KEY (snapshot_id, position),
    UNIQUE (snapshot_id, criterion_id, verification_method)
);

CREATE TABLE planning_input_evidence (
    snapshot_id  TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    position     INTEGER NOT NULL CHECK (position >= 0),
    evidence_ref TEXT NOT NULL CHECK (length(trim(evidence_ref)) > 0),
    PRIMARY KEY (snapshot_id, position),
    UNIQUE (snapshot_id, evidence_ref)
);

CREATE TABLE planning_input_lineage (
    snapshot_id TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    position    INTEGER NOT NULL CHECK (position >= 0),
    fingerprint TEXT NOT NULL CHECK (
        length(fingerprint) = 71 AND substr(fingerprint, 1, 7) = 'sha256:'
    ),
    PRIMARY KEY (snapshot_id, position),
    UNIQUE (snapshot_id, fingerprint)
);

CREATE TABLE planning_input_heads (
    plan_branch_id    TEXT PRIMARY KEY CHECK (length(trim(plan_branch_id)) > 0),
    snapshot_id       TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    input_fingerprint TEXT NOT NULL CHECK (
        length(input_fingerprint) = 71 AND substr(input_fingerprint, 1, 7) = 'sha256:'
    )
);

-- One durable operation identity per planning generation. A new run fences
-- older workers before they can publish a PlanRevision.
CREATE TABLE planning_runs (
    planning_run_id         TEXT PRIMARY KEY CHECK (length(trim(planning_run_id)) > 0),
    plan_branch_id          TEXT NOT NULL CHECK (length(trim(plan_branch_id)) > 0),
    generation              INTEGER NOT NULL CHECK (generation >= 1),
    snapshot_id             TEXT NOT NULL REFERENCES planning_input_snapshots(snapshot_id),
    input_fingerprint       TEXT NOT NULL CHECK (
        length(input_fingerprint) = 71 AND substr(input_fingerprint, 1, 7) = 'sha256:'
    ),
    expected_plan_head      INTEGER NOT NULL CHECK (expected_plan_head >= 0),
    trigger_evidence_key    TEXT NOT NULL CHECK (
        length(trigger_evidence_key) = 71 AND substr(trigger_evidence_key, 1, 7) = 'sha256:'
    ),
    state                   TEXT NOT NULL CHECK (state IN ('ACTIVE', 'SUCCEEDED', 'REFUSED', 'SUPERSEDED', 'FAILED')),
    created_at_ms           INTEGER NOT NULL CHECK (created_at_ms >= 0),
    finished_at_ms          INTEGER NULL CHECK (finished_at_ms IS NULL OR finished_at_ms >= 0),
    UNIQUE (plan_branch_id, generation)
);

CREATE TABLE planning_run_heads (
    plan_branch_id         TEXT PRIMARY KEY CHECK (length(trim(plan_branch_id)) > 0),
    planning_run_id        TEXT NOT NULL REFERENCES planning_runs(planning_run_id),
    generation             INTEGER NOT NULL CHECK (generation >= 1)
);

CREATE INDEX planning_runs_branch_generation
    ON planning_runs(plan_branch_id, generation);
