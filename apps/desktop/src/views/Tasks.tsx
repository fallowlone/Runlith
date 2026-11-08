import { useState, type KeyboardEvent } from "react";
import type { TaskDto } from "../lib/bindings/TaskDto";
import type { Resource } from "../lib/useResource";
import type { Route } from "../lib/router";
import { GROUP_LABELS, presentState, type StateGroup } from "../lib/taskState";
import { Empty, ErrorState, SkeletonRows, SourceChip, StateLabel } from "../components/Feedback";
import { Icon } from "../components/Icon";
import { TopBar } from "../components/TopBar";

type Filter = "all" | StateGroup;

type Props = {
  tasks: Resource<TaskDto[]>;
  reload: () => void;
  navigate: (route: Route) => void;
  query: string;
  onQuery: (value: string) => void;
  hasProject: boolean;
};

const FILTERS: Filter[] = ["all", "ready", "active", "waiting", "closed"];
// event.code, not event.key: J and K must work on the Russian layout too.
const ROW_KEYS: Record<string, number> = { KeyJ: 1, ArrowDown: 1, KeyK: -1, ArrowUp: -1 };

/** Single-character keys act only while focus is inside the list (WCAG 2.1.4). */
function moveRowFocus(event: KeyboardEvent<HTMLDivElement>): void {
  const step = ROW_KEYS[event.code];
  if (!step || event.metaKey || event.ctrlKey || event.altKey) return;
  const rows = [...event.currentTarget.querySelectorAll<HTMLElement>("a.tr")];
  const next = rows[rows.indexOf(document.activeElement as HTMLElement) + step];
  if (next) {
    event.preventDefault();
    next.focus();
  }
}

function matchesQuery(task: TaskDto, query: string): boolean {
  const needle = query.trim().toLowerCase();
  return !needle || task.id.toLowerCase().includes(needle) || task.objective.toLowerCase().includes(needle);
}

/** Dependencies that exist in this list and are not completed. Derived from real state, never from a renamed lifecycle name. */
function openDependencies(task: TaskDto, byId: Map<string, TaskDto>): string[] {
  return task.dependsOn.filter((id) => {
    const dependency = byId.get(id);
    return dependency !== undefined && dependency.state !== "COMPLETED";
  });
}

function inFilter(task: TaskDto, filter: Filter): boolean {
  return filter === "all" || presentState(task.state).group === filter;
}

function DependsCell({ task, blockedBy }: { task: TaskDto; blockedBy: string[] }) {
  const waits = blockedBy.length > 0 && !["closed", "active"].includes(presentState(task.state).group);
  return (
    <span role="cell" className="mono muted ellip" title={task.dependsOn.join(", ")}>
      {task.dependsOn.length === 0 ? "—" : task.dependsOn.join(", ")}
      {waits && <span className="state wait"> · ждёт зависимости</span>}
    </span>
  );
}

export function Tasks({ tasks, reload, navigate, query, onQuery, hasProject }: Props) {
  const [filter, setFilter] = useState<Filter>("all");
  const [activeId, setActiveId] = useState<string | null>(null);
  const all = tasks.status === "ok" ? tasks.data : [];
  const byId = new Map(all.map((task) => [task.id, task]));
  // Counters follow the search, so each one says what its filter would show right now.
  const found = all.filter((task) => matchesQuery(task, query));
  const visible = found.filter((task) => inFilter(task, filter));
  // One Tab stop for the whole list, so leaving it never takes hundreds of presses.
  const tabStopId = visible.some((task) => task.id === activeId) ? activeId : visible[0]?.id;
  const closed = all.filter((task) => presentState(task.state).group === "closed").length;
  const closedShare = all.length ? (closed / all.length) * 100 : 0;
  const isFiltered = filter !== "all" || query.trim() !== "";

  return (
    <main className="tile">
      <TopBar query={query} onQuery={onQuery}>
        <button className="btn btn-ghost" type="button" onClick={reload}>
          <Icon name="arrowsClockwise" small />
          Обновить
        </button>
      </TopBar>
      <div className="view">
        <div className="view-head">
          <div className="stack" style={{ gap: 4 }}>
            <h1 className="heading-5" tabIndex={-1}>Задачи</h1>
            <span className="caption-5 muted">Список задач проекта из agentd</span>
          </div>
          {tasks.status === "ok" && <SourceChip source={tasks.source} />}
          <span className="spacer" />
          <div className="seg" role="group" aria-label="Фильтр по состоянию">
            {FILTERS.map((item) => (
              <button key={item} type="button" aria-pressed={filter === item} onClick={() => setFilter(item)}>
                {item === "all" ? "Все" : GROUP_LABELS[item]}
                {tasks.status === "ok" && <span className="count muted">{found.filter((task) => inFilter(task, item)).length}</span>}
              </button>
            ))}
          </div>
        </div>

        {tasks.status === "error" && <ErrorState error={tasks.error} onRetry={reload} />}

        {tasks.status === "ok" && all.length > 0 && (
          <section className="card reveal" aria-labelledby="q-title" style={{ flex: "none" }}>
            <div className="row">
              <h2 id="q-title" className="title-4">Состав</h2>
              <span className="spacer" />
              <span className="caption-5 muted tnum">{closed} закрыто · {all.length - closed} открыто</span>
            </div>
            <div className="meter" style={{ marginTop: 12 }} role="img" aria-label={`${closed} из ${all.length} задач закрыты`}>
              <span style={{ width: `${closedShare}%`, background: "var(--done)" }} />
            </div>
          </section>
        )}

        <section className="card list list-pane reveal" aria-label="Список задач" style={{ ["--i" as string]: 1 }}>
          {tasks.status === "loading" && <SkeletonRows label="Задачи загружаются" />}
          {tasks.status === "ok" && all.length === 0 && (
            <Empty
              icon="listChecks"
              title={hasProject ? "Здесь пока нет задач" : "В agentd нет проектов"}
              text={hasProject
                ? "Следующий шаг: создайте первую задачу, например agentctl task create <id> <project> <цель>."
                : "Создайте проект: agentctl project create <id> <имя>. Интерфейс пока только читает данные."}
            />
          )}
          {tasks.status === "ok" && all.length > 0 && visible.length === 0 && (
            <Empty
              icon="magnifyingGlass"
              title="Ничего не найдено"
              text="Под фильтр и поиск не подходит ни одна задача. Это не ошибка."
              action={isFiltered ? { label: "Сбросить фильтры", onClick: () => { setFilter("all"); onQuery(""); } } : undefined}
            />
          )}
          {visible.length > 0 && (
            <div className="tbl" role="table" aria-label="Задачи" onKeyDown={moveRowFocus}>
              <div className="tr th cols-task-list" role="row">
                <span role="columnheader">Состояние</span>
                <span role="columnheader">ID</span>
                <span role="columnheader">Цель</span>
                <span role="columnheader">Зависит от</span>
              </div>
              {visible.map((task) => (
                <a
                  key={task.id}
                  className={`tr cols-task-list${filter === "all" && presentState(task.state).group === "ready" ? " tr-next" : ""}`}
                  role="row"
                  href="#"
                  tabIndex={task.id === tabStopId ? 0 : -1}
                  onFocus={() => setActiveId(task.id)}
                  onClick={(event) => {
                    event.preventDefault();
                    navigate({ name: "task", id: task.id, tab: "overview" });
                  }}
                >
                  <span role="cell"><StateLabel state={task.state} /></span>
                  <span role="cell" className="mono muted ellip">{task.id}</span>
                  <span role="cell" className="ellip">{task.objective}</span>
                  <DependsCell task={task} blockedBy={openDependencies(task, byId)} />
                </a>
              ))}
            </div>
          )}
          {visible.length > 0 && (
            <div className="list-foot caption-5 muted">
              <span className="tnum" role="status">Показано {visible.length} из {all.length}</span>
              <span className="list-keys">
                <span className="kbd">J</span><span className="kbd">K</span><span>по строкам</span>
                <span className="kbd">Enter</span><span>открыть</span>
              </span>
            </div>
          )}
        </section>
      </div>
    </main>
  );
}
