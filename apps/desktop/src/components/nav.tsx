/* Left navigation (§9.1), light Cursor/Rippling-style sidebar: grouped
   sections, gray-fill selection, utility zone pinned to the bottom. */
import { FIXTURE } from "../fixture";
import { Badge, cn } from "./ui";

const F = FIXTURE;

export function Nav({
  density,
  textSize,
  onDensity,
  onTextSize,
  onOpenExplorer,
}: {
  density: string;
  textSize: string;
  onDensity: (v: string) => void;
  onTextSize: (v: string) => void;
  onOpenExplorer: () => void;
}) {
  return (
    <nav aria-label="Project navigation" className="flex w-[240px] shrink-0 flex-col">
      {/* Project switcher (§9.1) */}
      <button
        type="button"
        className="row-h flex w-full items-center gap-2.5 rounded-[8px] px-2.5 text-left hover:bg-surface"
      >
        <span
          aria-hidden
          className="flex size-7 shrink-0 items-center justify-center rounded-full bg-accent text-xs font-bold text-white"
        >
          R
        </span>
        <strong className="min-w-0 flex-1 truncate text-sm">{F.project.name}</strong>
        <Badge className="!border-accent/30 !bg-accent/10 !text-accent">local</Badge>
        <svg aria-hidden width="12" height="12" viewBox="0 0 12 12" className="shrink-0 text-ink2">
          <path d="M3 4.5 6 7.5 9 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
        </svg>
      </button>

      {/* Tree for parent-child only (§14.4): children are indented under
          their parent; dependency edges belong to the Plan graph instead. */}
      <div role="tree" aria-label="Tasks" className="mt-3 px-2">
        <p className="px-2.5 pb-1 text-xs font-semibold text-ink2">Tasks</p>
        {F.tasks.map((task) => (
          <a
            key={task.id}
            href="#"
            role="treeitem"
            aria-level={task.parent ? 2 : 1}
            onClick={(e) => e.preventDefault()}
            aria-current={task.selected ? "true" : undefined}
            className={cn(
              "row-h flex items-center gap-2 rounded-[8px] px-2.5 text-sm no-underline hover:bg-surface",
              task.parent && "ml-5 border-l border-edge pl-2",
              task.selected && "bg-surface font-medium",
            )}
          >
            <span className="min-w-0 flex-1 truncate">{task.objective}</span>
            <Badge state={task.state}>{task.state}</Badge>
          </a>
        ))}
      </div>

      <div role="region" aria-label="Attention" className="mt-3 px-2">
        <p className="px-2.5 pb-1 text-xs font-semibold text-ink2">Attention</p>
        {F.attention.map((a) => (
          <a
            key={a.id}
            href="#"
            onClick={(e) => e.preventDefault()}
            className="row-h flex items-center gap-2 rounded-[8px] px-2.5 text-sm no-underline hover:bg-surface"
          >
            <Badge state={a.status === "PENDING" ? "BLOCKED" : undefined}>{a.type.split(" ")[0]}</Badge>
            <span className="min-w-0 flex-1 truncate text-xs text-ink2">
              {a.id} · {a.task}
            </span>
          </a>
        ))}
        {/* §9.1: Execution Explorer is a secondary diagnostic route, never a
            primary navigation item. */}
        <button
          type="button"
          onClick={onOpenExplorer}
          className="row-h flex w-full items-center gap-2 rounded-[8px] px-2.5 text-left text-xs text-ink2 hover:bg-surface hover:text-ink"
        >
          <svg aria-hidden width="12" height="12" viewBox="0 0 12 12" className="shrink-0">
            <path d="M1 6h3l1.5-3.5L8 9l1.5-3H11" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
          Execution Explorer
          <span className="ml-auto text-[10px]">diagnostics</span>
        </button>
      </div>

      <div className="mt-auto border-t border-edge p-2">
        <div title={F.connection.note} className="flex items-center gap-2 rounded-[8px] px-2 py-1.5">
          <span aria-hidden className="size-2 shrink-0 rounded-full bg-positive" />
          {/* min-w-0 + truncate: the utility row degrades gracefully at any width */}
          <span className="min-w-0 flex-1 truncate text-xs text-ink2">
            {F.connection.state} · {F.connection.protocol}
          </span>
          <span className="shrink-0 text-xs text-ink2">{F.connection.observed}</span>
        </div>
        {/* Stacked label/field rows: nothing clips at 240px or below. */}
        <div className="grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1.5 px-2 pb-1.5">
          <label htmlFor="density" className="text-xs text-ink2">
            Density
          </label>
          <select
            id="density"
            value={density}
            onChange={(e) => onDensity(e.target.value)}
            className="h-7 w-full min-w-0 rounded-[6px] border border-edge bg-shell px-1.5 text-xs"
          >
            <option value="compact">Compact</option>
            <option value="default">Default</option>
            <option value="comfortable">Comfortable</option>
          </select>
          <label htmlFor="textsize" className="text-xs text-ink2">
            Text
          </label>
          <select
            id="textsize"
            value={textSize}
            onChange={(e) => onTextSize(e.target.value)}
            className="h-7 w-full min-w-0 rounded-[6px] border border-edge bg-shell px-1.5 text-xs"
          >
            <option value="normal">100%</option>
            <option value="large">150%</option>
          </select>
        </div>
        <button
          type="button"
          className="row-h flex w-full items-center gap-2.5 rounded-[8px] px-2.5 text-left text-sm hover:bg-surface"
        >
          <svg aria-hidden width="15" height="15" viewBox="0 0 15 15" className="shrink-0 text-ink2">
            <circle cx="7.5" cy="7.5" r="2.2" fill="none" stroke="currentColor" strokeWidth="1.4" />
            <path
              d="M7.5 1.2v2m0 8.6v2M1.2 7.5h2m8.6 0h2M3.2 3.2l1.4 1.4m5.8 5.8 1.4 1.4m0-8.6-1.4 1.4M4.6 10.4l-1.4 1.4"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
            />
          </svg>
          Settings
        </button>
      </div>
    </nav>
  );
}