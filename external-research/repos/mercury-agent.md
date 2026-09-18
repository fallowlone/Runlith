# Repository

`cosmicstack-labs/mercury-agent`

- repository: `mercury-agent`
- url: https://github.com/cosmicstack-labs/mercury-agent
- default_branch: `main`
- reviewed_revision: `main@f465e6fb9e8c90712c46f8003801ef822bfe5320`
- commit_sha: `f465e6fb9e8c90712c46f8003801ef822bfe5320`
- review_date: `2026-09-15`

# Reviewed revision

The re-audit used a fresh shallow clone of upstream `main` and read source at the exact commit above. Previous Runlith research was treated only as a hypothesis list.

# Research target

Re-evaluate Mercury's capability registry, permission model, skill loading, agent loop, scheduling, subagent supervision, memory and runtime execution against current Runlith architecture and implemented G5/G6/G7 state.

# Executive summary

Mercury contains several useful implementation patterns: a central capability registry, metadata-first skill discovery with lazy body loading, bounded agent/tool loops, a resource-aware subagent supervisor, and structured long-term memory. These are valuable external validation for Runlith, but the previous report overstated their novelty.

The old proposals for a "skill metadata discovery index" and a "capability execution evidence contract" are invalid as new Runlith tasks. Lazy skills/progressive disclosure already exist in Runlith architecture (`INV-028`, Context Engine, `SkillRegistryPort` target), while trusted runtime receipts are already specified by `INV-072` and the provider/runtime architecture. Their concrete later implementation may still be pending, but they are `ALREADY_DESIGNED`, which is not implementation-queue eligible.

Mercury also provides negative evidence for Runlith's security model. Skill metadata can trigger permission elevation, shell authorization relies partly on command classification, and background shell execution uses direct host process spawning with `shell: true`. Those mechanisms conflict with Runlith's canonical Policy/Runner authority and are reusable anti-pattern evidence rather than adoption candidates.

# Architecture map

```text
Channel input (CLI / Telegram / Signal / Discord / Slack / Web)
  -> core Agent
  -> provider generation loop
  -> CapabilityRegistry
  -> PermissionManager + tool implementations
  -> host/filesystem/network/provider effects
  -> tool results / channel response
  -> memory / task-board / schedule persistence
```

Canonical state owner in Mercury is split by concern rather than one durable orchestration model: the core agent owns the active model/tool loop, `Scheduler` owns schedule manifests, `SubAgentSupervisor` + `TaskBoard` own subagent state, and `UserMemoryStore` owns memory. This is materially different from Runlith, where Task/Execution/Attempt/Provider Session/Policy/Approval/Verification/Recovery remain product-owned durable concepts and providers are replaceable executors.

| Area | Mercury source owner | Re-audit observation |
|---|---|---|
| Core runtime | `src/core/agent.ts` | Model loop directly receives executable tools and step budget. |
| Domain/business layer | distributed | No Runlith-equivalent durable Task authority boundary. |
| Orchestration | `Agent`, `SubAgentSupervisor`, `Scheduler` | In-process orchestration with persisted supporting manifests/boards. |
| Persistence | YAML/JSON + SQLite memory | Concern-specific persistence; not one authoritative event/reconciliation model. |
| Provider abstractions | `src/providers/*` | Provider registry feeds model instances to core loop. |
| Agent loop | `src/core/agent.ts` | Bounded by step counts and repetition detectors. |
| Scheduler | `src/core/scheduler.ts` | Cron/delayed tasks plus persisted schedule manifest. |
| Tool/capability system | `src/capabilities/registry.ts` | Central registration and plan-mode filtering. |
| Security/permission | `src/capabilities/permissions.ts` | Manifest, scopes, command classification, approvals and skill elevation. |
| Sandbox/runtime execution | `src/core/background-tasks.ts` and shell tools | Host process spawning; no Runlith-equivalent canonical Runner boundary shown by reviewed paths. |
| Recovery | scheduler/task-board/background state | Bounded resume exists for subagent step exhaustion; not a general durable reconciler. |
| Context/memory | `src/memory/user-memory.ts`, `src/skills/loader.ts` | Structured memory plus lazy skill materialization. |
| UI/API adapters | `src/channels/*` | Multiple channel adapters call shared core. |

# Strong engineering decisions

## MER-F01 — Metadata-first skill discovery with lazy body loading

**External source evidence**

- file: `src/skills/loader.ts`; symbol: `SkillLoader.discover`; lines_or_region: `42-76`; observation: discovery reads `SKILL.md`, keeps name/description metadata and builds an intent index without retaining every full instruction body as active context.
- file: `src/skills/loader.ts`; symbol: `SkillLoader.load`; lines_or_region: `79-109`; observation: full instructions are loaded on demand by name.
- file: `src/skills/loader.ts`; symbol: `getSkillSummariesText`; lines_or_region: `151-206`; observation: baseline model context receives compact skill summaries and routing hints.

**Problem solved:** avoid putting every reusable workflow into baseline prompt context.

**Implementation mechanism:** discover metadata, index it, expose summaries, then materialize a selected skill body through `use_skill`.

**Why it works:** prompt/context growth is bounded by active relevance rather than installed skill count.

**Failure modes:** metadata/routing text can become de facto behavioral authority; name collisions and user-installed content require provenance/trust controls.

**Runlith equivalent:** `AGENT.md` progressive disclosure rules; `docs/architecture/context-engine.md` disclosure levels and deferred schemas; `INV-028`; Source of Truth `SkillRegistryPort`; `docs/architecture/adr/COVERAGE.md` Tools/Skills registry target.

**Novelty:** `ALREADY_DESIGNED`.

**Decision:** `EXTERNAL_VALIDATION`.

**Previous conclusion invalidated:** do not create a new implementation task for a skill metadata index from this finding.

## MER-F02 — Explicit capability registry

**External source evidence**

- file: `src/capabilities/registry.ts`; symbol: `CapabilityRegistry`; lines_or_region: `79-103`, `174-263`, `265-291`; observation: one registry wires tool implementations, permissions and optional scheduler/skills/memory/subagent capabilities; plan mode exposes a filtered tool set.

**Problem solved:** central discovery/composition of executable agent capabilities.

**Implementation mechanism:** registry-owned tool map with manifest/dependency-gated registration.

**Why it works:** tool availability is explicit and inspectable at one composition boundary.

**Failure modes:** a registry can accidentally become authorization authority; name-based blocklists are brittle if treated as security policy.

**Runlith equivalent:** deferred Tool/Skill Registry target plus implemented provider-neutral G6 capability algebra/Policy Engine (`ADR-0026`, TASK-0060/0061).

**Novelty:** capability registry UX/composition is `ALREADY_DESIGNED`; canonical authorization is `ALREADY_IMPLEMENTED` at the G6 policy layer.

**Decision:** `EXTERNAL_VALIDATION` for registry separation; `REJECT` any design where registry membership grants authority.

## MER-F03 — Bounded tool loop and bounded continuation

**External source evidence**

- file: `src/core/agent.ts`; symbol: `ToolCallLoopDetector`; lines_or_region: `148-205`; observation: absolute tool-call, failed-call and no-action limits bound runaway loops.
- file: `src/core/agent.ts`; symbol: main generation / continuation loops; lines_or_region: `1401-1412`, `2388-2401`, `2845-2855`; observation: generation is bounded by `stepCountIs(...)`; the same capability set is supplied to the provider loop.
- file: `src/core/supervisor.ts`; symbol: `onAgentComplete`; lines_or_region: `242-305`; observation: a step-budget pause is resumed only within a bounded resume count and otherwise remains explicitly paused.

**Problem solved:** runaway model/tool iteration and hidden infinite retry.

**Implementation mechanism:** numeric hard bounds plus repetition detection and explicit paused state.

**Runlith equivalent:** G7 planning guardrails, bounded planner envelopes and retry/recovery separation; provider execution remains subordinate to product orchestration.

**Novelty:** `ALREADY_DESIGNED` / partially `ALREADY_IMPLEMENTED` in current plan guardrails.

**Decision:** `EXTERNAL_VALIDATION`.

## MER-F04 — Structured memory with merge/conflict handling

**External source evidence**

- file: `src/memory/user-memory.ts`; symbol: `UserMemoryRecord`; lines_or_region: `19-41`; observation: records carry type, scope, evidence kind, source, confidence, evidence count, provenance and supersession metadata.
- file: `src/memory/user-memory.ts`; symbol: `retrieveRelevant`, `remember`, `insertRecord`, `mergeRecord`, `resolveConflict`; lines_or_region: `211-335`, `430-535`; observation: retrieval is bounded; candidates are filtered, merged or conflict-resolved before storage.

**Problem solved:** prevent memory from degenerating into unbounded transcript append.

**Runlith equivalent:** accepted `ADR-0012 Structured memory with provenance`, including CAPTURE → NORMALIZE → DEDUPLICATE → LINK → VALIDATE → USE → REFRESH/SUPERSEDE → ARCHIVE.

**Novelty:** `ALREADY_DESIGNED`.

**Decision:** `EXTERNAL_VALIDATION`.

# Anti-patterns

## MER-A01 — Skill metadata participates in authority elevation

**Source evidence:** `src/capabilities/skills/use-skill.ts:18-20` calls `permissions.elevateForSkill(skill['allowed-tools'])`; `src/capabilities/permissions.ts:339-349` converts allowed tools into shell/filesystem elevation flags; `src/capabilities/permissions.ts:507-509` accepts shell skill elevation as automatic permission.

**Why it is dangerous:** reusable content becomes an input to authorization. If skill provenance/trust is weaker than canonical policy, an instruction package can widen effective runtime authority.

**When it becomes a problem:** user-installed, repository-provided, remotely installed, or compromised skills whose metadata is not a trusted policy root.

**Does Runlith already prevent it?** Yes architecturally and in G6 authority: skills cannot override policy/invariants; child authority is monotonic; provider/repository content cannot self-grant.

**Relevant Runlith invariant:** `INV-090`, G6 `INV-146..153`, plus `AGENT.md` skill rule.

**Additional guardrail required:** no new task from this repository. Preserve the existing rule when the future Skill Registry is implemented: skill metadata may request capabilities, never grant them.

## MER-A02 — Command classification/blocklists as a security boundary

**Source evidence:** `src/capabilities/permissions.ts:481-576` classifies shell strings into blocked/safe/approval paths using patterns and parsing heuristics.

**Why it is dangerous:** command-string analysis cannot provide the same boundary as typed capabilities plus OS/runtime confinement; shell semantics and nested execution create a large bypass surface.

**Runlith protection:** `ADR-0026` typed capability policy plus Runner enforcement; TASK-0069 production OS isolation; provider-native/security-string controls are defense in depth only.

**Rule:** command classification may improve UX or defense in depth, but it must never be canonical authorization.

## MER-A03 — Direct host shell process execution

**Source evidence:** `src/core/background-tasks.ts:132-210`, especially the `spawnShell` path, spawns a command with `shell: true`, captures output and enforces timeout.

**Why it is dangerous:** the process starts with host/runtime authority unless another independently enforced sandbox constrains it. Timeout/output bounds improve reliability, not isolation.

**Runlith protection:** canonical Policy + `IsolationPlan` + concrete Runner backend and fail-closed unsupported isolation (`ADR-0026`, TASK-0064/0069).

**Rule:** privileged process creation belongs behind Runner enforcement, never directly in orchestration/domain code.

## MER-A04 — Model-authored memory can overstate evidence provenance

**Source evidence:** `src/capabilities/system/save-memory.ts:5-49` allows the model to decide a fact is important enough to persist and constructs a `direct` evidence candidate; `src/memory/user-memory.ts:430-450` stores provenance from free-form candidate detail.

**Why it is dangerous:** confidence/evidence labels can become stronger than the source that actually supports the memory.

**Runlith protection:** `ADR-0012` requires source refs and validation; agent-generated annotations remain untrusted until promoted under provenance/validation policy (`INV-029`).

**Rule:** memory provenance must point to evidence; model confidence is metadata, never proof.

# Security findings

## MER-S01 — Skill-triggered permission elevation

Status: `CONFIRMED` mechanism.

Evidence: `use-skill.ts:18-20` → `PermissionManager.elevateForSkill` (`permissions.ts:339-349`) → shell elevation accepted by `checkShellCommand` (`permissions.ts:507-509`). The re-audit confirms the authority flow in source. Exploitability through a specific installation path was not required to classify the architecture mechanism and is not asserted here.

Runlith decision: `REJECT`; external validation of existing canonical-policy separation.

## MER-S02 — Direct shell execution without Runlith-style isolation contract

Status: `CONFIRMED` implementation pattern.

Evidence: `background-tasks.ts:132-210` uses Node process spawning with shell interpretation. This report does not claim a Mercury sandbox escape vulnerability; it records that the reviewed execution path is not equivalent to Runlith's typed Policy/Runner boundary.

Runlith decision: `REJECT`.

## MER-S03 — Permission parser residual risk

Status: `PLAUSIBLE` residual risk, not a confirmed vulnerability.

Evidence: `permissions.ts:481-576` contains substantial defensive parsing for chains, redirections and command-specific flags; the reviewed commit itself is a permission bypass-family fix. That history and mechanism make parser completeness a continuing risk class, but no new bypass was proven in this re-audit.

# Agent/orchestration findings

- The provider loop directly receives executable tools (`agent.ts:2388-2395`), so orchestration and model action selection are closely coupled. Runlith should not copy this authority shape; providers remain replaceable executors.
- Mercury's `SubAgentSupervisor` has useful bounded resource/queue behavior (`supervisor.ts:117-190`) and bounded resume (`242-305`), but its state model is not a substitute for Runlith Task → Execution → Attempt → Provider Session.
- `Scheduler` persists task manifests and restores schedules, which is useful operational evidence, but Runlith already separates durable task/recovery state from timers and provider loops.

# Implementation findings

No Mercury-derived item qualifies for a new implementation task after Runlith overlap analysis.

The two previous candidates are withdrawn:

1. **Skill metadata discovery index** → `ALREADY_DESIGNED` → `EXTERNAL_VALIDATION`.
2. **Capability execution evidence contract** → `ALREADY_DESIGNED` (`INV-072`, trusted Tool Execution Receipt boundary) → `EXTERNAL_VALIDATION`.

# Runlith overlap analysis

| External idea | Closest Runlith equivalent | Current Runlith state | Actual delta | Classification | Decision |
|---|---|---|---|---|---|
| Metadata-first lazy skills | Context Engine progressive disclosure; `INV-028`; `SkillRegistryPort` target | architecture only / deferred registry | Mercury validates the shape; no semantic gap | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Capability registry | Tool/Skill Registry target + G6 typed capabilities | registry architecture target; policy implemented | composition UX remains future, authority already stronger | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Capability execution evidence | `INV-072`, events/provider receipt distinction | architecture only for generic Tool Receipt | no semantic gap; mechanics intentionally deferred | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Bounded agent loops | G7 planner guardrails + execution budgets | partially implemented / scheduled | provider-loop-specific heuristic not canonical Runlith need | `PARTIALLY_EXISTS` | `EXTERNAL_VALIDATION` |
| Structured memory lifecycle | ADR-0012 | architecture only | Mercury validates lifecycle concerns; Runlith provenance is stronger | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` |
| Skill grants runtime permission | G6 Policy Engine | implemented | conflicts with Runlith authority model | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |
| Shell string permission boundary | typed capabilities + Runner | implemented G6 foundation/remediation | conflicts with enforcement model | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` |

# External validation of existing Runlith decisions

- Lazy skill loading independently validates `INV-028` without creating new scope.
- Explicit tool/capability composition supports Runlith's decision to make capabilities inspectable, while Mercury's elevation coupling validates why Policy must remain separate.
- Mercury's loop limits validate Runlith's bounded planning/execution guardrails.
- Mercury's memory merge/conflict machinery validates Runlith's rejection of transcript-as-memory and unbounded append.
- Direct shell execution and permission-parser complexity validate G6's two-layer Policy + OS isolation design.

# Novel gaps

None proven from Mercury at the reviewed revision.

# Adoption matrix

| Finding | Decision | Novelty | Runlith improvement |
|---|---|---|---|
| Lazy skill loading | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | provenance/trust + Policy separation |
| Capability registry | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | typed capability algebra; registry cannot grant authority |
| Bounded loop detection | `EXTERNAL_VALIDATION` | `PARTIALLY_EXISTS` | durable orchestration and provider neutrality |
| Structured memory | `EXTERNAL_VALIDATION` | `ALREADY_DESIGNED` | evidence refs and validation before promotion |
| Skill-based permission elevation | `REJECT` | `CONFLICTS_WITH_ARCHITECTURE` | skill may request, Policy decides |
| Shell classification as authority | `REJECT` | `CONFLICTS_WITH_ARCHITECTURE` | typed capabilities + Runner/OS enforcement |
| Direct host shell spawning | `REJECT` | `CONFLICTS_WITH_ARCHITECTURE` | isolated Runner boundary |

# Rejected ideas

- Skill `allowed-tools` as authority elevation.
- Name/string blocklists as canonical permission policy.
- Direct shell spawning from orchestration as a privileged execution boundary.
- Model-created durable memory labeled as direct evidence without source-bound provenance.
- Provider/model tool loop as owner of Runlith Task lifecycle.

# Candidate ADRs

None from Mercury. The future Tool/Skill Registry already requires its own ADR before implementation per `docs/architecture/adr/COVERAGE.md`; Mercury is evidence for that future review, not the reason to create a duplicate ADR now.

# Candidate tasks

None.

# Source evidence index

All links pin the reviewed commit.

- Capability registry: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/capabilities/registry.ts#L79-L292
- Permission manager: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/capabilities/permissions.ts#L283-L584
- Skill discovery/loading: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/skills/loader.ts#L42-L220
- Skill invocation/elevation: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/capabilities/skills/use-skill.ts#L6-L34
- Main tool loop: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/core/agent.ts#L148-L355 and https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/core/agent.ts#L2388-L2401
- Scheduler: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/core/scheduler.ts#L9-L225
- Subagent supervisor: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/core/supervisor.ts#L117-L190 and https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/core/supervisor.ts#L242-L345
- Background process execution: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/core/background-tasks.ts#L132-L210
- Structured memory: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/memory/user-memory.ts#L19-L41 and https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/memory/user-memory.ts#L288-L535
- Model memory save tool: https://github.com/cosmicstack-labs/mercury-agent/blob/f465e6fb9e8c90712c46f8003801ef822bfe5320/src/capabilities/system/save-memory.ts#L5-L50
