/**
 * Demo fixtures, shaped exactly like the host DTOs.
 * Used only outside Tauri (browser preview, e2e) and for screens whose read
 * contract does not exist in agentd yet. Every screen that shows them says so.
 * Values come from planning/tasks/queue.yaml and git on 21.10.2026.
 */
import type { ConnectionDto } from "./bindings/ConnectionDto";
import type { DecisionDto } from "./bindings/DecisionDto";
import type { ProjectDto } from "./bindings/ProjectDto";
import type { TaskDto } from "./bindings/TaskDto";
import type { Verdict } from "./taskState";

export type DemoScenario = "normal" | "empty" | "unavailable" | "loading";

/** `?demo=empty|unavailable|loading` forces a state in browser preview. */
export function demoScenario(): DemoScenario {
  const value = new URLSearchParams(window.location.search).get("demo");
  return value === "empty" || value === "unavailable" || value === "loading" ? value : "normal";
}

const PROJECT = "runlith";

const TASKS: Array<[string, string, string]> = [
  ["TASK-0229", "Cross-daemon resume race: атомарное потребление attempt-reservation", "COMPLETED"],
  ["TASK-0215", "Регистрация governance G15 (Git Graph & Operations)", "COMPLETED"],
  ["TASK-0216", "ADR-0048: контракт git graph и операций", "COMPLETED"],
  ["TASK-0217", "Чистый домен git-graph, lanes и контрактные crates", "READY"],
  ["TASK-0218", "git-cli адаптер истории и набор fixture-топологий", "DRAFT"],
  ["TASK-0219", "GitGraphService: композиция overlay", "DRAFT"],
  ["TASK-0220", "Git graph IPC и поверхность agentctl", "DRAFT"],
  ["TASK-0221", "Бенчмарк AI-проекции и dogfood", "DRAFT"],
  ["TASK-0222", "Tauri render contract и спецификация диаграммы", "DRAFT"],
  ["TASK-0223", "Ops I: refs, checkout и мутации worktree", "DRAFT"],
  ["TASK-0224", "Ops II: stage, commit, stash и reset", "DRAFT"],
  ["TASK-0228", "Closure audit G15", "DRAFT"],
];

const empty = (): boolean => demoScenario() === "empty";

export const demo = {
  get connection(): ConnectionDto {
    return { state: "connected", protocol: 12, daemonBuild: "демо", clientBuild: "0.1.0", detail: null };
  },
  get projects(): ProjectDto[] {
    return empty() ? [] : [{ id: PROJECT, name: "runlith" }];
  },
  get tasks(): TaskDto[] {
    return empty() ? [] : TASKS.map(([id, objective, state]) => ({ id, projectId: PROJECT, objective, state }));
  },
  dependencies: {
    "TASK-0216": ["TASK-0215"],
    "TASK-0217": ["TASK-0216"],
    "TASK-0218": ["TASK-0217"],
    "TASK-0219": ["TASK-0218"],
    "TASK-0220": ["TASK-0219"],
    "TASK-0221": ["TASK-0220"],
    "TASK-0222": ["TASK-0220"],
    "TASK-0223": ["TASK-0220"],
    "TASK-0224": ["TASK-0223"],
    "TASK-0228": ["TASK-0227", "TASK-0222", "TASK-0221"],
  } as Record<string, string[]>,
  get decisions(): DecisionDto[] {
    if (empty()) return [];
    return [
      {
        requestId: "hd-demo-0229",
        taskId: "TASK-0229",
        reason: "Демо запрос: проверка критерия 2 не завершилась, нужен выбор продолжения",
        risk: "medium",
        affectedScope: ["adapters/storage-sqlite", "bins/agentd"],
        options: ["RESUME", "REPLAN", "ABORT"],
        recommendation: "RESUME",
        evidence: ["execution_recovery · демо"],
        status: "PENDING",
        createdAtMs: 1792946400000,
        expiresAtMs: null,
      },
    ];
  },
};

/* ---------- screens without an agentd read contract ---------- */

export const GATE_COUNTS = [5, 5, 11, 7, 6, 15, 17, 46, 30, 16, 9, 9, 9, 9, 10, 14];
export const WEEKLY_COMMITS = [40, 167, 150, 34, 102, 33, 33];

export const BRANCH = "task-0229-cross-daemon-resume-race";

/** [hash, time, message, changed lines, kind, added, deleted] */
export const COMMITS: Array<[string, string, string, number, "feat" | "test" | "fix" | "docs", number, number]> = [
  ["f9757f6", "19:40", "feat(g5): serialize workspace cycles per execution", 258, "feat", 257, 1],
  ["329ccdc", "19:51", "feat(g5): real Windows cycle-lock path via LockFileEx", 160, "feat", 152, 8],
  ["1089f8a", "20:08", "test(g5): exercise cycle-lock primitive directly", 183, "test", 89, 94],
  ["95cd8ca", "20:21", "fix(g5): pass a real OVERLAPPED to LockFileEx", 9, "fix", 7, 2],
  ["82ee6d5", "20:49", "docs(readme): sync gate status and Windows CI", 12, "docs", 8, 4],
];

export const FILES: Array<[string, number, number]> = [
  ["adapters/storage-sqlite/src/execution_cycle_lock.rs", 355, 104],
  ["bins/agentd/tests/execution_recovery.rs", 77, 0],
  [".github/workflows/verification.yml", 28, 0],
  ["crates/app-services/src/orchestration_service.rs", 23, 0],
  ["planning/tasks/queue.yaml", 11, 0],
  ["README.md", 8, 4],
  ["adapters/storage-sqlite/Cargo.toml", 6, 0],
  ["adapters/storage-sqlite/src/lib.rs", 2, 0],
  ["Cargo.lock", 2, 0],
  [".githooks/pre-commit", 1, 1],
];

/** [verdict state, criterion, source, snapshot] */
export const CRITERIA: Array<[Verdict, string, string, string]> = [
  ["verified", "Race keeper (8 композиций, одна БД): одна SUCCEEDED attempt и один факт attempt.created", "execution_recovery · демо", "82ee6d5"],
  ["checking", "Проигравшие получают типизированный отказ без мутации журнала", "идёт · демо", "82ee6d5"],
  ["pending", "Target execution_recovery и workspace suite остаются зелёными", "нет evidence", "—"],
];

/** [state, id, objective, reason, nested] */
export const PLAN_ROWS: Array<[string, string, string, string, boolean]> = [
  ["COMPLETED", "TASK-0215", "Регистрация governance G15", "есть outcome", false],
  ["COMPLETED", "TASK-0216", "ADR-0048: контракт git graph и операций", "есть outcome", false],
  ["READY", "TASK-0217", "Чистый домен git-graph, lanes и контракты", "зависимость 0216 выполнена", false],
  ["DRAFT", "TASK-0218", "git-cli адаптер истории и fixture-топологии", "ждёт 0217", false],
  ["DRAFT", "TASK-0219", "GitGraphService: композиция overlay", "ждёт 0218", false],
  ["DRAFT", "TASK-0220", "Git graph IPC и поверхность agentctl", "ждёт 0219", false],
  ["DRAFT", "TASK-0221", "Бенчмарк AI-проекции и dogfood", "ждёт 0220", true],
  ["DRAFT", "TASK-0222", "Tauri render contract и спецификация диаграммы", "ждёт 0220", true],
  ["DRAFT", "TASK-0223", "Ops I: refs, checkout и мутации worktree", "ждёт 0220", true],
  ["DRAFT", "TASK-0224", "Ops II: stage, commit, stash и reset", "ждёт 0223", true],
  ["DRAFT", "TASK-0225", "Ops III: merge, cherry-pick, revert, rebase", "ждёт 0224", true],
  ["DRAFT", "TASK-0226", "Ops IV: remote fetch, pull и push", "ждёт 0225", true],
  ["DRAFT", "TASK-0227", "G15 failure matrix и adversarial dogfood", "ждёт 0226", false],
  ["DRAFT", "TASK-0228", "Closure audit G15", "ждёт 0227, 0222, 0221", false],
];
