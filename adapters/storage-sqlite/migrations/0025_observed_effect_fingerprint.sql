-- 0025_observed_effect_fingerprint: observed_effects.subject_fingerprint
-- binds the subject only, so a type-valid edit to payload_ref, captured_at
-- or provenance survived restore (audit F16). record_fingerprint is a
-- SHA-256 over every persisted field (evaluation::ObservedEffect::
-- record_fingerprint). SQLite has no SHA-256: the Rust migration step
-- backfills existing rows right after this file; NULL is Corrupt on read.

ALTER TABLE observed_effects ADD COLUMN record_fingerprint TEXT NULL;
