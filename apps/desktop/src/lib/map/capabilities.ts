/**
 * Capabilities the agent recorded for this repository: product level abilities
 * ("Авторизация и политика", ...) grouping the Cargo crates that implement them.
 *
 * Each record mirrors a SEMANTIC memory (crates/ipc-contracts/proto/memory_core.proto):
 * FILE source refs are the crate directories, `confidence` is per mille like
 * MemoryCaptureRequest.confidence, `validated` means a human confirmed it
 * (MemoryValidateRequest). agentd has no read RPC for a project map yet, so
 * this file is the record and the UI labels the grouping as demo.
 *
 * Crate membership and crate dependencies are not authored here: they come from
 * `cargo metadata` in scripts/gen-maps.ts and are checked by scripts/maps.test.ts.
 */
export interface CapabilityRecord {
  id: string;
  title: string;
  summary: string;
  crates: string[];
  confidence: number;
  validated: boolean;
}

export const CAPABILITIES: CapabilityRecord[] = [
  { id: "apps", title: "Точки входа", summary: "Daemon, CLI, композиция сервисов и формат виджетов", crates: ["agentd", "agentctl", "app-services", "widget-spec"], confidence: 950, validated: false },
  { id: "ipc", title: "Локальный IPC", summary: "Протокол, handshake и сокет с проверкой владельца", crates: ["ipc-contracts", "ipc-control", "ipc-transport", "product-version"], confidence: 920, validated: false },
  { id: "auth", title: "Авторизация и политика", summary: "ALLOW, REQUIRE_APPROVAL, DENY, роли и аудит", crates: ["policy", "security-audit", "org", "org-contract"], confidence: 780, validated: false },
  { id: "secrets", title: "Секреты", summary: "Vault, keychain и контракт бэкенда", crates: ["secrets", "secret-backend-contract", "secrets-vault", "keychain-native"], confidence: 900, validated: false },
  { id: "orch", title: "Исполнение агента", summary: "Рантайм агента, планировщик, контракт исполнения", crates: ["agent-runtime", "scheduler", "execution-contract"], confidence: 840, validated: false },
  { id: "tasks", title: "Задачи и планы", summary: "Домен Task и Plan, контракт планировщика", crates: ["task-domain", "plan-domain", "planner-contract", "fake-planner"], confidence: 900, validated: false },
  { id: "exec", title: "Песочница", summary: "Запуск команд, seatbelt и захват эффектов", crates: ["runner", "runner-native", "runner-macos-seatbelt", "effect-capture-contract", "effect-capture-local"], confidence: 860, validated: false },
  { id: "eval", title: "Проверка результатов", summary: "Оценка попыток по критериям", crates: ["evaluation", "fake-evaluator"], confidence: 820, validated: false },
  { id: "prov", title: "Провайдеры агентов", summary: "Claude и тестовый агент", crates: ["provider-claude", "fake-agent"], confidence: 880, validated: false },
  { id: "mem", title: "Память и контекст", summary: "Структурная память и сборка контекста", crates: ["memory", "memory-contract", "context-engine", "context-engine-contract"], confidence: 870, validated: false },
  { id: "cont", title: "Продолжение работы", summary: "Continuity и передача управления человеку", crates: ["continuity", "continuity-contract", "takeover"], confidence: 760, validated: false },
  { id: "repo", title: "Репозиторий", summary: "Рабочие копии, git и поиск по коду", crates: ["workspace", "git-cli", "repo-finder", "repo-finder-contract", "finder-local"], confidence: 850, validated: false },
  { id: "bg", title: "Фон и воркеры", summary: "Рутины и удалённые воркеры", crates: ["background", "background-contract", "worker", "worker-contract"], confidence: 800, validated: false },
  { id: "store", title: "Хранилище", summary: "Состояние daemon в SQLite", crates: ["storage-sqlite"], confidence: 930, validated: false },
];
