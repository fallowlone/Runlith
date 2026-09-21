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
  const tones = {
    accent: "text-accent",
    attention: "text-attention",
    positive: "text-positive",
  } as const;
  return (
    <button
      type="button"
      onClick={onClick}
      className="card flex flex-col items-start gap-1 px-5 py-4 text-left hover:shadow-[var(--shadow-pop)]"
    >
      <span className="text-xs font-medium text-ink2">{label}</span>
      <span className={cn("text-3xl font-bold leading-9", tones[tone])}>{value}</span>
      <span className="text-xs text-ink2">
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
      <h1 className="mb-1 text-[28px] font-bold leading-9 tracking-tight">Good afternoon</h1>
      <p className="mb-4 text-sm text-ink2">
        What changed, what is running, what waits on you — {F.project.name}
      </p>
      <div className="grid grid-cols-3 gap-4">
        <StatCard label="In work" value={inWork} tone="accent" />
        <StatCard label="Needs attention" value={needsAttention} tone="attention" onClick={onNeedsAttention} />
        <StatCard label="Completed" value={completed} tone="positive" />
      </div>
    </div>
  );
}