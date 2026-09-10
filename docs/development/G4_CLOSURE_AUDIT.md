# G4 Closure Audit (TASK-0045)

> Independent closure audit for **G4 — Workspace/Git**. Authority:
> ADR-0008, ADR-0023, `docs/architecture/workspace.md`, `INVARIANTS.md`.
> Prior task reports (`G4_FAILURE_MATRIX.md`, TASK-0040…0044 docs) are
> evidence, not authority. This audit re-traced production source first,
> re-ran all verification, challenged the TASK-0044 matrix per failure
> class, and ran independent throwaway probes (removed after execution —
> no probe files remain in the tree).
>
> Verdict: **`G4_CLOSED`** (rationale in §11).

## 1. Scope audited

G4 surface: isolated worktrees/branches, base-revision handling, diff
inspection, cleanup lifecycle, recovery/reconciliation, concurrency,
drift/staleness, Git failure semantics, workspace journal facts, primary
checkout preservation. Explicitly out of scope (verified absent, not
tested for behavior): merge/rebase/cherry-pick, remote Git
(`fetch`/`pull`/`push`/`clone`/`ls-remote` appear nowhere as executed
commands), providers/G5, Policy Engine/sandbox/G6.

## 2. Production paths inspected (all read end to end)

- `crates/workspace/src/`: `lib.rs`, `ids.rs` (WorkspaceId /
  RepositoryRef / Revision / BranchRef), `lease.rs` (lifecycle +
  snapshot/restore), `failure.rs` (16-variant taxonomy), `status.rs`
  (live status), `port.rs` (`GitPort`), `store.rs` (`WorkspaceStore`,
  `WorkspaceEventKind`).
- `adapters/git-cli/src/`: `lib.rs`, `runner.rs` (argv-only spawn,
  bounds), `observe.rs` (inspect/revision/default-branch/status),
  `create.rs` (verified creation), `diff.rs` (read-only diff),
  `cleanup.rs` (non-forced removal).
- `crates/app-services/src/workspace_service.rs` (full saga: allocate →
  materialize → finalize, reconcile, cleanup, TOCTOU re-checks).
- `adapters/storage-sqlite/src/workspace_store.rs` (lease rows, guarded
  transitions, atomic state+event commits), `migration.rs` (schema v5),
  `event.rs` + `task_store.rs::require_envelope` (scope matrix).

## 3. Architecture boundaries (mechanical evidence)

`cargo tree` (depth-unbounded for the four crates, relevant excerpt):

- `workspace` → `task-domain`, `agent-runtime` only. No process,
  filesystem, SQLite, Tokio, provider surface. ✓
- `git-cli` → `workspace` only. Adapter depends on the contract, never
  the reverse; stores nothing durable. ✓
- `storage-sqlite` → `workspace` + `rusqlite` (+ task/agent-runtime).
  Implements `WorkspaceStore`; knows SQL rows, nothing about Git
  commands. ✓
- `app-services` → `workspace`, `storage-sqlite`, task/agent-runtime,
  scheduler, evaluation. Owns orchestration/durability coordination;
  single `Command::new` in the whole G4 surface lives in
  `git-cli/runner.rs` (fixed executable + argv, `current_dir` for cwd,
  closed stdin, `GIT_TERMINAL_PROMPT=0`). No shell string exists in
  production source (`sh -c`/`bash -c`/`cmd /C` match only a doc
  comment stating they are never used). ✓
- No domain crate instantiates infrastructure (INV-025/026/027 hold);
  no cross-context table reads (lease decode validates through domain
  constructors; Git truth flows through `GitPort`). ✓
- `bins/` contains no workspace wiring (only incidental helper-name
  matches in agentctl test harnesses) — no production caller can
  currently pass a wrong repository path (see §8, finding F1).

## 4. Safety properties (traced, not just test-read)

- **Isolation**: one lease per Execution (`UNIQUE(execution_id)` +
  service guard); concurrent leases get distinct ids/paths/branches;
  same-file edits coexist (pinned by
  `concurrent_workspaces_isolate_same_file_edits`).
- **Primary preservation**: creation captures HEAD OID/ref/status
  baseline and re-verifies it post-mutation; any move →
  `CheckoutFailed`. Failures never clean/reset the primary.
- **Branch preservation**: removal is `branch -d` only; unmerged
  branches survive (`CleanupUnsafe`, lease stays `CleanupPending`, no
  event); protected/default branch never deleted (metadata-resolved,
  never `main`/`master` literals — none exist in the adapter).
- **Dirty preservation**: `dirty != safe to delete` at three levels —
  adapter pre-check (`WorktreeDirty`), service entry check (dirty
  `Ready` stays `Ready`, nothing written), service TOCTOU re-check
  before destruction (late dirt → stays `CleanupPending`). Git's own
  non-forced refusal is the final backstop. Forbidden primitives
  (`reset --hard`, `clean -fd`, `worktree remove --force`, `branch -D`,
  `rm -rf`, stash) appear in production source only in comments
  describing their absence.
- **Path safety**: service mints `<root>/ws-<slug>` (no caller path
  API); slugs are single components (`/`, `..`, spaces encoded);
  persisted refs re-validated against the derived path on every
  destructive call (`validate_managed_path` + canonical containment);
  foreign refs → `OwnershipMismatch`, nothing touched.
- **Command safety**: fixed executable + argv arrays; `--` separates
  paths; dash-led branch short names refused; hostile ExecutionIds
  encode to safe slugs (pinned by test).
- **Remote safety**: no remote subcommands exist in the adapter.
- **Default branch**: indeterminate metadata → `GitBackendFailure`
  (fail closed, names no branch); creation/cleanup proceed only for
  `agent/`-scoped branches, which cannot be the default by derivation.

## 5. Lifecycle / exactly-once (INV-145 saga)

`allocate` (intent row, read-only Git) → `materialize` (`&self`, no
store access across the subprocess) → `finalize_ready` (observe, then
atomic `Allocated → Ready` + `workspace.created`). Crash-A (intent
only) resumes; crash-B (Git done, no finalization) recognizes without
new mutation; crash-C (ambiguous partial) refuses blind replay —
all three pinned, plus guarded `WHERE lifecycle = …` transitions so a
concurrent mover fails instead of overwriting. `workspace.created`
exactly once (guarded transition + `Ready`-without-event treated as
corruption); `workspace.removed` exactly once (guarded
`CleanupPending → Removed` + atomic commit). Replayed cleanup of a
healthy `Removed` lease is a no-op (`AlreadyRemoved`) with corruption
detection (live worktree/branch under `Removed` → `CheckoutFailed`,
never resurrection, never silent deletion).

## 6. TOCTOU, drift, diff, events, persistence

- **TOCTOU**: destructive preconditions re-checked immediately before
  mutation at both service and adapter layers; the deterministic
  fault-injection test proves a late user edit is preserved
  (`CleanupPending`, bytes intact, no event). Branch state changes are
  covered by non-forced Git semantics (refusal, not destruction).
- **Drift/staleness**: wall-clock age authorizes nothing (far-future
  reopen deletes nothing, emits nothing). Missing/moved worktrees,
  changed tips, foreign refs classify as bounded typed failures;
  unknown state never becomes safe state.
- **Diff**: read-only (`rev-parse`/`diff --name-status`/`status`
  only), baselined at the recorded OID (service rejects baseline
  drift), bounded entries + explicit truncation, non-mutation proven
  (index/tree/refs/lease/Task unchanged). `diff != verification !=
  COMPLETED`.
- **Events**: closed two-name taxonomy; write-path scope matrix
  (`require_workspace_scope`: project+task+execution+workspace, no
  attempt, no Task states, fixed transition pair); unknown names are
  corruption on read; workspace facts never touch Task lifecycle or
  Attempt outcomes (`workspace_lifecycle_never_completes_tasks`).
- **Persistence**: schema v5; historical migrations valid (v1→v5,
  v2→v5, v3+G2-data, v4+G2/G3-data, fresh→v5 all green); lease table
  carries no dirtiness columns (live Git owns that); Git facts never
  duplicated as mutable authority.

## 7. TASK-0044 matrix challenge (per-class verdict)

Claims verified against the tree: 10 adapter + 22 service matrix tests
= 32 new tests ✓; full suite counts 521 passed / 0 failed —
independently reproduced exactly (see §10) ✓. All 32 run against real
temporary Git repositories (offline fixtures); the 2 fault-injection
cases (`DirtAfterFirstCheck`, `LiveOwner`) delegate every
non-injected op to the real `GitCli`, and the TOCTOU case plants a
real untracked file so live state is genuinely dirty from the re-check
on — the seams inject timing/ownership facts, never Git semantics. No
test weakens production code (TASK-0044 changed no `src/**` — confirmed
via `git status`: all G4 `src/` files predate this audit as
untracked/working-tree content from TASK-0040…0043). Assertions check
contract behavior (lifecycles, event counts, byte preservation,
ref targets), not implementation prose. No order dependence (unique
temp dirs per case, `Drop` reaps fixtures), no wall-clock dependence
(`FixedClock`; the staleness test jumps a decade and asserts
no-action), no global Git config mutation (fixture-local `-c`
identity), no skipped cases.

## 8. Non-blocking note re-verdict (independent probes, since deleted)

Scenario: externally deleted managed branch + clean at-base worktree +
cleanup. Reprobed from first principles: finalizes `RemovedFresh`;
recorded base still `cat-file`-reachable as a commit; primary HEAD OID
and symbolic ref byte-identical before/after; worktree gone; every
mutation non-forced and observed. The unique-commit twin fails closed
(work + lease preserved, no `Removed` event, never `AlreadyAbsent`).
Additional probe detail: with a dangling HEAD, `git status` surfaces
the tree as dirty, so the deleted-branch + unique-work shape refuses at
the *entry* check (`WorktreeDirty`) — a second, independent backstop
behind Git's non-forced refusal. **Concurrence with TASK-0044**: safe
finalization, not a contract defect. Classification stands.

## 9. Findings

**F1 — cleanup `repository_path` is caller-supplied without a positive
repo binding (low, non-blocking).** Throwaway probe: `cleanup_workspace`
with a *wrong but valid* repository path finalized `RemovedFresh` —
the real worktree was correctly removed (primary discovery is
worktree-derived), but branch deletion/finalization gates ran against
the wrong repo, so the true managed branch leaked while the lease went
terminal. Zero data loss (branch + commits survive; retry with the
correct path fails loudly as `CheckoutFailed` corruption, never silent
repair). Unreachable from any in-repo production caller (no
orchestration/agentctl wiring exists in G4), requires trusted-caller
error, and damage is confined to `agent/`-scoped refs. Remediation
(G5 wiring, not G4): bind cleanup to lease-derived repository
resolution or verify worktree registration in the passed repo before
branch ops. No new test committed (pinning current behavior would
enshrine the gap; the fix belongs with the first production caller).

**F2 — `BranchRef` admits Git-special characters (informational).**
Constructor rejects only `..`/spaces; e.g. `~` would alter `rev-parse`
semantics at `verify_git_consistent`. Unreachable: leases are minted
only via slug-safe derivation, and hand-planted rows are
trusted-store territory. Suggested hardening alongside F1.

**G5 integration note (not a defect).** Re-requesting a `Ready` lease
with `HEAD` after the primary advanced yields `OwnershipMismatch`
(bases are immutable per ADR-0023). Correct per contract; G5
orchestration must reuse the lease's recorded base rather than
re-resolving `HEAD`.

No production defects. No architecture violations. No blocking
test-integrity gaps.

## 10. Commands executed

- `cargo fmt --all -- --check` — clean.
- `cargo check --workspace` — clean.
- `cargo test --workspace --no-fail-fast` — **521 passed, 0 failed**
  (independently tallied per binary; matches TASK-0044's census).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  — clean.
- G4 binaries re-run a second time (git-cli 47 + app-services 52 +
  storage-sqlite workspace 13, plus unit 23) — green, deterministic,
  no flakes; full-suite second pass not needed beyond the G4 surface
  (first full pass already green).
- `cargo tree -p workspace/git-cli/storage-sqlite` — boundaries §3.
- `rg` sweeps for shell/force/remote/merge primitives — §3/§4.
- Three throwaway probes (wrong-repo cleanup; deleted-branch
  clean-at-base; deleted-branch unique-commit) — executed, verified,
  files deleted; `git status` confirms no probe debris.

Test census (G4-owned): workspace unit 15, git-cli unit 8, adapter
integration 47 (11 creation + 10 matrix + 9 safety + 17 diff/cleanup),
service integration 52 (10 saga + 20 diff/cleanup + 22 matrix),
storage workspace 13 → **135 G4 tests**, all green. Remainder of the
521 is G0–G3 regression (all green, untouched by G4).

## 11. Verdict

**`G4_CLOSED`.** All G4 acceptance criteria are independently
supported by production source; architecture boundaries hold
mechanically; failure semantics (creation/cleanup/reconciliation,
TOCTOU, drift, diff, events, persistence) are sufficient; the two
residuals (F1, F2) are latent hardening notes with no reachable
production path and explicit G5 remediation scope — neither prevents
honest gate closure. Full verification green; planning/version metadata
advanced consistently (version.toml → `0.1.0-dev.task0045`, queue
completed through TASK-0045, roadmap G4 completed / G5 in progress,
TASK-0046 queued as the first G5 task).

Known limitations (by design, unchanged): submodules/LFS deferred;
bare repos explicit-fail; no remote ops; no merge queue; Git isolation
is not an OS sandbox (G6); `owner_is_live` contractual-honest
(`false`) until Runner process authority arrives.
