import type { ReactNode } from "react";
import { Icon } from "./Icon";

type Props = {
  query: string;
  onQuery: (value: string) => void;
  children?: ReactNode;
};

/** Main tile header: global search (⌘K) plus per screen actions. Empty space drags the window. */
export function TopBar({ query, onQuery, children }: Props) {
  return (
    <div className="bar" data-tauri-drag-region>
      <label className="search">
        <Icon name="magnifyingGlass" small />
        <span className="sr-only">Поиск по задачам</span>
        <input
          id="global-search"
          type="search"
          placeholder="Поиск: ID или цель задачи"
          value={query}
          onChange={(event) => onQuery(event.target.value)}
        />
        <span className="kbd" aria-hidden="true">⌘ K</span>
      </label>
      <span className="spacer" data-tauri-drag-region />
      {children}
    </div>
  );
}

type CrumbProps = {
  onBack: () => void;
  trail: ReactNode;
  children?: ReactNode;
};

export function CrumbBar({ onBack, trail, children }: CrumbProps) {
  return (
    <div className="bar" data-tauri-drag-region>
      <button className="btn btn-icon btn-quiet" type="button" aria-label="Назад к задачам" title="⌘ [" onClick={onBack}>
        <Icon name="arrowLeft" />
      </button>
      <nav className="crumbs" aria-label="Путь">{trail}</nav>
      <span className="spacer" data-tauri-drag-region />
      {children}
    </div>
  );
}
