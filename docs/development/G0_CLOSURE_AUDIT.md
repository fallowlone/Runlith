# Final G0 Closure Audit

> Independent closure audit (TASK-0016) over the CURRENT repository. Every
> TASK-0010 PARTIAL/MISSING/G1_BLOCKER was re-evaluated from repository
> evidence, not from task reports. Historical audit
> [`G0_AUDIT.md`](./G0_AUDIT.md) (TASK-0010 state) is untouched.
> Row IDs (`G0-C001…`) are audit identifiers only.

## Executive Result

```text
G0_COMPLETE
```

Every active G0 exit requirement is satisfied with repository evidence:
all canonical artifacts exist; invariant census matches the Source of Truth
(144/144, no dupes); ADR-0001…0019 all Accepted with zero unclassified
coverage candidates; Task/Event schemas agree with their contracts;
Context and Continuity contracts are formalized; Provider/Runner/FakeAgent
contracts hold; the IPC skeleton is minimal and the full G1
implementation profile is decided; layout, queue, links, formats, and the
cargo baseline are green. No architecture contradiction was found; no G1
implementation-blocking open decision remains. The G1 bucket of remaining
open items is empty by the §45 standard (zero unresolved
implementation-blocking architecture decisions).

Non-blocking hygiene note (not a criterion failure): ARCHITECTURE.md
bounded-context "Refs" lines for Planning/Orchestration and
Evaluation/Verification still read "ADR status: Not yet materialized",
predating ADR-0017/ADR-0018. Cross-reference staleness only — authority is
unaffected — left for a later docs touch-up since this task may not repair
architecture docs.

## Artifact Checklist

| ID | Criterion | Status | Evidence | Gap | Gate impact |
|---|---|---|---|---|---|
| G0-C001 | PROJECT_CHARTER.md | DONE | purpose, local-first control plane, outcomes orientation, 5 differentiators, 6 principles, 13 v1 non-goals; no contradiction with architecture | none | — |
| G0-C002 | ARCHITECTURE.md bounded contexts | DONE | 15 contexts each with purpose, owns, forbidden, inbound/outbound, authoritative state, status semantics; cross-context rules; canonical-state table | none (see hygiene note above) | — |
| G0-C003 | INVARIANTS.md census | DONE | 144/144 SoT IDs, each exactly once, no conflicts, no inventions | none | — |
| G0-C004 | Threat Model draft | DONE | assets, actors, 17-entry register incl. THR-003 IPC with peer-auth Open | none for G0 | — |
| G0-C005 | Privacy contract | DONE | PRIV-001…010 all Accepted-pending; local-first, no account, telemetry-off core, provider-vs-own-cloud, SECRET rules intact | none (legal policy correctly out of scope) | — |
| G0-C006 | Data Classification | DONE | 5 fixed levels; SECRET never in model context/journal; category separation | none | — |
| G0-C007 | planning/roadmap.yaml | DONE | single file, G0…G13 in order, pre-transition state verified (current_gate G0, G1 blocked) | none | — |
| G0-C008 | ADR set 0001…0019 | DONE | 19 files, unique IDs, all Accepted, index covers all + COVERAGE.md | none | — |
| G0-C009 | Task schema | DONE | Draft 2020-12, required id/objective/status, exact 12-state enum, no transition/taxonomy/persistence invention | none | — |
| G0-C010 | Event schema | DONE | required 5-field envelope, integer version ≥1, date-time, distinct correlation/causation, no taxonomy/ID/encoding | none | — |
| G0-C011 | Provider contract | DONE | anti-corruption layer: no Task mutation, no Memory writes, no scheduling, no completion, no Policy bypass, no provider Task types | none | — |
| G0-C012 | Runner contract | DONE | NativeRunner = Trusted Host ≠ sandbox; ContainerRunner autonomous direction; fail-closed; fs ≠ network isolation | none | — |
| G0-C013 | Context contract | DONE | disposable view, smallest-sufficient purpose, 11-field ContextItem (all open points preserved), 9-concept ContextPlan, hybrid retrieval, L0–L4, cost≠attention, non-canonical rendering, observability, envelope distinction, port boundaries | none | — |
| G0-C014 | Continuity contract | DONE | Epoch model, 3 levels, 9-step rollover, NO CHECKPOINT→NO ROLLOVER, search→read, INV-021 retrievability, optimization-only compaction, Claude→Codex migration, Memory/Journal boundaries | none | — |
| G0-C015 | FakeAgent contract | DONE | all 11 accepted scenarios retained verbatim with non-bypass semantics; implementation correctly absent (G3) | none | — |
| G0-C016 | IPC control contract + proto | DONE | ipc.md semantics + 80-line proto: handshake/ping/shutdown only, no service block, no business messages | none | — |
| G0-C017 | Atomic task queue persisted | DONE | queue.yaml: TASK-0012…0021 bounded/explicit, deps intact, pre-transition state verified | none | — |
| G0-C018 | Repository layout | DONE | bins/agentd, bins/agentctl, crates/ipc-contracts, contracts/json-schema, docs/{architecture,security,privacy,development}, planning | none | — |

## Semantic Exit Criteria

| ID | Criterion | Status | Evidence |
|---|---|---|---|
| G0-C019 | Decisions→ADRs via ADR-0019 | DONE | every ACTIVE decision has Accepted ADR; deferred/research rows carry gates (§ADR Coverage) |
| G0-C020 | Task state machine formalized | DONE | 12 states in SoT order, accepted-vs-unresolved transitions separated, 17 open items listed-not-resolved |
| G0-C021 | Completion authority coherent | DONE | task-engine §13 + ADR-0018 + INV-009 agree: submit_result only, never complete_task; schema carries no completion API |
| G0-C022 | v1 persistence model | DONE | relational + append-only journal + separate audit; v1 ≠ full Event Sourcing, in contract and schema comments |
| G0-C023 | Resource/Context/Memory/Continuity split | DONE | preparer / selector / preserver / storer; no merged canonical owner in any contract |
| G0-C024 | No premature runtime | DONE | mains are 11-line version stubs; lib.rs holds version primitives + 3 tests; zero G1+ deps or mechanisms in code |

## ADR Coverage and Activation

| ID | Check | Status | Evidence |
|---|---|---|---|
| G0-C025 | ADR-0019 governance present | DONE | ACTIVE / DEFERRED / RESEARCH defined; invariants survive deferral; gate-audit rule stated |
| G0-C026 | Planning active + covered | DONE | ADR-0017 Accepted: proposal-only planner, declarative versioned PlanChangeSet, validation-before-mutation, PlanRevision+provenance, JIT branch revision, benchmark-before-default, categorical liveness bounds, lifecycle/readiness split, parent-completion rule |
| G0-C027 | Evaluation active + covered | DONE | ADR-0018 Accepted: Generation≠Done, submit→VERIFIED/REJECTED→trusted transition, contextual mandatory verification, TestMutationPolicy concepts, attestation principle, provider neutrality, score never authority |
| G0-C028 | Coverage register complete | DONE | 22 rows, all classified (2 active-covered, 3 existing-covered, 16 deferred with gates, 1 research); post-v1 concepts correctly untracked |
| G0-C029 | No ACTIVE G1 decision uncovered | DONE | G1 needs: transport/contracts (ADR-0001/0003), task shape (ADR-0004/0005), control surface (G1 profile in ADR-0003/ipc.md); all Accepted |

## G1 IPC Readiness

| ID | Former blocker | Status | Evidence |
|---|---|---|---|
| G0-C030 | wire framing | DONE | u32 big-endian length prefix + payload; tokio-util LengthDelimitedCodec direction with explicit config |
| G0-C031 | codegen + toolchain | DONE | prost/prost-build 0.14.4 family; protoc-bin-vendored =3.2.0; OUT_DIR-only, never committed |
| G0-C032 | Unix endpoint | DONE | XDG preferred + UID fallback; UID-owned, non-symlink, 0700 dir; agentd.sock 0600; pre-use validation; path-length preflight |
| G0-C033 | Windows pipe | DONE | SID-suffixed name; explicit user+SYSTEM DACL; no Everyone/Anonymous; remote rejected; Byte mode; 8-instance accept loop |
| G0-C034 | max frame | DONE | MAX_FRAME_BYTES = 1_048_576; oversized invalid; zero-length → close, no resync |
| G0-C035 | timeouts | DONE | 5 s frame R/W, 5 s handshake each side, 2 s ping, 5 s + 5 s shutdown; no idle disconnect |
| G0-C036 | stale endpoint | DONE | 8-step lstat/verify/probe/re-check/remove/bind sequence; never blind rm |
| G0-C037 | single instance | DONE | Unix bind-winner authority; Windows first_pipe_instance → AlreadyRunning; no PID-file reliance |
| G0-C038 | start races | DONE | connect-first, spawn-on-missing, endpoint ownership decides winner |
| G0-C039 | reconnect policy | DONE | 10 s budget, 50→1000 ms staged backoff, no jitter; new transport + new handshake |
| G0-C040 | multi-client | DONE | MAX_CONTROL_CLIENTS = 8; independent handshake state; no cross-client authority |
| G0-C041 | correlation decision | DONE | sequential one-in-flight; no request/response IDs in G1; multiplexing deferred |
| G0-C042 | shutdown auth + timing | DONE | endpoint-admission + handshake basis (not client_kind); 7-step semantics; idempotent; no force-kill command |
| G0-C043 | profile coherence | DONE | Tokio 1.51.x + prost 0.14.4 + vendored protoc + protocol 0 + highest-common selection + tauri-host/agentctl registry + CARGO_PKG_VERSION-concept builds; no gRPC; no business IPC |
| G0-C044 | peer auth PRE-G2 | DONE | classified PRE-G2 (not G1 blocker) in ipc.md; THR-003 still Open-consistent; no contradiction with TB2 |

G1 scope sufficiency: TASK-0017 (codegen) → 0018 (framing/transport) → 0019 (handshake/ping/negotiation) → 0020 (lifecycle/reconnect/shutdown) → 0021 (failure matrix) each map onto decided profile sections; none requires inventing a public/security contract choice.

## Architecture Consistency

| ID | Check | Status |
|---|---|---|
| G0-C045 | 17-point non-negotiable matrix (task≠exec, no frontend shell, adapters, self-completion, disposable context, non-durable provider ctx, checkpoint gating, journal≠audit, v1≠ES, native≠sandbox, fail-closed, SECRET exclusion, UDS+Protobuf, no HTTP fallback, planner-proposal, independent evaluation, cloud-optional) | DONE, all pass |
| G0-C046 | duplicate authority sweep (one lifecycle, one proto, one roadmap, one queue, one schema each, single gate authority) | DONE, no duplicates |

## Remaining Open Items by Earliest Gate

- **G1:** none — zero unresolved implementation-blocking architecture decisions (non-blocking observations: none outstanding).
- **G2:** peer-auth mechanism; Task/Core scheduler details (CLAIMED lease, retry budgets); error taxonomy beyond handshake.
- **G3:** FakeAgent physical placement/scenario config; scheduler algorithm (bounded by ADR-0017).
- **G4:** worktree mechanics; **G5:** provider capability taxonomy; **G6:** Policy/Secrets/Audit detail; **G7:** adaptive-planning benchmarks/policy; **G8:** evaluator implementations; **G9+:** Resource Engine ADR, retrieval implementations, retention/cleanup policies.
- **Deferred:** telemetry schema, sync model, remote worker, plugins, crash reports, export format, subprocessor mapping.
- **Research:** TOON/TRON benchmarking; reliability-score formulation.

## G1 Blockers

```text
NONE — evidence-backed per G0-C030…G0-C044.
```

## Deferred Non-Blocking Architecture

Per COVERAGE.md: 16 deferred targets with explicit earliest gates, 1 research-only item with promotion rule, 3 areas covered by existing ADRs. Deferred status suspends no invariant. None is required by G1 implementation.

## Gate Transition

Phase A concluded `G0_COMPLETE`; Phase B transition performed after that conclusion:

- `planning/roadmap.yaml`: `current_gate` G0 → G1; G0 status → completed (closure recorded); G1 blocked → in_progress; G2–G13 unchanged.
- `planning/tasks/queue.yaml`: `completed_through` TASK-0015 → TASK-0016; TASK-0016 item retained as history; `blocked_by: TASK-0016` markers removed from TASK-0017…0021 execution entries only; all `depends_on` edges preserved (0017→0018→0019→0020→0021 chain plus 0017→0014 intact).
- TASK-0017 is eligible to begin; it is not completed, running, or claimed — no runtime lifecycle semantics were introduced into planning YAML.
- No G1 implementation was started by this task.

## Verification Evidence

- Tree inventory: 60 files; required bootstrap surfaces present; no G2+ crates; no generated code committed.
- Invariant census re-run: 144/144 SoT IDs, each exactly once, no conflicts, no inventions.
- ADR census re-run: ADR-0001…0019, unique IDs, all Accepted, index + coverage complete; no ADR-0020+.
- Fixtures re-run (structural subset): valid Task/Event pass; bogus status, missing id, zero version, bad timestamp fail.
- Links: 160 Markdown references resolve, zero broken.
- Formats: roadmap + queue parse (YAML); both schemas parse (JSON); proto compiles via system protoc to temp descriptor.
- Leakage sweep: no tokio/prost/SQLite/Tauri/transport/business-IPC tokens in Rust sources or manifests.
- Cargo baseline: fmt --check clean; check clean; test 3 passed; clippy -D warnings clean.
- Pre-transition planning state verified before edits (roadmap G0/in-progress/G1-blocked; queue completed_through TASK-0015 with G1 gated ×5); post-transition state re-verified after edits.
