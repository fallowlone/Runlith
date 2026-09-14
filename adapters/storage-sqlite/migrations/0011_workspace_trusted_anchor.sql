-- 0011_workspace_trusted_anchor: durable trusted common-Git anchor sidecar
-- (TASK-0071 follow-up, closes F7 forged linked-worktree identity).
--
-- New relational state (current-state authority for trusted repository
-- binding reads; never journaled, never Task semantics, never lease
-- snapshot content):
--
--   workspace_trusted_anchor — one row per durable lease: the canonical
--     common-Git directory captured at admission (absolute, canonical),
--     capture time millis, and capture provenance. Immutable: UPDATE is
--     refused by trigger; deletion only via cascade when the owning lease
--     row is deleted (leases are tombstoned to REMOVED, not deleted, so
--     anchors persist for history; no explicit anchor DELETE exists).
--
-- No backfill: pre-existing leases have no row and fail closed until
-- re-admission with a new lease. No journal events for anchors.

CREATE TABLE workspace_trusted_anchor (
    workspace_id TEXT PRIMARY KEY REFERENCES workspace_leases (workspace_id) ON DELETE CASCADE,
    canonical_commondir TEXT NOT NULL CHECK (length(trim(canonical_commondir)) > 0),
    captured_at_millis INTEGER NOT NULL CHECK (captured_at_millis >= 0),
    capture_provenance TEXT NOT NULL DEFAULT 'workspace-application-service/allocate_workspace'
        CHECK (length(trim(capture_provenance)) > 0)
);

CREATE TRIGGER workspace_trusted_anchor_no_update
BEFORE UPDATE ON workspace_trusted_anchor
BEGIN
    SELECT RAISE(ABORT, 'trusted anchor is immutable');
END;
