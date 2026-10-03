# SQLite Storage - Derived Navigation

Generated: 2026-09-30. Source baseline: `5362f6e`.
Derived navigation under [adapters](../AGENTS.md); canonical policy: [AGENT.md](../../AGENT.md).

## Source map

| Concern | Source |
| --- | --- |
| Connection admission and Task persistence | [task_store.rs](src/task_store.rs), [path_security.rs](src/path_security.rs) |
| Schema upgrades | [migration.rs](src/migration.rs), latest [0022_candidate_input_assessments.sql](migrations/0022_candidate_input_assessments.sql) |
| Journal envelope and reads | [event.rs](src/event.rs), [journal_store.rs](src/journal_store.rs) |
| Execution / Attempt restoration and commits | [runtime_store.rs](src/runtime_store.rs) |
| Workspace leases and trusted anchor | [workspace_store.rs](src/workspace_store.rs), [WorkspaceStore contract](../../crates/workspace/src/store.rs) |
| Plan revisions and planning control | [plan_store.rs](src/plan_store.rs), [planning_control_store.rs](src/planning_control_store.rs) |
| Cost, guardrails and human decisions | [plan_cost_store.rs](src/plan_cost_store.rs), [plan_guardrail_store.rs](src/plan_guardrail_store.rs), [human_decision_store.rs](src/human_decision_store.rs) |
| Captured effects and candidate assessment | [observed_effect_store.rs](src/observed_effect_store.rs), [candidate_assessment_store.rs](src/candidate_assessment_store.rs) |
| Approval, audit and secret metadata | [approval_store.rs](src/approval_store.rs), [audit_store.rs](src/audit_store.rs), [secret_store.rs](src/secret_store.rs) |

## Persistence details

- [Schema version](src/migration.rs) is **22**, tracked by `PRAGMA user_version`.
  Numbered historical migrations are immutable; the migrator applies pending files in order.
  It rejects newer schemas and rechecks the version inside an immediate transaction.
- [SqliteStore::open](src/task_store.rs) verifies foreign keys and sets a 30-second busy timeout.
  Write transactions use `TransactionBehavior::Immediate` to serialize at BEGIN.
- [apply_lifecycle_transition](src/task_store.rs) guards the expected state and commits
  state plus journal together; stale state or append failure rolls back both.
  [Runtime commits](src/runtime_store.rs) likewise couple authoritative mutations to events.
- Reads reconstruct through [Task::restore callers](src/task_store.rs) and
  [Execution::restore / Attempt::restore callers](src/runtime_store.rs); invalid persisted data is an error.
- [Path admission](src/path_security.rs) rejects symlink database/parent paths;
  Unix checks require private regular database/sidecar files, matching owner and one hard link.
  [Opening](src/task_store.rs) uses `SQLITE_OPEN_NOFOLLOW` and rechecks file identity.
- [Secret storage](src/secret_store.rs) contains opaque refs and delivery bindings only;
  values belong to [secrets::SecretBackend](../../crates/secrets/src/lib.rs).

## Existing verification targets

Run from the repository root; package identity is [storage-sqlite](Cargo.toml).

| Scenario | Invocation / source |
| --- | --- |
| Fresh schema, 21-to-22 upgrade, reopen, corrupt rows | `rtk cargo test -p storage-sqlite --test durability` - [tests](tests/durability.rs) |
| State/event atomicity and stale writes | `rtk cargo test -p storage-sqlite --test journal` - [tests](tests/journal.rs) |
| Runtime restoration and atomic commits | `rtk cargo test -p storage-sqlite --test runtime` - [tests](tests/runtime.rs) |
| Database and sidecar admission | `rtk cargo test -p storage-sqlite --test path_security` - [tests](tests/path_security.rs) |
| Metadata-only secret persistence | `rtk cargo test -p storage-sqlite --test secrets` - [tests](tests/secrets.rs) |
| Plan persistence and receipts | `rtk cargo test -p storage-sqlite --test plan_persistence --test plan_cost_receipts` - [plan](tests/plan_persistence.rs), [cost](tests/plan_cost_receipts.rs) |
