/* Top command bar (Cursor-direction): global search affordance with ⌘K.
   Search execution requires the global-search read contract (§9.4) — the
   field is a navigation affordance fixture, not a working query surface. */
export function TopBar() {
  return (
    <div className="flex items-center gap-3 px-4 py-2">
      <div className="mx-auto flex h-9 w-full max-w-[480px] items-center gap-2.5 rounded-[8px] border border-edge bg-shell px-3 shadow-[0_1px_2px_rgba(16,20,25,0.05)]">
        <svg aria-hidden width="14" height="14" viewBox="0 0 14 14" className="shrink-0 text-ink2">
          <circle cx="6" cy="6" r="4.2" fill="none" stroke="currentColor" strokeWidth="1.4" />
          <path d="M9.2 9.2 12 12" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
        </svg>
        <input
          placeholder="Search for anything…"
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-ink2"
        />
        <kbd className="rounded-[5px] border border-edge bg-surface px-1.5 py-0.5 font-sans text-[11px] text-ink2">
          ⌘K
        </kbd>
      </div>
      <button
        type="button"
        title="Help"
        className="flex size-8 shrink-0 items-center justify-center rounded-[8px] text-ink2 hover:bg-surface hover:text-ink"
      >
        <svg aria-hidden width="15" height="15" viewBox="0 0 15 15">
          <circle cx="7.5" cy="7.5" r="6" fill="none" stroke="currentColor" strokeWidth="1.3" />
          <path d="M6 6a1.5 1.5 0 1 1 2.1 1.4c-.4.2-.6.5-.6 1v.3" fill="none" stroke="currentColor" strokeWidth="1.3" />
          <circle cx="7.5" cy="10.7" r="0.8" fill="currentColor" />
        </svg>
      </button>
    </div>
  );
}