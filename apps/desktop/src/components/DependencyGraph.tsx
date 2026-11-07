/**
 * G15 dependency graph. Edges and outcomes come from planning/tasks/queue.yaml,
 * layout and orthogonal routes from ELK (scripts/gen-maps.ts): edges run in
 * channels at least 12 px from every node they do not connect (data test and e2e).
 * The drawing scales down to fit its card instead of scrolling.
 */
import { useLayoutEffect, useRef, useState } from "react";
import { MAPS, roundedPath } from "../lib/map/data";

const SUB: Record<string, string> = {
  "TASK-0215": "реестр", "TASK-0216": "ADR", "TASK-0217": "домен", "TASK-0218": "git-cli", "TASK-0219": "сервис",
  "TASK-0220": "IPC", "TASK-0221": "AI", "TASK-0222": "render", "TASK-0223": "Ops I", "TASK-0224": "Ops II",
  "TASK-0225": "Ops III", "TASK-0226": "Ops IV", "TASK-0227": "matrix", "TASK-0228": "closure",
};
const STAGES: Array<[string, string]> = [
  ["TASK-0215", "старт"], ["TASK-0217", "домен"], ["TASK-0220", "IPC"], ["TASK-0223", "ветки"],
  ["TASK-0224", "операции"], ["TASK-0227", "проверка"], ["TASK-0228", "конец"],
];
const EDGE_CLASS = { done: "edge-done", main: "edge-main", side: "edge-side" } as const;
const LABELS = 24;

export function DependencyGraph() {
  const { width, height, nodes, edges, critical } = MAPS.g15;
  const frame = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);

  useLayoutEffect(() => {
    const el = frame.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setScale(Math.min(1, entry.contentRect.width / width)));
    observer.observe(el);
    return () => observer.disconnect();
  }, [width]);

  const done = nodes.filter((n) => n.state === "done").length;
  const ready = nodes.filter((n) => n.state === "ready").map((n) => n.id).join(", ");
  const byId = new Map(nodes.map((n) => [n.id, n]));
  return (
    <div ref={frame} className={scale < 0.9 ? "graph-compact" : undefined} style={{ height: (height + LABELS) * scale }} role="img"
      aria-label={`Граф G15: ${done} задачи завершены, готова ${ready}, критический путь ${critical.length} задач`}>
      <div style={{ position: "relative", width, height: height + LABELS, transform: `scale(${scale})`, transformOrigin: "0 0" }}>
        <svg width={width} height={height} aria-hidden="true" style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}>
          {edges.map((edge) => (
            <path key={`${edge.from}>${edge.to}`} className={EDGE_CLASS[edge.kind]} data-from={edge.from} data-to={edge.to} d={roundedPath(edge.points, 6)} />
          ))}
        </svg>
        {nodes.map((node) => (
          <div key={node.id} className={`node ${node.state}`} data-node={node.id} aria-hidden="true"
            style={{ left: node.x, top: node.y, width: node.w, height: node.h }}>
            <span className="mono">{node.id.slice(5)}</span>
            <span className="caption-5">{SUB[node.id] ?? ""}</span>
          </div>
        ))}
        {STAGES.map(([id, text]) => {
          const node = byId.get(id);
          return node ? (
            <span key={id} className="caption-5 muted" aria-hidden="true"
              style={{ position: "absolute", top: height + 6, left: node.x - 12, width: node.w + 24, textAlign: "center", whiteSpace: "nowrap" }}>
              {text}
            </span>
          ) : null;
        })}
      </div>
    </div>
  );
}
