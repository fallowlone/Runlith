import type { IconName } from "../components/Icon";
import type { SourceType } from "./bindings/SourceType";

const META: Record<SourceType, { title: string; icon: IconName }> = {
  "github.repo.stars": { title: "Звёзды на GitHub", icon: "star" },
  "github.repo.open_prs": { title: "Открытые пул-реквесты", icon: "gitPullRequest" },
};

export const sourceMeta = (source: SourceType): { title: string; icon: IconName } => META[source];

export const REFRESH_OPTIONS: Array<[number, string]> = [[300, "5 мин"], [900, "15 мин"], [3600, "1 час"]];

export function everyText(seconds: number): string {
  if (seconds === 3600) return "каждый час";
  return `каждые ${seconds / 60} мин`;
}

const MIN_READ_WAIT_MS = 1000;

/** Waits until the host will really ask GitHub, including past a rate limit. */
export function nextReadDelay(retryAtMs: number | null, everySeconds: number, now: number): number {
  return retryAtMs === null ? everySeconds * 1000 : Math.max(MIN_READ_WAIT_MS, retryAtMs - now);
}

/** Same rule as `RepoName` in widget-spec, which has the final word on save. */
export function isRepoName(value: string): boolean {
  return /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?\/[A-Za-z0-9._-]{1,100}$/.test(value) && !/\/\.{1,2}$/.test(value);
}

const FAILURE_TEXT: Record<string, string> = {
  provider_missing: "Не найден claude CLI. Установите Claude Code и попробуйте снова.",
  provider_incompatible: "Установленный claude CLI слишком старый. Обновите его и попробуйте снова.",
  provider_unavailable: "Claude сейчас недоступен: нет сети, лимит или сбой у провайдера. Попробуйте позже.",
  timed_out: "Claude не ответил за минуту. Попробуйте ещё раз.",
  bad_output: "Ответ Claude не прошёл проверку, поэтому виджет не создан. Попробуйте ещё раз или опишите иначе.",
};

/** A command stays on one line, so it can be read and retyped. */
const cmd = (command: string): string => command.replaceAll(" ", "\u00A0");
const NEW_TOKEN = `В терминале: ${cmd("claude setup-token")}, затем ${cmd("agentctl vault store")}.`;

/** Each reason is a distinct code inside agentd's refusal (`CredentialRefusal::message`, `ClaudeFailure`). */
const CREDENTIAL_TEXT: Array<[string, string]> = [
  ["provider_credential_vault_absent", `Хранилище токенов Runlith ещё не создано. В терминале: ${cmd("agentctl vault init")}, затем ${cmd("agentctl vault store")}.`],
  ["provider_credential_vault_unavailable", `Хранилище токенов Runlith не читается. Проверьте в терминале: ${cmd("agentctl vault status")}.`],
  ["provider_credential_vault_locked", `Хранилище токенов Runlith заблокировано. В терминале: ${cmd("agentctl vault unlock")}.`],
  ["provider_credential_missing", `Токен Claude не сохранён. ${NEW_TOKEN}`],
  ["provider_credential_expired", `Срок токена Claude истёк. ${NEW_TOKEN}`],
  ["provider_credential_rejected", `Claude отклонил сохранённый токен. ${NEW_TOKEN}`],
];

/** Fixed wording per code. `detail` only selects a sentence; provider prose is never shown. */
export function failureText(code: string | null, detail: string | null): string {
  if (code === "not_authenticated") {
    const reason = CREDENTIAL_TEXT.find(([code]) => detail?.includes(code));
    return `Нет доступа к Claude. ${reason?.[1] ?? "Войдите в claude CLI и попробуйте снова."}`;
  }
  return FAILURE_TEXT[code ?? ""] ?? "Claude завершился с ошибкой. Попробуйте ещё раз.";
}
