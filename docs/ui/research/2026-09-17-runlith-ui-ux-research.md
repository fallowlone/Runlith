# Runlith — исследование desktop UI/UX

Дата среза: **16 сентября 2026 года**; оформление завершено **17 сентября 2026 года**. Режим: read-only research. Репозиторий: ~/dev/runlith. HEAD: 891c36f4b30a030ee581344fb718403712c8bbf1.

Исследование предназначено для подготовки UI Product / Information Architecture contract. Это не утверждённый дизайн, frontend ADR или план реализации. TASK-0085 не создавался. Файлы репозитория не изменялись.

**Обозначения доказательств:** R — контракт/код Runlith; D — поведение, описанное официальной документацией; O — визуальное наблюдение официального изображения; M — утверждение производителя; C — мнение пользователя; S — синтез и предложение для Runlith. D не означает, что функция проверена в установленном приложении; O не доказывает доступность, скорость или удобство. Все веб-источники проверены 16.09.2026; дата публикации указана отдельно, где установлена.

Метод: сначала изучены продуктовые ограничения и доменные контракты; затем подобраны продукты под конкретные проблемы; изучены текущие документация, release notes, официальные изображения и ограниченная качественная выборка обсуждений; после этого сформулированы требования, варианты IA и проверяемые гипотезы. Интервью, usability tests, сравнение производительности приложений и полноценный аудит их accessibility не проводились.

Ограничение материалов: упомянутое пользователем вложение Warp в этом чате недоступно. Найденный в репозитории PNG оказался коллажем концептов Runlith, а не Warp. Поэтому ниже отдельно анализируются **описанная пользователем композиция** и **реально просмотренные официальные изображения Warp**. Точное вложение не подменено похожим изображением.

## 1. Executive research summary

1. **Основная единица интерфейса — Task.** Пользователь должен понимать цель, обязательства, состояние, препятствие и доказательства результата. Provider Session — технический контекст внутри Attempt, а не главный контейнер продукта. Основание: R, ADR-0005 и Task Engine.
2. **Главная UX-задача — быстро восстановить понимание автономной работы.** После перерыва нужно ответить: что изменилось, что сейчас происходит, чего ждёт система, что требуется от меня и чему можно доверять. Это важнее постоянного показа токенов диалога. S; сопоставление Linear Inbox, Temporal и агентных IDE.
3. **Task Workspace должен существовать без открытого чата.** Разговор полезен при постановке задачи и уточнении намерений; Plan, Diff, Evidence и Decisions обязаны быть доступны непосредственно. S; официальные Cursor/Warp показывают самостоятельную ценность файлов, планов и review.
4. **Адаптивный inspector — ведущая гипотеза, не доказанный победитель.** Постоянная навигация и центральная задача; правый inspector можно закрепить; нижняя диагностика открывается по запросу. В активной совместной работе вариант с постоянной правой панелью может оказаться лучше. Требуется сравнительный тест.
5. **Авторитетное состояние должно быть отделено от рассказа агента.** «Тесты прошли» в сообщении не равно VERIFIED; успешный Attempt не равен COMPLETED; отсутствие новых событий не равно остановке. R; аналогия вложенных ошибок Temporal.
6. **Plan, PlanRevision, PlanningRun и предложение изменения — разные объекты.** Нужны принятый план, его происхождение, изменения относительно предыдущей ревизии и отдельный статус работы планировщика. R, ADR-0027/0028.
7. **Plan и Run — отдельные действия.** RunExecution не должен незаметно запускать планировщик, менять план или расходовать дополнительный planning budget. R, ADR-0028.
8. **Human Decision и Approval нельзя объединять в один тип «вопроса агента».** Их допустимо собрать в общую очередь Attention, сохранив тип, scope, lifecycle, причины и разные действия. R, G6/G7.
9. **Решение пользователя ещё не означает исполнение.** HDR RESOLVED отличается от CONSUMED; approval approved — от consumed и от результата операции. Интерфейс должен показывать промежуточное положение без повторного запроса согласия. R.
10. **Прогресс — это факты, этапы и свежесть, а не придуманное число процентов.** Показывать текущий открытый Attempt, последний существенный event, завершённые обязательства и ожидания. Счётчик шагов допустим только с известным основанием и указанием ревизии плана. S на основе R и Warp.
11. **Recovery требует точных обещаний.** Текущий ResumeExecution покрывает admitted-but-not-attempted execution. Универсальные «Resume», «Restart with another provider» и восстановление любой сессии пока нельзя обещать. R, ADR-0025 и execution IPC.
12. **Блокировка должна оставаться локальной своему scope.** Один HDR не превращает проект в красный экран; независимая работа может продолжаться. R, G7.
13. **До серьёзного UI нужны дополнительные query/read contracts.** Сейчас недостаточно данных для истории Execution, богатого Task read, PlanRevision browser, общей очереди решений и связанных Evidence/Diff/Logs. Это результат проверки proto, а не повод читать SQLite или Git из frontend.
14. **Сдержанность Cursor/Warp работает через распределение внимания.** Нейтральные поверхности, повторяемые строки, компактные инструменты и доминирующий рабочий объект полезнее декоративной «AI-идентичности». O/S; конкретные шрифты и размеры конкурентов не измерялись.
15. **Рынок уже не сводится к editor + chat справа.** Cursor имеет Agents Window; VS Code документирует Agents window в Preview; документация Windsurf перенаправляет в Devin Desktop. Это свидетельство поиска новых моделей работы, а не доказательство одного правильного layout.
16. **Исследование не требует пересмотра всей visual semantics.** Сохранить тёмную IDE-направленность, синий для активности/фокуса, зелёный для положительного исхода, teal для вторичного выделения. Значение должно дублироваться текстом и формой.
17. **Desktop и accessibility — требования первого уровня.** Контекст нескольких окон, клавиатурный resize, восстановление focus, масштабирование текста, читаемые diff/logs и независимость daemon от окна влияют на архитектурный выбор.
18. **Следующий шаг — контракт продукта и IA, затем обоснованный frontend ADR.** В репозитории уже есть baseline Tauri/React и ADR о primitives-only. Будущий ADR должен подтвердить или пересмотреть эту основу по проверяемым требованиям. TASK-0085 до этого исследования не требовался и не создан.

Источники этого резюме раскрыты рядом с соответствующими выводами в разделах 2–18; полный реестр — в разделе 19.

## 2. Runlith product/UX problem definition

### 2.1 Что восстанавливается из репозитория

Runlith — локальный control plane инженерной работы с долгоживущим agentd, durable Task state, планированием, контролем полномочий, выполнением через providers и независимой проверкой. Пользователь управляет достижением результата и допустимыми действиями. Он не должен вручную собирать правду из терминальных сессий.

Иерархия ответственности:

    Project
      Task — цель, scope, acceptance obligations, lifecycle
        Execution — конкретный контейнер выполнения
          Attempt — отдельная попытка и её outcome
            Provider Session — временный контекст конкретного provider

Task hierarchy и зависимости Plan — связанные, но разные отношения. Родительская задача не равна группе вкладок. Последовательные Attempts не являются параллельными «агентами». У Execution нет отдельного универсального durable status enum: его представление выводится из известных данных и не должно создавать новую машину состояний.

Основание: [ADR-0005](../../architecture/adr/ADR-0005-task-execution-attempt-session.md), [Task Engine](../../architecture/task-engine.md), [provider contracts](../../architecture/providers.md), [Execution IPC](../../../crates/ipc-contracts/proto/execution_core.proto).

### 2.2 Текущее положение и граница готовности

Рабочая копия planning/tasks/queue.yaml помечает G7 CLOSED после TASK-0084; G8 не начат. Эти governance-изменения уже были незакоммичены при начале исследования. Отчёт использует их как текущую заявленную позицию проекта, а не как собственное подтверждение closure audit. Версия продукта — 0.1.0-dev.task0084.

Есть канонические контракты независимой evaluation, однако полноценный продуктовый объём verification относится к G8. Continuity и более широкое recovery нельзя считать готовыми только потому, что они описаны в долгосрочной архитектуре. G11 остаётся отдельной будущей областью. planning/tasks/queue.yaml — governance queue, не runtime Task database.

Основание: [roadmap](../../../planning/roadmap.yaml), [queue](../../../planning/tasks/queue.yaml), [version](../../../product/version.toml), [ADR-0018](../../architecture/adr/ADR-0018-independent-evaluation.md).

### 2.3 Жёсткие ограничения и оцениваемые гипотезы

| Основание | Следствие для продукта | Статус |
|---|---|---|
| Thin UI, business logic в agentd | Правила допустимости действий и transitions не дублировать в компонентах | Каноническое ограничение |
| Versioned IPC | Нужны явные unsupported/incompatible состояния и проверяемые query contracts | Каноническое ограничение |
| Независимая evaluation | «Агент закончил» и «результат проверен» видимо различаются | Каноническое ограничение |
| Immutable PlanRevision и freshness fencing | Старую ревизию можно просмотреть; применить устаревшее решение нельзя | Каноническое ограничение |
| Bounded approvals, hard DENY | Кнопка approve не даёт новых полномочий и не обходит policy | Каноническое ограничение |
| Local-first, durable identity | Окно — представление работы, его закрытие не означает отмену работы | Продуктовое следствие |
| Тёмная IDE-эстетика, семантика цветов | Принять за исходное направление, проверить контраст и плотность | Предпочтение пользователя |
| Mission Control, agent rosters, карточки в старых концептах | Проверять на частоту сценариев и зрелость контрактов | Гипотеза, не обязательная IA |
| Tauri 2 + React/TypeScript/Vite, primitives-only | Учесть существующее решение до обсуждения альтернатив | Текущий архитектурный baseline |

[ADR-0007](../../architecture/adr/ADR-0007-no-direct-frontend-shell.md) запрещает frontend shell/business logic. [ADR-0015](../../architecture/adr/ADR-0015-shadcn-primitives-only.md) не требует копировать shadcn dashboards. [ADR-0016](../../architecture/adr/ADR-0016-agentctl-before-complex-ui.md) устанавливает CLI-first discipline. [Architecture source of truth](../../../AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md) содержит и обязательные ограничения, и более ранние UI-направления; последние здесь не считаются результатом пользовательского исследования.

В roadmap первый серьёзный desktop UI предшествует сравнительному UI-TECHNOLOGY-SPIKE. Поэтому предлагаемый ADR перед shell прежде всего фиксирует требования и подтверждает текущую основу; benchmark Tauri/React, Slint и GPUI остаётся последующим этапом на репрезентативной реализации. Перенос такого spike раньше потребовал бы отдельного осознанного пересмотра roadmap, который это исследование не выполняет. [Roadmap: UI milestones (line 142)](../../../planning/roadmap.yaml).

### 2.4 Главные рабочие сценарии

| Сценарий | Вопрос пользователя | Успешный результат |
|---|---|---|
| Создать инженерную задачу | Что поручаю, где границы и что считать успехом? | Task с явными scope/criteria; стоимость planning не скрыта |
| Проверить план | Почему выбрана эта последовательность и что изменилось? | Читаемая accepted revision, proposal delta, зависимости и риски |
| Наблюдать автономную работу | Работает ли она и нужно ли вмешательство? | Фактический этап, blocker и свежесть без чтения transcript |
| Вернуться после перерыва | Что произошло с момента моего ухода? | Краткая сводка изменений со ссылками на события и evidence |
| Ответить системе | Это уточнение, изменение курса или разрешение операции? | Верное решение для верного scope и актуальной версии |
| Проверить результат | Какие обязательства выполнены и чем доказаны? | Criterion → evidence → revision/Attempt → verdict |
| Разобрать неудачу | Что именно упало и какие действия допустимы? | Ошибка нужного уровня; история попыток; честные recovery options |
| Переключаться между задачами | Где внимание действительно необходимо? | Очередь действий без постоянного мониторинга всего проекта |

Это рабочие гипотезы о поведении целевой аудитории, выведенные из назначения Runlith. Их частоты, длительность и предпочтения ещё нужно проверить на пользователях.

### 2.5 Обнаруженные contract gaps

| Для какого UX нужны данные | Что доступно сейчас | Чего не хватает для серьёзного UI |
|---|---|---|
| Task Workspace после перезапуска | Базовый Task read; v6 create_planned возвращает planning metadata | Полный read scope/criteria/risk/hierarchy; create response не заменяет повторный read |
| Execution history задачи | GetExecution по известному ID, список Attempts внутри него | Перечень Executions по Task и query для поиска |
| Plan browser | TriggerPlanning / ApplyHumanContinuation, ограниченный operation response | Accepted head, revisions, delta, provenance, PlanningRun/input/budget views |
| Attention | HDR list/get/resolve; Approval get/decide | Approval discovery/list; стабильное объединённое представление и сортировка |
| Evidence / artifacts / diff | Доменные основания и storage/workspace contracts | Версионированные UI query DTO, paging, provenance, freshness |
| Events / logs | Durable journal и отдельная диагностика на backend | Фильтры, курсоры, bounded payloads, reconnect semantics |
| Provider interaction | Provider-neutral execution view | Session/transcript/steering capabilities и адресуемость сообщений |
| Action availability | Серверные проверки операций | Представление причин недоступности и релевантных preconditions |

Текущий control channel последовательный: один запрос в работе, без multiplexing/streaming/request IDs. Это ограничение нельзя незаметно превратить в поток токенов и live logs. Сначала нужен контракт доставки данных, ограничения объёма и повторного чтения. UI не должен компенсировать пробелы чтением БД, запуском Git/process commands, разбором CLI-строк или предположениями по тексту агента. Источники: [IPC](../../architecture/ipc.md), [control proto](../../../crates/ipc-contracts/proto/agentd_control.proto), [planning proto](../../../crates/ipc-contracts/proto/planning_core.proto), [authoring proto](../../../crates/ipc-contracts/proto/task_authoring_core.proto).

## 3. Market landscape

Выборка ограничена продуктами, каждый из которых помогает решить отдельную проблему Runlith. Популярность, число пользователей и обещания роста производительности не использованы как аргумент качества.

| Класс | Продукты | Зачем исследованы |
|---|---|---|
| IDE с новым агентным workspace | Cursor, VS Code | Сосуществование файлов, review, sessions и фоновой работы |
| Компактные editor/tool windows | Zed, JetBrains IntelliJ IDEA | Плотность, focus, переключение контекста, capability differences |
| Terminal + agent environment | Warp | Структурированный прогресс, Plan document, contextual review, notifications |
| Agent-first workspace | Windsurf → текущие docs Devin Desktop; Devin | Sessions/Spaces, queueing, checkpoints, plan/approval boundary |
| Автономная работа вне editor | Codex app, Claude Code Desktop | Worktrees, несколько задач, review и регулируемая детализация |
| Product/work command surfaces | Linear, Raycast | Attention queue, peek, поиск объектов, действия выбранного объекта |
| Durable execution control | Temporal, Dagster | Вложенные ошибки, attempts/reruns, history, causal inspection |
| Verification и связанные данные | GitHub Actions, Grafana Explore | Deployment review, dependency graph, переход от span к logs |
| Нормативная и HCI-база | WCAG/WAI-ARIA, Apple HIG/WWDC, Microsoft HAI guidelines | Accessibility, inspector semantics, управление ожиданиями |

Важные временные уточнения: Cursor 3 Agents Window опубликован 02.04.2026, tiled layouts — 13.04.2026, Cursor Projects — 10.09.2026. Последнее слишком новое для выводов о зрелости. VS Code Agents window помечен Preview. Документация Windsurf в день исследования перенаправляет в Devin Desktop; это наблюдение текущих docs, не утверждение о дате переименования всех установок. Источники: [Cursor 3.0](https://cursor.com/changelog/3-0), [3.1](https://cursor.com/changelog/3-1), [Cursor changelog](https://cursor.com/changelog), [VS Code Agents window](https://code.visualstudio.com/docs/agents/run/agents-window), [Cascade](https://docs.devin.ai/desktop/cascade/cascade).

## 4. Comparative matrix

### 4.1 Сравнение по проблемам Runlith

Здесь нет суммарного рейтинга. «Полезно» означает пригодность конкретного паттерна; отсутствие описания функции означает недостаток проверенных данных, а не отсутствие функции в продукте.

| Продукт/паттерн | Основной контекст | Навигация и панели | Контроль автономной работы | Evidence / recovery | Что взять; где граница |
|---|---|---|---|---|---|
| Cursor | Agent workspace рядом с IDE | Sessions, файлы, review; несколько компоновок | План, сообщения и изменения | Review/checkpoints документированы | Сохранить доступ к рабочему артефакту; thread не равен Task |
| Warp | Terminal/agent conversation + рабочие объекты | Tabs, splits, contextual review/plan | Task list, questions, notifications | Diff и история plan edits | Компактный прогресс; не переносить permission semantics |
| VS Code | Editor и Preview Agents window | Sessions + files/changes, строгий session context | Локальная/фоновая работа | Разные scopes diff | Явная привязка панели к выбранной работе; высокая сложность |
| Zed | Project + agent thread | Project grouping, MRU, worktree context | Queue/steering зависят от integration | Checkpoints отдельных изменений | Показывать capabilities; не обещать одинаковые функции provider |
| JetBrains | Code workspace | Docked/floating/contextual tool windows | Инструменты возникают по контексту | Богатая диагностика | Управляемый focus; не импортировать всю IDE |
| Devin Desktop | Space + sessions | Sidebar / command center, workspace arrangement | Local/cloud sessions, queueing | Cascade checkpoints | Собирать связанные объекты; Space не заменяет domain identity |
| Codex app, launch reference | Project + parallel threads | Project threads, worktrees, review | Background work / review queue | Diff review | Изоляция задач и возвращение к review; февральский срез |
| Claude Code Desktop | Session + workspace | Настраиваемые panes, modes детализации | Основной ход работы и дополнительные разговоры | Diff/worktree review | Progressive disclosure; conversation остаётся session-oriented |
| Linear | Issue + inbox item | List, search, peek | Priority и snooze внимания | Issue history | Notification state отдельно от состояния работы |
| Raycast | Query + selected result | Palette + action panel | Явные действия объекта | Не execution product | Быстрый доступ с читаемым scope |
| Temporal | Workflow execution | Filtered list + history/relationships | Вложенная ошибка при работающем workflow | Compact/all/timeline history | Не красить весь Task по одной неудаче |
| Dagster | Run + step | Graph/Gantt + events/logs | Состояние шага и run | Связанные reruns | Диагностический workspace; не home screen |
| GitHub Actions | Workflow run/job/environment | Dependency graph и review gate | Approve/reject выбранного deployment | Logs и run context | Разрешение рядом с последствиями |
| Grafana Explore | Query/trace/span | Correlated split views | Investigation | Span → logs | Ссылки между доказательствами; не dashboard aesthetics |

Основания строк: [Cursor docs](https://cursor.com/docs/agent/agents-window), [Warp planning](https://docs.warp.dev/agents/capabilities/planning/), [VS Code](https://code.visualstudio.com/docs/agents/run/agents-window), [Zed](https://zed.dev/docs/ai/agent-panel), [JetBrains](https://www.jetbrains.com/help/idea/tool-windows.html), [Devin Spaces](https://docs.devin.ai/desktop/spaces), [Codex launch](https://openai.com/index/introducing-the-codex-app/), [Claude Desktop](https://code.claude.com/docs/en/desktop), [Linear](https://linear.app/docs/inbox), [Raycast](https://manual.raycast.com/action-panel), [Temporal](https://docs.temporal.io/web-ui), [Dagster](https://docs.dagster.io/guides/operate/webserver), [Actions review](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/review-deployments), [Grafana](https://grafana.com/docs/grafana/latest/visualizations/explore/trace-integration/).

### 4.2 Покрытие всех исследовательских измерений

| Измерения из задания | Сильнейшие наблюдения/источники | Применение и предел знания |
|---|---|---|
| 1 IA; 2 navigation | Cursor, Devin Spaces, Linear | Object/context navigation; требует Runlith-specific mapping |
| 3 search/palette; 4 keyboard | Raycast, Linear, VS Code | Search scope, object actions, shortcut discoverability |
| 5 panes; 6 tabs/workspaces | JetBrains, Cursor 3.1, Warp tabs | Resizing/pinning/MRU; привычка пользователя влияет на выбор |
| 7 density; 8 visual hierarchy | Просмотренные Cursor/Warp/Dagster изображения | Видна относительная иерархия; скорость чтения не измерена |
| 9 typography; 10 spacing | Те же официальные изображения | Разделение prose/code и повторяемый ритм; точные tokens неизвестны |
| 11 color; 12 surfaces/borders | Те же изображения + WCAG | Нейтральный фон, локальный акцент; screenshot не доказывает контраст |
| 13 motion/transitions | Apple inspector guidance; документация панелей | Конкурентные timing values не измерялись; собственные кандидаты в §14 |
| 14 empty; 15 loading | Docs entry flows; Runlith IPC | Полного hands-on сравнения нет; сценарные требования в §10 |
| 16 error; 17 blocked/approval | Temporal, GitHub Actions, Warp permissions | Иерархия ошибки и точная область разрешения |
| 18 background; 19 attention | Warp notifications, Linear Inbox | Durable queue + selective notification; delivery latency не измерена |
| 20 agent; 21 human-in-loop; 22 approval | Warp questions, Cascade, Runlith G6/G7 | Разделять уточнение, выбор курса и авторизацию |
| 23 plan; 24 long running | Warp plan, Cursor Plan mode, Temporal | Рабочий документ и компактные milestones |
| 25 logs; 26 terminal | Dagster, Warp, VS Code accessibility | Structured events отдельно от stdout; terminal не authoritative state |
| 27 diff/code; 28 evidence | Cursor review, VS Code diff scopes, Grafana | Явная база сравнения, связанные источники |
| 29 recovery; 30 multi-agent | Temporal/Dagster, Zed, agent workspaces | Identity и causal history; конкурентные checkpoint semantics не переносимы |
| 31 accessibility | W3C, VS Code, APG | Нормативные требования и практические паттерны; аудит продуктов не выполнен |
| 32 desktop; 33 cross-platform | JetBrains floating windows, Apple, VS Code | Focus/menu/window lifecycle; OS parity требует проверки |
| 34 performance | Ограничения IPC и объёма данных Runlith | Нет измеренного рейтинга frameworks; workload benchmark нужен отдельно |

## 5. Deep dives

### 5.1 Cursor: почему restraint полезнее копирования оболочки

**D, текущий контекст.** Cursor 3 добавил отдельный Agents Window, сохранив IDE; следующая версия добавила tiled layout. Это позволяет пользователю выбирать, когда он организует агентную работу, а когда работает с кодом. Для Runlith полезно само различие режимов внимания. Отсюда не следует необходимость двух отдельных приложений. [Cursor 3](https://cursor.com/blog/cursor-3), [3.1](https://cursor.com/changelog/3-1).

**O.** На просмотренном официальном изображении Agents Window большая часть площади отдана исходному файлу; сообщение агента содержит summary и результаты проверок, а chrome остаётся нейтральным. Изображение светлое: привлекательность этого паттерна нельзя объяснить одним dark mode. Название файла, код и диагностические маркеры дают опоры для проверки сообщения. [Официальное изображение](https://cursor.com/docs-static/images/agent/file-agents-window-final.png).

**S.** Для Runlith работает принцип «рабочий объект занимает больше места, чем его пересказ». В Review это Diff/Evidence; в планировании — accepted/proposed plan; при постановке задачи — цель и критерии. Структура сокращает расстояние между утверждением и проверкой. Правое расположение чата не является причиной этого эффекта.

**D.** Plan mode отделяет подготовку плана от команды Build, позволяет редактировать план и сохранять его как файл. Runlith полезно явное действие перехода от планирования к выполнению. Но редактируемый документ Cursor не является доказательством наличия immutable accepted graph и freshness fencing. [Cursor Plan mode](https://prod.cursor.com/help/ai-features/plan-mode).

**D.** Agent Review — отдельный путь проверки изменений; его документация различает варианты review. Для Runlith это аргумент за выделенный обзор результата, но не за автоматическое доверие отзыву второй модели. [Agent Review](https://prod.cursor.com/docs/agent/agent-review).

**C.** В обсуждениях апреля 2026 пользователи просили управление расположением files/chat и жаловались на потерю привычных review/editor возможностей при переходе к Agents Window. Часть возможностей затем появилась; эти сообщения нельзя использовать как список текущих дефектов. Они показывают цену нарушения пространственной привычки и несовпадения ожиданий от нового режима. Выборка мала и самоотобрана. [Layout discussion, 21–29.04.2026](https://forum.cursor.com/t/agents-window-customize-layout/158646), [launch feedback, апрель–июнь 2026](https://forum.cursor.com/t/agents-window-is-not-good/157226).

**Граница переноса.** Не превращать Runlith Task в thread, не считать review summary verified evidence, не использовать маркетинговые масштабы Cursor Projects как обоснование первого Runlith UI.

### 5.2 Warp: объекты, прогресс и внимание

**D/O.** В текущей документации план открывается в отдельном editor, имеет историю изменений и может сосуществовать с разговором. На официальных изображениях видны две разные поверхности: документ с заголовками и короткие записи выполняемой работы. Это делает план доступным для повторного чтения без поиска сообщения в transcript. [Planning](https://docs.warp.dev/agents/capabilities/planning/), [изображение документа](https://docs.warp.dev/_astro/planning-main-view.B3ScH40z_Z30qck.webp), [изображение progress](https://docs.warp.dev/_astro/agent-plans-tasks.D4jtQV3D_ZoE1B2.webp).

**S.** В Runlith принятую PlanRevision следует читать как объект с идентичностью, provenance и delta. Список шагов агента можно показать отдельно как reported activity. Число «4 из 9» без указания принятой ревизии, веса этапов и verified obligations не означает 44% готовности результата.

**O.** На просмотренных изображениях нейтральная большая поверхность, тонкие разделители, устойчивый input area, компактные worktree/branch/context элементы. Это сохраняет пространственные опоры. Одновременно крупный текст документа и широкая conversation pane уменьшают полезную площадь кода. Изображение не доказывает пригодность такой пропорции на небольшом ноутбуке.

**D.** Обновление 14.04.2026 описывает vertical tabs, поддержку сторонних coding agents, уведомления и inline review. Это существенно для provider-neutral продукта: общая оболочка может предлагать единый способ наблюдения при разных исполнителях. Равенство возможностей отдельных providers из этого не следует. [Universal agent support](https://www.warp.dev/blog/universal-agent-support-level-up-coding-agent-warp).

**D.** Notifications разделены на completion, request и error; есть mailbox и индикаторы tabs. Это полезный двухуровневый паттерн: краткий сигнал плюс место, куда можно вернуться. Ограничение числа toast не должно означать потерю незавершённых запросов. Для Runlith pending decisions должны восстанавливаться из daemon даже после пропущенного уведомления. [Agent notifications](https://docs.warp.dev/agents/capabilities/agent-notifications/).

**D.** Agent questions поддерживают варианты ответа и пропуск. Permissions — отдельная система, включая уровни autonomy и специальные правила auto-approve. Это поведение Warp, а не модель Runlith: пропустить обычное уточнение и продолжить по best judgment допустимо только при соответствующем контракте; pending HDR или REQUIRE_APPROVAL пропуском не снимаются. Особенности Warp denylist также нельзя переносить на Runlith hard DENY. [Questions](https://docs.warp.dev/agents/local-agents/interacting-with-agents/agent-questions/), [Profiles & permissions](https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/).

**C.** В феврале 2026 часть пользователей жаловалась на неопределённость между shell input и agent input и просила вернуть явное переключение. Это свидетельство риска mode errors, не доказательство, что текущий Warp имеет тот же дефект. Современные docs уже описывают отдельные Terminal и Agent modes. Вывод для Runlith: search, message, command и разрешение операции должны иметь явно различимый контекст исполнения. [Обсуждение 27.02.2026](https://www.reddit.com/r/warpdotdev/comments/1rgiesj/some_new_settings_restore_old_agent_mode/), [текущий Warp overview](https://docs.warp.dev/).

### 5.3 Описанный пользователем Warp reference

Это анализ композиционной гипотезы из задания; исходное вложение отсутствует. Нельзя достоверно оценить его точные размеры, контраст, typography, выбранный объект, содержимое approval или поведение панелей.

| Элемент описанной композиции | Почему может работать | Как проверить для Runlith |
|---|---|---|
| Постоянная навигация слева | Даёт ориентир проекта и место возврата | Может ли пользователь различить Task и session без раскрытия всего дерева? |
| Доминирующий центр | Оставляет место для plan/diff/evidence | Хватает ли ширины review при открытом inspector? |
| Contextual agent panel справа | Держит объяснение рядом с объектом | Нужен ли он при completed Task и evidence review? |
| Global search/command | Снижает стоимость переходов | Видны ли тип и scope результата до действия? |
| Structured progress | Уменьшает необходимость читать весь поток | Основан ли каждый элемент на durable event или только narration? |
| Suggested actions и approval | Делает следующий шаг явным | Отличается ли suggestion от авторизации и решения курса? |
| Restrained chrome | Снижает конкуренцию за внимание | Сохраняются ли focus, hit targets и различимость disabled states? |

**Предварительный вывод:** сильный reference для ролей областей экрана. Недостаточное основание для обязательной постоянной трёхпанельности. Для точного визуального разбора необходим именно исходный screenshot; остальные выводы исследования от него не зависят.

### 5.4 Другие наиболее полезные references

**VS Code — привязка инструментов к выбранной session.** В Preview Agents window связанные files, changes, terminals и browser context следуют выбранной работе; scopes сравнения diff различаются. Для Runlith выбор Attempt должен столь же явно определять показываемые logs/evidence, но переключение не должно незаметно перенаправлять набираемое сообщение или изменять базу review. [Agents window](https://code.visualstudio.com/docs/agents/run/agents-window).

**Zed — честность capability-dependent UI.** Документация различает доступность steering, queue и других возможностей для native/external agent integrations. Это хороший образец отказа от ложной универсальности. В Runlith скрытый или disabled control должен объясняться возможностью daemon/provider contract, а не распознаванием brand name в frontend. [Agent panel](https://zed.dev/docs/ai/agent-panel).

**JetBrains — панели принадлежат задаче внимания.** Tool windows могут быть docked, floating, на другом мониторе; некоторые появляются в релевантном контексте. Сильная сторона — управляемый focus и возвращение к основному объекту. Риск для Runlith — слишком много chrome и настроек до появления сложных сценариев. Источник относится к IntelliJ IDEA 2026.2, обновлён 17.08.2026. [Tool windows](https://www.jetbrains.com/help/idea/tool-windows.html).

**Devin Desktop / Windsurf — объединение работы вокруг контекста.** Spaces группируют sessions, файлы и связанные материалы; Agent Command Center предлагает группировку и status-oriented обзор. Для Runlith полезна собираемость контекста, но Task уже имеет durable identity: нельзя создавать конкурирующий Space как ещё одного владельца истины. Cascade checkpoints и изменения todo не равны Runlith recovery/PlanRevision. [Spaces](https://docs.devin.ai/desktop/spaces), [Agent Command Center](https://docs.devin.ai/desktop/agent-command-center), [Cascade](https://docs.devin.ai/desktop/cascade/cascade).

**Codex и Claude Desktop — работа вне постоянного редактирования.** Февральский анонс Codex app описывает несколько project threads, worktrees и review результатов. Это датированный reference 02.02.2026, а не гарантия нынешнего UI: старый адрес Codex features при проверке перенаправил в новую документацию ChatGPT. Текущие docs Claude Desktop отдельно описывают режимы Normal/Verbose/Summary: детализацию активности можно менять без изменения результата работы. Runlith полезно разделить компактную сводку, диагностические детали и conversation. [Codex launch](https://openai.com/index/introducing-the-codex-app/), [Claude Desktop](https://code.claude.com/docs/en/desktop).

**Linear — attention имеет собственную жизнь.** Inbox позволяет работать с уведомлениями отдельно от issues; peek сохраняет место в списке. Для Runlith read/snooze — пользовательские свойства внимания, а pending HDR/Approval — серверная истина. Snooze не должен разблокировать выполнение. [Inbox](https://linear.app/docs/inbox), [Peek](https://linear.app/docs/peek).

**Raycast — действие начинается с выбранного объекта.** Action panel показывает доступные операции и shortcuts для текущего результата. В Runlith это снижает количество постоянных кнопок: Task, Attempt или Artifact получают контекстные действия. Опасная операция должна открывать review, а не исполняться обычным Enter из поисковой выдачи. [Action panel](https://manual.raycast.com/action-panel).

**Temporal — вложенная неудача не означает завершение всей работы.** Task Failures view прямо показывает running workflows с отказавшими внутренними задачами. History имеет несколько уровней детализации. Это сильнейший reference для различения failed Attempt, живого Execution и состояния Task; конкретные Workflow states Temporal не переносимы. [Temporal Web UI](https://docs.temporal.io/web-ui).

**Dagster — диагностика по причинной структуре.** Просмотренное изображение Run Details связывает последовательность/длительности шагов с событиями и выводом. Полезна синхронная навигация «выбрал шаг → увидел его данные». Такой экран оправдан при расследовании, но был бы избыточным домашним экраном Runlith. Re-execute в Dagster не обосновывает универсальное Runlith Resume. [Webserver UI](https://docs.dagster.io/guides/operate/webserver).

**GitHub Actions / Grafana — действие и свидетельство рядом с объектом.** Deployment review относится к выбранному окружению; trace-to-logs связывает конкретный span с диагностикой. В Runlith approval должен указывать точную операцию, а evidence — соответствующий criterion/Attempt/revision. Полезна адресуемость связей, а не визуальный стиль monitoring dashboard. [Deployment review](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/review-deployments), [Trace integration](https://grafana.com/docs/grafana/latest/visualizations/explore/trace-integration/).

## 6. Pattern library

Каждая строка — переносимый принцип, а не предписание копировать компонент. Поведение источника обозначено D; адаптация к Runlith — S.

| Паттерн и источник | Польза | Ограничение | Применение в Runlith |
|---|---|---|---|
| Рабочий объект рядом с разговором — [Cursor](https://cursor.com/docs/agent/agents-window) | Можно проверить сказанное, сохраняя контекст | Два постоянно широких столбца конкурируют за место | Plan/Diff/Evidence в центре; agent panel по необходимости |
| План как документ — [Warp](https://docs.warp.dev/agents/capabilities/planning/) | Повторное чтение и review без прокрутки transcript | Текстовый план сам по себе не доказывает актуальность | Accepted revision + provenance; proposal в отдельном режиме |
| Управляемые tool windows — [JetBrains](https://www.jetbrains.com/help/idea/tool-windows.html) | Пользователь распределяет внимание и экран | Настройки могут стать самостоятельной сложностью | Два-три понятных preset, pin/hide/resize, restore layout |
| Scoped tool context — [VS Code](https://code.visualstudio.com/docs/agents/run/agents-window) | Ясно, к какой работе относятся файлы и терминал | Автопереключение может уничтожить контекст review | Видимый Task/Execution/Attempt header каждой диагностической панели |
| Capability-dependent controls — [Zed](https://zed.dev/docs/ai/agent-panel) | Честные ожидания от разных providers | Непонятные disabled controls раздражают | Причина недоступности и negotiated capability |
| Compact/verbose activity — [Claude Desktop](https://code.claude.com/docs/en/desktop) | Разная глубина наблюдения без разных продуктов | Summary может скрыть существенное событие | Короткая authoritative summary; постоянный доступ к detail |
| Inbox отдельно от object state — [Linear](https://linear.app/docs/inbox) | Можно управлять вниманием без изменения работы | Пользователь может принять read за resolved | Прочитанность отдельно; pending запрос остаётся в Attention |
| Peek без потери списка — [Linear](https://linear.app/docs/peek) | Быстрое сравнение нескольких задач | Keyboard-only affordance плохо обнаруживается | Видимая команда Preview и shortcut, не только скрытая клавиша |
| Object action panel — [Raycast](https://manual.raycast.com/action-panel) | Меньше постоянного chrome; легче учить shortcuts | Большой набор команд требует хорошего ранжирования | Контекстные команды с полным названием и scope |
| Несколько представлений history — [Temporal](https://docs.temporal.io/web-ui) | Краткое понимание и возможность расследования | Всё ещё можно утонуть в событиях | Significant changes по умолчанию; raw events вторым уровнем |
| Step → correlated output — [Dagster](https://docs.dagster.io/guides/operate/webserver) | Меньше ручного поиска в логах | Визуальная временная шкала не заменяет причину | Выбор Attempt/event фильтрует logs, artifacts и evidence |
| Review gate конкретного действия — [GitHub Actions](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/review-deployments) | Понятно, что будет разрешено | Слишком широкая группировка скрывает scope | Operation, target, consequence, expiry и exact-action CTA |
| Связь evidence без копирования — [Grafana](https://grafana.com/docs/grafana/latest/visualizations/explore/trace-integration/) | Переход от утверждения к первичным данным | Корреляция может быть ошибочной | Backend-owned refs; отсутствие связи показывать явно |
| Notification + durable mailbox — [Warp](https://docs.warp.dev/agents/capabilities/agent-notifications/) | Можно пропустить сигнал и вернуться позже | Объединение сообщений может скрыть самостоятельный blocker | Group by Task, но сохранить каждый обязательный запрос |
| Inspector как дополнение к content — [Apple WWDC23](https://developer.apple.com/videos/play/wwdc2023/10161/) | Primary object сохраняет пространственную роль | Скрытая панель менее обнаружима | Inspector toggle, счетчик релевантных запросов, keyboard access |

## 7. Anti-pattern library

| Избегать | Почему плохо именно для Runlith | Допустимая замена |
|---|---|---|
| Generic ChatGPT clone | Durable identity и обязательства теряются в threads | Task-first workspace; разговор как одна из поверхностей |
| Dashboard из KPI/cards | Числа running agents/tokens не отвечают на вопрос о результате | Список задач и конкретная рабочая область |
| Narration как state | Модель может ошибиться; процесс и результат различны | Серверная state summary, сообщения помечены как сообщения |
| Бесконечный activity feed | Возврат после перерыва требует реконструировать причинность | Milestones, changed-since-last-visit, фильтруемая history |
| Скрытые автономные действия | Невозможно понять scope и последствия | Адресуемые события, receipts и явная граница полномочий |
| Кнопки Yes / Continue / Approve без объекта | Несколько разных lifecycle выглядят одинаково | Глагол + операция/решение + scope |
| Цвет как единственный статус | Неразличимость при нарушении цветовосприятия; неоднозначность зелёного | Текст, icon, форма и доступное название |
| Шесть badges у каждой строки | Полезная информация не имеет приоритета | Один основной state + одна релевантная причина |
| Постоянные три панели | Сужают Diff и Evidence даже без диалога | Pinning и устойчивое адаптивное раскрытие |
| Glass, glow, AI gradients | Декор конкурирует с сигналами внимания и снижает читаемость | Нейтральные поверхности, сдержанный семантический акцент |
| Полная IDE до первого сценария | Tool windows и settings перекрывают простую задачу supervision | Progressive disclosure по рабочему объекту |
| Monitoring dashboard вместо workspace | Пользователь наблюдает показатели, но не проверяет работу | Диагностика как отдельный режим задачи |
| Checkpoint = recovery | Возврат файлов не возвращает policy, journal и внешние effects | Различимые «просмотреть», «повторить», «продолжить», если разрешены |
| Таймер, считающий молчание согласием | HDR и Approval требуют конкретного актуального решения | Никакого auto-consent для этих контрактов |
| «Всё готово» после exit code 0 | Attempt success не выполняет trusted verification | Отдельный этап проверки и criterion evidence |
| Агентные аватары как владельцы работы | Provider identity вытесняет Task и provenance | Work item → execution → attempt; provider вторичным |
| Автоматический прыжок к новому blocker | Разрушает ввод, review и клавиатурный focus | Ненавязчивый сигнал, переход по явному действию |
| Пустые будущие разделы Memory/Council/Factories | Создают обещание неготовых возможностей | Добавлять только при наличии сценария и контракта |

Это выводы S, опирающиеся на контракты R и исследованные patterns. Пример timer-based planning из исторических Devin release notes полезен как отрицательный перенос, но не как утверждение о текущем default Devin. [Devin release notes 2025](https://docs.devin.ai/release-notes/2025).

## 8. Runlith UX principles

1. **Показывать работу по её устойчивой идентичности.** Навигация и ссылки переживают provider session и смену окна.
2. **Видимая истина имеет источник и время.** Каноническое состояние, модельное объяснение и устаревшее наблюдение различимы.
3. **Каждое важное утверждение ведёт к evidence.** Необоснованный summary не получает verified treatment.
4. **У каждого действия видны объект, scope и последствия.** Действие получает полномочия от agentd.
5. **Внимание запрашивается пропорционально необходимости.** Фоновая работа не требует постоянного наблюдения.
6. **Первый уровень прост, второй достаточно глубок для расследования.** Упрощение достигается раскрытием, а не слиянием доменных объектов.
7. **Пользователь управляет пространством и focus.** Важные новые события не перестраивают окно без его действия.
8. **Ограничения продукта видимы до обещания результата.** Unsupported, stale, disconnected и unavailable не маскируются под pending success.

Общая HCI-опора — проверенные guidelines о границах системы, понятных ожиданиях и управлении взаимодействием; она не доказывает конкретный layout Runlith. [Microsoft Guidelines for Human-AI Interaction, CHI 2019](https://www.microsoft.com/en-us/research/publication/guidelines-for-human-ai-interaction/).

## 9. Information architecture

### 9.1 Предлагаемая навигация

Постоянная левая область содержит Project switcher, Tasks и Attention. Settings/Providers/Policy и состояние соединения с agentd находятся в служебной нижней зоне. Global search доступен из любого экрана. Последние/закреплённые Task — способ быстрого возврата, не новый доменный объект.

Tasks открывает фильтруемый список или дерево. «В работе», «Нужно решение», «Недавно завершены» — сохранённые представления существующих объектов. Execution Explorer для нескольких задач — вторичный маршрут диагностики, а не обязательный главный пункт первого запуска.

Task Workspace содержит устойчивый header и вкладки **Overview, Plan, Changes, Evidence, History**. Это кандидат IA. Execution/Attempt открываются из Overview/History как адресуемый detail; Agent panel и diagnostics — области текущего Task, а не шестая конкурирующая сущность верхнего уровня. Проверка названий и числа вкладок входит в usability work.

Provider Session, Policy decision, отдельный log stream и конкретный Artifact принадлежат contextual inspector/detail. Они доступны через поиск и ссылки, но не требуют постоянных глобальных разделов. Будущие Memory/Council/Delivery не размещаются в первом navigation только на основании долгосрочного SoT.

### 9.2 Полное отображение доменных объектов

| Объект | Предлагаемое UI-представление | Связи, которые обязаны остаться видимыми | Текущая граница |
|---|---|---|---|
| Project | Switcher, имя и repository context | Владелец Task collection; расположение рабочей среды | Богатый project browser требует отдельного read contract |
| Task | Главный workspace, строка списка | Objective, lifecycle, criteria, scope, родитель/дети | Basic read есть; полный read metadata недостаточен |
| Task hierarchy | Раскрываемое дерево + breadcrumb | Parent-child отдельно от dependency edges | Не подменять деревом Plan DAG |
| Plan | Основная поверхность Plan с accepted head | Branch/scope и текущая ревизия | Query surface требуется |
| PlanRevision | Read-only revision header, delta и provenance | Parent revision, source evidence, validation | Immutable; не «редактировать принятую ревизию» |
| PlanChangeSet | Proposal/diff относительно accepted revision | Proposed additions/removals, risk, validation | Не рисовать как уже действующий план |
| PlanningInputSnapshot | Details planning operation | Источник authority и freshness | Отличается от результата планирования |
| PlanningRun | Отдельная операция с временем/бюджетом/исходом | Input snapshot, generation, resulting revision или refusal | Storage states существуют; UI read surface отсутствует |
| Execution | Список внутри Task; отдельный detail route | Task ID, Attempts, workspace binding, live observation | Нет универсального durable execution status enum |
| Attempt | Нумерованный список с outcome и деталями | Execution, provider_ref, evidence, predecessor context | Последовательные попытки; concluded outcome immutable |
| Provider Session | Свернутый provider/session inspector | Конкретный Attempt и capability limitations | Session ID/transcript отсутствуют в текущем execution wire |
| HumanDecisionRequest | Typed Attention item и decision detail | Reason, scope, allowed options, evidence, validity | PENDING/RESOLVED/CONSUMED/SUPERSEDED |
| Approval | Отдельный typed Attention item и operation review | Точный request, policy, expiry, effect | approved не означает executed |
| Policy decision | Policy explanation рядом с отказом/approval | ALLOW/DENY/REQUIRE_APPROVAL и origin | Hard DENY не превращается в approve action |
| Verification evidence | Criterion-oriented Evidence view | Verdict source, snapshot/revision, Attempt/artifact | G8 scope отделить от уже существующих evaluation contracts |
| Git/worktree state | Краткий context header + подробности Changes | Recorded base, HEAD/index/worktree, ownership lease | Worktree lifecycle и observed dirtiness различны |
| Events | Timeline существенных transitions + raw detail | Durable identity, sequence, causal references | Нужен bounded/paged IPC read |
| Logs | Диагностическая панель, фильтр Attempt/source | Operational output, время и доступная корреляция | Не доказательство само по себе; не Event Journal |
| Artifacts | Список результатов с viewer/download/open action | Producing Attempt, type, provenance, revision | Preview capabilities и доверие требуют контракта |
| Diff | Полноширинный Changes view | Base_revision, слои изменений, источник | Не молча сравнивать с текущей main |
| Recovery state | Объяснение posture и допустимых действий | Execution/Attempt, daemon observation, причина | Не новый общий enum и не универсальный Resume |
| Background daemon activity | Служебный indicator + diagnostic detail | Connected/compatibility/freshness отдельно от Tasks | Ping доказывает отзывчивость control channel, не прогресс provider |

Основания R: [Task Engine](../../architecture/task-engine.md), [ADR-0027](../../architecture/adr/ADR-0027-g7-planning-orchestration-contract.md), [ADR-0028](../../architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md), [Workspace](../../architecture/workspace.md), [human decision model](../../../crates/plan/src/human_decision.rs).

### 9.3 План: список сначала, граф по необходимости

Default Plan view — иерархический список текущей ветви: цель, obligations, dependencies, state/readiness и причина. Graph view нужен для анализа связей, параллельности и критического blocker; он не должен быть единственным способом чтения плана.

Показывать выбранную branch и ближайшие зависимости. Не выводить весь проект как паутину. G7 имеет ограничители depth/fanout/branch size; это полезная верхняя граница fixture, а не основание всегда показывать максимальный граф. История ревизий должна отвечать «что изменилось и почему», а не только выдавать timestamps. Изменение criteria или scope заметнее косметического переименования.

Readiness выводится отдельно от lifecycle: READY не обещает RUNNABLE. Provider-reported todo не получает визуальное обозначение accepted plan. При stale input или changed head показывается причина невозможности применить старое предложение; обновление не выполняет silent rebase. [ADR-0027](../../architecture/adr/ADR-0027-g7-planning-orchestration-contract.md), [ADR-0028](../../architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md).

### 9.4 Search, tabs и возврат в контекст

Global search ищет адресуемые Task, Execution, Attempt, Decisions и Artifacts по имени/ID. Выдача показывает тип, Project, короткий контекст и свежесть данных; недоступные поисковые классы не имитируются. Поиск файлов относится к repository context и требует отдельного безопасного read contract.

Command palette использует тот же способ вызова действий, что меню и кнопки. Команды явно называют scope: «Открыть логи Attempt …», «Перейти к принятому плану Task …». Опасные команды открывают decision/review surface. Generic natural-language input не исполняет shell.

Различать navigation tabs объектов и вкладки внутри Task. MRU-переключение полезно для нескольких задач; закрытие вкладки закрывает представление. Back возвращает к прежнему списку, фильтру, scroll и выбранной строке. Pinning закрепляет место в UI и не меняет приоритет выполнения.

## 10. Screen inventory

Минимальный серьёзный продукт — набор связанных поверхностей, а не множество самостоятельных dashboard.

| Поверхность | Минимальное содержание | Критический acceptance scenario |
|---|---|---|
| Connection / compatibility | Daemon reachability, negotiated protocol, понятная ошибка | Несовместимость не выглядит как пустой список задач |
| Project entry / selection | Выбор контекста и Tasks | Пользователь понимает, где создастся Task |
| Task collection | Search, filters, tree/list, state + reason | Нужный blocker находится без открытия всех задач |
| Task authoring | Objective, scope, criteria, risk, review перед create | Сохранённые данные доступны после повторного открытия |
| Task Workspace | Summary, active work, next action, links | За короткий просмотр понятно, что происходит |
| Plan/revision review | Accepted/proposed, delta, dependencies, provenance | Нельзя перепутать предложение с принятой ревизией |
| Attention | Разные типы HDR/Approval, scope, freshness | Прочитанный pending запрос остаётся обнаружимым |
| Human Decision detail | Причина, evidence, allowed choices, effect | Выбор записан; применённость показана отдельно |
| Approval detail | Exact operation, authority, consequence, expiry | Нет approve для hard DENY или stale request |
| Changes / Evidence | Diff base, criteria, verdicts и первичные данные | Успешный Attempt не отображается как VERIFIED |
| Execution / Attempt diagnostics | Attempts, events, logs, recovery posture | Ошибка одного Attempt не перекрашивает весь Task |
| Settings / diagnostics | Providers, policy visibility, keyboard, density, accessibility | Возможности и ограничения объяснены без frontend business rules |

Task authoring не должен обещать редактирование metadata после создания: amendment TASK-0084 намеренно не добавил такой API. Если продукту нужен editable draft, сначала нужно отдельное решение о lifecycle/IPC. Сохранение незавершённой формы локально — presentation draft, а не изменение durable Task. [ADR-0028 amendment (line 210)](../../architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md).

| Состояние поверхности | Требование |
|---|---|
| Нет Project / Tasks | Объяснить следующий шаг; одна подходящая команда создания/выбора |
| Фильтр дал ноль результатов | Показать действующие фильтры и Clear filters; не onboarding |
| План ещё не создан | Показать отсутствие accepted Plan и явное действие planning |
| Работа ещё не запускалась | Не показывать пустой terminal как главный объект |
| Нет evidence | «Доказательства пока отсутствуют»; это не failed и не verified |
| Initial loading | Стабильный каркас/заголовок; не ложные числовые данные |
| Refresh | Сохранять прочитанные данные, отмечать freshness; не сбрасывать scroll |
| Lost connection | Last known state + время наблюдения; не утверждать, что работа остановлена |
| Request rejected | Причина рядом с действием; сохранить введённый текст и review context |
| Ambiguous mutation result | «Результат запроса уточняется»; перечитать authoritative object вместо слепого replay |
| Unsupported capability | Явно назвать требуемую возможность/совместимую версию |
| Удалённый или недоступный artifact | Сохранить provenance и объяснить отсутствие содержимого |

## 11. Task Workspace alternatives

Это три разные структуры распределения внимания. Сравнение — аналитическая гипотеза, а не результат измеренного A/B test.

### A. Постоянный трёхпанельный control workspace

    Project / Tasks | Task: Overview / Plan / Changes / Evidence | Agent / Decision
                    |                                           |
                    |                 Primary work              |

Плюсы: постоянная видимость контекста разговора; лёгкая постановка и корректировка задачи; удобно одновременно читать proposed plan и обсуждать его. Минусы: центральная область сужается; completed/review workflows платят постоянную цену за пустую или нерелевантную панель; велик риск превратить inspector в универсальный transcript.

При 1440 px условные 220 px navigation, 360 px inspector и два промежутка по 12 px оставляют 836 px центру. При 1280 px — 676 px. Это простая геометрия кандидата, не измерение Warp. Side-by-side diff в такой ширине часто потребует скрытия панели или unified mode. На большом мониторе ограничение слабее.

Хороший кандидат, если наблюдения покажут, что основная часть времени уходит на активную совместную работу и непрерывные уточнения. Decision panel не должен сам переключаться с одного pending request на другой во время чтения.

### B. IDE: центральные tabs и нижняя execution/evidence панель

    Project / Tasks | Task / Plan / Diff / Artifact tabs
                    | Primary document or code
                    |------------------------------------------
                    | Attempts / Events / Logs / Terminal

Плюсы: много горизонтальной площади для code/diff; привычная диагностика; можно сопоставлять ошибку и лог. Минусы: execution detail легко начинает доминировать; Outcome/Evidence теряются среди tools; bottom panel уменьшает высоту длинного плана; Task может восприниматься как ещё один файл.

Хороший кандидат для hands-on debugging, частого чтения кода и расследования failed attempts. Слабее для возвращения к нескольким автономным задачам, если их summary спрятаны за редакторными вкладками.

### C. Адаптивный inspector с устойчивой центральной задачей

    Project / Tasks | Task Workspace
                    | Overview / Plan / Changes / Evidence / History
                    | Full-width selected work object
                    | [Inspector: explicit open, pin or close]
                    | [Diagnostics: explicit open when needed]

Плюсы: площадь достаётся текущему рабочему объекту; chat может исчезать целиком; хорошо для review и небольших экранов. Минусы: дополнительные детали менее обнаружимы; динамическое раскрытие может нарушать привычку; без явных affordances человек пропустит важный запрос.

Условия жизнеспособности: доступный toggle с текстом; Attention indicator остаётся видимым; panel не открывается и не забирает focus сам; пользователь может pin и сохранить предпочтение; при недостатке ширины инспектор открывается отдельным detail view, а не насильно сжимает документ. Это не предложение непрерывно «умно» перестраивать экран.

### 11.1 Сравнение по workflow

| Workflow | A | B | C |
|---|---|---|---|
| Создать Task и обсудить scope | Сильный | Умеренный | Сильный при открытом interaction panel |
| Проверить plan proposal рядом с вопросом | Сильный | Умеренный | Сильный при закреплённом inspector |
| Читать широкий diff и evidence | Ограничен шириной | Сильный | Сильный |
| Разобрать Attempt failure с logs | Умеренный | Сильный | Сильный после раскрытия diagnostics |
| Вернуться после 30 минут отсутствия | Сильный при authoritative summary | Риск editor clutter | Сильный при хорошей Overview |
| Следить за несколькими Task | Зависит от navigation/Attention | Зависит от navigation/Attention | Зависит от navigation/Attention |
| Completed Task без разговора | Лишняя постоянная площадь | Приемлемый | Сильный |
| 1280 px и увеличенный текст | Слабее | Умеренный | Сильнее при смене inspector на detail |
| Запрос пришёл во время ввода | Устойчив при запрете автофокуса | Устойчив при запрете автофокуса | Требует особенно строгого запрета автоперестройки |
| Обучение новому пользователю | Видимость функций выше | Высокая IDE-нагрузка | Нужна хорошая discoverability |

**Рекомендация средней уверенности:** начать прототипирование с C и дать pinning, позволяющий получить A; B сохранить как отдельный эксперимент для диагностических сценариев. Это приоритет тестирования, не выбор окончательного UI.

### 11.2 Масштаб окна и несколько окон

На 1440 × 1000 проверить default density и оба review modes. На 1280 × 800 — collapsed navigation и отдельный inspector route. На 1920+ разрешить закрепление details и сравнение объектов, сохраняя читаемую ширину prose. Размеры являются design fixtures, а не поддерживаемыми разрешениями готового продукта.

Каждое окно должно показывать явный Project/Task context; detached diagnostics сохраняет выбранный Attempt. Решение, принятое в другом окне, обновляет локальное представление и повторно проверяется сервером. Несколько окон не создают несколько approval lifecycles и не дублируют выполнение. Закрыть окно, отменить Execution и остановить daemon — три разных действия.

### 11.3 Как выбрать победителя

Провести formative within-subject test на 8–12 разработчиках с разным опытом IDE и автономных agents, меняя порядок A/B/C. Это число — практический план, не статистическая гарантия. Для всех вариантов использовать одинаковые задачи, данные и visual styling.

Сценарии: понять задачу после перерыва; отличить succeeded Attempt от verifying Task; найти основание изменения PlanRevision; принять HDR; отклонить stale Approval; проверить diff/evidence; расследовать потерю daemon connection. Дополнительно — узкое окно, keyboard-only и увеличенный текст.

Измерять правильность идентификации объекта/scope, время нахождения blocker и evidence, число смен контекста, случайные команды, субъективную нагрузку. Нулевое число ошибочных approvals в малой выборке — критерий продолжения, но не доказательство безопасности. Предварительные цели вроде «восстановить контекст за 20 секунд» и «найти evidence за 30 секунд» должны быть согласованы по реальным fixtures, а не объявлены установленными нормами.

## 12. State / attention model

### 12.1 Сначала тип состояния, затем цвет

Нужны несколько независимых слоёв представления:

1. Канонический lifecycle Task.
2. Readiness/blocking reason для конкретного scope.
3. Состояние Execution/Attempt и наблюдаемая живость работы.
4. Наличие HDR/Approval и их validity.
5. Verification verdict и связь evidence с проверенным snapshot.
6. Свежесть клиентского наблюдения и состояние соединения.

Не выводить все слои как badges. Default header показывает lifecycle, одну наиболее важную причину и следующую допустимую команду. Details раскрывают остальные измерения. Выбор причины должен быть объяснимым; полноценная authoritative projection требует query contract.

### 12.2 Canonical Task states

| Канонический state | Кандидат подписи | Визуальное внимание | Следствие для действий |
|---|---|---|---|
| DRAFT | Черновик | Нейтральное | Подготовить данные; текущий API не разрешает произвольный edit metadata |
| READY | Готова к запуску | Нейтральное; доступная команда выделена | Проверить readiness/accepted Plan; READY не равно RUNNABLE |
| CLAIMED | Принята в работу | Сдержанный синий маркер | Не заявлять, что provider уже выполняет команды |
| RUNNING | Выполняется | Синий status icon и фактическая activity line | Показывать execution_live/freshness; не обещать прогресс |
| BLOCKED | Заблокирована | Причина важнее общего badge; amber при нужном действии | Перейти к blocker; scope не расширять автоматически |
| SUBMITTED | Результат представлен | Нейтральное/синее | Агент представил результат; проверка ещё не подтверждена |
| AWAITING_REVIEW | Ожидает review | Amber только при реальном человеческом действии | Не считать всякий review ожиданием пользователя |
| VERIFYING | Проверяется | Синий, отдельный verification context | Показать критерии и ход проверки, если данные доступны |
| RETRY_READY | Готова к повторной попытке | Нейтральное или amber по причине | Это готовность, а не уже выполняющийся retry |
| COMPLETED | Завершена | Зелёный icon + ссылка на основание | Никакого повторного action-required сигнала |
| FAILED | Завершилась неудачей | Красный label; внимание по последствиям | Показать фактическую причину и разрешённые дальнейшие действия |
| CANCELLED | Отменена | Нейтральный terminal icon | Не выдавать за failure или success |

FAILED и CANCELLED присутствуют в canonical vocabulary, но это не разрешение создавать произвольные UI transitions. В текущем TaskLifecycleOp нет универсальных Fail/Cancel operations. CancelExecution не равен CancelTask. [Task model / lifecycle](../../architecture/task-engine.md), [Task proto](../../../crates/ipc-contracts/proto/task_core.proto).

### 12.3 Запрошенные presentation labels без изобретения domain states

| Label | Реальное основание | Что показывать |
|---|---|---|
| RUNNING | Task RUNNING, отдельно live execution observation | «Выполняется» и время последнего подтверждённого наблюдения |
| WAITING | Readiness/dependency/policy reason или ожидаемая операция | «Ожидает Task … / policy …»; не новый TaskState |
| BLOCKED | Task BLOCKED и известный blocker | Причина и локальная область блокировки |
| NEEDS HUMAN | Pending действительный HDR | «Требуется решение: …» со ссылкой на decision |
| REQUIRE APPROVAL | Policy REQUIRE_APPROVAL и связанный request | «Требуется разрешение операции: …» |
| FAILED | Task FAILED либо outcome конкретного Attempt | Всегда назвать уровень: «Attempt 1 завершился неудачей» |
| RECOVERING | Фактически принятая recovery operation и наблюдаемый ход | Временная подпись «Восстановление запущено»; не durable TaskState |
| Recovery required | Backend-known stranded/admitted posture | Amber explanation + только поддерживаемые команды |
| VERIFIED | EvaluationVerdict VERIFIED для конкретного subject | «Проверено» с criterion/snapshot/provenance |
| COMPLETED | Task COMPLETED | «Завершена»; evidence доступны по ссылке |
| Retrying | Новая открытая Attempt после предыдущей неудачи | «Попытка 2 выполняется»; не выводить из RETRY_READY |
| Stale | Устаревший snapshot/head/request либо client observation | Уточнить, что устарело; блокировать только затронутые действия |

Пример вымышленного fixture: Task VERIFYING, Attempt 1 TIMED_OUT, Attempt 2 SUCCEEDED. Правильный главный статус — «Проверяется». Историческая неудача первой попытки остаётся в истории, но не делает весь workspace красным. Если evidence относится к старому base/snapshot, рядом показывается устаревшая привязка; старый VERIFIED verdict не переписывается.

### 12.4 Объектные lifecycle

| Объект | Существующие значения | Правило UI |
|---|---|---|
| Attempt | OPEN на wire; SUCCEEDED, FAILED, TIMED_OUT, CANCELLED | OPEN — отсутствие concluded outcome, не гарантия живого процесса |
| PlanningRun | ACTIVE, SUCCEEDED, REFUSED, SUPERSEDED, FAILED | Успех planning operation не равен успеху Task |
| HDR | PENDING, RESOLVED, CONSUMED, SUPERSEDED | RESOLVED: выбор записан; CONSUMED: применён; superseded — исторический запрос |
| Approval | pending, approved, denied, consumed | approved не означает consumed; consumed не доказывает успех операции |
| Policy decision | ALLOW, DENY, REQUIRE_APPROVAL | ALLOW — разрешение, не выполнение; DENY не имеет approve shortcut |
| EvaluationVerdict | VERIFIED, REJECTED | Оценка определённого результата, не общий цвет provider |
| Worktree lease | Allocated, Ready, CleanupPending, Removed | Lifecycle ownership отдельно от Clean/Dirty/Missing observations |

PlanningRun values проверены в текущем storage implementation; их экспорт в UI ещё требует явного query contract. Expired/stale нельзя добавлять в эти enums ради удобного badge: это дополнительные признаки validity, пока backend contract не определит иначе. [Planning store](../../../adapters/storage-sqlite/src/planning_control_store.rs), [HDR model](../../../crates/plan/src/human_decision.rs), [G6 enforcement](../../development/G6_APPROVAL_HUMAN_DECISION_ENFORCEMENT.md), [Evaluation verdict](../../../crates/evaluation/src/verdict.rs).

### 12.5 Attention policy как представление

| Уровень внимания | Примеры | Surface | Notification |
|---|---|---|---|
| Немедленное осмысленное действие | Действительный HDR/Approval, работа которого заблокирована | Persistent Attention item + local callout | Один OS/in-app сигнал, если пользователь вне контекста и разрешил уведомления |
| Требуется review в удобное время | Результат готов к человеческой проверке | Attention/Tasks view | Сгруппированный или обычный сигнал по предпочтению |
| Диагностическое исключение | Stranded execution, недоступный artifact, отказ provider | Local explanation + diagnostic route | Если затрагивает ожидаемый результат; без потока повторов |
| Обычная фоновая активность | Новый milestone, независимая Attempt работает | Небольшой indicator, history | Обычно без toast |
| Информация о завершении | Task завершена, evidence готовы | Положительный terminal state | Опционально; не конкурирует с pending requests |

Это presentation policy, а не новая domain priority enum. Age, expiry и impact помогают сортировке; приоритет не должен определяться только красным цветом или количеством токенов. Для active Task локальный callout обычно лучше дублирующего toast.

Не отбирать keyboard focus; не открывать modal из background event; не назначать обычной engineering-задаче emergency/critical notification. Уважать OS notification settings и Focus. При отказе в OS notifications продукт остаётся полноценным через Attention. [Apple notifications](https://developer.apple.com/design/human-interface-guidelines/managing-notifications).

Прочитанность, snooze и закрытие toast не изменяют pending request. Запрос, утративший validity во время review, остаётся читаемым, но получает объяснение и недоступную mutation action; пользователь может открыть текущий запрос. Не заменять весь panel молча другим запросом.

### 12.6 Recovery: точные различия

«Нет live background run» с открытым Attempt не означает автоматически «можно безопасно повторить». Текущий GetExecution прямо допускает stranded/concluding posture. ResumeExecution разрешён в узком admitted-before-attempt случае и сохраняет Execution и зарезервированную Attempt identity. Другие recovery сценарии требуют отдельной backend возможности.

Кнопки, если доступны, должны различать: посмотреть прошлый результат; запросить отмену Execution; продолжить admitted execution; начать новую разрешённую попытку; открыть диагностику. Provider switch и restore checkpoint не выводятся из одной универсальной кнопки. После CancelExecution показать «Отмена запрошена», пока наблюдение не подтвердит outcome. [Execution IPC](../../../crates/ipc-contracts/proto/execution_core.proto), [ADR-0025](../../architecture/adr/ADR-0025-g5-post-admission-recovery.md).

Закрытие Task view или отключение IPC-клиента не должно превращаться в запрос отмены. При этом полный lifecycle работы с закрытым desktop приложением, restart/orphan handling и соответствующие системные гарантии относятся к будущему G11; их готовность не следует из текущего background RunExecution. UI specification должна отдельно описать close view, quit application и shutdown daemon.

## 13. Agent interaction model

### 13.1 Что где живёт

| Surface | Подходящее содержание | Что туда не переносить |
|---|---|---|
| Chat / interaction | Намерение, уточнение задачи, объяснение, запрос на анализ | Authoritative lifecycle, единственный экземпляр approval или evidence |
| Structured state | Task, accepted Plan, Attempt outcome, readiness, freshness | Предположения модели как подтверждённые факты |
| Inline suggestion | Предлагаемый следующий шаг с источником и scope | Автоматическую mutation при просмотре |
| Human Decision | Причина остановки курса, допустимые варианты, evidence, последствия | Произвольное yes/no вместо RESUME/REPLAN/ABORT |
| Approval | Разрешение точной операции в заданных границах | Общую смену стратегии или обход hard DENY |
| Notifications | Сигнал «здесь требуется внимание», deep link | Полный decision review, секреты, запуск без проверки контекста |
| Background activity | Краткое наблюдение и число задач, которым нужно внимание | Непрерывную ленту токенов или обязательный dashboard |
| History / diagnostics | Durable events, Attempts, logs и provenance | Смешанный поток без типа и причинных связей |

### 13.2 Persistent chat, tab, overlay или contextual panel?

Default для прототипа: contextual panel, который можно закрепить. Разговор не должен быть обязательным обрамлением completed Task, diff review, поиска, evidence reading или решения approval. При коротком пояснении достаточно inline expansion. Длинный разговор может открываться самостоятельной адресуемой поверхностью.

Overlay годится для краткого ввода и поиска, но плохо подходит для длинного review, сравнения evidence и необратимого выбора: он перекрывает нужный контекст. Вкладка conversation полезна для возвращения к истории, однако может спрятать текущий запрос; Attention остаётся независимым.

Чат исчезает целиком, когда нет релевантного диалога или пользователь сосредоточен на результате. Это не удаление истории. Default open/closed state ещё нужно проверить по реальному времени использования.

### 13.3 Адресуемость разговора

У composer всегда видны Task и получатель/контекст. Выбор другого Attempt для диагностики не должен молча перенаправлять незавершённое сообщение. Для queued/steering message показывать target и статус доставки, если такие возможности появятся в контракте.

Task-level conversation и provider transcript нельзя считать одним потоком без решения о continuity. Текущий execution IPC не имеет transcript/steering API. Поэтому UI research может требовать interaction surface, но не объявляет реализованным бесшовное продолжение разговора между Attempts/providers.

Скрытую chain-of-thought показывать не требуется. Достаточно краткого explanation, выбранного действия, результата tool operation и ссылок на evidence. «Агент думает» без проверяемого этапа не является progress evidence.

### 13.4 HumanDecisionRequest

Decision detail должен содержать: идентичность Task/branch/scope; canonical reason; почему дальнейшая работа остановилась; исходные evidence; допустимые choices; рекомендацию отдельно от выбора пользователя; последствия каждого варианта; актуальность preconditions и expiry.

Текущие canonical reasons — RISK_ESCALATION, NO_PROGRESS, PLANNER_BUDGET, PLANNER_BLOCKED. Choices — RESUME, REPLAN, ABORT. TAKEOVER отсутствует в текущем enum и не должен появляться как готовая кнопка. Доступные choices берутся из запроса; UI не предлагает все три автоматически.

Рекомендация не считается предвыбранным согласием. Нельзя использовать таймер для применения выбора. После решения показывать «Решение записано» и затем «Применено», когда это подтверждено. Разные стадии не требуют двух человеческих подтверждений: один явно описанный пользовательский intent может покрывать последующие команды только в рамках согласованного backend/control contract. UI не выдумывает собственный workflow применения.

Для V9 RESUME важно показать, что будет принят именно рассмотренный proposal с его identity/hash, а не заново вызван planner. Если head или authority изменились, старое решение нельзя превратить в silently rebased continuation. [Human Decision contract](../../../crates/plan/src/human_decision.rs), [ADR-0028](../../architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md).

### 13.5 Approval

Operation review отвечает на пять вопросов: **что**, **где**, **от чьего имени/по какой policy**, **с какими последствиями**, **в течение каких границ**. Точные названия полей определяет contract; чувствительные значения должны быть безопасно представлены backend.

Кандидат CTA: «Разрешить [операцию] один раз» — только если request действительно одноразовый; иначе название точно отражает реальную семантику. Альтернатива — «Отклонить операцию». «Continue» не объясняет scope. Отказ не обязан означать Abort Task: дальнейшую реакцию определяет backend.

Просмотр evidence, выбор варианта и запись решения не должны приводить к расширению scope. После approved UI ждёт consumption/result, не повторяет решение при timeout. Для ambiguous IPC result сначала читается authoritative request. Hard DENY объясняется как отказ policy; рядом не появляется approve control. [Approval IPC](../../../crates/ipc-contracts/proto/policy_core.proto), [G6 enforcement](../../development/G6_APPROVAL_HUMAN_DECISION_ENFORCEMENT.md).

### 13.6 Evidence как интерфейс доверия

Каждое проверяемое утверждение связывается с subject/criterion, artifact или event, producing Attempt/validator, проверенным snapshot/revision и временем. Если часть данных отсутствует, это явно обозначается; UI не достраивает provenance.

Представление различает: agent-reported claim; наблюдённый результат инструмента; trusted evaluation verdict; human decision. Это классы представления источника, не предложение новых доменных enums. Наличие Execution receipt подтверждает provenance/выполнение в пределах контракта, но не доказывает correctness и не заменяет authorization.

Default Evidence view организован по acceptance criteria, а не по типам файлов. Из строки можно перейти к отчёту, лог-фрагменту, diff или artifact. После изменения кода старое evidence сохраняется с прежней привязкой; «не соответствует текущему snapshot» не переписывает прошлый verdict.

### 13.7 Multi-agent без театра

Основная визуализация — Task hierarchy/Plan dependencies и реально активная работа. Несколько providers/agents видны в контексте соответствующих Attempts или будущих typed assignments. Не рисовать отдельные движущиеся аватары или рой карточек ради демонстрации автономности.

Показывать причинные связи: какая задача ожидает другую, какое evidence позволило revision, какой HDR блокирует branch. Последовательные retries одного Execution не изображать параллельными agents. Opaque role labels G7 не дают оснований создавать полноценные Role/Mission/Council controls до появления их контрактов.

## 14. Design-system foundations

### 14.1 Durable semantics и изменяемая эстетика

Долговечными должны быть роли: primary/secondary text; canvas/workspace/inspector/overlay; focus/selection; active/positive/attention/danger; provenance и freshness. Конкретный hex, family, radius и pixel spacing — заменяемые кандидаты. Не кодировать Task states напрямую в сырые цвета и не делать teal универсальным «AI-цветом».

| Основа | Кандидат | Причина и проверка |
|---|---|---|
| Spacing | 4, 8, 12, 16, 24, 32 px; 2 px только micro-alignment | Повторяемый ритм без раздувания rows; проверить длинные labels |
| Body typography | System sans, 14/20 px | Читаемый prose; поддержка кириллицы и OS rendering |
| Metadata | 13/18 px; 12 px только для второстепенного | Важные причины/expiry не мельче основного содержания |
| Headings | 18–20 px, умеренный weight | Иерархия без SaaS hero typography |
| Code/terminal | System/installed mono, кандидат 13/20 px; настройка размера | Разделение prose/code; не навязывать одну font family |
| Density | Compact rows 28 px, default 32 px, comfortable 40 px | Один компонент и разные metrics, не три разных продукта |
| Radius | 4 px controls, 6 px grouped panels, 8 px overlays | Узнаваемые boundaries; не превращать строки в cards |
| Icons | Простые line icons 16/18/20 px, text labels важных действий | Icon не должен быть единственным объяснением статуса |
| Borders | Тонкие разделители областей; без рамки вокруг каждого текста | Поверхности группируют содержимое; интерактивные границы проверять отдельно |
| Elevation | В основном flat; тень для popover/modal | Elevation обозначает наложение, не важность каждого блока |
| Motion | Кандидат 120–180 ms для раскрытия; reduced-motion equivalent | Не анимировать каждый event и не создавать пульсирующее поле |

Размеры — S, не измеренные tokens Cursor/Warp. Compact density не уменьшает hit target до размера иконки. Пользовательская настройка размера текста независима от density.

### 14.2 Surface и color candidates

| Роль | Кандидат | Использование |
|---|---|---|
| Canvas | #101419 | Базовая оболочка |
| Workspace | #15191F | Основная область работы |
| Secondary surface | #1B2129 | Inspector/вложенные группы |
| Overlay | #242C36 | Меню, palette, popover |
| Primary text | #E6E9EF | Основная информация |
| Secondary text | #A4ABB7 | Metadata, не disabled control |
| Active/focus/selection accent | #76A9FA | Синий; назначение уточняется формой/контекстом |
| Positive | #82C99A | Completed/verified/allow, с разными labels/icons |
| Secondary relation accent | #69BFB8 | Teal для вторичных связей/сравнения; не новый status |
| Attention | #E9B86E | Необходимое решение, предупреждение об актуальности |
| Danger | #F18A8A | Failure/deny/destructive action, с пояснением уровня |

Математическая проверка пары foreground → #15191F дала contrast ratios: primary text 14.50:1; secondary 7.63:1; blue 7.41:1; green 9.05:1; teal 8.18:1; amber 9.70:1; red 7.34:1. Расчёт выполнен по относительной luminance sRGB. Это **не** визуальная или WCAG-проверка готового UI: другие surfaces, opacity, disabled states, selection fills и syntax colors ещё не проверены.

Светлый semantic color на тёмном фоне не означает, что белая надпись на кнопке того же цвета пройдёт contrast. Для filled action нужен отдельно проверенный foreground. Полупрозрачные state backgrounds также проверяются после compositing.

### 14.3 Focus, selection и действия

Focus ring — кандидат 2 px с 2 px отступом; он различим на всех соседних поверхностях. Selection — отдельный слабый fill плюс marker/label, чтобы selected row и keyboard focus не сливались. Не полагаться на изменение оттенка одной тонкой линии.

Primary action один на текущую surface, если существует ясный следующий шаг. В read-only review кнопка может отсутствовать. Destructive action не получает default focus в диалоге. Context menu показывает те же названия и shortcuts, что palette; pointer menu не единственный путь к функции.

### 14.4 Lists, trees, tables и split panes

Task list: objective, lifecycle, relevant reason, last meaningful change; вторичные колонки включаются по необходимости. Длинный путь допускает middle truncation с доступным полным значением и Copy. Не обрезать основную причину отказа только tooltip.

Tree используется для parent-child, table — для сравнения Attempts/Evidence, graph — для dependencies. Не пытаться представить все три отношения одним бесконечным деревом.

Panes имеют понятный divider, keyboard resize, collapse/restore и minimum readable width. Resize сохраняет читабельность и не меняет domain context. Narrow mode выбирается по реальной доступной ширине; пользователь понимает, как вернуть inspector.

### 14.5 Code, diff, terminal и logs

Default diff — unified на узком экране, side-by-side при достаточной ширине и предпочтении пользователя. Base revision видна; additions/deletions имеют знаки и доступную текстовую форму. Binary/large/generated files требуют отдельного объяснения, а не зависшего viewer.

Logs имеют filter/source/Attempt scope, понятную остановку follow и команду перехода к новому выводу. При чтении старого фрагмента incoming lines не двигают viewport. Terminal output не должен стилизоваться как всеобщая оболочка Task. Read-only command output и интерактивный terminal выглядят различимо; writable terminal требует собственного IPC/authority contract.

Artifacts могут содержать недоверенный HTML/текст. Preview не должен получать полномочия host UI; внешние ссылки и экспорт проходят предусмотренную desktop boundary. Это прямое следствие ADR-0007, а не выбор конкретной rendering library.

### 14.6 Performance foundations без преждевременного framework choice

Проверять задержку возврата к Task, открытия palette, перемещения по списку, обновления status и раскрытия большого diff. Скорость анимации не компенсирует блокирующее чтение данных. DOM/native view count, syntax highlighting и объём IPC payload должны быть ограничены.

Предложение fixtures: 10 000 Tasks, 100 000 log lines, branch у текущего G7 limit, большой diff порядка 50 MB, несколько окон с одним и разными Task. Это искусственные stress cases, не утверждение о типичной нагрузке. Их нужно дополнить реалистичными small/medium datasets и целевым Mac.

Предварительные UX budgets: cached palette response p95 около 100 ms, cached Task navigation около 200 ms; cold loads имеют видимую обратную связь и измеряются отдельно. Значения — кандидаты для согласования. Не объявлять «60 FPS» или низкое потребление памяти доказанными до benchmark.

Требования к данным: pagination, bounded snapshots, отмена ненужного read, приоритет текущего контекста, отсутствие частого polling скрытых panels. Virtualization сохраняет keyboard/screen-reader semantics. Решение о transport streaming и caching принимается отдельно с учётом текущего последовательного IPC. Из этих требований ещё не следует React, Slint или GPUI.

## 15. Accessibility and keyboard model

Цель — WCAG 2.2 AA как применимый baseline для интерфейсных поверхностей плюс проверка desktop assistive technologies. Для native частей нужно оценивать применимость требований, а не автоматически заявлять web conformance.

Текст обычного размера — contrast ≥4.5:1, крупный — ≥3:1; необходимая нетекстовая графика/границы controls — ≥3:1. Цвет не единственный носитель смысла. Text scaling до 200% сохраняет функции; reflow проверяется и при большем zoom для форм, lists и decisions. Пространственный code/diff может требовать горизонтального просмотра, но это не оправдывает нереспонсивную approval form. [WCAG 2.2](https://www.w3.org/TR/WCAG22/).

WCAG 2.2 AA включает focus not obscured minimum, альтернативу dragging и target size minimum 24×24 CSS px с предусмотренными исключениями. Runlith предпочтительно использует более крупные 32/40 px controls там, где позволяет плотность. 2 px focus ring — собственный дизайн-кандидат; не следует ошибочно называть весь Focus Appearance criterion требованием AA: он AAA. [Что добавлено в WCAG 2.2](https://www.w3.org/WAI/standards-guidelines/wcag/new-in-22/).

### 15.1 Keyboard contract — кандидат для проверки

| Действие | Кандидат binding | Правило |
|---|---|---|
| Global object search | Cmd/Ctrl+K | Всегда виден scope результата |
| Command palette | Cmd/Ctrl+Shift+P | Тот же action model, что в меню |
| Поиск в текущем объекте | Cmd/Ctrl+F | Не внезапно global search |
| Перейти между областями | F6 / Shift+F6 | Понятный цикл и объявляемый region |
| MRU object tabs | Ctrl+Tab / Ctrl+Shift+Tab | Сохраняет контекст каждого объекта |
| Tree navigation | Arrows, Home/End, type-ahead | Focus отдельно от selection/activation |
| Открыть выбранный объект | Enter | Не исполняет опасную mutation из общего списка |
| Закрыть transient surface | Esc | Не отменяет Task, Execution или отправленный запрос |
| Закрыть view/tab | Cmd/Ctrl+W | Работа daemon продолжает жить по контракту |
| Preview row | Space, только в совместимом list context | Видимая кнопка Preview; не конфликтует с tree multiselect |
| Context actions | Menu / Shift+F10 и доступное меню | Полный набор доступен без pointer |
| Resize pane | Focus divider + arrows; команды размера/сброса | Drag не единственный способ |

Bindings должны быть изменяемыми и проверенными на macOS/Windows/Linux conflicts, раскладках, IME и assistive shortcuts. macOS использует Cmd для application commands; Ctrl в terminal сохраняет terminal semantics. Несанкционированный Cmd/Ctrl+Enter не должен становиться approval shortcut.

Tree следует WAI-ARIA pattern: focus и selection имеют разные роли. Splitter должен быть доступным separator с именем, orientation и размером; APG Window Splitter содержит незавершённые детали примера, поэтому его нельзя считать готовым tested component. [Treeview](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/), [Window Splitter](https://www.w3.org/WAI/ARIA/apg/patterns/windowsplitter/).

### 15.2 Screen reader и динамические данные

Regions имеют осмысленные названия; status update не читает каждый token/log line. Завершение действия и возникновение обязательного запроса объявляются один раз, с возможностью открыть подробности. Continuous progress — throttled polite updates или по запросу; urgent announcement не дублируется одновременно в трёх местах.

Нужны доступная текстовая форма diff, последовательный просмотр logs, текстовая альтернатива graph, сохранение focus после обновления списка и возврат к инициатору после закрытия dialog. Пользователь может остановить автоследование за выводом. Практический reference — Accessible View и Accessible Diff Viewer VS Code; это не основание считать сторонний editor component автоматически доступным. [VS Code accessibility](https://code.visualstudio.com/docs/configure/accessibility/accessibility).

### 15.3 Desktop verification

Проверить VoiceOver на macOS, затем NVDA на Windows для переносимого пути; keyboard-only, high contrast, reduced motion, увеличение текста, многооконность, длинные пути и кириллический ввод. Не использовать hover как единственный доступ к действию. Для shortcut-only функций требуется menu/button equivalent.

Ручные сценарии обязательны: открыть Task из Attention; прочитать scope; найти evidence; принять/отклонить доступное решение; получить stale response; вернуться в исходный контекст. Автоматические accessibility checks дополняют эту проверку, но не заменяют её.

## 16. Recommendations

### High confidence

| Рекомендация | Основание | Что нужно зафиксировать |
|---|---|---|
| Task-first IA и видимая иерархия Execution/Attempt/Session | Прямые R contracts | Object naming, routes, ownership и provenance |
| Separate authoritative state / narration / evidence | R verification; Temporal pattern | Правила источников, freshness и displayed claims |
| Раздельные HDR и Approval при общей Attention | Разные R lifecycles и authority | Типизированные items, exact-action review, resolved/consumed |
| Явные Plan и Run | R ADR-0028 | Side effects каждой команды и запрет hidden planning |
| Точные recovery/cancel labels | R execution wire | Action eligibility и объяснение неподдерживаемого случая |
| Evidence linked to subject/snapshot | R independent evaluation | Query schema, stale behavior и source navigation |
| Keyboard/a11y и version compatibility с начала | WCAG + desktop constraints + R IPC | Проверяемые acceptance scenarios |
| UI read contracts до полноценной shell | Выявленные gaps в proto | Source of truth каждого поля и bounds каждого read |

Уверенность относится к необходимости требований. Она не означает, что их конкретный визуальный дизайн уже найден.

### Medium confidence

Адаптивный inspector как default; пять предложенных Task tabs; navigation из Tasks/Attention; criterion-oriented Evidence view; компактная сводка significant changes; предложенная density scale. Все хорошо согласуются с продуктом и исследованными patterns, но требуют наблюдения реальных пользователей и данных.

Тёмная restrained эстетика — уверенное исходное предпочтение пользователя. Конкретные hex/radii/metrics — средней или низкой уверенности до визуальных и accessibility checks. Отсутствует основание менять blue/green/teal semantics.

### Experiments required

| Вопрос | Эксперимент | Критерий решения |
|---|---|---|
| C или постоянный A? | Контрбалансированный A/B/C workflow test | Ошибки scope, orientation time, review efficiency и предпочтение по сценарию |
| Чат task-level или execution-level? | Prototype с несколькими Attempts/providers | Пользователь всегда понимает получателя и сохранённый контекст |
| Plan list или graph по умолчанию? | Планы разной глубины/параллельности | Правильное чтение dependency/blocking scope |
| Какие notifications нужны? | Неделя diary/field study с replayable fixtures | Пропущенные обязательные решения против interruption load |
| Нужен ли встроенный writable terminal? | Jobs-to-be-done интервью и task observation | Частота сценариев, которые не покрывает read-only output / open-in-editor |
| Нужен ли другой frontend baseline? | Сравнительный benchmark после первого серьёзного UI, как предусмотрено roadmap | Accessibility, desktop behavior, performance и стоимость поддержки |

## 17. Open product questions

1. Кто первичный пользователь: индивидуальный разработчик, reviewer нескольких autonomous tasks или команда с разделённой authority? Влияет на Attention, ownership и multi-window behavior.
2. Какой минимальный реальный цикл обязан поддержать первый desktop release: создание → planning → execution → review, или сначала supervision уже созданных Task?
3. Какие части G8 evidence доступны в первом UI milestone и как показывать ещё не материализованные obligations без ложных success states?
4. Нужен ли изменяемый DRAFT после create_planned? Текущий контракт намеренно не решает это.
5. Существует ли продуктовая conversation identity поверх Attempts, кто владеет continuity и как она относится к G9?
6. Когда пользователь должен вручную менять Plan: через новое proposal, structural editing или только intent feedback planner? Редактирование accepted revision исключено.
7. Как формируется набор action eligibility и причин refusal для UI, в том числе при stale client data?
8. Каков допустимый объём concurrent Tasks и Plan branches у целевого пользователя? Без этого нельзя обосновать первый global execution overview.
9. Что считать достаточно важным изменением для changed-since-last-visit summary? Какие данные должны храниться per-user/per-device?
10. Как разделяются read/snooze предпочтения между несколькими окнами и будущими пользователями? Они не должны менять domain state.
11. Какие execution recovery paths реально поддерживаются к первому release, помимо pre-attempt Resume?
12. Нужно ли просматривать исходники в Runlith, редактировать их или надёжно открывать в выбранной IDE? Это существенно меняет shell и trust requirements.
13. Какие platforms и assistive technologies входят в release acceptance, а какие — в последующие milestones?
14. Какой целевой Mac и dataset задают performance budgets? Только после этого можно сравнивать stacks.
15. Какой именно Warp screenshot был выбран пользователем и какие детали в нём важны? Нужен для точного visual follow-up, но не блокирует IA contract.

Эти вопросы не требуют возвращаться к общей подборке рынка. Они уточняют продуктовые решения, которые secondary research не может принять за пользователя.

## 18. Proposed next design tasks

Ниже — рабочие обозначения D01–D12 для обсуждения. Это **не TASK-XXXX**, не изменения queue и не назначение TASK-0085.

| Порядок | Предлагаемая задача | Выход и acceptance | Зависимость |
|---|---|---|---|
| D01 | Подтвердить primary workflows и release boundary | 6–8 конкретных сценариев; personas без выдуманных частот; current/future capability map; при наличии исходный Warp screenshot | Это исследование |
| D02 | UI Product / Information Architecture contract | Object dictionary, routes, navigation, default Task summary, source/freshness каждого поля, projection vs domain distinction; проверен против invariants | D01 |
| D03 | UI query и command requirements | Read-model inventory, pagination/bounds, action eligibility, compatibility, errors и reconnect; каждый control связан с backend operation | D02 |
| D04 | Attention / HDR / Approval interaction contract | Fixtures для pending/resolved/consumed/superseded/expired/stale, hard DENY и multi-window race; точные последствия choices | D02–D03 |
| D05 | Сравнимые A/B/C Task Workspace prototypes | Одинаковые данные и styling; desktop 1440×1000, laptop 1280×800, увеличенный текст; no hidden unsupported actions | D02, D04 |
| D06 | Formative usability research | Наблюдения 8–12 участников, ошибки/время/нагрузка, отрицательные результаты; решение о default layout с границами применимости | D05 |
| D07 | Semantic visual и accessibility contract | Tokens по ролям, state naming, density, keyboard/focus, notification behavior; contrast и ручной сценарный review | D04–D06 |
| D08 | Frontend stack ADR / reaffirmation | Зафиксировать требования и текущий Tauri/React baseline с ADR-0015; OS/a11y, IPC boundary и критерии будущего benchmark; сравнительный implementation spike остаётся отложенным по roadmap | D03, D06–D07 |
| D09 | Desktop shell product specification | Window/tab lifecycle, project navigation, compatibility, reconnect, command palette, persisted presentation preferences | D08 |
| D10 | Task Workspace interaction specification | Authoring/Overview/Plan/Changes/Evidence/History, empty/loading/error fixtures, event-to-view mappings | D03, D06–D09 |
| D11 | Execution / Attempt / Evidence diagnostic specification | Identity hierarchy, failed/retry/recovery scenarios, base revision и evidence links, limitations по gates | D03–D04, D10 |
| D12 | Visual + accessibility verification plan | Матрица screens×states×viewport×input mode; VoiceOver/keyboard workflows; performance datasets и pass/fail evidence | D07, D09–D11 |

D04 необходим рано: граница Human Decision/Approval влияет на IA и layout. Финальная детализация этих surfaces может идти после Task Workspace specification, но их контракты нельзя откладывать до конца frontend реализации.

После принятия этих материалов можно создавать ограниченные implementation tasks: сначала необходимые IPC read surfaces и shell foundation, затем Task Workspace, Decisions/Approvals, Execution/Attempt/Evidence, затем обязательная visual/a11y verification. Это не поручение начать код сейчас. Готовность каждого UI slice определяется существующим backend contract; G8/G9/G11 не реализуются неявно через frontend.

Предлагаемая последовательность решений:

    Research
      → UI Product / IA contract
      → Query + action + attention contracts
      → Comparable prototypes + formative validation
      → Frontend stack ADR
      → Desktop shell specification
      → Task Workspace specification
      → Human Decision / Approval detailed surfaces
      → Execution / Attempt / Evidence detailed views
      → Visual + accessibility acceptance
      → Bounded implementation tasks

Параллельно можно уточнять semantic tokens и keyboard behavior, не фиксируя implementation stack.

## 19. Source appendix

### 19.1 Evidence provenance и даты

Основной срез исследования — 16.09.2026; оформление завершено 17.09.2026 по локальному времени Europe/Berlin. Веб-источники ниже открывались/проверялись в ходе этого среза. «Текущие docs» означает состояние доступной страницы в момент проверки, не independently tested latest binary. Дата crawl поисковой системы не использована как дата release.

Источники, на которых основаны утверждения, приведены непосредственно рядом с ними. Реестр ниже позволяет повторить выборочную проверку без повторения всего market research.

### 19.2 Repository evidence

| Источник | Что прочитано и зачем |
|---|---|
| [AGENT.md](../../../AGENT.md) | Canonical authority, scope и engineering constraints |
| [Architecture INDEX](../../architecture/INDEX.md) | Навигация по subsystem contracts |
| [INVARIANTS](../../architecture/INVARIANTS.md) | Идентичность, authority, verification, planning и UI boundary |
| [Architecture source of truth](../../../AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md) | Цель продукта; stack/thin UI; Task; adaptive planning; UI/design-system направления; релевантные разделы, не построчное чтение всего документа |
| [Roadmap](../../../planning/roadmap.yaml), [Queue](../../../planning/tasks/queue.yaml) | G7/TASK-0084, будущие gates, UI milestone; queue governance отдельно от runtime |
| [ADR-0005](../../architecture/adr/ADR-0005-task-execution-attempt-session.md) | Task → Execution → Attempt → Session |
| [ADR-0007](../../architecture/adr/ADR-0007-no-direct-frontend-shell.md) | Frontend boundary и недоверенные previews |
| [ADR-0015](../../architecture/adr/ADR-0015-shadcn-primitives-only.md) | Primitives-only, отсутствие требования копировать чужой dashboard |
| [ADR-0016](../../architecture/adr/ADR-0016-agentctl-before-complex-ui.md) | CLI-first и materialization перед UI |
| [ADR-0018](../../architecture/adr/ADR-0018-independent-evaluation.md) | Независимая evaluation |
| [ADR-0025](../../architecture/adr/ADR-0025-g5-post-admission-recovery.md) | Узкие recovery guarantees |
| [ADR-0027](../../architecture/adr/ADR-0027-g7-planning-orchestration-contract.md) | Plan proposal/validation/revision и vocabulary scope |
| [ADR-0028](../../architecture/adr/ADR-0028-g7-planning-input-run-and-execution-fencing.md) | Input/Run/accepted Plan; freshness; TASK-0084 amendment от 16.09.2026 |
| [IPC](../../architecture/ipc.md), [control proto](../../../crates/ipc-contracts/proto/agentd_control.proto) | Документированный транспорт и актуальные v0–v6 envelopes |
| [Task proto](../../../crates/ipc-contracts/proto/task_core.proto), [authoring proto](../../../crates/ipc-contracts/proto/task_authoring_core.proto) | Реальные операции и read/write asymmetry |
| [Execution proto](../../../crates/ipc-contracts/proto/execution_core.proto) | Attempts, provider_ref, live flag, cancel/resume guarantees |
| [Planning proto](../../../crates/ipc-contracts/proto/planning_core.proto), [HDR proto](../../../crates/ipc-contracts/proto/human_decision_core.proto) | Текущие operations и доступные DTO |
| [Policy proto](../../../crates/ipc-contracts/proto/policy_core.proto), [G6 enforcement](../../development/G6_APPROVAL_HUMAN_DECISION_ENFORCEMENT.md) | Approval lifecycle и authority; исторический gate text не принят за текущий статус |
| [Task Engine](../../architecture/task-engine.md), [Providers](../../architecture/providers.md), [Workspace](../../architecture/workspace.md) | Subsystem semantics |
| [HDR code](../../../crates/plan/src/human_decision.rs), [Planning storage](../../../adapters/storage-sqlite/src/planning_control_store.rs), [Verdict code](../../../crates/evaluation/src/verdict.rs) | Проверка конкретных enums и lifecycle относительно prose |

Репозиторий уже содержал изменённые planning/roadmap.yaml, planning/tasks/queue.yaml и untracked PNG с концептами Runlith. Они не были созданы исследованием. PRODUCT.md и DESIGN.md, упоминаемые в части документов, не обнаружены как доступные отдельные источники. Старый PNG рассмотрен как гипотеза, не как реализованный интерфейс.

### 19.3 Product sources

| Источник | Дата/версия | Вид и роль |
|---|---|---|
| [Cursor 3.0](https://cursor.com/changelog/3-0) | 02.04.2026 | D/M; Agents Window |
| [Cursor 3 article](https://cursor.com/blog/cursor-3) | 02.04.2026 | D/M; связь agent workspace и IDE |
| [Cursor 3.1](https://cursor.com/changelog/3-1) | 13.04.2026 | D; tiled layout |
| [Cursor changelog](https://cursor.com/changelog) | Projects entry 10.09.2026 | D/M; актуальность рынка, не доказательство зрелости |
| [Cursor Agents Window](https://cursor.com/docs/agent/agents-window) | Текущие docs | D; navigation/files/review |
| [Cursor Plan mode](https://prod.cursor.com/help/ai-features/plan-mode) | Текущая help page | D; explicit plan/build |
| [Cursor Agent Review](https://prod.cursor.com/docs/agent/agent-review) | Текущие docs | D; review как отдельный workflow |
| [Cursor official image](https://cursor.com/docs-static/images/agent/file-agents-window-final.png) | Дата asset не установлена; получен 16.09 | O; реальная визуальная инспекция, light theme |
| [Warp overview](https://docs.warp.dev/) | Docs updated 14.09.2026 | D/M; актуальные modes и продуктовый контекст |
| [Warp universal agents](https://www.warp.dev/blog/universal-agent-support-level-up-coding-agent-warp) | 14.04.2026 | D/M; tabs, external agents, notifications/review |
| [Warp Planning](https://docs.warp.dev/agents/capabilities/planning/) | Текущие docs | D/O; plan editor и версия документа |
| [Warp plan editor image](https://docs.warp.dev/_astro/planning-main-view.B3ScH40z_Z30qck.webp) | Asset date не установлена | O; просмотрено локально |
| [Warp plan progress image](https://docs.warp.dev/_astro/agent-plans-tasks.D4jtQV3D_ZoE1B2.webp) | Asset date не установлена | O; просмотрено локально |
| [Warp Notifications](https://docs.warp.dev/agents/capabilities/agent-notifications/) | Текущие docs | D; attention channels |
| [Warp Permissions](https://docs.warp.dev/agents/capabilities/agent-profiles-permissions/) | Updated 14.09.2026 | D; ограничения переноса permission semantics |
| [Warp Questions](https://docs.warp.dev/agents/local-agents/interacting-with-agents/agent-questions/) | Текущие docs | D; structured questions, не Runlith HDR |
| [Warp Code review](https://docs.warp.dev/code/code-review/) | Текущие docs | D; contextual diffs |
| [Warp Tabs](https://docs.warp.dev/terminal/windows/tabs/) | Текущие docs | D; tabs/groups/pinning/keyboard |
| [VS Code Agents window](https://code.visualstudio.com/docs/agents/run/agents-window) | Preview на дату проверки | D; session-scoped work context |
| [VS Code Agents overview](https://code.visualstudio.com/docs/agents/overview) | Текущие docs | D; границы agent execution/permissions |
| [VS Code UX Guidelines](https://code.visualstudio.com/api/ux-guidelines/overview) | Текущие docs | D; роли workbench containers |
| [Zed Agent panel](https://zed.dev/docs/ai/agent-panel) | Текущие docs | D; threads, worktrees, capability differences |
| [JetBrains Tool windows](https://www.jetbrains.com/help/idea/tool-windows.html) | IDEA 2026.2; 17.08.2026 | D; panels/focus/multiple monitors |
| [JetBrains agents](https://www.jetbrains.com/help/ai-assistant/activate-agents.html) | Текущие docs | D; контекст современных integrations |
| [Windsurf Cascade redirect](https://docs.windsurf.com/windsurf/cascade/cascade) → [Devin Desktop Cascade](https://docs.devin.ai/desktop/cascade/cascade) | Redirect наблюдался 16.09.2026 | D; не дата полного brand rollout |
| [Devin Desktop Spaces](https://docs.devin.ai/desktop/spaces) | Текущие docs | D; grouping/context |
| [Devin Agent Command Center](https://docs.devin.ai/desktop/agent-command-center) | Текущие docs | D; status-oriented sessions |
| [Devin release notes 2025](https://docs.devin.ai/release-notes/2025) | Исторический материал 2025 | D/M; timer-based planner как ограниченный исторический пример |
| [Codex app introduction](https://openai.com/index/introducing-the-codex-app/) | 02.02.2026; update 04.03.2026 | D/M; датированный launch reference |
| [Old Codex features address](https://developers.openai.com/codex/app/features) → [current destination](https://learn.chatgpt.com/docs/features) | Redirect на дату проверки | Ограничение актуальности старой документации |
| [Claude Code Desktop](https://code.claude.com/docs/en/desktop) | Текущие docs; функции могут зависеть от версии | D; panes и density/detail modes |
| [Linear Inbox](https://linear.app/docs/inbox) | Текущие docs | D; внимание отдельно от issue |
| [Linear Peek](https://linear.app/docs/peek) | Текущие docs | D; preview с сохранением списка |
| [Linear Search](https://linear.app/docs/search) | Текущие docs | D; local/global retrieval |
| [Raycast Action panel](https://manual.raycast.com/action-panel) | Текущее руководство | D; selected object actions |
| [Raycast Keyboard shortcuts](https://manual.raycast.com/keyboard-shortcuts) | Текущее руководство | D; discoverability и platform modifiers |
| [Temporal Web UI](https://docs.temporal.io/web-ui) | Текущие docs | D; nested failures/history |
| [Dagster Webserver UI](https://docs.dagster.io/guides/operate/webserver) | Текущие docs | D/O; run details, structured events/logs |
| [GitHub Actions graph](https://docs.github.com/en/actions/how-tos/monitor-workflows/use-the-visualization-graph) | Текущие docs | D; dependency visualization reference |
| [GitHub Deployment review](https://docs.github.com/en/actions/how-tos/deploy/configure-and-manage-deployments/review-deployments) | Текущие docs | D; action-specific review |
| [Grafana Trace integration](https://grafana.com/docs/grafana/latest/visualizations/explore/trace-integration/) | Текущие docs | D; correlated evidence navigation |

### 19.4 Community и методическая база

| Источник | Дата | Как использован |
|---|---|---|
| [Cursor layout discussion](https://forum.cursor.com/t/agents-window-customize-layout/158646) | 21–29.04.2026 | C; потребность в управлении spatial layout |
| [Cursor Agents Window feedback](https://forum.cursor.com/t/agents-window-is-not-good/157226) | Апрель–июнь 2026 | C; историческая цена workflow migration; не текущий defect list |
| [Warp modes discussion](https://www.reddit.com/r/warpdotdev/comments/1rgiesj/some_new_settings_restore_old_agent_mode/) | 27.02.2026 | C; mode confusion и явный input context |
| [Дополнительное положительное Warp discussion](https://www.reddit.com/r/warpdotdev/comments/1snko27/warp_is_actually_surprisingly_kind_of_good_to_use/) | 2026; точная дата не использована | C; проверка, что отзывы не односторонние; не основание ключевых выводов |
| [WCAG 2.2](https://www.w3.org/TR/WCAG22/) | Действующая recommendation на дату проверки | Нормативная основа доступности |
| [What's new in WCAG 2.2](https://www.w3.org/WAI/standards-guidelines/wcag/new-in-22/) | Исходная publication 05.10.2023 | Уточнение AA/AAA, targets, focus и dragging |
| [APG Treeview](https://www.w3.org/WAI/ARIA/apg/patterns/treeview/) | Текущая страница | Keyboard/focus/selection |
| [APG Window Splitter](https://www.w3.org/WAI/ARIA/apg/patterns/windowsplitter/) | Текущая страница, work-in-progress caveat | Keyboard resize; не готовая реализация |
| [VS Code accessibility](https://code.visualstudio.com/docs/configure/accessibility/accessibility) | Страница 16.09.2026 | Практический reference accessible output/diff |
| [Apple Managing notifications](https://developer.apple.com/design/human-interface-guidelines/managing-notifications) | Текущая HIG page | Пропорциональность urgency и уважение OS preferences |
| [Apple Inspectors in SwiftUI](https://developer.apple.com/videos/play/wwdc2023/10161/) | WWDC23 | Primary content и supplemental inspector; не совет использовать SwiftUI |
| [Microsoft Guidelines for Human-AI Interaction](https://www.microsoft.com/en-us/research/publication/guidelines-for-human-ai-interaction/) | CHI, май 2019 | Общая human-AI interaction база; не доказательство конкретной Runlith компоновки |

Community evidence — несколько публичных обсуждений без контроля выборки. Из него нельзя вывести долю недовольных, удержание, скорость работы или превосходство одного продукта. Противоречия сохранены: одним пользователям полезен agent-first workspace, другим важнее стабильный editor layout и явные input modes.

### 19.5 Что проверено и что остаётся неизвестным

Проверены: соответствие предложений прочитанным canonical contracts и текущим proto; наличие всех требуемых доменных объектов в IA mapping; различение lifecycle и presentation state; основные временные оговорки рынка; локальные пути источников; арифметика предложенных contrast pairs; отсутствие внесённых исследованием изменений в repository.

Не проверены: реальное поведение установленных версий конкурентов, их performance/accessibility, usability гипотез Runlith, исходный пользовательский Warp screenshot, readiness ещё не реализованных backend query surfaces и будущих gates. Эти ограничения учитываются в confidence и последующих design tasks.
