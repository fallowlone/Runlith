/* Fixture data — identical source of truth for the whole UI. Ported from
   docs/ui/design/2026-09-21-d05-task-workspace-prototypes/fixture.js.
   Fictional design fixture per research §12.3; values are demonstrative
   only, no backend read contract is implied (ADR-0016: real query
   surfaces come before complex UI; none are invented here). */

export type TaskLifecycle =
  | "DRAFT"
  | "READY"
  | "CLAIMED"
  | "RUNNING"
  | "BLOCKED"
  | "SUBMITTED"
  | "AWAITING_REVIEW"
  | "VERIFYING"
  | "RETRY_READY"
  | "COMPLETED"
  | "FAILED"
  | "CANCELLED";

export interface TaskListItem {
  id: string;
  objective: string;
  state: TaskLifecycle;
  reason: string;
  parent?: string;
  selected?: boolean;
}

export interface DecisionChoice {
  id: "RESUME" | "REPLAN" | "ABORT";
  label: string;
  why: string;
}

export interface HumanDecisionRequest {
  type: "HumanDecisionRequest";
  id: string;
  task: string;
  reason: string;
  why: string;
  choices: DecisionChoice[];
  recommendation: string;
  validity: string;
  status: "PENDING";
}

export interface ExpiredApproval {
  type: "Approval";
  id: string;
  task: string;
  reason: string;
  why: string;
  operation: string;
  validity: string;
  status: "READ_ONLY";
  note: string;
}

export type AttentionItem = HumanDecisionRequest | ExpiredApproval;

export interface Criterion {
  id: string;
  text: string;
  verdict: "VERIFIED" | "PENDING";
  snapshot: string | null;
  producer: string | null;
  when: string | null;
  links: string[];
  note?: string;
}

export interface Attempt {
  n: number;
  outcome: "SUCCEEDED" | "FAILED" | "TIMED_OUT" | "CANCELLED";
  ended: string;
  note: string;
}

export interface DiffHunk {
  t: "ctx" | "add" | "del";
  l: string;
}

export interface DiffFile {
  path: string;
  hunks: DiffHunk[];
}

export interface HistoryEvent {
  seq: string;
  at: string;
  text: string;
}

export interface LogLine {
  src: string;
  line: string;
}

export const FIXTURE = {
  project: { name: "Runlith", repo: "fallowlone/Runlith" },
  connection: {
    state: "connected",
    protocol: "agentd-control v6",
    observed: "fresh 2s ago",
    note: "ping proves control-channel responsiveness, not provider progress",
  },
  tasks: [
    {
      id: "TASK-0342",
      objective: "Make provider auth retries stable across concurrent executions",
      state: "VERIFYING",
      reason: "Verifying 3 acceptance criteria",
      selected: true,
    },
    {
      id: "TASK-0344",
      objective: "Harden token refresh path",
      state: "BLOCKED",
      reason: "Pending decision HDR-0092",
      parent: "TASK-0342",
    },
    {
      id: "TASK-0351",
      objective: "Worktree reconcile protocol",
      state: "READY",
      reason: "Accepted plan r2; READY ≠ RUNNABLE",
    },
    {
      id: "TASK-0355",
      objective: "Planner budget guard",
      state: "RUNNING",
      reason: "Live 4s ago",
    },
    {
      id: "TASK-0338",
      objective: "Rotate keychain entries",
      state: "COMPLETED",
      reason: "Evidence archived",
    },
  ] as TaskListItem[],
  attention: [
    {
      type: "HumanDecisionRequest",
      id: "HDR-0092",
      task: "TASK-0344",
      reason: "NO_PROGRESS",
      why: "Attempt 2 made no progress for 25m; course of work stopped.",
      choices: [
        { id: "RESUME", label: "RESUME", why: "Continue with the reviewed proposal, identity-bound to its hash." },
        { id: "REPLAN", label: "REPLAN", why: "Start a new planning run from current input snapshot." },
        { id: "ABORT", label: "ABORT", why: "Stop TASK-0344; no automatic retry afterwards." },
      ],
      recommendation: "RESUME (planner recommendation — not pre-selected consent)",
      validity: "expires in 22h",
      status: "PENDING",
    },
    {
      type: "Approval",
      id: "APR-0041",
      task: "TASK-0338",
      reason: "REQUEST_EXPIRED",
      why: "Request expired while under review. Readable; mutation unavailable.",
      operation: "keychain.delete entry runlith/provider-token",
      validity: "expired 2h ago",
      status: "READ_ONLY",
      note: "Open the current request instead. Denial would not abort TASK-0338 (already COMPLETED).",
    },
  ] as AttentionItem[],
  task: {
    id: "TASK-0342",
    objective: "Make provider auth retries stable across concurrent executions",
    lifecycle: "VERIFYING" as TaskLifecycle,
    headlineReason: "Verifying 3 acceptance criteria — 2 VERIFIED, 1 pending",
    nextCommand: "Next allowed: view evidence",
    scope: ["adapters/provider-claude", "crates/task"],
    execution: {
      live: false,
      observation:
        "No live execution. Last confirmed observation 12m ago. No live run ≠ safe to repeat; recovery requires a supported backend capability.",
      allowedOps: ["View diagnostics"],
    },
    attempts: [
      { n: 1, outcome: "TIMED_OUT", ended: "45m ago", note: "Provider stream stalled; concluded outcome is immutable." },
      { n: 2, outcome: "SUCCEEDED", ended: "12m ago", note: "A succeeded Attempt is not a COMPLETED Task." },
    ] as Attempt[],
    plan: {
      acceptedRevision: "r4",
      head: "9f2c1ab",
      provenance: "PlanningRun #77 SUCCEEDED · input snapshot 2f8a11 · source evidence EV-118",
      proposed: {
        id: "CS-12",
        delta: "+2 steps, −1 step vs r4",
        summary: "Add retry-jitter step; drop redundant pre-flight check.",
        risk: "LOW (criteria unchanged)",
        validation: "validation pending — not an acting plan",
      },
    },
    criteria: [
      {
        id: "C1",
        text: "Expired tokens trigger a single retry",
        verdict: "VERIFIED",
        snapshot: "5e11d0…",
        producer: "Attempt 2",
        when: "10m ago",
        links: ["report", "log fragment"],
      },
      {
        id: "C2",
        text: "Concurrent refresh never double-authenticates",
        verdict: "VERIFIED",
        snapshot: "5e11d0…",
        producer: "Attempt 2",
        when: "9m ago",
        links: ["report", "diff hunk"],
      },
      {
        id: "C3",
        text: "No plaintext tokens in logs",
        verdict: "PENDING",
        snapshot: null,
        producer: null,
        when: null,
        links: [],
        note: "Evidence not yet present — neither failed nor verified.",
      },
    ] as Criterion[],
    staleEvidence:
      "Earlier C1 verdict was bound to snapshot 4d22… which predates the current snapshot. The earlier verdict is preserved and not rewritten; it is shown as not matching the current snapshot.",
    changes: {
      baseRevision: "1a4f9c2",
      files: [
        {
          path: "adapters/provider-claude/src/retry.rs",
          hunks: [
            { t: "ctx", l: "  fn retry_delay(attempt: u32) -> Duration {" },
            { t: "del", l: "-    Duration::from_secs(2)" },
            { t: "add", l: "+    let base = Duration::from_secs(2);" },
            { t: "add", l: "+    base + Duration::from_millis(jitter(attempt))" },
            { t: "ctx", l: "  }" },
          ],
        },
        {
          path: "crates/task/src/auth.rs",
          hunks: [
            { t: "ctx", l: "  pub fn refresh(state: &mut AuthState) {" },
            { t: "add", l: '+    debug_assert!(!state.refreshing, "single in-flight refresh");' },
            { t: "ctx", l: "      state.refreshing = true;" },
          ],
        },
      ] as DiffFile[],
    },
    history: [
      { seq: "0187", at: "12m ago", text: "Attempt 2 SUCCEEDED (execution E-0901)" },
      { seq: "0186", at: "10m ago", text: "Verification started for 3 acceptance criteria" },
      { seq: "0185", at: "10m ago", text: "Evidence C1 VERIFIED against snapshot 5e11d0…" },
      { seq: "0184", at: "9m ago", text: "Evidence C2 VERIFIED against snapshot 5e11d0…" },
      { seq: "0181", at: "52m ago", text: "Plan r4 accepted (PlanningRun #77, replaces r3)" },
      { seq: "0178", at: "1h ago", text: "Attempt 1 TIMED_OUT (execution E-0899)" },
    ] as HistoryEvent[],
    childBlocker: {
      id: "TASK-0344",
      text: "Blocked by pending decision HDR-0092 (NO_PROGRESS). Scope is not widened automatically; resolving is a separate action.",
    },
    logs: [
      { src: "attempt2", line: "12:04:18 INFO  provider stream closed after final chunk" },
      { src: "attempt2", line: "12:04:18 INFO  artifacts: 2 files, 1 report uploaded" },
      { src: "verifier", line: "12:04:22 INFO  criterion C1 -> VERIFIED (snapshot 5e11d0…)" },
      { src: "verifier", line: "12:04:29 INFO  criterion C3 -> PENDING (awaiting log scan)" },
    ] as LogLine[],
  },
};

export type PanelId = "overview" | "plan" | "changes" | "evidence" | "history";