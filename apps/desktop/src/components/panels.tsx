/* Task Workspace panels — content is identical regardless of where the
   layout places it (research §9.1, §13). Ported from the D05 proto.js. */
import { FIXTURE } from "../fixture";
import { Badge, Button, Callout, Group } from "./ui";

const t = FIXTURE.task;

/* ---------- Overview ---------- */

export function OverviewPanel({ onOpenDiagnostics, onOpenDecision }: { onOpenDiagnostics: () => void; onOpenDecision: () => void }) {
  return (
    <div>
      <Callout tone="info">
        <strong>{t.lifecycle}</strong> — {t.headlineReason}. <span className="text-ink2">{t.nextCommand}</span>
      </Callout>

      <Group title="Execution / Attempts">
        <p className="mb-2 text-xs leading-4 text-ink2">{t.execution.observation}</p>
        {t.attempts.map((a) => (
          <div key={a.n} className="row-h flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 rounded-[4px] px-1 py-1 hover:bg-surface">
            <Badge state={a.outcome === "SUCCEEDED" ? "COMPLETED" : "FAILED"}>{a.outcome}</Badge>
            <span className="min-w-0 flex-1 basis-48 truncate">
              Attempt {a.n} — {a.note}
            </span>
            <span className="shrink-0 text-xs text-ink2">{a.ended}</span>
            <Button size="sm" onClick={onOpenDiagnostics}>
              View diagnostics
            </Button>
          </div>
        ))}
      </Group>

      <Group title="Blockers">
        <p>{t.childBlocker.text}</p>
        <div className="mt-2">
          <Button onClick={onOpenDecision}>Open decision {FIXTURE.attention[0].id}</Button>
        </div>
      </Group>

      <Group title="Scope">
        <div className="flex flex-wrap gap-2">
          {t.scope.map((s) => (
            <span key={s} className="rounded-full border border-white/15 px-2 py-0.5 font-mono text-xs text-ink2">
              {s}
            </span>
          ))}
        </div>
      </Group>
    </div>
  );
}

/* ---------- Plan (§9.3: list first; accepted vs proposal distinct) ---------- */

export function PlanPanel() {
  const p = t.plan;
  return (
    <div>
      <Group title={`Accepted revision — ${p.acceptedRevision} (head ${p.head})`}>
        <p>Accepted plan head of the current branch. Provenance: {p.provenance}.</p>
        <p className="mt-2 text-xs leading-4 text-ink2">
          Plan list is the default view (§9.3); the graph is an analysis view, not the only way to read the plan.
        </p>
      </Group>
      <Group title={`Proposed change set ${p.proposed.id} — proposal, not the acting plan`}>
        <p>
          {p.proposed.delta} — {p.proposed.summary}
        </p>
        <p className="mt-2 text-xs leading-4 text-ink2">
          Risk: {p.proposed.risk}. {p.proposed.validation}. A proposal never renders as an already-accepted revision.
        </p>
        <div className="mt-3">
          <Button>Review change set</Button>
        </div>
      </Group>
    </div>
  );
}

/* ---------- Changes (diff names its base revision, §9.2/§14.5) ---------- */

export function ChangesPanel() {
  const c = t.changes;
  return (
    <div>
      <p className="mb-2 text-xs leading-4 text-ink2">
        Diff against recorded base revision {c.baseRevision} — never silently compared against current main (§9.2).
      </p>
      {c.files.map((f) => (
        <div key={f.path} className="mb-3 overflow-hidden rounded-[6px] border border-edge">
          <div className="bg-surface px-3 py-2 font-mono text-xs">{f.path}</div>
          <pre className="overflow-x-auto px-3 py-2 font-mono text-xs leading-5">
            {f.hunks.map((h, i) => (
              <span key={i} className={h.t === "add" ? "diff-add" : h.t === "del" ? "diff-del" : "diff-ctx"}>
                {h.l}
                {"\n"}
              </span>
            ))}
          </pre>
        </div>
      ))}
      <p className="text-xs leading-4 text-ink2">
        Unified diff shown; side-by-side appears at sufficient width per user preference (§14.5).
      </p>
    </div>
  );
}

/* ---------- Evidence (criterion-oriented, §13.6) ---------- */

export function EvidencePanel() {
  return (
    <div>
      {/* Tables need a bounded scroll container: columns degrade to horizontal
          scrolling instead of breaking the card (WCAG reflow, §15). */}
      <div className="-mx-1 overflow-x-auto px-1">
        <table className="w-full min-w-[640px] border-collapse text-sm">
          <thead>
            <tr className="border-b border-edge text-left text-xs text-ink2">
              <th className="px-3 py-1.5 font-semibold">Criterion</th>
              <th className="px-3 py-1.5 font-semibold">Verdict</th>
              <th className="px-3 py-1.5 font-semibold">Snapshot</th>
              <th className="px-3 py-1.5 font-semibold">Producer</th>
              <th className="px-3 py-1.5 font-semibold">When</th>
              <th className="px-3 py-1.5 font-semibold">Links</th>
            </tr>
          </thead>
          <tbody>
            {t.criteria.map((c) => (
              <tr key={c.id} className="border-b border-edge/50 align-top">
                <td className="px-3 py-2">
                  {c.id} — {c.text}
                </td>
                <td className={`px-3 py-2 font-medium ${c.verdict === "VERIFIED" ? "text-positive" : "text-ink2"}`}>
                  {c.verdict}
                  {c.note ? <div className="mt-0.5 text-xs font-normal leading-4 text-ink2">{c.note}</div> : null}
                </td>
                <td className="px-3 py-2 font-mono text-xs">{c.snapshot ?? "—"}</td>
                <td className="px-3 py-2">{c.producer ?? "—"}</td>
                <td className="px-3 py-2 whitespace-nowrap">{c.when ?? "—"}</td>
                <td className="px-3 py-2">{c.links.length ? c.links.join(", ") : "none yet"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <Callout tone="attention" title="Staleness note.">
        {t.staleEvidence}
      </Callout>
      <p className="text-xs leading-4 text-ink2">
        Evidence is organized by acceptance criterion, not by file type (§13.6). A VERIFIED verdict belongs to its
        recorded snapshot/revision and is never rewritten by later changes.
      </p>
    </div>
  );
}