/* Overview hero + stat cards, light dashboard direction. All numbers are
   derived from the fixture (no invented totals); domain honesty rules still
   apply (VERIFIED ≠ COMPLETED etc.). */
import { FIXTURE } from "../fixture";
import { cn } from "./ui";

const F = FIXTURE;

function StatCard({
  label,
  value,
  tone,
  onClick,
}: {
  label: string;
  value: number;
  tone: "accent" | "attention" | "positive";
  onClick?: () => void;
}) {
  const dots = {
    accent: "bg-accent",
    attention: "bg-attention",
    positive: "bg-positive",
  } as const;
  return (
    <button
      type="button"
      onClick={onClick}
      className="card flex flex-col items-start gap-1 px-5 py-4 text-left hover:shadow-[var(--shadow-pop)]"
    >
      <span className="flex items-center gap-1.5 text-xs font-medium text-ink2">
        <span aria-hidden className={cn("size-1.5 rounded-full", dots[tone])} />
        {label}
      </span>
      {/* Numbers stay in ink: color on the dot carries the state, the figure
          itself must not look decorated (light-theme polish). */}
      <span className="text-[32px] font-semibold leading-10 tracking-tight text-ink tabular-nums">{value}</span>
      <span className="truncate text-xs text-ink2">
        {label === "Needs attention"
          ? "HDR-0092 · TASK-0344"
          : label === "Completed"
            ? "TASK-0338 · evidence archived"
            : "TASK-0342 · TASK-0355"}
      </span>
    </button>
  );
}

export function OverviewHero({ onNeedsAttention }: { onNeedsAttention: () => void }) {
  const inWork = F.tasks.filter((t) => ["VERIFYING", "RUNNING", "CLAIMED"].includes(t.state)).length;
  const needsAttention = F.attention.filter((a) => a.status === "PENDING").length;
  const completed = F.tasks.filter((t) => t.state === "COMPLETED").length;
  return (
    <div className="mb-5">
      <h1 className="mb-1 text-[26px] font-bold leading-8 tracking-tight">Good afternoon</h1>
      <p className="mb-4 text-sm text-ink2">What changed, what is running, what waits on you — {F.project.name}</p>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <StatCard label="In work" value={inWork} tone="accent" />
        <StatCard label="Needs attention" value={needsAttention} tone="attention" onClick={onNeedsAttention} />
        <StatCard label="Completed" value={completed} tone="positive" />
      </div>
    </div>
  );
}