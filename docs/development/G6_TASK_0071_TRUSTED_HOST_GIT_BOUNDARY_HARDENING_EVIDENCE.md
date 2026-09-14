# G6 Trusted Host Git Boundary Hardening (TASK-0071)

> Remediation of the F-003 gaps left open after TASK-0070's independent
> adversarial re-audit: (1) `.git` pointer validation accepted any existing,
> syntactically valid directory, so a caller could redirect a worktree's
> repository identity to an unrelated but real repository; (2)
> `GitCli::worktree_status` (and, transitively, `compute_diff`) ran Git
> directly against a path without performing that identity validation at
> all. A final independent-style self-review also found that the first
> back-reference design could be bypassed with a forged local linked-worktree
> metadata shim whose reverse `gitdir` pointed back to the expected worktree
> while `commondir` pointed at a foreign repository. All three paths are
> closed. The already-applied `run_git` hardening
> (`env_clear`, explicit `-c` neutralization of hooks/fsmonitor/credential
> helper/external diff/pager/editor, `GIT_CONFIG_NOSYSTEM`,
> `GIT_CONFIG_GLOBAL=/dev/null`) is preserved unchanged.
>
> **Verdict: remediation implemented and independently retestable. G6 status
> to be updated by the required independent re-review (§20 of the task
> brief), not self-certified here.**

## 1. Repository identity

| Field | Value |
| --- | --- |
| Worktree | `/Users/artemmac/dev/runlith-task0071` |
| Branch | `task-0071-g6-trusted-host-git-boundary-hardening` |
| Starting HEAD | `e9009a8f7ca8db8230232c3a647dd6950cdfc3cf` |
| Initial TASK-0071 commit | `4128e7bfd58bf8c67a0a042ab26d3c694e28f96c` (superseded on this branch by the self-review remediation) |
| Final code remediation commit | `06b33dc906ea480316b4d09636465dc901217ed3` |
| Host | macOS 26.6.2 (build 25G83), arm64 (aarch64-apple-darwin) |
| Toolchain | rustc 1.95.0 / cargo 1.95.0 |

`e9009a8` is the legitimate vulnerable pre-fix baseline: it carries none of
`validate_repository_metadata`, none of the `run_git` hardening, and no
TASK-0071 test coverage (confirmed via
`git log --oneline 2402ce2..e9009a8 -- adapters/git-cli/src/runner.rs`,
which returns no commits — `runner.rs` is unchanged since the F-003-confirmed
`2402ce2` audit state).

Before this remediation, the branch already carried uncommitted TASK-0071
work (an initial, insufficient `validate_repository_metadata` plus the
`run_git` hardening, and three keeper tests) — preserved and built upon, not
discarded.

## 2. Independent-review findings reproduced

| Finding | Reproduced | Notes |
| --- | --- | --- |
| 1 (BLOCKER) — valid-but-wrong `gitdir:` redirects identity | **Yes** | Repo A's `.git` replaced with `gitdir: <repo B's real .git>`; `rev-parse HEAD` in A returned B's OID. The in-progress `validate_repository_metadata` only checked existence + directory-ness, which B's real `.git` trivially satisfies. |
| 2 (HIGH) — `worktree_status` bypasses metadata validation | **Yes** | `observe::worktree_status` called `run_git` directly on the absolutized path; `classify_repository`'s `validate_repository_metadata` call was never reached. |
| 3 — `compute_diff` bypasses validation | **Yes** | `diff::compute_diff` calls `observe::worktree_status` (itself unvalidated, per Finding 2) and then makes three further direct `run_git` calls (`prove_base_present`, `committed_changes`, `working_changes`) on the same path with no validation of its own. |
| 4 (MEDIUM) — `git_extension_execution_surfaces_are_disabled` is inert | **Yes** | It calls `is_repository` → `rev-parse --is-inside-work-tree`, which never invokes `core.fsmonitor`. It passes unchanged against the vulnerable e9009a8 baseline, proving it exercises no real hardening. Preserved as-is (still a valid, if narrow, keeper); a new keeper using the real path is added alongside it (§4, §5). |
| 5 (MEDIUM) — TOCTOU between validation and invocation | **Acknowledged, bounded** | See §7. No redesign attempted per task scope; validation is re-run at every entry point immediately before the Git calls it guards, rather than cached, which bounds the window to a single synchronous function's execution. |
| 6 (self-review BLOCKER) — forged linked-worktree shim redirects `commondir` | **Yes** | The initial `4128e7b` fix verified that the target's reverse `gitdir` pointed back to this `.git` and that `commondir` was an existing directory, but did not bind the target to that common repository. A local forged metadata directory could satisfy the reverse link while setting `commondir` to repo B; `resolve_revision(A, "HEAD")` then returned B's OID. The final fix additionally requires the metadata target itself to be structurally owned at `<resolved commondir>/worktrees/<id>`. |

## 3. Git entry-point audit

Every `run_git`/`run_git_text` call site in `adapters/git-cli/src` was
enumerated and traced back to whether it is reachable with a caller-supplied,
provider-writable path before any identity validation:

| Function | Path | Validated via |
| --- | --- | --- |
| `classify_repository` (→ `is_repository`, `repository_root`, `resolve_revision`, `default_branch`, `branch_exists`, `is_registered_worktree`, `worktree_for_branch`, `create_worktree`, `attach_worktree`, `remove_branch`) | already validated | `validate_repository_metadata` called directly inside `classify_repository` |
| `observe::worktree_status` (→ also the entry point for `compute_diff` and `remove_worktree`) | **was unvalidated — fixed** | `validate_repository_metadata` call added before the first `run_git` |
| `diff::compute_diff` | **was unvalidated — fixed (defense in depth)** | explicit `validate_repository_metadata` call added, in addition to the validation `worktree_status` now performs first |
| `cleanup::remove_worktree` | **was unvalidated for its own later calls — fixed (defense in depth)** | explicit `validate_repository_metadata` call added before `worktree_status`/`attached_branch`/`discover_primary` |
| `observe::worktree_registered`, `create::worktree_registered`, `create::observe_primary`, `create::verify_postconditions`, `cleanup::attached_branch`, `cleanup::discover_primary` | operate only on an already-classified `root`/freshly Git-created `target` | no independent caller-controlled path reaches these without passing through one of the validated functions above first |

This gives a single defensible boundary: one function,
`runner::validate_repository_metadata` (now `pub(crate)`), is the sole
implementation of the identity check, invoked at every point where this
adapter is about to run Git against a path whose `.git` metadata may be
provider-writable content — never re-implemented, never diverged.

## 4. Remediation

### A — valid-but-wrong `gitdir:` pointer (Finding 1)

`runner::validate_repository_metadata` previously accepted any `.git` file
whose `gitdir:` target existed and was a directory. That is exactly the
shape of another repository's real `.git` directory, so it is insufficient.

The fix adds `validate_linked_worktree_backreference`, which encodes the
actual Git semantics for a linked worktree rather than inventing a new rule:
when Git creates a linked worktree, it writes `<worktree>/.git` containing
`gitdir: <main>/.git/worktrees/<id>`, and it writes the *reverse* link at
`<main>/.git/worktrees/<id>/gitdir`, containing the absolute path back to
`<worktree>/.git`, plus `<main>/.git/worktrees/<id>/commondir`, a relative
path back to the common `.git` directory. This is the exact back-reference
`git worktree list`/`git worktree repair` use to detect a moved or foreign
worktree (verified empirically — see the probe transcript in §8). The fix
requires:

1. the pointer target exists, is a directory, and is not itself a symlink;
2. the target's `gitdir` file exists and its content resolves (after the
   same symlink canonicalization Git itself applies when it writes these
   paths — needed for e.g. macOS `/tmp` → `/private/tmp`) to this exact
   `.git` file;
3. the target's `commondir` file exists and resolves to a real, non-symlink
   directory;
4. after canonicalization, the metadata target's parent is exactly
   `<resolved commondir>/worktrees`, binding the target to the repository
   whose common metadata Git will actually use.

An arbitrary redirection target — another repository's ordinary `.git`
directory, another repository's *own* worktree metadata directory, or a
forged local shim with a valid-looking reverse link — fails one of these
checks. The shim case is the subtle one: its reverse `gitdir` can point back
to this worktree, but if its `commondir` points at repo B then the shim is not
located under `B/.git/worktrees/<id>` and is rejected before Git sees it. A
legitimate Runlith-managed linked worktree (created via `create_worktree`/
`attach_worktree`, i.e. real `git worktree add`) satisfies all four checks
because Git wrote exactly that structure.

### B — validation coverage (Findings 2, 3)

- `observe::worktree_status` now calls `validate_repository_metadata` right
  after the existing missing/is-file checks and before its first `run_git`
  call. This is also the fix for `compute_diff` and `remove_worktree`,
  since both call `worktree_status` first.
- `diff::compute_diff` additionally calls `validate_repository_metadata`
  itself, immediately before its own further direct `run_git` calls
  (`prove_base_present`, `committed_changes`, `working_changes`), rather
  than relying solely on the validation `worktree_status` already performed
  a moment earlier — see §7 (TOCTOU).
- `cleanup::remove_worktree` calls `validate_repository_metadata` at its own
  entry, before `worktree_status`, `attached_branch`, `discover_primary`,
  and the removal mutation.

### C — real F-003 regression keeper (Finding 4)

`worktree_status_production_path_disables_fsmonitor_execution` (new,
`adapters/git-cli/tests/safety_regression.rs`) writes a hostile
`core.fsmonitor = touch <marker>` into a fixture's real `.git/config` and
calls `GitCli::worktree_status` — the actual production path
`GitCli::worktree_status` → `git status` that TASK-0070's supplemental
evidence used to reproduce F-003 (`core.fsmonitor` fires on `git status`,
never on `rev-parse --is-inside-work-tree`, which is why the existing
`git_extension_execution_surfaces_are_disabled` keeper is inert against the
same baseline). The existing keeper is left in place unmodified (it is
still a valid, narrower assertion); this is an addition, not a replacement.

### D — valid-but-wrong pointer regression (Finding 1)

`valid_but_wrong_gitdir_pointer_redirection_is_refused` (new) builds two
separate fixture repositories with observably distinct HEAD commits, points
A's `.git` at B's real, valid `.git` directory, and asserts that both
`is_repository` and `resolve_revision(..., "HEAD")` fail closed with
`GitBackendFailure` rather than reporting B's identity.

### E — `worktree_status` metadata regression (Findings 2, 5)

`worktree_status_refuses_symlinked_and_redirected_git_metadata` (new)
proves, specifically through `worktree_status` (not `is_repository`), that
(1) a symlinked `.git` fails closed, and (2) a redirected-but-valid `.git`
(pointing at a second fixture's real metadata) fails closed rather than
silently returning canonical status evidence (`Clean`/`DirtyTracked`/etc.)
for the wrong repository.

### F — forged `commondir` shim regression (self-review Finding 6)

`forged_linked_worktree_commondir_redirection_is_refused` builds two
independent repositories, replaces A's `.git` with a pointer to a forged
linked-worktree-shaped metadata directory inside A, writes the expected
reverse `gitdir` back to A, but points `commondir` and `HEAD` at repository B.
The keeper proves that this shape is rejected before `resolve_revision` can
return B's commit identity. This test was first added against `4128e7b` and
failed with `resolve_revision(A, "HEAD") = Revision(<B HEAD>)`, directly
demonstrating the incomplete first fix before the structural ownership check
was added.

## 5. RED evidence

Captured against a disposable detached worktree at the vulnerable baseline
(`git worktree add --detach <scratch>/red-baseline-e9009a8 e9009a8`, removed
afterward — never the shared `~/dev/runlith` main worktree). A temporary,
uncommitted test file (`red_task0071_evidence.rs`, not part of any commit)
exercised the original three cases with assertions inverted to prove the
vulnerable behavior. The self-review shim bypass was independently re-run in
the same manner via temporary `red_task0071_commondir.rs`; neither RED file
is part of the task branch.

| Attack | SHA | Command | Result | Exit code |
| --- | --- | --- | --- | --- |
| Valid-but-wrong `gitdir:` redirection | `e9009a8f7ca8db8230232c3a647dd6950cdfc3cf` | `cargo test -p git-cli --test red_task0071_evidence red_valid_but_wrong_gitdir_pointer_redirects_identity -- --nocapture` | `resolve_revision(A) == B's HEAD` (e.g. `839df659233e0aada218ec4294746920dd485bf0` == `839df659233e0aada218ec4294746920dd485bf0`); `is_repository(A)` returns `Ok(true)` | `0` (assertions proving the vulnerability passed) |
| `worktree_status` + symlinked `.git` | `e9009a8f7ca8db8230232c3a647dd6950cdfc3cf` | `cargo test -p git-cli --test red_task0071_evidence red_worktree_status_accepts_symlinked_git_metadata -- --nocapture` | `worktree_status(symlinked) = Clean` — no error, canonical-looking status returned for hostile metadata | `0` |
| `worktree_status` + `core.fsmonitor` helper | `e9009a8f7ca8db8230232c3a647dd6950cdfc3cf` | `cargo test -p git-cli --test red_task0071_evidence red_worktree_status_executes_fsmonitor_helper -- --nocapture` | marker file created (`marker.exists() == true`) — hostile helper executed by the production `worktree_status` path | `0` |
| Forged linked-worktree shim with foreign `commondir` | `e9009a8f7ca8db8230232c3a647dd6950cdfc3cf` | `cargo test -p git-cli --test red_task0071_commondir red_forged_linked_worktree_commondir_redirects_identity -- --nocapture` | `resolve_revision(A) == B's HEAD` (`943864fee81c6dbae0ec97ee37545df438fc526f` in the recorded run) even though the shim's reverse `gitdir` pointed correctly back to A | `0` (assertion proving the vulnerability passed) |

Full run (all three in one process):

```text
$ cargo test -p git-cli --test red_task0071_evidence -- --nocapture
running 3 tests
RED worktree_status(symlinked) = Clean
test red_worktree_status_accepts_symlinked_git_metadata ... ok
test red_worktree_status_executes_fsmonitor_helper ... ok
RED resolve_revision(A) = 839df659233e0aada218ec4294746920dd485bf0 (B's HEAD = 839df659233e0aada218ec4294746920dd485bf0)
test red_valid_but_wrong_gitdir_pointer_redirects_identity ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
```

(Exit code `0` here is *positive* confirmation of the vulnerability: each RED
test asserts the unsafe outcome and only passes when that outcome occurs.)

## 6. GREEN evidence

```text
$ git diff --check
(no output — exit 0)

$ cargo fmt --check
(no output — exit 0)

$ cargo clippy -p git-cli --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.38s

$ cargo test -p git-cli
running 8 tests   (src/lib.rs unit tests)           ... 8 passed
running 18 tests  (tests/diff_cleanup.rs)           ... 18 passed
running 10 tests  (tests/failure_matrix.rs)         ... 10 passed
running 2 tests   (tests/liveness_binding.rs)       ... 2 passed
running 16 tests  (tests/safety_regression.rs)      ... 16 passed   (61 pre-existing + 5 new)
running 11 tests  (tests/worktree_creation.rs)      ... 11 passed
Doc-tests git_cli: 0 passed
Total: 66 passed, 0 failed, 0 ignored

$ cargo test -p app-services --test g6_security_integration
running 7 tests
test child_and_cross_workspace_authority_expansion_fail_closed ... ok
test repository_allow_cannot_create_privileged_authority ... ok
test unsupported_isolation_fails_closed_before_privileged_operation ... ok
test runner_revalidates_exact_process_target_adjacent_to_operation ... ok
test policy_runner_and_audit_form_one_trusted_launch_boundary ... ok
test secret_release_requires_exact_authority_and_audits_only_opaque_reference ... ok
test approval_survives_restart_consumes_once_and_replay_is_refused ... ok
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The pre-existing 61 `git-cli` tests (8 unit + 18 + 10 + 2 + 12 + 11, matching
the count named in the task brief) all remain green unmodified, plus the 5
new tests, for 66 total. `git_extension_execution_surfaces_are_disabled` and
the two pre-existing `.git`-metadata keepers (`hostile_git_metadata_is_
rejected_before_git_discovery`, `malicious_git_pointer_target_is_rejected`)
were left exactly as committed and still pass.

### Note on a self-inflicted false failure during verification

Mid-verification, five `safety_regression` tests failed transiently after a
`cargo test` run in the disposable RED-baseline worktree had been pointed
(via `CARGO_TARGET_DIR`) at this worktree's own `target/` directory to share
build cache across the two source trees. `cargo clean -p git-cli` followed
by a rebuild reproduced the then-current 65/65 green result; the final
post-self-review suite is 66/66 as shown above. This was
build-cache cross-contamination from a one-off convenience during evidence
gathering, not a code defect, and is called out here only for transparency
since it briefly looked like a regression during this session.

## 7. Security analysis

**Legitimate linked worktrees remain supported.** Every worktree this
adapter itself creates (`create_worktree`, `attach_worktree`) is created via
real `git worktree add`, which writes the forward pointer and the matching
`gitdir`/`commondir` back-reference. `validate_linked_worktree_backreference`
requires exactly that shape and nothing more, so no existing test (all of
which exercise real, Git-created linked worktrees) needed to change to stay
green.

**Arbitrary repository redirection is prevented.** A `.git` file can only
name a target that (a) exists, (b) is a real directory, (c) contains an
absolute reverse `gitdir` that canonicalizes to this exact `.git` file, and
(d) is itself located at `<canonical commondir>/worktrees/<id>`. The fourth
condition closes the self-review bypass: fabricating a metadata-shaped
directory inside the authorized worktree no longer lets an attacker redirect
Git's refs/objects by setting `commondir` to a different repository. A
foreign real worktree target also fails because its reverse `gitdir` points
to its actual worktree rather than the caller-controlled one.

**Git entry points obtain validated identity.** `classify_repository`
already validated every observation function (`is_repository`,
`repository_root`, `resolve_revision`, `default_branch`, `branch_exists`,
`is_registered_worktree`, `worktree_for_branch`) and every mutation entry
point that resolves its root via `repository_root`/`classify_repository`
(`create_worktree`, `attach_worktree`, `remove_branch`). `worktree_status`
now does too, which transitively covers `compute_diff` and
`remove_worktree`'s first Git-touching call; both of those also perform
their own explicit re-check immediately before their remaining direct
`run_git` calls, so every Git invocation in the crate is now preceded by a
validation of the exact path it is about to operate on.

**Residual TOCTOU (Finding 5).** A caller-controlled worktree's `.git`
metadata is provider-writable content between (and in principle during) the
validation call and the subsequent `run_git` invocation. This remediation
does not eliminate that window — doing so would require either an atomic
open-and-verify primitive Git's CLI does not expose, or sandboxing Git's own
process, both outside this task's scope (§9 non-goals: no adapter redesign).
What it does is (a) collapse the window to the shortest practical span, by
re-validating immediately before each cluster of Git calls rather than once
and caching, and (b) ensure every Git-touching function performs its own
check rather than trusting an earlier caller's. This is the same posture the
codebase already accepts elsewhere (`create.rs`'s own module doc: "a TOCTOU
race surfaces as a postcondition contradiction, not as silent success").
Documented here as an accepted residual, not silently dropped.

## 8. Empirical verification of Git's linked-worktree back-reference semantics

Before implementing the fix, the actual on-disk shape Git produces was
verified directly (not assumed), since the linked-worktree back-reference is
the entire basis for the fix:

```text
$ git worktree add -q -b feat ../wt HEAD
$ cat main/.git/worktrees/wt/gitdir
/private/tmp/.../gitwt-probe/wt/.git
$ cat main/.git/worktrees/wt/commondir
../..
$ cat wt/.git
gitdir: /private/tmp/.../gitwt-probe/main/.git/worktrees/wt
```

And the redirection attack itself, confirming Git's actual behavior when a
`.git` file is manually pointed at an unrelated repository's real metadata:

```text
$ mv A/.git A/.git.bak && echo "gitdir: $PWD/B/.git" > A/.git
$ git -C A rev-parse HEAD
1ca5b10b4e7068659a9d15bdd268f301958070e4   # B's HEAD, not A's (445ca374...)
```

## 9. Changed files

- `adapters/git-cli/src/runner.rs` — strengthened
  `validate_repository_metadata` (Finding 1), made it `pub(crate)`.
- `adapters/git-cli/src/observe.rs` — `worktree_status` now validates
  before running Git (Finding 2).
- `adapters/git-cli/src/diff.rs` — `compute_diff` validates before its own
  direct `run_git` calls (Finding 3).
- `adapters/git-cli/src/cleanup.rs` — `remove_worktree` validates before
  its own direct `run_git` calls (defense in depth).
- `adapters/git-cli/tests/safety_regression.rs` — 5 new tests (§4 C–F plus
  the Finding 1 keeper); 3 pre-existing tests from the in-progress work
  unchanged.

No policy, approval, Secrets, H7, or unrelated production code was touched.

## 10. Canonical state

- `completed_through` (before this task lands): `TASK-0070`
- `current_gate`: `G6`
- `gate_status`: `IN_PROGRESS` — this remediation requires independent
  security re-review (task §20) before any closure verdict; not
  self-certified here.
- This task: `TASK-0071`, scope = F-003 trusted-host Git boundary hardening
  (repository-identity validation coverage + regression hardening), no
  merge performed.
