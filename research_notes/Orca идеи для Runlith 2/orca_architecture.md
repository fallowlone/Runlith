# Orca: архитектура репозитория

## Что такое Orca: язык, рантайм, назначение, структура, манифесты, точки входа

### Takeaway
Orca — это десктопное Electron-приложение на TypeScript (оркестратор параллельных CLI-агентов кодирования в git-worktree), а не планировщик задач или демон исполнения в классическом смысле.

### Cited Findings
- Репозиторий stablyai/orca описан как ADE для работы с флотом параллельных агентов, запускает любой coding-агент под подпиской пользователя, доступен на десктопе, мобильном и удалённом рантайме — [repo](https://api.github.com/repos/stablyai/orca)
- Основной язык TypeScript, рантайм Electron 43, сборка electron-vite, пакетный менеджер pnpm с workspace — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Версия на момент исследования 1.4.214, движок Node 24, лицензия MIT — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Корень содержит классический Electron-каркас: electron.vite.config.ts, vite.web.config.ts, config/, resources/, native/, mobile/, cloud/, skills/, skill-guides/, skill-stubs/, tests/, examples/ — [корень](https://api.github.com/repos/stablyai/orca/contents/)
- Исходники разделены на src/main (бэкенд Electron), src/renderer (UI на React 19 + Tailwind + shadcn), src/preload, src/shared, src/types, src/cli, src/packages, src/wsl-guest — [src](https://api.github.com/repos/stablyai/orca/contents/src?ref=main)
- Точки входа: бинарники orca и orca-dev собираются из src/cli в out/cli/index.js, главный процесс Electron из out/main/index.js — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Внутренние workspace-пакеты включают @orca/process-host, @orca/windows-registry, @orca/windows-security — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Файл orca.yaml в корне содержит только dev-setup скрипт (установка через внутренний mjs-скрипт и pnpm install), это не продуктовый манифест — [orca.yaml](https://raw.githubusercontent.com/stablyai/orca/main/orca.yaml)
- Каталог cloud/ — отдельный pnpm-workspace с ретранслятором для спаривания мобильного приложения с десктоп-хостом — [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- Дистрибуция: DMG для macOS обеих архитектур, exe для Windows, AppImage для Linux, Homebrew-cask и AUR-пакет, мобильный компаньон под iOS и Android — [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)

### Inferences
- Продукт распространяется как подписанное десктоп-приложение с мобильным компаньоном и облачным ретранслятором, а не как серверный демон.
- Наличие src/wsl-guest и extensive Windows-специфики в AGENTS.md указывает на первоклассную поддержку Windows/WSL/SSH наряду с macOS/Linux.

### Gaps
- Не проверен состав src/packages, mobile/ и native/ поименно (лимит запросов); имена пакетов восстановлены из зависимостей package.json.
- Не выяснено, что лежит в skill-guides/ и skill-stubs/ относительно skills/.

## Архитектурные документы, ADR, заявления README против фактической структуры кода

### Takeaway
Формальных ADR в корне нет; архитектурные решения зафиксированы в AGENTS.md, STYLEGUIDE и наборах проверочных скриптов, а заявления README о функциях подтверждаются соответствующими каталогами в src/main.

### Cited Findings
- В docs/ лежат STYLEGUIDE.md, чек-лист реализации шаринга агентских скиллов, каталоги audits/, bug-reproductions/, reference/, review-evidence/, relay-region-correction/, site/ и readme/ — архитектурного обзора верхнего уровня и каталога ADR там не видно — [docs](https://api.github.com/repos/stablyai/orca/contents/docs?ref=main)
- AGENTS.md требует следовать docs/STYLEGUIDE.md во всей UI-работе и описывает принудительные линт-гейты для дизайн-системы — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- README заявляет параллельные worktree, терминальные сплиты, дизайн-режим с Chromium, нативные GitHub и Linear, SSH-worktree, аннотирование AI-диффов, Orca CLI и мобильный компаньон — [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- Каждому заявлению соответствует код: каталоги терминалов и PTY, браузера, GitHub/Linear/Jira-интеграций, SSH-направления, worktree-кода, CLI и мобильного ретранслятора присутствуют в src/main и cloud/ — [src/main](https://github.com/stablyai/orca/tree/main/src/main), [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- Для каждого поддерживаемого агента заведён отдельный каталог интеграции в src/main (claude, codex, opencode, pi, copilot, gemini, grok, cursor, droid и десятки других) плюс общие слои providers/ и provider-process/ — [src/main](https://github.com/stablyai/orca/tree/main/src/main)
- AGENTS.md фиксирует инварианты словами: хост исполнения владеет всем, что касается исполнения; статус агентов хранится в одном сторе хук-сервера; читатели только подписываются — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Контракты версионирования провода заданы явно: новые поля опциональны, новые опкоды стримов проходят согласование возможностей, старое поведение хоста достигает старых клиентов — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)

### Inferences
- Архитектурная документация распределена между AGENTS.md и проверяющими скриптами в config/scripts, а не собрана в единый обзорный документ.
- Широта пер-агентных каталогов подтверждает позиционирование «любой CLI-агент, если он работает в терминале» как реальное свойство кодовой базы, а не маркетинговый слоган.

### Gaps
- Не проверено наличие ADR внутри docs/reference/ или docs/audits/ поименно.
- Не сверены заявления README о Ghostty-классе терминалов и WebGL-рендеринге с конкретными файлами реализации.

## Ключевые подсистемы: модель задач, планировщик, исполнитель, хранилище состояния, IPC/демон/CLI

### Takeaway
Центральная сущность — не задача с жизненным циклом, а worktree-сессия с живым агентом в PTY; исполнение строится вокруг терминалов, хук-сервера статуса, демона терминалов и CLI-поверхности.

### Cited Findings
- Модель параллелизма — изолированные git-worktree на агента с возможностью разослать один промпт нескольким агентам и слить победителя — [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- Статус агентов держит один стор на стороне хука исполнения; сайдбар, команда worktree ps, мобильное приложение и дашборд являются подписчиками без собственной политики хранения — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Отдельного планировщика в смысле очереди заданий не видно; оркестрация — это запуск агентов по worktree, терминальные сплиты и ручное/CLI управление сессиями — [src/main](https://github.com/stablyai/orca/tree/main/src/main), [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- В src/main есть слои agent-launch, acp, jsonl-rpc, ipc, runtime, server, ports, services-подобные каталоги (claude-структурированные сессии, agent-hooks, automations), что указывает на запуск агентов как процессов с протоколом обмена поверх JSONL/ACP — [src/main](https://github.com/stablyai/orca/tree/main/src/main)
- Хранение состояния распределено: каталог persistence/, чтение чужих sqlite через foreign-sqlite-readers/, память и поиск через memory/ и ai-vault-search/, слепки раскладки через remote-layout-snapshot/ — [src/main](https://github.com/stablyai/orca/tree/main/src/main)
- Терминальный демон переживает обновления: на Windows работает из копии рантайма в локальных данных приложения, на macOS запускается через launchd из подписанного помощника, есть бенчмарки холодного старта демона — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md), [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Ретранслятор хуков для WSL и гостевой код WSL вынесены в wsl-guest и бенчмарки переподключения релея — [src](https://api.github.com/repos/stablyai/orca/contents/src?ref=main), [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Orca CLI позволяет скриптовать воркфлоу командами создания worktree, слепков, кликов и ввода; CLI собирается TypeScript-компилятором в out/ и проверяется скриптом валидации бинарника — [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md), [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Учёт использования и переключение аккаунтов Claude и Codex с отслеживанием лимитов встроены в продукт — [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- Система скиллов включает каталог skills/ в src/main, проверку манифеста бандла скиллов и каталога параметров RPC в CI-скриптах — [src/main](https://github.com/stablyai/orca/tree/main/src/main), [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)

### Inferences
- Исполнитель — это связка PTY-процессов, хук-сервера и демона терминалов, а не очередь заданий с пулом воркеров.
- ACP-каталог и JSONL-RPC намекают на стандартизированный протокол общения с агентами поверх терминального транспорта.

### Gaps
- Не прочитаны конкретные файлы модели worktree-сессий и стора статуса; точные переходы состояний неизвестны.
- Не выяснено устройство orcad (Node-сборка рантайма) — вероятно headless-вариант для удалённого обслуживания терминалов, но это не подтверждено чтением кода.

## Границы доверия: допуск, аутентификация, валидация, обработка ошибок, конкурентность, персистентность

### Takeaway
Доверие строится вокруг владения хоста исполнением, скепсиса к потере связи и изоляции процессов через выделенный process-host, а не вокруг admission-контроля входящих задач.

### Cited Findings
- Потеря связи никогда не считается доказательством смерти процесса; вердикты только из словаря живой / непроверяемый / завершённый — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Учётные данные агентов изолированы: отдельные каталоги аккаунтов Claude, Codex, Cursor, Grok, плюс macOS-keychain и общий credentials-модуль — [src/main](https://github.com/stablyai/orca/tree/main/src/main)
- Дочерние процессы на Windows запускаются только через обёртки runProcess/spawnProcess из @orca/process-host с запретом shell-режима и аккуратным кодированием аргументов cmd/bat — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Файловый поиск идёт через бандled ripgrep с резолверами команд под каждую платформу, а не через сырой вызов из PATH — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Перечисление процессов Windows читается через собственный модуль таблицы процессов, а не через форк powershell — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Git-команды выполняются бинарником пользователя с базовой линией Git 2.25, кэшем возможностей Git на хост и запретом размашистых сканов всех refs — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Конкурентность Git-операций защищена файловыми блокировками через зависимость proper-lockfile — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Валидация входящих данных опирается на zod-схемы — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Секреты и криптография historically присутствуют через tweetnacl, диагностика через crash-reporting и observability, зависания ловит hang-watchdog с бенчмарками памяти — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json), [src/main](https://github.com/stablyai/orca/tree/main/src/main)
- Телеметрия анонимного использования собирается через posthog-node с документированной возможностью отказа — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json), [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)

### Inferences
- Модель угроз ориентирована на локальный десктоп с удалёнными хостами: изоляция процессов, аккуратный spawn, скепсис к сетевым вердиктам.
- Формального admission-слоя для входящих задач не просматривается, потому что продукт однопользовательский: пользователь сам запускает агентов.

### Gaps
- Не прочитаны файлы credentials/ и server/, поэтому точные механизмы аутентификации мобильного и удалённого доступа неизвестны.
- Не выяснено, как хранятся токены спаривания мобильного приложения и ретранслятора.

## Что выглядит зрелым против заглушек и экспериментального

### Takeaway
Зрелое ядро — терминалы, worktree, мультиагентные интеграции и мультиплатформенность; экспериментальный край — шаринг скиллов, ACP-протокол и компьютерное использование.

### Cited Findings
- Зрелость терминалов подтверждается обилием золотых e2e-тестов рендеринга, перф-бенчмарками печати и прокрутки, WebGL-атласами и восстановлением скролла — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Ширина покрытия агентов (30+ именованных интеграций от Claude и Codex до нишевых Droid, Kimi, Qwen) и трекинг использования говорят о зрелой мультиагентной поверхности — [src/main](https://github.com/stablyai/orca/tree/main/src/main), [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)
- Кроссплатформенная зрелость видна по выделенным модулям Windows, macOS-демонологии с приватными грантами, WSL-гостю и строгим правилам spawn/шеллов в AGENTS.md — [AGENTS.md](https://raw.githubusercontent.com/stablyai/orca/main/AGENTS.md)
- Дисциплина качества gate-скриптами (храповики на max-lines, ts-nocheck, синхронные спавны, каталоги RPC и локализации) указывает на зрелую инженерию монорепозитория — [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json)
- Открытых issues порядка 8.4 тысяч при 94 тысячах звёзд и 6 тысячах форков говорит о взрывном росте с большим бэклогом — [repo](https://api.github.com/repos/stablyai/orca)
- Экспериментальный край: шаринг агентских скиллов с чек-листом реализации на 119 КБ, генерируемый ACP-протокол с проверкой в CI, скилл-стабы и гайды, computer-use smoke-тесты — [docs](https://api.github.com/repos/stablyai/orca/contents/docs?ref=main), [package.json](https://raw.githubusercontent.com/stablyai/orca/main/package.json), [корень](https://api.github.com/repos/stablyai/orca/contents/)
- Мобильный ретранслятор в cloud/ с коррекцией регионов релея выглядит активно достраиваемым направлением — [docs](https://api.github.com/repos/stablyai/orca/contents/docs?ref=main), [README](https://raw.githubusercontent.com/stablyai/orca/main/README.md)

### Inferences
- Проект быстро растёт вширь по агентам и платформам, а вглубь укрепляется храповиками качества и e2e-золотом вокруг терминалов.
- Соотношение звёзд к открытому бэклогу типично для хайпового open-source релиза с ежедневными поставками.

### Gaps
- Не оценена свежесть и покрытие tests/ относительно src/main количественно.
- Не проверены issues и PR для подтверждения, какие подсистемы реально болят у пользователей.
