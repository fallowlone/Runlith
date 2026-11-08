"use strict";
const assert = require("node:assert/strict");
const { test } = require("node:test");
require("./world.js");
const { scenes, createAgents, advance, findPath, nav, CELL } = globalThis.PixelWorld;
const free = (scene, [x, y]) => nav(scene).free[Math.floor(y / CELL) * nav(scene).cols + Math.floor(x / CELL)] === 1;
function rng(seed) { return () => (seed = (seed * 16807) % 2147483647) / 2147483647; }

for (const [name, scene] of Object.entries(scenes)) {
  test(`${name}: routes use only free floor, visit points and act`, () => {
    const agents = createAgents(scene), random = rng(7), acted = new Set(), moved = new Set();
    for (let frame = 0; frame < 60 * 240; frame++) {
      advance(agents, 1 / 60, scene, random);
      for (const agent of agents) {
        assert.ok(Number.isFinite(agent.x) && Number.isFinite(agent.y));
        if (agent.mode === "walk") { agent.path.forEach(p => assert.ok(free(scene, p), `${agent.name} routed through blocked ${p}`)); assert.ok(free(scene, [agent.x, agent.y]), `${agent.name} walked onto blocked floor at ${agent.x},${agent.y}`); }
        if (agent.mode === "act") acted.add(agent.id);
        if (agent.walking) moved.add(agent.id);
      }
    }
    assert.ok(moved.size >= 5 && acted.size >= 5, `moved ${moved.size}, acted ${acted.size}`);
  });
}

test("no two agents occupy the same point of interest", () => {
  const scene = scenes.studio, agents = createAgents(scene), random = rng(3);
  for (let frame = 0; frame < 60 * 120; frame++) {
    advance(agents, 1 / 60, scene, random);
    const used = agents.map(a => a.poi).filter(Boolean);
    assert.equal(new Set(used).size, used.length);
  }
});

test("path ends on free floor next to the requested point", () => {
  const path = findPath(scenes.studio, [560, 520], [790, 740]);
  assert.ok(path.length > 0 && free(scenes.studio, path.at(-1)));
});

test("rejects invalid elapsed time", () => {
  assert.throws(() => advance(createAgents(), -1), RangeError);
  assert.throws(() => advance(createAgents(), Number.NaN), RangeError);
});
