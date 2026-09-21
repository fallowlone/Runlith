/* Shadcn-style primitives (ADR-0015: shadcn primitives only). These follow
   the shadcn/ui component API (cva variants + cn() class merging); migrating
   to generated shadcn/ui components must remain drop-in. */
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
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[4px] border text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-45 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
  {
    variants: {
      variant: {
        default: "border-white/15 bg-surface text-ink hover:border-white/30",
        primary: "border-accent bg-accent font-semibold text-[#0B0E12] hover:border-accent",
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

/* ---------- Badge (state first, then color — §12.1) ---------- */

const lifecycleDot: Partial<Record<TaskLifecycle, string>> = {
  CLAIMED: "bg-accent",
  RUNNING: "bg-accent",
  VERIFYING: "bg-accent",
  SUBMITTED: "bg-accent",
  BLOCKED: "bg-attention",
  AWAITING_REVIEW: "bg-attention",
  COMPLETED: "bg-positive",
  FAILED: "bg-danger",
};

export function Badge({
  state,
  children,
  className,
  title,
}: {
  state?: TaskLifecycle;
  children: ReactNode;
  className?: string;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-full border border-white/15 px-2 py-0.5 text-xs leading-4 text-ink",
        !state && "text-ink2",
        className,
      )}
    >
      <span aria-hidden className={cn("size-2 rounded-full", state ? lifecycleDot[state] ?? "bg-ink2" : "bg-ink2")} />
      {children}
    </span>
  );
}

/* ---------- Panel & group (surfaces group content; thin dividers) ---------- */

export function Panel({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("min-h-0 flex flex-col rounded-[6px] bg-shell", className)} {...props} />;
}

export function Group({ title, children, className }: { title: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn("mb-4 overflow-hidden rounded-[6px] border border-edge", className)}>
      <h2 className="bg-surface px-3 py-2.5 text-xs font-semibold uppercase tracking-wider text-ink2">{title}</h2>
      <div className="p-3">{children}</div>
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
    info: "border-accent/35 bg-accent/8",
    attention: "border-attention/35 bg-attention/8",
    danger: "border-danger/35 bg-danger/8",
  } as const;
  return (
    <div className={cn("mb-4 rounded-[6px] border p-3 text-sm leading-5", tones[tone])}>
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
    <div role="tablist" aria-label={label} className={cn("flex gap-1 border-b border-edge px-4 pt-2", className)}>
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          type="button"
          aria-selected={active === t.id}
          onClick={() => onChange(t.id)}
          className={cn(
            "-mb-px rounded-t-[4px] border-b-2 px-3 py-2 text-sm",
            active === t.id
              ? "border-accent bg-transparent text-ink"
              : "border-transparent text-ink2 hover:bg-surface hover:text-ink",
          )}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}