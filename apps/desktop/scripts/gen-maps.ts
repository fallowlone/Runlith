/**
 * Builds src/lib/map/maps.generated.json from raw sources:
 *   - `cargo metadata` (crates and their normal dependencies),
 *   - planning/tasks/queue.yaml (G15 depends_on and outcomes),
 *   - src/lib/map/capabilities.ts (the agent's capability record).
 * Layout runs here with ELK (layered, orthogonal routing) so the app ships
 * coordinates only and elkjs never enters the runtime bundle.
 *
 * Usage: node scripts/gen-maps.ts
 */
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import ELK from "elkjs/lib/elk.bundled.js";
import { CAPABILITIES } from "../src/lib/map/capabilities.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO = join(HERE, "../../..");
const OUT = join(HERE, "../src/lib/map/maps.generated.json");

type Edge = [string, string];
type Point = { x: number; y: number };

export function workspaceGraph(): { crates: Array<{ name: string; path: string }>; edges: Edge[] } {
  const raw = execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    cwd: REPO, encoding: "utf8", maxBuffer: 64 * 1024 * 1024,
  });
  const meta = JSON.parse(raw) as {
    packages: Array<{ name: string; manifest_path: string; dependencies: Array<{ name: string; kind: string | null }> }>;
  };
  const names = new Set(meta.packages.map((p) => p.name));
  const crates = meta.packages
    .map((p) => ({ name: p.name, path: relative(REPO, dirname(p.manifest_path)) }))
    .sort((a, b) => a.name.localeCompare(b.name));
  const keys = new Set<string>();
  for (const p of meta.packages) {
    for (const d of p.dependencies) {
      if (d.kind === null && names.has(d.name)) keys.add(`${p.name}>${d.name}`);
    }
  }
  const edges = [...keys].sort().map((key) => key.split(">") as Edge);
  return { crates, edges };
}

export function g15Plan(): { tasks: Array<{ id: string; done: boolean }>; edges: Edge[] } {
  const text = readFileSync(join(REPO, "planning/tasks/queue.yaml"), "utf8");
  const tasks: Array<{ id: string; done: boolean }> = [];
  const edges: Edge[] = [];
  for (const block of text.split("\n  - id: ").slice(1)) {
    const id = block.slice(0, block.indexOf("\n")).trim();
    const match = /^TASK-(\d{4})$/.exec(id);
    const n = match ? Number(match[1]) : NaN;
    if (!(n >= 215 && n <= 228)) continue;
    tasks.push({ id, done: /\n {4}outcome:/.test(block) });
    const deps = /\n {4}depends_on: \[([^\]]*)\]/.exec(block)?.[1] ?? "";
    for (const dep of deps.split(",").map((s) => s.trim()).filter(Boolean)) edges.push([id, dep]);
  }
  const ids = new Set(tasks.map((t) => t.id));
  return { tasks, edges: edges.filter(([, dep]) => ids.has(dep)) };
}

function reach(adj: Map<string, string[]>, from: string): Set<string> {
  const seen = new Set<string>();
  const stack = [...(adj.get(from) ?? [])];
  while (stack.length) {
    const next = stack.pop() as string;
    if (seen.has(next)) continue;
    seen.add(next);
    stack.push(...(adj.get(next) ?? []));
  }
  return seen;
}

/** Keeps an edge only when no other path already implies it (reachability is unchanged). */
export function transitiveReduction(edges: Edge[]): Edge[] {
  const adj = new Map<string, string[]>();
  for (const [a, b] of edges) adj.set(a, [...(adj.get(a) ?? []), b]);
  return edges.filter(([a, b]) => !(adj.get(a) ?? []).some((c) => c !== b && reach(adj, c).has(b)));
}

const elk = new ELK();
const round = (p: Point): Point => ({ x: Math.round(p.x), y: Math.round(p.y) });

async function layout(
  nodes: Array<{ id: string; w: number; h: number }>,
  edges: Edge[],
  options: Record<string, string>,
): Promise<{ width: number; height: number; boxes: Map<string, { x: number; y: number; w: number; h: number }>; paths: Point[][] }> {
  const graph = await elk.layout({
    id: "root",
    layoutOptions: options,
    children: nodes.map((n) => ({ id: n.id, width: n.w, height: n.h })),
    edges: edges.map(([a, b], i) => ({ id: `e${i}`, sources: [a], targets: [b] })),
  });
  const boxes = new Map((graph.children ?? []).map((c) => [c.id, { x: Math.round(c.x ?? 0), y: Math.round(c.y ?? 0), w: c.width ?? 0, h: c.height ?? 0 }]));
  const paths = (graph.edges ?? []).map((e) => {
    const s = e.sections?.[0];
    if (!s) throw new Error(`ELK returned no route for ${e.id}`);
    return [s.startPoint, ...(s.bendPoints ?? []), s.endPoint].map(round);
  });
  return { width: Math.ceil(graph.width ?? 0), height: Math.ceil(graph.height ?? 0), boxes, paths };
}

const ORTHOGONAL = {
  "elk.algorithm": "layered",
  "elk.edgeRouting": "ORTHOGONAL",
  "elk.layered.crossingMinimization.strategy": "LAYER_SWEEP",
  "elk.layered.nodePlacement.strategy": "NETWORK_SIMPLEX",
};

export const CAP_WIDTH = 248;
export const capHeight = (crates: number): number => 80 + crates * 24;

async function main(): Promise<void> {
  const { crates, edges: crateEdges } = workspaceGraph();
  const capOf = new Map(CAPABILITIES.flatMap((c) => c.crates.map((name) => [name, c.id] as const)));
  const capKeys = new Set(crateEdges.map(([a, b]) => `${capOf.get(a)}>${capOf.get(b)}`).filter((k) => !k.includes("undefined")));
  const capAll = [...capKeys].map((k) => k.split(">") as Edge).filter(([a, b]) => a !== b).sort();
  const capDrawn = transitiveReduction(capAll);

  const map = await layout(
    CAPABILITIES.map((c) => ({ id: c.id, w: CAP_WIDTH, h: capHeight(c.crates.length) })),
    capDrawn,
    { ...ORTHOGONAL, "elk.direction": "DOWN", "elk.spacing.nodeNode": "48", "elk.layered.spacing.nodeNodeBetweenLayers": "72", "elk.spacing.edgeNode": "24", "elk.spacing.edgeEdge": "16", "elk.layered.spacing.edgeNodeBetweenLayers": "24", "elk.padding": "[top=32,left=32,bottom=32,right=32]" },
  );

  const plan = g15Plan();
  const done = new Set(plan.tasks.filter((t) => t.done).map((t) => t.id));
  const depsOf = (id: string): string[] => plan.edges.filter(([a]) => a === id).map(([, b]) => b);
  const state = (id: string): string => (done.has(id) ? "done" : depsOf(id).every((d) => done.has(d)) ? "ready" : "wait");
  // Critical path: longest dependency chain from the ready task to the sink.
  const memo = new Map<string, string[]>();
  const longestFrom = (id: string): string[] => {
    const cached = memo.get(id);
    if (cached) return cached;
    const next = plan.edges.filter(([, dep]) => dep === id).map(([task]) => longestFrom(task));
    const best = [id, ...(next.sort((a, b) => b.length - a.length)[0] ?? [])];
    memo.set(id, best);
    return best;
  };
  const ready = plan.tasks.find((t) => state(t.id) === "ready")?.id;
  const critical = ready ? longestFrom(ready) : [];
  // Layout edges point from dependency to dependent so time flows left to right.
  const flow: Edge[] = plan.edges.map(([task, dep]) => [dep, task]);
  const g15 = await layout(
    plan.tasks.map((t) => ({ id: t.id, w: 48, h: 48 })),
    flow,
    { ...ORTHOGONAL, "elk.direction": "RIGHT", "elk.layered.layering.strategy": "LONGEST_PATH_SOURCE", "elk.spacing.nodeNode": "16", "elk.layered.spacing.nodeNodeBetweenLayers": "12", "elk.spacing.edgeNode": "12", "elk.spacing.edgeEdge": "10", "elk.layered.spacing.edgeNodeBetweenLayers": "12", "elk.padding": "[top=4,left=4,bottom=4,right=4]" },
  );
  const onCritical = (a: string, b: string): boolean => critical.includes(a) && critical.includes(b) && critical.indexOf(b) === critical.indexOf(a) + 1;

  const data = {
    sources: { cargoMetadata: "cargo metadata --no-deps (normal dependencies)", queue: "planning/tasks/queue.yaml", capabilities: "src/lib/map/capabilities.ts" },
    crates: crates.map((c) => ({ ...c, capability: capOf.get(c.name) ?? null })),
    crateEdges,
    capabilityEdgesAll: capAll,
    map: {
      width: map.width,
      height: map.height,
      nodes: CAPABILITIES.map((c) => ({ ...c, ...map.boxes.get(c.id) })),
      edges: capDrawn.map(([from, to], i) => ({ from, to, points: map.paths[i] })),
    },
    g15: {
      width: g15.width,
      height: g15.height,
      critical,
      nodes: plan.tasks.map((t) => ({ id: t.id, state: state(t.id), ...g15.boxes.get(t.id) })),
      edges: flow.map(([from, to], i) => ({
        from, to, points: g15.paths[i],
        kind: done.has(from) && done.has(to) ? "done" : done.has(from) || onCritical(from, to) ? "main" : "side",
      })),
    },
  };
  writeFileSync(OUT, `${JSON.stringify(data, null, 1)}\n`);
  console.log(`map ${map.width}x${map.height}, ${capDrawn.length}/${capAll.length} capability edges drawn; G15 ${g15.width}x${g15.height}, critical ${critical.length}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().catch((error: unknown) => {
    console.error(error);
    process.exit(1);
  });
}
