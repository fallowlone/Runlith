# Orca mechanisms worth borrowing as ideas

> Identity note: the assigned URL (https://github.com/stablyai/orca) is Orca ADE, an Electron desktop IDE for parallel agent worktrees, NOT a scheduler/planner library — it has no scheduler/orchestrator/planner, tools/, runners/, or storage/ modules. Its `src/` is `cli/main/packages/preload/renderer/shared/types/wsl-guest`. Two same-named adjacent projects match the assignment's mechanism questions far better: `orca-cli/orca` (Go single-binary orchestration layer: runs, pods/DAGs, SQLite state, worktrees, MCP) and `VirtusLab/orca` (Scala 3 agent-flow library: stage-bound runtime, capability gating, progress log + event log, per-backend session durability). All three are covered below; each finding names which Orca it comes from. Ideas only, no verbatim code.

## How does Orca do planning/orchestration?

### Takeaway
None of the three Orcas has an LLM planner-prompt/task-decomposition engine; orchestration is structural (namespaces, task states, DAG dependencies, dispatches, decision gates) and the agent does the decomposing, while the system owns isolation, completion authority, and recovery.

### Cited Findings
- stablyai/orca orchestration core model: a Run is a durable namespace + home inbox that never schedules or places workers; a Task has spec, dependencies, status (`pending/ready/dispatched/completed/failed/blocked`); a Dispatch is one attempt of a task on a terminal and holds lifecycle authority for `worker_done`/heartbeat; Messages are inbox mail; decision gates are coordinator-owned questions blocking a task until resolved — [Source](https://www.onorca.dev/docs/cli/orchestration)
- stablyai/orca completion authority comes from the active dispatch context: worker completion and heartbeat messages must carry both `taskId` and `dispatchId` so stale retries cannot complete the wrong dispatch — [Source](https://www.onorca.dev/docs/cli/orchestration)
- stablyai/orca worker contract: dispatched workers get a preamble telling them to send `worker_done` exactly once even on failure with `--outcome succeeded|failed`, include a short body summary, include both IDs, send heartbeats during long work, and use `ask` for blocking questions instead of local TUI prompts — [Source](https://www.onorca.dev/docs/cli/orchestration)
- stablyai/orca messaging: default `check` returns the bound Run's oldest unacked Delivery (FIFO) with replay-until-ack; `--peek`/`--all` do not consume; group addresses (`@all`, `@idle`, per-agent, per-worktree) exist but are forbidden for `worker_done`/heartbeat — [Source](https://www.onorca.dev/docs/cli/orchestration)
- stablyai/orca retry placement is explicit (`--retry-of` does not inherit worktree/placement), settled workers are released (archiving inspectable output, closing only the coordinator-owned terminal) and re-read via `worker-read` rather than leaving terminals open — [Source](https://www.onorca.dev/docs/cli/orchestration)
- orca-cli/orca run lifecycle state machine: `queued → running → ready → shipped`, with `blocked` (waits on upstream pod dependency) and `failed → retry` (retry carries feedback) — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- orca-cli/orca pod flow: `pod create` decomposes a goal into a DAG of runs queued with dependency ordering; agents pick up work as upstream runs complete; constraints gate what reaches review; `ship` opens the PR — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- orca-cli/orca module layout shows dedicated `internal/pod`, `internal/runner`, `internal/state`, `internal/constraint`, `internal/cli` packages — [Source](https://api.github.com/repos/orca-cli/orca/contents/internal?ref=main)
- VirtusLab/orca user surface is three rungs: one-shot run, ephemeral fork-safe chat (InStage-only), durable flow-thread-only session; stage-bound runtime specified in ADR 0018 — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca review loop has a fixed vocabulary: finding (one reported problem), declined (fixer refused with reason), open finding (unresolved at run end, paired with an OpenReason: declined, never-reported, past round cap, still-failing, custom, or skipped) — surfaced in PR body and run output — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)

### Inferences
- The dispatch-ID-on-every-completion pattern is a cheap, transport-agnostic idempotency mechanism Runlith could adopt at its own execution seam without schema churn.
- Explicit-retry (no inherited placement) plus release-then-read discipline avoids two classic orchestration bugs: retries landing in the wrong workspace, and leaked worker terminals held open as "logs".
- Decision gates as first-class blocked-state objects (separate from worker questions via `ask`) give the DAG a human-in-the-loop point that survives worker restarts.

### Gaps
- No planner prompts, decomposition heuristics, scheduling policies, or workspace-admission policies found in any of the three Orcas; I could not clone repos (no shell) so scheduling internals (e.g. orca-cli pod dependency ordering implementation) were not inspected beyond module names.
- orca-cli/orca README links a full CLI reference `DOCS.md` that does not exist at repo root (404 on both HTML and raw URLs) — deeper command semantics unverified.

## What tools/capture mechanisms exist?

### Takeaway
stablyai/orca treats the IDE itself as the tool ecosystem (per-worktree browser with Design Mode element capture, computer-use, scriptable CLI, skills registry over MCP), orca-cli/orca standardizes review capture (diff + logs + tests + context per run), and VirtusLab/orca enforces tool restriction per backend with a rendered, test-pinned guarantee matrix.

### Cited Findings
- stablyai/orca Design Mode: click any UI element in a real Chromium window to send its HTML, CSS, and a cropped screenshot straight into the agent's prompt — [Source](https://github.com/stablyai/orca) (README feature table)
- stablyai/orca Computer Use: agents operate desktop apps and visible UI when a workflow needs real interaction — [Source](https://github.com/stablyai/orca) (README feature list)
- stablyai/orca Orca CLI lets agents drive Orca itself (`worktree create`, `snapshot`, `click`, `fill`); full CLI reference and skills registry/MCP docs exist — [Source](https://github.com/stablyai/orca) (README); [Source](https://www.onorca.dev/docs/cli/worktree-checkpoints) (docs nav)
- stablyai/orca worktree checkpoints: every worktree carries a free-text comment (status snapshot) plus card status (`todo/in-progress/in-review/completed`); agents update it via `worktree set --comment`; recommended checkpoint moments are slice-done, hypothesis confirmed/refuted, review done, blocker hit, phase transition; read-before-write to avoid clobbering user context — [Source](https://www.onorca.dev/docs/cli/worktree-checkpoints)
- stablyai/orca annotate-AI-diff: drop comments on any diff line and ship them back to the agent — [Source](https://github.com/stablyai/orca) (README feature table)
- stablyai/orca usage and rate-limit tracking with account hot-swap for Claude/Codex without re-login — [Source](https://github.com/stablyai/orca) (README feature list)
- orca-cli/orca `review` opens a run's diff, logs, tests, and context as one bundle; `logs` streams agent logs in real time; TUI has Watch / Pod-DAG / Review / New-Run views — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- orca-cli/orca `.orca/` per-repo config: `AGENTS.md` injected into every run, `orca.toml` (default adapter, concurrency limits, worktree cleanup policy), `skills/` patterns, `policies/` YAML validation rules checked by `config validate` before a run may ship, `templates/` injected into run context — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- orca-cli/orca context-loss survival: mandatory instruction-file snippets force the agent to re-list runs on every session start and stage work after every change, so compacted sessions re-establish state instead of orphaning worktrees — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- VirtusLab/orca tool enforcement: requested `ToolSet` (ReadOnly/NetworkOnly/Full/NoTools) × `AutoApprove` is answered per backend by an `enforcementCell` with the guarantee actually achieved (Hard/PromptOnly/SandboxApprox/Ignored); the full matrix is rendered by a test walking the backend×tools×approve×dispatch product, and turns that asked for more than the backend can enforce get an EnforcementNotice — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca turn-event grammar (message boundaries, optional tool names) is specified on `TurnEvent` and pinned per backend by conformance assertions in each module's tests — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)

### Inferences
- The enforcement-matrix-as-rendered-test-output pattern is directly reusable: Runlith provider backends could publish a capability/guarantee matrix generated by tests rather than hand-maintained docs that drift.
- Worktree checkpoints (free-text comment + phase status, agent-writable, read-before-write) are a lighter-weight alternative to full event sourcing for human-facing progress where Runlith needs glanceable state.
- Ship-gating policies (`config validate` before ship) map cleanly onto Runlith evaluation/verification as a pre-ship gate rather than a post-hoc check.

### Gaps
- No file/git/shell/browser tool implementations inspected (no shell to clone; stablyai `src/main` internals unexplored beyond top-level dirs) — tool granularity, approval flows, and output parsing are unverified.
- orca-cli `policies/arch-rules.yaml` semantics and `constraint` package behavior not inspected beyond names.

## How are runners/providers abstracted?

### Takeaway
All three Orcas are agent-agnostic at the boundary (any CLI agent works), but differ in mechanism: stablyai federates workers across local/SSH/remote hosts with per-launch model/effort overrides; orca-cli uses per-agent adapters behind one binary with run state in SQLite; VirtusLab isolates backends in one module each behind a private LLM SPI with per-backend session durability schemes.

### Cited Findings
- stablyai/orca works with any CLI agent (Claude Code, Codex, OpenCode, Pi, Cursor, Gemini, 20+ named) — agents run in terminals/worktrees, not through an LLM API — [Source](https://github.com/stablyai/orca) (README supported-agents)
- stablyai/orca federated workers: `worker-start --on <host> --worktree new-top-level --repo <selector>` places supervised workers on other hosts (including Windows); later commands route by Dispatch ID; per-worker `--model` (opaque provider IDs) and `--effort` overrides apply to that launch only and appear in the start receipt under `launch.requested/launch.effective`; federated starts need a host advertising launch-preference support — [Source](https://www.onorca.dev/docs/cli/orchestration)
- stablyai/orca SSH worktrees run agents on a remote box with auto-reconnect and port forwarding; remote Orca servers and session restore (scrollback/agents survive restarts) documented as first-class model concepts — [Source](https://github.com/stablyai/orca) (README); [Source](https://www.onorca.dev/docs/cli/worktree-checkpoints) (docs nav: SSH worktrees, Remote Orca Servers, Session restore)
- orca-cli/orca architecture: single Go binary; run state in `~/.orca/orca.db` (SQLite + FTS5); isolation in `.orca/runs/<id>/` git worktrees; adapters for Claude Code, Codex, Aider, OpenCode, Cursor via MCP stdio — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- orca-cli/orca module layout has dedicated `internal/adapter`, `internal/runner`, `internal/mcp`, `internal/store`, `internal/state`, `internal/worktree` packages — [Source](https://api.github.com/repos/orca-cli/orca/contents/internal?ref=main)
- VirtusLab/orca has one module per coding-agent backend (`claude/codex/gemini/opencode/pi`), each depending only on `tools`; the LLM SPI is `private[orca]` so new harnesses are added inside orca itself; terminal UI sits behind an `Interaction` interface so Slack/HTTP can substitute for the terminal with one call-site change — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)

### Inferences
- Per-launch model/effort overrides recorded in a start receipt (`requested` vs `effective`) is a clean pattern for Runlith model routing: it makes fallback/override decisions auditable instead of silent.
- Host capability advertisement (launch-preference support) before federated placement is a minimal admission-control idea Runlith could borrow for heterogeneous runners.
- The private-SPI-plus-one-module-per-backend layout mirrors Runlith's Ports & Adapters shape and validates keeping provider knowledge (model ids, cheap tiers, wire quirks) on the backend, not in core.

### Gaps
- No model routing, fallback chains, caching, or budget mechanisms found in any Orca — no evidence of token/cost budgets except VirtusLab `Turn` events carrying token usage and cost into the event log (see next section). Caching/retries at the LLM-call level are a gap.
- orca-cli adapter/store internals and stablyai remote-server protocol not inspected.

## What correctness mechanisms exist?

### Takeaway
This is the richest area: VirtusLab/orca contributes compile-time capability gating plus a two-file resume design (committed progress log + machine-local event log) with typed run/attempt vocabulary; orca-cli contributes SQLite + FTS5 state with an archival kill path; stablyai contributes FIFO-ack inbox delivery and dispatch-scoped completion authority.

### Cited Findings
- VirtusLab/orca capability gating: three compile-time capabilities — `FlowControl` (authority to start a stage, thread-affine), `InStage` shared-half token (required to spend tokens/drive an agent, fork-capturable), `WorkspaceWrite` exclusive-half token (required for every git/fs/gh/progress-log write, must NOT cross a fork boundary so two forks cannot race on one git index); only a `stage` body mints both, through the single `RuntimeInStage` door; violations fail compilation (pinned by negative tests) — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca progress log + recovery: each run commits `.orca/runs/<key>.progress.json` (keyed by prompt hash, branch-independent) with one entry per completed stage; resumes replay recorded entries and skip them; the header is treated as untrusted and validated by `RecoveryCheck` (prompt-hash match, protected-branch refusal) before any destructive git op — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca resume state is two files by design: the progress log must be committed (resume from a pushed branch works) while backend session ids are machine-local handles that a failing stage's `reset --hard` would erase — so session records live in machine-local cache `.orca/cache/runs/<key>/events.jsonl` (append-only JSONL: AttemptStarted, BranchBound, StageStarted/Ended, SessionMinted/SessionWireId, SessionCommitted, Turn with token usage and cost, RunSucceeded, AttemptFinished), a public additive-only format within one schema number — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca session identity: sessions keyed by (name, stage-path) so two stages cannot share a name; mint-twice-in-one-stage throws; reuse across attempts rehydrates the wire id with probe-then-confirm semantics distinguishing ThisAttempt vs EarlierAttempt conversations; ClientClaimed (client id IS wire id: claude/pi) vs ServerMinted (codex/gemini/opencode) id schemes — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca safety backstops: listener that throws is quarantined (logged, announced once, excluded for rest of run, flow survives); every spawned child carries a unique turn cookie and turn end scans `/proc/*/environ` for survivors of detached agent work (report-only unless kill flag set, Linux only); subprocesses must capture stderr so output cannot tear the terminal renderer; all `.orca/` writes go through `OrcaDir` which refuses symlinked directories and atomically replaces files — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca concurrency: single OS file lock per run (`FlowLock`, holder pid, OS-released on exit) plus per-worktree resolution locks; run vs attempt vocabulary strictly separated (run = one prompt across processes, keyed by prompt hash; attempt = one process, keyed by start-ms + pid); 0.x versioning rule bans default values on persisted fields so forgotten fields fail loudly at call sites — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- orca-cli/orca stores run state in SQLite with FTS5 full-text search (`~/.orca/orca.db`); `kill` cancels a run and archives its worktree rather than deleting it — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- stablyai/orca completion requires both task and dispatch IDs (stale-retry protection); inbox delivery is FIFO oldest-unacked-first with replay-until-ack and non-consuming peek — [Source](https://www.onorca.dev/docs/cli/orchestration)

### Inferences
- The two-file resume split (committed deterministic log vs machine-local handle cache) directly addresses a failure mode worth checking in Runlith: anything needed for resume must survive both `git reset --hard` and fresh clones, and those two survivors are different files.
- Capability-token gating (compile-time where possible, thread-bound runtime check elsewhere) is a stronger concurrency-control idiom than lock-everywhere: it makes "two writers on one workspace" a type error at the fan-out points that matter.
- Prompt-hash run keys (branch-independent resume), attempt ids encoding (start-ms, pid), additive-only event schema, and quarantine-instead-of-crash listeners are each small, independently adoptable mechanisms.

### Gaps
- No idempotency keys, leases with expiry, transactions, migrations, or compatibility shims found beyond the above; orca-cli SQLite schema/migration approach and stablyai orchestration persistence internals not inspected (no shell access to clone).

## Which mechanisms look mature/better than a typical Rust modular monolith and why?

### Takeaway
VirtusLab/orca's correctness engineering (capability tokens, two-file resume, quarantine listeners, turn-cookie sweep) and orca-cli's operational simplicity (one binary + SQLite/FTS5 + worktrees + MCP) each exceed what a typical modular monolith ships; stablyai's dispatch-scoped completion and checkpoint conventions are the most directly portable ideas.

### Cited Findings
- VirtusLab/orca renders its backend-enforcement guarantee matrix from a test that walks the full product space instead of hand-maintaining docs — evidence of maturity: doc/code drift is structurally impossible — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- VirtusLab/orca pins wire fragility with per-backend `TurnEventConformance` tests and documents a real case (a Claude CLI auto-update rejecting `$schema: 2020-12` and emitting a new visible tool call) with a fix-at-the-seam policy and direct-probe-first debugging rule — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)
- orca-cli/orca ships orchestration as one Go binary with one SQLite file (FTS5-searchable) and git-native isolation — no Node/Python/Docker — with per-repo `AGENTS.md` + `orca.toml` + skills + YAML ship-gate policies + templates — [Source](https://github.com/orca-cli/orca/blob/main/README.md)
- stablyai/orca 94k stars / 6k forks with daily ships and an experimental-but-documented orchestration layer (Run/Task/Dispatch/gates) on top of a stable worktree+terminal substrate — evidence of battle-tested isolation primitives under a thinner orchestration veneer — [Source](https://github.com/stablyai/orca)
- VirtusLab/orca review vocabulary (finding/declined/open-finding with OpenReason) is enforced in identifiers, prompts, screen output, and PR bodies alike, with a test asserting the reason case rather than prose sentences — [Source](https://github.com/VirtusLab/orca/blob/master/AGENTS.md)

### Inferences
- Highest-value borrows for Runlith, ranked: (1) dispatch-scoped completion IDs; (2) committed-log vs machine-local-cache resume split; (3) test-rendered capability matrices; (4) ship-gate policy validation; (5) worker preamble contract (exactly-once `worker_done` + heartbeats + coordinator-routed questions); (6) turn-cookie sweep for orphaned agent subprocesses.
- A typical Rust modular monolith already has what none of the Orcas demonstrate: trusted lifecycle mutation services, evaluation ports, and contract test suites — Runlith's edge is depth of invariant enforcement, and the Orca ideas above plug gaps around multi-agent supervision rather than replacing core architecture.

### Gaps
- Comparative maturity judged from docs and contributor guides, not from running code or reading internals (clone impossible without shell); test-suite sizes, defect rates, and production incident records for all three Orcas are unknown — the report writer should present maturity claims as documentation-evidence only.
- orca-cli/orca is pre-v1.0 with an explicitly unstable interface and a README link to a missing DOCS.md — [Source](https://github.com/orca-cli/orca/blob/main/README.md) — so its mechanisms should be treated as design ideas, not proven implementations.
