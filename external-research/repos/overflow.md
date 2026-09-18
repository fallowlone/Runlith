# Repository

`Nitjsefnie/Overflow`

- repository: `Overflow`
- url: https://github.com/Nitjsefnie/Overflow
- default_branch: `main`
- reviewed_revision: `main@eb11283d8535028702ff17f7ed1d928c8d069166`
- commit_sha: `eb11283d8535028702ff17f7ed1d928c8d069166`
- review_date: `2026-09-15`

# Reviewed revision

The re-audit used a fresh shallow clone of upstream `main` and read source at the exact commit above. The previous Runlith report was treated as a hypothesis list, not as evidence.

# Research target

Re-evaluate Overflow's deterministic settlement/domain rules, reconciliation workflow, external-event handling, authentication/authorization and durable worker semantics against Runlith's current Task, Event Journal, Reconciler, recovery, security and verification architecture.

# Executive summary

Overflow is most valuable to Runlith as a concrete example of evidence-driven reconciliation with explicit worker ownership, bounded retries and separate domain calculations. Its settlement/ledger code validates Runlith's preference for deterministic domain logic and derived views. Its reconciliation worker is also useful because it documents its own failure boundaries precisely.

The old proposal for an "evidence-backed task completion" task is invalid: Runlith already has the stronger accepted `ADR-0018` contract and executable completion tests where only independent evaluation can grant Task completion. The old broad "reconciliation workflow" proposal is also too vague: Runlith already owns reconciliation/recovery semantics for Tasks/workspaces and already routes external lifecycle signals into Event Journal/Reconciler as evidence.

One real delta remains. Runlith's Event architecture explicitly leaves generic external-signal delivery and concurrent-writer semantics open. Overflow supplies a useful leased-job/retry pattern, while also documenting a critical limitation: after lease/session loss, stale writes are not fully fenced. The re-audit therefore records one `NEEDS_ADR` knowledge item for future generic Reconciler work ownership. It does **not** create a TASK. A Runlith adaptation must improve the reference with durable stale-writer fencing and preserve the rule that external signals are evidence, never authority.

# Architecture map

```text
GitHub / forge event or periodic sweep
  -> webhook / integration boundary
  -> durable reconciliation job
  -> leased worker claim + bounded retry
  -> repository evidence fetch / synchronization
  -> deterministic fold / domain calculation
  -> persisted derived state / ledger / moderation state
  -> completion, retry, defer or failed job outcome
```

Canonical state ownership is clearer than the earlier report implied: settlement rules live in pure domain functions; reconciliation evidence and job ownership are explicit types; authorization has centralized gates. The external forge remains evidence input, while application/domain code derives product state.

| Area | Overflow source owner | Re-audit observation |
|---|---|---|
| Core/domain runtime | `src/lib/domain/*`, `src/lib/fold/*` | Deterministic calculations and fold/reconciliation pipeline. |
| Domain/business layer | `settlement.ts`, `ledger.ts`, moderation transitions | Pure typed decisions separated from transport. |
| Orchestration | reconciliation worker + store | Durable jobs, lease renewal, retries, bounded concurrency. |
| Persistence | fold/job stores | Reconciliation evidence/version and job ownership are durable concepts. |
| Provider abstraction | forge/GitHub clients | External source is integration evidence, not Runlith-style AI provider. |
| Scheduler | reconciliation worker schedule | Poll/drain worker with capacity bounds. |
| Security boundary | webhook verification, route authorization, token verification | Explicit verification and fail-closed checks in reviewed paths. |
| Event model | webhook delivery → reconciliation reason/evidence | External event triggers reconciliation; exact source truth is refreshed/folded. |
| Recovery/reconciliation | `reconciliation-worker.ts` | Leases, renewal, retries, deferral, generation, explicit failure. |
| Observability | durable job outcomes + failure hooks/logging | Job failure is isolated per repository. |
| UI/API adapters | Next/API routes | Adapters call domain/application services. |

# Strong engineering decisions

## OVF-F01 — Deterministic settlement and derived ledger

**External source evidence**

- file: `src/lib/domain/settlement.ts`; symbol: `calculateSettlement`; lines_or_region: `3-50`; observation: typed inputs produce `SELF_WORK`, `UNSETTLED` or `SETTLED` deterministically, with bounded point validation.
- file: `src/lib/domain/settlement.ts`; symbol: `creditsForSettledPoints`; lines_or_region: `53-60`; observation: credit amount is derived from settled points and review rounds instead of accepted as an independently mutable value.
- file: `src/lib/domain/ledger.ts`; symbol: `foldLedger`, `foldBalances`; lines_or_region: `9-46`; observation: ledger entries/balances are deterministic projections from valid settlements.

**Problem solved:** keep business state derivable from validated domain facts and avoid duplicated mutable truth.

**Implementation mechanism:** pure domain functions plus a fold into derived ledger entries.

**Why it works:** the same inputs reproduce the same settlement and accounting result; derived balance does not become a competing authority source.

**Failure modes:** correctness still depends on the provenance and validation of upstream settlement evidence; deterministic code does not make external evidence authoritative.

**Runlith equivalent:** Task/Application Service owns validated transitions; Event Journal and projections are evidence/derived views; `INV-070` forbids projections from becoming competing mutable truth; completion authority is separately governed by ADR-0018.

**Novelty:** `ALREADY_DESIGNED` / core state patterns `ALREADY_IMPLEMENTED`.

**Decision:** `EXTERNAL_VALIDATION`.

## OVF-F02 — Explicit moderation transition rule

**External source evidence**

- file: `src/lib/moderation/transitions.ts`; symbol: `deriveSubstantiatedState`; lines_or_region: `15-29`; observation: a validated confirmed-pattern count maps to explicit `WARNED`, `RECALIBRATING`, or `BANNED` states and invalid input fails rather than guessing.

**Problem solved:** avoid lifecycle decisions hidden in route/UI conditionals.

**Runlith equivalent:** explicit Task transition registry, typed Policy decisions and application-owned mutation authority.

**Novelty:** `ALREADY_DESIGNED` / `ALREADY_IMPLEMENTED` as a general Runlith principle.

**Decision:** `EXTERNAL_VALIDATION`.

## OVF-F03 — Reconciliation evidence/version separated from worker synchronization

**External source evidence**

- file: `src/lib/fold/reconciliation-evidence.ts`; symbol: `ReconciliationEvidence`; lines_or_region: `3-34`; observation: persisted upstream evidence carries a format version, checkpoint and full-pass timestamp, while synchronization carries expected version, scan start, full/dirty scope and generation.
- file: `src/lib/fold/reconciliation-jobs.ts`; symbol: reconciliation job types; lines_or_region: `1-18`; observation: jobs distinguish reason, state, attempt count, lease token and rederivation generation.

**Problem solved:** separate observed external evidence from coordination/ownership metadata and make refresh/rederivation explicit.

**Runlith equivalent:** Event Journal/external lifecycle signals are evidence; Reconciler owns derived readiness/recovery; durable current state remains separate from historical evidence.

**Novelty:** core separation is `ALREADY_DESIGNED`; generic external-signal worker ownership mechanics are `NEEDS_ADR`.

**Decision:** `EXTERNAL_VALIDATION` for evidence separation; `ADAPT` for future generic Reconciler work ownership.

## OVF-F04 — Bounded durable reconciliation worker

**External source evidence**

- file: `src/lib/fold/reconciliation-worker.ts`; lines_or_region: `40-89`; observation: retry delays are finite; lease duration, renewal interval and absolute renewal horizon are explicit.
- file: same; lines_or_region: `91-149`; observation: polling and worker concurrency are bounded.
- file: same; symbol: `runNextReconciliationJob`; lines_or_region: `200-280`; observation: one job is leased, same-store duplicate work is deferred, failures are retried/failed with bounded policy, skipped work is deferred, and completion is lease-token-bound.
- file: same; symbol: `startLeaseRenewal`; lines_or_region: `283-353`; observation: renewal has an explicit stop lifecycle and absolute deadline.

**Problem solved:** prevent hidden background retry loops, permanently stuck work and uncontrolled concurrency.

**Why it works:** work ownership and retry are explicit durable workflow semantics rather than timer-local behavior.

**Failure modes:** the implementation itself documents that losing a lease/session does not fence every later publication/write; process-local duplicate exclusion does not span separate processes or direct scripts.

**Runlith equivalent:** specific workspace/execution reconciliation is already implemented in G4/G5; Task lifecycle assigns readiness/recovery to Scheduler/Reconciler; generic external-signal delivery/concurrent-writer semantics remain open in `docs/architecture/events.md`.

**Novelty:** `NEEDS_ADR` for generic external-signal Reconciler ownership semantics.

**Decision:** `ADAPT`, only with stale-writer fencing and Runlith authority rules.

### Differentiation test for OVF-F04

**Reference behavior:** leased reconciliation jobs with heartbeat renewal, bounded retry, bounded concurrency and per-repository exclusion.

**Runlith adaptation:** provider/source-neutral durable reconciliation work claims over normalized external evidence; typed generation/version; bounded lease/retry; idempotent recovery; explicit fencing token/version required on every authoritative publication; Event Journal correlation; no direct side-effect replay from event history.

**Runlith improvement:** `reliability`, `recovery`, `provider neutrality`, `auditability`, and `security` through stale-writer fencing and separation of evidence from authority.

**Why it matters:** Runlith can survive worker/process loss without allowing a superseded worker to publish authoritative state after it has lost ownership.

# Anti-patterns

## OVF-A01 — Lease ownership without complete stale-writer fencing

**Source evidence:** `src/lib/fold/reconciliation-worker.ts:127-134` explicitly states that a reclaim can replace the lease token while the original fold has lost queue authority, and that the described mechanisms do not fence writes after session loss.

**Why it is dangerous:** an expired/superseded worker can continue side effects or publication unless every authoritative write validates current ownership/generation.

**When it becomes a problem:** long or hung I/O, lease renewal failure, process/network partitions, session loss, reclaim races.

**Does Runlith already prevent it?** Runlith has narrow ownership protections in G4/G5 (workspace leases, exact resume proofs, process-local live owner tokens), but generic external-signal reconciler fencing semantics are explicitly not yet designed.

**Relevant Runlith invariant:** `INV-001` durability/recoverability family, `INV-066` no blind side-effect replay, `INV-145` reconcile separate failure domains; Event Journal concurrent-writer/delivery semantics remain open.

**Additional guardrail required:** future Reconciler ADR must make ownership/generation part of the commit condition for every authoritative publication, not just queue bookkeeping.

## OVF-A02 — Process-local duplicate exclusion as a correctness substitute

**Source evidence:** `reconciliation-worker.ts:180-199` documents that the `WeakMap` exclusion is store/process local and that another process or direct reconciliation script can bypass it.

**Why it is dangerous:** in-memory exclusion disappears on restart and cannot coordinate multiple workers/processes.

**Runlith protection:** do not promote local liveness state to durable authority; current Runlith already uses durable ownership for workspace/recovery and explicitly distinguishes process-local live owner state from durable state.

**Rule:** process-local exclusion may reduce duplicate work, but correctness must rest on durable compare/fence conditions.

## OVF-A03 — External event as canonical truth — previous claim invalidated

The earlier report treated this as an Overflow anti-pattern. The re-audit did **not** find enough source evidence to support that broad conclusion. Reviewed code verifies webhook signatures and routes deliveries into processing/reconciliation, while reconciliation maintains explicit upstream evidence/checkpoints. Classification: `NOT_SUPPORTED_BY_EVIDENCE` as a repository-wide claim. Remove it from the reusable anti-pattern library as an Overflow-specific finding; retain the general Runlith rule independently.

# Security findings

## OVF-S01 — GitHub webhook HMAC verification

Status: `CONFIRMED` positive control.

Evidence: `src/lib/github/webhook-signature.ts:1-25` requires a strict SHA-256 signature shape, computes HMAC over the raw body and compares using `timingSafeEqual`; missing/malformed inputs fail closed.

Runlith relevance: `EXTERNAL_VALIDATION` for authenticated external-event ingestion, while signature validity still grants no Task/domain authority.

## OVF-S02 — Authorization re-reads current role

Status: `CONFIRMED` positive control.

Evidence: `src/lib/moderation/route-auth.ts:33-79` resolves credential identity and then fetches the current role from storage rather than trusting a stale session role; non-moderators fail closed.

Runlith relevance: external validation of checking current canonical authority at the operation boundary rather than trusting stale client/session claims.

## OVF-S03 — Forge identity token verified before persistence

Status: `CONFIRMED` positive control.

Evidence: `src/lib/forge/identities.ts:141-225` verifies the remote token/scope; `228-260` verifies identity and required read scope before persistence; storage contract carries encrypted token material rather than raw token.

Runlith relevance: `EXTERNAL_VALIDATION` for Secrets/credential boundary concepts, not a new task because Runlith Secrets Broker is already implemented in G6.

## OVF-S04 — Reconciliation stale-writer window

Status: `CONFIRMED` documented limitation at the architecture/worker level.

Evidence: `reconciliation-worker.ts:127-134` explicitly states missing write fencing after session loss. This report does not claim a concrete exploit beyond the source-documented concurrency failure mode.

Runlith relevance: `ADAPT` only after an ADR specifies durable fencing semantics.

# Agent/orchestration findings

Overflow is not an AI-agent orchestrator, but its reconciliation worker maps cleanly to a Runlith orchestration lesson: background work requires explicit durable ownership, retry bounds, generation/version and completion outcome. The transferable concept is the workflow discipline, not Overflow's product-specific settlement semantics.

The most important negative lesson is that lease heartbeat proves liveness only. It must not be confused with progress or continued authority. Runlith should bind authoritative publication to a current durable ownership generation/fence.

# Implementation findings

The previous proposed task **Evidence-backed Task Completion Records** is removed. Runlith already has `ADR-0018`, Event Journal evidence and executable completion-authority tests. Classification: `ALREADY_IMPLEMENTED` at the relevant semantic contract level and `EXTERNAL_VALIDATION`.

The previous generic **Reconciliation Workflow for External Integrations** is narrowed. Existing Runlith reconciliation/recovery is real and substantial; only the generic external-signal work-ownership/delivery/fencing semantics remain open. Classification: `NEEDS_ADR`, not an implementation task.

# Runlith overlap analysis

| External idea | Closest Runlith equivalent | Current Runlith state | Actual delta | Classification | Decision |
|---|---|---|---|---|---|
| Deterministic settlement / ledger fold | validated Task transitions; Event Journal/projections | designed + core persistence implemented | no semantic gap | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` |
| Explicit lifecycle transition function | Task transition registry / Policy decisions | implemented for current accepted transitions | no semantic gap | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` |
| Evidence-backed completion | ADR-0018 + app-services completion tests | implemented/verified for current flow | Overflow is weaker/general-domain evidence | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` |
| External signals trigger reconciliation | events.md external lifecycle signals → Journal/Reconciler | architecture only for generic signals | exact mappings/delivery semantics remain open | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Leased durable reconciliation jobs | Scheduler/Reconciler authority + G4/G5 specific recovery | specific recovery implemented; generic worker contract absent | generic claim/lease/retry/fencing semantics open | `NEEDS_ADR` | `ADAPT` |
| Webhook signature verification | external input authentication principle + Policy | pattern already required conceptually | protocol-specific implementation irrelevant | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Current-role revalidation | G6 operation-boundary enforcement | implemented | no gap | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` |
| Credential verification/encrypted storage | G6 Secrets Broker | implemented | no gap | `ALREADY_IMPLEMENTED` | `EXTERNAL_VALIDATION` |

Relevant Runlith files/decisions:

- `docs/architecture/events.md` §§11-15 — external signals are observations/evidence; delivery/concurrency semantics are open; event replay does not authorize side-effect replay.
- `docs/architecture/task-engine.md` §§7-13 — Scheduler/Reconciler owns readiness/retry requests; Evaluation alone grants completion.
- `docs/architecture/workspace.md` Crash/Reconciliation — concrete reconciliation for Git/workspace ownership.
- `docs/architecture/adr/ADR-0025-g5-post-admission-recovery.md` — explicit, fail-closed execution recovery.
- `docs/architecture/adr/ADR-0018-independent-evaluation.md` — completion authority.
- `docs/architecture/adr/ADR-0026-g6-security-authority-and-capability-contracts.md` — current authority and secrets boundaries.

# External validation of existing Runlith decisions

- Pure settlement/ledger functions support Runlith's separation of canonical state from derived projections.
- The reconciliation evidence/version model supports Runlith's decision that external signals and journal history are evidence used by a reconciler, not instructions to replay effects.
- Bounded retry/concurrency supports Runlith's explicit recovery and budget philosophy.
- Role revalidation and token verification support G6's operation-boundary authority checks and opaque credential handling.
- Overflow's documented lease/fencing limitation validates why Runlith must treat durable ownership as more than heartbeat/liveness.

# Novel gaps

## OVF-G01 — Generic Reconciler work ownership for external lifecycle signals

Classification: `NEEDS_ADR`.

Evidence of Runlith gap: `docs/architecture/events.md` explicitly leaves exact external-signal mappings, delivery semantics and concurrent-writer mechanism open. G4/G5 implement specific workspace/execution recovery, but no generic source-neutral durable reconciliation-job contract was found.

Reference behavior: Overflow's leased jobs, bounded retries, lease renewal, generation and bounded concurrency.

Required Runlith adaptation: durable, typed work claim with evidence correlation, expected generation/version, bounded retry/defer semantics and a fencing value checked by every authoritative publication.

One-percent improvement: require stale-writer fencing from the first generic contract, improving reliability/auditability over the reference implementation's documented session-loss hole.

No implementation task is proposed before this ADR-level contract is needed by an active gate.

# Adoption matrix

| Finding | Decision | Novelty | Runlith improvement |
|---|---|---|---|
| Deterministic fold/ledger | `EXTERNAL_VALIDATION` | `ALREADY_IMPLEMENTED` | keep domain state + projections explicit |
| Explicit transition rules | `EXTERNAL_VALIDATION` | `ALREADY_IMPLEMENTED` | same principle already enforced |
| Evidence-backed completion | `EXTERNAL_VALIDATION` | `ALREADY_IMPLEMENTED` | independent evaluator gives stronger completion authority |
| Reconciliation evidence/version | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | retain provenance and external-event non-authority |
| Durable leased reconciliation worker | `ADAPT` | `NEEDS_ADR` | add stale-writer fencing + source-neutral evidence correlation |
| Webhook authentication | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | authenticated signal still remains evidence |
| Current-role authorization | `EXTERNAL_VALIDATION` | `ALREADY_IMPLEMENTED` | canonical operation-boundary policy |

# Rejected ideas

- Treating lease heartbeat as proof of progress or sufficient publication authority.
- Using process-local duplicate exclusion as the correctness boundary.
- Copying Overflow's queue mechanics without a durable fencing condition.
- Treating a webhook/external delivery as a direct Runlith Task transition.
- Reintroducing a separate "evidence-backed completion" task when ADR-0018 already owns that contract.

# Candidate ADRs

**Candidate only; not created/accepted:** Generic Reconciler Work Ownership and External-Signal Delivery Semantics.

Trigger: only when an active gate needs general external lifecycle reconciliation beyond the already implemented workspace/execution recovery.

The ADR should decide at minimum: normalized external evidence identity, enqueue/coalescing semantics, claim/lease generation, stale-writer fencing, retry/defer budgets, idempotency, publication transaction boundary, restart behavior and correlation with Event Journal. It must preserve `external signal = evidence`, never authority.

# Candidate tasks

None now. The one actual delta is `NEEDS_ADR`; implementation must wait for an active architectural owner/gate and an accepted contract.

# Source evidence index

All links pin the reviewed commit.

- Settlement domain: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/domain/settlement.ts#L3-L80
- Ledger fold: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/domain/ledger.ts#L1-L46
- Moderation transition: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/moderation/transitions.ts#L1-L36
- Reconciliation evidence: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/fold/reconciliation-evidence.ts#L1-L34
- Reconciliation job vocabulary: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/fold/reconciliation-jobs.ts#L1-L18
- Reconciliation worker bounds/known limitations: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/fold/reconciliation-worker.ts#L40-L199
- Reconciliation worker claim/outcome: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/fold/reconciliation-worker.ts#L200-L353
- GitHub webhook signature: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/github/webhook-signature.ts#L1-L25
- GitHub webhook route/process boundary: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/app/api/github/webhooks/route.ts#L70-L143
- Forge identity verification: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/forge/identities.ts#L28-L260
- Moderator authorization gate: https://github.com/Nitjsefnie/Overflow/blob/eb11283d8535028702ff17f7ed1d928c8d069166/src/lib/moderation/route-auth.ts#L33-L79
