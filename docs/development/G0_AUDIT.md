# G0 Architecture Audit

> Evidence-based G0 exit audit for AI Engineering OS (TASK-0010).
> Audit only: nothing was repaired, created, or implemented to satisfy it.
> G0 row IDs (`G0-A001…`) are audit-row identifiers, not ADRs or invariants.
>
> - Baseline: `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` (G0 gate list,
>   §94 Definition of G0 complete, §101 planned documents)
> - Index: [`../architecture/INDEX.md`](../architecture/INDEX.md)

## Executive Result

```text
G0_INCOMPLETE
```

The architecture is coherent — no contradictory authority decisions were
found, all 144 invariants are present exactly once, all 16 baseline ADRs are
accepted, and every implemented-surface contract that G0 requires is
materially complete. G0 is not complete because required exit artifacts are
still missing or partial: no roadmap artifact, no repository-persisted atomic
task queue, no machine-readable Task/Event schemas, no formal Context
contract, no formal Continuity model, and accepted architecture-significant
decisions beyond the baseline 16 lack ADRs. Missing documents alone mean
incomplete, not blocked: per audit rules this is `G0_INCOMPLETE`, not
`G0_BLOCKED` and not a project failure.

## Source of Truth G0 Artifact Checklist

| ID | Criterion | Status | Evidence | Gap | Earliest gate affected |
|----|-----------|--------|----------|-----|------------------------|
| G0-A001 | PROJECT_CHARTER.md | DONE | [`PROJECT_CHARTER.md`](../../PROJECT_CHARTER.md) (79 lines): purpose = local-first control plane; 5 differentiators; non-negotiable principles incl. local-first, Task-central, provider-neutral, evidence-over-claims; 13 explicit v1 non-goals; no implementation content | none | — |
| G0-A002 | ARCHITECTURE.md | DONE | [`ARCHITECTURE.md`](../../ARCHITECTURE.md) (341 lines): layers §1; 15 bounded contexts §2, each with Purpose / Owns / Forbidden / Inbound / Outbound / Authoritative state / Refs; cross-context rules §3; interface ownership §4; 13-row canonical-state table §5; deferred-targets table §6; status legend §7; composition-root direction (§1 agentd, §4) | none | — |
| G0-A003 | INVARIANTS.md | DONE | [`INVARIANTS.md`](../architecture/INVARIANTS.md) (185 lines): census 144/144 SoT IDs present, 144 `**INV-n**` occurrences (no duplicates), no text conflicts found, header requires ADR + review for changes | none | — |
| G0-A004 | THREAT_MODEL.md | DONE | [`THREAT_MODEL.md`](../security/THREAT_MODEL.md) (110 lines): assets, actors, 17-entry THR register (THR-001…017 verified); THR-003 records IPC peer-auth as Open with user-private-dir mitigation — materially usable for the G1 surface | none for G1 | G6+ (peer-auth before privileged commands) |
| G0-A005 | PRIVACY_ARCHITECTURE.md | DONE | [`PRIVACY_ARCHITECTURE.md`](../privacy/PRIVACY_ARCHITECTURE.md) + [`TECHNICAL_PRIVACY_CONTRACT.md`](../privacy/TECHNICAL_PRIVACY_CONTRACT.md) (PRIV-001…010 verified) + [`DATA_FLOW_MAP.md`](../privacy/DATA_FLOW_MAP.md) (FLOW-001…012 verified); legal policy correctly absent | none | — |
| G0-A006 | DATA_CLASSIFICATION.md | DONE | [`DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md): PUBLIC / PROJECT / PRIVATE / RESTRICTED / SECRET levels + category-separation section; SECRET excluded from model context | none | — |
| G0-A007 | ROADMAP.yaml | MISSING | no `ROADMAP.yaml`, no `planning/roadmap.yaml`, no `planning/` dir anywhere in repo (full tree inventory §Verification Evidence) | entire artifact | G1 (governance: G1 must run as queued atomic tasks) |
| G0-A008 | ADR set | DONE | 16/16 files in [`adr/`](../architecture/adr/INDEX.md), all `Status: Accepted` (per-file loop, §Verification Evidence); index table complete | coverage of post-baseline decisions → G0-A014 | — |
| G0-A009 | Task schema | PARTIAL | [`task-engine.md`](../architecture/task-engine.md) (428 lines) is conceptual prose only; no JSON Schema / typed schema file; `contracts/` dir does not exist; IPC proto is explicitly control-plane, not Task state | machine-readable canonical Task schema | G2 |
| G0-A010 | Event schema | PARTIAL | [`events.md`](../architecture/events.md) (398 lines) formalizes envelope semantics only; no persistent-event schema file; must not be confused with the IPC Protobuf schema | machine-readable canonical event-envelope schema | G2 |
| G0-A011 | Provider contract | DONE | [`providers.md`](../architecture/providers.md) (331 lines): who-dimension, conceptual detect/capabilities/start/send/cancel, 16-item forbidden list, 8 availability states verbatim (lines 174–181), neutrality, auth boundary; sufficient for G1, which implements no providers | none | — |
| G0-A012 | Runner contract | DONE | [`runners.md`](../architecture/runners.md) (262 lines): exact 4-runner abstraction, NativeRunner = Trusted Host ≠ sandbox, fail-closed, fs ≠ network isolation; sufficient for G1, which implements no runners | none | — |
| G0-A013 | Context contract | PARTIAL | no repo-local canonical contract file; substantial material exists in [ADR-0010](../architecture/adr/ADR-0010-context-derived-state.md) (60 lines: funnel, renderer/consequence rules) + ARCHITECTURE.md Context Engine section (owned/forbidden) | formal contract guiding implementation (ContextItem / ContextPlan / Execution Envelope / budgeting / filtering) without reading SoT §§23–43 | G9 |

OPEN GOVERNANCE DETAIL (G0-A007): the SoT names the roadmap both as
`ROADMAP.yaml` (G0 checklist) and as `planning/roadmap.yaml` (example
layout). The ambiguity is reported, not resolved — the follow-up task must
pick one canonical path.

## Source of Truth G0 Semantic Checklist

| ID | Criterion | Status | Evidence | Gap | Earliest gate affected |
|----|-----------|--------|----------|-----|------------------------|
| G0-A014 | all current decisions → ADRs | PARTIAL | 16/16 baseline ADRs accepted (G0-A008); accepted architecture-significant decisions without ADR listed in ADR Coverage Audit below | unmaterializedAdrCandidates (JSON) | varies per candidate |
| G0-A015 | invariants accepted | DONE | INVARIANTS.md registry + all ADRs reference INV IDs + change-requires-ADR header | none | — |
| G0-A016 | bounded contexts enumerated | DONE | 15 contexts, ARCHITECTURE.md §2 | none | — |
| G0-A017 | owned responsibilities | DONE | every §2 context has Owns + Authoritative state | none | — |
| G0-A018 | forbidden responsibilities | DONE | every §2 context has Forbidden | none | — |
| G0-A019 | threat model draft | DONE | same evidence as G0-A004 | none | — |
| G0-A020 | privacy contract | DONE | same evidence as G0-A005 | none | — |
| G0-A021 | IPC contract skeleton | DONE | [`agentd_control.proto`](../../crates/ipc-contracts/proto/agentd_control.proto) (80 lines, protoc-validated) + [`ipc.md`](../architecture/ipc.md) (194 lines); surface verified = handshake/ping/shutdown only, no business APIs, no gRPC/Any/shell tokens | none | — |
| G0-A022 | Task state machine formalized | DONE | task-engine.md: 12 states in SoT order (lines 136–159), per-state semantics, transition registry separating Accepted from Unresolved, 17 open items listed-not-resolved | none | — |
| G0-A023 | Context Continuity model formalized | PARTIAL | [ADR-0011](../architecture/adr/ADR-0011-provider-independent-context-continuity.md) is substantive (L1/L2/L3, epochs, rollover algorithm, NO CHECKPOINT → NO ROLLOVER); but ADR acceptance ≠ formal continuity model — no `continuity.md` exists (SoT §101 plans one) | formal epochs/checkpoints/history/rollover model | G9 |
| G0-A024 | Provider/Runner/FakeAgent contracts | DONE | providers.md + runners.md + [`fake-agent.md`](../architecture/fake-agent.md) (142 lines; 11 scenarios verbatim lines 40–50, each with non-bypass semantics) | none | — |
| G0-A025 | repository layout | DONE | `bins/agentd`, `bins/agentctl`, `crates/ipc-contracts`, `docs/{architecture,security,privacy}` — coherent bootstrap; no crate explosion, none required | none | — |
| G0-A026 | first atomic task queue | MISSING | no `planning/tasks/`, no `TASK-*.md`, no roadmap queue in repo; TASK-0001…0009 reports exist only in conversation history, which is not canonical project state | entire queue incl. G1 scope | G1 (governance) |

## Architecture Consistency Checks

| ID | Criterion | Status | Evidence |
|----|-----------|--------|----------|
| G0-A027 | non-negotiable invariants hold in repo docs | DONE | 13/13 spot checks pass against current files: frontend no shell (ADR-0007); NativeRunner ≠ sandbox + fail-closed (runners.md); SECRET excluded (DATA_CLASSIFICATION.md); journal ≠ audit + v1 ≠ ES (events.md); attempt failure ≠ Task failure + no self-complete (task-engine.md); UDS + Protobuf (ipc.md); cloud optional (INV-011); Task≠Execution≠Attempt≠Session + adapters never mutate state (providers.md INV-004); context derived (ADR-0010) |
| G0-A028 | no duplicate/conflicting authority | DONE | single lifecycle definition (`RETRY_READY` occurs only in task-engine.md); single proto location (one `.proto` in repo); single invariant registry; single privacy contract set; provider-specific domain variants appear only as cited prohibitions (`ClaudeTask` in providers.md/task-engine.md, `ClaudeTaskCompleted` in events.md); `sandboxed = true`, `complete_task()`, `localhost HTTP` occur only in prohibitions |
| G0-A029 | code boundary: bootstrap only | DONE | `bins/agentd/src/main.rs` + `bins/agentctl/src/main.rs` are 11-line stubs printing the protocol version; `crates/ipc-contracts/src/lib.rs` holds version primitives only; no SQLite, providers, runners, Tauri, transport, or business IPC in code |
| G0-A030 | link health | DONE | 35 markdown files checked, 0 broken links. Note (non-blocking): SoT §102 names `docs/research/external-projects-adoption.md`, which does not exist — but no audited file links to it |

No contradictions between accepted sources were found: no `ARCHITECTURE_BLOCKER`, hence INCOMPLETE rather than BLOCKED.

## Missing / Partial Artifacts

- MISSING: roadmap artifact (G0-A007); repository-persisted atomic task queue (G0-A026).
- PARTIAL: Task schema (G0-A009); Event schema (G0-A010); Context contract (G0-A013); ADR coverage of current decisions (G0-A014); Continuity formalization (G0-A023).
- Deliberately absent and correct: legal Privacy Policy, `contracts/` implementations, codegen, transport, Task Engine code — none are G0 requirements.

## ADR Coverage Audit

Baseline ADR-0001…0016: all accepted, all referenced from ARCHITECTURE.md and
the contracts. The following accepted, implementation-constraining decisions
have no ADR (no IDs assigned, no ADRs written — audit only):

| Candidate | Why architecture-significant | Earliest gate | Class |
|-----------|------------------------------|---------------|-------|
| Planning / Orchestration (PlannerPort, PlanChangeSet, reconciliation, scheduler architecture) | G3 runs scheduler + G7 adaptive planning; ARCHITECTURE.md: "Not yet materialized" | G3 | PRE_G3 |
| Evaluation / Verification pipeline | G3 Submit→Verify flow grants COMPLETED; ARCHITECTURE.md: "Not yet materialized" | G3 | PRE_G3 |
| Policy Engine dedicated ADR (ADR-0013 self-marks partial) | G6 authorization core | G6 | PRE_G6 |
| Secrets Broker dedicated ADR | G6 credential boundary | G6 | PRE_G6 |
| Security Audit finalization | G6 tamper-evident record | G6 | PRE_G6 |
| Resource Engine | G9 retrieval support; "Not yet materialized" | G9 | PRE_G9 |
| Adaptive Model Router + DeviceResource / ModelResidency | routing constrains provider selection; gate unassigned | unassigned | PRE_RELEVANT_GATE |
| Tools / Skills registry | capability surface; gate unassigned | unassigned | PRE_RELEVANT_GATE |
| Tool contracts JSON Schema decision (SoT §1 decision table) | constrains all future tool work | unassigned | PRE_RELEVANT_GATE |
| Frontend stack + design system (Tauri 2 / React / Vite / own system) | constrains all UI work; UI follows agentctl per ADR-0016 | UI phase | PRE_RELEVANT_GATE |
| Security Runner | restricted mutative-test boundary; explicitly deferred, no gate | deferred | DEFERRED_NON_BLOCKER |
| Web Acquisition | accepted boundary, implementation deferred | deferred | DEFERRED_NON_BLOCKER |
| Document Worker | accepted boundary, implementation deferred | deferred | DEFERRED_NON_BLOCKER |
| Procedure Engine | explicitly deferred target | deferred | DEFERRED_NON_BLOCKER |
| Living Documentation / Documentation Engine | explicitly deferred target | deferred | DEFERRED_NON_BLOCKER |
| Extension-contract architecture (small API layer, INV-069) | principle exists as invariant; formal ADR optional | deferred | DEFERRED_NON_BLOCKER |
| Artifact Store | explicitly deferred target | deferred | DEFERRED_NON_BLOCKER |
| External documentation source | research-phase concern | deferred | DEFERRED_NON_BLOCKER |
| Local Inference port | optional adapter by design | deferred | DEFERRED_NON_BLOCKER |
| TOON/TRON renderer choice | explicitly future-research | research | RESEARCH_ONLY |

Not candidates (explicitly post-v1, correctly without ADRs): Tournament Mode,
Consensus Mode, Shadow Agents, Semantic Conflict Detection.

Literal SoT criterion "all current architectural decisions represented by
ADRs" is therefore not fully satisfied — even though no single missing ADR
blocks G1 (G1 needs none of the above).

## Open Design Items by Earliest Gate

Prioritization only; nothing resolved here.

- **G1** (must resolve to implement start/handshake/ping/version/reconnect/
  shutdown): wire framing; code-generation strategy + Protobuf toolchain
  pinning; UDS runtime path strategy; Named Pipe naming/security descriptors;
  maximum message size; read/write timeouts; stale-socket cleanup;
  multiple-agentd-instance prevention; daemon-start race handling; reconnect
  backoff; shutdown authorization + timeout/graceful behavior; client_kind
  registry; multiple-client semantics; request/response correlation IDs
  (evaluate — defer if G1 stays non-multiplexed).
- **G2**: Task schema; Event schema; exact IPC peer-authentication (before
  privileged business commands); error taxonomy beyond handshake rejection;
  TASK-0006 CLAIMED-lease / stale-claim / retry-budget items as the Task Core
  scheduler needs them.
- **G3**: Planning/Orchestration ADR; Evaluation ADR; FakeAgent placement and
  scenario-configuration items.
- **G4**: `workspace.md` contract (SoT §101 plans it); worktree failure-mode
  test coverage per ADR-0008.
- **G5**: provider port-shape / capability / error / availability items
  (already open in providers.md).
- **G6+**: Policy/Secrets/Audit ADRs; sandbox backend matrix; device
  signing-key lifecycle; audit retention; data-at-rest encryption.
- **Deferred**: telemetry schema; sync encrypted-data model; remote-worker
  protocol; plugin permission model; crash-report policy; export format;
  subprocessor mapping; version deprecation policy; doc-engine / security-
  runner / procedure ADRs; TOON research.

## G1 Blockers

Technical (G1 transport/handshake cannot be implemented without them):

1. wire framing over UDS/Named Pipe;
2. code-generation strategy + toolchain pinning;
3. UDS path strategy + Named Pipe naming/descriptors;
4. message size limits + read/write timeouts;
5. stale-socket cleanup + multiple-instance prevention + daemon-start races;
6. shutdown authorization + timeout behavior.

Governance (G0 exit requirement; G1 must execute as queued atomic tasks):

7. roadmap artifact + first atomic task queue including G1 scope (G0-A007, G0-A026).

Explicitly NOT a G1 blocker: exact local peer authentication. ADR-0003
defers per-installation keypair/challenge-response to later, and THR-003
records user-private endpoint + permissions as the accepted interim posture —
sufficient for the handshake/ping/shutdown surface. Peer-auth becomes
required PRE_G2, before privileged business commands exist.

## Non-Blocking Later Work

Schemas (PRE_G2), Context/Continuity contracts (PRE_G9), ADR waves PRE_G3
(Planning, Evaluation) and PRE_G6 (Policy, Secrets, Audit), workspace.md
(PRE_G4), all DEFERRED_NON_BLOCKER and RESEARCH_ONLY candidates above, and
the SoT §101 document-decomposition program (orchestration, memory, policy,
evaluation, storage, SECURITY_TEST_PLAN) as their gates approach. None of
this blocks G1 readiness decisions.

## Recommended Follow-Up Task Sequence

- **TASK-0011 — G0 governance closure.** Goal: roadmap artifact + first atomic
  task queue. Why: satisfies G0-A007/G0-A026; G1 must run as queued atomic
  tasks (§96, INV-108/INV-114). Must first resolve the ROADMAP.yaml vs
  `planning/roadmap.yaml` ambiguity (OPEN GOVERNANCE DETAIL). Dependencies:
  none. Blast radius: 1–2 new planning files + one INDEX link.
- **TASK-0012 — Task + Event schemas.** Goal: machine-readable canonical Task
  schema and persistent-event envelope schema. Why: satisfies G0-A009/G0-A010;
  G2 Task Core needs both. Dependencies: none. Blast radius: new schema
  files (do not invent storage layout beyond schema) + INDEX links.
- **TASK-0013 — Context + Continuity contracts.** Goal: `context-engine.md`
  and `continuity.md` per SoT §101. Why: satisfies G0-A013/G0-A023; enables
  G9. Dependencies: none. Blast radius: 2 docs + INDEX links.
- **TASK-0014 — G1 IPC implementation-readiness.** Goal: decide framing,
  codegen + pinning, endpoint paths, limits/timeouts, stale-socket /
  multi-instance / start-race behavior, shutdown authorization. Why: clears
  the 6 technical G1 blockers. Dependencies: TASK-0011 (queue). Blast
  radius: `ipc.md` (+ ADR-0003 only if a decision changes; no transport code
  — that is G1 itself).
- **TASK-0015 — ADR wave 1 (PRE_G3).** Goal: Planning/Orchestration ADR +
  Evaluation ADR. Why: G3 scheduler and Submit→Verify flow need decision
  stability. Dependencies: none (parallelizable with 0012–0014). Blast
  radius: 2 ADR files + index rows.

Later ADR waves (PRE_G6 Policy/Secrets/Audit; deferred candidates) intentionally
have no task numbers yet — schedule them as their gates approach.

## Verification Evidence

- `find` tree inventory: 45 repo files; no `planning/`, no `ROADMAP*`, no
  `TASK-*.md`, no `docs/development/` (created by this audit), no
  `contracts/`, one `.proto`, 16/16 ADRs, 35 markdown files.
- Invariant census (script): 144 SoT IDs vs 144 `**INV-n**` occurrences in
  INVARIANTS.md — exact match, zero missing, zero extra, zero duplicates.
- ADR status loop: all 16 files contain `Status: Accepted`.
- Link health (script): 35 files, 0 broken links.
- Consistency grep: potentially-violating strings (`complete_task()`,
  `sandboxed = true`, `localhost HTTP`, `ClaudeTask…`, `…Completed`) occur
  only inside prohibitions; `RETRY_READY` defined only in task-engine.md.
- `protoc` (libprotoc 36.1): `agentd_control.proto` validates clean to a
  temp descriptor; no committed generated source.
- Cargo baseline: `fmt --check` clean; `check` clean; `test` 3 passed;
  `clippy -D warnings` clean.
- Code inventory: both mains are 11-line version-reporting stubs;
  `lib.rs` holds version primitives + 3 unit tests only.
