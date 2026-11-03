import { useState } from "react";

export type Tab = "overview" | "plan" | "changes" | "evidence" | "history";

export type Route =
  | { name: "overview" }
  | { name: "tasks" }
  | { name: "task"; id: string; tab: Tab }
  | { name: "attention" }
  | { name: "map" }
  | { name: "settings" }
  | { name: "connection" };

/**
 * In-memory router. Screen entrances animate through `.reveal` on the new
 * content only; a whole-window View Transition was measured to cost 2–3
 * dropped frames at teardown (e2e "long frames" test) and was removed.
 */
export function useRouter(): [Route, (route: Route) => void] {
  return useState<Route>({ name: "overview" });
}
