# Orca developer experience, safety, evaluation and operations

## What is CLI/daemon UX (commands, flags, config files, setup flow) and how is it documented?

### Takeaway
Orca ships a bundled `orca` CLI (spec-driven, machine-readable) plus a headless `serve`/`orcad` runtime for remote/SSH/mobile use; UX is documented in-app docs site and per-command `--help`, with setup via installer/Homebrew, CLI registration toggle, and pairing-code environments.

### Cited Findings
- Orca is a desktop Electron IDE for running multiple AI coding agents side-by-side, each in its own git worktree — not a model, not a git replacement, not hosted by default — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/index.mdx)
- Package `orca` version 1.4.214 declares bins `orca` (`./out/cli/index.js`) and `orca-dev` (`./config/scripts/orca-dev.mjs`) — [Source](https://github.com/stablyai/orca/blob/main/package.json)
- CLI is spec-driven: `COMMAND_SPECS` aggregates ~25 groups (core, worktree/orchestration, browser, computer, agent-hooks, agent-launch, diagnostics, terminal-daemon, environment, managed-server, jira/linear, vm, emulator, skills, search, etc.) in `src/cli/specs/index.ts` — [Source](https://github.com/stablyai/orca/blob/main/src/cli/specs/index.ts)
- Root help groups: Startup (`open`, `serve`, `status`), Diagnostics (`diagnostics memory`), Agent Discovery (`agent-context`), References, Agent Sessions (`search`), Accounts, Skills, Hosts, Environments, vm recipes; every command supports `--json` and per-command `--help` — [Source](https://github.com/stablyai/orca/blob/main/src/cli/root-help-text-primary.ts)
- Common commands documented in help text include `orca open`, `orca serve [--port] [--pairing-address] [--mobile-pairing] [--no-pairing] [--grant-desktop-control] [--project-root] [--recipe-json] [--supervise] [--json]`, `orca status`, `orca worktree create/rm/ps/show`, `orca file open/diff`, `orca search`, `orca account add/list`, `orca environment add/list/show/rm` — [Source](https://github.com/stablyai/orca/blob/main/src/cli/root-help-text-secondary.ts)
- Help renderer supports exact-command help, group help, and unknown-command recovery with `Next step:` suggestions (`src/cli/help.ts`, `command-suggestion.ts`) — [Source](https://github.com/stablyai/orca/blob/main/src/cli/help.ts)
- `agent-context` prints machine-readable command schema for agents; skill guides are version-matched to the CLI build (`skills list/get/install/update`) — [Source](https://github.com/stablyai/orca/blob/main/src/cli/root-help-text-primary.ts)
- Install: macOS dmg (arm64/x64), Windows installer, Linux AppImage/.deb/.rpm, Homebrew cask `stablyai/orca/orca`; RC builds via in-app Check for Updates modifiers — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/install.mdx)
- Settings reference documents General (CLI registration toggle, updates channels), Terminal, Agents, Browser, Artifacts panes; searchable via Cmd-, — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/settings.mdx)
- Headless `orcad` build scripts exist (`build-orcad-node.mjs`, `build-orcad-template.mjs`, `build-orcad-prebuilds.mjs`) plus `smoke:orcad-terminal` / `smoke:serve-terminal` scripts — [Source](https://github.com/stablyai/orca/blob/main/package.json)
- Agent status hooks toggleable without restart via CLI `orca agent hooks on|off|status` — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/settings.mdx)
- Repo-local `opencode.json` example sets default-allow permissions with `ask` for `*.env`, `external_directory`, `doom_loop` — [Source](https://github.com/stablyai/orca/blob/main/opencode.json)

### Inferences
- Spec-driven CLI with `--json` everywhere and `agent-context` schema makes the CLI itself agent-scriptable — a pattern worth copying for Runlith (`agentctl --json` + schema command).
- Unknown-command recovery and per-group help lower agent/CLI UX errors; cheap to adopt.

### Gaps
- No single CLI reference page found on the docs site in sampled files; command docs appear spread across feature pages — full command catalog size not enumerated.
- Config-file story (user-level `~/.orca` vs project-level) not fully mapped; only `orca.yaml` (setup script) and `opencode.json` observed.

## What safety/trust-boundary mechanisms exist (admission, auth, secrets, sandbox caps, human approval, rollback)?

### Takeaway
Orca delegates agent approvals to each CLI (Yolo vs Manual mode) but adds its own layers: workspace trust pre-marking, host-authority/environment fencing, secret sealing via OS keychain or AES-256-GCM file fallback, plugin audit log, and destructive-command vocabulary/suggestion guards.

### Cited Findings
- Settings offer Agent Permissions **Yolo** (fewer CLI prompts) vs **Manual** (keep each agent's own approval flow) — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/settings.mdx)
- `src/main/agent-trust-presets.ts` pre-marks workspaces trusted for cursor-agent / Copilot / Codex trust files so bracketed-paste draft flows are not intercepted; explicitly avoids `--dangerously-bypass-approvals-and-sandbox` as non-equivalent — [Source](https://github.com/stablyai/orca/blob/main/src/main/agent-trust-presets.ts)
- Codex approval/sandbox policy modeled in `codex-structured-permission-policy.ts` and launch args in `codex-structured-launch-args.ts`; Claude permission presentation sanitizes ANSI/control chars before display — [Source](https://github.com/stablyai/orca/blob/main/src/main/codex/codex-structured-permission-policy.ts); [Source](https://github.com/stablyai/orca/blob/main/src/main/claude/claude-permission-presentation.ts)
- Host-authority contracts exist (`src/shared/host-authority.ts`, `agent-session-host-authority.ts`, `runtime-environment-authority-binding.ts`); CLI has host selectors, omitted-host-scope selectors, execution-host flags, and automation destination/owner fencing — [Source](https://github.com/stablyai/orca/blob/main/src/shared/host-authority.ts)
- Desktop credentials sealed with Electron `safeStorage` (OS keychain); headless `orcad` seals with AES-256-GCM, key in login Keychain / Secret Service keyring or `0600` file mixed with machine ID; `ORCA_HEADLESS_SECRET_STORE=file` override; key-file weakness explicitly logged with `[secrets]` prefix — [Source](https://github.com/stablyai/orca/blob/main/docs/reference/headless-secret-storage.md)
- `PluginAuditLog` is an append-only JSONL audit trail (`audit.log` with rotation) recording actor `plugin:<key>`, method, bounded summary (never full params), outcome attempt/ok/error; mutation intents awaited before handler runs — [Source](https://github.com/stablyai/orca/blob/main/src/main/plugins/plugin-host-methods.ts)
- CLI has destructive-family command-suggestion tests and vocabulary policy (`command-suggestion-destructive-families.test.ts`, `vocabulary-policy.ts`) guarding dangerous suggestions — [Source](https://github.com/stablyai/orca/blob/main/src/cli/command-suggestion.ts)
- Terminal ships a git-credential guard (`terminal-git-credential-guard.ts`) and secret-at-rest protection contract — [Source](https://github.com/stablyai/orca/blob/main/src/shared/terminal-git-credential-guard.ts)
- Remote pairing via `environment add --pairing-code`, mobile pairing flags on `serve`, `--grant-desktop-control` explicit opt-in — [Source](https://github.com/stablyai/orca/blob/main/src/cli/root-help-text-secondary.ts)

### Inferences
- Trust model is "orchestrator frames, agent CLI enforces": Orca avoids silently weakening agent sandbox flags and instead pre-writes trust markers — safer pattern than blanket bypass flags.
- Honest-scope logging (audit log admits worker Node access bypasses; secret doc admits key-file weakness) is a docs practice worth copying.

### Gaps
- No rollback mechanism found (worktree rm has `--force`/`--run-hooks`; no session/artifact rollback doc observed).
- No global policy file (e.g. permissions YAML) found for Orca itself — only per-agent modes and the repo-local `opencode.json` example.
- Sandbox caps (CPU/mem/timeout) for agents not confirmed; resource manager/daemon controls mentioned in settings but limits not enumerated.

## What observability exists (logs, metrics, tracing, events, audit journal)?

### Takeaway
Observability is local-first diagnostics plus anonymous aggregate telemetry: memory diagnostics command, crash breadcrumbs/minidumps, plugin audit JSONL, session journals/timelines, and opt-out PostHog telemetry that never sends content.

### Cited Findings
- `orca diagnostics memory [--json]` runs the same host process sweep as the Resource Usage popover — [Source](https://github.com/stablyai/orca/blob/main/src/cli/specs/diagnostics.ts)
- Crash reporting includes breadcrumb store, crashpad annotations, and minidump signature modules — [Source](https://github.com/stablyai/orca/blob/main/src/main/crash-reporting/crash-breadcrumb-store.ts)
- Plugin mutations logged to append-only `audit.log` JSONL with rotation and size cap — [Source](https://github.com/stablyai/orca/blob/main/src/main/plugins/plugin-audit-log.ts)
- Session journal types include approval subjects; timeline lanes exist for JSONL-RPC (`agent-session-journal-types`, `jsonl-rpc/timeline-lane.ts`) — [Source](https://github.com/stablyai/orca/blob/main/src/main/plugins/plugin-audit-log.ts)
- Telemetry: anonymous local-ID events to PostHog Cloud (US), categories lifecycle/repos-workspaces/agents/token-counts/error-category/settings/SSH-runtime; never file contents, prompts, outputs, paths, URLs, branch names, raw errors/stacks; off with `DO_NOT_TRACK=1` or `ORCA_TELEMETRY_DISABLED=1` or Settings → Privacy toggle — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/telemetry.mdx)
- Per-incident detail stays in local diagnostic trace file, shared only via explicit diagnostic bundle — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/telemetry.mdx)
- Windows/macOS daemon docs cover TCC attribution and workspace-close repros, indicating OS-level observability concerns — [Source](https://github.com/stablyai/orca/blob/main/docs/reference/macos-daemon-tcc-attribution.md)

### Inferences
- No OpenTelemetry/distributed tracing found — observability is desktop-appropriate (diagnostics + crash + audit), not server-style metrics/tracing.
- Bounded summaries in audit log (never full params) balance debuggability with content-privacy; directly applicable to Runlith journal design.

### Gaps
- No metrics endpoint, structured log schema, or log-level configuration found.
- Retention/rotation policy for session journals and diagnostic bundles not confirmed.

## What testing/evaluation exists (unit/integration/e2e, fixture vs live-provider smokes, eval suites, benchmarks)?

### Takeaway
Huge vitest unit surface (colocated `*.test.ts`), Playwright e2e specs, contract/ratchet checks as tests, benchmark suites with results dir, and opt-in live-provider smokes — no ML-style eval harness, but strong regression-contract culture.

### Cited Findings
- `pnpm test` builds packages, ensures native runtime, then runs vitest via `run-vitest.mjs` with `config/vitest.config.ts`; targeted runs `pnpm test [path]`; typechecks `pnpm tc` / `tc:node` / `tc:cli` / `tc:web` — [Source](https://github.com/stablyai/orca/blob/main/AGENTS.md)
- CLI specs enforce parity via tests: `cli-command-names-parity.test.ts`, `command-suggestion-destructive-families.test.ts`, `serve-electron-flag-parity.test.ts`, `index-*.test.ts` per command — [Source](https://github.com/stablyai/orca/blob/main/src/cli/specs/cli-command-names-parity.test.ts)
- E2E: Playwright specs in `tests/e2e/*.spec.ts` plus `tests/tools/` smokes (omp runtime smokes, session-resume, transcript, source-control); dedicated workflows `e2e.yml`, `packaged-browser-e2e.yml`, `computer-e2e.yml`, `terminal-ime-e2e.yml`, win/WSl variants — [Source](https://github.com/stablyai/orca/tree/main/tests/e2e)
- Benchmarks in `tests/tools/benchmarks/` (daemon coldstart, startup-time, main-thread jank, mutation-receipt capacity, terminal resource) with `results/` dir; perf contracts via `vitest.performance.config.ts` + `performance-contracts.yml` CI — [Source](https://github.com/stablyai/orca/tree/main/tests/tools/benchmarks)
- Live-provider smoke is explicit opt-in: `ORCA_REAL_CLAUDE_CLI_TEST=1 pnpm test src/main/claude/claude-structured-real-cli.test.ts` uses the developer's real Claude login — [Source](https://github.com/stablyai/orca/blob/main/AGENTS.md)
- Daemon/SSH coverage: `daemon-relocation-spike`, `daemon-protocol-facts`, `wsl-orcad-e2e.yml`, `ssh-hostile-hosts.yml`, `ssh-windows-hosts.yml` — [Source](https://github.com/stablyai/orca/blob/main/.github/workflows/wsl-orcad-e2e.yml)
- Reliability gates, max-lines/ts-nocheck/Electron-import ratchets run as checks inside `pnpm lint` — [Source](https://github.com/stablyai/orca/blob/main/package.json)

### Inferences
- Fixture-first with live smokes behind env flags mirrors Runlith's deterministic-fixture vs live-provider split — validates that approach.
- Ratchet checks (fail on new violations, grandfather old) are the mechanism that lets a huge codebase tighten quality incrementally.

### Gaps
- No agent-quality eval suite (task success rate, benchmarks like SWE-bench) found — Orca orchestrates third-party agents rather than evaluating model quality.
- Coverage thresholds not confirmed.

## What ops gates exist (fmt/lint/test CI, versioning, changelog, release)?

### Takeaway
Ops is heavyweight monorepo CI: oxlint multi-config + type-aware + react-doctor + knip, path-filtered PR matrix with sharded units, perf-contract and e2e lanes, semver single-version releases cut from CI with auto-update guard, Homebrew auto-bump.

### Cited Findings
- `pnpm format` = `oxfmt --write`; `pnpm lint` = oxlint base + anti-slop + native/type-aware code-quality + reliability gates + dead-classes + max-lines/ts-nocheck/Electron/focus/layout/readme-link/node-pin ratchets + RPC/ACP/skill/localization verifications — [Source](https://github.com/stablyai/orca/blob/main/package.json)
- Lint configs: `oxlint-code-quality-native-plugins.json`, `oxlint-code-quality-type-aware.json`, `oxlint-anti-slop.json`, `oxlint-design-system.json`, `oxlint-performance-audit.json`, `oxlint-react-doctor.json`, plus knip dead-code config — [Source](https://github.com/stablyai/orca/tree/main/config)
- PR workflow uses change detection with fail-closed empty diffs, path-filtered jobs (git-compat, xterm, packaging, shell, e2e), sharded unit tests via `unit-plan.yml`/`unit-tests.yml`, concurrency cancel per PR — [Source](https://github.com/stablyai/orca/blob/main/.github/workflows/pr.yml)
- Release cut is a single manual-entry workflow: resolve ref→SHA, compute next version from kind (rc/patch/minor/major), refuse stable if ≤ latest stable (electron-updater semver guard), bump-commit-tag-push from detached HEAD, fast-forward main, build+publish artifacts — [Source](https://github.com/stablyai/orca/blob/main/.github/workflows/release-cut.yml)
- Single `package.json` version (1.4.214 observed) is the product version; RC channel via Shift+click Check-for-Updates; Homebrew cask auto-bumped every stable release — [Source](https://github.com/stablyai/orca/blob/main/docs/site/content/docs/install.mdx)
- PRs must fill `.github/pull_request_template.md` (before/after UX, mechanism, alternatives, plain language) — [Source](https://github.com/stablyai/orca/blob/main/AGENTS.md)
- ~100 workflows cover per-platform builds (mac/win/linux), mobile releases, cloud relay deploy/monitor/operate, docs, skill roundtrips, credential/transcript contracts — [Source](https://github.com/stablyai/orca/tree/main/.github/workflows)

### Inferences
- The "refuse regressing version" guard is the minimal invariant that keeps auto-update working — a good example of one load-bearing check over many cosmetic ones.
- Path-filtered CI with fail-closed diffs keeps PRs fast without losing the required `verify` aggregate — relevant if Runlith CI grows.

### Gaps
- No CHANGELOG file found at root; release notes presumably live on GitHub Releases — not verified.
- Versioning policy doc (semver rules, RC promotion criteria) not found beyond the release-cut comments.
