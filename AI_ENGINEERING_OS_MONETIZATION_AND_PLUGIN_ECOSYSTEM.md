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
