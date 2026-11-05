# ADR-0051 - Per-node acceptance statements and explicit workspace change intent

Status: Accepted by the owner, 2026-11-05 (Option A of
`docs/development/PER_NODE_ACCEPTANCE_AND_CHANGE_INTENT_OPTIONS.md`).
Stages 1-6 implemented; see Implementation status.
Amends ADR-0027 and ADR-0030 (AGENT.md section 23).

## Context

A proposed child node carries criterion identities and proof methods but no
prose, so the child cannot state what "done" means and inherits only the
parent objective. `WriteScopeGuard` also derives `requires_change` from a
nonempty write scope, so permission to write implies an obligation to write.

## Decision

1. Each proposed child criterion carries a bounded, nonblank acceptance
   statement beside its stable identity and existing proof method. Trusted
   materialization stores the statement with the criterion and its
   EvidenceRequirement; provider, reviewer and verifier read the same text.
2. The statement participates in the proposal hash and the requirement
   revision. Changing it invalidates stale approvals, grants and
   verification evidence.
3. Text never grants or waives completion. Parent coverage (V3) and risk
   (V9) checks are unchanged. A statement with no supported verification
   requirement stays unverified; no natural-language equivalence with parent
   conditions, no automatic waiver.
4. `requires_workspace_change` becomes explicit Task intent, authorable only
   while DRAFT, consumed by WriteScopeGuard. Existing scoped tasks keep
   `true` through migration. Scoped work that may finish unchanged is declarable; this waives only the
   must-change check, not write restrictions, confinement or verification.
   Client/server negotiation must refuse (not ignore) the field on an old
   peer.
5. New proposal hash schema and pending-proposal codec version. Old records
   stay readable as their original version; old approvals are never upgraded
   to cover new fields. Missing data yields an explicit legacy state or a
   bounded replan refusal, never invented text. Parsing bounds length and
   count before allocation.
6. Materialization retries compare the full accepted specification
   (statements and intent).

## Staged implementation

1. `plan` crate: `CriterionDecl.statement`, hash v2, validation bounds.
2. Provider planner parser and prompt.
3. Storage: codec v2, Task criterion/intent columns, migration.
4. `PlanningService` materialization, retry comparison, provider context.
5. `WriteScopeGuard` + Task intent, IPC negotiation, display.
6. Tests per the options document's verification list; commit `33d7165`
   write-required behavior stays covered.

## Implementation status

- Stage 1 `c10d53a`: `CriterionDecl.statement`, bounded, hashed only when
  present (legacy proposal hashes unchanged).
- Stage 2 `f2062e8`: provider planner parses and prompts the statement.
- Stages 3-4 `77c6ee8`: approved-proposal codec v2 (v1 readable), migration
  0038, `Task.acceptance_statements`, child materialization, provider lines.
- Stage 5 `1bb6121`: `Task.requires_workspace_change` (default true),
  migration 0039, `WriteScopeGuard` consumes it.
- Stage 6: IPC protocol v14 carries the intent (ipc.md section 22).
- Planner-proposed per-node change intent is not part of this ADR: planned
  children keep the default obligation.
- A statement is descriptive; it never changes a requirement's verification.
  It is immutable after DRAFT, so no verification evidence can bind a stale
  statement.
