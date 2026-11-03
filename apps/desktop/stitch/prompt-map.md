Design one responsive web screen for Runlith, a local desktop control room for AI coding agents: the project map page "Карта проекта". Follow the attached DESIGN.md exactly (Geist, dark theme, floating tiles on a dotted black canvas). All UI text in Russian; keep identifiers exactly as given.

Purpose of the screen: answer "what will a change in X affect?" for a Rust workspace the agent has analysed.

Layout at 1440 px:
- Left sidebar tile, 232 px: project "runlith"; navigation Обзор, Задачи (218), Внимание (amber badge 2), Карта (current); at the bottom Настройки and the status "agentd подключён".
- Main tile header: "Карта проекта", caption "14 возможностей · 48 crates · 123 связи", chip "crates и связи: cargo metadata" (solid outline) and chip "группировка: запись агента, демо" (dashed outline).
- Main tile body: a large pan and zoom canvas (dotted surface, radius 16) on the left, a 296 px details panel on the right.
- Canvas: 14 capability cards, 248 px wide, dashed border (agent inferred), confidence like "0,92" top right. Cards are layered top to bottom by dependency and joined by orthogonal lines with rounded corners. Lines run in channels between cards and never touch or cross a card. Layers:
  1. Точки входа (agentd, agentctl, app-services)
  2. Локальный IPC (ipc-contracts, ipc-control, ipc-transport, product-version) · Хранилище (storage-sqlite) · Провайдеры агентов (provider-claude, fake-agent)
  3. Фон и воркеры · Продолжение работы · Секреты · Репозиторий · Песочница
  4. Память и контекст · Авторизация и политика · Проверка результатов
  5. Исполнение агента
  6. Задачи и планы
  Each card: title, one line summary, crate names in Geist Mono as small selectable rows.
- Selected state at 100 % zoom, view centred on the selection: crate row "ipc-contracts" filled with accent; its card "Локальный IPC" has an accent ring and glow; card "Точки входа" is highlighted as affected with rows agentd and agentctl tinted; every other card fades to about 30 % opacity; only the line between the two highlighted cards is accent, other lines fade.
- Floating zoom pill at the bottom right of the canvas: minus, "100%", plus, "Вписать".
- Details panel: chip "crate · cargo metadata"; title "ipc-contracts" in Geist Mono; path "crates/ipc-contracts"; link chip "Локальный IPC". Section "Затронет при изменении": "Напрямую: 3. Всего crates: 3. Других возможностей: 1.", then chips agentctl, agentd, ipc-control and "Точки входа". Section "Зависит от": "Нет". Amber notice: "Группировку в agentd пока не прочитать: нужен read RPC над memory_core. Связи между crates реальные."

Responsive:
- 768 px: the sidebar collapses to a 64 px icon rail; the details panel becomes a bottom sheet over the canvas, at most half the height.
- 390 px: no sidebar, a bottom tab bar with the four sections; the canvas is full width; selecting something opens the details as a full height sheet; no horizontal page scroll; touch targets at least 44 px.

Do not use: Inter or any font other than Geist, background gradients, lorem ipsum, invented numbers, emoji, one sided borders, curved spaghetti lines, lines passing through cards.
