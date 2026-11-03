import { useEffect, useRef, useState, type FormEvent } from "react";
import { Icon } from "./Icon";

type Props = {
  context: string;
  onClose: () => void;
  files: string[];
  onRemoveFile: (path: string) => void;
};

/**
 * The agent chat surface. agentd has no transcript, message ids or streaming
 * yet, so sending is honestly unavailable: the text is kept, nothing is lost.
 */
export function AgentPanel({ context, onClose, files, onRemoveFile }: Props) {
  const [text, setText] = useState("");
  const [isFocused, setFocused] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const input = useRef<HTMLTextAreaElement>(null);

  // Attaching a file means the person is about to ask: put the cursor there.
  useEffect(() => {
    if (files.length > 0) input.current?.focus();
  }, [files.length]);

  const handleSubmit = (event: FormEvent): void => {
    event.preventDefault();
    if (!text.trim()) return;
    setNotice("Сообщение не отправлено: в agentd пока нет канала чата. Текст сохранён.");
  };

  return (
    <aside className="tile" aria-label="Агент">
      <div className="agent-head">
        <Icon name="sparkle" className="accent" />
        <h2 className="title-3">Агент</h2>
        <span className="spacer" />
        <span className="chip chip-live ellip" style={{ maxWidth: 160 }}>{context}</span>
        <button className="btn btn-icon btn-quiet" type="button" aria-label="Скрыть панель агента" title="⌘ ⌥ B" onClick={onClose}>
          <Icon name="sidebarSimple" />
        </button>
      </div>
      <div className="msgs">
        <div className="msg-ai">
          <p className="body-6">Здесь будет диалог с агентом по текущему экрану.</p>
          <p className="caption-5 muted">
            Для него agentd должен отдавать transcript, ID сообщений и потоковые ответы. Сейчас канал последовательный и без
            истории, поэтому интерфейс ничего не выдумывает.
          </p>
          <p className="trace"><Icon name="shieldCheck" small />Только чтение через agentd</p>
        </div>
        {notice && (
          <div className="notice" role="status">
            <Icon name="warningDiamond" small />
            <p className="caption-5">{notice}</p>
          </div>
        )}
      </div>
      <div className="aurora" data-state={isFocused ? "working" : "idle"} aria-hidden="true" />
      <div className="composer-wrap">
        <form className="composer" aria-label="Сообщение агенту" onSubmit={handleSubmit}>
          {files.length > 0 && (
            <div className="composer-atts">
              {files.map((path) => {
                const name = path.split("/").pop() ?? path;
                return (
                  <span key={path} className="att" title={path}>
                    <Icon name="fileCode" small className="muted" />
                    <span className="ellip">{name}</span>
                    <button className="att-x" type="button" aria-label={`Убрать вложение ${name}`} onClick={() => onRemoveFile(path)}>
                      <Icon name="x" small />
                    </button>
                  </span>
                );
              })}
            </div>
          )}
          <label className="sr-only" htmlFor="agent-ask">Вопрос агенту</label>
          <textarea
            ref={input}
            id="agent-ask"
            className="prompt"
            rows={2}
            placeholder={`Задайте вопрос: ${context}`}
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              setNotice(null);
            }}
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                event.currentTarget.form?.requestSubmit();
              }
            }}
          />
          <div className="row">
            <button className="btn btn-icon btn-quiet" type="button" aria-label="Добавить вложение: нет контракта вложений" aria-disabled="true">
              <Icon name="plus" />
            </button>
            <button className="btn btn-icon btn-quiet" type="button" aria-label="Упомянуть файл или задачу: нет контракта" aria-disabled="true">
              <Icon name="at" />
            </button>
            <span className="spacer" />
            <span className="caption-5 muted">Enter</span>
            <button className="btn btn-icon btn-send" type="submit" aria-label="Отправить" disabled={!text.trim()}>
              <Icon name="arrowUp" />
            </button>
          </div>
        </form>
      </div>
    </aside>
  );
}
