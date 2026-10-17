# G10 Closure Audit (TASK-0187)

> Independent closure audit of G10 (Memory Engine), TASK-0179
> through TASK-0186, from a fresh context.
> Authority: `AGENT.md`, [ADR-0012](../architecture/adr/ADR-0012-structured-memory-provenance.md),
> [ADR-0044](../architecture/adr/ADR-0044-g10-memory-engine-contract.md),
> `planning/roadmap.yaml` G10, `planning/tasks/queue.yaml`
> TASK-0179..TASK-0187. Audit only: no production behavior was
> changed by this task.
>
> Verdict: **`G10_CLOSED`**. On this verdict the roadmap gate
> advances to G11.

## 1. Method

- Claims were collected from the TASK-0179..0186 queue outcomes and
  each was verified against code and executable evidence, not task
  self-reports. An independent fresh-context auditor re-ran the
  focused suites and the full agentd/agentctl run (exit 0).
- Falsification first: on the pre-G10 base `22d35fe`, the queue still
  reads `current_gate: G9` against roadmap `G10`, and no
  `IPC_PROTOCOL_VERSION_V9` or memory crate exists — every G10 claim
  is red on base by construction and green on this branch.
- Gate record: [G10_CLOSURE_GATE.md](./G10_CLOSURE_GATE.md).

## 2. Claim verification

| Claim | Evidence |
|---|---|
| Only Confirmed, unexpired, anchor-fresh memory is context-eligible | `crates/memory/src/model.rs` eligibility predicate; `memory_context` 2/2 (proposed yields zero candidates; stale/superseded/retired never become candidates) |
| Author cannot self-validate (INV-029) | Domain `UntrustedPromotion`; service test 2/2; shipped CLI refusal `untrusted_promotion` in binary cycle and dogfood |
| SECRET never stored or served (INV-008) | Capture refusal (M-3) in domain, service, dispatch and shipped-binary tests (`sensitivity_refusal`) |
| Anchor drift marks memory stale, never silently authoritative | Domain transition tests 6/6; service staleness flow; SQLite lifecycle persistence in contract suite |
| Dedup, supersession, lifecycle per ADR-0044 | `memory` 8/8; `memory-contract` 1/1 against in-memory double and 1/1 against SQLite; storage-sqlite full package 258/0 |
| Durable, atomic, restart-safe | Migration `0034`; single-transaction multi-row writes; contract reopen proof; failure matrix crash-restart 3/3 |
| Below-V9 sessions refuse memory typed | `ipc-control` 55/55 incl. v8-rejects/v9-surfaces; `ipc-contracts` 28/28 |
| Shipped operator surface works end to end | `execution_binaries` memory cycle 1/1 (capture, author refusal, validate, list, daemon restart, SECRET refusal); manual dogfood `mem_01M55M779S0000000000000000` confirmed and retrieved after daemon restart |
| Context integration is explainable | Selection explain-after-restart with `SourceRef::Memory` provenance (`memory_context` 2/2) |
| Non-goals held | No graph/vector DB, no transcript-as-memory; provider-native files are inputs only (ADR-0044 §2, M-6..M-8) |

## 3. Workspace gates

- `cargo fmt --all --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — clean.
- `cargo test --workspace --locked --no-fail-fast` — **1885 passed /
  0 failed**. An earlier run in this session read 1883/2: the
  `failure_matrix` schema-pin literal (33) since updated to 34, and
  the documented `execution_recovery` load flake, green standalone
  (1/1, twice) and in the final full run.

## 4. Residuals (non-blocking)

- Memory mutations are atomic in one SQLite transaction, but the
  append-only Event Journal has no memory event types yet
  (`event.rs` taxonomy is task-scoped). Recorded in TASK-0182
  outcome; follow-up contract task required before journal-based
  memory audit queries.
- `AGENTS.md` generated workspace map still stamps 40 packages;
  the workspace now has 42 (`memory`, `memory-contract`). Derived
  map refresh is separate chore work.

## 5. Verdict

**`G10_CLOSED`** — every TASK-0179..0186 exit criterion is backed by
current code and executable evidence. Roadmap advances to G11.
