-- 0032_journal_hash_chain: tamper-evident Event Journal. Each row carries
-- the previous row's record hash and its own SHA-256 over every journal
-- column (see journal_store.rs, mirroring security_audit in 0010). SQLite
-- has no SHA-256, so the Rust migration step backfills legacy rows between
-- this file and 0032_journal_hash_chain_guard.sql. The 0023 UPDATE trigger
-- is dropped only for that backfill and recreated in the same transaction.

DROP TRIGGER event_journal_no_update;

ALTER TABLE event_journal ADD COLUMN previous_hash TEXT NULL;
ALTER TABLE event_journal ADD COLUMN record_hash TEXT NULL;
