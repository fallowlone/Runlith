# Лог: глубина анализа Orca → Runlith + почему каждая функция не взята

Дата: 2026-10-16
Папка заметок: research_notes/Orca идеи для Runlith/
Отчет: reports/Orca идеи для Runlith.md
Дополнение по настройкам: research_notes/Orca идеи для Runlith/orca-settings-customization.md

## Насколько глубокий анализ

- Координатор deep-research + 4 параллельных исследователя + 1 писатель.
- Runlith baseline: AGENT.md, ORCHESTRATOR.md, Cargo.toml (40 members), crates/AGENTS.md, adapters/AGENTS.md, bins/AGENTS.md, docs/architecture/INDEX.md + ipc.md + task-engine.md + events.md + INVARIANTS.md, planning/roadmap.yaml (G0-G14), planning/tasks/queue.yaml (164 TASK, G9 IN_PROGRESS, TASK-0174 done / TASK-0175 next), product/version.toml (0.1.0-dev.task0174, gate G9), contracts/json-schema + crates/ipc-contracts/proto (9 proto), .github/workflows/verification.yml (fmt/clippy/test, Rust 1.95, macOS-only Seatbelt).
- Orca архитектура: README, package.json (Electron 43, React 19, Node 24, pnpm), src/ листинг (main/renderer/preload/cli/shared/types/packages/wsl-guest), @orca/process-host, entry bins orca/orca-dev/orcad, electron.vite.config, cloud/README (relay/postgres-sqlite/push-gateway), pnpm-workspace, orca.yaml + .worktreeinclude, docs onorca.dev (worktrees, agents-sessions, session-restore, orchestration, terminal, skills, native-chat, supported-agents).
- Orca DX/safety/ops: settings.mdx полностью, CLI specs/index.ts (~25 групп), root-help-text, help.ts, agent-trust-presets.ts, codex permission policy, host-authority.ts, headless-secret-storage.md (safeStorage/AES-256-GCM), plugin-audit-log.ts + plugin-host-methods.ts (JSONL), diagnostics.ts, crash-breadcrumb-store, telemetry.mdx (PostHog opt-out), tests/e2e + benchmarks + performance-contracts.yml, AGENTS.md (runProcess/spawnProcess, rg resolvers, git v2.25 baseline, wire compat, focus rules), package.json lint (oxlint multi-config + knip + ratchets), pr.yml + release-cut.yml (~100 workflows).
- Orca mechanisms: плюс два одноименных проекта для механизмов — orca-cli/orca (Go: queued→running→ready→shipped, pod DAG, internal/pod/runner/state/constraint/cli, ~/.orca/orca.db SQLite+FTS5, .orca/runs/<id> worktrees, adapters MCP stdio, orca.toml + policies YAML + config validate) и VirtusLab/orca (Scala: FlowControl/InStage/WorkspaceWrite capability tokens, .progress.json committed + events.jsonl machine-local, RecoveryCheck prompt-hash, FlowLock, session id schemes ClientClaimed/ServerMinted, EnforcementNotice matrix, TurnEvent conformance).
- Ограничения глубины: код не запускали, бинари не собирали, полного клона src/main не делали (лимит 15 fetch, нет shell у сабагентов), часть внутренних схем (RPC opcodes, ACP snapshot, desktop store paths/migrations, retry/backoff таймауты, лимиты параллелизма) не fetched — помечены как Gaps в заметках. Сравнение зрелости — по docs-evidence only.

## Каждая отклоненная функция — почему не брали

1. Центральный шедулер Orca — отсутствует. Scheduling = ручное размещение + opt-in Run/Task/Dispatch. Брать нечего.
2. Worktree-как-песочница (disposable checkout достаточно) — конфликт с Seatbelt INV-076 fail-closed. Не берем.
3. Pre-applied bypass-флаги (skip-permissions и аналоги) — нарушают INV-013/INV-153 (prompt/preflight не enforcement). Не берем.
4. SSH/remote федерация placement (--on, бейджи) — нет admission/квот, loss-of-contact != death без вердиктов. Не берем как placement; берем только multiplex defaults для G12.
5. Федеративное состояние без центрального журнала — слабее SQLite current-state + append-only journal + hash chain. Не берем.
6. Rollback — не найден (только rm --force/run-hooks). Брать нечего.
7. Global policy file — не найден (только per-agent Yolo/Manual + opencode.json пример). Брать нечего; свой Policy сильнее.
8. CPU/mem/timeout caps — не подтверждены. Брать нечего.
9. OpenTelemetry/tracing, metrics endpoint, log schema/levels, retention — не найдены. Только diagnostics + crash + audit. Не берем.
10. Agent-quality evals (SWE-bench подобное) — нет, только regression contracts. Не берем.
11. CHANGELOG/version policy doc — нет (только release-cut guard refuse ≤ stable). Берем только guard-идею.
12. Model routing/fallback/cache/budget — не найдены (только opaque --model + probe CLI + Turn token/cost). Не берем; нужен свой G14 router.
13. Planner prompts/decomposition heuristics — нет, планирует внешний CLI. Наш PlannerPort сильнее.
14. File/git/shell/browser tool internals — не inspected (src/main глубже top-level не fetched). Не копируем вслепую.
15. orca-cli DOCS.md — 404. Идеи как design, не proven impl.
16. Turn-cookie sweep /proc — Linux-only report-only. Нужен кроссплатформенный аналог, не копия.
17. Appearance/темы/шрифты/minimap/иконка/язык/zoom — IDE презентация, не control plane.
18. Terminal Ghostty/Warp import, OSC52 on default — эмуляторное; OSC52 держать off из-за exfil.
19. Browser profiles/Design Mode/link routing — нет browser tool, отложить.
20. Artifacts public links — нет store, отложить; взять default-off gate паттерн.
21. Voice/Floating/Plugins-marketplace/Cloud-VM experimental — вне scope durability; hibernation только как идея для G11.
22. Emoji/creature имена веток — ломают prompt-hash resume. Не копировать.

## Что взяли (кратко, детали в отчете)

- dispatch taskId+dispatchId, preamble exactly-once + heartbeats + ask, FIFO-ack inbox + decision gates, committed-log vs machine-local resume split + RecoveryCheck, test-rendered capability матрицы, ship-gate config validate, --json везде + agent-context, trust-presets, secrets sealing + JSONL audit, diagnostics + cleanup + notifications, SSH multiplex + base ref + auto-run.

## Файлы

- research_notes/Orca идеи для Runlith/runlith-baseline.md
- research_notes/Orca идеи для Runlith/orca-architecture.md
- research_notes/Orca идеи для Runlith/orca-dx-safety-ops.md
- research_notes/Orca идеи для Runlith/orca-mechanisms.md
- research_notes/Orca идеи для Runlith/orca-settings-customization.md
- reports/Orca идеи для Runlith.md
- Этот файл — reports/Orca анализ глубины и отказы.md
