import { useCallback, type RefObject } from "react";
import { flushSync } from "react-dom";

const EASE = "cubic-bezier(0.22, 1, 0.36, 1)";
const MOVE_MS = 500;
const ENTER_MS = 480;

export function prefersReducedMotion(): boolean {
  return document.querySelector(".rl")?.getAttribute("data-motion") === "reduce" || matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function cells(grid: HTMLElement): HTMLElement[] {
  return [...grid.querySelectorAll<HTMLElement>(":scope > [data-widget-id]")];
}

/** Runs a layout change, then eases cells from where they were: transform and opacity only. */
export function useFlip(grid: RefObject<HTMLElement | null>): (change: () => void) => void {
  return useCallback((change) => {
    const element = grid.current;
    if (!element || prefersReducedMotion()) {
      change();
      return;
    }
    const before = new Map(cells(element).map((cell) => [cell.dataset.widgetId, cell.getBoundingClientRect()]));
    flushSync(change);
    for (const cell of cells(element)) {
      const from = before.get(cell.dataset.widgetId);
      if (!from) {
        cell.animate([{ opacity: 0, transform: "translateY(12px) scale(0.98)" }, { opacity: 1, transform: "none" }], { duration: ENTER_MS, easing: EASE });
        continue;
      }
      const to = cell.getBoundingClientRect();
      const dx = from.left - to.left;
      const dy = from.top - to.top;
      if (Math.abs(dx) > 0.5 || Math.abs(dy) > 0.5) {
        cell.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: MOVE_MS, easing: EASE });
      }
    }
  }, [grid]);
}
