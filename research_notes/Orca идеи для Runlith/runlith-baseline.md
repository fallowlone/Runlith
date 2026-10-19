# Runlith Current-State Baseline (local repo, 2026-10-16)

## What is Runlith (purpose, stack, shape)

### Takeaway
Runlith (AI Engineering OS) is a local-first, provider-neutral control plane for durable AI-assisted software engineering in Rust: a modular monolith with Ports & Adapters where `agentd` owns orchestration and `agentctl` is the CLI; Claude/Codex/OpenCode/Gemini are replaceable executors.

### Cited Findings
- Product purpose: "local-first control plane for durable AI-assisted software engineering"; providers are "replaceable executors", value is durable tasks/plans/context/memory/verification/policy/routing — [PROJECT_CHARTER.md](../../PROJECT_CHARTER.md) (Purpose section, lines 9-17)
- User flow: User intent → Plan → Durable Task Graph → Scheduler/Reconciler → Claude/Codex/OpenCode → Verification → Human gate → Ship — [PROJECT_CHARTER.md](../../PROJECT_CHARTER.md) (User outcome section)
- Architecture: "modular Rust monolith with Ports & Adapters", `agentd` owns local orchestration business logic, domain crates never import infrastructure adapters, bounded contexts never read each other's tables — [AGENT.md](../../AGENT.md) (section 4, lines 63-80)
- Workspace has 40 members: 2 bins + ~26 crates + ~12 adapters — [Cargo.toml](../../Cargo.toml) (lines 1-44)
- Authority order: task envelope > INVARIANTS.md > ADRs > source-of-truth doc > module docs > code > assumptions — [AGENT.md](../../AGENT.md) (lines 8-18)
- Orchestrator role is workflow coordination only, never overrides AGENT.md/invariants/ADRs/roadmap — [ORCHESTRATOR.md](../../ORCHESTRATOR.md) (lines 1-16)
- Product version identity: `semver = "0.1.0"`, `development = "0.1.0-dev.task0174"`, `task = "TASK-0174"`, `gate = "G9"` — [product/version.toml](../../product/version.toml) (lines 7-10)
- Version policy: `product/version.toml` is single source; every completed task advances `development` once; public SemVer moves only on explicit release — [docs/development/VERSIONING.md](../../docs/development/VERSIONING.md) (Canonical source + Development version rule sections); enforced by [AGENT.md](../../AGENT.md) (section 26, lines 547-553)
- IPC topology: Tauri Host / agentctl → local typed IPC → agentd; TB2 privilege boundary — [docs/architecture/ipc.md](../../docs/architecture/ipc.md) (sections 1-2, lines 20-48)
- Transport: UDS on macOS/Linux, Named Pipe on Windows, Protobuf messages over framed byte transport, no gRPC/HTTP/TCP — [docs/architecture/ipc.md](../../docs/architecture/ipc.md) (lines 39-48)

### Inferences
- Runlith differentiates on durability/verification/policy, not on model quality; provider neutrality is load-bearing across domain, routing, and secrets rules.
- 40-member workspace is large for "monolith" label; `crates/` mixes domains, app-services, IPC infra, and contract-test suites (not a uniform domain layer).

### Gaps
- Could not confirm live binary behavior (no builds run; read-only constraint). UI/Tauri shell referenced in invariants but no UI package in workspace — status of Tauri implementation unverified.

## Package ownership and key traits

### Takeaway
Ownership: `crates/task` (Task lifecycle), `crates/agent-runtime` (Execution/Attempt), `crates/scheduler` (owns `ExecutionPort`), `crates/plan` (owns `PlannerPort`), `crates/evaluation` (owns `EvaluationPort`/`EffectCapturePort`), `crates/app-services` (trusted mutation: `TaskService`, `OrchestrationService`), `adapters/storage-sqlite` (`SqliteStore`), `bins/agentd` + `bins/agentctl` (composition + CLI).

### Cited Findings
- `crates/` is not uniform domain layer; `app-services` explicitly integrates concrete SQLite adapter — [crates/AGENTS.md](../../crates/AGENTS.md) (lines 8-11)
- Task lifecycle/state machine: `task/src/lib.rs`, package `task-domain`, fixed transition table in `task/src/state.rs` — [crates/AGENTS.md](../../crates/AGENTS.md) (line 17)
- Execution/Attempt snapshots: `agent-runtime/src/lib.rs`, separate from Task lifecycle — [crates/AGENTS.md](../../crates/AGENTS.md) (line 18)
- Scheduler owns `ExecutionPort`, does not persist decisions — [crates/AGENTS.md](../../crates/AGENTS.md) (line 19)
- Planning graph/validation: `plan/AGENTS.md`, package `plan-domain` — [crates/AGENTS.md](../../crates/AGENTS.md) (line 21)
- Evaluation: `EvaluationPort` + capture contracts in `evaluation/AGENTS.md` — [crates/AGENTS.md](../../crates/AGENTS.md) (line 22)
- Policy: capabilities/approvals/test-mutation-policy in `policy/AGENTS.md` — [crates/AGENTS.md](../../crates/AGENTS.md) (line 24)
- Runner consumes already-decided effective capabilities; `NativeRunner` offers no OS sandbox, Seatbelt implements confinement; required isolation cannot degrade to native — [crates/AGENTS.md](../../crates/AGENTS.md) (lines 55-58)
- Secrets: values belong to `SecretBackend`; SQLite owns references/bindings only — [crates/AGENTS.md](../../crates/AGENTS.md) (lines 59-61)
- `TaskService` struct: [crates/app-services/src/task_service.rs](../../crates/app-services/src/task_service.rs) (line 52)
- `OrchestrationService` struct: [crates/app-services/src/orchestration_service.rs](../../crates/app-services/src/orchestration_service.rs) (line 141)
- `ExecutionPort` trait: [crates/scheduler/src/execution_port.rs](../../crates/scheduler/src/execution_port.rs) (line 74); single injected executor, exactly one terminal behavior per `execute()` call, no persistence/Task access/journal writes (lines 74-90)
- `PlannerPort` trait (proposal-only; validated app-services mutate graph): [crates/plan/src/model.rs](../../crates/plan/src/model.rs) (line 638; methods `assess_atomicity`, `propose_decomposition`, `propose_replan`)
- `EvaluationPort` trait (pure judgment, no persistence/mutation; `evaluate` + fail-closed `revalidate_before_completion`): [crates/evaluation/src/port.rs](../../crates/evaluation/src/port.rs) (lines 11-30)
- `EffectCapturePort`: [crates/evaluation/src/capture.rs](../../crates/evaluation/src/capture.rs) (line 174)
- `IsolationBackend` (`enforce` + `sandbox`; backend that cannot restrict must refuse): [crates/runner/src/lib.rs](../../crates/runner/src/lib.rs) (lines 626-642); `enforce_process_operation` couples enforcement to privileged op in same call (lines 644-660)
- `SecretBackend`: [crates/secrets/src/lib.rs](../../crates/secrets/src/lib.rs) (line 217)
- `GitPort`: [crates/workspace/src/port.rs](../../crates/workspace/src/port.rs) (line 117)
- `SqliteStore`: [adapters/storage-sqlite/src/task_store.rs](../../adapters/storage-sqlite/src/task_store.rs) (line 34); persists state + appends journal record in one transaction, no journal-append path outside that transaction (lines 7-11)
- Adapter→port map: storage-sqlite, provider-claude→ExecutionPort+PlannerPort, git-cli→GitPort, fake-agent (bridged via test-side `ContractFakeExecutor`, not production impl), fake-planner, fake-evaluator, effect-capture-local, keychain-native, runner-native, runner-macos-seatbelt — [adapters/AGENTS.md](../../adapters/AGENTS.md) (lines 8-19)
- Shared contract suites live in `crates/` (`execution-contract`, `planner-contract`, `effect-capture-contract`); adapter integration targets supply implementations — [adapters/AGENTS.md](../../adapters/AGENTS.md) (lines 23-30)
- agentd: entry [bins/agentd/src/main.rs](../../bins/agentd/src/main.rs), composition [bins/agentd/src/lib.rs](../../bins/agentd/src/lib.rs), dispatch [bins/agentd/src/dispatch.rs](../../bins/agentd/src/dispatch.rs), execution wiring [bins/agentd/src/execution.rs](../../bins/agentd/src/execution.rs) — [bins/AGENTS.md](../../bins/AGENTS.md) (lines 10-16)
- agentctl: shell [bins/agentctl/src/main.rs](../../bins/agentctl/src/main.rs), negotiated clients [bins/agentctl/src/lib.rs](../../bins/agentctl/src/lib.rs); reconnect = new transport + handshake; ambiguous mutation replies are not replayed — [bins/AGENTS.md](../../bins/AGENTS.md) (lines 18, 28-31)

### Inferences
- Port ownership is deliberately scattered (each bounded context owns its port; `*-contract` crates hold test runners, not ports) — reduces coupling but requires navigation docs.
- `app-services` depending on concrete SQLite adapter breaks pure Ports & Adapters layering; documented as explicit exception.

### Gaps
- Did not enumerate all 40 crates' APIs; focused on the six traits named in the assignment plus adjacent ports found by grep.

## Execution/scheduling/admission, storage/journal, contracts

### Takeaway
Flow: agentctl negotiated IPC → agentd dispatch → `OrchestrationService::enter_execution` atomic admission (READY→CLAIMED→RUNNING + execution + attempt reservation in one IMMEDIATE transaction) → workspace materialization → provider execution via `ExecutionPort` → candidate evaluation → independent `EvaluationPort` verdict → completion authority outside agent. Storage: SQLite current-state + append-only Event Journal (atomic commit) + separate Security Audit. Contracts: JSON Schema (Task/Event, human/machine boundary) vs Protobuf (IPC wire: 9 proto files, negotiated versions v0-v3+).

### Cited Findings
- Core invariant: `Task != Execution != Attempt != Session` — [docs/architecture/task-engine.md](../../docs/architecture/task-engine.md) (lines 18-32); also [AGENT.md](../../AGENT.md) (line 67), [docs/architecture/INVARIANTS.md](../../docs/architecture/INVARIANTS.md) (INV-002)
- Task owns durable work/acceptance; survives restarts/provider migration — [docs/architecture/task-engine.md](../../docs/architecture/task-engine.md) (lines 85-96)
- Execution = durable orchestration instance; Attempt = one concrete try; Session = ephemeral provider detail — [docs/architecture/task-engine.md](../../docs/architecture/task-engine.md) (lines 92-110)
- Provider refs travel only as opaque `ProviderRef`/`external_session_ref`; `claude_session_id` on Task is a domain violation — [docs/architecture/task-engine.md](../../docs/architecture/task-engine.md) (lines 78-81)
- Atomic admission: claim + start + execution + attempt reservation; enforced by SQLite admission transaction before `create_attempt` — [crates/app-services/src/orchestration_service.rs](../../crates/app-services/src/orchestration_service.rs) (lines 24, 105, 193, 673-827)
- Agent cannot mark own Task completed; completion authority split — [docs/architecture/INVARIANTS.md](../../docs/architecture/INVARIANTS.md) (INV-009); [docs/architecture/task-engine.md](../../docs/architecture/task-engine.md) (completion-authority sections)
- Persistence model: authoritative relational current state + append-only Event Journal + separate Security Audit; explicitly NOT full Event Sourcing in v1 — [docs/architecture/events.md](../../docs/architecture/events.md) (sections 1-2)
- Journal purpose: auditability/debugging/causal tracing/recovery/historical inspection/projection/future replay — [docs/architecture/events.md](../../docs/architecture/events.md) (section 3)
- SQLite migrations: 33 files `0001_task_core.sql` … `0033_observed_effect_fingerprint.sql` (incl. `0008_approvals`, `0009_secret_refs`, `0010_security_audit`, `0012_plan_persistence`, `0028_context_selection`, `0029_continuity`, `0030_takeover`, `0031_journal_append_only`, `0032_journal_hash_chain`) — [adapters/storage-sqlite/migrations/](../../adapters/storage-sqlite/migrations/)
- Git/filesystem mutation and SQLite state are separate failure domains requiring reconciliation, never one ACID transaction — [docs/architecture/INVARIANTS.md](../../docs/architecture/INVARIANTS.md) (INV-145)
- JSON Schema contracts: [contracts/json-schema/task.schema.json](../../contracts/json-schema/task.schema.json), [contracts/json-schema/event.schema.json](../../contracts/json-schema/event.schema.json); routed from [docs/architecture/INDEX.md](../../docs/architecture/INDEX.md) (lines 61-65)
- Protobuf wire: 9 proto files (`agentd_control`, `task_core`, `execution_core`, `planning_core`, `policy_core`, `vault_core`, `finder_core`, `human_decision_core`, `task_authoring_core`) — [crates/ipc-contracts/proto/](../../crates/ipc-contracts/proto/)
- `.proto` sources are canonical; generated Rust into OUT_DIR via vendored protoc + Prost is disposable — [crates/AGENTS.md](../../crates/AGENTS.md) (lines 36-37)
- Handshake: every new stream begins with `HandshakeRequest`; one handshake per connection; reconnect = new transport + new handshake — [docs/architecture/ipc.md](../../docs/architecture/ipc.md) (lines 74-79)
- Negotiation: client advertises `supported_protocol_versions` + descriptive `client_kind`; daemon selects highest common version or rejects with reason — [docs/architecture/ipc.md](../../docs/architecture/ipc.md) (lines 81-94); bootstrap v0 preserved, v1 business IPC per ADR-0021 — [planning/tasks/queue.yaml](../../planning/tasks/queue.yaml) (TASK-0027 entry, lines 250-271)
- `client_kind` is descriptive metadata, never authorization; peer auth comes from transport (OS-user principal, ADR-0020, fail-closed) — [docs/architecture/ipc.md](../../docs/architecture/ipc.md) (lines 96-100); [crates/AGENTS.md](../../crates/AGENTS.md) (lines 38-42)
- Safety: secrets never enter model context/logs/event payloads (INV-008); prompt instructions never the only control (INV-013); required isolation fails closed (INV-076); policy preflight alone is not enforcement (INV-153) — [docs/architecture/INVARIANTS.md](../../docs/architecture/INVARIANTS.md) (lines 139-149)
- Evaluation: generator must never implement `EvaluationPort` for its own output; G8 adds real verifiers behind same seam without touching completion authority — [crates/evaluation/src/port.rs](../../crates/evaluation/src/port.rs) (lines 1-9)
- Scheduler: assignment/orchestration + trusted evaluation flow runtime evidence → submit_result → verification — [planning/tasks/queue.yaml](../../planning/tasks/queue.yaml) (TASK-0037, lines 434-448)

### Inferences
- Dual-contract strategy: JSON Schema for durable domain legibility, Protobuf for versioned IPC wire; negotiation + append-only journal + hash chain give migration-friendly durability without full event sourcing.
- Admission atomicity (G5 remediations) is the correctness boundary against concurrent duplicate execution/attempt identities.

### Gaps
- Exact current protocol version set (v0/v1/v2/v3 surface per proto) not enumerated; only v0-bootstrap and v1-business confirmed from queue history. Candidate/side-effect protocol (G8 ADRs 0030-0036) referenced in INDEX but not read in detail.

## Verification gate, roadmap/queue scope, limits and TODOs

### Takeaway
Gate: `cargo fmt --check` + `cargo clippy --workspace --all-targets --locked -D warnings` + `cargo test --workspace --locked --no-fail-fast` on macOS Rust 1.95.0 (CI reports after the fact, gates nothing). Roadmap: G0-G8 closed, G9 (Context Engine + Continuity) in progress through TASK-0175, G10-G14 planned. Known limits: macOS-only confinement (Seatbelt), no vector DB default, no full event sourcing in v1, cloud optional.

### Cited Findings
- Verification commands: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --locked --no-fail-fast` — [.github/workflows/verification.yml](../../.github/workflows/verification.yml) (lines 61-75)
- Toolchain pinned in CI: Rust 1.95.0; repo declares no rust-toolchain/MSRV — [.github/workflows/verification.yml](../../.github/workflows/verification.yml) (lines 52-59)
- CI runs on `macos-latest`, 45-min timeout; real-provider smokes stay ignored (need `RUNLITH_CLAUDE_SMOKE=1` + authenticated CLI) — [.github/workflows/verification.yml](../../.github/workflows/verification.yml) (lines 39-40, 70-75)
- CI "reports after the fact and gates nothing: acceptance still rests on the operator's local run" — [.github/workflows/verification.yml](../../.github/workflows/verification.yml) (lines 1-4)
- Only mandatory-confinement backend is macOS Seatbelt; non-macOS uses trusted-host NativeRunner which the shared verifier refuses for confined launch; non-macOS arm verified only by inspection — [.github/workflows/verification.yml](../../.github/workflows/verification.yml) (lines 24-31)
- Testing discipline: RED→GREEN→REFACTOR default for domain/bugfix; every adapter implementing a shared port must pass shared contract suite — [AGENT.md](../../AGENT.md) (sections 6, lines 100-113)
- Roadmap gates: G0 Architecture Freeze … G8 Verification completed; G9 Context Engine + Continuity in_progress; G10 Memory, G11 Background daemon, G12 Remote Worker, G13 Cloud/Team, G14 Router/Usage Economics planned — [planning/roadmap.yaml](../../planning/roadmap.yaml) (lines 14-166)
- Queue: 164 TASK items, `current_gate: G9`, `gate_status: IN_PROGRESS` — [planning/tasks/queue.yaml](../../planning/tasks/queue.yaml) (lines 11-12); G8 closed by TASK-0127, G9 activated by TASK-0167 (line 12 comment)
- Current head: TASK-0174 done (finder index + redaction filter, full workspace verification green with daemon-DB-override note), TASK-0175 (planner evidence resolution boundary, ADR-0029 option B) appears open/next — [planning/tasks/queue.yaml](../../planning/tasks/queue.yaml) (lines 6038-6061 tail + TASK-0174 outcome)
- G9 scope: Context Provenance Envelope, dogfoodable repo finder (no vector DB default), conditional entity graph only if justified, TAKEOVER/HandoffPacket deferred from G7 — [planning/roadmap.yaml](../../planning/roadmap.yaml) (lines 106-128)
- Guardrails: every capability must produce dogfood value (DOGFOOD_DETACHMENT P1); full Event Sourcing stays future-only; entity graph conditional — [planning/roadmap.yaml](../../planning/roadmap.yaml) (lines 202-220)
- G14 router: eligibility-first, explainable, offline-first shadow-mode evaluation; no live/learned routing until shadow evidence reviewed — [planning/roadmap.yaml](../../planning/roadmap.yaml) (lines 152-166)
- Historical hardening signals: G5 needed entry-atomicity/case-fold/recovery remediations (TASK-0051/0053/0054) before G5_CLOSED at TASK-0059; G6 closed only after isolation remediation (B-ISO-1: NativeRunner had zero sandbox axes) — [planning/tasks/queue.yaml](../../planning/tasks/queue.yaml) (TASK-0051, TASK-0059, TASK-0068/0069 entries)
- Pre-existing issue noted in TASK-0174 outcome: canonical v27 dev-database fails the 0030 migration; benchmarks ran under `AI_ENGINEERING_OS_{RUNTIME,DATA}_DIR` overrides; reproduced on unmodified binary, out of scope — [planning/tasks/queue.yaml](../../planning/tasks/queue.yaml) (TASK-0173/TASK-0174 outcome fields)
- No TODO/FIXME/`todo!`/`unimplemented!` hits in `app-services`, `storage-sqlite`, `agentd` sources (grep empty) — repo root (grep evidence, 2026-10-16)
- agentd lib header still says "No providers, Runner, Memory, Context, Policy Engine (G6), Execution/Attempt/Session runtimes, or AI of any kind" — [bins/agentd/src/lib.rs](../../bins/agentd/src/lib.rs) (lines 1-14) — stale comment (all those now exist), i.e. doc drift.

### Inferences
- Gate process is audit-heavy: each gate closes via independent (sometimes adversarial) closure audit + remediation + re-audit cycle; queue `outcome` fields carry the evidence trail.
- Biggest platform limit is macOS-only confinement; Linux/Windows production isolation has no mandatory backend yet.
- Stale `agentd/src/lib.rs` header + canonical-DB 0030 migration failure are two concrete known-drift items for the report-writer.

### Gaps
- Did not read G7/G8 closure audits or ADR-0029/0042/0043 in full; G9 remaining scope beyond TASK-0175 (takeover, epochs/checkpoints per TASK-0169/0170) confirmed by title only.
- "Unmeasured" reference counts from the compatibility-entrypoint map (OrchestrationService, EvaluationPort, SqliteStore) not re-measured; read-only constraint + tool budget.
- Full test count (~1100 cited in CI comments) not independently verified by running the suite.
