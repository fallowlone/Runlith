# G5 Claude Provider — Implementation Evidence (TASK-0046)

> First real-provider vertical slice: Claude Code CLI through the durable,
> provider-neutral execution architecture. Authority: `docs/architecture/
> providers.md`, ADR-0009, ADR-0005, ADR-0023; G4 residuals from
> `docs/development/G4_CLOSURE_AUDIT.md` (F1, F2).

## 1. Provider boundary

```text
Task → Execution → Attempt (durable, provider-neutral)
  → WorkspaceLease (G4, per-Execution)
  → OrchestrationService::schedule_in_workspace (new, generic over ExecutionPort)
  → ClaudeExecutor: scheduler::ExecutionPort (adapters/provider-claude)
  → claude CLI (argv-only, cwd-bound, no shell)
  → ParsedOutput (adapter-internal) → WorkEvidence / ExecutionProblem
  → conclude_attempt → submit_result → independent evaluation
```

The canonical seam is `scheduler::ExecutionPort` — the same seam the
test-side `FakeExecutor` wraps. No duplicate contract was created: the
repository had no frozen `AgentProviderPort` (providers.md §2, §16 leave
it open), and `ExecutionPort` is the de-facto provider seam the G3
orchestrator injects. A future `provider-codex` implements the same trait
behind its own factory closure; orchestration, storage, workspace, and
completion semantics do not change (see §13).

`provider-claude` depends only on contract crates (`agent-runtime`,
`task-domain`, `scheduler`, `workspace`) plus std. No serde (the repo
carries none; the JSON result parser is hand-rolled and bounded), no
tokio, no SQLite, no Git.

## 2. Claude CLI integration mode

- Non-interactive machine-readable execution: `claude -p
  --output-format json [--permission-mode M] [--resume S] <prompt>`.
- Prompt travels as the final positional argv value (whole, never split,
  never shell-interpolated). Stdin stays closed.
- Environment inherited unchanged (provider auth lives there: subscription
  session, keychain helpers, `PATH`). The adapter neither strips nor
  injects environment.
- Observed CLI: `2.1.263 (Claude Code)` (see §4 for compatibility policy).

## 3. Authentication ownership

Provider-owned, end to end. The adapter never reads credential files,
never persists tokens, never logs them, never forwards them, and never
attempts to repair authentication. Detection does not probe auth (no
quota burn, no interactive-login risk). An unauthenticated session
surfaces at execution time as bounded `AuthRequired` failure evidence
(via a small documented stderr-marker table — the only textual fallback,
unknown-safe) or as generic `Failed`.

## 4. Installation detection

`detect(executable)`: explicit `PATH` directory scan (`find_on_path`,
executable-bit heuristic; spawn is the real check) → `<exe> --version`
with 10 s timeout and 4 KiB bound → tolerant `major.minor.patch` parse
(`2.1.263 (Claude Code)`, `2.1.263`, `claude 2.1.263` all parse).

- Unresolvable → `MISSING`. Unspawnable/blank probe → `UNKNOWN`.
- Compatible (major == 2, minor >= 1) → `AVAILABLE`.
- Other majors / older minors → `INCOMPATIBLE` (fail closed; schema
  drift additionally fails closed at parse time as `MalformedOutput`).
- No `AUTH_REQUIRED` status exists: the availability vocabulary is
  exactly the closed providers.md §9 set (no per-provider additions).

## 5. Machine-readable protocol

`--output-format json` returns one result object. Critical fields the
adapter trusts structurally: `type == "result"`, `subtype`,
`is_error`, `result` (non-blank), `session_id` (opaque bounded text).
Observed 2026-09-10 against CLI 2.1.263 (24 top-level keys including
`duration_ms`, `num_turns`, `total_cost_usd`, `usage{input_tokens,
output_tokens, ...}`, `modelUsage`, `permission_denials`; all except the
critical five ignored forward-compatibly).

- `is_error == true` or `subtype != "success"` → `Failed` (never evidence).
- Empty stdout (unexpected EOF), non-JSON, wrong shape, blank result →
  `MalformedOutput` (never trusted).
- Unknown non-critical fields are parsed and ignored; missing critical
  fields fail explicitly.

## 6. Capabilities currently exposed

Probed live from `claude --help` (unknown != supported; failed probe
degrades to adapter-implemented mechanics only):

```text
non_interactive (--print) ......... proven
structured_json (--output-format+json) proven
streaming (stream-json) ........... proven flag (unused by argv today)
session_resume (--resume) ......... proven flag, minimal argv support
session_control (--session-id) .... proven flag (adapter omits it on fresh runs)
permission_modes .................. proven flag (default: no override)
workspace_execution (cwd binding) . always (adapter-enforced)
cancellation (process-group kill) . always (adapter-implemented)
usage_reporting ................... when structured_json (per-result provenance)
```

## 7. Workspace binding

`ClaudeExecutor::bind(config, prompt, workdir)` — the workdir is the
attempt's lease path, supplied by orchestration (never chosen by the
adapter, never guessed). `spawn_and_supervise` refuses a missing
workdir with `WorkspaceBinding` before spawning: no fallback to the
primary checkout, agentd cwd, `$HOME`, or another task's workspace.
`schedule_in_workspace` rejects a `WorkspaceRequest` whose execution id
differs from the cycle's execution id before committing anything.

## 8. Session semantics

`Task != Execution != Attempt != Claude session` (ADR-0005). The CLI's
`session_id` is opaque provider state: it travels only inside
`evidence_ref` (`claude:session:<id>`) and the adapter-internal
`ParsedOutput`. It is never a Task/Execution/Attempt/Workspace identity,
never persisted, never completion authority. `--resume` argv support
exists (opaque bounded text); orchestration performs no auto-resumption
(DEFERRED — session continuation needs no provider-specific persistence).

## 9. Cancellation

`ClaudeExecutor::cancel()` sets a shared flag; the wait loop kills on
the next quantum. The kill targets only the owned child tree: direct
`kill` + `wait` (reap truth) + unix process-group `killpg` (every spawn
runs as group leader via `process_group(0)`), so forked grandchildren
die too instead of orphaning with capture pipes open (found live: an
orphaned `sleep` stalled drain joining for the full 60 s before the
group kill). Never `pkill`/`killall`/name matching. Already-exited
children are safe no-ops; the terminal outcome is always observed;
durable Task state is untouched (cancellation maps to
`AttemptOutcome::Cancelled`, never completion).

## 10. Failure mapping

| Signal | Canonical outcome |
|---|---|
| CLI missing (PATH/spawn) | `Failed(cli_missing …)` |
| Version incompatible | `Incompatible` at detect; production fast precheck is PATH-presence-only (adapter owns compatibility policy; structured execution/parser fails closed for incompatible behavior; bounded-fast version gate deferred, TASK-0051 H5) |
| Spawn failure | `CliMissing` / `SpawnFailed` |
| Deadline exceeded | `TimedOut` (tree killed) |
| Cancel flag | `Cancelled` |
| Exit 0 + valid success JSON | `WorkEvidence` (submittable, never completion) |
| Exit 0 + `is_error`/error subtype | `Failed` |
| Exit 0 + empty/non-JSON/wrong shape | `Failed(malformed_output …)` |
| Non-zero exit | `Failed` (exit code + bounded stderr; auth markers → `AuthRequired`) |
| Missing workdir | `Failed(workspace_binding …)`, pre-spawn |
| Quota/rate-limit prose | `Failed` with detail (no stable code exists; stays explicit, never success) |

Structured signals (spawn errors, exit status, JSON fields) classify
everything; the auth-marker table is the sole textual fallback.

## 11. Test strategy

- **Unit** (52 lib tests): version parsing/compat, argv construction +
  injection-hostility, capability derivation + emitted-flag coverage,
  JSON parser (malformed battery, escapes, surrogate pairs, depth bound,
  number classes), output translation (error subtypes, EOF, blank,
  session-ref validation, usage unknown-vs-reported), failure taxonomy
  (auth markers narrow + unknown-safe), config guards (bypass-mode
  refusal), prompt bounds + no-completion-vocabulary guard, tracker
  refcounting.
- **Process integration** (10 unix fixture tests, no quota): scripted
  executable doubles prove spawn/stdout/stderr/exit/cwd without the real
  CLI — structured success → canonical evidence incl. cwd proof,
  usage + provenance, malformed/empty/non-zero/auth-hint mapping,
  hang → `TimedOut` with prompt tree kill, cancel → `Cancelled`
  killing only the owned tree, tracker live-during-execution, hostile
  prompt argv-safety (`$(…)` never substituted, `/tmp/pwned` absent).
  Re-run 3× green; `ps` shows no leaked children.
- **Contract**: the same `ExecutionPort` seam serves `FakeExecutor`
  (G3 suites untouched, still green) and `ClaudeExecutor`; the binding
  tests drive both through `schedule_in_workspace`.
- **Real smoke** (`tests/smoke.rs`, `#[ignore]` + `RUNLITH_CLAUDE_SMOKE=1`):
  `claude -p --output-format json` with a no-tool marker prompt in a
  temp dir → `G5_SMOKE_OK` in 5.8 s, session ref captured, usage
  reported. Missing CLI/auth skips gracefully; deterministic CI never
  runs it. **Result 2026-09-10: PASS.**

## 12. Known limitations (not defects)

- Real-provider coverage is one smoke path; quota-bearing suites stay
  fixture-driven by design.
- `--resume` continuation is argv-ready but orchestration never
  auto-resumes (leases + attempts remain the durability story).
- Usage evidence is exposed (`UsageSample` + provenance) for the future
  Usage Ledger; no ledger, no routing, no cost math exists.
- Model selection is provider-native (no `--model` passed).
- Lingering-daemon caveat: on the natural-exit path, a double-forked
  daemon grandchild outliving the CLI while holding a pipe could delay
  capture joining. Timeout/cancel paths kill the tree first, so
  supervision timing never depends on grandchildren.
- `agentd`/`agentctl` expose no Claude endpoint yet: composition-root
  wiring of the concrete adapter into the daemon is follow-up work
  (G5 stays `in_progress`; see §15).

## 13. Second-provider structural check (§29)

`provider-codex` (or `provider-opencode`) next requires: a new adapter
crate implementing `scheduler::ExecutionPort` (+ its own failure
taxonomy, argv/process mechanics, and optionally `OwnerLiveness` via
the existing consumer port + `GitCli::with_liveness`), plus a factory
closure at the composition call site. No changes to `task-domain`,
`agent-runtime`, `scheduler`, `workspace`, `storage-sqlite`,
`OrchestrationService`, completion semantics, or the journal. Verified
mechanically: `cargo tree -p provider-claude` shows only contract
crates (no domain crate depends on it); `grep` shows no
`provider == "claude"` branching in core; the binding tests prove a
non-Claude executor (`StubExecutor`, `FailExecutor`) binds identically.

## 14. G4 follow-up disposition

- **F1 (cleanup `repository_path` caller-supplied) — RESOLVED.**
  Binding is proven per observed worktree shape, before any mutation:
  (a) present worktree → `GitPort::is_registered_worktree` (positive
  registration proof via `worktree list`, canonical comparison, backend
  errors fail closed): wrong-but-valid repo → `OwnershipMismatch`, lease
  stays `Ready`, worktree + branch intact, no event; correct-repo retry
  succeeds; (b) absent worktree + `CleanupPending` (legitimate
  crash-continuation: nothing left to register) → history binding: the
  passed repository must resolve the lease's immutable recorded base
  commit, else `OwnershipMismatch` — an unrelated wrong repo cannot, the
  true repo still can. `already_removed_noop` is deliberately unprobed (a
  `Removed` lease has no worktree to register; `Removed` is reachable
  only through the guarded finalization, and true-repo leaks still
  surface as `CheckoutFailed` corruption). Two regression tests
  committed (present-shape + absent-shape).
- **F2 (BranchRef Git-special chars) — RESOLVED as interpretation B.**
  `BranchRef` intentionally represents arbitrary valid/existing refs
  (observation + recording), so the generic type is not scope-restricted;
  instead the constructor now rejects every `git check-ref-format`
  forbidden shape plus every revision-expression shape (`~ ^ : ? * [
  \`, controls, `@{`, trailing `/ . .lock`, empty/dot-led components),
  so a `BranchRef` always names exactly one ref. Managed authority stays
  at the derivation boundary: slug-safe `branch_for_execution` plus the
  new `BranchRef::is_managed` (`agent/` scope) gate used by deletion
  paths. Regression tests committed.

## 15. Dependency evidence

- `cargo tree -p provider-claude`: `agent-runtime`, `task-domain`,
  `scheduler`, `workspace` only (plus their transitive deps). No
  reverse dependency from any domain crate.
- `cargo tree -p app-services`: unchanged shape (no provider-claude
  dependency — the binding is generic over `ExecutionPort`).
- No new external crates added (no network needed; `Cargo.lock`
  gains no new packages).

## 16. Follow-ups (not blockers)

1. `agentd` composition-root wiring + `agentctl` surfacing for real
   provider execution (needs IPC/business-contract decisions; G0
   contract is frozen, so this is a deliberate later task).
2. G5 exit review: one provider passes the contract; closing G5 needs
   the gate's explicit criteria check (second-provider addability is
   proven structurally in §13, not by a second implementation).
3. Daemon-grandchild capture caveat above (see §12).
4. Future-CLI drift: major != 2 reports `INCOMPATIBLE`; minor/patch
   schema drift fails closed as `MalformedOutput` with the raw shape
   in the test battery for diagnosis.
