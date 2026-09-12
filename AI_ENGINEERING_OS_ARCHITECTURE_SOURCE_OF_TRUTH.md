# AI Engineering OS — Architecture Source of Truth

> **Status:** Architecture baseline / G0 draft  
> **Date:** 2026-09-10  
> **Purpose:** единый источник истины для разработки продукта — local-first AI Engineering OS / control plane для команд AI-агентов.  
> **Audience:** человек-разработчик, Codex, Claude Code, OpenCode и любые будущие coding-agents.  
> **Working product name:** `AI Engineering OS` до выбора финального бренда.

---

## 0. Что за продукт мы строим

Мы **не строим очередной GUI для нескольких терминалов** и не копируем BridgeMind один в один.

Продукт должен быть локальным control plane для AI-команды, где Claude Code, Codex, OpenCode, Gemini/Antigravity и будущие агенты — **сменные исполнители**, а настоящая ценность находится в:

- durable tasks и dependency graph;
- adaptive hierarchical planning с контролируемым runtime re-planning;
- orchestration и recovery;
- context management;
- long-term memory;
- verification-first delivery;
- policy/security;
- provider-independent execution;
- observability и replay;
- adaptive model/context routing;
- privacy by default.

Ключевой UX-принцип:

> Пользователь управляет **результатом и работой**, а не окнами терминалов и отдельными chat-сессиями.

Пример:

```text
User intent
   ↓
Plan
   ↓
Durable Task Graph
   ↓
Scheduler / Reconciler
   ↓
Claude / Codex / OpenCode
   ↓
Verification
   ↓
Human gate
   ↓
Ship
```


## 0.1 Product differentiation and leverage principles

AI Engineering OS must have a crisp answer to **why it exists as a product** and why a user should choose it instead of using Claude Code, Codex, OpenCode, Gemini or another single-provider coding harness directly.

The target distinction is structural, not marketing-only:

```text
Provider harnesses (Claude Code / Codex / OpenCode / etc.)
→ strong execution environments for one provider or one interaction model

AI Engineering OS
→ durable provider-neutral control plane over those executors
   with Task/Execution/Attempt semantics, policy, isolation, verification,
   continuity, cross-provider handoff, memory, orchestration, attention management,
   background routines and later team/enterprise governance.
```

A feature that can be obtained by simply opening another coding-agent terminal is not, by itself, sufficient product differentiation. The product must continuously increase the value of **coordination, reliability, continuity, verification, safety, and outcome management** around replaceable executors.

### 1% improvement principle

When we adopt an idea already demonstrated by a competitor or adjacent product, we should not target feature parity alone. The adopted capability must improve at least one meaningful dimension in a way that is explicit and, where practical, measurable, while not regressing critical dimensions. Relevant dimensions include:

- reliability;
- correctness / verification quality;
- provider neutrality;
- safety / isolation;
- recovery and continuity;
- latency;
- resource or token efficiency;
- usability / human attention cost;
- observability / auditability;
- configurability;
- privacy;
- team governance.

"1% better" is a **minimum improvement mindset**, not a license for arbitrary scope growth or cosmetic changes. The improvement must matter to the user or system outcome. If parity is necessary as groundwork, record the follow-up differentiation target explicitly.

### Pareto / high-leverage prioritization

Use the Pareto principle as a prioritization heuristic: a minority of capabilities often creates a majority of product value. We therefore deliberately concentrate disproportionate effort on the highest-leverage work — the approximate **20% of problems/capabilities that can drive roughly 80% of user value, reliability, differentiation or business value**.

This is not a rigid mathematical law and must never be used to skip mandatory correctness, security, data-integrity, migration, recovery or verification work. A small safety-critical task may have little visible UX value but still be non-negotiable.

Practical prioritization rule:

```text
1. Protect correctness / security / durable state first.
2. Identify the highest-leverage user and product bottlenecks.
3. Spend the majority of discretionary engineering effort there.
4. Prefer deep excellence in differentiating workflows over broad shallow parity.
5. Defer low-leverage polish and speculative feature breadth until the core advantage is clear.
```

The resulting product question should always be answerable in one sentence:

> **What important job can AI Engineering OS perform more reliably, continuously or efficiently than using individual coding-agent products separately?**

If the team cannot answer that clearly for a proposed feature set, the product strategy is drifting and should be re-evaluated before adding more surface area.

---

# 1. Финальные решения после проверки всей переписки

Ниже — решения, которые считаются актуальными. Более ранние противоречащие варианты считать отменёнными.

| Область | Финальное решение |
|---|---|
| Desktop shell | **Tauri 2** |
| Frontend | **React + TypeScript + Vite** |
| UI primitives | **shadcn/ui**, но только как primitives |
| Design system | **собственный**, поверх shadcn/ui |
| Local backend/runtime | **Rust `agentd`**, отдельный процесс |
| Tauri Rust host | тонкий native shell, без orchestration business logic |
| Local IPC | Unix Domain Socket (macOS/Linux) / Named Pipe (Windows), versioned Protobuf contracts |
| Local DB | **SQLite** |
| Local architecture | **modular monolith**, не микросервисы |
| Cloud backend | позже, **Go + PostgreSQL + object storage** |
| Cloud in v1 | **не нужен** |
| Local-first | **обязателен** |
| Account for core | **не обязателен** |
| Telemetry | **off by default / opt-in** |
| Task model | Task ≠ Execution ≠ Attempt ≠ Session |
| Agent completion | агент не может сам поставить Task = Done |
| Git isolation | worktree per execution/task attempt |
| Provider abstraction | provider/model не входят в domain model задачи |
| Context | disposable materialized view, не authoritative state |
| Provider compaction | optimization, не persistence mechanism |
| Memory | semantic + episodic + procedural + decisions, с provenance |
| Event history | v1: relational state + append-only Event Journal; стратегическая цель: эволюция к full Event Sourcing после стабилизации доменной модели |
| Tool contracts | JSON Schema / typed contracts |
| Local process contracts | Protobuf |
| AI-readable docs | Markdown + progressive disclosure |
| TOON/TRON | optional model-facing renderer, **никогда canonical format** |
| Security | Policy Engine + OS-level isolation; prompt-инструкции не являются sandbox |
| Secrets | OS Keychain/Credential Manager/Secret Service, не SQLite |
| Testing | FakeAgent и contract tests до реальных providers |
| First interface | `agentctl` CLI до сложного GUI |
| Development planning | gates + atomic tasks + ADR + invariants |
| Runtime planning | versioned durable DAG + bounded just-in-time decomposition; Planner proposes, Reconciler/Policy validates |

---

# 2. Исправленные противоречия

## 2.1 Go vs Rust для локального backend

Ранний вариант: local daemon на Go.  
Финальный вариант: **local daemon `agentd` на Rust**.

Причины:

- Tauri уже использует Rust;
- local runtime — системная программа, а не обычный CRUD backend;
- PTY/process/filesystem/IPC/sandbox integration естественно живут рядом с Rust;
- один native toolchain;
- удобнее делить crates между Tauri host, daemon и CLI;
- сильнее compile-time guarantees для security-critical orchestration.

Go сохраняется для будущего cloud/control server, где скорость серверной разработки и простота concurrency будут более полезны.

---

## 2.2 Event Sourcing

**Стратегическая цель:** в будущем система должна иметь возможность эволюционировать к полноценному Event Sourcing там, где это даст реальную ценность: воспроизводимость состояния, time-travel/debugging, durable replay, rebuilding projections, распределённые worker'ы и глубокий audit.

**Однако в v1 full Event Sourcing намеренно не реализуется.** На раннем этапе доменная модель, события и границы bounded contexts ещё будут уточняться; преждевременный Event Sourcing сильно увеличит стоимость миграций и сложность разработки.

Текущая архитектура v1:

```text
SQLite relational state
+
append-only Event Journal
+
separate Security Audit
```

Это рассматривается не как конечная точка, а как **migration-friendly stepping stone** к Event Sourcing. Все значимые доменные изменения должны уже сейчас иметь стабильные typed event records, correlation/causation IDs, schema versions и timestamps, чтобы позже можно было постепенно переводить выбранные bounded contexts на event-sourced persistence без переписывания всего приложения.

Переход должен быть поэтапным:

```text
v1
Relational State + Event Journal
        ↓
Stable domain events + projections
        ↓
Event-sourced selected bounded contexts
        ↓
Full Event Sourcing only where justified
```

Критерий перехода: Event Sourcing внедряется не «потому что архитектурно красиво», а когда конкретный bounded context выигрывает от replay, reconstruction, temporal queries, distributed processing или stronger audit guarantees.

Мы получаем replay, debug history и analytics без сложности полной event-sourced модели.

---

## 2.3 UI-first vs core-first

Финальное правило:

> Core должен быть полностью usable через `agentctl` до того, как UI станет сложным.

Tauri не определяет архитектуру backend.

---

## 2.4 Agent-centric vs task-centric

Ранние идеи вокруг persistent AI employees полезны как UX, но **центральная сущность системы — Task**, а не Agent и не Chat.

Agent — назначаемый исполнитель.  
Session — временный execution detail.

---

## 2.5 Memory vs Context

Memory и context — разные подсистемы.

- **Memory:** durable knowledge.
- **Context:** временный working set для конкретного model call/execution.
- **Provider native compaction:** локальная оптимизация provider runtime.

---

# 3. Главные архитектурные цели

Система должна позволять заменить любую крупную бизнес-подсистему без переписывания приложения.

Например:

- SQLite → другой local store;
- Claude → новый provider;
- BM25 retrieval → другой retrieval engine;
- TOON renderer → другой serializer;
- native runner → container runner;
- Tauri UI → другой client;
- текущий scheduler → более умный reconciler;
- memory implementation → новая memory strategy.

Если замена одного из этих компонентов требует менять Task Engine, UI, provider adapters и storage одновременно — архитектура считается нарушенной.

---

# 4. Архитектурные правила заменяемости

## 4.1 Ports & Adapters

Каждый bounded context имеет:

```text
Domain
Application Service
Inbound Ports
Outbound Ports
Adapters
```

Domain не знает:

- SQLite;
- Tauri;
- Claude;
- Codex;
- OpenAI;
- HTTP;
- filesystem paths конкретной OS;
- UI.

---

## 4.2 Интерфейс принадлежит потребителю

Пример: Orchestrator нуждается в Task data.

Плохо:

```text
orchestrator imports sqlite_task_repository
```

Правильно:

```rust
trait TaskReader {
    fn get_task(&self, id: TaskId) -> Result<TaskSnapshot>;
}
```

А адаптер связывает этот port с Task module.

---

## 4.3 Модуль не читает таблицы другого модуля

Даже при одной SQLite базе.

Плохо:

```sql
SELECT * FROM tasks
```

из `context` module.

Правильно:

```text
Context Engine → TaskSnapshotPort → Task Service
```

---

## 4.4 Никаких provider-specific типов в core

Плохо:

```rust
Task {
  claude_session_id: ...
}
```

Правильно:

```rust
Execution {
  provider_ref: ProviderRef,
  external_session_ref: Option<OpaqueRef>
}
```

---

## 4.5 Никакой бизнес-логики в Tauri commands

Tauri command только:

1. validates request;
2. calls local client;
3. returns DTO.

---

## 4.6 Cross-module communication

Допустимо:

- explicit application API;
- typed events;
- versioned contracts.

Недопустимо:

- shared mutable singleton;
- прямое чтение чужой DB schema;
- provider adapter, который меняет Task state;
- UI, напрямую управляющий Git/processes.

---

# 5. Общая системная структура

```text
┌──────────────────────────────────────────────────────────┐
│                      DESKTOP CLIENT                      │
│                                                          │
│ React + TypeScript + Vite + own Design System            │
│ shadcn/ui primitives                                     │
└──────────────────────────┬───────────────────────────────┘
                           │
                       Tauri IPC
                           │
┌──────────────────────────▼───────────────────────────────┐
│                   THIN TAURI HOST                        │
│                         Rust                             │
│                                                          │
│ Windows / updater / notifications / native dialogs       │
│ Starts or reconnects to agentd                           │
│ NO orchestration business logic                          │
└──────────────────────────┬───────────────────────────────┘
                           │
                  UDS / Named Pipe
                 versioned Protobuf
                           │
┌──────────────────────────▼───────────────────────────────┐
│                         agentd                           │
│                          Rust                            │
│                                                          │
│ Task Engine         Planner             Orchestrator     │
│ Scheduler           Reconciler          Runtime Control  │
│ Context Engine      Memory Engine       Policy Engine    │
│ Evaluation          Agent Runtime       Workspace/Git    │
│ Event Journal       Audit               Secrets Broker   │
│ Usage/Router        Routines            Tool Registry    │
└──────────────────────────┬───────────────────────────────┘
                           │
                 ports / adapters
                           │
        ┌──────────────────┼──────────────────────┐
        ▼                  ▼                      ▼
    Claude Code          Codex                 OpenCode
        │                  │                      │
        └──────────────────┼──────────────────────┘
                           │
                       Runners
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
           Native       Container     Remote
                                      (later)
```

---

# 6. Rust workspace / repository layout

```text
/
├── apps/
│   └── desktop/
│       ├── src/                     # React / TS
│       ├── src-tauri/               # thin Tauri host
│       └── tests/
│
├── bins/
│   ├── agentd/                      # local Rust daemon
│   └── agentctl/                    # CLI control/debug client
│
├── crates/
│   ├── task/
│   ├── orchestration/
│   ├── agent-runtime/
│   ├── context/
│   ├── continuity/
│   ├── memory/
│   ├── policy/
│   ├── evaluation/
│   ├── workspace/
│   ├── scheduler/
│   ├── tools/
│   ├── usage/
│   ├── events/
│   ├── audit/
│   ├── secrets/
│   ├── ipc-contracts/
│   └── app-services/
│
├── adapters/
│   ├── provider-claude/
│   ├── provider-codex/
│   ├── provider-opencode/
│   ├── storage-sqlite/
│   ├── runner-native/
│   ├── runner-container/
│   ├── git-cli/
│   └── keychain-native/
│
├── contracts/
│   ├── protobuf/
│   ├── json-schema/
│   └── events/
│
├── docs/
│   ├── architecture/
│   ├── adr/
│   ├── security/
│   ├── privacy/
│   └── development/
│
├── planning/
│   ├── roadmap.yaml
│   ├── epics/
│   └── tasks/
│
└── test/
    ├── fake-agent/
    ├── fixtures/
    ├── contract/
    ├── integration/
    ├── security/
    └── chaos/
```

---

# 7. Composition Root

Только `agentd` composition root знает конкретные implementations.

Пример:

```text
TaskService
   ↑
SQLiteTaskStore

ContextEngine
   ↑
FTS5Retriever

AgentRuntime
   ↑
ClaudeAdapter
CodexAdapter
OpenCodeAdapter

ExecutionService
   ↑
ContainerRunner
```

Ни один domain crate не создаёт adapter самостоятельно.

---

# 8. Tauri Desktop

## 8.1 Tauri отвечает только за native shell

Разрешено:

- lifecycle window;
- updater;
- native dialogs;
- notifications;
- OS integration;
- connection to `agentd`.

Запрещено:

- task transitions;
- agent scheduling;
- prompt construction;
- provider logic;
- Git merge logic;
- memory management;
- security policy decisions.

---

## 8.2 Frontend не получает shell capability

```text
React
  X
execute("git ...")
```

Только:

```text
React
  ↓
Tauri DTO
  ↓
agentd request
```

---

## 8.3 Browser pane — отдельная trust zone

Remote/browser WebView:

```text
Tauri IPC       DENY
filesystem      DENY
shell           DENY
agentd          DENY
secrets         DENY
```

Main app WebView и browser WebView никогда не делят одинаковый capability set.

---

# 9. IPC

## 9.1 Transport

macOS/Linux:

```text
Unix Domain Socket
```

Windows:

```text
Named Pipe
```

Не использовать localhost HTTP для core desktop IPC.

---

## 9.2 Contracts

Canonical IPC contracts:

```text
Protobuf
```

Почему:

- versioning;
- typed schema;
- Rust сейчас;
- Go cloud/remote tooling позже;
- transport-independent.

---

## 9.3 Handshake

IPC startup должен поддерживать:

1. protocol version negotiation;
2. daemon build version;
3. client identity;
4. capability negotiation;
5. reconnect.

Socket/pipe должен жить в user-private runtime directory.

Позже можно добавить per-installation keypair/challenge-response для усиления local-client authentication.

---

# 10. Storage

## 10.1 Local

```text
SQLite
```

Причины:

- single-user local-first;
- transactions;
- no server;
- portable;
- backups;
- mature Rust support.

---

## 10.2 Состояние + Event Journal

Транзакция:

```text
BEGIN
UPDATE task state
INSERT event journal record
COMMIT
```

**В v1 не делать full Event Sourcing.** Event Journal проектировать как совместимый фундамент для будущей поэтапной миграции: typed/versioned domain events, correlation/causation metadata и независимые projections должны появляться по мере стабилизации bounded contexts.

---

## 10.3 Основные logical entities

```text
projects
repositories

tasks
task_dependencies

agents
agent_profiles

executions
execution_attempts
sessions

artifacts
messages

evaluations
evaluation_results

policies
policy_bindings
approvals

memories
memory_links
decisions

context_epochs
context_checkpoints
context_items
context_plans

schedules

provider_installations
provider_accounts
usage_samples

events
audit_entries

secret_refs
```

Physical schema должна оставаться owned adapters/storage concern.

---

# 11. Task Engine

Task — главная business entity.

```text
Task
 ├── Objective
 ├── Parent/Epic
 ├── Dependencies
 ├── Acceptance Criteria
 ├── Scope
 ├── Risk
 ├── Required Verification
 ├── Required Review
 ├── Artifacts
 └── Status
```

Пример descriptor:

```yaml
id: AUTH-042
parent: AUTH-EPIC

objective: Implement refresh-token rotation.

depends_on:
  - AUTH-038

acceptance:
  - refresh token is single-use
  - old session is invalidated
  - integration tests pass

scope:
  allowed_paths:
    - apps/api/src/auth/**
    - apps/api/test/auth/**
  forbidden_paths:
    - infrastructure/**
    - apps/web/**

verification:
  - pnpm test auth
  - pnpm typecheck

risk:
  security: high

required_review:
  - security
```

---

# 12. Task state machine

```text
DRAFT
  ↓
READY
  ↓
CLAIMED
  ↓
RUNNING
  ├──→ BLOCKED ───→ RUNNING
  ↓
SUBMITTED
  ↓
AWAITING_REVIEW
  ↓
VERIFYING
  ├── failure → RETRY_READY → RUNNING
  ↓
COMPLETED
```

Side states:

```text
FAILED
CANCELLED
```

Критическое правило:

> Agent не имеет API `complete_task()`.

Agent может только:

```text
submit_result()
```

`COMPLETED` выдаёт verification/evaluation pipeline.

---

# 13. Task ≠ Execution ≠ Attempt ≠ Session

```text
Task AUTH-42
  │
  ├── Execution E1
  │      ├── Attempt 1 / Codex / failed
  │      └── Attempt 2 / Codex / rejected
  │
  └── Execution E2
         └── Attempt 1 / Claude / passed
```

Session — временная provider-specific сущность внутри attempt.

---

# 14. Agent Model

Persistent Agent profile может иметь:

```text
id
name
role
skills
policy binding
preferred provider
preferred model
memory scope
working style
```

Но Task не зависит от конкретного Agent.

Scheduler может сделать:

```text
Task → Codex
```

а после failure:

```text
Task → Claude
```

без изменения Task semantics.

---

# 15. Provider Adapters

Canonical interface conceptually:

```rust
trait AgentProvider {
    fn detect(&self) -> Result<Installation>;
    fn capabilities(&self) -> Result<ProviderCapabilities>;
    fn start(&self, request: StartExecution) -> Result<ProviderSession>;
    fn send(&self, session: &ProviderSession, input: AgentInput) -> Result<()>;
    fn cancel(&self, session: &ProviderSession) -> Result<()>;
}
```

Provider adapter не может:

- менять task state;
- писать memory напрямую;
- делать merge;
- обходить Policy Engine;
- решать scheduling.

Он переводит canonical request ↔ provider CLI/runtime.

---

# 16. FakeAgent обязателен до реальных providers

FakeAgent должен поддерживать сценарии:

```text
success
crash
hang
timeout
malformed_result
forbidden_path_write
forbidden_command
quota_reached
network_failure
partial_result
review_rejection
```

Весь orchestration pipeline должен проходить без реального LLM.

---

# 17. Runner abstraction

```text
Runner
 ├── NativeRunner
 ├── ContainerRunner
 ├── RemoteRunner      later
 └── VMRunner          later
```

## NativeRunner

Для interactive trusted work.

UI должен честно показывать:

```text
Isolation: Trusted Host
```

Это **не sandbox**.

## ContainerRunner

Основной вариант для autonomous work.

Предпочтительно rootless.

Mount:

```text
/workspace  rw
```

Не mount:

```text
~/.ssh
~/.aws
~/Documents
/
```

## VMRunner

Для high-risk/untrusted repositories и enterprise hardening позже.

## G6 implementation foundation (TASK-0064)

TASK-0064 materializes a provider-neutral Runner/OS-isolation enforcement
foundation without choosing a concrete OS sandbox or container runtime:

```text
already-computed EffectiveCapabilities
  + exact ProcessExecute target
  + non-authorizing IsolationRequirements
  -> deterministic IsolationPlan
  -> replaceable IsolationBackend
  -> exact-plan EnforcementReport
  -> verify filesystem / network / process axes
  -> privileged process operation
```

`crates/runner` owns these contracts. Exact process authority is rechecked at
the operation boundary; cross-workspace filesystem scope, stale/widened backend
reports and insufficient/unavailable isolation fail closed before execution.
`adapters/runner-native` is explicitly `Trusted Host` and reports zero sandbox
axes, so it cannot satisfy a task that requires filesystem, network or process
isolation.

TASK-0064 does not select Docker/Podman, Seatbelt, Linux namespace/seccomp/
cgroup mechanisms, Windows restricted-token/Job Object/AppContainer mechanisms
or any equivalent. The concrete platform backend matrix remains open.
TASK-0065 closes H7 worktree pollution at the provider-process environment
boundary: inherited environment is cleared, provider-writable XDG/state/cache/
scratch/temp roots are private and outside managed worktrees, and unsafe roots
fail before spawn. Provider-owned authentication remains an explicit profile/
HOME anchor and grants no canonical authority. Integrated production-path G6
wiring remains TASK-0067.

---

# 18. Workspace / Git

Git — first-class subsystem.

```text
Task
  ↓
Worktree
  ↓
Branch
  ↓
Execution
  ↓
Diff
  ↓
Verification
  ↓
Merge Queue
```

Каждый autonomous execution получает отдельный worktree.

Не разрешать agent напрямую писать в main/master.

Каждый `TASK-XXXX` выполняется в отдельной Git-ветке; отдельный worktree для
этой ветки предпочтителен. Имя ветки обязано содержать task ID, например
`task-0062-approval-enforcement` или эквивалент по текущей naming convention.

Во время реализации `main`/`master` не изменяется. Task branch попадает в
`main` только после полного выполнения task, удовлетворения acceptance criteria
и зелёной required verification suite. Незавершённая или красная task branch
не merge-ится. Если merge оставляет конфликт или post-merge verification
красная, `main` ещё не принял task: конфликт/ошибка должны быть устранены, а
verification повторена до зелёного результата.

---

# 19. Git failure modes, которые обязаны тестироваться

- dirty repository;
- dirty worktree;
- branch collision;
- concurrent task touching same files;
- untracked files;
- process crash;
- rebase conflict;
- merge conflict;
- cleanup while process alive;
- stale worktree;
- failed checkout;
- deleted branch;
- user edits files while agent works.

---

# 20. Orchestrator

Orchestrator управляет **работой**, не provider details.

Его задачи:

- dependency readiness;
- assignment;
- retries;
- blocker propagation;
- review routing;
- verification routing;
- task recovery;
- handoff;
- cancellation.

---

# 21. Reconciler model

Long-term правильная архитектура ближе к Kubernetes reconciliation, чем к одному LLM-planner.

```text
Desired State
     │
     ▼
Actual State
     │
     ▼
Reconciler
     │
     ▼
Next Actions
```

Пример desired state:

```yaml
desired_state:
  feature: stripe-subscriptions
  tests: passing
  security_review: required
  human_review_before_merge: true
```

Planner может строить plan, но не является абсолютным источником истины.

---

# 21.1 Adaptive Hierarchical Planning

Research on WriteHERE (EMNLP 2025) reinforces a planning model that is more flexible than generating one complete plan before execution. We adopt the **interleaving principle**, not the writing-specific implementation.

The target runtime model is:

```text
Coarse goal / existing plan revision
        ↓
Select currently actionable task
        ↓
Is it sufficiently atomic for this execution policy?
   ├── yes → execute → collect evidence → reconcile
   └── no  → propose bounded decomposition
                    ↓
             validate change set
                    ↓
          persist new plan revision
                    ↓
              continue execution
```

Planning therefore happens **just in time** and may adapt to real execution outcomes. A failed test, newly discovered dependency, changed assumption or missing prerequisite may justify a revision of only the affected branch instead of regenerating the entire project plan.

## 21.1.1 Planner is a proposal engine, not an authority

The Planner must live behind a replaceable port and never mutate Task state or the graph directly. Conceptually:

```rust
trait PlannerPort {
    fn assess_atomicity(&self, input: AtomicityRequest) -> Result<AtomicityAssessment>;
    fn propose_decomposition(&self, input: DecompositionRequest) -> Result<PlanChangeSet>;
    fn propose_replan(&self, input: ReplanRequest) -> Result<PlanChangeSet>;
}
```

A `PlanChangeSet` is declarative and versioned. It should contain at least:

```text
base_plan_revision
reason
source_evidence_refs
node additions/updates/removals
edge additions/removals
expected outcome
risk delta
budget delta
scope delta
```

Application services validate the proposal before it becomes durable state. Validation includes:

- DAG remains acyclic;
- dependencies reference valid nodes;
- parent acceptance criteria are preserved/covered;
- child scopes stay inside authorized project/task scope;
- policy and human-approval requirements are not weakened;
- budget/depth/fan-out limits are respected;
- duplicate or self-similar recursive decomposition is rejected;
- risk escalation triggers the required approval gate.

Only after validation does the Task/Plan application layer persist the new revision transactionally and append corresponding domain events to the Event Journal.

## 21.1.2 Plan revisions are first-class

The task graph is durable, but its **plan structure is not assumed immutable**. Every structural mutation creates a new revision with provenance:

```text
PlanRevision 17
  reason: integration test exposed missing migration step
  caused_by: TEST-RESULT-8831
  added: TASK-214 migration validation
  changed_edges: TASK-210 → TASK-214 → TASK-211
  proposed_by: Planner execution P-91
  validated_by: Reconciler/Policy
```

This enables audit, replay, diffing between plans and UI explanations such as “why was this task created?”.

## 21.1.3 Lifecycle state and scheduling readiness are different

WriteHERE uses a compact `ACTIVE/SUSPENDED/SILENT` scheduler state. We do **not** replace our richer Task lifecycle with those states. Instead, readiness is a derived scheduling concern.

Example:

```text
Persistent lifecycle:
DRAFT / READY / CLAIMED / RUNNING / BLOCKED / SUBMITTED / VERIFYING / COMPLETED ...

Derived scheduling readiness:
RUNNABLE / WAITING_ON_DEPENDENCY / WAITING_ON_POLICY / EXPANDED / TERMINAL
```

This prevents scheduler mechanics from leaking into durable business semantics.

## 21.1.4 Task type is extensible metadata

WriteHERE gains value by decomposing work into heterogeneous task types. We adopt the principle but not the fixed writing-specific categories `retrieval/reasoning/composition`.

Our planner may label work with extensible execution classes such as:

```text
Research
Design
Implementation
Verification
Integration
Operations
```

These labels guide decomposition strategy, capability routing, context preparation and evaluator selection. They must not become provider-specific branching in domain code. A future registry/strategy may add task kinds without changing the Task state machine.

## 21.1.5 Recursive planning requires hard liveness guardrails

The public WriteHERE repository has open reports involving infinite summarization loops, recursion-depth failures and decomposition that produces only thinking-type subtasks without progressing to writing. These are exactly the failure modes our runtime must prevent structurally.

Required controls:

```text
max_decomposition_depth
max_children_per_decomposition
max_total_nodes_per_plan
max_plan_mutations_per_reconcile_cycle
max_planning_time / token budget
no-progress timeout
duplicate-decomposition fingerprint detection
cycle detection
policy-controlled scope/risk escalation
```

Every decomposition must demonstrably move toward executable leaves. If the planner cannot produce a valid smaller decomposition within limits, the task becomes `BLOCKED`/requires escalation instead of recursively planning forever.

A parent task may not be considered resolved merely because children were created. Completion still depends on independent verification of the parent's acceptance contract.

## 21.1.6 Graph-scoped context control

WriteHERE's paper also separates structural memory (task graph + workspace) from the actual LLM context. We strengthen our existing Context Engine rule:

> The durable work graph is knowledge available to the system; it is **not** automatically the model prompt.

For a task, Context Engine should preferentially retrieve:

```text
current node
+ relevant ancestors/goal constraints
+ completed prerequisite outputs
+ selected dependent/sibling evidence when needed
+ relevant workspace/resources
```

not the full plan graph or full execution history. The exact policy remains replaceable behind Context ports.

## 21.1.7 Observability

The UI should eventually expose adaptive planning as a trust feature, not hidden chain-of-thought. Show durable operational facts only:

- task DAG and task kinds;
- plan revisions;
- dependencies and readiness;
- why a node was added/blocked/replanned;
- evidence/artifacts that triggered a revision;
- budget/guardrail interventions.

Do not expose or depend on private model chain-of-thought.

## 21.1.8 Evidence boundary

WriteHERE's published evidence is strong **for long-form writing**, including an EMNLP 2025 paper and ablations where removing recursive planning degraded report/story metrics. It is not direct evidence that the same policy improves software engineering. We therefore adopt this as a well-supported architectural hypothesis and must benchmark it against static upfront planning on our own coding workloads before making it the default planner strategy.

# 22. Typed Agent Bus

Не строить систему на свободном agent-to-agent chat.

Основные events/messages:

```text
TaskAssigned
TaskClaimed
TaskBlocked
ClarificationRequested
DecisionProposed
ArtifactProduced
ReviewRequested
ReviewRejected
ReviewApproved
TestFailed
TestPassed
MergeRequested
TaskSubmitted
```

Пример:

```json
{
  "type": "review.requested",
  "taskId": "AUTH-42",
  "attemptId": "a93f",
  "from": "backend",
  "to": "security-reviewer",
  "artifacts": ["git:commit:91ea882"]
}
```

Свободные textual messages разрешены как payload, но workflow state должен быть typed.

---

# 23. Context Engine — core subsystem

Context Engine равен по важности Task Engine.

Он отвечает за:

- context planning;
- token budgeting;
- retrieval;
- file navigation;
- deduplication;
- stale-result pruning;
- compaction;
- security filtering;
- provider rendering;
- cache planning;
- context observability.

Он **не отвечает** за:

- task scheduling;
- provider execution;
- persistent business state;
- UI.

---

# 24. Главный принцип context management

```text
Don't load it
  ↓
Just-in-time retrieval
  ↓
Deduplicate
  ↓
Prune stale tool output
  ↓
Structured state instead of raw history
  ↓
Retrieve relevant memory only
  ↓
Compact old execution history
  ↓
Prompt caching
  ↓
Serialization optimization
```

TOON/CSV compact formats — последний слой, не главный способ экономии.

---

# 25. Context as materialized view

**Invariant:** model context — disposable representation.

Authoritative sources:

```text
Task DB
Files
Git
Events
Memory
Decisions
Artifacts
```

Prompt/context package можно полностью пересобрать.

Потеря prompt не должна означать потерю business state.

---

# 26. ContextItem

Conceptual model:

```rust
struct ContextItem {
    id: ContextItemId,
    kind: ContextKind,
    source_ref: SourceRef,
    content_hash: ContentHash,
    priority: Priority,
    freshness: Freshness,
    trust: TrustLevel,
    sensitivity: DataClass,
    required: bool,
    cache_class: CacheClass,
    provenance: Provenance,
}
```

---

# 27. ContextPlan

Для каждого execution сохраняем plan:

```text
Required
Selected
Deferred
Rejected
Budget
Cache boundaries
Renderer
Reasons
Source hashes
```

UI должен уметь ответить:

> Почему этот файл попал в context?

---

# 28. Progressive disclosure для файлов

Не делать один огромный `ARCHITECTURE.md`.

Пример:

```text
/docs
  /architecture
    INDEX.md
    runtime.md
    context-engine.md
    memory.md
    policy.md
    storage.md
```

Level 0:

```text
repository tree
```

Level 1:

```text
INDEX summary
```

Level 2:

```text
specific document
```

Level 3:

```text
section / symbol
```

Level 4:

```text
raw evidence
```

---

# 29. AI-friendly documentation format

Markdown — default human/agent-readable format.

Каждый architecture file начинается с high-density summary:

```markdown
# Context Engine

Purpose:
Select the smallest sufficient context for each execution.

Owns:
- context planning
- token budgeting
- retrieval

Does not own:
- task scheduling
- model execution

Critical invariants:
- context is derived state
- secrets never enter context
```

Один концепт — один разумно ограниченный файл.

---

# 30. Retrieval architecture

Не делать «всё → embeddings → top 10 → LLM» единственным механизмом.

Hybrid retrieval:

```text
Exact/direct lexical scan
      │
Indexed lexical / trigram candidates
      │
SQLite FTS5 / BM25
      │
Optional embeddings
      │
AST / symbol index
      │
Git history / diff
      │
Metadata / recency
      │
      ▼
Candidate Ranker / Retrieval Planner
      │
      ▼
Agent opens and verifies exact evidence just-in-time
```

RAG и любые indexes — candidate generators, не authoritative truth. Источник остаётся исходным repository content/evidence.

## 30.1 Indexed lexical repository retrieval

Review `microsoft/tgrep` подтверждает, что быстрый deterministic lexical search должен быть отдельной first-class retrieval strategy для больших codebase, а не побочным shell convenience.

Целевой boundary:

```text
Context / Resource application layer
          ↓
RepositoryTextSearchPort
          ↓
replaceable strategies/adapters
          ├── direct scan / ripgrep-class
          ├── trigram indexed lexical search
          └── future equivalent implementation
```

`RepositoryTextSearchPort` является архитектурной границей; конкретный `tgrep-core` — только возможный future adapter и не входит в domain contracts.

### Base snapshot + live delta

Для mutable repository index предпочитать модель:

```text
RepositoryIndex
├── PublishedSnapshot(revision=N)    # immutable / rebuildable
└── LiveDelta                        # changed / created / deleted
```

Live delta перекрывает snapshot. Полный rebuild после каждого edit не требуется.

Индекс всегда:

```text
derived
rebuildable
discardable
```

и никогда не становится source of truth.

### Watcher is not authority

Filesystem notifications используются только как low-latency change hints. Они могут теряться, overflow'иться или не отражать wholesale tree changes.

Поэтому correctness требует отдельного reconciliation path:

```text
watcher events
   ↓
fast incremental update

periodic / explicit reconciliation
   ↓
filesystem truth comparison
   ↓
repair stale / missing / deleted index entries
```

Native watcher exhaustion может приводить к bounded polling fallback. Отсутствие watcher backend не должно тихо превращаться в stale-forever index.

### Freshness / consistency is observable

Repository retrieval должен уметь различать семантически как минимум:

```text
FAST
  derived index допустим в рамках известной freshness

FRESH
  live/reconciliation state учитывает известные recent mutations

AUTHORITATIVE
  exact source прочитан/проверен перед использованием результата как evidence
```

Имена будут формализованы в G9, но сама граница обязательна.

Search evidence должен уметь объяснять:

```text
strategy
index/source revision
freshness/consistency
candidate count
verified source count
fallback reason
material exclusions
```

Это входит в observability `ContextPlan`, а не в скрытую оптимизацию.

### Correctness-preserving fallback

Если query невозможно безопасно сузить индексом, индекс недоступен/повреждён или current-source correctness важнее latency:

```text
indexed optimization unavailable
→ exact/direct scan or another correct strategy
```

Optimization failure ухудшает performance, но не correctness.

Corrupt index:

```text
!= corrupt project
```

Он маркируется degraded/invalid, может быть отброшен и rebuilt. Нельзя silently использовать известный partial/corrupt index как полную repository truth.

### Resource budgets

Repository caches и builders должны иметь явные byte budgets, не только entry counts:

```text
CacheBudget
├── max_entries
├── max_total_bytes
└── max_single_entry_bytes
```

Large index builds должны быть memory-bounded и допускать spill/external merge или эквивалентную стратегию, чтобы indexing не вытеснял active agent runtimes из памяти.

### Progressive disclosure

Для broad lexical queries предпочитать:

```text
matching file/symbol census
→ selected locations
→ bounded context snippets
→ exact source read
```

Не загружать тысячи grep-lines в model context только потому, что retrieval tool может их вернуть.

### Lexical vs code intelligence

Разделять:

```text
RepositoryTextSearchPort
→ text / bytes / regex / literals / paths

CodeIntelligencePort
→ symbols / definitions / references / type/semantic graph
```

Retrieval Planner может комбинировать их, но ни один port не поглощает другой.

### Concrete tgrep adoption decision

На текущем этапе:

```text
ADAPT  trigram/snapshot/live/reconciliation/resource-budget patterns
DEFER  direct tgrep-core dependency until G9 benchmark + security review
REJECT standalone tgrep serve TCP/JSON-RPC daemon in production agentd architecture
```

Мы уже имеем OS-authenticated UDS/Named Pipe + versioned Protobuf для local control plane; второй localhost TCP control service не должен появляться только ради search implementation.

Перед прямым adoption `tgrep-core` провести benchmark против direct/ripgrep-class search, FTS/BM25 и structural lookup на representative repositories, включая cold/warm index, recent edits, branch switches, broad queries, latency, RSS, index size, correctness parity и freshness lag.

**Gate:** этот target относится в первую очередь к **G9 — Context Engine + Continuity**. Он не изменяет текущий G3 FakeAgent Runtime scope/queue.


## 30.2 Structural repository graph retrieval

Review `Graphify-Labs/graphify` confirms a second retrieval axis alongside lexical search: a **revision-bound structural evidence graph** built from AST/symbol relations and selected typed document/architecture references.

This does not introduce a new authoritative engine. It refines the existing `CodeIntelligencePort` boundary:

```text
CodeIntelligencePort
├── resolve symbol / definition / references
├── structural neighbors
├── directed relation path
├── dependency / call / import cycles
├── affected / reverse-dependency traversal
└── revision-to-revision structural diff (future)
```

`RepositoryTextSearchPort` remains responsible for exact/lexical text discovery. `CodeIntelligencePort` owns structural candidate evidence. `Retrieval Planner` may combine both and still verifies exact source when correctness requires authority.

### Structural relation evidence

Every derived relationship must retain enough provenance to distinguish source fact from inference:

```text
StructuralRelationEvidence
- source / target
- relation
- source_ref + source_location
- source_revision
- extractor / extractor_version
- evidence_class
- optional confidence
- provenance
```

Conceptual evidence classes:

```text
EXACT_STRUCTURAL
DERIVED_STRUCTURAL
SEMANTIC_INFERENCE
AMBIGUOUS
```

The exact names are deferred until G9 contracts. Confidence is an estimate, **not authority**. A relation with confidence `0.95` cannot authorize a mutation, verification success or task completion.

### Revision and corpus identity

A graph query must expose what source universe it represents:

```text
StructuralGraphSnapshot
- repository_ref
- source revision / manifest hash
- corpus-definition revision
- extractor version
- build completeness
```

The graph is:

```text
derived
rebuildable
revision-bound
```

Never canonical source state.

All full, incremental, hook and background rebuild paths must share **one canonical corpus-membership function**. A changed-files subset is a delta, not a replacement corpus.

### Build completeness before publication

A structural index must track completeness independently from graph size:

```text
expected sources
processed sources
failed / omitted sources
source manifest hash
partial flag
prior published graph revision
candidate graph revision
```

A partial/incomplete build must not silently replace a known-good published graph merely because serialization succeeded. Large unexpected graph shrinkage is a useful guard signal, but source coverage/completeness is the stronger integrity check.

### Affected / blast-radius analysis

Structural traversal may produce candidate impact evidence for:

```text
Context planning
Review Packet
Test selection
Security review scope
Delivery Reconciler
architecture explorer
```

Every affected hit should retain the path/relation/source location that explains *why* it is considered affected.

```text
Affected(symbol X)
→ relation/path evidence
→ candidate impacted symbols/files/tests
→ exact verification where required
```

Impact analysis is advisory evidence. It does not grant merge, completion or policy authority.

### Code ↔ intent traceability graph

The structural graph may connect code evidence to our already-canonical intent layers:

```text
Requirement
→ ChangeSpec revision
→ Task
→ Execution / workspace diff
→ symbol/file
→ ADR / rationale
→ verification evidence
```

This is a **derived traceability view** over canonical sources, not a second owner of Requirement, Task, Git or verification facts.

### Communities and graph centrality

Community detection, centrality / "god node" analysis and surprising cross-module connections are useful for navigation and UI, but remain heuristic derived views.

They may suggest:

- subsystem candidates;
- architecture hotspots;
- context grouping;
- review/merge-risk areas.

They may not automatically redefine bounded contexts, task decomposition, code ownership, security boundaries or merge order.

### Cross-project structural view

Future multi-repository support should federate namespaced project graphs:

```text
ProjectGraph(A) ─┐
ProjectGraph(B) ─┼─→ CrossProjectGraphView
ProjectGraph(C) ─┘
```

Do not collapse projects into one mutable global JSON authority. Cross-project entity resolution requires explicit provenance; label equality alone is not canonical identity.

### Retrieval policy

Do not make "graph first" a universal rule.

```text
query / task need
      ↓
Retrieval Planner
      ├── exact/direct lexical
      ├── indexed lexical
      ├── AST/symbol
      ├── structural graph
      ├── Git/history/diff
      ├── docs/requirements/ADRs
      └── optional semantic/embedding retrieval
      ↓
exact evidence verification
```

For broad architecture questions, graph traversal can be a high-value candidate generator. For exact code facts, current source remains authoritative.

### Incremental structural-index integrity

Graphify's real issue history reinforces several permanent rules for our background indexers:

```text
incremental delta != full corpus

all rebuild entry points use one corpus definition

failed/hollow/incomplete extraction != complete build

known-good derived index is not overwritten by an untrusted partial rebuild

absolute/relative source addressing stays rooted to canonical repository identity
```

Background watchers are hints; reconciliation against filesystem/Git source truth is the correctness mechanism, consistent with section 30.1.

### Graph storage and dependency decision

Do not adopt Graphify/NetworkX/Python as the `agentd` code-intelligence core. Keep graph storage and traversal replaceable behind `CodeIntelligencePort` and benchmark Rust-native/embedded approaches later.

Graph JSON may be supported as a future import/export artifact, not canonical durable state.

MCP/HTTP graph servers may be supported later as external plugin adapters, but they do not become an internal control plane; product control remains behind our accepted OS-authenticated local IPC/security architecture.

### Benchmark requirement

Graphify reports promising retrieval/memory results and an ERPNext code benchmark where adding its tool improved key-fact coverage, but the code set is currently only six graded questions and uses roughly 140K tokens/query. Treat it as directional evidence only.

G9 must benchmark structural graph retrieval against lexical/indexed lexical/symbol/BM25/optional embedding baselines on our own representative repositories, measuring quality, token budget, latency, freshness, index resource cost and corruption recovery.

**Gate:** structural graph construction/retrieval belongs primarily to **G9 — Context Engine + Continuity**. Current G4 Workspace/Git sequence is unchanged. Later consumers include Review Packet, Verification evidence selection, Delivery Reconciler and Architecture Explorer.

---


## 30.3 Execution Efficiency Architecture — work avoidance, context efficiency and token optimization

Reviews of `juliusbrussee/caveman` and `dietrichgebert/ponytail` refine the production optimization target. The objective is **not merely to minimize tokens**. The product should minimize the total cost of delivering a correct, independently verified change while preserving security, robustness, data safety and architecture invariants.

The optimization stack is ordered deliberately:

```text
USER / TASK INTENT
        │
        ▼
1. WORK AVOIDANCE / SOLUTION MINIMALITY
        │
        ├── YAGNI where genuinely applicable
        ├── reuse existing project capability
        ├── standard library / native platform
        ├── already-installed dependency
        └── minimum valid new implementation
        │
        ▼
2. EXACT CONTEXT SELECTION
        │
        ├── RepositoryTextSearchPort
        ├── CodeIntelligencePort / structural graph
        ├── Git / diff / history
        ├── ADRs / requirements / docs
        └── progressive disclosure
        │
        ▼
3. CONTEXT MATERIALIZATION / COMPRESSION
        │
        ├── shape-aware compression
        ├── exact recovery reference
        └── fail-through when unsafe/unprofitable
        │
        ▼
4. CACHE OPTIMIZATION
        │
        ├── stable prefixes
        ├── cache epochs / invalidation
        └── provider-aware reuse
        │
        ▼
5. MODEL + REASONING ROUTING
        │
        ├── model/provider
        ├── reasoning level
        └── latency / quota / risk trade-off
        │
        ▼
6. EXECUTION
        │
        ▼
7. INDEPENDENT VERIFICATION
        │
        ▼
8. EFFICIENCY OBSERVATORY + LEARNING/PROMOTION
```

This ordering matters. The cheapest token is often the one never needed because unnecessary work was never created; the next cheapest is context never retrieved; compression comes only after selection.

### Optimization objective: verified delivery, not one vanity metric

Do not optimize any single metric in isolation:

```text
MIN(tokens)             → insufficient
MIN(LOC)                → insufficient
MIN(provider_cost)      → insufficient
MIN(latency)            → insufficient
```

Target a multi-objective `EngineeringEfficiency` / `VerifiedDeliveryCost` concept that can account for:

```text
correctness / verification success
security and data-safety floors
robustness
wall-clock time
provider cost
input/output/reasoning tokens
cached vs uncached input
cache hit/miss behavior
tool-call count
retry count
human intervention / attention
changed LOC
new dependencies
new durable state
new public API/abstractions
maintenance surface
recovery overhead
```

Hard floors are non-negotiable:

```text
correctness
security
trust-boundary validation
data-loss prevention
accepted architecture invariants
explicit acceptance criteria
```

Only after these floors are satisfied may the system optimize cost/latency/tokens/complexity.

A lower-token strategy that increases retries or verification failures may be more expensive overall. A smaller implementation that removes required bad-input handling is not an optimization.

### P0 rule: no efficiency promotion without quality non-regression

Any optimization policy — compressor, retrieval policy, model downgrade, lower reasoning level, minimality policy, tool-schema reduction, cache transformation — must be evaluated against representative workloads before becoming a default:

```text
candidate optimization
        ↓
OBSERVE / SHADOW / controlled A-B
        ↓
measure quality + cost + latency + recovery + retries
        ↓
quality/security/robustness non-regression gate
       / \
   PASS   FAIL
    ↓       ↓
promote    reject / keep experimental
```

Conceptual permanent rule:

```text
NO EFFICIENCY PROMOTION WITHOUT QUALITY NON-REGRESSION
```

Savings claims are evidence with provenance, not marketing truth.

### Work Avoidance / Solution Minimality

The Ponytail review reinforces a useful pre-implementation decision ladder, but it must become structured product behavior rather than a large always-on prompt:

```text
Does this need to exist?
        ↓ if yes
Does the capability already exist in this codebase?
        ↓ if no
Does standard library support it?
        ↓ if no
Does the native platform support it?
        ↓ if no
Does an already-approved/installed dependency support it?
        ↓ if no
Can the requirement be satisfied by a smaller compatible change?
        ↓ if no
Create the minimum new mechanism that satisfies the contract.
```

The ladder runs **after enough repository understanding to avoid a superficial fix**. Minimality must not become laziness about reading the actual flow or finding the root cause.

The system should prefer reuse evidence generated by our own retrieval stack:

```text
Task needs capability X
        ↓
Repository Readiness / Code Intelligence
        ├── existing symbols / services / ports
        ├── existing architectural patterns
        ├── installed dependencies
        ├── standard/native capabilities
        └── related ADRs / ownership boundaries
        ↓
ReuseCandidateSet
        ↓
Planner / Execution Envelope
```

This turns “reuse before rewrite” into an evidence-backed mechanism rather than hoping the model remembers a prompt instruction.

### Minimality is not minimum LOC

Do not use raw LOC as authority. For example:

```text
6 lines with path traversal bug
```

is worse than:

```text
10 lines with the required trust-boundary validation
```

A minimality evaluator must preserve:

- input validation at trust boundaries;
- error handling required to prevent data loss;
- security controls;
- accessibility requirements where applicable;
- explicitly requested behavior;
- architecture invariants;
- migration/compatibility guarantees;
- verification requirements.

Independent Ponytail evidence found a real robustness trade-off when specifications omitted edge cases; therefore robust behavior cannot be simplified away merely because the spec failed to name every malformed input.

### Complexity Delta / Complexity Budget

Future Review Packet and planning may expose a bounded complexity signal, not an automatic rejection score:

```text
ComplexityDelta
- new dependencies
- new service / abstraction
- new durable state
- new configuration
- new public API
- new background process
- new files/modules
- duplicated existing capability?
- ignored native/stdlib capability?
```

A `ChangeSpec` may optionally carry a `ComplexityBudget`, for example:

```text
complexity_budget:
  new_dependencies: 0
  new_services: 0
  new_persistence: false
  new_public_api: minimal
```

Crossing a budget requires explicit justification; it is not automatically forbidden when the larger mechanism is genuinely required.

### Deliberate simplification with explicit upgrade trigger

When a simpler implementation is consciously chosen with a known ceiling, record the decision as typed evidence instead of leaving an unstructured TODO/comment:

```text
SimplificationDecision
- decision_id
- scope
- chosen_solution
- rejected_complex_solution / alternatives
- reason
- known_ceiling
- upgrade_trigger
- source evidence / measurements
- ChangeSpec revision
- provenance
```

Example:

```text
Chosen: linear scan
Reason: current corpus < 10k entries
Known ceiling: latency degrades at larger corpus
Upgrade trigger: p95 > 100ms OR corpus > 30k
Future candidate: indexed lookup
```

The Learning/Promotion/Reconciler layer can later detect that the upgrade trigger is crossed. “Later” must not silently become permanent architecture debt.

### Task-/role-/risk-aware minimality policy

Do not copy always-on `lite/full/ultra` prompt injection. A research agent, security reviewer, architecture agent and bounded implementation worker should not receive identical minimality instructions.

Future selection should use structured inputs:

```text
RoleProfile
+ TaskKind
+ ExecutionPhase
+ RiskClass
+ ChangeSpec
→ derived minimality/context policy
```

If named policy modes exist, their behavior must be materially different and machine-observable; labels must not merely inject almost-identical prompt text.

### Recoverable context compression

Caveman reinforces an architecture already compatible with our rule that Context is a disposable materialized view. Lossy compression is allowed only when exact source recovery remains available.

Target:

```text
Exact Resource / Evidence
        │
        ├── exact materialization
        │
        └── compressed materialization
                  │
                  └── RecoveryRef
                         ↓
                    exact original
```

A future `CompressionArtifact` should retain provenance sufficient to audit and invalidate it:

```text
CompressionArtifact
- source_ref
- source_hash
- source_revision
- compressor
- compressor_version
- original_tokens / bytes
- compressed_tokens / bytes
- lossiness class
- sensitivity
- recovery_ref
- provenance
```

Permanent semantic boundary:

```text
COMPRESSED != AUTHORITATIVE
COMPRESSED != VERIFIED
COMPRESSED != permission to mutate
```

If exact recovery cannot be guaranteed, a lossy transform fails through to the original/bounded exact evidence rather than silently discarding evidence.

Conceptual rule:

```text
NO RECOVERY → NO LOSSY COMPRESSION
```

### Shape-aware compressors

Do not summarize every payload with one generic LLM prompt. A future `ContextCompressorPort` can expose replaceable, typed strategies for common agent payloads:

```text
ContextCompressorPort
├── TestOutputCompressor
├── LogCompressor
├── JsonCompressor
├── DiffCompressor
├── SearchResultCompressor
├── CodeStructureCompressor
├── TerminalOutputCompressor
└── DocumentCompressor
```

Examples of desired behavior:

- tests: retain failures, panic/error context and exact failed names; collapse repetitive success noise;
- logs: prioritize errors, stack traces and diagnostic boundaries;
- JSON: preserve schema/keys and relevant error/decision subtrees;
- diffs: preserve file/hunk/change structure with exact recoverability;
- search: preserve high-value hits and exact source references;
- code: prefer symbols/signatures/structure when full bodies are not yet required.

Fixed arbitrary line caps may be implementation fallbacks, never correctness authority. Semantic selection + budget + recovery is preferred to “first N lines”.

### Tool-output compression is a high-value target

Tool-heavy coding agents repeatedly re-send terminal/test/search output through later model turns. Therefore repeated tool evidence is a likely major token sink.

Target pattern:

```text
trusted tool result
      ↓
exact artifact / source retained
      ↓
shape-aware compact view
      ↓
provider-visible context
      ↓ if model needs detail
RecoveryRef / exact bounded read
```

Tool-call/tool-result correlation and execution receipts remain intact; compression must not create dangling tool calls or remove provenance.

### Observe → Shadow → Active rollout

Context optimization should be introduced progressively:

```text
OFF
→ OBSERVE
→ SHADOW
→ ACTIVE
```

`OBSERVE`: measure token sinks without changing model-visible context.

`SHADOW`: build the compressed/optimized representation but still send the baseline/original representation; compare counterfactually where possible.

`ACTIVE`: use the optimized representation only after quality and safety gates pass.

The system must be able to fall back/pass through when compression is unsafe, not profitable or degraded.

### Effective Context Cost and provider cache awareness

Raw token count alone is not sufficient. Stable prompt prefixes and provider-native prompt caching can make a larger but mostly cached input cheaper/faster than a smaller cache-breaking input.

Conceptual metric:

```text
EffectiveContextCost
= uncached input cost
+ cached input cost
+ transformation overhead
+ latency impact
+ expected reuse
+ retry/recovery risk
```

Example:

```text
A: 40k input, 35k cached, 5k uncached
B: 25k input, 0 cached, 25k uncached
```

B has fewer raw tokens but may be economically and temporally worse.

Compression, rule rendering and context ordering should therefore preserve stable prefixes where useful and use explicit cache epochs/versioning when transformations change.

### Token / efficiency measurement provenance

Provider billing and local tokenizer estimates are different evidence classes. Future usage ledger should record provenance:

```text
TokenMeasurement
- value
- unit
- provider
- model
- source: ESTIMATED | PROVIDER_REPORTED | BENCHMARK_COUNTERFACTUAL | MEASURED_LOCAL
- cache_class
- execution_id
- timestamp
```

Savings claims should also carry evidence strength, for example:

```text
INFERRED
MEASURED
CONTROLLED_AB
VERIFIED
```

Do not present tokenizer-estimated savings as provider-billed savings.

### Exact context selection remains more important than compression

Compression does not replace retrieval discipline. Preferred order:

```text
avoid unnecessary work
→ retrieve only candidate evidence
→ progressively disclose exact regions
→ compress large retained payloads where beneficial
→ recover exact source on demand
```

Caveman browser-style evidence reinforces that focused extraction can reduce very large UI/DOM representations dramatically, while small pages can become larger after metadata/recovery overhead. Therefore compression/representation choice is workload-dependent and must be benchmarked, not universal.

### Derived compact policy views, not compressed authority

Canonical architecture/ADR/rules remain authoritative. Provider/agent-specific compact instructions are derived views:

```text
canonical rules / ADRs / ChangeSpec
          ↓ deterministic or validated renderer
compact Execution Envelope / AgentPolicyView
```

Do not let an LLM rewrite the canonical architecture into a shorter authoritative file merely to save tokens. A compact policy view must retain source revision/provenance and be regenerable.

### Tool-schema compression is deferred and high-risk

Tool schemas influence model behavior and available action semantics. Removing descriptions can change what the model calls or how it populates arguments.

Therefore tool-schema compression remains experimental until controlled benchmarks prove:

- tool selection non-regression;
- argument correctness non-regression;
- no security/permission ambiguity;
- cache and token benefit;
- exact schema recovery/reference.

Do not treat schema stripping as a generic token-saving trick.

### Text-as-image/pixel compression is not a core strategy

Encoding textual context as images may reduce provider token accounting for some multimodal models, but it is provider/model dependent and worsens exact retrieval, accessibility, provenance and error behavior. It is rejected as a core Context Engine strategy; it could only appear later as an isolated experimental plugin.

### Context recovery and secret/sensitivity policy

A recovery store can itself become a secret sink. Exact originals from terminal/log/tool output may contain credentials or sensitive user data.

Before durable recovery materialization:

```text
Sensitivity / Secret Policy
        ↓
Recovery retention decision
```

Do not copy secrets into a long-lived context recovery database merely to make lossy compression reversible. Recovery retention, encryption, TTL, local ownership and deletion policy must align with Secret Broker/Privacy architecture when this capability is implemented.

### Efficiency Observatory

Production optimization needs execution-level evidence:

```text
Context Efficiency Observatory
├── provider input/output/reasoning tokens
├── cached / uncached tokens
├── provider cost
├── model/reasoning selection
├── context materialized bytes/tokens
├── compression ratio
├── recovery calls
├── cache hits/misses
├── tool calls
├── wall-clock
├── retries
├── verification outcome
├── human interventions
├── changed LOC / complexity delta
└── task type / risk class
```

This belongs with the Usage Ledger/Observability architecture, not inside the model prompt.

### Adaptive model and reasoning selection is part of efficiency

The same task does not always require the strongest/slowest model or highest reasoning setting. Future routing should optimize quality/risk/latency/quota jointly and learn from project-local outcomes.

A provider/model benchmark score is evidence, not enough by itself. The system should prefer local measurements such as verified completion rate, retries, wall-clock, token use and human attention on comparable task classes.

### Combined placement

Primary implementation placement:

```text
G9  Context Engine + Continuity
     → exact selection
     → materialization/compression/recovery
     → cache-aware context planning

G7/G8 and later
     → minimality/complexity signals in planning/review
     → quality gates for efficiency promotion

G11 Background
     → observe/shadow benchmarking
     → index/compression maintenance
     → efficiency learning

Model Router / Usage Ledger
     → model + reasoning + provider efficiency
```

This section does **not** change the current G4 Workspace/Git sequence.

### Architecture principles adopted from Caveman + Ponytail reviews

The enduring rules are:

```text
Do less work.
Reuse before creating duplicate capability.
Retrieve less, but retrieve exact evidence when required.
Compress only derived context, never authoritative truth.
No recovery → no lossy compression.
Preserve cache value, not just raw token count.
Measure provider-reported and estimated tokens separately.
Treat LOC reduction as a signal, not correctness.
Never simplify away security, robustness or data safety.
Use task/risk-specific policies instead of large always-on prompts.
Promote optimization only after measured quality non-regression.
Optimize verified delivery cost, not token dashboards.
```


# 31. Code chunking

Для code retrieval chunking должен быть semantic:

- module;
- class;
- function;
- method;
- interface;
- symbol;
- ADR section.

Не фиксированные `500 tokens` как единственная стратегия.

---

# 32. Context Continuity Engine

Мы хотим provider-independent аналог/надстройку над идеями Astra-style context continuation.

Сущность:

```text
Task
 ├── Context Epoch 001
 ├── Context Epoch 002
 ├── Context Epoch 003
 └── ...
```

Epoch = bounded active context window.

---

# 33. Context rollover algorithm

1. Budget Monitor отслеживает soft/hard limits.
2. Перед soft limit создаётся structured checkpoint.
3. Checkpoint + notes + source refs пишутся durable transaction.
4. Старый epoch индексируется.
5. Persistence проверяется.
6. Только после успешного checkpoint создаётся новый epoch.
7. Новый context строится из stable instructions + task envelope + checkpoint + current working set.
8. Старые окна доступны через history search/read.
9. Raw evidence остаётся источником истины.

Критическое правило:

```text
NO CHECKPOINT → NO ROLLOVER
```

---

# 34. Три уровня context continuity

## Level 1 — Provider context

- Codex native compaction;
- Astra-style provider memory;
- Claude compaction;
- OpenCode native behavior.

Это provider optimization.

## Level 2 — Execution continuity

Наши:

- epochs;
- checkpoints;
- working notes;
- searchable task history.

## Level 3 — Durable project/task memory

- decisions;
- architecture;
- failures;
- skills;
- repository knowledge.

Task может перейти Claude → Codex без потери authoritative state.

---

# 35. History Search

История должна поддерживать:

```text
FTS/BM25
metadata filters
task filters
epoch filters
event type filters
optional embeddings
```

Agent сначала делает:

```text
history.search(...)
```

а затем:

```text
history.read(exact_ref)
```

Не загружать всю старую сессию в prompt.

---

# 36. Prompt caching

Context renderer должен разделять:

```text
STABLE
- system policy
- role
- stable tools
- project constitution

SEMI-STABLE
- architecture
- relevant ADRs

DYNAMIC
- current task
- current evidence
- recent events
- user input
```

Stable prefix должен идти раньше dynamic content.

Важно:

> cached tokens могут быть дешевле, но они всё ещё создают attention load.

Отдельно считать:

```text
Token Cost
Attention Load
```

---

# 37. Serialization formats

Canonical:

| Data | Canonical |
|---|---|
| Domain | Rust structs |
| Persistence | SQLite |
| IPC | Protobuf |
| Tool schema | JSON Schema |
| Task result | JSON Schema |
| Docs | Markdown |
| Code | Original |

Model-facing renderers могут быть:

```text
Markdown
JSON
Compact JSON
CSV
TOON
future formats
```

**TOON никогда не становится domain/persistence format.**

Renderer выбирается по model/task benchmark, а не религии формата.

---

# 38. TOON/TRON research decision

Идея compact AI serialization реальна, но end-to-end reliability может снижаться.

Поэтому:

```text
Canonical state
      ↓
Context Renderer
      ├── JSON
      ├── Markdown
      ├── TOON
      └── ...
      ↓
Model
```

Продукт должен уметь измерять:

```text
tokens
quality
parse failures
latency
```

и выбирать лучший renderer для конкретной модели/задачи.

---

# 39. Memory Engine

Memory не равно conversation transcript.

Типы:

```text
Semantic
Episodic
Procedural
Decision
```

## Semantic

Факты о проекте.

## Episodic

Что происходило в прошлых executions.

## Procedural

Как выполнять повторяемые действия.

## Decision

ADR/архитектурные решения и rationale.

---

# 40. Memory provenance

Каждая memory entry должна хранить:

```text
source refs
confidence
created_at
last_verified_at
supersedes
staleness
content hash where applicable
```

Пример:

```json
{
  "kind": "procedural",
  "fact": "Run codegen after protobuf changes",
  "sources": ["TASK-118/attempt-2", "git:4f82ab"],
  "confidence": 0.96
}
```

---

# 41. Memory lifecycle

```text
CAPTURE
  ↓
NORMALIZE
  ↓
DEDUPLICATE
  ↓
LINK
  ↓
VALIDATE
  ↓
USE
  ↓
REFRESH / SUPERSEDE
  ↓
ARCHIVE
```

Нельзя бесконечно append всё подряд.

Не делать summary-of-summary-of-summary источником истины.

Raw events/files/git остаются evidence.

---

# 42. Graph memory

Не добавлять Neo4j в v1.

Достаточно:

```text
memories
memory_links
entities
entity_links
```

в SQLite.

Если позже graph workload реально оправдает отдельную DB — adapter можно заменить.

---

# 43. Tool Registry

Не отправлять сотни tool schemas каждому агенту.

Default minimal tools:

```text
file.search
file.read
tool.search
task.status
history.search
history.read
```

Когда нужен GitHub tool:

```text
tool.search("create pull request")
```

только тогда загружается schema конкретного tool.

Плюсы:

- меньше tokens;
- меньше ambiguity;
- меньше attack surface.

---

# 44. Evaluation Engine

Generation — не definition of done.

```text
Agent submits result
      ↓
Evaluation Engine
      ├── tests
      ├── lint
      ├── typecheck
      ├── security
      ├── acceptance criteria
      ├── reviewer
      └── benchmarks
      ↓
VERIFIED / REJECTED
```

---

# 45. Executable Definition of Done

Пример:

```yaml
done_when:
  - pnpm test passes
  - typecheck passes
  - no critical security findings
  - API contract unchanged
  - reviewer score >= threshold
```

Task становится `COMPLETED` только после evaluation.

---

# 46. Reliability Score

Позже продукт может рассчитывать score не из LLM confidence, а из evidence:

```text
Tests
Review
Security
Requirement coverage
Historical agent performance
Integration tests
```

UI:

```text
Reliability 93/100
Missing: load test
```

---

# 47. Adaptive Model Router

Пользователь со временем должен выбирать цель:

```text
Best quality
Fast
Cheap
Private
Large context
Security review
```

а не вручную всегда выбирать модель.

Router учитывает:

```text
model × task type × repository
```

Метрики:

- success rate;
- first-pass verification;
- review rejection;
- latency;
- tokens/cost;
- subscription capacity.

---

# 48. Subscription Optimizer

Если пользователь уже платит за Claude/Codex/OpenCode, scheduler может учитывать доступные лимиты.

```text
Claude capacity
Codex capacity
OpenCode capacity
```

Routine work может уходить более дешёвому/свободному provider, critical work — сильнейшему.

Это optimization layer, не core task semantics.

---

# 49. Tournament Mode — later

Для важной задачи:

```text
Task
 ├── Claude solution
 ├── Codex solution
 └── Kimi solution
       ↓
Evaluator
       ↓
Winner
```

Сравнивать по:

- tests;
- benchmarks;
- security;
- diff size;
- review;
- complexity.

---

# 50. Consensus Mode — later

Для архитектуры:

```text
Round 1: independent proposals
Round 2: anonymized critique
Round 3: objections
Round 4: synthesis
Round 5: ADR
```

Не разрешать бесконечный свободный debate.

---

# 51. Shadow Agents — later

Primary работает, shadow дешёвая модель наблюдает и вмешивается только при высокой уверенности в проблеме.

Использовать для:

- security;
- requirement drift;
- architecture violations;
- regressions.

---

# 52. Security model

Главный принцип:

> Prompt-based restrictions не являются security boundary.

Нужны одновременно:

```text
Policy Engine
+
OS/container/VM isolation
```

---

# 53. Trust boundaries

Минимум:

```text
TB1 WebView → Tauri Host
TB2 Tauri Host → agentd
TB3 agentd → Agent CLI
TB4 Agent CLI → OS
TB5 Agent → Repository content
TB6 Agent → Internet/provider
TB7 App → Plugins/cloud services
```

Для каждой boundary должен существовать отдельный threat-model section.

---

# 54. Repository content считается untrusted input

В README/code может лежать prompt injection.

Следовательно:

```text
repository text ≠ trusted instruction
```

Security policy находится вне model-generated context authority.

---

# 55. Policy Engine

Любое privileged действие:

```text
Agent request
    ↓
Policy Engine
    ├── ALLOW
    ├── DENY
    └── REQUIRE_APPROVAL
```

Пример:

```yaml
filesystem:
  read:
    - workspace/**
  write:
    - workspace/apps/api/**

git:
  commit: allow
  push: approval
  force_push: deny

commands:
  pnpm: allow
  git: allow
  curl: approval
  ssh: deny
  sudo: deny
```

---

# 56. Agent Firewall — strategic feature

В будущем Policy Engine + isolation + audit превращаются в отдельный product moat:

```text
Agent
  ↓
Agent Firewall
  ├── filesystem
  ├── network
  ├── commands
  ├── secrets
  ├── API permissions
  ├── spend limits
  └── audit
  ↓
System
```

---

# 57. Secrets Broker

Actual secrets не хранятся в ordinary application/domain state, SQLite или
Event Journal. Durable state содержит только opaque `SecretRef` и exact
`SecretBinding` metadata:

```text
DB → SecretRef + SecretBinding(target = Execution | Tool)
```

TASK-0063 materializes provider-neutral `SecretsBroker`. Release принимает
только уже вычисленный `EffectiveCapabilities`, требует exact
`SecretAccess(secret_ref)` и exact bound delivery target до чтения backend:

```text
Policy / TASK-0062 approval
  → EffectiveCapabilities
  → SecretsBroker
  → exact SecretRef + exact Execution|Tool binding
  → local native credential backend
  → trusted SecretDelivery
```

Local native credential backend:

- macOS Keychain;
- Windows Credential Manager;
- Linux Secret Service.

Cloud/file fallback в TASK-0063 отсутствует. Borrowed secret bytes передаются
только trusted delivery boundary; временный mutable buffer очищается после
delivery attempt. Secret material не входит в ordinary logs, frontend state,
model context или approval explanation.

Если Claude/Codex CLI уже авторизован, наше приложение **не должно извлекать
его credential**.

Запускаем CLI и оставляем auth provider-native. Provider-native auth/permission
не является canonical authority и не может расширить product-owned
`EffectiveCapabilities`.

TASK-0063 не захватывает Runner/OS isolation (TASK-0064), provider environment
isolation/H7 (TASK-0065) или Security Audit persistence (TASK-0066).

---

# 58. Secrets никогда не входят в model context

Если tool требует secret, secret инжектируется на execution boundary, а не в prompt.

Context Security Filter должен работать до provider boundary.

Data classes:

```text
PUBLIC
PROJECT
PRIVATE
RESTRICTED
SECRET
```

`SECRET` renderer не пропускает.

---

# 59. Logs разделяются

Не один `logs.txt`.

## Operational Log

Технические события без prompts/source/secrets.

## Security Audit

Кто запросил privileged action, policy decision, approval, result.

## Agent Transcript

Отдельная user-controlled data category.

---

# 60. Desktop hardening

- strict CSP;
- no remote JS in main UI;
- sanitized Markdown;
- no frontend shell capability;
- separate browser WebView;
- signed builds;
- signed updater;
- dependency scanning;
- secret scanning;
- SAST;
- Cargo/Go/npm vulnerability monitoring.

---

# 61. Privacy architecture

Главное обещание:

# Local-first / cloud-optional

По умолчанию:

```text
Repository            LOCAL
Task history          LOCAL
Memory                LOCAL
Agent transcript      LOCAL
Git                   LOCAL
Analytics             LOCAL
Telemetry             OFF
```

---

# 62. Technical Privacy Contract

Это основа будущей юридической Privacy Policy.

```text
PRIV-001 Core product works without an account.
PRIV-002 Core product works without telemetry.
PRIV-003 Source code is not uploaded to our servers unless explicitly required by an enabled feature.
PRIV-004 Customer repositories/prompts/outputs are not used by us to train models.
PRIV-005 Personal data is not sold.
PRIV-006 Telemetry contains no source code, prompts, model outputs, secrets or raw filesystem contents.
PRIV-007 User can inspect locally stored application data.
PRIV-008 User can delete all app-owned local data.
PRIV-009 Cloud sync is opt-in.
PRIV-010 Provider data transfer is shown clearly.
```

---

# 63. Provider data flow must be explicit

Пример:

```text
Local App
   │
   ├── task metadata → stays local
   │
   └── Claude Code
           ↓
        Anthropic
```

Если CLI отправляет данные Anthropic/OpenAI, это не означает, что наш cloud получил эти данные.

UI должен показывать distinction.

---

# 64. Data Flow Inspector — strategic feature

Пример:

```text
Last execution

Sent to Claude provider:
14 files
31 KB

Sent to our servers:
0 bytes

Local memory writes:
3
```

Это одновременно privacy, trust и debugging feature.

---

# 65. Provider privacy labels

Пример:

```text
Claude Code
Execution: Local
Inference: Anthropic
Memory: Local
Telemetry: Off
Data leaves device: YES
```

Local model:

```text
Execution: Local
Inference: Local
Data leaves device: NO
```

---

# 66. Legal Privacy Policy должна отражать реальную систему

До публичного launch требуется отдельная юридическая проверка.

Политика должна описывать:

- account data;
- local data;
- AI providers;
- cloud sync;
- encryption;
- telemetry;
- crash reports;
- payments;
- subprocessors;
- retention;
- export;
- deletion;
- GDPR rights;
- international transfers;
- training usage;
- sale/ads policy.

Нельзя писать marketing policy, которую implementation не гарантирует.

---

# 67. Design system

Stack:

```text
Tauri 2
React
TypeScript
Vite
Tailwind
shadcn/ui primitives
Own Design System
```

shadcn/ui — implementation building blocks, **не продуктовый визуальный язык**.

---

# 68. UI layers

```text
components/ui
  → shadcn primitives

components/system
  → Status, Surface, SplitPane, DataRow, CommandBar

components/product
  → TaskCard, RunTimeline, ContextInspector,
    VerificationCard, PermissionGate, WorktreeDiff
```

Feature code не должен напрямую строиться из десятков случайных shadcn-компонентов.

---

# 69. Design direction

Продукт должен ощущаться как:

```text
Mission Control
+
Modern IDE
+
Operations Dashboard
```

Не как AI chat app.

Принципы:

- calm;
- dense;
- technical;
- trustworthy;
- progressive disclosure;
- keyboard-first;
- screenshotable;
- provider-neutral.

Избегать commodity AI aesthetics:

- неон ради неона;
- giant gradients;
- glass everywhere;
- giant chat bubbles;
- robot avatars как центральная часть продукта.

---

# 70. Главный экран — work-centric

Пример:

```text
┌──────────────────────────────────────────────────────────────┐
│ PROJECT ATLAS                 ⌘K            ● All systems OK│
├─────────────┬──────────────────────────────┬─────────────────┤
│ MISSION     │ OAuth implementation         │ INSPECTOR       │
│ CONTROL     │                              │ Context 18.2k   │
│             │ Plan ───●                    │ Isolation       │
│ Projects    │       ┌─┴─────┐              │ Container       │
│ Missions    │       ▼       ▼              │ Verification    │
│ Agents      │    Backend Frontend          │ 6/8 passed      │
│ Runs        │    Codex   Kimi              │                 │
│ Memory      │       └──┬────┘              │                 │
│ Policies    │          ▼                   │                 │
│             │     Security Review          │                 │
├─────────────┴──────────────────────────────┴─────────────────┤
│ Diff │ Terminal │ Tests │ Browser │ Events │ Context         │
└──────────────────────────────────────────────────────────────┘
```

Основной объект UI — работа/task state, а не «поговорить с агентом».

---

# 71. UI testing

Для product components:

- Storybook;
- Vitest;
- Testing Library;
- Playwright;
- axe accessibility;
- visual regression.

Каждый важный component имеет состояния:

```text
default
loading
empty
error
blocked
running
success
warning
permission-required
```

---

# 72. Agent developer awareness

Каждый coding-agent должен точно понимать:

- где он находится;
- текущий epic/task;
- сколько работы осталось;
- зависимости;
- allowed/forbidden files;
- relevant ADRs;
- acceptance criteria;
- required tests;
- что идёт после его task.

Для этого перед каждым execution генерируется **Execution Envelope**.

---

# 73. Execution Envelope

Пример:

```yaml
execution:
  id: exec_01J...

project:
  id: project_main
  name: AI Engineering OS

roadmap:
  current_epic: EPIC-03
  epic_progress: 7/13
  completed_tasks: 48
  remaining_tasks: 93
  blocked_tasks: 4

task:
  id: TASK-0317
  objective: Implement worktree lifecycle.

workspace:
  repository: /workspace/project
  worktree: /workspace/tasks/TASK-0317
  branch: agent/TASK-0317

scope:
  allowed:
    - crates/workspace/**
    - adapters/git-cli/**
  forbidden:
    - apps/**
    - crates/policy/**

architecture:
  required_docs:
    - docs/architecture/workspace.md
    - docs/adr/0012-worktree-isolation.md

acceptance:
  - worktree can be created
  - deletion is idempotent
  - dirty worktree cannot be silently destroyed

tests:
  required:
    - cargo test -p workspace

next_known_tasks:
  - TASK-0318
  - TASK-0320
```

Envelope генерируется из DB/task graph/git/ADRs. Он не редактируется вручную как источник истины.

---

# 74. AGENTS.md должен быть коротким

Корневой `AGENTS.md` только объясняет protocol работы.

Пример:

```text
1. Read current execution envelope.
2. Read referenced ADRs.
3. Do not modify paths outside task scope.
4. Do not expand scope silently.
5. Do not violate architecture invariants.
6. Run required verification.
7. Return task-result.json.
```

Не хранить весь проект в одном giant AGENTS.md.

---

# 75. Task result contract

Каждый AI development task возвращает machine-readable результат:

```json
{
  "taskId": "TASK-0317",
  "status": "submitted",
  "changedFiles": ["crates/workspace/src/worktree.rs"],
  "testsRun": ["cargo test -p workspace"],
  "testsPassed": true,
  "architectureChanges": [],
  "securityImpact": "none",
  "knownIssues": []
}
```

CI и verification engine не доверяют textual claim «готово».

---

# 76. Architecture Invariants

Файл `docs/architecture/INVARIANTS.md` должен содержать минимум:

```text
INV-001 Task state survives every agent/session/process restart.
INV-002 Task != Execution != Attempt != Agent Session.
INV-003 Frontend never executes system commands directly.
INV-004 Provider adapters never mutate domain state directly.
INV-005 Autonomous executions use isolated workspaces/worktrees.
INV-006 Agents never write directly to main/master.
INV-007 Destructive/privileged actions pass Policy Engine.
INV-008 Secrets never enter model context, logs or event payloads.
INV-009 Agent cannot mark its own task completed.
INV-010 Persistent state mutations are auditable.
INV-011 Cloud is optional for core functionality.
INV-012 Model/provider is not part of Task domain semantics.
INV-013 Prompt instructions are never the only security control.
INV-014 Core product works without telemetry.
INV-015 User can delete all app-owned local project data.
INV-016 Model context is a disposable materialized view.
INV-017 Model-facing serialization format is not part of domain model.
INV-018 Context selection is observable and reproducible.
INV-019 Provider context is not durable application state.
INV-020 No context epoch is discarded before durable checkpoint.
INV-021 Historical evidence remains retrievable after rollover.
INV-022 Provider-native compaction is optimization, not sole persistence.
INV-023 Task may migrate between providers without state loss.
INV-024 UI primitives do not define the product design system.
INV-025 Bounded contexts do not read each other's storage directly.
INV-026 Every infrastructure dependency is accessed through an explicit port.
INV-027 Replacing one adapter must not require changing unrelated domain crates.
```

---

# 77. ADR process

Начальные ADR:

```text
ADR-0001 Local runtime is Rust agentd
ADR-0002 Cloud control plane will use Go when needed
ADR-0003 Local IPC uses UDS/Named Pipe + versioned Protobuf
ADR-0004 Task is central durable entity
ADR-0005 Task != Execution != Attempt != Session
ADR-0006 v1: SQLite local state + append-only Event Journal; future: staged Event Sourcing migration
ADR-0007 No direct shell from frontend
ADR-0008 Worktree isolation
ADR-0009 Provider adapters are anti-corruption layers
ADR-0010 Context is derived/materialized state
ADR-0011 Provider-independent Context Continuity
ADR-0012 Structured Memory with provenance
ADR-0013 Policy Engine + OS isolation
ADR-0014 Local-first privacy
ADR-0015 shadcn/ui is primitive layer, not design system
ADR-0016 Core first / agentctl before complex UI
```

Любое изменение invariant требует ADR + explicit review.

---

# 78. Testing strategy

## Unit

Pure domain logic.

## Contract

Один test suite для каждого implementation:

```text
AgentProvider Contract
Runner Contract
TaskStore Contract
ContextRetriever Contract
PolicyEngine Contract
Evaluator Contract
```

## Integration

- real SQLite;
- temp git repositories;
- real process boundaries where useful;
- FakeAgent.

## E2E

Tauri + agentd + FakeAgent before real provider E2E.

## Security

- path traversal;
- symlink escapes;
- shell injection;
- malicious repo instructions;
- secret leakage;
- WebView capability escapes;
- IPC malformed messages;
- unauthorized client attempts.

## Chaos

- kill agentd;
- kill provider;
- disk full;
- network loss;
- quota reached;
- UI closes;
- stale socket;
- corrupted partial result;
- interrupted DB transaction.

---

# 79. Development Gates

Не планировать только календарными датами. Использовать exit criteria.

## G0 — Architecture Freeze

До production implementation существуют:

```text
PROJECT_CHARTER.md
ARCHITECTURE.md
INVARIANTS.md
THREAT_MODEL.md
PRIVACY_ARCHITECTURE.md
DATA_CLASSIFICATION.md
ROADMAP.yaml
ADR set
Task schema
Event schema
Provider contract
Runner contract
Context contract
```

## G1 — Tauri ↔ agentd

Только:

```text
start
handshake
ping
version negotiation
reconnect
shutdown
```

Тесты:

- daemon missing;
- wrong version;
- crash;
- stale socket;
- permission error;
- reconnect.

## G2 — Task Core

Только:

- Project;
- Task;
- Dependency;
- State machine;
- SQLite;
- Event Journal;
- `agentctl`.

Без AI.

## G3 — FakeAgent Runtime

Полный Task → Scheduler → FakeAgent → Submit → Verify flow.

## G4 — Workspace/Git

Worktrees, branches, diff, cleanup, failure modes.

## G5 — First real provider

Один provider полностью проходит AgentProvider contract.

После этого второй provider должен добавляться без изменений Task/Storage/UI core.

## G6 — Security

Policy Engine, approvals, Secret Broker, audit, runner security levels.

G6 authority is product-controlled and provider-neutral. Canonical privileged
requests carry scoped requested capabilities and deterministic policy produces
`ALLOW | DENY | REQUIRE_APPROVAL`. Requested authority is not effective
authority; child/delegated effective authority must remain a subset of parent
effective authority. Hard deny cannot be overridden by UI mode, provider,
repository/workspace content or approval. Approval binds the exact request,
scope, policy fingerprint, provenance and expiry. Provider-native permission
controls are defense in depth only. Policy decides while the operation owner
enforces close to the privileged action; preflight alone is not a security
boundary. Runner security profiles, if exposed, compile to canonical
capability/isolation requirements rather than becoming a second authority
model. Detailed decision: ADR-0026 and `docs/security/POLICY_CONTRACT.md`.

TASK-0061 materializes deterministic layered policy evaluation. TASK-0062
materializes durable exact-bound approvals: the authenticated local control
plane can observe an existing requirement and record human approve/deny, while
only the operation owner may turn an approved row into effective authority by
re-presenting the current matching `REQUIRE_APPROVAL` decision. Grants are
restart-durable, expiry-bound and single-use; mismatch, denial and replay fail
closed.

## G7 — Multi-agent DAG + bounded adaptive planning

Planner/Worker/Reviewer + typed messages. Add versioned plan revisions, `PlannerPort`, atomicity/decomposition proposals and hard recursion/liveness guardrails. Dynamic planning remains feature-gated until it beats static planning in our own coding benchmarks.

## G8 — Verification

Tests/review/security/acceptance pipeline.

## G9 — Context Engine + Continuity

Retrieval, token budget, context plans, epochs, checkpoints, history search.

## G10 — Memory Engine

Structured memory + provenance + lifecycle.

## G11 — Background daemon

Authorized work continues with UI closed.

## G12 — Remote Worker

Device identity, outbound worker connection, remote execution.

## G13 — Cloud/Team

Account, billing, encrypted sync, teams, org policies.

---

# 80. Что НЕ строим в v1

Не нужны для первого сильного продукта:

- voice orchestration;
- dictation;
- image generation;
- generic AI chat app;
- fancy agent avatars;
- 30+ plugins;
- plugin marketplace;
- remote fleet;
- cloud sync;
- microVM runner;
- Neo4j;
- Kafka/NATS;
- Redis без конкретной необходимости;
- full Event Sourcing в v1; сохранить его как post-v1 architectural target и мигрировать только после стабилизации bounded contexts;
- autonomous CTO;
- Tournament Mode;
- Consensus Mode;
- Semantic Conflict Detection;
- Shadow Agents.

Архитектура должна позволять их добавить позже без переписывания core.

---

# 81. Future Cloud Architecture

Cloud появляется только когда действительно нужен.

```text
Clients / local agentd
        │
        ▼
Go Control Plane
        │
   PostgreSQL
        │
Object Storage
```

Функции cloud:

- account;
- billing;
- encrypted sync;
- organizations;
- teams;
- remote control;
- device discovery;
- org policies;
- hosted workers.

Local `agentd` остаётся authoritative для local execution.

Не переносить local orchestration в cloud без необходимости.

---

# 82. Future Remote Workers

```text
Cloud / Coordination
       │
       ├── MacBook agentd
       ├── Mac Mini agentd
       └── VPS agentd
```

Workers делают outbound authenticated connection.

Не открывать public agent ports.

Позже: device keypair + mTLS/device identity.

---

# 83. Product moat

GUI around CLI — commodity.

Настоящая ценность:

```text
Durable Work
Context Intelligence
Memory
Verification
Policies
Observability
Historical performance data
Adaptive routing
```

---

# 84. Главные USP

## 84.1 Durable AI Team

Task переживает:

- model switch;
- context rollover;
- provider quota;
- process crash;
- UI restart;
- handoff.

## 84.2 Verified Delivery

Agent не говорит «готово» — система доказывает готовность.

## 84.3 Adaptive Model/Context Routing

Система учится, какой provider/model/context strategy лучше именно для конкретного repository/task type.

## 84.4 Agent Firewall

Security becomes a product, not warning text.

## 84.5 Context Continuity

Provider-independent history/checkpoints allow very long tasks without привязки к одному context window.

---

# 85. Future differentiators

## Context Replay

Показывает, какой context получил конкретный attempt и почему.

## Time Travel

Task timeline:

```text
10:18 plan
10:22 code
10:27 tests failed
10:31 review
10:36 fix
10:41 pass
```

Создание new branch/checkpoint из прошлой точки.

## Semantic Conflict Detection

Ловить логические конфликты между параллельными branches даже без Git conflict.

## Architecture Guardian

Code graph + architecture rules + Project Constitution.

## Project Constitution

```yaml
rules:
  - controllers contain no business logic
  - auth changes require two reviews
  - destructive migrations require human approval
```

## Self-improving Context Policy

Не fine-tune модель, а улучшать retrieval/context policy на основе успешных/неуспешных задач.

---

# 86. Monetization strategy

Важно: не продавать просто красивый wrapper за $50.

## Community — Free

- local desktop;
- local agents;
- Codex/Claude/OpenCode adapters;
- worktrees;
- task DAG;
- local context/memory;
- one machine.

Это growth funnel.

## Pro — ориентир €15–20/month

- advanced context analytics;
- history replay;
- model router;
- background daemon;
- multi-machine limited;
- advanced memory.

## Power — ориентир €35–40/month

- autonomous workflows;
- advanced firewall;
- remote workers;
- larger scheduling;
- premium workflow packs.

## Team — ориентир €25–40/user/month

- shared projects;
- shared policies;
- approvals;
- organizational memory;
- dashboards;
- shared runners.

## Enterprise

- SSO;
- SCIM;
- RBAC;
- private cloud;
- policy-as-code;
- audit export;
- VPC workers;
- compliance;
- air-gapped deployment.

---

# 87. Additional revenue streams

## Hosted Workers

Продавать compute/environment, а не обязательно model tokens.

## Validated Workflow Marketplace

Не prompt marketplace.

Продавать:

```text
roles
workflow
skills
evaluators
policies
tests
benchmarks
```

С metric history:

```text
success rate
runtime
version
verification score
```

## Enterprise Agent Firewall

Может быть отдельным продуктом/SDK.

---

# 88. Design as marketing

Дизайн — часть acquisition и доверия.

Каждый основной экран должен быть:

- понятен за несколько секунд;
- хорош на screenshot;
- хорошо смотреться в видео/demo;
- показывать работу и evidence;
- не перегружать новичка;
- предоставлять density экспертам.

Главная marketing message не должна быть:

> Run all your AI coding agents in one place.

Это легко копируется.

Лучше:

> Give your AI team an outcome. The system plans, assigns, isolates, remembers, verifies and delivers the work across the agents you already use.

---

# 89. Проверка архитектуры на заменяемость

Перед merge крупного архитектурного изменения задавать вопросы:

### Provider replacement

Можно ли добавить новый provider без изменения Task Engine?

### Storage replacement

Можно ли заменить SQLite adapter без изменения domain crate?

### UI replacement

Можно ли написать новый UI, используя versioned IPC, без переноса business logic?

### Runner replacement

Можно ли NativeRunner заменить ContainerRunner без изменения provider adapter?

### Retrieval replacement

Можно ли заменить FTS/BM25 strategy без изменения Memory/Task Engine?

### Context renderer replacement

Можно ли убрать TOON/JSON renderer без migration persistent data?

### Policy replacement

Можно ли изменить policy implementation без изменения agent providers?

### Scheduler replacement

Можно ли заменить scheduler/reconciler, сохранив task semantics?

Если ответ «нет» — boundary нужно пересмотреть.

---

# 90. Dependency direction

Общая идея:

```text
UI
 ↓
IPC Contracts
 ↓
Application Services
 ↓
Domain Modules
 ↑
Ports
 ↑
Adapters
```

Infrastructure зависит от domain contracts, а не наоборот.

`agentd` composition root — единственное место, где всё собирается вместе.

---

# 91. Anti-corruption layer для providers

Provider updates неизбежны.

Claude/Codex/OpenCode могут менять:

- CLI flags;
- auth;
- output format;
- context behavior;
- session storage;
- tool protocol.

Поэтому provider-specific parsing находится полностью внутри adapter.

Core получает только canonical:

```text
ProviderEvent
ProviderSession
AgentOutput
ToolRequest
UsageSample
ProviderCapability
```

---

# 92. Astra / provider context research decision

Современные providers могут иметь собственные compaction/context-memory mechanisms.

Мы используем их, когда доступны, но не делаем их фундаментом.

```text
Provider native context
        ↓
Execution Continuity Engine
        ↓
Durable Task/Project State
```

Если завтра provider уберёт experimental feature, наша Task continuity продолжает работать.

---

# 93. Research-backed context principles

Мы принимаем следующие практические выводы:

1. Большой context window сам по себе не решает long-running reasoning.
2. Coding-agent file navigation / search должен быть first-class.
3. Just-in-time retrieval предпочтительнее постоянной загрузки всего проекта.
4. RAG полезен как candidate retrieval, но не единственный memory mechanism.
5. Context pruning может одновременно снижать токены и повышать качество.
6. Tool schemas должны загружаться deferred/on-demand.
7. Structured durable state лучше полного transcript history.
8. Prompt caching полезен, но не заменяет context optimization.
9. Compact formats вроде TOON — secondary optimization.
10. Формат должен benchmark'иться per model/task.

---

# 94. Definition of G0 complete

G0 считается закрытым только когда:

- все текущие архитектурные решения оформлены ADR;
- invariants приняты;
- bounded contexts перечислены;
- каждый context имеет owned responsibilities и forbidden responsibilities;
- threat model draft существует;
- privacy contract существует;
- IPC contract skeleton существует;
- Task state machine формализован;
- Context continuity model формализован;
- Provider/Runner/FakeAgent contracts готовы;
- repository layout создан;
- первая очередь atomic tasks определена.

После этого можно безопасно начинать G1 implementation.

---

# 95. Что должен читать coding-agent перед работой

Минимальный порядок:

```text
1. AGENTS.md compatibility pointer → AGENT.md
2. generated Execution Envelope
3. referenced architecture INDEX
4. referenced ADRs
5. конкретные source files
```

Не просить модель перечитать весь repository documentation каждый раз.

---

# 96. Правило размера AI development tasks

Не:

> Implement Context Engine.

А:

> Implement ContextBudget value object and tests.

или:

> Implement SQLite adapter for ContextCheckpointStore against existing contract.

Каждая task должна иметь небольшой blast radius.

---

# 97. Scope protection

Если агент обнаруживает необходимость изменить forbidden/out-of-scope компонент:

```text
STOP
 ↓
Proposal
 ↓
New task / ADR if needed
```

Он не расширяет scope молча.

---

# 98. Architecture change process

Любая новая идея проходит:

```text
Problem
 ↓
Proposal
 ↓
Alternatives
 ↓
Impact on invariants
 ↓
Security impact
 ↓
Privacy impact
 ↓
Migration impact
 ↓
ADR
 ↓
Implementation tasks
```

Это особенно важно при работе большого количества AI-agents.

---

# 99. Критерий успеха архитектуры

Через год должно быть возможно:

- заменить Claude adapter;
- добавить новый model provider;
- заменить UI;
- поменять retrieval engine;
- добавить cloud;
- добавить remote runners;
- заменить local storage;
- расширить policy model;
- изменить memory strategy;

без переписывания Task domain и всей системы.

Если core business semantics остаются стабильны, архитектура работает правильно.

---

# 100. Краткая финальная формула продукта

```text
Tauri UI
  ↓
Thin Rust Host
  ↓
Rust agentd
  ↓
Durable Tasks
  + Context Engine
  + Context Continuity
  + Memory
  + Orchestration
  + Verification
  + Policy/Security
  ↓
Provider Adapters
  ↓
Claude / Codex / OpenCode / Future Agents
```

Cloud later:

```text
Go Control Plane
+
PostgreSQL
+
Object Storage
```

И главный архитектурный принцип:

> **Модели, UI, storage, retrieval algorithms и providers меняются. Durable task semantics, contracts, evidence и security boundaries должны пережить эти изменения.**

---

# 101. Следующие документы, которые должны появиться из этого Source of Truth

Этот файл — umbrella document. После G0 его нужно разложить на отдельные source-controlled документы:

```text
docs/
├── architecture/
│   ├── ARCHITECTURE.md
│   ├── INVARIANTS.md
│   ├── task-engine.md
│   ├── orchestration.md
│   ├── context-engine.md
│   ├── continuity.md
│   ├── memory.md
│   ├── policy.md
│   ├── evaluation.md
│   ├── workspace.md
│   ├── ipc.md
│   └── storage.md
│
├── security/
│   ├── THREAT_MODEL.md
│   ├── TRUST_BOUNDARIES.md
│   ├── DATA_CLASSIFICATION.md
│   └── SECURITY_TEST_PLAN.md
│
├── privacy/
│   ├── PRIVACY_ARCHITECTURE.md
│   ├── DATA_FLOW_MAP.md
│   ├── TECHNICAL_PRIVACY_CONTRACT.md
│   └── SUBPROCESSOR_MODEL.md
│
└── adr/
    └── ...
```

Но до появления этих документов **этот файл является текущим source of truth**.


---

# 102. External repository research adoption

A separate research pass over Agency Agents, Superpowers, MiroFish, OpenViking, Lightpanda, Impeccable, Page Agent, Context Hub, DeepAgents and BitNet produced several additions to the architecture. Detailed rationale lives in `docs/research/external-projects-adoption.md` / `EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md`.

## 102.1 Adopted immediately

- **OpenViking pattern:** Context resources use progressive L0/L1/L2 representations; L0/L1 are derived and L2/source remains authoritative. Retrieval selection must be observable.
- **Superpowers pattern:** atomic tasks, evidence-before-completion, TDD for meaningful behavior, separate spec-compliance review before code-quality/security review.
- **Context Hub pattern:** external docs are versioned/incrementally fetched; persistent agent annotations are untrusted by default.
- **DeepAgents pattern:** large tool results are offloaded into artifacts and subagents receive isolated minimal contexts; skills are loaded on demand.
- **Impeccable pattern:** durable product truth (`PRODUCT.md`) is separate from visual/interaction truth (`DESIGN.md`); deterministic design/a11y checks are part of frontend verification.
- **Agency Agents pattern:** agent definitions have typed deliverables, success metrics, escalation/handoff rules and capability requirements; personality never grants authority.
- **Page Agent pattern:** future browser automation separates LLM reasoning from a deterministic Browser Controller and prefers compact DOM/text state when vision is unnecessary.

## 102.2 Deferred extension points

- **Lightpanda:** future `BrowserAutomationPort` adapter; do not make it a hard dependency or bundle before compatibility/license review.
- **BitNet:** future `LocalInferencePort` for low-risk local utility workloads; no control-plane authority.
- **MiroFish:** inspiration for a future synthetic Orchestration Simulation Lab using FakeAgents; no integration into v1 execution model.

## 102.3 Additional domain concepts

Future Context/Tool implementation must account for:

```text
ContextResource
├── stable internal URI
├── kind / source / version / content hash
├── trust / sensitivity / provenance
├── L0 abstract
├── L1 overview
└── L2 source/detail reference

RetrievalTrace
├── query
├── candidate path
├── selection/rejection reasons
└── final ContextItem references

ToolResult
├── short inline summary
├── artifact_ref
├── content_hash
├── size/token estimate
└── provenance
```

Large outputs are stored once through a future `ArtifactStorePort`; context carries handles until exact content is required.

## 102.4 Additional ports to preserve replaceability

These are architecture targets, not instructions to create empty crates immediately:

```text
DocumentationSourcePort    # versioned external technical docs
ArtifactStorePort          # large tool/evidence blobs
SkillRegistryPort          # lazy reusable workflows
BrowserAutomationPort      # future browser backends
LocalInferencePort         # future local helper models
ProviderRendererPort       # future provider-native agent/skill rendering
```

## 102.5 Additional invariants

```text
INV-028 Skills are lazy-loaded; full skill bodies are not baseline context.
INV-029 Agent-generated annotations/learned notes are untrusted until validated.
INV-030 Context abstracts/summaries are derived; source evidence stays authoritative.
INV-031 Retrieval preserves an observable selection trajectory.
INV-032 Large tool outputs are content-addressed artifacts, not repeated prompt history.
INV-033 Browser automation is isolated behind a controller/port and browser-neutral.
INV-034 PRODUCT.md and DESIGN.md are separate sources of product vs visual truth.
INV-035 Agent personality/role metadata never grants security or completion authority.
INV-036 Copyleft/AGPL implementations require explicit license review before bundling/linking.
```

These additions do not change the G0→G1 sequencing. Browser automation, BitNet, swarm simulation and marketplace work remain outside v1 core gates.

---

# 103. Resource Engine and semantic resource handling

Research on `karpathy/reader3` reinforced an architectural separation that is now part of the target design: resource ingestion/normalization must be its own replaceable bounded context rather than being embedded inside the Context Engine.

## 103.1 Resource Engine boundary

```text
External / Local Sources
         │
         ▼
┌──────────────────────────────┐
│       RESOURCE ENGINE        │
│                              │
│ ingest                       │
│ parse                        │
│ normalize                    │
│ semantic segmentation        │
│ representation generation    │
│ provenance                   │
│ structural metadata          │
└──────────────┬───────────────┘
               │
               ▼
        Resource Store
               │
               ▼
┌──────────────────────────────┐
│        CONTEXT ENGINE        │
│                              │
│ retrieve/search              │
│ rank                         │
│ budget                       │
│ select                       │
│ security filter              │
│ provider rendering           │
└──────────────┬───────────────┘
               │
               ▼
             Agent
```

The Resource Engine owns how source material becomes semantically addressable knowledge. The Context Engine owns which existing knowledge enters a specific execution. Neither may reach into the other's persistence implementation directly.

## 103.2 Semantic boundaries before token chunks

Preferred segmentation follows the strongest structural representation available:

```text
Code       repository/package/file/symbol
Markdown   document/heading/section
EPUB       book/TOC/spine/chapter/section
PDF        document/chapter/section/table/figure, then page fallback
```

Fixed-size token chunks may be generated for retrieval/provider constraints but are never authoritative project/resource structure.

## 103.3 Resource representations

A single resource can have multiple derived representations:

```text
Resource
├── Original                 authoritative
├── Display                  UI-safe representation
├── PlainText                search/index use
├── Markdown                 optional normalized form
├── ModelOptimized           renderer-specific derived form
├── SummaryL0                compact abstract
└── SummaryL1                navigational overview
```

Every derived representation is linked to source evidence, its content hash and the parser/renderer version that created it. Derived representations are rebuildable materialized views.

## 103.4 Structural confidence

When authoritative structure does not exist, parsers may infer it but must expose the provenance/confidence of that inference.

Target enum/concept:

```text
Authoritative
Parsed
Inferred
Fallback
```

Context ranking may use structural confidence as one signal but must never hide it from observability/debugging.

## 103.5 Resource Registry

Filesystem presence is not authoritative application state. The registry tracks source identity, hash/version, parser version, processing state, trust/sensitivity, available representations and structural metadata. If a source disappears, the resource enters an explicit missing/unavailable state rather than silently disappearing from durable history.

## 103.6 ResourceParserPort

Target extension point:

```text
ResourceParserPort
```

Future adapters may include code/AST, Markdown, HTML, PDF, EPUB and DOCX. Add adapters only when real product requirements need them; do not create placeholder modules merely to satisfy the architecture diagram.

## 103.7 Security model for imported resources

All imported/external/user-controlled resource content is **untrusted data**. Repository files, README instructions, books, webpages and documents may contain prompt-injection text.

The system must preserve the distinction:

```text
SYSTEM / TASK AUTHORITY
        !=
RESOURCE CONTENT
```

Resource text never gains policy, architecture or instruction authority automatically.

Display sanitization and model-context normalization are separate pipelines. UI rendering uses an allowlisted sanitizer and safe URL handling. Model normalization retains provenance/trust metadata and never promotes document text to system instruction.

## 103.8 Additional invariants

```text
INV-037 Fixed-size token chunks are derived retrieval units, not authoritative semantic structure.
INV-038 Imported/user/external resource content is untrusted data and never gains instruction authority automatically.
INV-039 Original/source evidence remains authoritative over display/model/summary representations.
INV-040 Resource parsing/normalization is isolated from Context selection behind replaceable contracts.
INV-041 Persistent user-controlled resources must not use deserialization mechanisms capable of arbitrary code execution.
INV-042 UI/display sanitization and model-context normalization are separate pipelines.
INV-043 Planner output is a proposal; only validated application services may mutate the durable task graph.
INV-044 Every runtime graph mutation creates a versioned plan revision with reason and provenance.
INV-045 Recursive decomposition is bounded by depth, fan-out, node, time/token and no-progress limits.
INV-046 Decomposition may not weaken parent acceptance criteria, security policy, scope or human-approval requirements.
INV-047 Task lifecycle state is distinct from derived scheduling readiness.
INV-048 Task kinds are extensible routing/planning metadata and must not couple domain logic to specific providers.
INV-049 Durable task/work graphs are retrieval sources, not prompts; Context Engine selects graph-local evidence just in time.
INV-050 Planning loops must terminate in executable work, an explicit blocker, or escalation; silent infinite replanning is a correctness failure.

```

These additions do not expand the v1 implementation gate. The Resource Engine boundary and contracts are architecture targets; support for EPUB/PDF/DOCX is deferred unless a concrete product task requires it.


# 104. Living Documentation and Architecture Explorer (adapted from Davia)

Davia validates an important product direction: repository documentation can be a first-class interactive artifact shared between humans and coding agents. We adopt the concept, but not Davia's repository-loading or persistence architecture.

## 104.1 Documentation is a derived knowledge product, not authoritative state

Generated documentation is a **materialized view** derived from authoritative evidence:

```text
Source code / Resources / ADRs / Task state / Events / Git history
                         │
                         ▼
                Documentation Engine
                         │
                         ▼
       Generated documentation projections
                         │
              ┌──────────┴──────────┐
              ▼                     ▼
          Human UI              Agent context
```

A generated page, summary or diagram must never become more authoritative than the evidence it was derived from merely because an LLM wrote it.

Every generated documentation artifact must carry at least:

- artifact ID and version;
- source/provenance references;
- source content hashes or revision identifiers where possible;
- generator/renderer version;
- creation/update time;
- freshness/staleness state;
- confidence/validation state;
- human-edited vs generated ownership metadata.

## 104.2 DocumentationEnginePort

Documentation generation is isolated behind replaceable contracts and must not be embedded into Resource Engine, Context Engine or UI code.

Target ports/concepts:

```text
DocumentationEnginePort
DocumentationRendererPort
DiagramRendererPort
DocumentationIndexPort
```

The engine consumes normalized resources and durable project state. It does not directly scan arbitrary repository paths or call provider-specific APIs without going through existing Resource/Context/Agent abstractions.

## 104.3 Three-layer documentation model

To prevent AI regeneration from overwriting valuable human edits, documentation uses three conceptual layers:

```text
Evidence Layer
    source code, resources, ADRs, events, task results

Generated Layer
    rebuildable summaries, maps, dependency explanations, diagrams

Curated Layer
    explicit human edits, annotations, accepted explanations, overrides

Rendered View
    deterministic composition of Generated + Curated with provenance
```

Human-authored curated content must never be silently overwritten by regeneration. Generated content may be rebuilt at any time.

## 104.4 Incremental freshness instead of full regeneration

Documentation refresh is dependency-driven:

```text
git/resource change
      │
      ▼
impact analysis
      │
      ▼
affected documentation nodes
      │
      ▼
mark stale
      │
      ▼
selective regeneration / human review
```

Do not regenerate an entire repository knowledge base for every change. Documentation dependencies should be explicit enough that the system can explain **why a page or diagram is stale**.

Suggested freshness states:

```text
Fresh
PossiblyStale
Stale
Regenerating
NeedsReview
```

## 104.5 Diagrams are projections, not opaque truth

Interactive diagrams are valuable for architecture comprehension and marketing/UI quality, but the visual canvas itself should not become the only canonical representation of system relationships.

Preferred model:

```text
Canonical structured graph / declarative diagram source
              │
              ├── Mermaid renderer
              ├── React Flow renderer
              ├── Excalidraw renderer
              └── future renderer
```

A renderer is replaceable. A diagram generated by an LLM must retain node/edge provenance back to source symbols/resources/decisions where feasible.

Free-form whiteboards may exist as user-owned artifacts, but they are distinct from evidence-grounded architecture graphs.

## 104.6 Architecture Explorer / Project Map

The product should eventually expose an interactive architecture explorer built from Resource Engine + code graph + durable project state rather than from a one-shot LLM narrative.

Candidate views:

- repository/module/package map;
- symbol/dependency graph;
- service/data-flow diagram;
- task-to-code impact graph;
- ADR-to-component relationships;
- runtime/event flow;
- security/trust-boundary view;
- documentation freshness map.

Selecting a node should expose provenance, relevant files/symbols, related tasks/ADRs, token/context cost and available actions such as **Attach to Task**, **Ask Agent**, **Open Evidence**, **Regenerate Explanation**.

## 104.7 Documentation as context, with anti-feedback-loop rules

Generated documentation can accelerate agent understanding, but it is secondary evidence. Context Engine must distinguish:

```text
Primary evidence   source code, authoritative config, ADRs, verified task results
Secondary evidence generated docs, summaries, diagrams
Curated evidence   explicitly accepted human documentation
```

An agent may use generated docs for navigation, but important claims should be resolved back to primary evidence when correctness matters. Repeated generations must not recursively treat previous AI summaries as unquestioned truth.

## 104.8 Repository-content authority boundary

Repository/resource content must **never** be concatenated into system-level instructions merely for convenience. Imported source files remain untrusted evidence, even when used to generate documentation.

Correct separation:

```text
System / Policy / Task authority
              │
              ├───────────────┐
              ▼               ▼
      Agent instructions   Context references
                              │
                              ▼
                    untrusted repository data
```

This rule applies to documentation generation agents as strongly as to coding agents.

## 104.9 Context acquisition for documentation

Do not copy Davia's whole-repository prompt strategy. Documentation generation must use the normal Resource + Context pipeline:

```text
Repository
   ↓ Resource Engine
semantic units / code graph / metadata
   ↓ Context Planner
just-in-time relevant evidence
   ↓ Documentation Agent
```

Hard caps such as file count and token budget remain safety budgets, not retrieval strategies. Token estimates must use provider/model tokenization where available rather than a universal `4 chars = 1 token` heuristic.

## 104.10 Documentation proposal workflow

For generated or updated documentation that may overwrite user-visible knowledge:

```text
Generate proposal
      ↓
Diff / visual preview
      ↓
Automated grounding checks
      ↓
Accept / reject / edit
      ↓
Publish projection
```

Autonomous background refresh may be allowed for clearly generated/rebuildable views, but curated documentation requires a merge-like policy.

## 104.11 UI/editor implementation guidance

Davia demonstrates that a documentation workspace benefits from multiple presentation primitives (rich text, code, node graphs, whiteboards). Our UI may use libraries such as Tiptap, React Flow or Excalidraw where licensing/security remain acceptable, but each must sit behind our product component/design-system boundary.

Do not couple the domain to any editor library. The persisted documentation model must survive replacing Tiptap/React Flow/Excalidraw.

## 104.12 Security and privacy requirements

- API/provider credentials are never stored in plaintext project JSON/config files.
- Provider authentication remains behind Provider/Secret Broker abstractions.
- Documentation cloud sync is opt-in and separate from local generation.
- Before any upload, Data Flow Inspector must show which artifacts/resources leave the machine.
- Generated HTML/MDX/SVG/diagram content is untrusted until sanitized for its rendering surface.
- Headless browser conversion is not part of the core documentation contract; renderers may use isolated adapters when required.

## 104.13 Additional invariants

```text
INV-051 Generated documentation is derived state and never outranks its authoritative source evidence by default.
INV-052 Human-curated documentation must not be silently overwritten by AI regeneration.
INV-053 Every generated documentation artifact must retain sufficient provenance to explain what evidence produced it.
INV-054 Documentation refresh is incrementally freshness-aware; full repository regeneration is not the default update strategy.
INV-055 Repository/resource content may not be promoted into system instruction authority during documentation generation.
INV-056 Diagram/editor libraries are replaceable renderers; visual representations are not domain state.
INV-057 Generated documentation used as agent context remains secondary evidence and must not create recursive summary-as-truth feedback loops.
INV-058 Documentation cloud synchronization is opt-in and independent from the local documentation engine.
```

## 104.14 Delivery priority

**P2 product capability after Resource Engine, Context Engine, durable Task Core and basic repository graph are stable.**

A minimal Architecture Explorer can arrive earlier as a read-only projection, but a collaborative Notion-style editor and whiteboard are not v1 requirements.

---

# Adaptive Model Routing and Local Resource Scheduling

## Purpose

Model/provider selection is a replaceable decision subsystem. The domain never assumes that a particular provider, subscription, API, local model or hardware backend is permanent.

The system distinguishes **eligibility**, **preference** and **runtime capacity**.

A model that scores highly but cannot satisfy a hard requirement is not a candidate.

## Core boundaries

```text
Task / Execution Requirements
          |
          v
     Model Router
          |
   +------+------+----------------+
   |             |                |
Catalog      Quality/Profile   Capacity/Resources
   |             |                |
   v             v                v
Models       Evidence        Device/Quota state
          |
          v
     Ranked RoutePlan
          |
          v
      Agent Runtime
```

`ModelRouter` does not directly:

- call Ollama/llama.cpp/provider APIs;
- inspect `nvidia-smi` or OS GPU APIs;
- mutate task state;
- execute agents;
- manage credentials;
- judge task completion.

Those responsibilities remain behind ports.

## Required ports

Conceptual contracts:

```text
ModelRouterPort
ModelCatalogPort
ModelProfilerPort
BenchmarkEvidencePort
DeviceResourceManagerPort
ModelResidencyManagerPort
ProviderCapacityPort
RoutingFeedbackPort
```

Implementations may evolve independently.

## Model profile decomposition

Do not store a single universal "model score" as authoritative truth.

Maintain separate projections:

```text
ModelCapabilities
ModelQualityProfile
ModelRuntimeProfile
ProviderCapacityProfile
PrivacyProfile
```

### ModelCapabilities

Examples:

- context limits;
- tools/function calling;
- structured outputs;
- vision/audio;
- streaming;
- provider-native compaction/context features;
- supported effort/reasoning modes.

### ModelQualityProfile

Quality evidence is task-class-specific and may include:

- coding;
- architecture/reasoning;
- codebase retrieval;
- security review;
- frontend/UI;
- research;
- summarization;
- repository-specific historical outcomes.

Every evidence item retains provenance, date/version and whether it is measured, externally reported or estimated.

### ModelRuntimeProfile

Measured for a concrete runtime/hardware fingerprint:

- cold start;
- warm latency;
- throughput;
- memory footprint;
- context-size performance;
- reliability/error rate;
- concurrency behavior;
- measured timestamp/confidence.

### ProviderCapacityProfile

Examples:

- subscription/API availability;
- quota state when observable;
- rate-limit state;
- concurrency slots;
- degraded/outage state;
- estimated monetary cost where applicable.

### PrivacyProfile

Examples:

- local vs external inference;
- provider destination;
- retention/caching characteristics when known;
- policy classification for sensitive projects.

## Routing algorithm

### Phase A — authoritative requirements

Build `RoutingRequirements` from structured task/execution data before inspecting free-form prompt text.

Inputs include:

- task kind;
- task risk;
- required capabilities;
- repository/language/framework;
- context requirement;
- allowed providers/data destinations;
- isolation requirement;
- latency/cost preference;
- verification requirements.

Prompt classification is only a fallback for ad-hoc tasks with insufficient structured metadata.

### Phase B — hard eligibility gates

Reject candidates that fail any non-negotiable constraint:

- missing required capability;
- insufficient context;
- provider blocked by policy/privacy;
- unavailable model/runtime;
- impossible resource requirement;
- quota/capacity unavailable;
- incompatible execution mode.

### Phase C — preference scoring

Only eligible candidates are scored.

Signals can include:

- repository-specific historical success;
- task-kind historical success;
- independent verification/reviewer rejection history;
- external benchmark evidence;
- measured local latency/throughput;
- token/monetary cost;
- remaining quota/capacity;
- context efficiency;
- model residency/load cost;
- reliability;
- user quality/speed/privacy preference.

The scoring policy is itself replaceable/versioned.

### Phase D — route plan

The router returns an explainable ranked structure, not only a model ID:

```text
RoutePlan
  primary
  fallbacks[]
  reasons[]
  hard_constraints_satisfied[]
  rejected_candidates[]
  policy_version
  profile_versions
```

This enables replay, debugging and future learning.

## Repository-specific learning

Generic benchmarks bootstrap decisions; verified outcomes should increasingly dominate where enough local evidence exists.

Track model/runtime performance conditioned on:

```text
repository x task-kind x language/framework x risk-class
```

Candidate signals:

- verified success;
- first-pass success;
- review rejection;
- regression rate;
- duration;
- retries;
- token/cost consumption;
- context footprint;
- human override.

The router learns **routing policy**, not hidden user source-code content.

## Profiling

New local models/runtimes may be profiled through `ModelProfilerPort`.

Profiling must be:

- bounded;
- opt-in/visible when expensive;
- versioned;
- reproducible where practical;
- separated into capability probes, deterministic benchmarks and optional judge-based evaluation.

LLM-as-judge is secondary evidence. Deterministic verification outranks subjective judging whenever available.

## Device resource management

Local model capacity is a scheduling concern with a dedicated subsystem.

`DeviceResourceManagerPort` reports resource observations. Platform adapters may use NVIDIA, ROCm, Intel, Apple or future APIs without leaking platform details into routing business logic.

`ModelResidencyManagerPort` owns:

- loaded/resident model state;
- pinning;
- estimated load/unload cost;
- eviction proposals;
- backend load/unload operations through adapters.

The Model Router only consumes feasibility/cost information.

Resource observations may have confidence levels. Heuristic estimates must not be represented as precise measurements.

## Semantic cache policy

Separate caches by semantics:

### Safe by default

- exact immutable artifact cache;
- embedding cache keyed by content hash + embedding model/version;
- benchmark/profile query cache;
- routing-decision cache keyed by relevant immutable routing inputs;
- deterministic documentation/resource projections keyed by source hashes.

### Restricted

Generated-response caching is disabled for stateful coding/agent work unless all of the following hold:

- operation declares itself pure/cacheable;
- authoritative context fingerprint matches;
- task/plan revision matches;
- source/worktree content hashes match;
- system/policy/tool definitions match;
- provider/model/rendering parameters required by the cache contract match;
- sensitivity/privacy policy permits retention.

Semantic prompt similarity alone is never sufficient for response reuse.

## Failover semantics

Failover depends on side-effect class.

### Pure/read-only inference

Transparent fallback may be allowed.

### Stateful agent execution

A provider/model failure creates or resumes through a **new durable `ExecutionAttempt`** according to recovery policy.

Do not blindly replay previous tool calls or filesystem side effects.

Use:

- worktree state;
- event journal;
- execution checkpoint/context continuity;
- idempotency information;
- policy engine;
- human gate where required.

## Local inference adapters

Ollama, llama.cpp, BitNet-compatible runtimes and future engines remain implementations behind local inference/runtime ports. None becomes part of the domain model.

An OpenAI-compatible endpoint may be exposed as an optional integration adapter, but the internal routing protocol remains richer and typed.

## New invariants

```text
INV-059 Model/provider selection is a replaceable policy subsystem and never part of core Task domain identity.
INV-060 Routing applies hard capability, security, privacy and resource eligibility gates before preference scoring.
INV-061 Structured task metadata outranks prompt-text classification for routing decisions.
INV-062 Global benchmark scores are evidence with provenance, not authoritative universal model quality.
INV-063 Local/runtime measurements and estimated values must be distinguishable and retain hardware/backend/time provenance.
INV-064 Device/VRAM/unified-memory management is isolated behind resource/residency ports and may not leak platform-specific logic into the router domain.
INV-065 Semantic prompt similarity alone may never authorize cached response reuse for stateful agent/coding work.
INV-066 Stateful failover creates a new/recovered execution attempt; side effects may not be transparently replayed across models/providers.
INV-067 Routing decisions are explainable/replayable through versioned RoutePlan inputs, policy and profile references.
INV-068 Verified repository-specific outcomes may inform future routing, but model self-report or self-judging never grants completion authority.
```


---

## ZeroClaw-derived runtime hardening and extension architecture

**Reviewed:** 2026-09-09  
**Reference:** `zeroclaw-labs/zeroclaw`  
**Decision:** **ADAPT / STUDY deeply; do not adopt as a runtime dependency.**

ZeroClaw is valuable to this project because it is a large Rust agent runtime that has already encountered many of the exact concerns our local `agentd` will face: provider abstraction, tool execution, memory/history boundaries, sandboxing, background service operation, context trimming, deterministic procedures, and AI-assisted development of a large Rust workspace.

The most useful lesson is not its product surface. It is the set of runtime contracts and failure modes that appear once an agent host becomes long-lived and extensible.

### 1. Keep extension contracts in a deliberately small API layer

ZeroClaw exposes stable traits for model providers, channels, tools, memory and lower-level runtime surfaces. We adopt the principle, but our core contract surface must remain smaller and aligned with our bounded contexts.

Target shape:

```text
crates/contracts / crates/api
        |
        +-- AgentProviderPort
        +-- RunnerPort
        +-- ToolPort
        +-- MemoryPort
        +-- ResourceParserPort
        +-- BrowserAutomationPort
        +-- ModelRouterPort
        +-- SandboxPort
        +-- ObserverPort

Concrete adapters
        |
        +-- Claude
        +-- Codex
        +-- OpenCode
        +-- NativeRunner
        +-- ContainerRunner
        +-- SQLite
        +-- ...
```

The public contract crate may define capability and message types, but it must not become a dumping ground for application state or provider-specific configuration. Factories/registries compose adapters outside the domain.

### 2. Single Source of Truth is an enforced architecture rule

Before introducing a field, cache, lookup table or duplicated configuration value, the implementing task must name the canonical owner of that fact.

Derived copies must be one of:

- ephemeral materialized views;
- explicitly invalidatable caches;
- immutable snapshots with source revision/hash;
- generated projections that can be rebuilt.

Do not keep parallel mutable representations of the same policy, task state, provider capability, route decision or memory fact.

### 3. Architecture Map for agents and humans

`AGENT.md` stays small and always-loadable. A separate architecture map routes a coding agent to only the documentation relevant to its change.

Planned file:

```text
docs/architecture/MAP.md
```

It maps change surfaces such as:

```text
new provider      -> provider contracts + capability model + provider ADRs
context change    -> Context Engine + continuity + privacy rules
new runner        -> Runner/Sandbox contracts + threat model
memory change     -> Memory lifecycle + provenance + forgetting rules
task state change -> Task state machine + Event Journal + migration rules
UI design change  -> PRODUCT.md + DESIGN.md + design-system contracts
```

This is progressive disclosure for contributors: bootstrap instructions point to a routing map, not to the entire architecture corpus.

### 4. Tool-call / tool-result protocol integrity

History trimming, context compaction, memory promotion and context rollover must preserve protocol validity. A tool call and its corresponding tool result form a correlated unit for provider-visible history unless the provider protocol explicitly supports another representation.

The Context Engine must never produce:

```text
tool_call(id=42)
... result removed ...
```

or a dangling result with no call.

Large tool payloads may be offloaded to content-addressed artifacts, but the retained context envelope keeps:

- tool call id;
- tool name;
- status;
- bounded summary;
- artifact/reference id;
- provenance;
- integrity metadata.

### 5. Tool Execution Receipts

A model can claim that a command/tool ran when it did not. The runtime therefore needs independently verifiable execution evidence.

We adopt a two-layer design.

#### Layer A — live/session receipt

After a successful tool action, the trusted runtime emits a compact authenticated receipt into the provider-visible result envelope. The model cannot mint a new valid receipt.

Possible shape:

```text
ToolExecutionReceipt
- receipt_id
- tool_call_id
- tool_name
- canonical_args_hash
- result_hash
- timestamp
- session_scope
- mac/signature
```

A per-session secret/HMAC is sufficient for detecting fabrication within the active execution. The secret is never placed in model context or logs.

#### Layer B — durable audit evidence

For security-relevant actions, a separate durable audit record is written transactionally with the Event Journal/Audit subsystem. It may use a per-device signing key held by the OS credential store so evidence can survive session rollover and can later be verified against a device identity.

The durable record stores hashes/references rather than unnecessarily duplicating secret/sensitive tool payloads.

**Important semantics:**

```text
receipt == runtime evidence that the call/result occurred
receipt != authorization
receipt != correctness
receipt != safety approval
receipt != task completion
```

Policy Engine authorization happens before execution. Evaluation happens after relevant work. Receipts only attest to execution provenance.

### 6. Separate memory, history, files, logs and usage ledgers

These are different persistence domains and must never be conflated:

```text
Session History  -> provider/conversation continuity
Memory           -> selected durable learned knowledge
Files/Artifacts  -> external or generated content
Event Journal    -> durable application state transitions/evidence
Security Audit   -> privileged/security decisions and actions
Operational Logs -> diagnostics
Usage Ledger     -> tokens/cost/quota/runtime measurements
```

Reading a file does not make the file memory. Logging a tool result does not make it authoritative task state. A session summary does not become long-term knowledge until Memory policy explicitly promotes it.

### 7. Deterministic Procedure Engine complements the adaptive Planner

Not every repeated workflow should be planned by an LLM each time. Predictable procedures should run as deterministic, versioned workflows with explicit triggers, steps, approval gates and resumable state.

Add a future `ProcedureEngine` / `WorkflowEngine` boundary:

```text
Event/Trigger
    -> Procedure Definition
    -> deterministic step
    -> policy/approval gate
    -> durable step checkpoint
    -> next step
```

Use the adaptive Planner when work is ambiguous or decomposition must be discovered. Use deterministic procedures when the workflow is known.

Examples:

- release checklist;
- nightly repository health scan;
- dependency update workflow;
- generate documentation after verified source change;
- run security review after auth-related task;
- re-profile a local model after hardware/runtime change.

Procedure runs must be restartable and idempotency-aware.

### 8. Sandbox backend selection is a security decision, not convenience fallback

Platform-specific isolation remains replaceable behind `SandboxPort` / `RunnerPort`. Candidate implementations may include OS-native sandboxing, containers and later VMs.

A required isolation profile must **fail closed** if its backend is unavailable. It may not silently degrade to unsandboxed native execution.

Example:

```text
Task requires: ContainerOrBetter
Docker unavailable
    -> BLOCKED / REQUIRE USER ACTION
    != NativeRunner fallback
```

Network policy is a separate capability from filesystem isolation. A sandbox backend that only restricts files must not be represented as enforcing network isolation.

### 9. Incremental persistence for background runtimes

`agentd` must durably commit important state throughout execution. A graceful process shutdown must not be the only moment at which state is persisted.

Expected failure semantics:

- already committed task/event/memory state survives a hard process kill;
- an in-flight atomic transaction may be retried/reconciled;
- a task never becomes `COMPLETED` merely because a session exited cleanly;
- recovery uses durable execution/checkpoint/worktree state.

### 10. Runtime performance must be reproducible, but codebase complexity is a separate metric

ZeroClaw markets a very small release binary and low startup/RAM footprint, but its published comparison is project-reported and some values are normalized estimates. The current repository has also grown into a very large multi-crate Rust codebase. Therefore we take two lessons simultaneously:

1. Maintain automated startup/RSS/binary-size benchmarks for `agentd`.
2. Do not use runtime efficiency as an excuse for architecture/codebase bloat.

Our benchmark suite should record:

- binary size by feature set;
- cold startup time;
- idle RSS;
- RSS per active session;
- process spawn latency;
- IPC round-trip latency;
- SQLite/event commit latency;
- context compilation latency;
- sustained multi-agent resource profile.

Marketing may only quote reproducible measurements with platform/build/methodology metadata.

### 11. Architecture fitness budgets for Rust modules/crates

The project must actively detect architectural growth. Suggested default guardrails (generated code excluded):

- one bounded responsibility per crate;
- no concrete adapter imports into domain crates;
- architecture dependency checks in CI using `cargo metadata` or an equivalent custom checker;
- source file > ~800 LOC triggers a review warning;
- source file > ~1500 LOC requires a split or documented architecture exception/ADR;
- giant schema/config modules should be generated or decomposed instead of manually maintained monoliths;
- a feature that crosses multiple bounded contexts requires an explicit integration contract rather than direct internal imports.

LOC limits are maintainability signals, not correctness proofs. The dependency graph and ownership boundaries remain authoritative.

### 12. New invariants from ZeroClaw review

```text
INV-069 Stable extension contracts live in a deliberately small provider-neutral API/contracts layer.
INV-070 Every persistent fact has one canonical owner; caches, snapshots and projections are explicitly derived.
INV-071 Context pruning/compaction preserves provider-valid tool-call/tool-result correlation and provenance.
INV-072 Successful privileged/tool executions can emit runtime-authenticated receipts; a receipt proves execution provenance, not authorization or correctness.
INV-073 Durable security/audit evidence is separate from model-visible session receipt mechanics.
INV-074 Known repeatable workflows prefer a deterministic Procedure Engine over repeated LLM planning.
INV-075 Procedure executions are versioned, checkpointed, resumable and idempotency-aware, with Policy/approval gates.
INV-076 Required isolation fails closed; sandbox backend unavailability may not silently downgrade to unsandboxed execution.
INV-077 Platform-specific sandbox implementations stay behind replaceable Runner/Sandbox contracts and declare the capabilities they actually enforce.
INV-078 Long-running agentd state is persisted incrementally; graceful shutdown is never the sole durability mechanism.
INV-079 AGENT.md remains a compact bootstrap contract; an Architecture Map provides progressive-disclosure routing to detailed source-of-truth docs.
INV-080 Runtime performance claims require reproducible benchmark methodology and hardware/build provenance.
INV-081 Rust crate/module growth is monitored by architecture fitness checks; oversized components require decomposition or an explicit reviewed exception.
INV-082 Adding/replacing a provider, tool, runner, memory backend or channel/integration must not require modifying unrelated domain business logic.
```

### Priority

- **P0:** extension-contract discipline, canonical state ownership, protocol-valid tool result handling, fail-closed isolation.
- **P1:** tool execution receipts, architecture-map workflow, architecture fitness checks.
- **P2:** deterministic Procedure Engine and durable signed audit receipts beyond the initial Event Journal implementation.

---

## Multi-Provider Councils, Workflow Gates and Dispatch Preflight

**Derived from:** Claude Octopus review, 2026-09-09.  
**Decision:** adopt provider-neutral orchestration patterns; reject Claude-centric authority and fixed-majority completion semantics.

### 1. Workflow templates are orchestration policy, not Task lifecycle

A project may use versioned macro workflows such as:

```text
Discover -> Define -> Develop -> Deliver
```

or domain-specific flows such as:

```text
Threat Model -> Implement -> Security Verify -> Human Gate
```

These are expressed as `WorkflowTemplate` / deterministic `ProcedureEngine` definitions that create or coordinate durable Tasks.

They may not redefine the authoritative Task state machine, weaken acceptance criteria, or make a provider/session the owner of workflow state.

### 2. DispatchPreflight

Before a multi-provider, expensive, long-context or privileged workflow begins, build a versioned `DispatchPreflight`.

Possible shape:

```text
DispatchPreflight
  workflow_id
  task_revision
  candidate_seats[]
  required_capabilities[]
  provider_status[]
  estimated_context_tokens
  context_limit_checks[]
  estimated_cost_range
  quota/capacity observations[]
  privacy_destinations[]
  isolation_requirements
  fallback/degradation policy
  warnings[]
  generated_at
```

Preflight estimates are advisory evidence. They do not become billing truth or completion evidence.

### 3. Provider availability is explicit state

Provider/runtime state must be observable through provider-neutral statuses such as:

```text
AVAILABLE
MISSING
DEGRADED
QUOTA_EXHAUSTED
RATE_LIMITED
POLICY_BLOCKED
INCOMPATIBLE
UNKNOWN
```

A workflow defines:

- preferred seats;
- minimum viable seats;
- required verifier/provider classes;
- whether graceful degradation is allowed.

A missing optional research seat may degrade gracefully. A required independent security verifier may fail closed.

### 4. Council Engine

Introduce a future `CouncilEnginePort` for bounded multi-model deliberation.

```text
CouncilRequest
  goal
  evidence_set
  seat_requirements
  diversity_policy
  quorum_policy
  veto_policy
  max_rounds
  budget
  synthesis_policy
```

Supported goals may include:

```text
ADVICE
DECISION
PLAN_REVIEW
ARCHITECTURE_REVIEW
SECURITY_REVIEW
RISK_ASSESSMENT
```

Councils do not own implementation state. If an approved plan should be implemented, it creates/updates durable Tasks through normal Planner/Reconciler contracts.

### 5. Preserve disagreement

`CouncilResult` must retain material disagreement instead of collapsing everything into one confident summary.

```text
CouncilResult
  consensus_findings[]
  minority_findings[]
  critical_objections[]
  unresolved_uncertainty[]
  evidence_refs[]
  seat_contributions[]
  quorum_status
  veto_status
```

The synthesizer is allowed to organize arguments, not erase dissent.

### 6. Quorum and evidence-backed critical veto

Do not use a universal fixed percentage such as `75% = safe`.

Gate policy depends on risk and evidence class.

Examples:

```text
Architecture advisory:
  quorum = 3/4
  no hard veto

Security-sensitive auth change:
  required = independent security verifier
  confirmed critical vulnerability = veto

Deterministic test failure:
  veto independent of council vote
```

A veto must identify the rule/evidence that grants it authority. A model's unsupported concern is not automatically a blocking veto.

### 7. Diversity Policy

Multi-model review must distinguish nominal seat count from useful independence.

`DiversityPolicy` can require combinations of:

- different provider families;
- different model families;
- independent role prompts;
- blinded review;
- independently constructed context packages;
- different evaluation methods.

Two models from the same vendor may add value, but they do not satisfy a cross-vendor independence requirement unless policy explicitly allows it.

### 8. Contribution attribution

Every aggregate multi-agent artifact retains who actually contributed.

```text
ProviderContribution
  seat_id
  provider_family
  model
  role
  execution_attempt
  status
  evidence_refs
  token/usage_ref
```

The UI may only label work `multi-model` / `cross-provider verified` if the corresponding contribution policy was actually satisfied.

### 9. Reaction Policy

External lifecycle events feed the Event Journal and Reconciler, not hidden shell loops.

Examples:

```text
CI_FAILED
REVIEW_CHANGES_REQUESTED
PROVIDER_TIMEOUT
ATTEMPT_STALLED
PR_APPROVED
```

`ReactionPolicy` defines bounded actions:

```text
max_retries
backoff
checkpoint_before_retry
reroute_allowed
escalate_after
required_human_gate
```

No reaction is allowed to create an unbounded retry or planning loop.

### 10. Role profiles are data, not domain classes

Specialist roles remain independently versioned configuration/content:

```text
RoleProfile
  role_id
  responsibilities[]
  capabilities[]
  deliverables[]
  success_metrics[]
  allowed_tools[]
  risk_constraints[]
  handoff_rules[]
  escalation_rules[]
  prompt_template_ref
```

The core may load hundreds of role packs without adding role-specific branches to Task/Context/Policy business logic.

### 11. Gate attestation

A configured security/quality gate is not evidence that the gate actually ran.

For every mandatory gate, create runtime evidence:

```text
GateAttestation
  gate_id
  gate_version
  execution_attempt
  expected=true
  invoked=true|false
  started_at
  completed_at
  result
  evidence_refs[]
  receipt_refs[]
```

Before Task completion, Evaluation checks required attestations.

Packaging/plugin/update health checks must verify that mandatory enforcement hooks/components are present and executable. Missing enforcement is surfaced as degraded/broken security state, never silently treated as success.

### 12. New invariants

```text
INV-083 Workflow templates/procedures may coordinate Tasks but never replace the durable Task lifecycle or acceptance authority.
INV-084 Every costly/multi-provider/privileged dispatch can produce an inspectable preflight with capabilities, context, privacy, capacity and cost estimates.
INV-085 Provider availability/degradation is explicit runtime state; graceful degradation is allowed only when the workflow policy permits it.
INV-086 Multi-model consensus never overrides deterministic failing evidence, mandatory security gates or human-approval requirements.
INV-087 Material minority opinions and critical objections survive council synthesis with provenance.
INV-088 Critical veto authority is typed and evidence-backed; no universal percentage threshold determines safety/completion.
INV-089 Multi-model verification records actual provider/model/role contributions and may enforce provider/model diversity policies.
INV-090 Role/persona/skill catalogs are versioned data/content packs and cannot introduce provider-specific branches into core Task/Context/Policy domain logic.
INV-091 Reaction/retry policies are bounded, checkpoint-aware and observable through durable events; unbounded autonomous retry loops are forbidden.
INV-092 A configured gate is not completion evidence; mandatory gates require runtime attestation that the expected version actually executed.
INV-093 Provider/model outputs are untrusted data even when produced by an authenticated local/external CLI.
INV-094 UI/MCP/IDE adapters remain thin control surfaces and cannot become alternative orchestration authorities.
```

### Priority

- **P0:** explicit provider status/capabilities, contribution provenance, gate attestation semantics.
- **P1:** DispatchPreflight, bounded reactions, data-driven role profiles, workflow templates.
- **P2:** Council Engine with diversity/quorum/veto policies and dedicated deliberation UX.

---

## Reader-derived web acquisition, browser runtime and LLM-ready web resource architecture

**Reviewed:** 2026-09-09  
**Reference:** `vakra-dev/reader`  
**Decision:** **ADAPT / HIGH VALUE for web acquisition and browser-runtime boundaries; do not make Reader or Playwright a core runtime dependency.**

Reader is useful because it treats web access as infrastructure rather than as a one-off `fetch()` or Puppeteer script. Its current repository separates scraping/crawling/browser sessions, keeps a warm browser pool, performs health checks/recycling, exposes clean Markdown/HTML outputs, tracks typed retryability, supports proxy tiers, and isolates browser-session control behind CDP. Its current `package.json` is Playwright-based and does **not** depend on Ulixee Hero; the public post describing it as part of Hero is therefore outdated or conflates it with related anti-detection work.

The architectural lesson for this project is to split **web acquisition**, **browser automation**, and **resource normalization** into separate replaceable boundaries.

### 1. Web access is not part of the Context Engine

The Context Engine selects and renders already-addressable evidence. It must not own HTTP fetching, browser lifecycle, crawling or anti-bot logic.

Target flow:

```text
Task / Tool Request
        |
        v
Policy Engine / Network Access Policy
        |
        v
Web Acquisition
  |        |         |
  |        |         +--> BrowserAutomationPort (interactive stateful session)
  |        +------------> CrawlPort / discovery
  +---------------------> WebFetchPort / document fetch
        |
        v
Raw Web Artifact + provenance
        |
        v
Resource Engine
  parse -> sanitize -> semantic segment -> representations
        |
        v
Resource Store / indexes
        |
        v
Context Engine
```

This ensures the browser backend can change independently from resource parsing, context retrieval or agent orchestration.

### 2. Distinguish three web primitives

Do not reduce all web work to one `browser()` abstraction.

#### `WebFetchPort`

For obtaining a bounded representation of a known URL/document.

Possible result contract:

```text
WebFetchResult
- request_url
- final_url
- status
- headers_summary
- retrieved_at
- source_origin
- transport/backend
- raw_artifact_ref
- content_type
- content_hash
- cache/freshness metadata
- redirect_chain_ref
- policy_decision_ref
- extraction_hint
```

#### `CrawlPort`

For bounded link discovery. A crawl request must include explicit ceilings such as:

- max pages;
- max depth;
- allowed origins/domains;
- allowed schemes;
- total byte/time budget;
- concurrency limit;
- per-origin rate policy.

The crawl graph is discovery evidence, not automatically trusted context.

#### `BrowserAutomationPort`

For stateful browser sessions/actions that require rendering or interaction. It exposes capabilities rather than Playwright/CDP-specific types to core business logic.

Potential capabilities:

```text
navigate
read_dom
read_accessibility_tree
extract_text
screenshot
click
type
wait
execute_restricted_browser_action
```

CDP, WebDriver BiDi, Playwright, Lightpanda or a remote browser service are adapter choices, not domain concepts.

### 3. Browser process pooling is adapter/runtime infrastructure

Reader demonstrates useful operational patterns: warm daemon mode, bounded tabs, load-aware selection, proactive recycling after page/idle thresholds, health probing, and replacement of unhealthy browser processes.

Adopt the lifecycle concept, but keep it behind a `BrowserRuntimePort`/adapter-internal pool:

```text
BrowserRuntime
  launching -> healthy -> draining -> recycled/closed
```

The domain may observe health/capacity but may not manipulate Chrome process internals.

Browser pools must expose:

- capacity;
- active/pending session counts;
- health;
- backend/version;
- isolation profile;
- warm/cold state;
- last fault/recycle reason.

Browser crashes/recycling must not mutate durable Task state directly.

### 4. Web/browser execution requires an explicit isolation profile

Arbitrary web content is hostile input. Browser processes therefore have a stronger threat model than ordinary local resource parsing.

Preferred execution profiles:

```text
Read-only public fetch      -> restricted network worker
Interactive browser session -> container/VM or hardened browser worker
Authenticated private site  -> explicit user grant + scoped credential broker
```

The browser worker must not inherit unrestricted access to:

- repository secrets;
- `~/.ssh` / cloud credentials;
- unrelated local files;
- arbitrary IPC endpoints;
- the Tauri privileged host.

A stateful web session is its own capability boundary. Browser cookies/storage/session tokens are credentials and must follow Secret Broker/retention policy.

### 5. Network policy is checked before every acquisition and redirect

Reader checks `robots.txt`, classifies some failures as non-retryable and has responsible-use language, but our product needs a stronger centralized policy boundary.

`NetworkAccessPolicy` / Policy Engine must evaluate at minimum:

- URL scheme;
- normalized hostname;
- resolved IP ranges;
- redirect destination at every hop;
- loopback/link-local/private/internal destinations;
- user/domain allow/deny rules;
- authenticated-session scope;
- robots/compliance mode where applicable;
- crawl depth/page/byte/time budgets;
- provider/browser capability;
- workspace/org policy.

This is also the SSRF boundary for future cloud/remote workers. URL validation is insufficient if DNS/redirect resolution can later reach a forbidden network target.

### 6. Anti-detection/stealth is a replaceable capability, never a product invariant

Reader exposes fingerprint injection, proxy tiers and anti-detection behavior. We may support compatible browser backends that advertise a `StealthProfile` capability, but core orchestration must **not** require bypassing website protections.

Rules:

- no security/policy promise depends on stealth succeeding;
- stealth implementation is adapter-specific;
- user/org policy can disable it globally;
- site access still obeys legal/contractual and network policies;
- retries/escalation cannot become an unbounded attempt to defeat access controls;
- provenance records which acquisition backend/profile was used.

This keeps Browser Automation usable for testing, documentation and public web research without tying the product identity to anti-bot bypass.

### 7. Retrieval and extraction are separate stages

Reader returns raw HTML, cleaned HTML and Markdown, and falls back to plain-text stripping if its Markdown converter fails. We adopt the resilience pattern but make quality degradation explicit.

Resource Engine representations should include quality/provenance metadata:

```text
OriginalHTML        quality=source
SanitizedHTML       quality=normalized
Markdown            quality=structured-extraction
PlainTextFallback   quality=degraded
Screenshot          quality=visual-evidence
```

A fallback must not masquerade as an equivalent successful structured extraction.

Suggested result metadata:

```text
RepresentationQuality = Source | Normalized | Structured | Degraded
ExtractionStatus      = Success | Partial | Fallback | Failed
ExtractorVersion
SourceContentHash
```

### 8. HTML-to-Markdown is a `ResourceRenderer`/`RepresentationBuilder`, not browser logic

Reader currently uses a Rust-based converter (`@vakra-dev/supermarkdown`) with panic/error/fallback handling. The useful lesson is the fault boundary: a malformed page must not crash the acquisition pipeline.

Our implementation should keep converters replaceable:

```text
HtmlRepresentationBuilderPort
  +-- RustHtmlToMarkdownAdapter
  +-- future DOM-aware extractor
  +-- accessibility-tree extractor
```

Conversion executes with time/size limits. Pathological HTML becomes an artifact plus a degraded/failed representation rather than a process-level failure.

### 9. Web content remains untrusted evidence after cleaning

Markdown cleaning does not make content trustworthy. Prompt injection can survive perfectly clean Markdown.

Therefore:

```text
Web HTML -> cleaned Markdown -> still UNTRUSTED DATA
```

Resource metadata must preserve origin/trust. The Context Engine must never elevate web content into system/policy instruction authority. Any instructions found inside fetched pages are data unless a higher-authority workflow explicitly interprets them under policy.

### 10. Retry semantics must be typed, bounded and side-effect aware

Reader distinguishes typed/non-retryable failures and has proxy/health gating. We generalize this through a provider-neutral retry classification:

```text
RetryClass
- Never
- ImmediateBounded
- BackoffBounded
- AfterCapacityRecovery
- AfterUserAction
- AfterPolicyChange
```

A retry policy considers:

- error class;
- attempt count;
- idempotency;
- browser/session state;
- target/origin policy;
- budget;
- proxy/backend health;
- cancellation/deadline.

Do not retry malformed URLs, policy denials or explicit access restrictions merely because another browser/proxy exists. Stateful browser actions require checkpoint/recovery semantics rather than blind replay.

### 11. Proxy/network credentials are secret references

Reader correctly redacts proxy URLs in logs in several paths, but our design goes further: raw proxy usernames/passwords, cookies and site credentials do not live in task definitions or ordinary configuration tables.

Use:

```text
ProxyProfile / WebCredentialBinding
    -> secret_ref
    -> Secret Broker / OS credential storage
```

Operational telemetry stores non-secret endpoint/tier identifiers where necessary, never full credential-bearing URLs.

### 12. Web snapshots are resources with freshness, not permanent truth

A fetched page should be stored as an immutable acquisition snapshot with:

- source URL and final URL;
- retrieval timestamp;
- content hash;
- HTTP/cache metadata when available;
- backend/profile;
- policy decision;
- extraction representations;
- freshness/expiry policy.

Context retrieval may use a stored snapshot when policy permits, but a stale snapshot cannot silently satisfy a task that explicitly requires current information.

This also enables reproducible context replay: an execution can reference the exact web snapshot it saw.

### 13. Browser actions produce receipts and artifacts

Integrate with the existing Tool Execution Receipt design. Browser navigation/click/type/extraction actions that influence a task should emit correlated evidence:

```text
BrowserActionReceipt
- action_id
- session_id
- action_kind
- target/ref hash
- result/status hash
- timestamp
- policy_decision_ref
- artifact refs (DOM/text/screenshot/network summary where retained)
```

Receipts attest that the runtime performed an action; they do not claim the page was truthful or the action was authorized beyond the referenced Policy decision.

### 14. Optional Browser/Web service remains replaceable

We do **not** move Chromium/Playwright/Node into the Rust domain core. Implementation options may include:

- Rust-native HTTP for simple fetches;
- a separate browser sidecar/service;
- Playwright adapter;
- Lightpanda adapter;
- remote browser worker;
- a hosted acquisition provider.

`agentd` talks only to ports/contracts. This preserves the ability to replace the browser stack without changing Task, Context, Memory, Resource or Policy business logic.

### 15. New invariants from Reader review

```text
INV-095 Web acquisition, browser automation, resource normalization and context selection are separate bounded responsibilities.
INV-096 Browser/CDP/Playwright-specific types never enter core Task/Context/Memory domain contracts.
INV-097 Every web acquisition and redirect hop is subject to centralized network/SSRF/policy evaluation before access.
INV-098 Cleaned Markdown, extracted text and browser DOM remain untrusted external evidence; cleaning never elevates instruction authority.
INV-099 Browser sessions/cookies/auth storage are scoped credentials/capabilities and may not inherit unrestricted host secrets or filesystem access.
INV-100 Extraction fallbacks are explicitly marked degraded/partial and never silently treated as equivalent structured representations.
INV-101 Web retries/crawls are bounded by typed retry policy, time/page/byte/concurrency budgets and idempotency/session semantics.
INV-102 Anti-detection/stealth is an optional adapter capability governed by policy, never a core requirement or authorization bypass mechanism.
INV-103 Fetched web content is stored/referenced as provenance-bearing snapshots with explicit freshness semantics when reproducibility or reuse matters.
INV-104 Browser actions that materially affect execution can emit correlated receipts/artifacts and remain auditable independently of model claims.
INV-105 Browser process pooling/health/recycling is runtime adapter infrastructure and cannot become an alternative owner of durable Task state.
INV-106 Web/proxy/site credentials are represented by secret references and never embedded in ordinary task/config/event/log payloads.
```

### Priority

- **P0:** network/SSRF policy boundary, untrusted-web-content rule, web/browser/resource separation, secret-reference rules.
- **P1:** `WebFetchPort`, `BrowserAutomationPort`, typed web results/snapshots, extraction quality metadata, bounded retry model.
- **P2:** browser worker/pool, crawling, remote browser backend, optional stealth/proxy capabilities, browser-action receipts.


---

## AI-assisted development methodology hardening (AI Code Guide review)

Source reviewed: `automata/aicodeguide` (MIT). This source is primarily a methodology/roadmap rather than a runtime implementation. We adopt the principles that strengthen specification-driven, modular and verifiable AI development, while rejecting practices that conflict with our Context, Privacy and Security architecture.

### 1. Specification precedes implementation, but PRD is not the sole source of truth

A useful project/feature specification is required before implementation, but a monolithic PRD must not become an overloaded document containing product intent, architecture, implementation detail and execution state.

Use a layered specification model:

```text
Product Intent / Problem
        ↓
Requirements + Non-Functional Requirements
        ↓
Architecture Constraints / ADRs / Invariants
        ↓
Feature / Change Specification
        ↓
Durable Task DAG
        ↓
Execution Envelopes
        ↓
Verification Evidence
```

For every non-trivial feature/change, preserve traceability:

```text
Requirement ID
   ↓
Task IDs
   ↓
Acceptance Criteria
   ↓
Test / Evaluation IDs
   ↓
Evidence / Artifact IDs
```

A requirement is not considered delivered merely because implementation tasks are marked complete. Verification must establish that the requirement's acceptance criteria are satisfied.

### 2. Feature-level "mini-specs" are first-class and versioned

Large project specifications should not be injected into every execution. Each feature/change may have a bounded `ChangeSpec` containing:

- objective and user/product outcome;
- relevant requirement IDs;
- in-scope and out-of-scope behavior;
- architectural constraints/invariants;
- acceptance criteria;
- risk/security/privacy classification;
- migration/compatibility constraints where applicable;
- dependency references;
- verification requirements.

The `ChangeSpec` is versioned. Tasks reference a concrete spec revision. If the spec changes materially during implementation, the Planner/Reconciler must create a new revision and explicitly reconcile affected tasks rather than silently changing their meaning.

### 3. Requirement-to-code traceability is more valuable than raw prompt history

The guide recommends keeping prompt/chat history as a record of design intent. We adapt the intent but reject raw prompt logs as an authoritative or default version-controlled project record.

Our canonical design-intent trail is structured:

```text
Intent / Requirement
    ↓
Decision / ADR
    ↓
Plan Revision
    ↓
Task Revision
    ↓
Execution / Tool Events
    ↓
Verification Evidence
```

Raw model transcripts may be retained locally according to user privacy/retention settings, but:

- they are not authoritative project state;
- they are not automatically committed to Git;
- secrets/PII/tool payloads must not be preserved merely for historical convenience;
- important reasoning outcomes must be promoted into typed Decisions/ADRs/annotations with provenance;
- Context Continuity may search transcript history, but correctness-critical state must survive independently of it.

This provides the useful "why did we do this?" history without turning uncontrolled conversation logs into permanent source code artifacts.

### 4. Planning and implementation are separate authorities

The methodology reinforces "plan first, code second". In our system this becomes a hard separation:

```text
Planner / Spec Compiler
      ↓ proposed work
Durable Task Graph
      ↓ authorized execution
Implementation Agent
      ↓ submitted artifacts
Evaluation Engine
```

Implementation agents cannot silently reinterpret product requirements, broaden scope, or rewrite the task graph to match their implementation. They can propose changes/blockers through typed contracts.

### 5. Engine/domain logic must remain separate from policy

The guide explicitly recommends separating engine code from policy code. We adopt this strongly.

Examples:

```text
Mechanism / Engine                     Policy
────────────────────────────────────────────────────────
DAG mutation primitives                when replanning is allowed
Model route candidates                 which model/risk preference wins
Sandbox capability detection           required isolation by task risk
Retry executor                          which failures are retryable
Context renderer                        which context policy is selected
Workflow executor                       which gates a release requires
```

Mechanisms expose capabilities and deterministic operations. Policies decide when/how they are used. Provider adapters, UI code and persistence adapters may not embed product policy as convenience conditionals.

### 6. Test-first where useful; verification is always required

TDD is encouraged for bounded behavior with stable interfaces, but the architecture does not mandate one ceremony for every task. The invariant is stronger: **implementation is never accepted without independent verification appropriate to risk**.

Verification strategies may include:

- unit tests;
- property-based tests;
- contract tests;
- integration tests;
- state-machine/model-based tests;
- fuzzing;
- security/static analysis;
- performance/regression benchmarks;
- independent model review;
- human approval.

Property-based testing is especially valuable for parsers, state machines, planners, serialization, policies, retry logic, path/network validation and other domains with combinatorial edge cases.

### 7. Test Integrity: agents must not "solve" verification by weakening it

AI agents can make incorrect implementations appear successful by changing tests, fixtures, thresholds or evaluation configuration. Therefore test changes are governed separately from product implementation.

Each task defines a `TestMutationPolicy` such as:

```text
Forbidden
AllowedInScopedFiles
RequiresIndependentReview
RequiresHumanApproval
```

Critical acceptance tests, security policy tests and evaluator rules should be owned outside the ordinary implementation scope where practical.

Evaluation records must note whether the implementation agent changed any tests used to approve its own task.

A passing test suite is insufficient evidence when the same agent weakened the expected behavior without authorization.

### 8. Small tasks remain the execution unit

The guide's task-level prompting aligns with our atomic task model. A task must be small enough that:

- its objective is unambiguous;
- its affected bounded context is known;
- allowed paths/contracts can be specified;
- verification can complete in bounded time;
- failure/retry does not require reconstructing the whole project;
- an independent reviewer can understand the diff.

Do not decompose merely to maximize task count. Decomposition exists to improve isolation, dependency clarity, verification and recovery.

### 9. Existing large repositories are never blindly packed into the model context

The guide suggests tools such as repomix/files-to-prompt for existing codebases. This can be useful manually for small repositories but is not our product architecture.

For large/long-lived projects:

```text
Repository / Resources
       ↓
Resource Engine + indexes
       ↓
Context Planner
       ↓
Just-in-time structural retrieval
       ↓
Smallest sufficient working set
```

Full repository packing is permitted only as an explicit bounded fallback when it fits budget, privacy policy and the task genuinely benefits from it. It is never the default.

### 10. Provider-specific rules are rendered from canonical project policy

`AGENTS.md`, `CLAUDE.md`, Cursor rules and other provider/harness files are **derived compatibility views** rather than independent sources of project rules.

```text
Canonical Project / Agent Policy
        ↓
ProviderRuleRendererPort
        ├── AGENTS.md
        ├── CLAUDE.md
        ├── Cursor rules
        └── future harness formats
```

Generated/installable views carry source revision/hash and drift state. Human edits are never silently overwritten; drift is surfaced and must be reconciled.

### 11. New invariants from AI Code Guide review

```text
INV-107 Product intent, requirements, architecture constraints, task execution and verification are distinct layers; a single PRD may not silently own all of them.
INV-108 Every material requirement/change must be traceable to the tasks and verification evidence claiming to satisfy it.
INV-109 Tasks reference a concrete versioned ChangeSpec/requirement revision; material spec changes require explicit reconciliation.
INV-110 Raw prompt/chat history is optional historical evidence, not authoritative project state and not automatically version-controlled.
INV-111 Important design intent from conversations must be promoted into typed decisions/specifications/ADRs with provenance rather than depending on transcript recall.
INV-112 Mechanism/engine code exposes capabilities; mutable business/security/routing/workflow policy is owned by explicit policy components and cannot be hidden in adapters/UI.
INV-113 Verification is mandatory, but its technique is risk/context dependent; TDD is a method, not the source of truth for completion.
INV-114 An implementation agent may not weaken tests/evaluators/acceptance criteria used to approve its own task unless the task explicitly authorizes that change and independent review validates it.
INV-115 Whole-repository prompt packing is an explicit bounded fallback; default context construction uses structural just-in-time retrieval.
INV-116 Provider-specific agent/rules files are derived compatibility views of canonical project policy and must support provenance/drift detection.
```

### Priority

- **P0:** requirement/task/verification traceability, ChangeSpec revisioning, Test Integrity, mechanism-vs-policy boundaries.
- **P1:** structured design-intent journal, provider-rules renderer/drift detection, property/model-based test strategy.
- **P2:** tooling to visualize requirement → task → artifact → verification traceability and compare AI-assisted development workflows empirically.

---

## Document ingestion, multimodal extraction, and OCR

This section incorporates lessons from `getomni-ai/zerox` while keeping document processing replaceable, sandboxed, provider-neutral, and subordinate to the Resource Engine.

### 1. Document processing is a strategy pipeline, not "OCR everything"

Vision-first OCR is valuable for scanned pages, complex visual layouts, charts, handwriting and documents whose logical structure cannot be recovered reliably from the source format. It must **not** be the default for every document.

Preferred acquisition order:

```text
Document Input
    ↓
Type / MIME Detection
    ↓
Document Strategy Planner
    │
    ├── Native structural parser      # DOCX/XLSX/PPTX/HTML/XML/etc. when reliable
    ├── PDF text/layout parser        # born-digital PDFs
    ├── Region/table/image extractor  # targeted multimodal extraction
    └── Raster + Vision OCR           # scanned/visual/degraded fallback
    ↓
Normalized Resource + representations
```

Reasons:

- rasterizing structured documents destroys some source semantics;
- vision inference is slower and potentially expensive;
- native structure may preserve formulas, cell boundaries, headings, hyperlinks, notes and metadata better;
- some documents genuinely require visual reasoning, so native parsing alone is also insufficient.

The strategy is selected by policy/capability/quality evidence and may fall back progressively.

### 2. New replaceable document-processing ports

Document-specific mechanics stay outside Resource/Context domain semantics.

```text
DocumentTypeDetectorPort
DocumentParserPort
DocumentRendererPort
VisionExtractionPort
StructuredExtractionPort
DocumentWorkerPort
```

Illustrative contract direction:

```text
Resource Engine
     ↓
DocumentProcessingPolicy
     ↓
Document* ports
     ↓
Adapters
 ├── native OOXML parser
 ├── PDF layout parser
 ├── LibreOffice converter
 ├── Poppler/Ghostscript renderer
 ├── local vision model
 ├── OpenAI/Gemini/Anthropic vision
 └── future OCR/document-AI backend
```

No LibreOffice, Poppler, Ghostscript, provider SDK or image library type may leak into canonical Resource/Task/Context models.

### 3. Document conversion is an isolated security boundary

User/imported documents are untrusted binary inputs. Native document converters and renderers are high-risk parsers and must not execute with the same ambient authority as `agentd`.

```text
agentd
  ↓ typed job
Document Worker
  ↓ sandbox / container / restricted process
Converters + parsers
  ↓
Artifacts
```

Minimum rules:

- no shell interpolation for filenames, URLs, extensions or converter arguments;
- execute fixed binaries with argument arrays (`execve`-style), never constructed shell command strings;
- determine format using bounded allowlists and content/MIME inspection rather than trusting user filename extension;
- canonicalize all paths before use;
- worker filesystem access is limited to an owned per-job input/output root;
- network access is disabled for conversion workers unless the selected adapter explicitly needs it;
- remote document URLs are fetched by Web Acquisition under SSRF/network policy **before** document conversion;
- converters never receive arbitrary credentials;
- resource limits cover CPU, memory, disk, output file count, page count and wall time;
- crash/timeout becomes typed failure, not partial silent success.

### 4. Temp workspace ownership is explicit

Each document job receives a unique application-owned workspace:

```text
<app-runtime>/document-jobs/<job-id>/
    input/
    pages/
    derived/
    output/
```

Cleanup may delete only a directory carrying the expected ownership marker/job identity underneath the configured application-owned root.

Never recursively clear an arbitrary user-provided `tempDir` merely because it was passed as configuration.

Cleanup is idempotent and audit-visible when policy requires it.

### 5. Page-level durability and resumability

Large documents must not become one all-or-nothing LLM call.

```text
Document Job
 ├── Page/Unit 1  VERIFIED
 ├── Page/Unit 2  VERIFIED
 ├── Page/Unit 3  FAILED/RETRYABLE
 └── Page/Unit 4  PENDING
```

Persist per-unit status, source hash, renderer version, model/provider, prompt/schema revision, token/cost accounting, extraction quality and artifact references.

On timeout/provider failure, resume from incomplete units rather than repeating the entire document. Aggregation is deterministic over durable unit results.

### 6. Cross-page continuity is bounded state

Tables, paragraphs and layouts can span page boundaries. A serial pipeline may use bounded continuity state, but must not grow by repeatedly injecting every prior page.

```text
Page N result
    ↓
Continuity Extractor
    ↓
small CarryState
    ├── open table headers
    ├── continuation hints
    ├── section identity
    └── formatting constraints
    ↓
Page N+1
```

Carry state is derived, size-bounded, versioned and reproducible from page evidence where possible.

### 7. Multiple document representations are preserved

A document may have multiple useful representations simultaneously:

```text
Original binary
Rendered page image
Native structural tree
Plain text
Layout-aware text
Markdown
Tables / cells
Figures / image crops
Model-optimized representation
```

No single Markdown output becomes the canonical document truth.

Each representation stores:

- source resource/unit ID;
- producer/parser/renderer version;
- source/content hash;
- quality/confidence;
- structural confidence;
- provenance;
- sensitivity/trust classification;
- generation timestamp;
- model/provider and prompt revision when AI-generated.

### 8. Structured extraction is schema-constrained derived evidence

JSON Schema is the canonical contract for structured extraction requests/results at model boundaries.

```text
Resource/Page Evidence
      ↓
StructuredExtractionPort
      ↓
JSON candidate
      ↓
Schema validation
      ↓
Extraction Result + provenance
```

A schema-valid result is not automatically factually correct. High-risk extraction can require:

- field-level evidence references;
- page/region provenance;
- confidence thresholds;
- deterministic validators;
- second-model or independent extraction comparison;
- human review.

Extraction policy may choose between:

- native-structure → structured extraction;
- normalized Markdown/text → structured extraction;
- direct image → structured extraction.

The selection should be benchmarked by document type/model rather than fixed globally.

### 9. Extraction quality and failure are explicit

Never collapse all outcomes into `content: string`.

Example quality/state dimensions:

```text
ExactNative
StructuredParsed
VisionExtracted
OCRFallback
DegradedText
Partial
Failed
```

Per-unit failure policy may be strict or best-effort, but missing pages/failed extractions must remain visible in the aggregate result and UI.

### 10. Provider routing for document work is independent

Vision/document extraction is a capability consumed through the Adaptive Model Router, not hard-coded to one vendor.

Eligibility can consider:

- vision capability;
- document/image limits;
- structured-output support;
- privacy/data-residency policy;
- page/image size;
- cost/token budget;
- local vs external processing;
- historical accuracy on the same document class;
- latency/capacity/quota.

A local vision backend can be preferred for sensitive documents when policy permits and measured quality is adequate.

### 11. Cost, concurrency and backpressure are first-class

Document work can fan out into hundreds of page-level model calls. The scheduler therefore enforces:

- maximum concurrent pages per job/provider/device;
- document-level token/cost/time budgets;
- rate-limit-aware queues;
- bounded retries;
- provider backpressure;
- cancellation that stops future units without corrupting completed state.

Preflight should estimate pages, rendering cost, expected external data transfer and model usage before expensive processing when feasible.

### 12. Privacy UX for documents

Before external vision inference, the user can inspect which document/pages/derived images will leave the device and to which provider.

Example:

```text
Document: contract.pdf
Pages selected: 1-12
Rendering: local sandbox
Vision inference: Gemini (external)
Structured extraction: local model
Raw document sent to our cloud: NO
Rendered pages sent externally: YES, 12
Retention/caching: provider capability/policy display
```

This information derives from the actual execution plan, not marketing text.

### 13. New invariants from Zerox review

```text
INV-117 Vision OCR is a document-processing strategy/fallback, not the canonical ingestion path for every structured document.
INV-118 Document parsing/conversion/inference implementations are adapters behind document-processing ports; their library/provider types may not leak into Resource/Task/Context domain models.
INV-119 Untrusted document conversion/rendering executes in a restricted document worker with explicit resource and filesystem/network boundaries.
INV-120 User-controlled filenames, URLs, MIME declarations and extensions must never be interpolated into shell commands; native converters are invoked with fixed executable + argument arrays.
INV-121 Remote document URLs are acquired through the centralized Web Acquisition/SSRF policy before conversion; document parsers may not perform arbitrary network fetches themselves.
INV-122 Temporary document workspaces are unique and application-owned; cleanup may never recursively delete arbitrary user-selected paths.
INV-123 Large document processing is page/unit durable and resumable; one failed provider call must not require repeating already verified units.
INV-124 Cross-page continuity uses bounded derived carry-state rather than unbounded prior-page transcript accumulation.
INV-125 Markdown/OCR text is a derived representation with provenance and quality, never the sole canonical representation of the document.
INV-126 Schema-constrained extraction must be structurally validated and remain evidence-bearing derived data; schema validity alone is not factual verification.
INV-127 Failed/partial/degraded document units remain explicit in aggregate state and may not be silently omitted.
INV-128 Document model/provider selection is capability/policy/evidence driven and independent from document domain semantics.
INV-129 Document jobs enforce bounded concurrency, retries, token/cost/time/disk budgets and cancellation/backpressure semantics.
INV-130 External document inference must be privacy-observable: execution plans identify what representations/pages leave the device and which provider receives them.
```

### Priority

- **P0:** safe Document Worker boundary, URL/path/command/temp-dir safety, typed document job/unit state.
- **P1:** native-first strategy planner, per-page durability/resume, multiple representations, schema extraction with provenance.
- **P2:** adaptive OCR/extraction benchmarking, cross-page carry-state, local vision backends, advanced table/figure/region extraction.


## Security Verification, Exploit Validation and Release Gates (Shannon-derived)

### Why this exists

Security review must not be a single LLM opinion or a passive scanner result. For high-risk web/API changes, the system needs a structured path from source-informed hypotheses to isolated dynamic verification, evidence, reconciliation and release policy.

The design is inspired by Shannon's separation of source analysis, reconnaissance, category-specific vulnerability analysis, exploitation, deduplication/reconciliation and evidence-backed reporting, while avoiding Shannon-specific runtime/provider/licensing coupling.

### 1. Security verification is a separate bounded capability

Security verification is not implemented inside ordinary coding agents or the Task Engine.

```text
Task / ChangeSpec
      │
      ▼
SecurityVerificationPort
      │
      ▼
Security Verification Engine
      │
      ├── Static/structural analysis adapters
      ├── Recon adapters
      ├── Dynamic exploit adapters
      ├── Finding reconciliation
      ├── Evidence validation
      └── Report/SARIF adapters
```

The engine consumes project/task/resource/runtime evidence and emits typed findings/evidence. It does not own source-code mutation, provider selection, browser implementation or CI integration.

### 2. Separate hypothesis, risk and proven exploit states

Do not collapse every security signal into a boolean vulnerable/not-vulnerable result.

Suggested lifecycle:

```text
ObservedSignal
   ↓
SecurityHypothesis
   ↓
Candidate
   ├── StaticRiskOnly
   ├── ExploitQueued
   ├── ExploitBlocked
   ├── ExploitFailed
   └── ExploitProven
```

A live proof materially increases confidence, but failure to exploit does **not** automatically mean the code is safe. Some important classes (unsafe configuration, vulnerable dependencies, weak cryptography, latent authorization paths, race conditions, environment-specific flaws) may not be reproducible in the current test harness.

Therefore keep at least two externally visible channels:

- `AnalyticalRisk` — evidence-backed static/structural/security reasoning, not live-proven;
- `ProvenExploit` — reproduced against the authorized test target with evidence.

Release policy can weigh these differently.

### 3. Reconciliation before exploitation

Static/code analysis and runtime reconnaissance can discover overlapping candidates. Before expensive or destructive dynamic testing:

```text
Static candidates ───────┐
                         ├──► Finding Reconciler ─► Exploit Queue
Runtime/recon candidates ┘
```

Reconciliation should:

- normalize vulnerability class and affected asset;
- merge duplicate evidence without losing provenance;
- preserve conflicting observations;
- group related attack paths;
- prioritize candidates using risk, reachability, confidence and task/change relevance;
- avoid running duplicate exploit attempts against the same underlying path.

Reconciliation output is versioned and auditable.

### 4. Exploit validation runs in a destructive-security boundary

Dynamic security testing is fundamentally different from normal browser automation. It can create accounts, mutate data, generate outbound traffic, trigger SSRF, upload payloads or otherwise alter the target.

Use a dedicated `SecurityRunnerPort` / security worker profile with explicit capabilities and a stricter policy than ordinary agent work.

Required controls:

- authorized target identity;
- environment classification (`local`, `staging`, `disposable`, `production`);
- default deny for production exploit mode;
- explicit Rules of Engagement;
- network/host/port scope;
- request/concurrency/rate budgets;
- credential bindings through Secret Broker;
- filesystem/repository mounted read-only unless a test explicitly needs generated artifacts;
- isolated browser/session state;
- bounded runtime/cost/model budgets;
- kill switch/cancellation;
- durable action receipts and exploit artifacts;
- target-specific workspace binding.

High-risk exploit mode should prefer disposable containers/VMs and disposable application data.

### 5. Rules of Engagement are typed policy, not only prose

Free-form guidance is useful but insufficient for enforcement.

Canonical `RulesOfEngagement` should support typed constraints such as:

```text
allowed_origins
allowed_subdomains
allowed_paths
forbidden_paths
allowed_methods
max_requests_per_second
max_login_attempts
forbid_password_bruteforce
forbid_data_deletion
forbid_external_callbacks
allow_ssrf_internal_probe
credential_binding_ids
data_redaction_policy
time_budget
cost_budget
```

A prose note may accompany the structure, but enforceable controls live in typed policy and are evaluated by Policy Engine before privileged actions.

### 6. Target identity must be bound to durable security state

Resuming a security run against a different target can contaminate findings/evidence and create dangerous actions.

A security workspace/checkpoint binds to a normalized target identity/fingerprint, including the relevant origin/environment and repository revision when applicable.

Resume fails closed when the target identity or protected run scope differs unless an explicit migration/new-run flow is used.

### 7. Exploit evidence is first-class

A `ProvenExploit` cannot be created from narrative text alone.

Evidence can include:

- correlated Browser/Tool Execution Receipts;
- HTTP request/response artifacts with redaction;
- source-location references;
- screenshots/video where appropriate;
- test account/state identifiers;
- reproduction steps;
- payload hashes;
- target/build/repository revision;
- timestamps and runner identity;
- policy/approval IDs.

The finding links to immutable evidence artifacts by reference/hash.

### 8. Gate only on the correct finding state

A pipeline must distinguish:

```text
scan incomplete
scan complete, no findings
analytical risks present
proven exploits present
security verification blocked
```

Never map all of these to a generic failure/success.

Default release policy should not fail a build because an LLM merely proposed a hypothesis. A policy may fail on:

- `ProvenExploit` severity >= threshold;
- specific high-confidence analytical risk classes;
- missing required security verification;
- incomplete/failed mandatory gate;
- target mismatch or evidence-integrity failure.

The exact policy is product/project configuration, not hard-coded engine behavior.

### 9. Security findings use standard interchange formats where useful

Support adapters for SARIF 2.1.0 and structured JSON so findings can flow to GitHub/GitLab/code-scanning/vulnerability-management tooling.

SARIF is an output adapter, not the canonical internal model. Internal findings retain richer execution/provenance/evidence semantics.

### 10. Durable security workflow and resumability

Long security checks can run for hours. Each stage must checkpoint independently:

```text
Pre-analysis       done
Recon              done
Injection analysis done
Auth analysis      running
Exploit queue      partially processed
Report             pending
```

Restart/retry skips completed verified work when inputs/fingerprints still match.

Provider/session crashes create new attempts; they do not erase prior evidence.

### 11. Orchestration implementation remains replaceable

Shannon demonstrates a good boundary by keeping Temporal activities thin and business services Temporal-agnostic. We adopt the principle, not Temporal itself.

Our security workflow logic depends on our own ports/contracts. A future implementation may use:

- internal Rust durable scheduler;
- a local procedure engine;
- Temporal in cloud/enterprise;
- another workflow runtime.

No domain service may import Temporal-specific or CI-specific types.

### 12. Parallelism is category/evidence aware

Independent analyses/exploit attempts can run in parallel, but concurrency must be bounded by:

- shared target safety;
- rate limits;
- test account conflicts;
- database/state interference;
- browser/session isolation;
- model/provider capacity;
- task dependency graph.

Do not parallelize destructive steps merely because multiple agents are available.

### 13. Provider choice is independent from security semantics

The security engine must not assume Claude/OpenAI/xAI or one model for the entire scan. Security roles express required capabilities/risk; Adaptive Model Router selects eligible models/providers.

Independent-review policies may intentionally require provider/model diversity for critical findings.

### 14. Prompt injection in source code remains a threat

Security scanners are especially exposed because they intentionally read adversarial-looking source, payloads and web content.

Repository/web/tool content remains untrusted evidence and may not become system/policy authority. Security agents receive scoped resources through Context Engine with the same trust/provenance rules as other agents.

### 15. Human review remains part of high-impact security conclusions

Autonomous exploitation raises confidence but does not remove false positives, incomplete evidence, severity mistakes or environment-specific artifacts.

Before production-facing remediation/release decisions for material findings, policy may require human confirmation of:

- exploit evidence;
- severity/business impact;
- scope;
- proposed remediation;
- whether the test affected external/real data.

### 16. New invariants from Shannon review

```text
INV-131 Security hypotheses, analytical risks and live-proven exploits are distinct states; inability to reproduce an exploit may not silently erase the underlying analytical risk.
INV-132 Dynamic exploit verification executes only against an explicitly authorized, target-bound environment and defaults to fail-closed for production targets.
INV-133 Rules of Engagement that must be enforced are represented as typed Policy Engine constraints; free-form prose alone is never an enforcement boundary.
INV-134 Security run/checkpoint state is bound to target identity and relevant repository/build scope; incompatible resumes fail closed.
INV-135 A ProvenExploit requires runtime evidence/receipts/artifacts tied to the exact target/build and may not be created from an LLM narrative alone.
INV-136 Security CI/release policy distinguishes incomplete, blocked, analytical-risk and proven-exploit states; hypotheses alone may not masquerade as proven findings.
INV-137 Static/code-analysis and runtime/recon findings are reconciled/deduplicated with provenance before expensive exploit scheduling.
INV-138 Security testing that may mutate target state runs through a dedicated restricted Security Runner/worker boundary with bounded credentials, network, rate, cost and destructive-action policy.
INV-139 Canonical security findings remain provider/runtime/CI-format independent; SARIF/JSON/PDF/Markdown are output representations/adapters.
INV-140 Security workflow state is durable and resumable at stage/attempt granularity; provider/process failure may not require repeating already verified stages when fingerprints still match.
INV-141 Security workflow/business services may not depend on a specific orchestration runtime such as Temporal; workflow engines are replaceable adapters/infrastructure.
INV-142 Parallel security work must respect target-state interference, rate limits, account/session conflicts and policy; availability of multiple agents never authorizes unsafe concurrency.
INV-143 Security source/web/payload content remains untrusted evidence and cannot acquire system/policy authority through inclusion in a pentest context.
INV-144 High-impact security findings preserve human-review capability and explicit uncertainty; autonomous exploitation does not imply infallible severity or remediation.
```

### 17. G6 security authority invariants

```text
INV-146 Canonical authorization is a deterministic AI Engineering OS core decision; provider/model output, frontend state, repository content and provider-native permission configuration can never grant authority.
INV-147 Delegated/child effective capabilities are always a subset of the parent effective capabilities; capability kind or scope widening fails closed.
INV-148 Hard policy denial cannot be weakened by approval, UX mode, provider, repository/workspace policy or lower-precedence configuration.
INV-149 Unknown capabilities, malformed security scope and unprovable mandatory restrictions fail closed without being conflated with ordinary internal/runtime failure.
INV-150 Approval is bound to the exact operation/request, scoped capabilities, policy fingerprint, provenance and validity window; stale or widened replay is invalid.
INV-151 Secret material never becomes ordinary application/SQLite/Event Journal state; durable application state stores only opaque secret references and scoped bindings.
INV-152 Canonical Security Audit evidence is product-authored and separate from Event Journal, operational logs and model-visible execution receipts; providers may contribute context but not authoritative audit facts.
INV-153 Policy preflight alone is not an enforcement boundary; security-sensitive enforcement is coupled to the privileged operation closely enough to revalidate or mechanically enforce the decided scope.
```

### Priority

- **P0:** Security Finding/Evidence model, typed Rules of Engagement, target binding, Security Runner safety boundary, proven-vs-analytical status separation.
- **P1:** reconciliation/deduplication, resumable security procedures, browser/tool receipts, SARIF adapter, release gate policy.
- **P2:** source-informed attack-path graph, multi-provider security councils, automated remediation + targeted re-verification, enterprise continuous AppSec integrations.


---

# External architecture adoption note — Tessra (`NitrusAphalion/tessra`) — 2026-09-11

Tessra was reviewed as an external reference for agent-era version control. **Decision: adopt concepts selectively; do not replace Git/worktrees or add Tessra as a dependency in v1.**

Architectural additions/reinforcements for later gates:

- **G6:** scoped/delegable capabilities must obey `child authority ⊆ parent authority`; future principal revocation should reconcile sessions, capabilities, claims, unlanded work, workspaces and unconfirmed learned state.
- **G7:** consider advisory `WorkClaim` objects and dependency-graph-informed partitioning as coordination hints; claims do not replace worktree isolation or become hard locks.
- **G8:** represent delivery standards as typed verification policy/data. Builder self-claims never satisfy their own completion gate. Verification evidence is tied to subject/toolchain/config fingerprints and produced by authorized independent principals/runners/humans. Human approval is durable evidence bound to the exact subject/risk.
- **G8/G11:** keep blocking gates separate from reactive hooks/routines. Hooks may produce evidence but cannot silently redefine the transition policy.
- **G9:** Context Compiler should support explicit token/byte budgets and assemble provenance-bearing packs from exact code, dependencies/dependents, relevant tests, architecture constraints, memories, active claims/conflicts and prior evidence. Semantic/code indexes remain rebuildable derived projections; Git/repository state remains authority.
- **G10:** Memory records should support provenance, anchor fingerprints, confidence, trust/lifecycle (`proposed`, `confirmed`, `contradicted`, `stale`, `superseded`, `retired`), supersession and optional expiry. Changes to anchored code mark memory stale rather than silently keeping it authoritative.
- **Cross-cutting:** externally retryable mutations should expose explicit idempotency semantics. Idempotency does not replace transactional invariants.
- **Future Delivery Reconciler/Landing Manager:** local landing and remote publication are separate resumable steps. A remote push failure yields durable publish-pending/failed state and safe retry rather than fake cross-system atomicity.
- **Future research only:** stable semantic code-unit identity, unit-level merge/conflict prediction and rename-aware reconciliation may be investigated as merge-assist/code-intelligence features. They are not a v1 VCS substrate.

New architecture principle:

```text
Agent claim is not evidence.
Evidence is not policy.
Policy gates transitions.
Hooks react to transitions.
Indexes are derived.
Repository/durable domain state stays authoritative.
```

Current gate is unchanged: G5 / TASK-0051 remains in scope; none of the Tessra concepts above are to be implemented opportunistically during G5.

---

# External architecture adoption note — Anthropic Claude Code — 2026-09-11

Claude Code was reviewed through its public repository and official documentation. Its public repository is **not an open-source implementation source**: Anthropic's `LICENSE.md` is all-rights-reserved under Anthropic commercial terms. Per project policy, we adopt only independently reimplemented architectural/product ideas; no code or repository dependency is adopted.

Architectural additions/reinforcements for later gates:

- **Cross-cutting:** provider session/transcript/memory/compaction/checkpoint state is compatibility/runtime state, never canonical Task/Execution/Attempt/Memory truth.
- **Interfaces:** CLI/Tauri/IDE/Web/Mobile/CI/MCP remain thin surfaces over one orchestration/domain engine. No surface reimplements workflow semantics.
- **G6:** distinguish user-facing `InteractionMode` from authoritative `PolicyRule`/`CapabilityGrant`; lower-precedence project/user/session settings may not weaken organization-managed hard policy. Sandboxing scopes filesystem, network, process and credential authority; prompt prose is not isolation.
- **G7:** manager/worker/team patterns map to RoleProfile + durable Task Graph + bounded delegation. Generated dynamic workflows may instantiate bounded work through canonical orchestration APIs but never become a second authority. A stronger-model advisor is a bounded escalation role selected by risk/capability/cost/capacity, not a hard-coded vendor pair.
- **G8:** provider-local gather→act→verify loops are useful execution behavior but do not grant Task completion authority. Hooks may produce evidence; only Verification Policy/Gates decide transitions.
- **G9:** Context Compiler uses progressive disclosure and deferred tool/schema loading. Child/subagent contexts are isolated and return bounded summaries plus evidence refs; summaries remain untrusted derived data. Provider compaction is optimization only. Cross-surface/session continuity is subordinate to provider-neutral `ContinuationCheckpoint` semantics.
- **G10:** provider-native memories/CLAUDE.md-like files are compatibility inputs/views; canonical memory remains typed, provenance-bearing, stale/supersession-aware and independently retrievable.
- **G11:** scheduled/event-driven routines are durable/auditable `RoutineDefinition`/`RoutineRun` workflows with explicit capability, budgets, evidence and escalation, not merely prompts on timers.
- **G12:** remote workers require explicit worker/session identity, scoped credentials, controlled egress, lifecycle/health state and no ambient organization secrets.
- **G13:** enterprise governance may include centrally managed policy, connector/MCP allow/deny, provider gateways, spend/quota controls, audit/analytics and remote-worker fleet management.
- **Marketplace:** plugin packages should declare requested capabilities, components, schemas, dependencies and provenance; organization policy may allow/deny plugins/publishers/capabilities. Resolved plugin versions need a durable install/lock ledger and rollback path.
- **Recovery:** provider/file checkpoints, Git revert, Task recovery and external-side-effect reconciliation are distinct recovery layers; no provider checkpoint implies global rollback.

Additional architecture principle:

```text
Provider harness state is useful evidence/runtime state.
It is never the provider-neutral source of truth.

Interaction modes shape UX.
Policy shapes authority.

Hooks react.
Verification gates decide.
```

Current gate is unchanged: G5 / TASK-0051 remains in scope; none of these Claude Code-derived ideas are to be implemented opportunistically during G5.

---

# External architecture adoption note — OpenAI Codex — 2026-09-11

OpenAI Codex was reviewed as both a first-class future provider harness and an architecture reference. Repository license is Apache-2.0, but project policy remains stricter: **we adopt only independently reimplemented ideas/failure lessons, never source code or repository dependencies because of this review.**

## Product-positioning decision

AI Engineering OS must not become “another Codex”. The durable product boundary is:

```text
Codex / Claude Code / OpenCode / Gemini / future harnesses
                          ↓
                  interchangeable executors
                          ↓
                   AI Engineering OS
                          ↓
Task Graph + Policy + Context + Verification + Recovery + Human Attention
```

Canonical statement:

> A provider harness answers how one agent performs coding work. AI Engineering OS answers how engineering work remains durable, secure, verifiable and continuous across multiple interchangeable agent providers.

## Architecture additions / reinforcements

- **Control plane / execution plane:** orchestration/domain state and process/filesystem/remote-worker execution are separate contracts. Execution components never own Task lifecycle authority.
- **Protocol negotiation:** clients/workers/providers negotiate protocol version and capabilities; security-critical unsupported requirements fail closed rather than silently degrading.
- **Generated contracts:** canonical backend protocol types may generate TypeScript/JSON-schema artifacts, backed by checked-in snapshots and compatibility tests.
- **Deterministic Runner path:** deterministic Git/test/lint/filesystem/verification actions bypass LLM turns when reasoning is not required.
- **CapabilityDelta:** privilege escalation requests ask for the smallest additional filesystem/network/process/secret scope, with reason, duration, risk, approval principal and durable audit evidence.
- **Approval binding:** approval/authorization is bound to action + Task intent + policy version fingerprints and is re-evaluated after material context change.
- **Reviewer routing:** policy can require a minimum reviewer class (`AUTO_REVIEWER`, `HUMAN`, etc.). Auto-review is advisory/authorized automation and never overrides hard deny or a human-required rule.
- **Policy-as-data:** policy is typed, versioned, explainable and testable before activation. Structured action semantics are preferred over shell-prefix-only rules.
- **Config provenance:** effective settings/policies expose origin and version. Hard organization policy is not ordinary preference merging and cannot be weakened downstream.
- **ProviderCapabilitySnapshot:** Router validates model availability, reasoning levels, modalities, tools, capacity/quota and policy before provider launch when knowable. Impossible combinations fail fast.
- **Remote workers:** G12 worker protocol uses authenticated worker identity, workspace/Attempt lease, URI/resource references, scoped secrets/network, heartbeat, version negotiation and durable reconnect/reconciliation.
- **Fail closed:** if a required sandbox/network/secret/worker security control cannot prove enforcement, execution does not silently proceed in a weaker mode.
- **Daemon lifecycle:** lifecycle mutations are idempotent/serialized. Updates/restarts should enter drain/checkpoint/reconcile flow rather than casually interrupt active work.
- **Provider session capabilities:** resume/fork/ephemeral/background-terminal features are adapter capabilities, not domain Task semantics.
- **Structured AgentOutput:** output schemas improve machine handling but do not constitute evidence; verification-changing claims reference durable evidence.
- **Minimal blocking:** HumanDecisionRequest blocks only the smallest dependent scope; unrelated Tasks/Executions continue.
- **Scoped repository guidance:** hierarchical project guidance may exist but remains below organization/system policy, current Execution Envelope, invariants and accepted ADRs.

## New candidate concepts

```text
ProtocolCapabilities
ProviderCapabilitySnapshot
CapabilityDelta
ApprovalGrant
ReviewerClass
DeterministicRunner
WorkerLease
```

Exact naming is provisional and must be resolved in the owning gate/ADR.

## Failure lessons incorporated

- Provider capability catalogs and actual launch validation must agree; unsupported model/effort combinations should be rejected before durable entry/launch when knowable.
- UI/hook surfaces need the effective approval route, not merely a generic “permission requested” event.
- Security approval paths must not deadlock behind unbounded/noncritical persistence work.
- Provider `turn completed`/session events are not equivalent to canonical durable completion.
- Long-running local/remote agent servers require supervision, heartbeat, terminal-state reconciliation and orphan recovery.

## 1% improvement policy applied

Whenever a Codex concept is adopted, AI Engineering OS must improve at least one meaningful dimension. Examples:

```text
Codex worktree
→ provider-neutral durable WorkspaceLease + recovery

Codex thread resume/fork
→ provider-neutral ContinuationCheckpoint + cross-provider handoff

Codex auto-review
→ policy-bounded reviewer routing + human-required classes

Codex execpolicy
→ typed semantic policy + provenance + versioning + executable policy tests

Codex daemon restart/update
→ drain + checkpoint + reconcile
```

## Pareto priority from this review

Highest-leverage work for future gates:

1. control/execution plane split;
2. least-privilege capability/policy model;
3. versioned/generated protocol contracts;
4. provider capability preflight;
5. worker/daemon supervision and recovery;
6. context-bound approvals/reviewer routing;
7. deterministic no-LLM execution when possible.

Provider-specific TUI/editor parity is low priority unless later evidence shows it materially improves the product's core user outcome.

## Gate placement

- G6: CapabilityDelta, policy-as-data, reviewer routing, ApprovalGrant, fail-closed security.
- G7: deterministic-vs-reasoning work selection, minimal human blocking, provider session capability use.
- G8: output/evidence separation and approval evidence semantics.
- G9: provider resume/fork mapped into provider-neutral continuity.
- G11: daemon supervision, draining, upgrades/reconciliation.
- G12: remote WorkerLease/protocol/resource references.
- G13: layered organization policy/config provenance and enterprise governance.
- Cross-cutting: protocol schema generation/versioning and ProviderCapabilitySnapshot.

Current gate remains G5 / TASK-0051. No Codex-derived future feature is to be implemented opportunistically inside the current remediation.


---

# External architecture adoption note — Scape / Senior Engineer Loop reference — 2026-09-11

Scape and the associated senior-engineer loop material were reviewed as a
product/orchestration reference. **Decision: adopt the durable workflow concepts
independently; do not copy implementation code or make Scape a dependency.**

The strongest concepts fit existing future gates rather than current G5 work:

- **Mission model:** represent durable orchestration intent as structured data,
  not one giant manager prompt. Preserve the separation
  `RoleProfile (WHO) + Mission (WHAT) + Procedure/Routine (HOW) + Policy (MUST/MUST NOT)`.
- **Routine / Pulse Engine:** scheduled, event-driven or condition-triggered
  orchestration wakeups become durable `RoutineDefinition` / `RoutineRun`
  objects with idempotency, cooldowns, budgets, evidence and escalation.
- **Playbook Engine:** approved executable procedures are versioned first-class
  objects with input/output schemas, capability requirements, SecretRefs,
  timeout/retry policy, filesystem/network scope and verification requirements.
  An agent may execute an approved locked version without automatically gaining
  authority to edit that version.
- **Workflow hierarchy:** preserve the conceptual ladder
  `Tool → Playbook → Procedure → Routine`; each layer adds composition and policy
  rather than duplicating lower-level execution mechanics.
- **Capability profiles:** read-only investigator/reviewer roles should be
  enforced by capability/policy boundaries, not merely described in prompts.
- **Soft rule vs hard policy:** guidance such as “search the repo before
  inventing infrastructure” is a behavioral rule; constraints such as
  “merge requires verification” belong to enforceable policy/gates.
- **Human Decision Queue:** blocked questions become structured
  `HumanDecisionRequest` records with task/risk/options/recommendation/blocking
  scope rather than scattered chat messages. Related decisions may be batched.
- **Review Gates:** plan review, code review, security review and human QA are
  explicit evidence-producing stages. Human QA should normally follow machine
  verification/review rather than become the first error detector.
- **Typed agent messaging:** `AgentMessage` is durable, typed, provenance-bearing
  and untrusted by default. Messages can carry information/requests/results but
  do not directly mutate canonical Task state.
- **Cross-provider handoff:** a `ContinuationCheckpoint`/`ExecutionHandoff`
  captures durable provider-neutral state so Claude → Codex → Gemini handoff does
  not depend on copying a chat transcript.
- **Provider capacity:** quota/availability/degraded state becomes Router input,
  enabling future quota-aware handoff without changing Task semantics.
- **Session projection:** session/fleet UI is a projection, not authority. The
  primary UI should answer: what is running, blocked, changed, needs attention,
  costs resources, and failed verification.
- **Internal Task Graph remains authority:** external GitHub/Linear/Jira/Slack
  sources are ingress adapters/projections, never competing task authorities.
- **Learning/promotion pipeline:** reusable findings may be promoted into
  Memory/Skills/Procedures/Rules/Docs only with provenance, validation and the
  correct authority boundary.
- **Watchdog:** stuck/looping/silent/context-drift detection belongs to durable
  background reconciliation, not ad-hoc prompt loops.

Gate placement remains:

```text
G6  Policy / capability profiles / read-only investigator
G7  RoleProfile / Mission / orchestration / Human Decision Queue / WorkClaims
G8  Review + Verification Gates
G9  AgentMessage / ContinuationCheckpoint / cross-provider handoff
G10 Memory + learning/promotion + versioned procedures
G11 Routine/Pulse Engine + Watchdogs
Future Router: ProviderCapacity / quota-aware switching
```

Current principle:

```text
Human Intent
→ Durable Task Graph
→ Policy
→ Planner / Orchestrator
→ Provider-neutral Executors
→ Isolated Workspaces
→ Evidence
→ Independent Verification
→ Human Attention Queue
```

The control plane must remain deterministic and auditable around inherently
non-deterministic model behavior.


---

# External architecture adoption note — Observer reference — 2026-09-11

Observer was reviewed primarily for local-first/privacy/product architecture and
cheap-local-agent patterns. **Decision: adopt selected concepts independently;
Observer is not a dependency and its implementation is not copied.**

Architecture lessons to retain:

- **Local-first is an architecture boundary, not only a pricing choice:** source
  code, Task state, Event Journal, secrets and telemetry remain local by default.
  Hosted/team functionality is explicit and optional.
- **Evidence Sources → Context Materialization → Model/Agent → Typed Tools → Policy
  → Runtime:** models remain replaceable reasoning components between typed input
  evidence and typed capabilities.
- **Capability Registry:** filesystem, Git, tests, build, network, secrets,
  process, workspace and review actions should be explicit capabilities consumed
  by Policy, Playbooks, audit and marketplace metadata.
- **MicroAgent:** future narrow repeated background workers use bounded context,
  bounded capabilities, explicit triggers and small/local/cheap models for
  classification, log triage, workspace health, documentation drift, stale-task
  detection or quota monitoring.
- **Cheap watcher → strong reasoner escalation:** frequent inexpensive checks
  produce evidence packets and invoke frontier/costly agents only after
  threshold/deduplication policy says escalation is worthwhile.
- **LocalInferenceProvider:** a generic local inference adapter may serve utility
  models through OpenAI-compatible/local endpoints, but coding-agent harnesses
  retain richer provider adapters and must not be collapsed into a lowest-common
  denominator API.
- **MCP is an ingress/control surface, not authority:** MCP/UI/CLI/API all map into
  canonical commands and domain state.
- **Context Disclosure Preview:** future privacy UX should explain what material
  will leave the machine, why, to which provider and for which Task before
  sensitive external inference.
- **Release trust:** reproducible builds, signed releases, SBOM/provenance and
  attestations are future distribution/security goals for the desktop product.

Local models are supporting infrastructure, not a mandatory hardware
requirement. Subscription-backed Claude/Codex/Gemini-style providers remain
first-class high-capability workers; small local models are optional background
or routing helpers.
