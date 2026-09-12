-- 0008_approvals: durable bounded approval lifecycle (TASK-0062, G6).
--
-- Approvals are Policy/Security current state, separate from the Event
-- Journal and the future Security Audit store. The binding fingerprint is
-- computed by crates/policy over the exact request, policy fingerprint,
-- expiry and effective authority. A row may be consumed only once.

CREATE TABLE approvals (
    request_id           TEXT PRIMARY KEY,
    binding_fingerprint  TEXT NOT NULL,
    operation_id         TEXT NOT NULL,
    requester            TEXT NOT NULL,
    task_id              TEXT NOT NULL,
    execution_id         TEXT,
    attempt_id           TEXT,
    policy_fingerprint   TEXT NOT NULL,
    expires_at_ms        INTEGER NOT NULL CHECK (expires_at_ms >= 0),
    state                TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'denied', 'consumed')),
    approver             TEXT,
    decided_at_ms        INTEGER CHECK (decided_at_ms IS NULL OR decided_at_ms >= 0),
    consumed_at_ms       INTEGER CHECK (consumed_at_ms IS NULL OR consumed_at_ms >= 0),
    CHECK (
        (state = 'pending' AND approver IS NULL AND decided_at_ms IS NULL AND consumed_at_ms IS NULL)
        OR (state IN ('approved', 'denied') AND approver IS NOT NULL AND decided_at_ms IS NOT NULL AND consumed_at_ms IS NULL)
        OR (state = 'consumed' AND approver IS NOT NULL AND decided_at_ms IS NOT NULL AND consumed_at_ms IS NOT NULL)
    )
);

CREATE TABLE approval_capabilities (
    request_id     TEXT NOT NULL REFERENCES approvals (request_id),
    authority_set  TEXT NOT NULL CHECK (authority_set IN ('requested', 'effective')),
    position       INTEGER NOT NULL CHECK (position >= 0),
    kind           TEXT NOT NULL,
    workspace      TEXT,
    scope          TEXT NOT NULL,
    PRIMARY KEY (request_id, authority_set, position)
);

CREATE INDEX approvals_state ON approvals (state, expires_at_ms);
