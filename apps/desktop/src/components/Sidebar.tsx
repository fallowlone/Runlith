import type { ReactNode } from "react";
import type { ConnectionDto } from "../lib/bindings/ConnectionDto";
import type { ProjectDto } from "../lib/bindings/ProjectDto";
import { inTauri } from "../lib/api";
import type { Route } from "../lib/router";
import { Icon, type IconName } from "./Icon";

type Props = {
  route: Route;
  navigate: (route: Route) => void;
  taskCount: number | null;
  attentionCount: number | null;
  connection: ConnectionDto | null;
  projects: ProjectDto[];
  projectId: string | null;
  onProject: (id: string) => void;
};

const CONNECTION_VIEW: Record<ConnectionDto["state"], { dot: string; label: string }> = {
  connected: { dot: "live", label: "agentd подключён" },
  unavailable: { dot: "live off", label: "agentd не запущен" },
  incompatible: { dot: "live warn", label: "версия не совпадает" },
  security: { dot: "live off", label: "endpoint заблокирован" },
  error: { dot: "live off", label: "ошибка связи" },
};

function NavLink({ to, icon, label, current, navigate, children }: {
  to: Route; icon: IconName; label: string; current: boolean; navigate: (route: Route) => void; children?: ReactNode;
}) {
  return (
    <a
      href="#"
      title={label}
      aria-current={current ? "page" : undefined}
      onClick={(event) => {
        event.preventDefault();
        navigate(to);
      }}
    >
      <Icon name={icon} />
      <span className="nav-label">{label}</span>
      {children}
    </a>
  );
}

export function Sidebar({ route, navigate, taskCount, attentionCount, connection, projects, projectId, onProject }: Props) {
  const project = projects.find((item) => item.id === projectId);
  const conn = connection ? CONNECTION_VIEW[connection.state] : { dot: "live warn", label: "проверка связи" };
  const inTasks = route.name === "tasks" || route.name === "task";

  return (
    <nav className="tile side" aria-label="Основная навигация">
      {inTauri ? (
        <div data-tauri-drag-region style={{ height: 28 }} />
      ) : (
        <div className="lights" aria-hidden="true"><i /><i /><i /></div>
      )}
      <label className="proj" style={{ position: "relative", cursor: projects.length ? "pointer" : "default" }}>
        <span className="mark" aria-hidden="true">{(project?.name ?? "R").slice(0, 1).toUpperCase()}</span>
        <span className="proj-name">
          <span className="title-4 ellip">{project?.name ?? "Нет проекта"}</span>
          <span className="caption-5 muted">{projects.length} в agentd</span>
        </span>
        <Icon name="caretDown" small className="muted" />
        <select
          aria-label="Проект"
          value={projectId ?? ""}
          disabled={projects.length === 0}
          onChange={(event) => onProject(event.target.value)}
          style={{ position: "absolute", inset: 0, opacity: 0, cursor: "inherit" }}
        >
          {projects.map((item) => (
            <option key={item.id} value={item.id}>{item.name}</option>
          ))}
        </select>
      </label>
      <div className="nav">
        <NavLink to={{ name: "overview" }} icon="house" label="Обзор" current={route.name === "overview"} navigate={navigate} />
        <NavLink to={{ name: "tasks" }} icon="listChecks" label="Задачи" current={inTasks} navigate={navigate}>
          {taskCount !== null && <span className="count">{taskCount}</span>}
        </NavLink>
        <NavLink to={{ name: "attention" }} icon="bellSimple" label="Внимание" current={route.name === "attention"} navigate={navigate}>
          {attentionCount ? <span className="badge">{attentionCount}</span> : null}
        </NavLink>
        <NavLink to={{ name: "map" }} icon="graph" label="Карта" current={route.name === "map"} navigate={navigate} />
      </div>
      <div className="side-foot">
        <a
          className="navlink"
          href="#"
          title="Настройки"
          aria-current={route.name === "settings" ? "page" : undefined}
          onClick={(event) => {
            event.preventDefault();
            navigate({ name: "settings" });
          }}
        >
          <Icon name="gearSix" />
          <span className="nav-label">Настройки</span>
        </a>
        <a
          className="conn"
          href="#"
          title={conn.label}
          aria-current={route.name === "connection" ? "page" : undefined}
          style={route.name === "connection" ? { background: "var(--raise-2)", color: "var(--text)" } : undefined}
          onClick={(event) => {
            event.preventDefault();
            navigate({ name: "connection" });
          }}
        >
          <span className={conn.dot} aria-hidden="true" />
          <span className="nav-label ellip">{conn.label}</span>
        </a>
      </div>
    </nav>
  );
}
