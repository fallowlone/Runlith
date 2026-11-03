import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { Icon } from "./Icon";

export type FocusBox = { x: number; y: number; w: number; h: number; seq: number };

type Props = {
  width: number;
  height: number;
  label: string;
  children: ReactNode;
  onBackground: () => void;
  /** Each new `seq` centres this box; zooms in to readable scale if the view is far out. */
  focus?: FocusBox | null;
};

type View = { x: number; y: number; k: number };

const MIN_K = 0.3;
const MAX_K = 1.6;
const STEP = 1.2;
const PAN = 48;
/** Below this scale cards switch to their far level of detail (title and count only). */
const FAR_K = 0.6;

const clamp = (k: number): number => Math.min(MAX_K, Math.max(MIN_K, k));

/**
 * Canvas with pan and zoom. Trackpad scroll pans, pinch or ⌘ + wheel zooms at
 * the cursor, drag on empty canvas pans, keyboard: arrows, + and -, 0 fits,
 * Escape clears the selection. Only transform is animated.
 */
export function PanZoom({ width, height, label, children, onBackground, focus = null }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<View>({ x: 0, y: 0, k: 1 });
  const [isEasing, setEasing] = useState(false);
  const [isPanning, setPanning] = useState(false);
  const drag = useRef<{ px: number; py: number; x: number; y: number; moved: boolean } | null>(null);

  const fit = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const k = clamp(Math.min(el.clientWidth / width, el.clientHeight / height) * 0.96);
    setView({ k, x: (el.clientWidth - width * k) / 2, y: (el.clientHeight - height * k) / 2 });
  }, [width, height]);

  useLayoutEffect(fit, [fit]);

  useEffect(() => {
    const el = ref.current;
    if (!focus || !el) return;
    setEasing(true);
    setView((v) => {
      const k = v.k < 0.9 ? 1 : v.k;
      return { k, x: el.clientWidth / 2 - (focus.x + focus.w / 2) * k, y: el.clientHeight / 2 - (focus.y + focus.h / 2) * k };
    });
  }, [focus]);

  const zoomAt = useCallback((factor: number, cx: number, cy: number) => {
    setView((v) => {
      const k = clamp(v.k * factor);
      const f = k / v.k;
      return { k, x: cx - (cx - v.x) * f, y: cy - (cy - v.y) * f };
    });
  }, []);

  const animated = (action: () => void): void => {
    setEasing(true);
    action();
  };

  const zoomCenter = (factor: number): void => {
    const el = ref.current;
    if (el) animated(() => zoomAt(factor, el.clientWidth / 2, el.clientHeight / 2));
  };

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (event: WheelEvent): void => {
      event.preventDefault();
      setEasing(false);
      if (event.ctrlKey || event.metaKey) {
        const box = el.getBoundingClientRect();
        zoomAt(Math.exp(-event.deltaY * 0.01), event.clientX - box.left, event.clientY - box.top);
      } else {
        setView((v) => ({ ...v, x: v.x - event.deltaX, y: v.y - event.deltaY }));
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomAt]);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    if ((event.target as HTMLElement).closest("button, a, input")) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { px: event.clientX, py: event.clientY, x: view.x, y: view.y, moved: false };
    setEasing(false);
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    const d = drag.current;
    if (!d) return;
    const dx = event.clientX - d.px;
    const dy = event.clientY - d.py;
    if (!d.moved && Math.hypot(dx, dy) < 4) return;
    d.moved = true;
    setPanning(true);
    setView((v) => ({ ...v, x: d.x + dx, y: d.y + dy }));
  };

  const onPointerUp = (): void => {
    if (drag.current && !drag.current.moved) onBackground();
    drag.current = null;
    setPanning(false);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (event.target !== event.currentTarget && event.key !== "Escape") return;
    const moves: Record<string, [number, number]> = { ArrowLeft: [PAN, 0], ArrowRight: [-PAN, 0], ArrowUp: [0, PAN], ArrowDown: [0, -PAN] };
    if (moves[event.key]) {
      event.preventDefault();
      const [dx, dy] = moves[event.key];
      animated(() => setView((v) => ({ ...v, x: v.x + dx, y: v.y + dy })));
    } else if (event.key === "+" || event.key === "=") {
      zoomCenter(STEP);
    } else if (event.key === "-") {
      zoomCenter(1 / STEP);
    } else if (event.key === "0") {
      animated(fit);
    } else if (event.key === "Escape") {
      onBackground();
    }
  };

  return (
    <div
      ref={ref}
      className={`canvas${isPanning ? " panning" : ""}`}
      role="region"
      aria-roledescription="полотно"
      aria-label={label}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onKeyDown={onKeyDown}
    >
      <div
        className={`canvas-layer${isEasing ? " ease" : ""}${view.k < FAR_K ? " lod-far" : ""}`}
        style={{ width, height, transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})` }}
        onTransitionEnd={() => setEasing(false)}
      >
        {children}
      </div>
      <div className="zoombar" role="group" aria-label="Масштаб">
        <button className="btn btn-icon btn-quiet" type="button" aria-label="Уменьшить" onClick={() => zoomCenter(1 / STEP)}><Icon name="minus" small /></button>
        <span className="mono tnum muted" style={{ width: 44, textAlign: "center" }} aria-live="polite">{Math.round(view.k * 100)}%</span>
        <button className="btn btn-icon btn-quiet" type="button" aria-label="Увеличить" onClick={() => zoomCenter(STEP)}><Icon name="plus" small /></button>
        <button className="btn btn-quiet" type="button" onClick={() => animated(fit)}>Вписать</button>
      </div>
    </div>
  );
}
