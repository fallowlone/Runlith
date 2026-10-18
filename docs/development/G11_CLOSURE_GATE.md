# G11 Closure Gate (TASK-0196)

Verdict: **GATE_PASS** — `G11_CLOSED` (see
[G11_CLOSURE_AUDIT.md](./G11_CLOSURE_AUDIT.md)).

| Claim | Proof |
|---|---|
| PROPOSED routines never run | `background` model tests 7/7; service test `unauthorized_routine_never_ticks` |
| Self-authorization refused | Domain + service tests; shipped CLI `self_authorization` in `routine_define_authorize_run_cycle_through_shipped_binaries` 1/1 |
| Budget exhaustion terminal | Failure matrix 4/4 (`budget_exhaustion_is_terminal`) |
| Crashed runs interrupted, never replayed | Failure matrix `crashed_mid_run_is_interrupted_never_reexecuted`; agentd startup reconcile |
| Draining refuses work | Failure matrix `draining_refuses_new_runs` |
| Restart durability | SQLite routine contract 1/1 with reopen; integration test across store reopen |
| Work continues with UI closed | agentd routine tick loop; binary cycle restart retains `runs=1` |
| Below-V10 typed refusal | `ipc-control` green; V10 gate in negotiation/session |
| Full workspace green | `cargo test --workspace --locked --no-fail-fast` → 1904 passed / 0 failed / 4 ignored |
| Lint gates green | `cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean |

No claim is left at MISSING_PROOF.
