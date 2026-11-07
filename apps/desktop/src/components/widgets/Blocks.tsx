import type { ReactNode } from "react";
import type { BuiltinBlock } from "../../lib/bindings/BuiltinBlock";
import type { DecisionDto } from "../../lib/bindings/DecisionDto";
import type { TaskDto } from "../../lib/bindings/TaskDto";
import { BRANCH, COMMITS, GATE_COUNTS, WEEKLY_COMMITS } from "../../lib/demo";
import type { Route } from "../../lib/router";
import { GROUP_LABELS, presentState, type StateGroup } from "../../lib/taskState";
import type { Resource } from "../../lib/useResource";
import { DependencyGraph } from "../DependencyGraph";
import { SourceChip } from "../Feedback";
import { Icon } from "../Icon";

const GROUPS: StateGroup[] = ["ready", "active", "waiting", "closed"];
const GROUP_GLYPH: Record<StateGroup, string> = { ready: "g-ready", active: "g-run", waiting: "g-wait", closed: "g-done" };

function areaPath(values: number[], width: number, height: number): { line: string; fill: string } {
  const max = Math.max(...values) * 1.08;
  const points = values.map((value, index) => [8 + ((width - 16) * index) / (values.length - 1), height - 4 - (value / max) * (height - 16)]);
  const line = points.map(([x, y], index) => `${index ? "L" : "M"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  return { line, fill: `${line} L${width - 8},${height - 4} L8,${height - 4} Z` };
}

export type BlockData = {
  projectName: string | null;
  tasks: Resource<TaskDto[]>;
  decisions: Resource<DecisionDto[]>;
  navigate: (route: Route) => void;
};

/** The stock overview blocks. Each one stands alone, so any of them can be removed. */
export function Block({ block, data, index }: { block: BuiltinBlock; data: BlockData; index: number }): ReactNode {
  const { projectName, tasks, decisions, navigate } = data;
  const all = tasks.status === "ok" ? tasks.data : [];
  const counts = GROUPS.map((group) => all.filter((task) => presentState(task.state).group === group).length);
  const order = { ["--i" as string]: index };

  switch (block) {
    case "summary": {
      const closedShare = all.length ? (counts[3] / all.length) * 100 : 0;
      return (
        <section className="card reveal" aria-label="Сводка" style={{ display: "flex", flexDirection: "column", gap: 12, ...order }}>
          <div className="row">
            <span className="caption-4 muted ellip">{projectName ?? "Проект не выбран"}</span>
            <span className="spacer" />
            {tasks.status === "ok" && <SourceChip source={tasks.source} />}
          </div>
          <p className="body-6 muted">
            {tasks.status === "loading" ? "Читаем задачи из agentd." : `${counts[0]} готовы к запуску, ${counts[1]} в работе, ${counts[2]} ждут.`}
          </p>
          <span className="spacer" />
          <div className="row" style={{ alignItems: "baseline", gap: 12 }}>
            <span className="big-num">{tasks.status === "ok" ? all.length : "—"}</span>
            <span className="body-6 muted">задач, закрыто {counts[3]}</span>
          </div>
          <div className="meter" role="img" aria-label={`${counts[3]} из ${all.length} задач закрыты`}>
            <span style={{ width: `${closedShare}%`, background: "var(--done)" }} />
          </div>
        </section>
      );
    }
    case "states":
      return (
        <section className="card reveal" aria-labelledby="st-title" style={order}>
          <div className="card-head"><h2 id="st-title" className="title-4">Состояния задач</h2></div>
          <div className="stack" style={{ gap: 12 }}>
            {GROUPS.map((group, position) => (
              <div key={group} className="row" style={{ gap: 12 }}>
                <span className="state" style={{ width: 104 }}>
                  <span className={`g ${GROUP_GLYPH[group]}`} aria-hidden="true" />
                  {GROUP_LABELS[group]}
                </span>
                <span className="meter" style={{ flex: 1 }} aria-hidden="true">
                  <span style={{ width: `${all.length ? (counts[position] / all.length) * 100 : 0}%`, background: group === "closed" ? "var(--done)" : "var(--accent-text)" }} />
                </span>
                <span className="mono tnum" style={{ width: 32, textAlign: "right" }}>{counts[position]}</span>
              </div>
            ))}
          </div>
        </section>
      );
    case "graph":
      return (
        <section className="card reveal" aria-labelledby="graph-title" style={order}>
          <div className="card-head">
            <h2 id="graph-title" className="title-4">Граф зависимостей G15 · Git Graph &amp; Operations</h2>
            <span className="spacer" />
            <span className="chip chip-demo">снимок queue.yaml, демо</span>
          </div>
          <DependencyGraph />
          <p className="caption-5 muted" style={{ marginTop: 8 }}>
            Синяя линия: критический путь 0217 → 0228, 10 задач. Пунктир: параллельные ветки 0221 и 0222, их рёбра идут в своих каналах.
          </p>
        </section>
      );
    case "commits": {
      const area = areaPath(WEEKLY_COMMITS, 216, 96);
      const gateMax = Math.max(...GATE_COUNTS);
      return (
        <section className="card reveal" aria-labelledby="weeks-title" style={order}>
          <div className="card-head"><h2 id="weeks-title" className="title-4">Коммиты по неделям</h2><span className="spacer" /><span className="chip chip-demo">демо</span></div>
          <svg width="216" height="96" role="img" aria-label="Коммиты за недели W37–W43, пик 167 в W38" style={{ display: "block" }}>
            <path d={area.fill} style={{ fill: "var(--accent-soft)" }} />
            <path d={area.line} style={{ fill: "none", stroke: "var(--accent-text)", strokeWidth: 2 }} />
          </svg>
          <div className="row caption-5 muted" style={{ justifyContent: "space-between", width: 216 }}><span>W37</span><span>W43</span></div>
          <div className="bars" style={{ height: 40, gap: 4, marginTop: 12 }} aria-hidden="true">
            {GATE_COUNTS.map((count, position) => (
              <div key={position}><span className={position <= 13 ? "bar-fill" : "bar-plan"} style={{ height: Math.max(4, (count / gateMax) * 36) }} /></div>
            ))}
          </div>
          <p className="caption-5 muted" style={{ marginTop: 4 }}>Задачи по gates G0–G15</p>
        </section>
      );
    }
    case "branch":
      return (
        <section className="card list reveal" aria-labelledby="branch-title" style={order}>
          <div className="card-head"><Icon name="gitBranch" small className="muted" /><h2 id="branch-title" className="title-4 ellip">{BRANCH}</h2></div>
          {COMMITS.slice().reverse().map(([hash, , message]) => (
            <div key={hash} className="tr" style={{ gridTemplateColumns: "56px minmax(0,1fr)", gap: 8, minHeight: 32 }}>
              <span className="mono muted">{hash}</span>
              <span className="caption-5 ellip">{message}</span>
            </div>
          ))}
        </section>
      );
    case "attention": {
      const pending = decisions.status === "ok" ? decisions.data : [];
      return (
        <section className="card list reveal" aria-labelledby="att-title" style={order}>
          <div className="card-head">
            <h2 id="att-title" className="title-4">Требует внимания</h2>
            <span className="spacer" />
            <a className="caption-4 accent" href="#" onClick={(event) => { event.preventDefault(); navigate({ name: "attention" }); }}>Все</a>
          </div>
          {decisions.status === "ok" && pending.length === 0 && <p className="caption-5 muted" style={{ padding: "0 12px 8px" }}>Запросов на решение нет.</p>}
          {decisions.status === "error" && <p className="caption-5 muted" style={{ padding: "0 12px 8px" }}>Очередь недоступна: {decisions.error.message}</p>}
          {pending.slice(0, 3).map((decision) => (
            <a key={decision.requestId} className="tr" href="#" style={{ gridTemplateColumns: "12px minmax(0,1fr)", gap: 12, paddingBlock: 8 }}
              onClick={(event) => { event.preventDefault(); navigate({ name: "attention" }); }}>
              <span className="g g-attn" aria-hidden="true" />
              <span className="stack" style={{ gap: 2, minWidth: 0 }}>
                <span className="caption-4" style={{ color: "var(--amber)" }}>Требуется решение · {decision.taskId}</span>
                <span className="body-5 ellip">{decision.reason}</span>
              </span>
            </a>
          ))}
        </section>
      );
    }
  }
}
