(function () {
  "use strict";
  const { scenes, createAgents, advance, createRenderer } = PixelWorld;
  let agents = createAgents(scenes.studio); const canvas = document.querySelector("#scene");
  const renderer = createRenderer(canvas); renderer.onload(() => draw()); const stage = document.querySelector(".stage");
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const state = { scene: "studio", paused: reducedMotion.matches, speed: 1, labels: true, selected: -1, time: 0, debug: location.search.includes("debug") };
  const pause = document.querySelector("#pause");
  function draw() { renderer.render(scenes[state.scene], agents, state); }
  function paintAvatars() { avatars.forEach(([canvas, ctxRenderer, agent]) => ctxRenderer.portrait(agent, .5)); }
  const avatars = [];
  function updatePause() {
    pause.setAttribute("aria-pressed", String(state.paused));
    pause.innerHTML = state.paused ? '<span aria-hidden="true">▷</span> Продолжить' : '<span aria-hidden="true">Ⅱ</span> Пауза';
  }
  document.querySelectorAll(".sketch").forEach(button => {
    const sceneId = button.dataset.scene;
    const thumb = createRenderer(button.querySelector("canvas")), paintThumb = () => thumb.render(scenes[sceneId], [], { animate: false });
    paintThumb(); thumb.onload(paintThumb);
    button.addEventListener("click", () => {
      state.scene = sceneId; agents = createAgents(scenes[sceneId]); state.selected = -1;
      document.querySelectorAll(".agent").forEach(peer => peer.setAttribute("aria-pressed", "false"));
      document.querySelectorAll(".sketch").forEach(peer => peer.setAttribute("aria-pressed", String(peer === button)));
      document.querySelector("#scene-title").textContent = scenes[sceneId].title;
      document.querySelector("#scene-description").textContent = scenes[sceneId].description;
      document.querySelector("#scene-index").textContent = scenes[sceneId].index + " / 03";
      canvas.setAttribute("aria-label", scenes[sceneId].title + ". Шесть демонстрационных ИИ. Роли персонажей перечислены ниже.");
      draw();
    });
  });
  document.querySelectorAll(".swatch").forEach(button => button.addEventListener("click", () => {
    stage.dataset.backdrop = button.dataset.backdrop;
    document.querySelectorAll(".swatch").forEach(peer => peer.setAttribute("aria-pressed", String(peer === button)));
  }));
  agents.forEach(agent => {
    const button = document.createElement("button");
    button.className = "agent"; button.setAttribute("aria-pressed", "false");
    const avatar = document.createElement("canvas"); avatar.width = 56; avatar.height = 64; avatar.setAttribute("aria-hidden", "true");
    avatars.push([avatar, createRenderer(avatar), agent]);
    const name = document.createElement("strong"), role = document.createElement("span");
    name.textContent = agent.name; role.textContent = agent.role; button.append(avatar, name, role);
    button.addEventListener("click", () => {
      state.selected = state.selected === agent.id ? -1 : agent.id;
      document.querySelectorAll(".agent").forEach((peer, id) => peer.setAttribute("aria-pressed", String(state.selected === id)));
      document.querySelector("#agent-detail").textContent = state.selected < 0 ? "Выбери персонажа, чтобы познакомиться с его ролью." : agent.name + ". " + agent.detail + " Демонстрационная роль.";
      draw();
    });
    document.querySelector("#team").append(button);
  });
  pause.addEventListener("click", () => { state.paused = !state.paused; updatePause(); schedule(); });
  document.querySelector("#speed").addEventListener("click", event => {
    state.speed = state.speed === 1 ? 2 : 1;
    event.currentTarget.textContent = state.speed + "×";
    event.currentTarget.setAttribute("aria-label", "Скорость анимации: " + state.speed + "×");
  });
  document.querySelector("#labels").addEventListener("click", event => {
    state.labels = !state.labels; event.currentTarget.setAttribute("aria-pressed", String(state.labels)); draw();
  });
  document.querySelector("#theme").addEventListener("click", event => {
    const dark = document.documentElement.dataset.theme !== "dark";
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    event.currentTarget.setAttribute("aria-pressed", String(dark));
    event.currentTarget.textContent = dark ? "Светлая тема" : "Тёмная тема";
  });
  let previous = 0, frame = 0;
  function tick(now) {
    frame = 0;
    if (state.paused || document.hidden) { previous = 0; return; }
    const delta = previous ? Math.min((now - previous) / 1000, .05) * state.speed : 0;
    previous = now; state.time += delta; advance(agents, delta, scenes[state.scene]); draw(); schedule();
  }
  function schedule() {
    if (state.paused || document.hidden) { cancelAnimationFrame(frame); frame = 0; previous = 0; return; }
    if (!frame) frame = requestAnimationFrame(tick);
  }
  reducedMotion.addEventListener("change", event => { if (event.matches) { state.paused = true; updatePause(); schedule(); } });
  document.addEventListener("visibilitychange", schedule);
  renderer.onload(paintAvatars); updatePause(); paintAvatars(); draw(); schedule();
}());
