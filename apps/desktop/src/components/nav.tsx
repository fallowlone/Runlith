/* Left navigation (§9.1): Project, Tasks, Attention; utility zone with
   connection state and the density / text-size fixtures. */
import { FIXTURE } from "../fixture";
import { Badge, cn } from "./ui";

const F = FIXTURE;

export function Nav({
  density,
  textSize,
  onDensity,
  onTextSize,
}: {
  density: string;
  textSize: string;
  onDensity: (v: string) => void;
  onTextSize: (v: string) => void;
}) {
  return (
    <nav aria-label="Project navigation" className="flex w-[220px] shrink-0 flex-col rounded-[6px] bg-shell">
      <div className="px-2 pt-3">
        <div className="row-h flex items-center gap-2 rounded-[4px] px-3">
          <strong>{F.project.name}</strong>
          <span className="min-w-0 truncate text-xs text-ink2">{F.project.repo}</span>
        </div>
      </div>

      <div role="region" aria-label="Tasks" className="px-2 pt-2">
        <p className="px-3 pb-1 text-xs text-ink2">Tasks</p>
        {F.tasks.map((task) => (
          <a
            key={task.id}
            href="#"
            onClick={(e) => e.preventDefault()}
            aria-current={task.selected ? "true" : undefined}
            className={cn(
              "row-h flex items-center gap-2 rounded-[4px] px-3 text-sm no-underline hover:bg-surface",
              task.selected && "bg-surface shadow-[inset_2px_0_0_var(--color-accent)]",
            )}
          >
            <span className="shrink-0 font-mono text-xs text-ink2">{task.id.replace("TASK-", "")}</span>
            <span className="min-w-0 flex-1 truncate">{task.objective}</span>
            <Badge state={task.state}>{task.state}</Badge>
          </a>
        ))}
      </div>

      <div role="region" aria-label="Attention" className="px-2 pt-2">
        <p className="px-3 pb-1 text-xs text-ink2">Attention ({F.attention.length})</p>
        {F.attention.map((a) => (
          <a
            key={a.id}
            href="#"
            onClick={(e) => e.preventDefault()}
            className="row-h flex items-center gap-2 rounded-[4px] px-3 text-sm no-underline hover:bg-surface"
          >
            <Badge state={a.status === "PENDING" ? "BLOCKED" : undefined}>{a.type.split(" ")[0]}</Badge>
            <span className="min-w-0 flex-1 truncate text-xs text-ink2">
              {a.id} · {a.task}
            </span>
          </a>
        ))}
      </div>

      <div className="mt-auto border-t border-edge p-2">
        <div title={F.connection.note} className="flex items-center gap-2 rounded-[4px] bg-surface px-2 py-1.5">
          <span aria-hidden className="size-2 shrink-0 rounded-full bg-positive" />
          <span className="min-w-0 flex-1 truncate text-xs text-ink2">
            {F.connection.state} · {F.connection.protocol}
          </span>
          <span className="shrink-0 text-xs text-ink2">{F.connection.observed}</span>
        </div>
        <div className="mt-2 flex items-center gap-2">
          <label htmlFor="density" className="text-xs text-ink2">
            Density
          </label>
          <select
            id="density"
            value={density}
            onChange={(e) => onDensity(e.target.value)}
            className="h-7 rounded-[4px] border border-white/15 bg-surface px-1.5 text-xs"
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
            className="h-7 rounded-[4px] border border-white/15 bg-surface px-1.5 text-xs"
          >
            <option value="normal">100%</option>
            <option value="large">150%</option>
          </select>
        </div>
      </div>
    </nav>
  );
}