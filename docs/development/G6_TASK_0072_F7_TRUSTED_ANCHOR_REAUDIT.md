# TASK-0072 — Independent Re-Audit of the F7 Trusted-Anchor Follow-Up

> Audit only. No repairs, no implementation, no code changes retained.
> Scope: does the uncommitted trusted-anchor follow-up (evidence §11 of
> `G6_TASK_0071_TRUSTED_HOST_GIT_BOUNDARY_HARDENING_EVIDENCE.md`) close
> re-review BLOCKER F7 without weakening prior guarantees?
> F-002, G6 closure, commit/merge decisions are explicitly out of scope.

## 0. Verdict

**F7_CLOSED at the audited state. G6 remains `IN_PROGRESS`.**

The forged linked-worktree identity attack (F7) fails closed on every
Git-read entry point against the trusted admission anchor, the anchor
cannot be satisfied from provider-writable state, and all required suites
are GREEN with no skips. This verdict covers F7 only: it is not a G6
closure claim (F-002 remains `NEEDS_MORE_EVIDENCE` per TASK-0070), and it
does not authorize commit, merge, version promotion, or
`completed_through` advancement — those belong to a closure task after
this report is reviewed.

## 1. Audited state (exact)

| Field | Value |
| --- | --- |
| Worktree | `/Users/artemmac/dev/runlith-task0071` |
| Branch | `task-0071-g6-trusted-host-git-boundary-hardening` |
| Committed HEAD | `edd574914bab25236c49db9cec548a8a931825f6` |
| Uncommitted tracked diff (32 files) | sha256 `350eeb0919332de397e356cc8a147f526dd45d3aaaab4cba91219861701d79a5` (5647 lines, saved by auditor at `/tmp/runlith-f7-audit-base.diff`, not in repo) |
| Untracked `crates/workspace/src/trusted.rs` | sha256 `56e9dafc848ee78d6bbad1c0576435714b1d75f94b0a0fefc304bf823e8a559a` |
| Untracked `adapters/storage-sqlite/migrations/0011_workspace_trusted_anchor.sql` | sha256 `f7d481dc79b3a5dea187dd76f64eebe394b567237faabe0d4e6af8388feb7a1c` |
| Host | macOS arm64 |
| Audit residue in repo | this report only; two temporary probe tests were executed and deleted |

Any further edit to the worktree invalidates this verdict; re-hash before
any completion commit.

## 2. What was audited (design under test)

- `workspace::TrustedCommonGit`: opaque canonical-commondir identity,
  byte-equality, capture/rehydration validation (absolute, NUL-free,
  canonicalize-compatible, UTF-8 for storage).
- `GitPort`: `establish_trusted_anchor` is the sole anchor-free method;
  every Git-invoking method takes `expected: &TrustedCommonGit` (no
  `Option`). Threading is compiler-enforced — a missing anchor is a build
  error, not a runtime risk.
- `runner::validate_repository_boundary`: existing metadata + config
  checks, then byte-comparison of the observed `.git → gitdir → commondir`
  chain against the anchor; mismatch → `Err(GitBackendFailure)`, never
  `Ok(Invalid)`. Per-function re-check, no caching.
- Sidecar `workspace_trusted_anchor` (schema v11, immutable trigger,
  `ON DELETE CASCADE`, no journal events, no backfill).
- Service: `allocate_workspace` establishes → anchored
  `is_repository`/`resolve_revision` → single `IMMEDIATE` txn
  (`insert_allocated_lease_with_anchor`); every later operation
  (`finalize`, `verify_git_consistent`, `materialize`, `cleanup`,
  `validate_*`, `prove_branch_only_recovery`, `side_effects_present`)
  loads the anchor first and fails closed when absent.
- `bins/agentd` entry preflight: establish-then-bind before any provider
  execution.

## 3. Independent verification (re-ran, not trusted from §11)

```text
$ cargo fmt --check
exit 0

$ cargo clippy -p git-cli --all-targets -- -D warnings
exit 0

$ cargo test -p git-cli
8 + 18 + 10 + 2 + 25 + 11 = 74 passed, 0 failed (no --skip)

$ cargo test -p git-cli --test safety_regression fully_forged
fully_forged_linked_worktree_identity_in_provider_writable_sibling_is_refused ... ok

$ cargo test -p app-services --test g6_security_integration
7 passed, 0 failed

$ cargo test -p app-services -p storage-sqlite
all suites ok, 0 failed (28 result lines, none failed)
```

Suite list deliberately narrower than a full workspace run: the change
touches `workspace`, `git-cli`, `storage-sqlite`, `app-services`,
`bins/agentd` (preflight). Full-workspace regression belongs to the
closure task.

## 4. Independent adversarial probes (auditor-authored, then deleted)

The permanent F7 keeper exercises only `resolve_revision`. Two temporary
probes covered the gap:

1. `audit_probe_forged_shape_refused_on_all_read_entry_points` — rebuilt
   the fully-forged shape (anchor captured pre-forge) and demanded
   `Err(GitBackendFailure)` from `is_repository`, `worktree_status`,
   `default_branch`, and `resolve_revision`. **Passed.**
2. `audit_probe_anchor_of_a_refuses_pristine_b` — anchor of repo A
   against pristine repo B (`is_repository`, `resolve_revision`,
   `worktree_status`). **Passed.**

Both files were removed after the run; `git status` confirms no residue.

## 5. Attack-surface review (read, not just executed)

| Surface | Finding |
| --- | --- |
| Forged-identity acceptance (F7 core) | CLOSED. Deciding control is the anchor comparison, not structural self-consistency: the forged shape passes the back-reference/ownership checks and is refused only by byte-mismatch against the admission anchor (verified by probe 1 and by reading `validate_trusted_anchor`). |
| Anchor capture trust | SOUND within threat model. Capture runs pre-provider from the operator-supplied admission path (`allocate_workspace`, entry preflight); a provider cannot influence its own admission. Post-capture forgery cannot byte-match the anchor without writing the real common dir. |
| Sidecar tamper by confined provider | DENIED at OS boundary. Seatbelt profile is deny-default; provider write roots are the managed worktree plus H7 support/scratch roots. The DB (`state.sqlite3` under the app data dir) is in none of them. Host compromise remains out of scope (documented residual). |
| Missing-anchor / legacy leases | FAIL-CLOSED. `load_anchor` errors; `side_effects_present` returns `true`; no backfill path exists. |
| Snapshot/journal/contract leakage | ABSENT. `WorkspaceLeaseSnapshot` carries no anchor or absolute path; no `TrustedCommonGit` refs in `lease.rs`, `status.rs`, or `contracts/`. |
| Policy authority in adapter | ABSENT. No `policy::` refs in `git-cli` or `workspace` crates; enforcement stays with operation owners per ADR-0026. |
| `run_git` environment | HARDENED. `env_clear`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, forced `hooksPath=/dev/null`, `fsmonitor=false`, `sshCommand=false`, empty credential helper / external diff, `TERMINAL_PROMPT=0`. |
| No-`.git`-found skip in boundary check | SOUND. When the ancestor walk finds no `.git`, Git discovery agrees (env overrides cleared); `classify_repository` then maps to typed `NotRepository`/`BareRepositoryUnsupported`. Divergence where Git sees more (fs-boundary ascent) fails closed as non-repo. |
| `core.worktree` Invalid→Err tightening | Fail-closed direction; keeper updated explicitly, not weakened. |
| TOCTOU validation→exec | Accepted bounded residual (per-entry re-check, no cache). Eliminating it needs Git-process sandboxing or an atomic open-and-verify primitive — out of scope, correctly documented. |

## 6. Non-blocking observations (no verdict impact)

1. **Stale queue text.** The uncommitted `queue.yaml` TASK-0071 outcome
   still says the exact suite "remains intentionally RED" and "no
   completion commit is permitted". Ground truth is now 74 GREEN (§3).
   The closure task must rewrite that paragraph; the prohibition on
   merge/promotion still stands until then.
2. **F8/F9 candidate vs follow-up overlap.** The follow-up subsumes the
   earlier uncommitted F8/F9 candidate boundary (`validate_repository_boundary`
   + ancestor walk + config refusals). A closure diff-review should confirm
   no F8/F9 keeper was dropped in the subsumption — spot-checked present
   (`worktree_status_refuses_symlinked_and_redirected_git_metadata`,
   `core.gitProxy`/`sshCommand`/`hooksPath` keepers all GREEN).
3. **`agentctl --test execution_recovery` shim note** (§11) accepted as
   out of scope; flagged for the closure task, not this audit.

## 7. Traceability

- Re-review BLOCKER: evidence §0 table, F7 row.
- Required next step: evidence §0 "Required architectural next step" —
  implemented as §11; this report verifies §11.
- Prior verdicts unchanged: TASK-0070 `G6_REMEDIATION_REQUIRED` (F-003
  via Git path now addressed for F7; F-002 still open), G6 `IN_PROGRESS`.
