# G9 Closure Re-Audit Gate (TASK-0178)

Verdict: **`GATE_PASS`**

| Claim | Proof |
|---|---|
| V8 assigned; v0–v7 refuse TAKEOVER | PROOF — `cargo test -p ipc-contracts takeover` 3/3 ok on `298696a`; red pair on `a19a3e5` (current=7, supported `[0..=7]`, suite 26/26 encoding refusal) |
| Below-V8 refusal typed, pre-service | PROOF — `cargo test -p agentd --lib takeover` 4/4 ok |
| Assume/append/render/return via shipped binaries | PROOF — `takeover_assume_append_render_return_cycle_through_shipped_binaries` ok |
| Refusal matrix (active run, stale decision, stale lease, consumed HDR) | PROOF — three shipped-binary refusal tests ok; replay refusal inside cycle test |
| Expiry tick without service call | PROOF — `takeover_expiry_tick_reconciles_without_a_service_call` ok |
| Production requests offer Takeover | PROOF — planning_service.rs:1566, :1886 opened; binary decide-on-V8 test ok |
| TASK-0170..0175 evidence holds | PROOF — §4 suites re-run green (28/28, 18/18, 7/7, 4/4, 70/70, finder green) |
| §5 residuals non-blocking, unworsened | PROOF — §5 file/line re-inspection at audited head |
| Entity Graph condition | PROOF — fresh docs/planning/crates search; no measured ambiguity failure; benchmark 16/16 |
| No production behavior changed by this audit | PROOF — all runs in scratch clones; this task writes audit/planning records only |

Full record: `docs/development/G9_CLOSURE_REAUDIT.md`.
