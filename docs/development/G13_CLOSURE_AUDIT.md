# G13 Closure Audit (TASK-0214)

> Closure audit of G13 (Cloud/Team), TASK-0206 through TASK-0213,
> executed inline by the implementing agent (weaker than a
> fresh-context auditor; the owner decides whether that suffices
> before merge). Authority: `AGENT.md`, ADR-0047,
> `planning/roadmap.yaml` G13, `planning/tasks/queue.yaml`
> TASK-0206..TASK-0214. Audit only: no production behavior was
> changed by this task.
>
> Verdict: **`G13_CLOSED`**. On this verdict the roadmap gate
> advances to G14 (planned; requires owner ratification before any
> G14 task starts).

## 1. Method

- Every G13 plan claim was checked against code and executable
  evidence in this session.
- Falsification: on the pre-G13 base (TASK-0205 state), no `org`
  crate, no schema v37, and no `IPC_PROTOCOL_VERSION_V12` exist —
  every G13 claim is red on base by construction.
- Gate record: [G13_CLOSURE_GATE.md](./G13_CLOSURE_GATE.md).

## 2. Claim verification

| Claim | Evidence |
|---|---|
| Identity is a keypair fingerprint, never a name (C-4) | `org` model tests; `org_service` role-flow and refusal tests 8/8 |
| Revocation is durable and immediate (C-5) | `org_failure_matrix::revoked_member_refuses_after_store_reopen`; binary restart durability (TASK-0212) |
| Sync is server-blind; carrier sees ciphertext only (C-2) | Matrix carrier-blindness probe; TASK-0212 two-daemon carrier inspection |
| Secrets never enter sync (C-3) | Matrix secret probe: every kind × `RESTRICTED`/`SECRET` refuses `SensitivityRefusal` pre-encryption |
| Tamper applies nothing (C-6) | Matrix flipped-ciphertext `BadSignature`, wrong-key `TamperedEnvelope`, head 0 |
| Stale writer cannot overwrite (C-7) | Matrix out-of-order `Conflict`; TASK-0212 shipped-binary stale seal |
| Lower layer cannot weaken org policy (C-8) | Matrix per-knob weakening leaves policy v1; narrowing applies |
| Quota cannot be evaded by retry/edit (C-9) | Matrix same-key retry appends nothing; 900+200 `OverQuota`, exact-fit admits |
| Replay and impersonation refuse (C-10) | Matrix `DuplicateEnvelope`; forged signer `BadSignature` |
| Local work never needs cloud (C-1) | TASK-0212 fixture execution `SUCCEEDED` on receiver with conflicted sync state |
| Below-V12 typed refusal | V12 gate in negotiation/session; `ipc-control` green |
| Non-goals held | No payment processor, no hosted control plane, no key-distribution protocol (ADR-0047 §8, deferred to own ADRs) |

## 3. Workspace gates

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo test --workspace --locked --no-fail-fast` — **1946 passed /
  0 failed / 4 ignored** (exit 0, `/tmp/g13-test2.log`). One earlier
  run in this session flaked once on the pre-existing G5 test
  `concurrent_resumes_converge_on_exactly_one_attempt` (60 s polling
  deadline under full-suite parallel load; passes isolated in ~2 s
  and in the final full run). The G13 diff touches no execution or
  resume code; recorded as a residual below, not a G13 blocker.

## 4. Disclosed test changes

- TASK-0209: schema-version pins 36 → 37 and legacy drop-lists in
  existing storage tests (test-only).
- TASK-0211: protocol version pins 11 → 12 in ipc-contracts /
  ipc-control tests (test-only).
- TASK-0213: new `org_failure_matrix` test target only; no existing
  test was modified, weakened, or skipped by the G13 sequence.

## 5. Residuals (non-blocking)

- Real payment processor and hosted control plane remain deferred;
  each requires its own ADR before any implementation (ADR-0047 §8).
- Org key distribution/recovery is out-of-band in v1 (fingerprint-
  verified provisioning); no distribution protocol exists yet.
- Pre-existing load flake: `concurrent_resumes_converge_on_exactly_one_attempt`
  (`bins/agentd/tests/execution_recovery.rs`) can exceed its 60 s
  polling deadline under full-suite parallel load. It predates G13,
  is untouched by this gate, and should get a determinism pass in a
  G5-hardening follow-up if it recurs.
- Event Journal has no org event types yet (same class as the
  G10/G11/G12 residuals).

## 6. Verdict

**`G13_CLOSED`** — every TASK-0206..TASK-0213 exit criterion is
backed by current code and executable evidence. Roadmap advances
to G14.
