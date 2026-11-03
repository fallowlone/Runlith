import { useCallback, useEffect, useRef, useState } from "react";
import type { DashboardLayout } from "./bindings/DashboardLayout";
import type { UiError } from "./bindings/UiError";
import { api, toUiError, type Source } from "./api";

type Dashboard = {
  layout: DashboardLayout | null;
  source: Source;
  recoveredCopy: string | null;
  loadError: UiError | null;
  saveError: UiError | null;
  commit: (next: DashboardLayout) => void;
  reload: () => void;
};

/** The saved layout. A change shows at once; saves run one after another, in order. */
export function useDashboard(): Dashboard {
  const [layout, setLayout] = useState<DashboardLayout | null>(null);
  const [source, setSource] = useState<Source>("live");
  const [recoveredCopy, setRecoveredCopy] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<UiError | null>(null);
  const [saveError, setSaveError] = useState<UiError | null>(null);
  const [attempt, setAttempt] = useState(0);
  const saving = useRef<Promise<unknown>>(Promise.resolve());

  useEffect(() => {
    let isCurrent = true;
    setLoadError(null);
    api.dashboardLoad().then(
      ({ data, source: from }) => {
        if (!isCurrent) return;
        setLayout(data.layout);
        setRecoveredCopy(data.recoveredCopy);
        setSource(from);
      },
      (error: unknown) => {
        if (isCurrent) setLoadError(toUiError(error));
      },
    );
    return () => {
      isCurrent = false;
    };
  }, [attempt]);

  const commit = useCallback((next: DashboardLayout) => {
    setLayout(next);
    setRecoveredCopy(null);
    setSaveError(null);
    saving.current = saving.current.then(() =>
      api.dashboardSave(next).then(
        () => setSaveError(null),
        (error: unknown) => setSaveError(toUiError(error)),
      ),
    );
  }, []);

  return { layout, source, recoveredCopy, loadError, saveError, commit, reload: () => setAttempt((value) => value + 1) };
}
