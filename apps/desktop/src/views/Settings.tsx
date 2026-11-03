import type { Density, Settings as SettingsValue, ThemePref } from "../lib/settings";
import { Icon, type IconName } from "../components/Icon";

type Props = {
  settings: SettingsValue;
  update: (patch: Partial<SettingsValue>) => void;
};

const THEMES: Array<[ThemePref, string, IconName]> = [["light", "Светлая", "sun"], ["dark", "Тёмная", "moon"], ["system", "Как в системе", "monitor"]];
const DENSITIES: Array<[Density, string]> = [[32, "Компактно · 32"], [40, "Обычно · 40"], [48, "Свободно · 48"]];
const KEYS: Array<[string, string]> = [["Глобальный поиск", "⌘ K"], ["Назад к списку задач", "⌘ ["], ["Следующая и предыдущая задача в списке", "J K"], ["Скрыть или показать панель агента", "⌘ ⌥ B"]];
const POLICY: Array<[string, string, string]> = [
  ["ALLOW", "chip", "Разрешено политикой. Это не значит, что операция уже выполнена."],
  ["REQUIRE_APPROVAL", "chip chip-amber", "Нужно разрешение человека на точную операцию, у разрешения есть срок действия."],
  ["DENY", "chip chip-red", "Жёсткий запрет. Кнопки «Разрешить» у него нет."],
];

export function Settings({ settings, update }: Props) {
  return (
    <main className="tile">
      <div className="bar" data-tauri-drag-region><h1 className="heading-6" tabIndex={-1}>Настройки</h1></div>
      <div className="view">
        <div className="bento" style={{ gridTemplateColumns: "repeat(2,minmax(0,1fr))", alignItems: "start" }}>
          <section className="card reveal" aria-labelledby="v-title" style={{ padding: 24, display: "flex", flexDirection: "column", gap: 20 }}>
            <h2 id="v-title" className="title-3">Вид</h2>
            <div className="stack">
              <span className="title-4" id="th-l">Тема</span>
              <div className="seg" role="group" aria-labelledby="th-l" style={{ alignSelf: "flex-start" }}>
                {THEMES.map(([value, label, icon]) => (
                  <button key={value} type="button" aria-pressed={settings.theme === value} onClick={() => update({ theme: value })}>
                    <Icon name={icon} small />{label}
                  </button>
                ))}
              </div>
            </div>
            <div className="stack">
              <span className="title-4" id="dn-l">Плотность строк</span>
              <div className="seg" role="group" aria-labelledby="dn-l" style={{ alignSelf: "flex-start" }}>
                {DENSITIES.map(([value, label]) => (
                  <button key={value} type="button" aria-pressed={settings.density === value} onClick={() => update({ density: value })}>{label}</button>
                ))}
              </div>
            </div>
            <label className="row" style={{ gap: 12, cursor: "pointer" }}>
              <input type="checkbox" checked={settings.reduceMotion} onChange={(event) => update({ reduceMotion: event.target.checked })}
                style={{ width: 16, height: 16, margin: 0, accentColor: "var(--accent)" }} />
              <span className="body-6">Уменьшить движение в интерфейсе. Системная настройка macOS действует всегда.</span>
            </label>
          </section>

          <section className="card reveal" aria-labelledby="k-title" style={{ padding: 24, ["--i" as string]: 1 }}>
            <h2 id="k-title" className="title-3" style={{ marginBottom: 8 }}>Клавиатура</h2>
            {KEYS.map(([name, keys]) => (
              <div key={name} className="tr" style={{ gridTemplateColumns: "minmax(0,1fr) auto", padding: 0, borderRadius: 0, minHeight: 40 }}>
                <span className="body-6">{name}</span><span className="kbd">{keys}</span>
              </div>
            ))}
          </section>

          <section className="card reveal" aria-labelledby="p-title" style={{ padding: 24, ["--i" as string]: 2 }}>
            <h2 id="p-title" className="title-3" style={{ marginBottom: 16 }}>Провайдеры</h2>
            <p className="caption-5 muted">Состояние провайдеров agentd пока не отдаёт через IPC. Ключи хранятся в системном keychain и в интерфейсе не показываются.</p>
          </section>

          <section className="card reveal" aria-labelledby="pol-title" style={{ padding: 24, ["--i" as string]: 3 }}>
            <div className="row" style={{ marginBottom: 12 }}><h2 id="pol-title" className="title-3">Решения политики</h2><span className="chip chip-live">справка</span></div>
            {POLICY.map(([name, chip, text]) => (
              <div key={name} className="tr" style={{ gridTemplateColumns: "168px minmax(0,1fr)", padding: "8px 0", borderRadius: 0, alignItems: "start" }}>
                <span className={chip} style={{ justifySelf: "start" }}><span className="mono">{name}</span></span>
                <span className="caption-5 muted" style={{ paddingTop: 6 }}>{text}</span>
              </div>
            ))}
          </section>
        </div>
      </div>
    </main>
  );
}
