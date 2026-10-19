# Orca Architecture and Execution Model

## What is Orca (purpose, stack, layout, entry points)

### Takeaway
Orca is an agentic development environment (ADE): a desktop Electron app that runs many third-party CLI coding agents in parallel, one per git worktree, rather than an LLM provider or scheduler itself.

### Cited Findings
- Orca describes itself as the ADE for working with a fleet of parallel agents, able to run any coding agent with the user's own subscription on desktop, mobile, and remote runtime — [Repo About](https://github.com/stablyai/orca)
- One-line product pitch is running Codex, Claude Code, OpenCode, or Pi side by side, each in its own worktree and tracked in one place — [README](https://github.com/stablyai/orca/blob/main/README.md)
- Stack is TypeScript throughout: Electron 43 + electron-vite build, React 19 renderer, Node 24 engine, pnpm workspaces — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Top-level dirs include `src/`, `config/`, `native/`, `mobile/`, `cloud/`, `docs/`, `tests/`, `skills/`, `resources/`, `Casks/` — [Repo root](https://github.com/stablyai/orca)
- `src/` splits into `main`, `renderer`, `preload`, `cli`, `shared`, `types`, `packages`, `wsl-guest` — [src listing](https://api.github.com/repos/stablyai/orca/contents/src?ref=main)
- Internal package is `@orca/process-host` for subprocess execution, cancellation, and termination — [process-host package.json](https://raw.githubusercontent.com/stablyai/orca/main/src/packages/process-host/package.json)
- Entry binaries: packaged `orca` CLI maps to `./out/cli/index.js`, dev variant to `config/scripts/orca-dev.mjs`, Electron main to `./out/main/index.js` — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Renderer has four HTML entries: main window, pop-out dashboard, space, and web client — [electron.vite.config.ts](https://raw.githubusercontent.com/stablyai/orca/main/electron.vite.config.ts)
- `cloud/` is a separate pnpm workspace holding the mobile-pairing relay, push gateway, Terraform infra, and ops console — [cloud README](https://raw.githubusercontent.com/stablyai/orca/main/cloud/README.md)
- `mobile/` is a separate workspace with its own lockfile, deliberately excluded from the root package graph — [pnpm-workspace.yaml](https://raw.githubusercontent.com/stablyai/orca/main/pnpm-workspace.yaml)
- Repo config surface includes `orca.yaml` (per-repo worktree env recipes) and `.worktreeinclude` (gitignored files to copy into new worktrees) — [worktrees doc](https://www.onorca.dev/docs/model/worktrees)
- Build/test gates include electron-vite builds, tsc project typechecks, vitest, Playwright Electron e2e, oxlint/oxfmt, and generated-contract verifiers for RPC params, ACP protocol, and skill bundles — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)

### Inferences
- Orca owns orchestration UX and process/terminal lifecycle; model inference is delegated to external CLIs the user already pays for.
- Electron main is the trusted host; renderer/preload/CLI/mobile/relay are untrusted or capability-scoped clients around it.

### Gaps
- No ADRs or architecture decision records found in `docs/` (only style guide, audits, reference notes, site content); internal module boundaries beyond `src/*` naming were not mapped file-by-file.
- Exact daemon entry path and main-process composition were not pinned to a file; session-restore doc asserts a background daemon owns PTYs but the code file was not fetched.

## How execution works (lifecycle, scheduling, isolation, concurrency, recovery)

### Takeaway
Isolation unit is the git worktree (optionally on SSH/remote hosts); concurrency unit is the agent session (one CLI process per terminal); durability comes from a PTY-owning background daemon plus session restore, with an opt-in Run/Task/Dispatch orchestration layer for supervised multi-agent work.

### Cited Findings
- Every task gets its own on-disk repo copy via `git worktree`, which is what keeps parallel agents from clobbering each other's files — [worktrees doc](https://www.onorca.dev/docs/model/worktrees)
- Worktree lifecycle is create (with start-from ref picker), work, review (diff vs start-from), ship (commit/push/PR), archive/delete; creation runs `git fetch` + `git worktree add` in background with progress and retry — [worktrees doc](https://www.onorca.dev/docs/model/worktrees)
- New worktrees can share gitignored dependencies via Settings Shared Paths, `worktree.sharedDirectories` in `orca.yaml` (symlink/share), or copy via `.worktreeinclude` — [worktrees doc](https://www.onorca.dev/docs/model/worktrees)
- An agent session is one CLI agent running in one terminal in one worktree, with states working / waiting-on-user / done / blocked / idle derived from OSC terminal titles and agent hooks — [agents-sessions doc](https://www.onorca.dev/docs/model/agents-sessions)
- Session lifecycle is launch from picker, work, idle (fires agent-finished notification), exit (Restart chip rehydrates same agent and cwd) — [agents-sessions doc](https://www.onorca.dev/docs/model/agents-sessions)
- Every supported agent launches with its full-autonomy bypass flag pre-applied (e.g. Claude skip-permissions, Codex bypass-approvals-and-sandbox); the worktree itself is treated as the sandbox — [agents-sessions doc](https://www.onorca.dev/docs/model/agents-sessions)
- Terminals are xterm.js panes with splits/tabs, kitty keyboard protocol, OSC 52 clipboard, link popovers, and scrollback search — [terminal doc](https://www.onorca.dev/docs/terminal)
- A background daemon owns PTYs so quitting, updating, or crashing the UI does not kill agents; next launch warm-reattaches to live processes with scrollback intact — [session-restore doc](https://www.onorca.dev/docs/model/session-restore)
- Host reboot or daemon crash ends agent processes but layout, tabs, splits, and last-persisted scrollback still restore; user re-runs the agent — [session-restore doc](https://www.onorca.dev/docs/model/session-restore)
- Structured orchestration adds Run (durable namespace + inbox), Task (pending/ready/dispatched/completed/failed/blocked with deps), Dispatch (one attempt on a terminal), inbox Messages, and coordinator-owned decision gates — [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- Supervised loop is `run-create` + `task-create` + `worker-start` (onto current, new-child, or federated remote worktree), `check --wait` + `--ack` for completions, `worker_done` exactly once with outcome + task/dispatch IDs, plus `worker-show/read/stop/release/retain` and explicit `--retry-of` retries — [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- Workers can run on other hosts via `--on` + repo selector with dispatch-ID routing afterward; chat-mode launches use structured chat when host supports it — [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- Remote execution exists as SSH worktrees and paired Remote Orca Servers, with host badges on dashboard cards and per-host Quick Commands — [agents-sessions doc](https://www.onorca.dev/docs/model/agents-sessions); [terminal doc](https://www.onorca.dev/docs/terminal)
- Execution-host authority rule: the execution host owns everything touching execution, and loss of contact is never evidence of process death (verdicts live / unverifiable / exited) — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Child processes must go through `runProcess`/`spawnProcess` from `@orca/process-host` (pins Windows flags, refuses `shell:true`, encodes cmd args); ripgrep must go through bundled-rg resolvers; Windows process reads go through a dedicated process-table module — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Git baseline is v2.25 with capability probing, per-host capability cache, and baseline-compatible fallbacks for newer subcommands/options — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)

### Inferences
- No central scheduler with queues/priorities was found; scheduling is user/CLI-driven placement of agents onto worktrees plus coordinator-driven dispatch in the opt-in orchestration layer.
- Sandboxing is filesystem-level (disposable worktree + autonomy flags), not container/VM-level; remote hosts extend placement but inherit the same trust assumption.

### Gaps
- Retry/backoff policy, heartbeat timeouts, and crash-recovery state machine details for orchestration dispatches were not read from source (only CLI-surface behavior from docs).
- Resource limits (max parallel agents, CPU/mem quotas) and admission control were not found.

## State, storage, and IPC/API contracts

### Takeaway
Durable product state lives in side stores per surface (workspace/session files, orchestration runtime state, relay Postgres/SQLite, push-gateway queue), while the live contract surface is the `orca` CLI + typed RPC/stream frames + versioned skill guides, with explicit cross-version wire-compatibility rules.

### Cited Findings
- Session restore persists open worktrees, tab/split layout, focused tab, per-pane scrollback (including output produced while closed), and reattaches to daemon-held processes — [session-restore doc](https://www.onorca.dev/docs/model/session-restore)
- Orchestration Run/Task/Dispatch/message/gate state is runtime-global and durable enough to support wait/ack delivery, preamble re-reads, task-list inspection, and explicit `reset --tasks/--messages/--all` — [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- Relay wire contract (frame shapes, close codes, admission budgets, splice state machine) is shared via `packages/relay-contract` across relay, desktop, and mobile — [cloud README](https://raw.githubusercontent.com/stablyai/orca/main/cloud/README.md)
- Relay pairs phone and desktop over outbound WebSockets through director-assigned cells; storage is PostgreSQL in production, SQLite for tests/local dev — [cloud README](https://raw.githubusercontent.com/stablyai/orca/main/cloud/README.md)
- Push gateway persists one notification event per delivery, enforces per-host quotas, retires unregistered tokens, uses Postgres/SQLite like the relay, and requires queue drain (not migration) before incompatible format deploys — [cloud README](https://raw.githubusercontent.com/stablyai/orca/main/cloud/README.md)
- CLI is the scriptable API: `worktree ps/create/current/set/rm`, `terminal list/read/send/wait/create/split`, `file open/diff`, browser `goto/snapshot/click/fill/screenshot`, emulator, automations, artifacts, and `orchestration *` — [CLI overview](https://www.onorca.dev/docs/cli/overview); [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- Agents must resolve the session's CLI (`ORCA_CLI_COMMAND` / `orca-dev` / `orca`) and load the version-matched guide via `orca skills get <topic> [--full]`; flags live in the binary so guides cannot drift — [skills doc](https://www.onorca.dev/docs/cli/skills)
- Generated contracts are verified in CI: RPC params catalog, ACP protocol snapshot, skill-bundle manifest, localization catalogs — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Mixed client/server versions are the normal state: new optional fields are safe, new stream opcodes must be capability-negotiated (decoders drop unknown opcodes silently), and host-published content changes reach old clients even without wire changes — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Agent status has a single owner: the execution host's hook-server store; sidebar, `worktree ps`, mobile, and dashboard are subscribers holding presentation policy only — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)

### Inferences
- No single DB schema or migration chain for the desktop host was visible from the fetched surface; persistence appears federated per subsystem rather than one journal-backed store.
- CLI-over-local-runtime plus relay WebSocket frames plus MCP registrations form the de facto IPC/API, not a protobuf/gRPC IDL (ACP snapshot notwithstanding).

### Gaps
- Desktop-side store format/paths, schema files, and migration mechanism were not located; `src/main` file listing was not retrievable via API in this session.
- RPC method catalog contents, stream frame opcodes, and ACP protocol file paths were not fetched (only their generator/verifier script names in package.json).

## Provider, planner, and tool abstraction

### Takeaway
Orca abstracts providers as launchable CLI agents behind a picker + per-agent settings row, with deeper hooks only for first-class agents; planning lives in agent CLIs plus Orca's Run/Task/DAG orchestration and skill/MCP tool registries, not in a built-in model router.

### Cited Findings
- Any terminal CLI runs in Orca; ~40 agents ship preconfigured in the picker with one-click setup — [README](https://github.com/stablyai/orca/blob/main/README.md); [supported-agents doc](https://www.onorca.dev/docs/agents/supported)
- Depth varies: Claude/Codex/Muse/Cursor have usage, hot-swap, hooks, history; others have auto-setup only, or status/session/resume where noted — [supported-agents doc](https://www.onorca.dev/docs/agents/supported)
- Per-agent launch command, arguments, environment, permission mode (Yolo vs Manual), and model/effort pills are user-configurable; custom values opt out of global migrations — [supported-agents doc](https://www.onorca.dev/docs/agents/supported); [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- `worker-start` accepts opaque provider model IDs and effort levels for Claude/Codex/Muse/Cursor/Antigravity/OpenCode, validated on the execution host and scoped to that launch — [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)
- Model lists are probed from the installed CLIs (Claude CLI, `grok models`, Codex `/model`), not hardcoded — [native-chat doc](https://www.onorca.dev/docs/agents/native-chat)
- The Anthropic SDK dependency exists but its ~95MB per-platform bundled CLIs are deliberately excluded from install; Orca always launches the user's own resolved Claude CLI — [pnpm-workspace.yaml](https://raw.githubusercontent.com/stablyai/orca/main/pnpm-workspace.yaml)
- Structured native chat (Orca-owned transcript + composer, restorable across restarts) covers Claude, Codex, Grok, OpenCode, Pi, OMP locally or on paired servers; WSL/SSH and other agents stay terminal-backed — [native-chat doc](https://www.onorca.dev/docs/agents/native-chat)
- Tool-call transparency is presentational: live progress rows, compact settled rows, MCP/web-search labeling, per-task stop, TodoWrite/update_plan checklists, AskUserQuestion cards — [native-chat doc](https://www.onorca.dev/docs/agents/native-chat)
- Agent skills are hybrid: thin public `SKILL.md` stubs installed via `npx skills add` plus full version-matched guide served by the running CLI; installable skills include orca-cli, orchestration, computer-use, orca-linear, emulators, per-workspace-env — [skills doc](https://www.onorca.dev/docs/cli/skills)
- MCP servers register under Settings Integrations and surface as tools inside supporting agent CLIs — [skills doc](https://www.onorca.dev/docs/cli/skills)
- Agent hooks and memory files, usage/rate-limit tracking, account hot-swap, session history, and hibernation are per-agent Orca features, not a unified planner API — [docs nav](https://www.onorca.dev/docs/model/worktrees) (sidebar: hooks-memory, usage-tracking, session-history, hibernation, codex-hot-swap)
- Worker contract preamble teaches the worker to send `worker_done` once with outcome/IDs, heartbeats during long work, and `ask` for blocking questions instead of local TUI prompts — [orchestration doc](https://www.onorca.dev/docs/cli/orchestration)

### Inferences
- The "planner port" equivalent is the orchestration task DAG + worker preamble protocol; the "tool port" equivalent is CLI commands + skills + MCP passthrough. Intelligence stays in the external CLIs.
- Structured chat is Orca's narrowest provider coupling: it must parse each CLI's transcript/events per agent, hence the short supported list.

### Gaps
- No prompt templates, planner interfaces, or tool-calling schemas were read from source; `src/main/claude/*`, ACP generator, and skill-bundle sources were not fetched.
- Whether hooks have a common cross-agent schema or are per-CLI shims is unknown.

## Architecture invariants and safety rules

### Takeaway
Orca enforces host-ownership of execution truth, wire compatibility across versions, worktree-as-sandbox autonomy, and a dense set of platform/safety ratchets, but ships telemetry-gated official builds with content-free anonymous analytics.

### Cited Findings
- Execution host owns agent status in one store; all readers subscribe — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Loss of contact never implies process death; verdict vocabulary is fixed to live / unverifiable / exited — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Remote wire compatibility: additive optional fields only; new opcodes capability-negotiated; host-published content changes propagate to old clients — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Worktree is the sandbox: full-autonomy flags by default, disposable checkout, review-before-merge via diff/annotate/attribution — [agents-sessions doc](https://www.onorca.dev/docs/model/agents-sessions); [docs nav](https://www.onorca.dev/docs/model/worktrees)
- Status detection rules must be written against captured PTY transcripts (with account IDs scrubbed), not remembered screens — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- UI must never steal focus in tests; background launch required; hidden-renderer CDP screenshots for validation — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Windows/WSL/macOS/SSH invariants: process-host spawn wrappers, bundled-rg resolvers, WSL argv builders, macOS daemon TCC launchd copy, per-host Git capability caches, EDR-sensitive command review — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Telemetry is compile-time gated to official CI builds; contributor/dev rebuilds substitute null keys so tracking short-circuits — [electron.vite.config.ts](https://raw.githubusercontent.com/stablyai/orca/main/electron.vite.config.ts)
- Telemetry collects anonymous enums/versions only (lifecycle, workspace-create method, agent kind, coarse error category, whitelisted setting toggles); never prompts, outputs, paths, URLs, branches, raw errors, or identity; off via Settings toggle, `DO_NOT_TRACK=1`, or `ORCA_TELEMETRY_DISABLED=1` — [telemetry doc](https://www.onorca.dev/docs/telemetry)
- Push/relay logging is aggregate counters only; tokens, titles, bodies, fingerprints never logged — [cloud README](https://raw.githubusercontent.com/stablyai/orca/main/cloud/README.md)
- Merge bar: required PR checks on exact head commit must pass; `--admin`/protection bypass forbidden except explicit emergency override — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)

### Inferences
- Determinism is pursued through transcript-driven detection, captured fixtures, golden e2e tests, and contract verifiers rather than a deterministic execution core — agents remain nondeterministic by design.
- Safety model is review-gated autonomy (isolate, then inspect diff) plus platform hardening, not capability sandboxing of tool calls.

### Gaps
- No explicit threat model, permission-escalation policy, or secrets-handling doc was read (headless secret storage reference exists but was not fetched).
- Determinism/replay guarantees for orchestration runs are undocumented in the fetched surface.
