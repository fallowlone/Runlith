import { useState } from "react";
import type { DecisionDto } from "../lib/bindings/DecisionDto";
import type { Route } from "../lib/router";
import { expiryNote, isExpired, waitingFor } from "../lib/decisionTime";
import { DECISION_LABELS } from "../lib/taskState";
import type { Resource } from "../lib/useResource";
import { Empty, ErrorState, SkeletonRows, SourceChip } from "../components/Feedback";
import { Icon } from "../components/Icon";
import { TopBar } from "../components/TopBar";

type Props = {
  decisions: Resource<DecisionDto[]>;
  reload: () => void;
  navigate: (route: Route) => void;
  query: string;
  onQuery: (value: string) => void;
};

const formatTime = (ms: number): string =>
  new Intl.DateTimeFormat("ru-RU", { dateStyle: "medium", timeStyle: "short" }).format(new Date(ms));

export function Attention({ decisions, reload, navigate, query, onQuery }: Props) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const items = decisions.status === "ok" ? decisions.data : [];
  const selected = items.find((item) => item.requestId === selectedId) ?? items[0];
  // ponytail: age refreshes with the screen, no ticker; add one if this screen stays open for long.
  const now = Date.now();

  return (
    <main className="tile">
      <TopBar query={query} onQuery={(value) => { onQuery(value); if (value) navigate({ name: "tasks" }); }}>
        <button className="btn btn-ghost" type="button" onClick={reload}><Icon name="arrowsClockwise" small />Обновить</button>
      </TopBar>
      <div className="view">
        <div className="view-head">
          <div className="stack" style={{ gap: 4 }}>
            <h1 className="heading-5" tabIndex={-1}>Внимание</h1>
            <p className="caption-5 muted">Блокировка касается только своей области. Остальная работа продолжается.</p>
          </div>
          {decisions.status === "ok" && <SourceChip source={decisions.source} />}
        </div>
        {decisions.status === "error" && <ErrorState error={decisions.error} onRetry={reload} />}
        {decisions.status === "loading" && <section className="card list"><SkeletonRows rows={3} label="Очередь загружается" /></section>}
        {decisions.status === "ok" && items.length === 0 && (
          <section className="card">
            <Empty icon="check" title="Запросов на решение нет" text="Когда агенту понадобится выбор человека, запрос появится здесь и в счётчике слева." />
          </section>
        )}
        {selected && (
          <div className="bento att-split">
            <section className="card list reveal" aria-label="Очередь внимания">
              {items.map((item) => (
                <button key={item.requestId} type="button" className={`tr${item.requestId === selected.requestId ? " hl" : ""}`}
                  aria-current={item.requestId === selected.requestId ? "true" : undefined}
                  style={{ gridTemplateColumns: "12px minmax(0,1fr)", gap: 12, paddingBlock: 12, alignItems: "start" }}
                  onClick={() => setSelectedId(item.requestId)}>
                  <span className="g g-attn" aria-hidden="true" style={{ marginTop: 4 }} />
                  <span className="stack" style={{ gap: 2, minWidth: 0 }}>
                    <span className="caption-4" style={{ color: "var(--amber)" }}>Требуется решение</span>
                    <span className="body-5 ellip">{item.reason}</span>
                    <span className="mono muted ellip">{item.taskId}</span>
                    <span className="caption-5 muted tnum" style={{ marginTop: 2 }}>
                      {waitingFor(item.createdAtMs, now)} · <span style={isExpired(item.expiresAtMs, now) ? { color: "var(--amber)" } : undefined}>{expiryNote(item.expiresAtMs, now)}</span>
                    </span>
                  </span>
                </button>
              ))}
            </section>
            <DecisionDetail key={selected.requestId} decision={selected} navigate={navigate} now={now} />
          </div>
        )}
      </div>
    </main>
  );
}

function DecisionDetail({ decision, navigate, now }: { decision: DecisionDto; navigate: (route: Route) => void; now: number }) {
  const [choice, setChoice] = useState(decision.recommendation ?? decision.options[0] ?? "");
  return (
    <section className="card reveal" aria-labelledby="d-title" style={{ display: "flex", flexDirection: "column", gap: 24, padding: 24, ["--i" as string]: 1 }}>
      <div className="stack" style={{ gap: 12 }}>
        <span className="chip chip-amber" style={{ alignSelf: "flex-start" }}><span className="g g-attn" aria-hidden="true" />Human Decision · {decision.status}</span>
        <h2 id="d-title" className="heading-6">{decision.reason}</h2>
        <div className="row caption-5 muted" style={{ flexWrap: "wrap", gap: 12 }}>
          <a className="mono accent link-target" href="#" onClick={(event) => { event.preventDefault(); navigate({ name: "task", id: decision.taskId, tab: "overview" }); }}>{decision.taskId}</a>
          <span>риск: {decision.risk || "не указан"}</span>
          <span>создан {formatTime(decision.createdAtMs)}</span>
          {decision.expiresAtMs === null ? <span>бессрочно</span> : (
            <span style={isExpired(decision.expiresAtMs, now) ? { color: "var(--amber)" } : undefined}>
              {isExpired(decision.expiresAtMs, now) ? "истёк" : "истекает"} {formatTime(decision.expiresAtMs)}
            </span>
          )}
        </div>
      </div>
      {decision.affectedScope.length > 0 && (
        <div className="stack">
          <h3 className="title-4">Затронутая область</h3>
          <div className="row" style={{ flexWrap: "wrap", gap: 4 }}>{decision.affectedScope.map((scope) => <span key={scope} className="chip mono">{scope}</span>)}</div>
        </div>
      )}
      <fieldset style={{ border: 0, margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 8 }}>
        <legend className="title-4" style={{ padding: 0, marginBottom: 12 }}>Варианты</legend>
        {decision.options.map((option) => (
          <label key={option} style={{ display: "flex", flexWrap: "wrap", gap: 12, alignItems: "center", padding: "12px 16px", borderRadius: 12, background: "var(--raise-2)", cursor: "pointer", boxShadow: choice === option ? "inset 0 0 0 2px var(--accent-text)" : "none" }}>
            <input type="radio" name="decision" value={option} checked={choice === option} onChange={() => setChoice(option)} style={{ margin: 0, accentColor: "var(--accent)" }} />
            <span className="body-5">{DECISION_LABELS[option] ?? option}</span>
            <span className="mono muted">{option}</span>
            {decision.recommendation === option && <span className="chip chip-accent" style={{ marginLeft: "auto" }}>рекомендация</span>}
          </label>
        ))}
      </fieldset>
      {decision.evidence.length > 0 && (
        <div className="stack">
          <h3 className="title-4">Evidence</h3>
          {decision.evidence.map((item) => <span key={item} className="mono muted">{item}</span>)}
        </div>
      )}
      <div className="stack">
        <div className="row">
          <button className="btn btn-primary" type="button" disabled aria-describedby="decide-note"><Icon name="check" small />Записать выбор</button>
        </div>
        <p id="decide-note" className="caption-5 muted">
          Запись решений из интерфейса пока не подключена. Сейчас: <span className="mono">agentctl human-decision decide {decision.requestId} {choice.toLowerCase()}</span>
        </p>
      </div>
    </section>
  );
}
