/**
 * Project map: a canvas of everything the agent recorded about this repository.
 * Crates and their dependencies are real (cargo metadata); the grouping into
 * capabilities is the agent's record (src/lib/map/capabilities.ts), labelled demo
 * until agentd serves it. Layout comes precomputed from ELK (scripts/gen-maps.ts).
 */
import { useMemo, useState } from "react";
import {
  MAPS, capFacts, capTitle, capabilityOf, crateFacts, cratePath, highlight, roundedPath,
  type CapNode, type Highlight, type Selection,
} from "../lib/map/data";
import { Icon } from "../components/Icon";
import { PanZoom, type FocusBox } from "../components/PanZoom";

const confidence = (perMille: number): string => (perMille / 1000).toFixed(2).replace(".", ",");

function capClass(node: CapNode, hl: Highlight | null): string {
  const base = `cap${node.validated ? "" : " inferred"}`;
  if (!hl) return base;
  if (hl.primaryCap === node.id) return `${base} sel`;
  return hl.caps.has(node.id) ? `${base} rel` : `${base} dim`;
}

export function MapView() {
  const [selection, setSelection] = useState<Selection>(null);
  const [focus, setFocus] = useState<FocusBox | null>(null);
  const hl = useMemo(() => highlight(selection), [selection]);

  /** Selecting a capability (or a crate from the panel) brings its card into readable view. */
  const select = (next: Selection, center = true): void => {
    setSelection(next);
    const capId = next?.kind === "cap" ? next.id : next ? capabilityOf(next.name) : null;
    const node = MAPS.map.nodes.find((n) => n.id === capId);
    if (center && node) setFocus((prev) => ({ x: node.x, y: node.y, w: node.w, h: node.h, seq: (prev?.seq ?? 0) + 1 }));
  };
  const crateCount = MAPS.crates.length;

  return (
    <main className="tile">
      <div className="bar" data-tauri-drag-region>
        <h1 className="heading-6" tabIndex={-1}>Карта проекта</h1>
        <span className="caption-5 muted">
          {MAPS.map.nodes.length} возможностей · {crateCount} crates · {MAPS.crateEdges.length} связей
        </span>
        <span className="spacer" data-tauri-drag-region />
        <span className="chip chip-live">crates и связи: cargo metadata</span>
        <span className="chip chip-demo">группировка: запись агента, демо</span>
      </div>
      <div className="map-body">
        <PanZoom width={MAPS.map.width} height={MAPS.map.height} label="Карта возможностей проекта" focus={focus} onBackground={() => setSelection(null)}>
          <svg width={MAPS.map.width} height={MAPS.map.height} aria-hidden="true" style={{ position: "absolute", inset: 0, overflow: "visible" }}>
            {MAPS.map.edges.map((edge) => {
              const isHot = hl ? hl.caps.has(edge.from) && hl.caps.has(edge.to) : false;
              return (
                <path key={`${edge.from}>${edge.to}`} className={`edge${hl ? (isHot ? " hot" : " dim") : ""}`}
                  data-from={edge.from} data-to={edge.to} d={roundedPath(edge.points, 10)} />
              );
            })}
          </svg>
          {MAPS.map.nodes.map((node) => (
            <div key={node.id} className={capClass(node, hl)} data-node={node.id}
              style={{ left: node.x, top: node.y, width: node.w, height: node.h }}>
              <button className="cap-title" type="button" aria-pressed={selection?.kind === "cap" && selection.id === node.id}
                onClick={() => select({ kind: "cap", id: node.id })}>
                <span className="title-4 ellip">{node.title}</span>
                <span className="cap-conf" title="Уверенность агента">{confidence(node.confidence)}</span>
              </button>
              <p className="caption-5 muted ellip">{node.summary}</p>
              <ul className="cap-crates">
                {node.crates.map((name) => {
                  const isSelected = selection?.kind === "crate" && selection.name === name;
                  const cls = isSelected ? " sel" : hl?.crates.has(name) ? " hit" : "";
                  return (
                    <li key={name}>
                      <button className={`crate${cls}`} type="button" aria-pressed={isSelected}
                        onClick={() => select({ kind: "crate", name }, false)}>{name}</button>
                    </li>
                  );
                })}
              </ul>
              <span className="cap-count" aria-hidden="true">{node.crates.length} crates</span>
            </div>
          ))}
        </PanZoom>
        <aside className="card map-panel" aria-label="Детали" aria-live="polite">
          <Details selection={selection} select={select} />
        </aside>
      </div>
    </main>
  );
}

function CapLinks({ ids, select }: { ids: string[]; select: (s: Selection) => void }) {
  if (ids.length === 0) return <p className="caption-5 muted">Нет</p>;
  return (
    <div className="linklist">
      {ids.map((id) => (
        <button key={id} className="chip" type="button" onClick={() => select({ kind: "cap", id })}>{capTitle(id)}</button>
      ))}
    </div>
  );
}

function CrateLinks({ names, select }: { names: string[]; select: (s: Selection) => void }) {
  if (names.length === 0) return <p className="caption-5 muted">Нет</p>;
  return (
    <div className="linklist">
      {names.map((name) => (
        <button key={name} className="chip mono" type="button" onClick={() => select({ kind: "crate", name })}>{name}</button>
      ))}
    </div>
  );
}

function Details({ selection, select }: { selection: Selection; select: (s: Selection) => void }) {
  if (!selection) {
    return (
      <>
        <div className="stack">
          <h2 className="title-3">Как читать карту</h2>
          <p className="caption-5 muted">Выберите возможность или crate: связи подсветятся, здесь появятся источники и то, что затронет изменение.</p>
        </div>
        <div className="stack caption-5">
          <span className="row"><span className="cap inferred" style={{ position: "static", width: 24, height: 16, padding: 0 }} aria-hidden="true" />Пунктир: вывел агент, не проверено человеком</span>
          <span className="row"><span className="cap" style={{ position: "static", width: 24, height: 16, padding: 0 }} aria-hidden="true" />Сплошная рамка: проверено</span>
          <span className="muted">Линия ведёт от зависимого блока сверху к зависимости ниже. Показаны {MAPS.map.edges.length} связей из {MAPS.capabilityEdgesAll.length}: остальные следуют из них транзитивно, поэтому ответ «что затронет» не меняется.</span>
        </div>
        <div className="notice"><Icon name="warningDiamond" small /><p className="caption-5">Группировку в agentd пока не прочитать: нужен read RPC над memory_core (записи SEMANTIC с anchors CODE). Связи между crates реальные.</p></div>
        <p className="caption-5 muted">Колесо или трекпад двигают полотно, щипок или ⌘ с колесом масштабируют. С клавиатуры: стрелки, + и −, 0 вписывает, Escape снимает выбор.</p>
      </>
    );
  }

  if (selection.kind === "crate") {
    const facts = crateFacts(selection.name);
    const cap = capabilityOf(selection.name);
    const affectedCaps = [...new Set(facts.affected.map(capabilityOf).filter((c): c is string => c !== null && c !== cap))];
    return (
      <>
        <div className="stack">
          <span className="chip chip-live" style={{ alignSelf: "flex-start" }}>crate · cargo metadata</span>
          <h2 className="heading-6 mono" style={{ fontSize: 20 }}>{selection.name}</h2>
          <span className="mono muted">{cratePath(selection.name)}</span>
          {cap && <button className="chip" type="button" style={{ alignSelf: "flex-start" }} onClick={() => select({ kind: "cap", id: cap })}>{capTitle(cap)}</button>}
        </div>
        <section className="stack" aria-labelledby="aff">
          <h3 id="aff" className="title-4">Затронет при изменении</h3>
          <p className="caption-5 muted tnum">Напрямую: {facts.directDependents.length}. Всего crates: {facts.affected.length}. Других возможностей: {affectedCaps.length}.</p>
          <CrateLinks names={facts.directDependents} select={select} />
          {affectedCaps.length > 0 && <CapLinks ids={affectedCaps} select={select} />}
        </section>
        <section className="stack" aria-labelledby="dep">
          <h3 id="dep" className="title-4">Зависит от</h3>
          <CrateLinks names={facts.dependsOn} select={select} />
        </section>
      </>
    );
  }

  const node = MAPS.map.nodes.find((n) => n.id === selection.id);
  if (!node) return null;
  const facts = capFacts(node.id);
  return (
    <>
      <div className="stack">
        <div className="row" style={{ flexWrap: "wrap", gap: 4 }}>
          <span className="chip chip-demo">{node.validated ? "проверено" : "вывел агент"}</span>
          <span className="chip mono">уверенность {confidence(node.confidence)}</span>
        </div>
        <h2 className="heading-6">{node.title}</h2>
        <p className="body-6 muted">{node.summary}</p>
      </div>
      <section className="stack" aria-labelledby="aff">
        <h3 id="aff" className="title-4">Затронет при изменении</h3>
        <CapLinks ids={facts.affected} select={select} />
      </section>
      <section className="stack" aria-labelledby="dep">
        <h3 id="dep" className="title-4">Зависит от</h3>
        <CapLinks ids={facts.dependsOn} select={select} />
      </section>
      <section className="stack" aria-labelledby="src">
        <h3 id="src" className="title-4">Источники</h3>
        {node.crates.map((name) => (
          <div key={name} className="row" style={{ gap: 8 }}>
            <span className="chip mono" style={{ height: 24, padding: "0 8px" }}>FILE</span>
            <button className="mono ellip" type="button" style={{ textAlign: "left" }} onClick={() => select({ kind: "crate", name })}>{cratePath(name)}</button>
          </div>
        ))}
      </section>
    </>
  );
}
