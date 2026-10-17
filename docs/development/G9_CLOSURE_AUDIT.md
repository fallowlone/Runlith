# G9 Closure Audit (TASK-0176)

> Independent closure audit of G9 (Context Engine + Continuity) from a
> fresh context. Authority: `AGENT.md`, `INVARIANTS.md` (esp. INV-008,
> INV-043), ADR-0010/0011/0029/0042/0043, `planning/roadmap.yaml` G9,
> `planning/tasks/queue.yaml` TASK-0170..TASK-0175.
> Audit only: no production behavior was changed by this task.
>
> Verdict: **`G9_REMEDIATION_REQUIRED`** (blocker B1). G9 stays
> `in_progress`; G10 must not start. Remediation is assigned to
> TASK-0177; a re-audit follows it.

## 1. Method

- Independent read-only auditor re-tested every TASK-0170..0175 exit
  criterion against current `main` (`a19a3e5`), opening the proving
  code and tests; search output alone was not accepted as evidence.
- Orchestrator independently re-verified the decisive blocker (B1) by
  direct inspection and re-ran the full verification suite on `main`:
  `cargo fmt --all --check` clean, clippy `-D warnings` clean, full
  workspace 1852 passed / 1 failed (§4).

## 2. Per-task verdicts

| Task | Verdict | Decisive evidence |
|---|---|---|
| TASK-0170 Provenance Envelope | PASS | `ProvenanceEnvelope` (crates/context-engine/src/model.rs:497), durable `explain()` (port.rs:41), restart test (crates/app-services/tests/context_selection.rs:132), SECRET refusal (selection.rs:115). Suites: context-engine 28/28, context_selection 4/4. |
| TASK-0171 Epochs/checkpoints/history | PASS | Uncommitted-checkpoint refusal (`NotCommitted`, adapters/storage-sqlite/src/continuity_store.rs:853), history cap 100 + exact-ref reads (crates/continuity/src/history.rs:12), policy-owned retention (model.rs:383). Suites: continuity 18/18, service 7/7. |
| TASK-0172 TAKEOVER continuation | **FAIL — blocker B1** | Service layer complete (crates/takeover, apply/append/render/reconcile in human_decision_service.rs:167), but the shipped control plane cannot carry it (§3). |
| TASK-0173 Finder ids/ranking | PASS (caveat) | `rf2` hash16 ids + symbol ranking (crates/repo-finder/src/model.rs:388; adapters/finder-local/src/lib.rs:378); benchmark R6/R7 (docs/development/TASK_0147_FINDER_BENCHMARK.md:363). Caveat: staleness binding is a 64-bit prefix (model.rs:521) — accepted token-cost trade, recorded as known risk. |
| TASK-0174 Finder index + INV-008 filter | PASS (wording caveat) | Indexed finder (adapters/finder-local/src/index.rs:209), `RedactingFilter` default (crates/app-services/src/finder_service.rs:106), R8/R9 benchmark (TASK_0147_FINDER_BENCHMARK.md:434). Caveat: "cannot reach output by construction" overclaims a pattern screen with documented fail-open residuals (secret_redaction.rs:14); the TASK-0167 contracted API is met. Reword + hardening follow-up, not a blocker. |
| TASK-0175 Planner evidence boundary | PASS (residuals) | Trusted resolution only (crates/app-services/src/planning_service.rs:1194), bounds pre-spend (crates/plan/src/resolved_evidence.rs:16), `decided_by` never crosses (planning_service.rs:1268; test planning_service.rs:4866), FLOW-013 recorded. Suites: planning_service 70/70. Residuals in §5. |

## 3. Blocker B1 — TAKEOVER unreachable through the shipped control plane

TASK-0172's objective is TAKEOVER "through the shipped versioned
control plane"; its exit criterion is that a human can assume and
return authority mid-flight. On current `main`:

1. No negotiable session can decide TAKEOVER:
   `supports_takeover_continuation` requires version >= 8
   (crates/ipc-contracts/src/lib.rs:196) while
   `IPC_PROTOCOL_VERSION_CURRENT` is V7 and supported versions are
   0..=7 (crates/ipc-contracts/src/lib.rs:55). The daemon therefore
   refuses by construction (bins/agentd/src/human_decision.rs:58);
   the shipped binary test proves refusal, not capability
   (bins/agentctl/tests/execution_binaries.rs:876).
2. `apply_takeover` / `append_takeover` / `render_handoff_packet` /
   `reconcile_takeover_expiry` have no IPC or agentctl exposure;
   they exist only in the service and its tests.
3. No production human-decision request offers TAKEOVER: creation
   sites offer `[Replan, Abort]` / `[Resume, Replan, Abort]`
   (planning_service.rs:1563).

The TASK-0172 queue outcome re-labels these as later rollout
decisions; that is a post-hoc scope reduction, not satisfaction of
the stated exit criterion. Assigned to **TASK-0177**.

## 4. Verification totals (orchestrator, current main)

- `cargo fmt --all --check`: clean. Clippy `-D warnings`: clean.
- Full workspace: **1852 passed, 1 failed**. The single failure,
  `concurrent_resumes_converge_on_exactly_one_attempt`
  (bins/agentd/tests/execution_recovery.rs), passes standalone in
  2.16s and is the known load-induced `execution_recovery` flake
  already recorded against TASK-0171; not a regression and not a
  closure blocker.

## 5. Non-blocking residuals (recorded, follow-up wording/hardening)

- INV-008 absolute wording (TASK-0174/0175): pattern-screen residuals
  (bare high-entropy tokens, JWTs, lone fragments) can cross;
  whole-secret attempt details do drop to identity-only and the
  `password=hunter2` redaction is test-pinned.
- Planner prompt renders resolved content as readable material
  without an explicit untrusted-data/not-instructions label
  (adapters/provider-claude/src/planner.rs:799; cf. INV-038/INV-093).
- TASK-0175 doc mismatches: truncation marker `\n[truncated]` (code)
  vs `…[truncated]` (policy §3); policy `redact_line` vs code
  `redact_lines`; "resolution never fails the call" comment is
  inaccurate for backend errors (fail-closed via `?`, safe).
- TASK-0173 hash16 collision risk as noted in §2.

## 6. Conditional Entity Graph

**Condition UNMET.** No measured entity-ambiguity or
relationship-heavy retrieval failure exists in `docs/`, `planning/`,
or `crates/`. The finder's measured failures were lexical and were
closed by TASK-0173/0174 at 16/16. Per the roadmap's conditional
wording, not building the graph is compliant; it does not block G9.

## 7. Verdict

**`G9_REMEDIATION_REQUIRED`.** The context/continuity/finder core is
proven on current `main`. The gate cannot close while a roadmap
capability (TAKEOVER) is unreachable through the shipped control
plane. Next: TASK-0177 remediation, then a G9 closure re-audit.
