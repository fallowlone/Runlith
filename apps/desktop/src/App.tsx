/* Runlith desktop — Task Workspace, Variant C (adaptive inspector).
   Layout per research §11-C with its viability conditions: explicit
   open/pin/close with text labels, no self-opening, no focus stealing,
   Attention always visible in the nav. Fixture data only (see fixture.ts). */
import { useCallback, useEffect, useState } from "react";
import { FIXTURE, type PanelId } from "./fixture";
import { Badge, Button, cn, Tabs } from "./components/ui";
import { ChangesPanel, EvidencePanel, OverviewPanel, PlanPanel } from "./components/panels";
import { AttemptsList, HistoryPanel, LogsView } from "./components/diagnostics";
import { Nav } from "./components/nav";
import { Inspector } from "./components/inspector";

const F = FIXTURE;
const TASK = F.task;

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

  // Variant C viability: the inspector never opens itself and never takes
  // focus; closing returns focus to the initiator (§11-C, §15.2).
  const toggleInspector = useCallback(() => setInspectorOpen((o) => !o), []);
  const closeInspector = useCallback((e: React.MouseEvent<HTMLButtonElement>) => {
    setInspectorOpen(false);
    document.getElementById("btn-inspector")?.focus();
    e.preventDefault();
  }, []);

  // Opening the decision surface pins the inspector: reading a pending
  // decision is exactly the pinned use case (§11-C); still user-initiated.
  const openDecision = useCallback(() => {
    setInspectorOpen(true);
    setPinned(true);
  }, []);
  const openDiagnostics = useCallback(() => setDiagOpen(true), []);

  return (
    <div className="flex h-full gap-3 bg-canvas p-3">
      <Nav density={density} textSize={textSize} onDensity={setDensity} onTextSize={setTextSize} />

      {/* ---------- Task Workspace ---------- */}
      <main className="flex min-w-0 flex-1 flex-col rounded-[6px] bg-shell">
        <header className="px-4 pb-2 pt-4">
          <div className="flex flex-wrap items-baseline gap-3">
            <h1 className="text-xl font-semibold leading-7">{TASK.objective}</h1>
            <Badge state={TASK.lifecycle}>{TASK.lifecycle}</Badge>
            <span className="text-xs text-ink2">
              {TASK.headlineReason} · {TASK.nextCommand}
            </span>
          </div>
          <p className="mt-0.5 text-xs text-ink2">
            {TASK.id} · scope-bound workspace · attempt identity persists across provider sessions
          </p>
        </header>

        <Tabs tabs={TABS} active={tab} onChange={(id) => setTab(id as PanelId)} label="Task sections" />

        <div className="min-h-0 flex-1 overflow-y-auto p-4" role="tabpanel" aria-label={tab}>
          {tab === "overview" && <OverviewPanel onOpenDiagnostics={openDiagnostics} onOpenDecision={openDecision} />}
          {tab === "plan" && <PlanPanel />}
          {tab === "changes" && <ChangesPanel />}
          {tab === "evidence" && <EvidencePanel />}
          {tab === "history" && <HistoryPanel />}
        </div>

        {/* Inspector & diagnostics: explicit controls with text labels (§11-C). */}
        <div className="flex items-center gap-2 border-t border-edge px-4 py-2">
          <Button id="btn-inspector" aria-expanded={inspectorOpen} aria-controls="inspector" onClick={toggleInspector}>
            {inspectorOpen ? "Close inspector" : "Open inspector"}
          </Button>
          <Button aria-pressed={pinned} onClick={() => setPinned((p) => !p)} className={cn(pinned && "border-accent text-accent")}>
            Pin inspector
          </Button>
          <Button aria-expanded={diagOpen} aria-controls="diagnostics" onClick={() => setDiagOpen((d) => !d)}>
            {diagOpen ? "Close diagnostics" : "Open diagnostics"}
          </Button>
          <span className="flex-1" />
          <span className="hidden text-xs text-ink2 xl:block">
            Inspector never opens itself and never takes keyboard focus.
          </span>
        </div>

        {diagOpen && (
          <section id="diagnostics" aria-label="Diagnostics" className="mx-4 mb-3 flex h-[220px] flex-col rounded-[6px] bg-surface">
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

      {/* ---------- inspector (explicit open/pin/close only) ---------- */}
      {inspectorOpen && <Inspector onClose={closeInspector} />}
    </div>
  );
}