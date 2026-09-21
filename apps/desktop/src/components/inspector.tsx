/* Inspector (Variant C): conversation + decision panel. Opens only on
   explicit user action, never takes focus itself, supports pinning (§11-C). */
import { useState } from "react";
import { FIXTURE } from "../fixture";
import { Button } from "./ui";
import { DecisionCard } from "./diagnostics";

const F = FIXTURE;

export function Inspector({ onClose }: { onClose: (e: React.MouseEvent<HTMLButtonElement>) => void }) {
  const [choice] = useState<string | null>(null);
  void choice; // choice state lives inside DecisionCard; kept for future composer state
  return (
    <aside id="inspector" aria-label="Inspector" className="flex w-[360px] shrink-0 flex-col rounded-[6px] bg-surface">
      <div className="flex items-center gap-2 rounded-t-[6px] px-3 py-2.5">
        <strong className="text-sm">Inspector</strong>
        <span className="min-w-0 flex-1 text-xs text-ink2">Conversation · Decision</span>
        <Button size="sm" onClick={onClose}>
          Close
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        <section aria-label="Conversation">
          <p className="mb-2 text-xs leading-4 text-ink2">
            Task-level conversation — distinct from the provider transcript (continuity across attempts/providers is an
            unresolved product question, §13.3).
          </p>
          <p className="text-sm">
            <strong>You:</strong> Retry stability is the goal; do not touch planner code.
          </p>
          <p className="text-sm">
            <strong>Agent (reported claim):</strong> “Retries now use jitter; concurrent refresh is guarded.”{" "}
            <span className="text-xs text-ink2">Agent-reported claim — not completion evidence until verified (C1–C3).</span>
          </p>
          <div className="mt-2 flex gap-2">
            <Button size="sm">Attach to attempt 2</Button>
            <Button size="sm">Open provider transcript</Button>
          </div>
        </section>
        <div className="mt-3">
          {F.attention.map((a) => (
            <DecisionCard key={a.id} item={a} />
          ))}
        </div>
      </div>
    </aside>
  );
}