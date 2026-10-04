# Security Architecture Audit — 2026-10-03

Read-only independent review. Scope: full workspace (18 crates, 10
adapters, `agentd`, `agentctl`). Method: module-by-module evidence
gathering against the architecture contracts in
`docs/architecture/INDEX.md`, with mechanical verification of each
claimed invariant. Baseline commit `5362f6e`
(`docs/init-deep-agent-guidance`).

## Verdict

No Critical findings. No High findings. 8 Medium, 11 Low.

The four core invariants hold mechanically, not just by convention:

1. **Completion authority** — only an independent `EvaluationPort`
   verdict routes a Task to COMPLETED, and only through VERIFYING
   (`task/src/state.rs` transition table; `orchestration_service.rs`
   `finish()` chain).
2. **Secret separation** — SQLite stores only opaque `SecretRef` ids
   and `SecretBinding` records; values live in the OS credential
   backend and cannot cross the module boundary.
3. **Fail-closed isolation** — `ProviderLaunch::Unattached` is the
   default and refuses to spawn; required isolation cannot degrade to
   native execution.
4. **Prompt as evidence, never authority** — bounded prompt
   construction with explicit truncation, and a test asserting the
   prompt never grants completion vocabulary.

## Findings register

| # | Module | Severity | Finding |
|---|---|---|---|
| F1 | 3 | Medium | Stuck-task recovery gap: workspace leases have `reconcile_existing`; mid-chain task states (SUBMITTED/AWAITING_REVIEW/VERIFYING/RETRY_READY) have no sweep or operator resume command |
| F2 | 3, 6 | Medium | `EvaluationPort::revalidate_before_completion` defaults `Ok(())` — fail-open |
| F3 | 3 | Low | FAILED/CANCELLED have no incoming transition edges (deliberate design debt, documented) |
| F4 | 3 | Low | Correlation/causation ids (`op-{now}-{counter}`) can collide across daemon restarts (clock regression + counter reset) |
| F5 | 4 | Medium | Provider error taxonomy collapses rate-limit/quota/5xx into `Failed` — indistinguishable from hard failure, retry amplification risk |
| F6 | 4 | Medium | `bind_trusted_host` reachable from production builds; convention + tests only |
| F7 | 4 | Low | Kill is SIGKILL-only; no SIGTERM→grace→SIGKILL escalation |
| F8 | 4 | Low | Provider output becomes durable journal evidence and human-review input — indirect prompt injection residual (document in THREAT_MODEL.md) |
| F9 | 4 | Low | `--resume` session binding is composition responsibility; no cross-task guard |
| F10 | 5 | Medium | Journal append-only is a code convention — no triggers, no hash chain; direct DB write access rewrites history undetectably |
| F11 | 5 | Medium | Production `unwrap()`/`expect()` in journal decode paths (`journal_store.rs:413-460`, `:779`; `approved_proposal_codec.rs:362-372`) — panic-capable on reorder |
| F12 | 5 | Low | `path_security.rs:191` non-unix `validate_parent_metadata` is a no-op stub |
| F13 | 5 | Low | Keychain error mapping is coarse; locked vs missing vs I/O failures indistinguishable |
| F14 | 5 | Low | Journal `seq` is storage-local ordering, not a domain contract |
| F15 | 6 | Medium | `CandidateIntegrityAssessment::validate` — invocation digest + policy fingerprint pairing needs explicit assertion against the recorded candidate |
| F16 | 6 | Low | Observed-effect `restore` validates consistency but does not fingerprint the payload; DB mutation passes restore if types match |
| F17 | 7 | Medium | `agentd` background spawn (`lib.rs:396`) detaches `std::thread::spawn` with no join-handle registry |
| F18 | 7 | Medium | Candidate-verification loop uses 10ms sleep-poll (`execution.rs:2650`); deadline overshoot under load |
| F19 | 7 | Low | Some `tokio::spawn` sites (`lib.rs:699`, `:793`) do not retain `JoinHandle` |
| F20 | 7 | Low | `plan_graph` BFS has no node-count bound; unbounded graph = unbounded memory |
| F21 | 4 | Low | Provider host probes (`claude --version`, `--help`, `auth status`) spawn the configured executable unconfined with the daemon's inherited environment (added 2026-10-04, see addendum) |

## Priority remediation

**P0 — evidence integrity (F10, F2):**

```sql
CREATE TRIGGER event_journal_no_update BEFORE UPDATE ON event_journal
BEGIN
  SELECT RAISE(ABORT, 'event_journal is append-only');
END;
CREATE TRIGGER event_journal_no_delete BEFORE DELETE ON event_journal
BEGIN
  SELECT RAISE(ABORT, 'event_journal is append-only');
END;
```

Migrations 0004/0005/0012 already rebuild `event_journal`, so adding
triggers there is mechanical. For evidence-grade integrity, add
`prev_hash`/`row_hash` columns
(`sha256(seq || event_type || task_id || payload || prev_hash)`) and
verify the chain on open; a break is `StorageError::Corrupt`.

`revalidate_before_completion` default becomes fail-closed:

```rust
fn revalidate_before_completion(
    &self,
    _request: &EvaluationRequest,
) -> EvaluationResult<()> {
    Err(EvaluationError::RevalidationNotImplemented)
}
```

In-tree evaluators already override the hook, so this is a no-op for
them and a tripwire for future implementers.

**P1 — provider resilience (F5):** add a transient variant so rate
limits and 5xx are distinguishable from hard failure:

```rust
pub enum ClaudeFailure {
    // existing variants...
    Transient { retry_after: Option<Duration>, detail: String },
}
```

Map `ClaudeFailure::Transient` to a submittable, retryable
`AttemptOutcome` in `scheduler/src/interpret.rs`, with bounded backoff
owned by the scheduler (never inside the adapter).

**P1 — typed decode errors (F11):** replace the four
`expect("plan scope presence checked above")` sites and the
`approved_proposal_codec` unwraps with:

```rust
let task_id: String = row.get("task_id")
    .ok_or_else(|| StorageError::Corrupt("journal row missing task_id".into()))?;
```

**P2 — composition hygiene (F6, F15, F17, F18, F1, F16):**

- F6: gate `bind_trusted_host` behind `#[cfg(any(test, feature = "trusted-host"))]` or a `debug_assert!(cfg!(test))` guard.
- F15: `assert_eq!(assessment.candidate_digest, recorded_digest)` inside `CandidateIntegrityAssessment::validate`.
- F17: retain join handles in a `Mutex<Vec<JoinHandle<()>>>` registry; join all on daemon shutdown.
- F18: replace the sleep-poll with `wait_timeout` or a condvar.
  - **Disposition: mitigated, accepted.** The loop sleeps `min(remaining, 10ms)`,
    so the 30s deadline cannot overshoot by a quantum. Residual cost is at most
    100 wakeups/s for 30s and up to 10ms late exit detection. `std::process::Child`
    has no `wait_timeout`, and every other child wait in the tree (`provider-claude`,
    `git-cli`) uses the same bounded `try_wait` poll. A blocking wait would need a
    new dependency or a pid-based kill path. If ever wanted, add it once in the
    runner adapter (a spawn handle with `wait_timeout` and `kill`) and migrate all
    call sites together; that is a refactor, not an F18 fix.
- F19: closed in 6135b80 (`serve_streams` aborts and awaits the acceptor task).
- F12: closed by failing closed. Non-unix builds compile `path_security_unsupported.rs`, which refuses every durable-store open. Windows ACL admission is not implemented; a Windows `agentd` cannot open its durable store until it is.
- F8: closed (documented) as threat-model vector A9; no code change.
- F9: closed (documented), latent. No production caller sets a resume session; the binding duty is documented on `with_resume_session` and in the threat model.
- F14: closed (documented). `EventId`, `JournalEvent.seq` and `ARCHITECTURE.md` state that `seq` is storage-local.
- F13: closed (`SecretBackendError` gains `Locked`, `Failure`, `Corrupt`; keychain adapter maps them distinctly).
- F1: add a fail-closed reconciliation sweep plus an operator resume command that routes only through validated transitions.
- F16: append a fingerprint (hash of kind + pinned fields) to the observed-effect record and verify it in `restore`.

**P3 — hardening (F4, F7, F8, F9, F12, F13, F14, F19, F20):**
boot-unique instance id in correlation ids; SIGTERM→grace→SIGKILL
escalation; THREAT_MODEL.md entry for provider-output-as-evidence;
cross-task `--resume` guard doc note; implement or fail-closed the
non-unix parent validation; distinct keyring error variants; document
`seq` as storage-local; retain `JoinHandle` for all spawns;
`MAX_PLAN_NODES` bound on plan-graph traversal.

## Addendum — 2026-10-04

**F21 (Low, module 4).** Three host-side probes run the configured Claude
executable directly through `std::process::Command`, with no
`env_clear`, no isolation backend and the daemon's working directory:

- `detect::probe_version` (`adapters/provider-claude/src/detect.rs:193`),
  `claude --version`;
- `capabilities::probe_help` (`adapters/provider-claude/src/capabilities.rs:127`),
  `claude --help`;
- `authentication::probe` (`adapters/provider-claude/src/authentication.rs:140`),
  `claude auth status`.

Each probe has fixed argv, no shell, null stdin, bounded stdout and a short
timeout (providers.md §2, §9a). The child still inherits every daemon
variable, including any credential the operator exported, and the full
host filesystem. The executable is operator-configured, not caller-chosen,
so this is not an escalation path for an IPC peer. The concern is
defense in depth: a replaced or compromised CLI binary runs once per launch
outside the sandbox with the daemon's environment.

Planned change: ADR-0039 P5 removes the `auth status` probe from the
production path (kept only for the feature-gated `trusted-host` binding).
The `--version` and `--help` probes stay on the production path; clearing
their environment to the child allowlist is open hardening work, not
scheduled. The Verdict counts above describe the 2026-10-03 baseline and
do not include F21.

## Verification performed

- Transition table exhaustively tested in `task/src/state.rs`
  (matrix tests, `only_verifying_reaches_completed`,
  `failed_and_cancelled_have_no_incoming_edges`).
- `PRAGMA foreign_keys = ON` verified at startup; daemon refuses to
  boot if inactive (`storage-sqlite/src/migration.rs:217-220`).
- All journal queries parameterized (`?1` placeholders).
- Adversarial test
  `r1_h1_unattached_provider_launch_refuses_to_spawn` proves the
  fail-closed launch default.
- Provider prompt test `prompt_never_grants_completion_authority`.
- No `DELETE`/`UPDATE`/`DROP` on `event_journal` outside versioned
  migrations.
- All `unwrap()`/`expect()` in `crates/task`, `crates/policy`,
  `crates/app-services/src` confirmed inside `#[cfg(test)]` modules.

## Uncertainty

- `effect-capture-local` candidate snapshot paths were sampled, not
  exhaustively traced.
- Live provider behavior (real Claude CLI) is untested; deterministic
  fixture tests do not establish live-provider support.
- The 22-migration chain was spot-checked for the journal tables;
  earlier migrations were not line-audited.
