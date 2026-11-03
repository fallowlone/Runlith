/**
 * Ground-truth checks for src/lib/map/maps.generated.json.
 * Run: node --test scripts/   (needs cargo on PATH)
 */
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { CAPABILITIES } from "../src/lib/map/capabilities.ts";
import { REPO, g15Plan, transitiveReduction, workspaceGraph } from "./gen-maps.ts";

type Point = { x: number; y: number };
type Box = { id: string; x: number; y: number; w: number; h: number };

const data = JSON.parse(readFileSync(new URL("../src/lib/map/maps.generated.json", import.meta.url), "utf8"));
const key = ([a, b]: string[]): string => `${a}>${b}`;

test("crate edges equal cargo metadata: none missing, none extra", () => {
  const truth = workspaceGraph();
  assert.deepEqual(data.crateEdges.map(key).sort(), truth.edges.map(key).sort());
  assert.deepEqual(data.crates.map((c: { name: string }) => c.name).sort(), truth.crates.map((c) => c.name).sort());
});

test("every crate belongs to exactly one capability", () => {
  const owners = new Map<string, string[]>();
  for (const cap of CAPABILITIES) for (const crate of cap.crates) owners.set(crate, [...(owners.get(crate) ?? []), cap.id]);
  for (const crate of workspaceGraph().crates) assert.deepEqual(owners.get(crate.name)?.length, 1, `${crate.name}: ${owners.get(crate.name)}`);
  assert.equal([...owners.keys()].length, data.crates.length, "capabilities name crates that do not exist");
});

test("every agent-inferred capability has sources that exist on disk", () => {
  for (const node of data.map.nodes) {
    assert.ok(node.crates.length > 0, `${node.id} has no sources`);
    for (const crate of node.crates) {
      const path = data.crates.find((c: { name: string }) => c.name === crate)?.path;
      assert.ok(path && existsSync(join(REPO, path, "Cargo.toml")), `${node.id}: ${crate} has no manifest at ${path}`);
    }
  }
});

test("drawn capability edges keep exactly the reachability of the full graph", () => {
  const all: Array<[string, string]> = data.capabilityEdgesAll;
  const drawn = data.map.edges.map((e: { from: string; to: string }) => [e.from, e.to]);
  assert.deepEqual(drawn.map(key).sort(), transitiveReduction(all).map(key).sort());
  const reach = (edges: string[][], from: string): string[] => {
    const seen = new Set<string>();
    const stack = edges.filter(([a]) => a === from).map(([, b]) => b);
    while (stack.length) {
      const id = stack.pop() as string;
      if (!seen.has(id)) { seen.add(id); stack.push(...edges.filter(([a]) => a === id).map(([, b]) => b)); }
    }
    return [...seen].sort();
  };
  for (const cap of CAPABILITIES) assert.deepEqual(reach(drawn, cap.id), reach(all, cap.id), cap.id);
});

test("G15 edges equal queue.yaml depends_on", () => {
  const plan = g15Plan();
  assert.equal(plan.tasks.length, 14);
  assert.deepEqual(data.g15.edges.map((e: { from: string; to: string }) => `${e.to}>${e.from}`).sort(), plan.edges.map(key).sort());
});

function crossings(boxes: Box[], edges: Array<{ from: string; to: string; points: Point[] }>): string[] {
  const hits: string[] = [];
  for (const edge of edges) {
    for (let i = 0; i < edge.points.length - 1; i += 1) {
      const [a, b] = [edge.points[i], edge.points[i + 1]];
      const steps = Math.ceil(Math.hypot(b.x - a.x, b.y - a.y) / 2);
      for (let s = 0; s <= steps; s += 1) {
        const p = { x: a.x + ((b.x - a.x) * s) / steps, y: a.y + ((b.y - a.y) * s) / steps };
        for (const box of boxes) {
          if (box.id === edge.from || box.id === edge.to) continue;
          if (p.x > box.x + 1 && p.x < box.x + box.w - 1 && p.y > box.y + 1 && p.y < box.y + box.h - 1) hits.push(`${edge.from}>${edge.to} enters ${box.id}`);
        }
      }
    }
  }
  return [...new Set(hits)];
}

function overlaps(boxes: Box[]): string[] {
  const out: string[] = [];
  boxes.forEach((a, i) => boxes.slice(i + 1).forEach((b) => {
    if (a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h) out.push(`${a.id} overlaps ${b.id}`);
  }));
  return out;
}

/** Smallest distance from any edge to any node it does not connect: dashed lines must not hug dashed borders. */
function clearance(boxes: Box[], edges: Array<{ from: string; to: string; points: Point[] }>): number {
  let worst = Infinity;
  for (const edge of edges) {
    for (let i = 0; i < edge.points.length - 1; i += 1) {
      const [a, b] = [edge.points[i], edge.points[i + 1]];
      const steps = Math.max(1, Math.ceil(Math.hypot(b.x - a.x, b.y - a.y)));
      for (let s = 0; s <= steps; s += 1) {
        const p = { x: a.x + ((b.x - a.x) * s) / steps, y: a.y + ((b.y - a.y) * s) / steps };
        for (const box of boxes) {
          if (box.id === edge.from || box.id === edge.to) continue;
          const dx = Math.max(box.x - p.x, 0, p.x - (box.x + box.w));
          const dy = Math.max(box.y - p.y, 0, p.y - (box.y + box.h));
          worst = Math.min(worst, Math.hypot(dx, dy));
        }
      }
    }
  }
  return worst;
}

test("edges keep at least 12 px from every node they do not connect", () => {
  assert.ok(clearance(data.g15.nodes, data.g15.edges) >= 12, `G15 clearance ${clearance(data.g15.nodes, data.g15.edges)}`);
  assert.ok(clearance(data.map.nodes, data.map.edges) >= 12, `map clearance ${clearance(data.map.nodes, data.map.edges)}`);
});

test("no edge crosses a node and no nodes overlap (map and G15 layouts)", () => {
  assert.deepEqual(crossings(data.map.nodes, data.map.edges), []);
  assert.deepEqual(crossings(data.g15.nodes, data.g15.edges), []);
  assert.deepEqual(overlaps(data.map.nodes), []);
  assert.deepEqual(overlaps(data.g15.nodes), []);
});
