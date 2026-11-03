import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

const SHOTS = "e2e/screenshots";
const SIZES = [
  { width: 1440, height: 900 },
  { width: 1280, height: 800 },
];
const SCREENS: Array<{ name: string; heading: RegExp; open: (page: Page) => Promise<void> }> = [
  { name: "overview", heading: /Обзор проекта/, open: async () => undefined },
  { name: "tasks", heading: /^Задачи$/, open: (page) => page.getByRole("link", { name: /Задачи/ }).first().click() },
  { name: "task", heading: /Чистый домен/, open: async (page) => {
    await page.getByRole("link", { name: /Задачи/ }).first().click();
    await page.getByRole("row", { name: /TASK-0217/ }).click();
  } },
  { name: "attention", heading: /Внимание/, open: (page) => page.getByRole("link", { name: /Внимание/ }).click() },
  { name: "map", heading: /Карта проекта/, open: (page) => page.getByRole("link", { name: "Карта" }).click() },
  { name: "settings", heading: /Настройки/, open: (page) => page.getByRole("link", { name: "Настройки" }).click() },
];

async function setTheme(page: Page, theme: "dark" | "light"): Promise<void> {
  await page.addInitScript((value) => {
    localStorage.setItem("runlith.settings.v1", JSON.stringify({ theme: value, density: 40, reduceMotion: true, agentOpen: true }));
  }, theme);
}

for (const theme of ["dark", "light"] as const) {
  for (const size of SIZES) {
    test(`screens render without axe violations · ${theme} · ${size.width}`, async ({ page }) => {
      await page.setViewportSize(size);
      await setTheme(page, theme);
      await page.goto("/");
      for (const screen of SCREENS) {
        await page.goto("/");
        await screen.open(page);
        await expect(page.locator("main h1")).toHaveText(screen.heading);
        await expect(page.getByRole("navigation", { name: "Основная навигация" }).locator("[aria-current=page]")).toHaveCount(1);
        await page.evaluate(() => document.fonts.ready);
        await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running" || a.effect?.getTiming().iterations === Infinity));
        await page.screenshot({ path: `${SHOTS}/${screen.name}-${theme}-${size.width}.png` });
        const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
        expect(results.violations.map((v) => `${screen.name}: ${v.id} ${v.nodes.map((n) => n.target.join(" ")).slice(0, 3).join(", ")}`)).toEqual([]);
      }
    });
  }
}

test("states: daemon unavailable, empty project, loading", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await setTheme(page, "dark");

  await page.goto("/?demo=unavailable");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await expect(page.getByRole("alert")).toContainText("agentd не запущен");
  await expect(page.getByText("agentd не запущен").first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/state-unavailable.png` });

  await page.goto("/?demo=empty");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await expect(page.getByText("В agentd нет проектов")).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/state-empty.png` });

  await page.goto("/?demo=loading");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await expect(page.locator("[aria-busy=true]").first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/state-loading.png` });
});

/** Samples every rendered edge and checks it against every rendered node box, in screen space. */
async function geometry(page: Page, scope: string): Promise<{ crossings: string[]; overlaps: string[]; edges: number; nodes: number }> {
  return page.evaluate((root) => {
    const nodes = [...document.querySelectorAll<HTMLElement>(`${root} [data-node]`)].map((el) => ({ id: el.dataset.node ?? "", r: el.getBoundingClientRect() }));
    const paths = [...document.querySelectorAll<SVGPathElement>(`${root} path[data-from]`)];
    const crossings = new Set<string>();
    for (const path of paths) {
      const matrix = path.getScreenCTM();
      if (!matrix) continue;
      const from = path.dataset.from, to = path.dataset.to;
      const length = path.getTotalLength();
      for (let s = 0; s <= length; s += 2) {
        const point = path.getPointAtLength(s).matrixTransform(matrix);
        for (const node of nodes) {
          if (node.id === from || node.id === to) continue;
          const r = node.r;
          if (point.x > r.left + 1 && point.x < r.right - 1 && point.y > r.top + 1 && point.y < r.bottom - 1) crossings.add(`${from}>${to} enters ${node.id}`);
        }
      }
    }
    const overlaps: string[] = [];
    nodes.forEach((a, i) => nodes.slice(i + 1).forEach((b) => {
      if (a.r.left < b.r.right && b.r.left < a.r.right && a.r.top < b.r.bottom && b.r.top < a.r.bottom) overlaps.push(`${a.id}/${b.id}`);
    }));
    return { crossings: [...crossings], overlaps, edges: paths.length, nodes: nodes.length };
  }, scope);
}

for (const theme of ["dark", "light"] as const) {
  test(`graphs: no edge crosses a node, no nodes overlap · ${theme}`, async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await setTheme(page, theme);
    await page.goto("/");
    // The layout loads first, so wait for the graph itself rather than the page.
    await expect(page.locator("main [data-node]").first()).toBeVisible();
    const g15 = await geometry(page, "main");
    expect(g15.edges).toBe(15);
    expect(g15.nodes).toBe(14);
    expect(g15.crossings).toEqual([]);
    expect(g15.overlaps).toEqual([]);
    await page.getByRole("link", { name: "Карта" }).click();
    await expect(page.locator("main h1")).toHaveText(/Карта проекта/);
    const map = await geometry(page, ".canvas");
    expect(map.nodes).toBe(14);
    expect(map.edges).toBe(18);
    expect(map.crossings).toEqual([]);
    expect(map.overlaps).toEqual([]);
    await page.screenshot({ path: `${SHOTS}/map-canvas-${theme}-1440.png` });
  });
}

test("map answers «что затронет изменение в ipc-contracts?»: open the capability, pick the crate", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await setTheme(page, "dark");
  await page.goto("/");
  await page.getByRole("link", { name: "Карта" }).click();
  // Far zoom shows capability titles only; selecting one zooms into readable scale.
  await expect(page.locator(".canvas-layer.lod-far")).toHaveCount(1);
  await page.getByRole("button", { name: /Локальный IPC/ }).click();
  await expect(page.locator(".canvas-layer.lod-far")).toHaveCount(0);
  await page.locator(".cap .crate", { hasText: /^ipc-contracts$/ }).click();
  const panel = page.getByRole("complementary", { name: "Детали" });
  await expect(panel.getByRole("heading", { name: "Затронет при изменении" })).toBeVisible();
  for (const name of ["agentctl", "agentd", "ipc-control"]) await expect(panel.getByRole("button", { name, exact: true })).toBeVisible();
  await expect(page.locator(".cap.sel")).toHaveCount(1);
  await expect(page.locator(".cap.dim").first()).toBeVisible();
  await page.screenshot({ path: `${SHOTS}/map-selected-ipc-contracts.png` });
  await page.locator(".canvas").press("Escape");
  await expect(page.locator(".cap.dim")).toHaveCount(0);
});

test("reduced motion: no animations run when the OS asks for less motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await expect(page.locator("main h1")).toHaveText(/^Задачи$/);
  // Colour transitions are clamped to 1 ms and are not motion; keyframe motion and transform transitions must be gone.
  const moving = await page.evaluate(() => document.getAnimations().filter((a) => a.playState === "running" && (
    a instanceof CSSAnimation || (a instanceof CSSTransition && a.transitionProperty === "transform"))).length);
  expect(moving).toBe(0);
  const auroraAnimation = await page.locator(".aurora").evaluate((el) => getComputedStyle(el, "::before").animationName);
  expect(auroraAnimation).toBe("none");
});

// Headless WebKit paces rAF at its own rate (about 30 Hz here), so absolute
// budgets are meaningless. The check: a screen change with view transition and
// reveal animations must not add long frames over the idle baseline.
const SAMPLE = `(n) => new Promise((done) => { const f = []; let l = performance.now();
  const t = (now) => { f.push(now - l); l = now; if (f.length < n) requestAnimationFrame(t); else done(f.slice(1)); };
  requestAnimationFrame(t); })`;

test("motion: a screen change adds no long frames over the idle baseline", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("main h1")).toHaveText(/Обзор проекта/);
  // Measure idle only after entrance animations settle; otherwise the baseline itself is noisy.
  await page.evaluate(() => document.fonts.ready);
  await page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running" || a.effect?.getTiming().iterations === Infinity));
  const idle = (await page.evaluate(`(${SAMPLE})(61)`)) as number[];
  const busyPromise = page.evaluate(`(${SAMPLE})(61)`) as Promise<number[]>;
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  const busy = await busyPromise;
  const idleMax = Math.max(...idle);
  // A dropped frame at 60 Hz shows as a gap of two vsyncs (≥ 33.3 ms); below that is timer jitter.
  const limit = Math.max(idleMax + 8, 34);
  const over = busy.filter((ms) => ms > limit);
  console.log(`idle max=${idleMax.toFixed(1)}ms busy max=${Math.max(...busy).toFixed(1)}ms limit=${limit}ms frames over=${over.length}`);
  expect(over.length).toBe(0);
});

test("tasks: counters follow the search, the footer counts rows, J/K walk one Tab stop", async ({ page }) => {
  await setTheme(page, "dark");
  await page.goto("/");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await expect(page.getByRole("button", { name: "Готовы 1" })).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "Показано" })).toHaveText("Показано 12 из 12");

  const rows = page.getByRole("table", { name: "Задачи" }).getByRole("row", { name: /TASK-/ });
  await expect(page.locator("a.tr[tabindex='0']")).toHaveCount(1);
  await rows.first().focus();
  await page.keyboard.press("j");
  await expect(rows.nth(1)).toBeFocused();
  await page.keyboard.press("j");
  await page.keyboard.press("k");
  await expect(rows.nth(1)).toBeFocused();
  await expect(page.locator("a.tr[tabindex='0']")).toHaveCount(1);

  // Single-character keys stay inside the list: in the search box J is just a letter.
  await page.locator("#global-search").focus();
  await page.keyboard.type("0217");
  await expect(page.locator("#global-search")).toHaveValue("0217");
  await expect(page.getByRole("button", { name: "Все 1" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Ждут 0" })).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "Показано" })).toHaveText("Показано 1 из 12");
  await page.locator("#global-search").fill("");

  await rows.nth(3).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("main h1")).toHaveText(/Чистый домен/);
});

async function openTask(page: Page, tab?: string): Promise<void> {
  await page.goto("/");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await page.getByRole("row", { name: /TASK-0217/ }).click();
  if (tab) await page.getByRole("navigation", { name: "Разделы задачи" }).getByRole("link", { name: new RegExp(tab) }).click();
}

test("evidence: criteria show verdicts, not task states, with a total", async ({ page }) => {
  await setTheme(page, "dark");
  await openTask(page, "Evidence");
  const card = page.getByRole("region", { name: "Доказательства по критериям" });
  await expect(card.getByText("1 из 3 проверено")).toBeVisible();
  await expect(card.locator(".state.ok")).toHaveText("Проверено");
  await expect(card.locator(".state.wait")).toHaveText("Ожидает");
  await expect(card.getByText("Завершена")).toHaveCount(0);
});

test("changes: the tab names the file count; a file row attaches the file to the question", async ({ page }) => {
  await setTheme(page, "dark");
  await openTask(page);
  const tabs = page.getByRole("navigation", { name: "Разделы задачи" });
  await expect(tabs.getByRole("link", { name: /Changes/ })).toContainText("10 файлов");
  await tabs.getByRole("link", { name: /Changes/ }).click();

  const row = page.locator(".file-row").first();
  const ask = row.getByRole("button", { name: /execution_cycle_lock/ });
  await expect(ask).toHaveCSS("opacity", "0");
  await row.hover();
  await expect(ask).toHaveCSS("opacity", "1");
  await ask.click();

  const composer = page.getByRole("form", { name: "Сообщение агенту" });
  await expect(composer.locator(".att")).toHaveText("execution_cycle_lock.rs");
  await expect(page.locator("#agent-ask")).toBeFocused();
  await row.hover();
  await ask.click();
  await expect(composer.locator(".att")).toHaveCount(1);

  // Sending still says plainly that there is no chat channel, and keeps the text.
  await page.locator("#agent-ask").fill("Что здесь поменялось?");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("status").filter({ hasText: "нет канала чата" })).toBeVisible();
  await expect(page.locator("#agent-ask")).toHaveValue("Что здесь поменялось?");

  await composer.getByRole("button", { name: /Убрать вложение/ }).click();
  await expect(composer.locator(".att")).toHaveCount(0);
});

test("changes: the question action is reachable from the keyboard", async ({ page }) => {
  await setTheme(page, "dark");
  await openTask(page, "Changes");
  const ask = page.locator(".file-row").first().getByRole("button");
  await ask.focus();
  await page.keyboard.press("Enter");
  await expect(page.locator(".att")).toHaveCount(1);
});

for (const width of [1100, 1280, 1440, 1920]) {
  test(`layout holds at ${width}: nothing spills out of the content tile`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await setTheme(page, "dark");
    await openTask(page, "Changes");
    const spill = await page.locator(".view").evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(spill).toBeLessThanOrEqual(0);
    await page.getByRole("link", { name: /Задачи/ }).first().click();
    const tasksSpill = await page.locator(".view").evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(tasksSpill).toBeLessThanOrEqual(0);
  });
}

test("keyboard: ⌘K focuses search, filter narrows rows, focus moves to heading", async ({ page }) => {
  await setTheme(page, "dark");
  await page.goto("/");
  await page.getByRole("link", { name: /Задачи/ }).first().click();
  await expect(page.locator("main h1")).toBeFocused();
  await page.keyboard.press("Meta+k");
  await expect(page.locator("#global-search")).toBeFocused();
  await page.keyboard.type("0217");
  await expect(page.getByRole("row", { name: /TASK-0/ })).toHaveCount(1);
  await page.getByRole("button", { name: "Закрыты" }).click();
  await expect(page.getByText("Ничего не найдено")).toBeVisible();
});
