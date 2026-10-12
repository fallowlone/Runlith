-- 0023_journal_append_only: the Event Journal and its plan-cause sidecar
-- are evidence, not mutable state. 0010 already guards security_audit with
-- BEFORE UPDATE/DELETE triggers; the journal had only decoder-side checks,
-- so a compromised writer with raw SQL access could rewrite history and
-- only be caught on next decode. Mirror the 0010 pattern here.
--
-- Tamper-simulation tests DROP these triggers before planting corrupt rows
-- (same pattern as security_audit tamper tests); production code never
-- UPDATEs or DELETEs these tables.

CREATE TRIGGER event_journal_no_update
BEFORE UPDATE ON event_journal
BEGIN
    SELECT RAISE(ABORT, 'event journal is append-only');
END;

CREATE TRIGGER event_journal_no_delete
BEFORE DELETE ON event_journal
BEGIN
    SELECT RAISE(ABORT, 'event journal is append-only');
END;

CREATE TRIGGER event_plan_causes_no_update
BEFORE UPDATE ON event_plan_causes
BEGIN
    SELECT RAISE(ABORT, 'event plan causes are append-only');
END;

CREATE TRIGGER event_plan_causes_no_delete
BEFORE DELETE ON event_plan_causes
BEGIN
    SELECT RAISE(ABORT, 'event plan causes are append-only');
END;
