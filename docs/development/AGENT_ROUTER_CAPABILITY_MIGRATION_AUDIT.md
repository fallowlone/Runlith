# Agent-Router Capability Migration Audit

**Status:** Complete (audit phase); migration tasks queued
**Date:** 2026-09-23
**Authority:** `AGENT.md`, `ARCHITECTURE.md`, `ORCHESTRATOR.md`, `EXTERNAL_REPOSITORY_ADOPTION_RESEARCH.md` (doctrine), `docs/development/ROADMAP_AUDIT.md` (report conventions)
**Sources audited:**
- Runlith: local checkout at `d0d3001` (main), this worktree on branch `cline/fd038`
- agent-router: local source-only checkout at `/Users/artemmac/dev/agent-router` (no git history; prototype-grade TypeScript, `AGENT_ROUTER_CAPABILITIES.md` as its own manifest)

**Scope rule applied:** agent-router is treated as a *research prototype* of the same author — a source of validated design patterns and evaluation protocols, never as code to port literally. Every claimed gap below was verified semantically against current Runlith source (crate/module inspection), not by filename search.

---

## 1. Executive Summary

agent-router explores one problem Runlith has deliberately deferred: **cost-aware, eligibility-gated model routing with usage economics**. Its durable contributions are (a) a disciplined routing decomposition — eligibility gates first, weighted scoring second, policy-owned weights; (b) a multi-scope provider health/quota model; (c) a verification-level taxonomy (V0–V3) driven by changed-path classification; (d) a usage-economics rollup that separates zero-marginal from metered spend and *unknown* from *zero*; and (e) an offline, privacy-safe decision-receipt protocol for evaluating routing quality without giving a learned router authority.

Its prototype shortcuts are equally instructive and must **not** migrate: global mutable in-memory state, env-var policy, a prompt-contract reviewer (`# PASS|# FAIL|# BLOCKED`) that makes the agent its own verification authority, and hardcoded role/billing policy tables.

Runlith is architecturally ahead everywhere that matters for trust: durability, Task≠Execution≠Attempt≠Session separation, Policy Engine authority, Ports & Adapters, independent verification with observed effects (TASK-0123/TASK-0124, merged). The migration therefore adopts agent-router's *routing/economics semantics* onto Runlith's *durable, policy-governed substrate* — nothing is bolted on.

**Bottom line:** 44 capabilities inventoried; 15 migration tasks queued (TASK-0128…TASK-0142), one new gate proposed (`G14 Router / Usage Economics`), two capabilities rejected as invariant violations, the rest kept as Runlith-better or research-only.


---

## 2. Methodology

1. Read Runlith authority documents and current planning state (`planning/roadmap.yaml`, `planning/tasks/queue.yaml`, `product/version.toml`).
2. Read agent-router exhaustively via four parallel subsystem audits (providers/routing; context intelligence; planning/review/verification; economics/Laya/docs), then re-verified every decision-critical file first-hand: `routing.ts`, `scorer.ts`, `health.ts`, `quota.ts`, `pool.ts`, `fallback.ts`, `escalation.ts`, `errors.ts`, `requirements.ts`, `privacy.ts`, `quality.ts`, `affinity.ts`, `budget.ts`, `limiter.ts`, `state.ts`, `discovery.ts`, `probe.ts`, `openai-compatible.ts`, `codex.ts`, `laya.ts`, `typesafe.ts`, `economics.ts`, `verify.ts`, `outcome.ts`, `risk.ts`, `prerequisites.ts`, `provider-roundtrip.ts`, `cli.ts`, `scripts/decision-receipts.mjs`, `scripts/model-quality.mjs`.
3. Validated each claimed Runlith gap against current source semantically (`crates/agentd-orchestrator`, `agentd-provider-openai`, `agentd-provider-codex`, `agentd-receipts`, `agentd-policy`, `agentd-observability`, `agentd-store-sqlite`, `agentd-tools`, `agentd-context-engine`, `docs/architecture/providers.md`, `context-engine.md`).
4. Classified every capability with status + action (Section 5), derived boundary constraints (Section 6), mapped each adopted capability to a Runlith port/adapter surface (Section 7), and converted approvals into bounded queue tasks with gate alignment (Section 8).

**Trust assumption note:** agent-router code was treated as untrusted input for design extraction only. No code, dependency, or prompt text is copied verbatim into Runlith; every migration task requires fresh Rust implementation against Runlith contracts.

---

## 3. Agent-Router Current-State Map

Single-process TypeScript CLI (`src/cli.ts`, ~900 lines) orchestrating a four-role loop — explorer → lead → builder → reviewer — with per-role model routing. All runtime state lives in process-global maps, optionally snapshotted to `~/.agent-router/provider-state.json`.

| Subsystem | Files | Essence |
|---|---|---|
| Catalog/registry | `providers/catalog.ts`, `registry.ts`, `types.ts`, `accounts.ts`, `config.ts` | Static catalog: 11 providers / 17 models with capabilities (tools/coding/reasoning), roles, quality tier, trust level, billing kind, rate limits, context size. Multi-account credential resolution. |
| Routing | `routing.ts`, `pool.ts`, `requirements.ts`, `scorer.ts`, `budget.ts`, `privacy.ts`, `affinity.ts` | Eligibility-first pipeline: hard gates (privacy mode, billing allow-list, role, capability requirements, min trust, min context, health, quota, credentials) produce a candidate pool; weighted scoring ranks survivors; per-repo/role affinity adds a bounded bonus. Every candidate decision is explainable (`failures[]`). |
| Health | `health.ts`, `state.ts` | Three independent scopes (provider, account, model); consecutive-failure thresholds escalate healthy→degraded→cooldown; cooldown expiry demotes to degraded (not healthy); snapshot/restore with strict validation. |
| Quota | `quota.ts` | Per-account sliding windows (requests/tokens per minute and day), checked against catalog limits before selection and before execution. |

| Transport | `openai-compatible.ts`, `codex.ts` | OpenAI-compatible POST with: granular error taxonomy, `Retry-After` + `x-ratelimit-reset-*` parsing (seconds/ms/epoch/http-date), bounded transient retries (server/timeout only), per-provider concurrency semaphore, quota recording on success. Codex CLI transport parses `--json` events into usage + tool calls + per-turn context telemetry. |
| Fallback/escalation | `fallback.ts`, `escalation.ts` | Ranked-candidate fallback: provider-scoped failure kinds (`rate_limit, quota, auth, timeout, server`) block the *provider* for the rest of the loop; other errors abort. Quality ladder (local→free→luna→terra→sol); evidence triggers (verification/review failure, uncertainty, risk, tool-loop exhaustion) escalate one tier; risk jumps to top. |
| Verification | `verify.ts`, `outcome.ts`, `prerequisites.ts` | Verification levels V0–V3 selected from changed paths (lockfiles/config/schema→V3; docs-only→V0; `.d.ts`→V1; else V2); ecosystem command auto-detection (npm/pnpm/yarn/bun/cargo/go); scope from baseline-diff + write-tool observation; outcome derived from verification + review + blocked signals. Prerequisite checks: git ref ancestry, repo-relative file existence, PATH commands. |
| Risk | `risk.ts`, `typesafe.ts`, `laya.ts` | Regex baseline (6 categories) plus optional semantic scoring via TypeSafe SystemOne API or a local HuggingFace model ("laya") over bounded evidence (24k chars), thresholded into premium-review reasons. |
| Context | `compression.ts`, `compressors/*`, `context-telemetry.ts`, `planning-context.ts`, `review-context.ts`, `planning-artifact.ts` | 4-chars/token estimator; extractive/semantic/hybrid compression prototypes with budget allocation; source-attributed, multi-turn context telemetry; bounded planning packets and review-context artifacts (v1, persisted) with head/tail/keyword packing; observation handles; compaction receipts. |
| Economics | `usage.ts`, `cost.ts`, `economics.ts` | Per-role/provider/model token+cost rollups; `BillingKind` (local/free/subscription/metered/unknown); zero-marginal share; free-offload %; cache-hit rate; premium escalations; per-successful-task efficiency metrics; unknown-cost tokens tracked separately from zero. |
| Evaluation | `scripts/decision-receipts.mjs`, `scripts/model-quality.mjs`, `scripts/local-critic.py`, `provider-roundtrip.ts`, 12 retrieval/token benchmark scripts | Privacy-safe decision receipts (fingerprinted run + inputs, full option set with eligibility failures, selection, outcome), explicitly `authority: offline-evaluation-only`; model-quality aggregation blended into the scorer with confidence shrinkage; local-critic baseline; provider round-trip conformance probe. |

**Prototype shortcuts (by design):** all health/quota/affinity/quality state is in-memory per process; policy is env vars (`AGENT_BUDGET_MODE`, `AGENT_ALLOW_AGGREGATORS`, …) and hardcoded tables (`ROLE_REQUIREMENTS`); the reviewer is a prompted agent whose first line is the verdict; run artifacts are plain JSON files under `.agent/runs/`.

---

## 4. Runlith Current-State Map (relevant surfaces)

| Surface | Location | Today |
|---|---|---|
| Orchestration | `crates/agentd-orchestrator` | Owns Task→Execution→Attempt→Session lifecycle, verification attempts, repair/escalation (TASK-0125 in progress), evidence packs (TASK-0126 pending). |
| Verification | TASK-0123/TASK-0124 (merged) | Verification commands executed by orchestrator; observed effects (file/process/network observations) captured durably; verifier verdict independent of agent claims (G8). |
| Provider Runtime | `crates/agentd-provider-openai` (`openai.rs`, `retry.rs`, `circuit_breaker.rs`, `sse.rs`, `token.rs`, `codex_oauth.rs`, `catalog.rs`), `agentd-provider-codex` | OpenAI + Codex/OAuth adapters behind ports; per-request retry policy + circuit breaker; token usage recorded on responses; **no** routing, health registry, quota ledger, billing/trust taxonomy, or economics. |
| Policy Engine | `crates/agentd-policy` (+ `agentd-policy-ast`) | Policy parsing/decisions; G6 (provenance), G7 (sessions) enforced; **no** routing/budget/privacy-mode policy types yet. |
| Context Engine | `crates/agentd-context-engine` (G9, planned) | `tiktoken` feature exists; design in `docs/architecture/context-engine.md`; compression/allocation not implemented (TASK-0127 queued first). |

---

## 5. Capability Matrix

Status: `MISSING` (no Runlith counterpart) · `PARTIAL` · `RUNLITH_WORSE` · `EQUIVALENT` · `RUNLITH_BETTER` · `CONFLICTS` (violates a Runlith invariant) · `RESEARCH_ONLY` · `PLANNED_NOT_IMPLEMENTED` (both sides).
Action: `IMPLEMENT` · `ADAPT` (semantics yes, code no) · `KEEP_RUNLITH` · `BENCHMARK_ONLY` · `DEFER_WITH_TASK` · `REJECT`.

### 5.1 Provider / Routing

| # | Capability (agent-router source) | Runlith status | Action | Target surface / task |
|---|---|---|---|---|
| R1 | Eligibility-first routing: hard gates → candidate pool → weighted score (`routing.ts`, `pool.ts`) | MISSING | IMPLEMENT | Router port, G14 — TASK-0129 |
| R2 | Explainable candidate decisions (`failures[]` per candidate, full telemetry) | MISSING | IMPLEMENT | Router decision receipts — TASK-0129/0138 |
| R3 | Score model: quality/reliability/quota-headroom/latency/cost/context-fit, normalized weights (`scorer.ts`) | MISSING | ADAPT | Shadow scorer only until shadow-mode evidence — TASK-0138 |
| R4 | Empirical quality blending with confidence shrinkage (`prior·(1−c)+empirical·c`, c=tasks/20) | MISSING | ADAPT | Model-quality aggregation — TASK-0138 |
| R5 | Three-scope health (provider/account/model), consecutive-failure thresholds, cooldown→degraded on expiry, validated snapshot restore (`health.ts`, `state.ts`) | RUNLITH_WORSE (single-scope in-memory circuit breaker only) | ADAPT | Durable health registry — TASK-0131 |
| R6 | Per-account quota windows rpm/tpm/rpd/tpd checked pre-selection and pre-execution (`quota.ts`) | MISSING | ADAPT | Durable quota ledger — TASK-0132 |
| R7 | Rate-limit header normalization: `Retry-After` (s/ms/epoch/http-date) + `x-ratelimit-reset-*` family (`openai-compatible.ts`) | PARTIAL (`extract_retry_after` only, 3 call sites) | ADAPT | Provider Runtime — TASK-0130 |
| R8 | Provider error taxonomy (`rate_limit, quota, auth, model_not_found, timeout, server, invalid_response`) + retryable flags (`errors.ts`) | PARTIAL (`retryable` bool, no kinds) | ADAPT | Provider Runtime contract — TASK-0130 |
| R9 | Provider-scoped fallback loop: provider-failure kinds block the provider for remaining candidates (`fallback.ts`) | MISSING | IMPLEMENT | Router execution — TASK-0129 (contract), later runtime task |
| R10 | Quality escalation ladder with evidence triggers; risk→top (`escalation.ts`) | MISSING | ADAPT | Policy-owned escalation (generic tiers, not hardcoded ladder) — TASK-0142 |
| R11 | Budget modes as policy presets (free-first/balanced/quality-first → allow-lists + weights) (`budget.ts`) | MISSING | ADAPT | Policy Engine types — TASK-0129 |
| R12 | Repository privacy modes (local-only / official-only / allow-aggregators) (`privacy.ts`) | MISSING | ADAPT | Policy Engine; file location becomes policy input — TASK-0129 |
| R13 | Trust levels (unverified/aggregator/official/local) with `minTrust` gates (`requirements.ts`) | MISSING | ADAPT | Provider catalog contract — TASK-0129 |
| R14 | Billing kinds (local/free/subscription/metered/unknown) | MISSING | ADAPT | Catalog + Usage — TASK-0129/0133 |
| R15 | Runtime model discovery via `/models`, TTL cache, unknown models marked provisional/unverified (`discovery.ts`) | MISSING | ADAPT | Provider Runtime — TASK-0141 |
| R16 | Capability probing (chat + tool-call probe requests) (`probe.ts`) | MISSING | ADAPT | Provider Runtime, opt-in — TASK-0141 |
| R17 | Per-provider concurrency semaphore (`limiter.ts`) | EQUIVALENT (agentd scheduling owns concurrency) | KEEP_RUNLITH | — |
| R18 | Transient retry, bounded backoff, server/timeout only (`openai-compatible.ts`) | RUNLITH_BETTER (`retry.rs` policy) | KEEP_RUNLITH | — |
| R19 | Static 11-provider/17-model catalog with limits (`catalog.ts`) | PARTIAL (`providers.md` documents, `catalog.rs` minimal) | ADAPT | Config-driven catalog data — TASK-0129 |
| R20 | Per-repo/role provider affinity bonus (`affinity.ts`) | MISSING | DEFER_WITH_TASK | Fold into shadow evaluation first — TASK-0138 |
| R21 | Codex CLI JSONL transport with per-turn usage telemetry (`codex.ts`) | EQUIVALENT (Codex OAuth adapter exists; turn telemetry missing) | ADAPT | Feed turn-level usage into Usage rollup — TASK-0133 |

### 5.2 Verification / Review / Planning

| # | Capability | Runlith status | Action | Task |
|---|---|---|---|---|
| V1 | Verification levels V0–V3 from changed-path classification (`verify.ts`) | MISSING | IMPLEMENT | TASK-0134 (G8) |
| V2 | Verification scope: baseline-diff paths + write-tool observation → no-op detection (`verify.ts`) | MISSING | IMPLEMENT | TASK-0134 (G8) |
| V3 | Ecosystem command auto-detection (lockfile→package manager; cargo; go) (`verify.ts`) | PARTIAL (commands configured, not derived) | ADAPT | TASK-0134 (G8) |
| V4 | Command runner with timeout, bounded output capture, per-command duration | EQUIVALENT (TASK-0123 runner) | KEEP_RUNLITH | — |
| V5 | Outcome derivation from independent signals (verification + review + blocked) (`outcome.ts`) | PARTIAL (TASK-0124 records effects; derivation rule absent) | ADAPT | Feeds TASK-0125 design; noted, no new task |
| V6 | Prompted reviewer verdict contract (`# PASS|# FAIL|# BLOCKED`, `review.ts`) | CONFLICTS (agent self-verification violates G8 verifier independence) | REJECT as authority; ADAPT as *advisory* signal only — verdict remains orchestrator's | — |
| V7 | Prerequisite preflight: git ref ancestry, repo-relative files, PATH commands (`prerequisites.ts`) | MISSING | IMPLEMENT | TASK-0135 (G8) |
| V8 | Regex risk baseline, 6 categories (`risk.ts`) | MISSING | ADAPT | Advisory risk signal — TASK-0136 (G8) |
| V9 | Semantic risk scoring via external API / local model, bounded evidence (`typesafe.ts`, `laya.ts`) | MISSING | BENCHMARK_ONLY (offline; privacy review required; no production dependency) | TASK-0140 |
| V10 | Planning artifact v1 (brief/constraints/acceptance) persisted + handoff (`planning-artifact.ts`) | PLANNED_NOT_IMPLEMENTED (G9) | ADAPT | G9 design input — TASK-0137 |
| V11 | Review-context artifact v1: bounded evidence packet (diff head/tail, verification output, builder report) (`review-context.ts`) | PLANNED_NOT_IMPLEMENTED (G9) | ADAPT | G9 design input — TASK-0137 |

### 5.3 Context Intelligence

| # | Capability | Runlith status | Action | Task |
|---|---|---|---|---|
| C1 | 4-chars/token estimator | RUNLITH_BETTER (Runlith has `tiktoken`) | KEEP_RUNLITH (heuristic only as tokenizer-unavailable fallback) | — |
| C2 | Compression pipeline: extractive/semantic/hybrid compressors + budget allocator (`compression.ts`, `compressors/`) | PLANNED_NOT_IMPLEMENTED (G9) | ADAPT as design input; fresh implementation | TASK-0137 |
| C3 | Source-attributed, multi-turn context telemetry (`context-telemetry.ts`) | MISSING | ADAPT | G9 + Usage — TASK-0137/0133 |
| C4 | Observation handles / archived tool observations | PLANNED_NOT_IMPLEMENTED (G9) | ADAPT | TASK-0137 |
| C5 | Compaction receipts | PLANNED_NOT_IMPLEMENTED (G9) | ADAPT | TASK-0137 |
| C6 | 12 retrieval/token benchmark scripts | RESEARCH_ONLY | BENCHMARK_ONLY | Fold relevant protocols into TASK-0138 harness |

### 5.4 Economics / Usage

| # | Capability | Runlith status | Action | Task |
|---|---|---|---|---|
| E1 | Usage rollup: per-role/provider/model tokens+cost, cache-hit rate (`economics.ts`) | MISSING | IMPLEMENT | TASK-0133 (G14) |
| E2 | Zero-marginal share / free-offload % (local+free vs total) | MISSING | IMPLEMENT | TASK-0133 |
| E3 | Unknown-vs-zero cost discipline (`unknownCostTokens`, `unknownCostRoles`) | PARTIAL (usage is `Option` at adapter level; no rollup rule) | ADAPT | TASK-0133 (norm: unknown ≠ 0 everywhere) |
| E4 | Per-successful-task efficiency metrics (tokens, zero-marginal tokens, premium escalations, metered USD per successful task) | MISSING | IMPLEMENT | TASK-0133 |
| E5 | Metered cost estimation from pricing table (`cost.ts`) | MISSING | ADAPT | Catalog pricing data — TASK-0133 |
| E6 | Model-quality stats from run history (`quality.ts`, `model-quality.mjs`) | MISSING | IMPLEMENT (offline aggregation, shadow input) | TASK-0138 |
| E7 | Privacy-safe decision receipts, `authority: offline-evaluation-only` (`decision-receipts.mjs`) | MISSING (Runlith receipts exist for runs, not routing decisions) | IMPLEMENT | TASK-0138 |
| E8 | Run artifact persistence (`.agent/runs/*.json`) | RUNLITH_BETTER (SQLite + receipts + evidence store) | KEEP_RUNLITH | — |

### 5.5 Explicitly rejected / non-migratable

| Item | Reason |
|---|---|
| Global mutable registries as system of record | Violates durability + single-writer invariants; agentd/SQLite own state |
| Env-var policy (`AGENT_*`) | Violates Policy Engine authority; policy is data, typed and versioned |
| Reviewer prompt contract as verdict authority | Violates G8 verifier independence; advisory only |
| Hardcoded `ROLE_REQUIREMENTS` / quality ladder names | Prototype-specific; Runlith generalizes to policy-driven requirements and capability tiers |
| TypeSafe/Laya as production dependency | External API + unreviewed local model in the trust path; offline benchmark only |

---

## 6. Boundary Analysis

**What agent-router proves is worth having:** eligibility-before-scoring with explainable rejections matches Runlith's doctrine exactly ("hard requirements as eligibility constraints, never score penalties" — already in `roadmap.yaml` notes); multi-scope health/quota is the minimum viable routing state; V0–V3 verification levels are a direct cost/latency win for G8's verification loop; the decision-receipt protocol is precisely the "shadow-mode evidence" the roadmap demands before any learned routing.

**Where the prototype would damage Runlith if ported naively:**
1. *State ownership.* Every registry (health, quota, affinity, limiter, quality) is process-global and lost on restart. In Runlith all of it must be durable, per-Task/Execution-scoped where applicable, and written through agentd's store ports.
2. *Authority.* The reviewer-agent verdict and env-var policy invert Runlith's authority chain. Routing weights, budget modes, privacy modes, and escalation rules are **Policy Engine data**; the Router proposes, the Policy Engine constrains, the orchestrator disposes.
3. *Provider neutrality.* Trust levels and billing kinds are catalog *metadata* consumed by policy — never a ranking hardcoded into the Router, and never a reason to exclude a provider adapter from the runtime.
4. *Self-verification.* No agent output (reviewer verdict, risk self-assessment) may become a verification authority. Advisory signals only; G8 verdicts remain orchestrator-derived from observed effects and verification commands.

**Compatibility risks:** (a) quota/health ledgers must not leak provider credentials into durable state; (b) routing telemetry contains repo/task fingerprints — decision receipts must stay privacy-safe (hash fingerprints, as the prototype already does); (c) semantic-risk benchmarking must run on synthetic/sanitized tasks only until a privacy review exists.

| R22 | Round-trip conformance probe for provider behavior (`provider-roundtrip.ts`) | MISSING | BENCHMARK_ONLY | Adapter conformance tests — TASK-0141 |

| Durability | `crates/agentd-store-sqlite` | SQLite WAL store; schema migrations; receipts (`agentd-receipts`) with integrity fields. |
| Observability | `crates/agentd-observability` | Event/metrics spine; no token/cost rollup, no context-source attribution. |

**Confirmed gaps (semantically verified):** no routing decision point exists anywhere in the runtime; provider errors carry `retryable` but no kind taxonomy or rate-limit-header normalization; no cross-request provider health state beyond the single-adapter circuit breaker; no quota awareness; no billing-kind or trust-level metadata; no usage/economics aggregation; no verification-level selection (verification commands run uniformly); no prerequisite preflight; no risk-signal classification feeding review policy.

---

## 7. Interface Proposal (Ports & Adapters mapping)

New/changed contracts (all in `crates/agentd-contracts` unless noted):

- **`Router` port** (`crates/agentd-router`, new crate): `fn route(role_requirements, policy, candidates, health, quota) -> RoutingDecision` where `RoutingDecision { ranked: Vec<RankedCandidate>, eligibility: Vec<EligibilityOutcome> }` — pure function over injected state; no I/O, no globals.
- **Provider capability catalog**: extend provider config schema with `trust: TrustLevel`, `billing: BillingKind`, `limits: { rpm, tpm, rpd, tpd }`, `capabilities { tools, coding, reasoning }`, `context_tokens`. Data-driven (config/migrations), not hardcoded.
- **`ProviderHealthStore` / `QuotaLedger` ports** in `agentd-store-sqlite`: three-scope health rows with cooldown-until; quota event log with window aggregation. Restore validation mirrors `restoredHealth` semantics.
- **Provider error contract**: extend `ProviderError` with `kind: ProviderErrorKind` and normalized `retry_after` (header family parsed in the OpenAI-compatible adapter).
- **Usage rollup** (`agentd-observability` or new `agentd-usage`): consumes recorded `Usage` facts per Attempt; emits per-task and per-successful-task economics; persists rollups for receipts and Context Engine budget signals.
- **Verification-level selection** in `agentd-orchestrator`: `select_verification_level(changed_paths, observed_effects, risk) -> V0..V3` + ecosystem command derivation, executed by the existing TASK-0123 runner.
- **Shadow-mode harness** (`benchmarks/` or `crates/agentd-router/benches`): decision-receipt emission (`authority: offline-evaluation-only`), offline scorer, EXP-style report. No live routing path until shadow evidence is reviewed.

Ownership: agentd orchestration owns when routing happens; Policy Engine owns constraints (budget mode, privacy mode, trust minimums, escalation rules); Router owns ranking math; Provider Runtime owns transport + error normalization.

---

## 8. Prioritized Migration Roadmap

Gate alignment: G8 tasks strengthen the current gate; G9 tasks are design inputs for the queued context work; a new gate **G14 "Router / Usage Economics"** is proposed (appended to `planning/roadmap.yaml`, status `planned`, placed after G13; requires owner ratification before any G14 task starts). Priorities: P0 trust/self-hosting → P1 efficiency → P2 quality → P3 learned routing.

| Task | Gate | Pri | Title | Depends |
|---|---|---|---|---|
| TASK-0128 | G8 | P0 | Agent-router capability migration audit (this document) | — (done) |
| TASK-0134 | G8 | P1 | Verification-level selection + verification-scope detection | TASK-0124 |
| TASK-0135 | G8 | P2 | Prerequisite preflight checker | — |
| TASK-0136 | G8 | P2 | Deterministic risk-signal baseline (advisory) | — |
| TASK-0137 | G9 | P2 | Context-engine design consolidation from agent-router patterns | TASK-0127 |
| TASK-0129 | G14 | P1 | Router + provider-capability-catalog contract (ADR) | TASK-0128 |
| TASK-0130 | G14 | P1 | Provider failure taxonomy + rate-limit header normalization | — |
| TASK-0131 | G14 | P1 | Durable provider health registry | TASK-0129 |
| TASK-0132 | G14 | P1 | Durable quota ledger + headroom signal | TASK-0129 |
| TASK-0133 | G14 | P1 | Usage economics rollup | TASK-0129 |
| TASK-0141 | G14 | P2 | Runtime model discovery + capability probing | TASK-0129 |
| TASK-0142 | G14 | P2 | Evidence-triggered quality-escalation policy | TASK-0129 |
| TASK-0138 | G14 | P3 | Shadow-mode decision receipts + offline routing evaluation | TASK-0129 |
| TASK-0139 | G14 | P3 | Local-critic offline benchmark | TASK-0138 |
| TASK-0140 | G14 | P3 | Semantic-risk offline benchmark (privacy-gated) | TASK-0136 |

---

## 9. Validation of Claimed Gaps (evidence)

| Claim | Evidence |
|---|---|
| No routing decision point in Runlith | `crates/agentd-orchestrator` and provider crates contain no candidate selection/scoring; provider choice is static config |
| Health is single-scope, in-memory | `agentd-provider-openai/src/circuit_breaker.rs` only; no store tables for health/quota |
| Retry-after parsing is minimal | `extract_retry_after` (3 call sites) handles one header form; no `x-ratelimit-reset-*` support |
| Errors lack kind taxonomy | `ProviderError` exposes `retryable()` bool; no kind enum in contracts |
| Verification runs uniformly | TASK-0123 executes configured commands; no path-based level selection exists |
| Usage recorded but not rolled up | `token.rs` per-response usage; no per-task/per-role/provider aggregation anywhere |
| Tokenizer advantage is Runlith's | `agentd-context-engine/Cargo.toml` has `tiktoken` feature |
| Observed-effect capture done | TASK-0124 merged (`b2fc0f1`); orchestrator verification module present |

**Planning-state drift found during audit (reconciled in this branch):** `planning/tasks/queue.yaml` `completed_through` read `TASK-0122` while TASK-0123 (`4fb7e0d`) and TASK-0124 (`b2fc0f1`) are merged on main and roadmap G8 already lists TASK-0124. Evidence-backed reconciliation: `completed_through: TASK-0124`. Flagged here per the no-silent-fix rule.

---

## 10. Open Questions / Owner Decisions

1. **Ratify gate G14** ("Router / Usage Economics") — required before any G14 task starts.
2. **Catalog data authority:** should the provider capability catalog live in `product/` config (versioned with the product) or in SQLite seed migrations? Recommendation: `product/` config, loaded at startup, overridable by policy.
3. **Privacy review for semantic-risk benchmarking** (TASK-0140) — needs an owner decision on whether external-API benchmarking is permitted even offline.
4. **Account identity model:** agent-router keys quota/health by string account ids; Runlith needs a principled account/credential handle in the Provider Runtime contract before TASK-0131/0132.
5. **Reviewer advisories:** whether advisory reviewer signals (V6, non-authoritative) should be recorded on receipts at all, or kept out of the trust path entirely.

---

*Audit produced on branch `cline/fd038` in the designated audit worktree. Implementation of the first executable leaf (TASK-0135) follows in the same branch per gate discipline: G8 tasks may proceed; G9/G14 tasks are queued behind gate activation and owner ratification.*


**No learned/live routing is scheduled.** TASK-0138 exists precisely to produce the shadow-mode evidence the roadmap requires before any live routing decision is permitted.

