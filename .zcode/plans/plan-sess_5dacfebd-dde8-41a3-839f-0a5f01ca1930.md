## TASK-0087-R1: contract alignment and independent verification

### Исходное состояние

Зафиксирован main `6a02eaa4f696e673613f7ebd0511c2df7613b5db`. TASK-0087 уже смержен, но queue сохраняет `completed_through: TASK-0085` и `TASK-0087-R1: VERIFICATION_REQUIRED`. Версия — `0.1.0-dev.task0087`, G7 — IN_PROGRESS. Контрактные дополнения TASK-0086 (`bc4168c`) отсутствуют в ADR на main. Историческую ветку нельзя просто слить: это также внесёт устаревшее bookkeeping и не решит вопрос READY lifecycle.

**Проверяемый вопрос:** соответствует ли merged TASK-0087 предложенному контракту TASK-0086 и действующим accepted invariants/ADRs, включая READY-child, exact-retry lifecycle evidence, atomicity и L4?

### 1. Изолировать работу

Создать отдельные branch/worktree `task-0087-r1-contract-verification` / `/Users/artemmac/dev/runlith-wt-0087-r1` от указанного main после проверки отсутствия коллизии и изменения базы. Не трогать main, пользовательские untracked-файлы и чужие worktrees. Новое существенное изменение main требует пересмотра базы, а не автоматического merge/rebase.

### 2. Выполнить одну адресную сверку контракта и кода

Сопоставить изменения реализации `4cd3c77..6a02eaa` с child-materialization sections из TASK-0086:

- ADR-0027, branch lines 403–495;
- ADR-0028, branch lines 210–257.

Проверять фактический код `planning_service.rs`, `plan_graph.rs`, `plan_store.rs` и связанные journal/fence helpers. Licensing diff исключить из технической приёмки.

Составить таблицу **требование → source → test → результат/разрыв** по шести пунктам:

1. Child identity, непосредственный parent и наследование project без захвата чужого Task.
2. Сужение scope, сохранение criteria/risk/policy; planner остаётся proposal-only.
3. Atomic child + CREATED + MARKED_READY + Plan revision/journal; полный rollback при поздней ошибке.
4. Exact retry: snapshot, provenance, causation и обе lifecycle facts совпадают; конфликт или повреждение не создают частичное состояние.
5. Publication fencing против изменения parent/candidate authority и конкурентного занятия child ID.
6. L4 считает только исполнимые Task-backed leaves с завершёнными predecessors; parent не завершается автоматически.

**Архитектурный stop condition:** если READY-at-publication, adoption/retry semantics или иной пункт требует нового решения, которого нет в accepted authority, записать точный разрыв и предложенный ADR amendment. Не объявлять исторические дополнения Accepted автоматически, не подгонять контракт под код и не принимать R1 до решения владельца. Отсутствие интегрированного TASK-0086 само по себе не доказывает дефект реализации.

### 3. Проверить существующее покрытие и закрыть только доказанные пробелы

В `plan_persistence.rs` уже есть девять malformed-evidence случаев. Проверить наличие покрытия для CREATED/READY отдельно: отсутствие, дублирование, неправильные type/states, task/project, provenance/causation, malformed envelope. Добавлять только отсутствующие случаи, следующие из текущего контракта; сохранять assertions об отсутствии частичных Task/Plan/lineage/journal writes.

Проверить L4 для DRAFT child, READY child, незавершённого и завершённого predecessor. Новые тесты и любые изменения acceptance-тестов явно перечислить в результате.

Если тест обнаружит продуктовый дефект, зафиксировать RED и точное место. Не расширять работу на соседние задачи или незаданные изменения production-семантики: remediation выделить в bounded envelope после определения причины.

### 4. Выполнить verification последовательно

Сначала перестроить shipped binaries, чтобы исключить stale-binary evidence:

```bash
cargo build -p agentd -p agentctl
cargo test -p storage-sqlite --test plan_persistence
cargo test -p app-services --lib plan_graph::tests
cargo test -p app-services --test planning_service
cargo test -p app-services --test planning_failure_matrix
cargo test -p app-services --test planning_task_authority
cargo test -p plan-domain --test validation_rules
cargo test -p fake-planner --test planner_port_contract
cargo test -p provider-claude --test planner_port_contract
cargo test -p agentd --test planning_integration
cargo test -p agentd --test execution_recovery --test live_ownership
cargo test -p agentctl
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo fmt --check
```

Не запускать конкурирующие process/socket suites параллельно. Не менять глобальное окружение по исторической подсказке: обход через `DEVELOPER_DIR` допустим только при подтверждённой проблеме toolchain и локально для команды. Fixture green не выдавать за authenticated real-provider smoke. Любые failed/ignored/skipped проверки перечислить; flake не считать устранённым лишь по успешному повтору.

### 5. Независимая приёмка и минимальное bookkeeping

Независимый read-only reviewer проверяет реальный итоговый diff и exact verification tree, целостность тестов, authority и durable-state boundaries. Отчёт не заменяет source/test evidence.

Создать `docs/development/G7_TASK_0087_R1_VERIFICATION.md` с базой, diff identity, contract matrix, командами, результатами и verdict. Допустимые исходы: READY_FOR_ACCEPTANCE либо BLOCKED/REMEDIATION_REQUIRED с конкретными причинами; никакого G7_CLOSED.

В `planning/tasks/queue.yaml` исправить доказуемо устаревшее «implementation branch» на факт интеграции и явно отразить незакрытые contract/verification prerequisites. Не продвигать `completed_through` до TASK-0087 до принятия контрактов и независимой GREEN-проверки. При полном выполнении условий подготовить согласованную запись TASK-0086/TASK-0087/R1 без выдуманной истории прежней приёмки. Версию TASK-0087 сохранить; public SemVer не менять.

Разрешённый write scope: названный evidence report, queue, адресные regression tests; ADR amendments — только как явно предложенные изменения, не самовольное архитектурное принятие. Production fixes, licensing, G8/G9, новый provider и unrelated security backlog вне этого плана.

Завершить `git diff --check`, final diff review и machine-readable task result со статусом `submitted` при готовности к приёмке. **Commit, push и merge не выполнять.** Интеграция и post-merge verification требуют отдельной merge-authority.

### Результат

Одна доказательная contract/test matrix и однозначный verdict по TASK-0087-R1: либо конкретный архитектурный/продуктовый blocker, либо проверенная готовность к приёмке и минимальный reconciliation diff. Общую rehydration, повторные ancestry-проверки и исследование посторонних веток не продолжать.