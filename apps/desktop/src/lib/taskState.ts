/** Presentation of agentd's canonical Task lifecycle names (crates/task state.rs). */
export interface StatePresentation {
  label: string;
  /** Glyph shape carries meaning; colour only reinforces it. */
  glyph: "g-done" | "g-ready" | "g-run" | "g-wait" | "g-attn" | "g-fail";
  tone: "" | "ready" | "run" | "wait" | "attn" | "fail";
  group: StateGroup;
}

export type StateGroup = "ready" | "active" | "waiting" | "closed";

const STATES: Record<string, StatePresentation> = {
  DRAFT: { label: "Черновик", glyph: "g-wait", tone: "wait", group: "waiting" },
  READY: { label: "Готова к запуску", glyph: "g-ready", tone: "ready", group: "ready" },
  CLAIMED: { label: "Взята", glyph: "g-ready", tone: "ready", group: "active" },
  RUNNING: { label: "Выполняется", glyph: "g-run", tone: "run", group: "active" },
  BLOCKED: { label: "Заблокирована", glyph: "g-attn", tone: "attn", group: "waiting" },
  SUBMITTED: { label: "Результат отправлен", glyph: "g-ready", tone: "ready", group: "active" },
  AWAITING_REVIEW: { label: "Ждёт ревью", glyph: "g-attn", tone: "attn", group: "waiting" },
  VERIFYING: { label: "Проверяется", glyph: "g-ready", tone: "ready", group: "active" },
  RETRY_READY: { label: "Готова к повтору", glyph: "g-ready", tone: "ready", group: "ready" },
  COMPLETED: { label: "Завершена", glyph: "g-done", tone: "", group: "closed" },
  FAILED: { label: "Провалена", glyph: "g-fail", tone: "fail", group: "closed" },
  CANCELLED: { label: "Отменена", glyph: "g-wait", tone: "wait", group: "closed" },
};

/** Unknown names stay visible verbatim instead of being hidden or guessed. */
export function presentState(state: string): StatePresentation {
  return STATES[state] ?? { label: state, glyph: "g-wait", tone: "wait", group: "waiting" };
}

/** Verdict of one verification criterion. Separate from the Task lifecycle: a criterion is never "completed". */
export type Verdict = "verified" | "checking" | "pending";

export const VERDICTS: Record<Verdict, { label: string; glyph: string; tone: string }> = {
  verified: { label: "Проверено", glyph: "g-ok", tone: "ok" },
  checking: { label: "Проверяется", glyph: "g-ready", tone: "ready" },
  pending: { label: "Ожидает", glyph: "g-wait", tone: "wait" },
};

export const GROUP_LABELS: Record<StateGroup, string> = {
  ready: "Готовы",
  active: "В работе",
  waiting: "Ждут",
  closed: "Закрыты",
};

export const DECISION_LABELS: Record<string, string> = {
  RESUME: "Продолжить",
  REPLAN: "Перепланировать",
  ABORT: "Прервать",
  TAKEOVER: "Взять управление",
};
