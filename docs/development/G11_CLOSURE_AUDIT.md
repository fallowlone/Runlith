# G11 Closure Audit (TASK-0196)

> Closure audit of G11 (Background daemon), TASK-0188 through
> TASK-0195, executed inline by the implementing agent (weaker than
> a fresh-context auditor; the owner decides whether that suffices
> before merge). Authority: `AGENT.md`, ADR-0045,
> `planning/roadmap.yaml` G11, `planning/tasks/queue.yaml`
> TASK-0188..TASK-0196. Audit only: no production behavior was
> changed by this task beyond disclosed test-fixture pins.
>
> Verdict: **`G11_CLOSED`**. On this verdict the roadmap gate
> advances to G12.

## 1. Method

- Every claim from the G11 plan was checked against code and
  executable evidence in this session, not self-reports alone.
- Falsification: on the pre-G11 base (`main` at `4e041fb`), no
  `background` crate, no schema v35, and no `IPC_PROTOCOL_VERSION_V10`
  exist — every G11 claim is red on base by construction and green
  on this branch (`task-0188-g11-background-daemon`).
- Gate record: [G11_CLOSURE_GATE.md](./G11_CLOSURE_GATE.md).

## 2. Claim verification

| Claim | Evidence |
|---|---|
| Only AUTHORIZED routines run in background; PROPOSED never due | Domain eligibility predicate (B-5); `background` model 7/7; service `unauthorized_routine_never_ticks` |
| Author cannot self-authorize (INV-029 analogue, B-2) | Domain `SelfAuthorization`; service tests 4/4; shipped CLI refusal `self_authorization` in binary cycle |
| Budgets are hard caps; exhaustion is terminal (B-4) | Domain completion/exhaustion tests; failure matrix `budget_exhaustion_is_terminal` |
| Restart never re-executes in-flight runs (B-9) | `reconcile` marks RUNNING → INTERRUPTED; failure matrix `crashed_mid_run_is_interrupted_never_reexecuted`; agentd reconciles on startup |
| Draining refuses new runs (B-8) | Failure matrix `draining_refuses_new_runs`; service `is_draining` |
| Durable across daemon restart | SQLite contract reopen proof (`routines` 1/1); integration `authorized_routine_runs_without_client_and_survives_restart` |
| Authorized work runs with no client connected | `agentd` 1s routine tick loop; shipped-binary cycle retains `runs=1` after daemon restart without duplication |
| Below-V10 sessions refuse routine typed | `routine_core = 13`, V10 only; `ipc-control` suite green incl. version-gate patterns; negotiation current pin V10 |
| Contract shared by all stores | `background-contract` 1/1 in-memory + 1/1 SQLite |
| Non-goals held | No cron parser, no launchd/systemd registration, no G12 remote scope (ADR-0045 Alternatives) |

## 3. Workspace gates

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo test --workspace --locked --no-fail-fast` — **1904 passed /
  0 failed / 4 ignored**. The first full run failed 5 storage/agentd
  targets: rewind fixtures set an older `user_version` while the new
  v35 tables existed (`0035_routines.sql: table routines already
  exists`). Fixtures now drop `routine_runs`/`routines` alongside the
  v34 memory tables (disclosed test change, fixture class only);
  all 5 targets green standalone and in the final full run.

## 4. Disclosed test changes

- Schema literals 34 → 35 (pin class) in storage/app-services tests.
- Routine table drops added to schema-rewind fixtures.
- Negotiation current-version pin V9 → V10.
- No production behavior was changed to make a test pass.

## 5. Residuals (non-blocking)

- Routine runs are durable rows, but the Event Journal has no
  routine event types yet (same class as the G10 memory residual);
  follow-up contract task required before journal-based routine
  audit queries.
- `agentctl daemon start|status|stop` maps to the existing
  start/ping/shutdown lifecycle; OS service registration
  (launchd/systemd) is a follow-up, per ADR-0045.
- Tick evidence is a bounded reference string (`evidence:daemon-tick`);
  binding routine runs to full Execution/Attempt records is G12+
  scope, not required by the G11 exit criteria.
- AGENTS.md generated map still stamps 40 packages (now 44).

## 6. Verdict

**`G11_CLOSED`** — every TASK-0188..TASK-0195 exit criterion is
backed by current code and executable evidence. Roadmap advances
to G12.
