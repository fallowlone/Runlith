import type { TaskDto } from "../lib/bindings/TaskDto";
import { api } from "../lib/api";
import { BRANCH, COMMITS, CRITERIA, FILES, PLAN_ROWS, SCOPE } from "../lib/demo";
import type { Route, Tab } from "../lib/router";
import { useResource } from "../lib/useResource";
import { Empty, ErrorState, SourceChip, StateLabel, VerdictLabel } from "../components/Feedback";
import { Icon, type IconName } from "../components/Icon";
import { CrumbBar } from "../components/TopBar";

type Props = {
  taskId: string;
  task: TaskDto | undefined;
  tab: Tab;
  navigate: (route: Route) => void;
  onAskFile: (path: string) => void;
};

const TABS: Array<[Tab, string, IconName]> = [
  ["overview", "Overview", "squaresFour"],
  ["plan", "Plan", "listDashes"],
  ["changes", "Changes", "gitDiff"],
  ["evidence", "Evidence", "sealCheck"],
  ["history", "History", "clockCounterClockwise"],
];

const DEMO_NOTE = "демо: ветка TASK-0229";
const FILE_WORDS: Partial<Record<Intl.LDMLPluralRule, string>> = { one: "файл", few: "файла" };
const FILES_NOTE = `${FILES.length} ${FILE_WORDS[new Intl.PluralRules("ru").select(FILES.length)] ?? "файлов"}`;
const VERIFIED_NOTE = `${CRITERIA.filter(([verdict]) => verdict === "verified").length} из ${CRITERIA.length} проверено`;

export function TaskWorkspace({ taskId, task, tab, navigate, onAskFile }: Props) {
  const toTab = (next: Tab) => navigate({ name: "task", id: taskId, tab: next });
  return (
    <main className="tile">
      <CrumbBar
        onBack={() => navigate({ name: "tasks" })}
        trail={<>
          <a href="#" onClick={(event) => { event.preventDefault(); navigate({ name: "tasks" }); }}>Задачи</a>
          <Icon name="caretRight" small />
          <span className="mono">{taskId}</span>
        </>}
      />
      <div className="view">
        <div className="view-head">
          <h1 className="heading-5 ellip" tabIndex={-1} style={{ maxWidth: "100%" }}>{task?.objective ?? taskId}</h1>
          {task && <span className="chip"><StateLabel state={task.state} className="caption-4" /></span>}
        </div>
        <nav className="seg" aria-label="Разделы задачи" style={{ alignSelf: "flex-start" }}>
          {TABS.map(([key, label, icon]) => (
            <a key={key} href="#" aria-current={tab === key ? "page" : undefined} onClick={(event) => { event.preventDefault(); toTab(key); }}>
              <Icon name={icon} small />
              {label}
              {key === "changes" && <span className="count muted" title={DEMO_NOTE}>{FILES_NOTE}</span>}
            </a>
          ))}
        </nav>
        {tab === "overview" && <OverviewTab taskId={taskId} task={task} navigate={navigate} />}
        {tab === "plan" && <PlanTab />}
        {tab === "changes" && <ChangesTab onAskFile={onAskFile} />}
        {tab === "evidence" && <EvidenceTab />}
        {tab === "history" && <HistoryTab />}
      </div>
    </main>
  );
}

function OverviewTab({ taskId, task, navigate }: { taskId: string; task: TaskDto | undefined; navigate: (route: Route) => void }) {
  const [deps, reload] = useResource(() => api.dependencies(taskId), taskId);
  return (
    <>
      <div className="bento bento-2">
        <section className="card reveal" aria-labelledby="t-title">
          <div className="card-head"><h2 id="t-title" className="title-4">Задача</h2><span className="spacer" />{task && <span className="chip chip-live">agentd</span>}</div>
          {task ? (
            <dl className="stack" style={{ margin: 0, gap: 8 }}>
              <div className="row"><dt className="caption-5 muted" style={{ width: 96 }}>ID</dt><dd className="mono" style={{ margin: 0 }}>{task.id}</dd></div>
              <div className="row"><dt className="caption-5 muted" style={{ width: 96 }}>Проект</dt><dd className="mono" style={{ margin: 0 }}>{task.projectId}</dd></div>
              <div className="row"><dt className="caption-5 muted" style={{ width: 96 }}>Состояние</dt><dd style={{ margin: 0 }}><StateLabel state={task.state} /> <span className="mono muted">{task.state}</span></dd></div>
              <div className="row" style={{ alignItems: "flex-start" }}><dt className="caption-5 muted" style={{ width: 96, flex: "none" }}>Цель</dt><dd className="body-6" style={{ margin: 0 }}>{task.objective}</dd></div>
            </dl>
          ) : (
            <p className="caption-5 muted">Задача не найдена в текущем списке проекта.</p>
          )}
        </section>
        <section className="card list reveal" aria-labelledby="d-title" style={{ ["--i" as string]: 1 }}>
          <div className="card-head"><h2 id="d-title" className="title-4">Зависит от</h2><span className="spacer" />{deps.status === "ok" && <SourceChip source={deps.source} />}</div>
          {deps.status === "loading" && <div className="sk" style={{ margin: 8, width: "60%" }} />}
          {deps.status === "error" && <ErrorState error={deps.error} onRetry={reload} />}
          {deps.status === "ok" && deps.data.length === 0 && <p className="caption-5 muted" style={{ padding: 8 }}>Зависимостей нет: задачу ничто не блокирует.</p>}
          {deps.status === "ok" && deps.data.map((id) => (
            <a key={id} className="tr" href="#" style={{ gridTemplateColumns: "20px minmax(0,1fr)", minHeight: 36 }}
              onClick={(event) => { event.preventDefault(); navigate({ name: "task", id, tab: "overview" }); }}>
              <Icon name="gitCommit" small className="muted" />
              <span className="mono">{id}</span>
            </a>
          ))}
        </section>
      </div>
      <section className="card reveal" aria-labelledby="sc-title" style={{ ["--i" as string]: 1 }}>
        <div className="card-head"><h2 id="sc-title" className="title-4">Область изменений</h2><span className="spacer" /><span className="chip chip-demo">{DEMO_NOTE}</span></div>
        <ul className="row" style={{ listStyle: "none", margin: 0, padding: 0, gap: 12, flexWrap: "wrap" }}>
          {SCOPE.map(([crate, area]) => (
            <li key={crate} className="card" style={{ padding: "8px 12px", borderRadius: 12, background: "var(--raise-2)", minWidth: 200 }}>
              <div className="title-4">{crate}</div><div className="mono muted">{area}</div>
            </li>
          ))}
        </ul>
      </section>
      <section className="card list reveal" aria-labelledby="c-title" style={{ ["--i" as string]: 2 }}>
        <div className="card-head"><h2 id="c-title" className="title-4">Критерии проверки и evidence</h2><span className="spacer" /><span className="caption-4 tnum">{VERIFIED_NOTE}</span><span className="chip chip-demo">{DEMO_NOTE}</span></div>
        {CRITERIA.map(([verdict, text, source]) => (
          <div key={text} className="tr tr-text cols-criteria">
            <VerdictLabel verdict={verdict} />
            <span className="body-6">{text}</span>
            <span className="caption-5 muted">{source}</span>
          </div>
        ))}
      </section>
      <section className="card reveal" aria-labelledby="tl-title" style={{ ["--i" as string]: 3 }}>
        <div className="card-head"><h2 id="tl-title" className="title-4">Хронология ветки</h2><span className="spacer" /><span className="mono muted ellip">{BRANCH} · git log</span></div>
        <ol className="row" style={{ listStyle: "none", margin: 0, padding: 0, gap: 16, alignItems: "flex-start", flexWrap: "wrap" }}>
          {COMMITS.map(([hash, , message]) => (
            <li key={hash} className="stack" style={{ gap: 4, flex: "0 0 140px", minWidth: 0 }}>
              <span className="g g-done" aria-hidden="true" />
              <span className="mono muted">{hash}</span>
              <span className="caption-5">{message.replace(/^\w+\(\w+\): /, "")}</span>
            </li>
          ))}
        </ol>
      </section>
    </>
  );
}

function ContractNotice({ name, text }: { name: string; text: string }) {
  return (
    <div className="notice reveal">
      <Icon name="warningDiamond" />
      <p className="body-6"><b>Контракта чтения {name} нет.</b> {text}</p>
    </div>
  );
}

function PlanTab() {
  return (
    <>
      <ContractNotice name="Plan" text="Показана ветка из planning/tasks/queue.yaml. Принятые PlanRevision и PlanningRun появятся после query контракта." />
      <section className="card list reveal" aria-labelledby="p-title" style={{ ["--i" as string]: 1 }}>
        <div className="card-head"><h2 id="p-title" className="title-4">Ветка плана · G15 Git Graph &amp; Operations</h2><span className="spacer" /><span className="chip chip-demo">queue.yaml</span></div>
        <div className="tbl" role="table" aria-label="Задачи ветки плана">
          <div className="tr th cols-plan" role="row"><span role="columnheader">Готовность</span><span role="columnheader">ID</span><span role="columnheader">Цель</span><span role="columnheader">Причина</span></div>
          {PLAN_ROWS.map(([state, id, objective, reason, nested]) => (
            <div key={id} className="tr cols-plan" role="row" style={{ minHeight: 36 }}>
              <span role="cell"><StateLabel state={state} /></span>
              <span role="cell" className="mono muted">{id}</span>
              <span role="cell" className="ellip" style={{ paddingLeft: nested ? 24 : 0 }}>{objective}</span>
              <span role="cell" className="caption-5 muted ellip">{reason}</span>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

function DiffMeter({ added, deleted, max }: { added: number; deleted: number; max: number }) {
  return (
    <span className="meter" aria-hidden="true">
      <span style={{ width: `${(added / max) * 100}%`, background: "var(--accent-text)" }} />
      <span style={{ width: `${(deleted / max) * 100}%`, background: "var(--red)" }} />
    </span>
  );
}

function ChangesTab({ onAskFile }: { onAskFile: (path: string) => void }) {
  return (
    <>
      <div className="row reveal" style={{ gap: 12, flexWrap: "wrap" }}>
        <h2 className="title-3">Изменения ветки</h2>
        <span className="mono muted">{BRANCH}</span>
        <span className="spacer" />
        <span className="row">
          <span className="chip chip-accent tnum">+513</span>
          <span className="chip chip-red tnum">−109</span>
          <span className="chip chip-demo">{DEMO_NOTE}</span>
        </span>
      </div>
      <section className="card list reveal" aria-labelledby="cm-title" style={{ ["--i" as string]: 1 }}>
        <div className="card-head"><h3 id="cm-title" className="title-4">Объём по коммитам</h3></div>
        {COMMITS.map(([hash, , message, , , added, deleted]) => (
          <div key={hash} className="tr cols-commits" style={{ minHeight: 36 }}>
            <span className="mono muted">{hash}</span>
            <span className="caption-5 ellip">{message}</span>
            <DiffMeter added={added} deleted={deleted} max={260} />
            <span className="mono tnum" style={{ textAlign: "right" }}>+{added} −{deleted}</span>
          </div>
        ))}
      </section>
      <section className="card list reveal" aria-labelledby="f-title" style={{ ["--i" as string]: 2 }}>
        <div className="card-head"><h3 id="f-title" className="title-4">Затронутые файлы</h3><span className="caption-5 muted">объединённо по ветке</span></div>
        {FILES.map(([path, added, deleted]) => (
          <div key={path} className="tr cols-files file-row" style={{ minHeight: 36 }}>
            <Icon name="fileCode" small className="muted" />
            <span className="path" title={path}>
              <span className="path-dir ellip">{path.slice(0, path.lastIndexOf("/") + 1)}</span>
              <span className="path-name ellip">{path.slice(path.lastIndexOf("/") + 1)}</span>
            </span>
            <DiffMeter added={added} deleted={deleted} max={460} />
            <span className="file-end">
              <span className="mono tnum file-num">+{added} −{deleted}</span>
              <button className="btn btn-ghost file-ask" type="button" aria-label={`Спросить агента про ${path}`} onClick={() => onAskFile(path)}>
                <Icon name="sparkle" small />
                Спросить
              </button>
            </span>
          </div>
        ))}
      </section>
    </>
  );
}

function EvidenceTab() {
  return (
    <>
      <ContractNotice name="Evidence" text="В API нет запроса к evidence и artifacts, поэтому вердикты ниже демонстрационные." />
      <section className="card list reveal" aria-labelledby="e-title" style={{ ["--i" as string]: 1 }}>
        <div className="card-head"><h2 id="e-title" className="title-4">Доказательства по критериям</h2><span className="spacer" /><span className="caption-4 tnum">{VERIFIED_NOTE}</span><span className="chip chip-demo">{DEMO_NOTE}</span></div>
        {CRITERIA.map(([verdict, text, source, snapshot]) => (
          <div key={text} className="tr tr-text cols-evidence">
            <span className="body-6">{text}</span>
            <VerdictLabel verdict={verdict} />
            <span className="caption-5 muted">{source}</span>
            <span className="mono muted">{snapshot}</span>
          </div>
        ))}
      </section>
      <section className="card reveal" aria-label="Критерий без доказательств" style={{ ["--i" as string]: 2 }}>
        <Empty icon="circleDashed" title="Доказательств пока нет" text="Это не «провалено» и не «проверено». Для критерия 3 ещё не записано ни одного evidence." />
      </section>
    </>
  );
}

function HistoryTab() {
  return (
    <section className="card list reveal" aria-labelledby="h-title">
      <div className="card-head"><h2 id="h-title" className="title-4">Журнал ветки · 20 октября</h2><span className="spacer" /><span className="chip chip-demo">{DEMO_NOTE}</span></div>
      {COMMITS.slice().reverse().map(([hash, time, message, lines]) => (
        <div key={hash} className="tr cols-history" style={{ minHeight: 36 }}>
          <span className="mono muted tnum">{time}</span>
          <span className="mono">{hash}</span>
          <span className="caption-5 ellip">{message}</span>
          <span className="caption-5 muted tnum" style={{ textAlign: "right" }}>{lines} строк</span>
        </div>
      ))}
      <div className="notice info" style={{ margin: 8 }}>
        <Icon name="clockCounterClockwise" small />
        <p className="caption-5">События Event Journal и логи Attempt появятся здесь после query контракта с курсорами.</p>
      </div>
    </section>
  );
}
