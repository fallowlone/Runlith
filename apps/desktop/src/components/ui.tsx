/* Shadcn-style primitives (ADR-0015: shadcn primitives only). These follow
   the shadcn/ui component API (cva variants + cn() class merging); migrating
   to generated shadcn/ui components must remain drop-in. Light theme,
   Cursor/Warp direction. */
import { cva, type VariantProps } from "class-variance-authority";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import type { ButtonHTMLAttributes, HTMLAttributes, ReactNode } from "react";
import type { TaskLifecycle } from "../fixture";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/* ---------- Button ---------- */

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[6px] border text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-45 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
  {
    variants: {
      variant: {
        default: "border-edge bg-shell text-ink shadow-[0_1px_2px_rgba(16,20,25,0.05)] hover:bg-surface",
        primary: "border-accent bg-accent font-semibold text-white hover:bg-accent/90",
        ghost: "border-transparent bg-transparent text-ink2 hover:bg-surface hover:text-ink",
      },
      size: {
        default: "h-8 px-3.5",
        sm: "h-7 px-2.5 text-xs",
        icon: "h-8 w-8",
      },
    },
    defaultVariants: { variant: "default", size: "default" },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export function Button({ className, variant, size, type = "button", ...props }: ButtonProps) {
  return <button type={type} className={cn(buttonVariants({ variant, size }), className)} {...props} />;
}

/* ---------- Badge (state first, then color — §12.1) ----------
   Soft tinted chips: background and text carry the state, the dot is the
   color-independent carrier. Neutral states stay gray. */

const lifecycleChip: Partial<Record<TaskLifecycle, { chip: string; dot: string }>> = {
  CLAIMED: { chip: "border-transparent bg-accent/10 text-accent", dot: "bg-accent" },
  RUNNING: { chip: "border-transparent bg-accent/10 text-accent", dot: "bg-accent" },
  VERIFYING: { chip: "border-transparent bg-accent/10 text-accent", dot: "bg-accent" },
  SUBMITTED: { chip: "border-transparent bg-accent/10 text-accent", dot: "bg-accent" },
  BLOCKED: { chip: "border-transparent bg-attention/12 text-attention", dot: "bg-attention" },
  AWAITING_REVIEW: { chip: "border-transparent bg-attention/12 text-attention", dot: "bg-attention" },
  COMPLETED: { chip: "border-transparent bg-positive/12 text-positive", dot: "bg-positive" },
  FAILED: { chip: "border-transparent bg-danger/10 text-danger", dot: "bg-danger" },
};

/* Presentation labels for canonical enum values (§12.2); the enum itself is
   never renamed and stays in title/aria. Attempt outcomes use the same map. */
export const PRETTY_STATE: Record<string, string> = {
  DRAFT: "Draft",
  READY: "Ready",
  CLAIMED: "Claimed",
  RUNNING: "Running",
  BLOCKED: "Blocked",
  SUBMITTED: "Submitted",
  AWAITING_REVIEW: "Awaiting review",
  VERIFYING: "Verifying",
  RETRY_READY: "Retry ready",
  COMPLETED: "Completed",
  FAILED: "Failed",
  CANCELLED: "Cancelled",
  // Attempt outcomes (concluded, immutable)
  SUCCEEDED: "Succeeded",
  TIMED_OUT: "Timed out",
};

export function Badge({
  state,
  children,
  className,
  title,
}: {
  state?: TaskLifecycle;
  children?: ReactNode;
  className?: string;
  title?: string;
}) {
  const chip = state ? lifecycleChip[state] : undefined;
  // Canonical enum stays accessible via title; the visible label is a
  // friendly presentation form (research §12.2 candidate labels).
  const pretty = state ? PRETTY_STATE[state] : undefined;
  return (
    <span
      title={title ?? (state ? `${state}` : undefined)}
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2 py-0.5 text-xs leading-4 font-medium",
        chip ? chip.chip : "border-edge bg-surface text-ink2",
        className,
      )}
    >
      <span aria-hidden className={cn("size-1.5 rounded-full", chip ? chip.dot : "bg-ink2")} />
      {children && children !== state ? children : pretty ?? children}
    </span>
  );
}

/* ---------- Panel & group (white cards, soft shadow, thin dividers) ---------- */

export function Panel({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("bg-shell min-h-0 flex flex-col rounded-[10px] border border-edge shadow-[var(--shadow-card)]", className)} {...props} />;
}

export function Group({ title, children, className }: { title: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn("card mb-4 overflow-hidden", className)}>
      <h2 className="border-b border-edge px-3.5 py-2.5 text-xs font-semibold uppercase tracking-wider text-ink2">{title}</h2>
      <div className="p-3.5">{children}</div>
    </section>
  );
}

export function Callout({
  tone = "info",
  title,
  children,
}: {
  tone?: "info" | "attention" | "danger";
  title?: ReactNode;
  children: ReactNode;
}) {
  const tones = {
    info: "border-accent/25 bg-accent/5",
    attention: "border-attention/30 bg-attention/5",
    danger: "border-danger/25 bg-danger/5",
  } as const;
  return (
    <div className={cn("mb-4 rounded-[8px] border p-3 text-sm leading-5", tones[tone])}>
      {title ? <strong className="mr-1">{title}</strong> : null}
      {children}
    </div>
  );
}

/* ---------- Tabs (keyboard-operable, aria-correct) ---------- */

export interface TabDef {
  id: string;
  label: string;
}

export function Tabs({
  tabs,
  active,
  onChange,
  label,
  className,
}: {
  tabs: TabDef[];
  active: string;
  onChange: (id: string) => void;
  label: string;
  className?: string;
}) {
  return (
    <div role="tablist" aria-label={label} className={cn("flex gap-1 overflow-x-auto border-b border-edge px-4 pt-2", className)}>
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          type="button"
          aria-selected={active === t.id}
          onClick={() => onChange(t.id)}
          className={cn(
            "-mb-px rounded-t-[6px] border-b-2 px-3 py-2 text-sm",
            active === t.id
              ? "border-accent bg-transparent font-medium text-ink"
              : "border-transparent text-ink2 hover:bg-surface hover:text-ink",
          )}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}