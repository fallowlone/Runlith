import { useEffect, useRef, useState } from "react";
import type { DashboardLayout } from "../lib/bindings/DashboardLayout";
import type { DecisionDto } from "../lib/bindings/DecisionDto";
import type { Span } from "../lib/bindings/Span";
import type { TaskDto } from "../lib/bindings/TaskDto";
import type { Widget } from "../lib/bindings/Widget";
import type { Route } from "../lib/router";
import type { Resource } from "../lib/useResource";
import { api, toUiError } from "../lib/api";
import { useDashboard } from "../lib/useDashboard";
import { useFlip } from "../lib/useFlip";
import { ErrorState } from "../components/Feedback";
import { Icon } from "../components/Icon";
import { TopBar } from "../components/TopBar";
import { Block } from "../components/widgets/Blocks";
import { MetricCard } from "../components/widgets/MetricCard";
import { SpanPicker, WidgetAuthor } from "../components/widgets/WidgetAuthor";
import { sourceMeta } from "../lib/widgetText";

type Props = {
  projectName: string | null;
  tasks: Resource<TaskDto[]>;
  decisions: Resource<DecisionDto[]>;
  reload: () => void;
  navigate: (route: Route) => void;
  query: string;
  onQuery: (value: string) => void;
};

const UNDO_MS = 8000;
const NOTE_MS = 4000;
const STOCK_BLOCKS: Record<string, string> = {
  summary: "Сводка", states: "Состояния задач", graph: "Граф зависимостей", commits: "Коммиты по неделям", branch: "Ветка", attention: "Требует внимания",
};

const widgetName = (widget: Widget): string =>
  widget.kind === "metric" ? sourceMeta(widget.source.type).title : STOCK_BLOCKS[widget.block] ?? widget.block;

/** `undo` works on the current list, so a change made after the removal survives. */
type Toast = { text: string; undo?: (current: Widget[]) => Widget[] };

export function Overview({ projectName, tasks, decisions, reload, navigate, query, onQuery }: Props) {
  const dashboard = useDashboard();
  const [isEditing, setEditing] = useState(false);
  const [draftSpan, setDraftSpan] = useState<Span | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const grid = useRef<HTMLDivElement>(null);
  const undoButton = useRef<HTMLButtonElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const focusAfter = useRef<string | null>(null);
  const flip = useFlip(grid);
  const layout = dashboard.layout;

  useEffect(() => {
    if (!toast) return undefined;
    if (toast.undo) undoButton.current?.focus();
    const timer = window.setTimeout(() => {
      // The undo button is about to vanish; the keyboard must not fall back to the page start.
      if (document.activeElement === undoButton.current) heading.current?.focus();
      setToast(null);
    }, toast.undo ? UNDO_MS : NOTE_MS);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (!focusAfter.current) return;
    const cell = grid.current?.querySelector<HTMLElement>(`[data-widget-id="${focusAfter.current}"]`);
    (cell?.querySelector<HTMLElement>(".widget-add") ?? cell)?.focus();
    focusAfter.current = null;
  });

  const change = (next: Widget[]): void => {
    if (!layout) return;
    const updated: DashboardLayout = { ...layout, widgets: next };
    flip(() => dashboard.commit(updated));
  };

  const setSpan = (widget: Widget, span: Span): void => {
    if (!layout) return;
    change(layout.widgets.map((item) => (item.id === widget.id ? { ...item, span } : item)));
  };

  const remove = (widget: Widget): void => {
    if (!layout) return;
    const index = layout.widgets.findIndex((item) => item.id === widget.id);
    change(layout.widgets.filter((item) => item.id !== widget.id));
    setToast({
      text: "Виджет удалён",
      undo: (current) => [...current.slice(0, index), widget, ...current.slice(index)],
    });
  };

  const restoreStock = (): void => {
    if (!layout) return;
    const before = layout.widgets;
    api.dashboardStock().then(
      ({ data }) => {
        change(data.widgets);
        setToast({ text: "Главная сброшена", undo: () => before });
      },
      (error: unknown) => setToast({ text: `Не удалось вернуть главную: ${toUiError(error).message}` }),
    );
  };

  const undo = (): void => {
    if (!layout || !toast?.undo) return;
    change(toast.undo(layout.widgets));
    setToast(null);
    heading.current?.focus();
  };

  const add = (widget: Widget): void => {
    if (!layout) return;
    focusAfter.current = widget.id;
    flip(() => {
      dashboard.commit({ ...layout, widgets: [...layout.widgets, widget] });
      setDraftSpan(null);
    });
    setToast({ text: "Виджет добавлен и сохранён" });
  };

  const lastRepo = [...(layout?.widgets ?? [])].reverse().find((widget) => widget.kind === "metric");

  return (
    <main className="tile">
      <TopBar
        query={query}
        onQuery={(value) => {
          onQuery(value);
          if (value) navigate({ name: "tasks" });
        }}
      >
        {toast && (
          <div className="toast caption-5" role="status">
            <span>{toast.text}</span>
            {toast.undo && <button ref={undoButton} className="btn btn-ghost" type="button" onClick={undo}>Вернуть</button>}
          </div>
        )}
      </TopBar>
      <div className="view">
        {tasks.status === "error" && <ErrorState error={tasks.error} onRetry={reload} />}
        <div className="view-head" style={{ alignItems: "center" }}>
          <h1 ref={heading} className="heading-3 grad-text" tabIndex={-1}>Обзор проекта</h1>
          <span className="spacer" />
          {layout && (isEditing || layout.widgets.length === 0) && (
            <button className="btn btn-quiet" type="button" onClick={restoreStock}>Вернуть как было</button>
          )}
          <button
            className="btn btn-ghost"
            type="button"
            aria-pressed={isEditing}
            disabled={!layout}
            onClick={() => flip(() => { setEditing(!isEditing); setDraftSpan(null); })}
          >
            <Icon name={isEditing ? "check" : "slidersHorizontal"} small />
            {isEditing ? "Готово" : "Настроить виджеты"}
          </button>
        </div>

        {dashboard.loadError && <ErrorState error={dashboard.loadError} onRetry={dashboard.reload} />}
        {dashboard.recoveredCopy && (
          <div className="notice" role="status">
            <Icon name="warningDiamond" small />
            <p className="caption-5"><b>Файл раскладки был повреждён.</b> Показана стандартная главная, старый файл сохранён как <span className="mono">{dashboard.recoveredCopy}</span>.</p>
          </div>
        )}
        {dashboard.saveError && (
          <div className="notice err" role="alert">
            <Icon name="warningDiamond" small />
            <p className="caption-5"><b>Раскладка не сохранена.</b> {dashboard.saveError.message}</p>
          </div>
        )}

        {layout && (
          <div ref={grid} className="widget-grid">
            {layout.widgets.map((widget, index) => (
              <div key={widget.id} className={`widget-cell${widget.span === 2 ? " widget-span-2" : ""}`} data-widget-id={widget.id} tabIndex={-1}>
                {isEditing && (
                  <div className="widget-tools">
                    <SpanPicker span={widget.span} onSpan={(span) => setSpan(widget, span)} label={`Ширина: ${widgetName(widget)}`} />
                    <button className="btn btn-icon btn-quiet" type="button" aria-label={`Удалить: ${widgetName(widget)}`} title="Удалить" onClick={() => remove(widget)}>
                      <Icon name="trash" small />
                    </button>
                  </div>
                )}
                {widget.kind === "metric" ? (
                  <MetricCard source={widget.source} everySeconds={widget.refresh.everySeconds} index={index} />
                ) : (
                  <Block block={widget.block} data={{ projectName, tasks, decisions, navigate }} index={index} />
                )}
              </div>
            ))}

            {draftSpan !== null && (
              <div className={`widget-cell${draftSpan === 2 ? " widget-span-2" : ""}`} data-widget-id="draft">
                <WidgetAuthor
                  span={draftSpan}
                  onSpan={(span) => flip(() => setDraftSpan(span))}
                  lastRepo={lastRepo?.kind === "metric" ? lastRepo.source.repo : ""}
                  onAdd={add}
                  onClose={() => {
                    focusAfter.current = "add";
                    flip(() => setDraftSpan(null));
                  }}
                />
              </div>
            )}
            {draftSpan === null && (isEditing || layout.widgets.length === 0) && (
              <div className="widget-cell" data-widget-id="add">
                <button className="widget-add" type="button" onClick={() => flip(() => { setEditing(true); setDraftSpan(1); })}>
                  <Icon name="plus" small />
                  Добавить виджет
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </main>
  );
}
