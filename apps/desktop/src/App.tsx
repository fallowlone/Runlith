import { useEffect, useRef, useState } from "react";
import type { ConnectionDto } from "./lib/bindings/ConnectionDto";
import type { TaskDto } from "./lib/bindings/TaskDto";
import { api, inTauri, type Loaded } from "./lib/api";
import { useRouter, type Route } from "./lib/router";
import { useSettings } from "./lib/settings";
import { useResource } from "./lib/useResource";
import { AgentPanel } from "./components/AgentPanel";
import { Sidebar } from "./components/Sidebar";
import { Attention } from "./views/Attention";
import { Connection } from "./views/Connection";
import { MapView } from "./views/MapView";
import { Overview } from "./views/Overview";
import { Settings } from "./views/Settings";
import { TaskWorkspace } from "./views/TaskWorkspace";
import { Tasks } from "./views/Tasks";

const POLL_MS = 5000;

/** Polls agentd's handshake so a lost or restarted daemon shows up within seconds. */
function useConnection(): [ConnectionDto | null, Date | null, () => void] {
  const [connection, setConnection] = useState<ConnectionDto | null>(null);
  const [observedAt, setObservedAt] = useState<Date | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let alive = true;
    const probe = (): void => {
      api.connection().then(
        ({ data }) => {
          if (!alive) return;
          setConnection(data);
          setObservedAt(new Date());
        },
        (error: unknown) => {
          if (!alive) return;
          const message = error instanceof Error ? error.message : String((error as { message?: string }).message ?? error);
          setConnection({ state: "unavailable", protocol: null, daemonBuild: null, clientBuild: "0.1.0", detail: message });
          setObservedAt(new Date());
        },
      );
    };
    probe();
    const id = window.setInterval(probe, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [tick]);

  return [connection, observedAt, () => setTick((value) => value + 1)];
}

function agentContext(route: Route): string {
  switch (route.name) {
    case "task": return route.id;
    case "tasks": return "задачи";
    case "attention": return "решения";
    default: return "весь проект";
  }
}

export function App() {
  const [settings, update, theme] = useSettings();
  const [route, navigate] = useRouter();
  const [connection, observedAt, recheck] = useConnection();
  const [query, setQuery] = useState("");
  const [projectId, setProjectId] = useState<string | null>(null);
  const [askFiles, setAskFiles] = useState<string[]>([]);
  const isConnected = connection?.state === "connected";

  const [projects, reloadProjects] = useResource(api.projects, `projects:${isConnected}`);
  const projectList = projects.status === "ok" ? projects.data : [];
  const activeProject = projectList.find((item) => item.id === projectId) ?? projectList[0] ?? null;

  const loadTasks = (): Promise<Loaded<TaskDto[]>> => {
    if (projects.status === "loading") return new Promise<Loaded<TaskDto[]>>(() => undefined);
    if (!activeProject) return Promise.resolve({ data: [], source: projects.status === "ok" ? projects.source : "live" });
    return api.tasks(activeProject.id);
  };
  const [tasks, reloadTasks] = useResource(loadTasks, `tasks:${activeProject?.id ?? "none"}:${isConnected}:${projects.status}`);
  const [decisions, reloadDecisions] = useResource(api.decisions, `decisions:${isConnected}`);
  const tasksForProject = projects.status === "error" ? projects : tasks;

  // Keyboard: ⌘K search, ⌘[ back to tasks, ⌘⌥B toggles the agent panel.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (!event.metaKey) return;
      if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        const input = document.getElementById("global-search");
        if (input) input.focus();
        else navigate({ name: "tasks" });
      } else if (event.key === "[") {
        event.preventDefault();
        navigate({ name: "tasks" });
      } else if (event.altKey && event.code === "KeyB") {
        event.preventDefault();
        update({ agentOpen: !settingsRef.current.agentOpen });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navigate, update]);

  // After each screen change, move focus to the new heading for screen readers.
  const firstRender = useRef(true);
  useEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      return;
    }
    document.querySelector<HTMLElement>("main h1")?.focus({ preventScroll: true });
  }, [route]);

  // Attachments belong to the task being discussed, not to the whole session.
  const discussedTask = route.name === "task" ? route.id : null;
  useEffect(() => setAskFiles([]), [discussedTask]);
  const askAboutFile = (path: string): void => {
    setAskFiles((current) => (current.includes(path) ? current : [...current, path]));
    update({ agentOpen: true });
  };

  const reloadAll = (): void => {
    reloadProjects();
    reloadTasks();
    reloadDecisions();
  };

  const showAgent = settings.agentOpen && !["map", "settings", "connection"].includes(route.name);
  const taskList = tasks.status === "ok" ? tasks.data : [];
  const decisionCount = decisions.status === "ok" ? decisions.data.length : null;

  let screen;
  switch (route.name) {
    case "overview":
      screen = <Overview projectName={activeProject?.name ?? null} tasks={tasksForProject} decisions={decisions} reload={reloadAll} navigate={navigate} query={query} onQuery={setQuery} />;
      break;
    case "tasks":
      screen = <Tasks tasks={tasksForProject} reload={reloadAll} navigate={navigate} query={query} onQuery={setQuery} hasProject={activeProject !== null} />;
      break;
    case "task":
      screen = <TaskWorkspace key={route.id} taskId={route.id} task={taskList.find((task) => task.id === route.id)} tab={route.tab} navigate={navigate} onAskFile={askAboutFile} />;
      break;
    case "attention":
      screen = <Attention decisions={decisions} reload={reloadDecisions} navigate={navigate} query={query} onQuery={setQuery} />;
      break;
    case "map":
      screen = <MapView />;
      break;
    case "settings":
      screen = <Settings settings={settings} update={update} />;
      break;
    case "connection":
      screen = <Connection connection={connection} observedAt={observedAt} onCheck={() => { recheck(); reloadAll(); }} />;
      break;
  }

  return (
    <div
      className={`rl app${inTauri ? " in-tauri" : ""}`}
      data-theme={theme}
      data-density={settings.density}
      data-text={settings.textSize}
      data-motion={settings.reduceMotion ? "reduce" : undefined}
    >
      <div className={`win${showAgent ? "" : " no-agent"}`}>
        <Sidebar
          route={route}
          navigate={navigate}
          taskCount={tasks.status === "ok" ? taskList.length : null}
          attentionCount={decisionCount}
          connection={connection}
          projects={projectList}
          projectId={activeProject?.id ?? null}
          onProject={setProjectId}
        />
        {screen}
        {showAgent && (
          <AgentPanel
            context={agentContext(route)}
            onClose={() => update({ agentOpen: false })}
            files={askFiles}
            onRemoveFile={(path) => setAskFiles((current) => current.filter((item) => item !== path))}
          />
        )}
      </div>
    </div>
  );
}
