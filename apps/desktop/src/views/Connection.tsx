import type { ConnectionDto } from "../lib/bindings/ConnectionDto";
import { inTauri } from "../lib/api";
import { Icon } from "../components/Icon";

type Props = {
  connection: ConnectionDto | null;
  observedAt: Date | null;
  onCheck: () => void;
};

const COPY: Record<ConnectionDto["state"], { chip: string; glyph: string; label: string; title: string; text: string }> = {
  connected: { chip: "chip chip-accent", glyph: "live", label: "Подключено", title: "Протокол совместим", text: "Handshake выполнен, версии согласованы. Данные читаются из agentd." },
  unavailable: { chip: "chip chip-red", glyph: "g g-fail", label: "Нет связи", title: "agentd не запущен", text: "Запустите daemon: agentctl start. Это не значит, что работа остановлена: интерфейс просто его не видит." },
  incompatible: { chip: "chip chip-amber", glyph: "g g-attn", label: "Несовместимо", title: "Версия daemon не поддерживается", text: "Клиент не читает данные, чтобы не создать впечатление, что задач нет." },
  security: { chip: "chip chip-red", glyph: "g g-fail", label: "Заблокировано", title: "Endpoint не прошёл проверку", text: "Сокет принадлежит другому пользователю или имеет небезопасные права. Другой endpoint пробовать нельзя." },
  error: { chip: "chip chip-red", glyph: "g g-fail", label: "Ошибка", title: "Не удалось подключиться", text: "Подробности ниже." },
};

const time = (date: Date | null): string => (date ? date.toLocaleTimeString("ru-RU") : "—");

export function Connection({ connection, observedAt, onCheck }: Props) {
  const copy = connection ? COPY[connection.state] : null;
  return (
    <main className="tile">
      <div className="bar" data-tauri-drag-region>
        <h1 className="heading-6" tabIndex={-1}>Подключение и совместимость</h1>
        <span className="spacer" data-tauri-drag-region />
        <button className="btn btn-ghost" type="button" onClick={onCheck}><Icon name="arrowsClockwise" small />Проверить связь</button>
      </div>
      <div className="view">
        {!inTauri && <span className="chip chip-demo" style={{ alignSelf: "flex-start" }}>демо: запущено вне Tauri</span>}
        {copy && connection ? (
          <section className="card reveal" aria-live="polite" style={{ display: "flex", flexDirection: "column", gap: 16, padding: 24, maxWidth: 640 }}>
            <span className={copy.chip} style={{ alignSelf: "flex-start" }}><span className={copy.glyph} aria-hidden="true" />{copy.label}</span>
            <div className="stack"><h2 className="heading-6">{copy.title}</h2><p className="body-6 muted">{copy.text}</p></div>
            <dl className="stack" style={{ margin: 0, gap: 0 }}>
              {[
                ["Клиент", connection.clientBuild],
                ["agentd", connection.daemonBuild ?? "—"],
                ["Протокол", connection.protocol !== null ? String(connection.protocol) : "—"],
                ["Последнее наблюдение", time(observedAt)],
              ].map(([term, value]) => (
                <div key={term} className="tr" style={{ gridTemplateColumns: "minmax(0,1fr) auto", minHeight: 36, padding: 0, borderRadius: 0 }}>
                  <dt className="caption-5 muted">{term}</dt>
                  <dd className="mono" style={{ margin: 0 }}>{value}</dd>
                </div>
              ))}
            </dl>
            {connection.detail && <p className="mono muted" style={{ overflowWrap: "anywhere" }}>{connection.detail}</p>}
          </section>
        ) : (
          <section className="card" aria-busy="true" style={{ maxWidth: 640 }}><div className="sk" style={{ width: "40%" }} /></section>
        )}
        <div className="notice info" style={{ maxWidth: 640 }}>
          <Icon name="plugsConnected" />
          <p className="body-6">Потеря соединения не означает остановку работы: фоновые задачи продолжаются, пока daemon жив. Интерфейс проверяет связь каждые 5 секунд и показывает время последнего наблюдения.</p>
        </div>
      </div>
    </main>
  );
}
