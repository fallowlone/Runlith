# G5 Closure Remediation (TASK-0049)

> Remediation evidence for the three TASK-0048 blockers (B1–B3) plus the
> recommended H1, against `docs/development/G5_CLOSURE_AUDIT.md`
> (verdict `G5_REMEDIATION_REQUIRED`). TASK-0048 stays the historical
> record of failure; this document records what changed and the evidence
> that each blocker no longer exists. Authority: `AGENT.md`,
> `INVARIANTS.md`, ADR-0005/0009/0018/0019/0021/0022/0023/0024.
>
> Verdict: **`G5_REMEDIATION_READY_FOR_REAUDIT`**. G5 stays `in_progress`;
> TASK-0050 (re-audit) is next. G5 is NOT closed here.

## 1. B1 — execution-start identity safety (RESOLVED)

**Root cause.** `OrchestrationService::enter` committed `claim` then
`start` (Task → RUNNING, journaled) before `create_execution` checked
uniqueness, and nothing checked attempt freshness at entry at all. A
synchronously detectable identity conflict therefore committed unrelated
lifecycle facts: execution-id collision left the Task wedged RUNNING
with no Execution (`ALREADY_EXISTS` after mutation); reused attempt ids
were accepted at entry and stranded later in the background (workspace
created, no attempt).

**Fix.** Identity preflight in `enter`
(`crates/app-services/src/orchestration_service.rs`): after readiness,
before the first mutation, refuse with typed `StorageError::Duplicate`
(which maps to wire `ALREADY_EXISTS`) when the execution id already
exists (naming its owning task) or the attempt id already exists
(naming its owning execution). Zero claim, zero start, zero execution,
zero lease, zero attempt, zero worktree, zero journal lifecycle facts;
the Task stays in its pre-run state and is rerunnable with fresh ids.
There is no idempotent-replay path — a duplicate identity is always a
refusal (the entry boundary never re-enters an existing
Execution/Attempt). Store-level uniqueness guards stay the backstop for
concurrent races; the preflight fixes the deterministic ordering.

**Failure-ordering audit (§7).** All other `run` validations (id
shapes, repository inputs, provider precheck, managed-root ensure)
already run before `enter_execution`; no other synchronous validation
occurs post-claim. Post-entry failures (lease refusal, provider crash,
timeout, cancel) are genuinely async/environmental and correctly use
recoverable intermediate state. No other instance of the ordering bug.

**Regression matrix (all through `dispatch_execution` unless noted).**

| Test | Collision | Refusal | Zero mutation proven | Rerunnable |
|---|---|---|---|---|
| `run_execution_id_collision_refuses_without_mutation` (agentd) | E-A owned by T-A, requested by T-B | `ALREADY_EXISTS`, no job | T-B READY; E-A stays T-A's; no A-B; no lease; empty workspaces dir; no Claimed/Started/ExecutionCreated/AttemptCreated/WorkspaceCreated for T-B | T-B starts with E-B/A-B |
| `run_attempt_id_collision_refuses_without_mutation` (agentd) | A-1 under E-A, reused by T-B/E-B | `ALREADY_EXISTS`, no job | T-B READY; no E-B; A-1 stays E-A's; no lease; empty workspaces dir; journal clean | T-B starts with E-B2/A-2 |
| `enter_execution_refuses_duplicate_identities_before_any_mutation` (app-services, service-level pin) | both shapes via `enter_execution` | typed `Duplicate` | T-2 READY; no E-2/A-2; journal exactly `[TaskCreated, MarkedReady]` | fresh ids enter |

Both keeper tests failed before the fix for the audited reasons
(T-B left RUNNING; reused attempt accepted with a background job) and
pass after. Full `app-services` + `agentd` suites green (no existing
test relied on duplicate acceptance).

## 2. B2 — deterministic capture-grace test (RESOLVED)

**Root cause.** The implementation waited the capture grace
sequentially per stream (stdout grace + stderr grace ≈ 2×grace + spawn),
but the test asserted a fixed `< 4 s` divorced from the configured
grace — 1.7 s of headroom that full-suite load consumes (4.40 s
observed vs `< 4 s`).

**Fix (test + small H3 production improvement).** Drains now join under
one shared grace deadline (`join_drains_graceful` in
`adapters/provider-claude/src/process.rs`; the threads already ran
concurrently, so sequential graces doubled the bound for no reason —
no new async machinery, same detach/no-leak semantics). The test
(`daemon_grandchild_holding_pipes_does_not_block_finalization`) asserts
the deterministic contract: `elapsed < grace×2 + 3 s` scheduling margin
(5 s with the 1 s test grace) **and** `elapsed + 2 s < 10 s`
grandchild lifetime (explicitly `<<`). Fixture grandchild extended
5 s → 10 s to separate the bound from the lifetime. Stable ~1.2 s in
isolation; the bound still fails on any regression to unbounded
grandchild blocking (10 s) while tolerating per-stream-sequential
timing (~2 s).

## 3. B3 — provider contract as authority (RESOLVED)

**Contract suite.** `crates/execution-contract` (new, minimal, neutral:
depends only on `agent-runtime`, `task-domain`, `scheduler`) exports
`run_execution_port_contract` — the same behavioral contract runnable
against any `ExecutionPort`: success → submittable bounded evidence
(never completion); failure → canonical `Failed`; malformed → never
success; timeout → canonical `TimedOut`; provider refs opaque/stable.
Only seam-promised behavior; cancel/cwd/partial/Task-mutation are
documented exclusions (not on the seam; proven provider-side or
structurally). No live provider/network/quota. Negative control: wiring
a failing scenario as `success` fails the suite (proven, then
restored).

| Participant | Location | Result |
|---|---|---|
| FakeAgent (Success/Crash/MalformedResult/virtual-Timeout) | `adapters/fake-agent/tests/execution_port_contract.rs` | passes |
| ClaudeExecutor (success/fail/malformed fixtures + 1 s-timeout hang) | `adapters/provider-claude/tests/execution_port_contract.rs` (unix) | passes |

Second-provider recipe is mechanical and documented in the crate docs:
implement `ExecutionPort` → dev-depend on the contract crate → build
the four behaviors → invoke the suite → register the factory in
`agentd`. No Codex implementation (non-goal).

**ADR.** `ADR-0024-g5-provider-execution-seam.md` (next number after
0023) records: `ExecutionPort` is the seam; adapters are ACLs;
provider protocols stay in adapters; `agentd` composes; IPC v2
run/get/cancel; Task/Execution/Attempt/Session distinct; provider
success never completes; fail-fast entry (§6); Claude Code CLI first
with adapter-local version policy; provider-owned subscription auth;
cancel/liveness stay adapter-local (H6 decision); second-provider
addability. `INDEX.md` + `COVERAGE.md` (new G5 row) updated. No new
invariants.

**Docs.** `providers.md`: frozen execution seam (§2) + first
implementations (§2a, Claude as first not architecture) + contract
suite/recipe (§13a) + out-of-band cancel/liveness (§15) + closed vs
open items (§16). `ipc.md`: new §16 (protocol v2, envelope/fields,
run/get/cancel, `PROVIDER_UNAVAILABLE=10`, provider-neutrality,
negotiation/compatibility, 5 s exchange bound, char-safe errors).
`VERSIONING.md`: supported IPC `0, 1, 2` (version-agnostic wording).

## 4. H1 — multibyte error rendering (RESOLVED)

Byte cuts at 64 B / 512 B panicked the connection handler on validated
multibyte ids (`execution.rs` `truncate_id`; `dispatch.rs` `truncate`
via `String::truncate` + `truncate_id`). All three now floor to a UTF-8
char boundary through one shared `dispatch::floor_char_boundary`
helper (the `execution.rs` detail path already did this; now all do).
Sweep confirmed no other UTF-8 slicing sites (remaining `[..]` cuts are
byte frames, SIDs, or `Vec`s). Tests: 5 id shapes (ASCII, 2/3/4-byte,
mixed, each with byte-64 provably mid-char) × status/cancel NOT_FOUND
→ typed errors, handler survives, composition still usable; helper unit
tests; direct `truncate_id` boundary tests. The IPC test panics on the
old code with the exact audited signature (verified by temporary
revert), passes after.

## 5. Other hardening disposition

- **H2 quadratic JSON scan — RESOLVED.** `string()` validated the whole
  remaining input per char. Now decodes only the char's own bytes
  (`decode_first_char`, O(1), accept/reject-preserving; the
  whole-rest failure mode was unreachable via `&str` anyway). Tests:
  decoder unit battery + 512 KiB two-byte / 1 MiB ASCII linear parses
  under a generous 10 s guard (old: ~45 s / ~22 s; new: ~ms, suite
  0.09 s). No escape/Unicode regressions (existing battery green).
- **H3 sequential grace — RESOLVED** (see §2).
- **H4 branch-slug collision — DEFERRED.** Fail-closed today (second
  lease stays Allocated, first intact); fixing needs a workspace
  identity redesign, out of scope. Follow-up, not downgraded.
- **H5 production version gate — DEFERRED with new evidence.** Trialled
  wiring `detect` into `provider_precheck`; reverted: `detect` can
  block up to its 10 s probe timeout while `start_run` must answer
  within the 5 s IPC exchange bound (the cancel-path binary test timed
  out on its frame when the gate was active). Needs a bounded-fast
  probe design (cached/background detection) as a dedicated follow-up.
  Safe to defer: incompatible CLIs still fail closed at parse time
  (`MalformedOutput`, honest state, no authority granted). Rationale
  recorded at the `provider_precheck` callsite.
- **H6 neutral primitives — RESOLVED as documented decision.**
  `CancelFlag`/`LiveProcessTracker` stay in `provider-claude`
  (ADR-0024 §9): no extraction until a second provider proves the
  abstraction; liveness crosses to Workspace only via neutral
  `OwnerLiveness`. No speculative shared runtime crate.
- **H7 worktree dirtying — DEFERRED** (G6 isolation; fail-closed
  cleanup preserved, no force-clean/`gitignore`/deletion hacks).
- **H8 cancel-before-spawn — RESOLVED.** Pre-spawn flag check in
  `spawn_and_supervise` (structural failures still beat it); fixture
  marker proves no child starts; unit test updated for the explicitly
  authorized reorder (pre-set cancel now beats spawn-attempt; disclosed
  per test-mutation policy).
- **H9 large files — DEFERRED** (review signal only; only local edits
  made where B1–B3/H1 required).

## 6. Finding status vs TASK-0048

| Finding | Status | Evidence |
|---|---|---|
| B1 identity-unsafe entry | RESOLVED | §1 matrix |
| B2 flaky grace test | RESOLVED | §2, repeated + full-suite runs |
| B3 contract not materialized | RESOLVED | suite green ×2, ADR-0024, doc updates |
| H1 multibyte panic | RESOLVED | §4 |
| H2 quadratic parse | RESOLVED | §5 |
| H3 sequential grace | RESOLVED | §2 |
| H4 slug collision | DEFERRED | fail-closed; redesign follow-up |
| H5 version gate | DEFERRED | 5 s IPC bound vs 10 s probe; safe fail-closed |
| H6 primitive placement | RESOLVED (document) | ADR-0024 §9 |
| H7 worktree dirtying | DEFERRED | G6 isolation |
| H8 spawn-then-kill | RESOLVED | §5 |
| H9 file sizes | DEFERRED | signal only |

No new production defect discovered beyond the audited set. No new
invariants. Dependency direction holds: `cargo tree -e normal` from
domain crates never reaches a provider; `execution-contract` reaches
only neutral crates; providers consume it via dev-deps only.

## 7. Commands and census

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --no-fail-fast (×3 full runs after final changes)
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo tree direction audit (provider-claude, execution-contract)
rg sweeps: provider == "claude" (none in core), unsafe slicing, shell
```

Census deltas from the TASK-0048 baseline (624 passed + 1 ignored):
agentd lib +6 (2 B1 keepers, 1 H1 IPC, 1 H1 `truncate_id` unit, 2
dispatch H1 units) (+1 H5 trial test added then removed with the
revert — net 0);
app-services orchestration +1 (service-level B1 pin); provider-claude
lib +3 (H2 decoder, H2 linear, H8 reorder), process_fixtures +1 (H8
no-spawn) with the B2 assertion reworked; fake-agent +1 (contract),
provider-claude +1 contract target, execution-contract (harness, no
tests of its own — exercised through both providers). Ignored
real-provider smoke untouched.

## 8. Remaining hardening and follow-ups

1. TASK-0050 G5 Closure Re-Audit (next; G5 stays `in_progress`).
2. H4 workspace identity injectivity redesign.
3. H5 bounded-fast production version gate (cached/background detect).
4. H7 provider-artifact worktree hygiene (G6 isolation scope).
5. G6 design ADRs (Policy Engine, Secrets Broker, Security Audit) per
   the TASK-0048 forward check — required before G6 implementation.
