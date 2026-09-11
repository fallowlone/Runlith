# ADR-0024 - G5 Provider Execution Seam and First Real Provider

Status: Accepted
Date: 2026-09-11

## Context

G5 must put one real provider behind the durable Execution/Attempt core
through the production control plane, while keeping the second provider
addable without core changes (roadmap G5 objective). Prior architecture
left the provider seam conceptual: `providers.md` §2 froze no Rust trait
(`AgentProviderPort` stayed open with detect/capabilities/start/send/
cancel), and ADR-0009 required anti-corruption adapters plus a shared
port contract suite without naming the concrete seam. TASK-0046 adopted
`scheduler::ExecutionPort` as the de-facto seam; TASK-0047 composed the
Claude CLI behind it over IPC v2. This ADR records those choices as
accepted decisions so TASK-0050 can audit against authority, not prose.

## Decision

### 1. `scheduler::ExecutionPort` is the provider execution seam

```text
WorkItem { task_id, execution_id, attempt_id }   (durable identities only)
  → ExecutionPort::execute
  → Ok(WorkEvidence { summary, evidence_ref, partial })
  → Err(ExecutionProblem::{TimedOut, Cancelled, Failed})
```

One injected executor per orchestration cycle; no provider comparison,
no routing, no retry on the seam. `provider_ref()` is opaque identity
only — orchestration never branches on it. The conceptual
`AgentProviderPort` (detect/capabilities/start/send/cancel) stays open
for later gates; the execution half is frozen here as `ExecutionPort`.

### 2. Provider adapters are anti-corruption layers behind the seam

ADR-0009 holds and is concretized: adapters translate canonical
`WorkItem`s to provider runtimes and back. Provider-specific protocols
(CLI flags, JSON event shapes, session ids, model names, auth state)
stay inside the adapter crate. No provider-specific type enters Task,
Execution, Attempt, Workspace, Storage, IPC, or orchestration semantics
(INV-004, INV-012); external references travel as opaque `ProviderRef` /
`evidence_ref` text only.

### 3. `agentd` is the composition root for concrete providers

Only `bins/agentd/src/execution.rs` knows the concrete adapter
(`ClaudeExecutor` today): it builds the executor from the durable Task
plus the attempt's ready lease, owns the `CancelRegistry`, the shared
`LiveProcessTracker`, and the provider executable configuration. No
domain, storage, workspace, scheduler, or IPC crate depends on a
provider crate — in either normal or dev dependency direction for
production code (contract tests consume the seam from provider-owned
test targets only).

### 4. IPC v2 carries provider-neutral execution operations

Protocol 2 adds `execution_core` (`run` / `get-status` / `cancel`) with
no provider field, no model field, no credential, no session, and no
completion operation. `run` commits the durable entry synchronously
(claim → start → execution) and continues the provider half on a
detached background thread; `status` re-observes durable state;
`cancel` signals the live run for one execution identity. New error
code `PROVIDER_UNAVAILABLE` (additive; task_core paths never observe
it). v0/v1 sessions reject execution frames as unsupported, preserving
prior surfaces exactly.

### 5. Task / Execution / Attempt / ProviderSession stay distinct

ADR-0005 holds end to end: the CLI session id is opaque provider state
inside `evidence_ref` (`claude:session:<id>`) and attempt detail only —
never a durable identity, never persisted as a column, never completion
evidence. Provider success becomes submittable attempt evidence; only
the independent evaluation path grants completion (ADR-0018). There is
no provider-reachable complete-task operation on any seam.

### 6. Entry is identity-safe before mutation (fail-fast start contract)

`run` refuses synchronously detectable execution/attempt identity
conflicts with typed `ALREADY_EXISTS` before the first mutation — no
claim, no start, no execution, no lease, no attempt, no worktree, and
no journal lifecycle facts. A rejected Task stays in its pre-run state
and is rerunnable with fresh identities. There is no idempotent-replay
path: a duplicate identity is always a refusal. Store-level uniqueness
guards remain the backstop for concurrent races. Post-entry failures
(lease refusal, provider crash, timeout, cancel) legitimately commit
recoverable intermediate state instead.

### 7. First real provider is the Claude Code CLI

`provider-claude` drives the installed `claude` CLI (argv-only,
cwd-bound to the attempt's lease, no shell, stdin closed, environment
inherited unchanged). Version/capability policy is adapter-local:
tested major 2 with minimum minor 1 → `AVAILABLE`, other majors or
older minors → `INCOMPATIBLE` (fail closed), unresolvable → `MISSING`.
`--version` probing never touches authentication.

### 8. Subscription-native CLI authentication stays provider-owned

The adapter never reads credential files, persists tokens, logs them,
forwards them, or repairs them. Detection does not probe auth (no quota
burn, no login risk). Unauthenticated sessions surface at execution
time as bounded `AuthRequired` failure evidence via a narrow
stderr-marker table (unknown-safe), or as generic `Failed`. Auth state
is execution-time evidence, never an installation fact and never part
of the availability vocabulary.

### 9. Cancellation and liveness primitives stay adapter-local for now

`CancelFlag` and `LiveProcessTracker` live in `provider-claude` even
though they are provider-neutral in shape. Cancellation reaches the
provider out-of-band (composition-owned flag → executor wait loop →
owned-tree kill), not through an `ExecutionPort::cancel` method; the
tracker observes spawn/terminal transitions for workspace-cleanup
liveness through the neutral `workspace::OwnerLiveness` seam. No
extraction until a second provider proves the abstraction — speculative
shared runtime infrastructure is rejected.

### 10. A second provider adds without core semantic changes

`provider-codex` (or equivalent) requires: a new adapter crate
implementing `ExecutionPort` (+ its own failure taxonomy and
argv/process mechanics), a factory closure in the composition root, and
its own tests invoking the shared contract suite
(`crates/execution-contract`). No change to Task, Execution/Attempt,
Storage, Workspace, orchestration, completion, journal, or IPC
semantics. Addability is proven by the contract suite, not asserted by
prose.

## Alternatives considered

- Freezing the full conceptual `AgentProviderPort` (detect /
  capabilities / start / send / cancel) as the Rust seam: rejected; G5
  needs the execution half only, and freezing the rest would invent
  capability/routing contracts later gates own.
- Per-provider execution traits with a common subset: rejected; one
  seam, many implementations is the anti-corruption structure
  (ADR-0009).
- `ExecutionPort::cancel` on the seam: rejected; cancellation is
  inherently out-of-band (signal vs in-flight call) and provider
  wiring differs — the seam stays to one terminal behavior per call.
- Extracting neutral process primitives now (H6): rejected as
  speculative; see §9.

## Consequences

- Every adapter passes the shared contract suite
  (`crates/execution-contract`, invoked from provider-owned tests);
  FakeAgent and Claude fixture executors both pass.
- `providers.md` §2/§16 open items for the execution half are closed;
  capability taxonomy, routing, and streaming stay open for later gates.
- `cargo tree -e normal` from any domain crate must never reach a
  provider crate; CI-visible direction for audit.
- Error truncation on every IPC error path is char-boundary safe
  (validated identifiers may be multibyte).

## Security impact

Provider outputs stay untrusted data (INV-093): malformed output can
only become `Failed`, never evidence. No credential reads, no
`--permission-mode` bypass spellings (refused at construction), no
shell interpolation (argv-only), owned-tree kills only (never
name-based). Auth failures carry a ≤500-char stderr excerpt that may
reach attempt detail/IPC — bounded; redaction is G6. TB3
(agentd → Agent CLI) holds.

## Migration / replacement impact

Replacing Claude with (or adding) another provider touches the new
adapter crate plus the composition-root factory only (INV-027,
INV-082). The `ExecutionPort` shape, `WorkEvidence`/`ExecutionProblem`
vocabulary, IPC v2, and the contract suite are stable; provider
addition must not modify them.

## References

- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` §15 (Provider
  Adapters), §16 (FakeAgent), Routing (deferred)
- `INVARIANTS.md`: INV-002, INV-004, INV-009, INV-012, INV-023,
  INV-027, INV-048, INV-059, INV-066, INV-068, INV-082, INV-093
- ADR-0005 (Task != Execution != Attempt != Session),
  ADR-0009 (adapters are ACLs), ADR-0018 (independent evaluation),
  ADR-0021 (versioned business IPC), ADR-0022 (runtime journal scope),
  ADR-0023 (workspace ownership)
- `docs/architecture/providers.md`, `docs/architecture/ipc.md`
  (v2 sections), `crates/execution-contract/src/lib.rs`
