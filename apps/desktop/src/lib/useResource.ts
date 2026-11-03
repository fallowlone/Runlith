import { useCallback, useEffect, useState } from "react";
import type { UiError } from "./bindings/UiError";
import { toUiError, type Loaded, type Source } from "./api";

export type Resource<T> =
  | { status: "loading" }
  | { status: "ok"; data: T; source: Source }
  | { status: "error"; error: UiError };

/**
 * Loads one value and re-runs when `key` changes. Stale responses from an
 * earlier key are dropped, so fast navigation never shows the wrong data.
 */
export function useResource<T>(load: () => Promise<Loaded<T>>, key: string): [Resource<T>, () => void] {
  const [state, setState] = useState<Resource<T>>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let current = true;
    setState({ status: "loading" });
    load().then(
      (loaded) => current && setState({ status: "ok", data: loaded.data, source: loaded.source }),
      (error: unknown) => current && setState({ status: "error", error: toUiError(error) }),
    );
    return () => {
      current = false;
    };
    // `load` is recreated each render; `key` captures what it depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, attempt]);

  const reload = useCallback(() => setAttempt((value) => value + 1), []);
  return [state, reload];
}
