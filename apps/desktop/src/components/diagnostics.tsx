/* History, diagnostics content and decision/approval cards — identical
   content wherever the layout mounts them (research §12–§13). */
import { useState } from "react";
import { FIXTURE, type AttentionItem, type ExpiredApproval, type HumanDecisionRequest } from "../fixture";
import { Badge, Button } from "./ui";

const t = FIXTURE.task;

/* ---------- History ---------- */

export function HistoryPanel() {
  return (
    <ul className="list-none">
      {t.history.map((h) => (
        <li key={h.seq} className="flex gap-3 border-b border-edge/50 py-2">
          <span className="w-10 shrink-0 font-mono text-xs text-ink2">{h.seq}</span>
          <span className="min-w-0 flex-1">{h.text}</span>
          <span className="shrink-0 text-xs text-ink2">{h.at}</span>
        </li>
      ))}
    </ul>
  );
}

/* ---------- Attempts (diagnostics scope, §12.6/§10) ---------- */

export function AttemptsList() {
  return (
    <div>
      {t.attempts.map((a) => (
        <div key={a.n} className="row-h flex items-center gap-3 px-3">
          <strong className="text-sm">Attempt {a.n}</strong>
          <Badge state={a.outcome === "SUCCEEDED" ? "COMPLETED" : "FAILED"}>{a.outcome}</Badge>
          <span className="min-w-0 flex-1 truncate text-xs text-ink2">{a.note}</span>
          <span className="shrink-0 text-xs text-ink2">{a.ended}</span>
        </div>
      ))}
      <p className="px-3 py-1 text-xs leading-4 text-ink2">
        An error in one Attempt does not recolor the whole Task (§10); Task lifecycle stays {t.lifecycle}.
      </p>
    </div>
  );
}

/* ---------- Logs (read-only, §14.5) ---------- */

export function LogsView() {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 px-2 py-1.5">
        <span className="text-xs text-ink2">Follow: on</span>
        <Button size="sm">Stop follow</Button>
        <Button size="sm">Jump to newest</Button>
        <span className="text-xs text-ink2">Filter: all sources · Attempt 2</span>
      </div>
      <div role="log" aria-label="Attempt 2 logs" tabIndex={0} className="min-h-0 flex-1 overflow-auto rounded-[4px] bg-surface px-3 py-2 font-mono text-xs leading-5">
        {t.logs.map((l, i) => (
          <div key={i} className="whitespace-pre">
            <span className="font-medium text-teal">[{l.src}]</span> {l.line}
          </div>
        ))}
      </div>
      <p className="px-2 py-1 text-xs leading-4 text-ink2">
        Read-only command output — visibly distinct from a writable terminal, which would require its own IPC/authority
        contract (§14.5).
      </p>
    </div>
  );
}

/* ---------- Decision / Approval cards (§13.4, §13.5) ---------- */

export function DecisionCard({ item, focusOnOpen }: { item: AttentionItem; focusOnOpen?: boolean }) {
  const [choice, setChoice] = useState<string | null>(null);

  if (item.status === "PENDING") {
    const hdr = item as HumanDecisionRequest;
    return (
      <section className="mb-3 overflow-hidden rounded-[6px] border border-edge">
        <h3 className="bg-surface px-3 py-2.5 text-xs font-semibold uppercase tracking-wider text-ink2">
          {hdr.type} {hdr.id} · {hdr.task} — <span className="font-normal normal-case">{hdr.validity}</span>
        </h3>
        <div className="p-3">
          <p className="text-sm">{hdr.why}</p>
          <p className="mt-2 text-xs leading-4 text-ink2">
            Reason: {hdr.reason} — further work stopped. Recommendation shown separately from the choice:{" "}
            {hdr.recommendation}.
          </p>
          <div role="radiogroup" aria-label={`Decision choices for ${hdr.id}`} className="mt-2">
            {hdr.choices.map((c) => (
              <label
                key={c.id}
                className="mt-2 flex cursor-pointer items-start gap-2.5 rounded-[4px] border border-white/15 p-2.5 hover:border-white/30"
              >
                <input
                  type="radio"
                  name={`hdr-${hdr.id}`}
                  value={c.id}
                  checked={choice === c.id}
                  onChange={() => setChoice(c.id)}
                  className="mt-1"
                />
                <span>
                  <strong>{c.label}</strong>
                  <span className="block text-xs leading-4 text-ink2">{c.why}</span>
                </span>
              </label>
            ))}
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <Button variant="primary" disabled={choice === null}>
              Record decision{choice ? `: ${choice}` : ""}
            </Button>
            <span className="text-xs leading-4 text-ink2">
              A recorded decision is shown as “Recorded”, then “Applied” once confirmed. No timer applies a choice.
            </span>
          </div>
          {focusOnOpen ? null : null}
        </div>
      </section>
    );
  }

  const apr = item as ExpiredApproval;
  return (
    <section className="mb-3 overflow-hidden rounded-[6px] border border-edge">
      <h3 className="bg-surface px-3 py-2.5 text-xs font-semibold uppercase tracking-wider text-ink2">
        {apr.type} {apr.id} · {apr.task} — <span className="font-normal normal-case">{apr.validity}</span>
      </h3>
      <div className="p-3">
        <p className="font-mono text-xs">{apr.operation}</p>
        <p className="mt-1 text-sm">{apr.why}</p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button disabled title="Request expired — read-only">
            Allow once
          </Button>
          <Button disabled title="Request expired — read-only">
            Deny operation
          </Button>
          <Button>Open current request</Button>
          <span className="text-xs leading-4 text-ink2">{apr.note}</span>
        </div>
      </div>
    </section>
  );
}