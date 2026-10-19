# Runlith — базовые инварианты (фактическая карта)

> Только факты Runlith. Без Orca-идей, без синтеза. Все утверждения с путями `файл:строка`.

## Что такое Runlith

### Takeaway
Runlith — локальный модульный Rust-монолит в стиле Ports & Adapters: демон `agentd` владеет оркестрацией, CLI `agentctl` — тонкая переговорная оболочка, UI-логика в workspace-пакеты не входит.

### Cited Findings
- Проект — модульный Rust-монолит с Ports & Adapters — `AGENT.md:63`
- `agentd` владеет локальной оркестрационной бизнес-логикой — `AGENT.md:70`
- UI не исполняет shell/process/Git напрямую — `AGENT.md:68`
- Tauri — тонкая нативная оболочка, не рантайм бизнес-логики — `AGENT.md:69`
- Доменные крейты не импортируют инфраструктурные адаптеры — `AGENT.md:71`
- Bounded-контекст не читает таблицы другого контекста напрямую — `AGENT.md:72`
- Инфраструктура достигается только через явные порты/контракты — `AGENT.md:73`
- Вход `agentd` — `bins/agentd/src/main.rs:12` (таблица композиции), библиотека `bins/agentd/src/lib.rs:13`
- CLI-оболочка `agentctl` — `bins/agentctl/src/main.rs:18`, переговорные клиенты — `bins/agentctl/src/lib.rs:19`
- Диспетчеры демона вызывают application services, владеющие переходами и журналом — `bins/AGENTS.md:22-24`
- UI- и cloud-концепции из архитектурных документов не являются workspace-пакетами — `AGENTS.md:22-25`
- Каноническая политика кодинг-агента — `AGENT.md:1-4`, порядок авторитетов — `AGENT.md:8-18`

### Inferences
- Нет.

### Gaps
- Не проверялось содержимое `ARCHITECTURE.md` и `PROJECT_CHARTER.md` (упомянуты в `docs/architecture/INDEX.md:10-11`).

## Workspace: состав пакетов

### Takeaway
Workspace — 40 пакетов: 26 shared crates, 12 адаптеров, `agentd`/`agentctl`; членство зафиксировано списком в корне.

### Cited Findings
- 40 пакетов: 26 shared, 12 адаптеров, `agentd`/`agentctl` — `AGENTS.md:22-24`
- Члены workspace перечислены списком — `Cargo.toml:2-43`, резолвер — `Cargo.toml:44`
- `bins/agentd`, `bins/agentctl` — `Cargo.toml:3-4`
- `crates/` смешивает домены, доверенные сервисы, IPC-инфраструктуру, контрактные тесты — `crates/AGENTS.md:8-11`
- `app-services` явно интегрирует конкретный SQLite-адаптер, т.е. `crates/` не чисто доменный слой — `crates/AGENTS.md:10-11`
- Адаптеры: `storage-sqlite`, `fake-agent`, `fake-planner`, `provider-claude`, `fake-evaluator`, `git-cli`, `effect-capture-local`, `keychain-native`, `secrets-vault`, `runner-native`, `runner-macos-seatbelt`, `finder-local` — `Cargo.toml:22-37`
- Ответственности shared-пакетов расписаны таблицей — `crates/AGENTS.md:15-32`
- Карта портов адаптеров — `adapters/AGENTS.md:8-19`
- Общие контрактные сюиты живут в `crates/`, адаптеры предоставляют реализации в интеграционных таргетах — `adapters/AGENTS.md:23`
- `ipc-contracts/build.rs` использует вендорный `protoc` + Prost, генерат в `OUT_DIR`, править только `.proto` — `crates/AGENTS.md:36-38`
- Зафиксировано 9 proto-файлов (по glob; навигация упоминает семь как исторический счёт) — `crates/ipc-contracts/proto/*.proto` (glob), `crates/AGENTS.md:28`
- `product-version` читает канонический файл версий — `crates/AGENTS.md:32`
- Проверка: `cargo test --locked -p task-domain` и др. — `AGENTS.md:63-68`; CI — `AGENTS.md:73-80`

### Inferences
- Нет.

### Gaps
- Число «26 shared» не пересчитывалось покрейтово по `Cargo.toml`; взято из `AGENTS.md:23`.
- Расхождение «семь proto» (`crates/AGENTS.md:28`) vs 9 файлов по glob не расследовалось.

## Ключевые швы: сервисы и порты

### Takeaway
Мутации — только через доверенные сервисы (`TaskService`, `OrchestrationService`); внешний мир — только через порты (`ExecutionPort`, `PlannerPort`, `EvaluationPort`); durable-граница — `SqliteStore`.

### Cited Findings
- `TaskService` — доверенный владелец durable-мутаций Task — `crates/app-services/src/task_service.rs:51-52`
- `TaskService` в карте кода: 85 ссылок, роль «Trusted Task lifecycle mutation» — `AGENTS.md:51`
- `OrchestrationService` — доверенный владелец цикла оркестрации, компоновка двух сервисов над одной БД — `crates/app-services/src/orchestration_service.rs:138-144`
- `OrchestrationService` в карте: соединяет scheduling, workspace admission, execution, evaluation — `AGENTS.md:52`
- Один оркестрационный цикл создаёт максимум одно Execution и один Attempt, только на runnable-пути — `crates/app-services/src/orchestration_service.rs:174-179`
- `ExecutionPort` принадлежит `scheduler` — `crates/AGENTS.md:52-53`; трейт — `crates/scheduler/src/execution_port.rs:74`
- `ExecutionPort`: один инжектированный executor на цикл, ровно одно терминальное поведение, без ретрая внутри, без persistence/Task/journal — `crates/scheduler/src/execution_port.rs:73-83`
- В карте: `ExecutionPort` — 46 ссылок, provider-neutral execution seam — `AGENTS.md:53`
- `PlannerPort` принадлежит `plan-domain` — `crates/AGENTS.md:52-53`; трейт — `crates/plan/src/model.rs:638`
- В карте: `PlannerPort` — 41 ссылка, предложения планирования без durable-мутаций — `AGENTS.md:54`
- `EvaluationPort` (+ `EffectCapturePort`) принадлежат `evaluation` — `crates/AGENTS.md:52-53`; трейт — `crates/evaluation/src/port.rs:11`
- В карте: `EvaluationPort` — независимые верификационные вердикты — `AGENTS.md:55`
- `*-contract` пакеты — переиспользуемые раннеры тестов, не владельцы портов — `crates/AGENTS.md:53-54`
- `SqliteStore` — конкретный persistence-API — `adapters/AGENTS.md:10`; структура — `adapters/storage-sqlite/src/task_store.rs:34`
- В карте: `SqliteStore` — durable state + append-only journal boundary — `AGENTS.md:56`
- `TaskService::mutate` валидирует переход, затем один immediate-транзакция состояния + журнала — `crates/app-services/AGENTS.md:16-18`
- `plan_claim`/`plan_start` только готовят переходы; entry-транзакция коммитит факты допуска вместе — `crates/app-services/AGENTS.md:19-21`
- `submit_result` останавливается на SUBMITTED; завершение только через review+verification и `revalidate_before_completion` — `crates/app-services/AGENTS.md:22-24`
- Планировочные вызовы вне транзакций; принятие материализует детей READY + ревизию + журнал — `crates/app-services/AGENTS.md:25-27`
- Публикация проверяет admission родителя, свежесть input/run, ожидаемый head — `crates/app-services/AGENTS.md:28-30`
- Маппинги адаптеров: `provider-claude` → `ExecutionPort` + `PlannerPort` — `adapters/AGENTS.md:11`; `fake-agent` bridged через тестовый `ContractFakeExecutor`, не prod-impl — `adapters/AGENTS.md:13`; `fake-planner` → `PlannerPort` — `adapters/AGENTS.md:14`; `fake-evaluator` → `EvaluationPort` — `adapters/AGENTS.md:15`; `effect-capture-local` → `EffectCapturePort` — `adapters/AGENTS.md:16`; `keychain-native` → `SecretBackend` — `adapters/AGENTS.md:17`; `runner-native` / `runner-macos-seatbelt` → `IsolationBackend` — `adapters/AGENTS.md:18-19`
- `NativeRunner` без OS-песочницы; Seatbelt — confinement; требуемая изоляция не деградирует к native — `crates/AGENTS.md:55-58`
- Секреты: значения только в `SecretBackend`, SQLite хранит ссылки/байдинги — `crates/AGENTS.md:59-61`
- `runner` потребляет уже решённые effective capabilities — `crates/AGENTS.md:55-56`

### Inferences
- Нет.

### Gaps
- Точные ссылки `RuntimeService`, `PlanningService`, `ObservedEffectService` взяты только из `crates/app-services/AGENTS.md:8-13`, исходники не открывались.
- Числа ссылок 85/46/41 — из `AGENTS.md:51-54`, методика (LSP, с тестами, без деклараций) — `AGENTS.md:42-45`; независимо не пересчитывалось.

## Жизненный цикл: Task, scheduling, admission, execution, evaluation, durability

### Takeaway
Ядро: `Task != Execution != Attempt != Session`; Task-долговечность через SQLite + атомарный журнал; агент не завершает свой Task; Git/файлы и SQLite — разные failure-домены с reconciliation.

### Cited Findings
- Разделение четырёх сущностей — жёсткое правило — `AGENT.md:67`; инвариант — `docs/architecture/INVARIANTS.md:13`
- Task переживает любой рестарт агента/сессии/процесса — `docs/architecture/INVARIANTS.md:12`
- Агент не может пометить свой Task завершённым — `AGENT.md:78`; `docs/architecture/INVARIANTS.md:14`
- Мутации персистентного состояния аудируемы — `docs/architecture/INVARIANTS.md:15`
- Выход планировщика — предложение; мутировать граф могут только валидированные сервисы — `docs/architecture/INVARIANTS.md:19`
- Каждая мутация графа — версионированная ревизия плана с причиной и provenance — `docs/architecture/INVARIANTS.md:20`
- Рекурсивная декомпозиция ограничена глубиной/ветвлением/узлами/временем/токенами/прогрессом — `docs/architecture/INVARIANTS.md:21`
- Декомпозиция не ослабляет критерии приёмки/секьюрность/скоуп/апрувы родителя — `docs/architecture/INVARIANTS.md:22`
- Состояние lifecycle Task отделено от derived scheduling readiness — `docs/architecture/INVARIANTS.md:23`
- Планировочные циклы завершаются работой, явным блокером или эскалацией — `docs/architecture/INVARIANTS.md:25`
- Долгоживущее состояние `agentd` персистится инкрементально — `docs/architecture/INVARIANTS.md:26`
- Провайдер-адаптеры не мутируют доменное состояние напрямую — `docs/architecture/INVARIANTS.md:33`
- Автономные исполнения используют изолированные workspaces/worktrees — `docs/architecture/INVARIANTS.md:137`
- Git/файловая мутация и SQLite — отдельные failure-домены, требуют reconciliation, никогда одна ACID-транзакция — `docs/architecture/INVARIANTS.md:28`
- Контекст — derived/disposable, не authoritative — `AGENT.md:76-77`
- Долговечная модель: authoritative relational current state + append-only Event Journal + отдельный Security Audit; не full Event Sourcing в v1 — `docs/architecture/events.md:19-32`
- Current state authoritative в v1; реплей всего журнала не требуется — `docs/architecture/events.md:34-41`
- Роли журнала: auditability, debugging, causal tracing, recovery, inspection, projections, будущий replay — `docs/architecture/events.md:43-57`
- Task-контракт: владение, разделение четырёх сущностей, канонический lifecycle, completion authority — `docs/architecture/task-engine.md:1-14`
- Каноническое владение: task rows, dependency edges, lifecycle status — `docs/architecture/task-engine.md:34-38`
- Провайдер/модель не часть семантики Task-домена — `docs/architecture/task-engine.md:28-32`
- Допуск/планирование/независимая оценка — `OrchestrationService` — `crates/app-services/AGENTS.md:11`
- Оценка завершения — независимая (ссылка на ADR-0018) — `crates/app-services/AGENTS.md:22-24`
- Переподключение `agentctl` = новый транспорт + handshake; мутации не реплеятся — `bins/AGENTS.md:28-30`
- CLI wait-mode перепроверяет статус вместо повторного `run` — `bins/AGENTS.md:31-32`
- Candidate-хелперы диспетчатся до обычного стартапа — `bins/AGENTS.md:33-37`
- Процессные тесты находят бинари через `workspace_bin` под `target/<profile>`; stale-бинари возможны — `bins/AGENTS.md:38-41`
- `agentd` владеет композицией локальных данных/БД — упоминание в `planning/tasks/queue.yaml:281-284`

### Inferences
- Нет.

### Gaps
- Каноническая таблица переходов (`crates/task/src/state.rs`) не открывалась; ссылка только из `crates/AGENTS.md:17`.
- Детали scheduling/evidence-интерпретации в `crates/scheduler/src/lib.rs` не открывались.
- Workspace/Git lifecycle (worktrees, diff, cleanup) взят только из roadmap G4 (`planning/roadmap.yaml:43-48`); `docs/architecture/workspace.md` не читался.

## Контракты: архитектура, схемы, версии, планирование

### Takeaway
Навигация — `docs/architecture/INDEX.md` (без собственной архитектуры); привязка — `INVARIANTS.md` + ADR; IPC — Protobuf, не JSON; JSON-схемы — репрезентации Task/Event; версия продукта — `product/version.toml`; работа — гейты roadmap + очередь задач.

### Cited Findings
- `INDEX.md` — только навигация, архитектуры не определяет; каноника в связанных файлах — `docs/architecture/INDEX.md:1-6`
- Изменение принятого инварианта требует ADR и явного ревью — `docs/architecture/INDEX.md:25`
- Протокол агента — `AGENT.md`, продукт — `PROJECT_CHARTER.md`, карта — `ARCHITECTURE.md`, биндинги — `INVARIANTS.md` — `docs/architecture/INDEX.md:9-12`
- Безопасность — `docs/security/THREAT_MODEL.md` и др. — `docs/architecture/INDEX.md:14-17`; приватность — `docs/architecture/INDEX.md:18-20`
- Роадмап — `planning/roadmap.yaml`, очередь — `planning/tasks/queue.yaml` — `docs/architecture/INDEX.md:21-22`
- Маршрутизация по типам задач — `docs/architecture/INDEX.md:27-117`
- Task-модель: `task-engine.md`, `events.md`, JSON-схемы, ADR-0004/0005/0006 — `docs/architecture/INDEX.md:61-68`
- IPC: `ipc.md`, `agentd_control.proto`, ADR-0003 — `docs/architecture/INDEX.md:56-59`
- Машиночитаемая форма IPC — `crates/ipc-contracts/proto/agentd_control.proto` — `docs/architecture/ipc.md:3-9`
- Protobuf, не gRPC: нет `service`, нет tonic/HTTP-2; фрейм — собственный локальный транспорт — `docs/architecture/ipc.md:39-43`
- Транспорт: macOS/Linux — Unix Domain Socket, Windows — Named Pipe — `docs/architecture/ipc.md:32-37`
- IPC — граница привилегий TB2, не доверенная шина — `docs/architecture/ipc.md:28-30`
- Неготиация версий/ capabilities — `negotiation.rs`, peer-аутентификация из транспорта, admission из композиции демона — `crates/AGENTS.md:38-42`
- Фрейминг несёт bounded bytes, не интерпретирует Protobuf-бизнес-запросы — `crates/AGENTS.md:43-45`
- Исторические версии протокола — отдельные capabilities; новый shape нельзя протаскивать в старую сессию — `crates/AGENTS.md:46-48`
- JSON-схемы Task/Event — репрезентации, отличны от Protobuf IPC-контракта — `AGENTS.md:37`
- Пути схем — `contracts/json-schema/task.schema.json`, `contracts/json-schema/event.schema.json` (glob)
- `product/version.toml` — единственный источник product-version identity — `AGENT.md:546-553`
- Текущее: `semver = "0.1.0"`, `development = "0.1.0-dev.task0174"`, `task = "TASK-0174"`, `gate = "G9"` — `product/version.toml:7-10`
- Каждая завершённая задача двигает `development`/`task`/`gate`; публичный SemVer только явным релизом — `AGENT.md:548-553`
- Текущий гейт G9 (Context Engine + Continuity), статус in_progress — `planning/roadmap.yaml:11`, `planning/roadmap.yaml:106-128`
- G0–G8 completed — `planning/roadmap.yaml:15-104`; G10–G14 planned — `planning/roadmap.yaml:129-166`
- Очередь — governance-состояние, не runtime Task-БД — `planning/tasks/queue.yaml:1-7`
- Порядок: архитектура/ADR/инварианты > roadmap > очередь — `planning/tasks/queue.yaml:5`, `planning/roadmap.yaml:3`
- Прогресс по exit-критериям гейтов, не по датам — `planning/roadmap.yaml:4`
- Статусы roadmap — метаданные планирования, не `Task.status` lifecycle — `planning/roadmap.yaml:5-8`
- `gate_status: IN_PROGRESS`, G9 активирован TASK-0167, G8 закрыт TASK-0127 — `planning/tasks/queue.yaml:12`
- Политика версий — `docs/development/VERSIONING.md` (ссылка) — `product/version.toml:2`, `AGENTS.md:36`
- Аудиты/ремедиации гейтов — `docs/development` — `AGENTS.md:38`
- Изменение архитектуры требует ADR с problem/decision/alternatives/consequences/security/migration — `AGENT.md:182-189`

### Inferences
- Нет.

### Gaps
- Содержимое JSON-схем не читалось, только пути.
- Содержимое отдельных `.proto` (кроме `agentd_control.proto` как канонического) не читалось.
- ADR-список (`adr/INDEX.md`, `adr/COVERAGE.md`) не открывался.
- Хвост `queue.yaml` после строки 1156 (G6 TASK-0069 и далее до TASK-0174) не читался.
