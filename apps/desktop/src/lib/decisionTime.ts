const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function span(ms: number): string {
  if (ms < MINUTE) return "меньше минуты";
  if (ms < HOUR) return `${Math.floor(ms / MINUTE)} мин`;
  if (ms < DAY) return `${Math.floor(ms / HOUR)} ч`;
  return `${Math.floor(ms / DAY)} дн`;
}

/** How long a request has been waiting for a person. */
export function waitingFor(createdAtMs: number, nowMs: number): string {
  return `ждёт ${span(Math.max(0, nowMs - createdAtMs))}`;
}

export function isExpired(expiresAtMs: number | null, nowMs: number): boolean {
  return expiresAtMs !== null && expiresAtMs <= nowMs;
}

/** agentd sends no expiry for a request that never expires: say so instead of showing nothing. */
export function expiryNote(expiresAtMs: number | null, nowMs: number): string {
  if (expiresAtMs === null) return "бессрочно";
  return isExpired(expiresAtMs, nowMs) ? "срок истёк" : `срок ${span(expiresAtMs - nowMs)}`;
}
