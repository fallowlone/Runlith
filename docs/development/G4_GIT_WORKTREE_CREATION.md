# G4 Git Repository Inspection + Worktree Creation

> TASK-0041 — implementation evidence. Architecture authority lives in
> ADR-0008, ADR-0023, and `docs/architecture/workspace.md`; this document
> evidences the preflight, the adapter, and verification, and decides
> nothing itself.
>
> Explicit boundary: **Git worktree creation exists, but production
> orchestration integration does not**, because durable lease ownership and
> reconciliation are TASK-0042. `OrchestrationService` was not touched and
> must not invoke `GitPort` until TASK-0042 lands.

## Contract Authority

- ADR-0008 (isolated worktree/branch per autonomous execution).
- ADR-0023 §§1–10 (Execution-owned lease, identities, metadata-resolved
  default branch, Git-authoritative facts, dirtiness/cleanup/crash rules,
  `workspace.created/removed` taxonomy frozen but not implemented,
  CLI-adapter recommendation, INV-145 separate failure domains).
- `docs/architecture/workspace.md` (port boundary, failure taxonomy,
  deferred scope).
- `crates/workspace` `GitPort` + `WorkspaceFailure` + identities
  (TASK-0040 contract, unmodified by TASK-0041).

## Architecture Preflight

`crates/workspace::GitPort` was judged sufficient for all six required
capabilities; **no contract change was made** (`crates/workspace/**`
untouched):

| Requirement | Port surface |
|---|---|
| inspect repository | `is_repository` + `worktree_status` + `branch_exists` |
| resolve exact commit revision | `resolve_revision` → `Revision` OID |
| protected/default branch observation | `default_branch` (metadata only) |
| create managed branch + worktree | `create_worktree` (lease carries base + branch; explicit path) |
| verify resulting worktree identity | postcondition re-observation inside the adapter |
| classify expected failures | `WorkspaceFailure` taxonomy as-is |

No STOP condition triggered:

- **managed worktree root / path containment**: ADR-0023 §10 mentions
  "validated worktree roots" but defines no `ManagedRoot` type or
  containment rule. No policy was invented. The adapter enforces fail-closed
  target checks only (target must not exist, must not equal/nest inside the
  primary root — canonicalized comparison, macOS-symlink-safe — parent must
  exist). Durable path authorization arrives with TASK-0042 leases; until
  then the caller authorizes placement by provisioning the parent.
- **protected/default branch discovery**: resolved from exactly one metadata
  fact (`refs/remotes/origin/HEAD`); absent metadata → `GitBackendFailure`
  ("indeterminate; refusing to guess"), never a name heuristic.
- **branch ownership collision semantics**: TASK-0041 owns no durable store,
  so pre-existing ownership can never be proven — any pre-existing managed
  ref → `BranchCollision`; protected-branch misuse → `BranchCollision`
  naming the protection. No `-B`, no reset, no reuse.
- **partial creation result semantics**: mutation failure and postcondition
  contradiction → `CheckoutFailed` with an explicit partial-state note
  (branch/worktree re-observed); rollback is never claimed; reconciliation
  is TASK-0042 (INV-145).

## Git CLI Adapter

`adapters/git-cli` (`git-cli` package, `git_cli` lib), sole dependency
`workspace` → `agent-runtime`/`task-domain` (verified via
`cargo tree -p git-cli`; no storage, app-services, scheduler, IPC, Tauri,
HTTP, provider, or `git2`/libgit2 surface):

- `src/runner.rs` — direct-argv execution (`Command::new("git")` + args +
  `current_dir`, never a shell), process-local `GIT_TERMINAL_PROMPT=0` /
  `GIT_OPTIONAL_LOCKS=0` + closed stdin, 1 MiB output cap, 500-char
  diagnostic bound, lexical path normalization, bare/non-repo
  classification via `rev-parse` facts (never `.git` name heuristics).
- `src/observe.rs` — `is_repository` (working → `true`; bare →
  `BareRepositoryUnsupported`; other → `false`), `resolve_revision`
  (`rev-parse --verify --quiet <rev>^{commit}` → full OID), `default_branch`
  (`symbolic-ref refs/remotes/origin/HEAD`), `branch_exists` (`show-ref
  --verify --quiet`, exit-code classified), `worktree_status`
  (Clean/DirtyTracked/DirtyUntracked/Missing/Invalid incl. detached-HEAD →
  Invalid).
- `src/create.rs` — preflight order (classify → base proof → argv safety →
  protected check → collision preflight → target checks → primary baseline)
  → single mutation `worktree add -b <branch> -- <path> <base-oid>` →
  independent postcondition verification (registration, exact HEAD, exact
  branch, primary HEAD/ref/status unmoved).
- `src/lib.rs` — `GitCli` (`new` / `with_executable` for missing-git
  tests), `GitPort` impl; `compute_diff`/`remove_worktree` refuse with
  explicit TASK-0043 deferral (no mutation); `owner_is_live` honestly
  `false` (no liveness source; never faked).

## Command Safety

- All Git invocation is fixed-executable + argv array; `rg` sweep confirms
  no `sh -c`/`bash -c`/`cmd /C`/`powershell` execution (one doc comment
  names the forbidden forms).
- No remote operations: `fetch`/`pull`/`push`/`clone`/`ls-remote` appear
  nowhere as executed commands; tests use local temp repos, offline.
- Dash-led branch short names rejected before spawn; all paths absolutized
  (absolute paths cannot be option-parsed); `--` separates the explicit
  worktree path in `worktree add`.
- Injection regression tests: `;`, `$()`, backticks, quotes, pipes, Unicode
  branch names reach Git literally (exact ref asserted); space branch names
  rejected by domain validation; no payload file materializes (whole-fixture
  path-set diff allows only `managed-roots/` + `repo/.git/` additions).

## Repository Inspection

`rev-parse --is-inside-work-tree` / `--is-bare-repository` /
`--show-toplevel`: valid repo, bare (explicit failure, no clone),
non-repo, missing path, and file path all classified without panic and
without `.git` directory assumptions (linked worktrees use `.git` files).

## Revision Resolution

`resolve_revision` returns the full commit OID for `HEAD`, branch names, and
short OIDs; non-commit objects and unknown refs → `InvalidRevision`. The
OID — never a branch name — is what `create_worktree` pins the new branch
to, and the base OID's existence is re-proven pre-mutation.

## Protected Branch Resolution

Exactly one mechanism: read `refs/remotes/origin/HEAD`, validate
`refs/heads/` shape, return the full ref. No `main`/`master` literal (or any
other name) participates in logic — the sweep shows those strings only in
comments documenting the prohibition. Indeterminate metadata fails closed
with "refusing to guess". Proven semantic by the `trunkline` test.

## Managed Branch Creation

`BranchRef` domain validation first (full `refs/heads/` ref, no spaces/`..`),
plus adapter argv-safety (no dash-led short names). Branch created only by
`git worktree add -b`; never `-B`, never reset, never checkout in primary.

## Worktree Creation

Single Git-native `worktree add -b <branch> -- <path> <base-oid>`: exact base
commit + new managed branch + separate worktree, no file copies, no second
clone. Primary checkout never checked out/reset/cleaned/stashed; dirty
primary neither blocks (ADR-0023) nor is touched.

## Postcondition Verification

Exit 0 is never trusted alone. After mutation the adapter re-observes:
`worktree list --porcelain` registration (canonicalized compare), managed
`HEAD == base_oid`, managed `HEAD` ref == requested branch (detached →
failure), primary `HEAD` OID + ref + status byte-identical to baseline.
Contradiction → `CheckoutFailed` with partial-state note; no repair.

## Isolation

Proven by tests: tracked-file edit in managed worktree → `DirtyTracked`
there, primary byte-identical and `Clean`; two worktrees from one base with
distinct paths/branches stay mutually and primary-isolated. No
commit/merge in tests.

## Collision Handling

- Branch exists (unproven ownership) → `BranchCollision`; existing ref OID
  unchanged; no worktree registered; target untouched.
- Target exists → `WorktreeCollision`; sentinel bytes unchanged; no branch
  leaked.
- Target equals/nests inside primary → `WorktreeCollision` (canonicalized).
- Managed == protected/default → `BranchCollision` naming the protection,
  before mutation.

## Partial Failure Semantics

`CheckoutFailed` details carry re-observed `branch_exists` /
`worktree_registered` flags (mutation failure) or the failed postcondition
name (verification failure), always with "reconciliation deferred to
TASK-0042". Pre-mutation rejections (bad revision, collisions, protected
misuse, unsupported repo) create nothing — asserted per test.

## Trust / Security Boundary

Git worktree isolation != OS sandbox. Fixtures define no hooks/filters; the
adapter introduces no shell and stays non-interactive, but repository-local
hooks/filters/config may run as part of normal Git operation.
Hostile-repository hardening is `DEFERRED_BY_DESIGN → G6`, not a solved
property. Missing/unusable `git` → typed `GitBackendFailure` (conceptually
`GitUnavailable`); no fallback backend. No secrets, no credentials, no
network.

## Cross-Platform Behavior

Rust `Path`/argv APIs only; absolute argv paths; spaces/Unicode/metachar
paths verified literally. Detected Git: `2.55.0`. No minimum-version gate
was added: every command used (`rev-parse`, `symbolic-ref`, `show-ref`,
`worktree add/list`, `status --porcelain`) predates any plausible toolchain
by a decade; `--` separator support in `worktree add` verified on the
detected Git.

## Deferred Persistence

No migration, no tables: schema stays v4 (`0001`–`0004` only, untouched).
No `workspace.created`/`workspace.removed` implementation (contract frozen
in ADR-0023 §9; TASK-0042 implements).

## Deferred Cleanup / Diff

`compute_diff`/`remove_worktree` refuse explicitly; tests prove refusal
removes nothing. No `diff`/`merge`/`rebase`/`cherry-pick`/`commit`/`push`
product API. Test teardown deletes only wholly test-owned temp trees.

## Verification

```text
cargo fmt --all -- --check            # clean
cargo check --workspace               # clean
cargo test --workspace --no-fail-fast # all suites ok, 0 failures
cargo clippy --workspace --all-targets --all-features -- -D warnings  # clean
cargo test -p workspace               # domain contract green
cargo test -p git-cli                 # 5 unit + 11 creation + 9 safety green
cargo test -p product-version         # green
cargo tree -p workspace / -p git-cli  # bounded graphs (above)
```

Focused scenario coverage (`adapters/git-cli/tests/`): success + identity +
primary-untouched (spaces path), isolation, multi-workspace isolation,
branch collision, path collision, invalid revision, bare, not-repo/missing,
dirty primary (tracked + untracked, byte-identical), protected non-standard
branch, canonical inspection fields, injection matrix, dash-led refusal,
nested-target refusal, missing-git typed failure, deferred-op refusal
without mutation, detached-HEAD/file Invalid observation, honest
non-live owner.
