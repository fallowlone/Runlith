import { useEffect, useRef, useState } from "react";
import type { MetricDto } from "../../lib/bindings/MetricDto";
import type { MetricSource } from "../../lib/bindings/MetricSource";
import { api, toUiError, type Source } from "../../lib/api";
import { prefersReducedMotion } from "../../lib/useFlip";
import { everyText, nextReadDelay, sourceMeta } from "../../lib/widgetText";
import { Icon } from "../Icon";
import { Odometer } from "./Odometer";

const CLOCK_TICK_MS = 30_000;
const DELTA_MS = 2400;

const clock = (ms: number): string => new Intl.DateTimeFormat("ru-RU", { hour: "2-digit", minute: "2-digit" }).format(new Date(ms));

function agoText(fetchedAtMs: number, now: number): string {
  const minutes = Math.floor((now - fetchedAtMs) / 60_000);
  if (minutes < 1) return "только что";
  if (minutes < 60) return `${minutes} мин назад`;
  return `в ${clock(fetchedAtMs)}`;
}

/** The host decides whether GitHub is really asked; this only asks the host. */
function useMetric(source: MetricSource, everySeconds: number): { metric: MetricDto | null; from: Source; error: string | null } {
  const [metric, setMetric] = useState<MetricDto | null>(null);
  const [from, setFrom] = useState<Source>("live");
  const [error, setError] = useState<string | null>(null);
  const { type, repo } = source;

  useEffect(() => {
    let isCurrent = true;
    let timer = 0;
    const read = (): void => {
      window.clearTimeout(timer);
      if (document.hidden) return;
      api.metricRead({ type, repo }, everySeconds).then(
        ({ data, source: origin }) => {
          if (!isCurrent) return;
          setMetric(data);
          setFrom(origin);
          setError(null);
          timer = window.setTimeout(read, nextReadDelay(data.retryAtMs, everySeconds, Date.now()));
        },
        (failure: unknown) => {
          if (!isCurrent) return;
          setError(toUiError(failure).message);
          timer = window.setTimeout(read, nextReadDelay(null, everySeconds, Date.now()));
        },
      );
    };
    read();
    document.addEventListener("visibilitychange", read);
    return () => {
      isCurrent = false;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", read);
    };
  }, [type, repo, everySeconds]);

  return { metric, from, error };
}

function statusLine(metric: MetricDto | null, error: string | null, everySeconds: number, now: number): { text: string; isWarning: boolean } {
  if (error) return { text: `Не удалось обновить: ${error}`, isWarning: true };
  if (!metric) return { text: "Запрашиваем значение", isWarning: false };
  switch (metric.state) {
    case "fresh":
      return { text: `Обновлено ${metric.fetchedAtMs ? agoText(metric.fetchedAtMs, now) : "только что"} · ${everyText(everySeconds)}`, isWarning: false };
    case "offline":
      return { text: metric.fetchedAtMs ? `Нет связи с GitHub. Значение получено ${agoText(metric.fetchedAtMs, now)}` : "Нет связи с GitHub. Значения пока нет", isWarning: true };
    case "rateLimited":
      return { text: `Лимит GitHub исчерпан. Следующий запрос в ${clock(metric.retryAtMs ?? now)}`, isWarning: true };
    case "notFound":
      return { text: "Репозиторий не найден или закрыт. Проверьте имя", isWarning: true };
  }
}

export function MetricCard({ source, everySeconds, index }: { source: MetricSource; everySeconds: number; index: number }) {
  const { metric, from, error } = useMetric(source, everySeconds);
  const [now, setNow] = useState(Date.now);
  const [delta, setDelta] = useState<number | null>(null);
  const card = useRef<HTMLElement>(null);
  const previous = useRef<number | null>(null);
  const value = metric?.value ?? null;
  const meta = sourceMeta(source.type);
  const status = statusLine(metric, error, everySeconds, now);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_TICK_MS);
    return () => window.clearInterval(timer);
  }, []);
  // The clock moves when a new reading arrives, not only on the tick.
  useEffect(() => setNow(Date.now()), [metric]);

  // A real change, not the first reading, gets the small "+1" and a glow.
  useEffect(() => {
    const before = previous.current;
    previous.current = value;
    if (before === null || value === null || before === value) return;
    setDelta(value - before);
    const timer = window.setTimeout(() => setDelta(null), DELTA_MS);
    if (!prefersReducedMotion()) {
      card.current?.animate([{ opacity: 0 }, { opacity: 1, offset: 0.18 }, { opacity: 0 }], { duration: 1400, easing: "cubic-bezier(0.22, 1, 0.36, 1)", pseudoElement: "::after" });
    }
    return () => window.clearTimeout(timer);
  }, [value]);

  return (
    <section ref={card} className={`card metric-card reveal${status.isWarning ? " stale" : ""}`} aria-label={meta.title} style={{ ["--i" as string]: index }}>
      <div className="card-head">
        <Icon name={meta.icon} small className="muted" />
        <h2 className="title-4">{meta.title}</h2>
        <span className="spacer" />
        {from === "demo" && <span className="chip chip-demo">демо</span>}
        <span className={status.isWarning ? "live warn" : "live"} aria-hidden="true" />
      </div>
      <div className="metric-value">
        {value === null ? <span className="odo muted" aria-hidden="true">—</span> : <Odometer value={value} />}
        <span className="sr-only" data-metric-value="">{value === null ? "значения нет" : String(value)}</span>
        {delta !== null && (
          <span key={String(value)} className={`metric-delta ${delta > 0 ? "up" : "down"}`} role="status">
            {delta > 0 ? "+" : "−"}{Math.abs(delta)}
          </span>
        )}
      </div>
      <div className="metric-foot caption-5 muted">
        <span className="mono">{source.repo}</span>
        <span style={status.isWarning ? { color: "var(--amber)" } : undefined} data-metric-status="">{status.text}</span>
        <span>Запрос к api.github.com: только имя репозитория</span>
      </div>
    </section>
  );
}
