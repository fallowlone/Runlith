-- 0007_managed_identity_reservations: durable case-fold-safe managed
-- workspace identity backstop (TASK-0053, G5 B-CF).
--
-- TASK-0052 proved the TASK-0051 entry preflight insufficient: it compared
-- managed branches as exact strings, so two valid ExecutionIds differing
-- only in ASCII case (`E-CASE` vs `E-case`) both passed, and the loser
-- wedged RUNNING on a later Git `BranchCollision` (loose refs) or aliased
-- the winner's branch and worktree HEAD (packed refs).
--
-- The TASK-0053 v2 derivation emits canonical-lowercase identities
-- (`e2-…` readable / `h2-…` digest), so distinct valid ExecutionIds derive
-- branches that are distinct even after ASCII case folding. The remaining
-- same-string hazard is a digest collision (bounded hash, larger domain):
-- deterministic and collision-resistant, NOT mathematically injective.
-- That hazard fails closed here instead of aliasing.
--
-- New relational state (current-state authority for identity conflict
-- reads):
--
--   managed_identity_reservations — one row per admitted-or-allocated
--     managed branch: its canonical branch ref (PRIMARY KEY: global
--     branch uniqueness across executions — conservative and fail-closed:
--     the same branch string in any repository scope collides, so two
--     textual repository paths for one physical repository cannot bypass
--     the backstop, at the cost of refusing a cross-repository digest
--     collision that would otherwise be harmless; at ~2^-128 birthday
--     probability that cost is negligible), the owning Execution (UNIQUE:
--     one Execution reserves at most one branch), and the claiming layer
--     (`admission` for the atomic entry path, `lease` for direct
--     allocation paths that bypass admission, e.g. tests and legacy
--     callers). Foreign keys are restrictive (default NO ACTION).
--
-- Why a binary PRIMARY KEY is sufficient (no NOCASE): every row is a v2
-- canonical branch (lowercase ASCII only — enforced at the admission and
-- allocation boundaries, which accept only `agent/e2-…` / `agent/h2-…`
-- refs). Binary equality therefore coincides with ASCII-folded equality.
-- Mixed-case historical branches live only in `workspace_leases` (never
-- rederived, never inserted here) and are compared with an explicit
-- `COLLATE NOCASE` (ASCII-only, matching the ASCII-only managed alphabet)
-- wherever old and new identities meet.
--
-- Lifecycle: the atomic entry transaction inserts the reservation alongside
-- the Task claim/start, the Execution row, the Attempt reservation, and
-- their journal facts (see `entry_store.rs`) — a second admission for the
-- same branch rolls back with zero unrelated mutations. The later lease
-- allocation claims (or verifies ownership of) the same reservation before
-- inserting its lease row. Reservations are intent rows, not workspace
-- state: no journal event type is added and no lease lifecycle state is
-- invented. Recovery of stranded pre-attempt entries (including their
-- reservations) belongs to TASK-0054.
--
-- No journal rebuild: existing event types and scope contracts are
-- unchanged, so the journal table is untouched. Existing
-- `workspace_leases` rows are preserved verbatim (no UNIQUE added there,
-- so historical H4 artifacts can never fail this migration).

CREATE TABLE managed_identity_reservations (
    branch_ref   TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL UNIQUE REFERENCES executions (id),
    origin       TEXT NOT NULL CHECK (origin IN ('admission', 'lease'))
);

CREATE INDEX managed_identity_reservations_execution ON managed_identity_reservations (execution_id);
