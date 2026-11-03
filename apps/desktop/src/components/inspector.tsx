/* Agent panel (Warp/Cursor-direction): conversation on top, composer pinned
   to the bottom ("Ask a question…"). Opens only on explicit user action,
   never takes focus itself, supports pinning (§11-C). */
import { useState } from "react";
import { FIXTURE } from "../fixture";
import { DecisionCard } from "./diagnostics";

const F = FIXTURE;

export function Inspector({ onClose }: { onClose: (e: React.MouseEvent<HTMLButtonElement>) => void }) {
  const [draft, setDraft] = useState("");
  return (
    <aside
      id="inspector"
      aria-label="Agent"
      /* Responsive per §11-C: at wide windows (≥xl) the panel is a regular
         flex column; below xl it becomes a separate detail view overlaying
         the workspace instead of squeezing it. */
      className="absolute inset-y-3 right-3 z-20 flex w-[380px] max-w-[calc(100%-1.5rem)] shrink-0 flex-col overflow-hidden rounded-[10px] border border-edge bg-shell shadow-[var(--shadow-pop)] xl:static xl:inset-auto xl:z-auto xl:shadow-[var(--shadow-card)]"
    >
      <div className="flex items-center gap-2 border-b border-edge px-4 py-3">
        <strong className="text-sm">Runlith Agent</strong>
        <span className="min-w-0 flex-1" />
        <button
          type="button"
          title="History"
          className="flex size-7 items-center justify-center rounded-[6px] text-ink2 hover:bg-surface hover:text-ink"
        >
          <svg aria-hidden width="14" height="14" viewBox="0 0 14 14">
            <circle cx="7" cy="7" r="5.4" fill="none" stroke="currentColor" strokeWidth="1.3" />
            <path d="M7 4.2V7l1.8 1.4" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
          </svg>
        </button>
        <button
          type="button"
          onClick={onClose}
          title="Close"
          className="flex size-7 items-center justify-center rounded-[6px] text-ink2 hover:bg-surface hover:text-ink"
        >
          <svg aria-hidden width="12" height="12" viewBox="0 0 12 12">
            <path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
          </svg>
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        {/* User message — soft gray bubble, right-aligned like the reference */}
        <div className="mb-4 flex justify-end">
          <p className="max-w-[85%] rounded-[10px] rounded-br-[4px] bg-surface px-3.5 py-2.5 text-sm leading-5">
            Retry stability is the goal; do not touch planner code.
          </p>
        </div>

        {/* Agent narration — plain text, agent-reported claims are marked */}
        <div className="space-y-3 text-sm leading-5">
          <p>
            Received the auth retry scope. Analyzing the failing path across concurrent executions.
          </p>
          <p>
            Read <span className="font-mono text-xs text-ink2">adapters/provider-claude/src/retry.rs</span>
            <br />
            Verified 2 acceptance criteria against snapshot{" "}
            <span className="font-mono text-xs">5e11d0…</span>
          </p>
          <p>
            Got it — retries had no jitter, so concurrent refresh double-authenticated. Super simple: added jitter and a
            single in-flight guard. C1 and C2 are VERIFIED; the log-scan check (C3) is still pending.
          </p>
          <p className="text-ink2">Working…</p>
          <p className="text-xs leading-4 text-ink2">
            Task-level conversation — distinct from the provider transcript (continuity across attempts/providers is an
            unresolved product question, §13.3). Agent-reported claims are not completion evidence until verified
            (C1–C3).
          </p>
        </div>

        <div className="mt-4">
          {F.attention.map((a) => (
            <DecisionCard key={a.id} item={a} />
          ))}
        </div>
      </div>

      {/* Composer (Warp-style): rounded field, attach + send. Fixture only —
          sending is a no-op until an interaction contract exists. */}
      <form
        className="border-t border-edge p-3"
        onSubmit={(e) => e.preventDefault()}
        aria-label="Ask a question"
      >
        <div className="rounded-[10px] border border-edge focus-within:border-accent/50">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Ask a question…"
            rows={2}
            className="w-full resize-none bg-transparent px-3 pt-2.5 text-sm outline-none placeholder:text-ink2"
          />
          <div className="flex items-center gap-1 px-2 pb-2">
            <button
              type="button"
              title="Attach"
              className="flex size-7 items-center justify-center rounded-[6px] text-ink2 hover:bg-surface hover:text-ink"
            >
              <svg aria-hidden width="14" height="14" viewBox="0 0 14 14">
                <path
                  d="M9.5 4.5 5.2 8.8a1.6 1.6 0 0 0 2.3 2.3l4.3-4.4a3.1 3.1 0 0 0-4.4-4.4L3 6.7a4.7 4.7 0 0 0 6.6 6.6"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.3"
                  strokeLinecap="round"
                />
              </svg>
            </button>
            <span className="min-w-0 flex-1" />
            <button
              type="submit"
              title="Send"
              disabled={draft.trim() === ""}
              className="flex size-7 items-center justify-center rounded-[6px] text-ink2 hover:bg-surface hover:text-ink disabled:opacity-40"
            >
              <svg aria-hidden width="14" height="14" viewBox="0 0 14 14">
                <path d="M2 7 12 2 9.5 12 7 8 2 7Z" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
              </svg>
            </button>
          </div>
        </div>
      </form>
    </aside>
  );
}