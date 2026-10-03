# Journal Hash Chain (F10) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Append-only journal becomes tamper-evident via hash chain.

**Architecture:** Migration 0024 adds `previous_hash`/`record_hash` columns plus backfill; insert path chains hashes in-transaction; read paths verify full scan before decode. Mirrors `audit_store.rs:13,65,104,307,368` precedent. `JournalEvent` unchanged (storage-only hashes, no constructor churn).

**Tech Stack:** Rust, rusqlite, sha2 (already dep), SQLite triggers.

**Spec:** `docs/development/SECURITY_ARCHITECTURE_AUDIT_2026-10-03.md` P0 F10 follow-up (triggers done in 0023; this adds chain).

## Global Constraints

- TDD: no production code without failing test first; watch each test fail.
- `cargo fmt --all --check` clean before each commit.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` clean at end.
- Break = `StorageError::Corrupt` (`adapters/storage-sqlite/src/error.rs:24,86`), never silent.
- Downgrade-fixture tests DROP new triggers before planting rows (0023 precedent).

## Review Focus

- Pre-0024 DB upgraded with NULL hashes: backfill must chain all rows, else first read fails closed on legacy data.
- Filtered reads (`list_events_for_task`, `list_events_for_project`): verifier must full-scan `ORDER BY seq`, not filtered subset, else gaps false-positive.
- Concurrent writers: previous-hash read and INSERT must share one transaction, else forked chains.
- Downgrade fixtures (`durability.rs`, `plan_cost_receipts.rs`, `plan_persistence.rs`): must DROP new 0024 triggers, else re-migration fails.
- Empty journal: verifier returns Ok on zero rows (genesis, no break).

---

### Task 1: Migration 0024 columns, backfill, triggers

**Files:**
- Create: `adapters/storage-sqlite/migrations/0024_journal_hash_chain.sql`
- Modify: `adapters/storage-sqlite/src/migration.rs:27,59-212`
- Test: `adapters/storage-sqlite/tests/journal.rs:811` (append-only suite)

**Interfaces:**
- Consumes: 0023 triggers `event_journal_no_update/no_delete`, `event_plan_causes_no_update/no_delete`.
- Produces: `SUPPORTED_SCHEMA_VERSION = 24`; `verify_journal_chain()` used by Task 3.

- [ ] **Step 1: Write failing test** in `adapters/storage-sqlite/tests/journal.rs`: open fresh DB, assert `PRAGMA table_info(event_journal)` contains `previous_hash` and `record_hash`; insert two events via public store API, assert second row `previous_hash` equals first row `record_hash`, first row `previous_hash` equals genesis constant.
- [ ] **Step 1b: Write failing legacy-upgrade test**: build DB at schema 23 (pre-chain rows without hashes), run migration, assert `verify_journal_chain()` passes and all rows carry non-NULL hashes (backfill covers legacy data, else first read fails closed on old DBs).
- [ ] **Step 2: Run tests, verify they fail** Run: `cargo test --locked -p storage-sqlite --test journal hash_chain` Expected: FAIL (no such columns).
- [ ] **Step 3: Implement migration** in `migration.rs`: bump to 24; migration runs DROP 0023 triggers, `ALTER TABLE event_journal ADD COLUMN previous_hash TEXT`, `ADD COLUMN record_hash TEXT`, Rust backfill (read rows `ORDER BY seq`, chain `sha256(seq || previous_hash || labeled column values)` mirroring `audit_store.rs:307`, genesis for seq 1, UPDATE rows), re-create 0023 triggers plus new `event_journal_no_hash_update` trigger aborting `UPDATE OF previous_hash, record_hash`. Pure-SQL file 0024 holds trigger/DDL text; backfill is Rust between two `execute_batch` calls (batches are SQL-only).
- [ ] **Step 4: Run test, verify it passes** Run: same command. Expected: PASS.
- [ ] **Step 5: Update downgrade fixtures** (`durability.rs`, `plan_cost_receipts.rs`, `plan_persistence.rs`): add `DROP TRIGGER IF EXISTS` for new trigger names wherever 0023 triggers are dropped.
- [ ] **Step 6: Commit** `git add adapters/storage-sqlite/migrations/0024_journal_hash_chain.sql adapters/storage-sqlite/src/migration.rs adapters/storage-sqlite/tests/`

### Task 2: Chain hashes on insert

**Files:**
- Modify: `adapters/storage-sqlite/src/journal_store.rs:949` (`insert_journal_event`)
- Test: `adapters/storage-sqlite/tests/journal.rs`

**Interfaces:**
- Consumes: Task 1 columns + genesis constant.
- Produces: every new row carries valid chain link.

- [ ] **Step 1: Write failing test**: insert event, read raw `previous_hash`/`record_hash` via SQL, assert `record_hash` equals recomputed `sha256(seq || previous_hash || labeled fields)` and `previous_hash` equals prior max `record_hash` (genesis when empty).
- [ ] **Step 2: Run test, verify it fails** Run: `cargo test --locked -p storage-sqlite --test journal insert_chains_hash` Expected: FAIL (hashes NULL/empty).
- [ ] **Step 3: Implement `insert_journal_event`** in `journal_store.rs:949`: inside existing transaction, `SELECT record_hash ... ORDER BY seq DESC LIMIT 1` (genesis if none), compute hash with labeled `hash_field` helper (copy `audit_store.rs:368` pattern into `journal_store.rs`, journal labels), INSERT with both columns.
- [ ] **Step 4: Run test, verify it passes** Run: same command. Expected: PASS.
- [ ] **Step 5: Commit.**

### Task 3: Verify chain on read

**Files:**
- Modify: `adapters/storage-sqlite/src/journal_store.rs:40,48,57` (three list fns), `adapters/storage-sqlite/src/task_store.rs:547` (passthrough, no logic change)
- Test: `adapters/storage-sqlite/tests/journal.rs:864` (tamper harness area)

**Interfaces:**
- Consumes: Tasks 1-2 hashes.
- Produces: any break surfaces as `StorageError::Corrupt` on read.

- [ ] **Step 1: Write failing test**: insert two events, assert `list_events_ordered()` on the empty prefix (fresh DB, zero rows) returns Ok empty (genesis, no break); then `DROP TRIGGER` guards, `UPDATE event_journal SET record_hash='forged' WHERE seq=1`, assert `list_events_ordered()` returns `Err(StorageError::Corrupt(_))` and filtered `list_events_for_task()` returns `Err(StorageError::Corrupt(_))` too (verifier full-scans, not filtered subset).
- [ ] **Step 2: Run test, verify it fails** Run: `cargo test --locked -p storage-sqlite --test journal tampered_chain_fails_closed` Expected: FAIL (returns Ok).
- [ ] **Step 3: Implement `verify_journal_chain()`** in `journal_store.rs`: `SELECT seq, previous_hash, record_hash ... ORDER BY seq`, recompute each link, mismatch/NULL/gap returns `Corrupt`; call it first inside `list_events_ordered`, `list_events_for_task`, `list_events_for_project` (full scan even for filtered lists). O(n) per read, same class as audit list path. Verify on read, not on open (`migrate()` stays cheap).
- [ ] **Step 4: Run test, verify it passes** Run: same command. Expected: PASS.
- [ ] **Step 5: Run full gates** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked --no-fail-fast` (background, long). Expected: all green.
- [ ] **Step 6: Commit.**
