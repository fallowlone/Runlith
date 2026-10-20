# ADR-0048 — G15 Git Graph & Operations contract

- Status: **Proposed** (TASK-0216; gate G15 registered as `planned` by
  TASK-0215, implementation tasks start only on owner ratification)
- Date: 2026-10-20
- Gate: G15 (planned; independent of G14, no G15 implementation task
  starts before owner ratification, same discipline as G14)
- Extends: ADR-0008 (worktree isolation), G6 security authority
  (ADR-0026, `POLICY_CONTRACT.md`), source-of-truth workspace/Git
  contracts; AGENT.md §16.2 differentiation, §18 generated diagrams,
  §20 canonical state

## Problem

Runlith agents work in isolated worktrees and produce branches, merges
and verification evidence, but neither a human operator nor an AI
consumer can see that history as one trustworthy picture. External Git
clients show commits, yet know nothing about
which branch belongs to which Task, Attempt, agent session or
verification verdict — and their mutation UX assumes a human at a
keyboard, not a policy-governed agent that must never self-approve a
history rewrite. Reading the raw repository is the ground truth
(measured on this checkout, 2026-10-20: 539 commits, 128 merges, 25
refs, 6 registered worktrees), and it already exceeds what a person
reconstructs from `git log` output alone; agent fleets make the gap
wider, not narrower.

Without a frozen contract, a graph view, an AI projection and each Git
operation would invent their own topology, windowing, lane layout and
approval semantics — and the failure modes are not cosmetic: a
silently truncated history presented as complete, a lane layout that
differs between the human diagram and the AI text, or an interrupted
rebase left in an undeclared state each break a binding invariant
(INV-070, INV-145, INV-146, INV-148).

## Decision served and differentiation (AGENT.md §16.2)

This capability exists so an operator can answer, in one place, "what
did every agent do to this repository, what is verified, and what may
happen next" — and so an AI consumer can answer history questions from
a bounded, provenance-carrying projection instead of unbounded raw
`git` output. Parity with leading external Git GUI clients is the floor, not the reason: the
differentiation is (a) overlay of Task/Attempt/worktree/verification
state that no external client has, (b) mutations an agent may propose
but only a human may approve against the exact proposal, enforced by
the Policy Engine, and (c) one canonical graph contract feeding both
the human diagram and the AI projection, so the two audiences can
never silently disagree about topology. Kill criterion: if the AI
benchmark (TASK-0221) shows the projection no better than raw
`git log --graph` at equal or higher token cost, or the overlay cannot
be kept fresh without becoming a second canonical store, the
rendering slices stop and the gate returns for re-scoping; the
read-only graph core remains independently useful and is not killed.

## Decision

### 1. Canonical graph contract (one source, two audiences)

The graph is derived, disposable state (INV-070): the repository is
canonical, the graph is a rebuildable projection carrying provenance.
One producer emits it; the human diagram and the AI text are views of
the same value, never independently computed topologies.

| field | contract |
|---|---|
| `schema_version` | u64, starts at 1; additive-only evolution |
| `repo` | repository root identity + `head_oid` observed at read time |
| `generated_at` / `source` | Unix millis UTC; producer identity (adapter + git version class) |
| `window` | `limit`, `cursor` (opaque, oid-anchored), `returned`, `truncated` — truncation is always explicit, never silent (GR-1) |
| `commits[]` | `oid`, `parents[]` (ordered, first-parent first), `author_name`, `authored_at`, `subject` (bounded length), `refs[]` (names pointing here) |
| `refs[]` | `name`, `kind` (`BRANCH`/`TAG`/`REMOTE`/`HEAD`/`WORKTREE`), `target_oid`, optional `upstream`, optional worktree path |
| `lanes[]` | per-commit lane assignment computed once by the producer (§2) |
| `overlay[]` | optional joins: branch/worktree → `task_id`, attempt state, latest verification verdict, agent session ref, each with `observed_at` and `stale` flag (§5) |

Bounds are contractual, not cosmetic: default window 200 commits,
hard maximum 1000 per response, a byte ceiling on the serialized
payload, subject lines truncated at a fixed bound with an explicit
marker. Author email is excluded by default (privacy: the graph is
metadata, not a directory). Commit bodies, file lists and diffs are
not graph payload in v1; they are separate, separately bounded reads.

### 2. Deterministic lane layout

Lane assignment is a pure function of the window's commit set and
parent edges, computed in the domain crate. Invariants, all
property-tested (TASK-0217): same input → same lanes (determinism);
extending a window by cursor never reassigns lanes of already
returned commits (stability, so paginated human and AI views agree);
merge commits occupy exactly one lane with one edge per parent. The
human renderer and the text projection consume `lanes[]`; neither
recomputes layout.

### 3. AI text projection

The text projection is a deterministic rendering of the canonical
JSON (same crate, contract-tested for consistency): one line per
commit with short oid, lane glyphs, parents, refs and subject, plus a
header carrying window/truncation provenance. Any field dropped by
the projection (author timestamps beyond ordering, overlay detail) is
enumerated in the contract as lossy — nothing is lost silently
(GR-3). Acceptance is measured, not asserted (TASK-0221): a fixed
question set over fixture repositories (ancestry, merge-base, branch
containment, first-parent chains, ref targets) is answered from the
projection and from a `git log --graph --oneline --all` baseline;
pass = accuracy at least baseline at ≤50% of baseline tokens; fail =
any topology question the projection answers worse because parents
or refs were dropped.

### 4. Human diagram (future Tauri, contract now)

No UI package exists in the workspace today, and the UI never
executes Git directly (AGENT.md §4, INV-094). This ADR therefore
fixes only the render contract: the Tauri surface consumes the same
canonical JSON over IPC, renders `lanes[]` as assigned (orthogonal
connectors, one editorial accent for HEAD/current worktree at most,
branch/task identity carried by labels and the overlay, never by
color alone), and scales by window paging rather than by drawing
unbounded history. Visual design, interaction and the timed
human-comprehension benchmark against `git log` and a leading external Git GUI client are
specified in TASK-0222 and implemented with the first desktop UI
gate; nothing in this ADR pre-approves a visual style.

### 5. Architecture placement

| layer | placement |
|---|---|
| Domain | New pure crates `git-graph` (graph model, window, lanes, text projection) and `git-graph-contract` (shared conformance suite), following the org/org-contract pattern of TASK-0208; no I/O, no adapter or provider types (INV-025) |
| Representation schema | `contracts/json-schema/git-graph.schema.json`, a validation projection in the style of `task.schema.json`; schema ≠ persistence ≠ IPC encoding |
| History source | A history port in `crates/git-graph` (mirroring `OrgStorePort` in `org`) implemented by `adapters/git-cli` using the existing bounded argv runner (`run_git`/`run_git_text`, argv-safety checks); structured `rev-list`/`log` formats, no shell, no string-built commands (GR-8) |
| Composition | `GitGraphService` in `crates/app-services`: joins the history port with Task/workspace stores for `overlay[]`; overlay provenance is `observed_at` + source revision, stale overlay is marked, never presented as current fact (GR-10, AGENT.md §20) |
| Surface | Additive IPC envelope + `agentctl git graph --format json\|text` (TASK-0220), versioned like the G13 org surface; Tauri later consumes the same IPC |
| Canonical owner | The repository. Graph responses are snapshots with `head_oid`; a response whose HEAD has moved is stale by construction and says so via `repo.head_oid`, never by mutating a stored graph (INV-070) |

### 6. Mutation model (Ops slices)

Full operations parity is delivered as four bounded slices, each behind
the same approval machinery. No mutation executes from UI intent,
prompt text or provider output (INV-146); canonical authorization
is the Policy Engine decision (INV-146) and hard denials cannot be
weakened by approval or UX mode (INV-148).

1. **Proposal.** Every mutation is first an immutable proposal:
   operation kind, exact argv-class parameters, pre-state snapshot
   (`head_oid`, dirty-state digest, in-progress operation state).
   Approval binds to the proposal hash; any parameter change is a
   new proposal. The approval surface reuses the existing G6 human
   decision machinery; reconciling `ActionProposal`/`CapabilityLease`
   vocabulary with `HumanDecisionRequest` (roadmap G6 follow-up) is
   a precondition of TASK-0223, not something this ADR renames
   unilaterally.
2. **Execution.** Only `agentd` executes, through the workspace
   port and `git-cli` adapter, after a Policy Engine allow and a
   capability-scoped lease. Git/filesystem mutation and SQLite
   state are separate failure domains (INV-145): the journal
   records intent before execution and outcome after; a crash
   between them reconciles from repository state, never by
   assuming success.
3. **Interrupted states are typed, surfaced, and first-class.**
   `MERGE_IN_PROGRESS`, `REBASE_IN_PROGRESS`, `CHERRY_PICK_IN_PROGRESS`,
   `REVERT_IN_PROGRESS`, `BISECT_IN_PROGRESS` (from the existing
   `git_state_snapshot` discipline) appear in the graph contract as
   repository state; continue/abort are explicit proposals, never
   silent auto-resolution and never hidden from either audience
   (GR-7).
4. **Slices.** Ops I: branch/tag create+delete, switch/checkout,
   worktree create/remove (existing port surface). Ops II:
   stage/unstage, commit, stash push/apply/pop, reset (modes are
   distinct proposals; destructive modes require explicit
   dirty-state acknowledgement). Ops III: merge, cherry-pick,
   revert, non-interactive rebase, conflict state surface with
   abort/continue. Ops IV: fetch/pull/push (force-with-lease only
   as an explicitly approved proposal flag), credentials only via
   Secret Broker scoped bindings (INV-151), remote URLs redacted
   of embedded credentials in every output (GR-5), network access
   only under the existing network/SSRF policy boundary.

### 7. Threat model (GR-1..GR-10)

| ID | Threat | Normative rule | Verified by |
|---|---|---|---|
| GR-1 | Truncated/partial history presented as complete | §1: explicit window, `truncated`, cursor; adapter checked against `rev-list` ground truth | TASK-0218 fixture suite |
| GR-2 | Human and AI views disagree on topology | §2: one producer computes lanes; renderers never recompute | TASK-0217 property tests |
| GR-3 | Text projection silently loses parents/refs | §3: lossy fields enumerated; JSON↔text consistency test | TASK-0217/0221 |
| GR-4 | Repository content (subjects, ref names) treated as instructions or authority | Untrusted data (AGENT.md §7): bounded, escaped in UI, never an authorization input (INV-146) | TASK-0218/0222 cases |
| GR-5 | Secrets leak via remote URLs, commit metadata or logs | §6: credential redaction, metadata-only payload, INV-008/INV-151 | TASK-0226 probes |
| GR-6 | Agent or UI mutates history without human approval | §6: proposal + Policy Engine + human decision; UI thin (INV-094) | TASK-0223..0225 refusal tests |
| GR-7 | Interrupted mutation leaves undeclared repository state | §6.3: typed in-progress states, explicit abort/continue, INV-145 reconciliation | TASK-0227 failure matrix |
| GR-8 | Argv injection via crafted ref names, paths, subjects | Existing argv-safety runner; ref/shape validation; no shell anywhere | TASK-0218 adversarial fixtures |
| GR-9 | Resource exhaustion on very large histories | §1 bounds: window, byte ceiling, adapter timeouts; 100k-commit fixture | TASK-0218/0227 |
| GR-10 | Overlay joins stale/wrong Task or worktree state | §5: provenance + `stale` marking; overlay never canonical (INV-070) | TASK-0219 service tests |

Residual risk: a fully compromised local device controls its
repository and credentials and is out of scope, as in prior gates.
History rewrite of already-pushed branches remains possible for a
human approver by design; the contract makes it proposed, approved,
journaled and visible — not impossible.

### 8. Acceptance criteria per slice (external checks)

| slice | pass criterion (external evidence) |
|---|---|
| Domain + schema | Lane property tests hold; schema validates producer fixtures and rejects unbounded payloads |
| History adapter | Graph equals `rev-list` ground truth on every fixture topology: linear, branch/merge, octopus, criss-cross, detached HEAD, shallow clone, bare repo, empty repo, 100k-commit truncation with `truncated=true` |
| Overlay service | Overlay joins only from canonical Task/workspace stores, stale joins marked; graph identical with overlay absent |
| IPC/CLI | Shipped-binary `agentctl git graph` round-trip on a fixture repo; below-version sessions refuse typed |
| AI projection | §3 benchmark: accuracy ≥ `git log --graph` baseline at ≤50% tokens on the fixed question set |
| Human diagram | Timed comprehension vs `git log` and a leading external Git GUI client on the same questions (TASK-0222 spec; measured when UI exists) |
| Ops I–IV | Every mutation: refusal without approval, refusal on proposal-hash mismatch, post-state verified against repository; failure matrix leaves only declared states (GR-7) |

### 9. Deferred decisions and non-goals (explicit)

- **Visual design of the diagram: deferred** to the first desktop
  UI gate (TASK-0222 fixes the render contract and benchmark
  method only). No HTML/SVG prototype is G15 v1 scope.
- **Interactive rebase, patch staging/hunk selection, blame, file
  history, submodule/LFS management: not v1.** They need their own
  design pass; parity is staged, and v1 parity means the §6.4
  slices, not every menu item of an external Git GUI.
- **Hosted/remote repository management** (PRs, issues, provider
  APIs): out of scope; G15 is local Git only. Remote means git
  transport to an already-configured remote.
- **Replacing the workspace worktree machinery:** no. Ops reuse
  ADR-0008 isolation and the existing workspace port.

### 10. Error taxonomy

`NotARepository`, `InvalidRef`, `InvalidCursor`, `WindowTooLarge`,
`UnknownRevision`, `DirtyStateConflict`, `OperationInProgress`,
`ProposalMismatch`, `Forbidden`, `PolicyDenied`, `ConflictState`,
`RemoteRejected`, `CredentialUnavailable`, `BackendFailure`. Errors
are seam-local and typed; no panics on repository-controlled input.

## Alternatives considered

- **UI-first vertical slice** (one Tauri screen before the
  contract): rejected; no UI package exists, and a screen built on
  an unfrozen topology/window contract would fork the human and AI
  semantics this ADR exists to unify.
- **Wrap an existing Git GUI or graph library as the product:**
  rejected per AGENT.md §16.1 — external systems are studied for
  concepts, never inherited as implementation or license surface;
  and no external client can carry the Task/Attempt overlay.
- **AI view = raw `git log` passthrough:** rejected; unbounded,
  provenance-free, and its cost/accuracy is exactly what §3
  measures the projection against.
- **CRDT-style live graph store:** rejected; the repository is
  canonical and the graph is a derived snapshot (INV-070). A stored
  graph would become a second source of truth (GR-10).

## Consequences

Two audiences read one topology: a person gets a bounded,
lane-stable diagram joined with agent work; an AI gets the same
graph as JSON plus a measured text projection. Git mutations become
proposable by agents and decidable by humans under the existing
policy machinery, with interrupted states visible instead of
surprising. TASK-0217 can implement the domain without inventing
windowing, layout, overlay or approval semantics.

## Security impact

Read path is metadata-only, bounded and credential-free; the first
history-rewriting capabilities in the product arrive only as
proposals behind Policy Engine decisions and human approval, with
argv injection, credential leakage and undeclared interrupted
states as named, tested threats (GR-4..GR-9).

## Migration impact

Additive: two new pure crates, one JSON schema, one history port,
one IPC envelope. Repositories and installations that never open
the graph behave identically. Nothing in ADR-0008 worktree
isolation or the G12/G13 identity work changes.

## References

- [Roadmap G15](../../planning/roadmap.yaml), AGENT.md §16.2, §18,
  §20; INV-005, INV-008, INV-011, INV-025, INV-070, INV-094,
  INV-145, INV-146, INV-148, INV-151.
- [ADR-0008](./ADR-0008-worktree-isolation.md) (worktree
  isolation), [ADR-0026](./ADR-0026-g6-security-authority-and-capability-contracts.md)
  and [POLICY_CONTRACT.md](../security/POLICY_CONTRACT.md)
  (authorization and approval machinery),
  [ADR-0047](./ADR-0047-g13-cloud-team-contract.md) (gate/ADR/queue
  registration pattern this gate follows).
- `git rev-list`/`git log` documentation (behavior reference only;
  no external code adopted, AGENT.md §16.1).
