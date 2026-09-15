# External Pattern Library

Reusable architecture concepts observed in external repositories. This is not an implementation backlog. `Runlith status` records overlap with current Runlith architecture before any adoption decision.

## P-001 — Metadata-first lazy skill loading

**Pattern:** Discover compact skill metadata globally; materialize full instructions only when selected for the current task.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/skills/loader.ts:42-109,151-206`; `src/capabilities/skills/use-skill.ts:6-34`.

**Problem:** Installed workflow count can grow much faster than the useful context for one execution.

**Mechanism:** Metadata discovery/index → compact summaries/routing → on-demand body load.

**Runlith status:** `ALREADY_DESIGNED` — `INV-028`, Context Engine progressive disclosure, deferred `SkillRegistryPort`/Tools-Skills Registry target.

**Runlith adaptation:** Preserve source/trust metadata, retrieval provenance and Policy separation. Skill content may request capability but cannot grant authority.

**Constraints:** No full skill-body prompt bloat; no repository/provider/skill text promotion to system/policy authority.

**Relevant invariant:** `INV-028`, `INV-090`, `INV-098`.

## P-002 — Capability registry separated from authorization

**Pattern:** Compose executable tools in one inspectable registry while keeping authority decisions in a separate policy boundary.

**Source:** `cosmicstack-labs/mercury-agent` (registry pattern); Runlith adaptation deliberately rejects Mercury's skill-elevation coupling.

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/capabilities/registry.ts:79-103,174-291`; `src/capabilities/permissions.ts:283-361`.

**Problem:** Distributed tool registration obscures what an agent can invoke; combining registry membership with authorization creates accidental authority.

**Mechanism:** Central capability/tool inventory plus a separate typed Policy decision/enforcement path.

**Runlith status:** Tool/Skill Registry `ALREADY_DESIGNED`; typed G6 capability/Policy authority `ALREADY_IMPLEMENTED`.

**Runlith adaptation:** Registry describes availability and contracts. `PolicyRequest -> ALLOW | DENY | REQUIRE_APPROVAL` decides authority; Runner/operation owner enforces effective capabilities.

**Constraints:** Registry membership, provider flags, skill metadata and UI state never grant authority.

**Relevant invariant:** G6 `INV-146..153`; `INV-090`.

## P-003 — Bounded autonomous loop termination

**Pattern:** Put hard numeric bounds and repeated-failure/no-progress detection around autonomous model/tool loops.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/core/agent.ts:148-355,1401-1412,2388-2401`; `src/core/supervisor.ts:242-305`.

**Problem:** Agent loops can consume unbounded time/tokens, repeat failed actions or hide incomplete work behind endless continuation.

**Mechanism:** Step/call/failure ceilings, loop detection, explicit pause and bounded continuation.

**Runlith status:** `PARTIALLY_EXISTS` / `ALREADY_DESIGNED` — G7 planner guardrails and budget envelopes are materialized; other autonomous loops remain owned by their subsystem contracts.

**Runlith adaptation:** Bounds live outside provider self-report and produce durable reason/evidence when work pauses, fails, or replans.

**Constraints:** A loop timeout is not Task failure or completion by itself; retry/recovery policy remains canonical.

**Relevant invariant:** `INV-009`, `INV-043`, `INV-045`, `INV-066`.

## P-004 — Structured memory with merge, conflict and provenance metadata

**Pattern:** Store typed memory records with evidence metadata and merge/supersession behavior rather than raw transcript append.

**Source:** `cosmicstack-labs/mercury-agent`

**Revision:** `f465e6fb9e8c90712c46f8003801ef822bfe5320`

**Source evidence:** `src/memory/user-memory.ts:19-41,211-335,430-535,893-930`.

**Problem:** Unbounded conversation memory accumulates duplicates, contradictions, stale facts and summary-of-summary drift.

**Mechanism:** Typed records, bounded retrieval, candidate thresholds, dedup/merge, conflict resolution, confidence/evidence count and supersession.

**Runlith status:** `ALREADY_DESIGNED` by ADR-0012.

**Runlith adaptation:** Use real source refs/content hashes/validation status. Model confidence is metadata only; agent annotations stay untrusted until promoted under validation policy.

**Constraints:** Raw evidence remains authoritative; memory is not automatically injected into every context.

**Relevant invariant:** `INV-029`, `INV-049`.

## P-005 — Deterministic domain fold with derived projections

**Pattern:** Express business decisions as deterministic functions over validated inputs, then derive ledger/projection state from those decisions.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/domain/settlement.ts:3-80`; `src/lib/domain/ledger.ts:1-46`; `src/lib/moderation/transitions.ts:15-29`.

**Problem:** Repeating business semantics across routes/UI/workers creates duplicated canonical state and inconsistent transitions.

**Mechanism:** Pure typed decision functions and deterministic projections from accepted facts.

**Runlith status:** `ALREADY_IMPLEMENTED` as a core architecture principle in Task/Application Service/Event Journal flows; projection details remain subsystem-specific.

**Runlith adaptation:** Keep canonical Task/domain mutation in application/domain services; projections are rebuildable and never competing truth.

**Constraints:** Determinism does not promote untrusted external evidence to authority.

**Relevant invariant:** `INV-009`, `INV-043`, `INV-070`.

## P-006 — Separate reconciliation evidence from synchronization ownership

**Pattern:** Persist what was observed separately from the worker/version/scan state used to coordinate rederivation.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/fold/reconciliation-evidence.ts:3-34`; `src/lib/fold/reconciliation-jobs.ts:1-18`.

**Problem:** If evidence payload and worker ownership are merged, retry/reclaim mechanics can silently rewrite the meaning of the observation.

**Mechanism:** Versioned upstream evidence/checkpoint plus separate expected-version, dirty-subject, reason, attempt, lease-token and generation metadata.

**Runlith status:** Evidence/authority separation `ALREADY_DESIGNED`; general external-signal job ownership is `NEEDS_ADR`.

**Runlith adaptation:** External signal/evidence enters Journal/Reconciler with provenance; durable claim/generation coordinates processing but never changes the evidence's trust level.

**Constraints:** Event replay and job reclaim do not authorize replay of side effects.

**Relevant invariant:** `INV-066`, `INV-070`, `INV-091`.

## P-007 — Durable bounded reconciliation claim with stale-writer fencing

**Pattern:** Coordinate asynchronous reconciliation using durable claims, bounded lease renewal/retries/concurrency, and require a current fencing generation on every authoritative publication.

**Source:** `Nitjsefnie/Overflow` for claim/retry mechanics; the fencing requirement is the Runlith improvement derived from Overflow's documented limitation.

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/fold/reconciliation-worker.ts:40-149,200-353`; documented missing write fencing at `127-134`.

**Problem:** A crashed, partitioned or superseded worker can continue writing after ownership moved to a reclaimer; hidden retries can also run forever or monopolize capacity.

**Mechanism:** Durable claim token/generation, finite lease/renewal horizon, bounded retries/defer/concurrency, plus compare/fence condition on authoritative commit.

**Runlith status:** `NEEDS_ADR` for generic external-lifecycle reconciliation. G4/G5 already implement narrower workspace/execution ownership and recovery.

**Runlith adaptation:** Source-neutral Reconciler claim, Event Journal correlation, expected generation/version and stale-writer fencing at publication. Lease heartbeat proves liveness only.

**Constraints:** No implementation task before an active gate needs the generic contract and the ADR is accepted.

**Relevant invariant:** `INV-001`, `INV-066`, `INV-091`, `INV-145`.

## P-008 — Revalidate current authorization at the operation boundary

**Pattern:** Resolve acting identity, then read current canonical authorization state instead of trusting role/permission claims embedded in an older session.

**Source:** `Nitjsefnie/Overflow`

**Revision:** `eb11283d8535028702ff17f7ed1d928c8d069166`

**Source evidence:** `src/lib/moderation/route-auth.ts:33-79`.

**Problem:** Long-lived or stale client/session claims can outlive revocation or policy changes.

**Mechanism:** Authenticate the credential, reload current role/state, fail closed before privileged operation.

**Runlith status:** `ALREADY_IMPLEMENTED` conceptually and concretely through G6 Policy/approval operation-boundary enforcement.

**Runlith adaptation:** Re-present current exact approval/effective capability at the trusted operation boundary; never treat prior UI/provider/session state as bearer authority.

**Constraints:** Backing-store failure cannot silently become allow.

**Relevant invariant:** G6 `INV-146..153`.
