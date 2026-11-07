# Per-node acceptance and explicit workspace change intent

Status: proposal awaiting the owner's acceptance-semantics decision. This is
derived design material for continuing Claude session
`97faf86b-536a-4512-be47-c5e896ff5505`, not an accepted ADR or task completion.
The future TASK-0245 entry on `integration/hardening` names this gap; that
entry does not exist in the checked-out canonical queue.

## Observed loss and current authority

- `crates/plan/src/model.rs`: `NodeAddition` carries scope, criterion
  identities/proof methods, risk and a gate; it has no per-node prose.
- `adapters/provider-claude/src/planner.rs`: the parser accepts those fields.
  `crates/plan/src/hash.rs` hashes them; the pending-proposal codec is v1.
- `PlanningService::materialize_child_tasks` inherits the parent objective
  and stores the child's criterion identities and explicit requirements.
  It cannot preserve prose absent from the proposal.
- `WriteScopeGuard` currently derives `requires_change` from nonempty allowed
  paths. That makes permission to write imply an obligation to write, including
  read-only work performed within a bounded workspace.
- Accepted ADR-0027/ADR-0030 preserve parent obligations and bind completion
  to explicit evidence requirements. A planner's prose or success statement
  cannot grant completion authority.

AGENT.md section 23 requires an explicit ADR/spec revision when requirement
semantics or completion criteria change. The question below decides that
semantic boundary before changing the hash, persisted proposal and Task model.

## Option A: canonical acceptance statements (recommended)

Each proposed child criterion carries its stable identity, a bounded nonblank
acceptance statement, and its existing proof method. Trusted materialization
preserves the statement with the criterion and its EvidenceRequirement, so the
provider, reviewer and verifier refer to the same durable condition. Changing
the statement changes the exact proposal/requirement identity and invalidates
stale approval or verification evidence. Parent coverage and V3/V9 checks
remain mandatory; text cannot waive or replace an inherited obligation.

Example: criterion `c-file` has statement `hello.txt contains exactly hi`,
bound to an independently checked file-content requirement. A provider saying
`done` or merely creating an arbitrary file cannot satisfy it. A condition
without a supported verification requirement remains unverified.

This requires the canonical Task/requirement representation, authoring and
provider context, fingerprints, parser, proposal hash, codec, persistence and
display paths to carry the same data. The ADR amendment must define how newly
proposed text relates to the trusted parent conditions; no implicit natural
language equivalence or automatic waiver is permitted.

## Option B: explanatory text for the executor

Each proposed node carries bounded acceptance guidance alongside its existing
criterion references. It is hashed, persisted and shown to the provider and
operator as proposal context. Existing criterion identities and typed evidence
requirements remain the entire completion contract; the guidance itself is
not a newly verified condition.

Example: `hello.txt contains exactly hi` can explain the work to the provider,
but the UI cannot claim that sentence was verified unless an existing explicit
requirement independently checks it. This is a smaller proposal/context change
and does not deliver Option A's canonical acceptance contract.

## Shared implementation and migration rules

1. Preserve criterion identities; never replace an identity with prose or
   silently change an already accepted condition.
2. Introduce an explicit new proposal/hash schema and pending codec version.
   All new text/change-intent fields participate in exact binding. Old codec
   records remain readable as their original version; old approvals are never
   silently upgraded to authorize new fields. Missing data must yield a clear
   legacy state or a bounded replan refusal, not invented acceptance text.
3. Store `requires_workspace_change` as explicit Task intent and consume that
   field in WriteScopeGuard. It is authorable only while DRAFT. Existing scoped
   tasks retain their prior obligation through migration/legacy admission;
   new clients can explicitly declare scoped read-only work. Client/server
   negotiation must prevent an old server silently ignoring the new intent.
4. New and retried child materialization must compare the full same accepted
   specification, including statements/guidance and change intent.
5. Preserve all evaluator, observed-evidence, policy, human-approval and
   isolation gates. A request for required changes cannot succeed with an
   unchanged worktree; permission to write alone is not a new completion proof.

## Required verification

- A two-child plan keeps distinct per-node text through parsing, hashing,
  pending-proposal store/reopen, acceptance and child Task/provider context.
- Changing text or intent changes the bound hash; stale grants/refusals and
  materialization retries cannot substitute a different specification.
- Legacy codec/accepted history remains readable; malformed, oversized and
  unknown-version input refuses without prefix-driven allocation.
- Required-change work fails on an unchanged worktree. Explicit scoped
  read-only work can succeed when its independent verifier approves it.
  Existing write-required behavior from commit `33d7165` remains covered.
- Option A additionally proves statement/requirement revision binding and
  refusal of a completion claim lacking matching independent evidence.

No public protocol number, product version or canonical task acceptance is
changed by this proposal. Implementation depends on the owner's choice above.
