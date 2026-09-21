/* Runlith desktop — Task Workspace, Variant C on the light Cursor/Warp
   direction (user preference 2026-09-21): sidebar on canvas, top command
   bar, white cards. Inspector rules unchanged (§11-C): explicit open/pin/
   close, no self-opening, no focus stealing. Fixture data only. */
import { useCallback, useEffect, useState } from "react";
import { FIXTURE, type PanelId } from "./fixture";
import { Badge, Button, cn, Tabs } from "./components/ui";
import { ChangesPanel, EvidencePanel, OverviewPanel, PlanPanel } from "./components/panels";
import { AttemptsList, HistoryPanel, LogsView } from "./components/diagnostics";
import { OverviewHero } from "./components/overview";
import { Nav } from "./components/nav";
import { Inspector } from "./components/inspector";
import { TopBar } from "./components/topbar";

const TASK = FIXTURE.task;

const TABS: { id: PanelId; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "plan", label: "Plan" },
  { id: "changes", label: "Changes" },
  { id: "evidence", label: "Evidence" },
  { id: "history", label: "History" },
];

const DIAG_TABS = [
  { id: "attempts", label: "Attempts" },
  { id: "events", label: "Events" },
  { id: "logs", label: "Logs" },
] as const;

export default function App() {
  const [tab, setTab] = useState<PanelId>("overview");
  const [diagTab, setDiagTab] = useState<(typeof DIAG_TABS)[number]["id"]>("attempts");
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [diagOpen, setDiagOpen] = useState(false);
  const [density, setDensity] = useState("default");
  const [textSize, setTextSize] = useState("normal");

  useEffect(() => {
    document.documentElement.dataset.density = density;
    document.documentElement.dataset.text = textSize;
  }, [density, textSize]);

  const toggleInspector = useCallback(() => setInspectorOpen((o) => !o), []);
  const closeInspector = useCallback((e: React.MouseEvent<HTMLButtonElement>) => {
    setInspectorOpen(false);
    document.getElementById("btn-inspector")?.focus();
    e.preventDefault();
  }, []);
  const openDecision = useCallback(() => {
    setInspectorOpen(true);
    setPinned(true);
  }, []);
  const openDiagnostics = useCallback(() => setDiagOpen(true), []);

  return (
    <div className="relative flex h-full p-3">
      <Nav density={density} textSize={textSize} onDensity={setDensity} onTextSize={setTextSize} />

      <div className="flex min-w-0 flex-1 flex-col">
        <TopBar />

        <main className="flex min-h-0 flex-1 flex-col px-6 pb-3">
          {tab === "overview" && <OverviewHero onNeedsAttention={openDecision} />}

          <header className={cn(tab === "overview" && "border-t border-edge pt-4")}>
            <div className="flex flex-wrap items-baseline gap-3">
              <h2 className="text-lg font-semibold leading-6">{TASK.objective}</h2>
              <Badge state={TASK.lifecycle}>{TASK.lifecycle}</Badge>
              <span className="text-xs text-ink2">
                {TASK.headlineReason} · {TASK.nextCommand}
              </span>
            </div>
            <p className="mt-0.5 text-xs text-ink2">
              {TASK.id} · scope-bound workspace · attempt identity persists across provider sessions
            </p>
          </header>

          <Tabs tabs={TABS} active={tab} onChange={(id) => setTab(id as PanelId)} label="Task sections" className="!px-0" />

          <div className="min-h-0 flex-1 overflow-y-auto py-4 pr-1" role="tabpanel" aria-label={tab}>
            {tab === "overview" && <OverviewPanel onOpenDiagnostics={openDiagnostics} onOpenDecision={openDecision} />}
            {tab === "plan" && <PlanPanel />}
            {tab === "changes" && <ChangesPanel />}
            {tab === "evidence" && <EvidencePanel />}
            {tab === "history" && <HistoryPanel />}
          </div>

          {/* Inspector & diagnostics: explicit controls with text labels (§11-C). */}
          <div className="flex items-center gap-2 border-t border-edge py-2">
            <Button id="btn-inspector" aria-expanded={inspectorOpen} aria-controls="inspector" onClick={toggleInspector}>
              {inspectorOpen ? "Close agent" : "Open agent"}
            </Button>
            <Button aria-pressed={pinned} onClick={() => setPinned((p) => !p)} className={cn(pinned && "border-accent text-accent")}>
              Pin agent
            </Button>
            <Button aria-expanded={diagOpen} aria-controls="diagnostics" onClick={() => setDiagOpen((d) => !d)}>
              {diagOpen ? "Close diagnostics" : "Open diagnostics"}
            </Button>
            <span className="flex-1" />
            <span className="hidden text-xs text-ink2 xl:block">
              The agent panel never opens itself and never takes keyboard focus.
            </span>
          </div>

          {diagOpen && (
            <section id="diagnostics" aria-label="Diagnostics" className="card mb-1 flex h-[220px] flex-col overflow-hidden">
              <Tabs
                tabs={DIAG_TABS.map((d) => ({ ...d }))}
                active={diagTab}
                onChange={(id) => setDiagTab(id as typeof diagTab)}
                label="Diagnostics sections"
              />
              <div className="min-h-0 flex-1 overflow-y-auto">
                {diagTab === "attempts" && <AttemptsList />}
                {diagTab === "events" && <HistoryPanel />}
                {diagTab === "logs" && <LogsView />}
              </div>
            </section>
          )}
        </main>
      </div>

      {/* ---------- agent panel (explicit open/pin/close only) ---------- */}
      {inspectorOpen && <Inspector onClose={closeInspector} />}
    </div>
  );
}