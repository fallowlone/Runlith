import { useEffect, useRef, useState } from "react";
import type { MetricSource } from "../../lib/bindings/MetricSource";
import type { Refresh } from "../../lib/bindings/Refresh";
import type { SourceType } from "../../lib/bindings/SourceType";
import type { Span } from "../../lib/bindings/Span";
import type { Widget } from "../../lib/bindings/Widget";
import { api, toUiError } from "../../lib/api";
import { failureText, isRepoName, REFRESH_OPTIONS, sourceMeta } from "../../lib/widgetText";
import { Icon } from "../Icon";

const POLL_MS = 500;
const GIVE_UP_MS = 90_000;
const DEFAULT_REFRESH = 300;
const EXAMPLES = ["сколько звёзд у fallowlone/runlith", "сколько сейчас открыто пулреквестов"];

type Phase =
  | { name: "compose"; problem: string | null; refused: boolean }
  | { name: "thinking"; jobId: string | null }
  | { name: "proposal"; source: SourceType };

type Props = {
  span: Span;
  onSpan: (span: Span) => void;
  lastRepo: string;
  onAdd: (widget: Widget) => void;
  onClose: () => void;
};

function SpanPicker({ span, onSpan, label }: { span: Span; onSpan: (span: Span) => void; label: string }) {
  return (
    <div className="seg widget-size" role="group" aria-label={label}>
      {([1, 2] as const).map((value) => (
        <button key={value} type="button" aria-pressed={span === value} onClick={() => onSpan(value)}>
          {value === 1 ? "1 место" : "2 места"}
        </button>
      ))}
    </div>
  );
}

export { SpanPicker };

/** A description becomes a widget: words, then a proposal to check, then the grid. */
export function WidgetAuthor({ span, onSpan, lastRepo, onAdd, onClose }: Props) {
  const [text, setText] = useState("");
  const [phase, setPhase] = useState<Phase>({ name: "compose", problem: null, refused: false });
  const [repo, setRepo] = useState(lastRepo);
  const [every, setEvery] = useState(DEFAULT_REFRESH);
  const activeJob = useRef<string | null>(null);
  const isGone = useRef(false);
  const card = useRef<HTMLElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  const stop = useRef<HTMLButtonElement>(null);
  const confirm = useRef<HTMLInputElement>(null);

  // Leaving mid-generation must not keep a provider call running.
  useEffect(() => {
    isGone.current = false;
    return () => {
      isGone.current = true;
      if (activeJob.current) void api.widgetDraftCancel(activeJob.current).catch(() => undefined);
      activeJob.current = null;
    };
  }, []);

  const poll = (jobId: string, startedAt: number): void => {
    api.widgetDraftStatus(jobId).then(
      ({ data }) => {
        if (activeJob.current !== jobId) return;
        if (data.state === "running") {
          if (Date.now() - startedAt > GIVE_UP_MS) {
            void api.widgetDraftCancel(jobId).catch(() => undefined);
            activeJob.current = null;
            setPhase({ name: "compose", problem: failureText("timed_out", null), refused: false });
            return;
          }
          window.setTimeout(() => poll(jobId, startedAt), POLL_MS);
          return;
        }
        activeJob.current = null;
        if (data.state === "proposed" && data.source) {
          if (data.repo) setRepo(data.repo);
          setPhase({ name: "proposal", source: data.source });
        } else if (data.state === "refused") {
          setPhase({ name: "compose", problem: null, refused: true });
        } else if (data.state === "cancelled") {
          setPhase({ name: "compose", problem: null, refused: false });
        } else {
          setPhase({ name: "compose", problem: failureText(data.failureCode, data.failureDetail), refused: false });
        }
      },
      (error: unknown) => {
        activeJob.current = null;
        setPhase({ name: "compose", problem: toUiError(error).message, refused: false });
      },
    );
  };

  const ask = (description: string): void => {
    setPhase({ name: "thinking", jobId: null });
    api.widgetDraftStart(description).then(
      ({ data }) => {
        if (isGone.current) {
          void api.widgetDraftCancel(data.jobId).catch(() => undefined);
          return;
        }
        activeJob.current = data.jobId;
        setPhase({ name: "thinking", jobId: data.jobId });
        poll(data.jobId, Date.now());
      },
      (error: unknown) => setPhase({ name: "compose", problem: toUiError(error).message, refused: false }),
    );
  };

  const cancel = (): void => {
    if (phase.name === "thinking" && phase.jobId) {
      void api.widgetDraftCancel(phase.jobId).catch(() => undefined);
    }
  };

  // Each step keeps the keyboard in the card and the whole card on screen.
  useEffect(() => {
    const target = phase.name === "proposal" ? confirm.current : phase.name === "compose" ? field.current : stop.current;
    target?.focus({ preventScroll: true });
    card.current?.scrollIntoView({ block: "nearest" });
  }, [phase]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      if (phase.name === "thinking") cancel();
      else onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });

  const isThinking = phase.name === "thinking";
  const trimmedRepo = repo.trim();
  const repoOk = isRepoName(trimmedRepo);
  const spec = phase.name === "proposal"
    ? {
        kind: "metric" as const,
        span,
        source: { type: phase.source, repo: trimmedRepo } satisfies MetricSource,
        refresh: { everySeconds: every } satisfies Refresh,
        motion: "roll" as const,
      }
    : null;

  return (
    <section ref={card} className="card widget-author" aria-label="Новый виджет">
      <div className="aurora" data-state={isThinking ? "working" : "idle"} aria-hidden="true" />
      <div className="widget-author-head">
        <SpanPicker span={span} onSpan={onSpan} label="Ширина нового виджета" />
        <button className="btn btn-quiet" type="button" onClick={onClose}>Закрыть</button>
      </div>

      {phase.name !== "proposal" && (
        <form
          className="stack"
          style={{ gap: 12 }}
          onSubmit={(event) => {
            event.preventDefault();
            if (text.trim()) ask(text);
          }}
        >
          <label className="caption-4 muted" htmlFor="widget-ask">Что должен показывать виджет</label>
          <textarea
            ref={field}
            id="widget-ask"
            className="widget-field"
            rows={3}
            maxLength={500}
            placeholder={`Например: ${EXAMPLES[0]}`}
            value={text}
            disabled={isThinking}
            aria-describedby="widget-ask-note"
            onChange={(event) => setText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <p id="widget-ask-note" className="caption-5 muted">Описание уйдёт в Claude через ваш claude CLI. Файлы проекта Claude не получит.</p>
          {phase.name === "compose" && phase.refused && (
            <div className="notice" role="status">
              <Icon name="warningDiamond" small />
              <div className="stack" style={{ gap: 8 }}>
                <p className="caption-5"><b>Такого источника пока нет.</b> Сейчас виджет может показать звёзды и открытые пул-реквесты репозитория GitHub.</p>
                <div className="row" style={{ flexWrap: "wrap" }}>
                  {EXAMPLES.map((example) => (
                    <button key={example} className="btn btn-ghost widget-example" type="button" onClick={() => { setText(example); ask(example); }}>{example}</button>
                  ))}
                </div>
              </div>
            </div>
          )}
          {phase.name === "compose" && phase.problem && (
            <div className="notice err" role="alert">
              <Icon name="warningDiamond" small />
              <p className="caption-5">{phase.problem}</p>
            </div>
          )}
          <div className="row" style={{ flexWrap: "wrap" }}>
            {isThinking ? (
              <>
                <span className="caption-4" role="status">Claude собирает виджет</span>
                <button ref={stop} className="btn btn-ghost" type="button" disabled={!phase.jobId} onClick={cancel}>Остановить</button>
              </>
            ) : (
              <button className="btn btn-primary" type="submit" disabled={!text.trim()}><Icon name="sparkle" small />Создать виджет</button>
            )}
          </div>
        </form>
      )}

      {phase.name === "proposal" && spec && (
        <div className="stack" style={{ gap: 12 }}>
          <div className="row" style={{ alignItems: "center" }}>
            <Icon name={sourceMeta(phase.source).icon} small className="muted" />
            <h3 className="title-4">{sourceMeta(phase.source).title}</h3>
          </div>
          <div className="stack" style={{ gap: 4 }}>
            <label className="caption-4 muted" htmlFor="widget-repo">Репозиторий</label>
            <input
              ref={confirm}
              id="widget-repo"
              className="widget-field mono"
              value={repo}
              placeholder="владелец/имя"
              spellCheck={false}
              aria-invalid={!repoOk}
              aria-describedby="widget-repo-note"
              onChange={(event) => setRepo(event.target.value)}
            />
            <p id="widget-repo-note" className="caption-5 muted">
              {repoOk ? "Запрос к api.github.com отправит только это имя." : "Укажите репозиторий в виде владелец/имя."}
            </p>
          </div>
          <div className="stack" style={{ gap: 4 }}>
            <span className="caption-4 muted" id="widget-every">Как часто обновлять</span>
            <div className="seg" role="group" aria-labelledby="widget-every" style={{ alignSelf: "flex-start" }}>
              {REFRESH_OPTIONS.map(([seconds, label]) => (
                <button key={seconds} type="button" aria-pressed={every === seconds} onClick={() => setEvery(seconds)}>{label}</button>
              ))}
            </div>
          </div>
          <details className="widget-spec">
            <summary className="caption-4 muted">Что сохранится: данные, а не код</summary>
            <pre className="mono" tabIndex={0}>{JSON.stringify(spec, null, 2)}</pre>
          </details>
          <div className="row" style={{ flexWrap: "wrap" }}>
            <button
              className="btn btn-primary"
              type="button"
              disabled={!repoOk}
              onClick={() => onAdd({ ...spec, id: `w-${Date.now().toString(36)}`, source: { ...spec.source } })}
            >
              <Icon name="plus" small />Добавить на главную
            </button>
            <button className="btn btn-ghost" type="button" onClick={() => setPhase({ name: "compose", problem: null, refused: false })}>Изменить описание</button>
          </div>
        </div>
      )}
    </section>
  );
}
