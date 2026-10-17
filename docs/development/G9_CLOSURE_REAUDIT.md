# G9 Closure Re-Audit (TASK-0178)

> Independent closure re-audit of G9 (Context Engine + Continuity)
> after the TASK-0177 remediation, from a fresh context.
> Authority: `AGENT.md`, `docs/development/G9_CLOSURE_AUDIT.md`
> (blocker B1, §5 residuals, §6 Entity Graph condition),
> `planning/roadmap.yaml` G9, `planning/tasks/queue.yaml`
> TASK-0170..TASK-0178. Audit only: no production behavior was
> changed by this task.
>
> Verdict: **`G9_CLOSED`**. On this verdict the roadmap gate
> advances to G10 and G10 work may be registered.

## 1. Method

- All executable evidence was produced in a pristine scratch clone
  (`git clone` of this repository, checkout
  `298696afcd13ca2b220ff0c3dc182b4cd2f8469d`, the post-TASK-0177
  `main` head), never in the working tree. The audited Rust code is
  identical to merge commit `452da5f`; `298696a` changed only
  `planning/tasks/queue.yaml` and `product/version.toml`.
- Falsification first: blocker B1 was reproduced in its red state on
  the pre-remediation base `a19a3e5`, then shown green through the
  shipped `agentd`/`agentctl` binaries on the audited head.
- Every claim below carries one proof: a reproducing command with
  its observed result, or the red-then-green pair. Search output
  alone was not accepted as evidence; proving code and tests were
  opened.

## 2. Blocker B1 — red, then green

Red (base `a19a3e5`, scratch clone): `IPC_PROTOCOL_VERSION_CURRENT`
is 7 and `SUPPORTED_PROTOCOL_VERSIONS` is `[0..=7]`
(crates/ipc-contracts/src/lib.rs:51), so
`supports_takeover_continuation` is false for every negotiable
version and TAKEOVER refuses by construction — the B1 state recorded
in `G9_CLOSURE_AUDIT.md` §3. The base contract suite passes 26/26
while encoding that refusal.

Green (audited head `298696a`): protocol V8 is assigned additively
(`IPC_PROTOCOL_VERSION_V8 = 8`, current = 8, supported `[0..=8]`),
every older surface predicate now also accepts V8, and
`supports_takeover_continuation(v) = v >= 8`
(crates/ipc-contracts/src/lib.rs:213). The daemon refuses TAKEOVER
continuation and all four TAKEOVER operations below V8 before any
service access (`takeover_gate`,
bins/agentd/src/human_decision.rs:175; decide path gate at :66).
Production human-decision requests offer `Takeover`
(crates/app-services/src/planning_service.rs:1566, :1886). The
daemon spawns a shutdown-safe 1s expiry tick that reconciles
without a service call (bins/agentd/src/lib.rs:553, spawned :618).

## 3. Claim-by-claim gate evidence

| # | Claim | Proof (command → observed result) |
|---|---|---|
| C1 | V8 negotiates; v0–v7 still refuse TAKEOVER | `cargo test -p ipc-contracts takeover` → 3/3 ok (`v8_preserves_every_older_surface_and_adds_takeover`, `takeover_continuation_requires_v8_or_higher`, `v0_through_v7_still_refuse_takeover_after_v8_assignment`) |
| C2 | Below-V8 refusal is typed and pre-service | `cargo test -p agentd --lib takeover` → 4/4 ok, incl. `takeover_decide_refuses_below_v8_without_touching_service`, `takeover_operations_refuse_below_v8_without_touching_service` |
| C3 | Shipped binaries negotiate the TAKEOVER surface | `cargo build --locked --bin agentd --bin agentctl` → ok; `cargo test -p ipc-control` → 53/53 ok (negotiation incl. `g9_takeover_default`) |
| C4 | Assume → append → render → return cycle through shipped binaries | `takeover_assume_append_render_return_cycle_through_shipped_binaries` → ok (task BLOCKED while ACTIVE, durable record COMPLETED after return, consumed-HDR replay refuses) |
| C5 | Refusal matrix refuses typed through shipped binaries | `takeover_apply_refuses_active_run_…`, `takeover_apply_refuses_stale_decision_…`, `takeover_return_refuses_stale_lease_…` → all ok; refused attempts leave task/record state unchanged |
| C6 | TAKEOVER decides on a V8 CLI session | `human_decision_cli_takeover_decides_on_v8` → ok |
| C7 | Expiry tick reconciles with no service call | `takeover_expiry_tick_reconciles_without_a_service_call` → ok (record EXPIRED, task unparked to RUNNING, no `takeover-reconcile` invoked) |
| C8 | TASK-0172 exit criterion met as written | C4+C5+C6+C7: a human can assume and return authority mid-flight with durable evidence; stale/conflicting attempts refuse typed |
| C9 | No regression in the shipped binary surface | `cargo test -p agentctl --test execution_binaries` → 17/17 ok |

## 4. TASK-0170..0175 evidence re-check

| Task | Verdict | Re-executed evidence (audited head) |
|---|---|---|
| TASK-0170 Provenance Envelope | PASS | context-engine 28/28; `context_selection` 4/4 |
| TASK-0171 Epochs/checkpoints/history | PASS | continuity 18/18 |
| TASK-0172 TAKEOVER continuation | PASS (was FAIL/B1) | takeover domain 7/7; §3 C1–C9 close the control-plane gap |
| TASK-0173 Finder ids/ranking | PASS | repo-finder/finder-local suites green; `rf2.<rev>.<start>.<end>.<hash16>.<hexpath>` shape intact (crates/repo-finder/src/model.rs:389) |
| TASK-0174 Finder index + INV-008 filter | PASS | finder suites green; `RedactingFilter` posture unchanged |
| TASK-0175 Planner evidence boundary | PASS | `planning_service` 70/70 |

## 5. Non-blocking residuals — disposition unchanged

| Residual (from G9_CLOSURE_AUDIT §5) | State at audited head |
|---|---|
| INV-008 pattern-screen fail-open shapes (bare high-entropy tokens, JWTs, lone fragments) | Still documented as accepted residuals (crates/app-services/src/secret_redaction.rs:14); whole-secret attempt details still drop to identity-only. Follow-up hardening, not a blocker. |
| Planner prompt lacks explicit untrusted-data label | Unchanged; provider output still parsed as untrusted data (adapters/provider-claude/src/planner.rs:6). Wording follow-up. |
| Truncation marker mismatch: code `"\n[truncated]"` (crates/plan/src/resolved_evidence.rs:28) vs policy `` `…[truncated]` `` (docs/privacy/PLANNER_EVIDENCE_RESOLUTION_POLICY.md:69) | Unchanged doc/code wording mismatch; behavior fail-closed. Doc fix follow-up. |
| TASK-0173 hash16 (64-bit prefix) collision/staleness trade | Unchanged, previously accepted token-cost trade with recorded risk. |

No residual has worsened, and none has been silently promoted to a
blocker. TASK-0177 touched no finder, context, continuity, or
planner-evidence code, so the §5 posture is inherited intact.

## 6. Conditional Entity Graph

**Condition UNMET — compliantly.** A fresh search of `docs/`,
`planning/`, and `crates/` finds no measured entity-ambiguity or
relationship-heavy retrieval failure; the finder's measured failures
were lexical and remain closed at 16/16 relevance/support
(docs/development/TASK_0147_FINDER_BENCHMARK.md). Per the roadmap's
conditional wording (`ENTITY_GRAPH: conditional`), not building the
graph is compliant and does not block G9.

## 7. Verification totals (auditor, scratch clone, `298696a`)

- `cargo fmt --all --check`: clean.
  `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  clean.
- Shipped-binary takeover set: 6/6 ok; full `execution_binaries`
  17/17 ok. Contract/gate suites: ipc-contracts 3/3 (takeover),
  agentd lib 4/4 (takeover), ipc-control 53/53.
- Domain/service re-checks: context-engine 28/28, continuity 18/18,
  takeover 7/7, context_selection 4/4, planning_service 70/70,
  finder packages green.
- Full workspace `cargo test --workspace --locked --no-fail-fast`
  ran to completion in the scratch clone with no `FAILED` or
  failure-summary lines in the captured results; this matches the
  TASK-0177 post-merge record of 1864 passed / 0 failed on identical
  Rust code. The historical `execution_recovery` load flake did not
  appear.

## 8. Verdict

**`G9_CLOSED`.** Blocker B1 is closed through the shipped control
plane (V8 negotiation, TAKEOVER assume/return cycle, refusal
matrix, expiry tick) with executable shipped-binary evidence; the
TASK-0170..0175 evidence re-verifies; every §5 residual remains
non-blocking; the Conditional Entity Graph condition is confirmed
unmet and non-blocking. The roadmap gate advances to G10, and G10
may be registered.
