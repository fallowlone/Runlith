# G12 Closure Audit (TASK-0205)

> Closure audit of G12 (Remote Worker), TASK-0197 through
> TASK-0204, executed inline by the implementing agent (weaker
> than a fresh-context auditor; the owner decides whether that
> suffices before merge). Authority: `AGENT.md`, ADR-0046,
> `planning/roadmap.yaml` G12, `planning/tasks/queue.yaml`
> TASK-0197..TASK-0205. Audit only: no production behavior was
> changed by this task beyond disclosed test-fixture pins.
>
> Verdict: **`G12_CLOSED`**. On this verdict the roadmap gate
> advances to G13.

## 1. Method

- Every G12 plan claim was checked against code and executable
  evidence in this session.
- Falsification: on the pre-G12 base (local `main`), no `worker`
  crate, no schema v36, and no `IPC_PROTOCOL_VERSION_V11` exist —
  every G12 claim is red on base by construction.
- Gate record: [G12_CLOSURE_GATE.md](./G12_CLOSURE_GATE.md).

## 2. Claim verification

| Claim | Evidence |
|---|---|
| Device identity is a keypair fingerprint, never a name (W-1) | ed25519 domain tests 4/4; fingerprint binds public key; secret never in records |
| Only correctly signed fresh challenges claim/complete (W-2..W-4) | Service 2/2; failure matrix replay/double-claim/non-claimant refusals; shipped-binary bad-signature refusal `bad_signature` |
| Revoked device gets no challenge and no result (W-10) | Failure matrix `revoked_after_claim_cannot_complete`; domain revocation |
| Single claim per job (W-7) | Domain + failure matrix `AlreadyClaimed` |
| Offline mid-job never fabricates completion | Failure matrix `offline_mid_job_stays_claimed_not_completed` |
| Durable across coordinator restart | SQLite contract reopen 1/1; integration across store reopen; binary cycle restart |
| Below-V11 typed refusal | V11 gate in negotiation/session; `ipc-control` green |
| Worker keys never cross the wire | `agentctl worker keygen|sign` local-only; proto carries public keys and signatures only |
| Non-goals held | No cloud (G13), no worker inbound listener, no SSH/mTLS CA in v1 (ADR-0046) |

## 3. Workspace gates

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo test --workspace --locked --no-fail-fast` — **1917 passed /
  0 failed / 4 ignored** (exit 0, `/tmp/g12-test.log`).

## 4. Disclosed test changes

- Schema literals 35 → 36 (pin class); worker table drops added to
  schema-rewind fixtures alongside routine/memory drops.
- Negotiation current-version pin V10 → V11.
- New dependency `ed25519-dalek` 3 (worker domain only; evaluated:
  necessary for ADR-0046 signatures, maintained RustCrypto crate,
  no provider code copied).

## 5. Residuals (non-blocking)

- Transport v1 rides the coordinator's existing framed IPC endpoint;
  a network listener for remote machines (TLS, bind policy) is the
  next hardening step and must land with its own ADR before any
  non-loopback use. The signed-challenge contract is transport
  independent by design.
- `agentctl worker keygen` prints the secret once to stdout for the
  operator to vault; a direct-to-vault enrollment flow is a
  follow-up.
- Event Journal has no worker event types yet (same class as the
  G10/G11 residuals).

## 6. Verdict

**`G12_CLOSED`** — every TASK-0197..TASK-0204 exit criterion is
backed by current code and executable evidence. Roadmap advances
to G13.
