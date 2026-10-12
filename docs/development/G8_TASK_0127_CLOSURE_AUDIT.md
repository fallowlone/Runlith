# TASK-0127 — Independent G8 Closure Audit

> Audit-only re-evaluation of the complete accepted G8 contract on current
> `main`. No production behavior was changed by this task.
>
> **Verdict: `G8_CLOSED`.**

All 28 preceding G8 tasks are merged to `main` (PRs #10–#23 plus the
pre-PR base), every acceptance claim re-tested against the audited tree
passes, and the full workspace suite is green (134/134, EXIT=0) with
`fmt` and `clippy -D warnings` clean. The audit found one minor hygiene
regression (three contributor-machine paths in the TASK-0152 probe doc,
no secrets), adjudicated non-blocking with a recorded remediation path;
one governance staleness (product identity), repaired inside this audit
commit per `VERSIONING.md`; and residual owner actions (CI billing, two
key revocations, live-quota exhaustion), none of which any G8 exit
criterion requires closed.

## 1. Audited identity

| Field | Value |
| --- | --- |
| Source main | `8e290b7373eaefe38485f61c5119ddcd1e53ae2a` (post-PR-#23 merge) |
| Audit branch | `task-0127-g8-closure-audit` |
| Audit worktree | `.claude/worktrees/task-0127-g8-closure-audit` (clean tree at audit time) |
| Host | macOS 27.0, arm64 |
| Rust | `rustc 1.95.0` |
| Toolchain workaround | `DEVELOPER_DIR=/Library/Developer/CommandLineTools` |
| Real provider | Claude Code 2.1.290 (weekly quota exhausted; see §7) |

The technical audit ran on the clean worktree above. Governance records
(this report, the gate file, the queue outcome, the version identity)
were written only after the technical verdict was fixed.

## 2. Authority and scope

The audit re-read the current G8 authority rather than accepting task
outcomes as proof:

- `AGENT.md` authority order and hard rules;
- `planning/roadmap.yaml` G8 (Verification; two planned capabilities);
- `planning/tasks/queue.yaml` G8 items and their exit criteria;
- ADRs 0030–0038 and 0041 (all `Accepted` on the audited tree);
- `docs/architecture/INVARIANTS.md` (INV-145 seams for TASK-0126);
- the G7 precedent `docs/development/G7_TASK_0120_FINAL_CLOSURE_REAUDIT.md`;
- actual implementation and executable tests on the audited tree.

Per TASK-0127 scope the audit re-tested the TASK-0121..TASK-0126
acceptance evidence, verifier independence, schema migration safety,
the side-effect recovery matrix, completion-authority binding, and the
G5/G6/G7 regression envelopes. The remaining G8 tasks (0128, 0134–0136,
0143–0162) were verified through merge ancestry plus their named
acceptance suites. Prior outcomes were treated as claims; several say
"integration into main is not claimed" because they were written on
branch time — the merge commits below are the integration record, and
the outcome prose staleness is noted as observation O-05, not a defect.

## 3. Merge ancestry — every G8 branch is in `main`

`git merge-base --is-ancestor <head> origin/main` passes for every G8
implementation branch: `task-0125-resuscitation`, `task-0126-…`,
`task-0134-…`, `task-0136-…`, `task-0145-…`, `task-0151-…`,
`task-0152-…`, `task-0154-…`, `task-0160-…`, `task-0161-…`,
`task-0162-…`, and the unpushed TASK-0157–0159 stack commits
(`bde8264`, `4e93509`, `3cf6269`, via the TASK-0125 merge #13).
PR chain on `main`: #10 (0153) → #11/#12/#19 (0154) → #13 (0125) →
#14 (0161) → #15 (0136) → #16 (0134) → #17 (0151) → #18 (0152) →
#20 (0162) → #21 (0145) → #22 (0160) → #23 (0126).
TASK-0155 merged as `5371415` (ancestor, verified).

## 4. Re-verification battery

Full workspace run on the audited tree (`cargo test --workspace
--locked --no-fail-fast`, proof `/tmp/task-0127-full.log`):
**134/134 suites green, 1422 passed / 0 failed, EXIT=0**.
`cargo fmt --all --check` clean;
`cargo clippy --workspace --all-targets --locked -- -D warnings` clean.
Fresh targeted re-runs (22 runs, all green) plus full-log citations
(`execution_*`) and live-smoke status:

| Suite (acceptance of) | Result |
| --- | --- |
| `app-services` completion_admission (0125/0161) | 17/17 |
| `app-services` orchestration (0125/0161) | 16/16 |
| `app-services` side_effect_recovery (0126) | 8/8 |
| `app-services` verification_execution (0162) | 11/11 |
| `app-services` requirement_binding (0157) | 10/10 |
| `app-services` review_service (0145) | 8/8 |
| `app-services` lib, `verification_levels` filter (0134) | 13/13 |
| `app-services` lib (all, incl. 0162 allowlist) | 64/64 |
| `storage-sqlite` durability (migration upgrades →27) | 32/32 |
| `storage-sqlite` side_effect_intents (0126) | 6/6 |
| `storage-sqlite` claim_verification (0125) | 3/3 |
| `storage-sqlite` observed_effects (0124/0144/0151) | 21/21 |
| `storage-sqlite` review_authority (0145) | 8/8 |
| `plan-domain` validation_rules (0158) | 58/58 |
| `policy` lib (0136 advisory, 0146, 0157 catalog) | 43/43 |
| `evaluation` lib (0144 selector, 0145 records) | 31/31 |
| `task-domain` lib (0123 requirements) | 59/59 |
| `plan-domain` lib (G7 envelope) | 14/14 |
| `workspace` lib (0126 machine) | 30/30 |
| `provider-claude` lib (G5 envelope) | 76/76 |
| `agentd` lib (0159 e2e path) | 75 passed / 1 ignored (live) |
| `product-version` lib | 6/6 |
| `agentctl` execution_binaries + execution_composition | 10/10, 8/8 |
| live smokes (`smoke`, `planner_real_provider_smoke`) | 0 passed / 1 ignored each (§7) |

## 5. Scope-item deep checks

**Completion-authority binding (0125/0161/ADR-0041).**
`mint_candidate_verdicts` has exactly one production caller
(`orchestration_service.rs:775`), positioned after
`revalidate_before_completion`; `append_claim_verification` has no
other production caller (all remaining callers are tests), so
candidate-only minting holds on `main`. Operator dispatch
(`dispatch.rs:203`) refuses candidate-routed tasks before admission
(0151-f13) and derives the current policy fingerprint from the
composition (0125-D1′); `verification_succeeded_under_policy` mints
nothing — it admits from durable state only. The strict rule-5
snapshot (`AdmissionSnapshot`, 0161) is covered by 17/17 admission
tests including the adversarial two-handle Stale test.

**Side-effect recovery matrix (0122/0126).** The §2 machine, schema 27
(edge-table trigger, audit trail, sticky escalation, active-slot
unique index), and crash-A/B/C reconcile all re-pass (8/8 recovery,
6/6 store, workspace suites 24/14/13/20/9/2). Dispatch errors map to
FAILED only on proven pre-dispatch refusal, else AMBIGUOUS; any
non-terminal intent refuses blind re-dispatch. The cleanup kind stays
reserved: TASK-0043 (G4, open) owns cleanup, and no 0126 exit
criterion requires cleanup dogfood, so the reservation is a disclosed
design boundary, not a gap.

**Migration safety.** Migrations `0017`–`0027` apply in order to schema
27; upgrade tests re-pass fresh: `schema_21_…_to_current_27`,
`schema_22_…_to_current_27`, v2 journal survival, v15 audit upgrade,
legacy-criteria backfill, pre-trigger repair, pre-column refusal.
No migration rewrites history or fabricates evidence.

**Verifier independence (0144/0145/0150/0157–0160).** The evaluator
consumes bound requirements via `from_requirement` at the
selection/evaluate/revalidate sites, and the production candidate path
uses `from_requirement_under_policy` (`execution.rs:1092`, 0151-f7);
the 0159 interop proof re-passes. Review authority re-passes 8/8 store
+ 8/8 service; presence-gated, DRAFT-only binding re-passes (0157
catalog in `policy` lib, 0160 boundary/service/store/IPC tests).
`crates/ipc-contracts` contains no binding fields (0160 claim holds).
The 0145 disclosures check out in code: `admission_ref` is bound
opaquely with the missing-registry rationale documented
(`review_service.rs:247`); the candidate path never consults
`ScopedReviewed` (assessment-path enforcement genuinely out of scope,
as disclosed); B2 stays selectable-but-blocked per exit criteria.

**0134 → 0162 chain.** `required_kinds_for_level` maps
V1→ProcessExit, V2→TestSuite, V3→TestSuite+SecurityCheck
(`verification_levels.rs:329`), and the 0162 runner persists exactly
`EffectKind::ProcessExit` evidence, so selected levels execute through
the TASK-0123 obligation path as 0134's exit criteria require. The
absent orchestrator-cycle call site is disclosed in both tasks' scopes
("available for that follow-up") and required by no exit criterion —
recorded as orchestrator follow-up, not a finding.

**G5/G6/G7 regression envelopes.** Provider contract + fixture suites
green (`provider-claude` lib 76/76, `planner_port_contract` in the
full run); policy/runner/security suites green (`policy` 43/43,
`native_runner` 4/4, `g6_security_integration` in the full run);
planning suites green (`validation_rules` 58/58, `plan-domain` lib
14/14, `planning_service` in the full run).

## 6. Findings and adjudication

**F-01 (minor, non-blocking): contributor-machine paths in the 0152
probe doc.** The exact runbook command
`git grep -n -I -E '/Users/[A-Za-z0-9_]'` returns 3 hits on the audited
tree, all in `docs/development/G8_TASK_0152_PROVIDER_AUTH_PROBES.md`
(lines 26, 92, 108; `/Users/artemmac/…` — username plus repo layout,
no secrets; fresh `gitleaks dir`: no leaks, ~128.61 MB scanned).
TASK-0154's "returns nothing" was true when verified (commit `717555f`
predates the 0152 merge `1347e11`), so no historical claim is falsified;
the drift arrived with PR #18. Adjudication: no G8 exit criterion
requires the invariant to hold continuously on `main`; the
runbook re-checks exactly this grep at owner-executed publication, and
pre-publication hygiene is already a recorded follow-up (0153/0154
outcomes). Required fix: redact the 3 paths under that follow-up before
any public export. Not a closure blocker.

**F-02 (governance, repaired in this commit): stale product identity.**
`product/version.toml` sat at `0.1.0-dev.task0159` although eleven
later tasks completed, against the VERSIONING.md rule (every completed
task advances once). Repaired here by advancing to `task0127`
(`VERSIONING.md` expressly permits an audit advance; G7 precedent).
`product-version` 6/6 still green; `gate` stays `G8`, pinned to the
queue's `current_gate` by test. Not a blocker.

**Residual owner actions (not blockers — required by no G8 exit
criterion):** CI billing (the `verify` check on PR #23 FAILED with 0
steps in 11 s — no runner ever assigned; merges #10–#23 rest on local
gates, which repo policy accepts as Task evidence); revocation of the
2026-09-25 exposed OAuth token (0152, treated as exposed per the ADR)
and of the `nvapi-` history keys (0154, history unrewritten per scope);
live-provider quota (see §7).

**Post-audit confirmation (2026-10-07):** the owner confirmed
revocation of the 2026-09-25 exposed OAuth token (0152) and of the
`nvapi-` history keys (0154). CI billing remains open (owner action,
deferred).

**Observations (no action required for closure):** O-01 queue
`completed_through: TASK-0150` is legacy-stale (untouched by the whole
G8 series; out-of-order completion voided its semantics — recommend
deprecation, orchestrator-owned). O-02 outcome prose saying
"integration into main is not claimed" is branch-time-frozen; the §3
merge ancestry supersedes it. O-03 TASK-0043 (G4 cleanup), TASK-0163 /
TASK-0164 (G6 follow-ups) stay open in closed gates — the queue's
standing convention for deferred work, none of it G8-scoped. O-04 the
G5 live smoke misclassifies quota-exhaustion as a non-environment
failure (it panicked instead of passing the environment through) — a
G5-test robustness nit, pre-existing, out of G8 scope. O-05 LICENSE /
NOTICE / LICENSE are byte-stable since PR #10 and ratification
(`5ba1af1`) precedes that merge, so the 0153 merge gate and
byte-identity evidence inherit exactly.

## 7. Live-provider evidence

No G8 exit criterion requires a live call: every G8 acceptance suite is
deterministic with live smokes `#[ignore]`d behind
`RUNLITH_CLAUDE_SMOKE=1`. The audit attempted the bounded G5 executor
smoke once anyway (`-p provider-claude --test smoke -- --ignored`,
180 s timeout, one marker prompt, no tools): FAILED in ~2 s with
`claude exited with code Some(1)`. Direct reproduction outside the
product (`claude --print …`) shows the cause: "You've hit your weekly
limit · resets Oct 8" — purely environmental, same class as the G7
audit's account-limit block. The planner smoke was not attempted: its
own header documents it RED on D5 (G7-owned), and spending quota to
re-prove a documented RED is unjustified. G8 live-provider posture is
therefore: deterministic fixture path green (contract suites in the
full run), live call environmentally blocked and not required.

## 8. Independent second pass

After the full run, the audit re-executed the §4 targeted battery from
the same clean tree (all green, counts above), re-ran the 0154 hygiene
greps plus a fresh `gitleaks dir` (F-01 scoped, no leaks), re-derived
the verdict-minting call graph and the 0134→0162 obligation chain by
adversarial `grep` (single production minter; level→kind→effect kinds
match), and re-checked every merge ancestor and ADR status line. No
second-pass item contradicts the §4–§7 evidence.

## 9. Final verdict

**`G8_CLOSED`.** The accepted G8 contract — the Observed Effect /
Claim Verification capability and the Side-Effect Execution Protocol
capability per `roadmap.yaml`, with all 28 task exit criteria — holds
on `main` `8e290b7`, backed by current code and 134/134 executable
suites. Required follow-ups (orchestrator-owned, none blocking):
redact the F-01 paths under pre-publication hygiene; queue the
verification auto-invocation product decision + wiring follow-up to
TASK-0162; owner fixes CI billing and confirms both key revocations.
 Hermetic 0155 regression
tests (principal isolation, support-root fail-closed) re-pass inside
the full run.

