(function () {
  "use strict";
  // All coordinates are pixels of the 1536x1024 scene art. walk: polygons agents may stand in;
  // block: floor footprints of furniture; pois: where agents stop and what they do there.
  // kind -> pose: work = seated typing, sit = seated typing on a lap/stump, coffee, read.
  const W = 1536, H = 1024, CELL = 12;
  const scenes = {
    studio: { title: "Студия", description: "Большой лофт: рабочие зоны, кухня, библиотека и лаунж.", index: "01", img: "art/v2/studio-wide.png",
      walk: [[[70, 612], [700, 312], [1440, 600], [800, 985]]],
      block: [[130, 430, 310, 600], [270, 380, 420, 510], [225, 520, 530, 740], [415, 715, 625, 865], [660, 860, 830, 960], [690, 650, 1010, 880], [1000, 570, 1300, 790], [575, 345, 760, 470], [560, 230, 880, 340], [915, 330, 1200, 480], [1170, 420, 1450, 610]],
      pois: [
        { kind: "work", at: [245, 565], face: 1 }, { kind: "work", at: [385, 480], face: 1 }, { kind: "work", at: [420, 680], face: 1 }, { kind: "work", at: [510, 640], face: 1 },
        { kind: "work", at: [545, 820], face: 1 }, { kind: "work", at: [1090, 705], face: 1 }, { kind: "work", at: [1170, 715], face: 1 },
        { kind: "sit", at: [790, 740], face: 1 }, { kind: "sit", at: [870, 725], face: 1 }, { kind: "sit", at: [610, 435], face: 1 }, { kind: "sit", at: [735, 445], face: -1 },
        { kind: "coffee", at: [650, 345], face: 1 }, { kind: "coffee", at: [800, 345], face: -1 },
        { kind: "read", at: [1000, 495], face: 1 }, { kind: "read", at: [1130, 485], face: -1 }
      ],
      front: [[130, 400, 420, 600], [180, 520, 520, 740], [340, 700, 625, 865], [660, 860, 830, 960], [690, 650, 1010, 880], [1000, 560, 1300, 790], [575, 340, 760, 470], [560, 150, 880, 340], [915, 200, 1200, 480], [1170, 400, 1450, 610]],
      fx: [{ type: "dust", box: [60, 300, 800, 700], n: 26 }, { type: "steam", at: [650, 300] }] },
    forest: { title: "Лесной лагерь", description: "Костёр, ручей с мостиком, пруд, гамак и стол на пне.", index: "02", img: "art/v2/forest-wide.png",
      walk: [[[300, 610], [520, 450], [560, 400], [900, 420], [1060, 380], [1100, 300], [1200, 420], [1330, 450], [1050, 520], [990, 620], [900, 700], [760, 790], [780, 880], [700, 940], [520, 830], [330, 720]]],
      block: [[660, 360, 740, 400], [840, 600, 1000, 760], [140, 430, 340, 560]], bridge: [[800, 455, 950, 520]],
      pois: [
        { kind: "sit", at: [620, 420], face: 1 }, { kind: "sit", at: [775, 415], face: -1 }, { kind: "sit", at: [695, 440], face: 1 },
        { kind: "work", at: [520, 645], face: 1 }, { kind: "work", at: [440, 505], face: 1 },
        { kind: "coffee", at: [790, 335], face: -1 }, { kind: "coffee", at: [855, 515], face: 1 },
        { kind: "read", at: [1240, 400], face: -1 }, { kind: "read", at: [1000, 640], face: 1 }
      ],
      front: [[140, 430, 340, 560], [840, 600, 1000, 760], [370, 420, 490, 520], [520, 380, 600, 420]],
      fx: [
        { type: "water", poly: [[1030, 560], [1250, 520], [1420, 560], [1430, 640], [1300, 685], [1150, 675], [1040, 620]] },
        { type: "water", poly: [[790, 372], [985, 365], [1010, 395], [935, 445], [860, 480], [790, 520], [850, 545], [790, 570], [700, 600], [605, 625], [560, 690], [575, 765], [655, 795], [745, 745], [705, 645], [770, 580], [850, 525], [900, 470], [800, 410]] },
        { type: "sway", poly: [[0, 0], [1536, 0], [1536, 300], [0, 330]], anchor: 330, amp: 2.4 },
        { type: "fire", at: [690, 385] }, { type: "glow", at: [220, 500], r: 70 }, { type: "glow", at: [915, 690], r: 70 }, { type: "glow", at: [1385, 385], r: 60 },
        { type: "smoke", at: [690, 350] }, { type: "leaves", n: 14 }
      ] },
    orbit: { title: "Орбитальная станция", description: "Мостик, серверная, реактор, лаунж и лаборатория.", index: "03", img: "art/v2/orbit-wide.png",
      walk: [[[300, 500], [560, 380], [660, 340], [980, 430], [1130, 540], [1000, 780], [800, 960], [660, 930], [420, 780]]],
      block: [[700, 340, 960, 400], [440, 380, 600, 470], [390, 600, 705, 840], [885, 600, 1080, 750], [300, 495, 450, 630]],
      pois: [
        { kind: "work", at: [310, 400], face: 1 }, { kind: "work", at: [400, 370], face: 1 }, { kind: "work", at: [500, 440], face: 1 },
        { kind: "sit", at: [235, 620], face: 1 }, { kind: "sit", at: [350, 700], face: 1 },
        { kind: "coffee", at: [400, 590], face: 1 }, { kind: "coffee", at: [920, 700], face: -1 },
        { kind: "read", at: [1150, 690], face: -1 }, { kind: "read", at: [1060, 560], face: 1 }
      ],
      front: [[390, 600, 705, 840], [885, 600, 1080, 750], [300, 495, 450, 630], [440, 380, 600, 470], [700, 200, 960, 400]],
      fx: [{ type: "pulse", at: [815, 250], r: 190 }, { type: "stars", box: [200, 120, 560, 330], n: 22 }, { type: "blink", at: [[430, 330], [525, 295], [285, 410], [800, 380]] }] }
  };
  const people = [
    { name: "Атлас", role: "Планирует", detail: "Планировщик: раскладывает большую задачу на понятные шаги.", likes: ["sit", "coffee", "read"] },
    { name: "Мика", role: "Пишет код", detail: "Разработчик: превращает план в работающий код.", likes: ["work", "work", "coffee"] },
    { name: "Нова", role: "Проверяет", detail: "Ревьюер: ищет ошибки и задаёт неудобные вопросы.", likes: ["work", "read", "read"] },
    { name: "Финн", role: "Исследует", detail: "Исследователь: собирает контекст и сравнивает подходы.", likes: ["read", "sit", "coffee"] },
    { name: "Рэй", role: "Тестирует", detail: "Тестировщик: проверяет, выдержит ли решение реальные сценарии.", likes: ["work", "sit", "coffee"] },
    { name: "Луми", role: "Отдыхает", detail: "Свободный агент: отдыхает и ждёт следующую задачу.", likes: ["coffee", "sit", "read"] }
  ];
  const SLIDE = .45, SPEED = 90; // px per second
  const inPoly = (poly, x, y) => { let inside = false; for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) { const [xi, yi] = poly[i], [xj, yj] = poly[j]; if ((yi > y) !== (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi) inside = !inside; } return inside; };
  const inRect = (rects, x, y) => rects.some(([a, b, e, f]) => x >= a && x <= e && y >= b && y <= f);
  const wet = (scene, x, y) => scene.fx.some(f => f.type === "water" && [[0, 0], [16, 0], [-16, 0], [0, 12], [0, -12]].some(([dx, dy]) => inPoly(f.poly, x + dx, y + dy))) && !inRect(scene.bridge || [], x, y);
  const navs = new WeakMap();
  function nav(scene) {
    if (navs.has(scene)) return navs.get(scene);
    const cols = Math.ceil(W / CELL), rows = Math.ceil(H / CELL), free = new Uint8Array(cols * rows);
    for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) {
      const x = c * CELL + CELL / 2, y = r * CELL + CELL / 2;
      free[r * cols + c] = scene.walk.some(p => inPoly(p, x, y)) && !scene.block.some(([a, b, e, f]) => x >= a && x <= e && y >= b && y <= f) && !wet(scene, x, y) ? 1 : 0;
    }
    const made = { cols, rows, free }; navs.set(scene, made); return made;
  }
  function nearestFree(n, x, y) {
    const c0 = Math.floor(x / CELL), r0 = Math.floor(y / CELL);
    for (let d = 0; d < 60; d++) for (let r = r0 - d; r <= r0 + d; r++) for (let c = c0 - d; c <= c0 + d; c++)
      if (c >= 0 && r >= 0 && c < n.cols && r < n.rows && n.free[r * n.cols + c]) return [c, r];
    return null;
  }
  function findPath(scene, from, to) {
    const n = nav(scene), s = nearestFree(n, ...from), g = nearestFree(n, ...to);
    if (!s || !g) return [];
    const key = ([c, r]) => r * n.cols + c, open = [[0, s]], cost = new Map([[key(s), 0]]), prev = new Map();
    const h = ([c, r]) => Math.hypot(c - g[0], r - g[1]);
    while (open.length) {
      open.sort((a, b) => a[0] - b[0]); const [, cur] = open.shift();
      if (cur[0] === g[0] && cur[1] === g[1]) break;
      for (let dr = -1; dr <= 1; dr++) for (let dc = -1; dc <= 1; dc++) {
        if (!dc && !dr) continue;
        const nx = [cur[0] + dc, cur[1] + dr];
        if (nx[0] < 0 || nx[1] < 0 || nx[0] >= n.cols || nx[1] >= n.rows || !n.free[key(nx)]) continue;
        if (dc && dr && !(n.free[key([cur[0] + dc, cur[1]])] && n.free[key([cur[0], cur[1] + dr])])) continue;
        const next = cost.get(key(cur)) + Math.hypot(dc, dr);
        if (next < (cost.get(key(nx)) ?? Infinity)) { cost.set(key(nx), next); prev.set(key(nx), cur); open.push([next + h(nx), nx]); }
      }
    }
    if (!prev.has(key(g)) && key(s) !== key(g)) return [];
    const path = []; for (let cur = g; cur && key(cur) !== key(s); cur = prev.get(key(cur))) path.push([cur[0] * CELL + CELL / 2, cur[1] * CELL + CELL / 2]);
    return path.reverse();
  }
  function createAgents(scene = scenes.studio) {
    const n = nav(scene);
    return people.map((person, id) => {
      const [c, r] = nearestFree(n, 560 + id * 90, 520 + (id % 2) * 80);
      return { ...person, id, x: c * CELL, y: r * CELL, mode: "idle", wait: id * .5, path: [], poi: null, facing: 1, distance: 0, walking: false, pose: "stand", clock: 0 };
    });
  }
  function choose(scene, agents, agent, random) {
    const taken = new Set(agents.map(a => a.poi).filter(Boolean));
    const options = scene.pois.filter(p => !taken.has(p) && agent.likes.includes(p.kind));
    return options.length ? options[Math.floor(random() * options.length)] : null;
  }
  function advance(agents, seconds, scene = scenes.studio, random = Math.random) {
    if (!Number.isFinite(seconds) || seconds < 0) throw new RangeError("Animation time must be finite and positive");
    for (const agent of agents) {
      agent.clock += seconds; agent.walking = false;
      if (agent.mode === "idle") {
        agent.pose = "stand";
        if ((agent.wait -= seconds) > 0) continue;
        const poi = choose(scene, agents, agent, random);
        if (!poi) { agent.wait = 1; continue; }
        const goal = nearestFree(nav(scene), ...poi.at);
        agent.poi = poi; agent.path = findPath(scene, [agent.x, agent.y], poi.at); agent.mode = "walk";
        agent.door = goal ? [goal[0] * CELL + CELL / 2, goal[1] * CELL + CELL / 2] : [agent.x, agent.y];
      }
      if (agent.mode === "walk") {
        let left = seconds * SPEED;
        while (left > 0 && agent.path.length) {
          const [tx, ty] = agent.path[0], dx = tx - agent.x, dy = ty - agent.y, d = Math.hypot(dx, dy), step = Math.min(left, d);
          if (d > 0) { agent.x += dx / d * step; agent.y += dy / d * step; if (Math.abs(dx) > .1) agent.facing = dx > 0 ? 1 : -1; agent.distance += step; agent.walking = true; }
          left -= step; if (step === d) agent.path.shift();
        }
        if (!agent.path.length) { agent.mode = "enter"; agent.slide = 0; agent.from = [agent.x, agent.y]; }
      }
      // sitting down / standing up is a short slide between the free floor and the seat, never a walk over furniture
      if (agent.mode === "enter" || agent.mode === "exit") {
        agent.slide = Math.min(1, agent.slide + seconds / SLIDE);
        const [a, b] = agent.mode === "enter" ? [agent.from, agent.poi.at] : [agent.poi.at, agent.door];
        agent.x = a[0] + (b[0] - a[0]) * agent.slide; agent.y = a[1] + (b[1] - a[1]) * agent.slide;
        if (agent.slide === 1 && agent.mode === "enter") { agent.mode = "act"; agent.pose = agent.poi.kind; agent.facing = agent.poi.face; agent.wait = 7 + random() * 8; }
        else if (agent.slide === 1) { agent.mode = "idle"; agent.poi = null; agent.pose = "stand"; agent.wait = 1 + random() * 2; }
      } else if (agent.mode === "act" && (agent.wait -= seconds) <= 0) { agent.mode = "exit"; agent.slide = 0; agent.pose = "stand"; }
    }
  }
  globalThis.PixelWorld = { scenes, people, createAgents, advance, findPath, nav, CELL, size: [W, H] };
}());
