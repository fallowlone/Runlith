# AI Engineering OS — Monetization and Plugin Ecosystem

> Canonical product strategy for monetization and the future plugin ecosystem.
> This document defines product/commercial direction and future extension
> requirements. It does not override `AGENT.md`, architecture invariants,
> accepted ADRs, gate scope, or security policy. Any implementation-level
> architecture introduced by the plugin system requires its own accepted ADRs.

## 1. Product thesis

AI Engineering OS is a **local-first engineering control plane**, not a model
reseller and not a cloud account that happens to expose AI features.

The product monetizes the durable layer above replaceable executors:

```text
user intent
  → planning / durable tasks
  → orchestration
  → context / memory
  → policy / permissions
  → provider execution
  → review / verification
  → delivery
```

Users should be able to bring their own Claude, Codex, Gemini, OpenCode and
future provider subscriptions, local runtimes, or API credentials wherever the
provider permits that usage. The user pays the provider for model execution;
AI Engineering OS is paid for the control plane, workflow, safety, reliability,
collaboration and extensibility around it.

Local-first is therefore part of the paid value proposition rather than a
constraint on monetization.

## 2. Monetization principles

The commercial model must preserve these rules:

- Core local operation must not require AI Engineering OS cloud services.
- User repositories, prompts, task history and local agent output stay local by
  default.
- Cloud features are optional add-ons with explicit data-flow disclosure.
- The product must not depend on reselling model tokens to have healthy unit
  economics.
- BYO providers/subscriptions remain a first-class path.
- A perpetual local license remains usable after the included update period
  ends.
- Paid updates renew access to future versions; they do not remotely disable an
  already licensed local version.
- Subscription is an alternative purchasing model, not the only path to Pro.
- Team and Enterprise revenue comes from collaboration, governance, support and
  organizational control, not from forcing local users into cloud storage.
- Plugin revenue is a later recurring-revenue layer around a useful standalone
  core product.

Exact prices are commercial hypotheses and may change without an architecture
decision. The tier structure and local-first licensing principles in this file
are the canonical product direction.



## 2.1 Licensing and IP strategy

The commercial core is **not intended to use a permissive license that allows an
unrelated company to fork, rebrand and resell AI Engineering OS without a
separate agreement**. Source visibility and source-code commercialization are
separate questions.

Current strategic direction (subject to qualified legal review before public
release):

```text
Public/source-available core
  + useful Community rights
  + ordinary product subscriptions/licenses
  + separate Commercial/OEM rights
  + trademark protection
```

The project should distinguish three different rights:

```text
1. Use AI Engineering OS to build commercial software
   → normal user/product entitlement; should be broadly allowed.

2. Run AI Engineering OS internally at a company
   → permitted subject to the source license; additional paid components and
     services follow their separately identified commercial terms.

3. Reuse, redistribute, white-label, host-resell, embed or commercialize
   AI Engineering OS source/core itself
   → subject to Shield; rights beyond its permitted purposes require a
     separate Commercial/OEM agreement.
```

A developer paying for Pro must be able to build and sell their own SaaS or
software with AI Engineering OS. That does **not** automatically grant the right
to extract the orchestration core, repackage it as a competing product or offer
AI Engineering OS itself as a managed service.

### Selected repository license

The Community repository default is **PolyForm Shield 1.0.0**, provided verbatim
in [LICENSE](LICENSE). [LICENSING.md](LICENSING.md) defines the repository
boundaries and outstanding commercial/legal work. This is source-available,
not OSI Open Source; no delayed conversion to a permissive license is promised.

Additional Pro/Team/Enterprise components and rights beyond Shield require
separately identified commercial terms. Shield does not itself impose paid
feature or seat entitlements, and product purchase terms must not contradict
rights already granted to the same code under Shield. No commercial EULA or
OEM agreement is established by this strategy document.

The future independently scoped Plugin SDK is intended to use Apache-2.0.
No existing core, IPC or planner-contract component is thereby relicensed.
If the entire Pro implementation is later published while paid internal use
must remain mandatory, that requires a separately reviewed licensing decision,
not an assumed restriction in Shield.

This section is product strategy, not legal advice. Before public release or
accepting material third-party contributions, obtain qualified legal review of
the final license text, contributor terms and commercial agreements.

## 2.2 Contributor, trademark and repository licensing boundaries

Before broad external contribution starts, decide how dual licensing and future
relicensing will work. The project should choose an explicit contributor-rights
mechanism (for example an appropriate CLA) so accepted contributions can legally
remain compatible with both Community and Commercial licensing. A DCO alone is
not an additional grant of commercial relicensing rights. Do not wait until
hundreds of contributors exist before deciding.

Trademark is a separate protection layer. The product/company should reserve the
right to control the **AI Engineering OS** name, logo and implication of official
endorsement even when a code license permits some forms of modification.

Potential repository/license split once the extension contracts stabilize:

| Surface | Likely licensing direction |
| --- | --- |
| Core desktop/runtime/orchestration product | PolyForm Shield 1.0.0 repository default |
| Commercial/OEM/white-label rights beyond Shield | separate negotiated Commercial License |
| Future explicitly scoped Plugin SDK / public schemas / examples | Apache-2.0; no current component designated |
| Enterprise/cloud services | commercial/private as appropriate |
| Brand/trademark | separate trademark policy |

The split must follow real architectural boundaries. Do not create artificial
package boundaries solely to game licensing.

## 2.3 Anti-clone protection is not the whole moat

License restrictions reduce direct code appropriation; they do not replace a
product moat. The durable advantages should come from reliability, verification,
security, provider integrations, UX, cross-provider continuity, plugin/playbook
ecosystem, trusted distribution, enterprise governance, support and operational
experience.

## 3. Product tiers

| Tier | Commercial model | Product role |
| --- | --- | --- |
| **Community** | Free | Useful local-first entry point, single-user/core workflows, community plugins and evaluation of the product. |
| **Pro Local** | Perpetual license + 12 months of updates | Primary individual-developer offer: full local control plane and advanced orchestration without a required subscription. |
| **Pro Subscription** | Monthly/annual subscription | Alternative to the perpetual purchase for users who prefer predictable recurring billing and continuous updates. |
| **Teams** | Per-user subscription | Shared engineering workflows, team policy, shared registries, governance, collaboration and organizational visibility. |
| **Enterprise** | Annual contract | SSO/SCIM, private registry, advanced policy/audit, on-prem/air-gapped options, support and compliance-oriented controls. |
| **Cloud Add-ons** | Usage-based and/or fixed add-on subscription | Optional services such as encrypted sync, remote workers/runners, managed browsers, storage or hosted infrastructure. |

### 3.1 Community

Community must be useful enough to prove the local-first model and grow an
ecosystem.

Expected direction:

- local `agentd` and CLI/control-plane basics;
- single-user operation;
- core provider connections through user-owned credentials/subscriptions;
- basic task/orchestration flows appropriate to the mature core at release;
- local plugin installation for free/open plugins once the plugin platform
  exists;
- no forced cloud account for normal local use.

Community should not be intentionally crippled in ways that damage trust or
make the local-first claim misleading.

### 3.2 Pro Local — perpetual

Recommended primary individual offer:

```text
one-time Pro Local purchase
  + 12 months of product updates
  + perpetual right to use the last eligible version
```

After the included 12 months:

- the installed eligible version continues to work;
- the user can optionally buy another 12 months of updates;
- renewal is not required merely to keep using the owned version;
- security fixes for currently supported versions should follow a published
  support policy rather than opaque remote entitlement changes.

Initial pricing hypothesis:

```text
Pro Local:       €199–299 one-time
Update renewal:   €79–119 / 12 months
```

The final launch price should be validated against willingness to pay and the
actual amount of completed product value.

### 3.3 Pro Subscription

Offer the same individual Pro capability through an optional subscription for
users who do not want a larger one-time purchase.

Initial pricing hypothesis:

```text
€15–25 / month
```

The subscription should primarily change entitlement and update access, not
create an artificial second architecture for the product.

### 3.4 Teams

Teams monetizes organizational coordination and governance around the local
runtime.

Expected paid value includes:

- shared projects/workspaces where enabled;
- team policy and approval profiles;
- shared provider/tool/plugin configuration without exposing raw secrets;
- team-owned skills and plugin registries;
- shared review/verification standards;
- role-based administration;
- audit and change history appropriate to team operation;
- encrypted sync or coordination services where explicitly enabled;
- enterprise-ready migration path.

Initial pricing hypothesis:

```text
€30–50 / user / month
```

### 3.5 Enterprise

Enterprise monetizes governance, deployment control, integration and support.

Expected direction:

- SSO / SAML / OIDC where appropriate;
- SCIM/provisioning;
- centralized policy and approval administration;
- enterprise audit controls and retention policy;
- private plugin registry;
- organization allowlists/denylists;
- signed internal plugins;
- on-premise or customer-controlled deployment options;
- air-gapped/offline operation where feasible;
- compliance/security documentation;
- support, rollout assistance and negotiated SLAs.

Initial commercial hypothesis:

```text
from approximately €10k/year, then organization-specific pricing
```

### 3.6 Optional cloud add-ons

Cloud add-ons must remain separable from the local core. Candidates include:

- encrypted multi-device sync;
- remote workers/runners;
- managed browser execution;
- hosted artifact/storage capacity;
- optional remote control-plane relay;
- organization coordination services;
- managed marketplace/license services;
- optional hosted model capacity where it creates clear user value.

Every add-on must publish what leaves the local machine, why, how long it is
retained and how it can be disabled.

## 4. BYO subscriptions and providers

BYO execution is a central economic and architectural strategy.

AI Engineering OS should support replaceable provider/runtime adapters so users
can connect already-paid or independently billed execution sources, subject to
each provider's supported interfaces and terms.

Examples include:

- provider subscriptions exposed through supported local tools;
- user-owned API accounts;
- local/open models;
- organization-owned inference endpoints;
- future agent runtimes that implement accepted adapter contracts.

Provider billing remains between the user/organization and that provider unless
AI Engineering OS explicitly offers a separate hosted add-on.

This keeps model cost outside the base software margin and lets the product
compete on orchestration quality rather than token resale.

## 5. Plugin ecosystem strategy

The plugin ecosystem is a **post-core capability**.

It must not block or expand G1 or G2. It must not pull marketplace, plugin
runtime, payment, remote registry or extension-host work into early gates.

The dependency order is:

```text
core task/orchestration foundations
  → Policy / Security foundation
  → Tools / Skills foundation
  → stable extension contracts
  → local plugin runtime + permissions
  → signed local distribution
  → plugin library
  → public marketplace / payments
  → enterprise private registries
```

The plugin runtime should begin only after the host can enforce permissions and
extension boundaries. Marketplace implementation begins later still.

The plugin ecosystem must extend a strong standalone product. It must never be
used as a reason to postpone core reliability, policy, verification or Tools /
Skills foundations.



### 5.1 Playbooks, procedures and routines as extension products

The extension ecosystem should preserve distinct concepts rather than calling
all reusable automation a plugin:

```text
Tool
→ atomic capability

Playbook
→ versioned executable sequence with typed inputs/outputs, scopes and evidence

Procedure
→ conditional orchestration of tools/playbooks

Routine
→ scheduled/event/condition-triggered execution of an approved procedure

PluginPackage
→ distribution container that may ship tools, skills, playbooks, procedures,
  routines, connectors and small UI contributions
```

A high-value marketplace can therefore sell or distribute more than executable
code. Examples include audited Playbooks, policy packs, specialized reviewers,
security procedures, release workflows, context optimizers and integration
bundles.

Approved/locked Playbook versions should be executable without granting the
agent authority to rewrite the workflow itself. Updates create a new version and
may require review/re-approval when permissions, network access, secrets or
behavior change.

## 6. Library vs marketplace

Use two related concepts:

### Plugin Library

The local product surface where users can see installed, available and pinned
plugins. It works with:

- built-in/first-party plugins;
- locally installed plugin packages;
- organization/private registry packages;
- public marketplace packages when connected.

### Plugin Marketplace

An optional hosted discovery, reputation, update and commerce layer.

The marketplace can provide:

- search and discovery;
- publisher identity;
- signatures and package metadata;
- compatibility data;
- reviews/ratings;
- paid plugin checkout and entitlements;
- malware/security review signals;
- update/revocation metadata;
- publisher analytics that do not require repository-content collection.

The local plugin runtime must not depend on continuous marketplace availability.

## 7. Initial plugin categories

The first ecosystem should focus on extensions that strengthen existing
engineering workflows rather than create unrelated product surfaces.

### 7.1 Prompt Improver

Improves the initial request while preserving user authority.

Required behavior:

```text
Original User Prompt
  ↓
Prompt Improver
  ↓
Proposed Improved Intent
  ├─ original preserved
  ├─ assumptions made explicit
  ├─ constraints extracted
  ├─ ambiguity highlighted
  ├─ acceptance criteria suggested
  └─ structured Task/ChangeSpec proposal
  ↓
host/user/policy acceptance where required
```

It must never silently replace the original prompt, invent hidden authority or
modify protected system/policy instructions.

### 7.2 Context Optimizer

Selects, ranks, compresses or proposes context for a task through Context Engine
contracts. It must not directly rewrite authoritative project state.

### 7.3 Review Enhancer

Adds specialized review passes or structured findings to the verification
pipeline. It proposes evidence/findings; it does not grant completion authority
to itself.

### 7.4 Security Scanner

Runs bounded security checks and returns structured findings/evidence. Any shell,
filesystem, network or secret use is separately permission-gated.

### 7.5 Documentation Assistant

Generates or updates documentation proposals from accepted source-of-truth
inputs. Writes require the same policy/task-scope enforcement as other edits.

### 7.6 Commit / PR Packager

Builds commit messages, release notes, PR descriptions and review packets from
verified local changes. Git mutation and remote publication are distinct
permissions.

### 7.7 Requirement Refiner

Transforms rough intent into proposed requirements, constraints, non-goals and
acceptance criteria while preserving the original request and marking inferred
content.

### 7.8 Test Generator

Proposes or generates tests inside approved task scope. Test execution uses host
tool/runtime permissions rather than direct unrestricted process access.

### 7.9 Repo Readiness Rules

Adds repository-specific or ecosystem-specific readiness checks to the
Repository Readiness Engine through a versioned rule API.

### 7.10 Integrations

Connectors for systems such as GitHub, GitLab, Linear, Jira, Slack, CI systems,
issue trackers, documentation sources and internal engineering platforms.

Integrations must map external systems into host-owned contracts. They do not
become authoritative over Task state merely because they can read/write an
external service.

## 8. Plugin package model

The concrete format requires a future ADR, but the product requires every
installable plugin package to declare enough metadata for deterministic review
and enforcement.

Conceptual manifest fields:

```text
plugin_id
name
publisher
version
license
pricing model
host API compatibility
entry points
plugin capabilities
requested permission scopes
declared network domains
declared tool dependencies
package hashes
publisher signature
optional provenance / build metadata
```

Plugins must be content-addressable or otherwise hash-verifiable once resolved
to an installable artifact.

## 9. Plugin APIs and host boundaries

The plugin platform should expose narrow, versioned APIs rather than arbitrary
access to internal implementation objects.

Expected API families:

| API | Purpose | Boundary |
| --- | --- | --- |
| **Manifest / Capability API** | Declare metadata, capabilities and requested scopes. | Declarative only; grants no permission by itself. |
| **Prompt Proposal API** | Propose prompt/requirement transformations. | Original remains available; no hidden system/policy override. |
| **Context API** | Read approved context inputs and return ranked/derived context proposals. | No direct authoritative storage access. |
| **Review / Evaluation API** | Return structured findings, evidence and recommendations. | Plugin cannot mark its own work complete. |
| **Tool Invocation API** | Request host tools. | Host Policy decides and sandbox enforces. |
| **Readiness Rule API** | Add versioned repository-readiness checks. | Rule output is evidence, not authority over core invariants. |
| **Integration API** | Read/write supported external systems. | Explicit scopes, credentials through Secrets Broker, typed receipts. |
| **Event Subscription API** | Observe approved host events. | Versioned event contracts; no raw internal DB tailing. |
| **UI Contribution API** | Add small future UI surfaces when justified. | Sandboxed/limited contribution points; no arbitrary host UI mutation. |

### Hard boundaries

A plugin must not receive implicit access to:

- internal SQLite tables or another bounded context's storage;
- arbitrary host filesystem paths;
- arbitrary processes/shell execution;
- unrestricted network access;
- plaintext secrets from application state;
- protected system prompts or policy internals beyond explicit public contract;
- direct Task lifecycle mutation;
- direct completion authority;
- direct Policy mutation;
- direct architecture/ADR override;
- provider-specific credentials except through an authorized brokered binding.

Privileged actions must pass through host APIs that can enforce Policy, scope,
sandboxing, audit and receipts.

## 10. Permissions and scopes

Plugins use **default deny**. Installation does not automatically grant every
declared capability.

Conceptual scopes may include:

```text
project.read
project.write
git.read
git.write
task.read
task.propose
context.read
context.propose
memory.propose
review.submit
events.subscribe:<event-family>
tool.invoke:<tool-id>
network:<domain-or-service>
secrets.use:<binding-id>
process.run:<sandbox-profile>
ui.contribute
```

Exact names and semantics require future ADR/API design.

Rules:

- scopes must be explicit and inspectable before enabling a plugin;
- high-risk scopes require stronger user/org approval;
- network permission should be domain/service constrained where possible;
- secrets are referenced by binding/handle and never stored in plugin config as
  plaintext by the host;
- a plugin cannot convert one permission into a broader permission;
- permission changes are visible as an update-time diff;
- an update requesting broader permissions must not silently inherit approval.

## 11. Sandbox and runtime isolation

Policy decides whether an action is allowed; sandboxing enforces what the plugin
process/runtime can actually do. The plugin platform requires both.

The future runtime should support, as appropriate to the plugin type:

- isolated process/runtime boundary;
- bounded filesystem mounts/workspaces;
- temporary storage separated from host internals;
- network deny-by-default with approved destinations;
- CPU/memory/time/concurrency limits;
- controlled subprocess/tool access;
- explicit secret-handle injection only for approved calls;
- structured IPC between plugin and host;
- termination on protocol or sandbox violation;
- auditable privileged-action receipts.

In-process plugins with arbitrary native code should not be the default trust
model for third-party marketplace content.

## 12. Plugin trust levels

Trust is visible metadata, not a bypass around Policy.

Recommended trust ladder:

| Level | Meaning |
| --- | --- |
| **Local / Unverified** | Side-loaded by the user. Package may be unsigned or not known to the marketplace. Strong warnings and explicit permissions apply. |
| **Signed** | Publisher signature and package integrity verify, but behavior has not necessarily been marketplace-reviewed. |
| **Marketplace Reviewed** | Signed package passed automated checks plus required marketplace review for declared behavior/permissions. |
| **Verified Publisher / High-Trust** | Publisher identity and stronger security/provenance requirements are verified; may include deeper review or reproducible-build evidence. |
| **Enterprise Approved** | Organization-specific trust/allowlist decision for a particular plugin/version/source. |

Enterprise approval is organizational policy, not proof that a plugin is
universally safe.

## 13. Signing, provenance and review

Public marketplace packages should be signed.

Minimum direction:

- immutable versioned artifacts;
- cryptographic package hash;
- publisher signature;
- signed marketplace metadata;
- publisher identity for paid plugins;
- automated package scanning;
- permission-manifest validation;
- dependency/provenance metadata where applicable;
- human review for sensitive permissions or high-risk categories;
- transparent review status and date;
- revocation/update metadata that can be cached locally.

The marketplace must distinguish:

```text
signature valid
publisher verified
package reviewed
enterprise approved
```

These are different facts and must not be collapsed into a single vague
"trusted" badge.

## 14. Local installation and offline behavior

Local installation remains first-class even after a marketplace exists.

Supported future flows should include:

```text
local package/archive
private registry
public marketplace
first-party bundled catalog
```

Installation should:

1. resolve an exact plugin version;
2. verify integrity/signature when present/required;
3. show requested permissions and trust level;
4. record source, hash, version and granted scopes in the install ledger;
5. install into a host-controlled plugin location;
6. keep project/global pinning deterministic;
7. allow the plugin to run offline when its capability does not require a
   remote service.

Automatic updates should be opt-in or policy-controlled. Reproducible projects
must be able to pin plugin versions.

Paid-plugin entitlements should be locally cacheable. A temporary marketplace
outage must not immediately break already-authorized local workflows. Perpetual
plugin licenses, if offered, remain usable according to their purchased terms;
subscription plugins may use a reasonable signed offline entitlement window.

## 15. Compatibility and versioning

The host/plugin contract needs explicit compatibility rules before third-party
distribution.

Required direction:

- semantic or similarly explicit plugin package versions;
- separately versioned host Plugin API;
- manifest-declared minimum/maximum compatible host API range;
- capability-level versioning when one large API version would cause needless
  coupling;
- immutable published plugin versions;
- deterministic dependency resolution;
- project/global version pinning;
- permission changes surfaced separately from ordinary code updates;
- deprecation windows for public APIs;
- compatibility testing before marketplace publication/update;
- rollback to a previously installed compatible version where safe.

A marketplace listing must clearly show host compatibility before installation.

## 16. Marketplace discovery and reputation

Discovery should optimize for useful engineering outcomes rather than download
count alone.

Useful listing metadata:

- category and supported workflows;
- supported AI Engineering OS / Plugin API versions;
- requested permissions;
- trust/review level;
- publisher identity;
- pricing/license;
- last update;
- compatibility history;
- issue/support link;
- privacy/network behavior;
- ratings/reviews;
- install count where reliable and privacy-safe;
- organization approval status for enterprise users.

Ratings should require evidence of actual installation/use where possible and
must include anti-abuse controls. Marketplace ranking should not silently
penalize free/open plugins in favor of higher platform revenue.

Paid placement, if ever introduced, must be clearly labeled and separated from
organic/relevance ranking.

## 17. Free and paid plugins

The ecosystem supports:

- free/open-source plugins;
- free proprietary plugins;
- one-time paid plugins;
- subscription plugins;
- organization-negotiated plugins;
- first-party premium plugins/bundles;
- private/internal plugins that are never publicly listed.

Pricing is declared per plugin and must not be coupled to unnecessary data
collection.

Paid plugins can use locally cached signed entitlements. Runtime authorization
should validate entitlement independently from arbitrary repository content.

## 18. Marketplace revenue share

Recommended launch target for third-party paid plugins:

```text
Publisher: 85%
Platform:  15%
```

The split should be calculated on clearly defined net marketplace receipts,
with taxes, refunds and payment-processing treatment stated in publisher terms.

The platform share pays for:

- discovery/distribution;
- payment and entitlement infrastructure;
- signing and package metadata;
- review/security pipeline;
- compatibility infrastructure;
- marketplace support and abuse handling.

Private internal Enterprise plugins distributed only inside the customer's
registry should not incur a per-sale marketplace commission. The private
registry/governance capability is monetized through the Enterprise contract.

If an Enterprise customer purchases a commercial third-party plugin through the
public marketplace, the normal marketplace commercial terms may apply.

## 19. Enterprise private registry

Enterprise can operate an organization-scoped registry/catalog that supports:

- private internal plugins;
- organization signing keys/trust roots;
- curated mirrors of public plugins;
- version pinning;
- allow/deny policies;
- required trust levels;
- approved permission ceilings;
- staged rollout rings;
- emergency disable/revocation;
- audit of install/update/enable/disable events;
- offline/air-gapped package import where contractually supported.

The organization may require that only Enterprise Approved plugin versions run
in managed workspaces.

## 20. Security review policy

Marketplace review depth is risk-based.

Examples:

### Lower risk

- pure prompt/requirement transformations with no filesystem/network/tools;
- static local rule packs with deterministic inputs/outputs.

### Medium risk

- repository read access;
- Git metadata access;
- context provider access;
- local file generation inside task-approved paths.

### Higher risk

- repository writes;
- Git mutation;
- subprocess/tool execution;
- network access;
- external-service write access;
- secret bindings;
- organization administration;
- code that runs outside the strongest available sandbox.

Higher-risk plugins require stronger review, clearer consent and stricter
runtime enforcement.

## 21. Privacy and telemetry

Plugin discovery/commerce must preserve the product's local-first promise.

Default rules:

- plugin runtime data stays local unless the plugin has explicit network/service
  permission;
- marketplace analytics do not receive repository contents, prompts, code,
  task history or model output by default;
- publisher analytics expose aggregated marketplace/commercial events, not
  private local engineering data;
- diagnostics/telemetry are opt-in according to product privacy policy;
- listings must disclose whether a plugin sends data remotely and to which
  service/domain categories;
- uninstall removes executable/plugin-owned local state subject to explicit
  retention rules, while security/audit records follow host policy.

## 22. Plugin lifecycle and supply-chain response

The ecosystem must support safe lifecycle management:

```text
publish
  → review
  → install
  → enable
  → update
  → deprecate
  → revoke/disable when necessary
  → uninstall
```

Requirements:

- immutable published versions;
- reasoned revocation metadata;
- locally cached verification state;
- explicit compatibility failure rather than best-effort execution;
- visible permission changes;
- deterministic uninstall/disable;
- enterprise override/approval controls;
- rollback where compatible and safe.

Security revocation must identify the affected version/artifact precisely. It
must not provide a general-purpose remote kill switch for unrelated local
software.

## 23. First-party plugin strategy

Before opening a public marketplace, AI Engineering OS should prove the plugin
API with first-party plugins implemented through the same public extension
contracts intended for third parties.

Strong candidates:

```text
Prompt Improver
Requirement Refiner
Context Optimizer
Review Enhancer
Documentation Assistant
Commit / PR Packager
Repo Readiness Rules
```

Security Scanner and Test Generator should follow once the sandbox/tool
permission surfaces are mature enough to enforce their stronger capabilities.

First-party plugins must not receive secret undocumented bypass APIs that make
the public extension model second-class.

## 24. Roadmap sequencing

The plugin marketplace is deliberately outside the current early-gate critical
path.

### Now — G1/G2

```text
Do not implement marketplace.
Do not implement plugin commerce.
Do not introduce plugin runtime dependencies.
Do not expand G1/G2 acceptance criteria for plugins.
```

### Foundation phase

Before third-party plugins:

```text
Policy / permissions foundation
Tools / Skills registry foundation
Secrets Broker boundary where needed
sandbox/tool execution boundary
stable extension contracts
install-ledger/versioning model
```

### Plugin Platform v0

```text
first-party plugins through public contracts
local package installation
manifest + scopes
version pinning
local install ledger
basic sandbox boundary
```

### Plugin Platform v1

```text
publisher signing
private registries
public catalog/discovery
compatibility checks
review pipeline
```

### Marketplace v1

```text
paid/free listings
payments
entitlements
ratings/reviews
publisher portal
revenue share
```

### Enterprise ecosystem

```text
private registry governance
organization trust policy
staged rollouts
air-gapped import/mirroring where supported
enterprise audit controls
```

## 25. Product success measures

The plugin ecosystem is successful when it improves the value of the core
without making the core dependent on marketplace usage.

Useful future product metrics include:

- percentage of active installations using at least one plugin;
- plugin activation-to-retention rate;
- successful installs vs compatibility failures;
- permission-denial/error rates;
- first-party vs third-party usage distribution;
- number of maintained compatible plugins;
- paid plugin conversion and renewal;
- marketplace GMV and platform revenue;
- publisher retention;
- enterprise private-registry adoption;
- plugin-caused security/reliability incidents;
- review-to-publication time by risk class.

Repository code, prompts and private task content are not acceptable raw inputs
to commercial analytics by default.

## 26. Explicit non-goals

The future plugin ecosystem must not become:

- a store of opaque prompt snippets with no permission/provenance model;
- a path for plugins to bypass Policy, sandboxing or verification;
- a remote marketplace dependency required to run the local product;
- a mechanism for arbitrary in-process third-party native code by default;
- a second provider-specific orchestration system;
- a way for plugins to become direct Task-state authority;
- a way for marketplace ranking to override engineering trust/safety signals;
- a reason to delay G1, G2 or the core engineering control-plane roadmap.

## 27. Canonical commercial shape

The intended long-term business is:

```text
Community
  → free local adoption

Pro Local
  → perpetual paid control plane
  → 12 months updates included
  → optional paid update renewal

Pro Subscription
  → recurring alternative for the same individual market

Teams
  → collaboration + shared governance + shared extension management

Enterprise
  → deployment + policy + audit + private registry + support

Optional Cloud Add-ons
  → usage-based remote capabilities without making cloud mandatory

Plugin Ecosystem
  → free + paid extensions
  → third-party marketplace revenue share
  → first-party premium extensions
  → Enterprise private registry
```

The core economic principle is stable: **users can keep their execution
providers while paying AI Engineering OS for the durable, secure, extensible
engineering control plane around them.**


---

# Appendix — Detailed licensing and monetization research notes (2026-09-11)

> Historical research merged from the former staging file. The canonical
> commercial direction is defined by the main sections above. Questions and
> candidate licenses in this appendix remain subject to legal and market
> validation and do not override accepted architecture/security policy.

> Status: staging document for later consolidation into product strategy, repository policy, licensing decision memo, and commercialization roadmap.
>
> Important: this document records current strategic ideas and constraints. It is **not** legal advice and does not constitute a final license decision.
>
> Current development work remains unchanged: these notes must not alter G5 / TASK-0051.

## 1. Core owner requirement

A central strategic requirement is now explicit:

> The repository may be publicly visible and users should be able to inspect and use the product, but third parties should not be able to take AI Engineering OS, fork it, rebrand it, resell it, offer it as a competing hosted service, or freely reuse substantial parts of its core code to build a commercial competing product without a separate commercial license.

This requirement materially changes the licensing search space.

## 2. Open source vs source-available

```text
Open Source
-----------
must allow commercial use under OSI-style principles

Source Available
----------------
source can be public/inspectable
but commercial, competitive, hosted, redistribution,
or embedding rights may be restricted
```

Because the project owner wants to prevent commercial clones and commercial reuse of core code without permission, the likely direction is:

```text
source-available product
+ free/community rights
+ paid application subscriptions
+ separate commercial/OEM rights
```

rather than classic MIT/Apache/GPL/AGPL open source for the whole commercial core.

## 3. Licenses preliminarily deprioritized for the main commercial core

### MIT
Broad commercial reuse, redistribution, modification, and sublicensing are allowed subject to minimal notice requirements.

**Conclusion:** excellent for adoption; poor for protecting against commercial forks.

### Apache-2.0
Also broadly permits commercial reuse and modification, with stronger patent provisions.

**Conclusion:** excellent for ecosystem adoption; poor for preventing commercial forks or repackaging.

### GPLv3
Strong copyleft on distributed derivative works, but commercial use and sale are allowed.

**Conclusion:** does not satisfy a requirement that commercial reuse itself be prohibited.

### AGPLv3
Adds network-source obligations for modified network-interactive deployments.

Useful against closed SaaS forks, but commercial use is still allowed and competitors may potentially operate commercial services if they comply with AGPL. Enterprise procurement may also avoid AGPL.

**Conclusion:** potentially valuable for reciprocal openness; not sufficient if the owner's goal is to prevent commercial competing use itself.

## 4. Candidate strategy: Source-Available Community License + Commercial License

Current strongest strategic direction:

```text
Public repository
        │
        ├── Community / Personal rights
        │     inspect source
        │     run product
        │     modify for own use
        │     learn/evaluate
        │     contribute
        │     build plugins via supported APIs
        │
        ├── Product subscription rights
        │     use AI Engineering OS to build commercial software
        │     use existing Claude/Codex/Gemini subscriptions
        │
        └── Commercial source-code rights
              OEM
              redistribution
              white-label
              embedding
              source-code commercialization
              competing products
              hosted resale
              managed service based substantially on the core
              → separate agreement
```

This cleanly separates:

```text
using AI Engineering OS
≠
obtaining commercial rights to AI Engineering OS source code
```

## 5. Community rights concept

Potential free/community rights:

- inspect source;
- compile and run locally;
- personal use;
- educational use;
- evaluation;
- research;
- local modification for own use;
- contribution;
- development of plugins/extensions through documented extension points;
- potentially small-scale indie use, depending on final policy.

Potentially restricted without commercial license:

- selling AI Engineering OS itself;
- selling a rebranded fork;
- commercial redistribution;
- white-label redistribution;
- offering a hosted/managed clone;
- creating a substantially competing product from the source;
- extracting substantial core modules into another commercial competing product;
- OEM embedding;
- removing licensing/attribution protections;
- claiming official affiliation;
- commercial relicensing or sublicensing.

Final legal wording must be drafted/reviewed by qualified counsel.

## 6. Commercial use of the product should remain allowed

A customer should be allowed to use AI Engineering OS to build software that they themselves sell.

```text
Developer pays for AI Engineering OS Pro
→ uses it to build a SaaS product
→ sells that SaaS
→ fully allowed
```

This is different from:

```text
Developer takes AI Engineering OS core
→ embeds/rebrands it as their own product
→ sells that product
```

The second case should require a separate commercial/OEM agreement.

This distinction is essential to avoid making the product unattractive to professional developers.

## 7. PolyForm Shield as a serious candidate

PolyForm Shield is currently an interesting standard source-available candidate because its design focuses on allowing broad use while restricting use to build competing products.

Potential advantages:
- avoids inventing an entirely custom license from scratch;
- directly addresses competition;
- source remains visible;
- can be paired with a separate commercial license.

Potential concerns:
- need careful legal review of what counts as a competing product;
- may not cover every desired “commercial code extraction” scenario exactly as intended;
- enterprise legal teams may be less familiar with it than standard OSS licenses.

Current status: strong runner-up / possible default source-available license; not final.

## 8. Custom Community Source License

A custom AI Engineering OS Community License could precisely encode the desired commercial boundaries.

Potential benefits:
- exact control over hosted use, redistribution, embedding, OEM, competition, and commercial forks;
- clear separation of app usage from source-code exploitation.

Potential downsides:
- custom licenses create legal uncertainty;
- enterprises dislike unfamiliar terms;
- ecosystem tooling may not recognize the license;
- legal drafting cost is higher;
- mistakes can create unenforceable or ambiguous terms.

Recommendation: if a custom license is ever used, do not write it casually. Base it on established source-available patterns and have it reviewed professionally.

## 9. BSL / Business Source License

BSL-style licensing is attractive because source is visible and current versions may restrict production/competitive use, while older versions later convert to an open-source license.

Concern: the automatic Change Date means old versions eventually become open source.

If the owner strongly wants permanent anti-clone protection for the commercial core, BSL may not fit.

Could still be valuable if strategy becomes:

```text
current versions protected
older versions eventually open
```

## 10. FSL / Fair Source style delayed-open approach

Similar strategic concern:

```text
current code protected
→ after delay
→ older code becomes open source
```

This is attractive for ecosystem trust but weaker if the long-term goal is to prevent commercial reuse of even old core technology.

Use only if delayed openness becomes an intentional product strategy.

## 11. Elastic License / managed-service protection

Elastic-style licensing is useful as inspiration because it is designed to prevent third parties from simply offering the product itself as a managed service.

However, it may not map exactly to the desired restrictions on code extraction, competing products, OEM embedding, or commercial reuse of core modules.

Treat it as a reference model rather than an automatic answer.

## 12. Recommended licensing architecture — current hypothesis

```text
Core application / orchestration code
→ source-available community license
  (PolyForm Shield or professionally drafted equivalent)

Commercial rights
→ separate Commercial License

Plugins / SDK / client libraries
→ possibly more permissive license
  e.g. Apache-2.0 or MIT where ecosystem adoption matters

Examples / templates
→ permissive

Brand
→ trademark policy

Hosted/team proprietary services
→ separate commercial code where appropriate
```

This allows the project to protect its moat while still encouraging plugin/integration adoption.

## 13. Repository split strategy

Potential future split:

```text
ai-engineering-os
  core product
  source-available

ai-engineering-os-sdk
  plugin SDK
  permissive

ai-engineering-os-examples
  permissive

ai-engineering-os-protocol
  protocol/schema definitions
  potentially permissive

ai-engineering-os-enterprise
  private/commercial or separate repository

ai-engineering-os-cloud
  private/commercial
```

The final split should follow actual architecture boundaries, not artificial licensing boundaries.

## 14. Plugin ecosystem strategy

Possible approach:

```text
Core
→ protected source-available

Plugin SDK / API contracts
→ permissive

Third-party plugin
→ author chooses their own compatible license

Marketplace
→ separate terms / revenue share
```

This lets businesses safely integrate without receiving unrestricted rights to the commercial core.

## 15. Trademark strategy

Trademark can protect:
- “AI Engineering OS” name;
- official logo;
- product identity;
- official distributions;
- marketplace branding.

Potential future need:
- trademark usage policy;
- official/unofficial distribution rules.

## 16. Contributor / IP strategy before broad public contributions

Before accepting many external contributors, decide how future dual licensing/relicensing will work.

Potential mechanisms:

```text
DCO
CLA
copyright assignment
or
no future relicensing strategy
```

For dual licensing, the company/project must have sufficient rights to distribute contributions under both community and commercial terms.

Potential contributor model:

```text
Contributor retains copyright
+
grants project/company broad rights
to distribute/relicense contribution
under community and commercial terms
```

This should be legally reviewed.

Important: do not wait until hundreds of contributors exist before deciding this.

## 17. Dual licensing

Potential structure:

```text
Community License
→ free/community/local use under defined restrictions

Commercial License
→ additional rights for:
   OEM
   embedding
   commercial redistribution
   hosted resale
   white-label
   competing-product rights
   special contractual terms
```

## 18. Subscription rights vs source rights

Keep two commercial layers conceptually separate.

### Product subscription
Pays for access to product features/services.

Examples:
- Pro;
- Team;
- Enterprise.

### Source-code commercial license
Pays for IP rights.

Examples:
- OEM;
- redistribution;
- embedding;
- commercial fork;
- white-label;
- managed-service rights.

A Pro subscription should not implicitly grant OEM rights.

An Enterprise subscription should not automatically grant unrestricted source-code reuse unless explicitly contracted.

## 19. Preliminary monetization structure

Likely future product tiers:

```text
Community / Free
Pro Individual
Team / Business
Enterprise
Commercial / OEM License
```

The Commercial/OEM License is not necessarily a normal self-serve subscription tier. It may be contract-based.

## 20. Community / Free philosophy

Goal: make the local product genuinely useful rather than crippleware.

Potential Community features:
- local agentd;
- agentctl;
- local Task Graph;
- SQLite/Event Journal;
- local worktree isolation;
- use own Claude/Codex/Gemini subscriptions;
- manual provider selection;
- local verification basics;
- basic UI;
- community plugins/playbooks;
- no mandatory account;
- telemetry off by default.

Free should demonstrate the complete product concept, not merely act as a demo.

## 21. Pro Individual philosophy

Target: solo developer / power user.

Potential paid value:
- advanced UI;
- advanced orchestration;
- richer automation;
- provider routing;
- cross-provider handoff;
- advanced history/search;
- premium Playbooks;
- advanced review workflows;
- background routines;
- productivity analytics;
- optional sync.

Target price hypothesis for later research:

```text
~€15–25/month
```

with annual discount.

Important: do not charge users again for model tokens when they are using their existing provider subscriptions/CLI access.

## 22. Team / Business philosophy

Target: small teams and SMBs.

Potential value:
- shared Task Graph;
- team workspaces;
- approvals;
- shared policies;
- shared playbooks;
- audit history;
- centralized provider controls;
- team usage visibility;
- GitHub/Jira/Linear/Slack integrations;
- role-based permissions;
- shared remote workers.

Potential price hypothesis:

```text
~€35–70 / user / month
```

Final price requires market benchmarking and cost modeling.

## 23. Enterprise philosophy

Target: larger organizations, regulated teams, on-prem customers.

Potential features:
- SSO/SAML/OIDC;
- SCIM;
- advanced RBAC;
- centralized governance;
- audit retention;
- security/compliance controls;
- enterprise secrets integration;
- private/on-prem deployment;
- remote worker fleet management;
- policy packs;
- support/SLA;
- procurement/legal terms;
- deployment assistance.

Possible pricing:

```text
custom annual contract
```

Avoid making enterprise merely “Team with a higher price”.

## 24. Commercial / OEM License

Separate from ordinary subscription pricing.

Use cases:

```text
embed AI Engineering OS in another product
redistribute the product
white-label it
sell a managed version
commercially fork the core
ship it as part of another commercial platform
```

Pricing should likely be negotiated, with an annual minimum and potentially royalty/revenue-linked terms.

Do not publish a cheap self-serve OEM price early.

## 25. BYO-provider strategy

A major product advantage:

```text
User already pays for:
Claude
Codex
Gemini
etc.
```

AI Engineering OS should, where technically and contractually possible, orchestrate those existing subscription-backed CLIs/accounts.

Principle:

> Do not force users to buy tokens from AI Engineering OS when AI Engineering OS did not bear the inference cost.

## 26. Hosted/managed compute should be separately billable

If AI Engineering OS actually incurs cost, charge for it.

Examples:
- hosted remote workers;
- managed cloud execution;
- artifact storage;
- team synchronization;
- premium connectors;
- retained logs;
- hosted model inference;
- managed browser/computer workers.

Possible model:

```text
base subscription
+
usage-based hosted resources
```

## 27. Avoid double charging

Bad model:

```text
€40/month app subscription
+
forced token markup
+
forced cloud runner fee
```

when the user already has provider subscriptions and wants everything local.

Preferred:

```text
local BYO-provider path
→ predictable subscription

optional managed services
→ explicit usage price
```

## 28. Pricing balance principle

Too cheap can create:
- weak perceived value;
- support burden;
- difficult sustainable development;
- difficult future price increases.

Too expensive can:
- compete directly with Claude/Codex subscription cost;
- make users feel double-charged;
- damage solo-developer adoption.

A key pricing benchmark will be:

```text
AI provider subscription cost
+
developer tool subscription cost
+
team collaboration value
```

## 29. Open/local core vs monetized layer

Strong future boundary:

```text
Local core
----------
should remain useful
should not require account
should not require cloud
should support own provider subscriptions

Paid value
----------
advanced orchestration convenience
team collaboration
managed infrastructure
organization governance
enterprise security
support
premium integrations
premium marketplace content
```

Do not intentionally cripple fundamental local execution merely to create artificial paywalls.

## 30. Marketplace monetization

Future plugin/playbook marketplace could monetize through:
- paid plugins;
- paid Playbooks;
- organization policy packs;
- specialized reviewers;
- security packs;
- integration packs;
- release automation;
- context-optimization modules.

Possible later model:

```text
creator keeps majority
platform revenue share
```

Exact percentage should be researched later.

## 31. Anti-clone protection does not replace product moat

Real moat can come from:

```text
architecture quality
reliability
verification quality
security
provider integrations
UX
plugin/playbook ecosystem
community
release trust
team workflows
enterprise integrations
brand
customer support
historical execution intelligence
```

## 32. Protection layers

```text
Copyright
→ protects source expression

License
→ defines permitted uses

Commercial contracts
→ grant additional paid rights

Trademark
→ protects brand identity

Architecture / product moat
→ protects market position

Trade secrets / private services
→ protect non-public operational know-how where appropriate
```

## 33. Current preliminary ranking

Given the anti-commercial-fork requirement:

```text
1. PolyForm Shield-style Community License
   + separate Commercial License

2. Professionally drafted AI Engineering OS
   Community Source License
   + separate Commercial License

3. BSL
   only if delayed open-source conversion is acceptable

4. FSL/Fair Source
   only if delayed openness becomes intentional

5. AGPLv3
   if reciprocal open-source SaaS use is acceptable

MIT / Apache
   poor fit for protected commercial core
```

Final ranking must be based on deeper legal/business research.

## 34. Critical unresolved questions

Before final licensing decision, answer:

1. Should commercial internal business use of Community edition be free?
2. Should small companies have free internal use?
3. What exactly counts as a “competitive product”?
4. Should commercial internal use require Team subscription?
5. Is commercial redistribution always contract-only?
6. Should old versions ever become open source?
7. Is dual licensing required?
8. Do we want broad external contributions?
9. Will proprietary Enterprise modules exist?
10. Should plugin SDK/protocols use permissive licenses?
11. What legal entity will own IP?
12. Which jurisdictions matter most?
13. Should Community license permit SaaS use that is not competitive?
14. Should consultants be allowed to deploy the product for clients?
15. Should resellers/MSPs need a partner license?

## 35. Current recommendation direction

```text
AI Engineering OS
-----------------

Public source
+
useful local Community edition
+
paid Pro/Team/Enterprise product tiers
+
BYO provider subscriptions
+
optional paid hosted resources
+
separate Commercial/OEM source-code license
+
trademark protection
+
early contributor-rights strategy
```

This currently appears to best balance:

```text
user trust
local-first adoption
solo affordability
business value
enterprise monetization
anti-clone protection
source transparency
```

## 36. Current project impact

No current implementation change.

```text
G5: IN PROGRESS
TASK-0051: current
TASK-0052: next re-audit
G6: not started
```

Licensing and monetization remain a separate product-strategy track until a formal decision memo is approved.
