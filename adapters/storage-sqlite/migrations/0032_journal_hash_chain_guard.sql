-- 0032_journal_hash_chain_guard: restore the 0031 append-only UPDATE guard
-- after the legacy backfill. Hash columns are covered by the same trigger.

CREATE TRIGGER event_journal_no_update
BEFORE UPDATE ON event_journal
BEGIN
    SELECT RAISE(ABORT, 'event journal is append-only');
END;
