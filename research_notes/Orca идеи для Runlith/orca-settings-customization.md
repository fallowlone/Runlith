# Orca настройки и кастомизация — что перенять в Runlith

Дата: 2026-10-16
Источник: stablyai/orca `docs/site/content/docs/settings.mdx` (173 строки), `onorca.dev/docs/model/worktrees`, `src/cli/specs/index.ts`, `src/main/agent-trust-presets.ts`, `docs/reference/headless-secret-storage.md`
Локальный контекст: Runlith — Rust modular monolith, Ports & Adapters, 40 пакетов, agentd/agentctl, TaskService/OrchestrationService/ExecutionPort/PlannerPort/EvaluationPort/SqliteStore. Своей UI-панели настроек нет; конфиг — product/version.toml, policy, workspace materialization.

## Глубина анализа настроек

- Прочитан полностью settings.mdx: General, Appearance, Git, Terminal, Quick Commands, Agents, Browser, Artifacts, Integrations, Notifications, Voice, SSH, Remote Servers, Shortcuts, Repository, Floating Workspace, Plugins, Experimental.
- Прочитан worktrees doc: Shared Paths, orca.yaml sharedDirectories, .worktreeinclude.
- Проверен Runlith grep по settings/preferences: своих пользовательских панелей нет, есть только упоминания settings/control host в queue.yaml (TestMutationPolicy B1/B2) и policy контракт.
- Оценка через инварианты Runlith: Seatbelt confinement (INV-076), prompt не control (INV-013), preflight не enforcement (INV-153), secrets вне модели (INV-008), Task != Execution != Attempt != Session.

## Брать — 10 групп

### 1. Yolo vs Manual permission mode
- Что: явный выбор — fewer prompts vs keep agent approval flow.
- Почему брать: ложится на Policy capabilities/approvals, дешево, честно не ломает CLI sandbox.
- Куда: Policy preflight + agentctl flag.

### 2. Trust-presets вместо bypass
- Что: pre-mark workspaces trusted для cursor/copilot/codex trust files; явный отказ от --dangerously-bypass как non-equivalent.
- Почему брать: безопаснее blanket bypass, совместимо с Seatbelt.
- Источник: src/main/agent-trust-presets.ts

### 3. Worktree env: Shared Paths + orca.yaml + .worktreeinclude
- Что: Settings Shared Paths (APFS clone/symlink) + repo-checked sharedDirectories (node_modules, .cache, только gitignored) + .worktreeinclude copy (.env, .vscode/settings.json, literal paths, без glob).
- Почему брать: решает главную боль fresh worktree — gitignored deps/секреты отсутствуют. Нужно для Runlith workspace materialization.
- Правило сложения: orca.yaml добавляет к user-листу, не заменяет.

### 4. Машиночитаемый CLI
- Что: spec-driven COMMAND_SPECS ~25 групп, --json везде, per-command --help, agent-context schema, version-matched guides (skills get), флаги в бинаре.
- Почему брать: делает agentctl agent-scriptable, убирает дрейф гайдов. Нужно для G11/G12 headless.
- Плюс: unknown-command recovery с Next step.

### 5. Hooks без рестарта + Keep awake
- Что: orca agent hooks on|off|status без рестарта; Keep awake On/Agent/Off (Caffeinate).
- Почему брать: дешевая наблюдаемость статусов working/waiting/done; awake нужен демону.

### 6. Notifications для долгих прогонов
- Что: agent-finished system/sound/chip, custom sound per category, PR check failures, update available.
- Почему брать: нужно для фонового демона, иначе молчаливые зависания.

### 7. Ops: diagnostics + cleanup
- Что: diagnostics memory [--json] = тот же sweep что Resource Usage popover; Resource Manager Clean up workspaces (статус, size, Git state, linked review перед удалением); Preserved branches review.
- Почему брать: нужно для daemon ops и G11.

### 8. Secrets + audit
- Что: safeStorage (OS keychain) на десктопе; orcad AES-256-GCM ключ в Keychain/Secret Service/0600-file+machine ID; ORCA_HEADLESS_SECRET_STORE=file override; слабость key-file явно логируется [secrets]; PluginAuditLog append-only JSONL audit.log с rotation, actor plugin:<key>, bounded summary никогда full params.
- Почему брать: применимо к SecretBackend + journal без секретов (INV-008).

### 9. SSH defaults
- Что: reuse multiplex default on, default identity file, proxy/jump, Kerberos via GSSAPI.
- Почему брать: пригодится для G12 remote worker admission.

### 10. Per-repo base ref + auto-run + pairing
- Что: per-repo base ref resolver, auto-run commands on worktree create, host selectors, omitted-host-scope, --grant-desktop-control explicit opt-in, pairing-code environments.
- Почему брать: base ref + auto-run нужны для admission; explicit opt-in — образец для privileged ops.

## Не брать — с причиной на каждую

- Appearance (theme/accent/density/fonts/minimap/status toggles/icon/language/zoom): IDE презентация, не control plane. Нет ценности для durability.
- Terminal font/theme/cursor/Ghostty/Warp import: эмуляторные заботы. Runlith делегирует CLI, свой xterm не владеет.
- OSC52 Allow TUI Clipboard Writes on by default: риск exfil секретов через clipboard incl SSH. Для Runlith держать off.
- Browser profiles/zoom/Design Mode/link routing/terminal link actions: нет browser tool в Runlith. Отложить до появления tool.
- Artifacts public links gate: нет artifact store. Отложить; взять только паттерн default-off gate.
- Integrations GitHub/Linear/Jira/Bitbucket/MiniMax/MCP: брать только принцип — verify before save, env precedence скрывает Connect, credentials stay on machine. Сами интеграции вне scope ядра.
- Voice dictation (mic/mode/speech models Parakeet/Zipformer/Paraformer/SenseVoice/Whisper/GPT-4o): вне scope.
- Floating Workspace + Terminal Directory + Toggle Button: non-repo tabs, противоречит Task-центричной модели.
- Plugins Experimental marketplace: untrusted third-party, capability shape меняется. Конфликт с Policy Engine.
- Experimental Cloud VM / Agent Dashboard kanban / Chat UI / Activity feed / compact cards / hibernation: частично интересно (hibernation + dashboard для G11), но как UI-фичи не копировать; брать только идею hibernation pause/resume для idle agents.
- Emoji workspace names / marine creatures / auto-rename branches: ломают deterministic resume (нужен prompt-hash ключ, branch-independent). Не копировать.
- Worktree-как-песочница + full-autonomy bypass: нарушает Seatbelt fail-closed. Отклонено ранее.
- SSH-федерация как placement: нет admission/квот. Отклонено ранее.
- OTel/tracing, metrics endpoint, rollback, global policy file, resource caps, agent-quality evals, model routing/cache/budget: отсутствуют в Orca — брать нечего (см. orca-mechanisms.md Gaps).

## Приоритет переноса

1. Shared Paths + orca.yaml + .worktreeinclude (workspace materialization)
2. --json везде + agent-context + version-matched guides (agentctl)
3. Yolo/Manual + trust-presets (Policy)
4. Secrets sealing + JSONL audit без секретов
5. diagnostics --json + cleanup + notifications (G11 daemon)
6. SSH multiplex + base ref + auto-run (G12 remote)

## Связь с прошлыми выводами

- Дополняет reports/Orca идеи для Runlith.md: там надзор (dispatch-ID, preamble, FIFO-ack, resume-split), здесь — настройки/кастомизация.
- Не противоречит запрету копировать архитектуру: переносим протоколы и DX-паттерны, не Electron-хост и не bypass-модель.
