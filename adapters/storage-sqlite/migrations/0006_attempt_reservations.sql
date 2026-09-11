-- 0006_attempt_reservations: durable Attempt-identity reservations for
-- atomic execution-entry admission (TASK-0051, G5).
--
-- A reservation binds one future Attempt identity to the Execution admitted
-- to own it. It is committed in the SAME SQLite transaction as the Task
-- claim/start transitions, the Execution row, and their journal facts (see
-- `entry_store.rs`), so a concurrent duplicate Attempt identity refuses
-- with zero unrelated mutations instead of detonating later in the
-- background pipeline.
--
-- New relational state (current-state authority for entry conflict reads):
--
--   attempt_reservations — one row per admitted-but-not-yet-created
--                           Attempt: its identity (PRIMARY KEY: global
--                           Attempt uniqueness across executions), the
--                           owning Execution, the owning Task, and the
--                           reservation timestamp. Foreign keys are
--                           restrictive (default NO ACTION): executions and
--                           tasks are never deleted by design, so no
--                           cascade semantics are invented.
--
-- Lifecycle: the entry transaction inserts the reservation; the later
-- `create_attempt` transaction consumes (deletes) it when the Attempt row
-- is created for the owning execution. A reservation for an Attempt that
-- already exists as a row, or owned by another execution, refuses the
-- write. Reservations are intent rows, not execution state: no journal
-- event type is added and no Attempt lifecycle state is invented (the
-- Attempt row itself stays the only Attempt fact).
--
-- No journal rebuild: existing event types and scope contracts are
-- unchanged, so the journal table is untouched.

CREATE TABLE attempt_reservations (
    attempt_id   TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL REFERENCES executions (id),
    task_id      TEXT NOT NULL REFERENCES tasks (id),
    reserved_at  TEXT NOT NULL CHECK (length(reserved_at) > 0)
);

CREATE INDEX attempt_reservations_execution ON attempt_reservations (execution_id);
