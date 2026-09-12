# Data Flow Map

> Concise registry of architecturally accepted data flows. `FLOW-*` IDs are
> local documentation identifiers, not ADR IDs.
>
> - Guarantees: [`TECHNICAL_PRIVACY_CONTRACT.md`](./TECHNICAL_PRIVACY_CONTRACT.md)
> - Architecture: [`PRIVACY_ARCHITECTURE.md`](./PRIVACY_ARCHITECTURE.md)
> - Sensitivity rules: [`../security/DATA_CLASSIFICATION.md`](../security/DATA_CLASSIFICATION.md)
> - Boundaries: [`../security/TRUST_BOUNDARIES.md`](../security/TRUST_BOUNDARIES.md)

Destination vocabulary rule: never a generic "cloud". Destinations name one
of: AI provider infrastructure; AI Engineering OS infrastructure;
third-party plugin/integration; general internet/web target; future remote
worker. Provider transfer ≠ our-cloud transfer, always.

## FLOW-001 — Local repository → Resource/Context processing

- **Source:** local repository / worktree files.
- **Destination:** Resource Engine normalization + Context Engine selection
  (same device).
- **Data involved:** file contents, dependency metadata, structural
  representations, hashes, provenance.
- **Default classification:** PROJECT (higher where source requires;
  conservative inheritance for derived representations).
- **Purpose:** build normalized, addressable resources and select minimal
  execution context.
- **Local or external:** LOCAL.
- **User action / feature causing flow:** opening a project; task execution
  requiring context.
- **Policy/consent expectation:** none beyond using the product; content is
  untrusted data, never instruction (TB5).
- **Persistence at destination:** resource registry entries and derived
  representations, local only.
- **Observability requirement:** retrieval selection trajectory
  (INV-018, INV-031).
- **Relevant PRIV/INV/ADR:** INV-037…042, INV-115; ADR-0010.

## FLOW-002 — Context Engine → local model

- **Source:** Context Engine (local Context compilation).
- **Destination:** genuinely local inference adapter (same device).
- **Data involved:** selected provider-bound context package.
- **Default classification:** PROJECT/PRIVATE as selected; SECRET never.
- **Purpose:** inference without any device-boundary crossing.
- **Local or external:** LOCAL.
- **User action / feature causing flow:** execution routed to a local model.
- **Policy/consent expectation:** routing privacy eligibility gates
  (INV-060); local inference is the privacy-preserving route.
- **Persistence at destination:** provider context is not durable
  application state (INV-019).
- **Observability requirement:** standard execution evidence; privacy label
  shows Data leaves device: NO.
- **Relevant PRIV/INV/ADR:** PRIV-010; INV-019, INV-060; ADR-0010.

## FLOW-003 — Context Engine → external provider CLI/API

- **Source:** Context Engine (local Context compilation + security filter).
- **Destination:** AI provider infrastructure via provider CLI/API
  (illustrative: Claude Code → Anthropic; Codex → OpenAI; Gemini → Google).
- **Data involved:** selected provider-bound context only — never whole
  repository dump by default, never SECRET (any provider flow containing
  SECRET is invalid).
- **Default classification:** PROJECT/PRIVATE as selected; RESTRICTED only
  under explicit policy/approval; SECRET excluded.
- **Purpose:** external inference for the authorized execution.
- **Local or external:** EXTERNAL PROVIDER TRANSFER.
- **User action / feature causing flow:** execution routed to an external
  provider (user-selected provider / routing policy).
- **Policy/consent expectation:** provider choice is user-visible;
  transfer shown clearly (PRIV-010) via privacy labels and Data Flow
  Inspector.
- **Persistence at destination:** provider-owned retention applies — not
  controlled by our application and never claimed as ours (PRIV-004
  distinction).
- **Observability requirement:** provider, model, representation/data sent,
  classification, approximate/known quantity, execution time/reference;
  pre-upload-equivalent disclosure of what leaves the device and to whom.
- **Relevant PRIV/INV/ADR:** PRIV-003, PRIV-004, PRIV-010; INV-008,
  INV-018, INV-060, INV-084, INV-115; ADR-0009, ADR-0010, ADR-0014.

## FLOW-004 — Provider response → local runtime

- **Source:** AI provider infrastructure.
- **Destination:** local Agent Runtime.
- **Data involved:** provider output (inference results, tool requests).
- **Default classification:** handled as untrusted data regardless of
  source classification.
- **Purpose:** continue local orchestration with inference results.
- **Local or external:** EXTERNAL → LOCAL.
- **User action / feature causing flow:** continuation of a FLOW-003
  execution.
- **Policy/consent expectation:** none additional; output never gains
  instruction, policy or completion authority.
- **Persistence at destination:** attempt/session records and artifacts
  persist locally per canonical ownership; provider context itself is not
  durable state.
- **Observability requirement:** execution receipts (provenance only);
  evaluation evidence for completion claims.
- **Relevant PRIV/INV/ADR:** INV-004, INV-009, INV-019, INV-072, INV-092,
  INV-093; ADR-0009.

## FLOW-005 — Task/Event/Memory state → local persistence

- **Source:** Task, Execution, Memory, Event Journal writers (agentd).
- **Destination:** local application storage (same device).
- **Data involved:** task rows/edges/status, attempt/session records,
  checkpoints, memory entries with provenance, journal records, audit
  records. SECRET values never — refs/bindings only.
- **Default classification:** PROJECT/PRIVATE/RESTRICTED by category;
  SECRET excluded by construction.
- **Purpose:** durable local-first state.
- **Local or external:** LOCAL.
- **User action / feature causing flow:** any product use.
- **Policy/consent expectation:** none beyond using the product;
  inspectable and deletable (PRIV-007, PRIV-008).
- **Persistence at destination:** durable until user/application lifecycle
  removes it; exact per-category retention durations unresolved (open
  design item — none invented here).
- **Observability requirement:** auditable mutations (INV-010).
- **Relevant PRIV/INV/ADR:** PRIV-007, PRIV-008; INV-001, INV-008, INV-010,
  INV-015, INV-025; ADR-0004/0005/0006/0012.

## FLOW-006 — Secrets Broker → execution boundary

- **Source:** Secrets Broker (refs/bindings; values in OS Keychain /
  Credential Manager / Secret Service).
- **Destination:** authorized tool/process at the trusted execution
  boundary.
- **Data involved:** secret material injected at the boundary only; never
  as application data, never in prompts.
- **Default classification:** SECRET.
- **Purpose:** let an authorized tool authenticate without exposing secret
  values to model context, logs, events or storage.
- **Local or external:** LOCAL injection; the tool's own subsequent use is
  governed by its flow.
- **User action / feature causing flow:** execution of a tool requiring a
  scoped credential binding.
- **Policy/consent expectation:** Policy Engine authorization; scoped
  credential bindings; exact `SecretAccess(ref)` plus exact delivery target;
  no ambient inheritance and no provider-native authority.
- **Persistence at destination:** values are not persisted by the
  application; OS store owns values, app owns refs/bindings. Broker-owned
  temporary mutable bytes are cleared after the trusted delivery attempt.
- **Runner boundary:** TASK-0064 isolation plans contain capability scope and
  enforcement requirements only, never secret values. TASK-0065 separately
  clears ambient provider-launch environment and routes provider-writable
  XDG/state/cache/scratch/temp paths outside managed worktrees; provider-native
  login remains provider-owned and is never converted into Secret Broker data.
- **Observability requirement:** binding/decision audit without secret
  payloads (hashes/references only).
- **Relevant PRIV/INV/ADR:** PRIV-006; INV-008, INV-099, INV-106;
  ADR-0013, ADR-0014.

## FLOW-007 — Optional cloud sync → our infrastructure

- **Source:** local application state.
- **Destination:** AI Engineering OS infrastructure (distinct from any AI
  provider).
- **Data involved:** sync payloads — object categories not invented here;
  defined by future sync design.
- **Default classification:** per synced object; SECRET values never.
- **Purpose:** user-enabled synchronization across devices/teams (future).
- **Local or external:** LOCAL → OUR CLOUD, OPT-IN, implementation deferred.
- **User action / feature causing flow:** explicit enablement of cloud sync.
- **Policy/consent expectation:** opt-in (PRIV-009); payloads inspectable
  before upload via Data Flow Inspector; sync protocol, encryption
  implementation, vendor and retention unresolved (open design items).
- **Persistence at destination:** future cloud retention policy — unresolved.
- **Observability requirement:** what leaves, to which infrastructure,
  under which execution/feature; sync independent of local engines
  (INV-058).
- **Relevant PRIV/INV/ADR:** PRIV-003, PRIV-009; INV-011, INV-058; ADR-0014.

## FLOW-008 — Optional telemetry → our telemetry endpoint

- **Source:** local runtime measurements.
- **Destination:** AI Engineering OS telemetry endpoint.
- **Data involved:** product/usage measurements only. Prohibited by
  construction: source code, prompts, model outputs, secrets, raw
  filesystem contents (PRIV-006).
- **Default classification:** PUBLIC-compatible measurements only.
- **Purpose:** product improvement signals (future, optional).
- **Local or external:** LOCAL → OUR TELEMETRY, OFF BY DEFAULT
  (PRIV-002); core never depends on it (INV-014).
- **User action / feature causing flow:** explicit opt-in.
- **Policy/consent expectation:** opt-in; schema/minimization design is
  future implementation work — no event names or schema defined here.
- **Persistence at destination:** future telemetry retention — unresolved.
- **Observability requirement:** user-visible on/off state; payload
  categories reviewable against PRIV-006.
- **Relevant PRIV/INV/ADR:** PRIV-002, PRIV-006; INV-008, INV-014; ADR-0014.

## FLOW-009 — Browser / Web Acquisition → external internet

- **Source:** browser worker / Web Acquisition pipeline.
- **Destination:** general internet/web targets (fetched sites, web
  services) — separate from model-provider inference.
- **Data involved:** fetch requests/crawl budgets; fetched pages/snapshots
  back (untrusted even after cleaning); scoped session credentials where
  explicitly granted.
- **Default classification:** fetched content treated as untrusted external
  evidence; outbound requests carry only what the authorized acquisition
  requires.
- **Purpose:** web evidence for execution; interactive browser sessions.
- **Local or external:** EXTERNAL (general internet), SSRF-policed per hop.
- **User action / feature causing flow:** task step requiring web content;
  explicit user grant for authenticated private sites.
- **Policy/consent expectation:** centralized network/SSRF/policy
  evaluation before every hop and redirect; bounded budgets; no ambient
  host credential inheritance.
- **Persistence at destination:** provider/web-side retention not
  controlled by us; local snapshots carry provenance/freshness.
- **Observability requirement:** fetch snapshots with final URL, content
  hash, retrieval time; session credential scope visible.
- **Relevant PRIV/INV/ADR:** INV-097, INV-098, INV-099, INV-101, INV-103,
  INV-106; TB6.

## FLOW-010 — External document vision inference

- **Source:** local document/page representations.
- **Destination:** external vision provider (AI provider infrastructure).
- **Data involved:** page images/representations selected for inference.
- **Default classification:** source classification preserved (PRIVATE stays
  protected; RESTRICTED under explicit policy); derived output never less
  sensitive by itself.
- **Purpose:** OCR/extraction where native parsing is insufficient (when
  enabled).
- **Local or external:** EXTERNAL PROVIDER TRANSFER, implementation
  deferred — document processing is not promoted into v1.
- **User action / feature causing flow:** explicit enablement of external
  document inference for a document job.
- **Policy/consent expectation:** privacy-observable: execution metadata
  must explain which pages/representations left the device and which
  provider received them (INV-130).
- **Persistence at destination:** provider-owned retention — not controlled
  by us.
- **Observability requirement:** page/representation/provider-level
  transfer records; partial/degraded unit state explicit.
- **Relevant PRIV/INV/ADR:** PRIV-010; INV-117…130 (esp. INV-130);
  ADR-0014.

## FLOW-011 — Local device ↔ authorized remote worker (reserved, deferred)

- **Source:** local device (reserved; no payload defined).
- **Destination:** authorized remote worker (reserved; no protocol defined).
- **Data involved:** not defined — reserved distinction only.
- **Default classification:** unresolved.
- **Purpose:** future remote execution (later feature).
- **Local or external:** distinct from AI provider, our cloud, plugin and
  general web — by definition, not by protocol (no protocol defined here).
- **User action / feature causing flow:** none accepted yet.
- **Policy/consent expectation:** enrollment, transport and evidence rules
  unspecified.
- **Persistence at destination:** unspecified.
- **Observability requirement:** when defined, must meet the full
  what/whom/why/which-execution/classification standard of §37-equivalent
  observability.
- **Relevant PRIV/INV/ADR:** none yet; enrollment/transport are open
  security design items (see [`../security/THREAT_MODEL.md`](../security/THREAT_MODEL.md)).
- **Status in this map:** implementation deferred.

## FLOW-012 — Scoped plugin / integration transfer

- **Source:** application core.
- **Destination:** third-party plugin/integration.
- **Data involved:** explicitly scoped capability payloads only.
- **Default classification:** per payload; SECRET values never; audit
  content default-deny.
- **Purpose:** user-enabled integration capability.
- **Local or external:** EXTERNAL (third party), OPT-IN per integration.
- **User action / feature causing flow:** installation/enablement of a
  plugin or integration with explicit scopes.
- **Policy/consent expectation:** explicit capability/scope grant;
  least-necessary transfer; auditable flows. Full permission/data-sharing
  model unresolved (open design item).
- **Persistence at destination:** third-party retention — not controlled
  by us.
- **Observability requirement:** integration data flows auditable; payloads
  inspectable.
- **Relevant PRIV/INV/ADR:** PRIV-009 (opt-in principle); INV-011;
  ADR-0014.

## Flows deliberately absent

No flow exists for: provider-credential extraction (provider auth stays
provider-owned); SECRET in model context/prompts/logs/events/telemetry
(invalid under INV-008); automatic transcript upload to our cloud;
automatic crash-report upload; silent background export of any kind.
