# G13 Closure Gate (TASK-0214)

Verdict: **GATE_PASS** — `G13_CLOSED` (see
[G13_CLOSURE_AUDIT.md](./G13_CLOSURE_AUDIT.md)).

| Claim | Proof |
|---|---|
| Role authority and self-escalation refuse (C-4) | `org_service` 8/8 incl. `non_admin_and_self_escalation_refuse` |
| Revoked member refuses after reopen (C-5) | `org_failure_matrix::revoked_member_refuses_after_store_reopen` |
| Carrier sees ciphertext only (C-2) | Matrix carrier-blindness probe; TASK-0212 carrier inspection |
| Secret probe refuses pre-encryption (C-3) | `org_failure_matrix::secret_in_envelope_probe_refuses_before_encryption` |
| Tamper applies nothing (C-6) | `org_failure_matrix::tampered_envelope_variants_apply_nothing` |
| Stale revision conflicts (C-7) | `org_failure_matrix::stale_revision_conflict_applies_nothing` |
| Policy weakening leaves policy unchanged (C-8) | `org_failure_matrix::policy_weakening_attempts_leave_policy_unchanged` |
| Over-quota and retry evasion refuse (C-9) | `org_failure_matrix::over_quota_and_idempotency_evasion_refuse` |
| Replay and impersonation refuse (C-10) | `org_failure_matrix::replay_and_impersonation_refuse` |
| Two-store dogfood durable (C-1) | Matrix reopen round-trip; TASK-0212 two-daemon shipped-binary cycle |
| Below-V12 typed refusal | `ipc-control` green; V12 gates |
| Full workspace green | 1946 passed / 0 failed / 4 ignored |
| Lint gates green | fmt clean; clippy `-D warnings` clean |

No claim is left at MISSING_PROOF.
