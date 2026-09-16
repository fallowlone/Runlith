# SnailSploit/claude-red

## Source revision

- Repository URL: `https://github.com/SnailSploit/claude-red`
- Repository: `SnailSploit/claude-red`
- Default branch: `main`
- Reviewed HEAD: `24d7968bab4b883e7f13477afe0fd91f2df3b722`
- Exact tag at reviewed HEAD: `v0.3.0`
- Commit timestamp: `2026-08-29T19:31:36+03:00`
- Review date: `2026-09-16`
- Review mode: source-level architecture/security/reliability analysis; external source treated as knowledge only; no destructive exploitation

All repository-specific findings below are bound to the reviewed revision above. Where a finding depends on a pinned third-party GitHub Action, that dependency revision is named separately.

## Executive summary

`claude-red` is not an agent runtime or durable orchestration system. It is a Git-versioned library of offensive-security `SKILL.md` content plus a small amount of installation, manifest-generation, conversion, and GitHub Actions automation. Claude/provider runtime behavior sits outside the repository: the repository supplies instructions, but it does not own task lifecycle, durable workflow state, scheduling, retries, crash recovery, completion authority, capability enforcement, or execution receipts.

The repository is useful for Runlith primarily as external validation and as a source of failure modes around skill packaging. Its strongest positive decisions are metadata-first on-demand skill loading, a category/skill taxonomy, deterministic generation of a machine-readable skill manifest from the source tree, and explicit red-team engagement discipline around authorized scope, rules of engagement, deconfliction, evidence capture, and retesting.

The deeper source audit also exposes several important limits. The published skill-format contract requires YAML frontmatter, but only 50 of 78 reviewed skills use it; 28 legacy skills are still silently indexed by `tools/build_manifest.py` with fallback names and empty descriptions. Some individual skill bodies exceed 10,000 lines, including one around 567 KB, so whole-skill lazy loading can still become large-context loading. The legacy converter targets a different format and lowercase `skills/` path and self-installs PyYAML at runtime even though its conversion logic does not use YAML parsing. These are packaging/reproducibility lessons rather than architecture Runlith should import.

The most material security finding is in `.github/workflows/skill-optimize-apply.yml`. A created issue comment containing `/apply-optimize` on a pull request is sufficient to start a job with `pull-requests: write` and `contents: write`; the workflow does not check the comment author's association or another trusted-principal predicate. The pinned Tessl action used in `mode: apply` also identifies the supposed review comment only by a public HTML marker and parses optimized `SKILL.md` blocks directly from that comment without validating its author. A single crafted comment can therefore carry the marker, an optimized-content block and `/apply-optimize`, causing the write-capable job to treat commenter-authored content as the bot payload. On a same-repository PR branch that the token can update, the action writes the selected `SKILL.md`, commits it and pushes it. This is a confirmed authorization/provenance weakness. Fork-branch push behavior can differ and is not required for the finding.

For Runlith, the overlap search found no new implementation gap that merits an Implementation Knowledge Queue entry. Lazy skills, progressive disclosure, versioned skill/content packs, untrusted provider/model/content semantics, provenance, exact capability/Policy separation, exact approval binding, and future plugin requested-capability/provenance metadata are already designed or implemented. The already-required dedicated Tools / Skills Registry ADR is the right future owner for concrete skill-package validation details. `claude-red` therefore contributes external validation and anti-pattern evidence, not a new competing architecture.

## Project purpose

The project is a curated offensive-security knowledge library intended to prime Claude with specialized methodology for attack surfaces such as web vulnerabilities, identity, infrastructure, exploitation, cloud, containers, CI/CD, AI security, and reporting. The README describes on-demand activation by conversational triggers and supports three principal consumption modes: installation under a Claude skills directory, piping one or more `SKILL.md` files as a Claude system file, or manually placing skill content into a system prompt/conversation (`README.md:21-27,33-58`).

The Git repository is the durable content source. `claude-skills.json` is a generated machine-readable index. The repository does not provide a Runlith-like runtime that authenticates a caller, evaluates requested capabilities, executes tools under a policy envelope, records durable Attempts, or decides completion.

## Architecture map

```text
claude-red
├── Interfaces
│   ├── README installation/manual-loading instructions
│   ├── Claude Skills directory consumption
│   ├── Claude Code --system-file consumption
│   └── Claude.ai manual Project/system-prompt consumption
├── Orchestration
│   └── external to repository; Claude/runtime chooses skill activation
├── Domain
│   └── 78 offensive-security SKILL.md methodology documents
├── State/Persistence
│   ├── Git source tree = canonical content
│   └── claude-skills.json = generated discovery/index projection
├── Providers
│   └── Claude-specific skill/prompt consumption conventions
├── Tools/Capabilities
│   ├── skill prose references many external offensive tools/commands
│   └── no repository-owned capability registry or enforcement layer
├── Execution
│   └── delegated to Claude plus the user's environment/tooling
├── Recovery
│   └── no task/checkpoint/retry/crash-resume runtime
├── Security
│   ├── intended-use / disclosure documentation
│   ├── engagement scope/ROE advice inside content
│   ├── install/conversion/CI automation
│   └── no runtime authorization boundary for skill-directed actions
└── Observability
    ├── no runtime audit/receipt system
    └── reporting skill teaches evidence/log/report discipline
```

## Canonical state ownership

The Git repository/content tree is the canonical state owner for the project itself. `Skills/<category>/<skill>/SKILL.md` files are source content; `claude-skills.json` is derived from that tree by `tools/build_manifest.py` (`tools/build_manifest.py:15-17,49-86`).

For an actual AI session, this repository owns no canonical execution state. Skill selection, prompt construction, model context, tool availability, tool invocation, retries, and session lifecycle are owned by the consuming Claude/runtime environment. There is no durable job/task record, explicit state machine, scheduler, Attempt/Session hierarchy, transactional state transition, or restart reconciler in the reviewed source.

This distinction is essential for Runlith: skill content can be a versioned input to a workflow, but Task/Execution/Attempt/Provider Session state remains Runlith-owned canonical state.

## Strong engineering decisions

### CR-F001 — Metadata-first, on-demand skill discovery

**finding_id:** `CR-F001`

**source:** repository documentation + skill packaging contract

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `README.md`; `CONTRIBUTING.md`

**symbol:** skill loading/discovery convention; required frontmatter `name` / `description`

**region:** `README.md:21-25,33-39`; `CONTRIBUTING.md:5-30`

**observation:** The library exposes compact metadata for discovery and expects full skill content to be loaded only when conversational matching selects the skill. The contribution contract explicitly says the description is used for trigger matching.

**confidence:** `HIGH`

**Problem:** Loading every specialized workflow into baseline context wastes tokens and can create instruction interference.

**Mechanism:** Keep skills as separate content units; use compact metadata/description for selection; materialize a skill body only after selection.

**Why it works:** Most tasks never need most domain-specific methodologies, so metadata is enough for initial retrieval.

**Limitations:** The unit of loading is still an entire skill. Oversized skill bodies can remain expensive after activation, and the repository does not model trust/capabilities in the metadata.

**Failure modes:** weak/missing descriptions cause discovery failures; broad descriptions cause false-positive activation; a selected monolithic skill can still dominate context.

**Complexity:** low at library scale; higher once provenance, trust, capability requirements, versioning, and retrieval observability are added.

**Runlith overlap:** `ALREADY_DESIGNED`. `AGENT.md:130-152`, `INV-028`, Context Engine progressive disclosure, `SkillRegistryPort`, and the deferred Tools / Skills Registry target already require this behavior.

**Decision:** `EXTERNAL_VALIDATION`.

**Differentiation:** Runlith keeps the same retrieval advantage while attaching source/trust/provenance and keeping skill content unable to grant execution authority.

### CR-F002 — Generated manifest is a derived view of source content

**finding_id:** `CR-F002`

**source:** implementation

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `tools/build_manifest.py`

**symbol:** `parse_frontmatter`, `main`

**region:** `15-17,19-46,49-86`

**observation:** The generator walks the canonical `Skills` tree in sorted order and emits categories plus skill name/path/description into `claude-skills.json`. The manifest is reconstructed from source files rather than maintained as an independent runtime database.

**confidence:** `HIGH`

**Problem:** A separately edited registry can diverge from the actual skill source.

**Mechanism:** Rebuild the discovery projection from source-controlled canonical content.

**Why it works:** There is one editable content source and a reproducible derivation path.

**Limitations:** The generator validates very little. Deterministic derivation does not imply semantic validity.

**Failure modes:** invalid source metadata is deterministically propagated into an invalid/degraded manifest; hard-coded manifest version can drift from repository release state.

**Complexity:** low.

**Runlith overlap:** `ALREADY_DESIGNED`. Runlith already separates canonical state from derived views/projections and requires provenance/drift handling for provider-specific compatibility views (`INV-070`, `INV-116`).

**Decision:** `EXTERNAL_VALIDATION`.

### CR-F003 — Engagement scope and evidence discipline expressed as an operational envelope

**finding_id:** `CR-F003`

**source:** skill content

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `Skills/infrastructure/offensive-advanced-redteam/SKILL.md`; `Skills/utility/offensive-reporting/SKILL.md`

**symbol:** `Quick Workflow`, `Scope and Objectives`, `Rules of Engagement (ROE)`, `Deconfliction`, reporting `Quick Workflow`

**region:** advanced-redteam `8-25,29-63`; reporting `6-16,20-39,63-95`

**observation:** The methodology requires an explicit authorization window, allowed techniques, out-of-scope systems, pause/notification thresholds, emergency deconfliction contacts, action logging, evidence capture at execution time, two-pass review, and retest planning.

**confidence:** `HIGH`

**Problem:** Security work has high-cost side effects and ambiguous attribution unless scope and evidence are explicit before and during execution.

**Mechanism:** Treat engagement scope and allowed techniques as a bounded operational envelope and capture action/evidence provenance while executing.

**Why it works:** It gives operators and reviewers a shared boundary for what may happen and a contemporaneous record of what did happen.

**Limitations:** In `claude-red` these are instructions, not mechanically enforced capability constraints.

**Failure modes:** model/operator ignores prose, scope changes without synchronized authorization, or logs prove execution but not that the action was permitted/correct.

**Complexity:** low as methodology; materially higher when enforced as typed policy.

**Runlith overlap:** `ALREADY_IMPLEMENTED` / `ALREADY_DESIGNED` through G6 Policy/Approval/Runner, exact scoped capabilities, Security Audit, execution receipts, and Verification.

**Decision:** `EXTERNAL_VALIDATION`.

**Differentiation:** Runlith turns the useful methodology into product-owned Policy and execution evidence rather than trusting prompt compliance.

## Implementation techniques

### CR-F004 — Silent compatibility fallback hides a split skill schema

**finding_id:** `CR-F004`

**source:** implementation + repository census + documented contract

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `CONTRIBUTING.md`; `tools/build_manifest.py`; `Skills/**/SKILL.md`; `claude-skills.json`

**symbol:** required Frontmatter contract; `parse_frontmatter`; manifest entry fallback

**region:** `CONTRIBUTING.md:5-30`; `tools/build_manifest.py:19-46,63-85`

**observation:** The contribution contract states that YAML frontmatter is required and that skills without it will not load via the Claude Skills system. In the reviewed tree, 50 of 78 skills start with YAML frontmatter and 28 retain the older `## Metadata` format. The manifest builder does not reject missing frontmatter: it falls back to the directory name and an empty description, so all 78 are indexed and 28 manifest entries have empty descriptions.

**confidence:** `HIGH`

**Problem:** A registry can report a package as present while the actual consumer cannot discover/load it according to the documented contract.

**Mechanism:** Current implementation uses permissive compatibility fallback instead of a validation gate.

**Why it appears convenient:** legacy content remains visible in the aggregate manifest during migration.

**Limitations:** presence and validity become conflated.

**Failure modes:** broken discovery, false inventory counts, tools believing a skill is usable when required metadata is absent, and format drift surviving indefinitely.

**Complexity:** low to fix conceptually: validate package schema and represent invalid/degraded state explicitly. Migration policy still needs a versioned contract.

**Runlith overlap:** `ALREADY_DESIGNED` at the architecture level. Tools / Skills Registry still requires a dedicated ADR, while Runlith already has versioned contracts, provenance and fail-visible handling for malformed security-relevant state (`INV-090`, `INV-149`).

**Decision:** `EXTERNAL_VALIDATION` as a negative packaging lesson. No separate implementation queue item is justified because the existing deferred Tools / Skills Registry ADR already owns the concrete package-contract decision.

### CR-F005 — Whole-skill lazy loading still permits very large context units

**finding_id:** `CR-F005`

**source:** repository census + contribution guidance

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `CONTRIBUTING.md`; large `Skills/**/SKILL.md` content units

**symbol:** skill body as loading unit

**region:** `CONTRIBUTING.md:32-73`; `Skills/infrastructure/offensive-windows-boundaries/SKILL.md` whole file; `Skills/infrastructure/offensive-windows-mitigations/SKILL.md` whole file

**observation:** Contribution guidance says 200-800 lines is typical and recommends splitting monolithic skills, but the reviewed tree contains extreme outliers: `offensive-windows-boundaries` is about 15,151 lines / 567,484 bytes; `offensive-windows-mitigations` about 14,544 lines / 530,459 bytes; several other skills exceed 10,000 lines. Lazy selection therefore avoids baseline cost but does not bound selected-skill context cost.

**confidence:** `HIGH`

**Problem:** A retrieval unit can be correctly selected yet still be too large for efficient reasoning/context budgets.

**Mechanism:** The repository currently treats one `SKILL.md` as the unit of knowledge loading.

**Why it works for smaller skills:** a single self-contained methodology is simple to author and distribute.

**Limitations:** size and internal relevance are not part of retrieval planning.

**Failure modes:** context displacement, higher token cost, weaker instruction salience, and loading many irrelevant sections because one part of a skill matched.

**Complexity:** moderate if upgraded to addressable subresources/sections with metadata while retaining authoring ergonomics.

**Runlith overlap:** `SUPERSEDED`. Runlith Context Engine already defines progressive L0/L1/L2 disclosure and addressable resources instead of assuming a selected source must enter context as one blob.

**Decision:** `REJECT` whole-blob loading as the Runlith default; retain only the external validation that skill selection should be lazy.

### CR-F006 — Legacy conversion path is detached from the current source contract

**finding_id:** `CR-F006`

**source:** implementation

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `convert_skills.py`

**symbol:** `INPUT_DIR`, `parse_skill`, `main`, runtime PyYAML installation

**region:** `11-13,15-105,178-223`

**observation:** The converter targets lowercase `skills/` while the current canonical tree is `Skills/`. Its parser expects legacy Markdown sections such as `## Metadata`, `## Description`, `## Trigger Phrases`, and `## Instructions for Claude`, not the current required YAML-frontmatter format. At startup it imports PyYAML and, if absent, runs `pip install pyyaml`, although the shown conversion parser constructs YAML text manually and does not use YAML parsing for the conversion logic.

**confidence:** `HIGH`

**Problem:** Migration/build tooling can silently retain assumptions from an older content format and mutate its host environment during execution.

**Mechanism:** A standalone legacy converter was kept alongside a newer canonical manifest path without contract synchronization.

**Why it may have existed:** it automated an earlier one-time format migration.

**Limitations:** it is not a reliable validator/converter for the current tree.

**Failure modes:** no-op due path-case mismatch, degraded conversion of modern files, unpinned network/package-manager side effects, and false confidence that two formats are supported consistently.

**Complexity:** low if treated as removable migration tooling; higher if true multi-version import support is intended.

**Runlith overlap:** `CONFLICTS_WITH_ARCHITECTURE` for importing the behavior. Runlith's dependency/provenance and deterministic verification rules already favor explicit dependency setup and versioned contracts.

**Decision:** `REJECT`.

## AI / orchestration architecture

`claude-red` contains AI-oriented content but no application-owned agent loop. The repository defines no planner, no subagent runtime, no durable memory system, no provider router, no model selector, no retry/fallback engine, no cost-accounting pipeline, no long-running execution protocol, no human-approval state machine, and no independent completion verifier.

The answers to the required authority/lifecycle questions are therefore:

- **Who owns workflow state?** The consuming Claude/application session outside this repository. `claude-red` owns only static content/versioned source.
- **Who owns completion authority?** Outside this repository. Skills may instruct an operator/model to track steps, but no product component here decides durable task completion.
- **Can the model mutate canonical state?** Not within `claude-red` itself. When consumed, the model can act through whatever tools the host gives it; the repository supplies no mediation layer.
- **Can model output directly cause privileged action?** Potentially in the consuming environment, because the README explicitly supports loading skill text as system-level instructions and many skills contain executable operational commands. Whether privileged execution is possible depends entirely on the host's tool/capability boundary.
- **What happens after crash?** No repository-owned execution state exists to resume.
- **What happens after provider timeout?** No repository-owned retry/recovery semantics.
- **How does execution resume?** It is a host/provider concern; there is no checkpoint or reconciler contract in this repository.

### CR-F007 — Skill text can be elevated to system-level instruction, but capability authority is external

**finding_id:** `CR-F007`

**source:** repository documentation + representative skill content

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `README.md`; `Skills/utility/offensive-fast-checking/SKILL.md`

**symbol:** Claude Code/manual loading; `Instructions for Claude`

**region:** `README.md:48-58`; fast-checking `15-26`

**observation:** The documented Claude Code path pipes skill content into `claude --system-file -`, and the manual path recommends placing skill text in a Project system prompt or prepending it to the conversation. A representative legacy skill directs Claude to load/apply the full methodology, follow its steps, track completion, and suggest next actions. Other skills contain executable tool commands. The repository has no separate requested-capability manifest or runtime Policy gate that corresponds to these procedural instructions.

**confidence:** `HIGH`

**Runlith overlap:** The loading mechanism as an authority model `CONFLICTS_WITH_ARCHITECTURE`. `AGENT.md:146-153`, `INV-090`, and `INV-146` explicitly prevent a skill/content package from overriding task scope, policy, invariants, or granting authority. Future marketplace metadata already requires requested capabilities/provenance, and effective authority is product-owned.

**Decision:** `REJECT` as a Runlith authority mechanism. Skill content may propose workflow/tool use; Policy and trusted application services decide whether those actions are allowed.

## Security analysis

### CR-S001 — Comment text is treated as authorization for a write-capable PR mutation workflow

**classification:** `CONFIRMED`

**finding_id:** `CR-S001`

**source:** repository workflow + pinned action implementation

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `.github/workflows/skill-optimize-apply.yml`

**symbol:** `jobs.apply.if`, job permissions, `tesslio/skill-review-and-optimize` apply step

**region:** `1-21`

**dependency evidence:** `tesslio/skill-review-and-optimize@bff9490027d60847df6494fdac7dccfb3ad82948`, `src/apply.ts:15-29,51-58,75-99,101-145,147-180`

**observation:** The workflow is triggered by every created `issue_comment`. Its only job predicate is that the issue is a pull request and the comment body contains `/apply-optimize`; it does not check `author_association`, a maintainer/team membership, the PR author, or another trusted principal. The job requests `pull-requests: write` and `contents: write`. The pinned apply action uses `GITHUB_TOKEN`, reads the PR head branch and checks it out. Its `extractOptimizedContent` parser accepts public `<!-- tessl-optimized:<path> -->` Markdown blocks (`src/apply.ts:15-29`), and the apply path selects the supposed review comment using only `COMMENT_MARKER` presence (`src/apply.ts:101-125`) without checking comment author. It then writes accepted `SKILL.md` paths, commits, and runs `git push origin HEAD:<prBranch>`.

**Impact:** A commenter can forge the expected review marker and optimized-content block in the same comment that carries `/apply-optimize`. For a same-repository PR branch that the workflow token can update, this lets the workflow commit commenter-chosen content to a path accepted by the action's `SKILL.md` path checks, despite the commenter lacking independent push authority. Fork PR pushes can fail because the branch is not necessarily writable through the base repository origin; that limitation does not remove the same-repository authorization/provenance weakness.

**confidence:** `HIGH`

**Runlith lesson:** External commands/comments are evidence or requests. Authenticate the principal and evaluate exact mutation authority before a trusted component performs a write. Text matching must never become the authorization decision.

**Existing Runlith protection:** G6 Policy/Approval (`INV-146`-`INV-153`) separates requested operation from effective authority and binds approval to exact request, capabilities, provenance and validity.

**Runlith status:** `ALREADY_IMPLEMENTED` for the general authority principle.

**Decision:** `EXTERNAL_VALIDATION` of Runlith's external-event/Policy boundary; record as a reusable anti-pattern, not a new task.

### CR-S002 — Procedural skill instructions are security-sensitive content, but runtime privilege impact is host-dependent

**classification:** `PLAUSIBLE`

**finding_id:** `CR-S002`

**source:** documented installation path + skill content

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `README.md`; representative `Skills/**/SKILL.md`

**symbol:** `--system-file` loading/manual system-prompt loading

**region:** `README.md:48-58`

**observation:** Skill text can be placed at high model-instruction priority and contains operational commands/methodology. The repository itself does not define or enforce the host's filesystem/process/network/credential authority.

**confidence:** `HIGH`

**Impact:** If a host runtime equates instruction priority with tool authority, malicious or compromised skill content could influence privileged operations. The reviewed repository does not by itself prove such host behavior, so privilege escalation is not classified as confirmed here.

**Runlith lesson:** Treat skill content as untrusted/versioned workflow data. Requested capabilities are metadata; granted capabilities come from canonical Policy/approval and are enforced at the operation boundary.

**Existing Runlith protection:** `AGENT.md:128,146-153`, `INV-090`, `INV-093`, `INV-146`-`INV-153`, future marketplace requested-capability/provenance metadata.

**Decision:** `EXTERNAL_VALIDATION` / `REJECT` prompt-as-authority semantics.

### CR-S003 — Shell/argument injection in installer

**classification:** `NOT_SUPPORTED`

**finding_id:** `CR-S003`

**source:** implementation review

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `install.sh`

**symbol:** option parsing, category validation, copy/install path

**region:** `15-20,41-50,62-83,89-107`

**observation:** Reviewed user-controlled target/category values are quoted when passed to filesystem/copy operations, the category must correspond to a real directory, and there is no `eval`, `bash -c`, or similar interpolation path in the installer. The `--target` / `--category` parser assumes a following value and can fail under `set -u`, but that is an input-validation/reliability issue rather than demonstrated injection.

**confidence:** `HIGH`

### CR-S004 — Secret leakage from fork PR preview workflow

**classification:** `NOT_SUPPORTED`

**finding_id:** `CR-S004`

**source:** workflow review

**repository:** `SnailSploit/claude-red`

**revision:** `24d7968bab4b883e7f13477afe0fd91f2df3b722`

**file:** `.github/workflows/skill-preview.yml`

**symbol:** `jobs.review`, pinned Tessl review action invocation

**region:** `1-16`

**observation:** The workflow passes `secrets.TESSL_API_TOKEN` to a pinned third-party action on `pull_request`. The repository source alone does not establish that the secret is exposed to untrusted fork PR code; no source-level evidence of a credential leak was found. The relevant action is commit-pinned, reducing tag-mutation risk, although all third-party workflow code remains a supply-chain trust decision.

**confidence:** `MEDIUM`

## Reliability / recovery analysis

There is no durable execution lifecycle to recover. The repository's relevant reliability concerns are content/index consistency and automation behavior.

The largest reliability weakness is schema drift between the documented frontmatter contract and the source tree, combined with a permissive manifest generator that converts missing metadata into empty/fallback values rather than an explicit invalid state (`CR-F004`). This can make inventory appear healthy while discovery quality is degraded.

The legacy converter is a second drift signal (`CR-F006`). It encodes an earlier Markdown format and a different path casing. Its runtime dependency installation adds an avoidable network/environment side effect to a deterministic content-conversion task.

There are no retries, idempotency keys, durable leases, crash checkpoints, or reconciliation loops for agent execution because no such execution engine exists here.

## Performance / scalability analysis

At repository/catalog scale, 78 static skills and 23 categories are trivial to index. The meaningful scaling dimension is context size after retrieval.

The design successfully avoids loading all 78 skill bodies into every conversation, but selected-unit size is uncontrolled. Multiple individual skill files are hundreds of kilobytes and over ten thousand lines (`CR-F005`). A system that materializes an entire matching skill therefore shifts the scaling problem from global prompt bloat to per-selection prompt bloat.

Runlith's L0/L1/L2 progressive disclosure, addressable resource model, recoverable compression, exact retrieval, cache-aware token optimization, and `Verified Delivery Cost` framing are a stronger fit: retrieve only the relevant section/evidence shape and preserve source provenance, rather than treating one Markdown document as the atomic context unit.

The repository has no concurrency scheduler, worker pool, provider routing, queue, database, or distributed state that warrants separate throughput/locking analysis.

## Anti-patterns

### AP candidate — Comment command is treated as mutation authority

**Anti-pattern:** A natural-language/comment command becomes sufficient authority to run a write-capable workflow.

**Evidence:** `.github/workflows/skill-optimize-apply.yml:1-21`; pinned action `src/apply.ts:51-58,75-99,101-180`.

**Immediate impact:** a commenter can trigger the bot's precomputed skill changes on a writable same-repository PR branch without a separate trusted-principal authorization check.

**Long-term impact:** event transport and authorization become coupled; adding more powerful comment commands silently widens what an untrusted event can cause.

**Runlith lesson:** external commands are requests/evidence; principal authentication plus exact Policy authorization must precede mutation.

**Existing Runlith protection:** `INV-146`-`INV-153`, Approval binding and operation-boundary enforcement.

This anti-pattern is added to `ANTI_PATTERN_LIBRARY.md` because it is reusable beyond this repository and is not equivalent to the existing content-package or provider-permission anti-patterns.

### Existing AP-001 validation — Content package grants runtime authority

The README/system-file consumption model and skill bodies reinforce existing `AP-001`: workflow/skill content can describe required actions but must never directly widen process/filesystem/network/tool authority. No new anti-pattern ID is needed for this repository-specific instance.

### Packaging anti-pattern — Invalid/degraded skill metadata is silently indexed

The 28 legacy/no-frontmatter skills demonstrate a reusable ingestion lesson, but it remains in this repository report rather than adding another library entry: the future Runlith Tools / Skills Registry ADR already has to define package validation, versioning, provenance and requested capabilities, and the existing architecture already requires malformed security-relevant state to fail closed/visible.

### Packaging anti-pattern — Migration tool self-mutates the environment

`convert_skills.py:215-221` installs PyYAML during execution. This is a reproducibility/supply-chain smell, especially because the current conversion logic shown does not use YAML parsing. It is not promoted to a standalone Runlith anti-pattern entry because existing dependency and provenance rules already cover the lesson and no runtime Runlith gap was found.

## Runlith overlap analysis

### External idea: lazy skill loading by compact discovery metadata

**External evidence:** `README.md:23-25,33-39`; `CONTRIBUTING.md:20-30`.

**Closest Runlith equivalent:** Context Engine progressive disclosure and future `SkillRegistryPort`.

**Runlith evidence:** `AGENT.md:130-152`; `INV-028`; Source of Truth `SkillRegistryPort` around `4550-4555`; ADR coverage marks Tools / Skills Registry as a deferred architecture target requiring a dedicated ADR.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none created by this repository.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: versioned skill catalogs as content, not provider-specific core logic

**External evidence:** Git-owned skill tree/categories plus generated manifest.

**Closest Runlith equivalent:** `INV-090` role/persona/skill catalogs as versioned data/content packs.

**Runlith evidence:** `docs/architecture/INVARIANTS.md` `INV-090`; Source of Truth same invariant around `5825`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** none.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: deterministic manifest/index generation from canonical skill source

**External evidence:** `tools/build_manifest.py:15-17,49-86`.

**Closest Runlith equivalent:** canonical-state/derived-projection rules and provider compatibility views with provenance/drift detection.

**Runlith evidence:** `INV-070`; `INV-116`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** future Tools / Skills Registry ADR still needs a concrete package/index contract, but that ADR is already mandatory and this repository does not create a separate missing architecture owner.

**Decision:** `EXTERNAL_VALIDATION`.

### External idea: explicit skill metadata for capability requirements

**External evidence:** Negative evidence: current frontmatter contract contains `name` and `description`, not a typed capability request; tool needs appear in prose. `CONTRIBUTING.md:20-30`.

**Closest Runlith equivalent:** Capability Registry, G6 Policy, plugin/marketplace package metadata.

**Runlith evidence:** Source of Truth `3830-3834`, `7075-7079`, `7310-7316`; `INV-146`-`INV-153`.

**Current status:** `ALREADY_DESIGNED`.

**Actual gap:** concrete skill-package schema belongs to the already-required Tools / Skills Registry ADR; no separate new gap.

**Decision:** `EXTERNAL_VALIDATION` of the need for separation; `REJECT` prose-only capability authority.

### External idea: put the skill itself in system-level instructions

**External evidence:** `README.md:48-58`; representative skill instructions `Skills/utility/offensive-fast-checking/SKILL.md:15-26`.

**Closest Runlith equivalent:** skills as reusable workflows loaded into bounded context.

**Runlith evidence:** `AGENT.md:146-153`; `INV-090`, `INV-093`, `INV-146`.

**Current status:** `CONFLICTS_WITH_ARCHITECTURE` if system-prompt placement is interpreted as authority.

**Actual gap:** none. Runlith already distinguishes instruction/context from canonical Policy/effective authority.

**Decision:** `REJECT` as an authority mechanism; `INSPIRED_ONLY` as a compatibility rendering technique if a provider needs system-level placement while Runlith still enforces external capability boundaries.

### External idea: engagement scope, allowed techniques and deconfliction as first-class operator concepts

**External evidence:** advanced-redteam `8-25,35-63`; reporting `10-16`.

**Closest Runlith equivalent:** Execution Envelope, exact capabilities, Policy/Approval, Security Audit, execution receipts, Verification.

**Runlith evidence:** G6 architecture and `INV-146`-`INV-153`.

**Current status:** `ALREADY_IMPLEMENTED` / `ALREADY_DESIGNED` depending on the future security-runner UX surface.

**Actual gap:** no new canonical architecture gap was found. The future Security Runner can use this as UX/content inspiration without changing authority ownership.

**Decision:** `EXTERNAL_VALIDATION` / `INSPIRED_ONLY` for UX.

### External idea: comment-triggered apply automation

**External evidence:** `.github/workflows/skill-optimize-apply.yml:1-21`; pinned action `src/apply.ts:51-180`.

**Closest Runlith equivalent:** external events/commands enter as evidence; trusted application services plus Policy decide mutation authority.

**Runlith evidence:** `INV-146`-`INV-153`; exact approval binding; event/reconciliation architecture.

**Current status:** `CONFLICTS_WITH_ARCHITECTURE` when comment presence alone is used as authorization.

**Actual gap:** none.

**Decision:** `REJECT`; retain as anti-pattern evidence.

## External validation of existing Runlith design

`claude-red` independently supports several existing Runlith choices:

1. **Skill content should be lazy/discoverable rather than permanent prompt ballast.** This directly validates `INV-028` and Context Engine progressive disclosure.
2. **Skill catalogs are content packages, not workflow ownership.** The repository works as a useful library precisely because its content tree is separate from the consuming runtime; Runlith formalizes that separation more strongly with `INV-090`.
3. **A generated registry should remain a projection of canonical content.** `tools/build_manifest.py` reinforces Runlith's canonical-state/derived-view discipline.
4. **Operational security work needs explicit scope and contemporaneous evidence.** The advanced red-team/reporting methodologies validate Runlith's Execution Envelope, Policy, Security Audit and Verification direction.
5. **Skill text alone is not a sufficient security boundary.** The repository's own content can describe aggressive actions, while enforcement is left to the host. This validates Runlith's rule that model/provider/repository content is untrusted and cannot grant authority.
6. **External commands must not be trusted because they look intentional.** The comment-triggered CI workflow demonstrates exactly why Runlith treats external events as evidence/request input and binds privileged operations to an authenticated, authorized principal/capability decision.

## Novel gaps

No `NOVEL_GAP`, `PARTIALLY_EXISTS`, or newly discovered `NEEDS_ADR` item survived the mandatory Runlith overlap search.

The concrete skill package schema remains intentionally unresolved because Tools / Skills Registry is already a `DEFERRED ARCHITECTURE TARGET` with a mandatory dedicated ADR before implementation. `claude-red` supplies useful requirements/evidence for that future ADR — strict schema validation, explicit invalid/degraded state, source/content provenance, requested capabilities separated from granted authority, bounded/addressable content units — but it does not justify a second ADR candidate or a new implementation queue item today.

## Adoption matrix

| External finding / idea | Runlith status | Adoption decision | Reason |
|---|---|---|---|
| Metadata-first lazy skill loading | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Matches `INV-028`, Context Engine and `SkillRegistryPort`. |
| Focused category/skill taxonomy | `ALREADY_DESIGNED` | `INSPIRED_ONLY` | Useful authoring UX; canonical schema still belongs to future Tools / Skills ADR. |
| Source-derived manifest/index | `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Matches canonical-state / derived-view discipline. |
| Engagement scope / ROE / deconfliction / evidence | `ALREADY_IMPLEMENTED` / `ALREADY_DESIGNED` | `EXTERNAL_VALIDATION` | Maps to Policy, Approval, Runner, Security Audit and Verification. |
| Skill body as system-level operational instruction | `CONFLICTS_WITH_ARCHITECTURE` as authority | `REJECT` | Skill/context cannot override canonical Policy or grant effective capability. |
| Whole-skill blob as retrieval unit | `SUPERSEDED` | `REJECT` | Runlith has finer progressive disclosure/addressable context. |
| Silent fallback for schema-invalid skill packages | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Invalid/degraded package state should be explicit and provenance-bearing. |
| Prose-only tool/capability requirements | `ALREADY_DESIGNED` replacement | `REJECT` | Future package metadata can request typed capabilities; Policy grants authority. |
| Comment-text-triggered write authority | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | External event is a request/evidence item, not principal authorization. |
| Runtime `pip install` inside converter | `CONFLICTS_WITH_ARCHITECTURE` | `REJECT` | Hidden dependency/network mutation weakens reproducibility/provenance. |

## Ideas rejected for Runlith

- Do not treat provider system-prompt placement as a privilege grant. A renderer may need to place a skill in a provider-specific instruction channel for compatibility, but effective capabilities still come from Runlith Policy and operation-boundary enforcement.
- Do not load an entire selected skill merely because its metadata matched. Large skill resources should remain addressable and support exact retrieval/progressive disclosure.
- Do not index schema-invalid skill packages as normal valid entries with fallback/empty metadata. Preserve validation result and prevent activation when required contract fields are missing.
- Do not let skill/package prose be the only place required tools/capabilities are declared. Requested capability metadata must be typed and separate from grant/approval.
- Do not treat `/command` text in a webhook/comment/event as authority to perform a privileged mutation. Authenticate/authorize the principal separately.
- Do not let deterministic conversion/build tools install dependencies implicitly at runtime.
- Do not add Claude-specific skill semantics to core Task/Context/Policy branches. Provider-native skill rendering remains an adapter/compatibility concern.

## Candidate ADRs

No new ADR candidate is created.

The existing deferred **Tools / Skills Registry** ADR should incorporate the evidence from this review when it is opened, especially:

- versioned package schema and explicit compatibility/migration policy;
- source revision/content hash/provenance;
- compact discovery metadata separated from full content;
- addressable subresources for very large skills;
- strict validity/degraded-state representation;
- requested capability declarations that do not grant authority;
- provider-specific rendering as a derived compatibility view;
- activation/retrieval provenance and observability.

Because ADR coverage already assigns this architecture owner, creating a second candidate ADR would duplicate ownership rather than close a gap.

## Candidate implementation tasks

None.

No candidate met the user's eligibility rule of `NOVEL_GAP`, `PARTIALLY_EXISTS`, or a newly uncovered `NEEDS_ADR` without an existing architecture owner. `IMPLEMENTATION_KNOWLEDGE_QUEUE.md` is therefore intentionally unchanged.

## Source evidence index

### Repository evidence

| ID | File / region | Evidence |
|---|---|---|
| `CR-F001` | `README.md:21-25,33-39`; `CONTRIBUTING.md:5-30` | on-demand loading; description-driven discovery; focused skill contract |
| `CR-F002` | `tools/build_manifest.py:15-17,19-46,49-86` | source-derived manifest generation |
| `CR-F003` | `Skills/infrastructure/offensive-advanced-redteam/SKILL.md:8-25,29-63`; `Skills/utility/offensive-reporting/SKILL.md:6-16,63-95` | scope/ROE/deconfliction/evidence/reporting discipline |
| `CR-F004` | `CONTRIBUTING.md:5-30`; `tools/build_manifest.py:19-46,63-85`; reviewed tree census | required YAML frontmatter vs 50 modern / 28 legacy skills; empty-description fallback |
| `CR-F005` | `CONTRIBUTING.md:32-73`; large skill files | recommended bounded/focused skill size vs >10k-line outliers |
| `CR-F006` | `convert_skills.py:11-13,15-105,178-223` | lowercase legacy source path, legacy parser, runtime PyYAML installation |
| `CR-F007` | `README.md:48-58`; `Skills/utility/offensive-fast-checking/SKILL.md:15-26` | skill content injected as system-level instruction / procedural directives |
| `CR-S001` | `.github/workflows/skill-optimize-apply.yml:1-21` | comment-body gate, write permissions, apply action |
| `CR-S003` | `install.sh:15-20,41-50,62-83,89-107` | quoted target/category use and no observed shell-eval injection path |
| `CR-S004` | `.github/workflows/skill-preview.yml:1-16` | pinned third-party review action with repository secret input |

### Pinned dependency evidence used for CR-S001

- Dependency: `tesslio/skill-review-and-optimize`
- Reviewed dependency revision: `bff9490027d60847df6494fdac7dccfb3ad82948`
- `action.yml:7-8,49-54` — `apply` mode invokes `src/apply.ts` with `GITHUB_TOKEN`.
- `src/apply.ts:15-29` — optimized content and path are parsed directly from specially marked Markdown blocks in a comment body.
- `src/apply.ts:51-58` — obtains token/context.
- `src/apply.ts:75-99` — resolves PR and checks out `pr.head.ref`.
- `src/apply.ts:101-145` — selects a supposed review comment by marker presence without validating author, extracts content, validates path shape and writes `SKILL.md` files.
- `src/apply.ts:147-180` — stages, commits and pushes to the PR branch.

### Repository census at reviewed revision

- Skill files: `78`
- Categories: `23`
- Skills with YAML frontmatter: `50`
- Legacy/no-YAML-frontmatter skills: `28`
- Generated manifest entries with empty description: `28`
- Largest observed skill: `Skills/infrastructure/offensive-windows-boundaries/SKILL.md` — approximately `15,151` lines / `567,484` bytes
- Second-largest observed skill: `Skills/infrastructure/offensive-windows-mitigations/SKILL.md` — approximately `14,544` lines / `530,459` bytes
