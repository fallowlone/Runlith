/**
 * Browser preview and e2e stand-ins for widget authoring. Used only outside
 * Tauri; every card built from them says "демо". `?demo=` picks a failure state.
 */
import type { DashboardDto } from "./bindings/DashboardDto";
import type { DashboardLayout } from "./bindings/DashboardLayout";
import type { MetricDto } from "./bindings/MetricDto";
import type { MetricSource } from "./bindings/MetricSource";
import type { RepoName } from "./bindings/RepoName";
import type { WidgetDraftDto } from "./bindings/WidgetDraftDto";

/** Separate from the app-wide `demoScenario`: these only reach the widget stand-ins. */
const demoScenario = (): string | null => new URLSearchParams(window.location.search).get("demo");

const LAYOUT_KEY = "runlith.dashboard.demo.v1";
const THINK_MS = 700;

const STOCK: DashboardLayout = {
  version: 1,
  widgets: [
    { kind: "block", id: "summary", span: 1, block: "summary" },
    { kind: "block", id: "states", span: 1, block: "states" },
    { kind: "block", id: "graph", span: 2, block: "graph" },
    { kind: "block", id: "commits", span: 1, block: "commits" },
    { kind: "block", id: "branch", span: 1, block: "branch" },
    { kind: "block", id: "attention", span: 2, block: "attention" },
  ],
};

const FAILURES: Record<string, [string, string]> = {
  "draft-missing": ["provider_missing", "the claude CLI was not found"],
  "draft-incompatible": ["provider_incompatible", "the installed claude CLI has no --json-schema option; update it"],
  "draft-auth": ["not_authenticated", "provider_credential_vault_absent: no operator vault exists; run `agentctl vault init`, then `agentctl vault store`"],
  "draft-timeout": ["timed_out", "claude execution exceeded 60s; child killed"],
  "draft-bad": ["bad_output", "the provider's answer did not pass validation"],
  "draft-outage": ["provider_unavailable", "exit 1: overloaded"],
  "draft-failed": ["failed", "exit 1: stderr-excerpt"],
};

function classify(text: string): WidgetDraftDto {
  const base: WidgetDraftDto = { jobId: "", state: "refused", source: null, repo: null, failureCode: null, failureDetail: null };
  const failure = FAILURES[demoScenario() ?? ""];
  if (failure) return { ...base, state: "failed", failureCode: failure[0], failureDetail: failure[1] };
  const repo: RepoName | null = text.match(/[A-Za-z0-9-]+\/[A-Za-z0-9._-]+/)?.[0] ?? null;
  if (/https?:|игнорируй|ignore/i.test(text)) return base;
  if (/зв[её]зд|\bstars?\b/i.test(text)) return { ...base, state: "proposed", source: "github.repo.stars", repo };
  if (/пул|pull|реквест|\bprs?\b/i.test(text)) return { ...base, state: "proposed", source: "github.repo.open_prs", repo };
  return base;
}

type DemoJob = { startedAt: number; cancelled: boolean; result: WidgetDraftDto };
const jobs = new Map<string, DemoJob>();
let nextJob = 1;

function viewOf(jobId: string, job: DemoJob): WidgetDraftDto {
  if (job.cancelled) return { ...job.result, jobId, state: "cancelled", source: null, repo: null, failureCode: null, failureDetail: null };
  const stuck = demoScenario() === "draft-slow";
  if (stuck || Date.now() - job.startedAt < THINK_MS) {
    return { jobId, state: "running", source: null, repo: null, failureCode: null, failureDetail: null };
  }
  return { ...job.result, jobId };
}

const demoValues: Record<string, number> = { "github.repo.stars": 128, "github.repo.open_prs": 3 };

/** Lets e2e move a demo number, the way a new star would. */
declare global {
  interface Window {
    __runlithDemoBump?: (source: string, by: number) => void;
  }
}
if (typeof window !== "undefined") {
  window.__runlithDemoBump = (source, by) => {
    demoValues[source] = Math.max(0, (demoValues[source] ?? 0) + by);
  };
}

export const widgetDemo = {
  start(description: string): WidgetDraftDto {
    const jobId = `demo-${nextJob++}`;
    const job = { startedAt: Date.now(), cancelled: false, result: classify(description) };
    jobs.set(jobId, job);
    return viewOf(jobId, job);
  },
  status(jobId: string): WidgetDraftDto {
    const job = jobs.get(jobId);
    if (!job) throw { kind: "business", message: `no widget draft ${jobId}` };
    return viewOf(jobId, job);
  },
  cancel(jobId: string): WidgetDraftDto {
    const job = jobs.get(jobId);
    if (!job) throw { kind: "business", message: `no widget draft ${jobId}` };
    job.cancelled = true;
    return viewOf(jobId, job);
  },
  load(): DashboardDto {
    if (demoScenario() === "dashboard-corrupt") return { layout: STOCK, recoveredCopy: "dashboard.corrupt-1793540000000.json" };
    try {
      const saved = localStorage.getItem(LAYOUT_KEY);
      if (saved) return { layout: JSON.parse(saved) as DashboardLayout, recoveredCopy: null };
    } catch {
      // Blocked or broken storage only costs the demo its layout.
    }
    return { layout: STOCK, recoveredCopy: null };
  },
  save(layout: DashboardLayout): null {
    if (demoScenario() === "dashboard-save-fails") throw { kind: "internal", message: "could not save the layout: disk full" };
    try {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
    } catch {
      // Same: the demo keeps working for this page.
    }
    return null;
  },
  stock(): DashboardLayout {
    return STOCK;
  },
  metric(source: MetricSource): MetricDto {
    const now = Date.now();
    const value = demoValues[source.type] ?? 0;
    switch (demoScenario()) {
      case "metric-offline":
        return { value, fetchedAtMs: now - 720_000, state: "offline", retryAtMs: now + 300_000 };
      case "metric-limited":
        return { value, fetchedAtMs: now - 720_000, state: "rateLimited", retryAtMs: now + 720_000 };
      case "metric-notfound":
        return { value: null, fetchedAtMs: null, state: "notFound", retryAtMs: now + 300_000 };
      default:
        return { value, fetchedAtMs: now, state: "fresh", retryAtMs: now + 300_000 };
    }
  },
};
