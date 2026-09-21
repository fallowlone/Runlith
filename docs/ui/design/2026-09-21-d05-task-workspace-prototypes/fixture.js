/* D05 fixture — identical data for Task Workspace variants A, B and C.
   Fictional design fixture per research §12.3 (worked example): Task
   VERIFYING, Attempt 1 TIMED_OUT, Attempt 2 SUCCEEDED. Values are
   demonstrative only; no backend read contract is implied. */
window.FIXTURE = {
  project: { name: "Runlith", repo: "fallowlone/Runlith" },
  connection: {
    state: "connected",
    protocol: "agentd-control v6",
    observed: "fresh 2s ago",
    note: "ping proves control-channel responsiveness, not provider progress"
  },
  tasks: [
    { id: "TASK-0342", objective: "Make provider auth retries stable across concurrent executions", state: "VERIFYING", reason: "Verifying 3 acceptance criteria", selected: true },
    { id: "TASK-0344", objective: "Harden token refresh path", state: "BLOCKED", reason: "Pending decision HDR-0092", parent: "TASK-0342" },
    { id: "TASK-0351", objective: "Worktree reconcile protocol", state: "READY", reason: "Accepted plan r2; READY ≠ RUNNABLE" },
    { id: "TASK-0355", objective: "Planner budget guard", state: "RUNNING", reason: "Live 4s ago" },
    { id: "TASK-0338", objective: "Rotate keychain entries", state: "COMPLETED", reason: "Evidence archived" }
  ],
  attention: [
    {
      type: "HumanDecisionRequest", id: "HDR-0092", task: "TASK-0344",
      level: "immediate-action",
      reason: "NO_PROGRESS",
      why: "Attempt 2 made no progress for 25m; course of work stopped.",
      choices: [
        { id: "RESUME", label: "RESUME", why: "Continue with the reviewed proposal, identity-bound to its hash." },
        { id: "REPLAN", label: "REPLAN", why: "Start a new planning run from current input snapshot." },
        { id: "ABORT", label: "ABORT", why: "Stop TASK-0344; no automatic retry afterwards." }
      ],
      recommendation: "RESUME (planner recommendation — not pre-selected consent)",
      validity: "expires in 22h", status: "PENDING"
    },
    {
      type: "Approval", id: "APR-0041", task: "TASK-0338",
      level: "diagnostic-exception",
      reason: "REQUEST_EXPIRED",
      why: "Request expired while under review. Readable; mutation unavailable.",
      operation: "keychain.delete entry runlith/provider-token",
      validity: "expired 2h ago", status: "READ_ONLY",
      note: "Open the current request instead. Denial would not abort TASK-0338 (already COMPLETED)."
    }
  ],
  task: {
    id: "TASK-0342",
    objective: "Make provider auth retries stable across concurrent executions",
    lifecycle: "VERIFYING",
    headlineReason: "Verifying 3 acceptance criteria — 2 VERIFIED, 1 pending",
    nextCommand: "Next allowed: view evidence",
    scope: ["adapters/provider-claude", "crates/task"],
    execution: {
      live: false,
      observation: "No live execution. Last confirmed observation 12m ago. No live run ≠ safe to repeat; recovery requires a supported backend capability.",
      allowedOps: ["View diagnostics"]
    },
    attempts: [
      { n: 1, outcome: "TIMED_OUT", ended: "45m ago", note: "Provider stream stalled; concluded outcome is immutable." },
      { n: 2, outcome: "SUCCEEDED", ended: "12m ago", note: "A succeeded Attempt is not a COMPLETED Task." }
    ],
    plan: {
      acceptedRevision: "r4", head: "9f2c1ab",
      provenance: "PlanningRun #77 SUCCEEDED · input snapshot 2f8a11 · source evidence EV-118",
      proposed: {
        id: "CS-12", delta: "+2 steps, −1 step vs r4",
        summary: "Add retry-jitter step; drop redundant pre-flight check.",
        risk: "LOW (criteria unchanged)", validation: "validation pending — not an acting plan"
      }
    },
    criteria: [
      {
        id: "C1", text: "Expired tokens trigger a single retry",
        verdict: "VERIFIED", snapshot: "5e11d0…", producer: "Attempt 2", when: "10m ago",
        links: ["report", "log fragment"]
      },
      {
        id: "C2", text: "Concurrent refresh never double-authenticates",
        verdict: "VERIFIED", snapshot: "5e11d0…", producer: "Attempt 2", when: "9m ago",
        links: ["report", "diff hunk"]
      },
      {
        id: "C3", text: "No plaintext tokens in logs",
        verdict: "PENDING", snapshot: null, producer: null, when: null,
        links: [], note: "Evidence not yet present — neither failed nor verified."
      }
    ],
    staleEvidence: "Earlier C1 verdict was bound to snapshot 4d22… which predates the current snapshot. The earlier verdict is preserved and not rewritten; it is shown as not matching the current snapshot.",
    changes: {
      baseRevision: "1a4f9c2",
      files: [
        { path: "adapters/provider-claude/src/retry.rs", hunks: [
          { t: "ctx", l: "  fn retry_delay(attempt: u32) -> Duration {" },
          { t: "del", l: "-    Duration::from_secs(2)" },
          { t: "add", l: "+    let base = Duration::from_secs(2);" },
          { t: "add", l: "+    base + Duration::from_millis(jitter(attempt))" },
          { t: "ctx", l: "  }" }
        ] },
        { path: "crates/task/src/auth.rs", hunks: [
          { t: "ctx", l: "  pub fn refresh(state: &mut AuthState) {" },
          { t: "add", l: "+    debug_assert!(!state.refreshing, \"single in-flight refresh\");" },
          { t: "ctx", l: "      state.refreshing = true;" }
        ] }
      ]
    },
    history: [
      { seq: "0187", at: "12m ago", text: "Attempt 2 SUCCEEDED (execution E-0901)" },
      { seq: "0186", at: "10m ago", text: "Verification started for 3 acceptance criteria" },
      { seq: "0185", at: "10m ago", text: "Evidence C1 VERIFIED against snapshot 5e11d0…" },
      { seq: "0184", at: "9m ago",  text: "Evidence C2 VERIFIED against snapshot 5e11d0…" },
      { seq: "0181", at: "52m ago", text: "Plan r4 accepted (PlanningRun #77, replaces r3)" },
      { seq: "0178", at: "1h ago",  text: "Attempt 1 TIMED_OUT (execution E-0899)" }
    ],
    childBlocker: {
      id: "TASK-0344", text: "Blocked by pending decision HDR-0092 (NO_PROGRESS). Scope is not widened automatically; resolving is a separate action."
    },
    logs: [
      { src: "attempt2", line: "12:04:18 INFO  provider stream closed after final chunk" },
      { src: "attempt2", line: "12:04:18 INFO  artifacts: 2 files, 1 report uploaded" },
      { src: "verifier", line: "12:04:22 INFO  criterion C1 -> VERIFIED (snapshot 5e11d0…)" },
      { src: "verifier", line: "12:04:29 INFO  criterion C3 -> PENDING (awaiting log scan)" }
    ]
  }
};