# Audit log: worktree consolidation + G9/docs merge (2026-10-12)

Author: Muse Code (Muse Spark) audit session. Main: `328a381` → `76b0071`.
Worktrees 46 → 4, branches 52 → 4. Full suite green: 169/169 (final rerun pending at write time).

## 1. Initial state (verified, not assumed)

- Main checkout `/Users/artemmac/dev/runlith` on `docs/init-deep-agent-guidance @ 3da2c76`
  (ahead38/behind1 vs main `328a381`, fork at `4dcdcb5`). Dirty: 6 modified +
  untracked (AGENTS.md guidance, license 1-liners, external-research notes,
  license copies byte-identical to main).
- 46 worktrees enumerated via `git worktree list --porcelain`. Two large
  unmerged streams: G9 stack tip `task-0174-finder-index @ b7ca934`
  (ahead107/behind1) and docs-branch `3da2c76` (ahead38/behind1); neither side
  ancestor of the other. 28 G9 intermediates proven ancestors of tip via
  `git merge-base --is-ancestor` each; 11 worktrees proven reachable from main.
- Critical safety findings (all verified by direct inspection):
  - 44/46 worktree indexes carried staged license-revert text (BSL 1.1 in the
    G9 stack group, PolyForm Shield 1.0.0 in the cline group) contradicting
    main's Apache-2.0; staged `queue.yaml` also reverted the TASK-0153 title.
  - 12 worktree indexes staged live `nvapi-*` keys in
    `Support/openvidia/proxy_config.json` (staged-only, never in history per
    `git log --all -- <path>`).
  - Untracked `scripts/remove-cli-lic.sh` (executable license-stripper),
    personal `Modelfile`, stale `FETCH_HEAD`; missing `/private/tmp/task0147-corpus`.

## 2. Safety phase (before any merge)

- Safety tags: `audit-snap/{tip-g9,docs-branch,task0152-blocked,d05-design,main-before}`.
- `git restore --staged .` in all 44 dirty worktrees (unstage only; every file
  stays on disk; task0152's CHECKPOINT.json preserved as untracked).
- Quarantine to `/tmp/runlith-audit-quarantine/`: `remove-cli-lic.sh`, `Modelfile`,
  `FETCH_HEAD` (moved, not deleted). `git worktree prune` (46 → 45).
- Backported TASK-0154 ignore rules (`/.zcode/`, `/Support/`, `data/sessions.db`)
  to kept branches d05 (`ef57f96`) and task0152 (`fef8027`) as separate commits.
- Tip worktree dirt (10 tracked license/queue/doc files + 3 untracked BSL copies,
  all verified older than HEAD) discarded by explicit name list → tip clean.

## 3. Merge 1: relicense into G9 tip, tip into main

- `merge main → task-0174-finder-index`: 1 conflict (`docs/architecture/adr/INDEX.md`).
  Resolution: kept tip's 0037/0041/0042/0043 rows, dropped the 0038 row (file
  absent on both sides — dangling reference; main had deleted it). Commit `c3bfc12`.
- Verification on `c3bfc12`: `cargo fmt --check` ok; `cargo build` ok;
  `cargo clippy -- -D warnings` ok; full `cargo test --workspace` → 163/163 green
  (unsandboxed). Note: a sandboxed run showed 15 red suites, all
  `bind: Operation not permitted` / `sandbox-exec: sandbox_apply` — proven
  environmental (agent-sandbox artifact), zero code defects.
- `merge --ff-only task-0174-finder-index → main`: `328a381..c3bfc12`, 108 commits,
  zero conflicts (fast-forward).

## 4. Docs-branch integration (38 commits + uncommitted value)

- Committed worktree value on docs-branch first: license 1-liners
  (`secrets-vault`, `secret-backend-contract`), AGENTS.md guidance (root + 7),
  external-research notes (3 modified + 15 new). Untracked LICENSING.md/NOTICE
  (byte-identical to new main) removed as duplicates. → `8f9fc59`.
- `merge c3bfc12 → docs-branch`: 22 files / 47 hunks, all resolved as union.
  Notable resolutions:
  - Proto field collision: both sides took `= 10` → finder kept 10 (already on
    main), vault renumbered to 11 with in-file comment.
  - Migration number collision 0023–0025 (both sides): docs migrations moved to
    0031–0033 via `git mv` (main's 0023–0030 kept stable); SUPPORTED 22/25/30 → 33;
    reseal threshold 24 → 32; `MIGRATION_0024_GUARD` → `MIGRATION_0032_GUARD`;
    SQL header comments updated. No column/table overlap (verified).
  - `handshake_stream_v7` defined twice (vault vs finder profiles) → renamed to
    `handshake_stream_vault` / `handshake_stream_finder` + call sites.
  - agentd: kept docs-side bundled `ConnectionServices` signature; added main's
    `finder` field at all construction/call sites (unix + windows).
  - session/negotiation/app-services/agentctl: kept both vault and finder
    validators, enum variants, methods, dispatch arms, CLI subcommands.
- Merge commit `df1838d` + renumber-support commit `0d1c80b`.
- Verification found 3 real merge bugs (all mine, all in test fixtures):
  durability hardcoded `30`, journal legacy stamp `user_version = 23`,
  observed_effects teardown missing 0031–0033 revert. Fixed minimally in
  `86acb9f` (asserts → 33, stamp → 31, teardown extended, stale test names
  refreshed). No test weakened. Full suite on `86acb9f`: 169/169 green.
- `merge --ff-only docs-branch → main`: `c3bfc12..86acb9f`, zero conflicts.

## 5. Standalone / blocked preservation

- task0156: probes committed on its branch as `c7af8fa` (NOT_MERGED per author
  envelope); TASK-0156 queue entry transcribed from the author's own plan into
  main's queue (164 items, ids unique, strict YAML parse verified, references
  branch-qualified since probes live off-main).
- task0152 (blocked, CR-01): worktree+branch kept; consolidated 5 Linux probe
  files from `claude/adr-0037-7qvn2z@1aed0fd` onto the branch as `d7b8afa`
  (pure addition; the branch's newer macOS probe doc preserved, older Linux doc
  kept aside under a dated name). CHECKPOINT.json + drafts untouched on disk.
  NOTE: the task0152 worktree needs one owner command to materialize the 5 new
  files (see §8).
- d05 design prototypes (`apps/desktop/*`, 31 files): branch + worktree kept
  as-is (cleaned of license junk only), intentionally not merged (no TASK link).
- AGENTS.md workspace map refreshed: 30 → 40 packages (26 crates + 12 adapters
  + 2 bins), stamp updated with prior-commit note. Follow-ups merged as `4c2aa5a`.

## 6. Cleanup

- Removed 40 worktree directories (31 under repo root + 9 siblings), each only
  after asserting its dirt matched the known-junk name list (license revert +
  never-committed secrets); branches kept until proven merged.
- Removed temp worktrees (tip, main-merge, followups, fix0033); main checkout
  switched to `main`.
- Deleted 48 branches: 42 proven merged (`-d`, incl. `plan-g8-close` and
  `task-0127-g8-closure-audit` which the audit had flagged "unsquared"), plus 6
  unmerged-but-verified-obsolete (`-D` with `audit-snap/dropped-*` tags kept):
  merge-pr30-local (zero unique content), claude/adr-0037 (evidence consolidated),
  task-0125-old (claim-verification superset verified: same 5 tables, 88 vs 80
  lines, all key files present on main), task-0102 (design contradicts ADR-0037/
  ADR-0039), hardening/test-h (hermeticity already in main's tests), task-0057
  (all policy items present in main's evolved policy modules).
- Final: 4 branches (`main`, `design-d05-*`, `task-0152-*`, `task-0156-*`),
  4 worktrees. No shared history rewritten (all integrations are merges;
  the only ref updates are `--ff-only` and branch-local fix commits).

## 7. Independent review (separate agents) + fix

- 5-angle workflow review (merge-correctness, migration logic, IPC logic,
  wiring logic, test integrity) + critic + synthesis: found **1 ERROR + 4 WARN**.
  - ERROR (real, merge-interaction): 0033 backfill enumerated all rows but
    `load_row`'s quarantine filter hid invalidated effects → migration failed
    with `Corrupt("observed effect vanished")` on affected DBs.
  - 4 WARN: stale migration numbers in doc/SQL comments (0024→0032,
    0025→0033, 0023→0031 ×2). Docs-only.
- Fix on `audit/fix-0033-quarantine-backfill`, merged as `76b0071`:
  `load_row(..., include_quarantined)` (backfill=true, evidence reads=false),
  comments refreshed, regression test
  `upgrade_backfills_fingerprint_for_quarantined_effect` added (observed failing
  with the exact Corrupt error before the fix, green after). Focused test +
  full storage suite (21/21) + fmt + `clippy -D` green before merge.
- Follow-up single-agent merge-correctness review (all 23 conflicted files vs
  both parents + 4 follow-up commits): **VERDICT: CLEAN** — every file
  both-sides-preserved, follow-ups scoped, no remaining gaps.

## 8. Final verification on main

- `cargo fmt --all --check` → clean.
- `cargo build --locked --bin agentd --bin agentctl` → exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → exit 0.
- `cargo test --workspace --locked --no-fail-fast` on final main → PENDING
  (rerun in progress at write time; previous full green on `86acb9f`: 169/169,
  code delta to final is the backfill fix + comments only).
- One load-induced flake observed and cleared:
  `agentd::execution_recovery::concurrent_resumes_converge_on_exactly_one_attempt`
  failed once in a fully parallel run (64s stall, system under review-agent
  load), then green in isolation (1.72s) and as full suite (14/14, 6.11s);
  green in two earlier full runs. No code connection to the fix.

## 9. Remaining owner actions

1. Rotate the nvapi keys that sat in staged indexes (on-disk copies remain only
   in d05/task0152 worktrees, now gitignored).
2. In `/Users/artemmac/dev/runlith-task0152` run:
   `git checkout HEAD -- docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES_LINUX_20260925.md docs/development/task0152-probes/fake_anthropic_endpoint.py docs/development/task0152-probes/logging_connect_proxy.py docs/development/task0152-probes/macos_seatbelt_matrix.sh docs/development/task0152-probes/run_credential_input_probes.sh`
   (materializes the 5 consolidated files; sandbox blocked worktree writes).
3. CR-01 decision for task0152 (branch + worktree + CHECKPOINT preserved).
4. `/tmp/runlith-audit-quarantine/` (remove-cli-lic.sh, Modelfile, FETCH_HEAD)
   is ephemeral — relocate or consciously delete.
