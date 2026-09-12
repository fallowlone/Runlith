# G5 Legacy Workspace Identity Remediation (TASK-0058)

> Evidence/remediation record for G5 B-OL. Canonical architecture remains in
> `docs/architecture/workspace.md`, ADR-0023 and ADR-0025. This file records
> the defect, the bounded remediation, and the verification evidence.

## Purpose and root cause

TASK-0058 fixes B-OL: persisted pre-v2 `WorkspaceLease` rows could be
misclassified as current-v2 solely because their historical execution-derived
branch happened to begin with `agent/e2-` or `agent/h2-`. Those prefixes are
valid historical output when an old `ExecutionId` itself begins with `e2-` or
`h2-`; they are therefore not a format discriminator.

The historical derivations audited are:

- TASK-0051: readable `%XX` escaping with `%%` for `%`, plus the SHA-256
  digest fallback used by that generation.
- Earlier G4: readable `-XX` escaping plus the FNV-1a digest fallback used by
  that generation.

Prefix-only classification was invalid because the namespace was never
provably disjoint from historical user/execution identities. A historical
execution named `e2-legacy` can legitimately persist
`refs/heads/agent/e2-legacy`; the same applies to `h2-...`.

## Persisted classification semantics

Classification now uses the complete durable identity pair: persisted branch
and persisted worktree path, re-derived for the lease owner and workspace id.

Current-v2 requires all of the following:

1. the persisted branch equals the current-v2 branch derivation;
2. the persisted worktree path equals the current-v2 workspace-path
   derivation under the admitted managed root;
3. the durable managed-identity reservation exists for that owner and agrees
   byte-for-byte with the persisted branch.

Legacy classification requires the complete branch/path pair to match one of
the supported historical derivations above. Historical rows do not require a
managed-identity reservation because those reservations did not exist when
the rows were created.

Anything else fails closed as `OwnershipMismatch`. In particular:

- a damaged current-v2 lease cannot become legacy merely because its mutated
  branch/path happens to resemble an old format;
- a current-v2 branch paired with a historical path is rejected;
- a historical branch paired with a current-v2 path is rejected;
- branch/path pairs that match neither supported generation are rejected;
- a current-v2 pair with a missing or disagreeing durable reservation is
  rejected.

Persisted historical `WorkspaceLease` branch/path values remain authoritative
artifacts. Resume, reconciliation and cleanup operate on those byte-identical
values after proving the supported historical derivation and containment;
they are never renamed or silently upgraded to v2.

## Schema decision

TASK-0058 introduces no schema change and no format-version column. Existing
durable facts already contain enough information to classify the supported
generations from the complete branch/path pair, while the current-v2 managed
identity reservation provides the additional admission proof for v2 rows.
Adding another persistent discriminator would duplicate facts and require a
migration without improving the historical rows that predate such a field.

## Historical and migration evidence

Independent G5 evidence used a genuine TASK-0051 build/database rather than a
hand-rewritten approximation. The TASK-0051 daemon created schema-6 leases,
including `e2-`/`h2-` identities. Opening that database with the current tree
migrated schema 6 to schema 7 while preserving persisted `workspace_leases`
identity bytes; the new managed-identity reservation table was empty for the
historical rows as expected.

The remediation keepers prove both prefix-collision families:

- old `e2-*` ALLOCATED lease resumes from its persisted branch/path;
- old `h2-*` ALLOCATED lease resumes from its persisted branch/path;
- old `e2-*` READY lease cleans up using its persisted branch/path;
- old `h2-*` READY lease cleans up using its persisted branch/path.

The READY cleanup keeper packs refs before cleanup, so historical cleanup is
also covered with packed refs. Legacy cleanup re-validates managed-root
containment before mutation; a symlink escape is refused and the external
target is left untouched.

Corruption keepers cover current-v2 wrong-path, current-v2 wrong-branch,
durable-owner mismatch, impossible historical path, cross-workspace
historical path, current-v2-to-historical fallback, and mixed historical/v2
pairs. All fail closed before the unsafe continuation.

## Concurrency and exactly-once evidence

Historical resume was re-run through the TASK-0056 shared live-ownership
domain. Concurrent resumes elect one live continuation. Durable Attempt and
journal facts remain exactly-once: one canonical Attempt, one
`attempt.created`, one terminal conclusion, and one workspace-created fact
where materialization occurs.

Regression evidence retained from the remediation run:

- historical ALLOCATED compatibility: 25 `e2-*` + 25 `h2-*` rounds;
- historical READY cleanup: 50 rounds;
- corrupt-v2 fallback refusal: 50/50;
- TASK-0057 branch-only recovery: 14/14;
- B-CF/case-fold identity: 8/8;
- two full workspace runs: 722 passed / 0 failed / 1 ignored each;
- `cargo fmt --all -- --check`, `cargo check --workspace`, and workspace
  clippy with warnings denied: green;
- FakeAgent/provider contract and Claude provider contract suites: green;
- env-gated real provider-adapter smoke: green with Claude Code 2.1.268.

Finalization re-ran the minimum keeper set on 2026-09-12:

- `legacy_workspace_identity`: 6/6;
- TASK-0057 `branch_only_recovery`: 14/14;
- TASK-0056 `live_ownership`: 5/5;
- `casefold_identity`: 8/8;
- `cargo fmt --all -- --check`: green;
- `cargo check --workspace`: green;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  green.

A fresh full-workspace run then passed 722 / 0 / 1 across 75 suites in
90.72 s. The immediately preceding full-workspace attempt had one transient
failure in `concurrent_writer_pairs_serialize_without_busy_failures` while Git
reported an OS-level `commondir` read error during parallel worktree creation.
The exact failing test passed immediately in isolation, and the complete
`concurrent_cycles` suite then passed three consecutive repetitions. No
TASK-0058 product change was made for that transient test-environment failure.

The ignored full-workspace test is the subscription/environment-gated real
provider smoke (`provider-claude/tests/smoke.rs`); the production binary smoke
is executed manually and recorded below.

## Cross-remediation regression status

- TASK-0056 B-CR live ownership: green; one live owner and exactly-once
  durable Attempt/journal facts are preserved.
- TASK-0057 B-RS2 branch-only recovery: green; current-v2-only recovery uses
  this classifier and historical leases do not enter the v2 branch-only path.
- B-CF: green; v2 case-fold-safe identity and collision reservation semantics
  remain intact.
- Packed refs: green for historical cleanup and the existing branch-only/
  case-fold keepers.
- Containment: green; historical persisted paths still require direct-child
  managed-root containment and live symlink/canonical containment proof.

## Known test-harness status

TEST-H remains the previously classified test-hermeticity hardening issue: a
few real-process tests can race daemon auto-start and stale daemons if a test
aborts before cleanup. It is not repaired by TASK-0058. Final acceptance
therefore performs explicit process/socket ownership accounting after the
manual production smoke and regression runs.

## Final production-path evidence

PASS on 2026-09-12 through the shipped debug binaries and the production
composition path:

```text
agentctl
→ production Unix IPC
→ agentd
→ production execution composition
→ WorkspaceService
→ provider-claude
→ real Claude Code 2.1.268
```

The run used a unique `mktemp` root with separate repository, runtime
directory, data parent, socket, Task `T-T58`, Execution `E-T58-PROD`, Attempt
`A-T58-PROD`, and managed workspace root. It did not use the user's normal
runtime/data directory.

Result:

- wall duration: 12 s;
- provider: `claude-code` 2.1.268;
- Attempt `A-T58-PROD`: `SUCCEEDED`;
- opaque evidence: one `claude:session:<uuid>` reference;
- Task `T-T58`: `VERIFYING`, never `COMPLETED` by provider output;
- execution status after conclusion: `live=false`;
- cancel after conclusion: `ALREADY_CONCLUDED`, `requested=false`;
- Attempt reservations for the execution: 0;
- managed identity reservation: one admission reservation agreeing with the
  v2 branch owned by `E-T58-PROD`;
- WorkspaceLease: `READY`, with the same v2 branch/path reported by status;
- journal: exactly one each of `workspace.created`, `attempt.created`, and
  `attempt.concluded`, followed by submit/review/verification-began facts;
- primary repository HEAD stayed at the smoke baseline and primary porcelain
  stayed empty;
- managed worktree HEAD stayed at the same base; the only untracked worktree
  artifact was the already-known `data/sessions.db` provider-environment H7
  residue;
- application data directory mode: 0700;
- canonical SQLite contained none of the checked provider-secret markers
  (`ANTHROPIC_API_KEY`, `sk-ant-`, `Authorization: Bearer`);
- daemon answered ping, then `agentctl shutdown` completed successfully;
- socket disappeared after shutdown;
- no TASK-0058 `agentd` or provider child remained afterward.

Two Claude Desktop/Claude Code processes (PIDs 62689/62690 at the time of the
check) pre-existed the smoke and remained afterward. They belonged to the
user's Claude Desktop session, used a different installed Claude Code build,
and were not modified or terminated by TASK-0058.

## Final repository/process cleanliness

Finalization inspection found no TASK-0058-owned daemon/provider process after
shutdown. The smoke managed worktree was removed through Git, its managed
branch was deleted, and the exact isolated smoke root (repository, runtime,
socket parent, data directory and database) was then removed. No
`runlith-task0058-*` temp roots remained. The primary repository reports only
its expected development worktree and no `refs/heads/agent/*` residue.

The primary repository is intentionally not globally clean. Its working tree
already contained the accepted-but-uncommitted TASK-0053 through TASK-0057
stack plus unrelated/unowned files before this finalization. The index is
empty. TASK-0058-specific accounting is therefore based on task markers and
scope, not a false claim that every dirty path belongs to TASK-0058.

TASK-0058-specific files/slices are:

- `crates/app-services/src/managed_identity.rs`: historical derivations and
  complete persisted identity classifier (the file also contains earlier
  TASK-0053 v2 work);
- `crates/app-services/src/workspace_service.rs`: shared classifier use for
  persisted lease lifecycle paths and current-v2 reservation proof (the file
  also contains earlier G4/G5 work);
- `crates/app-services/tests/legacy_workspace_identity.rs`: TASK-0058 keeper
  suite;
- `docs/architecture/workspace.md` and ADR-0025: bounded correction of the
  persisted-format/recovery description;
- `docs/development/G5_CASEFOLD_WORKSPACE_IDENTITY_REMEDIATION.md`: correction
  of the earlier prefix-disjointness claim;
- `planning/tasks/queue.yaml` and `product/version.toml`: TASK-0058 queue and
  development-version state;
- this remediation evidence record.

Generated/test-only smoke artifacts were all under the isolated `/tmp`
smoke root and were removed. Pre-existing unrelated dirt was left untouched,
including `Support/openvidia/proxy_config.json` and the primary checkout's
`data/sessions.db`, along with the broader accepted uncommitted G5 stack.

TASK-0058-scoped whitespace/diff verification is clean for the tracked task
slices and the task-owned untracked files listed above. Global
`git diff --check` is not green because of pre-existing trailing spaces in
`AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` and
`EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md`; those lines are outside TASK-0058
and were intentionally not edited. Final Git status remains dirty from the
pre-existing stack described above, the current branch remains
`task-0057-branch-only-recovery`, and the Git index has no staged changes.

## Gate status and next step

TASK-0058 remediates B-OL only. It does **not** close G5 and it does not start
G6. After TASK-0058 acceptance, the next step is TASK-0059: an independent
adversarial G5 closure re-audit covering B-CF, B-CR, B-RS2, B-OL and the
remaining G5 evidence. No G5-closed verdict is asserted by this document.
