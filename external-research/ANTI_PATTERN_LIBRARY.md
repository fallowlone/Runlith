# External Anti-Pattern Library

Reusable security, reliability and architecture failure modes confirmed by source-level external evidence. These entries describe lessons for Runlith; they are not claims that every occurrence is an exploitable vulnerability.

## AP-001 — Content package grants runtime authority

**Anti-pattern:** Skill/plugin/workflow metadata directly widens filesystem/process/tool authority.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/capabilities/skills/use-skill.ts:18-20`; `src/capabilities/permissions.ts:339-349,507-509`.

**Failure mode:** A reusable instruction package becomes an authorization input; weaker-trust content can silently expand effective authority.

**Security/reliability impact:** privilege expansion, policy bypass by provenance confusion, difficult auditing of why an operation became allowed.

**Runlith protection:** G6 canonical Policy owns authority; skill/repository/provider content cannot self-grant; child authority is monotonic.

**Remaining exposure:** Future Skill/Tool Registry implementation must preserve this separation explicitly.

**Rule:** Content may declare/request required capabilities. Only canonical Policy/approval may grant effective authority.

## AP-002 — Permission blocklist/parser used as canonical authorization

**Anti-pattern:** Decide security from command strings, allowlists/blocklists and shell parsing heuristics instead of typed capability plus runtime enforcement.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/capabilities/permissions.ts:481-576`.

**Failure mode:** Shell grammar, indirection, nested execution and platform-specific behavior create an ever-growing parser/bypass surface.

**Security/reliability impact:** authorization can diverge from actual process effects; security fixes become reactive additions to a string classifier.

**Runlith protection:** typed Policy capability algebra plus operation-boundary Runner/OS isolation (`ADR-0026`, TASK-0064/0069).

**Remaining exposure:** Provider/tool-specific command classification may still exist as defense in depth; it must never be the canonical allow decision.

**Rule:** String classification may narrow or warn. It cannot substitute for typed authorization and enforced isolation.

## AP-003 — Direct host shell spawn from orchestration

**Anti-pattern:** Orchestration code launches shell-interpreted commands directly on the host without a mandatory isolation/enforcement contract.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/core/background-tasks.ts:132-210` (`spawnShell`, Node `spawn(..., { shell: true })`).

**Failure mode:** Runtime effects inherit whatever host authority is available to the process; timeouts and output limits do not constrain filesystem/network/process authority.

**Security/reliability impact:** sandbox escape is unnecessary if no sandbox is the boundary; host state and credentials can become ambient authority.

**Runlith protection:** Policy → `IsolationPlan` → concrete Runner backend; unsupported mandatory restriction fails before process spawn.

**Remaining exposure:** Trusted Host adapters remain appropriate only when the selected execution contract explicitly permits trusted-host authority.

**Rule:** Autonomous privileged process creation crosses the Runner enforcement boundary.

## AP-004 — Model-authored provenance upgrades its own memory claim

**Anti-pattern:** A model-created memory can assign itself an evidence class such as "direct" while durable provenance is only free-form model/user-summary text.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Evidence:** `src/capabilities/system/save-memory.ts:5-49`; `src/memory/user-memory.ts:430-450`.

**Failure mode:** confidence/provenance labels become stronger than their underlying evidence and survive as durable memory.

**Security/reliability impact:** stale or hallucinated claims can acquire false epistemic authority and contaminate later context/decisions.

**Runlith protection:** ADR-0012 requires source refs, validation, supersession and treats agent-generated annotations as untrusted until promoted (`INV-029`).

**Remaining exposure:** Memory implementation is future work; the source-ref and promotion contract must remain executable, not prompt-only.

**Rule:** Model confidence is metadata. Provenance names independently addressable evidence.

## AP-005 — Lease expires but stale writer is not fenced

**Anti-pattern:** A work lease/token controls queue bookkeeping, yet an old worker can continue publication after its lease/session authority is superseded.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Evidence:** `src/lib/fold/reconciliation-worker.ts:127-134` explicitly documents that lease replacement removes queue authority while neither described mechanism fences writes after session loss.

**Failure mode:** Two generations can both publish effects: the reclaimer is current owner while a stale worker finishes later.

**Security/reliability impact:** lost-update races, stale canonical state, duplicate side effects, audit ambiguity about which owner was authoritative.

**Runlith protection:** Narrow G4/G5 flows already bind workspace/execution ownership and fail closed; generic external-signal reconciler publication fencing is still open.

**Remaining exposure:** Future generic Scheduler/Reconciler work ownership needs an ADR-level fencing contract.

**Rule:** Every authoritative publication from leased work validates the current durable ownership generation/fence, not only the queue row.

## AP-006 — Process-local duplicate exclusion treated as correctness

**Anti-pattern:** An in-memory set/lock prevents duplicate work only inside one process/store instance and is treated as sufficient ownership.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Evidence:** `src/lib/fold/reconciliation-worker.ts:180-199` documents store/process-local exclusion and bypass by another process/direct reconciliation path.

**Failure mode:** restart or multi-process execution erases/bypasses exclusion while both workers still reach shared authoritative state.

**Security/reliability impact:** duplicate work, race-dependent publication, attempt accounting drift and false assumptions about single ownership.

**Runlith protection:** process-local live-owner tracking is explicitly distinguished from durable state in G5; durable leases/records own recovery truth.

**Remaining exposure:** Generic external-signal reconciliation has no accepted work-claim/fencing contract yet.

**Rule:** Process-local exclusion is an optimization/liveness aid. Durable state plus fencing owns correctness.

## Invalidated previous anti-pattern attribution

The previous research attributed **"external event as canonical truth"** to Overflow. This re-audit did not find source-level evidence sufficient to support that repository-specific claim. The reviewed webhook/reconciliation code instead shows authenticated delivery and explicit reconciliation evidence/checkpoints. The general Runlith rule remains valid independently: external events are evidence and require application/domain validation before authoritative mutation.
