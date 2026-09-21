/* Plan dependency graph (§9.3, §16 experiment), causal links (§13.7) and the
   Execution Explorer (§9.1). Rules honored:
   - list stays the default Plan view; graph is a secondary analysis view;
   - graph shows the selected branch + nearest dependencies, not the project;
   - text alternative is always available (§15.2);
   - agents appear only in Attempt context — no avatars, no swarm, retries
     stay sequential (§13.7);
   - tree = parent-child, table = comparisons, graph = dependencies only (§14.4). */
import { FIXTURE, type TaskLifecycle } from "../fixture";
import { Badge, Button, PRETTY_STATE } from "./ui";

const G = FIXTURE.planGraph;

interface LaidNode {
  id: string;
  label: string;
  state: string;
  agent: string | null;
  x: number;
  y: number;
}

const NODE_W = 168;
const NODE_H = 44;
const GAP_X = 210;
const GAP_Y = 68;

function layout(): { nodes: LaidNode[]; width: number; height: number } {
  const depth = new Map<string, number>();
  const byId = new Map(G.nodes.map((n) => [n.id, n]));
  const level = (id: string, seen = new Set<string>()): number => {
    if (depth.has(id)) return depth.get(id)!;
    if (seen.has(id)) return 0;
    seen.add(id);
    const node = byId.get(id);
    const d = node && node.deps.length ? Math.max(...node.deps.map((p) => level(p, seen) + 1)) : 0;
    depth.set(id, d);
    return d;
  };
  G.nodes.forEach((n) => level(n.id));
  const perLevel = new Map<number, number>();
  const nodes: LaidNode[] = G.nodes.map((n) => {
    const d = depth.get(n.id) ?? 0;
    const row = perLevel.get(d) ?? 0;
    perLevel.set(d, row + 1);
    return { ...n, x: 16 + d * GAP_X, y: 16 + row * GAP_Y };
  });
  const cols = Math.max(...perLevel.keys()) + 1;
  const rows = Math.max(...perLevel.values());
  return { nodes, width: 32 + cols * GAP_X, height: 32 + rows * GAP_Y };
}

const nodeStyle = (state: string): { fill: string; stroke: string; text: string } => {
  switch (state as TaskLifecycle | "SUCCEEDED") {
    case "SUCCEEDED":
      return { fill: "#16a34a14", stroke: "#16a34a66", text: "#15803d" };
    case "VERIFYING":
    case "RUNNING":
      return { fill: "#2563eb14", stroke: "#2563eb66", text: "#1d4ed8" };
    case "BLOCKED":
      return { fill: "#d9770614", stroke: "#d9770666", text: "#b45309" };
    default:
      return { fill: "#f0f1f3", stroke: "#e5e7eb", text: "#6b7280" };
  }
};

export function PlanGraphView() {
  const { nodes, width, height } = layout();
  const byId = new Map(nodes.map((n) => [n.id, n]));
  return (
    <div>
      <p className="mb-2 text-xs leading-4 text-ink2">
        Dependency graph of the selected branch ({G.branch}) — nearest dependencies only, not the whole project (§9.3).
        A text form of this graph follows below (§15.2).
      </p>
      <div className="overflow-x-auto rounded-[8px] border border-edge p-2">
        <svg width={width} height={height} role="img" aria-label="Plan dependency graph (see text alternative below)">
          {G.nodes.flatMap((n) =>
            n.deps.map((p) => {
              const a = byId.get(p)!;
              const b = byId.get(n.id)!;
              const x1 = a.x + NODE_W, y1 = a.y + NODE_H / 2, x2 = b.x, y2 = b.y + NODE_H / 2;
              const mid = (x1 + x2) / 2;
              return (
                <path
                  key={`${p}-${n.id}`}
                  d={`M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`}
                  fill="none"
                  stroke="#c7ccd4"
                  strokeWidth="1.5"
                />
              );
            }),
          )}
          {nodes.map((n) => {
            const s = nodeStyle(n.state);
            return (
              <g key={n.id}>
                <rect x={n.x} y={n.y} width={NODE_W} height={NODE_H} rx={8} fill={s.fill} stroke={s.stroke} />
                <text x={n.x + 10} y={n.y + 18} fontSize="12" fontWeight="600" fill={s.text}>
                  {n.label.length > 24 ? n.label.slice(0, 23) + "…" : n.label}
                </text>
                <text x={n.x + 10} y={n.y + 33} fontSize="10" fill="#6b7280">
                  {PRETTY_STATE[n.state] ?? n.state}
                  {n.agent ? ` · ${n.agent}` : ""}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
      {/* Text alternative (§15.2) */}
      <details className="mt-2 rounded-[8px] border border-edge px-3 py-2">
        <summary className="cursor-pointer text-xs font-medium text-ink2">Text form of the graph</summary>
        <ul className="mt-2 space-y-1 text-sm">
          {G.nodes.map((n) => (
            <li key={n.id}>
              <span className="font-medium">{n.label}</span>{" "}
              <span className="text-xs text-ink2">
                ({PRETTY_STATE[n.state] ?? n.state}
                {n.agent ? `, ${n.agent}` : ""})
              </span>
              {n.deps.length ? (
                <span className="text-xs text-ink2"> — after {n.deps.map((d) => byId.get(d)?.label ?? d).join(", ")}</span>
              ) : null}
            </li>
          ))}
        </ul>
      </details>
    </div>
  );
}

/* ---------- §13.7 causal links (no agent theater) ---------- */

const KIND_ICON: Record<string, string> = { evidence: "EV", hdr: "HD", retry: "→" };

export function CausalLinksCard() {
  return (
    <section className="card mb-4 overflow-hidden">
      <h2 className="border-b border-edge px-3.5 py-2.5 text-xs font-semibold uppercase tracking-wider text-ink2">
        Causal links — agents only in Attempt context (§13.7, no theater)
      </h2>
      <ul className="divide-y divide-edge/50">
        {FIXTURE.causalLinks.map((l, i) => (
          <li key={i} className="flex items-start gap-3 px-3.5 py-2.5 text-sm leading-5">
            <span
              aria-hidden
              className="mt-0.5 flex size-6 shrink-0 items-center justify-center rounded-full bg-surface text-[10px] font-bold text-ink2"
            >
              {KIND_ICON[l.kind] ?? "•"}
            </span>
            {l.text}
          </li>
        ))}
      </ul>
    </section>
  );
}

/* ---------- §9.1 Execution Explorer (secondary diagnostic route) ---------- */

export function ExecutionExplorer({ onClose }: { onClose: () => void }) {
  return (
    <div
      className="fixed inset-0 z-30 flex items-start justify-center bg-ink/20 p-6"
      role="dialog"
      aria-label="Execution Explorer"
      onClick={onClose}
    >
      <div className="card mt-10 w-full max-w-3xl overflow-hidden" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2 border-b border-edge px-4 py-3">
          <strong className="text-sm">Execution Explorer</strong>
          <span className="min-w-0 flex-1 text-xs text-ink2">
            Secondary diagnostic route across tasks (§9.1) — table for Attempts comparison (§14.4)
          </span>
          <Button size="sm" onClick={onClose}>
            Close
          </Button>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[640px] border-collapse text-sm">
            <thead>
              <tr className="border-b border-edge text-left text-xs text-ink2">
                <th className="px-3.5 py-2 font-semibold">Task</th>
                <th className="px-3.5 py-2 font-semibold">State</th>
                <th className="px-3.5 py-2 font-semibold">Attempt</th>
                <th className="px-3.5 py-2 font-semibold">Provider</th>
                <th className="px-3.5 py-2 font-semibold">Live observation</th>
              </tr>
            </thead>
            <tbody>
              {FIXTURE.explorer.map((r) => (
                <tr key={r.id} className="border-b border-edge/50">
                  <td className="px-3.5 py-2">
                    <span className="font-mono text-xs text-ink2">{r.id}</span> {r.objective}
                  </td>
                  <td className="px-3.5 py-2">
                    <Badge state={r.state as TaskLifecycle} />
                  </td>
                  <td className="px-3.5 py-2">{r.attempt}</td>
                  <td className="px-3.5 py-2">{r.provider}</td>
                  <td className="px-3.5 py-2 text-xs text-ink2">{r.live}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}