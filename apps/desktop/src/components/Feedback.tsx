import type { UiError } from "../lib/bindings/UiError";
import type { Source } from "../lib/api";
import { presentState, VERDICTS, type Verdict } from "../lib/taskState";
import { Icon, type IconName } from "./Icon";

export function StateLabel({ state, className = "" }: { state: string; className?: string }) {
  const view = presentState(state);
  return (
    <span className={`state ${view.tone} ${className}`}>
      <span className={`g ${view.glyph}`} aria-hidden="true" />
      {view.label}
    </span>
  );
}

export function VerdictLabel({ verdict }: { verdict: Verdict }) {
  const view = VERDICTS[verdict];
  return (
    <span className={`state ${view.tone}`}>
      <span className={`g ${view.glyph}`} aria-hidden="true" />
      {view.label}
    </span>
  );
}

export function SourceChip({ source, demoLabel = "демо данные" }: { source: Source; demoLabel?: string }) {
  return source === "live" ? (
    <span className="chip chip-live">agentd</span>
  ) : (
    <span className="chip chip-demo">{demoLabel}</span>
  );
}

export function SkeletonRows({ rows = 6, label }: { rows?: number; label: string }) {
  return (
    <div className="tbl" aria-busy="true" aria-label={label}>
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className="tr" style={{ gridTemplateColumns: "12px 96px minmax(0,1fr)" }}>
          <span className="sk" style={{ width: 10, height: 10 }} />
          <span className="sk" />
          <span className="sk" style={{ width: `${82 - ((index * 13) % 36)}%` }} />
        </div>
      ))}
    </div>
  );
}

interface EmptyProps {
  icon: IconName;
  title: string;
  text: string;
  action?: { label: string; onClick: () => void };
}

export function Empty({ icon, title, text, action }: EmptyProps) {
  return (
    <div className="empty">
      <span className="orb">
        <Icon name={icon} />
      </span>
      <h3 className="title-3">{title}</h3>
      <p className="caption-5 muted" style={{ maxWidth: 420 }}>
        {text}
      </p>
      {action && (
        <button className="btn btn-ghost" type="button" style={{ marginTop: 8 }} onClick={action.onClick}>
          {action.label}
        </button>
      )}
    </div>
  );
}

const ERROR_COPY: Record<UiError["kind"], { title: string; text: string }> = {
  unavailable: {
    title: "agentd не запущен",
    text: "Запустите daemon командой agentctl start. Фоновая работа не теряется: интерфейс просто его не видит.",
  },
  incompatible: {
    title: "Версия agentd не поддерживается",
    text: "Клиент не читает данные, чтобы не показать неверное состояние. Обновите agentd или приложение.",
  },
  security: {
    title: "Endpoint не прошёл проверку безопасности",
    text: "Подключение заблокировано. Другой endpoint интерфейс пробовать не будет.",
  },
  invalid: { title: "Некорректный запрос", text: "Интерфейс отправил данные, которые хост отклонил." },
  business: { title: "agentd отклонил запрос", text: "Причина указана ниже." },
  ambiguous: { title: "Ответ не получен", text: "Это чтение без побочных эффектов, его безопасно повторить." },
  internal: { title: "Внутренняя ошибка", text: "Подробности ниже." },
};

export function ErrorState({ error, onRetry }: { error: UiError; onRetry: () => void }) {
  const copy = ERROR_COPY[error.kind];
  const tone = error.kind === "unavailable" || error.kind === "ambiguous" ? "" : " err";
  return (
    <div className={`notice${tone}`} role="alert" style={{ alignItems: "center" }}>
      <Icon name={error.kind === "unavailable" ? "plug" : "warningDiamond"} />
      <div className="stack" style={{ gap: 2, flex: 1, minWidth: 0 }}>
        <p className="body-5">{copy.title}</p>
        <p className="caption-5 muted">{copy.text}</p>
        <p className="mono muted ellip">{error.message}</p>
      </div>
      <button className="btn btn-ghost" type="button" onClick={onRetry}>
        <Icon name="arrowsClockwise" small />
        Повторить
      </button>
    </div>
  );
}
