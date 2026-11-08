(function () {
  "use strict";
  const CELL_SIZE = 128, SPRITE_SCALE = 1.05, sheet = new Image();
  sheet.src = "art/v2/agents.png";
  const listeners = new Set();
  sheet.onload = () => listeners.forEach(fn => fn());
  const seeded = n => { const s = Math.sin(n * 127.1 + 311.7) * 43758.5453; return s - Math.floor(s); };
  const images = {};
  function sceneImage(scene) {
    if (!images[scene.img]) { const img = new Image(); img.src = scene.img; img.onload = () => listeners.forEach(fn => fn()); images[scene.img] = img; }
    return images[scene.img];
  }
  function frameOf(agent) {
    if (agent.walking) return (agent.facing > 0 ? 0 : 4) + Math.floor(agent.distance / 16) % 4;
    if (agent.pose === "work" || agent.pose === "sit") return 8 + Math.floor(agent.clock * 2) % 2;
    if (agent.pose === "coffee") return 10;
    if (agent.pose === "read") return 11;
    return agent.facing > 0 ? 1 : 5;
  }
  function createRenderer(canvas) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("Canvas 2D is unavailable");
    const [W, H] = PixelWorld.size;
    const ready = img => img.complete && img.naturalWidth > 0;
    const clipPoly = poly => { ctx.beginPath(); poly.forEach(([x, y], i) => i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)); ctx.closePath(); ctx.clip(); };
    function box(x, y, size, color, alpha) { ctx.globalAlpha = alpha; ctx.fillStyle = color; ctx.fillRect(Math.round(x), Math.round(y), size, size); ctx.globalAlpha = 1; }
    function glow(x, y, r, color, alpha) {
      const g = ctx.createRadialGradient(x, y, 0, x, y, r); g.addColorStop(0, color); g.addColorStop(1, "rgba(0,0,0,0)");
      ctx.globalCompositeOperation = "lighter"; ctx.globalAlpha = alpha; ctx.fillStyle = g; ctx.fillRect(x - r, y - r, r * 2, r * 2);
      ctx.globalCompositeOperation = "source-over"; ctx.globalAlpha = 1;
    }
    const effects = {
      water(fx, img, t) {
        ctx.save(); clipPoly(fx.poly);
        const ys = fx.poly.map(p => p[1]), top = Math.floor(Math.min(...ys)), bottom = Math.ceil(Math.max(...ys));
        for (let y = top; y < bottom; y += 3) ctx.drawImage(img, 0, y, W, 3, Math.round(Math.sin(t * 2.4 + y * .21) * 2.2), y, W, 3);
        const xs = fx.poly.map(p => p[0]), left = Math.min(...xs), width = Math.max(...xs) - left;
        for (let i = 0; i < 16; i++) { const x = left + seeded(i + top) * width, y = top + seeded(i * 3 + left) * (bottom - top), a = Math.sin(t * 2 + i * 1.7); if (a > .5) box(x, y, 3, "#e8fbff", .75); }
        ctx.restore();
      },
      sway(fx, img, t) {
        ctx.save(); clipPoly(fx.poly);
        for (let y = 0; y < fx.anchor; y += 6) ctx.drawImage(img, 0, y, W, 6, Math.round(Math.sin(t * 1.3 + y * .02) * fx.amp * (fx.anchor - y) / fx.anchor), y, W, 6);
        ctx.restore();
      },
      fire(fx, img, t) { const f = .75 + .25 * Math.sin(t * 11) * Math.sin(t * 7.3); glow(fx.at[0], fx.at[1] - 10, 150, "rgba(255,150,50,.9)", .42 * f); for (let i = 0; i < 6; i++) box(fx.at[0] - 18 + seeded(i) * 36 + Math.sin(t * 9 + i) * 3, fx.at[1] - 12 - ((t * 40 + i * 13) % 40), 4, i % 2 ? "#ffd27a" : "#ff8a3a", .8 - ((t * 40 + i * 13) % 40) / 50); },
      glow(fx, img, t) { glow(fx.at[0], fx.at[1], fx.r, "rgba(255,200,110,.9)", .3 + .08 * Math.sin(t * 3 + fx.at[0])); },
      smoke(fx, img, t) { for (let i = 0; i < 7; i++) { const p = (t * .35 + i / 7) % 1; box(fx.at[0] + Math.sin(p * 6 + i) * 14, fx.at[1] - p * 130, 6 + p * 8, "#d8d3c8", .35 * (1 - p)); } },
      steam(fx, img, t) { for (let i = 0; i < 5; i++) { const p = (t * .5 + i / 5) % 1; box(fx.at[0] + Math.sin(p * 7 + i) * 6, fx.at[1] - p * 50, 4, "#ffffff", .5 * (1 - p)); } },
      leaves(fx, img, t) { for (let i = 0; i < fx.n; i++) { const p = (t * .06 + seeded(i)) % 1; box(p * W, 120 + seeded(i + 9) * 600 + Math.sin(t + i) * 24 + p * 120, 4, i % 3 ? "#7ea03f" : "#d89a3c", .8); } },
      dust(fx, img, t) { const [x, y, w, h] = [fx.box[0], fx.box[1], fx.box[2], fx.box[3]]; for (let i = 0; i < fx.n; i++) box(x + (seeded(i) * w + t * 6 + Math.sin(t * .8 + i) * 10) % w, y + (seeded(i + 5) * h - t * 4 + h * 4) % h, 3, "#fff3c4", .4 + .3 * Math.sin(t * 1.5 + i)); },
      pulse(fx, img, t) { glow(fx.at[0], fx.at[1], fx.r, "rgba(70,230,255,.9)", .22 + .12 * Math.sin(t * 2.2)); glow(fx.at[0], fx.at[1] + 60, fx.r * .7, "rgba(255,90,220,.9)", .1 + .06 * Math.sin(t * 3)); },
      stars(fx, img, t) { for (let i = 0; i < fx.n; i++) { const a = Math.sin(t * 2 + i * 2.3); if (a > .2) box(fx.box[0] + seeded(i) * fx.box[2], fx.box[1] + seeded(i + 4) * fx.box[3] * .6 + 20, 3, "#ffffff", a); } },
      blink(fx, img, t) { fx.at.forEach(([x, y], i) => { if (Math.sin(t * 4 + i * 1.9) > 0) box(x, y, 4, i % 2 ? "#66f0ff" : "#ff7ae0", .9); }); }
    };
    function person(agent, x, y, options = {}) {
      if (!ready(sheet)) return;
      const frame = options.portrait ? 1 : frameOf(agent), s = options.portrait ? options.portrait : SPRITE_SCALE;
      const w = CELL_SIZE * s, h = CELL_SIZE * s, dx = x - w / 2, dy = y - h;
      ctx.imageSmoothingEnabled = false;
      if (!options.portrait) { ctx.fillStyle = "rgba(30,20,40,.28)"; ctx.beginPath(); ctx.ellipse(x, y - 2, 22, 7, 0, 0, 7); ctx.fill(); }
      if (options.selected) { ctx.strokeStyle = "#f8cf71"; ctx.lineWidth = 4; ctx.beginPath(); ctx.ellipse(x, y - 2, 34, 12, 0, 0, 7); ctx.stroke(); }
      const seated = !options.portrait && agent.pose === "work", cut = seated ? .2 : 0;
      ctx.drawImage(sheet, (frame % 12) * CELL_SIZE, agent.id * CELL_SIZE, CELL_SIZE, CELL_SIZE * (1 - cut), dx, dy + (seated ? 12 : 0), w, h * (1 - cut));
      if (options.labels) {
        ctx.font = "600 17px system-ui, sans-serif"; ctx.textAlign = "center";
        const tw = Math.ceil(ctx.measureText(agent.name).width) + 14, ly = y - h * .86;
        ctx.fillStyle = "rgba(252,251,243,.92)"; ctx.fillRect(Math.round(x - tw / 2), Math.round(ly - 18), tw, 24); ctx.fillStyle = "#354137"; ctx.fillText(agent.name, x, ly);
      }
    }
    function render(scene, agents = [], options = {}) {
      ctx.clearRect(0, 0, canvas.width, canvas.height); ctx.save(); ctx.scale(canvas.width / W, canvas.height / H);
      const img = sceneImage(scene), t = options.time || 0;
      if (ready(img)) {
        ctx.drawImage(img, 0, 0, W, H);
        if (options.animate !== false) scene.fx.forEach(fx => effects[fx.type](fx, img, t));
      }
      [...agents].sort((a, b) => a.y - b.y).forEach(agent => {
        person(agent, agent.x, agent.y, { labels: options.labels, selected: agent.id === options.selected });
        // furniture whose base is below the agent hides it again, except while it is using a point of interest
        if (agent.mode !== "act" && ready(img)) scene.front.forEach(([x1, y1, x2, y2]) => { if (y2 > agent.y && agent.x > x1 - 30 && agent.x < x2 + 30) ctx.drawImage(img, x1, y1, x2 - x1, y2 - y1, x1, y1, x2 - x1, y2 - y1); });
      });
      if (options.debug) {
        const n = PixelWorld.nav(scene), c = PixelWorld.CELL; ctx.fillStyle = "rgba(0,255,0,.25)";
        for (let r = 0; r < n.rows; r++) for (let q = 0; q < n.cols; q++) if (n.free[r * n.cols + q]) ctx.fillRect(q * c, r * c, c, c);
        scene.pois.forEach(p => { ctx.fillStyle = "#f0f"; ctx.fillRect(p.at[0] - 5, p.at[1] - 5, 10, 10); });
      }
      ctx.restore();
    }
    return { render, portrait: (agent, size) => { ctx.clearRect(0, 0, canvas.width, canvas.height); person(agent, canvas.width / 2, canvas.height, { portrait: size }); }, onload: fn => listeners.add(fn) };
  }
  PixelWorld.createRenderer = createRenderer;
}());
