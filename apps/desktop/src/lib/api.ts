import { invoke } from "@tauri-apps/api/core";
import type { ConnectionDto } from "./bindings/ConnectionDto";
import type { DashboardDto } from "./bindings/DashboardDto";
import type { DashboardLayout } from "./bindings/DashboardLayout";
import type { DecisionDto } from "./bindings/DecisionDto";
import type { MetricDto } from "./bindings/MetricDto";
import type { MetricSource } from "./bindings/MetricSource";
import type { ProjectDto } from "./bindings/ProjectDto";
import type { TaskDto } from "./bindings/TaskDto";
import type { UiError } from "./bindings/UiError";
import type { WidgetDraftDto } from "./bindings/WidgetDraftDto";
import { demo, demoScenario } from "./demo";
import { widgetDemo } from "./widgetDemo";

/** True inside the Tauri webview; false in a plain browser (preview, e2e). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Where a value came from. Demo values are always labelled in the UI. */
export type Source = "live" | "demo";

export interface Loaded<T> {
  data: T;
  source: Source;
}

function isUiError(value: unknown): value is UiError {
  return typeof value === "object" && value !== null && "kind" in value && "message" in value;
}

export function toUiError(error: unknown): UiError {
  if (isUiError(error)) return error;
  return { kind: "internal", message: error instanceof Error ? error.message : String(error) };
}

async function call<T>(
  command: string,
  args: Record<string, unknown> | undefined,
  fallback: () => T,
): Promise<Loaded<T>> {
  if (!inTauri) {
    const scenario = demoScenario();
    if (scenario === "unavailable") {
      throw { kind: "unavailable", message: "agentd is not running; start it with `agentctl start`" } satisfies UiError;
    }
    if (scenario === "loading") return new Promise<Loaded<T>>(() => undefined);
    return { data: fallback(), source: "demo" };
  }
  try {
    return { data: await invoke<T>(command, args), source: "live" };
  } catch (error: unknown) {
    throw toUiError(error);
  }
}

export const api = {
  connection: (): Promise<Loaded<ConnectionDto>> =>
    call("connection_status", undefined, () => demo.connection),
  projects: (): Promise<Loaded<ProjectDto[]>> => call("list_projects", undefined, () => demo.projects),
  tasks: (projectId: string): Promise<Loaded<TaskDto[]>> =>
    call("list_tasks", { projectId }, () => demo.tasks.filter((task) => task.projectId === projectId)),
  dependencies: (taskId: string): Promise<Loaded<string[]>> =>
    call("list_dependencies", { taskId }, () => demo.dependencies[taskId] ?? []),
  decisions: (): Promise<Loaded<DecisionDto[]>> =>
    call("list_human_decisions", undefined, () => demo.decisions),
  widgetDraftStart: (description: string): Promise<Loaded<WidgetDraftDto>> =>
    call("widget_draft_start", { description }, () => widgetDemo.start(description)),
  widgetDraftStatus: (jobId: string): Promise<Loaded<WidgetDraftDto>> =>
    call("widget_draft_status", { jobId }, () => widgetDemo.status(jobId)),
  widgetDraftCancel: (jobId: string): Promise<Loaded<WidgetDraftDto>> =>
    call("widget_draft_cancel", { jobId }, () => widgetDemo.cancel(jobId)),
  dashboardLoad: (): Promise<Loaded<DashboardDto>> => call("dashboard_load", undefined, () => widgetDemo.load()),
  dashboardSave: (layout: DashboardLayout): Promise<Loaded<null>> =>
    call("dashboard_save", { layout }, () => widgetDemo.save(layout)),
  dashboardStock: (): Promise<Loaded<DashboardLayout>> => call("dashboard_stock", undefined, () => widgetDemo.stock()),
  metricRead: (source: MetricSource, everySeconds: number): Promise<Loaded<MetricDto>> =>
    call("metric_read", { source, everySeconds }, () => widgetDemo.metric(source)),
};
