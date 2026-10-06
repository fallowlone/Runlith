-- 0027: ADR-0031 side-effect intents (TASK-0126 dogfood).
--
-- Durable SideEffectIntent records for the frozen v1 kind registry
-- (worktree-materialize, workspace-cleanup) with the §2 machine
-- enforced by trigger as well as by the domain: only the twelve
-- legal edges may update state, and only state/evidence/timestamps
-- may change at all. One active (non-terminal, non-escalated) intent
-- per (lease, kind): concurrent creators arbitrate on the slot, and a
-- terminal or escalated intent frees it for an explicit successor.
-- Transitions append audit rows; escalations resolve exactly once.

CREATE TABLE side_effect_intents (
    intent_id TEXT PRIMARY KEY CHECK (length(intent_id) BETWEEN 1 AND 1024),
    kind TEXT NOT NULL CHECK (kind IN ('worktree-materialize', 'workspace-cleanup')),
    target_ref TEXT NOT NULL CHECK (length(target_ref) BETWEEN 1 AND 1024),
    params_hash TEXT NOT NULL CHECK (params_hash GLOB 'sha256:*' AND length(params_hash) = 71),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 1024),
    state TEXT NOT NULL CHECK (state IN (
        'PREPARED', 'INTENT_RECORDED', 'EXECUTING', 'CONFIRMED', 'FAILED',
        'AMBIGUOUS', 'RECONCILE', 'SAFE_TO_RETRY', 'SAFE_TO_CONTINUE', 'ESCALATE'
    )),
    task_id TEXT NOT NULL REFERENCES tasks(id),
    execution_id TEXT NOT NULL REFERENCES executions(id),
    lease_id TEXT NOT NULL REFERENCES workspace_leases(workspace_id),
    pre_state_fingerprint TEXT CHECK (
        pre_state_fingerprint IS NULL
        OR (length(pre_state_fingerprint) BETWEEN 1 AND 4096)
    ),
    outcome_evidence_refs TEXT NOT NULL CHECK (length(outcome_evidence_refs) <= 32768),
    policy_fingerprint TEXT NOT NULL CHECK (length(policy_fingerprint) BETWEEN 1 AND 1024),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE INDEX side_effect_intents_by_idempotency ON side_effect_intents(idempotency_key);

CREATE UNIQUE INDEX side_effect_active_slot
ON side_effect_intents(lease_id, kind)
WHERE state NOT IN ('CONFIRMED', 'FAILED', 'ESCALATE');

CREATE TRIGGER side_effect_intent_body_immutable
BEFORE UPDATE OF intent_id, kind, target_ref, params_hash, idempotency_key,
    task_id, execution_id, lease_id, pre_state_fingerprint,
    policy_fingerprint, created_at_ms ON side_effect_intents
BEGIN
    SELECT RAISE(ABORT, 'side-effect intent bindings are immutable');
END;

-- The §2 edge table, enforced for raw writers as well as the store:
-- exactly the twelve legal transitions may land.
CREATE TRIGGER side_effect_intent_legal_transition
BEFORE UPDATE OF state ON side_effect_intents
WHEN NOT (
    (OLD.state = 'PREPARED' AND NEW.state = 'INTENT_RECORDED')
    OR (OLD.state = 'INTENT_RECORDED' AND NEW.state = 'EXECUTING')
    OR (OLD.state = 'EXECUTING' AND NEW.state IN ('CONFIRMED', 'FAILED', 'AMBIGUOUS'))
    OR (OLD.state = 'AMBIGUOUS' AND NEW.state = 'RECONCILE')
    OR (OLD.state = 'RECONCILE' AND NEW.state IN (
        'CONFIRMED', 'SAFE_TO_RETRY', 'SAFE_TO_CONTINUE', 'ESCALATE'))
    OR (OLD.state = 'SAFE_TO_RETRY' AND NEW.state = 'EXECUTING')
    OR (OLD.state = 'SAFE_TO_CONTINUE' AND NEW.state = 'EXECUTING')
)
BEGIN
    SELECT RAISE(ABORT, 'illegal side-effect transition');
END;

CREATE TRIGGER side_effect_intent_no_delete
BEFORE DELETE ON side_effect_intents
BEGIN
    SELECT RAISE(ABORT, 'side-effect intents are permanent');
END;

CREATE TABLE side_effect_transitions (
    transition_id INTEGER PRIMARY KEY AUTOINCREMENT,
    intent_id TEXT NOT NULL REFERENCES side_effect_intents(intent_id),
    from_state TEXT NOT NULL,
    to_state TEXT NOT NULL,
    evidence_refs TEXT NOT NULL CHECK (length(evidence_refs) <= 32768),
    transitioned_at_ms INTEGER NOT NULL CHECK (transitioned_at_ms >= 0)
) STRICT;

CREATE INDEX side_effect_transitions_by_intent ON side_effect_transitions(intent_id);

CREATE TRIGGER side_effect_transition_no_update
BEFORE UPDATE ON side_effect_transitions
BEGIN
    SELECT RAISE(ABORT, 'side-effect transitions are immutable');
END;

CREATE TRIGGER side_effect_transition_no_delete
BEFORE DELETE ON side_effect_transitions
BEGIN
    SELECT RAISE(ABORT, 'side-effect transitions are immutable');
END;

CREATE TABLE side_effect_escalations (
    intent_id TEXT PRIMARY KEY REFERENCES side_effect_intents(intent_id),
    code TEXT NOT NULL CHECK (length(code) BETWEEN 1 AND 256),
    evidence_refs TEXT NOT NULL CHECK (length(evidence_refs) <= 32768),
    escalated_at_ms INTEGER NOT NULL CHECK (escalated_at_ms >= 0),
    resolved_at_ms INTEGER CHECK (resolved_at_ms IS NULL OR resolved_at_ms >= 0),
    resolution TEXT CHECK (resolution IS NULL OR resolution IN ('discarded', 'successor')),
    successor_intent_id TEXT REFERENCES side_effect_intents(intent_id)
) STRICT;

CREATE TRIGGER side_effect_escalation_no_delete
BEFORE DELETE ON side_effect_escalations
BEGIN
    SELECT RAISE(ABORT, 'side-effect escalations are permanent');
END;

CREATE TRIGGER side_effect_escalation_body_immutable
BEFORE UPDATE OF intent_id, code, evidence_refs, escalated_at_ms ON side_effect_escalations
BEGIN
    SELECT RAISE(ABORT, 'side-effect escalation facts are immutable');
END;

-- Escalations attach only to ESCALATE intents.
CREATE TRIGGER side_effect_escalation_requires_escalate
BEFORE INSERT ON side_effect_escalations
WHEN (SELECT state FROM side_effect_intents WHERE intent_id = NEW.intent_id)
    IS NOT 'ESCALATE'
BEGIN
    SELECT RAISE(ABORT, 'escalation requires an ESCALATE intent');
END;

-- Resolution lands at most once (sticky).
CREATE TRIGGER side_effect_escalation_resolution_sticky
BEFORE UPDATE OF resolved_at_ms, resolution, successor_intent_id ON side_effect_escalations
WHEN OLD.resolved_at_ms IS NOT NULL
    AND (NEW.resolved_at_ms IS NOT OLD.resolved_at_ms
        OR NEW.resolution IS NOT OLD.resolution
        OR NEW.successor_intent_id IS NOT OLD.successor_intent_id)
BEGIN
    SELECT RAISE(ABORT, 'side-effect escalation resolution is permanent');
END;
