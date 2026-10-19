<div align="center">

<img src="docs/assets/banner.svg" alt="Runlith — durable AI engineering, on your machine" width="100%">

<br>

**A local-first control plane for durable, verified, AI-assisted software engineering.**

Claude Code, Codex, OpenCode, Gemini — they are *replaceable executors*. The core is provider-neutral;
the Claude adapter is the only real provider adapter today.<br>
Runlith is the layer above them that remembers, plans, isolates, verifies, and recovers.

<br>

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-5b8cff?style=for-the-badge)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95-f74c00?style=for-the-badge&logo=rust&logoColor=white)](Cargo.toml)
[![Status](https://img.shields.io/badge/status-pre--release-7cf5c4?style=for-the-badge&labelColor=0b0f1a)](#-status--roadmap)
[![Local first](https://img.shields.io/badge/local--first-no%20cloud%20%7C%20no%20telemetry-0b0f1a?style=for-the-badge)](#-principles)

[Why](#-why-runlith) ·
[Quick start](#-quick-start) ·
[How it works](#-how-it-works) ·
[Mechanics](#-mechanics-in-depth) ·
[Roadmap](#-status--roadmap) ·
[Contributing](#-contributing)

</div>

<br>

> [!WARNING]
> Runlith is **pre-release** (`0.1.0`, currently gate **G13**). There is no public release yet, interfaces
> may change, and only `main` is supported. Great time to read the code and shape the direction — not yet
> a time to depend on it in production.

---

## ✦ Why Runlith

AI coding agents are powerful and **forgetful**. A session ends, a context window fills up, a provider hits its
quota, a process crashes — and the work evaporates into a chat transcript.

Runlith treats that as an infrastructure problem, not a prompting problem.

```text
 Without Runlith                          With Runlith
 ───────────────                          ────────────
 chat session = project state             Task graph = project state (SQLite + append-only journal)
 agent says "done"  → you hope            evidence says "verified" → you know
 context full       → start over          checkpoints + epochs → continue, even on another provider
 prompt says "be careful"                 policy + OS isolation enforce it
 one provider, one session                provider-neutral core, swappable executors
```

> **What can Runlith do more reliably, continuously, or efficiently than using coding-agent products separately?**
> That question gates every feature. If a capability can be reproduced by opening another agent terminal, it
> does not belong here. See the [Project Charter](PROJECT_CHARTER.md).

## ✦ Highlights

<table>
<tr>
<td width="50%" valign="top">

### 🗂 Durable tasks
Tasks persist across restarts, crashes, model switches, and handoffs. `Task ≠ Execution ≠ Attempt ≠ Session`
— the Task is the durable entity, everything else is replaceable.

</td>
<td width="50%" valign="top">

### ✅ Verified delivery
An agent never declares *done*. Only the independent evaluation pipeline can grant completion, from tests,
review, and acceptance evidence — never from model self-report.

</td>
</tr>
<tr>
<td valign="top">

### 🛡 Agent Firewall
Policy returns `ALLOW` / `DENY` / `REQUIRE_APPROVAL` *before* execution. Runners enforce filesystem, network,
and process limits (macOS Seatbelt today) and **fail closed** when enforcement is missing.

</td>
<td valign="top">

### 🔑 Secrets that never leak
Credentials live only in the OS keychain or an encrypted vault. Plaintext never enters model context, logs,
events, or SQLite.

</td>
</tr>
<tr>
<td valign="top">

### 🌿 Isolated workspaces
Every execution gets its own Git worktree and branch. Agents never touch your working copy, and diffs are
explicit artifacts.

</td>
<td valign="top">

### ♾ Context continuity
Provider-independent epochs and checkpoints (*no checkpoint → no rollover*) let a task outlive any single
context window — or any single provider.

</td>
</tr>
<tr>
<td valign="top">

### 🧭 Bounded adaptive planning
Planners *propose*, validated services *decide*. Nine validation rules and hard numeric guardrails stop
runaway decomposition; risky changes need a human.

</td>
<td valign="top">

### 🧑‍💻 Human in the loop
Approvals, replan/abort decisions, and full **takeover** are first-class durable records — not chat
messages you hope someone sees.

</td>
</tr>
</table>

## ✦ Quick start

**Requirements:** Rust `1.95` (the version CI pins), Git, macOS for the sandboxed runner.

```sh
git clone https://github.com/fallowlone/Runlith.git
cd Runlith

cargo build --locked --bin agentd --bin agentctl
```

Start the local daemon and talk to it through `agentctl`:

```sh
agentctl=./target/debug/agentctl

$agentctl start                       # spawn the sibling agentd daemon
$agentctl ping                        # handshake + version check

$agentctl project create demo "Demo project"
$agentctl task create t1 demo "Add input validation to the signup form"

$agentctl execution run t1 /path/to/your/repo
$agentctl execution status <execution-id>
```

<details>
<summary><b>More commands</b></summary>

```text
agentctl project   create | get | list
agentctl task      create | get | list | dependency add|list
                   ready | claim | start | resume | block | submit | review
                   verify-begin | verify-fail | verify-succeed
agentctl execution run | status | cancel | resume
agentctl approval  get | decide <approve|deny>
agentctl human-decision list | get | decide <resume|replan|abort|takeover>
agentctl planning  trigger | apply
agentctl find      query | show        # evidence search over a repository
agentctl vault     …                   # secret references and bindings
```

There is intentionally **no** `set-state` and no `done` shortcut: lifecycle commands stay semantic, and
completion is earned through verification.

</details>

<details>
<summary><b>macOS: linking fails with <code>cc</code> exit 69?</b></summary>

Point the toolchain at the Command Line Tools:

```sh
export DEVELOPER_DIR=/Library/Developer/CommandLineTools
```

</details>

<details>
<summary><b>Using a real provider</b></summary>

The Claude adapter drives the authenticated `claude` CLI. Live smoke tests are `#[ignore]`d and need
`RUNLITH_CLAUDE_SMOKE=1`. Deterministic fixture tests do not prove live-provider support. See
[`docs/architecture/providers.md`](docs/architecture/providers.md).

</details>

---

## ✦ How it works

The big picture: **the agent is one box in the middle.** Everything around it — state, scheduling, authority,
verification — is owned by Runlith and survives the agent.

```mermaid
flowchart LR
    I([Intent]) --> P[Plan]
    P --> T[(Durable<br/>Task Graph)]
    T --> S{{Scheduler /<br/>Reconciler}}
    S --> POL[Policy<br/>ALLOW · DENY · APPROVE]
    POL --> R[Isolated Runner<br/>worktree + sandbox]
    R --> A[[Agent<br/>Claude today · other providers by design]]
    A -- submits result --> E[Evaluation]
    E -- VERIFIED --> H{Human gate}
    E -- REJECTED --> S
    H --> SHIP([Ship])

    classDef core fill:#151b2e,stroke:#5b8cff,color:#e8ecf8;
    classDef edge fill:#0b0f1a,stroke:#7cf5c4,color:#e8ecf8;
    class T,S,POL,E core;
    class A,R edge;
```

### System map

Runlith is a **local Rust modular monolith** with **Ports & Adapters**. Domains own behaviour, adapters own I/O,
and concrete adapters are assembled *only* in the composition root (`agentd`) — no domain crate instantiates
infrastructure. A port belongs to its **consumer**, never to the infrastructure that implements it.

```mermaid
flowchart TB
    subgraph CLIENTS["Clients  (privilege boundary TB2)"]
        CLI["agentctl"]
        UI["Desktop UI · planned"]
    end

    CLI & UI -- "typed local IPC<br/>UDS · framed Protobuf · versioned handshake" --> D

    subgraph D["agentd — composition root"]
        direction TB
        APP["app-services<br/>TaskService · OrchestrationService · Planning"]
        subgraph DOM["Domains (own ports, no I/O)"]
            direction LR
            TASK[task]:::dom
            PLAN[plan]:::dom
            SCH[scheduler]:::dom
            POLI[policy]:::dom
            EVAL[evaluation]:::dom
            WS[workspace]:::dom
            CTX[context-engine]:::dom
            CONT[continuity]:::dom
            TK[takeover]:::dom
            RUN[runner]:::dom
            SEC[secrets]:::dom
        end
        APP --> DOM
    end

    D -- ports --> AD

    subgraph AD["Adapters (replaceable, one crate each)"]
        direction LR
        SQL[("storage-sqlite")]:::ad
        GIT["git-cli"]:::ad
        CLA["provider-claude"]:::ad
        SB["runner-macos-seatbelt"]:::ad
        KC["keychain-native · secrets-vault"]:::ad
        FIN["finder-local"]:::ad
        CAP["effect-capture-local"]:::ad
    end

    classDef dom fill:#151b2e,stroke:#5b8cff,color:#e8ecf8;
    classDef ad fill:#0b0f1a,stroke:#7cf5c4,color:#e8ecf8;
```

| Layer | Packages | Role |
|---|---|---|
| **Binaries** | `agentd`, `agentctl` | Daemon composition root and the CLI client |
| **Domains & services** | `task`, `plan`, `scheduler`, `policy`, `evaluation`, `workspace`, `app-services`, … | Business rules; own ports, never infrastructure |
| **Contracts** | `ipc-contracts`, `execution-contract`, `planner-contract`, `secret-backend-contract`, … | Reusable contract suites any adapter must pass |
| **Adapters** | `storage-sqlite`, `git-cli`, `provider-claude`, `runner-macos-seatbelt`, `keychain-native`, `secrets-vault`, … | Concrete I/O, one replaceable crate each |

**48 packages** in one Cargo workspace (34 shared crates, 12 adapters, 2 binaries). A second provider must slot in
*without core changes* — that is the test of the design.

### One run, end to end

What actually happens when you type `agentctl execution run`:

```mermaid
sequenceDiagram
    autonumber
    actor U as You
    participant C as agentctl
    participant D as agentd
    participant T as TaskService
    participant W as Workspace + Git
    participant P as Policy
    participant R as Runner
    participant A as Agent (provider)
    participant E as Evaluation
    participant DB as SQLite + Journal

    U->>C: execution run t1 ./repo
    C->>D: handshake, then request (IPC)
    D->>T: claim, start (READY → CLAIMED → RUNNING)
    T->>DB: state + journal event, one transaction
    D->>W: allocate lease: worktree + managed branch @ base revision
    W->>DB: intent → git op → observe → Ready
    D->>P: PolicyRequest(capabilities, exact scopes)
    P-->>D: ALLOW / DENY / REQUIRE_APPROVAL
    D->>R: IsolationPlan from effective capabilities
    R-->>D: EnforcementReport (fails closed if any axis unproven)
    D->>A: execute inside sandboxed worktree (outside any DB txn)
    A-->>D: result + evidence refs (submit_result, never "complete")
    D->>T: SUBMITTED → AWAITING_REVIEW → VERIFYING
    D->>E: candidate diff + captured effects
    E-->>D: per-criterion verdicts
    alt every criterion VERIFIED
        D->>T: completion gate → COMPLETED
    else REJECTED
        D->>T: RETRY_READY → new Attempt
    end
    T->>DB: state + journal + admission record, atomically
    D-->>U: execution status + evidence
```

---

## ✦ Mechanics in depth

Each section below is a contract that the code enforces. Deep links point to the source documents.

- [Task lifecycle](#-task-lifecycle) · [Task ≠ Execution ≠ Attempt ≠ Session](#-the-four-concepts)
- [Policy & approvals](#-policy--approvals) · [Isolation (runners)](#-isolation-runners)
- [Workspaces & Git](#-workspaces--git) · [Verified completion](#-verified-completion)
- [Adaptive planning](#-adaptive-planning) · [Human decisions & takeover](#-human-decisions--takeover)
- [Context engine](#-context-engine) · [Continuity & rollover](#-continuity--rollover)
- [Persistence](#-persistence-three-ledgers) · [IPC](#-ipc-handshake) · [Secrets](#-secrets)

### 🔁 Task lifecycle

Twelve fixed states. State names are never renamed, added, or reordered; every transition has a defined authority.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> DRAFT
    DRAFT --> READY: authored
    READY --> CLAIMED: scheduler reserves
    CLAIMED --> RUNNING: execution begins
    RUNNING --> BLOCKED: explicit impediment
    BLOCKED --> RUNNING: cleared
    RUNNING --> SUBMITTED: agent submit_result()
    SUBMITTED --> AWAITING_REVIEW: routed
    AWAITING_REVIEW --> VERIFYING: review satisfied
    VERIFYING --> COMPLETED: VERIFIED + completion gate
    VERIFYING --> RETRY_READY: REJECTED
    RETRY_READY --> RUNNING: new / recovered attempt
    COMPLETED --> [*]

    note right of VERIFYING
        Only the Evaluation Engine
        can reach COMPLETED.
        Agents, providers, CLI, and
        sessions cannot.
    end note

    note left of DRAFT
        Side states: FAILED, CANCELLED.
        Their incoming transitions are
        deliberately unresolved in the
        architecture — never guessed.
    end note
```

| Rule | Meaning |
|---|---|
| **Submission is a claim** | The agent runtime API is `submit_result()` — there is no `complete_task()`. |
| **`READY` ≠ runnable** | *Scheduling readiness* (`RUNNABLE`, `WAITING_ON_DEPENDENCY`, …) is derived, never persisted, never merged into the lifecycle. |
| **Dependencies don't rewrite state** | An unfinished dependency yields `WAITING_ON_DEPENDENCY`; the Task row stays `READY`. |
| **Invalid transitions are refused** | Never silently coerced, never mutate durable state, always observable and testable. |
| **Attempt failure ≠ Task failure** | Provider A can fail; provider B may still finish the same Task. |

### 🧬 The four concepts

```mermaid
flowchart TB
    T["<b>Task</b> AUTH-42<br/>durable intent + acceptance criteria"]:::task
    T --> E1["Execution E1<br/>orchestration instance · owns 1 workspace lease"]:::exec
    T --> E2["Execution E2"]:::exec
    E1 --> A1["Attempt 1 · provider A · failed"]:::att
    E1 --> A2["Attempt 2 · provider A · rejected"]:::att
    E2 --> A3["Attempt 1 · provider B · passed"]:::att
    A3 --> S["Session<br/>ephemeral provider process / conversation"]:::sess

    classDef task fill:#151b2e,stroke:#5b8cff,stroke-width:2px,color:#e8ecf8;
    classDef exec fill:#151b2e,stroke:#7cf5c4,color:#e8ecf8;
    classDef att fill:#0b0f1a,stroke:#7cf5c4,color:#e8ecf8;
    classDef sess fill:#0b0f1a,stroke:#8c97b8,stroke-dasharray:4 3,color:#8c97b8;
```

Provider details travel only as opaque references. A field like `claude_session_id` on a Task is an explicit
domain-model violation.

### 🛡 Policy & approvals

Policy is the **canonical authorization authority**. The model, provider CLI, repository, plugin, and
provider-native permission prompts cannot grant authority. Prompts are not a security boundary.

```mermaid
flowchart LR
    REQ["Operation request<br/>typed capabilities + exact scope"] --> EV{{"Deterministic<br/>policy evaluation"}}
    EV -->|DENY| X(["Refused<br/>no approval path"]):::deny
    EV -->|REQUIRE_APPROVAL| AP["Durable approval request<br/>bound to exact request, scope,<br/>policy fingerprint, expiry"]
    EV -->|ALLOW| EF["Effective capabilities<br/>⊆ requested"]
    AP -->|human approves| RE["Operation owner re-presents<br/>the exact binding"]
    AP -->|human denies / expires| X
    RE -->|"recompute + consume once, atomically"| EF
    EF --> ENF["Local enforcement point<br/>adjacent to the operation"]
    ENF --> OP[Operation]
    OP --> AUD[("Security Audit<br/>hash-chained")]

    classDef deny fill:#2a1220,stroke:#ff5b7a,color:#ffd6de;
```

- **Eight capability kinds:** filesystem read · filesystem write · process execute · network access · Git mutate ·
  secret access · tool invoke · external action. Scopes are exact — no wildcards, no free-form expressions.
- **Precedence:** `DENY > REQUIRE_APPROVAL > ALLOW`. A weaker layer can never override a hard deny.
- **`effective ⊆ requested`**, and a child's authority is always `⊆` its parent's. Delegation can only narrow.
- **An approved row is not a bearer token.** It is consumed exactly once, only when the exact binding is
  re-presented; widening the scope, changing the policy fingerprint, or waiting past expiry requires a new approval.
- Unknown capability, malformed scope, or unprovable child scope **fails closed**.

Source: [`docs/security/POLICY_CONTRACT.md`](docs/security/POLICY_CONTRACT.md) ·
[`TRUST_BOUNDARIES.md`](docs/security/TRUST_BOUNDARIES.md) · [`THREAT_MODEL.md`](docs/security/THREAT_MODEL.md)

### 🧱 Isolation (runners)

Policy decides *what is allowed*. The Runner enforces *what can actually be enforced*. **Both layers are
required; neither replaces the other.**

```mermaid
flowchart TB
    EC["EffectiveCapabilities<br/>already decided by Policy"] --> PLAN["IsolationPlan::compile<br/>requires exact ProcessExecute capability"]
    REQS["IsolationRequirements<br/>filesystem · network · process"] --> PLAN
    PLAN --> BK{{"IsolationBackend"}}
    BK --> REP["EnforcementReport<br/>bound to this exact plan + backend"]
    REP --> GATE{"Every required axis<br/>proven at the operation boundary?"}
    GATE -->|yes| LAUNCH(["Launch provider process"]):::ok
    GATE -->|"no / stale / mismatched"| BLOCK(["BLOCKED — require user action<br/>never silent downgrade"]):::deny

    classDef ok fill:#0f2a22,stroke:#7cf5c4,color:#d6fff0;
    classDef deny fill:#2a1220,stroke:#ff5b7a,color:#ffd6de;
```

| Backend | Honest claim today |
|---|---|
| `runner-macos-seatbelt` | Confines filesystem, network (outbound TCP/443 only), and process via `sandbox-exec`. Autonomous Claude execution on macOS requires all three axes. |
| `runner-native` | **`Trusted Host`.** Zero enforcement axes. Satisfies only an explicit trusted-host requirement — **not a sandbox**, and never documented as one. |
| Container · Remote · VM | Architectural direction, not implemented. |

On a platform without a concrete backend, the mandatory three-axis requirement makes execution **fail closed
before the provider is launched**. Filesystem isolation is never presented as blocking exfiltration, and process
exit `0` never means the Task is done.

Source: [`docs/architecture/runners.md`](docs/architecture/runners.md)

### 🌿 Workspaces & Git

One Execution owns exactly one `WorkspaceLease`: an isolated worktree on a managed branch, pinned to an immutable
base commit. Agents never write to your checkout or the default branch.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Allocated: intent recorded
    Allocated --> Ready: git worktree observed consistent
    Ready --> CleanupPending: owner terminal
    CleanupPending --> Removed: clean worktree only
    Removed --> [*]

    Allocated --> Allocated: crash → reconcile, never replay blindly
    CleanupPending --> CleanupPending: dirty → fail closed (CleanupUnsafe)
```

```mermaid
flowchart LR
    A[Intent in SQLite] --> B[Git operation]
    B --> C[Observe live Git state]
    C --> D{Matches the lease?}
    D -->|yes| E[Record Ready]
    D -->|no| F[Typed failure<br/>OwnershipMismatch · WorktreeMissing · …]
    G(["Crash"]) -.-> H[Inspect · reconcile · propose]
    H -.-> C
```

- **SQLite and Git are separate failure domains** — there is no cross-system transaction. The pattern is
  *intent → Git operation → observe → reconcile*, and `Ready` is recorded only after success is observed.
- **Git is the authority for Git facts.** Live observations outrank persisted expectations.
- **Base revision** is an exact commit OID — never `HEAD` or a branch tip. Diffs are computed against it.
- **Dirty work is never silently destroyed.** No invisible `reset --hard`, `clean -fd`, forced removal, or
  `branch -D`. A crash means *inspection*, never *cleanup*.
- The default branch is resolved from repository metadata, not by string-matching `main`/`master`.

Source: [`docs/architecture/workspace.md`](docs/architecture/workspace.md)

### ✅ Verified completion

An agent's claim is **not evidence**. Completion is admitted only through the trusted completion gate, atomically,
and only when every acceptance criterion is independently proven.

```mermaid
flowchart TB
    CL["Agent claim<br/>'tests pass, done'"]:::weak -.->|"never satisfies a requirement (CV-1)"| REQ
    subgraph EVID["Evidence — captured by a trusted harness, never by the agent (CV-2)"]
        OE["ObservedEffects<br/>PROCESS_EXIT · TEST_SUITE · ARTIFACT_HASH<br/>FS_STATE · GIT_STATE"]
        RV["Independent review verdicts"]
        SC["Security-check results"]
        HA["Durable human approvals (CV-8)"]
    end
    EVID --> CVF
    REQ["EvidenceRequirement per criterion<br/>derived under the policy fingerprint (CV-7)"] --> CVF{{"ClaimVerification<br/>bound to subject fingerprint (CV-3)"}}
    CVF --> V1["VERIFIED"]:::ok
    CVF --> V2["REFUTED<br/>check ran, evidence contradicts"]:::deny
    CVF --> V3["UNVERIFIED<br/>no conclusive check"]:::warn
    CVF --> V4["BLOCKED<br/>check could not run"]:::warn
    CVF --> V5["EXEMPT<br/>low-risk, policy-minted, capture kinds only (CV-4)"]:::warn
    V1 & V5 --> GATE{"Every criterion<br/>current VERIFIED or EXEMPT?"}
    GATE -->|yes| TX[("ONE SQLite transaction<br/>state write + journal + admission record")]
    TX --> DONE(["COMPLETED"]):::ok
    GATE -->|no| STAY(["Stays VERIFYING<br/>refusal writes nothing"]):::deny

    classDef ok fill:#0f2a22,stroke:#7cf5c4,color:#d6fff0;
    classDef deny fill:#2a1220,stroke:#ff5b7a,color:#ffd6de;
    classDef warn fill:#2a2412,stroke:#ffc857,color:#fff0c9;
    classDef weak fill:#0b0f1a,stroke:#8c97b8,stroke-dasharray:4 3,color:#8c97b8;
```

- **A moved subject invalidates evidence.** If the content, config, or toolchain fingerprint changes, the
  criterion goes back to `UNVERIFIED`. Stale evidence is never silently reused.
- **Only `VERIFIED` passes.** `UNVERIFIED`, `BLOCKED`, and `REFUTED` are distinct and none is a pass (CV-6).
- **Supersession, not deletion.** Contradictory verdicts stay durable and queryable via a `superseded_by` chain.
- **Evidence integrity (CV-9):** an agent that edited the tests or checks used to approve its own task cannot
  self-verify.
- A `HUMAN_APPROVAL` requirement survives any automated outcome — automation can never discharge it.

Source: [`ADR-0018`](docs/architecture/adr/ADR-0018-independent-evaluation.md) ·
[`ADR-0030`](docs/architecture/adr/ADR-0030-g8-observed-effect-claim-verification-contract.md)

### 🧭 Adaptive planning

Planners are untrusted proposers. A `PlanChangeSet` becomes durable only after the application service applies
nine rules in order; the first failure refuses the whole proposal and nothing partial persists.

```mermaid
flowchart TB
    EVT["Triggering evidence"] --> PRE{"Headroom check<br/>before any planner call"}
    PRE -->|no spend into a certain refusal| HDR1
    PRE --> PL["PlannerPort call<br/>≤120 s · ≤32k in · ≤8k out"]
    PL --> PR["PlanChangeSet proposal + proposal_hash"]
    PR --> V

    subgraph V["Validation V1 → V9 (trusted service, in order)"]
        direction TB
        v1["V1 acyclicity"] --> v2["V2 referential integrity"] --> v3["V3 acceptance criteria preserved"] --> v4["V4 scope containment"] --> v5["V5 no policy / approval weakening"] --> v6["V6 budget · depth · fan-out"] --> v7["V7 no duplicate decomposition"] --> v8["V8 base revision not stale"] --> v9["V9 risk escalation needs human grant"]
    end

    V -->|pass| REV[("New plan revision<br/>with reason + provenance")]
    V -->|reject| CNT["typed error → counts toward no-progress"]
    CNT -->|"2 consecutive, same evidence"| HDR1["BLOCKED + HumanDecisionRequest"]:::deny
    v9 -.->|"needs exact-hash single-use grant"| HDR2["HumanDecisionRequest"]:::warn
    REV --> L4{"≥ 1 RUNNABLE leaf?"}
    L4 -->|no| HDR1

    classDef deny fill:#2a1220,stroke:#ff5b7a,color:#ffd6de;
    classDef warn fill:#2a2412,stroke:#ffc857,color:#fff0c9;
```

| Guardrail | Value |
|---|---|
| Decomposition depth | ≤ **4** edges |
| Children per decomposition | ≤ **8** |
| Nodes per plan branch | ≤ **64** |
| Accepted revisions per cycle per branch | **1** |
| Planner calls per evidence event per branch | ≤ **8** |
| Cumulative planner input tokens per event | ≤ **256,000** |
| No-progress trip | **2** consecutive rejections |

Creating children **never** satisfies the parent's acceptance — the parent still needs independent verification.
Plan *revisions* and Task *state transitions* are different concepts and never mix. The planner cannot raise its
own limits.

Source: [`ADR-0017`](docs/architecture/adr/ADR-0017-adaptive-planning.md) ·
[`ADR-0027`](docs/architecture/adr/ADR-0027-g7-planning-orchestration-contract.md)

### 🧑‍💻 Human decisions & takeover

When the machine should not continue alone, it opens a durable `HumanDecisionRequest` — it does not guess.

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Pending: RISK_ESCALATION, NO_PROGRESS, PLANNER_BUDGET, PLANNER_BLOCKED
    Pending --> Resolved: human decides
    Resolved --> Consumed: continuation applied once
    Pending --> Superseded
    Resolved --> Superseded

    state Resolved {
        direction LR
        Resume
        Replan
        Abort
        Takeover
    }
```

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Active: human takes over a scope
    Active --> Completed: handoff packet submitted
    Active --> Expired: TTL elapsed
    Completed --> [*]: authority returns only via a later machine planning cycle
    Expired --> [*]
```

Takeover transfers execution authority to the human for exactly the scoped work. Nothing resumes automatically.

### 🔎 Context engine

The prompt is a **disposable view**, never the system of record. A context package can always be rebuilt from
authoritative sources.

```mermaid
flowchart LR
    subgraph SRC["Authoritative sources"]
        s1[Task state] ~~~ s2[Files / Resources] ~~~ s3[Git] ~~~ s4[Events] ~~~ s5[Decisions] ~~~ s6[Artifacts]
    end
    SRC --> RET["Just-in-time retrieval<br/>(finder · search · reopen spans)"]
    RET --> RANK["Rank · dedupe · prune stale"]
    RANK --> FILT["Security filter<br/>SECRET never enters context"]
    FILT --> PLAN["ContextPlan<br/>Required · Selected · Deferred · Rejected<br/>Budget · Reasons · Source hashes"]
    PLAN --> REND["Provider renderer"]
    REND --> PROMPT(["Rendered package<br/>disposable"]):::weak

    classDef weak fill:#0b0f1a,stroke:#8c97b8,stroke-dasharray:4 3,color:#8c97b8;
```

Every `ContextItem` carries its provenance, content hash, freshness, **trust**, **sensitivity**, and cache class —
kept as independent dimensions. High relevance never implies high trust, and retrieved content **never gains
instruction authority**. The plan explains *why* each item is in the window, so selection is observable and
reproducible.

### ♾ Continuity & rollover

The provider's context window is not the task's memory. Work is organised into **epochs** — bounded windows —
with durable checkpoints between them.

```mermaid
sequenceDiagram
    autonumber
    participant B as Budget Monitor
    participant X as Continuity
    participant DB as SQLite
    participant CE as Context Engine
    participant P as Provider (same or different)

    B->>X: soft limit approaching
    X->>X: build structured checkpoint<br/>(working state, notes, source + evidence refs)
    X->>DB: persist checkpoint + notes + refs
    X->>DB: verify persistence
    alt checkpoint durable
        X->>DB: index old epoch (read-only, still searchable)
        X->>X: activate new epoch
        X->>CE: stable instructions + task envelope + checkpoint + working set
        CE->>P: fresh rendered context
    else checkpoint failed
        X--xX: NO CHECKPOINT → NO ROLLOVER
    end
    Note over B,P: past the hard limit, new execution cycles block
```

```mermaid
flowchart LR
    T[Task] --> E1["Epoch 001<br/>INDEXED · read-only"]
    T --> E2["Epoch 002<br/>INDEXED · read-only"]
    T --> E3["Epoch 003<br/>ACTIVE · exactly one"]:::act
    E1 & E2 -.->|"history.search → history.read(exact_ref)"| E3
    classDef act fill:#0f2a22,stroke:#7cf5c4,color:#d6fff0;
```

- **No epoch is discarded before a durable checkpoint** — and old evidence stays retrievable after rollover.
- **Raw evidence stays authoritative;** summaries of summaries never become truth.
- **A Task can migrate** between providers by design (for example Claude → another adapter) without losing
  state: a new Attempt, a new Session, a new rendered context — same Task and continuity truth. Claude is the
  only provider adapter implemented today.
- Continuity restores *context and evidence*, never *permission* to blindly replay side effects across providers.

Source: [`docs/architecture/continuity.md`](docs/architecture/continuity.md) ·
[`context-engine.md`](docs/architecture/context-engine.md)

### 🗄 Persistence: three ledgers

Three different questions, three separate stores. They are never merged.

```mermaid
flowchart TB
    subgraph W["One mutation"]
        M["Trusted service commits"]
    end
    M --> ST[("<b>Relational current state</b><br/>authoritative: tasks, plans, leases…")]
    M --> EJ[("<b>Event Journal</b><br/>append-only · hash-chained<br/>'what happened'")]
    SEC["Privileged decisions"] --> AU[("<b>Security Audit</b><br/>append-only · hash-chained<br/>'who was allowed what'")]
    KS[("<b>OS Keychain / vault</b><br/>secret values only")]

    ST -.- EJ
    EJ ~~~ AU
```

```text
BEGIN
  UPDATE task current state
  INSERT event-journal record
COMMIT          ← state and its evidence land together, or not at all
```

- **Not full Event Sourcing** in v1: normal operation never replays history to rebuild state.
- The journal is **tamper-evident** (SHA-256 hash chain, append-only triggers). It detects row mutation,
  reordering, and tail truncation when the head is preserved — it is not a signature against full host compromise.
- **Operational logs, the Event Journal, and the Security Audit are three separate concerns**, and the audit never
  flows into model context.

### 🔌 IPC handshake

`agentd` is reachable only through a user-private Unix Domain Socket (Named Pipe on Windows) — never localhost
HTTP, WebSocket, or TCP. **Local does not mean trusted.**

```mermaid
sequenceDiagram
    participant C as agentctl
    participant D as agentd
    C->>D: connect (UDS)
    Note over D: kernel-derived peer identity<br/>from the transport, not from the message
    C->>D: HandshakeRequest(supported versions, client_kind, capabilities)
    alt compatible version
        D-->>C: accepted: chosen version + accepted capabilities
        loop one in-flight request per connection
            C->>D: typed Protobuf request
            D-->>C: response
        end
    else no intersection / unknown client
        D-->>C: HandshakeRejected + daemon's supported versions
        D--xC: connection closed
    end
```

`client_kind` is descriptive metadata — a caller claiming `tauri-host` proves nothing. A negotiated capability is
neither permission nor authentication. Reconnect is a new transport plus a new handshake, never restored socket
state.

### 🔑 Secrets

```mermaid
flowchart LR
    SM[("SQLite<br/>SecretRef + SecretBinding<br/>references only")] --> BR{{"Secrets Broker"}}
    EC["Effective capabilities<br/>SecretAccess(exact ref)"] --> BR
    BR --> BE[("SecretBackend<br/>OS Keychain · vault")]
    BE -->|"borrowed plaintext"| DEL["SecretDelivery<br/>trusted execution / tool boundary only"]
    DEL --> PROC[Provider process]

    BR -. never .-> CTX["model context · logs · events · SQLite plaintext"]:::deny
    classDef deny fill:#2a1220,stroke:#ff5b7a,color:#ffd6de;
```

Plaintext is borrowed only through the trusted delivery boundary and never becomes ordinary application state.

---

## ✦ Repository map

```text
runlith/
├── bins/
│   ├── agentd/            daemon: listener + composition root
│   └── agentctl/          CLI: semantic lifecycle commands
├── crates/                domains, services, contracts (34)
│   ├── task · plan · scheduler · policy · evaluation · workspace
│   ├── context-engine · continuity · takeover · repo-finder
│   ├── runner · secrets · security-audit · agent-runtime
│   ├── app-services       trusted mutation + orchestration
│   └── ipc-contracts · ipc-control · ipc-transport
├── adapters/              concrete I/O, one replaceable crate each (12)
│   ├── storage-sqlite · git-cli · provider-claude · finder-local
│   ├── runner-native · runner-macos-seatbelt · effect-capture-local
│   ├── keychain-native · secrets-vault
│   └── fake-agent · fake-planner · fake-evaluator   deterministic test doubles
├── contracts/             JSON schemas for task and event representations
├── docs/architecture/     binding contracts, invariants, ADRs
├── docs/security/         policy, trust boundaries, threat model
├── planning/              roadmap + task queue (versioned scope & acceptance)
└── product/version.toml   canonical product-version identity
```

Go deeper: [`ARCHITECTURE.md`](ARCHITECTURE.md) · [Invariants](docs/architecture/INVARIANTS.md) ·
[ADRs](docs/architecture/adr/INDEX.md) · [IPC](docs/architecture/ipc.md) ·
[Task engine](docs/architecture/task-engine.md) · [Events](docs/architecture/events.md) ·
[Providers](docs/architecture/providers.md)

## ✦ Principles

- **Local-first.** Cloud and telemetry are opt-in only.
- **Provider-neutral core.** No provider-specific type in domain semantics.
- **Context is disposable.** Prompts are derived state, never the system of record.
- **Prompts are not a security boundary.** Policy decides; isolation enforces.
- **Evidence over claims.** Completion requires verification.
- **Fail closed.** Missing enforcement, unknown capability, ambiguous ownership → refuse, don't guess.
- **Honest isolation.** `NativeRunner` is not a sandbox, and Runlith never claims it is.

### Non-goals (v1)

Another multi-terminal GUI · a generic chat app · a single-provider product · microservices for the local
runtime · cloud-required operation · an autonomous CTO · unrestricted host execution.

## ✦ Status & roadmap

Gates advance on **exit criteria, never calendar dates**.

| Gate | Milestone | Status |
|:---:|---|:---:|
| G0 | Architecture freeze | ✅ |
| G1 | Local control channel (handshake, version negotiation, reconnect) | ✅ |
| G2 | Task core — state machine, SQLite, event journal, `agentctl` | ✅ |
| G3 | FakeAgent runtime — task → schedule → submit → verify | ✅ |
| G4 | Workspace / Git — worktrees, branches, diff, cleanup | ✅ |
| G5 | First real provider (Claude) | ✅ |
| G6 | Security — policy, approvals, secret broker, audit, runner isolation | ✅ |
| G7 | Multi-agent DAG + bounded adaptive planning | ✅ |
| G8 | Verification | ✅ |
| G9 | Context engine + continuity | ✅ |
| G10 | Memory engine | ✅ |
| G11 | Background daemon | ✅ |
| G12 | Remote worker | ✅ |
| **G13** | **Cloud / team** | 🚧 *in progress* |
| G14 | Router / usage economics | ⏳ |

The authoritative plan lives in [`planning/roadmap.yaml`](planning/roadmap.yaml).

## ✦ Development

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
```

Build `agentd` and `agentctl` **before** running binary-level tests — they locate sibling executables, and a
stale `target/debug/agentd` gives stale results.

CI runs the same three checks on macOS with a pinned Rust `1.95.0`
([workflow](.github/workflows/verification.yml)).

## ✦ Contributing

Contributions are welcome. Open a discussion first for larger changes.

- Read [CONTRIBUTING.md](CONTRIBUTING.md); sign off commits (`git commit -s`).
- Coding agents must follow [AGENT.md](AGENT.md) — the repository's canonical policy.
- Found a vulnerability? **Do not open a public issue.** See [SECURITY.md](SECURITY.md).

## ✦ License

[Apache-2.0](LICENSE) — use it, modify it, ship it, commercially or not. Contributions are accepted under the
same terms (inbound = outbound). See [LICENSING.md](LICENSING.md).

<br>

<div align="center">

<sub>Built so the work outlives the session.</sub>

</div>
