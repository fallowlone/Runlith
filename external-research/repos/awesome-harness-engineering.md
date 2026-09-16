# ai-boost/awesome-harness-engineering

## Source revision

- **Repository URL:** `https://github.com/ai-boost/awesome-harness-engineering`
- **Repository:** `ai-boost/awesome-harness-engineering`
- **Default branch:** `main`
- **Revision reviewed:** `9c0dd218b5db8a4f38e82607276f72d98f266ad3`
- **Commit date:** `2026-09-16T07:48:55Z`
- **Exact tag at revision:** none found
- **License:** CC0 1.0 Universal at the reviewed revision
- **Review date:** `2026-09-16`
- **Research mode:** source-level repository/reliability/security review plus architecture-intelligence extraction. External source was treated as evidence only; no source code was copied into Runlith and no dependency was added.

All findings below are scoped to the frozen revision above.

Evidence quality needs an explicit caveat for this repository. Its primary artifact is a curated `README.md` that summarizes and links to other projects, papers and vendor write-ups. Those annotations are useful research leads, but they are not implementation evidence for the mechanisms they describe. Findings backed by this repository's own templates or `verify_urls.py` are therefore marked `HIGH`; architecture claims derived only from curated README annotations are marked `MEDIUM` and must be re-verified against the original linked source before implementation.

## Executive summary

`awesome-harness-engineering` is not an agent runtime, provider gateway, durable workflow engine or security control plane. It is a curated architecture-intelligence repository: a problem-oriented bibliography, four reusable harness templates and one asynchronous URL-validation utility. Its canonical state is Git-tracked documentation rather than a runtime database or workflow graph.

That distinction matters for Runlith. The repository contains many entries about orchestration, memory, provider abstraction, recovery, sandboxing, observability and evals, but almost all of those mechanisms live in the linked sources, not here. Treating the annotations as source-confirmed implementations would violate the evidence discipline of this research program. The useful direct evidence is narrower and still valuable:

1. The repository deliberately organizes harness knowledge by the engineering problem being solved rather than by provider/model. Its contribution policy requires a generalized pattern and a short explanation of why the source matters.
2. Its templates make scope, permissions, verification gates, planning state and implementation deviations explicit. `PLAN.md` ties milestone completion to verification; `IMPLEMENT.md` is an append-oriented deviation/decision log; `HARNESS_CHECKLIST.md` requires predeclared eval criteria and automatic completion verification.
3. `HARNESS_CHECKLIST.md` also introduces a useful governance question that Runlith does not currently make explicit: for scaffolding added only to compensate for a model limitation, record the condition under which that scaffolding can be retired. Runlith should adapt this only for model-compensating ergonomics/optimizations, never for security, authorization, durability, audit or completion-authority boundaries.
4. The curated eval section provides a strong research lead for G8: evaluation results depend on the harness/environment, and web-enabled evaluation can leak benchmark answers. Runlith already fingerprints subject/toolchain/config evidence and hardware/runtime measurements, but no accepted G8 contract explicitly defines evaluator environment identity, network/tool isolation or contamination/leakage policy. That is the only new Implementation Knowledge Queue item from this review, and it is a `NEEDS_ADR` research/design candidate rather than an implementation task.

The repository's own verifier provides the strongest negative lessons. It imports `aiohttp` without a dependency manifest or lockfile, so the frozen checkout did not run in the review environment (`ModuleNotFoundError: aiohttp`). Its cache has no age/freshness field and indefinitely reuses cached `success`, `redirected` and `not_found` results. It also accepts every Markdown `http://`/`https://` URL and follows redirects with ordinary network access, with no loopback/link-local/private-network/per-hop destination policy. Because this repository contains no CI workflow that automatically runs the verifier on untrusted pull requests, the SSRF exploit path is classified `PLAUSIBLE`, not `CONFIRMED`; the network sink and absence of destination checks are source-confirmed.

No new runtime/provider/orchestration architecture should be adopted directly from this repository. Most valuable architectural annotations independently validate decisions Runlith already has: provider-neutral boundaries, deterministic Policy outside prompts, scoped network/secret authority, reviewer independence, durable state outside model context, verification as an independent authority, bounded retries, provenance-bearing context and observability.

## Project purpose

The repository describes itself as a curated list of resources, patterns and templates for reliable AI-agent harness engineering, with `README.md` as the primary artifact (`AGENTS.md:5-7`). Its contribution criteria require resources to address a specific harness problem, have enough substance to justify a reader's time, and generalize beyond one vendor even when the source itself is vendor-specific (`CONTRIBUTING.md:3-16`). Repository sections are explicitly organized by problem rather than provider/model (`AGENTS.md:9-26`; `README.md:100-102`).

The repository therefore acts as a research index and starter-kit library. It does not expose a product API, daemon, persistent agent service, provider session protocol or production deployment architecture.

## Architecture map

```text
Repository
├── Interfaces
│   ├── README.md — curated architecture/research index
│   ├── CONTRIBUTING.md — curation policy
│   ├── AGENTS.md — repository contribution instructions
│   └── templates/ — reusable harness planning/instruction/checklist artifacts
├── Orchestration
│   └── none; no runtime task/agent scheduler exists
├── Domain
│   ├── harness-problem taxonomy
│   ├── curation criteria
│   └── reusable review/planning conventions
├── State/Persistence
│   ├── Git-tracked Markdown is canonical repository state
│   └── url_verification_cache.json is local derived verification state
├── Providers
│   └── none; providers appear only as subjects of linked resources
├── Tools/Capabilities
│   └── verify_urls.py — asynchronous README link checker
├── Execution
│   └── bounded concurrent HTTP GET validation with redirects/retries
├── Recovery
│   ├── verifier retries request failures
│   └── cache avoids repeat requests; no durable workflow recovery exists
├── Security
│   ├── no authentication/authorization/runtime boundary exists
│   └── verifier has ambient outbound network authority from its host process
└── Observability
    ├── verifier terminal progress/summary
    └── optional JSON result/cache output
```

There are no package/workspace manifests, migrations, deployment files, GitHub Actions workflows or test suites at the reviewed revision. The only executable source file is `verify_urls.py`.

## Canonical state ownership

The canonical owner is the Git repository itself. `README.md`, contribution policy and templates are the durable source of truth. The project's own `AGENTS.md` explicitly names `README.md` as the primary artifact.

`url_verification_cache.json` is not canonical content. It is ignored by Git and reconstructed from URL checks. Unfortunately, the cache format carries status, final URL, error and response time but no observation timestamp, source revision or expiry, so its derived-state semantics are weaker than Runlith's freshness/provenance requirements.

There is no canonical runtime workflow state, Task/Execution/Attempt model, session store, explicit state machine, scheduler or reconciler. None should be inferred from bibliography entries about systems that have those components.

## Strong engineering decisions

### AHE-F001 — Problem-oriented, vendor-generalizing architecture curation

**finding_id:** `AHE-F001`

**source:** repository policy

**repository:** `ai-boost/awesome-harness-engineering`

**revision:** `9c0dd218b5db8a4f38e82607276f72d98f266ad3`

**file:** `AGENTS.md`; `CONTRIBUTING.md`; `README.md`

**symbol:** curation criteria / section taxonomy

**region:** `AGENTS.md:5-33`; `CONTRIBUTING.md:3-16`; `README.md:100-102`

**observation:** The repository organizes material around context, tools, planning, permissions, memory, verification and orchestration problems, not around vendors. A provider-specific source belongs only when its pattern generalizes.

**confidence:** `HIGH`

**Problem:** Agent engineering research is easy to organize by vendor/product and then accidentally encode vendor APIs as architecture.

**Mechanism:** Curate by problem class and require each entry to explain transferable value.

**Why it works:** It forces the reader to compare mechanisms that solve the same problem and makes provider-specific implementation choices easier to distinguish from general contracts.

**Limitations:** The annotations remain editorial summaries. A well-written entry can overstate evidence from the linked project unless the original source is audited.

**Failure modes:** link drift, outdated summaries, curation bias, or treating popularity/annotation wording as proof.

**Complexity:** low operational complexity; sustained editorial maintenance is the main cost.

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith already treats providers as replaceable executors and keeps provider-specific semantics behind adapters/anti-corruption boundaries.

**Decision:** `EXTERNAL_VALIDATION`.

**Differentiation:** Runlith's research process goes further by binding adoption to frozen revisions, source evidence, overlap analysis, explicit authority boundaries and differentiation tests rather than treating a curated entry as enough to implement.

### AHE-F002 — Planning artifacts bind progress to scope and verification

**finding_id:** `AHE-F002`

**source:** repository templates

**file:** `templates/PLAN.md`; `templates/IMPLEMENT.md`; `templates/HARNESS_CHECKLIST.md`

**symbol:** milestone, deviation log and verification sections

**region:** `PLAN.md:3-47`; `IMPLEMENT.md:3-52`; `HARNESS_CHECKLIST.md:29-48`

**observation:** Non-trivial work gets an explicit plan with scope boundaries and milestone verification. A milestone is not marked complete before its gate passes. Implementation deviations and decisions are recorded during execution. Eval criteria are expected before task execution, and completion verification should run automatically rather than only at PR time.

**confidence:** `HIGH`

**Problem:** Long-running agent work loses scope/decision history and can redefine success after seeing its own output.

**Mechanism:** persistent planning artifact, append-oriented implementation/deviation log, predeclared verification, and completion-time checks.

**Why it works:** It makes plan drift and evidence requirements inspectable across sessions instead of leaving them only in model context.

**Limitations:** Markdown files alone are weak concurrency/state machines and can be edited by the same agent whose work they are meant to govern.

**Failure modes:** self-marked completion, conflicting writers, stale plan state, or treating a file checkbox as trusted completion authority.

**Complexity:** low for a single-agent repository; insufficient by itself for durable multi-agent coordination.

**Runlith overlap:** `SUPERSEDED` for canonical lifecycle/state, because Runlith has a durable Task Graph, versioned Plan revisions, Event Journal, independent Evaluation and trusted application-service transitions. The template remains useful external validation of the human-readable projection/compatibility layer.

**Decision:** `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` for generated human-readable views.

**Differentiation:** Runlith separates plan proposal from authority, persists canonical graph/revision state transactionally, preserves restart semantics, and lets Markdown remain a view/artifact rather than the concurrency primitive.

### AHE-F003 — Model-compensating scaffolding has an explicit retirement condition

**finding_id:** `AHE-F003`

**source:** repository template

**file:** `templates/HARNESS_CHECKLIST.md`

**symbol:** `When this harness component should be removed`

**region:** `50-57`

**observation:** The checklist asks reviewers to record why a harness component exists and what capability improvement would make it unnecessary.

**confidence:** `HIGH`

**Problem:** Harnesses accumulate model-specific compensating logic long after its original limitation disappears, increasing context, latency and maintenance cost.

**Mechanism:** Every compensating component carries an explicit existence rationale and retirement trigger.

**Why it works:** Capability improvements become an opportunity to delete obsolete scaffolding instead of layering new work on top indefinitely.

**Limitations:** The template statement is too broad if applied literally. Many harness components exist because deterministic security, durability, authorization, audit, privacy or verification must remain outside the model regardless of model capability.

**Failure modes:** deleting a structural safety/correctness boundary because a newer model appears more capable; retirement based on anecdote rather than measured non-regression.

**Complexity:** low as governance metadata; the hard part is proving the retirement condition.

**Runlith overlap:** no explicit general decommission criterion for model-compensating scaffolding was found. This is a `NOVEL_GAP` in engineering governance, not a missing runtime subsystem.

**Decision:** `ADAPT`.

**Differentiation:** Runlith should restrict the pattern to optimizations, prompts, compatibility shims, model-specific workaround layers and other replaceable compensating mechanisms. Policy, Runner isolation, completion authority, provenance, audit, durability and privacy invariants are architecture boundaries and cannot be retired merely because a model benchmark improves.

No implementation-queue item is created because this is process/governance guidance rather than a product capability gap.

### AHE-F004 — Reviewer independence should influence context inheritance

**finding_id:** `AHE-F004`

**source:** curated README annotation; linked mechanism not independently audited in this review

**file:** `README.md`

**symbol:** Context Delivery & Compaction / `Organizing Context in a Multi-Agent Harness`

**region:** `175`

**observation:** The annotation distinguishes fresh isolated subagent context from context inherited from a supervisor and recommends isolation for verifiers whose independent judgment matters.

**confidence:** `MEDIUM`

**Runlith overlap:** `ALREADY_DESIGNED`. `AGENT.md` already requires isolated subagent context/minimum permissions; ADR-0018 separates implementation from independent Evaluation; G9 design requires isolated child contexts with bounded evidence refs.

**Decision:** `EXTERNAL_VALIDATION` for isolated reviewers. Wholesale parent-context inheritance is `INSPIRED_ONLY`; Runlith should pass explicit execution/evidence refs or continuity state where needed rather than inheriting an unbounded conversation by default.

**Differentiation:** reviewer independence remains a trusted workflow property and context provenance is observable/rebuildable, not a provider-specific conversation-copy trick.

### AHE-F005 — Deterministic checks precede expensive subjective evaluation

**finding_id:** `AHE-F005`

**source:** curated README annotation

**file:** `README.md`

**symbol:** Verification & CI Integration

**region:** `333-348`

**observation:** The curated section emphasizes deterministic trace/outcome checks where possible and LLM-as-judge only where it reduces meaningful residual risk. It also distinguishes capability/improvement evaluation from near-100% regression protection.

**confidence:** `MEDIUM`

**Runlith overlap:** `ALREADY_DESIGNED`. ADR-0018 makes Evaluation independent and accepts tests, lint, typecheck, security, benchmarks, review and human approval; Source of Truth states deterministic verification outranks subjective judging when available.

**Decision:** `EXTERNAL_VALIDATION`.

### AHE-F006 — Evaluation result identity includes harness/environment, and leakage is an isolation concern

**finding_id:** `AHE-F006`

**source:** curated README annotations; original linked empirical sources were not independently re-audited here

**file:** `README.md`

**symbol:** Evals & Verification

**region:** `565-575`

**observation:** The curated material treats container resource configuration as capable of materially changing agent behavior, argues that capability should be reported for a model-harness pairing rather than a base model alone, and calls out web-enabled benchmark evaluation as vulnerable to answer-key/research leakage.

**confidence:** `MEDIUM`

**Problem:** Two nominally identical model evaluations can measure different systems when runtime resources, tool access, network access, harness instructions or hidden answer availability differ. A successful score may also be invalid if the agent obtains benchmark answers rather than solving the task.

**Mechanism suggested by the sources:** bind evaluation to the harness/environment configuration and isolate/limit channels that reveal benchmark answers or hidden acceptance evidence.

**Runlith overlap:** `NEEDS_ADR`, narrowly. Runlith already has hardware/runtime fingerprints, benchmark provenance, versioned RoutePlan inputs and G8 verification evidence tied to subject/toolchain/config fingerprints. Targeted search did not find an accepted G8 contract for evaluator-environment identity, evaluator network/tool policy, held-out evidence isolation or contamination classification.

**Actual gap:** define which evaluation-environment properties are evidence identity versus incidental telemetry, how network/tool/fixture access is controlled for benchmark/held-out verification, and how a result becomes invalid/tainted when contamination is detected.

**Decision:** `ADAPT` as a G8 research/ADR candidate; do not copy any specific benchmark harness.

**Differentiation:** Runlith can make this provider-neutral and durable: evaluator environment fingerprint + Policy/Runner profile + evidence/fixture provenance + explicit contamination status, while keeping completion authority in the trusted Evaluation path.

### AHE-F007 — Capability risk can emerge from combinations, not individual tools

**finding_id:** `AHE-F007`

**source:** curated README annotation

**file:** `README.md`

**symbol:** Tool Design / risk vocabulary

**region:** `177-190`, especially `182`

**observation:** The annotation treats tool metadata as policy input rather than enforcement and highlights the combination of sensitive/private data access, untrusted input and external communication as a compound risk.

**confidence:** `MEDIUM`

**Runlith overlap:** `ALREADY_DESIGNED` at the architecture level. Runlith has typed multi-capability `PolicyRequest`s, atomic composition, scoped network/secret/filesystem authority, data-flow constraints, a Data Flow Inspector direction, per-hop SSRF policy and untrusted-content invariants. The current threat model already tracks secret/network exfiltration and private-network access.

**Decision:** `EXTERNAL_VALIDATION`. No new queue item is created.

**Differentiation:** Runlith's useful improvement over advisory tool hints is deterministic typed Policy + operation-bound enforcement + explicit data-flow/sensitivity provenance.

### AHE-F008 — Harness configuration that controls authority must not be self-modifiable authority

**finding_id:** `AHE-F008`

**source:** curated README annotation

**file:** `README.md`

**symbol:** Security, Sandbox & Permissions

**region:** `520-540`, especially the annotation at `528`

**observation:** The curated security guidance warns that an agent able to edit the configuration/hooks that define its own security controls can escalate itself even when process sandboxing otherwise exists.

**confidence:** `MEDIUM`

**Runlith overlap:** `ALREADY_DESIGNED` / `SUPERSEDED`. Repository instructions/provider-native permissions cannot grant canonical Runlith authority; Policy layers and trusted enforcement own it. `AGENTS.md`, provider instruction files and similar harness files are derived compatibility views rather than independent security roots.

**Decision:** `EXTERNAL_VALIDATION`.

### AHE-F009 — The link checker bounds concurrency and per-host pressure

**finding_id:** `AHE-F009`

**source:** external implementation

**file:** `verify_urls.py`

**symbol:** `URLValidator.__init__`, `check_all`, `check_one`

**region:** `48-58,112-161`

**observation:** The checker uses an async semaphore, a connector-wide concurrency bound and a lower per-host connection bound. Requests have total timeouts and bounded retry counts.

**confidence:** `HIGH`

**Problem:** A repository containing hundreds of links should not create unbounded concurrent outbound requests.

**Mechanism:** process-level and per-host bounds plus timeouts/retries.

**Why it works:** resource use and target pressure are capped.

**Limitations:** retry classification is coarse; all non-200/non-404 statuses are retried until exhaustion, with a small linear delay. Destination authority and cache freshness are not addressed by concurrency controls.

**Runlith overlap:** `ALREADY_DESIGNED` through bounded web acquisition/retry budgets and explicit network policy.

**Decision:** `EXTERNAL_VALIDATION` only.

## Implementation techniques

The repository's own implementation surface is small. The useful direct techniques are therefore concentrated in its templates and URL checker:

- explicit allowed/restricted/forbidden tool scope in `templates/AGENTS.md:45-76`;
- task milestones coupled to verification in `templates/PLAN.md:18-25`;
- append-oriented implementation decisions/deviations in `templates/IMPLEMENT.md:12-52`;
- predeclared eval criteria and automatic completion checks in `templates/HARNESS_CHECKLIST.md:43-48`;
- bounded async URL checking with per-host limits in `verify_urls.py:112-161`;
- local JSON result materialization in `verify_urls.py:192-240`.

For Runlith these should be treated as presentation/process patterns. Canonical task state, authority, recovery and verification remain typed durable domain/application concerns rather than Markdown conventions.

## AI / orchestration architecture

This repository does not implement an AI agent loop or orchestration runtime.

| Question | Reviewed repository |
|---|---|
| Who owns workflow state? | No workflow state exists. Git-tracked research Markdown owns repository content. |
| Who owns completion authority? | No task-completion runtime exists. Template checkboxes are human/agent process scaffolding only. |
| Can the model mutate canonical runtime state? | No runtime exists. A contributing agent can edit repository Markdown under ordinary Git permissions. |
| Can model output directly cause privileged action? | Not through repository code. The standalone URL checker causes outbound GETs based on Markdown URLs when a user invokes it. |
| What happens after crash? | No workflow recovery. A later verifier invocation can reuse its local JSON cache if present. |
| What happens after provider timeout? | No providers are invoked. HTTP URL timeouts are retried a bounded number of times. |
| How does execution resume? | No durable execution identity/checkpoint. Re-run the script; cached non-error results may be reused indefinitely. |

The README links to many real orchestration systems, but those systems require separate frozen-revision source reviews before their mechanisms can enter Runlith architecture decisions.

## Security analysis

### AHE-S001 — Repository-controlled URLs can drive unrestricted outbound link-checker requests

**classification:** `PLAUSIBLE`

**finding_id:** `AHE-S001`

**source:** external implementation + contribution path

**file:** `verify_urls.py`; `CONTRIBUTING.md`

**symbol:** `extract_urls`, `check_one`, `check_all`; README contribution format

**region:** `verify_urls.py:60-70,112-150`; `CONTRIBUTING.md:18-23`

**observation:** Contributions add arbitrary resource URLs to `README.md`. The checker accepts every Markdown destination beginning with `http://` or `https://`, performs a normal HTTP GET and follows redirects automatically. No code checks normalized hostnames/resolved addresses, loopback, link-local, private/internal ranges, redirect destinations or an allowlist.

**Why `PLAUSIBLE` instead of `CONFIRMED`:** the network sink and missing destination policy are confirmed. However, this revision has no checked-in CI workflow that automatically executes the checker on untrusted pull-request content. Exploitation as SSRF therefore depends on a maintainer/automation context invoking the script over attacker-controlled README content while having sensitive internal network reachability.

**Potential impact:** requests to local/private/link-local/internal destinations available from the validator host; redirect-based reachability expansion; dependence on where the script is run.

**Runlith lesson:** external/repository URLs are untrusted evidence. Runlith already requires centralized per-hop SSRF/network policy (`INV-097`), resolved-IP checks, redirect reevaluation and restricted web workers. This finding is `EXTERNAL_VALIDATION`, not a new gap.

### AHE-S002 — Shell/argument injection

**classification:** `NOT_SUPPORTED`

No subprocess/shell command execution path exists in the reviewed executable. URLs are passed to `aiohttp`, not interpolated into a shell command. The repository therefore provides no source-supported shell-injection finding at this revision.

### AHE-S003 — Authorization/IPC/sandbox bypass

**classification:** `NOT_SUPPORTED`

The repository has no privileged local API, IPC service, authentication system, agent process launcher or sandbox boundary. Those vulnerability classes do not apply to its implementation surface.

### AHE-S004 — Unsafe deserialization / secret persistence

**classification:** `NOT_SUPPORTED`

The checker loads JSON into a fixed local dataclass representation and does not execute serialized content. No credentials/secrets store or provider token persistence exists in this repository.

## Reliability / recovery analysis

### AHE-R001 — Verification cache has no freshness semantics

**finding_id:** `AHE-R001`

**source:** implementation

**file:** `verify_urls.py`

**symbol:** `load_cache`, `split_urls`, `save_json`

**region:** `72-110,192-198,225-240`

**observation:** Cached records have URL, status, status code, final URL, error and response time but no check timestamp, expiry, source revision or validator version. `split_urls` rechecks only cached `ERROR`/`TIMEOUT`; cached `SUCCESS`, `REDIRECTED` and `NOT_FOUND` are reused with no age bound.

**confidence:** `HIGH`

**Immediate impact:** `verify all URLs` can report an arbitrarily old success after a link dies, or preserve an arbitrarily old 404 after a resource is restored.

**Long-term impact:** verification evidence becomes detached from the current repository/review time; maintainers can trust stale results because the command still prints a fresh summary.

**Runlith lesson:** derived validation/cache evidence needs freshness, source/config identity and explicit revalidation policy. Runlith already designs context/web snapshots with provenance/freshness and treats caches as rebuildable derived state.

### AHE-R002 — The declared verifier is not reproducibly executable from repository metadata

**finding_id:** `AHE-R002`

**source:** implementation + repository structure + direct verification attempt

**file:** `verify_urls.py`; repository root

**symbol:** Python import/runtime environment

**region:** `verify_urls.py:18-25`; no `requirements*.txt`, `pyproject.toml`, lockfile or equivalent manifest exists at the reviewed revision

**observation:** The script imports third-party `aiohttp`, but the repository provides no dependency manifest/lock or setup command. A clean execution attempt against the frozen checkout failed before URL parsing with `ModuleNotFoundError: No module named 'aiohttp'`.

**confidence:** `HIGH`

**Immediate impact:** the repository's own `AGENTS.md` says all URLs must be reachable before a PR, but a new reviewer/CI environment cannot derive the validator dependency from repository metadata.

**Long-term impact:** link verification becomes operator-environment dependent and cannot serve as reproducible gate evidence.

**Runlith lesson:** a verification tool is part of the evidence-producing environment. Dependencies/tool versions must be reproducible or explicitly fingerprinted; a documented command that may not run is not a trustworthy gate.

### AHE-R003 — Retry policy does not classify HTTP failure semantics

**finding_id:** `AHE-R003`

**source:** implementation

**file:** `verify_urls.py`

**symbol:** `check_one`

**region:** `112-144`

**observation:** `200` and `404` terminate immediately; every other HTTP status is retried until the configured retry limit, using a small linear delay. The same policy therefore covers authentication failures, access denials, rate limits and server failures.

**confidence:** `HIGH`

**Impact:** avoidable traffic and weak semantics around 401/403/429/5xx; no server-directed retry timing or typed non-retryable classification.

**Runlith overlap:** `ALREADY_DESIGNED`. Web acquisition already specifies typed bounded retry classes, budgets, target policy and idempotency/session awareness.

## Performance / scalability analysis

The verifier has a sensible local concurrency cap: one semaphore controls active URL checks and `aiohttp.TCPConnector` additionally caps per-host connections (`verify_urls.py:48-58,147-150`). It schedules one coroutine per URL but executes only the bounded set concurrently. This is adequate for the current README scale.

Its cache is intended to reduce repeated network work, but the lack of expiry makes the performance optimization semantically unsafe as verification evidence. Runlith's relevant lesson is the one already encoded by its efficiency architecture: an optimization cannot be promoted if it silently weakens quality/correctness evidence.

The repository itself scales editorially rather than computationally. Its primary risk is knowledge drift: hundreds of external links and short annotations can age independently of the sources they summarize. There is no source-revision pinning for linked repositories and no automation that detects whether an annotation remains true after the upstream project changes.

## Anti-patterns

### AHE-AP001 — Derived verification cache without time/revision freshness

**Evidence:** `verify_urls.py:72-110,192-198`.

**Immediate impact:** old success/redirect/404 results are accepted as if current.

**Long-term impact:** the cache gradually becomes an alternative stale truth about the external web.

**Runlith lesson:** every reusable verification snapshot needs provenance/freshness/invalidation semantics; caches never become truth.

**Existing Runlith protection:** Context/Web snapshot provenance and freshness, `INV-070`, `INV-103`, evidence fingerprinting.

### AHE-AP002 — Untrusted repository URLs become ambient validator network authority

**Evidence:** contribution URLs enter `README.md` (`CONTRIBUTING.md:18-23`); `verify_urls.py:60-70,112-150` accepts them and follows redirects with no network policy.

**Immediate impact:** the network destinations reachable by the validator process define the effective URL-check authority.

**Long-term impact:** adding CI/internal execution later could silently turn harmless curation input into an SSRF boundary without changing the checker itself.

**Runlith lesson:** URL syntax validation is not network authorization; enforce scheme/host/resolved-IP/redirect policy at every hop.

**Existing Runlith protection:** `INV-097`, `THR-007`, centralized `NetworkAccessPolicy`, Runner/network capability separation.

### AHE-AP003 — Verification gate depends on undeclared local tooling

**Evidence:** `verify_urls.py:20` imports `aiohttp`; no Python dependency manifest/lock or CI workflow exists; direct execution of the frozen revision failed with missing `aiohttp`.

**Immediate impact:** the supposedly standard verification command is environment-dependent.

**Long-term impact:** contributors can satisfy/skip the gate inconsistently and evidence cannot be reproduced later.

**Runlith lesson:** evidence-producing tools and their environment are part of the verification contract and need versioned/reproducible provenance.

**Existing Runlith protection:** G8 design already binds verification to toolchain/config fingerprints; the remaining evaluator-environment contract is the novel gap captured below.

## Runlith overlap analysis

### External idea: problem-oriented/provider-neutral harness taxonomy

**External evidence:** `AGENTS.md:9-33`; `CONTRIBUTING.md:3-16`; `README.md:100-102`.

**Closest Runlith equivalent:** provider-neutral domain boundaries and replaceable provider/runner/tool adapters.

**Runlith evidence:** `docs/architecture/providers.md`; `docs/architecture/INVARIANTS.md`; Source of Truth provider hierarchy/adapter rules.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

### External idea: durable planning/progress outside the model context

**External evidence:** `templates/PLAN.md:3-47`; `templates/IMPLEMENT.md:3-52`; `HARNESS_CHECKLIST.md:22-34`.

**Closest Runlith equivalent:** durable Task Graph, versioned PlanRevision, Event Journal, Context/Continuity separation.

**Runlith evidence:** ADR-0017/ADR-0027; `docs/architecture/task-engine.md`; `docs/architecture/context-engine.md`; continuity contracts.

**Current status:** `SUPERSEDED` as canonical state; template form may remain a derived view.

**Actual gap:** none.

### External idea: reviewer context isolation

**External evidence:** `README.md:175` (secondary annotation).

**Closest Runlith equivalent:** isolated subagent contexts + independent Evaluation/Review.

**Runlith evidence:** `AGENT.md:152`; ADR-0018; Source of Truth G9 child-context isolation.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

### External idea: tool metadata is policy input, not authority; risk is compositional

**External evidence:** `README.md:182` (secondary annotation).

**Closest Runlith equivalent:** typed `Capability`/`PolicyRequest`, atomic multi-capability policy, data-flow constraints, scoped secrets/network, SSRF enforcement.

**Runlith evidence:** ADR-0026; `docs/security/POLICY_CONTRACT.md`; `docs/security/THREAT_MODEL.md`; Source of Truth Data Flow Inspector/Web policy.

**Current status:** `ALREADY_DESIGNED`, with core G6 portions already implemented.

**Actual gap:** no separate capability-combination task justified by this evidence.

### External idea: automatic completion verification with predeclared criteria

**External evidence:** `templates/HARNESS_CHECKLIST.md:43-48`; `templates/PLAN.md:18-25`.

**Closest Runlith equivalent:** independent Evaluation is sole completion authority; Task owns acceptance/required-verification references.

**Runlith evidence:** ADR-0018; `docs/architecture/task-engine.md:185-241,286-312,344-350`.

**Current status:** `ALREADY_DESIGNED` and partially implemented through the existing evaluation seam; full G8 remains future scope.

**Actual gap:** no new completion-authority concept.

### External idea: model-harness/evaluation environment is part of benchmark identity

**External evidence:** `README.md:565-575` (secondary annotations).

**Closest Runlith equivalent:** `ModelRuntimeProfile`, hardware/runtime fingerprint, benchmark provenance, G8 subject/toolchain/config fingerprints.

**Runlith evidence:** Source of Truth Adaptive Model Router (`ModelRuntimeProfile`, evidence provenance), `INV-062`, `INV-080`, G8 adoption note binding verification evidence to subject/toolchain/config fingerprints.

**Current status:** `NEEDS_ADR` for the narrower evaluator-environment and contamination-control contract.

**Actual gap:** evaluator network/tool/fixture access policy, evaluation-environment identity, contamination/taint semantics and whether environment differences invalidate result comparability are not accepted/frozen.

### External idea: web/URL requests require explicit network authority

**External evidence:** negative evidence from `verify_urls.py:60-70,112-150`.

**Closest Runlith equivalent:** centralized network/SSRF policy for every acquisition and redirect.

**Runlith evidence:** `INV-097`; `THR-007`; Source of Truth `NetworkAccessPolicy` rules around scheme/host/resolved IP/redirect/private range.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

## External validation of existing Runlith design

This repository gives additional, though often secondary, support for several Runlith choices:

- provider/model should remain an interchangeable executor under a harness-owned control plane rather than the owner of durable business state;
- persistent plans/decisions/progress must outlive prompt context;
- independent reviewers/verifiers should not inherit an implementation agent's full conversational framing by default;
- verification criteria belong before completion and deterministic checks should be preferred where available;
- Policy/authorization must be deterministic harness logic rather than natural-language instructions;
- network, filesystem and secret access are scoped capabilities, and compound data-flow risk matters;
- security configuration cannot safely be a self-authorizing file controlled by the same untrusted agent;
- observability/evaluation must include harness/environment configuration rather than attributing all behavior to the base model;
- web acquisition requires an explicit SSRF/network boundary on every redirect/resolution hop;
- derived caches/snapshots need freshness and cannot become canonical truth.

These validations do not justify copying linked implementations. Runlith already has stronger ownership, durability, policy and evidence semantics than the repository's generic templates.

## Novel gaps

### NG-001 — Evaluation environment identity and contamination/leakage policy

**Status:** `NEEDS_ADR`

Runlith already records rich subject/toolchain/config and runtime/hardware provenance, but the planned G8 Evaluation layer does not yet have an accepted rule for what evaluator environment is part of evidence identity or how held-out/benchmark contamination is prevented and represented.

The gap is intentionally narrow:

- record the evaluator/verification runner identity/profile needed to reproduce the result;
- bind relevant sandbox/network/tool/resource limits to evidence provenance;
- specify when evaluator or target-agent network access is forbidden/limited because it can reveal benchmark/held-out answers;
- bind hidden fixtures/answer artifacts so the implementation model cannot access them unless the verification design explicitly permits it;
- represent detected contamination/benchmark leakage as invalid/tainted evidence rather than a normal pass;
- keep these controls under Policy/Runner/Evaluation, not model prompt instructions;
- avoid making every normal repository test “secret”; isolation level is task/eval-policy dependent.

The external repository supplies only secondary evidence for this gap, so the next step is research/ADR work, not code.

## Adoption matrix

| External idea / finding | Runlith status | Decision | Reason |
|---|---|---|---|
| Problem-oriented provider-neutral curation | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Matches provider-neutral architecture/intelligence process. |
| Markdown PLAN/IMPLEMENT as canonical workflow state | `SUPERSEDED` | `INSPIRED_ONLY` | Useful human-readable artifacts; Runlith canonical state is durable typed domain state. |
| Predeclared verification + verify-before-complete | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | ADR-0018 already makes Evaluation completion authority. |
| Role-sensitive reviewer context isolation | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Runlith already isolates subagents/reviewers and supplies bounded evidence. |
| Component retirement criterion | `NOVEL_GAP` (governance only) | `ADAPT` | Apply to model-compensating scaffolding, never safety/durability invariants. |
| Compound capability/data-flow risk | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Typed capabilities + Policy + data-flow/SSRF architecture already cover the principle. |
| Harness/evaluator environment as result provenance | `NEEDS_ADR` | `ADAPT` | Existing fingerprints stop short of explicit evaluator-environment/contamination policy. |
| Web-enabled eval answer leakage | `NEEDS_ADR` | `ADAPT` | Requires G8 policy/research, not a blanket network ban. |
| URL checker cache semantics | `ALREADY_DESIGNED` protection | `REJECT` behavior | Runlith requires freshness/provenance for derived snapshots. |
| Ambient URL checker network authority | `ALREADY_DESIGNED` protection | `REJECT` behavior | Conflicts with Runlith per-hop network/SSRF Policy. |
| Undeclared verification runtime dependency | `PARTIALLY_EXISTS` protection | `REJECT` behavior | G8 evidence must be reproducible/fingerprinted; specific evaluator env contract remains open. |

## Ideas rejected for Runlith

### Use README annotations as sufficient implementation evidence

**Decision:** `REJECT`.

The repository is a bibliography. Linked claims must be source-audited at a frozen revision before they justify concrete architecture or implementation.

### Store canonical long-running workflow state in PLAN.md / IMPLEMENT.md

**Decision:** `REJECT` as canonical state; `INSPIRED_ONLY` as projection.

Runlith requires transactional, restart-safe, concurrent durable state and independent completion authority. Human-readable files can be generated views/artifacts.

### Remove every harness component when models improve

**Decision:** `REJECT` literally; `ADAPT` narrowly.

Security, policy, isolation, audit, provenance, durability and independent verification exist because deterministic trust boundaries are required, not merely because the model is currently weak.

### Use cached link status without expiry/source identity

**Decision:** `REJECT`.

Performance optimization cannot silently weaken verification freshness.

### Let arbitrary repository URLs inherit validator host network reachability

**Decision:** `REJECT`.

Runlith keeps network authority typed/scoped and checks destination/redirect resolution through Policy.

## Candidate ADRs

### Candidate: Evaluation Environment Identity and Contamination Control

**Type:** `ADR`

**Problem:** G8 needs a reproducible, provider-neutral answer to which evaluator-environment properties are part of trusted evidence identity and how benchmark/held-out answer leakage invalidates verification.

**Current Runlith state:** subject/toolchain/config fingerprints, runtime/hardware provenance, independent Evaluation and Policy/Runner isolation are designed; evaluator environment/network/fixture contamination semantics are not frozen.

**External inspiration:** curated eval annotations at `README.md:565-575`; direct negative evidence from this repository's own non-reproducible link checker reinforces that a verification tool's runtime environment is part of the evidence chain.

**Actual delta:** define evaluation runner/environment fingerprint fields, tool/network/isolation constraints, hidden-fixture visibility rules, contamination/taint outcome, and comparability/replay requirements.

**Target subsystem:** G8 Evaluation/Verification + Runner/Policy + evidence provenance.

**Dependencies:** G8 activation; accepted required-verification taxonomy; existing Runner/Policy capability model; evidence/config fingerprint contracts.

**Architecture impact:** moderate but bounded to evaluation evidence and execution environment; no Task/provider authority change.

**Security impact:** prevents privileged/network-enabled evaluators or target agents from silently obtaining held-out answers and prevents contaminated passes from becoming completion evidence.

**Acceptance direction:** a verification result can explain exact subject/toolchain/evaluator/profile/network/isolation/fixture versions relevant to reproducibility; contaminated/forbidden evidence cannot produce VERIFIED.

**Verification direction:** deterministic fixtures demonstrate equivalent environment fingerprints for equivalent runs, changed relevant environment produces changed evidence identity, forbidden network/fixture access fails closed, and allowed network-enabled integration tests remain representable without pretending they are isolated benchmarks.

**Suggested gate:** G8 before broad benchmark/evaluator execution becomes completion-authoritative.

**Priority:** `MEDIUM now; HIGH at G8 activation`.

## Candidate implementation tasks

### Candidate: G8 evaluation-environment/contamination research spike

**Type:** `RESEARCH`

**candidate_task:** `TASK-CANDIDATE`

**Problem:** The candidate ADR needs empirical and implementation-neutral requirements before code/schema design.

**Current Runlith state:** no accepted evaluator-environment contract found; G8 is future gate and canonical queue is intentionally unchanged by this research.

**External inspiration:** `awesome-harness-engineering` eval annotations and this repository's verifier reproducibility failure.

**Actual delta:** compare representative coding/eval harnesses on environment fingerprints, held-out test/answer isolation, network policy, resource limits and contamination outcomes; derive the smallest provider-neutral contract compatible with Runlith Policy/Runner/Evaluation ownership.

**Target subsystem:** G8 architecture/evaluation research.

**Dependencies:** none for research; implementation waits for accepted ADR/G8 task allocation.

**Architecture impact:** research only until accepted.

**Security impact:** positive if it prevents invalid/contaminated evidence from gaining completion authority.

**Acceptance direction:** research produces an evidence-backed ADR recommendation and identifies which properties are required for evidence identity versus ordinary telemetry.

**Verification direction:** use controlled evaluation fixtures with intentionally varied network/resource/tool/hidden-evidence conditions; confirm the proposed provenance model distinguishes materially different evaluation environments without overfitting to one provider.

**Suggested gate:** pre-G8 / G8 architecture activation.

**Priority:** `MEDIUM`.

No implementation task is proposed for the component-retirement checklist idea because it is governance/process guidance, and no task is proposed for capability-combination policy because targeted overlap found that Runlith already has the architectural mechanisms required by the principle.

## Source evidence index

| Finding | External source evidence | Evidence quality |
|---|---|---|
| `AHE-F001` | `AGENTS.md:5-33`; `CONTRIBUTING.md:3-16`; `README.md:100-102` | direct repository policy, `HIGH` |
| `AHE-F002` | `templates/PLAN.md:3-47`; `templates/IMPLEMENT.md:3-52`; `templates/HARNESS_CHECKLIST.md:29-48` | direct templates, `HIGH` |
| `AHE-F003` | `templates/HARNESS_CHECKLIST.md:50-57` | direct template, `HIGH` |
| `AHE-F004` | `README.md:175` | curated secondary annotation, `MEDIUM` |
| `AHE-F005` | `README.md:333-348` | curated secondary annotations, `MEDIUM` |
| `AHE-F006` | `README.md:565-575` | curated secondary annotations, `MEDIUM` |
| `AHE-F007` | `README.md:177-190` | curated secondary annotation, `MEDIUM` |
| `AHE-F008` | `README.md:520-540` | curated secondary annotation, `MEDIUM` |
| `AHE-F009` | `verify_urls.py:48-58,112-161` | direct implementation, `HIGH` |
| `AHE-S001` | `CONTRIBUTING.md:18-23`; `verify_urls.py:60-70,112-150` | direct implementation + input path, exploit context conditional |
| `AHE-R001` | `verify_urls.py:72-110,192-198,225-240` | direct implementation, `HIGH` |
| `AHE-R002` | `verify_urls.py:18-25`; root file set; direct execution failure on frozen checkout | direct implementation/environment verification, `HIGH` |
| `AHE-R003` | `verify_urls.py:112-144` | direct implementation, `HIGH` |

### Frozen repository inventory relevant to negative evidence

The reviewed repository has the following implementation/configuration-relevant files at top level: `.gitignore`, `AGENTS.md`, `CONTRIBUTING.md`, `LICENSE`, `README.md`, `verify_urls.py`, `assets/banner.jpg`, and four files under `templates/`. No Python dependency manifest/lock, CI workflow, test directory, migration, deployment manifest or package/workspace manifest was present.

### Runlith evidence used for overlap decisions

- `AGENT.md` — authority order, subagent isolation, independent review, provider/session boundaries.
- `docs/architecture/INVARIANTS.md` — provider neutrality, context derivation, SSRF/web, security authority, benchmark provenance.
- `docs/architecture/context-engine.md` — provenance/freshness, context isolation, derived model context.
- `docs/architecture/task-engine.md` — independent Evaluation and completion authority.
- `docs/architecture/adr/ADR-0018-independent-evaluation.md` — trusted Evaluation boundary.
- `docs/architecture/adr/ADR-0026-g6-security-authority-and-capability-contracts.md` — typed Policy/Capability model and enforcement.
- `docs/security/POLICY_CONTRACT.md` — atomic capability evaluation, exact approvals, enforcement points.
- `docs/security/THREAT_MODEL.md` — untrusted repository/web/tool content, SSRF/private-network and secret/network threats.
- `AI_ENGINEERING_OS_ARCHITECTURE_SOURCE_OF_TRUTH.md` — Adaptive Model Router provenance, G8 verification fingerprint direction, Web/NetworkAccessPolicy, G9 context isolation, data-flow architecture.
- `planning/roadmap.yaml` and `planning/tasks/queue.yaml` — read-only overlap search; neither file is modified by this research.
